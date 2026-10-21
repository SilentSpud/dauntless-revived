//! The game's chat connection (`realtime/chat.ts`; MIGRATION_PLAN.md M32-M34, docs/findings/chat.md):
//! XMPP over WebSocket, in the metagame's own process, on its own loopback port (`CHAT_PORT`, default
//! 61099). In public mode the path is launcher relay -> gateway :443 -> 127.0.0.1:61099; on a dev PC
//! the game connects straight to it.
//!
//! A player logs in with the same signed token as the metagame (SASL PLAIN), never with an account
//! id alone. No stanza, token, SASL payload or message text is logged; `CHAT_TRACE=1` logs frames
//! with those redacted.
//!
//! The game reconnects on its next tick when an established connection drops, so:
//! - bad input never takes the metagame down: every stanza handler returns, and a logged-in
//!   connection is never closed over bad input (bad stanzas are dropped and counted, and the stanza
//!   errors of docs/findings/chat.md are answered). Only the client's `<close/>`, a ping timeout,
//!   replacement, shutdown, an oversized frame, output it does not read, or sustained abuse end one.
//!   Replacement, abuse, an oversized frame and unread output hold that account's next login back for
//!   60 s, so the game backs off 15-45 s instead of looping. A ping timeout does not.
//! - before login a connection gets a few frames only, and nothing it sends may make the server hold
//!   more than `OUTPUT_LIMIT` of unsent output for it.
//!
//! Pings: the client answers a server ping only from its game-thread tick, which a long map load
//! holds up; about 150 s of silence pass on both sides: pinged after 50 s, ended after another 100 s.
//!
//! One task owns all of chat (`Chat`): connections hand it their frames and it answers them one at a
//! time, in order, as the Node server's single thread did. Each connection has a reader and a writer
//! task; the writer counts what waits unsent, Node's `bufferedAmount`.

pub mod connection;
pub mod muc;
pub mod nick;
pub mod presence;
pub mod xmpp;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use diesel::prelude::*;
use serde_json::Value;
use service_kit::env::Env;
use tokio::sync::{mpsc, oneshot};

use crate::accounts::{is_account_id_shape, username_for};
use crate::app::AppState;
use crate::crypto::TokenError;
use muc::Room;
use presence::ClientPresence;
use xmpp::{
    Bucket, DEFAULT_DOMAIN, Element, NS_BIND, NS_CLIENT, NS_FRAMING, NS_PING, NS_SASL, NS_SESSION, NS_STANZAS,
    NS_STREAM_ERRORS, NS_STREAMS, escape, has_control_character, has_markup_declaration, is_xmpp_domain, local_name,
    parse_frame, parse_jid, redact_frame, text_of,
};

pub const FRAME_LIMIT: usize = 32768;
pub const BODY_LIMIT: usize = 2048;
/// Shared by every region; leave headroom beyond the realm's 100+ concurrent players.
const MAX_SOCKETS: usize = 512;
/// Connections that have not bound yet (not logged in, or logged in and not bound), per address
const MAX_UNBOUND_PER_ADDRESS: usize = 8;
const MAX_FRAMES_BEFORE_LOGIN: u32 = 4;
const LOGIN_DEADLINE_MS: i64 = 15 * 1000;
/// From the login: the game binds right after `<success/>`
const BIND_DEADLINE_MS: i64 = 10 * 1000;
const BIND_TIMEOUT_LIMIT: usize = 3;
const BIND_TIMEOUT_WINDOW_MS: i64 = 10 * 60 * 1000;
const SASL_FAILURE_LIMIT: usize = 10;
const SASL_FAILURE_WINDOW_MS: i64 = 10 * 60 * 1000;
const SASL_THROTTLE_MS: i64 = 10 * 60 * 1000;
const UID_THROTTLE_MS: i64 = 60 * 1000;
const IDLE_PING_MS: i64 = 50 * 1000;
const PING_TIMEOUT_MS: i64 = 100 * 1000;
const STANZA_BURST: u32 = 60;
const STANZA_REFILL_MS: f64 = 1000.0 / 30.0;
const ABUSE_DROPS: usize = 100;
const ABUSE_WINDOW_MS: i64 = 60 * 1000;
const LOG_ONCE_MS: i64 = 10 * 60 * 1000;
const LOG_ONCE_MAX: usize = 10000;
pub const OUTPUT_LIMIT: usize = 256 * 1024;
const MAX_RESOURCE: usize = 256;
const MAX_ID: usize = 128;
const MAX_SESSIONS_PER_ACCOUNT: usize = 2;
/// Bound or not: two bound sessions (a live one and a ghost) and one login in progress
const MAX_CONNECTIONS_PER_ACCOUNT: usize = 3;
const GHOST_PING_MS: i64 = 10 * 1000;
const SWEEP_MS: i64 = 60 * 1000;

// ---- Settings ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NickCheck {
    Enforce,
    Log,
}

impl NickCheck {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Log => "log",
        }
    }
}

