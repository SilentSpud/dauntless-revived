//! `escalation.test.ts`: Escalation saves with ESCALATION_MODE=real unless a test says otherwise. The
//! soft rules refuse only with ESCALATION_STRICT=1, so each soft case runs twice; "a new process" reads
//! through a second store opened on the same database file.

use serde_json::{Value, json};

use crate::common::{Reply, TestApp, logged_warnings, req};

const S1: &str = "ESC_SEASON_1";
const S2: &str = "ESC_SEASON_2";

async fn real() -> TestApp {
    TestApp::with_env(&[("ESCALATION_MODE", "real")]).await
}

async fn strict() -> TestApp {
    TestApp::with_env(&[("ESCALATION_MODE", "real"), ("ESCALATION_STRICT", "1")]).await
}

fn empty_state() -> Value {
    json!({"escalation_level": 0, "next_level_xp": 0, "talents_progress": [], "unlock_progress": [], "update_version": 0})
}

fn snap(fields: Value) -> Value {
    let mut base = json!({"escalation_level": 0, "next_level_xp": 0, "talents_progress": [], "unlock_progress": [], "update_version": 1});
    base.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    base
}

/// GET as `as_user` (a bearer token), with the game server's key when `gs`.
async fn read_as(app: &TestApp, user: &str, season: &str, as_user: Option<&str>, gs: bool) -> Reply {
    let mut request = req();
    if let Some(who) = as_user {
        request = request.bearer(who);
    }
    if gs {
        request = request.gs();
    }
    app.get(&format!("/escalation/{season}/{user}"), request).await
}

async fn read(app: &TestApp, user: &str, season: &str) -> Reply {
    read_as(app, user, season, Some(user), false).await
}

async fn payload(app: &TestApp, user: &str) -> Value {
    read(app, user, S1).await.json["payload"].clone()
}

/// A game server's save.
async fn write(app: &TestApp, user: &str, snapshot: Value, season: &str) -> u16 {
    write_reply(app, user, snapshot, season, req().gs()).await.status
}

async fn write_reply(app: &TestApp, user: &str, snapshot: Value, season: &str, request: crate::common::Req) -> Reply {
    app.post(&format!("/escalation/{season}/{user}"), request.json(snapshot))
        .await
}

fn rows(app: &TestApp, user: &str) -> i64 {
    app.count("escalationprogression", "accountId = ?1", &[&user])
}

fn events(app: &TestApp, user: &str) -> Vec<Value> {
    app.rows(
        "SELECT status, body, reply, note FROM progression_events WHERE accountId = ?1 AND route = 'POST /escalation/:season/:uid' ORDER BY id",
        &[&user],
    )
}

fn note(event: &Value) -> &str {
    event["note"].as_str().unwrap_or_default()
}

