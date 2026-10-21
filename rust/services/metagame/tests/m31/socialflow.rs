//! `socialflow.test.ts`: the social flows end to end, the way the 1.4.4 client walks them
//! (docs/findings/social.md). Every reply goes through the client model (`socialclient`), which files
//! user info and account mappings exactly as the exe does. Node's file builds one state from section to
//! section; here each test sets up what it needs. The clock-moving cases call the controllers with the
//! time they want.

use std::collections::{HashMap, HashSet};

use metagame::friends::{self, Outcome};
use serde_json::{Value, json};

use crate::common::{Reply, TestApp, req};
use crate::deploykit::RecordingDeploy;
use crate::socialclient::{
    Added, Resolved, SocialClient, Transport, UserInfo, add_user_info, client_of, guild_error_of,
    parse_account_mappings, parse_block_list, parse_friends_list, parse_invitations, parse_phoenix_envelope,
};

const A: &str = "UID-social-a";
const B: &str = "UID-social-b";
const C: &str = "UID-social-c";
const D: &str = "UID-social-d";
/// an id on a blocklist whose account no longer exists
const GONE: &str = "UID-social-gone";
const NAMES: [(&str, &str); 4] = [(A, "Alpha"), (B, "Bravo"), (C, "Charlie"), (D, "Delta")];
const HUNT: &str = "CR19_PlayerHunt_FTUE_Pursuit_Beta_Host";
const BUILD_ID: &str = "239827_rel-1.4.4_shipping";

fn name(id: &str) -> &'static str {
    NAMES.iter().find(|(i, _)| *i == id).map_or("", |(_, n)| n)
}

/// `socialenv.ts`'s settings, the four players and the recording deploy server, plus `extra`.
async fn social(extra: &[(&str, &str)]) -> (TestApp, RecordingDeploy) {
    let deploy = RecordingDeploy::start().await;
    let mut env = vec![
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", deploy.address.as_str()),
        ("REGISTRATION_MODE", "OPEN"),
        ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
        ("TARGET_CHANGELIST", "239827"),
    ];
    env.extend_from_slice(extra);
    let app = TestApp::with_env(&env).await;
    for (id, shown) in NAMES {
        add_user(&app, id, shown);
    }
    (app, deploy)
}

fn add_user(app: &TestApp, id: &str, shown: &str) {
    app.run(
        "INSERT INTO users (userId, name, notes) VALUES (?1, ?2, 0)",
        &[&id, &shown],
    );
}

fn build(extra: Value) -> Value {
    let mut body = json!({"buildId": BUILD_ID, "featureOverrides": []});
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    body
}

async fn poll(app: &TestApp, who: &str) -> Reply {
    app.post("/party", req().bearer(who).json(build(json!({})))).await
}

async fn friends_list(app: &TestApp, who: &str) -> Vec<String> {
    let reply = app
        .get(
            &format!("/friends/api/public/friends/{who}?includePending=true"),
            req().bearer(who),
        )
        .await;
    parse_friends_list(&reply.text)
        .unwrap()
        .iter()
        .map(|e| format!("{} {} {}", e.account_id, e.status, e.direction))
        .collect()
}

async fn block_list(app: &TestApp, who: &str) -> Vec<String> {
    parse_block_list(
        &app.get(&format!("/friends/api/public/blocklist/{who}"), req().bearer(who))
            .await
            .text,
    )
    .unwrap()
}

async fn invitations(app: &TestApp, who: &str) -> Vec<Value> {
    parse_invitations(&app.get("/party/invites", req().bearer(who)).await.text).unwrap()
}

async fn invite(app: &TestApp, from: &str, to: &str, party: &str) -> Reply {
    app.call(
        "PUT",
        "/party/invite",
        req()
            .bearer(from)
            .json(build(json!({"recipientPlayerId": to, "partyId": party}))),
    )
    .await
}

// What the server answered before these fixes, recorded literally (upstream /accountinfo/public; the
// two /account/mapping hotfixes of 22 September 2026)
fn old_accountinfo(caller: &str, shown: &str) -> String {
    serde_json::to_string(&json!({"accountId": caller, "isSubscribed": true, "language": null,
        "linkedAccounts": [{"accountId": caller, "accountType": "epic"}], "username": shown}))
    .unwrap()
}

