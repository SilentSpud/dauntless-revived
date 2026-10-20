#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The baseline migration against the Node metagame's Drizzle history (M14, M18).

use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use metagame::db::{DRIZZLE_TAG, SCHEMA};
use store::{Error, Options, Store, drizzle, schema_snapshot};

fn drizzle_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../UndauntedMetagame/src/drizzle")
}

fn baseline() -> &'static str {
    SCHEMA.drizzle.as_ref().unwrap().snapshot
}

/// A database file the way the Node metagame leaves it after `migrations`.
fn drizzle_db(dir: &Path, migrations: &[drizzle::Migration]) -> PathBuf {
    let path = dir.join("undaunted.db");
    let mut conn = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    drizzle::replay(&mut conn, migrations).unwrap();
    path
}

#[derive(QueryableByName)]
struct Version {
    #[diesel(sql_type = Text)]
    version: String,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[test]
fn the_baseline_is_cut_from_the_last_drizzle_migration() {
    let migrations = drizzle::read_folder(&drizzle_folder()).unwrap();
    let last = migrations.last().unwrap();
    let baseline = SCHEMA.drizzle.as_ref().unwrap();
    assert_eq!(last.tag, DRIZZLE_TAG);
    assert_eq!(last.created_at, baseline.created_at);
    assert_eq!(drizzle::hash(&last.sql), drizzle::hash(baseline.sql));
}

#[test]
fn drizzle_history_and_baseline_give_the_same_schema() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = drizzle::read_folder(&drizzle_folder()).unwrap();
    let mut replayed = SqliteConnection::establish(drizzle_db(dir.path(), &migrations).to_str().unwrap()).unwrap();
    assert_eq!(schema_snapshot(&mut replayed).unwrap(), baseline());

    let fresh = Store::open(&dir.path().join("fresh.db"), &Options::default(), &SCHEMA).unwrap();
    drop(fresh);
    let mut fresh = SqliteConnection::establish(dir.path().join("fresh.db").to_str().unwrap()).unwrap();
    assert_eq!(schema_snapshot(&mut fresh).unwrap(), baseline());
}

#[test]
fn a_node_database_is_adopted_with_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = drizzle::read_folder(&drizzle_folder()).unwrap();
    let path = drizzle_db(dir.path(), &migrations);
    {
        let mut conn = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
        conn.batch_execute("INSERT INTO users (userId, name, notes) VALUES ('UID-a', 'Ada', 0)")
            .unwrap();
    }

    let store = Store::open(&path, &Options::default(), &SCHEMA).unwrap();
    drop(store);
    // Opening again finds the baseline recorded and has nothing to do.
    drop(Store::open(&path, &Options::default(), &SCHEMA).unwrap());

    let mut conn = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    let versions = diesel::sql_query("SELECT version FROM __diesel_schema_migrations")
        .load::<Version>(&mut conn)
        .unwrap();
    assert_eq!(
        versions.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(),
        ["0001"]
    );
    let drizzle_rows = diesel::sql_query("SELECT count(*) AS n FROM __drizzle_migrations")
        .get_result::<Count>(&mut conn)
        .unwrap();
    assert_eq!(
        drizzle_rows.n,
        migrations.len() as i64,
        "__drizzle_migrations is left as it was"
    );
    let users = diesel::sql_query("SELECT count(*) AS n FROM users")
        .get_result::<Count>(&mut conn)
        .unwrap();
    assert_eq!(users.n, 1);
}

#[test]
fn an_older_node_database_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = drizzle::read_folder(&drizzle_folder()).unwrap();
    let path = drizzle_db(dir.path(), &migrations[..migrations.len() - 1]);
    let refused = Store::open(&path, &Options::default(), &SCHEMA).err();
    assert!(matches!(refused, Some(Error::Adoption(_))), "{refused:?}");
}

#[test]
fn a_node_database_with_a_changed_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = drizzle::read_folder(&drizzle_folder()).unwrap();
    let path = drizzle_db(dir.path(), &migrations);
    {
        let mut conn = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
        conn.batch_execute("CREATE INDEX extra ON users (name)").unwrap();
    }
    let refused = Store::open(&path, &Options::default(), &SCHEMA).err();
    assert!(matches!(refused, Some(Error::Adoption(_))), "{refused:?}");
}
