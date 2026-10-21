//! The local offline stack (`local-stack.ts`, `local-stack-runtime.ts`, `local-stack-factory.ts`): the
//! metagame, then the DeployServer, each this binary started as `--service` (D12) and ready when its health
//! path answers 200 on loopback. Deploy stops first, since it uses the metagame. Only processes this stack
//! started are ever stopped: never one found by name.
//!
//! `StackManager` is the order and the state; a `Runtime` does the starting, so tests run the manager with
//! stand-ins, as `local-stack.test.ts` did.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt as _;
use tokio::process::{Child, Command};

use super::layout::OfflineLayout;
use crate::net::http::{Endpoint, RequestOptions, request};
use crate::platform::log;

pub type Boxed<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceName {
    Metagame,
    Deploy,
}

impl ServiceName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metagame => "metagame",
            Self::Deploy => "deploy",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "metagame" => Some(Self::Metagame),
            "deploy" => Some(Self::Deploy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackState {
    Stopped,
    StartingMetagame,
    StartingDeploy,
    Ready,
    Stopping,
    Failed,
}

/// One started service; stopping it ends exactly that process.
pub trait ServiceHandle: Send {
    fn stop(self: Box<Self>) -> Boxed<()>;
}

pub trait Runtime: Send + Sync {
    fn start_service(&self, name: ServiceName) -> Boxed<Result<Box<dyn ServiceHandle>, String>>;
    fn wait_until_ready(&self, name: ServiceName) -> Boxed<Result<(), String>>;
}

struct Inner {
    state: StackState,
    error: Option<String>,
    metagame: Option<Box<dyn ServiceHandle>>,
    deploy: Option<Box<dyn ServiceHandle>>,
}

pub struct StackManager {
    runtime: Arc<dyn Runtime>,
    inner: tokio::sync::Mutex<Inner>,
    state: Mutex<(StackState, Option<String>)>,
}

impl StackManager {
    pub fn new(runtime: Arc<dyn Runtime>) -> Self {
        Self {
            runtime,
            inner: tokio::sync::Mutex::new(Inner {
                state: StackState::Stopped,
                error: None,
                metagame: None,
                deploy: None,
            }),
            state: Mutex::new((StackState::Stopped, None)),
        }
    }

    /// The state and the last start's error.
    pub fn snapshot(&self) -> (StackState, Option<String>) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(&self, inner: &mut Inner, state: StackState, on_state: &(dyn Fn(StackState) + Send + Sync)) {
        inner.state = state;
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = (state, inner.error.clone());
        on_state(state);
    }

    /// Starts the metagame, then Deploy; `on_state` hears each step.
    pub async fn start(&self, on_state: &(dyn Fn(StackState) + Send + Sync)) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        if !matches!(inner.state, StackState::Stopped | StackState::Failed) {
            return Err("local stack is already active".into());
        }
        inner.error = None;
        let outcome: Result<(), String> = async {
            self.set(&mut inner, StackState::StartingMetagame, on_state);
            inner.metagame = Some(self.runtime.start_service(ServiceName::Metagame).await?);
            self.runtime.wait_until_ready(ServiceName::Metagame).await?;
            self.set(&mut inner, StackState::StartingDeploy, on_state);
            inner.deploy = Some(self.runtime.start_service(ServiceName::Deploy).await?);
            self.runtime.wait_until_ready(ServiceName::Deploy).await
        }
        .await;
        match outcome {
            Ok(()) => {
                self.set(&mut inner, StackState::Ready, on_state);
                Ok(())
            }
            Err(error) => {
                Self::cleanup(&mut inner).await;
                inner.error = Some(error.clone());
                self.set(&mut inner, StackState::Failed, on_state);
                Err(error)
            }
        }
    }

    pub async fn stop(&self) {
        let mut inner = self.inner.lock().await;
        if inner.state == StackState::Stopped {
            return;
        }
        let quiet = |_: StackState| {};
        self.set(&mut inner, StackState::Stopping, &quiet);
        Self::cleanup(&mut inner).await;
        inner.error = None;
        self.set(&mut inner, StackState::Stopped, &quiet);
    }

    /// Deploy first: it uses the metagame.
    async fn cleanup(inner: &mut Inner) {
        if let Some(deploy) = inner.deploy.take() {
            deploy.stop().await;
        }
        if let Some(metagame) = inner.metagame.take() {
            metagame.stop().await;
        }
    }
}

