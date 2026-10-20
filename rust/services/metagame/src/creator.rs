//! The Support-a-Creator code (`controllers/creator.ts`): "Void" grants 10,000 Rams once per account,
//! recorded in onetimejobs (a permanent receipt; inventory receipts expire and are per character).

use diesel::prelude::*;
use serde_json::{Value, json};

use crate::characters;
use crate::inventory::{self, Fail, InventoryCaller, TransactionRequest};
use crate::schema::{inventories, onetimejobs};
use crate::store::{Context, StoreFail};
use crate::time::iso;

pub type Conn = store::Connection;

const CODE: &str = "Void";
pub const REWARD_RAMS: i64 = 10_000;

fn receipt_id(account: &str) -> String {
    format!("creator:void:{account}")
}

fn reply(supported: bool) -> serde_json::Map<String, Value> {
    let mut reply = serde_json::Map::new();
    reply.insert("success".into(), json!(supported));
    reply.insert("slug".into(), json!(if supported { CODE } else { "" }));
    reply.insert("expirationDate".into(), json!("2099-01-01T00:00:00.000Z"));
    reply
}

fn redeemed(conn: &mut Conn, account: &str) -> QueryResult<bool> {
    onetimejobs::table
        .find(receipt_id(account))
        .select(onetimejobs::jobId)
        .first::<String>(conn)
        .optional()
        .map(|row| row.is_some())
}

/// `GetCreatorSupport`.
pub fn support(conn: &mut Conn, account: &str) -> QueryResult<Value> {
    Ok(Value::Object(reply(redeemed(conn, account)?)))
}

/// How a redeem failed: a refusal the client sees, an inventory refusal, or anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedeemFail {
    Store(StoreFail),
    Inventory,
}

impl From<StoreFail> for RedeemFail {
    fn from(fail: StoreFail) -> Self {
        Self::Store(fail)
    }
}

impl From<store::Error> for RedeemFail {
    fn from(error: store::Error) -> Self {
        Self::Store(error.into())
    }
}

impl From<diesel::result::Error> for RedeemFail {
    fn from(error: diesel::result::Error) -> Self {
        Self::Store(error.into())
    }
}

/// `RedeemCreatorCode`'s check before the transaction: only "void" (any case, trimmed).
pub fn check_slug(slug: Option<&Value>) -> Result<(), StoreFail> {
    match slug.and_then(Value::as_str) {
        Some(slug) if slug.chars().count() <= 64 && slug.trim().to_lowercase() == "void" => Ok(()),
        _ => Err(StoreFail::Refused(404, "Creator code not found".into())),
    }
}

/// `RedeemCreatorCode`'s transaction (`BEGIN IMMEDIATE`).
pub fn redeem(conn: &mut Conn, ctx: &Context, account: &str) -> Result<Value, RedeemFail> {
    let refused = |message: &str| RedeemFail::Store(StoreFail::Refused(409, message.into()));
    if redeemed(conn, account)? {
        return Err(refused("Void has already been redeemed on this account"));
    }
    let Some(character) = characters::active(conn, account)? else {
        return Err(refused("Create a character before redeeming Void"));
    };
    let saved: Option<String> = inventories::table
        .find(&character.characterId)
        .select(inventories::stackedItems)
        .first(conn)
        .optional()?;
    let balance = match saved {
        None => json!(0),
        Some(text) => {
            // A broken inventory is an inventory refusal (Node's SyntaxError); a stored non-list fails
            let stacks: Value = serde_json::from_str(&text).map_err(|_| RedeemFail::Inventory)?;
            let stacks = stacks
                .as_array()
                .ok_or_else(|| RedeemFail::Store(StoreFail::Internal("TypeError: find is not a function".into())))?;
            stacks
                .iter()
                .find(|item| item.get("catalogId").and_then(Value::as_str) == Some("CURRENCY_NOTES"))
                .and_then(|item| item.get("quantity"))
                .filter(|quantity| !quantity.is_null())
                .cloned()
                .unwrap_or(json!(0))
        }
    };
    // Native inventory quantities are signed 32-bit integers.
    if !crate::js::safe_integer(Some(&balance)).is_some_and(|b| (0..=2_147_483_647 - REWARD_RAMS).contains(&b)) {
        return Err(refused("Rams balance cannot receive this reward"));
    }
    diesel::insert_into(onetimejobs::table)
        .values((
            onetimejobs::jobId.eq(receipt_id(account)),
            onetimejobs::doneDate.eq(iso(ctx.now)),
        ))
        .execute(conn)?;
    let request = TransactionRequest {
        user_id: Some(account.to_owned()),
        character_id: Some(json!(character.characterId)),
        transaction_id: Some(json!(receipt_id(account))),
        add_instanced: None,
        add_stacked: Some(json!([{"catalogId": "CURRENCY_NOTES", "quantity": REWARD_RAMS}])),
        remove_instanced: None,
        remove_stacked: None,
        save_instanced: None,
    };
    match inventory::apply_in_tx(
        conn,
        request,
        InventoryCaller::Store,
        Some(&json!("creator:Void")),
        &ctx.inventory,
    ) {
        Ok(_) => {}
        Err(Fail::Broken(message)) => return Err(RedeemFail::Store(StoreFail::Internal(message))),
        Err(_) => return Err(RedeemFail::Inventory),
    }
    let mut reply = reply(true);
    reply.insert("grantedRams".into(), json!(REWARD_RAMS));
    reply.insert("refreshInventory".into(), json!(true));
    Ok(Value::Object(reply))
}
