//! `routes/progression.ts`: encountered content, breadcrumbs, objectives and progress tracks. Real-mode
//! accounts get stored progression (`realprogression.rs`); the others keep upstream's stub, whose
//! mastery ranks are the configured maximum. The real-mode-only routes fall through to the catch-all
//! 404 for everyone else, before any auth.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use std::sync::OnceLock;

use axum::extract::{FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use serde_json::{Map, Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::enforce;
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json, ok};
use crate::app::{AppError, AppState};
use crate::auth::{AdminKey, MetagameAuth, metagame_auth};
use crate::characterprogress::{self as content, Fail};
use crate::limits::per_client;
use crate::progression::{Caller, note_relayed_mismatch, real_reply};
use crate::realprogression;

const STUB_MAX_PROGRESS: i64 = 99_999_999;
const STUB_SEASON_RANK: i64 = 99_999_999;
const STUB_MASTERY_TRACKS: [&str; 9] = [
    "MasteryTrack_PlayerLevel",
    "MasteryTrack_Behemoth",
    "MasteryTrack_Weapon_Strikers",
    "MasteryTrack_Weapon_Hammer",
    "MasteryTrack_Weapon_Repeaters",
    "MasteryTrack_Weapon_ChainBlades",
    "MasteryTrack_Weapon_Axe",
    "MasteryTrack_Weapon_Sword",
    "MasteryTrack_Weapon_Spear",
];

pub fn routes(state: &AppState) -> Router<AppState> {
    let admin_limit = per_client(&state.limits.admin_mutation, state.origin.clone());
    Router::new()
        .get("/encountered-content/:characterId/:contentType", encountered)
        .post("/encountered-content/query/:characterId", query_encountered)
        .post("/encountered-content/:characterId", add_encountered)
        .get("/progression/objectives/:userId", objectives)
        .get("/progression/objectives/:userId/:objectiveId", objective)
        .get("/breadcrumbs/:characterId", get_breadcrumbs)
        .post("/breadcrumbs/:characterId", set_breadcrumbs)
        .post("/progression/:userId", grant)
        .post("/progression/:userId/:progressionId/:rank/confirm/:kind", confirm)
        .post("/progression/:userId/:progressionId/:amount", grant_in_track)
        .get("/progression/:userId/:progressionId", track)
        .delete("/progression/:userId/:progressionId", reset)
        .layer(from_fn_with_state(admin_limit, enforce))
        .get("/progression/:userId", tracks)
}

// ---- The stub ----

/// When the stub's ranks were confirmed: when the server started (`STUB_CONFIRMED_DATE`).
fn stub_confirmed_date() -> &'static str {
    static STARTED: OnceLock<String> = OnceLock::new();
    STARTED.get_or_init(crate::time::iso_now)
}

/// `GetConfiguredMaxRank`: the stub's mastery ranks are the configured maximum.
fn configured_max_rank(state: &AppState, id: &str) -> i64 {
    match state.progression_config.path(id) {
        Some(path) if !path.requirements.is_empty() => {
            path.requirements.iter().map(|r| r.rank_id as i64).max().unwrap_or(0)
        }
        _ => STUB_SEASON_RANK,
    }
}

/// A stub track, with `phx_account_id` first (left out for a request without a player, as
/// `JSON.stringify` leaves out undefined).
fn stub_track(account: Option<&str>, id: &str, rank: i64) -> Value {
    let mut track = Map::new();
    if let Some(account) = account {
        track.insert("phx_account_id".into(), json!(account));
    }
    track.insert("progression_id".into(), json!(id));
    track.insert("progress".into(), json!(STUB_MAX_PROGRESS));
    track.insert("confirmed_fremium_rank".into(), json!(rank));
    track.insert("confirmed_premium_rank".into(), json!(rank));
    track.insert("confirmed_date".into(), json!(stub_confirmed_date()));
    Value::Object(track)
}

fn stub_mastery_tracks(state: &AppState, account: Option<&str>) -> Vec<Value> {
    STUB_MASTERY_TRACKS
        .iter()
        .map(|id| stub_track(account, id, configured_max_rank(state, id)))
        .collect()
}

/// `new Date("1970-1-1").toISOString()`: JavaScript reads that form as local midnight.
fn local_epoch_day() -> String {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(1970, 1, 1, 0, 0, 0)
        .single()
        .map_or_else(|| crate::time::iso(0), |t| crate::time::iso(t.timestamp_millis()))
}

// ---- Shared checks ----

