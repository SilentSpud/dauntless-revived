//! The metagame as a running server (`app.ts`, `server.ts`; MIGRATION_PLAN.md M5, X7): the app-wide
//! middleware in Node's order, the routes, start-up and shutdown.
//!
//! App-wide, outermost first, as `app.ts` uses them: a panicking handler becomes Express's 500 page;
//! backend health; the ban check (every route but ServerStatus, so a banned launcher can still read
//! why); development logins refuse proxied requests; JSON and form bodies; the request log; then the
//! routes, and an "Unstubbed route" 404 for the rest.

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use service_kit::body::{BodyConfig, BodyError, error_page, parse_bodies};
use service_kit::json::Json;
use service_kit::serve::{ConnectionInfo, serve, stop_requested};
use tower::ServiceBuilder;

use crate::app::{App, AppState, AuthConfig};
use crate::auth::check_player_access;
use crate::config::Config;
use crate::crypto::TokenKeys;
use crate::health::{Health, track};
use crate::limits::Limits;
use crate::origin::{Origin, OwnAddresses, is_proxied};
use crate::progression_config::ProgressionConfig;
use crate::redact::redact;
use crate::storeimages::StoreImages;

/// The account routes whose bad JSON gets `{"error": "bad_request"}` instead of Express's page.
const JSON_ERROR_ROUTES: [&str; 7] = [
    "/undaunted/api/register",
    "/undaunted/api/createinvite",
    "/undaunted/api/renameuser",
    "/undaunted/api/partyinvite",
    "/undaunted/api/friends",
    "/undaunted/api/guildinvite",
    "/undaunted/api/disbandguild",
];

fn is_server_status(path: &str) -> bool {
    let path = path.strip_suffix('/').unwrap_or(path);
    path.eq_ignore_ascii_case("/undaunted/api/ServerStatus")
}

