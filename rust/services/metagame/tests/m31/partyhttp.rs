//! `partyhttp.test.ts`: parties over HTTP, as three players' clients (A, B, C) walk the plan's tests
//! T1-T7, plus the permission rules, the account lookups and the friends routes. Node's file builds one
//! state from test to test; here each test builds the stage it starts from. The party routes run on
//! the app's party clock, so Node's clock-moving cases run over HTTP too. PARTY_SOLO_STUB is read once,
//! at start, so its case is a second app.

use serde_json::{Value, json};

use crate::common::{Reply, Req, TestApp, req, sha256_hex};
use crate::deploykit::RecordingDeploy;

const HUNT: &str = "CR19_PlayerHunt_FTUE_Pursuit_Beta_Host";
const A: &str = "UID-party-a";
const B: &str = "UID-party-b";
const C: &str = "UID-party-c";
const ADMIN: &str = "UID-party-admin";
const BUILD_ID: &str = "239827_rel-1.4.4_shipping";

struct Ph {
    app: TestApp,
    deploy: RecordingDeploy,
}

fn key_of(user: &str) -> String {
    format!("UUK_{}", sha256_hex(user))
}

/// `partyenv.ts`'s settings and the four accounts with keys, plus `extra`.
async fn ph(extra: &[(&str, &str)]) -> Ph {
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
    for (id, shown) in [(A, "Alpha"), (B, "Bravo"), (C, "Charlie"), (ADMIN, "Boss")] {
        let admin = i64::from(id == ADMIN);
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, ?3)",
            &[&id, &shown, &admin],
        );
        app.run(
            "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
            &[&id, &sha256_hex(&key_of(id))],
        );
    }
    Ph { app, deploy }
}

fn build() -> Value {
    json!({"buildId": BUILD_ID, "featureOverrides": []})
}

fn with_build(extra: Value) -> Value {
    let mut body = build();
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    body
}

impl Ph {
    async fn poll(&self, who: &str) -> Reply {
        self.app.post("/party", req().bearer(who).json(build())).await
    }

    async fn party_id(&self, who: &str) -> String {
        self.poll(who).await.json["partyId"].as_str().unwrap().to_owned()
    }

    async fn invites(&self, who: &str) -> Value {
        self.app.get("/party/invites", req().bearer(who)).await.json
    }

    async fn invite(&self, who: &str, recipient: &str, party: &str) -> Reply {
        let body = with_build(json!({"recipientPlayerId": recipient, "partyId": party}));
        self.app
            .call("PUT", "/party/invite", req().bearer(who).json(body))
            .await
    }

    async fn accept(&self, who: &str, id: &str) -> Reply {
        let body = with_build(json!({"recipientPlayerId": who, "partyId": id}));
        self.app
            .call(
                "PUT",
                &format!("/party/invite/accept/{id}"),
                req().bearer(who).json(body),
            )
            .await
    }

    async fn join(&self, who: &str, extra: Value) -> Reply {
        let mut body = json!({"buildId": BUILD_ID, "gameArgs": "", "playerId": "", "allow_crossplay": true, "session_id": "SESSION_ID_LOL", "regionUrlsPings": {}});
        body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        self.app.post("/candidate/join", req().bearer(who).json(body)).await
    }

    async fn status(&self, who: &str) -> Reply {
        self.app.get("/candidate/status", req().bearer(who)).await
    }

    async fn decline(&self, who: &str, body: Value) -> u16 {
        self.app
            .call("DELETE", "/party/invite", req().bearer(who).json(body))
            .await
            .status
    }

    fn calls(&self) -> Vec<Value> {
        self.deploy.calls()
    }

    fn clear_calls(&self) {
        self.deploy.state.lock().unwrap().calls.clear();
    }

    /// T1: B's party with A. Returns B's party id.
    async fn party_b_a(&self) -> String {
        self.poll(A).await;
        let party = self.party_id(B).await;
        assert_eq!(self.invite(B, A, &party).await.status, 200);
        assert_eq!(self.accept(A, &party).await.status, 200);
        party
    }

    /// T1-T3: B's party with A and C.
    async fn party_b_a_c(&self) -> String {
        let party = self.party_b_a().await;
        assert_eq!(self.invite(B, C, &party).await.status, 200);
        assert_eq!(self.accept(C, &party).await.status, 200);
        party
    }

    /// T5: B's hunt for the party; returns the candidate id.
    async fn hunt(&self, party: &str) -> String {
        let joined = self
            .join(B, json!({"gameMode": "ISLAND", "isPrivate": false, "partyId": party, "hunts": ["CR19_MatchmakerHunt_Host_Beta"], "playerHuntId": HUNT}))
            .await;
        assert_eq!(joined.status, 200, "{}", joined.text);
        for member in [B, A, C] {
            assert_eq!(self.status(member).await.json["status"], "IN_PROGRESS", "{member}");
        }
        joined.json["candidateId"].as_str().unwrap().to_owned()
    }

