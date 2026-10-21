//! Chat rooms (multi-user chat) the way the 1.4.4 client needs them (`realtime/muc.ts`;
//! docs/findings/chat.md, "Rooms"):
//!
//! - A room presence is the client's only if it comes from exactly "muc." + its domain. Each occupant
//!   is known by the nickname it joined with (name, account id, resource), and `<item jid>` names the
//!   member; a message is matched to its sender through that occupant's presence.
//! - The joiner's own presence completes the join; an error presence drops a pending join cleanly;
//!   its own unavailable presence completes a leave.
//!
//! So every nickname is kept byte for byte and echoed in every "from"; every occupant presence carries
//! `<item jid>`; occupants learn about each other both ways; every message goes back to its sender
//! too; a refused join is an error presence, never a rewritten nickname. Two sessions of one account
//! never see each other in a room, and one account is in a room with one session at most: when a
//! reconnected session joins a room its ghost is still in, the ghost leaves first ("replaced").
//!
//! City-<id>, Hunt-<id> and General<id> are open to every signed-in player; Party-<partyId> only to
//! that party's members and Guild-<guildId> only to that guild's; anything else is refused.
//! Membership is checked at the join, at every message (sender and recipients) and every 60 s; a
//! player who lost it is removed from the room (status 307). Room messages skip recipients who
//! blocked the sender.

use super::nick::{check_nickname, name_part_of};
use super::xmpp::{Element, Jid, NS_CLIENT, NS_MUC, NS_MUC_USER, NS_STANZAS, escape, log_text, text_of};
use super::{Chat, NickCheck, Session};
use crate::app::AppError;

pub const MAX_ROOMS_PER_SESSION: usize = 8;
pub const MAX_OCCUPANTS: usize = 128;
pub const MAX_ROOMS: usize = 1000;
pub const MAX_NICK_BYTES: usize = 1023;
pub const JOIN_BURST: u32 = 10;
const JOIN_REFILL_MS: f64 = 6.0 * 1000.0;
pub const MESSAGE_BURST: u32 = 8;
const MESSAGE_REFILL_MS: f64 = 1000.0;
const BODY_LIMIT: usize = 2048;
const MAX_ID: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    Left,
    Disconnect,
    Evicted,
    Replaced,
}

impl LeaveReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Disconnect => "disconnect",
            Self::Evicted => "evicted",
            Self::Replaced => "replaced",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomClass {
    Zone,
    General,
    Party,
    Guild,
}

impl RoomClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Zone => "zone",
            Self::General => "general",
            Self::Party => "party",
            Self::Guild => "guild",
        }
    }
}

/// `RoomClassOf`: the class of a room by its local part, exactly as the client builds it.
pub fn room_class_of(local: &str) -> Option<(RoomClass, String)> {
    let (class, id) = [
        ("City-", RoomClass::Zone),
        ("Hunt-", RoomClass::Zone),
        ("General", RoomClass::General),
        ("Party-", RoomClass::Party),
        ("Guild-", RoomClass::Guild),
    ]
    .iter()
    .find_map(|(prefix, class)| local.strip_prefix(prefix).map(|id| (*class, id)))?;
    let valid = id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && (class == RoomClass::General || !id.is_empty());
    valid.then(|| (class, id.to_owned()))
}

/// `IsMucDomain`: muc.<domain>, or the legacy conference.<domain> alias some clients send.
pub fn is_muc_domain(account_domain: &str, domain: &str) -> bool {
    let (actual, account) = (domain.to_lowercase(), account_domain.to_lowercase());
    actual == format!("muc.{account}") || actual == format!("conference.{account}")
}

pub fn take_message_token(session: &mut Session, now: i64) -> bool {
    session.messages.take(MESSAGE_BURST, MESSAGE_REFILL_MS, now)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Nick(super::nick::NickReason),
    NotMember,
    NotAllowed,
    Conflict,
    Limit,
}

