//! What the launcher reads from outside (MIGRATION_PLAN.md L11, L12, L17, X9): invite links, which decide
//! where the account key goes; server status answers; usernames and pasted account keys; and manifests from
//! a content server. The launcher's own files, compiled in here as build.rs does, since the launcher crate
//! needs a webview to build.
#![no_main]
#![allow(dead_code, unused_imports)]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "../../../legacy/UndauntedLauncher/src-tauri/src/core/text.rs"]
mod text;

#[path = "../../../legacy/UndauntedLauncher/src-tauri/src/core/invite.rs"]
mod invite;

#[path = "../../../legacy/UndauntedLauncher/src-tauri/src/core/status.rs"]
mod status;

#[path = "../../../legacy/UndauntedLauncher/src-tauri/src/core/username.rs"]
mod username;

mod manifest {
    use super::*;
    include!("../../../legacy/UndauntedLauncher/src-tauri/src/core/pins.rs");
    include!("../../../legacy/UndauntedLauncher/src-tauri/src/core/manifest_check.rs");
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(parsed) = invite::parse_invite(&text) {
        // An invite that parses names a host the key may go to: never empty, and a public one is pinned
        assert!(!parsed.host.is_empty() && parsed.port != 0);
        assert!(parsed.mode != protocol::invite::ServerMode::Public || parsed.fp.is_some());
    }
    let _ = username::check_username(&text);
    if let Some(key) = username::extract_account_key(&text) {
        assert!(username::is_plausible_account_key(key));
    }
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        let _ = status::parse_server_status(&value);
        if let Ok(accepted) = manifest::validate_manifest(&value, manifest::EXPECTED_BUILD, false) {
            for file in &accepted.files {
                assert!(manifest::is_safe_relative_path(&file.path), "{:?}", file.path);
            }
            let _ = manifest::compare_manifests(&accepted, &value);
        }
    }
});
