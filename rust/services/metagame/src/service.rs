//! The metagame server itself, as `metagame serve` runs it and as the launcher runs it for offline
//! play (`launcher --service metagame`, D12).

use std::path::Path;
use std::process::ExitCode;

use service_kit::env::Env;

use crate::config::Config;

/// Serves until a stop request (`shutdown` on stdin, Ctrl+C / Ctrl+Break or SIGTERM), with the settings
/// in `env_file` (the environment wins over it).
pub fn serve(env_file: Option<&Path>) -> ExitCode {
    let env = match Env::load(env_file) {
        Ok(env) => env,
        Err(error) => {
            eprintln!("metagame: {error}");
            return ExitCode::FAILURE;
        }
    };
    let level = match service_kit::log::parse_level(env.get("LOG_LEVEL")) {
        Ok(level) => level,
        Err(error) => {
            eprintln!("metagame: {error}");
            return ExitCode::FAILURE;
        }
    };
    service_kit::log::init(level, env.get("NODE_ENV") != Some("production"));
    let fail = |message: String| {
        service_kit::fatal!("{message}");
        ExitCode::FAILURE
    };
    let config = match Config::from_env(&env) {
        Ok(config) => config,
        Err(error) => return fail(error),
    };
    // A bad folder, file or season stops the start here, not at a player's first request.
    let progression = match crate::progression_config::from_env(&env) {
        Ok(progression) => progression,
        Err(error) => return fail(format!("The progression config could not be loaded: {error}")),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => return fail(error.to_string()),
    };
    match runtime.block_on(crate::server::run(config, progression)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}
