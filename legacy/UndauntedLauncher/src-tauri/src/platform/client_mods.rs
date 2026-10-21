//! The pinned DLLs and the client mods next to the game exe (row L30): `src/main/dlls.ts` and
//! `client-mods.ts`. Every file is checked against its pin (the DLLs) or `client-mods/manifest.json`
//! before it is copied and again after. Mod files marked "keep" (mods.txt, UE4SS-settings.ini) are only
//! written when missing, so the player's own choices survive; files the bundle dropped are removed, and
//! mods named in "disable" are switched off in the kept mods.txt (only their own line changes).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::core::pins::{PINNED_DLLS, WIN64_RELATIVE_DIR};

pub const CLIENT_MODS_RESOURCE: &str = "client-mods";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallError {
    pub file: String,
    pub message: String,
}

impl InstallError {
    fn new(file: &str, message: &str) -> Self {
        Self {
            file: file.to_owned(),
            message: message.to_owned(),
        }
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

impl std::error::Error for InstallError {}

pub fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn win64_dir(install_dir: &Path) -> PathBuf {
    WIN64_RELATIVE_DIR
        .split('/')
        .fold(install_dir.to_path_buf(), |path, part| path.join(part))
}

/// Clears the "downloaded from the internet" mark (the Zone.Identifier stream), like Unblock-File.
pub fn unblock(file: &Path) {
    if cfg!(windows) {
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::remove_file(stream);
    }
}

/// Copies `source` to `target` through `<target>.new`, checking the hash before, during and after.
fn copy_checked(
    source: &Path,
    target: &Path,
    sha256: &str,
    name: &str,
    missing: &str,
    wrong: &str,
) -> Result<(), InstallError> {
    let source_hash = hash_file(source).map_err(|_| InstallError::new(name, missing))?;
    if source_hash != sha256 {
        return Err(InstallError::new(name, wrong));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| InstallError::new(name, &error.to_string()))?;
    }
    let mut temporary = target.as_os_str().to_owned();
    temporary.push(".new");
    let temporary = PathBuf::from(temporary);
    std::fs::copy(source, &temporary).map_err(|error| InstallError::new(name, &error.to_string()))?;
    if hash_file(&temporary).ok().as_deref() != Some(sha256) {
        let _ = std::fs::remove_file(&temporary);
        return Err(InstallError::new(name, "changed while copying (antivirus?)"));
    }
    if let Err(error) = std::fs::rename(&temporary, target) {
        // The target is in use (the game is running)
        let _ = std::fs::remove_file(&temporary);
        return Err(InstallError::new(name, &error.to_string()));
    }
    unblock(target);
    Ok(())
}

/// Whether each pinned DLL is in place: (name, matches its pin).
pub fn dll_status(install_dir: &Path) -> Vec<(&'static str, bool)> {
    let dir = win64_dir(install_dir);
    PINNED_DLLS
        .iter()
        .map(|dll| {
            (
                dll.name,
                hash_file(&dir.join(dll.name)).ok().as_deref() == Some(dll.sha256),
            )
        })
        .collect()
}

pub fn install_pinned_dlls(resources_dir: &Path, install_dir: &Path) -> Result<(), InstallError> {
    let dir = win64_dir(install_dir);
    for dll in &PINNED_DLLS {
        let target = dir.join(dll.name);
        if hash_file(&target).ok().as_deref() == Some(dll.sha256) {
            unblock(&target);
            continue;
        }
        copy_checked(
            &resources_dir.join(dll.name),
            &target,
            dll.sha256,
            dll.name,
            "missing from the launcher's resources",
            "the launcher's copy does not match the pinned hash",
        )?;
        if hash_file(&target).ok().as_deref() != Some(dll.sha256) {
            return Err(InstallError::new(dll.name, "changed after copying (antivirus?)"));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientModFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub keep: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientModsManifest {
    pub version: u32,
    pub target: String,
    pub files: Vec<ClientModFile>,
    #[serde(default)]
    pub remove: Vec<serde_json::Value>,
    #[serde(default)]
    pub disable: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientModsResult {
    pub installed: u32,
    pub kept: u32,
    pub up_to_date: u32,
    pub removed: u32,
    pub disabled: u32,
}

/// Only plain relative paths inside the bundle, written with "/": no backslashes, drive letters or
/// colons, no "." or "..", no absolute paths.
pub fn safe_relative(relative: &str) -> Result<PathBuf, InstallError> {
    let parts: Vec<&str> = relative.split('/').collect();
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains(['\\', ':'])
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(InstallError::new(relative, "unsafe path in manifest.json"));
    }
    Ok(parts.iter().collect())
}

pub fn read_client_mods_manifest(resources_dir: &Path) -> Result<Option<ClientModsManifest>, InstallError> {
    let file = resources_dir.join(CLIENT_MODS_RESOURCE).join("manifest.json");
    // A launcher built without client mods
    let Ok(text) = std::fs::read_to_string(file) else {
        return Ok(None);
    };
    let manifest: ClientModsManifest =
        serde_json::from_str(&text).map_err(|_| InstallError::new("manifest.json", "unknown format"))?;
    if manifest.version != 1 {
        return Err(InstallError::new("manifest.json", "unknown format"));
    }
    Ok(Some(manifest))
}

/// The lines of a text, each with the line terminator after it (JavaScript's multiline `^`/`$`).
fn lines_with_ends(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find(['\n', '\r', '\u{2028}', '\u{2029}']) {
            Some(at) => {
                let width = rest[at..].chars().next().map_or(1, char::len_utf8);
                out.push((&rest[..at], &rest[at..at + width]));
                rest = &rest[at + width..];
            }
            None => {
                out.push((rest, ""));
                rest = "";
            }
        }
    }
    out
}

fn is_name(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `[ \t]*Name[ \t]*:[ \t]*<state>[ \t]*`: (name, the text before the state, the state, the text after).
fn mod_line(line: &str) -> Option<(&str, &str, &str, &str)> {
    let blank = |c: char| c == ' ' || c == '\t';
    let trimmed = line.trim_start_matches(blank);
    let name_end = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(trimmed.len());
    let name = &trimmed[..name_end];
    if !is_name(name) {
        return None;
    }
    let after = trimmed[name_end..]
        .trim_start_matches(blank)
        .strip_prefix(':')?
        .trim_start_matches(blank);
    let state_at = line.len() - after.len();
    let state = after.get(..1)?;
    let rest = &after[1..];
    if !rest.chars().all(blank) {
        return None;
    }
    Some((name, &line[..state_at], state, rest))
}

/// "Name : 1" -> "Name : 0" for each name (mod folder names only), keeping every other line, the order
/// and the line endings exactly. (text, lines changed)
pub fn disable_mod_lines(text: &str, names: &[serde_json::Value]) -> (String, u32) {
    let off: HashSet<&str> = names
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|name| is_name(name))
        .collect();
    let mut changed = 0;
    let mut out = String::with_capacity(text.len());
    for (line, end) in lines_with_ends(text) {
        match mod_line(line) {
            Some((name, before, "1", after)) if off.contains(name) => {
                changed += 1;
                out.push_str(before);
                out.push('0');
                out.push_str(after);
            }
            _ => out.push_str(line),
        }
        out.push_str(end);
    }
    (out, changed)
}

/// Adds newly shipped mods without changing a player's existing choices, first, so the built-in
/// Keybinds entry stays at the end.
pub fn add_missing_mod_lines(text: &str, defaults: &str) -> String {
    let blank = |c: char| c == ' ' || c == '\t';
    let mut names: HashSet<String> = lines_with_ends(text)
        .into_iter()
        .filter_map(|(line, _)| {
            let trimmed = line.trim_start_matches(blank);
            let end = trimmed
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(trimmed.len());
            (end > 0 && trimmed[end..].trim_start_matches(blank).starts_with(':'))
                .then(|| trimmed[..end].to_ascii_lowercase())
        })
        .collect();
    let mut missing = Vec::new();
    for (line, _) in lines_with_ends(defaults) {
        if let Some((name, _, state @ ("0" | "1"), _)) = mod_line(line)
            && names.insert(name.to_ascii_lowercase())
        {
            missing.push(format!("{name} : {state}"));
        }
    }
    if missing.is_empty() {
        return text.to_owned();
    }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    format!("{}{eol}{text}", missing.join(eol))
}

fn write_replacing(file: &Path, text: &str) -> std::io::Result<()> {
    let mut temporary = file.as_os_str().to_owned();
    temporary.push(".new");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, file)
}

pub fn install_client_mods(resources_dir: &Path, install_dir: &Path) -> Result<Option<ClientModsResult>, InstallError> {
    let Some(manifest) = read_client_mods_manifest(resources_dir)? else {
        return Ok(None);
    };
    let source_root = resources_dir.join(CLIENT_MODS_RESOURCE);
    let target_root = win64_dir(install_dir);
    let mut result = ClientModsResult::default();

    // Dropped files first: never one the bundle still ships or an unsafe one; folders only while empty,
    // and never the Win64 folder or anything above it
    let shipped: HashSet<&str> = manifest.files.iter().map(|file| file.path.as_str()).collect();
    for removed in manifest.remove.iter().filter_map(serde_json::Value::as_str) {
        if shipped.contains(removed) {
            continue;
        }
        let Ok(relative) = safe_relative(removed) else { continue };
        let target = target_root.join(&relative);
        if std::fs::remove_file(&target).is_err() {
            continue;
        }
        result.removed += 1;
        let mut dir = target.parent().map(Path::to_path_buf);
        while let Some(current) = dir {
            if current == target_root || !current.starts_with(&target_root) || std::fs::remove_dir(&current).is_err() {
                break;
            }
            dir = current.parent().map(Path::to_path_buf);
        }
    }

    for file in &manifest.files {
        let relative = safe_relative(&file.path)?;
        let want = file.sha256.to_ascii_lowercase();
        let target = target_root.join(&relative);
        let source = source_root.join(&relative);
        let present = target.exists();
        if present && file.keep {
            if file.path == "ue4ss/Mods/mods.txt" {
                if hash_file(&source).ok().as_deref() != Some(want.as_str()) {
                    return Err(InstallError::new(
                        &file.path,
                        "the launcher's copy does not match manifest.json",
                    ));
                }
                let before = std::fs::read_to_string(&target)
                    .map_err(|error| InstallError::new(&file.path, &error.to_string()))?;
                let defaults = std::fs::read_to_string(&source)
                    .map_err(|error| InstallError::new(&file.path, &error.to_string()))?;
                let after = add_missing_mod_lines(&before, &defaults);
                if after != before {
                    write_replacing(&target, &after)
                        .map_err(|error| InstallError::new(&file.path, &error.to_string()))?;
                }
            }
            result.kept += 1;
            continue;
        }
        if present && hash_file(&target).ok().as_deref() == Some(want.as_str()) {
            result.up_to_date += 1;
            continue;
        }
        copy_checked(
            &source,
            &target,
            &want,
            &file.path,
            "missing from the launcher's resources",
            "the launcher's copy does not match manifest.json",
        )?;
        result.installed += 1;
    }

    let mods_txt = target_root.join("ue4ss").join("Mods").join("mods.txt");
    if !manifest.disable.is_empty()
        && let Ok(text) = std::fs::read_to_string(&mods_txt)
    {
        let (next, changed) = disable_mod_lines(&text, &manifest.disable);
        if changed > 0 && write_replacing(&mods_txt, &next).is_ok() {
            result.disabled = changed;
        }
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(data: &[u8]) -> String {
        Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn installs_keeps_removes_and_disables() {
        let dir = tempfile::tempdir().unwrap();
        let (resources, install) = (dir.path().join("resources"), dir.path().join("game"));
        let bundle = resources.join(CLIENT_MODS_RESOURCE);
        std::fs::create_dir_all(bundle.join("ue4ss").join("Mods")).unwrap();
        std::fs::write(bundle.join("dwmapi.dll"), b"loader").unwrap();
        std::fs::write(
            bundle.join("ue4ss").join("Mods").join("mods.txt"),
            "NewMod : 1\r\nHealthBars : 1\r\nKeybinds : 1\r\n",
        )
        .unwrap();
        let manifest = serde_json::json!({
            "version": 1, "target": "Archon/Binaries/Win64",
            "files": [
                { "path": "dwmapi.dll", "sha256": sha(b"loader"), "size": 6, "keep": false },
                { "path": "ue4ss/Mods/mods.txt", "sha256": sha(b"NewMod : 1\r\nHealthBars : 1\r\nKeybinds : 1\r\n"), "size": 42, "keep": true },
            ],
            "remove": ["old/gone.lua", "../escape.dll", "dwmapi.dll"],
            "disable": ["HealthBars"],
        });
        std::fs::write(bundle.join("manifest.json"), manifest.to_string()).unwrap();
        let win64 = win64_dir(&install);
        std::fs::create_dir_all(win64.join("old")).unwrap();
        std::fs::write(win64.join("old").join("gone.lua"), b"x").unwrap();
        std::fs::create_dir_all(win64.join("ue4ss").join("Mods")).unwrap();
        std::fs::write(
            win64.join("ue4ss").join("Mods").join("mods.txt"),
            "HealthBars : 1\r\nKeybinds : 1\r\n",
        )
        .unwrap();

        let result = install_client_mods(&resources, &install).unwrap().unwrap();
        assert_eq!(
            result,
            ClientModsResult {
                installed: 1,
                kept: 1,
                up_to_date: 0,
                removed: 1,
                disabled: 1
            }
        );
        assert!(!win64.join("old").exists(), "an emptied folder goes too");
        assert_eq!(std::fs::read(win64.join("dwmapi.dll")).unwrap(), b"loader");
        assert_eq!(
            std::fs::read_to_string(win64.join("ue4ss").join("Mods").join("mods.txt")).unwrap(),
            "NewMod : 1\r\nHealthBars : 0\r\nKeybinds : 1\r\n"
        );
        let again = install_client_mods(&resources, &install).unwrap().unwrap();
        assert_eq!((again.installed, again.up_to_date), (0, 1));

        std::fs::write(bundle.join("dwmapi.dll"), b"tampered").unwrap();
        std::fs::remove_file(win64.join("dwmapi.dll")).unwrap();
        assert_eq!(
            install_client_mods(&resources, &install).unwrap_err().message,
            "the launcher's copy does not match manifest.json"
        );
    }

    #[test]
    fn pinned_dlls_must_match_their_pins() {
        let dir = tempfile::tempdir().unwrap();
        let resources = dir.path().join("resources");
        std::fs::create_dir_all(&resources).unwrap();
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets");
        for dll in &PINNED_DLLS {
            std::fs::copy(assets.join(dll.name), resources.join(dll.name)).unwrap();
        }
        let install = dir.path().join("game");
        assert!(dll_status(&install).iter().all(|(_, ok)| !ok));
        install_pinned_dlls(&resources, &install).unwrap();
        assert!(dll_status(&install).iter().all(|(_, ok)| *ok));
        std::fs::write(resources.join("dxgi.dll"), b"not it").unwrap();
        std::fs::remove_file(win64_dir(&install).join("dxgi.dll")).unwrap();
        assert_eq!(
            install_pinned_dlls(&resources, &install).unwrap_err().message,
            "the launcher's copy does not match the pinned hash"
        );
    }
}
