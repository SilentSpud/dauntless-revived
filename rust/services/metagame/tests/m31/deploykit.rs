//! `fakedeploy.ts`: a stand-in for the deploy server on a spare loopback port. GET /gameservers answers
//! the servers it holds; the matchmaking call "starts" a game server on its matchmaking port.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

#[derive(Clone)]
pub(crate) struct FakeDeploy {
    /// host:port, for DEPLOYSERVER_URL
    pub address: String,
    pub servers: Arc<Mutex<Value>>,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub matchmaking_port: Arc<Mutex<u16>>,
}

impl FakeDeploy {
    pub(crate) async fn start(servers: Value) -> Self {
        use axum::response::IntoResponse;
        let fake = Self {
            address: String::new(),
            servers: Arc::new(Mutex::new(servers)),
            requests: Arc::default(),
            matchmaking_port: Arc::new(Mutex::new(8775)),
        };
        let shared = fake.clone();
        let handler = move |method: axum::http::Method, uri: axum::http::Uri| {
            let fake = shared.clone();
            async move {
                fake.requests.lock().unwrap().push(format!("{method} {uri}"));
                let reply = match (method.as_str(), uri.path()) {
                    ("GET", "/gameservers") => json!({"servers": *fake.servers.lock().unwrap()}),
                    ("POST", "/api/matchmaker/handle-matchmaking-for-player") => {
                        json!({"host": "127.0.0.1", "port": *fake.matchmaking_port.lock().unwrap()})
                    }
                    _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
                };
                ([("content-type", "application/json")], reply.to_string()).into_response()
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            axum::serve(listener, axum::Router::new().fallback(handler))
                .await
                .unwrap();
        });
        Self { address, ..fake }
    }

    pub(crate) fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

/// Three servers as the deploy server reports them: Ramsgate, a hunt and the tutorial
pub(crate) fn three_servers(hunter: &str) -> Value {
    json!([
        {"id": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d01", "port": 8777, "kind": "city", "map": "/Game/Maps/ramsgate/ramsgate_01_persistent", "gameMode": null,
            "behemoth": null, "huntId": null, "matchmakerHuntId": null, "expectedPlayers": [], "maxPlayers": null, "startedAt": "2026-09-21T10:00:00.000Z"},
        {"id": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d02", "port": 8774, "kind": "hunt", "map": "/Game/Maps/islands/1702/cora_jamima", "gameMode": null,
            "behemoth": "/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C", "huntId": "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr",
            "matchmakerHuntId": "CR19_MatchmakerHunt_Lerawr_Beta", "expectedPlayers": [hunter], "maxPlayers": 4, "startedAt": "2026-09-21T11:00:00.000Z"},
        {"id": "5c0a9e36-8a51-4c1b-9d7e-1f2a3b4c5d03", "port": 8775, "kind": "tutorial", "map": "/Game/Maps/islands/1705/dia_moss_triforce", "gameMode": null,
            "behemoth": "/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C", "huntId": null, "matchmakerHuntId": null,
            "expectedPlayers": [], "maxPlayers": 1, "startedAt": "2026-09-21T11:30:00.000Z"}
    ])
}

/// `partyenv.ts`'s `RecordingDeploy`: records every matchmaking body. Ramsgate is 8777, the Dojo 8776,
/// hunts get 8775, 8774, ... unless `fail` is set (then it answers 500) or `capacity` (503).
#[derive(Clone, Default)]
pub(crate) struct RecordingDeploy {
    pub address: String,
    pub state: Arc<Mutex<Recorded>>,
}

pub(crate) struct Recorded {
    pub calls: Vec<Value>,
    pub fail: bool,
    pub capacity: bool,
    pub delay_ms: u64,
    pub next_hunt_port: u16,
}

impl Default for Recorded {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            fail: false,
            capacity: false,
            delay_ms: 0,
            next_hunt_port: 8775,
        }
    }
}

