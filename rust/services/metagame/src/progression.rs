//! What the real-mode routes share (`controllers/progressionmode.ts`, `controllers/progressionevents.ts`,
//! `middleware/GameServerOnly.ts`, `middleware/RealProgressionOnly.ts`): which accounts get real
//! progression, the audit row of every real-mode write, the lenient readers of request fields, and the
//! game-server-only refusal.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::Response;
use diesel::prelude::*;
use serde_json::Value;

use crate::schema::{progression_events, users};
use crate::time::iso_now;

/// `PROGRESSION_MODE` and `PROGRESSION_REAL_ACCOUNTS`: real for everyone unless the mode is `stub`,
/// which keeps upstream's fake ranks for every account not listed.
#[derive(Debug, Clone, Default)]
pub struct ProgressionMode {
    stub: bool,
    real_accounts: HashSet<String>,
    unrecognised: Option<String>,
    default: bool,
}

impl ProgressionMode {
    pub fn new(mode: Option<&str>, accounts: Option<&str>) -> Self {
        let lowered = mode.unwrap_or("").trim().to_lowercase();
        let known = lowered.is_empty() || lowered == "stub" || lowered == "real";
        Self {
            stub: lowered == "stub",
            real_accounts: accounts
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect(),
            unrecognised: (!known).then(|| mode.unwrap_or("").to_owned()),
            default: lowered.is_empty(),
        }
    }

    /// `IsProgressionModeStub`.
    pub fn is_stub(&self) -> bool {
        self.stub
    }

    /// `IsRealProgressionAccount`.
    pub fn is_real(&self, account: Option<&str>) -> bool {
        match account {
            Some(id) if !id.is_empty() => !self.stub || self.real_accounts.contains(id),
            _ => false,
        }
    }

    /// The warning Node logs for an unknown mode, if any.
    pub fn warning(&self) -> Option<String> {
        self.unrecognised.as_ref().map(|raw| {
            format!("PROGRESSION_MODE={raw} is not \"real\" or \"stub\"; using real (the default). Set PROGRESSION_MODE=stub for upstream's fake max ranks")
        })
    }

    /// `DescribeProgressionMode`, for the start log.
    pub fn describe(&self) -> String {
        if self.stub {
            return format!("stub, real for {} listed account(s)", self.real_accounts.len());
        }
        let ignored = if self.real_accounts.is_empty() {
            ""
        } else {
            "; PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode"
        };
        let why = if self.default {
            " (the default)".to_owned()
        } else {
            self.unrecognised.as_ref().map_or(String::new(), |raw| {
                format!(" (PROGRESSION_MODE={raw} is not recognised)")
            })
        };
        format!("real for every account{why}{ignored}")
    }
}

/// Who made a real-mode request, for the audit row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    GameServer,
    Client,
    Admin,
}

impl Caller {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GameServer => "gameserver",
            Self::Client => "client",
            Self::Admin => "admin",
        }
    }
}

/// One audit row (`RecordProgressionEvent`).
pub struct Event<'a> {
    pub account: &'a str,
    pub caller: &'a str,
    pub route: &'a str,
    pub body: Option<&'a Value>,
    pub status: i64,
    pub reply: Option<&'a Value>,
    pub notes: &'a [String],
}

pub fn record_event(conn: &mut store::Connection, event: Event<'_>) -> QueryResult<()> {
    let stringify = |value: Option<&Value>| value.map(service_kit::jsjson::stringify);
    diesel::insert_into(progression_events::table)
        .values((
            progression_events::time.eq(iso_now()),
            progression_events::accountId.eq(event.account),
            progression_events::caller.eq(event.caller),
            progression_events::route.eq(event.route),
            progression_events::body.eq(stringify(event.body)),
            progression_events::status.eq(event.status),
            progression_events::reply.eq(stringify(event.reply)),
            progression_events::note.eq((!event.notes.is_empty()).then(|| event.notes.join("; "))),
        ))
        .execute(conn)
        .map(drop)
}

/// JavaScript's `String(number)`.
fn js_number(n: f64) -> String {
    service_kit::jsjson::number(n)
}

/// `ReadInteger`: an integer from a JSON number (a fraction is truncated, noted) or from a string of
/// digits (noted).
pub fn read_integer(value: Option<&Value>, what: &str, notes: &mut Vec<String>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) => {
            let n = n.as_f64()?;
            if !n.is_finite() {
                return None;
            }
            if n.fract() != 0.0 {
                notes.push(format!("{what} {} is not a whole number, truncated", js_number(n)));
            }
            Some(n.trunc() as i64)
        }
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            notes.push(format!("{what} came as the string \"{text}\""));
            trimmed.parse::<f64>().ok().map(|n| n as i64)
        }
        _ => None,
    }
}

