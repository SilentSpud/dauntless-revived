//! The host and launcher routes of `routes/undauntedapi.ts` (under `/undaunted/api`) that are not
//! about accounts or save history: hunt regions, Discord links and key statistics, backend health,
//! the dashboard's account directory, playing addresses, online stats and the server status.
//! Mounted in pieces so the routes keep Node's order around `admin_accounts.rs` and
//! `admin_history.rs`.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::{RawQuery, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::enforce;
use service_kit::routing::{Params, Router};

use super::{empty, field, json, ok, query};
use crate::app::{AppError, AppState};
use crate::auth::{AdminKey, SoftAccount, UserKey};
use crate::discordstats;
use crate::keyrotation::{discord_account, is_discord_id, link_discord_account};
use crate::limits::per_client;
use crate::matchmaking::region::{self, Region};
use crate::time::{iso_now, now_ms};

/// The routes before `RegistrationStatus`.
pub fn before(state: &AppState) -> Router<AppState> {
    let health_limit = || from_fn_with_state(per_client(&state.limits.health_read, state.origin.clone()), enforce);
    let admin_limit = from_fn_with_state(per_client(&state.limits.admin_mutation, state.origin.clone()), enforce);
    Router::new()
        .get("/undaunted/api/HuntRegion", hunt_region)
        .post("/undaunted/api/HuntRegion", set_hunt_region)
        .get("/undaunted/api/DiscordKeyStats", discord_key_stats)
        .layer(health_limit())
        .post("/undaunted/api/DiscordLink", link_discord)
        .layer(admin_limit)
        .get("/undaunted/api/DiscordLink/:discordId", discord_link)
        .layer(health_limit())
        .get("/undaunted/api/BackendHealth", backend_health)
        .layer(health_limit())
        .get("/undaunted/api/DashboardAccounts", dashboard_accounts)
        .layer(health_limit())
        .post("/undaunted/api/PlayingEnded", playing_ended)
}

/// After `GetUserInfo`.
pub fn online_stats() -> Router<AppState> {
    Router::new()
        .get("/undaunted/api/PrivateOnlineStats", private_online_stats)
        .get("/undaunted/api/PublicOnlineStats", public_online_stats)
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// ---- Hunt regions ----

async fn hunt_region(State(state): State<AppState>, UserKey(user): UserKey) -> Result<Response, AppError> {
    let settings = &state.matchmaking.settings.regions;
    // An invalid probe endpoint throws in Node, after the key check
    let probes = settings.probes_json().map_err(AppError::Other)?;
    let id = user.user_id.clone();
    let saved = state
        .store
        .read(move |conn| region::player_region(conn, &id).map_err(AppError::from))
        .await?;
    Ok(no_store(ok(json!({
        "region": saved.as_str(),
        "enabled": settings.us || settings.germany,
        "probes": probes,
    }))))
}

async fn set_hunt_region(
    State(state): State<AppState>,
    UserKey(user): UserKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let settings = &state.matchmaking.settings.regions;
    let refuse = |status: StatusCode, body: Value| Ok(no_store(json(status, body)));
    let Some(chosen) = field(&body, "region").and_then(Value::as_str).and_then(Region::parse) else {
        return refuse(StatusCode::BAD_REQUEST, json!({"error": "invalid_region"}));
    };
    match chosen {
        Region::Aus => {
            return refuse(
                StatusCode::GONE,
                json!({"error": "region_retired", "replacement": "us"}),
            );
        }
        Region::Ger if !settings.germany => {
            return refuse(StatusCode::SERVICE_UNAVAILABLE, json!({"error": "region_unavailable"}));
        }
        Region::Us if !settings.us => {
            return refuse(StatusCode::SERVICE_UNAVAILABLE, json!({"error": "region_unavailable"}));
        }
        _ => {}
    }
    let id = user.user_id.clone();
    state
        .store
        .write(move |conn| region::save_region(conn, &id, chosen).map_err(AppError::from))
        .await?;
    Ok(no_store(ok(json!({"region": chosen.as_str()}))))
}

// ---- Discord ----

async fn discord_key_stats(State(state): State<AppState>, _admin: AdminKey) -> Result<Response, AppError> {
    let linked = state
        .store
        .read(|conn| discordstats::linked_accounts(conn).map_err(AppError::from))
        .await?;
    let mut reply = json!({"at": iso_now(), "linkedAccounts": linked, "bot": null, "error": null});
    match state.discord_key_state_file.clone() {
        None => reply["error"] = json!("Bot claim statistics are not configured."),
        Some(file) => {
            let summary = match discordstats::read_state(&file).await {
                Some(bot) => {
                    let invites = state
                        .store
                        .read(|conn| discordstats::invites(conn).map_err(AppError::from))
                        .await?;
                    discordstats::summarize(&bot, &invites).ok()
                }
                None => None,
            };
            match summary {
                Some(bot) => reply["bot"] = bot,
                None => {
                    reply["error"] = json!("Bot claim statistics are unavailable; no zero counts have been assumed.")
                }
            }
        }
    }
    Ok(no_store(ok(reply)))
}

async fn link_discord(
    State(state): State<AppState>,
    _admin: AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let text = |name: &str| field(&body, name).and_then(Value::as_str).map(str::to_owned);
    let (discord, user) = (text("DiscordId"), text("UserId"));
    let status = state
        .store
        .write(move |conn| link_discord_account(conn, discord.as_deref(), user.as_deref()).map_err(AppError::from))
        .await?;
    Ok(no_store(ok(json!({"status": status}))))
}

async fn discord_link(State(state): State<AppState>, _admin: AdminKey, params: Params) -> Result<Response, AppError> {
    let discord = params.get("discordId").unwrap_or("").to_owned();
    if !is_discord_id(&discord) {
        return Ok(no_store(json(
            StatusCode::BAD_REQUEST,
            json!({"error": "invalid_discord_id"}),
        )));
    }
    let account = state
        .store
        .read(move |conn| discord_account(conn, &discord).map_err(AppError::from))
        .await?;
    let account = account.map(|(user, name)| json!({"userId": user, "username": name}));
    Ok(no_store(ok(json!({"account": account}))))
}

// ---- Host tools ----

async fn backend_health(State(state): State<AppState>, _admin: AdminKey) -> Response {
    if !state.health.enabled {
        return no_store(json(StatusCode::NOT_FOUND, json!({"error": "health_disabled"})));
    }
    no_store(ok(serde_json::to_value(state.health.report()).unwrap_or(Value::Null)))
}

#[derive(QueryableByName)]
struct DirectoryRow {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Bool)]
    admin: bool,
    #[diesel(sql_type = Nullable<Text>)]
    hash: Option<String>,
}

