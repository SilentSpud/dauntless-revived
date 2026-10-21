//! `bountiesported.test.ts`: Harmonic's bounty and cooldown cases (github.com/Harmonicrain/Undaunted
//! 895f7c7, test/huntpass-progress.test.js) against ours. Only a game server writes bounties and
//! cooldowns; the inverted and skipped cases are listed in the TS file.

use metagame::bounties;
use metagame::progression::Caller;
use serde_json::{Value, json};

use crate::common::{TestApp, req};

async fn save(app: &TestApp, user: &str, body: Value) {
    let user = user.to_owned();
    let reply = app
        .db(move |conn| bounties::set(conn, &user, Some(&body), Caller::GameServer))
        .await;
    assert_eq!(reply.status, 200);
}

async fn board(app: &TestApp, user: &str) -> Value {
    let user = user.to_owned();
    app.db(move |conn| bounties::reply(conn, &user)).await["payload"].clone()
}

fn draft(previous: &[&str], counts: [i64; 3], choices: &[&str]) -> Value {
    json!({"current_draft_choices": choices, "previous_draft_selections": previous,
        "bronze_count": counts[0], "silver_count": counts[1], "gold_count": counts[2]})
}

fn ids(board: &Value) -> Vec<Value> {
    board["bounties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["bounty_id"].clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn bounties_persist_across_a_read() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(board(&app, &user).await["bounties"], json!([]));
    save(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "BountyA", "progress": 3}], "draft_data": draft(&["X"], [1, 0, 0], &[])}),
    )
    .await;
    let stored = board(&app, &user).await;
    assert_eq!(ids(&stored), [json!("BountyA")]);
    assert_eq!(stored["draft_data"]["bronze_count"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn drafting_several_bounties_keeps_all_of_them() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let drafted = [
        ("Bounty_Bronze_KillAxePike", 0),
        ("Bounty_Bronze_HuntsSwordTwo", 1),
        ("Bounty_Silver_HuntsSwordTwo", 2),
    ];
    for (n, (id, slot)) in drafted.iter().enumerate() {
        let previous: Vec<&str> = drafted[..=n].iter().map(|(id, _)| *id).collect();
        let body = json!({"bounties": [{"bounty_id": id, "slot_index": slot, "objectives": [{"objective_id": id, "progress": 0}], "update_version": 0}],
            "draft_data": draft(&previous, [7, 2, 1], &[])});
        save(&app, &user, body).await;
    }
    let stored = board(&app, &user).await;
    let mut slots: Vec<i64> = stored["bounties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["slot_index"].as_i64().unwrap())
        .collect();
    slots.sort();
    assert_eq!(slots, [0, 1, 2], "all three drafted bounties survive");
    assert_eq!(
        stored["draft_data"]["previous_draft_selections"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "draft_data is the full state and replaces"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refresh_of_the_draft_options_does_not_wipe_the_held_board() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    save(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "Held", "slot_index": 0}], "draft_data": draft(&["Held"], [7, 3, 1], &[])}),
    )
    .await;
    // Between drafts the game server posts an empty bounties list with its new draft choices
    save(
        &app,
        &user,
        json!({"bounties": [], "draft_data": draft(&["Held"], [6, 3, 1], &["A", "B", "C"])}),
    )
    .await;
    let stored = board(&app, &user).await;
    assert_eq!(ids(&stored).len(), 1, "the held bounty survives a draft refresh");
    assert_eq!(stored["draft_data"]["current_draft_choices"], json!(["A", "B", "C"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn posting_a_bounty_again_updates_it_in_place() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    for value in [0, 5] {
        save(&app, &user, json!({"bounties": [{"bounty_id": "Tracked", "slot_index": 0, "objectives": [{"objective_id": "Tracked", "progress": value}]}]})).await;
    }
    let stored = board(&app, &user).await;
    assert_eq!(ids(&stored).len(), 1, "progress updates must not duplicate the bounty");
    assert_eq!(stored["bounties"][0]["objectives"][0]["progress"], 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_board_is_served_inside_the_code_message_payload_envelope() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    save(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "Wire", "slot_index": 0}], "draft_data": draft(&["Wire"], [7, 3, 1], &[])}),
    )
    .await;
    let reply = app.get(&format!("/bounty/{user}"), req().bearer(&user)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json["message"], "OK");
    assert_eq!(
        reply.json.get("bounties"),
        None,
        "the board sits inside payload, not also at the root"
    );
    let payload = &reply.json["payload"];
    assert_eq!(payload["bounties"][0]["bounty_id"], "Wire");
    assert_eq!(payload["draft_data"]["bronze_count"], 7);
    // Neither key occurs in the 1.4.4 executable
    assert_eq!(
        (payload.get("draft_data_daily"), payload.get("draft_data_weekly")),
        (None, None)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn abandoning_a_bounty_removes_it_instead_of_failing() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    save(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "Keep", "slot_index": 0}, {"bounty_id": "Abandon", "slot_index": 1}]}),
    )
    .await;
    let path = format!("/bounty/delete/{user}");
    let body = json!({"bounty_ids": ["Abandon"]});
    assert_eq!(
        app.post(&path, req().bearer(&user).json(body.clone())).await.status,
        403,
        "not a player's own client"
    );
    assert_eq!(app.post(&path, req().gs().bearer(&user).json(body)).await.status, 200);
    assert_eq!(ids(&board(&app, &user).await), [json!("Keep")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn abandoning_an_unknown_bounty_is_harmless() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    save(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "Held", "slot_index": 0}]}),
    )
    .await;
    let reply = app
        .post(
            &format!("/bounty/delete/{user}"),
            req().gs().json(json!({"bounty_ids": ["NeverHeld"]})),
        )
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(ids(&board(&app, &user).await).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_post_with_an_empty_board_and_zeroed_draft_data_keeps_the_bounties_held() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let held = json!({"bounties": [{"bounty_id": "Held_A", "slot_index": 2}, {"bounty_id": "Held_B", "slot_index": 3}],
        "draft_data": draft(&["Held_A", "Held_B"], [7, 2, 1], &[])});
    save(&app, &user, held).await;
    save(
        &app,
        &user,
        json!({"bounties": [], "draft_data": draft(&[], [0, 0, 0], &[])}),
    )
    .await;
    let stored = board(&app, &user).await;
    assert_eq!(ids(&stored), [json!("Held_A"), json!("Held_B")]);
    assert_eq!(stored["draft_data"], draft(&[], [0, 0, 0], &[]));
    // Claims and abandons go through /bounty/delete
    save(
        &app,
        &user,
        json!({"bounties": [{"bounty_id": "Fresh", "slot_index": 0}]}),
    )
    .await;
    assert_eq!(
        ids(&board(&app, &user).await),
        [json!("Fresh"), json!("Held_A"), json!("Held_B")]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bounty_game_data_serves_no_definitions_and_keeps_its_settings() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let reply = app.get("/bounty/game-data", req().bearer(&user)).await;
    assert_eq!(reply.status, 200);
    let payload = &reply.json["payload"];
    assert_eq!(payload["bounty_data"], json!([]));
    assert_eq!(
        json!([
            payload["max_slots"],
            payload["bounty_token_id"],
            payload["premium_bounty_token_id"],
            payload["new_season_reset_bounties"]
        ]),
        json!([4, "TOKEN_BOUNTY_DRAFT", "TOKEN_BOUNTY_DRAFT_PREMIUM", false])
    );
}

async fn batch(app: &TestApp, user: &str, entries: Value) -> u16 {
    app.call(
        "PUT",
        &format!("/cooldown/batch/{user}"),
        req().gs().json(json!({"cooldowns": entries})),
    )
    .await
    .status
}

async fn cooldowns(app: &TestApp, user: &str) -> Value {
    app.get(&format!("/cooldown/{user}"), req().bearer(user)).await.json["payload"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn cooldowns_persist_so_the_bounty_token_marker_survives_a_login() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let entries = json!([{"cooldown_id": "BountyTokens", "cooldown_started_date": "2026-09-21T13:38:13.654Z"}]);
    assert_eq!(batch(&app, &user, entries).await, 200);
    // GET is an id -> date map, not the {cooldowns: [...]} of the batch write
    let got = app.get(&format!("/cooldown/{user}"), req().bearer(&user)).await;
    assert_eq!(got.json["message"], "OK");
    assert_eq!(got.json["payload"], json!({"BountyTokens": "2026-09-21T13:38:13.654Z"}));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_updates_the_cooldowns_it_names_and_keeps_the_rest() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    batch(
        &app,
        &user,
        json!([{"cooldown_id": "A", "cooldown_started_date": "2026-01-01T00:00:00.000Z"},
        {"cooldown_id": "B", "cooldown_started_date": "2026-01-01T00:00:00.000Z"}]),
    )
    .await;
    batch(
        &app,
        &user,
        json!([{"cooldown_id": "A", "cooldown_started_date": "2026-06-01T00:00:00.000Z"}]),
    )
    .await;
    let by_id = cooldowns(&app, &user).await;
    assert_eq!(by_id["A"], "2026-06-01T00:00:00.000Z", "a named cooldown is updated");
    assert_eq!(by_id["B"], "2026-01-01T00:00:00.000Z", "an unnamed cooldown is kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_batch_route_is_not_taken_by_the_start_cooldown_route() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    batch(
        &app,
        &user,
        json!([{"cooldown_id": "Real", "cooldown_started_date": "2026-01-01T00:00:00.000Z"}]),
    )
    .await;
    let keys: Vec<String> = cooldowns(&app, &user)
        .await
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["Real"]);
    assert_eq!(
        app.get("/cooldown/batch", req().gs()).await.json["payload"],
        json!({}),
        "nothing stored under an account called batch"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn starting_a_cooldown_by_id_records_it_now() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let path = format!("/cooldown/{user}/DailyThing");
    assert_eq!(app.call("PUT", &path, req().bearer(&user)).await.status, 403);
    assert_eq!(app.post(&path, req().gs()).await.status, 404, "no POST");
    let before = metagame::time::now_ms();
    assert_eq!(app.call("PUT", &path, req().gs()).await.status, 200);
    let got = cooldowns(&app, &user).await;
    let keys: Vec<&String> = got.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["DailyThing"]);
    assert!(metagame::js::date_parse(got["DailyThing"].as_str().unwrap()).unwrap() >= before - 1000);
}
