// Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only; rewritten and
// corrected against the 1.4.4 executable for Dauntless Revived.

//! Slayer Links (`controllers/slayerlinks.ts`): two friends link up for a week (the social panel's
//! Linked Slayers tab). Stored, so an invite waits for a player who is offline. Routes:
//! `routes/slayerlinks.rs`.
//!
//! Every reply is the Phoenix envelope `{code, message, payload}`; an error is `{code: "<status>",
//! message, payload: null}` with that HTTP status. Rules: 3 slots each, numbered 1 to 3 as the client
//! numbers them; an invite runs out after 24 h and a link after a week. Only two accepted friends who
//! haven't blocked each other can link. A player sends at most [`MAX_INVITES_PER_WINDOW`] new invites
//! per [`INVITE_WINDOW_MS`], counted from the stored invites. Unfriending or a block cancels the
//! pending invites between the two (`friends.rs`); a running link stays until it ends.

use std::collections::HashSet;

use diesel::dsl::count_star;
use diesel::prelude::*;
use serde_json::{Map, Value, json};

use crate::accounts::is_account_id_shape;
use crate::friends::{account_exists, are_friends, blocked_either_way};
use crate::schema::{slayerlinkinvites, slayerlinks};
use crate::time::iso;

pub type Conn = store::Connection;

pub const LINK_SLOTS: i64 = 3;
pub const INVITE_EXPIRY_HOURS: i64 = 24;
pub const LINK_DURATION_HOURS: i64 = 168;
pub const MAX_AVAILABILITY_IDS: usize = 50;
pub const MAX_INVITES_PER_WINDOW: i64 = 20;
pub const INVITE_WINDOW_MS: i64 = 10 * 60 * 1000;
/// Accepted invites and ended links are kept this long for the log's sake, then removed.
pub const KEEP_DAYS: i64 = 30;
const HOUR_MS: i64 = 60 * 60 * 1000;

/// A status and its body.
pub type Reply = (u16, Value);

fn ok(payload: Value) -> Reply {
    (200, json!({"code": null, "message": "OK", "payload": payload}))
}

fn fail(status: u16, message: &str) -> Reply {
    (
        status,
        json!({"code": status.to_string(), "message": message, "payload": null}),
    )
}

fn shown(value: Option<&Value>) -> &str {
    match value {
        Some(Value::String(id)) if is_account_id_shape(id) => id,
        Some(Value::String(_)) => "<not an account id>",
        _ => "<none>",
    }
}

fn account_id(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|id| is_account_id_shape(id))
}

/// `SlotOf`: a whole number 1..LINK_SLOTS (a number, or a numeric string of one or two digits).
fn slot_of(value: Option<&Value>) -> Option<i64> {
    let slot = match value? {
        Value::Number(n) => n.as_f64()?,
        Value::String(text) if (1..=2).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse().ok()?
        }
        _ => return None,
    };
    (slot.fract() == 0.0 && (1.0..=LINK_SLOTS as f64).contains(&slot)).then_some(slot as i64)
}

fn shown_slot(slot: Option<i64>, missing: &str) -> String {
    slot.map_or_else(|| missing.to_owned(), |s| s.to_string())
}

#[derive(Debug, Clone, Queryable)]
struct Invite {
    invite_id: String,
    sender_id: String,
    target_id: String,
    sender_slot: i64,
    #[allow(dead_code)] // read with the row
    created_at: i64,
    expires_at: i64,
    status: String,
}

#[derive(Debug, Clone, Queryable)]
struct Link {
    link_id: String,
    sender_id: String,
    target_id: String,
    sender_slot: i64,
    target_slot: i64,
    #[allow(dead_code)] // read with the row
    created_at: i64,
    ends_at: i64,
}

fn other_of<'a>(sender: &'a str, target: &'a str, account: &str) -> &'a str {
    if sender == account { target } else { sender }
}

impl Link {
    fn slot_of(&self, account: &str) -> i64 {
        if self.sender_id == account {
            self.sender_slot
        } else {
            self.target_slot
        }
    }

    fn other(&self, account: &str) -> &str {
        other_of(&self.sender_id, &self.target_id, account)
    }
}

impl Invite {
    fn other(&self, account: &str) -> &str {
        other_of(&self.sender_id, &self.target_id, account)
    }
}

