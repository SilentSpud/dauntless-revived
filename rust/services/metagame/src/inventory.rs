//! Inventories (`controllers/inventory.ts`): a character's instanced items (each its own JSON object
//! with an `updateVersion`) and stacked items (`{catalogId, quantity}`), both stored as JSON text.
//!
//! The rules are Node's, on the untyped bodies the game sends: raw deltas only from a game server or
//! an internal controller; every list must be a list of objects; an existing instance keeps its
//! catalog id and only moves to a newer version (stateless items and Middleman tokens have their own
//! rules); removals may spend what the same transaction adds; overspends are clamped unless
//! `INVENTORY_REFUSE_OVERSPEND=1` (Rams never); a transaction id makes a retry answer the stored reply.
//! The whole transaction is one SQLite transaction, so a refusal changes nothing.
//!
//! Node mutates the request's item objects and shares them with the stored stack list (an added
//! stack *is* the request's object, and later adds in the same request add to it), and the reply
//! serializes those shared objects. Stacks therefore live in an arena here and lists hold indexes
//! into it, which gives the same sharing.

use diesel::prelude::*;
use serde_json::{Map, Value, json};
use service_kit::jsjson;
use sha2::{Digest, Sha256};

use crate::js;
use crate::middleman::{self, FusionError};
use crate::schema::{characters, inventories, inventorylog, inventorytransactions, onetimejobs};
use crate::time::iso_now;

pub type Conn = store::Connection;

const TRANSACTION_RECORD_DAYS: i64 = 30;
const DAILY_TOKENS_JOB: &str = "daily_tokens_10_6";

/// The inventory switches, read at start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// `INVENTORY_REFUSE_OVERSPEND=1`.
    pub refuse_overspend: bool,
    /// `INVENTORY_REPORT_REMOVALS` (on unless `0`): every touched stack is reported with its count.
    pub report_removals: bool,
    /// `PATROL_BONUS_DAILY` (10) and `BOUNTY_TOKENS_DAILY` (6).
    pub patrol_bonus_daily: i64,
    pub bounty_tokens_daily: i64,
    /// `MIDDLEMAN_FUSION_GUARDS` (off).
    pub fusion_guards: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refuse_overspend: false,
            report_removals: true,
            patrol_bonus_daily: 10,
            bounty_tokens_daily: 6,
            fusion_guards: false,
        }
    }
}

impl Settings {
    pub fn from_env(env: &service_kit::env::Env, fusion_guards: bool) -> Self {
        let daily = |name: &str, default: i64| {
            let n = js::number(env.raw(name).map(|v| Value::String(v.to_owned())).as_ref());
            if n.fract() == 0.0 && n >= 1.0 && n.is_finite() {
                n as i64
            } else {
                default
            }
        };
        Self {
            refuse_overspend: env.raw("INVENTORY_REFUSE_OVERSPEND") == Some("1"),
            report_removals: env.raw("INVENTORY_REPORT_REMOVALS") != Some("0"),
            patrol_bonus_daily: daily("PATROL_BONUS_DAILY", 10),
            bounty_tokens_daily: daily("BOUNTY_TOKENS_DAILY", 6),
            fusion_guards,
        }
    }
}

/// `InventoryError` and its HTTP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryError {
    Forbidden,
    NotFound,
    Conflict,
    InsufficientQuantity,
    InvalidItem,
    InvalidData,
    DbError,
}

impl InventoryError {
    pub fn status(self) -> u16 {
        match self {
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::Conflict | Self::InsufficientQuantity => 409,
            Self::InvalidItem => 400,
            Self::InvalidData | Self::DbError => 500,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::InsufficientQuantity => "insufficient_quantity",
            Self::InvalidItem => "invalid_inventory_item",
            Self::InvalidData => "invalid_inventory_data",
            Self::DbError => "db_error",
        }
    }
}

/// A refusal with the message Node logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    Forbidden(String),
    Validation(String),
    Conflict(String),
    Insufficient(String),
    /// The stored inventory is not JSON (Node's SyntaxError).
    Syntax(String),
    /// Anything else Node would hit as a TypeError: a 500.
    Broken(String),
}

impl From<FusionError> for Fail {
    fn from(error: FusionError) -> Self {
        match error {
            FusionError::Validation(message) => Fail::Validation(message),
            FusionError::Conflict(message) => Fail::Conflict(message),
        }
    }
}

// A database failure inside a transaction is Node's db_error, and rolls the transaction back.
impl From<diesel::result::Error> for Fail {
    fn from(error: diesel::result::Error) -> Self {
        Fail::Broken(error.to_string())
    }
}

impl From<store::Error> for Fail {
    fn from(error: store::Error) -> Self {
        Fail::Broken(error.to_string())
    }
}

impl Fail {
    /// `InventoryErrorOf`.
    pub fn error(&self) -> InventoryError {
        match self {
            Fail::Forbidden(_) => InventoryError::Forbidden,
            Fail::Validation(_) => InventoryError::InvalidItem,
            Fail::Conflict(_) => InventoryError::Conflict,
            Fail::Insufficient(_) => InventoryError::InsufficientQuantity,
            Fail::Syntax(_) => InventoryError::InvalidData,
            Fail::Broken(_) => InventoryError::DbError,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Fail::Forbidden(m)
            | Fail::Validation(m)
            | Fail::Conflict(m)
            | Fail::Insufficient(m)
            | Fail::Syntax(m)
            | Fail::Broken(m) => m,
        }
    }
}

