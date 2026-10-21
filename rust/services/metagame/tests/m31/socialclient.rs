//! `socialclient.ts`: a small model of how the 1.4.4 client parses the social replies
//! (docs/findings/social.md; the addresses are virtual addresses in Dauntless-Win64-Shipping.exe). Tests
//! check replies against this model, not only against literal JSON, so a reply the real client would
//! drop fails a test even when it "looks right".

use std::collections::{BTreeMap, HashMap};

use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};

/// UE4's FJsonObject keeps its fields in a TMap keyed by FString, whose comparison ignores case, so a
/// field lookup ignores case; with two keys differing only in case the later one wins.
pub(crate) fn field<'a>(object: &'a Value, name: &str) -> Option<&'a Value> {
    object
        .as_object()?
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
        .next_back()
}

fn non_empty_string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn parse_json(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}

fn is_success(status: u16) -> bool {
    (200..=206).contains(&status)
}

// ---- POST /account/mapping (FOnlineUserPhoenix::QueryExternalIdMappings, 0x140b09f60..0x140b0afd4) ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MappedId {
    pub kind: &'static str,
    pub id: String,
}

/// The HTTP status is not looked at. An empty body is "Empty response payload", unparseable JSON is
/// "Invalid response payload". Root "accountMappings" must be an object (an array reads as an empty
/// one); each value must be an object with non-empty string accountId and accountType; "epic" is an
/// MCP (Epic) id, "phoenix" a Phoenix id, anything else is skipped. The key is the asked id.
pub(crate) fn parse_account_mappings(_status: u16, text: &str) -> Result<Vec<(String, MappedId)>, &'static str> {
    if text.is_empty() {
        return Err("Empty response payload");
    }
    let parsed = parse_json(text)
        .filter(Value::is_object)
        .ok_or("Invalid response payload")?;
    let mut mapped = Vec::new();
    if let Some(Value::Object(mappings)) = field(&parsed, "accountMappings") {
        for (key, value) in mappings {
            let (Some(id), Some(kind)) = (
                non_empty_string(field(value, "accountId")),
                non_empty_string(field(value, "accountType")),
            ) else {
                continue;
            };
            let kind = match kind.to_lowercase().as_str() {
                "epic" => "MCP",
                "phoenix" => "Phoenix",
                _ => continue,
            };
            mapped.retain(|(k, _): &(String, MappedId)| k != key);
            mapped.push((
                key.clone(),
                MappedId {
                    kind,
                    id: id.to_owned(),
                },
            ));
        }
    }
    Ok(mapped)
}

// ---- POST /accountinfo/public (FOnlineUserPhoenix::QueryUserInfo, 0x140b5aed0; AddUserInfo 0x140b74870) ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserInfo {
    pub username: String,
    pub epic_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Added {
    Stored(String),
    Ignored(String),
    Failed,
}

impl Added {
    pub(crate) fn key(&self) -> Option<&str> {
        match self {
            Self::Stored(key) | Self::Ignored(key) => Some(key),
            Self::Failed => None,
        }
    }
}

