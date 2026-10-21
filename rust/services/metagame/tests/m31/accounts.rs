//! `accounts.test.ts`: the username rules, registration, renames and invite codes, against the
//! controller (`metagame::accounts`). Node's `GetUserInfoForApiKey` is GET /undaunted/api/GetUserInfo
//! and `DisplayNameForUserId` the display name the account route answers. Node's file shares one
//! database across tests; each test here starts from the owner's account alone.

use metagame::accounts::{self, Failure, Registered};
use serde_json::{Value, json};

use crate::common::{TestApp, req, sha256_hex};

async fn app_with(extra: &[(&str, &str)]) -> TestApp {
    let app = TestApp::with_env(extra).await;
    // An account from before the rules, as the owner's "Slayer" is
    app.exec("INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-owner', 'Slayer', 0, 1)");
    app
}

async fn app() -> TestApp {
    app_with(&[]).await
}

async fn register(
    app: &TestApp,
    mode: &str,
    name: Value,
    invite: Option<&str>,
) -> Result<Registered, (u16, &'static str)> {
    let (mode, invite) = (mode.to_owned(), invite.map(|i| json!(i)));
    let done = app
        .db(move |conn| accounts::register(conn, &mode, Some(&name).filter(|n| !n.is_null()), invite.as_ref()))
        .await;
    done.map_err(|Failure { status, error }| (status, error.code()))
}

async fn invite_code(app: &TestApp, code: &str, uses: i64, infinite: bool) {
    let code = json!(code);
    assert!(
        app.db(move |conn| accounts::add_invite_code(conn, Some(&code), Some(&json!(uses)), infinite))
            .await
    );
}

fn uses_left(app: &TestApp, code: &str) -> Value {
    app.row("SELECT usesRemaining FROM invitecodes WHERE invitecode = ?1", &[&code])["usesRemaining"].clone()
}

async fn user_info(app: &TestApp, key: &str) -> Option<Value> {
    let reply = app
        .get(
            "/undaunted/api/GetUserInfo",
            req().header("x-undaunted-user-api-key", key),
        )
        .await;
    (reply.status == 200).then_some(reply.json)
}

async fn rename(
    app: &TestApp,
    user_id: Option<&str>,
    username: Option<&str>,
    requested: Option<&str>,
) -> Result<accounts::Renamed, (u16, &'static str)> {
    let (user_id, username, requested) = (
        user_id.map(|v| json!(v)),
        username.map(|v| json!(v)),
        requested.map(|v| json!(v)),
    );
    let done = app
        .db(move |conn| accounts::rename(conn, user_id.as_ref(), username.as_ref(), requested.as_ref()))
        .await;
    done.map_err(|Failure { status, error }| (status, error.code()))
}

async fn display_name(app: &TestApp, user: &str) -> Option<String> {
    let user = user.to_owned();
    app.db(move |conn| accounts::username_for(conn, &user)).await
}

fn count(app: &TestApp, filter: &str) -> i64 {
    app.count("users", filter, &[])
}

#[test]
fn takes_3_16_letters_digits_and_underscores() {
    for name in ["abc", "Slayer_2", "A_B_C_D_E_F_G_H_", "0123456789abcdef", "___"] {
        assert!(accounts::is_valid_username(name), "{name}");
    }
}

#[test]
fn refuses_anything_else() {
    let long = "a".repeat(17);
    for name in [
        "ab",
        long.as_str(),
        "has space",
        " padded",
        "padded ",
        "dash-name",
        "dot.name",
        "émile",
        "名前名前",
        "",
        "tab\tname",
        "new\nline",
    ] {
        assert!(!accounts::is_valid_username(name), "{name:?}");
    }
    // Node's non-string values never reach the rule in Rust: `register` refuses them as bad_request
}

