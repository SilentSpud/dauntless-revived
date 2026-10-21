//! `party.test.ts`: parties (roadmap 1.9) at the controller level, the party and invite store on the
//! app's party clock (set to 2026-09-21T12:00:00Z, as Node's `SetPartyClockForTests`), and the party
//! matchmaking against the recording deploy server. Node's `SetRegionReader` is the stored region; its
//! "aus" reader is "us" here, as a saved AUS choice now reads as US (`GetHuntRegion`).

use metagame::matchmaking::{self, Decision, Entry};
use metagame::party::Reply as PartyReply;
use serde_json::{Value, json};

use crate::common::TestApp;
use crate::deploykit::RecordingDeploy;

const A: &str = "UID-a";
const B: &str = "UID-b";
const C: &str = "UID-c";
const D: &str = "UID-d";
const E: &str = "UID-e";
const HUNT: &str = "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr";
const TUTORIAL: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";

struct Pt {
    app: TestApp,
    deploy: RecordingDeploy,
}

/// `partyenv.ts`'s settings, the five players and the clock.
async fn pt() -> Pt {
    let deploy = RecordingDeploy::start().await;
    let app = TestApp::with_env(&[
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", deploy.address.as_str()),
        ("REGISTRATION_MODE", "OPEN"),
        ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
        ("TARGET_CHANGELIST", "239827"),
    ])
    .await;
    for (id, shown) in [(A, "Alpha"), (B, "Bravo"), (C, "Charlie"), (D, "Delta"), (E, "Echo")] {
        app.run(
            "INSERT INTO users (userId, name, notes) VALUES (?1, ?2, 0)",
            &[&id, &shown],
        );
    }
    app.state
        .party_clock
        .set_for_tests(metagame::js::date_parse("2026-09-21T12:00:00.000Z").unwrap());
    Pt { app, deploy }
}

impl Pt {
    fn now(&self) -> i64 {
        self.app.state.party_clock.now()
    }

    fn advance(&self, seconds: i64) {
        self.app.state.party_clock.advance_for_tests(seconds * 1000);
    }

    async fn poll(&self, who: &str) -> Value {
        let (state, who, now) = (self.app.state.clone(), who.to_owned(), self.now());
        let settings = state.party_settings();
        self.app
            .db(move |conn| state.parties.lock().poll(conn, settings, &who, now))
            .await
    }

    async fn party_id(&self, who: &str) -> String {
        self.poll(who).await["partyId"].as_str().unwrap().to_owned()
    }

    async fn invite(&self, from: &str, to: Option<&str>, party: Option<&str>) -> PartyReply {
        let (state, from, now) = (self.app.state.clone(), from.to_owned(), self.now());
        let (to, party) = (to.map(|t| json!(t)), party.map(|p| json!(p)));
        self.app
            .db(move |conn| {
                state
                    .parties
                    .lock()
                    .invite(conn, &from, to.as_ref(), party.as_ref(), now)
            })
            .await
    }

    async fn accept(&self, who: &str, id: &str) -> PartyReply {
        let (state, who, id, now) = (self.app.state.clone(), who.to_owned(), id.to_owned(), self.now());
        let settings = state.party_settings();
        self.app
            .db(move |conn| state.parties.lock().accept(conn, settings, &who, &id, now))
            .await
    }

    async fn invites(&self, who: &str) -> Vec<Value> {
        let (state, who, now) = (self.app.state.clone(), who.to_owned(), self.now());
        let listed = self
            .app
            .db(move |conn| state.parties.lock().list_invites(conn, &who, now))
            .await;
        listed["invitations"].as_array().unwrap().clone()
    }

    fn decline(&self, who: &str, body: Value) -> PartyReply {
        self.app.state.parties.lock().decline(who, Some(&body), self.now())
    }

    fn leave(&self, who: &str) -> PartyReply {
        self.app.state.parties.lock().leave(who, self.now())
    }

    fn kick(&self, who: &str, member: &str) -> u16 {
        self.app.state.parties.lock().kick(who, member, self.now()).status
    }

    fn promote(&self, who: &str, member: &str) -> u16 {
        self.app.state.parties.lock().promote(who, member, self.now()).status
    }

    fn evict(&self, who: &str, leader: &str) -> u16 {
        self.app
            .state
            .parties
            .lock()
            .evict_leader(who, leader, self.now())
            .status
    }

    fn touch(&self, who: &str) {
        self.app.state.parties.lock().touch(who, self.now());
    }

    /// `GetPartyOf`: the members and the leader of `who`'s party.
    fn party_of(&self, who: &str) -> Option<(String, Vec<String>, String)> {
        let state = self.app.state.parties.lock();
        let id = state.party_id_of(who)?;
        let party = state.party(&id)?;
        Some((party.party_id.clone(), party.members.clone(), party.leader_id.clone()))
    }

    fn members(&self, who: &str) -> Option<Vec<String>> {
        self.party_of(who).map(|p| p.1)
    }

