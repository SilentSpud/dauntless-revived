//! The store (`controllers/freestore.ts`, `store.ts`, `storeclock.ts`, `shoplayout.ts`,
//! `ramspricing.ts`): the free cosmetic webstore (`STORE=free`), Lady Luck's Trials store
//! (`TRIALS_STORE`) and the Middleman (`MIDDLEMAN_STORE`), which share the client's
//! token/notification purchase protocol, and the balance sheets.
//!
//! The client's flow (0x140b39657, 0x140b55011):
//! - GET /product/skus/public?requiredTags=<tag>: the offers of one tag (the storefront is "webstore"),
//!   each with "remaining" (0 once everything it grants is owned);
//! - GET /product/sku/<sku>: one offer, for the purchase dialog;
//! - GET /token/<currency>/<sku>: a purchase token for that offer;
//! - POST /notification/<currency>?token=<token>: confirms the purchase, and the grant happens here.
//!
//! Nothing the client sends decides what is granted or what it costs: offers come from the vendor
//! catalogues. Offers stay JSON objects, keys in the catalogue's order, because the client reads them
//! as they are and a purchase token is bound to the hash of the offer's JSON text.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, Ordering};

use diesel::prelude::*;
use serde_json::{Map, Value, json};
use service_kit::collate::locale_compare;
use service_kit::env::Env;

use crate::characters;
use crate::crypto::{random_hex, sha256_hex};
use crate::entitlements;
use crate::inventory::{self, Fail, InventoryCaller, TransactionRequest};
use crate::js;
use crate::schema::{characters as character_rows, inventories, storepurchases, users};
use crate::time::iso;
use crate::vendor;

pub type Conn = store::Connection;
pub type Offer = Map<String, Value>;

const PURCHASE_TOKEN_MINUTES: i64 = 10;
/// Purchase tokens one account may be issued in a sliding window, whatever became of them: room for a
/// player buying one cosmetic after another, not for a script.
pub const MAX_TOKENS_PER_WINDOW: i64 = 60;
pub const TOKEN_WINDOW_MS: i64 = 10 * 60 * 1000;
/// A redeemed row is the receipt a retried redeem is answered from; the grant itself stays in
/// inventorylog and entitlements, so the receipt goes after this many days.
pub const RECEIPT_DAYS: i64 = 30;
/// The sweep over the whole table: at start, then at most this often, when a token is issued.
const SWEEP_EVERY_MS: i64 = 60 * 60 * 1000;
const WEEK_MS: i64 = 7 * 86_400_000;
/// `Date.UTC(1970, 0, 1, 18)`: the weekly reset is Thursday 18:00 UTC.
const WEEK_EPOCH: i64 = 18 * 3_600_000;
const MIDDLEMAN_WEEKLY_TAG: &str = "weekly_cell_offering";

/// The store's switches (`features.ts` and the `STORE_*` variables), read once at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `STORE=free`: the ordinary cosmetic webstore.
    pub free: bool,
    /// `STORE_REPEATABLE_TOKENS`: the bounty-token bundle is sold.
    pub repeatable_tokens: bool,
    /// `TRIALS_STORE` (on): Lady Luck.
    pub trials_store: bool,
    /// `MIDDLEMAN_STORE` (on): the Middleman's slots and weekly cells.
    pub middleman_store: bool,
    /// `BALANCE_FROM_INVENTORY` (on): balance sheets show what the active character holds.
    pub balance_from_inventory: bool,
    /// `STORE_RAMS=1`: webstore prices in Rams, and the main shop only for launchers that speak it.
    pub rams: bool,
    /// `STORE_RAMS_TEST_ACCOUNT`: the one account Rams pricing is on for.
    pub rams_test_account: Option<String>,
    /// `STORE_RAMS_EVENT_TRANSPORT=1`: Rams prices sent as the Event01 currency the client can show.
    pub rams_event_transport: bool,
    /// `STORE_WEEKLY_ROTATION=1`: the Rams webstore shows a weekly selection.
    pub weekly_rotation: bool,
    /// `STORE_ROTATION_SEED`.
    pub rotation_seed: String,
    /// `STORE_CATALOG_PROFILE=curated30`.
    pub curated30: bool,
    /// `TRIAL_ROTATION_START`, parsed; `None` when it doesn't parse (the Trials code answers 500).
    pub trial_rotation_start: Option<i64>,
    /// `TRIALS_LEADERBOARDS` (on): the leaderboard routes and run submissions.
    pub trials_leaderboards: bool,
    /// `TRIALS_SCHEDULE` (off): the tuning schedule advertises this week's Trials.
    pub trials_schedule: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_env(&Env::from_pairs([]))
    }
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let free = match env.get("STORE").map(|v| v.trim().to_ascii_lowercase()) {
            None => false,
            Some(v) if v == "free" => true,
            Some(v) if v == "off" || v.is_empty() => false,
            Some(v) => {
                tracing::warn!("STORE={v:?} is not a valid value; using the default (off)");
                false
            }
        };
        Self {
            free,
            repeatable_tokens: crate::config::on_off(env, "STORE_REPEATABLE_TOKENS", false),
            trials_store: crate::config::on_off(env, "TRIALS_STORE", true),
            middleman_store: crate::config::on_off(env, "MIDDLEMAN_STORE", true),
            balance_from_inventory: crate::config::on_off(env, "BALANCE_FROM_INVENTORY", true),
            rams: env.raw("STORE_RAMS") == Some("1"),
            rams_test_account: env.get("STORE_RAMS_TEST_ACCOUNT").map(str::to_owned),
            rams_event_transport: env.raw("STORE_RAMS_EVENT_TRANSPORT") == Some("1"),
            weekly_rotation: env.raw("STORE_WEEKLY_ROTATION") == Some("1"),
            rotation_seed: env.raw("STORE_ROTATION_SEED").unwrap_or("").to_owned(),
            curated30: env.raw("STORE_CATALOG_PROFILE") == Some("curated30"),
            trial_rotation_start: js::date_parse(
                env.raw("TRIAL_ROTATION_START").unwrap_or(crate::trials::ROTATION_START),
            ),
            trials_leaderboards: crate::config::on_off(env, "TRIALS_LEADERBOARDS", true),
            trials_schedule: crate::config::on_off(env, "TRIALS_SCHEDULE", false),
        }
    }

    /// `RamsEnabled`.
    pub fn rams_enabled(&self, account: &str) -> bool {
        self.rams || self.rams_test_account.as_deref() == Some(account)
    }

    /// The pilot's wire adapter: only with `STORE_RAMS_EVENT_TRANSPORT=1` and Rams on for the account.
    fn rams_wire_enabled(&self, account: &str) -> bool {
        self.rams_event_transport && self.rams_enabled(account)
    }

    /// `IsAnyStoreEnabled`.
    pub fn any_enabled(&self) -> bool {
        self.free || self.trials_store || self.middleman_store
    }

    /// `IsStoreTagEnabled`.
    pub fn tag_enabled(&self, tag: &str) -> bool {
        match tag {
            "ladyluckstore" => self.trials_store,
            MIDDLEMAN_WEEKLY_TAG | "exchange_vendor_slot_2" | "exchange_vendor_slot_3" => self.middleman_store,
            _ => self.free,
        }
    }

    /// `IsStoreSkuEnabled`.
    pub fn sku_enabled(&self, sku: &str) -> bool {
        let catalog = catalog();
        if catalog.ladyluck_ids.contains(sku) {
            self.trials_store
        } else if catalog.middleman_vendor_ids.contains(sku) || catalog.middleman_offer_ids.contains(sku) {
            self.middleman_store
        } else {
            self.free
        }
    }
}

