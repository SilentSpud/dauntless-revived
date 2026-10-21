//! The chat listener (`ChatServer` in `realtime/chat.ts`): HTTP on its own loopback port, where any
//! WebSocket upgrade becomes a chat connection and any other request is a 404. Each connection has a
//! reader, which hands its frames to the chat task in order, and a writer, which sends what the chat
//! task gives it and counts what still waits (Node's `bufferedAmount`).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use service_kit::serve::ConnectionInfo;
use tokio::sync::{Notify, mpsc, oneshot};

use super::{Chat, Event, FRAME_LIMIT, Options, Out, Outbox, Settings};
use crate::app::AppState;
use crate::origin::Origin;

/// How long a close we sent may wait for the client's answer before the connection is cut (ws's
/// closeTimeout).
const CLOSE_WAIT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct Listener {
    app: AppState,
    events: mpsc::UnboundedSender<Event>,
}

/// Chat while it runs.
pub struct Running {
    events: mpsc::UnboundedSender<Event>,
    task: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
    stop_listening: oneshot::Sender<()>,
    pub port: u16,
    /// The most unsent output held for one session so far
    pub largest_backlog: Arc<AtomicUsize>,
}

impl Running {
    /// One tick now, for a chat started with `auto_tick` off.
    pub async fn tick(&self) {
        let (done, finished) = oneshot::channel();
        if self.events.send(Event::Tick(done)).is_ok() {
            let _ = finished.await;
        }
    }

    /// Shutdown: `<close/>` to every session, at most 1 s for them to go, then the listener stops.
    pub async fn stop(self) {
        let (done, finished) = oneshot::channel();
        if self.events.send(Event::Shutdown(done)).is_ok() {
            let _ = finished.await;
        }
        let _ = self.task.await;
        let _ = self.stop_listening.send(());
        let _ = self.server.await;
    }
}

/// `StartChat`: chat as the settings say. Never fails the metagame: a bad setting or a port in use
/// is one error line, and the metagame serves on without chat.
pub async fn start(app: AppState, settings: &Settings) -> Option<Running> {
    for warning in &settings.warnings {
        tracing::warn!("chat: {warning}");
    }
    if !settings.enabled {
        tracing::info!("chat: disabled by CHAT setting; use CHAT=1 and restart to enable in-game chat");
        return None;
    }
    if !settings.errors.is_empty() {
        tracing::error!(
            "chat: not started: {}. The metagame runs without chat.",
            settings.errors.join("; ")
        );
        return None;
    }
    start_with(app, settings, Options::from_settings(settings)).await
}

/// `start` with a test's options (its clock, its output limit).
pub async fn start_with(app: AppState, settings: &Settings, options: Options) -> Option<Running> {
    let host: std::net::IpAddr = match settings.host.parse() {
        Ok(host) => host,
        Err(_) => {
            tracing::error!("chat: not started: CHAT_BIND_HOST is not an address. The metagame runs without chat.");
            return None;
        }
    };
    let listener = match tokio::net::TcpListener::bind(SocketAddr::new(host, settings.port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(
                "chat: not started: could not listen on {}:{} ({}). The metagame runs without chat.",
                settings.host,
                settings.port,
                error_code(&error)
            );
            return None;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(settings.port);
    tracing::info!(
        "chat: listening on {}:{port} (nick check {})",
        settings.host,
        options.nick_check.as_str()
    );
    if options.presence {
        tracing::info!(
            "chat: friends' online status on (CHAT_PRESENCE=1): each player's presence goes to their online friends, never back to the player"
        );
    } else {
        tracing::info!("chat: friends' online status off: no presence is sent outside rooms");
    }
    let (events, receiver) = mpsc::unbounded_channel();
    let chat = Chat::new(app.clone(), options);
    let largest_backlog = chat.largest_backlog.clone();
    let task = tokio::spawn(super::run(chat, receiver));
    let router = axum::Router::new().fallback(upgrade).with_state(Listener {
        app,
        events: events.clone(),
    });
    let (stop_listening, stopped) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let serving = axum::serve(
            service_kit::serve::NoDelay(listener),
            router.into_make_service_with_connect_info::<ConnectionInfo>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stopped.await;
        });
        if let Err(error) = serving.await {
            tracing::error!("chat: listener error ({})", error.kind());
        }
    });
    Some(Running {
        events,
        task,
        server,
        stop_listening,
        port,
        largest_backlog,
    })
}

