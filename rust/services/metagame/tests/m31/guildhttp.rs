//! `guildhttp.test.ts`: guilds over HTTP (roadmap 3.11), with the 1.4.4 client's own bodies and the
//! create as the Ramsgate game server sends it. Every reply is checked twice: the exact JSON, and what
//! the client makes of it (`socialclient`). GUILD_MAX_MEMBERS is 3. Node's file builds one guild world
//! from section to section; here each test builds the stage it starts from (`slayers`, `full`, ...).
//! Node's guild clock is `guild_memory.advance_for_tests`; a switch read once at start is a second
//! app on the same database.

use serde_json::{Value, json};

use crate::common::{Reply, Req, TestApp, req, sha256_hex};
use crate::socialclient::{
    GuildInvite, client_of, guild_outcome, parse_guild_data, parse_guild_invites, parse_phoenix_envelope,
};

const A: &str = "UID-guild-a";
const B: &str = "UID-guild-b";
const C: &str = "UID-guild-c";
const D: &str = "UID-guild-d";
const E: &str = "UID-guild-e";
const F: &str = "UID-guild-f";
const NAMES: [(&str, &str); 6] = [
    (A, "Alpha"),
    (B, "Bravo"),
    (C, "Charlie"),
    (D, "Delta"),
    (E, "Echo"),
    (F, "Foxtrot"),
];
const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;

/// `guildenv.ts`'s settings and the six players, plus `extra`.
async fn guild_app(extra: &[(&str, &str)]) -> TestApp {
    let mut env = vec![
        ("MATCHMAKING_MODE", "DEPLOYSERVER"),
        ("DEPLOYSERVER_URL", "127.0.0.1:1"),
        ("REGISTRATION_MODE", "OPEN"),
        ("QOS_TARGET_URL", "http://127.0.0.1:61000/QoS"),
        ("TARGET_CHANGELIST", "239827"),
        ("GUILD_MAX_MEMBERS", "3"),
    ];
    env.extend_from_slice(extra);
    let app = TestApp::with_env(&env).await;
    for (id, shown) in NAMES {
        add_account(&app, id, shown);
    }
    app
}

/// A second app on `app`'s database with other settings.
async fn same_db(app: &TestApp, extra: &[(&str, &str)]) -> TestApp {
    let db = app.db.to_string_lossy().into_owned();
    let mut env = vec![("GUILD_MAX_MEMBERS", "3"), ("DB_FILENAME", db.as_str())];
    env.extend_from_slice(extra);
    TestApp::with_env(&env).await
}

fn add_account(app: &TestApp, id: &str, shown: &str) {
    app.run(
        "INSERT INTO users (userId, name, notes) VALUES (?1, ?2, 0)",
        &[&id, &shown],
    );
}

fn later(app: &TestApp, ms: i64) {
    app.state.guild_memory.advance_for_tests(ms);
}

// The client's calls

async fn validate(app: &TestApp, who: &str, name: Value, nameplate: Value) -> Reply {
    app.post(
        "/guild/validate",
        req()
            .bearer(who)
            .json(json!({"leader_account_id": who, "name": name, "nameplate": nameplate})),
    )
    .await
}

async fn ok_validate(app: &TestApp, who: &str, name: &str, nameplate: &str) {
    expect_ack(
        &validate(app, who, json!(name), json!(nameplate)).await,
        &format!("{who} validates {name}"),
    );
}

async fn create_with(app: &TestApp, leader: &str, name: Value, nameplate: Value, extra: Req) -> Reply {
    app.post(
        "/guild",
        extra
            .gs()
            .json(json!({"leader_account_id": leader, "name": name, "nameplate": nameplate})),
    )
    .await
}

async fn create(app: &TestApp, leader: &str, name: &str, nameplate: &str) -> Reply {
    create_with(app, leader, json!(name), json!(nameplate), req()).await
}

async fn get_guild(app: &TestApp, who: &str) -> Reply {
    app.get("/guild", req().bearer(who)).await
}

async fn get_invites(app: &TestApp, who: &str) -> Reply {
    app.get("/guild/invite/player", req().bearer(who)).await
}

async fn invite(app: &TestApp, who: &str, target: &str) -> Reply {
    app.call("PUT", &format!("/guild/invite/{target}"), req().bearer(who))
        .await
}

async fn invite_json(app: &TestApp, who: &str, target: &str) -> Reply {
    app.call("PUT", &format!("/guild/invite/{target}"), req().bearer(who).raw(""))
        .await
}

async fn accept(app: &TestApp, who: &str, id: &str) -> Reply {
    app.post(&format!("/guild/invite/accept/{id}"), req().bearer(who).raw(""))
        .await
}

async fn accept_bare(app: &TestApp, who: &str, id: &str) -> Reply {
    app.post(&format!("/guild/invite/accept/{id}"), req().bearer(who)).await
}

async fn decline(app: &TestApp, who: &str, id: &str) -> Reply {
    app.call("DELETE", &format!("/guild/invite/{id}"), req().bearer(who))
        .await
}

async fn leave(app: &TestApp, who: &str) -> Reply {
    app.call("DELETE", "/guild/player", req().bearer(who)).await
}

async fn kick(app: &TestApp, who: &str, target: &str) -> Reply {
    app.call("DELETE", &format!("/guild/player/{target}"), req().bearer(who))
        .await
}

async fn rank(app: &TestApp, who: &str, target: &str, new_rank: &str) -> Reply {
    app.call(
        "PUT",
        &format!("/guild/rank/{target}/{new_rank}"),
        req().bearer(who).raw(""),
    )
    .await
}

async fn disband(app: &TestApp, who: &str, guild: &str) -> Reply {
    app.call("DELETE", &format!("/guild/{guild}"), req().bearer(who)).await
}

async fn party_poll(app: &TestApp, who: &str) {
    app.post(
        "/party",
        req()
            .bearer(who)
            .json(json!({"buildId": "239827_rel-1.4.4_shipping", "featureOverrides": []})),
    )
    .await;
}

// What the replies must be

fn expect_ack(reply: &Reply, what: &str) {
    assert_eq!(reply.status, 200, "{what}: {}", reply.text);
    assert_eq!(
        reply.json,
        json!({"code": "OK", "message": "", "payload": {}}),
        "{what}"
    );
    assert!(
        guild_outcome(reply.status, &reply.text).0,
        "{what}: the client reads a success"
    );
}

/// A refusal: the status, the envelope with the code and an empty payload, and the client's reading
fn expect_refusal(reply: &Reply, status: u16, client_error: &str, what: &str) {
    assert_eq!(reply.status, status, "{what}: {}", reply.text);
    let keys: Vec<&String> = reply.json.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    assert_eq!(keys, ["code", "message", "payload"], "{what}");
    assert!(reply.json["message"].is_string());
    assert_eq!(reply.json["payload"], json!({}));
    let (ok, error, _) = guild_outcome(reply.status, &reply.text);
    assert_eq!((ok, error), (false, client_error), "{what}");
}

