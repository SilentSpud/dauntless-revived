//! The network layer (rows L19 to L25): pinned TLS, the HTTP client, the host API, the relay, the
//! downloader, verify and the hunt-region probe.
#![allow(dead_code, reason = "the controller (L38) calls the network layer")]

pub mod autoregion;
pub mod connector;
pub mod downloader;
pub mod hostapi;
pub mod http;
pub mod pinned;
pub mod relay;
pub mod verify;
