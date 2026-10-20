//! The metagame's shared state and error reply.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::crypto::TokenKeys;
use crate::origin::{OriginConfig, OwnAddresses};

/// `AUTH_MODE`: account keys checked against the database, or (development only, never with
/// `NODE_ENV=production`) any account id accepted as a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    ApiKey,
    None,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub production: bool,
    /// `ACCOUNT_KEY_RECOVERY=1`: keep an encrypted copy of each player's key.
    pub account_key_recovery: bool,
    /// `AUTH_SIGNING_PRIVKEY_B64`, which also derives the recovery key.
    pub signing_key_b64: String,
}

impl AuthConfig {
    /// Development logins: `AUTH_MODE=NONE` outside production.
    pub fn dev_logins(&self) -> bool {
        self.mode == AuthMode::None && !self.production
    }
}

pub struct App {
    pub store: store::Store,
    pub tokens: TokenKeys,
    pub auth: AuthConfig,
    pub origin: OriginConfig,
    pub own_addresses: OwnAddresses,
}

pub type AppState = Arc<App>;

/// A failure the request can't recover from: Express's 500 page, and the cause in the log.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Store(#[from] store::Error),
    #[error("database: {0}")]
    Query(#[from] diesel::result::Error),
    #[error("{0}")]
    Other(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(err = &self as &dyn std::error::Error, "Request failed");
        service_kit::body::error_page(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