/// What a store call needs besides the connection.
#[derive(Debug, Clone)]
pub struct Context {
    pub settings: Settings,
    pub entitlement_defaults: Vec<String>,
    pub inventory: inventory::Settings,
    pub now: i64,
}

/// A refusal (`StoreError`): its status and message go to the client. `Internal` is anything else,
/// which Node answered with a 500 and logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreFail {
    Refused(u16, String),
    Internal(String),
}

impl StoreFail {
    fn refused(status: u16, message: impl Into<String>) -> Self {
        Self::Refused(status, message.into())
    }
}

impl From<store::Error> for StoreFail {
    fn from(error: store::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<diesel::result::Error> for StoreFail {
    fn from(error: diesel::result::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

// ---- The catalogue ----

struct Catalog {
    /// Every tag and its offers, in `Catalog`'s key order (keys starting with _ are notes).
    tags: Vec<(String, Vec<Offer>)>,
    ladyluck_ids: HashSet<String>,
    ladyluck_kinds: Map<String, Value>,
    ladyluck_repeatable: HashSet<String>,
    middleman_cells: Vec<Offer>,
    middleman_cell_ids: HashSet<String>,
    middleman_offer_ids: HashSet<String>,
    middleman_vendor_ids: HashSet<String>,
    /// The weekly cell families, sorted by their rotation score.
    families: Vec<(String, String)>,
    item_kinds: Map<String, Value>,
    curated30: HashSet<String>,
}

fn offers_of(value: &Value) -> Vec<Offer> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|offer| offer.as_object().cloned())
        .collect()
}

fn ids(offers: &[Offer]) -> HashSet<String> {
    offers.iter().map(|offer| id(offer).to_owned()).collect()
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let ladyluck = vendor::store_ladyluck();
        let middleman = vendor::store_middleman();
        let families: Vec<(String, String)> = vendor::store_middleman_cells()["families"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|family| {
                (
                    family["id"].as_str().unwrap_or("").to_owned(),
                    family["name"].as_str().unwrap_or("").to_owned(),
                )
            })
            .collect();
        let middleman_cells: Vec<Offer> = families
            .iter()
            .flat_map(|(family, name)| {
                [("UC", 1, 40), ("R", 2, 100)].map(|(suffix, rank, price)| {
                    let catalog_id = format!("CELL_{family}_{suffix}");
                    let offer = json!({
                        "id": format!("middleman_weekly_{}", catalog_id.to_lowercase()),
                        "displayName": format!("+{rank} {name} Cell"),
                        "displayDescription": "Purchase one cell with Aetherdust.",
                        "displayPriority": 0,
                        "tags": [MIDDLEMAN_WEEKLY_TAG],
                        // Only the Aetherdust price: with a platinumPrice key present (even null) the
                        // 1.4.4 Middleman offers the cell for Platinum instead.
                        "cellDustPrice": price,
                        "items": [{"catalogId": catalog_id, "quantity": 1}],
                        "entitlements": [],
                        "maxAllowed": 1,
                        "remaining": 1,
                        "loadoutSlots": null,
                        "availableFrom": null,
                        "availableTo": null,
                        "timeAvailabilityReason": null,
                        "platformOfferId": null,
                        "missingEntitlementNames": null
                    });
                    match offer {
                        Value::Object(offer) => offer,
                        _ => unreachable!(),
                    }
                })
            })
            .collect();
        let mut sorted = families.clone();
        let score = |id: &str| sha256_hex(&format!("middleman-weekly-v1:{id}"));
        sorted.sort_by(|a, b| locale_compare(&score(&a.0), &score(&b.0)));

        let mut tags = Vec::new();
        for (tag, offers) in vendor::store_catalog().as_object().into_iter().flatten() {
            if !tag.starts_with('_') && offers.is_array() {
                tags.push((tag.clone(), offers_of(offers)));
            }
        }
        let ladyluck_offers = offers_of(&ladyluck["offers"]);
        let slot_2 = offers_of(&middleman["exchange_vendor_slot_2"]);
        let slot_3 = offers_of(&middleman["exchange_vendor_slot_3"]);
        let mut middleman_vendor_ids = ids(&slot_2);
        middleman_vendor_ids.extend(ids(&slot_3));
        let ladyluck_ids = ids(&ladyluck_offers);
        tags.push(("ladyluckstore".into(), ladyluck_offers));
        tags.push(("exchange_vendor_slot_2".into(), slot_2));
        tags.push(("exchange_vendor_slot_3".into(), slot_3));
        tags.push((MIDDLEMAN_WEEKLY_TAG.into(), middleman_cells.clone()));

        let strings = |value: &Value| -> HashSet<String> {
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        };
        Catalog {
            tags,
            ladyluck_ids,
            ladyluck_kinds: ladyluck["itemKinds"].as_object().cloned().unwrap_or_default(),
            ladyluck_repeatable: strings(&ladyluck["repeatable"]),
            middleman_cell_ids: middleman_cells
                .iter()
                .filter_map(|offer| items(offer).first()?.get("catalogId")?.as_str().map(str::to_owned))
                .collect(),
            middleman_offer_ids: ids(&middleman_cells),
            middleman_cells,
            middleman_vendor_ids,
            families: sorted,
            item_kinds: vendor::store_item_kinds().as_object().cloned().unwrap_or_default(),
            curated30: strings(vendor::store_curated_30()),
        }
    })
}

fn tag_offers(tag: &str) -> Option<&'static [Offer]> {
    catalog()
        .tags
        .iter()
        .find(|(name, _)| name == tag)
        .map(|(_, offers)| offers.as_slice())
}

/// `IsKnownStoreTag`.
pub fn is_known_tag(tag: &str) -> bool {
    tag_offers(tag).is_some()
}

/// Whether `sku` is an offer of the main shop (`MainShopSkus`).
pub fn is_main_shop_sku(sku: &str) -> bool {
    tag_offers("webstore").is_some_and(|offers| offers.iter().any(|offer| id(offer) == sku))
}

fn id(offer: &Offer) -> &str {
    offer.get("id").and_then(Value::as_str).unwrap_or("")
}

