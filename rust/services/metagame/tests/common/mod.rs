//! What the ported TS tests share (MIGRATION_PLAN.md M31): the app over its router with a database
//! of its own, the settings the TS tests ran with (`test/setup.ts`, `authenv.ts`, `appenv.ts`), and
//! `test/appclient.ts`'s `Call`, `test/helpers.ts`'s `MakePlayer` and `Count`. A test that changed an
//! environment variable with `WithEnv` starts an app with that setting instead: the Rust metagame
//! reads its settings once, at start.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request};
use http_body_util::BodyExt;
use metagame::app::AppState;
use metagame::config::Config;
use serde_json::{Map, Value};
use service_kit::env::Env;
use service_kit::serve::ConnectionInfo;
use tower::ServiceExt;

/// The game-server key every test app accepts (`appclient.ts`'s `GS_KEY`).
pub const GS_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

pub struct TestApp {
    pub state: AppState,
    router: axum::Router,
    pub db: PathBuf,
    _dir: tempfile::TempDir,
}

/// One request's extras: the account whose token is the bearer, the game-server key, a body.
#[derive(Default, Clone)]
pub struct Req {
    as_user: Option<String>,
    gs: bool,
    body: Option<String>,
    headers: Vec<(String, String)>,
    peer: Option<String>,
}

pub fn req() -> Req {
    Req::default()
}

impl Req {
    pub fn bearer(mut self, user: &str) -> Self {
        self.as_user = Some(user.to_owned());
        self
    }

    pub fn gs(mut self) -> Self {
        self.gs = true;
        self
    }

    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body.to_string());
        self
    }

    pub fn raw(mut self, body: &str) -> Self {
        self.body = Some(body.to_owned());
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// The caller's address (default 127.0.0.1).
    pub fn from(mut self, address: &str) -> Self {
        self.peer = Some(address.to_owned());
        self
    }
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub json: Value,
    pub text: String,
    pub headers: HeaderMap,
}

static PLAYERS: AtomicU64 = AtomicU64::new(0);

/// The settings `appenv.ts` gives every app test, before the test's own.
const DEFAULTS: [(&str, &str); 6] = [
    ("PORT", "0"),
    ("NODE_ENV", "production"),
    ("AUTH_MODE", "APIKEY"),
    ("MATCHMAKING_MODE", "DISABLED"),
    ("LOG_BODIES", "0"),
    ("LOG_REQUESTS", "0"),
];

impl TestApp {
    pub async fn start() -> Self {
        Self::with_env(&[]).await
    }

