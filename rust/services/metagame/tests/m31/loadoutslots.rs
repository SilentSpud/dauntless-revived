//! `loadoutslots.test.ts`: real-mode loadout slots: unlocks, saves by slot index, the active slot.

use metagame::characters::save_history;
use metagame::loadouts::{payload, read_slots, set_active, set_slot, slot_counts, unlock};
use serde_json::{Value, json};

use crate::common::TestApp;

async fn all(app: &TestApp, user: &str, character: &str) -> Value {
    let (u, c) = (user.to_owned(), character.to_owned());
    app.state
        .store
        .write(move |conn| payload(conn, &u, &c, true))
        .await
        .unwrap()
}

fn counts(payload: &Value) -> Value {
    json!([
        payload["num_account_slots"],
        payload["max_account_slots"],
        payload["num_character_slots"],
        payload["max_character_slots"]
    ])
}

async fn unlock_by(app: &TestApp, user: &str, character: &str, n: &str) -> (u16, Option<Value>) {
    let (u, c, n) = (user.to_owned(), character.to_owned(), n.to_owned());
    app.db(move |conn| unlock(conn, &u, &c, &n, "gameserver")).await
}

async fn active(app: &TestApp, user: &str, character: &str, index: &str) -> (u16, Option<Value>) {
    let (u, c, i) = (user.to_owned(), character.to_owned(), index.to_owned());
    app.db(move |conn| set_active(conn, &u, &c, &i, "gameserver")).await
}

async fn save(app: &TestApp, user: &str, character: &str, index: &str, data: &str) -> bool {
    let (h, u, c, i, d) = (
        app.state.history,
        user.to_owned(),
        character.to_owned(),
        index.to_owned(),
        json!(data),
    );
    app.db(move |conn| set_slot(conn, &h, &u, &c, &i, Some(&d), "save"))
        .await
}