fn old_mapping_array(id: &str, shown: &str) -> String {
    let entry = json!({"accountType": "phoenix", "accountId": id, "id": id, "epic": id, "phoenix": id, "srcAccountType": "epic",
        "srcAccountId": id, "srcId": id, "dstAccountType": "phoenix", "dstAccountId": id, "displayName": shown});
    json!({"code": "OK", "message": "", "payload": {"accountMappings": [entry]}, "accountMappings": [entry]})
        .to_string()
}

fn old_mapping_keyed(id: &str, shown: &str) -> String {
    json!({id: [{"accountId": id, "displayName": shown, "type": "epic", "externalAuthId": id, "externalAuthIdType": "epic", "externalDisplayName": shown}]})
        .to_string()
}

#[test]
fn the_old_account_mapping_replies_map_nothing_the_upstream_404_fails_the_query() {
    for old in [old_mapping_array(B, "Bravo"), old_mapping_keyed(B, "Bravo")] {
        assert_eq!(parse_account_mappings(200, &old).unwrap().len(), 0, "{old}");
    }
    assert_eq!(parse_account_mappings(404, ""), Err("Empty response payload"));
    assert_eq!(parse_account_mappings(200, "<html>"), Err("Invalid response payload"));
    let good = parse_account_mappings(
        200,
        &json!({"accountMappings": {B: {"accountId": B, "accountType": "Phoenix"}}}).to_string(),
    )
    .unwrap();
    assert_eq!(good.len(), 1);
    assert_eq!(
        (good[0].0.as_str(), good[0].1.kind, good[0].1.id.as_str()),
        (B, "Phoenix", B)
    );
}

#[test]
fn the_old_accountinfo_public_reply_files_another_player_under_the_caller_s_id_so_that_player_is_never_set_up() {
    let mut cache: HashMap<String, UserInfo> = HashMap::new();
    assert!(
        matches!(
            add_user_info(&mut cache, 200, &old_accountinfo(A, "Alpha")),
            Added::Stored(_)
        ),
        "A's own lookup at login"
    );
    assert_eq!(
        add_user_info(&mut cache, 200, &old_accountinfo(A, "Bravo")),
        Added::Ignored(A.into()),
        "B's lookup lands on A's entry"
    );
    assert!(!cache.contains_key(B));
    assert_eq!(cache[A].username, "Alpha");
    assert_eq!(add_user_info(&mut cache, 404, "{}"), Added::Failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_fed_the_old_replies_drops_add_friends_and_invalidates_a_party_invite_s_sender() {
    let old_server: Transport<'static> = Box::new(|_method, path, body| {
        Box::pin(async move {
            let body = body.unwrap_or(Value::Null);
            match path.as_str() {
                "/account/mapping" => {
                    let id = body["ids"][0].as_str().unwrap_or_default();
                    (200, old_mapping_array(id, name(id)))
                }
                "/accountinfo/public" => (
                    200,
                    old_accountinfo(A, name(body["accountId"].as_str().unwrap_or_default())),
                ),
                _ => (404, String::new()),
            }
        })
    });
    let mut client = SocialClient::new(A, old_server).login().await;
    assert!(
        matches!(client.resolve_from_epic(B).await, Resolved::Dropped(_)),
        "Add Friends: no request is ever sent"
    );
    assert_eq!(
        client.resolve_from_phoenix(B).await,
        "invalidated",
        "the invite never shows under PARTY INVITES"
    );
}

