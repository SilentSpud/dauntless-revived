//! `socialported.test.ts`: the two social fixes from Harmonic's fork: a repeated party accept answers
//! the party instead of 404, and GET /account/api/oauth/verify names the caller's own account.

use base64::Engine;
use serde_json::{Value, json};

use crate::common::{Reply, Req, TestApp, req};

const STUB_ACCOUNT: &str = "9626f441055349ce8cb7d7d5a483eaa2";
const JOHN: &str = "UID-soc-john";
const MANDA: &str = "UID-soc-manda";
const TODD: &str = "UID-soc-todd";

async fn social(extra: &[(&str, &str)]) -> TestApp {
    let app = TestApp::with_env(extra).await;
    app.exec(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES ('UID-soc-john', 'LookupJohn', 0, 0), \
         ('UID-soc-manda', 'LookupManda', 0, 0), ('UID-soc-todd', 'LookupTodd', 0, 0)",
    );
    app
}

fn build(extra: Value) -> Value {
    let mut body = json!({"buildId": "239827", "featureOverrides": []});
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    body
}

async fn poll(app: &TestApp, who: &str) -> Reply {
    app.post("/party", req().bearer(who).json(json!({}))).await
}

async fn invite(app: &TestApp, from: &str, to: &str, party: &Value) -> u16 {
    let body = build(json!({"recipientPlayerId": to, "partyId": party}));
    app.call("PUT", "/party/invite", req().bearer(from).json(body))
        .await
        .status
}

async fn accept(app: &TestApp, who: &str, id: &str) -> Reply {
    let body = build(json!({"recipientPlayerId": who, "partyId": id}));
    app.call(
        "PUT",
        &format!("/party/invite/accept/{id}"),
        req().bearer(who).json(body),
    )
    .await
}

/// A token signed with the app's key (or `secret`, an HMAC key), `expires_in` seconds from now.
fn sign_with(user: &str, expires_in: i64, secret: Option<&str>) -> String {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    let now = metagame::time::now_ms() / 1000;
    let claims = json!({"userId": user, "iat": now, "exp": now + expires_in, "iss": "undaunted-metagame", "aud": "undaunted-metagame"});
    match secret {
        Some(secret) => encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap(),
        None => {
            let keys: Value = serde_json::from_str(include_str!("../fixtures/node-crypto.json")).unwrap();
            let pem = base64::engine::general_purpose::STANDARD
                .decode(keys["privB64"].as_str().unwrap())
                .unwrap();
            encode(
                &Header::new(Algorithm::RS256),
                &claims,
                &EncodingKey::from_rsa_pem(&pem).unwrap(),
            )
            .unwrap()
        }
    }
}

