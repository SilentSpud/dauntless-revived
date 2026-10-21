//! `inventorytx.test.ts`: the inventory core inside the caller's transaction (`apply_in_tx`), which the
//! store and Trials grant through. Node's `RunInventoryTransaction` is POST /inventory here.

use std::sync::{Arc, Mutex};

use diesel::prelude::*;
use metagame::inventory::{Applied, Fail, InventoryCaller, InventoryError, TransactionRequest, apply_in_tx};
use serde_json::{Value, json};

use crate::common::{TestApp, req};

fn guid() -> String {
    metagame::crypto::random_uuid().replace('-', "").to_uppercase()
}

fn stack(catalog: &str, quantity: Value) -> Value {
    json!({"catalogId": catalog, "quantity": quantity})
}

fn request(user: &str, character: &str, id: &str) -> TransactionRequest {
    TransactionRequest {
        user_id: Some(user.to_owned()),
        character_id: Some(json!(character)),
        transaction_id: Some(json!(id)),
        add_instanced: None,
        add_stacked: None,
        remove_instanced: None,
        remove_stacked: None,
        save_instanced: None,
    }
}

fn store_source() -> Value {
    json!("store:test_offer")
}

/// One transaction applying `request` as the store; `Err` is the refusal (the transaction rolled back).
async fn apply_as(app: &TestApp, request: TransactionRequest, caller: InventoryCaller) -> Result<Applied, Fail> {
    let settings = app.state.inventory;
    let source = (caller == InventoryCaller::Store).then(store_source);
    let outcome = Arc::new(Mutex::new(None));
    let seen = outcome.clone();
    let result = app
        .try_db(
            move |conn| match apply_in_tx(conn, request, caller, source.as_ref(), &settings) {
                Ok(applied) => Ok(applied),
                Err(fail) => {
                    *seen.lock().unwrap() = Some(fail);
                    Err(diesel::result::Error::RollbackTransaction)
                }
            },
        )
        .await;
    match result {
        Ok(applied) => Ok(applied),
        Err(_) => Err(outcome.lock().unwrap().take().unwrap()),
    }
}

fn log_count(app: &TestApp, character: &str) -> i64 {
    app.count("inventorylog", "characterId = ?1", &[&character])
}

