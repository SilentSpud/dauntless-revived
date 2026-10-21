//! `matchmakingfailure.test.ts`: a deploy server that cannot start a game server ends the player's
//! matchmaking with FAILED, never with an address nothing listens on; capacity refusals wait and
//! retry. Run on the three paths that ask the deploy server: Ramsgate, the tutorial from its own game
//! args, and a hunt queue that fills up with 4 players. The deploy server is the scripted stand-in;
//! Node's party clock is the app's.

use std::sync::atomic::{AtomicUsize, Ordering};

use metagame::matchmaking::{self, Decision};
use serde_json::{Value, json};

use crate::common::{Reply, TestApp, req};
use crate::deploykit::{Answer, ScriptedDeploy};

const TUTORIAL_GAME_ARGS: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn working(_: &Value, _: usize) -> Answer {
    Answer::json(200, json!({"host": "127.0.0.1", "port": 8790}))
}

fn capacity(reason: &'static str, retry_after: Option<&'static str>) -> impl Fn(&Value, usize) -> Answer {
    move |_, _| {
        let headers = retry_after
            .map(|r| vec![("Retry-After".to_owned(), r.to_owned())])
            .unwrap_or_default();
        Answer::Reply(
            503,
            json!({"error": "capacity_unavailable", "reason": reason}).to_string(),
            headers,
        )
    }
}

struct Mm {
    app: TestApp,
    deploy: ScriptedDeploy,
}

async fn mm() -> Mm {
    let deploy = ScriptedDeploy::start(working).await;
    let app = TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", deploy.address.as_str()),
        ("TARGET_CHANGELIST", "239827"),
    ])
    .await;
    Mm { app, deploy }
}

fn new_player(app: &TestApp) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    let user = format!("UID-mmfail-{n}");
    app.run("INSERT INTO users (userId, name, notes) VALUES (?1, ?1, 0)", &[&user]);
    user
}

/// The three ways a join reaches the deploy server, with the body the 1.4.4 client sends for each
struct Path {
    name: &'static str,
    players: usize,
}

const PATHS: [Path; 3] = [
    Path {
        name: "going to Ramsgate",
        players: 1,
    },
    Path {
        name: "the tutorial",
        players: 1,
    },
    Path {
        name: "a full hunt queue of 4",
        players: 4,
    },
];

fn join_body(path: &Path) -> Value {
    match path.name {
        "going to Ramsgate" => {
            json!({"gameMode": "CITY", "gameArgs": "", "playerHuntId": "ShatteredIsles_ReturnToRamsgate"})
        }
        "the tutorial" => json!({"gameMode": "ISLAND", "gameArgs": TUTORIAL_GAME_ARGS, "playerHuntId": ""}),
        // A new hunt each time, so every group gets a queue of its own; the fourth join starts it
        _ => {
            json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": format!("CR19_PlayerHunt_Failure_Test_{}", COUNTER.fetch_add(1, Ordering::SeqCst))})
        }
    }
}

impl Mm {
    async fn join(&self, user: &str, body: Value) -> Reply {
        self.app.post("/candidate/join", req().bearer(user).json(body)).await
    }

    async fn status(&self, user: &str) -> Reply {
        self.app.get("/candidate/status", req().bearer(user)).await
    }

    /// New players join along the path, all with the same body
    async fn join_all(&self, path: &Path) -> Vec<String> {
        let players: Vec<String> = (0..path.players).map(|_| new_player(&self.app)).collect();
        let body = join_body(path);
        for player in &players {
            let joined = self.join(player, body.clone()).await;
            assert_eq!(joined.status, 200, "{}: join", path.name);
            assert_eq!(joined.json["status"], "MATCHING");
        }
        players
    }

