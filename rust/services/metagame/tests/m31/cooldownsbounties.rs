//! `cooldownsbounties.test.ts`: the cooldown map and the bounty board.

use metagame::progression::Caller;
use metagame::{bounties, cooldowns};
use serde_json::{Value, json};

use crate::common::TestApp;

async fn cooldown_reply(app: &TestApp, user: &str) -> Value {
    let user = user.to_owned();
    app.db(move |conn| cooldowns::reply(conn, &user)).await
}

async fn batch(app: &TestApp, user: &str, body: Value) -> u16 {
    let user = user.to_owned();
    app.db(move |conn| cooldowns::set_batch(conn, &user, Some(&body), Caller::GameServer))
        .await
        .status
}

async fn start(app: &TestApp, user: &str, id: &str) -> u16 {
    let (user, id) = (user.to_owned(), id.to_owned());
    app.db(move |conn| cooldowns::start(conn, &user, &id, Caller::GameServer))
        .await
        .status
}

async fn board(app: &TestApp, user: &str) -> Value {
    let user = user.to_owned();
    app.db(move |conn| bounties::reply(conn, &user)).await
}

async fn set(app: &TestApp, user: &str, body: Value) -> u16 {
    let user = user.to_owned();
    app.db(move |conn| bounties::set(conn, &user, Some(&body), Caller::GameServer))
        .await
        .status
}

async fn delete(app: &TestApp, user: &str, body: Value) -> u16 {
    let user = user.to_owned();
    app.db(move |conn| bounties::delete(conn, &user, Some(&body), Caller::GameServer))
        .await
        .status
}

fn bounty(id: &str, slot: i64, version: i64, progress: i64) -> Value {
    json!({"bounty_id": id, "premium_bounty": false, "slot_index": slot, "objectives": [{"objective_id": format!("{id}_OBJ"), "progress": progress}],
        "drafted_timestamp": "2026-09-21T14:31:05.123Z", "update_version": version})
}

