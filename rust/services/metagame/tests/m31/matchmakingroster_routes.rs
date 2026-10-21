//! `matchmakingroster-routes.test.ts`: what `/candidate/player/alive` and `/register` do with the
//! roster. The roster's clock is the app's party clock.

use metagame::matchmaking::roster::{
    CONNECTED_PLAYER_ABSENCE_GRACE_MS, PLAYER_LOADING_GRACE_MS, QUEUED_PLAYER_LEASE_MS, Removal, Seat,
};
use serde_json::{Value, json};

use crate::common::{Req, TestApp, req};

const SESSION: &str = "5f49491e-8a18-41fb-9e6c-5fbd7d655802";
const A: &str = "UID-roster-a";
const B: &str = "UID-roster-b";
const START: i64 = 1_800_000_000_000;

async fn rr() -> TestApp {
    let app = TestApp::start().await;
    app.state.party_clock.set_for_tests(START);
    app
}

fn set_now(app: &TestApp, ms: i64) {
    app.state.party_clock.set_for_tests(START + ms);
}

fn now(app: &TestApp) -> i64 {
    app.state.party_clock.now()
}

fn candidate(player: &str) -> String {
    format!("candidate-{player}")
}

fn assigned(app: &TestApp) {
    let now = now(app);
    let mut state = app.state.parties.lock();
    let roster = &mut state.mm.roster;
    for player in [A, B] {
        roster.record_queued(player, &candidate(player), "hunt", now);
    }
    let seats: Vec<Seat> = [A, B]
        .iter()
        .map(|player| Seat {
            player_id: (*player).to_owned(),
            candidate_id: candidate(player),
        })
        .collect();
    roster.begin_allocation("attempt", &seats, now + 300_000, now);
    roster.assign_allocation("attempt", Some(SESSION), now);
}

fn remove(app: &TestApp, player: &str) {
    app.state
        .parties
        .lock()
        .mm
        .roster
        .remove_player(player, &candidate(player), Removal::Cancelled);
}

fn active(app: &TestApp, player: &str) -> bool {
    let now = now(app);
    app.state
        .parties
        .lock()
        .mm
        .roster
        .queue_member_active(player, &candidate(player), now)
}

/// A game server's request, tagged with its session and (when given) a connection snapshot.
fn server_context(connected: Option<&[&str]>, session: &str) -> Req {
    let request = req().gs().header("x-dauntless-game-session-id", session);
    match connected {
        Some(ids) => request.header("x-dauntless-connected-player-ids", &json!(ids).to_string()),
        None => request,
    }
}

fn both() -> Value {
    json!({"playerIds": [A, B]})
}

async fn alive(app: &TestApp, path: &str, request: Req) -> Value {
    let reply = app.post(path, request.json(both())).await;
    assert_eq!(reply.status, 200);
    reply.json
}

fn expected(players: &[&str]) -> Value {
    json!({"expectedPlayerIds": players})
}

const SCOPED: &str = "/candidate/player/alive?sessionId=5f49491e-8a18-41fb-9e6c-5fbd7d655802&purpose=expected";

#[tokio::test]
async fn a_game_server_receives_the_reconciled_roster_only_for_its_tagged_session() {
    let app = rr().await;
    assigned(&app);
    remove(&app, B);
    assert_eq!(alive(&app, SCOPED, req().gs()).await, expected(&[A]));
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(None, SESSION)).await,
        expected(&[A])
    );
    assert_eq!(
        alive(&app, "/candidate/player/alive", req().gs()).await,
        expected(&[A, B])
    );
    assert_eq!(
        alive(
            &app,
            "/candidate/player/alive?sessionId=unknown&purpose=expected",
            req().gs()
        )
        .await,
        expected(&[A, B])
    );
    // `MatchmakingRoster.reset()`
    app.state.parties.lock().mm.roster = Default::default();
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[]), SESSION)).await,
        expected(&[A, B])
    );
}

#[tokio::test]
async fn a_complete_native_snapshot_preserves_connected_players_and_removes_an_expired_no_show() {
    let app = rr().await;
    assigned(&app);
    set_now(&app, PLAYER_LOADING_GRACE_MS + 1);
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[A]), SESSION)).await,
        expected(&[A])
    );
    app.state.party_clock.advance_for_tests(PLAYER_LOADING_GRACE_MS * 10);
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[A]), SESSION)).await,
        expected(&[A])
    );
}

