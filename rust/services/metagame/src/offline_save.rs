//! The database half of a portable offline save (X1): `offline-save-db.cjs` of the Electron launcher,
//! which the launcher calls in-process and `metagame offline-save` offers on the command line.
//!
//! - **Export** copies the stopped offline database, and only the copy is changed: its player gets a fresh
//!   random key (the save's own backup credential), the bootstrap queues, game-server keys and
//!   machine-bound recovery material are cleared, the account's token epoch goes up, and `VACUUM` with
//!   `secure_delete` rewrites the file so no old credential stays in a free page.
//! - **Import** works on a private copy of a save. Its migration history must be a nonempty prefix of the
//!   known history (the frozen Drizzle migrations of older launchers, or this metagame's Diesel
//!   migrations), and its schema must be exactly what that prefix makes. Then the rest is applied: the
//!   remaining Drizzle SQL, the Diesel baseline's adoption and any newer Diesel migrations (D11). The
//!   result must have the current schema, pass `integrity_check`, stay one file, and hold exactly one
//!   account whose key is the save's credential.
//!
//! Errors carry no detail of the credential or the data: `Invalid` for anything wrong with the save,
//! `Io` for the file system.

use std::path::Path;

use diesel::Connection as _;
use diesel::connection::SimpleConnection;
use diesel::migration::MigrationSource;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable, Text};
use diesel::sqlite::Sqlite;
use diesel_migrations::MigrationHarness;
use store::{Connection, drizzle, schema_snapshot};

use crate::crypto::{random_hex, sha256_hex};
use crate::db::{MIGRATIONS, SCHEMA};
use crate::drizzle_frozen;

/// Credentials and machine-bound data a save never carries.
pub const CLEARED_TABLES: [&str; 6] = [
    "userapikeystoregister",
    "gameserverapikeystoregister",
    "gameserverapikeys",
    "accountkeyrecovery",
    "recoveryproofs",
    "recoverychallenges",
];

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    #[error("invalid offline database")]
    Invalid,
    #[error("offline database file: {0}")]
    Io(String),
}

impl From<diesel::result::Error> for SaveError {
    fn from(error: diesel::result::Error) -> Self {
        let text = error.to_string();
        // SQLite's own words for a file-system failure; anything else is the save's fault
        if ["disk I/O error", "database or disk is full", "unable to open"]
            .iter()
            .any(|io| text.contains(io))
        {
            Self::Io(text)
        } else {
            Self::Invalid
        }
    }
}