/// A guild reply: wrapped, with flat copies of the payload at the root; the client parses it
fn expect_guild(reply: &Reply, expected: &Value, what: &str) -> crate::socialclient::GuildData {
    assert_eq!(reply.status, 200, "{what}: {}", reply.text);
    let mut whole = json!({"code": "OK", "message": "", "payload": expected});
    whole
        .as_object_mut()
        .unwrap()
        .extend(expected.as_object().unwrap().clone());
    assert_eq!(reply.json, whole, "{what}");
    let envelope = parse_phoenix_envelope(reply.status, &reply.text);
    assert!(envelope.ok, "{what}");
    parse_guild_data(&envelope.payload).unwrap()
}

fn guild_object(id: &str, name: &str, nameplate: &str, leader: &str, members: &[(&str, &str)]) -> Value {
    let members: Vec<Value> = members
        .iter()
        .map(|(account, rank)| json!({"phx_account_id": account, "rank": rank}))
        .collect();
    json!({"id": id, "name": name, "nameplate": nameplate, "leader_account_id": leader, "members": members, "maximum_guild_members": 3})
}

async fn own_invites(app: &TestApp, who: &str) -> Vec<GuildInvite> {
    let reply = get_invites(app, who).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json["invites"], reply.json["payload"]["invites"], "flat copy");
    parse_guild_invites(&parse_phoenix_envelope(reply.status, &reply.text).payload).unwrap()
}

async fn first_invite(app: &TestApp, who: &str) -> String {
    own_invites(app, who).await[0].id.clone()
}

fn guild_id(reply: &Reply) -> String {
    reply.json["payload"]["id"].as_str().unwrap_or_default().to_owned()
}

// The stages Node's sections build on

/// A validated and created Slayers (SLY), A its leader.
async fn slayers(app: &TestApp) -> String {
    ok_validate(app, A, "Slayers", "SLY").await;
    let created = create(app, A, "Slayers", "SLY").await;
    assert_eq!(created.status, 200, "{}", created.text);
    guild_id(&created)
}

/// Echoes (ECH), E its leader.
async fn echoes(app: &TestApp) -> String {
    ok_validate(app, E, "Echoes", "ECH").await;
    let created = create(app, E, "Echoes", "ECH").await;
    assert_eq!(created.status, 200, "{}", created.text);
    guild_id(&created)
}

/// Section 5's end: Slayers full with A, B and C (Members), Echoes, and A's invite to D open.
async fn full(app: &TestApp) -> String {
    let id = slayers(app).await;
    expect_ack(&invite(app, A, B).await, "A invites B");
    expect_ack(&accept(app, B, &first_invite(app, B).await).await, "B accepts");
    echoes(app).await;
    expect_ack(&invite(app, A, C).await, "A invites C");
    expect_ack(&invite(app, A, D).await, "A invites D");
    expect_ack(&accept(app, C, &first_invite(app, C).await).await, "C joins");
    id
}

/// Section 6's hand-over: B leads Slayers, A is an Officer, C a Member.
async fn handed_over(app: &TestApp) -> String {
    let id = full(app).await;
    expect_ack(&rank(app, A, B, "officer").await, "B is an Officer");
    expect_ack(&rank(app, A, B, "leader").await, "hand-over");
    id
}

