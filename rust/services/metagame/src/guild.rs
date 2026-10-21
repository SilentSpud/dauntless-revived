//! Guilds (`controllers/guild.ts`): the 1.4.4 client's v2 guild API, stored so a guild survives
//! restarts and an invite waits for a player who is offline. Routes: `routes/guild.rs`.
//!
//! - Every reply is the Phoenix envelope `{"code", "message", "payload"}`. Success is a 2xx status
//!   AND a JSON body, so a success never answers 204. On an error status the client maps "code" to
//!   its EGuildRequestError; an unknown or empty code shows as "Unable to create guild."
//! - Replies that carry a guild or the invite list also copy the payload's fields to the root, in
//!   case the client reads them flat.
//! - A guild is created by the Ramsgate game server with the game-server key and the leader id the
//!   client sent, never that player's token; so a create is only accepted when the leader validated
//!   that same name and nameplate with their own token in the last 15 minutes.
//!   `GUILD_CREATE_ACTIVITY_FALLBACK=1` also accepts a leader heard from in the last minute.
//! - A block removes the guild invites between the two (`friends.rs`), and an invite whose inviter
//!   is no longer a Leader or Officer of the guild goes too.
//!
//! In memory only (losing them on a restart is harmless): validate tickets, the creation and invite
//! rate windows, and the pause on re-inviting a player who declined.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde_json::{Map, Value, json};
use service_kit::env::Env;

use crate::accounts::is_account_id_shape;
use crate::friends::{account_exists, blocked_either_way};
use crate::schema::{guildinvites, guildmembers, guilds};

pub type Conn = store::Connection;

/// The codes the server sends (`GUILD_CODE`), each an EGuildRequestError of the client.
pub mod code {
    pub const INVALID_PERMISSION: &str = "SlyAdorableQuillshot";
    pub const NOT_IN_A_GUILD: &str = "ExcludedAdorableQuillshot";
    pub const NAME_INVALID_LENGTH: &str = "ObedientAdorableQuillshot";
    pub const NAME_TAKEN: &str = "SeizedAdorableQuillshot";
    pub const NAME_PROFANE: &str = "NastyAdorableQuillshot";
    pub const NAME_TOO_MANY_NUMBERS: &str = "NumberedAdorableQuillshot";
    pub const NAME_TOO_MANY_LETTERS: &str = "LetteredAdorableQuillshot";
    pub const NAMEPLATE_INVALID_LENGTH: &str = "DutifulAdorableQuillshot";
    pub const NAMEPLATE_TAKEN: &str = "CapturedAdorableQuillshot";
    pub const NAMEPLATE_PROFANE: &str = "DirtyAdorableQuillshot";
    pub const ALREADY_IN_A_GUILD: &str = "OccupiedAdorableQuillshot";
    pub const TARGET_ALREADY_IN_YOUR_GUILD: &str = "ClonedAdorableQuillshot";
    pub const TARGET_ALREADY_HAS_GUILD_INVITE: &str = "RedundantAdorableQuillshot";
    pub const GUILD_IS_FULL: &str = "StuffedAdorableQuillshot";
    pub const INVITE_NOT_FOUND: &str = "UninvitedAdorableQuillshot";
    pub const LEADER_CANNOT_LEAVE: &str = "ChiefAdorableQuillshot";
    pub const INVALID_RANK: &str = "DocileAdorableQuillshot";
    pub const UNKNOWN: &str = "";
}

/// The client's own texts where it has one (the create-guild widget), else a short reason.
fn message(code: &str) -> &'static str {
    match code {
        code::INVALID_PERMISSION => "You do not have permission to do that.",
        code::NOT_IN_A_GUILD => "Not in that guild.",
        code::NAME_INVALID_LENGTH => "Name is invalid. Must contain 4-15 english letters and digits.",
        code::NAME_TAKEN => "Guild name already in use.",
        code::NAME_PROFANE => "Guild name contains profanity.",
        code::NAME_TOO_MANY_NUMBERS => "Guild name must have 6 numbers or less.",
        code::NAME_TOO_MANY_LETTERS => "Guild name must not have more than 6 of the same letter in a row.",
        code::NAMEPLATE_INVALID_LENGTH => "Nameplate is invalid. Must contain 2-6 english letters and digits.",
        code::NAMEPLATE_TAKEN => "Guild nameplate already in use.",
        code::NAMEPLATE_PROFANE => "Guild nameplate contains profanity.",
        code::ALREADY_IN_A_GUILD => "Unable to create guild. You are already in a guild.",
        code::TARGET_ALREADY_IN_YOUR_GUILD => "That player is already in your guild.",
        code::TARGET_ALREADY_HAS_GUILD_INVITE => "That player already has an invite from your guild.",
        code::GUILD_IS_FULL => "The guild is full.",
        code::INVITE_NOT_FOUND => "Guild invite not found.",
        code::LEADER_CANNOT_LEAVE => "The guild leader cannot leave the guild.",
        code::INVALID_RANK => "Invalid guild rank.",
        _ => "",
    }
}

pub const DEFAULT_MAX_MEMBERS: i64 = 100;
pub const DEFAULT_INVITE_TTL_DAYS: f64 = 7.0;
pub const MAX_OPEN_INVITES_PER_GUILD: i64 = 50;
pub const MAX_OPEN_INVITES_PER_INVITEE: usize = 20;
pub const MAX_INVITES_PER_INVITER_PER_HOUR: usize = 30;
pub const CREATE_INTERVAL_MS: i64 = 10 * 60 * 1000;
pub const VALIDATE_TICKET_MS: i64 = 15 * 60 * 1000;
pub const CREATE_ACTIVITY_MS: i64 = 60 * 1000;
/// Validated name and nameplate pairs kept per account, in case Create is pressed before the
/// validate of the final name has come back.
pub const MAX_TICKETS_PER_ACCOUNT: usize = 5;
/// After a player declines a guild's invite, that guild cannot invite them again for this long.
pub const DECLINE_COOLDOWN_MS: i64 = 24 * 60 * 60 * 1000;
const HOUR_MS: i64 = 60 * 60 * 1000;

