//! Tauri's build step, with the app's own commands under the ACL (row L8): each command in
//! `src/bridge/spec.rs` gets `allow-<command>` / `deny-<command>` permissions, and the page may call
//! only what `capabilities/main.json` grants.

use std::fs;

/// The command names in the order of `COMMANDS` in spec.rs (read as text: build.rs cannot use the crate).
fn command_names() -> Vec<&'static str> {
    let spec = fs::read_to_string("src/bridge/spec.rs").unwrap_or_default();
    let list = spec.split_once("pub const COMMANDS").map_or("", |(_, rest)| rest);
    let list = list.split_once("];").map_or("", |(list, _)| list);
    list.split("command(\"")
        .skip(1)
        .filter_map(|item| {
            item.split_once('"')
                .map(|(name, _)| &*Box::leak(name.to_owned().into_boxed_str()))
        })
        .collect()
}

fn main() {
    println!("cargo:rerun-if-changed=src/bridge/spec.rs");
    println!("cargo:rerun-if-changed=permissions");
    println!("cargo:rerun-if-changed=../isolation");
    let names = command_names();
    assert!(!names.is_empty(), "no commands found in src/bridge/spec.rs");
    let commands: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    let attributes = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(commands));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("tauri build step failed: {error:#}");
    }
}
