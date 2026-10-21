//! The host and launcher routes of `routes/undauntedapi.ts` (under `/undaunted/api`) that are not
//! about accounts or save history: hunt regions, Discord links and key statistics, backend health,
//! the dashboard's account directory, device signals, moderation, playing addresses, online stats,
//! the server status, by-name fallbacks for parties, friends and guilds, and account key recovery.
//! Mounted in pieces so the routes keep Node's order around `admin_accounts.rs` and
//! `admin_history.rs`.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::{RawQuery, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::{around, enforce};
use service_kit::routing::{Params, Router};

use super::{empty, field, json, ok, query};
use crate::accounts::{AccountError, find_account};
use crate::app::{AppError, AppState};
use crate::auth::{AdminKey, SoftAccount, UserInfo, UserKey};
use crate::discordstats;
use crate::keyrotation::{discord_account, is_discord_id, link_discord_account};
use crate::limits::per_client;
use crate::matchmaking::region::{self, Region};
use crate::moderation::{self, Refusal, Reported};
use crate::time::{iso_now, now_ms};

/// The routes before `RegistrationStatus`.
pub fn before(state: &AppState) -> Router<AppState> {
    let health_limit = || from_fn_with_state(per_client(&state.limits.health_read, state.origin.clone()), enforce);
    let admin_limit = from_fn_with_state(per_client(&state.limits.admin_mutation, state.origin.clone()), enforce);
    Router::new()
        .post("/undaunted/api/DeviceSignals", device_signals)
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
        .get("/undaunted/api/Moderation/:accountId", moderation_info)
        .get("/undaunted/api/ModerationLog", moderation_log)
        .post("/undaunted/api/Moderation", set_moderation)
}

