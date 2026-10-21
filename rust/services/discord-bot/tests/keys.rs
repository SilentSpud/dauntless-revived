#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The invite and link guarantees (`keys.test.mjs`, `delivery.test.mjs`): one code per user, one DM
//! per code, nothing issued or sent twice across crashes, ambiguous answers or failing disks.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use discord_bot::keys::{
    Backend, BoxFuture, DM_DISABLED, Failure, Found, Keys, NO_MUTUAL_GUILD, Outcome, Row, save_state_with,
};
use serde_json::{Value, json};

const USER: &str = "123456789012345678";

type Identity = Arc<dyn Fn(&str) -> Option<(String, String)> + Send + Sync>;

#[derive(Default)]
struct Fake {
    rows: Mutex<HashMap<String, Row>>,
    links: Mutex<HashMap<String, String>>,
    creates: AtomicUsize,
    create_fails_after_write: Mutex<bool>,
    link_fails: Mutex<bool>,
    identity: Mutex<Option<Identity>>,
}

impl Fake {
    fn uses(&self, code: &str, uses: i64) {
        self.rows.lock().unwrap().insert(
            code.into(),
            Row {
                uses_remaining: json!(uses),
                infinite_uses: false,
            },
        );
    }
}

impl Backend for Fake {
    fn link_account(&self, discord_id: &str, user_id: &str) -> BoxFuture<'_, Result<Value, Failure>> {
        let result = if *self.link_fails.lock().unwrap() {
            Err(Failure::new("disk full"))
        } else {
            let mut links = self.links.lock().unwrap();
            Ok(match links.get(discord_id) {
                Some(linked) if linked != user_id => json!({ "status": "discord_already_linked" }),
                _ if links.iter().any(|(other, uid)| other != discord_id && uid == user_id) => {
                    json!({ "status": "account_already_linked" })
                }
                _ => {
                    links.insert(discord_id.into(), user_id.into());
                    json!({ "status": "linked" })
                }
            })
        };
        Box::pin(async move { result })
    }

    fn linked_account(&self, discord_id: &str) -> BoxFuture<'_, Result<Option<Value>, Failure>> {
        let found = self
            .links
            .lock()
            .unwrap()
            .get(discord_id)
            .map(|uid| json!({ "userId": uid }));
        Box::pin(async move { Ok(found) })
    }

    fn find(&self, code: &str) -> BoxFuture<'_, Result<Option<Row>, Failure>> {
        let row = self.rows.lock().unwrap().get(code).cloned();
        Box::pin(async move { Ok(row) })
    }

    fn create(&self, code: &str) -> BoxFuture<'_, Result<(), Failure>> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        self.uses(code, 1);
        let fail = *self.create_fails_after_write.lock().unwrap();
        Box::pin(async move {
            if fail {
                Err(Failure::new("timeout after write"))
            } else {
                Ok(())
            }
        })
    }

    fn identity(&self, key: &str) -> BoxFuture<'_, Result<Option<(String, String)>, Failure>> {
        let identity = self.identity.lock().unwrap().clone();
        let found = identity.map(|f| f(key)).unwrap_or(None);
        Box::pin(async move { Ok(found) })
    }
}

type Saved = Arc<Mutex<Option<Value>>>;

fn saver(saved: Saved) -> Arc<dyn Fn(Value) -> BoxFuture<'static, Result<(), Failure>> + Send + Sync> {
    Arc::new(move |value| {
        *saved.lock().unwrap() = Some(value);
        Box::pin(async { Ok(()) })
    })
}

fn failing() -> Arc<dyn Fn(Value) -> BoxFuture<'static, Result<(), Failure>> + Send + Sync> {
    Arc::new(|_| Box::pin(async { Err(Failure::new("disk full")) }))
}

struct Setup {
    api: Arc<Fake>,
    saved: Saved,
    keys: Keys,
}

fn setup_with(state: Value, generate: &'static str) -> Setup {
    let api = Arc::new(Fake::default());
    let saved: Saved = Arc::default();
    let keys = Keys::new(
        state,
        saver(saved.clone()),
        api.clone(),
        Arc::new(move || generate.to_owned()),
    );
    Setup { api, saved, keys }
}

fn setup() -> Setup {
    setup_with(json!({ "version": 1, "users": {} }), "DR-test")
}

