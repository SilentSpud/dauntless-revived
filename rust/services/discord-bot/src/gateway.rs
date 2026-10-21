//! Discord's gateway (API v10, JSON): one shard that identifies, keeps its heartbeat, resumes after a
//! dropped connection and hands every dispatch to the bot. Written here rather than taken from
//! serenity, whose gateway pins a rustls with open advisories; TLS is the workspace's rustls on
//! aws-lc-rs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// Gateway intents.
pub const GUILDS: u64 = 1 << 0;
pub const GUILD_MEMBERS: u64 = 1 << 1;

/// Close codes after which reconnecting cannot help: authentication failed, an invalid shard, sharding
/// required, an invalid API version, invalid or disallowed intents.
const FATAL: [u16; 6] = [4004, 4010, 4011, 4012, 4013, 4014];
/// Close codes that end the session: the next connection identifies again.
const SESSION_ENDED: [u16; 2] = [4007, 4009];

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

type Socket = WebSocketStream<Box<dyn Io>>;

/// A dispatch: its event name (`t`) and data (`d`).
pub type Dispatch = (String, Value);

pub struct Gateway {
    token: String,
    intents: u64,
    url: String,
    /// READY or RESUMED has been received on the current connection.
    pub connected: AtomicBool,
}

struct Session {
    id: String,
    resume_url: String,
}

enum End {
    Stopped,
    Fatal(u16),
    /// Connect again and resume the session, if there is one.
    Resume,
    /// Connect again with a new session.
    Fresh,
}

fn tls() -> Result<tokio_rustls::TlsConnector, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

/// A WebSocket to `url`: `wss://` over TLS, `ws://` (for tests) without.
async fn connect(url: &str) -> Result<Socket, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let secure = match parsed.scheme() {
        "wss" => true,
        "ws" => false,
        other => return Err(format!("unsupported gateway scheme {other}")),
    };
    let host = parsed.host_str().ok_or("the gateway URL has no host")?.to_owned();
    let port = parsed.port_or_known_default().unwrap_or(if secure { 443 } else { 80 });
    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| e.to_string())?;
    let stream: Box<dyn Io> = if secure {
        let name = rustls::pki_types::ServerName::try_from(host).map_err(|e| e.to_string())?;
        Box::new(tls()?.connect(name, tcp).await.map_err(|e| e.to_string())?)
    } else {
        Box::new(tcp)
    };
    let (socket, _) = tokio_tungstenite::client_async(url, stream)
        .await
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

/// Resolves once `stop` turns true; never when its sender is gone without it.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    if stop.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
}

async fn send(socket: &mut Socket, payload: Value) -> Result<(), ()> {
    socket
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|_| ())
}

impl Gateway {
    /// `url` as `GET /gateway/bot` gives it, without query.
    pub fn new(token: &str, intents: u64, url: &str) -> Self {
        Self {
            token: token.to_owned(),
            intents,
            url: url.trim_end_matches('/').to_owned(),
            connected: AtomicBool::new(false),
        }
    }

