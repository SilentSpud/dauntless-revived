//! Matchmaking (`controllers/matchmaking.ts`): hunt queues, party candidates and direct starts
//! (Ramsgate, the Dojo, the tutorial), each asking the deploy server for a game server.
//!
//! In memory, like parties, and under the same lock (`party::State::mm`): Node ran every step
//! between two awaits without interruption, so each step here runs with the lock held and never
//! across a request to the deploy server. A launch's synchronous start (its capacity turn, its
//! allocation record) runs inside the step that begins it, as `void PopQueue(...)` did in Node;
//! only the request and what follows it run on their own task.
//!
//! Identity checks Node made on objects (`MatchmakingResultMap.get(id) !== entry`) compare an
//! entry's serial here; queues live in a table by id.

pub mod capacity;
pub mod deploy;
pub mod region;
pub mod roster;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use diesel::QueryResult;
use serde_json::{Map, Value, json};
use service_kit::env::Env;

use crate::app::{AppError, AppState};
use crate::js;
use crate::party::{self, Candidate, CandidateState, LAST_CANDIDATE_REJOIN_MS};
use capacity::CapacityTurns;
use region::{Region, player_region, queue_key};
use roster::{Removal, Roster, Seat};

pub type Conn = store::Connection;

/// A player who joins the hunt their party is already on gets MATCHING for this long and nothing
/// new is started; if they are still asking after it, they are sent to the party's server again.
pub const REJOIN_PARK_MS: i64 = 20 * 1000;
const SOLO_REJOIN_WINDOW_MS: i64 = 60 * 1000;
const SOLO_JOIN_DEDUPE_MS: i64 = 30 * 1000;
/// The deploy server's 180 s startup grace, pacing, and its 120 s worker hop; one deadline for the
/// whole exchange.
const DEFAULT_DEPLOY_TIMEOUT_MS: i64 = 240 * 1000;
const HEALTH_TIMEOUT_MS: u64 = 3000;
const HEALTH_CACHE_MS: i64 = 5000;
/// A hunt queue goes to the deploy server this long after its first join (checked when a queued
/// player polls), or at once when it reaches [`QUEUE_FULL_PLAYERS`].
const QUEUE_WAIT_MS: i64 = 5 * 1000;
const QUEUE_FULL_PLAYERS: usize = 4;
/// A live player's polls renew this lease; disconnected waiters expire.
pub const CAPACITY_WAIT_MS: i64 = 60 * 1000;
/// The deploy server answers "capacity" whenever two launches already wait to spawn, so a retry
/// usually fits a few seconds later.
const CAPACITY_RETRY_MS: i64 = 3 * 1000;
/// A party's join waits this long at most for the deploy server, so the leader's first status
/// poll can already travel.
const PARTY_JOIN_WAIT_MS: u64 = 2500;
const WAIT_BRIEFLY_MS: u64 = 1500;
const DEPLOY_PATH: &str = "/api/matchmaker/handle-matchmaking-for-player";

// ---- Settings ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Disabled,
    DeployServer,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub mode: Mode,
    /// `DEPLOYSERVER_URL`: host:port, no scheme.
    pub deploy_url: Option<String>,
    pub deploy_timeout_ms: i64,
    /// `MATCHMAKING_CANCEL=1`: DELETE /candidate and /candidate/leave are answered.
    pub cancel: bool,
    pub qos_target_url: Option<String>,
    pub target_changelist: Option<String>,
    pub regions: region::Settings,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_env(&Env::from_pairs([]))
    }
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let mode = match env.raw("MATCHMAKING_MODE") {
            Some("DISABLED") => Mode::Disabled,
            Some("DEPLOYSERVER") => Mode::DeployServer,
            _ => Mode::Unsupported,
        };
        let deploy_timeout_ms = match env.raw("MATCHMAKING_DEPLOY_TIMEOUT_MS") {
            None => DEFAULT_DEPLOY_TIMEOUT_MS,
            Some(text) => match text.parse::<i64>() {
                Ok(value) if text.bytes().all(|b| b.is_ascii_digit()) && (1000..=900_000).contains(&value) => value,
                _ => {
                    tracing::warn!("Invalid MATCHMAKING_DEPLOY_TIMEOUT_MS; using 240000");
                    DEFAULT_DEPLOY_TIMEOUT_MS
                }
            },
        };
        Self {
            mode,
            deploy_url: env.raw("DEPLOYSERVER_URL").map(str::to_owned),
            deploy_timeout_ms,
            cancel: env.raw("MATCHMAKING_CANCEL") == Some("1"),
            qos_target_url: env.raw("QOS_TARGET_URL").map(str::to_owned),
            target_changelist: env.raw("TARGET_CHANGELIST").map(str::to_owned),
            regions: region::Settings::from_env(env),
        }
    }
}

/// Matchmaking's app-wide parts: its settings, and the one game-server health request at a time.
#[derive(Debug, Default)]
pub struct Matchmaking {
    pub settings: Settings,
    health_refresh: tokio::sync::Mutex<()>,
}

impl Matchmaking {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            health_refresh: tokio::sync::Mutex::new(()),
        }
    }
}

// ---- State ----

/// A player's matchmaking (`MatchmakingResult`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    serial: u64,
    pub region: Option<Region>,
    pub ready: bool,
    /// `None`: the join named no hunt (`undefined`).
    pub hunt_id: Option<Value>,
    pub candidate_id: String,
    pub host: String,
    pub port: i64,
    /// No game server could be started: the status poll answers FAILED.
    pub failed: bool,
    /// Part of a party's candidate (no per-hunt queue behind it).
    pub party_candidate: bool,
    /// The candidate's members, for the status reply's playerStates.
    pub party_member_ids: Option<Vec<String>>,
    pub parked_at: Option<i64>,
    pub private: bool,
    pub queued_at: Option<i64>,
    pub direct_key: Option<String>,
    pub launch_deadline: Option<i64>,
    pub session_id: Option<String>,
    pub allocated_at: Option<i64>,
    pub first_sent_at: Option<i64>,
}

impl Entry {
    fn new(candidate_id: String, hunt_id: Option<Value>) -> Self {
        Self {
            serial: 0,
            region: None,
            ready: false,
            hunt_id,
            candidate_id,
            host: String::new(),
            port: 0,
            failed: false,
            party_candidate: false,
            party_member_ids: None,
            parked_at: None,
            private: false,
            queued_at: None,
            direct_key: None,
            launch_deadline: None,
            session_id: None,
            allocated_at: None,
            first_sent_at: None,
        }
    }

    fn hunt_is(&self, hunt: &str) -> bool {
        self.hunt_id.as_ref().and_then(Value::as_str) == Some(hunt)
    }

    /// `${HuntId}`.
    pub fn hunt_text(&self) -> String {
        js::string(self.hunt_id.as_ref())
    }

    /// `GameSessionForCandidate`: a native session belongs to its allocation, never to a reused
    /// host:port.
    pub fn game_session(&self) -> &str {
        self.session_id.as_deref().unwrap_or(&self.candidate_id)
    }
}

#[derive(Debug, Clone)]
struct Queue {
    private: bool,
    region: Region,
    hunt_id: String,
    retry_after: Option<i64>,
    capacity_deadline: Option<i64>,
    players: Vec<String>,
    /// Player -> candidate, in insertion order (a JavaScript Map).
    candidate_ids: Vec<(String, String)>,
    first_player_added_time: i64,
    resolved: bool,
}

impl Queue {
    fn candidate_of(&self, player: &str) -> Option<&String> {
        self.candidate_ids.iter().find(|(p, _)| p == player).map(|(_, c)| c)
    }

    fn set_candidate(&mut self, player: &str, candidate: &str) {
        match self.candidate_ids.iter_mut().find(|(p, _)| p == player) {
            Some(slot) => slot.1 = candidate.to_owned(),
            None => self.candidate_ids.push((player.to_owned(), candidate.to_owned())),
        }
    }

    fn forget(&mut self, player: &str) {
        self.candidate_ids.retain(|(p, _)| p != player);
    }

    fn key(&self) -> String {
        queue_key(&self.hunt_id, self.region)
    }
}

#[derive(Debug, Clone)]
struct LastSent {
    host: String,
    port: i64,
    session_id: Option<String>,
}

/// A party launch: what its retries need.
#[derive(Debug, Clone)]
struct PartyLaunch {
    party_id: String,
    candidate_id: String,
    members: Vec<String>,
    game_mode: String,
    hunt_id: String,
    region: Region,
}

/// A direct start (Ramsgate, the Dojo, the tutorial): what its retries need.
#[derive(Debug, Clone)]
struct DirectLaunch {
    player: String,
    serial: u64,
    candidate_id: String,
    game_mode: Option<Value>,
    game_args: Option<Value>,
    hunt_id: Option<Value>,
    region: Region,
}

#[derive(Debug, Clone)]
enum Retry {
    Party(PartyLaunch),
    Direct(DirectLaunch),
}

/// A candidate waiting for capacity: only an active candidate's status poll retries it.
#[derive(Debug, Clone)]
struct CapacityWait {
    serial: u64,
    next: i64,
    deadline: i64,
    busy: bool,
    retry: Retry,
    retry_ms: i64,
}

#[derive(Debug, Clone)]
struct Health {
    started_at: i64,
    until: i64,
    complete: bool,
    sessions: HashSet<String>,
}

#[derive(Debug)]
pub struct State {
    queues: HashMap<u64, Queue>,
    queue_by_key: HashMap<String, u64>,
    player_queue: HashMap<String, u64>,
    /// Queues whose launch is out, kept even when no map holds them.
    popping: HashSet<u64>,
    results: BTreeMap<String, Entry>,
    /// The server each player was last told to travel to.
    last_sent: HashMap<String, LastSent>,
    waits: HashMap<String, CapacityWait>,
    turns: CapacityTurns,
    roster: Roster,
    health: Option<Health>,
    next_serial: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            queues: HashMap::new(),
            queue_by_key: HashMap::new(),
            player_queue: HashMap::new(),
            popping: HashSet::new(),
            results: BTreeMap::new(),
            last_sent: HashMap::new(),
            waits: HashMap::new(),
            turns: CapacityTurns::new(CAPACITY_WAIT_MS),
            roster: Roster::default(),
            health: None,
            next_serial: 0,
        }
    }
}

impl State {
    fn serial(&mut self) -> u64 {
        self.next_serial += 1;
        self.next_serial
    }

