//! `platform.test.ts`: the heartbeat's auth, the body log (LOG_BODIES: status, duration, per-path cap
//! and key redaction) and the switch reader. Switches are read once, at start, so a test that changes
//! one part-way uses a second app on the same body log file.

use metagame::bodylog::per_path_from_env;
use metagame::redact::redact;
use serde_json::{Value, json};
use service_kit::env::Env;

use crate::common::{TestApp, logged_infos, req, warnings};

const USER: &str = "UID-platform-a";

/// An app writing its body log to `file`, with `extra`.
async fn logging_app(file: &std::path::Path, extra: &[(&str, &str)]) -> TestApp {
    let file = file.to_string_lossy().into_owned();
    let mut env = vec![("LOG_BODIES", "1"), ("BODY_LOG_FILE", file.as_str())];
    env.extend_from_slice(extra);
    let app = TestApp::with_env(&env).await;
    // The account behind the token (POST /reconcile reads its notes)
    app.exec("INSERT OR IGNORE INTO users (userId, name, notes) VALUES ('UID-platform-a', 'PlatformA', 0)");
    app
}

fn lines(file: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .split_inclusive('\n')
        .filter(|l| l.ends_with('\n') && !l.trim().is_empty())
        .map(|l| serde_json::from_str(l.trim_end()).unwrap())
        .collect()
}