fn tags(offer: &Offer) -> Vec<&str> {
    offer
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn has_tag(offer: &Offer, tag: &str) -> bool {
    tags(offer).contains(&tag)
}

/// `Offer.items ?? []`.
fn items(offer: &Offer) -> &[Value] {
    offer.get("items").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// `Offer.entitlements ?? []`.
fn grants(offer: &Offer) -> &[Value] {
    offer
        .get("entitlements")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn catalog_id(item: &Value) -> &str {
    item.get("catalogId").and_then(Value::as_str).unwrap_or("")
}

fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

// ---- The weekly clock (`storeclock.ts`) ----

/// One boundary for Middleman offers, cosmetic stock and their availability dates: the week's index
/// and its start and end in milliseconds.
pub fn weekly_window(at: i64) -> (i64, i64, i64) {
    let index = (at - WEEK_EPOCH).div_euclid(WEEK_MS);
    let start = WEEK_EPOCH + index * WEEK_MS;
    (index, start, start + WEEK_MS)
}

/// `SelectWeeklyMiddlemanOffers`: three cell families a week, the third at +2.
pub fn weekly_middleman_offers(at: i64) -> Vec<Offer> {
    let catalog = catalog();
    let families = &catalog.families;
    if families.is_empty() {
        return Vec::new();
    }
    let (week, start, end) = weekly_window(at);
    let count = families.len() as i64;
    let offset = (week * 3).rem_euclid(count);
    (0..families.len().min(3))
        .map(|index| {
            let (family, _) = &families[((offset + index as i64) % count) as usize];
            let suffix = if index == 2 { "R" } else { "UC" };
            let wanted = format!("CELL_{family}_{suffix}");
            let mut offer = catalog
                .middleman_cells
                .iter()
                .find(|offer| items(offer).first().map(catalog_id) == Some(wanted.as_str()))
                .cloned()
                .unwrap_or_default();
            offer.insert("displayPriority".into(), json!(index));
            offer.insert("availableFrom".into(), json!(iso(start)));
            offer.insert("availableTo".into(), json!(iso(end)));
            offer
        })
        .collect()
}

// ---- What may be sold ----

const COSMETIC_PREFIXES: [&str; 9] = [
    "AR_",
    "WP_",
    "EM_",
    "DYE_",
    "QI_FLARE_",
    "BNC_FABRIC_",
    "BNC_STANDARD_",
    "BNC_SIGIL_",
    "LT_",
];

/// Consumables that may be bought any number of times: the premium bounty token, which persists into
/// the next season.
const REPEATABLE_ITEMS: [&str; 1] = ["TOKEN_BOUNTY_DRAFT_PREMIUM"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantKind {
    Stacked,
    Instanced,
}

fn kind_of(value: Option<&Value>) -> Option<GrantKind> {
    match value.and_then(Value::as_str) {
        Some("stacked") => Some(GrantKind::Stacked),
        Some("instanced") => Some(GrantKind::Instanced),
        _ => None,
    }
}

/// `GrantKind`: how an item is granted, from the client catalogue's stackable flag; `None` for
/// anything the store may not hand out (currencies, boosts, weapons proper).
pub fn grant_kind(catalog_id: &str) -> Option<GrantKind> {
    let catalog = catalog();
    if catalog.middleman_cell_ids.contains(catalog_id) {
        return Some(GrantKind::Stacked);
    }
    if let Some(kind) = kind_of(catalog.ladyluck_kinds.get(catalog_id)) {
        return Some(kind);
    }
    if !REPEATABLE_ITEMS.contains(&catalog_id) && !COSMETIC_PREFIXES.iter().any(|p| catalog_id.starts_with(p)) {
        return None;
    }
    kind_of(catalog.item_kinds.get(catalog_id))
}

/// A repeatable offer sells consumables only; it is never "owned", and each purchase grants its full
/// quantity.
fn is_repeatable(offer: &Offer) -> bool {
    let catalog = catalog();
    if catalog.middleman_offer_ids.contains(id(offer)) || catalog.ladyluck_repeatable.contains(id(offer)) {
        return true;
    }
    let items = items(offer);
    !items.is_empty() && items.iter().all(|item| REPEATABLE_ITEMS.contains(&catalog_id(item)))
}

fn is_listed(settings: &Settings, offer: &Offer) -> bool {
    let catalog = catalog();
    let id = id(offer);
    if id.starts_with("single_dye_sheen_")
        || grants(offer).iter().any(|grant| {
            grant
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.starts_with("ent_dye_sheen_"))
        })
    {
        return false;
    }
    let ladyluck = catalog.ladyluck_ids.contains(id);
    let middleman = catalog.middleman_vendor_ids.contains(id) || catalog.middleman_offer_ids.contains(id);
    if (ladyluck && !settings.trials_store)
        || (middleman && !settings.middleman_store)
        || (!ladyluck && !middleman && !settings.free)
    {
        return false;
    }
    if settings.curated30 && has_tag(offer, "webstore") && !catalog.curated30.contains(id) {
        return false;
    }
    !is_repeatable(offer) || catalog.ladyluck_repeatable.contains(id) || middleman || settings.repeatable_tokens
}

// ---- Rams pricing (`ramspricing.ts`) and the weekly shop (`shoplayout.ts`) ----

/// `(1234).toLocaleString("en-US")` for integers.
fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn priced_in_rams(offer: &Offer, price: i64) -> Offer {
    let mut result = offer.clone();
    result.insert("notesPrice".into(), json!(price));
    result.insert(
        "displayDescription".into(),
        json!(format!("Purchase for {} Rams.", grouped(price))),
    );
    result.shift_remove("platinumPrice");
    result
}

/// `PriceInRams`: a webstore offer's price in Rams, for accounts with Rams pricing. Pilot only.
pub fn price_in_rams(settings: &Settings, offer: Offer, account: &str) -> Offer {
    if !settings.rams_enabled(account) || !has_tag(&offer, "webstore") {
        return offer;
    }
    if !grants(&offer).is_empty() {
        let dye = tags(&offer).iter().any(|t| *t == "dye_armour" || *t == "dye_tint");
        if !dye || !items(&offer).is_empty() {
            return offer;
        }
        let mut result = offer.clone();
        result.insert("notesPrice".into(), json!(5000));
        result.insert("displayDescription".into(), json!("Purchase for 5,000 Rams."));
        result.shift_remove("platinumPrice");
        return result;
    }
    let items = items(&offer);
    if items.is_empty() {
        return offer;
    }
    let prices: Option<Vec<i64>> = items
        .iter()
        .map(|item| {
            if !js::strict_eq(item.get("quantity"), Some(&json!(1))) {
                return None;
            }
            let id = catalog_id(item);
            let starts = |prefixes: &[&str]| prefixes.iter().any(|p| id.starts_with(p));
            if id.starts_with("DYE_") {
                Some(5000)
            } else if id.starts_with("WP_") {
                Some(2500)
            } else if starts(&["BNC_SIGIL_", "QI_FLARE_"]) {
                Some(250)
            } else if starts(&["AR_", "EM_", "BNC_FABRIC_", "BNC_STANDARD_", "LT_"]) {
                Some(750)
            } else {
                None
            }
        })
        .collect();
    // Unsupported offers remain unchanged rather than inventing prices for currencies or consumables.
    let Some(prices) = prices else {
        return offer;
    };
    let full_kit = items.iter().filter(|item| catalog_id(item).starts_with("AR_")).count() >= 4;
    let price = if full_kit {
        5000
    } else {
        prices.iter().sum::<i64>().min(5000)
    };
    priced_in_rams(&offer, price)
}

/// `RamsWireOffer`: the old client parses event01Price but not notesPrice, so the pilot sends Rams as
/// Event01. Accounting always stays in Rams.
pub fn rams_wire_offer(settings: &Settings, offer: Offer, account: &str) -> Offer {
    let Some(price) = offer.get("notesPrice").filter(|v| v.is_number()).cloned() else {
        return offer;
    };
    if !settings.rams_wire_enabled(account) {
        return offer;
    }
    let mut result = offer;
    result.insert("event01Price".into(), price);
    let mut tags: Vec<Value> = result
        .get("tags")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    tags.push(json!("rams-store-pilot"));
    result.insert("tags".into(), Value::Array(tags));
    result.shift_remove("notesPrice");
    result
}

/// `RamsWireCurrency`: Event01 means Rams on the pilot's wire.
pub fn rams_wire_currency(settings: &Settings, currency: &str, account: &str) -> String {
    let lower = currency.to_lowercase();
    if settings.rams_wire_enabled(account) && (lower == "event01" || lower == "event_01") {
        "notes".into()
    } else {
        currency.into()
    }
}

fn category(offer: &Offer) -> String {
    tags(offer)
        .into_iter()
        .find(|tag| {
            ["skin_", "social_", "dye_", "personality_", "supplies_"]
                .iter()
                .any(|p| tag.starts_with(p))
        })
        .unwrap_or("other")
        .to_owned()
}

/// `ShopLayout`: with Rams and `STORE_WEEKLY_ROTATION=1`, up to three offers per category each week
/// (consecutive windows, so a category of six or more doesn't repeat), the first eight featured. A
/// seed refreshes current stock without changing the weekly schedule.
pub fn shop_layout(settings: &Settings, offers: Vec<Offer>, account: &str, now: i64) -> Vec<Offer> {
    if !settings.rams_enabled(account) || !settings.weekly_rotation {
        return offers;
    }
    let (week, start, end) = weekly_window(now);
    let seed = &settings.rotation_seed;
    let mut ranked: Vec<(String, String)> = offers
        .iter()
        .map(|offer| (id(offer).to_owned(), sha256_hex(&format!("{seed}:{}", id(offer)))))
        .collect();
    ranked.sort_by(|a, b| locale_compare(&a.1, &b.1).then_with(|| locale_compare(&a.0, &b.0)));
    let cleaned: Vec<Offer> = offers
        .iter()
        .map(|offer| {
            let all = tags(offer);
            let fabric_only = offer
                .get("items")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().all(|item| catalog_id(item).starts_with("BNC_FABRIC_")));
            let kept: Vec<Value> = all
                .iter()
                .filter(|tag| {
                    **tag != "feature"
                        && !(**tag == "personality_character" && all.contains(&"social_emote"))
                        && !(**tag == "personality_stylekit" && all.contains(&"dye_armour"))
                        && !(**tag == "personality_standard" && fabric_only)
                })
                .map(|tag| json!(tag))
                .collect();
            let mut offer = offer.clone();
            offer.insert("tags".into(), Value::Array(kept));
            offer
        })
        .collect();
    // Offers by id; with a repeated id the last one wins, as in a JavaScript Map.
    let by_id = |wanted: &str| cleaned.iter().rev().find(|offer| id(offer) == wanted);
    let mut categories: Vec<(String, Vec<&Offer>)> = Vec::new();
    for (offer_id, _) in &ranked {
        let Some(offer) = by_id(offer_id) else { continue };
        let name = category(offer);
        match categories.iter_mut().find(|(c, _)| *c == name) {
            Some((_, group)) => group.push(offer),
            None => categories.push((name, vec![offer])),
        }
    }
    let mut selected: Vec<&Offer> = Vec::new();
    for (_, group) in &categories {
        let len = group.len() as i64;
        let count = 3.min(len);
        let first = (week * count).rem_euclid(len);
        for i in 0..count {
            selected.push(group[((first + i) % len) as usize]);
        }
    }
    let featured: HashSet<&str> = selected.iter().take(8).map(|offer| id(offer)).collect();
    selected
        .into_iter()
        .map(|offer| {
            let mut offer = offer.clone();
            if featured.contains(id(&offer)) {
                let mut tags: Vec<Value> = offer.get("tags").and_then(Value::as_array).cloned().unwrap_or_default();
                tags.push(json!("feature"));
                offer.insert("tags".into(), Value::Array(tags));
            }
            offer.insert("availableFrom".into(), json!(iso(start)));
            offer.insert("availableTo".into(), json!(iso(end)));
            offer
        })
        .collect()
}

// ---- Offers ----

fn find_offer(ctx: &Context, sku: &str, account: &str) -> Option<Offer> {
    let catalog = catalog();
    let settings = &ctx.settings;
    if settings.middleman_store
        && let Some(offer) = weekly_middleman_offers(ctx.now)
            .into_iter()
            .find(|offer| id(offer) == sku)
    {
        return Some(offer);
    }
    if catalog.middleman_offer_ids.contains(sku) {
        return None;
    }
    let offer = catalog
        .tags
        .iter()
        .flat_map(|(_, offers)| offers)
        .find(|offer| id(offer) == sku && is_listed(settings, offer))?;
    if has_tag(offer, "webstore") && is_main_shop_sku(sku) {
        let listed: Vec<Offer> = tag_offers("webstore")
            .unwrap_or_default()
            .iter()
            .filter(|offer| is_listed(settings, offer))
            .cloned()
            .collect();
        let active = shop_layout(settings, listed, account, ctx.now)
            .into_iter()
            .find(|offer| id(offer) == sku)?;
        return Some(price_in_rams(settings, active, account));
    }
    Some(price_in_rams(settings, offer.clone(), account))
}

/// Whether finding `offer` must first settle finished Trials weeks (Lady Luck's Champion stock).
fn settles_trials(ctx: &Context, offer: &Offer) -> bool {
    catalog().ladyluck_ids.contains(id(offer)) && ctx.settings.trials_store
}

const CURRENCIES: [(&str, &str, &str); 6] = [
    ("notes", "notesPrice", "CURRENCY_NOTES"),
    ("platinum", "platinumPrice", "CURRENCY_PLATINUM_UNIV"),
    ("celldust", "cellDustPrice", "CURRENCY_CELLDUST"),
    ("markssteel", "steelMarksPrice", "CURRENCY_MARKS_STEEL"),
    ("marksgilded", "gildedMarksPrice", "CURRENCY_MARKS_GILDED"),
    ("prestige", "prestigePrice", "CURRENCY_PRESTIGE"),
];

/// The 1.4.4 store can surface either its service-facing id_currency_* name or the inventory
/// catalogue id that holds the balance; both are the same payment currency.
const CURRENCY_ALIASES: [(&str, &str); 15] = [
    ("id_currency_notes", "notes"),
    ("currency_notes", "notes"),
    ("rams", "notes"),
    ("id_currency_celldust", "celldust"),
    ("currency_celldust", "celldust"),
    ("id_currency_marks_steel", "markssteel"),
    ("currency_marks_steel", "markssteel"),
    ("id_currency_marks_gilded", "marksgilded"),
    ("currency_marks_gilded", "marksgilded"),
    ("currency_platinum_univ", "platinum"),
    ("currency_prestige", "prestige"),
    ("marks_steel", "markssteel"),
    ("marks_gilded", "marksgilded"),
    ("steelmarks", "markssteel"),
    ("gildedmarks", "marksgilded"),
];

struct Pricing {
    currency: &'static str,
    catalog_id: &'static str,
    price: i64,
}

/// `ResolveOfferPricing`: exactly one price field, a safe non-negative integer.
fn pricing(offer: &Offer) -> Result<Pricing, StoreFail> {
    let matches: Vec<_> = CURRENCIES
        .iter()
        .filter(|(_, field, _)| offer.get(*field).is_some_and(Value::is_number))
        .collect();
    let [(currency, field, catalog_id)] = matches.as_slice() else {
        return Err(StoreFail::refused(
            500,
            format!(
                "Offer {} has {} price field(s) set; exactly one is required",
                id(offer),
                matches.len()
            ),
        ));
    };
    match js::safe_integer(offer.get(*field)) {
        Some(price) if price >= 0 => Ok(Pricing {
            currency,
            catalog_id,
            price,
        }),
        _ => Err(StoreFail::refused(
            500,
            format!("Offer {} has an invalid price", id(offer)),
        )),
    }
}

/// `CheckCurrency`: the canonical currency of a name the client sent.
fn check_currency(currency: &str) -> Result<&'static str, StoreFail> {
    let lower = currency.to_lowercase();
    let canonical = CURRENCY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map_or(lower.as_str(), |(_, to)| to);
    CURRENCIES
        .iter()
        .find(|(name, _, _)| *name == canonical)
        .map(|(name, _, _)| *name)
        .ok_or_else(|| StoreFail::refused(400, "Unsupported store currency"))
}

/// `CheckOffer`.
fn check_offer(offer: &Offer) -> Result<(), StoreFail> {
    let items = items(offer);
    let grants = grants(offer);
    if items.is_empty() && grants.is_empty() {
        return Err(StoreFail::refused(409, "Offer grants nothing"));
    }
    let repeatable = is_repeatable(offer);
    if repeatable && !grants.is_empty() {
        return Err(StoreFail::refused(409, "Repeatable offers cannot grant entitlements"));
    }
    let unsupported = items.iter().any(|item| {
        let kind = grant_kind(catalog_id(item));
        kind.is_none()
            || if repeatable {
                kind != Some(GrantKind::Stacked) || !js::safe_integer(item.get("quantity")).is_some_and(|q| q > 0)
            } else {
                !js::strict_eq(item.get("quantity"), Some(&json!(1)))
            }
    });
    if unsupported {
        return Err(StoreFail::refused(409, "Offer is not a supported free item"));
    }
    let unusable = grants.iter().any(|grant| {
        !grant
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| !name.is_empty())
            || grant
                .get("duration")
                .is_some_and(|duration| !js::safe_integer(Some(duration)).is_some_and(|d| d >= 0))
    });
    if unusable {
        return Err(StoreFail::refused(409, "Offer has an unusable entitlement"));
    }
    Ok(())
}

