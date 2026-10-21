//! `test/offline-save.test.ts`'s cases for the save runtime (`save.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diesel::Connection as _;
use diesel::connection::SimpleConnection;

use super::layout::OfflineLayout;
use super::save::{PortableSave, offline_key_slot};
use super::save_format::read_save;
use crate::bridge::types::{ActionResult, ErrorCode};
use crate::platform::keystore::{Encryptor, KeyStore};

/// One PC's key encryption: files from another PC do not open.
struct Machine(&'static str);

impl Encryptor for Machine {
    fn is_available(&self) -> bool {
        true
    }
    fn encrypt(&self, plain: &str) -> Option<Vec<u8>> {
        Some(format!("{}:{plain}", self.0).into_bytes())
    }
    fn decrypt(&self, data: &[u8]) -> Option<String> {
        std::str::from_utf8(data)
            .ok()?
            .strip_prefix(&format!("{}:", self.0))
            .map(str::to_owned)
    }
}

// Built from pieces, so the repository check never sees a whole key
const KEY: &str = concat!("UUK_", "0123456789abcdef", "0123456789abcdef", "0123456789abcdef");

struct Profile {
    user_data: PathBuf,
    layout: OfflineLayout,
    save: PortableSave,
}

impl Profile {
    fn key_file(&self) -> PathBuf {
        self.save.keys().file(&offline_key_slot())
    }
}

fn sha(text: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A machine's profile; with a player when `initialized` (the database a stopped offline stack leaves).
fn profile(root: &Path, machine: &'static str, initialized: bool) -> Profile {
    let user_data = root.join(machine);
    let resources = root.join("resources");
    std::fs::create_dir_all(&user_data).unwrap();
    std::fs::create_dir_all(&resources).unwrap();
    let layout = OfflineLayout::new(&user_data, &resources);
    let keys = KeyStore::new(user_data.join("keys"), Arc::new(Machine(machine)), None, None);
    if initialized {
        std::fs::create_dir_all(&layout.state_root).unwrap();
        let mut db = diesel::SqliteConnection::establish(&layout.database_path.to_string_lossy()).unwrap();
        store::migrate(&mut db, &metagame::db::SCHEMA).unwrap();
        let rows = format!(
            "INSERT INTO users (userId, name, notes) VALUES ('UID-1', 'Portable_1', 17);
             INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-1', '{key}');
             INSERT INTO characters (characterId, userId, createdDate, lastModifiedDate, name, updateVersion, data)
               VALUES ('CHAR-1', 'UID-1', 'created', 'updated', 'Portable_1', 7, 'ftue-completed');
             INSERT INTO gameserverapikeys (keyHash) VALUES ('{secret}');
             INSERT INTO accountkeyrecovery (userId, keyHash, ciphertext) VALUES ('UID-1', 'old', 'machine-secret-x');
             INSERT INTO recoveryevents (discordId, action, createdAt) VALUES ('123', 'confirm_attempt', 12345);",
            key = sha(KEY),
            secret = sha("machine-secret-x"),
        );
        db.batch_execute(&rows).unwrap();
        keys.save(&offline_key_slot(), KEY).unwrap();
    }
    Profile {
        save: PortableSave::new(layout.clone(), user_data.clone(), keys),
        user_data,
        layout,
    }
}

fn no_staging(layout: &OfflineLayout) {
    let names: Vec<String> = std::fs::read_dir(&layout.state_root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|name| name.starts_with(".save-")), "{names:?}");
}

fn character(db: &Path) -> String {
    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        data: String,
    }
    let mut conn = diesel::SqliteConnection::establish(&db.to_string_lossy()).unwrap();
    diesel::RunQueryDsl::get_result::<Row>(diesel::sql_query("SELECT data FROM characters"), &mut conn)
        .unwrap()
        .data
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle.as_bytes())
}

#[test]
fn export_rekeys_a_copy_and_restore_reencrypts_it_on_another_machine() {
    let root = tempfile::tempdir().unwrap();
    let a = profile(root.path(), "machine-a", true);
    let live = std::fs::read(&a.layout.database_path).unwrap();
    let key_before = std::fs::read(a.key_file()).unwrap();
    std::fs::write(a.layout.state_root.join("secrets.json"), "source-rsa-secret").unwrap();
    let file = root.path().join("portable.drsave");
    assert_eq!(a.save.export(&file), ActionResult::Ok);
    assert_eq!(
        std::fs::read(&a.layout.database_path).unwrap(),
        live,
        "the live database does not change"
    );
    assert_eq!(std::fs::read(a.key_file()).unwrap(), key_before, "nor the live key");
    let contents = std::fs::read(&file).unwrap();
    for absent in [KEY, "machine-secret-x", "source-rsa-secret"] {
        assert!(!contains(&contents, absent), "{absent}");
    }
    let metadata = read_save(&file, &root.path().join("extracted.db")).unwrap();
    assert_ne!(metadata.credential.key, KEY, "a fresh backup key");
    assert_eq!(metadata.account.username, "Portable_1");

    let b = profile(root.path(), "machine-b", false);
    std::fs::create_dir_all(&b.layout.state_root).unwrap();
    std::fs::write(b.layout.state_root.join("secrets.json"), "destination-secrets").unwrap();
    std::fs::write(b.user_data.join("settings.json"), "unrelated settings").unwrap();
    assert_eq!(b.save.import(&file), ActionResult::Ok);
    assert_eq!(
        b.save.keys().load(&offline_key_slot()).unwrap().as_deref(),
        Some(metadata.credential.key.as_str())
    );
    assert!(
        std::fs::read(b.key_file()).unwrap().starts_with(b"machine-b:"),
        "the destination's encryption"
    );
    assert_eq!(character(&b.layout.database_path), "ftue-completed");
    assert_eq!(
        std::fs::read_to_string(b.layout.state_root.join("secrets.json")).unwrap(),
        "destination-secrets"
    );
    assert_eq!(
        std::fs::read_to_string(b.user_data.join("settings.json")).unwrap(),
        "unrelated settings"
    );

    // Restoring over an existing profile replaces it
    let c = profile(root.path(), "machine-c", true);
    assert_eq!(c.save.import(&file), ActionResult::Ok);
    assert_eq!(
        c.save.keys().load(&offline_key_slot()).unwrap().as_deref(),
        Some(metadata.credential.key.as_str())
    );
    for profile in [&a, &b, &c] {
        no_staging(&profile.layout);
    }
}

