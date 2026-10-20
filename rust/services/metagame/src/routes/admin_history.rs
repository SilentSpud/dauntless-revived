//! The save-history, progression and entitlement routes of `/undaunted/api` (admin keys only).

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::Router;

use super::{empty, field, json, query};
use crate::app::{AppError, AppState};
use crate::auth::AdminKey;
use crate::characters::{self, RollbackError};
use crate::entitlements;
use crate::progression::{Event, account_exists, record_event};
use crate::realprogression::{self, SeedMode};

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/undaunted/api/SaveHistory", save_history)
        .post("/undaunted/api/RollbackCharacter", rollback_character)
        .post("/undaunted/api/RollbackLoadout", rollback_loadout)
        .get("/undaunted/api/Progression", progression)
        .post("/undaunted/api/SeedProgression", seed_progression)
        .post("/undaunted/api/GrantEntitlement", grant)
        .post("/undaunted/api/RevokeEntitlement", revoke)
}

/// `Number.isSafeInteger`.
fn safe_integer(value: Option<&Value>) -> Option<i64> {
    let n = value?.as_f64()?;
    (n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0).then_some(n as i64)
}

/// `?CharacterId=` or `?UserId=` -> `{Characters: [...]}` with every saved version, newest first.
async fn save_history(
    State(state): State<AppState>,
    _admin: AdminKey,
    RawQuery(raw): RawQuery,
) -> Result<Response, AppError> {
    let query = query(raw.as_deref());
    let pick = |name: &str| {
        query
            .get(name)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let (character, user) = (pick("CharacterId"), pick("UserId"));
    if character.is_none() && user.is_none() {
        return Ok(empty(StatusCode::BAD_REQUEST));
    }
    let found = state
        .store
        .read(move |conn| {
            let ids = match (character, user) {
                (Some(id), _) => vec![id],
                (None, Some(user)) => characters::character_ids_of(conn, &user)?,
                (None, None) => vec![],
            };
            characters::save_history(conn, &ids).map_err(AppError::from)
        })
        .await?;
    if found.is_empty() {
        return Ok(empty(StatusCode::NOT_FOUND));
    }
    Ok(json(StatusCode::OK, json!({"Characters": found})))
}

fn rollback_reply(result: Result<Value, RollbackError>) -> Response {
    match result {
        Ok(data) => json(StatusCode::OK, data),
        Err(error) => empty(StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)),
    }
}

/// `{CharacterId, Version}`: a character's data back to a version listed by SaveHistory.
async fn rollback_character(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = body.ok_or_else(|| AppError::Other("RollbackCharacter without a body".into()))?;
    let (Some(Value::String(id)), Some(version)) =
        (body.get("CharacterId").cloned(), safe_integer(body.get("Version")))
    else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let history = state.history;
    let result = state
        .store
        .write(move |conn| {
            characters::rollback_character(conn, &history, &id, version, &admin.user_id).map_err(AppError::from)
        })
        .await?;
    Ok(rollback_reply(result))
}

/// `{CharacterId, Version}` with a loadout version from SaveHistory.
async fn rollback_loadout(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = body.ok_or_else(|| AppError::Other("RollbackLoadout without a body".into()))?;
    let (Some(Value::String(id)), Some(version)) =
        (body.get("CharacterId").cloned(), safe_integer(body.get("Version")))
    else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let history = state.history;
    let result = state
        .store
        .write(move |conn| {
            characters::rollback_loadout(conn, &history, &id, version, &admin.user_id).map_err(AppError::from)
        })
        .await?;
    Ok(rollback_reply(result))
}

