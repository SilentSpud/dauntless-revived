//! Discord itself: its REST API, what the bot keeps from the gateway's guild events, and both behind
//! the traits the rest of the bot is written (and tested) against.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::{Value, json};

use crate::counter::{Channel, Guild, Kind};
use crate::keys::{BoxFuture, Failure, Found};
use crate::recovery::Interaction;

const API: &str = "https://discord.com/api/v10";

type Https = hyper_rustls::HttpsConnector<HttpConnector>;

fn client() -> &'static Client<Https, Full<Bytes>> {
    static CLIENT: OnceLock<Client<Https, Full<Bytes>>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let builder = hyper_rustls::HttpsConnectorBuilder::new();
        let builder = match builder.with_provider_and_webpki_roots(rustls::crypto::aws_lc_rs::default_provider()) {
            Ok(builder) => builder,
            Err(_) => hyper_rustls::HttpsConnectorBuilder::new().with_webpki_roots(),
        };
        let connector = builder.https_only().enable_http1().build();
        Client::builder(TokioExecutor::new()).build(connector)
    })
}

/// Discord's REST API with the bot token.
pub struct DiscordRest {
    token: String,
}

/// A failed request, with how long a rate limit asks to wait.
struct Refused {
    failure: Failure,
    retry_after: Option<f64>,
}

impl From<Failure> for Refused {
    fn from(failure: Failure) -> Self {
        Self {
            failure,
            retry_after: None,
        }
    }
}

impl DiscordRest {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
        }
    }

    /// One request; a rate limit is waited out, up to three times, as discord.js waits them out.
    pub async fn call(&self, method: &str, route: &str, body: Option<&Value>) -> Result<Value, Failure> {
        let mut attempt = 0;
        loop {
            match self.exchange(method, route, body).await {
                Err(refused) if refused.failure.status == Some(429) && attempt < 3 => {
                    attempt += 1;
                    let wait = refused.retry_after.unwrap_or(1.0).clamp(0.0, 600.0);
                    tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                }
                other => return other.map_err(|refused| refused.failure),
            }
        }
    }

    async fn exchange(&self, method: &str, route: &str, body: Option<&Value>) -> Result<Value, Refused> {
        let mut request = http::Request::builder()
            .method(method)
            .uri(format!("{API}{route}"))
            .header("authorization", format!("Bot {}", self.token))
            .header(
                "user-agent",
                "DiscordBot (https://github.com/mixutin/dauntless-revived, 0.2)",
            );
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let request = request
            .body(Full::new(Bytes::from(body.map(Value::to_string).unwrap_or_default())))
            .map_err(|e| Failure::new(e.to_string()))?;
        let exchange = async {
            let response = client()
                .request(request)
                .await
                .map_err(|e| Failure::new(e.to_string()))?;
            let status = response.status().as_u16();
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| Failure::new(e.to_string()))?
                .to_bytes();
            if !(200..300).contains(&status) {
                let answer = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
                return Err(Refused {
                    failure: Failure {
                        message: format!("Discord answered {status}"),
                        code: answer["code"].as_i64(),
                        status: Some(status),
                    },
                    retry_after: answer["retry_after"].as_f64(),
                });
            }
            Ok(if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).map_err(|e| Failure::new(e.to_string()))?
            })
        };
        tokio::time::timeout(Duration::from_secs(30), exchange)
            .await
            .unwrap_or_else(|_| Err(Failure::new("The operation was aborted due to timeout").into()))
    }
}

impl crate::commands::Rest for DiscordRest {
    fn get(&self, route: &str) -> BoxFuture<'_, Result<Value, Failure>> {
        let route = route.to_owned();
        Box::pin(async move { self.call("GET", &route, None).await })
    }

    fn post(&self, route: &str, body: &Value) -> BoxFuture<'_, Result<Value, Failure>> {
        let (route, body) = (route.to_owned(), body.clone());
        Box::pin(async move { self.call("POST", &route, Some(&body)).await })
    }

    fn patch(&self, route: &str, body: &Value) -> BoxFuture<'_, Result<Value, Failure>> {
        let (route, body) = (route.to_owned(), body.clone());
        Box::pin(async move { self.call("PATCH", &route, Some(&body)).await })
    }

    fn delete(&self, route: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let route = route.to_owned();
        Box::pin(async move { self.call("DELETE", &route, None).await.map(|_| ()) })
    }
}

