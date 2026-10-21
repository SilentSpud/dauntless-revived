//! `moderation.test.ts`: temporary and address bans, the moderation log and info, and the trusted
//! address the heartbeat records. Node's `RecordPlayingAddress` with a request is the activity store's
//! `record_address`, or a heartbeat through the gateway; `ValidateMetagameJWTAndGetPayload` is a
//! heartbeat with a token signed before the ban.

use metagame::moderation::{self, Refusal};
use serde_json::{Value, json};

use crate::common::{TestApp, req, sha256_hex};

const DAY: i64 = 86_400_000;
const SECRET: &str = "test-gateway-secret-0123456789";

fn now() -> i64 {
    metagame::time::now_ms()
}

async fn set_ban(app: &TestApp, body: Value) -> Result<Value, &'static str> {
    let state = app.state.clone();
    let done = app
        .state
        .store
        .write(move |conn| {
            Ok::<_, store::Error>(moderation::set_ban(
                conn,
                &mut state.activity.lock(),
                Some(&body),
                "owner",
                now(),
            ))
        })
        .await
        .unwrap();
    done.map_err(|refusal| match refusal {
        Refusal::Code(code) => code,
        Refusal::Db(message) => panic!("{message}"),
    })
}

async fn info(app: &TestApp, account: &str) -> Value {
    let (state, account) = (app.state.clone(), account.to_owned());
    app.db(move |conn| moderation::info(conn, &mut state.activity.lock(), &account, now()))
        .await
}

async fn ban_for(app: &TestApp, account: &str, address: Option<&str>) -> Option<String> {
    let (account, address) = (account.to_owned(), address.map(str::to_owned));
    app.db(move |conn| metagame::auth::ban_for(conn, Some(&account), address.as_deref()))
        .await
}

async fn log(app: &TestApp, events: bool, offset: f64) -> Result<Value, &'static str> {
    let done = app
        .state
        .store
        .write(move |conn| Ok::<_, store::Error>(moderation::log(conn, events, offset, now())))
        .await
        .unwrap();
    done.map_err(|refusal| match refusal {
        Refusal::Code(code) => code,
        Refusal::Db(message) => panic!("{message}"),
    })
}

fn rows(log: &Value) -> Vec<Value> {
    log["rows"].as_array().unwrap().clone()
}

fn record(app: &TestApp, account: &str, address: &str, at: i64) {
    app.state
        .activity
        .lock()
        .record_address(account, address.to_owned(), at);
}

fn ban_body(account: &str, reason: &str, active: bool, address: Option<&str>, hours: Option<Value>) -> Value {
    let mut body = json!({"accountId": account, "reason": reason, "active": active, "address": address});
    if let Some(hours) = hours {
        body["durationHours"] = hours;
    }
    body
}

