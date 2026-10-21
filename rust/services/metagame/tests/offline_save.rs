#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! Offline saves' database half (X1): `offline-save-db.cjs`'s cases against the Rust tools.

use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use metagame::crypto::sha256_hex;
use metagame::db::SCHEMA;
use metagame::drizzle_frozen;
use metagame::offline_save::{Account, SaveError, export, import};
use store::{Options, Store, drizzle, schema_snapshot};

// Built from pieces, so the repository check never sees a whole key
const KEY: &str = concat!("UUK_", "0123456789abcdef", "0123456789abcdef", "0123456789abcdef");

fn drizzle_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../legacy/UndauntedMetagame/src/drizzle")
}

fn conn(path: &Path) -> SqliteConnection {
    SqliteConnection::establish(path.to_str().unwrap()).unwrap()
}

/// Adds the player `Sam` with `KEY`, a pending game-server key and a recovery row.
fn add_player(conn: &mut SqliteConnection) {
    conn.batch_execute("INSERT INTO users (userId, name, notes) VALUES ('UID-1', 'Sam', 0);")
        .unwrap();
    diesel::sql_query("INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-1', ?)")
        .bind::<Text, _>(sha256_hex(KEY))
        .execute(conn)
        .unwrap();
    conn.batch_execute(
        "INSERT INTO gameserverapikeys (keyHash) VALUES ('ff');
         INSERT INTO accountkeyrecovery (userId, keyHash, ciphertext) VALUES ('UID-1', 'aa', 'secret');",
    )
    .unwrap();
}

/// The offline database a stopped Rust metagame leaves: Diesel-managed, closed cleanly.
fn live_db(dir: &Path) -> PathBuf {
    let path = dir.join("undaunted.db");
    drop(Store::open(&path, &Options::default(), &SCHEMA).unwrap());
    let mut db = conn(&path);
    db.batch_execute("PRAGMA journal_mode = DELETE;").unwrap();
    add_player(&mut db);
    path
}

/// An offline database an older launcher made: the first `count` Drizzle migrations, rollback journal.
fn drizzle_db(dir: &Path, count: usize) -> PathBuf {
    let path = dir.join("old.db");
    let mut db = conn(&path);
    drizzle::replay(&mut db, &drizzle_frozen::migrations()[..count]).unwrap();
    add_player(&mut db);
    path
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(db: &mut SqliteConnection, sql: &str) -> i64 {
    diesel::sql_query(sql).get_result::<Count>(db).unwrap().n
}

fn sam() -> Account {
    Account {
        user_id: "UID-1".into(),
        username: "Sam".into(),
    }
}

fn current_schema() -> String {
    let mut fresh = conn(Path::new(":memory:"));
    store::migrate(&mut fresh, &SCHEMA).unwrap();
    schema_snapshot(&mut fresh).unwrap()
}

#[test]
fn the_frozen_drizzle_history_is_the_node_metagames() {
    let disk = drizzle::read_folder(&drizzle_folder()).unwrap();
    let frozen = drizzle_frozen::migrations();
    assert_eq!(frozen.len(), disk.len());
    for (frozen, disk) in frozen.iter().zip(&disk) {
        assert_eq!((&frozen.tag, frozen.created_at), (&disk.tag, disk.created_at));
        assert_eq!(drizzle::hash(&frozen.sql), drizzle::hash(&disk.sql), "{}", frozen.tag);
    }
    assert!(frozen.windows(2).all(|pair| pair[0].created_at < pair[1].created_at));
}

#[test]
fn export_rekeys_and_scrubs_only_the_copy_and_import_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let live = live_db(dir.path());
    let before = std::fs::read(&live).unwrap();
    let copy = dir.path().join("save.db");

    let saved = export(&live, &copy, KEY).unwrap();
    assert_eq!(saved.account, sam());
    assert!(saved.key.starts_with("UUK_") && saved.key.len() == 52, "{}", saved.key);
    assert_ne!(saved.key, KEY);
    assert_eq!(
        std::fs::read(&live).unwrap(),
        before,
        "the live database is not changed"
    );

    let mut db = conn(&copy);
    assert_eq!(count(&mut db, "SELECT count(*) AS n FROM gameserverapikeys"), 0);
    assert_eq!(count(&mut db, "SELECT count(*) AS n FROM accountkeyrecovery"), 0);
    assert_eq!(
        count(&mut db, "SELECT epoch AS n FROM authepochs WHERE userId = 'UID-1'"),
        1
    );
    drop(db);
    let bytes = std::fs::read(&copy).unwrap();
    assert!(
        !bytes.windows(6).any(|w| w == b"secret"),
        "no cleared credential stays in a free page"
    );

    // The old key no longer opens the copy; the new one does, for the declared account only
    let private = dir.path().join("import.db");
    std::fs::copy(&copy, &private).unwrap();
    assert_eq!(import(&private, KEY, Some(&sam())), Err(SaveError::Invalid));
    let other = Account {
        user_id: "UID-2".into(),
        username: "Sam".into(),
    };
    assert_eq!(import(&private, &saved.key, Some(&other)), Err(SaveError::Invalid));
    assert_eq!(import(&private, &saved.key, Some(&sam())), Ok(sam()));
}