/// Node's error code for a failed listen (EADDRINUSE and the like).
fn error_code(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "EADDRINUSE",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL",
        _ => "Error",
    }
}

async fn upgrade(
    State(listener): State<Listener>,
    ConnectInfo(connection): ConnectInfo<ConnectionInfo>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(ws) = ws else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let origin = Origin {
        connection,
        headers: &headers,
    };
    let address = origin.client_address(&listener.app.origin).to_string();
    let via = if origin.is_trusted_gateway(&listener.app.origin) {
        "gateway"
    } else {
        "direct"
    };
    let (reply, admitted) = oneshot::channel();
    let asked = listener.events.send(Event::Admit {
        address: address.clone(),
        via,
        reply,
    });
    if asked.is_err() || !admitted.await.unwrap_or(false) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONNECTION, "close"), (header::CONTENT_LENGTH, "0")],
        )
            .into_response();
    }
    // ws answers with the first subprotocol the client offers (the game offers "xmpp")
    let offered = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').map(str::trim).find(|p| !p.is_empty()))
        .map(str::to_owned);
    let ws = ws.max_message_size(FRAME_LIMIT).max_frame_size(FRAME_LIMIT);
    let ws = match offered {
        Some(protocol) => ws.protocols([protocol]),
        None => ws,
    };
    let events = listener.events.clone();
    ws.on_failed_upgrade(|error| tracing::warn!("chat: upgrade failed ({error})"))
        .on_upgrade(move |socket| connection_task(socket, events, address, via))
}

async fn connection_task(socket: WebSocket, events: mpsc::UnboundedSender<Event>, address: String, via: &'static str) {
    let (sender, mut outgoing) = mpsc::unbounded_channel::<Out>();
    let queued = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    let terminate = Arc::new(Notify::new());
    let out = Outbox {
        sender,
        queued: queued.clone(),
        closed: closed.clone(),
        terminate: terminate.clone(),
    };
    let (reply, id) = oneshot::channel();
    if events
        .send(Event::Connected {
            address,
            via,
            out,
            reply,
        })
        .is_err()
    {
        return;
    }
    let Ok(id) = id.await else {
        return;
    };
    let (mut sink, mut stream) = socket.split();
    let cut = terminate.clone();
    let writer = tokio::spawn(async move {
        while let Some(out) = outgoing.recv().await {
            match out {
                Out::Text(text) => {
                    let length = text.len();
                    let sent = sink.send(Message::Text(text.into())).await;
                    queued.fetch_sub(length, Ordering::Relaxed);
                    if sent.is_err() {
                        break;
                    }
                }
                Out::Close(code) => {
                    let _ = sink
                        .send(Message::Close(Some(CloseFrame {
                            code,
                            reason: "".into(),
                        })))
                        .await;
                    // The client answers, or the connection is cut after a while
                    let later = cut.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(CLOSE_WAIT).await;
                        later.notify_one();
                    });
                    break;
                }
                Out::Terminate => {
                    cut.notify_one();
                    break;
                }
            }
        }
        // A close the client sent is answered before the socket goes (tungstenite queued the
        // reply), as ws answers one
        let _ = tokio::time::timeout(Duration::from_millis(200), sink.flush()).await;
        sink
    });
    loop {
        tokio::select! {
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let _ = events.send(Event::Frame { id, text: text.to_string() });
                }
                // Text and binary frames alike are read as UTF-8 (the game sends text)
                Some(Ok(Message::Binary(bytes))) => {
                    let _ = events.send(Event::Frame { id, text: String::from_utf8_lossy(&bytes).into_owned() });
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_))) | None => {
                    let _ = events.send(Event::Gone { id, size: false });
                    break;
                }
                Some(Err(error)) => {
                    let size = is_too_long(&error);
                    let _ = events.send(Event::Gone { id, size });
                    break;
                }
            },
            () = terminate.notified() => break,
        }
    }
    // The writer goes once the chat task has ended the session; then the connection closes
    if let Ok(Ok(sink)) = tokio::time::timeout(CLOSE_WAIT, writer).await {
        drop(sink);
    }
    closed.store(true, Ordering::Relaxed);
}

/// An oversized frame or message (ws's `WS_ERR_UNSUPPORTED_MESSAGE_LENGTH`).
fn is_too_long(error: &axum::Error) -> bool {
    let text = error.to_string().to_lowercase();
    text.contains("too long") || text.contains("too big") || text.contains("capacity")
}
