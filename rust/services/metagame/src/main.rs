//! The `metagame` binary. So far only `metagame db …` (M19); serving comes with the domain port.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use metagame::dbtools::{self, DbCommand};

#[derive(Parser)]
#[command(name = "metagame", version, about = "Dauntless Revived metagame")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Database tools for the server kits.
    Db {
        #[command(subcommand)]
        command: DbCommand,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
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