impl From<store::Error> for SaveError {
    fn from(error: store::Error) -> Self {
        match error {
            store::Error::Query(error) => error.into(),
            store::Error::Connection(error) => Self::Io(error.to_string()),
            store::Error::Backup(reason) => Self::Io(reason),
            _ => Self::Invalid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub user_id: String,
    pub username: String,
}

/// An exported copy's account and the new key that is its credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub account: Account,
    pub key: String,
}

fn open(path: &Path) -> Result<Connection, SaveError> {
    // establish would create a missing file
    if !path.is_file() {
        return Err(SaveError::Io(format!("{} does not exist", path.display())));
    }
    let mut conn = Connection::establish(&path.to_string_lossy()).map_err(|e| SaveError::Io(e.to_string()))?;
    // A save's schema is untrusted until it is compared: none of its functions or views may run
    conn.batch_execute("PRAGMA trusted_schema = OFF;")?;
    Ok(conn)
}

fn memory() -> Result<Connection, SaveError> {
    Connection::establish(":memory:").map_err(|e| SaveError::Io(e.to_string()))
}

#[derive(QueryableByName)]
struct Integrity {
    #[diesel(sql_type = Text)]
    integrity_check: String,
}

#[derive(QueryableByName)]
struct JournalMode {
    #[diesel(sql_type = Text)]
    journal_mode: String,
}

#[derive(QueryableByName)]
struct Sql {
    #[diesel(sql_type = Nullable<Text>)]
    sql: Option<String>,
}

#[derive(QueryableByName)]
struct History {
    #[diesel(sql_type = Text)]
    hash: String,
    #[diesel(sql_type = Nullable<BigInt>)]
    created_at: Option<i64>,
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

#[derive(QueryableByName)]
struct Identity {
    #[diesel(sql_type = Text)]
    user_id: String,
    #[diesel(sql_type = Text)]
    username: String,
}

fn integrity_ok(conn: &mut Connection) -> Result<bool, SaveError> {
    let rows = diesel::sql_query("PRAGMA integrity_check").load::<Integrity>(conn)?;
    Ok(rows.len() == 1 && rows[0].integrity_check == "ok")
}

fn journal_mode(conn: &mut Connection) -> Result<String, SaveError> {
    let mode = diesel::sql_query("PRAGMA journal_mode").get_result::<JournalMode>(conn)?;
    Ok(mode.journal_mode.to_ascii_lowercase())
}

/// A table's `CREATE` statement with its white space made single spaces, or None.
fn table_sql(conn: &mut Connection, name: &str) -> Result<Option<String>, SaveError> {
    let row = diesel::sql_query("SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?")
        .bind::<Text, _>(name)
        .get_result::<Sql>(conn)
        .optional()?;
    Ok(row
        .and_then(|row| row.sql)
        .map(|sql| sql.split_whitespace().collect::<Vec<_>>().join(" ")))
}

/// The bookkeeping tables as Drizzle and Diesel make them.
fn bookkeeping() -> Result<(String, String), SaveError> {
    let mut conn = memory()?;
    drizzle::replay(&mut conn, &[])?;
    conn.applied_migrations().map_err(|_| SaveError::Invalid)?;
    let drizzle = table_sql(&mut conn, "__drizzle_migrations")?.ok_or(SaveError::Invalid)?;
    let diesel = table_sql(&mut conn, "__diesel_schema_migrations")?.ok_or(SaveError::Invalid)?;
    Ok((drizzle, diesel))
}

/// Each bookkeeping table the save has must be the one its tool makes, before its rows are read.
fn bookkeeping_ok(conn: &mut Connection) -> Result<(), SaveError> {
    let (drizzle, diesel) = bookkeeping()?;
    for (name, expected) in [
        ("__drizzle_migrations", drizzle),
        ("__diesel_schema_migrations", diesel),
    ] {
        if table_sql(conn, name)?.is_some_and(|actual| actual != expected) {
            return Err(SaveError::Invalid);
        }
    }
    Ok(())
}

/// The schema a database at the current migration has.
fn current_schema() -> Result<String, SaveError> {
    let mut conn = memory()?;
    store::migrate(&mut conn, &SCHEMA)?;
    Ok(schema_snapshot(&mut conn)?)
}

/// The one account whose key is `credential`, matching `declared` when the save declares one. A save
/// (declared) must also carry none of the cleared credentials.
fn identity(conn: &mut Connection, credential: &str, declared: Option<&Account>) -> Result<Account, SaveError> {
    let rows = diesel::sql_query(
        "SELECT u.userId AS user_id, u.name AS username FROM userapikeys k JOIN users u ON u.userId = k.userId \
         WHERE k.keyHash = ?",
    )
    .bind::<Text, _>(sha256_hex(credential))
    .load::<Identity>(conn)?;
    let [row] = rows.as_slice() else {
        return Err(SaveError::Invalid);
    };
    let account = Account {
        user_id: row.user_id.clone(),
        username: row.username.clone(),
    };
    if let Some(declared) = declared {
        if *declared != account {
            return Err(SaveError::Invalid);
        }
        for table in CLEARED_TABLES {
            let rows = diesel::sql_query(format!("SELECT count(*) AS n FROM {table}")).get_result::<Count>(conn)?;
            if rows.n != 0 {
                return Err(SaveError::Invalid);
            }
        }
    }
    Ok(account)
}

/// The checks every finished file passes (`validateDatabase`).
fn validate(
    conn: &mut Connection,
    expected: &str,
    credential: &str,
    declared: Option<&Account>,
) -> Result<Account, SaveError> {
    conn.batch_execute("PRAGMA trusted_schema = OFF;")?;
    bookkeeping_ok(conn)?;
    if schema_snapshot(conn)? != expected
        || !integrity_ok(conn)?
        || journal_mode(conn)? != "delete"
        || table_sql(conn, "__diesel_schema_migrations")?.is_none()
    {
        return Err(SaveError::Invalid);
    }
    identity(conn, credential, declared)
}

/// Copies the stopped offline database `source` to `destination` and makes the copy a save: see the
/// module notes. The live database is not changed.
pub fn export(source: &Path, destination: &Path, credential: &str) -> Result<Exported, SaveError> {
    // Journal files beside it mean the stack did not stop cleanly: refuse rather than miss their pages
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = source.as_os_str().to_owned();
        sidecar.push(suffix);
        if Path::new(&sidecar).exists() {
            return Err(SaveError::Invalid);
        }
    }
    if !source.is_file() {
        return Err(SaveError::Io(format!("{} does not exist", source.display())));
    }
    store::backup::backup_file(
        source,
        destination,
        std::time::Duration::from_secs(5),
        crate::dbtools::BACKUP_DEADLINE,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| SaveError::Io(e.to_string()))?;
    }
    let mut copy = open(destination)?;
    copy.batch_execute("PRAGMA journal_mode = DELETE;")?;
    let expected = current_schema()?;
    let account = validate(&mut copy, &expected, credential, None)?;

