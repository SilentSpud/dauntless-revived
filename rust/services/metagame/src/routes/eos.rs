//! `routes/eos.ts`: the Epic-style login, token check and account lookups the 1.4.4 client makes.

use std::collections::HashMap;

use axum::extract::{RawQuery, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use serde_json::{Map, Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::enforce;
use service_kit::routing::{Params, Router};

use super::{empty, field, json, ok, query};
use crate::accounts::{account_by_username, username_for, usernames_for};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, SoftPlayer, check_player_access, sign_token, user_id_for_key};
use crate::limits::per_client;
use crate::time::iso_now;

/// The account upstream's stub named for everyone.
const VERIFY_STUB_ACCOUNT_ID: &str = "9626f441055349ce8cb7d7d5a483eaa2";
const MAX_MAPPING_IDS: usize = 100;
const MAX_ACCOUNT_LOOKUP: usize = 100;

pub fn routes(state: &AppState) -> Router<AppState> {
    let login_limit = per_client(&state.limits.login, state.origin.clone());
    let verify_limit = per_client(&state.limits.token_verify, state.origin.clone());
    Router::new()
        .post("/account/api/oauth/token", token)
        .layer(from_fn_with_state(login_limit, enforce))
        .get("/account/api/oauth/verify", verify)
        .layer(from_fn_with_state(verify_limit, enforce))
        .get("/account/api/public/account/:AccId", public_account)
        // Before /:AccId/externalAuths, which a user named "externalAuths" would otherwise hit.
        .get(
            "/account/api/public/account/displayName/:displayName",
            public_account_by_name,
        )
        .get("/account/api/public/account/:AccId/externalAuths", || async {
            tracing::info!("External Auths (stubbed)");
            ok(json!({}))
        })
        .post("/account/mapping", mapping)
        .delete("/account/api/oauth/sessions/kill", kill_session)
        .delete("/account/api/oauth/sessions/kill/:AuthToken", kill_session)
        .get("/account/api/public/account", own_account)
}

async fn kill_session() -> Response {
    tracing::info!("Session kill (stubbed)");
    ok(json!({}))
}

fn login_reply(token: &str, user_id: &str) -> Value {
    json!({
        "access_token": token,
        "token_type": "bearer",
        "expires_at": "2085-09-09T01:01:01.703Z",
        "features": ["Achievements", "AntiCheat", "Ecom", "Voice"],
        "organization_id": "o-krlzxj88qrtb69fredeuaf887bl5az",
        "product_id": "prod-jackal",
        "sandbox_id": "jackal",
        "deployment_id": "53565ba467df4edbb6f5a3d939a8b4f2",
        "expires_in": 86400,
        "refresh_token": "refresh.token.lol",
        "refresh_expires_at": "2085-09-09T01:01:01.703Z",
        "account_id": user_id,
    })
}

/// `POST /account/api/oauth/token`: the launcher's account key (or, in development logins, the
/// account id) as `exchange_code` becomes a session token.
async fn token(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let Some(body) = body else {
        return Err(AppError::Other("oauth/token without a body".into()));
    };
    let Some(Value::String(code)) = body.get("exchange_code") else {
        return Err(AppError::Other("oauth/token without an exchange_code string".into()));
    };
    if state.auth.dev_logins() {
        tracing::info!("Logging in {code}!");
        let token = sign_token(&state, code)
            .await?
            .ok_or_else(|| AppError::Other("account_banned".into()))?;
        return Ok(ok(login_reply(&token, code)));
    }
    if state.auth.mode != crate::app::AuthMode::ApiKey {
        service_kit::fatal!("No login method configured!");
        return Err(AppError::Other("no login method".into()));
    }
    let Some(user_id) = user_id_for_key(&state, code).await? else {
        tracing::error!("Invalid API key auth!");
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    if let Some(refusal) = check_player_access(&state, &parts, Some(&user_id)).await? {
        return Ok(refusal);
    }
    tracing::info!("Logging in {user_id}!");
    let token = sign_token(&state, &user_id)
        .await?
        .ok_or_else(|| AppError::Other("account_banned".into()))?;
    Ok(ok(login_reply(&token, &user_id)))
}

/// `GET /account/api/oauth/verify`, every 30 s: a valid token gets its own account, anything else the
/// old stub (never a 401, which could log a player out mid-session).
async fn verify(State(state): State<AppState>, SoftPlayer(claims): SoftPlayer) -> Response {
    tracing::info!("Verifying token");
    let caller = if state.features.verify_stub_account {
        None
    } else {
        claims.map(|c| c.user_id)
    };
    ok(json!({
        "active": true,
        "scope": "basic_profile friends_list presence",
        "token_type": "bearer",
        "expires_in": 86400,
        "expires_at": "2085-09-09T01:01:01.703Z",
        "account_id": caller.as_deref().unwrap_or(VERIFY_STUB_ACCOUNT_ID),
        "client_id": "xyza7891lhxMVYGCON7LgnKZZ8HQGD5H",
        "application_id": "fghi4567O03HROxEjwbn7kgXpBhnhWwv",
    }))
}

fn public(user_id: &str, name: &str) -> Value {
    json!({"id": user_id, "displayName": name, "externalAuths": {}})
}

/// The first 64 UTF-16 units of a value, as `String(x).slice(0, 64)` logs it.
fn short(text: &str) -> String {
    String::from_utf16_lossy(&text.encode_utf16().take(64).collect::<Vec<_>>())
}

async fn public_account(
    State(state): State<AppState>,
    SoftPlayer(claims): SoftPlayer,
    params: Params,
) -> Result<Response, AppError> {
    let asked = params.get("AccId").unwrap_or("").to_owned();
    let Some(caller) = claims.map(|c| c.user_id) else {
        tracing::info!("EOS Account Info (stubbed)");
        return Ok(ok(json!({})));
    };
    let lookup = asked.clone();
    let name = state
        .store
        .read(move |conn| username_for(conn, &lookup).map_err(AppError::from))
        .await?;
    tracing::info!(
        "EOS Account Info for {} by {caller}: {}",
        short(&asked),
        if name.is_some() { "found" } else { "unknown" }
    );
    Ok(ok(name.map_or_else(|| json!({}), |name| public(&asked, &name))))
}

async fn public_account_by_name(
    State(state): State<AppState>,
    SoftPlayer(claims): SoftPlayer,
    params: Params,
) -> Result<Response, AppError> {
    let caller = claims.map(|c| c.user_id);
    let account = match &caller {
        Some(_) => {
            let name = params.get("displayName").unwrap_or("").to_owned();
            state
                .store
                .read(move |conn| account_by_username(conn, &name).map_err(AppError::from))
                .await?
        }
        None => None,
    };
    tracing::info!(
        "EOS Account by name by {}: {}",
        caller.as_deref().unwrap_or("<no token>"),
        account.as_ref().map_or("not found", |(id, _)| id.as_str())
    );
    Ok(match account {
        Some((id, name)) => ok(public(&id, &name)),
        None => empty(StatusCode::NOT_FOUND),
    })
}

/// The ids of a mapping request: `ids` (or the older names), strings of 1-128 characters, each once,
/// at most 100.
fn mapping_ids(body: &Option<Value>) -> Vec<String> {
    let list = match body {
        Some(Value::Array(list)) => Some(list),
        Some(body) => ["ids", "externalIds", "accountIds", "externalAuthIds"]
            .iter()
            .find_map(|key| body.get(*key).and_then(Value::as_array)),
        None => None,
    };
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| !id.is_empty() && id.encode_utf16().count() <= 128)
        .filter(|id| seen.insert(id.to_string()))
        .map(str::to_owned)
        .take(MAX_MAPPING_IDS)
        .collect()
}

fn mapping_source_type(body: &Option<Value>) -> String {
    let field = ["srcAccountType", "type", "externalAuthType"]
        .iter()
        .find_map(|key| field(body, key).and_then(Value::as_str));
    match field {
        Some(text)
            if (1..=32).contains(&text.len())
                && text.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) =>
        {
            text.to_owned()
        }
        _ => "epic".into(),
    }
}

