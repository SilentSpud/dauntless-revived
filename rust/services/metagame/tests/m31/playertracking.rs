//! `playertracking.test.ts`: activity is kept ten minutes after the last heartbeat, while a player who
//! keeps sending heartbeats stays. Node's hunt locations (`UpdatePlayerLocation`) were written by
//! nothing in the Node metagame; the Rust metagame never keeps them, so a returning player can't
//! inherit one.

use metagame::activity::Activity;
use serde_json::json;

#[test]
fn inactive_tracking_expires_without_removing_a_live_long_running_hunt() {
    let activity = Activity::default();
    let mut now = 1_000_000;
    let mut state = activity.lock();
    state.heartbeat("old", None, Some(&json!("city")), now);
    state.heartbeat("active", None, Some(&json!("island")), now);
    for _ in 0..11 {
        now += 60_000;
        state.heartbeat("active", None, Some(&json!("island")), now);
    }
    let online: Vec<String> = state.online(now).into_iter().map(|(id, _)| id).collect();
    assert_eq!(online, ["active"]);
    // Returning after the grace period starts over
    state.heartbeat("old", None, Some(&json!("city")), now);
    let old = state.online(now).into_iter().find(|(id, _)| id == "old").unwrap().1;
    assert_eq!((old.state.as_deref(), old.at), (Some("city"), now));
}
