//! `character.test.ts`: the save triggers, character saves and their history, and rollbacks.

use metagame::characters::{
    CharacterError, HistorySettings, RollbackError, character_date, process_triggers, rollback_character, save_history,
    update,
};
use serde_json::{Value, json};

use crate::common::TestApp;

const DOJO_OBJECTIVE: &str = "929A333B40E413C41E47B0A425EC3349";

fn ftue(objectives: Value) -> String {
    objectives.to_string()
}

fn history(app: &TestApp) -> HistorySettings {
    app.state.history
}

/// `UpdateCharacterForUid` with string data.
async fn save(app: &TestApp, character: &str, user: &str, data: &str, version: i64) -> Result<(), CharacterError> {
    save_value(app, character, user, Some(json!(data)), version).await
}

async fn save_value(
    app: &TestApp,
    character: &str,
    user: &str,
    data: Option<Value>,
    version: i64,
) -> Result<(), CharacterError> {
    let (h, c, u) = (history(app), character.to_owned(), user.to_owned());
    app.db(move |conn| update(conn, &h, Some(&c), Some(&u), data.as_ref(), version, "save"))
        .await
}

async fn rollback(app: &TestApp, character: &str, target: i64) -> Result<Value, RollbackError> {
    let (h, c) = (history(app), character.to_owned());
    app.db(move |conn| rollback_character(conn, &h, &c, target, "UID-admin"))
        .await
}

async fn versions(app: &TestApp, character: &str) -> Vec<Value> {
    let ids = vec![character.to_owned()];
    let list = app.db(move |conn| save_history(conn, &ids)).await;
    list[0]["CharacterVersions"].as_array().unwrap().clone()
}

fn read(app: &TestApp, character: &str) -> Value {
    app.row("SELECT * FROM characters WHERE characterId = ?1", &[&character])
}

#[test]
fn injects_serie_dojo_when_the_ftue_objective_is_complete() {
    let input = json!({"SERIE_cr19_series_1_ftue": ftue(json!({DOJO_OBJECTIVE: {"Status": 3}}))}).to_string();
    let out: Value = serde_json::from_str(&process_triggers(&input).unwrap()).unwrap();
    let dojo: Value = serde_json::from_str(out["SERIE_dojo"].as_str().unwrap()).unwrap();
    assert_eq!(dojo["ID"], "Dojo");
}

#[test]
fn saves_unchanged_when_the_ftue_objective_is_missing_this_used_to_throw_and_block_every_save() {
    let input = json!({"SERIE_cr19_series_1_ftue": ftue(json!({})), "PlayerAccountProgressStep": "EnteredRamsgate"})
        .to_string();
    assert_eq!(process_triggers(&input).unwrap(), input);
}

#[test]
fn saves_unchanged_when_the_ftue_value_is_not_json() {
    let input = json!({"SERIE_cr19_series_1_ftue": "not json"}).to_string();
    assert_eq!(process_triggers(&input).unwrap(), input);
}

#[test]
fn never_overwrites_an_existing_serie_dojo() {
    let input = json!({"SERIE_cr19_series_1_ftue": ftue(json!({DOJO_OBJECTIVE: {"Status": 3}})), "SERIE_dojo": "kept"})
        .to_string();
    let out: Value = serde_json::from_str(&process_triggers(&input).unwrap()).unwrap();
    assert_eq!(out["SERIE_dojo"], "kept");
}

