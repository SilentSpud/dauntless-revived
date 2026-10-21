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