// ---------------------------------------------------------------- the real runtime

/// What a service needs: its settings file, its loopback port and health path, how long it may take.
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub env_file: PathBuf,
    pub port: u16,
    pub health_path: &'static str,
    pub ready_timeout: Duration,
}

pub type BeforeStart = Arc<dyn Fn(ServiceName) -> Result<(), String> + Send + Sync>;

/// Starts `program --service <name> --env-file <file>` with only the operating system's plumbing in its
/// environment, so shell settings cannot change authentication, addresses or the database.
pub struct ProcessRuntime {
    program: PathBuf,
    specs: HashMap<ServiceName, ServiceSpec>,
    log_dir: PathBuf,
    before_start: BeforeStart,
    children: Arc<Mutex<HashMap<ServiceName, Arc<tokio::sync::Mutex<Child>>>>>,
}

/// `localServiceEnvironment`: the variables a service may inherit.
pub fn service_environment() -> Vec<(String, String)> {
    const ALLOWED: [&str; 19] = [
        "PATH",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "SYSTEMDRIVE",
        "HOMEDRIVE",
        "HOMEPATH",
        "HOME",
        "TMPDIR",
        "XDG_RUNTIME_DIR",
    ];
    std::env::vars()
        .filter(|(key, _)| ALLOWED.contains(&key.to_ascii_uppercase().as_str()))
        .collect()
}

impl ProcessRuntime {
    pub fn new(
        program: PathBuf,
        specs: HashMap<ServiceName, ServiceSpec>,
        log_dir: PathBuf,
        before_start: BeforeStart,
    ) -> Self {
        Self {
            program,
            specs,
            log_dir,
            before_start,
            children: Arc::default(),
        }
    }

    /// The real stack: the two services on 61000 and 61001, the metagame with time for a first migration.
    pub fn offline(program: PathBuf, layout: &OfflineLayout, before_start: BeforeStart) -> Self {
        let specs = HashMap::from([
            (
                ServiceName::Metagame,
                ServiceSpec {
                    env_file: layout.metagame_env_path.clone(),
                    port: 61000,
                    health_path: "/undaunted/api/RegistrationStatus",
                    ready_timeout: Duration::from_secs(90),
                },
            ),
            (
                ServiceName::Deploy,
                ServiceSpec {
                    env_file: layout.deploy_env_path.clone(),
                    port: 61001,
                    health_path: "/gameservers",
                    ready_timeout: Duration::from_secs(20),
                },
            ),
        ]);
        Self::new(program, specs, layout.runtime_state_dir.clone(), before_start)
    }
}

struct ProcessHandle {
    name: ServiceName,
    child: Arc<tokio::sync::Mutex<Child>>,
    children: Arc<Mutex<HashMap<ServiceName, Arc<tokio::sync::Mutex<Child>>>>>,
}

impl ServiceHandle for ProcessHandle {
    fn stop(self: Box<Self>) -> Boxed<()> {
        Box::pin(async move {
            let mut child = self.child.lock().await;
            // X7: `shutdown` on stdin asks for a clean stop; a kill after 7 s, and no more waiting after 10 s
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"shutdown\n").await;
                let _ = stdin.flush().await;
            }
            if tokio::time::timeout(Duration::from_secs(7), child.wait())
                .await
                .is_err()
            {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            }
            let mut children = self.children.lock().unwrap_or_else(|p| p.into_inner());
            if children
                .get(&self.name)
                .is_some_and(|known| Arc::ptr_eq(known, &self.child))
            {
                children.remove(&self.name);
            }
        })
    }
}

