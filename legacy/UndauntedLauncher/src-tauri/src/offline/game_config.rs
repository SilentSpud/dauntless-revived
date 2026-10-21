//! The game's endpoints pointed at this PC for offline play (`offline-game-config.ts`). Current DLLs hook the
//! Phoenix endpoints themselves; Game.ini stays the documented fallback for older DLLs. The player's own
//! settings in both files survive.

use std::path::Path;

use crate::core::pins::XMPP_PORT;
use crate::platform::game_config::{decode_ini, encode_ini, split_lines};

include!(concat!(env!("OUT_DIR"), "/game_endpoints.rs"));

/// `mergeIniKeys`: `entries` replace those keys in `section` (case-insensitive), first in the section, the
/// section's other lines after them; the section is added at the end when missing. Repeated headers of the
/// section become one.
pub fn merge_ini_keys(lines: &[String], section: &str, entries: &[(String, String)]) -> Vec<String> {
    let managed: Vec<String> = entries.iter().map(|(key, _)| key.to_lowercase()).collect();
    let replacement: Vec<String> = entries.iter().map(|(key, value)| format!("{key}={value}")).collect();
    let mut output: Vec<String> = Vec::new();
    let mut keep: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut insertion: Option<usize> = None;
    for line in lines {
        if let Some(name) = section_header(line) {
            in_section = name.eq_ignore_ascii_case(section);
            if !in_section {
                output.push(line.clone());
            } else if insertion.is_none() {
                output.push(line.clone());
                insertion = Some(output.len());
            }
        } else if !in_section {
            output.push(line.clone());
        } else {
            let key = line
                .split_once('=')
                .map(|(key, _)| key.trim().to_lowercase())
                .filter(|key| !key.is_empty());
            if !key.is_some_and(|key| managed.contains(&key)) {
                keep.push(line.clone());
            }
        }
    }
    match insertion {
        None => {
            output.push(format!("[{section}]"));
            output.extend(replacement);
            output.extend(keep);
        }
        Some(at) => {
            output.splice(at..at, replacement.into_iter().chain(keep));
        }
    }
    output
}

/// `/^\s*\[([^\]]+)\]\s*(?:[;#].*)?$/`: a section header and its name.
fn section_header(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (name, tail) = rest.split_once(']')?;
    if name.is_empty() {
        return None;
    }
    let tail = tail.trim_start();
    (tail.is_empty() || tail.starts_with([';', '#'])).then_some(name)
}

/// The DLL's endpoints as Game.ini values on the local metagame (`offlineEndpointEntries`).
pub fn offline_endpoint_entries(endpoints: &[(&str, &str)]) -> Result<Vec<(String, String)>, String> {
    let mut entries = Vec::new();
    for (key, suffix) in endpoints {
        let key_ok = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        let suffix_ok =
            (suffix.is_empty() || suffix.starts_with('/')) && !suffix.contains(['"', '\r', '\n', '\0', '\\']);
        if !key_ok || !suffix_ok {
            return Err("Invalid offline endpoint table".into());
        }
        entries.push(((*key).to_owned(), format!("\"http://127.0.0.1:61000{suffix}\"")));
    }
    let has = |name: &str| entries.iter().any(|(key, _)| key == name);
    if !has("AuthEndpoint") || !has("MatchmakingEndpoint") {
        return Err("Incomplete offline endpoint table".into());
    }
    Ok(entries)
}

fn update_ini(file: &Path, section: &str, entries: &[(String, String)]) -> std::io::Result<()> {
    let lines = match std::fs::read(file) {
        Ok(data) => split_lines(&decode_ini(&data)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut temporary = file.as_os_str().to_owned();
    temporary.push(".dr-tmp");
    std::fs::write(&temporary, encode_ini(&merge_ini_keys(&lines, section, entries)))?;
    std::fs::rename(&temporary, file)
}

/// Points Game.ini's Phoenix endpoints and Engine.ini's chat at the local services.
pub fn prepare(config_dir: &Path) -> std::io::Result<()> {
    let endpoints = offline_endpoint_entries(GAME_ENDPOINTS).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(config_dir)?;
    update_ini(&config_dir.join("Game.ini"), "OnlineSubsystemPhoenix", &endpoints)?;
    let chat = [
        ("ServerAddr".to_owned(), "\"ws://127.0.0.1\"".to_owned()),
        ("ServerPort".to_owned(), XMPP_PORT.to_string()),
        ("bUseSSL".to_owned(), "false".to_owned()),
    ];
    update_ini(&config_dir.join("Engine.ini"), "OnlineSubsystemMcp.XMPP", &chat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn merging_keeps_the_players_lines_and_replaces_only_ours() {
        let before = lines("[A]\nx=1\n[Phoenix]\nauthendpoint=old\nOther=keep\n[B]\ny=2\n[phoenix] ; again\nLate=keep");
        let merged = merge_ini_keys(&before, "Phoenix", &pairs(&[("AuthEndpoint", "new")]));
        assert_eq!(
            merged,
            lines("[A]\nx=1\n[Phoenix]\nAuthEndpoint=new\nOther=keep\nLate=keep\n[B]\ny=2")
        );
        let added = merge_ini_keys(&lines("[A]\nx=1"), "Phoenix", &pairs(&[("K", "v")]));
        assert_eq!(added, lines("[A]\nx=1\n[Phoenix]\nK=v"));
    }

    #[test]
    fn the_dll_table_without_its_vendor_credential() {
        assert_eq!(GAME_ENDPOINTS.len(), 167);
        let events = GAME_ENDPOINTS
            .iter()
            .find(|(name, _)| *name == "PhoenixEventsMessageEndpoint")
            .unwrap();
        assert_eq!(events.1, "/offline-events");
        assert!(GAME_ENDPOINTS.iter().all(|(_, path)| !path.contains("/services/")));
        let entries = offline_endpoint_entries(GAME_ENDPOINTS).unwrap();
        assert!(
            entries
                .iter()
                .all(|(_, value)| value.starts_with("\"http://127.0.0.1:61000"))
        );
        assert!(offline_endpoint_entries(&[("AuthEndpoint", "x")]).is_err());
        assert!(
            offline_endpoint_entries(&[("AuthEndpoint", "/a")]).is_err(),
            "no matchmaking endpoint"
        );
    }

    #[test]
    fn prepare_writes_both_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Game.ini"), "[Mine]\r\nkeep=1\r\n").unwrap();
        prepare(dir.path()).unwrap();
        let game = String::from_utf8_lossy(&std::fs::read(dir.path().join("Game.ini")).unwrap()).into_owned();
        assert!(game.contains("[Mine]\r\nkeep=1"), "{game}");
        assert!(game.contains("AuthEndpoint=\"http://127.0.0.1:61000"), "{game}");
        let engine = String::from_utf8_lossy(&std::fs::read(dir.path().join("Engine.ini")).unwrap()).into_owned();
        assert!(
            engine.contains("ServerAddr=\"ws://127.0.0.1\"\r\nServerPort=61099\r\nbUseSSL=false"),
            "{engine}"
        );
    }
}