/// `ReadChatConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub port: u16,
    pub host: String,
    pub nick_check: NickCheck,
    pub trace: bool,
    /// `CHAT_PRESENCE` (on by default): friends' online status.
    pub presence: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Settings {
    /// Chat is on when `CHAT` is unset, empty or 1. `CHAT_PORT` (61099) and `CHAT_BIND_HOST`
    /// (127.0.0.1) say where; in public mode (`GATEWAY_SECRET` set) only 127.0.0.1 is accepted.
    /// `CHAT_NICK_CHECK=log` admits room nicknames that fail the resource, format or name rule with a
    /// warning (a rollback switch only); another account's id is refused either way.
    pub fn from_env(env: &Env) -> Self {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let chat = env.raw("CHAT").map(crate::js::trim).unwrap_or("");
        let enabled = chat.is_empty() || chat == "1";
        let port_text = env.raw("CHAT_PORT").filter(|p| !p.is_empty()).unwrap_or("61099");
        let host = env
            .raw("CHAT_BIND_HOST")
            .filter(|h| !h.is_empty())
            .unwrap_or("127.0.0.1")
            .to_owned();
        let public_mode = env.raw("GATEWAY_SECRET").is_some_and(|s| !s.is_empty());
        if env.raw("EXPERIMENTAL_CHAT").is_some() && env.raw("CHAT").is_none() {
            warnings.push("EXPERIMENTAL_CHAT is no longer read; the switch is now CHAT=1".to_owned());
        }
        if !chat.is_empty() && chat != "0" && chat != "1" {
            warnings
                .push("CHAT is not 0 or 1; chat stays off. Unset it or use CHAT=1 to enable in-game chat.".to_owned());
        }
        let presence_text = env
            .raw("CHAT_PRESENCE")
            .map(crate::js::trim)
            .unwrap_or("")
            .to_ascii_lowercase();
        let presence_off = ["0", "false", "off", "no"].contains(&presence_text.as_str());
        if !enabled && !presence_off {
            warnings.push("CHAT_PRESENCE is on but chat is off (CHAT=1 is needed); nobody shows as online".to_owned());
        }
        let port = port_text
            .parse::<u32>()
            .ok()
            .filter(|_| (1..=5).contains(&port_text.len()) && port_text.bytes().all(|b| b.is_ascii_digit()))
            .filter(|p| (1..=65535).contains(p));
        if port.is_none() {
            errors.push("CHAT_PORT must be a port number (1-65535)".to_owned());
        }
        let host_ok = if public_mode {
            host == "127.0.0.1"
        } else {
            host == "127.0.0.1" || host == "::1"
        };
        if !host_ok {
            errors.push(if public_mode {
                "CHAT_BIND_HOST must be 127.0.0.1 in public mode (the gateway forwards chat there)".to_owned()
            } else {
                "CHAT_BIND_HOST must be 127.0.0.1 or ::1".to_owned()
            });
        }
        let nick_text = env.raw("CHAT_NICK_CHECK").unwrap_or("");
        let nick_check = if nick_text == "log" {
            NickCheck::Log
        } else {
            if !nick_text.is_empty() && nick_text != "enforce" {
                warnings.push(format!(
                    "CHAT_NICK_CHECK={} is not enforce or log; nicknames are checked",
                    nick_text.chars().take(16).collect::<String>()
                ));
            }
            NickCheck::Enforce
        };
        Self {
            enabled,
            port: port.unwrap_or(0) as u16,
            host,
            nick_check,
            trace: env.raw("CHAT_TRACE") == Some("1"),
            presence: crate::config::on_off(env, "CHAT_PRESENCE", true),
            errors,
            warnings,
        }
    }
}

// ---- Sessions ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Close,
    Socket,
    PingTimeout,
    Replaced,
    Abuse,
    Size,
    Backlog,
    Shutdown,
    Timeout,
    Refused,
}

impl EndReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Socket => "socket",
            Self::PingTimeout => "ping-timeout",
            Self::Replaced => "replaced",
            Self::Abuse => "abuse",
            Self::Size => "size",
            Self::Backlog => "backlog",
            Self::Shutdown => "shutdown",
            Self::Timeout => "timeout",
            Self::Refused => "refused",
        }
    }
}

/// What the writer task of a connection is asked to do.
#[derive(Debug)]
pub enum Out {
    Text(String),
    /// A close frame with this code, then the end once the client answers (or soon after)
    Close(u16),
    /// Cut the connection at once
    Terminate,
}

/// The connection's sending side, as the chat task sees it.
#[derive(Debug, Clone)]
pub struct Outbox {
    pub sender: mpsc::UnboundedSender<Out>,
    /// Bytes handed to the writer and not yet written to the socket (`bufferedAmount`)
    pub queued: Arc<AtomicUsize>,
    /// The connection is gone (both directions), for the shutdown's wait
    pub closed: Arc<AtomicBool>,
    /// Cuts the connection at once (`terminate()`)
    pub terminate: Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
pub struct Session {
    pub id: u64,
    pub out: Outbox,
    /// Still open for sending (`readyState === OPEN`): false once a close was sent, the connection
    /// was cut or the client closed it
    pub open: bool,
    pub address: String,
    pub via: &'static str,
    pub opened_at: i64,
    pub domain: String,
    pub uid: Option<String>,
    pub name_at_login: Option<String>,
    pub logged_in_at: Option<i64>,
    pub resource: Option<String>,
    pub ended: bool,
    early_frames: u32,
    login_failed: bool,
    backlogged: bool,
    pub last_inbound: i64,
    pub ping_id: Option<String>,
    pub ping_deadline: Option<i64>,
    stanzas: Bucket,
    drops: Vec<i64>,
    last_drop_log: i64,
    namespaces: HashSet<String>,
    pub available: bool,
    pub last_presence: Option<ClientPresence>,
    pub shown: Option<ClientPresence>,
    pub catch_up_due: bool,
    pub presence_tokens: Bucket,
    pub presence_pending: bool,
    /// Bare room JID -> the nickname held there, in join order
    pub rooms: Vec<(String, String)>,
    pub joins: Bucket,
    pub messages: Bucket,
}

impl Session {
    pub fn full_jid(&self) -> String {
        format!(
            "{}@{}/{}",
            self.uid.as_deref().unwrap_or("undefined"),
            self.domain,
            self.resource.as_deref().unwrap_or("undefined")
        )
    }

    pub fn room_nick(&self, room: &str) -> Option<&str> {
        self.rooms.iter().find(|(r, _)| r == room).map(|(_, n)| n.as_str())
    }
}

/// What the chat task is asked: by a connection (opened, a frame, gone), the upgrade check, or the
/// metagame (shutdown).
#[derive(Debug)]
pub enum Event {
    /// May this address open one more connection?
    Admit {
        address: String,
        via: &'static str,
        reply: oneshot::Sender<bool>,
    },
    Connected {
        address: String,
        via: &'static str,
        out: Outbox,
        reply: oneshot::Sender<u64>,
    },
    Frame {
        id: u64,
        text: String,
    },
    /// The connection ended on its own (the client closed it, an error, an oversized frame)
    Gone {
        id: u64,
        size: bool,
    },
    Shutdown(oneshot::Sender<()>),
}

/// Log keys -> when last logged, oldest first (at most `LOG_ONCE_MAX`).
#[derive(Debug, Default)]
struct LogOnce {
    at: HashMap<String, (i64, u64)>,
    order: BTreeMap<u64, String>,
    next: u64,
}

impl LogOnce {
    fn check(&mut self, key: &str, window_ms: i64, now: i64) -> bool {
        if let Some((last, _)) = self.at.get(key)
            && now - last < window_ms
        {
            return false;
        }
        if let Some((_, seq)) = self.at.remove(key) {
            self.order.remove(&seq);
        }
        while self.at.len() >= LOG_ONCE_MAX {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            self.at.remove(&oldest);
        }
        self.next += 1;
        self.at.insert(key.to_owned(), (now, self.next));
        self.order.insert(self.next, key.to_owned());
        true
    }

