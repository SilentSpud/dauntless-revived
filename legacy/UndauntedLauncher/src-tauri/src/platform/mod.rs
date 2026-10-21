//! The platform layer (rows L26 to L37): folders, settings, the key store, the log, the game folder,
//! launching, links, dialogs and the art protocol.
#![allow(dead_code, reason = "the controller (L38) uses the platform layer")]

pub mod client_mods;
pub mod game_config;
pub mod game_folder;
pub mod keystore;
pub mod log;
pub mod paths;
pub mod settings;

#[cfg(test)]
mod game_config_tests;