    fn leader(&self, who: &str) -> Option<String> {
        self.party_of(who).map(|p| p.2)
    }

    /// B leads, the others accepted
    async fn form(&self, leader: &str, members: &[&str]) -> String {
        let party = self.party_id(leader).await;
        for member in members {
            self.poll(member).await;
            assert_eq!(
                self.invite(leader, Some(member), Some(&party)).await.status,
                200,
                "invite {member}"
            );
            assert_eq!(self.accept(member, &party).await.status, 200, "accept {member}");
        }
        party
    }

    async fn join(&self, mode: &str, args: &str, hunt: &str, player: &str) -> bool {
        matchmaking::join(
            &self.app.state,
            Some(json!(mode)),
            Some(json!(args)),
            Some(json!(hunt)),
            player,
            false,
        )
        .await
        .unwrap()
    }

    async fn check(&self, player: &str) -> Option<Entry> {
        matchmaking::check_and_update(&self.app.state, player).await
    }

    async fn decide(&self, player: &str) -> Decision {
        matchmaking::decide_status(&self.app.state, player).await
    }

    fn calls(&self) -> Vec<Value> {
        self.deploy.calls()
    }

    fn clear_calls(&self) {
        self.deploy.state.lock().unwrap().calls.clear();
    }

    async fn wait_for_calls(&self, count: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.calls().len() < count && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(self.calls().len(), count);
    }

    async fn wait_for_kind(&self, player: &str, kind: &str) -> Decision {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut decision = self.decide(player).await;
        while kind_of(&decision) != kind && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            decision = self.decide(player).await;
        }
        assert_eq!(kind_of(&decision), kind, "{player}");
        decision
    }

    async fn set_region(&self, user: &str, region: matchmaking::region::Region) {
        let user = user.to_owned();
        self.app
            .db(move |conn| matchmaking::region::save_region(conn, &user, region))
            .await;
    }
}

fn kind_of(decision: &Decision) -> &'static str {
    match decision {
        Decision::Unknown => "unknown",
        Decision::Failed(_) => "failed",
        Decision::Matching { .. } => "matching",
        Decision::Travel(_) => "travel",
    }
}