/// Pending invites of these accounts that ran out become EXPIRED, one side at a time.
fn expire_invites(conn: &mut Conn, accounts: &[&str], now: i64) -> QueryResult<()> {
    let mut seen = HashSet::new();
    for account in accounts {
        if !seen.insert(*account) {
            continue;
        }
        let pending = slayerlinkinvites::status
            .eq("PENDING")
            .and(slayerlinkinvites::expiresAt.le(now));
        diesel::update(
            slayerlinkinvites::table
                .filter(pending)
                .filter(slayerlinkinvites::senderId.eq(account)),
        )
        .set(slayerlinkinvites::status.eq("EXPIRED"))
        .execute(conn)?;
        diesel::update(
            slayerlinkinvites::table
                .filter(pending)
                .filter(slayerlinkinvites::targetId.eq(account)),
        )
        .set(slayerlinkinvites::status.eq("EXPIRED"))
        .execute(conn)?;
    }
    Ok(())
}

fn pending_invites(conn: &mut Conn, account: &str, now: i64) -> QueryResult<Vec<Invite>> {
    slayerlinkinvites::table
        .filter(slayerlinkinvites::status.eq("PENDING"))
        .filter(slayerlinkinvites::expiresAt.gt(now))
        .filter(
            slayerlinkinvites::senderId
                .eq(account)
                .or(slayerlinkinvites::targetId.eq(account)),
        )
        .order(slayerlinkinvites::createdAt.asc())
        .load(conn)
}

fn active_links(conn: &mut Conn, account: &str, now: i64) -> QueryResult<Vec<Link>> {
    slayerlinks::table
        .filter(slayerlinks::endsAt.gt(now))
        .filter(slayerlinks::senderId.eq(account).or(slayerlinks::targetId.eq(account)))
        .order(slayerlinks::createdAt.asc())
        .load(conn)
}

/// Slots not held by a running link.
fn free_slots(conn: &mut Conn, account: &str, now: i64) -> QueryResult<Vec<i64>> {
    let used: HashSet<i64> = active_links(conn, account, now)?
        .iter()
        .map(|link| link.slot_of(account))
        .collect();
    Ok((1..=LINK_SLOTS).filter(|slot| !used.contains(slot)).collect())
}

fn linked_with(conn: &mut Conn, a: &str, b: &str, now: i64) -> QueryResult<bool> {
    Ok(active_links(conn, a, now)?.iter().any(|link| link.other(a) == b))
}

/// Old rows the client no longer sees.
fn sweep(conn: &mut Conn, now: i64) -> QueryResult<()> {
    let before = now - KEEP_DAYS * 24 * HOUR_MS;
    diesel::delete(
        slayerlinkinvites::table
            .filter(slayerlinkinvites::status.eq_any(["DECLINED", "CANCELED", "EXPIRED"]))
            .filter(slayerlinkinvites::expiresAt.le(now)),
    )
    .execute(conn)?;
    diesel::delete(
        slayerlinkinvites::table
            .filter(slayerlinkinvites::status.ne("PENDING"))
            .filter(slayerlinkinvites::expiresAt.le(before)),
    )
    .execute(conn)?;
    diesel::delete(slayerlinks::table.filter(slayerlinks::endsAt.le(before))).execute(conn)?;
    Ok(())
}

fn recent_invites_by(conn: &mut Conn, account: &str, now: i64) -> QueryResult<i64> {
    slayerlinkinvites::table
        .filter(slayerlinkinvites::senderId.eq(account))
        .filter(slayerlinkinvites::createdAt.gt(now - INVITE_WINDOW_MS))
        .select(count_star())
        .first(conn)
}

// ---- Replies ----

fn invite_entry(row: &Invite, account: &str) -> Value {
    json!({
        "account_id": row.other(account),
        "slot": row.sender_slot,
        "direction": if row.sender_id == account { "Sent" } else { "Received" },
        "status": "Pending",
        "expires": iso(row.expires_at),
        "link_id": row.invite_id,
    })
}

fn link_entry(link: &Link, account: &str) -> Value {
    let other = link.other(account);
    json!({
        "account_id": other,
        "linked_account_id": other,
        "slot": link.slot_of(account),
        "ends": iso(link.ends_at),
        "link_id": link.link_id,
        "prize_pool": [],
    })
}

/// The caller's pending invites and running links (`ReadLists`), after expiring old invites.
pub fn read_lists(conn: &mut Conn, account: &str, now: i64) -> QueryResult<(Vec<Value>, Vec<Value>)> {
    expire_invites(conn, &[account], now)?;
    let invites = pending_invites(conn, account, now)?
        .iter()
        .map(|row| invite_entry(row, account))
        .collect();
    let mut links = active_links(conn, account, now)?;
    links.sort_by_key(|link| link.slot_of(account));
    Ok((invites, links.iter().map(|link| link_entry(link, account)).collect()))
}

