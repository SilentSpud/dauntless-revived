//! `shoplayout.test.ts`: the Rams webstore's weekly selection.

use std::collections::HashSet;

use metagame::store::{Offer, Settings, shop_layout};
use serde_json::Value;
use service_kit::env::Env;

fn settings(pairs: &[(&str, &str)]) -> Settings {
    Settings::from_env(&Env::from_pairs(pairs.iter().copied()))
}

fn webstore() -> Vec<Offer> {
    metagame::vendor::store_catalog()["webstore"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_object().unwrap().clone())
        .collect()
}

fn at(text: &str) -> i64 {
    metagame::js::date_parse(text).unwrap()
}

fn tags(offer: &Offer) -> Vec<&str> {
    offer["tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

fn id(offer: &Offer) -> &str {
    offer["id"].as_str().unwrap()
}

fn category(offer: &Offer) -> String {
    tags(offer)
        .into_iter()
        .find(|t| {
            ["skin_", "social_", "dye_", "personality_", "supplies_"]
                .iter()
                .any(|p| t.starts_with(p))
        })
        .unwrap_or("other")
        .to_owned()
}

fn featured(offers: &[Offer]) -> Vec<String> {
    offers
        .iter()
        .filter(|o| tags(o).contains(&"feature"))
        .map(|o| id(o).to_owned())
        .collect()
}

#[test]
fn every_sufficiently_stocked_category_changes_all_three_offers_each_week() {
    let s = settings(&[
        ("STORE_RAMS", "1"),
        ("STORE_WEEKLY_ROTATION", "1"),
        ("STORE_ROTATION_SEED", "weekly"),
    ]);
    let offers = webstore();
    let a = shop_layout(&s, offers.clone(), "player", at("2026-10-17T00:00:00Z"));
    let b = shop_layout(&s, offers.clone(), "player", at("2026-10-23T12:00:00Z"));
    let categories: HashSet<String> = a.iter().map(category).collect();
    for c in categories {
        if offers.iter().filter(|o| category(o) == c).count() < 6 {
            continue;
        }
        let prior: HashSet<&str> = a.iter().filter(|o| category(o) == c).map(id).collect();
        assert!(
            b.iter().filter(|o| category(o) == c).all(|o| !prior.contains(id(o))),
            "{c}"
        );
    }
}

#[test]
fn manual_seed_refresh_changes_stock_and_retains_automatic_weekly_rotation() {
    let offers = webstore();
    let now = at("2026-10-17T04:00:00Z");
    let base = [("STORE_RAMS", "1"), ("STORE_WEEKLY_ROTATION", "1")];
    let previous = shop_layout(
        &settings(&[base[0], base[1], ("STORE_ROTATION_SEED", "")]),
        offers.clone(),
        "player",
        now,
    );
    let s = settings(&[base[0], base[1], ("STORE_ROTATION_SEED", "refresh-20261017")]);
    let fresh = shop_layout(&s, offers.clone(), "player", now);
    assert_ne!(fresh, previous);
    assert_eq!(fresh, shop_layout(&s, offers.clone(), "another-player", now));
    assert_eq!(
        fresh,
        shop_layout(&s, offers.clone(), "player", at("2026-10-18T23:59:59Z"))
    );
    assert_ne!(
        fresh,
        shop_layout(&s, offers.clone(), "player", at("2026-10-23T12:00:00Z"))
    );
    assert!(
        fresh
            .iter()
            .all(|o| offers.iter().any(|original| id(original) == id(o)))
    );
}

#[test]
fn weekly_stock_is_stable_rotates_preserves_prices_and_corrects_category_tags() {
    let s = settings(&[("STORE_RAMS_TEST_ACCOUNT", "pilot"), ("STORE_WEEKLY_ROTATION", "1")]);
    let offers = webstore();
    let day = at("2026-10-17T00:00:00Z");
    let first = shop_layout(&s, offers.clone(), "pilot", day);
    assert_eq!(featured(&first).len(), 8);
    assert_eq!(
        first,
        shop_layout(&s, offers.clone(), "pilot", at("2026-10-17T23:59:59Z"))
    );
    assert_ne!(
        featured(&first),
        featured(&shop_layout(&s, offers.clone(), "pilot", at("2026-10-23T12:00:00Z")))
    );
    assert!(first.len() < offers.len());
    for o in &first {
        let original = offers.iter().find(|x| id(x) == id(o)).unwrap();
        assert_eq!(o.get("platinumPrice"), original.get("platinumPrice"));
    }
    assert_eq!(shop_layout(&s, offers.clone(), "other", day), offers);
    for offer in &first {
        let t = tags(offer);
        if t.contains(&"social_emote") {
            assert!(!t.contains(&"personality_character"));
        }
        if t.contains(&"dye_armour") {
            assert!(!t.contains(&"personality_stylekit"));
        }
        let items = offer.get("items").and_then(Value::as_array);
        if items.is_some_and(|i| {
            !i.is_empty()
                && i.iter()
                    .all(|i| i["catalogId"].as_str().unwrap_or("").starts_with("BNC_FABRIC_"))
        }) {
            assert!(!t.contains(&"personality_standard"));
        }
    }
}
