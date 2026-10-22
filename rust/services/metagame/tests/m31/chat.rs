//! `chat.test.ts`: the chat listener (docs/findings/chat.md): handshake, login, liveness, crash guard,
//! public mode, settings and start, rooms and names, and logs. Node ran most cases on one shared server
//! with a test clock and one log capture; here each test has its own app, server and clock, and log
//! checks read every line logged in the process (lines that name the test's own accounts or rooms).

use std::sync::atomic::Ordering;
use std::time::Duration;

use metagame::chat::nick::url_encode_like_client;
use metagame::chat::xmpp::{escape, parse_frame};
use metagame::chat::{NickCheck, Options, Settings};
use metagame::friends::{self, Outcome};
use regex::Regex;
use serde_json::{Value, json};
use service_kit::env::Env;

use crate::chatclient::{ChatClientModel, JOINED, NOT_JOINED, lookup_from};
use crate::chatwire::{
    ChatServer, DOMAIN, FRAMING, LoginOptions, TestClock, WireClient, base64_plain, client_frame, game_resource, login,
    options, options_at, raw_closed, raw_upgrade, token, try_login,
};
use crate::common::{TestApp, logged_lines};

const A: &str = "UID-chat-a";
const B: &str = "UID-chat-b";
const C: &str = "UID-chat-c";
const D: &str = "UID-chat-d";
/// Signed tokens for it exist, the account does not
const GONE: &str = "UID-chat-gone";
const OLD: &str = "UID-chat-old";
/// An older name outside today's username rules
const OLD_NAME: &str = "Sölve Ö";
const LOOP: &str = "UID-chat-loop";
const FLOOD: &str = "UID-chat-flood";
const JOINS: &str = "UID-chat-joins";

fn muc_domain() -> String {
    format!("muc.{DOMAIN}")
}

fn logged(pattern: &str) -> bool {
    let pattern = Regex::new(pattern).unwrap();
    logged_lines().iter().any(|line| pattern.is_match(line))
}

fn logged_line(line: &str) -> bool {
    logged_lines().iter().any(|l| l == line)
}

// ---- The prototype's two tests, on a server with its own timer and the real clock ----

async fn experimental() -> (TestApp, ChatServer) {
    let app = TestApp::start().await;
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, 'Alpha', 0, 0), (?2, 'Bravo', 0, 0)",
        &[&A, &B],
    );
    let chat = ChatServer::start(&app.state, options()).await;
    (app, chat)
}

