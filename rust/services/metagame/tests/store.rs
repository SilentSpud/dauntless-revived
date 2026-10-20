#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The parts of the store the parity scripts can't record: what depends on the week (the Middleman's
//! cells, the Rams weekly shop), checked against Node's output at fixed times, and the free-store
//! revocation tool (`metagame db revoke-free-store`).

use diesel::prelude::*;
use metagame::db::SCHEMA;
use metagame::dbtools::{DbCommand, run};
use metagame::store::{self as shop, Context, Settings};
use serde_json::{Value, json};
use service_kit::env::Env;
use store::{Options, Store};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/store-weekly.json")).unwrap()
}

fn at(text: &str) -> i64 {
    metagame::js::date_parse(text).unwrap()
}

#[test]
fn weekly_middleman_cells_match_node() {
    for (date, want) in fixture()["middleman"].as_object().unwrap() {
        let got: Vec<Value> = shop::weekly_middleman_offers(at(date))
            .iter()
            .map(|o| {
                json!([
                    o["id"],
                    o["displayName"],
                    o["displayPriority"],
                    o["availableFrom"],
                    o["availableTo"]
                ])
            })
            .collect();
        assert_eq!(&Value::Array(got), want, "{date}");
    }
}

#[test]
fn weekly_shop_layout_matches_node() {
    let settings = Settings::from_env(&Env::from_pairs([
        ("STORE_RAMS", "1"),
        ("STORE_WEEKLY_ROTATION", "1"),
        ("STORE_ROTATION_SEED", "seed-7"),
    ]));
    let webstore: Vec<shop::Offer> = metagame::vendor::store_catalog()["webstore"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_object().unwrap().clone())
        .collect();
    for (date, want) in fixture()["layout"].as_object().unwrap() {
        let got: Vec<Value> = shop::shop_layout(&settings, webstore.clone(), "UID-x", at(date))
            .iter()
            .map(|o| {
                let tags: Vec<&str> = o["tags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap())
                    .collect();
                json!([o["id"], tags.join(" ")])
            })
            .collect();
        assert_eq!(&Value::Array(got), want, "{date}");
    }
    let off = Settings::default();
    assert_eq!(
        shop::shop_layout(&off, webstore.clone(), "UID-x", 0).len(),
        webstore.len()
    );
}

fn context(store_free: bool) -> Context {
    let settings = Settings::from_env(&Env::from_pairs(if store_free {
        vec![("STORE", "free")]
    } else {
        vec![]
    }));
    Context {
        settings,
        entitlement_defaults: Vec::new(),
        inventory: metagame::inventory::Settings::default(),
        now: metagame::time::now_ms(),
    }
}

fn stacks(conn: &mut store::Connection, character: &str) -> String {
    metagame::schema::inventories::table
        .find(character)
        .select(metagame::schema::inventories::stackedItems)
        .first(conn)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_free_purchases_previews_applies_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("undaunted.db");
    {
        let db = Store::open(&path, &Options::default(), &SCHEMA).unwrap();
        db.write(|conn| {
            use diesel::connection::SimpleConnection;
            conn.batch_execute(
                r#"INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-a', 'Ada', 0, 0);
                INSERT INTO characters (characterId, userId, createdDate, lastModifiedDate, name, updateVersion, data)
                    VALUES ('char-a', 'UID-a', 'Sep 21, 2026', 'Sep 21, 2026', 'Ada', 0, '{}');
                INSERT INTO inventories (characterId, instancedItems, stackedItems) VALUES ('char-a', '[]', '[{"catalogId":"ORE","quantity":3}]')"#,
            )
            .map_err(store::Error::from)
        })
        .await
        .unwrap();
        // A free bundle (four stacked armour pieces) and a weapon skin (instanced), bought for real
        for sku in ["bundle_armour_monk", "single_weapon_unseen_strikers"] {
            let ctx = context(true);
            let (offer, _) = shop::lookup(&ctx, sku, "UID-a").unwrap();
            shop::check_sale(&offer, "platinum").unwrap();
            let (token, _) = db
                .write(move |conn| shop::issue_token(conn, &ctx, "UID-a", sku, &offer))
                .await
                .unwrap();
            let (currency, hash) = shop::check_redeem("platinum", Some(&json!(token))).unwrap();
            let ctx = context(true);
            let done = db
                .write(move |conn| shop::redeem(conn, &ctx, "UID-a", currency, &hash))
                .await
                .unwrap();
            assert!(!done.replayed && done.items > 0, "{sku}");
        }
        let held = db
            .read(|conn| Ok::<_, store::Error>(stacks(conn, "char-a")))
            .await
            .unwrap();
        assert!(held.contains("AR_HP07A_MONK_HELM"), "{held}");
    }

    let cutoff = "2099-01-01T00:00:00.000Z".to_owned();
    let preview = run(&DbCommand::RevokeFreeStore {
        db: path.clone(),
        cutoff: cutoff.clone(),
        apply: false,
    })
    .unwrap();
    assert_eq!(
        preview,
        r#"{"mode":"preview","cutoff":"2099-01-01T00:00:00.000Z","purchases":2,"characters":1,"stackedUnits":4,"instances":1,"entitlements":0,"alreadyProcessed":0}"#
    );

    let applied = run(&DbCommand::RevokeFreeStore {
        db: path.clone(),
        cutoff: cutoff.clone(),
        apply: true,
    })
    .unwrap();
    let lines: Vec<&str> = applied.lines().collect();
    assert_eq!(lines.len(), 3, "{applied}");
    assert_eq!(
        lines[1],
        r#"{"mode":"applied","cutoff":"2099-01-01T00:00:00.000Z","purchases":2,"characters":1,"stackedUnits":4,"instances":1,"entitlements":0,"alreadyProcessed":0,"progressionUnchanged":true}"#
    );
    assert_eq!(lines[2], r#"{"mode":"verified","remaining":0,"integrity":"ok"}"#);

    let mut conn = SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    assert_eq!(stacks(&mut conn, "char-a"), r#"[{"catalogId":"ORE","quantity":3}]"#);
    let instanced: String = metagame::schema::inventories::table
        .find("char-a")
        .select(metagame::schema::inventories::instancedItems)
        .first(&mut conn)
        .unwrap();
    assert_eq!(instanced, "[]");

    // Run again: everything is already processed
    let again = run(&DbCommand::RevokeFreeStore {
        db: path.clone(),
        cutoff,
        apply: false,
    })
    .unwrap();
    assert!(
        again.contains(r#""purchases":0"#) && again.contains(r#""alreadyProcessed":2"#),
        "{again}"
    );
    assert!(
        run(&DbCommand::RevokeFreeStore {
            db: path,
            cutoff: "not a date".into(),
            apply: false,
        })
        .is_err()
    );
}
