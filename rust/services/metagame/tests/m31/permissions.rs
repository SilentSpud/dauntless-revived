//! `permissions.test.ts`: the permission audit. A player's token only reaches its own account and
//! characters, game-server-only writes refuse player tokens, admin routes need the admin key, and in
//! public mode neither the game-server key nor the admin key works through the gateway or any proxy.
//! Node's request-origin helpers are `metagame::origin::Origin`, and its gateway check at start
//! `Config::gateway_checks`. Switches are read at start, so a case that changes one uses another app.

use std::net::SocketAddr;

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use metagame::origin::{Origin, OriginConfig, OwnAddresses};
use serde_json::{Value, json};
use service_kit::serve::ConnectionInfo;

use crate::common::{Reply, Req, TestApp, req, sha256_hex};
use crate::deploykit::RecordingDeploy;

const A: &str = "UID-real-a";
const B: &str = "UID-real-b";
const C: &str = "UID-stub-c";
const ADMIN: &str = "UID-admin";
const SECRET: &str = "test-gateway-secret-0123456789abcdef";
const TUTORIAL_ARGS: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";

struct Perm {
    app: TestApp,
    deploy: RecordingDeploy,
    characters: std::collections::HashMap<&'static str, String>,
}

fn key_of(user: &str) -> String {
    format!("UUK_{}", &sha256_hex(&format!("permissions {user}"))[..48])
}

fn name_of(user: &str) -> String {
    user.replace('-', "_").chars().take(16).collect()
}

/// `permissionsenv.ts`'s settings (A and B real, C stub, public mode), the four accounts with keys and
/// characters, and A's data written the way the game server writes it.
async fn perm(extra: &[(&str, &str)]) -> Perm {
    let deploy = RecordingDeploy::start().await;
    let mut env = vec![
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", deploy.address.as_str()),
        ("REGISTRATION_MODE", "OPEN"),
        ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
        ("PROGRESSION_MODE", "stub"),
        ("PROGRESSION_REAL_ACCOUNTS", "UID-real-a,UID-real-b"),
        ("GATEWAY_SECRET", SECRET),
    ];
    env.extend_from_slice(extra);
    let app = TestApp::with_env(&env).await;
    let mut characters = std::collections::HashMap::new();
    for (user, admin) in [(A, false), (B, false), (C, false), (ADMIN, true)] {
        let shown = name_of(user);
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, ?3)",
            &[&user, &shown, &i64::from(admin)],
        );
        app.run(
            "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
            &[&user, &sha256_hex(&key_of(user))],
        );
        let character = app
            .db(move |conn| metagame::characters::create(conn, user, user))
            .await
            .characterId;
        characters.insert(user, character);
    }
    let grant = json!({"progress_tracks": [{"progression_id": "season09b", "progress": 150}], "objectives": [{"objective_id": "OBJ_X", "value": 3, "completed_count": 0}]});
    assert_eq!(
        app.post(&format!("/progression/{A}"), req().gs().bearer(A).json(grant))
            .await
            .status,
        200
    );
    let cooldowns = json!({"cooldowns": [{"cooldown_id": "bounty_tokens_season09b", "cooldown_started_date": "2026-09-21T14:10:39.154Z"}]});
    assert_eq!(
        app.call(
            "PUT",
            &format!("/cooldown/batch/{A}"),
            req().gs().bearer(A).json(cooldowns)
        )
        .await
        .status,
        200
    );
    let bounties = json!({"bounties": [{"bounty_id": "Bounty_Bronze_KillRadiant", "premium_bounty": false, "slot_index": 0,
        "objectives": [{"objective_id": "o", "progress": 1}], "drafted_timestamp": "2026-09-21T14:31:05.123Z", "update_version": 1}]});
    assert_eq!(
        app.post(&format!("/bounty/{A}"), req().gs().bearer(A).json(bounties))
            .await
            .status,
        200
    );
    Perm {
        app,
        deploy,
        characters,
    }
}

fn key(user: &str) -> Req {
    req().header("x-undaunted-user-api-key", &key_of(user))
}

fn via_gateway(request: Req) -> Req {
    request
        .header("x-forwarded-for", "203.0.113.9")
        .header("x-dauntless-gateway", SECRET)
}

fn with_headers(mut request: Req, headers: &[(&str, &str)]) -> Req {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
}

fn with_body(request: Req, body: &Option<Value>, method: &str) -> Req {
    match body {
        Some(body) => request.json(body.clone()),
        None if method == "POST" || method == "PUT" || method == "DELETE" => request.raw(""),
        None => request,
    }
}

