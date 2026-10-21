//! `middleman-store.test.ts`: the two exchange-slot SKUs, with STORE off, TRIALS_STORE off and
//! MIDDLEMAN_STORE on.

use metagame::store::{Offer, StoreFail};
use serde_json::{Value, json};

use crate::common::req;
use crate::storekit::{app_with, create, offer, offers, redeem};

fn ids(list: &[Offer]) -> Vec<Value> {
    list.iter().map(|o| o["id"].clone()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_the_two_exchange_slot_skus_the_1_4_4_client_has_art_for() {
    let app = app_with(false, true, &[]).await;
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
    let app = app_with(false, true, &[]).await;
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
    let app = app_with(false, true, &[]).await;
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
    let app = app_with(false, false, &[]).await;
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
