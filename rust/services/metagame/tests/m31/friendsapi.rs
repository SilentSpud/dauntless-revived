//! `friendsapi.test.ts`: the account routes and ServerStatus over HTTP, as the launcher and host scripts
//! call them, with the deploy server a fake. Node's file builds one state from test to test (the friend
//! registered first is renamed later); here each test sets up what it needs (`friends_app`).

use serde_json::{Value, json};

use crate::common::{Reply, Req, TestApp, req, sha256_hex};
use crate::deploykit::{FakeDeploy, three_servers};

const ADMIN_KEY: &str = "UUK_test_admin_key_for_this_process_only";
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

struct Friend {
    key: String,
    token: String,
    user: String,
}

/// `friendsenv.ts`'s settings and the admin account, plus `extra`.
async fn friends_app(deploy: Option<&FakeDeploy>, extra: &[(&str, &str)]) -> TestApp {
    let address = deploy.map_or("127.0.0.1:1".to_owned(), |d| d.address.clone());
    let mut env = vec![
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", address.as_str()),
        ("REGISTRATION_MODE", "OPEN"),
        ("CONTENT_PORT", "61002"),
    ];
    env.extend_from_slice(extra);
    let app = TestApp::with_env(&env).await;
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-admin', 'Slayer', 0, 1)",
        &[],
    );
    app.run(
        "INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-admin', ?1)",
        &[&sha256_hex(ADMIN_KEY)],
    );
    app
}

fn key(value: &str) -> Req {
    req().header("x-undaunted-user-api-key", value)
}

fn token(value: &str) -> Req {
    req().header("authorization", &format!("bearer {value}"))
}

async fn login(app: &TestApp, user_key: &str) -> String {
    let body = json!({"grant_type": "exchange_code", "exchange_code": user_key});
    let reply = app.post("/account/api/oauth/token", req().json(body)).await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    reply.json["access_token"].as_str().unwrap().to_owned()
}

async fn register(app: &TestApp, body: Value) -> Reply {
    app.post("/undaunted/api/Register", req().json(body)).await
}

/// The first test's friend: Hunter_1, registered, with a token.
async fn friend(app: &TestApp) -> Friend {
    let reply = register(app, json!({"Username": "Hunter_1"})).await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    let user_key = reply.json["UUK"].as_str().unwrap().to_owned();
    let info = app.get("/undaunted/api/GetUserInfo", key(&user_key)).await;
    Friend {
        token: login(app, &user_key).await,
        user: info.json["UserId"].as_str().unwrap().to_owned(),
        key: user_key,
    }
}

async fn rename(app: &TestApp, friend: &Friend, name: &str) -> Reply {
    let body = json!({"UserId": friend.user, "NewUsername": name});
    app.post("/undaunted/api/RenameUser", key(ADMIN_KEY).json(body)).await
}

async fn set_registration(app: &TestApp, mode: &str) {
    let body = json!({"RegistrationStatus": mode});
    assert_eq!(
        app.post("/undaunted/api/RegistrationStatus", key(ADMIN_KEY).json(body))
            .await
            .status,
        200
    );
}

fn assert_error(reply: &Reply, status: u16, error: &str) {
    assert_eq!(reply.status, status, "{}", reply.text);
    let kind = reply
        .headers
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    assert!(kind.contains("application/json"), "{kind}");
    assert_eq!(reply.json["error"], error);
    assert!(reply.json["message"].is_string());
}