impl Refusal {
    fn as_str(self) -> &'static str {
        match self {
            Self::Nick(reason) => reason.as_str(),
            Self::NotMember => "not-member",
            Self::NotAllowed => "not-allowed",
            Self::Conflict => "conflict",
            Self::Limit => "limit",
        }
    }

    fn error(self) -> (&'static str, &'static str) {
        match self {
            Self::Nick(_) | Self::NotMember => ("auth", "forbidden"),
            Self::NotAllowed => ("cancel", "not-allowed"),
            Self::Conflict => ("cancel", "conflict"),
            Self::Limit => ("wait", "service-unavailable"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Room {
    /// <local>@<muc domain>, the local part as the client sent it
    pub jid: String,
    pub local: String,
    pub class: RoomClass,
    /// The party or guild id
    pub class_id: String,
    /// Session id and nickname, in join order
    pub occupants: Vec<(u64, String)>,
}

impl Room {
    fn nick_of(&self, id: u64) -> Option<&str> {
        self.occupants.iter().find(|(o, _)| *o == id).map(|(_, n)| n.as_str())
    }
}

#[derive(Default)]
struct Shown {
    self_: bool,
    unavailable: bool,
    evicted: bool,
}

impl Chat {
    fn room_index(&self, jid: &str) -> Option<usize> {
        self.rooms.iter().position(|r| r.jid == jid)
    }

    fn full_jid_of(&self, id: u64) -> String {
        self.sessions.get(&id).map(Session::full_jid).unwrap_or_default()
    }

    fn occupant_presence(&self, room_jid: &str, nick: &str, occupant: u64, to: u64, shown: Shown) -> String {
        let statuses = format!(
            "{}{}",
            if shown.self_ { "<status code=\"110\"/>" } else { "" },
            if shown.evicted { "<status code=\"307\"/>" } else { "" }
        );
        format!(
            "<presence xmlns=\"{NS_CLIENT}\"{} from=\"{}\" to=\"{}\"><x xmlns=\"{NS_MUC_USER}\"><item affiliation=\"none\" role=\"{}\" jid=\"{}\"/>{statuses}</x></presence>",
            if shown.unavailable { " type=\"unavailable\"" } else { "" },
            escape(&format!("{room_jid}/{nick}")),
            escape(&self.full_jid_of(to)),
            if shown.unavailable { "none" } else { "participant" },
            escape(&self.full_jid_of(occupant))
        )
    }

    /// A refused join: the error presence, one log line per session, room class and reason every 10
    /// minutes, and one dropped stanza toward the session's abuse limit.
    async fn refuse_join(&mut self, id: u64, room_jid: &str, local: &str, nick: &str, reason: Refusal) {
        let class = room_class_of(local).map_or("other", |(c, _)| c.as_str());
        if self.log_once(&format!("join-refused|{id}|{class}|{}", reason.as_str()), None) {
            tracing::info!(
                "chat: join refused room={} uid={} reason={}",
                log_text(local, 80),
                self.sessions
                    .get(&id)
                    .and_then(|s| s.uid.as_deref())
                    .unwrap_or("undefined"),
                reason.as_str()
            );
        }
        let (kind, condition) = reason.error();
        let stanza = format!(
            "<presence xmlns=\"{NS_CLIENT}\" type=\"error\" from=\"{}\" to=\"{}\"><x xmlns=\"{NS_MUC}\"/><error type=\"{kind}\"><{condition} xmlns=\"{NS_STANZAS}\"/></error></presence>",
            escape(&format!("{room_jid}/{nick}")),
            escape(&self.full_jid_of(id))
        );
        self.send(id, &stanza);
        self.drop_stanza(id, "refused").await;
    }

    fn message_error(&mut self, id: u64, room_jid: &str, message_id: Option<&str>) {
        let stanza = format!(
            "<message xmlns=\"{NS_CLIENT}\" type=\"error\"{} from=\"{}\" to=\"{}\"><error type=\"modify\"><not-acceptable xmlns=\"{NS_STANZAS}\"/></error></message>",
            message_id.map(|i| format!(" id=\"{}\"", escape(i))).unwrap_or_default(),
            escape(room_jid),
            escape(&self.full_jid_of(id))
        );
        self.send(id, &stanza);
    }

    /// Party and guild rooms: members only.
    async fn may_use(&self, uid: &str, class: RoomClass, class_id: &str) -> bool {
        match class {
            RoomClass::Party => self.app.parties.lock().party_id_of(uid).as_deref() == Some(class_id),
            RoomClass::Guild => {
                let (uid, guild) = (uid.to_owned(), class_id.to_owned());
                self.app
                    .store
                    .read(move |conn| super::is_guild_member(conn, &uid, &guild).map_err(AppError::from))
                    .await
                    .unwrap_or(false)
            }
            RoomClass::Zone | RoomClass::General => true,
        }
    }

    async fn still_allowed(&self, id: u64, room: usize) -> bool {
        let Some(uid) = self.sessions.get(&id).and_then(|s| s.uid.clone()) else {
            return false;
        };
        let Some(room) = self.rooms.get(room) else {
            return false;
        };
        self.may_use(&uid, room.class, &room.class_id.clone()).await
    }

    /// Everyone in the room this session may see: not itself, not its own account's other sessions.
    fn others(&self, room: usize, id: u64) -> Vec<(u64, String)> {
        let uid = self.sessions.get(&id).and_then(|s| s.uid.clone());
        self.rooms[room]
            .occupants
            .iter()
            .filter(|(other, _)| *other != id && self.sessions.get(other).and_then(|s| s.uid.clone()) != uid)
            .cloned()
            .collect()
    }

    pub async fn join(&mut self, id: u64, to: &Jid) {
        let now = self.now();
        let nick = to.resource.clone().unwrap_or_default();
        let room_jid = format!("{}@{}", to.local, to.domain);
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        let uid = session.uid.clone().unwrap_or_default();
        // Every join takes a token first, refused ones included
        if !session.joins.take(JOIN_BURST, JOIN_REFILL_MS, now) {
            self.refuse_join(id, &room_jid, &to.local, &nick, Refusal::Limit).await;
            return;
        }
        let kind = room_class_of(&to.local);
        let domain = session.domain.clone();
        let (Some((class, class_id)), true) = (kind, is_muc_domain(&domain, &to.domain)) else {
            self.refuse_join(id, &room_jid, &to.local, &nick, Refusal::NotAllowed)
                .await;
            return;
        };
        let existing = self.room_index(&room_jid);
        let held = self
            .sessions
            .get(&id)
            .and_then(|s| s.room_nick(&room_jid))
            .map(str::to_owned);
        // Party and guild rooms: members only (someone who left is also removed from the room)
        if !self.may_use(&uid, class, &class_id).await {
            if let (Some(room), Some(_)) = (existing, &held) {
                self.remove(id, room, LeaveReason::Evicted);
            }
            self.refuse_join(id, &room_jid, &to.local, &nick, Refusal::NotMember)
                .await;
            return;
        }
        // Rejoin with the same nickname: this session alone gets the room again
        if let Some(room) = existing
            && held.as_deref() == Some(nick.as_str())
        {
            self.welcome(id, room, &nick);
            tracing::info!(
                "chat: join room={} uid={uid} name={} occupants={} rejoin",
                to.local,
                name_part_of(&nick).unwrap_or_else(|| "?".into()),
                self.others(room, id).len()
            );
            return;
        }
        if nick.is_empty() || nick.len() > MAX_NICK_BYTES {
            self.refuse_join(
                id,
                &room_jid,
                &to.local,
                &nick,
                Refusal::Nick(super::nick::NickReason::Format),
            )
            .await;
            return;
        }
        let lookup = uid.clone();
        let current = self
            .app
            .store
            .read(move |conn| crate::accounts::username_for(conn, &lookup).map_err(AppError::from))
            .await
            .ok()
            .flatten();
        let (resource, at_login) = self
            .sessions
            .get(&id)
            .map(|s| (s.resource.clone().unwrap_or_default(), s.name_at_login.clone()))
            .unwrap_or_default();
        let check = check_nickname(&nick, &uid, &resource, &[current.as_deref(), at_login.as_deref()]);
        // CHAT_NICK_CHECK=log relaxes the resource, format and name rules only; another account's id
        // is refused in both modes
        if let Err(reason) = check {
            if self.nick_check == NickCheck::Enforce || reason == super::nick::NickReason::Account {
                self.refuse_join(id, &room_jid, &to.local, &nick, Refusal::Nick(reason))
                    .await;
                return;
            }
            if self.log_once(&format!("nick-log|{id}|{}", to.local), None) {
                tracing::warn!(
                    "chat: join nickname not checked room={} uid={uid} reason={} (CHAT_NICK_CHECK=log)",
                    to.local,
                    reason.as_str()
                );
            }
        }
        // A different nickname in a room this session is already in: leave with the old one first
        if let (Some(room), Some(_)) = (existing, &held) {
            self.remove(id, room, LeaveReason::Left);
        }
        let room = self.room_index(&room_jid);
        // The occupants of other accounts. An older session of this account is replaced below.
        let other_accounts: Vec<(u64, String)> = room
            .map(|r| {
                self.rooms[r]
                    .occupants
                    .iter()
                    .filter(|(other, _)| {
                        *other != id && self.sessions.get(other).and_then(|s| s.uid.as_deref()) != Some(uid.as_str())
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if other_accounts.iter().any(|(_, other_nick)| *other_nick == nick) {
            self.refuse_join(id, &room_jid, &to.local, &nick, Refusal::Conflict)
                .await;
            return;
        }
        let held_rooms = self.sessions.get(&id).map_or(0, |s| s.rooms.len());
        if held_rooms >= MAX_ROOMS_PER_SESSION
            || other_accounts.len() >= MAX_OCCUPANTS
            || (room.is_none() && self.rooms.len() >= MAX_ROOMS)
        {
            self.refuse_join(id, &room_jid, &to.local, &nick, Refusal::Limit).await;
            return;
        }
        let room = match room {
            Some(room) => room,
            None => {
                self.rooms.push(Room {
                    jid: room_jid.clone(),
                    local: to.local.clone(),
                    class,
                    class_id,
                    occupants: Vec::new(),
                });
                self.rooms.len() - 1
            }
        };
        // 0. This account's older session in the room (normally a ghost) leaves first, told to the
        // others but not to itself
        let ghosts: Vec<u64> = self.rooms[room]
            .occupants
            .iter()
            .map(|(o, _)| *o)
            .filter(|other| {
                *other != id && self.sessions.get(other).and_then(|s| s.uid.as_deref()) == Some(uid.as_str())
            })
            .collect();
        for ghost in ghosts {
            self.remove(ghost, room, LeaveReason::Replaced);
        }
        // remove() drops an emptied room; the joiner's own room is held until it is in it
        let room = match self.room_index(&room_jid) {
            Some(room) => room,
            None => {
                self.rooms.push(Room {
                    jid: room_jid.clone(),
                    local: to.local.clone(),
                    class,
                    class_id: room_class_of(&to.local).map(|(_, i)| i).unwrap_or_default(),
                    occupants: Vec::new(),
                });
                self.rooms.len() - 1
            }
        };
        let others = self.others(room, id);
        // 1. The joiner hears of everyone already there; 2. they hear of the joiner; 3. the joiner's
        // own presence last, which completes the join on its screen
        for (other, other_nick) in &others {
            let stanza = self.occupant_presence(&room_jid, other_nick, *other, id, Shown::default());
            self.send(id, &stanza);
        }
        for (other, _) in &others {
            let stanza = self.occupant_presence(&room_jid, &nick, id, *other, Shown::default());
            self.send(*other, &stanza);
        }
        self.rooms[room].occupants.push((id, nick.clone()));
        if let Some(session) = self.sessions.get_mut(&id) {
            session.rooms.push((room_jid.clone(), nick.clone()));
        }
        let own = self.occupant_presence(
            &room_jid,
            &nick,
            id,
            id,
            Shown {
                self_: true,
                ..Shown::default()
            },
        );
        self.send(id, &own);
        tracing::info!(
            "chat: join room={} uid={uid} name={} occupants={}",
            to.local,
            check.ok().or_else(|| name_part_of(&nick)).unwrap_or_else(|| "?".into()),
            others.len()
        );
    }

    /// Party and guild rooms: anyone who left the party or guild is removed.
    pub async fn sweep_rooms(&mut self) {
        let jids: Vec<String> = self
            .rooms
            .iter()
            .filter(|r| matches!(r.class, RoomClass::Party | RoomClass::Guild))
            .map(|r| r.jid.clone())
            .collect();
        for jid in jids {
            let occupants: Vec<u64> = self
                .room_index(&jid)
                .map(|r| self.rooms[r].occupants.iter().map(|(o, _)| *o).collect())
                .unwrap_or_default();
            for occupant in occupants {
                let Some(room) = self.room_index(&jid) else {
                    break;
                };
                if !self.still_allowed(occupant, room).await
                    && let Some(room) = self.room_index(&jid)
                {
                    self.remove(occupant, room, LeaveReason::Evicted);
                }
            }
        }
    }

    fn welcome(&mut self, id: u64, room: usize, nick: &str) {
        let room_jid = self.rooms[room].jid.clone();
        for (other, other_nick) in self.others(room, id) {
            let stanza = self.occupant_presence(&room_jid, &other_nick, other, id, Shown::default());
            self.send(id, &stanza);
        }
        let own = self.occupant_presence(
            &room_jid,
            nick,
            id,
            id,
            Shown {
                self_: true,
                ..Shown::default()
            },
        );
        self.send(id, &own);
    }

    pub fn leave(&mut self, id: u64, to: &Jid) {
        let room_jid = format!("{}@{}", to.local, to.domain);
        let room = self
            .room_index(&room_jid)
            .filter(|r| self.rooms[*r].nick_of(id).is_some());
        let Some(room) = room else {
            // Not in it (after a refused join, or a restart): confirm the leave anyway, so the client's
            // room does not stay waiting for it
            let Some(session) = self.sessions.get(&id) else {
                return;
            };
            if is_muc_domain(&session.domain, &to.domain)
                && let Some(resource) = to.resource.as_deref().filter(|r| !r.is_empty())
            {
                let jid = escape(&session.full_jid());
                let stanza = format!(
                    "<presence xmlns=\"{NS_CLIENT}\" type=\"unavailable\" from=\"{}\" to=\"{jid}\"><x xmlns=\"{NS_MUC_USER}\"><item affiliation=\"none\" role=\"none\" jid=\"{jid}\"/><status code=\"110\"/></x></presence>",
                    escape(&format!("{room_jid}/{resource}"))
                );
                self.send(id, &stanza);
            }
            return;
        };
        self.remove(id, room, LeaveReason::Left);
    }

    /// A broadcast unavailable, or the end of the session.
    pub fn leave_all(&mut self, id: u64, reason: LeaveReason) {
        let held: Vec<String> = self
            .sessions
            .get(&id)
            .map(|s| s.rooms.iter().map(|(r, _)| r.clone()).collect())
            .unwrap_or_default();
        for jid in held {
            if let Some(room) = self.room_index(&jid) {
                self.remove(id, room, reason);
            }
        }
        if let Some(session) = self.sessions.get_mut(&id) {
            session.rooms.clear();
        }
    }

    fn remove(&mut self, id: u64, room: usize, reason: LeaveReason) {
        let Some(nick) = self.rooms[room].nick_of(id).map(str::to_owned) else {
            return;
        };
        let evicted = reason == LeaveReason::Evicted;
        let room_jid = self.rooms[room].jid.clone();
        for (other, _) in self.others(room, id) {
            let stanza = self.occupant_presence(
                &room_jid,
                &nick,
                id,
                other,
                Shown {
                    unavailable: true,
                    evicted,
                    ..Shown::default()
                },
            );
            self.send(other, &stanza);
        }
        self.rooms[room].occupants.retain(|(o, _)| *o != id);
        let ended = match self.sessions.get_mut(&id) {
            Some(session) => {
                session.rooms.retain(|(r, _)| *r != room_jid);
                session.ended
            }
            None => true,
        };
        if !ended && matches!(reason, LeaveReason::Left | LeaveReason::Evicted) {
            let stanza = self.occupant_presence(
                &room_jid,
                &nick,
                id,
                id,
                Shown {
                    self_: true,
                    unavailable: true,
                    evicted,
                },
            );
            self.send(id, &stanza);
        }
        let local = self.rooms[room].local.clone();
        if self.rooms[room].occupants.is_empty() {
            self.rooms.remove(room);
        }
        tracing::info!(
            "chat: leave room={local} uid={} reason={}",
            self.sessions
                .get(&id)
                .and_then(|s| s.uid.as_deref())
                .unwrap_or("undefined"),
            reason.as_str()
        );
    }

    // ---- Room messages ----

    pub async fn group_chat(&mut self, id: u64, to: &Jid, node: &Element) {
        let room_jid = format!("{}@{}", to.local, to.domain);
        let message_id = node
            .attr("id")
            .filter(|i| i.encode_utf16().count() <= MAX_ID)
            .map(str::to_owned);
        let body_node = node.child("body");
        // A subject change alone is ignored
        if body_node.is_none() && node.child("subject").is_some() {
            return;
        }
        let room = self.room_index(&room_jid);
        let nick = room.and_then(|r| self.rooms[r].nick_of(id)).map(str::to_owned);
        let body = text_of(body_node);
        let length = body.chars().count();
        let (Some(room), Some(nick), true) = (room, nick, (1..=BODY_LIMIT).contains(&length)) else {
            self.message_error(id, &room_jid, message_id.as_deref());
            return;
        };
        if !self.still_allowed(id, room).await {
            self.message_error(id, &room_jid, message_id.as_deref());
            if let Some(room) = self.room_index(&room_jid) {
                self.remove(id, room, LeaveReason::Evicted);
            }
            return;
        }
        let now = self.now();
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if !take_message_token(session, now) {
            let uid = session.uid.clone().unwrap_or_default();
            if self.log_once(&format!("message-rate|{id}"), Some(60 * 1000)) {
                tracing::warn!("chat: message limit c={id} uid={uid} room={}", self.rooms[room].local);
            }
            self.message_error(id, &room_jid, message_id.as_deref());
            return;
        }
        let uid = session.uid.clone().unwrap_or_default();
        let room_full = self.rooms[room].jid.clone();
        let local = self.rooms[room].local.clone();
        let from = escape(&format!("{room_full}/{nick}"));
        let text = escape(&body);
        let recipients: Vec<u64> = self.rooms[room]
            .occupants
            .iter()
            .map(|(o, _)| *o)
            .filter(|recipient| {
                *recipient == id || self.sessions.get(recipient).and_then(|s| s.uid.as_deref()) != Some(uid.as_str())
            })
            .collect();
        let recipient_uids: Vec<String> = recipients
            .iter()
            .filter_map(|r| self.sessions.get(r).and_then(|s| s.uid.clone()))
            .collect();
        let sender = uid.clone();
        let blockers = self
            .app
            .store
            .read(move |conn| super::blockers_among(conn, &sender, &recipient_uids).map_err(AppError::from))
            .await
            .unwrap_or_default();
        let id_attr = message_id
            .map(|i| format!(" id=\"{}\"", escape(&i)))
            .unwrap_or_default();
        let (mut delivered, mut blocked) = (0, 0);
        for recipient in recipients {
            // A recipient who left the party or guild is removed instead of served
            if recipient != id {
                let Some(room) = self.room_index(&room_jid) else {
                    break;
                };
                if !self.still_allowed(recipient, room).await {
                    if let Some(room) = self.room_index(&room_jid) {
                        self.remove(recipient, room, LeaveReason::Evicted);
                    }
                    continue;
                }
            }
            let recipient_uid = self
                .sessions
                .get(&recipient)
                .and_then(|s| s.uid.clone())
                .unwrap_or_default();
            if blockers.contains(&recipient_uid) {
                blocked += 1;
                continue;
            }
            let stanza = format!(
                "<message xmlns=\"{NS_CLIENT}\" type=\"groupchat\"{id_attr} from=\"{from}\" to=\"{}\"><body>{text}</body></message>",
                escape(&self.full_jid_of(recipient))
            );
            self.send(recipient, &stanza);
            delivered += 1;
        }
        tracing::info!(
            "chat: message room={local} uid={uid} len={length} to={delivered}{}",
            if blocked > 0 {
                format!(" blocked={blocked}")
            } else {
                String::new()
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_are_classed_like_the_client_names_them() {
        assert_eq!(room_class_of("City-abc-1"), Some((RoomClass::Zone, "abc-1".into())));
        assert_eq!(room_class_of("General"), Some((RoomClass::General, String::new())));
        assert_eq!(room_class_of("General7"), Some((RoomClass::General, "7".into())));
        assert_eq!(room_class_of("Party-"), None);
        assert_eq!(room_class_of("city-1"), None);
        assert_eq!(room_class_of("Guild-a_b"), None);
        assert_eq!(room_class_of(&format!("Hunt-{}", "a".repeat(65))), None);
        assert!(is_muc_domain("Dom", "MUC.dom"));
        assert!(is_muc_domain("dom", "conference.dom"));
        assert!(!is_muc_domain("dom", "muc.other"));
    }
}
