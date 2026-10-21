//! `capacityturns.test.ts`: who may retry a launch that waits for capacity, per region. The Node calls
//! without a queue age passed `undefined`, which counted as the time of the call.

use std::collections::HashSet;

use metagame::matchmaking::capacity::CapacityTurns;

fn active(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn oldest_capacity_retry_blocks_later_regional_launches_including_while_in_flight() {
    let mut q = CapacityTurns::new(60000);
    let active = active(&["a", "b", "c"]);
    assert!(q.start("a", "ger", 0, &active, 0));
    q.finish("a", true, 0);
    assert!(!q.start("b", "ger", 1, &active, 1));
    assert!(q.start("c", "us", 1, &active, 1));
    assert!(q.start("a", "ger", 2, &active, 2));
    assert!(!q.start("b", "ger", 2, &active, 2));
    q.finish("a", false, 3);
    assert!(q.start("b", "ger", 3, &active, 3));
}

#[test]
fn original_queue_age_wins_over_first_allocation_attempt_and_survives_retries() {
    let mut q = CapacityTurns::new(60000);
    let active = active(&["older", "newer", "last"]);
    assert!(q.start("newer", "eu", 100, &active, 20));
    q.finish("newer", true, 101);
    assert!(q.start("older", "eu", 102, &active, 10));
    q.finish("older", true, 103);
    assert!(!q.start("newer", "eu", 104, &active, 0));
    assert!(!q.start("last", "eu", 105, &active, 30));
    assert!(q.start("older", "eu", 106, &active, 1000));
    q.finish("older", false, 107);
    assert!(!q.start("last", "eu", 108, &active, 30));
    assert!(q.start("newer", "eu", 109, &active, 20));
}

#[test]
fn equal_age_candidates_keep_registration_order_and_an_in_flight_attempt_never_expires() {
    let mut q = CapacityTurns::new(10);
    let active = active(&["a", "b"]);
    assert!(q.start("a", "eu", 0, &active, 0));
    assert!(!q.start("b", "eu", 20, &active, 0));
    assert!(!q.start("a", "eu", 20, &active, 0));
    q.finish("a", true, 20);
    assert!(!q.start("b", "eu", 21, &active, 0));
    q.remove("a");
    assert!(q.start("b", "eu", 22, &active, 0));
}

#[test]
fn an_older_candidate_arriving_during_an_allocation_waits_for_its_result() {
    let mut q = CapacityTurns::new(60000);
    let active = active(&["older", "newer"]);
    assert!(q.start("newer", "eu", 100, &active, 20));
    assert!(!q.start("older", "eu", 101, &active, 10));
    q.finish("newer", true, 102);
    assert!(!q.start("newer", "eu", 103, &active, 20));
    assert!(q.start("older", "eu", 104, &active, 10));
}

#[test]
fn cancelled_and_disconnected_waiters_cannot_block_the_next_hunt() {
    for cancel in [true, false] {
        let mut q = CapacityTurns::new(60000);
        let mut active = active(&["a", "b"]);
        q.start("a", "ger", 0, &active, 0);
        q.finish("a", true, 0);
        assert!(!q.start("b", "ger", 1, &active, 1));
        if cancel {
            active.remove("a");
        }
        let at = if cancel { 2 } else { 60001 };
        assert!(q.start("b", "ger", at, &active, at));
    }
}
