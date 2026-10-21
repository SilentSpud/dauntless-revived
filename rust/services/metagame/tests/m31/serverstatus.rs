//! `serverstatus.test.ts`: the ServerStatus build and its caches against a fake deploy server, and who
//! counts as a registered caller (`SoftAccountAuth`, the full or limited answer of GET
//! /undaunted/api/ServerStatus). The helper-level cases are unit tests in `src/serverstatus.rs`.
//! Node's `Date.now` moves become a real wait (the cache's 5 s) and the heartbeat list read at a later
//! time.

use metagame::serverstatus;
use serde_json::{Value, json};

use crate::common::{Req, TestApp, req, sha256_hex};
use crate::deploykit::{FakeDeploy, three_servers};

const STATUS_KEYS: [&str; 12] = [
    "name",
    "online",
    "version",
    "commit",
    "sourceUrl",
    "registration",
    "playersOnline",
    "players",
    "instances",
    "contentPort",
    "uptimeSeconds",
    "limited",
];
const TUTORIAL_GAME_ARGS: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";

/// The `ServerStatus` setting: five players, Charlie sent to the tutorial server by matchmaking, and
/// their heartbeats. `deploy` is `None` for a deploy server that is down.
async fn status_app(deploy: Option<&FakeDeploy>) -> TestApp {
    let address = deploy.map_or("127.0.0.1:1".to_owned(), |d| d.address.clone());
    let app = TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", address.as_str()),
    ])
    .await;
    for (id, shown) in [
        ("UID-status-alpha", "Alpha"),
        ("UID-status-bravo", "bravo"),
        ("UID-status-charlie", "Charlie"),
        ("UID-status-delta", "Delta"),
        ("UID-status-echo", "Echo"),
    ] {
        app.run(
            "INSERT INTO users (userId, name, notes) VALUES (?1, ?2, 0)",
            &[&id, &shown],
        );
    }
    // Charlie is sent to the tutorial server (8775) by matchmaking, as the client asks for it
    let joined = metagame::matchmaking::join(
        &app.state,
        Some(json!("ISLAND")),
        Some(json!(TUTORIAL_GAME_ARGS)),
        None,
        "UID-status-charlie",
        false,
    )
    .await;
    assert!(joined.unwrap());
    beat(
        &app,
        "UID-status-alpha",
        "/Game/Maps/ramsgate/ramsgate_01_persistent",
        None,
    );
    beat(
        &app,
        "UID-status-bravo",
        "/Game/Maps/islands/1702/cora_jamima",
        Some("island"),
    );
    beat(
        &app,
        "UID-status-charlie",
        "/Game/Maps/islands/1705/dia_moss_triforce",
        None,
    );
    beat(&app, "UID-status-delta", "/Game/Maps/Map_LoginMenu", Some("menu"));
    beat(
        &app,
        "UID-status-echo",
        "/Game/Maps/islands/1803/frida_moss_falls",
        None,
    );
    beat(
        &app,
        "UID-no-such-account",
        "/Game/Maps/ramsgate/ramsgate_01_persistent",
        None,
    );
    // A heartbeat without a player token records nobody: the route never calls the activity store
    serverstatus::clear_cache(&app.state).await;
    app
}

fn beat(app: &TestApp, user: &str, map: &str, state: Option<&str>) {
    let state = state.map(|s| json!(s));
    app.state
        .activity
        .lock()
        .heartbeat(user, Some(json!(map)), state.as_ref(), metagame::time::now_ms());
}

async fn fake() -> FakeDeploy {
    FakeDeploy::start(three_servers("UID-status-bravo")).await
}

async fn status(app: &TestApp, full: bool) -> Value {
    serverstatus::status(&app.state, full).await.unwrap()
}

