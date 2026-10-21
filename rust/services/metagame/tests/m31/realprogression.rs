//! `realprogression.test.ts`: real-mode tracks and objectives: reads, grants with their cap and audit
//! rows, rank confirms, the Hunt Pass tutorial sequence, resets and seeds.

use metagame::progression::Caller;
use metagame::realprogression::{
    Reply, SeedMode, confirm as confirm_rank, grant as grant_body, grant_in_track, objective_record_of,
    objective_records, reset, seed, selected_hunt_pass, set_hunt_pass, track_record_of, track_records,
};
use serde_json::{Value, json};

use crate::common::TestApp;

struct P<'a> {
    app: &'a TestApp,
}

impl P<'_> {
    async fn grant(&self, user: &str, tracks: Value, objectives: Value) -> Reply {
        self.grant_raw(user, Some(json!({"progress_tracks": tracks, "objectives": objectives})))
            .await
    }

    async fn grant_raw(&self, user: &str, body: Option<Value>) -> Reply {
        let (ctx, user) = (self.app.state.progression_context(), user.to_owned());
        self.app
            .db(move |conn| grant_body(conn, &ctx, &user, body.as_ref(), Caller::GameServer))
            .await
    }

    async fn in_track(&self, user: &str, track: &str, amount: &str) -> Reply {
        let (ctx, user, track, amount) = (
            self.app.state.progression_context(),
            user.to_owned(),
            track.to_owned(),
            amount.to_owned(),
        );
        self.app
            .db(move |conn| grant_in_track(conn, &ctx, &user, &track, &amount, Caller::GameServer))
            .await
    }

    async fn confirm(&self, user: &str, track: &str, rank: &str, kind: &str) -> Reply {
        let (ctx, user, track, rank, kind) = (
            self.app.state.progression_context(),
            user.to_owned(),
            track.to_owned(),
            rank.to_owned(),
            kind.to_owned(),
        );
        self.app
            .db(move |conn| confirm_rank(conn, &ctx, &user, &track, &rank, &kind, Caller::GameServer))
            .await
    }

    async fn tracks(&self, user: &str) -> Vec<Value> {
        let (config, user) = (self.app.state.progression_config.clone(), user.to_owned());
        self.app.db(move |conn| track_records(conn, &config, &user)).await
    }

    async fn track(&self, user: &str, id: &str) -> Option<Value> {
        let (config, user, id) = (
            self.app.state.progression_config.clone(),
            user.to_owned(),
            id.to_owned(),
        );
        self.app
            .db(move |conn| track_record_of(conn, &config, &user, &id))
            .await
    }

    async fn objectives(&self, user: &str) -> Vec<Value> {
        let user = user.to_owned();
        self.app.db(move |conn| objective_records(conn, &user)).await
    }

    async fn objective(&self, user: &str, id: &str) -> Value {
        let (user, id) = (user.to_owned(), id.to_owned());
        self.app.db(move |conn| objective_record_of(conn, &user, &id)).await
    }

    async fn reset(&self, user: &str, id: &str) -> Reply {
        let (ctx, user, id) = (self.app.state.progression_context(), user.to_owned(), id.to_owned());
        self.app
            .db(move |conn| reset(conn, &ctx, &user, &id, Caller::Admin))
            .await
    }

    async fn selected(&self, user: &str) -> String {
        let (config, user) = (self.app.state.progression_config.clone(), user.to_owned());
        self.app.db(move |conn| selected_hunt_pass(conn, &config, &user)).await
    }

    async fn set_pass(&self, user: &str, id: &str) -> Reply {
        let (ctx, user, body) = (
            self.app.state.progression_context(),
            user.to_owned(),
            json!({"progression_id": id}),
        );
        self.app
            .db(move |conn| set_hunt_pass(conn, &ctx, &user, Some(&body), Caller::GameServer))
            .await
    }

    async fn seed(&self, user: &str, how: SeedMode) -> Reply {
        let (ctx, user) = (self.app.state.progression_context(), user.to_owned());
        self.app.db(move |conn| seed(conn, &ctx, &user, how, "UID-admin")).await
    }
}

fn payload(reply: &Reply) -> Value {
    reply.body.as_ref().unwrap()["payload"].clone()
}