/// Who authorizes a delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryCaller {
    Client,
    GameServer,
    Admin,
    Store,
    Trials,
}

impl InventoryCaller {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::GameServer => "gameserver",
            Self::Admin => "admin",
            Self::Store => "store",
            Self::Trials => "trials",
        }
    }
}

/// One POST /inventory body, the lists exactly as they arrived (`None` when missing).
#[derive(Debug, Clone)]
pub struct TransactionRequest {
    pub user_id: Option<String>,
    pub character_id: Option<Value>,
    pub transaction_id: Option<Value>,
    pub add_instanced: Option<Value>,
    pub add_stacked: Option<Value>,
    pub remove_instanced: Option<Value>,
    pub remove_stacked: Option<Value>,
    pub save_instanced: Option<Value>,
}

/// What a transaction did: the reply, whether it was a stored reply, and the overspends it clamped.
#[derive(Debug, Clone)]
pub struct Applied {
    pub response: Value,
    pub replayed: bool,
    pub overspent: Vec<String>,
}

fn js_string_of(value: Option<&Value>) -> String {
    js::string(value)
}

/// Node's `!Object.prototype.hasOwnProperty.call(Item, "itemData") || Item.itemData == null`.
fn is_stateless(item: &Value) -> bool {
    js::is_nullish(item.get("itemData"))
}

