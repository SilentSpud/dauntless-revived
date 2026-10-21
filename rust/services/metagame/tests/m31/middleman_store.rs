//! `middleman-store.test.ts`: the two exchange-slot SKUs (and the store helpers of
//! `middleman_rotation`), with STORE off, TRIALS_STORE off and MIDDLEMAN_STORE on. A purchase token is
//! issued through GET /token (Node's `CreateStorePurchase`); its redeem is called directly so the
//! replay shows.

use metagame::store::{self as shop, Offer, StoreFail};
use serde_json::{Value, json};

use crate::common::{TestApp, req};

pub(crate) const WEEKLY: &str = "weekly_cell_offering";

pub(crate) async fn app_with(extra: &[(&str, &str)]) -> TestApp {
    let mut env = vec![("STORE", "off"), ("TRIALS_STORE", "0"), ("MIDDLEMAN_STORE", "1")];
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

pub(crate) async fn offers(app: &TestApp, user: &str, tag: &str) -> Vec<Offer> {
    let (ctx, user, tag) = (app.state.store_context(), user.to_owned(), tag.to_owned());
    let for_tag = shop::offers_for_tag(&ctx, &tag);
    app.state
        .store
        .write(move |conn| shop::list_offers(conn, &ctx, &user, &tag, for_tag))
        .await
        .unwrap()
}

/// `GetStoreOffer`
pub(crate) async fn offer(app: &TestApp, user: &str, sku: &str) -> Result<Offer, StoreFail> {
    let (ctx, user) = (app.state.store_context(), user.to_owned());
    let (found, _) = shop::lookup(&ctx, sku, &user)?;
    app.state
        .store
        .write(move |conn| shop::offer_for(conn, &ctx, &user, found))
        .await
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
    let (currency, hash) = shop::check_redeem(currency, Some(&json!(token))).map_err(status)?;
    let (ctx, user, hash) = (app.state.store_context(), user.to_owned(), hash.clone());
    let done = app
        .state
        .store
        .write(move |conn| shop::redeem(conn, &ctx, &user, currency, &hash))
        .await;
    done.map(|r| r.replayed).map_err(status)
}

fn status(fail: StoreFail) -> u16 {
    match fail {
        StoreFail::Refused(status, _) => status,
        StoreFail::Internal(message) => panic!("{message}"),
    }
}

pub(crate) fn credit(app: &TestApp, character: &str, quantity: i64) {
    let stacked = json!([{"catalogId": "CURRENCY_CELLDUST", "quantity": quantity}]).to_string();
    app.run(
        "INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES (?1, '[]', ?2) \
         ON CONFLICT(characterId) DO UPDATE SET stackedItems = excluded.stackedItems",
        &[&character, &stacked],
    );
}

pub(crate) fn price(offer: &Offer) -> i64 {
    offer["cellDustPrice"].as_i64().unwrap()
}

fn ids(list: &[Offer]) -> Vec<Value> {
    list.iter().map(|o| o["id"].clone()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_the_two_exchange_slot_skus_the_1_4_4_client_has_art_for() {
    let app = app_with(&[]).await;
    let (user, _) = app.make_player().await;
    let slot2 = offers(&app, &user, "exchange_vendor_slot_2").await;
    let slot3 = offers(&app, &user, "exchange_vendor_slot_3").await;
    assert_eq!(ids(&slot2), [json!("single_exchange_slot_2")]);
    assert_eq!(ids(&slot3), [json!("single_exchange_slot_3")]);
    assert_eq!(slot2[0]["platinumPrice"], 0);
    assert_eq!(
        slot2[0]["entitlements"],
        json!([{"name": "exchange_slot_2", "duration": 0}])
    );
    assert_eq!(
        slot3[0]["entitlements"],
        json!([{"name": "exchange_slot_3", "duration": 0}])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unlocks_each_slot_once_through_the_normal_two_step_store_transaction() {
    let app = app_with(&[]).await;
    let (user, _) = app.make_player().await;
    for (sku, entitlement) in [
        ("single_exchange_slot_2", "exchange_slot_2"),
        ("single_exchange_slot_3", "exchange_slot_3"),
    ] {
        let token = create(&app, &user, "platinum", sku).await.unwrap();
        assert_eq!(redeem(&app, &user, "platinum", &token).await, Ok(false));
        assert_eq!(redeem(&app, &user, "platinum", &token).await, Ok(true));
        assert_eq!(
            app.count("entitlements", "accountId = ?1 AND name = ?2", &[&user, &entitlement]),
            1
        );
        assert_eq!(offer(&app, &user, sku).await.unwrap()["remaining"], 0);
        assert_eq!(create(&app, &user, "platinum", sku).await, Err(409));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn is_reachable_by_the_authenticated_1_4_4_store_routes_while_store_remains_off() {
    let app = app_with(&[]).await;
    let (user, _) = app.make_player().await;
    let listed = app
        .get(
            "/product/skus/public?requiredTags=exchange_vendor_slot_2",
            req().bearer(&user),
        )
        .await;
    assert_eq!(listed.status, 200);
    assert_eq!(
        listed
            .json
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["id"].clone())
            .collect::<Vec<_>>(),
        [json!("single_exchange_slot_2")]
    );
    let single = app
        .get("/product/sku/single_exchange_slot_3", req().bearer(&user))
        .await;
    assert_eq!(
        (single.status, single.json["id"].clone()),
        (200, json!("single_exchange_slot_3"))
    );
    assert_eq!(
        app.get("/product/skus/public?requiredTags=webstore", req().bearer(&user))
            .await
            .status,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hides_direct_middleman_offers_when_middleman_store_is_off() {
    let app = app_with(&[("MIDDLEMAN_STORE", "0")]).await;
    let (user, _) = app.make_player().await;
    assert_eq!(offers(&app, &user, "exchange_vendor_slot_2").await, Vec::<Offer>::new());
    assert!(matches!(
        offer(&app, &user, "single_exchange_slot_2").await,
        Err(StoreFail::Refused(404, _))
    ));
    let listed = app
        .get(
            "/product/skus/public?requiredTags=exchange_vendor_slot_2",
            req().bearer(&user),
        )
        .await;
    assert_eq!(listed.status, 400);
    assert_eq!(
        app.get("/product/sku/single_exchange_slot_2", req().bearer(&user))
            .await
            .status,
        404
    );
}
