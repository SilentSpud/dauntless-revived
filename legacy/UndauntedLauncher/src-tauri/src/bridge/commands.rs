//! One command per `LauncherApi` method (row L7), named after today's IPC channel without `dr:` (in
//! snake_case). Each checks its arguments again, whatever the isolation hook let through: the hook
//! guards the page, these guard the backend (`ipc-validate.ts`).

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::backend::{Backend, not_yet};
use super::spec::{GAME_PATH_MAX, INVITE_MAX, KEY_TEXT_MAX, USERNAME_MAX, fits, is_server_id};
use super::types::{
    ActionResult, Branding, ErrorCode, ExternalTarget, InviteCheck, LauncherError, NewsItem, OfflinePhase,
    OfflineStatus, RegisterOutcome, Settings, SettingsPatch, Snapshot, TaskProgress,
};
use crate::core::invite::{InviteError, parse_invite};
use crate::core::username::{UsernameCheck, check_username, extract_account_key};

type Backed<'a> = State<'a, Backend>;

#[tauri::command]
pub fn get_snapshot(backend: Backed<'_>) -> Snapshot {
    backend.snapshot()
}

fn invite_format_error(error: InviteError) -> LauncherError {
    LauncherError {
        code: ErrorCode::InviteInvalidFormat,
        detail: Some(error.as_str().to_owned()),
    }
}

/// Whether an invite can be joined. A public invite whose certificate differs from the one this PC's
/// key for that server belongs to says so; the key store (L27) has no keys yet, so none differs.
#[tauri::command]
pub fn check_invite(text: String) -> InviteCheck {
    if !fits(&text, INVITE_MAX) {
        return InviteCheck::Err(LauncherError::new(ErrorCode::InviteInvalidFormat));
    }
    match parse_invite(&text) {
        Ok(_) => InviteCheck::Ok {
            certificate_changed: false,
            previous_fingerprint: None,
        },
        Err(error) => InviteCheck::Err(invite_format_error(error)),
    }
}

#[tauri::command]
pub fn submit_invite(text: String, accept_new_certificate: bool) -> ActionResult {
    let _ = accept_new_certificate;
    if !fits(&text, INVITE_MAX) {
        return ActionResult::err(ErrorCode::InviteInvalidFormat);
    }
    match parse_invite(&text) {
        Ok(_) => not_yet(),
        Err(error) => ActionResult::Err(invite_format_error(error)),
    }
}

#[tauri::command]
pub fn retry_connect() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn forget_server() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn switch_server(id: String) -> ActionResult {
    if !is_server_id(&id) {
        return ActionResult::err(ErrorCode::InviteInvalidFormat);
    }
    not_yet()
}

#[tauri::command]
pub fn remove_saved_server(id: String) -> ActionResult {
    if !is_server_id(&id) {
        return ActionResult::err(ErrorCode::InviteInvalidFormat);
    }
    not_yet()
}

#[tauri::command]
pub fn register(username: String) -> RegisterOutcome {
    if !fits(&username, USERNAME_MAX) {
        return RegisterOutcome::Err(LauncherError::new(ErrorCode::UsernameInvalid));
    }
    let check = check_username(&username);
    if check != UsernameCheck::Ok {
        return RegisterOutcome::Err(LauncherError {
            code: ErrorCode::UsernameInvalid,
            detail: Some(check.as_str().to_owned()),
        });
    }
    // No server joined yet (L38)
    RegisterOutcome::Err(LauncherError::new(ErrorCode::ServerUnreachable))
}

#[tauri::command]
pub fn use_existing_key(text: String) -> ActionResult {
    if !fits(&text, KEY_TEXT_MAX) || extract_account_key(&text).is_none() {
        return ActionResult::err(ErrorCode::KeyInvalid);
    }
    not_yet()
}

#[tauri::command]
pub fn import_key_file() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn save_key_backup() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn dismiss_backup_offer(app: AppHandle, backend: Backed<'_>) {
    backend.update(&app, |snapshot| snapshot.account.offer_backup = false);
}

#[tauri::command]
pub fn choose_install_dir() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn use_existing_game_folder() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn use_existing_game_path(game_path: String) -> ActionResult {
    if !fits(&game_path, GAME_PATH_MAX) {
        return ActionResult::err(ErrorCode::FolderInvalid);
    }
    not_yet()
}

#[tauri::command]
pub fn start_install() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn pause_task() {}

#[tauri::command]
pub fn resume_task() {}

#[tauri::command]
pub fn cancel_task() {}

#[tauri::command]
pub fn repair() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn play() -> ActionResult {
    not_yet()
}

/// Offline play is off in 0.2.0 (row L42, D1); it returns with the Rust services (X2).
#[tauri::command]
pub fn start_offline(username: Option<String>) -> ActionResult {
    if username.as_deref().is_some_and(|name| !fits(name, USERNAME_MAX)) {
        return ActionResult::err(ErrorCode::UsernameInvalid);
    }
    ActionResult::err(ErrorCode::OfflineUnavailable)
}

#[tauri::command]
pub fn stop_offline() -> ActionResult {
    ActionResult::Ok
}

#[tauri::command]
pub fn offline_status() -> OfflineStatus {
    OfflineStatus {
        phase: OfflinePhase::Stopped,
        progress: None,
        error: None,
    }
}

/// An unknown field or a wrong value refuses the whole patch, which then changes nothing, as today.
#[tauri::command]
pub fn set_settings(patch: Value, app: AppHandle, backend: Backed<'_>) -> Settings {
    let patch = serde_json::from_value::<SettingsPatch>(patch).unwrap_or_default();
    backend.set_settings(&app, patch)
}

#[tauri::command]
pub fn set_status_polling(on: bool) {
    let _ = on;
}

#[tauri::command]
pub fn refresh_status() {}

#[tauri::command]
pub fn get_news() -> Vec<NewsItem> {
    Vec::new()
}

#[tauri::command]
pub fn get_branding() -> Branding {
    Branding {
        backgrounds: Vec::new(),
        accent: None,
    }
}

#[tauri::command]
pub fn open_external(target: Value) -> ActionResult {
    match serde_json::from_value::<ExternalTarget>(target) {
        Ok(_) => not_yet(),
        Err(_) => ActionResult::err(ErrorCode::Unknown),
    }
}

#[tauri::command]
pub fn open_game_folder() {}

#[tauri::command]
pub fn logout() -> ActionResult {
    not_yet()
}

#[tauri::command]
pub fn install_update() {}

#[tauri::command]
pub fn take_invite_link(backend: Backed<'_>) -> Option<String> {
    backend.take_invite_link()
}

#[tauri::command]
pub fn dismiss_notice(app: AppHandle, backend: Backed<'_>) {
    backend.update(&app, |snapshot| snapshot.notice = None);
}

#[tauri::command]
pub fn dismiss_error(app: AppHandle, backend: Backed<'_>) {
    backend.update(&app, |snapshot| snapshot.last_error = None);
}

#[tauri::command]
pub fn offline_export_save() -> ActionResult {
    ActionResult::err(ErrorCode::OfflineUnavailable)
}

#[tauri::command]
pub fn offline_import_save() -> ActionResult {
    ActionResult::err(ErrorCode::OfflineUnavailable)
}

#[tauri::command]
pub fn subscribe_progress(on_progress: Channel<TaskProgress>, backend: Backed<'_>) {
    backend.subscribe_progress(on_progress);
}