/// After `RevokeEntitlement`, at the end of the file.
pub fn after(state: &AppState) -> Router<AppState> {
    let health_limit = from_fn_with_state(per_client(&state.limits.health_read, state.origin.clone()), enforce);
    Router::new()
        .post("/undaunted/api/PartyInvite", party_invite)
        .post("/undaunted/api/Friends", friends)
        .post("/undaunted/api/GuildInvite", guild_invite)
        .post("/undaunted/api/DisbandGuild", disband_guild)
        .get("/undaunted/api/Guilds", list_guilds)
        .get("/undaunted/api/AccountRecovery/:userId", account_recovery)
        .layer(health_limit)
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

// ---- Device signals and moderation ----

/// `{smbiosUuid?, tpmEkSha256?}` from the launcher: 204, or 403 when they match a device ban.
async fn device_signals(
    State(state): State<AppState>,
    UserKey(user): UserKey,
    ExpressBody(body): ExpressBody,
) -> Response {
    let key = format!("device:{}", user.user_id);
    around(&state.limits.device_signals, Some(&key), async {
        let (key, account, now) = (state.device_signal_key.clone(), user.user_id.clone(), now_ms());
        let result = state
            .store
            .write(move |conn| {
                Ok::<_, AppError>(moderation::report_signals(
                    conn,
                    key.as_deref(),
                    &account,
                    body.as_ref(),
                    now,
                ))
            })
            .await;
        no_store(match result {
            Ok(Ok(Reported::Stored)) => empty(StatusCode::NO_CONTENT),
            Ok(Ok(Reported::Blocked(reason))) => json(
                StatusCode::FORBIDDEN,
                json!({"error": "device_banned", "reason": reason}),
            ),
            Ok(Err(Refusal::Code("device_signals_unavailable"))) => json(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": "device_signals_unavailable"}),
            ),
            _ => json(StatusCode::BAD_REQUEST, json!({"error": "invalid_device_signals"})),
        })
    })
    .await
}

async fn moderation_info(
    State(state): State<AppState>,
    _admin: AdminKey,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("accountId").unwrap_or("").to_owned();
    let app = state.clone();
    let info = state
        .store
        .read(move |conn| moderation::info(conn, &mut app.activity.lock(), &account, now_ms()).map_err(AppError::from))
        .await?;
    Ok(no_store(ok(info)))
}

/// `?kind=bans|events&offset=`.
async fn moderation_log(
    State(state): State<AppState>,
    _admin: AdminKey,
    RawQuery(raw): RawQuery,
) -> Result<Response, AppError> {
    let query = query(raw.as_deref());
    let events = match query.get("kind").and_then(Value::as_str) {
        Some("bans") => false,
        Some("events") => true,
        _ => {
            return Ok(no_store(json(
                StatusCode::BAD_REQUEST,
                json!({"error": "invalid_kind"}),
            )));
        }
    };
    let offset = match query.get("offset") {
        None => 0.0,
        Some(value) => crate::js::number(Some(value)),
    };
    let log = state
        .store
        .read(move |conn| Ok::<_, AppError>(moderation::log(conn, events, offset, now_ms())))
        .await?;
    Ok(no_store(match log {
        Ok(log) => ok(log),
        Err(_) => json(StatusCode::BAD_REQUEST, json!({"error": "invalid_offset"})),
    }))
}

/// `{accountId, reason, active, address?, durationHours?, includeDevice?}`.
async fn set_moderation(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let app = state.clone();
    let result = state
        .store
        .write(move |conn| {
            Ok::<_, AppError>(moderation::set_ban(
                conn,
                &mut app.activity.lock(),
                body.as_ref(),
                &admin.user_id,
                now_ms(),
            ))
        })
        .await?;
    Ok(match result {
        Ok(info) => ok(info),
        Err(Refusal::Code(code)) => json(StatusCode::BAD_REQUEST, json!({"error": code})),
        Err(Refusal::Db(error)) => {
            tracing::error!("Moderation failed: {error}");
            json(StatusCode::BAD_REQUEST, json!({"error": "moderation_failed"}))
        }
    })
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

// ---- Account key recovery ----

#[derive(QueryableByName)]
struct Ciphertext {
    #[diesel(sql_type = Text)]
    ciphertext: String,
}

/// An admin reads back a player's account key (`ACCOUNT_KEY_RECOVERY=1`), only while it is still the
/// account's key; `null` when there is none to give.
async fn account_recovery(
    State(state): State<AppState>,
    _admin: AdminKey,
    params: Params,
) -> Result<Response, AppError> {
    let id = params.get("userId").unwrap_or("").to_owned();
    if !crate::keyrotation::is_link_user_id(&id) {
        return Ok(no_store(empty(StatusCode::BAD_REQUEST)));
    }
    if !state.auth.account_key_recovery {
        return Ok(no_store(ok(json!({"accountId": id, "key": null}))));
    }
    let account = id.clone();
    let row = state
        .store
        .read(move |conn| {
            diesel::sql_query(
                "SELECT r.ciphertext FROM accountkeyrecovery r JOIN userapikeys k ON k.userId = r.userId AND k.keyHash = r.keyHash                  JOIN users u ON u.userId = r.userId WHERE r.userId = ? AND u.isAdmin = 0",
            )
            .bind::<Text, _>(&account)
            .load::<Ciphertext>(conn)
            .map_err(AppError::from)
        })
        .await?;
    let key = match row.first() {
        None => None,
        Some(row) => match crate::crypto::open_recovery(&state.auth.signing_key_b64, &id, &row.ciphertext) {
            Ok(key) => Some(key),
            Err(_) => {
                return Ok(no_store(json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error": "recovery_unavailable"}),
                )));
            }
        },
    };
    Ok(no_store(ok(json!({"accountId": id, "key": key}))))
}

// ---- Parties, friends and guilds by name ----
// Fallbacks for when the game's own buttons are missing or fail: the key's owner invites a player to
// their party or guild (who still accepts in game), or sends them a friend request. An admin may act
// for another player with "From" (a name or account id), only directly on the host.

fn not_found() -> Response {
    let error = AccountError::NotFound;
    json(
        StatusCode::NOT_FOUND,
        json!({"error": error.code(), "message": error.message()}),
    )
}

/// `FindAccount` on a body field.
async fn account_named(state: &AppState, who: Option<Value>) -> Result<Option<(String, String)>, AppError> {
    state
        .store
        .read(move |conn| find_account(conn, who.as_ref()).map_err(AppError::from))
        .await
}

/// `ActingAccount`: the key's owner, or for an admin the account `From` names; else the refusal.
async fn acting_account(
    state: &AppState,
    parts: &Parts,
    caller: &UserInfo,
    body: &Option<Value>,
) -> Result<Result<(String, String), Response>, AppError> {
    let Some(from) = field(body, "From") else {
        return Ok(Ok((caller.user_id.clone(), caller.username.clone())));
    };
    if !caller.is_admin {
        return Ok(Err(json(
            StatusCode::FORBIDDEN,
            json!({"error": "forbidden", "message": "Only an admin may act for another player."}),
        )));
    }
    if crate::auth::origin(parts).is_proxied() {
        tracing::warn!(
            "Refusing an admin request on {} {} from {}: admin keys are not accepted through a proxy",
            parts.method,
            parts.uri.path(),
            crate::auth::origin(parts).client_address(&state.origin)
        );
        return Ok(Err(empty(StatusCode::FORBIDDEN)));
    }
    Ok(account_named(state, Some(from.clone())).await?.ok_or_else(not_found))
}

/// The acting account and the one `Username` names, or the refusal.
async fn both_accounts(
    state: &AppState,
    parts: &Parts,
    caller: &UserInfo,
    body: &Option<Value>,
) -> Result<Result<((String, String), (String, String)), Response>, AppError> {
    let from = match acting_account(state, parts, caller, body).await? {
        Ok(from) => from,
        Err(refused) => return Ok(Err(refused)),
    };
    Ok(match account_named(state, field(body, "Username").cloned()).await? {
        Some(to) => Ok((from, to)),
        None => Err(not_found()),
    })
}

/// `{Username, From?}` -> `{From, To}`.
async fn party_invite(
    State(state): State<AppState>,
    parts: Parts,
    UserKey(caller): UserKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let ((from, from_name), (to, to_name)) = match both_accounts(&state, &parts, &caller, &body).await? {
        Ok(both) => both,
        Err(refused) => return Ok(refused),
    };
    let (shared, sender, recipient) = (state.clone(), from.clone(), to.clone());
    let reply = state
        .store
        .read(move |conn| {
            shared
                .parties
                .lock()
                .invite(conn, &sender, Some(&json!(recipient)), None, now_ms())
                .map_err(AppError::from)
        })
        .await?;
    tracing::info!(
        "party: /undaunted/api/PartyInvite by key of {}: from={from} to={to} -> {}",
        caller.user_id,
        reply.status
    );
    if reply.status != 200 {
        let message = reply.reason.unwrap_or_else(|| "The invite was refused.".into());
        return Ok(json(
            StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            json!({"error": "party_invite_refused", "message": message}),
        ));
    }
    Ok(ok(json!({"From": from_name, "To": to_name})))
}

fn friend_error_message(error: &str) -> &'static str {
    match error {
        "blocked" => "One of the two has blocked the other.",
        "self" => "That is the same account.",
        "limit" => "Too many friends or requests.",
        "pending_limit" => "Too many friend requests are still unanswered.",
        "rate" => "Too many friend requests in the last 10 minutes; try again later.",
        _ => "No such account.",
    }
}