fn read_items(value: Option<&str>) -> Result<Vec<Value>, StoreFail> {
    match serde_json::from_str(value.unwrap_or("[]")) {
        Ok(Value::Array(items)) => Ok(items),
        _ => Err(StoreFail::refused(
            409,
            "Inventory unavailable; purchase was not completed",
        )),
    }
}

fn inventory_row(conn: &mut Conn, character: &str) -> QueryResult<Option<(String, String)>> {
    inventories::table
        .find(character)
        .select((inventories::instancedItems, inventories::stackedItems))
        .first(conn)
        .optional()
}

/// `CurrencyBalance`: the first stack of the currency with a usable quantity.
fn currency_balance(conn: &mut Conn, character: &str, catalog: &str) -> Result<i64, StoreFail> {
    let row = inventory_row(conn, character)?;
    for stack in read_items(row.as_ref().map(|r| r.1.as_str()))? {
        let quantity = js::number(stack.get("quantity"));
        if stack.get("catalogId").and_then(Value::as_str) == Some(catalog)
            && quantity.fract() == 0.0
            && (0.0..=9_007_199_254_740_991.0).contains(&quantity)
        {
            return Ok(quantity as i64);
        }
    }
    Ok(0)
}

/// `HeldCatalogIds`: every catalogue id the character holds, stacked (above 0) or instanced.
fn held_catalog_ids(conn: &mut Conn, character: Option<&str>) -> Result<HashSet<String>, StoreFail> {
    let mut held = HashSet::new();
    let Some(character) = character else {
        return Ok(held);
    };
    let row = inventory_row(conn, character)?;
    for stack in read_items(row.as_ref().map(|r| r.1.as_str()))? {
        if let Some(id) = stack.get("catalogId").and_then(Value::as_str)
            && js::number(stack.get("quantity")) > 0.0
        {
            held.insert(id.to_owned());
        }
    }
    for instance in read_items(row.as_ref().map(|r| r.0.as_str()))? {
        if let Some(id) = instance.get("catalogId").and_then(Value::as_str) {
            held.insert(id.to_owned());
        }
    }
    Ok(held)
}

