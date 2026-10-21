//! `routes/keyrotation.ts`: `/internal/key-recovery/:action` for the Discord bot on this host, behind
//! `KEY_RECOVERY_SERVICE_SECRET` (at least 32 characters) sent as `x-key-recovery-service`, from
//! loopback and never through a proxy. Every answer of the route itself is `no-store`. State:
//! `keyrotation.rs`.

use axum::extract::{ConnectInfo, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::enforce;
use service_kit::routing::{Params, Router};
use service_kit::serve::ConnectionInfo;

use super::{field, json};
use crate::app::{AppError, AppState};
use crate::crypto::{hashes_equal, sha256_hex};
use crate::keyrotation;
use crate::limits::per_client;
use crate::origin::{is_loopback, is_proxied};
use crate::time::now_ms;

pub fn routes(state: &AppState) -> Router<AppState> {
    let admin_limit = per_client(&state.limits.admin_mutation, state.origin.clone());
    Router::new()
        .post("/internal/key-recovery/:action", recovery)
        .layer(from_fn_with_state(admin_limit, enforce))
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `KeyRecoveryAuth`.
fn authorized(state: &AppState, parts: &Parts) -> bool {
    let Some(expected) = state
        .key_recovery_secret
        .as_deref()
        .filter(|s| s.encode_utf16().count() >= 32)
    else {
        return false;
    };
    let loopback = parts
        .extensions
        .get::<ConnectInfo<ConnectionInfo>>()
        .is_some_and(|info| is_loopback(info.0.peer.ip()));
    let presented = parts
        .headers
        .get("x-key-recovery-service")
        .and_then(|v| v.to_str().ok());
    loopback
        && !is_proxied(&parts.headers)
        && presented.is_some_and(|presented| hashes_equal(&sha256_hex(expected), &sha256_hex(presented)))
}

async fn recovery(
    State(state): State<AppState>,
    parts: Parts,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Response {
    let unavailable = |status: StatusCode| no_store(json(status, json!({"error": "recovery_unavailable"})));
    if !authorized(&state, &parts) {
        return unavailable(StatusCode::FORBIDDEN);
    }
    let text = |name: &str| field(&body, name).and_then(Value::as_str).map(str::to_owned);
    let discord = text("discordId");
    let now = now_ms();
    let answer: Result<Value, AppError> = match params.get("action").unwrap_or("") {
        "link" => {
            let key = text("key");
            state
                .store
                .write(move |conn| {
                    keyrotation::prove_link(conn, discord.as_deref(), key.as_deref(), now).map_err(AppError::from)
                })
                .await
                .map(|verified| json!({"verified": verified}))
        }
        "begin" => state
            .store
            .write(move |conn| keyrotation::begin(conn, discord.as_deref(), now).map_err(AppError::from))
            .await
            .map(|challenge| json!({"challenge": challenge})),
        "confirm" => {
            let challenge = text("challenge");
            let result = state
                .store
                .write(move |conn| {
                    keyrotation::complete(conn, discord.as_deref(), challenge.as_deref(), now).map_err(AppError::from)
                })
                .await;
            result.map(|rotated| {
                let key = rotated.map(|(key, user)| {
                    // Chat closes the account's sessions; nobody listening is fine
                    let _ = state.key_revocations.send(user);
                    key
                });
                json!({"key": key})
            })
        }
        _ => return unavailable(StatusCode::NOT_FOUND),
    };
    match answer {
        Ok(reply) => no_store(json(StatusCode::OK, reply)),
        Err(error) => {
            tracing::error!("key recovery failed: {error:?}");
            unavailable(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}
