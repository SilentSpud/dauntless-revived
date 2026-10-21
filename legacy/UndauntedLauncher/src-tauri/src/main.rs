// No console window behind the launcher in release builds on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = launcher::run() {
        eprintln!("Dauntless Revived Launcher could not start: {error}");
        std::process::exit(1);
    }
}