/// A Middleman weekly cell is sold once per account per weekly rotation: a redeemed purchase of that
/// offer since this week's reset uses it up, whether or not the cell is still held.
fn bought_this_middleman_week(conn: &mut Conn, now: i64, account: &str, sku: &str) -> QueryResult<bool> {
    let (_, start, _) = weekly_window(now);
    storepurchases::table
        .filter(storepurchases::accountId.eq(account))
        .filter(storepurchases::skuId.eq(sku))
        .filter(storepurchases::redeemedDate.ge(iso(start)))
        .select(storepurchases::tokenHash)
        .first::<String>(conn)
        .optional()
        .map(|row| row.is_some())
}

/// `WithRemaining`: owned (0) when everything it grants is held, all of its items and all of its
/// active entitlements (a revoked or expired one can be bought again).
fn with_remaining(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    held: &HashSet<String>,
    mut offer: Offer,
) -> QueryResult<Offer> {
    let remaining = if catalog().middleman_offer_ids.contains(id(&offer)) {
        !bought_this_middleman_week(conn, ctx.now, account, id(&offer))?
    } else if is_repeatable(&offer) {
        true
    } else {
        let items = items(&offer);
        let grants = grants(&offer);
        let mut owned = (!items.is_empty() || !grants.is_empty()) && items.iter().all(|i| held.contains(catalog_id(i)));
        if owned {
            for grant in grants {
                let name = grant.get("name").and_then(Value::as_str).unwrap_or("");
                if !entitlements::has_active(conn, &ctx.entitlement_defaults, account, name)? {
                    owned = false;
                    break;
                }
            }
        }
        !owned
    };
    offer.insert("remaining".into(), json!(u8::from(remaining)));
    Ok(offer)
}

fn active_character(conn: &mut Conn, account: &str) -> QueryResult<Option<String>> {
    Ok(characters::active(conn, account)?.map(|row| row.characterId))
}

/// The offers of a tag before anything account-specific (`ForTag`); an unknown tag has none.
pub fn offers_for_tag(ctx: &Context, tag: &str) -> Vec<Offer> {
    if tag == MIDDLEMAN_WEEKLY_TAG {
        return if ctx.settings.middleman_store {
            weekly_middleman_offers(ctx.now)
        } else {
            Vec::new()
        };
    }
    tag_offers(tag)
        .unwrap_or_default()
        .iter()
        .filter(|offer| is_listed(&ctx.settings, offer))
        .cloned()
        .collect()
}