async fn ban_check(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if is_server_status(request.uri().path()) {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    match check_player_access(&state, &parts, None).await {
        Ok(Some(refusal)) => refusal,
        Ok(None) => next.run(Request::from_parts(parts, body)).await,
        Err(error) => error.into_response(),
    }
}

async fn refuse_proxied_dev_logins(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if state.auth.dev_logins() && is_proxied(request.headers()) {
        tracing::warn!(
            "Refusing proxied {} request: AUTH_MODE=NONE does not serve proxied requests",
            request.method()
        );
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

/// One line per request; `gs=1` marks a game server's call. The path never carries credentials.
async fn request_log(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if let Some(ConnectInfo(connection)) = request.extensions().get::<ConnectInfo<ConnectionInfo>>().copied() {
        let origin = Origin {
            connection,
            headers: request.headers(),
        };
        let gs = u8::from(request.headers().contains_key("x-undaunted-gameserver-apikey"));
        tracing::info!(
            "{} {} gs={gs}{}",
            request.method(),
            redact(request.uri().path()),
            origin.describe(&state.origin)
        );
    }
    next.run(request).await
}

fn unstubbed(request: &Request) -> Response {
    crate::routes::note_unhandled_progression(request.method(), request.uri(), request.headers());
    tracing::warn!("Unstubbed route {} {}", request.method(), request.uri().path());
    StatusCode::NOT_FOUND.into_response()
}

fn body_config() -> BodyConfig {
    BodyConfig {
        on_error: Arc::new(|parts, error| {
            let path = parts.uri.path().trim_end_matches('/').to_ascii_lowercase();
            (error == BodyError::Malformed && JSON_ERROR_ROUTES.contains(&path.as_str())).then(|| {
                let reply = json!({"error": "bad_request", "message": "The request body is not valid JSON."});
                (StatusCode::BAD_REQUEST, Json(reply)).into_response()
            })
        }),
        ..BodyConfig::default()
    }
}

/// The whole app for `state`, ready to serve (tests call it directly).
pub fn app(state: AppState, log_requests: bool) -> axum::Router {
    let routes = crate::routes::all(&state).into_service(state.clone(), unstubbed);
    let log = ServiceBuilder::new().option_layer(log_requests.then(|| from_fn_with_state(state.clone(), request_log)));
    axum::Router::new().fallback_service(routes).layer(
        ServiceBuilder::new()
            .layer(tower_http::catch_panic::CatchPanicLayer::custom(|_| {
                error_page(StatusCode::INTERNAL_SERVER_ERROR)
            }))
            .layer(from_fn_with_state(state.health.clone(), track))
            .layer(from_fn_with_state(state.clone(), ban_check))
            .layer(from_fn_with_state(state.clone(), refuse_proxied_dev_logins))
            .layer(from_fn_with_state(body_config(), parse_bodies))
            .layer(log),
    )
}

/// Everything a running metagame holds, built from its settings.
pub struct Prepared {
    pub state: AppState,
    pub progression: ProgressionConfig,
}

/// Start-up up to listening: checks, the database (adopted or migrated), the keys left to register.
pub fn prepare(config: &Config, progression: ProgressionConfig) -> Result<Prepared, String> {
    crate::serverstatus::started();
    let tokens = TokenKeys::from_base64_pem(&config.signing_private_b64, &config.signing_public_b64)
        .map_err(|e| format!("AUTH_SIGNING_PRIVKEY_B64 / AUTH_SIGNING_PUBKEY_B64: {e}"))?;
    let store = store::Store::open(&config.db_filename, &store::Options::default(), &crate::db::SCHEMA)
        .map_err(|e| format!("The database {} could not be opened: {e}", config.db_filename.display()))?;
    let state = Arc::new(App {
        store,
        tokens,
        auth: AuthConfig {
            mode: config.auth_mode,
            production: config.production,
            account_key_recovery: config.account_key_recovery,
            signing_key_b64: config.signing_private_b64.clone(),
        },
        origin: config.origin.clone(),
        own_addresses: OwnAddresses::default(),
        features: config.features.clone(),
        registration_mode: RwLock::new(config.registration_mode.clone()),
        limits: Limits::default(),
        store_images: StoreImages::load(&config.store_images_dir),
        progression: config.progression.clone(),
        progression_config: Arc::new(progression.clone()),
        real_progression: config.real_progression.clone(),
        entitlement_defaults: config.entitlement_defaults.clone(),
        history: config.history,
        inventory: config.inventory,
        shop: config.store.clone(),
        store_sweeps: crate::store::Sweeps::default(),
        friend_requests: crate::friends::RecentRequests::default(),
        parties: crate::party::Parties::default(),
        matchmaking: crate::matchmaking::Matchmaking::new(config.matchmaking.clone()),
        guilds: config.guilds.clone(),
        escalation: config.escalation,
        slayer_links: config.slayer_links,
        guild_memory: crate::guild::Memory::default(),
        key_recovery_secret: config.key_recovery_secret.clone(),
        backup_root: config.backup_root.clone(),
        discord_key_state_file: config.discord_key_state_file.clone(),
        developer_accounts: config.developer_accounts.clone(),
        activity: crate::activity::Activity::default(),
        server_status: config.server_status.clone(),
        status_cache: crate::serverstatus::Cache::default(),
        health: Arc::new(Health::new(config.backend_health)),
        key_revocations: tokio::sync::broadcast::channel(256).0,
    });
    Ok(Prepared { state, progression })
}

/// `metagame serve`: runs until asked to stop (X7), then drains and checkpoints the database.
pub async fn run(config: Config, progression: ProgressionConfig) -> Result<(), String> {
    let checks = config.gateway_checks();
    for warning in &checks.warnings {
        tracing::warn!("{warning}");
    }
    if let Some(error) = checks.errors.first() {
        return Err(error.clone());
    }
    let Prepared { state, progression } = prepare(&config, progression)?;

    // Daily token stacks set to the daily amounts once (`SetDailyTokensOnce`)
    let settings = state.inventory;
    match state
        .store
        .write(move |conn| crate::inventory::set_daily_tokens_once(conn, &settings).map_err(store::Error::from))
        .await
    {
        Ok(Some(changed)) => tracing::info!("Daily tokens: set {changed} character(s) to the daily amounts (one time)"),
        Ok(None) => {}
        Err(error) => tracing::warn!(
            err = &error as &dyn std::error::Error,
            "Could not set the daily token stacks"
        ),
    }

    // Store purchase tokens that expired without being redeemed, and old receipts (also at most once
    // an hour when a token is issued)
    let now = crate::time::now_ms();
    state.store_sweeps.mark(now);
    match state
        .store
        .write(move |conn| crate::store::prune(conn, now).map_err(store::Error::from))
        .await
    {
        Ok(pruned) => crate::store::log_pruned(pruned),
        Err(error) => tracing::warn!(
            err = &error as &dyn std::error::Error,
            "Could not remove the expired store purchase tokens"
        ),
    }

    let (servers, users) = state
        .store
        .write(|conn| crate::accounts::drain_keys_to_register(conn).map_err(store::Error::from))
        .await
        .map_err(|e| format!("Registering the keys left by the host scripts failed: {e}"))?;
    tracing::info!("Registered {servers} new Gameserver API Key(s) on boot!");
    tracing::info!("Registered {users} new User API Key(s) on boot!");
    state.limits.spawn_sweepers();

    let address = SocketAddr::new(config.bind_host, config.port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("Could not listen on {}:{}: {e}", config.bind_host, config.port))?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;
    tracing::info!("Dauntless Revived metagame on {}:{}", bound.ip(), bound.port());
    if let Some(warning) = config.progression.warning() {
        tracing::warn!("{warning}");
    }
    tracing::info!("Progression mode: {}", config.progression.describe());
    tracing::info!("Progression config: {}", progression.description);
    let stub = config.progression.is_stub();
    match state
        .store
        .read(move |conn| crate::realprogression::upgrade_notice(conn, stub).map_err(store::Error::from))
        .await
    {
        Ok(Some(notice)) => tracing::warn!("{notice}"),
        Ok(None) => {}
        Err(error) => tracing::warn!(
            err = &error as &dyn std::error::Error,
            "Could not count the accounts without stored progression"
        ),
    }
    if config.origin.gateway_secret.is_some() {
        tracing::info!(
            "Public mode: behind the gateway (player addresses from loopback callers that carry the gateway secret)"
        );
    }
    tracing::info!("Clear Skies, Slayer.");

    let app = app(state.clone(), config.log_requests);
    serve(listener, app, stop_requested(), Duration::from_secs(2))
        .await
        .map_err(|e| e.to_string())?;
    state
        .store
        .checkpoint()
        .await
        .map_err(|e| format!("The database could not be checkpointed: {e}"))?;
    Ok(())
}
