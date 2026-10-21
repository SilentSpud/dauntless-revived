//! `matchmaking.test.ts`: cancelling, the expected-player list, concurrent and private hunt queues
//! and the grouping window. Node replaced `fetch`; here the deploy server is a scripted stand-in, the
//! queue runs on the app's party clock, and Node's "aus" region reader is the stored US region.

use metagame::matchmaking::{self, Entry, region::Region};
use serde_json::{Value, json};

use crate::common::TestApp;
use crate::deploykit::{Answer, ScriptedDeploy};

async fn app_on(deploy: Option<&ScriptedDeploy>) -> TestApp {
    // Port 9 on loopback: nothing here may reach a real deploy server
    let address = deploy.map_or("127.0.0.1:9".to_owned(), |d| d.address.clone());
    TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", address.as_str()),
        ("US_REGION", "1"),
    ])
    .await
}

async fn join(app: &TestApp, mode: &str, hunt: &str, player: &str, private: bool) -> bool {
    matchmaking::join(
        &app.state,
        Some(json!(mode)),
        Some(json!("")),
        Some(json!(hunt)),
        player,
        private,
    )
    .await
    .unwrap()
}

async fn status(app: &TestApp, player: &str) -> Option<Entry> {
    matchmaking::check_and_update(&app.state, player).await
}

fn cancel(app: &TestApp, player: &str) -> Option<Entry> {
    let now = app.state.party_clock.now();
    matchmaking::cancel(&mut app.state.parties.lock(), player, now)
}

fn port_after(base: i64) -> impl Fn(&Value, usize) -> Answer {
    move |_, number| Answer::json(200, json!({"host": "127.0.0.1", "port": base + number as i64}))
}

fn fixed(port: i64) -> impl Fn(&Value, usize) -> Answer {
    move |_, _| Answer::json(200, json!({"host": "127.0.0.1", "port": port}))
}

fn rosters(deploy: &ScriptedDeploy) -> Vec<Value> {
    deploy.calls().iter().map(|c| c["ExpectedPlayers"].clone()).collect()
}

