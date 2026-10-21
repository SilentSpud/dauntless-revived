//! `matchmakingrecovery.test.ts`: deploy timeouts, late answers, parks, health snapshots and capacity
//! waits. Node mocked `fetch` and `setTimeout` and set the party clock; here the deploy server is the
//! scripted stand-in (it can hold an answer or its body, and serves the health snapshot), deadlines
//! that Rust keeps on the party clock move with it, and the deploy timeout and the 1.5 s join wait are
//! real waits. A looped Node case is one helper here, run once per value.

use metagame::matchmaking::{self, Decision, Entry};
use serde_json::{Value, json};

use crate::common::TestApp;
use crate::deploykit::{Answer, ScriptedDeploy};

const HUNT: &str = "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr";
const CITY: &str = "ShatteredIsles_ReturnToRamsgate";
const TUTORIAL: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?HuntID=";
const A: &str = "UID-recovery-a";
const B: &str = "UID-recovery-b";

fn ready(port: i64, session: Option<&str>) -> Answer {
    let mut body = json!({"host": "127.0.0.1", "port": port});
    if let Some(session) = session {
        body["sessionId"] = json!(session);
    }
    Answer::json(200, body)
}

fn full() -> Answer {
    Answer::json(503, json!({"error": "capacity_unavailable", "reason": "memory"}))
}

fn health(servers: Value, complete: Option<bool>) -> impl Fn() -> Answer + Send + Sync + 'static {
    move || {
        let mut body = json!({"servers": servers});
        if let Some(complete) = complete {
            body["complete"] = json!(complete);
        }
        Answer::json(200, body)
    }
}

#[derive(Debug, Default)]
struct PartyView {
    current: Option<String>,
    last: Option<String>,
    deadline: Option<i64>,
    created: Option<i64>,
    capacity_deadline: Option<i64>,
}

struct Rc {
    app: TestApp,
    deploy: ScriptedDeploy,
}

async fn rc(extra: &[(&str, &str)]) -> Rc {
    let deploy = ScriptedDeploy::start(|_, _| ready(8790, None)).await;
    let mut env = vec![
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", deploy.address.as_str()),
    ];
    env.extend_from_slice(extra);
    let app = TestApp::with_env(&env).await;
    for user in [A, B] {
        app.run("INSERT INTO users (userId, name, notes) VALUES (?1, ?1, 0)", &[&user]);
    }
    app.state
        .party_clock
        .set_for_tests(metagame::js::date_parse("2026-10-09T12:00:00Z").unwrap());
    Rc { app, deploy }
}

fn kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::Unknown => "unknown",
        Decision::Failed(_) => "failed",
        Decision::Matching { .. } => "matching",
        Decision::Travel(_) => "travel",
    }
}

async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
}

impl Rc {
    fn now(&self) -> i64 {
        self.app.state.party_clock.now()
    }

    fn advance(&self, ms: i64) {
        self.app.state.party_clock.advance_for_tests(ms);
    }

    async fn join(&self, mode: &str, args: &str, hunt: &str, player: &str, private: bool) -> bool {
        matchmaking::join(
            &self.app.state,
            Some(json!(mode)),
            Some(json!(args)),
            Some(json!(hunt)),
            player,
            private,
        )
        .await
        .unwrap()
    }

