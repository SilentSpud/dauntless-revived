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
        let lookup = uid.to_owned();
        let friends = self
            .app
            .store
            .read(move |conn| crate::friends::list_friends(conn, &lookup, false).map_err(AppError::from))
            .await;
        let friends: Vec<String> = match friends {
            Ok(list) => list
                .iter()
                .filter_map(|f| f.get("accountId").and_then(|v| v.as_str()).map(str::to_owned))
                .collect(),
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
            let (a, b) = (uid.to_owned(), friend.clone());
            let blocked = self
                .app
                .store
                .read(move |conn| crate::friends::blocked_either_way(conn, &a, &b).map_err(AppError::from))
                .await;
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
        let (a, b) = (requester.to_owned(), accepter.to_owned());
        let blocked = self
            .app
            .store
            .read(move |conn| crate::friends::blocked_either_way(conn, &a, &b).map_err(AppError::from))
            .await
            .unwrap_or(true);
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
