//! `unlocker-detection.test.ts`: a known unlocker's exact mutation suspends the account that sent it,
//! for seven days, and nothing else does.

use metagame::auth::{MetagameAuth, ban_for};
use metagame::crypto::TokenClaims;
use metagame::moderation::{info, log, set_ban};
use metagame::unlocker::{Detection, enforce};
use serde_json::{Value, json};

use crate::common::TestApp;

const SOURCE: &str = "nova-unlocker:platinum:user-request";

fn mutation() -> Value {
    json!({"source": SOURCE, "addStackedItems": [{"catalogId": "CURRENCY_PLATINUM", "quantity": 10}]})
}

fn player(user: &str) -> MetagameAuth {
    MetagameAuth::Player(TokenClaims {
        user_id: user.to_owned(),
        epoch: None,
    })
}

async fn detect(app: &TestApp, auth: MetagameAuth, body: Value) -> Detection {
    app.db(move |conn| enforce(conn, &auth, Some(&body))).await
}

async fn ban_of(app: &TestApp, user: &str) -> Value {
    let (state, user) = (app.state.clone(), user.to_owned());
    app.db(move |conn| info(conn, &mut state.activity.lock(), &user, metagame::time::now_ms()))
        .await["ban"]
        .clone()
}

async fn banned(app: &TestApp, user: &str) -> bool {
    let user = user.to_owned();
    app.db(move |conn| ban_for(conn, Some(&user), None)).await.is_some()
}

fn users(app: &TestApp, rows: &[(&str, bool)]) {
    for (id, admin) in rows {
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?1, 0, ?2)",
            &[id, admin],
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn exact_nonempty_mutation_suspends_only_authenticated_actor_for_seven_days_and_records_bounded_evidence() {
    let app = TestApp::start().await;
    users(&app, &[("UID-attacker", false), ("UID-victim", false)]);
    let token = metagame::auth::sign_token(&app.state, "UID-attacker")
        .await
        .unwrap()
        .unwrap();
    let before = metagame::time::now_ms();
    let mut body = mutation();
    body["accountId"] = json!("UID-victim");
    body["characterId"] = json!("victim-character");
    assert_eq!(
        detect(&app, player("UID-attacker"), body).await,
        Detection {
            blocked: true,
            suspended: true
        }
    );
    assert_eq!(metagame::auth::validate_token(&app.state, &token).await.unwrap(), None);
    let ban = ban_of(&app, "UID-attacker").await;
    let expires = ban["expires_at"].as_i64().unwrap();
    let week = 7 * 86_400_000;
    assert!(expires >= before + week && expires <= metagame::time::now_ms() + week);
    assert_eq!(ban["address"], Value::Null);
    assert!(!banned(&app, "UID-victim").await);
    assert!(ban["reason"].as_str().unwrap().contains("addStackedItems=1"));
    let events = app
        .db(|conn| Ok(log(conn, true, 0.0, metagame::time::now_ms()).unwrap()))
        .await;
    assert!(
        events["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["account_id"] == "UID-attacker" && r["actor"] == "anticheat:nova-marker")
    );
    // Again: the running suspension is not extended
    detect(&app, player("UID-attacker"), mutation()).await;
    assert_eq!(ban_of(&app, "UID-attacker").await["expires_at"], expires);
}

#[tokio::test(flavor = "multi_thread")]
async fn generic_reads_empty_probes_marker_lookalikes_game_servers_and_missing_auth_are_never_detections() {
    let app = TestApp::start().await;
    let clean = Detection {
        blocked: false,
        suspended: false,
    };
    let server = MetagameAuth::GameServer(Some(TokenClaims {
        user_id: "UID-clean".into(),
        epoch: None,
    }));
    assert_eq!(detect(&app, MetagameAuth::GameServer(None), mutation()).await, clean);
    assert_eq!(detect(&app, server, mutation()).await, clean);
    let mut lookalike = mutation();
    lookalike["source"] = json!(format!("{SOURCE}-lookalike"));
    let mut legitimate = mutation();
    legitimate["source"] = json!("legitimate-game");
    for body in [
        json!({"source": SOURCE}),
        json!({"source": "local-editor:empty-connection-check", "addStackedItems": []}),
        lookalike,
        legitimate,
    ] {
        assert_eq!(detect(&app, player("UID-clean"), body.clone()).await, clean, "{body}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn all_supplied_exact_mutation_signatures_detected_existing_permanent_bans_preserved_and_admins_blocked_without_ban()
 {
    let app = TestApp::start().await;
    let sources = [
        "local-editor:catalog-cosmetics:user-request",
        "local-editor:user-request",
        "local-editor:equipment:user-request",
        "local-editor:cells-five-each:user-request",
        "local-editor:unlock-equipment:user-request",
        "local-editor:tonics:user-request",
    ];
    for (i, source) in sources.iter().enumerate() {
        let id = format!("UID-signature-{i}");
        users(&app, &[(&id, false)]);
        let found = detect(&app, player(&id), json!({"source": source, "saveInstancedItems": [{}]})).await;
        assert!(found.suspended, "{source}");
    }
    users(&app, &[("UID-permanent", false), ("UID-protected", true)]);
    let state = app.state.clone();
    app.db(move |conn| {
        let body = json!({"accountId": "UID-permanent", "reason": "Prior decision", "active": true});
        Ok(set_ban(
            conn,
            &mut state.activity.lock(),
            Some(&body),
            "owner",
            metagame::time::now_ms(),
        )
        .unwrap())
    })
    .await;
    detect(&app, player("UID-permanent"), mutation()).await;
    assert_eq!(ban_of(&app, "UID-permanent").await["expires_at"], Value::Null);
    assert_eq!(
        detect(&app, player("UID-protected"), mutation()).await,
        Detection {
            blocked: true,
            suspended: false
        }
    );
    assert!(!banned(&app, "UID-protected").await);
}
