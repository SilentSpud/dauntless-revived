#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The auth extractors against the cases of the Node metagame's permissions tests (M9).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use http_body_util::BodyExt;
use metagame::app::{App, AppState, AuthConfig, AuthMode};
use metagame::auth::{AdminKey, GameServer, MetagameAuth, Player, SoftAccount, SoftPlayer, UserKey, sign_token};
use metagame::crypto::{TokenKeys, sha256_hex};
use metagame::db::SCHEMA;
use metagame::origin::{OriginConfig, OwnAddresses};
use metagame::schema::users;
use service_kit::json::not_found;
use service_kit::routing::Router;
use service_kit::serve::ConnectionInfo;
use tower::ServiceExt;

const GS_KEY: &str = "gameserver-key-0123456789abcdef0123456789";
const PLAYER_KEY: &str = "UUK_player0123456789abcdef0123456789abcdef01234567";
const ADMIN_KEY: &str = "UUK_admin0123456789abcdef0123456789abcdef0123456789";
const SECRET: &str = "a-long-gateway-secret";

struct Fixture {
    _dir: tempfile::TempDir,
    db: std::path::PathBuf,
    state: AppState,
}

fn fixture(account_key_recovery: bool) -> Fixture {
    fixture_with(account_key_recovery, None)
}

fn fixture_with(account_key_recovery: bool, backup_root: Option<String>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("undaunted.db");
    let store = store::Store::open(&db, &store::Options::default(), &SCHEMA).unwrap();
    let mut conn = SqliteConnection::establish(db.to_str().unwrap()).unwrap();
    conn.batch_execute(&format!(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-a', 'Ada', 0, 0), ('UID-b', 'Bo', 0, 0), ('UID-admin', 'Root', 0, 1);
         INSERT INTO userapikeys (userId, keyHash) VALUES ('UID-a', '{}'), ('UID-admin', '{}');
         INSERT INTO gameserverapikeys (keyHash) VALUES ('{}');",
        sha256_hex(PLAYER_KEY),
        sha256_hex(ADMIN_KEY),
        sha256_hex(GS_KEY)
    ))
    .unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap();
    let private = fixture["privB64"].as_str().unwrap();
    let state = Arc::new(App {
        store,
        tokens: TokenKeys::from_base64_pem(private, fixture["pubB64"].as_str().unwrap()).unwrap(),
        auth: AuthConfig {
            mode: AuthMode::ApiKey,
            production: true,
            account_key_recovery,
            signing_key_b64: private.into(),
        },
        origin: OriginConfig {
            gateway_secret: Some(SECRET.into()),
            gameserver_allow_from: vec![],
        },
        own_addresses: OwnAddresses::default(),
        features: metagame::config::Features::default(),
        registration_mode: std::sync::RwLock::new(Some("OPEN".into())),
        limits: metagame::limits::Limits::default(),
        store_images: metagame::storeimages::StoreImages::load(dir.path()),
        progression: metagame::progression::ProgressionMode::default(),
        progression_config: Arc::new(
            metagame::progression_config::from_env(&service_kit::env::Env::from_pairs([])).unwrap(),
        ),
        real_progression: Default::default(),
        entitlement_defaults: vec![],
        history: metagame::characters::HistorySettings::default(),
        inventory: Default::default(),
        shop: Default::default(),
        store_sweeps: Default::default(),
        friend_requests: Default::default(),
        parties: Default::default(),
        party_clock: Default::default(),
        matchmaking: Default::default(),
        guilds: Default::default(),
        escalation: Default::default(),
        slayer_links: true,
        guild_memory: Default::default(),
        key_recovery_secret: None,
        backup_root,
        body_log: None,
        discord_key_state_file: None,
        developer_accounts: Vec::new(),
        device_signal_key: None,
        activity: Default::default(),
        server_status: Default::default(),
        status_cache: Default::default(),
        health: Arc::new(metagame::health::Health::new(false)),
        key_revocations: tokio::sync::broadcast::channel(64).0,
        friendships: tokio::sync::broadcast::channel(64).0,
    });
    Fixture { _dir: dir, db, state }
}

fn app(state: AppState) -> axum::Router {
    let routes = Router::new()
        .get("/metagame", |auth: MetagameAuth| async move { format!("{auth:?}") })
        .get("/player", |player: Player| async move { player.user_id })
        .get("/soft", |SoftPlayer(claims): SoftPlayer| async move {
            format!("{:?}", claims.map(|c| c.user_id))
        })
        .get("/gameserver", |gs: GameServer| async move {
            format!("{:?}", gs.forwarded_user_id)
        })
        .get("/admin", |AdminKey(user): AdminKey| async move { user.user_id })
        .get("/user", |UserKey(user): UserKey| async move { user.user_id })
        .get("/registered", |SoftAccount(registered): SoftAccount| async move {
            registered.to_string()
        });
    axum::Router::new().fallback_service(routes.into_service(state, not_found))
}