    fn entry_is(&self, player: &str, serial: u64) -> bool {
        self.results.get(player).is_some_and(|e| e.serial == serial)
    }

    /// The candidate-leave hook: a member who leaves the party while its candidate is still
    /// starting doesn't go along.
    pub fn candidate_left(&mut self, user: &str, candidate: &str) {
        if let Some(entry) = self.results.get(user)
            && entry.party_candidate
            && entry.candidate_id == candidate
            && entry.first_sent_at.is_none()
        {
            self.roster.remove_player(user, candidate, Removal::Cancelled);
            self.results.remove(user);
            tracing::info!("mm: {user} left the party before candidate {candidate} got its server; they stay behind");
        }
    }

    /// `GetLastMatchmakingResult` (ServerStatus): ready, hunt and port of the player's entry.
    pub fn last_result(&self, player: &str) -> Option<(bool, Option<Value>, i64)> {
        self.results.get(player).map(|e| (e.ready, e.hunt_id.clone(), e.port))
    }

    /// `StoreEntry`.
    fn store_entry(&mut self, player: &str, mut entry: Entry, now: i64) -> u64 {
        entry.queued_at.get_or_insert(now);
        entry.serial = self.serial();
        if let Some(previous) = self.results.get(player)
            && previous.candidate_id != entry.candidate_id
        {
            let previous = previous.candidate_id.clone();
            self.roster.remove_player(player, &previous, Removal::Replaced);
        }
        let (candidate, hunt) = (entry.candidate_id.clone(), entry.hunt_text_or_empty());
        let serial = entry.serial;
        self.results.insert(player.to_owned(), entry);
        self.roster.record_queued(player, &candidate, &hunt, now);
        serial
    }

    /// Queues no map and no launch refer to any more.
    fn forget_unused_queues(&mut self) {
        let used: HashSet<u64> = self
            .queue_by_key
            .values()
            .chain(self.player_queue.values())
            .chain(self.popping.iter())
            .copied()
            .collect();
        self.queues.retain(|id, _| used.contains(id));
    }

    /// `LeaveWaitingQueues`: whatever replaces a player's candidate takes them out of every queue
    /// still waiting. A queue already asking the deploy server is left alone.
    fn leave_waiting_queues(&mut self, player: &str, why: &str) {
        let Some(&id) = self.player_queue.get(player) else {
            return;
        };
        let Some(queue) = self.queues.get_mut(&id) else {
            return;
        };
        if queue.resolved || !queue.players.iter().any(|p| p == player) {
            return;
        }
        queue.players.retain(|p| p != player);
        queue.forget(player);
        let (empty, key, hunt) = (queue.players.is_empty(), queue.key(), queue.hunt_id.clone());
        self.player_queue.remove(player);
        if empty && self.queue_by_key.get(&key) == Some(&id) {
            self.queue_by_key.remove(&key);
        }
        tracing::info!("mm: {player} taken out of the {hunt} queue: {why}");
    }
}

impl Entry {
    fn hunt_text_or_empty(&self) -> String {
        match &self.hunt_id {
            None | Some(Value::Null) => String::new(),
            Some(value) => js::string(Some(value)),
        }
    }
}

// ---- Steps that change parties as well ----

/// `FailCandidate`: every entry of the candidate answers FAILED, and the parties forget it.
fn fail_candidate(s: &mut party::State, candidate: &str, reason: &str) {
    s.mm.waits.remove(candidate);
    s.mm.turns.remove(candidate);
    s.mm.roster.cancel_candidate(candidate);
    let players: Vec<String> =
        s.mm.results
            .iter_mut()
            .filter(|(_, e)| e.candidate_id == candidate)
            .map(|(player, e)| {
                e.failed = true;
                e.ready = false;
                e.launch_deadline = None;
                player.clone()
            })
            .collect();
    for player in players {
        if let Some(party) = s.party_id_of(&player) {
            s.clear_candidate_if(&party, candidate, reason);
            s.drop_last_candidate_if(&party, candidate);
        }
    }
}

fn allocation_expired(entry: &Entry, now: i64) -> bool {
    let (at, lifetime) = if entry.party_candidate {
        (entry.allocated_at, LAST_CANDIDATE_REJOIN_MS)
    } else {
        (entry.first_sent_at.or(entry.allocated_at), SOLO_REJOIN_WINDOW_MS)
    };
    at.is_some_and(|at| now - at >= lifetime)
}

/// What a launch got (`LaunchResult`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Launched {
    capacity: bool,
    retry_after_ms: Option<i64>,
    succeeded: bool,
    host: String,
    port: i64,
    session_id: Option<String>,
    allocated_at: Option<i64>,
}

impl Launched {
    fn failed() -> Self {
        Self::default()
    }
}

/// `ApplyLaunchResult`.
fn apply_launch_result(s: &mut party::State, player: &str, serial: u64, game: &Launched, now: i64) {
    let Some(entry) = s.mm.results.get_mut(player).filter(|e| e.serial == serial && !e.failed) else {
        return;
    };
    entry.launch_deadline = None;
    if game.succeeded {
        entry.host = game.host.clone();
        entry.port = game.port;
        entry.session_id = game.session_id.clone();
        entry.allocated_at = Some(game.allocated_at.unwrap_or(now));
        entry.ready = true;
    } else {
        let candidate = entry.candidate_id.clone();
        fail_candidate(s, &candidate, "no game server could be started");
    }
}

/// `PruneWaitingQueue`: queued players who stopped polling fail and leave.
fn prune_waiting_queue(s: &mut party::State, id: u64, now: i64) {
    let Some(queue) = s.mm.queues.get(&id) else {
        return;
    };
    if queue.resolved {
        return;
    }
    let mut kept = Vec::new();
    for player in queue.players.clone() {
        let candidate = s.mm.queues.get(&id).and_then(|q| q.candidate_of(&player)).cloned();
        if let Some(candidate) = &candidate
            && s.mm.roster.queue_member_active(&player, candidate, now)
        {
            kept.push(player);
            continue;
        }
        if let Some(candidate) = &candidate {
            fail_candidate(s, candidate, "queued player stopped polling");
        }
        if let Some(queue) = s.mm.queues.get_mut(&id) {
            queue.forget(&player);
        }
        if s.mm.player_queue.get(&player) == Some(&id) {
            s.mm.player_queue.remove(&player);
        }
    }
    if let Some(queue) = s.mm.queues.get_mut(&id) {
        queue.players = kept;
        let key = queue.key();
        if queue.players.is_empty() && s.mm.queue_by_key.get(&key) == Some(&id) {
            s.mm.queue_by_key.remove(&key);
        }
    }
}

/// `ParkCapacity`: the candidate waits for capacity; its members' status polls retry it.
fn park_capacity(s: &mut party::State, candidate: &str, retry: Retry, retry_ms: i64, now: i64) -> bool {
    let ids: Vec<(String, i64)> = s.mm.waits.iter().map(|(id, w)| (id.clone(), w.deadline)).collect();
    for (id, deadline) in ids {
        let waiting =
            s.mm.results
                .values()
                .filter(|e| e.candidate_id == id && !e.failed && !e.ready)
                .count();
        if deadline < now {
            fail_candidate(s, &id, "capacity wait expired");
        }
        if deadline < now || waiting == 0 {
            s.mm.waits.remove(&id);
        }
    }
    if s.mm.waits.len() >= 1000 {
        return false;
    }
    let serial = s.mm.serial();
    s.mm.waits.insert(
        candidate.to_owned(),
        CapacityWait {
            serial,
            next: now + retry_ms,
            deadline: now + CAPACITY_WAIT_MS,
            busy: false,
            retry,
            retry_ms,
        },
    );
    let member =
        s.mm.results
            .iter()
            .find(|(_, e)| e.candidate_id == candidate && e.party_candidate)
            .map(|(player, _)| player.clone());
    if let Some(party) = member.and_then(|m| s.party_id_of(&m))
        && let Some(c) = s.party_mut(&party).and_then(party::Party::candidate_mut)
        && c.candidate_id == candidate
    {
        c.capacity_deadline = Some(now + CAPACITY_WAIT_MS);
    }
    true
}

fn hunt_requires_matchmaking(hunt: &str) -> bool {
    !hunt.contains("Ramsgate") && !hunt.contains("Dojo")
}

/// `DistinctPlayers`: each account once, in first-seen order. A hunt server waits for every
/// expected player, so a player listed twice keeps it waiting for someone who never comes.
pub fn distinct_players(players: &[String]) -> Vec<String> {
    let mut seen = Vec::with_capacity(players.len());
    for player in players {
        if !seen.contains(player) {
            seen.push(player.clone());
        }
    }
    seen
}

// ---- Launches ----

/// A launch begun: decided at once, or a request to send.
#[derive(Debug, Clone)]
enum LaunchStart {
    Decided(Launched),
    Ask {
        body: String,
        /// The candidate holding a capacity turn, released when the answer comes.
        turn: Option<String>,
    },
}

fn is_mode(mode: Option<&Value>, modes: &[&str]) -> bool {
    mode.and_then(Value::as_str).is_some_and(|m| modes.contains(&m))
}

/// `LaunchGameOnDeployserver`'s synchronous start: the capacity turn of an island or city
/// candidate, then the request (`LaunchGameOnDeployserverNow` up to the fetch).
fn launch_begin(
    s: &mut party::State,
    game_mode: Option<&Value>,
    game_args: Option<&Value>,
    hunt: Option<&Value>,
    asked: Option<&[String]>,
    region: &str,
    now: i64,
) -> LaunchStart {
    let candidate = match asked {
        Some(players) if is_mode(game_mode, &["ISLAND", "CITY"]) && !players.is_empty() => {
            s.mm.results.get(&players[0]).map(|e| e.candidate_id.clone())
        }
        _ => None,
    };
    if let (Some(candidate), Some(players)) = (&candidate, asked) {
        let active: HashSet<String> =
            s.mm.results
                .values()
                .filter(|e| !e.ready && !e.failed)
                .map(|e| e.candidate_id.clone())
                .collect();
        let queued_at = players
            .iter()
            .map(|id| s.mm.results.get(id).and_then(|e| e.queued_at).unwrap_or(now))
            .min()
            .unwrap_or(now);
        // City seats and hunt processes are separate resources; a full city must not hold up hunts
        let lane = format!("{region}:{}", js::string(game_mode));
        if !s.mm.turns.start(candidate, &lane, now, &active, queued_at) {
            return LaunchStart::Decided(Launched {
                capacity: true,
                ..Launched::failed()
            });
        }
    }
    let expected = asked.map(distinct_players);
    if let (Some(players), Some(expected)) = (asked, &expected)
        && expected.len() != players.len()
    {
        tracing::warn!(
            "mm: {}: {} repeated expected player(s) dropped ({})",
            js::string(hunt),
            players.len() - expected.len(),
            players.join(",")
        );
    }
    tracing::info!(
        "Querying DeployServer for GameMode: {} HuntId {} with {} Expected Players!",
        js::string(game_mode),
        js::string(hunt),
        expected
            .as_ref()
            .map_or("undefined".to_owned(), |e| e.len().to_string())
    );
    let mut body = Map::new();
    for (name, value) in [("GameMode", game_mode), ("GameArgs", game_args), ("HuntId", hunt)] {
        if let Some(value) = value {
            body.insert(name.into(), value.clone());
        }
    }
    if let Some(expected) = expected {
        body.insert("ExpectedPlayers".into(), json!(expected));
    }
    if is_mode(game_mode, &["ISLAND", "CITY", "SHARED"]) && region != "main" {
        body.insert("Region".into(), json!(region));
    }
    LaunchStart::Ask {
        body: service_kit::jsjson::stringify(&Value::Object(body)),
        turn: candidate,
    }
}

