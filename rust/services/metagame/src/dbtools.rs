//! `metagame db …`: the database tools the server kits run (MIGRATION_PLAN.md M19, X1), replacing
//! `deploy/unix-common/dr-db.cjs` and `deploy/windows-server/lib/dr-db.js`. Same commands, checks
//! and messages; the metagame folder argument is gone because nothing has to be loaded from it.
//! The tools work on the file as it is: they never migrate or adopt it. Keys are read from files,
//! never printed, and stored only as SHA-256 hashes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

use crate::crypto::sha256_hex;

/// The kit's limit for a live backup (backup-policy.cjs).
pub const BACKUP_DEADLINE: Duration = Duration::from_secs(15 * 60);
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, clap::Subcommand)]
pub enum DbCommand {
    /// Integrity check; prints "ok, <n> users".
    Integrity {
        db: PathBuf,
    },
    /// Live copy (safe while the metagame runs), then checks the copy.
    Backup {
        db: PathBuf,
        destination: PathBuf,
    },
    /// Adds a one-use invite code.
    AddInvite {
        db: PathBuf,
        code: String,
    },
    DelInvite {
        db: PathBuf,
        code: String,
    },
    MakeAdmin {
        db: PathBuf,
        user_id: String,
    },
    /// Makes sure the game-server key in `key_file` is registered (by its SHA-256).
    GsKey {
        db: PathBuf,
        key_file: PathBuf,
    },
    /// The journal mode: `delete` checkpoints and leaves one file, for a rollback to the Node
    /// metagame and its file-copy backups (M42); `wal` is what the Rust metagame runs in.
    Journal {
        #[arg(value_enum)]
        mode: JournalMode,
        db: PathBuf,
    },
    /// Revokes what free store purchases redeemed at or before `cutoff` granted; a preview unless
    /// `--apply`. Maintenance only: stop the game servers and the metagame, and back up first.
    RevokeFreeStore {
        db: PathBuf,
        cutoff: String,
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum JournalMode {
    Delete,
    Wal,
}

/// Runs a command; `Ok` is the line to print, `Err` the reason it failed.
pub fn run(command: &DbCommand) -> Result<String, String> {
    match command {
        DbCommand::Integrity { db } => integrity(db),
        DbCommand::Backup { db, destination } => {
            store::backup::backup_file(db, destination, BUSY_TIMEOUT, BACKUP_DEADLINE).map_err(|e| e.to_string())?;
            Ok(format!("db {}", integrity(destination)?))
        }
        DbCommand::AddInvite { db, code } | DbCommand::DelInvite { db, code } => {
            if !valid_invite_code(code) {
                return Err("bad invite code".into());
            }
            let mut conn = open(db)?;
            if matches!(command, DbCommand::AddInvite { .. }) {
                diesel::sql_query(
                    "INSERT OR REPLACE INTO invitecodes (invitecode, usesRemaining, infiniteUses) VALUES (?, 1, 0)",
                )
                .bind::<Text, _>(code)
                .execute(&mut conn)
                .map_err(|e| e.to_string())?;
                Ok("invite added".into())
            } else {
                diesel::sql_query("DELETE FROM invitecodes WHERE invitecode = ?")
                    .bind::<Text, _>(code)
                    .execute(&mut conn)
                    .map_err(|e| e.to_string())?;
                Ok("invite removed".into())
            }
        }
        DbCommand::MakeAdmin { db, user_id } => {
            let mut conn = open(db)?;
            let changed = diesel::sql_query("UPDATE users SET isAdmin = 1 WHERE userId = ?")
                .bind::<Text, _>(user_id)
                .execute(&mut conn)
                .map_err(|e| e.to_string())?;
            if changed != 1 {
                return Err("no such user".into());
            }
            Ok("admin flag set".into())
        }
        DbCommand::GsKey { db, key_file } => {
            let key = std::fs::read_to_string(key_file).map_err(|_| "gs-key needs the key file".to_owned())?;
            let key = key.trim();
            if key.len() < 32 {
                return Err("the game-server key file looks empty or truncated".into());
            }
            let hash = sha256_hex(key);
            let mut conn = open(db)?;
            let present = count(
                &mut conn,
                "SELECT count(*) AS n FROM gameserverapikeys WHERE keyHash = ?",
                &hash,
            )? > 0;
            if present {
                return Ok("game-server key already registered".into());
            }
            diesel::sql_query("INSERT INTO gameserverapikeys (keyHash) VALUES (?)")
                .bind::<Text, _>(&hash)
                .execute(&mut conn)
                .map_err(|e| e.to_string())?;
            Ok("game-server key registered".into())
        }
        DbCommand::RevokeFreeStore { db, cutoff, apply } => revoke_free_store(db, cutoff, *apply),
        DbCommand::Journal { mode, db } => {
            let mut conn = open(db)?;
            let sql = match mode {
                JournalMode::Delete => "PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;",
                JournalMode::Wal => "PRAGMA journal_mode = WAL;",
            };
            conn.batch_execute(sql).map_err(|e| e.to_string())?;
            let now: String = journal_mode(&mut conn)?;
            Ok(format!("journal mode {now}"))
        }
    }
}

/// `scripts/revoke-free-store.cjs`: one JSON line per step (preview, applied, verified). The applied
/// run rolls back if any character or escalation data changed with it.
fn revoke_free_store(db: &Path, cutoff: &str, apply: bool) -> Result<String, String> {
    use crate::store_revocation::{RevokeError, progression_hash, revoke_before};
    let line = |mode: &str, report: &crate::store_revocation::Report, extra: Option<(&str, serde_json::Value)>| {
        let mut fields = serde_json::Map::new();
        fields.insert("mode".into(), mode.into());
        fields.insert("cutoff".into(), cutoff.into());
        fields.extend(report.fields());
        if let Some((name, value)) = extra {
            fields.insert(name.into(), value);
        }
        service_kit::jsjson::stringify(&serde_json::Value::Object(fields))
    };
    let mut conn = open(db)?;
    let before = progression_hash(&mut conn).map_err(|e| e.to_string())?;
    let preview = conn
        .transaction(|c| revoke_before(c, cutoff, false))
        .map_err(|e| e.to_string())?;
    let mut lines = vec![line("preview", &preview, None)];
    if apply {
        let fail = |lines: &[String], error: String| format!("{}\n{error}", lines.join("\n"));
        let applied = conn
            .immediate_transaction(|c| {
                let result = revoke_before(c, cutoff, true)?;
                if progression_hash(c)? != before {
                    return Err(RevokeError::Other(
                        "Unrelated character or escalation data changed".into(),
                    ));
                }
                Ok(result)
            })
            .map_err(|e| fail(&lines, e.to_string()))?;
        lines.push(line("applied", &applied, Some(("progressionUnchanged", true.into()))));
        let repeat = conn
            .transaction(|c| revoke_before(c, cutoff, false))
            .map_err(|e| fail(&lines, e.to_string()))?;
        if repeat.purchases != 0 {
            return Err(fail(&lines, "Revocation audit did not cover all purchases".into()));
        }
        let check = diesel::sql_query("PRAGMA quick_check")
            .get_result::<QuickCheck>(&mut conn)
            .map_err(|e| fail(&lines, e.to_string()))?
            .quick_check;
        let mut verified = serde_json::Map::new();
        verified.insert("mode".into(), "verified".into());
        verified.insert("remaining".into(), repeat.purchases.into());
        verified.insert("integrity".into(), check.into());
        lines.push(service_kit::jsjson::stringify(&serde_json::Value::Object(verified)));
    }
    Ok(lines.join("\n"))
}

#[derive(QueryableByName)]
struct QuickCheck {
    #[diesel(sql_type = Text)]
    quick_check: String,
}

fn valid_invite_code(code: &str) -> bool {
    (4..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn open(db: &Path) -> Result<SqliteConnection, String> {
    if !db.is_file() {
        return Err(format!("no database at {}", db.display()));
    }
    let mut conn = SqliteConnection::establish(&db.to_string_lossy()).map_err(|e| e.to_string())?;
    conn.batch_execute(&format!("PRAGMA busy_timeout = {};", BUSY_TIMEOUT.as_millis()))
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Integrity {
    #[diesel(sql_type = Text)]
    integrity_check: String,
}

#[derive(QueryableByName)]
struct Journal {
    #[diesel(sql_type = Text)]
    journal_mode: String,
}

fn count(conn: &mut SqliteConnection, sql: &str, value: &str) -> Result<i64, String> {
    Ok(diesel::sql_query(sql)
        .bind::<Text, _>(value)
        .get_result::<Count>(conn)
        .map_err(|e| e.to_string())?
        .n)
}

fn journal_mode(conn: &mut SqliteConnection) -> Result<String, String> {
    Ok(diesel::sql_query("PRAGMA journal_mode")
        .get_result::<Journal>(conn)
        .map_err(|e| e.to_string())?
        .journal_mode)
}

fn integrity(db: &Path) -> Result<String, String> {
    let mut conn = open(db)?;
    let result = diesel::sql_query("PRAGMA integrity_check")
        .get_result::<Integrity>(&mut conn)
        .map_err(|e| e.to_string())?
        .integrity_check;
    if result != "ok" {
        return Err(format!("integrity_check says: {result}"));
    }
    let users = diesel::sql_query("SELECT count(*) AS n FROM users")
        .get_result::<Count>(&mut conn)
        .map_err(|e| e.to_string())?
        .n;
    Ok(format!("ok, {users} users"))
}
