#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The chat's parity with the Node chat (MIGRATION_PLAN.md M32-M34): each `tests/chatparity/<name>.chat.json`
//! script runs against the Rust chat, and what every scripted WebSocket client receives must be what
//! the Node chat sent it (`<name>.chat.expected.json`, written by `tests/chatparity/record.cjs`), step
//! by step and frame by frame.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use base64::Engine;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use futures_util::{SinkExt, StreamExt};
use metagame::config::Config;
use regex::Regex;
use serde_json::{Value, json};
use service_kit::env::Env;
use service_kit::serve::ConnectionInfo;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

/// Kept in step with normalize() in tests/chatparity/record.cjs.
fn normalize(text: &str) -> String {
    let rules = [
        (r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+", "<jwt>"),
        (r#"<open (.*?)id="[0-9a-f]{16}""#, r#"<open ${1}id="<id>""#),
        (r"srv-[0-9a-f]{32}", "srv-<hex>"),
        (r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z", "<date>"),
    ];
    rules.iter().fold(text.to_owned(), |text, (pattern, with)| {
        Regex::new(pattern).unwrap().replace_all(&text, *with).into_owned()
    })
}

/// What a client heard: its frames, and how its connection closed.
enum Heard {
    Frame(String),
    Closed(Value),
}

struct Client {
    sender: mpsc::UnboundedSender<Message>,
    heard: mpsc::UnboundedReceiver<Heard>,
}

async fn connect(port: u16, headers: &serde_json::Map<String, Value>) -> Result<Client, Value> {
    let mut request = format!("ws://127.0.0.1:{port}/").into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", "xmpp".parse().unwrap());
    for (name, value) in headers {
        request.headers_mut().insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.as_str().unwrap().parse().unwrap(),
        );
    }
    let socket = match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            return Err(json!(format!("http {}", response.status().as_u16())));
        }
        Err(error) => panic!("connect: {error}"),
    };
    let (mut sink, mut stream) = socket.split();
    let (sender, mut outgoing) = mpsc::unbounded_channel::<Message>();
    let (heard_tx, heard) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });
    tokio::spawn(async move {
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => {
                    let _ = heard_tx.send(Heard::Frame(normalize(&text)));
                }
                Some(Ok(Message::Close(frame))) => {
                    let code = frame.map_or(1005, |f| u16::from(f.code));
                    let _ = heard_tx.send(Heard::Closed(json!(code)));
                    break;
                }
                Some(Ok(_)) => {}
                // Cut without a close frame: ws reports 1006
                Some(Err(_)) | None => {
                    let _ = heard_tx.send(Heard::Closed(json!(1006)));
                    break;
                }
            }
        }
    });
    Ok(Client { sender, heard })
}

