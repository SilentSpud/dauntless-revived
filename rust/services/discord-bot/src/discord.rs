//! Discord itself: the REST calls for command registration, and serenity's objects behind the
//! traits the rest of the bot is written (and tested) against.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;
use serenity::all::{
    ChannelId, ChannelType, CommandInteraction, CreateAllowedMentions, CreateChannel, CreateMessage, EditChannel,
    EditInteractionResponse, GetMessages, GuildChannel, GuildId, Http, MessageId, PermissionOverwrite,
    PermissionOverwriteType, Permissions, RoleId, UserId,
};

use crate::counter::{Channel, Guild, Kind};
use crate::keys::{BoxFuture, Failure, Found};
use crate::recovery::Interaction;

const API: &str = "https://discord.com/api/v10";

type Https = hyper_rustls::HttpsConnector<HttpConnector>;

fn client() -> &'static Client<Https, Full<Bytes>> {
    static CLIENT: OnceLock<Client<Https, Full<Bytes>>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        // aws-lc-rs by name: with another provider compiled in (serenity brings ring), rustls has no
        // default to fall back on
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

impl DiscordRest {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
        }
    }

    async fn call(&self, method: &str, route: &str, body: Option<&Value>) -> Result<Value, Failure> {
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
                return Err(Failure {
                    message: format!("Discord answered {status}"),
                    code: serde_json::from_slice::<Value>(&bytes)
                        .ok()
                        .and_then(|v| v["code"].as_i64()),
                    status: Some(status),
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
            .unwrap_or_else(|_| Err(Failure::new("The operation was aborted due to timeout")))
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

/// A serenity error as the bot reasons about it: Discord's JSON error code when there is one.
pub fn failure(error: serenity::Error) -> Failure {
    if let serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response)) = &error {
        return Failure {
            message: response.error.message.clone(),
            code: Some(response.error.code as i64),
            status: Some(response.status_code.as_u16()),
        };
    }
    Failure::new(error.to_string())
}

fn no_mentions() -> CreateAllowedMentions {
    CreateAllowedMentions::new().empty_users().empty_roles().everyone(false)
}

/// A `/key` interaction (deferred ephemerally before it reaches the flows).
pub struct KeyInteraction {
    pub http: Arc<Http>,
    pub command: CommandInteraction,
    pub ephemeral: bool,
}

impl Interaction for KeyInteraction {
    fn ephemeral(&self) -> bool {
        self.ephemeral
    }

    fn user_id(&self) -> String {
        self.command.user.id.to_string()
    }

    fn option(&self, name: &str) -> Option<String> {
        self.command.data.options().into_iter().find_map(|o| match o.value {
            serenity::all::ResolvedValue::SubCommand(options) => {
                options
                    .into_iter()
                    .find(|o| o.name == name)
                    .and_then(|o| match o.value {
                        serenity::all::ResolvedValue::String(s) => Some(s.to_owned()),
                        _ => None,
                    })
            }
            _ => None,
        })
    }

    fn edit_reply(&self, content: String) -> BoxFuture<'_, Result<(), Failure>> {
        Box::pin(async move {
            self.command
                .edit_response(
                    &self.http,
                    EditInteractionResponse::new()
                        .content(content)
                        .allowed_mentions(no_mentions()),
                )
                .await
                .map(|_| ())
                .map_err(failure)
        })
    }

    fn send_dm(&self, content: String) -> BoxFuture<'_, Result<Option<String>, Failure>> {
        Box::pin(async move {
            self.command
                .user
                .direct_message(
                    &self.http,
                    CreateMessage::new().content(content).allowed_mentions(no_mentions()),
                )
                .await
                .map(|message| Some(message.id.to_string()))
                .map_err(failure)
        })
    }
}

/// Only this bot's exact invite counts, and an incomplete history is never proof of absence
/// (`dm-history.mjs`): `None` after 20 pages.
pub async fn find_invite_message(
    http: &Http,
    user: UserId,
    bot: UserId,
    code: &str,
) -> Result<Option<Option<Found>>, Failure> {
    let channel = user.create_dm_channel(http).await.map_err(failure)?;
    let mut before: Option<MessageId> = None;
    for _ in 0..20 {
        let mut request = GetMessages::new().limit(100);
        if let Some(before) = before {
            request = request.before(before);
        }
        let messages = channel.id.messages(http, request).await.map_err(failure)?;
        let found = messages.iter().find(|m| {
            m.author.id == bot
                && (m.content.contains(&format!("code={code}")) || m.content.contains(&format!("\n{code}\n")))
        });
        if let Some(message) = found {
            return Ok(Some(Some(Found {
                id: message.id.to_string(),
                created_at: message.timestamp.to_rfc3339().unwrap_or_default(),
            })));
        }
        if messages.len() < 100 {
            return Ok(Some(None));
        }
        before = messages.last().map(|m| m.id);
    }
    Ok(None)
}

/// A guild, for the counter.
pub struct DiscordGuild {
    pub http: Arc<Http>,
    pub cache: Arc<serenity::cache::Cache>,
    pub id: GuildId,
    pub bot: UserId,
}

