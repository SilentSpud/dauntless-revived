//! Who is calling (MIGRATION_PLAN.md M9): the Node auth middleware as typed extractors, so a handler
//! without the right one doesn't compile. Each refuses exactly as its Node middleware does:
//!
//! | Extractor | Node middleware | Refusals |
//! | --- | --- | --- |
//! | [`MetagameAuth`] | `HasUndauntedMetagameAuth` | 401 empty; 403 empty for the game-server key from outside |
//! | [`Player`] | `HasUndauntedMetagameAuth` + `PlayerTokenOnly` | as above, and 403 empty without a player token |
//! | [`SoftPlayer`] | `SoftMetagameAuth` | never: a bad token just leaves it empty |
//! | [`GameServer`] | `GameServerKeyAuth` | 403 JSON without the key, 403 empty from outside, 401 empty |
//! | [`AdminKey`] | `HasUndauntedAdminApiKey` | 403 empty through a proxy, 401 empty, 403 empty for non-admins |
//! | [`UserKey`] | `HasUndauntedUserApiKey` | 401 empty, 403 `account_banned` JSON |
//! | [`SoftAccount`] | `SoftAccountAuth` | never |
//!
//! A session token counts only while its `epoch` (missing = 0) is the account's current one and the
//! account isn't banned, as `ValidateMetagameJWTAndGetPayload` checks.

// Rejections are axum responses, the way extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use diesel::prelude::*;
use service_kit::json::Json;
use service_kit::serve::ConnectionInfo;

use crate::app::{AppError, AppState};
use crate::crypto::{TokenClaims, hashes_equal, seal_recovery, sha256_hex};
use crate::origin::Origin;
use crate::schema::{account_bans, accountkeyrecovery, authepochs, gameserverapikeys, userapikeys, users};

const GAMESERVER_KEY: &str = "x-undaunted-gameserver-apikey";
const USER_KEY: &str = "x-undaunted-user-api-key";

/// An empty reply with a status, as `res.status(n).send()`.
fn status(code: StatusCode) -> Response {
    code.into_response()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn origin(parts: &Parts) -> Origin<'_> {
    let connection = parts
        .extensions
        .get::<ConnectInfo<ConnectionInfo>>()
        .map(|info| info.0)
        .unwrap_or(ConnectionInfo {
            peer: ([0, 0, 0, 0], 0).into(),
            local: ([0, 0, 0, 0], 0).into(),
        });
    Origin {
        connection,
        headers: &parts.headers,
    }
}

fn header<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    parts.headers.get(name).map(|value| value.to_str().unwrap_or(""))
}

/// The active ban of an account or address (`BanFor`).
pub fn ban_for(
    conn: &mut store::Connection,
    account: Option<&str>,
    address: Option<&str>,
) -> QueryResult<Option<String>> {
    account_bans::table
        .filter(account_bans::active.eq(1))
        .filter(
            account_bans::expires_at
                .is_null()
                .or(account_bans::expires_at.gt(now_ms())),
        )
        .filter(
            account_bans::account_id
                .eq(account.unwrap_or(""))
                .or(account_bans::address.eq(address.unwrap_or(""))),
        )
        .select(account_bans::reason)
        .first::<String>(conn)
        .optional()
}

/// The account's key epoch; tokens from an older one were revoked by a key rotation (`AuthEpoch`).
pub fn auth_epoch(conn: &mut store::Connection, user_id: &str) -> QueryResult<i64> {
    Ok(authepochs::table
        .find(user_id)
        .select(authepochs::epoch)
        .first::<i64>(conn)
        .optional()?
        .unwrap_or(0))
}

/// A session token that still counts: valid, current epoch, account not banned.
pub async fn validate_token(state: &AppState, token: &str) -> Result<Option<TokenClaims>, AppError> {
    let Ok(claims) = state.tokens.verify(token) else {
        return Ok(None);
    };
    let user_id = claims.user_id.clone();
    let (epoch, banned) = state
        .store
        .read(move |conn| Ok::<_, AppError>((auth_epoch(conn, &user_id)?, ban_for(conn, Some(&user_id), None)?)))
        .await?;
    Ok((claims.epoch.unwrap_or(0) == epoch && banned.is_none()).then_some(claims))
}

