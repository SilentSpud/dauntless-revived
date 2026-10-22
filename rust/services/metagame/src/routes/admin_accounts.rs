//! The account routes of `routes/undauntedapi.ts` (under `/undaunted/api`): registration, invites,
//! the registration mode, renames and account info for the launcher and the host tools.

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use serde_json::{Map, Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::enforce;
use service_kit::routing::{Params, Router};

use super::{empty, field, json, query};
use crate::accounts::{
    AccountError, MAX_INVITE_USES, add_invite_code, all_users, clean_invite_note, delete_invite_code,
    generate_invite_code, invite_codes, is_valid_username, register, rename, username_taken,
};
use crate::app::{AppError, AppState};
use crate::auth::{AdminKey, UserKey, sign_token};
use crate::limits::per_client;

const VALID_REGISTRATION_MODES: [&str; 3] = ["NONE", "INVITECODE", "OPEN"];

pub fn routes(state: &AppState) -> Router<AppState> {
    let admin_limit = per_client(&state.limits.admin_mutation, state.origin.clone());
    Router::new()
        .get("/undaunted/api/RegistrationStatus", registration_status)
        .post("/undaunted/api/RegistrationStatus", set_registration_status)
        .get("/undaunted/api/InviteCodes", list_invite_codes)
        .post("/undaunted/api/GenerateJWTForUserId", generate_jwt)
        .get("/undaunted/api/GetAllUsers", get_all_users)
        .post("/undaunted/api/RegisterInviteCode", register_invite_code)
        .delete("/undaunted/api/InviteCode/:inviteCodeToDelete", delete_invite)
        .post("/undaunted/api/Register", register_account)
        .get("/undaunted/api/UsernameAvailable", username_available)
        .post("/undaunted/api/CreateInvite", create_invite)
        .layer(from_fn_with_state(admin_limit, enforce))
        .post("/undaunted/api/RenameUser", rename_user)
        .get("/undaunted/api/ServerStatus", super::undaunted::server_status)
        .get("/undaunted/api/GetUserInfo", |UserKey(user): UserKey| async move {
            json(
                StatusCode::OK,
                json!({"UserId": user.user_id, "Username": user.username, "IsAdmin": user.is_admin}),
            )
        })
}

/// A JSON body as Node's account routes read it: an object, else `{}`.
fn object(body: Option<Value>) -> Value {
    match body {
        Some(value @ Value::Object(_)) => value,
        _ => json!({}),
    }
}

/// JavaScript truthiness, for `!!value`.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// `Number.isSafeInteger`: a JSON number with no fraction within ±(2^53 - 1).
fn safe_integer(value: &Value) -> Option<i64> {
    const MAX: f64 = 9_007_199_254_740_991.0;
    let n = value.as_f64()?;
    (n.fract() == 0.0 && n.abs() <= MAX).then_some(n as i64)
}

fn current_mode(state: &AppState) -> Option<String> {
    state
        .registration_mode
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

async fn registration_status(State(state): State<AppState>) -> Response {
    let mut reply = Map::new();
    if let Some(mode) = current_mode(&state) {
        reply.insert("RegistrationMode".into(), mode.into());
    }
    json(StatusCode::OK, Value::Object(reply))
}

async fn set_registration_status(
    State(state): State<AppState>,
    _admin: AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let Some(body) = body else {
        return Err(AppError::Other("RegistrationStatus without a body".into()));
    };
    match body
        .get("RegistrationStatus")
        .and_then(Value::as_str)
        .filter(|mode| VALID_REGISTRATION_MODES.contains(mode))
    {
        Some(mode) => {
            *state
                .registration_mode
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(mode.to_owned());
            Ok(empty(StatusCode::OK))
        }
        None => Ok(empty(StatusCode::BAD_REQUEST)),
    }
}

async fn list_invite_codes(State(state): State<AppState>, _admin: AdminKey) -> Result<Response, AppError> {
    let codes = state
        .store
        .read(|conn| invite_codes(conn).map_err(AppError::from))
        .await?;
    Ok(json(StatusCode::OK, json!({"InviteCodes": codes})))
}

async fn generate_jwt(
    State(state): State<AppState>,
    _admin: AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let Some(Value::String(user_id)) = field(&body, "UserId").cloned() else {
        return Err(AppError::Other("GenerateJWTForUserId without a UserId string".into()));
    };
    let token = sign_token(&state, &user_id)
        .await?
        .ok_or_else(|| AppError::Other("account_banned".into()))?;
    Ok(json(StatusCode::OK, json!({"JWT": token})))
}

async fn get_all_users(State(state): State<AppState>, _admin: AdminKey) -> Result<Response, AppError> {
    let users = state.store.read(|conn| all_users(conn).map_err(AppError::from)).await?;
    Ok(json(StatusCode::OK, json!({"Users": users})))
}

async fn register_invite_code(
    State(state): State<AppState>,
    _admin: AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let Some(body) = body else {
        return Err(AppError::Other("RegisterInviteCode without a body".into()));
    };
    let added = state
        .store
        .write(move |conn| {
            add_invite_code(
                conn,
                body.get("NewInviteCode"),
                body.get("Uses"),
                truthy(body.get("InfiniteUses")),
            )
            .map_err(AppError::from)
        })
        .await?;
    Ok(empty(if added { StatusCode::OK } else { StatusCode::BAD_REQUEST }))
}

async fn delete_invite(State(state): State<AppState>, _admin: AdminKey, params: Params) -> Result<Response, AppError> {
    let code = params.get("inviteCodeToDelete").unwrap_or("").to_owned();
    state
        .store
        .write(move |conn| delete_invite_code(conn, &code).map_err(AppError::from))
        .await?;
    Ok(empty(StatusCode::OK))
}

/// `{Username, InviteCode}` -> 200 `{UUK}`; refusals are `{"error", "message"}` with 400, 401 or 409.
async fn register_account(State(state): State<AppState>, ExpressBody(body): ExpressBody) -> Result<Response, AppError> {
    let Some(mode) = current_mode(&state).filter(|mode| VALID_REGISTRATION_MODES.contains(&mode.as_str())) else {
        return Ok(empty(StatusCode::INTERNAL_SERVER_ERROR));
    };
    let body = object(body);
    let result = state
        .store
        .write(move |conn| register(conn, &mode, body.get("Username"), body.get("InviteCode")).map_err(AppError::from))
        .await?;
    match result {
        Err(failure) => {
            tracing::info!("Registration refused: {}", failure.error.code());
            Ok(json(
                StatusCode::from_u16(failure.status).unwrap_or(StatusCode::BAD_REQUEST),
                failure.error.body(),
            ))
        }
        Ok(account) => {
            tracing::info!("Registered {} as {}", account.user_id, account.username);
            let answer = protocol::account::Registered { key: account.key };
            Ok(json(StatusCode::OK, serde_json::to_value(answer).unwrap_or_default()))
        }
    }
}

/// "Is this name free?" for the launcher's register screen; registering can still lose a race.
async fn username_available(State(state): State<AppState>, RawQuery(raw): RawQuery) -> Result<Response, AppError> {
    let name = match query(raw.as_deref()).get("Username") {
        Some(Value::String(name)) => Some(name.trim().to_owned()),
        _ => None,
    };
    let Some(name) = name.filter(|name| is_valid_username(name)) else {
        return Ok(json(StatusCode::OK, unavailable(AccountError::UsernameInvalid)));
    };
    if state
        .store
        .read(move |conn| username_taken(conn, &name).map_err(AppError::from))
        .await?
    {
        return Ok(json(StatusCode::OK, unavailable(AccountError::UsernameTaken)));
    }
    Ok(json(StatusCode::OK, json!({"available": true})))
}

fn unavailable(error: AccountError) -> Value {
    json!({"available": false, "error": error.code(), "message": error.message()})
}

/// Admin: `{uses?: 1-1000 (default 1), name?: string}` -> `{"code": "XXXX-XXXX-XXXX"}`. The name is a
/// note for the host's log, not stored; codes are credentials, so the log gets the first group only.
async fn create_invite(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = object(body);
    let uses = match body.get("uses") {
        None | Some(Value::Null) => Some(1),
        Some(value) => safe_integer(value).filter(|uses| (1..=MAX_INVITE_USES).contains(uses)),
    };
    let name_ok = matches!(body.get("name"), None | Some(Value::Null) | Some(Value::String(_)));
    let (Some(uses), true) = (uses, name_ok) else {
        let message = format!("uses must be a whole number from 1 to {MAX_INVITE_USES}, and name a string");
        return Ok(json(
            StatusCode::BAD_REQUEST,
            json!({"error": "bad_request", "message": message}),
        ));
    };
    // A clash with an existing code is next to impossible (60 random bits); try again if it happens.
    for attempt in 1..=5 {
        let code = generate_invite_code();
        let stored = code.clone();
        match state
            .store
            .write(move |conn| {
                add_invite_code(conn, Some(&json!(stored)), Some(&json!(uses)), false).map_err(AppError::from)
            })
            .await
        {
            Ok(true) => {
                let note =
                    clean_invite_note(body.get("name")).map_or(String::new(), |note| format!(", note \"{note}\""));
                tracing::info!(
                    "Invite {}-... created by {}: {uses} use(s){note}",
                    &code[..4],
                    admin.user_id
                );
                return Ok(json(StatusCode::OK, json!({"code": code})));
            }
            Ok(false) => {}
            Err(error) => tracing::warn!("CreateInvite attempt {attempt} failed: {error}"),
        }
    }
    Ok(empty(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Admin: `{UserId or Username (the current name), NewUsername}` -> `{UserId, OldUsername, Username}`.
async fn rename_user(
    State(state): State<AppState>,
    AdminKey(admin): AdminKey,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let body = object(body);
    let result = state
        .store
        .write(move |conn| {
            rename(conn, body.get("UserId"), body.get("Username"), body.get("NewUsername")).map_err(AppError::from)
        })
        .await?;
    match result {
        Err(failure) => Ok(json(
            StatusCode::from_u16(failure.status).unwrap_or(StatusCode::BAD_REQUEST),
            failure.error.body(),
        )),
        Ok(renamed) => {
            tracing::info!(
                "Admin {} renamed {} from {} to {} ({} character(s))",
                admin.user_id,
                renamed.user_id,
                renamed.old_username,
                renamed.username,
                renamed.characters
            );
            Ok(json(
                StatusCode::OK,
                json!({"UserId": renamed.user_id, "OldUsername": renamed.old_username, "Username": renamed.username}),
            ))
        }
    }
}
