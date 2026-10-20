//! `routes/inventory.ts`: the inventory read, the transaction the game server sends after a hunt (and
//! a store or crafting change), and the single instanced item update. Both writes count against the
//! per-account `PlayerMutation` limit (game servers are never counted).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit;
use service_kit::routing::{Params, Router};

use super::{empty, json, ok};
use crate::app::{AppError, AppState};
use crate::auth::MetagameAuth;
use crate::characters::belongs;
use crate::inventory::{self, Fail, InventoryCaller, InventoryError, TransactionRequest};
use crate::js;
use crate::unlocker;

pub fn routes() -> Router<AppState> {
    Router::new()
        .post("/inventory/:characterId/:changeList", migration)
        .get("/inventory/:userId/:characterId", read)
        .post("/inventory", transaction)
        .post("/inventory/instanceditem", instanced_item)
}

fn caller(auth: &MetagameAuth) -> InventoryCaller {
    if auth.is_game_server() {
        InventoryCaller::GameServer
    } else {
        InventoryCaller::Client
    }
}

fn failed(error: InventoryError) -> Response {
    empty(StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
}

/// `PlayerMutationRateLimit`, after authentication: counted per account, game servers skipped.
async fn limited(
    state: &AppState,
    auth: &MetagameAuth,
    handler: impl Future<Output = Result<Response, AppError>>,
) -> Response {
    let key = (!auth.is_game_server()).then(|| format!("account:{}", auth.user_id().unwrap_or("undefined")));
    ratelimit::around(&state.limits.player_mutation, key.as_deref(), async {
        handler.await.unwrap_or_else(IntoResponse::into_response)
    })
    .await
}

/// `DoesCharacterBelongToUserId`: both ids must be strings.
async fn owns(state: &AppState, user_id: Option<&str>, character_id: Option<&Value>) -> Result<bool, AppError> {
    let (Some(user), Some(character)) = (user_id, character_id.and_then(Value::as_str)) else {
        return Ok(false);
    };
    let (user, character) = (user.to_owned(), character.to_owned());
    state
        .store
        .read(move |conn| belongs(conn, &user, &character).map_err(AppError::from))
        .await
}

async fn migration() -> Response {
    tracing::info!("Inventory migration (stubbed)");
    ok(json!({"code": null, "message": ""}))
}

async fn read(State(state): State<AppState>, auth: MetagameAuth, params: Params) -> Result<Response, AppError> {
    let user_id = if auth.is_game_server() {
        params.get("userId")
    } else {
        auth.user_id()
    }
    .map(str::to_owned);
    let character_id = params.get("characterId").unwrap_or("").to_owned();
    tracing::info!(
        "UserId {} requested inventory for CharacterId {character_id}",
        user_id.as_deref().unwrap_or("undefined")
    );
    let character = Value::String(character_id.clone());
    if !owns(&state, user_id.as_deref(), Some(&character)).await? {
        return Ok(failed(InventoryError::Forbidden));
    }
    let user_id = user_id.unwrap_or_default();
    let (c, u) = (character_id.clone(), user_id.clone());
    match state.store.write(move |conn| inventory::read(conn, &c, &u)).await {
        Ok(found) => Ok(ok(found)),
        Err(fail) => {
            match &fail {
                Fail::Syntax(_) => tracing::error!(
                    "Invalid inventory data while fetching inventory for characterId {character_id} and userId {user_id}: {}",
                    fail.message()
                ),
                _ => tracing::error!(
                    "Failed to fetch inventory for characterId {character_id} and userId {user_id}: {}",
                    fail.message()
                ),
            }
            Ok(failed(fail.error()))
        }
    }
}

async fn transaction(State(state): State<AppState>, auth: MetagameAuth, ExpressBody(body): ExpressBody) -> Response {
    limited(&state, &auth, run_transaction(&state, &auth, body)).await
}

async fn run_transaction(state: &AppState, auth: &MetagameAuth, body: Option<Value>) -> Result<Response, AppError> {
    let (a, b) = (auth.clone(), body.clone());
    match state
        .store
        .write(move |conn| unlocker::enforce(conn, &a, b.as_ref()).map_err(AppError::from))
        .await
    {
        Ok(detection) if detection.blocked => {
            let error = if detection.suspended {
                "account_banned"
            } else {
                "forbidden"
            };
            return Ok(json(StatusCode::FORBIDDEN, json!({"error": error})));
        }
        Ok(_) => {}
        Err(_) => {
            tracing::error!("Unlocker enforcement failed; inventory mutation refused");
            return Ok(json(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": "inventory_unavailable"}),
            ));
        }
    }
    let body = body.ok_or_else(|| AppError::Other("POST /inventory without a body".into()))?;
    let user_id = if auth.is_game_server() {
        body.get("accountId").and_then(Value::as_str)
    } else {
        auth.user_id()
    };
    let request = TransactionRequest {
        user_id: user_id.map(str::to_owned),
        character_id: body.get("characterId").cloned(),
        transaction_id: body.get("transactionId").cloned(),
        add_instanced: body.get("addInstancedItems").cloned(),
        add_stacked: body.get("addStackedItems").cloned(),
        remove_instanced: body.get("removeInstancedItems").cloned(),
        remove_stacked: body.get("removeStackedItems").cloned(),
        save_instanced: body.get("saveInstancedItems").cloned(),
    };
    let source = body.get("source").cloned();
    let caller = caller(auth);
    let transaction = js::string(request.transaction_id.as_ref());
    let user = request.user_id.clone().unwrap_or_else(|| "undefined".into());
    let character = js::string(request.character_id.as_ref());

    let outcome = run(state, request, caller, source, (&transaction, &user, &character)).await?;
    match outcome {
        Ok((response, replayed)) => {
            if replayed {
                tracing::warn!(
                    "Replayed the stored result of transactionId {transaction} for userId {user} and characterId {character}; the retry changed nothing"
                );
            } else {
                tracing::info!("Ran transactionId {transaction} for userId {user} and characterId {character}");
            }
            Ok(ok(response))
        }
        Err(error) => {
            tracing::error!(
                "transactionId {transaction} for userId {user} and characterId {character} FAILED! ({}, answered {})",
                error.name(),
                error.status()
            );
            Ok(failed(error))
        }
    }
}

/// `RunInventoryTransaction`.
async fn run(
    state: &AppState,
    request: TransactionRequest,
    caller: InventoryCaller,
    source: Option<Value>,
    (transaction, user, character): (&str, &str, &str),
) -> Result<Result<(Value, bool), InventoryError>, AppError> {
    if !owns(state, request.user_id.as_deref(), request.character_id.as_ref()).await? {
        tracing::error!("Specified characterId {character} does not belong to user {user}");
        return Ok(Err(InventoryError::Forbidden));
    }
    let shown_id = request
        .transaction_id
        .as_ref()
        .map_or("undefined".to_owned(), service_kit::jsjson::stringify);
    let logged = request.clone();
    let prepared = match inventory::prepare(request) {
        Ok(prepared) => prepared,
        Err(fail) => {
            tracing::warn!(
                "transactionId {transaction} for userId {user} and characterId {character}: {}",
                fail.message()
            );
            return Ok(Err(InventoryError::InvalidItem));
        }
    };
    if !prepared.touches_anything {
        return Ok(Ok((inventory::empty_response(&prepared), false)));
    }
    if prepared.dedupe.is_none() {
        tracing::warn!(
            "transactionId {shown_id} for userId {user} and characterId {character} is not usable for retry detection; running it without"
        );
    }
    let settings = state.inventory;
    let applied_source = source.clone();
    let applied = state
        .store
        .write(move |conn| {
            // Checked again inside the transaction, like every caller of the core
            inventory::assert_owned(
                conn,
                prepared.request.user_id.as_deref(),
                prepared.request.character_id.as_ref(),
            )?;
            inventory::apply(conn, &prepared, caller, applied_source.as_ref(), &settings)
        })
        .await;
    match applied {
        Ok(applied) => {
            inventory::log_overspend(&logged, caller, source.as_ref(), &applied.overspent);
            Ok(Ok((applied.response, applied.replayed)))
        }
        Err(fail) => {
            match &fail {
                Fail::Forbidden(message) => tracing::error!("{message}"),
                Fail::Insufficient(message) => tracing::warn!(
                    "Refusing transactionId {transaction} for userId {user} and characterId {character}: {message}; nothing was changed"
                ),
                Fail::Conflict(message) | Fail::Validation(message) => tracing::warn!("{message}"),
                Fail::Syntax(message) => tracing::error!(
                    "Invalid inventory data while running transaction {transaction} for characterId {character} and userId {user}: {message}"
                ),
                Fail::Broken(message) => tracing::error!(
                    "Failed to run inventory transaction {transaction} for characterId {character} and userId {user}: {message}"
                ),
            }
            Ok(Err(fail.error()))
        }
    }
}

async fn instanced_item(State(state): State<AppState>, auth: MetagameAuth, ExpressBody(body): ExpressBody) -> Response {
    limited(&state, &auth, update_instanced(&state, &auth, body)).await
}

/// `UpdateInstancedItem`.
async fn update_instanced(state: &AppState, auth: &MetagameAuth, body: Option<Value>) -> Result<Response, AppError> {
    let body = body.ok_or_else(|| AppError::Other("POST /inventory/instanceditem without a body".into()))?;
    let character_id = body.get("characterId").cloned();
    let user_id = if auth.is_game_server() {
        body.get("accountId").and_then(Value::as_str)
    } else {
        auth.user_id()
    }
    .map(str::to_owned);
    let user = user_id.clone().unwrap_or_else(|| "undefined".into());
    let character = js::string(character_id.as_ref());
    let catalog = js::string(body.get("catalogId"));
    let caller = caller(auth);
    if caller == InventoryCaller::Client {
        tracing::warn!(
            event = "inventory_authority_denied",
            userId = user.as_str(),
            operation = "instance_update"
        );
        return Ok(failed(InventoryError::Forbidden));
    }
    if !owns(state, user_id.as_deref(), character_id.as_ref()).await? {
        tracing::error!("Specified characterId {character} does not belong to user {user}");
        return Ok(failed(InventoryError::Forbidden));
    }
    // The incoming item as Node builds it: undefined fields are left out
    let mut incoming = Map::new();
    for key in ["catalogId", "instanceId", "itemData", "updateVersion"] {
        if let Some(value) = body.get(key) {
            incoming.insert(key.into(), value.clone());
        }
    }
    let item_data = body.get("itemData").cloned();
    let version = body.get("updateVersion").cloned();
    let (c, u) = (character.clone(), user.clone());
    let updated = state
        .store
        .write(move |conn| inventory::update_instanced(conn, &c, incoming, item_data, version, &u, caller))
        .await;
    Ok(match updated {
        Ok(Ok(item)) => ok(item),
        Ok(Err(error)) => failed(error),
        Err(fail) => {
            match &fail {
                Fail::Conflict(message) | Fail::Validation(message) => tracing::warn!("{message}"),
                Fail::Syntax(message) => tracing::error!(
                    "Invalid inventory data while updating instanced item {catalog} for characterId {character} and userId {user}: {message}"
                ),
                other => tracing::error!(
                    "Failed to update instanced item {catalog} for characterId {character} and userId {user}: {}",
                    other.message()
                ),
            }
            failed(fail.error())
        }
    })
}