const ADMINISTRATOR: u64 = 1 << 3;
const MANAGE_CHANNELS: u64 = 1 << 4;
const VIEW_CHANNEL: u64 = 1 << 10;
const CONNECT: u64 = 1 << 20;
const SPEAK: u64 = 1 << 21;

const GUILD_VOICE: u64 = 2;
const GUILD_CATEGORY: u64 = 4;

/// Permission bits, which Discord sends as a decimal string.
fn bits(value: &Value) -> u64 {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
        .unwrap_or(0)
}

fn id_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// What the bot keeps of a guild from its gateway events.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuildInfo {
    pub owner_id: String,
    pub member_count: u64,
    /// Role id -> its permission bits
    pub roles: HashMap<String, u64>,
}

/// The guilds the gateway has delivered, and the bot's own user id.
#[derive(Default)]
pub struct Cache {
    guilds: Mutex<HashMap<String, GuildInfo>>,
    user: Mutex<String>,
}

fn roles_of(list: &Value) -> HashMap<String, u64> {
    list.as_array()
        .into_iter()
        .flatten()
        .map(|role| (id_of(&role["id"]), bits(&role["permissions"])))
        .collect()
}

impl Cache {
    /// Takes in one dispatch.
    pub fn update(&self, event: &str, data: &Value) {
        let mut guilds = self.guilds.lock().unwrap_or_else(PoisonError::into_inner);
        let guild = id_of(&data["guild_id"]);
        match event {
            "READY" => *self.user.lock().unwrap_or_else(PoisonError::into_inner) = id_of(&data["user"]["id"]),
            "GUILD_CREATE" if data["unavailable"] != true => {
                guilds.insert(
                    id_of(&data["id"]),
                    GuildInfo {
                        owner_id: id_of(&data["owner_id"]),
                        member_count: data["member_count"].as_u64().unwrap_or(0),
                        roles: roles_of(&data["roles"]),
                    },
                );
            }
            "GUILD_UPDATE" => {
                if let Some(info) = guilds.get_mut(&id_of(&data["id"])) {
                    info.owner_id = id_of(&data["owner_id"]);
                    info.roles = roles_of(&data["roles"]);
                }
            }
            "GUILD_DELETE" => {
                guilds.remove(&id_of(&data["id"]));
            }
            "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
                if let Some(info) = guilds.get_mut(&guild) {
                    info.roles
                        .insert(id_of(&data["role"]["id"]), bits(&data["role"]["permissions"]));
                }
            }
            "GUILD_ROLE_DELETE" => {
                if let Some(info) = guilds.get_mut(&guild) {
                    info.roles.remove(&id_of(&data["role_id"]));
                }
            }
            "GUILD_MEMBER_ADD" => {
                if let Some(info) = guilds.get_mut(&guild) {
                    info.member_count += 1;
                }
            }
            "GUILD_MEMBER_REMOVE" => {
                if let Some(info) = guilds.get_mut(&guild) {
                    info.member_count = info.member_count.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    pub fn guild(&self, id: &str) -> Option<GuildInfo> {
        self.guilds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    pub fn user(&self) -> String {
        self.user.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// A member's permissions in a channel, worked out as discord.js's `permissionsFor` does: the owner and
/// administrators have all; otherwise the roles' permissions, then the channel's overwrites for
/// @everyone, for the member's roles together, and for the member.
pub fn permissions_in(guild_id: &str, guild: &GuildInfo, member: &Value, overwrites: &Value) -> u64 {
    let user = id_of(&member["user"]["id"]);
    if user == guild.owner_id {
        return u64::MAX;
    }
    let member_roles: Vec<String> = member["roles"].as_array().into_iter().flatten().map(id_of).collect();
    let mut permissions = guild.roles.get(guild_id).copied().unwrap_or(0);
    for role in &member_roles {
        permissions |= guild.roles.get(role).copied().unwrap_or(0);
    }
    if permissions & ADMINISTRATOR != 0 {
        return u64::MAX;
    }
    let overwrites: Vec<&Value> = overwrites.as_array().into_iter().flatten().collect();
    let apply = |permissions: u64, allow: u64, deny: u64| (permissions & !deny) | allow;
    if let Some(everyone) = overwrites.iter().find(|o| id_of(&o["id"]) == guild_id) {
        permissions = apply(permissions, bits(&everyone["allow"]), bits(&everyone["deny"]));
    }
    let (mut allow, mut deny) = (0, 0);
    for overwrite in overwrites
        .iter()
        .filter(|o| o["type"] == 0 && member_roles.contains(&id_of(&o["id"])))
    {
        allow |= bits(&overwrite["allow"]);
        deny |= bits(&overwrite["deny"]);
    }
    permissions = apply(permissions, allow, deny);
    if let Some(own) = overwrites.iter().find(|o| o["type"] == 1 && id_of(&o["id"]) == user) {
        permissions = apply(permissions, bits(&own["allow"]), bits(&own["deny"]));
    }
    permissions
}

fn no_mentions() -> Value {
    json!({ "parse": [] })
}

/// The user who ran a command: `member.user` in a guild, `user` in a DM.
pub fn command_user(command: &Value) -> String {
    let user = if command["member"]["user"].is_object() {
        &command["member"]["user"]
    } else {
        &command["user"]
    };
    id_of(&user["id"])
}

/// Answers an interaction.
pub async fn respond(rest: &DiscordRest, command: &Value, response: Value) -> Result<(), Failure> {
    let route = format!(
        "/interactions/{}/{}/callback",
        id_of(&command["id"]),
        id_of(&command["token"])
    );
    rest.call("POST", &route, Some(&response)).await.map(|_| ())
}

/// "Thinking…", seen only by the user.
pub async fn defer_ephemeral(rest: &DiscordRest, command: &Value) -> Result<(), Failure> {
    respond(rest, command, json!({ "type": 5, "data": { "flags": 64 } })).await
}

/// Replaces the interaction's reply.
pub async fn edit_response(rest: &DiscordRest, command: &Value, body: Value) -> Result<(), Failure> {
    let route = format!(
        "/webhooks/{}/{}/messages/@original",
        id_of(&command["application_id"]),
        id_of(&command["token"])
    );
    rest.call("PATCH", &route, Some(&body)).await.map(|_| ())
}

async fn dm_channel(rest: &DiscordRest, user: &str) -> Result<String, Failure> {
    let channel = rest
        .call("POST", "/users/@me/channels", Some(&json!({ "recipient_id": user })))
        .await?;
    Ok(id_of(&channel["id"]))
}

/// A DM to `user`; the message id.
pub async fn direct_message(rest: &DiscordRest, user: &str, body: Value) -> Result<String, Failure> {
    let channel = dm_channel(rest, user).await?;
    let message = rest
        .call("POST", &format!("/channels/{channel}/messages"), Some(&body))
        .await?;
    Ok(id_of(&message["id"]))
}

/// A `/key` interaction (deferred ephemerally before it reaches the flows).
pub struct KeyInteraction {
    pub rest: Arc<DiscordRest>,
    pub command: Value,
    pub ephemeral: bool,
}

impl Interaction for KeyInteraction {
    fn ephemeral(&self) -> bool {
        self.ephemeral
    }

    fn user_id(&self) -> String {
        command_user(&self.command)
    }

    fn option(&self, name: &str) -> Option<String> {
        let sub = &self.command["data"]["options"][0];
        if sub["type"] != 1 {
            return None;
        }
        sub["options"]
            .as_array()?
            .iter()
            .find(|o| o["name"] == name)
            .and_then(|o| o["value"].as_str())
            .map(str::to_owned)
    }

    fn edit_reply(&self, content: String) -> BoxFuture<'_, Result<(), Failure>> {
        Box::pin(async move {
            edit_response(
                &self.rest,
                &self.command,
                json!({ "content": content, "allowed_mentions": no_mentions() }),
            )
            .await
        })
    }

    fn send_dm(&self, content: String) -> BoxFuture<'_, Result<Option<String>, Failure>> {
        Box::pin(async move {
            let body = json!({ "content": content, "allowed_mentions": no_mentions() });
            direct_message(&self.rest, &self.user_id(), body).await.map(Some)
        })
    }
}

/// A message time as `Date.toISOString()` writes it.
fn iso_time(timestamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|t| {
            t.with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        })
        .unwrap_or_default()
}

/// Only this bot's exact invite counts, and an incomplete history is never proof of absence
/// (`dm-history.mjs`): `None` after 20 pages.
pub async fn find_invite_message(
    rest: &DiscordRest,
    user: &str,
    bot: &str,
    code: &str,
) -> Result<Option<Option<Found>>, Failure> {
    let channel = dm_channel(rest, user).await?;
    let mut before: Option<String> = None;
    for _ in 0..20 {
        let mut route = format!("/channels/{channel}/messages?limit=100");
        if let Some(before) = &before {
            route.push_str(&format!("&before={before}"));
        }
        let page = rest.call("GET", &route, None).await?;
        let messages = page.as_array().cloned().unwrap_or_default();
        let found = messages.iter().find(|m| {
            let content = m["content"].as_str().unwrap_or_default();
            id_of(&m["author"]["id"]) == bot
                && (content.contains(&format!("code={code}")) || content.contains(&format!("\n{code}\n")))
        });
        if let Some(message) = found {
            return Ok(Some(Some(Found {
                id: id_of(&message["id"]),
                created_at: iso_time(message["timestamp"].as_str().unwrap_or_default()),
            })));
        }
        if messages.len() < 100 {
            return Ok(Some(None));
        }
        before = messages.last().map(|m| id_of(&m["id"]));
    }
    Ok(None)
}

/// A guild, for the counter.
pub struct DiscordGuild {
    pub rest: Arc<DiscordRest>,
    pub cache: Arc<Cache>,
    pub id: String,
    pub bot: String,
}

fn channel_of(channel: &Value, guild: &str, bot: &str) -> Channel {
    let overwrites: Vec<&Value> = channel["permission_overwrites"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let voice = CONNECT | SPEAK;
    let locked = overwrites
        .iter()
        .any(|o| o["type"] == 0 && id_of(&o["id"]) == guild && bits(&o["deny"]) & voice == voice);
    let wanted = VIEW_CHANNEL | MANAGE_CHANNELS;
    let managed = overwrites
        .iter()
        .any(|o| o["type"] == 1 && id_of(&o["id"]) == bot && bits(&o["allow"]) & wanted == wanted);
    Channel {
        id: id_of(&channel["id"]),
        name: channel["name"].as_str().unwrap_or_default().to_owned(),
        kind: match channel["type"].as_u64() {
            Some(GUILD_CATEGORY) => Kind::Category,
            Some(GUILD_VOICE) => Kind::Voice,
            _ => Kind::Other,
        },
        parent_id: channel["parent_id"].as_str().map(str::to_owned),
        guild_id: id_of(&channel["guild_id"]),
        locked,
        managed,
    }
}

fn parse_channel(id: &str) -> Result<u64, Failure> {
    id.parse::<u64>()
        .ok()
        .filter(|n| *n != 0)
        .ok_or_else(|| Failure::new("Not a channel id"))
}

impl DiscordGuild {
    /// A guild channel; `None` for a DM or group channel.
    async fn fetch_channel(&self, id: u64) -> Result<Option<Value>, Failure> {
        let channel = self.rest.call("GET", &format!("/channels/{id}"), None).await?;
        Ok(channel["guild_id"].is_string().then_some(channel))
    }

    fn everyone_locked(&self) -> Value {
        json!({ "id": self.id, "type": 0, "allow": "0", "deny": (CONNECT | SPEAK).to_string() })
    }

    fn bot_allowed(&self) -> Value {
        json!({ "id": self.bot, "type": 1, "allow": (VIEW_CHANNEL | MANAGE_CHANNELS).to_string(), "deny": "0" })
    }

    async fn overwrite(&self, channel: &str, overwrite: Value) -> Result<(), Failure> {
        let route = format!(
            "/channels/{}/permissions/{}",
            parse_channel(channel)?,
            id_of(&overwrite["id"])
        );
        self.rest.call("PUT", &route, Some(&overwrite)).await.map(|_| ())
    }
}

impl Guild for DiscordGuild {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn member_count(&self) -> u64 {
        self.cache.guild(&self.id).map_or(0, |g| g.member_count)
    }

    fn counts(&self, _fetch: bool) -> BoxFuture<'_, Result<(u64, u64), Failure>> {
        Box::pin(async move {
            let (mut bots, mut users) = (0, 0);
            let mut after: Option<String> = None;
            loop {
                let mut route = format!("/guilds/{}/members?limit=1000", self.id);
                if let Some(after) = &after {
                    route.push_str(&format!("&after={after}"));
                }
                let page = self.rest.call("GET", &route, None).await?;
                let page = page.as_array().cloned().unwrap_or_default();
                for member in &page {
                    if member["user"]["bot"] == true {
                        bots += 1;
                    } else {
                        users += 1;
                    }
                }
                if page.len() < 1000 {
                    return Ok((bots, users));
                }
                after = page.last().map(|m| id_of(&m["user"]["id"]));
            }
        })
    }

    fn channel(&self, id: &str) -> BoxFuture<'_, Result<Option<Channel>, Failure>> {
        let id = id.to_owned();
        Box::pin(async move {
            let Ok(channel_id) = parse_channel(&id) else {
                return Ok(None);
            };
            match self.fetch_channel(channel_id).await {
                Ok(channel) => Ok(channel.map(|c| channel_of(&c, &self.id, &self.bot))),
                Err(failure) if failure.status == Some(404) => Ok(None),
                Err(failure) => Err(failure),
            }
        })
    }

    fn channels(&self) -> BoxFuture<'_, Result<Vec<Channel>, Failure>> {
        Box::pin(async move {
            let channels = self
                .rest
                .call("GET", &format!("/guilds/{}/channels", self.id), None)
                .await?;
            Ok(channels
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| channel_of(c, &self.id, &self.bot))
                .collect())
        })
    }