impl RecordingDeploy {
    pub(crate) async fn start() -> Self {
        use axum::response::IntoResponse;
        let state: Arc<Mutex<Recorded>> = Arc::default();
        let shared = state.clone();
        let handler = move |method: axum::http::Method, uri: axum::http::Uri, body: axum::body::Bytes| {
            let state = shared.clone();
            async move {
                if method != axum::http::Method::POST || uri.path() != "/api/matchmaker/handle-matchmaking-for-player" {
                    return axum::http::StatusCode::NOT_FOUND.into_response();
                }
                let parsed: Value = serde_json::from_slice(&body).unwrap();
                let delay = {
                    let mut state = state.lock().unwrap();
                    state.calls.push(parsed.clone());
                    state.delay_ms
                };
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                let mut state = state.lock().unwrap();
                if state.capacity {
                    let body = json!({"error": "capacity_unavailable", "reason": "memory"}).to_string();
                    return (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response();
                }
                if state.fail {
                    return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
                let port = match parsed["GameMode"].as_str() {
                    Some("CITY") => 8777,
                    Some("SHARED") => 8776,
                    _ => {
                        state.next_hunt_port -= 1;
                        state.next_hunt_port + 1
                    }
                };
                (
                    [("content-type", "application/json")],
                    json!({"host": "127.0.0.1", "port": port}).to_string(),
                )
                    .into_response()
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            axum::serve(listener, axum::Router::new().fallback(handler))
                .await
                .unwrap();
        });
        Self { address, state }
    }

    pub(crate) fn calls(&self) -> Vec<Value> {
        self.state.lock().unwrap().calls.clone()
    }
}

/// How a scripted deploy server answers one call.
#[derive(Clone)]
pub(crate) enum Answer {
    /// Status, body and any extra headers
    Reply(u16, String, Vec<(String, String)>),
    /// The headers at once, the body only when released
    SlowBody(u16, String),
    /// The connection is closed without an answer
    Drop,
}

impl Answer {
    pub(crate) fn json(status: u16, body: Value) -> Self {
        Self::Reply(status, body.to_string(), Vec::new())
    }
}

type Script = Box<dyn Fn(&Value, usize) -> Answer + Send + Sync>;
type Health = Box<dyn Fn() -> Answer + Send + Sync>;

/// A deploy server stand-in on raw TCP, so a test can hold an answer, drop the connection or send
/// any body (the Node tests replaced `fetch` or wrote to the socket). Every matchmaking call's body
/// is recorded; `script` picks its answer from the body and the call's number (from 1), and the
/// health snapshot (GET /gameservers) answers from `health`. With `hold` set, answers wait for
/// `release`; a held call whose client gives up is counted in `aborted`.
#[derive(Clone)]
pub(crate) struct ScriptedDeploy {
    pub address: String,
    pub calls: Arc<Mutex<Vec<Value>>>,
    pub health_calls: Arc<std::sync::atomic::AtomicUsize>,
    pub aborted: Arc<std::sync::atomic::AtomicUsize>,
    script: Arc<Mutex<Script>>,
    health: Arc<Mutex<Health>>,
    hold: Arc<Mutex<bool>>,
    hold_health: Arc<Mutex<bool>>,
    releases: tokio::sync::watch::Sender<u64>,
}

impl ScriptedDeploy {
    pub(crate) async fn start(script: impl Fn(&Value, usize) -> Answer + Send + Sync + 'static) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let deploy = Self {
            address,
            calls: Arc::default(),
            health_calls: Arc::default(),
            aborted: Arc::default(),
            script: Arc::new(Mutex::new(Box::new(script))),
            health: Arc::new(Mutex::new(Box::new(|| {
                Answer::json(200, json!({"servers": [], "complete": false}))
            }))),
            hold: Arc::default(),
            hold_health: Arc::default(),
            releases: tokio::sync::watch::channel(0).0,
        };
        let shared = deploy.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(shared.clone().answer(stream));
            }
        });
        deploy
    }

    /// Waits for `release`; false when the client closed the connection first.
    async fn held(&self, stream: &mut tokio::net::TcpStream) -> bool {
        use tokio::io::AsyncReadExt;
        let mut releases = self.releases.subscribe();
        let seen = *releases.borrow_and_update();
        let mut byte = [0u8; 1];
        loop {
            tokio::select! {
                changed = releases.changed() => {
                    if changed.is_err() || *releases.borrow() != seen {
                        return true;
                    }
                }
                read = stream.read(&mut byte) => {
                    if matches!(read, Ok(0) | Err(_)) {
                        self.aborted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return false;
                    }
                }
            }
        }
    }

    async fn answer(self, mut stream: tokio::net::TcpStream) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut data = Vec::new();
        let mut buffer = [0u8; 4096];
        let (request_line, body) = loop {
            let Ok(read) = stream.read(&mut buffer).await else {
                return;
            };
            if read == 0 {
                return;
            }
            data.extend_from_slice(&buffer[..read]);
            let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&data[..end]).to_string();
            let length = head
                .to_lowercase()
                .lines()
                .find_map(|line| line.strip_prefix("content-length:").map(|v| v.trim().to_owned()))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                let first = head.lines().next().unwrap_or_default().to_owned();
                break (first, data[end + 4..end + 4 + length].to_vec());
            }
        };
        let (answer, held) = if request_line.starts_with("GET ") {
            self.health_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ((self.health.lock().unwrap())(), *self.hold_health.lock().unwrap())
        } else {
            let parsed: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
            let mut calls = self.calls.lock().unwrap();
            calls.push(parsed.clone());
            let number = calls.len();
            (
                (self.script.lock().unwrap())(&parsed, number),
                *self.hold.lock().unwrap(),
            )
        };
        let slow = matches!(answer, Answer::SlowBody(..));
        if held && !slow && !self.held(&mut stream).await {
            return;
        }
        match answer {
            Answer::Drop => drop(stream),
            Answer::Reply(status, body, headers) => {
                let mut response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&body);
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
            Answer::SlowBody(status, body) => {
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                if !self.held(&mut stream).await {
                    return;
                }
                let _ = stream.write_all(body.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        }
    }

    pub(crate) fn set(&self, script: impl Fn(&Value, usize) -> Answer + Send + Sync + 'static) {
        *self.script.lock().unwrap() = Box::new(script);
    }

    pub(crate) fn set_health(&self, health: impl Fn() -> Answer + Send + Sync + 'static) {
        *self.health.lock().unwrap() = Box::new(health);
    }

    /// Answers wait for `release` from now on (true) or go out at once (false).
    pub(crate) fn hold(&self, hold: bool) {
        *self.hold.lock().unwrap() = hold;
    }

    pub(crate) fn hold_health(&self, hold: bool) {
        *self.hold_health.lock().unwrap() = hold;
    }

    /// Sends every held answer.
    pub(crate) fn release(&self) {
        self.releases.send_modify(|generation| *generation += 1);
    }

    pub(crate) fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }

    /// Waits up to 2 s for `count` calls, then checks there are exactly that many.
    pub(crate) async fn wait_for_calls(&self, count: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.calls().len() < count && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(self.calls().len(), count);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
