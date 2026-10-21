//! `inventoryported.test.ts`: Harmonic's inventory cases (github.com/Harmonicrain/Undaunted 895f7c7,
//! test/inventory.test.js) against our rules, through a game server's POST /inventory. Where the rules
//! differ, the case asserts ours: an overspend is refused only with INVENTORY_REFUSE_OVERSPEND=1, a
//! quantity of 0 is accepted, and the reply lists every changed stack (compared by catalogId).

use serde_json::{Value, json};

use crate::common::{Reply, TestApp, req};

#[derive(Default)]
struct Lists {
    add: Value,
    remove: Value,
}

fn lists(add: Value, remove: Value) -> Lists {
    Lists { add, remove }
}

async fn transaction(app: &TestApp, user: &str, character: &str, id: &str, lists: Lists) -> Reply {
    let or_empty = |v: Value| if v.is_null() { json!([]) } else { v };
    let body = json!({"accountId": user, "characterId": character, "transactionId": id, "addInstancedItems": [],
        "addStackedItems": or_empty(lists.add), "removeInstancedItems": [], "removeStackedItems": or_empty(lists.remove), "saveInstancedItems": []});
    app.post("/inventory", req().gs().json(body)).await
}

/// The reply's stacks, by catalogId.
fn reported(reply: &Reply) -> Vec<Value> {
    assert_eq!(reply.status, 200, "{}", reply.text);
    let mut items: Vec<Value> = reply.json["updatedStackedItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| json!({"catalogId": i["catalogId"], "quantity": i["quantity"]}))
        .collect();
    items.sort_by(|a, b| a["catalogId"].as_str().cmp(&b["catalogId"].as_str()));
    items
}

fn stack(catalog: &str, quantity: i64) -> Value {
    json!({"catalogId": catalog, "quantity": quantity})
}

#[tokio::test(flavor = "multi_thread")]
async fn grants_a_stacked_item() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    let reply = transaction(
        &app,
        &u,
        &c,
        "tx-grant-1",
        lists(json!([stack("BREAK_TEST_HIDE", 5)]), Value::Null),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_eq!(app.stack_quantity(&c, "BREAK_TEST_HIDE"), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn accumulates_repeated_grants_of_the_same_catalog_id() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "tx-acc-1",
        lists(json!([stack("ORB_TEST", 3)]), Value::Null),
    )
    .await;
    transaction(
        &app,
        &u,
        &c,
        "tx-acc-2",
        lists(json!([stack("ORB_TEST", 4)]), Value::Null),
    )
    .await;
    assert_eq!(app.stack_quantity(&c, "ORB_TEST"), 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn removes_a_stacked_item() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "tx-rem-1",
        lists(json!([stack("ORB_TEST", 10)]), Value::Null),
    )
    .await;
    transaction(
        &app,
        &u,
        &c,
        "tx-rem-2",
        lists(Value::Null, json!([stack("ORB_TEST", 4)])),
    )
    .await;
    assert_eq!(app.stack_quantity(&c, "ORB_TEST"), 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_transaction_against_a_character_the_account_does_not_own_and_writes_nothing() {
    let app = TestApp::start().await;
    let (mine, _) = app.make_player().await;
    let (_, theirs) = app.make_player().await;
    let reply = transaction(
        &app,
        &mine,
        &theirs,
        "tx-cross-1",
        lists(json!([stack("ORB_TEST", 1)]), Value::Null),
    )
    .await;
    assert_eq!(reply.status, 403);
    assert_eq!(app.stack_quantity(&theirs, "ORB_TEST"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replayed_transaction_id_grants_only_once() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "tx-replay-1",
        lists(json!([stack("ORB_TEST", 5)]), Value::Null),
    )
    .await;
    let replay = transaction(
        &app,
        &u,
        &c,
        "tx-replay-1",
        lists(json!([stack("ORB_TEST", 5)]), Value::Null),
    )
    .await;
    assert_eq!(replay.status, 200);
    assert_eq!(app.stack_quantity(&c, "ORB_TEST"), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_inventory_refuse_overspend_1_refuses_to_remove_more_of_a_stacked_item_than_is_held() {
    let app = TestApp::with_env(&[("INVENTORY_REFUSE_OVERSPEND", "1")]).await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "tx-over-1",
        lists(json!([stack("ORB_TEST", 2)]), Value::Null),
    )
    .await;
    let reply = transaction(
        &app,
        &u,
        &c,
        "tx-over-2",
        lists(Value::Null, json!([stack("ORB_TEST", 5)])),
    )
    .await;
    assert_eq!(reply.status, 409);
    assert_eq!(
        app.stack_quantity(&c, "ORB_TEST"),
        2,
        "a refused removal leaves the held quantity untouched"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn by_default_clamps_that_removal_at_0_and_still_applies_the_rest_of_the_transaction() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "tx-over-3",
        lists(json!([stack("ORB_TEST", 2)]), Value::Null),
    )
    .await;
    let reply = transaction(
        &app,
        &u,
        &c,
        "tx-over-4",
        lists(json!([stack("HUNT_REWARD", 3)]), json!([stack("ORB_TEST", 5)])),
    )
    .await;
    assert_eq!(reported(&reply), [stack("HUNT_REWARD", 3), stack("ORB_TEST", 0)]);
    assert_eq!(app.stack_quantity(&c, "ORB_TEST"), 0);
    assert_eq!(app.stack_quantity(&c, "HUNT_REWARD"), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_negative_grant_quantity_and_still_accepts_a_quantity_of_0() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "tx-neg-1",
        lists(json!([stack("ORB_TEST", 10)]), Value::Null),
    )
    .await;
    let negative = transaction(
        &app,
        &u,
        &c,
        "tx-neg-2",
        lists(json!([stack("ORB_TEST", -8)]), Value::Null),
    )
    .await;
    assert_eq!(negative.status, 400);
    assert_eq!(app.stack_quantity(&c, "ORB_TEST"), 10);
    let zero = transaction(
        &app,
        &u,
        &c,
        "tx-neg-3",
        lists(json!([stack("ORB_TEST", 0)]), Value::Null),
    )
    .await;
    assert_eq!(zero.status, 200, "a reward of 0 is not an error");
    assert_eq!(app.stack_quantity(&c, "ORB_TEST"), 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn crafting_style_removals_report_the_final_quantity_of_every_changed_stack() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "seed-craft",
        lists(
            json!([stack("CURRENCY_NOTES", 4160), stack("MAT_TEST_ORE", 14)]),
            Value::Null,
        ),
    )
    .await;
    let reply = transaction(
        &app,
        &u,
        &c,
        "craft-1",
        lists(
            json!([stack("MAT_TEST_PART", 1)]),
            json!([stack("CURRENCY_NOTES", 10), stack("MAT_TEST_ORE", 1)]),
        ),
    )
    .await;
    assert_eq!(
        reported(&reply),
        [
            stack("CURRENCY_NOTES", 4150),
            stack("MAT_TEST_ORE", 13),
            stack("MAT_TEST_PART", 1)
        ]
    );
    assert_eq!(app.stack_quantity(&c, "CURRENCY_NOTES"), 4150);
    assert_eq!(app.stack_quantity(&c, "MAT_TEST_ORE"), 13);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stack_used_up_entirely_is_reported_at_zero_not_omitted() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "seed-deplete",
        lists(json!([stack("MAT_TEST_ORE", 2)]), Value::Null),
    )
    .await;
    let reply = transaction(
        &app,
        &u,
        &c,
        "deplete-1",
        lists(Value::Null, json!([stack("MAT_TEST_ORE", 2)])),
    )
    .await;
    assert_eq!(reported(&reply), [stack("MAT_TEST_ORE", 0)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stack_touched_more_than_once_is_reported_once_at_its_final_quantity() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "seed-twice",
        lists(json!([stack("MAT_TEST_ORE", 5)]), Value::Null),
    )
    .await;
    let reply = transaction(
        &app,
        &u,
        &c,
        "twice-1",
        lists(
            json!([stack("MAT_TEST_ORE", 1), stack("MAT_TEST_ORE", 1)]),
            json!([stack("MAT_TEST_ORE", 3)]),
        ),
    )
    .await;
    assert_eq!(reported(&reply), [stack("MAT_TEST_ORE", 4)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retried_transaction_returns_the_same_reported_quantities_not_the_current_ones_and_does_not_deduct_again() {
    let app = TestApp::start().await;
    let (u, c) = app.make_player().await;
    transaction(
        &app,
        &u,
        &c,
        "seed-retry",
        lists(json!([stack("CURRENCY_NOTES", 100)]), Value::Null),
    )
    .await;
    let first = transaction(
        &app,
        &u,
        &c,
        "retry-craft",
        lists(Value::Null, json!([stack("CURRENCY_NOTES", 30)])),
    )
    .await;
    transaction(
        &app,
        &u,
        &c,
        "later-spend",
        lists(Value::Null, json!([stack("CURRENCY_NOTES", 5)])),
    )
    .await;
    let retry = transaction(
        &app,
        &u,
        &c,
        "retry-craft",
        lists(Value::Null, json!([stack("CURRENCY_NOTES", 30)])),
    )
    .await;
    assert_eq!(reported(&first), [stack("CURRENCY_NOTES", 70)]);
    assert_eq!(
        reported(&retry),
        reported(&first),
        "a replay answers with what the original reported"
    );
    assert_eq!(app.stack_quantity(&c, "CURRENCY_NOTES"), 65);
}
