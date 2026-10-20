//! Progression of real-mode accounts (`controllers/realprogression.ts`, `controllers/progressionrank.ts`).
//! Wire shapes are the 1.4.4 client's. The rules that matter:
//! - phx_account_id is always the account in the URL; the game server keys its cache by it, and a
//!   wrong one re-grants every rank reward from rank 0.
//! - Track progress in a grant request is an increment; every reply carries the new total.
//! - The backend grants no items. The game server grants rank rewards itself through /inventory from
//!   the difference between its cached track and our reply, and /confirm only records that ranks were
//!   claimed.

use diesel::prelude::*;
use serde_json::{Map, Value, json};
use service_kit::env::Env;

use crate::entitlements;
use crate::progression::{Caller, Event, account_exists, parse_path_integer, read_field, read_integer, record_event};
use crate::progression_config::{ProgressionConfig, ProgressionPath};
use crate::schema::{characters, huntpassselection, objectives, progress_tracks, progression_events, users};
use crate::time::{iso_now, now_ms};

pub type Conn = store::Connection;

pub const INT32_MAX: i64 = 2_147_483_647;
const DEFAULT_CONFIRMED_DATE: &str = "1970-01-01T00:00:00.000Z";
const DEFAULT_GRANT_CAP: i64 = 5000;
const RETRY_NOTE: &str = "retry of event";

/// The writes of an account's tracks, in progression_events.
const TRACK_WRITE_ROUTES: [&str; 5] = [
    "POST /progression/:uid",
    "POST /progression/:uid/:track/:amount",
    "POST /progression/:uid/:track/:rank/confirm/:kind",
    "DELETE /progression/:uid/:track",
    "admin SeedProgression",
];

/// The progression switches, read once at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `PROGRESSION_GRANT_CAP` (5000): the most one request may add to one track.
    pub grant_cap: i64,
    /// `PROGRESSION_REPLAY_WINDOW_S` (5, 0 = off): the retry guard of POST /progression/:uid.
    pub replay_window_s: i64,
    /// `PROGRESSION_CONFIRM_ENTITLEMENTS`: a confirm that raises a rank grants its permanent
    /// entitlements.
    pub confirm_entitlements: bool,
    /// `PROGRESSION_CONFIRM=off`: real-mode accounts get the old 404 on confirms.
    pub confirm_off: bool,
    /// `PROGRESSION_ALLOW_DELETE=1`: game servers may reset a track.
    pub allow_delete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_env(&Env::from_pairs([]))
    }
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        // `Number(x)`, then `Number.isInteger(Cap) && Cap > 0`
        let cap = crate::js::number(env.raw("PROGRESSION_GRANT_CAP").map(|v| json!(v)).as_ref());
        let replay_window_s = match env.get("PROGRESSION_REPLAY_WINDOW_S").map(str::trim) {
            None | Some("") => 5,
            Some(v) if (1..=9).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_digit()) => v.parse().unwrap_or(5),
            Some(v) => {
                tracing::warn!("PROGRESSION_REPLAY_WINDOW_S={v:?} is not a valid value; using the default (5s)");
                5
            }
        };
        Self {
            grant_cap: if cap.fract() == 0.0 && cap > 0.0 && cap.is_finite() {
                cap as i64
            } else {
                DEFAULT_GRANT_CAP
            },
            replay_window_s,
            confirm_entitlements: crate::config::on_off(env, "PROGRESSION_CONFIRM_ENTITLEMENTS", false),
            confirm_off: env.raw("PROGRESSION_CONFIRM") == Some("off"),
            allow_delete: env.raw("PROGRESSION_ALLOW_DELETE") == Some("1"),
        }
    }
}

/// What a real-mode write needs besides the connection.
#[derive(Debug, Clone)]
pub struct Context {
    pub settings: Settings,
    pub config: std::sync::Arc<ProgressionConfig>,
    pub entitlement_defaults: Vec<String>,
}

/// `RealReply`: a status and maybe a JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Option<Value>,
}

impl Reply {
    fn status(status: u16) -> Self {
        Self { status, body: None }
    }

    fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body: Some(body),
        }
    }
}

// ---- Rank math (`progressionrank.ts`, the client's 0x140b40fa0) ----

/// `ComputeEarnedRanks`: requirements walked in array order, like the client; prestige levels after
/// the last rank. Returns the earned free and premium ranks.
pub fn earned_ranks(path: &ProgressionPath, progress: i64, premium: bool) -> (i64, i64) {
    let mut cumulative: i64 = 0;
    let mut free = 0;
    let mut paid = 0;
    let mut reached_end = true;
    for requirement in &path.requirements {
        cumulative += requirement.xp_required as i64;
        if progress < cumulative {
            reached_end = false;
            break;
        }
        free = requirement.rank_id as i64;
        if premium {
            paid = requirement.rank_id as i64;
        }
    }
    if reached_end && let Some(per_level) = path.prestige_xp_per_level.filter(|xp| *xp > 0) {
        let excess = progress - cumulative;
        if excess > 0 {
            free += excess.div_euclid(per_level as i64);
            if premium {
                paid = free;
            }
        }
    }
    (free, paid)
}