// ---- Settings ----

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// `GUILDS=0` puts the old stubs back.
    pub enabled: bool,
    pub max_members: i64,
    pub invite_ttl_ms: i64,
    pub activity_fallback: bool,
    pub reserved_names: bool,
    /// `GUILD_NAME_DENYLIST`, already undone like the names it is compared with.
    pub extra_denylist: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_env(&Env::from_pairs([]))
    }
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let number = |name: &str| crate::js::number(env.raw(name).map(|v| json!(v)).as_ref());
        let max = number("GUILD_MAX_MEMBERS");
        let days = number("GUILD_INVITE_TTL_DAYS");
        let days = if days.is_finite() && days > 0.0 && days <= 365.0 {
            days
        } else {
            DEFAULT_INVITE_TTL_DAYS
        };
        Self {
            enabled: env.raw("GUILDS") != Some("0"),
            max_members: if max.fract() == 0.0 && (1.0..=10000.0).contains(&max) {
                max as i64
            } else {
                DEFAULT_MAX_MEMBERS
            },
            invite_ttl_ms: (days * 24.0 * 60.0 * 60.0 * 1000.0).round() as i64,
            activity_fallback: env.raw("GUILD_CREATE_ACTIVITY_FALLBACK") == Some("1"),
            reserved_names: env.raw("GUILD_RESERVED_NAMES") != Some("0"),
            extra_denylist: env
                .raw("GUILD_NAME_DENYLIST")
                .unwrap_or("")
                .split(',')
                .map(|word| unleet(word.trim()))
                .filter(|word| {
                    (2..=32).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
                .collect(),
        }
    }
}

// ---- In memory ----

#[derive(Debug, Clone)]
struct Ticket {
    name_key: String,
    nameplate_key: String,
    at: i64,
}

#[derive(Debug, Default)]
pub struct MemoryState {
    /// Per account, oldest first, at most [`MAX_TICKETS_PER_ACCOUNT`].
    tickets: HashMap<String, Vec<Ticket>>,
    last_created: HashMap<String, i64>,
    invites_sent: HashMap<String, Vec<i64>>,
    /// `<guildId>|<inviteeId>` -> when the invitee declined that guild's invite.
    declined: HashMap<String, i64>,
    last_sweep: i64,
}

#[derive(Debug, Default)]
pub struct Memory(Mutex<MemoryState>, std::sync::atomic::AtomicI64);

impl Memory {
    pub fn lock(&self) -> MutexGuard<'_, MemoryState> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `GuildNow`: the clock the guild routes use, the real time plus what tests moved it by.
    pub fn now(&self) -> i64 {
        crate::time::now_ms() + self.1.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Moves [`Memory::now`] on (Node's `SetGuildClockForTests`).
    pub fn advance_for_tests(&self, ms: i64) {
        self.1.fetch_add(ms, std::sync::atomic::Ordering::Relaxed);
    }

    /// Forgets the tickets and windows, as a restart does (`ResetGuildMemoryForTests`).
    pub fn reset_for_tests(&self) {
        *self.lock() = MemoryState::default();
    }
}

impl MemoryState {
    fn live_tickets(&self, account: &str, now: i64) -> Vec<Ticket> {
        self.tickets
            .get(account)
            .map(|list| {
                list.iter()
                    .filter(|t| now - t.at <= VALIDATE_TICKET_MS)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn recent_invites_by(&self, inviter: &str, now: i64) -> Vec<i64> {
        self.invites_sent
            .get(inviter)
            .map(|list| list.iter().copied().filter(|at| now - at < HOUR_MS).collect())
            .unwrap_or_default()
    }

    fn forget_old(&mut self, now: i64) {
        let accounts: Vec<String> = self.tickets.keys().cloned().collect();
        for account in accounts {
            let live = self.live_tickets(&account, now);
            if live.is_empty() {
                self.tickets.remove(&account);
            } else {
                self.tickets.insert(account, live);
            }
        }
        self.declined.retain(|_, at| now - *at < DECLINE_COOLDOWN_MS);
        self.last_created.retain(|_, at| now - *at <= CREATE_INTERVAL_MS);
        self.invites_sent.retain(|_, times| {
            times.retain(|at| now - at < HOUR_MS);
            !times.is_empty()
        });
    }

    /// Whether a sweep is due (at most once a minute); forgets old memory when it is.
    fn sweep_due(&mut self, now: i64) -> bool {
        if now - self.last_sweep < 60 * 1000 {
            return false;
        }
        self.last_sweep = now;
        self.forget_old(now);
        true
    }
}

/// `Sweep`'s database half: expired invites go.
pub fn delete_expired_invites(conn: &mut Conn, now: i64) -> QueryResult<()> {
    let removed = diesel::delete(guildinvites::table.filter(guildinvites::expiresAt.le(now))).execute(conn)?;
    if removed > 0 {
        tracing::info!("guild: removed {removed} expired invite(s)");
    }
    Ok(())
}

/// Whether the store sweep is due now (`Sweep` runs the database half only then).
pub fn sweep_due(memory: &Memory, now: i64) -> bool {
    memory.lock().sweep_due(now)
}

// ---- Replies ----

/// Status and body for the route; `None` answers no body (only GET /guild's "no guild").
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Option<Value>,
    pub code: Option<&'static str>,
}

fn envelope(payload: Value) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("code".into(), json!("OK"));
    body.insert("message".into(), json!(""));
    body.insert("payload".into(), payload);
    body
}

fn ack() -> Reply {
    Reply {
        status: 200,
        body: Some(Value::Object(envelope(json!({})))),
        code: None,
    }
}

/// Wrapped, plus flat copies of the payload's fields at the root.
fn with_payload(payload: Map<String, Value>) -> Reply {
    let mut body = envelope(Value::Object(payload.clone()));
    for (key, value) in payload {
        body.insert(key, value);
    }
    Reply {
        status: 200,
        body: Some(Value::Object(body)),
        code: None,
    }
}

fn refuse(status: u16, code: &'static str) -> Reply {
    refuse_with(status, code, message(code))
}

fn refuse_with(status: u16, code: &'static str, text: &str) -> Reply {
    Reply {
        status,
        body: Some(json!({"code": code, "message": text, "payload": {}})),
        code: Some(code),
    }
}

fn no_guild() -> Reply {
    Reply {
        status: 204,
        body: None,
        code: None,
    }
}

fn describe(reply: &Reply) -> String {
    if reply.status < 300 {
        return reply.status.to_string();
    }
    match reply.code.unwrap_or("") {
        "" => format!("{} (no code)", reply.status),
        code => format!("{} {code}", reply.status),
    }
}

/// A name as typed, safe for one log line.
pub fn quoted(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => {
            let cut: String = text.chars().take(32).collect();
            service_kit::jsjson::stringify(&json!(cut))
        }
        Some(Value::Null) => "<object>".into(),
        Some(Value::Bool(_)) => "<boolean>".into(),
        Some(Value::Number(_)) => "<number>".into(),
        Some(Value::Array(_) | Value::Object(_)) => "<object>".into(),
        None => "<undefined>".into(),
    }
}

/// `Quoted(Nameplate ?? "")`.
fn quoted_plate(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => quoted(Some(&json!(""))),
        other => quoted(other),
    }
}

fn shown_account(value: Option<&str>) -> &str {
    value
        .filter(|v| is_account_id_shape(v))
        .unwrap_or("<not an account id>")
}

// ---- Names and nameplates ----

const MAX_DIGITS: usize = 6;
const MAX_RUN: usize = 6;

/// A short list; `GUILD_NAME_DENYLIST` adds more. Compared after lowercasing and undoing common
/// digit swaps, as a substring.
const BUILT_IN_DENYLIST: [&str; 20] = [
    "fuck", "shit", "cunt", "bitch", "whore", "slut", "nigger", "nigga", "faggot", "retard", "nazi", "hitler", "penis",
    "vagina", "pussy", "dildo", "twat", "wank", "porn", "kike",
];
/// Offensive words too short for the substring list, refused only as the whole text ("kkk" also
/// anywhere).
const OFFENSIVE_WHOLE: [&str; 12] = [
    "kkk", "fag", "fags", "nig", "nigs", "fck", "fuk", "rape", "isis", "ss", "1488", "hh88",
];
const OFFENSIVE_ANYWHERE: [&str; 1] = ["kkk"];
/// Words that would let a guild pose as the server's staff or the project; they answer "already in
/// use".
const RESERVED_ANYWHERE: [&str; 7] = [
    "admin",
    "moderator",
    "official",
    "gamemaster",
    "staff",
    "dauntlessrevived",
    "phoenixlabs",
];
const RESERVED_WHOLE_NAME: [&str; 11] = [
    "dauntless",
    "phoenix",
    "revived",
    "support",
    "system",
    "server",
    "servers",
    "mods",
    "developer",
    "developers",
    "devteam",
];
const RESERVED_WHOLE_NAMEPLATE: [&str; 11] = [
    "gm", "gms", "dev", "devs", "mod", "mods", "sys", "phx", "dr", "drev", "undt",
];

fn unleet(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            other => other,
        })
        .collect()
}

