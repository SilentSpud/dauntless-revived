//! `ramspricing.test.ts`: the Rams pilot's prices and the Event01 wire adapter, for the pilot account
//! only.

use metagame::store::{Offer, Settings, price_in_rams, rams_wire_currency, rams_wire_offer};
use serde_json::{Value, json};
use service_kit::env::Env;

fn settings(pairs: &[(&str, &str)]) -> Settings {
    Settings::from_env(&Env::from_pairs(pairs.iter().copied()))
}

fn offer(value: Value) -> Offer {
    value.as_object().unwrap().clone()
}

fn item_offer(ids: &[&str]) -> Offer {
    let items: Vec<Value> = ids.iter().map(|id| json!({"catalogId": id, "quantity": 1})).collect();
    offer(
        json!({"id": "test", "tags": ["webstore"], "platinumPrice": 0, "remaining": 1, "entitlements": [], "items": items}),
    )
}

#[test]
fn pilot_price_tiers_bundle_cap_and_account_isolation() {
    let s = settings(&[("STORE_RAMS_TEST_ACCOUNT", "pilot")]);
    let cases: [(&[&str], i64); 8] = [
        (&["QI_FLARE_TEST"], 250),
        (&["BNC_SIGIL_TEST"], 250),
        (&["EM_TEST"], 750),
        (&["WP_TEST"], 2500),
        (&["DYE_TEST"], 5000),
        (&["AR_HEAD", "AR_BODY", "AR_ARMS", "AR_LEGS"], 5000),
        (&["WP_TEST", "EM_TEST"], 3250),
        (&["WP_TEST", "DYE_TEST"], 5000),
    ];
    for (ids, price) in cases {
        let original = item_offer(ids);
        assert_eq!(
            price_in_rams(&s, original.clone(), "pilot")["notesPrice"],
            price,
            "{ids:?}"
        );
        assert_eq!(price_in_rams(&s, original.clone(), "other"), original);
        assert_eq!(original["platinumPrice"], 0);
    }
    let unsupported = item_offer(&["CURRENCY_NOTES"]);
    assert_eq!(price_in_rams(&s, unsupported.clone(), "pilot"), unsupported);
    let mut sheen = item_offer(&[]);
    sheen.insert("tags".into(), json!(["webstore", "dye_armour"]));
    sheen.insert("entitlements".into(), json!([{"name": "ent_dye_sheen_glossy"}]));
    assert_eq!(price_in_rams(&s, sheen.clone(), "pilot")["notesPrice"], 5000);
    assert_eq!(price_in_rams(&s, sheen.clone(), "other"), sheen);
}

#[test]
fn native_pilot_transport_preserves_canonical_rams_accounting_and_isolates_other_accounts() {
    let s = settings(&[
        ("STORE_RAMS_TEST_ACCOUNT", "pilot"),
        ("STORE_RAMS_EVENT_TRANSPORT", "1"),
    ]);
    let original = offer(
        json!({"id": "test", "tags": ["webstore"], "notesPrice": 5000, "remaining": 1, "entitlements": [], "items": []}),
    );
    let wire = rams_wire_offer(&s, original.clone(), "pilot");
    assert_eq!(wire["event01Price"], 5000);
    assert!(!wire.contains_key("notesPrice"));
    assert!(wire["tags"].as_array().unwrap().contains(&json!("rams-store-pilot")));
    assert_eq!(original["notesPrice"], 5000);
    assert!(!original.contains_key("event01Price"));
    assert_eq!(rams_wire_offer(&s, original.clone(), "other"), original);
    assert_eq!(rams_wire_currency(&s, "event01", "pilot"), "notes");
    assert_eq!(rams_wire_currency(&s, "event_01", "pilot"), "notes");
    assert_eq!(rams_wire_currency(&s, "event_01", "other"), "event_01");
    assert_eq!(rams_wire_currency(&s, "event01", "other"), "event01");
    assert_eq!(rams_wire_currency(&s, "platinum", "pilot"), "platinum");
    let mut vendor = original.clone();
    vendor.shift_remove("notesPrice");
    vendor.insert("event01Price".into(), json!(5));
    assert_eq!(rams_wire_offer(&s, vendor.clone(), "pilot"), vendor);
}

#[test]
fn wire_adapter_remains_disabled_without_explicit_native_pilot_switch() {
    let s = settings(&[("STORE_RAMS_TEST_ACCOUNT", "pilot")]);
    let original = offer(
        json!({"id": "test", "tags": ["webstore"], "notesPrice": 250, "remaining": 1, "entitlements": [], "items": []}),
    );
    assert_eq!(rams_wire_offer(&s, original.clone(), "pilot"), original);
    assert_eq!(rams_wire_currency(&s, "event01", "pilot"), "event01");
}
