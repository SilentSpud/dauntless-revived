//! The `metagame` binary: `metagame serve --env-file <file>` runs the server, `metagame migrate` brings
//! its database to the latest migration and exits (the kits' install step), `metagame db …` holds the
//! database tools for the server kits (M19), `metagame offline-save …` the offline save tools (X1).

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
    /// Creates the database the settings name, or adopts it, and applies every migration, then exits:
    /// what a start does before it serves, for an install that registers keys and invites next.
    Migrate {
        /// The settings file (`.env` format); variables set in the environment win over it.
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    /// Database tools for the server kits.
    Db {
        #[command(subcommand)]
        command: DbCommand,
    },
    /// Offline saves: the account key is read from stdin, never from the arguments.
    OfflineSave {
        #[command(subcommand)]
        command: OfflineSaveCommand,
    },
    /// Adds a one-use invite code and prints its launcher invite (Unix kits:
    /// `--config <dir>` `--name <server name>`).
    #[command(disable_help_flag = true)]
    Invite {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum OfflineSaveCommand {
    /// Copies a stopped offline database into a save's database; prints the account and the copy's new
    /// key as JSON (`userId`, `username`, `key`).
    Export { source: PathBuf, destination: PathBuf },
    /// Brings a private copy of a save's database to the current schema and checks it; prints the
    /// account as JSON.
    Import {
        database: PathBuf,
        /// The account the save's metadata names (both or neither)
        #[arg(long, requires = "username")]
        user_id: Option<String>,
        #[arg(long, requires = "user_id")]
        username: Option<String>,
    },
}

fn offline_save(command: OfflineSaveCommand) -> ExitCode {
    use metagame::offline_save::{self, Account};
    let mut credential = String::new();
    if std::io::stdin().read_line(&mut credential).is_err() {
        eprintln!("metagame offline-save: the account key is read from stdin");
        return ExitCode::FAILURE;
    }
    let credential = credential.trim();
    let outcome = match command {
        OfflineSaveCommand::Export { source, destination } => offline_save::export(&source, &destination, credential)
            .map(|saved| {
                serde_json::json!({
                    "userId": saved.account.user_id,
                    "username": saved.account.username,
                    "key": saved.key,
                })
            }),
        OfflineSaveCommand::Import {
            database,
            user_id,
            username,
        } => {
            let declared = user_id
                .zip(username)
                .map(|(user_id, username)| Account { user_id, username });
            offline_save::import(&database, credential, declared.as_ref())
                .map(|account| serde_json::json!({ "userId": account.user_id, "username": account.username }))
        }
    };
    match outcome {
        Ok(answer) => {
            println!("{answer}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("metagame offline-save: {error}");
            ExitCode::FAILURE
        }
    }
}

fn migrate(env_file: Option<PathBuf>) -> ExitCode {
    let config = match Env::load(env_file.as_deref())
        .map_err(|e| e.to_string())
        .and_then(|env| Config::from_env(&env))
    {
        Ok(config) => config,
        Err(error) => {
            eprintln!("metagame migrate: {error}");
            return ExitCode::FAILURE;
        }
    };
    match store::Store::open(&config.db_filename, &store::Options::default(), &metagame::db::SCHEMA) {
        Ok(_) => {
            println!("{} is at the latest migration", config.db_filename.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "metagame migrate: the database {} could not be opened: {error}",
                config.db_filename.display()
            );
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Serve { env_file } => metagame::service::serve(env_file.as_deref()),
        Command::Migrate { env_file } => migrate(env_file),
        Command::OfflineSave { command } => offline_save(command),
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
