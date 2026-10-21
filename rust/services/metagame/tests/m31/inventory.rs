//! `inventory.test.ts`, the transaction-level part: Node's `RunInventoryTransaction` is a game
//! server's POST /inventory here, its `{success: false, error}` the status it answers. The pure
//! helpers' tests (`ParseStackQuantity`, `ApplyStackedChanges`, the retry keys) are unit tests in
//! `src/inventory.rs`.

use metagame::inventory::set_daily_tokens_once;
use serde_json::{Value, json};

use crate::common::{Reply, TestApp, req};

fn guid() -> String {
    metagame::crypto::random_uuid().replace('-', "").to_uppercase()
}

fn stack(catalog: &str, quantity: i64) -> Value {
    json!({"catalogId": catalog, "quantity": quantity})
}

struct Tx<'a> {
    user: &'a str,
    character: &'a str,
    id: String,
    body: Value,
}

impl<'a> Tx<'a> {
    fn new(user: &'a str, character: &'a str, id: &str) -> Self {
        Self {
            user,
            character,
            id: id.to_owned(),
            body: json!({}),
        }
    }

    fn with(mut self, list: &str, items: Value) -> Self {
        self.body[list] = items;
        self
    }

    fn json(&self, source: &str) -> Value {
        let mut body = self.body.clone();
        body["accountId"] = json!(self.user);
        body["characterId"] = json!(self.character);
        body["transactionId"] = json!(self.id);
        body["source"] = json!(source);
        body
    }

    /// As the game server, with the source the Node tests passed ("UnitTest" unless given).
    async fn run(&self, app: &TestApp) -> Reply {
        self.run_from(app, "UnitTest").await
    }

    async fn run_from(&self, app: &TestApp, source: &str) -> Reply {
        app.post("/inventory", req().gs().json(self.json(source))).await
    }
}

async fn grant(app: &TestApp, user: &str, character: &str, id: &str, add: Value, remove: Value) -> Reply {
    Tx::new(user, character, id)
        .with("addStackedItems", add)
        .with("removeStackedItems", remove)
        .run(app)
        .await
}

