#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The party rules that depend on time, which the parity scripts can't wait for: members swept after
//! [`MEMBER_TIMEOUT_MS`], invites expiring, the decline cooldown, leader eviction and idle parties
//! being forgotten.

use metagame::db::SCHEMA;
use metagame::party::{DECLINE_COOLDOWN_MS, IDLE_PARTY_MS, INVITE_TTL_MS, MEMBER_TIMEOUT_MS, Settings, State};
use serde_json::{Value, json};
use store::{Options, Store};

const SETTINGS: Settings = Settings {
    display_names: true,
    solo_stub: true,
};
const T0: i64 = 1_800_000_000_000;

async fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open(&dir.path().join("undaunted.db"), &Options::default(), &SCHEMA).unwrap();
    db.write(|conn| {
        use diesel::connection::SimpleConnection;
        conn.batch_execute(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-a', 'Ada', 0, 0), ('UID-b', 'Bo', 0, 0), ('UID-c', 'Cy', 0, 0)",
        )
        .map_err(store::Error::from)
    })
    .await
    .unwrap();
    (dir, db)
}

/// Runs `f` with a connection and the state, as the routes do.
async fn with<T: Send + 'static>(
    db: &Store,
    state: &std::sync::Arc<std::sync::Mutex<State>>,
    f: impl FnOnce(&mut store::Connection, &mut State) -> T + Send + 'static,
) -> T {
    let state = state.clone();
    db.read(move |conn| Ok::<_, store::Error>(f(conn, &mut state.lock().unwrap())))
        .await
        .unwrap()
}

/// UID-a invites UID-b at `at`, and UID-b accepts.
async fn party_of_two(db: &Store, state: &std::sync::Arc<std::sync::Mutex<State>>, at: i64) -> String {
    with(db, state, move |conn, s| {
        let id = s.poll(conn, SETTINGS, "UID-a", at).unwrap()["partyId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            s.invite(conn, "UID-a", Some(&json!("UID-b")), None, at).unwrap().status,
            200
        );
        assert_eq!(s.accept(conn, SETTINGS, "UID-b", &id, at).unwrap().status, 200);
        id
    })
    .await
}

fn members(reply: &Value) -> Vec<&str> {
    reply["playerStates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["playerId"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_member_is_swept_and_a_silent_leader_can_be_evicted() {
    let (_dir, db) = store().await;
    let state = std::sync::Arc::new(std::sync::Mutex::new(State::default()));
    party_of_two(&db, &state, T0).await;
    // Within the timeout the leader stays, and the eviction is ignored
    let (kept, evicted) = with(&db, &state, |conn, s| {
        s.evict_leader("UID-b", "UID-a", T0 + MEMBER_TIMEOUT_MS);
        let kept = s.poll(conn, SETTINGS, "UID-b", T0 + MEMBER_TIMEOUT_MS).unwrap();
        s.evict_leader("UID-b", "UID-a", T0 + MEMBER_TIMEOUT_MS + 1);
        let evicted = s.poll(conn, SETTINGS, "UID-b", T0 + MEMBER_TIMEOUT_MS + 1).unwrap();
        (kept, evicted)
    })
    .await;
    assert_eq!(members(&kept), ["UID-a", "UID-b"]);
    assert_eq!(members(&evicted), ["UID-b"]);
    assert_eq!(evicted["leaderPlayerId"], "UID-b");

    // A member who stops polling is swept by the next poll of another
    let (_dir, db) = store().await;
    let state = std::sync::Arc::new(std::sync::Mutex::new(State::default()));
    party_of_two(&db, &state, T0).await;
    let swept = with(&db, &state, |conn, s| {
        s.poll(conn, SETTINGS, "UID-a", T0 + MEMBER_TIMEOUT_MS + 1).unwrap()
    })
    .await;
    assert_eq!(members(&swept), ["UID-a"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn invites_expire_and_a_decline_holds_off_the_sender_for_a_while() {
    let (_dir, db) = store().await;
    let state = std::sync::Arc::new(std::sync::Mutex::new(State::default()));
    let (fresh, expired) = with(&db, &state, |conn, s| {
        s.invite(conn, "UID-a", Some(&json!("UID-b")), None, T0).unwrap();
        let fresh = s.list_invites(conn, "UID-b", T0 + INVITE_TTL_MS).unwrap();
        let expired = s.list_invites(conn, "UID-b", T0 + INVITE_TTL_MS + 1).unwrap();
        (fresh, expired)
    })
    .await;
    assert_eq!(fresh["invitations"].as_array().unwrap().len(), 1);
    assert_eq!(expired["invitations"], json!([]));

    let statuses = with(&db, &state, |conn, s| {
        let at = T0 + INVITE_TTL_MS + 2;
        s.invite(conn, "UID-a", Some(&json!("UID-c")), None, at).unwrap();
        s.decline("UID-c", Some(&json!({"sendingPlayerId": "UID-a"})), at);
        [
            s.invite(conn, "UID-a", Some(&json!("UID-c")), None, at + DECLINE_COOLDOWN_MS - 1)
                .unwrap()
                .status,
            s.invite(conn, "UID-a", Some(&json!("UID-c")), None, at + DECLINE_COOLDOWN_MS)
                .unwrap()
                .status,
        ]
    })
    .await;
    assert_eq!(statuses, [409, 200]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_party_of_one_is_forgotten() {
    let (_dir, db) = store().await;
    let state = std::sync::Arc::new(std::sync::Mutex::new(State::default()));
    let (first, kept, later) = with(&db, &state, |conn, s| {
        let first = s.poll(conn, SETTINGS, "UID-a", T0).unwrap()["partyId"].clone();
        // Another player's call runs the sweep; UID-a was seen within the idle time
        s.poll(conn, SETTINGS, "UID-b", T0 + IDLE_PARTY_MS).unwrap();
        let kept = s.party_id_of("UID-a");
        s.poll(conn, SETTINGS, "UID-b", T0 + IDLE_PARTY_MS + 61_000).unwrap();
        let later = s.party_id_of("UID-a");
        (first, kept, later)
    })
    .await;
    assert_eq!(kept.as_deref(), first.as_str());
    assert_eq!(later, None);
}