/// `TotalXpToMaxRank`.
pub fn total_xp_to_max_rank(path: &ProgressionPath) -> i64 {
    path.requirements.iter().map(|r| r.xp_required as i64).sum()
}

/// `MaxRankId`.
pub fn max_rank_id(path: &ProgressionPath) -> i64 {
    path.requirements
        .iter()
        .map(|r| r.rank_id as i64)
        .max()
        .unwrap_or(0)
        .max(0)
}

// ---- Records ----

#[derive(Debug, Clone, Queryable)]
#[allow(non_snake_case, dead_code)]
pub struct TrackRow {
    accountId: String,
    progressionId: String,
    pub progress: i64,
    pub confirmedFreeRank: i64,
    pub confirmedPremiumRank: i64,
    pub confirmedDate: String,
    updatedDate: String,
}

#[derive(Debug, Clone, Queryable)]
#[allow(non_snake_case, dead_code)]
struct ObjectiveRow {
    accountId: String,
    objectiveId: String,
    progress: i64,
    completedCount: i64,
    createdDate: String,
    lastModifiedDate: String,
}

/// `ToTrackRecord`.
pub fn track_record(account: &str, id: &str, row: Option<&TrackRow>) -> Value {
    json!({
        "phx_account_id": account,
        "progression_id": id,
        "progress": row.map_or(0, |r| r.progress),
        "confirmed_fremium_rank": row.map_or(0, |r| r.confirmedFreeRank),
        "confirmed_premium_rank": row.map_or(0, |r| r.confirmedPremiumRank),
        "confirmed_date": row.map_or(DEFAULT_CONFIRMED_DATE, |r| r.confirmedDate.as_str()),
    })
}

fn objective_record(account: &str, row: &ObjectiveRow) -> Value {
    json!({
        "phx_account_id": account,
        "objective_id": row.objectiveId,
        "progress": row.progress,
        "completed_count": row.completedCount,
        "created_date": row.createdDate,
        "last_modified_date": row.lastModifiedDate,
    })
}

fn read_track(conn: &mut Conn, account: &str, id: &str) -> QueryResult<Option<TrackRow>> {
    progress_tracks::table.find((account, id)).first(conn).optional()
}

fn write_track(
    conn: &mut Conn,
    account: &str,
    id: &str,
    (progress, free, premium, date): (i64, i64, i64, &str),
) -> QueryResult<TrackRow> {
    let updated = iso_now();
    diesel::insert_into(progress_tracks::table)
        .values((
            progress_tracks::accountId.eq(account),
            progress_tracks::progressionId.eq(id),
            progress_tracks::progress.eq(progress),
            progress_tracks::confirmedFreeRank.eq(free),
            progress_tracks::confirmedPremiumRank.eq(premium),
            progress_tracks::confirmedDate.eq(date),
            progress_tracks::updatedDate.eq(&updated),
        ))
        .on_conflict((progress_tracks::accountId, progress_tracks::progressionId))
        .do_update()
        .set((
            progress_tracks::progress.eq(progress),
            progress_tracks::confirmedFreeRank.eq(free),
            progress_tracks::confirmedPremiumRank.eq(premium),
            progress_tracks::confirmedDate.eq(date),
            progress_tracks::updatedDate.eq(&updated),
        ))
        .get_result(conn)
}

fn earned_free(config: &ProgressionConfig, id: &str, progress: i64) -> Option<i64> {
    config.path(id).map(|path| earned_ranks(path, progress, false).0)
}

fn warn_notes(what: &str, notes: &[String]) {
    if !notes.is_empty() {
        tracing::warn!("{what}: {}", notes.join("; "));
    }
}

fn event(
    conn: &mut Conn,
    account: &str,
    caller: Caller,
    route: &str,
    body: Option<&Value>,
    reply: &Reply,
    notes: &[String],
) -> QueryResult<()> {
    record_event(
        conn,
        Event {
            account,
            caller: caller.as_str(),
            route,
            body,
            status: i64::from(reply.status),
            reply: reply.body.as_ref(),
            notes,
        },
    )
}

/// `GetTrackRecords`: every configured track (stored, or at 0), then any stored track the config
/// doesn't know.
pub fn track_records(conn: &mut Conn, config: &ProgressionConfig, account: &str) -> QueryResult<Vec<Value>> {
    let rows: Vec<TrackRow> = progress_tracks::table
        .filter(progress_tracks::accountId.eq(account))
        .load(conn)?;
    // With two rows for one track the last wins, as a JavaScript Map built from them
    let stored = |id: &str| rows.iter().rev().find(|row| row.progressionId == id);
    let mut records: Vec<Value> = config
        .paths
        .iter()
        .map(|path| track_record(account, &path.id, stored(&path.id)))
        .collect();
    for row in &rows {
        if config.path(&row.progressionId).is_none() {
            records.push(track_record(account, &row.progressionId, Some(row)));
        }
    }
    Ok(records)
}

