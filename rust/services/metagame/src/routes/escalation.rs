//! `routes/escalation.ts`: GetSeasonalEscalationEndpoint and UpdateSeasonalEscalationEndpoint, both
//! `/escalation/{season_id}/{account_id}`, answering `{code, message, payload: <season>}`. Rules:
//! `escalation.rs`.
//!
//! `ESCALATION_MODE=stub` (the default) keeps what every account got before: the fake maximum on GET
//! and the catch-all's 404 on POST. With `ESCALATION_MODE=real`, accounts in real progression mode
//! read and save their own seasons; stub-mode accounts keep the stub.

use axum::extract::State;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::json;
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::progression::{caller, refuse_foreign};
use super::{fall_through, ok};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, metagame_auth};
use crate::escalation;
use crate::progression::{real_reply, refuse_unless_game_server};

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/escalation/:escalationSeason/:userId", read)
        .post("/escalation/:escalationSeason/:userId", save)
}

async fn read(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let season = params.get("escalationSeason").unwrap_or("").to_owned();
    let account = params.get("userId").unwrap_or("").to_owned();
    // Real: a player reads only their own account, a game server the one it names
    if state.escalation.real && state.progression.is_real(Some(&account)) {
        if let Some(refusal) = refuse_foreign(&auth, &parts, &account) {
            return Ok(refusal);
        }
        let (status, body) = state
            .store
            .read(move |conn| escalation::read_reply(conn, &account, &season).map_err(AppError::from))
            .await?;
        return Ok(real_reply(status, body));
    }
    tracing::info!("Escalation Configuration for season {season} (stubbed)");
    Ok(ok(json!({
        "code": null,
        "message": "OK",
        "payload": {
            "escalation_level": 99999,
            "next_level_xp": 99999,
            "talents_progress": [],
            "unlock_progress": [],
            "update_version": 1,
        }
    })))
}

/// The world server's authoritative snapshot: a game server's key only. A relayed player token for
/// another account is logged and the save is kept for the account the URL names.
async fn save(
    State(state): State<AppState>,
    parts: Parts,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let season = params.get("escalationSeason").unwrap_or("").to_owned();
    let account = params.get("userId").unwrap_or("").to_owned();
    // Only with ESCALATION_MODE=real, and only for real-mode accounts (`RealProgressionOnly`)
    if !state.escalation.real || !state.progression.is_real(Some(&account)) {
        return Ok(fall_through(&parts));
    }
    let auth = match metagame_auth(&parts, &state).await {
        Ok(auth) => auth,
        Err(refused) => return Ok(refused),
    };
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "escalation save", &account) {
        return Ok(refusal);
    }
    let (settings, who) = (state.escalation, caller(&auth));
    let (status, body) = state
        .store
        .write(move |conn| {
            escalation::save(conn, settings, &account, &season, body.as_ref(), who).map_err(AppError::from)
        })
        .await?;
    Ok(real_reply(status, body))
}
