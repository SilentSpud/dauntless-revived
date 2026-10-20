#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! `metagame db …` as the kits run it, with the cases of deploy/windows-server/tests/dr-db.test.cjs.

use std::path::{Path, PathBuf};
use std::process::Command;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use metagame::crypto::sha256_hex;
use metagame::db::SCHEMA;
use store::{Options, Store};

fn metagame(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_metagame"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap().trim().to_owned(),
        String::from_utf8(out.stderr).unwrap().trim().to_owned(),
    )
}

fn database(dir: &Path) -> PathBuf {
    let path = dir.join("undaunted.db");
    drop(Store::open(&path, &Options::default(), &SCHEMA).unwrap());
    let mut conn = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    conn.batch_execute("INSERT INTO users (userId, name, notes) VALUES ('UID-a', 'Ada', 0), ('UID-b', 'Bo', 0)")
        .unwrap();
    path
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn integrity_counts_users_and_refuses_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(dir.path());
    assert_eq!(
        metagame(&["db", "integrity", s(&db)]),
        (true, "ok, 2 users".into(), String::new())
    );
    let (ok, _, err) = metagame(&["db", "integrity", s(&dir.path().join("missing.db"))]);
    assert!(!ok);
    assert!(err.starts_with("metagame db: no database at"), "{err}");
    assert!(!dir.path().join("missing.db").exists(), "never creates a database");
}

#[test]
fn gs_key_registers_the_hash_once_and_never_prints_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(dir.path());
    let key_file = dir.path().join("gameserver.key");
    let key = metagame::crypto::random_hex(24);
    std::fs::write(&key_file, format!("{key}\n")).unwrap();

    let first = metagame(&["db", "gs-key", s(&db), s(&key_file)]);
    assert_eq!((first.0, first.1.as_str()), (true, "game-server key registered"));
    let second = metagame(&["db", "gs-key", s(&db), s(&key_file)]);
    assert_eq!(second.1, "game-server key already registered");
    assert!(!first.1.contains(&key) && !first.2.contains(&key));

    #[derive(QueryableByName)]
    struct Hash {
        #[diesel(sql_type = diesel::sql_types::Text)]
        #[diesel(column_name = keyHash)]
        key_hash: String,
    }
    let mut conn = SqliteConnection::establish(s(&db)).unwrap();
    let hashes = diesel::sql_query("SELECT keyHash FROM gameserverapikeys")
        .load::<Hash>(&mut conn)
        .unwrap();
    assert_eq!(
        hashes.into_iter().map(|h| h.key_hash).collect::<Vec<_>>(),
        [sha256_hex(&key)]
    );

    std::fs::write(&key_file, "short").unwrap();
    assert_eq!(
        metagame(&["db", "gs-key", s(&db), s(&key_file)]).2,
        "metagame db: the game-server key file looks empty or truncated"
    );
    assert_eq!(
        metagame(&["db", "gs-key", s(&db), s(&dir.path().join("none.key"))]).2,
        "metagame db: gs-key needs the key file"
    );
}

#[test]
fn invites_and_admins() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(dir.path());
    assert_eq!(metagame(&["db", "add-invite", s(&db), "OWNER-1234"]).1, "invite added");
    assert_eq!(
        metagame(&["db", "del-invite", s(&db), "OWNER-1234"]).1,
        "invite removed"
    );
    assert_eq!(
        metagame(&["db", "add-invite", s(&db), "no spaces"]).2,
        "metagame db: bad invite code"
    );
    assert_eq!(metagame(&["db", "make-admin", s(&db), "UID-a"]).1, "admin flag set");
    assert_eq!(
        metagame(&["db", "make-admin", s(&db), "UID-nobody"]).2,
        "metagame db: no such user"
    );
}

#[test]
fn backup_checks_its_copy_and_journal_switches_for_a_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(dir.path());
    let copy = dir.path().join("2026-10-20_120000.db");
    assert_eq!(
        metagame(&["db", "backup", s(&db), s(&copy)]),
        (true, "db ok, 2 users".into(), String::new())
    );
    assert!(
        !metagame(&["db", "backup", s(&db), s(&copy)]).0,
        "never overwrites a backup"
    );
    assert_eq!(metagame(&["db", "journal", "delete", s(&db)]).1, "journal mode delete");
    assert!(!dir.path().join("undaunted.db-wal").exists());
    assert_eq!(metagame(&["db", "journal", "wal", s(&db)]).1, "journal mode wal");
}
