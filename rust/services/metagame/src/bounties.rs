//! The bounty board of real-mode accounts (`controllers/bounties.ts`). POST /bounty/:uid is a partial
//! upsert: each sender puts only the bounty that changed (or none) in "bounties", and draft_data only
//! when it changed. Nothing is removed for being absent; claim and abandon go through
//! POST /bounty/delete/:uid. Elements are stored and returned as sent.

use diesel::prelude::*;
use serde_json::{Value, json};

use crate::progression::{Caller, Event, read_field, read_integer, record_event};
use crate::realprogression::Reply;
use crate::schema::{bounties, bountydraft};
use crate::time::iso_now;

pub type Conn = store::Connection;

fn default_draft() -> Value {
    json!({
        "current_draft_choices": [],
        "previous_draft_selections": [],
        "bronze_count": 0,
        "silver_count": 0,
        "gold_count": 0
    })
}

/// Stored text that no longer parses: Node's `JSON.parse` threw, and the request failed.
fn parse(text: &str) -> QueryResult<Value> {
    service_kit::jsjson::parse(text)
        .map_err(|e| diesel::result::Error::DeserializationError(format!("SyntaxError: {e}").into()))
}

/// The board: every bounty by slot and id (each with drafted_timestamp, which the client doesn't
/// zero), and the draft data.
fn board(conn: &mut Conn, account: &str) -> QueryResult<Value> {
    let rows: Vec<(String, String)> = bounties::table
        .filter(bounties::accountId.eq(account))
        .order((bounties::slotIndex.asc(), bounties::bountyId.asc()))
        .select((bounties::data, bounties::updatedDate))
        .load(conn)?;
    let mut list = Vec::with_capacity(rows.len());
    for (data, updated) in rows {
        let mut element = parse(&data)?;
        if !element.get("drafted_timestamp").is_some_and(Value::is_string)
            && let Some(map) = element.as_object_mut()
        {
            map.insert("drafted_timestamp".into(), json!(updated));
        }
        list.push(element);
    }
    let draft: Option<String> = bountydraft::table
        .find(account)
        .select(bountydraft::data)
        .first(conn)
        .optional()?;
    let draft = match draft {
        Some(text) => parse(&text)?,
        None => default_draft(),
    };
    Ok(json!({"bounties": list, "draft_data": draft}))
}

/// `GetBountyReply`, GET /bounty/:uid.
pub fn reply(conn: &mut Conn, account: &str) -> QueryResult<Value> {
    Ok(json!({"code": "OK", "message": "OK", "payload": board(conn, account)?}))
}

fn event(
    conn: &mut Conn,
    account: &str,
    who: Caller,
    route: &str,
    body: Option<&Value>,
    status: u16,
    notes: &[String],
) -> QueryResult<()> {
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
}