pub fn is_profane(settings: &Settings, text: &str) -> bool {
    let lower = text.to_lowercase();
    let plain = unleet(text);
    BUILT_IN_DENYLIST.iter().any(|word| plain.contains(word))
        || settings.extra_denylist.iter().any(|word| plain.contains(word.as_str()))
        || OFFENSIVE_ANYWHERE.iter().any(|word| plain.contains(word))
        || OFFENSIVE_WHOLE.contains(&lower.as_str())
        || OFFENSIVE_WHOLE.contains(&plain.as_str())
}

pub fn is_reserved(settings: &Settings, text: &str, nameplate: bool) -> bool {
    if !settings.reserved_names {
        return false;
    }
    let whole: &[&str] = if nameplate {
        &RESERVED_WHOLE_NAMEPLATE
    } else {
        &RESERVED_WHOLE_NAME
    };
    [text.to_lowercase(), unleet(text)]
        .iter()
        .any(|form| RESERVED_ANYWHERE.iter().any(|word| form.contains(word)) || whole.contains(&form.as_str()))
}

fn longest_run(text: &str) -> usize {
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let (mut longest, mut run) = (0, 0);
    for index in 0..lower.len() {
        run = if index > 0 && lower[index] == lower[index - 1] {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
    }
    longest
}

fn alphanumeric(text: &str, lengths: std::ops::RangeInclusive<usize>) -> bool {
    lengths.contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric())
}

struct CheckedName {
    name: String,
    name_key: String,
    nameplate: String,
    nameplate_key: Option<String>,
}

/// The rules on the name alone, without the database.
fn check_name_rules(settings: &Settings, name: Option<&Value>) -> Option<Reply> {
    let Some(Value::String(name)) = name.filter(|n| matches!(n, Value::String(t) if alphanumeric(t, 4..=15))) else {
        return Some(refuse(400, code::NAME_INVALID_LENGTH));
    };
    if name.bytes().filter(u8::is_ascii_digit).count() > MAX_DIGITS {
        return Some(refuse(400, code::NAME_TOO_MANY_NUMBERS));
    }
    if longest_run(name) > MAX_RUN {
        return Some(refuse(400, code::NAME_TOO_MANY_LETTERS));
    }
    if is_profane(settings, name) {
        return Some(refuse(400, code::NAME_PROFANE));
    }
    if is_reserved(settings, name, false) {
        return Some(refuse(409, code::NAME_TAKEN));
    }
    None
}

/// An empty nameplate is allowed (the client skips it when empty).
fn plate_of(nameplate: Option<&Value>) -> Option<&Value> {
    match nameplate {
        None | Some(Value::Null) => None,
        other => other,
    }
}

/// The rules on the nameplate alone, without the database.
fn check_nameplate_rules(settings: &Settings, nameplate: Option<&Value>) -> Option<Reply> {
    let plate = match plate_of(nameplate) {
        None => return None,
        Some(Value::String(plate)) if plate.is_empty() => return None,
        Some(Value::String(plate)) if alphanumeric(plate, 2..=6) => plate,
        Some(_) => return Some(refuse(400, code::NAMEPLATE_INVALID_LENGTH)),
    };
    if is_profane(settings, plate) {
        return Some(refuse(400, code::NAMEPLATE_PROFANE));
    }
    if is_reserved(settings, plate, true) {
        return Some(refuse(409, code::NAMEPLATE_TAKEN));
    }
    None
}