async fn run_chat(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/chatparity");
    let script: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.chat.json"))).unwrap()).unwrap();
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.chat.expected.json"))).unwrap())
            .unwrap();
    let keys: Value = serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("undaunted.db");
    let mut pairs: Vec<(String, String)> = vec![
        ("PORT".into(), "0".into()),
        ("CHAT_PORT".into(), "61099".into()),
        ("DB_FILENAME".into(), db.to_string_lossy().into_owned()),
        (
            "AUTH_SIGNING_PRIVKEY_B64".into(),
            keys["privB64"].as_str().unwrap().into(),
        ),
        (
            "AUTH_SIGNING_PUBKEY_B64".into(),
            keys["pubB64"].as_str().unwrap().into(),
        ),
        ("MATCHMAKING_MODE".into(), "DISABLED".into()),
    ];
    for (name, value) in script["env"].as_object().unwrap() {
        pairs.push((name.clone(), value.as_str().unwrap().into()));
    }
    let env = Env::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let config = Config::from_env(&env).unwrap();
    let progression = metagame::progression_config::from_env(&env).unwrap();
    let state = metagame::server::prepare(&config, progression).unwrap().state;
    {
        let mut conn = SqliteConnection::establish(db.to_str().unwrap()).unwrap();
        for sql in script["setup"].as_array().into_iter().flatten() {
            conn.batch_execute(sql.as_str().unwrap()).unwrap();
        }
    }
    // The chat on a port the system picks
    let mut settings = config.chat.clone();
    settings.port = 0;
    let options = metagame::chat::Options::from_settings(&settings);
    let chat = metagame::chat::connection::start_with(state.clone(), &settings, options)
        .await
        .unwrap();
    let app = metagame::server::app(state.clone(), false);

    let token = |uid: &str| {
        let state = state.clone();
        let uid = uid.to_owned();
        async move { metagame::auth::sign_token(&state, &uid).await.unwrap().unwrap() }
    };
    let fill = |text: String| {
        let token = &token;
        async move {
            let mut text = text;
            for (pattern, bearer) in [(r"\{\{plain:([^}]+)\}\}", false), (r"\{\{bearer:([^}]+)\}\}", true)] {
                let ids: Vec<String> = Regex::new(pattern)
                    .unwrap()
                    .captures_iter(&text)
                    .map(|c| c[1].to_owned())
                    .collect();
                for id in ids {
                    let signed = token(&id).await;
                    let with = if bearer {
                        format!("bearer {signed}")
                    } else {
                        base64::engine::general_purpose::STANDARD.encode(format!("\0{id}\0{signed}"))
                    };
                    let placeholder = format!("{{{{{}:{id}}}}}", if bearer { "bearer" } else { "plain" });
                    text = text.replace(&placeholder, &with);
                }
            }
            text
        }
    };

    let mut clients: BTreeMap<String, Client> = BTreeMap::new();
    let mut actual = Vec::new();
    let wanted = expected["results"].as_array().unwrap().clone();
    for (index, step) in script["steps"].as_array().unwrap().iter().enumerate() {
        let mut http = None;
        let mut seen = serde_json::Map::new();
        if let Some(who) = step["connect"].as_str() {
            let headers = step["headers"].as_object().cloned().unwrap_or_default();
            match connect(chat.port, &headers).await {
                Ok(client) => {
                    clients.insert(who.into(), client);
                }
                Err(closed) => {
                    seen.insert(who.into(), json!({"frames": [], "closed": closed}));
                }
            }
        } else if let Some(send) = step["send"].as_array() {
            let who = send[0].as_str().unwrap();
            let frame = fill(send[1].as_str().unwrap().to_owned()).await;
            clients[who].sender.send(Message::Text(frame.into())).unwrap();
        } else if let Some(who) = step["close"].as_str() {
            let close = Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: 1000.into(),
                reason: "".into(),
            }));
            clients[who].sender.send(close).unwrap();
        } else if let Some(request) = step.get("http") {
            let mut builder = Request::builder()
                .method(request["method"].as_str().unwrap())
                .uri(request["path"].as_str().unwrap());
            for (name, value) in request["headers"].as_object().into_iter().flatten() {
                builder = builder.header(name, fill(value.as_str().unwrap().to_owned()).await);
            }
            let body = match request.get("json") {
                Some(json) => {
                    builder = builder.header("content-type", "application/json; charset=utf-8");
                    Body::from(fill(serde_json::to_string(json).unwrap()).await)
                }
                None => Body::empty(),
            };
            let mut call = builder.body(body).unwrap();
            call.extensions_mut().insert(ConnectInfo(ConnectionInfo {
                peer: "127.0.0.1:50000".parse().unwrap(),
                local: "127.0.0.1:61000".parse().unwrap(),
            }));
            http = Some(app.clone().oneshot(call).await.unwrap().status().as_u16());
        }
        let wait = step["wait"].as_u64().or(step["settle"].as_u64()).unwrap_or(150);
        tokio::time::sleep(Duration::from_millis(wait)).await;
        // A busy runner can deliver a step's frames after the wait: keep listening, up to 3 s more,
        // while fewer frames than Node's have arrived (more, or different ones, still fail below)
        let want = wanted.get(index).map(|w| w["seen"].clone()).unwrap_or(Value::Null);
        let mut heard: BTreeMap<String, (Vec<Value>, Option<Value>)> = BTreeMap::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            for (who, client) in clients.iter_mut() {
                let entry = heard.entry(who.clone()).or_default();
                while let Ok(next) = client.heard.try_recv() {
                    match next {
                        Heard::Frame(frame) => entry.0.push(json!(frame)),
                        Heard::Closed(code) => entry.1 = Some(code),
                    }
                }
            }
            let short = clients.keys().any(|who| {
                let (frames, closed) = &heard[who];
                let expected_frames = want[who]["frames"].as_array().map_or(0, Vec::len);
                frames.len() < expected_frames || (closed.is_none() && !want[who]["closed"].is_null())
            });
            if !short || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut gone = Vec::new();
        for (who, (frames, closed)) in heard {
            if !frames.is_empty() || closed.is_some() {
                let mut entry = json!({"frames": frames});
                if let Some(code) = closed {
                    entry["closed"] = code;
                    gone.push(who.clone());
                }
                seen.insert(who, entry);
            }
        }
        for who in gone {
            clients.remove(&who);
        }
        let shown: String = serde_json::to_string(step).unwrap().chars().take(200).collect();
        let mut result = json!({"step": shown});
        if let Some(status) = http {
            result["http"] = json!(status);
        }
        result["seen"] = Value::Object(seen);
        actual.push(result);
    }
    chat.stop().await;

    let expected = expected["results"].as_array().unwrap();
    let differences: Vec<String> = expected
        .iter()
        .zip(&actual)
        .enumerate()
        .filter(|(_, (want, got))| want != got)
        .map(|(index, (want, got))| {
            format!(
                "#{index} {}\n  node: {}\n  rust: {}",
                want["step"], want["seen"], got["seen"]
            )
        })
        .collect();
    assert_eq!(expected.len(), actual.len(), "step count");
    assert!(
        differences.is_empty(),
        "{} of {} steps differ:\n{}",
        differences.len(),
        expected.len(),
        differences.join("\n")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn basic() {
    run_chat("basic").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions() {
    run_chat("sessions").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn presence() {
    run_chat("presence").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rooms() {
    run_chat("rooms").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn nicklog() {
    run_chat("nicklog").await;
}

// ---- The timers, on a clock the test moves (values from realtime/chat.ts) ----

struct Timed {
    _dir: tempfile::TempDir,
    state: metagame::app::AppState,
    chat: Option<metagame::chat::connection::Running>,
    now: std::sync::Arc<std::sync::atomic::AtomicI64>,
}

impl Timed {
    async fn start() -> Self {
        let keys: Value = serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("undaunted.db");
        let db_text = db.to_string_lossy().into_owned();
        let env = Env::from_pairs([
            ("PORT", "0"),
            ("AUTH_MODE", "APIKEY"),
            ("NODE_ENV", "production"),
            ("DB_FILENAME", db_text.as_str()),
            ("AUTH_SIGNING_PRIVKEY_B64", keys["privB64"].as_str().unwrap()),
            ("AUTH_SIGNING_PUBKEY_B64", keys["pubB64"].as_str().unwrap()),
        ]);
        let config = Config::from_env(&env).unwrap();
        let state = metagame::server::prepare(&config, metagame::progression_config::from_env(&env).unwrap())
            .unwrap()
            .state;
        SqliteConnection::establish(&db_text)
            .unwrap()
            .batch_execute("INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-a', 'Ada', 0, 0)")
            .unwrap();
        let now = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(1_000_000));
        let mut settings = config.chat.clone();
        settings.port = 0;
        let mut options = metagame::chat::Options::from_settings(&settings);
        let clock = now.clone();
        options.clock = std::sync::Arc::new(move || clock.load(std::sync::atomic::Ordering::Relaxed));
        let chat = metagame::chat::connection::start_with(state.clone(), &settings, options).await;
        Self {
            _dir: dir,
            state,
            chat,
            now,
        }
    }

    fn port(&self) -> u16 {
        self.chat.as_ref().unwrap().port
    }

    /// Connects and gives the server a moment to register the session, so its start time is read
    /// before the test moves the clock
    async fn connect(&self) -> Client {
        let client = connect(self.port(), &serde_json::Map::new()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        client
    }

    /// Moves the clock and lets a tick (every second) see it
    async fn later(&self, ms: i64) {
        self.now.fetch_add(ms, std::sync::atomic::Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(1300)).await;
    }

    async fn login(&self, client: &Client, bind: bool) {
        let token = metagame::auth::sign_token(&self.state, "UID-a").await.unwrap().unwrap();
        let plain = base64::engine::general_purpose::STANDARD.encode(format!("\0UID-a\0{token}"));
        let open = r#"<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" to="127.0.0.1:61000" version="1.0"/>"#;
        client.sender.send(Message::Text(open.into())).unwrap();
        client
            .sender
            .send(Message::Text(
                format!(r#"<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="PLAIN">{plain}</auth>"#).into(),
            ))
            .unwrap();
        if bind {
            client.sender.send(Message::Text(open.into())).unwrap();
            client
                .sender
                .send(Message::Text(
                    r#"<iq type="set" id="b"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><resource>r1</resource></bind></iq>"#.into(),
                ))
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// What the client heard since the last call.
fn drain(client: &mut Client) -> (Vec<String>, Option<Value>) {
    let mut frames = Vec::new();
    let mut closed = None;
    while let Ok(heard) = client.heard.try_recv() {
        match heard {
            Heard::Frame(frame) => frames.push(frame),
            Heard::Closed(code) => closed = Some(code),
        }
    }
    (frames, closed)
}

/// How the client's connection closed, waiting up to 5 s for it: the sweep runs on a real-time timer,
/// so a busy runner may close it later than `Timed::later`'s pause.
async fn close_code(client: &mut Client) -> Option<Value> {
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(heard) = client.heard.recv().await {
            if let Heard::Closed(code) = heard {
                return Some(code);
            }
        }
        None
    });
    wait.await.ok().flatten()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_that_never_logs_in_goes_after_15_s() {
    let timed = Timed::start().await;
    let mut client = timed.connect().await;
    timed.later(14_000).await;
    assert_eq!(drain(&mut client).1, None);
    timed.later(1_000).await;
    assert_eq!(close_code(&mut client).await, Some(json!(1006)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_login_that_never_binds_goes_after_10_s() {
    let timed = Timed::start().await;
    let mut client = timed.connect().await;
    timed.login(&client, false).await;
    let (frames, closed) = drain(&mut client);
    assert_eq!(
        frames.last().map(String::as_str),
        Some(r#"<success xmlns="urn:ietf:params:xml:ns:xmpp-sasl"/>"#)
    );
    assert_eq!(closed, None);
    timed.later(10_000).await;
    assert_eq!(close_code(&mut client).await, Some(json!(1006)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_session_is_pinged_after_50_s_and_ended_100_s_later() {
    let timed = Timed::start().await;
    let mut client = timed.connect().await;
    timed.login(&client, true).await;
    drain(&mut client);
    timed.later(49_000).await;
    assert_eq!(drain(&mut client), (vec![], None));
    timed.later(1_000).await;
    assert_eq!(
        drain(&mut client).0,
        vec![
            r#"<iq xmlns="jabber:client" type="get" id="sp1" from="127.0.0.1:61000" to="UID-a@127.0.0.1:61000/r1"><ping xmlns="urn:xmpp:ping"/></iq>"#
        ]
    );
    // Any frame answers it; the next ping comes 50 s after it
    client
        .sender
        .send(Message::Text(r#"<iq type="result" id="sp1"/>"#.into()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    timed.later(49_000).await;
    assert_eq!(drain(&mut client), (vec![], None));
    timed.later(1_000).await;
    let (frames, _) = drain(&mut client);
    assert_eq!(frames.len(), 1, "pinged again 50 s after the answer");
    assert!(frames[0].contains(r#"id="sp2""#), "{frames:?}");
    timed.later(99_000).await;
    assert_eq!(drain(&mut client), (vec![], None));
    timed.later(1_000).await;
    assert_eq!(close_code(&mut client).await, Some(json!(1006)), "no answer for 100 s");
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_sends_close_to_every_session() {
    let mut timed = Timed::start().await;
    let mut client = timed.connect().await;
    timed.login(&client, true).await;
    drain(&mut client);
    timed.chat.take().unwrap().stop().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        drain(&mut client),
        (
            vec![r#"<close xmlns="urn:ietf:params:xml:ns:xmpp-framing"/>"#.to_owned()],
            Some(json!(1000))
        )
    );
}
