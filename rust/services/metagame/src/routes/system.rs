//! `routes/system.ts`: the status banner, heartbeats, the leaderboard profile, entitlements, the
//! progression config, Hunt Pass, cooldowns, bounties and the client's stubbed odds and ends.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json, ok};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, metagame_auth};
use crate::entitlements;
use crate::progression::{
    Caller, Event, account_exists, read_field, read_integer, real_reply, record_event, refuse_unless_game_server,
};
use crate::serverstatus;

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/dauntless-status", dauntless_status)
        .post("/heartbeat", heartbeat)
        .post("/event", event)
        .post("/account/migrate", account_migrate)
        .post("/profile/update", update_profile)
        .get("/vivox/login", vivox_login)
        .post("/motd/", motd)
        .get("/entitlementsv2", list)
        .post("/entitlementv2/:userId", grant)
        .delete("/entitlement/:userId/:entitlement", revoke)
        .get("/playertreatments/:userId", player_treatments)
        .get("/eventstats/", event_stats)
        .get("/progression/config", progression_config)
        .get("/huntpass/:userId", hunt_pass)
        .post("/huntpass/:userId", set_hunt_pass)
        .get("/cooldown/:userId", cooldowns)
        .put("/cooldown/batch/:userId", cooldown_batch)
        // After /cooldown/batch/:userId, which it would otherwise swallow
        .put("/cooldown/:userId/:cooldownId", start_cooldown)
        .get("/bounty/game-data", bounty_game_data)
        .get("/bounty/:userId", bounties)
        .post("/bounty/delete/:userId", delete_bounties)
        .post("/bounty/:userId", set_bounties)
        .get("/all/", mailbox)
        .get("/motd/trigger", motd_trigger)
}

/// The client reads exactly the first nine fields by name, so the fields after them are for people
/// and scripts: the server's name and the source it runs. Nothing about players: this answers
/// anyone. `STATUS_EXTRA=0` answers the nine fields alone.
async fn dauntless_status(State(state): State<AppState>) -> Response {
    tracing::info!("Status");
    let identity = &state.server_status;
    let mut reply = serde_json::Map::new();
    reply.insert("show-status".into(), json!(true));
    reply.extend(serverstatus::welcome(&identity.name));
    if identity.status_extra {
        reply.extend(identity.identity());
    }
    ok(Value::Object(reply))
}

/// A game server's own heartbeat carries no player token: there is no player to record. A
/// player's heartbeat also keeps them in their party.
async fn heartbeat(
    State(state): State<AppState>,
    parts: Parts,
    auth: MetagameAuth,
    ExpressBody(body): ExpressBody,
) -> Response {
    if let Some(user) = auth.user_id() {
        let now = crate::time::now_ms();
        if !auth.is_game_server() {
            let address = crate::auth::origin(&parts).client_address(&state.origin);
            if !address.is_loopback() {
                state.activity.lock().record_address(user, address.to_string(), now);
            }
        }
        let map = body.as_ref().and_then(|b| b.get("map")).cloned();
        let beat_state = body.as_ref().and_then(|b| b.get("state"));
        state.activity.lock().heartbeat(user, map, beat_state, now);
        state.parties.lock().touch(user, now);
    }
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "20000",
    )
        .into_response()
}

async fn event() -> Response {
    ok(json!({}))
}

async fn account_migrate(_auth: MetagameAuth) -> Response {
    tracing::info!("Account migration (stubbed)");
    ok(json!({"migration_failed": false, "migration_finished": true}))
}

async fn vivox_login(_auth: MetagameAuth) -> Response {
    tracing::info!("Vivox login (stubbed)");
    empty(StatusCode::NOT_FOUND)
}

async fn motd(_auth: MetagameAuth) -> Response {
    tracing::info!("MOTD (stubbed)");
    empty(StatusCode::NO_CONTENT)
}

async fn player_treatments(_auth: MetagameAuth) -> Response {
    tracing::info!("Cohorts (stubbed)");
    ok(json!({"treatments": ["CohortTreatment.Dojo.B"]}))
}

async fn event_stats(_auth: MetagameAuth) -> Response {
    tracing::info!("Event stats (stubbed)");
    ok(json!({"stats": []}))
}

async fn mailbox(_auth: MetagameAuth) -> Response {
    tracing::info!("Mailbox (stubbed)");
    ok(json!({"code": null, "message": "OK", "payload": {"messages": []}}))
}