/// `GetTrackRecord`: `None` for a track that is neither configured nor stored.
pub fn track_record_of(
    conn: &mut Conn,
    config: &ProgressionConfig,
    account: &str,
    id: &str,
) -> QueryResult<Option<Value>> {
    let row = read_track(conn, account, id)?;
    if row.is_none() && config.path(id).is_none() {
        return Ok(None);
    }
    Ok(Some(track_record(account, id, row.as_ref())))
}

/// `GetObjectiveRecords`, by objective id.
pub fn objective_records(conn: &mut Conn, account: &str) -> QueryResult<Vec<Value>> {
    let rows: Vec<ObjectiveRow> = objectives::table
        .filter(objectives::accountId.eq(account))
        .order(objectives::objectiveId.asc())
        .load(conn)?;
    Ok(rows.iter().map(|row| objective_record(account, row)).collect())
}

/// `GetObjectiveRecord`: the stored objective, or zeros (never the stub's 9,999,999).
pub fn objective_record_of(conn: &mut Conn, account: &str, id: &str) -> QueryResult<Value> {
    let row: Option<ObjectiveRow> = objectives::table.find((account, id)).first(conn).optional()?;
    Ok(match row {
        Some(row) => objective_record(account, &row),
        None => {
            let now = iso_now();
            json!({
                "phx_account_id": account,
                "objective_id": id,
                "progress": 0,
                "completed_count": 0,
                "created_date": now,
                "last_modified_date": now,
            })
        }
    })
}

// ---- Grants ----

struct ParsedGrant {
    /// Increments by track, in first-seen order.
    tracks: Vec<(String, i64)>,
    /// Absolute values by objective (the last one for an id wins), in first-seen order.
    objectives: Vec<(String, i64, Option<i64>)>,
}

fn is_plain_object(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Object(_)))
}

/// `ParseGrantBody`: `{progress_tracks: [{progression_id, progress}], objectives: [{objective_id,
/// value, completed_count}]}`, both arrays always present in what the game server sends.
fn parse_grant(body: Option<&Value>, cap: i64, notes: &mut Vec<String>) -> Option<ParsedGrant> {
    if !is_plain_object(body) {
        notes.push("body is not a JSON object".into());
        return None;
    }
    let empty = json!([]);
    let tracks = body.and_then(|b| b.get("progress_tracks")).unwrap_or_else(|| {
        notes.push("no progress_tracks key".into());
        &empty
    });
    let objective_list = body.and_then(|b| b.get("objectives")).unwrap_or_else(|| {
        notes.push("no objectives key".into());
        &empty
    });
    let (Some(tracks), Some(objective_list)) = (tracks.as_array(), objective_list.as_array()) else {
        notes.push("progress_tracks or objectives is not an array".into());
        return None;
    };
    let mut parsed = ParsedGrant {
        tracks: Vec::new(),
        objectives: Vec::new(),
    };
    for entry in tracks {
        let id = read_field(Some(entry), &["progression_id", "progressionId"], "track id", notes);
        let amount = read_field(Some(entry), &["progress", "amount", "value"], "track amount", notes);
        let amount = read_integer(amount, "track amount", notes);
        let (Some(id), Some(amount)) = (id.and_then(Value::as_str).filter(|s| !s.is_empty()), amount) else {
            notes.push(format!("skipped track entry {}", service_kit::jsjson::stringify(entry)));
            continue;
        };
        let mut clamped = amount;
        if clamped < 0 {
            notes.push(format!("{id} amount {amount} is negative, counted as 0"));
            clamped = 0;
        }
        match parsed.tracks.iter_mut().find(|(t, _)| t == id) {
            Some((_, total)) => *total += clamped,
            None => parsed.tracks.push((id.to_owned(), clamped)),
        }
    }
    // The cap is per track and request, so entries repeating one track can't add it twice
    for (id, total) in &mut parsed.tracks {
        if *total > cap {
            notes.push(format!("{id} total {total} is over the cap of {cap}, cut to {cap}"));
            *total = cap;
        }
    }
    for entry in objective_list {
        let id = read_field(Some(entry), &["objective_id", "objectiveId"], "objective id", notes);
        let value = read_field(Some(entry), &["value", "progress"], "objective value", notes);
        let value = read_integer(value, "objective value", notes);
        let completed = read_field(
            Some(entry),
            &["completed_count", "completedCount"],
            "objective completed_count",
            notes,
        );
        let completed = read_integer(completed, "objective completed_count", notes);
        let (Some(id), Some(value)) = (id.and_then(Value::as_str).filter(|s| !s.is_empty()), value) else {
            notes.push(format!(
                "skipped objective entry {}",
                service_kit::jsjson::stringify(entry)
            ));
            continue;
        };
        if completed.is_none() {
            notes.push(format!(
                "objective {id} has no usable completed_count, kept the stored one"
            ));
        }
        match parsed.objectives.iter_mut().find(|(o, _, _)| o == id) {
            Some(slot) => *slot = (id.to_owned(), value, completed),
            None => parsed.objectives.push((id.to_owned(), value, completed)),
        }
    }
    Some(parsed)
}

fn clamp32(value: i64) -> i64 {
    value.clamp(-2_147_483_648, INT32_MAX)
}