#[test]
fn carries_the_client_s_five_seasons_with_frost_disabled() {
    let seasons = metagame::escalation::seasons();
    assert_eq!(
        seasons.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        [
            "ESC_SEASON_1",
            "ESC_SEASON_2",
            "ESC_SEASON_3",
            "ESC_SEASON_4",
            "ESC_SEASON_5"
        ]
    );
    assert_eq!(
        seasons.iter().map(|s| s.enabled).collect::<Vec<_>>(),
        [true, true, true, true, false]
    );
    for season in seasons {
        assert_eq!(season.levels.len(), 25);
        assert_eq!(season.levels[0].required_experience, 500);
        assert_eq!((season.talents.len(), season.unlocks.len()), (18, 6));
    }
    assert!(
        metagame::escalation::season("ESC_SEASON_9").is_none(),
        "unknown ids never fall back"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_the_fake_maximum_to_anyone_and_keeps_the_save_route_s_404() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    let stub = json!({"code": null, "message": "OK", "payload": {"escalation_level": 99999, "next_level_xp": 99999, "talents_progress": [], "unlock_progress": [], "update_version": 1}});
    for reply in [
        read(&app, &a, S1).await,
        read_as(&app, &a, S1, Some(&b), false).await,
        read(&app, &a, "ESC_SEASON_9").await,
        read_as(&app, &a, S1, None, true).await,
    ] {
        assert_eq!((reply.status, reply.json), (200, stub.clone()));
    }
    assert_eq!(write(&app, &a, snap(json!({"escalation_level": 1})), S1).await, 404);
    assert_eq!(
        write_reply(
            &app,
            &a,
            snap(json!({"escalation_level": 1})),
            S1,
            req().gs().bearer(&a)
        )
        .await
        .status,
        404
    );
    assert_eq!(rows(&app, &a), 0);
    assert!(events(&app, &a).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_stub_for_accounts_outside_real_progression_progression_mode_stub_with_escalation_mode_real() {
    let app = TestApp::with_env(&[("ESCALATION_MODE", "real"), ("PROGRESSION_MODE", "stub")]).await;
    let (user, _) = app.make_player().await;
    assert_eq!(payload(&app, &user).await["escalation_level"], 99999);
    assert_eq!(write(&app, &user, snap(json!({"escalation_level": 1})), S1).await, 404);
    assert_eq!(rows(&app, &user), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_account_reads_the_native_default_for_every_season_and_no_row_is_created() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    for season in metagame::escalation::seasons() {
        let reply = read(&app, &user, &season.id).await;
        assert_eq!(reply.status, 200);
        assert_eq!(
            (reply.json["code"].clone(), reply.json["message"].clone()),
            (Value::Null, json!("OK"))
        );
        assert_eq!(reply.json["payload"], empty_state(), "never the old stub's 99999");
    }
    assert_eq!(rows(&app, &user), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_season_is_a_404_not_another_season_s_data() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    assert_eq!(read(&app, &user, "ESC_SEASON_9").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_reads_only_their_own_season_a_game_server_reads_the_named_account() {
    let app = real().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    assert_eq!(
        write(&app, &a, snap(json!({"escalation_level": 1, "next_level_xp": 10})), S1).await,
        200
    );
    let as_b = read_as(&app, &a, S1, Some(&b), false).await;
    assert_eq!((as_b.status, as_b.text.as_str()), (403, ""));
    assert_eq!(
        payload(&app, &a).await["escalation_level"],
        1,
        "the account's own client"
    );
    assert_eq!(
        read_as(&app, &a, S1, None, true).await.json["payload"]["escalation_level"],
        1,
        "a game server with its key alone"
    );
    assert_eq!(
        read_as(&app, &a, S1, Some(&b), true).await.json["payload"]["escalation_level"],
        1,
        "a game server relaying another token reads the named account"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_game_server_may_save_a_relayed_token_for_another_account_is_logged_not_refused() {
    let app = real().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    logged_warnings();
    assert_eq!(
        write_reply(&app, &a, snap(json!({"escalation_level": 25})), S1, req().bearer(&a))
            .await
            .status,
        403
    );
    assert_eq!(rows(&app, &a), 0);
    assert_eq!(
        write_reply(&app, &a, snap(json!({})), S1, req().gs().bearer(&b))
            .await
            .status,
        200
    );
    let expected = format!(
        "Game server escalation save for {a} carries the token of {b}: accepted for {a}, the account the request names"
    );
    assert!(logged_warnings().contains(&expected));
    assert_eq!(
        write_reply(&app, &a, snap(json!({})), S1, req().gs().bearer(&a))
            .await
            .status,
        200,
        "the same save again is a retry"
    );
    assert_eq!(rows(&app, &a), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_s_own_client_cannot_save_and_nothing_is_stored_or_audited() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        write_reply(
            &app,
            &user,
            snap(json!({"escalation_level": 3})),
            S1,
            req().bearer(&user)
        )
        .await
        .status,
        403
    );
    assert_eq!(rows(&app, &user), 0);
    assert!(events(&app, &user).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_account_is_a_404_audited() {
    let app = real().await;
    let nobody = "UID-nobody-escalation";
    assert_eq!(write(&app, nobody, snap(json!({})), S1).await, 404);
    assert_eq!(rows(&app, nobody), 0);
    assert_eq!(
        events(&app, nobody)
            .iter()
            .map(|e| e["status"].clone())
            .collect::<Vec<_>>(),
        [json!(404)]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_snapshot_reads_back_identically_including_from_a_new_process() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    let snapshot = snap(json!({"escalation_level": 6, "next_level_xp": 250, "update_version": 3,
        "talents_progress": [{"rank": 2, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}, {"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADETWO"}],
        "unlock_progress": [{"collected": true, "reward_id": "ESC_Reward_3"}, {"collected": false, "reward_id": "ESC_Reward_2"}]}));
    let written = write_reply(&app, &user, snapshot, S1, req().gs()).await;
    assert_eq!(written.status, 200);
    let read1 = payload(&app, &user).await;
    assert_eq!(written.json["payload"], read1);
    assert_eq!(
        json!([
            read1["escalation_level"],
            read1["next_level_xp"],
            read1["update_version"]
        ]),
        json!([6, 250, 3])
    );
    // Canonical: season order, uncollected unlocks omitted (unlocked-ness is derived natively)
    assert_eq!(
        read1["talents_progress"],
        json!([{"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADETWO"}, {"rank": 2, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}])
    );
    assert_eq!(
        read1["unlock_progress"],
        json!([{"collected": true, "reward_id": "ESC_Reward_3"}])
    );

    let reopened = store::Store::open(&app.db, &store::Options::default(), &metagame::db::SCHEMA).unwrap();
    let u = user.clone();
    let (status, body) = reopened
        .write(move |conn| metagame::escalation::read_reply(conn, &u, S1).map_err(store::Error::from))
        .await
        .unwrap();
    assert_eq!((status, body.unwrap()["payload"].clone()), (200, read1));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exact_retry_is_a_replay_a_reused_version_with_different_content_is_refused() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    let first = snap(json!({"escalation_level": 1, "next_level_xp": 100, "update_version": 1}));
    let saved = write_reply(&app, &user, first.clone(), S1, req().gs()).await;
    assert_eq!(saved.status, 200);
    let stored = || {
        app.row(
            "SELECT updatedDate FROM escalationprogression WHERE accountId = ?1",
            &[&user],
        )["updatedDate"]
            .clone()
    };
    let date = stored();
    let again = write_reply(&app, &user, first.clone(), S1, req().gs()).await;
    assert_eq!(
        (again.status, &again.json),
        (200, &saved.json),
        "the stored state, as the first answer"
    );
    assert_eq!(stored(), date, "a retry writes nothing");
    assert!(note(&events(&app, &user)[1]).contains("replay of version 1, nothing changed"));
    let mut changed = first;
    changed["next_level_xp"] = json!(120);
    assert_eq!(write(&app, &user, changed, S1).await, 409);
    assert_eq!(payload(&app, &user).await["next_level_xp"], 100);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_and_reordered_saves_cannot_overwrite_newer_state() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 2, "next_level_xp": 50, "update_version": 5})),
            S1
        )
        .await,
        200
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 1, "next_level_xp": 0, "update_version": 4})),
            S1
        )
        .await,
        409
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 1, "next_level_xp": 0, "update_version": 6})),
            S1
        )
        .await,
        409,
        "a newer version still may not lower progress"
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 2, "next_level_xp": 10, "update_version": 7})),
            S1
        )
        .await,
        409
    );
    let state = payload(&app, &user).await;
    assert_eq!(
        json!([
            state["escalation_level"],
            state["next_level_xp"],
            state["update_version"]
        ]),
        json!([2, 50, 5])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_talent_reset_lowers_ranks_and_keeps_level_xp_and_collections() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    let unlocks = json!([{"collected": true, "reward_id": "ESC_Reward_3"}]);
    let first = snap(
        json!({"escalation_level": 5, "update_version": 1, "talents_progress": [{"rank": 3, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}], "unlock_progress": unlocks}),
    );
    assert_eq!(write(&app, &user, first, S1).await, 200);
    assert_eq!(
        write(
            &app,
            &user,
            snap(
                json!({"escalation_level": 5, "update_version": 2, "talents_progress": [], "unlock_progress": unlocks})
            ),
            S1
        )
        .await,
        200
    );
    let state = payload(&app, &user).await;
    assert_eq!(state["talents_progress"], json!([]));
    assert_eq!(state["escalation_level"], 5);
    assert_eq!(state["unlock_progress"], unlocks);
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_xp_follows_native_level_arithmetic_below_the_next_cost_and_unbounded_only_at_the_cap() {
    let app = strict().await;
    let (user, _) = app.make_player().await;
    let w = |fields: Value| write(&app, &user, snap(fields), S1);
    assert_eq!(
        w(json!({"escalation_level": 0, "next_level_xp": 500})).await,
        409,
        "500 XP at level 0 is level 1"
    );
    assert_eq!(w(json!({"escalation_level": 0, "next_level_xp": 499})).await, 200);
    assert_eq!(w(json!({"escalation_level": 26, "update_version": 2})).await, 400);
    assert_eq!(w(json!({"escalation_level": -1, "update_version": 2})).await, 400);
    assert_eq!(
        w(json!({"escalation_level": 25, "next_level_xp": 10, "update_version": 2})).await,
        200
    );
    assert_eq!(
        w(json!({"escalation_level": 25, "next_level_xp": 123_456, "update_version": 3})).await,
        200
    );
    assert_eq!(
        w(json!({"escalation_level": 25, "next_level_xp": 2_147_483_648_i64, "update_version": 4})).await,
        400,
        "int32 limit"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn warn_only_twin_xp_at_or_above_the_next_level_s_cost_is_stored_and_logged() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    logged_warnings();
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 0, "next_level_xp": 500})),
            S1
        )
        .await,
        200
    );
    assert_eq!(payload(&app, &user).await["next_level_xp"], 500);
    assert!(
        note(&events(&app, &user)[0])
            .contains("accepted although next_level_xp 500 is not below the level 0 cost 500 (ESCALATION_STRICT=0)")
    );
    assert!(
        logged_warnings()
            .iter()
            .any(|line| line.contains(&user) && line.contains("breaks a soft rule, stored anyway"))
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 26, "update_version": 2})),
            S1
        )
        .await,
        400,
        "the hard rules still refuse"
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 25, "next_level_xp": 2_147_483_648_i64, "update_version": 2})),
            S1
        )
        .await,
        400
    );
}