fn restarted(s: &Setup) -> Keys {
    let state = s.saved.lock().unwrap().clone().unwrap();
    Keys::new(
        state,
        saver(s.saved.clone()),
        s.api.clone(),
        Arc::new(|| "DR-other".into()),
    )
}

type Sends = Arc<AtomicUsize>;

fn send(sends: Sends) -> discord_bot::keys::Send_<'static> {
    Box::new(move |_| {
        sends.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Some("message".into())) })
    })
}

fn rejected(code: i64) -> discord_bot::keys::Send_<'static> {
    Box::new(move |_| Box::pin(async move { Err(Failure::discord(code)) }))
}

fn timeout() -> discord_bot::keys::Send_<'static> {
    Box::new(|_| Box::pin(async { Err(Failure::new("timeout")) }))
}

fn history(answer: Result<Option<Option<Found>>, Failure>) -> Option<discord_bot::keys::Reconcile<'static>> {
    Some(Box::new(move |_| Box::pin(async move { answer })))
}

async fn status(keys: &Keys, user: &str, claim: bool) -> Outcome {
    keys.run(user, claim).await.unwrap()
}

#[tokio::test]
async fn concurrent_claims_reuse_one_single_use_code() {
    let s = setup();
    let results = futures_util::future::join_all((0..20).map(|_| s.keys.run(USER, true))).await;
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
    assert!(results.into_iter().all(|r| r.unwrap()
        == Outcome {
            status: "ready".into(),
            code: Some("DR-test".into())
        }));
}

#[tokio::test]
async fn status_never_mints_replacements() {
    let s = setup();
    assert_eq!(status(&s.keys, USER, false).await, Outcome::of("none"));
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 0);
    status(&s.keys, USER, true).await;
    s.api.uses("DR-test", 0);
    assert_eq!(status(&s.keys, USER, false).await, Outcome::of("redeemed"));
    assert_eq!(status(&s.keys, USER, true).await, Outcome::of("redeemed"));
    s.api.rows.lock().unwrap().clear();
    assert_eq!(status(&s.keys, USER, true).await, Outcome::of("revoked"));
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_uncertain_backend_answer_is_recovered_without_another_code() {
    let s = setup();
    *s.api.create_fails_after_write.lock().unwrap() = true;
    assert!(s.keys.run(USER, true).await.is_err());
    *s.api.create_fails_after_write.lock().unwrap() = false;
    assert_eq!(status(&s.keys, USER, true).await.code.as_deref(), Some("DR-test"));
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failed_state_write_never_permits_issuance() {
    let s = setup();
    s.keys.set_save(failing());
    assert!(s.keys.run(USER, true).await.is_err());
    assert!(s.keys.run(USER, true).await.is_err());
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 0);
    assert_eq!(s.keys.state.lock().await["users"], json!({}));
}

#[tokio::test]
async fn an_issued_invite_survives_a_failed_final_commit_and_a_restart() {
    let s = setup();
    let saves = Arc::new(AtomicUsize::new(0));
    let durable: Saved = Arc::default();
    let (n, d) = (saves.clone(), durable.clone());
    s.keys.set_save(Arc::new(move |value| {
        let second = n.fetch_add(1, Ordering::SeqCst) + 1 == 2;
        if !second {
            *d.lock().unwrap() = Some(value);
        }
        Box::pin(async move {
            if second {
                Err(Failure::new("rename failed"))
            } else {
                Ok(())
            }
        })
    }));
    assert!(s.keys.run(USER, true).await.is_err());
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
    let state = durable.lock().unwrap().clone().unwrap();
    let again = Keys::new(
        state,
        saver(Arc::default()),
        s.api.clone(),
        Arc::new(|| panic!("must reuse the pending invite")),
    );
    assert_eq!(
        status(&again, USER, true).await,
        Outcome {
            status: "ready".into(),
            code: Some("DR-test".into())
        }
    );
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn linking_verifies_ownership_stores_identity_only_and_refuses_conflicts() {
    let s = setup();
    let secret = "UUK_test-private-launcher-key";
    *s.api.identity.lock().unwrap() = Some(Arc::new(move |key| {
        (key == secret).then(|| ("UID-account".into(), "Slayer".into()))
    }));
    assert_eq!(
        s.keys.link(USER, "invalid-private-key").await.unwrap(),
        Outcome::of("invalid_key")
    );
    assert_eq!(s.keys.link(USER, secret).await.unwrap(), Outcome::of("linked"));
    assert!(
        s.saved.lock().unwrap().is_none(),
        "linking writes nothing to the state file"
    );
    assert!(!s.keys.state.lock().await.to_string().contains(secret));
    assert_eq!(status(&s.keys, USER, false).await, Outcome::of("linked"));
    assert_eq!(status(&s.keys, USER, true).await, Outcome::of("linked"));
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.keys.link("223456789012345678", secret).await.unwrap(),
        Outcome::of("account_already_linked")
    );
    *s.api.identity.lock().unwrap() = Some(Arc::new(|_| Some(("UID-other".into(), "Other".into()))));
    assert_eq!(
        s.keys.link(USER, secret).await.unwrap(),
        Outcome::of("discord_already_linked")
    );
}

