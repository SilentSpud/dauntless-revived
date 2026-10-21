// The game manifest (shared contract 4, row L17): the exact list of Dauntless 1.4.4 files, their sizes
// and SHA-256 hashes, checked as `src/main/manifest.ts` checks it, with the same reasons. build.rs
// includes this file to refuse a malformed compiled-in manifest, so it names what it needs from its
// includer: `serde_json::Value`, `sha2::{Digest, Sha256}` and the pins (`pins.rs`).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameManifest {
    pub build: String,
    pub total_bytes: u64,
    /// Sorted by path
    pub files: Vec<ManifestFile>,
}

/// `con`, `prn`, `aux`, `nul`, `com0`-`com9`, `lpt0`-`lpt9`, `conin$`, `conout$`, with or without an
/// extension, in any case.
fn is_reserved_name(segment: &str) -> bool {
    let stem = segment.split_once('.').map_or(segment, |(stem, _)| stem).to_ascii_lowercase();
    let numbered = |prefix: &str| stem.len() == 4 && stem.starts_with(prefix) && stem.as_bytes()[3].is_ascii_digit();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul" | "conin$" | "conout$") || numbered("com") || numbered("lpt")
}

/// A manifest path is relative, uses forward slashes, and cannot climb out of the install folder or
/// name anything Windows treats specially (drive letters, streams, device names).
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 240 || path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    path.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.+() -".contains(&byte))
            && !segment.starts_with(' ')
            && !segment.ends_with(' ')
            && !segment.ends_with('.')
            && !is_reserved_name(segment)
    })
}

/// `Number.isSafeInteger(v) && v >= 0`, for a JSON value.
fn safe_size(value: Option<&Value>) -> Option<u64> {
    let n = value?.as_f64()?;
    (n.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&n)).then_some(n as u64)
}

/// `String(v).slice(0, 120)`, for the reason of an unsafe path.
fn shown(value: Option<&Value>) -> String {
    let text = match value {
        None => "undefined".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(_)) => "[object Object]".to_owned(),
        Some(other) => other.to_string(),
    };
    let mut out = String::new();
    let mut units = 0;
    for c in text.chars() {
        units += c.len_utf16();
        if units > 120 {
            break;
        }
        out.push(c);
    }
    out
}

pub fn validate_manifest(raw: &Value, expected_build: &str, require_game_exe: bool) -> Result<GameManifest, String> {
    let raw = raw.as_object().ok_or("not an object")?;
    if raw.get("build").and_then(Value::as_str) != Some(expected_build) {
        return Err("wrong build".into());
    }
    let list = raw.get("files").and_then(Value::as_array).filter(|list| !list.is_empty()).ok_or("no files")?;
    let total = safe_size(raw.get("totalBytes")).ok_or("bad totalBytes")?;

    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::with_capacity(list.len());
    let mut sum: u64 = 0;
    for entry in list {
        let entry = entry.as_object().ok_or("bad entry")?;
        let path = match entry.get("path") {
            Some(Value::String(path)) if is_safe_relative_path(path) => path,
            other => return Err(format!("unsafe path: {}", shown(other))),
        };
        let size = safe_size(entry.get("size")).ok_or_else(|| format!("bad size: {path}"))?;
        let sha256 = match entry.get("sha256") {
            Some(Value::String(hash)) if hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) => hash,
            _ => return Err(format!("bad sha256: {path}")),
        };
        // Windows file names are case-insensitive
        let key = path.to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            return Err(format!("duplicate: {path}"));
        }
        if PINNED_DLLS.iter().any(|dll| format!("{WIN64_RELATIVE_DIR}/{}", dll.name).to_ascii_lowercase() == key) {
            return Err(format!("pinned DLL listed: {path}"));
        }
        files.push(ManifestFile { path: path.clone(), size, sha256: sha256.clone() });
        sum = sum.saturating_add(size);
    }
    if sum != total {
        return Err("totalBytes does not match the files".into());
    }

    if require_game_exe {
        let exe = files.iter().find(|file| file.path == EXE_RELATIVE_PATH);
        if !exe.is_some_and(|exe| exe.sha256 == PINNED_EXE_SHA256 && exe.size == PINNED_EXE_SIZE) {
            return Err("game executable is not the pinned 1.4.4 build".into());
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(GameManifest { build: expected_build.to_owned(), total_bytes: sum, files })
}

/// The server's manifest must be exactly the compiled-in one: same build, same files, same sizes, same
/// hashes. Order does not matter. `Err` names the first difference.
pub fn compare_manifests(expected: &GameManifest, actual: &Value) -> Result<(), String> {
    let other = validate_manifest(actual, &expected.build, false).map_err(|reason| format!("server manifest invalid ({reason})"))?;
    if other.total_bytes != expected.total_bytes {
        return Err("total size differs".into());
    }
    if other.files.len() != expected.files.len() {
        return Err("file count differs".into());
    }
    let by_path: std::collections::HashMap<&str, &ManifestFile> = other.files.iter().map(|file| (file.path.as_str(), file)).collect();
    for file in &expected.files {
        let found = by_path.get(file.path.as_str()).ok_or_else(|| format!("missing on server: {}", file.path))?;
        if found.size != file.size {
            return Err(format!("size differs: {}", file.path));
        }
        if found.sha256 != file.sha256 {
            return Err(format!("hash differs: {}", file.path));
        }
    }
    Ok(())
}

/// SHA-256 of the build and the sorted `path<TAB>size<TAB>sha256` lines.
pub fn manifest_fingerprint(manifest: &GameManifest) -> String {
    let mut files: Vec<&ManifestFile> = manifest.files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let canonical: Vec<String> = files.iter().map(|file| format!("{}\t{}\t{}", file.path, file.size, file.sha256)).collect();
    let digest = Sha256::digest(format!("{}\n{}", manifest.build, canonical.join("\n")).as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
