//! `chatmodel.test.ts`: the client model against two servers (docs/findings/chat.md): pull request #9's
//! first server, from its stanza templates (the model reproduces what Vvoidddd saw with a real 1.4.4
//! client on 22 September 2026), and ours, captured from the real chat in this process.

use crate::chatclient::{
    ChatClientModel, EXIT_PENDING, JOIN_PUBLIC_PENDING, JOINED, JoinResult, NOT_JOINED, lookup_from, url_decode,
    url_encode,
};
use crate::chatwire::{ChatServer, DOMAIN, WireClient, login, options};
use crate::common::TestApp;

const A: &str = "UID-model-a";
const B: &str = "UID-model-b";
const NAMES: [(&str, &str); 2] = [(A, "Alpha"), (B, "Bravo")];
/// A room name seen in the 1.4.4 client's log
const ROOM: &str = "City-d147475b-7742-4e7b-8142-6c0f55dda06b";
const DEFAULT_RESOURCE: &str = "V2:MissingGameServiceForAppId:WIN::00000000000000000000000000000000";

fn muc() -> String {
    format!("muc.{DOMAIN}")
}

// ---- Pull request #9's replies (UndauntedMetagame/src/realtime/chat.ts at 9981955) ----
// It kept the room JID lowercased, answered from <room>/<account id> with no <item jid>, and bound a
// random hex resource when the game's (67 characters) did not fit its 64-character rule.

fn pr9_room() -> String {
    format!("{}@{}", ROOM.to_lowercase(), muc())
}

fn pr9_jid(uid: &str) -> String {
    format!("{uid}@{DOMAIN}/0a1b2c3d4e5f")
}

fn pr9_self_presence(uid: &str, resource: Option<&str>, extra: &str) -> String {
    format!(
        r#"<presence from="{}/{}" to="{}"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="member" role="participant"/><status code="110"/>{extra}</x></presence>"#,
        pr9_room(),
        resource.unwrap_or(uid),
        pr9_jid(uid)
    )
}

