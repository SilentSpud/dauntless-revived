//! Invites and account links (`keys.mjs`). The state file (`KEY_STATE_FILE`) loads unchanged from
//! the Node bot, and a file that cannot be read stops issuance: a lost state would hand everyone a
//! second invite. Every operation runs one at a time; a code is saved before the metagame hears of
//! it, and a DM is reserved durably before it is sent, so a crash or an ambiguous Discord answer
//! never issues or sends twice.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::link_input::{LinkInput, link_input};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A failure, with what the Node code kept of it: Discord's error code, an HTTP status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub code: Option<i64>,
    pub status: Option<u16>,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }

    pub fn discord(code: i64) -> Self {
        Self {
            message: format!("Discord error {code}"),
            code: Some(code),
            status: None,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Discord: cannot send messages to this user / no mutual guild.
pub const DM_DISABLED: i64 = 50007;
pub const NO_MUTUAL_GUILD: i64 = 50278;

fn is_discord_id(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// `loadState`.
pub fn load_state(file: &Path) -> Result<Value, String> {
    let refuse = || "Cannot read key state; refusing to issue replacement codes".to_owned();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({ "version": 1, "users": {} })),
        Err(_) => return Err(refuse()),
    };
    let state: Value = serde_json::from_str(&text).map_err(|_| refuse())?;
    if state["version"] != 1 || !state["users"].is_object() {
        return Err(refuse());
    }
    if let Some(links) = state.get("links") {
        let valid = links.as_object().is_some_and(|links| {
            links
                .iter()
                .all(|(id, value)| is_discord_id(id) && value["userId"].is_string() && value["username"].is_string())
        });
        if !valid {
            return Err(refuse());
        }
    }
    Ok(state)
}

/// `saveState`: a private temporary file, then one atomic rename. On Windows a rename that an
/// antivirus or indexer briefly denies is retried (50, 100, 200 ... ms, six times), never replaced by
/// a delete and copy.
pub async fn save_state(file: &Path, state: &Value) -> Result<(), Failure> {
    save_state_with(
        file,
        state,
        cfg!(windows),
        |from, to| std::fs::rename(from, to),
        |ms| Box::pin(tokio::time::sleep(Duration::from_millis(ms))),
    )
    .await
}

pub async fn save_state_with(
    file: &Path,
    state: &Value,
    windows: bool,
    replace: impl Fn(&Path, &Path) -> std::io::Result<()>,
    sleep: impl Fn(u64) -> BoxFuture<'static, ()>,
) -> Result<(), Failure> {
    let failed = |e: std::io::Error| Failure::new(e.to_string());
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(failed)?;
    }
    let mut temp = file.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = std::path::PathBuf::from(temp);
    write_private(&temp, state.to_string().as_bytes()).map_err(failed)?;
    let mut attempt = 0;
    loop {
        match replace(&temp, file) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let transient =
                    error.kind() == std::io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(32);
                if !windows || !transient || attempt >= 6 {
                    return Err(failed(error));
                }
                sleep(50 * 2u64.pow(attempt)).await;
                attempt += 1;
            }
        }
    }
}

fn write_private(file: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(file)?.write_all(data)
}

/// A row of the metagame's invite codes.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub uses_remaining: Value,
    pub infinite_uses: bool,
}

/// What the bot asks the metagame (on this machine, with the admin key).
pub trait Backend: Send + Sync {
    fn link_account(&self, discord_id: &str, user_id: &str) -> BoxFuture<'_, Result<Value, Failure>>;
    /// The linked account, if any
    fn linked_account(&self, discord_id: &str) -> BoxFuture<'_, Result<Option<Value>, Failure>>;
    fn find(&self, code: &str) -> BoxFuture<'_, Result<Option<Row>, Failure>>;
    fn create(&self, code: &str) -> BoxFuture<'_, Result<(), Failure>>;
    /// The account a launcher key belongs to: (user id, username), `None` for an unknown key
    fn identity(&self, key: &str) -> BoxFuture<'_, Result<Option<(String, String)>, Failure>>;
}

/// The answer of [`Keys`]: a status for the reply, and the code when one is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub status: String,
    pub code: Option<String>,
}

impl Outcome {
    pub fn of(status: &str) -> Self {
        Self {
            status: status.to_owned(),
            code: None,
        }
    }
}

/// A message the bot sent earlier, found in the user's DMs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: String,
    pub created_at: String,
}

/// The DM history check: `Ok(None)` when the history could not be read to the end (never proof of
/// absence), `Ok(Some(None))` when it proves the invite was not sent.
pub type Reconcile<'a> = Box<dyn FnOnce(String) -> BoxFuture<'a, Result<Option<Option<Found>>, Failure>> + Send + 'a>;
pub type Send_<'a> = Box<dyn FnOnce(String) -> BoxFuture<'a, Result<Option<String>, Failure>> + Send + 'a>;
pub type Reply<'a> = Box<dyn FnOnce(String) -> BoxFuture<'a, Result<(), Failure>> + Send + 'a>;

