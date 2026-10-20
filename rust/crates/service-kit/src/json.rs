//! JSON replies as Express's `res.json` sends them (MIGRATION_PLAN.md M7, SB1): the body is
//! `serde_json`'s compact text and the type is `application/json; charset=utf-8` (axum's own
//! `Json` leaves the charset out). No ETag (D16) and no `X-Powered-By`.

use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

pub const JSON_UTF8: &str = "application/json; charset=utf-8";

pub struct Json<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        match serde_json::to_vec(&self.0) {
            Ok(body) => ([(CONTENT_TYPE, HeaderValue::from_static(JSON_UTF8))], body).into_response(),
            Err(error) => {
                tracing::error!(err = &error as &dyn std::error::Error, "Could not serialize a reply");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// The reply of a request no route answers: 404 with an empty body, like the Node services' catch-all.
pub fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}