/// Signs a session token for an account (`SignMetagameJWTForUid`); `None` when it is banned.
pub async fn sign_token(state: &AppState, user_id: &str) -> Result<Option<String>, AppError> {
    let id = user_id.to_owned();
    let (epoch, banned) = state
        .store
        .read(move |conn| Ok::<_, AppError>((auth_epoch(conn, &id)?, ban_for(conn, Some(&id), None)?)))
        .await?;
    if banned.is_some() {
        return Ok(None);
    }
    state
        .tokens
        .sign(user_id, epoch, now_secs())
        .map(Some)
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Whether `key` is a registered game-server key (every stored hash compared in constant time).
pub async fn is_valid_gameserver_key(state: &AppState, key: &str) -> Result<bool, AppError> {
    let hash = sha256_hex(key);
    let hashes: Vec<Option<String>> = state
        .store
        .read(|conn| {
            gameserverapikeys::table
                .select(gameserverapikeys::keyHash)
                .load(conn)
                .map_err(AppError::from)
        })
        .await?;
    Ok(hashes
        .iter()
        .flatten()
        .fold(false, |found, stored| hashes_equal(&hash, stored) | found))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInfo {
    pub user_id: String,
    pub username: String,
    pub is_admin: bool,
}

/// The account an account key belongs to (`GetUserInfoForApiKey`).
/// The account id of a key, comparing every stored hash in constant time; the last match wins, as
/// in the Node loop (`GetUserIDForAPIKey`). It needn't have a `users` row.
fn account_for_key(conn: &mut store::Connection, key: &str) -> QueryResult<Option<String>> {
    let hash = sha256_hex(key);
    let keys: Vec<(String, String)> = userapikeys::table
        .select((userapikeys::userId, userapikeys::keyHash))
        .load(conn)?;
    Ok(keys.into_iter().fold(None, |found, (user, stored)| {
        if hashes_equal(&hash, &stored) {
            Some(user)
        } else {
            found
        }
    }))
}

/// [`account_for_key`] on the read pool: the login's key lookup.
pub async fn user_id_for_key(state: &AppState, key: &str) -> Result<Option<String>, AppError> {
    let key = key.to_owned();
    state
        .store
        .read(move |conn| account_for_key(conn, &key).map_err(AppError::from))
        .await
}

pub async fn user_for_key(state: &AppState, key: &str) -> Result<Option<UserInfo>, AppError> {
    let dev = state.auth.dev_logins();
    let key = key.to_owned();
    state
        .store
        .read(move |conn| {
            let user_id = if dev { Some(key) } else { account_for_key(conn, &key)? };
            let Some(user_id) = user_id else {
                return Ok::<_, AppError>(None);
            };
            let row: Option<(String, bool)> = users::table
                .find(&user_id)
                .select((users::name, users::isAdmin))
                .first(conn)
                .optional()?;
            Ok(row.map(|(username, is_admin)| UserInfo {
                user_id,
                username,
                is_admin,
            }))
        })
        .await
}

/// Refuses a banned account or (in public mode) a banned player address (`CheckPlayerAccess`).
pub async fn check_player_access(
    state: &AppState,
    parts: &Parts,
    account: Option<&str>,
) -> Result<Option<Response>, AppError> {
    let address = origin(parts).client_address(&state.origin);
    let public = (!address.is_loopback()).then(|| address.to_string());
    let account = account.map(str::to_owned);
    let ban = state
        .store
        .read(move |conn| ban_for(conn, account.as_deref(), public.as_deref()).map_err(AppError::from))
        .await?;
    Ok(ban.map(|reason| {
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "account_banned", "reason": reason})),
        )
            .into_response()
    }))
}

/// The caller of a route that needs a session token or the game-server key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetagameAuth {
    Player(TokenClaims),
    /// A game server; with the player token it forwarded, if any.
    GameServer(Option<TokenClaims>),
}

impl MetagameAuth {
    pub fn user_id(&self) -> Option<&str> {
        match self {
            Self::Player(claims) | Self::GameServer(Some(claims)) => Some(&claims.user_id),
            Self::GameServer(None) => None,
        }
    }

    pub fn is_game_server(&self) -> bool {
        matches!(self, Self::GameServer(_))
    }
}

fn has_bearer_prefix(value: &str) -> bool {
    value.starts_with("bearer ") || value.starts_with("Bearer ") || value.starts_with("BEARER ")
}

/// `AuthHeader.slice("bearer ".length)`.
fn after_scheme(value: &str) -> &str {
    value.get(7..).unwrap_or("")
}

