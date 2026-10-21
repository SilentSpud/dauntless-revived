//! `store-maintenance.test.ts`, the curated shop: STORE_CATALOG_PROFILE=curated30 offers exactly 30
//! free cosmetics and refuses the others, through the store routes. The revocation test is ported as
//! `tests/store.rs`'s `revoking_free_purchases_previews_applies_and_verifies`.

use crate::common::{TestApp, req};

#[tokio::test(flavor = "multi_thread")]
async fn curated_shop_has_exactly_30_free_cosmetics_and_blocks_excluded_direct_purchases() {
    let app = TestApp::with_env(&[
        ("STORE", "free"),
        ("TRIALS_STORE", "0"),
        ("MIDDLEMAN_STORE", "0"),
        ("STORE_CATALOG_PROFILE", "curated30"),
        ("STORE_REPEATABLE_TOKENS", "1"),
    ])
    .await;
    let (user, _) = app.make_player().await;
    let reply = app
        .get("/product/skus/public?requiredTags=webstore", req().bearer(&user))
        .await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    let offers = reply.json.as_array().unwrap();
    assert_eq!(offers.len(), 30);
    let ids: std::collections::HashSet<&str> = offers.iter().map(|o| o["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 30);
    assert!(ids.contains("bundle_dyes_palette_01"));
    for offer in offers {
        let id = offer["id"].as_str().unwrap();
        assert!(!id.starts_with("single_dye_sheen_"), "{id}");
        let sheen = offer["entitlements"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|e| e["name"].as_str().unwrap_or("").starts_with("ent_dye_sheen_"));
        assert!(!sheen, "{id}");
        assert_eq!(offer["platinumPrice"], 0, "{id}");
        let token = offer["items"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|i| i["catalogId"].as_str().unwrap_or("").starts_with("TOKEN_"));
        assert!(!token, "{id}");
    }
    let single = app
        .get("/product/sku/bundle_currency_bounty_small", req().bearer(&user))
        .await;
    assert!(single.status >= 400, "{} {}", single.status, single.text);
    let token = app
        .get("/token/platinum/single_weapon_kats_claw", req().bearer(&user))
        .await;
    assert!(token.status >= 400, "{} {}", token.status, token.text);
}
