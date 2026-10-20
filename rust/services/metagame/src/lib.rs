//! The Rust metagame (MIGRATION_PLAN.md Part B). So far: its database schema and the store setup
//! that adopts the Node metagame's `undaunted.db` (M13, M14, M18).

pub mod accounts;
pub mod app;
pub mod auth;
pub mod bounties;
pub mod characterprogress;
pub mod characters;
pub mod config;
pub mod cooldowns;
pub mod creator;
pub mod crypto;
pub mod db;
pub mod dbtools;
pub mod entitlements;
pub mod friends;
pub mod health;
pub mod inventory;
pub mod js;
pub mod limits;
pub mod loadouts;
pub mod matchmaking;
pub mod middleman;
pub mod origin;
pub mod party;
pub mod progression;
pub mod progression_config;
pub mod realprogression;
pub mod redact;
pub mod routes;
#[allow(non_snake_case)]
pub mod schema;
pub mod server;
pub mod store;
pub mod store_revocation;
pub mod storeimages;
pub mod time;
pub mod trials;
pub mod unlocker;
pub mod vendor;
