//! `inventoryauthority.test.ts`: a player's client can't mint items through POST /inventory; a game
//! server's grant works and a retry grants once.

use serde_json::{Value, json};

use crate::common::{TestApp, req};

async fn inventory(app: &TestApp, request: crate::common::Req) -> u16 {
    app.post("/inventory", request).await.status
}

#[tokio::test(flavor = "multi_thread")]
async fn player_cannot_mint_currency_by_forging_a_gameserver_source() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let body = json!({"accountId": user, "characterId": character, "transactionId": "forged-reward", "source": "owner-authorized:reward",
        "addStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 1_000_000}]});
    assert_eq!(inventory(&app, req().bearer(&user).json(body)).await, 403);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn player_cannot_insert_an_unearned_item_through_saveinstanceditems() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let body = json!({"accountId": user, "characterId": character, "transactionId": "forged-save",
        "saveInstancedItems": [{"catalogId": "AC_HEAD_HALO", "instanceId": "forged-instance", "updateVersion": 0}]});
    assert_eq!(inventory(&app, req().bearer(&user).json(body)).await, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn authenticated_native_reward_still_succeeds_and_identical_retry_grants_once() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let body = json!({"accountId": user, "characterId": character, "transactionId": "native-reward",
        "addStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 5}]});
    assert_eq!(inventory(&app, req().gs().json(body.clone())).await, 200);
    assert_eq!(inventory(&app, req().gs().json(body)).await, 200);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn rams_purchases_refuse_stale_balances_atomically_even_with_legacy_material_clamping() {
    let app = TestApp::with_env(&[("INVENTORY_REFUSE_OVERSPEND", "0")]).await;
    let (user, character) = app.make_player().await;
    let funds = json!({"accountId": user, "characterId": character, "transactionId": "tonic-funds",
        "addStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 350}]});
    assert_eq!(inventory(&app, req().gs().json(funds)).await, 200);
    let buy = |id: &str| {
        req()
            .gs()
            .json(json!({"accountId": user, "characterId": character, "transactionId": id,
            "removeStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 350}],
            "addStackedItems": [{"catalogId": "QI_STAGGER_POTION", "quantity": 1}]}))
    };
    let (a, b) = tokio::join!(inventory(&app, buy("tonic-a")), inventory(&app, buy("tonic-b")));
    let mut statuses = [a, b];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_NOTES"), 0);
    assert_eq!(app.stack_quantity(&character, "QI_STAGGER_POTION"), 1);
    let accepted = if a == 200 { "tonic-a" } else { "tonic-b" };
    assert_eq!(inventory(&app, buy(accepted)).await, 200);
    assert_eq!(app.stack_quantity(&character, "QI_STAGGER_POTION"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn foreign_character_invented_catalog_negative_quantity_and_impossible_upgrade_cannot_mutate_inventory() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let (other, other_character) = app.make_player().await;
    let bodies: [Value; 4] = [
        json!({"accountId": other, "characterId": other_character, "addStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 10}]}),
        json!({"accountId": user, "characterId": character, "addInstancedItems": [{"catalogId": "NONEXISTENT", "instanceId": "fake", "updateVersion": 0}]}),
        json!({"accountId": user, "characterId": character, "removeStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": -10}]}),
        json!({"accountId": user, "characterId": character, "saveInstancedItems": [{"catalogId": "WEAPON_TEST", "instanceId": "fake", "updateVersion": 999, "itemData": "{\"Level\":999}"}]}),
    ];
    for mut body in bodies {
        body["transactionId"] = json!(metagame::crypto::random_uuid());
        let status = inventory(&app, req().bearer(&user).json(body.clone())).await;
        assert!([400, 403].contains(&status), "{status} {body}");
    }
    assert_eq!(app.stack_quantity(&other_character, "CURRENCY_NOTES"), 0);
    let item = json!({"characterId": character, "instanceId": "fake", "catalogId": "WEAPON_TEST", "itemData": "{\"Level\":999}", "updateVersion": 999});
    assert_eq!(
        app.post("/inventory/instanceditem", req().bearer(&user).json(item))
            .await
            .status,
        403
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mutation_quota_is_per_player_and_does_not_stop_native_rewards_or_reads() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let body = json!({"characterId": character, "addStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 1}]});
    for _ in 0..60 {
        assert_eq!(inventory(&app, req().bearer(&user).json(body.clone())).await, 403);
    }
    assert_eq!(inventory(&app, req().bearer(&user).json(body.clone())).await, 429);
    let read = app
        .get(&format!("/inventory/{user}/{character}"), req().bearer(&user))
        .await;
    assert_eq!(read.status, 200);
    let mut native = body;
    native["accountId"] = json!(user);
    native["transactionId"] = json!("native-after-rate");
    assert_eq!(inventory(&app, req().gs().json(native)).await, 200);
}
