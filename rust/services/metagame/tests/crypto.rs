#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The Rust cryptography against values the Node metagame made (fixtures/node-crypto.json, written by
//! fixtures/make-node-crypto.cjs with the Node metagame's own libraries): M20, M21, M22.

use metagame::crypto::{
    CryptoError, TokenClaims, TokenKeys, hashes_equal, open_recovery, random_below, random_hex, random_uuid,
    seal_recovery, sha256_hex,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap()
}

fn keys(fixture: &Value) -> TokenKeys {
    TokenKeys::from_base64_pem(
        fixture["privB64"].as_str().unwrap(),
        fixture["pubB64"].as_str().unwrap(),
    )
    .unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn tokens_node_issued_verify() {
    let fixture = fixture();
    let keys = keys(&fixture);
    let token = |name: &str| fixture["tokens"][name].as_str().unwrap().to_owned();
    assert_eq!(
        keys.verify(&token("valid")),
        Ok(TokenClaims {
            user_id: "UID-node".into(),
            epoch: Some(3)
        })
    );
    assert_eq!(
        keys.verify(&token("noEpoch")),
        Ok(TokenClaims {
            user_id: "UID-old".into(),
            epoch: None
        })
    );
}

#[test]
fn forged_and_stale_tokens_are_refused() {
    let fixture = fixture();
    let keys = keys(&fixture);
    for name in [
        "expired",
        "wrongIssuer",
        "wrongAudience",
        "algNone",
        "hs256WithPublicKey",
        "badSignature",
    ] {
        let result = keys.verify(fixture["tokens"][name].as_str().unwrap());
        assert!(
            matches!(result, Err(CryptoError::InvalidToken(_))),
            "{name}: {result:?}"
        );
    }
    assert!(keys.verify("not a token").is_err());
}

#[test]
fn tokens_rust_issues_verify_and_carry_the_epoch() {
    let fixture = fixture();
    let keys = keys(&fixture);
    let token = keys.sign("UID-rust", 7, now()).unwrap();
    assert_eq!(
        keys.verify(&token),
        Ok(TokenClaims {
            user_id: "UID-rust".into(),
            epoch: Some(7)
        })
    );
    let stale = keys.sign("UID-rust", 7, now() - 24 * 60 * 60 - 1).unwrap();
    assert!(keys.verify(&stale).is_err(), "24 hours and no leeway");
}

#[test]
fn recovery_blobs_node_wrote_decrypt() {
    let fixture = fixture();
    let secret = fixture["privB64"].as_str().unwrap();
    let recovery = &fixture["recovery"];
    let user = recovery["userId"].as_str().unwrap();
    let blob = recovery["blob"].as_str().unwrap();
    assert_eq!(
        open_recovery(secret, user, blob).unwrap(),
        recovery["accountKey"].as_str().unwrap()
    );
    assert_eq!(
        open_recovery(secret, "UID-other", blob),
        Err(CryptoError::Recovery),
        "bound to the user id"
    );
    assert_eq!(open_recovery("another key", user, blob), Err(CryptoError::Recovery));
}

#[test]
fn recovery_blobs_round_trip_with_a_fresh_nonce() {
    let a = seal_recovery("secret", "UID-a", "UUK_abc").unwrap();
    let b = seal_recovery("secret", "UID-a", "UUK_abc").unwrap();
    assert_ne!(a, b);
    assert_eq!(open_recovery("secret", "UID-a", &a).unwrap(), "UUK_abc");
}

#[test]
fn key_hashes_match_node() {
    let fixture = fixture();
    let recovery = &fixture["recovery"];
    let hash = sha256_hex(recovery["accountKey"].as_str().unwrap());
    assert_eq!(hash, recovery["accountKeyHash"].as_str().unwrap());
    assert!(hashes_equal(&hash, recovery["accountKeyHash"].as_str().unwrap()));
    assert!(!hashes_equal(&hash, &sha256_hex("other")));
    assert!(!hashes_equal(&hash, "short"));
}

#[test]
fn random_values_have_nodes_formats() {
    assert!(
        random_hex(24).len() == 48
            && random_hex(24)
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    let uuid = random_uuid();
    assert_eq!((uuid.len(), &uuid[14..15]), (36, "4"));
    assert_eq!(uuid, uuid.to_lowercase());
    assert!((0..1000).all(|_| random_below(31) < 31));
}