    fn prune(&mut self, now: i64) {
        let old: Vec<String> = self
            .at
            .iter()
            .filter(|(_, (at, _))| now - at >= LOG_ONCE_MS)
            .map(|(k, _)| k.clone())
            .collect();
        for key in old {
            if let Some((_, seq)) = self.at.remove(&key) {
                self.order.remove(&seq);
            }
        }
    }
}

/// The clock chat reads: the real one, or a test's.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Everything chat holds. One task owns it (`run`).
pub struct Chat {
    pub app: AppState,
    /// In connection order (ids only grow)
    pub sessions: BTreeMap<u64, Session>,
    /// Bound sessions per account, oldest first
    pub by_uid: HashMap<String, Vec<u64>>,
    sasl_failures: HashMap<String, Vec<i64>>,
    throttled_addresses: HashMap<String, i64>,
    throttled_uids: HashMap<String, i64>,
    logged_once: LogOnce,
    bind_timeouts: HashMap<String, Vec<i64>>,
    clock: Clock,
    trace: bool,
    pub nick_check: NickCheck,
    output_limit: usize,
    pub presence: bool,
    /// In creation order
    pub rooms: Vec<Room>,
    next_id: u64,
    next_ping: u64,
    last_sweep: i64,
}

/// What a test may change.
#[derive(Clone)]
pub struct Options {
    pub trace: bool,
    pub nick_check: NickCheck,
    pub presence: bool,
    pub clock: Clock,
    pub output_limit: usize,
}

impl Options {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            trace: settings.trace,
            nick_check: settings.nick_check,
            presence: settings.presence,
            clock: Arc::new(crate::time::now_ms),
            output_limit: OUTPUT_LIMIT,
        }
    }
}

impl Chat {
    pub fn new(app: AppState, options: Options) -> Self {
        Self {
            app,
            sessions: BTreeMap::new(),
            by_uid: HashMap::new(),
            sasl_failures: HashMap::new(),
            throttled_addresses: HashMap::new(),
            throttled_uids: HashMap::new(),
            logged_once: LogOnce::default(),
            bind_timeouts: HashMap::new(),
            clock: options.clock,
            trace: options.trace,
            nick_check: options.nick_check,
            output_limit: options.output_limit,
            presence: options.presence,
            rooms: Vec::new(),
            next_id: 1,
            next_ping: 1,
            last_sweep: 0,
        }
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn log_once(&mut self, key: &str, window_ms: Option<i64>) -> bool {
        let now = self.now();
        self.logged_once.check(key, window_ms.unwrap_or(LOG_ONCE_MS), now)
    }

    /// A session that is still in the server (not ended and removed).
    pub fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.get(&id)
    }

    /// The account's bound sessions, oldest first.
    pub fn sessions_of(&self, uid: &str) -> Vec<u64> {
        self.by_uid.get(uid).cloned().unwrap_or_default()
    }

    // ---- Output ----

    fn is_backlogged(&self, session: &Session) -> bool {
        session.backlogged || session.out.queued.load(Ordering::Relaxed) > self.output_limit
    }

    /// A client that does not read its output gets no more once `output_limit` bytes wait unsent; the
    /// session ends at the next frame or tick (ending it here would change rooms mid fan-out).
    pub fn send(&mut self, id: u64, stanza: &str) {
        let (trace, limit) = (self.trace, self.output_limit);
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if !session.open {
            return;
        }
        if !session.ended && (session.backlogged || session.out.queued.load(Ordering::Relaxed) > limit) {
            session.backlogged = true;
            return;
        }
        if trace {
            tracing::info!("chat: trace c={} out {}", session.id, redact_frame(stanza));
        }
        session.out.queued.fetch_add(stanza.len(), Ordering::Relaxed);
        if session.out.sender.send(Out::Text(stanza.to_owned())).is_err() {
            session.open = false;
        }
    }

    fn close_socket(&mut self, id: u64, how: Out) {
        if let Some(session) = self.sessions.get_mut(&id) {
            session.open = false;
            let _ = session.out.sender.send(how);
        }
    }

    // ---- Connections ----

    fn admit(&mut self, address: &str, via: &str) -> bool {
        let unbound = self
            .sessions
            .values()
            .filter(|s| s.address == address && s.resource.is_none())
            .count();
        if self.sessions.len() >= MAX_SOCKETS || unbound >= MAX_UNBOUND_PER_ADDRESS {
            if self.log_once(&format!("busy|{address}"), None) {
                tracing::warn!(
                    "chat: refused a connection from={address} via={via} ({})",
                    if self.sessions.len() >= MAX_SOCKETS {
                        "too many connections"
                    } else {
                        "too many logins in progress from this address"
                    }
                );
            }
            return false;
        }
        true
    }

    fn connected(&mut self, address: String, via: &'static str, out: Outbox) -> u64 {
        let now = self.now();
        let id = self.next_id;
        self.next_id += 1;
        tracing::info!("chat: connect c={id} from={address} via={via}");
        self.sessions.insert(
            id,
            Session {
                id,
                out,
                open: true,
                address,
                via,
                opened_at: now,
                domain: DEFAULT_DOMAIN.to_owned(),
                uid: None,
                name_at_login: None,
                logged_in_at: None,
                resource: None,
                ended: false,
                early_frames: 0,
                login_failed: false,
                backlogged: false,
                last_inbound: now,
                ping_id: None,
                ping_deadline: None,
                stanzas: Bucket::new(STANZA_BURST, now),
                drops: Vec::new(),
                last_drop_log: 0,
                namespaces: HashSet::new(),
                available: false,
                last_presence: None,
                shown: None,
                catch_up_due: false,
                presence_tokens: Bucket::new(presence::PRESENCE_BURST, now),
                presence_pending: false,
                rooms: Vec::new(),
                joins: Bucket::new(muc::JOIN_BURST, now),
                messages: Bucket::new(muc::MESSAGE_BURST, now),
            },
        );
        id
    }