#[tokio::test(flavor = "multi_thread")]
async fn temporary_account_only_bans_expire_preserve_history_and_reject_invalid_durations() {
    let app = TestApp::start().await;
    let account = "UID-temp-ban";
    app.exec("INSERT INTO users (userId, name, notes) VALUES ('UID-temp-ban', 'Temporary', 0)");
    let start = now();
    set_ban(
        &app,
        ban_body(account, "Verified evidence", true, None, Some(json!(168))),
    )
    .await
    .unwrap();
    let ban = info(&app, account).await["ban"].clone();
    let expires = ban["expires_at"].as_i64().unwrap();
    assert!(expires >= start + 7 * DAY && expires <= now() + 7 * DAY);
    assert_eq!(ban_for(&app, account, None).await.as_deref(), Some("Verified evidence"));
    assert_eq!(ban_for(&app, "UID-unrelated", Some("203.0.113.9")).await, None);
    assert!(
        rows(&log(&app, false, 0.0).await.unwrap())
            .iter()
            .any(|r| r["account_id"] == account)
    );
    for hours in [json!(0), json!(-1), json!(721), json!(1.5), Value::Null, json!("168")] {
        assert_eq!(
            set_ban(&app, ban_body(account, "reason", true, None, Some(hours.clone())))
                .await
                .err(),
            Some("invalid_duration"),
            "{hours}"
        );
    }
    assert_eq!(
        set_ban(
            &app,
            ban_body(account, "reason", true, Some("203.0.113.9"), Some(json!(168)))
        )
        .await
        .err(),
        Some("address_not_observed")
    );
    let past = now() - 1;
    app.run(
        "UPDATE account_bans SET expires_at = ?1 WHERE account_id = ?2",
        &[&past, &account],
    );
    assert_eq!(ban_for(&app, account, None).await, None);
    assert_eq!(info(&app, account).await["ban"]["active"], 0);
    assert!(
        !rows(&log(&app, false, 0.0).await.unwrap())
            .iter()
            .any(|r| r["account_id"] == account)
    );
    assert!(
        rows(&log(&app, true, 0.0).await.unwrap())
            .iter()
            .any(|r| r["account_id"] == account && r["expires_at"] == expires)
    );
    assert_eq!(log(&app, true, -1.0).await.err(), Some("invalid_offset"));
    set_ban(&app, ban_body(account, "Verified again", true, None, Some(json!(168))))
        .await
        .unwrap();
    set_ban(&app, ban_body(account, "Appeal accepted", false, None, None))
        .await
        .unwrap();
    assert_eq!(ban_for(&app, account, None).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn temporary_observed_ip_bans_expire_and_revoke_with_the_account_sanction() {
    let app = TestApp::start().await;
    let (account, address) = ("UID-temp-ip-ban", "203.0.113.123");
    app.exec("INSERT INTO users (userId, name, notes) VALUES ('UID-temp-ip-ban', 'TemporaryIP', 0)");
    let ip_ban = |reason: &str, active: bool, address: Option<&str>, hours: Option<Value>| {
        ban_body(account, reason, active, address, hours)
    };
    assert_eq!(
        set_ban(&app, ip_ban("Verified evidence", true, Some(address), Some(json!(168))))
            .await
            .err(),
        Some("address_not_observed")
    );
    assert_eq!(info(&app, account).await["ban"], Value::Null);
    record(&app, account, address, now() - 90_001);
    assert_eq!(
        set_ban(&app, ip_ban("Verified evidence", true, Some(address), Some(json!(168))))
            .await
            .err(),
        Some("address_not_observed")
    );
    record(&app, account, address, now());
    let start = now();
    set_ban(&app, ip_ban("Verified evidence", true, Some(address), Some(json!(168))))
        .await
        .unwrap();
    let expires = info(&app, account).await["ban"]["expires_at"].as_i64().unwrap();
    assert!(expires >= start + 7 * DAY && expires <= now() + 7 * DAY);
    assert_eq!(ban_for(&app, account, None).await.as_deref(), Some("Verified evidence"));
    assert_eq!(
        ban_for(&app, "UID-unrelated", Some(address)).await.as_deref(),
        Some("Verified evidence")
    );
    assert_eq!(ban_for(&app, "UID-unrelated", Some("203.0.113.124")).await, None);
    let past = now() - 1;
    app.run(
        "UPDATE account_bans SET expires_at = ?1 WHERE account_id = ?2",
        &[&past, &account],
    );
    assert_eq!(ban_for(&app, account, None).await, None);
    assert_eq!(ban_for(&app, "UID-unrelated", Some(address)).await, None);
    set_ban(&app, ip_ban("Verified again", true, Some(address), Some(json!(168))))
        .await
        .unwrap();
    app.state.activity.lock().end_address(account);
    set_ban(&app, ip_ban("Appeal accepted", false, None, None))
        .await
        .unwrap();
    assert_eq!(ban_for(&app, account, None).await, None);
    assert_eq!(ban_for(&app, "UID-unrelated", Some(address)).await, None);
}

fn addresses(info: &Value) -> usize {
    info["addresses"].as_array().unwrap().len()
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_ip_logging_account_ip_bans_required_reasons_jwt_revocation_and_unban() {
    let app = TestApp::with_env(&[("GATEWAY_SECRET", SECRET)]).await;
    app.exec("INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-ban-test', 'BanTest', 0, 0), ('UID-admin-test', 'AdminTest', 0, 1)");
    let (key, admin) = (format!("UUK_{}", "a".repeat(48)), format!("UUK_{}", "b".repeat(48)));
    app.run(
        "INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-ban-test', ?1), ('UID-admin-test', ?2)",
        &[&sha256_hex(&key), &sha256_hex(&admin)],
    );
    let token = app.token("UID-ban-test").await;
    let account = "UID-ban-test";
    let beat = |through_gateway: bool| {
        let mut request = req()
            .header("authorization", &format!("Bearer {token}"))
            .header("x-forwarded-for", "203.0.113.9")
            .json(json!({"map": "ramsgate", "state": "city"}));
        if through_gateway {
            request = request.header("x-dauntless-gateway", SECRET);
        }
        app.post_owned("/heartbeat".into(), request)
    };
    // A forwarded address without the gateway's secret is not trusted, so nothing is recorded
    assert_eq!(beat(false).await.status, 200);
    assert_eq!(addresses(&info(&app, account).await), 0);
    assert_eq!(beat(true).await.status, 200);
    let seen = info(&app, account).await;
    assert_eq!((seen["username"].clone(), addresses(&seen)), (json!("BanTest"), 1));
    app.state.activity.lock().end_address(account);
    assert_eq!(addresses(&info(&app, account).await), 0);
    record(&app, account, "203.0.113.9", now() - 90_001);
    assert_eq!(addresses(&info(&app, account).await), 0, "expired");
    record(&app, account, "203.0.113.9", now());
    assert_eq!(
        app.count("player_addresses", "1=1", &[]),
        0,
        "addresses are never stored"
    );

    assert_eq!(
        set_ban(&app, ban_body(account, " ", true, None, None)).await.err(),
        Some("invalid_ban")
    );
    assert_eq!(
        set_ban(&app, ban_body("UID-admin-test", "reason", true, None, None))
            .await
            .err(),
        Some("admin_account_protected")
    );
    assert_eq!(
        set_ban(&app, ban_body(account, "reason", true, Some("203.0.113.10"), None))
            .await
            .err(),
        Some("address_not_observed")
    );
    set_ban(
        &app,
        ban_body(account, "Repeated harassment", true, Some("203.0.113.9"), None),
    )
    .await
    .unwrap();
    assert_eq!(
        ban_for(&app, "UID-other", Some("203.0.113.9")).await.as_deref(),
        Some("Repeated harassment")
    );
    assert_eq!(beat(true).await.status, 403, "the token of a banned account is refused");

    let with_key = |k: &str| req().header("x-undaunted-user-api-key", k);
    let get = |path: &str, k: &str| app.get_owned(path.to_owned(), with_key(k));
    let post = |path: &str, k: &str, body: Value| app.post_owned(path.to_owned(), with_key(k).json(body));
    assert_eq!(get("/undaunted/api/ModerationLog?kind=events", &key).await.status, 403);
    assert_eq!(
        get("/undaunted/api/ModerationLog?kind=events", &admin).await.status,
        200
    );
    assert_eq!(
        get("/undaunted/api/ModerationLog?kind=bans&offset=-1", &admin)
            .await
            .status,
        400
    );
    let banned = get("/undaunted/api/GetUserInfo", &key).await;
    assert_eq!(
        (banned.status, banned.json),
        (403, json!({"error": "account_banned", "reason": "Repeated harassment"}))
    );
    assert_eq!(get("/undaunted/api/Moderation/UID-ban-test", &key).await.status, 403);
    let unban = json!({"accountId": account, "reason": "Appeal accepted", "active": false});
    assert_eq!(post("/undaunted/api/Moderation", &admin, unban).await.status, 200);
    assert_eq!(get("/undaunted/api/GetUserInfo", &key).await.status, 200);
    assert_eq!(ban_for(&app, "UID-other", Some("203.0.113.9")).await, None);
    assert_eq!(beat(true).await.status, 200, "the token is good again");
    assert_eq!(addresses(&info(&app, account).await), 1);
    assert_eq!(post("/undaunted/api/PlayingEnded", &key, json!({})).await.status, 204);
    assert_eq!(addresses(&info(&app, account).await), 0);
    assert_eq!(get("/undaunted/api/GetUserInfo", &key).await.status, 200);
    assert_eq!(addresses(&info(&app, account).await), 0);
}
