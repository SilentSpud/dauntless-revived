//! `trials.test.ts`: the Trials leaderboards, the weekly Marks and the Champion finalisation, with
//! TRIALS_LEADERBOARDS on, TRIALS_SCHEDULE off, STORE off, TRIALS_STORE on and MIDDLEMAN_STORE off. Each
//! test has its own database, so Node's table wipes between tests are not needed.

use metagame::trials::{
    self, CHAMPION_ENTITLEMENT, CHAMPION_TITLE, DAUNTLESS_TITLE, Finalized, ROTATION_START, behemoth_for_week,
    reward_rank, trial_id_for_week, window_for_week,
};
use serde_json::{Value, json};

use crate::common::{TestApp, req};
use crate::storekit::{app_with, at, instanced};

const MINUTE: i64 = 60 * 1000;
const WEEK_S: i64 = 7 * 24 * 60 * 60;

async fn app() -> TestApp {
    app_with(true, false, &[("TRIALS_LEADERBOARDS", "1"), ("TRIALS_SCHEDULE", "0")]).await
}

fn epoch() -> i64 {
    at(ROTATION_START)
}

fn week_at(time: i64) -> i64 {
    trials::week_at(Some(epoch()), time).unwrap()
}

fn current_week() -> i64 {
    week_at(metagame::time::now_ms())
}

fn trial(difficulty: i64) -> String {
    trial_id_for_week(difficulty, current_week()).unwrap()
}

fn solo(user: &str, time: i64, session: &str, difficulty: i64) -> Value {
    json!({"difficulty": difficulty, "trial_id": trial(difficulty), "completion_time": time, "objectives_completed": 3,
        "phx_account_id": user, "platform": "WIN", "platform_name": user, "player_role_id": "PR_FRANK", "weapon": 2,
        "secondary_weapon": 1, "session_id": session})
}

fn query(extra: Value) -> Value {
    let mut base = json!({"difficulty": 1, "page": 0, "page_size": 100, "trial_id": trial(1), "target_platforms": []});
    base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    base
}

async fn submit(app: &TestApp, body: Value) -> u16 {
    app.post("/trials/leaderboards", req().gs().json(body)).await.status
}

async fn finalize(app: &TestApp) -> Finalized {
    let ctx = app.state.store_context();
    app.state
        .store
        .write(move |conn| trials::finalize_completed_weeks(conn, &ctx))
        .await
        .unwrap()
}

/// A stored run, as Node's direct `trialruns` insert (its session id is its key).
struct StoredRun<'a> {
    trial_id: &'a str,
    week: i64,
    mode: &'a str,
    key: &'a str,
    group: &'a str,
    time: i64,
    entries: Value,
    submitted: &'a str,
}

fn insert_run(app: &TestApp, run: StoredRun) {
    let StoredRun {
        trial_id,
        week,
        mode,
        key,
        group,
        time,
        entries,
        submitted,
    } = run;
    let entries = entries.to_string();
    app.run(
        "INSERT INTO trialruns (trialId, trialWeek, difficulty, mode, runKey, groupKey, completionTime, objectivesCompleted, \
         sessionId, entries, submittedDate) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, 3, ?4, ?7, ?8)",
        &[&trial_id, &week, &mode, &key, &group, &time, &entries, &submitted],
    );
}

fn entry(user: &str, name: &str) -> Value {
    json!({"phx_account_id": user, "platform": "WIN", "platform_name": name, "player_role_id": "PR_FRANK", "weapon": 2})
}

fn ids(entries: &Value) -> Vec<Value> {
    entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["phx_account_id"].clone())
        .collect()
}

