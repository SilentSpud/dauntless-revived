//! The live member counter (`counter.mjs`): three locked voice channels named `Bots : n`,
//! `Users : n` and `Total : n` in a category the owner chose. Counts come from a complete member
//! list; a channel is created once and only renamed when its count changes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::keys::{BoxFuture, Failure, save_state};

pub const COUNTER_OWNER: &str = "1121938502947459152";
pub const CATEGORY_NAME: &str = "📊 Live Counter";
const LABELS: [&str; 3] = ["Bots", "Users", "Total"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Category,
    Voice,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub parent_id: Option<String>,
    pub guild_id: String,
    /// @everyone may not connect or speak
    pub locked: bool,
    /// The bot may see and manage it
    pub managed: bool,
}

/// One guild, as far as the counter needs it.
pub trait Guild: Send + Sync {
    fn id(&self) -> String;
    /// Discord's member count
    fn member_count(&self) -> u64;
    /// (bots, users) of the members known; `fetch` loads them all first
    fn counts(&self, fetch: bool) -> BoxFuture<'_, Result<(u64, u64), Failure>>;
    fn channel(&self, id: &str) -> BoxFuture<'_, Result<Option<Channel>, Failure>>;
    fn channels(&self) -> BoxFuture<'_, Result<Vec<Channel>, Failure>>;
    /// The bot may manage channels in this category
    fn can_manage(&self, category: &Channel) -> BoxFuture<'_, Result<bool, Failure>>;
    fn rename(&self, id: &str, name: &str) -> BoxFuture<'_, Result<(), Failure>>;
    fn create_voice(&self, name: &str, parent: &str) -> BoxFuture<'_, Result<Channel, Failure>>;
    fn lock(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>>;
    fn allow_self(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>>;
}

pub fn load_counters(file: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({ "version": 1, "guilds": {} })),
        Err(error) => Err(error.to_string()),
        Ok(text) => {
            let state: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if state["version"] != 1 || !state["guilds"].is_object() {
                return Err("Invalid counter state".into());
            }
            Ok(state)
        }
    }
}

type Save = Arc<dyn Fn(Value) -> BoxFuture<'static, Result<(), Failure>> + Send + Sync>;

pub struct Counters {
    pub state: tokio::sync::Mutex<Value>,
    save: Save,
    loaded: Mutex<HashSet<String>>,
    timers: Mutex<HashSet<String>>,
}

/// A refusal the owner sees as it is.
fn says(message: &str) -> Failure {
    Failure::new(message)
}

fn shown(error: &Failure) -> bool {
    ["Enter ", "A counter", "The category", "I need"]
        .iter()
        .any(|start| error.message.starts_with(start))
}

impl Counters {
    pub fn new(state: Value, save: Save) -> Self {
        Self {
            state: tokio::sync::Mutex::new(state),
            save,
            loaded: Mutex::default(),
            timers: Mutex::default(),
        }
    }

    pub fn from_file(file: PathBuf) -> Result<Self, String> {
        let state = load_counters(&file)?;
        let file = Arc::new(file);
        Ok(Self::new(
            state,
            Arc::new(move |state| {
                let file = file.clone();
                Box::pin(async move { save_state(&file, &state).await })
            }),
        ))
    }

    /// `/counter category_id:`; the private reply.
    pub async fn configure(&self, guild: Option<&dyn Guild>, user: &str, category: &str) -> String {
        if user != COUNTER_OWNER {
            return "Only the server owner can configure counters.".into();
        }
        let Some(guild) = guild else {
            return "Run this command in the Discord server.".into();
        };
        let result = async {
            if !((17..=20).contains(&category.len()) && category.bytes().all(|b| b.is_ascii_digit())) {
                return Err(says("Enter a valid category ID."));
            }
            let mut state = self.state.lock().await;
            let found = guild.channel(category).await?;
            validate(guild, found.as_ref()).await?;
            let id = guild.id();
            if let Some(previous) = state["guilds"].get(&id)
                && previous["categoryId"] != category
            {
                return Err(says(
                    "A counter category is already configured. Reuse it to avoid duplicate counters.",
                ));
            }
            if state["guilds"].get(&id).is_none() {
                state["guilds"][&id] = json!({ "categoryId": category, "channels": {} });
            }
            (self.save)(state.clone()).await?;
            self.refresh_locked(&mut state, guild).await
        }
        .await;
        match result {
            Ok(()) => "📊 Live Counter configured. Bots, Users and Total will update automatically.".into(),
            Err(error) if shown(&error) => error.message,
            Err(_) => {
                "Counter setup failed. Check the category, Manage Channels permission and Server Members intent.".into()
            }
        }
    }

    pub async fn refresh(&self, guild: &dyn Guild) -> Result<(), Failure> {
        let mut state = self.state.lock().await;
        self.refresh_locked(&mut state, guild).await
    }