/// The request and its answer; never fails (an unreachable deploy server or an unusable answer is
/// a failed launch).
async fn launch_finish(app: &AppState, start: LaunchStart) -> Launched {
    let (body, turn) = match start {
        LaunchStart::Decided(result) => return result,
        LaunchStart::Ask { body, turn } => (body, turn),
    };
    let settings = &app.matchmaking.settings;
    let authority = settings.deploy_url.clone().unwrap_or_else(|| "undefined".into());
    let reply = deploy::request(
        &authority,
        hyper::Method::POST,
        DEPLOY_PATH,
        Some(body),
        &[200, 503],
        settings.deploy_timeout_ms as u64,
    )
    .await;
    let result = match reply {
        // A timeout or reset may have started a remote game: this attempt ends rather than
        // asking for another allocation with an ambiguous result
        Err(error) => {
            tracing::error!("DeployServer allocation failed: {error}");
            Launched::failed()
        }
        Ok(reply) => read_launch(reply, app.party_clock.now()),
    };
    if let Some(turn) = turn {
        app.parties
            .lock()
            .mm
            .turns
            .finish(&turn, result.capacity, app.party_clock.now());
    }
    result
}

fn read_launch(reply: deploy::Reply, now: i64) -> Launched {
    let body = reply.body.unwrap_or(Value::Null);
    if reply.status == 503 {
        let reason = body.get("error").and_then(Value::as_str) == Some("capacity_unavailable")
            && body
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|r| ["memory", "ports", "hunts", "cpu"].contains(&r));
        if !reason {
            return Launched::failed();
        }
        tracing::warn!(
            reason = js::string(body.get("reason")),
            capacity_wait_ms = CAPACITY_WAIT_MS,
            "mm: allocation waiting for capacity"
        );
        let seconds = reply
            .retry_after
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| js::number(Some(&json!(text))));
        let retry_after_ms = match seconds {
            Some(seconds) if seconds.is_finite() && seconds > 0.0 => {
                (seconds * 1000.0).max(CAPACITY_RETRY_MS as f64).min(30_000.0) as i64
            }
            _ => CAPACITY_RETRY_MS,
        };
        return Launched {
            capacity: true,
            retry_after_ms: Some(retry_after_ms),
            ..Launched::failed()
        };
    }
    if reply.status != 200 {
        tracing::error!("DeployServer returned status {}", reply.status);
        return Launched::failed();
    }
    let host = body.get("host").and_then(Value::as_str).filter(|h| !h.is_empty());
    let port = body
        .get("port")
        .and_then(Value::as_f64)
        .filter(|p| p.fract() == 0.0 && *p > 0.0 && *p <= 65535.0);
    // The client needs a non-empty host and a numeric port, or it polls forever or refuses the
    // session
    let (Some(host), Some(port)) = (host, port) else {
        tracing::error!("DeployServer returned no usable game server");
        return Launched::failed();
    };
    let session = body
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    tracing::info!("DeployServer returned gameserver {host}:{port}");
    Launched {
        capacity: false,
        retry_after_ms: None,
        succeeded: true,
        host: host.to_owned(),
        port: port as i64,
        session_id: session.map(str::to_owned),
        allocated_at: Some(now),
    }
}

/// `AllocationHealth`: whether the deploy server still runs the entry's session (`None`: unknown).
async fn allocation_health(app: &AppState, entry: &Entry) -> Option<bool> {
    let session = entry.session_id.as_ref()?;
    let allocated = entry.allocated_at.unwrap_or(0);
    if app.party_clock.now() - allocated < HEALTH_CACHE_MS {
        return None;
    }
    let fresh = |app: &AppState| {
        app.parties
            .lock()
            .mm
            .health
            .as_ref()
            .is_some_and(|h| h.until > app.party_clock.now())
    };
    if !fresh(app) {
        let _turn = app.matchmaking.health_refresh.lock().await;
        if !fresh(app) {
            let started_at = app.party_clock.now();
            let authority = app
                .matchmaking
                .settings
                .deploy_url
                .clone()
                .unwrap_or_else(|| "undefined".into());
            let ok: Vec<u16> = (200..300).collect();
            let reply = deploy::request(
                &authority,
                hyper::Method::GET,
                "/gameservers",
                None,
                &ok,
                HEALTH_TIMEOUT_MS,
            )
            .await;
            let (mut complete, mut sessions) = (false, HashSet::new());
            // An unavailable worker is unknown, not proof of a dead process
            if let Ok(deploy::Reply { body: Some(body), .. }) = reply
                && let Some(servers) = body.get("servers").and_then(Value::as_array)
                && servers.iter().all(|s| s.get("id").is_some_and(Value::is_string))
            {
                sessions = servers
                    .iter()
                    .filter_map(|s| s.get("id").and_then(Value::as_str).map(str::to_owned))
                    .collect();
                complete = body.get("complete") == Some(&Value::Bool(true));
            }
            app.parties.lock().mm.health = Some(Health {
                started_at,
                until: app.party_clock.now() + HEALTH_CACHE_MS,
                complete,
                sessions,
            });
        }
    }
    let state = app.parties.lock();
    let snapshot = state.mm.health.as_ref()?;
    if snapshot.started_at < allocated {
        return None;
    }
    if snapshot.sessions.contains(session) {
        return Some(true);
    }
    snapshot.complete.then_some(false)
}

/// `ReadyAllocationUsable`: a ready entry's server is still worth sending players to.
async fn ready_allocation_usable(app: &AppState, player: &str, entry: &Entry) -> bool {
    {
        let mut s = app.parties.lock();
        if entry.failed {
            return false;
        }
        if entry.party_candidate && allocation_expired(entry, app.party_clock.now()) {
            fail_candidate(&mut s, &entry.candidate_id, "allocation reuse expired");
            return false;
        }
    }
    let health = allocation_health(app, entry).await;
    let mut s = app.parties.lock();
    if health == Some(false) {
        fail_candidate(&mut s, &entry.candidate_id, "allocated game server exited");
        return false;
    }
    let current =
        s.mm.results
            .get(player)
            .filter(|e| e.serial == entry.serial)
            .cloned()
            .unwrap_or_else(|| entry.clone());
    // Positive session identity keeps a healthy running hunt reusable; without it, polls can't
    // keep extending the original lease
    if health != Some(true) && allocation_expired(&current, app.party_clock.now()) {
        fail_candidate(&mut s, &entry.candidate_id, "allocation reuse expired");
        return false;
    }
    let failed =
        s.mm.results
            .get(player)
            .filter(|e| e.serial == entry.serial)
            .map_or(current.failed, |e| e.failed);
    !failed
}

/// Runs `work` on its own task and waits for it at most `ms` (`WaitBriefly`); the task goes on.
async fn wait_briefly<F>(work: F, ms: u64)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let task = tokio::spawn(work);
    let _ = tokio::time::timeout(Duration::from_millis(ms), task).await;
}

// ---- Queues ----

/// A queue launch begun (`PopQueue` up to the request).
#[derive(Debug, Clone)]
struct PopTicket {
    queue: u64,
    hunt: String,
    allocation: String,
    deadline: i64,
    launch: LaunchStart,
}

/// `PopQueue`'s synchronous start.
fn pop_queue_begin(s: &mut party::State, settings: &Settings, hunt: &str, id: u64, now: i64) -> Option<PopTicket> {
    let queue = s.mm.queues.get(&id)?;
    if queue.resolved || queue.retry_after.is_some_and(|at| at > now) {
        return None;
    }
    prune_waiting_queue(s, id, now);
    let queue = s.mm.queues.get_mut(&id)?;
    if queue.players.is_empty() {
        return None;
    }
    queue.resolved = true;
    let key = queue_key(hunt, queue.region);
    let (players, region, capacity_deadline) = (queue.players.clone(), queue.region, queue.capacity_deadline);
    let seats: Vec<Seat> = players
        .iter()
        .map(|p| Seat {
            player_id: p.clone(),
            candidate_id: queue.candidate_of(p).cloned().unwrap_or_default(),
        })
        .collect();
    if s.mm.queue_by_key.get(&key) == Some(&id) {
        s.mm.queue_by_key.remove(&key);
    }
    let allocation = crate::crypto::random_uuid();
    let deadline = now + settings.deploy_timeout_ms;
    for seat in &seats {
        if let Some(entry) = s.mm.results.get_mut(&seat.player_id)
            && entry.candidate_id == seat.candidate_id
        {
            entry.launch_deadline = Some(deadline);
        }
    }
    s.mm.roster.begin_allocation(&allocation, &seats, deadline, now);
    let launch = if capacity_deadline.is_some_and(|at| now >= at) {
        LaunchStart::Decided(Launched::failed())
    } else {
        launch_begin(
            s,
            Some(&json!("ISLAND")),
            Some(&json!("")),
            Some(&json!(hunt)),
            Some(&players),
            region.as_str(),
            now,
        )
    };
    s.mm.popping.insert(id);
    Some(PopTicket {
        queue: id,
        hunt: hunt.to_owned(),
        allocation,
        deadline,
        launch,
    })
}

