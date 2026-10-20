//! The metagame's settings, read once at start from the `--env-file` (MIGRATION_PLAN.md M25, X7).
//! Each area adds its own as it is ported; the names and meanings are the Node metagame's
//! (`.env.example`).
//!
//! Two settings the Node metagame tolerates are required here: `AUTH_MODE` must be `APIKEY` or `NONE`
//! (Node answered no login at all otherwise, leaving the client waiting), and the signing keys must
//! be valid (Node started and failed every login).

use std::net::IpAddr;
use std::path::PathBuf;

use service_kit::env::Env;

use crate::app::AuthMode;
use crate::origin::{OriginConfig, is_loopback, normalize_text};

/// Switches of this area. Node reads them at every use; here at start (they never change at runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Features {
    /// `ACCOUNTINFO_PUBLIC_LEGACY=1`: POST /accountinfo/public answers upstream's way.
    pub accountinfo_public_legacy: bool,
    /// `ACCOUNT_DISPLAY_NAME=0`: displayName is `{}` again, as upstream sent it.
    pub account_display_name: bool,
    /// `VERIFY_STUB_ACCOUNT` (on/off): the token check names the stub account for everyone.
    pub verify_stub_account: bool,
    /// `ACCOUNT_MAPPING=0`: POST /account/mapping maps nothing.
    pub account_mapping: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            accountinfo_public_legacy: false,
            account_display_name: true,
            verify_stub_account: false,
            account_mapping: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// 0: a port the system picks (`PORT=` empty).
    pub port: u16,
    pub bind_host: IpAddr,
    pub production: bool,
    pub auth_mode: AuthMode,
    pub signing_private_b64: String,
    pub signing_public_b64: String,
    /// As set; an unknown value makes registration answer 500, as in Node.
    pub registration_mode: Option<String>,
    pub db_filename: PathBuf,
    pub origin: OriginConfig,
    pub qos_target_url: Option<String>,
    pub account_key_recovery: bool,
    pub backend_health: bool,
    pub log_requests: bool,
    pub store_images_dir: PathBuf,
    pub features: Features,
}

/// `ParseOnOff` of `features.ts`: 1/true/on/yes, 0/false/off/no; anything else is the default.
fn on_off(env: &Env, name: &str, default: bool) -> bool {
    match env.get(name).map(|v| v.trim().to_ascii_lowercase()) {
        None => default,
        Some(v) if ["1", "true", "on", "yes"].contains(&v.as_str()) => true,
        Some(v) if ["0", "false", "off", "no"].contains(&v.as_str()) => false,
        Some(v) => {
            tracing::warn!(
                "{name}={v:?} is not a valid value; using the default ({})",
                if default { "on" } else { "off" }
            );
            default
        }
    }
}

fn raw(env: &Env, name: &str) -> Option<String> {
    env.get(name).map(str::to_owned)
}

/// The start-up messages of a config: errors stop the start, warnings are logged.
#[derive(Debug, Default)]
pub struct Checks {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Config {
    pub fn from_env(env: &Env) -> Result<Self, String> {
        let port = match env.raw("PORT") {
            None => return Err("PORT is not set".into()),
            Some(text) if text.trim().is_empty() => 0,
            Some(text) => text
                .trim()
                .parse()
                .map_err(|_| format!("PORT {text:?} is not a port number"))?,
        };
        let bind_host = match env.get("BIND_HOST") {
            None => IpAddr::from([127, 0, 0, 1]),
            Some(text) => normalize_text(text).ok_or_else(|| format!("BIND_HOST {text:?} is not an IP address"))?,
        };
        let auth_mode = match env.get("AUTH_MODE") {
            Some("APIKEY") => AuthMode::ApiKey,
            Some("NONE") => AuthMode::None,
            other => return Err(format!("AUTH_MODE is {other:?}; it must be APIKEY or NONE")),
        };
        let required = |name: &str| raw(env, name).ok_or_else(|| format!("{name} is not set"));
        let gameserver_allow_from = env
            .get("GAMESERVER_ALLOW_FROM")
            .unwrap_or("")
            .split(',')
            .filter_map(normalize_text)
            .collect();
        Ok(Self {
            port,
            bind_host,
            production: env.get("NODE_ENV") == Some("production"),
            auth_mode,
            signing_private_b64: required("AUTH_SIGNING_PRIVKEY_B64")?,
            signing_public_b64: required("AUTH_SIGNING_PUBKEY_B64")?,
            registration_mode: raw(env, "REGISTRATION_MODE"),
            db_filename: required("DB_FILENAME")?.into(),
            origin: OriginConfig {
                gateway_secret: raw(env, "GATEWAY_SECRET"),
                gameserver_allow_from,
            },
            qos_target_url: raw(env, "QOS_TARGET_URL"),
            account_key_recovery: env.get("ACCOUNT_KEY_RECOVERY") == Some("1"),
            backend_health: env.get("BACKEND_HEALTH") == Some("1"),
            log_requests: env.get("LOG_REQUESTS") != Some("0"),
            store_images_dir: raw(env, "STORE_IMAGES_DIR").map_or_else(|| "store-images".into(), PathBuf::from),
            features: Features {
                accountinfo_public_legacy: env.get("ACCOUNTINFO_PUBLIC_LEGACY") == Some("1"),
                account_display_name: env.get("ACCOUNT_DISPLAY_NAME") != Some("0"),
                verify_stub_account: on_off(env, "VERIFY_STUB_ACCOUNT", false),
                account_mapping: env.get("ACCOUNT_MAPPING") != Some("0"),
            },
        })
    }

