//! `LOG_BODIES=1` (`middleware/BodyLog.ts`): one JSON object per line in `BODY_LOG_FILE` (default
//! `bodies.log`) for the routes whose bodies are worth recording (the save routes that are still
//! stubbed or missing, the social lookups, the store, Trials, Slayer Link and Escalation routes):
//!
//! `{t, method, url, gs, body, status, ms}`, and `"aborted": true` when the connection closed before
//! the answer was sent. `t` is when the request arrived, `url` has the query string, `gs` is 1 for a
//! game server's call, `body` is capped at 8 KB (64 KB for `/inventory`). Credentials are removed
//! from the url and the body (`redact`). `BODY_LOG_PER_PATH` caps the lines per method and path.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::env::Env;

use crate::redact::redact;

const MAX_PATHS: usize = 10_000;

/// The prefixes of `BODY_ROUTES`, after the leading `/`.
const BODY_ROUTES: [&str; 23] = [
    "progression",
    "huntpass",
    "bounty",
    "cooldown",
    "escalation",
    "entitlement",
    "product",
    "token/",
    "notification/",
    "reconcile",
    "slayerlink",
    "candidate",
    "party",
    "friends",
    "guild",
    "balance",
    "store",
    "inventory",
    "trials/",
    "profile/update",
    "account/api/public/account",
    "account/mapping",
    "accountinfo/public",
];

/// `BODY_ROUTES.test(path)`, with its `loadout/[^/]+/[^/]+/unlock` too.
fn is_body_route(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    if BODY_ROUTES.iter().any(|prefix| rest.starts_with(prefix)) {
        return true;
    }
    let mut segments = rest.splitn(4, '/');
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some("loadout"), Some(a), Some(b), Some(last)) if !a.is_empty() && !b.is_empty() && last.starts_with("unlock")
    )
}

/// `LOG_BODIES=1`'s settings, read once at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub file: PathBuf,
    /// `BODY_LOG_PER_PATH` (0: no cap).
    pub per_path: u64,
}

impl Settings {
    /// `None` unless `LOG_BODIES=1`.
    pub fn from_env(env: &Env) -> Option<Self> {
        if env.raw("LOG_BODIES") != Some("1") {
            return None;
        }
        let file = env
            .raw("BODY_LOG_FILE")
            .filter(|f| !f.is_empty())
            .unwrap_or("bodies.log");
        let per_path = env
            .get("BODY_LOG_PER_PATH")
            .filter(|v| (1..=9).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Some(Self {
            file: file.into(),
            per_path,
        })
    }
}

/// The body log and its lines-per-path counts.
#[derive(Debug)]
pub struct BodyLog {
    file: PathBuf,
    per_path: u64,
    written: Mutex<HashMap<String, u64>>,
    /// Lines are appended one at a time, in the order the answers finish.
    writing: Arc<Mutex<()>>,
}

impl BodyLog {
    pub fn new(settings: &Settings) -> Self {
        Self {
            file: settings.file.clone(),
            per_path: settings.per_path,
            written: Mutex::default(),
            writing: Arc::default(),
        }
    }

    /// Whether this request may still be logged under the cap.
    fn take_line(&self, method: &str, path: &str) -> bool {
        if self.per_path == 0 {
            return true;
        }
        let key = format!("{method} {}", redact(path));
        let mut written = self.written.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = written.get(&key).copied().unwrap_or(0);
        if count >= self.per_path {
            return false;
        }
        if written.len() >= MAX_PATHS {
            written.clear();
        }
        written.insert(key.clone(), count + 1);
        if count + 1 == self.per_path {
            tracing::info!(
                "body log: {key} reached BODY_LOG_PER_PATH={}; no more lines for it in this run",
                self.per_path
            );
        }
        true
    }

    fn append(&self, line: String) {
        let (file, writing) = (self.file.clone(), self.writing.clone());
        tokio::task::spawn_blocking(move || {
            let _turn = writing.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let result = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file)
                .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
            if let Err(error) = result {
                tracing::warn!("body log write failed: {error}");
            }
        });
    }
}

/// The first `limit` UTF-16 units of `text`, as JavaScript's `slice` cuts it (a cut pair goes).
fn truncate_utf16(text: &str, limit: usize) -> Option<String> {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > limit {
            return Some(text[..at].to_owned());
        }
    }
    None
}

/// The request's line until its answer is done; dropped without one (the client went away), it is
/// written with `"aborted": true`.
struct Pending {
    log: Arc<BodyLog>,
    entry: serde_json::Map<String, Value>,
    started: Instant,
    status: Option<u16>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        let mut entry = std::mem::take(&mut self.entry);
        entry.insert("status".into(), json!(self.status.unwrap_or(200)));
        entry.insert("ms".into(), json!(self.started.elapsed().as_millis() as u64));
        if self.status.is_none() {
            entry.insert("aborted".into(), json!(true));
        }
        self.log.append(Value::Object(entry).to_string());
    }
}

/// Middleware, inside the body parser (it logs the parsed body).
pub async fn body_log(State(log): State<Arc<BodyLog>>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if !is_body_route(&path) || !log.take_line(request.method().as_str(), &path) {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let parsed = <ExpressBody as axum::extract::FromRequestParts<()>>::from_request_parts(&mut parts, &())
        .await
        .map(|ExpressBody(b)| b)
        .unwrap_or(None);
    let request = Request::from_parts(parts, body);
    let mut text = redact(&parsed.unwrap_or(Value::Null).to_string());
    let limit = if path.starts_with("/inventory") { 65536 } else { 8192 };
    if let Some(cut) = truncate_utf16(&text, limit) {
        text = format!("{cut}…<truncated>");
    }
    let url = request
        .uri()
        .path_and_query()
        .map_or_else(|| path.clone(), |p| p.as_str().to_owned());
    let gs = request
        .headers()
        .get("x-undaunted-gameserver-apikey")
        .is_some_and(|v| !v.is_empty());
    let mut entry = serde_json::Map::new();
    entry.insert("t".into(), json!(crate::time::iso_now()));
    entry.insert("method".into(), json!(request.method().as_str()));
    entry.insert("url".into(), json!(redact(&url)));
    entry.insert("gs".into(), json!(u8::from(gs)));
    entry.insert("body".into(), json!(text));
    let mut pending = Pending {
        log,
        entry,
        started: Instant::now(),
        status: None,
    };
    let response = next.run(request).await;
    pending.status = Some(response.status().as_u16());
    drop(pending);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routes_are_nodes() {
        for path in [
            "/progression/UID-a",
            "/token/platinum/X",
            "/loadout/a/b/unlock",
            "/loadout/a/b/unlocked/x",
            "/trials/leaderboards",
        ] {
            assert!(is_body_route(path), "{path}");
        }
        for path in [
            "/playertreatments/UID-a",
            "/token",
            "/loadout/a/b",
            "/loadout//b/unlock",
            "/Progression/x",
        ] {
            assert!(!is_body_route(path), "{path}");
        }
    }

    #[test]
    fn bodies_are_cut_by_utf16_units() {
        assert_eq!(truncate_utf16("abc", 3), None);
        assert_eq!(truncate_utf16("abcd", 3), Some("abc".into()));
        assert_eq!(truncate_utf16("a😀b", 2), Some("a".into()));
    }
}