/// `PopQueue` after the request: a capacity refusal puts the queue back to wait (still taking
/// joiners); anything else answers its players.
async fn pop_queue_finish(app: AppState, ticket: PopTicket) {
    let game = launch_finish(&app, ticket.launch).await;
    let now = app.party_clock.now();
    let mut guard = app.parties.lock();
    let s = &mut *guard;
    let id = ticket.queue;
    s.mm.popping.remove(&id);
    let Some(queue) = s.mm.queues.get(&id).cloned() else {
        return;
    };
    if game.capacity {
        s.mm.roster.release_allocation(&ticket.allocation);
        for (player, candidate) in &queue.candidate_ids {
            if let Some(entry) = s.mm.results.get_mut(player)
                && entry.candidate_id == *candidate
            {
                entry.launch_deadline = None;
            }
        }
        let Some(queue) = s.mm.queues.get_mut(&id) else {
            return;
        };
        let deadline = *queue.capacity_deadline.get_or_insert(now + CAPACITY_WAIT_MS);
        if now < deadline && !queue.players.is_empty() {
            queue.resolved = false;
            queue.retry_after = Some(now + game.retry_after_ms.unwrap_or(CAPACITY_RETRY_MS));
            // A private retry never enters or merges with the public pool
            if queue.private {
                s.mm.forget_unused_queues();
                return;
            }
            // Keep taking joiners while waiting. Players who joined while the request was out
            // opened a fresh queue under this key: fold them in if they fit and it hasn't asked
            let key = queue.key();
            let size = queue.players.len();
            let newer = s.mm.queue_by_key.get(&key).copied();
            match newer {
                Some(newer)
                    if newer != id
                        && s.mm.queues.get(&newer).is_some_and(|n| {
                            !n.resolved && n.retry_after.is_none() && size + n.players.len() <= QUEUE_FULL_PLAYERS
                        }) =>
                {
                    let moved = s.mm.queues.get(&newer).cloned();
                    if let Some(moved) = moved {
                        for player in &moved.players {
                            let Some(candidate) = moved.candidate_of(player) else {
                                continue;
                            };
                            let Some(queue) = s.mm.queues.get_mut(&id) else {
                                break;
                            };
                            if queue.players.contains(player) {
                                continue;
                            }
                            queue.players.push(player.clone());
                            queue.set_candidate(player, candidate);
                            if s.mm.player_queue.get(player) == Some(&newer) {
                                s.mm.player_queue.insert(player.clone(), id);
                            }
                        }
                    }
                    if let Some(old) = s.mm.queues.get_mut(&newer) {
                        old.players.clear();
                        old.candidate_ids.clear();
                        old.resolved = true;
                    }
                    s.mm.queue_by_key.insert(key, id);
                }
                None => {
                    s.mm.queue_by_key.insert(key, id);
                }
                Some(_) => {}
            }
            s.mm.forget_unused_queues();
            return;
        }
    }
    let queue = s.mm.queues.get(&id).cloned().unwrap_or(queue);
    if game.succeeded {
        s.mm.roster
            .assign_allocation(&ticket.allocation, game.session_id.as_deref(), now);
    }
    for player in &queue.players {
        let candidate = queue.candidate_of(player);
        if let (Some(entry), Some(candidate)) = (s.mm.results.get(player), candidate)
            && entry.candidate_id == *candidate
            && !entry.ready
            && !entry.party_candidate
            && entry.hunt_is(&ticket.hunt)
        {
            let serial = entry.serial;
            if now >= ticket.deadline || queue.capacity_deadline.is_some_and(|at| now >= at) {
                fail_candidate(s, candidate, "allocation deadline expired");
            } else {
                apply_launch_result(s, player, serial, &game, now);
            }
        }
        if s.mm.player_queue.get(player) == Some(&id) {
            s.mm.player_queue.remove(player);
        }
    }
    if !game.succeeded {
        tracing::warn!(
            "mm: the {} queue ({}) got no game server; their status polls answer FAILED",
            ticket.hunt,
            queue.players.join(",")
        );
    }
    s.mm.forget_unused_queues();
}

fn spawn_pops(app: &AppState, tickets: Vec<PopTicket>) {
    for ticket in tickets {
        tokio::spawn(pop_queue_finish(app.clone(), ticket));
    }
}

// ---- Party and direct launches ----

/// A launch try begun (`TryLaunch` up to the request).
#[derive(Debug, Clone)]
enum TryStart {
    Done,
    Asking {
        allocation: String,
        deadline: i64,
        active: Vec<String>,
        launch: LaunchStart,
    },
}

fn party_try_begin(s: &mut party::State, settings: &Settings, launch: &PartyLaunch, now: i64) -> TryStart {
    let members = s.party(&launch.party_id).map(|p| p.members.clone()).unwrap_or_default();
    let active: Vec<String> = launch
        .members
        .iter()
        .filter(|m| {
            s.mm.results
                .get(*m)
                .is_some_and(|e| e.candidate_id == launch.candidate_id && !e.failed)
                && members.contains(m)
        })
        .cloned()
        .collect();
    let current = s
        .party(&launch.party_id)
        .and_then(party::Party::candidate)
        .is_some_and(|c| c.candidate_id == launch.candidate_id);
    if !current || active.is_empty() {
        fail_candidate(s, &launch.candidate_id, "party candidate no longer active");
        return TryStart::Done;
    }
    let deadline = now + settings.deploy_timeout_ms;
    if let Some(candidate) = s.party_mut(&launch.party_id).and_then(party::Party::candidate_mut) {
        candidate.launch_deadline = Some(deadline);
    }
    for member in &active {
        if let Some(entry) = s.mm.results.get_mut(member) {
            entry.launch_deadline = Some(deadline);
        }
    }
    let allocation = crate::crypto::random_uuid();
    let seats: Vec<Seat> = active
        .iter()
        .map(|p| Seat {
            player_id: p.clone(),
            candidate_id: launch.candidate_id.clone(),
        })
        .collect();
    s.mm.roster.begin_allocation(&allocation, &seats, deadline, now);
    let mode = json!(launch.game_mode);
    let asked = ["ISLAND", "CITY"]
        .contains(&launch.game_mode.as_str())
        .then_some(active.as_slice());
    let start = launch_begin(
        s,
        Some(&mode),
        Some(&json!("")),
        Some(&json!(launch.hunt_id)),
        asked,
        launch.region.as_str(),
        now,
    );
    TryStart::Asking {
        allocation,
        deadline,
        active,
        launch: start,
    }
}

/// The party try after the request: done, or a capacity refusal with the delay to retry after.
async fn party_try_finish(app: &AppState, launch: &PartyLaunch, start: TryStart) -> (bool, Option<i64>) {
    let TryStart::Asking {
        allocation,
        deadline,
        active,
        launch: request,
    } = start
    else {
        return (true, None);
    };
    let game = launch_finish(app, request).await;
    let now = app.party_clock.now();
    let mut guard = app.parties.lock();
    let s = &mut *guard;
    let wait_over = s.mm.waits.get(&launch.candidate_id).is_some_and(|w| w.deadline <= now);
    if now >= deadline || wait_over {
        fail_candidate(s, &launch.candidate_id, "allocation deadline expired");
        return (true, None);
    }
    if game.capacity {
        s.mm.roster.release_allocation(&allocation);
        if let Some(candidate) = s.party_mut(&launch.party_id).and_then(party::Party::candidate_mut)
            && candidate.candidate_id == launch.candidate_id
        {
            candidate.launch_deadline = None;
        }
        for member in &active {
            if let Some(entry) = s.mm.results.get_mut(member)
                && entry.candidate_id == launch.candidate_id
            {
                entry.launch_deadline = None;
            }
        }
        return (false, Some(game.retry_after_ms.unwrap_or(CAPACITY_RETRY_MS)));
    }
    if game.succeeded {
        s.mm.roster
            .assign_allocation(&allocation, game.session_id.as_deref(), now);
    }
    finish_party_candidate(s, &launch.party_id, &launch.candidate_id, &active, &game, now);
    (true, None)
}

/// `FinishPartyCandidate`.
fn finish_party_candidate(
    s: &mut party::State,
    party_id: &str,
    candidate: &str,
    members: &[String],
    game: &Launched,
    now: i64,
) {
    let mut updated = 0;
    for member in members {
        let Some(entry) = s.mm.results.get(member) else {
            continue;
        };
        if entry.candidate_id != candidate || entry.failed {
            continue;
        }
        let serial = entry.serial;
        apply_launch_result(s, member, serial, game, now);
        updated += 1;
    }
    if game.succeeded {
        s.mark_candidate_ready(party_id, candidate, &game.host, game.port as u16);
        if let Some(c) = s.party_mut(party_id).and_then(party::Party::candidate_mut)
            && c.candidate_id == candidate
        {
            c.session_id = game.session_id.clone();
            c.allocated_at = Some(game.allocated_at.unwrap_or(now));
            c.launch_deadline = None;
        }
        tracing::info!(
            "mm: party P={party_id} candidate {candidate} ready at {}:{} for {updated} member(s)",
            game.host,
            game.port
        );
    } else {
        s.clear_candidate_if(party_id, candidate, "no game server could be started");
        tracing::warn!(
            "mm: party P={party_id} candidate {candidate} got no game server; {updated} member(s) will see FAILED"
        );
    }
}

/// The party's first try, and what happens when it ends in a capacity refusal.
async fn party_launch(app: AppState, launch: PartyLaunch, start: TryStart) {
    let (done, retry_ms) = party_try_finish(&app, &launch, start).await;
    if done {
        return;
    }
    let mut guard = app.parties.lock();
    let s = &mut *guard;
    let retry_ms = retry_ms.unwrap_or(CAPACITY_RETRY_MS);
    if !park_capacity(
        s,
        &launch.candidate_id,
        Retry::Party(launch.clone()),
        retry_ms,
        app.party_clock.now(),
    ) {
        finish_party_candidate(
            s,
            &launch.party_id,
            &launch.candidate_id,
            &launch.members,
            &Launched::failed(),
            app.party_clock.now(),
        );
    }
}

