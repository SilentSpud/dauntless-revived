//! Known unlocker tools (`controllers/unlocker-detection.ts`): an inventory write from a player's
//! client that carries one of the Nova 1.0.3 editor's exact `source` markers bans the account for a
//! week (`anticheat:nova-marker`), recorded like an admin's ban. Game servers are never checked.

use diesel::prelude::*;
use serde_json::Value;

use crate::auth::{MetagameAuth, ban_for};
use crate::schema::{account_bans, device_ban_signals, moderation_events, users};
use crate::time::now_ms;

const SOURCES: [&str; 7] = [
    "local-editor:catalog-cosmetics:user-request",
    "local-editor:user-request",
    "local-editor:equipment:user-request",
    "local-editor:cells-five-each:user-request",
    "local-editor:unlock-equipment:user-request",
    "local-editor:tonics:user-request",
    "nova-unlocker:platinum:user-request",
];
const DELTAS: [&str; 5] = [
    "addInstancedItems",
    "removeInstancedItems",
    "saveInstancedItems",
    "addStackedItems",
    "removeStackedItems",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    pub blocked: bool,
    pub suspended: bool,
}

fn is_account_id(id: &str) -> bool {
    id.strip_prefix("UID-").is_some_and(|rest| {
        (1..=100).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// What the inventory route must do before anything else; `Err` means the ban could not be
/// recorded and the write must be refused (fail closed).
pub fn enforce(conn: &mut store::Connection, auth: &MetagameAuth, body: Option<&Value>) -> QueryResult<Detection> {
    let clean = Detection {
        blocked: false,
        suspended: false,
    };
    let Some(account) = auth.user_id().filter(|id| !auth.is_game_server() && is_account_id(id)) else {
        return Ok(clean);
    };
    let Some(Value::Object(request)) = body else {
        return Ok(clean);
    };
    let Some(source) = request
        .get("source")
        .and_then(Value::as_str)
        .filter(|s| SOURCES.contains(s))
    else {
        return Ok(clean);
    };
    let counts: Vec<(&str, usize)> = DELTAS
        .iter()
        .map(|key| (*key, request.get(*key).and_then(Value::as_array).map_or(0, Vec::len)))
        .collect();
    if counts.iter().all(|(_, n)| *n == 0) {
        return Ok(clean);
    }
    if ban_for(conn, Some(account), None)?.is_some() {
        return Ok(Detection {
            blocked: true,
            suspended: true,
        });
    }
    let evidence: Vec<String> = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(key, n)| format!("{key}={n}"))
        .collect();
    let reason = format!(
        "Blocked inventory mutation; Nova 1.0.3 signature {source}; {}",
        evidence.join(", ")
    );
    // SetAccountBan(account, reason, true, null, 'anticheat:nova-marker', 168, false)
    let is_admin: Option<bool> = users::table
        .find(account)
        .select(users::isAdmin)
        .first(conn)
        .optional()?;
    match is_admin {
        None => return Err(diesel::result::Error::NotFound),
        Some(true) => {
            return Ok(Detection {
                blocked: true,
                suspended: false,
            });
        }
        Some(false) => {}
    }
    let now = now_ms();
    let expires = now + 168 * 3_600_000;
    let actor = "anticheat:nova-marker";
    diesel::delete(device_ban_signals::table.filter(device_ban_signals::account_id.eq(account))).execute(conn)?;
    diesel::insert_into(account_bans::table)
        .values((
            account_bans::account_id.eq(account),
            account_bans::reason.eq(&reason),
            account_bans::address.eq(None::<String>),
            account_bans::active.eq(1),
            account_bans::updated_at.eq(now),
            account_bans::actor.eq(actor),
            account_bans::expires_at.eq(Some(expires)),
        ))
        .on_conflict(account_bans::account_id)
        .do_update()
        .set((
            account_bans::reason.eq(&reason),
            account_bans::address.eq(None::<String>),
            account_bans::active.eq(1),
            account_bans::updated_at.eq(now),
            account_bans::actor.eq(actor),
            account_bans::expires_at.eq(Some(expires)),
        ))
        .execute(conn)?;
    diesel::insert_into(moderation_events::table)
        .values((
            moderation_events::account_id.eq(account),
            moderation_events::reason.eq(&reason),
            moderation_events::address.eq(None::<String>),
            moderation_events::active.eq(1),
            moderation_events::created_at.eq(now),
            moderation_events::actor.eq(actor),
            moderation_events::expires_at.eq(Some(expires)),
        ))
        .execute(conn)?;
    Ok(Detection {
        blocked: true,
        suspended: true,
    })
}