fn over() -> Value {
    json!([{"rank": 3, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}, {"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADETWO"}])
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_talents_are_limited_to_one_point_per_level_known_ids_and_their_max_rank() {
    let app = strict().await;
    let (user, _) = app.make_player().await;
    let w = |fields: Value| write(&app, &user, snap(fields), S1);
    assert_eq!(w(json!({"escalation_level": 3, "talents_progress": over()})).await, 409);
    assert_eq!(w(json!({"escalation_level": 4, "talents_progress": over()})).await, 200);
    assert_eq!(w(json!({"escalation_level": 25, "update_version": 2, "talents_progress": [{"rank": 4, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}]})).await, 400);
    assert_eq!(
        w(json!({"escalation_level": 25, "update_version": 2, "talents_progress": [{"rank": 1, "talent_id": "ESC_TALENT_S2_TIER1_PASSIVE"}]})).await,
        400,
        "another season's talent"
    );
    let twice = json!([{"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}, {"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}]);
    assert_eq!(
        w(json!({"escalation_level": 25, "update_version": 2, "talents_progress": twice})).await,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn warn_only_twin_more_talent_points_than_levels_are_stored_and_logged_ids_and_max_ranks_still_refuse() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    let w = |fields: Value| write(&app, &user, snap(fields), S1);
    assert_eq!(w(json!({"escalation_level": 3, "talents_progress": over()})).await, 200);
    assert!(note(&events(&app, &user)[0]).contains("talents spend 4 points but level 3 only earns 3"));
    assert_eq!(w(json!({"escalation_level": 25, "update_version": 2, "talents_progress": [{"rank": 4, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}]})).await, 400);
    assert_eq!(w(json!({"escalation_level": 25, "update_version": 2, "talents_progress": [{"rank": 1, "talent_id": "ESC_TALENT_S2_TIER1_PASSIVE"}]})).await, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_a_talent_tier_needs_its_pointstounlock_spent_in_earlier_tiers_as_sharedupgradetalent_requires() {
    let app = strict().await;
    let (user, _) = app.make_player().await;
    let w = |fields: Value| write(&app, &user, snap(fields), S1);
    // TIER2 talents have PointsToUnlock 4
    let tier2 = json!({"rank": 1, "talent_id": "ESC_TALENT_S1_TIER2_PASSIVE"});
    assert_eq!(
        w(json!({"escalation_level": 1, "talents_progress": [tier2]})).await,
        409,
        "a level 1 player cannot hold a tier 2 talent"
    );
    assert_eq!(
        w(json!({"escalation_level": 25, "talents_progress": [tier2, {"rank": 3, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}]})).await,
        409,
        "3 tier-1 points do not open tier 2"
    );
    let own_gate = json!([tier2, {"rank": 2, "talent_id": "ESC_TALENT_S1_TIER2_UPGRADEONE"}, {"rank": 1, "talent_id": "ESC_TALENT_S1_TIER2_UPGRADETWO"}]);
    assert_eq!(
        w(json!({"escalation_level": 25, "talents_progress": own_gate})).await,
        409,
        "a tier cannot pay for its own gate"
    );
    let gated = json!([tier2, {"rank": 3, "talent_id": "ESC_TALENT_S1_TIER1_PASSIVE"}, {"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADETWO"}]);
    assert_eq!(w(json!({"escalation_level": 5, "talents_progress": gated})).await, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn warn_only_twin_a_tier_held_without_its_gate_is_stored_and_logged() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    let tier2 = json!([{"rank": 1, "talent_id": "ESC_TALENT_S1_TIER2_PASSIVE"}]);
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 1, "talents_progress": tier2})),
            S1
        )
        .await,
        200
    );
    assert!(
        note(&events(&app, &user)[0])
            .contains("talent ESC_TALENT_S1_TIER2_PASSIVE needs 4 points spent in earlier tiers, found 0")
    );
    assert_eq!(payload(&app, &user).await["talents_progress"], tier2);
}

