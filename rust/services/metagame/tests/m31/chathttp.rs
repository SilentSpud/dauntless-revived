//! `chathttp.test.ts`: names through the real account routes (docs/findings/chat.md, "Where each name
//! comes from"): the metagame app and chat for one test app, and client models that read their names
//! the way the 1.4.4 client does. Node served the app on a spare port; here the routes are called in
//! process, which reaches the same handlers.

use serde_json::Value;

use crate::chatclient::{ChatClientModel, JOINED};
use crate::chatwire::{ChatServer, WireClient, login, options};
use crate::common::{TestApp, logged_lines, req};

const A: &str = "UID-chathttp-a";
const B: &str = "UID-chathttp-b";
const ROOM: &str = "City-9e8d7c6b-0000-4000-8000-000000000001";

/// `encodeURIComponent`
fn encode_uri_component(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn get(app: &TestApp, path: &str, caller: Option<&str>) -> Value {
    let request = match caller {
        Some(uid) => req().bearer(uid),
        None => req(),
    };
    app.get(path, request).await.json
}

/// The client's own name: GET /account/api/public/account/<own id> at login; GetPlayerNickname then
/// reads its displayName, or "InvalidMCPUser"
async fn player_nickname(app: &TestApp, uid: &str) -> String {
    let reply = get(app, &format!("/account/api/public/account/{uid}"), Some(uid)).await;
    match reply["displayName"].as_str() {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => "InvalidMCPUser".into(),
    }
}

/// Other players: GET /account/api/public/account?accountId=<id> with the caller's own token, read as an
/// array of {id, displayName}
async fn lookup_as(app: &TestApp, caller: &str, id: &str) -> Option<String> {
    let path = format!("/account/api/public/account?accountId={}", encode_uri_component(id));
    let reply = get(app, &path, Some(caller)).await;
    reply
        .as_array()?
        .iter()
        .find(|entry| entry["id"] == id)?
        .get("displayName")?
        .as_str()
        .map(str::to_owned)
}

async fn setup() -> (TestApp, ChatServer) {
    let app = TestApp::start().await;
    app.run(
        "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, 'Alpha', 0, 0), (?2, 'Bravo', 0, 0)",
        &[&A, &B],
    );
    let mut options = options();
    options.auto_tick = false;
    let chat = ChatServer::start(&app.state, options).await;
    (app, chat)
}

struct Player {
    name: String,
    wire: WireClient,
    model: ChatClientModel,
}

#[tokio::test(flavor = "multi_thread")]
async fn each_players_own_name_comes_from_its_account_route_joins_with_it_and_sees_the_others_name_from_the_lookup() {
    let (app, chat) = setup().await;
    let mut players = Vec::new();
    for uid in [A, B] {
        let name = player_nickname(&app, uid).await;
        let wire = login(&app.state, chat.port, uid).await;
        let model = ChatClientModel::new(uid, &wire.resource, &name, None);
        players.push(Player { name, wire, model });
    }
    assert_eq!(players[0].name, "Alpha");
    assert_eq!(players[1].name, "Bravo");

    // The sender's own connection settles first, so everything it caused is in the others' queues
    for index in 0..players.len() {
        let name = players[index].name.clone();
        let join = players[index].model.join_public_room(ROOM, &name).unwrap();
        players[index].wire.send(&join);
        let order: Vec<usize> = std::iter::once(index)
            .chain((0..players.len()).filter(|other| *other != index))
            .collect();
        for other in order {
            let frames = players[other].wire.barrier().await;
            players[other].model.receive_all(&frames);
        }
    }

    assert_eq!(
        players[0].model.room_of(ROOM).unwrap().state,
        JOINED,
        "the join built from the account route's name is accepted"
    );
    assert_eq!(players[1].model.room_of(ROOM).unwrap().state, JOINED);

    let line = players[0].model.room_message(ROOM, "hello", "g1");
    players[0].wire.send(&line);
    for player in players.iter_mut() {
        let frames = player.wire.barrier().await;
        player.model.receive_all(&frames);
    }

    assert_eq!(
        players[1]
            .model
            .shown_lines_with(async |id: &str| lookup_as(&app, B, id).await)
            .await,
        ["Alpha: hello"]
    );
    assert_eq!(
        players[0]
            .model
            .shown_lines_with(async |id: &str| lookup_as(&app, A, id).await)
            .await,
        ["Alpha: hello"],
        "its own line: the self path"
    );
    assert!(logged_lines().contains(&format!("info Account info for 1 account(s) by userId {B}: 1 found")));
    for player in players {
        player.wire.logout().await;
    }
    chat.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_token_the_per_id_route_answers_and_the_client_falls_back_to_the_nicknames_name_part_which_the_server_checked()
 {
    let (app, chat) = setup().await;
    assert_eq!(
        get(&app, &format!("/account/api/public/account/{A}"), None).await,
        serde_json::json!({})
    );

    let unsigned = async |id: &str| {
        let path = format!("/account/api/public/account/{}", encode_uri_component(id));
        get(&app, &path, None).await["displayName"].as_str().map(str::to_owned)
    };
    let mut bravo = ChatClientModel::new(B, "r", "Bravo", None);
    bravo.join_public_room(ROOM, "Bravo");
    bravo.receive(&format!(
        r#"<presence from="{ROOM}@muc.prod.ol.epicgames.com/Alpha:{A}:rA"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant" jid="{A}@prod.ol.epicgames.com/rA"/></x></presence>"#
    ));
    bravo.receive(&format!(
        r#"<message type="groupchat" from="{ROOM}@muc.prod.ol.epicgames.com/Alpha:{A}:rA"><body>hi</body></message>"#
    ));
    assert_eq!(bravo.shown_lines_with(unsigned).await, ["Alpha: hi"]);
    chat.stop().await;
}

/// Regression: the fleet outgrew the old realm-wide 64-socket ceiling.
#[tokio::test(flavor = "multi_thread")]
async fn admits_100_authenticated_players_on_the_shared_chat_service() {
    let (app, chat) = setup().await;
    let mut wires = Vec::new();
    for i in 0..100 {
        let uid = format!("UID-chat-capacity-{i}");
        let name = format!("Capacity{i}");
        app.run(
            "INSERT INTO users (userId, name, notes, isAdmin) VALUES (?1, ?2, 0, 0)",
            &[&uid, &name],
        );
        wires.push(login(&app.state, chat.port, &uid).await);
    }
    for wire in wires {
        wire.close();
    }
    chat.stop().await;
}