    async fn refresh_locked(&self, state: &mut Value, guild: &dyn Guild) -> Result<(), Failure> {
        let id = guild.id();
        let Some(config) = state["guilds"].get(&id).cloned() else {
            return Ok(());
        };
        let category_id = config["categoryId"].as_str().unwrap_or("").to_owned();
        let category = guild.channel(&category_id).await?;
        validate(guild, category.as_ref()).await?;
        let Some(category) = category else {
            return Ok(());
        };
        let first = self
            .loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone());
        let (mut bots, mut users) = guild.counts(first).await?;
        if bots + users != guild.member_count() {
            (bots, users) = guild.counts(true).await?;
        }
        if bots + users != guild.member_count() {
            return Err(Failure::new("Member cache is incomplete"));
        }
        let counts: HashMap<&str, u64> = [("Bots", bots), ("Users", users), ("Total", bots + users)].into();
        let channels = guild.channels().await?;
        if category.name != CATEGORY_NAME {
            guild.rename(&category.id, CATEGORY_NAME).await?;
        }
        for label in LABELS {
            let name = format!("{label} : {}", counts[label]);
            let configured = config["channels"][label].as_str();
            let mut channel = configured
                .and_then(|cid| channels.iter().find(|c| c.id == cid))
                .cloned();
            if channel.is_none() {
                channel = channels
                    .iter()
                    .find(|c| {
                        c.parent_id.as_deref() == Some(category.id.as_str())
                            && c.kind == Kind::Voice
                            && c.name.starts_with(&format!("{label} : "))
                    })
                    .cloned();
            }
            if let Some(channel) = &channel
                && (channel.parent_id.as_deref() != Some(category.id.as_str()) || channel.kind != Kind::Voice)
            {
                return Err(Failure::new("Counter channel was moved; operator review required"));
            }
            let channel = match channel {
                Some(channel) => channel,
                None => guild.create_voice(&name, &category.id).await?,
            };
            if configured != Some(channel.id.as_str()) {
                state["guilds"][&id]["channels"][label] = json!(channel.id);
                (self.save)(state.clone()).await?;
            }
            if !channel.locked {
                guild.lock(&channel.id).await?;
            }
            if !channel.managed {
                guild.allow_self(&channel.id).await?;
            }
            if channel.name != name {
                guild.rename(&channel.id, &name).await?;
            }
        }
        Ok(())
    }

    /// A refresh 10 s after a member joined or left (several changes fold into one).
    pub fn schedule(self: &Arc<Self>, guild: Arc<dyn Guild>) {
        let id = guild.id();
        let configured = self
            .state
            .try_lock()
            .map(|state| state["guilds"].get(&id).is_some())
            .unwrap_or(true);
        if !configured
            || !self
                .timers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id.clone())
        {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            this.timers.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            if this.refresh(&*guild).await.is_err() {
                eprintln!("Counter refresh failed; will retry on next refresh");
            }
        });
    }

    pub async fn configured_guilds(&self) -> Vec<String> {
        self.state.lock().await["guilds"]
            .as_object()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default()
    }
}