impl Runtime for ProcessRuntime {
    fn start_service(&self, name: ServiceName) -> Boxed<Result<Box<dyn ServiceHandle>, String>> {
        let spec = self.specs.get(&name).cloned();
        let program = self.program.clone();
        let log_dir = self.log_dir.clone();
        let before_start = self.before_start.clone();
        let children = self.children.clone();
        Box::pin(async move {
            let spec = spec.ok_or_else(|| format!("no settings for {}", name.as_str()))?;
            if children.lock().unwrap_or_else(|p| p.into_inner()).contains_key(&name) {
                return Err(format!("{} is already running", name.as_str()));
            }
            tokio::task::spawn_blocking(move || before_start(name))
                .await
                .map_err(|e| e.to_string())??;
            std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
            let log_file =
                std::fs::File::create(log_dir.join(format!("{}.log", name.as_str()))).map_err(|e| e.to_string())?;
            let errors = log_file.try_clone().map_err(|e| e.to_string())?;
            let mut command = Command::new(&program);
            command
                .args(["--service", name.as_str(), "--env-file"])
                .arg(&spec.env_file)
                .env_clear()
                .envs(service_environment())
                .stdin(Stdio::piped())
                .stdout(Stdio::from(log_file))
                .stderr(Stdio::from(errors))
                .kill_on_drop(true);
            #[cfg(windows)]
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            let child = command
                .spawn()
                .map_err(|e| format!("{} could not start: {e}", name.as_str()))?;
            log::info(&format!(
                "local {} spawned (pid {})",
                name.as_str(),
                child.id().map_or_else(|| "unknown".into(), |pid| pid.to_string())
            ));
            let child = Arc::new(tokio::sync::Mutex::new(child));
            children
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(name, child.clone());
            Ok(Box::new(ProcessHandle { name, child, children }) as Box<dyn ServiceHandle>)
        })
    }

