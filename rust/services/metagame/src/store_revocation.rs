//! Revoking free store grants (`controllers/storerevocation.ts`, `scripts/revoke-free-store.cjs`), for
//! `metagame db revoke-free-store`. Maintenance only: stop game processes and the metagame first so
//! cached inventories can't restore revoked grants. Preview first, back up, then apply the same
//! cutoff.

use std::collections::HashSet;

use diesel::prelude::*;
use diesel::sql_types::Text;
use serde_json::{Map, Value, json};

use crate::inventory::{self, InventoryCaller, TransactionRequest};
use crate::schema::{entitlements, inventories, inventorylog, storepurchases};
use crate::time::iso_now;

pub type Conn = store::Connection;

/// What a run found (and, applied, revoked).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub purchases: i64,
    pub characters: i64,
    pub stacked_units: i64,
    pub instances: i64,
    pub entitlements: i64,
    pub already_processed: i64,
}

impl Report {
    /// The report's fields as the Node script printed them.
    pub fn fields(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("purchases".into(), json!(self.purchases));
        map.insert("characters".into(), json!(self.characters));
        map.insert("stackedUnits".into(), json!(self.stacked_units));
        map.insert("instances".into(), json!(self.instances));
        map.insert("entitlements".into(), json!(self.entitlements));
        map.insert("alreadyProcessed".into(), json!(self.already_processed));
        map
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RevokeError {
    #[error("Invalid cutoff")]
    InvalidCutoff,
    #[error("Paid purchase encountered; this tool only revokes free grants")]
    Paid,
    #[error("{0}")]
    Inventory(String),
    #[error("{0}")]
    Db(#[from] diesel::result::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Queryable)]
#[allow(non_snake_case)]
struct Purchase {
    tokenHash: String,
    accountId: String,
    characterId: String,
    skuId: String,
}

#[derive(Queryable)]
#[allow(non_snake_case)]
struct Entry {
    operation: String,
    catalogId: Option<String>,
    instanceId: Option<String>,
    quantityChange: Option<i64>,
}

/// The cutoff as an ISO time, as `new Date(cutoff).toISOString()` writes it.
pub fn parse_cutoff(cutoff: &str) -> Result<String, RevokeError> {
    crate::js::date_parse(cutoff)
        .map(crate::time::iso)
        .ok_or(RevokeError::InvalidCutoff)
}

fn json_list(text: Option<&str>) -> Vec<Value> {
    serde_json::from_str::<Value>(text.unwrap_or("[]"))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

/// `RevokeStorePurchasesBefore`, in the caller's transaction: every free purchase redeemed at or
/// before `cutoff`, with what it granted and the character still holds. `apply` revokes them.
pub fn revoke_before(conn: &mut Conn, cutoff: &str, apply: bool) -> Result<Report, RevokeError> {
    let cutoff = parse_cutoff(cutoff)?;
    let mut report = Report::default();
    let mut affected = HashSet::new();
    let purchases: Vec<Purchase> = storepurchases::table
        .filter(storepurchases::redeemedDate.is_not_null())
        .filter(storepurchases::redeemedDate.le(&cutoff))
        .select((
            storepurchases::tokenHash,
            storepurchases::accountId,
            storepurchases::characterId,
            storepurchases::skuId,
        ))
        .load(conn)?;
    for purchase in purchases {
        let transaction = format!("store-revoke:{}", purchase.tokenHash);
        let done = inventorylog::table
            .filter(inventorylog::transactionId.eq(&transaction))
            .filter(inventorylog::operation.eq("store_revoke"))
            .select(inventorylog::id)
            .first::<i64>(conn)
            .optional()?
            .is_some();
        if done {
            report.already_processed += 1;
            continue;
        }
        let entries: Vec<Entry> = inventorylog::table
            .filter(inventorylog::characterId.eq(&purchase.characterId))
            .filter(inventorylog::transactionId.eq(format!("store:{}", purchase.tokenHash)))
            .filter(inventorylog::caller.eq("store"))
            .select((
                inventorylog::operation,
                inventorylog::catalogId,
                inventorylog::instanceId,
                inventorylog::quantityChange,
            ))
            .load(conn)?;
        if entries
            .iter()
            .any(|e| e.operation == "remove" && e.quantityChange.unwrap_or(0) < 0)
        {
            return Err(RevokeError::Paid);
        }
        let grants: Vec<&Entry> = entries.iter().filter(|e| e.operation == "add").collect();
        let row: Option<(String, String)> = inventories::table
            .find(&purchase.characterId)
            .select((inventories::instancedItems, inventories::stackedItems))
            .first(conn)
            .optional()?;
        let stacks = json_list(row.as_ref().map(|r| r.1.as_str()));
        let instances = json_list(row.as_ref().map(|r| r.0.as_str()));
        let stacked: Vec<Value> = grants
            .iter()
            .filter(|g| g.instanceId.as_deref().is_none_or(str::is_empty) && g.quantityChange.unwrap_or(0) > 0)
            .filter_map(|g| {
                let catalog = g.catalogId.as_deref().filter(|c| !c.is_empty())?;
                let held = stacks
                    .iter()
                    .find(|s| s.get("catalogId").and_then(Value::as_str) == Some(catalog))
                    .map_or(0.0, |s| crate::js::number(Some(s.get("quantity").unwrap_or(&json!(0)))));
                let quantity = (g.quantityChange.unwrap_or(0) as f64).min(held);
                (quantity > 0.0).then(|| json!({"catalogId": catalog, "quantity": quantity as i64}))
            })
            .collect();
        let instanced: Vec<Value> = grants
            .iter()
            .filter(|g| g.instanceId.as_deref().is_some_and(|i| !i.is_empty()))
            .filter_map(|g| {
                let item = instances.iter().find(|i| {
                    i.get("instanceId").and_then(Value::as_str) == g.instanceId.as_deref()
                        && i.get("catalogId").and_then(Value::as_str) == g.catalogId.as_deref()
                })?;
                let mut item = item.as_object()?.clone();
                let version = crate::js::number(Some(item.get("updateVersion").unwrap_or(&json!(0))));
                item.insert("updateVersion".into(), json!(version as i64 + 1));
                Some(Value::Object(item))
            })
            .collect();
        let owned: Vec<String> = entitlements::table
            .filter(entitlements::accountId.eq(&purchase.accountId))
            .filter(entitlements::source.eq(format!("store:{}", purchase.skuId)))
            .filter(entitlements::revokedDate.is_null())
            .filter(entitlements::grantedDate.le(&cutoff))
            .select(entitlements::name)
            .load(conn)?;
        report.purchases += 1;
        report.stacked_units += stacked.iter().map(|s| s["quantity"].as_i64().unwrap_or(0)).sum::<i64>();
        report.instances += instanced.len() as i64;
        report.entitlements += owned.len() as i64;
        affected.insert(purchase.characterId.clone());
        if !apply {
            continue;
        }
        if !stacked.is_empty() || !instanced.is_empty() {
            let request = TransactionRequest {
                user_id: Some(purchase.accountId.clone()),
                character_id: Some(json!(purchase.characterId)),
                transaction_id: Some(json!(transaction)),
                add_instanced: None,
                add_stacked: None,
                remove_instanced: Some(Value::Array(instanced)),
                remove_stacked: Some(Value::Array(stacked)),
                save_instanced: None,
            };
            inventory::apply_in_tx(
                conn,
                request,
                InventoryCaller::Admin,
                Some(&json!("free-store-revocation")),
                &inventory::Settings::default(),
            )
            .map_err(|fail| RevokeError::Inventory(fail.message().to_owned()))?;
        }
        for name in &owned {
            diesel::update(
                entitlements::table
                    .filter(entitlements::accountId.eq(&purchase.accountId))
                    .filter(entitlements::name.eq(name)),
            )
            .set(entitlements::revokedDate.eq(iso_now()))
            .execute(conn)?;
        }
        // An append-only marker that outlives the transaction receipts' retention.
        diesel::insert_into(inventorylog::table)
            .values((
                inventorylog::time.eq(iso_now()),
                inventorylog::userId.eq(&purchase.accountId),
                inventorylog::characterId.eq(&purchase.characterId),
                inventorylog::transactionId.eq(&transaction),
                inventorylog::caller.eq("admin"),
                inventorylog::source.eq("free-store-revocation"),
                inventorylog::operation.eq("store_revoke"),
            ))
            .execute(conn)?;
    }
    if apply {
        diesel::update(
            storepurchases::table
                .filter(storepurchases::redeemedDate.is_null())
                .filter(storepurchases::createdDate.le(&cutoff)),
        )
        .set(storepurchases::expiresDate.eq(&cutoff))
        .execute(conn)?;
    }
    report.characters = affected.len() as i64;
    Ok(report)
}

#[derive(QueryableByName)]
struct Column {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct Rows {
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    rows: Option<String>,
}

/// A digest of the character and escalation tables, which revoking must never change.
pub fn progression_hash(conn: &mut Conn) -> QueryResult<String> {
    let mut all = String::new();
    for table in [
        "characters",
        "characterhistory",
        "escalationprogression",
        "escalationtalents",
        "escalationunlocks",
    ] {
        let columns: Vec<Column> = diesel::sql_query("SELECT name FROM pragma_table_info(?)")
            .bind::<Text, _>(table)
            .load(conn)?;
        let row = columns
            .iter()
            .map(|c| format!("quote(\"{}\")", c.name.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let rows: Rows = diesel::sql_query(format!(
            "SELECT group_concat(r, char(10)) AS rows FROM (SELECT {row} AS r FROM \"{table}\" ORDER BY rowid)"
        ))
        .get_result(conn)?;
        all.push_str(table);
        all.push('\n');
        all.push_str(&rows.rows.unwrap_or_default());
        all.push('\n');
    }
    Ok(crate::crypto::sha256_hex(&all))
}
