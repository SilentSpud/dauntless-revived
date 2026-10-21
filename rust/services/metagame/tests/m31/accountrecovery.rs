//! `accountrecovery.test.ts`: the encrypted copy of a player's key, kept with ACCOUNT_KEY_RECOVERY=1
//! when the player's key is used, and read back by an admin. The Node test called the controller; here
//! the key reaches it through a player-key route and comes back through `AccountRecovery/:userId`.

use crate::common::{TestApp, req, sha256_hex};

const ADMIN_KEY: &str = "UUK_00000000000000000000000000000000000000000000adm1";

async fn app() -> TestApp {
    let app = TestApp::with_env(&[("ACCOUNT_KEY_RECOVERY", "1")]).await;
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-admin', 'Admin', 0, 1)",
        &[],
    );
    app.run(
        "INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-admin', ?1)",
        &[&sha256_hex(ADMIN_KEY)],
    );
    app
}

/// The player's key, as the admin's recovery reads it (`null`: none).
async fn recovered(app: &TestApp, user: &str) -> serde_json::Value {
    let reply = app
        .get(
            &format!("/undaunted/api/AccountRecovery/{user}"),
            req().header("x-undaunted-user-api-key", ADMIN_KEY),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    reply.json["key"].clone()
}

async fn use_key(app: &TestApp, key: &str) {
    let reply = app
        .get(
            "/undaunted/api/GetUserInfo",
            req().header("x-undaunted-user-api-key", key),
        )
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text);
}

#[tokio::test(flavor = "multi_thread")]
async fn expiry_cleanup_uses_an_index_instead_of_scanning_every_receipt() {
    let app = TestApp::start().await;
    let plan = app.rows(
        "EXPLAIN QUERY PLAN DELETE FROM inventorytransactions WHERE createdDate < ?1",
        &[&"2026-01-01"],
    );
    assert!(
        serde_json::to_string(&plan)
            .unwrap()
            .contains("inventorytransactions_created_date"),
        "{plan:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_encrypts_keys_rejects_stale_keys_and_excludes_administrators() {
    let app = app().await;
    let (user, _) = app.make_player().await;
    let key = format!("UUK_{}", "ab".repeat(24));
    app.run(
        "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
        &[&user, &sha256_hex(&key)],
    );
    use_key(&app, &key).await;
    assert_eq!(recovered(&app, &user).await, key);
    let stored = serde_json::to_string(&app.rows("SELECT * FROM accountkeyrecovery", &[])).unwrap();
    assert!(!stored.contains(&key));
    app.run("UPDATE users SET isAdmin = 1 WHERE userId = ?1", &[&user]);
    assert_eq!(recovered(&app, &user).await, serde_json::Value::Null);
    app.run("UPDATE users SET isAdmin = 0 WHERE userId = ?1", &[&user]);
    app.run(
        "UPDATE userapikeys SET keyHash = ?1 WHERE userId = ?2",
        &[&"0".repeat(64), &user],
    );
    assert_eq!(recovered(&app, &user).await, serde_json::Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_fingerprints_preserve_the_authentication_join_for_registered_and_rotated_random_keys() {
    let app = app().await;
    for bytes in [24, 32] {
        let (user, _) = app.make_player().await;
        let key = format!("UUK_{}", metagame::crypto::random_hex(bytes));
        let digest = sha256_hex(&key);
        app.run(
            "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
            &[&user, &digest],
        );
        use_key(&app, &key).await;
        let row = app.row(
            "SELECT keyHash, ciphertext FROM accountkeyrecovery WHERE userId = ?1",
            &[&user],
        );
        assert_eq!(row["keyHash"], digest);
        assert_eq!(recovered(&app, &user).await, key);
        // Using the key again keeps the same copy
        use_key(&app, &key).await;
        assert_eq!(
            app.row(
                "SELECT keyHash, ciphertext FROM accountkeyrecovery WHERE userId = ?1",
                &[&user]
            ),
            row
        );
    }
}