/// A status outside 200-206 or a body that is not a JSON object fails (nothing cached). The cache key
/// is the reply's accountId; an id already cached is left alone (first write wins, 0x140b748f9). The
/// user's "epic:id" comes from the first linkedAccounts entry whose accountType is "epic" (0x140b7499f).
pub(crate) fn add_user_info(cache: &mut HashMap<String, UserInfo>, status: u16, text: &str) -> Added {
    if !is_success(status) {
        return Added::Failed;
    }
    let Some(parsed) = parse_json(text).filter(Value::is_object) else {
        return Added::Failed;
    };
    let Some(key) = non_empty_string(field(&parsed, "accountId")).map(str::to_owned) else {
        return Added::Failed;
    };
    if cache.contains_key(&key) {
        return Added::Ignored(key);
    }
    let epic = field(&parsed, "linkedAccounts")
        .and_then(Value::as_array)
        .and_then(|linked| {
            linked.iter().find(|entry| {
                field(entry, "accountType")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.eq_ignore_ascii_case("epic"))
            })
        });
    let epic_id = epic
        .and_then(|e| field(e, "accountId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let username = field(&parsed, "username")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    cache.insert(key.clone(), UserInfo { username, epic_id });
    Added::Stored(key)
}

// ---- The Phoenix envelope (OnlinePhoenix::Parse 0x140aae300, envelope reader 0x140b12170) ----

#[derive(Debug, Clone)]
pub(crate) struct Envelope {
    pub ok: bool,
    pub code: Option<String>,
    pub payload: Value,
}

/// Success needs a 200-206 status AND a JSON object body: a 204 with no body is a failure. The body is
/// parsed on errors too, for its code.
pub(crate) fn parse_phoenix_envelope(status: u16, text: &str) -> Envelope {
    let root = if text.is_empty() {
        None
    } else {
        parse_json(text).filter(Value::is_object)
    };
    let get = |name: &str| root.as_ref().and_then(|r| field(r, name)).cloned();
    Envelope {
        ok: is_success(status) && root.is_some(),
        code: get("code").and_then(|c| c.as_str().map(str::to_owned)),
        payload: get("payload").unwrap_or(Value::Null),
    }
}

// ---- Guild error codes (mapper 0x140ade3f0; EGuildRequestError in the SDK) ----

pub(crate) const GUILD_ERROR_CODES: [(&str, &str); 17] = [
    ("SlyAdorableQuillshot", "InvalidPermission"),
    ("ExcludedAdorableQuillshot", "NotInAGuild"),
    ("ObedientAdorableQuillshot", "GuildNameInvalidLength"),
    ("SeizedAdorableQuillshot", "GuildNameTaken"),
    ("NastyAdorableQuillshot", "GuildNameProfane"),
    ("NumberedAdorableQuillshot", "GuildNameTooManyNumbers"),
    ("LetteredAdorableQuillshot", "GuildNameTooManyLetters"),
    ("DutifulAdorableQuillshot", "GuildNameplateInvalidLength"),
    ("CapturedAdorableQuillshot", "GuildNameplateTaken"),
    ("DirtyAdorableQuillshot", "GuildNameplateProfane"),
    ("OccupiedAdorableQuillshot", "YouAlreadyInAGuild"),
    ("ClonedAdorableQuillshot", "TargetAlreadyInYourGuild"),
    ("RedundantAdorableQuillshot", "TargetAlreadyHasGuildInvite"),
    ("StuffedAdorableQuillshot", "GuildIsFull"),
    ("UninvitedAdorableQuillshot", "GuildInviteNotFound"),
    ("ChiefAdorableQuillshot", "GuildLeaderCannotLeaveGuild"),
    ("DocileAdorableQuillshot", "InvalidGuildRank"),
];

pub(crate) fn guild_error_of(code: Option<&str>) -> &'static str {
    GUILD_ERROR_CODES
        .iter()
        .find(|(c, _)| Some(*c) == code)
        .map_or("Unknown", |(_, e)| e)
}

/// A guild request as the client sees it: success, or the EGuildRequestError it shows
pub(crate) fn guild_outcome(status: u16, text: &str) -> (bool, &'static str, Envelope) {
    let parsed = parse_phoenix_envelope(status, text);
    let error = if parsed.ok {
        "None"
    } else {
        guild_error_of(parsed.code.as_deref())
    };
    (parsed.ok, error, parsed)
}

// ---- The guild payload (FGuildData / FGuildMemberData; serializer 0x140b13de0, member 0x140b13ce0) ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuildData {
    pub id: String,
    pub name: String,
    pub nameplate: String,
    pub leader: String,
    pub members: Vec<(String, String)>,
    pub max: i64,
}