fn keys(value: &Value) -> Vec<&str> {
    value.as_object().unwrap().keys().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_native_occupancy_for_instance_counts_including_an_explicit_zero() {
    let deploy = fake().await;
    let app = status_app(Some(&deploy)).await;
    {
        let mut servers = deploy.servers.lock().unwrap();
        servers[0]["connectedPlayers"] = json!(7);
        servers[1]["connectedPlayers"] = json!(0);
    }
    let built = status(&app, true).await;
    let ids: Vec<Value> = deploy
        .servers
        .lock()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    let players_of = |id: &Value| {
        built["instances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| &i["id"] == id)
            .unwrap()["players"]
            .clone()
    };
    assert_eq!(players_of(&ids[0]), 7);
    assert_eq!(players_of(&ids[1]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_who_is_where_and_the_three_servers_with_titles_and_per_server_counts() {
    let deploy = fake().await;
    let app = status_app(Some(&deploy)).await;
    let built = status(&app, true).await;
    assert_eq!(keys(&built), STATUS_KEYS);
    assert_eq!(
        json!([built["online"], built["limited"], built["playersOnline"]]),
        json!([true, false, 5])
    );
    assert_eq!(
        built["players"],
        json!([
            {"name": "Alpha", "where": "city", "instance": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d01"},
            {"name": "bravo", "where": "hunt", "instance": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d02"},
            {"name": "Charlie", "where": "tutorial", "instance": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d03"},
            {"name": "Delta", "where": "menu", "instance": null},
            {"name": "Echo", "where": "hunt", "instance": null}
        ])
    );
    assert_eq!(
        built["instances"],
        json!([
            {"id": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d01", "kind": "city", "title": "Ramsgate", "map": "ramsgate_01_persistent", "behemoth": null, "players": 1, "maxPlayers": 32, "startedAt": "2026-09-21T10:00:00.000Z"},
            {"id": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d02", "kind": "hunt", "title": "Hunt: Lesser Embermane", "map": "cora_jamima", "behemoth": "Lesser Embermane", "players": 1, "maxPlayers": 4, "startedAt": "2026-09-21T11:00:00.000Z"},
            {"id": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d03", "kind": "tutorial", "title": "Tutorial", "map": "dia_moss_triforce", "behemoth": "Gnasher", "players": 1, "maxPlayers": 1, "startedAt": "2026-09-21T11:30:00.000Z"}
        ])
    );
    assert!(
        !regex::Regex::new(r"UID-|UUK_|127\.0\.0\.1|eyJ")
            .unwrap()
            .is_match(&built.to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_from_a_5_s_cache() {
    let deploy = fake().await;
    let app = status_app(Some(&deploy)).await;
    let first = status(&app, true).await;
    let asked = deploy.request_count();
    beat(&app, "UID-status-alpha", "/Game/Maps/Map_LoginMenu", None);
    let (second, third) = tokio::join!(status(&app, true), status(&app, true));
    assert_eq!((&second, &third), (&first, &first));
    assert_eq!(deploy.request_count(), asked, "no deploy-server call while cached");
    serverstatus::clear_cache(&app.state).await;
    let fresh = status(&app, true).await;
    let alpha = fresh["players"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Alpha")
        .unwrap()
        .clone();
    assert_eq!(alpha["where"], "menu");
}

#[tokio::test(flavor = "multi_thread")]
async fn limited_the_same_shape_with_no_players_and_no_servers_and_it_never_asks_the_deploy_server() {
    let deploy = fake().await;
    let app = status_app(Some(&deploy)).await;
    let asked = deploy.request_count();
    let full = status(&app, true).await;
    let limited = status(&app, false).await;
    assert_eq!(
        deploy.request_count(),
        asked + 1,
        "only the full status asked the deploy server"
    );
    assert_eq!(keys(&limited), STATUS_KEYS);
    assert_eq!(
        json!([
            limited["playersOnline"],
            limited["players"],
            limited["instances"],
            limited["limited"]
        ]),
        json!([0, [], [], true])
    );
    assert!(
        full["playersOnline"].as_i64().unwrap() > 0
            && !full["instances"].as_array().unwrap().is_empty()
            && full["limited"] == false
    );
    // Everything else is the same as the full answer
    for key in [
        "name",
        "online",
        "version",
        "commit",
        "sourceUrl",
        "registration",
        "contentPort",
    ] {
        assert_eq!(limited[key], full[key], "{key}");
    }
    assert!((limited["uptimeSeconds"].as_i64().unwrap() - full["uptimeSeconds"].as_i64().unwrap()).abs() <= 1);
    assert!(
        !regex::Regex::new("Alpha|bravo|Charlie|Delta|Echo|5c0a9e36|UID-")
            .unwrap()
            .is_match(&limited.to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn caches_each_variant_separately_a_limited_answer_never_serves_the_full_one_s_data_and_the_other_way_round() {
    let deploy = fake().await;
    let app = status_app(Some(&deploy)).await;
    let asked = deploy.request_count();
    let (limited, full) = tokio::join!(status(&app, false), status(&app, true));
    assert_eq!(
        (limited["limited"].clone(), limited["players"].as_array().unwrap().len()),
        (json!(true), 0)
    );
    assert_eq!(full["limited"], false);
    assert!(!full["players"].as_array().unwrap().is_empty());
    let (limited_again, full_again, _) = tokio::join!(status(&app, false), status(&app, true), status(&app, false));
    assert_eq!(limited_again, limited, "limited served from its cache");
    assert_eq!(full_again, full, "full served from its cache");
    assert_eq!(
        deploy.request_count(),
        asked + 1,
        "one deploy-server call for the full variant only"
    );
    // A fresh limited build (its cache expired) still does not touch the full one
    tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
    let later = status(&app, false).await;
    assert_ne!(later["uptimeSeconds"], limited["uptimeSeconds"], "built again");
    assert_eq!(later["limited"], true);
    assert_eq!(deploy.request_count(), asked + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn drops_a_heartbeat_older_than_90_s() {
    let deploy = fake().await;
    let app = status_app(Some(&deploy)).await;
    let now = metagame::time::now_ms();
    assert!(app.state.activity.lock().online(now + 91 * 1000).is_empty());
    // A fresh setting: the read above pruned nothing it may not (prunes are about a day)
    let again = status_app(Some(&deploy)).await;
    assert!(again.state.activity.lock().online(metagame::time::now_ms()).len() >= 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn still_answers_with_no_servers_when_the_deploy_server_is_down() {
    let app = status_app(None).await;
    let built = status(&app, true).await;
    assert_eq!(built["instances"], json!([]));
    assert_eq!(built["playersOnline"], 5);
    assert!(
        built["players"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["instance"].is_null())
    );
}

const PLAYER_KEY: &str = "UUK_0123456789abcdef0123456789abcdef0123456789abcdef";
const ADMIN_KEY: &str = "UUK_fedcba9876543210fedcba9876543210fedcba9876543210";
const GONE_KEY: &str = "UUK_00112233445566778899aabb00112233445566778899aabb";

async fn soft_app() -> TestApp {
    let app = TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", "127.0.0.1:1"),
    ])
    .await;
    app.exec("INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-soft-player', 'SoftPlayer', 0, 0), ('UID-soft-admin', 'SoftAdmin', 0, 1)");
    for (user, key) in [
        ("UID-soft-player", PLAYER_KEY),
        ("UID-soft-admin", ADMIN_KEY),
        ("UID-soft-gone", GONE_KEY),
    ] {
        // The last is a key whose account no longer exists
        app.run(
            "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
            &[&user, &sha256_hex(key)],
        );
    }
    app
}

/// Whether the caller counts as registered: ServerStatus answers it in full, never with a 401.
async fn registered(app: &TestApp, request: Req) -> bool {
    let reply = app.get("/undaunted/api/ServerStatus", request).await;
    assert_eq!(reply.status, 200, "SoftAccountAuth must never answer");
    reply.json["limited"] == false
}

fn key(value: &str) -> Req {
    req().header("x-undaunted-user-api-key", value)
}

#[tokio::test(flavor = "multi_thread")]
async fn no_key_anonymous() {
    let app = soft_app().await;
    assert!(!registered(&app, req()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_key_anonymous_never_a_401() {
    let app = soft_app().await;
    let long = "x".repeat(5000);
    let upper = PLAYER_KEY.to_uppercase();
    let extended = format!("{PLAYER_KEY}x");
    for bad in [
        "UUK_not_a_real_key",
        "",
        " ",
        extended.as_str(),
        upper.as_str(),
        "UID-soft-player",
        GONE_KEY,
        long.as_str(),
    ] {
        assert!(!registered(&app, key(bad)).await, "key of length {}", bad.len());
    }
    assert!(
        !registered(&app, key(PLAYER_KEY).header("x-undaunted-user-api-key", PLAYER_KEY)).await,
        "a repeated header is not a key"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_s_key_registered() {
    let app = soft_app().await;
    assert!(registered(&app, key(PLAYER_KEY)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_s_key_registered_like_any_account() {
    let app = soft_app().await;
    assert!(registered(&app, key(ADMIN_KEY)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_s_bearer_token_counts_a_bad_token_a_token_for_a_deleted_account_or_a_bad_key_next_to_a_good_token_do_not_change_that()
 {
    let app = soft_app().await;
    let token = app.token("UID-soft-player").await;
    let auth = |value: String| req().header("authorization", &value);
    assert!(registered(&app, auth(format!("bearer {token}"))).await);
    assert!(registered(&app, auth(format!("Bearer {token}"))).await);
    assert!(
        registered(
            &app,
            auth(format!("bearer {token}")).header("x-undaunted-user-api-key", "UUK_wrong")
        )
        .await
    );
    assert!(
        !registered(&app, auth(format!("bearer {}AAAA", &token[..token.len() - 4]))).await,
        "a forged signature"
    );
    assert!(!registered(&app, auth(token.clone())).await, "no bearer scheme");
    // A token for an account that does not exist: signed with the app's key directly
    let deleted = crate::socialported::sign_with("UID-soft-deleted", 60, None);
    assert!(
        !registered(&app, auth(format!("bearer {deleted}"))).await,
        "the account does not exist"
    );
}
