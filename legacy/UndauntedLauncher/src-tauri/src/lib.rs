//! The Dauntless Revived Launcher on Tauri 2 (MIGRATION_PLAN.md Part A). The page is the same
//! TypeScript page the Electron launcher shows; everything the Electron main process did moves here.

mod bridge;
mod core;
mod net;
mod platform;
mod window;

use bridge::backend::{Backend, initial_snapshot};
use bridge::commands;

use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Manager};

use platform::invite_link::{self, Claim};

/// The running app, for what arrives from a second launcher.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// A second launcher's arguments: bring the window forward and take its invite link.
fn on_second_launcher(args: Vec<String>) {
    let Some(app) = APP.get() else { return };
    if let Some(window) = app.get_webview_window(window::MAIN) {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    app.state::<Backend>()
        .deliver_invite_link(Some(app), invite_link::invite_from_args(&args));
}

/// Builds and runs the app until the last window closes; hands over to the launcher already running.
pub fn run() -> tauri::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let on_args: invite_link::OnArgs = Arc::new(|args| Box::pin(async move { on_second_launcher(args) }));
    if tauri::async_runtime::block_on(invite_link::claim(&invite_link::channel_name(), &args, on_args)) == Claim::Handed
    {
        return Ok(());
    }
    let context = tauri::generate_context!();
    let version = context.package_info().version.to_string();
    let backend = Backend::new(initial_snapshot(&version, !cfg!(dev)));
    backend.deliver_invite_link(None, invite_link::invite_from_args(&args));
    tauri::Builder::default()
        // Called from Rust only: the page has no dialog permission (L36)
        .plugin(tauri_plugin_dialog::init())
        .manage(window::CloseGuard::default())
        .manage(backend)
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::check_invite,
            commands::submit_invite,
            commands::retry_connect,
            commands::forget_server,
            commands::switch_server,
            commands::remove_saved_server,
            commands::register,
            commands::use_existing_key,
            commands::import_key_file,
            commands::save_key_backup,
            commands::dismiss_backup_offer,
            commands::choose_install_dir,
            commands::use_existing_game_folder,
            commands::use_existing_game_path,
            commands::start_install,
            commands::pause_task,
            commands::resume_task,
            commands::cancel_task,
            commands::repair,
            commands::play,
            commands::start_offline,
            commands::stop_offline,
            commands::offline_status,
            commands::set_settings,
            commands::set_status_polling,
            commands::refresh_status,
            commands::get_news,
            commands::get_branding,
            commands::open_external,
            commands::open_game_folder,
            commands::logout,
            commands::install_update,
            commands::take_invite_link,
            commands::dismiss_notice,
            commands::dismiss_error,
            commands::offline_export_save,
            commands::offline_import_save,
            commands::subscribe_progress,
        ])
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            if !cfg!(dev)
                && let Ok(exe) = std::env::current_exe()
            {
                invite_link::register_scheme(&exe);
            }
            window::create(app.handle())?;
            Ok(())
        })
        .run(context)
}
