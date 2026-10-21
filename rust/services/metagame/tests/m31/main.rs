//! The Node metagame's `node:test` suites ported to Rust (MIGRATION_PLAN.md M31), one module per TS
//! file; `tests/ported.json` maps each TS test to its port.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

#[path = "../common/mod.rs"]
mod common;

mod accountrecovery;
mod backup;
mod bountiesported;
mod branding;
mod capacityturns;
mod character;
mod cooldownsbounties;
mod creator;
mod deploykit;
mod devicesignals;
mod discordlinks;
mod discordstats;
mod entitlements;
mod escalation;
mod freestore;
mod friendsapi;
mod gameserveronly;
mod guildhttp;
mod huntpassported;
mod huntregion;
mod inventory;
mod inventoryauthority;
mod inventoryported;
mod inventorytx;
mod keyrotation;
mod ladyluck;
mod loadout;
mod loadoutslots;
mod masteryported;
mod middleman;
mod middleman_rotation;
mod middleman_store;
mod novaunlocker;
mod party;
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
mod slayerlinks;
mod socialclient;
mod socialflow;
mod socialported;
mod storage;
mod store_maintenance;
mod storekit;
mod trials;
mod unlocker_detection;
