//! The network layer (rows L19 to L25): pinned TLS, the HTTP client, the host API, and later the relay,
//! downloader and verify.
#![allow(dead_code, reason = "the controller (L38) calls the network layer")]

pub mod hostapi;
pub mod http;
pub mod pinned;
