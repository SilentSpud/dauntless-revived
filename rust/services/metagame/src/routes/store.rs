//! `routes/store.ts`: balance sheets, the creator code, store offer pictures and the store purchase
//! service. `STORE=free` controls the ordinary cosmetic webstore; Lady Luck and the Middleman share
//! its token/notification protocol but are gated by `TRIALS_STORE` and `MIDDLEMAN_STORE`, so the
//! old storefront can stay off without turning off gameplay vendors. A store that is off passes the
//! request on (`next("route")`): the offer list to the "not available" reply, the rest to the 404.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, HOST, HeaderValue};
use axum::http::request::Parts;
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use serde_json::{Map, Value, json};
use service_kit::body::ExpressBody;
use service_kit::ratelimit::{self, enforce};
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, json, ok, query};
use crate::app::{AppError, AppState};
use crate::auth::{MetagameAuth, Player, metagame_auth};
use crate::creator::{self, RedeemFail};
use crate::limits::per_client;
use crate::store::{self, Offer, StoreFail};
use crate::trials;

pub fn routes(state: &AppState) -> Router<AppState> {
    let image_limit = per_client(&state.limits.store_image, state.origin.clone());
    Router::new()
        .post("/reconcile", reconcile)
        .get("/creator", creator_support)
        .post("/creator", creator_redeem)
        .get("/balance", balance)
        .get("/store-images/:file", store_image)
        .layer(from_fn_with_state(image_limit, enforce))
        .get("/product/skus/public", list)
        .get("/product/sku/:skuId", offer)
        .get("/token/:currency/:skuId", token)
        .post("/notification/:currency", notification)
}

fn store_error(fail: &StoreFail, what: &str) -> Response {
    match fail {
        StoreFail::Refused(status, message) => {
            tracing::warn!("Store {what} refused ({status}): {message}");
            json(
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                json!({"code": status.to_string(), "message": message}),
            )
        }
        StoreFail::Internal(message) => {
            tracing::error!(err = message.as_str(), "Store {what} failed");
            json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"code": "500", "message": "Store request failed"}),
            )
        }
    }
}

/// `PlayerMutationRateLimit`: counted per account, game servers skipped.
async fn limited(state: &AppState, player: &Player, handler: impl Future<Output = Response>) -> Response {
    let key = (!player.via_game_server).then(|| format!("account:{}", player.user_id));
    ratelimit::around(&state.limits.player_mutation, key.as_deref(), handler).await
}

/// `RequireRamsClient`: with `STORE_RAMS=1` the main shop answers only launchers that speak Rams.
fn require_rams_client(state: &AppState, parts: &Parts, applies: bool) -> Result<(), StoreFail> {
    let protocol = query(parts.uri.query()).get("shop_protocol").cloned();
    if applies && state.shop.rams && protocol != Some(json!("rams-v1")) {
        return Err(StoreFail::Refused(
            426,
            "Update the launcher to use the Rams shop".into(),
        ));
    }
    Ok(())
}

// ---- Balances ----

/// `ApplyHeldCurrencies`: with `BALANCE_FROM_INVENTORY` on, the currencies the account's active
/// character holds replace the sheet's values. The held stacks are returned for reuse.
async fn apply_held(
    state: &AppState,
    account: Option<&str>,
    sheet: &mut Map<String, Value>,
) -> Result<Option<store::Held>, AppError> {
    let Some(account) = account.filter(|_| state.shop.balance_from_inventory) else {
        return Ok(None);
    };
    let (character, held) = held_currencies(state, account).await?;
    let changed = store::overlay_held(sheet, &held);
    if !changed.is_empty() {
        let shown: Vec<String> = changed.iter().map(|key| format!("{key} {}", sheet[key])).collect();
        tracing::info!(
            "Balances of {account} from the inventory of character {}: {}",
            character.as_deref().unwrap_or("undefined"),
            shown.join(", ")
        );
    }
    Ok(Some(held))
}

async fn held_currencies(state: &AppState, account: &str) -> Result<(Option<String>, store::Held), AppError> {
    let account = account.to_owned();
    state
        .store
        .read(move |conn| store::held_currencies(conn, &account))
        .await
        .map_err(|fail| match fail {
            StoreFail::Refused(_, message) | StoreFail::Internal(message) => AppError::Other(message),
        })
}

async fn notes(state: &AppState, account: Option<&str>) -> Result<i64, AppError> {
    let account = account.unwrap_or("").to_owned();
    let found = state
        .store
        .read(move |conn| store::notes_of(conn, &account).map_err(AppError::from))
        .await?;
    // Node read `.notes` of a missing user: a TypeError, the 500 page
    found.ok_or_else(|| AppError::Other("TypeError: Cannot read properties of undefined (reading 'notes')".into()))
}

