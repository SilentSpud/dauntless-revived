//! A port of the Node tests' `chatclient.ts`: a small model of how the 1.4.4 client reads chat
//! (docs/findings/chat.md). The rules were read from the game's executable (image base 0x140000000;
//! the addresses are virtual addresses in that image). Tests feed it the frames a server sent, so a
//! reply the real client would read differently fails a test even when it "looks right".
//!
//! It parses frames with the chat's own XML reader (a port of ltx, which the Node model used); every
//! rule of the model is its own.

use std::collections::HashMap;

use metagame::chat::xmpp::{Element, parse_frame};

// ---- The client's URL encoding (FGenericPlatformHttp) ----

/// UrlEncode (0x1428aebb0): the text as UTF-8; A-Z a-z 0-9 - _ . ~ kept; every other byte %XX in upper
/// case
pub(crate) fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// UrlDecode (0x1428ae680): %XX back to bytes, then UTF-8
pub(crate) fn url_decode(text: &str) -> String {
    let mut bytes = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let hex: String = chars.iter().skip(index + 1).take(2).collect();
        if chars[index] == '%' && hex.len() == 2 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            bytes.push(u8::from_str_radix(&hex, 16).unwrap());
            index += 3;
        } else {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(chars[index].encode_utf8(&mut buffer).as_bytes());
            index += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---- JIDs ----

struct Jid {
    local: String,
    domain: String,
    resource: String,
}

fn parse_jid(text: &str) -> Jid {
    let (bare, resource) = match text.find('/') {
        Some(slash) => (&text[..slash], &text[slash + 1..]),
        None => (text, ""),
    };
    let (local, domain) = match bare.find('@') {
        Some(at) => (&bare[..at], &bare[at + 1..]),
        None => ("", bare),
    };
    Jid {
        local: local.to_owned(),
        domain: domain.to_owned(),
        resource: resource.to_owned(),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn child<'a>(node: Option<&'a Element>, name: &str) -> Option<&'a Element> {
    node?.elements().find(|e| e.name == name)
}

// ---- Rooms ----

/// The Xmpp MUC room states (to-string switch 0x143a32c00)
pub(crate) const NOT_JOINED: u8 = 0;
pub(crate) const JOINED: u8 = 1;
pub(crate) const JOIN_PUBLIC_PENDING: u8 = 4;
pub(crate) const EXIT_PENDING: u8 = 5;

/// A room member (parsed at 0x1408fe300 / 0x1408c3500)
#[derive(Debug, Clone)]
pub(crate) struct Member {
    /// The full room JID it was seen from
    pub from: String,
    /// Member id: `<item jid>`'s node, else nickname part 2, else the room JID's full path
    pub id: String,
    /// Nickname part 1 URL-decoded, or the whole nickname when it has no ":"
    pub name: String,
}

/// A room as the client keeps it. `members` is keyed by member id in lower case: one entry per account,
/// however many connections it has in the room (0x1408fe300, 0x1408c0680).
#[derive(Debug, Clone)]
pub(crate) struct Room {
    pub room_id: String,
    pub state: u8,
    pub nick: String,
    pub members: HashMap<String, Member>,
}

/// The member id the client takes from a leaving occupant: nickname part 2, else the room JID's full path
fn exit_id_of(from: &str, nick: &str) -> String {
    if nick.contains(':') {
        nick.split(':').nth(1).unwrap_or_default().to_owned()
    } else {
        from.to_owned()
    }
}

/// A line shown in a room. `member` is `None` when the sender matched no occupant.
#[derive(Debug, Clone)]
pub(crate) struct RoomLine {
    pub member: Option<Member>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Whisper {
    pub from_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinResult {
    pub room: String,
    pub ok: bool,
}

fn parse_member(from: &str, nick: &str, item: Option<&Element>) -> Member {
    let colon = nick.find(':');
    let id = match item.and_then(|i| i.attr("jid")) {
        Some(jid) if jid.chars().count() > 1 => parse_jid(jid).local,
        _ => match colon {
            Some(_) => nick.split(':').nth(1).unwrap_or_default().to_owned(),
            // No ":" at all: the id becomes the room JID's full path (0x1408c3686)
            None => from.to_owned(),
        },
    };
    Member {
        from: from.to_owned(),
        id,
        name: match colon {
            Some(colon) => url_decode(&nick[..colon]),
            None => nick.to_owned(),
        },
    }
}

/// Names by account id, as the `?accountId=` route answers them
pub(crate) fn lookup_from<'a>(names: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |id| {
        names
            .iter()
            .find(|(uid, _)| *uid == id)
            .map(|(_, name)| (*name).to_owned())
    }
}

pub(crate) struct ChatClientModel {
    pub local_uid: String,
    pub domain: String,
    pub resource: String,
    pub local_social_name: String,
    pub logged_in: bool,
    rooms: HashMap<String, Room>,
    pub log: Vec<String>,
    pub lines: Vec<RoomLine>,
    pub whispers: Vec<Whisper>,
    pub join_results: Vec<JoinResult>,
    pub exit_results: Vec<String>,
}

impl ChatClientModel {
    pub(crate) fn new(local_uid: &str, resource: &str, local_social_name: &str, domain: Option<&str>) -> Self {
        Self {
            local_uid: local_uid.to_owned(),
            domain: domain.unwrap_or("prod.ol.epicgames.com").to_owned(),
            resource: resource.to_owned(),
            local_social_name: local_social_name.to_owned(),
            logged_in: true,
            rooms: HashMap::new(),
            log: Vec::new(),
            lines: Vec::new(),
            whispers: Vec::new(),
            join_results: Vec::new(),
            exit_results: Vec::new(),
        }
    }

    /// "muc." + the connection's domain (0x143a33392)
    pub(crate) fn muc_domain(&self) -> String {
        format!("muc.{}", self.domain)
    }

    /// `UrlEncode(Nickname):UserId:Resource` (0x1408b5aeb); an empty display name is "InvalidMCPUser"
    pub(crate) fn nickname(&self, display_name: &str) -> String {
        let name = if display_name.is_empty() {
            "InvalidMCPUser"
        } else {
            display_name
        };
        format!("{}:{}:{}", url_encode(name), self.local_uid, self.resource)
    }

    /// The room lookup ignores case
    pub(crate) fn room_of(&self, room_id: &str) -> Option<&Room> {
        self.rooms.get(&room_id.to_lowercase())
    }

    /// JoinPublicRoom (0x143a3267b): needs a logged-in connection and a room that is not joining or
    /// leaving, then state 4. Returns the stanza sent.
    pub(crate) fn join_public_room(&mut self, room_id: &str, display_name: &str) -> Option<String> {
        if !self.logged_in {
            self.log
                .push("MUC: JoinPublicRoom failed. Not currently connected".into());
            return None;
        }
        let existing = self.room_of(room_id).cloned();
        if let Some(existing) = &existing
            && existing.state != NOT_JOINED
            && existing.state != JOINED
        {
            self.log.push(format!(
                "MUC: JoinPublicRoom failed. Another operation already pending for room {room_id}"
            ));
            return None;
        }
        let nick = self.nickname(display_name);
        self.rooms.insert(
            room_id.to_lowercase(),
            Room {
                room_id: room_id.to_owned(),
                state: JOIN_PUBLIC_PENDING,
                nick: nick.clone(),
                members: existing.map(|r| r.members).unwrap_or_default(),
            },
        );
        Some(format!(
            r#"<presence to="{}"><x xmlns="http://jabber.org/protocol/muc"><history maxstanzas="50"/></x></presence>"#,
            escape(&format!("{room_id}@{}/{nick}", self.muc_domain()))
        ))
    }

    /// ExitRoom: state 5 until the room's own unavailable presence arrives
    pub(crate) fn exit_room(&mut self, room_id: &str) -> Option<String> {
        let muc = self.muc_domain();
        let room = self.rooms.get_mut(&room_id.to_lowercase())?;
        if room.state != JOINED {
            return None;
        }
        room.state = EXIT_PENDING;
        Some(format!(
            r#"<presence type="unavailable" to="{}"/>"#,
            escape(&format!("{}@{muc}/{}", room.room_id, room.nick))
        ))
    }

    /// A groupchat as SendChat builds it (0x143a3a6b0): to the bare room JID
    pub(crate) fn room_message(&self, room_id: &str, text: &str, id: &str) -> String {
        format!(
            r#"<message id="{}" type="groupchat" to="{}"><body>{}</body></message>"#,
            escape(id),
            escape(&format!("{room_id}@{}", self.muc_domain())),
            escape(text)
        )
    }

    /// A whisper (builder 0x143a3a9d0): to the bare JID of the account
    pub(crate) fn whisper_to(&self, account_id: &str, text: &str) -> String {
        format!(
            r#"<message type="chat" to="{}"><body>{}</body></message>"#,
            escape(&format!("{account_id}@{}", self.domain)),
            escape(text)
        )
    }

    pub(crate) fn receive(&mut self, frame: &str) {
        let Some(node) = parse_frame(frame) else {
            self.log.push("XMPP: parse error".into());
            return;
        };
        match node.name.as_str() {
            "presence" => self.presence(&node),
            "message" => self.message(&node),
            _ => {}
        }
    }

    pub(crate) fn receive_all(&mut self, frames: &[String]) {
        for frame in frames {
            self.receive(frame);
        }
    }

    fn presence(&mut self, node: &Element) {
        let from_text = node.attr("from").unwrap_or_default().to_owned();
        let from = parse_jid(&from_text);
        // The MUC module claims a presence only from exactly muc.<Domain>, case included
        if from.domain != self.muc_domain() {
            return;
        }
        let key = from.local.to_lowercase();
        let local_uid = self.local_uid.clone();
        // The presence error handler (0x143a34d80): a pending join is dropped and fails
        if node.attr("type") == Some("error") {
            if let Some(room) = self.rooms.get(&key)
                && room.state == JOIN_PUBLIC_PENDING
            {
                let room_id = room.room_id.clone();
                self.rooms.remove(&key);
                self.join_results.push(JoinResult {
                    room: room_id.clone(),
                    ok: false,
                });
                self.log.push(format!("MUC: join failed for room {room_id}"));
            }
            return;
        }
        let Some(room) = self.rooms.get_mut(&key) else {
            self.log
                .push("MUC: Ignored presence from room we haven't joined".into());
            return;
        };
        let nick = from.resource.clone();
        let unavailable = node.attr("type") == Some("unavailable");
        // The self test: a case-sensitive search for the local account id in the nickname. Status 110
        // is not read.
        let is_self = nick.contains(&local_uid);
        let item = child(child(Some(node), "x"), "item");
        if is_self {
            if unavailable {
                if room.state == EXIT_PENDING {
                    self.exit_results.push(room.room_id.clone());
                }
                room.state = NOT_JOINED;
                room.members.clear();
            } else if room.state == JOIN_PUBLIC_PENDING {
                let member = parse_member(&from_text, &nick, item);
                room.state = JOINED;
                room.members.insert(member.id.to_lowercase(), member);
                let room_id = room.room_id.clone();
                self.join_results.push(JoinResult {
                    room: room_id,
                    ok: true,
                });
            }
            return;
        }
        // Someone else: no state check
        if unavailable {
            room.members.remove(&exit_id_of(&from_text, &nick).to_lowercase());
        } else {
            let member = parse_member(&from_text, &nick, item);
            room.members.insert(member.id.to_lowercase(), member);
        }
    }

    /// The member a room line came from: the entry whose room JID matches, ignoring case (0x1408a83c0)
    fn member_from(room: &Room, from_text: &str) -> Option<Member> {
        let wanted = from_text.to_lowercase();
        room.members.values().find(|m| m.from.to_lowercase() == wanted).cloned()
    }

    fn message(&mut self, node: &Element) {
        let from_text = node.attr("from").unwrap_or_default().to_owned();
        let from = parse_jid(&from_text);
        let body = node.child("body").map(Element::text).unwrap_or_default();
        match node.attr("type") {
            Some("error") => self.log.push("MUC: Received GroupChat error".into()),
            Some("groupchat") => {
                if from.domain != self.muc_domain() {
                    return;
                }
                let Some(room) = self.rooms.get(&from.local.to_lowercase()) else {
                    return;
                };
                if body.is_empty() {
                    return;
                }
                let member = Self::member_from(room, &from_text);
                if member.is_none() {
                    // No occupant: the id comes from the nickname (0x1408b1030)
                    let derived = if from.resource.contains(':') {
                        from.resource.split(':').nth(1).unwrap_or_default().to_owned()
                    } else if !from.resource.is_empty() {
                        from_text.clone()
                    } else {
                        String::new()
                    };
                    if derived.is_empty() {
                        self.log.push(format!(
                            "could not get user id for room={} member={}",
                            from.local, from.resource
                        ));
                        return;
                    }
                }
                self.lines.push(RoomLine { member, text: body });
            }
            Some("chat") => {
                // The shared receive path drops senders without a resource
                if from.resource.is_empty() || from.local.is_empty() {
                    self.log.push("XMPP: dropped a message from a bare JID".into());
                    return;
                }
                self.whispers.push(Whisper {
                    from_id: from.local,
                    text: body,
                });
            }
            _ => {}
        }
    }

    /// The name the chat shows for a room line: "[unknown]" for no occupant, the local social name for
    /// the local account, else the lookup's displayName, or nickname part 1 when it finds nobody
    pub(crate) async fn shown_name(&self, line: &RoomLine, lookup: &impl AsyncFn(&str) -> Option<String>) -> String {
        let Some(member) = &line.member else {
            return "[unknown]".into();
        };
        if member.id == self.local_uid {
            return self.local_social_name.clone();
        }
        match lookup(&member.id).await {
            Some(found) if !found.is_empty() => found,
            _ => member.name.clone(),
        }
    }

    pub(crate) async fn shown_lines_with(&self, lookup: impl AsyncFn(&str) -> Option<String>) -> Vec<String> {
        let mut shown = Vec::new();
        for line in &self.lines {
            shown.push(format!("{}: {}", self.shown_name(line, &lookup).await, line.text));
        }
        shown
    }

    /// `ShownLines` with a fixed list of names
    pub(crate) async fn shown_lines(&self, lookup: impl Fn(&str) -> Option<String>) -> Vec<String> {
        self.shown_lines_with(async |id: &str| lookup(id)).await
    }

    /// A whisper's sender comes from the same lookup; not found: the whisper waits (0x14157e290)
    pub(crate) fn shown_whispers(&self, lookup: impl Fn(&str) -> Option<String>) -> Vec<String> {
        self.whispers
            .iter()
            .map(|w| match lookup(&w.from_id) {
                Some(found) if !found.is_empty() => format!("{found}: {}", w.text),
                _ => format!("(waiting): {}", w.text),
            })
            .collect()
    }
}