pub fn invites_reply(lists: (Vec<Value>, Vec<Value>)) -> Reply {
    ok(json!({"invites": lists.0}))
}

pub fn links_reply(lists: (Vec<Value>, Vec<Value>)) -> Reply {
    ok(json!({"links": lists.1}))
}

/// GET /slayerlink/status_good (the client polls it for news).
pub fn status_reply(lists: (Vec<Value>, Vec<Value>)) -> Reply {
    ok(json!({
        "invites": lists.0,
        "links": lists.1,
        "config": {"link_duration_hours": LINK_DURATION_HOURS, "invite_expiry_hours": INVITE_EXPIRY_HOURS},
    }))
}

// ---- Changes ----

fn field<'a>(body: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    body.and_then(|b| b.get(name))
}

/// PUT /slayerlink/invite `{account_id, slot, action_source}`.
pub fn invite(conn: &mut Conn, caller: &str, body: Option<&Value>, now: i64) -> QueryResult<Reply> {
    let target_value = field(body, "account_id");
    let slot = slot_of(field(body, "slot"));
    let refuse = |status: u16, why: &str| {
        tracing::info!(
            "slayerlink: invite by={caller} to={} slot={} refused {status}: {why}",
            shown(target_value),
            shown_slot(slot, "?")
        );
        fail(status, why)
    };
    let Some(target) = account_id(target_value) else {
        return Ok(refuse(400, "no account to invite"));
    };
    let Some(slot) = slot else {
        return Ok(refuse(400, &format!("the slot must be 1 to {LINK_SLOTS}")));
    };
    if target == caller {
        return Ok(refuse(409, "you cannot link with yourself"));
    }
    let outcome = conn.transaction(|conn| -> QueryResult<Result<(String, bool), Reply>> {
        if !account_exists(conn, target)? {
            return Ok(Err(refuse(404, "no such account")));
        }
        if blocked_either_way(conn, caller, target)? {
            return Ok(Err(refuse(403, "blocked")));
        }
        if !are_friends(conn, caller, target)? {
            return Ok(Err(refuse(403, "only friends can link")));
        }
        expire_invites(conn, &[caller, target], now)?;
        let pending = pending_invites(conn, caller, now)?;
        if let Some(existing) = pending.iter().find(|r| r.sender_id == caller && r.target_id == target) {
            return Ok(Ok((existing.invite_id.clone(), true)));
        }
        if pending.iter().any(|r| r.sender_id == target && r.target_id == caller) {
            return Ok(Err(refuse(409, "this player already invited you")));
        }
        if linked_with(conn, caller, target, now)? {
            return Ok(Err(refuse(409, "already linked with this player")));
        }
        if !free_slots(conn, caller, now)?.contains(&slot) {
            return Ok(Err(refuse(409, "the slot is already linked")));
        }
        if pending.iter().any(|r| r.sender_id == caller && r.sender_slot == slot) {
            return Ok(Err(refuse(409, "the slot already has an invite waiting")));
        }
        if free_slots(conn, target, now)?.is_empty() {
            return Ok(Err(refuse(409, "the player has no free slot")));
        }
        if recent_invites_by(conn, caller, now)? >= MAX_INVITES_PER_WINDOW {
            return Ok(Err(refuse(
                409,
                &format!(
                    "too many invites: at most {MAX_INVITES_PER_WINDOW} in {} minutes",
                    INVITE_WINDOW_MS / 60000
                ),
            )));
        }
        let id = crate::crypto::random_uuid();
        diesel::insert_into(slayerlinkinvites::table)
            .values((
                slayerlinkinvites::inviteId.eq(&id),
                slayerlinkinvites::senderId.eq(caller),
                slayerlinkinvites::targetId.eq(target),
                slayerlinkinvites::senderSlot.eq(slot),
                slayerlinkinvites::createdAt.eq(now),
                slayerlinkinvites::expiresAt.eq(now + INVITE_EXPIRY_HOURS * HOUR_MS),
                slayerlinkinvites::status.eq("PENDING"),
            ))
            .execute(conn)?;
        sweep(conn, now)?;
        Ok(Ok((id, false)))
    })?;
    let (id, again) = match outcome {
        Ok(found) => found,
        Err(refusal) => return Ok(refusal),
    };
    tracing::info!(
        "slayerlink: invite by={caller} to={target} slot={slot} -> {} id={id}",
        if again { "already invited" } else { "sent" }
    );
    Ok(ok(json!({"link_id": id})))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Accept,
    Reject,
    Cancel,
}

