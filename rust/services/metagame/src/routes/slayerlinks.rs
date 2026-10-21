//! `routes/slayerlinks.ts`: the eight Slayer Link routes the 1.4.4 client sends. Rules and shapes:
//! `slayerlinks.rs`. Every route acts as the bearer token's own account; ids in the body or URL only
//! name the other player. A game server's key alone gets 403.
//!
//! `SLAYER_LINKS=0` skips every route here, and the requests fall through to the 404 they got before.
//! The reward routes (`/slayerlink/links/rewards...`) are not answered either way.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::Value;
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json, query};
use crate::app::{AppError, AppState};
use crate::auth::metagame_auth;
use crate::slayerlinks::{self, Reply};
use crate::time::now_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/slayerlink/status_good", status)
        .get("/slayerlink/invites", invites)
        .get("/slayerlink/links", links)
        .put("/slayerlink/invite", invite)
        .post("/slayerlink/invite", answer)
        .delete("/slayerlink/invites/:accountId", delete_invites)
        .delete("/slayerlink/links", delete_link)
        .post("/slayerlink/availability", availability)
}

fn send((status, body): Reply) -> Response {
    json(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        body,
    )
}

/// `SlayerLinksOn`, the auth and `PlayerTokenOnly`: the caller, or the answer to send.
async fn caller(state: &AppState, parts: &Parts) -> Result<String, Response> {
    if !state.slayer_links {
        return Err(fall_through(parts));
    }
    let auth = metagame_auth(parts, state).await?;
    match auth.user_id().filter(|id| !id.is_empty()) {
        Some(user) => Ok(user.to_owned()),
        None => {
            tracing::warn!(
                "Refusing {} {}: it needs a player's token",
                parts.method,
                parts.uri.path()
            );
            Err(empty(StatusCode::FORBIDDEN))
        }
    }
}

type Lists = (Vec<Value>, Vec<Value>);

async fn lists(state: &AppState, parts: &Parts, reply: fn(Lists) -> Reply) -> Result<Response, AppError> {
    let user = match caller(state, parts).await {
        Ok(user) => user,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let lists = state
        .store
        .write(move |conn| slayerlinks::read_lists(conn, &user, now).map_err(AppError::from))
        .await?;
    Ok(send(reply(lists)))
}

/// The social panel's poll for news: `{invites, links, config}`.
async fn status(State(state): State<AppState>, parts: Parts) -> Result<Response, AppError> {
    lists(&state, &parts, slayerlinks::status_reply).await
}

async fn invites(State(state): State<AppState>, parts: Parts) -> Result<Response, AppError> {
    lists(&state, &parts, slayerlinks::invites_reply).await
}

async fn links(State(state): State<AppState>, parts: Parts) -> Result<Response, AppError> {
    lists(&state, &parts, slayerlinks::links_reply).await
}

/// `{account_id, slot, action_source}`: invite a friend.
async fn invite(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = match caller(&state, &parts).await {
        Ok(user) => user,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let reply = state
        .store
        .write(move |conn| slayerlinks::invite(conn, &user, body.as_ref(), now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// `{account_id, action: accept|reject|cancel, slot, action_source}`.
async fn answer(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = match caller(&state, &parts).await {
        Ok(user) => user,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let reply = state
        .store
        .write(move |conn| slayerlinks::answer(conn, &user, body.as_ref(), now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// DeleteAllInvites: the caller's own id clears all of the caller's invites, another id those
/// between the two.
async fn delete_invites(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let user = match caller(&state, &parts).await {
        Ok(user) => user,
        Err(refused) => return Ok(refused),
    };
    let account = params.get("accountId").unwrap_or("").to_owned();
    let now = now_ms();
    let reply = state
        .store
        .write(move |conn| slayerlinks::delete_invites(conn, &user, &account, now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// `{account_id, slot, delete_pair}`: remove a link.
async fn delete_link(
    State(state): State<AppState>,
    parts: Parts,
    RawQuery(raw): RawQuery,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = match caller(&state, &parts).await {
        Ok(user) => user,
        Err(refused) => return Ok(refused),
    };
    let query = query(raw.as_deref());
    let now = now_ms();
    let reply = state
        .store
        .write(move |conn| slayerlinks::delete_link(conn, &user, body.as_ref(), &query, now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// `{account_ids}`: which friends can be invited now.
async fn availability(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = match caller(&state, &parts).await {
        Ok(user) => user,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let reply = state
        .store
        .write(move |conn| slayerlinks::availability(conn, &user, body.as_ref(), now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}
