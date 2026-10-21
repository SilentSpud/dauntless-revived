//! `presence.test.ts`: friends' online status (CHAT_PRESENCE=1; docs/findings/chat.md). Real chat
//! servers on port 0 with a test clock, real friendships in the test database, and raw clients shaped
//! like the 1.4.4 client. Node shared one server across the file and cleaned up after each test; here
//! each test has its own app and server. Every server checks every stanza it sent at `stop`: never one
//! from the receiver's own account.
//!
//! "the last guard" (a broken host filing the player's own session under a friend) cannot be built
//! from outside the chat; it is a unit test in `src/chat/presence.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use metagame::chat::presence::{FRIEND_MESSAGE_TYPE, PRESENCE_BURST, PRESENCE_REFILL_MS};
use metagame::chat::xmpp::{Element, escape, parse_frame};
use metagame::chat::{FriendsLookup, Settings};
use metagame::friends::{self, Outcome};
use serde_json::Value;
use service_kit::env::Env;

use crate::chatwire::{ChatServer, DOMAIN, TestClock, WireClient, is_self_stanza, login, options_at};
use crate::common::{TestApp, logged_lines};

const A: &str = "UID-presence-a";
const B: &str = "UID-presence-b";
const C: &str = "UID-presence-c";
const D: &str = "UID-presence-d";
const E: &str = "UID-presence-e";
const REFILL: i64 = PRESENCE_REFILL_MS as i64;
const BURST: usize = PRESENCE_BURST as usize;

/// The client's `<status>`: JSON written by 0x140951da0, with characters that need escaping
fn status_json(text: &str) -> String {
    serde_json::json!({
        "Status": text, "bIsPlaying": false, "bIsJoinable": false, "bHasVoiceSupport": false, "SessionId": "",
        "Properties": {"RichPresence_s": "InTheCity", "PartyPlayerCountData_i": 1, "Note_s": "<&'\">"}
    })
    .to_string()
}

fn city() -> String {
    status_json("Dauntless - In the city")
}

fn hunt() -> String {
    status_json("Dauntless - In a hunt")
}

/// A broadcast presence as FXmppPresenceStrophe::UpdatePresence builds it: no "to", no type
fn broadcast(client: &WireClient, status: &str, show: Option<&str>) {
    client.send(&format!(
        r#"<presence>{}<status>{}</status><delay xmlns="urn:xmpp:delay" stamp="2026-09-23T12:00:00.000Z"/></presence>"#,
        show.map(|s| format!("<show>{s}</show>")).unwrap_or_default(),
        escape(status)
    ));
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Seen {
    kind: String,
    from: String,
    to: String,
    kind_type: Option<String>,
    show: Option<String>,
    status: Option<String>,
    body: Option<String>,
}

/// The stanzas outside rooms among these frames, parsed
fn outside_rooms(frames: &[String]) -> Vec<Seen> {
    let text = |node: &Element, name: &str| node.child(name).map(Element::text);
    frames
        .iter()
        .filter_map(|frame| parse_frame(frame))
        .filter(|node| {
            (node.name == "presence" || node.name == "message")
                && !node.attr("from").unwrap_or_default().contains("@muc.")
        })
        .map(|node| Seen {
            kind: node.name.clone(),
            from: node.attr("from").unwrap_or_default().to_owned(),
            to: node.attr("to").unwrap_or_default().to_owned(),
            kind_type: node.attr("type").map(str::to_owned),
            show: text(&node, "show"),
            status: text(&node, "status"),
            body: text(&node, "body"),
        })
        .collect()
}

async fn frames(client: &WireClient) -> Vec<Seen> {
    outside_rooms(&client.barrier().await)
}

fn jid(client: &WireClient) -> String {
    format!("{}@{}/{}", client.uid, client.domain, client.resource)
}

fn presence_from(client: &WireClient, to: &WireClient, show: Option<&str>, status: &str) -> Seen {
    Seen {
        kind: "presence".into(),
        from: jid(client),
        to: jid(to),
        show: show.map(str::to_owned),
        status: Some(status.to_owned()),
        ..Seen::default()
    }
}

fn froms(seen: &[Seen]) -> Vec<String> {
    seen.iter().map(|s| s.from.clone()).collect()
}

fn from_and_type(seen: &[Seen]) -> Vec<(String, Option<String>)> {
    seen.iter().map(|s| (s.from.clone(), s.kind_type.clone())).collect()
}

struct Pr {
    app: TestApp,
    clock: TestClock,
    chat: ChatServer,
}

async fn pr() -> Pr {
    let app = TestApp::start().await;
    for (uid, name) in [(A, "Alpha"), (B, "Bravo"), (C, "Charlie"), (D, "Delta"), (E, "Echo")] {
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, 0)",
            &[&uid, &name],
        );
    }
    let clock = TestClock::at("2026-09-23T12:00:00Z");
    let mut options = options_at(&clock);
    options.presence = true;
    let chat = ChatServer::start(&app.state, options).await;
    Pr { app, clock, chat }
}