#[test]
fn the_guild_envelope_a_204_is_a_failure_and_an_unknown_code_is_unknown_unable_to_create_guild() {
    assert!(!parse_phoenix_envelope(204, "").ok);
    assert!(parse_phoenix_envelope(200, &json!({"code": "OK", "message": "", "payload": {}}).to_string()).ok);
    assert_eq!(guild_error_of(Some("")), "Unknown");
    assert_eq!(guild_error_of(Some("SeizedAdorableQuillshot")), "GuildNameTaken");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_adds_bravo_by_name_name_lookup_mapping_user_info_the_toolkit_check_then_the_request_b_sees_it_at_login_and_accepts()
 {
    let (app, _deploy) = social(&[]).await;
    let mut client_a = client_of(&app, A).login().await;
    let by_name = app
        .get("/account/api/public/account/displayName/bravo", req().bearer(A))
        .await;
    assert_eq!(
        (by_name.status, &by_name.json),
        (200, &json!({"id": B, "displayName": "Bravo", "externalAuths": {}})),
        "any case"
    );

    assert_eq!(
        client_a.resolve_from_epic(by_name.json["id"].as_str().unwrap()).await,
        Resolved::Resolved(B.into())
    );
    assert_eq!(
        client_a.calls,
        [
            "POST /accountinfo/public",
            "POST /account/mapping",
            "POST /accountinfo/public"
        ]
    );
    assert_eq!(
        client_a.users[B],
        UserInfo {
            username: "Bravo".into(),
            epic_id: Some(B.into())
        }
    );

    let sent = app
        .post(&format!("/friends/api/public/friends/{A}/{B}"), req().bearer(A))
        .await;
    assert_eq!((sent.status, sent.text.as_str()), (204, ""));
    assert_eq!(friends_list(&app, A).await, [format!("{B} PENDING OUTBOUND")]);

    // B's next login: the friends list, then each listed id is resolved the same way
    let mut client_b = client_of(&app, B).login().await;
    assert_eq!(friends_list(&app, B).await, [format!("{A} PENDING INBOUND")]);
    assert_eq!(client_b.resolve_from_epic(A).await, Resolved::Resolved(A.into()));
    assert_eq!(client_b.name_of(A), Some("Alpha"));

    // Accept Epic Friend Invite: the same POST from B's side, with an empty JSON body
    let accepted = app
        .post(&format!("/friends/api/public/friends/{B}/{A}"), req().bearer(B).raw(""))
        .await;
    assert_eq!(accepted.status, 204);
    assert_eq!(friends_list(&app, A).await, [format!("{B} ACCEPTED OUTBOUND")]);
    assert_eq!(friends_list(&app, B).await, [format!("{A} ACCEPTED INBOUND")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_name_is_404_the_unknown_toast_the_player_s_own_name_is_their_own_id_the_invitedself_toast() {
    let (app, _deploy) = social(&[]).await;
    assert_eq!(
        app.get("/account/api/public/account/displayName/Nobody", req().bearer(A))
            .await
            .status,
        404
    );
    assert_eq!(
        app.get("/account/api/public/account/displayName/ALPHA", req().bearer(A))
            .await
            .json["id"],
        A
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resolve_through_the_same_chain_a_blocked_account_that_no_longer_exists_is_dropped_without_breaking_the_others()
{
    let (app, _deploy) = social(&[]).await;
    assert_eq!(
        app.post(&format!("/friends/api/public/blocklist/{A}/{C}"), req().bearer(A))
            .await
            .status,
        204
    );
    let now = metagame::time::now_ms();
    app.run(
        "INSERT INTO blocks (blockerId, blockedId, createdAt) VALUES (?1, ?2, ?3)",
        &[&A, &GONE, &now],
    );
    let blocked = block_list(&app, A).await;
    assert_eq!(blocked, [C, GONE]);

    let mut client_a = client_of(&app, A).login().await;
    let mut outcomes = Vec::new();
    for id in &blocked {
        outcomes.push(matches!(client_a.resolve_from_epic(id).await, Resolved::Resolved(_)));
    }
    assert_eq!(outcomes, [true, false], "resolved, dropped");
    assert_eq!(client_a.name_of(C), Some("Charlie"));
    assert_eq!(
        app.post("/accountinfo/public", req().bearer(A).json(json!({"accountId": GONE})))
            .await
            .status,
        404
    );

    for id in [C, GONE] {
        assert_eq!(
            app.call(
                "DELETE",
                &format!("/friends/api/public/blocklist/{A}/{id}"),
                req().bearer(A)
            )
            .await
            .status,
            204
        );
    }
    assert!(block_list(&app, A).await.is_empty());
}

/// B's party with invites out to A and C; returns B's party id.
async fn party_b_invited(app: &TestApp) -> String {
    let party = poll(app, B).await.json["partyId"].as_str().unwrap().to_owned();
    poll(app, A).await;
    poll(app, C).await;
    for recipient in [A, C] {
        let sent = invite(app, B, recipient, &party).await;
        assert_eq!((sent.status, &sent.json), (200, &json!({})));
    }
    party
}

/// B's party with A and C in it.
async fn party_b_joined(app: &TestApp) -> String {
    let party = party_b_invited(app).await;
    for who in [A, C] {
        let body = build(json!({"recipientPlayerId": who, "partyId": party}));
        assert_eq!(
            app.call(
                "PUT",
                &format!("/party/invite/accept/{party}"),
                req().bearer(who).json(body)
            )
            .await
            .status,
            200
        );
    }
    party
}

#[tokio::test(flavor = "multi_thread")]
async fn b_invites_a_and_c_with_the_client_s_body_each_recipient_s_client_sets_up_b_from_accountinfo_public_and_shows_the_invite()
 {
    let (app, _deploy) = social(&[]).await;
    let party = party_b_invited(&app).await;
    for recipient in [A, C] {
        let received = invitations(&app, recipient).await;
        assert_eq!(
            received,
            [
                json!({"recipientPlayerId": recipient, "sendingPlayerId": B, "partyId": party, "sendingPlatform": "win", "sendingDisplayName": "Bravo"})
            ]
        );
        let mut client = client_of(&app, recipient).login().await;
        assert_eq!(
            client
                .resolve_from_phoenix(received[0]["sendingPlayerId"].as_str().unwrap())
                .await,
            "initialised",
            "the entry shows under PARTY INVITES"
        );
        assert_eq!(client.name_of(B), Some("Bravo"));
        assert_eq!(
            client.name_of(recipient),
            Some(name(recipient)),
            "the recipient's own entry is untouched"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn both_accept_with_the_client_s_body_every_client_resolves_every_member_to_that_member_s_own_name() {
    let (app, _deploy) = social(&[]).await;
    let party = party_b_joined(&app).await;
    for who in [A, B, C] {
        let state = poll(&app, who).await.json;
        assert_eq!(
            json!([state["partyId"], state["leaderPlayerId"], state["playerIds"]]),
            json!([party, B, [B, A, C]])
        );
        let mut client = client_of(&app, who).login().await;
        for member in state["playerStates"].as_array().unwrap() {
            let id = member["playerId"].as_str().unwrap();
            assert_eq!(client.resolve_from_phoenix(id).await, "initialised");
            assert_eq!(client.name_of(id), Some(name(id)), "{who} sees {id}");
            assert_eq!(member["displayName"], name(id));
        }
        let names: HashSet<&str> = client.users.values().map(|info| info.username.as_str()).collect();
        assert_eq!(names.len(), 3, "no member filed under the caller's name");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_picks_a_hunt_one_deploy_call_every_member_expected_exactly_once() {
    let (app, deploy) = social(&[]).await;
    let party = party_b_joined(&app).await;
    deploy.state.lock().unwrap().calls.clear();
    let body = json!({"buildId": BUILD_ID, "gameArgs": "", "playerId": "", "allow_crossplay": true, "session_id": "SESSION_ID_LOL",
        "regionUrlsPings": {}, "gameMode": "ISLAND", "isPrivate": false, "partyId": party, "hunts": ["CR19_MatchmakerHunt_Host_Beta"], "playerHuntId": HUNT});
    let joined = app.post("/candidate/join", req().bearer(B).json(body)).await;
    assert_eq!(joined.status, 200, "{}", joined.text);
    let calls = deploy.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["ExpectedPlayers"], json!([B, A, C]));
    for member in [A, C] {
        assert_eq!(
            poll(&app, member).await.json["candidateId"],
            joined.json["candidateId"],
            "{member} follows the leader"
        );
    }
    for who in [A, B, C] {
        assert_eq!(app.call("DELETE", "/party/member", req().bearer(who)).await.status, 200);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn with_accountinfo_public_legacy_1_the_same_invite_is_lost_again_exactly_as_on_22_september_2026() {
    let (app, _deploy) = social(&[("ACCOUNTINFO_PUBLIC_LEGACY", "1")]).await;
    let party = poll(&app, D).await.json["partyId"].as_str().unwrap().to_owned();
    assert_eq!(invite(&app, D, A, &party).await.status, 200);
    assert_eq!(invitations(&app, A).await.len(), 1, "the poll delivers it ...");
    let mut client = client_of(&app, A).login().await;
    assert_eq!(
        client.resolve_from_phoenix(D).await,
        "invalidated",
        "... and the client drops it"
    );
    assert_eq!(client.name_of(A), Some("Alpha"));
    let decline = json!({"sendingPlayerId": D, "recipientPlayerId": A, "partyId": party});
    assert_eq!(
        app.call("DELETE", "/party/invite", req().bearer(A).json(decline))
            .await
            .status,
        200
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn c_looks_up_a_b_and_itself_in_different_orders_each_id_holds_its_own_name_and_epic_id() {
    let (app, _deploy) = social(&[]).await;
    for order in [[A, B, C], [C, B, A], [B, C, A]] {
        let mut client = client_of(&app, C);
        for id in order {
            assert_eq!(client.query_user_info(id).await.key(), Some(id));
        }
        for id in [A, B, C] {
            assert_eq!(
                client.users[id],
                UserInfo {
                    username: name(id).into(),
                    epic_id: Some(id.into())
                },
                "{order:?}: {id}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_displayname_body_answers_the_named_account_any_case_an_unknown_name_is_404_accountid_wins_over_a_name() {
    let (app, _deploy) = social(&[]).await;
    let public = |body: Option<Value>| {
        let request = match body {
            Some(body) => req().bearer(A).json(body),
            None => req().bearer(A),
        };
        app.post("/accountinfo/public", request)
    };
    let by_name = public(Some(json!({"displayname": "bRAVO"}))).await;
    assert_eq!(
        json!([by_name.status, by_name.json["accountId"], by_name.json["username"]]),
        json!([200, B, "Bravo"])
    );
    assert_eq!(public(Some(json!({"displayname": "Nobody"}))).await.status, 404);
    assert_eq!(
        public(Some(json!({"accountId": C, "displayname": "Bravo"}))).await.json["accountId"],
        C
    );
    assert_eq!(public(Some(json!({}))).await.status, 404);
    assert_eq!(public(None).await.status, 404, "no body at all");
}

#[tokio::test(flavor = "multi_thread")]
async fn phoenix_maps_to_epic_at_most_100_ids_each_once_unknown_ids_left_out_nothing_without_a_token_or_with_account_mapping_0()
 {
    let (app, _deploy) = social(&[]).await;
    let mapping = |who: Option<&str>, body: Value| {
        let request = match who {
            Some(who) => req().bearer(who).json(body),
            None => req().json(body),
        };
        app.post("/account/mapping", request)
    };
    let reverse = mapping(Some(A), json!({"srcAccountType": "phoenix", "ids": [B]})).await;
    let parsed = parse_account_mappings(reverse.status, &reverse.text).unwrap();
    assert_eq!(
        parsed
            .iter()
            .map(|(k, m)| (k.as_str(), m.kind, m.id.as_str()))
            .collect::<Vec<_>>(),
        [(B, "MCP", B)]
    );

    let many: Vec<String> = (0..105).map(|i| format!("UID-social-many-{i:03}")).collect();
    for (index, id) in many.iter().enumerate() {
        add_user(&app, id, &format!("Many{index}"));
    }
    let mut ids: Vec<Value> = many.iter().map(|id| json!(id)).collect();
    ids.extend([json!(many[0]), json!("UID-nobody")]);
    let capped = mapping(Some(A), json!({"srcAccountType": "epic", "ids": ids})).await;
    let keys: Vec<&String> = capped.json["accountMappings"].as_object().unwrap().keys().collect();
    assert_eq!(keys, many[..100].iter().collect::<Vec<_>>());
    assert_eq!(
        capped.json["payload"]["accountMappings"],
        capped.json["accountMappings"]
    );

    let twice = mapping(Some(A), json!({"srcAccountType": "epic", "ids": [B, B, "UID-nobody"]})).await;
    assert_eq!(
        twice.json["accountMappings"],
        json!({B: {"accountId": B, "accountType": "phoenix"}})
    );
    assert_eq!(
        mapping(None, json!({"srcAccountType": "epic", "ids": [B]})).await.json["accountMappings"],
        json!({})
    );

    let (off, _deploy) = social(&[("ACCOUNT_MAPPING", "0")]).await;
    let reply = off
        .post(
            "/account/mapping",
            req().bearer(A).json(json!({"srcAccountType": "epic", "ids": [B]})),
        )
        .await;
    assert!(parse_account_mappings(reply.status, &reply.text).is_ok());
    assert_eq!(reply.json["accountMappings"], json!({}));
    assert!(
        matches!(client_of(&off, A).resolve_from_epic(B).await, Resolved::Dropped(_)),
        "the switch puts the old outcome back"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn accountinfo_public_legacy_1_restores_the_upstream_reply_byte_for_byte() {
    let (app, _deploy) = social(&[("ACCOUNTINFO_PUBLIC_LEGACY", "1")]).await;
    let public = |id: &str| app.post("/accountinfo/public", req().bearer(C).json(json!({"accountId": id})));
    assert_eq!(public(A).await.text, old_accountinfo(C, "Alpha"));
    assert_eq!(public("UID-nobody").await.text, old_accountinfo(C, ""));
}

#[tokio::test(flavor = "multi_thread")]
async fn put_on_the_blocklist_path_blocks_like_post_friend_requests_refused_and_delete_unblocks() {
    let (app, _deploy) = social(&[]).await;
    let path = |a: &str, b: &str| format!("/friends/api/public/blocklist/{a}/{b}");
    assert_eq!(app.call("PUT", &path(A, D), req().bearer(A)).await.status, 204);
    assert_eq!(block_list(&app, A).await, [D]);
    assert_eq!(
        app.post(&format!("/friends/api/public/friends/{D}/{A}"), req().bearer(D))
            .await
            .status,
        403
    );
    assert_eq!(
        app.call("PUT", &path(D, A), req().bearer(A)).await.status,
        403,
        "only the caller's own list"
    );
    assert_eq!(app.call("PUT", &path(A, D), req()).await.status, 401);
    assert_eq!(app.call("DELETE", &path(A, D), req().bearer(A)).await.status, 204);
    assert!(block_list(&app, A).await.is_empty());
}

fn status_of((outcome, _): friends::Changed) -> u16 {
    match outcome {
        Outcome::Done(_) => 204,
        Outcome::Refused(status, _) => status,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn t_20_new_requests_per_10_minutes_at_most_50_unanswered_accepting_an_inbound_request_is_never_limited() {
    let (app, _deploy) = social(&[]).await;
    let (e, x) = ("UID-social-e", "UID-social-x");
    add_user(&app, e, "Echo");
    add_user(&app, x, "Xray");
    let targets: Vec<String> = (0..52).map(|i| format!("UID-social-cap-{i:02}")).collect();
    for (index, id) in targets.iter().enumerate() {
        add_user(&app, id, &format!("Cap{index}"));
    }
    let mut offset = 0;
    let ask = |target: String, offset: i64| {
        let state = app.state.clone();
        app.db(move |conn| {
            friends::send_or_accept_at(
                conn,
                &state.friend_requests,
                e,
                &target,
                metagame::time::now_ms() + offset,
            )
        })
    };

    // X asks E first: E's accept later must not count against E's limits
    assert_eq!(
        app.post(&format!("/friends/api/public/friends/{x}/{e}"), req().bearer(x))
            .await
            .status,
        204
    );
    for (index, target) in targets[..20].iter().enumerate() {
        assert_eq!(
            status_of(ask(target.clone(), offset).await),
            204,
            "request {}",
            index + 1
        );
    }
    assert_eq!(
        status_of(ask(targets[20].clone(), offset).await),
        409,
        "the 21st in 10 minutes"
    );
    assert_eq!(
        status_of(ask(targets[0].clone(), offset).await),
        204,
        "asking again changes nothing and is not refused"
    );
    assert_eq!(
        status_of(ask(x.to_owned(), offset).await),
        204,
        "accepting X's request while the window is full"
    );

    offset += 10 * 60 * 1000 + 1;
    for (index, target) in targets[20..40].iter().enumerate() {
        assert_eq!(
            status_of(ask(target.clone(), offset).await),
            204,
            "request {}, after the window moved",
            index + 21
        );
    }
    offset += 10 * 60 * 1000 + 1;
    for (index, target) in targets[40..50].iter().enumerate() {
        assert_eq!(
            status_of(ask(target.clone(), offset).await),
            204,
            "request {}",
            index + 41
        );
    }
    assert_eq!(
        status_of(ask(targets[50].clone(), offset).await),
        409,
        "the 51st unanswered request"
    );
    let outbound = friends_list(&app, e)
        .await
        .iter()
        .filter(|entry| entry.ends_with("PENDING OUTBOUND"))
        .count();
    assert_eq!(outbound, 50);

    // Withdrawing one frees a place
    assert_eq!(
        app.call(
            "DELETE",
            &format!("/friends/api/public/friends/{e}/{}", targets[0]),
            req().bearer(e)
        )
        .await
        .status,
        204
    );
    assert_eq!(status_of(ask(targets[50].clone(), offset).await), 204);
    assert_eq!(status_of(ask(targets[51].clone(), offset).await), 409, "full again");
    let wrong = app
        .post(
            &format!("/friends/api/public/friends/{}/{e}", targets[51]),
            req().bearer(e),
        )
        .await;
    assert_eq!(wrong.status, 403, "the URL's first id must be the caller");
}

const P: &str = "UID-social-p";
const Q: &str = "UID-social-q";
const R: &str = "UID-social-r";
const S: &str = "UID-social-s";
const W: &str = "UID-social-w";

/// Section 11's players: P, Q, R, S, W and 21 targets.
async fn invite_app() -> (TestApp, RecordingDeploy, Vec<String>) {
    let (app, deploy) = social(&[]).await;
    for (id, shown) in [(P, "Papa"), (Q, "Quebec"), (R, "Romeo"), (S, "Sierra"), (W, "Whiskey")] {
        add_user(&app, id, shown);
    }
    let targets: Vec<String> = (0..21).map(|i| format!("UID-social-t{i:02}")).collect();
    for (index, id) in targets.iter().enumerate() {
        add_user(&app, id, &format!("Tango{index}"));
    }
    (app, deploy, targets)
}

/// The party controller at `now` (Node's `SetPartyClockForTests`).
struct PartyAt<'a> {
    app: &'a TestApp,
}

impl PartyAt<'_> {
    async fn party_of(&self, who: &str, now: i64) -> String {
        let (state, who) = (self.app.state.clone(), who.to_owned());
        let settings = state.party_settings();
        let reply = self
            .app
            .db(move |conn| state.parties.lock().poll(conn, settings, &who, now))
            .await;
        reply["partyId"].as_str().unwrap().to_owned()
    }

    async fn send(&self, from: &str, to: &str, now: i64) -> u16 {
        let party = self.party_of(from, now).await;
        let (state, from, to) = (self.app.state.clone(), from.to_owned(), to.to_owned());
        self.app
            .db(move |conn| {
                state
                    .parties
                    .lock()
                    .invite(conn, &from, Some(&json!(to)), Some(&json!(party)), now)
            })
            .await
            .status
    }

    async fn invites_of(&self, who: &str, now: i64) -> Vec<Value> {
        let (state, who) = (self.app.state.clone(), who.to_owned());
        let listed = self
            .app
            .db(move |conn| state.parties.lock().list_invites(conn, &who, now))
            .await;
        listed["invitations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["sendingPlayerId"].clone())
            .collect()
    }

    async fn decline(&self, who: &str, from: &str, now: i64) -> u16 {
        let party = self.party_of(from, now).await;
        let body = json!({"sendingPlayerId": party, "recipientPlayerId": who, "partyId": party});
        self.app.state.parties.lock().decline(who, Some(&body), now).status
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_block_removes_the_invites_between_the_two_both_ways_an_invite_between_blocked_players_is_never_listed_and_cannot_be_accepted()
 {
    let (app, _deploy, _) = invite_app().await;
    let at = PartyAt { app: &app };
    let now = metagame::time::now_ms;
    assert_eq!(at.send(P, Q, now()).await, 200);
    assert_eq!(at.send(Q, P, now()).await, 200);
    assert_eq!(
        (at.invites_of(Q, now()).await, at.invites_of(P, now()).await),
        (vec![json!(P)], vec![json!(Q)])
    );

    assert_eq!(
        app.post(&format!("/friends/api/public/blocklist/{Q}/{P}"), req().bearer(Q))
            .await
            .status,
        204
    );
    assert_eq!(
        (at.invites_of(Q, now()).await, at.invites_of(P, now()).await),
        (vec![], vec![]),
        "both directions"
    );
    assert_eq!(
        app.call(
            "DELETE",
            &format!("/friends/api/public/blocklist/{Q}/{P}"),
            req().bearer(Q)
        )
        .await
        .status,
        204
    );
    assert!(at.invites_of(Q, now()).await.is_empty(), "removed, not only hidden");

    // A block written straight to the database (as one made before this rule): the invite is hidden,
    // and accepting it is 404
    let party = at.party_of(P, now()).await;
    assert_eq!(at.send(P, R, now()).await, 200);
    let stamp = now();
    app.run(
        "INSERT INTO blocks (blockerId, blockedId, createdAt) VALUES (?1, ?2, ?3)",
        &[&R, &P, &stamp],
    );
    assert!(invitations(&app, R).await.is_empty());
    let body = build(json!({"recipientPlayerId": R, "partyId": party}));
    let accepted = app
        .call(
            "PUT",
            &format!("/party/invite/accept/{party}"),
            req().bearer(R).json(body),
        )
        .await;
    assert_eq!((accepted.status, accepted.json), (404, json!({})));
    let members: Vec<Value> = poll(&app, R).await.json["playerStates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["playerId"].clone())
        .collect();
    assert_eq!(members, [json!(R)], "R stays in their own party");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_sender_waits_2_minutes_before_inviting_the_same_player_again_other_senders_do_not() {
    let (app, _deploy, _) = invite_app().await;
    let at = PartyAt { app: &app };
    let now = metagame::time::now_ms();
    assert_eq!(at.send(P, Q, now).await, 200);
    assert_eq!(at.decline(Q, P, now).await, 200);
    assert!(at.invites_of(Q, now).await.is_empty());
    assert_eq!(at.send(P, Q, now).await, 409, "at once");
    assert_eq!(at.send(S, Q, now).await, 200, "another sender");
    assert_eq!(
        at.send(P, Q, now + 2 * 60 * 1000 + 1000).await,
        200,
        "two minutes later"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sender_sends_at_most_20_invites_per_10_minutes_409_counted_across_declines() {
    let (app, _deploy, targets) = invite_app().await;
    let at = PartyAt { app: &app };
    let mut now = metagame::time::now_ms() + 2 * 60 * 1000 + 1000;
    for (index, target) in targets[..20].iter().enumerate() {
        assert_eq!(at.send(W, target, now).await, 200, "invite {}", index + 1);
        assert_eq!(at.decline(target, W, now).await, 200);
    }
    assert_eq!(at.send(W, &targets[20], now).await, 409, "the 21st in 10 minutes");
    now += 10 * 60 * 1000;
    assert_eq!(at.send(W, &targets[20], now).await, 200, "after the window moved");
}
