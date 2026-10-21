//! The Node metagame's `node:test` suites ported to Rust (MIGRATION_PLAN.md M31), one module per TS
//! file; `tests/ported.json` maps each TS test to its port.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

#[path = "../common/mod.rs"]
mod common;

mod accountrecovery;
mod backup;
mod branding;
mod capacityturns;
mod character;
mod cooldownsbounties;
mod devicesignals;
mod discordlinks;
mod discordstats;
mod entitlements;
mod gameserveronly;
mod huntregion;
mod inventory;
mod inventoryauthority;
mod inventoryported;
mod inventorytx;
mod keyrotation;
mod loadout;
mod loadoutslots;
mod novaunlocker;
mod playertracking;
mod progressiondefault;
mod progressionextras;
mod progressionmode;
mod progressionrank;
mod ramspricing;
mod realprogression;
mod regionprobes;
mod shoplayout;
mod shutdown;
mod storage;
mod store_maintenance;
mod unlocker_detection;
