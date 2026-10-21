//! The compiled-in game manifest (row L17): `legacy/UndauntedContent/data/dauntless-1.4.4.json`, the list
//! of files, sizes and hashes that is part of the launcher's public source and cannot be changed by a
//! server. build.rs refuses to build with a malformed one; it is read once, when first needed.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the downloader (L23) and verify (L24) use the manifest")
)]

use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::pins::*;

include!("manifest_check.rs");

include!(concat!(env!("OUT_DIR"), "/manifest.rs"));

/// The compiled-in manifest, checked as build.rs checked it.
pub static GAME_MANIFEST: LazyLock<GameManifest> = LazyLock::new(|| {
    let raw: Value = serde_json::from_slice(GAME_MANIFEST_JSON).unwrap_or(Value::Null);
    // build.rs ran this same check on these same bytes, so it cannot fail here
    validate_manifest(&raw, EXPECTED_BUILD, true).unwrap_or(GameManifest {
        build: String::new(),
        total_bytes: 0,
        files: Vec::new(),
    })
});

/// Joins a checked manifest path onto the install folder and makes sure the result stays inside it.
pub fn resolve_inside(root: &Path, relative: &str) -> Result<PathBuf, &'static str> {
    if !is_safe_relative_path(relative) {
        return Err("unsafe path");
    }
    let mut full = root.to_path_buf();
    for segment in relative.split('/') {
        full.push(segment);
    }
    // Every segment is a plain name (no `..`, no root, no prefix), so nothing can climb out
    let inside = full
        .strip_prefix(root)
        .is_ok_and(|rest| rest.components().all(|part| matches!(part, Component::Normal(_))));
    if !inside || full == root {
        return Err("path escapes install folder");
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Cases {
        base: Value,
        test_build: String,
        fingerprint: String,
        validate: Vec<Validate>,
        compare: Vec<Compare>,
        paths: Vec<PathCase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Validate {
        #[serde(default)]
        input: Value,
        build: String,
        require_game_exe: bool,
        result: Value,
    }

    #[derive(Deserialize)]
    struct Compare {
        input: Value,
        result: Value,
    }

    #[derive(Deserialize)]
    struct PathCase {
        path: String,
        safe: bool,
    }

    fn as_json(manifest: &GameManifest) -> Value {
        serde_json::json!({
            "build": manifest.build,
            "totalBytes": manifest.total_bytes,
            "files": manifest.files.iter().map(|f| serde_json::json!({ "path": f.path, "size": f.size, "sha256": f.sha256 })).collect::<Vec<_>>(),
        })
    }

    fn cases() -> Cases {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test/cases/manifest.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn the_shared_table() {
        let cases = cases();
        for case in &cases.validate {
            let result = match validate_manifest(&case.input, &case.build, case.require_game_exe) {
                Ok(manifest) => serde_json::json!({ "ok": true, "manifest": as_json(&manifest) }),
                Err(reason) => serde_json::json!({ "ok": false, "reason": reason }),
            };
            assert_eq!(result, case.result, "{}", case.input);
        }
        let reference = validate_manifest(&cases.base, &cases.test_build, false).unwrap();
        assert_eq!(manifest_fingerprint(&reference), cases.fingerprint);
        for case in &cases.compare {
            let result = match compare_manifests(&reference, &case.input) {
                Ok(()) => serde_json::json!({ "equal": true }),
                Err(difference) => serde_json::json!({ "equal": false, "difference": difference }),
            };
            assert_eq!(result, case.result, "{}", case.input);
        }
        for case in &cases.paths {
            assert_eq!(is_safe_relative_path(&case.path), case.safe, "{:?}", case.path);
        }
    }

    #[test]
    fn the_compiled_in_manifest() {
        assert_eq!(GAME_MANIFEST.build, EXPECTED_BUILD);
        assert!(GAME_MANIFEST.files.len() > 400);
        assert_eq!(manifest_fingerprint(&GAME_MANIFEST), GAME_MANIFEST_FINGERPRINT);
        assert!(
            GAME_MANIFEST
                .files
                .iter()
                .any(|file| file.path == EXE_RELATIVE_PATH && file.sha256 == PINNED_EXE_SHA256)
        );
    }

    #[test]
    fn paths_stay_inside_the_install_folder() {
        let root = Path::new("C:/games/dr");
        assert_eq!(
            resolve_inside(root, "Archon/Binaries/Win64/x.exe").unwrap(),
            root.join("Archon").join("Binaries").join("Win64").join("x.exe")
        );
        assert!(resolve_inside(root, "../x").is_err());
        assert!(resolve_inside(root, "C:/x").is_err());
    }
}