#[derive(Queryable)]
#[allow(dead_code)]
struct EventRow {
    id: i64,
    time: String,
    route: String,
    status: i64,
    body: Option<String>,
    reply: Option<String>,
    note: Option<String>,
}

fn event_columns() -> (
    progression_events::id,
    progression_events::time,
    progression_events::route,
    progression_events::status,
    progression_events::body,
    progression_events::reply,
    progression_events::note,
) {
    (
        progression_events::id,
        progression_events::time,
        progression_events::route,
        progression_events::status,
        progression_events::body,
        progression_events::reply,
        progression_events::note,
    )
}

/// The retry guard of POST /progression/:uid. The game server retries a request whose answer it
/// didn't get, and a retried grant would add its XP twice. A grant is taken for such a retry when the
/// account's last track write was a grant with the same body (the same JSON), applied less than the
/// window ago, or a retry of one: it gets that grant's stored reply and adds nothing. A confirm or any
/// other grant in between means the game server had the answer. The window stays clearly under the
/// game server's 10 s grant flush (`QueuedGrantTimeout`); the idea is Harmonic's
/// (github.com/Harmonicrain/Undaunted 895f7c7).
fn find_retried_grant(
    conn: &mut Conn,
    window: i64,
    account: &str,
    route: &str,
    body: Option<&Value>,
) -> QueryResult<Option<(Value, f64, String)>> {
    if window == 0 {
        return Ok(None);
    }
    let last: Option<EventRow> = progression_events::table
        .filter(progression_events::accountId.eq(account))
        .filter(progression_events::route.eq_any(TRACK_WRITE_ROUTES))
        .order(progression_events::id.desc())
        .select(event_columns())
        .first(conn)
        .optional()?;
    let Some(last) = last else { return Ok(None) };
    let same_body = body.map(service_kit::jsjson::stringify);
    if last.route != route
        || last.status != 200
        || last.reply.is_none()
        || last.body != same_body
        || same_body.is_none()
    {
        return Ok(None);
    }
    // An answered retry names the grant it repeated; the window runs from that grant
    // `^retry of event (\d+) `
    let retry_of = last
        .note
        .as_deref()
        .and_then(|note| note.strip_prefix(RETRY_NOTE)?.strip_prefix(' '))
        .and_then(|rest| {
            let digits = rest.split(' ').next()?;
            (rest.len() > digits.len() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                .then(|| digits.to_owned())
        });
    let original = match retry_of {
        None => Some(last),
        Some(id) => progression_events::table
            .filter(progression_events::id.eq(id.parse::<i64>().unwrap_or(-1)))
            .filter(progression_events::accountId.eq(account))
            .select(event_columns())
            .first(conn)
            .optional()?,
    };
    let Some(original) = original else { return Ok(None) };
    let Some(reply) = original.reply.as_deref() else {
        return Ok(None);
    };
    let Some(at) = crate::js::date_parse(&original.time) else {
        return Ok(None);
    };
    let age = now_ms() - at;
    if !(age >= 0 && age < window * 1000) {
        return Ok(None);
    }
    let reply = service_kit::jsjson::parse(reply).unwrap_or(Value::Null);
    let seconds = (age as f64 / 100.0).round() / 10.0;
    Ok(Some((
        reply,
        seconds,
        format!(
            "{RETRY_NOTE} {} within {window} s: its reply, nothing added",
            original.id
        ),
    )))
}

/// `GrantProgression`, POST /progression/:uid (game server only). Reply: one track per distinct
/// requested track with the new total and the stored confirmed ranks, and every requested objective as
/// stored. Confirmed ranks are not raised here: the game server confirms them after granting rewards.
pub fn grant(conn: &mut Conn, ctx: &Context, account: &str, body: Option<&Value>, who: Caller) -> QueryResult<Reply> {
    let route = "POST /progression/:uid";
    let mut notes = Vec::new();
    let parsed = parse_grant(body, ctx.settings.grant_cap, &mut notes);
    if !account_exists(conn, account)? {
        notes.push("unknown account".into());
        let reply = Reply::status(404);
        event(conn, account, who, route, body, &reply, &notes)?;
        tracing::warn!("Refusing progression grant for unknown account {account}");
        return Ok(reply);
    }
    let Some(parsed) = parsed else {
        let reply = Reply::status(400);
        event(conn, account, who, route, body, &reply, &notes)?;
        warn_notes(&format!("Refusing progression grant for {account}"), &notes);
        return Ok(reply);
    };
    if let Some((stored, seconds, note)) = find_retried_grant(conn, ctx.settings.replay_window_s, account, route, body)?
    {
        let reply = Reply::ok(stored);
        let mut all = vec![note];
        all.extend(notes);
        event(conn, account, who, route, body, &reply, &all)?;
        tracing::warn!(
            "Progression grant for {account} repeats the grant of {} s ago: answered its stored reply, nothing added (PROGRESSION_REPLAY_WINDOW_S={})",
            service_kit::jsjson::number(seconds),
            ctx.settings.replay_window_s
        );
        return Ok(reply);
    }
    let now = iso_now();
    let mut track_replies = Vec::new();
    let mut summary = Vec::new();
    for (id, amount) in &parsed.tracks {
        let current = read_track(conn, account, id)?;
        let old = current.as_ref().map_or(0, |c| c.progress);
        let new = (old + amount).min(INT32_MAX);
        if ctx.config.path(id).is_none() {
            notes.push(format!("{id} is not in the progression config, stored anyway"));
        }
        let written = write_track(
            conn,
            account,
            id,
            (
                new,
                current.as_ref().map_or(0, |c| c.confirmedFreeRank),
                current.as_ref().map_or(0, |c| c.confirmedPremiumRank),
                current
                    .as_ref()
                    .map_or(DEFAULT_CONFIRMED_DATE, |c| c.confirmedDate.as_str()),
            ),
        )?;
        track_replies.push(track_record(account, id, Some(&written)));
        let ranks = match (earned_free(&ctx.config, id, old), earned_free(&ctx.config, id, new)) {
            (Some(before), Some(after)) if after > before => format!(" (rank {before} -> {after})"),
            _ => String::new(),
        };
        summary.push(format!("{id} +{amount} -> {new}{ranks}"));
    }
    let mut objective_replies = Vec::new();
    for (id, value, completed) in &parsed.objectives {
        let current: Option<ObjectiveRow> = objectives::table.find((account, id)).first(conn).optional()?;
        let completed = clamp32(completed.or(current.as_ref().map(|c| c.completedCount)).unwrap_or(0));
        let progress = clamp32(*value);
        // Only noted: objective values are absolute and stored as sent. A repeatable objective starts
        // again at a lower value when its completed_count goes up, which is not "backwards".
        if let Some(current) = &current
            && progress < current.progress
            && completed <= current.completedCount
        {
            notes.push(format!(
                "objective {id} went backwards ({}/{} -> {progress}/{completed}), stored as sent",
                current.progress, current.completedCount
            ));
            tracing::warn!(
                "progression: objective went backwards: {id} of {account} from {} (completed {}) to {progress} (completed {completed}); stored as sent",
                current.progress,
                current.completedCount
            );
        }
        let written: ObjectiveRow = diesel::insert_into(objectives::table)
            .values((
                objectives::accountId.eq(account),
                objectives::objectiveId.eq(id),
                objectives::progress.eq(progress),
                objectives::completedCount.eq(completed),
                objectives::createdDate.eq(&now),
                objectives::lastModifiedDate.eq(&now),
            ))
            .on_conflict((objectives::accountId, objectives::objectiveId))
            .do_update()
            .set((
                objectives::progress.eq(progress),
                objectives::completedCount.eq(completed),
                objectives::lastModifiedDate.eq(&now),
            ))
            .get_result(conn)?;
        objective_replies.push(objective_record(account, &written));
    }
    let count = objective_replies.len();
    let reply = Reply::ok(json!({
        "code": "OK",
        "message": "OK",
        "payload": {"progress_tracks": track_replies, "objectives": objective_replies}
    }));
    event(conn, account, who, route, body, &reply, &notes)?;
    warn_notes(&format!("Progression grant for {account}"), &notes);
    tracing::info!(
        "Progression grant for {account}: {}; {count} objective(s)",
        if summary.is_empty() {
            "no tracks".to_owned()
        } else {
            summary.join(", ")
        }
    );
    Ok(reply)
}

/// `GrantProgressionInTrack`, POST /progression/:uid/:track/:amount (game server; quest and tutorial
/// rewards). No body.
pub fn grant_in_track(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    id: &str,
    raw: &str,
    who: Caller,
) -> QueryResult<Reply> {
    let route = "POST /progression/:uid/:track/:amount";
    let amount = parse_path_integer(raw, true);
    if !account_exists(conn, account)? {
        let reply = Reply::status(404);
        event(
            conn,
            account,
            who,
            route,
            None,
            &reply,
            &[format!("track {id} amount {raw}"), "unknown account".into()],
        )?;
        tracing::warn!("Refusing progression grant for unknown account {account}");
        return Ok(reply);
    }
    let Some(amount) = amount else {
        let reply = Reply::status(400);
        event(
            conn,
            account,
            who,
            route,
            None,
            &reply,
            &[format!("track {id}"), format!("amount {raw} is not an integer")],
        )?;
        tracing::warn!("Refusing progression grant for {account} in {id}: amount {raw} is not an integer");
        return Ok(reply);
    };
    let cap = ctx.settings.grant_cap;
    let mut notes = Vec::new();
    let mut clamped = amount;
    if clamped < 0 {
        notes.push(format!("amount {amount} is negative, counted as 0"));
        clamped = 0;
    }
    if clamped > cap {
        notes.push(format!("amount {amount} is over the cap of {cap}, cut to {cap}"));
        clamped = cap;
    }
    if ctx.config.path(id).is_none() {
        notes.push(format!("{id} is not in the progression config, stored anyway"));
    }
    let current = read_track(conn, account, id)?;
    let old = current.as_ref().map_or(0, |c| c.progress);
    let written = write_track(
        conn,
        account,
        id,
        (
            (old + clamped).min(INT32_MAX),
            current.as_ref().map_or(0, |c| c.confirmedFreeRank),
            current.as_ref().map_or(0, |c| c.confirmedPremiumRank),
            current
                .as_ref()
                .map_or(DEFAULT_CONFIRMED_DATE, |c| c.confirmedDate.as_str()),
        ),
    )?;
    let reply = Reply::ok(json!({"code": "OK", "message": "OK", "payload": track_record(account, id, Some(&written))}));
    let mut all = vec![format!("track {id} amount {raw}")];
    all.extend(notes.iter().cloned());
    event(conn, account, who, route, None, &reply, &all)?;
    warn_notes(&format!("Progression grant in {id} for {account}"), &notes);
    tracing::info!(
        "Progression grant for {account}: {id} +{clamped} -> {}",
        written.progress
    );
    Ok(reply)
}

/// `GrantRankEntitlements` (`PROGRESSION_CONFIRM_ENTITLEMENTS=1` only): the permanent entitlements the
/// config lists for the ranks a confirm has just raised (above `from`, up to `to`) on one reward list.
#[allow(clippy::too_many_arguments)]
fn grant_rank_entitlements(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    path: &ProgressionPath,
    list: &str,
    from: i64,
    to: i64,
    notes: &mut Vec<String>,
) -> QueryResult<()> {
    for reward in path.json.get(list).and_then(Value::as_array).into_iter().flatten() {
        let Some(rank) = crate::js::safe_integer(reward.get("rank_id")) else {
            continue;
        };
        let Some(entitlements) = reward.get("entitlements").and_then(Value::as_array) else {
            continue;
        };
        if rank <= from || rank > to {
            continue;
        }
        for entitlement in entitlements {
            let Some(name) = entitlement
                .get("entitlement")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            if !crate::js::strict_eq(entitlement.get("duration"), Some(&json!(0))) {
                notes.push(format!(
                    "rank {rank} {list}: {name} ({} h) is timed, not granted on confirm",
                    crate::js::string(entitlement.get("duration"))
                ));
                continue;
            }
            entitlements::grant(
                conn,
                &ctx.entitlement_defaults,
                account,
                name,
                0,
                &format!("confirm:{}:{rank}", path.id),
            )?;
            notes.push(format!(
                "rank {rank} {list}: granted entitlement {name} (PROGRESSION_CONFIRM_ENTITLEMENTS=1)"
            ));
        }
    }
    Ok(())
}

/// `ConfirmRank`, POST /progression/:uid/:track/:rank/confirm/{public|premium} (game server, no body):
/// raises the confirmed rank to min(rank, earned); never lowers it, never grants items. Premium needs
/// the track's premium_gating_entitlement.
pub fn confirm(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    id: &str,
    raw: &str,
    kind: &str,
    who: Caller,
) -> QueryResult<Reply> {
    let route = "POST /progression/:uid/:track/:rank/confirm/:kind";
    let context = format!("track {id} rank {raw} {kind}");
    let rank = parse_path_integer(raw, false);
    let path = ctx.config.path(id);
    let refuse = |conn: &mut Conn, status: u16, why: &str| -> QueryResult<Reply> {
        let reply = Reply::status(status);
        event(
            conn,
            account,
            who,
            route,
            None,
            &reply,
            &[context.clone(), why.to_owned()],
        )?;
        tracing::warn!("Refusing confirm of {context} for {account}: {why}");
        Ok(reply)
    };
    if kind != "public" && kind != "premium" {
        return refuse(conn, 400, "kind is not public or premium");
    }
    let Some(rank) = rank else {
        return refuse(conn, 400, "rank is not a non-negative integer");
    };
    if !account_exists(conn, account)? {
        return refuse(conn, 404, "unknown account");
    }
    let Some(path) = path else {
        return refuse(conn, 404, "track is not in the progression config");
    };
    let current = read_track(conn, account, id)?;
    let progress = current.as_ref().map_or(0, |c| c.progress);
    let before_free = current.as_ref().map_or(0, |c| c.confirmedFreeRank);
    let before_premium = current.as_ref().map_or(0, |c| c.confirmedPremiumRank);
    let (mut free, mut premium) = (before_free, before_premium);
    let mut notes = vec![context.clone()];
    if kind == "public" {
        let earned = earned_ranks(path, progress, false).0;
        if rank > earned {
            notes.push(format!(
                "rank {rank} is above the earned rank {earned} at progress {progress}, clamped"
            ));
        }
        free = free.max(rank.min(earned));
    } else {
        let gate = path.premium_gating_entitlement.as_deref().unwrap_or("");
        if gate.is_empty() {
            notes.push("premium confirm on a track without premium, nothing changed".into());
        } else if !entitlements::has_active(conn, &ctx.entitlement_defaults, account, gate)? {
            return refuse(conn, 403, &format!("account does not own {gate}"));
        } else {
            let earned = earned_ranks(path, progress, true).1;
            if rank > earned {
                notes.push(format!(
                    "rank {rank} is above the earned premium rank {earned} at progress {progress}, clamped"
                ));
            }
            premium = premium.max(rank.min(earned));
        }
    }
    let changed = free != before_free || premium != before_premium;
    let row = if changed {
        let written = write_track(conn, account, id, (progress, free, premium, &iso_now()))?;
        if ctx.settings.confirm_entitlements {
            grant_rank_entitlements(conn, ctx, account, path, "free_rewards", before_free, free, &mut notes)?;
            grant_rank_entitlements(
                conn,
                ctx,
                account,
                path,
                "premium_rewards",
                before_premium,
                premium,
                &mut notes,
            )?;
        }
        Some(written)
    } else {
        notes.push("already confirmed".into());
        current
    };
    let record = track_record(account, id, row.as_ref());
    let reply = Reply::ok(json!({"code": "OK", "message": "OK", "payload": record}));
    event(conn, account, who, route, None, &reply, &notes)?;
    if notes
        .iter()
        .any(|n| n.contains("clamped") || n.contains("without premium"))
    {
        warn_notes(&format!("Confirm for {account}"), &notes);
    }
    tracing::info!(
        "Confirmed {context} for {account}: confirmed free {}, premium {}{}",
        record["confirmed_fremium_rank"],
        record["confirmed_premium_rank"],
        if changed { "" } else { " (unchanged)" }
    );
    Ok(reply)
}

/// `ResetTrack`, DELETE /progression/:uid/:track: back to progress 0 and ranks 0.
pub fn reset(conn: &mut Conn, ctx: &Context, account: &str, id: &str, who: Caller) -> QueryResult<Reply> {
    let route = "DELETE /progression/:uid/:track";
    let current = read_track(conn, account, id)?;
    if current.is_none() && ctx.config.path(id).is_none() {
        let reply = Reply::status(404);
        event(
            conn,
            account,
            who,
            route,
            None,
            &reply,
            &[format!("track {id} is unknown")],
        )?;
        return Ok(reply);
    }
    diesel::delete(progress_tracks::table.find((account, id))).execute(conn)?;
    let reply = Reply::ok(json!({"code": "OK", "message": "OK", "payload": track_record(account, id, None)}));
    let (progress, free, premium) = current
        .as_ref()
        .map_or((0, 0, 0), |c| (c.progress, c.confirmedFreeRank, c.confirmedPremiumRank));
    event(
        conn,
        account,
        who,
        route,
        None,
        &reply,
        &[format!("track {id} was at {progress}, confirmed {free}/{premium}")],
    )?;
    tracing::warn!(
        "Reset track {id} of {account} (was progress {progress}) by {}",
        who.as_str()
    );
    Ok(reply)
}

// ---- The Hunt Pass selection ----

/// `GetSelectedHuntPass`: the stored selection while it is a configured track, else
/// `ACTIVE_HUNT_PASS` (season09b, the only one with reward data).
pub fn selected_hunt_pass(conn: &mut Conn, config: &ProgressionConfig, account: &str) -> QueryResult<String> {
    let stored: Option<String> = huntpassselection::table
        .find(account)
        .select(huntpassselection::progressionId)
        .first(conn)
        .optional()?;
    Ok(stored
        .filter(|id| config.path(id).is_some())
        .unwrap_or_else(|| config.active_hunt_pass.clone()))
}

/// `SetSelectedHuntPass`, POST /huntpass/:uid `{"progression_id": "..."}` (game server, debug only in
/// the client).
pub fn set_hunt_pass(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    body: Option<&Value>,
    who: Caller,
) -> QueryResult<Reply> {
    let route = "POST /huntpass/:uid";
    let mut notes = Vec::new();
    let id = read_field(body, &["progression_id", "progressionId"], "hunt pass id", &mut notes).cloned();
    let Some(id) = id
        .as_ref()
        .and_then(Value::as_str)
        .filter(|id| ctx.config.path(id).is_some())
    else {
        notes.push("not a configured track".into());
        let reply = Reply::status(400);
        event(conn, account, who, route, body, &reply, &notes)?;
        tracing::warn!(
            "Refusing hunt pass selection {} for {account}: not a configured track",
            id.as_ref()
                .map_or("undefined".to_owned(), service_kit::jsjson::stringify)
        );
        return Ok(reply);
    };
    let now = iso_now();
    diesel::insert_into(huntpassselection::table)
        .values((
            huntpassselection::accountId.eq(account),
            huntpassselection::progressionId.eq(id),
            huntpassselection::updatedDate.eq(&now),
        ))
        .on_conflict(huntpassselection::accountId)
        .do_update()
        .set((
            huntpassselection::progressionId.eq(id),
            huntpassselection::updatedDate.eq(&now),
        ))
        .execute(conn)?;
    let reply = Reply::ok(json!({"code": "OK", "message": "OK", "payload": id}));
    event(conn, account, who, route, body, &reply, &notes)?;
    warn_notes(&format!("Hunt pass selection for {account}"), &notes);
    tracing::info!("Hunt pass of {account} set to {id}");
    Ok(reply)
}

// ---- Seeding and the upgrade notice ----

/// `SeedProgression`'s modes: grandfather keeps upstream's max ranks, fresh starts over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    Grandfather,
    Fresh,
}