/// `{Username, From?}` -> `{From, To, Result: requested | accepted | already_friends | already_requested}`.
async fn friends(
    State(state): State<AppState>,
    parts: Parts,
    UserKey(caller): UserKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let ((from, from_name), (to, to_name)) = match both_accounts(&state, &parts, &caller, &body).await? {
        Ok(both) => both,
        Err(refused) => return Ok(refused),
    };
    let shared = state.clone();
    let (outcome, event) = state
        .store
        .write(move |conn| {
            crate::friends::send_or_accept(conn, &shared.friend_requests, &from, &to).map_err(AppError::from)
        })
        .await?;
    crate::friends::notify(&state, event);
    Ok(match outcome {
        crate::friends::Outcome::Done(result) => ok(json!({"From": from_name, "To": to_name, "Result": result})),
        crate::friends::Outcome::Refused(status, error) => json(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            json!({"error": error, "message": friend_error_message(error)}),
        ),
    })
}

/// `GUILDS=0` turns the guild fallbacks off too.
fn guilds_off(state: &AppState) -> Option<Response> {
    (!state.guilds.enabled).then(|| {
        json(
            StatusCode::NOT_FOUND,
            json!({"error": "guilds_off", "message": "Guilds are turned off on this server (GUILDS=0)."}),
        )
    })
}

