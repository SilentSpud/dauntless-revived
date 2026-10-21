//! `shutdown.test.ts`: stopping the metagame is idempotent and releases both listeners, at any point
//! of its start. The Node test held start-up at its chat and HTTP steps with injected gates; the Rust
//! binary has none, so the modes here are a stop before it listens, a stop with a chat client and an
//! unfinished HTTP request open, and a stop with chat off. "shutdown" is sent twice each time.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn spawn(dir: &std::path::Path, port: u16, chat_port: u16, chat: bool) -> Child {
    let keys: serde_json::Value = serde_json::from_str(include_str!("../fixtures/node-crypto.json")).unwrap();
    let env_file = dir.join("metagame.env");
    std::fs::write(
        &env_file,
        format!(
            "PORT={port}\nBIND_HOST=127.0.0.1\nNODE_ENV=production\nAUTH_MODE=APIKEY\nDB_FILENAME={}\nAUTH_SIGNING_PRIVKEY_B64={}\nAUTH_SIGNING_PUBKEY_B64={}\nCHAT={}\nCHAT_PORT={chat_port}\nCHAT_BIND_HOST=127.0.0.1\n",
            dir.join("test.db").to_string_lossy().replace('\\', "/"),
            keys["privB64"].as_str().unwrap(),
            keys["pubB64"].as_str().unwrap(),
            if chat { "1" } else { "0" },
        ),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_metagame"))
        .args(["serve", "--env-file"])
        .arg(&env_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Waits for the line that says the HTTP listener is up.
fn wait_listening(child: &mut Child) -> BufReader<std::process::ChildStdout> {
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        if stdout.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if line.contains("Dauntless Revived metagame on 127.0.0.1:") {
            return stdout;
        }
    }
    panic!("the metagame never logged where it listens");
}

fn stop(child: &mut Child) {
    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(b"shutdown\n").unwrap();
    // A second request while stopping changes nothing (the pipe may already be closed)
    let _ = stdin.write_all(b"shutdown\n");
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "stopped within the drain deadline"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn assert_free(ports: &[u16]) {
    for port in ports {
        std::net::TcpListener::bind(("127.0.0.1", *port)).unwrap_or_else(|e| panic!("port {port} still taken: {e}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn metagame_shutdown_is_idempotent_and_releases_listeners_mode() {
    for mode in ["early", "ready", "chat-disabled"] {
        let dir = tempfile::tempdir().unwrap();
        let (port, chat_port) = (free_port(), free_port());
        let mut child = spawn(dir.path(), port, chat_port, mode != "chat-disabled");
        match mode {
            "early" => stop(&mut child),
            "ready" => {
                let _stdout = wait_listening(&mut child);
                let (mut chat, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{chat_port}/"))
                    .await
                    .unwrap();
                let mut http = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                http.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n").unwrap();
                stop(&mut child);
                // The chat client was told goodbye and its connection ended; so did the HTTP one
                let ended = tokio::time::timeout(Duration::from_secs(5), async {
                    use futures_util::StreamExt;
                    while let Some(Ok(_)) = chat.next().await {}
                })
                .await;
                assert!(ended.is_ok(), "{mode}: the chat connection stayed open");
                http.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut rest = Vec::new();
                let _ = std::io::Read::read_to_end(&mut http, &mut rest);
            }
            _ => {
                let _stdout = wait_listening(&mut child);
                stop(&mut child);
            }
        }
        assert_free(&[port, chat_port]);
    }
}
