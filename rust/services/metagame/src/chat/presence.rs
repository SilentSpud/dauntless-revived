//! Friends' online status (`realtime/presence.ts`; `CHAT_PRESENCE`, on by default, with chat on). The
//! 1.4.4 client never asks for a roster, never subscribes and never probes: it only knows what the
//! server pushes, so:
//!
//! - A player's own broadcast presence (no "to") goes to every session of each ACCEPTED friend the
//!   player has not blocked and is not blocked by, once that session has sent a presence of its own:
//!   `<show>` and the `<status>` JSON as the client sent them, from the player's FULL JID. Every later
//!   change goes the same way.
//! - At a session's first broadcast it gets the current presence of each friend online in that sense.
//! - When a session ends or broadcasts type="unavailable", its friends get type="unavailable" from
//!   its full JID; if the account has another session with a presence, that presence follows.
//! - An accepted friend request (HTTP) pushes the client's friends-list message from
//!   xmpp-admin@<domain> to both players, then exchanges their presences; an unfriend or a block sends
//!   each the other's unavailable.
//!
//! The rule that must never break: no stanza outside a room reaches a player from the player's own
//! account, not even from a second session of it (`deliver` refuses one).
//!
//! A client that changes its presence too often has the changes held back (`PRESENCE_BURST`, then one
//! per `PRESENCE_REFILL_MS`); the latest state goes out at the next tick. Only the end of a session is
//! sent at once.

use serde_json::json;

use super::xmpp::{Element, NS_CLIENT, escape, text_of};
use super::{Chat, Session};
use crate::app::AppError;
use crate::friends::FriendshipEvent;

pub const PRESENCE_BURST: u32 = 5;
pub const PRESENCE_REFILL_MS: f64 = 2000.0;
/// `<status>` is a short JSON object (about 250 characters from the real client); a longer one is
/// not relayed
pub const MAX_STATUS_LENGTH: usize = 4096;
pub const ADMIN_LOCAL: &str = "xmpp-admin";
pub const FRIEND_MESSAGE_TYPE: &str = "com.epicgames.friends.core.apiobjects.Friend";
const SHOW_VALUES: [&str; 4] = ["away", "chat", "dnd", "xa"];

/// What the client last broadcast: `<show>` (only the four values it writes) and `<status>`, as sent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClientPresence {
    pub show: Option<String>,
    pub status: Option<String>,
}

fn same_account(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a.to_lowercase() == b.to_lowercase())
}

fn is_online(session: &Session) -> bool {
    !session.ended && session.uid.is_some() && session.resource.is_some()
}

/// It sent a presence of its own and has not said unavailable since: it hears its friends' changes
fn listening(session: &Session) -> bool {
    session.last_presence.is_some()
}

impl Chat {
    // ---- Who sees whom ----

    /// The online sessions of the account's ACCEPTED friends who have not blocked it and are not
    /// blocked by it. Never one of the account's own sessions.
    async fn friend_sessions(&mut self, uid: &str) -> Vec<u64> {
        let friends = match self.friends.clone() {
            Some(lookup) => Ok(lookup.friends_of(uid)),
            None => {
                let lookup = uid.to_owned();
                self.app
                    .store
                    .read(move |conn| crate::friends::list_friends(conn, &lookup, false).map_err(AppError::from))
                    .await
                    .map(|list| {
                        list.iter()
                            .filter_map(|f| f.get("accountId").and_then(|v| v.as_str()).map(str::to_owned))
                            .collect()
                    })
            }
        };
        let friends: Vec<String> = match friends {
            Ok(list) => list,
            Err(_) => {
                if self.log_once(&format!("presence-friends|{uid}"), Some(60 * 1000)) {
                    tracing::warn!("chat: presence: could not read the friends of {uid}; nothing relayed");
                }
                return Vec::new();
            }
        };
        let mut seen = std::collections::HashSet::new();
        let mut sessions = Vec::new();
        for friend in friends {
            if !seen.insert(friend.clone()) {
                continue;
            }
            let online: Vec<u64> = if same_account(Some(&friend), Some(uid)) {
                Vec::new()
            } else {
                self.sessions_of(&friend)
                    .into_iter()
                    .filter(|s| self.sessions.get(s).is_some_and(is_online))
                    .collect()
            };
            if online.is_empty() {
                continue;
            }
            let blocked = self.blocked_either_way(uid, &friend).await;
            match blocked {
                Ok(false) => sessions.extend(online),
                Ok(true) => {}
                Err(_) => {
                    if self.log_once(&format!("presence-friends|{uid}"), Some(60 * 1000)) {
                        tracing::warn!("chat: presence: could not read the friends of {uid}; nothing relayed");
                    }
                    return Vec::new();
                }
            }
        }
        sessions
    }

