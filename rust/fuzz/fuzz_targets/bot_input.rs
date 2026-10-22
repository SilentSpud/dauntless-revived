//! The Discord key bot's input (MIGRATION_PLAN.md S5, X9): what a member pastes into `/key link`, and the
//! interaction payloads Discord delivers. A refusal or a key, never a panic, and a key it accepts is in the
//! launcher's key alphabet (the metagame decides whether it is an account's).
#![no_main]

use discord_bot::link_input::{LinkInput, link_input};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let LinkInput::Key(key) = link_input(&text) {
        assert!(
            (8..=128).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "{key:?}"
        );
    }
    if let Ok(command) = serde_json::from_str::<serde_json::Value>(&text) {
        let _ = discord_bot::discord::command_user(&command);
    }
});
