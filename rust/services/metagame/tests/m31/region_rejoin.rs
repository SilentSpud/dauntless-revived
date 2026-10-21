//! `region-rejoin.test.ts`: a player who changes region after a ready solo candidate gets a new server
//! in the new region. Node replaced `fetch` and the region reader; here a deploy server answers by the
//! request's region, and the region is the stored one.

use std::sync::{Arc, Mutex};

use metagame::matchmaking::{self, region::Region};
use serde_json::{Value, json};

use crate::common::TestApp;

const USER: &str = "UID-region-test";

/// A deploy server whose host names the region it was asked for.
async fn regional_deploy() -> (String, Arc<Mutex<Vec<Value>>>) {
    use axum::response::IntoResponse;
    let calls: Arc<Mutex<Vec<Value>>> = Arc::default();
    let seen = calls.clone();
    let handler = move |body: axum::body::Bytes| {
        let seen = seen.clone();
        async move {
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let host = if body["Region"] == "us" { "us" } else { "main" };
            seen.lock().unwrap().push(body);
            (
                [("content-type", "application/json")],
                json!({"host": host, "port": 39000}).to_string(),
            )
                .into_response()
        }
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().fallback(handler))
            .await
            .unwrap();
    });
    (address, calls)
}

async fn set_region(app: &TestApp, region: Region) {
    app.db(move |conn| matchmaking::region::save_region(conn, USER, region))
        .await;
}

async fn join(app: &TestApp, mode: &str, hunt: &str, private: bool) {
    let joined = matchmaking::join(
        &app.state,
        Some(json!(mode)),
        Some(json!("")),
        Some(json!(hunt)),
        USER,
        private,
    )
    .await;
    assert!(joined.unwrap());
}

async fn host(app: &TestApp) -> Option<String> {
    matchmaking::check_and_update(&app.state, USER)
        .await
        .map(|entry| entry.host)
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_from_eu_to_us_cannot_reuse_a_ready_solo_eu_candidate() {
    let (address, calls) = regional_deploy().await;
    let app = TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", address.as_str()),
        ("US_REGION", "1"),
    ])
    .await;
    app.run(
        "INSERT INTO users (userId, name, notes) VALUES (?1, 'RegionTest', 0)",
        &[&USER],
    );
    join(&app, "ISLAND", "Hunt_Test_A", true).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(host(&app).await.as_deref(), Some("main"));
    set_region(&app, Region::Us).await;
    join(&app, "ISLAND", "Hunt_Test_A", true).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(host(&app).await.as_deref(), Some("us"));
    assert_eq!(calls.lock().unwrap().len(), 2);
    join(&app, "SHARED", "ShatteredIsles_TrainingDojo", false).await;
    assert_eq!(calls.lock().unwrap().last().unwrap()["Region"], "us");
}
