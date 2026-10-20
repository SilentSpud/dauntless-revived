#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! Parity with the Node metagame, area by area (MIGRATION_PLAN.md M29, M31): each
//! `parity/<area>.requests.json` is run against the Rust app and the replies compared with
//! `parity/<area>.expected.json`, which `parity/record.cjs` recorded from the Node metagame. Status,
//! the headers clients and tools read, and the body text must be the same, after replacing what
//! differs on every run (normalize(), kept in step with record.cjs).

use std::path::Path;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use metagame::config::Config;
use regex::Regex;
use serde_json::{Value, json};
use service_kit::env::Env;
use service_kit::serve::ConnectionInfo;
use tower::ServiceExt;

const HEADERS: [&str; 7] = [
    "content-type",
    "cache-control",
    "retry-after",
    "ratelimit",
    "ratelimit-policy",
    "content-security-policy",
    "x-content-type-options",
];

fn normalize(text: &str) -> String {
    let rules = [
        (r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+", "<jwt>"),
        (r"UUK_[0-9a-f]{48}", "<uuk>"),
        (
            r"UID-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            "<uid>",
        ),
        (r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z", "<date>"),
        (
            r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            "<uuid>",
        ),
        (
            r"\b(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{1,2}, \d{4}\b",
            "<day>",
        ),
        (
            r"[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}",
            "<invite>",
        ),
    ];
    rules.iter().fold(text.to_owned(), |text, (pattern, with)| {
        Regex::new(pattern).unwrap().replace_all(&text, *with).into_owned()
    })
}

async fn run_area(area: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let script: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{area}.requests.json"))).unwrap()).unwrap();
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{area}.expected.json"))).unwrap()).unwrap();
    let keys: Value = serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap();

    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("undaunted.db");
    let mut pairs: Vec<(String, String)> = vec![
        ("PORT".into(), "0".into()),
        ("DB_FILENAME".into(), db.to_string_lossy().into_owned()),
        (
            "AUTH_SIGNING_PRIVKEY_B64".into(),
            keys["privB64"].as_str().unwrap().into(),
        ),
        (
            "AUTH_SIGNING_PUBKEY_B64".into(),
            keys["pubB64"].as_str().unwrap().into(),
        ),
    ];
    for (name, value) in script["env"].as_object().unwrap() {
        pairs.push((name.clone(), value.as_str().unwrap().into()));
    }
    let env = Env::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let config = Config::from_env(&env).unwrap();
    let progression = metagame::progression_config::from_env(&env).unwrap();
    let state = metagame::server::prepare(&config, progression).unwrap().state;
    {
        let mut conn = SqliteConnection::establish(db.to_str().unwrap()).unwrap();
        for sql in script["setup"].as_array().unwrap() {
            conn.batch_execute(sql.as_str().unwrap()).unwrap();
        }
    }
    let app = metagame::server::app(state.clone(), false);
    let game_server_key = script["gameServerKey"].as_str().unwrap_or("").to_owned();

    let mut actual = Vec::new();
    for request in script["requests"].as_array().unwrap() {
        let mut builder = Request::builder()
            .method(request["method"].as_str().unwrap())
            .uri(request["path"].as_str().unwrap());
        for (name, value) in request["headers"].as_object().into_iter().flatten() {
            let mut value = value.as_str().unwrap().replace("{{gs}}", &game_server_key);
            if let Some(start) = value.find("{{bearer:") {
                let end = value[start..].find("}}").unwrap() + start;
                let user = value[start + 9..end].to_owned();
                let token = metagame::auth::sign_token(&state, &user).await.unwrap().unwrap();
                value = format!("{}bearer {token}{}", &value[..start], &value[end + 2..]);
            }
            builder = builder.header(name, value);
        }
        let body = if let Some(json) = request.get("json") {
            builder = builder.header("content-type", "application/json; charset=utf-8");
            Body::from(serde_json::to_vec(json).unwrap())
        } else if let Some(raw) = request.get("raw") {
            builder = builder.header("content-type", "application/json; charset=utf-8");
            Body::from(raw.as_str().unwrap().to_owned())
        } else if let Some(form) = request.get("form") {
            builder = builder.header("content-type", "application/x-www-form-urlencoded");
            Body::from(form.as_str().unwrap().to_owned())
        } else {
            Body::empty()
        };
        let mut http = builder.body(body).unwrap();
        let connection = ConnectionInfo {
            peer: "127.0.0.1:50000".parse().unwrap(),
            local: "127.0.0.1:61000".parse().unwrap(),
        };
        http.extensions_mut().insert(ConnectInfo(connection));
        let response = app.clone().oneshot(http).await.unwrap();
        let mut headers = serde_json::Map::new();
        for name in HEADERS {
            if let Some(value) = response.headers().get(name) {
                headers.insert(name.into(), normalize(value.to_str().unwrap()).into());
            }
        }
        let status = response.status().as_u16();
        let body = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
        actual.push(json!({
            "request": format!("{} {}", request["method"].as_str().unwrap(), request["path"].as_str().unwrap()),
            "status": status,
            "headers": headers,
            "body": normalize(&body),
        }));
    }

    let expected = expected["results"].as_array().unwrap();
    let differences: Vec<String> = expected
        .iter()
        .zip(&actual)
        .enumerate()
        .filter(|(_, (want, got))| want != got)
        .map(|(index, (want, got))| {
            format!(
                "#{index} {}\n  node: {want}\n  rust: {got}",
                want["request"].as_str().unwrap_or("")
            )
        })
        .collect();
    assert_eq!(expected.len(), actual.len(), "request count");
    assert!(
        differences.is_empty(),
        "{} of {} replies differ:\n{}",
        differences.len(),
        actual.len(),
        differences.join("\n")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn accounts() {
    run_area("accounts").await;
}

/// Every Rust route is a route of the Node metagame (`UndauntedMetagame/routes.json`, M3), in the
/// same order, so first-match routing picks the same one.
#[tokio::test(flavor = "multi_thread")]
async fn route_table_matches_node() {
    let node: Value = serde_json::from_str(include_str!("../../../../UndauntedMetagame/routes.json")).unwrap();
    let node: Vec<(String, String)> = node["routes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|route| route["method"] != "USE")
        .map(|route| {
            (
                route["method"].as_str().unwrap().to_owned(),
                route["path"].as_str().unwrap().to_owned(),
            )
        })
        .collect();

    let keys: Value = serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("undaunted.db");
    let db_text = db.to_string_lossy().into_owned();
    let env = Env::from_pairs([
        ("PORT", "0"),
        ("AUTH_MODE", "APIKEY"),
        ("DB_FILENAME", db_text.as_str()),
        ("AUTH_SIGNING_PRIVKEY_B64", keys["privB64"].as_str().unwrap()),
        ("AUTH_SIGNING_PUBKEY_B64", keys["pubB64"].as_str().unwrap()),
    ]);
    let config = Config::from_env(&env).unwrap();
    let state = metagame::server::prepare(&config, metagame::progression_config::from_env(&env).unwrap())
        .unwrap()
        .state;

    let mut last = None;
    for (method, path) in metagame::routes::all(&state).table() {
        let index = node
            .iter()
            .position(|(m, p)| *m == method.as_str() && *p == path)
            .unwrap_or_else(|| panic!("{method} {path} is not a Node route"));
        assert!(
            last.is_none_or(|last| index > last),
            "{method} {path} is out of Node's order"
        );
        last = Some(index);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn characters() {
    run_area("characters").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn characters_stub() {
    run_area("characters-stub").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn loadouts() {
    run_area("loadouts").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn loadouts_stub() {
    run_area("loadouts-stub").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn inventory() {
    run_area("inventory").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn inventory_switches() {
    run_area("inventory-switches").await;
}