/// A test's own lookup of friends and blocks (`Friends`)
struct Lookup<F, G>(F, G);

impl<F, G> FriendsLookup for Lookup<F, G>
where
    F: Fn(&str) -> Vec<String> + Send + Sync,
    G: Fn(&str, &str) -> bool + Send + Sync,
{
    fn friends_of(&self, uid: &str) -> Vec<String> {
        (self.0)(uid)
    }
    fn blocked_either_way(&self, a: &str, b: &str) -> bool {
        (self.1)(a, b)
    }
}

impl Pr {
    async fn online(&self, uid: &str) -> WireClient {
        login(&self.app.state, self.chat.port, uid).await
    }

    /// Another server for the same app, with a test's own friends lookup
    async fn own(&self, lookup: impl FriendsLookup + 'static) -> ChatServer {
        let mut options = options_at(&self.clock);
        options.presence = true;
        options.friends = Some(Arc::new(lookup));
        ChatServer::start(&self.app.state, options).await
    }

    async fn request(&self, me: &str, them: &str) -> Outcome {
        let (state, me, them) = (self.app.state.clone(), me.to_owned(), them.to_owned());
        let (outcome, event) = self
            .app
            .db(move |conn| friends::send_or_accept(conn, &state.friend_requests, &me, &them))
            .await;
        friends::notify(&self.app.state, event);
        outcome
    }

    async fn befriend(&self, x: &str, y: &str) {
        assert!(matches!(self.request(x, y).await, Outcome::Done(_)));
        assert_eq!(self.request(y, x).await, Outcome::Done("accepted"));
    }

    async fn unfriend(&self, x: &str, y: &str) {
        let (x, y) = (x.to_owned(), y.to_owned());
        let (_, event) = self.app.db(move |conn| friends::remove(conn, &x, &y)).await;
        friends::notify(&self.app.state, event);
    }

    async fn block(&self, me: &str, them: &str) {
        let (me, them) = (me.to_owned(), them.to_owned());
        let (outcome, event) = self.app.db(move |conn| friends::block(conn, &me, &them)).await;
        assert_eq!(outcome, Outcome::Done("blocked"));
        friends::notify(&self.app.state, event);
    }

