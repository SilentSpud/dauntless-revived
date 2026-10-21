//! `keyrotation.test.ts`: key recovery through the Discord bot. A proven link, a challenge, then a new
//! key, which revokes the old key and every token issued before it.

use metagame::keyrotation::{begin, complete, link_discord_account, prove_link};
use serde_json::json;

use crate::common::{TestApp, req, sha256_hex};

struct Fixture {
    user: String,
    discord: String,
    key: String,
}

async fn fixture(app: &TestApp) -> Fixture {
    let (user, _) = app.make_player().await;
    let n: u64 = rand::random::<u32>().into();
    let discord = (100_000_000_000_000_000u64 + n).to_string();
    let key = format!("UUK_{}", metagame::crypto::random_hex(16));
    app.run("INSERT INTO userapikeys VALUES (?1, ?2)", &[&user, &sha256_hex(&key)]);
    Fixture { user, discord, key }
}

fn now() -> i64 {
    metagame::time::now_ms()
}

async fn prove(app: &TestApp, f: &Fixture, key: &str, at: i64) -> bool {
    let (discord, key) = (f.discord.clone(), key.to_owned());
    app.state
        .store
        .write(move |conn| prove_link(conn, Some(&discord), Some(&key), at).map_err(store::Error::from))
        .await
        .unwrap()
}

async fn challenge(app: &TestApp, discord: &str, at: i64) -> Option<String> {
    let discord = discord.to_owned();
    app.state
        .store
        .write(move |conn| begin(conn, Some(&discord), at).map_err(store::Error::from))
        .await
        .unwrap()
}

async fn finish(app: &TestApp, discord: &str, token: &str, at: i64) -> Result<Option<(String, String)>, store::Error> {
    let (discord, token) = (discord.to_owned(), token.to_owned());
    app.state
        .store
        .write(move |conn| complete(conn, Some(&discord), Some(&token), at).map_err(store::Error::from))
        .await
}

async fn owner(app: &TestApp, key: &str) -> Option<String> {
    metagame::auth::user_id_for_key(&app.state, key).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn verified_recovery_rotates_atomically_revokes_old_key_and_jwt_and_refuses_replay() {
    let app = TestApp::start().await;
    let f = fixture(&app).await;
    let token = metagame::auth::sign_token(&app.state, &f.user).await.unwrap().unwrap();
    assert!(prove(&app, &f, &f.key, now()).await);
    let ticket = challenge(&app, &f.discord, now()).await.unwrap();
    let (key, user) = finish(&app, &f.discord, &ticket, now()).await.unwrap().unwrap();
    assert_eq!(user, f.user);
    assert!(regex::Regex::new("^UUK_[a-f0-9]{64}$").unwrap().is_match(&key), "{key}");
    assert_eq!(owner(&app, &f.key).await, None);
    assert_eq!(owner(&app, &key).await, Some(f.user.clone()));
    // The token from before the rotation is refused; a new one works
    assert_eq!(metagame::auth::validate_token(&app.state, &token).await.unwrap(), None);
    assert_eq!(finish(&app, &f.discord, &ticket, now()).await.unwrap(), None);
    let fresh = metagame::auth::sign_token(&app.state, &f.user).await.unwrap().unwrap();
    assert!(
        metagame::auth::validate_token(&app.state, &fresh)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn administrator_created_link_alone_and_wrong_key_do_not_prove_ownership() {
    let app = TestApp::start().await;
    let f = fixture(&app).await;
    let (discord, user) = (f.discord.clone(), f.user.clone());
    app.state
        .store
        .write(move |conn| link_discord_account(conn, Some(&discord), Some(&user)).map_err(store::Error::from))
        .await
        .unwrap();
    assert!(!prove(&app, &f, "wrong", now()).await);
    let ticket = challenge(&app, &f.discord, now()).await;
    if let Some(ticket) = ticket {
        assert_eq!(finish(&app, &f.discord, &ticket, now()).await.unwrap(), None);
    }
    assert_eq!(owner(&app, &f.key).await, Some(f.user.clone()));
}

#[tokio::test(flavor = "multi_thread")]
async fn another_discord_actor_cannot_redeem_a_challenge() {
    let app = TestApp::start().await;
    let (f, other) = (fixture(&app).await, fixture(&app).await);
    assert!(prove(&app, &f, &f.key, now()).await);
    let ticket = challenge(&app, &f.discord, now()).await.unwrap();
    assert_eq!(finish(&app, &other.discord, &ticket, now()).await.unwrap(), None);
    assert!(finish(&app, &f.discord, &ticket, now()).await.unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_challenge_and_excessive_attempts_refuse_without_revoking_the_key() {
    let app = TestApp::start().await;
    let f = fixture(&app).await;
    let at = now();
    assert!(prove(&app, &f, &f.key, at).await);
    let ticket = challenge(&app, &f.discord, at).await.unwrap();
    assert_eq!(finish(&app, &f.discord, &ticket, at + 300_001).await.unwrap(), None);
    for _ in 0..6 {
        challenge(&app, &f.discord, at + 300_002).await;
    }
    assert_eq!(challenge(&app, &f.discord, at + 300_003).await, None);
    assert_eq!(owner(&app, &f.key).await, Some(f.user.clone()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_failure_rolls_back_key_rotation_and_leaves_the_challenge_usable() {
    let app = TestApp::start().await;
    let f = fixture(&app).await;
    assert!(prove(&app, &f, &f.key, now()).await);
    let ticket = challenge(&app, &f.discord, now()).await.unwrap();
    app.exec("CREATE TRIGGER fail_epoch BEFORE INSERT ON authepochs BEGIN SELECT RAISE(ABORT, 'test failure'); END");
    assert!(finish(&app, &f.discord, &ticket, now()).await.is_err());
    assert_eq!(owner(&app, &f.key).await, Some(f.user.clone()));
    app.exec("DROP TRIGGER fail_epoch");
    assert!(finish(&app, &f.discord, &ticket, now()).await.unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_endpoint_refuses_ordinary_players_admin_headers_and_proxied_service_requests() {
    let secret = format!("{}{}", metagame::crypto::random_uuid(), metagame::crypto::random_uuid());
    let app = TestApp::with_env(&[("KEY_RECOVERY_SERVICE_SECRET", &secret)]).await;
    let f = fixture(&app).await;
    let body = json!({"discordId": f.discord});
    let path = "/internal/key-recovery/begin";
    assert_eq!(
        app.post(path, req().bearer(&f.user).json(body.clone())).await.status,
        403
    );
    assert_eq!(app.post(path, req().gs().json(body.clone())).await.status, 403);
    let proxied = req()
        .header("x-key-recovery-service", &secret)
        .header("x-forwarded-for", "192.0.2.1")
        .json(body.clone());
    assert_eq!(app.post(path, proxied).await.status, 403);
    let reply = app
        .post(path, req().header("x-key-recovery-service", &secret).json(body))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    let challenge = reply.json["challenge"].as_str().unwrap();
    assert!(regex::Regex::new("^[a-f0-9]{64}$").unwrap().is_match(challenge));
}