impl Perm {
    async fn call(&self, method: &str, path: &str, request: Req) -> Reply {
        self.app.call(method, path, request).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn device_reports_require_an_account_key_reject_supplied_identities_and_limit_requests() {
    let p = perm(&[("DEVICE_SIGNAL_HMAC_KEY", &"cd".repeat(32))]).await;
    let report = |request: Req, body: Value| {
        p.app
            .post_owned("/undaunted/api/DeviceSignals".into(), request.json(body))
    };
    assert_eq!(report(req(), json!({})).await.status, 401);
    assert_eq!(report(key(A), json!({"accountId": B})).await.status, 400);
    assert_eq!(
        report(key(A), json!({"smbiosUuid": "12345678-1234-5678-9abc-123456789abc"}))
            .await
            .status,
        204
    );
    assert_eq!(p.app.count("account_device_signals", "account_id = ?1", &[&B]), 0);
    for _ in 0..8 {
        assert_eq!(report(key(A), json!({})).await.status, 204);
    }
    assert_eq!(report(key(A), json!({})).await.status, 429);
}

#[tokio::test(flavor = "multi_thread")]
async fn hunt_region_changes_require_the_account_key_and_cannot_target_another_account() {
    let p = perm(&[("US_REGION", "1")]).await;
    let set = |request: Req, body: Value| p.app.post_owned("/undaunted/api/HuntRegion".into(), request.json(body));
    assert_eq!(set(req(), json!({"region": "us"})).await.status, 401);
    assert_eq!(set(key(A), json!({"region": "remote"})).await.status, 400);
    assert_eq!(set(key(A), json!({"region": "us", "userId": B})).await.status, 200);
    assert_eq!(
        p.app.get("/undaunted/api/HuntRegion", key(A)).await.json["region"],
        "us"
    );
    assert_eq!(
        p.app.get("/undaunted/api/HuntRegion", key(B)).await.json["region"],
        "main"
    );
    assert_eq!(set(key(A), json!({"region": "main"})).await.status, 200);
    // Without US_REGION (read at start): another app on the same database
    let db = p.app.db.to_string_lossy().into_owned();
    let off = TestApp::with_env(&[("DB_FILENAME", db.as_str())]).await;
    assert_eq!(
        off.post("/undaunted/api/HuntRegion", key(A).json(json!({"region": "us"})))
            .await
            .status,
        503
    );
    assert_eq!(
        off.post("/undaunted/api/HuntRegion", key(A).json(json!({"region": "aus"})))
            .await
            .status,
        410
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dashboard_account_directory_exposes_names_and_short_fingerprints_never_login_keys_or_full_hashes() {
    let p = perm(&[]).await;
    let directory = |query: &str| {
        p.app
            .get_owned(format!("/undaunted/api/DashboardAccounts?{query}"), key(ADMIN))
    };
    let reply = directory("offset=0").await;
    assert_eq!(reply.status, 200);
    let account = reply.json["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == A)
        .unwrap()
        .clone();
    assert_eq!(account["name"], name_of(A));
    assert_eq!(account["keyFingerprint"], &sha256_hex(&key_of(A))[..16]);
    assert_eq!(account["admin"], false);
    assert!(!reply.text.contains(&key_of(A)) && !reply.text.contains(&sha256_hex(&key_of(A))));
    assert_eq!(directory("offset=-1").await.status, 400);
    for i in 0..105 {
        let (id, shown) = (format!("UID-zz-search-{i:03}"), format!("DirectorySearch{i}"));
        p.app.run(
            "INSERT INTO users (userId, name, notes) VALUES (?1, ?2, 0)",
            &[&id, &shown],
        );
    }
    let names = |reply: Reply| {
        reply.json["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(directory("offset=0&q=DirectorySearch104").await),
        [json!("DirectorySearch104")]
    );
    p.app.exec("DELETE FROM users WHERE userId LIKE 'UID-zz-search-%'");
    let search = directory(&format!("offset=0&q={A}")).await;
    assert_eq!(
        search.json["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].clone())
            .collect::<Vec<_>>(),
        [json!(A)]
    );
    assert_eq!(
        directory("offset=0&q=%25").await.json["accounts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        directory("offset=99999").await.json,
        json!({"accounts": [], "nextOffset": null})
    );
}

fn real_reads() -> Vec<String> {
    vec![
        format!("/progression/{A}"),
        format!("/progression/objectives/{A}"),
        format!("/progression/objectives/{A}/OBJ_X"),
        format!("/progression/{A}/season09b"),
        format!("/huntpass/{A}"),
        format!("/cooldown/{A}"),
        format!("/bounty/{A}"),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn answer_the_account_s_own_client_and_any_game_server_and_refuse_every_other_player_403_no_body() {
    let p = perm(&[]).await;
    let own = p.app.get(&format!("/progression/{A}"), req().bearer(A)).await;
    let season = own.json["payload"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["progression_id"] == "season09b")
        .unwrap()
        .clone();
    assert_eq!(season["progress"], 150, "the test data is there");
    for path in real_reads() {
        let mine = p.app.get(&path, req().bearer(A)).await;
        assert_eq!(mine.status, 200, "own client: {path}");
        for other in [B, C] {
            let foreign = p.app.get(&path, req().bearer(other)).await;
            assert_eq!(
                (foreign.status, foreign.text.as_str()),
                (403, ""),
                "{other}'s client: {path}"
            );
        }
        let server = p.app.get(&path, req().gs().bearer(B)).await;
        assert_eq!(
            (server.status, &server.text),
            (200, &mine.text),
            "game server carrying another player's bearer: {path}"
        );
        let server_only = p.app.get(&path, req().gs()).await;
        assert_eq!(
            (server_only.status, &server_only.text),
            (200, &mine.text),
            "game server without a bearer: {path}"
        );
        assert_eq!(p.app.get(&path, req()).await.status, 401, "no auth: {path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stub_mode_replies_are_the_caller_s_own_stub_never_another_account_s_stored_data() {
    let p = perm(&[]).await;
    assert_eq!(
        p.app.get(&format!("/progression/{A}"), req().bearer(C)).await.status,
        403,
        "A is in real mode: C's token is refused"
    );
    let for_c = p.app.get(&format!("/progression/{C}"), req().bearer(A)).await;
    assert_eq!(for_c.status, 200);
    assert!(
        for_c.json["payload"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["phx_account_id"] == A && t["progress"] == 99_999_999),
        "the stub for the token's own account"
    );
    let entitlements = p.app.get("/entitlementsv2", req().bearer(B)).await;
    assert_eq!(entitlements.status, 200);
    assert!(
        !entitlements.text.contains(A),
        "entitlements come from the token's account"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuse_another_player_s_character_whatever_the_url_or_body_names() {
    let p = perm(&[]).await;
    let cid = p.characters[A].clone();
    let attempts: Vec<(&str, String, Option<Value>, u16)> = vec![
        ("GET", format!("/breadcrumbs/{cid}"), None, 403),
        (
            "POST",
            format!("/breadcrumbs/{cid}"),
            Some(json!({"breadcrumbs": ["x"], "updateVersion": 1})),
            403,
        ),
        ("GET", format!("/encountered-content/{cid}/1"), None, 403),
        (
            "POST",
            format!("/encountered-content/query/{cid}"),
            Some(json!({"content_types": [1]})),
            403,
        ),
        (
            "POST",
            format!("/encountered-content/{cid}"),
            Some(json!({"content_type": 1, "content_id": "x"})),
            403,
        ),
        ("GET", format!("/inventory/{A}/{cid}"), None, 403),
        (
            "POST",
            "/inventory".into(),
            Some(
                json!({"accountId": A, "characterId": cid, "transactionId": "t-foreign", "addStackedItems": [{"catalogId": "x", "quantity": 1}]}),
            ),
            403,
        ),
        (
            "POST",
            "/inventory/instanceditem".into(),
            Some(
                json!({"accountId": A, "characterId": cid, "instanceId": "i", "catalogId": "c", "itemData": "{}", "updateVersion": 1}),
            ),
            403,
        ),
        (
            "POST",
            "/character".into(),
            Some(json!({"characterId": cid, "data": "{\"x\":1}", "updateVersion": 99})),
            404,
        ),
        ("GET", format!("/loadout/{A}/{cid}/all"), None, 404),
        (
            "POST",
            format!("/loadout/{A}/{cid}/0"),
            Some(json!({"data": "{}"})),
            404,
        ),
        ("GET", format!("/loadout/{A}/{cid}/slotcount"), None, 404),
        ("POST", format!("/loadout/{A}/{cid}/unlock/1"), None, 404),
        ("POST", format!("/loadout/{A}/{cid}/active/0"), None, 404),
    ];
    for (method, path, body, status) in attempts {
        let reply = p.call(method, &path, with_body(req().bearer(B), &body, method)).await;
        assert_eq!(reply.status, status, "{method} {path} with B's token");
    }
    let character = p.app.row(
        "SELECT updateVersion, data FROM characters WHERE characterId = ?1",
        &[&cid],
    );
    assert_eq!(
        json!([character["updateVersion"], character["data"]]),
        json!([0, "{}"]),
        "A's character untouched"
    );
    for table in ["breadcrumbs", "encounteredcontent", "inventories"] {
        assert_eq!(
            p.app.count(table, "characterId = ?1", &[&cid]),
            0,
            "nothing in {table} for A's character"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn refuse_player_tokens_the_account_s_own_included_and_change_nothing() {
    let p = perm(&[]).await;
    let before = p.app.get(&format!("/progression/{A}"), req().gs()).await.text;
    let cid = p.characters[A].clone();
    // The account's own token always gets 403; loadout slots resolve the character through the token's
    // account first, so another player gets 404
    let writes: Vec<(&str, String, Option<Value>, u16)> = vec![
        (
            "POST",
            format!("/progression/{A}"),
            Some(json!({"progress_tracks": [{"progression_id": "season09b", "progress": 5000}], "objectives": []})),
            403,
        ),
        ("POST", format!("/progression/{A}/season09b/100"), None, 403),
        (
            "POST",
            format!("/progression/{A}/season09b/1/confirm/public"),
            None,
            403,
        ),
        (
            "POST",
            format!("/entitlementv2/{A}"),
            Some(json!({"entitlement": "season09b_premium", "duration": 0})),
            403,
        ),
        ("DELETE", format!("/entitlement/{A}/season09b_premium"), None, 403),
        (
            "PUT",
            format!("/cooldown/batch/{A}"),
            Some(json!({"cooldowns": []})),
            403,
        ),
        ("PUT", format!("/cooldown/{A}/ABC"), None, 403),
        ("POST", format!("/bounty/{A}"), Some(json!({"bounties": []})), 403),
        (
            "POST",
            format!("/bounty/delete/{A}"),
            Some(json!({"bounty_ids": ["Bounty_Bronze_KillRadiant"]})),
            403,
        ),
        (
            "POST",
            format!("/huntpass/{A}"),
            Some(json!({"progression_id": "season09b"})),
            403,
        ),
        ("DELETE", format!("/progression/{A}/season09b"), None, 403),
        ("POST", format!("/loadout/{A}/{cid}/unlock/3"), None, 404),
        ("POST", format!("/loadout/{A}/{cid}/active/1"), None, 404),
    ];
    for caller in [A, B] {
        for (method, path, body, other_status) in &writes {
            let reply = p
                .call(method, path, with_body(req().bearer(caller), body, method))
                .await;
            let expected = if caller == A { 403 } else { *other_status };
            assert_eq!(reply.status, expected, "{method} {path} with {caller}'s token");
        }
    }
    assert_eq!(
        p.app.get(&format!("/progression/{A}"), req().gs()).await.text,
        before,
        "A's progression unchanged"
    );
}

fn admin_routes(p: &Perm) -> Vec<(&'static str, String, Option<Value>)> {
    let cid = p.characters[C].clone();
    vec![
        (
            "POST",
            "/undaunted/api/RegistrationStatus".into(),
            Some(json!({"RegistrationStatus": "OPEN"})),
        ),
        ("GET", "/undaunted/api/InviteCodes".into(), None),
        (
            "POST",
            "/undaunted/api/GenerateJWTForUserId".into(),
            Some(json!({"UserId": C})),
        ),
        ("GET", "/undaunted/api/GetAllUsers".into(), None),
        ("GET", "/undaunted/api/DiscordKeyStats".into(), None),
        (
            "POST",
            "/undaunted/api/DiscordLink".into(),
            Some(json!({"DiscordId": "12345678901234567", "UserId": A})),
        ),
        ("GET", "/undaunted/api/DiscordLink/12345678901234567".into(), None),
        ("GET", "/undaunted/api/BackendHealth".into(), None),
        ("GET", "/undaunted/api/DashboardAccounts".into(), None),
        ("GET", "/undaunted/api/AccountRecovery/UID-example".into(), None),
        ("GET", format!("/undaunted/api/Moderation/{C}"), None),
        ("GET", "/undaunted/api/ModerationLog?kind=events".into(), None),
        (
            "POST",
            "/undaunted/api/Moderation".into(),
            Some(json!({"accountId": C, "reason": "test", "active": false})),
        ),
        (
            "POST",
            "/undaunted/api/RegisterInviteCode".into(),
            Some(json!({"NewInviteCode": "PERM-TEST-CODE", "Uses": 1})),
        ),
        ("DELETE", "/undaunted/api/InviteCode/PERM-TEST-CODE".into(), None),
        ("POST", "/undaunted/api/CreateInvite".into(), Some(json!({}))),
        (
            "POST",
            "/undaunted/api/RenameUser".into(),
            Some(json!({"UserId": C, "NewUsername": "Renamed_c"})),
        ),
        ("GET", "/undaunted/api/PrivateOnlineStats".into(), None),
        ("GET", format!("/undaunted/api/SaveHistory?UserId={C}"), None),
        (
            "POST",
            "/undaunted/api/RollbackCharacter".into(),
            Some(json!({"CharacterId": cid, "Version": 0})),
        ),
        (
            "POST",
            "/undaunted/api/RollbackLoadout".into(),
            Some(json!({"CharacterId": cid, "Version": 0})),
        ),
        ("GET", format!("/undaunted/api/Progression?UserId={C}"), None),
        (
            "POST",
            "/undaunted/api/SeedProgression".into(),
            Some(json!({"UserId": C, "Mode": "fresh"})),
        ),
        (
            "POST",
            "/undaunted/api/GrantEntitlement".into(),
            Some(json!({"UserId": C, "Entitlement": "perm_test"})),
        ),
        (
            "POST",
            "/undaunted/api/RevokeEntitlement".into(),
            Some(json!({"UserId": C, "Entitlement": "perm_test"})),
        ),
        (
            "POST",
            "/undaunted/api/DisbandGuild".into(),
            Some(json!({"Guild": "NoSuchGuild"})),
        ),
        ("GET", "/undaunted/api/Guilds".into(), None),
        ("DELETE", format!("/progression/{B}/season09b"), None),
    ]
}

/// Node's fetch sends no body for a body-less call
fn plain_body(request: Req, body: &Option<Value>) -> Req {
    match body {
        Some(body) => request.json(body.clone()),
        None => request,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn answer_401_without_a_key_and_403_for_a_player_s_key() {
    let p = perm(&[]).await;
    for (method, path, body) in admin_routes(&p) {
        assert_eq!(
            p.call(method, &path, plain_body(req(), &body)).await.status,
            401,
            "no key: {method} {path}"
        );
        assert_eq!(
            p.call(method, &path, plain_body(key(C), &body)).await.status,
            403,
            "player key: {method} {path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn refuse_the_admin_key_through_the_gateway_or_any_proxy_403_even_with_odd_path_spellings() {
    let p = perm(&[]).await;
    let proxies: [&[(&str, &str)]; 5] = [
        &[("x-forwarded-for", "203.0.113.9"), ("x-dauntless-gateway", SECRET)],
        &[("x-forwarded-for", "198.51.100.4")],
        &[("via", "1.1 someproxy")],
        &[("forwarded", "for=198.51.100.4")],
        &[("x-real-ip", "198.51.100.4")],
    ];
    for (method, path, body) in admin_routes(&p) {
        for headers in proxies {
            let reply = p
                .call(method, &path, with_headers(plain_body(key(ADMIN), &body), headers))
                .await;
            assert_eq!(
                (reply.status, reply.text.as_str()),
                (403, ""),
                "{method} {path} via {headers:?}"
            );
        }
    }
    // Express matched paths case-insensitively and with a trailing slash; the metagame still refuses these
    for path in [
        "/undaunted/api/createinvite",
        "/UNDAUNTED/API/CreateInvite/",
        "/undaunted/api/CreateInvite?x=1",
    ] {
        assert_eq!(
            p.call("POST", path, via_gateway(key(ADMIN).json(json!({}))))
                .await
                .status,
            403,
            "{path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn work_with_the_admin_key_on_a_direct_request() {
    let p = perm(&[]).await;
    for (method, path, body) in admin_routes(&p) {
        let reply = p.call(method, &path, plain_body(key(ADMIN), &body)).await;
        assert!(
            reply.status != 401 && reply.status != 403,
            "{method} {path}: {}",
            reply.status
        );
    }
    assert_eq!(
        p.call("POST", "/undaunted/api/createinvite", key(ADMIN).json(json!({})))
            .await
            .status,
        200,
        "lower-case path reaches the route"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn works_on_a_direct_loopback_request_and_is_refused_403_key_not_even_checked_with_any_forwarding_header() {
    let p = perm(&[]).await;
    let path = format!("/progression/{A}");
    let direct = p.app.get(&path, req().gs().bearer(A)).await;
    assert_eq!(direct.status, 200);
    let forwarding: [&[(&str, &str)]; 8] = [
        &[("x-forwarded-for", "203.0.113.9"), ("x-dauntless-gateway", SECRET)],
        &[("x-forwarded-for", "127.0.0.1")],
        &[("x-dauntless-gateway", "wrong")],
        &[("via", "1.1 relay")],
        &[("forwarded", "for=127.0.0.1")],
        &[("x-real-ip", "127.0.0.1")],
        &[("x-forwarded-host", "example.org")],
        &[("x-forwarded-proto", "https")],
    ];
    for headers in forwarding {
        let relayed = p.app.get(&path, with_headers(req().gs().bearer(A), headers)).await;
        assert_eq!(
            (relayed.status, relayed.text.as_str()),
            (403, ""),
            "valid key via {headers:?}"
        );
        let grant = json!({"progress_tracks": [{"progression_id": "season09b", "progress": 9999}], "objectives": []});
        assert_eq!(
            p.app
                .post(&path, with_headers(req().gs().bearer(A), headers).json(grant))
                .await
                .status,
            403,
            "grant via {headers:?}"
        );
    }
    let wrong = || req().header("x-undaunted-gameserver-apikey", "not-the-key");
    assert_eq!(
        p.app.get(&path, via_gateway(wrong())).await.status,
        403,
        "a wrong key through the gateway: 403, same as a right one"
    );
    assert_eq!(
        p.app.get(&path, wrong()).await.status,
        401,
        "a wrong key directly: 401 as before"
    );
    assert_eq!(
        p.app.get(&path, req().gs()).await.text,
        direct.text,
        "nothing was granted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn leaves_player_traffic_through_the_gateway_as_it_was() {
    let p = perm(&[]).await;
    let direct = p.app.get("/character", req().bearer(A)).await;
    let relayed = p.app.get("/character", via_gateway(req().bearer(A))).await;
    assert_eq!((relayed.status, &relayed.text), (200, &direct.text));
    let beat = p
        .app
        .post(
            "/heartbeat",
            via_gateway(req().bearer(A)).json(json!({"map": "/Game/Maps/ramsgate/ramsgate_01_persistent"})),
        )
        .await;
    assert_eq!((beat.status, beat.text.as_str()), (200, "20000"));
    let info = p.app.get("/undaunted/api/GetUserInfo", via_gateway(key(A))).await;
    assert_eq!((info.status, info.json["UserId"].clone()), (200, json!(A)));
    let registered = p
        .app
        .post(
            "/undaunted/api/Register",
            via_gateway(req()).json(json!({"Username": "Via_Gateway"})),
        )
        .await;
    assert_eq!(registered.status, 200);
    let new_key = registered.json["UUK"].as_str().unwrap().to_owned();
    let login = p
        .app
        .post(
            "/account/api/oauth/token",
            via_gateway(req()).json(json!({"grant_type": "exchange_code", "exchange_code": new_key})),
        )
        .await;
    assert_eq!(login.status, 200);
    assert!(login.json["access_token"].is_string());
    // ServerStatus: the player list only for a registered player's key (an admin's too: it is not an
    // admin route); without one, or with a wrong one, the limited answer, never a 401
    for request in [
        via_gateway(req()),
        via_gateway(req().header("x-undaunted-user-api-key", "UUK_not_a_real_key_00000000")),
    ] {
        let anonymous = p.app.get("/undaunted/api/ServerStatus", request).await;
        let j = &anonymous.json;
        assert_eq!(
            json!([anonymous.status, j["limited"], j["playersOnline"], j["players"]]),
            json!([200, true, 0, []])
        );
    }
    for k in [key_of(A), new_key.clone(), key_of(ADMIN)] {
        let full = p
            .app
            .get(
                "/undaunted/api/ServerStatus",
                via_gateway(req().header("x-undaunted-user-api-key", &k)),
            )
            .await;
        assert_eq!((full.status, full.json["limited"].clone()), (200, json!(false)));
        assert!(
            full.json["players"]
                .as_array()
                .unwrap()
                .iter()
                .any(|pl| pl["name"] == "UID_real_a"),
            "A's heartbeat is listed"
        );
    }
    assert_eq!(p.app.get("/QoS", via_gateway(req())).await.status, 200);
    let regions = p.app.get("/candidate/regions", via_gateway(req().bearer(A))).await;
    assert_eq!(regions.json["payload"]["regionUrls"][0], "http://127.0.0.1:61000/QoS");
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_mode_none_development_never_answers_a_proxied_request() {
    let dev = perm(&[("AUTH_MODE", "NONE"), ("NODE_ENV", "development")]).await;
    let relayed = dev
        .app
        .post(
            "/account/api/oauth/token",
            via_gateway(req()).json(json!({"exchange_code": ADMIN})),
        )
        .await;
    assert_eq!((relayed.status, relayed.text.as_str()), (403, ""));
    assert_eq!(
        dev.app
            .get("/dauntless-status", req().header("x-forwarded-for", "198.51.100.4"))
            .await
            .status,
        403
    );
    let direct = dev
        .app
        .post("/account/api/oauth/token", req().json(json!({"exchange_code": ADMIN})))
        .await;
    assert_eq!(
        (direct.status, direct.json["account_id"].clone()),
        (200, json!(ADMIN)),
        "direct development logins as before"
    );
    let p = perm(&[]).await;
    assert_eq!(
        p.app.get("/dauntless-status", via_gateway(req())).await.status,
        200,
        "APIKEY mode serves proxied requests"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_player_s_token_the_leader_s_own_included_403_and_the_game_server_key_through_the_gateway_or_any_proxy_403()
 {
    let p = perm(&[]).await;
    let body = json!({"leader_account_id": A, "name": "PermGuild", "nameplate": "PRM"});
    for caller in [A, B] {
        let reply = p.app.post("/guild", req().bearer(caller).json(body.clone())).await;
        assert_eq!(reply.status, 403, "{caller}'s token");
        assert_eq!(
            reply.json["code"], "",
            "the envelope, so the client shows its generic failure"
        );
    }
    let proxies: [&[(&str, &str)]; 3] = [
        &[("x-forwarded-for", "203.0.113.9"), ("x-dauntless-gateway", SECRET)],
        &[("x-forwarded-for", "127.0.0.1")],
        &[("via", "1.1 relay")],
    ];
    for headers in proxies {
        assert_eq!(
            p.app
                .post("/guild", with_headers(req().gs().bearer(A), headers).json(body.clone()))
                .await
                .status,
            403,
            "{headers:?}"
        );
    }
    let unknown = req().header("x-undaunted-gameserver-apikey", "not-a-registered-key");
    assert_eq!(p.app.post("/guild", unknown.json(body)).await.status, 401);
    assert_eq!(
        p.app.get("/guild", req().bearer(A)).await.status,
        204,
        "no guild was created"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_client_s_guild_routes_refuse_the_game_server_key_alone_403_and_no_auth_401() {
    let p = perm(&[]).await;
    let routes = [
        ("POST", "/guild/validate".to_owned()),
        ("PUT", format!("/guild/invite/{B}")),
        ("DELETE", "/guild/player".to_owned()),
        ("DELETE", format!("/guild/player/{B}")),
        ("POST", "/guild/invite/accept/x".to_owned()),
        ("DELETE", "/guild/invite/x".to_owned()),
        ("PUT", format!("/guild/rank/{B}/officer")),
        ("DELETE", "/guild/x".to_owned()),
    ];
    for (method, path) in routes {
        assert_eq!(
            p.call(method, &path, req().gs().raw("")).await.status,
            403,
            "key alone: {method} {path}"
        );
        assert_eq!(
            p.call(method, &path, req().raw("")).await.status,
            401,
            "no auth: {method} {path}"
        );
    }
}

fn headers_of(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.append(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

fn origin<'a>(peer: &str, local: &str, headers: &'a HeaderMap) -> Origin<'a> {
    let address = |ip: &str| ip.parse::<std::net::IpAddr>().map(|ip| SocketAddr::new(ip, 1)).unwrap();
    Origin {
        connection: ConnectionInfo {
            peer: address(peer),
            local: address(local),
        },
        headers,
    }
}

fn gateway(secret: Option<&str>, allow: &[&str]) -> OriginConfig {
    OriginConfig {
        gateway_secret: secret.map(str::to_owned),
        gameserver_allow_from: allow.iter().map(|a| a.parse().unwrap()).collect(),
    }
}

#[test]
fn keeps_ipv4_and_ipv6_addresses_out_of_persisted_request_traces() {
    let config = gateway(Some(SECRET), &[]);
    for address in ["203.0.113.7", "2001:db8::7"] {
        let relayed = headers_of(&[("x-dauntless-gateway", SECRET), ("x-forwarded-for", address)]);
        assert_eq!(
            origin("127.0.0.1", "127.0.0.1", &relayed).describe(&config),
            " via=gateway"
        );
        let proxied = headers_of(&[("via", "proxy")]);
        assert_eq!(origin(address, "127.0.0.1", &proxied).describe(&config), " via=proxy");
        let none = HeaderMap::new();
        assert_eq!(origin(address, "127.0.0.1", &none).describe(&config), "");
    }
}

#[test]
fn takes_x_forwarded_for_only_from_loopback_with_the_right_gateway_secret() {
    let config = gateway(Some(SECRET), &[]);
    let address = |peer: &str, local: &str, pairs: &[(&str, &str)]| {
        let headers = headers_of(pairs);
        origin(peer, local, &headers).client_address(&config).to_string()
    };
    let right = ("x-dauntless-gateway", SECRET);
    assert_eq!(
        address("127.0.0.1", "127.0.0.1", &[right, ("x-forwarded-for", "203.0.113.7")]),
        "203.0.113.7"
    );
    assert_eq!(
        address(
            "::ffff:127.0.0.1",
            "::ffff:127.0.0.1",
            &[right, ("x-forwarded-for", "::ffff:203.0.113.7")]
        ),
        "203.0.113.7"
    );
    assert_eq!(
        address("127.0.0.1", "127.0.0.1", &[right, ("x-forwarded-for", "2001:db8::7")]),
        "2001:db8::7"
    );
    assert_eq!(
        address(
            "127.0.0.1",
            "127.0.0.1",
            &[right, ("x-forwarded-for", "1.1.1.1, 203.0.113.7")]
        ),
        "203.0.113.7",
        "the right-most entry, written by the gateway"
    );
    assert_eq!(
        address("127.0.0.1", "127.0.0.1", &[right, ("x-forwarded-for", "not-an-ip")]),
        "127.0.0.1"
    );
    assert_eq!(address("127.0.0.1", "127.0.0.1", &[right]), "127.0.0.1");
    assert_eq!(
        address(
            "127.0.0.1",
            "127.0.0.1",
            &[("x-dauntless-gateway", "wrong"), ("x-forwarded-for", "203.0.113.7")]
        ),
        "127.0.0.1"
    );
    assert_eq!(
        address("127.0.0.1", "127.0.0.1", &[("x-forwarded-for", "203.0.113.7")]),
        "127.0.0.1"
    );
    assert_eq!(
        address("198.51.100.4", "10.0.0.2", &[right, ("x-forwarded-for", "203.0.113.7")]),
        "198.51.100.4",
        "the secret from a remote peer is not trusted"
    );
    let trusted = headers_of(&[right]);
    assert!(origin("127.0.0.1", "127.0.0.1", &trusted).is_trusted_gateway(&config));
    let unset = gateway(None, &[]);
    assert!(
        !origin("127.0.0.1", "127.0.0.1", &trusted).is_trusted_gateway(&unset),
        "GATEWAY_SECRET unset: no gateway"
    );
    let forwarded = headers_of(&[right, ("x-forwarded-for", "203.0.113.7")]);
    assert_eq!(
        origin("127.0.0.1", "127.0.0.1", &forwarded)
            .client_address(&unset)
            .to_string(),
        "127.0.0.1"
    );
    // An empty GATEWAY_SECRET reads as unset (Env::get), so it trusts nothing either
    let empty = headers_of(&[("x-dauntless-gateway", "")]);
    assert!(
        !origin("127.0.0.1", "127.0.0.1", &empty).is_trusted_gateway(&unset),
        "an empty secret trusts nothing"
    );
}

#[test]
fn counts_as_direct_and_local_only_callers_on_this_machine_that_no_proxy_relayed() {
    let (config, own) = (gateway(Some(SECRET), &[]), OwnAddresses::default());
    let local = |peer: &str, local: &str, pairs: &[(&str, &str)], config: &OriginConfig| {
        let headers = headers_of(pairs);
        origin(peer, local, &headers).is_direct_local(config, &own)
    };
    assert!(local("127.0.0.1", "127.0.0.1", &[], &config));
    assert!(local("::1", "::1", &[], &config));
    assert!(local("::ffff:127.0.0.1", "::ffff:127.0.0.1", &[], &config));
    assert!(
        local("100.64.1.2", "100.64.1.2", &[], &config),
        "a game server calling this machine's Tailscale address (private mode)"
    );
    assert!(
        !local("100.64.1.3", "100.64.1.2", &[], &config),
        "another tailnet machine"
    );
    assert!(!local("203.0.113.5", "127.0.0.1", &[], &config));
    // Node's missing socket addresses (undefined) cannot occur: every Rust connection has both
    assert!(!local(
        "127.0.0.1",
        "127.0.0.1",
        &[("x-forwarded-for", "127.0.0.1")],
        &config
    ));
    assert!(!local(
        "127.0.0.1",
        "127.0.0.1",
        &[("x-dauntless-gateway", SECRET)],
        &config
    ));
    let allowed = gateway(Some(SECRET), &["172.20.0.5"]);
    assert!(
        local("172.20.0.5", "172.20.0.1", &[], &allowed),
        "listed in GAMESERVER_ALLOW_FROM"
    );
    assert!(!local("172.20.0.6", "172.20.0.1", &[], &allowed));
    assert!(
        !local("172.20.0.5", "172.20.0.1", &[("via", "1.1 x")], &allowed),
        "still never through a proxy"
    );
    let none = HeaderMap::new();
    assert!(!origin("127.0.0.1", "127.0.0.1", &none).is_proxied());
    let forwarded = headers_of(&[("forwarded", "for=1.2.3.4")]);
    assert!(origin("127.0.0.1", "127.0.0.1", &forwarded).is_proxied());
}

#[test]
fn checks_the_public_mode_configuration_at_startup() {
    let checks = |pairs: &[(&str, &str)]| {
        let mut all = vec![
            ("AUTH_SIGNING_PRIVKEY_B64", "x"),
            ("AUTH_SIGNING_PUBKEY_B64", "y"),
            ("DB_FILENAME", "undaunted.db"),
            ("PORT", "1"),
        ];
        all.extend_from_slice(pairs);
        let config = metagame::config::Config::from_env(&service_kit::env::Env::from_pairs(all)).unwrap();
        let checks = config.gateway_checks();
        (checks.errors.len(), checks.warnings.len())
    };
    let good: Vec<(&str, &str)> = vec![
        ("GATEWAY_SECRET", SECRET),
        ("AUTH_MODE", "APIKEY"),
        ("BIND_HOST", "127.0.0.1"),
        ("NODE_ENV", "production"),
        ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
    ];
    let with = |name: &str, value: Option<&str>| -> Vec<(&str, &str)> {
        let mut pairs: Vec<(&str, &str)> = good.iter().copied().filter(|(k, _)| *k != name).collect();
        if let Some(value) = value {
            pairs.push((
                Box::leak(name.to_owned().into_boxed_str()),
                Box::leak(value.to_owned().into_boxed_str()),
            ));
        }
        pairs
    };
    assert_eq!(
        checks(&[("AUTH_MODE", "APIKEY")]),
        (0, 0),
        "no secret, no gateway, nothing to say"
    );
    assert_eq!(checks(&with("GATEWAY_SECRET", Some(""))), (0, 0));
    assert_eq!(checks(&good), (0, 0));
    assert_eq!(checks(&with("BIND_HOST", None)).1, 0, "BIND_HOST defaults to loopback");
    assert_eq!(checks(&with("AUTH_MODE", Some("NONE"))).0, 1);
    // An unset AUTH_MODE: the Rust config refuses it before the gateway check (Node's check errored)
    let unset: Vec<(&str, &str)> = [
        ("AUTH_SIGNING_PRIVKEY_B64", "x"),
        ("AUTH_SIGNING_PUBKEY_B64", "y"),
        ("DB_FILENAME", "undaunted.db"),
        ("PORT", "1"),
    ]
    .into_iter()
    .chain(with("AUTH_MODE", None))
    .collect();
    assert!(metagame::config::Config::from_env(&service_kit::env::Env::from_pairs(unset)).is_err());
    assert_eq!(checks(&with("GATEWAY_SECRET", Some("short"))).1, 1);
    assert_eq!(checks(&with("BIND_HOST", Some("0.0.0.0"))).1, 1);
    assert_eq!(checks(&with("NODE_ENV", Some("development"))).1, 1);
    assert_eq!(
        checks(&with("QOS_TARGET_URL", Some("http://203.0.113.1:61000/QoS"))).1,
        1
    );
}

#[test]
fn accepts_what_the_client_sends_and_nothing_shaped_otherwise() {
    let check = |mode: &str, args: Value, hunt: Option<Value>| {
        metagame::matchmaking::check_input(Some(&json!(mode)), Some(&args), hunt.as_ref())
    };
    assert_eq!(check("ISLAND", json!(TUTORIAL_ARGS), None), None);
    assert_eq!(
        check("CITY", json!(""), Some(json!("ShatteredIsles_ReturnToRamsgate"))),
        None
    );
    assert_eq!(
        check("SHARED", json!(""), Some(json!("ShatteredIsles_TrainingDojo"))),
        None
    );
    assert_eq!(
        check(
            "ISLAND",
            json!(""),
            Some(json!("CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr"))
        ),
        None
    );
    assert_eq!(
        check("ISLAND", json!(""), Some(json!("CR19_PlayerHunt_Patrol_Heroic+_Gem"))),
        None
    );
    assert_eq!(check("ISLAND", json!("   "), Some(json!(""))), None);
    assert_eq!(
        check("ISLAND", json!("/Game/Maps/islands/x?MaxPlayers=1?MonsterClass="), None),
        None,
        "an empty behemoth is an empty argument"
    );
    assert_eq!(
        check(
            "CITY",
            json!("whatever the city ignores"),
            Some(json!("ShatteredIsles_ReturnToRamsgate"))
        ),
        None,
        "game args are only used for ISLAND"
    );
    let bad: [(Value, Option<Value>); 14] = [
        (
            json!("/Game/Maps/islands/x -ExecCmds=quit?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"),
            None,
        ),
        (
            json!("/Game/Maps/islands/x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C -log"),
            None,
        ),
        (
            json!("/Game/Maps/islands/x?MaxPlayers=1?MonsterClass=\"/Game/M/b.b_C"),
            None,
        ),
        (json!("/Game/Maps/islands/x"), None),
        (json!("/Game/Maps/islands/x?MaxPlayers=1"), None),
        (json!("C:/Windows/x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"), None),
        (json!("/Game/../x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"), None),
        (json!("/Game/Maps/x?MaxPlayers=1?MonsterClass=-ExecCmds"), None),
        (json!({"a": 1}), None),
        (json!(""), Some(json!("CR19_PlayerHunt_Arena x"))),
        (json!(""), Some(json!("CR19_PlayerHunt_Arena:x,UID-y:z"))),
        (json!(""), Some(json!(5))),
        (json!(""), Some(json!(["CR19"]))),
        (json!(""), Some(json!("x".repeat(129)))),
    ];
    for (args, hunt) in bad {
        assert!(check("ISLAND", args.clone(), hunt.clone()).is_some(), "{args} {hunt:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_bad_input_at_candidate_join_400_before_the_deploy_server_hears_of_it() {
    let p = perm(&[]).await;
    let before = p.deploy.calls().len();
    for body in [
        json!({"gameMode": "ISLAND", "gameArgs": "/Game/Maps/islands/x -ExecCmds=quit?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"}),
        json!({"gameMode": "ISLAND", "gameArgs": "/Game/Maps/islands/x"}),
        json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": "CR19_PlayerHunt_Arena -ExecCmds=quit"}),
        json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": 5}),
        json!({"gameMode": "CITY", "gameArgs": "", "playerHuntId": "Ramsgate\""}),
    ] {
        assert_eq!(
            p.app
                .post("/candidate/join", req().bearer(A).json(body.clone()))
                .await
                .status,
            400,
            "{body}"
        );
    }
    assert_eq!(p.deploy.calls().len(), before, "nothing reached the deploy server");
}

#[tokio::test(flavor = "multi_thread")]
async fn passes_the_client_s_real_requests_through_unchanged() {
    let p = perm(&[]).await;
    let tutorial = p
        .app
        .post(
            "/candidate/join",
            req()
                .bearer(A)
                .json(json!({"gameMode": "ISLAND", "gameArgs": TUTORIAL_ARGS})),
        )
        .await;
    assert_eq!(tutorial.status, 200);
    assert_eq!(
        p.deploy.calls().last().unwrap(),
        &json!({"GameMode": "ISLAND", "GameArgs": TUTORIAL_ARGS})
    );
    let city = p
        .app
        .post(
            "/candidate/join",
            req()
                .bearer(A)
                .json(json!({"gameMode": "CITY", "gameArgs": "", "playerHuntId": "ShatteredIsles_ReturnToRamsgate"})),
        )
        .await;
    assert_eq!(city.status, 200);
    assert_eq!(
        p.deploy.calls().last().unwrap(),
        &json!({"GameMode": "CITY", "GameArgs": "", "HuntId": "ShatteredIsles_ReturnToRamsgate", "ExpectedPlayers": [A]})
    );
    let hunt = p
        .app
        .post(
            "/candidate/join",
            req().bearer(B).json(
                json!({"gameMode": "ISLAND", "gameArgs": "", "playerHuntId": "CR19_PlayerHunt_Patrol_Heroic+_Gem"}),
            ),
        )
        .await;
    assert_eq!(
        (hunt.status, hunt.json["status"].clone()),
        (200, json!("MATCHING")),
        "queued"
    );
}
