//! `progressionmode.test.ts`: PROGRESSION_MODE and PROGRESSION_REAL_ACCOUNTS, the start line, and the
//! upgrade notice for servers that ran in stub mode. The Rust metagame reads the mode once, at start,
//! so the "warned once, not on every request" of an unknown mode is its start warning.

use metagame::progression::{Caller, ProgressionMode};
use metagame::realprogression::{SeedMode, grant, players_without_real_progression, seed, upgrade_notice};
use serde_json::json;

use crate::common::TestApp;

fn mode(mode: Option<&str>, accounts: Option<&str>) -> ProgressionMode {
    ProgressionMode::new(mode, accounts)
}

#[test]
fn is_real_for_every_account_by_default_progression_mode_unset() {
    let m = mode(None, None);
    assert!(m.is_real(Some("UID-a")) && m.is_real(Some("UID-anyone")));
    assert!(!m.is_stub());
}

#[test]
fn is_real_for_every_account_when_progression_mode_is_empty_or_blank() {
    assert!(mode(Some(""), None).is_real(Some("UID-a")));
    assert!(mode(Some("   "), None).is_real(Some("UID-a")));
}

#[test]
fn is_stub_for_everyone_with_progression_mode_stub_and_no_list_as_upstream() {
    let m = mode(Some("stub"), None);
    assert!(!m.is_real(Some("UID-a")));
    assert!(m.is_stub());
}

#[test]
fn accepts_stub_and_real_in_any_case_with_spaces_around() {
    assert!(!mode(Some(" STUB "), None).is_real(Some("UID-a")));
    assert!(mode(Some(" Real "), None).is_real(Some("UID-a")));
}

#[test]
fn is_real_only_for_the_listed_accounts_in_stub_mode() {
    let m = mode(Some("stub"), Some(" UID-a , UID-b,,"));
    assert!(m.is_real(Some("UID-a")) && m.is_real(Some("UID-b")));
    assert!(!m.is_real(Some("UID-c")) && !m.is_real(Some("")) && !m.is_real(None));
}

#[test]
fn is_real_for_every_account_with_progression_mode_real_but_never_for_a_missing_id() {
    let m = mode(Some("real"), None);
    assert!(m.is_real(Some("UID-anyone")));
    assert!(!m.is_real(Some("")) && !m.is_real(None));
}

#[test]
fn is_real_for_every_account_by_default_even_when_progression_real_accounts_lists_some_the_list_only_matters_in_stub_mode()
 {
    let m = mode(None, Some("UID-a"));
    assert!(m.is_real(Some("UID-a")) && m.is_real(Some("UID-b")));
}

#[test]
fn treats_an_unknown_mode_as_real_the_default_and_says_so_in_the_log() {
    let m = mode(Some("yes"), None);
    assert!(m.is_real(Some("UID-a")) && !m.is_stub());
    assert_eq!(
        m.warning().as_deref(),
        Some(
            r#"PROGRESSION_MODE=yes is not "real" or "stub"; using real (the default). Set PROGRESSION_MODE=stub for upstream's fake max ranks"#
        )
    );
}

#[test]
fn does_not_warn_for_the_known_values_or_an_unset_mode() {
    for value in [None, Some(""), Some("real"), Some("stub"), Some("REAL")] {
        assert_eq!(mode(value, None).warning(), None, "{value:?}");
    }
}

#[test]
fn names_real_as_the_default_when_nothing_is_set() {
    assert_eq!(mode(None, None).describe(), "real for every account (the default)");
}

#[test]
fn does_not_call_an_explicit_progression_mode_real_the_default() {
    assert_eq!(mode(Some("real"), None).describe(), "real for every account");
}

