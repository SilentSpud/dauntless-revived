//! `masteryported.test.ts`: Harmonic's mastery cases (github.com/Harmonicrain/Undaunted 895f7c7,
//! test/mastery.test.js) against our real progression. The inverted and skipped cases are listed in the
//! TS file; "also in a new process" reopens the database file with a fresh store.

use metagame::progression::Caller;
use metagame::realprogression::{Reply, confirm, earned_ranks, grant, objective_records, track_record_of};
use serde_json::{Value, json};

use crate::common::{TestApp, req};

const PLAYER: &str = "MasteryTrack_PlayerLevel";
const SWORD: &str = "MasteryTrack_Weapon_Sword";

fn earned(app: &TestApp, id: &str, progress: i64) -> i64 {
    earned_ranks(app.state.progression_config.path(id).unwrap(), progress, false).0
}

async fn award(app: &TestApp, user: &str, tracks: Value, objectives: Value) -> Reply {
    let (ctx, user) = (app.state.progression_context(), user.to_owned());
    let body = json!({"progress_tracks": tracks, "objectives": objectives});
    app.db(move |conn| grant(conn, &ctx, &user, Some(&body), Caller::GameServer))
        .await
}

async fn progress(app: &TestApp, user: &str, id: &str) -> i64 {
    let (config, user, id) = (app.state.progression_config.clone(), user.to_owned(), id.to_owned());
    app.db(move |conn| track_record_of(conn, &config, &user, &id))
        .await
        .unwrap()["progress"]
        .as_i64()
        .unwrap()
}