/// `ListStoreOffers`, after `FinalizeCompletedTrialWeeks` for Lady Luck's tag.
pub fn list_offers(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    tag: &str,
    for_tag: Vec<Offer>,
) -> Result<Vec<Offer>, StoreFail> {
    let character = active_character(conn, account)?;
    let held = held_catalog_ids(conn, character.as_deref())?;
    let mut offers = Vec::with_capacity(for_tag.len());
    for offer in for_tag {
        let priced = price_in_rams(&ctx.settings, offer, account);
        offers.push(with_remaining(conn, ctx, account, &held, priced)?);
    }
    Ok(if tag == "webstore" {
        shop_layout(&ctx.settings, offers, account, ctx.now)
    } else {
        offers
    })
}

/// The offer behind `sku` and whether Trials weeks must be settled first; 404 when it isn't sold.
pub fn lookup(ctx: &Context, sku: &str, account: &str) -> Result<(Offer, bool), StoreFail> {
    let offer = find_offer(ctx, sku, account).ok_or_else(|| StoreFail::refused(404, "Unknown store offer"))?;
    let settle = settles_trials(ctx, &offer);
    Ok((offer, settle))
}

/// `GetStoreOffer`, given the offer [`lookup`] found.
pub fn offer_for(conn: &mut Conn, ctx: &Context, account: &str, offer: Offer) -> Result<Offer, StoreFail> {
    let character = active_character(conn, account)?;
    let held = held_catalog_ids(conn, character.as_deref())?;
    Ok(with_remaining(conn, ctx, account, &held, offer)?)
}

// ---- Purchases ----

/// `CheckCurrency`, for the routes: the canonical currency of a name the client sent.
pub fn canonical_currency(currency: &str) -> Result<&'static str, StoreFail> {
    check_currency(currency)
}

/// Checked before a token is issued (`CheckOffer`, then the price's currency).
pub fn check_sale(offer: &Offer, currency: &str) -> Result<(), StoreFail> {
    check_offer(offer)?;
    if pricing(offer)?.currency != currency {
        return Err(StoreFail::refused(400, "Offer is not sold in that currency"));
    }
    Ok(())
}

/// When the whole table was last swept (`LastSweep`).
#[derive(Debug, Default)]
pub struct Sweeps {
    last: AtomicI64,
}

impl Sweeps {
    /// Whether a sweep is due, claiming it if so.
    pub fn due(&self, now: i64) -> bool {
        let last = self.last.load(Ordering::Relaxed);
        now - last >= SWEEP_EVERY_MS
            && self
                .last
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
    }

    pub fn mark(&self, now: i64) {
        self.last.store(now, Ordering::Relaxed);
    }
}

/// `PruneExpiredStorePurchases`: tokens past their expiry that were never redeemed, and receipts
/// redeemed more than [`RECEIPT_DAYS`] ago. Returns both counts.
pub fn prune(conn: &mut Conn, now: i64) -> QueryResult<(usize, usize)> {
    let expired = diesel::delete(
        storepurchases::table
            .filter(storepurchases::redeemedDate.is_null())
            .filter(storepurchases::expiresDate.lt(iso(now))),
    )
    .execute(conn)?;
    let receipts = diesel::delete(
        storepurchases::table
            .filter(storepurchases::redeemedDate.is_not_null())
            .filter(storepurchases::redeemedDate.lt(iso(now - RECEIPT_DAYS * 24 * 60 * 60 * 1000))),
    )
    .execute(conn)?;
    Ok((expired, receipts))
}

/// Logs what [`prune`] removed.
pub fn log_pruned((expired, receipts): (usize, usize)) {
    if expired > 0 {
        tracing::info!("Removed {expired} expired store purchase token(s) that were never redeemed");
    }
    if receipts > 0 {
        tracing::info!("Removed {receipts} store purchase receipt(s) redeemed more than {RECEIPT_DAYS} days ago");
    }
}

fn offer_hash(offer: &Offer) -> String {
    sha256_hex(&service_kit::jsjson::stringify(&Value::Object(offer.clone())))
}

/// `CreateStorePurchase`'s transaction: a 64-hex token bound to the account's active character and
/// to the offer as it is now, valid for 10 minutes. Returns the token and the character.
pub fn issue_token(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    sku: &str,
    offer: &Offer,
) -> Result<(String, String), StoreFail> {
    let pricing = pricing(offer)?;
    let token = random_hex(32);
    let now = ctx.now;
    // This account's own expired tokens, through the account index
    diesel::delete(
        storepurchases::table
            .filter(storepurchases::accountId.eq(account))
            .filter(storepurchases::redeemedDate.is_null())
            .filter(storepurchases::expiresDate.lt(iso(now))),
    )
    .execute(conn)?;
    let character = active_character(conn, account)?
        .ok_or_else(|| StoreFail::refused(409, "The account has no character to deliver the purchase to"))?;
    let held = held_catalog_ids(conn, Some(&character))?;
    if with_remaining(conn, ctx, account, &held, offer.clone())?["remaining"] == json!(0) {
        return Err(StoreFail::refused(
            409,
            if catalog().middleman_offer_ids.contains(id(offer)) {
                "Already bought this week; the Middleman restocks at the weekly reset"
            } else {
                "You already own everything this offer grants"
            },
        ));
    }
    if pricing.price > 0 && currency_balance(conn, &character, pricing.catalog_id)? < pricing.price {
        return Err(StoreFail::refused(409, "Insufficient balance"));
    }
    let recent: i64 = storepurchases::table
        .filter(storepurchases::accountId.eq(account))
        .filter(storepurchases::createdDate.gt(iso(now - TOKEN_WINDOW_MS)))
        .count()
        .get_result(conn)?;
    if recent >= MAX_TOKENS_PER_WINDOW {
        return Err(StoreFail::refused(
            409,
            format!(
                "Too many purchases: at most {MAX_TOKENS_PER_WINDOW} in {} minutes",
                TOKEN_WINDOW_MS / 60000
            ),
        ));
    }
    diesel::insert_into(storepurchases::table)
        .values((
            storepurchases::tokenHash.eq(sha256_hex(&token)),
            storepurchases::accountId.eq(account),
            storepurchases::characterId.eq(&character),
            storepurchases::skuId.eq(sku),
            storepurchases::offerHash.eq(offer_hash(offer)),
            storepurchases::createdDate.eq(iso(now)),
            storepurchases::expiresDate.eq(iso(now + PURCHASE_TOKEN_MINUTES * 60 * 1000)),
            storepurchases::redeemedDate.eq(None::<String>),
        ))
        .execute(conn)?;
    Ok((token, character))
}

/// What a redeem did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redeemed {
    pub replayed: bool,
    pub sku: String,
    pub character: String,
    pub items: usize,
    pub entitlements: usize,
}

#[derive(Queryable)]
#[allow(non_snake_case, dead_code)]
struct Purchase {
    tokenHash: String,
    accountId: String,
    characterId: String,
    skuId: String,
    offerHash: String,
    createdDate: String,
    expiresDate: String,
    redeemedDate: Option<String>,
}