/// `HasUndauntedMetagameAuth` on a request's parts, for handlers that check something before auth
/// (`RealProgressionOnly` runs first).
pub async fn metagame_auth(parts: &Parts, state: &AppState) -> Result<MetagameAuth, Response> {
    let authorization = header(parts, "authorization");
    if let Some(key) = header(parts, GAMESERVER_KEY) {
        let origin = origin(parts);
        if !origin.is_direct_local(&state.origin, &state.own_addresses) {
            tracing::warn!(
                "Refusing the game-server key on {} {} from {}{}: it is only accepted from this machine",
                parts.method,
                parts.uri.path(),
                origin.client_address(&state.origin),
                if origin.is_proxied() {
                    " (relayed by a proxy)"
                } else {
                    ""
                }
            );
            return Err(status(StatusCode::FORBIDDEN));
        }
        if !is_valid_gameserver_key(state, key)
            .await
            .map_err(IntoResponse::into_response)?
        {
            tracing::error!("Invalid Gameserver API Key Auth");
            return Err(status(StatusCode::UNAUTHORIZED));
        }
        // A game server may forward the player's bearer; it has to be valid then.
        return match authorization {
            None => Ok(MetagameAuth::GameServer(None)),
            Some(value) => match validate_token(state, after_scheme(value))
                .await
                .map_err(IntoResponse::into_response)?
            {
                Some(claims) => Ok(MetagameAuth::GameServer(Some(claims))),
                None => Err(status(StatusCode::UNAUTHORIZED)),
            },
        };
    }
    let Some(value) = authorization.filter(|value| has_bearer_prefix(value)) else {
        tracing::error!(
            "Unauthenticated {} to {} which needs metagame auth!",
            parts.method,
            parts.uri.path()
        );
        return Err(status(StatusCode::UNAUTHORIZED));
    };
    match validate_token(state, after_scheme(value))
        .await
        .map_err(IntoResponse::into_response)?
    {
        Some(claims) => Ok(MetagameAuth::Player(claims)),
        None => {
            tracing::warn!("Request with bad metagame auth!");
            Err(status(StatusCode::UNAUTHORIZED))
        }
    }
}

impl FromRequestParts<AppState> for MetagameAuth {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        metagame_auth(parts, state).await
    }
}

/// A route only a player's own token may call (`PlayerTokenOnly`); a game server forwarding a
/// player's token counts as that player, as in Node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub user_id: String,
    pub via_game_server: bool,
}

impl FromRequestParts<AppState> for Player {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let auth = metagame_auth(parts, state).await?;
        match auth.user_id().filter(|id| !id.is_empty()) {
            Some(user_id) => Ok(Self {
                user_id: user_id.to_owned(),
                via_game_server: auth.is_game_server(),
            }),
            None => {
                tracing::warn!(
                    "Refusing {} {}: it needs a player's token",
                    parts.method,
                    parts.uri.path()
                );
                Err(status(StatusCode::FORBIDDEN))
            }
        }
    }
}

/// The player behind a valid bearer token, if there is one (`SoftMetagameAuth`); never refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftPlayer(pub Option<TokenClaims>);

impl FromRequestParts<AppState> for SoftPlayer {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let Some(value) = header(parts, "authorization").filter(|value| has_bearer_prefix(value)) else {
            return Ok(Self(None));
        };
        let claims = validate_token(state, after_scheme(value))
            .await
            .map_err(IntoResponse::into_response)?;
        Ok(Self(claims.filter(|claims| !claims.user_id.is_empty())))
    }
}

/// A route only a game server may call (`GameServerKeyAuth`). A player token that comes along is
/// read softly, for the log only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameServer {
    pub forwarded_user_id: Option<String>,
}

impl FromRequestParts<AppState> for GameServer {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let Some(key) = header(parts, GAMESERVER_KEY).filter(|key| !key.is_empty()) else {
            tracing::warn!(
                "Refusing {} {}: only a game server may call it",
                parts.method,
                parts.uri.path()
            );
            let reply = serde_json::json!({"code": "", "message": "Only a game server may call this.", "payload": {}});
            return Err((StatusCode::FORBIDDEN, Json(reply)).into_response());
        };
        if !origin(parts).is_direct_local(&state.origin, &state.own_addresses) {
            return Err(status(StatusCode::FORBIDDEN));
        }
        if !is_valid_gameserver_key(state, key)
            .await
            .map_err(IntoResponse::into_response)?
        {
            tracing::error!("Invalid Gameserver API Key Auth");
            return Err(status(StatusCode::UNAUTHORIZED));
        }
        let forwarded =
            match header(parts, "authorization").filter(|value| value.to_ascii_lowercase().starts_with("bearer ")) {
                Some(value) => validate_token(state, after_scheme(value))
                    .await
                    .map_err(IntoResponse::into_response)?,
                None => None,
            };
        Ok(Self {
            forwarded_user_id: forwarded.map(|claims| claims.user_id).filter(|id| !id.is_empty()),
        })
    }
}