#[tokio::test]
async fn a_connected_player_who_leaves_is_eventually_omitted_while_the_remaining_player_is_retained() {
    let app = rr().await;
    assigned(&app);
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[A, B]), SESSION)).await,
        expected(&[A, B])
    );
    set_now(&app, PLAYER_LOADING_GRACE_MS + 1);
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[A]), SESSION)).await,
        expected(&[A, B])
    );
    app.state
        .party_clock
        .advance_for_tests(CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1);
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[A]), SESSION)).await,
        expected(&[A])
    );
}

#[tokio::test]
async fn absent_malformed_or_oversized_connection_snapshots_cannot_expire_unresolved_players() {
    let app = rr().await;
    assigned(&app);
    set_now(&app, PLAYER_LOADING_GRACE_MS + 1);
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(None, SESSION)).await,
        expected(&[A, B])
    );
    let many: Vec<String> = (0..129).map(|i| format!("player-{i}")).collect();
    let long: Vec<String> = (0..70).map(|i| format!("{i}-{}", "x".repeat(125))).collect();
    // Node sent the é as a raw header byte; a test request here can only carry ASCII, so the JSON
    // escape brings the same id to the check.
    for snapshot in [
        String::new(),
        "[".into(),
        "{}".into(),
        json!([A, A]).to_string(),
        json!([""]).to_string(),
        json!(["space in id"]).to_string(),
        r#"["nonascii-é"]"#.into(),
        json!(["line\nfeed"]).to_string(),
        json!(["x".repeat(129)]).to_string(),
        json!(many).to_string(),
        json!(long).to_string(),
    ] {
        let request = server_context(None, SESSION).header("x-dauntless-connected-player-ids", &snapshot);
        assert_eq!(
            alive(&app, "/candidate/player/alive", request).await,
            expected(&[A, B]),
            "{}",
            &snapshot[..snapshot.len().min(80)]
        );
    }
}

#[tokio::test]
async fn a_snapshot_cannot_be_applied_to_a_conflicting_absent_or_malformed_session_tag() {
    let app = rr().await;
    assigned(&app);
    set_now(&app, PLAYER_LOADING_GRACE_MS + 1);
    let other = "/candidate/player/alive?sessionId=9c7e8098-d2ad-4d5d-9df4-4028da3a21ae&purpose=expected";
    assert_eq!(
        alive(&app, other, server_context(Some(&[]), SESSION)).await,
        expected(&[A, B])
    );
    assert_eq!(
        alive(&app, SCOPED, server_context(Some(&[]), "invalid")).await,
        expected(&[A, B])
    );
    assert_eq!(
        alive(
            &app,
            "/candidate/player/alive?purpose=connected",
            server_context(Some(&[]), SESSION)
        )
        .await,
        expected(&[A, B])
    );
    assert_eq!(
        alive(
            &app,
            SCOPED,
            req().gs().header("x-dauntless-connected-player-ids", "[]")
        )
        .await,
        expected(&[A, B])
    );
}

#[tokio::test]
async fn player_tokens_cannot_use_native_headers_to_prune_players_or_claim_that_they_arrived() {
    let app = rr().await;
    assigned(&app);
    remove(&app, B);
    let caller = req()
        .bearer(A)
        .header("x-dauntless-game-session-id", SESSION)
        .header("x-dauntless-connected-player-ids", &json!([B]).to_string());
    assert_eq!(alive(&app, SCOPED, caller).await, expected(&[A, B]));
    assert_eq!(
        alive(&app, "/candidate/player/alive", server_context(Some(&[]), SESSION)).await,
        expected(&[A])
    );
}

#[tokio::test]
async fn platform_registrations_do_not_renew_waiting_leases_or_resurrect_cancelled_candidates() {
    let app = rr().await;
    {
        let now = now(&app);
        let mut state = app.state.parties.lock();
        for player in [A, B] {
            state.mm.roster.record_queued(player, &candidate(player), "hunt", now);
        }
    }
    set_now(&app, QUEUED_PLAYER_LEASE_MS + 1);
    let registered = app
        .post(
            "/candidate/player/register",
            req().bearer(A).json(json!({"userId": B, "playerIds": [B]})),
        )
        .await;
    assert_eq!(registered.status, 200);
    assert!(!active(&app, A));
    assert!(!active(&app, B));
    remove(&app, A);
    app.post("/candidate/player/register", req().bearer(A).json(json!({})))
        .await;
    assert!(!active(&app, A));
    app.post("/candidate/player/register", req().gs().bearer(B).json(json!({})))
        .await;
    assert!(!active(&app, B));
}