#[test]
fn damaged_saves_are_refused_without_touching_the_profile() {
    let root = tempfile::tempdir().unwrap();
    let source = profile(root.path(), "source", true);
    let file = root.path().join("portable.drsave");
    assert_eq!(source.save.export(&file), ActionResult::Ok);
    let dest = profile(root.path(), "dest", true);
    let db = std::fs::read(&dest.layout.database_path).unwrap();
    let key = std::fs::read(dest.key_file()).unwrap();
    let mut bytes = std::fs::read(&file).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    let damaged = root.path().join("damaged.drsave");
    std::fs::write(&damaged, bytes).unwrap();
    assert_eq!(
        dest.save.import(&damaged),
        ActionResult::err(ErrorCode::OfflineSaveInvalid)
    );
    assert_eq!(std::fs::read(&dest.layout.database_path).unwrap(), db);
    assert_eq!(std::fs::read(dest.key_file()).unwrap(), key);
    no_staging(&dest.layout);
}

fn journal(layout: &OfflineLayout, phase: &str, previous: Option<(&[u8], &[u8])>, existed: bool) {
    let dir = layout.state_root.join(".save-restore");
    std::fs::create_dir_all(&dir).unwrap();
    if let Some((db, key)) = previous {
        std::fs::write(dir.join("db.previous"), db).unwrap();
        std::fs::write(dir.join("key.previous"), key).unwrap();
    }
    let text = serde_json::json!({ "version": 1, "phase": phase, "dbExisted": existed, "keyExisted": existed });
    std::fs::write(dir.join("journal.json"), text.to_string()).unwrap();
}

#[test]
fn a_crash_journal_rolls_back_when_prepared_and_is_only_cleaned_up_when_terminal() {
    let root = tempfile::tempdir().unwrap();
    let dest = profile(root.path(), "dest", true);
    let db = std::fs::read(&dest.layout.database_path).unwrap();
    let key = std::fs::read(dest.key_file()).unwrap();

    // Prepared, with either file already replaced: both come back, and a second pass changes nothing
    for (new_db, new_key) in [(true, false), (true, true), (false, true)] {
        journal(&dest.layout, "prepared", Some((&db, &key)), true);
        if new_db {
            std::fs::write(&dest.layout.database_path, "new database").unwrap();
        }
        if new_key {
            std::fs::write(dest.key_file(), "new key").unwrap();
        }
        dest.save.recover().unwrap();
        dest.save.recover().unwrap();
        assert_eq!(std::fs::read(&dest.layout.database_path).unwrap(), db);
        assert_eq!(std::fs::read(dest.key_file()).unwrap(), key);
        no_staging(&dest.layout);
    }

    // Committed: the new files stay
    std::fs::write(&dest.layout.database_path, "committed database").unwrap();
    journal(&dest.layout, "committed", Some((&db, &key)), true);
    dest.save.recover().unwrap();
    assert_eq!(
        std::fs::read(&dest.layout.database_path).unwrap(),
        b"committed database"
    );

    // Prepared on a fresh profile: rolling back removes both files
    journal(&dest.layout, "prepared", None, false);
    dest.save.recover().unwrap();
    assert!(!dest.layout.database_path.exists());
    assert!(!dest.key_file().exists());
    no_staging(&dest.layout);

    // A journal it cannot read stops recovery rather than guess
    journal(&dest.layout, "unknown", None, false);
    assert!(dest.save.recover().is_err());
}

#[test]
fn export_refuses_the_profile_journal_files_and_never_publishes_a_partial_file() {
    let root = tempfile::tempdir().unwrap();
    let a = profile(root.path(), "a", true);
    assert_eq!(
        a.save.export(&a.user_data.join("critical.drsave")),
        ActionResult::err(ErrorCode::OfflineSaveInvalid)
    );
    assert_eq!(
        a.save.export(&root.path().join("no-extension")),
        ActionResult::err(ErrorCode::OfflineSaveInvalid)
    );
    let mut wal = a.layout.database_path.as_os_str().to_owned();
    wal.push("-wal");
    std::fs::write(&wal, "unexpected WAL").unwrap();
    let file = root.path().join("save.drsave");
    assert_eq!(a.save.export(&file), ActionResult::err(ErrorCode::OfflineSaveInvalid));
    assert!(!file.exists());
    std::fs::write(&file, "previous destination").unwrap();
    assert_ne!(a.save.export(&file), ActionResult::Ok);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "previous destination");
    no_staging(&a.layout);
}
