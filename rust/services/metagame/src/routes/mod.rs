//! The metagame's routes, one module per Node route file, merged in the order `app.ts` mounts them.

pub mod admin_accounts;
pub mod admin_history;
pub mod character;
pub mod eos;
pub mod loadout;
pub mod login;
pub mod system;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use service_kit::json::Json;

use crate::app::AppState;

/// `res.status(n).send()`: the status and no body.
pub fn empty(status: StatusCode) -> Response {
    status.into_response()
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
    tracing::warn!("Unstubbed route {} {}", parts.method, parts.uri.path());
    empty(StatusCode::NOT_FOUND)
}

/// Every route of the metagame ported so far, in Node's mount order.
pub fn all(state: &AppState) -> service_kit::routing::Router<AppState> {
    service_kit::routing::Router::new()
        .merge(login::routes())
        .merge(eos::routes(state))
        .merge(system::routes())
        .merge(character::routes())
        .merge(loadout::routes())
        .merge(admin_accounts::routes(state))
        .merge(admin_history::routes())
}