fn invalid_token() -> StoreFail {
    StoreFail::refused(400, "Invalid purchase token or currency")
}

/// The checks of `RedeemStorePurchase` that need no database: the currency and the token's form.
pub fn check_redeem(currency: &str, token: Option<&Value>) -> Result<(&'static str, String), StoreFail> {
    let currency = check_currency(currency).map_err(|_| invalid_token())?;
    match token.and_then(Value::as_str) {
        Some(token) if token.len() == 64 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => {
            Ok((currency, sha256_hex(token)))
        }
        _ => Err(invalid_token()),
    }
}

/// `RedeemStorePurchase`'s transaction (`BEGIN IMMEDIATE`): the grant and the receipt together. A
/// token already redeemed answers again and grants nothing (a retry after a lost answer).
pub fn redeem(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    currency: &str,
    token_hash: &str,
) -> Result<Redeemed, StoreFail> {
    let purchase: Option<Purchase> = storepurchases::table.find(token_hash).first(conn).optional()?;
    let purchase = match purchase {
        Some(purchase) if purchase.accountId == account => purchase,
        _ => return Err(StoreFail::refused(403, "Invalid purchase token")),
    };
    let owner: Option<String> = character_rows::table
        .filter(character_rows::characterId.eq(&purchase.characterId))
        .select(character_rows::userId)
        .first(conn)
        .optional()?;
    if owner.as_deref() != Some(account) {
        return Err(StoreFail::refused(
            403,
            "The purchase's character is no longer the account's",
        ));
    }
    if purchase.redeemedDate.is_some() {
        return Ok(Redeemed {
            replayed: true,
            sku: purchase.skuId,
            character: purchase.characterId,
            items: 0,
            entitlements: 0,
        });
    }
    if purchase.expiresDate < iso(ctx.now) {
        return Err(StoreFail::refused(410, "Purchase token expired"));
    }
    let offer = find_offer(ctx, &purchase.skuId, account)
        .ok_or_else(|| StoreFail::refused(409, "Offer is no longer sold; request a new token"))?;
    check_offer(&offer)?;
    if offer_hash(&offer) != purchase.offerHash {
        return Err(StoreFail::refused(409, "Offer changed; request a new token"));
    }
    let pricing = pricing(&offer)?;
    if pricing.currency != currency {
        return Err(StoreFail::refused(400, "Offer is not sold in that currency"));
    }
    // Two tokens issued before either was redeemed: only the first one buys this week's cell
    if catalog().middleman_offer_ids.contains(id(&offer))
        && bought_this_middleman_week(conn, ctx.now, account, id(&offer))?
    {
        return Err(StoreFail::refused(
            409,
            "Already bought this week; the Middleman restocks at the weekly reset",
        ));
    }
    if pricing.price > 0 && currency_balance(conn, &purchase.characterId, pricing.catalog_id)? < pricing.price {
        return Err(StoreFail::refused(409, "Insufficient balance"));
    }
    // Cosmetics are unlocks: something the character already holds is not granted again (overlapping
    // bundles, a second token). The repeatable consumable grants its full quantity every time.
    let held = held_catalog_ids(conn, Some(&purchase.characterId))?;
    if pricing.price > 0 && with_remaining(conn, ctx, account, &held, offer.clone())?["remaining"] == json!(0) {
        return Err(StoreFail::refused(409, "You already own everything this offer grants"));
    }
    let repeatable = is_repeatable(&offer);
    let to_grant: Vec<&Value> = items(&offer)
        .iter()
        .filter(|item| repeatable || !held.contains(catalog_id(item)))
        .collect();
    let stacked: Vec<Value> = to_grant
        .iter()
        .filter(|item| grant_kind(catalog_id(item)) == Some(GrantKind::Stacked))
        .map(|item| json!({"catalogId": item.get("catalogId"), "quantity": item.get("quantity")}))
        .collect();
    // Instanced items need an id; derived from the token, a retried redeem names the same instance
    let instanced: Vec<Value> = to_grant
        .iter()
        .filter(|item| grant_kind(catalog_id(item)) == Some(GrantKind::Instanced))
        .map(|item| {
            json!({
                "catalogId": item.get("catalogId"),
                "instanceId": &sha256_hex(&format!("{token_hash}:{}", catalog_id(item)))[..32],
                "updateVersion": 0
            })
        })
        .collect();
    let granted_items = stacked.len() + instanced.len();
    if granted_items > 0 || pricing.price > 0 {
        let request = TransactionRequest {
            user_id: Some(account.to_owned()),
            character_id: Some(json!(purchase.characterId)),
            transaction_id: Some(json!(format!("store:{token_hash}"))),
            add_instanced: Some(Value::Array(instanced)),
            add_stacked: Some(Value::Array(stacked)),
            remove_instanced: None,
            remove_stacked: (pricing.price > 0)
                .then(|| json!([{"catalogId": pricing.catalog_id, "quantity": pricing.price}])),
            save_instanced: None,
        };
        let source = json!(format!("store:{}", purchase.skuId));
        match inventory::apply_in_tx(conn, request, InventoryCaller::Store, Some(&source), &ctx.inventory) {
            Ok(_) => {}
            Err(Fail::Broken(message)) => return Err(StoreFail::Internal(message)),
            Err(_) => {
                return Err(StoreFail::refused(
                    409,
                    "The inventory could not complete this purchase",
                ));
            }
        }
    }
    let grants = grants(&offer);
    for grant in grants {
        entitlements::grant(
            conn,
            &ctx.entitlement_defaults,
            account,
            grant.get("name").and_then(Value::as_str).unwrap_or(""),
            js::safe_integer(grant.get("duration")).unwrap_or(0),
            &format!("store:{}", purchase.skuId),
        )?;
    }
    diesel::update(storepurchases::table.find(token_hash))
        .set(storepurchases::redeemedDate.eq(iso(ctx.now)))
        .execute(conn)?;
    Ok(Redeemed {
        replayed: false,
        sku: purchase.skuId,
        character: purchase.characterId,
        items: granted_items,
        entitlements: grants.len(),
    })
}

// ---- Balances (`controllers/store.ts`) ----

/// Held currency stacks: catalogue id and total quantity, in the order first seen.
pub type Held = Vec<(String, f64)>;

/// `GetNotesForUser`: the account's Notes; `None` for an account that doesn't exist (Node failed
/// the request).
pub fn notes_of(conn: &mut Conn, account: &str) -> QueryResult<Option<i64>> {
    users::table.find(account).select(users::notes).first(conn).optional()
}