/// `RefuseForeignPlayer`: a game server names the player in the URL and is trusted; a player client
/// only gets its own account.
fn refuse_foreign(auth: &MetagameAuth, parts: &Parts, account: &str) -> Option<Response> {
    if auth.is_game_server() || auth.user_id() == Some(account) {
        return None;
    }
    tracing::warn!(
        "Refusing {} {} to userId {}: another player's account",
        parts.method,
        parts.uri.path(),
        auth.user_id().unwrap_or("undefined")
    );
    Some(empty(StatusCode::FORBIDDEN))
}

fn caller(auth: &MetagameAuth) -> Caller {
    if auth.is_game_server() {
        Caller::GameServer
    } else {
        Caller::Client
    }
}

fn failed(fail: &Fail) -> Response {
    match fail {
        Fail::Conflict(message) => tracing::warn!("{message}"),
        Fail::Forbidden(message) | Fail::InvalidData(message) | Fail::Db(message) => tracing::error!("{message}"),
    }
    empty(StatusCode::from_u16(fail.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
}

fn content_reply(payload: Value) -> Response {
    json(
        StatusCode::OK,
        json!({"code": null, "message": "OK", "payload": payload}),
    )
}

/// Node read `req.body.x` of a request without a body: a TypeError, the 500 page.
fn body_field<'a>(body: &'a Option<Value>, name: &str, route: &str) -> Result<Option<&'a Value>, AppError> {
    match body {
        Some(Value::Object(map)) => Ok(map.get(name)),
        Some(Value::Null) | None => Err(AppError::Other(format!(
            "TypeError: Cannot read properties of undefined (reading '{name}') in {route}"
        ))),
        Some(_) => Ok(None),
    }
}

// ---- Encountered content and breadcrumbs ----

async fn encountered(State(state): State<AppState>, auth: MetagameAuth, params: Params) -> Result<Response, AppError> {
    let user = auth.user_id().map(str::to_owned);
    let character = params.get("characterId").unwrap_or("").to_owned();
    let category = json!([params.get("contentType").unwrap_or("")]);
    query(&state, user, character, category).await
}

async fn query(
    state: &AppState,
    user: Option<String>,
    character: String,
    categories: Value,
) -> Result<Response, AppError> {
    let shown = user.as_deref().unwrap_or("undefined");
    tracing::info!("Querying encountered content for userId {shown} and characterId {character}");
    let count = match &categories {
        Value::Array(list) => list.len().to_string(),
        Value::String(text) => text.encode_utf16().count().to_string(),
        _ => "undefined".into(),
    };
    tracing::info!("Querying {count} categories for userId {shown} and characterId {character}");
    let result = state
        .store
        .read(move |conn| {
            content::check_owner(conn, user.as_deref(), &character)?;
            content::query_encountered(conn, user.as_deref().unwrap_or(""), &character, &categories)
        })
        .await;
    Ok(match result {
        Ok(types) => content_reply(json!({"content_types": types, "success": true})),
        Err(fail) => failed(&fail),
    })
}

async fn query_encountered(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let route = "POST /encountered-content/query";
    let categories = match body_field(&body, "content_types", route)? {
        // `categoriesToQuery.length` of null or undefined: the 500 page
        None | Some(Value::Null) => {
            return Err(AppError::Other(format!(
                "TypeError: Cannot read properties of undefined (reading 'length') in {route}"
            )));
        }
        Some(value) => value.clone(),
    };
    let user = auth.user_id().map(str::to_owned);
    let character = params.get("characterId").unwrap_or("").to_owned();
    query(&state, user, character, categories).await
}

async fn add_encountered(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let route = "POST /encountered-content";
    let content_type = body_field(&body, "content_type", route)?.cloned();
    let content_id = body_field(&body, "content_id", route)?.cloned();
    let user = auth.user_id().map(str::to_owned);
    let character = params.get("characterId").unwrap_or("").to_owned();
    tracing::info!(
        "Adding encountered content {} for userId {} and characterId {character}",
        crate::js::string(content_id.as_ref()),
        user.as_deref().unwrap_or("undefined")
    );
    let result = state
        .store
        .write(move |conn| {
            content::check_owner(conn, user.as_deref(), &character)?;
            content::add_encountered(
                conn,
                user.as_deref().unwrap_or(""),
                &character,
                content_type.as_ref(),
                content_id.as_ref(),
            )
        })
        .await;
    Ok(match result {
        Ok(()) => content_reply(json!({})),
        Err(fail) => failed(&fail),
    })
}

async fn get_breadcrumbs(State(state): State<AppState>, auth: MetagameAuth, params: Params) -> Response {
    let user = auth.user_id().map(str::to_owned);
    let character = params.get("characterId").unwrap_or("").to_owned();
    tracing::info!("Requested breadcrumbs for characterId {character}");
    let result = state
        .store
        .write(move |conn| {
            content::check_owner(conn, user.as_deref(), &character)?;
            content::get_breadcrumbs(conn, user.as_deref().unwrap_or(""), &character)
        })
        .await;
    match result {
        Ok(payload) => content_reply(payload),
        Err(fail) => failed(&fail),
    }
}

