//! `routes/loadout.ts`. Loadout slots (unlock, slot count, active slot) exist for real-mode accounts
//! only; everyone else falls through to the catch-all 404 they always got, before any auth.

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::json;
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, metagame_auth};
use crate::characters::belongs;
use crate::loadouts;
use crate::progression::{real_reply, refuse_unless_game_server};

pub fn routes() -> Router<AppState> {
    Router::new()
        .post("/loadout/:userId/:characterId/unlock/:numSlots", unlock)
        .get("/loadout/:userId/:characterId/slotcount", slot_count)
        .post("/loadout/:userId/:characterId/active/:index", set_active)
        .get("/loadout/:userId/:characterId/all", all)
        .post("/loadout/:userId/:characterId/:index", save)
}

/// The account a request acts for: the URL's for a game server, the token's for a player.
fn requestor(auth: &MetagameAuth, params: &Params) -> Option<String> {
    if auth.is_game_server() {
        params.get("userId").map(str::to_owned)
    } else {
        auth.user_id().map(str::to_owned)
    }
}

fn caller(auth: &MetagameAuth) -> &'static str {
    if auth.is_game_server() { "gameserver" } else { "client" }
}

enum Target {
    /// Not a real-mode request: Express's next("route"), the catch-all 404.
    FallThrough,
    Refused(Response),
    Found {
        auth: MetagameAuth,
        account: String,
        character: String,
    },
}

/// `RealProgressionOnly`, `HasUndauntedMetagameAuth` and `ResolveRealLoadoutCharacter`.
async fn resolve(state: &AppState, parts: &Parts, params: &Params) -> Result<Target, AppError> {
    if !state.progression.is_real(params.get("userId")) {
        return Ok(Target::FallThrough);
    }
    let auth = match metagame_auth(parts, state).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(Target::Refused(refusal)),
    };
    let account = requestor(&auth, params);
    let character = params.get("characterId").unwrap_or("").to_owned();
    if !state.progression.is_real(account.as_deref()) {
        return Ok(Target::FallThrough);
    }
    let account = account.unwrap_or_default();
    let (a, c) = (account.clone(), character.clone());
    if !state
        .store
        .read(move |conn| belongs(conn, &a, &c).map_err(AppError::from))
        .await?
    {
        tracing::warn!(
            "Refusing loadout slots of characterId {character} to userId {account}: no such character for this user"
        );
        return Ok(Target::Refused(empty(StatusCode::NOT_FOUND)));
    }
    Ok(Target::Found {
        auth,
        account,
        character,
    })
}

/// Unlocks and the active slot are the game server's; a player's own client may not set them.
async fn unlock(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let (auth, account, character) = match resolve(&state, &parts, &params).await? {
        Target::FallThrough => return Ok(fall_through(&parts)),
        Target::Refused(refusal) => return Ok(refusal),
        Target::Found {
            auth,
            account,
            character,
        } => (auth, account, character),
    };
    if let Some(refusal) = refuse_unless_game_server(
        &auth,
        params.get("userId"),
        "loadout slot unlock",
        &format!("characterId {character}"),
    ) {
        return Ok(refusal);
    }
    let raw = params.get("numSlots").unwrap_or("").to_owned();
    let who = caller(&auth);
    let (status, body) = state
        .store
        .write(move |conn| loadouts::unlock(conn, &account, &character, &raw, who).map_err(AppError::from))
        .await?;
    Ok(real_reply(status, body))
}

async fn slot_count(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let character = match resolve(&state, &parts, &params).await? {
        Target::FallThrough => return Ok(fall_through(&parts)),
        Target::Refused(refusal) => return Ok(refusal),
        Target::Found { character, .. } => character,
    };
    let slots = state
        .store
        .read(move |conn| loadouts::read_slots(conn, &character).map_err(AppError::from))
        .await?;
    Ok(json(
        StatusCode::OK,
        json!({"code": null, "message": "OK", "payload": loadouts::slot_counts(slots)}),
    ))
}