async fn reconcile(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    let account = auth.user_id();
    let notes = notes(&state, account).await?;
    tracing::info!(
        "Retrieved notes balance of {notes} for {}",
        account.unwrap_or("undefined")
    );
    let mut balances = Map::new();
    balances.insert("id_currency_notes".into(), json!(notes));
    balances.insert("CURRENCY_NOTES".into(), json!(notes));
    apply_held(&state, account, &mut balances).await?;
    Ok(ok(json!({"balances": balances, "refreshInventory": true})))
}

async fn balance(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    let account = auth.user_id();
    let notes = notes(&state, account).await?;
    tracing::info!(
        "Fetched notes balance of {notes} for userId {}",
        account.unwrap_or("undefined")
    );
    let mut sheet = store::balance_sheet(notes);
    let held = apply_held(&state, account, &mut sheet).await?;
    // The pilot client still indexes its purchase balance by Event01 even when Rams metadata is
    // displayed. These are wire aliases, never separate funds.
    let account = account.unwrap_or("");
    if store::rams_on_event_wire(&state.shop, account) {
        let held = match held {
            Some(held) => held,
            None => held_currencies(&state, account).await?.1,
        };
        let rams = store::held_value(&held, "CURRENCY_NOTES");
        sheet.insert("CURRENCY_NOTES".into(), rams.clone());
        sheet.insert("CURRENCY_EVENT_01".into(), rams);
    }
    Ok(ok(Value::Object(sheet)))
}

// ---- The creator code ----