async fn verify(app: &TestApp, request: Req) -> Reply {
    app.get("/account/api/oauth/verify", request).await
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_200_with_the_party_the_caller_already_joined_named_by_its_party_id_or_by_another_member() {
    let app = social(&[]).await;
    let party = poll(&app, JOHN).await.json["partyId"].clone();
    let id = party.as_str().unwrap();
    assert_eq!(invite(&app, JOHN, MANDA, &party).await, 200);
    let first = accept(&app, MANDA, id).await;
    assert_eq!(first.status, 200);
    assert_eq!(first.json["playerIds"], json!([JOHN, MANDA]));
    let again = accept(&app, MANDA, id).await;
    assert_eq!(
        again.status, 200,
        "the fork's retry case: the invite is used up, the party is the answer"
    );
    assert_eq!(again.json, first.json);
    assert_eq!(again.json, poll(&app, MANDA).await.json, "the same as the poll");
    assert_eq!(
        accept(&app, MANDA, JOHN).await.json["partyId"],
        party,
        "named by the sender, a member"
    );
    assert_eq!(
        accept(&app, MANDA, MANDA).await.status,
        404,
        "not by the caller's own id"
    );
    assert_eq!(accept(&app, MANDA, "not-a-party").await.status, 404);
    assert_eq!(
        accept(&app, TODD, id).await.status,
        404,
        "a player who is not in it and has no invite"
    );
    assert_eq!(
        poll(&app, JOHN).await.json["playerIds"],
        json!([JOHN, MANDA]),
        "nothing changed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stays_404_for_a_party_of_one_and_for_a_party_the_caller_left() {
    let app = social(&[]).await;
    let party = poll(&app, JOHN).await.json["partyId"].clone();
    let todds = poll(&app, TODD).await.json["partyId"].as_str().unwrap().to_owned();
    assert_eq!(accept(&app, TODD, &todds).await.status, 404, "alone in a party of one");
    assert_eq!(invite(&app, JOHN, MANDA, &party).await, 200);
    assert_eq!(accept(&app, MANDA, party.as_str().unwrap()).await.status, 200);
    assert_eq!(
        app.call("DELETE", "/party/member", req().bearer(MANDA)).await.status,
        200
    );
    assert_eq!(
        accept(&app, MANDA, party.as_str().unwrap()).await.status,
        404,
        "Manda left the party"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn names_the_caller_s_account_the_lookups_find_the_other_player_and_the_mapping_maps_them() {
    let app = social(&[]).await;
    let named = app
        .get(
            "/account/api/public/account/displayName/lookupmanda",
            req().bearer(JOHN),
        )
        .await;
    assert_eq!(named.json["id"], MANDA);
    let by_id = app
        .get(&format!("/account/api/public/account/{MANDA}"), req().bearer(JOHN))
        .await;
    assert_eq!(by_id.json["displayName"], "LookupManda");
    let verified = verify(&app, req().bearer(JOHN)).await;
    assert_eq!(verified.status, 200);
    assert_eq!(
        json!([
            verified.json["account_id"],
            verified.json["active"],
            verified.json["expires_at"]
        ]),
        json!([JOHN, true, "2085-09-09T01:01:01.703Z"]),
        "still far off"
    );
    let mapping = app
        .post(
            "/account/mapping",
            req()
                .bearer(JOHN)
                .json(json!({"srcAccountType": "epic", "ids": [MANDA]})),
        )
        .await;
    assert_eq!(
        mapping.json["accountMappings"],
        json!({MANDA: {"accountId": MANDA, "accountType": "phoenix"}})
    );
    let public = app
        .post(
            "/accountinfo/public",
            req().bearer(JOHN).json(json!({"accountId": MANDA})),
        )
        .await;
    assert_eq!(public.json["accountId"], MANDA);
    assert_eq!(public.json["linkedAccounts"][0]["accountId"], MANDA);
}

#[tokio::test(flavor = "multi_thread")]
async fn never_answers_401_no_token_a_bad_one_and_an_expired_one_get_the_old_stub_with_200() {
    let app = social(&[]).await;
    let cases = [
        ("no token", req()),
        ("not a token", req().header("authorization", "bearer not-a-token")),
        (
            "an expired token",
            req().header("authorization", &format!("bearer {}", sign_with(JOHN, -60, None))),
        ),
        (
            "another key's token",
            req().header(
                "authorization",
                &format!("bearer {}", sign_with(JOHN, 60, Some("not-the-key"))),
            ),
        ),
    ];
    for (label, request) in cases {
        let reply = verify(&app, request).await;
        assert_eq!(reply.status, 200, "{label}");
        assert_eq!(
            (reply.json["account_id"].clone(), reply.json["active"].clone()),
            (json!(STUB_ACCOUNT), json!(true)),
            "{label}"
        );
    }
    let valid = req().header("authorization", &format!("bearer {}", sign_with(MANDA, 60, None)));
    assert_eq!(verify(&app, valid).await.json["account_id"], MANDA, "a valid one");
}

#[tokio::test(flavor = "multi_thread")]
async fn verify_stub_account_1_answers_the_stub_to_everyone_again() {
    let stub = social(&[("VERIFY_STUB_ACCOUNT", "1")]).await;
    assert_eq!(verify(&stub, req().bearer(JOHN)).await.json["account_id"], STUB_ACCOUNT);
    let app = social(&[]).await;
    assert_eq!(verify(&app, req().bearer(JOHN)).await.json["account_id"], JOHN);
}