/// `SetBounties`, POST /bounty/:uid.
pub fn set(conn: &mut Conn, account: &str, body: Option<&Value>, who: Caller) -> QueryResult<Reply> {
    let route = "POST /bounty/:uid";
    let mut notes = Vec::new();
    let object = body.and_then(Value::as_object);
    let empty = json!([]);
    let list = match object {
        Some(map) => match map.get("bounties") {
            Some(list) => list,
            None => {
                notes.push("no bounties key".into());
                &empty
            }
        },
        None => &Value::Null,
    };
    let Some(list) = list.as_array() else {
        notes.push("body is not {bounties: [...]}".into());
        event(conn, account, who, route, body, 400, &notes)?;
        tracing::warn!("Refusing bounty update for {account}: body is not {{bounties: [...]}}");
        return Ok(Reply {
            status: 400,
            body: None,
        });
    };
    let now = iso_now();
    let mut saved = Vec::new();
    for element in list {
        let id = element
            .as_object()
            .and_then(|e| e.get("bounty_id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let Some(id) = id else {
            notes.push(format!(
                "skipped element without a bounty_id: {}",
                service_kit::jsjson::stringify(element)
            ));
            continue;
        };
        let version = read_integer(
            element.get("update_version"),
            &format!("{id} update_version"),
            &mut notes,
        )
        .unwrap_or(0);
        let slot = read_integer(element.get("slot_index"), &format!("{id} slot_index"), &mut notes);
        let stored: Option<i64> = bounties::table
            .find((account, id))
            .select(bounties::updateVersion)
            .first(conn)
            .optional()?;
        if let Some(stored) = stored
            && stored > version
        {
            notes.push(format!(
                "kept {id}: stored update_version {stored} is newer than {version}"
            ));
            continue;
        }
        if let Some(slot) = slot {
            let replaced: Vec<String> = diesel::delete(
                bounties::table
                    .filter(bounties::accountId.eq(account))
                    .filter(bounties::slotIndex.eq(slot))
                    .filter(bounties::bountyId.ne(id)),
            )
            .returning(bounties::bountyId)
            .get_results(conn)?;
            for other in replaced {
                notes.push(format!("{id} took slot {slot} from {other}"));
            }
        }
        let data = service_kit::jsjson::stringify(element);
        diesel::insert_into(bounties::table)
            .values((
                bounties::accountId.eq(account),
                bounties::bountyId.eq(id),
                bounties::slotIndex.eq(slot),
                bounties::updateVersion.eq(version),
                bounties::data.eq(&data),
                bounties::updatedDate.eq(&now),
            ))
            .on_conflict((bounties::accountId, bounties::bountyId))
            .do_update()
            .set((
                bounties::slotIndex.eq(slot),
                bounties::updateVersion.eq(version),
                bounties::data.eq(&data),
                bounties::updatedDate.eq(&now),
            ))
            .execute(conn)?;
        saved.push(format!("{id} v{version}"));
    }
    if let Some(draft) = object.and_then(|map| map.get("draft_data")) {
        if draft.is_object() {
            let data = service_kit::jsjson::stringify(draft);
            diesel::insert_into(bountydraft::table)
                .values((
                    bountydraft::accountId.eq(account),
                    bountydraft::data.eq(&data),
                    bountydraft::updatedDate.eq(&now),
                ))
                .on_conflict(bountydraft::accountId)
                .do_update()
                .set((bountydraft::data.eq(&data), bountydraft::updatedDate.eq(&now)))
                .execute(conn)?;
            saved.push("draft data".into());
        } else {
            notes.push("draft_data is not an object, ignored".into());
        }
    }
    let reply = json!({"code": "OK", "message": "OK", "payload": board(conn, account)?});
    event(conn, account, who, route, body, 200, &notes)?;
    if !notes.is_empty() {
        tracing::warn!("Bounty update for {account}: {}", notes.join("; "));
    }
    tracing::info!(
        "Bounty update for {account}: {}",
        if saved.is_empty() {
            "nothing stored".to_owned()
        } else {
            saved.join(", ")
        }
    );
    Ok(Reply {
        status: 200,
        body: Some(reply),
    })
}

/// `DeleteBounties`, POST /bounty/delete/:uid `{"bounty_ids": [...]}`: claim, abandon and reset.
/// Unknown ids are fine.
pub fn delete(conn: &mut Conn, account: &str, body: Option<&Value>, who: Caller) -> QueryResult<Reply> {
    let route = "POST /bounty/delete/:uid";
    let mut notes = Vec::new();
    let ids = read_field(body, &["bounty_ids", "bountyIds"], "bounty ids", &mut notes).cloned();
    let Some(Value::Array(ids)) = ids else {
        notes.push("no bounty_ids array".into());
        event(conn, account, who, route, body, 400, &notes)?;
        tracing::warn!("Refusing bounty delete for {account}: no bounty_ids array");
        return Ok(Reply {
            status: 400,
            body: None,
        });
    };
    let mut removed = Vec::new();
    for id in &ids {
        let Some(id) = id.as_str() else {
            notes.push(format!("skipped id {}", service_kit::jsjson::stringify(id)));
            continue;
        };
        if diesel::delete(bounties::table.find((account, id))).execute(conn)? > 0 {
            removed.push(id.to_owned());
        }
    }
    let reply = json!({"code": "OK", "message": "OK", "payload": board(conn, account)?});
    event(conn, account, who, route, body, 200, &notes)?;
    tracing::info!(
        "Bounty delete for {account}: asked {}, removed {}",
        ids.len(),
        if removed.is_empty() {
            "none".to_owned()
        } else {
            removed.join(", ")
        }
    );
    Ok(Reply {
        status: 200,
        body: Some(reply),
    })
}
