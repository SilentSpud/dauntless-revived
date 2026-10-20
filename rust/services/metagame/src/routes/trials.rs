//! `routes/tuning.ts` and `routes/trials.ts`, which Node mounts one after the other: the tuning
//! schedule and Hunt Pass XP config, and the Trials leaderboards with the game servers' run
//! submissions.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::Router;

use super::{empty, fall_through, json};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, metagame_auth};
use crate::store::StoreFail;
use crate::time::now_ms;
use crate::trials;

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/game_tuning/seasonal_event_schedule", seasonal_event_schedule)
        .get("/game_tuning/huntpass_xp_config", huntpass_xp_config)
        // The 1.4.4 DLL points TrialsLeaderboardsEndpoint at /trials/leaderboards; Phoenix Labs'
        // public service also had the same aggregate at /trials/leaderboards/all
        .post("/trials/leaderboards", aggregate)
        .post("/trials/leaderboards/all", aggregate)
        .post("/trials/leaderboards/solo", solo)
        .post("/trials/leaderboards/solo/individual", solo_individual)
        .post("/trials/leaderboards/group", group)
        .post("/trials/leaderboards/group/individual", group_individual)
}

async fn seasonal_event_schedule(State(state): State<AppState>) -> Result<Response, AppError> {
    tracing::info!(
        "Seasonal Event Schedule ({})",
        if state.shop.trials_schedule {
            "Trials active"
        } else {
            "stubbed"
        }
    );
    match trials::seasonal_event_schedule(&state.shop, now_ms()) {
        Ok(reply) => Ok(json(StatusCode::OK, reply)),
        Err(StoreFail::Refused(_, message) | StoreFail::Internal(message)) => Err(AppError::Other(message)),
    }
}

async fn huntpass_xp_config() -> Response {
    tracing::info!("Huntpass XP Config (stubbed)");
    json(
        StatusCode::OK,
        json!({
            "code": null,
            "message": "OK",
            "payload": {
                "EventConfigs": [],
                "GlobalConfig": {"DifficultyBias": 1, "GlobalMultiplier": 1, "MaxXPAwarded": 200}
            }
        }),
    )
}

/// `TrialsOn` (`TRIALS_LEADERBOARDS`, before auth), then `HasUndauntedMetagameAuth`.
async fn trials_on(state: &AppState, parts: &Parts) -> Result<MetagameAuth, Response> {
    if !state.shop.trials_leaderboards {
        return Err(fall_through(parts));
    }
    metagame_auth(parts, state).await
}

/// A Trials refusal as `{code: "<status>", message}`; anything else is logged and answered 500.
pub(super) fn refusal(fail: StoreFail, failed: &str) -> Response {
    match fail {
        StoreFail::Refused(status, message) => json(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            json!({"code": status.to_string(), "message": message}),
        ),
        StoreFail::Internal(error) => {
            tracing::error!("{failed}: {error}");
            json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"code": "500", "message": failed}),
            )
        }
    }
}

const READ_FAILED: &str = "Trials leaderboard request failed";

#[derive(Debug, Clone, Copy)]
enum Board {
    All,
    Solo,
    Group,
    SoloIndividual,
    GroupIndividual,
}

/// Every read settles the finished weeks first (in its own transaction, as Node did), then reads.
async fn read_board(state: &AppState, body: Value, board: Board) -> Result<Value, StoreFail> {
    let epoch = trials::epoch(state.shop.trial_rotation_start)?;
    let ctx = state.store_context();
    state
        .store
        .write(move |conn| trials::finalize_completed_weeks(conn, &ctx))
        .await?;
    let now = now_ms();
    let individual = matches!(board, Board::SoloIndividual | Board::GroupIndividual);
    let query = trials::query_from(&body, epoch, now, individual)?;
    state
        .store
        .read(move |conn| match board {
            Board::All => trials::all_boards(conn, &query),
            Board::Solo => trials::board(conn, &query, "solo", now),
            Board::Group => trials::board(conn, &query, "group", now),
            Board::SoloIndividual => trials::individual(conn, &query, &body, "solo"),
            Board::GroupIndividual => trials::individual(conn, &query, &body, "group"),
        })
        .await
}

async fn serve(state: AppState, parts: Parts, body: Option<Value>, board: Board) -> Response {
    if let Err(refused) = trials_on(&state, &parts).await {
        return refused;
    }
    match read_board(&state, body.unwrap_or(Value::Null), board).await {
        Ok(reply) => json(StatusCode::OK, reply),
        Err(fail) => refusal(fail, READ_FAILED),
    }
}

/// The aggregate board, which also takes game-server results: a body with completion_time is a
/// submission (never from a player token), anything else the aggregate query.
async fn aggregate(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    let auth = match trials_on(&state, &parts).await {
        Ok(auth) => auth,
        Err(refused) => return refused,
    };
    let body = body.unwrap_or(Value::Null);
    if body.get("completion_time").is_none() {
        return match read_board(&state, body, Board::All).await {
            Ok(reply) => json(StatusCode::OK, reply),
            Err(fail) => refusal(fail, READ_FAILED),
        };
    }
    if !auth.is_game_server() {
        return json(
            StatusCode::FORBIDDEN,
            json!({"code": "403", "message": "Trials results may only be submitted by a game server"}),
        );
    }
    let ctx = state.store_context();
    match state.store.write(move |conn| trials::save_run(conn, &ctx, &body)).await {
        Ok(saved) => {
            tracing::info!(
                "Saved {} Trials result for {} on {}",
                saved.mode,
                saved.accounts.join(","),
                saved.trial_id
            );
            empty(StatusCode::NO_CONTENT)
        }
        Err(fail) => refusal(fail, "Trials result save failed"),
    }
}

async fn solo(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    serve(state, parts, body, Board::Solo).await
}

async fn solo_individual(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    serve(state, parts, body, Board::SoloIndividual).await
}

async fn group(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    serve(state, parts, body, Board::Group).await
}

async fn group_individual(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    serve(state, parts, body, Board::GroupIndividual).await
}
