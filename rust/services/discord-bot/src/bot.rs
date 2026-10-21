//! The running bot (`bot.mjs`): registration once connected, the `/key` and `/counter` handlers,
//! the counter's refreshes, and a health check that ends the process (for its supervisor to start
//! again) when the gateway has been gone for two minutes. Key state is never touched by a restart.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::commands::{Rest, key_command, sync_counter_command, sync_key_commands};
use crate::counter::Counters;
use crate::discord::{
    Cache, DiscordGuild, DiscordRest, KeyInteraction, command_user, defer_ephemeral, direct_message, edit_response,
    find_invite_message, respond,
};
use crate::gateway;
use crate::invite::invite_message;
use crate::keys::{Failure, Keys, Outcome};
use crate::link_input::{KEY_INSTRUCTIONS, LinkInput, link_input};
use crate::metagame::{Http as MetagameHttp, recovery_request};
use crate::recovery::{Interaction, recover, resend};

pub struct Settings {
    pub guild: Option<String>,
    pub metagame_url: String,
    pub recovery_secret: Option<String>,
}

pub struct Bot {
    pub keys: Arc<Keys>,
    pub counters: Arc<Counters>,
    pub invite_config: Value,
    pub settings: Settings,
    pub rest: Arc<DiscordRest>,
    pub member_intent: bool,
    pub last_ready: Arc<AtomicI64>,
    pub cache: Arc<Cache>,
    active: Mutex<HashSet<String>>,
    cooldown: Mutex<HashMap<String, i64>>,
    transport: MetagameHttp,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// The reply for each outcome.
pub fn message(status: &str) -> String {
    match status {
        "sent" => "🔑 **Invite Sent**\nCheck your DMs. Paste the complete invite into the launcher’s Join box.\n**Clear skies, Slayer.**".into(),
        "already_sent" => "🔑 **Already Claimed**\nUse `/key resend` to receive another private copy of your existing unused invite. No additional invite will be issued.".into(),
        "dm_disabled" => "Enable direct messages, then run `/key claim` again. Your existing code is saved.".into(),
        "no_mutual_guild" => "Join the Revived Discord server first, enable direct messages from server members, then run `/key claim` again. Discord cannot DM you without a shared server. Your existing invite is saved.".into(),
        "delivery_uncertain" => "Discord did not confirm delivery. Check your DMs, then contact the server team if missing. No replacement key will be issued.".into(),
        "linked" => "🔑 **Key Accepted**\nYour Discord has been linked to your existing Revived account. Keep using the same launcher key—nothing to replace.\n**Clear skies, Slayer.**".into(),
        "invite_not_key" => format!("🔑 **Use Your Saved Account Key**\nThat is a Join invite or registration code. It cannot identify your existing account.\n\n{KEY_INSTRUCTIONS}\n\nIf you have not registered yet, paste the complete invite into the launcher’s Join screen first."),
        "invalid_key_format" => format!("🔑 **Copy Your Saved Account Key**\n{KEY_INSTRUCTIONS}"),
        "invalid_key" => format!("🔑 **Account Key Not Recognized**\nThis server did not recognize that account key.\n\n{KEY_INSTRUCTIONS}\n\nIf the saved key still fails, contact the server team with your launcher username and this message, not your key."),
        "discord_already_linked" => "Your Discord is already linked to another account. Contact the server team to change it.".into(),
        "account_already_linked" => "This account is already linked to another Discord. Contact the server team to change it.".into(),
        "ready" => "🔑 **Key Ready**\nYour code has not been redeemed. Run `/key claim` to receive your existing invite by DM.".into(),
        "redeemed" => "🔑 **Key Accepted**\nYour Revived code has been redeemed successfully.\n**Clear skies, Slayer.**".into(),
        "none" => "🔑 **No Key Yet**\nRun `/key claim` to receive a saved copy of your invite by DM.".into(),
        "pending" => "Your code is being prepared. Try `/key claim` again shortly.".into(),
        "revoked" => "Your code is unavailable. Contact the server team.".into(),
        _ => "The key service is unavailable. Please try again shortly.".into(),
    }
}

fn subcommand(command: &Value) -> String {
    command["data"]["options"][0]["name"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

impl Bot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        keys: Arc<Keys>,
        counters: Arc<Counters>,
        invite_config: Value,
        settings: Settings,
        rest: Arc<DiscordRest>,
        member_intent: bool,
    ) -> Self {
        Self {
            keys,
            counters,
            invite_config,
            settings,
            rest,
            member_intent,
            last_ready: Arc::new(AtomicI64::new(now())),
            cache: Arc::default(),
            active: Mutex::default(),
            cooldown: Mutex::default(),
            transport: MetagameHttp::default(),
        }
    }

    pub fn intents(member_intent: bool) -> u64 {
        if member_intent {
            gateway::GUILDS | gateway::GUILD_MEMBERS
        } else {
            gateway::GUILDS
        }
    }

    fn format_invite(&self, code: &str) -> Result<String, Failure> {
        invite_message(&self.invite_config, code).map_err(Failure::new)
    }

    async fn key(&self, command: Value) {
        let id = command_user(&command);
        let mut stage = "acknowledge".to_owned();
        let interaction = KeyInteraction {
            rest: self.rest.clone(),
            command: command.clone(),
            ephemeral: true,
        };
        let mut deferred = false;
        let result: Result<(), Failure> = async {
            defer_ephemeral(&self.rest, &command).await?;
            deferred = true;
            if let (Some(guild), Some(here)) = (&self.settings.guild, command["guild_id"].as_str())
                && here != guild
            {
                return interaction
                    .edit_reply("Use this command in the Revived server or in a DM with me.".into())
                    .await;
            }
            {
                let busy = self.active.lock().unwrap_or_else(PoisonError::into_inner).contains(&id)
                    || now()
                        < self
                            .cooldown
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .get(&id)
                            .copied()
                            .unwrap_or(0);
                if busy {
                    return interaction
                        .edit_reply("Your key request is being handled. Try again in a few seconds.".into())
                        .await;
                }
                self.active
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(id.clone());
                self.cooldown
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(id.clone(), now() + 10_000);
            }
            let result = self.key_command(&command, &interaction, &mut stage).await;
            self.active.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            result
        }
        .await;
        if let Err(error) = result {
            // No tokens, codes or payloads in the log
            eprintln!(
                "{}",
                json!({ "event": "key_interaction_failed", "at": iso(), "discordId": id, "stage": stage, "code": error.code, "status": error.status, "kind": "Error" })
            );
            if deferred {
                let _ = interaction
                    .edit_reply("The key service is unavailable. Please try again shortly.".into())
                    .await;
            }
        }
    }

    async fn key_command(
        &self,
        command: &Value,
        interaction: &KeyInteraction,
        stage: &mut String,
    ) -> Result<(), Failure> {
        let id = command_user(command);
        let name = subcommand(command);
        stage.clone_from(&name);
        let base = self.settings.metagame_url.clone();
        let secret = self.settings.recovery_secret.clone();
        let transport = &self.transport;
        let request = |action: &'static str, extra: Value| {
            let (base, secret, id) = (base.clone(), secret.clone(), id.clone());
            Box::pin(async move { recovery_request(transport, &base, secret.as_deref(), action, &id, extra).await })
                as crate::keys::BoxFuture<'_, Result<Value, Failure>>
        };
        if name == "recover" {
            return recover(interaction, Box::new(request)).await;
        }
        if name == "resend" {
            return resend(
                interaction,
                self.keys.run(&id, false),
                |code| self.format_invite(code),
                self.settings.recovery_secret.is_some(),
            )
            .await;
        }
        let claim = name == "claim";
        let mut result: Outcome = if name == "link" {
            let key = interaction.option("key").unwrap_or_default();
            self.keys.link(&id, service_kit::js::trim(&key)).await?
        } else if claim {
            let (rest, user, bot) = (&*self.rest, id.clone(), self.cache.user());
            let config = &self.invite_config;
            let dm_user = user.clone();
            let send: crate::keys::Send_<'_> = Box::new(move |code| {
                let dm_user = dm_user.clone();
                Box::pin(async move {
                    let content = invite_message(config, &code).map_err(Failure::new)?;
                    direct_message(rest, &dm_user, json!({ "content": content }))
                        .await
                        .map(Some)
                })
            });
            let reconcile: crate::keys::Reconcile<'_> = Box::new(move |code| {
                let (user, bot) = (user.clone(), bot.clone());
                Box::pin(async move { find_invite_message(rest, &user, &bot, &code).await })
            });
            self.keys.deliver(&id, send, Some(reconcile)).await?
        } else {
            self.keys.run(&id, false).await?
        };
        if claim && ["dm_disabled", "no_mutual_guild", "delivery_uncertain"].contains(&result.status.as_str()) {
            if !interaction.ephemeral() {
                return Err(Failure::new("Private acknowledgement required"));
            }
            let reply: crate::keys::Reply<'_> =
                Box::new(move |code| Box::pin(async move { interaction.edit_reply(self.format_invite(&code)?).await }));
            result = self.keys.deliver_private(&id, reply).await?;
            if result.status == "private_sent" {
                return Ok(());
            }
        }
        if name == "link" && result.status == "linked" && self.settings.recovery_secret.is_some() {
            let raw = interaction.option("key").unwrap_or_default();
            let key = match link_input(&raw) {
                LinkInput::Key(key) => Value::String(key),
                LinkInput::Refused(_) => Value::Null,
            };
            let proof = request("link", json!({ "key": key })).await?;
            if proof["verified"] != true {
                return interaction
                    .edit_reply("Account linked, but recovery verification did not complete. Retry /key link with the raw account key later.".into())
                    .await;
            }
        }
        *stage = "reply".into();
        println!(
            "{}",
            json!({ "event": "key_result", "status": result.status, "at": iso() })
        );
        if result.status == "sent" {
            println!("{}", json!({ "event": "key_dm_sent", "at": iso() }));
        }
        interaction.edit_reply(message(&result.status)).await
    }

    async fn counter(&self, command: Value) {
        if !self.member_intent {
            let reply = json!({ "type": 4, "data": {
                "content": "Enable Server Members Intent for this bot in Discord Developer Portal. The bot will reconnect automatically within a minute.",
                "flags": 64,
            } });
            let _ = respond(&self.rest, &command, reply).await;
            return;
        }
        if defer_ephemeral(&self.rest, &command).await.is_err() {
            eprintln!("Counter interaction failed");
            return;
        }
        let category = command["data"]["options"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|o| o["name"] == "category_id")
            .and_then(|o| o["value"].as_str())
            .unwrap_or("")
            .to_owned();
        let guild = command["guild_id"].as_str().map(|id| self.guild(id));
        let reply = self
            .counters
            .configure(
                guild.as_ref().map(|g| g as &dyn crate::counter::Guild),
                &command_user(&command),
                &category,
            )
            .await;
        if edit_response(&self.rest, &command, json!({ "content": reply }))
            .await
            .is_err()
        {
            eprintln!("Counter interaction failed");
        }
    }

    fn guild(&self, id: &str) -> DiscordGuild {
        DiscordGuild {
            rest: self.rest.clone(),
            cache: self.cache.clone(),
            id: id.to_owned(),
            bot: self.cache.user(),
        }
    }

    fn schedule(&self, id: &str) {
        if self.member_intent && !id.is_empty() {
            self.counters.schedule(Arc::new(self.guild(id)));
        }
    }

    /// One gateway dispatch. The cache takes it in first; the handlers run as their own tasks.
    pub fn dispatch(self: &Arc<Self>, name: &str, data: Value) {
        self.cache.update(name, &data);
        let bot = self.clone();
        match name {
            "READY" => {
                tokio::spawn(async move { bot.ready(data).await });
            }
            "RESUMED" => self.last_ready.store(now(), Ordering::SeqCst),
            // Application commands only
            "INTERACTION_CREATE" if data["type"] == 2 => {
                tokio::spawn(async move {
                    match data["data"]["name"].as_str() {
                        Some("counter") => bot.counter(data).await,
                        Some("key") => bot.key(data).await,
                        _ => {}
                    }
                });
            }
            "GUILD_MEMBER_ADD" | "GUILD_MEMBER_REMOVE" => {
                self.schedule(data["guild_id"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }

    async fn ready(&self, ready: Value) {
        self.last_ready.store(now(), Ordering::SeqCst);
        let mut guilds: Vec<String> = ready["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g["id"].as_str().map(str::to_owned))
            .collect();
        // discord.js reports ready once its guilds have arrived, or after 15 seconds without them
        for _ in 0..60 {
            if guilds.iter().all(|id| self.cache.guild(id).is_some()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if let Some(guild) = &self.settings.guild {
            guilds.push(guild.clone());
        }
        let application = ready["application"]["id"].as_str().unwrap_or_default().to_owned();
        let command = key_command(self.settings.recovery_secret.is_some());
        let rest: &dyn Rest = &*self.rest;
        let registered = async {
            let changed = sync_key_commands(rest, &application, &guilds, &command).await?;
            sync_counter_command(rest, &application).await?;
            Ok::<_, Failure>(changed)
        }
        .await;
        match registered {
            Ok(changed) => {
                println!("Key command registration: {changed} scopes updated");
                println!("Revived key bot ready");
            }
            Err(_) => {
                eprintln!("Could not register /key");
                std::process::exit(1);
            }
        }
        if self.member_intent {
            for id in self.counters.configured_guilds().await {
                if self.cache.guild(&id).is_none() {
                    continue;
                }
                if self.counters.refresh(&self.guild(&id)).await.is_err() {
                    eprintln!("Counter startup refresh failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_has_its_reply() {
        for status in [
            "sent",
            "already_sent",
            "dm_disabled",
            "no_mutual_guild",
            "delivery_uncertain",
            "linked",
            "invite_not_key",
            "invalid_key_format",
            "invalid_key",
            "discord_already_linked",
            "account_already_linked",
            "ready",
            "redeemed",
            "none",
            "pending",
            "revoked",
        ] {
            assert_ne!(message(status), message("unknown"), "{status}");
        }
    }
}
