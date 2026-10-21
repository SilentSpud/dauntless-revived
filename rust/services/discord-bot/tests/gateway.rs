#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The gateway against a local stand-in for Discord's: identify, heartbeats, dispatches, a reconnect
//! that resumes, a fresh session after an invalid one, and a close that ends the bot.

use std::time::Duration;

use discord_bot::gateway::{Dispatch, GUILDS, Gateway};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

type Peer = WebSocketStream<TcpStream>;

async fn accept(listener: &TcpListener) -> Peer {
    let (tcp, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap();
    tokio_tungstenite::accept_async(tcp).await.unwrap()
}

async fn send(peer: &mut Peer, payload: Value) {
    peer.send(Message::Text(payload.to_string().into())).await.unwrap();
}

/// The next payload from the bot that is not a heartbeat.
async fn next(peer: &mut Peer) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            let payload: Value = serde_json::from_str(&text).unwrap();
            if payload["op"] != 1 {
                return payload;
            }
        }
    }
}

async fn received(events: &mut mpsc::UnboundedReceiver<Dispatch>) -> Dispatch {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn identifies_resumes_and_stops() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("ws://{}", listener.local_addr().unwrap());
    let gateway = std::sync::Arc::new(Gateway::new("secret", GUILDS, &base));
    let (events, mut dispatches) = mpsc::unbounded_channel();
    let (stop, stopped) = watch::channel(false);
    let runner = gateway.clone();
    let run = tokio::spawn(async move { runner.run(events, stopped).await });

    let mut peer = accept(&listener).await;
    send(&mut peer, json!({ "op": 10, "d": { "heartbeat_interval": 100 } })).await;
    let identify = next(&mut peer).await;
    assert_eq!(identify["op"], 2);
    assert_eq!(identify["d"]["token"], "secret");
    assert_eq!(identify["d"]["intents"], GUILDS);
    // The heartbeat starts and is acknowledged
    let beat = tokio::time::timeout(Duration::from_secs(5), peer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(beat.to_text().unwrap()).unwrap()["op"], 1);
    send(&mut peer, json!({ "op": 11 })).await;
    send(
        &mut peer,
        json!({ "op": 0, "s": 1, "t": "READY", "d": { "session_id": "abc", "resume_gateway_url": base, "user": { "id": "9" } } }),
    )
    .await;
    assert_eq!(received(&mut dispatches).await.0, "READY");
    send(
        &mut peer,
        json!({ "op": 0, "s": 2, "t": "GUILD_CREATE", "d": { "id": "5" } }),
    )
    .await;
    assert_eq!(
        received(&mut dispatches).await,
        ("GUILD_CREATE".to_owned(), json!({ "id": "5" }))
    );
    assert!(gateway.connected.load(std::sync::atomic::Ordering::SeqCst));

    // Discord asks for a reconnect: the next connection resumes from the last sequence
    send(&mut peer, json!({ "op": 7, "d": null })).await;
    let mut peer = accept(&listener).await;
    send(&mut peer, json!({ "op": 10, "d": { "heartbeat_interval": 45000 } })).await;
    let resume = next(&mut peer).await;
    assert_eq!(
        resume,
        json!({ "op": 6, "d": { "token": "secret", "session_id": "abc", "seq": 2 } })
    );
    send(&mut peer, json!({ "op": 0, "s": 3, "t": "RESUMED", "d": {} })).await;
    assert_eq!(received(&mut dispatches).await.0, "RESUMED");

    // A session that cannot be resumed identifies again
    send(&mut peer, json!({ "op": 9, "d": false })).await;
    let mut peer = accept(&listener).await;
    send(&mut peer, json!({ "op": 10, "d": { "heartbeat_interval": 45000 } })).await;
    assert_eq!(next(&mut peer).await["op"], 2);

    stop.send(true).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), run)
            .await
            .unwrap()
            .unwrap(),
        Ok(())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_token_ends_the_run() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("ws://{}", listener.local_addr().unwrap());
    let gateway = Gateway::new("wrong", GUILDS, &base);
    let (events, _dispatches) = mpsc::unbounded_channel();
    let (_stop, stopped) = watch::channel(false);
    let server = async {
        let mut peer = accept(&listener).await;
        send(&mut peer, json!({ "op": 10, "d": { "heartbeat_interval": 45000 } })).await;
        next(&mut peer).await;
        let frame = CloseFrame {
            code: CloseCode::Library(4004),
            reason: "Authentication failed.".into(),
        };
        peer.close(Some(frame)).await.unwrap();
    };
    let (result, ()) = tokio::join!(gateway.run(events, stopped), server);
    assert_eq!(result, Err(4004));
}