/// The name and nameplate rules, in the order the first failure decides. The caller checks "already
/// in a guild" first.
fn check_name_and_nameplate(
    conn: &mut Conn,
    settings: &Settings,
    name: Option<&Value>,
    nameplate: Option<&Value>,
) -> QueryResult<Result<CheckedName, Reply>> {
    if let Some(refusal) = check_name_rules(settings, name) {
        return Ok(Err(refusal));
    }
    let name = name.and_then(Value::as_str).unwrap_or_default();
    let name_key = name.to_lowercase();
    let taken = guilds::table
        .filter(guilds::nameKey.eq(&name_key))
        .select(guilds::guildId)
        .first::<String>(conn)
        .optional()?;
    if taken.is_some() {
        return Ok(Err(refuse(409, code::NAME_TAKEN)));
    }
    if let Some(refusal) = check_nameplate_rules(settings, nameplate) {
        return Ok(Err(refusal));
    }
    let plate = plate_of(nameplate).and_then(Value::as_str).unwrap_or("");
    if plate.is_empty() {
        return Ok(Ok(CheckedName {
            name: name.to_owned(),
            name_key,
            nameplate: String::new(),
            nameplate_key: None,
        }));
    }
    let plate_key = plate.to_lowercase();
    let taken = guilds::table
        .filter(guilds::nameplateKey.eq(&plate_key))
        .select(guilds::guildId)
        .first::<String>(conn)
        .optional()?;
    if taken.is_some() {
        return Ok(Err(refuse(409, code::NAMEPLATE_TAKEN)));
    }
    Ok(Ok(CheckedName {
        name: name.to_owned(),
        name_key,
        nameplate: plate.to_owned(),
        nameplate_key: Some(plate_key),
    }))
}

/// The ticket key of a typed name and nameplate (lowercase; an empty nameplate is "").
fn ticket_keys(name: Option<&Value>, nameplate: Option<&Value>) -> Option<(String, String)> {
    let plate = match plate_of(nameplate) {
        None => "",
        Some(Value::String(plate)) => plate.as_str(),
        Some(_) => return None,
    };
    match name {
        Some(Value::String(name)) => Some((name.to_lowercase(), plate.to_lowercase())),
        _ => None,
    }
}

// ---- Reading ----

#[derive(Debug, Clone, Queryable)]
struct Membership {
    account_id: String,
    guild_id: String,
    rank: String,
    joined_at: i64,
    #[allow(dead_code)] // read with the row
    updated_at: i64,
}

fn rank_order(rank: &str) -> u8 {
    match rank {
        "Leader" => 0,
        "Officer" => 1,
        "Member" => 2,
        _ => 3,
    }
}

fn normalize_rank(rank: &str) -> Option<&'static str> {
    match rank.to_lowercase().as_str() {
        "member" => Some("Member"),
        "officer" => Some("Officer"),
        "leader" => Some("Leader"),
        _ => None,
    }
}

fn membership_of(conn: &mut Conn, account: &str) -> QueryResult<Option<Membership>> {
    guildmembers::table.find(account).first(conn).optional()
}

fn member_count(conn: &mut Conn, guild: &str) -> QueryResult<i64> {
    guildmembers::table
        .filter(guildmembers::guildId.eq(guild))
        .select(count_star())
        .first(conn)
}

/// The guild object of every guild reply (FGuildData / FGuildMemberData in the client).
fn guild_data(conn: &mut Conn, settings: &Settings, guild: &str) -> QueryResult<Option<Map<String, Value>>> {
    let row = guilds::table
        .find(guild)
        .select((guilds::guildId, guilds::name, guilds::nameplate, guilds::leaderId))
        .first::<(String, String, String, String)>(conn)
        .optional()?;
    let Some((id, name, nameplate, leader)) = row else {
        return Ok(None);
    };
    let mut members: Vec<Membership> = guildmembers::table.filter(guildmembers::guildId.eq(guild)).load(conn)?;
    members.sort_by(|a, b| {
        rank_order(&a.rank)
            .cmp(&rank_order(&b.rank))
            .then(a.joined_at.cmp(&b.joined_at))
            .then_with(|| service_kit::collate::locale_compare(&a.account_id, &b.account_id))
    });
    let mut data = Map::new();
    data.insert("id".into(), json!(id));
    data.insert("name".into(), json!(name));
    data.insert("nameplate".into(), json!(nameplate));
    data.insert("leader_account_id".into(), json!(leader));
    data.insert(
        "members".into(),
        Value::Array(
            members
                .iter()
                .map(|m| json!({"phx_account_id": m.account_id, "rank": m.rank}))
                .collect(),
        ),
    );
    data.insert(
        "maximum_guild_members".into(),
        json!(settings.max_members.max(members.len() as i64)),
    );
    Ok(Some(data))
}

fn live_invite_exists(conn: &mut Conn, guild: &str, invitee: &str, now: i64) -> QueryResult<bool> {
    Ok(guildinvites::table
        .filter(guildinvites::guildId.eq(guild))
        .filter(guildinvites::inviteeId.eq(invitee))
        .filter(guildinvites::expiresAt.gt(now))
        .select(guildinvites::inviteId)
        .first::<String>(conn)
        .optional()?
        .is_some())
}

/// GET /guild: the caller's guild, or 204 (no body) when there is none, which is how the client
/// learns "no guild".
pub fn own_guild(conn: &mut Conn, settings: &Settings, user: Option<&str>) -> QueryResult<Reply> {
    let Some(user) = user else {
        return Ok(no_guild());
    };
    let Some(membership) = membership_of(conn, user)? else {
        return Ok(no_guild());
    };
    Ok(guild_data(conn, settings, &membership.guild_id)?.map_or_else(no_guild, with_payload))
}

/// The inviter still leads or officers the invite's guild (an invite dies with that right).
fn inviter_may_invite(conn: &mut Conn, inviter: &str, guild: &str) -> QueryResult<bool> {
    Ok(membership_of(conn, inviter)?
        .is_some_and(|m| m.guild_id == guild && (m.rank == "Leader" || m.rank == "Officer")))
}

/// Invites sent by this account for this guild go (it was demoted to Member, kicked or left).
fn drop_invites_sent_by(conn: &mut Conn, inviter: &str, guild: &str) -> QueryResult<usize> {
    diesel::delete(
        guildinvites::table
            .filter(guildinvites::guildId.eq(guild))
            .filter(guildinvites::inviterId.eq(inviter)),
    )
    .execute(conn)
}