/// Operator-only directory. Keys are irreversible hashes in storage: only a short fingerprint, for
/// matching a launcher's backup key, never a key or the full stored hash.
async fn dashboard_accounts(
    State(state): State<AppState>,
    _admin: AdminKey,
    RawQuery(raw): RawQuery,
) -> Result<Response, AppError> {
    let query = query(raw.as_deref());
    let offset = match query.get("offset") {
        None => 0.0,
        Some(value) => crate::js::number(Some(value)),
    };
    if offset.fract() != 0.0 || !(0.0..=10_000_000.0).contains(&offset) {
        return Ok(no_store(json(
            StatusCode::BAD_REQUEST,
            json!({"error": "invalid_offset"}),
        )));
    }
    let offset = offset as i64;
    let search = match query.get("q") {
        Some(Value::String(text)) => text.trim().to_owned(),
        _ => String::new(),
    };
    if search.encode_utf16().count() > 100 {
        return Ok(no_store(json(
            StatusCode::BAD_REQUEST,
            json!({"error": "invalid_query"}),
        )));
    }
    let rows: Vec<DirectoryRow> = state
        .store
        .read(move |conn| {
            let base = "SELECT users.userId AS id, users.name AS name, users.isAdmin AS admin, userapikeys.keyHash AS hash \
                FROM users LEFT JOIN userapikeys ON users.userId = userapikeys.userId";
            let rows = if search.is_empty() {
                diesel::sql_query(format!("{base} ORDER BY users.userId LIMIT 101 OFFSET ?"))
                    .bind::<BigInt, _>(offset)
                    .load(conn)
            } else {
                diesel::sql_query(format!(
                    "{base} WHERE instr(lower(users.name), lower(?)) > 0 OR instr(lower(users.userId), lower(?)) > 0 \
                     OR instr(lower(substr(userapikeys.keyHash, 1, 16)), lower(?)) > 0 \
                     ORDER BY users.userId LIMIT 101 OFFSET ?"
                ))
                .bind::<Text, _>(&search)
                .bind::<Text, _>(&search)
                .bind::<Text, _>(&search)
                .bind::<BigInt, _>(offset)
                .load(conn)
            };
            rows.map_err(AppError::from)
        })
        .await?;
    let next = (rows.len() > 100).then_some(offset + 100);
    let accounts: Vec<Value> = rows
        .into_iter()
        .take(100)
        .map(|row| {
            let fingerprint = row
                .hash
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(|h| h[..16].to_ascii_lowercase());
            json!({
                "id": row.id,
                "name": row.name,
                "admin": row.admin,
                "developer": state.developer_accounts.contains(&row.id),
                "keyFingerprint": fingerprint,
            })
        })
        .collect();
    Ok(no_store(ok(json!({"accounts": accounts, "nextOffset": next}))))
}

async fn playing_ended(State(state): State<AppState>, UserKey(user): UserKey) -> Response {
    state.activity.lock().end_address(&user.user_id);
    empty(StatusCode::NO_CONTENT)
}

// ---- Who is online ----

/// `GetRecentPlayerData`: heartbeats of the last 90 s. Nothing records hunt locations any more, so
/// `HuntId` and `EnteredHuntAt` are never set.
fn recent_players(state: &AppState) -> Vec<Value> {
    state
        .activity
        .lock()
        .online(now_ms())
        .into_iter()
        .map(|(user, beat)| {
            let mut entry = serde_json::Map::new();
            entry.insert("UserId".into(), json!(user));
            if let Some(map) = beat.map {
                entry.insert("Map".into(), map);
            }
            Value::Object(entry)
        })
        .collect()
}

async fn private_online_stats(State(state): State<AppState>, _admin: AdminKey) -> Response {
    ok(Value::Array(recent_players(&state)))
}

async fn public_online_stats(State(state): State<AppState>, _user: UserKey) -> Response {
    ok(json!({"NumActivePlayers": recent_players(&state).len()}))
}

/// Server name, source, registration mode and (for registered players only) who is online and which
/// game servers run. Never a 401: the launcher reads it before registering. Cached 5 s per variant.
pub async fn server_status(
    State(state): State<AppState>,
    SoftAccount(registered): SoftAccount,
) -> Result<Response, AppError> {
    let status = crate::serverstatus::status(&state, registered).await?;
    let mut response = no_store(ok(status));
    response.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static("x-undaunted-user-api-key, authorization"),
    );
    Ok(response)
}