fn ids(board: &Value) -> Vec<&str> {
    board["payload"]["bounties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["bounty_id"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_batch_put_is_stored_exactly_as_sent_and_read_back_as_an_object_map_with_an_int_code() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        cooldown_reply(&app, &user).await,
        json!({"code": 200, "message": "OK", "payload": {}})
    );
    let first = json!({"cooldowns": [{"cooldown_id": "bounty_tokens_season09b", "cooldown_started_date": "2026-09-21T14:10:39.154Z"}]});
    assert_eq!(batch(&app, &user, first).await, 200);
    batch(
        &app,
        &user,
        json!({"cooldowns": [
            {"cooldown_id": "bounty_tokens_season09b", "cooldown_started_date": "2026-09-22T08:00:00.000Z"},
            {"cooldown_id": "arena_rank_1", "cooldown_started_date": "2026-09-21T15:00:00.000Z"}
        ]}),
    )
    .await;
    assert_eq!(
        cooldown_reply(&app, &user).await["payload"],
        json!({"bounty_tokens_season09b": "2026-09-22T08:00:00.000Z", "arena_rank_1": "2026-09-21T15:00:00.000Z"})
    );
    assert_eq!(batch(&app, &user, json!({"nothing": 1})).await, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_start_put_no_body_stamps_now_old_harvest_cooldowns_are_pruned_bounty_tokens_never() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let old = metagame::time::iso(metagame::time::now_ms() - 3 * 24 * 3600 * 1000);
    batch(
        &app,
        &user,
        json!({"cooldowns": [
            {"cooldown_id": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "cooldown_started_date": old},
            {"cooldown_id": "bounty_tokens_season09b", "cooldown_started_date": old}
        ]}),
    )
    .await;
    let before = metagame::time::now_ms();
    assert_eq!(start(&app, &user, "1E3ADD6243A0C2A96C9F8B83AABF5C85").await, 200);
    let map = cooldown_reply(&app, &user).await["payload"].clone();
    let mut keys: Vec<&String> = map.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["1E3ADD6243A0C2A96C9F8B83AABF5C85", "bounty_tokens_season09b"]);
    let stamped = metagame::js::date_parse(map["1E3ADD6243A0C2A96C9F8B83AABF5C85"].as_str().unwrap()).unwrap();
    assert!(stamped >= before - 1000);
    assert_eq!(map["bounty_tokens_season09b"], old);
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_accounts_apart() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    start(&app, &a, "X").await;
    assert_eq!(cooldown_reply(&app, &b).await["payload"], json!({}));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_board_for_a_new_account_with_draft_data() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        board(&app, &user).await,
        json!({"code": "OK", "message": "OK", "payload": {"bounties": [], "draft_data": {"current_draft_choices": [], "previous_draft_selections": [], "bronze_count": 0, "silver_count": 0, "gold_count": 0}}})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_upserts_select_progress_draft_saves_with_empty_bounty_lists_never_remove_anything() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    // Select a drafted bounty, progress it (no draft_data), add a second, then a draft save with no bounties
    set(&app, &user, json!({"bounties": [bounty("Bounty_Bronze_KillRadiant", 0, 0, 0)], "draft_data": {"current_draft_choices": [], "previous_draft_selections": ["Bounty_Bronze_KillRadiant"], "bronze_count": 1, "silver_count": 0, "gold_count": 0}})).await;
    set(
        &app,
        &user,
        json!({"bounties": [bounty("Bounty_Bronze_KillRadiant", 0, 1, 2)]}),
    )
    .await;
    set(&app, &user, json!({"bounties": [bounty("Bounty_Silver_X", 1, 0, 0)]})).await;
    set(&app, &user, json!({"bounties": [], "draft_data": {"current_draft_choices": ["A", "B", "C"], "previous_draft_selections": ["Bounty_Bronze_KillRadiant"], "bronze_count": 1, "silver_count": 1, "gold_count": 0}})).await;
    let payload = board(&app, &user).await["payload"].clone();
    let summary: Vec<Value> = payload["bounties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            json!([
                b["bounty_id"],
                b["slot_index"],
                b["update_version"],
                b["objectives"][0]["progress"]
            ])
        })
        .collect();
    assert_eq!(
        summary,
        [
            json!(["Bounty_Bronze_KillRadiant", 0, 1, 2]),
            json!(["Bounty_Silver_X", 1, 0, 0])
        ]
    );
    assert_eq!(payload["draft_data"]["current_draft_choices"], json!(["A", "B", "C"]));
    assert_eq!(payload["bounties"][0], bounty("Bounty_Bronze_KillRadiant", 0, 1, 2));
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_a_newer_stored_bounty_over_a_stale_write_and_a_new_bounty_takes_over_its_slot() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    set(&app, &user, json!({"bounties": [bounty("Bounty_A", 0, 5, 4)]})).await;
    assert_eq!(
        set(&app, &user, json!({"bounties": [bounty("Bounty_A", 0, 3, 1)]})).await,
        200
    );
    assert_eq!(board(&app, &user).await["payload"]["bounties"][0]["update_version"], 5);
    set(&app, &user, json!({"bounties": [bounty("Bounty_B", 0, 0, 0)]})).await;
    assert_eq!(ids(&board(&app, &user).await), ["Bounty_B"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_removes_claimed_or_abandoned_bounties_unknown_ids_are_fine() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    set(
        &app,
        &user,
        json!({"bounties": [bounty("Bounty_A", 0, 0, 0), bounty("Bounty_B", 1, 0, 0)]}),
    )
    .await;
    assert_eq!(
        delete(&app, &user, json!({"bounty_ids": ["Bounty_A", "Bounty_Unknown"]})).await,
        200
    );
    assert_eq!(ids(&board(&app, &user).await), ["Bounty_B"]);
    assert_eq!(delete(&app, &user, json!({})).await, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn always_sends_drafted_timestamp_even_if_the_stored_element_lacks_it() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    set(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "Bounty_NoTime", "slot_index": 2, "objectives": [], "update_version": 0}]}),
    )
    .await;
    let stamp = board(&app, &user).await["payload"]["bounties"][0]["drafted_timestamp"].clone();
    assert!(
        regex::Regex::new(r"^\d{4}-\d\d-\d\dT")
            .unwrap()
            .is_match(stamp.as_str().unwrap())
    );
}