/// GET /guild/invite/player: the caller's open invites, newest first, leaving out invites between
/// blocked players and invites whose inviter lost the right to invite.
pub fn own_invites(conn: &mut Conn, user: Option<&str>, now: i64) -> QueryResult<Reply> {
    let mut invites = Vec::new();
    if let Some(user) = user {
        let rows: Vec<(String, String, String, String)> = guildinvites::table
            .inner_join(guilds::table.on(guilds::guildId.eq(guildinvites::guildId)))
            .filter(guildinvites::inviteeId.eq(user))
            .filter(guildinvites::expiresAt.gt(now))
            .order((guildinvites::createdAt.desc(), guildinvites::inviteId.asc()))
            .select((
                guildinvites::inviteId,
                guildinvites::guildId,
                guilds::name,
                guildinvites::inviterId,
            ))
            .load(conn)?;
        for (id, guild, name, inviter) in rows {
            if blocked_either_way(conn, user, &inviter)? || !inviter_may_invite(conn, &inviter, &guild)? {
                continue;
            }
            invites.push(json!({"id": id, "guild_id": guild, "guild_name": name, "inviter_account_id": inviter}));
        }
    }
    let mut payload = Map::new();
    payload.insert("invites".into(), Value::Array(invites));
    Ok(with_payload(payload))
}

// ---- Creating ----

/// POST /guild/validate's database half: `Ok` when the name and nameplate may be created.
pub fn validate_in_tx(
    conn: &mut Conn,
    settings: &Settings,
    user: &str,
    name: Option<&Value>,
    nameplate: Option<&Value>,
) -> QueryResult<Result<(String, String), Reply>> {
    if membership_of(conn, user)?.is_some() {
        return Ok(Err(refuse(409, code::ALREADY_IN_A_GUILD)));
    }
    Ok(check_name_and_nameplate(conn, settings, name, nameplate)?
        .map(|checked| (checked.name_key, checked.nameplate_key.unwrap_or_default())))
}

/// POST /guild/validate's memory half: logs the outcome and keeps a ticket for a name that passed.
pub fn validate_finish(
    memory: &Memory,
    user: &str,
    name: Option<&Value>,
    nameplate: Option<&Value>,
    checked: Result<(String, String), Reply>,
    now: i64,
) -> Reply {
    let shown = format!(
        "guild: validate by {user} name={} tag={}",
        quoted(name),
        quoted_plate(nameplate)
    );
    let (name_key, nameplate_key) = match checked {
        Err(refusal) => {
            tracing::info!("{shown} -> {}", describe(&refusal));
            return refusal;
        }
        Ok(keys) => keys,
    };
    let mut memory = memory.lock();
    let mut kept: Vec<Ticket> = memory
        .live_tickets(user, now)
        .into_iter()
        .filter(|t| t.name_key != name_key || t.nameplate_key != nameplate_key)
        .collect();
    kept.push(Ticket {
        name_key,
        nameplate_key,
        at: now,
    });
    let skip = kept.len().saturating_sub(MAX_TICKETS_PER_ACCOUNT);
    memory.tickets.insert(user.to_owned(), kept.split_off(skip));
    tracing::info!("{shown} -> ok");
    ack()
}

/// How a create is tied to the leader's own action (`CreateBinding`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Ticket,
    Fallback,
}

/// `CreateBinding`: `seen` is whether the leader was heard from in the last minute.
pub fn create_binding(
    memory: &Memory,
    settings: &Settings,
    leader: &str,
    name: Option<&Value>,
    nameplate: Option<&Value>,
    seen: bool,
    now: i64,
) -> (Option<Binding>, usize) {
    let live = memory.lock().live_tickets(leader, now);
    if let Some((name_key, plate_key)) = ticket_keys(name, nameplate)
        && live
            .iter()
            .any(|t| t.name_key == name_key && t.nameplate_key == plate_key)
    {
        return (Some(Binding::Ticket), live.len());
    }
    if settings.activity_fallback && (!live.is_empty() || seen) {
        return (Some(Binding::Fallback), live.len());
    }
    (None, live.len())
}

/// The refusal for a create no validate is tied to: a name the rules refuse anyway gets that rule's
/// own text; otherwise the plain failure.
pub fn unbound_create(settings: &Settings, name: Option<&Value>, nameplate: Option<&Value>) -> Reply {
    check_name_rules(settings, name)
        .or_else(|| check_nameplate_rules(settings, nameplate))
        .unwrap_or_else(|| {
            refuse_with(
                403,
                code::UNKNOWN,
                "Check the name in the create window, then try again.",
            )
        })
}

/// What creating a guild found.
pub enum Created {
    Done(Map<String, Value>),
    Refused(Reply),
}

/// POST /guild's database half: the checks and the new guild, inside one transaction. A unique
/// index refusing the insert (a name or nameplate taken in between) is "name taken".
pub fn create_in_tx(
    conn: &mut Conn,
    settings: &Settings,
    memory: &Memory,
    leader: &str,
    name: Option<&Value>,
    nameplate: Option<&Value>,
    now: i64,
) -> QueryResult<Created> {
    if membership_of(conn, leader)?.is_some() {
        return Ok(Created::Refused(refuse(409, code::ALREADY_IN_A_GUILD)));
    }
    let checked = match check_name_and_nameplate(conn, settings, name, nameplate)? {
        Ok(checked) => checked,
        Err(refusal) => return Ok(Created::Refused(refusal)),
    };
    if let Some(previous) = memory.lock().last_created.get(leader).copied()
        && now - previous < CREATE_INTERVAL_MS
    {
        return Ok(Created::Refused(refuse_with(
            429,
            code::UNKNOWN,
            "A guild was created a moment ago; try again later.",
        )));
    }
    let id = crate::crypto::random_uuid();
    let inserted = conn.transaction(|conn| {
        diesel::insert_into(guilds::table)
            .values((
                guilds::guildId.eq(&id),
                guilds::name.eq(&checked.name),
                guilds::nameKey.eq(&checked.name_key),
                guilds::nameplate.eq(&checked.nameplate),
                guilds::nameplateKey.eq(&checked.nameplate_key),
                guilds::leaderId.eq(leader),
                guilds::createdAt.eq(now),
                guilds::updatedAt.eq(now),
            ))
            .execute(conn)?;
        diesel::insert_into(guildmembers::table)
            .values((
                guildmembers::accountId.eq(leader),
                guildmembers::guildId.eq(&id),
                guildmembers::rank.eq("Leader"),
                guildmembers::joinedAt.eq(now),
                guildmembers::updatedAt.eq(now),
            ))
            .execute(conn)?;
        diesel::delete(guildinvites::table.filter(guildinvites::inviteeId.eq(leader))).execute(conn)
    });
    match inserted {
        Ok(_) => {}
        Err(DieselError::DatabaseError(
            kind @ (DatabaseErrorKind::UniqueViolation
            | DatabaseErrorKind::CheckViolation
            | DatabaseErrorKind::NotNullViolation),
            _,
        )) => {
            tracing::warn!("guild: create for {leader} refused by the database: {kind:?}");
            return Ok(Created::Refused(refuse(409, code::NAME_TAKEN)));
        }
        Err(error) => return Err(error),
    }
    Ok(match guild_data(conn, settings, &id)? {
        Some(data) => Created::Done(data),
        None => Created::Refused(refuse(409, code::NAME_TAKEN)),
    })
}