/// `ReadField`: the first key present; any but the first is noted.
pub fn read_field<'a>(
    source: Option<&'a Value>,
    keys: &[&str],
    what: &str,
    notes: &mut Vec<String>,
) -> Option<&'a Value> {
    let source = source?;
    for (index, key) in keys.iter().enumerate() {
        if let Some(value) = source.get(*key) {
            if index > 0 {
                notes.push(format!("{what} came as \"{key}\", not \"{}\"", keys[0]));
            }
            return Some(value);
        }
    }
    None
}

/// `DoesAccountExist`.
pub fn account_exists(conn: &mut store::Connection, account: &str) -> QueryResult<bool> {
    Ok(users::table
        .find(account)
        .select(users::userId)
        .first::<String>(conn)
        .optional()?
        .is_some())
}

/// `ParsePathInteger`: a decimal int32 from a URL segment.
pub fn parse_path_integer(value: &str, allow_negative: bool) -> Option<i64> {
    let digits = if allow_negative {
        value.strip_prefix('-').unwrap_or(value)
    } else {
        value
    };
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let parsed: i64 = value.parse().ok()?;
    (-2_147_483_648..=2_147_483_647).contains(&parsed).then_some(parsed)
}

/// `RefuseUnlessGameserver`: a player's own client may not make these writes (403); a game server's
/// request that carries another player's token is noted, at most once a minute per write and pair.
pub fn refuse_unless_game_server(
    auth: &crate::auth::MetagameAuth,
    account: Option<&str>,
    what: &str,
    subject: &str,
) -> Option<Response> {
    if auth.is_game_server() {
        note_relayed_mismatch(auth, account, what);
        return None;
    }
    tracing::warn!("Refusing {what} for {subject} from a player client");
    Some(crate::routes::empty(StatusCode::FORBIDDEN))
}

/// `NoteRelayedAccountMismatch`.
pub fn note_relayed_mismatch(auth: &crate::auth::MetagameAuth, account: Option<&str>, what: &str) -> bool {
    static LAST_NOTED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let Some(relayed) = auth.user_id().filter(|id| !id.is_empty()) else {
        return false;
    };
    if !auth.is_game_server() || Some(relayed) == account {
        return false;
    }
    let shown = account.unwrap_or("undefined");
    let key = format!("{what}|{shown}|{relayed}");
    let mut guard = LAST_NOTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let noted = guard.get_or_insert_with(HashMap::new);
    let due = noted.get(&key).is_none_or(|last| last.elapsed().as_secs() >= 60);
    if due {
        if noted.len() >= 1000 {
            noted.clear();
        }
        noted.insert(key, Instant::now());
        tracing::warn!(
            "Game server {what} for {shown} carries the token of {relayed}: accepted for {shown}, the account the request names"
        );
    }
    true
}

/// `SendRealReply`.
pub fn real_reply(status: u16, body: Option<Value>) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match body {
        Some(body) => crate::routes::json(status, body),
        None => crate::routes::empty(status),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn modes_read_like_node() {
        assert!(ProgressionMode::new(None, None).is_real(Some("UID-a")));
        let stub = ProgressionMode::new(Some(" STUB "), Some("UID-a, UID-b"));
        assert!(stub.is_real(Some("UID-b")) && !stub.is_real(Some("UID-c")) && !stub.is_real(None));
        assert_eq!(stub.describe(), "stub, real for 2 listed account(s)");
        assert_eq!(
            ProgressionMode::new(None, None).describe(),
            "real for every account (the default)"
        );
        assert!(ProgressionMode::new(Some("off"), None).warning().is_some());
    }

    #[test]
    fn lenient_readers() {
        let mut notes = Vec::new();
        assert_eq!(read_integer(Some(&json!(2.7)), "duration", &mut notes), Some(2));
        assert_eq!(read_integer(Some(&json!(" -12 ")), "duration", &mut notes), Some(-12));
        assert_eq!(read_integer(Some(&json!("1.5")), "duration", &mut notes), None);
        assert_eq!(
            notes,
            [
                "duration 2.7 is not a whole number, truncated",
                "duration came as the string \" -12 \""
            ]
        );
        assert_eq!(parse_path_integer("2147483648", false), None);
        assert_eq!(parse_path_integer("-3", false), None);
        assert_eq!(parse_path_integer("-3", true), Some(-3));
        assert_eq!(parse_path_integer("007", false), Some(7));
    }
}
