//! A port of the Node tests' `chatwire.ts` and `chatinvariant.ts`: a raw chat client shaped like the
//! 1.4.4 client's connection (upgrade to "//" with protocol xmpp; stanzas without a jabber:client
//! namespace; bind id _xmpp_bind1), and a chat server on port 0 for a test app. Every frame a client
//! receives is kept in order, so a test can feed them to the client model (`chatclient.rs`).
//!
//! Every server started here checks every stanza it sends: none outside a room comes from the
//! receiving player's own account (docs/findings/chat.md, party safety). `stop` fails the test when
//! one did.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use metagame::app::AppState;
use metagame::chat::connection::Running;
use metagame::chat::{Options, Settings};
use rand::RngExt;
use service_kit::env::Env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub(crate) const DOMAIN: &str = "prod.ol.epicgames.com";
pub(crate) const FRAMING: &str = "urn:ietf:params:xml:ns:xmpp-framing";

/// The resource the game binds: V2:<AppId>:WIN::<32 hex>, a new one at every login
pub(crate) fn game_resource() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("V2:MissingGameServiceForAppId:WIN::{hex}")
}

pub(crate) fn base64_plain(authz: &str, authc: &str, token: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("{authz}\0{authc}\0{token}"))
}

pub(crate) async fn token(state: &AppState, uid: &str) -> String {
    metagame::auth::sign_token(state, uid).await.unwrap().unwrap()
}

// ---- The test clock ----

/// The chat's clock in a test (`Clock: () => Now`).
#[derive(Clone)]
pub(crate) struct TestClock(Arc<AtomicI64>);

impl TestClock {
    pub(crate) fn at(iso: &str) -> Self {
        Self(Arc::new(AtomicI64::new(metagame::js::date_parse(iso).unwrap())))
    }