/// `GetSlotCountReply`.
async fn slot_count(app: &TestApp, character: &str) -> Value {
    let c = character.to_owned();
    let slots = app.db(move |conn| read_slots(conn, &c)).await;
    json!({"code": null, "message": "OK", "payload": slot_counts(slots)})
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_at_1_character_slot_max_5_plus_the_account_slot_all_four_counts_numbers() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let reply = all(&app, &user, &character).await;
    assert_eq!(counts(&reply), json!([1, 1, 1, 5]));
    assert_eq!(
        (reply["active_index"].clone(), reply["needs_migration"].clone()),
        (json!(0), json!(false))
    );
    assert_eq!(reply["loadouts"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn unlock_3_adds_3_the_game_server_sends_the_difference_and_all_slotcount_and_the_reply_agree() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    all(&app, &user, &character).await;
    let (status, body) = unlock_by(&app, &user, &character, "3").await;
    assert_eq!(status, 200);
    let body = body.unwrap();
    assert_eq!(
        body,
        json!({"code": null, "message": "OK", "payload": {"num_account_slots": 1, "max_account_slots": 1, "num_character_slots": 4, "max_character_slots": 5}})
    );
    assert_eq!(slot_count(&app, &character).await, body);
    assert_eq!(counts(&all(&app, &user, &character).await), json!([1, 1, 4, 5]));
    // What the client computes next: 1 + 3 conditions met - 4 = 0, so no more unlocks
    assert_eq!(1 + 3 - body["payload"]["num_character_slots"].as_i64().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn caps_at_5_character_slots_so_the_account_slot_keeps_the_client_total_at_6() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    unlock_by(&app, &user, &character, "4").await;
    let (_, capped) = unlock_by(&app, &user, &character, "3").await;
    assert_eq!(
        capped.unwrap()["payload"],
        json!({"num_account_slots": 1, "max_account_slots": 1, "num_character_slots": 5, "max_character_slots": 5})
    );
    for bad in ["0", "x", "-1", "1.5"] {
        assert_eq!(unlock_by(&app, &user, &character, bad).await.0, 400, "{bad}");
    }
    assert_eq!(slot_count(&app, &character).await["payload"]["num_character_slots"], 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn saves_any_unlocked_slot_by_its_slot_index_refuses_slots_not_unlocked_yet() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    all(&app, &user, &character).await;
    let slot = |index: i64, flask: &str| json!({"slot_index": index, "flask": flask}).to_string();
    // Two slots (0 and 1) until an unlock
    assert!(!save(&app, &user, &character, "2", &slot(2, "FL_TWO")).await);
    unlock_by(&app, &user, &character, "3").await;
    assert!(save(&app, &user, &character, "2", &slot(2, "FL_TWO")).await);
    assert!(save(&app, &user, &character, "2", &slot(2, "FL_TWO_B")).await);
    assert!(save(&app, &user, &character, "1", &slot(7, "FL_ONE")).await);
    assert!(save(&app, &user, &character, "0", &slot(0, "FL_ZERO")).await);
    assert!(
        save(
            &app,
            &user,
            &character,
            "persistent",
            &json!({"banner": "BN_X"}).to_string()
        )
        .await
    );
    assert!(!save(&app, &user, &character, "4", "[]").await);
    assert!(!save(&app, &user, &character, "5", "{}").await);
    let reply = all(&app, &user, &character).await;
    let shown: Vec<Value> = reply["loadouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| json!([e["slot_index"], e["flask"]]))
        .collect();
    assert_eq!(
        shown,
        [json!([0, "FL_ZERO"]), json!([1, "FL_ONE"]), json!([2, "FL_TWO_B"])]
    );
    assert_eq!(reply["persistent"]["banner"], "BN_X");
    let ids = vec![character.clone()];
    let history = app.db(move |conn| save_history(conn, &ids)).await;
    assert!(history[0]["LoadoutVersions"].as_array().unwrap().len() >= 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn creates_the_loadout_row_when_a_valid_slot_save_arrives_before_the_initial_all_read() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    assert!(
        save(
            &app,
            &user,
            &character,
            "0",
            &json!({"slot_index": 0, "flask": "FL_EARLY"}).to_string()
        )
        .await
    );
    assert!(
        save(
            &app,
            &user,
            &character,
            "persistent",
            &json!({"banner": "BN_EARLY"}).to_string()
        )
        .await
    );
    let reply = all(&app, &user, &character).await;
    assert_eq!(reply["loadouts"][0]["flask"], "FL_EARLY");
    assert_eq!(reply["persistent"]["banner"], "BN_EARLY");
}

#[tokio::test(flavor = "multi_thread")]
async fn supports_all_six_client_slots_without_advertising_a_seventh() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    all(&app, &user, &character).await;
    unlock_by(&app, &user, &character, "9").await;
    for index in 0..6 {
        let data = json!({"slot_index": index, "custom_name": format!("Slot {}", index + 1)}).to_string();
        assert!(
            save(&app, &user, &character, &index.to_string(), &data).await,
            "{index}"
        );
    }
    assert!(!save(&app, &user, &character, "6", &json!({"slot_index": 6}).to_string()).await);
    let reply = all(&app, &user, &character).await;
    assert_eq!(counts(&reply), json!([1, 1, 5, 5]));
    let indexes: Vec<&Value> = reply["loadouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["slot_index"])
        .collect();
    assert_eq!(indexes, [0, 1, 2, 3, 4, 5]);
}

#[tokio::test(flavor = "multi_thread")]
async fn stores_the_active_slot_only_below_the_number_of_slots() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    all(&app, &user, &character).await;
    // 1 account slot + 1 character slot = slots 0 and 1 before any unlock
    assert_eq!(active(&app, &user, &character, "1").await.0, 200);
    assert_eq!(active(&app, &user, &character, "2").await.0, 400);
    unlock_by(&app, &user, &character, "3").await;
    assert_eq!(
        active(&app, &user, &character, "2").await,
        (200, Some(json!({"code": null, "message": "OK"})))
    );
    assert_eq!(all(&app, &user, &character).await["active_index"], 2);
    assert_eq!(active(&app, &user, &character, "5").await.0, 400);
    assert_eq!(active(&app, &user, &character, "x").await.0, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_element_stored_past_the_slot_count_is_not_sent() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    all(&app, &user, &character).await;
    let stored = app.row("SELECT loadouts FROM loadouts WHERE characterId = ?1", &[&character]);
    let mut list: Vec<Value> = serde_json::from_str(stored["loadouts"].as_str().unwrap()).unwrap();
    list.push(json!({"slot_index": 3, "flask": "FL_STRAY"}));
    app.run(
        "UPDATE loadouts SET loadouts = ?1 WHERE characterId = ?2",
        &[&Value::Array(list).to_string(), &character],
    );
    let reply = all(&app, &user, &character).await;
    let indexes: Vec<&Value> = reply["loadouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["slot_index"])
        .collect();
    assert_eq!(indexes, [0]);
}