    /// T7's end: A alone in B's old party, B and C on their own, C having declined A's invite.
    async fn after_t7(&self) -> String {
        let party = self.party_b_a_c().await;
        assert_eq!(
            self.app
                .call("PUT", &format!("/party/member/promote/{A}"), req().bearer(B))
                .await
                .status,
            200
        );
        assert_eq!(
            self.app
                .call("DELETE", &format!("/party/member/{C}"), req().bearer(A))
                .await
                .status,
            200
        );
        // The kicked member's poll gives them a party of one
        self.poll(C).await;
        assert_eq!(self.invite(A, C, &party).await.status, 200);
        assert_eq!(
            self.decline(
                C,
                json!({"sendingPlayerId": party, "recipientPlayerId": C, "partyId": party})
            )
            .await,
            200
        );
        assert_eq!(
            self.app.call("DELETE", "/party/member", req().bearer(B)).await.status,
            200
        );
        party
    }
}

fn expected_party(party: &str, members: &[(&str, &str)]) -> Value {
    let ids: Vec<&str> = members.iter().map(|(id, _)| *id).collect();
    let states: Vec<Value> = members
        .iter()
        .map(|(id, shown)| json!({"playerId": id, "isMemberOfCandidate": false, "platform": "win", "displayName": shown, "consoleSessionId": null}))
        .collect();
    json!({"partyId": party, "leaderPlayerId": ids[0], "playerIds": ids, "playerStates": states, "candidateState": null, "candidateId": null,
        "playerHuntId": null, "gauntletLevel": null})
}

#[tokio::test(flavor = "multi_thread")]
async fn each_client_polls_its_own_party_of_one_b_invites_a_a_s_invite_poll_shows_it_a_accepts_both_polls_show_the_party_with_names()
 {
    let ph = ph(&[]).await;
    let solo_a = ph.poll(A).await;
    let solo_b = ph.poll(B).await;
    assert_eq!(solo_a.status, 200);
    let j = &solo_a.json;
    assert_eq!(
        json!([
            j["candidateId"],
            j["candidateState"],
            j["playerStates"][0]["displayName"]
        ]),
        json!(["CANDIDATE_ID_LOL", "QUEUED_FOR_START", "Alpha"])
    );
    let party = solo_b.json["partyId"].as_str().unwrap().to_owned();
    let sent = ph.invite(B, A, &party).await;
    assert_eq!((sent.status, sent.json), (200, json!({})));
    assert_eq!(
        ph.invites(A).await,
        json!({"invitations": [{"recipientPlayerId": A, "sendingPlayerId": B, "partyId": party, "sendingPlatform": "win", "sendingDisplayName": "Bravo"}]})
    );
    let accepted = ph.accept(A, &party).await;
    assert_eq!(accepted.status, 200);
    assert_eq!(accepted.json, expected_party(&party, &[(B, "Bravo"), (A, "Alpha")]));
    for who in [A, B] {
        assert_eq!(ph.poll(who).await.json, accepted.json, "{who}'s poll");
    }
    assert_eq!(ph.invites(A).await, json!({"invitations": []}));
}

