//! `routes/friends.ts`: the Epic-style friends service the 1.4.4 client talks to
//! (`[OnlineSubsystemMcp.OnlineFriendsMcp] ServiceName="friends"`). Data: `friends.rs`. Everyone
//! shows as offline.

use std::sync::atomic::{AtomicI64, Ordering};

use axum::extract::{FromRequestParts, RawQuery, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json, query};
use crate::accounts::is_account_id_shape;
use crate::app::{AppError, AppState};
use crate::auth::{Player, SoftPlayer};
use crate::friends::{self, Outcome};
use crate::time::now_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/friends/api/public/friends/:userId", list_friends)
        .get("/friends/api/public/blocklist/:userId", list_blocked)
        .post("/friends/api/public/friends/:userId/:friendId", request)
        .delete("/friends/api/public/friends/:userId/:friendId", remove)
        // The verb is inferred from the exe (the Block request wasn't fully traced), so PUT does the
        // same; unblock is DELETE (traced)
        .post("/friends/api/public/blocklist/:userId/:friendId", block)
        .put("/friends/api/public/blocklist/:userId/:friendId", block)
        .delete("/friends/api/public/blocklist/:userId/:friendId", unblock)
        .get(
            "/friends/api/public/list/:namespace/:userId/recentPlayers",
            recent_players,
        )
        .get("/friends/api/v1/:userId/settings", settings)
}

static LAST_FOREIGN_LIST_WARNING: AtomicI64 = AtomicI64::new(0);

/// The reads the client makes at login. Without a valid token, or with a token asking for another
/// account's list (the client only reads its own), they answer the old static reply.
fn own_list(parts: &Parts, caller: Option<&str>, account: &str) -> Option<String> {
    if let Some(caller) = caller
        && caller != account
    {
        let now = now_ms();
        if now - LAST_FOREIGN_LIST_WARNING.load(Ordering::Relaxed) > 60 * 1000 {
            LAST_FOREIGN_LIST_WARNING.store(now, Ordering::Relaxed);
            tracing::warn!(
                "{} {} by {caller}: another account's list; answering the empty reply",
                parts.method,
                parts.uri.path()
            );
        }
    }
    caller.filter(|caller| *caller == account).map(str::to_owned)
}

/// The friends service's reads that 404'd upstream (`MiscRoutesOn`), before any auth.
fn misc_off(state: &AppState, parts: &Parts) -> Option<Response> {
    (!state.features.misc_routes).then(|| fall_through(parts))
}

async fn list_friends(
    State(state): State<AppState>,
    parts: Parts,
    params: Params,
    RawQuery(raw): RawQuery,
) -> Result<Response, AppError> {
    if let Some(off) = misc_off(&state, &parts) {
        return Ok(off);
    }
    let claims = match SoftPlayer::from_request_parts(&mut parts.clone(), &state).await {
        Ok(SoftPlayer(claims)) => claims,
        Err(refused) => return Ok(refused),
    };
    let account = params.get("userId").unwrap_or("");
    let Some(caller) = own_list(&parts, claims.as_ref().map(|c| c.user_id.as_str()), account) else {
        return Ok(json(StatusCode::OK, json!([])));
    };
    let pending = query(raw.as_deref()).get("includePending").and_then(Value::as_str) == Some("true");
    let id = caller.clone();
    let list = state
        .store
        .read(move |conn| friends::list_friends(conn, &id, pending).map_err(AppError::from))
        .await?;
    let count = |status: &str| list.iter().filter(|f| f["status"] == status).count();
    tracing::info!(
        "friends: list for {caller}: {} friend(s), {} pending",
        count("ACCEPTED"),
        count("PENDING")
    );
    Ok(json(StatusCode::OK, Value::Array(list)))
}

async fn list_blocked(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    if let Some(off) = misc_off(&state, &parts) {
        return Ok(off);
    }
    let claims = match SoftPlayer::from_request_parts(&mut parts.clone(), &state).await {
        Ok(SoftPlayer(claims)) => claims,
        Err(refused) => return Ok(refused),
    };
    let account = params.get("userId").unwrap_or("");
    let Some(caller) = own_list(&parts, claims.as_ref().map(|c| c.user_id.as_str()), account) else {
        return Ok(json(StatusCode::OK, json!({"blockedUsers": []})));
    };
    let list = state
        .store
        .read(move |conn| friends::list_blocked(conn, &caller).map_err(AppError::from))
        .await?;
    Ok(json(StatusCode::OK, json!({"blockedUsers": list})))
}

type Change = fn(&mut store::Connection, &AppState, &str, &str) -> diesel::QueryResult<Outcome>;

/// Changes answer 204 with no body, as the Epic service did. The URL's first id must be the caller.
async fn change(
    state: AppState,
    parts: Parts,
    player: Player,
    params: Params,
    work: Change,
) -> Result<Response, AppError> {
    let me = player.user_id;
    if params.get("userId") != Some(me.as_str()) {
        tracing::warn!(
            "Refusing {} {} by {me}: another account's list",
            parts.method,
            parts.uri.path()
        );
        return Ok(empty(StatusCode::FORBIDDEN));
    }
    let them = params.get("friendId").unwrap_or("").to_owned();
    if !is_account_id_shape(&them) {
        return Ok(empty(StatusCode::NOT_FOUND));
    }
    let shared = state.clone();
    let outcome = state
        .store
        .write(move |conn| work(conn, &shared, &me, &them).map_err(AppError::from))
        .await?;
    Ok(empty(match outcome {
        Outcome::Done(_) => StatusCode::NO_CONTENT,
        Outcome::Refused(status, _) => StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    }))
}

/// Send a friend request, or accept the one the other player sent.
async fn request(
    State(state): State<AppState>,
    parts: Parts,
    player: Player,
    params: Params,
) -> Result<Response, AppError> {
    change(state, parts, player, params, |conn, state, me, them| {
        friends::send_or_accept(conn, &state.friend_requests, me, them)
    })
    .await
}

/// Unfriend, withdraw a request, or decline one.
async fn remove(
    State(state): State<AppState>,
    parts: Parts,
    player: Player,
    params: Params,
) -> Result<Response, AppError> {
    change(state, parts, player, params, |conn, _, me, them| {
        friends::remove(conn, me, them)
    })
    .await
}

async fn block(
    State(state): State<AppState>,
    parts: Parts,
    player: Player,
    params: Params,
) -> Result<Response, AppError> {
    change(state, parts, player, params, |conn, _, me, them| {
        friends::block(conn, me, them)
    })
    .await
}

async fn unblock(
    State(state): State<AppState>,
    parts: Parts,
    player: Player,
    params: Params,
) -> Result<Response, AppError> {
    change(state, parts, player, params, |conn, _, me, them| {
        friends::unblock(conn, me, them)
    })
    .await
}

/// Recent players are kept in the character data already; the friends service's list stays empty.
async fn recent_players(State(state): State<AppState>, parts: Parts) -> Response {
    misc_off(&state, &parts).unwrap_or_else(|| json(StatusCode::OK, json!([])))
}

async fn settings(State(state): State<AppState>, parts: Parts) -> Response {
    misc_off(&state, &parts).unwrap_or_else(|| json(StatusCode::OK, json!({"acceptInvites": "public"})))
}
