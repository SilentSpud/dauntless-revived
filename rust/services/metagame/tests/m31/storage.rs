//! `storage.test.ts`: the database's tables, and encountered content and breadcrumbs under concurrent
//! requests. The journal-mode tests do not carry over: the Rust metagame runs SQLite in WAL mode
//! (D17), and switching a file back to the rollback journal is `metagame db journal delete`.

use metagame::characterprogress::{add_encountered, get_breadcrumbs, query_encountered, set_breadcrumbs};
use serde_json::json;

use crate::common::TestApp;

fn names(app: &TestApp) -> Vec<String> {
    app.rows("SELECT name FROM sqlite_master WHERE type IN ('table', 'trigger')", &[])
        .into_iter()
        .map(|row| row["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn has_the_save_history_tables_and_the_append_only_triggers_after_migrating() {
    let app = TestApp::start().await;
    let names = names(&app);
    for name in [
        "characterhistory",
        "loadouthistory",
        "inventorylog",
        "inventorytransactions",
        "inventorylog_no_update",
        "inventorylog_no_delete",
        "users",
        "invitecodes",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} missing");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn has_the_real_progression_tables_and_the_audit_triggers_after_migrating() {
    let app = TestApp::start().await;
    let names = names(&app);
    for name in [
        "progress_tracks",
        "objectives",
        "progression_events",
        "huntpassselection",
        "entitlements",
        "cooldowns",
        "bounties",
        "bountydraft",
        "loadoutslots",
        "progression_events_no_update",
        "progression_events_no_delete",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} missing");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_every_entry_when_many_are_added_at_once_a_lost_update_before() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let adds = (0..20).map(|i| {
        let (user, character) = (user.clone(), character.clone());
        app.try_db(move |conn| {
            Ok(add_encountered(
                conn,
                &user,
                &character,
                Some(&json!(5)),
                Some(&json!(format!("NPC_{i}"))),
            ))
        })
    });
    for result in futures_util::future::join_all(adds).await {
        assert_eq!(result.unwrap(), Ok(()));
    }
    let (u, c) = (user.clone(), character.clone());
    let found = app
        .db(move |conn| Ok(query_encountered(conn, &u, &c, &json!([5]))))
        .await
        .unwrap();
    assert_eq!(found[0]["content"].as_array().unwrap().len(), 20);
}

#[tokio::test(flavor = "multi_thread")]
async fn creates_the_breadcrumbs_row_once_under_concurrent_reads() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let reads = (0..5).map(|_| {
        let (user, character) = (user.clone(), character.clone());
        app.try_db(move |conn| Ok(get_breadcrumbs(conn, &user, &character)))
    });
    for result in futures_util::future::join_all(reads).await {
        assert!(result.unwrap().is_ok());
    }
    assert_eq!(app.count("breadcrumbs", "characterId = ?1", &[&character]), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn lets_only_one_of_two_breadcrumb_writes_with_the_same_version_win() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let (u, c) = (user.clone(), character.clone());
    app.db(move |conn| Ok(get_breadcrumbs(conn, &u, &c))).await.unwrap();
    let write = |crumb: &'static str| {
        let (user, character) = (user.clone(), character.clone());
        app.try_db(move |conn| {
            Ok(set_breadcrumbs(
                conn,
                &user,
                &character,
                Some(&json!([crumb])),
                Some(&json!(1)),
            ))
        })
    };
    let (a, b) = tokio::join!(write("a"), write("b"));
    let mut won = [a.unwrap().is_ok(), b.unwrap().is_ok()];
    won.sort_unstable();
    assert_eq!(won, [false, true]);
}

/// Through POST /encountered-content/:characterId, which checks the owner first (Node's
/// `RunProgressionOperation` did it inside the controller).
#[tokio::test(flavor = "multi_thread")]
async fn refuses_another_player_s_character() {
    let app = TestApp::start().await;
    let (a, _) = app.make_player().await;
    let (_, b_character) = app.make_player().await;
    let body = json!({"content_type": 5, "content_id": "NPC"});
    let reply = app
        .post(
            &format!("/encountered-content/{b_character}"),
            crate::common::req().bearer(&a).json(body),
        )
        .await;
    assert_eq!(reply.status, 403);
    assert_eq!(app.count("encounteredcontent", "characterId = ?1", &[&b_character]), 0);
}
