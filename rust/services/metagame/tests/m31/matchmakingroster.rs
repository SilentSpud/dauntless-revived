//! matchmakingroster.test.ts: the candidate roster on its own, with a test clock.

use metagame::matchmaking::roster::{
    CONNECTED_PLAYER_ABSENCE_GRACE_MS, Context, PLAYER_LOADING_GRACE_MS, QUEUED_PLAYER_LEASE_MS, Removal, Roster, Seat,
};

/// The Node tests' `MAX_ALLOCATION_WAIT_MS`, the default allocation deadline.
const MAX_ALLOCATION_WAIT_MS: i64 = 300_000;

#[derive(Default)]
struct Fixture {
    roster: Roster,
    now: i64,
}

fn seat(player: &str, candidate: &str) -> Seat {
    Seat {
        player_id: player.into(),
        candidate_id: candidate.into(),
    }
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

fn session(id: &str, connected: Option<&[&str]>) -> Context {
    Context {
        session_id: Some(id.into()),
        connected_player_ids: connected.map(ids),
        ..Context::default()
    }
}

fn allocation(id: &str, connected: Option<&[&str]>) -> Context {
    Context {
        allocation_id: Some(id.into()),
        connected_player_ids: connected.map(ids),
        ..Context::default()
    }
}

const NONE: Option<&[&str]> = None;
const EMPTY: Option<&[&str]> = Some(&[]);

impl Fixture {
    fn advance(&mut self, ms: i64) {
        self.now += ms;
    }

    fn start(&mut self, allocation: &str, session: &str, player: &str, candidate: &str) {
        self.queued(player, candidate, "hunt");
        self.begin(allocation, &[seat(player, candidate)]);
        self.roster.assign_allocation(allocation, Some(session), self.now);
    }

    fn start_default(&mut self) {
        self.start("allocation", "session", "a", "candidate-a");
    }

    fn queued(&mut self, player: &str, candidate: &str, hunt: &str) {
        self.roster.record_queued(player, candidate, hunt, self.now);
    }

    fn begin(&mut self, allocation: &str, seats: &[Seat]) {
        self.begin_until(allocation, seats, self.now + MAX_ALLOCATION_WAIT_MS);
    }

    fn begin_until(&mut self, allocation: &str, seats: &[Seat], deadline: i64) {
        self.roster.begin_allocation(allocation, seats, deadline, self.now);
    }

    fn touch(&mut self, player: &str, candidate: &str) -> bool {
        self.roster.touch_candidate(player, candidate, self.now)
    }

    fn active(&self, player: &str, candidate: &str) -> bool {
        self.roster.queue_member_active(player, candidate, self.now)
    }

    fn served(&mut self, player: &str, candidate: &str) {
        self.roster.served(player, candidate, self.now);
    }

    fn restore(&mut self, player: &str, candidate: &str, hunt: &str, session: Option<&str>) -> bool {
        self.roster.restore_assigned(player, candidate, hunt, session, self.now)
    }

    fn expected(&mut self, players: &[&str], context: &Context) -> Vec<String> {
        self.roster.reconcile_expected(&ids(players), context, self.now)
    }
}

#[test]
fn only_actual_activity_of_that_candidate_renews_its_waiting_place() {
    let mut f = Fixture::default();
    f.queued("gone", "gone-candidate", "hunt");
    f.queued("active", "active-candidate", "hunt");
    f.advance(QUEUED_PLAYER_LEASE_MS);
    assert!(!f.touch("gone", "different-candidate"));
    assert!(!f.touch("unknown", "unknown-candidate"));
    assert!(f.touch("active", "active-candidate"));
    f.advance(1);
    assert!(!f.active("gone", "gone-candidate"));
    assert!(f.active("active", "active-candidate"));
}

#[test]
fn an_in_flight_launch_gets_its_full_deadline_and_a_refusal_does_not_revive_silent_members() {
    let mut f = Fixture::default();
    f.queued("a", "candidate-a", "hunt");
    f.queued("b", "candidate-b", "hunt");
    f.begin_until(
        "attempt",
        &[seat("a", "candidate-a"), seat("b", "candidate-b")],
        300_000,
    );
    f.advance(250_000);
    assert_eq!(f.expected(&["a", "b"], &allocation("attempt", EMPTY)), ids(&["a", "b"]));
    f.touch("b", "candidate-b");
    f.roster.release_allocation("attempt");
    assert!(!f.active("a", "candidate-a"));
    assert!(f.active("b", "candidate-b"));
}

#[test]
fn long_cold_startup_does_not_use_up_the_grace_granted_after_the_first_travel_reply() {
    let mut f = Fixture::default();
    f.queued("a", "candidate-a", "hunt");
    f.begin_until("attempt", &[seat("a", "candidate-a")], 300_000);
    f.advance(240_000);
    f.roster.assign_allocation("attempt", Some("session"), f.now);
    f.advance(20_000);
    f.served("a", "candidate-a");
    f.advance(PLAYER_LOADING_GRACE_MS - 1);
    assert_eq!(f.expected(&["a"], &session("session", EMPTY)), ids(&["a"]));
    // The server's polling and duplicate travel replies are not player activity.
    f.served("a", "candidate-a");
    f.advance(2);
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn expected_player_polling_cannot_renew_an_abandoned_loading_player() {
    let mut f = Fixture::default();
    f.start_default();
    for _ in 0..6 {
        f.advance(30_000);
        assert_eq!(f.expected(&["a"], &session("session", EMPTY)), ids(&["a"]));
    }
    f.advance(1);
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn complete_session_snapshots_retain_connected_players_and_eventually_remove_a_player_who_left() {
    let mut f = Fixture::default();
    f.start_default();
    let here = Some(&["a"][..]);
    f.advance(PLAYER_LOADING_GRACE_MS + 1);
    assert_eq!(f.expected(&["a"], &session("session", here)), ids(&["a"]));
    f.advance(PLAYER_LOADING_GRACE_MS);
    assert_eq!(f.expected(&["a"], &session("session", here)), ids(&["a"]));
    assert_eq!(f.expected(&["a"], &session("session", EMPTY)), ids(&["a"]));
    f.advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS - 1);
    assert_eq!(f.expected(&["a"], &session("session", EMPTY)), ids(&["a"]));
    f.advance(2);
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn missing_snapshots_preserve_unresolved_players_instead_of_treating_the_full_roster_as_absent() {
    let mut f = Fixture::default();
    f.start_default();
    f.advance(PLAYER_LOADING_GRACE_MS * 10);
    assert_eq!(f.expected(&["a"], &session("session", NONE)), ids(&["a"]));
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn an_incomplete_snapshot_resets_the_missing_interval_for_a_previously_arrived_player() {
    let mut f = Fixture::default();
    f.start_default();
    f.expected(&["a"], &session("session", Some(&["a"])));
    f.advance(PLAYER_LOADING_GRACE_MS + 1);
    f.expected(&["a"], &session("session", EMPTY));
    f.advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS - 1);
    assert_eq!(f.expected(&["a"], &session("session", NONE)), ids(&["a"]));
    f.advance(2);
    assert_eq!(f.expected(&["a"], &session("session", EMPTY)), ids(&["a"]));
    f.advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1);
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn replacement_candidate_activity_cannot_keep_the_old_server_waiting_for_the_same_player() {
    let mut f = Fixture::default();
    f.start("old-attempt", "old-session", "a", "candidate-a");
    f.advance(1);
    f.start("new-attempt", "new-session", "a", "new-candidate");
    f.touch("a", "new-candidate");
    // late cancellation of the old candidate
    f.roster.remove_player("a", "candidate-a", Removal::Cancelled);
    assert!(f.expected(&["a"], &session("old-session", NONE)).is_empty());
    assert_eq!(f.expected(&["a"], &session("new-session", NONE)), ids(&["a"]));
    assert!(f.touch("a", "new-candidate"));
}

#[test]
fn a_still_connected_player_is_preserved_during_replacement_with_bounded_grace_after_leaving() {
    let mut f = Fixture::default();
    f.start_default();
    f.queued("a", "new-candidate", "other-hunt");
    assert_eq!(f.expected(&["a"], &session("session", Some(&["a"]))), ids(&["a"]));
    assert_eq!(f.expected(&["a"], &session("session", EMPTY)), ids(&["a"]));
    f.advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1);
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn cancellation_removes_a_missing_party_member_and_late_activity_cannot_resurrect_it() {
    let mut f = Fixture::default();
    for player in ["a", "b"] {
        f.queued(player, "party-candidate", "hunt");
    }
    f.begin(
        "party-attempt",
        &[seat("a", "party-candidate"), seat("b", "party-candidate")],
    );
    f.roster
        .assign_allocation("party-attempt", Some("party-session"), f.now);
    f.roster.remove_player("b", "party-candidate", Removal::Cancelled);
    assert!(!f.touch("b", "party-candidate"));
    assert_eq!(f.expected(&["a", "b"], &session("party-session", NONE)), ids(&["a"]));
    f.roster.cancel_candidate("party-candidate");
    assert!(f.expected(&["a", "b"], &session("party-session", NONE)).is_empty());
}

#[test]
fn legacy_unknown_and_restart_surviving_sessions_retain_their_supplied_roster() {
    let mut f = Fixture::default();
    f.start_default();
    f.roster.remove_player("a", "candidate-a", Removal::Cancelled);
    assert_eq!(f.expected(&["a"], &Context::default()), ids(&["a"]));
    assert_eq!(f.expected(&["a"], &session("unknown", NONE)), ids(&["a"]));
    assert_eq!(
        f.expected(&["untracked-player"], &session("session", NONE)),
        ids(&["untracked-player"])
    );
    // `reset()`: a restart starts with an empty roster
    f.roster = Roster::default();
    assert_eq!(f.expected(&["a"], &session("session", NONE)), ids(&["a"]));
}

#[test]
fn connected_evidence_for_another_session_cannot_rescue_an_abandoned_allocation() {
    let mut f = Fixture::default();
    f.start("old-attempt", "old-session", "a", "candidate-a");
    f.start("new-attempt", "new-session", "a", "new-candidate");
    f.expected(&["a"], &session("new-session", Some(&["a"])));
    f.advance(1);
    assert!(f.expected(&["a"], &session("old-session", EMPTY)).is_empty());
    assert_eq!(f.expected(&["a"], &session("new-session", NONE)), ids(&["a"]));
}

#[test]
fn recreating_a_ready_party_entry_restores_the_original_member_without_restarting_loading_grace() {
    let mut f = Fixture::default();
    f.start_default();
    f.served("a", "candidate-a");
    f.advance(1000);
    f.roster.remove_player("a", "candidate-a", Removal::Replaced);
    f.queued("a", "city-candidate", "city");
    f.advance(170_000);
    // The party's healthy old candidate is selected by the controller again.
    f.queued("a", "candidate-a", "hunt");
    assert!(f.restore("a", "candidate-a", "hunt", Some("session")));
    assert_eq!(f.expected(&["a"], &session("session", NONE)), ids(&["a"]));
    f.served("a", "candidate-a");
    f.advance(QUEUED_PLAYER_LEASE_MS + 1);
    assert!(f.expected(&["a"], &session("session", EMPTY)).is_empty());
}

#[test]
fn an_explicit_member_cancellation_cannot_be_undone_by_ready_candidate_restoration() {
    let mut f = Fixture::default();
    f.start_default();
    f.roster.remove_player("a", "candidate-a", Removal::Cancelled);
    f.queued("a", "candidate-a", "hunt");
    // Replacing the recreated map entry must not erase the earlier cancellation.
    f.roster.remove_player("a", "candidate-a", Removal::Replaced);
    f.queued("a", "candidate-a", "hunt");
    assert!(!f.restore("a", "candidate-a", "hunt", Some("session")));
    assert!(f.expected(&["a"], &session("session", NONE)).is_empty());
}

#[test]
fn restoration_requires_the_original_candidate_and_native_session_membership() {
    let mut f = Fixture::default();
    f.start_default();
    f.start("other-attempt", "other-session", "b", "candidate-b");
    f.queued("a", "replacement", "city");
    f.queued("a", "candidate-a", "hunt");
    assert!(!f.restore("a", "candidate-a", "hunt", Some("other-session")));
    assert!(!f.restore("a", "candidate-a", "wrong-hunt", Some("session")));
    assert!(f.restore("a", "candidate-a", "hunt", None));
    assert_eq!(f.expected(&["a"], &session("session", NONE)), ids(&["a"]));
}

#[test]
fn cancelling_a_candidate_also_invalidates_its_replaced_historical_members() {
    let mut f = Fixture::default();
    f.start_default();
    f.queued("a", "replacement", "city");
    f.roster.cancel_candidate("candidate-a");
    f.queued("a", "candidate-a", "hunt");
    assert!(!f.restore("a", "candidate-a", "hunt", Some("session")));
}

#[test]
fn restoration_finds_the_original_candidate_when_another_allocation_shares_its_pooled_native_session() {
    let mut f = Fixture::default();
    f.start("older-attempt", "pooled-session", "a", "candidate-a");
    f.start("later-attempt", "pooled-session", "b", "candidate-b");
    f.queued("a", "temporary-candidate", "other-hunt");
    assert!(f.restore("a", "candidate-a", "hunt", Some("pooled-session")));
    assert!(f.touch("a", "candidate-a"));
    assert_eq!(f.expected(&["a"], &allocation("older-attempt", NONE)), ids(&["a"]));
    assert_eq!(f.expected(&["b"], &allocation("later-attempt", NONE)), ids(&["b"]));
}

#[test]
fn a_failed_restoration_leaves_the_current_unrelated_membership_usable() {
    let mut f = Fixture::default();
    f.start("original-attempt", "original-session", "a", "candidate-a");
    f.start("current-attempt", "current-session", "a", "current-candidate");
    assert!(!f.restore("a", "candidate-a", "hunt", Some("wrong-session")));
    assert!(f.touch("a", "current-candidate"));
    assert_eq!(f.expected(&["a"], &session("current-session", NONE)), ids(&["a"]));
}
