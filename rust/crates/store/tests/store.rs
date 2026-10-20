#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

use std::time::Duration;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use diesel_migrations::{EmbeddedMigrations, embed_migrations};
use store::{Error, Options, Schema, Store};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("tests/migrations");
const SCHEMA: Schema = Schema {
    migrations: || MIGRATIONS,
    drizzle: None,
};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Mode {
    #[diesel(sql_type = Text)]
    journal_mode: String,
}

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("test.db"), &Options::default(), &SCHEMA).unwrap();
    (dir, store)
}

fn count(conn: &mut store::Connection) -> Result<i64, Error> {
    Ok(diesel::sql_query("SELECT count(*) AS n FROM items")
        .get_result::<Count>(conn)?
        .n)
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_commit_and_errors_roll_back() {
    let (_dir, store) = open();
    store
        .write(|c| {
            c.batch_execute("INSERT INTO items (name) VALUES ('a')")
                .map_err(Error::from)
        })
        .await
        .unwrap();
    let failed: Result<(), Error> = store
        .write(|c| {
            c.batch_execute("INSERT INTO items (name) VALUES ('b')")?;
            Err(Error::Migration("refused".into()))
        })
        .await;
    assert!(failed.is_err());
    assert_eq!(store.read(count).await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_cannot_write() {
    let (_dir, store) = open();
    let refused = store
        .read(|c| {
            c.batch_execute("INSERT INTO items (name) VALUES ('a')")
                .map_err(Error::from)
        })
        .await;
    assert!(refused.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_database_is_in_wal_mode() {
    let (_dir, store) = open();
    let mode = store
        .read(|c| {
            Ok::<_, Error>(
                diesel::sql_query("PRAGMA journal_mode")
                    .get_result::<Mode>(c)?
                    .journal_mode,
            )
        })
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    store.checkpoint().await.unwrap();
}

// SQLite is built like better-sqlite3's (.cargo/config.toml): these differ from SQLite's defaults.
#[tokio::test(flavor = "multi_thread")]
async fn sqlite_behaves_like_better_sqlite3() {
    let (_dir, store) = open();
    let double_quoted_string = store
        .read(|c| c.batch_execute("SELECT \"not a column\"").map_err(Error::from))
        .await;
    assert!(
        double_quoted_string.is_err(),
        "a double-quoted string is an identifier only"
    );
    let orphan = store
        .write(|c| {
            c.batch_execute("INSERT INTO tags (item, tag) VALUES (42, 'x')")
                .map_err(Error::from)
        })
        .await;
    assert!(orphan.is_err(), "foreign keys are enforced");
    store
        .read(|c| c.batch_execute("SELECT sqrt(4), ln(1)").map_err(Error::from))
        .await
        .unwrap();
}

// M16: a request dropped mid-transaction doesn't leave its transaction open.
#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_request_still_finishes_its_transaction() {
    let (_dir, store) = open();
    let slow = store.write(|c| {
        c.batch_execute("INSERT INTO items (name) VALUES ('slow')")?;
        std::thread::sleep(Duration::from_millis(300));
        Ok::<_, Error>(())
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), slow).await.is_err(),
        "the request was dropped"
    );
    store
        .write(|c| {
            c.batch_execute("INSERT INTO items (name) VALUES ('next')")
                .map_err(Error::from)
        })
        .await
        .unwrap();
    assert_eq!(store.read(count).await.unwrap(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panicking_closure_rolls_back_and_the_store_keeps_working() {
    let (_dir, store) = open();
    let panicked: Result<(), Error> = store
        .write(|c| {
            c.batch_execute("INSERT INTO items (name) VALUES ('lost')")?;
            panic!("handler bug");
        })
        .await;
    assert!(matches!(panicked, Err(Error::Panicked)));
    store
        .write(|c| {
            c.batch_execute("INSERT INTO items (name) VALUES ('kept')")
                .map_err(Error::from)
        })
        .await
        .unwrap();
    assert_eq!(store.read(count).await.unwrap(), 1);
}

#[test]
fn a_database_with_unknown_tables_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("other.db");
    SqliteConnection::establish(path.to_str().unwrap())
        .unwrap()
        .batch_execute("CREATE TABLE other (x)")
        .unwrap();
    assert!(matches!(
        Store::open(&path, &Options::default(), &SCHEMA),
        Err(Error::Adoption(_))
    ));
}