async fn settle(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn takes_the_player_out_of_the_queue_so_the_status_poll_can_no_longer_send_them_to_the_hunt() {
    let app = app_on(None).await;
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-a", false).await);
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-b", false).await);
    assert_eq!(
        cancel(&app, "UID-a").and_then(|e| e.hunt_id),
        Some(json!("Hunt_Test_A"))
    );
    assert!(status(&app, "UID-a").await.is_none());
    assert_eq!(status(&app, "UID-b").await.map(|e| e.ready), Some(false));
    // The last player leaving drops the queue, so a new group can start one
    cancel(&app, "UID-b");
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-c", false).await);
    assert_eq!(
        status(&app, "UID-c").await.and_then(|e| e.hunt_id),
        Some(json!("Hunt_Test_A"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn is_a_no_op_for_a_player_who_is_not_queued() {
    let app = app_on(None).await;
    assert!(cancel(&app, "UID-nobody").is_none());
}

#[test]
fn lists_each_account_once_in_first_seen_order() {
    let ids = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    // The live list on 22 September 2026 was V, V, O: the hunt server waited for a third player
    assert_eq!(
        matchmaking::distinct_players(&ids(&["UID-v", "UID-v", "UID-o"])),
        ids(&["UID-v", "UID-o"])
    );
    assert_eq!(
        matchmaking::distinct_players(&ids(&["UID-o", "UID-v", "UID-o", "UID-v"])),
        ids(&["UID-o", "UID-v"])
    );
    assert!(matchmaking::distinct_players(&[]).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn same_hunt_queues_fill_independently_in_each_region() {
    let deploy = ScriptedDeploy::start(port_after(39000)).await;
    let app = app_on(Some(&deploy)).await;
    for i in 0..4 {
        let id = format!("AUS{i}");
        app.run("INSERT INTO users (userId, name, notes) VALUES (?1, ?1, 0)", &[&id]);
        app.db(move |conn| matchmaking::region::save_region(conn, &id, Region::Us))
            .await;
    }
    for i in 0..4 {
        join(&app, "ISLAND", "Hunt_Test_A", &format!("AUS{i}"), false).await;
        join(&app, "ISLAND", "Hunt_Test_A", &format!("MAIN{i}"), false).await;
    }
    deploy.wait_for_calls(2).await;
    // The two queues launch at once, so their calls can arrive in either order
    let mut calls = deploy.calls();
    calls.sort_by_key(|call| call.get("Region").is_none());
    assert_eq!(calls[0]["ExpectedPlayers"], json!(["AUS0", "AUS1", "AUS2", "AUS3"]));
    assert_eq!(calls[0]["Region"], "us");
    assert_eq!(calls[1]["ExpectedPlayers"], json!(["MAIN0", "MAIN1", "MAIN2", "MAIN3"]));
    assert_eq!(calls[1].get("Region"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn separate_city_candidates_share_the_native_game_session_for_zone_chat() {
    let deploy = ScriptedDeploy::start(|_, _| {
        Answer::json(
            200,
            json!({"host": "127.0.0.1", "port": 39009, "sessionId": "native-city-1"}),
        )
    })
    .await;
    let app = app_on(Some(&deploy)).await;
    join(&app, "CITY", "", "UID-a", false).await;
    join(&app, "CITY", "", "UID-b", false).await;
    let (a, b) = (
        status(&app, "UID-a").await.unwrap(),
        status(&app, "UID-b").await.unwrap(),
    );
    assert_ne!(a.candidate_id, b.candidate_id);
    assert_eq!((a.game_session(), b.game_session()), ("native-city-1", "native-city-1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn private_solo_hunts_never_absorb_public_players_or_another_private_player() {
    let deploy = ScriptedDeploy::start(port_after(39000)).await;
    let app = app_on(Some(&deploy)).await;
    join(&app, "ISLAND", "Hunt_Test_A", "UID-public", false).await;
    join(&app, "ISLAND", "Hunt_Test_A", "UID-private", true).await;
    join(&app, "ISLAND", "Hunt_Test_A", "UID-private2", true).await;
    settle(20).await;
    // The second private candidate waits for the older allocation result, then retries alone
    app.state.party_clock.set_for_tests(metagame::time::now_ms() + 3100);
    status(&app, "UID-private2").await;
    assert_eq!(rosters(&deploy), [json!(["UID-private"]), json!(["UID-private2"])]);
    assert_eq!(status(&app, "UID-public").await.map(|e| e.ready), Some(false));
    assert_eq!(status(&app, "UID-private").await.map(|e| e.ready), Some(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_a_waiting_public_join_to_private_removes_it_from_the_shared_roster() {
    let deploy = ScriptedDeploy::start(fixed(39001)).await;
    let app = app_on(Some(&deploy)).await;
    for id in ["UID-a", "UID-b"] {
        join(&app, "ISLAND", "Hunt_Test_A", id, false).await;
    }
    join(&app, "ISLAND", "Hunt_Test_A", "UID-a", true).await;
    // Node's mocked fetch answered A's private start before C's join ran; here it is a real request,
    // and a public start waits its turn behind one still out
    deploy.wait_for_calls(1).await;
    settle(20).await;
    for id in ["UID-c", "UID-d", "UID-e"] {
        join(&app, "ISLAND", "Hunt_Test_A", id, false).await;
    }
    settle(20).await;
    assert_eq!(
        rosters(&deploy),
        [json!(["UID-a"]), json!(["UID-b", "UID-c", "UID-d", "UID-e"])]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_a_duplicate_join_on_the_same_candidate() {
    let app = app_on(None).await;
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-a", false).await);
    let first = status(&app, "UID-a").await.unwrap();
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-a", false).await);
    assert_eq!(
        status(&app, "UID-a").await.map(|e| e.candidate_id),
        Some(first.candidate_id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn opens_the_next_batch_while_a_full_batch_is_still_allocating() {
    let deploy = ScriptedDeploy::start(fixed(39000)).await;
    deploy.hold(true);
    let app = app_on(Some(&deploy)).await;
    for id in ["UID-a", "UID-b", "UID-c"] {
        assert!(join(&app, "ISLAND", "Hunt_Test_A", id, false).await);
    }
    let fourth = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        join(&app, "ISLAND", "Hunt_Test_A", "UID-d", false),
    )
    .await;
    assert_eq!(
        fourth.ok(),
        Some(true),
        "the fourth join answers without waiting for the allocation"
    );
    deploy.wait_for_calls(1).await;
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-e", false).await);
    assert_eq!(status(&app, "UID-e").await.map(|e| e.ready), Some(false));
    assert!(join(&app, "ISLAND", "Hunt_Test_B", "UID-a", false).await);
    assert!(join(&app, "ISLAND", "Hunt_Test_A", "UID-a", false).await);
    let newer = status(&app, "UID-a").await.unwrap();
    deploy.release();
    settle(20).await;
    let after = status(&app, "UID-a").await.unwrap();
    assert_eq!((after.candidate_id, after.ready), (newer.candidate_id, false));
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_a_long_running_allocation_on_the_same_candidate_when_the_client_retries() {
    let deploy = ScriptedDeploy::start(fixed(39002)).await;
    deploy.hold(true);
    let app = app_on(Some(&deploy)).await;
    app.state.party_clock.set_for_tests(metagame::time::now_ms());
    for player in ["UID-long-a", "UID-long-b", "UID-long-c", "UID-long-d"] {
        assert!(join(&app, "ISLAND", "Hunt_Long_A", player, false).await);
    }
    deploy.wait_for_calls(1).await;
    let before = status(&app, "UID-long-a").await.unwrap();
    app.state.party_clock.advance_for_tests(31_000);
    assert!(join(&app, "ISLAND", "Hunt_Long_A", "UID-long-a", false).await);
    assert_eq!(
        status(&app, "UID-long-a").await.map(|e| e.candidate_id),
        Some(before.candidate_id.clone())
    );
    assert_eq!(deploy.calls().len(), 1);
    deploy.release();
    for _ in 0..40 {
        if let Some(entry) = status(&app, "UID-long-a").await.filter(|e| e.ready) {
            assert_eq!(entry.candidate_id, before.candidate_id);
            return;
        }
        settle(5).await;
    }
    panic!("allocation never completed");
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_a_private_solo_hunt_immediately_instead_of_waiting_for_a_public_batch() {
    let deploy = ScriptedDeploy::start(fixed(39001)).await;
    let app = app_on(Some(&deploy)).await;
    assert!(join(&app, "ISLAND", "Hunt_Private_A", "UID-private", true).await);
    deploy.wait_for_calls(1).await;
    for _ in 0..40 {
        if let Some(entry) = status(&app, "UID-private").await.filter(|e| e.ready) {
            assert_eq!(entry.port, 39001);
            return;
        }
        settle(5).await;
    }
    panic!("private hunt never became ready");
}

#[tokio::test(flavor = "multi_thread")]
async fn launches_five_seconds_after_the_first_join_even_when_another_player_joins_late() {
    let deploy = ScriptedDeploy::start(fixed(39000)).await;
    let app = app_on(Some(&deploy)).await;
    app.state.party_clock.set_for_tests(100_000);
    join(&app, "ISLAND", "Hunt_Test_A", "UID-first", false).await;
    app.state.party_clock.advance_for_tests(4999);
    join(&app, "ISLAND", "Hunt_Test_A", "UID-late", false).await;
    assert_eq!(status(&app, "UID-first").await.map(|e| e.ready), Some(false));
    assert!(deploy.calls().is_empty());
    app.state.party_clock.advance_for_tests(1);
    assert_eq!(status(&app, "UID-first").await.map(|e| e.ready), Some(true));
    assert_eq!(status(&app, "UID-late").await.map(|e| e.ready), Some(true));
    assert_eq!(rosters(&deploy), [json!(["UID-first", "UID-late"])]);
}
