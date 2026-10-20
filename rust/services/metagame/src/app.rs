//! The metagame's shared state and error reply.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::crypto::TokenKeys;
use crate::health::Health;
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
    pub features: crate::config::Features,
    /// `REGISTRATION_MODE` as set, changed at runtime by POST /undaunted/api/RegistrationStatus.
    pub registration_mode: std::sync::RwLock<Option<String>>,
    pub limits: crate::limits::Limits,
    pub store_images: crate::storeimages::StoreImages,
    pub progression: crate::progression::ProgressionMode,
    /// The Hunt Pass seasons and mastery tracks.
    pub progression_config: Arc<crate::progression_config::ProgressionConfig>,
    pub real_progression: crate::realprogression::Settings,
    /// `ENTITLEMENTS_DEFAULT`.
    pub entitlement_defaults: Vec<String>,
    pub history: crate::characters::HistorySettings,
    pub inventory: crate::inventory::Settings,
    pub shop: crate::store::Settings,
    /// When the store purchase table was last swept.
    pub store_sweeps: crate::store::Sweeps,
    /// Each account's recent friend requests, for the request window.
    pub friend_requests: crate::friends::RecentRequests,
    pub health: Arc<Health>,
    /// Accounts whose key was rotated (M28, `KeyRevocations`): chat closes their sessions.
    pub key_revocations: tokio::sync::broadcast::Sender<String>,
}

pub type AppState = Arc<App>;

impl App {
    /// What a real-mode progression write needs.
    pub fn progression_context(&self) -> crate::realprogression::Context {
        crate::realprogression::Context {
            settings: self.real_progression.clone(),
            config: self.progression_config.clone(),
            entitlement_defaults: self.entitlement_defaults.clone(),
        }
    }

    /// What a store call needs, at this moment.
    pub fn store_context(&self) -> crate::store::Context {
        crate::store::Context {
            settings: self.shop.clone(),
            entitlement_defaults: self.entitlement_defaults.clone(),
            inventory: self.inventory,
            now: crate::time::now_ms(),
        }
    }
}

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