/// `{Username, From?}` -> `{From, To, Guild}`.
async fn guild_invite(
    State(state): State<AppState>,
    parts: Parts,
    UserKey(caller): UserKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    if let Some(off) = guilds_off(&state) {
        return Ok(off);
    }
    let ((from, from_name), (to, to_name)) = match both_accounts(&state, &parts, &caller, &body).await? {
        Ok(both) => both,
        Err(refused) => return Ok(refused),
    };
    super::guild::sweep(&state).await?;
    let now = now_ms();
    let (shared, user, them) = (state.clone(), from.clone(), to.clone());
    let (reply, why) = state
        .store
        .write(move |conn| {
            crate::guild::invite_in_tx(conn, &shared.guilds, &shared.guild_memory, &user, &them, now)
                .map_err(AppError::from)
        })
        .await?;
    crate::guild::invite_finish(&state.guild_memory, &from, &to, &reply, why, now);
    tracing::info!(
        "guild: /undaunted/api/GuildInvite by key of {}: from={from} to={to} -> {}",
        caller.user_id,
        reply.status
    );
    if reply.status != 200 {
        let text = |name: &str| {
            reply
                .body
                .as_ref()
                .and_then(|b| b.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let message = match (text("code").filter(|c| !c.is_empty()), text("message")) {
            (Some(code), message) => format!("{code}: {}", message.unwrap_or_default()),
            (None, Some(message)) => message,
            (None, None) => "The invite was refused.".into(),
        };
        return Ok(json(
            StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            json!({"error": "guild_refused", "message": message}),
        ));
    }
    let guild = state
        .store
        .read(move |conn| crate::guild::guild_name_of(conn, &from).map_err(AppError::from))
        .await?;
    Ok(ok(
        json!({"From": from_name, "To": to_name, "Guild": guild.unwrap_or_default()}),
    ))
}

/// `{Guild: <id or name>}` -> `{Guild, Members}`.
async fn disband_guild(
    State(state): State<AppState>,
    _admin: AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    if let Some(off) = guilds_off(&state) {
        return Ok(off);
    }
    let guild = field(&body, "Guild").cloned();
    let result = state
        .store
        .write(move |conn| crate::guild::disband_as_admin(conn, guild.as_ref()).map_err(AppError::from))
        .await?;
    Ok(match result {
        Some((name, members)) => ok(json!({"Guild": name, "Members": members})),
        None => json(
            StatusCode::NOT_FOUND,
            json!({"error": "not_found", "message": "No such guild."}),
        ),
    })
}

/// `[{guildId, name, nameplate, leader, members}]`.
async fn list_guilds(State(state): State<AppState>, _admin: AdminKey) -> Result<Response, AppError> {
    if let Some(off) = guilds_off(&state) {
        return Ok(off);
    }
    let list = state
        .store
        .read(|conn| crate::guild::list_guilds(conn).map_err(AppError::from))
        .await?;
    Ok(ok(Value::Array(list)))
}
