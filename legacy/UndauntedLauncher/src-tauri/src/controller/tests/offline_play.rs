//! Offline play through the controller (`controller.test.ts`'s offline cases), with a stand-in stack in front
//! of the fake host, which plays the local metagame here (open registration, no invite code).

use std::sync::{Arc, Mutex};

use super::*;
use crate::bridge::types::OfflinePhase;
use crate::controller::offline::OfflineSetup;
use crate::offline::layout::OfflineLayout;
use crate::offline::save::PortableSave;
use crate::offline::stack::{Boxed, Runtime, ServiceHandle, ServiceName, StackManager};
use crate::platform::keystore::KeyStore;

#[derive(Default)]
struct Stand {
    events: Mutex<Vec<String>>,
    /// The metagame becomes ready only once this is notified
    gate: Option<Arc<tokio::sync::Notify>>,
}

struct StandHandle(ServiceName, Arc<Stand>);

impl ServiceHandle for StandHandle {
    fn stop(self: Box<Self>) -> Boxed<()> {
        self.1.events.lock().unwrap().push(format!("stop {}", self.0.as_str()));
        Box::pin(async {})
    }
}

struct StandRuntime(Arc<Stand>);

impl Runtime for StandRuntime {
    fn start_service(&self, name: ServiceName) -> Boxed<Result<Box<dyn ServiceHandle>, String>> {
        self.0.events.lock().unwrap().push(format!("start {}", name.as_str()));
        let stand = self.0.clone();
        Box::pin(async move { Ok(Box::new(StandHandle(name, stand)) as Box<dyn ServiceHandle>) })
    }

    fn wait_until_ready(&self, name: ServiceName) -> Boxed<Result<(), String>> {
        let gate = self.0.gate.clone().filter(|_| name == ServiceName::Metagame);
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(())
        })
    }
}

struct Offline {
    hx: Harness,
    meta: Shared,
    stand: Arc<Stand>,
    port: u16,
}

async fn offline(free_memory: Option<fn() -> f64>, gate: Option<Arc<tokio::sync::Notify>>) -> Offline {
    let meta = host(&[]);
    meta.lock().unwrap().open_registration = true;
    let port = serve(meta.clone(), None).await;
    let user = tempfile::tempdir().unwrap();
    let stand = Arc::new(Stand {
        gate,
        ..Stand::default()
    });
    let layout = OfflineLayout::new(user.path(), user.path());
    let keys = KeyStore::new(user.path().join("keys"), Arc::new(Plain), None, None);
    let setup = OfflineSetup {
        stack: Arc::new(StackManager::new(Arc::new(StandRuntime(stand.clone())))),
        save: Arc::new(PortableSave::new(layout, user.path().to_path_buf(), keys)),
        free_memory,
        port,
    };
    let hx = Harness::build(free_port().await, user, Some(setup));
    // The game is installed
    for (path, data) in files() {
        let file = path.split('/').fold(hx.install.path().to_path_buf(), |p, s| p.join(s));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, data).unwrap();
    }
    hx.c.init().await;
    Offline { hx, meta, stand, port }
}

fn events(stand: &Stand) -> Vec<String> {
    stand.events.lock().unwrap().clone()
}

fn phase(hx: &Harness) -> OfflinePhase {
    hx.c.offline_status().phase
}