async fn get(app: &TestApp, path: &str, user: &str) -> Value {
    let reply = app.get(path, req().bearer(user)).await;
    assert_eq!(reply.status, 200, "{path}");
    reply.json["payload"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_account_starts_at_slayer_level_1_with_no_weapon_or_behemoth_mastery() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(earned(&app, PLAYER, 0), 1);
    let all = get(&app, &format!("/progression/{user}"), &user).await;
    let masteries: Vec<&Value> = all
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["progression_id"].as_str().unwrap().starts_with("MasteryTrack_"))
        .collect();
    assert_eq!(masteries.len(), 9);
    for track in masteries {
        let id = track["progression_id"].as_str().unwrap();
        assert_eq!(
            json!([track["progress"], track["confirmed_fremium_rank"]]),
            json!([0, 0]),
            "{id}"
        );
        if id != PLAYER {
            assert_eq!(earned(&app, id, 0), 0);
        }
    }
    assert_eq!(
        app.count("progress_tracks", "accountId = ?1", &[&user]),
        0,
        "reads do not create or max accounts"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_mastery_track_uses_its_configured_thresholds_and_cap() {
    let app = TestApp::start().await;
    let masteries: Vec<_> = app
        .state
        .progression_config
        .paths
        .iter()
        .filter(|p| p.id.starts_with("MasteryTrack_"))
        .collect();
    assert_eq!(masteries.len(), 9);
    for path in masteries {
        assert_eq!(path.prestige_xp_per_level, None, "{} has no prestige", path.id);
        let mut total = 0i64;
        for requirement in &path.requirements {
            let (xp, rank) = (requirement.xp_required as i64, requirement.rank_id as i64);
            total += xp;
            if xp > 0 {
                assert_eq!(earned(&app, &path.id, total - 1), rank - 1);
            }
            // Player ranks 0 and 1 share the zero threshold
            assert!(earned(&app, &path.id, total) >= rank);
        }
        assert_eq!(
            earned(&app, &path.id, total + 1000),
            if path.id.contains("Weapon") { 20 } else { 50 }
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mastery_awards_read_back_the_same_through_the_list_the_objectives_and_the_single_track_also_in_a_new_process()
{
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let body = json!({"progress_tracks": [{"progression_id": PLAYER, "progress": 2}, {"progression_id": SWORD, "progress": 2}],
        "objectives": [{"objective_id": "MasteryObjective_Sword_Craft", "value": 1, "completed_count": 1}]});
    assert_eq!(
        app.post(&format!("/progression/{user}"), req().gs().bearer(&user).json(body))
            .await
            .status,
        200
    );

    let all = get(&app, &format!("/progression/{user}"), &user).await;
    let objectives = get(&app, &format!("/progression/objectives/{user}"), &user).await;
    let single = get(&app, &format!("/progression/{user}/{SWORD}"), &user).await;
    assert_eq!(
        &single,
        all.as_array()
            .unwrap()
            .iter()
            .find(|t| t["progression_id"] == SWORD)
            .unwrap()
    );
    assert!(
        objectives.is_array(),
        "FindObjectivesEndpoint needs payload to be an array"
    );
    assert_eq!(
        json!([
            objectives[0]["objective_id"],
            objectives[0]["progress"],
            objectives[0]["completed_count"]
        ]),
        json!(["MasteryObjective_Sword_Craft", 1, 1])
    );
    assert_eq!(single["progress"], 2);
    assert_eq!(earned(&app, SWORD, 2), 1);

    let reopened = store::Store::open(&app.db, &store::Options::default(), &metagame::db::SCHEMA).unwrap();
    let (config, u) = (app.state.progression_config.clone(), user.clone());
    let (track, stored): (Option<Value>, Vec<Value>) = reopened
        .write(move |conn| {
            Ok::<_, store::Error>((track_record_of(conn, &config, &u, SWORD)?, objective_records(conn, &u)?))
        })
        .await
        .unwrap();
    assert_eq!(track.unwrap(), single);
    assert_eq!(Value::Array(stored), objectives);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replayed_objective_backed_mastery_award_does_not_add_its_points_twice() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let tracks = json!([{"progression_id": SWORD, "progress": 2}]);
    award(
        &app,
        &user,
        tracks.clone(),
        json!([{"objective_id": "CraftSword", "value": 1, "completed_count": 1}]),
    )
    .await;
    award(
        &app,
        &user,
        tracks.clone(),
        json!([{"objective_id": "CraftSword", "value": 1, "completed_count": 1}]),
    )
    .await;
    assert_eq!(progress(&app, &user, SWORD).await, 2);
    award(
        &app,
        &user,
        tracks,
        json!([{"objective_id": "CraftSword", "value": 2, "completed_count": 2}]),
    )
    .await;
    assert_eq!(progress(&app, &user, SWORD).await, 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_server_s_objective_read_gets_the_named_account_and_another_player_cannot_read_it() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    award(
        &app,
        &a,
        json!([{"progression_id": SWORD, "progress": 2}]),
        json!([{"objective_id": "CraftSword", "value": 1, "completed_count": 1}]),
    )
    .await;

    let as_server = app.get(&format!("/progression/objectives/{a}"), req().gs()).await;
    assert_eq!(as_server.status, 200);
    let first = &as_server.json["payload"][0];
    assert_eq!(
        json!([first["phx_account_id"], first["progress"], first["completed_count"]]),
        json!([a, 1, 1])
    );

    let as_other = app.get(&format!("/progression/objectives/{a}"), req().bearer(&b)).await;
    assert_eq!((as_other.status, as_other.text.as_str()), (403, ""));
}

#[tokio::test(flavor = "multi_thread")]
async fn playerlevel_ranks_with_zero_quantity_rewards_in_the_config_can_be_confirmed() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": PLAYER, "progress": 62}]),
        json!([]),
    )
    .await;
    let reached = earned(&app, PLAYER, progress(&app, &user, PLAYER).await);
    assert!(reached >= 10, "reached {reached}");

    let (ctx, u) = (app.state.progression_context(), user.clone());
    let reply = app
        .db(move |conn| {
            confirm(
                conn,
                &ctx,
                &u,
                PLAYER,
                &reached.to_string(),
                "public",
                Caller::GameServer,
            )
        })
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.body.as_ref().unwrap()["payload"]["confirmed_fremium_rank"],
        reached
    );
    assert_eq!(
        app.count("inventories", "characterId = ?1", &[&character]),
        0,
        "no stack, empty or not"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unearned_claims_and_player_authenticated_xp_awards_stay_blocked() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let (ctx, u) = (app.state.progression_context(), user.clone());
    let reply = app
        .db(move |conn| confirm(conn, &ctx, &u, SWORD, "20", "public", Caller::GameServer))
        .await;
    assert_eq!(reply.body.as_ref().unwrap()["payload"]["confirmed_fremium_rank"], 0);

    let body = json!({"progress_tracks": [{"progression_id": SWORD, "progress": 999}]});
    assert_eq!(
        app.post(&format!("/progression/{user}"), req().bearer(&user).json(body))
            .await
            .status,
        403
    );
    assert_eq!(progress(&app, &user, SWORD).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_entry_does_not_throw_away_the_valid_entries_xp() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let reply = award(
        &app,
        &user,
        json!([{"progression_id": SWORD, "progress": 2}, {"progression_id": 42, "progress": 1}, {"progression_id": PLAYER}]),
        json!([{"objective_id": "Valid", "value": 1, "completed_count": 0}, {"value": 3}]),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_eq!(progress(&app, &user, SWORD).await, 2);
    assert_eq!(progress(&app, &user, PLAYER).await, 0);
    let u = user.clone();
    let stored: Vec<Value> = app
        .db(move |conn| objective_records(conn, &u))
        .await
        .iter()
        .map(|o| json!([o["objective_id"], o["progress"]]))
        .collect();
    assert_eq!(stored, [json!(["Valid", 1])]);

    let note = app.row(
        "SELECT note FROM progression_events WHERE accountId = ?1 ORDER BY id DESC",
        &[&user],
    );
    let note = note["note"].as_str().unwrap();
    assert!(
        note.contains(r#"skipped track entry {"progression_id":42,"progress":1}"#),
        "{note}"
    );
    assert!(note.contains(r#"skipped objective entry {"value":3}"#), "{note}");
}
