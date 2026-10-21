//! `freestore.test.ts`: the free store (STORE=free), the Rams pilot and the purchase-token flow. Node's
//! `WithEnv` around part of a test is a store `Context` with other settings (`storekit::context`); a
//! setting that names an account uses an account made with that id (`player_named`). Node's
//! `SetStoreTokenLimitForTests` becomes ageing the account's tokens out of the 10-minute window, and
//! `SetExtraOffersForTests` (extra offers handed to the store) has no Rust hook: those cases are n/a.

use std::collections::HashSet;

use metagame::entitlements;
use metagame::store::{self as shop, Context, GrantKind, MAX_TOKENS_PER_WINDOW, Offer, RECEIPT_DAYS, StoreFail};
use serde_json::{Value, json};

use crate::common::{TestApp, req};
use crate::storekit::{context, instanced, offer_in, offers_in, player_named, redeem_in, token_in};

const SKU: &str = "single_armour_helm_savant";
const ITEM: &str = "AR_SAINTS19_ROMANTIC_HELM_01";
const PILOT: &str = "UID-rams-pilot";

fn catalog() -> &'static Value {
    metagame::vendor::store_catalog()
}

fn webstore() -> Vec<Offer> {
    catalog()["webstore"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_object().unwrap().clone())
        .collect()
}

fn sellable() -> Vec<Offer> {
    webstore()
        .into_iter()
        .filter(|o| !o["id"].as_str().unwrap().starts_with("single_dye_sheen_"))
        .collect()
}

fn every_offer() -> Vec<Value> {
    catalog()
        .as_object()
        .unwrap()
        .iter()
        .filter(|(key, value)| !key.starts_with('_') && value.is_array())
        .flat_map(|(_, value)| value.as_array().unwrap().clone())
        .collect()
}

/// The free store's settings, plus `extra`.
fn free(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![("STORE".to_owned(), "free".to_owned())];
    env.extend(extra.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    env
}

async fn app(extra: &[(&str, &str)]) -> TestApp {
    let env = free(extra);
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    TestApp::with_env(&pairs).await
}

fn ctx(app: &TestApp, extra: &[(&str, &str)]) -> Context {
    let env = free(extra);
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    context(app, &pairs)
}

/// The app's own store context.
fn own(app: &TestApp) -> Context {
    app.state.store_context()
}

async fn token(app: &TestApp, user: &str, sku: &str) -> Result<String, (u16, String)> {
    token_in(app, &own(app), user, "platinum", sku).await
}

async fn redeem(app: &TestApp, user: &str, purchase: &str) -> Result<bool, (u16, String)> {
    redeem_in(app, &own(app), user, "platinum", purchase).await
}

async fn buy(app: &TestApp, user: &str, sku: &str) {
    let purchase = token(app, user, sku).await.unwrap();
    redeem(app, user, &purchase).await.unwrap();
}

async fn remaining(app: &TestApp, user: &str, sku: &str) -> Value {
    offer_in(app, &own(app), user, sku).await.unwrap()["remaining"].clone()
}

fn refused_status<T: std::fmt::Debug>(result: Result<T, (u16, String)>) -> u16 {
    result.unwrap_err().0
}

/// Every storefront offer not owned yet when its turn comes, past the token limit: the account's
/// tokens are aged out of the window before each one.
async fn buy_every_offer(app: &TestApp, user: &str, offers: &[Offer]) {
    for found in offers {
        let id = found["id"].as_str().unwrap();
        if remaining(app, user, id).await == 1 {
            app.run(
                "UPDATE storepurchases SET createdDate = '2000-01-01T00:00:00.000Z' WHERE accountId = ?1",
                &[&user],
            );
            buy(app, user, id).await;
        }
    }
}

fn set_stacks(app: &TestApp, character: &str, stacked: &str) {
    app.run(
        "INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES (?1, '[]', ?2) \
         ON CONFLICT(characterId) DO UPDATE SET stackedItems = excluded.stackedItems",
        &[&character, &stacked],
    );
}

fn notes(app: &TestApp, character: &str, quantity: i64) {
    set_stacks(
        app,
        character,
        &json!([{"catalogId": "CURRENCY_NOTES", "quantity": quantity}]).to_string(),
    );
}

async fn has_pass(app: &TestApp, user: &str) -> bool {
    let (defaults, user) = (app.state.entitlement_defaults.clone(), user.to_owned());
    app.db(move |conn| entitlements::has_active(conn, &defaults, &user, "season09b_premium"))
        .await
}

async fn names(app: &TestApp, user: &str) -> Vec<String> {
    let (defaults, user) = (app.state.entitlement_defaults.clone(), user.to_owned());
    app.db(move |conn| entitlements::list(conn, &defaults, &user))
        .await
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_owned())
        .collect()
}

fn no_defaults() -> [(&'static str, &'static str); 1] {
    [("ENTITLEMENTS_DEFAULT", "")]
}

