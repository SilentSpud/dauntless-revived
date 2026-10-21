//! The environment as the Node deploy server reads it: most settings are read again on every use
//! (`process.env.X` inside the function), so a host can change a drain file or a limit without a
//! restart and tests can flip them. A few are read once at start, as Node read them when the module
//! loaded; those live in [`Fixed`].

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use service_kit::env::Env;

#[derive(Clone, Default)]
pub struct Settings {
    vars: Arc<RwLock<HashMap<String, String>>>,
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let vars = env.vars().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        Self {
            vars: Arc::new(RwLock::new(vars)),
        }
    }

    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self::from_env(&Env::from_pairs(pairs))
    }

    /// `process.env[name]`: the value as set, also when empty.
    pub fn get(&self, name: &str) -> Option<String> {
        self.vars
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    /// `process.env[name] || undefined`: unset and empty alike.
    pub fn non_empty(&self, name: &str) -> Option<String> {
        self.get(name).filter(|v| !v.is_empty())
    }

    pub fn is(&self, name: &str, value: &str) -> bool {
        self.get(name).as_deref() == Some(value)
    }

    /// Sets (or with `None` removes) a variable, as `process.env[name] = ...` did.
    pub fn set(&self, name: &str, value: Option<&str>) {
        let mut vars = self.vars.write().unwrap_or_else(PoisonError::into_inner);
        match value {
            Some(value) => vars.insert(name.to_owned(), value.to_owned()),
            None => vars.remove(name),
        };
    }

    /// Every variable, as set.
    pub fn snapshot(&self) -> Vec<(String, String)> {
        let vars = self.vars.read().unwrap_or_else(PoisonError::into_inner);
        vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// `Number(value)` for a set value, `None` when unset.
    pub fn number(&self, name: &str) -> Option<f64> {
        self.get(name)
            .map(|v| service_kit::js::number(Some(&serde_json::Value::String(v))))
    }

    /// The deploy server's `setting(name, fallback)`: a whole number of at least 1, digits only.
    pub fn whole(&self, name: &str, fallback: u64) -> Result<u64, String> {
        let Some(value) = self.get(name) else {
            return Ok(fallback);
        };
        let digits = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
        match value.parse::<u64>() {
            Ok(number) if digits && (1..=9_007_199_254_740_991).contains(&number) => Ok(number),
            _ => Err(format!("Invalid {name}")),
        }
    }
}

/// What Node read once, when `gameservers.ts` loaded.
#[derive(Debug, Clone)]
pub struct Fixed {
    pub port_range_begin: u16,
    pub port_range_end: u16,
    pub binary: String,
    pub metagame_api_key: String,
    pub my_ip: String,
    /// `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP`, in milliseconds
    pub launch_spacing_ms: u64,
}

impl Fixed {
    pub fn from_settings(settings: &Settings) -> Result<Self, String> {
        let port = |name: &str| -> Result<u16, String> {
            settings
                .number(name)
                .filter(|n| n.fract() == 0.0 && (1.0..=65535.0).contains(n))
                .map(|n| n as u16)
                .ok_or_else(|| format!("{name} must be a port number"))
        };
        let spacing = settings
            .number("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP")
            .unwrap_or(f64::NAN);
        Ok(Self {
            port_range_begin: port("PORT_RANGE_BEGIN")?,
            port_range_end: port("PORT_RANGE_END")?,
            binary: settings.get("GAMESERVER_BINARY_PATH").unwrap_or_default(),
            metagame_api_key: settings.get("METAGAME_API_KEY").unwrap_or_default(),
            my_ip: settings.get("MY_IP").unwrap_or_default(),
            // Node: a NaN wait never delays (Date.now() + NaN compares false)
            launch_spacing_ms: if spacing.is_finite() && spacing > 0.0 {
                (spacing * 1000.0) as u64
            } else {
                0
            },
        })
    }
}