/// Every field is a string except maximum_guild_members (int32). A value of the wrong type is skipped,
/// so an id that is not a string leaves the guild without an id (the client then treats it as no guild).
pub(crate) fn parse_guild_data(payload: &Value) -> Option<GuildData> {
    if !payload.is_object() {
        return None;
    }
    let text = |value: &Value, name: &str| {
        field(value, name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let members = field(payload, "members")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|m| m.is_object())
                .map(|m| (text(m, "phx_account_id"), text(m, "rank")))
                .collect()
        })
        .unwrap_or_default();
    let max = field(payload, "maximum_guild_members")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let data = GuildData {
        id: text(payload, "id"),
        name: text(payload, "name"),
        nameplate: text(payload, "nameplate"),
        leader: text(payload, "leader_account_id"),
        members,
        max,
    };
    (!data.id.is_empty()).then_some(data)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuildInvite {
    pub id: String,
    pub guild_id: String,
    pub guild_name: String,
    pub inviter: String,
}

fn js_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

pub(crate) fn parse_guild_invites(payload: &Value) -> Option<Vec<GuildInvite>> {
    let invites = field(payload, "invites")?.as_array()?;
    Some(
        invites
            .iter()
            .filter(|i| i.is_object())
            .map(|i| GuildInvite {
                id: js_string(field(i, "id")),
                guild_id: js_string(field(i, "guild_id")),
                guild_name: js_string(field(i, "guild_name")),
                inviter: js_string(field(i, "inviter_account_id")),
            })
            .collect(),
    )
}

// ---- Friends service (OnlineFriendsMcp, 0x1408dd229..) ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FriendEntry {
    pub account_id: String,
    pub status: String,
    pub direction: String,
}

/// The client wraps the body as {"friends": <body>}: it must be a JSON array, each entry with a string
/// accountId (status and direction are strings)
pub(crate) fn parse_friends_list(text: &str) -> Option<Vec<FriendEntry>> {
    let list = parse_json(text)?;
    Some(
        list.as_array()?
            .iter()
            .filter_map(|entry| {
                Some(FriendEntry {
                    account_id: field(entry, "accountId")?.as_str()?.to_owned(),
                    status: js_string(field(entry, "status")),
                    direction: js_string(field(entry, "direction")),
                })
            })
            .collect(),
    )
}

pub(crate) fn parse_block_list(text: &str) -> Option<Vec<String>> {
    let parsed = parse_json(text)?;
    Some(
        field(&parsed, "blockedUsers")?
            .as_array()?
            .iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect(),
    )
}

// ---- GET /party/invites (0x140b675d0) ----

const INVITATION_FIELDS: [&str; 5] = [
    "recipientPlayerId",
    "sendingPlayerId",
    "partyId",
    "sendingPlatform",
    "sendingDisplayName",
];

/// "invitations": an array of objects with five strings each; an entry missing one is skipped
pub(crate) fn parse_invitations(text: &str) -> Option<Vec<Value>> {
    let parsed = parse_json(text)?;
    let list = field(&parsed, "invitations")?.as_array()?;
    Some(
        list.iter()
            .filter(|entry| {
                INVITATION_FIELDS
                    .iter()
                    .all(|name| field(entry, name).is_some_and(Value::is_string))
            })
            .map(|entry| {
                let mut out = Map::new();
                for name in INVITATION_FIELDS {
                    out.insert(name.to_owned(), field(entry, name).unwrap().clone());
                }
                Value::Object(out)
            })
            .collect(),
    )
}

// ---- One player's client ----

/// How a client reaches a server: method, path and body, to the status and the body text.
pub(crate) type Transport<'a> =
    Box<dyn Fn(String, String, Option<Value>) -> BoxFuture<'a, (u16, String)> + Send + Sync + 'a>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    Resolved(String),
    Dropped(&'static str),
}

/// Keeps what the real client keeps for a session: user info per Phoenix id, mappings per source type
pub(crate) struct SocialClient<'a> {
    pub this: String,
    pub users: HashMap<String, UserInfo>,
    pub mappings: BTreeMap<String, HashMap<String, MappedId>>,
    pub calls: Vec<String>,
    send: Transport<'a>,
}

