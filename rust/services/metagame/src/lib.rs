//! The Rust metagame (MIGRATION_PLAN.md Part B). So far: its database schema and the store setup
//! that adopts the Node metagame's `undaunted.db` (M13, M14, M18).

pub mod crypto;
pub mod db;
#[allow(non_snake_case)]
pub mod schema;