#[tokio::test(flavor = "multi_thread")]
async fn b_invites_c_too_the_party_is_three() {
    let ph = ph(&[]).await;
    let party = ph.party_b_a().await;
    assert_eq!(ph.invite(B, C, &party).await.status, 200);
    assert_eq!(ph.accept(C, &party).await.status, 200);
    assert_eq!(ph.poll(C).await.json["playerIds"], json!([B, A, C]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_invite_poll_answers_an_empty_list_without_a_valid_token_and_nobody_s_invites_but_the_token_s() {
    let ph = ph(&[]).await;
    ph.party_b_a_c().await;
    ph.poll(ADMIN).await;
    assert_eq!(ph.invite(ADMIN, A, "").await.status, 200, "an invite for A to see");
    for (label, request) in [
        ("none", req()),
        ("bad token", req().header("authorization", "bearer not-a-token")),
        ("gs", req().gs()),
    ] {
        let anonymous = ph.app.get("/party/invites", request).await;
        assert_eq!(
            (anonymous.status, anonymous.json),
            (200, json!({"invitations": []})),
            "{label}"
        );
    }
    assert_eq!(ph.invites(A).await["invitations"].as_array().unwrap().len(), 1);
    assert_eq!(
        ph.invites(C).await["invitations"].as_array().unwrap().len(),
        0,
        "C does not see A's invite"
    );
    let admins = ph.party_id(ADMIN).await;
    assert_eq!(
        ph.decline(
            A,
            json!({"sendingPlayerId": admins, "recipientPlayerId": A, "partyId": admins})
        )
        .await,
        200
    );
    assert_eq!(
        ph.invites(A).await["invitations"].as_array().unwrap().len(),
        0,
        "declined"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn party_routes_need_a_player_s_token_none_is_401_the_game_server_key_alone_is_403() {
    let ph = ph(&[]).await;
    assert_eq!(ph.app.post("/party", req().json(build())).await.status, 401);
    assert_eq!(ph.app.post("/party", req().gs().json(build())).await.status, 403);
    assert_eq!(
        ph.app
            .call("PUT", "/party/invite", req().gs().json(json!({"recipientPlayerId": A})))
            .await
            .status,
        403
    );
    assert_eq!(ph.app.call("DELETE", "/party/member", req()).await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn accepting_for_someone_else_kicking_from_a_party_you_do_not_lead_or_are_not_in_and_inviting_into_another_s_party_are_refused()
 {
    let ph = ph(&[]).await;
    let party = ph.party_b_a_c().await;
    ph.poll(ADMIN).await;
    assert_eq!(ph.invite(ADMIN, C, "").await.status, 200);
    let admins = ph.party_id(ADMIN).await;
    let for_c = ph
        .app
        .call(
            "PUT",
            &format!("/party/invite/accept/{admins}"),
            req().bearer(A).json(json!({"recipientPlayerId": C, "partyId": "x"})),
        )
        .await;
    assert_eq!(
        for_c.status, 404,
        "A accepting C's invite: only A's own invites are looked up"
    );
    assert_eq!(
        ph.invites(C).await["invitations"].as_array().unwrap().len(),
        1,
        "C's invite is untouched"
    );
    assert_eq!(
        ph.poll(A).await.json["playerIds"],
        json!([B, A, C]),
        "A stayed where A was"
    );
    assert_eq!(
        ph.decline(
            C,
            json!({"recipientPlayerId": C, "partyId": admins, "sendingPlayerId": ADMIN})
        )
        .await,
        200
    );

    let call =
        |method: &'static str, path: String, who: &'static str| ph.app.call_owned(method, path, req().bearer(who));
    assert_eq!(
        call("DELETE", format!("/party/member/{B}"), A).await.status,
        403,
        "A is not the leader"
    );
    assert_eq!(
        call("DELETE", format!("/party/member/{B}"), ADMIN).await.status,
        404,
        "not in the caller's party"
    );
    assert_eq!(call("PUT", format!("/party/member/promote/{A}"), C).await.status, 403);
    assert_eq!(ph.invite(A, ADMIN, &party).await.status, 403, "only the leader invites");
    assert_eq!(
        call("DELETE", format!("/party/leader/{B}"), A).await.status,
        200,
        "the client's eviction call answers 200 ..."
    );
    assert_eq!(
        ph.poll(A).await.json["leaderPlayerId"],
        B,
        "... and changes nothing while B is heard from"
    );
    assert_eq!(ph.poll(B).await.json["playerIds"], json!([B, A, C]));
}

#[tokio::test(flavor = "multi_thread")]
async fn one_deploy_call_with_every_member_expected_the_members_polls_show_the_candidate_everyone_gets_the_same_server()
{
    let ph = ph(&[]).await;
    let party = ph.party_b_a_c().await;
    ph.clear_calls();
    let joined = ph
        .join(B, json!({"gameMode": "ISLAND", "isPrivate": false, "partyId": party, "hunts": ["CR19_MatchmakerHunt_Host_Beta"], "playerHuntId": HUNT}))
        .await;
    assert_eq!(joined.status, 200);
    let keys: Vec<&String> = joined.json.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "candidateId",
            "candidateStatusPeriodMillis",
            "gameMode",
            "huntId",
            "status",
            "statusReason"
        ]
    );
    assert_eq!(joined.json["candidateStatusPeriodMillis"], 1000);
    assert_eq!(
        json!([joined.json["status"], joined.json["gameMode"], joined.json["huntId"]]),
        json!(["MATCHING", "ISLAND", HUNT])
    );
    let candidate = joined.json["candidateId"].as_str().unwrap().to_owned();
    assert_eq!(
        ph.calls(),
        [json!({"GameMode": "ISLAND", "GameArgs": "", "HuntId": HUNT, "ExpectedPlayers": [B, A, C]})]
    );

    // The client sends DELETE /candidate right after every queued join; it keeps its 404 (MATCHMAKING_CANCEL unset)
    assert_eq!(ph.app.call("DELETE", "/candidate", req().bearer(B)).await.status, 404);
    for member in [A, C] {
        let party = ph.poll(member).await.json;
        assert_eq!(
            json!([party["candidateState"], party["candidateId"], party["playerHuntId"]]),
            json!(["IN_PROGRESS", candidate, HUNT]),
            "{member}"
        );
        assert!(
            party["playerStates"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["isMemberOfCandidate"] == true)
        );
    }
    // A member's client may follow by joining the candidate by id (the exe's CandidateJoin)
    let follow = ph
        .app
        .post(
            &format!("/candidate/join/{candidate}"),
            req().bearer(C).json(json!({"buildId": BUILD_ID})),
        )
        .await;
    assert_eq!(
        (follow.status, follow.json),
        (
            200,
            json!({"candidateId": candidate, "candidateStatusPeriodMillis": 1000, "gameMode": "ISLAND", "huntId": HUNT, "status": "MATCHING", "statusReason": null})
        )
    );
    assert_eq!(
        ph.app
            .post(
                &format!("/candidate/join/{candidate}"),
                req().bearer(ADMIN).json(json!({}))
            )
            .await
            .status,
        404,
        "not a member of it"
    );
    assert_eq!(ph.calls().len(), 1);
    for member in [B, A, C] {
        let polled = ph.status(member).await;
        assert_eq!(polled.status, 200);
        assert_eq!(
            polled.json,
            json!({"candidateId": candidate, "candidateStatusPeriodMillis": 1000, "gameMode": "ISLAND", "huntId": HUNT,
                "playerStates": {B: {}, A: {}, C: {}},
                "serverInfo": {"buildId": "239827_1.4.4_shipping", "gameSessionId": candidate, "host": "127.0.0.1", "port": 8775},
                "status": "IN_PROGRESS", "statusDuration": 0, "statusReason": null}),
            "{member}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_s_second_join_while_loading_the_island_starts_no_second_server() {
    let ph = ph(&[]).await;
    let party = ph.party_b_a_c().await;
    let candidate = ph.hunt(&party).await;
    let again = ph
        .join(B, json!({"gameMode": "ISLAND", "isPrivate": true, "partyId": party, "hunts": ["CR19_MatchmakerHunt_Host_Beta"], "playerHuntId": HUNT}))
        .await;
    assert_eq!(
        json!([again.status, again.json["candidateId"], again.json["status"]]),
        json!([200, candidate, "MATCHING"])
    );
    assert_eq!(ph.calls().len(), 1);
    assert_eq!(
        ph.status(B).await.json["status"],
        "MATCHING",
        "answered like the solo second join, so the client does not travel twice"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_cannot_start_a_hunt_of_their_own_while_in_the_party() {
    let ph = ph(&[]).await;
    let party = ph.party_b_a_c().await;
    ph.hunt(&party).await;
    assert_eq!(
        ph.join(
            A,
            json!({"gameMode": "ISLAND", "playerHuntId": "CR19_PlayerHunt_Pursuit_Bullseye_Beta"})
        )
        .await
        .status,
        400
    );
    assert_eq!(ph.calls().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn city_from_the_leader_moves_every_member_that_was_sent_to_the_leader_s_server() {
    let ph = ph(&[]).await;
    let party = ph.party_b_a_c().await;
    ph.hunt(&party).await;
    ph.clear_calls();
    let back = ph
        .join(
            B,
            json!({"gameMode": "CITY", "playerHuntId": "ShatteredIsles_ReturnToRamsgate"}),
        )
        .await;
    assert_eq!(back.status, 200);
    assert_eq!(
        ph.calls().iter().map(|c| c["GameMode"].clone()).collect::<Vec<_>>(),
        [json!("CITY")]
    );
    for member in [A, C] {
        assert_eq!(
            ph.poll(member).await.json["playerHuntId"],
            "ShatteredIsles_ReturnToRamsgate"
        );
        let polled = ph.status(member).await;
        assert_eq!(
            json!([polled.json["status"], polled.json["serverInfo"]["port"]]),
            json!(["IN_PROGRESS", 8777]),
            "{member}"
        );
    }
    assert_eq!(ph.status(B).await.json["serverInfo"]["port"], 8777);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_change_shows_in_the_next_poll() {
    let ph = ph(&[]).await;
    let party = ph.party_b_a_c().await;
    assert_eq!(
        ph.app
            .call("PUT", &format!("/party/member/promote/{A}"), req().bearer(B))
            .await
            .status,
        200
    );
    assert_eq!(ph.poll(C).await.json["leaderPlayerId"], A);
    assert_eq!(
        ph.app
            .call("DELETE", &format!("/party/member/{C}"), req().bearer(A))
            .await
            .status,
        200
    );
    assert_eq!(ph.poll(B).await.json["playerIds"], json!([B, A]));
    let kicked = ph.poll(C).await.json;
    assert_ne!(kicked["partyId"], party.as_str());
    assert_eq!(kicked["candidateId"], "CANDIDATE_ID_LOL");
    assert_eq!(ph.invite(A, C, &party).await.status, 200);
    assert_eq!(
        ph.decline(
            C,
            json!({"sendingPlayerId": party, "recipientPlayerId": C, "partyId": party})
        )
        .await,
        200
    );
    assert_eq!(ph.invites(C).await, json!({"invitations": []}));
    assert_eq!(
        ph.app.call("DELETE", "/party/member", req().bearer(B)).await.status,
        200
    );
    let alone = ph.poll(A).await.json;
    assert_eq!(
        json!([
            alone["partyId"],
            alone["leaderPlayerId"],
            alone["candidateState"],
            alone["playerStates"].as_array().unwrap().len()
        ]),
        json!([party, A, "QUEUED_FOR_START", 1])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn party_solo_stub_0_answers_a_party_of_one_without_the_placeholder_candidate_idle_to_the_client_read_on_every_poll()
 {
    let plain = ph(&[("PARTY_SOLO_STUB", "0")]).await;
    let reply = plain.poll(A).await;
    assert_eq!(reply.status, 200);
    let party = reply.json["partyId"].as_str().unwrap().to_owned();
    assert_eq!(reply.json, expected_party(&party, &[(A, "Alpha")]));
    let stub = ph(&[]).await;
    assert_eq!(
        stub.poll(A).await.json["candidateId"],
        "CANDIDATE_ID_LOL",
        "the stub again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn post_party_status_lists_the_asked_players_parties_and_the_caller_s_own_invitations() {
    let ph = ph(&[]).await;
    ph.after_t7().await;
    ph.poll(B).await;
    let reply = ph
        .app
        .post("/party/status", req().bearer(C).json(json!({"playerIds": [A, B, C]})))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["parties", "invitations"]
    );
    assert_eq!(reply.json["parties"].as_array().unwrap().len(), 3);
    assert_eq!(reply.json["invitations"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn accountid_accountid_answers_an_array_of_the_known_accounts_no_query_answers_the_caller_s_account_as_before() {
    let ph = ph(&[]).await;
    let many = ph
        .app
        .get(
            &format!("/account/api/public/account?accountId={A}&accountId={B}&accountId=UID-nobody"),
            req().bearer(C),
        )
        .await;
    assert_eq!(
        many.json,
        json!([{"id": A, "displayName": "Alpha", "externalAuths": {}}, {"id": B, "displayName": "Bravo", "externalAuths": {}}])
    );
    let one = ph
        .app
        .get(&format!("/account/api/public/account?accountId={A}"), req().bearer(C))
        .await;
    assert_eq!(
        one.json,
        json!([{"id": A, "displayName": "Alpha", "externalAuths": {}}])
    );
    let own = ph.app.get("/account/api/public/account", req().bearer(C)).await;
    assert_eq!(
        json!([own.json["id"], own.json["displayName"], own.json["ageGroup"]]),
        json!([C, "Charlie", "ADULT"])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_id_and_account_displayname_name_answer_with_a_token_and_the_old_replies_without() {
    let ph = ph(&[]).await;
    let get = |path: String, who: Option<&'static str>| {
        let request = who.map_or_else(req, |w| req().bearer(w));
        ph.app.get_owned(path, request)
    };
    assert_eq!(
        get(format!("/account/api/public/account/{B}"), Some(A)).await.json,
        json!({"id": B, "displayName": "Bravo", "externalAuths": {}})
    );
    assert_eq!(
        get(format!("/account/api/public/account/{B}"), None).await.json,
        json!({})
    );
    assert_eq!(
        get("/account/api/public/account/UID-nobody".into(), Some(A)).await.json,
        json!({})
    );
    assert_eq!(
        get(format!("/account/api/public/account/{B}/externalAuths"), Some(A))
            .await
            .json,
        json!({})
    );
    assert_eq!(
        get("/account/api/public/account/displayName/charlie".into(), Some(A))
            .await
            .json,
        json!({"id": C, "displayName": "Charlie", "externalAuths": {}})
    );
    assert_eq!(
        get("/account/api/public/account/displayName/Nobody".into(), Some(A))
            .await
            .status,
        404
    );
    assert_eq!(
        get("/account/api/public/account/displayName/Charlie".into(), None)
            .await
            .status,
        404
    );
}

fn mappings(entries: Value) -> Value {
    json!({"accountMappings": entries, "code": "OK", "message": "", "payload": {"accountMappings": entries}})
}

#[tokio::test(flavor = "multi_thread")]
async fn post_account_mapping_answers_accountmappings_as_an_object_keyed_by_each_asked_id_flat_and_wrapped_at_once_and_never_404s()
 {
    let ph = ph(&[]).await;
    let map = |body: Value, who: Option<&'static str>| {
        let request = who.map_or_else(req, |w| req().bearer(w));
        ph.app.post_owned("/account/mapping".into(), request.json(body))
    };
    let phoenix = |id: &str| json!({"accountId": id, "accountType": "phoenix"});
    // Exactly what the live 1.4.4 client sent after Add Friends (22 September 2026)
    let as_sent = map(json!({"srcAccountType": "epic", "ids": [B]}), Some(A)).await;
    assert_eq!(as_sent.status, 200);
    assert_eq!(as_sent.json, mappings(json!({B: phoenix(B)})));
    assert!(
        as_sent.json["accountMappings"].is_object(),
        "an array reads as an empty object in the client"
    );
    assert_eq!(
        as_sent.json["accountMappings"][B]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["accountId", "accountType"],
        "only the two keys the exe reads"
    );
    // Several ids: keyed as asked, once each, unknown ids left out
    let several = map(
        json!({"srcAccountType": "epic", "ids": [C, "UID-nobody", B, C]}),
        Some(A),
    )
    .await;
    assert_eq!(several.json, mappings(json!({C: phoenix(C), B: phoenix(B)})));
    assert_eq!(
        several.json["accountMappings"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [C, B]
    );
    // srcAccountType "phoenix" (any case) maps to Epic ids
    assert_eq!(
        map(json!({"srcAccountType": "Phoenix", "ids": [B]}), Some(A))
            .await
            .json,
        mappings(json!({B: {"accountId": B, "accountType": "epic"}}))
    );
    // The older guesses at the body still work: a bare array, externalIds, type/externalAuthType
    assert_eq!(map(json!([C]), Some(A)).await.json, mappings(json!({C: phoenix(C)})));
    assert_eq!(
        map(json!({"type": "epic", "externalIds": [B]}), Some(A)).await.json,
        mappings(json!({B: phoenix(B)}))
    );
    assert_eq!(
        map(json!({"externalAuthType": "psn", "ids": [B]}), Some(A)).await.json,
        mappings(json!({B: phoenix(B)}))
    );
    assert_eq!(
        map(json!({"srcAccountType": "bad type!", "ids": [B]}), Some(A))
            .await
            .json,
        mappings(json!({B: phoenix(B)}))
    );
    // No token: nothing is mapped; an empty or odd body is still a 200 with no mappings.
    let no_token = map(json!({"srcAccountType": "epic", "ids": [B]}), None).await;
    assert_eq!((no_token.status, no_token.json), (200, mappings(json!({}))));
    assert_eq!(map(json!({}), Some(A)).await.json, mappings(json!({})));
    assert_eq!(
        map(json!({"ids": [{"a": 1}, 7, "", "x".repeat(200)]}), Some(A))
            .await
            .json,
        mappings(json!({}))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn account_mapping_0_maps_nothing_read_on_every_request() {
    let off = ph(&[("ACCOUNT_MAPPING", "0")]).await;
    let reply = off
        .app
        .post(
            "/account/mapping",
            req().bearer(A).json(json!({"srcAccountType": "epic", "ids": [B]})),
        )
        .await;
    assert_eq!((reply.status, reply.json), (200, mappings(json!({}))));
    let on = ph(&[]).await;
    let reply = on
        .app
        .post(
            "/account/mapping",
            req().bearer(A).json(json!({"srcAccountType": "epic", "ids": [B]})),
        )
        .await;
    assert_eq!(
        reply.json["accountMappings"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [B]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn accountinfo_public_describes_the_asked_account_its_id_its_name_its_epic_id_and_answers_404_for_an_unknown_one()
{
    let ph = ph(&[]).await;
    let info = |id: &str, who: Option<&'static str>| {
        let request = who.map_or_else(req, |w| req().bearer(w));
        ph.app
            .post_owned("/accountinfo/public".into(), request.json(json!({"accountId": id})))
    };
    let other = info(B, Some(A)).await;
    assert_eq!(other.status, 200);
    assert_eq!(
        other.json,
        json!({"accountId": B, "username": "Bravo", "linkedAccounts": [{"accountId": B, "accountType": "epic"}], "isSubscribed": true, "language": null})
    );
    let own = info(A, Some(A)).await;
    assert_eq!(
        json!([own.json["accountId"], own.json["username"], own.json["linkedAccounts"]]),
        json!([A, "Alpha", [{"accountId": A, "accountType": "epic"}]])
    );
    let unknown = info("UID-nobody", Some(A)).await;
    assert_eq!((unknown.status, unknown.json), (404, json!({})));
    assert_eq!(info("bad id!", Some(A)).await.status, 404);
    assert_eq!(info(B, None).await.status, 401, "still needs auth");
}

#[tokio::test(flavor = "multi_thread")]
async fn accountinfo_public_legacy_1_puts_the_upstream_reply_back_the_caller_s_id_200_with_an_empty_name_for_an_unknown_id()
 {
    let ph = ph(&[("ACCOUNTINFO_PUBLIC_LEGACY", "1")]).await;
    let old = ph
        .app
        .post("/accountinfo/public", req().bearer(A).json(json!({"accountId": B})))
        .await;
    let expected = json!({"accountId": A, "isSubscribed": true, "language": null, "linkedAccounts": [{"accountId": A, "accountType": "epic"}], "username": "Bravo"});
    assert_eq!(old.text, expected.to_string());
    let unknown = ph
        .app
        .post(
            "/accountinfo/public",
            req().bearer(A).json(json!({"accountId": "UID-nobody"})),
        )
        .await;
    assert_eq!(
        json!([unknown.status, unknown.json["username"], unknown.json["accountId"]]),
        json!([200, "", A])
    );
}

async fn friend_list(ph: &Ph, who: &str, pending: bool) -> Value {
    let query = if pending { "?includePending=true" } else { "" };
    ph.app
        .get(&format!("/friends/api/public/friends/{who}{query}"), req().bearer(who))
        .await
        .json
}

fn summary(list: &Value) -> Vec<Value> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|f| json!([f["accountId"], f["status"], f["direction"]]))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn request_accept_the_two_directions_and_unfriend() {
    let ph = ph(&[]).await;
    assert_eq!(friend_list(&ph, A, true).await, json!([]));
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{A}/{B}"), req().bearer(A))
            .await
            .status,
        204
    );
    let outbound = friend_list(&ph, A, true).await;
    assert_eq!(summary(&outbound), [json!([B, "PENDING", "OUTBOUND"])]);
    let created = outbound[0]["created"].as_str().unwrap();
    assert!(
        regex::Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$")
            .unwrap()
            .is_match(created)
    );
    assert_eq!(
        summary(&friend_list(&ph, B, true).await),
        [json!([A, "PENDING", "INBOUND"])]
    );
    assert_eq!(
        friend_list(&ph, B, false).await,
        json!([]),
        "without includePending only friends"
    );
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{B}/{A}"), req().bearer(B))
            .await
            .status,
        204,
        "B accepts by asking back"
    );
    assert_eq!(
        summary(&friend_list(&ph, A, true).await),
        [json!([B, "ACCEPTED", "OUTBOUND"])]
    );
    assert_eq!(
        summary(&friend_list(&ph, B, false).await),
        [json!([A, "ACCEPTED", "INBOUND"])]
    );
    assert_eq!(
        ph.app
            .call(
                "DELETE",
                &format!("/friends/api/public/friends/{B}/{A}"),
                req().bearer(B)
            )
            .await
            .status,
        204
    );
    assert_eq!(friend_list(&ph, A, true).await, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_reads_and_changes_only_its_own_lists() {
    let ph = ph(&[]).await;
    ph.app
        .post(&format!("/friends/api/public/friends/{A}/{C}"), req().bearer(A))
        .await;
    let path = format!("/friends/api/public/friends/{A}?includePending=true");
    let foreign = ph.app.get(&path, req().bearer(B)).await;
    assert_eq!(
        (foreign.status, foreign.json),
        (200, json!([])),
        "another account's list: the old empty reply, never A's data"
    );
    assert_eq!(
        ph.app.get(&path, req()).await.json,
        json!([]),
        "no token: the old empty list"
    );
    assert_eq!(
        ph.app.get(&path, req().bearer(A)).await.json.as_array().unwrap().len(),
        1,
        "A sees the request to C"
    );
    assert_eq!(
        ph.app
            .call(
                "DELETE",
                &format!("/friends/api/public/friends/{A}/{C}"),
                req().bearer(A)
            )
            .await
            .status,
        204
    );
    assert_eq!(
        ph.app
            .get(&format!("/friends/api/public/blocklist/{A}"), req())
            .await
            .json,
        json!({"blockedUsers": []})
    );
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{B}/{C}"), req().bearer(A))
            .await
            .status,
        403
    );
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{A}/{C}"), req())
            .await
            .status,
        401
    );
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{A}/UID-nobody"), req().bearer(A))
            .await
            .status,
        404
    );
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{A}/{A}"), req().bearer(A))
            .await
            .status,
        400
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_block_removes_the_party_invites_between_the_two_and_stops_friend_requests_and_party_invites_both_ways() {
    let ph = ph(&[]).await;
    ph.after_t7().await;
    // C declined A's invite in T7: A waits 2 minutes before inviting C again
    assert_eq!(ph.invite(A, C, "").await.status, 409, "declined a moment ago");
    ph.app
        .state
        .party_clock
        .set_for_tests(metagame::time::now_ms() + 2 * 60 * 1000 + 1000);
    assert_eq!(ph.invite(A, C, "").await.status, 200, "two minutes later");
    let from_a = |list: &Value| {
        list["invitations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["sendingPlayerId"] == A)
            .count()
    };
    assert_eq!(from_a(&ph.invites(C).await), 1);
    let blocklist = |who: &str| format!("/friends/api/public/blocklist/{who}");
    assert_eq!(
        ph.app
            .post(&format!("{}/{A}", blocklist(C)), req().bearer(C))
            .await
            .status,
        204
    );
    assert_eq!(
        ph.app.get(&blocklist(C), req().bearer(C)).await.json,
        json!({"blockedUsers": [A]})
    );
    assert_eq!(from_a(&ph.invites(C).await), 0, "the block removed A's invite");
    assert_eq!(
        ph.app
            .post(&format!("/friends/api/public/friends/{A}/{C}"), req().bearer(A))
            .await
            .status,
        403
    );
    assert_eq!(ph.invite(A, C, "").await.status, 403);
    assert_eq!(
        ph.app
            .call("DELETE", &format!("{}/{A}", blocklist(C)), req().bearer(C))
            .await
            .status,
        204
    );
    assert_eq!(
        ph.app.get(&blocklist(C), req().bearer(C)).await.json,
        json!({"blockedUsers": []})
    );
    assert_eq!(
        ph.invites(C).await["invitations"],
        json!([]),
        "removed, not only hidden"
    );
    assert_eq!(ph.invite(A, C, "").await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn recent_players_and_settings_answer_their_empty_defaults() {
    let ph = ph(&[]).await;
    assert_eq!(
        ph.app
            .get(
                &format!("/friends/api/public/list/fortnite/{A}/recentPlayers"),
                req().bearer(A)
            )
            .await
            .json,
        json!([])
    );
    assert_eq!(
        ph.app
            .get(&format!("/friends/api/v1/{A}/settings"), req().bearer(A))
            .await
            .json,
        json!({"acceptInvites": "public"})
    );
}

fn by_key(who: &str) -> Req {
    req().header("x-undaunted-user-api-key", &key_of(who))
}

#[tokio::test(flavor = "multi_thread")]
async fn partyinvite_the_key_s_owner_invites_an_admin_may_name_the_inviter_directly_only() {
    let ph = ph(&[]).await;
    let host = |request: Req, body: Value| {
        ph.app
            .post_owned("/undaunted/api/PartyInvite".into(), request.json(body))
    };
    let sent = host(by_key(B), json!({"Username": "charlie"})).await;
    assert_eq!(
        (sent.status, sent.json),
        (200, json!({"From": "Bravo", "To": "Charlie"}))
    );
    assert!(
        ph.invites(C).await["invitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["sendingPlayerId"] == B)
    );
    assert_eq!(
        host(by_key(A), json!({"Username": "Bravo", "From": "Charlie"}))
            .await
            .status,
        403,
        "not an admin"
    );
    let for_someone = host(by_key(ADMIN), json!({"Username": "Alpha", "From": C})).await;
    assert_eq!(
        (for_someone.status, for_someone.json),
        (200, json!({"From": "Charlie", "To": "Alpha"}))
    );
    let proxied = host(
        by_key(ADMIN).header("x-forwarded-for", "203.0.113.9"),
        json!({"Username": "Alpha", "From": "Bravo"}),
    )
    .await;
    assert_eq!(proxied.status, 403, "admin through a proxy");
    assert_eq!(host(by_key(B), json!({"Username": "Nobody"})).await.status, 404);
    assert_eq!(host(req(), json!({"Username": "Alpha"})).await.status, 401);
    let refused = host(by_key(B), json!({"Username": "Charlie"})).await;
    assert_eq!(
        (refused.status, refused.json["error"].clone()),
        (409, json!("party_invite_refused")),
        "already invited"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn friends_sends_a_request_or_accepts_the_one_the_other_side_sent() {
    let ph = ph(&[]).await;
    let sent = ph
        .app
        .post("/undaunted/api/Friends", by_key(C).json(json!({"Username": "Bravo"})))
        .await;
    assert_eq!(
        (sent.status, sent.json),
        (200, json!({"From": "Charlie", "To": "Bravo", "Result": "requested"}))
    );
    let back = ph
        .app
        .post("/undaunted/api/Friends", by_key(B).json(json!({"Username": "charlie"})))
        .await;
    assert_eq!((back.status, back.json["Result"].clone()), (200, json!("accepted")));
    let ids: Vec<Value> = friend_list(&ph, B, false)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["accountId"].clone())
        .collect();
    assert_eq!(ids, [json!(C)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_server_s_heartbeat_without_a_player_token_records_nobody() {
    let ph = ph(&[]).await;
    let beat = ph
        .app
        .post(
            "/heartbeat",
            req()
                .gs()
                .json(json!({"map": "/Game/Maps/ramsgate/ramsgate_01_persistent"})),
        )
        .await;
    assert_eq!((beat.status, beat.text.as_str()), (200, "20000"));
    let own = ph
        .app
        .post(
            "/heartbeat",
            req()
                .bearer(A)
                .json(json!({"map": "/Game/Maps/ramsgate/ramsgate_01_persistent"})),
        )
        .await;
    assert_eq!(own.text, "20000");
    let players = ph.app.get("/undaunted/api/PrivateOnlineStats", by_key(ADMIN)).await;
    let ids: Vec<Value> = players
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["UserId"].clone())
        .collect();
    assert_eq!(ids, [json!(A)], "only the player's own heartbeat");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_join_the_same_player_s_new_join_and_a_second_player_s_join_for_the_same_hunt_one_server_expecting_two_accounts()
 {
    let ph = ph(&[]).await;
    let clock = &ph.app.state.party_clock;
    clock.set_for_tests(metagame::time::now_ms());
    let live_hunt = "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr";
    let hunt_join = |who: &'static str| {
        ph.join(who, json!({"gameMode": "ISLAND", "isPrivate": false, "hunts": ["CR19_MatchmakerHunt_Lerawr_Beta"], "playerHuntId": live_hunt}))
    };
    let cancel = |who: &'static str| ph.app.call("DELETE", "/candidate", req().bearer(who));

    // V (A here) joins after a metagame restart, and the client's cancel right after keeps its 404
    let stale = hunt_join(A).await;
    assert_eq!(json!([stale.status, stale.json["status"]]), json!([200, "MATCHING"]));
    assert_eq!(cancel(A).await.status, 404);
    // That join is never matched. Over three minutes later V joins again, then O (B here)
    clock.advance_for_tests(192 * 1000);
    let again = hunt_join(A).await;
    assert_eq!(cancel(A).await.status, 404);
    assert_ne!(again.json["candidateId"], stale.json["candidateId"]);
    clock.advance_for_tests(4 * 1000);
    assert_eq!(hunt_join(B).await.status, 200);
    assert_eq!(cancel(B).await.status, 404);
    assert_eq!(ph.calls().len(), 0, "the queue waits five seconds after its first join");
    clock.advance_for_tests(1000);
    let polled_o = ph.status(B).await;
    let polled_v = ph.status(A).await;
    assert_eq!(
        ph.calls(),
        [json!({"GameMode": "ISLAND", "GameArgs": "", "HuntId": live_hunt, "ExpectedPlayers": [A, B]})],
        "one hunt server"
    );
    assert_eq!(
        json!([polled_v.json["status"], polled_o.json["status"]]),
        json!(["IN_PROGRESS", "IN_PROGRESS"])
    );
    assert_eq!(
        polled_v.json["serverInfo"]["port"], polled_o.json["serverInfo"]["port"],
        "both on the same server"
    );
    assert_eq!(
        polled_v.json["candidateId"], again.json["candidateId"],
        "V follows their new join"
    );
}