/// The line is written once the answer is done, and the write itself runs apart from it
async fn wait_for_lines(file: &std::path::Path, matches: impl Fn(&Value) -> bool, count: usize) -> Vec<Value> {
    for _ in 0..200 {
        if lines(file).iter().filter(|l| matches(l)).count() >= count {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    lines(file).into_iter().filter(|l| matches(l)).collect()
}

fn url_is(line: &Value, url: &str) -> bool {
    line["url"] == url
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bogus_or_missing_bearer_and_a_wrong_game_server_key_get_401() {
    let app = TestApp::start().await;
    let body = json!({"map": "/Game/Maps/ramsgate/ramsgate_01_persistent"});
    assert_eq!(
        app.post(
            "/heartbeat",
            req().header("authorization", "bearer not-a-token").json(body)
        )
        .await
        .status,
        401
    );
    assert_eq!(
        app.post(
            "/heartbeat",
            req().header("authorization", "Basic dXNlcjpwYXNz").json(json!({}))
        )
        .await
        .status,
        401
    );
    assert_eq!(app.post("/heartbeat", req().json(json!({}))).await.status, 401);
    let wrong = metagame::crypto::random_uuid().replace('-', "");
    let reply = app
        .post(
            "/heartbeat",
            req().header("x-undaunted-gameserver-apikey", &wrong).json(json!({})),
        )
        .await;
    assert_eq!(reply.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_token_gets_200_and_the_plain_text_20000() {
    let app = TestApp::start().await;
    app.exec("INSERT INTO users (userId, name, notes) VALUES ('UID-platform-a', 'PlatformA', 0)");
    let beat = app
        .post(
            "/heartbeat",
            req()
                .bearer(USER)
                .json(json!({"map": "/Game/Maps/ramsgate/ramsgate_01_persistent"})),
        )
        .await;
    assert_eq!((beat.status, beat.text.as_str()), (200, "20000"));
    assert!(beat.headers["content-type"].to_str().unwrap().starts_with("text/plain"));
}

#[tokio::test(flavor = "multi_thread")]
async fn records_the_answer_s_status_and_duration_with_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bodies.log");
    let app = logging_app(&file, &[]).await;
    let before = metagame::time::now_ms();
    let refused = app
        .post(
            "/progression/UID-platform-status",
            req().bearer(USER).json(json!({"progress_tracks": []})),
        )
        .await;
    assert_eq!(refused.status, 403, "a player may not grant progression");
    let found = wait_for_lines(&file, |l| url_is(l, "/progression/UID-platform-status"), 1).await;
    let line = found.first().expect("the line was written");
    let keys: Vec<&String> = line.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["t", "method", "url", "gs", "body", "status", "ms"]);
    assert_eq!(
        json!([line["method"], line["gs"], line["status"]]),
        json!(["POST", 0, 403])
    );
    assert_eq!(line["body"], json!({"progress_tracks": []}).to_string());
    assert!(line["ms"].as_u64().is_some_and(|ms| ms < 60_000));
    assert!(metagame::js::date_parse(line["t"].as_str().unwrap()).unwrap() >= before - 1000);
}

#[tokio::test(flavor = "multi_thread")]
async fn also_records_the_store_slayer_link_and_reconcile_routes_and_still_leaves_other_routes_out() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bodies.log");
    let app = logging_app(&file, &[]).await;
    let key = format!(
        "{}{}",
        metagame::crypto::random_uuid().replace('-', ""),
        metagame::crypto::random_uuid().replace('-', "")
    );
    assert_eq!(
        app.get("/product/skus/public?requiredTags=Store", req().bearer(USER))
            .await
            .status,
        400
    );
    app.get("/product/sku/SKU_TEST", req().bearer(USER)).await;
    app.get("/token/platinum/SKU_TEST", req().bearer(USER)).await;
    app.post(
        &format!("/notification/platinum?token={key}"),
        req().bearer(USER).json(json!({})),
    )
    .await;
    app.get("/slayerlink/invites", req().bearer(USER)).await;
    app.post("/reconcile", req().bearer(USER).json(json!({}))).await;
    app.post(
        "/trials/leaderboards",
        req()
            .bearer(USER)
            .json(json!({"difficulty": 1, "trial_id": "Arena_MatchmakerHunt_Elite_001"})),
    )
    .await;
    app.post(
        "/profile/update",
        req()
            .bearer(USER)
            .json(json!({"dauntlessid": USER, "currentdisplayname": "PlatformA"})),
    )
    .await;
    app.get(&format!("/playertreatments/{USER}"), req().bearer(USER)).await;

    let wanted =
        regex::Regex::new("^/(product|token|notification|slayerlink|reconcile|trials|profile/update|playertreatments)")
            .unwrap();
    let found = wait_for_lines(&file, |l| wanted.is_match(l["url"].as_str().unwrap_or_default()), 8).await;
    let mut urls: Vec<&str> = found.iter().map(|l| l["url"].as_str().unwrap()).collect();
    urls.sort();
    assert_eq!(
        urls,
        [
            "/notification/platinum?token=<redacted>",
            "/product/sku/SKU_TEST",
            "/product/skus/public?requiredTags=Store",
            "/profile/update",
            "/reconcile",
            "/slayerlink/invites",
            "/token/platinum/SKU_TEST",
            "/trials/leaderboards"
        ]
    );
    assert_eq!(
        found
            .iter()
            .find(|l| l["url"].as_str().unwrap().starts_with("/product/skus"))
            .unwrap()["status"],
        400
    );
    assert!(
        !lines(&file)
            .iter()
            .any(|l| l["url"].as_str().unwrap_or_default().contains(&key)),
        "the purchase token is never written"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn blanks_account_keys_uuk_and_48_hex_characters_in_the_url_and_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bodies.log");
    let app = logging_app(&file, &[]).await;
    let key = format!(
        "UUK_{}",
        &format!("{}{}", metagame::crypto::random_uuid(), metagame::crypto::random_uuid()).replace('-', "")[..48]
    );
    assert_eq!(key.len(), 52);
    let body = json!({"apiKey": key, "nested": [format!("x {key} y")]});
    app.post(
        &format!("/progression/UID-platform-key?key={key}"),
        req().bearer(USER).json(body),
    )
    .await;
    let found = wait_for_lines(
        &file,
        |l| {
            l["url"]
                .as_str()
                .unwrap_or_default()
                .starts_with("/progression/UID-platform-key")
        },
        1,
    )
    .await;
    assert_eq!(found[0]["url"], "/progression/UID-platform-key?key=<redacted>");
    assert_eq!(
        found[0]["body"],
        json!({"apiKey": "<redacted>", "nested": ["x <redacted> y"]}).to_string()
    );
    assert!(!std::fs::read_to_string(&file).unwrap().contains(&key));
}

#[tokio::test(flavor = "multi_thread")]
async fn body_log_per_path_caps_the_lines_per_method_and_path_unset_0_is_no_cap() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bodies.log");
    let capped = logging_app(&file, &[("BODY_LOG_PER_PATH", "2")]).await;
    logged_infos();
    let path = "/progression/UID-platform-cap";
    for i in 0..4 {
        capped.post(path, req().bearer(USER).json(json!({"i": i}))).await;
    }
    capped
        .post(
            "/progression/UID-platform-cap-other",
            req().bearer(USER).json(json!({"i": 0})),
        )
        .await;
    wait_for_lines(&file, |l| url_is(l, "/progression/UID-platform-cap-other"), 1).await;
    let numbers = |found: Vec<Value>| -> Vec<Value> {
        found
            .iter()
            .map(|l| serde_json::from_str::<Value>(l["body"].as_str().unwrap()).unwrap()["i"].clone())
            .collect()
    };
    assert_eq!(
        numbers(lines(&file).into_iter().filter(|l| url_is(l, path)).collect()),
        [json!(0), json!(1)]
    );
    let message =
        "body log: POST /progression/UID-platform-cap reached BODY_LOG_PER_PATH=2; no more lines for it in this run";
    assert!(logged_infos().iter().any(|line| line == message));
    // Unset (read at start): a second app writing to the same file
    let uncapped = logging_app(&file, &[]).await;
    uncapped.post(path, req().bearer(USER).json(json!({"i": 9}))).await;
    assert_eq!(
        numbers(wait_for_lines(&file, |l| url_is(l, path), 3).await),
        [json!(0), json!(1), json!(9)]
    );
}

#[test]
fn redact_keeps_the_old_rules_for_tokens_and_long_runs() {
    let jwt = [
        "eyJhbGciOiJSUzI1NiJ9",
        "eyJ1c2VySWQiOiJVSUQtYSJ9",
        "c2lnbmF0dXJlLW5vdC1yZWFs",
    ]
    .join(".");
    assert_eq!(
        redact(&format!("/account/api/oauth/sessions/kill/{jwt}")),
        "/account/api/oauth/sessions/kill/<token>"
    );
    assert_eq!(redact(&"x".repeat(64)), "<redacted>");
    assert_eq!(redact(&"x".repeat(63)), "x".repeat(63));
    let short = format!("UUK_{}", "0".repeat(47));
    assert_eq!(redact(&short), short, "not a whole key");
    assert_eq!(
        redact("/party/UID-00000000-0000-4000-8000-00000000f609"),
        "/party/UID-00000000-0000-4000-8000-00000000f609"
    );
}

/// The required settings a `Config` needs, plus `extra`.
fn config(extra: &[(&str, &str)]) -> metagame::config::Config {
    let mut pairs = vec![
        ("AUTH_MODE", "APIKEY"),
        ("AUTH_SIGNING_PRIVKEY_B64", "x"),
        ("AUTH_SIGNING_PUBKEY_B64", "y"),
        ("DB_FILENAME", "undaunted.db"),
        ("PORT", "1"),
    ];
    pairs.extend_from_slice(extra);
    metagame::config::Config::from_env(&Env::from_pairs(pairs)).unwrap()
}

/// Node's `ParseOnOff`, `ParseCount` and `ParseChoice` are inside the settings in Rust: each is read
/// through a switch that uses it.
#[test]
fn parses_on_off_counts_and_choices_and_refuses_anything_else() {
    for on in ["1", "true", "TRUE", "on", "yes"] {
        assert!(
            config(&[("VERIFY_STUB_ACCOUNT", on)]).features.verify_stub_account,
            "{on}"
        );
    }
    for off in ["0", "false", "Off", "no", "2", "enabled", ""] {
        assert!(
            !config(&[("VERIFY_STUB_ACCOUNT", off)]).features.verify_stub_account,
            "{off} (off is the default)"
        );
    }
    assert!(
        config(&[("TRIALS_STORE", "2")]).store.trials_store,
        "an invalid value is the default (on)"
    );
    let count = |value: &str| per_path_from_env(&Env::from_pairs([("BODY_LOG_PER_PATH", value)]));
    assert_eq!((count("0"), count("25")), (0, 25));
    for bad in ["-1", "1.5", "ten", "1e3", "9999999999"] {
        assert_eq!(count(bad), 0, "{bad}");
    }
    assert!(config(&[("STORE", "FREE")]).store.free);
    assert!(!config(&[("STORE", "priced")]).store.free);
}

#[test]
fn uses_the_default_for_an_unset_empty_or_invalid_value_with_one_warning_per_invalid_value() {
    let count = |value: Option<&str>| match value {
        Some(value) => per_path_from_env(&Env::from_pairs([("BODY_LOG_PER_PATH", value)])),
        None => per_path_from_env(&Env::from_pairs([])),
    };
    let (counts, warned) = warnings(|| [count(None), count(Some(" ")), count(Some(" 7 ")), count(Some("lots"))]);
    assert_eq!(counts, [0, 0, 7, 0]);
    assert_eq!(
        warned,
        [r#"BODY_LOG_PER_PATH="lots" is not a valid value; using the default (no-cap)"#]
    );
}

#[test]
fn describes_every_switch_in_one_boot_line() {
    assert_eq!(
        config(&[]).describe_features(),
        "features: bodyLogPerPath=no-cap escalation=stub escalationStrict=off store=off storeRepeatableTokens=off trialsStore=on middlemanStore=on replayWindow=5s confirmEntitlements=off balanceFromInventory=on middlemanFusionGuards=off trialsLeaderboards=on trialsSchedule=off slayerLinks=on chatPresence=on verifyStubAccount=off"
    );
    assert_eq!(
        config(&[("BODY_LOG_PER_PATH", "5")]).describe_features(),
        "features: bodyLogPerPath=5 escalation=stub escalationStrict=off store=off storeRepeatableTokens=off trialsStore=on middlemanStore=on replayWindow=5s confirmEntitlements=off balanceFromInventory=on middlemanFusionGuards=off trialsLeaderboards=on trialsSchedule=off slayerLinks=on chatPresence=on verifyStubAccount=off"
    );
}
