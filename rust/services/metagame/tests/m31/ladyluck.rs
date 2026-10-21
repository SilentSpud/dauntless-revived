//! `ladyluck.test.ts`: the Lady Luck Trials store, with STORE off, TRIALS_STORE on and MIDDLEMAN_STORE
//! off. Node's `StoreImageRateLimit.resetKey` is not needed: each test app has its own limiters.

use metagame::store::{GrantKind, StoreFail, grant_kind};
use serde_json::Value;

use crate::common::{TestApp, req};
use crate::storekit::{app_with, buy, create, credit, instanced, offer, offers, redeem};

async fn trials() -> TestApp {
    app_with(true, false, &[]).await
}

fn held(app: &TestApp, character: &str, catalog: &str) -> Vec<Value> {
    instanced(app, character)
        .into_iter()
        .filter(|i| i["catalogId"] == catalog)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_the_1_4_4_flat_price_shape_and_lists_champion_gear_without_a_leaderboard_placement() {
    let app = trials().await;
    let (user, _) = app.make_player().await;
    let listed = offers(&app, &user, "ladyluckstore").await;
    assert_eq!(listed.len(), 58);
    assert!(listed.iter().all(|o| !o.contains_key("prices")));
    assert!(
        listed
            .iter()
            .all(|o| o.get("steelMarksPrice").is_some_and(Value::is_i64)
                != o.get("gildedMarksPrice").is_some_and(Value::is_i64))
    );
    assert_eq!(
        offer(&app, &user, "ladyluck_weapon_strikers_normal").await.unwrap()["gildedMarksPrice"],
        500
    );
    assert_eq!(
        offer(&app, &user, "ladyluck_cb_passive_trials_02").await.unwrap()["steelMarksPrice"],
        250
    );
    assert_eq!(
        offer(&app, &user, "ladyluck_weapon_strikers_prestige").await.unwrap()["gildedMarksPrice"],
        1000
    );
    assert_eq!(
        create(&app, &user, "marksgilded", "ladyluck_weapon_strikers_prestige").await,
        Err(409)
    );
}

#[test]
fn uses_captured_instanced_vs_stacked_grant_kinds_instead_of_guessing_from_prefixes() {
    assert_eq!(grant_kind("WP_AC_TRIALS_00"), Some(GrantKind::Instanced));
    assert_eq!(grant_kind("AR_TRIALS_CHEST_00"), Some(GrantKind::Instanced));
    assert_eq!(grant_kind("CELL_TRIALS_05_E"), Some(GrantKind::Stacked));
    assert_eq!(grant_kind("QI_LANTERN_POTION"), Some(GrantKind::Stacked));
}

#[tokio::test(flavor = "multi_thread")]
async fn charges_gilded_marks_once_grants_an_instanced_cosmetic_once_and_then_marks_it_owned() {
    let app = trials().await;
    let (user, character) = app.make_player().await;
    credit(&app, &character, "CURRENCY_MARKS_GILDED", 700);
    let token = create(&app, &user, "CURRENCY_MARKS_GILDED", "ladyluck_weapon_strikers_normal")
        .await
        .unwrap();
    assert_eq!(redeem(&app, &user, "CURRENCY_MARKS_GILDED", &token).await, Ok(false));
    assert_eq!(redeem(&app, &user, "CURRENCY_MARKS_GILDED", &token).await, Ok(true));
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_GILDED"), 200);
    assert_eq!(held(&app, &character, "WP_AC_TRIALS_00").len(), 1);
    assert_eq!(
        offer(&app, &user, "ladyluck_weapon_strikers_normal").await.unwrap()["remaining"],
        0
    );
    assert_eq!(
        create(&app, &user, "marksgilded", "ladyluck_weapon_strikers_normal").await,
        Err(409)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_unlimited_cell_offer_repeatable_and_charges_each_purchase_atomically() {
    let app = trials().await;
    let (user, character) = app.make_player().await;
    credit(&app, &character, "CURRENCY_MARKS_STEEL", 300);
    buy(&app, &user, "markssteel", "trials_cell_discipline_e").await;
    buy(&app, &user, "steelmarks", "trials_cell_discipline_e").await;
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), 0);
    assert_eq!(app.stack_quantity(&character, "CELL_TRIALS_05_E"), 2);
    assert_eq!(
        offer(&app, &user, "trials_cell_discipline_e").await.unwrap()["remaining"],
        1
    );
    assert_eq!(
        create(&app, &user, "markssteel", "trials_cell_discipline_e").await,
        Err(409)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sells_the_twin_suns_the_weapon_specials_every_trials_mod_and_the_six_3_trials_cells_for_steel_marks() {
    let app = trials().await;
    let (user, character) = app.make_player().await;
    let steel: &[(&str, i64)] = &[
        ("ladyluck_weapon_twin_suns", 1000),
        ("ladyluck_eb_special_parry", 500),
        ("ladyluck_ih_special_islandcracker", 500),
        ("ladyluck_ms_special_rocketlunge", 500),
        ("ladyluck_ac_special_mastery", 500),
        ("ladyluck_ga_special_skillshot", 500),
        ("ladyluck_cb_passive_trials_01", 250),
        ("ladyluck_cb_passive_trials_02", 250),
        ("ladyluck_dp_passive_trials_01", 250),
        ("ladyluck_dp_passive_trials_02", 250),
        ("ladyluck_eb_passive_trials_01", 250),
        ("ladyluck_eb_passive_trials_02", 250),
        ("ladyluck_ga_passive_trials_01", 250),
        ("ladyluck_ga_passive_trials_02", 250),
        ("ladyluck_ih_passive_trials_01", 250),
        ("ladyluck_ih_passive_trials_02", 250),
        ("ladyluck_ms_passive_trials_01", 250),
        ("ladyluck_ms_passive_trials_02", 250),
        ("trials_cell_berserker_e", 150),
        ("trials_cell_strategist_e", 150),
        ("trials_cell_engineer_e", 150),
        ("trials_cell_discipline_e", 150),
        ("trials_cell_sprinter_e", 150),
        ("trials_cell_mender_e", 150),
    ];
    credit(
        &app,
        &character,
        "CURRENCY_MARKS_STEEL",
        steel.iter().map(|(_, p)| p).sum(),
    );
    for (sku, price) in steel {
        assert_eq!(
            offer(&app, &user, sku).await.unwrap()["steelMarksPrice"],
            *price,
            "{sku}"
        );
        buy(&app, &user, "markssteel", sku).await;
    }
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), 0);
    assert_eq!(held(&app, &character, "PART_GA_SPECIAL_SKILLSHOT").len(), 1);
    assert_eq!(app.stack_quantity(&character, "CELL_TRIALS_01_E"), 1);
    assert_eq!(grant_kind("WP_DP_EXOTIC_BOMBS"), Some(GrantKind::Instanced));
    assert_eq!(
        create(&app, &user, "markssteel", "ladyluck_weapon_twin_suns").await,
        Err(409)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn charges_steel_marks_for_one_time_gameplay_rewards() {
    let app = trials().await;
    let (user, character) = app.make_player().await;
    credit(&app, &character, "CURRENCY_MARKS_STEEL", 300);
    buy(&app, &user, "CURRENCY_MARKS_STEEL", "ladyluck_cb_passive_trials_02").await;
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), 50);
    assert_eq!(held(&app, &character, "PART_CB_PASSIVE_TRIALS_02").len(), 1);
    assert_eq!(
        offer(&app, &user, "ladyluck_cb_passive_trials_02").await.unwrap()["remaining"],
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_its_priced_routes_while_the_unrelated_free_cosmetic_store_stays_off() {
    let app = trials().await;
    let (user, _) = app.make_player().await;
    let listed = app
        .get("/product/skus/public?requiredTags=ladyluckstore", req().bearer(&user))
        .await;
    assert_eq!(listed.status, 200);
    let listed = listed.json.as_array().unwrap().clone();
    assert_eq!(listed.len(), 58);
    // Every offer points at its 1.4.4 icon on this server, and the icon is served without a login
    let icon = regex::Regex::new(r"^http://[^/]+(/store-images/[a-z0-9_]+\.png)$").unwrap();
    for offer in &listed {
        assert!(
            icon.is_match(offer["images"]["standard"].as_str().unwrap()),
            "{}",
            offer["id"]
        );
        assert_eq!(offer["images"]["feature"], offer["images"]["standard"]);
    }
    let twin = listed.iter().find(|o| o["id"] == "ladyluck_weapon_twin_suns").unwrap();
    let path = icon.captures(twin["images"]["standard"].as_str().unwrap()).unwrap()[1].to_owned();
    let image = app.get(&path, req()).await;
    assert_eq!(image.status, 200);
    assert_eq!(image.headers["content-type"], "image/png");
    assert_eq!(app.get("/store-images/..%2Fpackage.json", req()).await.status, 404);
    assert_eq!(app.get("/store-images/not_an_item.png", req()).await.status, 404);

    let single = app
        .get("/product/sku/ladyluck_cb_passive_trials_02", req().bearer(&user))
        .await;
    assert_eq!(
        (single.status, single.json["steelMarksPrice"].clone()),
        (200, 250.into())
    );
    assert_eq!(
        app.get("/product/skus/public?requiredTags=webstore", req().bearer(&user))
            .await
            .status,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn is_hidden_behind_trials_store_without_exposing_single_offer_purchases() {
    let app = app_with(false, false, &[]).await;
    let (user, _) = app.make_player().await;
    assert!(offers(&app, &user, "ladyluckstore").await.is_empty());
    assert!(matches!(
        offer(&app, &user, "ladyluck_weapon_strikers_normal").await,
        Err(StoreFail::Refused(404, _))
    ));
    assert_eq!(
        create(&app, &user, "marksgilded", "ladyluck_weapon_strikers_normal").await,
        Err(404)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn store_images_reject_encoded_path_escapes_and_enforce_their_independent_request_budget() {
    let app = trials().await;
    for value in [
        "..%2Fpackage.json",
        "..%5Cpackage.json",
        "%252e%252e%252fpackage.json",
        "C%3A%5CWindows%5Cwin.ini",
    ] {
        assert_eq!(
            app.get(&format!("/store-images/{value}"), req()).await.status,
            404,
            "{value}"
        );
    }
    let fresh = trials().await;
    for _ in 0..600 {
        assert_eq!(fresh.get("/store-images/not_an_item.png", req()).await.status, 404);
    }
    assert_eq!(fresh.get("/store-images/not_an_item.png", req()).await.status, 429);
}

#[tokio::test(flavor = "multi_thread")]
async fn grants_both_reported_trials_unlocks_as_equipable_instances_and_never_stacks_or_duplicates_them() {
    let app = trials().await;
    let (user, character) = app.make_player().await;
    credit(&app, &character, "CURRENCY_MARKS_STEEL", 750);
    for (sku, id) in [
        ("ladyluck_eb_special_parry", "PART_EB_SPECIAL_PARRY"),
        ("ladyluck_ms_passive_trials_02", "PART_MS_PASSIVE_TRIALS_02"),
    ] {
        assert_eq!(grant_kind(id), Some(GrantKind::Instanced));
        let token = create(&app, &user, "markssteel", sku).await.unwrap();
        redeem(&app, &user, "markssteel", &token).await.unwrap();
        redeem(&app, &user, "markssteel", &token).await.unwrap();
        let instances = held(&app, &character, id);
        assert_eq!(instances.len(), 1);
        assert!(instances[0]["instanceId"].as_str().is_some_and(|i| !i.is_empty()));
        assert_eq!(app.stack_quantity(&character, id), 0);
        assert_eq!(offer(&app, &user, sku).await.unwrap()["remaining"], 0);
    }
    assert_eq!(app.stack_quantity(&character, "CURRENCY_MARKS_STEEL"), 0);
}
