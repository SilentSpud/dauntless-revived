//! Store helpers shared by the store test ports: Node's `ListStoreOffers`, `GetStoreOffer`,
//! `CreateStorePurchase` (over GET /token, or directly as `token_in`) and `RedeemStorePurchase` (called
//! directly, so a replay shows). The `_in` forms take a `Context`, so a test can change the store's
//! settings part-way as Node's `WithEnv` does.

use metagame::store::{self as shop, Context, Offer, StoreFail};
use serde_json::json;
use service_kit::env::Env;

use crate::common::{TestApp, req};

/// An app with STORE off and the two side stores as given, plus `extra`.
pub(crate) async fn app_with(trials: bool, middleman: bool, extra: &[(&str, &str)]) -> TestApp {
    let on = |flag: bool| if flag { "1" } else { "0" };
    let mut env = vec![
        ("STORE", "off"),
        ("TRIALS_STORE", on(trials)),
        ("MIDDLEMAN_STORE", on(middleman)),
    ];
    env.extend_from_slice(extra);
    TestApp::with_env(&env).await
}

pub(crate) fn at(text: &str) -> i64 {
    metagame::js::date_parse(text).unwrap()
}

pub(crate) fn family(offer: &Offer) -> String {
    let id = offer["items"][0]["catalogId"].as_str().unwrap();
    regex::Regex::new("_(UC|R)$").unwrap().replace(id, "").into_owned()
}

/// The app's store context with its settings read from `env` instead (Node's `WithEnv`).
pub(crate) fn context(app: &TestApp, env: &[(&str, &str)]) -> Context {
    let mut ctx = app.state.store_context();
    ctx.settings = shop::Settings::from_env(&Env::from_pairs(env.iter().copied()));
    ctx
}

pub(crate) async fn offers(app: &TestApp, user: &str, tag: &str) -> Vec<Offer> {
    offers_in(app, &app.state.store_context(), user, tag).await
}

pub(crate) async fn offers_in(app: &TestApp, ctx: &Context, user: &str, tag: &str) -> Vec<Offer> {
    let (ctx, user, tag) = (ctx.clone(), user.to_owned(), tag.to_owned());
    let for_tag = shop::offers_for_tag(&ctx, &tag);
    app.state
        .store
        .write(move |conn| shop::list_offers(conn, &ctx, &user, &tag, for_tag))
        .await
        .unwrap()
}

/// `GetStoreOffer`
pub(crate) async fn offer(app: &TestApp, user: &str, sku: &str) -> Result<Offer, StoreFail> {
    offer_in(app, &app.state.store_context(), user, sku).await
}

pub(crate) async fn offer_in(app: &TestApp, ctx: &Context, user: &str, sku: &str) -> Result<Offer, StoreFail> {
    let (ctx, user) = (ctx.clone(), user.to_owned());
    let (found, _) = shop::lookup(&ctx, sku, &user)?;
    app.state
        .store
        .write(move |conn| shop::offer_for(conn, &ctx, &user, found))
        .await
}

/// `CreateStorePurchase` without the route: the token, or the refusal's status and message.
pub(crate) async fn token_in(
    app: &TestApp,
    ctx: &Context,
    user: &str,
    currency: &str,
    sku: &str,
) -> Result<String, (u16, String)> {
    let (ctx, user, sku) = (ctx.clone(), user.to_owned(), sku.to_owned());
    let issued = async {
        let currency = shop::rams_wire_currency(&ctx.settings, currency, &user);
        let canonical = shop::canonical_currency(&currency)?;
        let (offer, _) = shop::lookup(&ctx, &sku, &user)?;
        shop::check_sale(&offer, canonical)?;
        app.state
            .store
            .write(move |conn| shop::issue_token(conn, &ctx, &user, &sku, &offer))
            .await
    };
    issued.await.map(|(token, _)| token).map_err(refusal)
}

/// `CreateStorePurchase` over GET /token: the token, or the refusal's status.
pub(crate) async fn create(app: &TestApp, user: &str, currency: &str, sku: &str) -> Result<String, u16> {
    let reply = app.get(&format!("/token/{currency}/{sku}"), req().bearer(user)).await;
    match reply.status {
        200 => Ok(reply.json["purchaseToken"].as_str().unwrap().to_owned()),
        status => Err(status),
    }
}

/// `RedeemStorePurchase`: whether it was a replay, or the refusal's status.
pub(crate) async fn redeem(app: &TestApp, user: &str, currency: &str, token: &str) -> Result<bool, u16> {
    redeem_in(app, &app.state.store_context(), user, currency, token)
        .await
        .map_err(|(status, _)| status)
}

pub(crate) async fn redeem_in(
    app: &TestApp,
    ctx: &Context,
    user: &str,
    currency: &str,
    token: &str,
) -> Result<bool, (u16, String)> {
    let (currency, hash) = shop::check_redeem(currency, Some(&json!(token))).map_err(refusal)?;
    let (ctx, user) = (ctx.clone(), user.to_owned());
    let done = app
        .state
        .store
        .write(move |conn| shop::redeem(conn, &ctx, &user, currency, &hash))
        .await;
    done.map(|r| r.replayed).map_err(refusal)
}

/// A refusal's status and message; anything else fails the test.
pub(crate) fn refusal(fail: StoreFail) -> (u16, String) {
    match fail {
        StoreFail::Refused(status, message) => (status, message),
        StoreFail::Internal(message) => panic!("{message}"),
    }
}

pub(crate) fn credit(app: &TestApp, character: &str, catalog: &str, quantity: i64) {
    let stacked = json!([{"catalogId": catalog, "quantity": quantity}]).to_string();
    app.run(
        "INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES (?1, '[]', ?2) \
         ON CONFLICT(characterId) DO UPDATE SET stackedItems = excluded.stackedItems",
        &[&character, &stacked],
    );
}

pub(crate) fn price(offer: &Offer) -> i64 {
    offer["cellDustPrice"].as_i64().unwrap()
}

/// The instanced items of a character's inventory.
pub(crate) fn instanced(app: &TestApp, character: &str) -> Vec<serde_json::Value> {
    let row = app.row(
        "SELECT instancedItems FROM inventories WHERE characterId = ?1",
        &[&character],
    );
    serde_json::from_str(row["instancedItems"].as_str().unwrap_or("[]")).unwrap()
}

/// `Buy`: a token and its redeem.
pub(crate) async fn buy(app: &TestApp, user: &str, currency: &str, sku: &str) -> bool {
    let token = create(app, user, currency, sku).await.unwrap();
    redeem(app, user, currency, &token).await.unwrap()
}

/// `MakePlayer` with a chosen account id, for settings that name the account.
pub(crate) async fn player_named(app: &TestApp, user: &str) -> String {
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?1, 0, 0)",
        &[&user],
    );
    let u = user.to_owned();
    app.state
        .store
        .write(move |conn| metagame::characters::create(conn, &u, &u).map_err(store::Error::from))
        .await
        .unwrap()
        .characterId
}
