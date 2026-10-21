//! `huntpassported.test.ts`: Harmonic's Hunt Pass cases (github.com/Harmonicrain/Undaunted 895f7c7,
//! test/huntpass.test.js and the progression part of test/huntpass-progress.test.js) against our real
//! progression: ranks come from the stored XP as the client derives them, and a confirm records the claim
//! and pays nothing. Node's `XpInLevel` (shown only by the client) is not computed by the Rust metagame.

use metagame::progression::Caller;
use metagame::realprogression::{Reply, confirm, earned_ranks, grant, max_rank_id, track_record_of};
use serde_json::{Value, json};

use crate::common::{TestApp, req};

const SEASON: &str = "season09b";

fn earned(app: &TestApp, id: &str, progress: i64) -> i64 {
    earned_ranks(app.state.progression_config.path(id).unwrap(), progress, false).0
}

async fn award(app: &TestApp, user: &str, tracks: Value, objectives: Value) {
    let (ctx, user) = (app.state.progression_context(), user.to_owned());
    let body = json!({"progress_tracks": tracks, "objectives": objectives});
    let reply = app
        .db(move |conn| grant(conn, &ctx, &user, Some(&body), Caller::GameServer))
        .await;
    assert_eq!(reply.status, 200);
}

async fn track(app: &TestApp, user: &str, id: &str) -> Option<Value> {
    let (config, user, id) = (app.state.progression_config.clone(), user.to_owned(), id.to_owned());
    app.db(move |conn| track_record_of(conn, &config, &user, &id)).await
}

async fn claim(app: &TestApp, user: &str, id: &str, rank: i64, kind: &str) -> Reply {
    let (ctx, user, id, kind) = (
        app.state.progression_context(),
        user.to_owned(),
        id.to_owned(),
        kind.to_owned(),
    );
    app.db(move |conn| confirm(conn, &ctx, &user, &id, &rank.to_string(), &kind, Caller::GameServer))
        .await
}

fn payload(reply: &Reply) -> Value {
    reply.body.as_ref().unwrap()["payload"].clone()
}

async fn give_premium(app: &TestApp, user: &str) {
    let (defaults, user) = (app.state.entitlement_defaults.clone(), user.to_owned());
    app.db(move |conn| metagame::entitlements::grant(conn, &defaults, &user, "season09b_premium", 0, "test"))
        .await;
}