#[tokio::test(flavor = "multi_thread")]
async fn offline_play_registers_once_stores_its_key_and_reuses_the_account() {
    let Offline { hx, meta, stand, port } = offline(None, None).await;

    // First start: the stack runs and the player picks a name
    assert_eq!(hx.c.start_offline(None).await, ok());
    assert_eq!(phase(&hx), OfflinePhase::NeedsUsername);
    assert_eq!(events(&stand), ["start metagame", "start deploy"]);
    assert_eq!(
        hx.c.play().await,
        failed(ErrorCode::Busy),
        "online play waits for the offline stack"
    );
    assert_eq!(
        hx.c.offline_save(true).await,
        failed(ErrorCode::Busy),
        "no save while the stack runs"
    );

    let refused = hx.c.start_offline(Some("x".into())).await;
    assert_eq!(code_of(&refused), Some(ErrorCode::UsernameInvalid));
    assert_eq!(
        phase(&hx),
        OfflinePhase::NeedsUsername,
        "the stack keeps running for another name"
    );

    assert_eq!(hx.c.start_offline(Some("Offline_1".into())).await, ok());
    assert_eq!(phase(&hx), OfflinePhase::Running);
    assert_eq!(meta.lock().unwrap().registrations.len(), 1);
    assert_eq!(
        events(&stand),
        ["start metagame", "start deploy"],
        "the running stack is reused"
    );
    let (_, args, _) = hx.spawns.lock().unwrap()[0].clone();
    assert_eq!(args[0], format!("127.0.0.1:{port}"));
    assert_eq!(hx.c.keep_open(), Some(KeepOpen::Offline));
    assert_eq!(
        hx.c.stop_offline().await,
        failed(ErrorCode::Busy),
        "not while the game runs"
    );

    // The game exits: the stack stops with it
    hx.exit(0).await;
    assert_eq!(phase(&hx), OfflinePhase::Stopped);
    assert_eq!(
        events(&stand),
        ["start metagame", "start deploy", "stop deploy", "stop metagame"]
    );
    assert_eq!(hx.c.keep_open(), None);

    // Next time the stored key signs in: no second registration
    assert_eq!(hx.c.start_offline(None).await, ok());
    assert_eq!(phase(&hx), OfflinePhase::Running);
    assert_eq!(meta.lock().unwrap().registrations.len(), 1);
    hx.exit(0).await;
    hx.c.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn too_little_memory_is_refused_before_the_stack_starts() {
    let Offline { hx, stand, .. } = offline(Some(|| 1024.0 * 1024.0 * 1024.0), None).await;
    let refused = hx.c.start_offline(None).await;
    assert_eq!(
        refused,
        ActionResult::Err(LauncherError {
            code: ErrorCode::OfflineInsufficientMemory,
            detail: Some("4608".into())
        })
    );
    assert_eq!(phase(&hx), OfflinePhase::Error);
    assert!(events(&stand).is_empty());
    hx.c.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_offline_cancels_a_stack_that_is_still_starting() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let Offline { hx, stand, .. } = offline(None, Some(gate.clone())).await;
    let start = {
        let c = hx.c.clone();
        tokio::spawn(async move { c.start_offline(None).await })
    };
    while !events(&stand).contains(&"start metagame".to_owned()) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(hx.c.stop_offline().await, ok());
    assert_eq!(phase(&hx), OfflinePhase::Stopping);
    gate.notify_one();
    assert_eq!(start.await.unwrap(), ok());
    assert_eq!(phase(&hx), OfflinePhase::Stopped);
    assert_eq!(
        events(&stand),
        ["start metagame", "start deploy", "stop deploy", "stop metagame"]
    );
    assert!(hx.spawns.lock().unwrap().is_empty(), "no game after a cancel");
    hx.c.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_waits_for_a_stack_that_is_still_starting() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let Offline { hx, stand, .. } = offline(None, Some(gate.clone())).await;
    let start = {
        let c = hx.c.clone();
        tokio::spawn(async move { c.start_offline(None).await })
    };
    while !events(&stand).contains(&"start metagame".to_owned()) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let shutdown = {
        let c = hx.c.clone();
        tokio::spawn(async move { c.shutdown().await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!shutdown.is_finished(), "shutdown waits for the start");
    gate.notify_one();
    shutdown.await.unwrap();
    let _ = start.await.unwrap();
    assert_eq!(
        events(&stand),
        ["start metagame", "start deploy", "stop deploy", "stop metagame"]
    );
    assert_eq!(phase(&hx), OfflinePhase::Stopped);
}