/// After-hunt news. The client always asks for `event_name=test`; a 204 takes the same "no news"
/// path as the old 404. `MISC_ROUTES=0` puts the 404 back.
async fn motd_trigger(State(state): State<AppState>, parts: Parts) -> Response {
    if !state.features.misc_routes {
        return fall_through(&parts);
    }
    empty(StatusCode::NO_CONTENT)
}

fn stub() -> Response {
    tracing::info!("Entitlements (stubbed)");
    json(StatusCode::OK, json!({"code": null, "message": "OK", "payload": []}))
}

fn caller(auth: &MetagameAuth) -> Caller {
    if auth.is_game_server() {
        Caller::GameServer
    } else {
        Caller::Client
    }
}

/// Real mode: the token's account, as a flat `{entitlements}` list (expired ones left out).
async fn list(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    let user_id = auth.user_id().map(str::to_owned);
    if state.progression.is_real(user_id.as_deref()) {
        let (id, defaults) = (user_id.clone().unwrap_or_default(), state.entitlement_defaults.clone());
        let list = state
            .store
            .write(move |conn| entitlements::list(conn, &defaults, &id).map_err(AppError::from))
            .await?;
        let names: Vec<&str> = list.iter().filter_map(|e| e["name"].as_str()).collect();
        let shown = if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        };
        tracing::info!("Entitlements of {}: {shown}", user_id.unwrap_or_default());
        return Ok(json(StatusCode::OK, json!({"entitlements": list})));
    }
    if auth.is_game_server() && user_id.is_none() {
        tracing::warn!(
            "GET /entitlementsv2 from a game server without the player's bearer: no account to look up, answering the stub's empty list"
        );
    }
    Ok(stub())
}

/// Real mode, game servers only: `{"entitlement": name, "duration": hours}`; the reply replaces the
/// game server's cached list, so it is the whole list after the grant.
async fn grant(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(stub());
    }
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "entitlement grant", &account) {
        return Ok(refusal);
    }
    let mut notes = Vec::new();
    let name = read_field(body.as_ref(), &["entitlement", "name"], "entitlement name", &mut notes).cloned();
    let duration_raw = read_field(body.as_ref(), &["duration"], "entitlement duration", &mut notes).cloned();
    let mut duration = read_integer(duration_raw.as_ref(), "duration", &mut notes);
    if duration_raw.is_none() {
        notes.push("no duration, taken as permanent".into());
        duration = Some(0);
    }
    let who = caller(&auth).as_str();
    let (defaults, id, request, event_notes) = (
        state.entitlement_defaults.clone(),
        account.clone(),
        body.clone(),
        notes.clone(),
    );
    let (status, reply) = state
        .store
        .write(move |conn| {
            let route = "POST /entitlementv2/:uid";
            let event = |conn: &mut store::Connection, status, reply: Option<&Value>, extra: Option<&str>| {
                let mut notes = event_notes.clone();
                notes.extend(extra.map(str::to_owned));
                record_event(
                    conn,
                    Event {
                        account: &id,
                        caller: who,
                        route,
                        body: request.as_ref(),
                        status,
                        reply,
                        notes: &notes,
                    },
                )
            };
            if !account_exists(conn, &id)? {
                event(conn, 404, None, Some("unknown account"))?;
                return Ok::<_, AppError>((404, None));
            }
            let name = match (&name, duration) {
                (Some(Value::String(name)), Some(d)) if !name.is_empty() && d >= 0 => name.clone(),
                _ => {
                    event(conn, 400, None, Some("needs an entitlement name and a duration >= 0"))?;
                    return Ok((400, None));
                }
            };
            let list = entitlements::grant(conn, &defaults, &id, &name, duration.unwrap_or(0), "gameserver")?;
            let reply = json!({"entitlements": list});
            event(conn, 200, Some(&reply), None)?;
            Ok((200, Some(reply)))
        })
        .await?;
    if !notes.is_empty() || status != 200 {
        let why = if notes.is_empty() {
            "needs an entitlement name and a duration >= 0".to_owned()
        } else {
            notes.join("; ")
        };
        tracing::warn!("Entitlement grant for {account} answered {status}: {why}");
    }
    Ok(real_reply(status, reply))
}

