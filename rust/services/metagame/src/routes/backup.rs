//! `routes/backup.ts`: POST /internal/backup `{name}` copies the live database to
//! `BACKUP_ROOT/<name>/undaunted.db` for the host's backup job. Only from this machine, with a game
//! server key, and only when `BACKUP_ROOT` is set; then one host-wide budget of six an hour (callers
//! refused before it do not use it up) and one backup at a time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::around;
use service_kit::routing::Router;

use super::{field, ok, send_status};
use crate::app::AppState;
use crate::auth::{is_valid_gameserver_key, origin};

/// Multi-gigabyte production databases need more than a minute on VPS disks.
const DEADLINE: Duration = Duration::from_secs(15 * 60);

/// One backup at a time.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn routes() -> Router<AppState> {
    Router::new().post("/internal/backup", backup)
}

/// `LocalBackupAuth`: the refusal, if any.
async fn refusal(state: &AppState, parts: &Parts) -> Option<Response> {
    if !origin(parts).is_direct_local(&state.origin, &state.own_addresses) {
        return Some(send_status(StatusCode::FORBIDDEN));
    }
    if state.backup_root.is_none() {
        return Some(send_status(StatusCode::NOT_FOUND));
    }
    let key = parts
        .headers
        .get("x-undaunted-gameserver-apikey")
        .and_then(|v| v.to_str().ok());
    let valid = match key {
        Some(key) => match is_valid_gameserver_key(state, key).await {
            Ok(valid) => valid,
            Err(error) => {
                tracing::error!("Internal database backup: the key check failed: {error}");
                return Some(send_status(StatusCode::INTERNAL_SERVER_ERROR));
            }
        },
        None => false,
    };
    (!valid).then(|| send_status(StatusCode::FORBIDDEN))
}

/// `YYYY-MM-DD_HHMMSS`.
fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            10 => *b == b'_',
            _ => b.is_ascii_digit(),
        })
}

async fn backup(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    if let Some(refused) = refusal(&state, &parts).await {
        return refused;
    }
    around(&state.limits.backup, Some("local-backup"), run(&state, body)).await
}

async fn run(state: &AppState, body: Option<Value>) -> Response {
    let root = state.backup_root.clone().unwrap_or_default();
    let Some(name) = field(&body, "name").and_then(Value::as_str).filter(|n| valid_name(n)) else {
        return send_status(StatusCode::BAD_REQUEST);
    };
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return send_status(StatusCode::CONFLICT);
    }
    let reply = copy(state, Path::new(&root), name).await;
    ACTIVE.store(false, Ordering::SeqCst);
    match reply {
        Ok(reply) => reply,
        Err(error) => {
            tracing::error!("Internal database backup failed: {error}");
            send_status(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn copy(state: &AppState, root: &Path, name: &str) -> Result<Response, String> {
    let base = tokio::fs::canonicalize(root).await.map_err(|e| e.to_string())?;
    let requested: PathBuf = base.join(name);
    // The folder must exist and be no link
    if tokio::fs::canonicalize(&requested).await.map_err(|e| e.to_string())? != requested {
        return Ok(send_status(StatusCode::BAD_REQUEST));
    }
    let destination = requested.join("undaunted.db");
    match tokio::fs::symlink_metadata(&destination).await {
        Ok(_) => return Ok(send_status(StatusCode::CONFLICT)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    state
        .store
        .backup(&destination, DEADLINE)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ok(json!({"copied": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_timestamps() {
        assert!(valid_name("2026-10-20_013000"));
        assert!(!valid_name("2026-10-20_01300"));
        assert!(!valid_name("2026-10-20T013000"));
        assert!(!valid_name("../etc/passwd_00"));
    }
}