    /// Ends a session once. Replacement, abuse, an oversized frame and unread output hold that
    /// account's next login back for 60 s; a ping timeout does not.
    pub async fn end(&mut self, id: u64, reason: EndReason) {
        let now = self.now();
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.ended {
            return;
        }
        session.ended = true;
        let leave = if reason == EndReason::Replaced {
            muc::LeaveReason::Replaced
        } else {
            muc::LeaveReason::Disconnect
        };
        self.leave_all(id, leave);
        let Some(session) = self.sessions.get(&id) else {
            return;
        };
        let (uid, resource, opened_at) = (session.uid.clone(), session.resource.clone(), session.opened_at);
        if let (Some(uid), Some(_)) = (&uid, &resource) {
            let list: Vec<u64> = self.sessions_of(uid).into_iter().filter(|other| *other != id).collect();
            if list.is_empty() {
                self.by_uid.remove(uid);
            } else {
                self.by_uid.insert(uid.clone(), list);
            }
            if matches!(
                reason,
                EndReason::Replaced | EndReason::Abuse | EndReason::Size | EndReason::Backlog
            ) {
                self.throttled_uids.insert(uid.clone(), now + UID_THROTTLE_MS);
            }
            // Its friends see it go offline (at shutdown every session ends anyway)
            if reason != EndReason::Shutdown && self.presence {
                self.presence_unavailable(id, reason.as_str()).await;
            }
        }
        tracing::info!(
            "chat: closed c={id} uid={} reason={} after={}s",
            uid.as_deref().unwrap_or("-"),
            reason.as_str(),
            ((now - opened_at) as f64 / 1000.0).round() as i64
        );
        if self.sessions.get(&id).is_some_and(|s| s.open) {
            match reason {
                EndReason::Close | EndReason::Shutdown | EndReason::Replaced => {
                    self.send(id, &format!("<close xmlns=\"{NS_FRAMING}\"/>"));
                    self.close_socket(id, Out::Close(1000));
                }
                EndReason::Size => self.close_socket(id, Out::Close(1009)),
                _ => self.close_socket(id, Out::Terminate),
            }
        }
        self.sessions.remove(&id);
    }

    pub fn ping(&mut self, id: u64, timeout_ms: i64) {
        let now = self.now();
        let ping_id = format!("sp{}", self.next_ping);
        self.next_ping += 1;
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.ping_id = Some(ping_id.clone());
        session.ping_deadline = Some(now + timeout_ms);
        let stanza = format!(
            "<iq xmlns=\"{NS_CLIENT}\" type=\"get\" id=\"{ping_id}\" from=\"{}\" to=\"{}\"><ping xmlns=\"{NS_PING}\"/></iq>",
            escape(&session.domain),
            escape(&session.full_jid())
        );
        self.send(id, &stanza);
    }

    // ---- Throttles ----

    /// Three logins of one account in 10 minutes that never bound: its logins wait 60 s
    fn bind_timed_out(&mut self, uid: &str, now: i64) {
        let recent = self.bind_timeouts.entry(uid.to_owned()).or_default();
        recent.retain(|at| now - at < BIND_TIMEOUT_WINDOW_MS);
        recent.push(now);
        if recent.len() >= BIND_TIMEOUT_LIMIT {
            self.throttled_uids.insert(uid.to_owned(), now + UID_THROTTLE_MS);
        }
    }

    fn count_failure(&mut self, address: &str, now: i64) {
        let recent = self.sasl_failures.entry(address.to_owned()).or_default();
        recent.retain(|at| now - at < SASL_FAILURE_WINDOW_MS);
        recent.push(now);
        if recent.len() >= SASL_FAILURE_LIMIT {
            self.throttled_addresses
                .insert(address.to_owned(), now + SASL_THROTTLE_MS);
        }
    }

    fn prune(&mut self, now: i64) {
        self.throttled_addresses.retain(|_, until| *until > now);
        self.throttled_uids.retain(|_, until| *until > now);
        self.logged_once.prune(now);
        self.bind_timeouts
            .retain(|_, times| !times.iter().all(|at| now - at >= BIND_TIMEOUT_WINDOW_MS));
        self.sasl_failures.retain(|_, times| {
            times.retain(|at| now - at < SASL_FAILURE_WINDOW_MS);
            !times.is_empty()
        });
    }

    /// A dropped stanza; past `ABUSE_DROPS` in a minute the session ends.
    pub async fn drop_stanza(&mut self, id: u64, reason: &str) {
        let now = self.now();
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.drops.retain(|at| now - at < ABUSE_WINDOW_MS);
        session.drops.push(now);
        if now - session.last_drop_log >= 60 * 1000 {
            session.last_drop_log = now;
            tracing::info!("chat: dropped c={id} reason={reason}");
        }
        if session.drops.len() > ABUSE_DROPS {
            self.end(id, EndReason::Abuse).await;
        }
    }

    // ---- The tick ----

