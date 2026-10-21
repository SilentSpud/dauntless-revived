//! Portable offline saves (`offline-save.ts`): backing the offline profile up to a `.drsave` file and
//! restoring one over it. The database half is the metagame crate's (`metagame::offline_save`, X1), called
//! in-process.
//!
//! A restore replaces two files, the database and the offline account's key, so it runs under a journal in
//! `offline/.save-restore`: copies of both, then `prepared`, then the new files, then `committed`. A failure
//! puts both back and writes `rolled-back`; a start after a crash rolls a `prepared` journal back. Terminal
//! journals only need cleaning up. A rollback that fails keeps its copies and stops offline play from
//! starting rather than lose them. This is recovery across two files, not one atomic step: power loss can
//! still defeat the file system's ordering.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use metagame::offline_save::{self, Account, SaveError};
use serde::{Deserialize, Serialize};

use super::layout::OfflineLayout;
use super::save_format::{FormatError, PortableIdentity, read_save, write_save};
use crate::bridge::types::{ActionResult, ErrorCode, LauncherError};
use crate::platform::keystore::{KeySlot, KeyStore, KeyStoreError};

/// The offline profile's key slot; never a network host.
pub fn offline_key_slot() -> KeySlot {
    KeySlot {
        host: "offline-local".into(),
        port: 61000,
        public: false,
        fp: None,
    }
}

#[derive(Debug)]
enum Failure {
    Invalid,
    KeyStorage,
    Other(String),
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<FormatError> for Failure {
    fn from(error: FormatError) -> Self {
        match error {
            FormatError::Invalid => Self::Invalid,
            FormatError::Io(error) => error.into(),
        }
    }
}

impl From<SaveError> for Failure {
    fn from(error: SaveError) -> Self {
        match error {
            SaveError::Invalid => Self::Invalid,
            SaveError::Io(reason) => Self::Other(reason),
        }
    }
}

impl From<KeyStoreError> for Failure {
    fn from(_: KeyStoreError) -> Self {
        Self::KeyStorage
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Prepared,
    Committed,
    RolledBack,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    version: u32,
    phase: Phase,
    db_existed: bool,
    key_existed: bool,
}

fn exists(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn durable_copy(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::copy(source, target)?;
    private(target)?;
    std::fs::OpenOptions::new().write(true).open(target)?.sync_all()
}

fn atomic_json(file: &Path, value: &Journal) -> std::io::Result<()> {
    let mut temporary = file.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, serde_json::to_vec(value).map_err(std::io::Error::other)?)?;
    private(&temporary)?;
    std::fs::OpenOptions::new().write(true).open(&temporary)?.sync_all()?;
    std::fs::rename(&temporary, file)
}

fn make_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub struct PortableSave {
    layout: OfflineLayout,
    user_data: PathBuf,
    keys: KeyStore,
    active: AtomicBool,
}

impl PortableSave {
    /// `keys` is the offline key store (no Documents backup: the offline identity stays in userData).
    pub fn new(layout: OfflineLayout, user_data: PathBuf, keys: KeyStore) -> Self {
        Self {
            layout,
            user_data,
            keys,
            active: AtomicBool::new(false),
        }
    }

    pub fn keys(&self) -> &KeyStore {
        &self.keys
    }

    fn transaction_dir(&self) -> PathBuf {
        self.layout.state_root.join(".save-restore")
    }

    fn key_file(&self) -> PathBuf {
        self.keys.file(&offline_key_slot())
    }

    fn begin(&self) -> bool {
        !self.active.swap(true, Ordering::SeqCst)
    }