#[tokio::test(flavor = "multi_thread")]
async fn paid_competing_skus_recheck_funds_and_overlapping_bundles_never_duplicate_cosmetics() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let pilot = ctx(&app, &[("STORE_RAMS_TEST_ACCOUNT", &user)]);
    notes(&app, &character, 5000);
    let chest = token_in(&app, &pilot, &user, "notes", "single_armour_monk_chest")
        .await
        .unwrap();
    let kit = token_in(&app, &pilot, &user, "notes", "bundle_armour_monk")
        .await
        .unwrap();
    redeem_in(&app, &pilot, &user, "notes", &chest).await.unwrap();
    assert_eq!(refused_status(redeem_in(&app, &pilot, &user, "notes", &kit).await), 409);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 4250);
    let mut held = app.stacks(&character);
    held.iter_mut().find(|i| i["catalogId"] == "CURRENCY_NOTES").unwrap()["quantity"] = json!(5000);
    set_stacks(&app, &character, &Value::Array(held).to_string());
    redeem_in(&app, &pilot, &user, "notes", &kit).await.unwrap();
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 0);
    let cosmetics: Vec<Value> = app
        .stacks(&character)
        .into_iter()
        .filter(|i| i["catalogId"] != "CURRENCY_NOTES")
        .collect();
    assert_eq!(cosmetics.len(), 4);
    assert!(cosmetics.iter().all(|i| i["quantity"] == 1));
    redeem_in(&app, &pilot, &user, "notes", &kit).await.unwrap();
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_corrupt_and_invalid_rams_balances_fail_closed_without_repairing_player_inventory() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let pilot = ctx(&app, &[("STORE_RAMS_TEST_ACCOUNT", &user)]);
    assert_eq!(refused_status(token_in(&app, &pilot, &user, "notes", SKU).await), 409);
    set_stacks(&app, &character, "[]");
    let valid = json!([{"catalogId": "CURRENCY_NOTES", "quantity": 10000}]).to_string();
    let mut raws: Vec<String> = ["{", "null", "{}", "[]"].map(str::to_owned).to_vec();
    for quantity in [json!(-1), json!(1500.5), json!("bad"), Value::Null] {
        raws.push(json!([{"catalogId": "CURRENCY_NOTES", "quantity": quantity}]).to_string());
    }
    for raw in raws {
        set_stacks(&app, &character, &valid);
        let purchase = token_in(&app, &pilot, &user, "notes", SKU).await.unwrap();
        set_stacks(&app, &character, &raw);
        assert_eq!(
            refused_status(token_in(&app, &pilot, &user, "notes", SKU).await),
            409,
            "{raw}"
        );
        assert_eq!(
            refused_status(redeem_in(&app, &pilot, &user, "notes", &purchase).await),
            409,
            "{raw}"
        );
        let stored = app.row(
            "SELECT stackedItems FROM inventories WHERE characterId = ?1",
            &[&character],
        );
        assert_eq!(stored["stackedItems"], raw.as_str());
    }
    assert_eq!(
        app.count(
            "storepurchases",
            "accountId = ?1 AND redeemedDate IS NOT NULL",
            &[&user]
        ),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rams_pilot_charges_only_the_selected_account_rejects_wrong_currency_replay_and_duplicate_charges() {
    let app = app(&[]).await;
    let (a, a_character) = app.make_player().await;
    let (b, _) = app.make_player().await;
    let pilot = ctx(&app, &[("STORE_RAMS_TEST_ACCOUNT", &a)]);
    let balance = || app.stack_quantity(&a_character, "CURRENCY_NOTES");
    let a_offer = offer_in(&app, &pilot, &a, SKU).await.unwrap();
    assert_eq!(
        (a_offer["notesPrice"].clone(), a_offer.get("platinumPrice")),
        (json!(750), None)
    );
    assert_eq!(offer_in(&app, &pilot, &b, SKU).await.unwrap()["platinumPrice"], 0);
    assert_eq!(refused_status(token_in(&app, &pilot, &a, "platinum", SKU).await), 400);
    notes(&app, &a_character, 1500);
    let first = token_in(&app, &pilot, &a, "notes", SKU).await.unwrap();
    let second = token_in(&app, &pilot, &a, "notes", SKU).await.unwrap();
    assert_eq!(refused_status(redeem_in(&app, &pilot, &b, "notes", &first).await), 403);
    redeem_in(&app, &pilot, &a, "notes", &first).await.unwrap();
    assert_eq!(balance(), 750);
    assert_eq!(app.stack_quantity(&a_character, ITEM), 1);
    redeem_in(&app, &pilot, &a, "notes", &first).await.unwrap();
    assert_eq!(balance(), 750);
    assert_eq!(refused_status(redeem_in(&app, &pilot, &a, "notes", &second).await), 409);
    assert_eq!(balance(), 750);
}

#[tokio::test(flavor = "multi_thread")]
async fn native_rams_wire_purchase_charges_rams_once_and_rejects_another_account_using_the_bridge() {
    let app = app(&[("STORE_RAMS_TEST_ACCOUNT", PILOT), ("STORE_RAMS_EVENT_TRANSPORT", "1")]).await;
    let character = player_named(&app, PILOT).await;
    let (b, _) = app.make_player().await;
    notes(&app, &character, 1500);
    let offer = app.get(&format!("/product/sku/{SKU}"), req().bearer(PILOT)).await.json;
    assert_eq!(
        (offer["event01Price"].clone(), offer.get("notesPrice")),
        (json!(750), None)
    );
    assert!(offer["tags"].as_array().unwrap().contains(&json!("rams-store-pilot")));
    let balance = app.get("/balance", req().bearer(PILOT)).await.json;
    assert_eq!(
        (balance["CURRENCY_NOTES"].clone(), balance["CURRENCY_EVENT_01"].clone()),
        (json!(1500), json!(1500))
    );
    assert_eq!(
        app.get("/balance", req().bearer(&b))
            .await
            .json
            .get("CURRENCY_EVENT_01"),
        None
    );
    assert_eq!(
        app.get(&format!("/token/event01/{SKU}"), req().bearer(&b)).await.status,
        400
    );
    let issued = app.get(&format!("/token/event_01/{SKU}"), req().bearer(PILOT)).await;
    assert_eq!(issued.status, 200);
    let purchase = issued.json["purchaseToken"].as_str().unwrap().to_owned();
    for _ in 0..2 {
        assert_eq!(
            app.post(&format!("/notification/event_01?token={purchase}"), req().bearer(PILOT))
                .await
                .status,
            204
        );
    }
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 750);
    assert_eq!(app.stack_quantity(&character, ITEM), 1);
    assert_eq!(
        app.get("/balance", req().bearer(PILOT)).await.json["CURRENCY_EVENT_01"],
        750
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn production_rams_shop_requires_updated_clients_and_charges_every_account_through_the_native_bridge() {
    let app = app(&[
        ("STORE_RAMS", "1"),
        ("STORE_RAMS_EVENT_TRANSPORT", "1"),
        ("STORE_WEEKLY_ROTATION", "1"),
    ])
    .await;
    for _ in 0..2 {
        let (user, character) = app.make_player().await;
        notes(&app, &character, 100_000);
        for path in [
            "/product/skus/public?requiredTags=webstore".to_owned(),
            format!("/product/sku/{SKU}"),
            format!("/token/event01/{SKU}"),
        ] {
            assert_eq!(app.get(&path, req().bearer(&user)).await.status, 426, "{path}");
        }
        assert_ne!(
            app.get("/product/skus/public?requiredTags=ladyluckstore", req().bearer(&user))
                .await
                .status,
            426
        );
        let listed = app
            .get(
                "/product/skus/public?requiredTags=webstore&shop_protocol=rams-v1",
                req().bearer(&user),
            )
            .await;
        assert_eq!(listed.status, 200);
        let listed = listed.json.as_array().unwrap().clone();
        let ids: Vec<Value> = listed.iter().map(|o| o["id"].clone()).collect();
        let direct: Vec<Value> = offers_in(&app, &own(&app), &user, "webstore")
            .await
            .iter()
            .map(|o| o["id"].clone())
            .collect();
        assert_eq!(ids, direct);
        let offer = listed
            .iter()
            .find(|o| {
                o["event01Price"].as_i64().is_some_and(|p| p > 0)
                    && o["items"].as_array().is_some_and(|i| !i.is_empty())
            })
            .unwrap();
        let (id, price) = (offer["id"].as_str().unwrap(), offer["event01Price"].as_i64().unwrap());
        let detail = app
            .get(&format!("/product/sku/{id}?shop_protocol=rams-v1"), req().bearer(&user))
            .await;
        assert_eq!(
            (detail.status, detail.json["event01Price"].clone()),
            (200, json!(price))
        );
        let issued = app
            .get(
                &format!("/token/event01/{id}?shop_protocol=rams-v1"),
                req().bearer(&user),
            )
            .await;
        assert_eq!(issued.status, 200);
        let purchase = issued.json["purchaseToken"].as_str().unwrap();
        let path = format!("/notification/event01?token={purchase}&shop_protocol=rams-v1");
        assert_eq!(app.post(&path, req().bearer(&user)).await.status, 204);
        let balance = app.get("/balance", req().bearer(&user)).await.json;
        assert_eq!(balance["CURRENCY_NOTES"], 100_000 - price);
        assert_eq!(balance["CURRENCY_EVENT_01"], 100_000 - price);
        let item = &offer["items"][0]["catalogId"];
        assert!(
            app.stacks(&character).iter().any(|i| &i["catalogId"] == item)
                || instanced(&app, &character).iter().any(|i| &i["catalogId"] == item)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn weekly_shop_rejects_inactive_offers_and_outstanding_tokens_without_taking_rams() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let pilot = ctx(&app, &[("STORE_RAMS_TEST_ACCOUNT", &user)]);
    let weekly = ctx(
        &app,
        &[("STORE_RAMS_TEST_ACCOUNT", &user), ("STORE_WEEKLY_ROTATION", "1")],
    );
    notes(&app, &character, 100_000);
    let all = offers_in(&app, &pilot, &user, "webstore").await;
    let active: HashSet<Value> = offers_in(&app, &weekly, &user, "webstore")
        .await
        .iter()
        .map(|o| o["id"].clone())
        .collect();
    let hidden = all
        .iter()
        .find(|o| o.get("notesPrice").is_some_and(Value::is_number) && !active.contains(&o["id"]))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        refused_status(token_in(&app, &weekly, &user, "notes", &hidden).await),
        404
    );
    let purchase = token_in(&app, &pilot, &user, "notes", &hidden).await.unwrap();
    assert_eq!(
        refused_status(redeem_in(&app, &weekly, &user, "notes", &purchase).await),
        409
    );
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 100_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn rams_pilot_checks_balance_again_when_redeeming_and_rejects_tokens_after_pricing_changes() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let pilot = ctx(&app, &[("STORE_RAMS_TEST_ACCOUNT", &user)]);
    notes(&app, &character, 749);
    assert_eq!(refused_status(token_in(&app, &pilot, &user, "notes", SKU).await), 409);
    notes(&app, &character, 750);
    let purchase = token_in(&app, &pilot, &user, "notes", SKU).await.unwrap();
    notes(&app, &character, 0);
    assert_eq!(
        refused_status(redeem_in(&app, &pilot, &user, "notes", &purchase).await),
        409
    );
    assert_eq!(app.stack_quantity(&character, ITEM), 0);
    notes(&app, &character, 750);
    // Without STORE_RAMS_TEST_ACCOUNT the offer is free again: the token's price no longer matches
    assert_eq!(
        refused_status(redeem_in(&app, &own(&app), &user, "notes", &purchase).await),
        409
    );
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 750);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_free_grant_persists_reads_as_owned_and_retries_or_new_tokens_do_not_duplicate_it() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let first = token(&app, &user, SKU).await.unwrap();
    let second = token(&app, &user, SKU).await.unwrap();
    redeem(&app, &user, &first).await.unwrap();
    redeem(&app, &user, &first).await.unwrap();
    redeem(&app, &user, &second).await.unwrap();
    assert_eq!(
        refused_status(token(&app, &user, SKU).await),
        409,
        "no new token for what the player owns"
    );
    assert_eq!(app.stack_quantity(&character, ITEM), 1);
    let listed = offers_in(&app, &own(&app), &user, "webstore").await;
    assert_eq!(listed.iter().find(|o| o["id"] == SKU).unwrap()["remaining"], 0);
    // A separate database connection sees the committed item and both receipts
    assert_eq!(app.stack_quantity(&character, ITEM), 1);
    assert_eq!(
        app.count(
            "storepurchases",
            "accountId = ?1 AND redeemedDate IS NOT NULL",
            &[&user]
        ),
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_overlapping_bundle_and_single_item_unlock_each_cosmetic_once() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    buy(&app, &user, "single_armour_monk_chest").await;
    buy(&app, &user, "bundle_armour_monk").await;
    let held = app.stacks(&character);
    assert_eq!(held.len(), 4);
    assert!(held.iter().all(|i| i["quantity"] == 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn another_account_s_token_a_wrong_currency_an_unknown_sku_and_a_malformed_token_are_refused() {
    let app = app(&[]).await;
    let (a, a_character) = app.make_player().await;
    let (b, b_character) = app.make_player().await;
    let purchase = token(&app, &a, SKU).await.unwrap();
    assert_eq!(refused_status(redeem(&app, &b, &purchase).await), 403);
    assert_eq!(
        refused_status(redeem_in(&app, &own(&app), &a, "notes", &purchase).await),
        400
    );
    assert_eq!(refused_status(token_in(&app, &own(&app), &a, "notes", SKU).await), 400);
    assert_eq!(refused_status(token(&app, &a, "made_up").await), 404);
    assert_eq!(refused_status(redeem(&app, &a, "made_up").await), 400);
    assert_eq!(
        (
            app.stack_quantity(&a_character, ITEM),
            app.stack_quantity(&b_character, ITEM)
        ),
        (0, 0)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_token_and_a_changed_offer_cannot_grant() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let expired = token(&app, &user, SKU).await.unwrap();
    app.run(
        "UPDATE storepurchases SET expiresDate = '2000-01-01T00:00:00.000Z' WHERE accountId = ?1",
        &[&user],
    );
    assert_eq!(refused_status(redeem(&app, &user, &expired).await), 410);
    let changed = token(&app, &user, SKU).await.unwrap();
    app.run(
        "UPDATE storepurchases SET offerHash = 'changed' WHERE accountId = ?1",
        &[&user],
    );
    assert_eq!(refused_status(redeem(&app, &user, &changed).await), 409);
    assert_eq!(app.stack_quantity(&character, ITEM), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_grant_its_item_log_and_the_receipt_roll_back_together_if_the_receipt_cannot_be_written() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let purchase = token(&app, &user, SKU).await.unwrap();
    // A TEMP trigger would only see its own connection; the store writes through another
    app.exec("CREATE TRIGGER test_store_abort BEFORE UPDATE ON storepurchases BEGIN SELECT RAISE(ABORT, 'test failure'); END");
    let failed = std::panic::AssertUnwindSafe(redeem(&app, &user, &purchase));
    let outcome = futures_util::FutureExt::catch_unwind(failed).await;
    app.exec("DROP TRIGGER test_store_abort");
    let message = outcome.expect_err("the redeem fails");
    let text = message.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("test failure"), "{text}");
    assert_eq!(app.stack_quantity(&character, ITEM), 0);
    assert_eq!(app.count("inventorylog", "characterId = ?1", &[&character]), 0);
    assert_eq!(app.count("inventorytransactions", "characterId = ?1", &[&character]), 0);
    redeem(&app, &user, &purchase).await.unwrap();
    assert_eq!(app.stack_quantity(&character, ITEM), 1);
}

async fn save(app: &TestApp, character: &str, user: &str, version: i64) {
    let (history, c, u) = (app.state.history, character.to_owned(), user.to_owned());
    let saved = app
        .db(move |conn| {
            metagame::characters::update(conn, &history, Some(&c), Some(&u), Some(&json!("{}")), version, "test")
        })
        .await;
    assert!(saved.is_ok(), "{saved:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn buys_for_the_character_played_last_never_an_arbitrary_one_and_not_for_a_character_moved_away() {
    let app = app(&[]).await;
    let (user, first) = app.make_player().await;
    let u = user.clone();
    let (second, third) = app
        .db(move |conn| {
            let second = metagame::characters::create(conn, &u, "Second")?.characterId;
            let third = metagame::characters::create(conn, &u, "Third")?.characterId;
            Ok((second, third))
        })
        .await;
    // The second character is saved last
    save(&app, &first, &user, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    save(&app, &second, &user, 1).await;
    buy(&app, &user, SKU).await;
    assert_eq!(
        [
            app.stack_quantity(&second, ITEM),
            app.stack_quantity(&first, ITEM),
            app.stack_quantity(&third, ITEM)
        ],
        [1, 0, 0]
    );
    // Now the first one is played
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    save(&app, &first, &user, 2).await;
    let purchase = token(&app, &user, "single_armour_monk_chest").await.unwrap();
    let row = app.row(
        "SELECT characterId FROM storepurchases WHERE accountId = ?1 AND skuId = 'single_armour_monk_chest'",
        &[&user],
    );
    assert_eq!(row["characterId"], first.as_str());
    // ... and moved to another account before the purchase is confirmed
    let (other, _) = app.make_player().await;
    app.run(
        "UPDATE characters SET userId = ?1 WHERE characterId = ?2",
        &[&other, &first],
    );
    assert_eq!(refused_status(redeem(&app, &user, &purchase).await), 403);
    assert!(app.stacks(&first).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_account_without_a_character_gets_no_token_409() {
    let app = app(&[]).await;
    app.exec("INSERT INTO users (userId, name, notes) VALUES ('UID-store-nochar', 'NoChar', 0)");
    assert_eq!(refused_status(token(&app, "UID-store-nochar", SKU).await), 409);
    assert_eq!(app.count("storepurchases", "accountId = 'UID-store-nochar'", &[]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn tokens_that_expired_without_being_redeemed_are_removed_when_a_token_is_issued() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    token(&app, &user, SKU).await.unwrap();
    buy(&app, &user, "single_armour_monk_chest").await;
    app.run(
        "UPDATE storepurchases SET expiresDate = '2000-01-01T00:00:00.000Z' WHERE accountId = ?1",
        &[&user],
    );
    token(&app, &user, "bundle_armour_iron").await.unwrap();
    let mut rows: Vec<Value> = app
        .rows(
            "SELECT skuId, redeemedDate IS NOT NULL AS redeemed FROM storepurchases WHERE accountId = ?1",
            &[&user],
        )
        .iter()
        .map(|r| json!([r["skuId"], r["redeemed"]]))
        .collect();
    rows.sort_by_key(Value::to_string);
    assert_eq!(
        rows,
        [json!(["bundle_armour_iron", 0]), json!(["single_armour_monk_chest", 1])],
        "the redeemed receipt is kept, the unused expired token removed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offer_outside_the_webstore_tag_is_found_rather_than_reported_unknown() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    let purchase = token(&app, &user, "season09b_premium").await.unwrap();
    assert!(regex::Regex::new("^[a-f0-9]{64}$").unwrap().is_match(&purchase));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_sku_is_still_refused() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    assert_eq!(refused_status(token(&app, &user, "not_a_real_sku").await), 404);
    assert!(matches!(
        offer_in(&app, &own(&app), &user, "not_a_real_sku").await,
        Err(StoreFail::Refused(404, _))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_single_offer_lookup_finds_a_sku_outside_the_webstore_tag() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    let found = offer_in(&app, &own(&app), &user, "season09b_premium").await.unwrap();
    assert_eq!(
        (found["id"].clone(), found["remaining"].clone()),
        (json!("season09b_premium"), json!(1)),
        "not yet owned"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_entitlement_offer_reads_as_owned_once_granted() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    buy(&app, &user, "season09b_premium").await;
    assert_eq!(remaining(&app, &user, "season09b_premium").await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn redeeming_the_pass_grants_the_entitlement() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    assert!(!has_pass(&app, &user).await, "starts without it");
    buy(&app, &user, "season09b_premium").await;
    assert!(has_pass(&app, &user).await);
    let source = app.row(
        "SELECT source FROM entitlements WHERE accountId = ?1 AND name = 'season09b_premium'",
        &[&user],
    );
    assert_eq!(source["source"], "store:season09b_premium");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_entitlementsv2_matches_the_1_4_4_queryentitlements_response_contract() {
    let app = app(&no_defaults()).await;
    let (a, _) = app.make_player().await;
    let (other, _) = app.make_player().await;
    buy(&app, &a, "season09b_premium").await;
    for (user, expected) in [(&a, vec!["season09b_premium"]), (&other, vec![])] {
        let reply = app.get("/entitlementsv2", req().bearer(user)).await;
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.json.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["entitlements"]
        );
        let list = reply.json["entitlements"].as_array().unwrap();
        assert_eq!(
            list.iter().map(|e| e["name"].as_str().unwrap()).collect::<Vec<_>>(),
            expected
        );
        for entry in list {
            let mut keys: Vec<&String> = entry.as_object().unwrap().keys().collect();
            keys.sort();
            assert_eq!(keys, ["activatedDate", "duration", "name"]);
            assert_eq!(entry["duration"], 0);
            assert!(metagame::js::date_parse(entry["activatedDate"].as_str().unwrap()).is_some());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_purchase_of_the_same_entitlement_does_not_duplicate_it() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    let first = token(&app, &user, "season09b_premium").await.unwrap();
    let second = token(&app, &user, "season09b_premium").await.unwrap();
    redeem(&app, &user, &first).await.unwrap();
    redeem(&app, &user, &second).await.unwrap();
    assert_eq!(names(&app, &user).await, ["season09b_premium"]);
    assert_eq!(refused_status(token(&app, &user, "season09b_premium").await), 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn redeeming_the_same_token_twice_is_idempotent() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    let purchase = token(&app, &user, "season09b_premium").await.unwrap();
    assert_eq!(redeem(&app, &user, &purchase).await, Ok(false));
    assert_eq!(redeem(&app, &user, &purchase).await, Ok(true));
    assert_eq!(names(&app, &user).await, ["season09b_premium"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_entitlement_can_be_bought_again() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    buy(&app, &user, "single_dye_hairtint_01").await;
    let (defaults, u) = (app.state.entitlement_defaults.clone(), user.clone());
    app.db(move |conn| entitlements::revoke(conn, &defaults, &u, "ent_cchd_hp07a_01"))
        .await;
    assert_eq!(remaining(&app, &user, "single_dye_hairtint_01").await, 1);
    buy(&app, &user, "single_dye_hairtint_01").await;
    assert_eq!(names(&app, &user).await, ["ent_cchd_hp07a_01"]);
    assert_eq!(remaining(&app, &user, "single_dye_hairtint_01").await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_elite_hunt_pass_offer_reads_as_owned_through_the_default_elite_every_account_has() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    assert_eq!(remaining(&app, &user, "season09b_premium").await, 0);
    let listed: Vec<Value> = offers_in(&app, &own(&app), &user, "season09b_pass")
        .await
        .iter()
        .map(|o| json!([o["id"], o["remaining"]]))
        .collect();
    assert_eq!(listed, [json!(["season09b_premium", 0])]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cosmetic_purchase_is_recorded_in_the_inventory_ledger_and_the_item_log_as_the_store() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    assert_eq!(app.count("inventorytransactions", "characterId = ?1", &[&character]), 0);
    buy(&app, &user, "bundle_armour_iron").await;
    assert_eq!(
        app.count("inventorytransactions", "characterId = ?1", &[&character]),
        1,
        "the grant went through the inventory core"
    );
    let log = app.rows(
        "SELECT caller, source, operation, transactionId, quantityChange FROM inventorylog WHERE characterId = ?1",
        &[&character],
    );
    assert!(!log.is_empty());
    let id = regex::Regex::new("^store:[0-9a-f]{64}$").unwrap();
    for row in &log {
        assert_eq!(
            json!([row["caller"], row["source"], row["operation"], row["quantityChange"]]),
            json!(["store", "store:bundle_armour_iron", "add", 1]),
            "{row}"
        );
        assert!(id.is_match(row["transactionId"].as_str().unwrap()), "{row}");
    }
    assert!(!app.stacks(&character).is_empty(), "items were granted");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_already_owned_cosmetic_is_not_granted_a_second_copy() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let first = token(&app, &user, "bundle_armour_iron").await.unwrap();
    let second = token(&app, &user, "bundle_armour_iron").await.unwrap();
    redeem(&app, &user, &first).await.unwrap();
    redeem(&app, &user, &second).await.unwrap();
    assert_eq!(refused_status(token(&app, &user, "bundle_armour_iron").await), 409);
    for stack in app.stacks(&character) {
        assert_eq!(
            stack["quantity"], 1,
            "{} should be held once, cosmetics are unlocks",
            stack["catalogId"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_storefront_offer_can_be_bought_and_redeemed() {
    let app = app(&[("STORE_REPEATABLE_TOKENS", "1")]).await;
    let (user, character) = app.make_player().await;
    buy_every_offer(&app, &user, &sellable()).await;
    let ids: HashSet<Value> = app
        .stacks(&character)
        .iter()
        .chain(&instanced(&app, &character))
        .map(|i| i["catalogId"].clone())
        .collect();
    for offer in sellable() {
        for item in offer["items"].as_array().into_iter().flatten() {
            assert!(
                ids.contains(&item["catalogId"]),
                "{} should have granted {}",
                offer["id"],
                item["catalogId"]
            );
        }
    }
    // Every cosmetic reads as owned; the consumable bundle stays on sale
    for listed in offers_in(&app, &own(&app), &user, "webstore").await {
        let items = listed["items"].as_array().cloned().unwrap_or_default();
        let repeatable = !items.is_empty() && items.iter().all(|i| i["catalogId"] == "TOKEN_BOUNTY_DRAFT_PREMIUM");
        assert_eq!(
            listed["remaining"],
            if repeatable { 1 } else { 0 },
            "{} ownership",
            listed["id"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bounty_tokens_can_be_bought_repeatedly_and_each_purchase_adds_the_full_bundle() {
    let app = app(&[("STORE_REPEATABLE_TOKENS", "1")]).await;
    let (user, character) = app.make_player().await;
    let sku = "bundle_currency_bounty_small";
    for _ in 0..2 {
        let purchase = token(&app, &user, sku).await.unwrap();
        redeem(&app, &user, &purchase).await.unwrap();
        redeem(&app, &user, &purchase).await.unwrap(); // a retried redeem adds nothing
    }
    assert_eq!(app.stack_quantity(&character, "TOKEN_BOUNTY_DRAFT_PREMIUM"), 40);
    assert_eq!(remaining(&app, &user, sku).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_store_repeatable_tokens_the_bounty_token_bundle_is_neither_listed_nor_sold() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let sku = "bundle_currency_bounty_small";
    let listed = offers_in(&app, &own(&app), &user, "webstore").await;
    assert!(!listed.iter().any(|o| o["id"] == sku));
    assert_eq!(listed.len(), sellable().len() - 1);
    assert_eq!(refused_status(token(&app, &user, sku).await), 404);
    assert!(matches!(
        offer_in(&app, &own(&app), &user, sku).await,
        Err(StoreFail::Refused(404, _))
    ));
    // A token issued while it was on sale is not redeemed after it was taken off
    let early = token_in(
        &app,
        &ctx(&app, &[("STORE_REPEATABLE_TOKENS", "1")]),
        &user,
        "platinum",
        sku,
    )
    .await
    .unwrap();
    assert_eq!(refused_status(redeem(&app, &user, &early).await), 409);
    assert_eq!(app.stack_quantity(&character, "TOKEN_BOUNTY_DRAFT_PREMIUM"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn hair_tint_offers_grant_the_entitlements_the_client_s_own_tables_name() {
    let app = app(&no_defaults()).await;
    let (user, _) = app.make_player().await;
    let expected = [
        ("single_dye_hairtint_01", vec!["ent_cchd_hp07a_01"]),
        (
            "bundle_hairtint_springtime",
            vec![
                "ent_cchd_hp09a_assassins_01",
                "ent_cchd_hp09a_assassins_02",
                "ent_cchd_hp09a_assassins_03",
                "ent_cchd_hp09a_assassins_04",
                "ent_cchd_hp09a_assassins_05",
            ],
        ),
    ];
    for (sku, wanted) in expected {
        buy(&app, &user, sku).await;
        let held = names(&app, &user).await;
        for name in wanted {
            assert!(held.iter().any(|h| h == name), "{sku} should grant {name}");
        }
        assert_eq!(remaining(&app, &user, sku).await, 0, "{sku} should read as owned");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn store_grants_land_stacked_or_instanced_exactly_as_the_catalogue_flag_says() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let offers: Vec<Offer> = sellable()
        .into_iter()
        .filter(|o| o["id"] != "bundle_currency_bounty_small")
        .collect();
    buy_every_offer(&app, &user, &offers).await;
    let held = instanced(&app, &character);
    let stacked: HashSet<Value> = app.stacks(&character).iter().map(|i| i["catalogId"].clone()).collect();
    let instances: HashSet<Value> = held.iter().map(|i| i["catalogId"].clone()).collect();
    let kinds = metagame::vendor::store_item_kinds();
    for offer in &offers {
        for item in offer["items"].as_array().into_iter().flatten() {
            let id = item["catalogId"].as_str().unwrap();
            let expected = kinds[id].as_str().unwrap_or_default();
            assert!(
                expected == "stacked" || expected == "instanced",
                "{id} has no recorded kind"
            );
            let kind = if expected == "stacked" {
                GrantKind::Stacked
            } else {
                GrantKind::Instanced
            };
            assert_eq!(shop::grant_kind(id), Some(kind));
            let (right, wrong) = if expected == "stacked" {
                (&stacked, &instances)
            } else {
                (&instances, &stacked)
            };
            assert!(right.contains(&item["catalogId"]), "{id} should be {expected}");
            assert!(!wrong.contains(&item["catalogId"]), "{id} granted both ways");
        }
    }
    // An instanced grant carries an id of its own, 32 hex characters, and a version of 0
    let id = regex::Regex::new("^[0-9a-f]{32}$").unwrap();
    assert!(
        held.iter()
            .all(|i| i["instanceId"].as_str().is_some_and(|s| id.is_match(s)) && i["updateVersion"] == 0)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_token_for_an_offer_the_player_already_owns_409_over_http_too_the_bounty_token_bundle_is_never_owned() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    buy(&app, &user, SKU).await;
    assert_eq!(refused_status(token(&app, &user, SKU).await), 409);
    let refused = app.get(&format!("/token/platinum/{SKU}"), req().bearer(&user)).await;
    assert_eq!(
        (refused.status, refused.json),
        (
            409,
            json!({"code": "409", "message": "You already own everything this offer grants"})
        )
    );
    assert_eq!(
        app.count("storepurchases", "accountId = ?1", &[&user]),
        1,
        "only the first token was stored"
    );
    let repeatable = ctx(&app, &[("STORE_REPEATABLE_TOKENS", "1")]);
    let sku = "bundle_currency_bounty_small";
    let purchase = token_in(&app, &repeatable, &user, "platinum", sku).await.unwrap();
    redeem_in(&app, &repeatable, &user, "platinum", &purchase)
        .await
        .unwrap();
    let again = token_in(&app, &repeatable, &user, "platinum", sku).await.unwrap();
    assert!(regex::Regex::new("^[a-f0-9]{64}$").unwrap().is_match(&again));
}

#[tokio::test(flavor = "multi_thread")]
async fn at_most_max_tokens_per_window_tokens_per_account_in_10_minutes() {
    let app = app(&[]).await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    for _ in 0..MAX_TOKENS_PER_WINDOW {
        token(&app, &a, SKU).await.unwrap();
    }
    assert_eq!(refused_status(token(&app, &a, SKU).await), 409);
    assert_eq!(
        app.count("storepurchases", "accountId = ?1", &[&a]),
        MAX_TOKENS_PER_WINDOW
    );
    assert!(token(&app, &b, SKU).await.is_ok(), "per account");
    let moved = metagame::time::iso(metagame::time::now_ms() - 10 * 60 * 1000);
    app.run(
        "UPDATE storepurchases SET createdDate = ?1 WHERE accountId = ?2",
        &[&moved, &a],
    );
    assert!(token(&app, &a, SKU).await.is_ok(), "the window has moved on");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_removes_tokens_never_redeemed_and_receipts_older_than_receipt_days_days_a_recent_receipt_stays() {
    let app = app(&[]).await;
    let (a, a_character) = app.make_player().await;
    let (b, _) = app.make_player().await;
    let old = token(&app, &a, SKU).await.unwrap();
    redeem(&app, &a, &old).await.unwrap();
    buy(&app, &a, "single_armour_monk_chest").await;
    token(&app, &b, SKU).await.unwrap();
    let long_ago = metagame::time::iso(metagame::time::now_ms() - (RECEIPT_DAYS + 1) * 24 * 60 * 60 * 1000);
    app.run(
        "UPDATE storepurchases SET redeemedDate = ?1 WHERE accountId = ?2 AND skuId = ?3",
        &[&long_ago, &a, &SKU],
    );
    app.run(
        "UPDATE storepurchases SET expiresDate = '2000-01-01T00:00:00.000Z' WHERE accountId = ?1",
        &[&b],
    );
    let (expired, receipts) = app.db(|conn| shop::prune(conn, metagame::time::now_ms())).await;
    assert!(expired >= 1 && receipts >= 1, "{expired} {receipts}");
    let left: Vec<Value> = app
        .rows("SELECT skuId FROM storepurchases WHERE accountId = ?1", &[&a])
        .iter()
        .map(|r| r["skuId"].clone())
        .collect();
    assert_eq!(left, [json!("single_armour_monk_chest")]);
    assert_eq!(app.count("storepurchases", "accountId = ?1", &[&b]), 0);
    assert_eq!(app.stack_quantity(&a_character, ITEM), 1, "the grant stays");
    let source = format!("store:{SKU}");
    assert_eq!(
        app.count(
            "inventorylog",
            "characterId = ?1 AND source = ?2",
            &[&a_character, &source]
        ),
        1,
        "and so does its log"
    );
}

#[test]
fn offer_ids_are_unique() {
    let ids: Vec<Value> = every_offer().iter().map(|o| o["id"].clone()).collect();
    let unique: HashSet<&Value> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "duplicate offer ids break the single-offer lookup"
    );
}

#[test]
fn every_storefront_offer_id_is_a_store_item_table_row_with_2_1_art_so_its_tile_is_neither_blank_nor_stretched() {
    let art: Value = serde_json::from_str(include_str!(
        "../../../../../legacy/UndauntedMetagame/test/data/store_art_skus.json"
    ))
    .unwrap();
    let rows: HashSet<&Value> = art["skus"].as_array().unwrap().iter().collect();
    let missing: Vec<Value> = webstore()
        .iter()
        .filter(|o| !rows.contains(&o["id"]))
        .map(|o| o["id"].clone())
        .collect();
    assert_eq!(missing, Vec::<Value>::new(), "offers without store art");
}

#[test]
fn no_store_tab_before_a_populated_tab_is_left_empty() {
    let tags: HashSet<Value> = webstore()
        .iter()
        .flat_map(|o| o["tags"].as_array().unwrap().clone())
        .collect();
    // The tab bar indexes the full tab list including hidden (empty) tabs, so an empty tab ahead of a
    // populated one shifts every later tab: EMOTES opened the weapons page
    for required in [
        "feature",
        "your_offers",
        "supplies_boost",
        "skin_weapon_ac",
        "skin_weapon_eb",
        "skin_weapon_dp",
        "skin_weapon_ih",
        "skin_weapon_ga",
        "skin_weapon_ms",
        "skin_weapon_cb",
        "skin_armour",
        "skin_lantern",
        "social_arrival",
        "social_emote",
        "dye_armour",
        "personality_stylekit",
        "personality_character",
        "personality_flaresigil",
        "personality_fabric",
    ] {
        assert!(tags.contains(&json!(required)), "{required} must not be empty");
    }
}

#[test]
fn sells_no_rank_skip_and_every_offer_is_free() {
    assert_eq!(catalog()["season09b_rank"], json!([]));
    let offers = every_offer();
    assert!(offers.iter().all(|o| o["platinumPrice"] == 0));
    assert!(
        !Value::Array(offers).to_string().contains("CURRENCY_"),
        "no offer grants a currency"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn authenticates_ignores_asked_for_grants_and_prices_and_follows_the_client_s_token_flow() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    assert_eq!(app.get(&format!("/token/platinum/{SKU}"), req()).await.status, 401);
    let issued = app
        .get(
            &format!("/token/platinum/{SKU}?price=-100&quantity=999&catalogId=anything"),
            req().bearer(&user),
        )
        .await;
    assert_eq!(issued.status, 200);
    let purchase = issued.json["purchaseToken"].as_str().unwrap().to_owned();
    assert!(regex::Regex::new("^[a-f0-9]{64}$").unwrap().is_match(&purchase));
    let body = json!({"quantity": 999, "price": -100, "catalogId": "anything"});
    let path = format!("/notification/platinum?token={purchase}");
    let first = app.post(&path, req().bearer(&user).json(body.clone())).await;
    let second = app.post(&path, req().bearer(&user).json(body)).await;
    assert_eq!((first.status, first.text.as_str(), second.status), (204, "", 204));
    assert_eq!(
        (
            app.stack_quantity(&character, ITEM),
            app.stack_quantity(&character, "anything")
        ),
        (1, 0)
    );
    // Every listed offer is free and sits in a real store category, not just the webstore request filter
    let listed = app
        .get("/product/skus/public?requiredTags=webstore", req().bearer(&user))
        .await;
    assert_eq!(listed.status, 200);
    let listed = listed.json.as_array().expect("a bare array").clone();
    assert!(
        listed
            .iter()
            .all(|o| o["platinumPrice"] == 0 && o["tags"].as_array().unwrap().iter().any(|t| t != "webstore"))
    );
    assert_eq!(listed.iter().find(|o| o["id"] == SKU).unwrap()["remaining"], 0);
    let single = app.get(&format!("/product/sku/{SKU}"), req().bearer(&user)).await;
    assert_eq!(
        (
            single.status,
            single.json["id"].clone(),
            single.json["remaining"].clone()
        ),
        (200, json!(SKU), json!(0))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_server_s_key_alone_gets_403_on_every_store_route() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let purchase = token(&app, &user, SKU).await.unwrap();
    for (method, path) in [
        ("GET", "/product/skus/public?requiredTags=webstore".to_owned()),
        ("GET", format!("/product/sku/{SKU}")),
        ("GET", format!("/token/platinum/{SKU}")),
        ("POST", format!("/notification/platinum?token={purchase}")),
    ] {
        assert_eq!(app.call(method, &path, req().gs()).await.status, 403, "{method} {path}");
    }
    assert_eq!(app.stack_quantity(&character, ITEM), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_the_store_s_refusals_with_a_code_and_a_message() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    let missing = app.get("/product/skus/public", req().bearer(&user)).await;
    assert_eq!(
        (missing.status, missing.json),
        (
            400,
            json!({"code": "400", "message": "missing requiredTags query parameter"})
        )
    );
    let unknown = app
        .get("/product/skus/public?requiredTags=not_a_tag", req().bearer(&user))
        .await;
    assert_eq!((unknown.status, unknown.json), (200, json!([])));
    assert_eq!(
        app.get("/product/skus/public?requiredTags=season09b_rank", req().bearer(&user))
            .await
            .json,
        json!([]),
        "no rank skip"
    );
    assert_eq!(
        app.get("/product/sku/season09b_10_ranks", req().bearer(&user))
            .await
            .status,
        404
    );
    assert_eq!(
        app.get("/token/notes/single_armour_monk_chest", req().bearer(&user))
            .await
            .status,
        400
    );
    assert_eq!(
        app.post("/notification/platinum?token=nope", req().bearer(&user))
            .await
            .status,
        400
    );
    assert_eq!(
        app.post("/notification/platinum", req().bearer(&user)).await.status,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_store_off_the_default_the_store_answers_as_it_did_before() {
    let app = TestApp::with_env(&[("TRIALS_STORE", "off"), ("MIDDLEMAN_STORE", "off")]).await;
    let (user, _) = app.make_player().await;
    let listed = app
        .get("/product/skus/public?requiredTags=webstore", req().bearer(&user))
        .await;
    assert_eq!(
        (listed.status, listed.json),
        (
            400,
            json!({"code": "400", "message": "The store is not available on Dauntless Revived yet."})
        )
    );
    assert_eq!(
        app.get("/product/skus/public?requiredTags=webstore", req().gs())
            .await
            .status,
        400,
        "the game server's key, as before"
    );
    for (method, path) in [
        ("GET", format!("/product/sku/{SKU}")),
        ("GET", format!("/token/platinum/{SKU}")),
        ("POST", format!("/notification/platinum?token={}", "a".repeat(64))),
    ] {
        assert_eq!(
            app.call(method, &path, req().bearer(&user)).await.status,
            404,
            "{method} {path}"
        );
        assert_eq!(
            app.call(method, &path, req()).await.status,
            404,
            "{method} {path} without a token"
        );
    }
    assert_eq!(app.count("storepurchases", "accountId = ?1", &[&user]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn sheens_cannot_be_listed_looked_up_issued_or_redeemed_from_an_outstanding_token() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    let sheen = "single_dye_sheen_glossy";
    for tag in catalog().as_object().unwrap().keys() {
        let listed = offers_in(&app, &own(&app), &user, tag).await;
        let is_sheen = |o: &Offer| {
            o["id"].as_str().unwrap_or_default().starts_with("single_dye_sheen_")
                || o["entitlements"].as_array().is_some_and(|g| {
                    g.iter()
                        .any(|e| e["name"].as_str().unwrap_or_default().starts_with("ent_dye_sheen_"))
                })
        };
        assert!(!listed.iter().any(is_sheen), "{tag}");
    }
    assert!(matches!(
        offer_in(&app, &own(&app), &user, sheen).await,
        Err(StoreFail::Refused(404, _))
    ));
    assert_eq!(refused_status(token(&app, &user, sheen).await), 404);
    let pending = token(&app, &user, "single_dye_hairtint_01").await.unwrap();
    app.run(
        "UPDATE storepurchases SET skuId = ?1 WHERE accountId = ?2",
        &[&sheen, &user],
    );
    assert_eq!(
        redeem(&app, &user, &pending).await,
        Err((409, "Offer is no longer sold; request a new token".to_owned()))
    );
    assert!(!names(&app, &user).await.iter().any(|n| n == "ent_dye_sheen_glossy"));
    let row = app.row("SELECT redeemedDate FROM storepurchases WHERE accountId = ?1", &[&user]);
    assert_eq!(row["redeemedDate"], Value::Null);
    // Node's second half hands the store a renamed sheen bundle (SetExtraOffersForTests): no Rust hook
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_dye_pack_charges_5000_rams_once_and_cannot_grant_while_funds_are_short() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let pilot = ctx(&app, &[("STORE_RAMS_TEST_ACCOUNT", &user)]);
    let sku = "bundle_dyes_palette_01";
    notes(&app, &character, 4999);
    let inventory = || (app.stacks(&character), instanced(&app, &character));
    let before = inventory();
    assert_eq!(offer_in(&app, &pilot, &user, sku).await.unwrap()["notesPrice"], 5000);
    assert_eq!(refused_status(token_in(&app, &pilot, &user, "notes", sku).await), 409);
    assert_eq!(inventory(), before);
    notes(&app, &character, 10000);
    let pending = token_in(&app, &pilot, &user, "notes", sku).await.unwrap();
    redeem_in(&app, &pilot, &user, "notes", &pending).await.unwrap();
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 5000);
    for colour in ["DYE_BLUE07_DURABLE", "DYE_BLUE08_DURABLE"] {
        assert_eq!(app.stack_quantity(&character, colour), 1);
    }
    let purchased = inventory();
    redeem_in(&app, &pilot, &user, "notes", &pending).await.unwrap();
    assert_eq!(inventory(), purchased);
}

// Node's non-platinum pricing cases hand the store a made-up Aetherdust offer; here they run against this
// week's first Middleman cell, a real Aetherdust offer (its item is the cell, not the helm).

async fn cell_offer(app: &TestApp, user: &str) -> (String, String, i64) {
    let found = offers_in(app, &own(app), user, "weekly_cell_offering").await.remove(0);
    (
        found["id"].as_str().unwrap().to_owned(),
        found["items"][0]["catalogId"].as_str().unwrap().to_owned(),
        found["cellDustPrice"].as_i64().unwrap(),
    )
}

fn dust(app: &TestApp, character: &str, quantity: i64) {
    set_stacks(
        app,
        character,
        &json!([{"catalogId": "CURRENCY_CELLDUST", "quantity": quantity}]).to_string(),
    );
}

async fn dust_token(app: &TestApp, user: &str, id: &str) -> crate::common::Reply {
    app.get(&format!("/token/celldust/{id}"), req().bearer(user)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn charges_the_exact_amount_from_the_matching_currency_stack_and_grants_the_item() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let (id, cell, price) = cell_offer(&app, &user).await;
    dust(&app, &character, price + 250);
    let purchase = dust_token(&app, &user, &id).await.json["purchaseToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let confirm = app
        .post(&format!("/notification/celldust?token={purchase}"), req().bearer(&user))
        .await;
    assert_eq!(confirm.status, 204);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_CELLDUST"), 250);
    assert_eq!(app.stack_quantity(&character, &cell), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_the_token_when_the_balance_is_short_and_grants_or_charges_nothing() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let (id, cell, price) = cell_offer(&app, &user).await;
    dust(&app, &character, price - 1);
    assert_eq!(dust_token(&app, &user, &id).await.status, 409);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_CELLDUST"), price - 1);
    assert_eq!(app.stack_quantity(&character, &cell), 0);
    assert_eq!(app.count("storepurchases", "accountId = ?1", &[&user]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_redeem_if_the_balance_drops_below_the_price_after_the_token_is_issued() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let (id, cell, price) = cell_offer(&app, &user).await;
    dust(&app, &character, price);
    let purchase = dust_token(&app, &user, &id).await.json["purchaseToken"]
        .as_str()
        .unwrap()
        .to_owned();
    dust(&app, &character, 10);
    let confirm = app
        .post(&format!("/notification/celldust?token={purchase}"), req().bearer(&user))
        .await;
    assert_eq!(confirm.status, 409);
    assert_eq!(app.stack_quantity(&character, &cell), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_currency_that_does_not_match_the_offer_s_own_price_field() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let (id, _, _) = cell_offer(&app, &user).await;
    set_stacks(
        &app,
        &character,
        &json!([{"catalogId": "CURRENCY_PLATINUM_UNIV", "quantity": 1000}]).to_string(),
    );
    assert_eq!(
        app.get(&format!("/token/platinum/{id}"), req().bearer(&user))
            .await
            .status,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_an_unrecognised_currency_string() {
    let app = app(&[]).await;
    let (user, _) = app.make_player().await;
    let (id, _, _) = cell_offer(&app, &user).await;
    assert_eq!(
        app.get(&format!("/token/event01/{id}"), req().bearer(&user))
            .await
            .status,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retried_confirm_does_not_charge_twice() {
    let app = app(&[]).await;
    let (user, character) = app.make_player().await;
    let (id, _, price) = cell_offer(&app, &user).await;
    dust(&app, &character, price);
    let purchase = dust_token(&app, &user, &id).await.json["purchaseToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/notification/celldust?token={purchase}");
    let (first, second) = (
        app.post(&path, req().bearer(&user)).await,
        app.post(&path, req().bearer(&user)).await,
    );
    assert_eq!((first.status, second.status), (204, 204));
    assert_eq!(app.stack_quantity(&character, "CURRENCY_CELLDUST"), 0);
}