/// `?UserId=`: the account's tracks with the ranks the client will compute, objectives, Hunt Pass
/// and entitlements.
async fn progression(
    State(state): State<AppState>,
    _admin: AdminKey,
    RawQuery(raw): RawQuery,
) -> Result<Response, AppError> {
    let query = query(raw.as_deref());
    let Some(user) = query
        .get("UserId")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
    else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let (config, defaults, id) = (
        state.progression_config.clone(),
        state.entitlement_defaults.clone(),
        user.clone(),
    );
    let found = state
        .store
        .write(move |conn| {
            if !account_exists(conn, &id)? {
                return Ok::<_, AppError>(None);
            }
            let entitlements = entitlements::list(conn, &defaults, &id)?;
            let hunt_pass = realprogression::selected_hunt_pass(conn, &config, &id)?;
            let mut tracks = realprogression::track_records(conn, &config, &id)?;
            for track in &mut tracks {
                let path = track["progression_id"].as_str().and_then(|p| config.path(p));
                let progress = track["progress"].as_i64().unwrap_or(0);
                let (free, premium) = match path {
                    None => (Value::Null, Value::Null),
                    Some(path) => {
                        let gate = path.premium_gating_entitlement.as_deref().unwrap_or("");
                        let has_premium =
                            !gate.is_empty() && entitlements.iter().any(|e| e["name"].as_str() == Some(gate));
                        (
                            json!(realprogression::earned_ranks(path, progress, false).0),
                            json!(realprogression::earned_ranks(path, progress, has_premium).1),
                        )
                    }
                };
                if let Some(map) = track.as_object_mut() {
                    map.insert("earned_free_rank".into(), free);
                    map.insert("earned_premium_rank".into(), premium);
                }
            }
            let objectives = realprogression::objective_records(conn, &id)?;
            Ok(Some((hunt_pass, tracks, objectives, entitlements)))
        })
        .await?;
    let Some((hunt_pass, tracks, objectives, entitlements)) = found else {
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    let real = state.progression.is_real(Some(&user));
    Ok(json(
        StatusCode::OK,
        json!({
            "UserId": user,
            "RealMode": real,
            "HuntPass": hunt_pass,
            "Tracks": tracks,
            "Objectives": objectives,
            "Entitlements": entitlements
        }),
    ))
}

/// `{UserId, Mode: "grandfather" | "fresh"}`, while the player is offline: grandfather puts every
/// track at its max rank, confirmed; fresh puts every track at 0 and clears the objectives.
async fn seed_progression(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = field(&body, "UserId").and_then(Value::as_str).map(str::to_owned);
    let mode = match field(&body, "Mode").and_then(Value::as_str) {
        Some("grandfather") => Some(SeedMode::Grandfather),
        Some("fresh") => Some(SeedMode::Fresh),
        _ => None,
    };
    let (Some(user), Some(mode)) = (user, mode) else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let ctx = state.progression_context();
    let id = user.clone();
    let reply = state
        .store
        .write(move |conn| realprogression::seed(conn, &ctx, &id, mode, &admin.user_id).map_err(AppError::from))
        .await?;
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let Some(tracks) = reply.body else {
        return Ok(empty(status));
    };
    Ok(json(
        status,
        json!({
            "UserId": user,
            "Mode": mode.as_str(),
            "RealMode": state.progression.is_real(Some(&user)),
            "Tracks": tracks
        }),
    ))
}

/// `{UserId, Entitlement, Duration (hours, 0 = permanent)}` -> the account's list.
async fn grant(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = field(&body, "UserId").and_then(Value::as_str).map(str::to_owned);
    let name = field(&body, "Entitlement")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    let duration = match field(&body, "Duration") {
        None | Some(Value::Null) => Some(0),
        value => safe_integer(value).filter(|d| *d >= 0),
    };
    let (Some(user), Some(name), Some(duration)) = (user, name, duration) else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let defaults = state.entitlement_defaults.clone();
    let source = format!("admin:{}", admin.user_id);
    let (id, request) = (user.clone(), body.clone());
    let list = state
        .store
        .write(move |conn| {
            if !account_exists(conn, &id)? {
                return Ok::<_, AppError>(None);
            }
            let list = entitlements::grant(conn, &defaults, &id, &name, duration, &source)?;
            let reply = Value::Array(list.clone());
            let event = Event {
                account: &id,
                caller: "admin",
                route: "admin GrantEntitlement",
                body: request.as_ref(),
                status: 200,
                reply: Some(&reply),
                notes: &[],
            };
            record_event(conn, event)?;
            Ok(Some(list))
        })
        .await?;
    match list {
        None => Ok(empty(StatusCode::NOT_FOUND)),
        Some(list) => Ok(json(StatusCode::OK, json!({"UserId": user, "Entitlements": list}))),
    }
}

/// `{UserId, Entitlement}`; a revoked default stays revoked until granted again.
async fn revoke(
    State(state): State<AppState>,
    _admin: AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let user = field(&body, "UserId").and_then(Value::as_str).map(str::to_owned);
    let name = field(&body, "Entitlement")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    let (Some(user), Some(name)) = (user, name) else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let defaults = state.entitlement_defaults.clone();
    let (id, request) = (user.clone(), body.clone());
    let revoked = state
        .store
        .write(move |conn| {
            if !account_exists(conn, &id)? {
                return Ok::<_, AppError>(None);
            }
            let revoked = entitlements::revoke(conn, &defaults, &id, &name)?;
            let note = if revoked { "revoked" } else { "not owned" }.to_owned();
            let event = Event {
                account: &id,
                caller: "admin",
                route: "admin RevokeEntitlement",
                body: request.as_ref(),
                status: 200,
                reply: None,
                notes: &[note],
            };
            record_event(conn, event)?;
            Ok(Some(revoked))
        })
        .await?;
    let Some(revoked) = revoked else {
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    let defaults = state.entitlement_defaults.clone();
    let id = user.clone();
    let list = state
        .store
        .write(move |conn| entitlements::list(conn, &defaults, &id).map_err(AppError::from))
        .await?;
    Ok(json(
        StatusCode::OK,
        json!({"UserId": user, "Revoked": revoked, "Entitlements": list}),
    ))
}
