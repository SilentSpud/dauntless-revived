//! The network layer (rows L19 to L25): pinned TLS, the HTTP client, and later the host API, relay,
//! downloader and verify.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the host API (L21), relay (L22) and downloader (L23) use these")
)]

pub mod http;
pub mod pinned;