#[tokio::test(flavor = "multi_thread")]
async fn rolls_back_with_an_outer_transaction_that_throws_no_stack_no_item_log_no_ledger_row() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let id = guid();
    let mut r = request(&user, &character, &id);
    r.add_stacked = Some(json!([stack("ORB", json!(5))]));
    r.add_instanced = Some(json!([{"catalogId": "WP_AXE", "instanceId": "store-1", "updateVersion": 0}]));
    let settings = app.state.inventory;
    let c = character.clone();
    let result = app
        .try_db(move |conn| {
            let applied = apply_in_tx(conn, r, InventoryCaller::Store, Some(&store_source()), &settings).unwrap();
            assert!(!applied.replayed);
            // Visible inside the transaction
            let stacked: String = metagame::schema::inventories::table
                .find(&c)
                .select(metagame::schema::inventories::stackedItems)
                .first(conn)?;
            let stacks: Value = serde_json::from_str(&stacked).unwrap();
            assert_eq!(stacks[0]["quantity"], 5);
            // The purchase receipt could not be written
            Err::<(), _>(diesel::result::Error::RollbackTransaction)
        })
        .await;
    assert!(result.is_err());
    assert_eq!(app.stacks(&character), Vec::<Value>::new());
    assert_eq!(log_count(&app, &character), 0);
    assert_eq!(app.count("inventorytransactions", "transactionId = ?1", &[&id]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn commits_with_the_outer_transaction_the_same_ledger_and_item_log_as_post_inventory_as_caller_store() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let id = guid();
    let mut r = request(&user, &character, &id);
    r.add_stacked = Some(json!([stack("CURRENCY_RAMS", json!(20))]));
    let applied = apply_as(&app, r.clone(), InventoryCaller::Store).await.unwrap();
    assert_eq!(
        applied.response,
        json!({"updatedInstancedItems": [], "updatedStackedItems": [stack("CURRENCY_RAMS", json!(20))]})
    );
    assert!(applied.overspent.is_empty());
    assert_eq!(app.stack_quantity(&character, "CURRENCY_RAMS"), 20);
    let rows = app.rows(
        "SELECT caller, source, transactionId, operation, catalogId, quantityChange FROM inventorylog WHERE characterId = ?1",
        &[&character],
    );
    assert_eq!(
        rows,
        [
            json!({"caller": "store", "source": "store:test_offer", "transactionId": id, "operation": "add", "catalogId": "CURRENCY_RAMS", "quantityChange": 20})
        ]
    );
    assert_eq!(app.count("inventorytransactions", "transactionId = ?1", &[&id]), 1);
    // One ledger for both entry points: the same request again is a replay, whichever way it comes
    assert!(apply_as(&app, r, InventoryCaller::Store).await.unwrap().replayed);
    let body = json!({"accountId": user, "characterId": character, "transactionId": id, "addStackedItems": [stack("CURRENCY_RAMS", json!(20))]});
    assert_eq!(app.post("/inventory", req().gs().json(body)).await.status, 200);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_RAMS"), 20);
    assert_eq!(log_count(&app, &character), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn gives_the_same_reply_and_the_same_inventory_as_runinventorytransaction_for_the_same_request() {
    let app = TestApp::start().await;
    let (a, a_character) = app.make_player().await;
    let (b, b_character) = app.make_player().await;
    let add_instanced = json!([{"catalogId": "WP_SWORD", "instanceId": "i-9", "updateVersion": 0}]);
    let add_stacked = json!([stack("ORB", json!(3)), stack("NOTES", json!("7"))]);
    let remove_stacked = json!([stack("NOTES", json!(2))]);
    let body = json!({"accountId": a, "characterId": a_character, "transactionId": guid(), "addInstancedItems": add_instanced,
        "addStackedItems": add_stacked, "removeStackedItems": remove_stacked});
    let via_route = app.post("/inventory", req().gs().json(body)).await;
    assert_eq!(via_route.status, 200, "{}", via_route.text);
    let mut r = request(&b, &b_character, &guid());
    r.add_instanced = Some(add_instanced);
    r.add_stacked = Some(add_stacked);
    r.remove_stacked = Some(remove_stacked);
    let via_core = apply_as(&app, r, InventoryCaller::GameServer).await.unwrap();
    assert_eq!(service_kit::jsjson::stringify(&via_core.response), via_route.text);
    assert_eq!(app.stacks(&b_character), app.stacks(&a_character));
    assert_eq!(
        app.stacks(&b_character),
        [stack("ORB", json!(3)), stack("NOTES", json!(5))]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_throws_and_takes_the_caller_s_earlier_writes_in_the_same_transaction_back_with_it() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let settings = app.state.inventory;
    let (u, c) = (user.clone(), character.clone());
    let refused = Arc::new(Mutex::new(None));
    let seen = refused.clone();
    let result = app
        .try_db(move |conn| {
            let mut first = request(&u, &c, &guid());
            first.add_stacked = Some(json!([stack("ORB", json!(1))]));
            apply_in_tx(conn, first, InventoryCaller::Store, Some(&store_source()), &settings).unwrap();
            let mut second = request(&u, &c, &guid());
            second.add_stacked = Some(json!("not a list"));
            match apply_in_tx(conn, second, InventoryCaller::Store, Some(&store_source()), &settings) {
                Ok(_) => Ok(()),
                Err(fail) => {
                    *seen.lock().unwrap() = Some(fail);
                    Err(diesel::result::Error::RollbackTransaction)
                }
            }
        })
        .await;
    assert!(result.is_err());
    let fail = refused.lock().unwrap().take().unwrap();
    assert!(matches!(fail, Fail::Validation(_)), "{fail:?}");
    assert_eq!(fail.error().name(), "invalid_inventory_item");
    assert_eq!(app.stacks(&character), Vec::<Value>::new());
    assert_eq!(log_count(&app, &character), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_another_player_s_character_inside_the_transaction_even_for_an_empty_request() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (_, b_character) = app.make_player().await;
    let mut add = request(&a, &b_character, &guid());
    add.add_stacked = Some(json!([stack("ORB", json!(1))]));
    let empty = request(&a, &b_character, &guid());
    let mut missing = request(&a, "no-such-character", &guid());
    missing.add_stacked = Some(json!([stack("ORB", json!(1))]));
    for r in [add, empty, missing] {
        let fail = apply_as(&app, r, InventoryCaller::Store).await.unwrap_err();
        assert_eq!(fail.error(), InventoryError::Forbidden);
        assert_eq!(fail.error().name(), "forbidden");
    }
    assert_eq!(app.stacks(&b_character), Vec::<Value>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_request_changes_nothing_and_answers_the_empty_reply() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let mut r = request(&user, &character, &guid());
    r.add_stacked = Some(json!([]));
    let applied = apply_as(&app, r, InventoryCaller::Store).await.unwrap();
    assert_eq!(
        applied.response,
        json!({"updatedInstancedItems": [], "updatedStackedItems": []})
    );
    assert!(!applied.replayed);
    assert_eq!(log_count(&app, &character), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn overspends_follow_inventory_refuse_overspend_returned_for_the_caller_to_log_by_default_thrown_with_1() {
    let overspend = |user: &str, character: &str| {
        let mut r = request(user, character, &guid());
        r.add_stacked = Some(json!([stack("REWARD", json!(2))]));
        r.remove_stacked = Some(json!([stack("ORB", json!(4))]));
        r
    };
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let clamped = apply_as(&app, overspend(&user, &character), InventoryCaller::Store)
        .await
        .unwrap();
    assert_eq!(clamped.overspent, ["removes 4 of ORB but only 0 held"]);
    assert_eq!(app.stack_quantity(&character, "REWARD"), 2);

    let strict = TestApp::with_env(&[("INVENTORY_REFUSE_OVERSPEND", "1")]).await;
    let (user, character) = strict.make_player().await;
    let fail = apply_as(&strict, overspend(&user, &character), InventoryCaller::Store)
        .await
        .unwrap_err();
    assert!(matches!(fail, Fail::Insufficient(_)), "{fail:?}");
    assert_eq!(fail.error().name(), "insufficient_quantity");
    assert_eq!(strict.stack_quantity(&character, "REWARD"), 0, "nothing was granted");
}

/// A database error has no inventory error of its own in Node (`InventoryErrorOf` gives undefined);
/// here every failure is a `Fail`, and a database one is `db_error`, the 500 the route answers either way.
#[tokio::test(flavor = "multi_thread")]
async fn maps_a_stored_inventory_that_is_not_json_to_invalid_inventory_data_and_a_database_error_to_nothing() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    app.run(
        "INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES (?1, '[]', '{broken')",
        &[&character],
    );
    let mut r = request(&user, &character, &guid());
    r.add_stacked = Some(json!([stack("ORB", json!(1))]));
    let fail = apply_as(&app, r, InventoryCaller::Store).await.unwrap_err();
    assert_eq!(fail.error().name(), "invalid_inventory_data");
    assert_eq!(Fail::Broken("SQLITE_BUSY".into()).error(), InventoryError::DbError);
}