    /// A join on its own task, as Node's un-awaited promise
    fn join_later(&self, mode: &str, args: &str, hunt: &str, player: &str) -> tokio::task::JoinHandle<bool> {
        let (state, mode, args, hunt, player) = (
            self.app.state.clone(),
            mode.to_owned(),
            args.to_owned(),
            hunt.to_owned(),
            player.to_owned(),
        );
        tokio::spawn(async move {
            matchmaking::join(
                &state,
                Some(json!(mode)),
                Some(json!(args)),
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

    async fn decide(&self, player: &str) -> &'static str {
        kind(&matchmaking::decide_status(&self.app.state, player).await)
    }

    async fn start_hunt(&self, player: &str) -> Option<Entry> {
        assert!(self.join("ISLAND", "", HUNT, player, true).await);
        settle().await;
        self.status(player).await
    }

    async fn poll(&self, who: &str) -> Value {
        let (state, who, now) = (self.app.state.clone(), who.to_owned(), self.now());
        let settings = state.party_settings();
        self.app
            .db(move |conn| state.parties.lock().poll(conn, settings, &who, now))
            .await
    }

    async fn form_party(&self) {
        let party = self.poll(A).await["partyId"].as_str().unwrap().to_owned();
        self.poll(B).await;
        let (state, now, id) = (self.app.state.clone(), self.now(), party.clone());
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
        let (state, now) = (self.app.state.clone(), self.now());
        let settings = state.party_settings();
        let accepted = self
            .app
            .db(move |conn| state.parties.lock().accept(conn, settings, B, &party, now))
            .await;
        assert_eq!(accepted.status, 200);
    }

    fn touch(&self, who: &str) {
        self.app.state.parties.lock().touch(who, self.now());
    }

    /// The party's current and last candidate ids, and its current candidate's deadlines and creation
    fn party(&self, who: &str) -> PartyView {
        let state = self.app.state.parties.lock();
        let Some(party) = state.party_id_of(who).and_then(|id| state.party(&id)) else {
            return PartyView::default();
        };
        let current = party.candidate();
        PartyView {
            current: current.map(|c| c.candidate_id.clone()),
            last: party.last_candidate().map(|c| c.candidate_id.clone()),
            deadline: current.and_then(|c| c.launch_deadline),
            created: current.map(|c| c.created_at),
            capacity_deadline: current.and_then(|c| c.capacity_deadline),
        }
    }

    async fn join_candidate(&self, player: &str, candidate: &str) -> bool {
        matchmaking::join_candidate(&self.app.state, player, candidate)
            .await
            .unwrap()
            .is_some()
    }

    fn posts(&self) -> usize {
        self.deploy.calls().len()
    }

    async fn wait_until_ready(&self, player: &str) {
        for _ in 0..100 {
            if self.status(player).await.is_some_and(|e| e.ready) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("{player} never became ready");
    }
}

/// `an unanswered deploy ${phase} reaches FAILED, aborts, and ignores a late success without retrying`
async fn unanswered_deploy_reaches_failed(body_phase: bool) {
    let r = rc(&[("MATCHMAKING_DEPLOY_TIMEOUT_MS", "1000")]).await;
    if body_phase {
        r.deploy
            .set(|_, _| Answer::SlowBody(200, json!({"host": "127.0.0.1", "port": 8790}).to_string()));
    } else {
        r.deploy.hold(true);
    }
    r.start_hunt(A).await;
    assert_eq!(r.decide(A).await, "matching");
    r.advance(1000);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert_eq!(
        r.deploy.aborted.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the request was given up"
    );
    assert_eq!(r.decide(A).await, "failed");
    r.deploy.release();
    settle().await;
    let late = r.status(A).await.unwrap();
    assert_eq!((late.failed, late.ready), (true, false));
    r.advance(60_000);
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(
        r.posts(),
        1,
        "an ambiguous timeout must not allocate another game automatically"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unanswered_deploy_headers_reaches_failed_aborts_and_ignores_a_late_success_without_retrying() {
    unanswered_deploy_reaches_failed(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unanswered_deploy_body_reaches_failed_aborts_and_ignores_a_late_success_without_retrying() {
    unanswered_deploy_reaches_failed(true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_default_deployment_deadline_permits_a_180_second_cold_start_and_ends_at_240_seconds() {
    let r = rc(&[]).await;
    r.deploy.hold(true);
    r.start_hunt(A).await;
    r.advance(200_000);
    assert_eq!(r.decide(A).await, "matching");
    r.advance(40_000);
    assert_eq!(r.decide(A).await, "failed");
    r.deploy.release();
    settle().await;
    assert_eq!(r.decide(A).await, "failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cold_direct_join_publishes_its_candidate_immediately_answers_within_1_5_seconds_and_deduplicates_retries() {
    let r = rc(&[]).await;
    r.deploy.hold(true);
    let joining = r.join_later("CITY", "", CITY, A);
    let mut pending = None;
    for _ in 0..50 {
        pending = r.status(A).await;
        if pending.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let pending = pending.expect("published before the deploy server answers");
    assert_eq!(r.decide(A).await, "matching");
    assert!(r.join("CITY", "", CITY, A, false).await);
    assert_eq!(r.posts(), 1);
    let answered = tokio::time::timeout(std::time::Duration::from_millis(1700), joining).await;
    assert_eq!(answered.ok().map(Result::unwrap), Some(true), "answered within 1.5 s");
    assert_eq!(r.status(A).await.map(|e| e.candidate_id), Some(pending.candidate_id));
    r.deploy.set(|_, _| ready(8777, None));
    r.deploy.release();
    r.wait_until_ready(A).await;
    assert_eq!(r.decide(A).await, "travel");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_configured_long_party_launch_remains_visible_until_its_fixed_deadline_and_retains_its_ready_window() {
    let r = rc(&[("MATCHMAKING_DEPLOY_TIMEOUT_MS", "900000")]).await;
    r.form_party().await;
    r.deploy.set(|_, _| ready(8790, Some("native-slow-party")));
    r.deploy.hold(true);
    let joining = {
        let state = r.app.state.clone();
        tokio::spawn(async move {
            matchmaking::join(
                &state,
                Some(json!("ISLAND")),
                Some(json!("")),
                Some(json!(HUNT)),
                A,
                false,
            )
            .await
            .unwrap()
        })
    };
    for _ in 0..50 {
        if r.party(A).deadline.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let deadline = r.party(A).deadline;
    assert_eq!(deadline, Some(r.now() + 900_000));
    joining.await.unwrap();
    r.advance(300_000);
    r.touch(A);
    r.touch(B);
    assert_eq!(r.poll(B).await["candidateState"], "MATCHING");
    assert_eq!(r.party(A).deadline, deadline, "polls do not extend the deadline");
    r.deploy.release();
    for _ in 0..100 {
        if r.poll(B).await["candidateState"] == "IN_PROGRESS" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(r.poll(B).await["candidateState"], "IN_PROGRESS");
    assert_eq!(r.decide(B).await, "travel");
}

const DIRECT_PATHS: [(&str, &str, &str, &str); 3] = [
    ("CITY", "CITY", "", CITY),
    ("SHARED", "SHARED", "", "ShatteredIsles_TrainingDojo"),
    ("tutorial", "ISLAND", TUTORIAL, ""),
];

/// `a late ${path.name} completion cannot overwrite a newer hunt`
async fn late_completion_cannot_overwrite_a_newer_hunt(path: usize) {
    let (_, mode, args, hunt) = DIRECT_PATHS[path];
    let r = rc(&[]).await;
    let (old_mode, old_hunt) = (mode.to_owned(), hunt.to_owned());
    r.deploy.set(move |body, _| {
        if body["GameMode"] == old_mode.as_str() && body["HuntId"] == old_hunt.as_str() {
            ready(8777, None)
        } else {
            ready(8790, None)
        }
    });
    r.deploy.hold(true);
    let joining = r.join_later(mode, args, hunt, A);
    // The entry is published before the deploy server is asked
    r.deploy.wait_for_calls(1).await;
    r.deploy.hold(false);
    let newer = r.start_hunt(A).await.unwrap();
    assert_eq!(newer.hunt_id, Some(json!(HUNT)));
    r.deploy.release();
    joining.await.unwrap();
    settle().await;
    let actual = r.status(A).await.unwrap();
    assert_eq!(
        (actual.candidate_id, actual.hunt_id, actual.port),
        (newer.candidate_id, Some(json!(HUNT)), 8790)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_city_completion_cannot_overwrite_a_newer_hunt() {
    late_completion_cannot_overwrite_a_newer_hunt(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_shared_completion_cannot_overwrite_a_newer_hunt() {
    late_completion_cannot_overwrite_a_newer_hunt(1).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_tutorial_completion_cannot_overwrite_a_newer_hunt() {
    late_completion_cannot_overwrite_a_newer_hunt(2).await;
}

/// `cancelling a pending ${path.name} join prevents a late response from recreating it`
async fn cancelling_a_pending_join_prevents_a_late_response(path: usize) {
    let (_, mode, args, hunt) = DIRECT_PATHS[path];
    let r = rc(&[]).await;
    r.deploy.hold(true);
    let joining = r.join_later(mode, args, hunt, A);
    // The entry is published before the deploy server is asked
    r.deploy.wait_for_calls(1).await;
    assert!(matchmaking::cancel(&mut r.app.state.parties.lock(), A, r.now()).is_some());
    r.deploy.release();
    joining.await.unwrap();
    settle().await;
    assert_eq!(r.decide(A).await, "unknown");
    assert_eq!(r.posts(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_pending_city_join_prevents_a_late_response_from_recreating_it() {
    cancelling_a_pending_join_prevents_a_late_response(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_pending_shared_join_prevents_a_late_response_from_recreating_it() {
    cancelling_a_pending_join_prevents_a_late_response(1).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_pending_tutorial_join_prevents_a_late_response_from_recreating_it() {
    cancelling_a_pending_join_prevents_a_late_response(2).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_solo_joins_do_not_extend_an_existing_20_second_park() {
    let r = rc(&[]).await;
    r.start_hunt(A).await;
    assert_eq!(r.decide(A).await, "travel");
    r.advance(1000);
    r.join("ISLAND", "", HUNT, A, true).await;
    let parked = r.status(A).await.unwrap().parked_at;
    r.advance(10_000);
    r.join("ISLAND", "", HUNT, A, true).await;
    assert_eq!(r.status(A).await.unwrap().parked_at, parked);
    assert_eq!(r.decide(A).await, "matching");
    r.advance(10_000);
    r.join("ISLAND", "", HUNT, A, true).await;
    assert_eq!(r.decide(A).await, "travel");
    assert_eq!(r.posts(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_candidate_reusing_an_old_hunt_port_does_not_inherit_its_travel_park() {
    let r = rc(&[]).await;
    let old = r.start_hunt(A).await.unwrap();
    assert_eq!(r.decide(A).await, "travel");
    r.advance(61_000);
    let fresh = r.start_hunt(A).await.unwrap();
    assert_ne!(fresh.candidate_id, old.candidate_id);
    assert_eq!(fresh.port, old.port);
    r.join("ISLAND", "", HUNT, A, true).await;
    assert_eq!(r.decide(A).await, "travel");
    assert_eq!(r.posts(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn party_candidate_joins_do_not_extend_a_20_second_park() {
    let r = rc(&[]).await;
    r.form_party().await;
    r.join("ISLAND", "", HUNT, A, false).await;
    let initial = r.status(B).await.unwrap();
    assert_eq!(r.decide(B).await, "travel");
    r.advance(1000);
    assert!(r.join_candidate(B, &initial.candidate_id).await);
    r.advance(10_000);
    assert!(r.join_candidate(B, &initial.candidate_id).await);
    assert_eq!(r.decide(B).await, "matching");
    r.advance(10_000);
    assert!(r.join_candidate(B, &initial.candidate_id).await);
    assert_eq!(r.decide(B).await, "travel");
    assert_eq!(r.posts(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_repeated_travel_retries_expire_from_the_original_first_travel_and_only_an_explicit_join_reallocates() {
    let r = rc(&[]).await;
    let initial = r.start_hunt(A).await.unwrap();
    assert_eq!(r.decide(A).await, "travel");
    let first_sent = r.status(A).await.unwrap().first_sent_at;
    for _ in 0..2 {
        r.advance(1000);
        r.join("ISLAND", "", HUNT, A, true).await;
        r.advance(20_000);
        assert_eq!(r.decide(A).await, "travel");
        assert_eq!(r.status(A).await.unwrap().first_sent_at, first_sent);
    }
    r.advance(19_000);
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(r.posts(), 1);
    let fresh = r.start_hunt(A).await.unwrap();
    assert_ne!(fresh.candidate_id, initial.candidate_id);
    assert_eq!(r.posts(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_healthy_native_session_remains_reusable_after_60_seconds_with_immutable_first_travel_identity() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| ready(8790, Some("native-hunt-1")));
    r.deploy
        .set_health(health(json!([{"id": "native-hunt-1", "port": 8790}]), Some(true)));
    let initial = r.start_hunt(A).await.unwrap();
    assert_eq!(r.decide(A).await, "travel");
    let first_sent = r.status(A).await.unwrap().first_sent_at;
    r.advance(61_000);
    assert_eq!(r.decide(A).await, "travel");
    r.join("ISLAND", "", HUNT, A, true).await;
    r.advance(20_000);
    assert_eq!(r.decide(A).await, "travel");
    let now = r.status(A).await.unwrap();
    assert_eq!(
        (now.candidate_id, now.first_sent_at),
        (initial.candidate_id, first_sent)
    );
    assert_eq!(r.posts(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_complete_snapshot_detects_a_dead_native_session_even_when_its_port_was_reused() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| ready(8790, Some("native-old")));
    let initial = r.start_hunt(A).await.unwrap();
    assert_eq!(r.decide(A).await, "travel");
    r.deploy
        .set_health(health(json!([{"id": "native-replacement", "port": 8790}]), Some(true)));
    r.advance(5000);
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(initial.game_session(), "native-old");
    assert_eq!(r.posts(), 1);
    r.deploy.set(|_, _| ready(8791, Some("native-new")));
    let fresh = r.start_hunt(A).await.unwrap();
    assert_ne!(fresh.candidate_id, initial.candidate_id);
    assert_eq!(fresh.port, 8791);
}

#[tokio::test(flavor = "multi_thread")]
async fn incomplete_or_legacy_snapshots_are_unknown_not_death_and_cannot_slide_the_fallback_deadline() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| ready(8790, Some("native-unreachable-worker")));
    r.start_hunt(A).await;
    assert_eq!(r.decide(A).await, "travel");
    for complete in [Some(false), None] {
        r.deploy.set_health(health(json!([]), complete));
        r.advance(10_000);
        assert_eq!(r.decide(A).await, "travel");
    }
    r.advance(41_000);
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(r.posts(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_status_polls_share_one_bounded_health_snapshot_request() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| ready(8790, Some("native-party")));
    r.form_party().await;
    r.join("ISLAND", "", HUNT, A, false).await;
    r.deploy.set_health(health(json!([{"id": "native-party"}]), Some(true)));
    r.deploy.hold_health(true);
    r.advance(5000);
    let polling = futures_util::future::join3(r.decide(A), r.decide(B), r.decide(A));
    let checked = async {
        for _ in 0..100 {
            if r.deploy.health_calls.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        settle().await;
        assert_eq!(r.deploy.health_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        r.deploy.release();
    };
    let ((first, second, third), ()) = tokio::join!(polling, checked);
    assert_eq!([first, second, third], ["travel", "travel", "travel"]);
    r.decide(A).await;
    assert_eq!(r.deploy.health_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dead_party_allocation_is_removed_from_both_current_and_last_candidate_caches() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| ready(8790, Some("native-party-dead")));
    r.form_party().await;
    r.join("ISLAND", "", HUNT, A, false).await;
    let initial = r.status(B).await.unwrap();
    r.deploy.set_health(health(json!([]), Some(true)));
    r.advance(5000);
    assert_eq!(r.decide(B).await, "failed");
    assert_eq!((r.party(A).current, r.party(A).last), (None, None));
    assert!(!r.join_candidate(B, &initial.candidate_id).await);
    r.deploy.set(|_, _| ready(8791, Some("native-party-new")));
    r.join("ISLAND", "", HUNT, A, false).await;
    assert_ne!(r.status(B).await.map(|e| e.candidate_id), Some(initial.candidate_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_party_restoration_preserves_the_current_usable_city_candidate() {
    let r = rc(&[]).await;
    r.deploy.set(|body, _| {
        if body["GameMode"] == "CITY" {
            ready(8777, Some("native-current-city"))
        } else {
            ready(8790, Some("native-original-party"))
        }
    });
    r.deploy.set_health(health(
        json!([{"id": "native-original-party"}, {"id": "native-current-city"}]),
        Some(true),
    ));
    r.form_party().await;
    r.join("ISLAND", "", HUNT, A, false).await;
    let original = r.status(B).await.unwrap();
    assert_eq!(r.decide(A).await, "travel");
    assert_eq!(r.decide(B).await, "travel");
    r.advance(61_000);
    r.poll(B).await;
    assert_eq!(r.party(B).current, None);
    assert_eq!(r.party(B).last, Some(original.candidate_id.clone()));
    assert!(r.join("CITY", "", CITY, B, false).await);
    let city = r.status(B).await.unwrap();
    assert!(city.ready);
    // The old party cache cannot authorize restoring a roster that was invalidated.
    r.app
        .state
        .parties
        .lock()
        .mm
        .roster
        .cancel_candidate(&original.candidate_id);
    assert!(!r.join_candidate(B, &original.candidate_id).await);
    assert!(!r.join("ISLAND", "", HUNT, B, false).await);
    let still = r.status(B).await.unwrap();
    assert_eq!((still.candidate_id, still.ready), (city.candidate_id.clone(), true));
    assert_eq!(r.decide(B).await, "travel");
    assert_eq!(city.session_id.as_deref(), Some("native-current-city"));
    assert_eq!(r.posts(), 2);
}

/// `a ${kind} capacity retry cannot revive an attempt after its renewed idle lease`
async fn capacity_retry_cannot_revive_an_attempt(kind: &str) {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| full());
    if kind == "party" {
        r.form_party().await;
    }
    if kind == "direct" {
        r.join("CITY", "", CITY, A, false).await;
    } else {
        r.join("ISLAND", "", HUNT, A, kind == "private").await;
    }
    settle().await;
    assert!(r.status(A).await.is_some());
    r.deploy.set(|_, _| ready(8790, None));
    r.deploy.hold(true);
    r.advance(10_000);
    let retry = {
        let state = r.app.state.clone();
        tokio::spawn(async move { matchmaking::check_and_update(&state, A).await })
    };
    r.deploy.wait_for_calls(2).await;
    r.advance(1500);
    retry.await.unwrap();
    r.touch(A);
    r.touch(B);
    // The retry poll at 10 seconds renews the lease to 70 seconds.
    r.advance(58_500);
    assert_eq!(r.decide(A).await, "failed");
    r.deploy.release();
    settle().await;
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(r.status(A).await.map(|e| e.ready), Some(false));
    if kind == "party" {
        assert_eq!(r.decide(B).await, "failed");
    }
    assert_eq!(r.posts(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_private_capacity_retry_cannot_revive_an_attempt_after_its_renewed_idle_lease() {
    capacity_retry_cannot_revive_an_attempt("private").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_direct_capacity_retry_cannot_revive_an_attempt_after_its_renewed_idle_lease() {
    capacity_retry_cannot_revive_an_attempt("direct").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_capacity_retry_cannot_revive_an_attempt_after_its_renewed_idle_lease() {
    capacity_retry_cannot_revive_an_attempt("party").await;
}

/// `party polls honor bounded Retry-After ${header} without duplicate launches`
async fn party_polls_honor_bounded_retry_after(header: Option<&'static str>, delay: i64) {
    let r = rc(&[]).await;
    r.deploy.set(move |_, _| {
        let headers = header
            .map(|h| vec![("Retry-After".to_owned(), h.to_owned())])
            .unwrap_or_default();
        Answer::Reply(
            503,
            json!({"error": "capacity_unavailable", "reason": "memory"}).to_string(),
            headers,
        )
    });
    r.form_party().await;
    r.join("ISLAND", "", HUNT, A, false).await;
    settle().await;
    assert_eq!(r.posts(), 1);
    r.advance(delay - 1);
    r.touch(A);
    r.touch(B);
    tokio::join!(r.status(A), r.status(B), r.status(A));
    assert_eq!(r.posts(), 1);
    r.deploy.set(|_, _| ready(8790, None));
    r.deploy.hold(true);
    r.advance(1);
    let polls = async { tokio::join!(r.status(A), r.status(B), r.status(A)) };
    let released = async {
        r.deploy.wait_for_calls(2).await;
        r.deploy.release();
    };
    tokio::join!(polls, released);
    r.wait_until_ready(A).await;
    r.wait_until_ready(B).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn party_polls_honor_bounded_retry_after_null_without_duplicate_launches() {
    party_polls_honor_bounded_retry_after(None, 3000).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn party_polls_honor_bounded_retry_after_invalid_without_duplicate_launches() {
    party_polls_honor_bounded_retry_after(Some("invalid"), 3000).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn party_polls_honor_bounded_retry_after_10_without_duplicate_launches() {
    party_polls_honor_bounded_retry_after(Some("10"), 10_000).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn party_polls_honor_bounded_retry_after_999999_without_duplicate_launches() {
    party_polls_honor_bounded_retry_after(Some("999999"), 30_000).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ramsgate_capacity_waits_retain_a_polling_player_for_twenty_minutes_then_admit_them() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| full());
    r.join("CITY", "", CITY, A, false).await;
    settle().await;
    let candidate = r.status(A).await.unwrap().candidate_id;
    for _ in (0..20 * 60).step_by(10) {
        r.advance(10_000);
        assert_eq!(r.decide(A).await, "matching");
        assert_eq!(r.status(A).await.map(|e| e.candidate_id), Some(candidate.clone()));
    }
    r.deploy.set(|_, _| ready(8790, None));
    r.advance(10_000);
    assert_eq!(r.decide(A).await, "travel");
    assert_eq!(r.status(A).await.map(|e| e.candidate_id), Some(candidate));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_ramsgate_request_cannot_overtake_the_oldest_waiting_player() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| full());
    r.join("CITY", "", CITY, A, false).await;
    settle().await;
    r.advance(1000);
    r.deploy.set(|_, _| ready(8790, None));
    r.join("CITY", "", CITY, B, false).await;
    settle().await;
    assert_eq!(r.posts(), 1);
    r.advance(10_000);
    assert_eq!(r.status(B).await.map(|e| e.ready), Some(false));
    assert_eq!(r.posts(), 1);
    assert_eq!(r.status(A).await.map(|e| e.ready), Some(true));
    assert_eq!(r.deploy.calls()[1]["ExpectedPlayers"][0], A);
    r.advance(10_000);
    assert_eq!(r.status(B).await.map(|e| e.ready), Some(true));
    assert_eq!(r.deploy.calls()[2]["ExpectedPlayers"][0], B);
}

#[tokio::test(flavor = "multi_thread")]
async fn full_ramsgate_capacity_does_not_prevent_a_hunt_allocation() {
    let r = rc(&[]).await;
    r.deploy.set(|body, _| {
        if body["GameMode"] == "CITY" {
            full()
        } else {
            ready(8790, None)
        }
    });
    r.join("CITY", "", CITY, A, false).await;
    settle().await;
    assert_eq!(r.start_hunt(B).await.map(|e| e.ready), Some(true));
    assert_eq!(r.status(A).await.map(|e| e.ready), Some(false));
    let modes: Vec<Value> = r.deploy.calls().iter().map(|c| c["GameMode"].clone()).collect();
    assert_eq!(modes, [json!("CITY"), json!("ISLAND")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_the_oldest_ramsgate_waiter_releases_the_next_player() {
    let r = rc(&[]).await;
    r.deploy.set(|_, _| full());
    r.join("CITY", "", CITY, A, false).await;
    settle().await;
    r.advance(1000);
    r.join("CITY", "", CITY, B, false).await;
    settle().await;
    assert_eq!(r.posts(), 1);
    matchmaking::cancel(&mut r.app.state.parties.lock(), A, r.now());
    r.deploy.set(|_, _| ready(8790, None));
    r.advance(10_000);
    assert_eq!(r.status(B).await.map(|e| e.ready), Some(true));
    assert!(r.status(A).await.is_none());
    assert_eq!(r.posts(), 2);
    assert_eq!(r.deploy.calls()[1]["ExpectedPlayers"], json!([B]));
}

#[tokio::test(flavor = "multi_thread")]
async fn actively_polling_parties_retain_capacity_queue_membership_beyond_five_minutes() {
    let r = rc(&[]).await;
    r.form_party().await;
    r.deploy.set(|_, _| full());
    r.join("ISLAND", "", HUNT, A, false).await;
    settle().await;
    let PartyView {
        current: candidate,
        created,
        ..
    } = r.party(A);
    for _ in (0..20 * 60).step_by(10) {
        r.advance(10_000);
        r.touch(A);
        r.touch(B);
        r.poll(A).await;
        r.poll(B).await;
        assert_eq!(r.party(A).current, candidate);
        assert_eq!(r.decide(A).await, "matching");
        assert_eq!(r.decide(B).await, "matching");
        assert_eq!(r.party(A).created, created);
    }
    r.deploy.set(|_, _| ready(8790, None));
    r.advance(10_000);
    assert_eq!(r.decide(A).await, "travel");
    assert_eq!(r.decide(B).await, "travel");
    assert_eq!(
        r.party(A).capacity_deadline,
        None,
        "no capacity deadline once it has a server"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn party_polling_without_matchmaking_polls_cannot_keep_a_capacity_lease_alive() {
    let r = rc(&[]).await;
    r.form_party().await;
    r.deploy.set(|_, _| full());
    r.join("ISLAND", "", HUNT, A, false).await;
    settle().await;
    assert!(r.party(A).current.is_some());
    for _ in (0..70).step_by(10) {
        r.advance(10_000);
        r.touch(A);
        r.touch(B);
        r.poll(A).await;
        r.poll(B).await;
    }
    assert_eq!(r.party(A).current, None);
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(r.decide(B).await, "failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_orphaned_party_capacity_retry_fails_its_entries_and_releases_the_next_hunt() {
    let r = rc(&[]).await;
    r.form_party().await;
    r.deploy.set(|_, _| full());
    r.join("ISLAND", "", HUNT, A, false).await;
    settle().await;
    let old = r.status(A).await.unwrap().candidate_id;
    // Simulate the old party expiry path independently of the new lease guard.
    {
        let mut state = r.app.state.parties.lock();
        let id = state.party_id_of(A).unwrap();
        state.party_mut(&id).unwrap().clear_candidate();
    }
    let next = "UID-recovery-next";
    r.advance(1000);
    r.start_hunt(next).await;
    assert_eq!(r.posts(), 1);
    r.deploy.set(|_, _| ready(8790, None));
    r.advance(10_000);
    assert_eq!(r.decide(A).await, "failed");
    assert_eq!(r.decide(B).await, "failed");
    assert_eq!(r.status(A).await.map(|e| e.candidate_id), Some(old));
    assert_eq!(r.status(next).await.map(|e| e.ready), Some(true));
    assert_eq!(r.posts(), 2);
    assert_eq!(r.deploy.calls()[1]["ExpectedPlayers"], json!([next]));
}