async fn game_login(app: &TestApp, chat: &ChatServer, uid: &str) -> WireClient {
    let options = LoginOptions {
        resource: Some("game".into()),
        ..LoginOptions::default()
    };
    let client = try_login(&app.state, chat.port, uid, options).await.unwrap();
    assert_eq!(client.resource, "game");
    client
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_a_token_for_another_account() {
    let (app, chat) = experimental().await;
    let socket = WireClient::connect(chat.port, &[]).await.unwrap();
    socket.send(&format!(r#"<open xmlns="{FRAMING}"/>"#));
    socket.expect("open").await;
    socket.expect("features").await;
    socket.send(&format!(
        r#"<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="PLAIN">{}</auth>"#,
        base64_plain("", A, &token(&app.state, B).await)
    ));
    assert!(socket.expect("failure").await.contains("not-authorized"));
    socket.close();
    chat.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn routes_a_direct_message_and_a_shared_room_message_between_two_authenticated_accounts() {
    let (app, chat) = experimental().await;
    let a = game_login(&app, &chat, A).await;
    let b = game_login(&app, &chat, B).await;
    a.send(&format!(
        r#"<message to="{B}@prod.ol.epicgames.com" type="chat" id="m1"><body>Hello &amp; welcome</body></message>"#
    ));
    assert!(b.expect("direct message").await.contains("Hello &amp; welcome"));
    // Names observed in the 1.4.4 client log, not a fabricated generic room. Rooms live on
    // muc.<domain>, and the client joins with <name>:<account id>:<resource>.
    let city = "City-d147475b-7742-4e7b-8142-6c0f55dda06b@muc.prod.ol.epicgames.com";
    let alpha = format!("Alpha:{A}:game");
    let bravo = format!("Bravo:{B}:game");
    a.send(&format!(
        r#"<presence to="{city}/{alpha}"><x xmlns="http://jabber.org/protocol/muc"/></presence>"#
    ));
    let joined_alpha = a.expect("join").await;
    assert!(joined_alpha.contains(r#"status code="110""#));
    assert!(
        joined_alpha.contains(&format!(r#"from="{city}/{alpha}""#)),
        "the nickname is kept as sent"
    );
    assert!(
        joined_alpha.contains(&format!(r#"jid="{A}@prod.ol.epicgames.com/game""#)),
        "and the occupant's own JID"
    );
    b.send(&format!(
        r#"<presence to="{city}/{bravo}"><x xmlns="http://jabber.org/protocol/muc"/></presence>"#
    ));
    let alpha_for_b = b.expect("join").await;
    assert!(
        alpha_for_b.contains(&format!(r#"from="{city}/{alpha}""#)) && !alpha_for_b.contains("110"),
        "B first hears of A"
    );
    let joined_bravo = b.expect("join").await;
    assert!(joined_bravo.contains(r#"status code="110""#));
    assert!(joined_bravo.contains(&format!(r#"from="{city}/{bravo}""#)));
    assert!(
        a.expect("presence")
            .await
            .contains(&format!(r#"from="{city}/{bravo}""#)),
        "A hears of B"
    );
    a.send(&format!(
        r#"<message to="{city}" type="groupchat" id="m2"><body>Ready?</body></message>"#
    ));
    let delivered = b.expect("room message").await;
    assert!(delivered.contains("Ready?"));
    assert!(delivered.contains(&format!(r#"from="{city}/{alpha}""#)));
    assert!(
        !delivered.contains("<nick "),
        "no XEP-0172 nickname: the client never reads it"
    );
    assert!(a.expect("room message").await.contains("Ready?"));
    a.logout().await;
    b.logout().await;
    chat.stop().await;
}

// ---- The listener itself, on a server with a test clock and no timer of its own ----

struct Lt {
    app: TestApp,
    clock: TestClock,
    chat: ChatServer,
}

async fn lt_on(app: TestApp, configure: impl FnOnce(&mut Options)) -> Lt {
    for (uid, name) in [
        (A, "Alpha"),
        (B, "Bravo"),
        (C, "Charlie"),
        (D, "Delta"),
        (OLD, OLD_NAME),
        (LOOP, "Loop"),
        (FLOOD, "Flood"),
        (JOINS, "Joins"),
    ] {
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, 0)",
            &[&uid, &name],
        );
    }
    let clock = TestClock::at("2026-09-22T12:00:00Z");
    let mut options = options_at(&clock);
    configure(&mut options);
    let chat = ChatServer::start(&app.state, options).await;
    Lt { app, clock, chat }
}

async fn lt_with(configure: impl FnOnce(&mut Options)) -> Lt {
    lt_on(TestApp::start().await, configure).await
}

async fn lt() -> Lt {
    lt_with(|_| {}).await
}

impl Lt {
    async fn connected(&self) -> WireClient {
        WireClient::connect(self.chat.port, &[]).await.unwrap()
    }

    async fn signed_in(&self, uid: &str) -> WireClient {
        login(&self.app.state, self.chat.port, uid).await
    }

    async fn signed_in_with(&self, uid: &str, options: LoginOptions) -> Result<WireClient, String> {
        try_login(&self.app.state, self.chat.port, uid, options).await
    }

    /// A login payload for `authc` with `signer`'s token
    async fn plain(&self, authc: &str, signer: &str) -> String {
        base64_plain("", authc, &token(&self.app.state, signer).await)
    }

    async fn tick(&self) {
        self.chat.tick().await;
    }

    fn advance(&self, ms: i64) {
        self.clock.advance(ms);
    }

    async fn stop(self) {
        self.chat.stop().await;
    }
}

/// Open and features, then the SASL answer to this payload
async fn sasl_answer(client: &WireClient, payload: &str, mechanism: &str) -> String {
    client.send(&format!(r#"<open xmlns="{FRAMING}" to="{DOMAIN}" version="1.0"/>"#));
    client.expect("open").await;
    client.expect("features").await;
    client.send(&format!(
        r#"<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="{mechanism}">{payload}</auth>"#
    ));
    client.expect("sasl answer").await
}

static WHISPERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

async fn whispered(from: &WireClient, to: &WireClient, marker: &str) -> bool {
    from.send(&format!(
        r#"<message to="{}@{DOMAIN}" type="chat" id="w{}"><body>{marker}</body></message>"#,
        to.uid,
        WHISPERS.fetch_add(1, Ordering::SeqCst)
    ));
    from.barrier().await;
    to.barrier().await.iter().any(|frame| frame.contains(marker))
}

const OPEN_ANSWER: &str = r#"^<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" from="prod\.ol\.epicgames\.com" id="[0-9a-f]{16}" version="1\.0" xml:lang="en"/>$"#;
const FAILURE: &str = r#"<failure xmlns="urn:ietf:params:xml:ns:xmpp-sasl"><not-authorized/></failure>"#;
const HELD: &str = r#"<failure xmlns="urn:ietf:params:xml:ns:xmpp-sasl"><temporary-auth-failure/></failure>"#;

// ---- handshake ----

#[tokio::test(flavor = "multi_thread")]
async fn answers_the_games_handshake_frame_by_frame_and_echoes_its_67_character_resource() {
    let t = lt().await;
    let client = t.connected().await;
    let resource = game_resource();
    let token = token(&t.app.state, C).await;
    let open = Regex::new(OPEN_ANSWER).unwrap();

    assert_eq!(resource.len(), 67);
    client.send(&format!(r#"<open xmlns="{FRAMING}" to="{DOMAIN}" version="1.0"/>"#));
    assert!(open.is_match(&client.expect("open").await));
    assert_eq!(
        client.expect("features").await,
        r#"<stream:features xmlns:stream="http://etherx.jabber.org/streams"><mechanisms xmlns="urn:ietf:params:xml:ns:xmpp-sasl"><mechanism>PLAIN</mechanism></mechanisms></stream:features>"#
    );
    client.send(&format!(
        r#"<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="PLAIN">{}</auth>"#,
        base64_plain("", C, &token)
    ));
    assert_eq!(
        client.expect("success").await,
        r#"<success xmlns="urn:ietf:params:xml:ns:xmpp-sasl"/>"#
    );
    client.send(&format!(r#"<open xmlns="{FRAMING}" to="{DOMAIN}" version="1.0"/>"#));
    assert!(open.is_match(&client.expect("open").await));
    assert_eq!(
        client.expect("bind features").await,
        r#"<stream:features xmlns:stream="http://etherx.jabber.org/streams"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"/></stream:features>"#
    );
    client.send(&format!(
        r#"<iq type="set" id="_xmpp_bind1"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><resource>{resource}</resource></bind></iq>"#
    ));
    assert_eq!(
        client.expect("bind").await,
        format!(
            r#"<iq xmlns="jabber:client" type="result" id="_xmpp_bind1"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><jid>{C}@{DOMAIN}/{resource}</jid></bind></iq>"#
        )
    );

    // Each server message is one complete stanza
    for frame in client.frames.lock().unwrap().iter() {
        assert!(parse_frame(frame).is_some(), "{frame}");
    }

    client.send(r#"<iq type="set" id="_xmpp_session1"><session xmlns="urn:ietf:params:xml:ns:xmpp-session"/></iq>"#);
    assert_eq!(
        client.expect("session").await,
        r#"<iq xmlns="jabber:client" type="result" id="_xmpp_session1"/>"#
    );
    client.send(r#"<iq type="get" id="v1"><query xmlns="jabber:iq:version"/></iq>"#);
    assert_eq!(
        client.expect("version").await,
        r#"<iq xmlns="jabber:client" type="result" id="v1"/>"#
    );
    let bound = format!("uid={C} resource={resource} domain={DOMAIN} sessions=1");
    assert!(
        logged_lines()
            .iter()
            .any(|line| line.contains("chat: bound c=") && line.contains(&bound))
    );
    client.logout().await;
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn takes_a_host_or_launcher_host_port_from_open_to_and_falls_back_when_it_is_missing_or_invalid() {
    let t = lt().await;
    for (open_to, domain) in [
        (Some("dauntless.local"), "dauntless.local"),
        (Some("127.0.0.1:61000"), "127.0.0.1:61000"),
        (None, DOMAIN),
        (Some("not a host!"), DOMAIN),
    ] {
        let options = LoginOptions {
            open_to: Some(open_to.map(str::to_owned)),
            ..LoginOptions::default()
        };
        let client = t.signed_in_with(D, options).await.unwrap();
        assert_eq!(client.domain, domain);
        client.logout().await;
    }
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn gives_an_empty_resource_a_server_one_and_refuses_an_over_long_one_or_one_with_control_characters() {
    let t = lt().await;
    let options = LoginOptions {
        resource: Some(String::new()),
        ..LoginOptions::default()
    };
    let empty = t.signed_in_with(D, options).await.unwrap();
    assert!(Regex::new("^srv-[0-9a-f]{32}$").unwrap().is_match(&empty.resource));

    for bad in ["x".repeat(257), "V2:\tTab".to_owned()] {
        let client = t.connected().await;
        assert!(
            sasl_answer(&client, &t.plain(D, D).await, "PLAIN")
                .await
                .contains("<success")
        );
        client.send(&format!(r#"<open xmlns="{FRAMING}" to="{DOMAIN}" version="1.0"/>"#));
        client.expect("open").await;
        client.expect("features").await;
        client.send(&format!(
            r#"<iq type="set" id="_xmpp_bind1"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><resource>{bad}</resource></bind></iq>"#
        ));
        assert_eq!(
            client.expect("bind").await,
            r#"<iq xmlns="jabber:client" type="error" id="_xmpp_bind1"><error type="modify"><bad-request xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error></iq>"#
        );
        assert!(client.is_open(), "a refused bind does not hang up");
        client.close();
    }
    empty.logout().await;
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_the_clients_close_with_its_own_and_hangs_up() {
    let t = lt().await;
    let client = t.signed_in(C).await;
    client.send(&format!(r#"<close xmlns="{FRAMING}"/>"#));
    assert_eq!(client.expect("close").await, format!(r#"<close xmlns="{FRAMING}"/>"#));
    assert_eq!(client.closed().await, 1000);
    assert!(logged(r"chat: closed c=\d+ uid=UID-chat-c reason=close after=\d+s"));
    t.stop().await;
}

// ---- login ----

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_token_for_another_account_then_the_legacy_login_the_game_tries_next_and_hangs_up() {
    let t = lt().await;
    let client = t.connected().await;
    assert_eq!(sasl_answer(&client, &t.plain(C, D).await, "PLAIN").await, FAILURE);
    assert!(client.is_open(), "the game gets to send its legacy login");
    client.send(&format!(
        r#"<iq type="set" id="_xmpp_auth1"><query xmlns="jabber:iq:auth"><username>{C}</username><password>not-a-real-password-marker</password><resource>r</resource></query></iq>"#
    ));
    assert_eq!(
        client.expect("legacy").await,
        r#"<iq xmlns="jabber:client" type="error" id="_xmpp_auth1"><error type="auth"><not-authorized xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error></iq>"#
    );
    client.closed().await;
    assert!(logged(r"chat: login refused c=\d+ reason=uid-mismatch uid=UID-chat-c$"));
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_an_expired_token_a_deleted_account_another_mechanism_and_a_malformed_payload() {
    let t = lt().await;
    let now = metagame::time::now_ms() as u64 / 1000;
    let expired = t
        .app
        .state
        .tokens
        .sign(C, 0, now - metagame::crypto::TOKEN_LIFETIME_SECS - 60)
        .unwrap();
    let own = token(&t.app.state, C).await;
    let cases = [
        ("expired", base64_plain("", C, &expired), "PLAIN", "not-authorized"),
        ("no-account", t.plain(GONE, GONE).await, "PLAIN", "not-authorized"),
        (
            "bad-token",
            base64_plain("", C, "not.a.token"),
            "PLAIN",
            "not-authorized",
        ),
        (
            "bad-mechanism",
            base64_plain("", C, &own),
            "SCRAM-SHA-1",
            "invalid-mechanism",
        ),
        (
            "bad-format",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, format!("{C}\0only-two")),
            "PLAIN",
            "not-authorized",
        ),
        ("bad-format", base64_plain(D, C, &own), "PLAIN", "not-authorized"),
    ];
    for (reason, payload, mechanism, condition) in cases {
        let client = t.connected().await;
        assert_eq!(
            sasl_answer(&client, &payload, mechanism).await,
            format!(r#"<failure xmlns="urn:ietf:params:xml:ns:xmpp-sasl"><{condition}/></failure>"#),
            "{reason}"
        );
        client.close();
        assert!(logged(&format!("reason={reason}")), "{reason}");
    }
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn holds_an_address_back_after_10_failed_logins_in_10_minutes() {
    let t = lt().await;
    for _ in 0..10 {
        let client = t.connected().await;
        assert!(
            sasl_answer(&client, &t.plain(C, D).await, "PLAIN")
                .await
                .contains("not-authorized")
        );
        client.close();
    }
    let held = t.connected().await;
    assert_eq!(sasl_answer(&held, &t.plain(C, C).await, "PLAIN").await, HELD);
    held.close();

    t.advance(10 * 60 * 1000);
    let later = t.signed_in(C).await;
    assert!(later.is_open());
    later.logout().await;
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hangs_up_on_a_connection_that_does_not_log_in_within_15_s_or_bind_within_10_s_of_its_login() {
    let t = lt().await;
    let silent = t.connected().await;
    t.advance(15 * 1000);
    t.tick().await;
    silent.closed().await;

    let unbound = t.connected().await;
    t.advance(12 * 1000);
    assert!(
        sasl_answer(&unbound, &t.plain(C, C).await, "PLAIN")
            .await
            .contains("<success")
    );
    t.advance(9 * 1000);
    t.tick().await;
    assert!(unbound.is_open(), "10 s from the login, not from the connection");
    t.advance(1000);
    t.tick().await;
    unbound.closed().await;
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn before_login_a_few_frames_only_and_one_sasl_attempt_per_connection() {
    let t = lt().await;
    // A flood of <open/> that is never read: the connection ends at the fifth frame, having been
    // answered four times
    let flood = t.connected().await;
    flood.pause();
    for _ in 0..200 {
        flood.send(&format!(r#"<open xmlns="{FRAMING}" to="{DOMAIN}" version="1.0"/>"#));
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(logged(r"^info chat: closed c=\d+ uid=- reason=refused after=\d+s$"));
    flood.resume();
    flood.closed().await;
    assert!(
        flood.frames.lock().unwrap().len() <= 8,
        "at most two frames for each of the first four"
    );

    // A second <auth> after a refused one ends the connection
    let twice = t.connected().await;
    assert!(
        sasl_answer(&twice, &t.plain(C, D).await, "PLAIN")
            .await
            .contains("not-authorized")
    );
    twice.send(&format!(
        r#"<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="PLAIN">{}</auth>"#,
        t.plain(C, C).await
    ));
    assert_eq!(twice.next("answer").await, None, "closed without an answer");
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_account_has_at_most_three_connections_a_new_login_closes_the_oldest_one_that_has_not_bound() {
    let t = lt().await;
    let mut pending = Vec::new();
    for _ in 0..5 {
        let client = t.connected().await;
        assert!(
            sasl_answer(&client, &t.plain(LOOP, LOOP).await, "PLAIN")
                .await
                .contains("<success")
        );
        pending.push(client);
    }
    pending[0].closed().await;
    pending[1].closed().await;
    assert!(pending[2..].iter().all(WireClient::is_open), "the three newest stay");

    // They count toward the address's limit of 8 unfinished logins until they bind
    let mut more = Vec::new();
    for _ in 0..5 {
        more.push(t.connected().await);
    }
    assert_eq!(WireClient::connect(t.chat.port, &[]).await.err(), Some(503));

    // Binding the same resource replaces the older session; the account's next login then waits 60 s
    let resource = game_resource();
    for client in &pending[2..] {
        client.send(&format!(r#"<open xmlns="{FRAMING}" to="{DOMAIN}" version="1.0"/>"#));
        client.expect("open").await;
        client.expect("features").await;
        client.send(&format!(
            r#"<iq type="set" id="_xmpp_bind1"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><resource>{resource}</resource></bind></iq>"#
        ));
        client.expect("bind").await;
    }

    assert!(pending[4].is_open());
    let held = t.connected().await;
    assert!(
        sasl_answer(&held, &t.plain(LOOP, LOOP).await, "PLAIN")
            .await
            .contains("temporary-auth-failure")
    );
    for client in more.iter().chain([&held]) {
        client.close();
    }
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn three_logins_of_one_account_in_10_minutes_that_never_bind_hold_its_logins_back_for_60_s() {
    let t = lt().await;
    for _ in 0..3 {
        let client = t.connected().await;
        assert!(
            sasl_answer(&client, &t.plain(LOOP, LOOP).await, "PLAIN")
                .await
                .contains("<success")
        );
        t.advance(10 * 1000);
        t.tick().await;
        client.closed().await;
    }

    let held = t.connected().await;
    assert!(
        sasl_answer(&held, &t.plain(LOOP, LOOP).await, "PLAIN")
            .await
            .contains("temporary-auth-failure")
    );
    held.close();
    t.advance(60 * 1000);
    t.tick().await;
    let later = t.signed_in(LOOP).await;
    later.logout().await;
    t.stop().await;
}

// ---- liveness ----

#[tokio::test(flavor = "multi_thread")]
async fn pings_a_connection_that_was_silent_for_50_s_keeps_it_when_it_answers_late_a_long_map_load_and_ends_it_100_s_after_an_unanswered_ping()
 {
    let t = lt().await;
    let client = t.signed_in(C).await;
    client.auto_pong.store(false, Ordering::SeqCst);
    t.advance(50 * 1000);
    t.tick().await;

    // The game answers from its game-thread tick, which a map load holds up: 99 s later is in time
    let ping = client.expect("server ping").await;
    let shape = Regex::new(&format!(
        r#"^<iq xmlns="jabber:client" type="get" id="(sp\d+)" from="{}" to="{}"><ping xmlns="urn:xmpp:ping"/></iq>$"#,
        regex::escape(DOMAIN),
        regex::escape(&format!("{C}@{DOMAIN}/{}", client.resource))
    ))
    .unwrap();
    let id = shape.captures(&ping).expect("a server ping")[1].to_owned();
    t.advance(99 * 1000);
    t.tick().await;
    assert!(client.is_open(), "still waiting for the answer");
    client.send(&format!(r#"<iq type="result" id="{id}"/>"#));
    client.barrier().await;
    t.advance(30 * 1000);
    t.tick().await;
    assert!(client.is_open(), "answered, so still connected");

    t.advance(20 * 1000);
    t.tick().await;
    client.expect("second ping").await;
    t.advance(99 * 1000);
    t.tick().await;
    assert!(client.is_open());
    t.advance(1000);
    t.tick().await;
    client.closed().await;
    assert!(logged(r"chat: closed c=\d+ uid=UID-chat-c reason=ping-timeout"));

    // A ping timeout does not hold the account's next login back: one reconnect is not a loop
    let again = t.signed_in(C).await;
    again.logout().await;
    t.stop().await;
}

// ---- crash guard ----

#[tokio::test(flavor = "multi_thread")]
async fn survives_oversized_frames_bad_utf_8_bad_opcodes_dtds_and_garbage_and_never_hangs_up_a_signed_in_player_over_bad_xml()
 {
    let t = lt().await;
    let sender = t.signed_in(C).await;
    let receiver = t.signed_in(D).await;
    assert!(whispered(&sender, &receiver, "marker-before-guard").await);

    // Before login: a DTD gets a stream error and the connection ends; so does garbage
    let early = t.connected().await;
    early.send(r#"<!DOCTYPE lol [<!ENTITY lol "lol">]><open/>"#);
    assert!(
        early
            .expect("stream error")
            .await
            .contains(r#"<restricted-xml xmlns="urn:ietf:params:xml:ns:xmpp-streams"/>"#)
    );
    early.closed().await;
    let garbage = t.connected().await;
    garbage.send("<<<not xml");
    garbage.closed().await;

    // After login: dropped, never hung up
    let player = t.signed_in(C).await;
    for bad in [
        "<!DOCTYPE x><iq/>",
        r#"<?xml version="1.0"?><iq type="get" id="x"/>"#,
        "<<<not xml",
        r#"<iq type="get" id="half""#,
        "<message><body>&bogus;</body></message>",
        "<nonsense/>",
    ] {
        player.send(bad);
    }
    assert_eq!(player.barrier().await, Vec::<String>::new());
    assert!(player.is_open());

    // A binary frame is read as text
    player.send_binary(br#"<iq type="get" id="bin1"><ping xmlns="urn:xmpp:ping"/></iq>"#);
    assert_eq!(
        player.expect("binary").await,
        r#"<iq xmlns="jabber:client" type="result" id="bin1"/>"#
    );

    // An oversized frame: closed with 1009, the metagame stays up
    let big = t.signed_in(D).await;
    big.send(&format!(
        r#"<message to="{C}@{DOMAIN}" type="chat"><body>{}</body></message>"#,
        "x".repeat(33 * 1024)
    ));
    assert_eq!(big.closed().await, 1009);

    // Invalid UTF-8 in a text frame, a reserved opcode and an unmasked frame, over raw sockets
    for frame in [
        client_frame(0x1, &[0x3c, 0xff, 0xfe, 0x3e], true),
        client_frame(0x3, b"x", true),
        client_frame(0x1, b"<iq/>", false),
    ] {
        let mut raw = raw_upgrade(t.chat.port).await;
        tokio::io::AsyncWriteExt::write_all(&mut raw, &frame).await.unwrap();
        assert!(raw_closed(&mut raw).await, "the bad connection is closed");
    }

    assert!(
        whispered(&sender, &receiver, "marker-after-guard").await,
        "the others still chat"
    );
    for client in [sender, receiver, player] {
        client.logout().await;
    }
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_new_connections_with_503_past_8_unfinished_logins_from_one_address() {
    let t = lt().await;
    let mut pending = Vec::new();
    for _ in 0..8 {
        pending.push(t.connected().await);
    }
    assert_eq!(WireClient::connect(t.chat.port, &[]).await.err(), Some(503));
    for client in &pending {
        client.close();
    }
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_who_stops_reading_at_most_the_cap_of_unsent_output_is_held_for_it_then_it_ends_backlog_and_its_logins_wait_60_s()
 {
    let limit = 64 * 1024;
    let t = lt_with(|options| options.output_limit = limit).await;
    let room = format!("City-backlog@muc.{DOMAIN}");
    let slow = t.signed_in(C).await;
    let fast = t.signed_in(D).await;

    for (client, name) in [(&slow, "Charlie"), (&fast, "Delta")] {
        client.send(&format!(
            r#"<presence to="{}"><x xmlns="http://jabber.org/protocol/muc"/></presence>"#,
            escape(&format!("{room}/{name}:{}:{}", client.uid, client.resource))
        ));
        client.barrier().await;
    }

    slow.barrier().await;
    slow.pause();
    let big = escape(&"<".repeat(2048));
    let ended = || logged(&format!(r"^info chat: closed c=\d+ uid={C} reason=backlog"));
    let mut largest = 0;

    let mut round = 0;
    while round < 400 && !ended() {
        // About 8 KB per message once escaped: four keep the reading player's burst at half the cap, so only
        // the player who stopped reading can pass it, however slowly the server writes on a busy runner
        for index in 0..4 {
            fast.send(&format!(
                r#"<message type="groupchat" to="{room}" id="bl{round}-{index}"><body>{big}</body></message>"#
            ));
        }
        fast.barrier().await;
        largest = largest.max(t.chat.largest_backlog());
        // Still talking, so it is neither idle nor pinged; its next frame is where the server checks
        slow.send(&format!(
            r#"<iq type="get" id="alive{round}"><ping xmlns="urn:xmpp:ping"/></iq>"#
        ));
        tokio::task::yield_now().await;
        t.advance(8 * 1000);
        round += 1;
    }

    // The ending is logged as the frame after the last round is read
    for _ in 0..100 {
        if ended() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ended(), "ended for its backlog");
    assert!(largest <= limit + 9 * 1024, "held {largest} bytes");
    slow.resume();
    slow.closed().await;
    assert!(fast.is_open(), "the others chat on");

    let next = t.connected().await;
    assert!(
        sasl_answer(&next, &t.plain(C, C).await, "PLAIN")
            .await
            .contains("temporary-auth-failure")
    );
    next.close();
    fast.logout().await;
    t.stop().await;
}

// ---- public mode ----

#[tokio::test(flavor = "multi_thread")]
async fn takes_the_players_address_from_the_gateway_only_when_it_carries_the_secret() {
    let secret: String = (0..24).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    let app = TestApp::with_env(&[("GATEWAY_SECRET", secret.as_str())]).await;
    let t = lt_on(app, |_| {}).await;

    let right = WireClient::connect(
        t.chat.port,
        &[
            ("X-Dauntless-Gateway", secret.as_str()),
            ("X-Forwarded-For", "203.0.113.7"),
        ],
    )
    .await
    .unwrap();
    let wrong = WireClient::connect(
        t.chat.port,
        &[
            ("X-Dauntless-Gateway", "wrong-secret-value-000"),
            ("X-Forwarded-For", "203.0.113.8"),
        ],
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(logged(r"chat: connect c=\d+ from=203\.0\.113\.7 via=gateway$"));
    assert!(logged(r"chat: connect c=\d+ from=127\.0\.0\.1 via=direct$"));
    assert!(!logged_lines().iter().any(|line| line.contains("203.0.113.8")));
    assert!(
        !logged_lines().iter().any(|line| line.contains(&secret)),
        "never the secret"
    );
    right.close();
    wrong.close();
    t.stop().await;
}

// ---- settings and start ----

fn settings(pairs: &[(&str, &str)]) -> Settings {
    Settings::from_env(&Env::from_pairs(pairs.iter().copied()))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_a_loopback_listener_with_chat_unset_and_can_close_it_cleanly() {
    let app = TestApp::start().await;
    let port = free_port().to_string();
    let started = metagame::chat::connection::start(app.state.clone(), &settings(&[("CHAT_PORT", &port)])).await;
    started.expect("started").stop().await;
}

#[test]
fn reads_chat_chat_port_and_chat_bind_host_with_127_0_0_1_the_only_host_in_public_mode() {
    assert!(settings(&[]).enabled);
    assert!(settings(&[("CHAT", " ")]).enabled);
    assert!(!settings(&[("CHAT", "0")]).enabled);
    assert!(!settings(&[("CHAT", "wrong")]).enabled);
    assert_eq!(settings(&[("CHAT", "wrong")]).warnings.len(), 2);
    let on = settings(&[("CHAT", "1")]);
    assert_eq!(
        (on.enabled, on.port, on.host.as_str(), on.nick_check, on.trace),
        (true, 61099, "127.0.0.1", NickCheck::Enforce, false)
    );
    assert!(on.errors.is_empty() && on.warnings.is_empty());
    assert_eq!(
        settings(&[("CHAT", "1"), ("CHAT_NICK_CHECK", "log")]).nick_check,
        NickCheck::Log
    );
    let off = settings(&[("CHAT", "1"), ("CHAT_NICK_CHECK", "off")]);
    assert_eq!(off.nick_check, NickCheck::Enforce);
    assert!(off.warnings[0].contains("CHAT_NICK_CHECK=off is not enforce or log"));
    assert_eq!(settings(&[("CHAT", "1"), ("CHAT_BIND_HOST", "::1")]).errors.len(), 0);
    let secret = "x".repeat(32);
    assert_eq!(
        settings(&[("CHAT", "1"), ("CHAT_BIND_HOST", "::1"), ("GATEWAY_SECRET", &secret)])
            .errors
            .len(),
        1
    );
    assert_eq!(
        settings(&[("CHAT", "1"), ("CHAT_BIND_HOST", "0.0.0.0")]).errors.len(),
        1
    );
    assert_eq!(settings(&[("CHAT", "1"), ("CHAT_PORT", "70000")]).errors.len(), 1);
    assert!(settings(&[("CHAT", "1"), ("CHAT_TRACE", "1")]).trace);
    assert!(
        Regex::new("no longer read.*CHAT=1")
            .unwrap()
            .is_match(&settings(&[("EXPERIMENTAL_CHAT", "1")]).warnings[0])
    );
}

/// Starts the built metagame in its own process with a fresh database and waits for its "Clear Skies"
/// line, or 20 s. Returns everything it printed.
async fn run_metagame(extra: &[(&str, &str)]) -> String {
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    let keys: Value = serde_json::from_str(include_str!("../fixtures/node-crypto.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let port = free_port().to_string();
    let db = dir.path().join("start.db").to_string_lossy().into_owned();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_metagame"));
    command
        .arg("serve")
        .env_clear()
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for name in ["PATH", "SystemRoot"] {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    let settings = [
        ("PORT", port.as_str()),
        ("BIND_HOST", "127.0.0.1"),
        ("DB_FILENAME", db.as_str()),
        ("NODE_ENV", "production"),
        ("LOG_LEVEL", "info"),
        ("AUTH_MODE", "APIKEY"),
        ("AUTH_SIGNING_PRIVKEY_B64", keys["privB64"].as_str().unwrap()),
        ("AUTH_SIGNING_PUBKEY_B64", keys["pubB64"].as_str().unwrap()),
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", "127.0.0.1:1"),
        ("REGISTRATION_MODE", "OPEN"),
        ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
        ("TARGET_CHANGELIST", "239827"),
        ("LOG_REQUESTS", "0"),
    ];
    for (name, value) in settings.iter().chain(extra) {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    let output: Arc<Mutex<Vec<u8>>> = Arc::default();
    let readers: Vec<Box<dyn Read + Send>> = vec![
        Box::new(child.stdout.take().unwrap()),
        Box::new(child.stderr.take().unwrap()),
    ];
    for mut reader in readers {
        let output = output.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                output.lock().unwrap().extend_from_slice(&buffer[..read]);
            }
        });
    }
    let text = || String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !text().contains("Clear Skies")
        && std::time::Instant::now() < deadline
        && child.try_wait().ok().flatten().is_none()
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = child.kill();
    let _ = child.wait();
    text()
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_one_error_line_and_starts_nothing_on_a_port_in_use_or_a_bad_host_the_metagame_starts_anyway() {
    let app = TestApp::start().await;
    logged_lines();
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = blocker.local_addr().unwrap().port().to_string();
    let start = |pairs: &[(&str, &str)]| {
        let settings = settings(pairs);
        let state = app.state.clone();
        async move { metagame::chat::connection::start(state, &settings).await }
    };

    assert!(start(&[("CHAT", "1"), ("CHAT_PORT", &taken)]).await.is_none());
    assert!(logged_line(&format!(
        "error chat: not started: could not listen on 127.0.0.1:{taken} (EADDRINUSE). The metagame runs without chat."
    )));
    assert!(start(&[("CHAT", "1"), ("CHAT_BIND_HOST", "0.0.0.0")]).await.is_none());
    assert!(start(&[("CHAT", "0")]).await.is_none());
    assert!(
        logged_lines()
            .iter()
            .any(|line| line.contains("chat: disabled by CHAT setting"))
    );

    // The real metagame process with chat on and its port taken: it still serves
    let output = run_metagame(&[("CHAT_PORT", &taken)]).await;
    assert!(
        Regex::new(r"chat: not started: could not listen on 127\.0\.0\.1:\d+ \(EADDRINUSE\)")
            .unwrap()
            .is_match(&output),
        "{output}"
    );
    assert!(output.contains("Clear Skies, Slayer."), "{output}");
    drop(blocker);
}

// ---- rooms and names ----

const CITY: &str = "City-5f1c0000-aaaa-4bbb-8ccc-000000000001";

/// A player with a client model beside its connection: the model's stanzas go out, every reply comes in
struct Player {
    wire: WireClient,
    model: ChatClientModel,
    name: String,
}

impl Lt {
    async fn player(&self, uid: &str, name: &str) -> Player {
        self.player_with(uid, name, LoginOptions::default()).await.unwrap()
    }

    async fn player_with(&self, uid: &str, name: &str, options: LoginOptions) -> Result<Player, String> {
        let wire = self.signed_in_with(uid, options).await?;
        let model = ChatClientModel::new(uid, &wire.resource, name, Some(&wire.domain));
        Ok(Player {
            wire,
            model,
            name: name.to_owned(),
        })
    }
}

/// Each player's frames up to a barrier, fed to its model
async fn settle(players: &mut [&mut Player]) -> Vec<Vec<String>> {
    let mut seen = Vec::new();
    for player in players.iter_mut() {
        let frames = player.wire.barrier().await;
        player.model.receive_all(&frames);
        seen.push(frames);
    }
    seen
}

fn join_as(who: &mut Player, room: &str) {
    let name = who.name.clone();
    join_named(who, room, &name);
}

fn join_named(who: &mut Player, room: &str, name: &str) {
    let stanza = who.model.join_public_room(room, name).expect("a join");
    who.wire.send(&stanza);
}

fn nick(who: &Player) -> String {
    who.model.nickname(&who.name)
}

fn full_jid(who: &Player) -> String {
    format!("{}@{DOMAIN}/{}", who.wire.uid, who.wire.resource)
}

fn occupant_presence(room: &str, nick: &str, occupant: &Player, to: &Player, is_self: bool, kind: &str) -> String {
    let (type_attr, role, statuses) = match kind {
        "unavailable" => (r#" type="unavailable""#, "none", ""),
        "evicted" => (r#" type="unavailable""#, "none", r#"<status code="307"/>"#),
        _ => ("", "participant", ""),
    };
    format!(
        r#"<presence xmlns="jabber:client"{type_attr} from="{}" to="{}"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="{role}" jid="{}"/>{}{statuses}</x></presence>"#,
        escape(&format!("{room}@{}/{nick}", muc_domain())),
        escape(&full_jid(to)),
        escape(&full_jid(occupant)),
        if is_self { r#"<status code="110"/>"# } else { "" }
    )
}

fn presence(room: &str, nick: &str, occupant: &Player, to: &Player, is_self: bool) -> String {
    occupant_presence(room, nick, occupant, to, is_self, "available")
}

fn unavailable(room: &str, nick: &str, occupant: &Player, to: &Player, is_self: bool) -> String {
    occupant_presence(room, nick, occupant, to, is_self, "unavailable")
}

fn evicted(room: &str, nick: &str, occupant: &Player, to: &Player, is_self: bool) -> String {
    occupant_presence(room, nick, occupant, to, is_self, "evicted")
}

fn room_join(room: &str, nick: &str) -> String {
    format!(
        r#"<presence to="{}"><x xmlns="http://jabber.org/protocol/muc"/></presence>"#,
        escape(&format!("{room}@{}/{nick}", muc_domain()))
    )
}

fn names() -> impl Fn(&str) -> Option<String> {
    lookup_from(&[(A, "Alpha"), (B, "Bravo"), (C, "Charlie"), (D, "Delta")])
}

const FORBIDDEN: &str = r#"<error type="auth"><forbidden xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error>"#;

async fn finish(t: Lt, players: Vec<Player>) {
    for player in players {
        player.wire.logout().await;
    }
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_players_joins_in_order_item_jid_everywhere_110_only_on_self_every_message_to_both_with_one_from_and_id_both_see_usernames()
 {
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo = t.player(B, "Bravo").await;
    let (nick_a, nick_b) = (nick(&alpha), nick(&bravo));

    join_as(&mut alpha, CITY);
    assert_eq!(
        settle(&mut [&mut alpha]).await[0],
        [presence(CITY, &nick_a, &alpha, &alpha, true)]
    );

    join_as(&mut bravo, CITY);
    let seen = settle(&mut [&mut bravo, &mut alpha]).await;
    assert_eq!(
        seen[0],
        [
            presence(CITY, &nick_a, &alpha, &bravo, false),
            presence(CITY, &nick_b, &bravo, &bravo, true)
        ],
        "B hears of A, then itself"
    );
    assert_eq!(
        seen[1],
        [presence(CITY, &nick_b, &bravo, &alpha, false)],
        "A hears of B"
    );

    let text = "©héllo & <b>marker-room</b>";
    let line = alpha.model.room_message(CITY, text, "3F2504E04F8911D39A0C0305E82C3301");
    alpha.wire.send(&line);
    let seen = settle(&mut [&mut alpha, &mut bravo]).await;
    let expected = |to: &Player| {
        format!(
            r#"<message xmlns="jabber:client" type="groupchat" id="3F2504E04F8911D39A0C0305E82C3301" from="{}" to="{}"><body>{}</body></message>"#,
            escape(&format!("{CITY}@{}/{nick_a}", muc_domain())),
            escape(&full_jid(to)),
            escape(text)
        )
    };
    assert_eq!(seen[0], [expected(&alpha)], "the sender gets its own line back");
    assert_eq!(seen[1], [expected(&bravo)]);

    let reply = bravo.model.room_message(CITY, "marker-reply", "g2");
    bravo.wire.send(&reply);
    settle(&mut [&mut bravo, &mut alpha]).await;
    let shown = [format!("Alpha: {text}"), "Bravo: marker-reply".to_owned()];
    assert_eq!(alpha.model.shown_lines(names()).await, shown);
    assert_eq!(bravo.model.shown_lines(names()).await, shown);
    assert!(logged_line(&format!(
        "info chat: join room={CITY} uid={B} name=Bravo occupants=1"
    )));
    assert!(logged_line(&format!(
        "info chat: message room={CITY} uid={A} len={} to=2",
        text.chars().count()
    )));
    finish(t, vec![alpha, bravo]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rooms_accept_the_1_4_4_muc_and_legacy_conference_aliases_for_the_authenticated_domain() {
    let t = lt().await;
    let options = LoginOptions {
        open_to: Some(Some("dauntless.local".into())),
        ..LoginOptions::default()
    };
    let mut local = t.player_with(C, "Charlie", options).await.unwrap();

    join_as(&mut local, CITY);
    settle(&mut [&mut local]).await;
    assert_eq!(
        local.model.room_of(CITY).unwrap().state,
        JOINED,
        "joined on muc.dauntless.local"
    );

    local.wire.send(&format!(
        r#"<presence to="Hunt-legacy@conference.dauntless.local/{}"><x xmlns="http://jabber.org/protocol/muc"/></presence>"#,
        nick(&local)
    ));
    let legacy = settle(&mut [&mut local]).await.remove(0);
    assert_eq!(legacy.len(), 1);
    assert!(legacy[0].starts_with(r#"<presence xmlns="jabber:client" from="Hunt-legacy@conference.dauntless.local/"#));

    let refused = Regex::new(r#"^<presence xmlns="jabber:client" type="error" from="Hunt-1@[^"]+"[^>]*><x xmlns="http://jabber.org/protocol/muc"/><error type="cancel"><not-allowed xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error></presence>$"#).unwrap();
    for domain in [format!("conference.{DOMAIN}"), format!("muc.{DOMAIN}")] {
        local.wire.send(&format!(
            r#"<presence to="Hunt-1@{domain}/{}"><x xmlns="http://jabber.org/protocol/muc"/></presence>"#,
            nick(&local)
        ));
        let frames = settle(&mut [&mut local]).await.remove(0);
        assert_eq!(frames.len(), 1, "{domain}");
        assert!(refused.is_match(&frames[0]), "{domain}: {}", frames[0]);
    }
    finish(t, vec![local]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn three_players_the_last_one_hears_of_both_leaves_and_a_dropped_connection_are_told_to_the_rest() {
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo = t.player(B, "Bravo").await;
    let mut charlie = t.player(C, "Charlie").await;
    let (nick_a, nick_b, nick_c) = (nick(&alpha), nick(&bravo), nick(&charlie));

    join_as(&mut alpha, CITY);
    settle(&mut [&mut alpha]).await;
    join_as(&mut bravo, CITY);
    settle(&mut [&mut bravo, &mut alpha]).await;
    join_as(&mut charlie, CITY);
    let seen = settle(&mut [&mut charlie, &mut alpha, &mut bravo]).await;
    assert_eq!(
        seen[0],
        [
            presence(CITY, &nick_a, &alpha, &charlie, false),
            presence(CITY, &nick_b, &bravo, &charlie, false),
            presence(CITY, &nick_c, &charlie, &charlie, true)
        ]
    );

    let exit = alpha.model.exit_room(CITY).unwrap();
    alpha.wire.send(&exit);
    let seen = settle(&mut [&mut alpha, &mut bravo, &mut charlie]).await;
    assert_eq!(seen[0], [unavailable(CITY, &nick_a, &alpha, &alpha, true)]);
    assert_eq!(seen[1], [unavailable(CITY, &nick_a, &alpha, &bravo, false)]);
    assert_eq!(seen[2], [unavailable(CITY, &nick_a, &alpha, &charlie, false)]);
    assert_eq!(alpha.model.room_of(CITY).unwrap().state, NOT_JOINED);

    charlie.wire.close();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let dropped = settle(&mut [&mut bravo]).await.remove(0);
    assert_eq!(dropped, [unavailable(CITY, &nick_c, &charlie, &bravo, false)]);
    assert!(logged_line(&format!(
        "info chat: leave room={CITY} uid={C} reason=disconnect"
    )));
    finish(t, vec![alpha, bravo]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_nickname_with_another_accounts_id_another_name_the_wrong_resource_or_characters_the_client_never_writes()
 {
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;
    let res = alpha.wire.resource.clone();
    let cases = [
        (format!("Alpha:{B}:{res}"), "nick-account"),
        ("Alpha".to_owned(), "nick-account"),
        (format!("Bravo:{A}:{res}"), "nick-name"),
        (format!("Al+pha:{A}:{res}"), "nick-format"),
        (format!("Al pha:{A}:{res}"), "nick-format"),
        (format!("%ZZ:{A}:{res}"), "nick-format"),
        (format!("%FF:{A}:{res}"), "nick-format"),
        (format!("Alpha:{A}:V2:Other:WIN::0"), "nick-resource"),
    ];

    for (nick, reason) in cases {
        alpha.wire.send(&room_join("Hunt-9", &nick));
        let frames = settle(&mut [&mut alpha]).await.remove(0);
        assert_eq!(frames.len(), 1, "{nick}");
        assert!(frames[0].contains(FORBIDDEN), "{nick}");
        assert!(
            logged_line(&format!("info chat: join refused room=Hunt-9 uid={A} reason={reason}")),
            "{nick}: {reason}"
        );
    }

    // Another account's id hidden in a bound resource is refused too
    let options = LoginOptions {
        resource: Some(format!("V2:{A}:WIN::0")),
        ..LoginOptions::default()
    };
    let mut forged = t.player_with(B, "Bravo", options).await.unwrap();
    join_as(&mut forged, "Hunt-10");
    settle(&mut [&mut forged]).await;
    assert!(forged.model.room_of("Hunt-10").is_none());
    assert!(logged_line(&format!(
        "info chat: join refused room=Hunt-10 uid={B} reason=nick-account"
    )));
    finish(t, vec![alpha, forged]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn accepts_the_clients_own_encodings_41lpha_lower_case_hex_invalidmcpuser_and_an_older_non_ascii_name() {
    let t = lt().await;
    let mut old = t.player(OLD, OLD_NAME).await;
    let res = old.wire.resource.clone();

    assert_eq!(url_encode_like_client(OLD_NAME), "S%C3%B6lve%20%C3%96");

    for (room, nick) in [
        ("Hunt-20", format!("{}:{OLD}:{res}", url_encode_like_client(OLD_NAME))),
        ("Hunt-21", format!("S%c3%b6lve%20%c3%96:{OLD}:{res}")),
        ("Hunt-22", format!("InvalidMCPUser:{OLD}:{res}")),
    ] {
        old.wire.send(&room_join(room, &nick));
        let frames = settle(&mut [&mut old]).await.remove(0);
        assert!(frames.last().unwrap().contains(r#"<status code="110"/>"#), "{nick}");
    }

    let mut alpha = t.player(A, "Alpha").await;
    let join = room_join("Hunt-23", &format!("%41lpha:{A}:{}", alpha.wire.resource));
    alpha.wire.send(&join);
    let frames = settle(&mut [&mut alpha]).await.remove(0);
    assert!(frames.last().unwrap().contains(r#"<status code="110"/>"#));
    assert!(logged_line(&format!(
        "info chat: join room=Hunt-23 uid={A} name=Alpha occupants=0"
    )));
    finish(t, vec![old, alpha]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_nick_check_log_admits_a_bad_name_or_resource_with_a_warning_but_never_another_accounts_id() {
    let t = lt_with(|options| options.nick_check = NickCheck::Log).await;
    let alpha = t.signed_in(A).await;
    let bravo = t.signed_in(B).await;

    alpha.send(&room_join("Hunt-30", &format!("Somebody:{A}:{}", alpha.resource)));
    assert!(
        alpha
            .barrier()
            .await
            .last()
            .unwrap()
            .contains(r#"<status code="110"/>"#),
        "admitted"
    );
    assert!(logged_line(&format!(
        "warn chat: join nickname not checked room=Hunt-30 uid={A} reason=nick-name (CHAT_NICK_CHECK=log)"
    )));

    alpha.send(&room_join("Hunt-32", &format!("Alpha:{A}:V2:Other:WIN::0")));
    assert!(
        alpha
            .barrier()
            .await
            .last()
            .unwrap()
            .contains(r#"<status code="110"/>"#),
        "another resource is admitted"
    );

    // Another account's id: refused in log mode too, as part 2 or anywhere else in the nickname
    for (room, nick) in [
        ("Hunt-31", format!("Alpha:{A}:{}", alpha.resource)),
        ("Hunt-33", format!("{A}:{B}:{}", bravo.resource)),
        ("Hunt-34", "Bravo".to_owned()),
    ] {
        bravo.send(&room_join(room, &nick));
        let refused = bravo.barrier().await;
        assert_eq!(refused.len(), 1, "{nick}");
        assert!(refused[0].contains(FORBIDDEN), "{nick}");
    }

    // Logged once per connection, room kind and reason
    assert!(logged_line(&format!(
        "info chat: join refused room=Hunt-31 uid={B} reason=nick-account"
    )));
    assert!(
        !logged_lines()
            .iter()
            .any(|line| line.starts_with("info chat: join refused room=Hunt-33 "))
    );

    alpha.logout().await;
    bravo.logout().await;
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn whispers_a_bare_jid_reaches_every_session_of_the_account_a_full_jid_one_from_the_senders_full_jid_offline_and_self_are_dropped()
 {
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo1 = t.player(B, "Bravo").await;
    let mut bravo2 = t.player(B, "Bravo").await;

    alpha.wire.send(&format!(
        r#"<message type="chat" to="{B}@{DOMAIN}" id="w1"><body>marker-psst</body></message>"#
    ));
    let seen = settle(&mut [&mut alpha, &mut bravo1, &mut bravo2]).await;
    let expected = |to: &Player| {
        format!(
            r#"<message xmlns="jabber:client" type="chat" from="{}" to="{}" id="w1"><body>marker-psst</body></message>"#,
            escape(&full_jid(&alpha)),
            escape(&format!("{B}@{DOMAIN}/{}", to.wire.resource))
        )
    };
    assert_eq!(seen[1], [expected(&bravo1)]);
    assert_eq!(seen[2], [expected(&bravo2)]);
    assert_eq!(bravo1.model.shown_whispers(names()), ["Alpha: marker-psst"]);

    alpha.wire.send(&format!(
        r#"<message type="chat" to="{}"><body>marker-only-two</body></message>"#,
        escape(&format!("{B}@{DOMAIN}/{}", bravo2.wire.resource))
    ));
    let seen = settle(&mut [&mut alpha, &mut bravo1, &mut bravo2]).await;
    assert_eq!(seen[1].len(), 0);
    assert_eq!(seen[2].len(), 1);

    alpha.wire.send(&format!(
        r#"<message type="chat" to="{D}@{DOMAIN}"><body>marker-offline</body></message>"#
    ));
    alpha.wire.send(&format!(
        r#"<message type="chat" to="{A}@{DOMAIN}"><body>marker-self</body></message>"#
    ));
    assert_eq!(
        settle(&mut [&mut alpha]).await[0],
        Vec::<String>::new(),
        "no error goes back"
    );
    assert!(logged_line(&format!(
        "info chat: whisper from={A} to={D} len=14 reason=offline"
    )));
    assert!(logged_line(&format!(
        "info chat: whisper from={A} to={A} len=11 reason=self"
    )));
    assert!(logged_line(&format!(
        "info chat: whisper from={A} to={B} len=11 delivered=2"
    )));
    finish(t, vec![alpha, bravo1, bravo2]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_sessions_of_one_account_never_see_each_other_the_newer_takes_a_room_over_a_third_replaces_the_silent_one_a_ghost_is_pinged_out_in_10_s()
 {
    let t = lt().await;
    let mut first = t.player(D, "Delta").await;
    let mut second = t.player(D, "Delta").await;
    let room = "City-7d1f0000-0000-4000-8000-00000000000d";
    let other = "City-7d1f0000-0000-4000-8000-00000000000e";

    join_as(&mut first, room);
    settle(&mut [&mut first]).await;
    join_as(&mut second, room);
    let seen = settle(&mut [&mut second, &mut first]).await;
    assert_eq!(seen[0].len(), 1, "only its own presence");
    assert_eq!(
        seen[1].len(),
        0,
        "nothing about the other session, not even that it lost the room"
    );
    assert!(logged_line(&format!(
        "info chat: leave room={room} uid={D} reason=replaced"
    )));

    let line = first.model.room_message(room, "marker-same", "s1");
    first.wire.send(&line);
    let seen = settle(&mut [&mut first, &mut second]).await;
    let kind = Regex::new(r#"type="(\w+)""#).unwrap();
    assert_eq!(
        seen[0]
            .iter()
            .map(|f| kind.captures(f).unwrap()[1].to_owned())
            .collect::<Vec<_>>(),
        ["error"],
        "the older session is no longer in the room"
    );
    assert_eq!(seen[1].len(), 0, "the other session hears nothing");

    // In a room of its own, the older one still chats; the other session hears nothing of it
    join_as(&mut first, other);
    settle(&mut [&mut first]).await;
    let line = first.model.room_message(other, "marker-own", "s2");
    first.wire.send(&line);
    let seen = settle(&mut [&mut first, &mut second]).await;
    assert_eq!(seen[0].len(), 1, "its own line comes back");
    assert_eq!(seen[1].len(), 0);

    // A third session: the one silent longest goes (reason replaced), and the new bind pings the
    // other one, which is ended as a ghost when it stays silent for 10 s
    t.advance(1000);
    settle(&mut [&mut second]).await;
    second.wire.auto_pong.store(false, Ordering::SeqCst);
    let third = t.player(D, "Delta").await;
    first.wire.closed().await;
    assert!(logged(r"chat: closed c=\d+ uid=UID-chat-d reason=replaced"));
    t.advance(10 * 1000);
    t.tick().await;
    second.wire.closed().await;
    assert!(logged(r"chat: closed c=\d+ uid=UID-chat-d reason=ping-timeout"));
    assert!(third.wire.is_open());

    // The replacement holds the account's next login back for 60 s (the ping timeout would not)
    let refused = t.player_with(D, "Delta", LoginOptions::default()).await;
    assert!(refused.err().unwrap().contains("was refused"));
    finish(t, vec![third]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reconnect_while_the_old_connection_lingers_the_new_one_takes_the_room_over_and_the_others_keep_the_name() {
    // The client keeps one room member per account id and removes it by the account id in a leaving
    // nickname. If the old connection stayed in the room until its ping timeout, its leave would
    // remove the member the new connection had just updated, and every later line of that player
    // would show as [unknown] to the others.
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo1 = t.player(B, "Bravo").await;
    let room = "City-7d1f0000-0000-4000-8000-0000000000b2";

    join_as(&mut alpha, room);
    settle(&mut [&mut alpha]).await;
    join_as(&mut bravo1, room);
    settle(&mut [&mut bravo1, &mut alpha]).await;

    // B's connection drops without a close; the game reconnects and joins the room again
    bravo1.wire.auto_pong.store(false, Ordering::SeqCst);
    let seen_before = bravo1.wire.frames.lock().unwrap().len();
    let mut bravo2 = t.player(B, "Bravo").await;
    join_as(&mut bravo2, room);
    let seen = settle(&mut [&mut bravo2, &mut alpha]).await;
    assert_eq!(
        seen[1],
        [
            unavailable(room, &nick(&bravo1), &bravo1, &alpha, false),
            presence(room, &nick(&bravo2), &bravo2, &alpha, false)
        ],
        "the old one leaves first, then the new one comes"
    );
    assert_eq!(
        seen[0],
        [
            presence(room, &nick(&alpha), &alpha, &bravo2, false),
            presence(room, &nick(&bravo2), &bravo2, &bravo2, true)
        ]
    );
    assert!(logged_line(&format!(
        "info chat: leave room={room} uid={B} reason=replaced"
    )));

    // The old connection is pinged out 10 s later; nobody hears of it again
    t.advance(10 * 1000);
    t.tick().await;
    bravo1.wire.closed().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !bravo1.wire.frames.lock().unwrap()[seen_before..]
            .iter()
            .any(|frame| frame.contains(room)),
        "the old connection is not told"
    );
    assert_eq!(
        settle(&mut [&mut alpha]).await[0],
        Vec::<String>::new(),
        "no second leave"
    );

    let line = bravo2.model.room_message(room, "marker-back", "rb1");
    bravo2.wire.send(&line);
    settle(&mut [&mut bravo2, &mut alpha]).await;
    assert_eq!(alpha.model.shown_lines(names()).await, ["Bravo: marker-back"]);
    assert_eq!(bravo2.model.shown_lines(names()).await, ["Bravo: marker-back"]);
    finish(t, vec![alpha, bravo2]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flood_of_refused_joins_each_takes_a_join_token_and_counts_toward_the_abuse_limit_the_log_says_it_once_per_room_kind_and_reason_with_the_room_name_cut()
 {
    let t = lt().await;
    let joiner = t.player(JOINS, "Joins").await;
    let long = format!("Lobby-{}", "x".repeat(1800));

    for index in 0..150 {
        joiner.wire.send(&room_join(&format!("{long}{index}"), &nick(&joiner)));
    }

    joiner.wire.closed().await;
    let lines: Vec<String> = logged_lines()
        .into_iter()
        .filter(|line| line.contains(&format!("uid={JOINS}")))
        .collect();
    let refused: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("chat: join refused"))
        .collect();
    let reason = Regex::new(r"reason=(\S+)$").unwrap();
    assert_eq!(
        refused
            .iter()
            .map(|line| reason.captures(line).unwrap()[1].to_owned())
            .collect::<Vec<_>>(),
        ["not-allowed", "limit"],
        "the first ten take the burst, the rest are over the rate"
    );
    let cut = format!("room=Lobby-{}... ", "x".repeat(74));
    assert!(
        refused.iter().all(|line| line.contains(&cut)),
        "the room name is cut to 80 characters"
    );
    let abuse = Regex::new(r"^info chat: closed c=\d+ uid=UID-chat-joins reason=abuse").unwrap();
    assert!(lines.iter().any(|line| abuse.is_match(line)));
    t.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_a_2049_character_body_and_a_message_burst_get_the_room_error_a_stanza_flood_ends_the_session_and_holds_its_logins()
 {
    let t = lt().await;
    let mut flooder = t.player(FLOOD, "Flood").await;

    join_as(&mut flooder, "Hunt-40");
    settle(&mut [&mut flooder]).await;
    flooder.wire.send(&format!(
        r#"<message type="groupchat" to="Hunt-40@{}" id="big"><body>{}</body></message>"#,
        muc_domain(),
        "x".repeat(2049)
    ));
    let big = settle(&mut [&mut flooder]).await.remove(0);
    assert_eq!(
        big,
        [format!(
            r#"<message xmlns="jabber:client" type="error" id="big" from="Hunt-40@{}" to="{}"><error type="modify"><not-acceptable xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error></message>"#,
            muc_domain(),
            escape(&full_jid(&flooder))
        )]
    );

    for index in 0..9 {
        flooder.wire.send(&format!(
            r#"<message type="groupchat" to="Hunt-40@{}" id="r{index}"><body>marker-burst</body></message>"#,
            muc_domain()
        ));
    }

    let burst = settle(&mut [&mut flooder]).await.remove(0);
    assert_eq!(burst.iter().filter(|f| f.contains("<body>")).count(), 8, "a burst of 8");
    assert_eq!(burst.iter().filter(|f| f.contains(r#"type="error""#)).count(), 1);

    for index in 0..200 {
        flooder.wire.send(&format!(
            r#"<iq type="get" id="f{index}"><ping xmlns="urn:xmpp:ping"/></iq>"#
        ));
    }

    flooder.wire.closed().await;
    assert!(logged(r"chat: closed c=\d+ uid=UID-chat-flood reason=abuse"));

    let next = t.connected().await;
    assert!(
        sasl_answer(&next, &t.plain(FLOOD, FLOOD).await, "PLAIN")
            .await
            .contains("temporary-auth-failure")
    );
    next.close();
    t.stop().await;
}

// ---- Who may join, and blocks ----

impl Lt {
    async fn poll(&self, who: &str) -> Value {
        let (state, who) = (self.app.state.clone(), who.to_owned());
        let now = state.party_clock.now();
        let settings = state.party_settings();
        self.app
            .db(move |conn| state.parties.lock().poll(conn, settings, &who, now))
            .await
    }

    async fn invite(&self, leader: &str, member: &str, party: &str) {
        let (state, leader, member, party) = (
            self.app.state.clone(),
            leader.to_owned(),
            member.to_owned(),
            party.to_owned(),
        );
        let now = state.party_clock.now();
        let sent = self
            .app
            .db(move |conn| {
                state
                    .parties
                    .lock()
                    .invite(conn, &leader, Some(&json!(member)), Some(&json!(party)), now)
            })
            .await;
        assert_eq!(sent.status, 200);
    }

    async fn accept(&self, member: &str, party: &str) {
        let (state, member, party) = (self.app.state.clone(), member.to_owned(), party.to_owned());
        let now = state.party_clock.now();
        let settings = state.party_settings();
        let accepted = self
            .app
            .db(move |conn| state.parties.lock().accept(conn, settings, &member, &party, now))
            .await;
        assert_eq!(accepted.status, 200);
    }

    fn kick(&self, leader: &str, member: &str) {
        let now = self.app.state.party_clock.now();
        assert_eq!(self.app.state.parties.lock().kick(leader, member, now).status, 200);
    }

    async fn party_of(&self, leader: &str, members: &[&str]) -> String {
        let party = self.poll(leader).await["partyId"].as_str().unwrap().to_owned();
        for member in members {
            self.poll(member).await;
            self.invite(leader, member, &party).await;
            self.accept(member, &party).await;
        }
        party
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn party_rooms_members_only_a_kicked_member_is_removed_at_their_next_message_or_by_the_60_s_sweep() {
    let t = lt().await;
    let party = t.party_of(A, &[B]).await;
    t.poll(C).await;
    let room = format!("Party-{party}");
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo = t.player(B, "Bravo").await;
    let mut charlie = t.player(C, "Charlie").await;

    join_as(&mut alpha, &room);
    settle(&mut [&mut alpha]).await;
    join_as(&mut bravo, &room);
    settle(&mut [&mut bravo, &mut alpha]).await;
    assert_eq!(bravo.model.room_of(&room).unwrap().state, JOINED);

    join_as(&mut charlie, &room);
    let refused = settle(&mut [&mut charlie]).await.remove(0);
    assert!(refused[0].contains(FORBIDDEN));
    assert!(charlie.model.room_of(&room).is_none(), "the client drops the room");
    assert!(logged_line(&format!(
        "info chat: join refused room={room} uid={C} reason=not-member"
    )));

    // The leader kicks B: B's next line is refused and B is removed; A sees B go (307)
    t.kick(A, B);
    let line = bravo.model.room_message(&room, "marker-kicked", "k1");
    bravo.wire.send(&line);
    let seen = settle(&mut [&mut bravo, &mut alpha]).await;
    assert!(seen[0][0].starts_with(r#"<message xmlns="jabber:client" type="error" id="k1""#));
    assert_eq!(seen[0][1], evicted(&room, &nick(&bravo), &bravo, &bravo, true));
    assert_eq!(seen[1], [evicted(&room, &nick(&bravo), &bravo, &alpha, false)]);
    assert_eq!(
        bravo.model.room_of(&room).unwrap().state,
        NOT_JOINED,
        "server initiated room exit"
    );
    assert!(logged_line(&format!(
        "info chat: leave room={room} uid={B} reason=evicted"
    )));

    // C joins the party, then is kicked without saying anything: the sweep removes it
    t.invite(A, C, &party).await;
    t.accept(C, &party).await;
    join_as(&mut charlie, &room);
    settle(&mut [&mut charlie, &mut alpha]).await;
    assert_eq!(charlie.model.room_of(&room).unwrap().state, JOINED);
    t.kick(A, C);
    t.advance(60 * 1000);
    t.tick().await;
    let seen = settle(&mut [&mut charlie, &mut alpha]).await;
    assert_eq!(seen[0], [evicted(&room, &nick(&charlie), &charlie, &charlie, true)]);
    assert_eq!(seen[1], [evicted(&room, &nick(&charlie), &charlie, &alpha, false)]);

    // D joins, is kicked, and sends its join again: removed from the room and refused
    let mut delta = t.player(D, "Delta").await;
    t.invite(A, D, &party).await;
    t.accept(D, &party).await;
    let join = delta.model.join_public_room(&room, "Delta").unwrap();
    delta.wire.send(&join);
    settle(&mut [&mut delta, &mut alpha]).await;
    t.kick(A, D);
    delta.wire.send(&join);
    let seen = settle(&mut [&mut delta, &mut alpha]).await;
    assert_eq!(seen[0][0], evicted(&room, &nick(&delta), &delta, &delta, true));
    assert!(seen[0][1].contains(FORBIDDEN));
    assert_eq!(seen[1], [evicted(&room, &nick(&delta), &delta, &alpha, false)]);
    finish(t, vec![alpha, bravo, charlie, delta]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn guild_rooms_members_only_a_member_whose_row_is_gone_is_removed_at_the_next_message() {
    let t = lt().await;
    let guild = "7d1f0000-0000-4000-8000-0000000000aa";
    let stamp = metagame::time::now_ms();
    t.app.run(
        "INSERT INTO guilds (guildId, name, nameKey, nameplate, leaderId, createdAt, updatedAt) VALUES (?1, 'Chatters', 'chatters', '', ?2, ?3, ?3)",
        &[&guild, &A, &stamp],
    );
    t.app.run(
        "INSERT INTO guildmembers (accountId, guildId, rank, joinedAt, updatedAt) VALUES (?1, ?3, 'Leader', ?4, ?4), (?2, ?3, 'Member', ?4, ?4)",
        &[&A, &B, &guild, &stamp],
    );

    let room = format!("Guild-{guild}");
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo = t.player(B, "Bravo").await;
    let mut charlie = t.player(C, "Charlie").await;

    join_as(&mut alpha, &room);
    settle(&mut [&mut alpha]).await;
    join_as(&mut bravo, &room);
    settle(&mut [&mut bravo, &mut alpha]).await;
    join_as(&mut charlie, &room);
    settle(&mut [&mut charlie]).await;
    assert_eq!(alpha.model.room_of(&room).unwrap().state, JOINED);
    assert_eq!(bravo.model.room_of(&room).unwrap().state, JOINED);
    assert!(charlie.model.room_of(&room).is_none(), "not a member");

    t.app.run("DELETE FROM guildmembers WHERE accountId = ?1", &[&B]);
    let line = alpha.model.room_message(&room, "marker-guild", "gm1");
    alpha.wire.send(&line);
    let seen = settle(&mut [&mut alpha, &mut bravo]).await;
    assert_eq!(
        seen[1],
        [evicted(&room, &nick(&bravo), &bravo, &bravo, true)],
        "removed, not served"
    );
    assert_eq!(seen[0].len(), 2, "A's own line and B's removal");
    assert!(seen[0].iter().any(|frame| frame.contains("marker-guild")));
    finish(t, vec![alpha, bravo, charlie]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_rooms_the_client_never_names_lobby_other_case_bad_ids_the_9th_room_and_joins_past_the_rate() {
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;

    for room in ["Lobby-x", "party-x", "CITY-x", "City-", "Hunt-a_b", "Whatever"] {
        alpha.wire.send(&room_join(room, &nick(&alpha)));
        let frames = settle(&mut [&mut alpha]).await.remove(0);
        assert!(
            frames[0]
                .contains(r#"<error type="cancel"><not-allowed xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error>"#),
            "{room}"
        );
    }

    // Refused joins take join tokens too; a minute later the burst is full again
    t.advance(61 * 1000);

    for index in 1..=8 {
        let room = format!("City-room{index}");
        join_as(&mut alpha, &room);
        settle(&mut [&mut alpha]).await;
        assert_eq!(alpha.model.room_of(&room).unwrap().state, JOINED);
    }

    join_as(&mut alpha, "City-room9");
    let ninth = settle(&mut [&mut alpha]).await.remove(0);
    assert!(
        ninth[0].contains(
            r#"<error type="wait"><service-unavailable xmlns="urn:ietf:params:xml:ns:xmpp-stanzas"/></error>"#
        )
    );

    // A tenth join fits the burst of 10 joins (after a leave); the eleventh does not
    let exit = alpha.model.exit_room("City-room8").unwrap();
    alpha.wire.send(&exit);
    settle(&mut [&mut alpha]).await;
    join_as(&mut alpha, "City-room10");
    settle(&mut [&mut alpha]).await;
    assert_eq!(alpha.model.room_of("City-room10").unwrap().state, JOINED);
    let exit = alpha.model.exit_room("City-room7").unwrap();
    alpha.wire.send(&exit);
    settle(&mut [&mut alpha]).await;
    join_as(&mut alpha, "City-room11");
    let eleventh = settle(&mut [&mut alpha]).await.remove(0);
    assert!(eleventh[0].contains(r#"<error type="wait"><service-unavailable"#));

    // Each is logged once per connection, room kind and reason
    assert!(logged_line(&format!(
        "info chat: join refused room=Lobby-x uid={A} reason=not-allowed"
    )));
    assert!(!logged_line(&format!(
        "info chat: join refused room=party-x uid={A} reason=not-allowed"
    )));
    assert!(logged_line(&format!(
        "info chat: join refused room=City-room9 uid={A} reason=limit"
    )));
    assert!(!logged_line(&format!(
        "info chat: join refused room=City-room11 uid={A} reason=limit"
    )));
    finish(t, vec![alpha]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn blocks_a_room_line_skips_whoever_blocked_its_sender_whispers_need_neither_player_to_have_blocked_the_other() {
    let t = lt().await;
    let mut alpha = t.player(A, "Alpha").await;
    let mut bravo = t.player(B, "Bravo").await;
    let mut charlie = t.player(C, "Charlie").await;
    let room = "City-blocks";

    for index in 0..3 {
        let each = [&mut alpha, &mut bravo, &mut charlie].into_iter().nth(index).unwrap();
        join_as(each, room);
        settle(&mut [each]).await;
        settle(&mut [&mut alpha, &mut bravo, &mut charlie]).await;
    }

    let (state, me, them) = (t.app.state.clone(), B.to_owned(), A.to_owned());
    let (outcome, event) = t.app.db(move |conn| friends::block(conn, &me, &them)).await;
    assert_eq!(outcome, Outcome::Done("blocked"));
    friends::notify(&state, event);

    let line = alpha.model.room_message(room, "marker-blocked", "b1");
    alpha.wire.send(&line);
    let seen = settle(&mut [&mut alpha, &mut bravo, &mut charlie]).await;
    assert_eq!(seen[0].len(), 1, "A still sees its own line");
    assert_eq!(seen[1].len(), 0, "B blocked A");
    assert_eq!(seen[2].len(), 1);
    assert!(logged_line(&format!(
        "info chat: message room={room} uid={A} len=14 to=2 blocked=1"
    )));

    let line = charlie.model.room_message(room, "marker-free", "b2");
    charlie.wire.send(&line);
    let seen = settle(&mut [&mut charlie, &mut bravo, &mut alpha]).await;
    assert_eq!(seen[1].len(), 1, "C's lines still reach B");

    alpha.wire.send(&format!(
        r#"<message type="chat" to="{B}@{DOMAIN}"><body>marker-w1</body></message>"#
    ));
    bravo.wire.send(&format!(
        r#"<message type="chat" to="{A}@{DOMAIN}"><body>marker-w2</body></message>"#
    ));
    let seen = settle(&mut [&mut alpha, &mut bravo]).await;
    assert_eq!(seen, [Vec::<String>::new(), Vec::new()]);
    assert!(logged_line(&format!(
        "info chat: whisper from={A} to={B} len=9 reason=blocked"
    )));
    assert!(logged_line(&format!(
        "info chat: whisper from={B} to={A} len=9 reason=blocked"
    )));

    let (me, them) = (B.to_owned(), A.to_owned());
    let unblocked = t.app.db(move |conn| friends::unblock(conn, &me, &them)).await;
    assert!(matches!(unblocked, Outcome::Done(_)));

    let line = alpha.model.room_message(room, "marker-again", "b3");
    alpha.wire.send(&line);
    let seen = settle(&mut [&mut alpha, &mut bravo, &mut charlie]).await;
    assert_eq!(seen[1].len(), 1, "after the unblock the lines arrive again");
    finish(t, vec![alpha, bravo, charlie]).await;
}

// ---- logs ----

#[tokio::test(flavor = "multi_thread")]
async fn chat_trace_1_logs_frames_with_the_login_passwords_message_text_and_tokens_redacted() {
    let t = lt_with(|options| options.trace = true).await;
    let sender = t.signed_in(C).await;
    let receiver = t.signed_in(D).await;
    assert!(whispered(&sender, &receiver, "marker-traced-body").await);
    assert!(
        logged_lines()
            .iter()
            .any(|line| line.contains("chat: trace c=") && line.contains("<auth") && line.contains("[redacted]"))
    );
    assert!(
        logged_lines()
            .iter()
            .any(|line| line.contains("chat: trace c=") && line.contains("<body>[18 chars]</body>"))
    );
    sender.logout().await;
    receiver.logout().await;
    t.stop().await;
}

/// Node checked every line its file logged; here a session of logins, refusals, a password, room text,
/// whispers and a gateway secret runs first, and every line logged in the process is checked.
#[tokio::test(flavor = "multi_thread")]
async fn never_logs_a_token_a_login_payload_a_password_message_text_or_the_gateway_secret() {
    let secret: String = (0..24).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    let app = TestApp::with_env(&[("GATEWAY_SECRET", secret.as_str())]).await;
    let t = lt_on(app, |options| options.trace = true).await;
    let mut markers = vec![secret.clone()];

    let refused = t.connected().await;
    let payload = t.plain(C, D).await;
    markers.push(payload.clone());
    sasl_answer(&refused, &payload, "PLAIN").await;
    refused.send(&format!(
        r#"<iq type="set" id="_xmpp_auth1"><query xmlns="jabber:iq:auth"><username>{C}</username><password>not-a-real-password-marker</password><resource>r</resource></query></iq>"#
    ));
    markers.push("not-a-real-password-marker".into());
    refused.closed().await;

    let gateway = WireClient::connect(t.chat.port, &[("X-Dauntless-Gateway", secret.as_str())])
        .await
        .unwrap();
    let mut sender = t.player(C, "Charlie").await;
    let mut receiver = t.player(D, "Delta").await;
    assert!(whispered(&sender.wire, &receiver.wire, "marker-never-logged").await);
    join_as(&mut sender, CITY);
    settle(&mut [&mut sender]).await;
    join_as(&mut receiver, CITY);
    settle(&mut [&mut receiver, &mut sender]).await;
    let line = sender.model.room_message(CITY, "marker-room-never-logged", "n1");
    sender.wire.send(&line);
    settle(&mut [&mut sender, &mut receiver]).await;
    gateway.close();

    let lines = logged_lines();
    let text = lines.join("\n");
    assert!(lines.len() > 20);
    assert!(!text.contains("eyJ"), "no token");
    let shown: Vec<&String> = lines.iter().filter(|line| line.contains("marker-")).collect();
    assert!(shown.is_empty(), "no message text: {shown:?}");
    for marker in &markers {
        assert!(!text.contains(marker.as_str()), "no login payload, password or secret");
    }
    finish(t, vec![sender, receiver]).await;
}