/// Real mode, game servers only; status only (the game server never reads the body).
async fn revoke(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(fall_through(&parts));
    }
    let auth = match metagame_auth(&parts, &state).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "entitlement revoke", &account) {
        return Ok(refusal);
    }
    let name = params.get("entitlement").unwrap_or("").to_owned();
    let who = caller(&auth).as_str();
    let defaults = state.entitlement_defaults.clone();
    state
        .store
        .write(move |conn| {
            let revoked = entitlements::revoke(conn, &defaults, &account, &name)?;
            let note = format!("{name}{}", if revoked { "" } else { " (not owned)" });
            record_event(
                conn,
                Event {
                    account: &account,
                    caller: who,
                    route: "DELETE /entitlement/:uid/:name",
                    body: None,
                    status: 200,
                    reply: None,
                    notes: &[note],
                },
            )
            .map_err(AppError::from)
        })
        .await?;
    Ok(ok(json!({})))
}

// ---- Progression config, Hunt Pass, cooldowns and bounties ----

async fn progression_config(State(state): State<AppState>, _auth: MetagameAuth) -> Response {
    tracing::info!("Progression Config (stubbed)");
    ok(state.progression_config.payload.clone())
}

/// A real-mode-only route: everyone else passes on before auth (`RealProgressionOnly`).
async fn real_only(state: &AppState, parts: &Parts, account: &str) -> Result<MetagameAuth, Response> {
    if !state.progression.is_real(Some(account)) {
        return Err(fall_through(parts));
    }
    metagame_auth(parts, state).await
}

/// Real mode: the stored selection (the payload is a string), the active Hunt Pass by default.
async fn hunt_pass(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if state.progression.is_real(Some(&account)) {
        if let Some(refusal) = super::progression::refuse_foreign(&auth, &parts, &account) {
            return Ok(refusal);
        }
        let config = state.progression_config.clone();
        let id = account.clone();
        let pass = state
            .store
            .read(move |conn| crate::realprogression::selected_hunt_pass(conn, &config, &id).map_err(AppError::from))
            .await?;
        tracing::info!("Huntpass of {account}: {pass}");
        return Ok(ok(json!({"code": "OK", "message": "OK", "payload": pass})));
    }
    tracing::info!("Huntpass (stubbed)");
    Ok(ok(
        json!({"code": null, "message": "OK", "payload": state.progression_config.active_hunt_pass}),
    ))
}

async fn set_hunt_pass(
    State(state): State<AppState>,
    parts: Parts,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    let auth = match real_only(&state, &parts, &account).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "hunt pass selection", &account) {
        return Ok(refusal);
    }
    let (ctx, who) = (state.progression_context(), super::progression::caller(&auth));
    let reply = state
        .store
        .write(move |conn| {
            crate::realprogression::set_hunt_pass(conn, &ctx, &account, body.as_ref(), who).map_err(AppError::from)
        })
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

fn empty_payload_stub(what: &str) -> Response {
    tracing::info!("{what} (stubbed)");
    ok(json!({"code": null, "message": "OK", "payload": {}}))
}

/// Real mode: the payload is an object of cooldown id -> ISO start, with an int code.
async fn cooldowns(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(empty_payload_stub("Cooldowns"));
    }
    if let Some(refusal) = super::progression::refuse_foreign(&auth, &parts, &account) {
        return Ok(refusal);
    }
    let id = account.clone();
    let reply = state
        .store
        .read(move |conn| crate::cooldowns::reply(conn, &id).map_err(AppError::from))
        .await?;
    tracing::info!(
        "Cooldowns of {account}: {}",
        reply["payload"].as_object().map_or(0, serde_json::Map::len)
    );
    Ok(ok(reply))
}

async fn cooldown_batch(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(empty_payload_stub("Add Cooldowns"));
    }
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "cooldown batch", &account) {
        return Ok(refusal);
    }
    let who = super::progression::caller(&auth);
    let reply = state
        .store
        .write(move |conn| crate::cooldowns::set_batch(conn, &account, body.as_ref(), who).map_err(AppError::from))
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

/// No body: the cooldown starts now.
async fn start_cooldown(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    let auth = match real_only(&state, &parts, &account).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "cooldown start", &account) {
        return Ok(refusal);
    }
    let id = params.get("cooldownId").unwrap_or("").to_owned();
    let who = super::progression::caller(&auth);
    let reply = state
        .store
        .write(move |conn| crate::cooldowns::start(conn, &account, &id, who).map_err(AppError::from))
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