struct Call<'a> {
    path: &'a str,
    peer: &'a str,
    headers: Vec<(&'a str, String)>,
}

impl<'a> Call<'a> {
    fn new(path: &'a str) -> Self {
        Self {
            path,
            peer: "127.0.0.1:50000",
            headers: vec![],
        }
    }

    fn from(mut self, peer: &'a str) -> Self {
        self.peer = peer;
        self
    }

    fn header(mut self, name: &'a str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    async fn send(self, state: &AppState) -> (StatusCode, String) {
        let mut request = Request::get(self.path);
        for (name, value) in self.headers {
            request = request.header(name, value);
        }
        let mut request = request.body(Body::empty()).unwrap();
        let connection = ConnectionInfo {
            peer: self.peer.parse().unwrap(),
            local: "127.0.0.1:61000".parse().unwrap(),
        };
        request.extensions_mut().insert(ConnectInfo(connection));
        let response = app(state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }
}

async fn bearer(state: &AppState, user: &str) -> String {
    format!("Bearer {}", sign_token(state, user).await.unwrap().unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn session_tokens() {
    let f = fixture(false);
    let s = &f.state;
    let token = bearer(s, "UID-a").await;
    assert_eq!(
        Call::new("/metagame").header("authorization", &token).send(s).await.0,
        StatusCode::OK
    );
    assert_eq!(
        Call::new("/player")
            .header("authorization", token.replace("Bearer", "bearer"))
            .send(s)
            .await
            .1,
        "UID-a"
    );
    assert_eq!(
        Call::new("/metagame").send(s).await,
        (StatusCode::UNAUTHORIZED, String::new())
    );
    assert_eq!(
        Call::new("/metagame")
            .header("authorization", "Basic abc")
            .send(s)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        Call::new("/metagame")
            .header("authorization", "bEaReR x")
            .send(s)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    // A key rotation (a newer epoch) revokes older tokens, and so does a ban.
    let mut conn = SqliteConnection::establish(f.db.to_str().unwrap()).unwrap();
    conn.batch_execute("INSERT INTO authepochs (userId, epoch) VALUES ('UID-a', 1)")
        .unwrap();
    assert_eq!(
        Call::new("/metagame").header("authorization", &token).send(s).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        Call::new("/player")
            .header("authorization", bearer(s, "UID-a").await)
            .send(s)
            .await
            .1,
        "UID-a"
    );
    let bo = bearer(s, "UID-b").await;
    conn.batch_execute("INSERT INTO account_bans (account_id, reason, active, updated_at, actor) VALUES ('UID-b', 'cheating', 1, 0, 'test')").unwrap();
    assert_eq!(
        Call::new("/metagame").header("authorization", &bo).send(s).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        sign_token(s, "UID-b").await.unwrap().is_none(),
        "no token for a banned account"
    );
    assert_eq!(
        Call::new("/soft").header("authorization", &bo).send(s).await.1,
        "None",
        "soft auth never refuses"
    );
    assert_eq!(
        Call::new("/soft")
            .header("authorization", bearer(s, "UID-a").await)
            .send(s)
            .await
            .1,
        "Some(\"UID-a\")"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_game_server_key_works_only_from_this_machine() {
    let f = fixture(false);
    let s = &f.state;
    let ok = Call::new("/metagame")
        .header("x-undaunted-gameserver-apikey", GS_KEY)
        .send(s)
        .await;
    assert_eq!(ok, (StatusCode::OK, "GameServer(None)".into()));
    let token = bearer(s, "UID-a").await;
    let forwarded = Call::new("/player")
        .header("x-undaunted-gameserver-apikey", GS_KEY)
        .header("authorization", &token)
        .send(s)
        .await;
    assert_eq!(
        forwarded.1, "UID-a",
        "a game server forwarding the player's token acts for that player"
    );
    assert_eq!(
        Call::new("/player")
            .header("x-undaunted-gameserver-apikey", GS_KEY)
            .send(s)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        Call::new("/metagame")
            .header("x-undaunted-gameserver-apikey", GS_KEY)
            .header("authorization", "Bearer bad")
            .send(s)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        Call::new("/metagame")
            .header("x-undaunted-gameserver-apikey", "wrong")
            .send(s)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        Call::new("/metagame")
            .from("203.0.113.5:4000")
            .header("x-undaunted-gameserver-apikey", GS_KEY)
            .send(s)
            .await,
        (StatusCode::FORBIDDEN, String::new())
    );
    let through_gateway = Call::new("/metagame")
        .header("x-undaunted-gameserver-apikey", GS_KEY)
        .header("x-dauntless-gateway", SECRET)
        .send(s)
        .await;
    assert_eq!(through_gateway.0, StatusCode::FORBIDDEN, "never through the gateway");
}

#[tokio::test(flavor = "multi_thread")]
async fn game_server_only_routes() {
    let f = fixture(false);
    let s = &f.state;
    let refused = Call::new("/gameserver")
        .header("authorization", bearer(s, "UID-a").await)
        .send(s)
        .await;
    assert_eq!(
        refused,
        (
            StatusCode::FORBIDDEN,
            r#"{"code":"","message":"Only a game server may call this.","payload":{}}"#.into()
        )
    );
    let soft = Call::new("/gameserver")
        .header("x-undaunted-gameserver-apikey", GS_KEY)
        .header("authorization", "Bearer expired")
        .send(s)
        .await;
    assert_eq!(
        soft,
        (StatusCode::OK, "None".into()),
        "a bad forwarded token is ignored"
    );
    let with_player = Call::new("/gameserver")
        .header("x-undaunted-gameserver-apikey", GS_KEY)
        .header("authorization", bearer(s, "UID-a").await)
        .send(s)
        .await;
    assert_eq!(with_player.1, "Some(\"UID-a\")");
}

#[tokio::test(flavor = "multi_thread")]
async fn account_keys() {
    let f = fixture(true);
    let s = &f.state;
    assert_eq!(
        Call::new("/admin")
            .header("x-undaunted-user-api-key", ADMIN_KEY)
            .send(s)
            .await
            .1,
        "UID-admin"
    );
    assert_eq!(
        Call::new("/admin")
            .header("x-undaunted-user-api-key", PLAYER_KEY)
            .send(s)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        Call::new("/admin")
            .header("x-undaunted-user-api-key", "UUK_unknown")
            .send(s)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(Call::new("/admin").send(s).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        Call::new("/admin")
            .header("x-undaunted-user-api-key", ADMIN_KEY)
            .header("x-forwarded-for", "1.2.3.4")
            .send(s)
            .await
            .0,
        StatusCode::FORBIDDEN,
        "admin keys never through a proxy"
    );

    assert_eq!(
        Call::new("/user")
            .header("x-undaunted-user-api-key", PLAYER_KEY)
            .send(s)
            .await
            .1,
        "UID-a"
    );
    // The recovery copy of the player's key was kept and decrypts to it.
    let mut conn = SqliteConnection::establish(f.db.to_str().unwrap()).unwrap();
    #[derive(QueryableByName)]
    struct Blob {
        #[diesel(sql_type = diesel::sql_types::Text)]
        ciphertext: String,
    }
    let blob = diesel::sql_query("SELECT ciphertext FROM accountkeyrecovery WHERE userId = 'UID-a'")
        .get_result::<Blob>(&mut conn)
        .unwrap();
    assert_eq!(
        metagame::crypto::open_recovery(&s.auth.signing_key_b64, "UID-a", &blob.ciphertext).unwrap(),
        PLAYER_KEY
    );

    conn.batch_execute("INSERT INTO account_bans (account_id, reason, active, updated_at, actor) VALUES ('UID-a', 'griefing', 1, 0, 'test')").unwrap();
    assert_eq!(
        Call::new("/user")
            .header("x-undaunted-user-api-key", PLAYER_KEY)
            .send(s)
            .await,
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"account_banned","reason":"griefing"}"#.into()
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn registered_callers() {
    let f = fixture(false);
    let s = &f.state;
    assert_eq!(Call::new("/registered").send(s).await.1, "false");
    assert_eq!(
        Call::new("/registered")
            .header("x-undaunted-user-api-key", PLAYER_KEY)
            .send(s)
            .await
            .1,
        "true"
    );
    assert_eq!(
        Call::new("/registered")
            .header("authorization", bearer(s, "UID-b").await)
            .send(s)
            .await
            .1,
        "true"
    );
    assert_eq!(
        Call::new("/registered")
            .header("authorization", bearer(s, "UID-gone").await)
            .send(s)
            .await
            .1,
        "false",
        "deleted account"
    );
}

/// POST /internal/backup copies the live database into an existing folder, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_backup_copies_the_database() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("2026-10-20_013000")).unwrap();
    let f = fixture_with(false, Some(root.path().to_string_lossy().into_owned()));
    let backup = || async {
        let mut request = Request::post("/internal/backup")
            .header("x-undaunted-gameserver-apikey", GS_KEY)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"2026-10-20_013000"}"#))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(ConnectionInfo {
            peer: "127.0.0.1:50000".parse().unwrap(),
            local: "127.0.0.1:61000".parse().unwrap(),
        }));
        let response = metagame::server::app(f.state.clone(), false)
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    };
    assert_eq!(backup().await, (StatusCode::OK, r#"{"copied":true}"#.into()));
    let copy = root.path().join("2026-10-20_013000").join("undaunted.db");
    let mut conn = SqliteConnection::establish(copy.to_str().unwrap()).unwrap();
    let names: Vec<String> = users::table
        .select(users::name)
        .order(users::name)
        .load(&mut conn)
        .unwrap();
    assert_eq!(names, ["Ada", "Bo", "Root"]);
    assert_eq!(
        backup().await,
        (StatusCode::CONFLICT, "Conflict".into()),
        "never over a backup"
    );
}
