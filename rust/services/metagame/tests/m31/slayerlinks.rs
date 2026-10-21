//! `slayerlinks.test.ts`: Slayer Links over HTTP with the bodies and paths the 1.4.4 client sends. Node's
//! `SetSlayerLinkClockForTests` cases call the controller with the time they want, as the Rust
//! functions take `now`.

use metagame::slayerlinks::{self, INVITE_EXPIRY_HOURS, INVITE_WINDOW_MS, LINK_DURATION_HOURS, MAX_INVITES_PER_WINDOW};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{Reply, TestApp, req};

const HOUR: i64 = 60 * 60 * 1000;
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn player(app: &TestApp, name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    let user = format!("UID-link-{n}-{name}");
    let shown = format!("{name}{n}");
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, 0)",
        &[&user, &shown],
    );
    user
}

/// Friends the way the client makes them: a request, then the other side's request accepts it
async fn befriend(app: &TestApp, a: &str, b: &str) {
    assert_eq!(
        app.post(&format!("/friends/api/public/friends/{a}/{b}"), req().bearer(a))
            .await
            .status,
        204
    );
    assert_eq!(
        app.post(&format!("/friends/api/public/friends/{b}/{a}"), req().bearer(b))
            .await
            .status,
        204
    );
}

async fn invite(app: &TestApp, from: &str, to: &str, slot: Value) -> Reply {
    let body = json!({"account_id": to, "slot": slot, "action_source": "social_panel"});
    app.call("PUT", "/slayerlink/invite", req().bearer(from).json(body))
        .await
}

async fn answer(app: &TestApp, by: &str, other: &str, action: &str, slot: i64) -> Reply {
    let body = json!({"account_id": other, "action": action, "slot": slot, "action_source": "social_panel"});
    app.post("/slayerlink/invite", req().bearer(by).json(body)).await
}

async fn invites(app: &TestApp, of: &str) -> Vec<Value> {
    app.get("/slayerlink/invites", req().bearer(of)).await.json["payload"]["invites"]
        .as_array()
        .unwrap()
        .clone()
}

async fn links(app: &TestApp, of: &str) -> Vec<Value> {
    app.get("/slayerlink/links", req().bearer(of)).await.json["payload"]["links"]
        .as_array()
        .unwrap()
        .clone()
}

async fn available(app: &TestApp, of: &str, ids: &[&str]) -> Value {
    app.post(
        "/slayerlink/availability",
        req().bearer(of).json(json!({"account_ids": ids})),
    )
    .await
    .json["payload"]["availability"]
        .clone()
}

async fn remove_link(app: &TestApp, by: &str, other: &str, slot: i64) -> Reply {
    let body = json!({"account_id": other, "slot": slot, "delete_pair": true});
    app.call("DELETE", "/slayerlink/links", req().bearer(by).json(body))
        .await
}

fn ok(reply: &Reply, message: &str) -> Value {
    assert_eq!(reply.status, 200, "{message} {}", reply.text);
    assert_eq!(
        (reply.json["code"].clone(), reply.json["message"].clone()),
        (Value::Null, json!("OK"))
    );
    reply.json["payload"].clone()
}

fn refused(reply: &Reply, status: u16, message: &str) {
    refused_json(reply.status, &reply.json, status, message);
}

fn refused_json(got: u16, json: &Value, status: u16, message: &str) {
    assert_eq!(got, status, "{message} {json}");
    assert_eq!(
        json.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["code", "message", "payload"]
    );
    assert_eq!(json["code"], status.to_string());
    assert!(json["message"].is_string());
    assert_eq!(json["payload"], Value::Null);
}

fn near(text: &Value, expected: i64) -> bool {
    (metagame::js::date_parse(text.as_str().unwrap()).unwrap() - expected).abs() < 60 * 1000
}

fn without(entry: &Value, key: &str) -> Value {
    let mut copy = entry.clone();
    copy.as_object_mut().unwrap().shift_remove(key);
    copy
}