#[tokio::test(flavor = "multi_thread")]
async fn invite_only_signup_issues_distinct_personal_keys_stores_only_hashes_and_rejects_altered_keys() {
    let app = app().await;
    invite_code(&app, "TEST-AUTO-KEYS", 2, false).await;
    let a = register(&app, "INVITECODE", json!("AutoKey_A"), Some("TEST-AUTO-KEYS"))
        .await
        .unwrap();
    let b = register(&app, "INVITECODE", json!("AutoKey_B"), Some("TEST-AUTO-KEYS"))
        .await
        .unwrap();
    assert_ne!(a.key, b.key);
    for account in [&a, &b] {
        let stored = app.row(
            "SELECT userId, keyHash FROM userapikeys WHERE userId = ?1",
            &[&account.user_id],
        );
        assert_eq!(
            stored,
            json!({"userId": account.user_id, "keyHash": sha256_hex(&account.key)})
        );
        assert_eq!(
            user_info(&app, &account.key).await,
            Some(json!({"UserId": account.user_id, "Username": account.username, "IsAdmin": false}))
        );
        let last = if account.key.ends_with('0') { "1" } else { "0" };
        let altered = format!("{}{last}", &account.key[..account.key.len() - 1]);
        assert_eq!(user_info(&app, &altered).await, None);
    }
    assert_eq!(uses_left(&app, "TEST-AUTO-KEYS"), 0);
    assert!(
        register(&app, "INVITECODE", json!("AutoKey_NoInvite"), None)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn registers_a_valid_name_with_an_account_and_its_key_in_one_go_open() {
    let app = app().await;
    let done = register(&app, "OPEN", json!("Friend_One"), None).await.unwrap();
    assert!(
        regex::Regex::new("^UUK_[0-9a-f]{48}$").unwrap().is_match(&done.key),
        "{}",
        done.key
    );
    assert_eq!(app.count("users", "userId = ?1", &[&done.user_id]), 1);
    assert_eq!(app.count("userapikeys", "userId = ?1", &[&done.user_id]), 1);
    assert_eq!(
        user_info(&app, &done.key).await,
        Some(json!({"UserId": done.user_id, "Username": "Friend_One", "IsAdmin": false}))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_name_that_exists_in_any_case_including_the_owner_s_older_name() {
    let app = app().await;
    register(&app, "OPEN", json!("Friend_One"), None).await.unwrap();
    let before = count(&app, "1=1");
    for name in ["slayer", "SLAYER", "Slayer", "friend_one", "FRIEND_ONE"] {
        assert_eq!(
            register(&app, "OPEN", json!(name), None).await.err(),
            Some((409, "username_taken")),
            "{name}"
        );
    }
    assert_eq!(count(&app, "1=1"), before);
    assert!(app.db(|conn| accounts::username_taken(conn, "sLaYeR")).await);
    assert!(!app.db(|conn| accounts::username_taken(conn, "Nobody")).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_bad_request_username_invalid_and_registration_closed() {
    let app = app().await;
    assert_eq!(
        register(&app, "OPEN", Value::Null, None).await.err(),
        Some((400, "bad_request"))
    );
    assert_eq!(
        register(&app, "OPEN", json!(42), None).await.err(),
        Some((400, "bad_request"))
    );
    assert_eq!(
        register(&app, "OPEN", json!("no"), None).await.err(),
        Some((400, "username_invalid"))
    );
    assert_eq!(
        register(&app, "OPEN", json!("has space"), None).await.err(),
        Some((400, "username_invalid"))
    );
    assert_eq!(
        register(&app, "NONE", json!("Valid_Name"), Some("ANY")).await.err(),
        Some((400, "registration_closed"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invitecode_needs_a_live_code_spends_one_use_and_a_taken_name_costs_no_use() {
    let app = app().await;
    invite_code(&app, "TEST-CODE-0001", 2, false).await;
    let refused = Some((401, "invite_invalid"));
    assert_eq!(
        register(&app, "INVITECODE", json!("Invited_A"), None).await.err(),
        refused
    );
    assert_eq!(
        register(&app, "INVITECODE", json!("Invited_A"), Some("  ")).await.err(),
        refused
    );
    assert_eq!(
        register(&app, "INVITECODE", json!("Invited_A"), Some("WRONG-CODE"))
            .await
            .err(),
        refused
    );
    assert_eq!(
        register(&app, "INVITECODE", json!("SLAYER"), Some("TEST-CODE-0001"))
            .await
            .err(),
        Some((409, "username_taken"))
    );
    assert_eq!(uses_left(&app, "TEST-CODE-0001"), 2);
    assert!(
        register(&app, "INVITECODE", json!("Invited_A"), Some("TEST-CODE-0001"))
            .await
            .is_ok()
    );
    assert_eq!(uses_left(&app, "TEST-CODE-0001"), 1);
    assert!(
        register(&app, "INVITECODE", json!("Invited_B"), Some(" TEST-CODE-0001 "))
            .await
            .is_ok()
    );
    assert_eq!(uses_left(&app, "TEST-CODE-0001"), 0);
    assert_eq!(
        register(&app, "INVITECODE", json!("Invited_C"), Some("TEST-CODE-0001"))
            .await
            .err(),
        refused
    );
    // A bad code is reported before the name is looked at
    assert_eq!(
        register(&app, "INVITECODE", json!("slayer"), Some("TEST-CODE-0001"))
            .await
            .err(),
        refused
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invitecode_an_unlimited_code_stays_usable() {
    let app = app().await;
    invite_code(&app, "TEST-OPEN-CODE", 0, true).await;
    assert!(
        register(&app, "INVITECODE", json!("Unlimited_1"), Some("TEST-OPEN-CODE"))
            .await
            .is_ok()
    );
    assert!(
        register(&app, "INVITECODE", json!("Unlimited_2"), Some("TEST-OPEN-CODE"))
            .await
            .is_ok()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drops_surrounding_whitespace_before_checking_and_storing_the_name() {
    let app = app().await;
    let done = register(&app, "OPEN", json!("  Trim_Me \n"), None).await.unwrap();
    assert_eq!(done.username, "Trim_Me");
    assert_eq!(
        app.row("SELECT name FROM users WHERE userId = ?1", &[&done.user_id])["name"],
        "Trim_Me"
    );
    assert_eq!(
        register(&app, "OPEN", json!(" trim_me "), None).await.err(),
        Some((409, "username_taken"))
    );
    assert_eq!(
        register(&app, "OPEN", json!("   "), None).await.err(),
        Some((400, "username_invalid"))
    );
    assert_eq!(
        register(&app, "OPEN", json!("in side"), None).await.err(),
        Some((400, "username_invalid"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lets_only_one_of_two_registrations_of_the_same_name_through() {
    let app = app().await;
    let (first, second) = tokio::join!(
        register(&app, "OPEN", json!("Twin_Name"), None),
        register(&app, "OPEN", json!("TWIN_NAME"), None)
    );
    let mut oks = [first.is_ok(), second.is_ok()];
    oks.sort();
    assert_eq!(oks, [false, true]);
    assert_eq!(count(&app, "lower(name) = 'twin_name'"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn renames_the_account_and_its_characters_together() {
    let app = app().await;
    let registered = register(&app, "OPEN", json!("Old_Name"), None).await.unwrap();
    let user = registered.user_id.clone();
    let character = app
        .db(move |conn| metagame::characters::create(conn, &user, "Old_Name"))
        .await
        .characterId;
    let renamed = rename(&app, Some(&registered.user_id), None, Some("New_Name"))
        .await
        .unwrap();
    assert_eq!(
        (
            renamed.user_id.as_str(),
            renamed.old_username.as_str(),
            renamed.username.as_str(),
            renamed.characters
        ),
        (registered.user_id.as_str(), "Old_Name", "New_Name", 1)
    );
    assert_eq!(
        app.row("SELECT name FROM users WHERE userId = ?1", &[&registered.user_id])["name"],
        "New_Name"
    );
    assert_eq!(
        app.row("SELECT name FROM characters WHERE characterId = ?1", &[&character])["name"],
        "New_Name"
    );
    let account = app
        .get("/account/api/public/account", req().bearer(&registered.user_id))
        .await;
    assert_eq!(account.json["displayName"], "New_Name");
    assert!(register(&app, "OPEN", json!("new_name"), None).await.is_err());
    assert!(
        register(&app, "OPEN", json!("Old_Name"), None).await.is_ok(),
        "the old name is free again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn finds_the_account_by_its_current_name_in_any_case_and_allows_a_change_of_case() {
    let app = app().await;
    register(&app, "OPEN", json!("New_Name"), None).await.unwrap();
    let renamed = rename(&app, None, Some("new_NAME"), Some(" NEW_name ")).await.unwrap();
    assert_eq!(
        (renamed.old_username.as_str(), renamed.username.as_str()),
        ("New_Name", "NEW_name")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_taken_invalid_or_missing_name_and_an_unknown_account() {
    let app = app().await;
    register(&app, "OPEN", json!("NEW_name"), None).await.unwrap();
    assert_eq!(
        rename(&app, None, Some("NEW_name"), Some("slayer")).await.err(),
        Some((409, "username_taken"))
    );
    assert_eq!(
        rename(&app, None, Some("NEW_name"), Some("x")).await.err(),
        Some((400, "username_invalid"))
    );
    assert_eq!(
        rename(&app, None, Some("NEW_name"), None).await.err(),
        Some((400, "bad_request"))
    );
    assert_eq!(
        rename(&app, None, None, Some("Fine_Name")).await.err(),
        Some((400, "bad_request"))
    );
    assert_eq!(
        rename(&app, Some("UID-nobody"), None, Some("Fine_Name")).await.err(),
        Some((404, "not_found"))
    );
    assert_eq!(
        rename(&app, None, Some("Nobody_Here"), Some("Fine_Name")).await.err(),
        Some((404, "not_found"))
    );
}

#[test]
fn makes_three_groups_of_four_unambiguous_characters_that_fit_the_invite_string_rules() {
    let shape = regex::Regex::new("^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$").unwrap();
    let rule = regex::Regex::new("^[A-Za-z0-9-]{4,64}$").unwrap();
    let mut codes = std::collections::HashSet::new();
    for _ in 0..200 {
        let code = accounts::generate_invite_code();
        assert!(shape.is_match(&code) && rule.is_match(&code), "{code}");
        codes.insert(code);
    }
    assert_eq!(codes.len(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn is_the_stored_name_for_an_unknown_account_and_again_with_account_display_name_0() {
    let app = app().await;
    assert_eq!(display_name(&app, "UID-owner").await.as_deref(), Some("Slayer"));
    assert_eq!(
        display_name(&app, "UID-nobody").await.unwrap_or_default(),
        "",
        "an unknown account"
    );
    assert_eq!(
        app.get("/account/api/public/account", req().bearer("UID-owner"))
            .await
            .json["displayName"],
        "Slayer"
    );
    let off = app_with(&[("ACCOUNT_DISPLAY_NAME", "0")]).await;
    assert_eq!(
        off.get("/account/api/public/account", req().bearer("UID-owner"))
            .await
            .json["displayName"],
        json!({})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_key_table_and_the_account_table_in_step() {
    let app = app().await;
    for name in ["Step_One", "Step_Two"] {
        register(&app, "OPEN", json!(name), None).await.unwrap();
    }
    assert_eq!(
        count(&app, "userId LIKE 'UID-%' AND userId != 'UID-owner'"),
        app.count("userapikeys", "1=1", &[])
    );
}
