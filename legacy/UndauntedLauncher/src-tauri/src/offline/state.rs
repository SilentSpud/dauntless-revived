//! The offline profile's secrets and the services' settings (`offline-state.ts`). The secrets are made once
//! and kept: a new signing pair would sign the player out, a new game-server key would orphan the one the
//! database knows. A secrets file that cannot be read is never replaced.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::layout::OfflineLayout;
use super::{MIN_FREE_MIB, SERVER_STARTUP_MIB};
use crate::core::pins::XMPP_PORT;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineSecrets {
    /// Base64 of the PKCS#8 PEM, as the metagame reads `AUTH_SIGNING_PRIVKEY_B64`
    pub signing_private_key: String,
    pub signing_public_key: String,
    /// 48 lower-case hex
    pub gameserver_key: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Offline secrets are invalid; refusing to replace the saved identity")]
    Invalid,
    #[error("Could not read offline secrets; refusing to replace the saved identity")]
    Unreadable,
    #[error("offline state: {0}")]
    Io(#[from] std::io::Error),
    #[error("offline state: {0}")]
    Other(String),
}

pub fn generate_secrets() -> Result<OfflineSecrets, StateError> {
    let (signing_private_key, signing_public_key) = deploy::kitconfig::signing_pair().map_err(StateError::Other)?;
    let mut key = [0u8; 24];
    rand::fill(&mut key);
    Ok(OfflineSecrets {
        signing_private_key,
        signing_public_key,
        gameserver_key: key.iter().map(|b| format!("{b:02x}")).collect(),
    })
}

/// An RSA pair of at least 2048 bits whose halves belong together, and a well-formed game-server key.
fn validate(secrets: &OfflineSecrets) -> Result<(), StateError> {
    let pem = |b64: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let key_ok = secrets.gameserver_key.len() == 48
        && secrets
            .gameserver_key
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let pair_ok = (|| {
        let private = rcgen::KeyPair::from_pem(&pem(&secrets.signing_private_key)?).ok()?;
        let public = pem(&secrets.signing_public_key)?;
        let normalize = |text: &str| text.split_whitespace().collect::<String>();
        // An RSA-2048 SubjectPublicKeyInfo is 294 bytes
        Some(
            private.algorithm() == &rcgen::PKCS_RSA_SHA256
                && rcgen::PublicKeyData::subject_public_key_info(&private).len() >= 294
                && normalize(&private.public_key_pem()) == normalize(&public),
        )
    })()
    .unwrap_or(false);
    if key_ok && pair_ok {
        Ok(())
    } else {
        Err(StateError::Invalid)
    }
}

/// The saved secrets, or new ones published without overwriting another launch's.
pub fn load_secrets(state_root: &Path) -> Result<OfflineSecrets, StateError> {
    std::fs::create_dir_all(state_root)?;
    let file = state_root.join("secrets.json");
    match std::fs::read(&file) {
        Ok(bytes) => {
            // Neither the JSON nor the key text goes into an error message
            let saved: OfflineSecrets = serde_json::from_slice(&bytes).map_err(|_| StateError::Unreadable)?;
            validate(&saved).map_err(|_| StateError::Unreadable)?;
            return Ok(saved);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(StateError::Unreadable),
    }
    let generated = generate_secrets()?;
    let mut suffix = [0u8; 8];
    rand::fill(&mut suffix);
    let temporary = state_root.join(format!(
        "secrets.json.{}.tmp",
        suffix.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    write_new_private(
        &temporary,
        &serde_json::to_vec(&generated).map_err(|e| StateError::Other(e.to_string()))?,
    )?;
    // A link publishes the complete file, and fails if another launch published first
    let linked = std::fs::hard_link(&temporary, &file);
    let _ = std::fs::remove_file(&temporary);
    match linked {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    load_secrets(state_root)
}

/// Creates a file only its owner may read (where the file system knows owners); fails if it exists.
fn write_new_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

/// Replaces a file through a temporary one, private where the file system knows owners.
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let _ = std::fs::remove_file(&temporary);
    write_new_private(&temporary, data)?;
    std::fs::rename(&temporary, path)
}

/// A settings file the services' `.env` reader takes literally: single-quoted values, nothing that could
/// break a line or a quote.
pub fn serialize_env(values: &[(&str, String)]) -> Result<String, StateError> {
    let mut out = String::new();
    for (key, value) in values {
        let key_ok = key.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
            && key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        if !key_ok || value.contains(['\r', '\n', '\0', '\'']) {
            return Err(StateError::Other("invalid runtime environment value".into()));
        }
        out.push_str(&format!("{key}='{value}'\n"));
    }
    Ok(out)
}

/// The two services' settings (`offlineServiceConfig`): loopback only, the profile's database and secrets,
/// one city and the memory admission of `offline-memory.ts`.
/// One service's settings, in file order.
pub type ServiceEnv = Vec<(&'static str, String)>;

pub fn service_config(
    layout: &OfflineLayout,
    game_executable: &Path,
    secrets: &OfflineSecrets,
    server_console: bool,
) -> Result<(ServiceEnv, ServiceEnv), StateError> {
    if !game_executable.is_absolute() || !layout.database_path.is_absolute() {
        return Err(StateError::Other("Offline runtime paths must be absolute".into()));
    }
    let absolute = |path: &Path| path.to_string_lossy().replace('\\', "/");
    let text = |value: &str| value.to_owned();
    let metagame = vec![
        ("NODE_ENV", text("production")),
        ("PORT", text("61000")),
        ("BIND_HOST", text("127.0.0.1")),
        ("AUTH_MODE", text("APIKEY")),
        ("REGISTRATION_MODE", text("OPEN")),
        ("AUTH_SIGNING_PRIVKEY_B64", secrets.signing_private_key.clone()),
        ("AUTH_SIGNING_PUBKEY_B64", secrets.signing_public_key.clone()),
        ("DB_FILENAME", absolute(&layout.database_path)),
        ("MATCHMAKING_MODE", text("DEPLOYSERVER")),
        ("DEPLOYSERVER_URL", text("127.0.0.1:61001")),
        ("QOS_TARGET_URL", text("http://127.0.0.1:61000/QoS")),
        ("TARGET_CHANGELIST", text("239827")),
        ("CHAT", text("1")),
        ("CHAT_BIND_HOST", text("127.0.0.1")),
        ("CHAT_PORT", XMPP_PORT.to_string()),
        ("LOG_BODIES", text("0")),
        ("ACCOUNT_KEY_RECOVERY", text("0")),
    ];
    let deploy = vec![
        ("NODE_ENV", text("production")),
        ("PORT", text("61001")),
        ("BIND_HOST", text("127.0.0.1")),
        ("MY_IP", text("127.0.0.1")),
        ("PORT_RANGE_BEGIN", text("8770")),
        ("PORT_RANGE_END", text("8777")),
        ("GAMESERVER_BINARY_PATH", absolute(game_executable)),
        ("METAGAME_API_KEY", secrets.gameserver_key.clone()),
        ("RAMSGATE_POOL", text("1")),
        ("ENABLE_DOJO", text("0")),
        ("SERVER_ROLE", text("game")),
        ("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP", text("10")),
        ("GAMESERVER_READY_DIR", absolute(&layout.ready_dir)),
        ("GAMESERVER_MIN_FREE_MB", MIN_FREE_MIB.to_string()),
        ("GAMESERVER_STARTUP_MB", SERVER_STARTUP_MIB.to_string()),
        ("GAMESERVER_HUNT_STARTUP_MB", SERVER_STARTUP_MIB.to_string()),
        ("DR_SHOW_CONSOLE", text(if server_console { "1" } else { "0" })),
    ];
    Ok((metagame, deploy))
}

/// Writes both settings files and the game-server key file; the key file's path. The database belongs to
/// the metagame: this never creates, truncates or replaces it.
pub fn prepare(layout: &OfflineLayout, game_executable: &Path, server_console: bool) -> Result<PathBuf, StateError> {
    let secrets = load_secrets(&layout.state_root)?;
    let (metagame, deploy) = service_config(layout, game_executable, &secrets, server_console)?;
    std::fs::create_dir_all(&layout.runtime_state_dir)?;
    std::fs::create_dir_all(&layout.ready_dir)?;
    write_private(&layout.metagame_env_path, serialize_env(&metagame)?.as_bytes())?;
    write_private(&layout.deploy_env_path, serialize_env(&deploy)?.as_bytes())?;
    let key_file = layout.gameserver_key_path();
    write_private(&key_file, secrets.gameserver_key.as_bytes())?;
    Ok(key_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_made_once_kept_and_never_replaced_when_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_secrets(dir.path()).unwrap();
        assert_eq!(first.gameserver_key.len(), 48);
        assert_eq!(load_secrets(dir.path()).unwrap(), first, "kept");
        let file = dir.path().join("secrets.json");

        std::fs::write(&file, b"{not json").unwrap();
        let error = load_secrets(dir.path()).unwrap_err().to_string();
        assert!(error.contains("refusing to replace"), "{error}");
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"{not json",
            "the file is left for the player to recover"
        );

        // Halves of two different pairs
        let other = generate_secrets().unwrap();
        let mixed = OfflineSecrets {
            signing_public_key: other.signing_public_key,
            ..first.clone()
        };
        std::fs::write(&file, serde_json::to_vec(&mixed).unwrap()).unwrap();
        assert!(load_secrets(dir.path()).is_err());
        let short = OfflineSecrets {
            gameserver_key: "abc".into(),
            ..first
        };
        std::fs::write(&file, serde_json::to_vec(&short).unwrap()).unwrap();
        assert!(load_secrets(dir.path()).is_err());
    }

    #[test]
    fn the_settings_files_are_literal_and_loopback_only() {
        let dir = tempfile::tempdir().unwrap();
        let layout = OfflineLayout::new(dir.path(), dir.path());
        let exe = dir.path().join("Dauntless-Win64-Shipping.exe");
        let key_file = prepare(&layout, &exe, false).unwrap();
        let secrets = load_secrets(&layout.state_root).unwrap();
        assert_eq!(std::fs::read_to_string(key_file).unwrap(), secrets.gameserver_key);

        let env = service_kit::env::Env::load(Some(&layout.metagame_env_path)).unwrap();
        assert_eq!(env.raw("BIND_HOST"), Some("127.0.0.1"));
        assert_eq!(
            env.raw("AUTH_SIGNING_PRIVKEY_B64"),
            Some(secrets.signing_private_key.as_str())
        );
        assert_eq!(env.raw("CHAT_PORT"), Some("61099"));
        assert!(env.raw("DB_FILENAME").unwrap().ends_with("offline/undaunted.db"));
        let env = service_kit::env::Env::load(Some(&layout.deploy_env_path)).unwrap();
        assert_eq!(env.raw("METAGAME_API_KEY"), Some(secrets.gameserver_key.as_str()));
        assert_eq!(env.raw("GAMESERVER_MIN_FREE_MB"), Some("3072"));
        assert!(!layout.database_path.exists(), "the database is the metagame's to make");

        assert!(serialize_env(&[("A", "it's".into())]).is_err());
        assert!(serialize_env(&[("A", "x\ny".into())]).is_err());
        assert!(serialize_env(&[("lower", "x".into())]).is_err());
    }
}
