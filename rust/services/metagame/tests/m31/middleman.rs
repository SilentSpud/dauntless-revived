//! `middleman.test.ts`: fusion tokens stored through the inventory core as the game server sends
//! them. MIDDLEMAN_FUSION_GUARDS is off unless a test turns it on.

use std::sync::{Arc, Mutex};

use metagame::inventory::{Applied, Fail, InventoryCaller, InventoryError, TransactionRequest, apply_in_tx};
use serde_json::{Value, json};

use crate::common::TestApp;

fn fusion(slot: i64, exchange: &str, end: &str, cell: &str) -> Value {
    json!({"catalogId": "TOKEN_CELL_EXCHANGE", "instanceId": "TOKEN_CELL_EXCHANGE", "updateVersion": 0,
        "itemData": json!({"SlotID": slot, "ExchangeID": exchange, "EndTime": end, "ResultCell": cell}).to_string()})
}

fn stack(catalog: &str, quantity: i64) -> Value {
    json!({"catalogId": catalog, "quantity": quantity})
}

#[derive(Default)]
struct Change {
    add_instanced: Vec<Value>,
    add_stacked: Vec<Value>,
    remove_instanced: Vec<Value>,
    remove_stacked: Vec<Value>,
    save_instanced: Vec<Value>,
}

/// `RunInventoryTransaction` as the game server; `Err` is the refusal's error code.
async fn run(
    app: &TestApp,
    guards: bool,
    (user, character): &(String, String),
    id: &str,
    change: Change,
) -> Result<Applied, InventoryError> {
    let request = TransactionRequest {
        user_id: Some(user.clone()),
        character_id: Some(json!(character)),
        transaction_id: Some(json!(id)),
        add_instanced: Some(json!(change.add_instanced)),
        add_stacked: Some(json!(change.add_stacked)),
        remove_instanced: Some(json!(change.remove_instanced)),
        remove_stacked: Some(json!(change.remove_stacked)),
        save_instanced: Some(json!(change.save_instanced)),
    };
    let mut settings = app.state.inventory;
    settings.fusion_guards = guards;
    let refused: Arc<Mutex<Option<Fail>>> = Arc::default();
    let seen = refused.clone();
    let result = app
        .try_db(move |conn| {
            apply_in_tx(conn, request, InventoryCaller::GameServer, None, &settings).map_err(|fail| {
                *seen.lock().unwrap() = Some(fail);
                diesel::result::Error::RollbackTransaction
            })
        })
        .await;
    result.map_err(|_| refused.lock().unwrap().take().unwrap().error())
}

fn instanced(app: &TestApp, character: &str) -> Vec<Value> {
    let row = app.row(
        "SELECT instancedItems FROM inventories WHERE characterId = ?1",
        &[&character],
    );
    serde_json::from_str(row["instancedItems"].as_str().unwrap_or("[]")).unwrap()
}