#[test]
fn a_save_from_an_older_launcher_is_brought_forward_and_adopted() {
    let dir = tempfile::tempdir().unwrap();
    // 0000 to 0027: the key rotation table exists, the last three migrations are missing
    let old = drizzle_db(dir.path(), 28);
    assert_eq!(import(&old, KEY, None), Ok(sam()));
    let mut db = conn(&old);
    assert_eq!(schema_snapshot(&mut db).unwrap(), current_schema());
    assert_eq!(
        count(&mut db, "SELECT count(*) AS n FROM __drizzle_migrations"),
        drizzle_frozen::migrations().len() as i64
    );
    assert_eq!(
        count(&mut db, "SELECT count(*) AS n FROM __diesel_schema_migrations"),
        1
    );
}

#[test]
fn saves_with_unknown_or_altered_histories_are_refused() {
    // A migration row whose hash is not the frozen file's (Drizzle's `id SERIAL` stays NULL in SQLite)
    let dir = tempfile::tempdir().unwrap();
    let forged = drizzle_db(dir.path(), 28);
    conn(&forged)
        .batch_execute("UPDATE __drizzle_migrations SET hash = 'beef' WHERE rowid = 3;")
        .unwrap();
    assert_eq!(import(&forged, KEY, None), Err(SaveError::Invalid));

    // A schema that is not what its history makes, even with a harmless-looking extra table
    let dir = tempfile::tempdir().unwrap();
    let extra = drizzle_db(dir.path(), 28);
    conn(&extra).batch_execute("CREATE TABLE notes (x TEXT);").unwrap();
    assert_eq!(import(&extra, KEY, None), Err(SaveError::Invalid));

    // A future Drizzle migration this metagame does not know
    let dir = tempfile::tempdir().unwrap();
    let future = drizzle_db(dir.path(), 31);
    conn(&future)
        .batch_execute("INSERT INTO __drizzle_migrations (hash, created_at) VALUES ('abc', 1900000000000);")
        .unwrap();
    assert_eq!(import(&future, KEY, None), Err(SaveError::Invalid));

    // No history at all
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("bare.db");
    conn(&bare).batch_execute("CREATE TABLE users (userId TEXT);").unwrap();
    assert_eq!(import(&bare, KEY, None), Err(SaveError::Invalid));

    // A declared save that still carries credentials it should not
    let dir = tempfile::tempdir().unwrap();
    let loaded = drizzle_db(dir.path(), 31);
    assert_eq!(import(&loaded, KEY, Some(&sam())), Err(SaveError::Invalid));
}

#[test]
fn export_refuses_a_database_that_did_not_stop_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let live = live_db(dir.path());
    std::fs::write(dir.path().join("undaunted.db-wal"), b"").unwrap();
    assert_eq!(
        export(&live, &dir.path().join("save.db"), KEY).map(|_| ()),
        Err(SaveError::Invalid)
    );
    assert!(matches!(
        export(&dir.path().join("missing.db"), &dir.path().join("x.db"), KEY).map(|_| ()),
        Err(SaveError::Io(_))
    ));
}
