//! `discordstats.test.ts`: what became of the bot's invite codes, as counts only, for the owner.

use metagame::discordstats::summarize;
use serde_json::json;

use crate::common::{TestApp, req, sha256_hex};

#[test]
fn bot_statistics_distinguish_claims_issuance_and_redemption_without_exposing_codes_or_identities() {
    let entries = [
        ("unused", false),
        ("redeemed", false),
        ("pending", true),
        ("revoked", false),
        ("uncertain", true),
    ];
    let users: serde_json::Map<String, serde_json::Value> = entries
        .iter()
        .enumerate()
        .map(|(i, (code, pending))| {
            (
                (12_345_678_901_234_567u64 + i as u64).to_string(),
                json!({"code": code, "pending": pending}),
            )
        })
        .collect();
    let state = json!({"version": 1, "users": users});
    let rows: Vec<(String, i64, bool)> = ["unused", "redeemed", "uncertain", "not-a-bot-code"]
        .iter()
        .map(|code| ((*code).to_owned(), if *code == "redeemed" { 0 } else { 1 }, false))
        .collect();
    assert_eq!(
        summarize(&state, &rows),
        Ok(json!({"claims": 5, "issued": 4, "unused": 2, "redeemed": 1, "pending": 1, "revoked": 1}))
    );
    assert!(
        summarize(
            &json!({"version": 1, "users": {"bad": {"code": "secret", "pending": false}}}),
            &rows
        )
        .is_err()
    );
}

async fn owner_app(state_file: Option<&str>) -> TestApp {
    let app = match state_file {
        Some(file) => TestApp::with_env(&[("DISCORD_KEY_STATE_FILE", file)]).await,
        None => TestApp::start().await,
    };
    for (id, admin) in [("owner", 1), ("player", 0)] {
        let user = format!("UID-{id}");
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, ?3)",
            &[&user, &id, &admin],
        );
        app.run(
            "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
            &[&user, &sha256_hex(&format!("test-key-{id}"))],
        );
    }
    app
}

async fn stats(app: &TestApp) -> serde_json::Value {
    let reply = app
        .get(
            "/undaunted/api/DiscordKeyStats",
            req().header("x-undaunted-user-api-key", "test-key-owner"),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    reply.json
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_bot_state_remains_unavailable_while_canonical_linked_account_count_stays_readable() {
    let app = owner_app(Some("missing-bot-state-for-test.json")).await;
    let result = stats(&app).await;
    assert_eq!(result["bot"], serde_json::Value::Null);
    assert_eq!(result["linkedAccounts"], 0);
    assert!(result["error"].is_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn statistics_join_real_sqlite_invite_column_names_and_return_only_counts() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    std::fs::write(
        &file,
        json!({"version": 1, "users": {"12345678901234567": {"code": "secret-invite", "pending": false}}}).to_string(),
    )
    .unwrap();
    let app = owner_app(Some(file.to_str().unwrap())).await;
    app.exec("INSERT INTO invitecodes (invitecode, usesRemaining, infiniteUses) VALUES ('secret-invite', 0, 0)");
    let result = stats(&app).await;
    assert_eq!(
        (result["bot"]["redeemed"].clone(), result["bot"]["revoked"].clone()),
        (json!(1), json!(0))
    );
    assert_eq!(result["error"], serde_json::Value::Null);
    let text = result.to_string();
    assert!(!text.contains("secret-invite"));
    assert!(!text.contains("12345678901234567"));
}

#[tokio::test(flavor = "multi_thread")]
async fn discord_key_statistics_are_owner_only_and_refuse_proxied_owner_requests() {
    let app = owner_app(None).await;
    let path = "/undaunted/api/DiscordKeyStats";
    assert_eq!(app.get(path, req()).await.status, 401);
    assert_eq!(
        app.get(path, req().header("x-undaunted-user-api-key", "test-key-player"))
            .await
            .status,
        403
    );
    let proxied = req()
        .header("x-undaunted-user-api-key", "test-key-owner")
        .header("x-forwarded-for", "203.0.113.1");
    assert_eq!(app.get(path, proxied).await.status, 403);
    assert_eq!(
        app.get(path, req().header("x-undaunted-user-api-key", "test-key-owner"))
            .await
            .status,
        200
    );
}
