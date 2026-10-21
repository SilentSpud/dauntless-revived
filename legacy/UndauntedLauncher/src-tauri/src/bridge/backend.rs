//! What the commands talk to until the controller exists (row L38): the snapshot the page draws, the
//! settings it changes, and the progress channel. Everything that needs the network, the key store or
//! the game folder answers "not in this build yet" until its row is ported (P2 to P5).

use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};

use super::progress::Throttle;
use super::types::{
    ActionResult, ErrorCode, ExposureMode, GameLanguage, GraphicsPreset, LauncherError, Phase, Settings, SettingsPatch,
    Snapshot, SnapshotAccount, SnapshotApp, SnapshotConnect, SnapshotGame, SnapshotInstall, TaskProgress,
};
use crate::window::MAIN;

/// Pushed to the page whenever the snapshot changes.
pub const SNAPSHOT_EVENT: &str = "snapshot";

pub fn default_settings() -> Settings {
    Settings {
        hunt_region: None,
        graphics: GraphicsPreset::DEFAULT,
        exposure: ExposureMode::Game,
        windowed: false,
        language: crate::core::i18n::os_language(),
        game_language: GameLanguage::Auto,
        show_console: false,
        streamer_mode: None,
    }
}

pub fn initial_snapshot(version: &str, packaged: bool) -> Snapshot {
    Snapshot {
        phase: Phase::Join,
        busy: false,
        server: None,
        saved_servers: Vec::new(),
        connect: SnapshotConnect {
            checking: false,
            problem: None,
            tailscale_installed: false,
            last_checked_at: None,
        },
        account: SnapshotAccount {
            username: None,
            has_key: false,
            offer_backup: false,
        },
        install: SnapshotInstall {
            dir: String::new(),
            default_dir: String::new(),
            installed: false,
            missing_files: 0,
            verified: false,
            dlls_ok: false,
            free_bytes: None,
            required_bytes: 0,
            total_bytes: 0,
            vc_runtime_missing: Vec::new(),
            direct_x_missing: None,
            content_available: false,
        },
        task: None,
        game: SnapshotGame {
            running: false,
            relay_port: None,
        },
        settings: default_settings(),
        app: SnapshotApp {
            version: version.to_owned(),
            packaged,
            update_ready: false,
        },
        status: None,
        status_unsupported: false,
        last_error: None,
        notice: None,
    }
}

/// The answer of every call whose row is not ported yet.
pub fn not_yet() -> ActionResult {
    ActionResult::Err(not_yet_error())
}

pub fn not_yet_error() -> LauncherError {
    LauncherError {
        code: ErrorCode::Unknown,
        detail: Some("not in the Tauri launcher yet".into()),
    }
}

pub struct Backend {
    snapshot: Mutex<Snapshot>,
    progress: Mutex<Option<(Channel<TaskProgress>, Throttle)>>,
    /// An invite link the launcher was started with or handed, until the page takes it
    invite_link: Mutex<Option<String>>,
}

/// Tells the page an invite link is waiting (`onInviteLink`).
pub const INVITE_LINK_EVENT: &str = "invite-link";

/// A poisoned lock only means a panic elsewhere while holding it; the data is still whole.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Backend {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot: Mutex::new(snapshot),
            progress: Mutex::new(None),
            invite_link: Mutex::new(None),
        }
    }

    /// Keeps an invite link for the page and tells it one is waiting.
    pub fn deliver_invite_link(&self, app: Option<&AppHandle>, link: Option<String>) {
        let Some(link) = link else { return };
        *lock(&self.invite_link) = Some(link);
        if let Some(app) = app {
            let _ = app.emit_to(MAIN, INVITE_LINK_EVENT, ());
        }
    }

    pub fn take_invite_link(&self) -> Option<String> {
        lock(&self.invite_link).take()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    /// Changes the snapshot and pushes the new one to the page.
    pub fn update(&self, app: &AppHandle, change: impl FnOnce(&mut Snapshot)) -> Snapshot {
        let snapshot = {
            let mut snapshot = lock(&self.snapshot);
            change(&mut snapshot);
            snapshot.clone()
        };
        let _ = app.emit_to(MAIN, SNAPSHOT_EVENT, &snapshot);
        snapshot
    }

    pub fn set_settings(&self, app: &AppHandle, patch: SettingsPatch) -> Settings {
        self.update(app, |snapshot| snapshot.settings.apply(patch)).settings
    }

    pub fn subscribe_progress(&self, channel: Channel<TaskProgress>) {
        *lock(&self.progress) = Some((channel, Throttle::default()));
    }

    /// Sends a progress value to the page, unless the last one went out too recently.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the downloader (L23) and verify (L24) report progress")
    )]
    pub fn push_progress(&self, progress: &TaskProgress) {
        if let Some((channel, throttle)) = lock(&self.progress).as_mut()
            && throttle.pass(Instant::now(), progress)
        {
            let _ = channel.send(progress.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::bridge::types::TaskKind;

    #[test]
    fn progress_reaches_the_subscribed_channel_throttled() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&sent);
        let channel = Channel::new(move |body| {
            lock(&sink).push(format!("{body:?}"));
            Ok(())
        });
        let backend = Backend::new(initial_snapshot("0.0.0", false));
        let mut progress = TaskProgress {
            kind: TaskKind::Download,
            paused: false,
            total_bytes: 10,
            done_bytes: 1,
            files_total: 1,
            files_done: 0,
            bytes_per_second: 0.0,
            eta_seconds: None,
            current_file: None,
        };
        backend.push_progress(&progress);
        assert!(lock(&sent).is_empty(), "nothing goes out before the page subscribes");
        backend.subscribe_progress(channel);
        backend.push_progress(&progress);
        progress.done_bytes = 2;
        backend.push_progress(&progress);
        assert_eq!(lock(&sent).len(), 1);
        progress.paused = true;
        backend.push_progress(&progress);
        assert_eq!(lock(&sent).len(), 2);
    }
}