async fn validate(guild: &dyn Guild, category: Option<&Channel>) -> Result<(), Failure> {
    let Some(category) = category.filter(|c| c.kind == Kind::Category && c.guild_id == guild.id()) else {
        return Err(says("The category must belong to this server."));
    };
    if !guild.can_manage(category).await? {
        return Err(says("I need Manage Channels in that category."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const GUILD: &str = "123456789012345678";
    const CATEGORY: &str = "223456789012345678";

    #[derive(Default)]
    struct Fake {
        channels: Mutex<Vec<Channel>>,
        members: Mutex<(u64, u64)>,
        member_count: Mutex<u64>,
        manage: Mutex<bool>,
        edits: AtomicUsize,
        creates: AtomicUsize,
    }

    impl Guild for Fake {
        fn id(&self) -> String {
            GUILD.into()
        }

        fn member_count(&self) -> u64 {
            *self.member_count.lock().unwrap()
        }

        fn counts(&self, _fetch: bool) -> BoxFuture<'_, Result<(u64, u64), Failure>> {
            let counts = *self.members.lock().unwrap();
            Box::pin(async move { Ok(counts) })
        }

        fn channel(&self, id: &str) -> BoxFuture<'_, Result<Option<Channel>, Failure>> {
            let found = self.channels.lock().unwrap().iter().find(|c| c.id == id).cloned();
            Box::pin(async move { Ok(found) })
        }

        fn channels(&self) -> BoxFuture<'_, Result<Vec<Channel>, Failure>> {
            let all = self.channels.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn can_manage(&self, _category: &Channel) -> BoxFuture<'_, Result<bool, Failure>> {
            let manage = *self.manage.lock().unwrap();
            Box::pin(async move { Ok(manage) })
        }

        fn rename(&self, id: &str, name: &str) -> BoxFuture<'_, Result<(), Failure>> {
            self.edits.fetch_add(1, Ordering::SeqCst);
            if let Some(c) = self.channels.lock().unwrap().iter_mut().find(|c| c.id == id) {
                c.name = name.into();
            }
            Box::pin(async { Ok(()) })
        }

        fn create_voice(&self, name: &str, parent: &str) -> BoxFuture<'_, Result<Channel, Failure>> {
            let n = self.creates.fetch_add(1, Ordering::SeqCst) + 1;
            let channel = voice(&n.to_string(), name, parent);
            self.channels.lock().unwrap().push(channel.clone());
            Box::pin(async move { Ok(channel) })
        }

        fn lock(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>> {
            self.edits.fetch_add(1, Ordering::SeqCst);
            if let Some(c) = self.channels.lock().unwrap().iter_mut().find(|c| c.id == id) {
                c.locked = true;
            }
            Box::pin(async { Ok(()) })
        }

        fn allow_self(&self, id: &str) -> BoxFuture<'_, Result<(), Failure>> {
            self.edits.fetch_add(1, Ordering::SeqCst);
            if let Some(c) = self.channels.lock().unwrap().iter_mut().find(|c| c.id == id) {
                c.managed = true;
            }
            Box::pin(async { Ok(()) })
        }
    }

    fn voice(id: &str, name: &str, parent: &str) -> Channel {
        Channel {
            id: id.into(),
            name: name.into(),
            kind: Kind::Voice,
            parent_id: Some(parent.into()),
            guild_id: GUILD.into(),
            locked: false,
            managed: false,
        }
    }

    fn fixture() -> (Fake, Arc<Counters>, Arc<AtomicUsize>) {
        let guild = Fake::default();
        guild.channels.lock().unwrap().push(Channel {
            id: CATEGORY.into(),
            name: "old".into(),
            kind: Kind::Category,
            parent_id: None,
            guild_id: GUILD.into(),
            locked: false,
            managed: false,
        });
        *guild.members.lock().unwrap() = (1, 1);
        *guild.member_count.lock().unwrap() = 2;
        *guild.manage.lock().unwrap() = true;
        let saves = Arc::new(AtomicUsize::new(0));
        let s = saves.clone();
        let counters = Counters::new(
            json!({ "version": 1, "guilds": {} }),
            Arc::new(move |_| {
                s.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }),
        );
        (guild, Arc::new(counters), saves)
    }

    #[tokio::test]
    async fn counts_bots_apart_and_creates_three_locked_channels_once() {
        let (guild, counters, _) = fixture();
        let reply = counters.configure(Some(&guild), COUNTER_OWNER, CATEGORY).await;
        assert!(reply.starts_with("📊 Live Counter configured"), "{reply}");
        assert_eq!(guild.creates.load(Ordering::SeqCst), 3);
        let channels = guild.channels.lock().unwrap().clone();
        assert_eq!(channels[0].name, CATEGORY_NAME);
        let names: Vec<&str> = channels
            .iter()
            .filter(|c| c.kind == Kind::Voice)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Bots : 1", "Users : 1", "Total : 2"]);
        assert!(channels.iter().filter(|c| c.kind == Kind::Voice).all(|c| c.locked));
        let edits = guild.edits.load(Ordering::SeqCst);
        counters.configure(Some(&guild), COUNTER_OWNER, CATEGORY).await;
        assert_eq!(guild.creates.load(Ordering::SeqCst), 3);
        assert_eq!(guild.edits.load(Ordering::SeqCst), edits);
    }

    #[tokio::test]
    async fn reuses_a_channel_survives_a_restart_and_renames_only_changed_counts() {
        let (guild, counters, _) = fixture();
        guild.channels.lock().unwrap().push(voice("old", "Bots : 0", CATEGORY));
        counters.configure(Some(&guild), COUNTER_OWNER, CATEGORY).await;
        assert_eq!(guild.creates.load(Ordering::SeqCst), 2);
        let restarted = Counters::new(
            counters.state.lock().await.clone(),
            Arc::new(|_| Box::pin(async { Ok(()) })),
        );
        let edits = guild.edits.load(Ordering::SeqCst);
        restarted.refresh(&guild).await.unwrap();
        assert_eq!(guild.edits.load(Ordering::SeqCst), edits);
        *guild.members.lock().unwrap() = (1, 2);
        *guild.member_count.lock().unwrap() = 3;
        restarted.refresh(&guild).await.unwrap();
        assert_eq!(guild.edits.load(Ordering::SeqCst), edits + 2);
    }

    #[tokio::test]
    async fn non_owner_wrong_category_and_missing_permission_are_refused() {
        for mode in ["owner", "category", "permission"] {
            let (guild, counters, saves) = fixture();
            if mode == "category" {
                guild.channels.lock().unwrap()[0].kind = Kind::Other;
            }
            if mode == "permission" {
                *guild.manage.lock().unwrap() = false;
            }
            let user = if mode == "owner" { "wrong" } else { COUNTER_OWNER };
            let reply = counters.configure(Some(&guild), user, CATEGORY).await;
            assert!(!reply.starts_with("📊"), "{mode}: {reply}");
            assert_eq!(guild.creates.load(Ordering::SeqCst), 0);
            assert_eq!(saves.load(Ordering::SeqCst), 0);
        }
    }
}
