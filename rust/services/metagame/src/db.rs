//! The metagame's database: Diesel migrations, starting with `0001_baseline`, which creates the
//! schema exactly as the last Drizzle migration of the Node metagame leaves it (D11).

use diesel_migrations::{EmbeddedMigrations, embed_migrations};
use store::{DrizzleBaseline, Schema};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// The Node metagame's last Drizzle migration, which `0001_baseline` was cut from. The Drizzle
/// files stay in `UndauntedMetagame/src/drizzle` until the Node metagame is deleted (M43); after the
/// Node catch-up (M44) the baseline is cut again from the final one.
pub const DRIZZLE_TAG: &str = "0030_device_signals";

pub const SCHEMA: Schema = Schema {
    migrations: || MIGRATIONS,
    drizzle: Some(DrizzleBaseline {
        created_at: 1_792_200_000_002,
        sql: include_str!("../../../../UndauntedMetagame/src/drizzle/0030_device_signals.sql"),
        snapshot: include_str!("../migrations/0001_baseline/up.sql"),
    }),
};
