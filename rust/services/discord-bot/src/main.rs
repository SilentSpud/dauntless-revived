//! The `discord-bot` binary (`npm start`): `discord-bot [--env-file <file>]`, `.env` in the working
//! folder by default as `node --env-file=.env` read it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clap::Parser;
use discord_bot::bot::{Bot, Settings};
use discord_bot::commands::Rest;
use discord_bot::counter::Counters;
use discord_bot::discord::DiscordRest;
use discord_bot::keys::{Keys, generate_code, load_state, save_state};
use discord_bot::metagame::{Http, MetagameBackend};
use service_kit::env::Env;

#[derive(Parser)]
#[command(name = "discord-bot", version, about = "Dauntless Revived key bot")]
struct Cli {
    #[arg(long)]
    env_file: Option<PathBuf>,
}

/// Gateway member intents: GUILD_MEMBERS, or its limited form.
const MEMBER_INTENT_FLAGS: u64 = (1 << 14) | (1 << 15);

fn main() -> ExitCode {
    // Two rustls providers are compiled in (serenity's HTTP client brings ring): name the default
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    let file = cli
        .env_file
        .or_else(|| Some(PathBuf::from(".env")).filter(|p| p.is_file()));
    let env = match Env::load(file.as_deref()) {
        Ok(env) => env,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(env)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(env: Env) -> Result<ExitCode, String> {
    let (Some(token), Some(admin_key)) = (env.get("DISCORD_BOT_TOKEN"), env.get("METAGAME_ADMIN_KEY")) else {
        return Err("Configure DISCORD_BOT_TOKEN and METAGAME_ADMIN_KEY".into());
    };
    let state_file =
        std::path::absolute(env.get("KEY_STATE_FILE").unwrap_or("./data/keys.json")).map_err(|e| e.to_string())?;
    let _instance = discord_bot::instance::acquire_instance(&state_file)?;
    let invite_config = discord_bot::invite::load_invite_config(env.get("SERVER_CONFIG_FILE"))?;
    let metagame_url = env.get("METAGAME_URL").unwrap_or("http://127.0.0.1:61000").to_owned();
    let backend = MetagameBackend::new(&metagame_url, admin_key, Http::default())?;
    let save_file = Arc::new(state_file.clone());
    let keys = Arc::new(Keys::new(
        load_state(&state_file)?,
        Arc::new(move |state| {
            let file = save_file.clone();
            Box::pin(async move { save_state(&file, &state).await })
        }),
        Arc::new(backend),
        Arc::new(generate_code),
    ));
    keys.migrate_links().await.map_err(|e| e.message)?;
    println!("Key state and account links loaded");

    let rest = Arc::new(DiscordRest::new(token));
    let app = rest.get("/oauth2/applications/@me").await.map_err(|e| e.message)?;
    println!("Discord application authenticated");
    let member_intent = app["flags"].as_u64().unwrap_or(0) & MEMBER_INTENT_FLAGS != 0;
    let counter_file = env.get("COUNTER_STATE_FILE").map(PathBuf::from).unwrap_or_else(|| {
        state_file
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("counters.json")
    });
    let counters = Arc::new(Counters::from_file(counter_file)?);
    if !member_intent {
        eprintln!(
            "Counter unavailable: enable Server Members Intent in Discord Developer Portal. Key commands remain online."
        );
        let rest = rest.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                match rest.get("/oauth2/applications/@me").await {
                    // Restart to connect with the intent
                    Ok(app) if app["flags"].as_u64().unwrap_or(0) & MEMBER_INTENT_FLAGS != 0 => std::process::exit(0),
                    Ok(_) => {}
                    Err(_) => eprintln!("Could not recheck Server Members Intent"),
                }
            }
        });
    }
    let settings = Settings {
        guild: env.get("DISCORD_GUILD_ID").map(str::to_owned),
        metagame_url,
        recovery_secret: env.get("KEY_RECOVERY_SERVICE_SECRET").map(str::to_owned),
    };
    let bot = Bot::new(keys, counters.clone(), invite_config, settings, rest, member_intent);
    let last_ready = bot.last_ready.clone();
    let mut client = serenity::Client::builder(token, Bot::intents(member_intent))
        .event_handler(bot)
        .await
        .map_err(|e| e.to_string())?;

    // A live process is not necessarily a connected bot: let the supervisor recover a stalled start or
    // a gateway that never reconnects, without touching key state
    let shards = client.shard_manager.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let connected = shards
                .runners
                .lock()
                .await
                .values()
                .any(|runner| runner.stage == serenity::gateway::ConnectionStage::Connected);
            let now = chrono::Utc::now().timestamp_millis();
            if connected {
                last_ready.store(now, Ordering::SeqCst);
            } else if now - last_ready.load(Ordering::SeqCst) > 120_000 {
                eprintln!("Key bot connection unavailable for 120 seconds; restarting");
                std::process::exit(1);
            }
        }
    });
    let shards = client.shard_manager.clone();
    tokio::spawn(async move {
        service_kit::serve::stop_requested().await;
        shards.shutdown_all().await;
    });
    if client.start().await.is_err() {
        eprintln!("Discord login failed");
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}