#[tokio::test]
async fn invites_and_bad_input_never_reach_identity_checks() {
    let s = setup();
    *s.api.identity.lock().unwrap() = Some(Arc::new(|_| {
        panic!("a Join invite must never be sent as an account key")
    }));
    assert_eq!(
        s.keys
            .link(USER, "dauntless-revived://join?v=2&code=OLD-CODE")
            .await
            .unwrap(),
        Outcome::of("invite_not_key")
    );
    for input in ["Key: firstkey\nKey: secondkey", "bad key value"] {
        assert_eq!(
            s.keys.link(USER, input).await.unwrap(),
            Outcome::of("invalid_key_format")
        );
    }
    *s.api.identity.lock().unwrap() = Some(Arc::new(|_| None));
    assert_eq!(
        s.keys.link(USER, "DR-old-registration").await.unwrap(),
        Outcome::of("invite_not_key")
    );
    assert!(s.api.links.lock().unwrap().is_empty());
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_failed_link_leaves_the_account_unlinked() {
    let s = setup();
    *s.api.identity.lock().unwrap() = Some(Arc::new(|_| Some(("UID-account".into(), "Slayer".into()))));
    *s.api.link_fails.lock().unwrap() = true;
    assert_eq!(
        s.keys.link(USER, "private-key-for-testing").await.unwrap_err().message,
        "disk full"
    );
    assert!(s.keys.state.lock().await.get("links").is_none());
}

#[tokio::test]
async fn legacy_links_migrate_idempotently() {
    let s = setup_with(
        json!({ "version": 1, "users": {}, "links": { USER: { "userId": "UID-account", "username": "Slayer" } } }),
        "DR-test",
    );
    s.keys.migrate_links().await.unwrap();
    s.keys.migrate_links().await.unwrap();
    assert_eq!(s.api.links.lock().unwrap().len(), 1);
    assert_eq!(status(&s.keys, USER, true).await, Outcome::of("linked"));
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 0);
}

// ---- delivery ----