    /// Connects until `stop` turns true or Discord refuses the bot for good (`Err` with the close code).
    pub async fn run(
        &self,
        events: mpsc::UnboundedSender<Dispatch>,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), u16> {
        let mut session: Option<Session> = None;
        let mut seq: Option<u64> = None;
        let mut backoff = Duration::from_secs(1);
        loop {
            if *stop.borrow() {
                return Ok(());
            }
            let url = session.as_ref().map_or(self.url.clone(), |s| s.resume_url.clone());
            let end = self.connection(&url, &mut session, &mut seq, &events, &mut stop).await;
            let was_connected = self.connected.swap(false, Ordering::SeqCst);
            if was_connected {
                backoff = Duration::from_secs(1);
            }
            let pause = match end {
                End::Stopped => return Ok(()),
                End::Fatal(code) => return Err(code),
                End::Resume if was_connected => Duration::ZERO,
                End::Resume => {
                    let pause = backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                    pause
                }
                End::Fresh => {
                    session = None;
                    seq = None;
                    // Discord asks for 1 to 5 seconds before identifying again
                    Duration::from_millis(1000 + u64::from(rand::random::<u16>() % 4000))
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(pause) => {}
                _ = stopped(&mut stop) => return Ok(()),
            }
        }
    }

    async fn connection(
        &self,
        url: &str,
        session: &mut Option<Session>,
        seq: &mut Option<u64>,
        events: &mpsc::UnboundedSender<Dispatch>,
        stop: &mut watch::Receiver<bool>,
    ) -> End {
        let Ok(mut socket) = connect(&format!("{url}/?v=10&encoding=json")).await else {
            return End::Resume;
        };
        // Hello first
        let hello = tokio::time::timeout(Duration::from_secs(30), socket.next()).await;
        let interval = match hello {
            Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text)
                .ok()
                .filter(|p| p["op"] == 10)
                .and_then(|p| p["d"]["heartbeat_interval"].as_u64()),
            _ => None,
        };
        let Some(interval) = interval.filter(|i| *i > 0).map(Duration::from_millis) else {
            return End::Resume;
        };
        let start = match session {
            Some(s) => json!({ "op": 6, "d": { "token": self.token, "session_id": s.id, "seq": seq } }),
            None => json!({ "op": 2, "d": {
                "token": self.token,
                "intents": self.intents,
                "properties": { "os": std::env::consts::OS, "browser": "dauntless-revived", "device": "dauntless-revived" },
            } }),
        };
        if send(&mut socket, start).await.is_err() {
            return End::Resume;
        }
        // The first beat comes after a random part of the interval
        let first = interval.mul_f64(f64::from(rand::random::<u16>()) / f64::from(u16::MAX));
        let mut beat = tokio::time::interval_at(tokio::time::Instant::now() + first, interval);
        let mut acked = true;
        loop {
            tokio::select! {
                _ = stopped(stop) => {
                    let _ = socket.close(Some(CloseFrame { code: CloseCode::Normal, reason: "".into() })).await;
                    return End::Stopped;
                }
                _ = beat.tick() => {
                    if !acked {
                        // No answer to the last beat: a zombie connection
                        let _ = socket.close(Some(CloseFrame { code: CloseCode::Library(4000), reason: "".into() })).await;
                        return End::Resume;
                    }
                    acked = false;
                    if send(&mut socket, json!({ "op": 1, "d": seq })).await.is_err() {
                        return End::Resume;
                    }
                }
                message = socket.next() => {
                    let text = match message {
                        Some(Ok(Message::Text(text))) => text,
                        Some(Ok(Message::Close(frame))) => {
                            let code = frame.map_or(1005, |f| u16::from(f.code));
                            return if FATAL.contains(&code) {
                                End::Fatal(code)
                            } else if SESSION_ENDED.contains(&code) {
                                End::Fresh
                            } else {
                                End::Resume
                            };
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(_)) | None => return End::Resume,
                    };
                    let Ok(payload) = serde_json::from_str::<Value>(&text) else { continue };
                    match payload["op"].as_u64() {
                        Some(0) => {
                            if let Some(s) = payload["s"].as_u64() {
                                *seq = Some(s);
                            }
                            let name = payload["t"].as_str().unwrap_or_default().to_owned();
                            let data = payload["d"].clone();
                            if name == "READY" {
                                *session = Some(Session {
                                    id: data["session_id"].as_str().unwrap_or_default().to_owned(),
                                    resume_url: data["resume_gateway_url"]
                                        .as_str()
                                        .unwrap_or(&self.url)
                                        .trim_end_matches('/')
                                        .to_owned(),
                                });
                            }
                            if name == "READY" || name == "RESUMED" {
                                self.connected.store(true, Ordering::SeqCst);
                            }
                            let _ = events.send((name, data));
                        }
                        Some(1) => {
                            if send(&mut socket, json!({ "op": 1, "d": seq })).await.is_err() {
                                return End::Resume;
                            }
                        }
                        Some(7) => {
                            let _ = socket.close(Some(CloseFrame { code: CloseCode::Library(4000), reason: "".into() })).await;
                            return End::Resume;
                        }
                        Some(9) => {
                            return if payload["d"] == true { End::Resume } else { End::Fresh };
                        }
                        Some(11) => acked = true,
                        _ => {}
                    }
                }
            }
        }
    }
}