fn pr9_message(sender_resource: &str, to: &str, text: &str, nick: Option<&str>) -> String {
    format!(
        r#"<message from="{}/{sender_resource}" to="{}" type="groupchat" id="m1"><body>{text}</body>{}</message>"#,
        pr9_room(),
        pr9_jid(to),
        nick.map(|n| format!(r#"<nick xmlns="http://jabber.org/protocol/nick">{n}</nick>"#))
            .unwrap_or_default()
    )
}

fn model(uid: &str, resource: &str) -> ChatClientModel {
    let name = NAMES.iter().find(|(id, _)| *id == uid).unwrap().1;
    ChatClientModel::new(uid, resource, name, None)
}

fn lookup() -> impl Fn(&str) -> Option<String> {
    lookup_from(&NAMES)
}

// ---- The client model reproduces what the first chat server showed (pull request #9) ----

#[tokio::test]
async fn m1_as_shipped_a_players_own_line_shows_its_account_id_uid() {
    let mut alpha = model(A, DEFAULT_RESOURCE);
    assert!(alpha.join_public_room(ROOM, "Alpha").is_some());
    alpha.receive(&pr9_self_presence(A, None, ""));
    assert_eq!(
        alpha.room_of(ROOM).unwrap().state,
        JOINED,
        "the join completes: the account id is in the nickname"
    );
    alpha.receive(&pr9_message(A, A, "Ready?", Some("Alpha")));
    assert_eq!(alpha.shown_lines(lookup()).await, [format!("{A}: Ready?")]);
}

#[tokio::test]
async fn m2_a_second_player_would_have_seen_those_lines_as_unknown_it_never_heard_of_the_sender() {
    let mut bravo = model(B, DEFAULT_RESOURCE);
    bravo.join_public_room(ROOM, "Bravo");
    bravo.receive(&pr9_self_presence(B, None, ""));
    assert_eq!(bravo.room_of(ROOM).unwrap().state, JOINED);
    bravo.receive(&pr9_message(A, B, "Ready?", Some("Alpha")));
    assert_eq!(bravo.shown_lines(lookup()).await, ["[unknown]: Ready?"]);
}

#[tokio::test]
async fn m3_the_username_in_place_of_the_account_id_also_with_status_210_leaves_the_join_pending_another_operation_already_pending()
 {
    for extra in ["", r#"<status code="210"/>"#] {
        let mut alpha = model(A, DEFAULT_RESOURCE);
        alpha.join_public_room(ROOM, "Alpha");
        alpha.receive(&pr9_self_presence(A, Some("Alpha"), extra));
        assert_eq!(
            alpha.room_of(ROOM).unwrap().state,
            JOIN_PUBLIC_PENDING,
            "not recognised as its own presence"
        );
        assert_eq!(alpha.join_public_room(ROOM, "Alpha"), None);
        assert_eq!(
            alpha.log.last().unwrap(),
            &format!("MUC: JoinPublicRoom failed. Another operation already pending for room {ROOM}")
        );
    }
}

#[tokio::test]
async fn m4_presence_from_the_account_id_but_the_message_from_the_username_shows_unknown() {
    let mut alpha = model(A, DEFAULT_RESOURCE);
    alpha.join_public_room(ROOM, "Alpha");
    alpha.receive(&pr9_self_presence(A, None, ""));
    alpha.receive(&pr9_message("Alpha", A, "Ready?", None));
    assert_eq!(alpha.shown_lines(lookup()).await, ["[unknown]: Ready?"]);
}

#[tokio::test]
async fn m5_a_message_from_a_nickname_with_no_presence_first_is_shown_as_unknown_not_dropped() {
    let mut alpha = model(A, DEFAULT_RESOURCE);
    alpha.join_public_room(ROOM, "Alpha");
    let presence = format!(
        r#"<presence from="{ROOM}@{}/{}" to="x"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant" jid="{A}@{DOMAIN}/{}"/><status code="110"/></x></presence>"#,
        muc(),
        alpha.nickname("Alpha"),
        alpha.resource
    );
    alpha.receive(&presence);
    alpha.receive(&format!(
        r#"<message type="groupchat" from="{ROOM}@{}/Bravo:{B}:V2:X:WIN::1" to="x"><body>hi</body></message>"#,
        muc()
    ));
    assert_eq!(alpha.shown_lines(lookup()).await, ["[unknown]: hi"]);
    assert!(!alpha.log.iter().any(|line| line.starts_with("could not get user id")));
}

#[tokio::test]
async fn m9_one_member_per_account_a_lingering_old_connections_leave_after_the_new_ones_join_turns_that_players_lines_to_unknown()
 {
    let res1 = "V2:MissingGameServiceForAppId:WIN::11111111111111111111111111111111";
    let res2 = "V2:MissingGameServiceForAppId:WIN::22222222222222222222222222222222";
    let occupant = |resource: &str, unavailable: bool| {
        format!(
            r#"<presence{} from="{ROOM}@{}/Bravo:{B}:{resource}" to="x"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="{}" jid="{B}@{DOMAIN}/{resource}"/></x></presence>"#,
            if unavailable { r#" type="unavailable""# } else { "" },
            muc(),
            if unavailable { "none" } else { "participant" }
        )
    };
    let line = format!(
        r#"<message type="groupchat" from="{ROOM}@{}/Bravo:{B}:{res2}" to="x"><body>back</body></message>"#,
        muc()
    );
    let joined = || {
        let mut alpha = model(A, DEFAULT_RESOURCE);
        alpha.join_public_room(ROOM, "Alpha");
        let presence = format!(
            r#"<presence from="{ROOM}@{}/{}" to="x"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant" jid="{A}@{DOMAIN}/{}"/><status code="110"/></x></presence>"#,
            muc(),
            alpha.nickname("Alpha"),
            alpha.resource
        );
        alpha.receive(&presence);
        alpha.receive(&occupant(res1, false));
        alpha
    };

    // The new connection joins while the old one is still in the room, which leaves later
    let mut late = joined();
    late.receive_all(&[occupant(res2, false), occupant(res1, true), line.clone()]);
    assert_eq!(late.shown_lines(lookup()).await, ["[unknown]: back"]);

    // The old one leaves first, then the new one joins
    let mut replaced = joined();
    replaced.receive_all(&[occupant(res1, true), occupant(res2, false), line]);
    assert_eq!(replaced.shown_lines(lookup()).await, ["Bravo: back"]);
    assert_eq!(replaced.room_of(ROOM).unwrap().members.len(), 2);
}

#[test]
fn the_clients_url_encoding_utf_8_unreserved_characters_kept_xx_in_upper_case() {
    assert_eq!(url_encode("Alpha_1"), "Alpha_1");
    assert_eq!(url_encode("Söme Name:1"), "S%C3%B6me%20Name%3A1");
    assert_eq!(url_decode("S%c3%b6me%20Name%3A1"), "Söme Name:1");
}

// ---- Our chat server, through the same model ----

/// A player: the model's own stanzas go over a real connection, and every reply is fed back to it
struct Player {
    wire: WireClient,
    model: ChatClientModel,
}

struct Ours {
    app: TestApp,
    chat: ChatServer,
}

async fn ours() -> Ours {
    let app = TestApp::start().await;
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, 'Alpha', 0, 0), (?2, 'Bravo', 0, 0)",
        &[&A, &B],
    );
    let mut options = options();
    options.auto_tick = false;
    let chat = ChatServer::start(&app.state, options).await;
    Ours { app, chat }
}

impl Ours {
    async fn player(&self, uid: &str) -> Player {
        let wire = login(&self.app.state, self.chat.port, uid).await;
        let model = model(uid, &wire.resource);
        Player { wire, model }
    }

    /// Each test logs its players out, so sessions never pile up
    async fn finish(self, players: Vec<Player>) {
        for player in players {
            player.wire.logout().await;
        }
        self.chat.stop().await;
    }
}

async fn settle(players: &mut [&mut Player]) {
    for player in players.iter_mut() {
        let frames = player.wire.barrier().await;
        player.model.receive_all(&frames);
    }
}

fn send(wire: &WireClient, stanza: Option<String>) {
    wire.send(&stanza.expect("the model sent something"));
}

#[tokio::test(flavor = "multi_thread")]
async fn m6_both_players_see_usernames_a_leave_completes_and_a_rejoin_is_accepted() {
    let o = ours().await;
    let mut alpha = o.player(A).await;
    let mut bravo = o.player(B).await;

    send(&alpha.wire, alpha.model.join_public_room(ROOM, "Alpha"));
    settle(&mut [&mut alpha]).await;
    send(&bravo.wire, bravo.model.join_public_room(ROOM, "Bravo"));
    settle(&mut [&mut bravo, &mut alpha]).await;
    assert_eq!(alpha.model.room_of(ROOM).unwrap().state, JOINED);
    assert_eq!(bravo.model.room_of(ROOM).unwrap().state, JOINED);

    send(
        &alpha.wire,
        Some(
            alpha
                .model
                .room_message(ROOM, "Ready?", "3F2504E04F8911D39A0C0305E82C3301"),
        ),
    );
    settle(&mut [&mut alpha, &mut bravo]).await;
    send(
        &bravo.wire,
        Some(
            bravo
                .model
                .room_message(ROOM, "Always ©", "3F2504E04F8911D39A0C0305E82C3302"),
        ),
    );
    settle(&mut [&mut bravo, &mut alpha]).await;

    assert_eq!(
        alpha.model.shown_lines(lookup()).await,
        ["Alpha: Ready?", "Bravo: Always ©"]
    );
    assert_eq!(
        bravo.model.shown_lines(lookup()).await,
        ["Alpha: Ready?", "Bravo: Always ©"]
    );

    send(&bravo.wire, bravo.model.exit_room(ROOM));
    assert_eq!(bravo.model.room_of(ROOM).unwrap().state, EXIT_PENDING);
    settle(&mut [&mut bravo, &mut alpha]).await;
    assert_eq!(
        bravo.model.room_of(ROOM).unwrap().state,
        NOT_JOINED,
        "the leave completes"
    );
    assert_eq!(bravo.model.exit_results, [ROOM]);
    assert_eq!(
        alpha.model.room_of(ROOM).unwrap().members.len(),
        1,
        "Alpha saw Bravo leave"
    );

    send(&bravo.wire, bravo.model.join_public_room(ROOM, "Bravo"));
    settle(&mut [&mut bravo, &mut alpha]).await;
    assert_eq!(
        bravo.model.room_of(ROOM).unwrap().state,
        JOINED,
        "the rejoin is accepted"
    );
    assert!(!bravo.model.log.iter().any(|line| line.contains("already pending")));
    o.finish(vec![alpha, bravo]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn m7_a_refused_join_drops_the_room_cleanly_and_the_next_join_is_allowed() {
    let o = ours().await;
    let mut alpha = o.player(A).await;

    // a name that is not this account's
    send(&alpha.wire, alpha.model.join_public_room(ROOM, "Bravo"));
    settle(&mut [&mut alpha]).await;
    assert!(alpha.model.room_of(ROOM).is_none(), "the room is gone");
    assert_eq!(
        alpha.model.join_results,
        [JoinResult {
            room: ROOM.into(),
            ok: false
        }]
    );

    send(&alpha.wire, alpha.model.join_public_room(ROOM, "Alpha"));
    settle(&mut [&mut alpha]).await;
    assert_eq!(alpha.model.room_of(ROOM).unwrap().state, JOINED);
    assert!(!alpha.model.log.iter().any(|line| line.contains("already pending")));
    o.finish(vec![alpha]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn m8_a_player_whose_own_account_read_failed_invalidmcpuser_is_still_shown_by_username_to_others() {
    let o = ours().await;
    let mut alpha = o.player(A).await;
    let mut bravo = o.player(B).await;
    let room = "Hunt-7c3e0a52-1111-4a4a-9c9c-000000000001";

    send(&alpha.wire, alpha.model.join_public_room(room, "InvalidMCPUser"));
    settle(&mut [&mut alpha]).await;
    send(&bravo.wire, bravo.model.join_public_room(room, "Bravo"));
    settle(&mut [&mut bravo, &mut alpha]).await;
    send(&alpha.wire, Some(alpha.model.room_message(room, "hello", "g1")));
    settle(&mut [&mut alpha, &mut bravo]).await;

    assert_eq!(bravo.model.shown_lines(lookup()).await, ["Alpha: hello"]);
    o.finish(vec![alpha, bravo]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn whispers_reach_the_other_player_with_a_full_sender_jid_shown_by_username() {
    let o = ours().await;
    let mut alpha = o.player(A).await;
    let mut bravo = o.player(B).await;

    send(&alpha.wire, Some(alpha.model.whisper_to(B, "psst")));
    settle(&mut [&mut alpha, &mut bravo]).await;
    assert_eq!(bravo.model.shown_whispers(lookup()), ["Alpha: psst"]);
    o.finish(vec![alpha, bravo]).await;
}
