#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)] // test helpers

//! Stand-ins for game processes (`UseProcessFunctionsForTests` in the Node tests), the deploy
//! server's log as "level: message" lines, and the test environment (`test/deployenv.ts`). No game
//! process is ever started: the binary path is in a folder that does not exist.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use deploy::gameservers::Deploy;
use deploy::process::{BoxFuture, Exit, GameProcess, Platform, SpawnOptions};
use deploy::settings::Settings;
use tokio::sync::watch;

pub const TUTORIAL_ARGS: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";
pub const HUNT_POOL: [u16; 6] = [8770, 8771, 8772, 8773, 8774, 8775];

// ---- the log ----

type Lines = Arc<Mutex<Vec<String>>>;

struct Capture(Lines);

struct Message(String, bool);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "fatal" {
            self.1 = value;
        }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut message = Message(String::new(), false);
        event.record(&mut message);
        let level = match *event.metadata().level() {
            tracing::Level::ERROR if message.1 => "fatal",
            tracing::Level::ERROR => "error",
            tracing::Level::WARN => "warn",
            tracing::Level::INFO => "info",
            _ => "debug",
        };
        self.0.lock().unwrap().push(format!("{level}: {}", message.0));
    }
}

pub fn log() -> Lines {
    static LINES: OnceLock<Lines> = OnceLock::new();
    LINES
        .get_or_init(|| {
            use tracing_subscriber::layer::SubscriberExt;
            let lines: Lines = Arc::default();
            let subscriber = tracing_subscriber::registry().with(Capture(lines.clone()));
            let _ = tracing::subscriber::set_global_default(subscriber);
            lines
        })
        .clone()
}

pub fn logged() -> Vec<String> {
    log().lock().unwrap().clone()
}

/// Tests that read the shared log run one at a time.
pub async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(())).lock().await
}

// ---- stand-in processes ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Starts and runs until the test ends it; cannot be killed
    Running,
    /// As `Running`, and ends when killed
    Killable,
    /// What Node does for a binary that is not there: no pid, and an error
    Failing,
    /// Spawning itself throws
    Throwing,
}

#[derive(Debug, Clone)]
pub struct Spawned {
    pub command: String,
    pub args: Vec<String>,
    pub pid: Option<u32>,
    pub hide: bool,
    pub console: Option<String>,
    pub ready_file: Option<String>,
}

pub struct Fake {
    pid: Option<u32>,
    error: Option<String>,
    exit: watch::Sender<Option<Exit>>,
    killable: bool,
    alive: Arc<Mutex<HashSet<u32>>>,
}

impl GameProcess for Fake {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn exit(&self) -> watch::Receiver<Option<Exit>> {
        self.exit.subscribe()
    }

    fn kill(&self, _force: bool) -> bool {
        if !self.killable || self.exit.borrow().is_some() {
            return false;
        }
        if let Some(pid) = self.pid {
            self.alive.lock().unwrap().remove(&pid);
        }
        self.exit.send_replace(Some(Exit {
            code: None,
            signal: Some("SIGTERM".into()),
        }));
        true
    }
}

type PortCheck = Arc<dyn Fn(u16) -> bool + Send + Sync>;
pub type OnSpawn = Arc<dyn Fn(&Spawned) + Send + Sync>;

pub struct FakePlatform {
    pub mode: Mutex<Mode>,
    next_pid: AtomicU32,
    pub alive: Arc<Mutex<HashSet<u32>>>,
    pub spawned: Mutex<Vec<Spawned>>,
    processes: Mutex<Vec<Arc<Fake>>>,
    pub port_bound: Mutex<PortCheck>,
    /// Every pid counts as alive (the Node tests' `IsAlive: () => true`)
    pub all_alive: Mutex<bool>,
    /// Runs after each spawn, with the new process's port and pid
    pub on_spawn: Mutex<Option<OnSpawn>>,
}

