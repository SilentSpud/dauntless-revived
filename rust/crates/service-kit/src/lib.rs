//! What every Rust service shares (MIGRATION_PLAN.md B1): Express-compatible routing ([`routing`]),
//! JSON replies like Express's ([`json`]), pino-format logs ([`log`]), configuration from an explicit
//! env file ([`env`]) and the listen / stop cycle of X7 ([`serve`]).

pub mod env;
pub mod json;
pub mod log;
pub mod routing;
pub mod serve;
