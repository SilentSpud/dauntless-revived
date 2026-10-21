//! The two routes (`app.ts`, `routes/*.ts`). The deploy server has no authentication: only the
//! metagame on this machine may talk to it, straight over loopback. A caller elsewhere, or anything
//! relayed by a proxy, gets 403.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

use crate::capacity::DeployError;
use crate::matchmaker::{Matchmaker, check_request};
use crate::remote::{Region, overflow_url, read_worker_snapshot, region_url};

/// 127.0.0.0/8 and ::1, also as IPv4-mapped IPv6.
pub fn is_loopback_address(address: Option<&str>) -> bool {
    let Some(address) = address else {
        return false;
    };
    let lower = address.to_ascii_lowercase();
    let v4 = lower.strip_prefix("::ffff:").unwrap_or(&lower);
    let parts: Vec<&str> = v4.split('.').collect();
    let dotted = parts.len() == 4
        && parts[0] == "127"
        && parts
            .iter()
            .all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()));
    dotted || address == "::1"
}

/// Headers any proxy (the public-mode gateway included) adds. The metagame never sends them.
const PROXY_HEADERS: [&str; 7] = [
    "x-dauntless-gateway",
    "x-forwarded-for",
    "forwarded",
    "x-real-ip",
    "x-forwarded-host",
    "x-forwarded-proto",
    "via",
];

pub fn is_direct_loopback(peer: Option<&str>, headers: &HeaderMap) -> bool {
    is_loopback_address(peer) && !PROXY_HEADERS.iter().any(|h| headers.contains_key(*h))
}

/// How Node prints a socket's address: IPv4 peers of a dual-stack socket as `::ffff:a.b.c.d`.
fn peer_text(peer: &SocketAddr) -> String {
    peer.ip().to_string()
}

pub fn router(matchmaker: Arc<Matchmaker>) -> Router {
    Router::new()
        .route("/gameservers", get(gameservers))
        .route("/api/matchmaker/handle-matchmaking-for-player", post(matchmaking))
        .with_state(matchmaker)
}

fn json_reply(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body.to_string(),
    )
        .into_response()
}