fn collect(level: i64, version: i64, ids: &[&str]) -> Value {
    let unlocks: Vec<Value> = ids
        .iter()
        .map(|id| json!({"collected": true, "reward_id": id}))
        .collect();
    snap(json!({"escalation_level": level, "update_version": version, "unlock_progress": unlocks}))
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_a_collection_needs_its_unlock_level_and_can_never_be_undone() {
    let app = strict().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        write(&app, &user, collect(4, 1, &["ESC_Reward_3"]), S1).await,
        409,
        "ESC_Reward_3 unlocks at level 5"
    );
    assert_eq!(write(&app, &user, collect(5, 1, &["ESC_Reward_3"]), S1).await, 200);
    let collected = || {
        app.row(
            "SELECT collectedDate FROM escalationunlocks WHERE accountId = ?1 AND unlockId = 'ESC_Reward_3'",
            &[&user],
        )["collectedDate"]
            .clone()
    };
    let at = collected();
    assert_eq!(
        write(&app, &user, collect(8, 2, &["ESC_Reward_3", "ESC_Reward_2"]), S1).await,
        200
    );
    assert_eq!(
        write(&app, &user, collect(9, 3, &["ESC_Reward_2"]), S1).await,
        409,
        "ESC_Reward_3 may not be un-collected"
    );
    assert_eq!(collected(), at, "the first collection time is kept");
    let unknown =
        snap(json!({"update_version": 9, "unlock_progress": [{"collected": true, "reward_id": "ESC_Reward_404"}]}));
    assert_eq!(write(&app, &user, unknown, S1).await, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn warn_only_twin_a_reward_collected_below_its_level_is_stored_and_logged_un_collecting_still_refuses() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    assert_eq!(write(&app, &user, collect(4, 1, &["ESC_Reward_3"]), S1).await, 200);
    assert!(note(&events(&app, &user)[0]).contains("unlock ESC_Reward_3 is collected at level 4 but needs level 5"));
    assert_eq!(
        write(&app, &user, collect(5, 2, &[]), S1).await,
        409,
        "a collection is never undone"
    );
    let unknown = snap(
        json!({"escalation_level": 5, "update_version": 3, "unlock_progress": [{"collected": true, "reward_id": "ESC_Reward_404"}]}),
    );
    assert_eq!(write(&app, &user, unknown, S1).await, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn seasons_and_accounts_are_isolated() {
    let app = real().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    assert_eq!(write(&app, &a, snap(json!({"escalation_level": 7})), S1).await, 200);
    assert_eq!(
        write(&app, &a, snap(json!({"escalation_level": 3})), S2).await,
        200,
        "version 1 is independent per season"
    );
    assert_eq!(
        write(&app, &b, snap(json!({"escalation_level": 1})), S1).await,
        200,
        "and per account"
    );
    let level = |reply: Reply| reply.json["payload"]["escalation_level"].clone();
    assert_eq!(level(read(&app, &a, S1).await), 7);
    assert_eq!(level(read(&app, &a, S2).await), 3);
    assert_eq!(level(read(&app, &b, S1).await), 1);
    assert_eq!(level(read(&app, &b, S2).await), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_disabled_frost_season_reads_normally_but_refuses_saves() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    assert_eq!(read(&app, &user, "ESC_SEASON_5").await.status, 200);
    assert_eq!(write(&app, &user, snap(json!({})), "ESC_SEASON_5").await, 409);
    assert_eq!(
        write(&app, &user, snap(json!({})), "ESC_SEASON_9").await,
        404,
        "an unknown season"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_world_server_still_holding_the_old_stub_values_cannot_save_them() {
    let app = real().await;
    let (a, _) = app.make_player().await;
    assert_eq!(
        write(
            &app,
            &a,
            snap(json!({"escalation_level": 25, "next_level_xp": 99999, "update_version": 2})),
            S1
        )
        .await,
        409
    );
    assert_eq!(
        write(
            &app,
            &a,
            snap(json!({"escalation_level": 25, "next_level_xp": 99999 + 4321, "update_version": 3})),
            S1
        )
        .await,
        409,
        "the fork's guard checked 99999 exactly"
    );
    assert_eq!(rows(&app, &a), 0);
    assert!(note(&events(&app, &a)[0]).contains("first save carries the old stub values"));
    // A player who really is at the cap keeps gaining XP there
    let (b, _) = app.make_player().await;
    assert_eq!(
        write(&app, &b, snap(json!({"escalation_level": 25, "next_level_xp": 10})), S1).await,
        200
    );
    assert_eq!(
        write(
            &app,
            &b,
            snap(json!({"escalation_level": 25, "next_level_xp": 150_000, "update_version": 2})),
            S1
        )
        .await,
        200
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_world_server_still_holding_the_old_stub_values_cannot_overwrite_a_stored_season_either() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 2, "next_level_xp": 100, "update_version": 1})),
            S1
        )
        .await,
        200
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 25, "next_level_xp": 99999, "update_version": 4})),
            S1
        )
        .await,
        409,
        "a newer version with the stub values"
    );
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 25, "next_level_xp": 120_000, "update_version": 4})),
            S1
        )
        .await,
        409,
        "or more XP since"
    );
    assert!(note(events(&app, &user).last().unwrap()).contains("save over level 2 carries the old stub values"));
    let stored = |level: i64, xp: i64, version: i64| json!({"escalation_level": level, "next_level_xp": xp, "talents_progress": [], "unlock_progress": [], "update_version": version});
    assert_eq!(payload(&app, &user).await, stored(2, 100, 1));
    assert_eq!(
        write(
            &app,
            &user,
            snap(json!({"escalation_level": 2, "next_level_xp": 150, "update_version": 5})),
            S1
        )
        .await,
        200,
        "the player's real save afterwards"
    );
    assert_eq!(payload(&app, &user).await, stored(2, 150, 5));
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_snapshots_are_refused_without_storing_anything() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    for bad in [
        json!([]),
        snap(json!({"update_version": 0})),
        snap(json!({"talents_progress": null})),
        snap(json!({"unlock_progress": [{"reward_id": "ESC_Reward_2"}]})),
        snap(json!({"next_level_xp": 1.5})),
        snap(json!({"escalation_level": "3"})),
    ] {
        assert_eq!(write(&app, &user, bad.clone(), S1).await, 400, "{bad}");
    }
    assert_eq!(rows(&app, &user), 0);
    assert!(
        events(&app, &user).iter().all(|e| e["status"] == 400),
        "each refusal is audited"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn every_save_is_a_progression_events_row_with_the_body_and_the_reply() {
    let app = real().await;
    let (user, _) = app.make_player().await;
    let body = snap(
        json!({"escalation_level": 2, "next_level_xp": 40, "talents_progress": [{"rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADEONE"}]}),
    );
    let reply = write_reply(&app, &user, body.clone(), S1, req().gs().bearer(&user)).await;
    assert_eq!(reply.status, 200);
    let event = &events(&app, &user)[0];
    assert_eq!(event["status"], 200);
    assert_eq!(
        serde_json::from_str::<Value>(event["body"].as_str().unwrap()).unwrap(),
        body
    );
    assert_eq!(
        serde_json::from_str::<Value>(event["reply"].as_str().unwrap()).unwrap(),
        reply.json
    );
    assert_eq!(event["note"], format!("season {S1}"));
    let caller = app.row(
        "SELECT caller FROM progression_events WHERE accountId = ?1 ORDER BY id DESC",
        &[&user],
    );
    assert_eq!(caller["caller"], "gameserver");
}