#[test]
fn refuses_data_that_is_not_a_json_object() {
    for bad in ["", "{", "null", "[]", "5", "\"text\""] {
        match process_triggers(bad) {
            Err(CharacterError::InvalidData(message)) => {
                assert!(
                    message.contains("not valid JSON") || message.contains("not a JSON object"),
                    "{bad}: {message}"
                );
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn acknowledges_identical_committed_retries_without_writing_another_history_entry() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    assert!(
        save(&app, &character, &user, r#"{"hunt":"complete","reward":1}"#, 1)
            .await
            .is_ok()
    );
    let before = app.count("characterhistory", "characterId = ?1", &[&character]);
    assert!(
        save(&app, &character, &user, r#"{"reward":1,"hunt":"complete"}"#, 1)
            .await
            .is_ok()
    );
    assert_eq!(app.count("characterhistory", "characterId = ?1", &[&character]), before);
    assert!(
        save(&app, &character, &user, r#"{"hunt":"incomplete","reward":0}"#, 1)
            .await
            .is_err()
    );
    assert!(
        save(&app, &character, &user, r#"{"hunt":"complete","reward":1}"#, 0)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_not_found_for_a_character_that_is_not_the_caller_s() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (_, b_character) = app.make_player().await;
    assert!(matches!(
        save(&app, &b_character, &a, "{}", 1).await,
        Err(CharacterError::NotFound)
    ));
    assert!(save(&app, &metagame::crypto::random_uuid(), &a, "{}", 1).await.is_err());
    assert_eq!(read(&app, &b_character)["updateVersion"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_version_that_is_not_newer_and_the_second_of_two_saves_racing_for_one_version() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    assert!(save(&app, &character, &user, r#"{"a":"1"}"#, 1).await.is_ok());
    assert!(matches!(
        save(&app, &character, &user, r#"{"a":"2"}"#, 1).await,
        Err(CharacterError::Conflict(1))
    ));
    assert!(matches!(
        save(&app, &character, &user, r#"{"a":"0"}"#, 0).await,
        Err(CharacterError::Conflict(1))
    ));
    assert_eq!(read(&app, &character)["data"], r#"{"a":"1"}"#);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_invalid_data_without_touching_the_stored_character() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    assert!(matches!(
        save_value(&app, &character, &user, Some(Value::Null), 1).await,
        Err(CharacterError::InvalidData(_))
    ));
    assert!(matches!(
        save(&app, &character, &user, "{oops", 1).await,
        Err(CharacterError::InvalidData(_))
    ));
    let row = read(&app, &character);
    assert_eq!(
        (row["updateVersion"].clone(), row["data"].clone()),
        (json!(0), json!("{}"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sets_lastmodifieddate_in_the_createddate_format() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    save(&app, &character, &user, "{}", 1).await.unwrap();
    let date = read(&app, &character)["lastModifiedDate"].as_str().unwrap().to_owned();
    assert_eq!(date, character_date());
    assert!(
        regex::Regex::new(r"^[A-Z][a-z]{2} \d{1,2}, \d{4}$")
            .unwrap()
            .is_match(&date)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_a_baseline_plus_each_saved_version_pruned_to_save_history_keep() {
    let app = TestApp::with_env(&[("SAVE_HISTORY_KEEP", "4")]).await;
    let (user, character) = app.make_player().await;
    for version in 1..=6 {
        save(
            &app,
            &character,
            &user,
            &json!({"v": version.to_string()}).to_string(),
            version,
        )
        .await
        .unwrap();
    }
    let kept: Vec<i64> = versions(&app, &character)
        .await
        .iter()
        .map(|v| v["Version"].as_i64().unwrap())
        .collect();
    assert_eq!(kept, [6, 5, 4, 3]);
    assert_eq!(app.count("characterhistory", "characterId = ?1", &[&character]), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn also_keeps_the_last_version_of_each_hour_48_h_and_of_each_day_30_days() {
    let app = TestApp::with_env(&[("SAVE_HISTORY_KEEP", "4")]).await;
    let (user, character) = app.make_player().await;
    let now = metagame::time::now_ms();
    let hour_start = now / 3_600_000 * 3_600_000 - 5 * 3_600_000;
    let noon_three_days_ago = now / 86_400_000 * 86_400_000 - 3 * 86_400_000 + 12 * 3_600_000;
    for (version, at) in [
        (1, now - 40 * 86_400_000),
        (2, noon_three_days_ago),
        (3, noon_three_days_ago + 60_000),
        (4, hour_start + 10 * 60_000),
        (5, hour_start + 20 * 60_000),
    ] {
        app.run(
            "INSERT INTO characterhistory (characterId, userId, updateVersion, name, data, savedDate, reason) VALUES (?1, ?2, ?3, 'Old', '{}', ?4, 'save')",
            &[&character, &user, &version, &metagame::time::iso(at)],
        );
    }
    for version in 10..=15 {
        save(
            &app,
            &character,
            &user,
            &json!({"v": version.to_string()}).to_string(),
            version,
        )
        .await
        .unwrap();
    }
    // The newest 4, the last of 5 hours ago, the last of 3 days ago; not 40 days ago
    let kept: Vec<i64> = versions(&app, &character)
        .await
        .iter()
        .map(|v| v["Version"].as_i64().unwrap())
        .collect();
    assert_eq!(kept, [15, 14, 13, 12, 5, 3]);
}

#[tokio::test(flavor = "multi_thread")]
async fn rolls_a_character_back_one_version_under_a_new_higher_updateversion() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    save(&app, &character, &user, r#"{"step":"one"}"#, 1).await.unwrap();
    save(&app, &character, &user, r#"{"step":"two"}"#, 2).await.unwrap();
    assert_eq!(
        rollback(&app, &character, 1).await,
        Ok(json!({"CharacterId": character, "RolledBackTo": 1, "PreviousVersion": 2, "UpdateVersion": 3}))
    );
    let row = read(&app, &character);
    assert_eq!(
        (row["data"].clone(), row["updateVersion"].clone()),
        (json!(r#"{"step":"one"}"#), json!(3))
    );
    // A stale save still in flight from before the rollback is refused
    assert!(save(&app, &character, &user, r#"{"step":"stale"}"#, 3).await.is_err());
    assert_eq!(read(&app, &character)["data"], r#"{"step":"one"}"#);
    // The version that was rolled away is still in the history
    let list: Vec<Value> = versions(&app, &character)
        .await
        .iter()
        .map(|v| json!([v["Version"], v["Reason"]]))
        .collect();
    assert_eq!(
        list,
        [
            json!([3, "rollback:1"]),
            json!([2, "save"]),
            json!([1, "save"]),
            json!([0, "baseline"])
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn can_undo_a_rollback() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    save(&app, &character, &user, r#"{"step":"one"}"#, 1).await.unwrap();
    save(&app, &character, &user, r#"{"step":"two"}"#, 2).await.unwrap();
    rollback(&app, &character, 1).await.unwrap();
    assert!(rollback(&app, &character, 2).await.is_ok());
    let row = read(&app, &character);
    assert_eq!(
        (row["data"].clone(), row["updateVersion"].clone()),
        (json!(r#"{"step":"two"}"#), json!(4))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_version_not_found_and_not_found_without_changing_anything() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    save(&app, &character, &user, "{}", 1).await.unwrap();
    assert_eq!(
        rollback(&app, &character, 42).await,
        Err(RollbackError::VersionNotFound)
    );
    assert_eq!(
        rollback(&app, "no-such-character", 0).await,
        Err(RollbackError::NotFound)
    );
    assert_eq!(read(&app, &character)["updateVersion"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn records_the_version_found_in_the_table_before_the_first_tracked_save() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    save(&app, &character, &user, r#"{"x":"1"}"#, 7).await.unwrap();
    assert!(rollback(&app, &character, 0).await.is_ok());
    let row = read(&app, &character);
    assert_eq!(
        (row["data"].clone(), row["updateVersion"].clone()),
        (json!("{}"), json!(8))
    );
}