    /// An app with `env` on top of the test defaults (a later pair wins).
    pub async fn with_env(env: &[(&str, &str)]) -> Self {
        let keys: Value = serde_json::from_str(include_str!("../fixtures/node-crypto.json")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let db_text = db.to_string_lossy().into_owned();
        let mut pairs: Vec<(String, String)> = DEFAULTS
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        pairs.push(("DB_FILENAME".into(), db_text));
        pairs.push((
            "AUTH_SIGNING_PRIVKEY_B64".into(),
            keys["privB64"].as_str().unwrap().into(),
        ));
        pairs.push((
            "AUTH_SIGNING_PUBKEY_B64".into(),
            keys["pubB64"].as_str().unwrap().into(),
        ));
        for (name, value) in env {
            pairs.retain(|(k, _)| k != name);
            pairs.push(((*name).to_owned(), (*value).to_owned()));
        }
        let env = Env::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let config = Config::from_env(&env).unwrap();
        let progression = metagame::progression_config::from_env(&env).unwrap();
        let state = metagame::server::prepare(&config, progression).unwrap().state;
        let app = Self {
            router: metagame::server::app(state.clone(), false),
            state,
            db,
            _dir: dir,
        };
        let hash = sha256_hex(GS_KEY);
        app.exec(&format!("INSERT INTO gameserverapikeys (keyHash) VALUES ('{hash}')"));
        app
    }

    pub async fn call(&self, method: &str, path: &str, req: Req) -> Reply {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:61000");
        if let Some(user) = &req.as_user {
            let token = metagame::auth::sign_token(&self.state, user).await.unwrap().unwrap();
            builder = builder.header("authorization", format!("bearer {token}"));
        }
        if req.gs {
            builder = builder.header("x-undaunted-gameserver-apikey", GS_KEY);
        }
        if req.body.is_some() {
            builder = builder.header("content-type", "application/json; charset=utf-8");
        }
        for (name, value) in &req.headers {
            builder = builder.header(name, value);
        }
        let mut http = builder.body(req.body.map_or_else(Body::empty, Body::from)).unwrap();
        let peer = req.peer.as_deref().unwrap_or("127.0.0.1");
        http.extensions_mut().insert(ConnectInfo(ConnectionInfo {
            peer: format!("{peer}:50000").parse().unwrap(),
            local: "127.0.0.1:61000".parse().unwrap(),
        }));
        let response = self.router.clone().oneshot(http).await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let json = serde_json::from_str(&text).unwrap_or(Value::Null);
        Reply {
            status,
            json,
            text,
            headers,
        }
    }

    pub async fn get(&self, path: &str, req: Req) -> Reply {
        self.call("GET", path, req).await
    }

    pub async fn post(&self, path: &str, req: Req) -> Reply {
        self.call("POST", path, req).await
    }

    /// `f` on the app's write connection, as the routes run their database work.
    pub async fn db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut store::Connection) -> diesel::QueryResult<T> + Send + 'static,
    ) -> T {
        self.try_db(f).await.unwrap()
    }

    pub async fn try_db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut store::Connection) -> diesel::QueryResult<T> + Send + 'static,
    ) -> Result<T, store::Error> {
        self.state
            .store
            .write(move |conn| f(conn).map_err(store::Error::from))
            .await
    }

    fn connection(&self) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        conn
    }

    /// SQL run as it is (several statements allowed).
    pub fn exec(&self, sql: &str) {
        self.connection().execute_batch(sql).unwrap();
    }

    /// One statement with parameters.
    pub fn run(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) {
        self.connection().execute(sql, params).unwrap();
    }

    /// Every row of a query, as JSON objects by column name.
    pub fn rows(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Vec<Value> {
        let conn = self.connection();
        let mut statement = conn.prepare(sql).unwrap();
        let names: Vec<String> = statement.column_names().iter().map(|n| (*n).to_owned()).collect();
        let rows = statement
            .query_map(params, |row| {
                let mut object = Map::new();
                for (i, name) in names.iter().enumerate() {
                    let value = match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => Value::Null,
                        rusqlite::types::ValueRef::Integer(n) => Value::from(n),
                        rusqlite::types::ValueRef::Real(f) => Value::from(f),
                        rusqlite::types::ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
                        rusqlite::types::ValueRef::Blob(b) => Value::from(b.to_vec()),
                    };
                    object.insert(name.clone(), value);
                }
                Ok(Value::Object(object))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// The first row, or Null.
    pub fn row(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Value {
        self.rows(sql, params).into_iter().next().unwrap_or(Value::Null)
    }

    /// `helpers.ts`'s `Count`.
    pub fn count(&self, table: &str, filter: &str, params: &[&dyn rusqlite::ToSql]) -> i64 {
        self.connection()
            .query_row(&format!("SELECT count(*) FROM {table} WHERE {filter}"), params, |row| {
                row.get(0)
            })
            .unwrap()
    }

    /// `helpers.ts`'s `MakePlayer`: an account and its character. Returns (user id, character id).
    pub async fn make_player(&self) -> (String, String) {
        let n = PLAYERS.fetch_add(1, Ordering::SeqCst) + 1;
        let user = format!("UID-test-{}-{n}", std::process::id());
        let name = format!("Tester{n}");
        self.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, 0)",
            &[&user, &name],
        );
        let (u, nm) = (user.clone(), name.clone());
        let character = self
            .state
            .store
            .write(move |conn| metagame::characters::create(conn, &u, &nm).map_err(store::Error::from))
            .await
            .unwrap();
        (user, character.characterId)
    }

    /// `helpers.ts`'s `ReadStacks`: a character's stacked items.
    pub fn stacks(&self, character: &str) -> Vec<Value> {
        let row = self.row(
            "SELECT stackedItems FROM inventories WHERE characterId = ?1",
            &[&character],
        );
        row["stackedItems"]
            .as_str()
            .map(|text| serde_json::from_str(text).unwrap())
            .unwrap_or_default()
    }

    /// `helpers.ts`'s `StackQuantity`.
    pub fn stack_quantity(&self, character: &str, catalog: &str) -> i64 {
        self.stacks(character)
            .iter()
            .find(|item| item["catalogId"] == catalog)
            .and_then(|item| item["quantity"].as_i64())
            .unwrap_or(0)
    }
}

pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
