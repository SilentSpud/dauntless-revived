//! The Dauntless Revived Launcher on Tauri 2 (MIGRATION_PLAN.md Part A). The page is the same
//! TypeScript page the Electron launcher shows; everything the Electron main process did moves here.

mod bridge;
mod controller;
mod core;
mod net;
mod platform;
mod window;

use bridge::backend::Backend;
use bridge::commands;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Manager, RunEvent};

use controller::platform::TauriPlatform;
use controller::{Config, Controller, ControllerHandle};
use platform::paths::Paths;

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
    platform::log::install();
    let paths = Paths::from_system().ok_or_else(|| tauri::Error::UnknownPath)?;
    platform::log::log_to_file(paths.logs_dir());
    let context = tauri::generate_context!();
    let version = context.package_info().version.to_string();
    platform::log::info(&format!("Dauntless Revived Launcher {version} starting"));
    let encryptors = tauri::async_runtime::block_on(platform::keystore::os::encryptors(&paths.user_data));
    let backend = Backend::default();
    backend.deliver_invite_link(None, invite_link::invite_from_args(&args));
    tauri::Builder::default()
        // Called from Rust only: the page has no dialog permission (L36)
        .plugin(tauri_plugin_dialog::init())
        .manage(window::CloseGuard::default())
        .manage(platform::art::ArtStore::default())
        // The host's art, from the controller's cache (L37); nothing from the network
        .register_asynchronous_uri_scheme_protocol(platform::art::SCHEME, |context, request, responder| {
            let response = context
                .app_handle()
                .state::<platform::art::ArtStore>()
                .respond(request.uri());
            responder.respond(response);
        })
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
        .setup(move |app| {
            let _ = APP.set(app.handle().clone());
            if !cfg!(dev)
                && let Ok(exe) = std::env::current_exe()
            {
                invite_link::register_scheme(&exe);
            }
            let controller = make_controller(app.handle(), paths, version, encryptors)?;
            let guard = app.state::<window::CloseGuard>();
            let (reason, kept) = (Arc::downgrade(&controller), Arc::downgrade(&controller));
            guard.set(
                move || reason.upgrade().and_then(|controller| controller.keep_open()),
                move || {
                    if let Some(controller) = kept.upgrade() {
                        controller.set_notice(Some(bridge::types::NoticeCode::KeepOpen));
                    }
                },
            );
            app.manage(controller.clone());
            window::create(app.handle())?;
            tauri::async_runtime::spawn(async move { controller.init().await });
            Ok(())
        })
        .build(context)?
        .run(|app, event| {
            // As the Electron launcher on window-all-closed: stop the relay and wait for the background work
            static STOPPED: AtomicBool = AtomicBool::new(false);
            if let RunEvent::ExitRequested { api, .. } = event
                && !STOPPED.swap(true, Ordering::SeqCst)
            {
                api.prevent_exit();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    app.state::<ControllerHandle>().inner().clone().shutdown().await;
                    app.exit(0);
                });
            }
        });
    Ok(())
}

/// The controller with this PC's folders, the compiled-in manifest and the app's resources.
fn make_controller(
    app: &AppHandle,
    paths: Paths,
    version: String,
    (encryptor, legacy_encryptor): (
        Arc<dyn platform::keystore::Encryptor>,
        Option<Arc<dyn platform::keystore::Encryptor>>,
    ),
) -> tauri::Result<ControllerHandle> {
    use crate::core::manifest::GAME_MANIFEST;
    use crate::core::pins::{EXE_RELATIVE_PATH, PINNED_EXE_SHA256};

    let resources_dir = if cfg!(dev) {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"))
    } else {
        app.path().resource_dir()?
    };
    let manifest = (!GAME_MANIFEST.files.is_empty()).then(|| GAME_MANIFEST.clone());
    match &manifest {
        Some(manifest) => platform::log::info(&format!(
            "game manifest {} files, fingerprint {}",
            manifest.files.len(),
            crate::core::manifest::manifest_fingerprint(manifest)
        )),
        None => platform::log::error("compiled-in game manifest is not usable"),
    }
    let relay_port = net::relay::relay_port_override(std::env::var("DAUNTLESS_REVIVED_RELAY_PORT").ok().as_deref())
        .unwrap_or(net::relay::DEFAULT_RELAY_PORT);
    let config = Config {
        user_data: paths.user_data.clone(),
        documents: Some(paths.documents.clone()),
        resources_dir,
        default_install_dir: paths.default_install_dir.clone(),
        host: platform::settings::Platform::current(),
        app_version: version,
        packaged: !cfg!(dev),
        language: core::i18n::os_language(),
        system_locale: sys_locale::get_locale().unwrap_or_else(|| "en-US".into()),
        manifest,
        game_config_dir: platform::game_config::default_config_dir(),
        relay_port,
        exe_pin: (EXE_RELATIVE_PATH.into(), PINNED_EXE_SHA256.into()),
        status_poll: std::time::Duration::from_millis(core::pins::STATUS_POLL_MS),
        encryptor,
        legacy_encryptor,
        spawn: None,
    };
    let platform = TauriPlatform {
        app: app.clone(),
        user_data: paths.user_data,
        documents: paths.documents,
        home: paths.home,
    };
    Ok(Controller::new(Arc::new(platform), config))
}
