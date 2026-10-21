//! `middleman-rotation.test.ts`: the weekly Aetherdust cells, sold once per account per reset.

use metagame::store::{self as shop, GrantKind, Offer};
use serde_json::{Value, json};

use crate::common::TestApp;
use crate::storekit::{app_with, at, create, credit, family, offers, price, redeem};

const WEEKLY: &str = "weekly_cell_offering";

#[tokio::test(flavor = "multi_thread")]
async fn shares_exact_availability_boundaries_with_all_rotating_shop_categories() {
    let app = TestApp::with_env(&[("STORE_RAMS", "1"), ("STORE_WEEKLY_ROTATION", "1")]).await;
    let boundary = shop::weekly_window(at("2026-10-17T12:00:00Z")).2;
    for timestamp in [boundary - 1, boundary, boundary + 1] {
        let (_, start, end) = shop::weekly_window(timestamp);
        let webstore: Vec<Offer> = metagame::vendor::store_catalog()["webstore"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_object().unwrap().clone())
            .collect();
        let layout = shop::shop_layout(&app.state.shop, webstore, "test", timestamp);
        let cells = shop::weekly_middleman_offers(timestamp);
        assert!(!layout.is_empty() && !cells.is_empty());
        for offer in layout.iter().chain(&cells) {
            assert_eq!(offer["availableFrom"], metagame::time::iso(start));
            assert_eq!(offer["availableTo"], metagame::time::iso(end));
        }
    }
}

#[test]
fn rotates_three_stable_families_at_the_thursday_18_00_utc_reset() {
    let (before, after) = (at("2026-10-01T17:59:59.000Z"), at("2026-10-01T18:00:00.000Z"));
    let first = shop::weekly_middleman_offers(before);
    let same = shop::weekly_middleman_offers(at("2026-09-30T12:00:00.000Z"));
    let next = shop::weekly_middleman_offers(after);
    assert_eq!(first.len(), 3);
    assert_eq!(same, first);
    let families: std::collections::HashSet<String> = first.iter().map(family).collect();
    assert_eq!(families.len(), 3);
    assert!(next.iter().all(|offer| !families.contains(&family(offer))));
    assert_eq!(shop::weekly_window(before).1, at("2026-09-24T18:00:00.000Z"));
    assert_eq!(shop::weekly_window(after).1, at("2026-10-01T18:00:00.000Z"));
    assert_eq!(first.iter().map(price).collect::<Vec<_>>(), [40, 40, 100]);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_only_this_week_s_cells_each_available_once() {
    let app = app_with(false, true, &[]).await;
    let (user, _) = app.make_player().await;
    let listed = offers(&app, &user, WEEKLY).await;
    assert_eq!(listed.len(), 3);
    for offer in &listed {
        assert_eq!(offer["remaining"], 1);
        assert!(offer["tags"].as_array().unwrap().contains(&json!(WEEKLY)));
        assert!(offer["availableFrom"].is_string() && offer["availableTo"].is_string());
        assert_eq!(
            shop::grant_kind(offer["items"][0]["catalogId"].as_str().unwrap()),
            Some(GrantKind::Stacked)
        );
        // No other price key at all, or the client offers the cell for Platinum
        for key in [
            "platinumPrice",
            "prestigePrice",
            "event01Price",
            "steelMarksPrice",
            "gildedMarksPrice",
        ] {
            assert!(!offer.contains_key(key), "{key}");
        }
    }
    let id = listed[0]["id"].as_str().unwrap();
    assert_eq!(create(&app, &user, "platinum", id).await, Err(400));
}

#[tokio::test(flavor = "multi_thread")]
async fn spends_aetherdust_atomically_and_sells_each_weekly_cell_once_per_reset() {
    let app = app_with(false, true, &[]).await;
    let (user, character) = app.make_player().await;
    credit(&app, &character, "CURRENCY_CELLDUST", 500);
    let first = offers(&app, &user, WEEKLY).await.remove(0);
    let (id, cost) = (first["id"].as_str().unwrap().to_owned(), price(&first));

    let token = create(&app, &user, "CURRENCY_CELLDUST", &id).await.unwrap();
    assert_eq!(redeem(&app, &user, "CURRENCY_CELLDUST", &token).await, Ok(false));
    assert_eq!(redeem(&app, &user, "CURRENCY_CELLDUST", &token).await, Ok(true));

    // Sold out for this account until the reset, even once the cell is no longer held
    let remaining = |list: Vec<Offer>| list.iter().find(|o| o["id"] == id.as_str()).unwrap()["remaining"].clone();
    assert_eq!(remaining(offers(&app, &user, WEEKLY).await), 0);
    assert_eq!(create(&app, &user, "CURRENCY_CELLDUST", &id).await, Err(409));
    credit(&app, &character, "CURRENCY_CELLDUST", 500 - cost);
    assert_eq!(create(&app, &user, "CURRENCY_CELLDUST", &id).await, Err(409));
    assert_eq!(app.stack_quantity(&character, "CURRENCY_CELLDUST"), 500 - cost);

    // The other cells this week are still for sale, and other accounts are unaffected
    let others: Vec<Value> = offers(&app, &user, WEEKLY)
        .await
        .iter()
        .filter(|o| o["id"] != id.as_str())
        .map(|o| o["remaining"].clone())
        .collect();
    assert!(others.iter().all(|r| r == 1));
    let (other, _) = app.make_player().await;
    assert_eq!(remaining(offers(&app, &other, WEEKLY).await), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn lets_only_the_first_of_two_outstanding_tokens_buy_this_week_s_cell() {
    let app = app_with(false, true, &[]).await;
    let (user, character) = app.make_player().await;
    credit(&app, &character, "CURRENCY_CELLDUST", 500);
    let first = offers(&app, &user, WEEKLY).await.remove(0);
    let (id, cell) = (
        first["id"].as_str().unwrap(),
        first["items"][0]["catalogId"].as_str().unwrap(),
    );
    let one = create(&app, &user, "CURRENCY_CELLDUST", id).await.unwrap();
    let two = create(&app, &user, "CURRENCY_CELLDUST", id).await.unwrap();
    redeem(&app, &user, "CURRENCY_CELLDUST", &one).await.unwrap();
    assert_eq!(redeem(&app, &user, "CURRENCY_CELLDUST", &two).await, Err(409));
    assert_eq!(app.stack_quantity(&character, cell), 1);
    assert_eq!(app.stack_quantity(&character, "CURRENCY_CELLDUST"), 500 - price(&first));
}
