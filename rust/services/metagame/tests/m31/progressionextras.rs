//! `progressionextras.test.ts`: the extras of the Harmonic port on real progression: a confirm grants
//! nothing (unless PROGRESSION_CONFIRM_ENTITLEMENTS=1, and then only permanent entitlements), the
//! grant's retry guard, objectives going backwards, the progression config folder and
//! ACTIVE_HUNT_PASS, /balance from the inventory, and the log lines.

use std::time::Duration;

use metagame::realprogression::earned_ranks;
use serde_json::{Value, json};
use service_kit::env::Env;

use crate::common::{Reply, TestApp, logged_warnings, req};

async fn grant(app: &TestApp, user: &str, body: Value) -> Reply {
    app.post(&format!("/progression/{user}"), req().gs().bearer(user).json(body))
        .await
}

async fn confirm(app: &TestApp, user: &str, track: &str, rank: i64, kind: &str) -> Reply {
    app.post(
        &format!("/progression/{user}/{track}/{rank}/confirm/{kind}"),
        req().gs().bearer(user),
    )
    .await
}

async fn track(app: &TestApp, user: &str, id: &str) -> Value {
    app.get(&format!("/progression/{user}/{id}"), req().bearer(user))
        .await
        .json["payload"]
        .clone()
}

fn events(app: &TestApp, user: &str, route: &str) -> Vec<Value> {
    app.rows(
        "SELECT status, note, reply FROM progression_events WHERE accountId = ?1 AND route = ?2 ORDER BY id",
        &[&user, &route],
    )
}

const GRANT_ROUTE: &str = "POST /progression/:uid";

fn snapshot(app: &TestApp, user: &str, character: &str) -> Value {
    json!({
        "inventory": app.row("SELECT * FROM inventories WHERE characterId = ?1", &[&character]),
        "inventorylog": app.count("inventorylog", "characterId = ?1", &[&character]),
        "inventorytransactions": app.count("inventorytransactions", "characterId = ?1", &[&character]),
        "entitlements": app.rows("SELECT name, duration, source, revokedDate FROM entitlements WHERE accountId = ?1 ORDER BY name", &[&user]),
    })
}