    /// Either player blocked the other: from the test's lookup when there is one, else the database
    async fn blocked_either_way(&self, a: &str, b: &str) -> Result<bool, AppError> {
        if let Some(lookup) = &self.friends {
            return Ok(lookup.blocked_either_way(a, b));
        }
        let (a, b) = (a.to_owned(), b.to_owned());
        self.app
            .store
            .read(move |conn| crate::friends::blocked_either_way(conn, &a, &b).map_err(AppError::from))
            .await
    }

    fn listening_of(&self, uid: &str) -> Vec<u64> {
        self.sessions_of(uid)
            .into_iter()
            .filter(|s| self.sessions.get(s).is_some_and(|s| is_online(s) && listening(s)))
            .collect()
    }

    fn shown_of(&self, uid: &str) -> Vec<u64> {
        self.sessions_of(uid)
            .into_iter()
            .filter(|s| self.sessions.get(s).is_some_and(|s| is_online(s) && s.shown.is_some()))
            .collect()
    }

    // ---- Sending ----

    /// Every stanza of this module goes through here: none reaches a session of the account it names
    /// as its sender.
    fn deliver(&mut self, from_uid: Option<&str>, to: u64, stanza: &str) {
        let to_uid = self.sessions.get(&to).and_then(|s| s.uid.clone());
        if from_uid.is_none() || to_uid.is_none() || same_account(from_uid, to_uid.as_deref()) {
            if self.log_once(
                &format!("presence-self|{}", to_uid.as_deref().unwrap_or("undefined")),
                Some(60 * 1000),
            ) {
                tracing::error!("chat: presence: refused to send c={to} a stanza from its own account");
            }
            return;
        }
        self.send(to, stanza);
    }

    /// The presence friends were last told of (`shown`).
    fn presence_stanza(&self, from: u64, to: u64) -> String {
        let (Some(from), Some(to)) = (self.sessions.get(&from), self.sessions.get(&to)) else {
            return String::new();
        };
        let told = from.shown.clone().unwrap_or_default();
        format!(
            "<presence xmlns=\"{NS_CLIENT}\" from=\"{}\" to=\"{}\">{}{}</presence>",
            escape(&from.full_jid()),
            escape(&to.full_jid()),
            told.show.map(|s| format!("<show>{s}</show>")).unwrap_or_default(),
            told.status
                .map(|s| format!("<status>{}</status>", escape(&s)))
                .unwrap_or_default()
        )
    }

    fn unavailable_stanza(&self, from: u64, to: u64) -> String {
        let (Some(from), Some(to)) = (self.sessions.get(&from), self.sessions.get(&to)) else {
            return String::new();
        };
        format!(
            "<presence xmlns=\"{NS_CLIENT}\" type=\"unavailable\" from=\"{}\" to=\"{}\"/>",
            escape(&from.full_jid()),
            escape(&to.full_jid())
        )
    }

    /// The friends-list push: `{type, payload, timestamp}` in the body of a plain message from xmpp-admin.
    fn friend_message(&self, to: u64, other: &str, direction: &str, created_at: i64) -> String {
        let now = self.now();
        let Some(to) = self.sessions.get(&to) else {
            return String::new();
        };
        let body = json!({
            "type": FRIEND_MESSAGE_TYPE,
            "payload": {
                "accountId": other,
                "status": "ACCEPTED",
                "direction": direction,
                "created": crate::time::iso(if created_at > 0 { created_at } else { now }),
            },
            "timestamp": crate::time::iso(now),
        });
        format!(
            "<message xmlns=\"{NS_CLIENT}\" from=\"{ADMIN_LOCAL}@{}\" to=\"{}\"><body>{}</body></message>",
            escape(&to.domain),
            escape(&to.full_jid()),
            escape(&body.to_string())
        )
    }

