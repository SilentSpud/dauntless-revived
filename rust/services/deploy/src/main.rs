//! The `deploy` binary (`npm start`): `deploy [--env-file <file>]`. It listens on `BIND_HOST`
//! (loopback by default) and `PORT`, starts Ramsgate, runs the watchdog every 15 s, and on a stop
//! request stops the game processes it started before it exits.

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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let env_file = cli
        .env_file
        .or_else(|| Some(PathBuf::from(".env")).filter(|p| p.is_file()));
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
