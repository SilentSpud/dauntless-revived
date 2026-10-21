//! `matchmakingroster-integration.test.ts`: the roster as the matchmaker uses it. Node's deploy server
//! numbered its answers and could hold the first one; the scripted stand-in does the same, with a
//! session id built from the call's number.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use metagame::matchmaking::roster::{Context, QUEUED_PLAYER_LEASE_MS};
use metagame::matchmaking::{self, Decision, Entry};
use serde_json::{Value, json};

use crate::common::TestApp;
use crate::deploykit::{Answer, ScriptedDeploy};

const HUNT: &str = "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr";
const OTHER_HUNT: &str = "CR19_PlayerHunt_Pursuit_Alpha_McBeaver";
const A: &str = "UID-roster-queue-a";
const B: &str = "UID-roster-queue-b";
const START: i64 = 1_800_000_000_000;

/// The session id and port of call `number` (from 1).
fn session_of(number: usize) -> String {
    format!("00000000-0000-4000-8000-{number:012}")
}

fn port_of(number: usize) -> i64 {
    8699 + number as i64
}

struct Ri {
    app: TestApp,
    deploy: ScriptedDeploy,
    refuse: Arc<AtomicBool>,
}

async fn ri() -> Ri {
    let refuse = Arc::new(AtomicBool::new(false));
    let refusing = refuse.clone();
    let deploy = ScriptedDeploy::start(move |_, number| {
        if refusing.load(Ordering::SeqCst) {
            return Answer::json(503, json!({"error": "capacity_unavailable", "reason": "hunts"}));
        }
        Answer::json(
            200,
            json!({"host": "127.0.0.1", "port": port_of(number), "sessionId": session_of(number)}),
        )
    })
    .await;
    let calls = deploy.calls.clone();
    deploy.set_health(move || {
        let count = calls.lock().unwrap().len();
        let servers: Vec<Value> = (1..=count)
            .map(|n| json!({"id": session_of(n), "port": port_of(n)}))
            .collect();
        Answer::json(200, json!({"servers": servers}))
    });
    let app = TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", deploy.address.as_str()),
    ])
    .await;
    for user in [A, B] {
        app.run("INSERT INTO users (userId, name, notes) VALUES (?1, ?1, 0)", &[&user]);
    }
    app.state.party_clock.set_for_tests(START);
    Ri { app, deploy, refuse }
}

impl Ri {
    fn set_now(&self, ms: i64) {
        self.app.state.party_clock.set_for_tests(START + ms);
    }

    fn advance(&self, ms: i64) {
        self.app.state.party_clock.advance_for_tests(ms);
    }