/// The running game servers, read-only, for the metagame's ServerStatus (account ids inside).
async fn gameservers(
    State(matchmaker): State<Arc<Matchmaker>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !is_direct_loopback(Some(&peer_text(&peer)), &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let deploy = &matchmaker.deploy;
    let settings = &deploy.settings;
    let http = &matchmaker.remote.http;
    let snapshot = |url: Option<url::Url>, host: &'static str, region: Option<&'static str>| {
        let http = http.clone();
        async move {
            let Some(url) = url else {
                return crate::remote::Snapshot {
                    servers: Vec::new(),
                    complete: true,
                };
            };
            let mut snapshot = read_worker_snapshot(&http, &url).await;
            for server in &mut snapshot.servers {
                if let Some(fields) = server.as_object_mut() {
                    fields.insert("host".into(), json!(host));
                    if let Some(region) = region {
                        fields.insert("region".into(), json!(region));
                    }
                }
            }
            snapshot
        }
    };
    let urls = (
        overflow_url(settings),
        region_url(settings, Region::Ger),
        region_url(settings, Region::Us),
    );
    let (Ok(overflow), Ok(germany), Ok(us)) = urls else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let (overflow, aus, germany, us) = tokio::join!(
        snapshot(overflow, "overflow", None),
        snapshot(None, "aus", Some("aus")),
        snapshot(germany, "ger", Some("ger")),
        snapshot(us, "us", Some("us")),
    );
    let mut servers = Vec::new();
    let local = deploy.gameservers();
    for mut described in deploy.describe() {
        let record = local.iter().find(|s| described["id"] == s.id.as_str());
        let players = record.and_then(|s| deploy.connected_players(s));
        if let (Some(fields), Some(players)) = (described.as_object_mut(), players) {
            fields.insert("connectedPlayers".into(), json!(players));
        }
        servers.push(described);
    }
    let complete = overflow.complete && aus.complete && germany.complete && us.complete;
    for snapshot in [overflow, aus, germany, us] {
        servers.extend(snapshot.servers);
    }
    let Ok(status) = deploy.hunt_admission.status() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut capacity = serde_json::to_value(status).unwrap_or(Value::Null);
    capacity["cpu"] = serde_json::to_value(deploy.cpu_admission.status()).unwrap_or(Value::Null);
    json_reply(
        StatusCode::OK,
        json!({ "servers": servers, "complete": complete, "capacity": capacity }),
    )
}

/// Starts game server processes, with no authentication: the metagame on this machine only.
async fn matchmaking(
    State(matchmaker): State<Arc<Matchmaker>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_direct_loopback(Some(&peer_text(&peer)), &headers) {
        tracing::warn!(
            "Refusing a matchmaking call from {}: only the metagame on this machine may call it",
            peer_text(&peer)
        );
        return StatusCode::FORBIDDEN.into_response();
    }
    // express.json(): a JSON body when the request says it is one
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("application/json"));
    let body: Value = if is_json && !body.is_empty() {
        match serde_json::from_slice(&body) {
            Ok(body) => body,
            Err(_) => return (StatusCode::BAD_REQUEST, "Bad Request").into_response(),
        }
    } else {
        json!({})
    };
    let body = if body.is_object() { body } else { json!({}) };
    let region_text = body["Region"].as_str().map(str::to_owned);
    let region = match (&body["Region"], region_text.as_deref().and_then(Region::parse)) {
        (Value::Null, _) => Region::Main,
        (_, Some(region)) => region,
        _ => return json_reply(StatusCode::BAD_REQUEST, json!({ "error": "invalid_region" })),
    };
    let field = |name: &str| body.get(name).filter(|v| !v.is_null());
    if let Some(problem) = check_request(
        field("GameMode"),
        field("GameArgs"),
        field("HuntId"),
        field("ExpectedPlayers"),
    ) {
        tracing::warn!("Refusing a matchmaking call: {problem}");
        return json_reply(
            StatusCode::BAD_REQUEST,
            json!({ "error": "bad_request", "message": problem }),
        );
    }
    match matchmaker.handle(&body, region).await {
        Ok(connection) => json_reply(StatusCode::OK, serde_json::to_value(connection).unwrap_or(Value::Null)),
        Err(DeployError::Capacity(reason)) => {
            let mut reply = json_reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({ "error": "capacity_unavailable", "reason": reason.as_str() }),
            );
            reply
                .headers_mut()
                .insert(header::RETRY_AFTER, header::HeaderValue::from_static("10"));
            reply
        }
        Err(error) => {
            // No free port, or a game server that could not be started: the metagame answers the
            // player FAILED
            let mode = body["GameMode"].as_str().unwrap_or("undefined");
            let hunt = body["HuntId"].as_str().unwrap_or("");
            tracing::error!("Matchmaking for {mode} {hunt} failed: {error}");
            json_reply(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "no_game_server" }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_callers_only() {
        for address in ["127.0.0.1", "127.8.9.10", "::1", "::ffff:127.0.0.1"] {
            assert!(is_loopback_address(Some(address)), "{address}");
        }
        for address in [
            "100.64.0.7",
            "192.168.1.20",
            "::ffff:100.101.102.103",
            "fe80::1",
            "0.0.0.0",
            "",
        ] {
            assert!(!is_loopback_address(Some(address)), "{address}");
        }
        assert!(!is_loopback_address(None));
        let mut headers = HeaderMap::new();
        assert!(is_direct_loopback(Some("127.0.0.1"), &headers));
        headers.insert("x-real-ip", "127.0.0.1".parse().unwrap());
        assert!(!is_direct_loopback(Some("127.0.0.1"), &headers));
    }
}
