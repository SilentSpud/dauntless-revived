//! The deploy server itself, as `deploy serve` runs it and as the launcher runs it for offline play
//! (`launcher --service deploy`, D12).

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use service_kit::env::Env;

use crate::gameservers::Deploy;
use crate::matchmaker::Matchmaker;
use crate::process::RealPlatform;
use crate::settings::Settings;

/// Serves until a stop request (`shutdown` on stdin, Ctrl+C / Ctrl+Break or SIGTERM), with the settings
/// in `env_file` (the environment wins over it).
pub fn serve(env_file: Option<&Path>) -> ExitCode {
    let env = match Env::load(env_file) {
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
    let app = crate::app::router(matchmaker).into_make_service_with_connect_info::<SocketAddr>();
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