async fn set_active(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let (auth, account, character) = match resolve(&state, &parts, &params).await? {
        Target::FallThrough => return Ok(fall_through(&parts)),
        Target::Refused(refusal) => return Ok(refusal),
        Target::Found {
            auth,
            account,
            character,
        } => (auth, account, character),
    };
    if let Some(refusal) = refuse_unless_game_server(
        &auth,
        params.get("userId"),
        "active loadout slot",
        &format!("characterId {character}"),
    ) {
        return Ok(refusal);
    }
    let raw = params.get("index").unwrap_or("").to_owned();
    let who = caller(&auth);
    let (status, body) = state
        .store
        .write(move |conn| loadouts::set_active(conn, &account, &character, &raw, who).map_err(AppError::from))
        .await?;
    Ok(real_reply(status, body))
}

async fn all(State(state): State<AppState>, auth: MetagameAuth, params: Params) -> Result<Response, AppError> {
    let account = requestor(&auth, &params);
    let character = params.get("characterId").unwrap_or("").to_owned();
    let real = state.progression.is_real(account.as_deref());
    let shown = account.clone().unwrap_or_else(|| "undefined".into());
    let reply = state
        .store
        .write(move |conn| {
            let Some(account) = account else {
                return Ok::<_, AppError>(None);
            };
            if !belongs(conn, &account, &character)? {
                return Ok(None);
            }
            loadouts::payload(conn, &account, &character, real).map(Some)
        })
        .await?;
    match reply {
        Some(payload) => Ok(json(
            StatusCode::OK,
            json!({"code": null, "message": "OK", "payload": payload}),
        )),
        None => {
            tracing::warn!(
                "Refusing loadouts of characterId {} to userId {shown}: no such character for this user",
                params.get("characterId").unwrap_or("")
            );
            Ok(empty(StatusCode::NOT_FOUND))
        }
    }
}

/// Real mode saves any unlocked slot by its slot index; the stub only knows slot 0 and persistent.
async fn save(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = body.ok_or_else(|| AppError::Other("POST /loadout without a body".into()))?;
    let account = requestor(&auth, &params);
    let character = params.get("characterId").unwrap_or("").to_owned();
    let index = params.get("index").unwrap_or("").to_owned();
    let shown = account.clone().unwrap_or_else(|| "undefined".into());
    let owned = match account.clone() {
        Some(a) => {
            let c = character.clone();
            state
                .store
                .read(move |conn| belongs(conn, &a, &c).map_err(AppError::from))
                .await?
        }
        None => false,
    };
    if !owned {
        tracing::warn!(
            "Failed to update loadout index {index} for userId {shown} and characterId {character}: no such character for this user"
        );
        return Ok(empty(StatusCode::NOT_FOUND));
    }
    let account = account.unwrap_or_default();
    let real = state.progression.is_real(Some(&account));
    let reason = if auth.is_game_server() {
        "save:gameserver"
    } else {
        "save:client"
    };
    let history = state.history;
    let (a, c, i, data) = (
        account.clone(),
        character.clone(),
        index.clone(),
        body.get("data").cloned(),
    );
    // Node catches any error of the save (rolled back) and answers 400.
    let saved = state
        .store
        .write(move |conn| {
            if real {
                loadouts::set_slot(conn, &history, &a, &c, &i, data.as_ref(), reason)
            } else {
                loadouts::set_data(conn, &history, &a, &c, &i, data.as_ref(), reason)
            }
            .map_err(AppError::from)
        })
        .await
        .unwrap_or_else(|error| {
            tracing::error!(
                "Failed to update loadout index {index} for userId {account} and characterId {character}: {error}"
            );
            false
        });
    if !saved {
        tracing::error!("Failed to update loadout index {index} for userId {account} and characterId {character}");
        return Ok(empty(StatusCode::BAD_REQUEST));
    }
    tracing::info!("Successfully updated loadout index {index} for userId {account} and characterId {character}");
    let payload = state
        .store
        .write(move |conn| loadouts::payload(conn, &account, &character, real))
        .await?;
    Ok(json(
        StatusCode::OK,
        json!({"code": null, "message": "OK", "payload": payload}),
    ))
}
