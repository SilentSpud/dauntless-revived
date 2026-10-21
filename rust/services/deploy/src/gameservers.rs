//! The game servers of this machine (`controllers/gameservers.ts`): Ramsgate and the Training Dojo
//! (one process each on a fixed port, or a pool of cities), and hunts on the ports between. Starts
//! are queued and spaced, admitted against CPU, memory and hunt limits, and count as done once the
//! process listens; exits free their ports at once, and persistent worlds start again when needed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};
use std::time::Duration;

use futures_util::FutureExt;
use futures_util::future::Shared;
use serde_json::{Value, json};

use crate::capacity::{DeployError, HuntAdmission, MemoryAdmission, Reason, Release};
use crate::cities::{CITY_PLAYER_LIMIT, City, CityPool, Connection, city_caps};
use crate::cpu::CpuAdmission;
use crate::lifetime::{Hunt, HuntLifetime};
use crate::occupancy::{native_city_occupancy, native_occupancy};
use crate::process::{BoxFuture, Exit, GameProcess, Platform, SpawnOptions};
use crate::settings::{Fixed, Settings};
use crate::{readiness, tables, trials};

pub const RAMSGATE_MAP_PATH: &str = "/Game/Maps/ramsgate/ramsgate_01_persistent";
pub const TRAINING_DOJO_MAP_PATH: &str = "/Game/Maps/islands/dojo/training_dojo_persistent";
pub const TRIALS_MAP_PATH: &str = "/Game/Maps/islands/arenas/arena_ramsgate_00";
pub const TRAINING_DOJO_MATCHMAKER_HUNT_ID: &str = "CR19_MatchmakerHunt_ShatteredIsles_TrainingDojo";
const STANDARD_GAMESERVER_ARGS: [&str; 3] = ["-EpicPortal", "-server", "-nullrhi"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedPlayer {
    pub player_uid: String,
    pub player_hunt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gameserver {
    pub id: String,
    pub port: u16,
    pub map: String,
    pub behemoth: Option<String>,
    pub matchmaker_hunt_id: Option<String>,
    pub expected_players: Option<Vec<ExpectedPlayer>>,
    pub is_ramsgate: bool,
    pub is_training_dojo: bool,
    pub process_id: u32,
    pub ready_at: Option<i64>,
    /// Milliseconds since the epoch
    pub start_time: i64,
}

impl Gameserver {
    pub fn persistent(&self) -> bool {
        self.is_ramsgate || self.is_training_dojo
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum World {
    Ramsgate,
    Dojo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    City,
    Hunt,
    Dojo,
    Tutorial,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::City => "city",
            Self::Hunt => "hunt",
            Self::Dojo => "dojo",
            Self::Tutorial => "tutorial",
        }
    }
}

/// The tutorial is started from the client's own game args: the tutorial Gnasher on
/// dia_moss_triforce (regular hunts use dia_moss_triforce_2, hence the exact match).
fn is_tutorial_map(map: &str) -> bool {
    let lower = map.to_lowercase();
    let needle = "/dia_moss_triforce";
    let mut from = 0;
    while let Some(at) = lower[from..].find(needle) {
        let end = from + at + needle.len();
        match lower[end..].chars().next() {
            None | Some('.' | '?') => return true,
            _ => from = end,
        }
    }
    false
}

pub fn kind_of(server: &Gameserver) -> Kind {
    if server.is_ramsgate {
        return Kind::City;
    }
    if server.is_training_dojo {
        return Kind::Dojo;
    }
    let tutorial_behemoth = server
        .behemoth
        .as_deref()
        .is_some_and(|b| b.to_lowercase().contains("_tutorial_bp"));
    if tutorial_behemoth || is_tutorial_map(&server.map) {
        return Kind::Tutorial;
    }
    Kind::Hunt
}

/// A hunt's player limit from the matchmaker or trials tables; `None` where they don't say.
pub fn max_players_from_tables(hunt_id: &str) -> Option<i64> {
    tables::any_hunt_row(hunt_id)
        .and_then(|row| row["MaxPlayers"].as_f64())
        .filter(|n| n.fract() == 0.0)
        .map(|n| n as i64)
}

fn game_mode_override(hunt_id: &str) -> Result<String, DeployError> {
    tables::matchmaker_row(hunt_id)
        .and_then(|row| row["GameModeOverride"].as_str())
        .map(|mode| mode.replace("Archon/Content", "/Game"))
        .ok_or_else(|| DeployError::failed(format!("Matchmaker hunt {hunt_id} is not in the table")))
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

/// `GET /gameservers`'s list: servers whose process has exited are left out at once.
pub fn describe(servers: &[Gameserver], is_alive: impl Fn(u32) -> bool, pool: bool) -> Vec<Value> {
    servers
        .iter()
        .filter(|s| is_alive(s.process_id))
        .map(|server| {
            let kind = kind_of(server);
            let mut parts = server.map.split('?');
            let map = parts.next().unwrap_or("");
            let game_mode = parts.find_map(|o| o.strip_prefix("game=")).filter(|m| !m.is_empty());
            let behemoth = server
                .behemoth
                .as_deref()
                .filter(|b| !b.is_empty() && *b != "NO_BEHEMOTH");
            let max_players = match kind {
                Kind::City => pool.then_some(CITY_PLAYER_LIMIT as i64),
                Kind::Tutorial => Some(1),
                Kind::Dojo => max_players_from_tables(TRAINING_DOJO_MATCHMAKER_HUNT_ID),
                Kind::Hunt => server.matchmaker_hunt_id.as_deref().and_then(max_players_from_tables),
            };
            let players = server.expected_players.as_deref().unwrap_or(&[]);
            json!({
                "id": server.id,
                "port": server.port,
                "kind": kind.as_str(),
                "map": map,
                "gameMode": game_mode,
                "behemoth": behemoth,
                "huntId": players.first().map(|p| p.player_hunt_id.clone()),
                "matchmakerHuntId": server.matchmaker_hunt_id,
                "expectedPlayers": players.iter().map(|p| p.player_uid.clone()).collect::<Vec<_>>(),
                "maxPlayers": max_players,
                "startedAt": iso(server.start_time),
            })
        })
        .collect()
}

#[derive(Default)]
struct State {
    gameservers: Vec<Gameserver>,
    free_ports: Vec<u16>,
    /// The persistent worlds' records, kept even after they leave the list (Node's `RamsgateServer`)
    ramsgate: Option<Gameserver>,
    dojo: Option<Gameserver>,
    children: HashMap<String, Arc<dyn GameProcess>>,
    next_launch_at: i64,
}

type Launch = Shared<BoxFuture<Result<Gameserver, DeployError>>>;

pub struct Deploy {
    pub settings: Settings,
    fixed: Fixed,
    platform: RwLock<Arc<dyn Platform>>,
    state: Mutex<State>,
    launches: Mutex<HashMap<World, Launch>>,
    launch_queue: Arc<tokio::sync::Mutex<()>>,
    pub hunt_admission: HuntAdmission,
    memory_admission: MemoryAdmission,
    pub cpu_admission: CpuAdmission,
    cities: CityPool<Gameserver>,
    lifetime: Mutex<HuntLifetime>,
    watchdog_running: AtomicBool,
    shutting_down: AtomicBool,
    startup_failed: AtomicBool,
    this: Weak<Deploy>,
}

pub struct StateForTests {
    pub free_ports: Vec<u16>,
    pub ramsgate: Option<Gameserver>,
    pub dojo: Option<Gameserver>,
}

impl Deploy {
    pub fn new(settings: Settings, platform: Arc<dyn Platform>) -> Result<Arc<Self>, String> {
        let fixed = Fixed::from_settings(&settings)?;
        let cpu_number = |name: &str, default: f64| settings.number(name).unwrap_or(default);
        let cpu_admission = CpuAdmission::new(
            cpu_number("GAMESERVER_CPU_LIMIT_PERCENT", 80.0),
            cpu_number("GAMESERVER_CPU_RESUME_PERCENT", 65.0),
        )?;
        let lifetime = HuntLifetime::from_settings(&settings)?;
        trials::validate(
            trials::pool(),
            tables::rows(tables::trials_hard()),
            tables::rows(tables::trials_elite()),
        )?;
        Ok(Arc::new_cyclic(|this: &Weak<Deploy>| {
            let running = {
                let this = this.clone();
                move || {
                    this.upgrade().map_or(0, |deploy| {
                        let alive: Vec<u32> = deploy
                            .lock()
                            .gameservers
                            .iter()
                            .filter(|s| !s.persistent())
                            .map(|s| s.process_id)
                            .collect();
                        alive.into_iter().filter(|pid| deploy.is_alive(*pid)).count() as u64
                    })
                }
            };
            let cities = {
                let (list, start, get, caps) = (this.clone(), this.clone(), this.clone(), settings.clone());
                CityPool::new(
                    move || list.upgrade().map(|d| d.city_list()).unwrap_or_default(),
                    move || {
                        let start = start.clone();
                        Box::pin(async move {
                            let deploy = start
                                .upgrade()
                                .ok_or_else(|| DeployError::failed("Deploy service is shutting down"))?;
                            deploy
                                .start_server(
                                    format!("{RAMSGATE_MAP_PATH}?MaxPlayers={CITY_PLAYER_LIMIT}"),
                                    None,
                                    None,
                                    None,
                                    true,
                                    false,
                                )
                                .await
                        })
                    },
                    move |id| get.upgrade().and_then(|d| d.find(id)),
                    |server: &Gameserver| server.id.clone(),
                    crate::now_ms,
                    move || city_caps(&caps),
                )
            };
            Deploy {
                hunt_admission: HuntAdmission::new(settings.clone(), running),
                memory_admission: MemoryAdmission::new(settings.clone(), crate::system::free_memory, crate::now_ms),
                settings,
                fixed,
                platform: RwLock::new(platform),
                state: Mutex::default(),
                launches: Mutex::default(),
                launch_queue: Arc::new(tokio::sync::Mutex::new(())),
                cpu_admission,
                cities,
                lifetime: Mutex::new(lifetime),
                watchdog_running: AtomicBool::new(false),
                shutting_down: AtomicBool::new(false),
                startup_failed: AtomicBool::new(false),
                this: this.clone(),
            }
        }))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn platform(&self) -> Arc<dyn Platform> {
        self.platform.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn is_alive(&self, pid: u32) -> bool {
        self.platform().is_alive(pid)
    }

    async fn is_port_bound(&self, port: u16) -> Result<bool, DeployError> {
        self.platform().is_port_bound(port).await.map_err(DeployError::Failed)
    }

    fn ramsgate_port(&self) -> u16 {
        self.fixed.port_range_end
    }

    fn dojo_port(&self) -> u16 {
        self.fixed.port_range_end - 1
    }

    pub fn city_pool_enabled(&self) -> bool {
        !self.settings.is("RAMSGATE_POOL", "0")
    }

    /// On (the default) unless `PERSISTENT_WORLD_LIVENESS=0`: before Ramsgate or the Dojo is handed
    /// to a player, its process must still be running, or it is started again first.
    pub fn persistent_world_liveness(&self) -> bool {
        !self.settings.is("PERSISTENT_WORLD_LIVENESS", "0")
    }

    pub fn my_ip(&self) -> &str {
        &self.fixed.my_ip
    }

    pub fn ready_dir(&self) -> Option<String> {
        self.settings.get("GAMESERVER_READY_DIR")
    }

    pub fn gameservers(&self) -> Vec<Gameserver> {
        self.lock().gameservers.clone()
    }

    pub fn find(&self, id: &str) -> Option<Gameserver> {
        self.lock().gameservers.iter().find(|s| s.id == id).cloned()
    }

    fn contains(&self, id: &str) -> bool {
        self.lock().gameservers.iter().any(|s| s.id == id)
    }

    pub fn startup_failed(&self) -> bool {
        self.startup_failed.load(Ordering::SeqCst)
    }

    fn city_list(&self) -> Vec<City> {
        let cities: Vec<Gameserver> = self
            .lock()
            .gameservers
            .iter()
            .filter(|s| s.is_ramsgate)
            .cloned()
            .collect();
        let (dir, now) = (self.ready_dir(), crate::now_ms());
        cities
            .into_iter()
            .filter(|s| self.is_alive(s.process_id))
            .map(|s| City {
                players: native_city_occupancy(dir.as_deref(), s.process_id, s.start_time, now),
                id: s.id,
                started_at: s.start_time,
            })
            .collect()
    }

    /// Connected players of a running server, from the game's own counts.
    pub fn connected_players(&self, server: &Gameserver) -> Option<u64> {
        let (dir, now) = (self.ready_dir(), crate::now_ms());
        if server.is_ramsgate {
            native_city_occupancy(dir.as_deref(), server.process_id, server.start_time, now)
        } else {
            native_occupancy(dir.as_deref(), server.process_id, server.start_time, now)
        }
    }

    /// `DescribeGameservers` for the servers this machine runs now.
    pub fn describe(&self) -> Vec<Value> {
        let servers = self.gameservers();
        let platform = self.platform();
        describe(&servers, |pid| platform.is_alive(pid), self.city_pool_enabled())
    }

    fn startup_timeout_ms(&self) -> Result<u64, DeployError> {
        let Some(raw) = self.settings.get("GAMESERVER_STARTUP_GRACE_MS") else {
            return Ok(90_000);
        };
        match raw.parse::<u64>() {
            Ok(value) if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) && value <= 180_000 => Ok(value),
            _ => Err(DeployError::failed("Invalid GAMESERVER_STARTUP_GRACE_MS")),
        }
    }

    fn check_cpu(&self) -> Result<(), DeployError> {
        if self.settings.is("GAMESERVER_CPU_GUARD", "1") {
            self.cpu_admission.assert_available()?;
        }
        Ok(())
    }

    async fn take_hunt_port(&self) -> Result<u16, DeployError> {
        let mut busy = Vec::new();
        loop {
            let Some(port) = self.lock().free_ports.pop() else {
                break;
            };
            match self.is_port_bound(port).await {
                Ok(false) => {
                    self.lock().free_ports.splice(0..0, busy);
                    return Ok(port);
                }
                Ok(true) => busy.push(port),
                Err(error) => {
                    busy.push(port);
                    self.lock().free_ports.splice(0..0, busy);
                    return Err(error);
                }
            }
        }
        self.lock().free_ports.splice(0..0, busy);
        Err(DeployError::Capacity(Reason::Ports))
    }

    fn give_back(&self, port: u16) {
        self.lock().free_ports.push(port);
    }

    /// Called for a server whose process has exited. Ramsgate is started again (through the shared
    /// launch, so the watchdog and a player's request never start two processes on one port); a
    /// hunt's port goes back to the pool.
    pub fn cleanup_server(&self, server: &Gameserver) -> BoxFuture<Result<(), DeployError>> {
        let Some(this) = self.this.upgrade() else {
            return Box::pin(async { Ok(()) });
        };
        let server = server.clone();
        Box::pin(async move { this.cleanup(&server).await })
    }

    fn cleanup(self: &Arc<Self>, server: &Gameserver) -> BoxFuture<Result<(), DeployError>> {
        let (this, server) = (self.clone(), server.clone());
        Box::pin(async move { this.cleanup_now(&server).await })
    }

    async fn cleanup_now(self: &Arc<Self>, server: &Gameserver) -> Result<(), DeployError> {
        {
            let mut state = self.lock();
            // The exit event and the watchdog may race
            if !state.gameservers.iter().any(|s| s.id == server.id) {
                return Ok(());
            }
            state.children.remove(&server.id);
            state.gameservers.retain(|s| s.id != server.id);
        }
        if server.is_ramsgate
            && let Some(dir) = self.ready_dir()
        {
            let file = PathBuf::from(dir).join(format!("city-occupancy-{}.txt", server.process_id));
            let _ = tokio::fs::remove_file(file).await;
        }
        // During shutdown, exited game servers are only forgotten
        if self.shutting_down.load(Ordering::SeqCst) {
            let mut state = self.lock();
            if state.ramsgate.as_ref().is_some_and(|r| r.id == server.id) {
                state.ramsgate = None;
            }
            if state.dojo.as_ref().is_some_and(|r| r.id == server.id) {
                state.dojo = None;
            }
            return Ok(());
        }
        if server.is_ramsgate && self.city_pool_enabled() {
            let mut state = self.lock();
            if state.ramsgate.as_ref().is_some_and(|r| r.id == server.id) {
                state.ramsgate = None;
            }
            if server.port != self.ramsgate_port() {
                state.free_ports.push(server.port);
            }
        } else if server.is_ramsgate {
            self.ensure_persistent_world(World::Ramsgate, Some("RAMSGATE HAS FALLEN! Restarting!"), false)
                .await?;
        } else if server.is_training_dojo {
            // The next SHARED request starts the Dojo on demand
            let mut state = self.lock();
            if state.dojo.as_ref().is_some_and(|r| r.id == server.id) {
                state.dojo = None;
            }
        } else {
            self.give_back(server.port);
        }
        Ok(())
    }

    fn current_world(&self, world: World) -> Option<Gameserver> {
        let state = self.lock();
        match world {
            World::Ramsgate => state.ramsgate.clone(),
            World::Dojo => state.dojo.clone(),
        }
    }

    /// The launch of a persistent world: the watchdog, CITY and SHARED requests and the Dojo's
    /// first start all wait for one launch instead of starting their own.
    pub async fn ensure_persistent_world(
        self: &Arc<Self>,
        world: World,
        why: Option<&str>,
        info: bool,
    ) -> Result<Gameserver, DeployError> {
        let launch = {
            let mut launches = self.launches.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(launch) = launches.get(&world) {
                launch.clone()
            } else {
                let current = self.current_world(world);
                if let Some(current) = &current
                    && self.is_alive(current.process_id)
                {
                    return Ok(current.clone());
                }
                if let Some(current) = current {
                    self.lock().gameservers.retain(|s| s.id != current.id);
                }
                if let Some(why) = why {
                    if info {
                        tracing::info!("{why}");
                    } else {
                        tracing::warn!("{why}");
                    }
                }
                let this = self.clone();
                let task = tokio::spawn(async move {
                    let started = this.start_world(world).await;
                    if let Ok(server) = &started {
                        let mut state = this.lock();
                        match world {
                            World::Ramsgate => state.ramsgate = Some(server.clone()),
                            World::Dojo => state.dojo = Some(server.clone()),
                        }
                    }
                    this.launches
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&world);
                    started
                });
                let launch: BoxFuture<Result<Gameserver, DeployError>> =
                    Box::pin(async move { task.await.unwrap_or_else(|e| Err(DeployError::failed(e.to_string()))) });
                let launch = launch.shared();
                launches.insert(world, launch.clone());
                launch
            }
        };
        launch.await
    }

    async fn start_world(self: &Arc<Self>, world: World) -> Result<Gameserver, DeployError> {
        match world {
            World::Ramsgate => {
                self.start_server(RAMSGATE_MAP_PATH.to_owned(), None, None, None, true, false)
                    .await
            }
            World::Dojo => {
                // Shared worlds have no fixed roster, but Training Grounds still needs its rules and mode
                let id = TRAINING_DOJO_MATCHMAKER_HUNT_ID;
                let max = max_players_from_tables(id).map_or_else(|| "null".to_owned(), |n| n.to_string());
                let map = format!(
                    "{TRAINING_DOJO_MAP_PATH}?game={}?MaxPlayers={max}",
                    game_mode_override(id)?
                );
                self.start_server(map, None, Some(id.to_owned()), None, false, true)
                    .await
            }
        }
    }

    /// `StartServer`: admission, then one launch at a time, spaced, through to readiness.
    pub fn start_server(
        self: &Arc<Self>,
        map: String,
        behemoth: Option<String>,
        matchmaker_hunt_id: Option<String>,
        expected: Option<Vec<ExpectedPlayer>>,
        is_ramsgate: bool,
        is_dojo: bool,
    ) -> BoxFuture<Result<Gameserver, DeployError>> {
        let this = self.clone();
        Box::pin(async move {
            this.start_server_queued(map, behemoth, matchmaker_hunt_id, expected, is_ramsgate, is_dojo)
                .await
        })
    }

    async fn start_server_queued(
        self: &Arc<Self>,
        map: String,
        behemoth: Option<String>,
        matchmaker_hunt_id: Option<String>,
        expected: Option<Vec<ExpectedPlayer>>,
        is_ramsgate: bool,
        is_dojo: bool,
    ) -> Result<Gameserver, DeployError> {
        let is_hunt = !is_ramsgate && !is_dojo;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(DeployError::failed("Deploy service is shutting down"));
        }
        if self.settings.is("SERVER_ROLE", "database-only") {
            return Err(DeployError::failed("Game allocation disabled on database-only node"));
        }
        if is_hunt {
            self.check_cpu()?;
        }
        let release_hunt = if is_hunt {
            self.hunt_admission.reserve()?
        } else {
            Release::nothing()
        };
        let mut turn = Some(self.launch_queue.clone().lock_owned().await);
        let result = async {
            if self.shutting_down.load(Ordering::SeqCst) {
                return Err(DeployError::failed("Deploy service is shutting down"));
            }
            if is_hunt && self.lock().free_ports.is_empty() {
                return Err(DeployError::Capacity(Reason::Ports));
            }
            let wait = self.lock().next_launch_at - crate::now_ms();
            if wait > 0 {
                tokio::time::sleep(Duration::from_millis(wait as u64)).await;
            }
            let spawned = || {
                release_hunt.release();
                self.lock().next_launch_at = crate::now_ms() + self.fixed.launch_spacing_ms as i64;
                turn.take();
            };
            self.start_server_now(
                map,
                behemoth,
                matchmaker_hunt_id,
                expected,
                is_ramsgate,
                is_dojo,
                spawned,
            )
            .await
        }
        .await;
        release_hunt.release();
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_server_now(
        self: &Arc<Self>,
        map: String,
        behemoth: Option<String>,
        matchmaker_hunt_id: Option<String>,
        expected: Option<Vec<ExpectedPlayer>>,
        is_ramsgate: bool,
        is_dojo: bool,
        spawned: impl FnOnce(),
    ) -> Result<Gameserver, DeployError> {
        let is_hunt = !is_ramsgate && !is_dojo;
        let ready_dir = self.ready_dir().unwrap_or_else(|| {
            let dir = std::env::temp_dir()
                .join(format!("dauntless-revived-ready-{}", std::process::id()))
                .to_string_lossy()
                .into_owned();
            self.settings.set("GAMESERVER_READY_DIR", Some(&dir));
            dir
        });
        tokio::fs::create_dir_all(&ready_dir)
            .await
            .map_err(|e| DeployError::failed(e.to_string()))?;
        if is_hunt && self.lock().free_ports.is_empty() {
            return Err(DeployError::Capacity(Reason::Ports));
        }
        if is_hunt {
            self.check_cpu()?;
        }
        let reservation = self.memory_admission.reserve(is_hunt)?;
        let port = if is_ramsgate {
            let ramsgate_port = self.ramsgate_port();
            let taken = self.city_pool_enabled() && {
                let alive_on_port = self
                    .gameservers()
                    .iter()
                    .any(|s| s.is_ramsgate && s.port == ramsgate_port && self.is_alive(s.process_id));
                alive_on_port
                    || self
                        .is_port_bound(ramsgate_port)
                        .await
                        .inspect_err(|_| reservation.release())?
            };
            if taken {
                self.take_hunt_port().await.inspect_err(|_| reservation.release())?
            } else {
                ramsgate_port
            }
        } else if is_dojo {
            self.dojo_port()
        } else {
            self.take_hunt_port().await.inspect_err(|_| reservation.release())?
        };
        let id = uuid::Uuid::new_v4().to_string();
        let ready_file = PathBuf::from(&ready_dir).join(format!("{id}.ready"));
        let borrowed = !is_dojo && port != self.ramsgate_port();
        let fail = |message: String| {
            reservation.release();
            if borrowed {
                self.give_back(port);
            }
            DeployError::Failed(message)
        };
        if !is_hunt && self.is_port_bound(port).await? {
            return Err(fail(format!(
                "Persistent game server port {port} is already in use; refusing to reuse a stale listener"
            )));
        }
        let _ = tokio::fs::remove_file(&ready_file).await;
        // Shutdown may begin during the setup above; never create a new child after it starts
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(fail("Deploy service is shutting down".into()));
        }

        let args: Vec<String> = [
            self.fixed.metagame_api_key.clone(),
            port.to_string(),
            map.clone(),
            behemoth.clone().unwrap_or_else(|| "NO_BEHEMOTH".into()),
            matchmaker_hunt_id.clone().unwrap_or_else(|| "NO_MM_HUNTID".into()),
            expected.as_ref().map_or_else(
                || "NO_EXPECTED_PLAYERS".into(),
                |players| {
                    players
                        .iter()
                        .map(|p| format!("{}:{}", p.player_uid, p.player_hunt_id))
                        .collect::<Vec<_>>()
                        .join(",")
                },
            ),
            format!("{}:{port}", self.fixed.my_ip),
        ]
        .into_iter()
        .chain(STANDARD_GAMESERVER_ARGS.iter().map(|s| (*s).to_owned()))
        .collect();
        // Explicit Offline/diagnostic policy; unset deployments keep the old visibility
        let hide = match self.settings.get("DR_SHOW_CONSOLE").as_deref() {
            Some("0") => true,
            Some("1") => false,
            _ => is_hunt,
        };
        let mut env: Vec<(String, String)> = {
            let mut vars = std::collections::BTreeMap::new();
            for (name, value) in self.settings.snapshot() {
                vars.insert(name, value);
            }
            vars.insert("GAMESERVER_READY_DIR".into(), ready_dir.clone());
            vars.insert("DR_SERVER_READY_FILE".into(), ready_file.to_string_lossy().into_owned());
            vars.insert(
                "DR_RAMSGATE_POOL".into(),
                if self.city_pool_enabled() { "1" } else { "0" }.into(),
            );
            vars.into_iter().collect()
        };
        env.sort();
        let options = SpawnOptions { hide, env };
        let binary = self.fixed.binary.clone();
        let child = match self.platform().spawn(&binary, &args, &options) {
            Ok(child) => child,
            Err(error) => {
                tracing::error!("Could not start a game server on port {port}: {error}");
                return Err(fail(format!("Could not start a game server on port {port}")));
            }
        };
        // No process id: the start failed (a wrong GAMESERVER_BINARY_PATH, a missing file). Nothing
        // runs on the port, so a hunt's port goes back to the pool and the caller gets an error
        let Some(pid) = child.pid() else {
            let reason = child.error().unwrap_or_else(|| "no process".into());
            tracing::error!("Game server on port {port} failed: {reason} (GAMESERVER_BINARY_PATH is {binary})");
            return Err(fail(format!("Could not start a game server on port {port}")));
        };

        let record = Gameserver {
            id: id.clone(),
            port,
            map,
            behemoth,
            matchmaker_hunt_id,
            expected_players: expected,
            is_ramsgate,
            is_training_dojo: is_dojo,
            process_id: pid,
            ready_at: None,
            start_time: crate::now_ms(),
        };
        {
            let mut state = self.lock();
            state.gameservers.push(record.clone());
            state.children.insert(id.clone(), child.clone());
        }
        self.watch_exit(&record, child.clone(), reservation.clone(), ready_file.clone());
        spawned();

        let ready = async {
            let timeout = self.startup_timeout_ms()?;
            if timeout > 0 {
                readiness::wait_for_server_ready(child.clone(), port, &ready_file, timeout)
                    .await
                    .map_err(DeployError::Failed)?;
                let now = crate::now_ms();
                if let Some(server) = self.lock().gameservers.iter_mut().find(|s| s.id == id) {
                    server.ready_at = Some(now);
                }
                reservation.release();
            }
            Ok(())
        };
        if let Err(error) = ready.await {
            if self.contains(&id) && self.is_alive(pid) {
                child.kill(false);
            } else if is_hunt || (is_ramsgate && self.city_pool_enabled()) {
                self.cleanup(&record).await?;
            }
            return Err(error);
        }
        Ok(self.find(&id).unwrap_or(record))
    }

    fn watch_exit(
        self: &Arc<Self>,
        record: &Gameserver,
        child: Arc<dyn GameProcess>,
        reservation: Release,
        ready_file: PathBuf,
    ) {
        let this = Arc::downgrade(self);
        let record = record.clone();
        let mut exit = child.exit();
        tokio::spawn(async move {
            let ended: Exit = loop {
                if let Some(ended) = exit.borrow().clone() {
                    break ended;
                }
                if exit.changed().await.is_err() {
                    return;
                }
            };
            let Some(deploy) = this.upgrade() else { return };
            deploy.lock().children.remove(&record.id);
            reservation.release();
            let _ = tokio::fs::remove_file(&ready_file).await;
            let is_hunt = !record.persistent();
            // Hunt ports go back at once instead of waiting up to a minute for the watchdog
            if (is_hunt || record.is_training_dojo || (record.is_ramsgate && deploy.city_pool_enabled()))
                && let Some(finished) = deploy.find(&record.id)
            {
                let _ = deploy.cleanup(&finished).await;
            }
            let how = match (&ended.signal, ended.code) {
                (Some(signal), _) => format!("on {signal}"),
                (None, Some(code)) => format!("with code {code}"),
                (None, None) => "with code null".into(),
            };
            let line = format!(
                "Game server on port {} (pid {}) exited {how}",
                record.port, record.process_id
            );
            if ended.code == Some(0) {
                tracing::info!("{line}");
            } else {
                tracing::warn!("{line}");
            }
        });
    }

    pub async fn ramsgate_connection(self: &Arc<Self>, size: u64) -> Result<Connection, DeployError> {
        if self.city_pool_enabled() {
            let server = self.cities.allocate(size).await?;
            return Ok(self.connection(&server));
        }
        let server = match self.current_world(World::Ramsgate) {
            Some(server) if !self.persistent_world_liveness() => server,
            current => {
                let why = if current.is_none() {
                    "Ramsgate is not running: starting it"
                } else {
                    "Ramsgate is not running any more: starting it again before sending anyone there"
                };
                self.ensure_persistent_world(World::Ramsgate, Some(why), false).await?
            }
        };
        Ok(self.connection(&server))
    }

    /// The Dojo starts on demand the first time someone is matchmade into it: a full game process
    /// most sessions never visit. Concurrent first requests share one launch.
    pub async fn dojo_connection(self: &Arc<Self>) -> Result<Connection, DeployError> {
        let server = match self.current_world(World::Dojo) {
            None => {
                self.ensure_persistent_world(World::Dojo, Some("Starting the Training Dojo on demand"), true)
                    .await?
            }
            Some(_) if self.persistent_world_liveness() => {
                self.ensure_persistent_world(
                    World::Dojo,
                    Some("The Training Dojo is not running any more: starting it again before sending anyone there"),
                    false,
                )
                .await?
            }
            Some(server) => server,
        };
        Ok(self.connection(&server))
    }

    fn connection(&self, server: &Gameserver) -> Connection {
        Connection {
            host: self.fixed.my_ip.clone(),
            port: server.port,
            session_id: Some(server.id.clone()),
        }
    }

    /// A hunt from the client's own game args (the tutorial): map, then `?...?Monster=behemoth`.
    pub async fn start_with_args(self: &Arc<Self>, game_args: &str) -> Result<Connection, DeployError> {
        let parts: Vec<&str> = game_args.split('?').collect();
        let behemoth = parts
            .get(2)
            .and_then(|p| p.split('=').nth(1))
            .ok_or_else(|| DeployError::failed("The game args have no behemoth"))?;
        let server = self
            .start_server(parts[0].to_owned(), Some(behemoth.to_owned()), None, None, false, false)
            .await?;
        Ok(self.connection(&server))
    }

    /// A catalog hunt (or this week's Trial) for a party.
    pub async fn start_with_hunt(
        self: &Arc<Self>,
        hunt_id: &str,
        players: &[String],
    ) -> Result<Connection, DeployError> {
        let trials = if hunt_id.contains("Arena") {
            Some(trials::trials_data(
                hunt_id.contains("Elite"),
                self.settings.get("TRIAL_ROTATION_START").as_deref(),
                crate::now_ms(),
            )?)
        } else {
            None
        };
        let missing = || DeployError::failed(format!("Hunt {hunt_id} is not in the tables"));
        let (matchmaker_hunt_id, behemoth, mut map) = match &trials {
            Some(trials) => (
                Some(trials.hunt_id.clone()),
                trials.behemoth.clone(),
                TRIALS_MAP_PATH.to_owned(),
            ),
            None => {
                let ids = tables::rows(tables::player_hunts())
                    .get(hunt_id)
                    .and_then(|row| row["MatchmakerHuntIDs"].as_array())
                    .ok_or_else(missing)?;
                let id = (!ids.is_empty())
                    .then(|| ids[rand::random_range(0..ids.len())]["RowName"].as_str())
                    .flatten()
                    .ok_or_else(missing)?;
                let row = tables::matchmaker_row(id).ok_or_else(missing)?;
                let behemoth = tables::behemoth_asset(row).ok_or_else(missing)?.to_owned();
                let maps = row["MapList"]
                    .as_array()
                    .filter(|m| !m.is_empty())
                    .ok_or_else(missing)?;
                let map = maps[rand::random_range(0..maps.len())]["MapAssetName"]
                    .as_str()
                    .ok_or_else(missing)?
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                (Some(id.to_owned()), behemoth, map)
            }
        };
        if let Some(id) = &matchmaker_hunt_id
            && !id.contains("Arena")
        {
            let mode = game_mode_override(id)?;
            if mode.contains("_C") {
                tracing::info!("Overriding gamemode to {mode}");
                map = format!("{map}?game={mode}");
            }
        }
        let expected = players
            .iter()
            .map(|uid| ExpectedPlayer {
                player_uid: uid.clone(),
                player_hunt_id: hunt_id.to_owned(),
            })
            .collect();
        let server = self
            .start_server(map, Some(behemoth), matchmaker_hunt_id, Some(expected), false, false)
            .await?;
        Ok(self.connection(&server))
    }

    /// The hunt ports, then Ramsgate (unless cities are pooled) and, with `ENABLE_DOJO=1`, the Dojo.
    pub async fn startup(self: &Arc<Self>) -> Result<(), DeployError> {
        {
            let mut state = self.lock();
            let (begin, end) = (self.fixed.port_range_begin, self.fixed.port_range_end);
            state.free_ports.extend(begin..=end.saturating_sub(2));
        }
        if self.settings.is("HUNT_WORKER", "1") && !self.settings.is("WORKER_RAMSGATE", "1") {
            return Ok(());
        }
        if !self.city_pool_enabled() {
            self.ensure_persistent_world(World::Ramsgate, None, false).await?;
        }
        // Opt in with ENABLE_DOJO=1 on a machine with RAM to spare; otherwise it starts on first use
        if self.settings.is("ENABLE_DOJO", "1") {
            self.ensure_persistent_world(World::Dojo, None, false).await?;
        }
        Ok(())
    }

    /// A failed start is one fatal line and a failure exit code for when the process ends; the
    /// deploy server keeps answering, and the next trip to Ramsgate tries again.
    pub async fn startup_and_report_failure(self: &Arc<Self>) {
        if let Err(error) = self.startup().await {
            if self.shutting_down.load(Ordering::SeqCst) {
                return;
            }
            service_kit::fatal!("Starting the game servers failed: {error}");
            self.startup_failed.store(true, Ordering::SeqCst);
        }
    }

    /// Stops a hunt the lifetime policy retired.
    pub fn stop_expired_hunt(&self, server: &Gameserver) -> bool {
        if server.persistent() || !self.contains(&server.id) {
            return false;
        }
        let child = self.lock().children.get(&server.id).cloned();
        child.is_some_and(|child| {
            child.pid() == Some(server.process_id) && child.exit().borrow().is_none() && child.kill(false)
        })
    }

    /// Stops every game process this deploy server started, without restarting any.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let children: Vec<Arc<dyn GameProcess>> = self.lock().children.values().cloned().collect();
        futures_util::future::join_all(children.into_iter().map(stop_child)).await;
        let mut state = self.lock();
        state.children.clear();
        state.gameservers.clear();
        state.ramsgate = None;
        state.dojo = None;
        self.launches.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    /// The watchdog's round: forgets (or restarts) servers whose process ended, and stops hunts
    /// past their lifetime.
    pub async fn run_watchdog(self: &Arc<Self>) {
        if self.watchdog_running.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!("Running Gameserver Watchdog!");
        let servers = self.gameservers();
        self.lifetime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .prune(&servers.iter().map(|s| s.id.clone()).collect());
        let mut cleanups = Vec::new();
        for server in servers {
            if !self.is_alive(server.process_id) {
                tracing::info!("Cleaning up Gameserver on port {}", server.port);
                let this = self.clone();
                cleanups.push(async move {
                    if let Err(error) = this.cleanup(&server).await {
                        tracing::error!("Could not restart the game server on port {}: {error}", server.port);
                    }
                });
                continue;
            }
            let now = crate::now_ms();
            let connections = native_occupancy(self.ready_dir().as_deref(), server.process_id, server.start_time, now);
            let hunt = Hunt {
                id: server.id.clone(),
                persistent: server.persistent(),
                start_time: server.start_time,
                ready_at: server.ready_at,
            };
            let reason = self
                .lifetime
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .reason(&hunt, connections, now);
            if let Some(reason) = reason
                && self.stop_expired_hunt(&server)
            {
                tracing::warn!(
                    port = server.port,
                    reason = reason.as_str(),
                    ageMinutes = (now - server.start_time) as f64 / 60_000.0,
                    "Hunt lifetime limit: stopping instance"
                );
            }
        }
        futures_util::future::join_all(cleanups).await;
        self.watchdog_running.store(false, Ordering::SeqCst);
    }

    // ---- tests only ----

    pub fn use_platform(&self, platform: Arc<dyn Platform>) {
        *self.platform.write().unwrap_or_else(PoisonError::into_inner) = platform;
    }

    pub fn reset_for_tests(&self) {
        self.cities.reset();
        *self.lock() = State::default();
        self.launches.lock().unwrap_or_else(PoisonError::into_inner).clear();
        self.shutting_down.store(false, Ordering::SeqCst);
        self.startup_failed.store(false, Ordering::SeqCst);
    }

    pub fn state_for_tests(&self) -> StateForTests {
        let state = self.lock();
        StateForTests {
            free_ports: state.free_ports.clone(),
            ramsgate: state.ramsgate.clone(),
            dojo: state.dojo.clone(),
        }
    }

    /// Puts a record back in the list (a test of a watchdog round that still holds a dead record).
    pub fn push_for_tests(&self, server: Gameserver) {
        self.lock().gameservers.push(server);
    }
}

/// Asks a process to end, forces it after 4 s, and gives up waiting after 5 s.
async fn stop_child(child: Arc<dyn GameProcess>) {
    let mut exit = child.exit();
    if exit.borrow().is_some() {
        return;
    }
    // A process that cannot be asked to end is left as it is
    if !child.kill(false) {
        return;
    }
    let ended = async {
        while exit.borrow().is_none() {
            if exit.changed().await.is_err() {
                return;
            }
        }
    };
    tokio::pin!(ended);
    if tokio::time::timeout(Duration::from_secs(4), &mut ended).await.is_ok() {
        return;
    }
    child.kill(true);
    let _ = tokio::time::timeout(Duration::from_secs(1), ended).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(id: &str, port: u16, map: &str) -> Gameserver {
        Gameserver {
            id: id.into(),
            port,
            map: map.into(),
            behemoth: None,
            matchmaker_hunt_id: None,
            expected_players: None,
            is_ramsgate: false,
            is_training_dojo: false,
            process_id: std::process::id(),
            ready_at: None,
            start_time: service_kit::js::date_parse("2026-09-21T12:00:00.000Z").unwrap(),
        }
    }

    fn servers() -> [Gameserver; 4] {
        let ramsgate = Gameserver {
            is_ramsgate: true,
            ..fake("11111111-1111-4111-8111-111111111111", 8777, RAMSGATE_MAP_PATH)
        };
        let dojo = Gameserver {
            is_training_dojo: true,
            ..fake("22222222-2222-4222-8222-222222222222", 8776, TRAINING_DOJO_MAP_PATH)
        };
        let tutorial = Gameserver {
            behemoth: Some("/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C".into()),
            ..fake(
                "33333333-3333-4333-8333-333333333333",
                8775,
                "/Game/Maps/islands/1705/dia_moss_triforce",
            )
        };
        let hunt = Gameserver {
            behemoth: Some("/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C".into()),
            matchmaker_hunt_id: Some("CR19_MatchmakerHunt_Lerawr_Beta".into()),
            expected_players: Some(
                ["UID-a", "UID-b"]
                    .iter()
                    .map(|uid| ExpectedPlayer {
                        player_uid: (*uid).into(),
                        player_hunt_id: "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr".into(),
                    })
                    .collect(),
            ),
            ..fake(
                "44444444-4444-4444-8444-444444444444",
                8774,
                "/Game/Maps/islands/1705/dia_moss_triforce_2?game=/Game/Blueprints/GameMode/Some_GameMode.Some_GameMode_C",
            )
        };
        [ramsgate, dojo, tutorial, hunt]
    }

    #[test]
    fn kinds() {
        let [ramsgate, dojo, tutorial, hunt] = servers();
        assert_eq!(
            [kind_of(&ramsgate), kind_of(&dojo), kind_of(&tutorial), kind_of(&hunt)],
            [Kind::City, Kind::Dojo, Kind::Tutorial, Kind::Hunt]
        );
        let regular = Gameserver {
            behemoth: Some("/Game/Monsters/lerawr/lerawr_normal_bp.lerawr_normal_bp_C".into()),
            ..fake("x", 1, "/Game/Maps/islands/1705/dia_moss_triforce_2")
        };
        assert_eq!(kind_of(&regular), Kind::Hunt);
        assert_eq!(
            kind_of(&fake("x", 1, "/Game/Maps/islands/1705/dia_moss_triforce")),
            Kind::Tutorial
        );
    }

    #[test]
    fn describes_each_server() {
        let all = servers();
        let listed = describe(&all, |_| true, false);
        assert_eq!(
            listed[0],
            json!({
                "id": all[0].id, "port": 8777, "kind": "city", "map": RAMSGATE_MAP_PATH, "gameMode": null,
                "behemoth": null, "huntId": null, "matchmakerHuntId": null, "expectedPlayers": [], "maxPlayers": null,
                "startedAt": "2026-09-21T12:00:00.000Z"
            })
        );
        assert_eq!(describe(&all, |_| true, true)[0]["maxPlayers"], 20);
        assert_eq!(
            (listed[1]["kind"].as_str(), listed[1]["maxPlayers"].as_i64()),
            (Some("dojo"), Some(12))
        );
        assert_eq!(
            (listed[2]["kind"].as_str(), listed[2]["maxPlayers"].as_i64()),
            (Some("tutorial"), Some(1))
        );
        assert_eq!(
            listed[3],
            json!({
                "id": all[3].id, "port": 8774, "kind": "hunt", "map": "/Game/Maps/islands/1705/dia_moss_triforce_2",
                "gameMode": "/Game/Blueprints/GameMode/Some_GameMode.Some_GameMode_C",
                "behemoth": "/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C",
                "huntId": "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", "matchmakerHuntId": "CR19_MatchmakerHunt_Lerawr_Beta",
                "expectedPlayers": ["UID-a", "UID-b"], "maxPlayers": 4, "startedAt": "2026-09-21T12:00:00.000Z"
            })
        );
        let dead = Gameserver {
            process_id: 999_999,
            ..all[3].clone()
        };
        let me = std::process::id();
        let listed = describe(&[all[0].clone(), dead], |pid| pid == me, false);
        assert_eq!(listed.len(), 1);
        let trial = Gameserver {
            matchmaker_hunt_id: Some("Arena_MatchmakerHunt_Hard_001".into()),
            ..fake("t", 1, "/Game/x")
        };
        assert_eq!(describe(&[trial], |_| true, false)[0]["maxPlayers"], 4);
        let unknown = Gameserver {
            matchmaker_hunt_id: Some("No_Such_Hunt".into()),
            ..fake("u", 1, "/Game/x")
        };
        assert_eq!(describe(&[unknown], |_| true, false)[0]["maxPlayers"], Value::Null);
    }
}