    /// `CheckGatewayConfig`: public mode (`GATEWAY_SECRET` set) needs `AUTH_MODE=APIKEY`; the rest
    /// are warnings.
    pub fn gateway_checks(&self) -> Checks {
        let mut checks = Checks::default();
        let Some(secret) = &self.origin.gateway_secret else {
            return checks;
        };
        if self.auth_mode != AuthMode::ApiKey {
            checks
                .errors
                .push("GATEWAY_SECRET is set (public mode) but AUTH_MODE is not APIKEY; refusing to start".into());
        }
        if secret.len() < 16 {
            checks
                .warnings
                .push("GATEWAY_SECRET is shorter than 16 characters; use a long random value".into());
        }
        if !is_loopback(self.bind_host) {
            checks
                .warnings
                .push("GATEWAY_SECRET is set but BIND_HOST is not loopback; in public mode the gateway should be the only way in".into());
        }
        if !self.production {
            checks.warnings.push(
                "GATEWAY_SECRET is set but NODE_ENV is not production; error pages would include stack traces".into(),
            );
        }
        let qos_ok = self.qos_target_url.as_deref().is_some_and(|url| {
            url.strip_prefix("http://127.0.0.1:")
                .and_then(|rest| rest.strip_suffix("/QoS"))
                .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        });
        if !qos_ok {
            checks.warnings.push("GATEWAY_SECRET is set but QOS_TARGET_URL is not http://127.0.0.1:<port>/QoS; in public mode the client pings the launcher's local relay".into());
        }
        checks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        let mut all = vec![
            ("AUTH_MODE", "APIKEY"),
            ("AUTH_SIGNING_PRIVKEY_B64", "x"),
            ("AUTH_SIGNING_PUBKEY_B64", "y"),
            ("DB_FILENAME", "undaunted.db"),
        ];
        all.extend_from_slice(pairs);
        Env::from_pairs(all)
    }

    #[test]
    fn required_settings_and_defaults() {
        let config = Config::from_env(&env(&[("PORT", "61000")])).unwrap();
        assert_eq!((config.port, config.bind_host.to_string()), (61000, "127.0.0.1".into()));
        assert_eq!(config.features, Features::default());
        assert!(config.log_requests && !config.production);
        assert_eq!(Config::from_env(&env(&[("PORT", "")])).unwrap().port, 0);
        assert!(Config::from_env(&env(&[])).is_err());
        assert!(Config::from_env(&env(&[("AUTH_MODE", "OPEN"), ("PORT", "1")])).is_err());
    }

    #[test]
    fn switches_read_like_node() {
        let config = Config::from_env(&env(&[
            ("VERIFY_STUB_ACCOUNT", "Yes"),
            ("ACCOUNT_DISPLAY_NAME", "0"),
            ("ACCOUNT_MAPPING", "false"),
            ("PORT", "1"),
        ]))
        .unwrap();
        assert!(config.features.verify_stub_account);
        assert!(!config.features.account_display_name);
        assert!(
            config.features.account_mapping,
            "only 0 turns ACCOUNT_MAPPING off, as in Node"
        );
    }

    #[test]
    fn public_mode_checks() {
        let config = Config::from_env(&env(&[
            ("GATEWAY_SECRET", "short"),
            ("AUTH_MODE", "NONE"),
            ("PORT", "1"),
        ]))
        .unwrap();
        let checks = config.gateway_checks();
        assert_eq!(checks.errors.len(), 1);
        assert_eq!(checks.warnings.len(), 3, "{:?}", checks.warnings);
        let good = Config::from_env(&env(&[
            ("GATEWAY_SECRET", "a-long-enough-secret"),
            ("NODE_ENV", "production"),
            ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
            ("PORT", "1"),
        ]))
        .unwrap();
        let checks = good.gateway_checks();
        assert!(checks.errors.is_empty() && checks.warnings.is_empty());
    }
}