async fn entitlement_names(app: &TestApp, user: &str) -> Vec<String> {
    let reply = app.get("/entitlementsv2", req().bearer(user)).await;
    reply.json["entitlements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn leaves_the_inventory_the_item_log_the_ledger_and_the_entitlements_as_they_were_for_every_track_and_kind() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let body = json!({"progress_tracks": [
        {"progression_id": "season09b", "progress": 5000}, {"progression_id": "MasteryTrack_PlayerLevel", "progress": 1111},
        {"progression_id": "MasteryTrack_Weapon_Sword", "progress": 115}, {"progression_id": "MasteryTrack_Behemoth", "progress": 411}
    ], "objectives": []});
    assert_eq!(grant(&app, &user, body).await.status, 200);
    entitlement_names(&app, &user).await; // the defaults exist before the snapshot
    let before = snapshot(&app, &user, &character);
    for (id, rank) in [
        ("season09b", 50),
        ("MasteryTrack_PlayerLevel", 50),
        ("MasteryTrack_Weapon_Sword", 20),
        ("MasteryTrack_Behemoth", 50),
    ] {
        for kind in ["public", "premium"] {
            assert_eq!(confirm(&app, &user, id, rank, kind).await.status, 200, "{id} {kind}");
        }
    }
    let season = track(&app, &user, "season09b").await;
    assert_eq!(
        json!([season["confirmed_fremium_rank"], season["confirmed_premium_rank"]]),
        json!([50, 50])
    );
    assert_eq!(snapshot(&app, &user, &character), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_play_through_over_http_a_grant_crosses_a_rank_the_game_server_pays_through_inventory_the_confirm_pays_nothing_nothing_is_left_pending()
 {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let granted = grant(
        &app,
        &user,
        json!({"progress_tracks": [{"progression_id": "MasteryTrack_Weapon_Sword", "progress": 6}], "objectives": []}),
    )
    .await;
    assert_eq!(granted.status, 200);
    let progress = granted.json["payload"]["progress_tracks"][0]["progress"]
        .as_i64()
        .unwrap();
    let earned = earned_ranks(
        app.state.progression_config.path("MasteryTrack_Weapon_Sword").unwrap(),
        progress,
        false,
    )
    .0;
    assert_eq!(earned, 3);
    // The game server grants the rank rewards itself, from the difference between its cache and the reply
    let paid = app
        .post(
            "/inventory",
            req().gs().bearer(&user).json(json!({"accountId": user, "characterId": character, "source": "Progression",
                "transactionId": "5E1A0D63-0000-4000-8000-000000000147", "addStackedItems": [{"catalogId": "CURRENCY_NOTES", "quantity": 500}],
                "addInstancedItems": [], "removeStackedItems": [], "removeInstancedItems": [], "saveInstancedItems": []})),
        )
        .await;
    assert_eq!(paid.status, 200);
    for _ in 0..2 {
        let confirmed = confirm(&app, &user, "MasteryTrack_Weapon_Sword", 3, "public").await;
        assert_eq!(
            (
                confirmed.status,
                confirmed.json["payload"]["confirmed_fremium_rank"].clone()
            ),
            (200, json!(3))
        );
    }
    assert_eq!(
        app.stacks(&character),
        [json!({"catalogId": "CURRENCY_NOTES", "quantity": 500})],
        "the Rams arrived once"
    );
    assert_eq!(app.count("inventorylog", "characterId = ?1", &[&character]), 1);
    let read = track(&app, &user, "MasteryTrack_Weapon_Sword").await;
    assert_eq!(
        json!([read["progress"], read["confirmed_fremium_rank"]]),
        json!([6, earned]),
        "confirmed equals earned: nothing pending"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_season09b_confirm_at_a_prestige_rank_above_50_passes_over_http() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    grant(
        &app,
        &user,
        json!({"progress_tracks": [{"progression_id": "season09b", "progress": 5000}], "objectives": []}),
    )
    .await;
    grant(
        &app,
        &user,
        json!({"progress_tracks": [{"progression_id": "season09b", "progress": 250}], "objectives": []}),
    )
    .await;
    let confirmed = confirm(&app, &user, "season09b", 52, "public").await;
    assert_eq!(
        (
            confirmed.status,
            confirmed.json["payload"]["confirmed_fremium_rank"].clone()
        ),
        (200, json!(52))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirm_that_raises_a_rank_grants_that_rank_s_permanent_config_entitlements_never_timed_ones_or_items() {
    let app = TestApp::with_env(&[("PROGRESSION_CONFIRM_ENTITLEMENTS", "1")]).await;
    let (user, character) = app.make_player().await;
    grant(
        &app,
        &user,
        json!({"progress_tracks": [{"progression_id": "season09b", "progress": 1600}], "objectives": []}),
    )
    .await;
    // Elite ranks up to 16: ranks 6 and 9 are permanent, rank 16's escalation_xp_boost_entitlement is 24 h
    assert_eq!(confirm(&app, &user, "season09b", 16, "premium").await.status, 200);
    let names = entitlement_names(&app, &user).await;
    assert!(
        names.contains(&"ent_cchd_hp09b_commando_00".to_owned())
            && names.contains(&"ent_ccfp_hp09b_commando_00".to_owned()),
        "{names:?}"
    );
    assert!(!names.contains(&"escalation_xp_boost_entitlement".to_owned()));
    let source = app.row(
        "SELECT source FROM entitlements WHERE accountId = ?1 AND name = 'ent_cchd_hp09b_commando_00'",
        &[&user],
    );
    assert_eq!(source["source"], "confirm:season09b:6");
    assert_eq!(
        app.count("inventorylog", "characterId = ?1", &[&character]),
        0,
        "no items"
    );
    let confirms = events(&app, &user, "POST /progression/:uid/:track/:rank/confirm/:kind");
    let note = confirms.last().unwrap()["note"].as_str().unwrap().to_owned();
    assert!(
        note.contains("escalation_xp_boost_entitlement (24 h) is timed, not granted on confirm"),
        "{note}"
    );
    // Nothing again for ranks already confirmed
    let count = entitlement_names(&app, &user).await.len();
    assert_eq!(confirm(&app, &user, "season09b", 16, "premium").await.status, 200);
    assert_eq!(confirm(&app, &user, "season09b", 16, "public").await.status, 200);
    assert_eq!(entitlement_names(&app, &user).await.len(), count);
}

fn retry_body() -> Value {
    json!({"progress_tracks": [{"progression_id": "MasteryTrack_Weapon_Axe", "progress": 2}], "objectives": [{"objective_id": "OBJ_RETRY", "value": 1, "completed_count": 1}]})
}

#[tokio::test(flavor = "multi_thread")]
async fn an_identical_grant_within_the_window_answers_the_stored_reply_and_adds_nothing() {
    logged_warnings();
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let first = grant(&app, &user, retry_body()).await;
    let again = grant(&app, &user, retry_body()).await;
    let third = grant(&app, &user, retry_body()).await;
    assert_eq!([first.status, again.status, third.status], [200, 200, 200]);
    assert_eq!(again.text, first.text, "the same bytes");
    assert_eq!(third.text, first.text);
    assert_eq!(track(&app, &user, "MasteryTrack_Weapon_Axe").await["progress"], 2);
    let audit = events(&app, &user, GRANT_ROUTE);
    assert_eq!(audit.len(), 3, "every request is audited");
    let note = audit[1]["note"].as_str().unwrap();
    assert!(
        regex::Regex::new(r"^retry of event \d+ within 5 s: its reply, nothing added")
            .unwrap()
            .is_match(note),
        "{note}"
    );
    let pattern = regex::Regex::new(&format!(
        r"Progression grant for {user} repeats the grant of .* s ago: answered its stored reply, nothing added \(PROGRESSION_REPLAY_WINDOW_S=5\)"
    ))
    .unwrap();
    assert!(logged_warnings().iter().any(|line| pattern.is_match(line)));
    // A different body is a new grant, and after it the first body counts again
    let mut other = retry_body();
    other["objectives"] = json!([{"objective_id": "OBJ_RETRY", "value": 2, "completed_count": 2}]);
    grant(&app, &user, other).await;
    grant(&app, &user, retry_body()).await;
    assert_eq!(track(&app, &user, "MasteryTrack_Weapon_Axe").await["progress"], 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_identical_grant_after_the_window_adds_its_xp() {
    let app = TestApp::with_env(&[("PROGRESSION_REPLAY_WINDOW_S", "1")]).await;
    let (user, _) = app.make_player().await;
    grant(&app, &user, retry_body()).await;
    tokio::time::sleep(Duration::from_millis(1300)).await;
    grant(&app, &user, retry_body()).await;
    assert_eq!(track(&app, &user, "MasteryTrack_Weapon_Axe").await["progress"], 4);
}

/// Node moved `Date.now` to put the next grant 9.9 s, 13.9 s and 18.9 s after the first; the Rust guard
/// reads the real clock, so this waits out the default 5 s window instead: the game server's next
/// flush (10 s later) is never caught.
#[tokio::test(flavor = "multi_thread")]
async fn the_default_window_is_shorter_than_the_game_server_s_10_s_grant_flush_the_same_body_at_the_next_flush_adds_its_xp()
 {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    grant(&app, &user, retry_body()).await;
    tokio::time::sleep(Duration::from_millis(5300)).await;
    grant(&app, &user, retry_body()).await;
    assert_eq!(
        track(&app, &user, "MasteryTrack_Weapon_Axe").await["progress"],
        4,
        "the next flush is a new grant"
    );
    grant(&app, &user, retry_body()).await;
    assert_eq!(
        track(&app, &user, "MasteryTrack_Weapon_Axe").await["progress"],
        4,
        "a retry right after is still caught"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn progression_replay_window_s_0_turns_the_guard_off() {
    let app = TestApp::with_env(&[("PROGRESSION_REPLAY_WINDOW_S", "0")]).await;
    let (user, _) = app.make_player().await;
    grant(&app, &user, retry_body()).await;
    grant(&app, &user, retry_body()).await;
    assert_eq!(track(&app, &user, "MasteryTrack_Weapon_Axe").await["progress"], 4);
    assert!(
        events(&app, &user, GRANT_ROUTE)
            .iter()
            .all(|e| !e["note"].as_str().unwrap_or("").contains("retry of event"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirm_or_a_reset_in_between_means_the_game_server_had_the_answer_the_same_body_again_is_a_new_grant() {
    let app = TestApp::with_env(&[("PROGRESSION_ALLOW_DELETE", "1")]).await;
    let (user, _) = app.make_player().await;
    let season = json!({"progress_tracks": [{"progression_id": "season09b", "progress": 100}], "objectives": []});
    grant(&app, &user, season.clone()).await;
    assert_eq!(confirm(&app, &user, "season09b", 1, "public").await.status, 200);
    grant(&app, &user, season.clone()).await;
    assert_eq!(track(&app, &user, "season09b").await["progress"], 200);
    assert_eq!(
        app.call("DELETE", &format!("/progression/{user}/season09b"), req().gs())
            .await
            .status,
        200
    );
    grant(&app, &user, season).await;
    assert_eq!(
        track(&app, &user, "season09b").await["progress"],
        100,
        "after the reset"
    );
    assert!(
        events(&app, &user, GRANT_ROUTE)
            .iter()
            .all(|e| !e["note"].as_str().unwrap_or("").contains("retry of event"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_grant_is_never_the_one_a_retry_repeats() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let bad = json!({"progress_tracks": {}, "objectives": []});
    assert_eq!(grant(&app, &user, bad.clone()).await.status, 400);
    assert_eq!(grant(&app, &user, bad).await.status, 400);
    assert_eq!(events(&app, &user, GRANT_ROUTE).len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn are_stored_as_sent_and_logged_unless_a_new_completion_cycle_started() {
    logged_warnings();
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let objective = |value: i64, completed: i64| json!({"progress_tracks": [], "objectives": [{"objective_id": "OBJ_BACK", "value": value, "completed_count": completed}]});
    grant(&app, &user, objective(5, 1)).await;
    grant(&app, &user, objective(2, 1)).await;
    let read = app
        .get(&format!("/progression/objectives/{user}/OBJ_BACK"), req().bearer(&user))
        .await;
    assert_eq!(read.json["payload"]["progress"], 2, "stored as sent");
    let note = events(&app, &user, GRANT_ROUTE)[1]["note"].as_str().unwrap().to_owned();
    assert!(
        note.contains("objective OBJ_BACK went backwards (5/1 -> 2/1), stored as sent"),
        "{note}"
    );
    let backwards =
        |line: &String| line.starts_with("progression: objective went backwards: OBJ_BACK") && line.contains(&user);
    assert_eq!(logged_warnings().iter().filter(|l| backwards(l)).count(), 1);
    grant(&app, &user, objective(0, 2)).await;
    assert_eq!(
        logged_warnings().iter().filter(|l| backwards(l)).count(),
        1,
        "a new cycle is not backwards"
    );
}

fn config(pairs: &[(&str, &str)]) -> Result<metagame::progression_config::ProgressionConfig, String> {
    metagame::progression_config::from_env(&Env::from_pairs(pairs.iter().copied())).map_err(|e| e.0)
}

fn folder(root: &std::path::Path, name: &str, files: &[(&str, String)]) -> String {
    let at = root.join(name);
    std::fs::create_dir_all(&at).unwrap();
    for (file, text) in files {
        std::fs::write(at.join(file), text).unwrap();
    }
    at.to_string_lossy().into_owned()
}

fn bundled_season() -> Value {
    metagame::vendor::progression_config()["payload"]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["progression_id"] == "season09b")
        .unwrap()
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_the_bundled_file_unchanged_when_nothing_is_set() {
    assert_eq!(
        config(&[]).unwrap().description,
        "bundled, 10 tracks; active Hunt Pass season09b"
    );
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let served = app.get("/progression/config", req().bearer(&user)).await;
    let bundled = include_str!("../../../../../legacy/UndauntedMetagame/src/vendor/progression_config.json");
    let bundled: Value = serde_json::from_str(bundled).unwrap();
    assert_eq!(served.text, service_kit::jsjson::stringify(&bundled));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_override_from_the_folder_feeds_both_get_progression_config_and_the_rank_math() {
    let dir = tempfile::tempdir().unwrap();
    let mut doubled = bundled_season();
    for requirement in doubled["requirements"].as_array_mut().unwrap() {
        requirement["xp_required"] = json!(requirement["xp_required"].as_i64().unwrap() * 2);
    }
    let extra = json!([{"progression_id": "season_test", "premium_gating_entitlement": "", "requirements": [{"rank_id": 0, "xp_required": 0}, {"rank_id": 1, "xp_required": 10}]}]);
    let at = folder(
        dir.path(),
        "override",
        &[
            ("10-season09b.json", doubled.to_string()),
            ("20-extra.json", extra.to_string()),
        ],
    );
    let description = config(&[("PROGRESSION_CONFIG_DIR", &at)]).unwrap().description;
    assert!(
        regex::Regex::new(
            r"^11 tracks, from .*override: season09b replaced, season_test added; active Hunt Pass season09b$"
        )
        .unwrap()
        .is_match(&description),
        "{description}"
    );
    let app = TestApp::with_env(&[("PROGRESSION_CONFIG_DIR", &at)]).await;
    let (user, _) = app.make_player().await;
    let paths = app.get("/progression/config", req().bearer(&user)).await.json["payload"]["paths"].clone();
    assert_eq!(paths.as_array().unwrap().len(), 11);
    assert_eq!(paths[0]["progression_id"], "season09b", "an override keeps its place");
    assert_eq!(paths[0]["requirements"][1]["xp_required"], 200);
    assert_eq!(paths[10]["progression_id"], "season_test");
    // 250 XP is rank 2 with the bundled costs, rank 1 with the doubled ones: a confirm is clamped to 1
    grant(
        &app,
        &user,
        json!({"progress_tracks": [{"progression_id": "season09b", "progress": 250}], "objectives": []}),
    )
    .await;
    assert_eq!(
        confirm(&app, &user, "season09b", 2, "public").await.json["payload"]["confirmed_fremium_rank"],
        1
    );
    assert_eq!(
        app.get(&format!("/progression/{user}"), req().bearer(&user)).await.json["payload"]
            .as_array()
            .unwrap()
            .len(),
        11
    );
    assert_eq!(
        config(&[]).unwrap().description,
        "bundled, 10 tracks; active Hunt Pass season09b",
        "back to the bundled file"
    );
}

#[test]
fn refuses_a_missing_folder_bad_json_a_path_without_ranks_a_duplicate_id_and_an_unknown_active_hunt_pass() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let season = bundled_season().to_string();
    let missing = root.join("does-not-exist").to_string_lossy().into_owned();
    let cases: Vec<(Vec<(&str, String)>, &str)> = vec![
        (vec![("PROGRESSION_CONFIG_DIR", missing)], r"does-not-exist does not exist"),
        (vec![("PROGRESSION_CONFIG_DIR", folder(root, "empty", &[]))], r"holds no \.json file"),
        (vec![("PROGRESSION_CONFIG_DIR", folder(root, "badjson", &[("a.json", "{ not json".into())]))], r"a\.json is not valid JSON"),
        (
            vec![("PROGRESSION_CONFIG_DIR", folder(root, "noranks", &[("a.json", json!({"progression_id": "x", "requirements": []}).to_string())]))],
            r"\(x\): no requirements",
        ),
        (
            vec![(
                "PROGRESSION_CONFIG_DIR",
                folder(root, "order", &[("a.json", json!({"progression_id": "x", "requirements": [{"rank_id": 2, "xp_required": 1}, {"rank_id": 1, "xp_required": 1}]}).to_string())]),
            )],
            r"not in rising rank_id order",
        ),
        (
            vec![("PROGRESSION_CONFIG_DIR", folder(root, "twice", &[("a.json", season.clone()), ("b.json", season)]))],
            r"season09b is also in",
        ),
        (vec![("ACTIVE_HUNT_PASS", "season99z".to_owned())], r#"ACTIVE_HUNT_PASS is "season99z", which is not a loaded track"#),
    ];
    for (pairs, pattern) in cases {
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let error = config(&pairs).unwrap_err();
        assert!(
            regex::Regex::new(pattern).unwrap().is_match(&error),
            "{pattern}: {error}"
        );
    }
}

#[test]
fn a_bad_folder_stops_the_metagame_at_boot_with_a_fatal_line_naming_it() {
    let keys: Value = serde_json::from_str(include_str!("../fixtures/node-crypto.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let env_file = dir.path().join("metagame.env");
    std::fs::write(
        &env_file,
        format!(
            "PORT=9\nNODE_ENV=production\nAUTH_MODE=APIKEY\nDB_FILENAME={}\nAUTH_SIGNING_PRIVKEY_B64={}\nAUTH_SIGNING_PUBKEY_B64={}\nPROGRESSION_CONFIG_DIR={}\n",
            dir.path().join("test.db").to_string_lossy().replace('\\', "/"),
            keys["privB64"].as_str().unwrap(),
            keys["pubB64"].as_str().unwrap(),
            dir.path().join("not-there").to_string_lossy().replace('\\', "/"),
        ),
    )
    .unwrap();
    let started = std::process::Command::new(env!("CARGO_BIN_EXE_metagame"))
        .args(["serve", "--env-file"])
        .arg(&env_file)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&started.stdout);
    assert_eq!(started.status.code(), Some(1), "{out}");
    assert!(
        regex::Regex::new(
            r"The progression config could not be loaded: PROGRESSION_CONFIG_DIR .*not-there does not exist"
        )
        .unwrap()
        .is_match(&out),
        "{out}"
    );
    assert!(!out.contains("Clear Skies"));
}

#[tokio::test(flavor = "multi_thread")]
async fn active_hunt_pass_is_the_hunt_pass_an_account_has_before_it_chooses_one() {
    let app = TestApp::with_env(&[("ACTIVE_HUNT_PASS", "MasteryTrack_PlayerLevel")]).await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        app.state.progression_config.active_hunt_pass,
        "MasteryTrack_PlayerLevel"
    );
    assert_eq!(
        app.get(&format!("/huntpass/{user}"), req().bearer(&user)).await.json["payload"],
        "MasteryTrack_PlayerLevel"
    );
    let plain = TestApp::start().await;
    let (user, _) = plain.make_player().await;
    assert_eq!(
        plain.get(&format!("/huntpass/{user}"), req().bearer(&user)).await.json["payload"],
        "season09b",
        "the default"
    );
}

async fn holding(app: &TestApp, stacks: Value) -> String {
    let (user, character) = app.make_player().await;
    app.run(
        "INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES (?1, '[]', ?2)",
        &[&character, &stacks.to_string()],
    );
    user
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_the_currencies_the_character_holds_under_both_spellings_and_keeps_the_sheet_for_the_rest() {
    let stacks = json!([{"catalogId": "CURRENCY_NOTES", "quantity": 1260}, {"catalogId": "CURRENCY_PLATINUM", "quantity": 75}, {"catalogId": "ORB_SHOCK", "quantity": 5}]);
    let app = TestApp::start().await;
    let user = holding(&app, stacks.clone()).await;
    let sheet = app.get("/balance", req().bearer(&user)).await.json;
    let pick = |s: &Value, keys: &[&str]| Value::Array(keys.iter().map(|k| s[*k].clone()).collect());
    assert_eq!(
        pick(
            &sheet,
            &[
                "CURRENCY_NOTES",
                "id_currency_notes",
                "CURRENCY_PLATINUM",
                "id_currency_platinum"
            ]
        ),
        json!([1260, 1260, 75, 75])
    );
    assert_eq!(
        pick(
            &sheet,
            &["CURRENCY_WEAPON_TOKEN", "id_currency_weapon_token", "CURRENCY_PRESTIGE"]
        ),
        json!([25, 25, 0]),
        "not held: the sheet as before"
    );
    let fixed_app = TestApp::with_env(&[("BALANCE_FROM_INVENTORY", "0")]).await;
    let fixed_user = holding(&fixed_app, stacks).await;
    let fixed = fixed_app.get("/balance", req().bearer(&fixed_user)).await.json;
    let keys = |s: &Value| s.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&sheet), keys(&fixed), "no key added, removed or moved");
    let mut changed: Vec<String> = keys(&sheet).into_iter().filter(|k| sheet[k] != fixed[k]).collect();
    changed.sort();
    assert_eq!(
        changed,
        [
            "CURRENCY_NOTES",
            "CURRENCY_PLATINUM",
            "id_currency_notes",
            "id_currency_platinum"
        ]
    );
    let reconciled = app.post("/reconcile", req().bearer(&user).json(json!({}))).await.json;
    assert_eq!(
        reconciled,
        json!({"balances": {"id_currency_notes": 1260, "CURRENCY_NOTES": 1260}, "refreshInventory": true})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn balance_from_inventory_0_answers_the_fixed_sheet_as_before() {
    let app = TestApp::with_env(&[("BALANCE_FROM_INVENTORY", "0")]).await;
    let user = holding(
        &app,
        json!([{"catalogId": "CURRENCY_NOTES", "quantity": 1260}, {"catalogId": "CURRENCY_PLATINUM", "quantity": 75}]),
    )
    .await;
    let sheet = app.get("/balance", req().bearer(&user)).await.json;
    assert_eq!(
        json!([
            sheet["CURRENCY_NOTES"],
            sheet["CURRENCY_PLATINUM"],
            sheet["CURRENCY_WEAPON_TOKEN"]
        ]),
        json!([0, 0, 25])
    );
    let balances = app.post("/reconcile", req().bearer(&user).json(json!({}))).await.json["balances"].clone();
    assert_eq!(balances, json!({"id_currency_notes": 0, "CURRENCY_NOTES": 0}));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_with_nothing_held_gets_the_sheet_unchanged() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let sheet = app.get("/balance", req().bearer(&user)).await.json;
    assert_eq!(
        json!([
            sheet["CURRENCY_NOTES"],
            sheet["CURRENCY_PLATINUM"],
            sheet["CURRENCY_WEAPON_TOKEN"]
        ]),
        json!([0, 0, 25])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_server_s_grant_confirm_or_track_grant_that_relays_another_player_s_token_is_logged_and_kept_for_the_named_account()
 {
    logged_warnings();
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    let body = json!({"progress_tracks": [{"progression_id": "season09b", "progress": 100}], "objectives": []});
    assert_eq!(
        app.post(&format!("/progression/{a}"), req().gs().bearer(&b).json(body))
            .await
            .status,
        200
    );
    assert_eq!(
        app.post(&format!("/progression/{a}/season09b/100"), req().gs().bearer(&b))
            .await
            .status,
        200
    );
    assert_eq!(
        app.post(
            &format!("/progression/{a}/season09b/2/confirm/public"),
            req().gs().bearer(&b)
        )
        .await
        .status,
        200
    );
    let (ta, tb) = (track(&app, &a, "season09b").await, track(&app, &b, "season09b").await);
    assert_eq!(
        json!([ta["progress"], ta["confirmed_fremium_rank"], tb["progress"]]),
        json!([200, 2, 0])
    );
    let logged = logged_warnings();
    for what in ["progression grant", "progression grant in a track", "rank confirm"] {
        let line = format!(
            "Game server {what} for {a} carries the token of {b}: accepted for {a}, the account the request names"
        );
        assert!(logged.contains(&line), "{what}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn names_a_progression_request_no_route_answered_then_the_catch_all_answers_404() {
    logged_warnings();
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        app.call("PUT", &format!("/progression/{user}/season09b"), req().gs())
            .await
            .status,
        404
    );
    let line = format!("Unhandled progression request PUT /progression/{user}/season09b from a game server");
    assert!(logged_warnings().contains(&line), "{line}");
    let stub = TestApp::with_env(&[("PROGRESSION_MODE", "stub")]).await;
    let (stub_user, _) = stub.make_player().await;
    assert_eq!(
        stub.get(&format!("/progression/{stub_user}/season09b"), req().bearer(&stub_user))
            .await
            .status,
        404
    );
    let line = format!("Unhandled progression request GET /progression/{stub_user}/season09b from a player");
    assert!(logged_warnings().contains(&line), "{line}");
}