async fn bounty_game_data(State(state): State<AppState>, _auth: MetagameAuth) -> Response {
    tracing::info!("Bounty game data (stubbed)");
    let tokens = state.inventory.bounty_tokens_daily;
    ok(json!({
        "code": null,
        "message": "OK",
        "payload": {
            "max_slots": 4,
            "num_draft_options": 3,
            "num_spicy_options": 1,
            "bounty_token_id": "TOKEN_BOUNTY_DRAFT",
            "premium_bounty_token_id": "TOKEN_BOUNTY_DRAFT_PREMIUM",
            "num_tokens_hp_start": tokens,
            "num_tokens_per_day": tokens,
            "bounty_token_grant_hour": 0,
            "history_length": 10,
            "bronze_count": 9,
            "silver_count": 3,
            "gold_count": 1,
            "new_season_reset_bounties": false,
            "bounty_data": [],
            "item_grant_data": [],
            "token_rollover_warning_days": 1000,
            "automatic_draft": false,
            "automatic_claim": false,
            "delete_claimed_bounties": false
        }
    }))
}

/// The stub's empty board.
fn bounty_stub(what: &str) -> Response {
    tracing::info!("{what} (stubbed)");
    let draft = json!({
        "current_draft_choices": [],
        "previous_draft_selections": [],
        "bronze_count": 0,
        "silver_count": 0,
        "gold_count": 0
    });
    ok(json!({
        "code": null,
        "message": "OK",
        "payload": {"bounties": [], "draft_data": draft, "draft_data_daily": draft, "draft_data_weekly": draft}
    }))
}

/// Real mode: the stored board; every bounty carries drafted_timestamp.
async fn bounties(
    State(state): State<AppState>,
    auth: MetagameAuth,
    parts: Parts,
    params: Params,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(bounty_stub("Bounties"));
    }
    if let Some(refusal) = super::progression::refuse_foreign(&auth, &parts, &account) {
        return Ok(refusal);
    }
    let id = account.clone();
    let reply = state
        .store
        .read(move |conn| crate::bounties::reply(conn, &id).map_err(AppError::from))
        .await?;
    tracing::info!(
        "Bounties of {account}: {}",
        reply["payload"]["bounties"].as_array().map_or(0, Vec::len)
    );
    Ok(ok(reply))
}

async fn delete_bounties(
    State(state): State<AppState>,
    parts: Parts,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    let auth = match real_only(&state, &parts, &account).await {
        Ok(auth) => auth,
        Err(refusal) => return Ok(refusal),
    };
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "bounty delete", &account) {
        return Ok(refusal);
    }
    let who = super::progression::caller(&auth);
    let reply = state
        .store
        .write(move |conn| crate::bounties::delete(conn, &account, body.as_ref(), who).map_err(AppError::from))
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

/// Real mode: a partial upsert by bounty_id; only the status is read by the game server.
async fn set_bounties(
    State(state): State<AppState>,
    auth: MetagameAuth,
    params: Params,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let account = params.get("userId").unwrap_or("").to_owned();
    if !state.progression.is_real(Some(&account)) {
        return Ok(bounty_stub("Set Bounties"));
    }
    if let Some(refusal) = refuse_unless_game_server(&auth, Some(&account), "bounty update", &account) {
        return Ok(refusal);
    }
    let who = super::progression::caller(&auth);
    let reply = state
        .store
        .write(move |conn| crate::bounties::set(conn, &account, body.as_ref(), who).map_err(AppError::from))
        .await?;
    Ok(real_reply(reply.status, reply.body))
}

// ---- Leaderboard profile ----

/// The public leaderboard identity the 1.4.4 client reports at login (Trials).
async fn update_profile(State(state): State<AppState>, auth: MetagameAuth, ExpressBody(body): ExpressBody) -> Response {
    let account = auth.user_id().map(str::to_owned);
    let now = crate::time::now_ms();
    let id = account.clone();
    let result = state
        .store
        .write(move |conn| crate::trials::update_profile(conn, id.as_deref(), body.as_ref(), now))
        .await;
    match result {
        Ok(()) => {
            tracing::info!(
                "Updated leaderboard profile for {}",
                account.as_deref().unwrap_or("undefined")
            );
            empty(StatusCode::OK)
        }
        Err(fail) => super::trials::refusal(fail, "Leaderboard profile update failed"),
    }
}
