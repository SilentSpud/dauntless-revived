//! Offline play (rows L42 and L43): the metagame and the DeployServer on this PC, run by the launcher's own
//! binary as `--service metagame|deploy` (D12), and portable `.drsave` saves. Built only with the Cargo
//! feature `offline`, which 0.2.0 leaves off (D1); X2 turns it on.
//!
//! - `layout`: where the offline state lives (`userData/offline`, as the Electron launcher kept it)
//! - `state`: the profile's secrets and the services' settings files (`offline-state.ts`)
//! - `game_config`: the game's endpoints pointed at this PC (`offline-game-config.ts`)
//! - `save_format` and `save`: `.drsave` files and the restore journal (`offline-save*.ts`)
//! - `stack`: starting and stopping the two services (`local-stack*.ts`)
//! - `service`: the `--service` entry point

pub mod game_config;
pub mod layout;
pub mod save;
pub mod save_format;
#[cfg(test)]
mod save_tests;
pub mod service;
pub mod stack;
pub mod state;

/// Offline play needs this much available memory before it starts (`offline-memory.ts`): Deploy's
/// free-memory floor plus one game server's start-up estimate. Deploy checks again before each server.
pub const MIN_FREE_MIB: u64 = 3072;
pub const SERVER_STARTUP_MIB: u64 = 1536;
pub const REQUIRED_AVAILABLE_MIB: u64 = MIN_FREE_MIB + SERVER_STARTUP_MIB;

pub fn has_offline_memory(available_bytes: f64) -> bool {
    available_bytes.is_finite() && available_bytes >= (REQUIRED_AVAILABLE_MIB * 1024 * 1024) as f64
}

/// Available physical memory in bytes (NaN when unknown).
pub fn free_memory() -> f64 {
    deploy::system::free_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_admission_is_at_the_boundary() {
        let boundary = (REQUIRED_AVAILABLE_MIB * 1024 * 1024) as f64;
        assert!(has_offline_memory(boundary));
        assert!(!has_offline_memory(boundary - 1.0));
        assert!(!has_offline_memory(f64::NAN));
        assert!(!has_offline_memory(f64::INFINITY));
    }
}
