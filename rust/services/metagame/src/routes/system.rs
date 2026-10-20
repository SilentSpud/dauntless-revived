//! The entitlement routes of `routes/system.ts` (the rest of the file comes with its area).

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::{fall_through, json, ok};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, metagame_auth};
use crate::entitlements;
use crate::progression::{
    Caller, Event, account_exists, read_field, read_integer, real_reply, record_event, refuse_unless_game_server,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/entitlementsv2", list)
        .post("/entitlementv2/:userId", grant)
        .delete("/entitlement/:userId/:entitlement", revoke)
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
