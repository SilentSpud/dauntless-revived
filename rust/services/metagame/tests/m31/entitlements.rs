//! `entitlements.test.ts`: defaults, grants, timed boosts and revocations.

use metagame::entitlements::{grant, has_active, list, revoke};
use serde_json::Value;

use crate::common::TestApp;

fn defaults(app: &TestApp) -> Vec<String> {
    app.state.entitlement_defaults.clone()
}

async fn names(app: &TestApp, user: &str) -> Vec<String> {
    let (d, u) = (defaults(app), user.to_owned());
    let list = app.db(move |conn| list(conn, &d, &u)).await;
    list.iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect()
}

async fn give(app: &TestApp, user: &str, name: &str, hours: i64) -> Vec<Value> {
    let (d, u, n) = (defaults(app), user.to_owned(), name.to_owned());
    app.db(move |conn| grant(conn, &d, &u, &n, hours, "test")).await
}

async fn take(app: &TestApp, user: &str, name: &str) -> bool {
    let (d, u, n) = (defaults(app), user.to_owned(), name.to_owned());
    app.db(move |conn| revoke(conn, &d, &u, &n)).await
}

async fn active(app: &TestApp, user: &str, name: &str) -> bool {
    let (d, u, n) = (defaults(app), user.to_owned(), name.to_owned());
    app.db(move |conn| has_active(conn, &d, &u, &n)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn every_account_owns_the_elite_hunt_pass_by_default_permanently_as_name_duration_activateddate() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let (d, u) = (defaults(&app), user.clone());
    let list = app.db(move |conn| list(conn, &d, &u)).await;
    let listed: Vec<&str> = list.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(listed, ["season_free_any", "season_premium_any", "season09b_premium"]);
    let date = regex::Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$").unwrap();
    for entitlement in &list {
        let keys: Vec<&String> = entitlement.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["name", "duration", "activatedDate"]);
        assert_eq!(entitlement["duration"], 0);
        assert!(date.is_match(entitlement["activatedDate"].as_str().unwrap()));
    }
    assert!(active(&app, &user, "season09b_premium").await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grant_is_listed_once_a_second_identical_grant_does_not_duplicate_it() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert!(give(&app, &user, "x", 0).await.iter().any(|e| e["name"] == "x"));
    assert_eq!(
        give(&app, &user, "x", 0)
            .await
            .iter()
            .filter(|e| e["name"] == "x")
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timed_grant_still_running_is_extended_an_expired_one_is_left_out_of_every_list() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    give(&app, &user, "escalation_xp_boost_entitlement", 2).await;
    let extended = give(&app, &user, "escalation_xp_boost_entitlement", 24).await;
    let boost = extended
        .iter()
        .find(|e| e["name"] == "escalation_xp_boost_entitlement")
        .unwrap();
    assert_eq!(boost["duration"], 26);

    // A 2-hour boost activated 3 hours ago
    give(&app, &user, "boost_old", 2).await;
    let three_hours_ago = metagame::time::iso(metagame::time::now_ms() - 3 * 3600 * 1000);
    app.run(
        "UPDATE entitlements SET activatedDate = ?1 WHERE accountId = ?2 AND name = 'boost_old'",
        &[&three_hours_ago, &user],
    );
    assert!(!names(&app, &user).await.contains(&"boost_old".to_owned()));
    assert!(!active(&app, &user, "boost_old").await);

    // Granting it again starts it over
    give(&app, &user, "boost_old", 2).await;
    assert!(names(&app, &user).await.contains(&"boost_old".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_entitlement_default_or_not_stays_revoked_until_granted_again() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    assert!(take(&app, &user, "season09b_premium").await);
    assert!(!take(&app, &user, "season09b_premium").await);
    assert!(!names(&app, &user).await.contains(&"season09b_premium".to_owned()));
    give(&app, &user, "season09b_premium", 0).await;
    assert!(names(&app, &user).await.contains(&"season09b_premium".to_owned()));
}
