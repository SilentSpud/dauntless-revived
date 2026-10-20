//! `routes/party.ts`: parties. Every route acts as the bearer token's own account: ids in the URL or
//! body only ever name the other player or the party, never who is acting. State: `party.rs`.

use std::sync::atomic::{AtomicI64, Ordering};

use axum::extract::{FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::json;
use crate::app::{AppError, AppState};
use crate::auth::{Player, SoftPlayer};
use crate::party::Reply;
use crate::time::now_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .post("/party", poll)
        .get("/party/invites", invites)
        .put("/party/invite", invite)
        .put("/party/invite/accept/:inviteId", accept)
        .delete("/party/invite", decline)
        .delete("/party/member", leave)
        .delete("/party/member/:memberId", kick)
        .put("/party/member/promote/:memberId", promote)
        .delete("/party/leader/:leaderId", evict_leader)
        .post("/party/status", status)
}

fn send(reply: Reply) -> Response {
    json(
        StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        reply.body,
    )
}

fn field<'a>(body: &'a Option<Value>, name: &str) -> Option<&'a Value> {
    body.as_ref().and_then(Value::as_object).and_then(|b| b.get(name))
}

/// The client's poll, about every 10 s: the caller's party (a party of one if none).
async fn poll(State(state): State<AppState>, player: Player) -> Result<Response, AppError> {
    let shared = state.clone();
    let reply = state
        .store
        .read(move |conn| {
            let settings = shared.party_settings();
            shared
                .parties
                .lock()
                .poll(conn, settings, &player.user_id, now_ms())
                .map_err(AppError::from)
        })
        .await?;
    Ok(json(StatusCode::OK, reply))
}

static LAST_ANONYMOUS_INVITES_WARNING: AtomicI64 = AtomicI64::new(0);

/// Polled with POST /party. Without a valid token it answers the old empty list, so the poll never
/// breaks; nobody ever sees another account's invites.
async fn invites(State(state): State<AppState>, parts: Parts) -> Result<Response, AppError> {
    let claims = match SoftPlayer::from_request_parts(&mut parts.clone(), &state).await {
        Ok(SoftPlayer(claims)) => claims,
        Err(refused) => return Ok(refused),
    };
    let Some(user) = claims.map(|c| c.user_id) else {
        let now = now_ms();
        if now - LAST_ANONYMOUS_INVITES_WARNING.load(Ordering::Relaxed) > 60 * 1000 {
            LAST_ANONYMOUS_INVITES_WARNING.store(now, Ordering::Relaxed);
            tracing::warn!("GET /party/invites without a valid player token: answering an empty list");
        }
        return Ok(json(StatusCode::OK, json!({"invitations": []})));
    };
    let shared = state.clone();
    let reply = state
        .store
        .read(move |conn| {
            shared
                .parties
                .lock()
                .list_invites(conn, &user, now_ms())
                .map_err(AppError::from)
        })
        .await?;
    Ok(json(StatusCode::OK, reply))
}

/// `{recipientPlayerId, partyId, buildId, featureOverrides}`.
async fn invite(
    State(state): State<AppState>,
    player: Player,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let shared = state.clone();
    let reply = state
        .store
        .read(move |conn| {
            shared
                .parties
                .lock()
                .invite(
                    conn,
                    &player.user_id,
                    field(&body, "recipientPlayerId"),
                    field(&body, "partyId"),
                    now_ms(),
                )
                .map_err(AppError::from)
        })
        .await?;
    Ok(send(reply))
}

/// `{recipientPlayerId: <caller>, partyId: <invite id>, ...}`. Only the token account's own invites
/// are looked up, whatever the body says; a body naming someone else is logged and changes nothing.
async fn accept(
    State(state): State<AppState>,
    player: Player,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    if let Some(Value::String(recipient)) = field(&body, "recipientPlayerId")
        && !recipient.is_empty()
        && *recipient != player.user_id
    {
        tracing::warn!(
            "PUT /party/invite/accept by {}: the body names recipient {}; only the caller's own invites are used",
            player.user_id,
            recipient.chars().take(64).collect::<String>()
        );
    }
    let invite = params.get("inviteId").unwrap_or("").to_owned();
    let shared = state.clone();
    let reply = state
        .store
        .read(move |conn| {
            let settings = shared.party_settings();
            shared
                .parties
                .lock()
                .accept(conn, settings, &player.user_id, &invite, now_ms())
                .map_err(AppError::from)
        })
        .await?;
    Ok(send(reply))
}

/// `{sendingPlayerId, recipientPlayerId, partyId}`: decline (or withdraw one's own invite).
async fn decline(State(state): State<AppState>, player: Player, ExpressBody(body): ExpressBody) -> Response {
    send(state.parties.lock().decline(&player.user_id, body.as_ref(), now_ms()))
}

/// No body: the caller leaves their party.
async fn leave(State(state): State<AppState>, player: Player) -> Response {
    send(state.parties.lock().leave(&player.user_id, now_ms()))
}

async fn kick(State(state): State<AppState>, player: Player, params: Params) -> Response {
    let member = params.get("memberId").unwrap_or("");
    send(state.parties.lock().kick(&player.user_id, member, now_ms()))
}

async fn promote(State(state): State<AppState>, player: Player, params: Params) -> Response {
    let member = params.get("memberId").unwrap_or("");
    send(state.parties.lock().promote(&player.user_id, member, now_ms()))
}

/// Only the client's auto-eviction of a leader it thinks is offline sends this.
async fn evict_leader(State(state): State<AppState>, player: Player, params: Params) -> Response {
    let leader = params.get("leaderId").unwrap_or("");
    send(state.parties.lock().evict_leader(&player.user_id, leader, now_ms()))
}

/// `{playerIds: [...]}` -> `{parties, invitations}`.
async fn status(
    State(state): State<AppState>,
    player: Player,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let shared = state.clone();
    let reply = state
        .store
        .read(move |conn| {
            let settings = shared.party_settings();
            shared
                .parties
                .lock()
                .status(conn, settings, &player.user_id, field(&body, "playerIds"), now_ms())
                .map_err(AppError::from)
        })
        .await?;
    Ok(send(reply))
}
