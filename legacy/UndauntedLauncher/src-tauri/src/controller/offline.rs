//! Offline play in the controller (L43): `startOffline`, `stopOffline` and the save operations of
//! `controller.ts`. The local stack and the saves are the `offline` module's; this is the order of steps,
//! the cancellation between them, and the status the page draws. Built with the feature `offline` only.

use std::sync::Arc;

use super::{Controller, EndPlaying, Outcome, err};
use crate::bridge::types::{ActionResult, ErrorCode, LauncherError, OfflinePhase, OfflineStatus};
use crate::core::pins::XMPP_PORT;
use crate::core::username::{UsernameCheck, check_username};
use crate::net::hostapi::{RegisterResult, end_playing, fetch_user_info, register_account};
use crate::net::http::Endpoint;
use crate::offline::save::{PortableSave, offline_key_slot};
use crate::offline::stack::{StackManager, StackState};
use crate::offline::{REQUIRED_AVAILABLE_MIB, has_offline_memory};
use crate::platform::log;

/// Offline play's pieces, made by the app where the build and the PC have them.
pub struct OfflineSetup {
    pub stack: Arc<StackManager>,
    pub save: Arc<PortableSave>,
    /// Available memory in bytes (NaN when unknown); None skips the admission check
    pub free_memory: Option<fn() -> f64>,
    /// The local metagame's port: 61000, which the game and the Engine.ini point at
    pub port: u16,
}

#[derive(Debug)]
pub(super) struct OfflineState {
    status: OfflineStatus,
    /// The local stack runs for this launcher (`offlineSession`)
    session: bool,
    starting: bool,
    cancel_requested: bool,
    /// A start's busy section has ended; shutdown waits for it
    start_done: Arc<tokio::sync::Notify>,
}