impl Action {
    /// The status an invite gets from the action, and the one a repeated action finds.
    fn answered(self) -> &'static str {
        match self {
            Self::Accept => "ACCEPTED",
            Self::Reject => "DECLINED",
            Self::Cancel => "CANCELED",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
            Self::Cancel => "cancel",
        }
    }
}

/// The invite an answer is about: accept and reject are the invited player's, cancel the sender's.
fn find_invite(
    conn: &mut Conn,
    caller: &str,
    action: Action,
    ids: &[String],
    other: Option<&str>,
) -> QueryResult<Option<Invite>> {
    let cancel = action == Action::Cancel;
    for id in ids {
        let query = slayerlinkinvites::table.filter(slayerlinkinvites::inviteId.eq(id));
        let found: Option<Invite> = if cancel {
            query
                .filter(slayerlinkinvites::senderId.eq(caller))
                .first(conn)
                .optional()?
        } else {
            query
                .filter(slayerlinkinvites::targetId.eq(caller))
                .first(conn)
                .optional()?
        };
        if found.is_some() {
            return Ok(found);
        }
    }
    let Some(other) = other else {
        return Ok(None);
    };
    let (sender, target) = if cancel { (caller, other) } else { (other, caller) };
    let rows: Vec<Invite> = slayerlinkinvites::table
        .filter(slayerlinkinvites::senderId.eq(sender))
        .filter(slayerlinkinvites::targetId.eq(target))
        .order(slayerlinkinvites::createdAt.asc())
        .load(conn)?;
    // The pending one if there is one, else the latest (a repeated answer finds it)
    Ok(rows.iter().find(|r| r.status == "PENDING").or(rows.last()).cloned())
}

/// An answered invite: the row, whether it was already answered, and the slot an accept took.
type Answered = (Invite, bool, Option<i64>);

/// POST /slayerlink/invite `{account_id, action, slot, action_source}`.
pub fn answer(conn: &mut Conn, caller: &str, body: Option<&Value>, now: i64) -> QueryResult<Reply> {
    let raw = field(body, "action")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
        .unwrap_or_default();
    let other_value = field(body, "account_id");
    let refuse = |status: u16, why: &str| {
        tracing::info!(
            "slayerlink: {} by={caller} other={} refused {status}: {why}",
            if raw.is_empty() { "answer" } else { raw.as_str() },
            shown(other_value)
        );
        fail(status, why)
    };
    let action = match raw.as_str() {
        "accept" => Action::Accept,
        "reject" => Action::Reject,
        "cancel" => Action::Cancel,
        _ => return Ok(refuse(400, "the action must be accept, reject or cancel")),
    };
    let ids: Vec<String> = ["link_id", "invite_id"]
        .iter()
        .filter_map(|name| field(body, name).and_then(Value::as_str))
        .filter(|id| !id.is_empty() && id.encode_utf16().count() <= 64)
        .map(str::to_owned)
        .collect();
    let asked = slot_of(field(body, "slot"));
    let outcome = conn.transaction(|conn| -> QueryResult<Result<Answered, Reply>> {
        let Some(row) = find_invite(conn, caller, action, &ids, account_id(other_value))? else {
            return Ok(Err(refuse(404, "no such invite")));
        };
        if row.status != "PENDING" {
            if row.status == action.answered() {
                return Ok(Ok((row, true, None)));
            }
            let why = if row.status == "EXPIRED" {
                "the invite ran out".to_owned()
            } else {
                format!("the invite was already {}", row.status.to_lowercase())
            };
            return Ok(Err(refuse(409, &why)));
        }
        if row.expires_at <= now {
            diesel::update(slayerlinkinvites::table.find(&row.invite_id))
                .set(slayerlinkinvites::status.eq("EXPIRED"))
                .execute(conn)?;
            return Ok(Err(refuse(409, "the invite ran out")));
        }
        let mut target_slot = None;
        if action == Action::Accept {
            if blocked_either_way(conn, &row.sender_id, &row.target_id)?
                || !are_friends(conn, &row.sender_id, &row.target_id)?
            {
                return Ok(Err(refuse(403, "only friends can link")));
            }
            if linked_with(conn, &row.sender_id, &row.target_id, now)? {
                return Ok(Err(refuse(409, "already linked with this player")));
            }
            if !free_slots(conn, &row.sender_id, now)?.contains(&row.sender_slot) {
                return Ok(Err(refuse(409, "the other player's slot is taken")));
            }
            let free = free_slots(conn, &row.target_id, now)?;
            let Some(slot) = asked.filter(|s| free.contains(s)).or(free.first().copied()) else {
                return Ok(Err(refuse(409, "no free slot")));
            };
            target_slot = Some(slot);
            diesel::insert_into(slayerlinks::table)
                .values((
                    slayerlinks::linkId.eq(&row.invite_id),
                    slayerlinks::senderId.eq(&row.sender_id),
                    slayerlinks::targetId.eq(&row.target_id),
                    slayerlinks::senderSlot.eq(row.sender_slot),
                    slayerlinks::targetSlot.eq(slot),
                    slayerlinks::createdAt.eq(now),
                    slayerlinks::endsAt.eq(now + LINK_DURATION_HOURS * HOUR_MS),
                ))
                .execute(conn)?;
        }
        diesel::update(slayerlinkinvites::table.find(&row.invite_id))
            .set(slayerlinkinvites::status.eq(action.answered()))
            .execute(conn)?;
        Ok(Ok((row, false, target_slot)))
    })?;
    let (row, again, slot) = match outcome {
        Ok(found) => found,
        Err(refusal) => return Ok(refusal),
    };
    tracing::info!(
        "slayerlink: {} by={caller} other={} id={} -> {}{}",
        action.name(),
        row.other(caller),
        row.invite_id,
        if again {
            "already done".to_owned()
        } else {
            action.answered().to_lowercase()
        },
        slot.map_or_else(String::new, |s| format!(" (slots {} and {s})", row.sender_slot))
    );
    Ok(ok(json!({"link_id": row.invite_id})))
}

