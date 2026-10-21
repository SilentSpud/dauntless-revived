//! The Dauntless Revived Launcher on Tauri 2 (MIGRATION_PLAN.md Part A). The page is the same
//! TypeScript page the Electron launcher shows; everything the Electron main process did moves here.

mod window;

/// Builds and runs the app until the last window closes.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .setup(|app| {
            window::create(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
}