fn direct_try_begin(s: &mut party::State, settings: &Settings, launch: &DirectLaunch, now: i64) -> TryStart {
    if !s.mm.entry_is(&launch.player, launch.serial) || s.mm.results.get(&launch.player).is_some_and(|e| e.failed) {
        return TryStart::Done;
    }
    let deadline = now + settings.deploy_timeout_ms;
    if let Some(entry) = s.mm.results.get_mut(&launch.player) {
        entry.launch_deadline = Some(deadline);
    }
    let allocation = crate::crypto::random_uuid();
    let seat = Seat {
        player_id: launch.player.clone(),
        candidate_id: launch.candidate_id.clone(),
    };
    s.mm.roster.begin_allocation(&allocation, &[seat], deadline, now);
    let players = [launch.player.clone()];
    let asked = is_mode(launch.game_mode.as_ref(), &["CITY"]).then_some(&players[..]);
    let start = launch_begin(
        s,
        launch.game_mode.as_ref(),
        launch.game_args.as_ref(),
        launch.hunt_id.as_ref(),
        asked,
        launch.region.as_str(),
        now,
    );
    TryStart::Asking {
        allocation,
        deadline,
        active: players.to_vec(),
        launch: start,
    }
}

async fn direct_try_finish(app: &AppState, launch: &DirectLaunch, start: TryStart) -> (bool, Option<i64>) {
    let TryStart::Asking {
        allocation,
        deadline,
        launch: request,
        ..
    } = start
    else {
        return (true, None);
    };
    let game = launch_finish(app, request).await;
    let now = app.party_clock.now();
    let mut guard = app.parties.lock();
    let s = &mut *guard;
    if game.succeeded {
        s.mm.roster
            .assign_allocation(&allocation, game.session_id.as_deref(), now);
    }
    if game.capacity {
        s.mm.roster.release_allocation(&allocation);
    }
    if !s.mm.entry_is(&launch.player, launch.serial) || s.mm.results.get(&launch.player).is_some_and(|e| e.failed) {
        return (true, None);
    }
    let wait_over = s.mm.waits.get(&launch.candidate_id).is_some_and(|w| w.deadline <= now);
    if now >= deadline || wait_over {
        fail_candidate(s, &launch.candidate_id, "allocation deadline expired");
        return (true, None);
    }
    if game.capacity {
        if let Some(entry) = s.mm.results.get_mut(&launch.player) {
            entry.launch_deadline = None;
        }
        return (false, Some(game.retry_after_ms.unwrap_or(CAPACITY_RETRY_MS)));
    }
    apply_launch_result(s, &launch.player, launch.serial, &game, now);
    (true, None)
}

/// The direct start's first try, and its capacity wait.
async fn direct_launch(app: AppState, launch: DirectLaunch, start: TryStart) {
    let (done, retry_ms) = direct_try_finish(&app, &launch, start).await;
    if done {
        return;
    }
    let mut guard = app.parties.lock();
    let s = &mut *guard;
    let current =
        s.mm.entry_is(&launch.player, launch.serial) && s.mm.results.get(&launch.player).is_some_and(|e| !e.failed);
    if current
        && !park_capacity(
            s,
            &launch.candidate_id,
            Retry::Direct(launch.clone()),
            retry_ms.unwrap_or(CAPACITY_RETRY_MS),
            app.party_clock.now(),
        )
    {
        fail_candidate(s, &launch.candidate_id, "capacity wait unavailable");
    }
}

/// A waiting candidate's retry, started by a status poll; afterwards the wait (if it is still the
/// same one) may be tried again after its delay.
async fn retry_wait(app: AppState, candidate: String, wait: u64, retry: Retry, start: TryStart, retry_ms: i64) {
    let (done, new_delay) = match &retry {
        Retry::Party(launch) => party_try_finish(&app, launch, start).await,
        Retry::Direct(launch) => direct_try_finish(&app, launch, start).await,
    };
    let mut s = app.parties.lock();
    if done {
        s.mm.waits.remove(&candidate);
    }
    if let Some(found) = s.mm.waits.get_mut(&candidate)
        && found.serial == wait
    {
        let delay = new_delay.unwrap_or(retry_ms);
        found.retry_ms = delay;
        found.next = app.party_clock.now() + delay;
        found.busy = false;
    }
}

fn retry_begin(s: &mut party::State, settings: &Settings, retry: &Retry, now: i64) -> TryStart {
    match retry {
        Retry::Party(launch) => party_try_begin(s, settings, launch, now),
        Retry::Direct(launch) => direct_try_begin(s, settings, launch, now),
    }
}

// ---- The status poll ----

/// What the first step of a status check left to do.
enum Next {
    Done(Option<Entry>),
    Usable(Entry),
    Retry {
        candidate: String,
        wait: u64,
        retry: Retry,
        start: TryStart,
        retry_ms: i64,
    },
    Pop(PopTicket),
}

/// `CheckAndUpdateQueueStatus`: the player's entry after expiring, retrying or popping what is
/// due.
pub async fn check_and_update(app: &AppState, player: &str) -> Option<Entry> {
    let next = {
        let mut guard = app.parties.lock();
        status_step(&mut guard, &app.matchmaking.settings, player, app.party_clock.now())
    };
    match next {
        Next::Done(entry) => return entry,
        Next::Usable(entry) => {
            ready_allocation_usable(app, player, &entry).await;
        }
        Next::Retry {
            candidate,
            wait,
            retry,
            start,
            retry_ms,
        } => {
            wait_briefly(
                retry_wait(app.clone(), candidate, wait, retry, start, retry_ms),
                WAIT_BRIEFLY_MS,
            )
            .await;
        }
        Next::Pop(ticket) => {
            wait_briefly(pop_queue_finish(app.clone(), ticket), WAIT_BRIEFLY_MS).await;
        }
    }
    app.parties.lock().mm.results.get(player).cloned()
}

fn status_step(s: &mut party::State, settings: &Settings, player: &str, now: i64) -> Next {
    let Some(entry) = s.mm.results.get(player).cloned() else {
        return Next::Done(None);
    };
    let candidate = entry.candidate_id.clone();
    s.mm.roster.touch_candidate(player, &candidate, now);
    if !entry.ready && !entry.failed && entry.launch_deadline.is_some_and(|at| now >= at) {
        fail_candidate(s, &candidate, "allocation request timed out");
    }
    let entry = s.mm.results.get(player).cloned().unwrap_or(entry);
    if entry.failed {
        return Next::Done(Some(entry));
    }
    if entry.ready {
        return Next::Usable(entry);
    }
    let queue_candidate =
        s.mm.player_queue
            .get(player)
            .and_then(|id| s.mm.queues.get(id))
            .and_then(|q| q.candidate_ids.first())
            .map(|(_, c)| c.clone());
    s.mm.turns.touch(queue_candidate.as_ref().unwrap_or(&candidate), now);
    if let Some(wait) = s.mm.waits.get(&candidate).cloned() {
        if now >= wait.deadline {
            fail_candidate(s, &candidate, "capacity wait expired");
        } else {
            let deadline = now + CAPACITY_WAIT_MS;
            if let Some(found) = s.mm.waits.get_mut(&candidate) {
                found.deadline = deadline;
            }
            if entry.party_candidate
                && let Some(party) = s.party_id_of(player)
                && let Some(c) = s.party_mut(&party).and_then(party::Party::candidate_mut)
                && c.candidate_id == candidate
            {
                c.capacity_deadline = Some(deadline);
            }
            if !wait.busy && now >= wait.next {
                if let Some(found) = s.mm.waits.get_mut(&candidate) {
                    found.busy = true;
                    found.next = now + CAPACITY_RETRY_MS;
                }
                let start = retry_begin(s, settings, &wait.retry, now);
                return Next::Retry {
                    candidate,
                    wait: wait.serial,
                    retry: wait.retry,
                    start,
                    retry_ms: wait.retry_ms,
                };
            }
        }
        return Next::Done(s.mm.results.get(player).cloned());
    }
    // Party candidates have no queue: the leader's join started their server already
    if !entry.party_candidate && entry.direct_key.is_none() {
        let Some(id) =
            s.mm.player_queue
                .get(player)
                .copied()
                .filter(|id| s.mm.queues.contains_key(id))
        else {
            tracing::warn!(
                "mm: {player} waits for {} but no queue holds them; answering FAILED",
                entry.hunt_text()
            );
            fail_candidate(s, &candidate, "queue no longer exists");
            return Next::Done(s.mm.results.get(player).cloned());
        };
        let Some(queue) = s.mm.queues.get_mut(&id) else {
            return Next::Done(Some(entry));
        };
        if queue.capacity_deadline.is_some_and(|at| now >= at) {
            let candidates: Vec<String> = queue.candidate_ids.iter().map(|(_, c)| c.clone()).collect();
            for candidate in candidates {
                fail_candidate(s, &candidate, "capacity wait expired");
            }
            return Next::Done(s.mm.results.get(player).cloned());
        }
        if queue.capacity_deadline.is_some() {
            queue.capacity_deadline = Some(now + CAPACITY_WAIT_MS);
        }
        if now - queue.first_player_added_time >= QUEUE_WAIT_MS || queue.retry_after.is_some_and(|at| now >= at) {
            let hunt = entry.hunt_text();
            if let Some(ticket) = pop_queue_begin(s, settings, &hunt, id, now) {
                return Next::Pop(ticket);
            }
        }
    }
    Next::Done(s.mm.results.get(player).cloned())
}

/// What the status poll answers (`CandidateStatusDecision`).
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Unknown,
    Failed(Entry),
    Matching { entry: Entry, parked: bool },
    Travel(Entry),
}

/// `DecideCandidateStatus`: what to answer, with the bookkeeping of a player being sent.
pub async fn decide_status(app: &AppState, player: &str) -> Decision {
    let Some(entry) = check_and_update(app, player).await else {
        return Decision::Unknown;
    };
    let now = app.party_clock.now();
    let mut guard = app.parties.lock();
    let s = &mut *guard;
    // A player polling for their party's candidate is still there
    if entry.party_candidate {
        s.touch(player, now);
    }
    if entry.failed {
        return Decision::Failed(entry);
    }
    if !entry.ready {
        return Decision::Matching { entry, parked: false };
    }
    let mut entry =
        s.mm.results
            .get(player)
            .filter(|e| e.serial == entry.serial)
            .cloned()
            .unwrap_or(entry);
    if let Some(parked) = entry.parked_at {
        if now - parked < REJOIN_PARK_MS {
            return Decision::Matching { entry, parked: true };
        }
        entry.parked_at = None;
        tracing::info!(
            "mm: {player} still asking {} s after joining candidate {} again: sending them to {}:{} again",
            REJOIN_PARK_MS / 1000,
            entry.candidate_id,
            entry.host,
            entry.port
        );
    }
    let first = *entry.first_sent_at.get_or_insert(now);
    if let Some(stored) = s.mm.results.get_mut(player).filter(|e| e.serial == entry.serial) {
        *stored = entry.clone();
    }
    s.mm.roster.served(player, &entry.candidate_id, now);
    s.mm.last_sent.insert(
        player.to_owned(),
        LastSent {
            host: entry.host.clone(),
            port: entry.port,
            session_id: entry.session_id.clone(),
        },
    );
    if entry.party_candidate {
        s.mark_candidate_served(player, &entry.candidate_id, first, now);
    }
    Decision::Travel(entry)
}