fn assert_valid_incoming(item: &Value, operation: &str) -> Result<(), Fail> {
    if is_stateless(item) && !js::strict_eq(item.get("updateVersion"), Some(&json!(0))) {
        return Err(Fail::Validation(format!(
            "Refusing stateless instanced item {operation} {}/{}: expected updateVersion 0, got {}",
            js_string_of(item.get("catalogId")),
            js_string_of(item.get("instanceId")),
            js_string_of(item.get("updateVersion"))
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteKind {
    Write,
    Skip,
}

/// `AssertExistingInstancedItemWrite`.
fn assert_existing_write(current: &Value, incoming: &Value, operation: &str) -> Result<WriteKind, Fail> {
    if !js::strict_eq(current.get("catalogId"), incoming.get("catalogId")) {
        return Err(Fail::Validation(
            "An existing instance cannot change catalog identity".into(),
        ));
    }
    if operation != "remove" {
        assert_valid_incoming(incoming, operation)?;
    }
    let Some(current_version) = current
        .get("updateVersion")
        .filter(|v| v.is_number())
        .and_then(Value::as_f64)
    else {
        return Ok(WriteKind::Write);
    };
    if operation != "remove" && is_stateless(current) && is_stateless(incoming) && current_version == 0.0 {
        return Ok(WriteKind::Skip);
    }
    if middleman::is_valid_save(current, incoming, operation)?
        || middleman::is_valid_removal(current, incoming, operation)
    {
        return Ok(WriteKind::Write);
    }
    let stale = match incoming.get("updateVersion") {
        Some(Value::Number(n)) => n.as_f64().is_none_or(|v| v <= current_version),
        _ => true,
    };
    if stale {
        return Err(Fail::Conflict(format!(
            "Refusing stale instanced item {operation} {}/{}: current updateVersion {}, incoming updateVersion {}",
            js_string_of(incoming.get("catalogId")),
            js_string_of(incoming.get("instanceId")),
            js_string_of(current.get("updateVersion")),
            js_string_of(incoming.get("updateVersion"))
        )));
    }
    Ok(WriteKind::Write)
}

/// `FindInstancedItemIndex`: the same instance id (two missing ids match, as in JavaScript), else a
/// Middleman token of the same slot.
fn find_instanced(items: &[Value], incoming: &Value) -> Option<usize> {
    items
        .iter()
        .position(|item| js::strict_eq(item.get("instanceId"), incoming.get("instanceId")))
        .or_else(|| middleman::find_index(items, incoming))
}

/// `AssertItemList`: a list whose items are all objects (arrays count, as `typeof` says).
fn assert_item_list(items: &Value, name: &str) -> Result<(), Fail> {
    match items {
        Value::Array(list) if list.iter().all(|item| item.is_object() || item.is_array()) => Ok(()),
        _ => Err(Fail::Validation(format!(
            "Refusing transaction: {name} is not a list of items"
        ))),
    }
}

/// `ParseStackQuantity`: a whole number of at least 0 (a string of digits too) and a catalog id.
fn parse_quantity(item: &Value, operation: &str) -> Result<f64, Fail> {
    let raw = item.get("quantity");
    let quantity = match raw {
        Some(Value::String(text)) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse::<f64>().ok()
        }
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    };
    let Some(quantity) = quantity.filter(|q| q.fract() == 0.0 && *q >= 0.0 && *q <= 9_007_199_254_740_991.0) else {
        return Err(Fail::Validation(format!(
            "Refusing stacked item {operation} {}: quantity {} is not a whole number of at least 0",
            js_string_of(item.get("catalogId")),
            raw.map_or("undefined".to_owned(), jsjson::stringify)
        )));
    };
    match item.get("catalogId") {
        Some(Value::String(id)) if !id.is_empty() => Ok(quantity),
        _ => Err(Fail::Validation(format!(
            "Refusing stacked item {operation}: missing catalogId"
        ))),
    }
}

/// One inventorylog row.
#[derive(Debug, Clone, Default)]
struct LogEntry {
    operation: &'static str,
    catalog_id: Option<String>,
    instance_id: Option<String>,
    update_version: Option<i64>,
    quantity_change: Option<f64>,
    quantity_after: Option<f64>,
}

fn log_instanced(operation: &'static str, item: &Value) -> LogEntry {
    LogEntry {
        operation,
        catalog_id: item.get("catalogId").and_then(Value::as_str).map(str::to_owned),
        instance_id: item.get("instanceId").and_then(Value::as_str).map(str::to_owned),
        update_version: js::safe_integer(item.get("updateVersion")),
        quantity_change: Some(match operation {
            "remove" => -1.0,
            "update" => 0.0,
            _ => 1.0,
        }),
        quantity_after: None,
    }
}

/// Stack objects with JavaScript's sharing: lists hold indexes into the arena.
struct Arena {
    items: Vec<Value>,
}

impl Arena {
    fn add(&mut self, item: Value) -> usize {
        self.items.push(item);
        self.items.len() - 1
    }

    fn catalog(&self, id: usize) -> Option<&Value> {
        self.items[id].get("catalogId")
    }

    fn quantity(&self, id: usize) -> f64 {
        js::number(self.items[id].get("quantity"))
    }

    fn quantity_or_zero(&self, id: usize) -> f64 {
        match self.items[id].get("quantity") {
            None | Some(Value::Null) => 0.0,
            other => js::number(other),
        }
    }

    fn set_quantity(&mut self, id: usize, quantity: f64) {
        if let Value::Object(map) = &mut self.items[id] {
            map.insert("quantity".into(), number_value(quantity));
        }
    }
}

fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        json!(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

fn find_stack(arena: &Arena, stacked: &[usize], catalog: &Value) -> Option<usize> {
    stacked
        .iter()
        .position(|id| js::strict_eq(arena.catalog(*id), Some(catalog)))
}

/// An ordered map with insertion order, as JavaScript's Map.
fn bump(map: &mut Vec<(String, f64)>, key: &str, by: f64) {
    match map.iter_mut().find(|(k, _)| k == key) {
        Some((_, value)) => *value += by,
        None => map.push((key.to_owned(), by)),
    }
}

struct StackResult {
    touched: Vec<Touched>,
    log: Vec<LogEntry>,
    overspent: Vec<String>,
}

enum Touched {
    Shared(usize),
    New(Value),
}

/// `ApplyStackedChanges`: removals then additions, a removal may spend what the same transaction adds.
fn apply_stacked(
    arena: &mut Arena,
    stacked: &mut Vec<usize>,
    removes: &[Value],
    adds: &[usize],
    settings: &Settings,
) -> Result<StackResult, Fail> {
    let mut touched = Vec::new();
    let mut log = Vec::new();
    let mut overspent = Vec::new();
    let remove_quantities: Vec<f64> = removes
        .iter()
        .map(|item| parse_quantity(item, "remove"))
        .collect::<Result<_, _>>()?;
    let add_quantities: Vec<f64> = adds
        .iter()
        .map(|id| parse_quantity(&arena.items[*id], "add"))
        .collect::<Result<_, _>>()?;
    let catalog_of = |item: &Value| item.get("catalogId").and_then(Value::as_str).unwrap_or("").to_owned();

    let mut wanted: Vec<(String, f64)> = Vec::new();
    for (item, quantity) in removes.iter().zip(&remove_quantities) {
        bump(&mut wanted, &catalog_of(item), *quantity);
    }
    let mut added: Vec<(String, f64)> = Vec::new();
    for (id, quantity) in adds.iter().zip(&add_quantities) {
        bump(&mut added, &catalog_of(&arena.items[*id]), *quantity);
    }
    let added_of = |catalog: &str| added.iter().find(|(k, _)| k == catalog).map(|(_, v)| *v);

    for (catalog, quantity) in &wanted {
        let held = find_stack(arena, stacked, &json!(catalog)).map_or(0.0, |i| arena.quantity_or_zero(stacked[i]));
        let added_here = added_of(catalog).unwrap_or(0.0);
        if *quantity > held + added_here {
            let also = if added_here > 0.0 {
                format!(" and {} added", jsjson::number(added_here))
            } else {
                String::new()
            };
            let message = format!(
                "removes {} of {catalog} but only {} held{also}",
                jsjson::number(*quantity),
                jsjson::number(held)
            );
            // Rams purchases must never succeed against a stale balance.
            if settings.refuse_overspend || catalog == "CURRENCY_NOTES" {
                return Err(Fail::Insufficient(message));
            }
            overspent.push(message);
        }
    }

    let mut deferred: Vec<(String, f64)> = Vec::new();
    for (item, quantity) in removes.iter().zip(&remove_quantities) {
        let catalog = item.get("catalogId").cloned().unwrap_or(Value::Null);
        let index = find_stack(arena, stacked, &catalog);
        let covered = index.map_or(0.0, |i| quantity.min(arena.quantity(stacked[i])).max(0.0));
        let short = quantity - covered;
        let name = catalog_of(item);
        if short > 0.0 && added_of(&name).is_some() {
            bump(&mut deferred, &name, short);
        }
        let Some(index) = index else { continue };
        let id = stacked[index];
        let before = arena.quantity(id);
        let after = before - quantity;
        arena.set_quantity(id, after);
        if after <= 0.0 {
            stacked.remove(index);
        }
        log.push(LogEntry {
            operation: "remove",
            catalog_id: Some(name),
            quantity_change: Some(-quantity.min(before)),
            quantity_after: Some(after.max(0.0)),
            ..LogEntry::default()
        });
    }

    for (id, quantity) in adds.iter().zip(&add_quantities) {
        let catalog = arena.items[*id].get("catalogId").cloned().unwrap_or(Value::Null);
        let name = catalog_of(&arena.items[*id]);
        match find_stack(arena, stacked, &catalog) {
            Some(index) => {
                let stack = stacked[index];
                let total = arena.quantity(stack) + quantity;
                arena.set_quantity(stack, total);
                touched.push(Touched::Shared(stack));
                log.push(LogEntry {
                    operation: "add",
                    catalog_id: Some(name),
                    quantity_change: Some(*quantity),
                    quantity_after: Some(total),
                    ..LogEntry::default()
                });
            }
            None => {
                arena.set_quantity(*id, *quantity);
                touched.push(Touched::Shared(*id));
                stacked.push(*id);
                log.push(LogEntry {
                    operation: "add",
                    catalog_id: Some(name),
                    quantity_change: Some(*quantity),
                    quantity_after: Some(*quantity),
                    ..LogEntry::default()
                });
            }
        }
    }

    for (catalog, short) in &deferred {
        let Some(index) = find_stack(arena, stacked, &json!(catalog)) else {
            continue;
        };
        let id = stacked[index];
        let taken = short.min(arena.quantity(id));
        let after = arena.quantity(id) - taken;
        arena.set_quantity(id, after);
        if after <= 0.0 {
            stacked.remove(index);
        }
        log.push(LogEntry {
            operation: "remove",
            catalog_id: Some(catalog.clone()),
            quantity_change: Some(-taken),
            quantity_after: Some(after.max(0.0)),
            ..LogEntry::default()
        });
    }

    if settings.report_removals {
        let mut keys: Vec<String> = added.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in &wanted {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        touched = keys
            .into_iter()
            .map(|catalog| match find_stack(arena, stacked, &json!(catalog)) {
                Some(index) => Touched::Shared(stacked[index]),
                None => Touched::New(json!({"catalogId": catalog, "quantity": 0})),
            })
            .collect();
    }
    Ok(StackResult {
        touched,
        log,
        overspent,
    })
}

/// `GetTransactionDedupeKey`: a non-empty id that isn't all zeros and dashes.
fn dedupe_key(transaction_id: Option<&Value>) -> Option<String> {
    match transaction_id {
        Some(Value::String(id)) if !id.is_empty() && !id.bytes().all(|b| b == b'0' || b == b'-') => Some(id.clone()),
        _ => None,
    }
}

/// `HashTransactionRequest`.
fn request_hash(request: &TransactionRequest) -> String {
    let or_null = |value: &Option<Value>| value.clone().unwrap_or(Value::Null);
    let list = json!([
        or_null(&request.character_id),
        or_null(&request.add_instanced),
        or_null(&request.add_stacked),
        or_null(&request.remove_instanced),
        or_null(&request.remove_stacked),
        or_null(&request.save_instanced),
    ]);
    Sha256::digest(jsjson::stringify(&list).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `MakeTransactionResponse`: two of the request's lists echoed (missing ones left out).
fn response(request: &TransactionRequest, stacks: Vec<Value>) -> Value {
    let mut reply = Map::new();
    if let Some(created) = &request.add_instanced {
        reply.insert("createdInstancedItems".into(), created.clone());
    }
    reply.insert("updatedInstancedItems".into(), json!([]));
    reply.insert("updatedStackedItems".into(), Value::Array(stacks));
    if let Some(removed) = &request.remove_instanced {
        reply.insert("removedInstancedItems".into(), removed.clone());
    }
    Value::Object(reply)
}

/// The lists after `PrepareInventoryTransaction`: checked, fusion tokens normalised, hash taken.
pub struct Prepared {
    pub request: TransactionRequest,
    add_instanced: Vec<Value>,
    add_stacked: Vec<Value>,
    remove_instanced: Vec<Value>,
    remove_stacked: Vec<Value>,
    save_instanced: Vec<Value>,
    pub touches_anything: bool,
    pub dedupe: Option<String>,
    hash: String,
}

fn list_of(value: &Option<Value>) -> Value {
    match value {
        None | Some(Value::Null) => json!([]),
        Some(other) => other.clone(),
    }
}

/// `PrepareInventoryTransaction`: the checks that need no database.
pub fn prepare(mut request: TransactionRequest) -> Result<Prepared, Fail> {
    let names = [
        "addInstancedItems",
        "addStackedItems",
        "removeInstancedItems",
        "removeStackedItems",
        "saveInstancedItems",
    ];
    let lists = [
        &request.add_instanced,
        &request.add_stacked,
        &request.remove_instanced,
        &request.remove_stacked,
        &request.save_instanced,
    ];
    for (list, name) in lists.iter().zip(names) {
        assert_item_list(&list_of(list), name)?;
    }
    // Pending Middleman exchanges get a slot-stable identity before the hash, in the request itself.
    for list in [&mut request.add_instanced, &mut request.save_instanced] {
        if let Some(Value::Array(items)) = list {
            for item in items {
                middleman::normalise(item, false)?;
            }
        }
    }
    if let Some(Value::Array(items)) = &mut request.remove_instanced {
        for item in items {
            middleman::normalise(item, true)?;
        }
    }
    let array = |value: &Option<Value>| list_of(value).as_array().cloned().unwrap_or_default();
    let prepared = Prepared {
        add_instanced: array(&request.add_instanced),
        add_stacked: array(&request.add_stacked),
        remove_instanced: array(&request.remove_instanced),
        remove_stacked: array(&request.remove_stacked),
        save_instanced: array(&request.save_instanced),
        touches_anything: false,
        dedupe: dedupe_key(request.transaction_id.as_ref()),
        hash: request_hash(&request),
        request,
    };
    let touches = !prepared.add_instanced.is_empty()
        || !prepared.remove_instanced.is_empty()
        || !prepared.save_instanced.is_empty()
        || !prepared.add_stacked.is_empty()
        || !prepared.remove_stacked.is_empty();
    Ok(Prepared {
        touches_anything: touches,
        ..prepared
    })
}

/// The empty reply of a transaction that changes nothing.
pub fn empty_response(prepared: &Prepared) -> Value {
    response(&prepared.request, vec![])
}

fn read_or_create(conn: &mut Conn, character_id: &str, user_id: Option<&str>) -> QueryResult<(String, String)> {
    let row: Option<(String, String)> = inventories::table
        .find(character_id)
        .select((inventories::instancedItems, inventories::stackedItems))
        .first(conn)
        .optional()?;
    match row {
        Some(row) => Ok(row),
        None => {
            match user_id {
                Some(user_id) => {
                    tracing::info!("Creating inventory for characterId {character_id} and userId {user_id}")
                }
                None => tracing::info!("Creating inventory for characterId {character_id}"),
            }
            diesel::insert_into(inventories::table)
                .values((
                    inventories::characterId.eq(character_id),
                    inventories::instancedItems.eq("[]"),
                    inventories::stackedItems.eq("[]"),
                ))
                .execute(conn)?;
            Ok(("[]".into(), "[]".into()))
        }
    }
}

fn parse_stored(text: &str) -> Result<Value, Fail> {
    jsjson::parse(text).map_err(|e| Fail::Syntax(e.to_string()))
}

fn write_log(
    conn: &mut Conn,
    user_id: &str,
    character_id: &str,
    transaction: Option<&str>,
    caller: InventoryCaller,
    source: Option<&Value>,
    entries: &[LogEntry],
) -> QueryResult<()> {
    let time = iso_now();
    let source = source.and_then(Value::as_str);
    for entry in entries {
        diesel::insert_into(inventorylog::table)
            .values((
                inventorylog::time.eq(&time),
                inventorylog::userId.eq(user_id),
                inventorylog::characterId.eq(character_id),
                inventorylog::transactionId.eq(transaction),
                inventorylog::source.eq(source),
                inventorylog::caller.eq(caller.as_str()),
                inventorylog::operation.eq(entry.operation),
                inventorylog::catalogId.eq(entry.catalog_id.as_deref()),
                inventorylog::instanceId.eq(entry.instance_id.as_deref()),
                inventorylog::quantityChange.eq(entry.quantity_change.map(|n| n as i64)),
                inventorylog::quantityAfter.eq(entry.quantity_after.map(|n| n as i64)),
                inventorylog::updateVersion.eq(entry.update_version),
            ))
            .execute(conn)?;
    }
    Ok(())
}

/// `AssertCharacterOwnedInTx`.
pub fn assert_owned(conn: &mut Conn, user_id: Option<&str>, character_id: Option<&Value>) -> Result<(), Fail> {
    let owned = match (user_id, character_id.and_then(Value::as_str)) {
        (Some(user), Some(character)) => characters::table
            .filter(characters::characterId.eq(character))
            .filter(characters::userId.eq(user))
            .select(characters::characterId)
            .first::<String>(conn)
            .optional()
            .map_err(|e| Fail::Broken(e.to_string()))?
            .is_some(),
        _ => false,
    };
    if owned {
        Ok(())
    } else {
        Err(Fail::Forbidden(format!(
            "Specified characterId {} does not belong to user {}",
            js_string_of(character_id),
            user_id.unwrap_or("undefined")
        )))
    }
}

fn top_up_daily_tokens(
    arena: &mut Arena,
    stacked: &[usize],
    adds: &[usize],
    source: Option<&Value>,
    settings: &Settings,
) {
    let source_text = source.and_then(Value::as_str);
    let tokens = [
        (
            "TOKEN_DAILY_PATROL_BONUS",
            settings.patrol_bonus_daily,
            source_text == Some("PERIODIC_REWARD"),
        ),
        (
            "TOKEN_BOUNTY_DRAFT",
            settings.bounty_tokens_daily,
            source_text.is_some_and(|s| s.starts_with("UBountyComponent::")),
        ),
    ];
    for (catalog, amount, granted) in tokens {
        if !granted {
            continue;
        }
        let held = find_stack(arena, stacked, &json!(catalog)).map_or(0.0, |i| arena.quantity_or_zero(stacked[i]));
        for id in adds {
            if arena.items[*id].get("catalogId").and_then(Value::as_str) == Some(catalog) {
                arena.set_quantity(*id, (amount as f64 - held).max(0.0));
            }
        }
    }
}

/// `ApplyPreparedInventoryTransaction`, inside the caller's transaction. An `Err` must roll it back.
pub fn apply(
    conn: &mut Conn,
    prepared: &Prepared,
    caller: InventoryCaller,
    source: Option<&Value>,
    settings: &Settings,
) -> Result<Applied, Fail> {
    let db = |e: diesel::result::Error| Fail::Broken(e.to_string());
    if caller == InventoryCaller::Client {
        tracing::warn!(
            event = "inventory_authority_denied",
            userId = prepared.request.user_id.as_deref().unwrap_or(""),
            operation = "transaction"
        );
        return Err(Fail::Forbidden("Player cannot authorize raw inventory deltas".into()));
    }
    let user_id = prepared.request.user_id.clone().unwrap_or_default();
    let character_id = prepared
        .request
        .character_id
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    if let Some(key) = &prepared.dedupe {
        let stored: Option<String> = inventorytransactions::table
            .filter(inventorytransactions::characterId.eq(&character_id))
            .filter(inventorytransactions::transactionId.eq(key))
            .filter(inventorytransactions::requestHash.eq(&prepared.hash))
            .select(inventorytransactions::response)
            .first(conn)
            .optional()
            .map_err(db)?;
        if let Some(stored) = stored {
            return Ok(Applied {
                response: parse_stored(&stored)?,
                replayed: true,
                overspent: vec![],
            });
        }
        let seen: Option<i64> = inventorytransactions::table
            .filter(inventorytransactions::characterId.eq(&character_id))
            .filter(inventorytransactions::transactionId.eq(key))
            .select(inventorytransactions::id)
            .first(conn)
            .optional()
            .map_err(db)?;
        if seen.is_some() {
            tracing::warn!(
                "transactionId {key} for userId {user_id} and characterId {character_id} was seen before with a different body; running it as a new transaction"
            );
        }
    }

    let (stored_instanced, stored_stacked) = read_or_create(conn, &character_id, None).map_err(db)?;
    let mut new_instanced: Option<String> = None;
    let mut new_stacked: Option<String> = None;
    let mut log = Vec::new();
    let mut touched_values = Vec::new();
    let mut overspent = Vec::new();

    let touches_instanced = !prepared.add_instanced.is_empty()
        || !prepared.remove_instanced.is_empty()
        || !prepared.save_instanced.is_empty();
    if touches_instanced {
        let Value::Array(mut items) = parse_stored(&stored_instanced)? else {
            return Err(Fail::Broken("the stored instanced items are not a list".into()));
        };
        let mut changed = false;
        if settings.fusion_guards {
            middleman::validate_completions(
                &items,
                &prepared.remove_instanced,
                &prepared.add_stacked,
                crate::time::now_ms(),
            )?;
        }
        for incoming in &prepared.remove_instanced {
            match find_instanced(&items, incoming) {
                Some(index) => {
                    assert_existing_write(&items[index], incoming, "remove")?;
                    log.push(log_instanced("remove", &items[index]));
                    items.remove(index);
                    changed = true;
                }
                None => tracing::warn!(
                    "transactionId {} removes instanced item {}/{}, which characterId {character_id} does not hold; ignored",
                    js_string_of(prepared.request.transaction_id.as_ref()),
                    js_string_of(incoming.get("catalogId")),
                    js_string_of(incoming.get("instanceId"))
                ),
            }
        }
        for (operation, list) in [("save", &prepared.save_instanced), ("add", &prepared.add_instanced)] {
            for incoming in list {
                match find_instanced(&items, incoming) {
                    Some(index) => {
                        if assert_existing_write(&items[index], incoming, operation)? == WriteKind::Skip {
                            continue;
                        }
                        items[index] = incoming.clone();
                    }
                    None => {
                        assert_valid_incoming(incoming, operation)?;
                        items.push(incoming.clone());
                    }
                }
                changed = true;
                log.push(log_instanced(
                    if operation == "save" { "save" } else { "add" },
                    incoming,
                ));
            }
        }
        if changed {
            new_instanced = Some(jsjson::stringify(&Value::Array(items)));
        }
    }

    if !prepared.add_stacked.is_empty() || !prepared.remove_stacked.is_empty() {
        let Value::Array(items) = parse_stored(&stored_stacked)? else {
            return Err(Fail::Broken("the stored stacked items are not a list".into()));
        };
        let mut arena = Arena { items: Vec::new() };
        let mut stacked: Vec<usize> = items.into_iter().map(|item| arena.add(item)).collect();
        let adds: Vec<usize> = prepared
            .add_stacked
            .iter()
            .map(|item| arena.add(item.clone()))
            .collect();
        if caller == InventoryCaller::GameServer {
            top_up_daily_tokens(&mut arena, &stacked, &adds, source, settings);
        }
        let result = apply_stacked(&mut arena, &mut stacked, &prepared.remove_stacked, &adds, settings)?;
        touched_values = result
            .touched
            .into_iter()
            .map(|touched| match touched {
                Touched::Shared(id) => arena.items[id].clone(),
                Touched::New(value) => value,
            })
            .collect();
        overspent = result.overspent;
        log.extend(result.log);
        new_stacked = Some(jsjson::stringify(&Value::Array(
            stacked.iter().map(|id| arena.items[*id].clone()).collect(),
        )));
    }

    if new_instanced.is_some() || new_stacked.is_some() {
        let target = inventories::table.find(&character_id);
        match (&new_instanced, &new_stacked) {
            (Some(i), Some(s)) => diesel::update(target)
                .set((inventories::instancedItems.eq(i), inventories::stackedItems.eq(s)))
                .execute(conn),
            (Some(i), None) => diesel::update(target)
                .set(inventories::instancedItems.eq(i))
                .execute(conn),
            (None, Some(s)) => diesel::update(target)
                .set(inventories::stackedItems.eq(s))
                .execute(conn),
            (None, None) => Ok(0),
        }
        .map_err(db)?;
    }

    let logged_id = prepared.dedupe.clone().or_else(|| {
        prepared
            .request
            .transaction_id
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    write_log(
        conn,
        &user_id,
        &character_id,
        logged_id.as_deref(),
        caller,
        source,
        &log,
    )
    .map_err(db)?;

    let reply = response(&prepared.request, touched_values);
    if let Some(key) = &prepared.dedupe {
        let now = chrono::Utc::now();
        diesel::insert_into(inventorytransactions::table)
            .values((
                inventorytransactions::transactionId.eq(key),
                inventorytransactions::characterId.eq(&character_id),
                inventorytransactions::userId.eq(&user_id),
                inventorytransactions::requestHash.eq(&prepared.hash),
                inventorytransactions::status.eq(200),
                inventorytransactions::response.eq(jsjson::stringify(&reply)),
                inventorytransactions::createdDate.eq(now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
            ))
            .execute(conn)
            .map_err(db)?;
        let cutoff = (now - chrono::Duration::days(TRANSACTION_RECORD_DAYS))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        diesel::delete(inventorytransactions::table.filter(inventorytransactions::createdDate.lt(cutoff)))
            .execute(conn)
            .map_err(db)?;
    }
    Ok(Applied {
        response: reply,
        replayed: false,
        overspent,
    })
}

/// `LogOverspend`, after the transaction committed.
pub fn log_overspend(
    request: &TransactionRequest,
    caller: InventoryCaller,
    source: Option<&Value>,
    overspent: &[String],
) {
    if overspent.is_empty() {
        return;
    }
    let source = source
        .and_then(Value::as_str)
        .map_or(String::new(), |s| format!(", source {s}"));
    tracing::warn!(
        "Allowing overspend in transactionId {} for userId {} and characterId {} ({}{source}): {}; clamped at 0 as before (INVENTORY_REFUSE_OVERSPEND=1 refuses these)",
        js_string_of(request.transaction_id.as_ref()),
        request.user_id.as_deref().unwrap_or("undefined"),
        js_string_of(request.character_id.as_ref()),
        caller.as_str(),
        overspent.join("; ")
    );
}

/// `GetInventoryForUserIdAndCharacterId`, for an owned character.
pub fn read(conn: &mut Conn, character_id: &str, user_id: &str) -> Result<Value, Fail> {
    let (instanced, stacked) =
        read_or_create(conn, character_id, Some(user_id)).map_err(|e| Fail::Broken(e.to_string()))?;
    Ok(
        json!({"characterId": character_id, "instancedItems": parse_stored(&instanced)?, "stackedItems": parse_stored(&stacked)?}),
    )
}

/// `UpdateInstancedItem`, inside a write transaction: one stored item's data and version.
pub fn update_instanced(
    conn: &mut Conn,
    character_id: &str,
    incoming: Map<String, Value>,
    item_data: Option<Value>,
    version: Option<Value>,
    user_id: &str,
    caller: InventoryCaller,
) -> Result<Result<Value, InventoryError>, Fail> {
    let (stored, _) = read_or_create(conn, character_id, Some(user_id)).map_err(|e| Fail::Broken(e.to_string()))?;
    let Value::Array(mut items) = parse_stored(&stored)? else {
        return Err(Fail::Broken("the stored instanced items are not a list".into()));
    };
    let mut incoming = Value::Object(incoming);
    middleman::normalise(&mut incoming, false)?;
    let Some(index) = find_instanced(&items, &incoming) else {
        return Ok(Err(InventoryError::NotFound));
    };
    let catalog = js_string_of(incoming.get("catalogId"));
    if assert_existing_write(&items[index], &incoming, "update")? == WriteKind::Skip {
        tracing::info!(
            "Skipping stale stateless Instanced Item {catalog} for CharacterId {character_id} and UserId {user_id}"
        );
        return Ok(Ok(items[index].clone()));
    }
    if let Value::Object(item) = &mut items[index] {
        for (key, value) in [
            ("instanceId", incoming.get("instanceId").cloned()),
            ("itemData", item_data),
            ("updateVersion", version),
        ] {
            match value {
                Some(value) => {
                    item.insert(key.into(), value);
                }
                // Assigning undefined: the key stays in place and JSON.stringify leaves it out.
                None => {
                    item.shift_remove(key);
                }
            }
        }
    }
    tracing::info!("Updating Instanced Item {catalog} for CharacterId {character_id} and UserId {user_id}");
    diesel::update(inventories::table.find(character_id))
        .set(inventories::instancedItems.eq(jsjson::stringify(&Value::Array(items.clone()))))
        .execute(conn)
        .map_err(|e| Fail::Broken(e.to_string()))?;
    write_log(
        conn,
        user_id,
        character_id,
        None,
        caller,
        None,
        &[log_instanced("update", &items[index])],
    )
    .map_err(|e| Fail::Broken(e.to_string()))?;
    Ok(Ok(items[index].clone()))
}

/// `SetDailyTokensOnce`: on the first start with the daily tokens, every character's stacks are set
/// to the daily amounts. The number of characters changed, or `None` when the job had run.
pub fn set_daily_tokens_once(conn: &mut Conn, settings: &Settings) -> QueryResult<Option<usize>> {
    if onetimejobs::table
        .find(DAILY_TOKENS_JOB)
        .select(onetimejobs::jobId)
        .first::<String>(conn)
        .optional()?
        .is_some()
    {
        return Ok(None);
    }
    let rows: Vec<(String, String)> = inventories::table
        .select((inventories::characterId, inventories::stackedItems))
        .load(conn)?;
    let mut changed = 0;
    for (character_id, text) in rows {
        let Ok(Value::Array(mut stacks)) = jsjson::parse(&text) else {
            continue;
        };
        let mut touched = false;
        for (catalog, amount) in [
            ("TOKEN_DAILY_PATROL_BONUS", settings.patrol_bonus_daily),
            ("TOKEN_BOUNTY_DRAFT", settings.bounty_tokens_daily),
        ] {
            let found = stacks
                .iter_mut()
                .find(|stack| stack.get("catalogId").and_then(Value::as_str) == Some(catalog));
            match found {
                Some(stack) if stack.get("quantity").and_then(Value::as_f64) == Some(amount as f64) => continue,
                Some(Value::Object(stack)) => {
                    stack.insert("quantity".into(), json!(amount));
                }
                Some(_) => {}
                None => stacks.push(json!({"catalogId": catalog, "quantity": amount})),
            }
            touched = true;
        }
        if touched {
            diesel::update(inventories::table.find(&character_id))
                .set(inventories::stackedItems.eq(jsjson::stringify(&Value::Array(stacks))))
                .execute(conn)?;
            changed += 1;
        }
    }
    diesel::insert_into(onetimejobs::table)
        .values((
            onetimejobs::jobId.eq(DAILY_TOKENS_JOB),
            onetimejobs::doneDate.eq(iso_now()),
        ))
        .execute(conn)?;
    Ok(Some(changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arena(stored: Value, adds: Value) -> (Arena, Vec<usize>, Vec<usize>) {
        let mut arena = Arena { items: vec![] };
        let stacked = stored
            .as_array()
            .unwrap()
            .iter()
            .map(|v| arena.add(v.clone()))
            .collect();
        let adds = adds.as_array().unwrap().iter().map(|v| arena.add(v.clone())).collect();
        (arena, stacked, adds)
    }

    #[test]
    fn removals_may_spend_what_the_same_transaction_adds() {
        let (mut arena, mut stacked, adds) = arena(
            json!([{"catalogId": "A", "quantity": 2}]),
            json!([{"catalogId": "B", "quantity": "3"}]),
        );
        let removes = vec![
            json!({"catalogId": "A", "quantity": 1}),
            json!({"catalogId": "B", "quantity": 2}),
        ];
        let result = apply_stacked(&mut arena, &mut stacked, &removes, &adds, &Settings::default()).unwrap();
        let stored: Vec<Value> = stacked.iter().map(|id| arena.items[*id].clone()).collect();
        assert_eq!(
            stored,
            [
                json!({"catalogId": "A", "quantity": 1}),
                json!({"catalogId": "B", "quantity": 1})
            ]
        );
        assert_eq!(result.touched.len(), 2);
        assert!(result.overspent.is_empty());
    }

    #[test]
    fn rams_overspends_are_always_refused() {
        let (mut arena, mut stacked, adds) = arena(json!([]), json!([]));
        let removes = vec![json!({"catalogId": "CURRENCY_NOTES", "quantity": 5})];
        assert!(matches!(
            apply_stacked(&mut arena, &mut stacked, &removes, &adds, &Settings::default()),
            Err(Fail::Insufficient(_))
        ));
        let removes = vec![json!({"catalogId": "ORE", "quantity": 5})];
        let result = apply_stacked(&mut arena, &mut stacked, &removes, &adds, &Settings::default()).unwrap();
        assert_eq!(result.overspent, ["removes 5 of ORE but only 0 held"]);
    }

    #[test]
    fn dedupe_keys() {
        assert_eq!(dedupe_key(Some(&json!("00000000-0000"))), None);
        assert_eq!(dedupe_key(Some(&json!(5))), None);
        assert_eq!(dedupe_key(Some(&json!("abc-1"))), Some("abc-1".into()));
    }
}