    fn can_manage(&self, category: &Channel) -> BoxFuture<'_, Result<bool, Failure>> {
        let id = category.id.clone();
        Box::pin(async move {
            let Some(channel) = self.fetch_channel(parse_channel(&id)?).await? else {
                return Ok(false);
            };
            let member = self
                .rest
                .call("GET", &format!("/guilds/{}/members/{}", self.id, self.bot), None)
                .await?;
            let Some(guild) = self.cache.guild(&self.id) else {
                return Ok(false);
            };
            let permissions = permissions_in(&self.id, &guild, &member, &channel["permission_overwrites"]);
            Ok(permissions & MANAGE_CHANNELS != 0)
        })
    }

    fn rename(&self, id: &str, name: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let (id, name) = (id.to_owned(), name.to_owned());
        Box::pin(async move {
            let route = format!("/channels/{}", parse_channel(&id)?);
            self.rest
                .call("PATCH", &route, Some(&json!({ "name": name })))
                .await
                .map(|_| ())
        })
    }

    fn create_voice(&self, name: &str, parent: &str) -> BoxFuture<'_, Result<Channel, Failure>> {
        let (name, parent) = (name.to_owned(), parent.to_owned());
        Box::pin(async move {
            let body = json!({
                "name": name,
                "type": GUILD_VOICE,
                "parent_id": parse_channel(&parent)?.to_string(),
                "permission_overwrites": [self.everyone_locked(), self.bot_allowed()],
            });
            let channel = self
                .rest
                .call("POST", &format!("/guilds/{}/channels", self.id), Some(&body))
                .await?;
            Ok(channel_of(&channel, &self.id, &self.bot))
        })
    }

    fn lock(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let id = id.to_owned();
        Box::pin(async move { self.overwrite(&id, self.everyone_locked()).await })
    }

    fn allow_self(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let id = id.to_owned();
        Box::pin(async move { self.overwrite(&id, self.bot_allowed()).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild() -> GuildInfo {
        GuildInfo {
            owner_id: "1".into(),
            member_count: 3,
            roles: HashMap::from([("10".to_owned(), VIEW_CHANNEL), ("20".to_owned(), MANAGE_CHANNELS)]),
        }
    }

    fn member(id: &str, roles: &[&str]) -> Value {
        json!({ "user": { "id": id }, "roles": roles })
    }

    #[test]
    fn permissions_follow_roles_then_overwrites() {
        let none = json!([]);
        assert_eq!(
            permissions_in("10", &guild(), &member("1", &[]), &none),
            u64::MAX,
            "owner"
        );
        assert_eq!(permissions_in("10", &guild(), &member("2", &[]), &none), VIEW_CHANNEL);
        assert_ne!(
            permissions_in("10", &guild(), &member("2", &["20"]), &none) & MANAGE_CHANNELS,
            0
        );
        // Denied for the role, allowed again for the member
        let role_denied = json!([{ "id": "20", "type": 0, "allow": "0", "deny": MANAGE_CHANNELS.to_string() }]);
        assert_eq!(
            permissions_in("10", &guild(), &member("2", &["20"]), &role_denied) & MANAGE_CHANNELS,
            0
        );
        let member_allowed = json!([
            { "id": "20", "type": 0, "allow": "0", "deny": MANAGE_CHANNELS.to_string() },
            { "id": "2", "type": 1, "allow": MANAGE_CHANNELS.to_string(), "deny": "0" },
        ]);
        assert_ne!(
            permissions_in("10", &guild(), &member("2", &["20"]), &member_allowed) & MANAGE_CHANNELS,
            0
        );
        // Administrators ignore overwrites
        let mut admin = guild();
        admin.roles.insert("30".into(), ADMINISTRATOR);
        assert_eq!(
            permissions_in("10", &admin, &member("2", &["30"]), &role_denied),
            u64::MAX
        );
    }

    #[test]
    fn the_cache_follows_guild_events() {
        let cache = Cache::default();
        cache.update("READY", &json!({ "user": { "id": "99" } }));
        cache.update("GUILD_CREATE", &json!({ "id": "5", "unavailable": true }));
        assert_eq!(cache.guild("5"), None);
        cache.update(
            "GUILD_CREATE",
            &json!({ "id": "5", "owner_id": "1", "member_count": 2, "roles": [{ "id": "5", "permissions": "1024" }] }),
        );
        cache.update("GUILD_MEMBER_ADD", &json!({ "guild_id": "5" }));
        cache.update(
            "GUILD_ROLE_CREATE",
            &json!({ "guild_id": "5", "role": { "id": "7", "permissions": "16" } }),
        );
        let info = cache.guild("5").unwrap_or_default();
        assert_eq!((info.member_count, info.roles.get("7").copied()), (3, Some(16)));
        cache.update("GUILD_MEMBER_REMOVE", &json!({ "guild_id": "5" }));
        cache.update("GUILD_ROLE_DELETE", &json!({ "guild_id": "5", "role_id": "7" }));
        let info = cache.guild("5").unwrap_or_default();
        assert_eq!((info.member_count, info.roles.len()), (2, 1));
        cache.update("GUILD_DELETE", &json!({ "id": "5" }));
        assert_eq!((cache.guild("5"), cache.user()), (None, "99".to_owned()));
    }

    #[test]
    fn channels_options_and_times_from_payloads() {
        let channel = json!({
            "id": "3", "name": "Users: 4", "type": 2, "parent_id": "2", "guild_id": "1",
            "permission_overwrites": [
                { "id": "1", "type": 0, "allow": "0", "deny": (CONNECT | SPEAK).to_string() },
                { "id": "9", "type": 1, "allow": (VIEW_CHANNEL | MANAGE_CHANNELS).to_string(), "deny": "0" },
            ],
        });
        let seen = channel_of(&channel, "1", "9");
        assert!(seen.locked && seen.managed && seen.kind == Kind::Voice);
        assert_eq!(seen.parent_id.as_deref(), Some("2"));
        assert!(!channel_of(&channel, "1", "8").managed);

        let interaction = KeyInteraction {
            rest: Arc::new(DiscordRest::new("t")),
            command: json!({
                "member": { "user": { "id": "42" } },
                "data": { "options": [{ "name": "link", "type": 1, "options": [{ "name": "key", "type": 3, "value": "UUK" }] }] },
            }),
            ephemeral: true,
        };
        assert_eq!(interaction.user_id(), "42");
        assert_eq!(interaction.option("key").as_deref(), Some("UUK"));
        assert_eq!(interaction.option("other"), None);
        assert_eq!(command_user(&json!({ "user": { "id": "7" } })), "7");
        assert_eq!(iso_time("2024-05-01T10:20:30.123000+00:00"), "2024-05-01T10:20:30.123Z");
    }
}