fn js_typeof(value: &Value) -> &'static str {
    match value {
        Value::Null | Value::Array(_) | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
    }
}

/// `DescribeMappingBody`: the body's shape for the log, never values that could be tokens.
fn describe_mapping_body(body: &Option<Value>) -> String {
    match body {
        None | Some(Value::Null) => "no body".into(),
        Some(Value::Array(list)) => {
            let mut kinds: Vec<&str> = Vec::new();
            for value in list {
                let kind = js_typeof(value);
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
            }
            format!("array of {} ({})", list.len(), kinds.join(","))
        }
        Some(Value::Object(map)) if map.is_empty() => "empty object".into(),
        Some(Value::Object(map)) => map
            .iter()
            .take(12)
            .map(|(key, value)| {
                let shape = match value {
                    Value::Array(list) => format!("[{}]", list.len()),
                    Value::String(text)
                        if text.encode_utf16().count() <= 24
                            && text.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) =>
                    {
                        Value::String(text.clone()).to_string()
                    }
                    Value::String(text) => format!("string({})", text.encode_utf16().count()),
                    other => js_typeof(other).into(),
                };
                format!(
                    "{}={shape}",
                    String::from_utf16_lossy(&key.encode_utf16().take(40).collect::<Vec<_>>())
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
        Some(other) => js_typeof(other).into(),
    }
}

/// `POST /account/mapping`: Epic ids to Phoenix ids, which on this server are the same `UID-…`, keyed
/// by each asked id under `accountMappings` (and again under `payload`). Only our accounts, only for a
/// caller with a valid player token; `ACCOUNT_MAPPING=0` maps nothing.
async fn mapping(
    State(state): State<AppState>,
    SoftPlayer(claims): SoftPlayer,
    headers: HeaderMap,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let caller = claims.map(|c| c.user_id);
    let ids = mapping_ids(&body);
    let source = mapping_source_type(&body);
    let enabled = state.features.account_mapping;
    let known: HashMap<String, String> = if caller.is_some() && enabled {
        let lookup = ids.clone();
        state
            .store
            .read(move |conn| usernames_for(conn, &lookup).map_err(AppError::from))
            .await?
    } else {
        HashMap::new()
    };
    let account_type = if source.eq_ignore_ascii_case("phoenix") {
        "epic"
    } else {
        "phoenix"
    };
    let mapped: Vec<&String> = ids.iter().filter(|id| known.contains_key(*id)).collect();
    let mappings: Map<String, Value> = mapped
        .iter()
        .map(|id| ((*id).clone(), json!({"accountId": id, "accountType": account_type})))
        .collect();
    let content_type = headers.get("content-type").map_or("none".to_owned(), |v| {
        String::from_utf8_lossy(v.as_bytes()).into_owned()
    });
    tracing::info!(
        "account/mapping by {}: {}; content-type {} -> {} of {} mapped{}",
        caller.as_deref().unwrap_or("<no token>"),
        describe_mapping_body(&body),
        content_type.chars().take(60).collect::<String>(),
        mapped.len(),
        ids.len(),
        if enabled { "" } else { " (ACCOUNT_MAPPING=0)" }
    );
    Ok(json(
        StatusCode::OK,
        json!({"accountMappings": mappings, "code": "OK", "message": "", "payload": {"accountMappings": mappings}}),
    ))
}

/// `GET /account/api/public/account`: the caller's own account, or with `?accountId=…` (repeatable)
/// the asked accounts that exist.
async fn own_account(
    State(state): State<AppState>,
    auth: MetagameAuth,
    RawQuery(raw): RawQuery,
) -> Result<Response, AppError> {
    let user_id = auth.user_id().map(str::to_owned);
    let query = query(raw.as_deref());
    if let Some(asked) = query.get("accountId") {
        let asked: Vec<String> = match asked {
            Value::Array(list) => list.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
            Value::String(id) => vec![id.clone()],
            _ => vec![],
        };
        let asked: Vec<String> = asked.into_iter().take(MAX_ACCOUNT_LOOKUP).collect();
        let lookup = asked.clone();
        let names = state
            .store
            .read(move |conn| usernames_for(conn, &lookup).map_err(AppError::from))
            .await?;
        let mut seen = std::collections::HashSet::new();
        let found: Vec<Value> = asked
            .iter()
            .filter(|id| seen.insert((*id).clone()))
            .filter_map(|id| names.get(id).map(|name| public(id, name)))
            .collect();
        tracing::info!(
            "Account info for {} account(s) by userId {}: {} found",
            asked.len(),
            user_id.as_deref().unwrap_or("undefined"),
            found.len()
        );
        return Ok(ok(Value::Array(found)));
    }

    // DisplayNameForUserId
    let display_name = if !state.features.account_display_name {
        json!({})
    } else if let Some(id) = user_id.clone() {
        let name = state
            .store
            .read(move |conn| {
                use diesel::prelude::*;
                crate::schema::users::table
                    .find(&id)
                    .select(crate::schema::users::name)
                    .first::<String>(conn)
                    .optional()
                    .map_err(AppError::from)
            })
            .await?;
        Value::String(name.unwrap_or_default())
    } else {
        Value::String(String::new())
    };
    tracing::info!("Account info for userId {}", user_id.as_deref().unwrap_or("undefined"));
    let now = iso_now();
    let mut reply = Map::new();
    if let Some(id) = &user_id {
        reply.insert("id".into(), id.clone().into());
    }
    let rest = json!({
        "displayName": display_name,
        "name": "",
        "lastName": "",
        "email": "",
        "failedLoginAttempts": 0,
        "lastLogin": now,
        "numberOfDisplayNameChanges": 0,
        "ageGroup": "ADULT",
        "headless": false,
        "country": "US",
        "lastNameChange": now,
        "preferredLanguage": "en",
        "canUpdateDisplayName": false,
        "tfaEnabled": false,
        "emailVerified": true,
        "minorVerified": false,
        "minorStatus": "NOT_MINOR",
    });
    if let Value::Object(rest) = rest {
        reply.extend(rest);
    }
    Ok(ok(Value::Object(reply)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values the Node DescribeMappingBody / MappingIds give for the same bodies.
    #[test]
    fn mapping_bodies() {
        let body = Some(json!({"srcAccountType": "epic", "ids": ["UID-a", "UID-a", "", 5, "UID-b"]}));
        assert_eq!(mapping_ids(&body), ["UID-a", "UID-b"]);
        assert_eq!(describe_mapping_body(&body), "srcAccountType=\"epic\" ids=[5]");
        assert_eq!(
            describe_mapping_body(&Some(json!(["x", 1]))),
            "array of 2 (string,number)"
        );
        assert_eq!(
            describe_mapping_body(&Some(json!({"t": "eyJhbGciOiJSUzI1NiJ9.payload.sig"}))),
            "t=string(32)"
        );
        assert_eq!(describe_mapping_body(&None), "no body");
        assert_eq!(mapping_source_type(&Some(json!({"type": "Phoenix"}))), "Phoenix");
        assert_eq!(mapping_source_type(&Some(json!({"srcAccountType": "a b"}))), "epic");
    }
}