/// After a create: the pause starts and the leader's tickets go.
pub fn created(memory: &Memory, leader: &str, now: i64) {
    let mut memory = memory.lock();
    memory.last_created.insert(leader.to_owned(), now);
    memory.tickets.remove(leader);
}

pub fn created_reply(data: Map<String, Value>) -> Reply {
    with_payload(data)
}

pub fn create_refusal_log(
    leader: Option<&str>,
    name: Option<&Value>,
    nameplate: Option<&Value>,
    reply: &Reply,
    why: &str,
) {
    tracing::info!(
        "guild: create for {} name={} tag={} refused {}: {why}",
        shown_account(leader),
        quoted(name),
        quoted_plate(nameplate),
        describe(reply)
    );
}

pub fn no_such_account() -> Reply {
    refuse_with(400, code::UNKNOWN, "No such account.")
}

/// Whether the account exists (the create's leader).
pub fn leader_exists(conn: &mut Conn, leader: &str) -> QueryResult<bool> {
    account_exists(conn, leader)
}

// ---- Membership ----

fn remove_guild(conn: &mut Conn, guild: &str) -> QueryResult<usize> {
    let members = diesel::delete(guildmembers::table.filter(guildmembers::guildId.eq(guild))).execute(conn)?;
    diesel::delete(guildinvites::table.filter(guildinvites::guildId.eq(guild))).execute(conn)?;
    diesel::delete(guilds::table.find(guild)).execute(conn)?;
    Ok(members)
}

/// DELETE /guild/:guildId: the leader disbands their own guild.
pub fn disband(conn: &mut Conn, user: &str, guild: &str) -> QueryResult<Reply> {
    let reply = match membership_of(conn, user)? {
        Some(m) if m.guild_id == guild => {
            if m.rank != "Leader" {
                refuse(403, code::INVALID_PERMISSION)
            } else {
                remove_guild(conn, &m.guild_id)?;
                ack()
            }
        }
        _ => refuse(404, code::NOT_IN_A_GUILD),
    };
    let shown: String = guild.chars().take(64).collect();
    tracing::info!("guild: disband G={shown} by={user} -> {}", describe(&reply));
    Ok(reply)
}

/// PUT /guild/invite/:accountId's checks and insert; `Ok(reply, why)`.
pub fn invite_in_tx(
    conn: &mut Conn,
    settings: &Settings,
    memory: &Memory,
    user: &str,
    target: &str,
    now: i64,
) -> QueryResult<(Reply, &'static str)> {
    let Some(membership) = membership_of(conn, user)? else {
        return Ok((refuse(404, code::NOT_IN_A_GUILD), ""));
    };
    if membership.rank != "Leader" && membership.rank != "Officer" {
        return Ok((refuse(403, code::INVALID_PERMISSION), ""));
    }
    if !is_account_id_shape(target) || !account_exists(conn, target)? {
        return Ok((refuse_with(404, code::UNKNOWN, "No such account."), ""));
    }
    let guild = membership.guild_id;
    let theirs = membership_of(conn, target)?;
    if target == user || theirs.is_some_and(|t| t.guild_id == guild) {
        return Ok((refuse(409, code::TARGET_ALREADY_IN_YOUR_GUILD), ""));
    }
    // The same answer as the friends and party routes (403), with a message that does not say why
    if blocked_either_way(conn, user, target)? {
        return Ok((
            refuse_with(403, code::UNKNOWN, "The invite could not be sent."),
            "blocked",
        ));
    }
    if member_count(conn, &guild)? >= settings.max_members {
        return Ok((refuse(409, code::GUILD_IS_FULL), ""));
    }
    if live_invite_exists(conn, &guild, target, now)? {
        return Ok((refuse(409, code::TARGET_ALREADY_HAS_GUILD_INVITE), ""));
    }
    let (declined, recent) = {
        let memory = memory.lock();
        (
            memory.declined.get(&format!("{guild}|{target}")).copied(),
            memory.recent_invites_by(user, now).len(),
        )
    };
    if declined.is_some_and(|at| now - at < DECLINE_COOLDOWN_MS) {
        return Ok((
            refuse_with(
                429,
                code::UNKNOWN,
                "That player turned down an invite from this guild recently; try again later.",
            ),
            "declined this guild's invite in the last 24 hours",
        ));
    }
    let open: i64 = guildinvites::table
        .filter(guildinvites::guildId.eq(&guild))
        .filter(guildinvites::expiresAt.gt(now))
        .select(count_star())
        .first(conn)?;
    if open >= MAX_OPEN_INVITES_PER_GUILD {
        return Ok((
            refuse_with(429, code::UNKNOWN, "Too many open invites for this guild."),
            "",
        ));
    }
    if recent >= MAX_INVITES_PER_INVITER_PER_HOUR {
        return Ok((
            refuse_with(
                429,
                code::UNKNOWN,
                "Too many guild invites sent in the last hour; try again later.",
            ),
            "",
        ));
    }
    // An expired invite for the same pair gives way; the invitee keeps at most the newest 20
    diesel::delete(
        guildinvites::table
            .filter(guildinvites::guildId.eq(&guild))
            .filter(guildinvites::inviteeId.eq(target)),
    )
    .execute(conn)?;
    let pending: Vec<String> = guildinvites::table
        .filter(guildinvites::inviteeId.eq(target))
        .filter(guildinvites::expiresAt.gt(now))
        .order((guildinvites::createdAt.asc(), guildinvites::inviteId.asc()))
        .select(guildinvites::inviteId)
        .load(conn)?;
    let excess = pending.len().saturating_sub(MAX_OPEN_INVITES_PER_INVITEE - 1);
    for old in &pending[..excess] {
        diesel::delete(guildinvites::table.find(old)).execute(conn)?;
    }
    diesel::insert_into(guildinvites::table)
        .values((
            guildinvites::inviteId.eq(crate::crypto::random_uuid()),
            guildinvites::guildId.eq(&guild),
            guildinvites::inviteeId.eq(target),
            guildinvites::inviterId.eq(user),
            guildinvites::createdAt.eq(now),
            guildinvites::expiresAt.eq(now + settings.invite_ttl_ms),
        ))
        .execute(conn)?;
    Ok((ack(), ""))
}

