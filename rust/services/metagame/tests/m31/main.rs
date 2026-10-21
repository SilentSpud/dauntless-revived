//! The Node metagame's `node:test` suites ported to Rust (MIGRATION_PLAN.md M31), one module per TS
//! file; `tests/ported.json` maps each TS test to its port.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

#[path = "../common/mod.rs"]
mod common;

mod accountrecovery;
mod capacityturns;
mod devicesignals;
mod discordlinks;
mod discordstats;
mod entitlements;
mod gameserveronly;
mod huntregion;
mod keyrotation;
mod playertracking;
mod progressionrank;
mod ramspricing;
mod regionprobes;
mod shoplayout;