    /// Holds the first deploy call's answer; later calls answer at once.
    async fn hold_first(&self) {
        self.deploy.hold(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.deploy.calls().is_empty() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        self.deploy.hold(false);
    }

    async fn join(&self, mode: &str, hunt: &str, player: &str, private: bool) -> bool {
        matchmaking::join(
            &self.app.state,
            Some(json!(mode)),
            Some(json!("")),
            Some(json!(hunt)),
            player,
            private,
        )
        .await
        .unwrap()
    }

    /// A join on its own task, so a held deploy answer doesn't stall the test.
    fn join_later(&self, mode: &str, hunt: &str, player: &str) -> tokio::task::JoinHandle<bool> {
        let (state, mode, hunt, player) = (
            self.app.state.clone(),
            mode.to_owned(),
            hunt.to_owned(),
            player.to_owned(),
        );
        tokio::spawn(async move {
            matchmaking::join(
                &state,
                Some(json!(mode)),
                Some(json!("")),
                Some(json!(hunt)),
                &player,
                false,
            )
            .await
            .unwrap()
        })
    }

    async fn status(&self, player: &str) -> Option<Entry> {
        matchmaking::check_and_update(&self.app.state, player).await
    }

    async fn ready(&self, player: &str) -> Entry {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(entry) = self.status(player).await
                && entry.ready
                && !entry.failed
            {
                return entry;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "player {player} did not receive a ready allocation"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    fn expected(&self, players: &[&str], session: &str) -> Vec<String> {
        let now = self.app.state.party_clock.now();
        let players: Vec<String> = players.iter().map(|p| (*p).to_owned()).collect();
        let context = Context {
            session_id: Some(session.to_owned()),
            ..Context::default()
        };
        self.app
            .state
            .parties
            .lock()
            .mm
            .roster
            .reconcile_expected(&players, &context, now)
    }

    fn rosters(&self) -> Vec<Value> {
        self.deploy
            .calls()
            .iter()
            .map(|c| c["ExpectedPlayers"].clone())
            .collect()
    }

    async fn poll(&self, who: &str) -> Value {
        let (state, who, now) = (self.app.state.clone(), who.to_owned(), self.app.state.party_clock.now());
        let settings = state.party_settings();
        self.app
            .db(move |conn| state.parties.lock().poll(conn, settings, &who, now))
            .await
    }

    async fn form_party(&self) {
        let party = self.poll(A).await["partyId"].as_str().unwrap().to_owned();
        self.poll(B).await;
        let (state, now, id) = (self.app.state.clone(), self.app.state.party_clock.now(), party.clone());
        let sent = self
            .app
            .db(move |conn| {
                state
                    .parties
                    .lock()
                    .invite(conn, A, Some(&json!(B)), Some(&json!(id)), now)
            })
            .await;
        assert_eq!(sent.status, 200);
        let (state, now) = (self.app.state.clone(), self.app.state.party_clock.now());
        let settings = state.party_settings();
        let accepted = self
            .app
            .db(move |conn| state.parties.lock().accept(conn, settings, B, &party, now))
            .await;
        assert_eq!(accepted.status, 200);
    }

    /// The party's current and last candidate ids
    fn party(&self, who: &str) -> (Option<String>, Option<String>) {
        let state = self.app.state.parties.lock();
        let party = state.party_id_of(who).and_then(|id| state.party(&id)).unwrap();
        (
            party.candidate().map(|c| c.candidate_id.clone()),
            party.last_candidate().map(|c| c.candidate_id.clone()),
        )
    }
}

fn is_travel(decision: &Decision) -> bool {
    matches!(decision, Decision::Travel(_))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_public_traveller_starts_without_the_player_who_stopped_polling() {
    let r = ri().await;
    assert!(r.join("ISLAND", HUNT, A, false).await);
    r.set_now(QUEUED_PLAYER_LEASE_MS + 1);
    assert!(r.join("ISLAND", HUNT, B, false).await);
    r.advance(5000);
    let entry = r.ready(B).await;
    assert_eq!(r.rosters(), [json!([B])]);
    assert_eq!(entry.port, port_of(1));
    assert!(r.status(A).await.unwrap().failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn capacity_retry_resnapshots_the_roster_and_omits_a_silent_public_member() {
    let r = ri().await;
    r.refuse.store(true, Ordering::SeqCst);
    r.join("ISLAND", HUNT, A, false).await;
    r.join("ISLAND", HUNT, B, false).await;
    r.set_now(5000);
    r.status(B).await;
    r.deploy.wait_for_calls(1).await;
    assert_eq!(r.rosters(), [json!([A, B])]);
    r.refuse.store(false, Ordering::SeqCst);
    r.advance(QUEUED_PLAYER_LEASE_MS + 1);
    r.ready(B).await;
    assert_eq!(r.rosters(), [json!([A, B]), json!([B])]);
    assert!(r.status(A).await.unwrap().failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn replacing_a_candidate_during_startup_removes_it_from_the_original_server_roster() {
    let r = ri().await;
    r.deploy.hold(true);
    r.join("ISLAND", HUNT, A, false).await;
    r.join("ISLAND", HUNT, B, false).await;
    r.set_now(5000);
    r.status(B).await;
    r.hold_first().await;
    assert_eq!(
        r.deploy.calls().len(),
        1,
        "the first allocation must still be in flight"
    );
    r.join("ISLAND", OTHER_HUNT, A, true).await;
    let replacement = r.ready(A).await;
    assert_eq!(r.deploy.calls().len(), 2);
    r.deploy.release();
    r.ready(B).await;
    assert_eq!(r.expected(&[A, B], &session_of(1)), [B]);
    assert_eq!(r.expected(&[A], &session_of(2)), [A]);
    assert_eq!(r.status(A).await.unwrap().candidate_id, replacement.candidate_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_member_returning_from_a_temporary_city_restores_the_healthy_original_hunt_membership() {
    let r = ri().await;
    r.form_party().await;
    assert!(r.join("ISLAND", HUNT, A, false).await);
    let initial = r.ready(B).await;
    assert!(is_travel(&matchmaking::decide_status(&r.app.state, A).await));
    assert!(is_travel(&matchmaking::decide_status(&r.app.state, B).await));
    // Node's entry was a live object, so its first-sent time came from the travel decision above
    let first_sent_at = r.status(B).await.unwrap().first_sent_at;
    assert!(first_sent_at.is_some());
    r.set_now(61_000);
    r.poll(B).await;
    let (current, last) = r.party(B);
    assert_eq!(current, None);
    assert_eq!(last.as_deref(), Some(initial.candidate_id.as_str()));

    assert!(r.join("CITY", "Ramsgate", B, false).await);
    assert_ne!(r.ready(B).await.candidate_id, initial.candidate_id);
    assert_eq!(r.expected(&[A, B], &session_of(1)), [A]);
    assert!(r.join("ISLAND", HUNT, B, false).await);
    let restored = r.ready(B).await;
    assert_eq!(restored.candidate_id, initial.candidate_id);
    assert_eq!(restored.session_id, Some(session_of(1)));
    assert_eq!(restored.first_sent_at, first_sent_at);
    assert_eq!(r.expected(&[A, B], &session_of(1)), [A, B]);
    assert_eq!(
        r.deploy.calls().len(),
        2,
        "returning to the healthy party hunt must not create a third server"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_member_departing_during_startup_is_omitted_from_the_original_allocation() {
    let r = ri().await;
    r.form_party().await;
    r.deploy.hold(true);
    let launching = r.join_later("ISLAND", HUNT, A);
    r.hold_first().await;
    assert_eq!(r.deploy.calls().len(), 1);
    let initial = r.status(B).await.unwrap();
    assert!(!initial.ready);
    assert!(initial.launch_deadline.is_some());

    assert!(r.join("CITY", "Ramsgate", B, false).await);
    let city = r.ready(B).await;
    assert_ne!(city.candidate_id, initial.candidate_id);
    assert!(
        matchmaking::join_candidate(&r.app.state, B, &initial.candidate_id)
            .await
            .unwrap()
            .is_none()
    );
    r.deploy.release();
    assert!(launching.await.unwrap());
    assert_eq!(r.ready(A).await.session_id, Some(session_of(1)));
    assert_eq!(r.ready(B).await.candidate_id, city.candidate_id);
    assert_eq!(r.expected(&[A, B], &session_of(1)), [A]);
    assert_eq!(
        r.deploy.calls().len(),
        2,
        "a late party launch must not overwrite the departing member's city"
    );
}