    pub(crate) fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

// ---- The invariant (chatinvariant.ts) ----

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The stanza's own "from" (on its first tag only), when it is an account's JID outside a room
fn sender_account(stanza: &str) -> Option<String> {
    let head = regex::Regex::new(r"^<[^>]*>").unwrap();
    let head = head.find(stanza).map(|m| m.as_str()).unwrap_or_default();
    let from = regex::Regex::new(r#"\sfrom=(?:"([^"]*)"|'([^']*)')"#).unwrap();
    let captures = from.captures(head)?;
    let jid = unescape(
        captures
            .get(1)
            .or(captures.get(2))
            .map(|m| m.as_str())
            .unwrap_or_default(),
    );
    let bare = jid.split('/').next().unwrap_or_default();
    let at = bare.find('@').filter(|at| *at > 0)?;
    // A room's stanzas come from muc.<domain> (or conference.<domain>) and carry a nickname, not a sender
    let room = regex::Regex::new(r"(?i)^(muc|conference)\.").unwrap();
    (!room.is_match(&bare[at + 1..])).then(|| bare[..at].to_owned())
}

/// The rule itself: true when a stanza to `uid`'s session breaks it
pub(crate) fn is_self_stanza(uid: Option<&str>, stanza: &str) -> bool {
    match (uid, sender_account(stanza)) {
        (Some(uid), Some(sender)) => sender.to_lowercase() == uid.to_lowercase(),
        _ => false,
    }
}

// ---- The server ----

/// A chat server on port 0 for a test app (`new ChatServer(...)`, `listen(0)`).
pub(crate) struct ChatServer {
    running: Option<Running>,
    pub port: u16,
    self_stanzas: Arc<Mutex<Vec<String>>>,
    checked: Arc<AtomicUsize>,
}

/// The options every test server starts from: chat's defaults with no settings, ticking on its own
/// (`new ChatServer()`), checked by the invariant.
pub(crate) fn options() -> Options {
    Options::from_settings(&Settings::from_env(&Env::from_pairs(std::iter::empty())))
}

/// `Clock: () => Now, AutoTick: false`
pub(crate) fn options_at(clock: &TestClock) -> Options {
    let mut options = options();
    let now = clock.0.clone();
    options.clock = Arc::new(move || now.load(Ordering::SeqCst));
    options.auto_tick = false;
    options
}

impl ChatServer {
    pub(crate) async fn start(state: &AppState, mut options: Options) -> Self {
        // Log capture starts before the server's first line
        crate::common::logged_lines();
        let self_stanzas: Arc<Mutex<Vec<String>>> = Arc::default();
        let checked: Arc<AtomicUsize> = Arc::default();
        let (seen, count) = (self_stanzas.clone(), checked.clone());
        options.observe = Some(Arc::new(move |uid: Option<&str>, stanza: &str| {
            count.fetch_add(1, Ordering::SeqCst);
            if is_self_stanza(uid, stanza) {
                let shown: String = stanza.chars().take(300).collect();
                seen.lock()
                    .unwrap()
                    .push(format!("to {}: {shown}", uid.unwrap_or("undefined")));
            }
        }));
        let mut settings = Settings::from_env(&Env::from_pairs(std::iter::empty()));
        settings.port = 0;
        let running = metagame::chat::connection::start_with(state.clone(), &settings, options)
            .await
            .expect("chat started");
        Self {
            port: running.port,
            running: Some(running),
            self_stanzas,
            checked,
        }
    }

    /// `Tick()`
    pub(crate) async fn tick(&self) {
        self.running.as_ref().unwrap().tick().await;
    }

    /// `LargestBacklog`
    pub(crate) fn largest_backlog(&self) -> usize {
        self.running.as_ref().unwrap().largest_backlog.load(Ordering::Relaxed)
    }

    /// Stanzas the invariant saw
    pub(crate) fn checked(&self) -> usize {
        self.checked.load(Ordering::SeqCst)
    }

    /// `close()`, then the invariant: no stanza outside a room came from the receiving player's own
    /// account
    pub(crate) async fn stop(mut self) {
        if let Some(running) = self.running.take() {
            running.stop().await;
        }
        assert_eq!(
            *self.self_stanzas.lock().unwrap(),
            Vec::<String>::new(),
            "no stanza outside a room came from the receiving player's own account"
        );
    }
}

// ---- The client ----

static NEXT_BARRIER: AtomicU64 = AtomicU64::new(1);

pub(crate) struct WireClient {
    sender: mpsc::UnboundedSender<Message>,
    queue: Arc<(Mutex<VecDeque<String>>, Notify)>,
    /// Every frame received, in order
    pub frames: Arc<Mutex<Vec<String>>>,
    closed: watch::Receiver<Option<u16>>,
    /// Answer the server's liveness pings, as the game does (they are kept in `frames`, not queued)
    pub auto_pong: Arc<AtomicBool>,
    paused: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    terminated: Arc<AtomicBool>,
    pub uid: String,
    pub resource: String,
    pub domain: String,
}

impl WireClient {
    /// Connects as the game does; `Err(status)` when the upgrade is refused.
    pub(crate) async fn connect(port: u16, headers: &[(&str, &str)]) -> Result<Self, u16> {
        let mut request = format!("ws://127.0.0.1:{port}//").into_client_request().unwrap();
        let wanted = [
            ("sec-websocket-protocol", "xmpp"),
            ("pragma", "no-cache"),
            ("cache-control", "no-cache"),
            ("origin", "http://127.0.0.1"),
        ];
        for (name, value) in wanted.iter().chain(headers) {
            request.headers_mut().insert(
                axum::http::HeaderName::from_bytes(name.to_lowercase().as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        let socket = match tokio_tungstenite::connect_async(request).await {
            Ok((socket, _)) => socket,
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => return Err(response.status().as_u16()),
            Err(error) => panic!("connect: {error}"),
        };
        let (mut sink, mut stream) = socket.split();
        let (sender, mut outgoing) = mpsc::unbounded_channel::<Message>();
        let queue: Arc<(Mutex<VecDeque<String>>, Notify)> = Arc::default();
        let frames: Arc<Mutex<Vec<String>>> = Arc::default();
        let (closed_tx, closed) = watch::channel(None);
        let auto_pong = Arc::new(AtomicBool::new(true));
        let (paused, mut paused_rx) = watch::channel(false);
        let writer = tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });
        let ping =
            regex::Regex::new(r#"^<iq [^>]*type="get" id="(sp\d+)"[^>]*><ping xmlns="urn:xmpp:ping"/></iq>$"#).unwrap();
        let (reader_queue, reader_frames, pong, replies) =
            (queue.clone(), frames.clone(), auto_pong.clone(), sender.clone());
        let reader = tokio::spawn(async move {
            let code = loop {
                while *paused_rx.borrow_and_update() {
                    if paused_rx.changed().await.is_err() {
                        break;
                    }
                }
                let text = match stream.next().await {
                    Some(Ok(Message::Text(text))) => text.to_string(),
                    Some(Ok(Message::Binary(data))) => String::from_utf8_lossy(&data).into_owned(),
                    Some(Ok(Message::Close(frame))) => break frame.map_or(1005, |f| u16::from(f.code)),
                    Some(Ok(_)) => continue,
                    Some(Err(_)) | None => break 1006,
                };
                reader_frames.lock().unwrap().push(text.clone());
                if let Some(id) = ping.captures(&text).map(|c| c[1].to_owned())
                    && pong.load(Ordering::SeqCst)
                {
                    let _ = replies.send(Message::Text(format!(r#"<iq type="result" id="{id}"/>"#).into()));
                    continue;
                }
                reader_queue.0.lock().unwrap().push_back(text);
                reader_queue.1.notify_waiters();
            };
            let _ = closed_tx.send(Some(code));
            reader_queue.1.notify_waiters();
        });
        Ok(Self {
            sender,
            queue,
            frames,
            closed,
            auto_pong,
            paused,
            tasks: vec![writer, reader],
            terminated: Arc::default(),
            uid: String::new(),
            resource: String::new(),
            domain: DOMAIN.to_owned(),
        })
    }

    pub(crate) fn is_open(&self) -> bool {
        self.closed.borrow().is_none() && !self.terminated.load(Ordering::SeqCst)
    }

    pub(crate) fn send(&self, stanza: &str) {
        let _ = self.sender.send(Message::Text(stanza.to_owned().into()));
    }

    pub(crate) fn send_binary(&self, data: &[u8]) {
        let _ = self.sender.send(Message::Binary(data.to_vec().into()));
    }

    /// Stops reading (`Socket.pause()`): the server's output waits in the connection.
    pub(crate) fn pause(&self) {
        let _ = self.paused.send(true);
    }

    pub(crate) fn resume(&self) {
        let _ = self.paused.send(false);
    }

    /// The next frame, or a panic after 2 s (`None` when the socket closed)
    pub(crate) async fn next(&self, label: &str) -> Option<String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let notified = self.queue.1.notified();
            if let Some(frame) = self.queue.0.lock().unwrap().pop_front() {
                return Some(frame);
            }
            if !self.is_open() {
                return None;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                panic!("{label} timed out");
            }
        }
    }

    pub(crate) async fn expect(&self, label: &str) -> String {
        self.next(label)
            .await
            .unwrap_or_else(|| panic!("{label}: the socket closed"))
    }

    /// Everything the server sent before the answer to a fresh ping: the server handles one
    /// connection's frames in order, so this collects every frame caused by what was sent before
    pub(crate) async fn barrier(&self) -> Vec<String> {
        let id = format!("barrier-{}", NEXT_BARRIER.fetch_add(1, Ordering::SeqCst));
        self.send(&format!(
            r#"<iq type="get" id="{id}"><ping xmlns="urn:xmpp:ping"/></iq>"#
        ));
        let mut before = Vec::new();
        loop {
            let frame = self.expect(&format!("answer to {id}")).await;
            if frame.contains(&format!("id=\"{id}\"")) && frame.contains("type=\"result\"") {
                return before;
            }
            before.push(frame);
        }
    }

    /// Cuts the connection without a close (`terminate()`)
    pub(crate) fn close(&self) {
        self.terminated.store(true, Ordering::SeqCst);
        for task in &self.tasks {
            task.abort();
        }
    }

    /// Waits for the connection to close (`Closed`), with its close code (1006 when cut)
    pub(crate) async fn closed(&self) -> u16 {
        if self.terminated.load(Ordering::SeqCst) {
            return 1006;
        }
        let mut closed = self.closed.clone();
        tokio::time::timeout(Duration::from_secs(5), closed.wait_for(Option::is_some))
            .await
            .expect("the connection closed")
            .map(|code| code.unwrap())
            .unwrap_or(1006)
    }

    /// A clean logout: `<close/>`, which the server has fully handled once its own `<close/>` comes back
    pub(crate) async fn logout(&self) {
        if !self.is_open() {
            return;
        }
        self.send(&format!(r#"<close xmlns="{FRAMING}"/>"#));
        self.closed().await;
    }
}

impl Drop for WireClient {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// `LoginOptions`
#[derive(Default)]
pub(crate) struct LoginOptions {
    pub resource: Option<String>,
    /// `None`: the default domain; `Some(None)`: no `to` at all
    pub open_to: Option<Option<String>>,
    pub token: Option<String>,
    pub headers: Vec<(String, String)>,
}

/// Open, SASL PLAIN with the account's own token, open again, bind: exactly as the game does.
/// `Err` when the login was refused.
pub(crate) async fn try_login(
    state: &AppState,
    port: u16,
    uid: &str,
    options: LoginOptions,
) -> Result<WireClient, String> {
    let headers: Vec<(&str, &str)> = options.headers.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    let mut client = WireClient::connect(port, &headers)
        .await
        .map_err(|s| format!("http {s}"))?;
    let open_to = match options.open_to {
        None => Some(DOMAIN.to_owned()),
        Some(to) => to,
    };
    let open = format!(
        r#"<open xmlns="{FRAMING}"{} version="1.0"/>"#,
        open_to.map(|to| format!(r#" to="{to}""#)).unwrap_or_default()
    );
    client.send(&open);
    client.expect("open").await;
    client.expect("features").await;
    let token = match options.token {
        Some(token) => token,
        None => token(state, uid).await,
    };
    client.send(&format!(
        r#"<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="PLAIN">{}</auth>"#,
        base64_plain("", uid, &token)
    ));
    let success = client.expect("success").await;
    if !success.contains("<success") {
        return Err(format!("login of {uid} was refused"));
    }
    client.send(&open);
    client.expect("open again").await;
    client.expect("bind features").await;
    let resource = options.resource.unwrap_or_else(game_resource);
    client.send(&format!(
        r#"<iq type="set" id="_xmpp_bind1"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><resource>{resource}</resource></bind></iq>"#
    ));
    let bound = client.expect("bind result").await;
    let jid = regex::Regex::new(r"<jid>([^<]*)</jid>")
        .unwrap()
        .captures(&bound)
        .map(|c| c[1].to_owned())
        .ok_or_else(|| format!("bind of {uid} failed"))?;
    client.uid = uid.to_owned();
    client.resource = jid[jid.find('/').unwrap() + 1..].to_owned();
    client.domain = jid[jid.find('@').unwrap() + 1..jid.find('/').unwrap()].to_owned();
    Ok(client)
}

pub(crate) async fn login(state: &AppState, port: u16, uid: &str) -> WireClient {
    try_login(state, port, uid, LoginOptions::default()).await.unwrap()
}

// ---- Raw connections, for frames a WebSocket client refuses to send ----

/// A raw TCP connection upgraded to WebSocket by hand
pub(crate) async fn raw_upgrade(port: u16) -> tokio::net::TcpStream {
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let key: [u8; 16] = rand::rng().random();
    let request = [
        "GET // HTTP/1.1".to_owned(),
        "Host: 127.0.0.1".to_owned(),
        "Upgrade: websocket".to_owned(),
        "Connection: Upgrade".to_owned(),
        format!(
            "Sec-WebSocket-Key: {}",
            base64::engine::general_purpose::STANDARD.encode(key)
        ),
        "Sec-WebSocket-Protocol: xmpp".to_owned(),
        "Sec-WebSocket-Version: 13".to_owned(),
        String::new(),
        String::new(),
    ]
    .join("\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        assert_eq!(socket.read(&mut byte).await.unwrap(), 1, "the upgrade answer");
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head);
    assert!(
        head.starts_with("HTTP/1.1 101"),
        "no upgrade: {}",
        head.lines().next().unwrap_or_default()
    );
    socket
}

/// One client frame (payload under 126 bytes), masked unless `masked` is false
pub(crate) fn client_frame(opcode: u8, payload: &[u8], masked: bool) -> Vec<u8> {
    let mask: [u8; 4] = rand::rng().random();
    let mut frame = vec![0x80 | opcode, (if masked { 0x80 } else { 0 }) | payload.len() as u8];
    if masked {
        frame.extend_from_slice(&mask);
    }
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| if masked { byte ^ mask[index % 4] } else { *byte }),
    );
    frame
}

/// True when the peer closes the raw socket within 2 s
pub(crate) async fn raw_closed(socket: &mut tokio::net::TcpStream) -> bool {
    let mut buffer = [0u8; 1024];
    let reading = async {
        loop {
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(2), reading).await.is_ok()
}