type Save = Arc<dyn Fn(Value) -> BoxFuture<'static, Result<(), Failure>> + Send + Sync>;
type Generate = Arc<dyn Fn() -> String + Send + Sync>;

pub fn generate_code() -> String {
    let bytes: [u8; 18] = rand::random();
    format!("DR-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub struct Keys {
    pub state: tokio::sync::Mutex<Value>,
    save: std::sync::Mutex<Save>,
    api: Arc<dyn Backend>,
    generate: Generate,
}

impl Keys {
    pub fn new(state: Value, save: Save, api: Arc<dyn Backend>, generate: Generate) -> Self {
        Self {
            state: tokio::sync::Mutex::new(state),
            save: std::sync::Mutex::new(save),
            api,
            generate,
        }
    }

    /// Replaces how the state is saved (tests of failing disks).
    pub fn set_save(&self, save: Save) {
        *self.save.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = save;
    }

    async fn persist(&self, state: &Value) -> Result<(), Failure> {
        let save = self
            .save
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        save(state.clone()).await
    }

    /// Links from the Node bot's old state move to the metagame, once and idempotently.
    pub async fn migrate_links(&self) -> Result<(), Failure> {
        let links: Vec<(String, String)> = {
            let state = self.state.lock().await;
            state["links"]
                .as_object()
                .map(|links| {
                    links
                        .iter()
                        .map(|(id, v)| (id.clone(), v["userId"].as_str().unwrap_or("").to_owned()))
                        .collect()
                })
                .unwrap_or_default()
        };
        for (discord_id, user_id) in links {
            let result = self.api.link_account(&discord_id, &user_id).await?;
            if result["status"] != "linked" {
                return Err(Failure::new("Legacy account link needs operator review"));
            }
        }
        Ok(())
    }

    /// `/key status` (`claim` false) or the first half of a claim.
    pub async fn run(&self, user: &str, claim: bool) -> Result<Outcome, Failure> {
        let mut state = self.state.lock().await;
        self.handle(&mut state, user, claim).await
    }

    pub async fn link(&self, user: &str, launcher_key: &str) -> Result<Outcome, Failure> {
        let _turn = self.state.lock().await;
        if !is_discord_id(user) {
            return Err(Failure::new("Invalid Discord user"));
        }
        let key = match link_input(launcher_key) {
            LinkInput::Key(key) => key,
            LinkInput::Refused(status) => return Ok(Outcome::of(status)),
        };
        let Some((user_id, _)) = self.api.identity(&key).await? else {
            return Ok(Outcome::of(if key.starts_with("DR-") {
                "invite_not_key"
            } else {
                "invalid_key"
            }));
        };
        let result = self.api.link_account(user, &user_id).await?;
        Ok(Outcome::of(result["status"].as_str().unwrap_or("")))
    }

    /// Shows this user's existing unused invite privately (when the DM failed or is uncertain).
    pub async fn deliver_private(&self, user: &str, reply: Reply<'_>) -> Result<Outcome, Failure> {
        let mut state = self.state.lock().await;
        let result = self.handle(&mut state, user, true).await?;
        let Some(code) = result.code.clone().filter(|_| result.status == "ready") else {
            return Ok(result);
        };
        reply(code).await?;
        let entry = &mut state["users"][user];
        entry["delivery"] = json!("sent");
        entry["deliveryChannel"] = json!("ephemeral");
        entry["sentAt"] = json!(now_iso());
        if let Some(fields) = entry.as_object_mut() {
            fields.remove("lastDeliveryError");
        }
        self.persist(&state).await?;
        Ok(Outcome::of("private_sent"))
    }

    /// `/key claim`: the invite by DM, once.
    pub async fn deliver(
        &self,
        user: &str,
        send: Send_<'_>,
        reconcile: Option<Reconcile<'_>>,
    ) -> Result<Outcome, Failure> {
        let mut state = self.state.lock().await;
        let result = self.handle(&mut state, user, true).await?;
        let Some(code) = result.code.clone().filter(|_| result.status == "ready") else {
            return Ok(result);
        };
        // A previous private reply was dismissible: deliver that same invite once by DM
        if state["users"][user]["deliveryChannel"] == "ephemeral" {
            state["users"][user]["delivery"] = json!("unsent");
        }
        let delivery = state["users"][user]["delivery"].as_str().map(str::to_owned);
        if delivery.as_deref() == Some("sent") {
            return Ok(Outcome::of("already_sent"));
        }
        if delivery.as_deref() != Some("unsent") {
            let Some(reconcile) = reconcile else {
                return Ok(Outcome::of(if delivery.as_deref() == Some("reserved") {
                    "delivery_uncertain"
                } else {
                    "already_sent"
                }));
            };
            let previous = match reconcile(code.clone()).await {
                Ok(previous) => previous,
                Err(error) if error.code == Some(DM_DISABLED) => return Ok(Outcome::of("dm_disabled")),
                Err(error) if error.code == Some(NO_MUTUAL_GUILD) => return Ok(Outcome::of("no_mutual_guild")),
                Err(_) => return Ok(Outcome::of("delivery_uncertain")),
            };
            match previous {
                None => return Ok(Outcome::of("delivery_uncertain")),
                Some(Some(found)) => {
                    let entry = &mut state["users"][user];
                    entry["delivery"] = json!("sent");
                    entry["deliveryChannel"] = json!("dm");
                    entry["messageId"] = json!(found.id);
                    entry["sentAt"] = json!(found.created_at);
                    self.persist(&state).await?;
                    return Ok(Outcome::of("already_sent"));
                }
                Some(None) => {}
            }
        }
        // Reserve durably before Discord: an ambiguous timeout must never send twice
        {
            let entry = &mut state["users"][user];
            entry["delivery"] = json!("reserved");
            entry["deliveryChannel"] = json!("dm");
            entry["attemptedAt"] = json!(now_iso());
        }
        if let Err(error) = self.persist(&state).await {
            state["users"][user]["delivery"] = json!("unsent");
            return Err(error);
        }
        let message = match send(code).await {
            Ok(message) => message,
            // Discord explicitly rejected the DM: nothing was delivered
            Err(error) if matches!(error.code, Some(DM_DISABLED | NO_MUTUAL_GUILD)) => {
                let entry = &mut state["users"][user];
                entry["delivery"] = json!("unsent");
                entry["lastDeliveryError"] = json!(error.code);
                self.persist(&state).await?;
                return Ok(Outcome::of(if error.code == Some(NO_MUTUAL_GUILD) {
                    "no_mutual_guild"
                } else {
                    "dm_disabled"
                }));
            }
            Err(_) => return Ok(Outcome::of("delivery_uncertain")),
        };
        let entry = &mut state["users"][user];
        entry["delivery"] = json!("sent");
        if let Some(fields) = entry.as_object_mut() {
            fields.remove("lastDeliveryError");
        }
        entry["sentAt"] = json!(now_iso());
        if let Some(id) = message {
            entry["messageId"] = json!(id);
        }
        self.persist(&state).await?;
        Ok(Outcome::of("sent"))
    }

    async fn handle(&self, state: &mut Value, user: &str, claim: bool) -> Result<Outcome, Failure> {
        if !is_discord_id(user) {
            return Err(Failure::new("Invalid Discord user"));
        }
        if self.api.linked_account(user).await?.is_some() {
            return Ok(Outcome::of("linked"));
        }
        if state["users"].get(user).is_none() {
            if !claim {
                return Ok(Outcome::of("none"));
            }
            let mut entry = Map::new();
            entry.insert("code".into(), json!((self.generate)()));
            entry.insert("pending".into(), json!(true));
            entry.insert("delivery".into(), json!("unsent"));
            entry.insert("createdAt".into(), json!(now_iso()));
            if let Some(users) = state["users"].as_object_mut() {
                users.insert(user.to_owned(), Value::Object(entry));
            }
            // Saved before the metagame hears of it: a retry after a crash reuses this code
            if let Err(error) = self.persist(state).await {
                if let Some(users) = state["users"].as_object_mut() {
                    users.remove(user);
                }
                return Err(error);
            }
        }
        let code = state["users"][user]["code"].as_str().unwrap_or("").to_owned();
        let pending = state["users"][user]["pending"] == true;
        let mut row = self.api.find(&code).await?;
        if row.is_none() && pending && claim {
            self.api.create(&code).await?;
            row = self.api.find(&code).await?;
        }
        let Some(row) = row else {
            return Ok(Outcome::of(if pending { "pending" } else { "revoked" }));
        };
        let uses = row.uses_remaining.as_f64().filter(|n| n.fract() == 0.0);
        let Some(uses) = uses.filter(|u| (0.0..=1.0).contains(u)).filter(|_| !row.infinite_uses) else {
            return Err(Failure::new("Unexpected code state"));
        };
        if pending {
            state["users"][user]["pending"] = json!(false);
            self.persist(state).await?;
        }
        Ok(if uses == 0.0 {
            Outcome::of("redeemed")
        } else {
            Outcome {
                status: "ready".into(),
                code: Some(code),
            }
        })
    }
}
