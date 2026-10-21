#![cfg(feature = "offline")]
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

//! The real stack (L43): this package's launcher binary started as `--service metagame` and
//! `--service deploy` by `ProcessRuntime`, on free loopback ports, ready when their health paths answer,
//! with the game-server key registered in between, and stopped with `shutdown` on stdin.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launcher::offline::layout::OfflineLayout;
use launcher::offline::stack::{ProcessRuntime, ServiceName, ServiceSpec, StackManager, StackState};
use launcher::offline::state;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_launcher_binary_runs_both_services_and_stops_them() {
    let dir = tempfile::tempdir().unwrap();
    let layout = OfflineLayout::new(dir.path(), dir.path());
    let game = dir.path().join("game").join("Dauntless-Win64-Shipping.exe");
    let key_file = state::prepare(&layout, &game, false).unwrap();
    // The offline settings, moved to free ports so the test never meets a real offline stack
    let (metagame_port, deploy_port, chat_port) = (free_port(), free_port(), free_port());
    let moved = |file: &PathBuf, changes: &[(&str, String)]| {
        let mut text = std::fs::read_to_string(file).unwrap();
        for (key, value) in changes {
            let start = text.find(&format!("{key}='")).unwrap();
            let end = start + text[start..].find('\n').unwrap();
            text.replace_range(start..end, &format!("{key}='{value}'"));
        }
        std::fs::write(file, text).unwrap();
    };
    moved(
        &layout.metagame_env_path,
        &[
            ("PORT", metagame_port.to_string()),
            ("CHAT_PORT", chat_port.to_string()),
            ("DEPLOYSERVER_URL", format!("127.0.0.1:{deploy_port}")),
        ],
    );
    moved(&layout.deploy_env_path, &[("PORT", deploy_port.to_string())]);

    let specs = HashMap::from([
        (
            ServiceName::Metagame,
            ServiceSpec {
                env_file: layout.metagame_env_path.clone(),
                port: metagame_port,
                health_path: "/undaunted/api/RegistrationStatus",
                ready_timeout: Duration::from_secs(60),
            },
        ),
        (
            ServiceName::Deploy,
            ServiceSpec {
                env_file: layout.deploy_env_path.clone(),
                port: deploy_port,
                health_path: "/gameservers",
                ready_timeout: Duration::from_secs(30),
            },
        ),
    ]);
    let registered = Arc::new(Mutex::new(false));
    let seen = registered.clone();
    let database = layout.database_path.clone();
    let before_start = Arc::new(move |name: ServiceName| {
        if name == ServiceName::Deploy {
            metagame::dbtools::run(&metagame::dbtools::DbCommand::GsKey {
                db: database.clone(),
                key_file: key_file.clone(),
            })?;
            *seen.lock().unwrap() = true;
        }
        Ok(())
    });
    let runtime = ProcessRuntime::new(
        PathBuf::from(env!("CARGO_BIN_EXE_dauntless-revived-launcher")),
        specs,
        layout.runtime_state_dir.clone(),
        before_start,
    );
    let stack = StackManager::new(Arc::new(runtime));
    let states = Arc::new(Mutex::new(Vec::new()));
    let log = states.clone();
    let started = stack.start(&move |state| log.lock().unwrap().push(state)).await;
    let logs =
        |name: &str| std::fs::read_to_string(layout.runtime_state_dir.join(format!("{name}.log"))).unwrap_or_default();
    assert!(
        started.is_ok(),
        "{started:?}\n--- metagame\n{}\n--- deploy\n{}",
        logs("metagame"),
        logs("deploy")
    );
    assert!(
        *registered.lock().unwrap(),
        "the key is registered once the metagame has migrated"
    );
    assert!(layout.database_path.exists(), "the metagame made its database");
    assert_eq!(
        *states.lock().unwrap(),
        [
            StackState::StartingMetagame,
            StackState::StartingDeploy,
            StackState::Ready
        ]
    );

    stack.stop().await;
    assert_eq!(stack.snapshot().0, StackState::Stopped);
    // Stopped means the ports are free again
    for port in [metagame_port, deploy_port] {
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "port {port} still taken"
        );
    }
}