    async fn finish(self, clients: Vec<WireClient>) {
        for client in clients {
            client.logout().await;
        }
        self.chat.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_friends_exchange_presence_show_and_status_as_sent_from_the_full_jid_the_late_one_hears_of_the_early_one() {
    let p = pr().await;
    p.befriend(A, B).await;

    let alice = p.online(A).await;
    broadcast(&alice, &city(), None);
    assert_eq!(
        frames(&alice).await,
        [],
        "no friend is online yet, and nothing comes back to the sender"
    );

    let bob = p.online(B).await;
    broadcast(&bob, &hunt(), Some("away"));
    assert_eq!(
        frames(&bob).await,
        [presence_from(&alice, &bob, None, &city())],
        "Bob hears of Alice at his first presence"
    );
    assert_eq!(
        frames(&alice).await,
        [presence_from(&bob, &alice, Some("away"), &hunt())],
        "Alice hears of Bob, <show> included"
    );
    assert!(
        jid(&alice).contains("/V2:"),
        "the from carries the resource the game bound"
    );
    p.finish(vec![alice, bob]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn b_never_a_stanza_from_the_players_own_account_a_second_session_hears_nothing_from_the_first_and_a_friend_hears_both()
 {
    let p = pr().await;
    p.befriend(A, B).await;

    let first = p.online(A).await;
    let second = p.online(A).await;
    let bob = p.online(B).await;

    broadcast(&bob, &city(), None);
    frames(&bob).await;
    broadcast(&first, &city(), None);
    assert_eq!(
        froms(&frames(&first).await),
        [jid(&bob)],
        "only Bob's presence, never its own"
    );
    assert_eq!(
        frames(&second).await,
        [],
        "the second session of the same account hears nothing from the first"
    );

    broadcast(&second, &hunt(), None);
    assert_eq!(froms(&frames(&second).await), [jid(&bob)]);
    assert_eq!(frames(&first).await, [], "nor the first from the second");
    let seen = frames(&bob).await;
    assert_eq!(
        seen.iter()
            .map(|s| (s.from.clone(), s.status.clone()))
            .collect::<Vec<_>>(),
        [(jid(&first), Some(city())), (jid(&second), Some(hunt()))]
    );

    // The session ends: the friend sees it go, then the other session's presence again
    first.logout().await;
    let seen = frames(&bob).await;
    assert_eq!(
        seen.iter()
            .map(|s| (
                s.from.clone(),
                s.kind_type.clone().unwrap_or("available".into()),
                s.status.clone()
            ))
            .collect::<Vec<_>>(),
        [
            (jid(&first), "unavailable".into(), None),
            (jid(&second), "available".into(), Some(hunt()))
        ]
    );
    assert_eq!(
        frames(&second).await,
        [],
        "the remaining session is not told about its twin"
    );
    p.finish(vec![second, bob]).await;
}

#[test]
fn b_the_check_itself_a_stanza_from_the_receivers_own_account_is_caught_room_stanzas_and_server_stanzas_are_not() {
    let a = Some(A);
    assert!(is_self_stanza(
        a,
        &format!(r#"<presence from="{A}@{DOMAIN}/V2:x" to="{A}@{DOMAIN}/V2:y"/>"#)
    ));
    assert!(
        is_self_stanza(
            a,
            &format!(r#"<presence from="{}@{DOMAIN}/r" to="x"/>"#, A.to_lowercase())
        ),
        "case does not matter to the client"
    );
    assert!(
        is_self_stanza(a, &format!(r#"<message from="{A}@{DOMAIN}"><body>x</body></message>"#)),
        "a bare JID too"
    );
    assert!(
        !is_self_stanza(
            a,
            &format!(r#"<presence from="City-1@muc.{DOMAIN}/Alpha:{A}:r" to="{A}@{DOMAIN}/r"/>"#)
        ),
        "a room presence names the player in its nickname"
    );
    assert!(!is_self_stanza(
        a,
        &format!(r#"<iq type="get" id="sp1" from="{DOMAIN}" to="{A}@{DOMAIN}/r"><ping xmlns="urn:xmpp:ping"/></iq>"#)
    ));
    assert!(
        !is_self_stanza(
            a,
            &format!(
                r#"<presence from="{B}@{DOMAIN}/r" to="{A}@{DOMAIN}/r"><status>from="{A}@{DOMAIN}/r"</status></presence>"#
            )
        ),
        "only the stanza's own from counts"
    );
    assert!(!is_self_stanza(
        a,
        &format!(r#"<message from="xmpp-admin@{DOMAIN}" to="{A}@{DOMAIN}/r"><body/></message>"#)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn b_a_friends_lookup_that_names_the_player_itself_sends_it_nothing() {
    let p = pr().await;
    let own = p
        .own(Lookup(
            |uid: &str| vec![uid.to_owned(), uid.to_lowercase()],
            |_: &str, _: &str| false,
        ))
        .await;
    let first = login(&p.app.state, own.port, C).await;
    let second = login(&p.app.state, own.port, C).await;

    broadcast(&second, &city(), None);
    frames(&second).await;
    broadcast(&first, &hunt(), None);
    assert_eq!(frames(&first).await, []);
    assert_eq!(frames(&second).await, []);
    first.logout().await;
    assert_eq!(frames(&second).await, []);
    second.logout().await;
    own.stop().await;
    p.finish(vec![]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn c_unavailable_from_the_full_jid_when_a_session_closes_drops_pings_out_or_says_so() {
    let p = pr().await;
    p.befriend(A, B).await;

    let bob = p.online(B).await;
    broadcast(&bob, &city(), None);
    frames(&bob).await;

    let unavailable = |alice: &WireClient, bob: &WireClient| Seen {
        kind: "presence".into(),
        from: jid(alice),
        to: jid(bob),
        kind_type: Some("unavailable".into()),
        ..Seen::default()
    };

    for label in [
        "<close/>",
        "a dropped socket",
        "an unavailable broadcast",
        "a ping timeout",
    ] {
        let alice = p.online(A).await;
        broadcast(&alice, &city(), None);
        frames(&alice).await;
        assert_eq!(
            from_and_type(&frames(&bob).await),
            [(jid(&alice), None)],
            "{label}: online first"
        );

        match label {
            "<close/>" => alice.logout().await,
            "a dropped socket" => {
                let closed = regex::Regex::new(&format!(r"^info chat: closed c=\d+ uid={A} reason=socket ")).unwrap();
                let count = || logged_lines().iter().filter(|line| closed.is_match(line)).count();
                let before = count();
                alice.close();
                for _ in 0..100 {
                    if count() > before {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                assert_eq!(count(), before + 1, "the server saw the socket go");
            }
            "an unavailable broadcast" => {
                alice.send(r#"<presence type="unavailable"/>"#);
                alice.barrier().await;
                assert!(alice.is_open(), "the session stays");
            }
            _ => {
                alice.auto_pong.store(false, Ordering::SeqCst);
                p.clock.advance(51 * 1000);
                p.chat.tick().await;
                p.clock.advance(101 * 1000);
                bob.send(r#"<iq type="get" id="keepalive"><ping xmlns="urn:xmpp:ping"/></iq>"#);
                let mut answer = bob.expect("keepalive").await;
                // Bob was silent too: his own ping came first, answered on its own
                while !answer.contains("keepalive") {
                    answer = bob.expect("keepalive").await;
                }
                p.chat.tick().await;
                alice.closed().await;
            }
        }

        assert_eq!(frames(&bob).await, [unavailable(&alice, &bob)], "{label}");
        if label == "an unavailable broadcast" {
            alice.logout().await;
            frames(&bob).await;
        }
    }
    p.finish(vec![bob]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn d_nobody_but_friends_hears_it_not_strangers_not_a_blocked_player_not_after_an_unfriend() {
    let p = pr().await;
    p.befriend(A, B).await;

    let alice = p.online(A).await;
    let bob = p.online(B).await;
    let carl = p.online(C).await;
    broadcast(&carl, &city(), None);
    broadcast(&bob, &city(), None);
    frames(&carl).await;
    frames(&bob).await;
    broadcast(&alice, &city(), None);
    frames(&alice).await;
    assert_eq!(frames(&bob).await.len(), 1, "the friend hears it");
    assert_eq!(frames(&carl).await, [], "a stranger does not");

    // A block ends the friendship: each side sees the other go once, then nothing more
    p.block(B, A).await;
    assert_eq!(
        from_and_type(&frames(&bob).await),
        [(jid(&alice), Some("unavailable".into()))]
    );
    assert_eq!(
        from_and_type(&frames(&alice).await),
        [(jid(&bob), Some("unavailable".into()))]
    );
    broadcast(&alice, &hunt(), None);
    frames(&alice).await;
    assert_eq!(frames(&bob).await, [], "a blocked player hears no more");

    // A lookup that still listed the pair as friends does not get past the block either
    let own = p
        .own(Lookup(
            |uid: &str| vec![if uid == D { E } else { D }.to_owned()],
            |_: &str, _: &str| true,
        ))
        .await;
    let dee = login(&p.app.state, own.port, D).await;
    let eve = login(&p.app.state, own.port, E).await;
    broadcast(&eve, &city(), None);
    broadcast(&dee, &city(), None);
    assert_eq!(frames(&dee).await, []);
    assert_eq!(frames(&eve).await, []);
    dee.logout().await;
    assert_eq!(frames(&eve).await, []);
    eve.logout().await;
    own.stop().await;

    // An unfriend: the same
    p.app.run("DELETE FROM blocks", &[]);
    p.befriend(A, C).await;
    let seen = frames(&carl).await;
    assert_eq!(
        seen.iter()
            .map(|s| (s.kind.clone(), s.from.clone()))
            .collect::<Vec<_>>(),
        [
            ("message".to_owned(), format!("xmpp-admin@{DOMAIN}")),
            ("presence".to_owned(), jid(&alice))
        ]
    );
    frames(&alice).await;
    broadcast(&alice, &city(), None);
    frames(&alice).await;
    assert_eq!(frames(&carl).await.len(), 1, "a new friend hears the next change");
    p.unfriend(C, A).await;
    assert_eq!(
        from_and_type(&frames(&carl).await),
        [(jid(&alice), Some("unavailable".into()))]
    );
    broadcast(&alice, &hunt(), None);
    frames(&alice).await;
    assert_eq!(frames(&carl).await, []);
    p.finish(vec![alice, bob, carl]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn e_an_accepted_friend_request_pushes_the_friends_list_entry_from_xmpp_admin_then_the_two_presences() {
    let p = pr().await;
    let alice = p.online(A).await;
    let carl = p.online(C).await;
    broadcast(&alice, &city(), Some("chat"));
    broadcast(&carl, &hunt(), None);
    frames(&alice).await;
    frames(&carl).await;

    assert_eq!(p.request(C, A).await, Outcome::Done("requested"));
    assert_eq!(frames(&alice).await, [], "a request alone pushes nothing");

    assert_eq!(p.request(A, C).await, Outcome::Done("accepted"));

    for (me, other, direction) in [(&alice, &carl, "INBOUND"), (&carl, &alice, "OUTBOUND")] {
        let seen = frames(me).await;
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(
            (
                seen[0].kind.as_str(),
                seen[0].from.clone(),
                seen[0].to.clone(),
                seen[0].kind_type.clone()
            ),
            ("message", format!("xmpp-admin@{DOMAIN}"), jid(me), None),
            "a plain message from xmpp-admin at the session's own domain"
        );

        let body: Value = serde_json::from_str(seen[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(
            body.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["type", "payload", "timestamp"]
        );
        assert_eq!(body["type"], FRIEND_MESSAGE_TYPE);
        let mut payload = body["payload"].clone();
        let created = payload.as_object_mut().unwrap().remove("created").unwrap();
        assert_eq!(
            payload,
            serde_json::json!({"accountId": other.uid, "status": "ACCEPTED", "direction": direction})
        );
        assert!(metagame::js::date_parse(created.as_str().unwrap()).is_some());
        assert!(metagame::js::date_parse(body["timestamp"].as_str().unwrap()).is_some());

        assert_eq!(
            (seen[1].kind.as_str(), seen[1].from.clone(), seen[1].kind_type.clone()),
            ("presence", jid(other), None),
            "then the new friend's presence"
        );
    }
    p.finish(vec![alice, carl]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn f_a_change_is_relayed_the_same_presence_again_is_not_a_flood_is_held_back_and_its_latest_goes_out_at_the_next_tick()
 {
    let p = pr().await;
    p.befriend(A, B).await;

    let alice = p.online(A).await;
    let bob = p.online(B).await;
    broadcast(&bob, &city(), None);
    frames(&bob).await;
    broadcast(&alice, &city(), None);
    frames(&alice).await;
    frames(&bob).await;

    broadcast(&alice, &hunt(), None);
    broadcast(&alice, &hunt(), None);
    frames(&alice).await;
    let statuses = |seen: Vec<Seen>| seen.into_iter().map(|s| s.status.unwrap()).collect::<Vec<_>>();
    assert_eq!(statuses(frames(&bob).await), [hunt()], "one change, one relay");

    // The first presence and HUNT took two of PRESENCE_BURST tokens
    let texts: Vec<String> = (0..BURST + 2)
        .map(|index| status_json(&format!("Dauntless - In events {index}")))
        .collect();
    for text in &texts {
        broadcast(&alice, text, None);
    }

    frames(&alice).await;
    assert_eq!(
        statuses(frames(&bob).await),
        texts[..BURST - 2],
        "held back after the burst"
    );

    p.clock.advance(REFILL);
    p.chat.tick().await;
    assert_eq!(
        statuses(frames(&bob).await),
        [texts.last().unwrap().clone()],
        "the latest one, once"
    );
    p.chat.tick().await;
    assert_eq!(frames(&bob).await, []);
    p.finish(vec![alice, bob]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn f_going_offline_and_online_in_a_loop_is_held_back_like_any_other_change_and_the_friends_lookups_stay_few() {
    let p = pr().await;
    let lookups = Arc::new(AtomicUsize::new(0));
    let block_checks = Arc::new(AtomicUsize::new(0));
    let (counted, checked) = (lookups.clone(), block_checks.clone());
    let own = p
        .own(Lookup(
            move |uid: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
                match uid {
                    D => vec![E.to_owned()],
                    E => vec![D.to_owned()],
                    _ => Vec::new(),
                }
            },
            move |_: &str, _: &str| {
                checked.fetch_add(1, Ordering::SeqCst);
                false
            },
        ))
        .await;

    let eve = login(&p.app.state, own.port, E).await;
    let dee = login(&p.app.state, own.port, D).await;
    broadcast(&eve, &city(), None);
    frames(&eve).await;
    broadcast(&dee, &city(), None);
    frames(&dee).await;
    assert_eq!(frames(&eve).await.len(), 1, "Eve hears Dee come online");
    lookups.store(0, Ordering::SeqCst);
    block_checks.store(0, Ordering::SeqCst);

    // The first presence took one of PRESENCE_BURST tokens; each unavailable and each return takes one
    for _ in 0..BURST + 2 {
        dee.send(r#"<presence type="unavailable"/>"#);
        broadcast(&dee, &city(), None);
    }

    frames(&dee).await;
    let heard = frames(&eve).await;
    assert!(heard.len() <= BURST, "{} stanzas", heard.len());
    assert_eq!(
        heard
            .iter()
            .map(|s| s.kind_type.clone().unwrap_or("available".into()))
            .collect::<Vec<_>>(),
        ["unavailable", "available", "unavailable", "available"],
        "two rounds, then held back"
    );
    let (looked, blocks) = (lookups.load(Ordering::SeqCst), block_checks.load(Ordering::SeqCst));
    assert!(
        looked < BURST && blocks < BURST,
        "{looked} lookups, {blocks} block checks"
    );

    // Only the latest state goes out when its turn comes. Dee ended the loop online as Eve last saw
    // it, so Eve is told nothing; Dee, deaf for a while, hears of Eve again.
    p.clock.advance(REFILL);
    own.tick().await;
    assert_eq!(frames(&eve).await, []);
    assert_eq!(froms(&frames(&dee).await), [jid(&eve)]);

    dee.send(r#"<presence type="unavailable"/>"#);
    broadcast(&dee, &hunt(), None);
    frames(&dee).await;
    assert_eq!(frames(&eve).await, [], "no token left");
    p.clock.advance(REFILL);
    own.tick().await;
    let seen = frames(&eve).await;
    assert_eq!(
        seen.iter()
            .map(|s| (s.from.clone(), s.kind_type.clone(), s.status.clone()))
            .collect::<Vec<_>>(),
        [(jid(&dee), None, Some(hunt()))],
        "the latest one, once"
    );
    own.tick().await;
    assert_eq!(frames(&eve).await, []);

    // Ending the session is never held back
    dee.logout().await;
    assert_eq!(
        from_and_type(&frames(&eve).await),
        [(jid(&dee), Some("unavailable".into()))]
    );
    eve.logout().await;
    own.stop().await;
    p.finish(vec![]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn g_chat_presence_off_not_one_presence_or_friends_list_stanza_outside_rooms() {
    let p = pr().await;
    let mut options = options_at(&p.clock);
    options.presence = false;
    let off = ChatServer::start(&p.app.state, options).await;

    let alice = login(&p.app.state, off.port, A).await;
    let bob = login(&p.app.state, off.port, B).await;

    broadcast(&alice, &city(), None);
    broadcast(&bob, &hunt(), None);
    p.befriend(A, B).await;
    broadcast(&alice, &hunt(), None);
    alice.send(r#"<presence type="unavailable"/>"#);
    alice.barrier().await;
    alice.logout().await;
    off.tick().await;
    bob.barrier().await;

    assert_eq!(outside_rooms(&bob.frames.lock().unwrap()), []);
    assert_eq!(outside_rooms(&alice.frames.lock().unwrap()), []);
    assert!(
        logged_lines().contains(&"info chat: friends' online status off: no presence is sent outside rooms".to_owned())
    );
    bob.logout().await;
    off.stop().await;
    p.finish(vec![]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_default_friends_automatically_see_login_and_logout_without_a_presence_setting() {
    let p = pr().await;
    let mut options = options_at(&p.clock);
    options.presence = Settings::from_env(&Env::from_pairs(std::iter::empty())).presence;
    let on = ChatServer::start(&p.app.state, options).await;
    p.befriend(A, B).await;
    let alice = login(&p.app.state, on.port, A).await;
    let bob = login(&p.app.state, on.port, B).await;
    broadcast(&alice, &city(), None);
    alice.barrier().await;
    broadcast(&bob, &hunt(), None);
    let alice_jid = jid(&alice);
    assert!(
        frames(&bob)
            .await
            .iter()
            .any(|f| f.kind == "presence" && f.from == alice_jid && f.status == Some(city()))
    );
    alice.logout().await;
    assert!(
        frames(&bob)
            .await
            .iter()
            .any(|f| f.kind == "presence" && f.from == alice_jid && f.kind_type.as_deref() == Some("unavailable"))
    );
    bob.logout().await;
    assert!(
        logged_lines()
            .iter()
            .any(|line| line.starts_with("info chat: friends' online status on (CHAT_PRESENCE=1)"))
    );
    on.stop().await;
    p.finish(vec![]).await;

    let warnings = |pairs: &[(&str, &str)]| Settings::from_env(&Env::from_pairs(pairs.iter().copied())).warnings;
    let off = ["CHAT_PRESENCE is on but chat is off (CHAT=1 is needed); nobody shows as online"];
    assert_eq!(warnings(&[("CHAT", "0"), ("CHAT_PRESENCE", "1")]), off);
    assert_eq!(warnings(&[("CHAT", "0")]), off);
    assert!(warnings(&[("CHAT_PRESENCE", "1")]).is_empty());
    assert!(warnings(&[("CHAT", "1"), ("CHAT_PRESENCE", "1")]).is_empty());
    assert!(warnings(&[("CHAT_PRESENCE", "0")]).is_empty());
}

/// Node checked every stanza of its file at the end; each server here checks its own at `stop`.
#[tokio::test(flavor = "multi_thread")]
async fn every_stanza_of_this_file_kept_the_rule() {
    let p = pr().await;
    p.befriend(A, B).await;
    let alice = p.online(A).await;
    let second = p.online(A).await;
    let bob = p.online(B).await;
    for client in [&alice, &second, &bob] {
        broadcast(client, &city(), None);
        frames(client).await;
    }
    for client in [&alice, &second, &bob] {
        frames(client).await;
    }
    assert!(p.chat.checked() > 5, "{}", p.chat.checked());
    p.finish(vec![alice, second, bob]).await;
}
