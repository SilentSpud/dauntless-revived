//! `creator.test.ts`: the Void creator code, 10000 Rams once per account. Node's "independent backend
//! processes" are two stores opened on the same database file.

use metagame::creator::{self, RedeemFail};
use metagame::store::StoreFail;
use serde_json::{Value, json};

use crate::common::{TestApp, req};

async fn token(app: &TestApp, user: &str) -> String {
    app.token(user).await
}

/// `RedeemCreatorCode` through `store`: the reply, or the refusal's status and message.
async fn redeem_with(app: &TestApp, store: &store::Store, user: &str) -> Result<Value, (u16, String)> {
    let (ctx, user) = (app.state.store_context(), user.to_owned());
    match store.write(move |conn| creator::redeem(conn, &ctx, &user)).await {
        Ok(reply) => Ok(reply),
        Err(RedeemFail::Store(StoreFail::Refused(status, message))) => Err((status, message)),
        Err(RedeemFail::Inventory) => Err((409, "inventory".into())),
        Err(other) => panic!("{other:?}"),
    }
}

async fn redeem(app: &TestApp, user: &str) -> Result<Value, (u16, String)> {
    redeem_with(app, &app.state.store, user).await
}

async fn supported(app: &TestApp, user: &str) -> bool {
    let user = user.to_owned();
    app.db(move |conn| creator::support(conn, &user)).await["success"] == true
}

async fn second_character(app: &TestApp, user: &str, name: &str) -> String {
    let (user, name) = (user.to_owned(), name.to_owned());
    app.db(move |conn| metagame::characters::create(conn, &user, &name))
        .await
        .characterId
}

fn rams(app: &TestApp, character: &str) -> i64 {
    app.stack_quantity(character, "CURRENCY_NOTES")
}

#[tokio::test(flavor = "multi_thread")]
async fn native_post_grants_exactly_10000_rams_to_the_authenticated_account_get_never_grants() {
    let app = TestApp::start().await;
    let (a, a_character) = app.make_player().await;
    let (b, b_character) = app.make_player().await;
    for _ in 0..3 {
        let reply = app.get("/creator?slug=Void", req().bearer(&a)).await;
        assert_eq!((reply.status, reply.json["success"].clone()), (200, json!(false)));
    }
    assert_eq!(rams(&app, &a_character), 0);
    let body = json!({"slug": "Void", "authorization": token(&app, &a).await, "accountId": b, "characterId": b_character, "amount": 999_999});
    let reply = app.post("/creator", req().json(body)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        (reply.json["slug"].clone(), reply.json["grantedRams"].clone()),
        (json!("Void"), json!(10000))
    );
    assert_eq!((rams(&app, &a_character), rams(&app, &b_character)), (10000, 0));
    let status = app
        .get("/creator", req().header("x-epic-authorization", &token(&app, &a).await))
        .await;
    assert_eq!(status.json["slug"], "Void");
    assert_eq!(rams(&app, &a_character), 10000);
}

#[tokio::test(flavor = "multi_thread")]
async fn case_whitespace_variants_and_concurrent_submissions_cannot_give_a_second_reward() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let calls = [" Void ", "VOID", "void", "Void"]
        .map(|slug| app.post("/creator", req().bearer(&user).json(json!({"slug": slug}))));
    let replies = futures_util::future::join_all(calls).await;
    assert_eq!(replies.iter().filter(|r| r.status == 200).count(), 1);
    assert_eq!(replies.iter().filter(|r| r.status == 409).count(), 3);
    assert_eq!(rams(&app, &character), 10000);
}