#[test]
fn serves_an_opt_in_current_trials_schedule_without_changing_the_default_stub() {
    let when = at("2026-10-06T12:00:00.000Z");
    let settings =
        |on: &str| metagame::store::Settings::from_env(&service_kit::env::Env::from_pairs([("TRIALS_SCHEDULE", on)]));
    let stub = trials::seasonal_event_schedule(&settings("0"), when).unwrap();
    assert_eq!(stub["payload"]["ScheduledItems"], json!([]));
    let active = trials::seasonal_event_schedule(&settings("1"), when).unwrap();
    let rows = active["payload"]["ScheduledItems"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let week = week_at(when);
    let names: Vec<Value> = rows.iter().map(|r| r["ScheduledItems"][0]["ID"].clone()).collect();
    assert_eq!(
        names,
        [
            json!("event_ladyluck_repeatable"),
            json!("CR19_PlayerHunt_Arena_Hard"),
            json!("CR19_PlayerHunt_Arena_Elite"),
            json!(trial_id_for_week(0, week).unwrap()),
            json!(trial_id_for_week(1, week).unwrap()),
        ]
    );
    assert!(rows.iter().all(|r| r["StartTime"] == "2026.10.01-18.00.00"));
    assert!(rows.iter().all(|r| r["EndTime"] == "2026.10.08-18.00.00"));
    assert!(
        rows.iter()
            .all(|r| r["ScheduledItems"][0]["MaxCompletionPerInterval"] == -1)
    );
}

#[test]
fn picks_the_same_weekly_trial_row_as_the_deploy_server() {
    // Pinned from UndauntedDeployServer's GetTrialsData; both servers must pick the same row.
    for (week, suffix) in [
        (0, "045"),
        (1, "075"),
        (2, "029"),
        (68, "064"),
        (377, "039"),
        (378, "069"),
        (379, "087"),
    ] {
        assert_eq!(
            trial_id_for_week(0, week).unwrap(),
            format!("Arena_MatchmakerHunt_Hard_{suffix}")
        );
        assert_eq!(
            trial_id_for_week(1, week).unwrap(),
            format!("Arena_MatchmakerHunt_Elite_{suffix}")
        );
    }
    let deploy: Value = serde_json::from_str(include_str!(
        "../../../../../legacy/UndauntedDeployServer/src/vendor/trials_pool.json"
    ))
    .unwrap();
    assert_eq!(deploy["behemoths"], metagame::vendor::trials_pool()["behemoths"]);
}

#[test]
fn rotates_a_variant_only_pool_weekly_without_repeating_a_behemoth_back_to_back() {
    assert_eq!(metagame::time::iso(epoch()), "2019-07-18T18:00:00.000Z");
    assert_eq!(week_at(at("2019-07-18T17:59:59.999Z")), -1);
    assert_eq!(week_at(at("2019-07-18T18:00:00.000Z")), 0);
    let pool = &metagame::vendor::trials_pool()["behemoths"];
    let mut previous = String::new();
    for week in -30..400 {
        let behemoth = behemoth_for_week(week).unwrap();
        assert!(behemoth.ends_with("_alpha"));
        let id = trial_id_for_week(1, week).unwrap();
        assert!(pool[behemoth].as_array().unwrap().contains(&json!(&id[id.len() - 3..])));
        assert_ne!(behemoth, previous);
        previous = behemoth.to_owned();
    }
}

#[test]
fn uses_the_retail_bronze_silver_gold_time_boundaries_for_reward_rank() {
    assert_eq!(reward_rank(30 * MINUTE), -1);
    assert_eq!(reward_rank(30 * MINUTE - 1), 0);
    assert_eq!(reward_rank(5 * MINUTE), 0);
    assert_eq!(reward_rank(5 * MINUTE - 1), 1);
    assert_eq!(reward_rank(3 * MINUTE), 1);
    assert_eq!(reward_rank(3 * MINUTE - 1), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn awards_weekly_steel_gilded_tiers_once_and_exposes_them_through_balance() {
    let app = app().await;
    let (user, character) = app.make_player().await;
    let marks = |kind: &str| app.stack_quantity(&character, kind);
    // Normal sub-5: Bronze + Silver = 200 Steel.
    let normal = solo(&user, 5 * MINUTE - 1, "normal-silver", 0);
    assert_eq!(submit(&app, normal.clone()).await, 204);
    assert_eq!(
        (marks("CURRENCY_MARKS_STEEL"), marks("CURRENCY_MARKS_GILDED")),
        (200, 0)
    );
    // Exact retry pays nothing, then a faster Normal run pays only the newly reached Gold tier.
    assert_eq!(submit(&app, normal).await, 204);
    assert_eq!(marks("CURRENCY_MARKS_STEEL"), 200);
    assert_eq!(submit(&app, solo(&user, 3 * MINUTE - 1, "normal-gold", 0)).await, 204);
    assert_eq!(marks("CURRENCY_MARKS_STEEL"), 300);
    // Dauntless sub-5 pays two Gilded tiers; Steel is already claimed and is not duplicated.
    assert_eq!(
        submit(&app, solo(&user, 5 * MINUTE - 1, "dauntless-silver", 1)).await,
        204
    );
    assert_eq!(
        (marks("CURRENCY_MARKS_STEEL"), marks("CURRENCY_MARKS_GILDED")),
        (300, 200)
    );
    // A later sub-3 run pays only the final Gilded tier.
    assert_eq!(
        submit(&app, solo(&user, 3 * MINUTE - 1, "dauntless-gold", 1)).await,
        204
    );
    assert_eq!(marks("CURRENCY_MARKS_GILDED"), 300);

    let balance = app.get("/balance", req().bearer(&user)).await;
    assert_eq!(balance.status, 200);
    for key in [
        "CURRENCY_MARKS_STEEL",
        "id_currency_marks_steel",
        "CURRENCY_MARKS_GILDED",
        "id_currency_marks_gilded",
    ] {
        assert_eq!(balance.json[key], 300, "{key}");
    }
    assert_eq!(
        app.count(
            "cooldowns",
            "accountId = ?1 AND cooldownId LIKE 'revived_trials_marks:%'",
            &[&user]
        ),
        6
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_a_reused_result_session_with_changed_timing_instead_of_escalating_its_reward() {
    let app = app().await;
    let (user, character) = app.make_player().await;
    let session = "immutable-result-session";
    assert_eq!(submit(&app, solo(&user, 5 * MINUTE - 1, session, 0)).await, 204);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), 200);
    assert_eq!(submit(&app, solo(&user, 3 * MINUTE - 1, session, 0)).await, 409);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), 200);
    assert_eq!(app.count("trialruns", "1=1", &[]), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn completes_the_trial_marks_lady_luck_purchase_loop_without_refilling_on_retry() {
    let app = app().await;
    let (user, character) = app.make_player().await;
    let run = solo(&user, 3 * MINUTE - 1, "purchase-loop", 0);
    assert_eq!(submit(&app, run.clone()).await, 204);
    assert_eq!(
        app.get("/balance", req().bearer(&user)).await.json["CURRENCY_MARKS_STEEL"],
        300
    );
    let token = app
        .get(
            "/token/CURRENCY_MARKS_STEEL/ladyluck_cb_passive_trials_02",
            req().bearer(&user),
        )
        .await;
    assert_eq!(token.status, 200);
    let path = format!(
        "/notification/CURRENCY_MARKS_STEEL?token={}",
        token.json["purchaseToken"].as_str().unwrap()
    );
    assert_eq!(app.post(&path, req().bearer(&user)).await.status, 204);
    let parts = || {
        instanced(&app, &character)
            .iter()
            .filter(|i| i["catalogId"] == "PART_CB_PASSIVE_TRIALS_02")
            .count()
    };
    assert_eq!(
        (app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), parts()),
        (50, 1)
    );
    // A retried result is still idempotent after the player has spent the Marks.
    assert_eq!(submit(&app, run).await, 204);
    assert_eq!(
        (app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), parts()),
        (50, 1)
    );
}

#[test]
fn reports_timing_for_the_trial_week_being_queried_not_always_the_current_week() {
    let when = epoch() + 10 * WEEK_S * 1000 + 2 * 24 * 60 * 60 * 1000;
    let week = week_at(when);
    let current = window_for_week(epoch(), week, when);
    let previous = window_for_week(epoch(), week - 1, when);
    assert_eq!(current.1 - current.0, WEEK_S);
    assert_eq!(current.2, 5 * 24 * 60 * 60);
    assert_eq!(previous.1, current.0);
    assert_eq!(previous.2, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn stores_game_server_solo_results_keeps_each_player_s_best_time_and_serves_aggregate_and_individual_shapes() {
    let app = app().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    for body in [
        solo(&a, 42000, "a-slow", 1),
        solo(&b, 39000, "b", 1),
        solo(&a, 37000, "a-fast", 1),
    ] {
        assert_eq!(submit(&app, body).await, 204);
    }
    let board = app
        .post("/trials/leaderboards/solo", req().bearer(&a).json(query(json!({}))))
        .await;
    assert_eq!(board.status, 200);
    let payload = &board.json["payload"];
    assert!(payload["trial_start"].is_i64());
    assert_eq!(
        payload["trial_end"].as_i64().unwrap() - payload["trial_start"].as_i64().unwrap(),
        WEEK_S
    );
    let refresh = payload["time_to_refresh"].as_i64().unwrap();
    assert!(refresh > 0 && refresh <= WEEK_S);
    let ranked: Vec<Value> = payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| json!([e["phx_account_id"], e["completion_time"], e["rank"]]))
        .collect();
    assert_eq!(ranked, [json!([a, 37000, 1]), json!([b, 39000, 2])]);

    let one = app
        .post(
            "/trials/leaderboards/solo/individual",
            req().bearer(&a).json(query(json!({"phx_account_id": b}))),
        )
        .await;
    assert_eq!(one.status, 200);
    let p = &one.json["payload"];
    assert_eq!(
        json!([p["phx_account_id"], p["rank"], p["trial_id"], p["difficulty"]]),
        json!([b, 2, trial(1), "1"])
    );

    let all = app
        .post("/trials/leaderboards", req().bearer(&a).json(query(json!({}))))
        .await;
    assert_eq!(all.status, 200);
    let mut keys: Vec<&String> = all.json["payload"].as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["difficulty", "guild", "page", "page_size", "trial_id", "world"]);
    assert_eq!(
        ids(&all.json["payload"]["world"]["solo"]["all"]["entries"]),
        [json!(a), json!(b)]
    );
    assert_eq!(
        all.json["payload"]["world"]["group"],
        json!({"difficulty": 1, "entries": [], "page": 0, "page_size": 100, "trial_id": trial(1)})
    );
    let captured = app
        .post("/trials/leaderboards/all", req().bearer(&a).json(query(json!({}))))
        .await;
    assert_eq!((captured.status, captured.json), (200, all.json));
}

#[tokio::test(flavor = "multi_thread")]
async fn stores_one_group_run_once_on_retry_and_serves_group_and_member_lookup() {
    let app = app().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    let body = json!({"difficulty": 1, "trial_id": trial(1), "completion_time": 51000, "objectives_completed": 3,
        "session_id": "group-session", "entries": [
            {"phx_account_id": a, "platform": "WIN", "platform_name": "A", "player_role_id": "PR_FRANK", "weapon": 2},
            {"phx_account_id": b, "platform": "WIN", "platform_name": "B", "player_role_id": "PR_BASTION", "weapon": 5}]});
    assert_eq!(submit(&app, body.clone()).await, 204);
    assert_eq!(submit(&app, body).await, 204);
    assert_eq!(app.count("trialruns", "1=1", &[]), 1);
    let group = app
        .post("/trials/leaderboards/group", req().bearer(&a).json(query(json!({}))))
        .await;
    assert_eq!(group.status, 200);
    let entries = group.json["payload"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(ids(&entries[0]["entries"]), [json!(a), json!(b)]);
    let one = app
        .post(
            "/trials/leaderboards/group/individual",
            req().bearer(&b).json(query(json!({"phx_account_id": b}))),
        )
        .await;
    assert_eq!(one.status, 200);
    assert_eq!(
        (
            one.json["payload"]["rank"].clone(),
            one.json["payload"]["session_id"].clone()
        ),
        (json!(1), json!("group-session"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn persists_the_1_4_4_profile_update_and_uses_its_latest_public_identity_on_leaderboard_reads() {
    let app = app().await;
    let (user, _) = app.make_player().await;
    let updated = app
        .post(
            "/profile/update",
            req().bearer(&user).json(
                json!({"dauntlessid": user, "epicid": "epic-a", "platformid": "platform-a",
                "currentplatform": "PSN", "currentdisplayname": "Current Slayer"}),
            ),
        )
        .await;
    assert_eq!(updated.status, 200);
    assert_eq!(app.count("leaderboardprofiles", "1=1", &[]), 1);
    let mut run = solo(&user, 32000, "profile-run", 1);
    run["platform_name"] = json!("Old Name");
    submit(&app, run).await;
    let board = app
        .post("/trials/leaderboards/solo", req().bearer(&user).json(query(json!({}))))
        .await;
    let first = &board.json["payload"]["entries"][0];
    assert_eq!(
        (first["platform"].clone(), first["platform_name"].clone()),
        (json!("PSN"), json!("Current Slayer"))
    );
    let spoof = app
        .post(
            "/profile/update",
            req().bearer(&user).json(
                json!({"dauntlessid": "UID-someone-else", "epicid": "", "platformid": "",
                "currentplatform": "WIN", "currentdisplayname": "Nope"}),
            ),
        )
        .await;
    assert_eq!(spoof.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_player_submitted_scores_and_malformed_results() {
    let app = app().await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        app.post(
            "/trials/leaderboards",
            req().bearer(&user).json(solo(&user, 1, "cheat", 1))
        )
        .await
        .status,
        403
    );
    let mut bad = solo(&user, 1, "bad", 1);
    bad["trial_id"] = json!("not-a-trial");
    assert_eq!(submit(&app, bad).await, 400);
    assert_eq!(app.count("trialruns", "1=1", &[]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn filters_by_target_platform_and_paginates_after_ranking() {
    let app = app().await;
    let (a, _) = app.make_player().await;
    let (b, _) = app.make_player().await;
    submit(&app, solo(&a, 30000, "a", 1)).await;
    let mut psn = solo(&b, 31000, "b", 1);
    psn["platform"] = json!("PSN");
    submit(&app, psn).await;
    let win = app
        .post(
            "/trials/leaderboards/solo",
            req().bearer(&a).json(query(json!({"target_platforms": ["WIN"]}))),
        )
        .await;
    assert_eq!(ids(&win.json["payload"]["entries"]), [json!(a)]);
    let page = app
        .post(
            "/trials/leaderboards/solo",
            req().bearer(&a).json(query(json!({"page": 1, "page_size": 1}))),
        )
        .await;
    let first = &page.json["payload"]["entries"][0];
    assert_eq!(
        (first["phx_account_id"].clone(), first["rank"].clone()),
        (json!(b), json!(2))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_a_repeated_trial_row_isolated_by_week_and_rejects_stale_game_server_submissions() {
    let app = app().await;
    let (user, _) = app.make_player().await;
    let week = current_week();
    insert_run(
        &app,
        StoredRun {
            trial_id: &trial(1),
            week: week - 52,
            mode: "solo",
            key: "old-cycle",
            group: &user,
            time: 1000,
            entries: json!([entry(&user, "Old")]),
            submitted: "2025-01-01T00:00:00.000Z",
        },
    );
    assert_eq!(submit(&app, solo(&user, 30000, "current-cycle", 1)).await, 204);
    let current = app
        .post("/trials/leaderboards/solo", req().bearer(&user).json(query(json!({}))))
        .await;
    let times: Vec<Value> = current.json["payload"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["completion_time"].clone())
        .collect();
    assert_eq!(times, [json!(30000)]);
    let mut stale = solo(&user, 25000, "stale", 1);
    stale["trial_id"] = json!(trial_id_for_week(1, week - 1).unwrap());
    assert_eq!(submit(&app, stale).await, 409);
}

#[tokio::test(flavor = "multi_thread")]
async fn finalizes_a_completed_dauntless_week_once_and_permanently_unlocks_champion_access() {
    let app = app().await;
    let previous = current_week() - 1;
    let trial_id = trial_id_for_week(1, previous).unwrap();
    let solo_id = "UID-trials-champion-solo";
    let group = ["UID-trials-champion-g1", "UID-trials-champion-g2"];
    insert_run(
        &app,
        StoredRun {
            trial_id: &trial_id,
            week: previous,
            mode: "solo",
            key: "champion-solo",
            group: solo_id,
            time: 30000,
            entries: json!([entry(solo_id, "Solo")]),
            submitted: "2026-01-01T00:00:00.000Z",
        },
    );
    let members: Vec<Value> = group.iter().map(|id| entry(id, id)).collect();
    insert_run(
        &app,
        StoredRun {
            trial_id: &trial_id,
            week: previous,
            mode: "group",
            key: "champion-group",
            group: &group.join(":"),
            time: 31000,
            entries: json!(members),
            submitted: "2026-01-01T00:00:01.000Z",
        },
    );
    assert_eq!(
        finalize(&app).await,
        Finalized {
            weeks: 1,
            awarded_accounts: 3
        }
    );
    assert_eq!(
        finalize(&app).await,
        Finalized {
            weeks: 0,
            awarded_accounts: 0
        }
    );
    assert_eq!(app.count("trialweeks", "1=1", &[]), 1);
    for account in std::iter::once(solo_id).chain(group) {
        let held = app.count(
            "entitlements",
            "accountId = ?1 AND name = ?2 AND duration = 0 AND revokedDate IS NULL",
            &[&account, &CHAMPION_ENTITLEMENT],
        );
        assert_eq!(held, 1, "{account}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn awards_trials_champion_to_top_100_and_the_dauntless_only_to_top_5() {
    let app = app().await;
    let mut players = Vec::new();
    for _ in 0..6 {
        players.push(app.make_player().await);
    }
    let previous = current_week() - 1;
    let trial_id = trial_id_for_week(1, previous).unwrap();
    for (index, (user, _)) in players.iter().enumerate() {
        let key = format!("title-rank-{}", index + 1);
        let submitted = format!("2026-01-01T00:00:0{index}.000Z");
        insert_run(
            &app,
            StoredRun {
                trial_id: &trial_id,
                week: previous,
                mode: "solo",
                key: &key,
                group: user,
                time: 30000 + index as i64,
                entries: json!([entry(user, user)]),
                submitted: &submitted,
            },
        );
    }
    assert_eq!(
        finalize(&app).await,
        Finalized {
            weeks: 1,
            awarded_accounts: 6
        }
    );
    for (_, character) in &players {
        assert_eq!(app.stack_quantity(character, CHAMPION_TITLE), 1);
    }
    for (_, character) in &players[..5] {
        assert_eq!(app.stack_quantity(character, DAUNTLESS_TITLE), 1);
    }
    assert_eq!(app.stack_quantity(&players[5].1, DAUNTLESS_TITLE), 0);
    // Finalization and the inventory transaction ledger make the permanent titles one-copy grants.
    assert_eq!(
        finalize(&app).await,
        Finalized {
            weeks: 0,
            awarded_accounts: 0
        }
    );
    assert_eq!(app.stack_quantity(&players[0].1, CHAMPION_TITLE), 1);
    assert_eq!(app.stack_quantity(&players[0].1, DAUNTLESS_TITLE), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn returns_404_while_the_feature_switch_is_off() {
    let app = app_with(true, false, &[("TRIALS_LEADERBOARDS", "0")]).await;
    let (user, _) = app.make_player().await;
    assert_eq!(
        app.post("/trials/leaderboards/solo", req().bearer(&user).json(query(json!({}))))
            .await
            .status,
        404
    );
}