    async fn wait_for_status(&self, user: &str, expected: &str) -> Reply {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let last = self.status(user).await;
            if last.json["status"] == expected || std::time::Instant::now() >= deadline {
                assert_eq!(last.json["status"], expected, "{user}");
                return last;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    fn clock(&self) -> &metagame::time::Clock {
        &self.app.state.party_clock
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn private_hunts_allocate_immediately_poll_quickly_and_never_take_a_public_waiting_player() {
    let m = mm().await;
    let hunt = format!(
        "CR19_PlayerHunt_Private_Fast_{}",
        COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let (public, private) = (new_player(&m.app), new_player(&m.app));
    let body = json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": hunt});
    m.join(&public, body.clone()).await;
    assert!(
        m.deploy.calls().is_empty(),
        "public queue should still wait for players"
    );
    m.deploy.hold(true);
    let mut private_body = body.clone();
    private_body["privateMatch"] = json!(true);
    let joined = m.join(&private, private_body).await;
    assert_eq!(joined.json["candidateStatusPeriodMillis"], 1000);
    m.deploy.wait_for_calls(1).await;
    let pending = m.status(&private).await;
    assert_eq!(
        (
            pending.json["status"].clone(),
            pending.json["candidateStatusPeriodMillis"].clone()
        ),
        (json!("MATCHING"), json!(1000))
    );
    assert_eq!(pending.json.get("connection"), None, "no destination before readiness");
    assert!(!m.deploy.calls()[0].to_string().contains(&public));
    m.deploy.hold(false);
    m.deploy.release();
    m.wait_for_status(&private, "IN_PROGRESS").await;
    assert_eq!(m.status(&public).await.json["status"], "MATCHING");
    matchmaking::cancel(&mut m.app.state.parties.lock(), &public, m.clock().now());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cold_hunt_does_not_block_join_status_requests_or_allocate_twice() {
    let m = mm().await;
    m.deploy.hold(true);
    let started = std::time::Instant::now();
    let players = m.join_all(&PATHS[2]).await;
    assert!(
        started.elapsed() < std::time::Duration::from_millis(2300),
        "join waited for the cold game process"
    );
    m.deploy.wait_for_calls(1).await;
    for player in &players {
        assert_eq!(m.status(player).await.json["status"], "MATCHING");
    }
    m.deploy.hold(false);
    m.deploy.release();
    for _ in 0..30 {
        if m.status(&players[0]).await.json["status"] == "IN_PROGRESS" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("ready hunt was never returned");
}

#[tokio::test(flavor = "multi_thread")]
async fn capacity_waits_retry_once_after_ten_seconds_and_reach_the_same_server_on_every_path() {
    let m = mm().await;
    m.clock().set_for_tests(metagame::time::now_ms());
    for path in &PATHS {
        m.deploy.clear();
        m.deploy.set(capacity("memory", Some("10")));
        let players = m.join_all(path).await;
        m.deploy.wait_for_calls(1).await;
        for player in &players {
            assert_eq!(m.status(player).await.json["status"], "MATCHING");
        }
        if path.players == 4 {
            let hunt = m.deploy.calls()[0]["HuntId"].as_str().unwrap().to_owned();
            let fifth = new_player(&m.app);
            assert!(
                matchmaking::join(
                    &m.app.state,
                    Some(json!("ISLAND")),
                    Some(json!("")),
                    Some(json!(hunt)),
                    &fifth,
                    false
                )
                .await
                .unwrap()
            );
            matchmaking::cancel(&mut m.app.state.parties.lock(), &fifth, m.clock().now());
        }
        m.deploy.set(working);
        m.clock().advance_for_tests(9999);
        let polls = players.iter().flat_map(|p| (0..4).map(move |_| p)).map(|p| m.status(p));
        futures_util::future::join_all(polls).await;
        assert_eq!(m.deploy.calls().len(), 1, "concurrent polls must honor Retry-After");
        m.clock().advance_for_tests(2);
        for player in &players {
            m.wait_for_status(player, "IN_PROGRESS").await;
        }
        assert_eq!(m.deploy.calls().len(), 2, "{}", path.name);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_steady_trickle_of_joins_still_sends_the_queue_5_seconds_after_the_first_join() {
    let m = mm().await;
    m.clock().set_for_tests(metagame::time::now_ms());
    let body = json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": format!("CR19_PlayerHunt_Trickle_{}", COUNTER.fetch_add(1, Ordering::SeqCst))});
    let players: Vec<String> = (0..3).map(|_| new_player(&m.app)).collect();
    for player in &players {
        m.join(player, body.clone()).await;
        m.clock().advance_for_tests(2000);
        m.status(&players[0]).await;
    }
    m.deploy.wait_for_calls(1).await;
    let mut expected: Vec<String> = m.deploy.calls()[0]["ExpectedPlayers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    expected.sort();
    let mut joined = players.clone();
    joined.sort();
    assert_eq!(expected, joined);
    for player in &players {
        m.wait_for_status(player, "IN_PROGRESS").await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn players_who_join_while_a_launch_is_out_share_its_capacity_wait_and_its_server() {
    let m = mm().await;
    m.clock().set_for_tests(metagame::time::now_ms());
    let body = json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": format!("CR19_PlayerHunt_MidLaunch_{}", COUNTER.fetch_add(1, Ordering::SeqCst))});
    let (first, late) = (new_player(&m.app), new_player(&m.app));
    m.deploy.set(capacity("memory", None));
    m.deploy.hold(true);
    m.join(&first, body.clone()).await;
    m.clock().advance_for_tests(20_001);
    m.status(&first).await;
    m.deploy.wait_for_calls(1).await;
    m.join(&late, body).await;
    m.deploy.hold(false);
    m.deploy.release();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    m.deploy.set(working);
    m.clock().advance_for_tests(3001);
    for player in [&first, &late] {
        m.wait_for_status(player, "IN_PROGRESS").await;
    }
    let calls = m.deploy.calls();
    assert_eq!(calls.len(), 2, "one server for both, not one each");
    let mut expected: Vec<&str> = calls[1]["ExpectedPlayers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    expected.sort();
    let mut both = vec![first.as_str(), late.as_str()];
    both.sort();
    assert_eq!(expected, both);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_solo_candidate_can_be_cancelled_without_launching_when_capacity_returns() {
    let m = mm().await;
    m.deploy.set(capacity("ports", None));
    let players = m.join_all(&PATHS[1]).await;
    matchmaking::cancel(&mut m.app.state.parties.lock(), &players[0], m.clock().now());
    assert!(matchmaking::check_and_update(&m.app.state, &players[0]).await.is_none());
    assert_eq!(m.deploy.calls().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn active_capacity_waiters_remain_queued_beyond_a_minute_and_enter_when_capacity_returns() {
    for path in &PATHS {
        let m = mm().await;
        m.clock().set_for_tests(metagame::time::now_ms());
        m.deploy.set(capacity("hunts", None));
        let players = m.join_all(path).await;
        m.deploy.wait_for_calls(1).await;
        for _ in 0..6 {
            m.clock().advance_for_tests(20_000);
            for player in &players {
                assert_eq!(m.status(player).await.json["status"], "MATCHING", "{}", path.name);
            }
        }
        m.deploy.set(working);
        m.clock().advance_for_tests(4000);
        for player in &players {
            m.wait_for_status(player, "IN_PROGRESS").await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn capacity_waiting_fails_after_one_minute_on_every_path_without_another_allocation() {
    let m = mm().await;
    m.clock().set_for_tests(metagame::time::now_ms());
    m.deploy.set(capacity("memory", None));
    for path in &PATHS {
        m.deploy.clear();
        let players = m.join_all(path).await;
        m.deploy.wait_for_calls(1).await;
        m.clock().advance_for_tests(60_001);
        for player in &players {
            m.wait_for_status(player, "FAILED").await;
        }
        assert_eq!(m.deploy.calls().len(), 1, "{}", path.name);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_successful_start_sends_every_player_to_the_returned_address_in_progress() {
    let m = mm().await;
    for path in &PATHS {
        m.deploy.clear();
        let players = m.join_all(path).await;
        m.deploy.wait_for_calls(1).await;
        for player in &players {
            let status = m.wait_for_status(player, "IN_PROGRESS").await;
            assert_eq!(status.json["statusReason"], Value::Null);
            assert_eq!(
                json!([status.json["serverInfo"]["host"], status.json["serverInfo"]["port"]]),
                json!(["127.0.0.1", 8790]),
                "{}",
                path.name
            );
        }
    }
}

/// Each failure mode is FAILED for every player on all three paths, with no address to travel to
async fn failed_on_every_path(answer: impl Fn(&Value, usize) -> Answer + Send + Sync + 'static) {
    let m = mm().await;
    m.deploy.set(answer);
    for path in &PATHS {
        m.deploy.clear();
        let players = m.join_all(path).await;
        m.deploy.wait_for_calls(1).await;
        if path.players == 4 {
            assert_eq!(m.deploy.calls()[0]["ExpectedPlayers"], json!(players));
        }
        for player in &players {
            let status = m.wait_for_status(player, "FAILED").await;
            assert_eq!(status.status, 200, "{}", path.name);
            assert_eq!(status.json["statusReason"], Value::Null);
            assert_eq!(status.json.get("serverInfo"), None, "no address to travel to");
            match matchmaking::decide_status(&m.app.state, player).await {
                Decision::Failed(entry) => assert_eq!((entry.ready, entry.port), (false, 0)),
                _ => panic!("not failed"),
            }
        }
    }
}

fn reply(status: u16, body: &'static str) -> impl Fn(&Value, usize) -> Answer {
    move |_, _| Answer::Reply(status, body.to_owned(), Vec::new())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_error_status_500_is_failed_for_every_player_never_ready_on_every_path() {
    failed_on_every_path(reply(500, "{}")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_error_status_503_is_failed_for_every_player_never_ready_on_every_path() {
    failed_on_every_path(reply(503, "{}")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_200_with_no_host_is_failed_for_every_player_never_ready_on_every_path() {
    failed_on_every_path(reply(200, r#"{"host":"","port":0}"#)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_200_with_the_port_in_quotes_is_failed_for_every_player_never_ready_on_every_path() {
    failed_on_every_path(reply(200, r#"{"host":"127.0.0.1","port":"8790"}"#)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_200_that_is_not_json_is_failed_for_every_player_never_ready_on_every_path() {
    failed_on_every_path(reply(200, "not json")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_connection_is_failed_for_every_player_never_ready_on_every_path() {
    failed_on_every_path(|_, _| Answer::Drop).await;
}
