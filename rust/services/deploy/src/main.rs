//! The `deploy` binary. `deploy [serve] [--env-file <file>]` (`npm start`) listens on `BIND_HOST`
//! (loopback by default) and `PORT`, starts Ramsgate, runs the watchdog every 15 s, and on a stop
//! request stops the game processes it started before it exits. The Unix kits' game launchers are
//! subcommands (`run-game`, `worker`, `ssh-game`, `generate-config`; see `kit.rs`), and the same
//! binary linked as `dr-run-game` or `dr-ssh-game` is that launcher, for `GAMESERVER_BINARY_PATH`.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use deploy::gameservers::Deploy;
use deploy::matchmaker::Matchmaker;
use deploy::process::RealPlatform;
use deploy::settings::Settings;
use service_kit::env::Env;

#[derive(Parser)]
#[command(name = "deploy", version, about = "Dauntless Revived deploy server")]
struct Cli {
    #[arg(long)]
    env_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Serves the matchmaking calls of the metagame on this machine (the default).
    Serve {
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    /// Runs one game server under Proton or Wine (Linux kits).
    #[command(disable_help_flag = true)]
    RunGame {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Runs one game server from an encoded argument list (a Linux worker's SSH forced command).
    Worker { payload: String },
    /// Starts one game server on the Linux worker over SSH (OpenBSD kit).
    #[command(disable_help_flag = true)]
    SshGame {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Writes the Unix kits' settings for every service.
    #[command(disable_help_flag = true)]
    GenerateConfig {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Writes a fresh metagame token-signing key pair to a private file as two `.env` lines (the
    /// Windows kit's installer).
    SigningKeys { out: PathBuf },
}

fn process_env() -> std::collections::HashMap<String, String> {
    std::env::vars().collect()
}

fn launcher(launch: Result<deploy::kit::Launch, String>, label: &str) -> ExitCode {
    let launch = match launch {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{label}: {error}");
            return ExitCode::from(127);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{label}: {error}");
            return ExitCode::from(127);
        }
    };
    let code = runtime.block_on(deploy::kit::run(launch, label));
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn run_game(args: &[String]) -> ExitCode {
    launcher(
        deploy::kit::build_wine_launch(args, &process_env()),
        "dauntless linux game launcher",
    )
}

fn ssh_game(args: &[String]) -> ExitCode {
    launcher(
        deploy::kit::build_ssh_launch(args, &process_env()).map(|(launch, _)| launch),
        "dauntless OpenBSD game launcher",
    )
}

fn worker(payload: &str) -> ExitCode {
    let mut env = process_env();
    let file = env
        .get("DR_WORKER_ENV")
        .cloned()
        .unwrap_or_else(|| "/etc/dauntless-revived-worker.env".into());
    deploy::kit::load_worker_env(std::path::Path::new(&file), &mut env);
    let launch =
        deploy::kit::decode_worker_payload(payload).and_then(|args| deploy::kit::build_wine_launch(&args, &env));
    launcher(launch, "dauntless linux worker")
}

fn main() -> ExitCode {
    // Linked as the game launcher: every argument is the game's
    let args: Vec<String> = std::env::args().collect();
    let invoked = args
        .first()
        .and_then(|a| {
            std::path::Path::new(a)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    match invoked.as_str() {
        "dr-run-game" => return run_game(&args[1..]),
        "dr-ssh-game" => return ssh_game(&args[1..]),
        _ => {}
    }
    let cli = Cli::parse();
    let env_file = match cli.command {
        Some(Command::RunGame { args }) => return run_game(&args),
        Some(Command::SshGame { args }) => return ssh_game(&args),
        Some(Command::Worker { payload }) => return worker(&payload),
        Some(Command::GenerateConfig { args }) => {
            return match deploy::kitconfig::generate(&args) {
                Ok(line) => {
                    println!("{line}");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("generate-config: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        Some(Command::SigningKeys { out }) => {
            return match deploy::kitconfig::signing_keys_file(&out) {
                Ok(()) => {
                    println!("signing keys written");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("signing-keys: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        Some(Command::Serve { env_file }) => env_file.or(cli.env_file),
        None => cli.env_file,
    };
    let env_file = env_file.or_else(|| Some(PathBuf::from(".env")).filter(|p| p.is_file()));
    let env = match Env::load(env_file.as_deref()) {
        Ok(env) => env,
        Err(error) => {
            eprintln!("Configuration: {error}");
            return ExitCode::FAILURE;
        }
    };
    let level = match service_kit::log::parse_level(env.raw("LOG_LEVEL")) {
        Ok(level) => level,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    service_kit::log::init(level, env.raw("NODE_ENV") != Some("production"));
    if env.raw("SERVER_ROLE") == Some("database-only") {
        eprintln!("Deploy service disabled on database-only node");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(Settings::from_env(&env)))
}

async fn run(settings: Settings) -> ExitCode {
    let port = settings
        .number("PORT")
        .filter(|p| p.fract() == 0.0 && (0.0..=65535.0).contains(p))
        .map(|p| p as u16);
    let host = settings.non_empty("BIND_HOST").unwrap_or_else(|| "127.0.0.1".into());
    let (Some(port), Ok(ip)) = (port, host.parse::<IpAddr>()) else {
        service_kit::fatal!(
            "Could not listen on {host}:{:?}: not an address and port",
            settings.get("PORT")
        );
        return ExitCode::FAILURE;
    };
    let deploy = match Deploy::new(settings, Arc::new(RealPlatform)) {
        Ok(deploy) => deploy,
        Err(error) => {
            service_kit::fatal!("Configuration: {error}");
            return ExitCode::FAILURE;
        }
    };
    deploy.cpu_admission.start_sampling();
    let listener = match tokio::net::TcpListener::bind(SocketAddr::new(ip, port)).await {
        Ok(listener) => listener,
        Err(error) => {
            service_kit::fatal!("Could not listen on {host}:{port}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let matchmaker = Matchmaker::new(deploy.clone());
    let app = deploy::app::router(matchmaker).into_make_service_with_connect_info::<SocketAddr>();
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(async move {
        let mut stopping = stopping;
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopping.wait_for(|s| *s).await;
            })
            .await;
    });

    let startup = {
        let deploy = deploy.clone();
        tokio::spawn(async move { deploy.startup_and_report_failure().await })
    };
    let watchdog = {
        let deploy = deploy.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(15));
            every.tick().await;
            loop {
                every.tick().await;
                deploy.run_watchdog().await;
            }
        })
    };
    tracing::info!("Dauntless Revived deploy server on port {port}");
    tracing::info!(
        "Ramsgate and Dojo liveness check before handing them out: {}",
        if deploy.persistent_world_liveness() {
            "on"
        } else {
            "off (PERSISTENT_WORLD_LIVENESS=0)"
        }
    );
    tracing::info!("Clear Skies, Slayer.");

    service_kit::serve::stop_requested().await;
    tracing::info!("stopping: ending the game servers this deploy server started");
    watchdog.abort();
    let _ = stop.send(true);
    deploy.shutdown().await;
    let _ = startup.await;
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
    if deploy.startup_failed() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