/// An admin's account key, never through a proxy (`HasUndauntedAdminApiKey`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminKey(pub UserInfo);

impl FromRequestParts<AppState> for AdminKey {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        if origin(parts).is_proxied() {
            tracing::warn!(
                "Refusing an admin request on {} {}: admin keys are not accepted through a proxy",
                parts.method,
                parts.uri.path()
            );
            return Err(status(StatusCode::FORBIDDEN));
        }
        let Some(key) = header(parts, USER_KEY) else {
            return Err(status(StatusCode::UNAUTHORIZED));
        };
        match user_for_key(state, key).await.map_err(IntoResponse::into_response)? {
            None => Err(status(StatusCode::UNAUTHORIZED)),
            Some(user) if !user.is_admin => Err(status(StatusCode::FORBIDDEN)),
            Some(user) => Ok(Self(user)),
        }
    }
}

/// A player's account key (`HasUndauntedUserApiKey`): banned accounts and addresses are refused, and
/// with `ACCOUNT_KEY_RECOVERY=1` an encrypted copy of a player's key is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserKey(pub UserInfo);

impl FromRequestParts<AppState> for UserKey {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let Some(key) = header(parts, USER_KEY) else {
            return Err(status(StatusCode::UNAUTHORIZED));
        };
        let Some(user) = user_for_key(state, key).await.map_err(IntoResponse::into_response)? else {
            return Err(status(StatusCode::UNAUTHORIZED));
        };
        if let Some(refusal) = check_player_access(state, parts, Some(&user.user_id))
            .await
            .map_err(IntoResponse::into_response)?
        {
            return Err(refusal);
        }
        if !user.is_admin
            && state.auth.account_key_recovery
            && remember_account_key(state, &user.user_id, key).await.is_err()
        {
            tracing::warn!("Account recovery copy unavailable");
        }
        Ok(Self(user))
    }
}

/// Keeps the encrypted recovery copy of a player's key up to date (`RememberAccountKey`).
async fn remember_account_key(state: &AppState, user_id: &str, key: &str) -> Result<(), AppError> {
    let hash = sha256_hex(key);
    let id = user_id.to_owned();
    let stored = {
        let id = id.clone();
        state
            .store
            .read(move |conn| {
                accountkeyrecovery::table
                    .find(&id)
                    .select(accountkeyrecovery::keyHash)
                    .first::<String>(conn)
                    .optional()
                    .map_err(AppError::from)
            })
            .await?
    };
    if stored.as_deref() == Some(hash.as_str()) {
        return Ok(());
    }
    let blob = seal_recovery(&state.auth.signing_key_b64, user_id, key).map_err(|e| AppError::Other(e.to_string()))?;
    state
        .store
        .write(move |conn| {
            diesel::insert_into(accountkeyrecovery::table)
                .values((
                    accountkeyrecovery::userId.eq(&id),
                    accountkeyrecovery::keyHash.eq(&hash),
                    accountkeyrecovery::ciphertext.eq(&blob),
                ))
                .on_conflict(accountkeyrecovery::userId)
                .do_update()
                .set((
                    accountkeyrecovery::keyHash.eq(&hash),
                    accountkeyrecovery::ciphertext.eq(&blob),
                ))
                .execute(conn)
                .map(drop)
                .map_err(AppError::from)
        })
        .await
}

/// Whether the caller is a registered player (`SoftAccountAuth`): a valid account key, or a valid
/// player token whose account still exists. Never refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoftAccount(pub bool);

impl FromRequestParts<AppState> for SoftAccount {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        if let Some(key) = header(parts, USER_KEY).filter(|key| !key.is_empty() && key.len() <= 1024)
            && matches!(user_for_key(state, key).await, Ok(Some(_)))
        {
            return Ok(Self(true));
        }
        let Some(value) =
            header(parts, "authorization").filter(|value| value.to_ascii_lowercase().starts_with("bearer "))
        else {
            return Ok(Self(false));
        };
        let Ok(Some(claims)) = validate_token(state, after_scheme(value)).await else {
            return Ok(Self(false));
        };
        if claims.user_id.is_empty() {
            return Ok(Self(false));
        }
        let id = claims.user_id;
        let exists = state
            .store
            .read(move |conn| {
                users::table
                    .find(&id)
                    .select(users::userId)
                    .first::<String>(conn)
                    .optional()
                    .map_err(AppError::from)
            })
            .await
            .map(|row| row.is_some())
            .unwrap_or(false);
        Ok(Self(exists))
    }
}