/// After an invite: counted in the inviter's hour, and logged.
pub fn invite_finish(memory: &Memory, user: &str, target: &str, reply: &Reply, why: &str, now: i64) {
    if reply.status == 200 {
        let mut memory = memory.lock();
        let mut recent = memory.recent_invites_by(user, now);
        recent.push(now);
        memory.invites_sent.insert(user.to_owned(), recent);
    }
    tracing::info!(
        "guild: invite by={user} to={} -> {}{}",
        shown_account(Some(target)),
        describe(reply),
        if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        }
    );
}

#[derive(Debug, Clone, Queryable)]
struct InviteRow {
    invite_id: String,
    guild_id: String,
    invitee_id: String,
    inviter_id: String,
    #[allow(dead_code)] // read with the row
    created_at: i64,
    expires_at: i64,
}

fn invite_row(conn: &mut Conn, id: &str) -> QueryResult<Option<InviteRow>> {
    guildinvites::table.find(id).first(conn).optional()
}

/// POST /guild/invite/accept/:inviteId: the invitee joins as a Member; all their other invites go.
pub fn accept(conn: &mut Conn, settings: &Settings, user: &str, invite: &str, now: i64) -> QueryResult<Reply> {
    let (mut guild, mut why) = (String::new(), "");
    let reply = 'reply: {
        let Some(row) = invite_row(conn, invite)?.filter(|r| r.invitee_id == user && r.expires_at > now) else {
            break 'reply refuse(404, code::INVITE_NOT_FOUND);
        };
        guild = row.guild_id.clone();
        if membership_of(conn, user)?.is_some() {
            break 'reply refuse_with(
                409,
                code::ALREADY_IN_A_GUILD,
                "You need to leave your guild before accepting another guild invite.",
            );
        }
        // Gone with its guild, a block between the two, or an inviter no longer Leader or Officer
        let exists = guilds::table
            .find(&row.guild_id)
            .select(guilds::guildId)
            .first::<String>(conn)
            .optional()?
            .is_some();
        let stale = if !exists {
            Some("the guild is gone")
        } else if blocked_either_way(conn, user, &row.inviter_id)? {
            Some("blocked")
        } else if !inviter_may_invite(conn, &row.inviter_id, &row.guild_id)? {
            Some("the inviter may no longer invite")
        } else {
            None
        };
        if let Some(stale) = stale {
            why = stale;
            diesel::delete(guildinvites::table.find(&row.invite_id)).execute(conn)?;
            break 'reply refuse(404, code::INVITE_NOT_FOUND);
        }
        if member_count(conn, &row.guild_id)? >= settings.max_members {
            break 'reply refuse(409, code::GUILD_IS_FULL);
        }
        diesel::insert_into(guildmembers::table)
            .values((
                guildmembers::accountId.eq(user),
                guildmembers::guildId.eq(&row.guild_id),
                guildmembers::rank.eq("Member"),
                guildmembers::joinedAt.eq(now),
                guildmembers::updatedAt.eq(now),
            ))
            .execute(conn)?;
        diesel::delete(guildinvites::table.filter(guildinvites::inviteeId.eq(user))).execute(conn)?;
        ack()
    };
    tracing::info!(
        "guild: accept by={user}{} -> {}{}",
        if guild.is_empty() {
            String::new()
        } else {
            format!(" G={guild}")
        },
        describe(&reply),
        if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        }
    );
    Ok(reply)
}

/// DELETE /guild/invite/:inviteId: the invitee declines (an expired invite of theirs is simply
/// removed); the declined guild can't invite them again for [`DECLINE_COOLDOWN_MS`].
pub fn decline(conn: &mut Conn, memory: &Memory, user: &str, invite: &str, now: i64) -> QueryResult<Reply> {
    let reply = match invite_row(conn, invite)?.filter(|r| r.invitee_id == user) {
        None => refuse(404, code::INVITE_NOT_FOUND),
        Some(row) => {
            diesel::delete(guildinvites::table.find(&row.invite_id)).execute(conn)?;
            if row.expires_at > now {
                memory.lock().declined.insert(format!("{}|{user}", row.guild_id), now);
            }
            ack()
        }
    };
    tracing::info!("guild: decline by={user} -> {}", describe(&reply));
    Ok(reply)
}

fn leave_in_tx(conn: &mut Conn, user: &str, membership: &Membership) -> QueryResult<Reply> {
    if membership.rank == "Leader" {
        return Ok(refuse(409, code::LEADER_CANNOT_LEAVE));
    }
    diesel::delete(guildmembers::table.find(user)).execute(conn)?;
    drop_invites_sent_by(conn, user, &membership.guild_id)?;
    Ok(ack())
}

/// DELETE /guild/player: a Member or Officer leaves (the leader disbands, or hands over first).
pub fn leave(conn: &mut Conn, user: &str) -> QueryResult<Reply> {
    let reply = match membership_of(conn, user)? {
        None => refuse(404, code::NOT_IN_A_GUILD),
        Some(membership) => leave_in_tx(conn, user, &membership)?,
    };
    tracing::info!("guild: leave by={user} -> {}", describe(&reply));
    Ok(reply)
}

/// DELETE /guild/player/:accountId: the leader removes a member (naming oneself is a leave).
pub fn kick(conn: &mut Conn, user: &str, target: &str) -> QueryResult<Reply> {
    let reply = 'reply: {
        let Some(membership) = membership_of(conn, user)? else {
            break 'reply refuse(404, code::NOT_IN_A_GUILD);
        };
        if target == user {
            break 'reply leave_in_tx(conn, user, &membership)?;
        }
        if membership.rank != "Leader" {
            break 'reply refuse(403, code::INVALID_PERMISSION);
        }
        let theirs = if is_account_id_shape(target) {
            membership_of(conn, target)?
        } else {
            None
        };
        let Some(theirs) = theirs.filter(|t| t.guild_id == membership.guild_id) else {
            break 'reply refuse(404, code::NOT_IN_A_GUILD);
        };
        diesel::delete(guildmembers::table.find(&theirs.account_id)).execute(conn)?;
        drop_invites_sent_by(conn, &theirs.account_id, &membership.guild_id)?;
        ack()
    };
    tracing::info!(
        "guild: kick by={user} target={} -> {}",
        shown_account(Some(target)),
        describe(&reply)
    );
    Ok(reply)
}