#[tokio::test(flavor = "multi_thread")]
async fn get_guild_is_204_with_no_body_the_client_clears_its_guild_get_guild_invite_player_is_the_wrapped_empty_list() {
    let app = guild_app(&[]).await;
    let mine = get_guild(&app, A).await;
    assert_eq!((mine.status, mine.text.as_str()), (204, ""));
    assert!(!parse_phoenix_envelope(mine.status, &mine.text).ok, "not in a guild");
    let invites = get_invites(&app, A).await;
    assert_eq!(invites.status, 200);
    assert_eq!(
        invites.text,
        json!({"code": "OK", "message": "", "payload": {"invites": []}, "invites": []}).to_string()
    );
    assert_eq!(
        parse_guild_invites(&parse_phoenix_envelope(invites.status, &invites.text).payload),
        Some(vec![])
    );
    assert_eq!(
        app.get("/guild", req().gs()).await.status,
        204,
        "the game-server key alone: no guild"
    );
    assert_eq!(
        app.get("/guild/invite/player", req().gs()).await.json["invites"],
        json!([])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn every_name_and_nameplate_rule_answers_its_code_valid_pairs_answer_the_ack_and_create_nothing() {
    let app = guild_app(&[]).await;
    let cases: [(Value, Value, u16, &str); 16] = [
        (json!("Abc"), json!("SLY"), 400, "GuildNameInvalidLength"),
        (json!("Abcdefghijklmnop"), json!(""), 400, "GuildNameInvalidLength"),
        (json!("Ab cd"), json!(""), 400, "GuildNameInvalidLength"),
        (json!(" Slayers"), json!(""), 400, "GuildNameInvalidLength"),
        (json!("Slayers!"), json!(""), 400, "GuildNameInvalidLength"),
        (Value::Null, json!(""), 400, "GuildNameInvalidLength"),
        (json!(12345), json!(""), 400, "GuildNameInvalidLength"),
        (json!("Guild1234567"), json!(""), 400, "GuildNameTooManyNumbers"),
        (json!("Gaaaaaaa"), json!(""), 400, "GuildNameTooManyLetters"),
        (json!("GaAaAaAa"), json!(""), 400, "GuildNameTooManyLetters"),
        (json!("Sh1tHeads"), json!(""), 400, "GuildNameProfane"),
        (json!("Slayers"), json!("ABCDEFG"), 400, "GuildNameplateInvalidLength"),
        (json!("Slayers"), json!("A"), 400, "GuildNameplateInvalidLength"),
        (json!("Slayers"), json!("A-B"), 400, "GuildNameplateInvalidLength"),
        (json!("Slayers"), json!(42), 400, "GuildNameplateInvalidLength"),
        (json!("Slayers"), json!("P0RN"), 400, "GuildNameplateProfane"),
    ];
    for (name, nameplate, status, error) in cases {
        // Node's undefined name is a missing key
        let reply = if name.is_null() {
            app.post(
                "/guild/validate",
                req()
                    .bearer(A)
                    .json(json!({"leader_account_id": A, "nameplate": nameplate})),
            )
            .await
        } else {
            validate(&app, A, name.clone(), nameplate.clone()).await
        };
        expect_refusal(&reply, status, error, &format!("{name} / {nameplate}"));
    }
    for (name, nameplate) in [
        ("Guild123456", json!("")),
        ("Gaaaaaa", json!("AB")),
        ("Slayers", json!("SLY")),
        ("Slayers", json!("")),
        ("Slayers", Value::Null),
        ("ABCDEFGHIJKLMNO", json!("ABCDEF")),
    ] {
        expect_ack(
            &validate(&app, A, json!(name), nameplate.clone()).await,
            &format!("{name} / {nameplate}"),
        );
    }
    expect_ack(
        &app.post("/guild/validate", req().bearer(A).json(json!({"name": "Slayers"})))
            .await,
        "no nameplate key at all",
    );
    assert_eq!(get_guild(&app, A).await.status, 204, "validating creates nothing");
    assert_eq!(app.count("guilds", "1=1", &[]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn guild_name_denylist_adds_words_digit_swaps_included() {
    let listed = guild_app(&[("GUILD_NAME_DENYLIST", "grief, b4d")]).await;
    expect_refusal(
        &validate(&listed, A, json!("GriefGuild"), json!("")).await,
        400,
        "GuildNameProfane",
        "a listed word",
    );
    expect_refusal(
        &validate(&listed, A, json!("Bad1Guild"), json!("")).await,
        400,
        "GuildNameProfane",
        "b4d matches bad",
    );
    expect_refusal(
        &validate(&listed, A, json!("Fine"), json!("GRIEF")).await,
        400,
        "GuildNameplateProfane",
        "in the nameplate",
    );
    let app = guild_app(&[]).await;
    expect_ack(
        &validate(&app, A, json!("GriefGuild"), json!("")).await,
        "without the list",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn staff_and_project_words_are_already_in_use_short_offensive_tags_are_profane_guild_reserved_names_0_frees_only_the_reserved_words()
 {
    let app = guild_app(&[]).await;
    let reserved = [
        ("ServerAdmins", "", 409, "GuildNameTaken"),
        ("Moderators", "", 409, "GuildNameTaken"),
        ("TheOfficials", "", 409, "GuildNameTaken"),
        ("St4ffOnly", "", 409, "GuildNameTaken"),
        ("TheGameMasters", "", 409, "GuildNameTaken"),
        ("Dauntless", "", 409, "GuildNameTaken"),
        ("Phoenix", "", 409, "GuildNameTaken"),
        ("Slayers", "GM", 409, "GuildNameplateTaken"),
        ("Slayers", "dev", 409, "GuildNameplateTaken"),
        ("Slayers", "ADMIN", 409, "GuildNameplateTaken"),
        ("Slayers", "M0D", 409, "GuildNameplateTaken"),
        ("Slayers", "KKK", 400, "GuildNameplateProfane"),
        ("Slayers", "FAG", 400, "GuildNameplateProfane"),
        ("Slayers", "SS", 400, "GuildNameplateProfane"),
        ("Slayers", "1488", 400, "GuildNameplateProfane"),
        ("KkkRiders", "", 400, "GuildNameProfane"),
    ];
    for (name, nameplate, status, error) in reserved {
        expect_refusal(
            &validate(&app, A, json!(name), json!(nameplate)).await,
            status,
            error,
            &format!("{name} / {nameplate}"),
        );
    }
    for (name, nameplate) in [
        ("DauntlessCrew", ""),
        ("PhoenixRising", ""),
        ("Slayers", "NIGHT"),
        ("Slayers", "GMX"),
        ("Slayers", "DEVS1"),
    ] {
        expect_ack(
            &validate(&app, A, json!(name), json!(nameplate)).await,
            &format!("{name} / {nameplate}: only whole words are reserved there"),
        );
    }
    let free = guild_app(&[("GUILD_RESERVED_NAMES", "0")]).await;
    expect_ack(
        &validate(&free, A, json!("Moderators"), json!("")).await,
        "reserved words allowed",
    );
    expect_ack(
        &validate(&free, A, json!("Slayers"), json!("GM")).await,
        "reserved nameplates allowed",
    );
    expect_refusal(
        &validate(&free, A, json!("Slayers"), json!("KKK")).await,
        400,
        "GuildNameplateProfane",
        "offensive tags stay refused",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn needs_a_player_s_token() {
    let app = guild_app(&[]).await;
    let body = json!({"leader_account_id": A, "name": "Slayers", "nameplate": ""});
    assert_eq!(app.post("/guild/validate", req().json(body.clone())).await.status, 401);
    assert_eq!(app.post("/guild/validate", req().gs().json(body)).await.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_player_s_token_and_a_leader_who_did_not_validate_this_name_403_with_no_code_which_the_client_shows_as_unable_to_create_guild()
 {
    let app = guild_app(&[]).await;
    let player_only = app
        .post(
            "/guild",
            req()
                .bearer(D)
                .json(json!({"leader_account_id": D, "name": "Deltas", "nameplate": ""})),
        )
        .await;
    expect_refusal(&player_only, 403, "Unknown", "a player's token alone");
    expect_refusal(
        &create(&app, D, "Deltas", "").await,
        403,
        "Unknown",
        "D never validated",
    );
    party_poll(&app, D).await;
    expect_refusal(
        &create(&app, D, "Deltas", "").await,
        403,
        "Unknown",
        "being online (a party poll a moment ago) is not enough",
    );
    expect_refusal(
        &create(&app, D, "x", "").await,
        400,
        "GuildNameInvalidLength",
        "a name the rules refuse keeps its own text",
    );
    expect_refusal(
        &create(&app, D, "Deltas", "GM").await,
        409,
        "GuildNameplateTaken",
        "so does a reserved nameplate",
    );
    assert_eq!(get_guild(&app, D).await.status, 204);
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_s_validate_creates_the_guild_and_answers_it_in_full_a_s_get_guild_answers_the_same() {
    let app = guild_app(&[]).await;
    ok_validate(&app, A, "Slayers", "SLY").await;
    let created = create(&app, A, "Slayers", "SLY").await;
    let id = guild_id(&created);
    assert!(
        regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
            .unwrap()
            .is_match(&id),
        "{id}"
    );
    let parsed = expect_guild(
        &created,
        &guild_object(&id, "Slayers", "SLY", A, &[(A, "Leader")]),
        "create",
    );
    let keys: Vec<&String> = created.json["payload"].as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "id",
            "name",
            "nameplate",
            "leader_account_id",
            "members",
            "maximum_guild_members"
        ]
    );
    assert!(created.json["payload"]["maximum_guild_members"].is_number());
    assert_eq!(
        (parsed.leader.as_str(), parsed.members.clone(), parsed.max),
        (A, vec![(A.to_owned(), "Leader".to_owned())], 3)
    );
    assert_eq!(get_guild(&app, A).await.text, created.text);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cannot_validate_or_create_a_second_guild_and_names_and_nameplates_are_taken_regardless_of_case() {
    let app = guild_app(&[]).await;
    slayers(&app).await;
    party_poll(&app, A).await;
    expect_refusal(
        &validate(&app, A, json!("Another"), json!("")).await,
        409,
        "YouAlreadyInAGuild",
        "validate while in a guild",
    );
    expect_refusal(
        &validate(&app, A, json!("x"), json!("")).await,
        409,
        "YouAlreadyInAGuild",
        "checked before the name rules",
    );
    expect_refusal(
        &create(&app, A, "Another", "").await,
        403,
        "Unknown",
        "no validate of that name (a member of a guild gets none)",
    );
    expect_refusal(
        &validate(&app, B, json!("sLAYERS"), json!("")).await,
        409,
        "GuildNameTaken",
        "name",
    );
    expect_refusal(
        &validate(&app, B, json!("Others"), json!("sly")).await,
        409,
        "GuildNameplateTaken",
        "nameplate",
    );
    expect_ack(
        &validate(&app, B, json!("Others"), json!("")).await,
        "an empty nameplate is never taken",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_create_must_be_what_the_leader_validated_another_name_another_nameplate_or_another_leader_is_refused_the_last_five_pairs_count_in_any_case()
 {
    let app = guild_app(&[]).await;
    ok_validate(&app, B, "Bravos", "BRV").await;
    expect_refusal(
        &create(&app, B, "Other", "BRV").await,
        403,
        "Unknown",
        "another name for B",
    );
    expect_refusal(
        &create(&app, B, "Bravos", "XYZ").await,
        403,
        "Unknown",
        "another nameplate",
    );
    expect_refusal(&create(&app, B, "Bravos", "").await, 403, "Unknown", "no nameplate");
    expect_refusal(
        &create(&app, C, "Bravos", "BRV").await,
        403,
        "Unknown",
        "B's name for C, who validated nothing",
    );
    assert_eq!(get_guild(&app, C).await.status, 204);
    for name in [
        "BravoOne",
        "BravoTwo",
        "BravoThree",
        "BravoFour",
        "BravoFive",
        "BravoSix",
    ] {
        ok_validate(&app, B, name, "").await;
    }
    expect_refusal(
        &create(&app, B, "BravoOne", "").await,
        403,
        "Unknown",
        "only the last five validated pairs are kept",
    );
    expect_refusal(&create(&app, B, "Bravos", "BRV").await, 403, "Unknown", "older still");
    let created = create(&app, B, "bravotwo", "").await;
    assert_eq!(created.status, 200, "{}", created.text);
    assert_eq!(
        created.json["payload"]["name"], "bravotwo",
        "the name as the create sends it"
    );
    expect_ack(&disband(&app, B, &guild_id(&created)).await, "B disbands");
    expect_refusal(
        &create(&app, B, "BravoThree", "").await,
        403,
        "Unknown",
        "a create uses up the validated names",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_leader_is_400_a_token_that_comes_along_is_only_logged_it_is_the_game_server_s_own_a_bad_one_never_causes_a_500()
 {
    let app = guild_app(&[]).await;
    slayers(&app).await;
    expect_refusal(
        &create(&app, "UID-nobody", "Nobodies", "").await,
        400,
        "Unknown",
        "no such account",
    );
    expect_refusal(
        &create(&app, "not an id!", "Nobodies", "").await,
        400,
        "Unknown",
        "not an account id",
    );
    ok_validate(&app, C, "Charlies", "CHR").await;
    let with_other = create_with(&app, C, json!("Charlies"), json!("CHR"), req().bearer(A)).await;
    assert_eq!(with_other.status, 200, "{}", with_other.text);
    assert_eq!(
        with_other.json["payload"]["leader_account_id"], C,
        "A's token changes nothing"
    );
    expect_ack(&disband(&app, C, &guild_id(&with_other)).await, "C disbands");
    ok_validate(&app, E, "Echoing", "").await;
    let bad_token = create_with(
        &app,
        E,
        json!("Echoing"),
        json!(""),
        req().header("authorization", "bearer not.a.token"),
    )
    .await;
    assert_eq!(bad_token.status, 200, "{}", bad_token.text);
    expect_ack(&disband(&app, E, &guild_id(&bad_token)).await, "E disbands");
    let leaders: Vec<Value> = app
        .rows("SELECT accountId, rank FROM guildmembers", &[])
        .iter()
        .map(|r| json!([r["accountId"], r["rank"]]))
        .collect();
    assert_eq!(leaders, [json!([A, "Leader"])]);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_guild_per_leader_per_10_minutes_429_even_with_a_validated_name() {
    let app = guild_app(&[]).await;
    ok_validate(&app, B, "BravoTwo", "").await;
    let first = create(&app, B, "BravoTwo", "").await;
    expect_ack(&disband(&app, B, &guild_id(&first)).await, "B's first guild, disbanded");
    ok_validate(&app, B, "BravosTwo", "").await;
    expect_refusal(
        &create(&app, B, "BravosTwo", "").await,
        429,
        "Unknown",
        "a second guild within 10 minutes",
    );
    later(&app, 10 * 60 * 1000 + 1);
    let again = create(&app, B, "BravosTwo", "").await;
    assert_eq!(again.status, 200, "{}", again.text);
    expect_ack(&disband(&app, B, &guild_id(&again)).await, "disband");
}

#[tokio::test(flavor = "multi_thread")]
async fn guild_create_activity_fallback_1_also_accepts_a_leader_heard_from_in_the_last_minute_nobody_else() {
    let fallback = guild_app(&[("GUILD_CREATE_ACTIVITY_FALLBACK", "1")]).await;
    party_poll(&fallback, D).await;
    let created = create(&fallback, D, "Deltas", "").await;
    assert_eq!(created.status, 200, "{}", created.text);
    expect_ack(&disband(&fallback, D, &guild_id(&created)).await, "disband");
    expect_refusal(
        &create(&fallback, F, "Foxtrots", "").await,
        403,
        "Unknown",
        "F never validated and was never heard from",
    );
    let app = guild_app(&[]).await;
    party_poll(&app, D).await;
    expect_refusal(&create(&app, D, "Deltas", "").await, 403, "Unknown", "off again");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_invites_b_b_s_list_shows_it_with_the_guild_s_name_and_the_inviter_whom_b_s_client_sets_up_by_id() {
    let app = guild_app(&[]).await;
    let id = slayers(&app).await;
    expect_ack(&invite(&app, A, B).await, "invite without a content type");
    let raw = get_invites(&app, B).await;
    let invites = own_invites(&app, B).await;
    assert_eq!(invites.len(), 1);
    let keys: Vec<&String> = raw.json["payload"]["invites"][0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["id", "guild_id", "guild_name", "inviter_account_id"]);
    assert_eq!(
        (
            invites[0].guild_id.as_str(),
            invites[0].guild_name.as_str(),
            invites[0].inviter.as_str()
        ),
        (id.as_str(), "Slayers", A)
    );
    let mut client_b = client_of(&app, B).login().await;
    assert_eq!(client_b.resolve_from_phoenix(&invites[0].inviter).await, "initialised");
    assert_eq!(client_b.name_of(A), Some("Alpha"));
    assert!(own_invites(&app, A).await.is_empty(), "the inviter sees nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_second_invite_redundant_oneself_cloned_an_unknown_account_and_a_block_either_way() {
    let app = guild_app(&[]).await;
    slayers(&app).await;
    expect_ack(&invite(&app, A, B).await, "A invites B");
    expect_refusal(
        &invite_json(&app, A, B).await,
        409,
        "TargetAlreadyHasGuildInvite",
        "again",
    );
    expect_refusal(&invite(&app, A, A).await, 409, "TargetAlreadyInYourGuild", "oneself");
    expect_refusal(&invite(&app, A, "UID-nobody").await, 404, "Unknown", "unknown account");
    expect_refusal(&invite(&app, C, D).await, 404, "NotInAGuild", "C has no guild");
    assert_eq!(
        app.post(&format!("/friends/api/public/blocklist/{D}/{A}"), req().bearer(D))
            .await
            .status,
        204
    );
    expect_refusal(&invite(&app, A, D).await, 403, "Unknown", "D blocked A");
    assert_eq!(
        app.call(
            "DELETE",
            &format!("/friends/api/public/blocklist/{D}/{A}"),
            req().bearer(D)
        )
        .await
        .status,
        204
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn b_accepts_a_s_invite_b_is_a_member_and_every_other_invite_b_had_is_gone() {
    let app = guild_app(&[]).await;
    let id = slayers(&app).await;
    expect_ack(&invite(&app, A, B).await, "A invites B");
    echoes(&app).await;
    expect_ack(&invite(&app, E, B).await, "E invites B");
    let both = own_invites(&app, B).await;
    assert_eq!(
        both.iter().map(|i| i.guild_name.as_str()).collect::<Vec<_>>(),
        ["Echoes", "Slayers"],
        "newest first"
    );
    expect_ack(&accept(&app, B, &both[1].id).await, "accept, empty JSON body");
    assert!(own_invites(&app, B).await.is_empty());
    expect_guild(
        &get_guild(&app, B).await,
        &guild_object(&id, "Slayers", "SLY", A, &[(A, "Leader"), (B, "Member")]),
        "B's guild",
    );
    expect_refusal(
        &accept(&app, B, &both[0].id).await,
        404,
        "GuildInviteNotFound",
        "E's invite went with the accept",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn c_declines_nobody_accepts_or_declines_another_player_s_invite_a_member_of_a_guild_cannot_accept_another() {
    let app = guild_app(&[]).await;
    slayers(&app).await;
    expect_ack(&invite(&app, A, B).await, "A invites B");
    expect_ack(&accept(&app, B, &first_invite(&app, B).await).await, "B joins Slayers");
    echoes(&app).await;
    expect_ack(&invite(&app, E, C).await, "E invites C");
    let for_c = first_invite(&app, C).await;
    expect_refusal(
        &accept_bare(&app, D, &for_c).await,
        404,
        "GuildInviteNotFound",
        "D accepting C's invite",
    );
    expect_refusal(
        &decline(&app, D, &for_c).await,
        404,
        "GuildInviteNotFound",
        "D declining C's invite",
    );
    expect_ack(&decline(&app, C, &for_c).await, "decline, no body");
    assert!(own_invites(&app, C).await.is_empty());
    expect_refusal(
        &decline(&app, C, &for_c).await,
        404,
        "GuildInviteNotFound",
        "declined twice",
    );
    expect_ack(&invite(&app, E, B).await, "a player in another guild may be invited");
    let for_b = first_invite(&app, B).await;
    expect_refusal(
        &accept(&app, B, &for_b).await,
        409,
        "YouAlreadyInAGuild",
        "B is in Slayers",
    );
    expect_ack(&decline(&app, B, &for_b).await, "B declines");
    expect_refusal(
        &accept(&app, C, "not-an-invite").await,
        404,
        "GuildInviteNotFound",
        "a made-up id",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invite_expires_after_guild_invite_ttl_days_7_it_leaves_the_list_and_cannot_be_accepted() {
    let app = guild_app(&[]).await;
    slayers(&app).await;
    expect_ack(&invite(&app, A, C).await, "A invites C");
    let old = first_invite(&app, C).await;
    later(&app, 7 * DAY + 60 * 1000);
    assert!(own_invites(&app, C).await.is_empty());
    expect_refusal(&accept(&app, C, &old).await, 404, "GuildInviteNotFound", "expired");
    expect_ack(&invite(&app, A, C).await, "a new invite replaces the expired one");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_guild_is_full_at_guild_max_members_3_accepting_and_inviting_answer_409_guildisfull() {
    let app = guild_app(&[]).await;
    let id = full(&app).await;
    expect_refusal(
        &accept(&app, D, &first_invite(&app, D).await).await,
        409,
        "GuildIsFull",
        "D's accept",
    );
    expect_refusal(&invite(&app, A, F).await, 409, "GuildIsFull", "a new invite");
    let members = [(A, "Leader"), (B, "Member"), (C, "Member")];
    let parsed = expect_guild(
        &get_guild(&app, A).await,
        &guild_object(&id, "Slayers", "SLY", A, &members),
        "full",
    );
    assert_eq!(
        parsed.max - parsed.members.len() as i64,
        0,
        "the client shows 0 open positions"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_promotes_and_demotes_rank_in_any_case_members_cannot_invite_officers_can() {
    let app = guild_app(&[]).await;
    let id = full(&app).await;
    expect_refusal(&invite(&app, C, F).await, 403, "InvalidPermission", "a Member invites");
    expect_ack(&rank(&app, A, B, "officer").await, "Promote To Guild Officer");
    expect_refusal(
        &invite(&app, B, F).await,
        409,
        "GuildIsFull",
        "an Officer may invite (the guild is full)",
    );
    expect_ack(&rank(&app, A, C, "OFFICER").await, "upper case");
    let officers = [(A, "Leader"), (B, "Officer"), (C, "Officer")];
    expect_guild(
        &get_guild(&app, C).await,
        &guild_object(&id, "Slayers", "SLY", A, &officers),
        "two officers",
    );
    expect_ack(&rank(&app, A, C, "Member").await, "Demote To Guild Member");
    expect_ack(
        &rank(&app, A, B, "officer").await,
        "the same rank again changes nothing",
    );
    expect_refusal(
        &rank(&app, A, C, "captain").await,
        400,
        "InvalidGuildRank",
        "no such rank",
    );
    expect_refusal(
        &rank(&app, B, C, "officer").await,
        403,
        "InvalidPermission",
        "an Officer changes ranks",
    );
    expect_refusal(
        &rank(&app, A, A, "member").await,
        403,
        "InvalidPermission",
        "the leader's own rank",
    );
    expect_refusal(&rank(&app, A, D, "officer").await, 404, "NotInAGuild", "not a member");
    expect_refusal(
        &rank(&app, D, A, "member").await,
        404,
        "NotInAGuild",
        "the caller has no guild",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn promote_to_guild_leader_hands_the_guild_over_the_old_leader_becomes_an_officer() {
    let app = guild_app(&[]).await;
    let id = full(&app).await;
    expect_ack(&rank(&app, A, B, "officer").await, "B is an Officer");
    expect_ack(&rank(&app, A, B, "leader").await, "hand-over");
    let members = [(B, "Leader"), (A, "Officer"), (C, "Member")];
    let after = expect_guild(
        &get_guild(&app, A).await,
        &guild_object(&id, "Slayers", "SLY", B, &members),
        "after",
    );
    assert_eq!(after.leader, B);
    assert_eq!(
        app.row("SELECT leaderId FROM guilds WHERE guildId = ?1", &[&id])["leaderId"],
        B
    );
    expect_refusal(
        &rank(&app, A, C, "officer").await,
        403,
        "InvalidPermission",
        "the old leader no longer changes ranks",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn everything_is_read_back_from_the_database_after_the_in_memory_state_is_cleared_a_restart() {
    let app = guild_app(&[]).await;
    handed_over(&app).await;
    let before = get_guild(&app, C).await.text;
    app.state.guild_memory.reset_for_tests();
    assert_eq!(get_guild(&app, C).await.text, before);
    assert_eq!(own_invites(&app, D).await.len(), 1, "D's invite survived too");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_cannot_leave_an_officer_cannot_kick_a_member_leaves_through_delete_guild_player_not_the_disband_route()
 {
    let app = guild_app(&[]).await;
    let id = handed_over(&app).await;
    expect_refusal(
        &leave(&app, B).await,
        409,
        "GuildLeaderCannotLeaveGuild",
        "the leader leaves",
    );
    expect_refusal(
        &kick(&app, B, B).await,
        409,
        "GuildLeaderCannotLeaveGuild",
        "the leader kicks themself",
    );
    expect_refusal(&kick(&app, A, C).await, 403, "InvalidPermission", "an Officer kicks");
    expect_ack(&leave(&app, C).await, "Leave Guild");
    assert_eq!(get_guild(&app, C).await.status, 204);
    expect_guild(
        &get_guild(&app, B).await,
        &guild_object(&id, "Slayers", "SLY", B, &[(B, "Leader"), (A, "Officer")]),
        "the guild is still there",
    );
    expect_refusal(&leave(&app, C).await, 404, "NotInAGuild", "leaving again");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_leader_kicks_a_kick_of_someone_outside_the_guild_is_404_a_member_naming_themself_leaves() {
    let app = guild_app(&[]).await;
    handed_over(&app).await;
    expect_ack(&leave(&app, C).await, "C leaves");
    expect_ack(&kick(&app, B, A).await, "Kick From Guild");
    assert_eq!(get_guild(&app, A).await.status, 204);
    expect_refusal(&kick(&app, B, D).await, 404, "NotInAGuild", "not a member");
    expect_refusal(&kick(&app, C, B).await, 404, "NotInAGuild", "the caller has no guild");
    expect_ack(&invite(&app, B, A).await, "B invites A back");
    expect_ack(&accept(&app, A, &first_invite(&app, A).await).await, "A accepts");
    expect_ack(&kick(&app, A, A).await, "a Member naming themself leaves");
    assert_eq!(get_guild(&app, A).await.status, 204);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_leader_disbands_only_their_own_guild_afterwards_nobody_is_in_it_its_invites_are_gone_and_the_name_is_free()
 {
    let app = guild_app(&[]).await;
    let id = handed_over(&app).await;
    expect_ack(&leave(&app, C).await, "C leaves");
    expect_ack(&kick(&app, B, A).await, "A is kicked");
    expect_ack(&invite(&app, B, A).await, "B invites A");
    expect_ack(&accept(&app, A, &first_invite(&app, A).await).await, "A accepts");
    expect_ack(&invite(&app, B, C).await, "B invites C (left open)");
    expect_refusal(
        &disband(&app, A, &id).await,
        403,
        "InvalidPermission",
        "a Member disbands",
    );
    expect_refusal(
        &disband(&app, B, &metagame::crypto::random_uuid()).await,
        404,
        "NotInAGuild",
        "a wrong id",
    );
    expect_refusal(&disband(&app, C, &id).await, 404, "NotInAGuild", "not a member");
    expect_ack(&disband(&app, B, &id).await, "DISBAND GUILD");
    for who in [A, B] {
        assert_eq!(get_guild(&app, who).await.status, 204, "{who}");
    }
    assert!(own_invites(&app, C).await.is_empty());
    assert!(own_invites(&app, D).await.is_empty());
    assert_eq!(app.count("guildinvites", "guildId = ?1", &[&id]), 0);
    expect_ack(
        &validate(&app, A, json!("slayers"), json!("SLY")).await,
        "the name and nameplate are free again",
    );
}

/// Foxes (FOX), F its leader.
async fn foxes(app: &TestApp) -> String {
    ok_validate(app, F, "Foxes", "FOX").await;
    let created = create(app, F, "Foxes", "FOX").await;
    assert_eq!(created.status, 200, "{}", created.text);
    guild_id(&created)
}

#[tokio::test(flavor = "multi_thread")]
async fn t_30_invites_per_inviter_per_hour_then_at_most_50_open_invites_per_guild_429() {
    let app = guild_app(&[]).await;
    foxes(&app).await;
    let many: Vec<String> = (0..52).map(|i| format!("UID-guild-many-{i:02}")).collect();
    for (index, id) in many.iter().enumerate() {
        add_account(&app, id, &format!("Many{index}"));
    }
    for (index, target) in many[..30].iter().enumerate() {
        expect_ack(&invite(&app, F, target).await, &format!("invite {}", index + 1));
    }
    expect_refusal(
        &invite(&app, F, &many[30]).await,
        429,
        "Unknown",
        "the 31st within an hour",
    );
    later(&app, HOUR + 1);
    for (index, target) in many[30..50].iter().enumerate() {
        expect_ack(
            &invite(&app, F, target).await,
            &format!("invite {}, an hour later", index + 31),
        );
    }
    expect_refusal(
        &invite(&app, F, &many[50]).await,
        429,
        "Unknown",
        "the 51st open invite",
    );
    expect_ack(
        &decline(&app, &many[0], &first_invite(&app, &many[0]).await).await,
        "one declines",
    );
    expect_ack(&invite(&app, F, &many[50]).await, "room again");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_keeps_at_most_20_open_invites_the_oldest_gives_way() {
    let app = guild_app(&[]).await;
    let target = "UID-guild-target";
    add_account(&app, target, "Target");
    let mut leaders = Vec::new();
    for index in 0..21u8 {
        let leader = format!("UID-guild-lead-{index:02}");
        add_account(&app, &leader, &format!("Lead{index}"));
        let name = format!("Band{}Guild", (b'A' + index) as char);
        ok_validate(&app, &leader, &name, "").await;
        assert_eq!(create(&app, &leader, &name, "").await.status, 200);
        later(&app, 1000);
        expect_ack(&invite(&app, &leader, target).await, &format!("invite {}", index + 1));
        leaders.push(leader);
    }
    let open = own_invites(&app, target).await;
    assert_eq!(open.len(), 20);
    assert_eq!(open[0].inviter, leaders[20], "newest first");
    assert!(!open.iter().any(|i| i.inviter == leaders[0]), "the oldest was dropped");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_token_is_401_and_the_game_server_key_alone_is_403_on_every_client_route_the_reads_answer_only_the_token_s_own_data()
 {
    let app = guild_app(&[]).await;
    foxes(&app).await;
    echoes(&app).await;
    let routes = [
        ("POST", "/guild/validate".to_owned()),
        ("PUT", format!("/guild/invite/{A}")),
        ("POST", "/guild/invite/accept/x".to_owned()),
        ("DELETE", "/guild/invite/x".to_owned()),
        ("DELETE", "/guild/player".to_owned()),
        ("DELETE", format!("/guild/player/{A}")),
        ("PUT", format!("/guild/rank/{A}/officer")),
        ("DELETE", "/guild/x".to_owned()),
    ];
    for (method, path) in &routes {
        assert_eq!(
            app.call(method, path, req().raw("")).await.status,
            401,
            "{method} {path}"
        );
        assert_eq!(
            app.call(method, path, req().gs().raw("")).await.status,
            403,
            "{method} {path}"
        );
    }
    assert_eq!(app.get("/guild", req()).await.status, 401);
    assert_eq!(app.get("/guild/invite/player", req()).await.status, 401);
    assert_eq!(
        app.get("/guild", req().header("authorization", "bearer not.a.token"))
            .await
            .status,
        401
    );
    let mine = get_guild(&app, F).await;
    assert_eq!(mine.json["payload"]["leader_account_id"], F);
    assert_eq!(
        get_guild(&app, E).await.json["payload"]["name"],
        "Echoes",
        "each token its own guild"
    );
    assert_eq!(
        app.get("/guild", req().gs().bearer(F)).await.text,
        mine.text,
        "a game server forwarding F's token reads F's guild"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn brings_back_the_old_stubs_and_every_other_guild_route_404s() {
    let app = guild_app(&[]).await;
    foxes(&app).await;
    let off = same_db(&app, &[("GUILDS", "0")]).await;
    let mine = get_guild(&off, F).await;
    assert_eq!((mine.status, mine.text.as_str()), (204, ""));
    assert_eq!(
        get_invites(&off, A).await.text,
        json!({"code": null, "message": "OK", "payload": {"invites": []}}).to_string()
    );
    let routes = [
        (
            "POST",
            "/guild/validate".to_owned(),
            req()
                .bearer(A)
                .json(json!({"leader_account_id": A, "name": "Slayers", "nameplate": ""})),
        ),
        (
            "POST",
            "/guild".to_owned(),
            req()
                .gs()
                .json(json!({"leader_account_id": A, "name": "Slayers", "nameplate": ""})),
        ),
        ("PUT", format!("/guild/invite/{B}"), req().bearer(F)),
        ("POST", "/guild/invite/accept/x".to_owned(), req().bearer(A)),
        ("DELETE", "/guild/invite/x".to_owned(), req().bearer(A)),
        ("DELETE", "/guild/player".to_owned(), req().bearer(F)),
        ("DELETE", format!("/guild/player/{A}"), req().bearer(F)),
        ("PUT", format!("/guild/rank/{A}/officer"), req().bearer(F)),
        ("DELETE", "/guild/x".to_owned(), req().bearer(F)),
    ];
    for (method, path, request) in routes {
        let reply = off.call(method, &path, request).await;
        assert_eq!((reply.status, reply.text.as_str()), (404, ""), "{method} {path}");
    }
    assert_eq!(get_guild(&app, F).await.status, 200, "and back on");
    assert_eq!(
        app.count("guilds", "name = 'Foxes'", &[]),
        1,
        "nothing was lost while off"
    );
}

const ADMIN: &str = "UID-guild-admin";

/// Section 11's setting: Foxes and Echoes, the admin, and keys for F, D and the admin.
async fn host_app() -> (TestApp, [String; 3]) {
    let app = guild_app(&[]).await;
    foxes(&app).await;
    echoes(&app).await;
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, 'Boss', 0, 1)",
        &[&ADMIN],
    );
    let keys = [F, D, ADMIN].map(|who| {
        let key = format!("UUK_{}", metagame::crypto::random_uuid().replace('-', ""));
        app.run(
            "INSERT INTO userapikeys (userId, keyHash) VALUES (?1, ?2)",
            &[&who, &sha256_hex(&key)],
        );
        key
    });
    (app, keys)
}

fn key(value: &str) -> Req {
    req().header("x-undaunted-user-api-key", value)
}

#[tokio::test(flavor = "multi_thread")]
async fn guildinvite_the_key_s_owner_invites_by_name_an_admin_may_name_the_inviter_directly_only() {
    let (app, [f_key, d_key, admin_key]) = host_app().await;
    let host_invite = |k: &str, body: Value| app.post("/undaunted/api/GuildInvite", key(k).json(body));
    let by_key = host_invite(&f_key, json!({"Username": "delta"})).await;
    assert_eq!(
        (by_key.status, by_key.json),
        (200, json!({"From": "Foxtrot", "To": "Delta", "Guild": "Foxes"}))
    );
    let listed: Vec<(String, String)> = own_invites(&app, D)
        .await
        .into_iter()
        .map(|i| (i.guild_name, i.inviter))
        .collect();
    assert_eq!(listed, [("Foxes".to_owned(), F.to_owned())]);
    let again = host_invite(&f_key, json!({"Username": "Delta"})).await;
    assert_eq!(
        (again.status, again.json["error"].clone()),
        (409, json!("guild_refused"))
    );
    assert!(
        again.json["message"]
            .as_str()
            .unwrap()
            .starts_with("RedundantAdorableQuillshot: ")
    );
    let no_guild = host_invite(&d_key, json!({"Username": "Foxtrot"})).await;
    assert_eq!(
        (no_guild.status, no_guild.json["error"].clone()),
        (404, json!("guild_refused")),
        "D has no guild"
    );
    assert_eq!(
        host_invite(&d_key, json!({"Username": "Echo", "From": "Foxtrot"}))
            .await
            .status,
        403,
        "not an admin"
    );
    let for_f = host_invite(&admin_key, json!({"Username": "Echo", "From": F})).await;
    assert_eq!(
        (for_f.status, for_f.json),
        (200, json!({"From": "Foxtrot", "To": "Echo", "Guild": "Foxes"}))
    );
    let proxied = app
        .post(
            "/undaunted/api/GuildInvite",
            key(&admin_key)
                .header("x-forwarded-for", "203.0.113.9")
                .json(json!({"Username": "Echo", "From": F})),
        )
        .await;
    assert_eq!(proxied.status, 403, "admin through a proxy");
    assert_eq!(host_invite(&f_key, json!({"Username": "Nobody"})).await.status, 404);
    assert_eq!(
        app.post("/undaunted/api/GuildInvite", req().json(json!({"Username": "Delta"})))
            .await
            .status,
        401
    );
    let broken = app.post("/undaunted/api/GuildInvite", key(&f_key).raw("{nope")).await;
    assert_eq!(
        broken.json,
        json!({"error": "bad_request", "message": "The request body is not valid JSON."})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn guilds_and_disbandguild_admin_only_disband_by_name_or_id_removes_members_and_invites() {
    let (app, [f_key, _, admin_key]) = host_app().await;
    app.post(
        "/undaunted/api/GuildInvite",
        key(&f_key).json(json!({"Username": "Delta"})),
    )
    .await;
    assert_eq!(app.get("/undaunted/api/Guilds", key(&f_key)).await.status, 403);
    let list = app.get("/undaunted/api/Guilds", key(&admin_key)).await;
    assert_eq!(list.status, 200);
    let found = |name: &str, list: &Value| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|g| g["name"] == name)
            .unwrap()
            .clone()
    };
    let foxes = found("Foxes", &list.json);
    assert_eq!(
        foxes,
        json!({"guildId": foxes["guildId"], "name": "Foxes", "nameplate": "FOX", "leader": F, "members": 1})
    );
    assert_eq!(
        app.post(
            "/undaunted/api/DisbandGuild",
            key(&f_key).json(json!({"Guild": "Foxes"}))
        )
        .await
        .status,
        403
    );
    assert_eq!(
        app.post(
            "/undaunted/api/DisbandGuild",
            key(&admin_key).json(json!({"Guild": "NoSuchGuild"}))
        )
        .await
        .json["error"],
        "not_found"
    );
    let done = app
        .post(
            "/undaunted/api/DisbandGuild",
            key(&admin_key).json(json!({"Guild": "fOXES"})),
        )
        .await;
    assert_eq!((done.status, done.json), (200, json!({"Guild": "Foxes", "Members": 1})));
    assert_eq!(get_guild(&app, F).await.status, 204);
    assert!(own_invites(&app, D).await.is_empty(), "its invites went with it");
    let echoes = found("Echoes", &app.get("/undaunted/api/Guilds", key(&admin_key)).await.json);
    let by_id = app
        .post(
            "/undaunted/api/DisbandGuild",
            key(&admin_key).json(json!({"Guild": echoes["guildId"]})),
        )
        .await;
    assert_eq!(by_id.json, json!({"Guild": "Echoes", "Members": 1}));
}

#[tokio::test(flavor = "multi_thread")]
async fn guilds_0_turns_them_off_too() {
    let (app, [_, _, admin_key]) = host_app().await;
    let off = same_db(&app, &[("GUILDS", "0")]).await;
    for (method, path, body) in [
        ("POST", "/undaunted/api/GuildInvite", Some(json!({"Username": "Delta"}))),
        ("POST", "/undaunted/api/DisbandGuild", Some(json!({"Guild": "x"}))),
        ("GET", "/undaunted/api/Guilds", None),
    ] {
        let request = match body {
            Some(body) => key(&admin_key).json(body),
            None => key(&admin_key),
        };
        let reply = off.call(method, path, request).await;
        assert_eq!(
            (reply.status, reply.json["error"].clone()),
            (404, json!("guilds_off")),
            "{path}"
        );
    }
}

const L: &str = "UID-guild-l1";
const O: &str = "UID-guild-o1";
const P: &str = "UID-guild-p1";
const Q: &str = "UID-guild-p2";
const R: &str = "UID-guild-p3";

/// Section 12's setting: Lancers (LNC), L its leader, and O, P, Q, R.
async fn lancers() -> (TestApp, String) {
    let app = guild_app(&[]).await;
    for (id, shown) in [(L, "Lancer"), (O, "Oscar"), (P, "Papa"), (Q, "Quebec"), (R, "Romeo")] {
        add_account(&app, id, shown);
    }
    ok_validate(&app, L, "Lancers", "LNC").await;
    let created = create(&app, L, "Lancers", "LNC").await;
    assert_eq!(created.status, 200, "{}", created.text);
    let id = guild_id(&created);
    (app, id)
}

fn old_invite(app: &TestApp, guild: &str, invitee: &str, inviter: &str) -> String {
    let id = metagame::crypto::random_uuid();
    let now = app.state.guild_memory.now();
    let expires = now + HOUR;
    app.run(
        "INSERT INTO guildinvites (inviteId, guildId, inviteeId, inviterId, createdAt, expiresAt) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        &[&id, &guild, &invitee, &inviter, &now, &expires],
    );
    id
}

#[tokio::test(flavor = "multi_thread")]
async fn a_block_removes_the_guild_invites_between_the_two_an_older_invite_from_a_blocked_inviter_is_never_listed_and_cannot_be_accepted()
 {
    let (app, lancers) = lancers().await;
    expect_ack(&invite(&app, L, P).await, "L invites P");
    assert_eq!(own_invites(&app, P).await.len(), 1);
    assert_eq!(
        app.post(&format!("/friends/api/public/blocklist/{P}/{L}"), req().bearer(P))
            .await
            .status,
        204
    );
    assert!(own_invites(&app, P).await.is_empty(), "gone with the block");
    assert_eq!(
        app.count("guildinvites", "inviteeId = ?1", &[&P]),
        0,
        "deleted, not only hidden"
    );
    let blocked = invite(&app, L, P).await;
    expect_refusal(&blocked, 403, "Unknown", "no new invite while blocked");
    assert_eq!(
        blocked.json["message"], "The invite could not be sent.",
        "the reply does not say why"
    );
    // An invite written before the block existed (as after an update): hidden, and accepting it is 404
    let old = old_invite(&app, &lancers, P, L);
    assert!(own_invites(&app, P).await.is_empty());
    expect_refusal(&accept(&app, P, &old).await, 404, "GuildInviteNotFound", "blocked");
    assert_eq!(app.count("guildinvites", "inviteId = ?1", &[&old]), 0, "and removed");
    assert_eq!(
        app.call(
            "DELETE",
            &format!("/friends/api/public/blocklist/{P}/{L}"),
            req().bearer(P)
        )
        .await
        .status,
        204
    );
    expect_ack(&invite(&app, L, P).await, "unblocked: invites work again");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guild_whose_invite_was_declined_cannot_invite_that_player_again_for_24_hours() {
    let (app, _) = lancers().await;
    expect_ack(&invite(&app, L, P).await, "L invites P");
    expect_ack(&decline(&app, P, &first_invite(&app, P).await).await, "P declines");
    expect_refusal(&invite(&app, L, P).await, 429, "Unknown", "at once");
    later(&app, 23 * HOUR);
    expect_refusal(&invite(&app, L, P).await, 429, "Unknown", "23 hours later");
    later(&app, HOUR + 1);
    expect_ack(&invite(&app, L, P).await, "a day later");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_officer_s_invites_go_when_the_officer_is_demoted_kicked_or_leaves_an_invite_whose_inviter_lost_the_right_cannot_be_accepted()
 {
    let (app, lancers) = lancers().await;
    expect_ack(&invite(&app, L, P).await, "the leader's own invite to P");
    expect_ack(&invite(&app, L, O).await, "L invites O");
    expect_ack(&accept(&app, O, &first_invite(&app, O).await).await, "O joins");
    expect_ack(&rank(&app, L, O, "officer").await, "O is an Officer");

    expect_ack(&invite(&app, O, Q).await, "the Officer invites Q");
    assert_eq!(own_invites(&app, Q).await.len(), 1);
    expect_ack(&rank(&app, L, O, "member").await, "demoted");
    assert!(own_invites(&app, Q).await.is_empty(), "demoted: the invite is gone");

    expect_ack(&rank(&app, L, O, "officer").await, "an Officer again");
    expect_ack(&invite(&app, O, Q).await, "invites Q again");
    expect_ack(&kick(&app, L, O).await, "kicked");
    assert!(own_invites(&app, Q).await.is_empty(), "kicked: gone");

    expect_ack(&invite(&app, L, O).await, "back in");
    expect_ack(&accept(&app, O, &first_invite(&app, O).await).await, "O joins again");
    expect_ack(&rank(&app, L, O, "officer").await, "Officer");
    expect_ack(&invite(&app, O, Q).await, "invites Q a third time");
    expect_ack(&leave(&app, O).await, "O leaves");
    assert!(own_invites(&app, Q).await.is_empty(), "left: gone");
    assert_eq!(
        own_invites(&app, P).await.len(),
        1,
        "the leader's own invite to P stays"
    );

    // A row whose inviter is only a Member (as if written before this rule): hidden, and 404 on accept
    expect_ack(&invite(&app, L, O).await, "O comes back as a Member");
    expect_ack(&accept(&app, O, &first_invite(&app, O).await).await, "joins");
    let stale = old_invite(&app, &lancers, R, O);
    assert!(own_invites(&app, R).await.is_empty());
    expect_refusal(
        &accept(&app, R, &stale).await,
        404,
        "GuildInviteNotFound",
        "the inviter is only a Member",
    );
    assert_eq!(get_guild(&app, R).await.status, 204);
}
