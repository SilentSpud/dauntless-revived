//! `backup.test.ts`: POST /internal/backup. The Node test held a copy in flight through an injected
//! copier to see the second request's 409; the Rust route has no such seam, so the duplicate is the
//! one that finds the copy already there. The test of a copy under constant writes is the store
//! crate's (`a_backup_under_constant_writes_is_one_consistent_snapshot`).

use serde_json::json;

use crate::common::{TestApp, req};

async fn app(root: &std::path::Path) -> TestApp {
    TestApp::with_env(&[("BACKUP_ROOT", root.to_str().unwrap())]).await
}

/// The route copies one database at a time in the whole process, so these tests take turns.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn post(name: &str) -> crate::common::Req {
    req().gs().json(json!({"name": name}))
}

#[tokio::test(flavor = "multi_thread")]
async fn live_backup_refuses_proxy_callers_wrong_keys_traversal_duplicate_copies_and_overwrites() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let root = tempfile::tempdir().unwrap();
    let name = "2026-10-05_120000";
    std::fs::create_dir(root.path().join(name)).unwrap();
    let app = app(root.path()).await;
    let path = "/internal/backup";
    assert_eq!(
        app.post(path, post(name).header("x-forwarded-for", "192.0.2.1"))
            .await
            .status,
        403
    );
    let wrong = req()
        .header("x-undaunted-gameserver-apikey", "wrong")
        .json(json!({"name": name}));
    assert_eq!(app.post(path, wrong).await.status, 403);
    assert_eq!(app.post(path, post("../outside")).await.status, 400);
    assert_eq!(app.post(path, post(name)).await.status, 200);
    assert_eq!(app.post(path, post(name)).await.status, 409);
    let copy = root.path().join(name).join("undaunted.db");
    let conn = rusqlite::Connection::open(&copy).unwrap();
    let users: i64 = conn.query_row("SELECT count(*) FROM users", [], |r| r.get(0)).unwrap();
    assert_eq!(users, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_budget_limits_authenticated_copies_before_filesystem_work_not_rejected_callers() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let root = tempfile::tempdir().unwrap();
    let app = app(root.path()).await;
    let path = "/internal/backup";
    for _ in 0..8 {
        let wrong = req()
            .header("x-undaunted-gameserver-apikey", "wrong")
            .json(json!({"name": "2026-10-05_120000"}));
        assert_eq!(app.post(path, wrong).await.status, 403);
    }
    for i in 0..6 {
        let name = format!("2026-10-05_12000{i}");
        std::fs::create_dir(root.path().join(&name)).unwrap();
        assert_eq!(app.post(path, post(&name)).await.status, 200, "{name}");
    }
    // This folder does not exist: the 429 shows the budget is checked before the copy
    let blocked = app.post(path, post("2026-10-05_120006")).await;
    assert_eq!(blocked.status, 429);
    let after: i64 = blocked.headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!(after > 0);
    assert_eq!(blocked.json["error"], "rate_limited");
}