// ---- Cancels ----

/// `CancelMatchmaking`, DELETE /candidate (only with `MATCHMAKING_CANCEL=1`): takes the player out
/// of an unresolved queue and forgets their result. A party leader's cancel clears the party's
/// candidate and the members' entries that weren't sent yet; another member's cancel only takes
/// that member out of it.
pub fn cancel(s: &mut party::State, player: &str, now: i64) -> Option<Entry> {
    let entry = s.mm.results.get(player).cloned()?;
    let candidate = entry.candidate_id.clone();
    if entry.party_candidate {
        let party = s.party_id_of(player);
        let leads = party
            .as_ref()
            .and_then(|id| s.party(id))
            .is_some_and(|p| p.leader_id == player && p.candidate().is_some_and(|c| c.candidate_id == candidate));
        if let (true, Some(party)) = (leads, &party) {
            let members = s
                .party(party)
                .and_then(party::Party::candidate)
                .map(|c| c.member_ids.clone())
                .unwrap_or_default();
            let mut dropped = 0;
            for member in members {
                let drop =
                    s.mm.results
                        .get(&member)
                        .is_some_and(|e| member != player && e.candidate_id == candidate && e.first_sent_at.is_none());
                if drop {
                    s.mm.roster.remove_player(&member, &candidate, Removal::Cancelled);
                    s.mm.results.remove(&member);
                    dropped += 1;
                }
            }
            s.clear_candidate_if(party, &candidate, &format!("cancelled by the leader {player}"));
            s.drop_last_candidate_if(party, &candidate);
            tracing::info!(
                "mm: cancel by={player} party P={party} candidate {candidate}: {dropped} member entr{} not yet sent dropped",
                if dropped == 1 { "y" } else { "ies" }
            );
        } else {
            s.remove_from_candidate(player, &candidate, now);
            s.leave_last_candidate(player, &candidate);
            tracing::info!("mm: cancel by={player} candidate {candidate}: left the party's candidate");
        }
        s.mm.roster.remove_player(player, &candidate, Removal::Cancelled);
        if !s
            .mm
            .results
            .iter()
            .any(|(id, e)| id != player && e.candidate_id == candidate)
        {
            s.mm.waits.remove(&candidate);
        }
        s.mm.results.remove(player);
        return Some(entry);
    }
    if let Some(&id) = s.mm.player_queue.get(player)
        && let Some(queue) = s.mm.queues.get_mut(&id)
        && !queue.resolved
    {
        queue.players.retain(|p| p != player);
        queue.forget(player);
        let key = queue.key();
        if queue.players.is_empty() && s.mm.queue_by_key.get(&key) == Some(&id) {
            s.mm.queue_by_key.remove(&key);
        }
    }
    s.mm.player_queue.remove(player);
    s.mm.roster.remove_player(player, &candidate, Removal::Cancelled);
    s.mm.waits.remove(&candidate);
    s.mm.results.remove(player);
    s.mm.forget_unused_queues();
    Some(entry)
}

/// `LeaveCandidate`, DELETE /candidate/leave: the caller alone leaves their candidate.
pub fn leave(s: &mut party::State, player: &str, now: i64) -> Option<Entry> {
    let entry = match s.mm.results.get(player) {
        Some(entry) if entry.party_candidate => entry.clone(),
        _ => return cancel(s, player, now),
    };
    s.remove_from_candidate(player, &entry.candidate_id, now);
    s.leave_last_candidate(player, &entry.candidate_id);
    s.mm.roster
        .remove_player(player, &entry.candidate_id, Removal::Cancelled);
    s.mm.results.remove(player);
    tracing::info!("mm: leave by={player} candidate {}", entry.candidate_id);
    Some(entry)
}

/// `reconcileExpected` for POST /candidate/player/alive.
pub fn reconcile_expected(
    s: &mut party::State,
    players: &[String],
    context: &roster::Context,
    now: i64,
) -> Vec<String> {
    s.mm.roster.reconcile_expected(players, context, now)
}

// ---- Joins ----

/// `/^[A-Za-z0-9_+]{1,128}$/`.
fn is_hunt_id(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'+')
}

fn is_word(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `/^\/Game(\/[A-Za-z0-9_]+)+(\.[A-Za-z0-9_]+)?$/`, the extension required with `asset`.
fn is_game_path(text: &str, asset: bool) -> bool {
    let Some(rest) = text.strip_prefix("/Game/") else {
        return false;
    };
    let (path, extension) = match rest.rsplit_once('.') {
        Some((path, extension)) => (path, Some(extension)),
        None => (rest, None),
    };
    if asset && extension.is_none() {
        return false;
    }
    extension.is_none_or(is_word) && path.split('/').all(is_word)
}

/// `CheckMatchmakingInput`: what a client may send to /candidate/join. Its input ends up on a
/// game server's command line, so only the shapes the client really sends get through; the
/// deploy server checks the same again.
pub fn check_input(game_mode: Option<&Value>, game_args: Option<&Value>, hunt: Option<&Value>) -> Option<&'static str> {
    if !js::is_nullish(hunt) && !matches!(hunt, Some(Value::String(h)) if h.is_empty() || is_hunt_id(h)) {
        return Some("the hunt id is not a hunt id");
    }
    if !js::is_nullish(game_args) && !matches!(game_args, Some(Value::String(_))) {
        return Some("the game args are not a string");
    }
    // Used only for ISLAND: map ? option ? MonsterClass=<behemoth> ? ...
    if game_mode.and_then(Value::as_str) == Some("ISLAND")
        && let Some(Value::String(args)) = game_args
        && !args.trim().is_empty()
    {
        let parts: Vec<&str> = args.split('?').collect();
        let behemoth = parts.get(2).and_then(|p| p.split('=').nth(1));
        let characters = (1..=2048).contains(&args.chars().count())
            && args
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_/.?=+-".contains(&b));
        if !characters
            || !is_game_path(parts[0], false)
            || behemoth.is_none_or(|b| !b.is_empty() && !is_game_path(b, true))
        {
            return Some("the game args are not a map and behemoth the deploy server can start");
        }
    }
    None
}

/// `HandlePlayerMatchmaking`, POST /candidate/join: whether the join was taken.
pub async fn join(
    app: &AppState,
    game_mode: Option<Value>,
    game_args: Option<Value>,
    hunt: Option<Value>,
    player: &str,
    private: bool,
) -> Result<bool, AppError> {
    if let Some(bad) = check_input(game_mode.as_ref(), game_args.as_ref(), hunt.as_ref()) {
        tracing::warn!("Refusing matchmaking for {player}: {bad}");
        return Ok(false);
    }
    match app.matchmaking.settings.mode {
        Mode::Disabled => {
            tracing::warn!("Matchmaking is disabled, refusing MM!");
            return Ok(false);
        }
        Mode::Unsupported => {
            tracing::error!("Unsupported MATCHMAKING_MODE!");
            return Ok(false);
        }
        Mode::DeployServer => {}
    }
    match party_join(app, game_mode.as_ref(), game_args.as_ref(), hunt.as_ref(), player).await? {
        PartyJoin::Handled(taken) => return Ok(taken),
        PartyJoin::Solo => {}
    }
    {
        let mut s = app.parties.lock();
        // A player going somewhere on their own no longer counts in their party's candidate
        if let Some(entry) = s.mm.results.get(player).filter(|e| e.party_candidate).cloned() {
            s.remove_from_candidate(player, &entry.candidate_id, app.party_clock.now());
        }
    }
    match hunt.as_ref().and_then(Value::as_str) {
        Some(h) if !h.trim().is_empty() && hunt_requires_matchmaking(h) => queue_player(app, h, player, private).await,
        _ => direct_join(app, game_mode, game_args, hunt, player).await,
    }
}

/// Ramsgate, the Dojo or the tutorial: one server for the player (or a city seat), no queue.
async fn direct_join(
    app: &AppState,
    game_mode: Option<Value>,
    game_args: Option<Value>,
    hunt: Option<Value>,
    player: &str,
) -> Result<bool, AppError> {
    let shared = app.clone();
    let id = player.to_owned();
    let mode = game_mode.clone();
    // Instead of a hunt the player was still queued for; party members follow their leader's
    // region for shared worlds, keeping their own saved preference for when they leave
    let (region, previous) = app
        .store
        .read(move |conn| {
            let mut s = shared.parties.lock();
            s.mm.leave_waiting_queues(&id, "their new join replaces it");
            let world = if is_mode(mode.as_ref(), &["CITY", "SHARED"]) {
                s.party_id_of(&id)
                    .and_then(|p| s.party(&p).map(|p| p.leader_id.clone()))
            } else {
                None
            };
            let region = player_region(conn, world.as_deref().unwrap_or(&id))?;
            Ok::<_, AppError>((region, s.mm.results.get(&id).cloned()))
        })
        .await?;
    let key = service_kit::jsjson::stringify(&json!([
        game_mode.clone().unwrap_or(Value::Null),
        match &game_args {
            None | Some(Value::Null) => json!(""),
            Some(args) => args.clone(),
        },
        match &hunt {
            None | Some(Value::Null) => json!(""),
            Some(hunt) => hunt.clone(),
        },
        region.as_str()
    ]));
    if let Some(previous) = previous.filter(|p| p.direct_key.as_deref() == Some(key.as_str()) && !p.failed) {
        let ready = {
            let mut s = app.parties.lock();
            s.mm.roster
                .touch_candidate(player, &previous.candidate_id, app.party_clock.now());
            if !previous.ready
                && (previous.launch_deadline.is_some() || s.mm.waits.contains_key(&previous.candidate_id))
            {
                return Ok(true);
            }
            previous.ready
        };
        if ready {
            let usable = ready_allocation_usable(app, player, &previous).await;
            if !app.parties.lock().mm.entry_is(player, previous.serial) || usable {
                return Ok(true);
            }
        }
    }
    let (launch, start) = {
        let mut guard = app.parties.lock();
        let s = &mut *guard;
        // A concurrent join with the same key may have started its launch while this one read the
        // region: Node ran up to here without a wait, so the second join found the first one's entry
        if let Some(current) = s.mm.results.get(player)
            && current.direct_key.as_deref() == Some(key.as_str())
            && !current.failed
            && !current.ready
            && (current.launch_deadline.is_some() || s.mm.waits.contains_key(&current.candidate_id))
        {
            return Ok(true);
        }
        let now = app.party_clock.now();
        let candidate = crate::crypto::random_uuid();
        let mut entry = Entry::new(candidate.clone(), hunt.clone());
        entry.region = Some(region);
        entry.direct_key = Some(key);
        entry.queued_at = Some(now);
        // Published before any network wait: a cancel or a new hunt can replace it at once, and
        // this request may only finish its own entry
        let serial = s.mm.store_entry(player, entry, now);
        let launch = DirectLaunch {
            player: player.to_owned(),
            serial,
            candidate_id: candidate,
            game_mode,
            game_args,
            hunt_id: hunt,
            region,
        };
        let start = direct_try_begin(s, &app.matchmaking.settings, &launch, now);
        (launch, start)
    };
    wait_briefly(direct_launch(app.clone(), launch, start), WAIT_BRIEFLY_MS).await;
    Ok(true)
}