fn channel_of(channel: &GuildChannel, guild: GuildId, bot: UserId) -> Channel {
    let everyone = RoleId::new(guild.get());
    let locked = channel.permission_overwrites.iter().any(|o| {
        o.kind == PermissionOverwriteType::Role(everyone) && o.deny.contains(Permissions::CONNECT | Permissions::SPEAK)
    });
    let managed = channel.permission_overwrites.iter().any(|o| {
        o.kind == PermissionOverwriteType::Member(bot)
            && o.allow
                .contains(Permissions::VIEW_CHANNEL | Permissions::MANAGE_CHANNELS)
    });
    Channel {
        id: channel.id.to_string(),
        name: channel.name.clone(),
        kind: match channel.kind {
            ChannelType::Category => Kind::Category,
            ChannelType::Voice => Kind::Voice,
            _ => Kind::Other,
        },
        parent_id: channel.parent_id.map(|p| p.to_string()),
        guild_id: channel.guild_id.to_string(),
        locked,
        managed,
    }
}

fn parse_channel(id: &str) -> Result<ChannelId, Failure> {
    id.parse::<u64>()
        .ok()
        .filter(|n| *n != 0)
        .map(ChannelId::new)
        .ok_or_else(|| Failure::new("Not a channel id"))
}

impl Guild for DiscordGuild {
    fn id(&self) -> String {
        self.id.to_string()
    }

    fn member_count(&self) -> u64 {
        self.cache.guild(self.id).map_or(0, |g| g.member_count)
    }

    fn counts(&self, _fetch: bool) -> BoxFuture<'_, Result<(u64, u64), Failure>> {
        Box::pin(async move {
            let (mut bots, mut users) = (0, 0);
            let mut after = None;
            loop {
                let page = self.id.members(&self.http, Some(1000), after).await.map_err(failure)?;
                for member in &page {
                    if member.user.bot {
                        bots += 1;
                    } else {
                        users += 1;
                    }
                }
                if page.len() < 1000 {
                    return Ok((bots, users));
                }
                after = page.last().map(|m| m.user.id);
            }
        })
    }

    fn channel(&self, id: &str) -> BoxFuture<'_, Result<Option<Channel>, Failure>> {
        let id = id.to_owned();
        Box::pin(async move {
            let Ok(channel_id) = parse_channel(&id) else {
                return Ok(None);
            };
            match channel_id.to_channel(&self.http).await {
                Ok(serenity::all::Channel::Guild(channel)) => Ok(Some(channel_of(&channel, self.id, self.bot))),
                Ok(_) => Ok(None),
                Err(error) => {
                    let failure = failure(error);
                    if failure.status == Some(404) {
                        Ok(None)
                    } else {
                        Err(failure)
                    }
                }
            }
        })
    }

    fn channels(&self) -> BoxFuture<'_, Result<Vec<Channel>, Failure>> {
        Box::pin(async move {
            let channels = self.id.channels(&self.http).await.map_err(failure)?;
            Ok(channels.values().map(|c| channel_of(c, self.id, self.bot)).collect())
        })
    }

    fn can_manage(&self, category: &Channel) -> BoxFuture<'_, Result<bool, Failure>> {
        let id = category.id.clone();
        Box::pin(async move {
            let channel = match parse_channel(&id)?.to_channel(&self.http).await.map_err(failure)? {
                serenity::all::Channel::Guild(channel) => channel,
                _ => return Ok(false),
            };
            let member = self.id.member(&self.http, self.bot).await.map_err(failure)?;
            let Some(guild) = self.cache.guild(self.id).map(|g| g.clone()) else {
                return Ok(false);
            };
            Ok(guild.user_permissions_in(&channel, &member).manage_channels())
        })
    }

    fn rename(&self, id: &str, name: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let (id, name) = (id.to_owned(), name.to_owned());
        Box::pin(async move {
            parse_channel(&id)?
                .edit(&self.http, EditChannel::new().name(name))
                .await
                .map(|_| ())
                .map_err(failure)
        })
    }

    fn create_voice(&self, name: &str, parent: &str) -> BoxFuture<'_, Result<Channel, Failure>> {
        let (name, parent) = (name.to_owned(), parent.to_owned());
        Box::pin(async move {
            let builder = CreateChannel::new(name)
                .kind(ChannelType::Voice)
                .category(parse_channel(&parent)?)
                .permissions(vec![
                    PermissionOverwrite {
                        allow: Permissions::empty(),
                        deny: Permissions::CONNECT | Permissions::SPEAK,
                        kind: PermissionOverwriteType::Role(RoleId::new(self.id.get())),
                    },
                    PermissionOverwrite {
                        allow: Permissions::VIEW_CHANNEL | Permissions::MANAGE_CHANNELS,
                        deny: Permissions::empty(),
                        kind: PermissionOverwriteType::Member(self.bot),
                    },
                ]);
            let channel = self.id.create_channel(&self.http, builder).await.map_err(failure)?;
            Ok(channel_of(&channel, self.id, self.bot))
        })
    }

    fn lock(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let id = id.to_owned();
        Box::pin(async move {
            let overwrite = PermissionOverwrite {
                allow: Permissions::empty(),
                deny: Permissions::CONNECT | Permissions::SPEAK,
                kind: PermissionOverwriteType::Role(RoleId::new(self.id.get())),
            };
            parse_channel(&id)?
                .create_permission(&self.http, overwrite)
                .await
                .map_err(failure)
        })
    }

    fn allow_self(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let id = id.to_owned();
        Box::pin(async move {
            let overwrite = PermissionOverwrite {
                allow: Permissions::VIEW_CHANNEL | Permissions::MANAGE_CHANNELS,
                deny: Permissions::empty(),
                kind: PermissionOverwriteType::Member(self.bot),
            };
            parse_channel(&id)?
                .create_permission(&self.http, overwrite)
                .await
                .map_err(failure)
        })
    }
}