impl Default for OfflineState {
    fn default() -> Self {
        Self {
            status: OfflineStatus {
                phase: OfflinePhase::Stopped,
                progress: None,
                error: None,
            },
            session: false,
            starting: false,
            cancel_requested: false,
            start_done: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl OfflineState {
    /// Online play waits while an offline stack runs or starts.
    pub(super) fn blocks_online(&self) -> bool {
        self.session || self.starting
    }
}

/// The local metagame, as the game and the account calls reach it.
fn local_endpoint(port: u16) -> Endpoint {
    Endpoint {
        host: "127.0.0.1".into(),
        port,
        pin: None,
    }
}

enum Account {
    Ready { key: String },
    Register,
    Error(LauncherError),
}

impl Controller {
    fn offline_setup(&self) -> Option<&OfflineSetup> {
        self.c.offline.as_ref()
    }

    pub fn offline_status(&self) -> OfflineStatus {
        self.st().off.status.clone()
    }

    fn set_offline_status(&self, phase: OfflinePhase, error: Option<LauncherError>) {
        let status = OfflineStatus {
            phase,
            progress: None,
            error,
        };
        self.st().off.status = status.clone();
        self.p.emit_offline_status(&status);
    }

    /// A failure the page shows, both as the error and in the offline status.
    fn offline_failed(&self, code: ErrorCode, detail: Option<String>) -> LauncherError {
        let error = self.fail(code, detail);
        self.set_offline_status(OfflinePhase::Error, Some(error.clone()));
        error
    }

    /// `offlineGameActive`: the game runs (or is starting) on the offline services.
    pub(super) fn offline_game_active(&self) -> bool {
        let st = self.st();
        st.off.session && (self.game.running() || st.off.status.phase == OfflinePhase::LaunchingGame)
    }

    async fn release_session(&self) {
        let session = std::mem::take(&mut self.st().off.session);
        if session && let Some(setup) = self.offline_setup() {
            setup.stack.stop().await;
        }
    }

    /// When a stop was asked for during a start: stops what started; Some(answer) ends the start.
    async fn finish_cancellation(&self) -> Option<Outcome> {
        {
            let mut st = self.st();
            if !st.off.cancel_requested {
                return None;
            }
            st.off.cancel_requested = false;
        }
        self.release_session().await;
        self.set_offline_status(OfflinePhase::Stopped, None);
        Some(Ok(()))
    }

    pub async fn start_offline(&self, username: Option<String>) -> ActionResult {
        {
            let st = self.st();
            if st.busy || st.task.is_some() || st.shutting_down {
                return ActionResult::err(ErrorCode::Busy);
            }
        }
        if self.game.running() {
            return ActionResult::Err(self.fail(ErrorCode::AlreadyRunning, None));
        }
        if self.offline_setup().is_none() {
            return ActionResult::Err(self.offline_failed(ErrorCode::OfflineUnavailable, None));
        }
        let done = {
            let mut st = self.st();
            if st.busy || st.task.is_some() || st.shutting_down || st.off.starting {
                return ActionResult::err(ErrorCode::Busy);
            }
            st.off.starting = true;
            st.off.cancel_requested = false;
            st.off.start_done.clone()
        };
        let outcome = self.with_busy(self.start_offline_now(username)).await;
        self.st().off.starting = false;
        done.notify_waiters();
        super::result(outcome)
    }

    async fn start_offline_now(&self, username: Option<String>) -> Outcome {
        let Some(setup) = self.offline_setup() else {
            return Err(err(ErrorCode::OfflineUnavailable, None));
        };
        self.set_offline_status(OfflinePhase::Preparing, None);
        let session = self.st().off.session;
        if !session {
            let save = setup.save.clone();
            let recovered = tokio::task::spawn_blocking(move || save.recover()).await;
            if !matches!(recovered, Ok(Ok(()))) {
                return Err(self.offline_failed(ErrorCode::OfflineUnavailable, None));
            }
        }
        let environment = self.check_game_launch_environment().await;
        if let Some(cancelled) = self.finish_cancellation().await {
            return cancelled;
        }
        if let Err(error) = environment {
            self.set_offline_status(OfflinePhase::Error, Some(error.clone()));
            return Err(error);
        }

        if !self.st().off.session {
            if let Some(free_memory) = setup.free_memory
                && !has_offline_memory(free_memory())
            {
                return Err(self.offline_failed(
                    ErrorCode::OfflineInsufficientMemory,
                    Some(REQUIRED_AVAILABLE_MIB.to_string()),
                ));
            }
            let me = self.me.clone();
            let on_state = move |state: StackState| {
                let Some(this) = me.upgrade() else { return };
                let phase = match state {
                    StackState::StartingMetagame => OfflinePhase::StartingMetagame,
                    StackState::StartingDeploy => OfflinePhase::StartingDeploy,
                    _ => return,
                };
                if !this.st().off.cancel_requested {
                    this.set_offline_status(phase, None);
                }
            };
            let started = setup.stack.start(&on_state).await;
            if let Err(reason) = started {
                if let Some(cancelled) = self.finish_cancellation().await {
                    return cancelled;
                }
                log::error(&format!("local stack could not start: {reason}"));
                return Err(self.offline_failed(ErrorCode::OfflineUnavailable, None));
            }
            self.st().off.session = true;
        }
        if let Some(cancelled) = self.finish_cancellation().await {
            return cancelled;
        }

        let endpoint = local_endpoint(setup.port);
        let mut account = self.offline_account(&endpoint).await;
        if let Some(cancelled) = self.finish_cancellation().await {
            return cancelled;
        }
        if matches!(account, Account::Register) {
            let Some(username) = username else {
                self.set_offline_status(OfflinePhase::NeedsUsername, None);
                return Ok(());
            };
            account = self.register_offline_account(&endpoint, &username).await;
            if let Some(cancelled) = self.finish_cancellation().await {
                return cancelled;
            }
        }
        let key = match account {
            Account::Ready { key } => key,
            // A name to fix: the stack keeps running and the page asks again
            Account::Error(error) if matches!(error.code, ErrorCode::UsernameInvalid | ErrorCode::UsernameTaken) => {
                self.set_offline_status(OfflinePhase::NeedsUsername, Some(error.clone()));
                return Err(error);
            }
            Account::Error(error) => {
                let error = self.fail(error.code, error.detail);
                self.release_session().await;
                self.set_offline_status(OfflinePhase::Error, Some(error.clone()));
                return Err(error);
            }
            Account::Register => return Err(err(ErrorCode::Unknown, None)),
        };

        let install = self.prepare_game_install().await;
        if let Some(cancelled) = self.finish_cancellation().await {
            return cancelled;
        }
        if let Err(error) = install {
            self.release_session().await;
            self.set_offline_status(OfflinePhase::Error, Some(error.clone()));
            return Err(error);
        }

        self.set_offline_status(OfflinePhase::LaunchingGame, None);
        let launched = self
            .launch_game(
                &endpoint.host,
                endpoint.port,
                XMPP_PORT,
                &key,
                EndPlaying::Offline(endpoint.clone(), key.clone()),
                " (offline)",
            )
            .await;
        if let Err(error) = launched {
            self.release_session().await;
            self.set_offline_status(OfflinePhase::Error, Some(error.clone()));
            return Err(error);
        }
        // The game may already have ended during the launch, which stopped the stack itself
        if self.game.running() {
            self.set_offline_status(OfflinePhase::Running, None);
        }
        Ok(())
    }

    /// The offline account: its key from the offline key store, checked with the local metagame.
    async fn offline_account(&self, endpoint: &Endpoint) -> Account {
        let Some(setup) = self.offline_setup() else {
            return Account::Error(err(ErrorCode::OfflineUnavailable, None));
        };
        let key = match setup.save.keys().load(&offline_key_slot()) {
            Ok(key) => key,
            Err(error) => {
                log::error(&format!("could not load the offline account key: {error:?}"));
                return Account::Error(err(ErrorCode::KeyStorageUnavailable, None));
            }
        };
        let Some(key) = key else { return Account::Register };
        match fetch_user_info(endpoint, &key).await {
            Ok(_) => Account::Ready { key },
            // The rejected key stays until a replacement account exists
            Err((ErrorCode::KeyRejected, _)) => Account::Register,
            Err((code, detail)) => Account::Error(err(code, detail)),
        }
    }

    async fn register_offline_account(&self, endpoint: &Endpoint, username: &str) -> Account {
        let check = check_username(username);
        if check != UsernameCheck::Ok {
            return Account::Error(err(ErrorCode::UsernameInvalid, Some(check.as_str().to_owned())));
        }
        let key = match register_account(endpoint, username, "").await {
            RegisterResult::Ok { key } => key,
            RegisterResult::Err { error, .. } => {
                let code = serde_json::to_value(error).unwrap_or_default();
                log::warn(&format!(
                    "offline registration refused: {}",
                    code.as_str().unwrap_or("")
                ));
                return Account::Error(err(error, None));
            }
        };
        let Some(setup) = self.offline_setup() else {
            return Account::Error(err(ErrorCode::OfflineUnavailable, None));
        };
        if let Err(error) = setup.save.keys().save(&offline_key_slot(), &key) {
            log::error(&format!("could not store the offline account key: {error:?}"));
            return Account::Error(err(ErrorCode::KeyStorageUnavailable, None));
        }
        log::info(&format!("registered offline account as {username}"));
        Account::Ready { key }
    }

    /// The game launched for offline play has exited: the session ends and the stack stops with it.
    pub(super) async fn offline_game_ended(&self, endpoint: Endpoint, key: String) {
        self.set_offline_status(OfflinePhase::Stopping, None);
        let _ = end_playing(&endpoint, &key).await;
        self.release_session().await;
        self.set_offline_status(OfflinePhase::Stopped, None);
    }

    pub async fn stop_offline(&self) -> ActionResult {
        if self.game.running() || self.st().off.status.phase == OfflinePhase::LaunchingGame {
            return ActionResult::err(ErrorCode::Busy);
        }
        {
            let mut st = self.st();
            if st.off.starting {
                st.off.cancel_requested = true;
                drop(st);
                self.set_offline_status(OfflinePhase::Stopping, None);
                return ActionResult::Ok;
            }
            if !st.off.session {
                drop(st);
                self.set_offline_status(OfflinePhase::Stopped, None);
                return ActionResult::Ok;
            }
        }
        self.set_offline_status(OfflinePhase::Stopping, None);
        self.release_session().await;
        self.set_offline_status(OfflinePhase::Stopped, None);
        ActionResult::Ok
    }

    /// Backs the offline profile up (`export`) or restores one, after the player picks the file. Only while
    /// nothing runs: no task, no game, no stack.
    pub async fn offline_save(&self, export: bool) -> ActionResult {
        let free = |st: &super::State| {
            !(st.busy
                || st.task.is_some()
                || st.off.session
                || st.off.starting
                || st.off.status.phase == OfflinePhase::Stopping
                || st.shutting_down)
        };
        if !free(&self.st()) || self.game.running() {
            return ActionResult::err(ErrorCode::Busy);
        }
        let Some(save) = self.offline_setup().map(|setup| setup.save.clone()) else {
            return ActionResult::err(ErrorCode::OfflineUnavailable);
        };
        let Some(file) = self.p.choose_offline_save(export).await else {
            return ActionResult::err(ErrorCode::Cancelled);
        };
        if !self.begin_busy(free) || self.game.running() {
            return ActionResult::err(ErrorCode::Busy);
        }
        // Another launcher's game, or one this launcher lost track of, could be using the profile
        let outcome = if !self.p.find_running_clients().await.is_empty() {
            ActionResult::err(ErrorCode::Busy)
        } else {
            tokio::task::spawn_blocking(move || if export { save.export(&file) } else { save.import(&file) })
                .await
                .unwrap_or_else(|_| ActionResult::err(ErrorCode::Unknown))
        };
        self.end_busy();
        outcome
    }

    /// On exit: a start still running is cancelled and waited for, then the stack stops.
    pub(super) async fn shutdown_offline(&self) {
        let waiting = {
            let mut st = self.st();
            if st.off.starting {
                st.off.cancel_requested = true;
                Some(st.off.start_done.clone())
            } else {
                None
            }
        };
        if let Some(done) = waiting {
            let notified = done.notified();
            if self.st().off.starting {
                if !self.game.running() && self.st().off.status.phase != OfflinePhase::LaunchingGame {
                    self.set_offline_status(OfflinePhase::Stopping, None);
                }
                notified.await;
            }
        }
        if self.st().off.session {
            self.set_offline_status(OfflinePhase::Stopping, None);
            self.release_session().await;
            self.set_offline_status(OfflinePhase::Stopped, None);
        }
    }
}