async fn no_defaults() -> TestApp {
    TestApp::with_env(&[("ENTITLEMENTS_DEFAULT", "")]).await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_active_season_is_loaded_and_is_a_known_track() {
    let app = TestApp::start().await;
    assert_eq!(app.state.progression_config.active_hunt_pass, SEASON);
    assert!(
        app.state.progression_config.path(SEASON).is_some(),
        "the active season exists in the loaded config"
    );
    let set = TestApp::with_env(&[("ACTIVE_HUNT_PASS", SEASON)]).await;
    assert_eq!(set.state.progression_config.active_hunt_pass, SEASON);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_config_payload_carries_the_season_and_the_mastery_tracks() {
    let app = TestApp::start().await;
    let paths = app.state.progression_config.payload["payload"]["paths"]
        .as_array()
        .unwrap()
        .clone();
    assert!(paths.len() >= 10, "season plus nine mastery tracks");
    assert!(paths.iter().any(|p| p["progression_id"] == SEASON));
    assert!(paths.iter().any(|p| p["progression_id"] == "MasteryTrack_PlayerLevel"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_premium_gate_is_read_from_config_not_hardcoded() {
    let app = TestApp::start().await;
    let gate = |id: &str| {
        app.state
            .progression_config
            .path(id)
            .unwrap()
            .premium_gating_entitlement
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(gate(SEASON), "season09b_premium");
    assert_eq!(gate("MasteryTrack_Behemoth"), "", "no gate: no premium tier");
}

#[tokio::test(flavor = "multi_thread")]
async fn requirements_are_per_rank_xp_not_cumulative() {
    let app = TestApp::start().await;
    assert_eq!([0, 100, 250, 500].map(|p| earned(&app, SEASON, p)), [0, 1, 2, 5]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_xp_within_a_rank_is_the_remainder() {
    let app = TestApp::start().await;
    assert_eq!(earned(&app, SEASON, 250), 2);
    // 50 XP into rank 3: the client shows it; the server keeps only the total
    assert_eq!(250 - earned(&app, SEASON, 250) * 100, 50);
}

#[tokio::test(flavor = "multi_thread")]
async fn season09b_goes_past_50_with_prestige_a_track_without_prestige_stays_at_its_maximum() {
    let app = TestApp::start().await;
    assert_eq!(earned(&app, SEASON, 5000), 50);
    assert_eq!(earned(&app, SEASON, 5250), 52);
    assert_eq!(earned(&app, SEASON, 99_999_999), 50 + (99_999_999 - 5000) / 100);
    let behemoth = app.state.progression_config.path("MasteryTrack_Behemoth").unwrap();
    assert_eq!(earned(&app, "MasteryTrack_Behemoth", 99_999_999), max_rank_id(behemoth));
    assert_eq!(max_rank_id(behemoth), 50);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rank_costing_zero_xp_is_reached_immediately() {
    let app = TestApp::start().await;
    assert!(earned(&app, "MasteryTrack_PlayerLevel", 0) >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_negative_total_does_not_produce_a_negative_rank() {
    let app = TestApp::start().await;
    assert_eq!(earned(&app, SEASON, -500), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_track_derives_nothing_rather_than_throwing() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert!(app.state.progression_config.path("NotATrack").is_none());
    assert_eq!(track(&app, &user, "NotATrack").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_with_no_progression_reads_as_rank_0_slayer_level_1_and_no_row_is_created() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let record = track(&app, &user, SEASON).await.unwrap();
    assert_eq!(
        json!([
            record["progress"],
            record["confirmed_fremium_rank"],
            record["confirmed_premium_rank"]
        ]),
        json!([0, 0, 0])
    );
    let level = track(&app, &user, "MasteryTrack_PlayerLevel").await.unwrap()["progress"]
        .as_i64()
        .unwrap();
    assert_eq!(
        earned(&app, "MasteryTrack_PlayerLevel", level),
        1,
        "Slayer level 1 at 0 XP"
    );
    assert_eq!(
        app.count("progress_tracks", "accountId = ?1", &[&user]),
        0,
        "a read does not create rows"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stored_points_drive_the_derived_rank() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 1000}]),
        json!([]),
    )
    .await;
    let progress = track(&app, &user, SEASON).await.unwrap()["progress"].as_i64().unwrap();
    assert_eq!((progress, earned(&app, SEASON, progress)), (1000, 10));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wire_track_uses_the_field_names_the_client_parses() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let wire = track(&app, &user, SEASON).await.unwrap();
    // "fremium" is the original spelling, not a typo to fix
    let mut keys: Vec<&String> = wire.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "confirmed_date",
            "confirmed_fremium_rank",
            "confirmed_premium_rank",
            "phx_account_id",
            "progress",
            "progression_id"
        ]
    );
    assert_eq!(wire["progression_id"], SEASON);
    assert!(metagame::js::date_parse(wire["confirmed_date"].as_str().unwrap()).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn premium_is_withheld_without_the_entitlement_and_released_with_it() {
    let app = no_defaults().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 200}]),
        json!([]),
    )
    .await;
    assert_eq!(claim(&app, &user, SEASON, 2, "premium").await.status, 403);
    give_premium(&app, &user).await;
    assert_eq!(
        payload(&claim(&app, &user, SEASON, 2, "premium").await)["confirmed_premium_rank"],
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_track_with_no_gate_has_no_premium_tier_to_unlock() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": "MasteryTrack_Behemoth", "progress": 411}]),
        json!([]),
    )
    .await;
    let reply = claim(&app, &user, "MasteryTrack_Behemoth", 50, "premium").await;
    assert_eq!(
        (reply.status, payload(&reply)["confirmed_premium_rank"].clone()),
        (200, json!(0))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_progression_config_is_not_shadowed_by_progression_userid() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let reply = app.get("/progression/config", req().bearer(&user)).await;
    assert_eq!(reply.status, 200);
    assert!(
        reply.json["payload"]["paths"].is_array(),
        "the config, not a progression track"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_progression_objectives_userid_is_not_taken_for_a_track_lookup() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let reply = app
        .get(&format!("/progression/objectives/{user}"), req().bearer(&user))
        .await;
    assert_eq!((reply.status, reply.json["payload"].clone()), (200, json!([])));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_progression_userid_trackid_answers_instead_of_404() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 300}]),
        json!([]),
    )
    .await;
    let reply = app
        .get(&format!("/progression/{user}/{SEASON}"), req().bearer(&user))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        (
            reply.json["payload"]["progression_id"].clone(),
            reply.json["payload"]["progress"].clone()
        ),
        (json!(SEASON), json!(300))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_track_is_still_a_404() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        app.get(&format!("/progression/{user}/NotATrack"), req().bearer(&user))
            .await
            .status,
        404
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_progression_list_carries_the_hunt_pass_track_plus_every_mastery_track() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 700}]),
        json!([]),
    )
    .await;
    let list = app.get(&format!("/progression/{user}"), req().bearer(&user)).await.json["payload"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        list.iter().find(|t| t["progression_id"] == SEASON).unwrap()["progress"],
        700,
        "the season comes from the database"
    );
    let mastery = list
        .iter()
        .find(|t| t["progression_id"] == "MasteryTrack_Behemoth")
        .unwrap();
    assert_eq!(
        json!([mastery["progress"], mastery["confirmed_fremium_rank"]]),
        json!([0, 0])
    );
    let ids: Vec<&Value> = list.iter().map(|t| &t["progression_id"]).collect();
    let configured: Vec<Value> = app.state.progression_config.paths.iter().map(|p| json!(p.id)).collect();
    assert_eq!(
        ids,
        configured.iter().collect::<Vec<_>>(),
        "every configured track, in config order"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn xp_accrues_and_drives_the_derived_rank() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 250}]),
        json!([]),
    )
    .await;
    assert_eq!(track(&app, &user, SEASON).await.unwrap()["progress"], 250);
    assert_eq!(earned(&app, SEASON, 250), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_is_a_delta_so_repeated_awards_add_up() {
    let app = TestApp::with_env(&[("PROGRESSION_REPLAY_WINDOW_S", "0")]).await;
    let (user, _) = app.make_player().await;
    for _ in 0..3 {
        award(
            &app,
            &user,
            json!([{"progression_id": SEASON, "progress": 100}]),
            json!([]),
        )
        .await;
    }
    let progress = track(&app, &user, SEASON).await.unwrap()["progress"].as_i64().unwrap();
    assert_eq!(earned(&app, SEASON, progress), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn objectives_persist() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([]),
        json!([{"objective_id": "Obj_A", "value": 5, "completed_count": 1}]),
    )
    .await;
    let stored = app
        .get(&format!("/progression/objectives/{user}/Obj_A"), req().bearer(&user))
        .await
        .json["payload"]
        .clone();
    assert_eq!(json!([stored["progress"], stored["completed_count"]]), json!([5, 1]));
}

#[tokio::test(flavor = "multi_thread")]
async fn confirming_a_rank_records_the_claim_and_grants_nothing() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 300}]),
        json!([]),
    )
    .await;
    assert_eq!(
        payload(&claim(&app, &user, SEASON, 3, "public").await)["confirmed_fremium_rank"],
        3
    );
    assert_eq!(
        app.row("SELECT * FROM inventories WHERE characterId = ?1", &[&character]),
        Value::Null,
        "no inventory at all"
    );
    assert_eq!(app.count("inventorylog", "characterId = ?1", &[&character]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn confirming_twice_changes_nothing_the_second_time() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 300}]),
        json!([]),
    )
    .await;
    let first = payload(&claim(&app, &user, SEASON, 3, "public").await);
    let second = payload(&claim(&app, &user, SEASON, 3, "public").await);
    assert_eq!(second, first);
    let note = app.row(
        "SELECT note FROM progression_events WHERE accountId = ?1 ORDER BY id DESC",
        &[&user],
    );
    assert!(note["note"].as_str().unwrap().contains("already confirmed"), "{note}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirm_cannot_run_ahead_of_the_earned_rank() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 200}]),
        json!([]),
    )
    .await;
    assert_eq!(
        payload(&claim(&app, &user, SEASON, 40, "public").await)["confirmed_fremium_rank"],
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_accounts_can_claim_the_same_rank() {
    let app = TestApp::start().await;
    let (first, _) = app.make_player().await;
    let (second, _) = app.make_player().await;
    for user in [&first, &second] {
        award(
            &app,
            user,
            json!([{"progression_id": SEASON, "progress": 100}]),
            json!([]),
        )
        .await;
        assert_eq!(
            payload(&claim(&app, user, SEASON, 1, "public").await)["confirmed_fremium_rank"],
            1
        );
    }
    let ranks = [
        track(&app, &first, SEASON).await.unwrap()["confirmed_fremium_rank"].clone(),
        track(&app, &second, SEASON).await.unwrap()["confirmed_fremium_rank"].clone(),
    ];
    assert_eq!(ranks, [json!(1), json!(1)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn premium_ranks_are_refused_without_the_entitlement() {
    let app = no_defaults().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 200}]),
        json!([]),
    )
    .await;
    assert_eq!(claim(&app, &user, SEASON, 2, "premium").await.status, 403);
    assert_eq!(track(&app, &user, SEASON).await.unwrap()["confirmed_premium_rank"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_elite_pass_bought_late_opens_the_premium_ranks_already_earned() {
    let app = no_defaults().await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 500}]),
        json!([]),
    )
    .await;
    claim(&app, &user, SEASON, 5, "public").await;
    assert_eq!(claim(&app, &user, SEASON, 5, "premium").await.status, 403);
    give_premium(&app, &user).await;
    assert_eq!(
        payload(&claim(&app, &user, SEASON, 5, "premium").await)["confirmed_premium_rank"],
        5,
        "all five earned premium ranks at once"
    );
    let (defaults, u) = (app.state.entitlement_defaults.clone(), user.clone());
    let names: Vec<Value> = app
        .db(move |conn| metagame::entitlements::list(conn, &defaults, &u))
        .await
        .iter()
        .map(|e| e["name"].clone())
        .collect();
    assert_eq!(names, [json!("season09b_premium")], "and nothing else granted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_s_token_cannot_award_itself_progression() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let body = json!({"progress_tracks": [{"progression_id": SEASON, "progress": 99_999}], "objectives": []});
    assert_eq!(
        app.post(&format!("/progression/{user}"), req().bearer(&user).json(body))
            .await
            .status,
        403
    );
    assert_eq!(track(&app, &user, SEASON).await.unwrap()["progress"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reset_zeroes_progress_and_lets_ranks_be_earned_and_claimed_again() {
    let plain = TestApp::start().await;
    let (user, _) = plain.make_player().await;
    assert_eq!(
        plain
            .call("DELETE", &format!("/progression/{user}/{SEASON}"), req().gs())
            .await
            .status,
        403,
        "not without PROGRESSION_ALLOW_DELETE"
    );
    let app = TestApp::with_env(&[("PROGRESSION_ALLOW_DELETE", "1")]).await;
    let (user, _) = app.make_player().await;
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 200}]),
        json!([]),
    )
    .await;
    claim(&app, &user, SEASON, 2, "public").await;
    let reset = app
        .call("DELETE", &format!("/progression/{user}/{SEASON}"), req().gs())
        .await;
    assert_eq!(reset.status, 200);
    assert_eq!(
        json!([
            reset.json["payload"]["progress"],
            reset.json["payload"]["confirmed_fremium_rank"]
        ]),
        json!([0, 0])
    );
    award(
        &app,
        &user,
        json!([{"progression_id": SEASON, "progress": 200}]),
        json!([]),
    )
    .await;
    assert_eq!(
        payload(&claim(&app, &user, SEASON, 2, "public").await)["confirmed_fremium_rank"],
        2,
        "ranks are claimable again"
    );
}
