//! `routes/matchmaking.ts`: the client's matchmaking (queue, status poll, party candidates) and the
//! game servers' expected-player check. State: `matchmaking/`.

use axum::extract::{RawQuery, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json, query};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, metagame_auth};
use crate::matchmaking::{self, Decision, Entry, roster};
use crate::time::now_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .delete("/candidate", cancel)
        .delete("/candidate/leave", leave)
        .post("/candidate/player/alive", alive)
        .post("/candidate/player/register", register)
        .get("/candidate/regions", regions)
        .post("/key/generate", generate_key)
        .get("/candidate/status", status)
        .post("/candidate/join", join)
        .post("/candidate/join/:candidateId", join_candidate)
        .get("/QoS", qos)
}

/// `${req.AuthData.userId}`.
fn shown(auth: &MetagameAuth) -> &str {
    auth.user_id().unwrap_or("undefined")
}

/// Off unless `MATCHMAKING_CANCEL=1` (`CancelOn`): the client sends DELETE /candidate right after
/// every queued join, and those hunts only started because the cancel 404'd.
async fn cancel(State(state): State<AppState>, parts: Parts) -> Response {
    if !state.matchmaking.settings.cancel {
        return fall_through(&parts);
    }
    let auth = match metagame_auth(&parts, &state).await {
        Ok(auth) => auth,
        Err(refused) => return refused,
    };
    let cancelled = auth
        .user_id()
        .and_then(|user| matchmaking::cancel(&mut state.parties.lock(), user, now_ms()));
    let user = shown(&auth);
    match cancelled {
        Some(entry) => tracing::info!(
            "userId {user} cancelled matchmaking for {}{}",
            entry.hunt_text(),
            if entry.ready {
                " (a server was already assigned)"
            } else {
                ""
            }
        ),
        None => tracing::info!("userId {user} cancelled matchmaking but was not queued"),
    }
    json(StatusCode::OK, json!({}))
}

/// The caller alone leaves the candidate; same switch as DELETE /candidate.
async fn leave(State(state): State<AppState>, parts: Parts) -> Response {
    if !state.matchmaking.settings.cancel {
        return fall_through(&parts);
    }
    let auth = match metagame_auth(&parts, &state).await {
        Ok(auth) => auth,
        Err(refused) => return refused,
    };
    let left = auth
        .user_id()
        .and_then(|user| matchmaking::leave(&mut state.parties.lock(), user, now_ms()));
    let user = shown(&auth);
    match left {
        Some(entry) => tracing::info!("userId {user} left matchmaking for {}", entry.hunt_text()),
        None => tracing::info!("userId {user} left matchmaking but was not queued"),
    }
    json(StatusCode::OK, json!({}))
}

/// `NativeSessionId`: a UUID, lowercased.
fn native_session_id(value: Option<&str>) -> Option<String> {
    let value = value?;
    let groups: Vec<&str> = value.split('-').collect();
    let shape = groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()));
    shape.then(|| value.to_ascii_lowercase())
}

/// `ConnectedPlayerSnapshot`: a JSON array of distinct printable ids, or nothing.
fn connected_snapshot(value: Option<&str>) -> Option<Vec<String>> {
    let value = value.filter(|v| v.len() <= 8192)?;
    let Ok(Value::Array(ids)) = service_kit::jsjson::parse(value) else {
        return None;
    };
    if ids.len() > 128 {
        return None;
    }
    let mut seen: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids {
        let Value::String(id) = id else {
            return None;
        };
        if !(1..=128).contains(&id.len()) || !id.bytes().all(|b| (0x21..=0x7e).contains(&b)) || seen.contains(&id) {
            return None;
        }
        seen.push(id);
    }
    Some(seen)
}