/// `QueuePlayer`: the player joins (or stays in) the hunt's queue for their region.
async fn queue_player(app: &AppState, hunt: &str, player: &str, private: bool) -> Result<bool, AppError> {
    let shared = app.clone();
    let id = player.to_owned();
    let region = app
        .store
        .read(move |conn| player_region(conn, &id).map_err(AppError::from))
        .await?;
    let current = {
        let mut guard = app.parties.lock();
        let s = &mut *guard;
        let now = app.party_clock.now();
        let current = s.mm.results.get(player).cloned();
        if let Some(entry) = &current {
            s.mm.roster.touch_candidate(player, &entry.candidate_id, now);
        }
        match current {
            Some(entry)
                if !entry.party_candidate
                    && !entry.failed
                    && entry.hunt_is(hunt)
                    && entry.region == Some(region)
                    && entry.private == private =>
            {
                let queue = s.mm.player_queue.get(player).and_then(|id| s.mm.queues.get(id));
                if !entry.ready
                    && let Some(queue) = queue
                    && queue.players.iter().any(|p| p == player)
                    && (queue.resolved
                        || queue.retry_after.is_some()
                        || now - entry.queued_at.unwrap_or(0) <= SOLO_JOIN_DEDUPE_MS)
                {
                    return Ok(true);
                }
                entry.ready.then_some(entry)
            }
            _ => None,
        }
    };
    if let Some(current) = current {
        let usable = ready_allocation_usable(app, player, &current).await;
        let mut s = app.parties.lock();
        if !s.mm.entry_is(player, current.serial) {
            return Ok(true);
        }
        if usable {
            let now = app.party_clock.now();
            if let Some(entry) = s.mm.results.get_mut(player)
                && entry.first_sent_at.is_some()
            {
                entry.parked_at.get_or_insert(now);
            }
            return Ok(true);
        }
    }
    let tickets = {
        let mut guard = app.parties.lock();
        queue_player_rest(
            &mut guard,
            &app.matchmaking.settings,
            hunt,
            player,
            private,
            region,
            app.party_clock.now(),
        )
    };
    spawn_pops(&shared, tickets);
    Ok(true)
}

fn queue_player_rest(
    s: &mut party::State,
    settings: &Settings,
    hunt: &str,
    player: &str,
    private: bool,
    region: Region,
    now: i64,
) -> Vec<PopTicket> {
    let key = queue_key(hunt, region);
    let mut tickets = Vec::new();
    let existing = if private {
        None
    } else {
        s.mm.queue_by_key.get(&key).copied()
    };
    if let Some(existing) = existing {
        prune_waiting_queue(s, existing, now);
        if s.mm
            .queues
            .get(&existing)
            .is_some_and(|q| q.players.len() >= QUEUE_FULL_PLAYERS)
        {
            tickets.extend(pop_queue_begin(s, settings, hunt, existing, now));
        }
    }
    s.mm.leave_waiting_queues(player, "their new join replaces it");
    let mut queue = if private {
        None
    } else {
        s.mm.queue_by_key.get(&key).copied()
    };
    if let Some(full) = queue
        && s.mm
            .queues
            .get(&full)
            .is_some_and(|q| q.players.len() >= QUEUE_FULL_PLAYERS)
    {
        tickets.extend(pop_queue_begin(s, settings, hunt, full, now));
        queue = None;
    }
    let candidate = crate::crypto::random_uuid();
    let mut entry = Entry::new(candidate.clone(), Some(json!(hunt)));
    entry.private = private;
    entry.region = Some(region);
    entry.queued_at = Some(now);
    s.mm.store_entry(player, entry, now);
    let id = match queue {
        Some(id) => id,
        None => {
            let id = s.mm.serial();
            s.mm.queues.insert(
                id,
                Queue {
                    private,
                    region,
                    hunt_id: hunt.to_owned(),
                    retry_after: None,
                    capacity_deadline: None,
                    players: Vec::new(),
                    candidate_ids: Vec::new(),
                    first_player_added_time: now,
                    resolved: false,
                },
            );
            if !private {
                s.mm.queue_by_key.insert(key, id);
            }
            id
        }
    };
    let size = match s.mm.queues.get_mut(&id) {
        Some(queue) => {
            queue.players.push(player.to_owned());
            queue.set_candidate(player, &candidate);
            queue.players.len()
        }
        None => 0,
    };
    s.mm.player_queue.insert(player.to_owned(), id);
    if private || size >= QUEUE_FULL_PLAYERS {
        tickets.extend(pop_queue_begin(s, settings, hunt, id, now));
    }
    s.mm.forget_unused_queues();
    tickets
}

// ---- Parties: the whole party on one server ----
//
// Only the leader starts the party's hunts. Its join makes one candidate for all members and asks
// the deploy server once, with every member expected. The members never join: they see the
// candidate in their POST /party poll and then poll GET /candidate/status. Going back to Ramsgate
// (CITY) or the Dojo (SHARED) takes the members who are on the leader's current server along. A
// party of one, and the tutorial, use the solo code above.

enum PartyJoin {
    Solo,
    Handled(bool),
}

/// `FindRejoinCandidate`: the candidate the join is for, if the player is already part of it.
#[allow(clippy::too_many_arguments)] // the join's whole context, as Node passed it
fn find_rejoin(
    conn: &mut Conn,
    s: &mut party::State,
    party_id: &str,
    player: &str,
    mode: &str,
    hunt: &str,
    leader: bool,
    now: i64,
) -> QueryResult<Option<Candidate>> {
    let Some(party) = s.party(party_id) else {
        return Ok(None);
    };
    let both = [party.candidate().cloned(), party.last_candidate().cloned()];
    for candidate in both.into_iter().flatten() {
        if candidate.state != CandidateState::InProgress {
            continue;
        }
        let failed_entry =
            s.mm.results
                .get(player)
                .is_some_and(|e| e.candidate_id == candidate.candidate_id && e.failed);
        if now - candidate.allocated_at.unwrap_or(candidate.created_at) >= LAST_CANDIDATE_REJOIN_MS || failed_entry {
            fail_candidate(s, &candidate.candidate_id, "party allocation reuse expired");
            s.clear_candidate_if(party_id, &candidate.candidate_id, "party allocation reuse expired");
            s.drop_last_candidate_if(party_id, &candidate.candidate_id);
        }
    }
    let Some(party) = s.party(party_id) else {
        return Ok(None);
    };
    let leader_region = player_region(conn, &party.leader_id)?;
    let matches = |c: &Candidate| {
        c.region.unwrap_or(Region::Main) == leader_region
            && c.game_mode == mode
            && c.hunt_id == hunt
            && c.member_ids.iter().any(|m| m == player)
    };
    if let Some(current) = party.candidate().filter(|c| matches(c)) {
        return Ok(Some(current.clone()));
    }
    Ok(party
        .last_candidate()
        .filter(|last| {
            !leader
                && matches(last)
                && last.state == CandidateState::InProgress
                && now - last.allocated_at.unwrap_or(last.created_at) <= LAST_CANDIDATE_REJOIN_MS
        })
        .cloned())
}

/// `RejoinPartyCandidate`: the join is for the candidate the player is already part of, so
/// nothing new is started.
fn rejoin(s: &mut party::State, party_id: &str, candidate: &Candidate, player: &str, now: i64) -> bool {
    let place = format!(
        "candidate {} of P={party_id} ({})",
        candidate.candidate_id, candidate.hunt_id
    );
    if let Some(entry) = s.mm.results.get(player).cloned()
        && entry.party_candidate
        && entry.candidate_id == candidate.candidate_id
        && !entry.failed
    {
        s.mm.roster.touch_candidate(player, &candidate.candidate_id, now);
        if !entry.ready {
            tracing::info!("mm: {player} joined again while {place} is still starting; nothing new started");
        } else if candidate.served.contains_key(player) || entry.parked_at.is_some() {
            if let Some(stored) = s.mm.results.get_mut(player) {
                stored.parked_at.get_or_insert(now);
            }
            tracing::info!(
                "mm: {player} joined again after being sent to {}:{} for {place}; no second server, answering MATCHING",
                entry.host,
                entry.port
            );
        } else {
            tracing::info!("mm: {player} joined again before being sent to {place}; nothing new started");
        }
        return true;
    }
    let ready = candidate.state == CandidateState::InProgress && candidate.host.is_some() && candidate.port.is_some();
    let served = candidate.served.get(player).copied();
    if ready
        && !s.mm.roster.restore_assigned(
            player,
            &candidate.candidate_id,
            &candidate.hunt_id,
            candidate.session_id.as_deref(),
            now,
        )
    {
        tracing::warn!("mm: {player} cannot restore their original roster membership for {place}");
        return false;
    }
    s.mm.leave_waiting_queues(player, "their party's candidate takes them");
    let mut entry = Entry::new(candidate.candidate_id.clone(), Some(json!(candidate.hunt_id)));
    entry.ready = ready;
    if ready {
        entry.host = candidate.host.clone().unwrap_or_default();
        entry.port = i64::from(candidate.port.unwrap_or(0));
    } else {
        entry.launch_deadline = candidate.launch_deadline;
    }
    entry.session_id = candidate.session_id.clone();
    entry.allocated_at = candidate.allocated_at;
    entry.first_sent_at = served;
    entry.party_candidate = true;
    entry.party_member_ids = Some(candidate.member_ids.clone());
    entry.parked_at = (ready && served.is_some()).then_some(now);
    s.mm.store_entry(player, entry, now);
    tracing::info!(
        "mm: {player} joined {place} again; {}; no second server",
        if ready {
            if served.is_some() {
                "already sent there once, answering MATCHING"
            } else {
                "its server is up"
            }
        } else {
            "still starting"
        }
    );
    true
}