fn keys(entry: &Value) -> Vec<&str> {
    entry.as_object().unwrap().keys().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_friend_is_invited_accepts_both_see_the_link_outsiders_cannot_touch_it_and_it_is_removed_for_both() {
    let app = TestApp::start().await;
    let (john, manda, todd) = (player(&app, "John"), player(&app, "Manda"), player(&app, "Todd"));
    refused(&invite(&app, &john, &manda, json!(1)).await, 403, "not friends yet");
    assert_eq!(
        available(&app, &john, &[&manda]).await,
        json!([{"account_id": manda, "available": false}])
    );

    befriend(&app, &john, &manda).await;
    assert_eq!(
        available(&app, &john, &[&manda, &todd, &john]).await,
        json!([{"account_id": manda, "available": true}, {"account_id": todd, "available": false}, {"account_id": john, "available": false}])
    );

    let id = ok(&invite(&app, &john, &manda, json!(1)).await, "")["link_id"].clone();
    assert!(
        regex::Regex::new("^[0-9a-f-]{36}$")
            .unwrap()
            .is_match(id.as_str().unwrap())
    );
    assert_eq!(
        ok(&invite(&app, &john, &manda, json!(1)).await, "")["link_id"],
        id,
        "inviting again answers the same invite"
    );
    assert_eq!(
        available(&app, &john, &[&manda]).await,
        json!([{"account_id": manda, "available": false}]),
        "an invite is already waiting"
    );

    let received = invites(&app, &manda).await;
    assert_eq!(received.len(), 1);
    assert_eq!(
        keys(&received[0]),
        ["account_id", "slot", "direction", "status", "expires", "link_id"]
    );
    assert_eq!(
        without(&received[0], "expires"),
        json!({"account_id": john, "slot": 1, "direction": "Received", "status": "Pending", "link_id": id})
    );
    let now = metagame::time::now_ms();
    assert!(
        near(&received[0]["expires"], now + INVITE_EXPIRY_HOURS * HOUR),
        "{}",
        received[0]["expires"]
    );
    let sent: Vec<Value> = invites(&app, &john)
        .await
        .iter()
        .map(|e| json!([e["account_id"], e["direction"]]))
        .collect();
    assert_eq!(sent, [json!([manda, "Sent"])]);

    refused(
        &answer(&app, &todd, &john, "accept", 1).await,
        404,
        "an outsider has no invite from John",
    );
    let by_id = app
        .post(
            "/slayerlink/invite",
            req().bearer(&todd).json(json!({"link_id": id, "action": "accept"})),
        )
        .await;
    refused(&by_id, 404, "nor by the invite's id");

    ok(&answer(&app, &manda, &john, "accept", 3).await, "");
    assert_eq!(
        ok(&answer(&app, &manda, &john, "accept", 3).await, "")["link_id"],
        id,
        "a repeated accept answers the same link"
    );

    let (of_john, of_manda) = (links(&app, &john).await, links(&app, &manda).await);
    assert_eq!(of_john.len(), 1);
    assert_eq!(
        keys(&of_john[0]),
        [
            "account_id",
            "linked_account_id",
            "slot",
            "ends",
            "link_id",
            "prize_pool"
        ]
    );
    assert_eq!(
        without(&of_john[0], "ends"),
        json!({"account_id": manda, "linked_account_id": manda, "slot": 1, "link_id": id, "prize_pool": []})
    );
    assert_eq!(
        without(&of_manda[0], "ends"),
        json!({"account_id": john, "linked_account_id": john, "slot": 3, "link_id": id, "prize_pool": []}),
        "the slot Manda chose, the client's third"
    );
    assert!(
        near(&of_john[0]["ends"], now + LINK_DURATION_HOURS * HOUR),
        "{}",
        of_john[0]["ends"]
    );
    assert_eq!(app.count("slayerlinks", "1=1", &[]), 1);
    assert!(invites(&app, &manda).await.is_empty());

    refused(&invite(&app, &john, &manda, json!(2)).await, 409, "already linked");

    let status = ok(&app.get("/slayerlink/status_good", req().bearer(&john)).await, "");
    assert_eq!(keys(&status), ["invites", "links", "config"]);
    assert_eq!(
        status["config"],
        json!({"link_duration_hours": 168, "invite_expiry_hours": 24})
    );
    assert_eq!(status["links"], json!(of_john));
    assert_eq!(status["invites"], json!([]));

    ok(&remove_link(&app, &todd, &john, 1).await, "someone without that link");
    assert_eq!(links(&app, &john).await.len(), 1, "Todd removed nothing");
    ok(&remove_link(&app, &john, &manda, 1).await, "");
    assert!(links(&app, &john).await.is_empty());
    assert!(links(&app, &manda).await.is_empty(), "a link ends for both players");
    ok(
        &remove_link(&app, &john, &manda, 1).await,
        "removing it again is still 200",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reject_and_cancel_are_resolved_by_the_other_player_s_account_id() {
    let app = TestApp::start().await;
    let (ann, ben) = (player(&app, "Ann"), player(&app, "Ben"));
    befriend(&app, &ann, &ben).await;
    ok(&invite(&app, &ann, &ben, json!(2)).await, "");
    refused(
        &answer(&app, &ben, &ann, "cancel", 1).await,
        404,
        "only the sender cancels",
    );
    ok(&answer(&app, &ben, &ann, "reject", 1).await, "");
    ok(&answer(&app, &ben, &ann, "reject", 1).await, "a repeated reject");
    refused(
        &answer(&app, &ben, &ann, "accept", 1).await,
        409,
        "the invite was declined",
    );
    assert!(invites(&app, &ann).await.is_empty() && invites(&app, &ben).await.is_empty());

    let second = ok(&invite(&app, &ann, &ben, json!(2)).await, "")["link_id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(&answer(&app, &ann, &ben, "cancel", 1).await, "");
    assert!(invites(&app, &ben).await.is_empty());
    assert_eq!(
        app.row("SELECT status FROM slayerlinkinvites WHERE inviteId = ?1", &[&second])["status"],
        "CANCELED"
    );

    let decline = app
        .post(
            "/slayerlink/invite",
            req().bearer(&ben).json(json!({"account_id": ann, "action": "decline"})),
        )
        .await;
    refused(&decline, 400, "the client sends reject");
    let nobody = app
        .post(
            "/slayerlink/invite",
            req().bearer(&ben).json(json!({"action": "accept"})),
        )
        .await;
    refused(&nobody, 404, "no account, no invite");
    assert_eq!(app.count("slayerlinks", "1=1", &[]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_slayerlink_invites_account_id_another_player_s_id_clears_the_invites_between_the_two_the_caller_s_own_id_all_of_them()
 {
    let app = TestApp::start().await;
    let (ivy, jon, kim, lea) = (
        player(&app, "Ivy"),
        player(&app, "Jon"),
        player(&app, "Kim"),
        player(&app, "Lea"),
    );
    befriend(&app, &ivy, &jon).await;
    befriend(&app, &ivy, &kim).await;
    befriend(&app, &lea, &ivy).await;
    ok(&invite(&app, &ivy, &jon, json!(1)).await, "");
    ok(&invite(&app, &ivy, &kim, json!(2)).await, "");
    ok(&invite(&app, &lea, &ivy, json!(1)).await, "");

    ok(
        &app.call("DELETE", &format!("/slayerlink/invites/{jon}"), req().bearer(&ivy))
            .await,
        "",
    );
    let mut left: Vec<Value> = invites(&app, &ivy)
        .await
        .iter()
        .map(|e| e["account_id"].clone())
        .collect();
    left.sort_by_key(Value::to_string);
    let mut expected = vec![json!(kim), json!(lea)];
    expected.sort_by_key(Value::to_string);
    assert_eq!(left, expected);
    assert!(invites(&app, &jon).await.is_empty());

    ok(
        &app.call("DELETE", &format!("/slayerlink/invites/{ivy}"), req().bearer(&ivy))
            .await,
        "",
    );
    assert!(invites(&app, &ivy).await.is_empty() && invites(&app, &lea).await.is_empty());
    assert_eq!(
        app.rows("SELECT status FROM slayerlinkinvites WHERE senderId = ?1", &[&lea]),
        [json!({"status": "DECLINED"})]
    );
    refused(
        &app.call("DELETE", "/slayerlink/invites/not%20an%20id", req().bearer(&ivy))
            .await,
        400,
        "",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn three_slots_each_numbered_1_to_3_like_the_client_s_one_waiting_invite_per_slot_and_the_slot_and_self_checks() {
    let app = TestApp::start().await;
    let max = player(&app, "Max");
    let friends = [
        player(&app, "Fa"),
        player(&app, "Fb"),
        player(&app, "Fc"),
        player(&app, "Fd"),
    ];
    for friend in &friends {
        befriend(&app, &max, friend).await;
    }
    for bad in [
        json!(0),
        json!(4),
        json!(-1),
        json!("0"),
        json!("x"),
        json!(1.5),
        Value::Null,
    ] {
        refused(
            &invite(&app, &max, &friends[0], bad.clone()).await,
            400,
            &format!("slot {bad}"),
        );
    }
    refused(&invite(&app, &max, &max, json!(1)).await, 409, "yourself");
    refused(&invite(&app, &max, "UID-link-nobody", json!(1)).await, 404, "");

    ok(&invite(&app, &max, &friends[0], json!(1)).await, "");
    refused(
        &invite(&app, &max, &friends[1], json!(1)).await,
        409,
        "slot 1 already has an invite waiting",
    );
    refused(
        &invite(&app, &friends[0], &max, json!(2)).await,
        409,
        "the other way round while Max's invite waits",
    );

    ok(&answer(&app, &friends[0], &max, "accept", 1).await, "");
    ok(
        &invite(&app, &max, &friends[1], json!("2")).await,
        "a numeric string is a slot too",
    );
    ok(&answer(&app, &friends[1], &max, "accept", 1).await, "");
    ok(
        &invite(&app, &max, &friends[2], json!(3)).await,
        "the client's third slot",
    );
    ok(&answer(&app, &friends[2], &max, "accept", 1).await, "");

    let slots = |list: &[Value]| list.iter().map(|l| l["slot"].clone()).collect::<Vec<_>>();
    assert_eq!(slots(&links(&app, &max).await), [json!(1), json!(2), json!(3)]);
    let status = ok(&app.get("/slayerlink/status_good", req().bearer(&max)).await, "");
    assert_eq!(
        slots(status["links"].as_array().unwrap()),
        [json!(1), json!(2), json!(3)]
    );
    assert_eq!(
        available(&app, &max, &[&friends[3]]).await,
        json!([{"account_id": friends[3], "available": false}]),
        "no free slot"
    );
    refused(
        &invite(&app, &max, &friends[3], json!(1)).await,
        409,
        "the slot is linked",
    );
    refused(
        &invite(&app, &friends[3], &max, json!(1)).await,
        409,
        "Max has no free slot",
    );
    assert_eq!(
        available(&app, &friends[3], &[&max]).await,
        json!([{"account_id": max, "available": false}])
    );

    // The accepting player's chosen slot is used when it is free, otherwise the first free one; a slot
    // the client does not have (0) is never stored
    let pairs = |list: Vec<Value>| {
        list.iter()
            .map(|l| json!([l["account_id"], l["slot"]]))
            .collect::<Vec<_>>()
    };
    befriend(&app, &friends[3], &friends[0]).await;
    ok(&invite(&app, &friends[3], &friends[0], json!(1)).await, "");
    ok(&answer(&app, &friends[0], &friends[3], "accept", 1).await, "");
    assert_eq!(
        pairs(links(&app, &friends[0]).await),
        [json!([max, 1]), json!([friends[3], 2])]
    );
    befriend(&app, &friends[3], &friends[1]).await;
    ok(&invite(&app, &friends[3], &friends[1], json!(2)).await, "");
    ok(&answer(&app, &friends[1], &friends[3], "accept", 0).await, "");
    assert_eq!(
        pairs(links(&app, &friends[1]).await),
        [json!([max, 1]), json!([friends[3], 2])]
    );
    assert_eq!(
        app.count(
            "slayerlinks",
            "senderSlot NOT BETWEEN 1 AND 3 OR targetSlot NOT BETWEEN 1 AND 3",
            &[]
        ),
        0
    );
}

/// The controller at a chosen time (Node's `SetSlayerLinkClockForTests`).
struct At<'a> {
    app: &'a TestApp,
}

impl At<'_> {
    async fn invite(&self, from: &str, to: &str, slot: i64, now: i64) -> (u16, Value) {
        let (from, body) = (
            from.to_owned(),
            json!({"account_id": to, "slot": slot, "action_source": "social_panel"}),
        );
        self.app
            .db(move |conn| slayerlinks::invite(conn, &from, Some(&body), now))
            .await
    }

    async fn answer(&self, by: &str, other: &str, action: &str, now: i64) -> (u16, Value) {
        let (by, body) = (
            by.to_owned(),
            json!({"account_id": other, "action": action, "slot": 1, "action_source": "social_panel"}),
        );
        self.app
            .db(move |conn| slayerlinks::answer(conn, &by, Some(&body), now))
            .await
    }

    async fn lists(&self, of: &str, now: i64) -> (Vec<Value>, Vec<Value>) {
        let of = of.to_owned();
        self.app.db(move |conn| slayerlinks::read_lists(conn, &of, now)).await
    }
}

fn ok_at((status, json): &(u16, Value), message: &str) -> Value {
    assert_eq!(*status, 200, "{message} {json}");
    json["payload"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invite_runs_out_after_24_hours_and_a_link_after_a_week() {
    let app = TestApp::start().await;
    let at = At { app: &app };
    let (ola, pia) = (player(&app, "Ola"), player(&app, "Pia"));
    let mut now = metagame::time::now_ms();
    befriend(&app, &ola, &pia).await;

    ok_at(&at.invite(&ola, &pia, 1, now).await, "");
    now += INVITE_EXPIRY_HOURS * HOUR + 1;
    assert!(at.lists(&pia, now).await.0.is_empty());
    let late = at.answer(&pia, &ola, "accept", now).await;
    refused_json(late.0, &late.1, 409, "it ran out");
    assert_eq!(
        app.rows("SELECT status FROM slayerlinkinvites WHERE senderId = ?1", &[&ola]),
        [json!({"status": "EXPIRED"})]
    );

    let fresh = ok_at(
        &at.invite(&ola, &pia, 1, now).await,
        "a new invite after the old one ran out",
    )["link_id"]
        .clone();
    ok_at(&at.answer(&pia, &ola, "accept", now).await, "");
    assert_eq!(at.lists(&ola, now).await.1[0]["link_id"], fresh);

    now += LINK_DURATION_HOURS * HOUR - 1;
    assert_eq!(at.lists(&ola, now).await.1.len(), 1, "a week less a moment");
    now += 2;
    assert!(at.lists(&ola, now).await.1.is_empty());
    assert!(at.lists(&pia, now).await.1.is_empty());
    ok_at(
        &at.invite(&ola, &pia, 1, now).await,
        "the slot and the pair are free again",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unfriend_or_a_block_cancels_the_waiting_invites_a_running_link_stays_until_it_ends() {
    let app = TestApp::start().await;
    let (rex, sam, tia) = (player(&app, "Rex"), player(&app, "Sam"), player(&app, "Tia"));
    befriend(&app, &rex, &sam).await;
    befriend(&app, &rex, &tia).await;
    ok(&invite(&app, &rex, &sam, json!(1)).await, "");
    ok(&invite(&app, &tia, &rex, json!(1)).await, "");
    ok(&answer(&app, &rex, &tia, "accept", 2).await, "");

    assert_eq!(
        app.call(
            "DELETE",
            &format!("/friends/api/public/friends/{rex}/{sam}"),
            req().bearer(&rex)
        )
        .await
        .status,
        204
    );
    assert!(
        invites(&app, &sam).await.is_empty(),
        "the unfriend cancelled Rex's invite"
    );
    assert_eq!(
        app.row(
            "SELECT status FROM slayerlinkinvites WHERE senderId = ?1 AND targetId = ?2",
            &[&rex, &sam]
        )["status"],
        "CANCELED"
    );
    refused(&answer(&app, &sam, &rex, "accept", 1).await, 409, "");
    refused(&invite(&app, &rex, &sam, json!(1)).await, 403, "no longer friends");

    assert_eq!(
        app.post(
            &format!("/friends/api/public/blocklist/{tia}/{rex}"),
            req().bearer(&tia)
        )
        .await
        .status,
        204
    );
    assert_eq!(
        links(&app, &rex).await.len(),
        1,
        "the running link stays (ending it is the owner's decision)"
    );
    refused(&invite(&app, &rex, &tia, json!(1)).await, 403, "blocked");
    assert_eq!(
        available(&app, &rex, &[&tia, &sam]).await,
        json!([{"account_id": tia, "available": false}, {"account_id": sam, "available": false}])
    );

    // A waiting invite between players who then block each other is cancelled too
    let uma = player(&app, "Uma");
    befriend(&app, &uma, &sam).await;
    ok(&invite(&app, &uma, &sam, json!(1)).await, "");
    assert_eq!(
        app.post(
            &format!("/friends/api/public/blocklist/{sam}/{uma}"),
            req().bearer(&sam)
        )
        .await
        .status,
        204
    );
    assert!(invites(&app, &uma).await.is_empty());
    assert_eq!(
        app.count("slayerlinkinvites", "senderId = ?1 AND status = 'CANCELED'", &[&uma]),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_sends_at_most_20_new_invites_in_10_minutes_declined_and_cancelled_invites_go_after_their_day() {
    let app = TestApp::start().await;
    let at = At { app: &app };
    let (nia, oto) = (player(&app, "Nia"), player(&app, "Oto"));
    let mut now = metagame::time::now_ms();
    befriend(&app, &nia, &oto).await;

    for round in 0..MAX_INVITES_PER_WINDOW {
        let id = ok_at(&at.invite(&nia, &oto, 1, now).await, &format!("invite {}", round + 1))["link_id"].clone();
        assert_eq!(
            ok_at(&at.invite(&nia, &oto, 1, now).await, "")["link_id"],
            id,
            "the same invite again is not a new one"
        );
        ok_at(&at.answer(&nia, &oto, "cancel", now).await, "");
        now += 1000;
    }
    let over = at.invite(&nia, &oto, 1, now).await;
    refused_json(over.0, &over.1, 409, "the 21st new invite in 10 minutes");
    assert_eq!(
        app.count("slayerlinkinvites", "senderId = ?1", &[&nia]),
        MAX_INVITES_PER_WINDOW,
        "the refused one is not stored"
    );
    ok_at(&at.invite(&oto, &nia, 1, now).await, "the limit is per sender");
    ok_at(&at.answer(&nia, &oto, "reject", now).await, "");

    now += INVITE_WINDOW_MS;
    ok_at(&at.invite(&nia, &oto, 1, now).await, "the window has moved on");
    assert_eq!(
        app.count(
            "slayerlinkinvites",
            "status IN ('CANCELED', 'DECLINED') AND ?1 IN (senderId, targetId)",
            &[&nia]
        ),
        MAX_INVITES_PER_WINDOW + 1,
        "kept while their day lasts"
    );

    now += INVITE_EXPIRY_HOURS * HOUR;
    let fresh = ok_at(&at.invite(&oto, &nia, 2, now).await, "the next new invite sweeps")["link_id"].clone();
    assert_eq!(
        app.rows(
            "SELECT inviteId, status FROM slayerlinkinvites WHERE ?1 IN (senderId, targetId)",
            &[&nia]
        ),
        [json!({"inviteId": fresh, "status": "PENDING"})],
        "declined, cancelled and run-out invites are gone once their day is over"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn needs_a_player_s_token_a_game_server_s_key_alone_is_403_and_no_auth_is_401() {
    let app = TestApp::start().await;
    let routes = [
        ("GET", "/slayerlink/status_good", None),
        ("GET", "/slayerlink/invites", None),
        ("GET", "/slayerlink/links", None),
        (
            "PUT",
            "/slayerlink/invite",
            Some(json!({"account_id": "UID-x", "slot": 1})),
        ),
        (
            "POST",
            "/slayerlink/invite",
            Some(json!({"account_id": "UID-x", "action": "accept"})),
        ),
        ("DELETE", "/slayerlink/invites/UID-x", None),
        ("DELETE", "/slayerlink/links", Some(json!({"slot": 1}))),
        ("POST", "/slayerlink/availability", Some(json!({"account_ids": []}))),
    ];
    for (method, path, body) in routes {
        let with = |r: crate::common::Req| match &body {
            Some(b) => r.json(b.clone()),
            None => r,
        };
        assert_eq!(
            app.call(method, path, with(req().gs())).await.status,
            403,
            "{method} {path} with the game server's key"
        );
        assert_eq!(
            app.call(method, path, with(req())).await.status,
            401,
            "{method} {path} without auth"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn slayer_links_0_answers_every_route_with_the_old_404_and_keeps_the_data() {
    let on = TestApp::start().await;
    let (val, wes) = (player(&on, "Val"), player(&on, "Wes"));
    befriend(&on, &val, &wes).await;
    ok(&invite(&on, &val, &wes, json!(1)).await, "");
    ok(&answer(&on, &wes, &val, "accept", 1).await, "");

    // The switch is read once, at start: a second app on the same database with SLAYER_LINKS=0
    let db = on.db.to_string_lossy().into_owned();
    let off = TestApp::with_env(&[("SLAYER_LINKS", "0"), ("DB_FILENAME", db.as_str())]).await;
    let routes = [
        ("GET", "/slayerlink/status_good".to_owned(), None),
        ("GET", "/slayerlink/invites".to_owned(), None),
        ("GET", "/slayerlink/links".to_owned(), None),
        (
            "PUT",
            "/slayerlink/invite".to_owned(),
            Some(json!({"account_id": wes, "slot": 2})),
        ),
        (
            "POST",
            "/slayerlink/invite".to_owned(),
            Some(json!({"account_id": wes, "action": "cancel"})),
        ),
        ("DELETE", format!("/slayerlink/invites/{wes}"), None),
        (
            "DELETE",
            "/slayerlink/links".to_owned(),
            Some(json!({"account_id": wes, "slot": 1})),
        ),
        (
            "POST",
            "/slayerlink/availability".to_owned(),
            Some(json!({"account_ids": [wes]})),
        ),
    ];
    for (method, path, body) in routes {
        let mut request = req().bearer(&val);
        if let Some(body) = body {
            request = request.json(body);
        }
        let reply = off.call(method, &path, request).await;
        assert_eq!((reply.status, reply.text.as_str()), (404, ""), "{method} {path}");
    }
    assert_eq!(
        links(&on, &val).await.len(),
        1,
        "the link is still there once the switch is back on"
    );
    assert_eq!(
        on.get("/slayerlink/links/rewards/UID-x/1", req().bearer(&val))
            .await
            .status,
        404,
        "the reward routes are not answered"
    );
}
