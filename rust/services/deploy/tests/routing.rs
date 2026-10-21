#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The HTTP routes and the routing to other machines (`test/matchmaker.test.ts`,
//! `workersnapshot.test.ts`, `overflow.test.ts`, `regions.test.ts`, `readiness.test.ts`). Nothing here
//! starts a game process.

mod common;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use deploy::capacity::{AdmissionStatus, DeployError, Reason};
use deploy::cities::Connection;
use deploy::matchmaker::Matchmaker;
use deploy::remote::{
    HuntRouter, Region, RegionalRouter, Remote, RemoteFn, Snapshot, read_worker_snapshot, region_url,
};
use deploy::settings::Settings;
use serde_json::{Value, json};
use url::Url;

async fn serve(router: axum::Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });
    port
}

async fn deploy_server(w: &World) -> u16 {
    serve(deploy::app::router(Matchmaker::new(w.deploy.clone()))).await
}

/// One request: the status and the body text.
async fn call(port: u16, method: &str, path: &str, body: Option<Value>, headers: &[(&str, &str)]) -> (u16, String) {
    let http = deploy::remote::Http::new();
    let url = Url::parse(&format!("http://127.0.0.1:{port}{path}")).unwrap();
    let answer = http
        .call(method, &url, body.as_ref(), Duration::from_secs(10), headers)
        .await
        .unwrap();
    let text = answer.body.map(|b| b.to_string()).unwrap_or_default();
    (answer.status, text)
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_input_gets_400_and_starts_nothing() {
    let w = world(Mode::Running, &[]);
    let port = deploy_server(&w).await;
    let path = "/api/matchmaker/handle-matchmaking-for-player";
    let (status, body) = call(
        port,
        "POST",
        path,
        Some(json!({ "GameMode": "ISLAND", "GameArgs": "/Game/Maps/islands/x -ExecCmds=quit?MaxPlayers=1?MonsterClass=/Game/M/b.b_C" })),
        &[],
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"], "bad_request");
    let hunt = json!({ "GameMode": "ISLAND", "GameArgs": "", "HuntId": "CR19_PlayerHunt_Arena -ExecCmds=quit", "ExpectedPlayers": ["UID-a"] });
    assert_eq!(call(port, "POST", path, Some(hunt), &[]).await.0, 400);
    let players = json!({ "GameMode": "ISLAND", "GameArgs": "", "HuntId": "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", "ExpectedPlayers": ["UID-a:x,UID-b"] });
    assert_eq!(call(port, "POST", path, Some(players), &[]).await.0, 400);
    assert_eq!(
        call(port, "POST", path, Some(json!({ "Region": "moon" })), &[]).await.0,
        400
    );
    assert!(w.deploy.gameservers().is_empty(), "no game server was started");
}

#[tokio::test(flavor = "multi_thread")]
async fn anything_relayed_by_a_proxy_gets_403() {
    let w = world(Mode::Running, &[]);
    let port = deploy_server(&w).await;
    let path = "/api/matchmaker/handle-matchmaking-for-player";
    for (name, value) in [
        ("x-forwarded-for", "203.0.113.9"),
        ("x-dauntless-gateway", "secret"),
        ("via", "1.1 relay"),
        ("forwarded", "for=203.0.113.9"),
    ] {
        let body = json!({ "GameMode": "ISLAND", "GameArgs": "/Game/Maps/x -ExecCmds=quit" });
        assert_eq!(
            call(port, "POST", path, Some(body), &[(name, value)]).await.0,
            403,
            "{name}"
        );
    }
    assert_eq!(
        call(port, "GET", "/gameservers", None, &[("x-forwarded-for", "203.0.113.9")])
            .await
            .0,
        403
    );
    assert_eq!(call(port, "GET", "/gameservers", None, &[]).await.0, 200);
    assert!(w.deploy.gameservers().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn gameservers_answers_the_running_servers() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap();
    let port = deploy_server(&w).await;
    let (status, body) = call(port, "GET", "/gameservers", None, &[]).await;
    assert_eq!(status, 200);
    let body: Value = serde_json::from_str(&body).unwrap();
    let kinds: Vec<(u64, &str)> = body["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["port"].as_u64().unwrap(), s["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(kinds[0], (8777, "city"));
    assert_eq!(kinds[1].1, "tutorial");
    assert_eq!(body["complete"], true);
    assert_eq!(body["capacity"]["running"], 1);
    assert!(body["capacity"]["cpu"].is_object());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_start_answers_500_which_the_metagame_reports_as_failed() {
    let w = world(Mode::Running, &[]);
    w.deploy.startup().await.unwrap();
    w.platform.kill(w.deploy.state_for_tests().ramsgate.unwrap().process_id);
    w.platform.set_mode(Mode::Failing);
    let port = deploy_server(&w).await;
    let body = json!({ "GameMode": "CITY", "GameArgs": "", "HuntId": "ShatteredIsles_ReturnToRamsgate" });
    let (status, text) = call(
        port,
        "POST",
        "/api/matchmaker/handle-matchmaking-for-player",
        Some(body),
        &[],
    )
    .await;
    assert_eq!(
        (status, serde_json::from_str::<Value>(&text).unwrap()),
        (500, json!({ "error": "no_game_server" }))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_capacity_refusal_answers_503_with_retry_after() {
    let w = world(Mode::Running, &[("MAX_LOCAL_HUNTS", "1")]);
    w.deploy.startup().await.unwrap();
    w.deploy.start_with_args(TUTORIAL_ARGS).await.unwrap();
    let port = deploy_server(&w).await;
    let body = json!({ "GameMode": "ISLAND", "GameArgs": TUTORIAL_ARGS });
    let (status, text) = call(
        port,
        "POST",
        "/api/matchmaker/handle-matchmaking-for-player",
        Some(body),
        &[],
    )
    .await;
    assert_eq!(status, 503);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({ "error": "capacity_unavailable", "reason": "hunts" })
    );
}

// ---- worker snapshots ----

#[derive(Clone)]
struct Worker {
    status: Arc<AtomicU16>,
    body: Arc<Mutex<String>>,
    posts: Arc<AtomicU32>,
    post_reply: Arc<Mutex<(u16, Value, Option<String>)>>,
}

async fn worker() -> (Worker, Url) {
    let state = Worker {
        status: Arc::new(AtomicU16::new(200)),
        body: Arc::new(Mutex::new(json!({ "servers": [], "complete": true }).to_string())),
        posts: Arc::default(),
        post_reply: Arc::new(Mutex::new((200, json!({ "host": "worker", "port": 8780 }), None))),
    };
    let s = state.clone();
    let handler = move |request: axum::extract::Request| {
        let s = s.clone();
        async move {
            use axum::response::IntoResponse;
            if request.method() == "POST" {
                s.posts.fetch_add(1, Ordering::SeqCst);
                let (status, body, retry) = s.post_reply.lock().unwrap().clone();
                let mut reply = (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    body.to_string(),
                )
                    .into_response();
                if let Some(retry) = retry {
                    reply.headers_mut().insert("retry-after", retry.parse().unwrap());
                }
                return reply;
            }
            let status = axum::http::StatusCode::from_u16(s.status.load(Ordering::SeqCst)).unwrap();
            (
                status,
                [("content-type", "application/json")],
                s.body.lock().unwrap().clone(),
            )
                .into_response()
        }
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port())).unwrap();
    tokio::spawn(async move { axum::serve(listener, axum::Router::new().fallback(handler)).await });
    (state, url)
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_snapshots_tell_missing_sessions_from_incomplete_ones() {
    let (worker, url) = worker().await;
    let http = deploy::remote::Http::new();
    assert_eq!(
        read_worker_snapshot(&http, &url).await,
        Snapshot {
            servers: Vec::new(),
            complete: true
        }
    );
    *worker.body.lock().unwrap() =
        json!({ "servers": [{ "id": "still-alive", "port": 8790 }], "complete": false }).to_string();
    assert_eq!(
        read_worker_snapshot(&http, &url).await,
        Snapshot {
            servers: vec![json!({ "id": "still-alive", "port": 8790 })],
            complete: false
        }
    );

    // Through the deploy server's own route
    let w = world(Mode::Running, &[("OVERFLOW_DEPLOYSERVER_URL", url.as_str())]);
    let port = deploy_server(&w).await;
    let partial: Value = serde_json::from_str(&call(port, "GET", "/gameservers", None, &[]).await.1).unwrap();
    assert_eq!(
        partial["complete"], false,
        "a missing worker snapshot must not certify a session dead"
    );
    assert!(
        partial["servers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "still-alive" && s["host"] == "overflow")
    );

    *worker.body.lock().unwrap() = json!({ "servers": [] }).to_string();
    assert!(
        !read_worker_snapshot(&http, &url).await.complete,
        "older workers cannot certify a complete aggregate"
    );
    *worker.body.lock().unwrap() = json!({ "servers": {}, "complete": true }).to_string();
    assert_eq!(
        read_worker_snapshot(&http, &url).await,
        Snapshot {
            servers: Vec::new(),
            complete: false
        }
    );
    worker.status.store(500, Ordering::SeqCst);
    assert!(!read_worker_snapshot(&http, &url).await.complete);
    worker.status.store(200, Ordering::SeqCst);
    *worker.body.lock().unwrap() = "{\"servers\":".into();
    assert!(
        !read_worker_snapshot(&http, &url).await.complete,
        "a broken body is unknown"
    );
    let closed = Url::parse(&format!("http://127.0.0.1:{}", dead_port())).unwrap();
    assert!(
        !read_worker_snapshot(&http, &closed).await.complete,
        "a closed worker is unknown, not an empty fleet"
    );
}

fn dead_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

// ---- overflow ----

fn body() -> Value {
    json!({ "GameMode": "ISLAND", "GameArgs": "", "HuntId": "pursuit", "ExpectedPlayers": ["one", "two"] })
}

fn connection(host: &str, port: u16) -> Connection {
    Connection {
        host: host.into(),
        port,
        session_id: None,
    }
}

fn remote_answering(answer: Option<Connection>, calls: Arc<AtomicU32>) -> RemoteFn {
    Arc::new(move |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        let answer = answer.clone();
        Box::pin(async move { Ok(answer) })
    })
}

fn overflow_settings() -> Settings {
    Settings::from_pairs([
        ("OVERFLOW_DEPLOYSERVER_URL", "http://127.0.0.1:61011"),
        ("OVERFLOW_AFTER_HUNTS", "4"),
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn the_threshold_spills_whole_hunts_and_persistent_worlds_stay() {
    let (local, worker) = (connection("main", 8770), connection("worker", 8780));
    let calls = Arc::new(AtomicU32::new(0));
    let router = HuntRouter::new(
        overflow_settings(),
        || 4,
        remote_answering(Some(worker.clone()), calls.clone()),
        None,
    );
    assert_eq!(
        router.launch(&body(), async { Ok(local.clone()) }).await.unwrap(),
        worker
    );
    let mut city = body();
    city["GameMode"] = json!("CITY");
    assert_eq!(router.launch(&city, async { Ok(local.clone()) }).await.unwrap(), local);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Below the threshold the primary is preferred; both full refuses without another spawn
    let calls = Arc::new(AtomicU32::new(0));
    let router = HuntRouter::new(overflow_settings(), || 0, remote_answering(None, calls.clone()), None);
    assert_eq!(
        router.launch(&body(), async { Ok(local.clone()) }).await.unwrap(),
        local
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let full = router
        .launch(&body(), async { Err(DeployError::Capacity(Reason::Hunts)) })
        .await;
    assert_eq!(full.unwrap_err(), DeployError::Capacity(Reason::Hunts));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Local capacity falls back to the worker; a worker refusal falls back here
    let router = HuntRouter::new(
        overflow_settings(),
        || 0,
        remote_answering(Some(worker.clone()), Arc::default()),
        None,
    );
    assert_eq!(
        router
            .launch(&body(), async { Err(DeployError::Capacity(Reason::Ports)) })
            .await
            .unwrap(),
        worker
    );
    let router = HuntRouter::new(overflow_settings(), || 4, remote_answering(None, Arc::default()), None);
    assert_eq!(
        router.launch(&body(), async { Ok(local.clone()) }).await.unwrap(),
        local
    );

    // An ambiguous worker failure never starts a duplicate here
    let failing: RemoteFn = Arc::new(|_, _| Box::pin(async { Err(DeployError::failed("timeout")) }));
    let local_calls = Arc::new(AtomicU32::new(0));
    let c = local_calls.clone();
    let router = HuntRouter::new(overflow_settings(), || 4, failing, None);
    let request = body();
    let result = router.launch(&request, async move {
        c.fetch_add(1, Ordering::SeqCst);
        Ok(connection("main", 8770))
    });
    assert_eq!(result.await.unwrap_err(), DeployError::failed("timeout"));
    assert_eq!(local_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_launches_count_toward_the_threshold() {
    let worker = connection("worker", 8780);
    let router = Arc::new(HuntRouter::new(
        overflow_settings(),
        || 3,
        remote_answering(Some(worker.clone()), Arc::default()),
        None,
    ));
    let (finish, gate) = tokio::sync::oneshot::channel::<()>();
    let first = {
        let router = router.clone();
        tokio::spawn(async move {
            router
                .launch(&body(), async move {
                    gate.await.ok();
                    Ok(connection("main", 8770))
                })
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        router
            .launch(&body(), async { Ok(connection("main", 8770)) })
            .await
            .unwrap(),
        worker
    );
    finish.send(()).unwrap();
    first.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_remote_contract_carries_the_hunt_and_recognizes_capacity() {
    let (worker, url) = worker().await;
    let remote = Remote::default();
    assert_eq!(
        remote.launch(&url, &body()).await.unwrap(),
        Some(connection("worker", 8780))
    );
    *worker.post_reply.lock().unwrap() = (503, json!({ "error": "capacity_unavailable" }), None);
    assert_eq!(remote.launch(&url, &body()).await.unwrap(), None);
    let closed = Url::parse(&format!("http://127.0.0.1:{}", dead_port())).unwrap();
    assert_eq!(Remote::default().launch(&closed, &body()).await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn capacity_cooldowns_are_shared_bounded_and_per_worker_and_world() {
    let (worker, url) = worker().await;
    *worker.post_reply.lock().unwrap() = (503, json!({ "error": "capacity_unavailable" }), Some("10".into()));
    let now = Arc::new(AtomicI64::new(1_000_000));
    let n = now.clone();
    let remote = Remote::new(Arc::new(move || n.load(Ordering::SeqCst)));
    assert_eq!(remote.launch(&url, &body()).await.unwrap(), None);
    assert_eq!(remote.launch(&url, &body()).await.unwrap(), None);
    assert_eq!(
        worker.posts.load(Ordering::SeqCst),
        1,
        "the refusal is shared until its Retry-After"
    );
    let mut city = body();
    city["GameMode"] = json!("CITY");
    remote.launch(&url, &city).await.unwrap();
    assert_eq!(
        worker.posts.load(Ordering::SeqCst),
        2,
        "another world has its own cooldown"
    );
    now.fetch_add(9999, Ordering::SeqCst);
    remote.launch(&url, &body()).await.unwrap();
    assert_eq!(worker.posts.load(Ordering::SeqCst), 2);
    now.fetch_add(1, Ordering::SeqCst);
    remote.launch(&url, &body()).await.unwrap();
    assert_eq!(worker.posts.load(Ordering::SeqCst), 3);

    for (value, delay) in [
        ("999999", 30_000),
        ("0.1", 1000),
        ("invalid", 10_000),
        ("-1", 10_000),
        ("", 10_000),
    ] {
        *worker.post_reply.lock().unwrap() = (
            503,
            json!({ "error": "capacity_unavailable" }),
            (!value.is_empty()).then(|| value.to_owned()),
        );
        let now = Arc::new(AtomicI64::new(2_000_000));
        let n = now.clone();
        let remote = Remote::new(Arc::new(move || n.load(Ordering::SeqCst)));
        let before = worker.posts.load(Ordering::SeqCst);
        remote.launch(&url, &body()).await.unwrap();
        now.fetch_add(delay - 1, Ordering::SeqCst);
        remote.launch(&url, &body()).await.unwrap();
        assert_eq!(worker.posts.load(Ordering::SeqCst), before + 1, "{value}");
        now.fetch_add(1, Ordering::SeqCst);
        remote.launch(&url, &body()).await.unwrap();
        assert_eq!(worker.posts.load(Ordering::SeqCst), before + 2, "{value}");
    }

    // Other HTTP errors never create a cooldown
    *worker.post_reply.lock().unwrap() = (503, json!({ "error": "different_failure" }), None);
    let remote = Remote::default();
    let before = worker.posts.load(Ordering::SeqCst);
    for _ in 0..2 {
        assert!(
            remote
                .launch(&url, &body())
                .await
                .unwrap_err()
                .to_string()
                .contains("Overflow launch failed")
        );
    }
    assert_eq!(worker.posts.load(Ordering::SeqCst), before + 2);
}

// ---- regions ----

fn load() -> Arc<dyn Fn() -> deploy::process::BoxFuture<Result<AdmissionStatus, DeployError>> + Send + Sync> {
    Arc::new(|| {
        Box::pin(async {
            Ok(AdmissionStatus {
                running: 10,
                pending: 1,
                limit: Some(22),
            })
        })
    })
}

fn region_load(
    running: u64,
    limit: u64,
) -> Arc<dyn Fn(Url) -> deploy::process::BoxFuture<Result<AdmissionStatus, DeployError>> + Send + Sync> {
    Arc::new(move |_| {
        Box::pin(async move {
            Ok(AdmissionStatus {
                running,
                pending: 0,
                limit: Some(limit),
            })
        })
    })
}

fn regions(settings: &Settings, remote: RemoteFn) -> RegionalRouter {
    RegionalRouter::new(settings.clone(), load(), remote, region_load(0, 40))
}

#[tokio::test(flavor = "multi_thread")]
async fn chosen_regions_stay_regional_and_main_may_overflow_there() {
    let settings = Settings::from_pairs([
        ("US_DEPLOYSERVER_URL", "http://127.0.0.1:61041"),
        ("US_PUBLIC_HOST", "us"),
        ("GERMANY_DEPLOYSERVER_URL", "http://127.0.0.1:61031"),
        ("GERMANY_PUBLIC_HOST", "germany"),
    ]);
    let main = connection("main", 8762);
    for (region, port, host) in [(Region::Us, 61041, "us"), (Region::Ger, 61031, "germany")] {
        let answer = connection(host, 8900);
        let router = regions(
            &settings,
            Arc::new(move |url: Url, request: Value| {
                assert_eq!(url.port(), Some(port));
                assert_eq!(request["ExpectedPlayers"], json!(["one", "two"]));
                let answer = answer.clone();
                Box::pin(async move { Ok(Some(answer)) })
            }),
        );
        for mode in ["CITY", "SHARED", "ISLAND"] {
            let mut request = body();
            request["GameMode"] = json!(mode);
            let chosen = router
                .launch(&request, region, || async {
                    Err(DeployError::failed("Must not reach Main"))
                })
                .await;
            assert_eq!(chosen.unwrap().host, host);
        }
        let none = regions(&settings, remote_answering(None, Arc::default()));
        assert_eq!(
            none.launch(&body(), region, || async { Ok(main.clone()) })
                .await
                .unwrap_err(),
            DeployError::Capacity(Reason::Hunts)
        );
        let outside = regions(&settings, remote_answering(Some(main.clone()), Arc::default()));
        assert!(
            outside
                .launch(&body(), region, || async { Ok(connection("main", 8762)) })
                .await
                .unwrap_err()
                .to_string()
                .contains("outside")
        );
    }
    let germany = regions(
        &settings,
        remote_answering(Some(connection("germany", 8800)), Arc::default()),
    );
    let spilled = germany
        .launch(&body(), Region::Main, || async {
            Err(DeployError::Capacity(Reason::Cpu))
        })
        .await;
    assert_eq!(spilled.unwrap().host, "germany");
    settings.set("US_DEPLOYSERVER_URL", None);
    let router = regions(&settings, remote_answering(Some(connection("us", 1)), Arc::default()));
    assert_eq!(
        router
            .launch(&body(), Region::Us, || async { Ok(connection("main", 1)) })
            .await
            .unwrap_err(),
        DeployError::Capacity(Reason::Hunts)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mixed_parties_choose_by_utilization_and_us_slots_stay_reserved() {
    let settings = Settings::from_pairs([("US_DEPLOYSERVER_URL", "http://127.0.0.1:61041")]);
    let (main, us) = (connection("main", 8762), connection("us", 8700));
    let roomy = RegionalRouter::new(
        settings.clone(),
        load(),
        remote_answering(Some(us.clone()), Arc::default()),
        region_load(12, 40),
    );
    assert_eq!(
        roomy
            .launch(&body(), Region::Mixed, || async { Ok(connection("main", 8762)) })
            .await
            .unwrap(),
        us
    );
    let full = RegionalRouter::new(
        settings.clone(),
        load(),
        remote_answering(Some(us.clone()), Arc::default()),
        region_load(24, 24),
    );
    assert_eq!(
        full.launch(&body(), Region::Mixed, || async { Ok(connection("main", 8762)) })
            .await
            .unwrap(),
        main
    );

    settings.set("US_RESERVED_HUNTS", Some("8"));
    let calls = Arc::new(AtomicU32::new(0));
    let router = RegionalRouter::new(
        settings.clone(),
        load(),
        remote_answering(Some(us.clone()), calls.clone()),
        region_load(16, 24),
    );
    let refused = router
        .launch(&body(), Region::Main, || async {
            Err(DeployError::Capacity(Reason::Cpu))
        })
        .await;
    assert_eq!(refused.unwrap_err(), DeployError::Capacity(Reason::Cpu));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        router
            .launch(&body(), Region::Us, || async { Ok(connection("main", 1)) })
            .await
            .unwrap(),
        us
    );

    // An ambiguous failure never falls back
    let failing: RemoteFn = Arc::new(|_, _| Box::pin(async { Err(DeployError::failed("timeout")) }));
    let router = RegionalRouter::new(settings.clone(), load(), failing, region_load(0, 40));
    assert_eq!(
        router
            .launch(&body(), Region::Us, || async { Ok(connection("main", 1)) })
            .await
            .unwrap_err(),
        DeployError::failed("timeout")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tunnels_must_be_loopback_and_oceania_stays_retired() {
    let settings = Settings::from_pairs([
        ("US_DEPLOYSERVER_URL", "http://example.com"),
        ("AUS_DEPLOYSERVER_URL", "http://127.0.0.1:61021"),
    ]);
    assert!(
        region_url(&settings, Region::Us)
            .unwrap_err()
            .to_string()
            .contains("loopback")
    );
    assert_eq!(region_url(&settings, Region::Aus), Ok(None));
    let calls = Arc::new(AtomicU32::new(0));
    let router = regions(&settings, remote_answering(Some(connection("main", 1)), calls.clone()));
    for mode in ["CITY", "SHARED", "ISLAND"] {
        let mut request = body();
        request["GameMode"] = json!(mode);
        assert_eq!(
            router
                .launch(&request, Region::Aus, || async { Ok(connection("main", 1)) })
                .await
                .unwrap_err(),
            DeployError::Capacity(Reason::Hunts)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// ---- readiness ----

#[tokio::test(flavor = "multi_thread")]
async fn readiness_accepts_the_marker_by_port_and_sees_an_early_exit() {
    let _serial = serial().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ready");
    let platform = FakePlatform::new(Mode::Running);
    use deploy::process::Platform;
    let child = platform.spawn("x", &[], &Default::default()).unwrap();
    std::fs::write(&file, "999:8764").unwrap();
    deploy::readiness::wait_for_server_ready(child.clone(), 8764, &file, 100)
        .await
        .unwrap();
    assert!(!file.exists());
    std::fs::write(&file, "999:8765").unwrap();
    let error = deploy::readiness::wait_for_server_ready(child.clone(), 8764, &file, 50)
        .await
        .unwrap_err();
    assert!(error.contains("did not start listening"), "{error}");
    let pid = child.pid().unwrap();
    let ending = platform.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        ending.exit(pid, 1);
    });
    let error = deploy::readiness::wait_for_server_ready(child, 8764, &file, 1000)
        .await
        .unwrap_err();
    assert!(error.contains("exited before listening"), "{error}");
    assert!(!file.exists());
}

// ---- databaserole.test.ts ----

#[test]
fn a_database_only_node_refuses_to_start_the_deploy_service() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_deploy"))
        .current_dir(dir.path())
        .env("SERVER_ROLE", "database-only")
        .env("PORT", "0")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Deploy service disabled on database-only node"));
}