impl FakePlatform {
    pub fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            next_pid: AtomicU32::new(41_000),
            alive: Arc::default(),
            spawned: Mutex::default(),
            processes: Mutex::default(),
            port_bound: Mutex::new(Arc::new(|_| false)),
            all_alive: Mutex::new(false),
            on_spawn: Mutex::new(None),
        })
    }

    pub fn spawned(&self) -> Vec<Spawned> {
        self.spawned.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.spawned.lock().unwrap().len()
    }

    /// The test "kills" a process: it is no longer alive (no exit event, as in the Node tests).
    pub fn kill(&self, pid: u32) {
        self.alive.lock().unwrap().remove(&pid);
    }

    /// The process ends with `code` (Node's `exit` event).
    pub fn exit(&self, pid: u32, code: i32) {
        self.alive.lock().unwrap().remove(&pid);
        if let Some(process) = self.processes.lock().unwrap().iter().find(|p| p.pid == Some(pid)) {
            process.exit.send_replace(Some(Exit {
                code: Some(code),
                signal: None,
            }));
        }
    }

    pub fn set_mode(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }
}

impl Platform for FakePlatform {
    fn spawn(&self, command: &str, args: &[String], options: &SpawnOptions) -> Result<Arc<dyn GameProcess>, String> {
        let mode = *self.mode.lock().unwrap();
        let env = |name: &str| options.env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        let mut record = Spawned {
            command: command.to_owned(),
            args: args.to_vec(),
            pid: None,
            hide: options.hide,
            console: env("DR_SHOW_CONSOLE"),
            ready_file: env("DR_SERVER_READY_FILE"),
        };
        if mode == Mode::Throwing {
            return Err("test failure".into());
        }
        let (exit, _) = watch::channel(None);
        let process = if mode == Mode::Failing {
            Fake {
                pid: None,
                error: Some(format!("spawn {command} ENOENT")),
                exit,
                killable: false,
                alive: self.alive.clone(),
            }
        } else {
            let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
            self.alive.lock().unwrap().insert(pid);
            record.pid = Some(pid);
            Fake {
                pid: Some(pid),
                error: None,
                exit,
                killable: mode == Mode::Killable,
                alive: self.alive.clone(),
            }
        };
        let process = Arc::new(process);
        self.processes.lock().unwrap().push(process.clone());
        self.spawned.lock().unwrap().push(record.clone());
        if let Some(hook) = self.on_spawn.lock().unwrap().clone() {
            hook(&record);
        }
        Ok(process)
    }

    fn is_alive(&self, pid: u32) -> bool {
        *self.all_alive.lock().unwrap() || self.alive.lock().unwrap().contains(&pid)
    }

    fn is_port_bound(&self, port: u16) -> BoxFuture<Result<bool, String>> {
        let bound = (self.port_bound.lock().unwrap())(port);
        Box::pin(async move { Ok(bound) })
    }
}

// ---- the deploy server ----

pub struct World {
    pub deploy: Arc<Deploy>,
    pub platform: Arc<FakePlatform>,
    pub dir: tempfile::TempDir,
}

/// `test/deployenv.ts` and `test/setup.ts`, plus `extra`.
pub fn settings(dir: &std::path::Path, extra: &[(&str, &str)]) -> Settings {
    let binary = dir.join("missing").join("Dauntless-Win64-Shipping.exe");
    let drain = dir.join("drain.flag");
    let ready = dir.join("ready");
    let settings = Settings::from_pairs([
        ("PORT_RANGE_BEGIN", "8770"),
        ("PORT_RANGE_END", "8777"),
        ("GAMESERVER_BINARY_PATH", binary.to_str().unwrap()),
        ("METAGAME_API_KEY", "test-key-not-real"),
        ("MY_IP", "127.0.0.1"),
        ("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP", "0"),
        ("RAMSGATE_POOL", "0"),
        ("GAMESERVER_MEMORY_GUARD", "0"),
        ("GAMESERVER_STARTUP_GRACE_MS", "0"),
        ("DRAIN_FILE", drain.to_str().unwrap()),
        ("GAMESERVER_READY_DIR", ready.to_str().unwrap()),
    ]);
    for (name, value) in extra {
        settings.set(name, Some(value));
    }
    settings
}

pub fn world(mode: Mode, extra: &[(&str, &str)]) -> World {
    log();
    let dir = tempfile::tempdir().unwrap();
    let platform = FakePlatform::new(mode);
    let deploy = Deploy::new(settings(dir.path(), extra), platform.clone()).unwrap();
    World { deploy, platform, dir }
}

pub async fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(tokio::time::Instant::now() < until, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