/// `JoinPartyCandidateById`, POST /candidate/join/:candidateId: a party member following the
/// leader's candidate. Anyone else gets `None` (the 404 the route always gave).
pub async fn join_candidate(app: &AppState, player: &str, candidate_id: &str) -> Result<Option<Candidate>, AppError> {
    let shared = app.clone();
    let (player, wanted) = (player.to_owned(), candidate_id.to_owned());
    app.store
        .read(move |conn| {
            let mut guard = shared.parties.lock();
            let s = &mut *guard;
            let now = shared.party_clock.now();
            let Some(party_id) = s.party_for_matchmaking(conn, &player, now)? else {
                return Ok(None);
            };
            let Some(party) = s.party(&party_id) else {
                return Ok(None);
            };
            let candidate = match (party.candidate(), party.last_candidate()) {
                (Some(current), _) if current.candidate_id == wanted => Some(current.clone()),
                (_, Some(last))
                    if last.candidate_id == wanted
                        && last.state == CandidateState::InProgress
                        && now - last.allocated_at.unwrap_or(last.created_at) <= LAST_CANDIDATE_REJOIN_MS =>
                {
                    Some(last.clone())
                }
                _ => None,
            };
            let leader_region = player_region(conn, &party.leader_id)?;
            let Some(candidate) = candidate else {
                return Ok(None);
            };
            let failed =
                s.mm.results
                    .get(&player)
                    .is_some_and(|e| e.candidate_id == candidate.candidate_id && e.failed);
            if candidate.region.unwrap_or(Region::Main) != leader_region
                || !candidate.member_ids.contains(&player)
                || failed
                || (candidate.state == CandidateState::InProgress
                    && now - candidate.allocated_at.unwrap_or(candidate.created_at) >= LAST_CANDIDATE_REJOIN_MS)
            {
                return Ok(None);
            }
            if !rejoin(s, &party_id, &candidate, &player, now) {
                return Ok(None);
            }
            Ok::<_, AppError>(Some(candidate))
        })
        .await
}

/// `HandlePartyMatchmaking`: Solo when it isn't a party matter.
async fn party_join(
    app: &AppState,
    game_mode: Option<&Value>,
    game_args: Option<&Value>,
    hunt: Option<&Value>,
    player: &str,
) -> Result<PartyJoin, AppError> {
    let mode = game_mode.and_then(Value::as_str).unwrap_or("").to_owned();
    let hunt = hunt.and_then(Value::as_str).unwrap_or("").to_owned();
    let tutorial = mode == "ISLAND" && game_args.and_then(Value::as_str).is_some_and(|a| !a.trim().is_empty());
    let shared = app.clone();
    let (id, wanted_mode, wanted_hunt) = (player.to_owned(), mode.clone(), hunt.clone());
    // The party, and the candidate a rejoin is for
    let found = app
        .store
        .read(move |conn| {
            let mut guard = shared.parties.lock();
            let s = &mut *guard;
            let now = shared.party_clock.now();
            let Some(party_id) = s.party_for_matchmaking(conn, &id, now)? else {
                return Ok(None);
            };
            let Some(party) = s.party(&party_id).filter(|p| p.members.len() >= 2) else {
                return Ok(None);
            };
            let leader = party.leader_id == id;
            if tutorial {
                tracing::info!(
                    "mm: {id} (party P={party_id}) starts a game from its own game args (the tutorial): alone"
                );
                return Ok(None);
            }
            let rejoin = find_rejoin(conn, s, &party_id, &id, &wanted_mode, &wanted_hunt, leader, now)?;
            let ready_entry = rejoin.as_ref().and_then(|c| {
                s.mm.results
                    .get(&id)
                    .filter(|e| e.candidate_id == c.candidate_id && e.ready)
                    .cloned()
            });
            Ok::<_, AppError>(Some((party_id, leader, rejoin, ready_entry)))
        })
        .await?;
    let Some((party_id, leader, mut rejoin_candidate, ready_entry)) = found else {
        return Ok(PartyJoin::Solo);
    };
    if let Some(entry) = ready_entry {
        let usable = ready_allocation_usable(app, player, &entry).await;
        if !app.parties.lock().mm.entry_is(player, entry.serial) {
            return Ok(PartyJoin::Handled(true));
        }
        if !usable {
            rejoin_candidate = None;
        }
    }
    let shared = app.clone();
    let id = player.to_owned();
    let started = app
        .store
        .read(move |conn| {
            let mut guard = shared.parties.lock();
            let s = &mut *guard;
            let now = shared.party_clock.now();
            if let Some(candidate) = rejoin_candidate {
                return Ok(Err(rejoin(s, &party_id, &candidate, &id, now)));
            }
            if !leader {
                if mode == "CITY" || mode == "SHARED" {
                    tracing::info!(
                        "mm: {id} is not the leader of P={party_id}; goes to {} alone",
                        if hunt.is_empty() { &mode } else { &hunt }
                    );
                    return Ok(Ok(None));
                }
                let leader_id = s.party(&party_id).map(|p| p.leader_id.clone()).unwrap_or_default();
                tracing::warn!(
                    "mm: refusing {mode} {hunt} for {id}: only the leader of P={party_id} ({leader_id}) starts the party's hunts"
                );
                return Ok(Err(false));
            }
            let party_members = s.party(&party_id).map(|p| p.members.clone()).unwrap_or_default();
            let members = match mode.as_str() {
                "ISLAND" => {
                    if hunt.trim().is_empty() || !hunt_requires_matchmaking(&hunt) {
                        return Ok(Ok(None));
                    }
                    party_members
                }
                "CITY" | "SHARED" => {
                    let leader_was = s.mm.last_sent.get(&id).cloned();
                    let members: Vec<String> = party_members
                        .into_iter()
                        .filter(|member| {
                            *member == id
                                || match (&leader_was, s.mm.last_sent.get(member)) {
                                    (Some(was), Some(member_was)) => {
                                        member_was.host == was.host
                                            && member_was.port == was.port
                                            && ((was.session_id.is_none() && member_was.session_id.is_none())
                                                || member_was.session_id == was.session_id)
                                    }
                                    _ => false,
                                }
                        })
                        .collect();
                    if members.len() < 2 {
                        tracing::info!(
                            "mm: party leader {id} goes to {} alone: no member of P={party_id} is on the leader's server",
                            if hunt.is_empty() { &mode } else { &hunt }
                        );
                        return Ok(Ok(None));
                    }
                    members
                }
                _ => return Ok(Ok(None)),
            };
            let region = player_region(conn, &id)?;
            let candidate = crate::crypto::random_uuid();
            for member in &members {
                s.mm.leave_waiting_queues(member, "their party's hunt takes them");
                let mut entry = Entry::new(candidate.clone(), Some(json!(hunt)));
                entry.party_candidate = true;
                entry.party_member_ids = Some(members.clone());
                s.mm.store_entry(member, entry, now);
            }
            s.set_candidate(
                &party_id,
                Candidate {
                    region: Some(region),
                    candidate_id: candidate.clone(),
                    state: CandidateState::Matching,
                    game_mode: mode.clone(),
                    hunt_id: hunt.clone(),
                    member_ids: members.clone(),
                    served: HashMap::new(),
                    host: None,
                    port: None,
                    session_id: None,
                    allocated_at: None,
                    launch_deadline: None,
                    capacity_deadline: None,
                    created_at: now,
                    last_served_at: None,
                },
            );
            tracing::info!(
                "mm: party P={party_id} candidate {candidate} mode={mode} hunt={hunt} members={} by={id}",
                members.join(",")
            );
            let launch = PartyLaunch {
                party_id: party_id.clone(),
                candidate_id: candidate,
                members,
                game_mode: mode.clone(),
                hunt_id: hunt.clone(),
                region,
            };
            let start = party_try_begin(s, &shared.matchmaking.settings, &launch, now);
            Ok::<_, AppError>(Ok(Some((launch, start))))
        })
        .await?;
    match started {
        Err(taken) => Ok(PartyJoin::Handled(taken)),
        Ok(None) => Ok(PartyJoin::Solo),
        Ok(Some((launch, start))) => {
            wait_briefly(party_launch(app.clone(), launch, start), PARTY_JOIN_WAIT_MS).await;
            Ok(PartyJoin::Handled(true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_input_is_checked_like_node() {
        let s = |v: &str| -> Option<&'static Value> { Some(Box::leak(Box::new(json!(v)))) };
        assert_eq!(
            check_input(s("ISLAND"), None, s("CR19_PlayerHunt_Patrol_Heroic+_Gem")),
            None
        );
        assert_eq!(
            check_input(s("ISLAND"), None, Some(&json!(5))),
            Some("the hunt id is not a hunt id")
        );
        assert_eq!(
            check_input(s("ISLAND"), None, s("a b")),
            Some("the hunt id is not a hunt id")
        );
        assert_eq!(check_input(s("ISLAND"), None, s("")), None);
        assert_eq!(
            check_input(s("ISLAND"), Some(&json!(1)), None),
            Some("the game args are not a string")
        );
        let tutorial = "/Game/Maps/islands/1705/dia_moss_triforce?x?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C";
        assert_eq!(check_input(s("ISLAND"), s(tutorial), None), None);
        assert_eq!(check_input(s("CITY"), s("anything at all"), None), None);
        let bad = [
            "/Game/Maps/x",
            "/Game/Maps/x?y",
            "/Game/Maps/x?y?MonsterClass=/Game/m/b",
            "/Maps/x?y?MonsterClass=",
            "/Game/Maps/x?y?MonsterClass=/Game/m/b.c d",
        ];
        for args in bad {
            assert!(check_input(s("ISLAND"), s(args), None).is_some(), "{args}");
        }
        assert_eq!(check_input(s("ISLAND"), s("/Game/Maps/x?y?MonsterClass="), None), None);
    }

    #[test]
    fn repeated_players_are_expected_once() {
        let players: Vec<String> = ["V", "V", "O"].iter().map(|p| (*p).to_owned()).collect();
        assert_eq!(distinct_players(&players), ["V", "O"]);
    }
}
