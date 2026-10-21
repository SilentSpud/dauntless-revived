//! `devicesignals.test.ts`: device claims stored as keyed hashes only, and a device ban that blocks
//! matching claims while its account ban lasts. Node's `SnapshotDeviceBan` is part of a ban in the
//! Rust metagame (`includeDevice` on `SetAccountBan`); `DeviceSignalInfo` is the moderation info's
//! `devices`.

use metagame::moderation::{Refusal, Reported, info, report_signals, set_ban, signal_key};
use serde_json::{Value, json};

use crate::common::TestApp;

const SECRET: &str = "abababababababababababababababababababababababababababababababab";
const UUID: &str = "12345678-1234-5678-9abc-123456789abc";

fn tpm() -> String {
    "12".repeat(32)
}

fn now() -> i64 {
    metagame::time::now_ms()
}

fn user(app: &TestApp, id: &str) -> String {
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?1, 0, 0)",
        &[&id],
    );
    id.to_owned()
}

async fn report(app: &TestApp, secret: Option<&str>, account: &str, body: Value) -> Result<Reported, Refusal> {
    let (key, account) = (signal_key(secret), account.to_owned());
    app.state
        .store
        .write(move |conn| Ok::<_, store::Error>(report_signals(conn, key.as_deref(), &account, Some(&body), now())))
        .await
        .unwrap()
}

async fn blocked(app: &TestApp, account: &str, body: Value) -> bool {
    matches!(
        report(app, Some(SECRET), account, body).await.unwrap(),
        Reported::Blocked(_)
    )
}

async fn ban(app: &TestApp, body: Value) -> Result<Value, Refusal> {
    let state = app.state.clone();
    app.state
        .store
        .write(move |conn| {
            Ok::<_, store::Error>(set_ban(conn, &mut state.activity.lock(), Some(&body), "owner", now()))
        })
        .await
        .unwrap()
}

async fn devices(app: &TestApp, account: &str) -> Value {
    let (state, account) = (app.state.clone(), account.to_owned());
    app.db(move |conn| info(conn, &mut state.activity.lock(), &account, now()))
        .await["devices"]
        .clone()
}