#[tokio::test(flavor = "multi_thread")]
async fn independent_backend_processes_serialize_competing_account_claims() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    let open = || store::Store::open(&app.db, &store::Options::default(), &metagame::db::SCHEMA).unwrap();
    let (one, two) = (open(), open());
    let (first, second) = tokio::join!(redeem_with(&app, &one, &user), redeem_with(&app, &two, &user));
    let mut statuses = [
        first.map(|_| 200).unwrap_or_else(|e| e.0),
        second.map(|_| 200).unwrap_or_else(|e| e.0),
    ];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(rams(&app, &character), 10000);
    assert_eq!(
        redeem_with(&app, &open(), &user).await.unwrap_err().0,
        409,
        "a fresh process still sees the permanent receipt"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn receipt_is_account_wide_and_survives_inventory_receipt_pruning() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    redeem(&app, &user).await.unwrap();
    let second = second_character(&app, &user, "Second").await;
    app.run(
        "UPDATE characters SET updateVersion = 500 WHERE characterId = ?1",
        &[&second],
    );
    app.run("DELETE FROM inventorytransactions WHERE userId = ?1", &[&user]);
    let refused = redeem(&app, &user).await.unwrap_err();
    assert!(refused.1.contains("already been redeemed"), "{refused:?}");
    assert_eq!((rams(&app, &character), rams(&app, &second)), (10000, 0));
    assert!(supported(&app, &user).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn selects_the_active_owned_character_and_lets_different_accounts_redeem_independently() {
    let app = TestApp::start().await;
    let (a, a_character) = app.make_player().await;
    let (b, b_character) = app.make_player().await;
    let second = second_character(&app, &a, "Active").await;
    app.run(
        "UPDATE characters SET updateVersion = 500 WHERE characterId = ?1",
        &[&second],
    );
    redeem(&app, &a).await.unwrap();
    redeem(&app, &b).await.unwrap();
    assert_eq!(
        [rams(&app, &a_character), rams(&app, &second), rams(&app, &b_character)],
        [0, 10000, 10000]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_missing_unknown_structured_codes_without_consuming_the_reward() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    for slug in [
        json!(null),
        json!(""),
        json!("Other"),
        json!("Void2"),
        json!(["Void"]),
        json!({"code": "Void"}),
        json!("V".repeat(100)),
    ] {
        assert_eq!(
            app.post("/creator", req().bearer(&user).json(json!({"slug": slug})))
                .await
                .status,
            404,
            "{slug}"
        );
    }
    assert_eq!(
        app.post("/creator", req().bearer(&user).json(json!({}))).await.status,
        404,
        "missing"
    );
    assert!(!supported(&app, &user).await);
    assert_eq!(
        app.post("/creator", req().bearer(&user).json(json!({"slug": "Void"})))
            .await
            .status,
        200
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn requires_a_valid_player_token_including_native_token_fields() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    assert_eq!(
        app.post("/creator", req().json(json!({"slug": "Void", "accountId": user})))
            .await
            .status,
        401
    );
    assert_eq!(
        app.post("/creator", req().gs().json(json!({"slug": "Void"})))
            .await
            .status,
        403
    );
    assert_eq!(
        app.post(
            "/creator",
            req().json(json!({"slug": "Void", "authorization": "invalid"}))
        )
        .await
        .status,
        401
    );
    assert_eq!(
        app.get("/creator", req().header("x-epic-authorization", "invalid"))
            .await
            .status,
        401
    );
    let body = json!({"slug": "Void", "authorization": token(&app, &user).await});
    assert_eq!(
        app.post("/creator", req().json(body).header("authorization", "bearer invalid"))
            .await
            .status,
        401
    );
    assert_eq!(rams(&app, &character), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_inventory_grants_roll_back_the_account_receipt_and_can_be_retried() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    app.run(
        "INSERT INTO inventories (characterId, stackedItems, instancedItems) VALUES (?1, '{', '[]')",
        &[&character],
    );
    assert_eq!(
        app.post("/creator", req().bearer(&user).json(json!({"slug": "Void"})))
            .await
            .status,
        409
    );
    assert!(!supported(&app, &user).await);
    app.run(
        "UPDATE inventories SET stackedItems = '[]' WHERE characterId = ?1",
        &[&character],
    );
    assert_eq!(
        app.post("/creator", req().bearer(&user).json(json!({"slug": "Void"})))
            .await
            .status,
        200
    );
    assert_eq!(rams(&app, &character), 10000);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_character_does_not_burn_the_account_s_code() {
    let app = TestApp::start().await;
    let user = "UID-no-character";
    app.run(
        "INSERT INTO users (userId, name, notes) VALUES (?1, 'NoCharacter', 0)",
        &[&user],
    );
    let refused = redeem(&app, user).await.unwrap_err();
    assert!(refused.1.contains("Create a character"), "{refused:?}");
    assert!(!supported(&app, user).await);
    let character = second_character(&app, user, "NowCreated").await;
    redeem(&app, user).await.unwrap();
    assert_eq!(rams(&app, &character), 10000);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_or_overflowing_native_rams_balances_do_not_consume_a_reward() {
    let app = TestApp::start().await;
    let (user, character) = app.make_player().await;
    app.run(
        "INSERT INTO inventories (characterId, stackedItems, instancedItems) VALUES (?1, '[]', '[]')",
        &[&character],
    );
    let set = |quantity: Value| {
        let stacked = json!([{"catalogId": "CURRENCY_NOTES", "quantity": quantity}]).to_string();
        app.run(
            "UPDATE inventories SET stackedItems = ?1 WHERE characterId = ?2",
            &[&stacked, &character],
        );
    };
    for quantity in [json!(-1), json!("68000"), json!(2_147_483_647), json!(1.5)] {
        set(quantity.clone());
        assert_eq!(
            app.post("/creator", req().bearer(&user).json(json!({"slug": "Void"})))
                .await
                .status,
            409,
            "{quantity}"
        );
        assert!(!supported(&app, &user).await);
    }
    set(json!(2_147_473_647));
    redeem(&app, &user).await.unwrap();
    assert_eq!(rams(&app, &character), 2_147_483_647);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_audit_log_records_the_grant_source_and_only_one_permanent_receipt() {
    let app = TestApp::start().await;
    let (user, _) = app.make_player().await;
    redeem(&app, &user).await.unwrap();
    let log = app.rows(
        "SELECT catalogId, quantityChange, caller, source FROM inventorylog WHERE userId = ?1",
        &[&user],
    );
    assert_eq!(
        log,
        [json!({"catalogId": "CURRENCY_NOTES", "quantityChange": 10000, "caller": "store", "source": "creator:Void"})]
    );
    let job = format!("creator:void:{user}");
    assert_eq!(app.count("onetimejobs", "jobId = ?1", &[&job]), 1);
}