impl SeedMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grandfather => "grandfather",
            Self::Fresh => "fresh",
        }
    }
}

/// `SeedProgression` (admin, per account). Grandfather: every configured track at least at its max
/// rank and confirmed there, which looks like the stub and grants nothing; never lowers what is
/// stored. Fresh: every configured track at 0 and the stored objectives cleared.
pub fn seed(conn: &mut Conn, ctx: &Context, account: &str, mode: SeedMode, admin: &str) -> QueryResult<Reply> {
    let route = "admin SeedProgression";
    if !account_exists(conn, account)? {
        return Ok(Reply::status(404));
    }
    let before: Vec<TrackRow> = progress_tracks::table
        .filter(progress_tracks::accountId.eq(account))
        .load(conn)?;
    if mode == SeedMode::Fresh {
        diesel::delete(progress_tracks::table.filter(progress_tracks::accountId.eq(account))).execute(conn)?;
        diesel::delete(objectives::table.filter(objectives::accountId.eq(account))).execute(conn)?;
    }
    for path in &ctx.config.paths {
        let current = before.iter().find(|row| row.progressionId == path.id);
        let values = match mode {
            SeedMode::Fresh => (0, 0, 0, DEFAULT_CONFIRMED_DATE.to_owned()),
            SeedMode::Grandfather => {
                let progress = current.map_or(0, |c| c.progress).max(total_xp_to_max_rank(path));
                let earned = earned_ranks(path, progress, false).0.max(max_rank_id(path));
                (
                    progress,
                    current.map_or(0, |c| c.confirmedFreeRank).max(earned),
                    current.map_or(0, |c| c.confirmedPremiumRank).max(earned),
                    iso_now(),
                )
            }
        };
        write_track(conn, account, &path.id, (values.0, values.1, values.2, &values.3))?;
    }
    let tracks: Vec<TrackRow> = progress_tracks::table
        .filter(progress_tracks::accountId.eq(account))
        .load(conn)?;
    let tracks: Vec<Value> = tracks
        .iter()
        .map(|row| track_record(account, &row.progressionId, Some(row)))
        .collect();
    let shown: Vec<String> = before
        .iter()
        .map(|r| {
            format!(
                "{}={}/{}/{}",
                r.progressionId, r.progress, r.confirmedFreeRank, r.confirmedPremiumRank
            )
        })
        .collect();
    let reply = Reply::ok(Value::Array(tracks));
    let mut body = Map::new();
    body.insert("Mode".into(), json!(mode.as_str()));
    body.insert("Admin".into(), json!(admin));
    let note = format!(
        "before: {}",
        if shown.is_empty() {
            "no stored tracks".to_owned()
        } else {
            shown.join(", ")
        }
    );
    event(
        conn,
        account,
        Caller::Admin,
        route,
        Some(&Value::Object(body)),
        &reply,
        &[note],
    )?;
    tracing::warn!("Admin {admin} seeded the progression of {account} ({})", mode.as_str());
    Ok(reply)
}