/// The currencies the account's active character holds as inventory stacks (catalog id
/// CURRENCY_*), with the character's id; empty for an account without a character or inventory.
/// Balances from the inventory are Harmonic's idea (github.com/Harmonicrain/Undaunted 895f7c7).
pub fn held_currencies(conn: &mut Conn, account: &str) -> Result<(Option<String>, Held), StoreFail> {
    let Some(character) = active_character(conn, account)? else {
        return Ok((None, Vec::new()));
    };
    let row = inventory_row(conn, &character)?;
    let stacks: Value = serde_json::from_str(row.as_ref().map_or("[]", |r| r.1.as_str()))
        .map_err(|e| StoreFail::Internal(format!("SyntaxError: {e}")))?;
    let mut held: Held = Vec::new();
    // `for ... of` over a non-array: Node threw a TypeError
    let stacks = stacks
        .as_array()
        .ok_or_else(|| StoreFail::Internal("TypeError: stackedItems is not iterable".into()))?;
    for stack in stacks {
        let quantity = js::number(stack.get("quantity"));
        if let Some(id) = stack.get("catalogId").and_then(Value::as_str)
            && id.starts_with("CURRENCY_")
            && quantity.fract() == 0.0
            && quantity.abs() <= 9_007_199_254_740_991.0
        {
            match held.iter_mut().find(|(held_id, _)| held_id == id) {
                Some((_, total)) => *total += quantity,
                None => held.push((id.to_owned(), quantity)),
            }
        }
    }
    Ok((Some(character), held))
}

/// `OverlayHeldCurrencies`: every key of a balance sheet that names a held currency gets the held
/// quantity (CURRENCY_X is the stack CURRENCY_X, and so is id_currency_x). Returns the changed keys.
pub fn overlay_held(sheet: &mut Map<String, Value>, held: &[(String, f64)]) -> Vec<String> {
    let mut changed = Vec::new();
    for (key, value) in sheet.iter_mut() {
        let catalog_id = if key.starts_with("CURRENCY_") {
            Some(key.clone())
        } else {
            key.strip_prefix("id_currency_")
                .map(|rest| format!("CURRENCY_{}", rest.to_uppercase()))
        };
        if let Some((_, quantity)) = catalog_id.and_then(|c| held.iter().find(|(id, _)| *id == c)) {
            *value = number_value(*quantity);
            changed.push(key.clone());
        }
    }
    changed
}

/// The balance sheet of GET /balance before the held currencies, in the order the client got it.
pub fn balance_sheet(notes: i64) -> Map<String, Value> {
    let order: [(&str, Value); 52] = [
        ("id_currency_s20_coin", json!(0)),
        ("CURRENCY_GAUNTLET_COIN_FADED", json!(0)),
        ("CURRENCY_S20_COIN", json!(0)),
        ("CURRENCY_S18_COIN", json!(0)),
        ("id_currency_seasonal_coin", json!(0)),
        ("id_currency_s18_coin", json!(0)),
        ("id_currency_weapon_token", json!(25)),
        ("id_currency_celldust", json!(0)),
        ("id_currency_event_ramsgiving", json!(0)),
        ("CURRENCY_NOTES", json!(notes)),
        ("id_currency_event_frostfall", json!(0)),
        ("CURRENCY_EVENT_DARKHARVEST", json!(0)),
        ("CURRENCY_S19_COIN", json!(0)),
        ("id_currency_s16_coin", json!(0)),
        ("CURRENCY_S16_COIN", json!(0)),
        ("id_currency_gauntlet_coin", json!(0)),
        ("id_currency_s13_coin", json!(0)),
        ("CURRENCY_MARKS_STEEL", json!(0)),
        ("CURRENCY_S13_COIN", json!(0)),
        ("CURRENCY_EVENT_FROSTFALL", json!(0)),
        ("CURRENCY_GAUNTLET_COIN", json!(0)),
        ("id_currency_marks_steel", json!(0)),
        ("id_currency_rewardcache", json!(0)),
        ("CURRENCY_PRESTIGE", json!(0)),
        ("CURRENCY_SEASONAL_COIN", json!(0)),
        ("CURRENCY_REWARDCACHE", json!(0)),
        ("id_currency_token_exchange_speed_up", json!(0)),
        ("id_currency_event_springtide", json!(0)),
        ("CURRENCY_TOKEN_EXCHANGE_SPEED_UP", json!(0)),
        ("id_currency_gauntlet_coin_faded", json!(0)),
        ("CURRENCY_S15_COIN", json!(0)),
        ("CURRENCY_PLATINUM", json!(0)),
        ("id_currency_platinum", json!(0)),
        ("id_currency_s15_coin", json!(0)),
        ("id_currency_marks_gilded", json!(0)),
        ("id_currency_event_darkharvest", json!(0)),
        ("id_currency_event_saintsbond", json!(0)),
        ("CURRENCY_EVENT_SPRINGTIDE", json!(0)),
        ("id_currency_s19_coin", json!(0)),
        ("id_currency_notes", json!(notes)),
        ("id_currency_prestige", json!(0)),
        ("id_currency_s13_daily", json!(0)),
        ("CURRENCY_WEAPON_TOKEN", json!(25)),
        ("CURRENCY_MARKS_GILDED", json!(0)),
        ("CURRENCY_S13_DAILY", json!(0)),
        ("CURRENCY_CELLDUST", json!(0)),
        ("CURRENCY_S14_COIN", json!(0)),
        ("CURRENCY_EVENT_SAINTSBOND", json!(0)),
        ("CURRENCY_S17_COIN", json!(0)),
        ("id_currency_s14_coin", json!(0)),
        ("CURRENCY_EVENT_RAMSGIVING", json!(0)),
        ("id_currency_s17_coin", json!(0)),
    ];
    order.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

/// Whether GET /balance must alias Rams as Event01 for the pilot client.
pub fn rams_on_event_wire(settings: &Settings, account: &str) -> bool {
    rams_wire_currency(settings, "event_01", account) == "notes"
}

/// The held Rams as a balance value.
pub fn held_value(held: &[(String, f64)], catalog_id: &str) -> Value {
    number_value(held.iter().find(|(id, _)| id == catalog_id).map_or(0.0, |(_, q)| *q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_window_matches_storeclock() {
        // 2026-10-15T18:00:00.000Z is a Thursday reset
        let reset = js::date_parse("2026-10-15T18:00:00.000Z").unwrap();
        assert_eq!(weekly_window(reset).1, reset);
        assert_eq!(weekly_window(reset - 1).2, reset);
        assert_eq!(weekly_window(0), (-1, WEEK_EPOCH - WEEK_MS, WEEK_EPOCH));
    }

    #[test]
    fn grouped_numbers() {
        assert_eq!(grouped(750), "750");
        assert_eq!(grouped(5000), "5,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn currencies_and_aliases() {
        assert_eq!(check_currency("Rams").unwrap(), "notes");
        assert_eq!(check_currency("ID_CURRENCY_MARKS_STEEL").unwrap(), "markssteel");
        assert_eq!(check_currency("platinum").unwrap(), "platinum");
        assert!(check_currency("event01").is_err());
        assert!(check_currency("gold").is_err());
    }

    #[test]
    fn balance_sheet_has_every_key() {
        let sheet = balance_sheet(7);
        assert_eq!(sheet.len(), 52);
        assert_eq!(sheet["id_currency_notes"], json!(7));
        assert_eq!(sheet.keys().next().unwrap(), "id_currency_s20_coin");
        let mut sheet = sheet;
        let changed = overlay_held(&mut sheet, &[("CURRENCY_NOTES".into(), 40.0)]);
        assert_eq!(changed, ["CURRENCY_NOTES", "id_currency_notes"]);
    }
}
