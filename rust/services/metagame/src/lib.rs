//! The Rust metagame (MIGRATION_PLAN.md Part B). So far: its database schema and the store setup
//! that adopts the Node metagame's `undaunted.db` (M13, M14, M18).

pub mod app;
pub mod auth;
pub mod crypto;
pub mod db;
pub mod dbtools;
pub mod health;
pub mod limits;
pub mod origin;
pub mod progression_config;
#[allow(non_snake_case)]
pub mod schema;
pub mod storeimages;
pub mod vendor;