    fn end(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    /// Resolves a journal a crash left behind, and removes staging folders. Before offline services start.
    pub fn recover(&self) -> Result<(), String> {
        if !self.begin() {
            return Err("Offline save recovery is busy".into());
        }
        let outcome = self.recover_journal().map_err(|error| format!("{error:?}"));
        self.end();
        outcome
    }

    fn rollback(&self, db_existed: bool, key_existed: bool) -> Result<(), Failure> {
        let dir = self.transaction_dir();
        for (existed, previous, target) in [
            (db_existed, "db.previous", self.layout.database_path.clone()),
            (key_existed, "key.previous", self.key_file()),
        ] {
            if existed {
                let mut temporary = target.as_os_str().to_owned();
                temporary.push(".restore-tmp");
                let temporary = PathBuf::from(temporary);
                durable_copy(&dir.join(previous), &temporary)?;
                std::fs::rename(&temporary, &target)?;
            } else if let Err(error) = std::fs::remove_file(&target)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(error.into());
            }
        }
        // The copies stay until a journal no longer needs them
        atomic_json(
            &dir.join("journal.json"),
            &Journal {
                version: 1,
                phase: Phase::RolledBack,
                db_existed,
                key_existed,
            },
        )?;
        Ok(())
    }

    fn recover_journal(&self) -> Result<(), Failure> {
        let dir = self.transaction_dir();
        if exists(&dir)? {
            if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
                return Err(Failure::Other("Unsafe Offline restore journal".into()));
            }
            let marker = dir.join("journal.json");
            if exists(&marker)? {
                let journal: Journal = serde_json::from_slice(&std::fs::read(&marker)?)
                    .map_err(|_| Failure::Other("Invalid Offline restore journal".into()))?;
                if journal.version != 1 {
                    return Err(Failure::Other("Invalid Offline restore journal".into()));
                }
                if journal.phase == Phase::Prepared {
                    self.rollback(journal.db_existed, journal.key_existed)?;
                }
            }
            // Terminal journals need cleaning up only, even when an earlier pass removed copies
            std::fs::remove_dir_all(&dir)?;
        }
        if exists(&self.layout.state_root)? {
            for entry in std::fs::read_dir(&self.layout.state_root)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let staging = name
                    .strip_prefix(".save-stage-")
                    .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()));
                if !staging {
                    continue;
                }
                if entry.file_type()?.is_symlink() {
                    return Err(Failure::Other("Unsafe Offline save staging directory".into()));
                }
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }

    /// The canonical path to write a save to: absolute, `.drsave`, and outside the profile and the app.
    fn destination(&self, file: &Path) -> Result<PathBuf, Failure> {
        let extension_ok = file
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("drsave"));
        if !file.is_absolute() || !extension_ok {
            return Err(Failure::Invalid);
        }
        let parent = file.parent().ok_or(Failure::Invalid)?.canonicalize()?;
        let canonical = parent.join(file.file_name().ok_or(Failure::Invalid)?);
        for root in [&self.user_data, &self.layout.resources_dir] {
            if let Ok(real) = root.canonicalize()
                && canonical.starts_with(&real)
            {
                return Err(Failure::Invalid);
            }
        }
        if exists(&canonical)? && !std::fs::symlink_metadata(&canonical)?.is_file() {
            return Err(Failure::Invalid);
        }
        Ok(canonical)
    }

    fn operate(&self, action: impl FnOnce(&Path) -> Result<(), Failure>) -> ActionResult {
        if !self.begin() {
            return ActionResult::err(ErrorCode::Busy);
        }
        let outcome = (|| {
            make_private_dir(&self.layout.state_root)?;
            self.recover_journal()?;
            let mut suffix = [0u8; 6];
            rand::fill(&mut suffix);
            let work = self.layout.state_root.join(format!(
                ".save-stage-{}",
                suffix.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ));
            std::fs::create_dir(&work)?;
            make_private_dir(&work)?;
            let outcome = action(&work);
            let _ = std::fs::remove_dir_all(&work);
            outcome
        })();
        self.end();
        match outcome {
            Ok(()) => ActionResult::Ok,
            Err(Failure::Invalid) => ActionResult::err(ErrorCode::OfflineSaveInvalid),
            Err(Failure::KeyStorage) => ActionResult::err(ErrorCode::KeyStorageUnavailable),
            Err(Failure::Other(reason)) => {
                crate::platform::log::warn(&format!("offline save failed: {reason}"));
                ActionResult::Err(LauncherError::new(ErrorCode::Unknown))
            }
        }
    }

    /// Backs the offline profile up to `file` (a stopped stack only; the caller holds the controller's lock).
    pub fn export(&self, file: &Path) -> ActionResult {
        self.operate(|work| {
            let destination = self.destination(file)?;
            let key = self.keys.load(&offline_key_slot())?.ok_or(Failure::Invalid)?;
            let database = work.join("portable.db");
            let exported = offline_save::export(&self.layout.database_path, &database, &key)?;
            let identity = PortableIdentity {
                user_id: exported.account.user_id,
                username: exported.account.username,
                key: exported.key,
            };
            let mut suffix = [0u8; 8];
            rand::fill(&mut suffix);
            let mut temporary = destination.as_os_str().to_owned();
            temporary.push(format!(
                ".{}.tmp",
                suffix.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ));
            let temporary = PathBuf::from(temporary);
            let outcome = (|| {
                write_save(&temporary, &database, &identity)?;
                // What was written must read back and import before it replaces anything
                let verified = work.join("verified.db");
                let metadata = read_save(&temporary, &verified)?;
                offline_save::import(
                    &verified,
                    &metadata.credential.key,
                    Some(&Account {
                        user_id: metadata.account.user_id,
                        username: metadata.account.username,
                    }),
                )?;
                std::fs::rename(&temporary, &destination)?;
                Ok::<_, Failure>(())
            })();
            let _ = std::fs::remove_file(&temporary);
            outcome
        })
    }

    /// Restores the offline profile from `file` under the journal.
    pub fn import(&self, file: &Path) -> ActionResult {
        self.operate(|work| {
            let database = work.join("portable.db");
            let metadata = read_save(file, &database)?;
            let key = metadata.credential.key.clone();
            offline_save::import(
                &database,
                &key,
                Some(&Account {
                    user_id: metadata.account.user_id,
                    username: metadata.account.username,
                }),
            )?;
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut sidecar = self.layout.database_path.as_os_str().to_owned();
                sidecar.push(suffix);
                if exists(Path::new(&sidecar))? {
                    return Err(Failure::Invalid);
                }
            }
            let key_file = self.key_file();
            if let Some(parent) = key_file.parent() {
                make_private_dir(parent)?;
            }
            let dir = self.transaction_dir();
            std::fs::create_dir(&dir)?;
            make_private_dir(&dir)?;
            let db_existed = exists(&self.layout.database_path)?;
            let key_existed = exists(&key_file)?;
            let mut prepared = false;
            let outcome = (|| {
                if db_existed {
                    durable_copy(&self.layout.database_path, &dir.join("db.previous"))?;
                }
                if key_existed {
                    durable_copy(&key_file, &dir.join("key.previous"))?;
                }
                let marker = dir.join("journal.json");
                atomic_json(
                    &marker,
                    &Journal {
                        version: 1,
                        phase: Phase::Prepared,
                        db_existed,
                        key_existed,
                    },
                )?;
                prepared = true;
                std::fs::rename(&database, &self.layout.database_path)?;
                self.keys.save(&offline_key_slot(), &key)?;
                if self.keys.load(&offline_key_slot())?.as_deref() != Some(key.as_str()) {
                    return Err(Failure::KeyStorage);
                }
                std::fs::OpenOptions::new().write(true).open(&key_file)?.sync_all()?;
                atomic_json(
                    &marker,
                    &Journal {
                        version: 1,
                        phase: Phase::Committed,
                        db_existed,
                        key_existed,
                    },
                )?;
                Ok::<_, Failure>(())
            })();
            match outcome {
                Ok(()) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    Ok(())
                }
                Err(error) => {
                    if prepared {
                        self.rollback(db_existed, key_existed)?;
                    }
                    std::fs::remove_dir_all(&dir)?;
                    Err(error)
                }
            }
        })
    }
}