/// `CountPlayersWithoutRealProgression`: accounts that own a character but have no stored track and
/// no progression event.
pub fn players_without_real_progression(conn: &mut Conn) -> QueryResult<i64> {
    use diesel::dsl::{exists, not};
    users::table
        .filter(exists(characters::table.filter(characters::userId.eq(users::userId))))
        .filter(not(exists(
            progress_tracks::table.filter(progress_tracks::accountId.eq(users::userId)),
        )))
        .filter(not(exists(
            progression_events::table.filter(progression_events::accountId.eq(users::userId)),
        )))
        .count()
        .get_result(conn)
}

/// `ProgressionUpgradeNotice`: the start warning after an upgrade from stub mode. Nothing is migrated.
pub fn upgrade_notice(conn: &mut Conn, stub_mode: bool) -> QueryResult<Option<String>> {
    if stub_mode {
        return Ok(None);
    }
    let count = players_without_real_progression(conn)?;
    Ok((count > 0).then(|| format!("{count} player account(s) have no stored progression yet: they start at Slayer level 1 with an empty Hunt Pass, not upstream's fake max ranks. Nothing was migrated. To keep max ranks for a player, seed them before they play (admin POST /undaunted/api/SeedProgression with Mode \"grandfather\"); PROGRESSION_MODE=stub brings the fake ranks back for everyone. See the upgrade notes (docs/setup/upgrading.md).")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_like_node() {
        let defaults = Settings::default();
        assert_eq!((defaults.grant_cap, defaults.replay_window_s), (5000, 5));
        let set = Settings::from_env(&Env::from_pairs([
            ("PROGRESSION_GRANT_CAP", " 12 "),
            ("PROGRESSION_REPLAY_WINDOW_S", "0"),
            ("PROGRESSION_CONFIRM", "off"),
        ]));
        assert_eq!((set.grant_cap, set.replay_window_s, set.confirm_off), (12, 0, true));
        let bad = Settings::from_env(&Env::from_pairs([
            ("PROGRESSION_GRANT_CAP", "1.5"),
            ("PROGRESSION_REPLAY_WINDOW_S", "-1"),
        ]));
        assert_eq!((bad.grant_cap, bad.replay_window_s), (5000, 5));
    }
}