async fn set_breadcrumbs(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let route = "POST /breadcrumbs";
    let list = body_field(&body, "breadcrumbs", route)?.cloned();
    let version = body_field(&body, "updateVersion", route)?.cloned();
    let user = auth.user_id().map(str::to_owned);
    let character = params.get("characterId").unwrap_or("").to_owned();
    tracing::info!("Setting breadcrumbs for characterId {character}");
    let result = state
        .store
        .write(move |conn| {
            content::check_owner(conn, user.as_deref(), &character)?;
            content::set_breadcrumbs(
                conn,
                user.as_deref().unwrap_or(""),
                &character,
                list.as_ref(),
                version.as_ref(),
            )
        })
        .await;
    Ok(match result {
        Ok(payload) => content_reply(payload),
        Err(fail) => failed(&fail),
    })
}

// ---- Objectives and tracks ----

async fn objectives(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    // Real mode: every stored objective as a payload array (int code). The stub's payload object
    // fails the client's array check, so it never had any objectives.
    if state.progression.is_real(Some(&account)) {
        if let Some(refusal) = refuse_foreign(&auth, &parts, &account) {
            return Ok(refusal);
        }
        let id = account.clone();
        let list = state
            .store
            .read(move |conn| realprogression::objective_records(conn, &id).map_err(AppError::from))
            .await?;
        tracing::info!(
            "Objective progression fetched for userId {account}: {} objective(s)",
            list.len()
        );
        return Ok(ok(json!({"code": 200, "message": "OK", "payload": list})));
    }
    tracing::info!(
        "Objective progression fetched for userId {}",
        auth.user_id().unwrap_or("undefined")
    );
    Ok(ok(json!({
        "code": null,
        "message": "OK",
        "payload": {"objectives": [], "progress_tracks": stub_mastery_tracks(&state, auth.user_id())}
    })))
}

async fn objective(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    let id = params.get("objectiveId").unwrap_or("").to_owned();
    // Real mode: the stored objective, or zeros. The client asks for each objective of a grant and
    // raises the "objective updated" event for every answer it gets.
    if state.progression.is_real(Some(&account)) {
        if let Some(refusal) = refuse_foreign(&auth, &parts, &account) {
            return Ok(refusal);
        }
        tracing::info!("Objective {id} fetched for userId {account}");
        let record = state
            .store
            .read(move |conn| realprogression::objective_record_of(conn, &account, &id).map_err(AppError::from))
            .await?;
        return Ok(ok(json!({"code": "OK", "message": "OK", "payload": record})));
    }
    tracing::info!(
        "Objective progression fetched for userId {}",
        auth.user_id().unwrap_or("undefined")
    );
    let day = local_epoch_day();
    Ok(ok(json!({
        "code": null,
        "message": "OK",
        "payload": {
            "phx_account_id": account,
            "objective_id": id,
            "progress": 9_999_999,
            "completed_count": 9_999_999,
            "created_date": day,
            "last_modified_date": day,
        }
    })))
}

/// Real mode: store the grant and objectives, answer with the new totals. The pop that made upstream
/// answer 400 came from never storing objectives: every successful grant made the client re-read
/// unchanged zeros and notify again.
async fn grant(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        tracing::info!("Progression set for userId {account} (stubbed)");
        // Anything but 400 here brings back the infinite mastery pop
        return Ok(empty(StatusCode::BAD_REQUEST));
    }
    if !auth.is_game_server() {
        tracing::warn!("Refusing progression grant for {account} from a player client");
        return Ok(empty(StatusCode::FORBIDDEN));
    }
    note_relayed_mismatch(&auth, Some(&account), "progression grant");
    let (ctx, who) = (state.progression_context(), caller(&auth));
    let reply = state
        .store
        .write(move |conn| realprogression::grant(conn, &ctx, &account, body.as_ref(), who).map_err(AppError::from))
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

/// The real-mode-only routes: `RealProgressionOnly` passes everyone else on before auth.
async fn real_auth(state: &AppState, parts: &Parts, account: &str) -> Result<MetagameAuth, Response> {
    if !state.progression.is_real(Some(account)) {
        return Err(fall_through(parts));
    }
    metagame_auth(parts, state).await
}

fn game_server_only(auth: &MetagameAuth, account: &str, refused: &str, noted: &str) -> Option<Response> {
    if !auth.is_game_server() {
        tracing::warn!("{refused}");
        return Some(empty(StatusCode::FORBIDDEN));
    }
    note_relayed_mismatch(auth, Some(account), noted);
    None
}

