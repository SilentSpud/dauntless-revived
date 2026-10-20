//! `routes/character.ts`: the account's characters, a new character, and character saves.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;
use service_kit::body::ExpressBody;
use service_kit::routing::Router;

use super::{empty, json, ok};
use crate::app::{AppError, AppState};
use crate::auth::MetagameAuth;
use crate::characters::{self, CharacterError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/character", list)
        .put("/character", create)
        .post("/character", save)
}

fn no_account(what: &str) -> AppError {
    AppError::Other(format!("{what} without an account"))
}

async fn list(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    let user_id = auth
        .user_id()
        .map(str::to_owned)
        .ok_or_else(|| no_account("GET /character"))?;
    let id = user_id.clone();
    let rows = state
        .store
        .write(move |conn| characters::for_user(conn, &id).map_err(AppError::from))
        .await?;
    let rows = rows.map_err(|_| AppError::Other(format!("GET /character for {user_id}, which has no account")))?;
    tracing::info!("Retrieved {} characters for {user_id}", rows.len());
    Ok(json(
        StatusCode::OK,
        Value::Array(rows.iter().map(characters::CharacterRow::wire).collect()),
    ))
}

/// The name as SQLite stores what better-sqlite3 binds into the TEXT column: a string as it is, a
/// number as a double turned into text (5 becomes "5.0"); anything else fails, as in Node.
fn stored_name(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(name) => Some(name.clone()),
        Value::Number(n) => n.as_f64().map(|n| {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{n:.1}")
            } else {
                n.to_string()
            }
        }),
        _ => None,
    }
}

async fn create(
    State(state): State<AppState>,
    auth: MetagameAuth,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = body.ok_or_else(|| AppError::Other("PUT /character without a body".into()))?;
    let user_id = auth
        .user_id()
        .map(str::to_owned)
        .ok_or_else(|| no_account("PUT /character"))?;
    let raw_name = body.get("name");
    tracing::info!(
        "Creating a character named {} for user {user_id}",
        raw_name.map_or("undefined".to_owned(), |v| v
            .as_str()
            .map_or_else(|| v.to_string(), str::to_owned))
    );
    let name =
        stored_name(raw_name).ok_or_else(|| AppError::Other("PUT /character with a name SQLite can't store".into()))?;
    let row = state
        .store
        .write(move |conn| characters::create(conn, &user_id, &name).map_err(AppError::from))
        .await?;
    Ok(json(StatusCode::OK, row.wire()))
}

/// `ParseUpdateVersion`: a JSON number, or a string of digits, as a whole number of at least 0.
fn update_version(value: Option<&Value>) -> Option<i64> {
    const MAX_SAFE: f64 = 9_007_199_254_740_991.0;
    let number = match value? {
        Value::String(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse::<f64>().ok()?
        }
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    (number.fract() == 0.0 && (0.0..=MAX_SAFE).contains(&number)).then_some(number as i64)
}

fn shown(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => service_kit::jsjson::stringify(other),
    }
}

async fn save(
    State(state): State<AppState>,
    auth: MetagameAuth,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = body.ok_or_else(|| AppError::Other("POST /character without a body".into()))?;
    let character_id = body.get("characterId").and_then(Value::as_str).map(str::to_owned);
    let user_id = auth.user_id().map(str::to_owned);
    let shown_character = shown(body.get("characterId"));
    let shown_user = user_id.clone().unwrap_or_else(|| "undefined".into());
    tracing::info!(
        "Updating characterId {shown_character} for userId {shown_user} with updateVersion {}",
        shown(body.get("updateVersion"))
    );
    let Some(version) = update_version(body.get("updateVersion")) else {
        let raw = body
            .get("updateVersion")
            .map_or("undefined".to_owned(), service_kit::jsjson::stringify);
        tracing::warn!(
            "Refusing update of characterId {shown_character} for userId {shown_user}: updateVersion {raw} is not a version number"
        );
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let reason = if auth.is_game_server() {
        "save:gameserver"
    } else {
        "save:client"
    };
    let history = state.history;
    let (id, user) = (character_id.clone(), user_id.clone());
    let result = state
        .store
        .write(move |conn| {
            let outcome = characters::update(
                conn,
                &history,
                id.as_deref(),
                user.as_deref(),
                body.get("data"),
                version,
                reason,
            )?;
            let row = match (&outcome, id.as_deref(), user.as_deref()) {
                (Ok(()), Some(c), Some(u)) => characters::find(conn, c, u)?,
                _ => None,
            };
            Ok::<_, AppError>((outcome, row))
        })
        .await?;
    match result {
        (Ok(()), Some(row)) => Ok(json(StatusCode::OK, row.wire())),
        (Ok(()), None) => Ok(ok(Value::Null)),
        (Err(CharacterError::NotFound | CharacterError::NoAccount), _) => {
            tracing::warn!(
                "Failed to update characterId {shown_character} for userId {shown_user}: no such character for this user"
            );
            Ok(empty(StatusCode::NOT_FOUND))
        }
        (Err(CharacterError::InvalidData(why)), _) => {
            tracing::warn!(
                "Refusing update of characterId {shown_character} for userId {shown_user} with updateVersion {version}: {why}"
            );
            Ok(empty(StatusCode::BAD_REQUEST))
        }
        (Err(CharacterError::Conflict(stored)), _) => {
            tracing::warn!(
                "Failed to update characterId {shown_character} for userId {shown_user} due to conflict (stored updateVersion {stored}, incoming {version})"
            );
            Ok(empty(StatusCode::CONFLICT))
        }
    }
}
