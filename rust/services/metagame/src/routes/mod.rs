//! The metagame's routes, one module per Node route file, merged in the order `app.ts` mounts them.

pub mod admin_accounts;
pub mod admin_history;
pub mod backup;
pub mod character;
pub mod eos;
pub mod escalation;
pub mod friends;
pub mod guild;
pub mod inventory;
pub mod keyrotation;
pub mod loadout;
pub mod login;
pub mod matchmaking;
pub mod party;
pub mod progression;
pub mod slayerlinks;
pub mod store;
pub mod system;
pub mod trials;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use service_kit::json::Json;

use crate::app::AppState;

/// `res.status(n).send()`: the status and no body.
pub fn empty(status: StatusCode) -> Response {
    status.into_response()
}

/// `res.sendStatus(n)`: the status's name as text.
pub fn send_status(status: StatusCode) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        status.canonical_reason().unwrap_or("").to_owned(),
    )
        .into_response()
}

/// `res.status(n).json(value)`.
pub fn json(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}

/// `res.json(value)`.
pub fn ok(value: Value) -> Response {
    Json(value).into_response()
}

/// A field of a JSON body, as `req.body?.field` reads it (`None` when the body or field is missing).
pub fn field<'a>(body: &'a Option<Value>, name: &str) -> Option<&'a Value> {
    body.as_ref().and_then(|body| body.get(name))
}

/// The query string as Express 5's simple parser gives it: repeated keys become arrays.
pub fn query(raw: Option<&str>) -> serde_json::Map<String, Value> {
    match raw.map(service_kit::body::parse_form) {
        Some(Ok(Value::Object(map))) => map,
        _ => serde_json::Map::new(),
    }
}

/// What Express does when a route passes the request on (`next("route")`) and nothing else matches:
/// the catch-all "Unstubbed route" 404.
pub fn fall_through(parts: &axum::http::request::Parts) -> Response {
    note_unhandled_progression(&parts.method, &parts.uri, &parts.headers);
    tracing::warn!("Unstubbed route {} {}", parts.method, parts.uri.path());
    empty(StatusCode::NOT_FOUND)
}

/// `progressionRouter.use("/progression", …)`: anything under /progression no route answered is
/// named before the catch-all 404 (several progression endpoints of the client are still
/// unanswered, and stub-mode accounts get 404 on the real-mode-only routes). From Harmonic's fork
/// (github.com/Harmonicrain/Undaunted 895f7c7).
pub fn note_unhandled_progression(method: &axum::http::Method, uri: &axum::http::Uri, headers: &axum::http::HeaderMap) {
    let path = uri.path().to_ascii_lowercase();
    if path != "/progression" && !path.starts_with("/progression/") {
        return;
    }
    let shown = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
    tracing::warn!(
        "Unhandled progression request {method} {} from a {}",
        crate::redact::redact(shown),
        if headers.contains_key("x-undaunted-gameserver-apikey") {
            "game server"
        } else {
            "player"
        }
    );
}

/// Every route of the metagame ported so far, in Node's mount order.
pub fn all(state: &AppState) -> service_kit::routing::Router<AppState> {
    service_kit::routing::Router::new()
        .merge(login::routes())
        .merge(keyrotation::routes(state))
        .merge(backup::routes())
        .merge(eos::routes(state))
        .merge(system::routes())
        .merge(escalation::routes())
        .merge(friends::routes())
        .merge(character::routes())
        .merge(inventory::routes())
        .merge(store::routes(state))
        .merge(guild::routes())
        .merge(trials::routes())
        .merge(matchmaking::routes())
        .merge(party::routes())
        .merge(slayerlinks::routes())
        .merge(progression::routes(state))
        .merge(loadout::routes())
        .merge(admin_accounts::routes(state))
        .merge(admin_history::routes())
}
