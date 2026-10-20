//! Cooldowns of real-mode accounts (`controllers/cooldowns.ts`). A cooldown is an opaque id
//! (bounty_tokens_<hunt pass>, gatherable tracking GUIDs, Trials rewards, Blueprint ids) and an
//! absolute start time as an ISO-8601 string; durations and resets live in the client.

use diesel::prelude::*;
use serde_json::{Map, Value, json};

use crate::progression::{Caller, Event, read_field, record_event};
use crate::realprogression::Reply;
use crate::schema::cooldowns;
use crate::time::{iso_now, now_ms};

pub type Conn = store::Connection;

/// Harvest cooldowns are only read back for RespawnGatherableTimerMinutes; older ones are pruned.
const GATHERABLE_KEEP_MS: i64 = 24 * 60 * 60 * 1000;

fn is_gatherable(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
}

/// Every cooldown of the account: id -> start.
fn cooldown_map(conn: &mut Conn, account: &str) -> QueryResult<Map<String, Value>> {
    let rows: Vec<(String, String)> = cooldowns::table
        .filter(cooldowns::accountId.eq(account))
        .select((cooldowns::cooldownId, cooldowns::startedDate))
        .load(conn)?;
    Ok(rows.into_iter().map(|(id, started)| (id, json!(started))).collect())
}

fn upsert(conn: &mut Conn, account: &str, id: &str, started: &str) -> QueryResult<()> {
    let now = iso_now();
    diesel::insert_into(cooldowns::table)
        .values((
            cooldowns::accountId.eq(account),
            cooldowns::cooldownId.eq(id),
            cooldowns::startedDate.eq(started),
            cooldowns::updatedDate.eq(&now),
        ))
        .on_conflict((cooldowns::accountId, cooldowns::cooldownId))
        .do_update()
        .set((cooldowns::startedDate.eq(started), cooldowns::updatedDate.eq(&now)))
        .execute(conn)
        .map(drop)
}

/// `GetCooldownReply`, GET /cooldown/:uid: every cooldown in one object.
pub fn reply(conn: &mut Conn, account: &str) -> QueryResult<Value> {
    Ok(json!({"code": 200, "message": "OK", "payload": cooldown_map(conn, account)?}))
}

/// `SetCooldownBatch`, PUT /cooldown/batch/:uid `{"cooldowns": [{cooldown_id, cooldown_started_date}]}`
/// (or an id -> date map). Stored as sent.
pub fn set_batch(conn: &mut Conn, account: &str, body: Option<&Value>, who: Caller) -> QueryResult<Reply> {
    let route = "PUT /cooldown/batch/:uid";
    let mut notes = Vec::new();
    let entries: Option<Vec<(Option<Value>, Option<Value>)>> = match body {
        Some(Value::Object(map)) => match map.get("cooldowns") {
            Some(Value::Array(list)) => Some(
                list.iter()
                    .map(|entry| {
                        let id = read_field(
                            Some(entry),
                            &["cooldown_id", "cooldownId", "id"],
                            "cooldown id",
                            &mut notes,
                        )
                        .cloned();
                        let started = read_field(
                            Some(entry),
                            &[
                                "cooldown_started_date",
                                "cooldown_start_date",
                                "startedDate",
                                "started_date",
                            ],
                            "cooldown start",
                            &mut notes,
                        )
                        .cloned();
                        (id, started)
                    })
                    .collect(),
            ),
            None if map.values().all(Value::is_string) => {
                notes.push("body is an id -> date map, not {cooldowns: [...]}".into());
                Some(
                    map.iter()
                        .map(|(id, started)| (Some(json!(id)), Some(started.clone())))
                        .collect(),
                )
            }
            _ => None,
        },
        _ => None,
    };
    let event = |conn: &mut Conn, status: u16, notes: &[String]| {
        record_event(
            conn,
            Event {
                account,
                caller: who.as_str(),
                route,
                body,
                status: i64::from(status),
                reply: None,
                notes,
            },
        )
    };
    let Some(entries) = entries else {
        notes.push("no cooldowns array".into());
        event(conn, 400, &notes)?;
        tracing::warn!("Refusing cooldown batch for {account}: no cooldowns array");
        return Ok(Reply {
            status: 400,
            body: None,
        });
    };
    for (id, started) in &entries {
        let Some(id) = id.as_ref().and_then(Value::as_str).filter(|id| !id.is_empty()) else {
            // `JSON.stringify({Id, Started})`, undefined fields left out
            let mut shown = Map::new();
            if let Some(id) = id {
                shown.insert("Id".into(), id.clone());
            }
            if let Some(started) = started {
                shown.insert("Started".into(), started.clone());
            }
            notes.push(format!(
                "skipped entry without an id: {}",
                service_kit::jsjson::stringify(&Value::Object(shown))
            ));
            continue;
        };
        let started = match started.as_ref().and_then(Value::as_str).filter(|s| !s.is_empty()) {
            None => {
                notes.push(format!("{id} has no start date, stored now"));
                iso_now()
            }
            Some(text) => {
                if crate::js::date_parse(text).is_none() {
                    notes.push(format!("{id} start {text} does not parse as a date, stored as sent"));
                }
                text.to_owned()
            }
        };
        upsert(conn, account, id, &started)?;
    }
    let reply = json!({"code": 200, "message": "OK", "payload": cooldown_map(conn, account)?});
    event(conn, 200, &notes)?;
    if !notes.is_empty() {
        tracing::warn!("Cooldown batch for {account}: {}", notes.join("; "));
    }
    tracing::info!("Stored {} cooldown(s) for {account}", entries.len());
    Ok(Reply {
        status: 200,
        body: Some(reply),
    })
}

/// `StartCooldown`, PUT /cooldown/:uid/:id with no body: the cooldown starts now. Old gatherable
/// cooldowns go first.
pub fn start(conn: &mut Conn, account: &str, id: &str, who: Caller) -> QueryResult<Reply> {
    let cutoff = now_ms() - GATHERABLE_KEEP_MS;
    let rows: Vec<(String, String)> = cooldowns::table
        .filter(cooldowns::accountId.eq(account))
        .select((cooldowns::cooldownId, cooldowns::startedDate))
        .load(conn)?;
    for (cooldown, started) in rows {
        if is_gatherable(&cooldown) && crate::js::date_parse(&started).is_some_and(|at| at < cutoff) {
            diesel::delete(cooldowns::table.find((account, &cooldown))).execute(conn)?;
        }
    }
    upsert(conn, account, id, &iso_now())?;
    record_event(
        conn,
        Event {
            account,
            caller: who.as_str(),
            route: "PUT /cooldown/:uid/:id",
            body: None,
            status: 200,
            reply: None,
            notes: &[format!("cooldown {id}")],
        },
    )?;
    tracing::info!("Started cooldown {id} for {account}");
    Ok(Reply {
        status: 200,
        body: Some(json!({})),
    })
}
