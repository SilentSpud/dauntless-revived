//! The `metagame` binary: `metagame serve --env-file <file>` runs the server, `metagame db …` holds
//! the database tools for the server kits (M19).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use metagame::config::Config;
use metagame::dbtools::{self, DbCommand};
use service_kit::env::Env;

#[derive(Parser)]
#[command(name = "metagame", version, about = "Dauntless Revived metagame")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs the metagame until `shutdown` on stdin, Ctrl+C / Ctrl+Break or SIGTERM.
    Serve {
        /// The settings file (`.env` format); variables set in the environment win over it.
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    /// Database tools for the server kits.
    Db {
        #[command(subcommand)]
        command: DbCommand,
    },
    /// Adds a one-use invite code and prints its launcher invite (Unix kits:
    /// `--config <dir>` `--name <server name>`).
    #[command(disable_help_flag = true)]
    Invite {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn serve(env_file: Option<PathBuf>) -> ExitCode {
    let env = match Env::load(env_file.as_deref()) {
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
    let progression = match metagame::progression_config::from_env(&env) {
        Ok(progression) => progression,
        Err(error) => return fail(format!("The progression config could not be loaded: {error}")),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => return fail(error.to_string()),
    };
    match runtime.block_on(metagame::server::run(config, progression)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Serve { env_file } => serve(env_file),
        Command::Invite { args } => match metagame::kitinvite::run(&args) {
            Ok(invite) => {
                println!("{invite}");
                ExitCode::SUCCESS
            }
            Err(reason) => {
                eprintln!("metagame invite: {reason}");
                ExitCode::FAILURE
            }
        },
        Command::Db { command } => match dbtools::run(&command) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(reason) => {
                eprintln!("metagame db: {reason}");
                ExitCode::FAILURE
            }
        },
    }
}