    /// The deadlines, pings and sweeps, every second.
    pub async fn tick(&mut self) {
        let now = self.now();
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            let Some(session) = self.sessions.get(&id) else {
                continue;
            };
            if session.ended {
                continue;
            }
            if self.is_backlogged(session) {
                self.end(id, EndReason::Backlog).await;
            } else if session.uid.is_none() && now - session.opened_at >= LOGIN_DEADLINE_MS {
                self.end(id, EndReason::Timeout).await;
            } else if let (Some(uid), None) = (&session.uid, &session.resource) {
                if now - session.logged_in_at.unwrap_or(session.opened_at) >= BIND_DEADLINE_MS {
                    let uid = uid.clone();
                    self.bind_timed_out(&uid, now);
                    self.end(id, EndReason::Timeout).await;
                }
            } else if session.resource.is_some() {
                match session.ping_deadline {
                    Some(deadline) => {
                        if now >= deadline {
                            self.end(id, EndReason::PingTimeout).await;
                        }
                    }
                    None => {
                        if now - session.last_inbound >= IDLE_PING_MS {
                            self.ping(id, PING_TIMEOUT_MS);
                        }
                    }
                }
            }
        }
        if now - self.last_sweep >= SWEEP_MS {
            self.last_sweep = now;
            self.sweep_rooms().await;
        }
        if self.presence {
            self.presence_tick().await;
        }
        self.prune(now);
    }

    // ---- Stanzas ----

    pub async fn frame(&mut self, id: u64, raw: &str) {
        let now = self.now();
        let trace = self.trace;
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.ended {
            return;
        }
        session.last_inbound = now;
        session.ping_deadline = None;
        session.ping_id = None;
        if trace {
            tracing::info!("chat: trace c={id} in {}", redact_frame(raw));
        }
        if self.sessions.get(&id).is_some_and(|s| self.is_backlogged(s)) {
            self.end(id, EndReason::Backlog).await;
            return;
        }
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        // Before login the game sends <open>, <auth> and, after a refused login, its legacy login: a
        // few frames at most
        if session.uid.is_none() {
            session.early_frames += 1;
            if session.early_frames > MAX_FRAMES_BEFORE_LOGIN {
                let address = session.address.clone();
                self.count_failure(&address, now);
                self.end(id, EndReason::Refused).await;
                return;
            }
        } else if !session.stanzas.take(STANZA_BURST, STANZA_REFILL_MS, now) {
            self.drop_stanza(id, "rate").await;
            return;
        }
        let logged_in = self.sessions.get(&id).is_some_and(|s| s.uid.is_some());
        // No DTDs or processing instructions, ever. Before login that ends the connection; after it the
        // frame is dropped (the game never sends one, and closing would start a reconnect loop).
        if has_markup_declaration(raw) {
            if logged_in {
                self.drop_stanza(id, "parse").await;
            } else {
                self.send(
                    id,
                    &format!(
                        "<stream:error xmlns:stream=\"{NS_STREAMS}\"><restricted-xml xmlns=\"{NS_STREAM_ERRORS}\"/></stream:error>"
                    ),
                );
                self.end(id, EndReason::Refused).await;
            }
            return;
        }
        let Some(node) = parse_frame(raw) else {
            if logged_in {
                self.drop_stanza(id, "parse").await;
            } else {
                self.end(id, EndReason::Refused).await;
            }
            return;
        };
        let kind = local_name(&node.name);
        if kind == "open" {
            self.open(id, &node);
            return;
        }
        if kind == "close" {
            self.end(id, EndReason::Close).await;
            return;
        }
        if !logged_in {
            self.before_login(id, &node, &kind).await;
            return;
        }
        if kind == "iq" {
            self.iq(id, &node).await;
            return;
        }
        if self.sessions.get(&id).is_some_and(|s| s.resource.is_none()) {
            self.drop_stanza(id, "unknown").await;
            return;
        }
        match kind.as_str() {
            "presence" => self.presence(id, &node).await,
            "message" => self.message(id, &node).await,
            _ => self.drop_stanza(id, "unknown").await,
        }
    }

    fn open(&mut self, id: u64, node: &Element) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.resource.is_none() {
            match node.attr("to").filter(|to| is_xmpp_domain(to)) {
                Some(to) => session.domain = to.to_owned(),
                None => {
                    if session.uid.is_none() {
                        session.domain = DEFAULT_DOMAIN.to_owned();
                    }
                }
            }
        }
        let (domain, logged_in) = (escape(&session.domain), session.uid.is_some());
        self.send(
            id,
            &format!(
                "<open xmlns=\"{NS_FRAMING}\" from=\"{domain}\" id=\"{}\" version=\"1.0\" xml:lang=\"en\"/>",
                crate::crypto::random_hex(8)
            ),
        );
        self.send(
            id,
            &if logged_in {
                format!("<stream:features xmlns:stream=\"{NS_STREAMS}\"><bind xmlns=\"{NS_BIND}\"/></stream:features>")
            } else {
                format!(
                    "<stream:features xmlns:stream=\"{NS_STREAMS}\"><mechanisms xmlns=\"{NS_SASL}\"><mechanism>PLAIN</mechanism></mechanisms></stream:features>"
                )
            },
        );
    }

    async fn before_login(&mut self, id: u64, node: &Element, kind: &str) {
        if kind == "auth" {
            self.auth(id, node).await;
            return;
        }
        // After a failed SASL the game tries legacy jabber:iq:auth: refuse it and hang up
        if kind == "iq" {
            let iq_id = node.attr("id").unwrap_or("");
            let id_attr = if !iq_id.is_empty() && iq_id.encode_utf16().count() <= MAX_ID {
                format!(" id=\"{}\"", escape(iq_id))
            } else {
                String::new()
            };
            self.send(
                id,
                &format!(
                    "<iq xmlns=\"{NS_CLIENT}\" type=\"error\"{id_attr}><error type=\"auth\"><not-authorized xmlns=\"{NS_STANZAS}\"/></error></iq>"
                ),
            );
        }
        self.end(id, EndReason::Refused).await;
    }

    fn refuse_login(&mut self, id: u64, reason: &str, uid: Option<&str>, condition: &str) {
        let now = self.now();
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.login_failed = true;
        let address = session.address.clone();
        if reason != "throttled" {
            self.count_failure(&address, now);
        }
        if self.log_once(&format!("login|{}|{reason}", uid.unwrap_or(&address)), None) {
            tracing::info!(
                "chat: login refused c={id} reason={reason}{}",
                uid.map(|u| format!(" uid={u}")).unwrap_or_default()
            );
        }
        self.send(id, &format!("<failure xmlns=\"{NS_SASL}\"><{condition}/></failure>"));
    }

    async fn auth(&mut self, id: u64, node: &Element) {
        let now = self.now();
        let Some(session) = self.sessions.get(&id) else {
            return;
        };
        // One SASL attempt per connection: after a refusal the game tries its legacy login, never <auth>
        if session.login_failed {
            self.end(id, EndReason::Refused).await;
            return;
        }
        if self
            .throttled_addresses
            .get(&session.address)
            .is_some_and(|until| *until > now)
        {
            self.refuse_login(id, "throttled", None, "temporary-auth-failure");
            return;
        }
        if node.attr("mechanism") != Some("PLAIN") {
            self.refuse_login(id, "bad-mechanism", None, "invalid-mechanism");
            return;
        }
        let text = node.text();
        let encoded = crate::js::trim(&text);
        let valid_shape = !encoded.is_empty() && encoded.len() <= 12000 && {
            let body = encoded.trim_end_matches('=');
            encoded.len() - body.len() <= 2
                && !body.is_empty()
                && body
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        };
        if !valid_shape {
            self.refuse_login(id, "bad-format", None, "not-authorized");
            return;
        }
        let decoded = String::from_utf8_lossy(&decode_base64_like_node(encoded)).into_owned();
        let fields: Vec<&str> = decoded.split('\0').collect();
        if fields.len() != 3
            || (!fields[0].is_empty() && fields[0] != fields[1])
            || !is_account_id_shape(fields[1])
            || fields[2].is_empty()
            || fields[2].encode_utf16().count() > 8192
        {
            self.refuse_login(id, "bad-format", None, "not-authorized");
            return;
        }
        let (uid, token) = (fields[1].to_owned(), fields[2].to_owned());
        if self.throttled_uids.get(&uid).is_some_and(|until| *until > now) {
            self.refuse_login(id, "throttled", Some(&uid), "temporary-auth-failure");
            return;
        }
        match self.app.tokens.verify_detailed(&token) {
            Err(TokenError::Expired) => {
                self.refuse_login(id, "expired", Some(&uid), "not-authorized");
                return;
            }
            Err(TokenError::Invalid) => {
                self.refuse_login(id, "bad-token", Some(&uid), "not-authorized");
                return;
            }
            Ok(_) => {}
        }
        // The epoch and ban checks of ValidateMetagameJWTAndGetPayload
        let claims = match crate::auth::validate_token(&self.app, &token).await {
            Ok(Some(claims)) => claims,
            _ => {
                self.refuse_login(id, "bad-token", Some(&uid), "not-authorized");
                return;
            }
        };
        if claims.user_id != uid {
            self.refuse_login(id, "uid-mismatch", Some(&uid), "not-authorized");
            return;
        }
        let lookup = uid.clone();
        let name = self
            .app
            .store
            .read(move |conn| username_for(conn, &lookup).map_err(crate::app::AppError::from))
            .await
            .ok()
            .flatten();
        let Some(name) = name else {
            self.refuse_login(id, "no-account", Some(&uid), "not-authorized");
            return;
        };
        // At most three connections per account, bound or not: past that the oldest one that has not
        // bound goes (bound ones are limited to two at the bind)
        let mine: Vec<(u64, bool)> = self
            .sessions
            .values()
            .filter(|other| other.id != id && !other.ended && other.uid.as_deref() == Some(uid.as_str()))
            .map(|other| (other.id, other.resource.is_none()))
            .collect();
        let excess = (mine.len() + 1).saturating_sub(MAX_CONNECTIONS_PER_ACCOUNT);
        for old in mine
            .iter()
            .filter(|(_, unbound)| *unbound)
            .take(excess)
            .map(|(i, _)| *i)
            .collect::<Vec<_>>()
        {
            self.end(old, EndReason::Replaced).await;
        }
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.uid = Some(uid.clone());
        session.name_at_login = Some(name);
        session.logged_in_at = Some(now);
        tracing::info!("chat: login ok c={id} uid={uid}");
        self.send(id, &format!("<success xmlns=\"{NS_SASL}\"/>"));
    }

    async fn iq(&mut self, id: u64, node: &Element) {
        let kind = node.attr("type");
        let iq_id = node.attr("id").unwrap_or("").to_owned();
        if matches!(kind, Some("result" | "error")) {
            return;
        }
        if !matches!(kind, Some("get" | "set")) || iq_id.encode_utf16().count() > MAX_ID {
            self.drop_stanza(id, "unknown").await;
            return;
        }
        if kind == Some("set")
            && let Some(binding) = node.child("bind")
        {
            let requested = text_of(binding.child("resource"));
            self.bind(id, &iq_id, &requested).await;
            return;
        }
        // Pings, the never-advertised session IQ, and anything else: an empty result. The namespace of
        // an unknown one is logged once per session.
        let namespace = node
            .elements()
            .next()
            .map(|child| child.attr("xmlns").unwrap_or("").to_owned());
        if let Some(namespace) = namespace
            && namespace != NS_PING
            && namespace != NS_SESSION
            && let Some(session) = self.sessions.get_mut(&id)
            && !session.namespaces.contains(&namespace)
            && session.namespaces.len() < 32
        {
            tracing::debug!(
                "chat: iq c={id} {} namespace={}",
                kind.unwrap_or(""),
                namespace.chars().take(80).collect::<String>()
            );
            session.namespaces.insert(namespace);
        }
        self.send(
            id,
            &format!("<iq xmlns=\"{NS_CLIENT}\" type=\"result\" id=\"{}\"/>", escape(&iq_id)),
        );
    }

    async fn bind(&mut self, id: u64, iq_id: &str, requested: &str) {
        let Some(session) = self.sessions.get(&id) else {
            return;
        };
        let uid = session.uid.clone().unwrap_or_default();
        let bound = |jid: &str| {
            format!(
                "<iq xmlns=\"{NS_CLIENT}\" type=\"result\" id=\"{}\"><bind xmlns=\"{NS_BIND}\"><jid>{}</jid></bind></iq>",
                escape(iq_id),
                escape(jid)
            )
        };
        if session.resource.is_some() {
            let stanza = bound(&session.full_jid());
            self.send(id, &stanza);
            return;
        }
        if requested.encode_utf16().count() > MAX_RESOURCE || has_control_character(requested) {
            self.send(
                id,
                &format!(
                    "<iq xmlns=\"{NS_CLIENT}\" type=\"error\" id=\"{}\"><error type=\"modify\"><bad-request xmlns=\"{NS_STANZAS}\"/></error></iq>",
                    escape(iq_id)
                ),
            );
            return;
        }
        let resource = if requested.is_empty() {
            format!("srv-{}", crate::crypto::random_hex(16))
        } else {
            requested.to_owned()
        };
        // At most two sessions per account. The same resource again is a ghost or a forgery, and a
        // third session replaces the one silent longest.
        let same = self.sessions_of(&uid).into_iter().find(|other| {
            self.sessions
                .get(other)
                .is_some_and(|s| s.resource.as_deref() == Some(resource.as_str()))
        });
        if let Some(same) = same {
            self.end(same, EndReason::Replaced).await;
        }
        let remaining = self.sessions_of(&uid);
        if remaining.len() >= MAX_SESSIONS_PER_ACCOUNT {
            // The first of the least recently heard, as reduce keeps
            let mut oldest: Option<(u64, i64)> = None;
            for other in remaining {
                let heard = self.sessions.get(&other).map_or(i64::MAX, |s| s.last_inbound);
                if oldest.is_none_or(|(_, at)| heard < at) {
                    oldest = Some((other, heard));
                }
            }
            if let Some((other, _)) = oldest {
                self.end(other, EndReason::Replaced).await;
            }
        }
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.resource = Some(resource.clone());
        let domain = session.domain.clone();
        let jid = session.full_jid();
        self.by_uid.entry(uid.clone()).or_default().push(id);
        tracing::info!(
            "chat: bound c={id} uid={uid} resource={resource} domain={domain} sessions={}",
            self.sessions_of(&uid).len()
        );
        self.send(id, &bound(&jid));
        // An older session of the same account that does not answer a ping within 10 s is a ghost
        let now = self.now();
        for other in self.sessions_of(&uid) {
            if other != id
                && self
                    .sessions
                    .get(&other)
                    .is_some_and(|s| s.ping_deadline.is_none_or(|deadline| deadline > now + GHOST_PING_MS))
            {
                self.ping(other, GHOST_PING_MS);
            }
        }
    }

    // ---- Presence and messages ----

    async fn presence(&mut self, id: u64, node: &Element) {
        let kind = node.attr("type");
        // A broadcast presence (no "to") is recorded; it is never echoed to its sender or to another
        // session of the same account. With CHAT_PRESENCE it goes to the sender's online friends. An
        // unavailable one leaves every room.
        let Some(to_text) = node.attr("to") else {
            match kind {
                Some("unavailable") => {
                    self.leave_all(id, muc::LeaveReason::Left);
                    if self.presence {
                        self.presence_unavailable(id, "unavailable").await;
                    }
                }
                None => {
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.available = true;
                    }
                    tracing::debug!("chat: presence c={id} status={:?}", status_text_of(node));
                    if self.presence {
                        self.presence_broadcast(id, node).await;
                    }
                }
                Some(_) => {}
            }
            return;
        };
        let to = if to_text.encode_utf16().count() <= 2048 {
            parse_jid(to_text)
        } else {
            None
        };
        let muc_x = node.child("x");
        let to_room = to.as_ref().is_some_and(|to| {
            self.sessions
                .get(&id)
                .is_some_and(|s| muc::is_muc_domain(&s.domain, &to.domain))
                || starts_with_ignore_case(&to.domain, "muc.")
                || starts_with_ignore_case(&to.domain, "conference.")
                || muc_x.is_some_and(|x| x.attr("xmlns") == Some(xmpp::NS_MUC))
        });
        let (Some(to), true) = (to, to_room) else {
            // Directed presence to a user, subscriptions and probes: dropped
            tracing::debug!("chat: presence to a user dropped c={id}");
            return;
        };
        match kind {
            None => self.join(id, &to).await,
            Some("unavailable") => self.leave(id, &to),
            Some(_) => {}
        }
    }

    async fn message(&mut self, id: u64, node: &Element) {
        let kind = node.attr("type");
        let to_text = node.attr("to").unwrap_or("");
        let to = if to_text.encode_utf16().count() <= 2048 {
            parse_jid(to_text)
        } else {
            None
        };
        match (kind, to) {
            (Some("groupchat"), Some(to)) => self.group_chat(id, &to, node).await,
            (Some("chat"), Some(to)) => self.whisper(id, node, &to.local, to.resource.as_deref()).await,
            _ => tracing::debug!(
                "chat: message dropped c={id} type={}",
                kind.unwrap_or("undefined").chars().take(16).collect::<String>()
            ),
        }
    }

    /// A whisper: to one session of the account when the resource names one, else to all of them,
    /// from the sender's full JID. Nothing goes back when it is not delivered.
    async fn whisper(&mut self, id: u64, node: &Element, target: &str, resource: Option<&str>) {
        let now = self.now();
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        let uid = session.uid.clone().unwrap_or_default();
        let body = text_of(node.child("body"));
        let length = body.chars().count();
        let message_id = node
            .attr("id")
            .filter(|i| i.encode_utf16().count() <= MAX_ID)
            .map(str::to_owned);
        let shown = if is_account_id_shape(target) { target } else { "?" };
        let refused = |reason: &str| tracing::info!("chat: whisper from={uid} to={shown} len={length} reason={reason}");
        // Every whisper takes a token first, refused ones included, so its log lines are as limited
        // as delivered ones
        if !muc::take_message_token(session, now) {
            if self.log_once(&format!("whisper-rate|{id}"), Some(60 * 1000)) {
                refused("limit");
            }
            return;
        }
        if length == 0 || length > BODY_LIMIT {
            refused("size");
            return;
        }
        if target == uid {
            refused("self");
            return;
        }
        let sessions = if target == "xmpp-admin" {
            Vec::new()
        } else {
            self.sessions_of(target)
        };
        if sessions.is_empty() {
            refused("offline");
            return;
        }
        // Either player blocked the other: not delivered (the same rule as party invites)
        let (a, b) = (uid.clone(), target.to_owned());
        let blocked = self
            .app
            .store
            .read(move |conn| crate::friends::blocked_either_way(conn, &a, &b).map_err(crate::app::AppError::from))
            .await
            .unwrap_or(true);
        if blocked {
            refused("blocked");
            return;
        }
        let chosen: Vec<u64> = sessions
            .iter()
            .copied()
            .filter(|other| self.sessions.get(other).and_then(|s| s.resource.as_deref()) == resource)
            .collect();
        let recipients = if chosen.is_empty() { sessions } else { chosen };
        let from = self.sessions.get(&id).map(Session::full_jid).unwrap_or_default();
        let text = escape(&body);
        let id_attr = message_id
            .map(|i| format!(" id=\"{}\"", escape(&i)))
            .unwrap_or_default();
        for recipient in &recipients {
            let to = self.sessions.get(recipient).map(Session::full_jid).unwrap_or_default();
            self.send(
                *recipient,
                &format!(
                    "<message xmlns=\"{NS_CLIENT}\" type=\"chat\" from=\"{}\" to=\"{}\"{id_attr}><body>{text}</body></message>",
                    escape(&from),
                    escape(&to)
                ),
            );
        }
        tracing::info!(
            "chat: whisper from={uid} to={target} len={length} delivered={}",
            recipients.len()
        );
    }

    // ---- The task ----

    /// Shutdown: `<close/>` to every session; whatever is still open is cut after 1 s.
    async fn shutdown(&mut self) {
        type Open = (u64, Arc<AtomicBool>, Arc<tokio::sync::Notify>);
        let open: Vec<Open> = self
            .sessions
            .values()
            .map(|s| (s.id, s.out.closed.clone(), s.out.terminate.clone()))
            .collect();
        for (id, _, _) in &open {
            self.end(*id, EndReason::Shutdown).await;
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        while tokio::time::Instant::now() < deadline
            && !open.iter().all(|(_, closed, _)| closed.load(Ordering::Relaxed))
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        for (_, _, terminate) in &open {
            terminate.notify_one();
        }
    }
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

/// `Buffer.from(text, "base64")` after the shape check: 6 bits per character up to the first `=`,
/// whole bytes kept, a trailing partial one dropped.
fn decode_base64_like_node(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut bits, mut count) = (0u32, 0u32);
    for b in text.bytes() {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => break,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out
}

/// The Status text of a broadcast presence (the client sends JSON in `<status>`), at most 64 characters.
fn status_text_of(node: &Element) -> String {
    let raw = text_of(node.child("status"));
    serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|v| {
            v.get("Status")
                .and_then(Value::as_str)
                .map(|s| s.chars().take(64).collect())
        })
        .unwrap_or_default()
}

