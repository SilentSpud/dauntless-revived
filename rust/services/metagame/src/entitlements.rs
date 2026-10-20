//! Entitlements of real-mode accounts (`controllers/entitlements.ts`). The client never checks expiry,
//! so an expired timed entitlement is left out of every list; `duration` is in hours, 0 is
//! permanent. The defaults (`ENTITLEMENTS_DEFAULT`) are added once per account; a revoked default
//! stays revoked.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::{Value, json};
use service_kit::collate::locale_compare;

use crate::schema::entitlements;
use crate::time::{iso_now, now_ms};

pub type Conn = store::Connection;

const HOUR_MS: i64 = 60 * 60 * 1000;
const DEFAULT_ENTITLEMENTS: &str = "season09b_premium,season_premium_any,season_free_any";

/// `GetDefaultEntitlements`: `ENTITLEMENTS_DEFAULT` replaces the list (even with an empty one).
pub fn defaults_from(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or(DEFAULT_ENTITLEMENTS)
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = entitlements)]
#[allow(non_snake_case)]
struct Row {
    name: String,
    activatedDate: String,
    duration: i64,
    revokedDate: Option<String>,
}

/// `Date.parse` of a stored ISO time; `None` where JavaScript gets NaN.
fn parse_time(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc).timestamp_millis())
}

fn is_active(row: &Row, now: i64) -> bool {
    if row.revokedDate.is_some() {
        return false;
    }
    if row.duration <= 0 {
        return true;
    }
    parse_time(&row.activatedDate).is_none_or(|activated| activated + row.duration * HOUR_MS > now)
}

fn ensure_defaults(conn: &mut Conn, defaults: &[String], account: &str) -> QueryResult<()> {
    let now = iso_now();
    for name in defaults {
        diesel::insert_into(entitlements::table)
            .values((
                entitlements::accountId.eq(account),
                entitlements::name.eq(name),
                entitlements::activatedDate.eq(&now),
                entitlements::duration.eq(0),
                entitlements::source.eq("default"),
                entitlements::grantedDate.eq(&now),
                entitlements::revokedDate.eq(None::<String>),
            ))
            .on_conflict_do_nothing()
            .execute(conn)?;
    }
    Ok(())
}

/// The active records, by name in `localeCompare` order: `{name, duration, activatedDate}`.
fn active_records(conn: &mut Conn, account: &str) -> QueryResult<Vec<Value>> {
    let now = now_ms();
    let mut rows: Vec<Row> = entitlements::table
        .filter(entitlements::accountId.eq(account))
        .filter(entitlements::revokedDate.is_null())
        .select(Row::as_select())
        .load(conn)?;
    rows.retain(|row| is_active(row, now));
    rows.sort_by(|a, b| locale_compare(&a.name, &b.name));
    Ok(rows
        .into_iter()
        .map(|row| json!({"name": row.name, "duration": row.duration, "activatedDate": row.activatedDate}))
        .collect())
}

/// `ListEntitlements`.
pub fn list(conn: &mut Conn, defaults: &[String], account: &str) -> QueryResult<Vec<Value>> {
    ensure_defaults(conn, defaults, account)?;
    active_records(conn, account)
}

/// `HasActiveEntitlement`.
pub fn has_active(conn: &mut Conn, defaults: &[String], account: &str, name: &str) -> QueryResult<bool> {
    ensure_defaults(conn, defaults, account)?;
    let row: Option<Row> = entitlements::table
        .filter(entitlements::accountId.eq(account))
        .filter(entitlements::name.eq(name))
        .select(Row::as_select())
        .first(conn)
        .optional()?;
    Ok(row.is_some_and(|row| is_active(&row, now_ms())))
}

/// `GrantEntitlementInTx`: a permanent one owned stays; a timed one still running gets the hours
/// added on (activation kept); expired or revoked starts again now. Returns the active list.
pub fn grant(
    conn: &mut Conn,
    defaults: &[String],
    account: &str,
    name: &str,
    duration: i64,
    source: &str,
) -> QueryResult<Vec<Value>> {
    ensure_defaults(conn, defaults, account)?;
    let now = iso_now();
    let row: Option<Row> = entitlements::table
        .filter(entitlements::accountId.eq(account))
        .filter(entitlements::name.eq(name))
        .select(Row::as_select())
        .first(conn)
        .optional()?;
    let target = entitlements::table
        .filter(entitlements::accountId.eq(account))
        .filter(entitlements::name.eq(name));
    match row {
        Some(row) if is_active(&row, now_ms()) => {
            if row.duration == 0 {
                tracing::info!("Entitlement {name} of {account} is already permanent");
            } else if duration == 0 {
                diesel::update(target)
                    .set((
                        entitlements::duration.eq(0),
                        entitlements::source.eq(source),
                        entitlements::grantedDate.eq(&now),
                    ))
                    .execute(conn)?;
                tracing::info!("Entitlement {name} of {account} is now permanent");
            } else {
                diesel::update(target)
                    .set((
                        entitlements::duration.eq(row.duration + duration),
                        entitlements::source.eq(source),
                        entitlements::grantedDate.eq(&now),
                    ))
                    .execute(conn)?;
                tracing::info!(
                    "Entitlement {name} of {account} extended by {duration} h to {} h from {}",
                    row.duration + duration,
                    row.activatedDate
                );
            }
        }
        _ => {
            diesel::insert_into(entitlements::table)
                .values((
                    entitlements::accountId.eq(account),
                    entitlements::name.eq(name),
                    entitlements::activatedDate.eq(&now),
                    entitlements::duration.eq(duration),
                    entitlements::source.eq(source),
                    entitlements::grantedDate.eq(&now),
                    entitlements::revokedDate.eq(None::<String>),
                ))
                .on_conflict((entitlements::accountId, entitlements::name))
                .do_update()
                .set((
                    entitlements::activatedDate.eq(&now),
                    entitlements::duration.eq(duration),
                    entitlements::source.eq(source),
                    entitlements::grantedDate.eq(&now),
                    entitlements::revokedDate.eq(None::<String>),
                ))
                .execute(conn)?;
            let shown = if duration == 0 {
                "permanent".to_owned()
            } else {
                format!("{duration} h")
            };
            tracing::info!("Granted entitlement {name} to {account} ({shown})");
        }
    }
    active_records(conn, account)
}

/// `RevokeEntitlementInTx`: whether something was revoked.
pub fn revoke(conn: &mut Conn, defaults: &[String], account: &str, name: &str) -> QueryResult<bool> {
    ensure_defaults(conn, defaults, account)?;
    let revoked = diesel::update(
        entitlements::table
            .filter(entitlements::accountId.eq(account))
            .filter(entitlements::name.eq(name))
            .filter(entitlements::revokedDate.is_null()),
    )
    .set(entitlements::revokedDate.eq(iso_now()))
    .execute(conn)?;
    if revoked > 0 {
        tracing::info!("Revoked entitlement {name} of {account}");
    } else {
        tracing::info!("Revoke of entitlement {name} of {account}: not owned, nothing to do");
    }
    Ok(revoked > 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn defaults() {
        assert_eq!(
            super::defaults_from(None),
            ["season09b_premium", "season_premium_any", "season_free_any"]
        );
        assert!(super::defaults_from(Some("")).is_empty());
        assert_eq!(super::defaults_from(Some(" a, ,b ")), ["a", "b"]);
    }
}