/// DELETE /slayerlink/invites/{account_id}: the caller's own id withdraws every invite the caller
/// sent and declines every one they got; another player's id does that for the invites between the
/// two.
pub fn delete_invites(conn: &mut Conn, caller: &str, account: &str, now: i64) -> QueryResult<Reply> {
    if !is_account_id_shape(account) {
        tracing::info!("slayerlink: delete invites by={caller} refused 400: no account");
        return Ok(fail(400, "no account"));
    }
    let (cancelled, declined) = conn.transaction(|conn| -> QueryResult<(usize, usize)> {
        expire_invites(conn, &[caller], now)?;
        let pending = slayerlinkinvites::status.eq("PENDING");
        let cancelled = if account == caller {
            diesel::update(
                slayerlinkinvites::table
                    .filter(pending)
                    .filter(slayerlinkinvites::senderId.eq(caller)),
            )
            .set(slayerlinkinvites::status.eq("CANCELED"))
            .execute(conn)?
        } else {
            diesel::update(
                slayerlinkinvites::table
                    .filter(pending)
                    .filter(slayerlinkinvites::senderId.eq(caller))
                    .filter(slayerlinkinvites::targetId.eq(account)),
            )
            .set(slayerlinkinvites::status.eq("CANCELED"))
            .execute(conn)?
        };
        let declined = if account == caller {
            diesel::update(
                slayerlinkinvites::table
                    .filter(pending)
                    .filter(slayerlinkinvites::targetId.eq(caller)),
            )
            .set(slayerlinkinvites::status.eq("DECLINED"))
            .execute(conn)?
        } else {
            diesel::update(
                slayerlinkinvites::table
                    .filter(pending)
                    .filter(slayerlinkinvites::targetId.eq(caller))
                    .filter(slayerlinkinvites::senderId.eq(account)),
            )
            .set(slayerlinkinvites::status.eq("DECLINED"))
            .execute(conn)?
        };
        Ok((cancelled, declined))
    })?;
    tracing::info!(
        "slayerlink: delete invites by={caller} of={} -> {cancelled} withdrawn, {declined} declined",
        if account == caller { "all" } else { account }
    );
    Ok(ok(json!({})))
}