#[tokio::test]
async fn concurrent_claims_and_a_restart_send_one_dm() {
    let s = setup();
    let sends: Sends = Arc::default();
    let results =
        futures_util::future::join_all((0..10).map(|_| s.keys.deliver(USER, send(sends.clone()), None))).await;
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        results.iter().filter(|r| r.as_ref().unwrap().status == "sent").count(),
        1
    );
    assert_eq!(
        restarted(&s).deliver(USER, send(sends.clone()), None).await.unwrap(),
        Outcome::of("already_sent")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_ambiguous_discord_answer_is_never_retried() {
    let s = setup();
    assert_eq!(
        s.keys.deliver(USER, timeout(), None).await.unwrap(),
        Outcome::of("delivery_uncertain")
    );
    let sends: Sends = Arc::default();
    assert_eq!(
        restarted(&s).deliver(USER, send(sends.clone()), None).await.unwrap(),
        Outcome::of("delivery_uncertain")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_explicit_rejection_retries_the_same_code_only() {
    for code in [DM_DISABLED, NO_MUTUAL_GUILD] {
        let s = setup();
        let expected = if code == DM_DISABLED {
            "dm_disabled"
        } else {
            "no_mutual_guild"
        };
        assert_eq!(
            s.keys.deliver(USER, rejected(code), None).await.unwrap(),
            Outcome::of(expected)
        );
        assert_eq!(s.keys.state.lock().await["users"][USER]["delivery"], "unsent");
        let sends: Sends = Arc::default();
        assert_eq!(
            s.keys.deliver(USER, send(sends), None).await.unwrap(),
            Outcome::of("sent")
        );
        assert_eq!(s.keys.state.lock().await["users"][USER]["code"], "DR-test");
        assert_eq!(s.api.rows.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn legacy_issued_codes_are_not_resent() {
    let s = setup_with(
        json!({ "version": 1, "users": { USER: { "code": "DR-old", "pending": false } } }),
        "DR-test",
    );
    s.api.uses("DR-old", 1);
    let sends: Sends = Arc::default();
    assert_eq!(
        s.keys.deliver(USER, send(sends.clone()), None).await.unwrap(),
        Outcome::of("already_sent")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_failed_reservation_prevents_the_dm() {
    let s = setup();
    s.keys.run(USER, true).await.unwrap();
    s.keys.set_save(failing());
    let sends: Sends = Arc::default();
    assert!(s.keys.deliver(USER, send(sends.clone()), None).await.is_err());
    assert_eq!(sends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_reserved_delivery_is_reconciled_from_the_dm_history() {
    // Found in the history: no second DM
    let s = setup();
    s.keys.deliver(USER, timeout(), None).await.unwrap();
    let sends: Sends = Arc::default();
    let found = Found {
        id: "123".into(),
        created_at: "2026-10-04T04:00:00Z".into(),
    };
    assert_eq!(
        s.keys
            .deliver(USER, send(sends.clone()), history(Ok(Some(Some(found)))))
            .await
            .unwrap(),
        Outcome::of("already_sent")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(s.keys.state.lock().await["users"][USER]["messageId"], "123");

    // A complete history proving no message: the same reserved code is sent
    let s = setup();
    s.keys.deliver(USER, timeout(), None).await.unwrap();
    assert_eq!(
        s.keys
            .deliver(USER, send(sends.clone()), history(Ok(Some(None))))
            .await
            .unwrap(),
        Outcome::of("sent")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(s.api.rows.lock().unwrap().len(), 1);

    // An incomplete history proves nothing
    let s = setup();
    s.keys.deliver(USER, timeout(), None).await.unwrap();
    assert_eq!(
        s.keys
            .deliver(USER, send(sends.clone()), history(Ok(None)))
            .await
            .unwrap(),
        Outcome::of("delivery_uncertain")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 1);

    // A blocked history lookup keeps the reservation and explains DM settings
    let s = setup();
    s.keys.run(USER, true).await.unwrap();
    s.keys.state.lock().await["users"][USER]["delivery"] = json!("reserved");
    let result = s.keys.deliver(
        USER,
        Box::new(|_| panic!("must not duplicate")),
        history(Err(Failure::discord(DM_DISABLED))),
    );
    assert_eq!(result.await.unwrap(), Outcome::of("dm_disabled"));
    assert_eq!(s.keys.state.lock().await["users"][USER]["delivery"], "reserved");
}

fn private_reply(seen: Arc<Mutex<Vec<String>>>) -> discord_bot::keys::Reply<'static> {
    Box::new(move |code| {
        seen.lock().unwrap().push(code);
        Box::pin(async { Ok(()) })
    })
}

#[tokio::test]
async fn private_claims_recover_uncertain_dms_with_the_same_invite() {
    let s = setup();
    let first = status(&s.keys, USER, true).await;
    s.keys.state.lock().await["users"][USER]["delivery"] = json!("reserved");
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (a, b) = tokio::join!(
        s.keys.deliver_private(USER, private_reply(seen.clone())),
        s.keys.deliver_private(USER, private_reply(seen.clone()))
    );
    a.unwrap();
    b.unwrap();
    let code = first.code.unwrap();
    assert_eq!(*seen.lock().unwrap(), [code.clone(), code.clone()]);
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
    assert_eq!(s.keys.state.lock().await["users"][USER]["deliveryChannel"], "ephemeral");
    s.api.uses(&code, 0);
    let redeemed = s
        .keys
        .deliver_private(USER, Box::new(|_| panic!("must not show a redeemed invite")));
    assert_eq!(redeemed.await.unwrap(), Outcome::of("redeemed"));
}

#[tokio::test]
async fn a_failed_private_reply_keeps_the_invite_for_a_retry() {
    let s = setup();
    let failed = s.keys.deliver_private(
        USER,
        Box::new(|_| Box::pin(async { Err(Failure::new("response failed")) })),
    );
    assert!(failed.await.is_err());
    assert_eq!(s.keys.state.lock().await["users"][USER]["delivery"], "unsent");
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    s.keys.deliver_private(USER, private_reply(seen.clone())).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), ["DR-test"]);
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_private_recipient_gets_the_same_invite_once_by_dm() {
    let s = setup();
    s.keys
        .deliver_private(USER, private_reply(Arc::default()))
        .await
        .unwrap();
    let sends: Sends = Arc::default();
    assert_eq!(
        s.keys.deliver(USER, send(sends.clone()), None).await.unwrap(),
        Outcome::of("sent")
    );
    assert_eq!(
        s.keys.deliver(USER, send(sends.clone()), None).await.unwrap(),
        Outcome::of("already_sent")
    );
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(s.keys.state.lock().await["users"][USER]["deliveryChannel"], "dm");

    // An ambiguous DM after the private reply stays reserved and is reconciled
    let s = setup();
    s.keys
        .deliver_private(USER, private_reply(Arc::default()))
        .await
        .unwrap();
    assert_eq!(
        s.keys.deliver(USER, timeout(), None).await.unwrap(),
        Outcome::of("delivery_uncertain")
    );
    let found = Found {
        id: "existing".into(),
        created_at: "now".into(),
    };
    let reconciled = s.keys.deliver(
        USER,
        Box::new(|_| panic!("must not send again")),
        history(Ok(Some(Some(found)))),
    );
    assert_eq!(reconciled.await.unwrap(), Outcome::of("already_sent"));
    assert_eq!(s.api.creates.load(Ordering::SeqCst), 1);
}

// ---- the state file ----

#[tokio::test]
async fn windows_retries_a_denied_replacement_and_keeps_the_old_state_until_then() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keys.json");
    std::fs::write(&file, "old").unwrap();
    let calls = AtomicUsize::new(0);
    let waits: Mutex<Vec<u64>> = Mutex::default();
    let state = json!({ "version": 1, "users": {} });
    save_state_with(
        &file,
        &state,
        true,
        |from, to| {
            assert_eq!(std::fs::read_to_string(to).unwrap(), "old");
            if calls.fetch_add(1, Ordering::SeqCst) < 3 {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            std::fs::rename(from, to)
        },
        |ms| {
            waits.lock().unwrap().push(ms);
            Box::pin(async {})
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(*waits.lock().unwrap(), [50, 100, 200]);
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap(),
        state
    );

    for (windows, kind, expected) in [
        (true, std::io::ErrorKind::PermissionDenied, 7),
        (true, std::io::ErrorKind::StorageFull, 1),
        (false, std::io::ErrorKind::PermissionDenied, 1),
    ] {
        let calls = AtomicUsize::new(0);
        let result = save_state_with(
            &file,
            &json!({ "version": 1, "users": { "pending": {} } }),
            windows,
            |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::from(kind))
            },
            |_| Box::pin(async {}),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), expected, "{windows} {kind:?}");
        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap(),
            state
        );
    }
}

#[test]
fn a_state_file_that_cannot_be_read_stops_issuance() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keys.json");
    assert_eq!(
        discord_bot::keys::load_state(&file).unwrap(),
        json!({ "version": 1, "users": {} })
    );
    for bad in [
        "not json",
        "{\"version\":2,\"users\":{}}",
        "{\"version\":1,\"users\":[]}",
        "{\"version\":1,\"users\":{},\"links\":{\"x\":{}}}",
    ] {
        std::fs::write(&file, bad).unwrap();
        assert!(
            discord_bot::keys::load_state(&file).unwrap_err().contains("refusing"),
            "{bad}"
        );
    }
    let good = json!({ "version": 1, "users": { USER: { "code": "DR-a", "pending": false } }, "links": { USER: { "userId": "UID-a", "username": "S" } } });
    std::fs::write(&file, good.to_string()).unwrap();
    assert_eq!(discord_bot::keys::load_state(&file).unwrap(), good);
}