fn pairs(list: &Value, keys: &[&str]) -> Vec<Value> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|item| Value::Array(keys.iter().map(|k| item[*k].clone()).collect()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_all_10_configured_tracks_at_0_for_a_new_account_keyed_by_the_url_account() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    let records = p.tracks(&user).await;
    assert_eq!(records.len(), 10);
    for record in &records {
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "phx_account_id",
                "progression_id",
                "progress",
                "confirmed_fremium_rank",
                "confirmed_premium_rank",
                "confirmed_date"
            ]
        );
        assert_eq!(record["phx_account_id"], user);
        assert_eq!(
            json!([
                record["progress"],
                record["confirmed_fremium_rank"],
                record["confirmed_premium_rank"]
            ]),
            json!([0, 0, 0])
        );
        assert_eq!(record["confirmed_date"], "1970-01-01T00:00:00.000Z");
    }
    assert_eq!(records[0]["progression_id"], "season09b");
    assert_eq!(p.track(&user, "NotATrack").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn adds_increments_answers_the_new_totals_for_only_the_tracks_it_touched_stored_ranks_unchanged() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    let first = p
        .grant(
            &user,
            json!([{"progression_id": "MasteryTrack_Weapon_Sword", "progress": 4}]),
            json!([]),
        )
        .await;
    assert_eq!(first.status, 200);
    assert_eq!(first.body.as_ref().unwrap()["code"], "OK");
    assert_eq!(
        pairs(
            &payload(&first)["progress_tracks"],
            &["phx_account_id", "progression_id", "progress", "confirmed_fremium_rank"]
        ),
        [json!([user, "MasteryTrack_Weapon_Sword", 4, 0])]
    );
    assert_eq!(payload(&first)["objectives"], json!([]));
    let second = p
        .grant(
            &user,
            json!([
                {"progression_id": "MasteryTrack_Weapon_Sword", "progress": 3},
                {"progression_id": "MasteryTrack_Weapon_Sword", "progress": 2},
                {"progression_id": "season09b", "progress": 150}
            ]),
            json!([]),
        )
        .await;
    assert_eq!(
        pairs(&payload(&second)["progress_tracks"], &["progression_id", "progress"]),
        [json!(["MasteryTrack_Weapon_Sword", 9]), json!(["season09b", 150])]
    );
    assert_eq!(
        p.track(&user, "MasteryTrack_Weapon_Sword").await.unwrap()["progress"],
        9
    );
    assert_eq!(p.track(&user, "MasteryTrack_PlayerLevel").await.unwrap()["progress"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn stores_objectives_exactly_as_sent_absolute_last_one_wins_and_echoes_them() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    let reply = p
        .grant(
            &user,
            json!([]),
            json!([
                {"objective_id": "OBJ_A", "value": 3, "completed_count": 0},
                {"objective_id": "OBJ_B", "value": 1, "completed_count": 2},
                {"objective_id": "OBJ_A", "value": 5, "completed_count": 1}
            ]),
        )
        .await;
    let echo = payload(&reply)["objectives"].clone();
    assert_eq!(
        pairs(
            &echo,
            &["phx_account_id", "objective_id", "progress", "completed_count"]
        ),
        [json!([user, "OBJ_A", 5, 1]), json!([user, "OBJ_B", 1, 2])]
    );
    let keys: Vec<&String> = echo[0].as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "phx_account_id",
            "objective_id",
            "progress",
            "completed_count",
            "created_date",
            "last_modified_date"
        ]
    );
    p.grant(
        &user,
        json!([]),
        json!([{"objective_id": "OBJ_A", "value": 2, "completed_count": 1}]),
    )
    .await;
    assert_eq!(
        pairs(
            &Value::Array(p.objectives(&user).await),
            &["objective_id", "progress", "completed_count"]
        ),
        [json!(["OBJ_A", 2, 1]), json!(["OBJ_B", 1, 2])]
    );
    assert_eq!(p.objective(&user, "OBJ_B").await["progress"], 1);
    assert_eq!(
        p.objective(&user, "OBJ_A").await["created_date"],
        echo[0]["created_date"]
    );
    let missing = p.objective(&user, "OBJ_NEVER").await;
    assert_eq!(
        json!([
            missing["phx_account_id"],
            missing["objective_id"],
            missing["progress"],
            missing["completed_count"]
        ]),
        json!([user, "OBJ_NEVER", 0, 0])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn clamps_negative_amounts_to_0_caps_big_ones_reads_numeric_strings_stores_unknown_tracks() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    let reply = p
        .grant(
            &user,
            json!([
                {"progression_id": "MasteryTrack_Behemoth", "progress": -5},
                {"progression_id": "MasteryTrack_PlayerLevel", "progress": 999_999},
                {"progression_id": "MasteryTrack_Weapon_Axe", "progress": "7"},
                {"progression_id": "SomeFutureTrack", "progress": 1}
            ]),
            json!([]),
        )
        .await;
    assert_eq!(
        pairs(&payload(&reply)["progress_tracks"], &["progression_id", "progress"]),
        [
            json!(["MasteryTrack_Behemoth", 0]),
            json!(["MasteryTrack_PlayerLevel", 5000]),
            json!(["MasteryTrack_Weapon_Axe", 7]),
            json!(["SomeFutureTrack", 1])
        ]
    );
    assert_eq!(p.tracks(&user).await.len(), 11);
    assert_eq!(p.track(&user, "SomeFutureTrack").await.unwrap()["progress"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn caps_each_track_s_total_in_a_request_so_repeating_a_track_does_not_add_the_cap_twice() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    let reply = p
        .grant(
            &user,
            json!([
                {"progression_id": "MasteryTrack_PlayerLevel", "progress": 5000},
                {"progression_id": "MasteryTrack_PlayerLevel", "progress": 5000},
                {"progression_id": "MasteryTrack_PlayerLevel", "progress": 5000},
                {"progression_id": "MasteryTrack_Behemoth", "progress": 3000},
                {"progression_id": "MasteryTrack_Behemoth", "progress": 1000}
            ]),
            json!([]),
        )
        .await;
    assert_eq!(
        pairs(&payload(&reply)["progress_tracks"], &["progression_id", "progress"]),
        [
            json!(["MasteryTrack_PlayerLevel", 5000]),
            json!(["MasteryTrack_Behemoth", 4000])
        ]
    );
    assert_eq!(
        p.track(&user, "MasteryTrack_PlayerLevel").await.unwrap()["progress"],
        5000
    );
    let event = app.row(
        "SELECT note FROM progression_events WHERE accountId = ?1 ORDER BY id DESC",
        &[&user],
    );
    assert!(
        event["note"]
            .as_str()
            .unwrap()
            .contains("MasteryTrack_PlayerLevel total 15000 is over the cap of 5000, cut to 5000"),
        "{event}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_body_that_is_not_the_grant_shape_and_an_unknown_account_and_audits_both() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    assert_eq!(p.grant_raw(&user, Some(json!("nonsense"))).await.status, 400);
    assert_eq!(
        p.grant_raw(&user, Some(json!({"progress_tracks": {}, "objectives": []})))
            .await
            .status,
        400
    );
    assert_eq!(
        p.grant_raw("UID-nobody", Some(json!({"progress_tracks": [], "objectives": []})))
            .await
            .status,
        404
    );
    assert_eq!(
        app.count("progression_events", "accountId = ?1 AND status = 400", &[&user]),
        2
    );
    // Missing arrays are taken as empty (noted), not refused
    let body = json!({"progress_tracks": [{"progression_id": "season09b", "progress": 1}]});
    assert_eq!(p.grant_raw(&user, Some(body)).await.status, 200);
    let event = app.row(
        "SELECT note, body, reply FROM progression_events WHERE accountId = ?1 AND status = 200 ORDER BY id DESC",
        &[&user],
    );
    assert!(event["note"].as_str().unwrap().contains("no objectives key"));
    let stored: Value = serde_json::from_str(event["body"].as_str().unwrap()).unwrap();
    let answered: Value = serde_json::from_str(event["reply"].as_str().unwrap()).unwrap();
    assert_eq!(stored["progress_tracks"][0]["progress"], 1);
    assert_eq!(answered["payload"]["progress_tracks"][0]["progress"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_audit_log_append_only() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.grant(
        &user,
        json!([{"progression_id": "season09b", "progress": 1}]),
        json!([]),
    )
    .await;
    for sql in [
        "UPDATE progression_events SET status = 0",
        "DELETE FROM progression_events",
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
async fn raises_the_confirmed_rank_to_the_earned_rank_never_above_never_down_idempotent() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.grant(
        &user,
        json!([{"progression_id": "MasteryTrack_Weapon_Sword", "progress": 18}]),
        json!([]),
    )
    .await;
    let earned = 7; // 18 XP on a weapon track
    let reply = p
        .confirm(&user, "MasteryTrack_Weapon_Sword", &earned.to_string(), "public")
        .await;
    assert_eq!(reply.status, 200);
    let track = payload(&reply);
    assert_eq!(
        json!([
            track["progress"],
            track["confirmed_fremium_rank"],
            track["confirmed_premium_rank"]
        ]),
        json!([18, earned, 0])
    );
    assert_ne!(track["confirmed_date"], "1970-01-01T00:00:00.000Z");
    assert_eq!(
        payload(&p.confirm(&user, "MasteryTrack_Weapon_Sword", "20", "public").await)["confirmed_fremium_rank"],
        earned
    );
    assert_eq!(
        payload(&p.confirm(&user, "MasteryTrack_Weapon_Sword", "2", "public").await)["confirmed_fremium_rank"],
        earned
    );
    assert_eq!(
        p.track(&user, "MasteryTrack_Weapon_Sword").await.unwrap()["confirmed_fremium_rank"],
        earned
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn confirms_playerlevel_rank_1_at_progress_0_rank_1_costs_nothing() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    assert_eq!(
        payload(&p.confirm(&user, "MasteryTrack_PlayerLevel", "1", "public").await)["confirmed_fremium_rank"],
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn confirms_premium_on_season09b_only_while_the_account_owns_season09b_premium() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.in_track(&user, "season09b", "250").await;
    assert_eq!(
        payload(&p.confirm(&user, "season09b", "2", "premium").await)["confirmed_premium_rank"],
        2
    );
    let (defaults, u) = (app.state.entitlement_defaults.clone(), user.clone());
    app.db(move |conn| metagame::entitlements::revoke(conn, &defaults, &u, "season09b_premium"))
        .await;
    assert_eq!(p.confirm(&user, "season09b", "2", "premium").await.status, 403);
    assert_eq!(p.track(&user, "season09b").await.unwrap()["confirmed_premium_rank"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn prestige_ranks_above_50_can_be_confirmed_on_season09b() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.in_track(&user, "season09b", "5000").await;
    p.in_track(&user, "season09b", "200").await;
    assert_eq!(
        payload(&p.confirm(&user, "season09b", "52", "public").await)["confirmed_fremium_rank"],
        52
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn leaves_premium_on_a_mastery_track_alone_and_refuses_bad_input() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.grant(
        &user,
        json!([{"progression_id": "MasteryTrack_Behemoth", "progress": 411}]),
        json!([]),
    )
    .await;
    let mastery = p.confirm(&user, "MasteryTrack_Behemoth", "50", "premium").await;
    assert_eq!(mastery.status, 200);
    assert_eq!(payload(&mastery)["confirmed_premium_rank"], 0);
    assert_eq!(p.confirm(&user, "MasteryTrack_Behemoth", "5", "both").await.status, 400);
    assert_eq!(
        p.confirm(&user, "MasteryTrack_Behemoth", "-1", "public").await.status,
        400
    );
    assert_eq!(
        p.confirm(&user, "MasteryTrack_Behemoth", "x", "public").await.status,
        400
    );
    assert_eq!(p.confirm(&user, "NotATrack", "1", "public").await.status, 404);
    assert_eq!(
        p.confirm("UID-nobody", "MasteryTrack_Behemoth", "1", "public")
            .await
            .status,
        404
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn season09b_100_leaves_rank_1_pending_the_claim_s_confirm_records_it_once() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    let granted = p.in_track(&user, "season09b", "100").await;
    assert_eq!(granted.status, 200);
    let track = payload(&granted);
    assert_eq!(
        json!([
            track["progress"],
            track["confirmed_fremium_rank"],
            track["confirmed_premium_rank"]
        ]),
        json!([100, 0, 0])
    );
    assert_eq!(
        payload(&p.confirm(&user, "season09b", "1", "public").await)["confirmed_fremium_rank"],
        1
    );
    assert_eq!(
        payload(&p.confirm(&user, "season09b", "1", "premium").await)["confirmed_premium_rank"],
        1
    );
    let stored = p.track(&user, "season09b").await.unwrap();
    assert_eq!(
        json!([
            stored["progress"],
            stored["confirmed_fremium_rank"],
            stored["confirmed_premium_rank"]
        ]),
        json!([100, 1, 1])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_non_integer_amount_and_caps_a_large_one() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    assert_eq!(p.in_track(&user, "season09b", "1.5").await.status, 400);
    assert_eq!(payload(&p.in_track(&user, "season09b", "-10").await)["progress"], 0);
    assert_eq!(
        payload(&p.in_track(&user, "season09b", "100000").await)["progress"],
        5000
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_selected_hunt_pass_consistent_with_the_config() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    assert_eq!(p.selected(&user).await, "season09b");
    assert_eq!(p.set_pass(&user, "NotATrack").await.status, 400);
    assert_eq!(p.set_pass(&user, "season09b").await.status, 200);
    assert_eq!(p.selected(&user).await, "season09b");
}

#[tokio::test(flavor = "multi_thread")]
async fn resets_one_track_to_0() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.grant(
        &user,
        json!([{"progression_id": "MasteryTrack_Weapon_Spear", "progress": 50}]),
        json!([]),
    )
    .await;
    let reply = p.reset(&user, "MasteryTrack_Weapon_Spear").await;
    assert_eq!(
        json!([payload(&reply)["progress"], payload(&reply)["confirmed_fremium_rank"]]),
        json!([0, 0])
    );
    assert_eq!(
        p.track(&user, "MasteryTrack_Weapon_Spear").await.unwrap()["progress"],
        0
    );
    assert_eq!(p.reset(&user, "NotATrack").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn grandfather_every_track_at_its_max_fully_confirmed_never_lowers_stored_values() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.in_track(&user, "season09b", "5000").await;
    p.in_track(&user, "season09b", "300").await;
    assert_eq!(p.seed(&user, SeedMode::Grandfather).await.status, 200);
    let tracks = p.tracks(&user).await;
    let by = |id: &str, keys: &[&str]| {
        let t = tracks.iter().find(|t| t["progression_id"] == id).unwrap();
        Value::Array(keys.iter().map(|k| t[*k].clone()).collect())
    };
    let all = ["progress", "confirmed_fremium_rank", "confirmed_premium_rank"];
    assert_eq!(by("MasteryTrack_PlayerLevel", &all), json!([1111, 50, 50]));
    assert_eq!(by("MasteryTrack_Behemoth", &all[..2]), json!([411, 50]));
    assert_eq!(by("MasteryTrack_Weapon_Hammer", &all[..2]), json!([115, 20]));
    assert_eq!(by("season09b", &all), json!([5300, 53, 53]));
    // A confirm after grandfathering changes nothing: nothing is pending
    assert_eq!(
        payload(&p.confirm(&user, "MasteryTrack_PlayerLevel", "50", "public").await)["confirmed_fremium_rank"],
        50
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fresh_every_track_at_0_and_the_objectives_cleared_unknown_accounts_refused() {
    let app = TestApp::start().await;
    let p = P { app: &app };
    let (user, _) = app.make_player().await;
    p.grant(
        &user,
        json!([{"progression_id": "MasteryTrack_Weapon_Sword", "progress": 60}]),
        json!([{"objective_id": "OBJ", "value": 1, "completed_count": 0}]),
    )
    .await;
    assert_eq!(p.seed(&user, SeedMode::Fresh).await.status, 200);
    assert!(
        p.tracks(&user)
            .await
            .iter()
            .all(|t| t["progress"] == 0 && t["confirmed_fremium_rank"] == 0)
    );
    assert_eq!(app.count("progress_tracks", "accountId = ?1", &[&user]), 10);
    assert!(p.objectives(&user).await.is_empty());
    assert_eq!(p.seed("UID-nobody", SeedMode::Fresh).await.status, 404);
}