async fn confirm(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(fall_through(&parts));
    }
    // PROGRESSION_CONFIRM=off gives real-mode accounts the old 404 here
    if state.real_progression.confirm_off {
        return Ok(fall_through(&parts));
    }
    let auth = match metagame_auth(&parts, &state).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    if let Some(refusal) = game_server_only(
        &auth,
        &account,
        &format!("Refusing rank confirm for {account} from a player client"),
        "rank confirm",
    ) {
        return Ok(refusal);
    }
    let id = params.get("progressionId").unwrap_or("").to_owned();
    let rank = params.get("rank").unwrap_or("").to_owned();
    let kind = params.get("kind").unwrap_or("").to_owned();
    let (ctx, who) = (state.progression_context(), caller(&auth));
    let reply = state
        .store
        .write(move |conn| {
            realprogression::confirm(conn, &ctx, &account, &id, &rank, &kind, who).map_err(AppError::from)
        })
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

async fn grant_in_track(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    let auth = match real_auth(&state, &parts, &account).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    let id = params.get("progressionId").unwrap_or("").to_owned();
    if let Some(refusal) = game_server_only(
        &auth,
        &account,
        &format!("Refusing progression grant in {id} for {account} from a player client"),
        "progression grant in a track",
    ) {
        return Ok(refusal);
    }
    let amount = params.get("amount").unwrap_or("").to_owned();
    let (ctx, who) = (state.progression_context(), caller(&auth));
    let reply = state
        .store
        .write(move |conn| {
            realprogression::grant_in_track(conn, &ctx, &account, &id, &amount, who).map_err(AppError::from)
        })
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

async fn track(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    let auth = match real_auth(&state, &parts, &account).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    if let Some(refusal) = refuse_foreign(&auth, &parts, &account) {
        return Ok(refusal);
    }
    let id = params.get("progressionId").unwrap_or("").to_owned();
    let config = state.progression_config.clone();
    let record = state
        .store
        .read(move |conn| realprogression::track_record_of(conn, &config, &account, &id).map_err(AppError::from))
        .await?;
    Ok(match record {
        Some(record) => ok(json!({"code": "OK", "message": "OK", "payload": record})),
        None => empty(StatusCode::NOT_FOUND),
    })
}

/// Resets one track. The game server only sends it from a debug command, so it needs an admin key
/// (x-undaunted-user-api-key), or `PROGRESSION_ALLOW_DELETE=1` for game servers.
async fn reset(State(state): State<AppState>, mut parts: Parts, params: Params) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(fall_through(&parts));
    }
    let id = params.get("progressionId").unwrap_or("").to_owned();
    let who = if parts.headers.contains_key("x-undaunted-user-api-key") {
        if let Err(refusal) = AdminKey::from_request_parts(&mut parts, &state).await {
            return Ok(refusal);
        }
        Caller::Admin
    } else {
        let auth = match metagame_auth(&parts, &state).await {
            Ok(auth) => auth,
            Err(refusal) => return Ok(refusal),
        };
        if !auth.is_game_server() || !state.real_progression.allow_delete {
            tracing::warn!(
                "Refusing reset of track {id} of {account}: needs an admin key, or PROGRESSION_ALLOW_DELETE=1 for game servers"
            );
            return Ok(empty(StatusCode::FORBIDDEN));
        }
        caller(&auth)
    };
    let ctx = state.progression_context();
    let reply = state
        .store
        .write(move |conn| realprogression::reset(conn, &ctx, &account, &id, who).map_err(AppError::from))
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

async fn tracks(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    // Real mode: every configured track, stored or at 0, with phx_account_id = the URL account (the
    // stub uses the token's user, which a game server may not carry)
    if state.progression.is_real(Some(&account)) {
        if let Some(refusal) = refuse_foreign(&auth, &parts, &account) {
            return Ok(refusal);
        }
        tracing::info!("Progression fetched for userId {account}");
        let config = state.progression_config.clone();
        let records = state
            .store
            .read(move |conn| realprogression::track_records(conn, &config, &account).map_err(AppError::from))
            .await?;
        return Ok(ok(json!({"code": 200, "message": "OK", "payload": records})));
    }
    let user = auth.user_id();
    tracing::info!(
        "Progression fetched for userId {} (stubbed)",
        user.unwrap_or("undefined")
    );
    let mut payload = vec![stub_track(user, "season09b", STUB_SEASON_RANK)];
    payload.extend(stub_mastery_tracks(&state, user));
    Ok(ok(json!({"code": null, "message": "OK", "payload": payload})))
}