/// A header as Node reads it: repeated values joined with ", ".
fn header_text(parts: &Parts, name: &str) -> Option<String> {
    let values: Vec<&str> = parts
        .headers
        .get_all(name)
        .iter()
        .map(|v| v.to_str().unwrap_or(""))
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

/// The game server's expected-player check (`MiscRoutesOn`). Only a complete connection snapshot
/// from the session's own deployment can tell a missing player from a connected one who stopped
/// matchmaking; otherwise the list comes back as sent.
async fn alive(
    State(state): State<AppState>,
    parts: Parts,
    RawQuery(raw): RawQuery,
    ExpressBody(body): ExpressBody,
) -> Response {
    if !state.features.misc_routes {
        return fall_through(&parts);
    }
    let auth = match metagame_auth(&parts, &state).await {
        Ok(auth) => auth,
        Err(refused) => return refused,
    };
    let players: Vec<String> = match super::field(&body, "playerIds") {
        Some(Value::Array(ids)) => ids.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        _ => Vec::new(),
    };
    let query = query(raw.as_deref());
    let session_header = header_text(&parts, "x-dauntless-game-session-id");
    let header_session = native_session_id(session_header.as_deref());
    let query_value = query.get("sessionId");
    let query_session = native_session_id(query_value.and_then(Value::as_str));
    let session = if session_header.is_none() {
        query_session.clone()
    } else {
        header_session.clone()
    };
    let matching = query_value.is_none() || query_session == session;
    let purpose = query.get("purpose");
    let expected =
        purpose.and_then(Value::as_str) == Some("expected") || (header_session.is_some() && purpose.is_none());
    let expected_players = match session {
        Some(session) if auth.is_game_server() && matching && expected => {
            let connected = if header_session.is_some() {
                connected_snapshot(header_text(&parts, "x-dauntless-connected-player-ids").as_deref())
            } else {
                None
            };
            let context = roster::Context {
                session_id: Some(session),
                allocation_id: None,
                connected_player_ids: connected,
            };
            matchmaking::reconcile_expected(&mut state.parties.lock(), &players, &context, now_ms())
        }
        _ => players,
    };
    json(StatusCode::OK, json!({"expectedPlayerIds": expected_players}))
}

async fn register(auth: MetagameAuth) -> Response {
    tracing::info!("userId {} is registering for matchmaking!", shown(&auth));
    json(StatusCode::OK, json!({}))
}

async fn regions(State(state): State<AppState>, auth: MetagameAuth) -> Response {
    tracing::info!("Querying regions for QoS (userId {})", shown(&auth));
    let target = state
        .matchmaking
        .settings
        .qos_target_url
        .clone()
        .map_or(Value::Null, Value::String);
    json(
        StatusCode::OK,
        json!({
            "code": 200,
            "message": "success",
            "payload": {
                "maxPingingStepTime": 3,
                "pingCount": 5,
                "pingFrequency": 0.25,
                "regionUrls": [target]
            }
        }),
    )
}

async fn generate_key(_auth: MetagameAuth) -> Response {
    empty(StatusCode::BAD_REQUEST)
}

/// The status poll's period: private matches and party candidates poll every second.
fn period(entry: &Entry) -> i64 {
    if entry.private || entry.party_candidate {
        1000
    } else {
        10000
    }
}

/// `PlayerStatesOf`: a party candidate lists its members by id; solo replies keep upstream's
/// literal "UserId" key.
fn player_states(entry: &Entry) -> Value {
    match &entry.party_member_ids {
        Some(members) => Value::Object(members.iter().map(|m| (m.clone(), json!({}))).collect()),
        None => json!({"UserId": {}}),
    }
}

/// The fields every status reply starts with, `huntId` left out when the join named none.
fn status_reply(entry: &Entry) -> Map<String, Value> {
    let mut reply = Map::new();
    reply.insert("candidateId".into(), json!(entry.candidate_id));
    reply.insert("candidateStatusPeriodMillis".into(), json!(period(entry)));
    reply.insert("gameMode".into(), json!("ISLAND"));
    if let Some(hunt) = &entry.hunt_id {
        reply.insert("huntId".into(), hunt.clone());
    }
    reply.insert("playerStates".into(), player_states(entry));
    reply
}

async fn status(State(state): State<AppState>, auth: MetagameAuth) -> Response {
    let user = shown(&auth).to_owned();
    let decision = match auth.user_id() {
        Some(id) => matchmaking::decide_status(&state, id).await,
        None => Decision::Unknown,
    };
    let party = |entry: &Entry| {
        if entry.party_candidate {
            format!(", party candidate {}", entry.candidate_id)
        } else {
            String::new()
        }
    };
    let mut reply = match &decision {
        Decision::Unknown => {
            tracing::error!("UserId {user} was not found in the MatchmakingMap");
            return empty(StatusCode::NOT_FOUND);
        }
        Decision::Failed(entry) => {
            // A status the client knows ends matchmaking; the old reply sent it to ":0"
            tracing::warn!(
                "Telling userId {user} matchmaking FAILED for {}{}",
                entry.hunt_text(),
                party(entry)
            );
            let mut reply = status_reply(entry);
            reply.insert("status".into(), json!("FAILED"));
            reply
        }
        Decision::Travel(entry) => {
            tracing::info!(
                "Telling client to travel to {}:{} (userId {user}{})",
                entry.host,
                entry.port,
                party(entry)
            );
            let changelist = state
                .matchmaking
                .settings
                .target_changelist
                .as_deref()
                .unwrap_or("undefined");
            let mut reply = status_reply(entry);
            reply.insert(
                "serverInfo".into(),
                json!({
                    "buildId": format!("{changelist}_1.4.4_shipping"),
                    "gameSessionId": entry.game_session(),
                    "host": entry.host,
                    "port": entry.port
                }),
            );
            reply.insert("status".into(), json!("IN_PROGRESS"));
            reply
        }
        Decision::Matching { entry, parked } => {
            tracing::info!(
                "MM not ready yet! (userId {user}{}{})",
                party(entry),
                if *parked { ", joined again after being sent" } else { "" }
            );
            let mut reply = status_reply(entry);
            reply.insert("status".into(), json!("MATCHING"));
            reply
        }
    };
    reply.insert("statusDuration".into(), json!(0.0));
    reply.insert("statusReason".into(), Value::Null);
    json(StatusCode::OK, Value::Object(reply))
}

async fn join(
    State(state): State<AppState>,
    auth: MetagameAuth,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    // Node read `req.body.gameMode` of a request without a body: a TypeError, the 500 page
    let body = body
        .ok_or_else(|| AppError::Other("TypeError: Cannot read properties of undefined (reading 'gameMode')".into()))?;
    let read = |name: &str| body.get(name).cloned();
    let (mode, args, hunt) = (read("gameMode"), read("gameArgs"), read("playerHuntId"));
    let user = shown(&auth).to_owned();
    tracing::info!(
        "UserId {user} wants to join a game with GameMode {} & GameArgs {} & HuntId {}",
        crate::js::string(mode.as_ref()),
        crate::js::string(args.as_ref()),
        crate::js::string(hunt.as_ref())
    );
    let private =
        body.get("isPrivate") == Some(&Value::Bool(true)) || body.get("privateMatch") == Some(&Value::Bool(true));
    let Some(id) = auth.user_id() else {
        // A game server key without a player token has no account to queue
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    if !matchmaking::join(&state, mode.clone(), args, hunt.clone(), id, private).await? {
        return Ok(empty(StatusCode::BAD_REQUEST));
    }
    let Some(entry) = matchmaking::check_and_update(&state, id).await else {
        tracing::error!("UserId {user} has no matchmaking entry after joining");
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let mut reply = Map::new();
    reply.insert("candidateId".into(), json!(entry.candidate_id));
    reply.insert("candidateStatusPeriodMillis".into(), json!(period(&entry)));
    if let Some(mode) = mode {
        reply.insert("gameMode".into(), mode);
    }
    if let Some(hunt) = hunt {
        reply.insert("huntId".into(), hunt);
    }
    reply.insert("status".into(), json!("MATCHING"));
    reply.insert("statusReason".into(), Value::Null);
    Ok(json(StatusCode::OK, Value::Object(reply)))
}

/// A party member following the leader's candidate; anything else keeps the 404 it always got.
async fn join_candidate(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
) -> Result<Response, AppError> {
    let wanted = params.get("candidateId").unwrap_or("");
    let user = shown(&auth).to_owned();
    let candidate = match auth.user_id() {
        Some(id) => matchmaking::join_candidate(&state, id, wanted).await?,
        None => None,
    };
    let Some(candidate) = candidate else {
        let shown: String = wanted.chars().take(64).collect();
        tracing::warn!("UserId {user} asked to join candidate {shown}, which is not their party's");
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    tracing::info!(
        "UserId {user} joins their party's candidate {} ({} {})",
        candidate.candidate_id,
        candidate.game_mode,
        candidate.hunt_id
    );
    Ok(json(
        StatusCode::OK,
        json!({
            "candidateId": candidate.candidate_id,
            "candidateStatusPeriodMillis": 1000,
            "gameMode": candidate.game_mode,
            "huntId": candidate.hunt_id,
            "status": "MATCHING",
            "statusReason": null
        }),
    ))
}

async fn qos() -> Response {
    tracing::info!("QoS Ping");
    let mut response = (StatusCode::OK, "<!DOCTYPE html><html><body>pong</body></html>").into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}
