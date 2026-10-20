//! `routes/login.ts`: the client's login handshake and account info.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use diesel::prelude::*;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::Router;

use super::{empty, field, json, ok};
use crate::accounts::{account_by_username, is_account_id_shape, username_for};
use crate::app::{AppError, AppState};
use crate::auth::MetagameAuth;
use crate::schema::users;

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/features/platform/win", || async {
            tracing::info!("Features");
            ok(json!({"code": null, "message": "OK", "payload": {"crossplay": true, "crossprogression": true}}))
        })
        .get("/account/link/epic/:AccId", || async {
            tracing::info!("Account Linking");
            ok(json!({"code": null, "message": "OK", "payload": {"isLinked": true}}))
        })
        .post("/login", login)
        .get("/accountinfo", account_info)
        .get("/tags", |auth: MetagameAuth| async move {
            tracing::info!("Tags");
            let mut reply = serde_json::Map::new();
            if let Some(user_id) = auth.user_id() {
                reply.insert("accountId".into(), user_id.into());
            }
            reply.insert("tags".into(), json!([]));
            ok(Value::Object(reply))
        })
        .put("/gamesession/epic", game_session)
        .post("/accountinfo/public", account_info_public)
}

/// `POST /login`: the body's email must be the token's account, and the account must exist.
async fn login(
    State(state): State<AppState>,
    auth: MetagameAuth,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    // req.body.email with no body throws in Node: a 500.
    let Some(body) = body else {
        return Err(AppError::Other("POST /login without a JSON body".into()));
    };
    let user_id = auth.user_id().map(str::to_owned);
    let same = match (&user_id, body.get("email")) {
        (Some(id), Some(Value::String(email))) => id == email,
        (None, None) => true,
        _ => false,
    };
    if !same {
        tracing::error!(
            "UserID from metagame auth {} didn't match UserID from token {}",
            user_id.as_deref().unwrap_or("undefined"),
            "undefined"
        );
        return Ok(empty(StatusCode::BAD_REQUEST));
    }
    let Some(id) = user_id else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let lookup = id.clone();
    let exists = state
        .store
        .read(move |conn| {
            users::table
                .find(&lookup)
                .select(users::userId)
                .first::<String>(conn)
                .optional()
                .map_err(AppError::from)
        })
        .await?
        .is_some();
    if !exists {
        tracing::error!("UserID from metagame auth {id} had no database entry!");
        return Ok(empty(StatusCode::BAD_REQUEST));
    }
    tracing::info!("{id} is logging in!");
    Ok(ok(
        json!({"error_code": "TicketRateOk", "message": "", "state": "OPEN", "timeout": 8000, "title": ""}),
    ))
}

/// `GET /accountinfo`: the token's own account. An account that no longer exists is a 500, as in Node.
async fn account_info(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    tracing::info!("Account info");
    let Some(id) = auth.user_id().map(str::to_owned) else {
        return Err(AppError::Other("GET /accountinfo without a player".into()));
    };
    let lookup = id.clone();
    let name = state
        .store
        .read(move |conn| {
            users::table
                .find(&lookup)
                .select(users::name)
                .first::<String>(conn)
                .optional()
                .map_err(AppError::from)
        })
        .await?
        .ok_or_else(|| AppError::Other(format!("GET /accountinfo for {id}, which has no account")))?;
    Ok(ok(json!({
        "accountId": id,
        "creationDate": "2000-01-01 00:00:00",
        "email": null,
        "preferredLanguage": null,
        "username": name,
        "verified": true,
    })))
}

/// `PUT /gamesession/epic`: echoes the bearer token as the session token (one token for Epic, the
/// launcher and Phoenix).
async fn game_session(_auth: MetagameAuth, headers: axum::http::HeaderMap) -> Result<Response, AppError> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::Other("PUT /gamesession/epic without a bearer".into()))?;
    let token = token.get(7..).unwrap_or("");
    Ok(ok(json!({
        "code": null,
        "message": "OK",
        "payload": {"error_code": null, "sessionid": "SESSION_ID_LOL", "sessionToken": token},
    })))
}

/// `POST /accountinfo/public`: the looked-up account (by `accountId`, else by `displayname` /
/// `displayName`), filed by the client under the reply's `accountId`; 404 `{}` when unknown.
/// `ACCOUNTINFO_PUBLIC_LEGACY=1` answers upstream's way: the caller's own id.
async fn account_info_public(
    State(state): State<AppState>,
    auth: MetagameAuth,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let caller = auth.user_id().map(str::to_owned);
    if state.features.accountinfo_public_legacy {
        let asked = match field(&body, "accountId") {
            Some(Value::String(id)) => Some(id.clone()),
            _ => None,
        };
        let name = match asked {
            Some(id) => {
                state
                    .store
                    .read(move |conn| username_for(conn, &id).map_err(AppError::from))
                    .await?
            }
            None => None,
        };
        let mut linked = serde_json::Map::new();
        let mut reply = serde_json::Map::new();
        if let Some(caller) = &caller {
            reply.insert("accountId".into(), caller.clone().into());
            linked.insert("accountId".into(), caller.clone().into());
        }
        linked.insert("accountType".into(), "epic".into());
        reply.insert("isSubscribed".into(), true.into());
        reply.insert("language".into(), Value::Null);
        reply.insert("linkedAccounts".into(), json!([linked]));
        reply.insert("username".into(), name.unwrap_or_default().into());
        return Ok(json(StatusCode::OK, Value::Object(reply)));
    }

    // AskedAccount: accountId wins when it is set (not missing, null or ""), else the name.
    enum Ask {
        Id(String),
        Name(String),
        Nothing(&'static str),
    }
    let ask = match field(&body, "accountId") {
        Some(Value::String(id)) if !id.is_empty() && is_account_id_shape(id) => Ask::Id(id.clone()),
        Some(value) if !value.is_null() && value != "" => Ask::Nothing("<not an account id>"),
        _ => match field(&body, "displayname")
            .filter(|v| !v.is_null())
            .or_else(|| field(&body, "displayName"))
        {
            Some(Value::String(name)) => Ask::Name(name.clone()),
            _ => Ask::Nothing("<nothing asked>"),
        },
    };
    let (account, asked) = match ask {
        Ask::Id(id) => {
            let lookup = id.clone();
            let name = state
                .store
                .read(move |conn| username_for(conn, &lookup).map_err(AppError::from))
                .await?;
            (name.map(|name| (id.clone(), name)), id)
        }
        Ask::Name(name) => {
            let shown = String::from_utf16_lossy(&name.encode_utf16().take(32).collect::<Vec<_>>());
            let found = state
                .store
                .read(move |conn| account_by_username(conn, &name).map_err(AppError::from))
                .await?;
            (found, format!("name:{}", Value::String(shown)))
        }
        Ask::Nothing(text) => (None, text.to_owned()),
    };
    tracing::info!(
        "accountinfo/public by {} for {asked} -> {}",
        caller.as_deref().unwrap_or("<game server>"),
        if account.is_some() { "found" } else { "404" }
    );
    let Some((id, name)) = account else {
        return Ok(json(StatusCode::NOT_FOUND, json!({})));
    };
    Ok(json(
        StatusCode::OK,
        json!({
            "accountId": id,
            "username": name,
            "linkedAccounts": [{"accountId": id, "accountType": "epic"}],
            "isSubscribed": true,
            "language": null,
        }),
    ))
}