fn travel(decision: &Decision) -> &Entry {
    match decision {
        Decision::Travel(entry) => entry,
        other => panic!("not travel: {}", kind_of(other)),
    }
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

fn expected(call: &Value) -> Vec<String> {
    call["ExpectedPlayers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn fails_every_party_member_after_one_minute_of_unavailable_capacity() {
    let pt = pt().await;
    pt.form(B, &[A, C]).await;
    pt.deploy.state.lock().unwrap().capacity = true;
    pt.join("ISLAND", "", HUNT, B).await;
    pt.advance(61);
    for member in [A, B, C] {
        assert_eq!(kind_of(&pt.decide(member).await), "failed", "{member}");
    }
    assert_eq!(pt.calls().len(), 1, "no allocation after the deadline");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_whole_party_waits_for_capacity_and_retries_once_with_members_still_present() {
    let pt = pt().await;
    pt.form(B, &[A, C]).await;
    pt.deploy.state.lock().unwrap().capacity = true;
    pt.join("ISLAND", "", HUNT, B).await;
    assert_eq!(pt.check(A).await.map(|e| e.ready), Some(false));
    assert_eq!(pt.calls().len(), 1);
    pt.leave(C);
    pt.advance(11);
    pt.deploy.state.lock().unwrap().capacity = false;
    tokio::join!(pt.check(A), pt.check(B));
    assert_eq!(pt.calls().len(), 2);
    assert_eq!(sorted(expected(&pt.calls()[1])), sorted(ids(&[A, B])));
    assert_eq!(pt.check(A).await.map(|e| e.ready), Some(true));
    assert_eq!(pt.check(B).await.map(|e| e.ready), Some(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_the_old_stub_s_reply_with_a_uuid_party_id_and_the_account_s_name() {
    let pt = pt().await;
    let reply = pt.poll(A).await;
    let id = reply["partyId"].as_str().unwrap().to_owned();
    assert!(
        regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
            .unwrap()
            .is_match(&id)
    );
    assert_eq!(
        reply,
        json!({"candidateId": "CANDIDATE_ID_LOL", "candidateState": "QUEUED_FOR_START", "gauntletLevel": null, "leaderPlayerId": A, "partyId": id,
            "playerHuntId": null, "playerStates": [{"consoleSessionId": null, "displayName": "Alpha", "isMemberOfCandidate": true, "platform": "win", "playerId": A}]})
    );
    let keys: Vec<&String> = reply.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "candidateId",
            "candidateState",
            "gauntletLevel",
            "leaderPlayerId",
            "partyId",
            "playerHuntId",
            "playerStates"
        ]
    );
    assert_eq!(pt.party_id(A).await, id, "the same party on the next poll");
    assert!(pt.invites(A).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn b_invites_a_a_sees_the_invite_and_accepts_with_the_party_id_both_polls_show_the_party() {
    let pt = pt().await;
    let party = pt.party_id(B).await;
    let old_a = pt.party_id(A).await;
    let sent = pt.invite(B, Some(A), Some(&party)).await;
    assert_eq!((sent.status, sent.body), (200, json!({})));
    assert_eq!(
        pt.invites(A).await,
        [
            json!({"recipientPlayerId": A, "sendingPlayerId": B, "partyId": party, "sendingPlatform": "win", "sendingDisplayName": "Bravo"})
        ]
    );
    assert!(pt.invites(B).await.is_empty(), "the sender sees no invite");
    let accepted = pt.accept(A, &party).await;
    let expected = json!({"partyId": party, "leaderPlayerId": B, "playerIds": [B, A],
        "playerStates": [
            {"playerId": B, "isMemberOfCandidate": false, "platform": "win", "displayName": "Bravo", "consoleSessionId": null},
            {"playerId": A, "isMemberOfCandidate": false, "platform": "win", "displayName": "Alpha", "consoleSessionId": null}],
        "candidateState": null, "candidateId": null, "playerHuntId": null, "gauntletLevel": null});
    assert_eq!((accepted.status, &accepted.body), (200, &expected));
    assert_eq!(pt.poll(A).await, expected);
    assert_eq!(pt.poll(B).await, expected);
    assert!(pt.invites(A).await.is_empty(), "the invite is used up");
    assert_eq!(pt.party_of(A).map(|p| p.0), Some(party.clone()));
    assert_ne!(old_a, party);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_invite_id_may_also_be_the_sender_s_id_the_client_s_decline_body_carries_it_in_both_fields() {
    let pt = pt().await;
    let party = pt.party_id(B).await;
    pt.invite(B, Some(A), Some(&party)).await;
    let accepted = pt.accept(A, B).await;
    assert_eq!(accepted.status, 200);
    assert_eq!(accepted.body["playerIds"], json!([B, A]));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_invite_id_is_404_and_accepting_twice_is_harmless() {
    let pt = pt().await;
    let party = pt.party_id(B).await;
    assert_eq!(pt.accept(A, &party).await.status, 404, "no invite yet");
    pt.invite(B, Some(A), Some(&party)).await;
    assert_eq!(pt.accept(A, "not-a-party").await.status, 404);
    assert_eq!(pt.accept(A, &party).await.status, 200);
    assert!(pt.invites(A).await.is_empty(), "the invite is gone once used");
    // A repeated accept answers the party A already joined (it was 404 before the Harmonic port)
    let again = pt.accept(A, &party).await;
    assert_eq!((again.status, again.body["playerIds"].clone()), (200, json!([B, A])));
    assert_eq!(pt.members(A), Some(ids(&[B, A])));
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_leader_invites_not_yourself_not_a_member_not_an_unknown_account_not_twice() {
    let pt = pt().await;
    let party = pt.form(B, &[A]).await;
    let p = Some(party.as_str());
    assert_eq!(pt.invite(A, Some(C), p).await.status, 403, "A is not the leader");
    assert_eq!(pt.invite(B, Some(B), p).await.status, 409, "yourself");
    assert_eq!(pt.invite(B, Some(A), p).await.status, 409, "already a member");
    assert_eq!(pt.invite(B, Some("UID-nobody"), p).await.status, 404, "unknown account");
    assert_eq!(pt.invite(B, None, p).await.status, 404, "no recipient");
    assert_eq!(
        pt.invite(B, Some("../../etc"), p).await.status,
        404,
        "not an account id"
    );
    assert_eq!(pt.invite(B, Some(C), p).await.status, 200);
    assert_eq!(pt.invite(B, Some(C), p).await.status, 409, "already invited");
    assert_eq!(pt.invites(C).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_party_id_in_the_body_is_logged_and_the_caller_s_own_party_used() {
    let pt = pt().await;
    let party = pt.party_id(B).await;
    assert_eq!(pt.invite(B, Some(A), Some("some-other-party")).await.status, 200);
    assert_eq!(pt.invites(A).await[0]["partyId"], party);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_holds_4_the_5th_is_refused_at_invite_and_at_accept() {
    let pt = pt().await;
    let party = pt.form(A, &[B, C]).await;
    assert_eq!(pt.invite(A, Some(D), Some(&party)).await.status, 200);
    assert_eq!(pt.invite(A, Some(E), Some(&party)).await.status, 200);
    assert_eq!(pt.accept(D, &party).await.status, 200);
    assert_eq!(pt.members(A).map(|m| m.len()), Some(4));
    assert_eq!(pt.accept(E, &party).await.status, 409, "full");
    assert_eq!(pt.party_of(E), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_block_either_way_stops_invites() {
    let pt = pt().await;
    pt.poll(B).await;
    let blocked = pt.app.db(|conn| metagame::friends::block(conn, A, B)).await;
    assert!(matches!(blocked.0, metagame::friends::Outcome::Done(_)));
    assert_eq!(pt.invite(B, Some(A), None).await.status, 403, "A blocked B");
    assert_eq!(pt.invite(A, Some(B), None).await.status, 403, "and the other way round");
    pt.app.db(|conn| metagame::friends::unblock(conn, A, B)).await;
    assert_eq!(pt.invite(B, Some(A), None).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn invites_expire_after_5_minutes_and_die_with_their_party() {
    let pt = pt().await;
    let party = pt.party_id(B).await;
    pt.invite(B, Some(A), Some(&party)).await;
    pt.advance(299);
    pt.touch(B);
    assert_eq!(pt.invites(A).await.len(), 1);
    pt.advance(2);
    assert!(pt.invites(A).await.is_empty(), "expired");
    pt.touch(B);
    pt.invite(B, Some(C), Some(&party)).await;
    pt.leave(B);
    assert!(pt.invites(C).await.is_empty(), "the sender left: gone");
}

#[tokio::test(flavor = "multi_thread")]
async fn decline_removes_the_invite_body_carries_the_id_in_both_fields_a_sender_can_withdraw() {
    let pt = pt().await;
    let party = pt.party_id(B).await;
    pt.invite(B, Some(A), Some(&party)).await;
    pt.invite(B, Some(C), Some(&party)).await;
    let declined = pt.decline(
        A,
        json!({"sendingPlayerId": party, "recipientPlayerId": A, "partyId": party}),
    );
    assert_eq!((declined.status, declined.body), (200, json!({})));
    assert!(pt.invites(A).await.is_empty());
    assert_eq!(
        pt.decline(
            B,
            json!({"sendingPlayerId": B, "recipientPlayerId": C, "partyId": party})
        )
        .status,
        200
    );
    assert!(pt.invites(C).await.is_empty(), "withdrawn by the sender");
    pt.invite(B, Some(C), Some(&party)).await;
    assert_eq!(
        pt.decline(
            A,
            json!({"sendingPlayerId": party, "recipientPlayerId": C, "partyId": party})
        )
        .status,
        200
    );
    assert_eq!(pt.invites(C).await.len(), 1, "someone else cannot remove C's invite");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_leaving_hands_the_lead_to_the_next_member_the_leaver_gets_a_new_party_of_one() {
    let pt = pt().await;
    let party = pt.form(A, &[B, C]).await;
    let left = pt.leave(A);
    assert_eq!((left.status, left.body), (200, json!({})));
    let now = pt.poll(B).await;
    assert_eq!(
        json!([now["partyId"], now["leaderPlayerId"], now["playerIds"]]),
        json!([party, B, [B, C]])
    );
    let alone = pt.poll(A).await;
    assert_ne!(alone["partyId"], party.as_str());
    assert_eq!(alone["candidateId"], "CANDIDATE_ID_LOL");
    pt.leave(C);
    let solo = pt.poll(B).await;
    assert_eq!(solo["partyId"], party.as_str(), "B keeps the party id");
    assert_eq!(
        solo["candidateState"], "QUEUED_FOR_START",
        "a party of one answers the old stub"
    );
    assert_eq!(
        pt.leave("UID-nobody").status,
        200,
        "leaving without a party is harmless"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_leader_kicks_the_kicked_member_is_on_their_own_after() {
    let pt = pt().await;
    let party = pt.form(A, &[B, C]).await;
    assert_eq!(pt.kick(B, C), 403, "B is not the leader");
    assert_eq!(pt.kick(A, D), 404, "not a member");
    assert_eq!(pt.kick(A, C), 200);
    assert_eq!(pt.poll(A).await["playerIds"], json!([A, B]));
    assert_ne!(pt.poll(C).await["partyId"], party.as_str());
    assert_eq!(pt.kick(B, B), 200, "naming yourself is leaving");
    assert_eq!(pt.poll(A).await["candidateId"], "CANDIDATE_ID_LOL");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_leader_promotes_and_only_a_member() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    assert_eq!(pt.promote(B, B), 403);
    assert_eq!(pt.promote(A, C), 404);
    assert_eq!(pt.promote(A, B), 200);
    assert_eq!(pt.poll(A).await["leaderPlayerId"], B);
    assert_eq!(pt.invite(A, Some(C), None).await.status, 403, "A no longer leads");
    assert_eq!(pt.invite(B, Some(C), None).await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn members_that_keep_polling_stay_the_client_s_leader_eviction_is_ignored_while_the_leader_is_heard_from() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    for _ in 0..12 {
        pt.advance(10);
        pt.poll(A).await;
        pt.poll(B).await;
    }
    assert_eq!(pt.evict(B, A), 200);
    assert_eq!(pt.members(B), Some(ids(&[A, B])), "eviction ignored: A polled 10 s ago");
    assert_eq!(pt.leader(B).as_deref(), Some(A));
}

#[tokio::test(flavor = "multi_thread")]
async fn heartbeats_count_as_being_heard_from_hunts_loading_screens() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    for _ in 0..10 {
        pt.advance(20);
        pt.touch(B);
        pt.poll(A).await;
    }
    assert_eq!(pt.members(A), Some(ids(&[A, B])));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_nobody_has_heard_from_for_120_s_is_dropped_a_vanished_leader_is_replaced() {
    let pt = pt().await;
    pt.form(A, &[B, C]).await;
    pt.advance(100);
    pt.poll(A).await;
    pt.poll(B).await;
    pt.advance(30);
    assert_eq!(pt.poll(A).await["playerIds"], json!([A, B]), "C dropped after 130 s");
    pt.advance(121);
    assert_eq!(pt.evict(B, A), 200);
    assert_eq!(
        pt.members(B),
        Some(ids(&[B])),
        "A, not heard from for 121 s, evicted on B's request"
    );
    assert_eq!(pt.party_of(A), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_the_parties_of_the_asked_players_and_only_the_caller_s_own_invitations() {
    let pt = pt().await;
    let party = pt.form(A, &[B]).await;
    pt.poll(C).await;
    pt.invite(A, Some(C), Some(&party)).await;
    pt.poll(D).await;
    pt.invite(D, Some(E), None).await;
    let status = |players: Value| {
        let (state, now) = (pt.app.state.clone(), pt.now());
        let settings = state.party_settings();
        pt.app
            .db(move |conn| state.parties.lock().status(conn, settings, C, Some(&players), now))
    };
    let result = status(json!([A, B, C, "UID-nobody", 5])).await;
    assert_eq!(result.status, 200);
    let mut parties: Vec<String> = result.body["parties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["partyId"].as_str().unwrap().to_owned())
        .collect();
    parties.sort();
    assert_eq!(parties, sorted(vec![party, pt.party_of(C).unwrap().0]));
    let recipients: Vec<Value> = result.body["invitations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["recipientPlayerId"].clone())
        .collect();
    assert_eq!(recipients, [json!(C)]);
    let (state, now) = (pt.app.state.clone(), pt.now());
    let settings = state.party_settings();
    let nope = pt
        .app
        .db(move |conn| {
            state
                .parties
                .lock()
                .status(conn, settings, A, Some(&json!("nope")), now)
        })
        .await;
    assert_eq!(nope.body, json!({"parties": [], "invitations": []}));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_s_join_starts_one_server_with_every_member_expected_members_follow_through_their_polls() {
    let pt = pt().await;
    pt.form(A, &[B, C]).await;
    assert!(pt.join("ISLAND", "", HUNT, A).await);
    assert_eq!(pt.calls().len(), 1, "one deploy call for the whole party");
    assert_eq!(
        pt.calls()[0],
        json!({"GameMode": "ISLAND", "GameArgs": "", "HuntId": HUNT, "ExpectedPlayers": [A, B, C]})
    );
    let party = pt.poll(B).await;
    assert_eq!(
        (party["candidateState"].clone(), party["playerHuntId"].clone()),
        (json!("IN_PROGRESS"), json!(HUNT))
    );
    assert!(
        party["playerStates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["isMemberOfCandidate"] == true)
    );
    assert_eq!(
        json!(pt.check(A).await.unwrap().candidate_id),
        party["candidateId"],
        "the leader's join answer carries the party's candidate"
    );
    for member in [A, B, C] {
        let decision = pt.decide(member).await;
        let entry = travel(&decision);
        assert_eq!(
            (entry.host.as_str(), entry.port, json!(entry.candidate_id)),
            ("127.0.0.1", 8775, party["candidateId"].clone())
        );
        assert_eq!(entry.party_member_ids, Some(ids(&[A, B, C])));
    }
    assert!(
        pt.join("ISLAND", "", HUNT, B).await,
        "a member's own join of the same hunt: nothing new"
    );
    assert_eq!(pt.calls().len(), 1);
    assert!(
        !pt.join("ISLAND", "", "CR19_PlayerHunt_Other", B).await,
        "a member cannot start another hunt (400)"
    );
    assert_eq!(pt.calls().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_may_follow_by_joining_the_candidate_by_id_the_exe_s_candidatejoin_nobody_else_may() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.poll(C).await;
    pt.deploy.state.lock().unwrap().delay_ms = 3000;
    pt.join("ISLAND", "", HUNT, A).await;
    let candidate = pt.poll(B).await["candidateId"].as_str().unwrap().to_owned();
    assert_eq!(
        matchmaking::join_candidate(&pt.app.state, B, &candidate)
            .await
            .unwrap()
            .map(|c| c.hunt_id),
        Some(HUNT.to_owned())
    );
    assert_eq!(kind_of(&pt.decide(B).await), "matching", "still starting");
    assert!(
        matchmaking::join_candidate(&pt.app.state, C, &candidate)
            .await
            .unwrap()
            .is_none(),
        "not in the party"
    );
    assert!(
        matchmaking::join_candidate(&pt.app.state, B, "some-other-candidate")
            .await
            .unwrap()
            .is_none()
    );
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(kind_of(&pt.decide(B).await), "travel", "then sent like any member");
    assert_eq!(pt.calls().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_s_second_join_after_travelling_the_client_always_sends_one_starts_nothing_matching_for_20_s_then_the_same_server()
 {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.join("ISLAND", "", HUNT, A).await;
    assert_eq!(kind_of(&pt.decide(A).await), "travel");
    pt.advance(25);
    assert!(pt.join("ISLAND", "", HUNT, A).await);
    assert_eq!(pt.calls().len(), 1, "no second server");
    let parked = pt.decide(A).await;
    assert!(matches!(parked, Decision::Matching { parked: true, .. }));
    pt.advance(21);
    let again = pt.decide(A).await;
    assert_eq!(
        travel(&again).port,
        8775,
        "still asking after 20 s: back to the party's server"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn members_who_poll_before_the_server_is_up_get_matching_then_in_progress() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.deploy.state.lock().unwrap().delay_ms = 3500;
    assert!(pt.join("ISLAND", "", HUNT, A).await, "answers after 2.5 s at most");
    assert_eq!(pt.poll(B).await["candidateState"], "MATCHING");
    assert_eq!(kind_of(&pt.decide(B).await), "matching");
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(pt.poll(B).await["candidateState"], "IN_PROGRESS");
    assert_eq!(kind_of(&pt.decide(B).await), "travel");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_candidate_leaves_the_party_poll_60_s_after_the_last_member_was_sent() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.join("ISLAND", "", HUNT, A).await;
    pt.decide(A).await;
    pt.advance(30);
    pt.decide(B).await;
    pt.advance(59);
    assert_ne!(pt.poll(A).await["candidateId"], Value::Null);
    pt.advance(2);
    let party = pt.poll(A).await;
    assert_eq!(
        json!([party["candidateId"], party["candidateState"], party["playerHuntId"]]),
        json!([null, null, null])
    );
    assert!(
        party["playerStates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["isMemberOfCandidate"] == false)
    );
    assert!(
        pt.join("ISLAND", "", HUNT, B).await,
        "a member's late join of the hunt they are on is answered from the last candidate"
    );
    assert_eq!(pt.calls().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_launch_tells_every_member_failed_and_clears_the_candidate() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.deploy.state.lock().unwrap().fail = true;
    assert!(pt.join("ISLAND", "", HUNT, A).await);
    assert_eq!(kind_of(&pt.decide(A).await), "failed");
    assert_eq!(kind_of(&pt.decide(B).await), "failed");
    assert_eq!(pt.poll(B).await["candidateState"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_who_leaves_before_the_server_is_up_stays_behind() {
    let pt = pt().await;
    pt.form(A, &[B, C]).await;
    pt.deploy.state.lock().unwrap().delay_ms = 3000;
    pt.join("ISLAND", "", HUNT, A).await;
    pt.leave(C);
    assert_eq!(kind_of(&pt.decide(C).await), "unknown");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(kind_of(&pt.decide(B).await), "travel");
    assert_eq!(
        expected(&pt.calls()[0]),
        ids(&[A, B, C]),
        "the server was asked for before C left"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_s_cancel_only_with_matchmaking_cancel_1_calls_it_off_for_members_not_sent_yet() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.deploy.state.lock().unwrap().delay_ms = 3000;
    pt.join("ISLAND", "", HUNT, A).await;
    matchmaking::cancel(&mut pt.app.state.parties.lock(), A, pt.now());
    assert_eq!(pt.poll(B).await["candidateId"], Value::Null);
    assert_eq!(kind_of(&pt.decide(B).await), "unknown");
    assert_eq!(kind_of(&pt.decide(A).await), "unknown");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(
        kind_of(&pt.decide(B).await),
        "unknown",
        "the late server answer changes nothing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_s_candidate_leave_takes_only_that_member_out() {
    let pt = pt().await;
    pt.form(A, &[B, C]).await;
    pt.join("ISLAND", "", HUNT, A).await;
    matchmaking::leave(&mut pt.app.state.parties.lock(), C, pt.now());
    let party = pt.poll(A).await;
    assert_eq!(party["candidateState"], "IN_PROGRESS");
    let members: Vec<Value> = party["playerStates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["isMemberOfCandidate"].clone())
        .collect();
    assert_eq!(members, [json!(true), json!(true), json!(false)]);
    assert_eq!(kind_of(&pt.decide(B).await), "travel");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tutorial_its_own_game_args_and_a_party_of_one_use_the_solo_code() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    assert!(pt.join("ISLAND", TUTORIAL, "", B).await);
    assert_eq!(pt.calls()[0].get("ExpectedPlayers"), None);
    assert_eq!(pt.poll(A).await["candidateId"], Value::Null, "no party candidate");
    *pt.app.state.parties.lock() = Default::default();
    pt.poll(C).await;
    assert!(pt.join("CITY", "", "ShatteredIsles_ReturnToRamsgate", C).await);
    assert_eq!(kind_of(&pt.decide(C).await), "travel");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_s_city_join_takes_the_members_on_the_leader_s_server_along_and_nobody_else() {
    let pt = pt().await;
    pt.form(A, &[B, C]).await;
    pt.join("ISLAND", "", HUNT, A).await;
    pt.decide(A).await;
    pt.decide(B).await;
    // C went back to Ramsgate on their own (a member's CITY join is theirs alone)
    assert!(pt.join("CITY", "", "ShatteredIsles_ReturnToRamsgate", C).await);
    assert_eq!(kind_of(&pt.decide(C).await), "travel");
    pt.clear_calls();
    assert!(pt.join("CITY", "", "ShatteredIsles_ReturnToRamsgate", A).await);
    assert_eq!(pt.calls().len(), 1);
    assert_eq!(pt.calls()[0]["GameMode"], "CITY");
    let party = pt.poll(B).await;
    assert_eq!(party["playerHuntId"], "ShatteredIsles_ReturnToRamsgate");
    let states: Vec<Value> = party["playerStates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!([s["playerId"], s["isMemberOfCandidate"]]))
        .collect();
    assert_eq!(states, [json!([A, true]), json!([B, true]), json!([C, false])]);
    for member in [A, B] {
        assert_eq!(travel(&pt.decide(member).await).port, 8777, "{member}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leader_whose_members_are_elsewhere_goes_alone() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    assert!(pt.join("SHARED", "", "ShatteredIsles_TrainingDojo", A).await);
    assert_eq!(pt.poll(B).await["candidateId"], Value::Null);
    assert_eq!(kind_of(&pt.decide(A).await), "travel");
    assert_eq!(kind_of(&pt.decide(B).await), "unknown");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_queue_whose_launch_fails_answers_failed_instead_of_sending_players_to_0() {
    let pt = pt().await;
    pt.deploy.state.lock().unwrap().fail = true;
    for player in [A, B, C, D] {
        assert!(pt.join("ISLAND", "", "CR19_PlayerHunt_SoloFail", player).await);
    }
    pt.wait_for_calls(1).await;
    for player in [A, B, C, D] {
        pt.wait_for_kind(player, "failed").await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_candidate_takes_over_a_member_s_solo_queue_entry() {
    let pt = pt().await;
    assert!(pt.join("ISLAND", "", "CR19_PlayerHunt_Solo", B).await);
    pt.form(A, &[B]).await;
    pt.join("ISLAND", "", HUNT, A).await;
    let entry = pt.check(B).await.unwrap();
    assert_eq!(
        (entry.hunt_id, entry.party_candidate, entry.ready),
        (Some(json!(HUNT)), true, true)
    );
}

const V: &str = D;
const O: &str = E;

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_join_the_same_player_s_new_join_and_a_second_player_s_join_one_server_expecting_the_two_accounts_once_each()
 {
    let pt = pt().await;
    assert!(pt.join("ISLAND", "", HUNT, V).await);
    let stale = pt.check(V).await.map(|e| e.candidate_id);
    pt.advance(192);
    assert!(pt.join("ISLAND", "", HUNT, V).await);
    pt.advance(17);
    assert!(pt.join("ISLAND", "", HUNT, O).await);
    assert_eq!(pt.calls().len(), 0, "allocation waits for a queued player to poll");
    pt.advance(21);
    let sent_o = pt.decide(O).await;
    let sent_v = pt.decide(V).await;
    assert_eq!(pt.calls().len(), 1, "one hunt server");
    assert_eq!(
        pt.calls()[0],
        json!({"GameMode": "ISLAND", "GameArgs": "", "HuntId": HUNT, "ExpectedPlayers": [V, O]})
    );
    assert_eq!(
        travel(&sent_v).port,
        travel(&sent_o).port,
        "both are sent to the same server"
    );
    assert_ne!(
        Some(travel(&sent_v).candidate_id.clone()),
        stale,
        "V follows their new join"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_who_joins_again_does_not_count_twice_toward_a_full_queue() {
    let pt = pt().await;
    for player in [V, V, O, C] {
        assert!(pt.join("ISLAND", "", HUNT, player).await, "{player}");
    }
    assert_eq!(pt.calls().len(), 0, "three players, not four");
    assert!(pt.join("ISLAND", "", HUNT, B).await);
    pt.wait_for_calls(1).await;
    assert_eq!(expected(&pt.calls()[0]), ids(&[V, O, C, B]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_join_for_another_hunt_or_for_ramsgate_also_replaces_the_older_queued_join() {
    let pt = pt().await;
    let other = "CR19_PlayerHunt_Other";
    assert!(pt.join("ISLAND", "", other, V).await);
    assert!(pt.join("ISLAND", "", HUNT, V).await, "V changes their mind");
    assert!(pt.join("ISLAND", "", HUNT, C).await);
    assert!(
        pt.join("CITY", "", "ShatteredIsles_ReturnToRamsgate", C).await,
        "C goes back to Ramsgate instead"
    );
    assert!(pt.join("ISLAND", "", other, O).await);
    pt.clear_calls();
    pt.advance(21);
    assert_eq!(kind_of(&pt.decide(V).await), "travel");
    assert_eq!(kind_of(&pt.decide(O).await), "travel");
    assert_eq!(travel(&pt.decide(C).await).port, 8777, "C is sent to Ramsgate");
    let calls: Vec<Value> = pt
        .calls()
        .iter()
        .map(|c| json!([c["HuntId"], c["ExpectedPlayers"]]))
        .collect();
    assert_eq!(calls, [json!([HUNT, [V]]), json!([other, [O]])]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_party_s_members_are_expected_together_and_once_each_even_with_older_solo_joins_of_their_own_still_queued() {
    let pt = pt().await;
    // B and a stranger (V) queued for the hunt on their own; then B's party leader A starts it for the party
    assert!(pt.join("ISLAND", "", HUNT, B).await);
    assert!(pt.join("ISLAND", "", HUNT, V).await);
    pt.form(A, &[B, C]).await;
    assert!(pt.join("ISLAND", "", HUNT, A).await);
    assert!(
        pt.join("ISLAND", "", HUNT, A).await,
        "the leader's second join starts nothing"
    );
    assert!(
        pt.join("ISLAND", "", HUNT, B).await,
        "nor a member's own join of the party's hunt"
    );
    assert_eq!(pt.calls().len(), 1);
    assert_eq!(
        pt.calls()[0],
        json!({"GameMode": "ISLAND", "GameArgs": "", "HuntId": HUNT, "ExpectedPlayers": [A, B, C]})
    );
    for member in [A, B, C] {
        assert_eq!(travel(&pt.decide(member).await).port, 8775, "{member}");
    }
    // The stranger's queue goes on without B
    pt.advance(21);
    assert_eq!(travel(&pt.decide(V).await).port, 8774);
    assert_eq!(
        pt.calls()[1],
        json!({"GameMode": "ISLAND", "GameArgs": "", "HuntId": HUNT, "ExpectedPlayers": [V]})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_travelling_party_member_survives_the_heartbeat_gap_but_the_grace_is_bounded() {
    let pt = pt().await;
    pt.form(A, &[B]).await;
    pt.join("ISLAND", "", HUNT, A).await;
    pt.wait_for_kind(A, "travel").await;
    pt.wait_for_kind(B, "travel").await;
    pt.advance(121);
    pt.poll(B).await;
    assert_eq!(pt.leader(B).as_deref(), Some(A));
    pt.evict(B, A);
    assert_eq!(pt.leader(B).as_deref(), Some(A));
    pt.advance(181);
    pt.poll(B).await;
    assert_eq!(pt.leader(B).as_deref(), Some(B));
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_tutorial_joins_share_one_allocation_and_the_immediate_retry_keeps_that_allocation() {
    let pt = pt().await;
    pt.deploy.state.lock().unwrap().delay_ms = 100;
    tokio::join!(pt.join("ISLAND", TUTORIAL, "", A), pt.join("ISLAND", TUTORIAL, "", A));
    pt.join("ISLAND", TUTORIAL, "", A).await;
    assert_eq!(pt.calls().len(), 1);
    pt.advance(61);
    pt.join("ISLAND", TUTORIAL, "", A).await;
    assert_eq!(pt.calls().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn cross_region_party_members_follow_the_leader_for_shared_worlds_then_recover_their_own_region_on_leaving() {
    let pt = pt().await;
    pt.set_region(B, matchmaking::region::Region::Us).await;
    pt.form(B, &[A]).await;
    pt.join("CITY", "", "ShatteredIsles_ReturnToRamsgate", A).await;
    assert_eq!(pt.calls().last().unwrap()["Region"], "us");
    pt.join("SHARED", "", "ShatteredIsles_TrainingDojo", A).await;
    assert_eq!(pt.calls().last().unwrap()["Region"], "us");
    pt.leave(A);
    pt.join("CITY", "", "ShatteredIsles_ReturnToRamsgate", A).await;
    assert_eq!(pt.calls().last().unwrap().get("Region"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn oce_party_leader_retains_region_when_inviting_main_players_and_city_requests_carry_the_selection() {
    let pt = pt().await;
    pt.set_region(B, matchmaking::region::Region::Us).await;
    pt.join("CITY", "", "", B).await;
    assert_eq!(pt.calls().last().unwrap()["Region"], "us");
    pt.form(B, &[A]).await;
    pt.join("ISLAND", "", HUNT, B).await;
    pt.wait_for_calls(2).await;
    assert_eq!(pt.calls()[1]["Region"], "us");
    assert_eq!(sorted(expected(&pt.calls()[1])), sorted(ids(&[B, A])));
    pt.wait_for_kind(B, "travel").await;
    pt.wait_for_kind(A, "travel").await;
}
