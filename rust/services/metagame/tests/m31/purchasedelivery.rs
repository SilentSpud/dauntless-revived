//! `purchasedelivery.test.ts` (upstream 7c1191d, the M44 catch-up): a purchase is a debit and a grant in
//! one transaction, and whatever stops the grant must stop the debit with it. An instanced add whose
//! instance the character already holds is skipped (so a retry does not duplicate the item); if that
//! request also takes payment, it is a repeat of a purchase that already succeeded and is refused.

use std::sync::{Arc, Mutex};

use metagame::inventory::{Applied, Fail, InventoryCaller, TransactionRequest, apply_in_tx};
use serde_json::{Value, json};

use crate::common::TestApp;

fn guid() -> String {
    metagame::crypto::random_uuid().replace('-', "").to_uppercase()
}

fn helm(instance: &str) -> Value {
    json!({"catalogId": "AR_HELM_TEST", "instanceId": instance, "updateVersion": 0})
}

fn notes(quantity: i64) -> Value {
    json!([{"catalogId": "CURRENCY_NOTES", "quantity": quantity}])
}

fn request(user: &str, character: &str) -> TransactionRequest {
    TransactionRequest {
        user_id: Some(user.to_owned()),
        character_id: Some(json!(character)),
        transaction_id: Some(json!(guid())),
        add_instanced: None,
        add_stacked: None,
        remove_instanced: None,
        remove_stacked: None,
        save_instanced: None,
    }
}

/// One transaction applying `request` as a game server's vendor; `Err` is the refusal (rolled back).
async fn apply(app: &TestApp, request: TransactionRequest) -> Result<Applied, Fail> {
    let settings = app.state.inventory;
    let outcome = Arc::new(Mutex::new(None));
    let seen = outcome.clone();
    let result = app
        .try_db(move |conn| {
            let source = json!("vendor:test");
            match apply_in_tx(conn, request, InventoryCaller::GameServer, Some(&source), &settings) {
                Ok(applied) => Ok(applied),
                Err(fail) => {
                    *seen.lock().unwrap() = Some(fail);
                    Err(diesel::result::Error::RollbackTransaction)
                }
            }
        })
        .await;
    match result {
        Ok(applied) => Ok(applied),
        Err(_) => Err(outcome.lock().unwrap().take().unwrap()),
    }
}

fn instanced_items(app: &TestApp, character: &str) -> usize {
    let row = app.row(
        "SELECT instancedItems FROM inventories WHERE characterId = ?1",
        &[&character],
    );
    let text = row["instancedItems"].as_str().unwrap_or("[]");
    serde_json::from_str::<Vec<Value>>(text).unwrap().len()
}

async fn player_holding(app: &TestApp, instance: &str) -> (String, String) {
    let (user, character) = app.make_player().await;
    let mut grant = request(&user, &character);
    grant.add_stacked = Some(notes(5000));
    grant.add_instanced = Some(json!([helm(instance)]));
    apply(app, grant).await.unwrap();
    (user, character)
}

#[tokio::test(flavor = "multi_thread")]
async fn takes_no_currency_and_writes_nothing_when_the_grant_would_be_skipped() {
    let app = TestApp::start().await;
    let (user, character) = player_holding(&app, "collide-1").await;
    let log_rows = app.count("inventorylog", "characterId = ?1", &[&character]);

    let mut repeat = request(&user, &character);
    repeat.remove_stacked = Some(notes(750));
    repeat.add_instanced = Some(json!([helm("collide-1")]));
    assert!(matches!(apply(&app, repeat).await, Err(Fail::Conflict(_))));

    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 5000);
    assert_eq!(instanced_items(&app, &character), 1);
    assert_eq!(app.count("inventorylog", "characterId = ?1", &[&character]), log_rows);
}

#[tokio::test(flavor = "multi_thread")]
async fn still_delivers_and_charges_when_the_same_request_also_brings_a_new_instance() {
    let app = TestApp::start().await;
    let (user, character) = player_holding(&app, "collide-2").await;

    let mut purchase = request(&user, &character);
    purchase.remove_stacked = Some(notes(750));
    purchase.add_instanced = Some(json!([helm("collide-2"), helm("fresh-2")]));
    apply(&app, purchase).await.unwrap();

    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 4250);
    assert_eq!(instanced_items(&app, &character), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaves_a_free_re_add_alone_nothing_is_paid_so_nothing_is_refused() {
    let app = TestApp::start().await;
    let (user, character) = player_holding(&app, "collide-3").await;

    let mut again = request(&user, &character);
    again.add_instanced = Some(json!([helm("collide-3")]));
    let applied = apply(&app, again).await.unwrap();

    assert!(!applied.replayed);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 5000);
    assert_eq!(instanced_items(&app, &character), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_a_true_retry_from_the_ledger_without_reaching_the_refusal() {
    let app = TestApp::start().await;
    let (user, character) = player_holding(&app, "collide-4").await;
    let id = guid();
    let purchase = || {
        let mut purchase = request(&user, &character);
        purchase.transaction_id = Some(json!(id));
        purchase.remove_stacked = Some(notes(750));
        purchase.add_instanced = Some(json!([helm("fresh-4")]));
        purchase
    };

    apply(&app, purchase()).await.unwrap();
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 4250);

    let again = apply(&app, purchase()).await.unwrap();
    assert!(again.replayed);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 4250);
    assert_eq!(instanced_items(&app, &character), 2);
}