/// PUT /guild/rank/:accountId/:rank (member, officer or leader, any case): leader only. Making
/// someone leader hands the guild over; the old leader becomes an Officer.
pub fn change_rank(conn: &mut Conn, user: &str, target: &str, rank: &str, now: i64) -> QueryResult<Reply> {
    let reply = 'reply: {
        let Some(membership) = membership_of(conn, user)? else {
            break 'reply refuse(404, code::NOT_IN_A_GUILD);
        };
        if membership.rank != "Leader" {
            break 'reply refuse(403, code::INVALID_PERMISSION);
        }
        let Some(rank) = normalize_rank(rank) else {
            break 'reply refuse(400, code::INVALID_RANK);
        };
        let theirs = if is_account_id_shape(target) {
            membership_of(conn, target)?
        } else {
            None
        };
        let Some(theirs) = theirs.filter(|t| t.guild_id == membership.guild_id) else {
            break 'reply refuse(404, code::NOT_IN_A_GUILD);
        };
        if theirs.account_id == user {
            break 'reply refuse(403, code::INVALID_PERMISSION);
        }
        if theirs.rank == rank {
            break 'reply ack();
        }
        diesel::update(guildmembers::table.find(&theirs.account_id))
            .set((guildmembers::rank.eq(rank), guildmembers::updatedAt.eq(now)))
            .execute(conn)?;
        // A Member may not invite: the invites they sent as an Officer go
        if rank == "Member" {
            drop_invites_sent_by(conn, &theirs.account_id, &membership.guild_id)?;
        }
        if rank == "Leader" {
            diesel::update(guildmembers::table.find(user))
                .set((guildmembers::rank.eq("Officer"), guildmembers::updatedAt.eq(now)))
                .execute(conn)?;
            diesel::update(guilds::table.find(&membership.guild_id))
                .set((guilds::leaderId.eq(&theirs.account_id), guilds::updatedAt.eq(now)))
                .execute(conn)?;
        }
        ack()
    };
    tracing::info!(
        "guild: rank by={user} target={} rank={} -> {}",
        shown_account(Some(target)),
        quoted(Some(&json!(rank))),
        describe(&reply)
    );
    Ok(reply)
}

// ---- The host's by-name fallbacks (`/undaunted/api`) ----

/// `GuildNameOf`: the name of the account's guild.
pub fn guild_name_of(conn: &mut Conn, user: &str) -> QueryResult<Option<String>> {
    let Some(membership) = membership_of(conn, user)? else {
        return Ok(None);
    };
    guilds::table
        .find(&membership.guild_id)
        .select(guilds::name)
        .first(conn)
        .optional()
}

/// `ListGuilds`: every guild, by name.
pub fn list_guilds(conn: &mut Conn) -> QueryResult<Vec<Value>> {
    let rows: Vec<(String, String, String, String)> = guilds::table
        .order(guilds::nameKey.asc())
        .select((guilds::guildId, guilds::name, guilds::nameplate, guilds::leaderId))
        .load(conn)?;
    rows.into_iter()
        .map(|(id, name, nameplate, leader)| {
            let members = member_count(conn, &id)?;
            Ok(json!({"guildId": id, "name": name, "nameplate": nameplate, "leader": leader, "members": members}))
        })
        .collect()
}

/// `DisbandGuildAsAdmin`: an admin removes a guild named by id or by name (any case); its name and
/// how many members it had.
pub fn disband_as_admin(conn: &mut Conn, guild: Option<&Value>) -> QueryResult<Option<(String, usize)>> {
    let Some(guild) = guild
        .and_then(Value::as_str)
        .filter(|g| (1..=64).contains(&g.encode_utf16().count()))
    else {
        return Ok(None);
    };
    conn.transaction(|conn| {
        let by_id = guilds::table
            .find(guild)
            .select((guilds::guildId, guilds::name))
            .first::<(String, String)>(conn)
            .optional()?;
        let row = match by_id {
            Some(row) => Some(row),
            None => guilds::table
                .filter(guilds::nameKey.eq(guild.to_lowercase()))
                .select((guilds::guildId, guilds::name))
                .first::<(String, String)>(conn)
                .optional()?,
        };
        let Some((id, name)) = row else {
            return Ok(None);
        };
        let members = remove_guild(conn, &id)?;
        tracing::info!("guild: G={id} name={name} disbanded by an admin ({members} member(s))");
        Ok(Some((name, members)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked_like_node() {
        let settings = Settings::default();
        let name = |n: &str| check_name_rules(&settings, Some(&json!(n))).and_then(|r| r.code);
        assert_eq!(name("Slayers"), None);
        assert_eq!(name("abc"), Some(code::NAME_INVALID_LENGTH));
        assert_eq!(name("a b c d"), Some(code::NAME_INVALID_LENGTH));
        assert_eq!(name("ab1234567"), Some(code::NAME_TOO_MANY_NUMBERS));
        assert_eq!(name("aaaaaaab"), Some(code::NAME_TOO_MANY_LETTERS));
        assert_eq!(name("Sh1tGuild"), Some(code::NAME_PROFANE));
        assert_eq!(name("TheAdmins"), Some(code::NAME_TAKEN));
        assert_eq!(name("Phoenix"), Some(code::NAME_TAKEN));
        assert_eq!(
            check_name_rules(&settings, Some(&json!(5))).and_then(|r| r.code),
            Some(code::NAME_INVALID_LENGTH)
        );
        let plate = |p: Value| check_nameplate_rules(&settings, Some(&p)).and_then(|r| r.code);
        assert_eq!(plate(json!("")), None);
        assert_eq!(plate(Value::Null), None);
        assert_eq!(plate(json!("ABC")), None);
        assert_eq!(plate(json!("ss")), Some(code::NAMEPLATE_PROFANE));
        assert_eq!(plate(json!("GM")), Some(code::NAMEPLATE_TAKEN));
        assert_eq!(plate(json!("toolong")), Some(code::NAMEPLATE_INVALID_LENGTH));
        assert_eq!(plate(json!(3)), Some(code::NAMEPLATE_INVALID_LENGTH));
    }
}
