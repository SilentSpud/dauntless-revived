//! `controllers/discordstats.ts`: how many accounts are linked to Discord, and what became of the
//! invite codes the Discord bot handed out (from the bot's state file, `DISCORD_KEY_STATE_FILE`).

use std::collections::{HashMap, HashSet};

use diesel::dsl::count_star;
use diesel::prelude::*;
use serde_json::{Value, json};

use crate::keyrotation::is_discord_id;
use crate::schema::{discordlinks, invitecodes};

pub type Conn = store::Connection;

const MAX_STATE_BYTES: u64 = 5 * 1024 * 1024;

/// `SummarizeBotClaims`: the counts, or why the state can't be trusted.
pub fn summarize(state: &Value, invites: &[(String, i64, bool)]) -> Result<Value, &'static str> {
    if state.get("version").and_then(Value::as_f64) != Some(1.0) {
        return Err("Invalid bot state");
    }
    let Some(users) = state.get("users").and_then(Value::as_object) else {
        return Err("Invalid bot state");
    };
    let rows: HashMap<&str, (i64, bool)> = invites
        .iter()
        .map(|(code, uses, infinite)| (code.as_str(), (*uses, *infinite)))
        .collect();
    let mut codes = HashSet::new();
    let (mut claims, mut issued, mut unused, mut redeemed, mut pending, mut revoked) = (0, 0, 0, 0, 0, 0);
    for (discord, entry) in users {
        let code = entry.get("code").and_then(Value::as_str);
        let is_pending = entry.get("pending").and_then(Value::as_bool);
        let (Some(code), Some(is_pending)) = (code, is_pending) else {
            return Err("Invalid bot claim");
        };
        if !is_discord_id(discord) || !codes.insert(code) {
            return Err("Invalid bot claim");
        }
        claims += 1;
        let Some((uses, infinite)) = rows.get(code) else {
            if is_pending {
                pending += 1;
            } else {
                revoked += 1;
                issued += 1;
            }
            continue;
        };
        if *infinite || !(0..=1).contains(uses) {
            return Err("Invalid bot invite");
        }
        issued += 1;
        if *uses == 0 {
            redeemed += 1;
        } else {
            unused += 1;
        }
    }
    Ok(json!({
        "claims": claims, "issued": issued, "unused": unused,
        "redeemed": redeemed, "pending": pending, "revoked": revoked,
    }))
}

pub fn linked_accounts(conn: &mut Conn) -> QueryResult<i64> {
    discordlinks::table.select(count_star()).first(conn)
}

pub fn invites(conn: &mut Conn) -> QueryResult<Vec<(String, i64, bool)>> {
    invitecodes::table
        .select((
            invitecodes::invitecode,
            invitecodes::usesRemaining,
            invitecodes::infiniteUses,
        ))
        .load(conn)
}

/// Reads the bot's state file: at most 5 MB of JSON.
pub async fn read_state(file: &str) -> Option<Value> {
    if tokio::fs::metadata(file).await.ok()?.len() > MAX_STATE_BYTES {
        return None;
    }
    let text = tokio::fs::read_to_string(file).await.ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_are_counted_like_node() {
        let state = json!({"version": 1, "users": {
            "12345678901234567": {"code": "A", "pending": false},
            "12345678901234568": {"code": "B", "pending": false},
            "12345678901234569": {"code": "C", "pending": true},
            "12345678901234570": {"code": "D", "pending": false},
        }});
        let invites = [("A".to_owned(), 0, false), ("B".to_owned(), 1, false)];
        assert_eq!(
            summarize(&state, &invites),
            Ok(json!({"claims": 4, "issued": 3, "unused": 1, "redeemed": 1, "pending": 1, "revoked": 1}))
        );
        let twice = json!({"version": 1, "users": {
            "12345678901234567": {"code": "A", "pending": false},
            "12345678901234568": {"code": "A", "pending": false},
        }});
        assert!(summarize(&twice, &invites).is_err());
        assert!(summarize(&json!({"version": 2, "users": {}}), &invites).is_err());
        assert!(summarize(&json!({"version": 1, "users": []}), &invites).is_err());
    }
}
