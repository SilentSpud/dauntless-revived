#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! TrackBackendHealth's second Node test: replies stay intact, each request counts once, monitoring
//! polls don't count, and nothing is counted with BACKEND_HEALTH off.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use metagame::health::{Health, track};
use service_kit::json::{Json, not_found};
use service_kit::routing::Router;
use tower::ServiceExt;

fn app(health: Arc<Health>) -> axum::Router {
    let routes = Router::new()
        .get("/fixture", || async {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"fixture": true})),
            )
        })
        .get("/undaunted/api/ServerStatus", || async { Json(serde_json::json!({})) })
        .get("/slow", || async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        });
    axum::Router::new()
        .fallback_service(routes.into_service((), not_found))
        .layer(axum::middleware::from_fn_with_state(health, track))
}

async fn get(app: &axum::Router, path: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap(),
    )
}

fn completed(health: &Health) -> u64 {
    health.report().requests.completed
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_count_once_and_monitoring_polls_never() {
    let health = Arc::new(Health::new(true));
    let app = app(health.clone());
    assert_eq!(
        get(&app, "/fixture").await,
        (StatusCode::SERVICE_UNAVAILABLE, r#"{"fixture":true}"#.into())
    );
    get(&app, "/undaunted/api/serverstatus/").await;
    let report = health.report().requests;
    assert_eq!((report.completed, report.server_errors), (1, 1));

    // A client that goes away before the reply counts as aborted.
    let slow = tokio::time::timeout(Duration::from_millis(50), get(&app, "/slow")).await;
    assert!(slow.is_err());
    assert_eq!(health.report().requests.aborted, 1);

    let off = Arc::new(Health::new(false));
    let app = self::app(off.clone());
    get(&app, "/fixture").await;
    assert_eq!(completed(&off), 0);
}
