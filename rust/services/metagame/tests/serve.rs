#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! `metagame serve` as the kits and the launcher run it (MIGRATION_PLAN.md X7): settings from
//! `--env-file`, pino lines on stdout, readiness by HTTP on loopback, `shutdown` on stdin stops it with
//! exit code 0.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn starts_answers_and_stops_on_shutdown() {
    let keys: serde_json::Value = serde_json::from_str(include_str!("fixtures/node-crypto.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let env_file = dir.path().join("metagame.env");
    std::fs::write(
        &env_file,
        format!(
            "PORT=\nBIND_HOST=127.0.0.1\nNODE_ENV=production\nAUTH_MODE=APIKEY\nREGISTRATION_MODE=OPEN\nDB_FILENAME={}\nAUTH_SIGNING_PRIVKEY_B64={}\nAUTH_SIGNING_PUBKEY_B64={}\n",
            dir.path().join("undaunted.db").to_string_lossy().replace('\\', "/"),
            keys["privB64"].as_str().unwrap(),
            keys["pubB64"].as_str().unwrap()
        ),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_metagame"))
        .args(["serve", "--env-file"])
        .arg(&env_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // The listen line gives the port the system picked.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut port = None;
    let mut line = String::new();
    while port.is_none() && Instant::now() < deadline {
        line.clear();
        if stdout.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let entry: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(
            entry["level"].is_u64() && entry["time"].is_u64() && entry["pid"].is_u64(),
            "pino fields: {line}"
        );
        if let Some(rest) = entry["msg"]
            .as_str()
            .and_then(|msg| msg.strip_prefix("Dauntless Revived metagame on 127.0.0.1:"))
        {
            port = rest.parse::<u16>().ok();
        }
    }
    let port = port.expect("the metagame logged where it listens");

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "GET /undaunted/api/RegistrationStatus HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut stream, &mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.ends_with(r#"{"RegistrationMode":"OPEN"}"#), "{reply}");

    child.stdin.as_mut().unwrap().write_all(b"shutdown\n").unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stopped within the drain deadline"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