    let key = format!("UUK_{}", random_hex(24));
    copy.batch_execute("PRAGMA secure_delete = ON;")?;
    copy.immediate_transaction(|conn| {
        let changed = diesel::sql_query("UPDATE userapikeys SET keyHash = ? WHERE userId = ?")
            .bind::<Text, _>(sha256_hex(&key))
            .bind::<Text, _>(&account.user_id)
            .execute(conn)?;
        if changed != 1 {
            return Err(SaveError::Invalid);
        }
        for table in CLEARED_TABLES {
            conn.batch_execute(&format!("DELETE FROM {table};"))?;
        }
        diesel::sql_query(
            "INSERT INTO authepochs (userId, epoch) VALUES (?, 1) ON CONFLICT(userId) DO UPDATE SET epoch = epoch + 1",
        )
        .bind::<Text, _>(&account.user_id)
        .execute(conn)?;
        Ok::<_, SaveError>(())
    })?;
    // Deleted credentials can stay in free pages: rewrite the copy
    copy.batch_execute("VACUUM;")?;
    let account = validate(&mut copy, &expected, &key, Some(&account))?;
    Ok(Exported { account, key })
}

/// Brings the private copy of a save at `database` to the current schema and checks it: see the module
/// notes. `declared` is the account the save's metadata names.
pub fn import(database: &Path, credential: &str, declared: Option<&Account>) -> Result<Account, SaveError> {
    let mut db = open(database)?;
    // Before anything reads its rows: the bookkeeping tables must be what their tools make
    bookkeeping_ok(&mut db)?;
    if !integrity_ok(&mut db)? || journal_mode(&mut db)? != "delete" {
        return Err(SaveError::Invalid);
    }
    if table_sql(&mut db, "__diesel_schema_migrations")?.is_some() {
        check_diesel_prefix(&mut db)?;
    } else if table_sql(&mut db, "__drizzle_migrations")?.is_some() {
        check_drizzle_prefix(&mut db)?;
        // The rest of the frozen Drizzle history, then the baseline's adoption
        drizzle::replay(&mut db, &drizzle_frozen::migrations())?;
    } else {
        return Err(SaveError::Invalid);
    }
    store::migrate(&mut db, &SCHEMA)?;
    validate(&mut db, &current_schema()?, credential, declared)
}

/// A Drizzle history that is a nonempty prefix of the frozen one, with the schema that prefix makes.
fn check_drizzle_prefix(db: &mut Connection) -> Result<(), SaveError> {
    let frozen = drizzle_frozen::migrations();
    let history = diesel::sql_query("SELECT hash, created_at FROM __drizzle_migrations ORDER BY created_at LIMIT ?")
        .bind::<BigInt, _>(frozen.len() as i64 + 1)
        .load::<History>(db)?;
    if history.is_empty() || history.len() > frozen.len() {
        return Err(SaveError::Invalid);
    }
    for (row, migration) in history.iter().zip(&frozen) {
        if row.created_at != Some(migration.created_at) || !drizzle::hash_matches(&row.hash, &migration.sql) {
            return Err(SaveError::Invalid);
        }
    }
    let mut canonical = memory()?;
    drizzle::replay(&mut canonical, &frozen[..history.len()])?;
    if schema_snapshot(db)? != schema_snapshot(&mut canonical)? {
        return Err(SaveError::Invalid);
    }
    Ok(())
}

/// A Diesel history that is a nonempty prefix of this metagame's migrations, with the schema that
/// prefix makes.
fn check_diesel_prefix(db: &mut Connection) -> Result<(), SaveError> {
    let known = MigrationSource::<Sqlite>::migrations(&MIGRATIONS).map_err(|_| SaveError::Invalid)?;
    let applied =
        diesel::sql_query("SELECT version FROM __diesel_schema_migrations ORDER BY version").load::<Version>(db)?;
    if applied.is_empty() || applied.len() > known.len() {
        return Err(SaveError::Invalid);
    }
    for (row, migration) in applied.iter().zip(&known) {
        if row.version != migration.name().version().to_string() {
            return Err(SaveError::Invalid);
        }
    }
    let mut canonical = memory()?;
    // Creates Diesel's bookkeeping table, which run_migration records into
    canonical.applied_migrations().map_err(|_| SaveError::Invalid)?;
    for migration in &known[..applied.len()] {
        canonical
            .run_migration(migration.as_ref())
            .map_err(|_| SaveError::Invalid)?;
    }
    if schema_snapshot(db)? != schema_snapshot(&mut canonical)? {
        return Err(SaveError::Invalid);
    }
    Ok(())
}