#[test]
fn names_an_unrecognised_value_instead_of_calling_it_the_default_off_used_to_mean_stub() {
    assert_eq!(
        mode(Some("off"), None).describe(),
        "real for every account (PROGRESSION_MODE=off is not recognised)"
    );
    assert_eq!(
        mode(Some("0"), Some("UID-a")).describe(),
        "real for every account (PROGRESSION_MODE=0 is not recognised); PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode"
    );
    // Blank is the same as unset
    assert_eq!(
        mode(Some("  "), None).describe(),
        "real for every account (the default)"
    );
}

#[test]
fn says_that_progression_real_accounts_is_ignored_outside_stub_mode() {
    assert_eq!(
        mode(None, Some("UID-a,UID-b")).describe(),
        "real for every account (the default); PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode"
    );
}

#[test]
fn describes_stub_mode_with_the_number_of_listed_accounts_worded_as_before() {
    assert_eq!(
        mode(Some("stub"), Some("UID-a,UID-b")).describe(),
        "stub, real for 2 listed account(s)"
    );
}

async fn without(app: &TestApp) -> i64 {
    app.db(players_without_real_progression).await
}

async fn notice(app: &TestApp, stub: bool) -> Option<String> {
    app.db(move |conn| upgrade_notice(conn, stub)).await
}

async fn seed_as(app: &TestApp, user: &str, how: SeedMode) -> u16 {
    let (ctx, user) = (app.state.progression_context(), user.to_owned());
    app.db(move |conn| seed(conn, &ctx, &user, how, "UID-admin"))
        .await
        .status
}

#[tokio::test(flavor = "multi_thread")]
async fn counts_players_who_own_a_character_and_have_no_stored_progression_and_nothing_else() {
    let app = TestApp::start().await;
    let before = without(&app).await;
    // An account without a character (an admin account that never played) is not a player
    app.exec("INSERT INTO users (userId, name, notes) VALUES ('UID-nochar', 'NoCharacter', 0)");
    assert_eq!(without(&app).await, before);
    let (_old, _) = app.make_player().await;
    let (played, _) = app.make_player().await;
    let (seeded, _) = app.make_player().await;
    assert_eq!(without(&app).await, before + 3);
    let (ctx, p) = (app.state.progression_context(), played.clone());
    let body =
        json!({"progress_tracks": [{"progression_id": "MasteryTrack_PlayerLevel", "progress": 8}], "objectives": []});
    let granted = app
        .db(move |conn| grant(conn, &ctx, &p, Some(&body), Caller::GameServer))
        .await;
    assert_eq!(granted.status, 200);
    assert_eq!(seed_as(&app, &seeded, SeedMode::Grandfather).await, 200);
    assert_eq!(without(&app).await, before + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn warns_in_real_mode_while_such_players_exist_names_both_ways_out_and_never_changes_their_data() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    let text = notice(&app, false).await.unwrap();
    assert!(
        regex::Regex::new(r"^\d+ player account\(s\) have no stored progression yet: they start at Slayer level 1")
            .unwrap()
            .is_match(&text)
    );
    for part in [
        "Nothing was migrated",
        r#"SeedProgression with Mode "grandfather""#,
        "PROGRESSION_MODE=stub",
    ] {
        assert!(text.contains(part), "{part}");
    }
    // Asking twice changes nothing: still no stored tracks for the player
    notice(&app, false).await;
    assert_eq!(app.count("progress_tracks", "accountId = ?1", &[&user]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn stays_quiet_in_stub_mode_where_nobody_s_ranks_change() {
    let app = TestApp::start().await;
    app.make_player().await;
    assert_eq!(notice(&app, true).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn stays_quiet_once_every_player_has_stored_progression() {
    let app = TestApp::start().await;
    app.make_player().await;
    app.make_player().await;
    for row in app.rows("SELECT DISTINCT userId FROM characters", &[]) {
        seed_as(&app, row["userId"].as_str().unwrap(), SeedMode::Fresh).await;
    }
    assert_eq!(without(&app).await, 0);
    assert_eq!(notice(&app, false).await, None);
}
