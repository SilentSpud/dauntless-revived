//! The metagame's rate limits (`middleware/RateLimits.ts`, MIGRATION_PLAN.md M11), on
//! `service_kit::ratelimit`. All but `PlayerMutation` count per client address: the player's behind
//! the gateway, else the connection's (never a header a direct caller could choose).
//! `PlayerMutation` counts per account after authentication, so it is applied with the domain
//! routes that use it (M29).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Request};
use serde_json::{Value, json};
use service_kit::ratelimit::{Limited, Limiter, ip_key};
use service_kit::serve::ConnectionInfo;

use crate::origin::{Origin, OriginConfig};

fn reply() -> Value {
    json!({"error": "rate_limited", "message": "Too many requests; try again later."})
}

const MINUTE: Duration = Duration::from_secs(60);

/// The limiters, created once per process.
pub struct Limits {
    pub store_image: Arc<Limiter>,
    pub player_mutation: Arc<Limiter>,
    pub health_read: Arc<Limiter>,
    pub login: Arc<Limiter>,
    pub token_verify: Arc<Limiter>,
    pub admin_mutation: Arc<Limiter>,
    /// Shared by every caller of the backup route (`routes/backup.ts`).
    pub backup: Arc<Limiter>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // A shop page fetches many icons without changing an account.
            store_image: Arc::new(Limiter::new(600, MINUTE, reply())),
            player_mutation: Arc::new(Limiter::new(60, MINUTE, reply())),
            // Dashboard polling can't use up the admin write allowance.
            health_read: Arc::new(Limiter::new(120, MINUTE, reply())),
            // Only failed logins count, so reconnecting clients behind one address don't lock each other out.
            login: Arc::new(Limiter::new(30, 10 * MINUTE, reply()).skip_successful()),
            // The 1.4.4 client verifies its token about every 30 s.
            token_verify: Arc::new(Limiter::new(120, MINUTE, reply())),
            admin_mutation: Arc::new(Limiter::new(120, MINUTE, reply())),
            backup: Arc::new(Limiter::new(
                6,
                60 * MINUTE,
                json!({"error": "rate_limited", "message": "Too many backup requests; try again later."}),
            )),
        }
    }
}

impl Limits {
    pub fn spawn_sweepers(&self) {
        for limiter in [
            &self.store_image,
            &self.player_mutation,
            &self.health_read,
            &self.login,
            &self.token_verify,
            &self.admin_mutation,
            &self.backup,
        ] {
            drop(limiter.spawn_sweeper());
        }
    }
}

/// A limiter counted per client address (`ClientRateLimitKey`).
pub fn per_client(limiter: &Arc<Limiter>, origin: OriginConfig) -> Limited {
    Limited::new(limiter.clone(), move |request: &Request| {
        let connection = request
            .extensions()
            .get::<ConnectInfo<ConnectionInfo>>()
            .map(|info| info.0)?;
        let address = Origin {
            connection,
            headers: request.headers(),
        }
        .client_address(&origin);
        Some(ip_key(&address.to_string()))
    })
}

/// The backup limiter: one budget for the whole host (`keyGenerator: () => 'local-backup'`).
pub fn host_wide(limiter: &Arc<Limiter>) -> Limited {
    Limited::new(limiter.clone(), |_| Some("local-backup".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_budgets_match_node() {
        let limits = Limits::default();
        assert_eq!(limits.store_image.name(), "600-in-1min");
        assert_eq!(limits.login.name(), "30-in-10min");
        assert_eq!(limits.backup.name(), "6-in-1hr");
    }
}