    // ---- Announcing a session's state ----

    /// A change of a session that stays connected: sent now if it has a token left, else held back
    async fn announce(&mut self, id: u64) {
        let now = self.now();
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.presence_tokens.take(PRESENCE_BURST, PRESENCE_REFILL_MS, now) {
            self.flush(id).await;
        } else {
            session.presence_pending = true;
        }
    }

    /// Tells the session's friends what changed since they were last told.
    async fn flush(&mut self, id: u64) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.presence_pending = false;
        let (now_state, before, uid, catch_up) = (
            session.last_presence.clone(),
            session.shown.clone(),
            session.uid.clone().unwrap_or_default(),
            session.catch_up_due,
        );
        let Some(now_state) = now_state else {
            if before.is_some() {
                self.went_offline(id, "unavailable").await;
            }
            return;
        };
        let friends = self.friend_sessions(&uid).await;
        let audience: Vec<u64> = friends
            .iter()
            .copied()
            .filter(|f| self.sessions.get(f).is_some_and(listening))
            .collect();
        let mut heard = 0;
        // It hears of its friends who are online, as they are shown to everyone
        if before.is_none() || catch_up {
            let shown: Vec<u64> = friends
                .iter()
                .copied()
                .filter(|f| self.sessions.get(f).is_some_and(|s| s.shown.is_some()))
                .collect();
            for friend in shown {
                let friend_uid = self.sessions.get(&friend).and_then(|s| s.uid.clone());
                let stanza = self.presence_stanza(friend, id);
                self.deliver(friend_uid.as_deref(), id, &stanza);
                heard += 1;
            }
            if let Some(session) = self.sessions.get_mut(&id) {
                session.catch_up_due = false;
            }
        }
        if before.as_ref() == Some(&now_state) {
            // Back to what its friends were last told (a change and its undoing, both held back)
            return;
        }
        if let Some(session) = self.sessions.get_mut(&id) {
            session.shown = Some(now_state);
        }
        for to in &audience {
            let stanza = self.presence_stanza(id, *to);
            self.deliver(Some(&uid), *to, &stanza);
        }
        if before.is_none() {
            tracing::info!(
                "chat: presence c={id} uid={uid} online: told {} friend session(s), heard of {heard}",
                audience.len()
            );
        } else {
            tracing::debug!(
                "chat: presence c={id} changed: relayed to {} friend session(s)",
                audience.len()
            );
        }
    }

    /// Its friends see it go; if the account has another session they were shown, that one's
    /// presence follows.
    async fn went_offline(&mut self, id: u64, why: &str) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.shown = None;
        let uid = session.uid.clone().unwrap_or_default();
        let audience: Vec<u64> = self
            .friend_sessions(&uid)
            .await
            .into_iter()
            .filter(|f| self.sessions.get(f).is_some_and(listening))
            .collect();
        for to in &audience {
            let stanza = self.unavailable_stanza(id, *to);
            self.deliver(Some(&uid), *to, &stanza);
        }
        let other = self.sessions_of(&uid).into_iter().find(|each| {
            *each != id
                && self
                    .sessions
                    .get(each)
                    .is_some_and(|s| is_online(s) && s.shown.is_some())
        });
        if let Some(other) = other {
            let other_uid = self.sessions.get(&other).and_then(|s| s.uid.clone());
            for to in &audience {
                let stanza = self.presence_stanza(other, *to);
                self.deliver(other_uid.as_deref(), *to, &stanza);
            }
        }
        tracing::info!(
            "chat: presence c={id} uid={uid} offline ({why}): told {} friend session(s){}",
            audience.len(),
            other.map(|o| format!("; c={o} is still online")).unwrap_or_default()
        );
    }

    // ---- The chat server's events ----

    /// A broadcast presence (no "to", no type) from a bound session.
    pub async fn presence_broadcast(&mut self, id: u64, node: &Element) {
        if !self.sessions.get(&id).is_some_and(is_online) {
            return;
        }
        let show = crate::js::trim(&text_of(node.child("show"))).to_owned();
        let mut status = text_of(node.child("status"));
        let length = status.encode_utf16().count();
        if length > MAX_STATUS_LENGTH {
            if self.log_once(&format!("presence-long|{id}"), None) {
                tracing::info!("chat: presence c={id} status of {length} characters not relayed");
            }
            status = String::new();
        }
        let next = ClientPresence {
            show: SHOW_VALUES.contains(&show.as_str()).then_some(show),
            status: (!status.is_empty()).then_some(status),
        };
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        let previous = session.last_presence.replace(next.clone());
        if previous.as_ref() == Some(&next) {
            return;
        }
        self.announce(id).await;
    }

    /// The session went offline: an unavailable broadcast from a session that stays connected (held
    /// back like any other change), or the end of the session (sent at once).
    pub async fn presence_unavailable(&mut self, id: u64, why: &str) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.uid.is_none()
            || session.resource.is_none()
            || (session.last_presence.is_none() && session.shown.is_none())
        {
            return;
        }
        session.last_presence = None;
        if !session.ended {
            session.catch_up_due = true;
            self.announce(id).await;
            return;
        }
        session.presence_pending = false;
        if session.shown.is_some() {
            self.went_offline(id, why).await;
        }
    }

    /// Held-back changes whose turn has come (the 1 s tick).
    pub async fn presence_tick(&mut self) {
        let now = self.now();
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            let Some(session) = self.sessions.get_mut(&id) else {
                continue;
            };
            if session.presence_pending
                && is_online(session)
                && session.presence_tokens.take(PRESENCE_BURST, PRESENCE_REFILL_MS, now)
            {
                self.flush(id).await;
            }
        }
    }

    /// A friendship began or ended over HTTP.
    pub async fn friendship(&mut self, event: FriendshipEvent) {
        match event {
            FriendshipEvent::Accepted {
                requester,
                accepter,
                created_at,
            } => self.accepted(&requester, &accepter, created_at).await,
            FriendshipEvent::Ended { a, b } => self.ended_friendship(&a, &b),
        }
    }

    async fn accepted(&mut self, requester: &str, accepter: &str, created_at: i64) {
        if same_account(Some(requester), Some(accepter)) {
            return;
        }
        let blocked = self.blocked_either_way(requester, accepter).await.unwrap_or(true);
        if blocked {
            return;
        }
        let online = |chat: &Chat, uid: &str| -> Vec<u64> {
            chat.sessions_of(uid)
                .into_iter()
                .filter(|s| chat.sessions.get(s).is_some_and(is_online))
                .collect()
        };
        let (of_requester, of_accepter) = (online(self, requester), online(self, accepter));
        // The list entry to every session first, then the presences between the sessions that are
        // online for presence
        for to in &of_accepter {
            let stanza = self.friend_message(*to, requester, "INBOUND", created_at);
            self.deliver(Some(ADMIN_LOCAL), *to, &stanza);
        }
        for to in &of_requester {
            let stanza = self.friend_message(*to, accepter, "OUTBOUND", created_at);
            self.deliver(Some(ADMIN_LOCAL), *to, &stanza);
        }
        for (from, to) in [
            (self.shown_of(requester), self.listening_of(accepter)),
            (self.shown_of(accepter), self.listening_of(requester)),
        ] {
            for sender in &from {
                let sender_uid = self.sessions.get(sender).and_then(|s| s.uid.clone());
                for receiver in &to {
                    let stanza = self.presence_stanza(*sender, *receiver);
                    self.deliver(sender_uid.as_deref(), *receiver, &stanza);
                }
            }
        }
        if of_requester.len() + of_accepter.len() > 0 {
            tracing::info!(
                "chat: presence: {requester} and {accepter} are friends now: told {} and {} session(s)",
                of_requester.len(),
                of_accepter.len()
            );
        }
    }

    /// Each sees the other go, once.
    fn ended_friendship(&mut self, a: &str, b: &str) {
        for (from, to) in [
            (self.shown_of(a), self.listening_of(b)),
            (self.shown_of(b), self.listening_of(a)),
        ] {
            for sender in &from {
                let sender_uid = self.sessions.get(sender).and_then(|s| s.uid.clone());
                for receiver in &to {
                    let stanza = self.unavailable_stanza(*sender, *receiver);
                    self.deliver(sender_uid.as_deref(), *receiver, &stanza);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use service_kit::env::Env;
    use tokio::sync::mpsc;
    use tracing_subscriber::layer::SubscriberExt;

    use super::super::{Chat, FriendsLookup, Options, Out, Outbox, Settings};
    use crate::chat::xmpp::{escape, parse_frame};

    /// The error lines logged while it runs.
    struct Errors(Arc<Mutex<Vec<String>>>);
    struct Message(String);
    impl tracing::field::Visit for Message {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Errors {
        fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            if *event.metadata().level() == tracing::Level::ERROR {
                let mut message = Message(String::new());
                event.record(&mut message);
                self.0.lock().unwrap().push(message.0);
            }
        }
    }

    /// A broken host's friends lookup: it names a friend whose sessions are the player's own.
    struct Broken;
    impl FriendsLookup for Broken {
        fn friends_of(&self, _: &str) -> Vec<String> {
            vec!["UID-guard-friend".into()]
        }
        fn blocked_either_way(&self, _: &str, _: &str) -> bool {
            false
        }
    }

    fn outbox() -> (Outbox, mpsc::UnboundedReceiver<Out>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let out = Outbox {
            sender,
            queued: Arc::default(),
            closed: Arc::default(),
            terminate: Arc::default(),
        };
        (out, receiver)
    }

    /// presence.test.ts: "the last guard: a stanza that would reach a session of its own sender's
    /// account is refused and logged"
    #[tokio::test]
    async fn a_stanza_to_a_session_of_its_own_senders_account_is_refused_and_logged() {
        let keys: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/node-crypto.json")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("undaunted.db").to_string_lossy().into_owned();
        let env = Env::from_pairs([
            ("PORT", "0"),
            ("AUTH_MODE", "APIKEY"),
            ("NODE_ENV", "production"),
            ("DB_FILENAME", db.as_str()),
            ("AUTH_SIGNING_PRIVKEY_B64", keys["privB64"].as_str().unwrap()),
            ("AUTH_SIGNING_PUBKEY_B64", keys["pubB64"].as_str().unwrap()),
        ]);
        let config = crate::config::Config::from_env(&env).unwrap();
        let state = crate::server::prepare(&config, crate::progression_config::from_env(&env).unwrap())
            .unwrap()
            .state;
        let mut options = Options::from_settings(&Settings::from_env(&env));
        options.friends = Some(Arc::new(Broken));
        let mut chat = Chat::new(state, options);
        let (mine_out, _mine_heard) = outbox();
        let (twin_out, mut twin_heard) = outbox();
        let mine = chat.connected("127.0.0.1".into(), "direct", mine_out);
        let twin = chat.connected("127.0.0.1".into(), "direct", twin_out);
        for (id, uid) in [(mine, "UID-guard"), (twin, "uid-GUARD")] {
            let session = chat.sessions.get_mut(&id).unwrap();
            session.uid = Some(uid.into());
            session.resource = Some(format!("V2:test:WIN::{id}"));
            session.last_presence = Some(Default::default());
        }
        // The broken host files the player's own second session under the friend
        chat.by_uid.insert("UID-guard-friend".into(), vec![twin]);

        let errors = Arc::new(Mutex::new(Vec::new()));
        let _logging = tracing::subscriber::set_default(tracing_subscriber::registry().with(Errors(errors.clone())));
        let status = r#"{"Status":"Dauntless - In the city","Note_s":"<&'\">"}"#;
        let node = parse_frame(&format!("<presence><status>{}</status></presence>", escape(status))).unwrap();
        chat.presence_broadcast(mine, &node).await;
        chat.presence_unavailable(mine, "test").await;

        assert!(twin_heard.try_recv().is_err(), "nothing reached the twin");
        assert_eq!(
            *errors.lock().unwrap(),
            [format!(
                "chat: presence: refused to send c={twin} a stanza from its own account"
            )]
        );
    }
}