fn log_count(app: &TestApp, character: &str) -> i64 {
    app.count("inventorylog", "characterId = ?1", &[&character])
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_the_same_transaction_twice_but_changes_the_inventory_once_answering_the_same_both_times() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let id = guid();
    let first = grant(&app, &user, &character, &id, json!([stack("BREAK_HIDE", 3)]), json!([])).await;
    let second = grant(&app, &user, &character, &id, json!([stack("BREAK_HIDE", 3)]), json!([])).await;
    assert_eq!((first.status, second.status), (200, 200));
    assert_eq!(second.text, first.text);
    assert_eq!(app.stack_quantity(&character, "BREAK_HIDE"), 3);
    assert_eq!(app.count("inventorytransactions", "characterId = ?1", &[&character]), 1);
    assert_eq!(log_count(&app, &character), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn applies_concurrent_copies_of_one_transaction_once() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let id = guid();
    let copies = (0..5).map(|_| grant(&app, &user, &character, &id, json!([stack("ORB", 2)]), json!([])));
    let replies = futures_util::future::join_all(copies).await;
    assert!(replies.iter().all(|r| r.status == 200));
    assert_eq!(app.stack_quantity(&character, "ORB"), 2);
    assert_eq!(log_count(&app, &character), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_a_reused_id_with_a_different_body_as_a_new_transaction() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let id = guid();
    grant(&app, &user, &character, &id, json!([stack("ORB", 2)]), json!([])).await;
    assert_eq!(
        grant(&app, &user, &character, &id, json!([stack("ORB", 5)]), json!([]))
            .await
            .status,
        200
    );
    assert_eq!(app.stack_quantity(&character, "ORB"), 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn does_not_deduplicate_transactions_without_a_usable_id() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let zeros = "00000000000000000000000000000000";
    grant(&app, &user, &character, zeros, json!([stack("ORB", 1)]), json!([])).await;
    grant(&app, &user, &character, zeros, json!([stack("ORB", 1)]), json!([])).await;
    assert_eq!(app.stack_quantity(&character, "ORB"), 2);
    assert_eq!(app.count("inventorytransactions", "characterId = ?1", &[&character]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_inventory_refuse_overspend_1_refuses_an_overspend_as_a_whole_the_grant_in_the_same_transaction_is_not_applied_either()
 {
    let app = TestApp::with_env(&[("INVENTORY_REFUSE_OVERSPEND", "1")]).await;
    let (user, character) = app.make_player().await;
    grant(&app, &user, &character, &guid(), json!([stack("ORB", 3)]), json!([])).await;
    let log_before = log_count(&app, &character);
    let id = guid();
    let craft = Tx::new(&user, &character, &id)
        .with(
            "addInstancedItems",
            json!([{"catalogId": "WP_AXE", "instanceId": "i-1", "updateVersion": 0}]),
        )
        .with("addStackedItems", json!([stack("NOTES", 100)]))
        .with("removeStackedItems", json!([stack("ORB", 5)]));
    let refused = craft.run(&app).await;
    assert_eq!(refused.status, 409, "{}", refused.text);
    assert_eq!(app.stacks(&character), [stack("ORB", 3)]);
    assert_eq!(log_count(&app, &character), log_before);
    assert_eq!(app.count("inventorytransactions", "transactionId = ?1", &[&id]), 0);
    // A grant spent in the same transaction is not an overspend
    let spend = grant(
        &app,
        &user,
        &character,
        &guid(),
        json!([stack("TOKEN", 5)]),
        json!([stack("TOKEN", 2)]),
    )
    .await;
    assert_eq!(spend.status, 200);
    assert_eq!(app.stack_quantity(&character, "TOKEN"), 3);
    // Refusals are not stored, so the same request succeeds once the player holds enough
    grant(&app, &user, &character, &guid(), json!([stack("ORB", 2)]), json!([])).await;
    assert_eq!(craft.run(&app).await.status, 200);
    assert_eq!(app.stacks(&character), [stack("TOKEN", 3), stack("NOTES", 100)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn by_default_runs_an_overspending_transaction_the_old_way_the_removal_is_clamped_the_rest_applied() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    grant(&app, &user, &character, &guid(), json!([stack("ORB", 1)]), json!([])).await;
    let reply = grant(
        &app,
        &user,
        &character,
        &guid(),
        json!([stack("REWARD", 3)]),
        json!([stack("ORB", 2), stack("NEVER_HELD", 1)]),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_eq!(app.stacks(&character), [stack("REWARD", 3)]);
    assert_eq!(
        reply.json["updatedStackedItems"],
        json!([stack("REWARD", 3), stack("ORB", 0), stack("NEVER_HELD", 0)])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_reply_shape_echoed_lists_stay_as_sent_missing_ones_stay_missing() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let item = json!({"catalogId": "WP_AXE", "instanceId": "i-2", "updateVersion": 0});
    let reply = Tx::new(&user, &character, &guid())
        .with("addInstancedItems", json!([item]))
        .with("addStackedItems", json!([stack("ORB", 1)]))
        .run(&app)
        .await;
    assert_eq!(
        reply.json,
        json!({"createdInstancedItems": [item], "updatedInstancedItems": [], "updatedStackedItems": [stack("ORB", 1)]})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_another_player_s_character() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (_, b_character) = app.make_player().await;
    let reply = grant(&app, &a, &b_character, &guid(), json!([stack("ORB", 1)]), json!([])).await;
    assert_eq!(reply.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_every_item_change_and_the_log_refuses_update_and_delete() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let id = guid();
    let axe = |version: i64| json!([{"catalogId": "WP_AXE", "instanceId": "i-3", "updateVersion": version}]);
    let first = Tx::new(&user, &character, &id)
        .with("addInstancedItems", axe(0))
        .with("addStackedItems", json!([stack("ORB", 4)]));
    assert_eq!(first.run(&app).await.status, 200);
    // Upstream rule, unchanged: removing an instanced item needs a newer updateVersion than the stored one
    let stale = Tx::new(&user, &character, &guid())
        .with("removeInstancedItems", axe(0))
        .with("removeStackedItems", json!([stack("ORB", 1)]));
    assert_eq!(stale.run(&app).await.status, 409);
    let newer = Tx::new(&user, &character, &guid())
        .with("removeInstancedItems", axe(1))
        .with("removeStackedItems", json!([stack("ORB", 1)]));
    assert_eq!(newer.run(&app).await.status, 200);
    let rows = app.rows(
        "SELECT transactionId, caller, source, operation, catalogId, instanceId, quantityChange, quantityAfter FROM inventorylog WHERE characterId = ?1 ORDER BY id",
        &[&character],
    );
    let summary: Vec<Value> = rows
        .iter()
        .map(|r| json!([r["operation"], r["catalogId"], r["quantityChange"], r["quantityAfter"]]))
        .collect();
    assert_eq!(
        summary,
        [
            json!(["add", "WP_AXE", 1, null]),
            json!(["add", "ORB", 4, 4]),
            json!(["remove", "WP_AXE", -1, null]),
            json!(["remove", "ORB", -1, 3]),
        ]
    );
    assert_eq!(
        (
            rows[0]["transactionId"].clone(),
            rows[0]["caller"].clone(),
            rows[0]["source"].clone()
        ),
        (json!(id), json!("gameserver"), json!("UnitTest"))
    );
    for sql in [
        "UPDATE inventorylog SET quantityChange = 99",
        "DELETE FROM inventorylog",
    ] {
        let error = rusqlite::Connection::open(&app.db)
            .unwrap()
            .execute(sql, [])
            .unwrap_err()
            .to_string();
        assert!(error.contains("append-only"), "{sql}: {error}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lands_the_daily_patrol_bonus_refill_on_10_whatever_the_game_server_asks_for() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let run = |source: &'static str, add: Value, remove: Value| {
        let (user, character) = (user.clone(), character.clone());
        let app = &app;
        async move {
            Tx::new(&user, &character, &guid())
                .with("addStackedItems", add)
                .with("removeStackedItems", remove)
                .run_from(app, source)
                .await
        }
    };
    run(
        "PERIODIC_REWARD",
        json!([stack("TOKEN_DAILY_PATROL_BONUS", 6)]),
        json!([]),
    )
    .await;
    assert_eq!(app.stack_quantity(&character, "TOKEN_DAILY_PATROL_BONUS"), 10);
    run("UnitTest", json!([]), json!([stack("TOKEN_DAILY_PATROL_BONUS", 7)])).await;
    run(
        "PERIODIC_REWARD",
        json!([stack("TOKEN_DAILY_PATROL_BONUS", 3)]),
        json!([]),
    )
    .await;
    assert_eq!(app.stack_quantity(&character, "TOKEN_DAILY_PATROL_BONUS"), 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn lands_bounty_token_grants_on_6_and_never_above_it() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    for (source, quantity) in [
        ("UBountyComponent::ServerInitializeBounties", 4),
        ("UBountyComponent::DailyGrant", 6),
    ] {
        Tx::new(&user, &character, &guid())
            .with("addStackedItems", json!([stack("TOKEN_BOUNTY_DRAFT", quantity)]))
            .run_from(&app, source)
            .await;
        assert_eq!(app.stack_quantity(&character, "TOKEN_BOUNTY_DRAFT"), 6, "{source}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn does_not_top_up_other_sources_and_refuses_forged_player_token_grants() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    Tx::new(&user, &character, &guid())
        .with(
            "addStackedItems",
            json!([stack("TOKEN_BOUNTY_DRAFT", 2), stack("TOKEN_DAILY_PATROL_BONUS", 1)]),
        )
        .run_from(&app, "HuntReward")
        .await;
    let forged = Tx::new(&user, &character, &guid())
        .with("addStackedItems", json!([stack("TOKEN_BOUNTY_DRAFT", 3)]))
        .json("UBountyComponent::ServerInitializeBounties");
    assert_eq!(
        app.post("/inventory", req().bearer(&user).json(forged)).await.status,
        403
    );
    assert_eq!(app.stack_quantity(&character, "TOKEN_BOUNTY_DRAFT"), 2);
    assert_eq!(app.stack_quantity(&character, "TOKEN_DAILY_PATROL_BONUS"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn sets_every_character_to_the_daily_amounts_once_adding_missing_stacks_and_keeping_the_rest() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    grant(
        &app,
        &user,
        &character,
        &guid(),
        json!([stack("TOKEN_DAILY_PATROL_BONUS", 3), stack("BREAK_HIDE", 7)]),
        json!([]),
    )
    .await;
    let settings = app.state.inventory;
    let first = app.db(move |conn| set_daily_tokens_once(conn, &settings)).await;
    assert!(first.unwrap_or(0) >= 1);
    assert_eq!(app.stack_quantity(&character, "TOKEN_DAILY_PATROL_BONUS"), 10);
    assert_eq!(app.stack_quantity(&character, "TOKEN_BOUNTY_DRAFT"), 6);
    assert_eq!(app.stack_quantity(&character, "BREAK_HIDE"), 7);
    grant(
        &app,
        &user,
        &character,
        &guid(),
        json!([]),
        json!([stack("TOKEN_BOUNTY_DRAFT", 4)]),
    )
    .await;
    assert_eq!(app.db(move |conn| set_daily_tokens_once(conn, &settings)).await, None);
    assert_eq!(app.stack_quantity(&character, "TOKEN_BOUNTY_DRAFT"), 2);
}