fn data(item: &Value) -> Value {
    serde_json::from_str(item["itemData"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_three_pending_slots_independent_even_when_the_client_sends_one_generic_instance_id() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let tokens = vec![
        fusion(1, "fusion-1", "2099-01-01T00:00:00.000Z", "CELL_TEST_A_UC"),
        fusion(2, "fusion-2", "2099-01-01T00:00:00.000Z", "CELL_TEST_B_UC"),
        fusion(3, "fusion-3", "2099-01-01T00:00:00.000Z", "CELL_TEST_C_R"),
    ];
    assert!(
        run(
            &app,
            false,
            &a,
            "middleman-three",
            Change {
                add_instanced: tokens,
                ..Default::default()
            }
        )
        .await
        .is_ok()
    );
    let held = instanced(&app, &a.1);
    let mut ids: Vec<&str> = held.iter().map(|i| i["instanceId"].as_str().unwrap()).collect();
    ids.sort();
    assert_eq!(
        ids,
        [
            "TOKEN_CELL_EXCHANGE:1",
            "TOKEN_CELL_EXCHANGE:2",
            "TOKEN_CELL_EXCHANGE:3"
        ]
    );
    let mut exchanges: Vec<Value> = held.iter().map(|i| data(i)["ExchangeID"].clone()).collect();
    exchanges.sort_by_key(|v| v.to_string());
    assert_eq!(exchanges, [json!("fusion-1"), json!("fusion-2"), json!("fusion-3")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn accepts_a_same_version_speed_up_only_when_it_keeps_the_slot_exchange_and_result() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let original = fusion(1, "fusion-speed", "2099-01-01T00:00:00.000Z", "CELL_TEST_A_R");
    assert!(
        run(
            &app,
            false,
            &a,
            "middleman-start",
            Change {
                add_instanced: vec![original],
                ..Default::default()
            }
        )
        .await
        .is_ok()
    );

    let sped_up = fusion(1, "fusion-speed", "2020-01-01T00:00:00.000Z", "CELL_TEST_A_R");
    assert!(
        run(
            &app,
            false,
            &a,
            "middleman-speed",
            Change {
                save_instanced: vec![sped_up],
                ..Default::default()
            }
        )
        .await
        .is_ok()
    );
    assert_eq!(data(&instanced(&app, &a.1)[0])["EndTime"], "2020-01-01T00:00:00.000Z");

    let changed = fusion(1, "fusion-speed", "2019-01-01T00:00:00.000Z", "CELL_TEST_WRONG_R");
    let refused = run(
        &app,
        false,
        &a,
        "middleman-change-result",
        Change {
            save_instanced: vec![changed],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(refused.err(), Some(InventoryError::Conflict));
    assert_eq!(data(&instanced(&app, &a.1)[0])["ResultCell"], "CELL_TEST_A_R");
}

#[tokio::test(flavor = "multi_thread")]
async fn persists_cell_dusting_as_one_inventory_transaction() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    run(
        &app,
        false,
        &a,
        "middleman-dust-seed",
        Change {
            add_stacked: vec![stack("CELL_TEST_DUST_R", 2)],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let dust = || Change {
        remove_stacked: vec![stack("CELL_TEST_DUST_R", 1)],
        add_stacked: vec![stack("CURRENCY_CELLDUST", 80)],
        ..Default::default()
    };
    assert!(run(&app, false, &a, "middleman-dust", dust()).await.is_ok());
    assert_eq!(
        (
            app.stack_quantity(&a.1, "CELL_TEST_DUST_R"),
            app.stack_quantity(&a.1, "CURRENCY_CELLDUST")
        ),
        (1, 80)
    );
    assert!(run(&app, false, &a, "middleman-dust", dust()).await.unwrap().replayed);
    assert_eq!(
        (
            app.stack_quantity(&a.1, "CELL_TEST_DUST_R"),
            app.stack_quantity(&a.1, "CURRENCY_CELLDUST")
        ),
        (1, 80)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn persists_an_ace_chip_speed_up_with_the_fusion_timer_change_atomically() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let seed = Change {
        add_instanced: vec![fusion(
            2,
            "fusion-chip",
            "2099-01-01T00:00:00.000Z",
            "CELL_TEST_SPEED_R",
        )],
        add_stacked: vec![stack("CURRENCY_TOKEN_EXCHANGE_SPEED_UP", 5)],
        ..Default::default()
    };
    run(&app, false, &a, "middleman-chip-seed", seed).await.unwrap();
    let speed = Change {
        save_instanced: vec![fusion(
            2,
            "fusion-chip",
            "2020-01-01T00:00:00.000Z",
            "CELL_TEST_SPEED_R",
        )],
        remove_stacked: vec![stack("CURRENCY_TOKEN_EXCHANGE_SPEED_UP", 2)],
        ..Default::default()
    };
    assert!(run(&app, false, &a, "middleman-chip-speed", speed).await.is_ok());
    assert_eq!(app.stack_quantity(&a.1, "CURRENCY_TOKEN_EXCHANGE_SPEED_UP"), 3);
    assert_eq!(data(&instanced(&app, &a.1)[0])["EndTime"], "2020-01-01T00:00:00.000Z");
}

#[tokio::test(flavor = "multi_thread")]
async fn allows_the_completed_version_zero_fusion_token_to_be_removed() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let token = fusion(1, "fusion-remove", "2020-01-01T00:00:00.000Z", "CELL_TEST_A_UC");
    run(
        &app,
        false,
        &a,
        "middleman-remove-start",
        Change {
            add_instanced: vec![token.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        run(
            &app,
            false,
            &a,
            "middleman-remove",
            Change {
                remove_instanced: vec![token],
                ..Default::default()
            }
        )
        .await
        .is_ok()
    );
    assert_eq!(instanced(&app, &a.1), Vec::<Value>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn lets_a_finished_fusion_be_claimed_after_a_speed_up_even_if_the_client_sends_its_older_token() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let token = fusion(1, "fusion-claim", "2099-01-01T00:00:00.000Z", "CELL_TEST_CLAIM_R");
    run(
        &app,
        false,
        &a,
        "middleman-claim-start",
        Change {
            add_instanced: vec![token.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut sped_up = fusion(1, "fusion-claim", "2020-01-01T00:00:00.000Z", "CELL_TEST_CLAIM_R");
    sped_up["updateVersion"] = json!(1);
    assert!(
        run(
            &app,
            false,
            &a,
            "middleman-claim-speed",
            Change {
                save_instanced: vec![sped_up],
                ..Default::default()
            }
        )
        .await
        .is_ok()
    );
    let claim = Change {
        remove_instanced: vec![token],
        add_stacked: vec![stack("CELL_TEST_CLAIM_R", 1)],
        ..Default::default()
    };
    assert!(run(&app, false, &a, "middleman-claim", claim).await.is_ok());
    assert_eq!(instanced(&app, &a.1), Vec::<Value>::new());
    assert_eq!(app.stack_quantity(&a.1, "CELL_TEST_CLAIM_R"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn lets_a_fusion_be_claimed_when_the_removal_s_itemdata_is_not_the_shape_we_expect() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let token = fusion(2, "fusion-odd", "2020-01-01T00:00:00.000Z", "CELL_TEST_ODD_R");
    run(
        &app,
        false,
        &a,
        "middleman-odd-start",
        Change {
            add_instanced: vec![token],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let odd = json!({"catalogId": "TOKEN_CELL_EXCHANGE", "instanceId": "TOKEN_CELL_EXCHANGE:2", "updateVersion": 0, "itemData": "{\"Slot\":2}"});
    let claim = Change {
        remove_instanced: vec![odd],
        add_stacked: vec![stack("CELL_TEST_ODD_R", 1)],
        ..Default::default()
    };
    assert!(run(&app, false, &a, "middleman-odd-claim", claim).await.is_ok());
    assert_eq!(instanced(&app, &a.1), Vec::<Value>::new());
    assert_eq!(app.stack_quantity(&a.1, "CELL_TEST_ODD_R"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_strict_guards_refuses_early_or_wrong_reveals_and_makes_a_completed_reveal_idempotent() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let future = fusion(1, "fusion-guard", "2099-01-01T00:00:00.000Z", "CELL_TEST_RESULT_R");
    run(
        &app,
        true,
        &a,
        "middleman-guard-start",
        Change {
            add_instanced: vec![future.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let reveal = |token: &Value, cell: &str| Change {
        remove_instanced: vec![token.clone()],
        add_stacked: vec![stack(cell, 1)],
        ..Default::default()
    };

    let early = run(
        &app,
        true,
        &a,
        "middleman-guard-early",
        reveal(&future, "CELL_TEST_RESULT_R"),
    )
    .await;
    assert_eq!(early.err(), Some(InventoryError::Conflict));
    assert_eq!(instanced(&app, &a.1).len(), 1);
    assert_eq!(app.stack_quantity(&a.1, "CELL_TEST_RESULT_R"), 0);

    let ready = fusion(1, "fusion-guard", "2020-01-01T00:00:00.000Z", "CELL_TEST_RESULT_R");
    assert!(
        run(
            &app,
            true,
            &a,
            "middleman-guard-speed",
            Change {
                save_instanced: vec![ready.clone()],
                ..Default::default()
            }
        )
        .await
        .is_ok()
    );

    let wrong = run(
        &app,
        true,
        &a,
        "middleman-guard-wrong",
        reveal(&ready, "CELL_TEST_WRONG_R"),
    )
    .await;
    assert_eq!(wrong.err(), Some(InventoryError::Conflict));
    assert_eq!(instanced(&app, &a.1).len(), 1);
    assert_eq!(app.stack_quantity(&a.1, "CELL_TEST_WRONG_R"), 0);

    let first = run(
        &app,
        true,
        &a,
        "middleman-guard-reveal",
        reveal(&ready, "CELL_TEST_RESULT_R"),
    )
    .await
    .unwrap();
    let retry = run(
        &app,
        true,
        &a,
        "middleman-guard-reveal",
        reveal(&ready, "CELL_TEST_RESULT_R"),
    )
    .await
    .unwrap();
    let new_id = run(
        &app,
        true,
        &a,
        "middleman-guard-reveal-again",
        reveal(&ready, "CELL_TEST_RESULT_R"),
    )
    .await;
    assert!(!first.replayed && retry.replayed);
    assert_eq!(new_id.err(), Some(InventoryError::Conflict));
    assert_eq!(app.stack_quantity(&a.1, "CELL_TEST_RESULT_R"), 1);
    assert_eq!(instanced(&app, &a.1), Vec::<Value>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_malformed_fusion_metadata_without_corrupting_the_inventory() {
    let app = TestApp::start().await;
    let a = app.make_player().await;
    let bad = json!({"catalogId": "TOKEN_CELL_EXCHANGE", "instanceId": "TOKEN_CELL_EXCHANGE", "updateVersion": 0,
        "itemData": json!({"SlotID": 99, "EndTime": "never", "ResultCell": "NOT_A_CELL", "ExchangeID": ""}).to_string()});
    let refused = run(
        &app,
        false,
        &a,
        "middleman-bad",
        Change {
            add_instanced: vec![bad],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(refused.err(), Some(InventoryError::InvalidItem));
    assert_eq!(instanced(&app, &a.1), Vec::<Value>::new());
}