fn keys(value: &Value) -> Vec<&str> {
    value.as_object().unwrap().keys().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_200_uuk_and_nothing_else_for_a_valid_new_name() {
    let app = friends_app(None, &[]).await;
    let reply = register(&app, json!({"Username": "Hunter_1"})).await;
    assert_eq!(reply.status, 200);
    assert_eq!(keys(&reply.json), ["UUK"]);
    let info = app
        .get("/undaunted/api/GetUserInfo", key(reply.json["UUK"].as_str().unwrap()))
        .await;
    assert_eq!(info.status, 200);
    let mut fields = keys(&info.json);
    fields.sort();
    assert_eq!(fields, ["IsAdmin", "UserId", "Username"]);
    assert_eq!(
        (info.json["Username"].clone(), info.json["IsAdmin"].clone()),
        (json!("Hunter_1"), json!(false))
    );
    login(&app, reply.json["UUK"].as_str().unwrap()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_with_the_json_errors_of_the_contract() {
    let app = friends_app(None, &[]).await;
    friend(&app).await;
    assert_error(
        &register(&app, json!({"Username": "ab"})).await,
        400,
        "username_invalid",
    );
    assert_error(
        &register(&app, json!({"Username": "bad name!"})).await,
        400,
        "username_invalid",
    );
    assert_error(
        &register(&app, json!({"Username": "SLAYER"})).await,
        409,
        "username_taken",
    );
    assert_error(
        &register(&app, json!({"Username": "hunter_1"})).await,
        409,
        "username_taken",
    );
    assert_error(&register(&app, json!({})).await, 400, "bad_request");
    assert_error(&register(&app, json!({"Username": 7})).await, 400, "bad_request");
    assert_error(&app.post("/undaunted/api/Register", req()).await, 400, "bad_request");
    assert_error(
        &app.post("/undaunted/api/Register", req().raw("{\"Username\": ")).await,
        400,
        "bad_request",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn says_whether_a_name_is_free() {
    let app = friends_app(None, &[]).await;
    friend(&app).await;
    assert_eq!(
        app.get("/undaunted/api/UsernameAvailable?Username=Brand_New", req())
            .await
            .json,
        json!({"available": true})
    );
    assert_eq!(
        app.get("/undaunted/api/UsernameAvailable?Username=HUNTER_1", req())
            .await
            .json["error"],
        "username_taken"
    );
    assert_eq!(
        app.get("/undaunted/api/UsernameAvailable?Username=x", req()).await.json["error"],
        "username_invalid"
    );
    assert_eq!(
        app.get("/undaunted/api/UsernameAvailable", req()).await.json["available"],
        false
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn createinvite_is_admin_only() {
    let app = friends_app(None, &[]).await;
    let friend = friend(&app).await;
    let path = "/undaunted/api/CreateInvite";
    assert_eq!(app.post(path, req().json(json!({}))).await.status, 401);
    assert_eq!(app.post(path, key("UUK_not_a_key").json(json!({}))).await.status, 401);
    assert_eq!(app.post(path, key(&friend.key).json(json!({}))).await.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn createinvite_makes_a_code_that_registers_exactly_as_many_accounts_as_it_has_uses() {
    let app = friends_app(None, &[]).await;
    friend(&app).await;
    set_registration(&app, "INVITECODE").await;
    assert_error(
        &register(&app, json!({"Username": "Invited_1"})).await,
        401,
        "invite_invalid",
    );
    assert_error(
        &register(&app, json!({"Username": "Invited_1", "InviteCode": "NOPE-NOPE-NOPE"})).await,
        401,
        "invite_invalid",
    );

    let created = app
        .post(
            "/undaunted/api/CreateInvite",
            key(ADMIN_KEY).json(json!({"uses": 2, "name": "for the Tuesday group"})),
        )
        .await;
    assert_eq!(created.status, 200);
    assert_eq!(keys(&created.json), ["code"]);
    let code = created.json["code"].as_str().unwrap().to_owned();
    let shape = regex::Regex::new("^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$").unwrap();
    assert!(shape.is_match(&code), "{code}");

    // A taken name does not use the code up
    assert_error(
        &register(&app, json!({"Username": "Hunter_1", "InviteCode": code})).await,
        409,
        "username_taken",
    );
    assert_eq!(
        register(&app, json!({"Username": "Invited_1", "InviteCode": code}))
            .await
            .status,
        200
    );
    assert_eq!(
        register(&app, json!({"Username": "Invited_2", "InviteCode": code}))
            .await
            .status,
        200
    );
    assert_error(
        &register(&app, json!({"Username": "Invited_3", "InviteCode": code})).await,
        401,
        "invite_invalid",
    );

    let single = app
        .post("/undaunted/api/CreateInvite", key(ADMIN_KEY).json(json!({})))
        .await
        .json["code"]
        .clone();
    assert_eq!(
        register(&app, json!({"Username": "Invited_3", "InviteCode": single}))
            .await
            .status,
        200
    );
    assert_error(
        &register(&app, json!({"Username": "Invited_4", "InviteCode": single})).await,
        401,
        "invite_invalid",
    );

    let listed = app.get("/undaunted/api/InviteCodes", key(ADMIN_KEY)).await;
    let entry = listed.json["InviteCodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["inviteCode"] == code.as_str())
        .unwrap()
        .clone();
    assert_eq!(entry["usesRemaining"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn createinvite_refuses_a_bad_use_count() {
    let app = friends_app(None, &[]).await;
    for body in [
        json!({"uses": 0}),
        json!({"uses": -1}),
        json!({"uses": 1.5}),
        json!({"uses": "2"}),
        json!({"uses": 1001}),
        json!({"name": 5}),
    ] {
        assert_error(
            &app.post("/undaunted/api/CreateInvite", key(ADMIN_KEY).json(body)).await,
            400,
            "bad_request",
        );
    }
    assert_error(
        &app.post("/undaunted/api/CreateInvite", key(ADMIN_KEY).raw("{uses:"))
            .await,
        400,
        "bad_request",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_registration_closed_when_registration_is_off() {
    let app = friends_app(None, &[]).await;
    set_registration(&app, "NONE").await;
    assert_error(
        &register(&app, json!({"Username": "Too_Late", "InviteCode": "ANY-CODE"})).await,
        400,
        "registration_closed",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn is_admin_only() {
    let app = friends_app(None, &[]).await;
    let friend = friend(&app).await;
    let body = json!({"UserId": friend.user, "NewUsername": "Renamed_1"});
    assert_eq!(
        app.post("/undaunted/api/RenameUser", req().json(body.clone()))
            .await
            .status,
        401
    );
    assert_eq!(
        app.post("/undaunted/api/RenameUser", key(&friend.key).json(body))
            .await
            .status,
        403
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn renames_and_the_client_sees_the_stored_name_everywhere_it_shows_one() {
    let app = friends_app(None, &[]).await;
    let friend = friend(&app).await;
    assert_eq!(
        app.get("/character", token(&friend.token)).await.json[0]["name"],
        "Hunter_1"
    );
    let renamed = rename(&app, &friend, "Renamed_1").await;
    assert_eq!(renamed.status, 200);
    assert_eq!(
        renamed.json,
        json!({"UserId": friend.user, "OldUsername": "Hunter_1", "Username": "Renamed_1"})
    );
    assert_eq!(
        app.get("/character", token(&friend.token)).await.json[0]["name"],
        "Renamed_1"
    );
    assert_eq!(
        app.get("/accountinfo", token(&friend.token)).await.json["username"],
        "Renamed_1"
    );
    assert_eq!(
        app.get("/account/api/public/account", token(&friend.token)).await.json["displayName"],
        "Renamed_1"
    );
    assert_eq!(
        app.post("/party", token(&friend.token).json(json!({}))).await.json["playerStates"][0]["displayName"],
        "Renamed_1"
    );
    assert_eq!(
        app.get("/undaunted/api/GetUserInfo", key(&friend.key)).await.json["Username"],
        "Renamed_1"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_taken_or_invalid_name_and_an_unknown_account() {
    let app = friends_app(None, &[]).await;
    let friend = friend(&app).await;
    assert_error(&rename(&app, &friend, "slayer").await, 409, "username_taken");
    assert_error(&rename(&app, &friend, "no").await, 400, "username_invalid");
    let unknown = json!({"Username": "Nobody_At_All", "NewUsername": "Fine_Name"});
    assert_error(
        &app.post("/undaunted/api/RenameUser", key(ADMIN_KEY).json(unknown))
            .await,
        404,
        "not_found",
    );
    let nameless = json!({"NewUsername": "Fine_Name"});
    assert_error(
        &app.post("/undaunted/api/RenameUser", key(ADMIN_KEY).json(nameless))
            .await,
        400,
        "bad_request",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_display_name_0_puts_the_old_display_name_back() {
    let app = friends_app(None, &[("ACCOUNT_DISPLAY_NAME", "0")]).await;
    let friend = friend(&app).await;
    assert_eq!(
        app.get("/account/api/public/account", token(&friend.token)).await.json["displayName"],
        json!({})
    );
    assert_eq!(
        app.post("/party", token(&friend.token).json(json!({}))).await.json["playerStates"][0]["displayName"],
        json!({})
    );
}

/// ServerStatus's setting: the fake deploy server with its three servers, the friend renamed and in
/// the hunt, the admin in Ramsgate.
async fn status_app() -> (TestApp, FakeDeploy, Friend) {
    let deploy = FakeDeploy::start(json!([])).await;
    let app = friends_app(Some(&deploy), &[]).await;
    let friend = friend(&app).await;
    rename(&app, &friend, "Renamed_1").await;
    *deploy.servers.lock().unwrap() = three_servers(&friend.user);
    metagame::serverstatus::clear_cache(&app.state).await;
    let hunt = app
        .post(
            "/heartbeat",
            token(&friend.token).json(json!({"map": "/Game/Maps/islands/1702/cora_jamima"})),
        )
        .await;
    assert_eq!(hunt.text, "20000");
    let admin = login(&app, ADMIN_KEY).await;
    let city = json!({"map": "/Game/Maps/ramsgate/ramsgate_01_persistent", "state": "city"});
    assert_eq!(app.post("/heartbeat", token(&admin).json(city)).await.status, 200);
    (app, deploy, friend)
}

async fn status(app: &TestApp, request: Req) -> Reply {
    app.get("/undaunted/api/ServerStatus", request).await
}

#[tokio::test(flavor = "multi_thread")]
async fn shows_a_registered_player_who_is_online_where_and_the_deploy_server_s_three_servers_with_no_ids_keys_or_addresses()
 {
    let (app, _deploy, friend) = status_app().await;
    let reply = status(&app, key(&friend.key)).await;
    assert_eq!(reply.status, 200);
    assert!(
        reply.headers["content-type"]
            .to_str()
            .unwrap()
            .contains("application/json")
    );
    let s = &reply.json;
    assert_eq!(keys(s), STATUS_KEYS);
    assert_eq!(
        json!([
            s["limited"],
            s["name"],
            s["online"],
            s["sourceUrl"],
            s["registration"],
            s["contentPort"],
            s["playersOnline"]
        ]),
        json!([
            false,
            "Dauntless Revived",
            true,
            "https://github.com/mixutin/dauntless-revived",
            "OPEN",
            61002,
            2
        ])
    );
    assert!(s["version"].is_string() && s["commit"].is_string());
    assert!(s["uptimeSeconds"].as_i64().is_some_and(|u| u >= 0));
    assert_eq!(
        s["players"],
        json!([
            {"name": "Renamed_1", "where": "hunt", "instance": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d02"},
            {"name": "Slayer", "where": "city", "instance": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d01"}
        ])
    );
    let instances: Vec<Value> = s["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| json!([i["kind"], i["title"], i["behemoth"], i["players"], i["maxPlayers"]]))
        .collect();
    assert_eq!(
        instances,
        [
            json!(["city", "Ramsgate", null, 1, 32]),
            json!(["hunt", "Hunt: Lesser Embermane", "Lesser Embermane", 1, 4]),
            json!(["tutorial", "Tutorial", "Gnasher", 0, 1])
        ]
    );
    for instance in s["instances"].as_array().unwrap() {
        assert_eq!(
            keys(instance),
            [
                "id",
                "kind",
                "title",
                "map",
                "behemoth",
                "players",
                "maxPlayers",
                "startedAt"
            ]
        );
        assert!(metagame::js::date_parse(instance["startedAt"].as_str().unwrap()).is_some());
    }
    assert!(!reply.text.contains(&friend.user) && !reply.text.contains("UID-admin"));
    assert!(
        !regex::Regex::new(r"UID-|UUK_|eyJ|127\.0\.0\.1")
            .unwrap()
            .is_match(&reply.text)
    );
    assert_eq!(reply.headers["cache-control"], "no-store");
    assert!(
        reply.headers["vary"]
            .to_str()
            .unwrap()
            .to_lowercase()
            .contains("x-undaunted-user-api-key")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_key_200_the_same_shape_with_no_players_and_no_servers_limited() {
    let (app, deploy, friend) = status_app().await;
    let full = status(&app, key(&friend.key)).await.json;
    let asked = deploy.request_count();
    metagame::serverstatus::clear_cache(&app.state).await;
    let reply = status(&app, req()).await;
    assert_eq!(reply.status, 200);
    assert_eq!(keys(&reply.json), STATUS_KEYS);
    let j = &reply.json;
    assert_eq!(
        json!([j["playersOnline"], j["players"], j["instances"], j["limited"]]),
        json!([0, [], [], true])
    );
    for field in [
        "name",
        "online",
        "version",
        "commit",
        "sourceUrl",
        "registration",
        "contentPort",
    ] {
        assert_eq!(j[field], full[field], "{field}");
    }
    assert!(
        !regex::Regex::new("Renamed_1|Slayer|5c0a9e36|UID-|UUK_|eyJ")
            .unwrap()
            .is_match(&reply.text)
    );
    assert_eq!(
        deploy.request_count(),
        asked,
        "a limited answer never asks the deploy server"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_key_or_a_bad_token_200_limited_never_a_401() {
    let (app, _deploy, friend) = status_app().await;
    let broken = format!("{}AAAAAA", &friend.token[..friend.token.len() - 6]);
    let options = [
        key("UUK_not_a_real_key_000000000000000000"),
        key(&format!("{}0", friend.key)),
        key("UID-admin"),
        token("not.a.token"),
        token(&broken),
    ];
    for (n, request) in options.into_iter().enumerate() {
        let reply = status(&app, request).await;
        assert_eq!(reply.status, 200, "{n}");
        let j = &reply.json;
        assert_eq!(
            json!([j["limited"], j["playersOnline"], j["players"], j["instances"]]),
            json!([true, 0, [], []]),
            "{n}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_s_key_an_admin_s_key_or_a_player_s_token_the_full_answer() {
    let (app, _deploy, friend) = status_app().await;
    let both = key("UUK_wrong").header("authorization", &format!("bearer {}", friend.token));
    for (n, request) in [key(&friend.key), key(ADMIN_KEY), token(&friend.token), both]
        .into_iter()
        .enumerate()
    {
        let reply = status(&app, request).await;
        assert_eq!(reply.status, 200);
        assert_eq!(reply.json["limited"], false, "{n}");
        assert_eq!(reply.json["playersOnline"], 2);
        let names: Vec<Value> = reply.json["players"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].clone())
            .collect();
        assert_eq!(names, [json!("Renamed_1"), json!("Slayer")]);
        assert_eq!(reply.json["instances"].as_array().unwrap().len(), 3);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_each_variant_from_its_own_5_s_cache() {
    let (app, deploy, friend) = status_app().await;
    metagame::serverstatus::clear_cache(&app.state).await;
    status(&app, req()).await;
    status(&app, token(&friend.token)).await;
    let asked = deploy.request_count();
    let (a, b, c, d) = tokio::join!(
        status(&app, req()),
        status(&app, req()),
        status(&app, key(&friend.key)),
        status(&app, key(ADMIN_KEY))
    );
    assert_eq!(a.status, 200);
    assert_eq!(a.text, b.text);
    assert_eq!(c.text, d.text);
    assert_ne!(a.text, c.text);
    assert_eq!(deploy.request_count(), asked);
}

const LANGUAGES: [&str; 8] = ["en", "fr", "it", "es", "de", "pt", "ru", "ja"];

async fn dauntless_status(app: &TestApp, request: Req) -> Value {
    app.get("/dauntless-status", request).await.json
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_nine_fields_the_client_reads_and_adds_the_source_link_and_says_nothing_about_players() {
    let (app, _deploy, friend) = status_app().await;
    let reply = dauntless_status(&app, req()).await;
    let fields = keys(&reply).into_iter().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(
        fields[..9],
        ["show-status", "en", "fr", "it", "es", "de", "pt", "ru", "ja"]
    );
    assert_eq!(reply["show-status"], true);
    assert_eq!(fields[9..], ["name", "version", "commit", "sourceUrl"]);
    assert_eq!(reply["sourceUrl"], "https://github.com/mixutin/dauntless-revived");
    // Two players are online (the heartbeats), yet anyone may call this route: not even the count is in
    // it, with or without a key
    assert_eq!(app.state.activity.lock().online(metagame::time::now_ms()).len(), 2);
    for request in [req(), key(&friend.key), token(&friend.token)] {
        let again = dauntless_status(&app, request).await;
        assert_eq!(keys(&again), fields.iter().map(String::as_str).collect::<Vec<_>>());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn status_extra_0_answers_the_nine_fields_alone() {
    let app = friends_app(None, &[("STATUS_EXTRA", "0")]).await;
    let reply = dauntless_status(&app, req()).await;
    assert_eq!(
        keys(&reply),
        ["show-status", "en", "fr", "it", "es", "de", "pt", "ru", "ja"]
    );
    assert_eq!(reply["en"], "Welcome to Dauntless Revived!");
}

fn texts(reply: &Value) -> Vec<String> {
    LANGUAGES
        .iter()
        .map(|l| reply[l].as_str().unwrap_or_else(|| panic!("{l}")).to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn welcomes_the_player_to_the_server_by_name_in_each_of_the_client_s_languages() {
    let app = friends_app(None, &[]).await;
    let default = dauntless_status(&app, req()).await;
    assert_eq!(
        texts(&default),
        [
            "Welcome to Dauntless Revived!",
            "Bienvenue sur Dauntless Revived !",
            "Ti diamo il benvenuto su Dauntless Revived!",
            "¡Te damos la bienvenida a Dauntless Revived!",
            "Willkommen bei Dauntless Revived!",
            "Boas-vindas ao Dauntless Revived!",
            "Добро пожаловать в Dauntless Revived!",
            "Dauntless Revivedへようこそ！",
        ]
    );
    assert_eq!(default["name"], "Dauntless Revived");
    // Every language has its own text, none of them carries a version number
    let all = texts(&default);
    assert_eq!(all.iter().collect::<std::collections::HashSet<_>>().len(), all.len());
    let version = regex::Regex::new(r"(?i)undaunted|\d+\.\d+").unwrap();
    assert!(all.iter().all(|t| !version.is_match(t)));

    let named_app = friends_app(None, &[("SERVER_NAME", "Friday Hunts")]).await;
    let named = dauntless_status(&named_app, req()).await;
    assert_eq!(
        json!([named["en"], named["de"], named["ja"], named["name"]]),
        json!([
            "Welcome to Friday Hunts!",
            "Willkommen bei Friday Hunts!",
            "Friday Huntsへようこそ！",
            "Friday Hunts"
        ])
    );
    for text in texts(&named) {
        assert!(
            text.contains("Friday Hunts") && !text.contains("Dauntless Revived"),
            "{text}"
        );
    }
    // Only printable ASCII of SERVER_NAME is used, as for the ServerStatus name
    let odd = friends_app(None, &[("SERVER_NAME", "  \u{0007}Night\u{200b}Shift  ")]).await;
    assert_eq!(dauntless_status(&odd, req()).await["en"], "Welcome to NightShift!");
}

#[tokio::test(flavor = "multi_thread")]
async fn tells_the_player_the_store_is_not_available_in_plain_english() {
    // With every store off; Lady Luck and the Middleman are on by default
    let app = friends_app(None, &[("TRIALS_STORE", "off"), ("MIDDLEMAN_STORE", "off")]).await;
    let friend = friend(&app).await;
    let reply = app.get("/product/skus/public", token(&friend.token)).await;
    assert_eq!(
        (reply.status, reply.json),
        (
            400,
            json!({"code": "400", "message": "The store is not available on Dauntless Revived yet."})
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn never_say_undaunted() {
    let (app, _deploy, friend) = status_app().await;
    let calls = [
        ("/dauntless-status", req()),
        ("/dauntless-status", token(&friend.token)),
        ("/product/skus/public", token(&friend.token)),
        ("/undaunted/api/ServerStatus", req()),
        ("/undaunted/api/ServerStatus", key(&friend.key)),
        ("/undaunted/api/GetUserInfo", key(&friend.key)),
        ("/undaunted/api/UsernameAvailable?Username=Someone_New", req()),
        ("/accountinfo", token(&friend.token)),
        ("/character", token(&friend.token)),
    ];
    let routes = regex::Regex::new(r"(?i)/undaunted/api/").unwrap();
    let product = regex::Regex::new(r"(?i)undaunted").unwrap();
    for (path, request) in calls {
        let reply = app.get(path, request).await;
        assert!(reply.status < 500, "{path}: {}", reply.status);
        assert!(!product.is_match(&routes.replace_all(&reply.text, "/")), "{path}");
    }
}
