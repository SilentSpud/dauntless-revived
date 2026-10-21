//! `novaunlocker.test.ts`: the payload shapes of Nova Unlocker 1.0.3 (transcribed, never run) are all
//! refused, whatever authority they claim, and leave the inventory as it was. The Node file makes one
//! test per shape from a template name; here one test walks the same table.

use serde_json::{Value, json};

use crate::common::{TestApp, req};

fn stack(catalog: &str, quantity: i64) -> Value {
    json!({"catalogId": catalog, "quantity": quantity})
}

fn gear() -> Value {
    json!({"catalogId": "WP_SWORD_TEST", "instanceId": "owned-weapon", "updateVersion": 1,
        "itemData": json!({"CurrentLevel": 0, "CurrentXP": 0}).to_string()})
}

fn cases() -> Vec<(&'static str, Value)> {
    let mut invented = gear();
    invented["instanceId"] = json!("invented-weapon");
    let mut maxed = gear();
    maxed["updateVersion"] = json!(2);
    maxed["itemData"] = json!(json!({"CurrentLevel": 15, "CurrentXP": 999_999}).to_string());
    vec![
        (
            "Rams grant",
            json!({"addStackedItems": [stack("CURRENCY_NOTES", 1_000_000)]}),
        ),
        (
            "Rams removal",
            json!({"removeStackedItems": [stack("CURRENCY_NOTES", 1)]}),
        ),
        (
            "platinum",
            json!({"addStackedItems": [stack("CURRENCY_PLATINUM", 1_000_000)]}),
        ),
        (
            "universal platinum",
            json!({"addStackedItems": [stack("CURRENCY_PLATINUM_UNIV", 1_000_000)]}),
        ),
        (
            "dyes and cosmetics",
            json!({"addStackedItems": [stack("DYE_BLACK", 1)]}),
        ),
        (
            "instanced cosmetics",
            json!({"addInstancedItems": [{"catalogId": "AC_HEAD_HALO", "instanceId": "invented-halo", "updateVersion": 0}]}),
        ),
        (
            "consumable curiosities",
            json!({"addStackedItems": [stack("TY_TEST", 1_000_000)]}),
        ),
        ("cells", json!({"addStackedItems": [stack("CELL_POWER_TEST", 5)]})),
        ("weapons and parts", json!({"addInstancedItems": [invented]})),
        (
            "tonics",
            json!({"addStackedItems": [stack("QI_ATTACK_SPEED_POTION", 1_000_000)]}),
        ),
        (
            "event tonics",
            json!({"addStackedItems": [stack("QI_11A_CRIT_POTION", 30)]}),
        ),
        ("owned equipment max-level save", json!({"saveInstancedItems": [maxed]})),
        ("remove owned equipment", json!({"removeInstancedItems": [gear()]})),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn nova_name_is_refused_including_forged_authority_and_changed_retry_ids() {
    let app = TestApp::start().await;
    for (name, delta) in cases() {
        let (user, character) = app.make_player().await;
        let seed = json!({"accountId": user, "characterId": character, "transactionId": metagame::crypto::random_uuid(),
            "addInstancedItems": [gear()], "addStackedItems": [stack("CURRENCY_NOTES", 10)]});
        assert_eq!(
            app.post("/inventory", req().gs().json(seed)).await.status,
            200,
            "{name}"
        );
        let path = format!("/inventory/{user}/{character}");
        let snapshot = app.get(&path, req().bearer(&user)).await.json;
        let bearer = format!("bearer {}", app.token(&user).await);
        let signed = || req().header("authorization", &bearer);
        let transaction = metagame::crypto::random_uuid();
        for source in [
            "local-editor:user-request",
            "nova-unlocker:platinum:user-request",
            "PERIODIC_REWARD",
            "UBountyComponent::Reward",
            "owner-authorized:reward",
        ] {
            let mut body = json!({"characterId": character, "transactionId": transaction, "source": source, "Caller": "gameserver", "IsGameserver": true});
            for (key, value) in delta.as_object().unwrap() {
                body[key] = value.clone();
            }
            let status = app.post("/inventory", signed().json(body.clone())).await.status;
            assert!([401, 403].contains(&status), "{name} {source}: {status}");
            body["transactionId"] = json!(metagame::crypto::random_uuid());
            let status = app.post("/inventory", signed().json(body)).await.status;
            assert!([401, 403].contains(&status), "{name} {source} (new id): {status}");
        }
        // The first refusal suspended the account: its token no longer reads either
        assert_eq!(app.get(&path, signed()).await.status, 401, "{name}");
        assert_eq!(app.get(&path, req().gs()).await.json, snapshot, "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nova_empty_probe_remains_harmless_and_does_not_authorize_a_subsequent_grant() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let mut body = json!({"characterId": character, "transactionId": metagame::crypto::random_uuid(), "addStackedItems": [],
        "addInstancedItems": [], "saveInstancedItems": [], "removeStackedItems": [], "removeInstancedItems": []});
    assert_eq!(
        app.post("/inventory", req().bearer(&user).json(body.clone()))
            .await
            .status,
        200
    );
    body["addStackedItems"] = json!([stack("CURRENCY_NOTES", 1_000_000)]);
    assert_eq!(app.post("/inventory", req().bearer(&user).json(body)).await.status, 403);
}