/// HMAC-SHA256 under SECRET, as Node's `crypto.createHmac` gives it.
/// (`<kind>\0<value>`, with the UUID or the TPM hash as the value).
fn hmac(kind: &str, uuid: bool) -> &'static str {
    match (kind, uuid) {
        ("smbiosUuid", true) => "8cdfbe84c036894a0deb61fad1d839188a2096faa8013ba80de009da9bfb3707",
        ("tpmEkSha256", false) => "7b3bd602a94e0db1a8ccf0479b211d3ab860951abc544d8216bad9eec0c0e272",
        ("other", true) => "a241c2b517b22fb21fe3cc9bf9c20207a63d2ca959a38d352dcda09c8e608d63",
        ("other", false) => "7c473d2eb9641b741308ea75332ebba09a941c36fdd50f94f9f3ba94e61732dc",
        other => panic!("no Node HMAC for {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_input_validation_hmac_only_storage_and_replacement_without_missing_device_buckets() {
    let app = TestApp::start().await;
    let id = user(&app, "device-validation");
    let refused = [
        json!(null),
        json!([]),
        json!(42),
        json!({"accountId": "victim"}),
        json!({"smbiosUuid": ""}),
        json!({"smbiosUuid": "0".repeat(32)}),
        json!({"smbiosUuid": "00000000-0000-0000-0000-000000000000"}),
        json!({"smbiosUuid": "ffffffff-ffff-ffff-ffff-ffffffffffff"}),
        json!({"tpmEkSha256": "0".repeat(64)}),
        json!({"tpmEkSha256": "f".repeat(64)}),
        json!({"tpmEkSha256": 123}),
        json!({"tpmEkSha256": "x".repeat(64)}),
    ];
    for body in refused {
        assert_eq!(
            report(&app, Some(SECRET), &id, body.clone()).await,
            Err(Refusal::Code("invalid_device_signals")),
            "{body}"
        );
    }
    assert_eq!(
        report(&app, None, &id, json!({})).await,
        Err(Refusal::Code("device_signals_unavailable"))
    );
    assert_eq!(
        report(&app, Some("ab"), &id, json!({})).await,
        Err(Refusal::Code("device_signals_unavailable"))
    );
    let tpm = tpm();
    let body = json!({"smbiosUuid": UUID.to_uppercase(), "tpmEkSha256": tpm});
    assert_eq!(report(&app, Some(SECRET), &id, body).await, Ok(Reported::Stored));
    let rows = app.rows("SELECT * FROM account_device_signals WHERE account_id = ?1", &[&id]);
    assert_eq!(rows.len(), 2);
    let text = serde_json::to_string(&rows).unwrap();
    assert!(!text.contains(UUID) && !text.contains(&tpm));
    for row in &rows {
        let kind = row["kind"].as_str().unwrap();
        let uuid = kind == "smbiosUuid";
        assert_eq!(row["signal_hash"], hmac(kind, uuid));
        assert_ne!(row["signal_hash"], hmac("other", uuid));
    }
    let shown = devices(&app, &id).await;
    assert_eq!(
        (shown["verified"].clone(), shown["claimed"].clone()),
        (json!(false), json!(true))
    );
    assert_eq!(shown["signals"][0]["hash"].as_str().unwrap().len(), 12);
    report(&app, Some(SECRET), &id, json!({"smbiosUuid": UUID}))
        .await
        .unwrap();
    assert_eq!(devices(&app, &id).await["signals"].as_array().unwrap().len(), 1);
    report(&app, Some(SECRET), &id, json!({})).await.unwrap();
    assert_eq!(devices(&app, &id).await["signals"], json!([]));
    let device_ban = json!({"accountId": id, "reason": "r", "active": true, "includeDevice": true});
    assert_eq!(ban(&app, device_ban).await, Err(Refusal::Code("device_not_observed")));
    assert_eq!(
        report(&app, Some(SECRET), "does-not-exist", json!({"smbiosUuid": UUID})).await,
        Err(Refusal::Code("account_not_found"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_explicit_snapshots_block_matching_claims_expiry_revocation_missing_claims_and_replacement_are_safe() {
    let app = TestApp::start().await;
    let (source, other) = (user(&app, "device-source"), user(&app, "device-other"));
    let tpm = tpm();
    report(
        &app,
        Some(SECRET),
        &source,
        json!({"smbiosUuid": UUID, "tpmEkSha256": tpm}),
    )
    .await
    .unwrap();
    let verified = |device: bool, active: bool, reason: &str| json!({"accountId": source, "reason": reason, "active": active, "durationHours": 168, "includeDevice": device});
    ban(&app, verified(false, true, "manual verified evidence"))
        .await
        .unwrap();
    assert!(!blocked(&app, &other, json!({"smbiosUuid": UUID})).await);
    ban(&app, verified(true, true, "manual verified evidence"))
        .await
        .unwrap();
    assert_eq!(devices(&app, &source).await["enabled"], true);
    assert_eq!(
        report(&app, Some(SECRET), &other, json!({"smbiosUuid": UUID})).await,
        Ok(Reported::Blocked("manual verified evidence".into()))
    );
    assert!(blocked(&app, &other, json!({"tpmEkSha256": tpm})).await);
    assert!(!blocked(&app, &other, json!({})).await);
    assert!(!blocked(&app, &other, json!({"tpmEkSha256": "34".repeat(32)})).await);
    // Later claims must not erase the moderator's snapshot
    report(&app, Some(SECRET), &source, json!({})).await.unwrap();
    assert!(blocked(&app, &other, json!({"smbiosUuid": UUID})).await);
    app.run(
        "UPDATE account_bans SET expires_at = ?1 WHERE account_id = ?2",
        &[&(now() - 1), &source],
    );
    assert!(!blocked(&app, &other, json!({"smbiosUuid": UUID})).await);
    ban(&app, verified(false, true, "manual verified evidence"))
        .await
        .unwrap();
    ban(&app, json!({"accountId": source, "reason": "appeal", "active": false}))
        .await
        .unwrap();
    assert!(!blocked(&app, &other, json!({"smbiosUuid": UUID})).await);
    assert_eq!(devices(&app, &source).await["enabled"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_and_future_claims_cannot_be_snapshotted_failure_preserves_the_prior_snapshot() {
    let app = TestApp::start().await;
    let id = user(&app, "device-stale");
    report(&app, Some(SECRET), &id, json!({"smbiosUuid": UUID}))
        .await
        .unwrap();
    let device_ban = json!({"accountId": id, "reason": "r", "active": true, "includeDevice": true});
    ban(&app, device_ban.clone()).await.unwrap();
    app.run(
        "UPDATE account_device_signals SET observed_at = ?1 WHERE account_id = ?2",
        &[&(now() - 86_400_000), &id],
    );
    assert_eq!(
        ban(&app, device_ban.clone()).await,
        Err(Refusal::Code("device_not_observed"))
    );
    assert_eq!(devices(&app, &id).await["enabled"], true);
    app.run(
        "UPDATE account_device_signals SET observed_at = ?1 WHERE account_id = ?2",
        &[&(now() + 86_400_000), &id],
    );
    assert_eq!(ban(&app, device_ban).await, Err(Refusal::Code("device_not_observed")));
    ban(
        &app,
        json!({"accountId": id, "reason": "r", "active": true, "includeDevice": false}),
    )
    .await
    .unwrap();
    assert_eq!(devices(&app, &id).await["enabled"], false);
}