impl<'a> SocialClient<'a> {
    pub(crate) fn new(this: &str, send: Transport<'a>) -> Self {
        Self {
            this: this.to_owned(),
            users: HashMap::new(),
            mappings: BTreeMap::new(),
            calls: Vec::new(),
            send,
        }
    }

    async fn request(&mut self, method: &str, path: &str, body: Option<Value>) -> (u16, String) {
        self.calls.push(format!("{method} {path}"));
        (self.send)(method.to_owned(), path.to_owned(), body).await
    }

    /// At login the client looks up its own account first (2.1.1 capture, request 6462)
    pub(crate) async fn login(mut self) -> Self {
        let this = self.this.clone();
        self.query_user_info(&this).await;
        self
    }

    pub(crate) async fn query_user_info(&mut self, id: &str) -> Added {
        let (status, text) = self
            .request("POST", "/accountinfo/public", Some(json!({"accountId": id})))
            .await;
        add_user_info(&mut self.users, status, &text)
    }

    pub(crate) async fn query_mappings(&mut self, ids: &[&str], source: &str) -> HashMap<String, MappedId> {
        let cache = self.mappings.entry(source.to_owned()).or_default().clone();
        let missing: Vec<&str> = ids.iter().copied().filter(|id| !cache.contains_key(*id)).collect();
        if !missing.is_empty() {
            let (status, text) = self
                .request(
                    "POST",
                    "/account/mapping",
                    Some(json!({"srcAccountType": source, "ids": missing})),
                )
                .await;
            if let Ok(mapped) = parse_account_mappings(status, &text) {
                self.mappings.get_mut(source).unwrap().extend(mapped);
            }
        }
        self.mappings[source].clone()
    }

    /// Epic id -> Phoenix id -> user info -> the toolkit's check (0x1409e3c60, 0x1409add00) that the
    /// user's Epic id is the one the action started from. "dropped" is the player seeing nothing happen.
    pub(crate) async fn resolve_from_epic(&mut self, epic_id: &str) -> Resolved {
        let mapped = self.query_mappings(&[epic_id], "epic").await.get(epic_id).cloned();
        let Some(mapped) = mapped.filter(|m| m.kind == "Phoenix") else {
            return Resolved::Dropped("Mapping primary Id for unknown, unmapped external Id");
        };
        if !self.users.contains_key(&mapped.id) {
            self.query_user_info(&mapped.id).await;
        }
        match self.users.get(&mapped.id) {
            None => Resolved::Dropped("no user info"),
            Some(info) if info.epic_id.as_deref() != Some(epic_id) => {
                Resolved::Dropped("the user's Epic id is another")
            }
            Some(_) => Resolved::Resolved(mapped.id),
        }
    }

    /// A Phoenix id (party invite sender, party member, guild member):
    /// USocialUser::TryBroadcastInitializationComplete (0x1409ede50) invalidates a user without info
    pub(crate) async fn resolve_from_phoenix(&mut self, id: &str) -> &'static str {
        if !self.users.contains_key(id) {
            self.query_user_info(id).await;
        }
        if self.users.contains_key(id) {
            "initialised"
        } else {
            "invalidated"
        }
    }

    pub(crate) fn name_of(&self, id: &str) -> Option<&str> {
        self.users.get(id).map(|info| info.username.as_str())
    }
}

/// A player's game client talking to this server with its own token
pub(crate) fn client_of<'a>(app: &'a crate::common::TestApp, who: &str) -> SocialClient<'a> {
    let user = who.to_owned();
    let send: Transport<'a> = Box::new(move |method, path, body| {
        let user = user.clone();
        Box::pin(async move {
            let mut request = crate::common::req().bearer(&user);
            if let Some(body) = body {
                request = request.json(body);
            }
            let reply = app.call(&method, &path, request).await;
            (reply.status, reply.text)
        })
    });
    SocialClient::new(who, send)
}