/// `CreatorAuth`: native creator requests also carry their player token in X-Epic-Authorization
/// (GET) or `authorization` in the JSON body (POST). Never an account id.
async fn creator_player(state: &AppState, mut parts: Parts, body: &Option<Value>) -> Result<Player, Response> {
    if !parts.headers.contains_key(AUTHORIZATION) {
        let token = parts
            .headers
            .get("x-epic-authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| {
                super::field(body, "authorization")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        if let Some(token) = token.filter(|t| t.encode_utf16().count() <= 8192) {
            let bearer = if token.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("bearer ")) {
                token
            } else {
                format!("bearer {token}")
            };
            if let Ok(value) = HeaderValue::from_str(&bearer) {
                parts.headers.insert(AUTHORIZATION, value);
            }
        }
    }
    Player::from_request_parts(&mut parts, state).await
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn creator_support(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let player = match creator_player(&state, parts, &body).await {
        Ok(player) => player,
        Err(refusal) => return Ok(refusal),
    };
    let account = player.user_id;
    let support = state
        .store
        .read(move |conn| creator::support(conn, &account).map_err(AppError::from))
        .await?;
    Ok(no_store(ok(support)))
}

async fn creator_redeem(State(state): State<AppState>, parts: Parts, ExpressBody(body): ExpressBody) -> Response {
    let player = match creator_player(&state, parts, &body).await {
        Ok(player) => player,
        Err(refusal) => return refusal,
    };
    limited(&state, &player, async {
        let failed =
            |status: StatusCode, message: &str| no_store(json(status, json!({"success": false, "message": message})));
        if let Err(StoreFail::Refused(status, message)) = creator::check_slug(super::field(&body, "slug")) {
            return failed(StatusCode::from_u16(status).unwrap_or(StatusCode::NOT_FOUND), &message);
        }
        let (ctx, account) = (state.store_context(), player.user_id.clone());
        match state
            .store
            .write(move |conn| creator::redeem(conn, &ctx, &account))
            .await
        {
            Ok(reply) => no_store(ok(reply)),
            Err(RedeemFail::Store(StoreFail::Refused(status, message))) => failed(
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                &message,
            ),
            Err(RedeemFail::Inventory) => failed(
                StatusCode::CONFLICT,
                "Inventory could not receive the reward; try again later",
            ),
            Err(RedeemFail::Store(StoreFail::Internal(message))) => {
                tracing::error!(err = message.as_str(), "Creator reward could not be committed");
                failed(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not redeem the creator code; try again later",
                )
            }
        }
    })
    .await
}

// ---- Store offer pictures ----

async fn store_image(State(state): State<AppState>, params: Params, request: Request) -> Response {
    let file = params.get("file").unwrap_or("").to_owned();
    state.store_images.serve(&file, request).await
}

/// `WithStoreImage`: an offer whose first item has a picture gets its URL on this server, built from
/// the host the client called so it works behind the relay too.
fn with_image(state: &AppState, parts: &Parts, mut offer: Offer) -> Offer {
    let file = offer
        .get("items")
        .and_then(|items| items.get(0))
        .and_then(|item| item.get("catalogId"))
        .and_then(Value::as_str)
        .and_then(|id| state.store_images.file_for(id));
    let Some(file) = file else {
        return offer;
    };
    let host = parts
        .headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("undefined");
    let url = json!(format!("http://{host}/store-images/{file}"));
    let mut images = offer
        .get("images")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    images.insert("standard".into(), url.clone());
    images.insert("feature".into(), url);
    offer.insert("images".into(), Value::Object(images));
    offer
}

fn wire(state: &AppState, parts: &Parts, offer: Offer, account: &str) -> Value {
    Value::Object(with_image(
        state,
        parts,
        store::rams_wire_offer(&state.shop, offer, account),
    ))
}

// ---- The purchase service ----

/// `FinalizeCompletedTrialWeeks`, in its own transaction.
async fn settle_trials(state: &AppState) -> Result<(), StoreFail> {
    let ctx = state.store_context();
    state
        .store
        .write(move |conn| trials::finalize_completed_weeks(conn, &ctx))
        .await
        .map(|_| ())
}

/// The STORE=off answer the store screen always got.
async fn store_off(state: &AppState, parts: &mut Parts) -> Response {
    if let Err(refusal) = metagame_auth(parts, state).await {
        return refusal;
    }
    tracing::info!("Store SKUs (stubbed)");
    json(
        StatusCode::BAD_REQUEST,
        json!({"code": "400", "message": "The store is not available on Dauntless Revived yet."}),
    )
}

/// StoreGetItemByTagEndpoint: a bare array of offers.
async fn list(State(state): State<AppState>, mut parts: Parts) -> Response {
    let required = query(parts.uri.query()).get("requiredTags").cloned();
    let on = match required.as_ref().and_then(Value::as_str) {
        Some(tag) => state.shop.tag_enabled(tag),
        None => state.shop.any_enabled(),
    };
    if !on {
        return store_off(&state, &mut parts).await;
    }
    let player = match Player::from_request_parts(&mut parts, &state).await {
        Ok(player) => player,
        Err(refusal) => return refusal,
    };
    let tag = match required {
        Some(Value::String(tag)) if !tag.is_empty() => tag,
        _ => {
            tracing::warn!("Store SKU request with no requiredTags");
            return json(
                StatusCode::BAD_REQUEST,
                json!({"code": "400", "message": "missing requiredTags query parameter"}),
            );
        }
    };
    match list_offers(&state, &parts, &player.user_id, &tag).await {
        Ok(offers) => {
            if store::is_known_tag(&tag) {
                tracing::info!("Store SKUs for tag {tag}: {} offer(s)", offers.len());
            } else {
                tracing::warn!("Store SKUs requested for unknown tag {tag}: an empty list");
            }
            let account = player.user_id.as_str();
            ok(Value::Array(
                offers
                    .into_iter()
                    .map(|offer| wire(&state, &parts, offer, account))
                    .collect(),
            ))
        }
        Err(fail) => store_error(&fail, &format!("list {tag}")),
    }
}

async fn list_offers(state: &AppState, parts: &Parts, account: &str, tag: &str) -> Result<Vec<Offer>, StoreFail> {
    require_rams_client(state, parts, tag == "webstore")?;
    let ctx = state.store_context();
    let for_tag = store::offers_for_tag(&ctx, tag);
    if tag == "ladyluckstore" && state.shop.trials_store {
        settle_trials(state).await?;
    }
    let (account, tag) = (account.to_owned(), tag.to_owned());
    state
        .store
        .write(move |conn| store::list_offers(conn, &ctx, &account, &tag, for_tag))
        .await
}

/// The gate and the player shared by the routes of one offer (`StoreSkuOn`, auth, `PlayerTokenOnly`).
async fn sku_player(state: &AppState, parts: &mut Parts, sku: &str) -> Result<Player, Response> {
    if !state.shop.sku_enabled(sku) {
        return Err(fall_through(parts));
    }
    Player::from_request_parts(parts, state).await
}

/// StoreGetItemByIdEndpoint: one offer, for the purchase dialog.
async fn offer(State(state): State<AppState>, mut parts: Parts, params: Params) -> Response {
    let sku = params.get("skuId").unwrap_or("").to_owned();
    let player = match sku_player(&state, &mut parts, &sku).await {
        Ok(player) => player,
        Err(refusal) => return refusal,
    };
    let found = async {
        require_rams_client(&state, &parts, store::is_main_shop_sku(&sku))?;
        let ctx = state.store_context();
        let (offer, settle) = store::lookup(&ctx, &sku, &player.user_id)?;
        if settle {
            settle_trials(&state).await?;
        }
        let account = player.user_id.clone();
        state
            .store
            .write(move |conn| store::offer_for(conn, &ctx, &account, offer))
            .await
    };
    match found.await {
        Ok(offer) => ok(wire(&state, &parts, offer, &player.user_id)),
        Err(fail) => store_error(&fail, &format!("offer {sku}")),
    }
}

/// StorePurchaseItemEndpoint: `{purchaseToken}`. Whatever else the request carries is ignored.
async fn token(State(state): State<AppState>, mut parts: Parts, params: Params) -> Response {
    let sku = params.get("skuId").unwrap_or("").to_owned();
    let player = match sku_player(&state, &mut parts, &sku).await {
        Ok(player) => player,
        Err(refusal) => return refusal,
    };
    let currency = params.get("currency").unwrap_or("").to_owned();
    limited(&state, &player, async {
        match issue(&state, &parts, &player.user_id, &currency, &sku).await {
            Ok(token) => ok(json!({"purchaseToken": token})),
            Err(fail) => store_error(&fail, &format!("token for {sku}")),
        }
    })
    .await
}

/// `CreateStorePurchase`: refused (409) for an offer the account already owns and past the token
/// limit.
async fn issue(state: &AppState, parts: &Parts, account: &str, currency: &str, sku: &str) -> Result<String, StoreFail> {
    require_rams_client(state, parts, store::is_main_shop_sku(sku))?;
    let currency = store::rams_wire_currency(&state.shop, currency, account);
    let canonical = store::canonical_currency(&currency)?;
    let ctx = state.store_context();
    let (offer, settle) = store::lookup(&ctx, sku, account)?;
    if settle {
        settle_trials(state).await?;
    }
    store::check_sale(&offer, canonical)?;
    if state.store_sweeps.due(ctx.now) {
        let now = ctx.now;
        let pruned = state
            .store
            .write(move |conn| store::prune(conn, now).map_err(StoreFail::from))
            .await?;
        store::log_pruned(pruned);
    }
    let (owner, offer_sku) = (account.to_owned(), sku.to_owned());
    let (token, character) = state
        .store
        .write(move |conn| store::issue_token(conn, &ctx, &owner, &offer_sku, &offer))
        .await?;
    tracing::info!("Store purchase token for {sku} issued to {account} (character {character})");
    Ok(token)
}

/// StorePurchaseItemConfirmEndpoint: redeems the token; 204, also for a token already redeemed.
async fn notification(State(state): State<AppState>, mut parts: Parts, params: Params) -> Response {
    if !state.shop.any_enabled() {
        return fall_through(&parts);
    }
    let player = match Player::from_request_parts(&mut parts, &state).await {
        Ok(player) => player,
        Err(refusal) => return refusal,
    };
    let currency = params.get("currency").unwrap_or("").to_owned();
    limited(&state, &player, async {
        match redeem(&state, &parts, &player.user_id, &currency).await {
            Ok(()) => empty(StatusCode::NO_CONTENT),
            Err(fail) => store_error(&fail, "purchase"),
        }
    })
    .await
}

/// `RedeemStorePurchase`: the grant and the receipt in one transaction.
async fn redeem(state: &AppState, parts: &Parts, account: &str, currency: &str) -> Result<(), StoreFail> {
    let lower = currency.to_lowercase();
    require_rams_client(
        state,
        parts,
        ["notes", "rams", "event01", "event_01"].contains(&lower.as_str()),
    )?;
    let currency = store::rams_wire_currency(&state.shop, currency, account);
    let token = query(parts.uri.query()).get("token").cloned();
    let (currency, token_hash) = store::check_redeem(&currency, token.as_ref())?;
    let ctx = state.store_context();
    let owner = account.to_owned();
    let done = state
        .store
        .write(move |conn| store::redeem(conn, &ctx, &owner, currency, &token_hash))
        .await?;
    if done.replayed {
        tracing::info!(
            "Store purchase {} of {account} was already redeemed; nothing granted again",
            done.sku
        );
    } else {
        tracing::info!(
            "Store purchase {} for {account} (character {}): {} item(s), {} entitlement(s)",
            done.sku,
            done.character,
            done.items,
            done.entitlements
        );
    }
    Ok(())
}
