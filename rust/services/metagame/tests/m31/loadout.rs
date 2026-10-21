//! `loadout.test.ts`: stub-mode loadout saves (slot 0 and the persistent loadout), their history and
//! rollback.

use metagame::characters::{RollbackError, rollback_loadout, save_history};
use metagame::loadouts::{ensure_row, payload, set_data};
use serde_json::{Value, json};

use crate::common::TestApp;

async fn set(app: &TestApp, user: &str, character: &str, index: &str, data: Option<&str>) -> bool {
    let (h, u, c, i) = (
        app.state.history,
        user.to_owned(),
        character.to_owned(),
        index.to_owned(),
    );
    let data = data.map(|d| json!(d));
    app.db(move |conn| set_data(conn, &h, &u, &c, &i, data.as_ref(), "save"))
        .await
}

/// `GetAllLoadoutsForUserIdAndCharacterId` and `GetPersistentLoadoutForUserIdAndCharacterId`: the
/// stub reply holds both.
async fn read_all(app: &TestApp, user: &str, character: &str) -> Value {
    let (u, c) = (user.to_owned(), character.to_owned());
    app.state
        .store
        .write(move |conn| payload(conn, &u, &c, false))
        .await
        .unwrap()
}

fn row(app: &TestApp, character: &str) -> Value {
    app.row("SELECT * FROM loadouts WHERE characterId = ?1", &[&character])
}

#[tokio::test(flavor = "multi_thread")]
async fn fails_when_no_loadout_row_matches_it_used_to_report_success_and_drop_the_save() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    assert!(!set(&app, &user, &character, "persistent", Some("{}")).await);
    assert_eq!(row(&app, &character), Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn fails_for_another_player_s_character() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (b, b_character) = app.make_player().await;
    read_all(&app, &b, &b_character).await;
    let before = row(&app, &b_character);
    assert!(!set(&app, &a, &b_character, "persistent", Some(r#"{"x":1}"#)).await);
    assert_eq!(row(&app, &b_character), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_data_that_is_not_json_instead_of_storing_an_unloadable_loadout() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    read_all(&app, &user, &character).await;
    let before = row(&app, &character);
    assert!(!set(&app, &user, &character, "persistent", Some("{broken")).await);
    assert!(!set(&app, &user, &character, "0", Some("{broken")).await);
    assert!(!set(&app, &user, &character, "0", None).await);
    assert_eq!(row(&app, &character), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn still_refuses_unsupported_slot_indexes() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    read_all(&app, &user, &character).await;
    assert!(!set(&app, &user, &character, "1", Some("{}")).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn saves_slot_0_and_persistent_keeping_each_version() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    read_all(&app, &user, &character).await;
    assert!(
        set(
            &app,
            &user,
            &character,
            "0",
            Some(&json!({"slot_index": 0, "flask": "FL_A"}).to_string())
        )
        .await
    );
    assert!(
        set(
            &app,
            &user,
            &character,
            "persistent",
            Some(&json!({"banner": "BN_A"}).to_string())
        )
        .await
    );
    let stored = row(&app, &character);
    let loadouts: Value = serde_json::from_str(stored["loadouts"].as_str().unwrap()).unwrap();
    let persistent: Value = serde_json::from_str(stored["persistent"].as_str().unwrap()).unwrap();
    assert_eq!(
        (loadouts[0]["flask"].clone(), persistent["banner"].clone()),
        (json!("FL_A"), json!("BN_A"))
    );
    let ids = vec![character.clone()];
    let history = app.db(move |conn| save_history(conn, &ids)).await;
    let versions: Vec<Value> = history[0]["LoadoutVersions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| json!([v["Version"], v["Reason"]]))
        .collect();
    assert_eq!(
        versions,
        [json!([3, "save"]), json!([2, "save"]), json!([1, "baseline"])]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn can_read_persistent_data_before_all_and_creates_one_consistent_row() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let (u, c) = (user.clone(), character.clone());
    let first = app.db(move |conn| ensure_row(conn, &u, &c)).await;
    let persistent: Value = serde_json::from_str(&first.persistent).unwrap();
    assert_eq!(persistent["banner"], "BN_BEGINNER_00");
    let all = read_all(&app, &user, &character).await;
    assert_eq!(all["loadouts"].as_array().unwrap().len(), 1);
    assert_eq!(row(&app, &character)["userId"], user);
    assert_eq!(app.count("loadouts", "characterId = ?1", &[&character]), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn rolls_the_loadouts_back_one_version() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    read_all(&app, &user, &character).await;
    set(
        &app,
        &user,
        &character,
        "persistent",
        Some(&json!({"banner": "BN_A"}).to_string()),
    )
    .await;
    set(
        &app,
        &user,
        &character,
        "persistent",
        Some(&json!({"banner": "BN_B"}).to_string()),
    )
    .await;
    let (h, c) = (app.state.history, character.clone());
    let result = app.db(move |conn| rollback_loadout(conn, &h, &c, 2, "UID-admin")).await;
    assert_eq!(
        result,
        Ok(json!({"CharacterId": character, "RolledBackTo": 2, "Version": 4}))
    );
    let persistent: Value = serde_json::from_str(row(&app, &character)["persistent"].as_str().unwrap()).unwrap();
    assert_eq!(persistent["banner"], "BN_A");
    let (h, c) = (app.state.history, character.clone());
    let missing = app
        .db(move |conn| rollback_loadout(conn, &h, &c, 99, "UID-admin"))
        .await;
    assert_eq!(missing, Err(RollbackError::VersionNotFound));
}