/// Whether the account belongs to the guild (`IsGuildMember`).
pub fn is_guild_member(conn: &mut store::Connection, uid: &str, guild: &str) -> QueryResult<bool> {
    use crate::schema::guildmembers;
    Ok(guildmembers::table
        .filter(guildmembers::accountId.eq(uid))
        .filter(guildmembers::guildId.eq(guild))
        .select(guildmembers::guildId)
        .first::<String>(conn)
        .optional()?
        .is_some())
}

/// `BlockersAmong`: who of the recipients blocked the sender.
pub fn blockers_among(
    conn: &mut store::Connection,
    sender: &str,
    recipients: &[String],
) -> QueryResult<HashSet<String>> {
    use crate::schema::blocks;
    let ids: Vec<&String> = recipients
        .iter()
        .filter(|id| *id != sender)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(blocks::table
        .filter(blocks::blockedId.eq(sender))
        .filter(blocks::blockerId.eq_any(ids))
        .select(blocks::blockerId)
        .load::<String>(conn)?
        .into_iter()
        .collect())
}

/// The chat task: answers events one at a time, ticks every second, hears key revocations and
/// friendship changes. Returns after a shutdown.
pub async fn run(mut chat: Chat, mut events: mpsc::UnboundedReceiver<Event>) {
    let mut revocations = chat.app.key_revocations.subscribe();
    let mut friendships = chat.app.friendships.subscribe();
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                None => return,
                Some(Event::Admit { address, via, reply }) => {
                    let _ = reply.send(chat.admit(&address, via));
                }
                Some(Event::Connected { address, via, out, reply }) => {
                    let id = chat.connected(address, via, out);
                    let _ = reply.send(id);
                }
                Some(Event::Frame { id, text }) => chat.frame(id, &text).await,
                Some(Event::Gone { id, size }) => {
                    let now = chat.now();
                    if let Some(session) = chat.sessions.get_mut(&id) {
                        if size {
                            // ws answers an oversized frame with a close 1009 of its own
                            let address = session.address.clone();
                            tracing::warn!("chat: socket error c={id} (RangeError)");
                            chat.count_failure(&address, now);
                            chat.end(id, EndReason::Size).await;
                        } else {
                            session.open = false;
                            chat.end(id, EndReason::Socket).await;
                        }
                    }
                }
                Some(Event::Shutdown(done)) => {
                    chat.shutdown().await;
                    let _ = done.send(());
                    return;
                }
            },
            _ = ticker.tick() => chat.tick().await,
            revoked = revocations.recv() => {
                if let Ok(uid) = revoked {
                    let ids: Vec<u64> = chat.sessions.values().filter(|s| s.uid.as_deref() == Some(uid.as_str())).map(|s| s.id).collect();
                    for id in ids {
                        chat.end(id, EndReason::Replaced).await;
                    }
                }
            }
            changed = friendships.recv() => {
                if let Ok(event) = changed
                    && chat.presence
                {
                    chat.friendship(event).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_read_like_node() {
        let env = |pairs: &[(&str, &str)]| Settings::from_env(&Env::from_pairs(pairs.iter().map(|(k, v)| (*k, *v))));
        let plain = env(&[]);
        assert!(plain.enabled && plain.presence);
        assert_eq!(
            (plain.port, plain.host.as_str(), plain.nick_check),
            (61099, "127.0.0.1", NickCheck::Enforce)
        );
        assert!(plain.errors.is_empty() && plain.warnings.is_empty());
        assert!(!env(&[("CHAT", "0")]).enabled);
        let wrong = env(&[("CHAT", "wrong")]);
        assert!(!wrong.enabled);
        assert_eq!(wrong.warnings.len(), 2);
        assert_eq!(env(&[("CHAT_PORT", "0")]).errors.len(), 1);
        assert_eq!(env(&[("CHAT_BIND_HOST", "::1")]).errors.len(), 0);
        assert_eq!(
            env(&[("CHAT_BIND_HOST", "::1"), ("GATEWAY_SECRET", "x")]).errors.len(),
            1
        );
        assert_eq!(env(&[("CHAT_NICK_CHECK", "log")]).nick_check, NickCheck::Log);
        assert_eq!(env(&[("CHAT_NICK_CHECK", "off")]).warnings.len(), 1);
        assert_eq!(env(&[("EXPERIMENTAL_CHAT", "1")]).warnings.len(), 1);
    }

    #[test]
    fn sasl_payloads_decode_like_buffer_from() {
        assert_eq!(decode_base64_like_node("AGEAYg=="), b"\0a\0b");
        assert_eq!(decode_base64_like_node("AGEAYg"), b"\0a\0b");
        assert_eq!(decode_base64_like_node("QQ"), b"A");
        assert_eq!(decode_base64_like_node("Q"), b"");
    }

    #[test]
    fn log_once_keeps_a_window() {
        let mut once = LogOnce::default();
        assert!(once.check("a", 1000, 0));
        assert!(!once.check("a", 1000, 999));
        assert!(once.check("a", 1000, 1000));
    }
}