    fn wait_until_ready(&self, name: ServiceName) -> Boxed<Result<(), String>> {
        let spec = self.specs.get(&name).cloned();
        let children = self.children.clone();
        Box::pin(async move {
            let spec = spec.ok_or_else(|| format!("no settings for {}", name.as_str()))?;
            let deadline = Instant::now() + spec.ready_timeout;
            let endpoint = Endpoint {
                host: "127.0.0.1".into(),
                port: spec.port,
                pin: None,
            };
            while Instant::now() < deadline {
                // A crashed start fails at once instead of waiting out the deadline
                let child = children.lock().unwrap_or_else(|p| p.into_inner()).get(&name).cloned();
                let exited = match child {
                    Some(child) => child.lock().await.try_wait().map_or(true, |status| status.is_some()),
                    None => true,
                };
                if exited {
                    return Err(format!("{} exited before becoming ready", name.as_str()));
                }
                let options = RequestOptions::get(Duration::from_secs(1), 64 * 1024);
                if let Ok(response) = request(&endpoint, spec.health_path, options).await
                    && response.status == 200
                {
                    log::info(&format!("local {} is ready on 127.0.0.1:{}", name.as_str(), spec.port));
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(format!(
                "{} did not become ready within {} ms",
                name.as_str(),
                spec.ready_timeout.as_millis()
            ))
        })
    }
}

/// `makeLocalStack`'s preparation: before the metagame, the settings files and the game's offline config;
/// before Deploy (once the metagame has migrated its database), the game-server key registered in it.
pub fn before_start(
    layout: OfflineLayout,
    game_executable: impl Fn() -> PathBuf + Send + Sync + 'static,
    game_config_dir: PathBuf,
    server_console: bool,
) -> BeforeStart {
    let key_file: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    Arc::new(move |name| match name {
        ServiceName::Metagame => {
            let file = super::state::prepare(&layout, &game_executable(), server_console).map_err(|e| e.to_string())?;
            super::game_config::prepare(&game_config_dir).map_err(|e| e.to_string())?;
            *key_file.lock().unwrap_or_else(|p| p.into_inner()) = Some(file);
            Ok(())
        }
        ServiceName::Deploy => {
            let file = key_file
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
                .ok_or("Offline state has not been prepared")?;
            register_gameserver_key(&layout.database_path, &file)
        }
    })
}

fn register_gameserver_key(database: &Path, key_file: &Path) -> Result<(), String> {
    metagame::dbtools::run(&metagame::dbtools::DbCommand::GsKey {
        db: database.to_path_buf(),
        key_file: key_file.to_path_buf(),
    })
    .map(|_| ())
    .map_err(|_| "Offline game-server credential registration failed".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        events: Mutex<Vec<String>>,
        fail_ready: Option<ServiceName>,
    }

    struct FakeHandle(ServiceName, Arc<Fake>);

    impl ServiceHandle for FakeHandle {
        fn stop(self: Box<Self>) -> Boxed<()> {
            self.1.events.lock().unwrap().push(format!("stop {}", self.0.as_str()));
            Box::pin(async {})
        }
    }

    struct FakeRuntime(Arc<Fake>);

    impl Runtime for FakeRuntime {
        fn start_service(&self, name: ServiceName) -> Boxed<Result<Box<dyn ServiceHandle>, String>> {
            self.0.events.lock().unwrap().push(format!("start {}", name.as_str()));
            let fake = self.0.clone();
            Box::pin(async move { Ok(Box::new(FakeHandle(name, fake)) as Box<dyn ServiceHandle>) })
        }
        fn wait_until_ready(&self, name: ServiceName) -> Boxed<Result<(), String>> {
            let fail = self.0.fail_ready == Some(name);
            Box::pin(async move {
                if fail {
                    Err(format!("{} not ready", name.as_str()))
                } else {
                    Ok(())
                }
            })
        }
    }

    #[tokio::test]
    async fn starts_in_order_stops_in_reverse_and_cleans_up_a_failed_start() {
        let fake = Arc::new(Fake::default());
        let stack = StackManager::new(Arc::new(FakeRuntime(fake.clone())));
        let states = Arc::new(Mutex::new(Vec::new()));
        let seen = states.clone();
        let on_state = move |state| seen.lock().unwrap().push(state);
        stack.start(&on_state).await.unwrap();
        assert!(stack.start(&on_state).await.is_err(), "already active");
        stack.stop().await;
        assert_eq!(
            *fake.events.lock().unwrap(),
            ["start metagame", "start deploy", "stop deploy", "stop metagame"]
        );
        assert_eq!(
            *states.lock().unwrap(),
            [
                StackState::StartingMetagame,
                StackState::StartingDeploy,
                StackState::Ready
            ]
        );
        assert_eq!(stack.snapshot(), (StackState::Stopped, None));

        let fake = Arc::new(Fake {
            fail_ready: Some(ServiceName::Deploy),
            ..Fake::default()
        });
        let stack = StackManager::new(Arc::new(FakeRuntime(fake.clone())));
        let quiet = |_| {};
        assert_eq!(stack.start(&quiet).await, Err("deploy not ready".into()));
        assert_eq!(
            *fake.events.lock().unwrap(),
            ["start metagame", "start deploy", "stop deploy", "stop metagame"]
        );
        assert_eq!(stack.snapshot(), (StackState::Failed, Some("deploy not ready".into())));
        // A failed stack may start again
        fake.events.lock().unwrap().clear();
        assert!(stack.start(&quiet).await.is_err());
        assert_eq!(fake.events.lock().unwrap()[0], "start metagame");
    }

    #[test]
    fn services_inherit_only_the_systems_plumbing() {
        let names: Vec<String> = service_environment()
            .into_iter()
            .map(|(k, _)| k.to_ascii_uppercase())
            .collect();
        assert!(names.iter().all(|name| !name.contains("KEY") && name != "DB_FILENAME"));
    }
}