/// DELETE /slayerlink/links `{account_id, slot, delete_pair}`: ends the caller's link in that slot,
/// or with that player. The link is one row for both, so it ends for both whatever `delete_pair` says.
/// Nothing to remove still answers 200.
pub fn delete_link(
    conn: &mut Conn,
    caller: &str,
    body: Option<&Value>,
    query: &Map<String, Value>,
    now: i64,
) -> QueryResult<Reply> {
    let pick = |name: &str| match field(body, name) {
        None | Some(Value::Null) => query.get(name),
        found => found,
    };
    let slot = slot_of(pick("slot"));
    let other = account_id(pick("account_id"));
    if slot.is_none() && other.is_none() {
        tracing::info!("slayerlink: delete link by={caller} refused 400: no slot or account");
        return Ok(fail(400, "no slot or account"));
    }
    let removed = conn.transaction(|conn| -> QueryResult<Option<Link>> {
        let links = active_links(conn, caller, now)?;
        let link = links
            .iter()
            .find(|l| slot.is_none_or(|s| l.slot_of(caller) == s) && other.is_none_or(|o| l.other(caller) == o))
            .or_else(|| links.iter().find(|l| other.is_some_and(|o| l.other(caller) == o)))
            .or_else(|| links.iter().find(|l| slot.is_some_and(|s| l.slot_of(caller) == s)))
            .cloned();
        if let Some(link) = &link {
            diesel::delete(slayerlinks::table.find(&link.link_id)).execute(conn)?;
        }
        Ok(link)
    })?;
    let pair = match field(body, "delete_pair") {
        None | Some(Value::Null) => "null".to_owned(),
        Some(value) => service_kit::jsjson::stringify(value),
    };
    tracing::info!(
        "slayerlink: delete link by={caller} slot={} other={} delete_pair={pair} -> {}",
        shown_slot(slot, "-"),
        other.unwrap_or("-"),
        removed.as_ref().map_or_else(
            || "nothing to remove".to_owned(),
            |l| format!("removed {} (with {})", l.link_id, l.other(caller))
        )
    );
    Ok(ok(json!({})))
}

/// POST /slayerlink/availability `{account_ids}`: which of these friends the caller could invite now.
pub fn availability(conn: &mut Conn, caller: &str, body: Option<&Value>, now: i64) -> QueryResult<Reply> {
    let mut ids: Vec<String> = Vec::new();
    if let Some(Value::Array(list)) = field(body, "account_ids") {
        for id in list.iter().filter_map(Value::as_str) {
            if !id.is_empty() && id.encode_utf16().count() <= 128 && !ids.iter().any(|seen| seen == id) {
                ids.push(id.to_owned());
            }
        }
    }
    ids.truncate(MAX_AVAILABILITY_IDS);
    let entries = conn.transaction(|conn| -> QueryResult<Vec<Value>> {
        expire_invites(conn, &[caller], now)?;
        let pending = pending_invites(conn, caller, now)?;
        // A slot of the caller's that neither a link nor a waiting invite holds
        let has_slot = free_slots(conn, caller, now)?
            .iter()
            .any(|slot| !pending.iter().any(|r| r.sender_id == caller && r.sender_slot == *slot));
        let mut entries = Vec::with_capacity(ids.len());
        for id in &ids {
            let available = has_slot
                && id != caller
                && is_account_id_shape(id)
                && account_exists(conn, id)?
                && are_friends(conn, caller, id)?
                && !blocked_either_way(conn, caller, id)?
                && !linked_with(conn, caller, id, now)?
                && !pending.iter().any(|r| r.other(caller) == id)
                && !free_slots(conn, id, now)?.is_empty();
            entries.push(json!({"account_id": id, "available": available}));
        }
        Ok(entries)
    })?;
    tracing::debug!(
        "slayerlink: availability for {caller}: {} of {}",
        entries.iter().filter(|e| e["available"] == true).count(),
        entries.len()
    );
    Ok(ok(json!({"availability": entries})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_read_like_node() {
        assert_eq!(slot_of(Some(&json!(1))), Some(1));
        assert_eq!(slot_of(Some(&json!(3.0))), Some(3));
        assert_eq!(slot_of(Some(&json!("2"))), Some(2));
        assert_eq!(slot_of(Some(&json!("02"))), Some(2));
        assert_eq!(slot_of(Some(&json!("4"))), None);
        assert_eq!(slot_of(Some(&json!(0))), None);
        assert_eq!(slot_of(Some(&json!(1.5))), None);
        assert_eq!(slot_of(Some(&json!("1.0"))), None);
        assert_eq!(slot_of(Some(&json!(true))), None);
        assert_eq!(slot_of(None), None);
    }
}
