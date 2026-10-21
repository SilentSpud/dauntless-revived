//! How many game servers this machine takes on (`capacity.ts`): the hunt limit, the launch backlog,
//! the drain file and the free-memory reservations. Refusals are [`DeployError::Capacity`], which the
//! matchmaking call answers 503 with Retry-After.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Memory,
    Ports,
    Hunts,
    Cpu,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Ports => "ports",
            Self::Hunts => "hunts",
            Self::Cpu => "cpu",
        }
    }
}

/// What a launch fails with: a capacity refusal, or anything else (a message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployError {
    Capacity(Reason),
    Failed(String),
}

impl DeployError {
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

impl fmt::Display for DeployError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity(reason) => write!(f, "Game-server capacity unavailable: {}", reason.as_str()),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl From<String> for DeployError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// `HuntLimit`: `MAX_LOCAL_HUNTS` (`MAX_HUNTS` on a hunt worker), `auto` for every hunt port, or
/// `None` for no limit.
pub fn hunt_limit(settings: &Settings) -> Result<Option<u64>, String> {
    let name = if settings.is("HUNT_WORKER", "1") {
        "MAX_HUNTS"
    } else {
        "MAX_LOCAL_HUNTS"
    };
    if settings.is(name, "auto") {
        if !settings.is("GAMESERVER_CPU_GUARD", "1") || settings.is("GAMESERVER_MEMORY_GUARD", "0") {
            return Err("Automatic hunt capacity requires CPU and memory guards".into());
        }
        let first = settings.whole("PORT_RANGE_BEGIN", 1)?;
        let last = settings.whole("PORT_RANGE_END", 1)?;
        if last > 65535 || last < first + 2 {
            return Err("Invalid automatic hunt port range".into());
        }
        // Two ports remain reserved for Ramsgate and Training. CPU and RAM admission decide how many
        // of the remaining ports can actually run at any moment.
        return Ok(Some(last - first - 1));
    }
    if settings.get(name).is_none() {
        return Ok(None);
    }
    settings.whole(name, 1).map(Some)
}

/// Launches start one at a time, `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP` apart, and the
/// coordinator gives up on a launch after 120 seconds. Half of that is the budget for the line.
const PENDING_BUDGET_SECONDS: f64 = 60.0;

/// While this file exists no new hunt is admitted; running ones finish normally. Checked on every
/// request, so no restart is needed to toggle it.
pub fn hunts_draining(settings: &Settings) -> bool {
    std::path::Path::new(&settings.non_empty("DRAIN_FILE").unwrap_or_else(|| "drain.flag".into())).exists()
}

pub fn pending_hunt_limit(settings: &Settings) -> Result<u64, String> {
    let wanted = settings.whole("MAX_PENDING_HUNTS", 8)?;
    let pacing = settings
        .number("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP")
        .filter(|n| n.is_finite() && *n != 0.0)
        .unwrap_or(1.0)
        .max(1.0);
    Ok(wanted.min((PENDING_BUDGET_SECONDS / pacing).floor() as u64).max(1))
}

/// Gives back a slot or a reservation once, however often it is called.
#[derive(Clone)]
pub struct Release(Arc<dyn Fn() + Send + Sync>);

impl fmt::Debug for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Release")
    }
}

impl Release {
    pub fn new(release: impl Fn() + Send + Sync + 'static) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        Self(Arc::new(move || {
            if !done.swap(true, Ordering::SeqCst) {
                release();
            }
        }))
    }

    pub fn nothing() -> Self {
        Self(Arc::new(|| {}))
    }

    pub fn release(&self) {
        (self.0)();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct AdmissionStatus {
    pub running: u64,
    pub pending: u64,
    pub limit: Option<u64>,
}

type Count = Arc<dyn Fn() -> u64 + Send + Sync>;
type Limit = Arc<dyn Fn(&Settings) -> Result<Option<u64>, String> + Send + Sync>;

/// Hard hunt slots: running hunts plus launches on their way, against the limit.
#[derive(Clone)]
pub struct HuntAdmission {
    /// Checking and counting a slot is one step
    turn: Arc<Mutex<()>>,
    pending: Arc<AtomicU64>,
    running: Count,
    limit: Limit,
    settings: Settings,
}

impl HuntAdmission {
    pub fn new(settings: Settings, running: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self::with_limit(settings, running, hunt_limit)
    }

    pub fn with_limit(
        settings: Settings,
        running: impl Fn() -> u64 + Send + Sync + 'static,
        limit: impl Fn(&Settings) -> Result<Option<u64>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            turn: Arc::default(),
            pending: Arc::new(AtomicU64::new(0)),
            running: Arc::new(running),
            limit: Arc::new(limit),
            settings,
        }
    }

    pub fn pending(&self) -> u64 {
        self.pending.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> Result<AdmissionStatus, String> {
        Ok(AdmissionStatus {
            running: (self.running)(),
            pending: self.pending(),
            limit: (self.limit)(&self.settings)?,
        })
    }

    /// Keeps paced launches within the caller's request timeout instead of spawning abandoned hunts.
    pub fn reserve(&self) -> Result<Release, DeployError> {
        let _turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        let status = self.status()?;
        if hunts_draining(&self.settings) || status.pending >= pending_hunt_limit(&self.settings)? {
            return Err(DeployError::Capacity(Reason::Hunts));
        }
        if status
            .limit
            .is_some_and(|limit| status.running + status.pending >= limit)
        {
            return Err(DeployError::Capacity(Reason::Hunts));
        }
        self.pending.fetch_add(1, Ordering::SeqCst);
        let pending = self.pending.clone();
        Ok(Release::new(move || {
            pending.fetch_sub(1, Ordering::SeqCst);
        }))
    }
}

type Free = Arc<dyn Fn() -> f64 + Send + Sync>;
type Now = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Reserves estimated startup memory before a spawn, so concurrent requests cannot all claim the
/// same free RAM before Windows has charged the new processes for their allocations.
#[derive(Clone)]
pub struct MemoryAdmission {
    reservations: Arc<Mutex<HashMap<u64, (i64, u64)>>>,
    next: Arc<AtomicU64>,
    free: Free,
    now: Now,
    settings: Settings,
}

const MIB: u64 = 1_048_576;

impl MemoryAdmission {
    pub fn new(
        settings: Settings,
        free: impl Fn() -> f64 + Send + Sync + 'static,
        now: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            reservations: Arc::default(),
            next: Arc::new(AtomicU64::new(0)),
            free: Arc::new(free),
            now: Arc::new(now),
            settings,
        }
    }

    pub fn reserve(&self, temporary_hunt: bool) -> Result<Release, DeployError> {
        let settings = &self.settings;
        if settings.is("GAMESERVER_MEMORY_GUARD", "0") {
            return Ok(Release::nothing());
        }
        let floor = settings.whole("GAMESERVER_MIN_FREE_MB", 3072)? * MIB;
        let standard = settings.whole("GAMESERVER_STARTUP_MB", 1536)?;
        let cost = if temporary_hunt {
            settings.whole("GAMESERVER_HUNT_STARTUP_MB", standard)?
        } else {
            standard
        } * MIB;
        let hold = settings.whole("GAMESERVER_RESERVATION_SECONDS", 60)? as i64 * 1000;
        let now = (self.now)();
        let mut reservations = self.reservations.lock().unwrap_or_else(PoisonError::into_inner);
        reservations.retain(|_, (until, _)| *until > now);
        let available = (self.free)();
        let reserved: u64 = reservations.values().map(|(_, bytes)| bytes).sum();
        if !available.is_finite() || available - (reserved as f64) < (floor + cost) as f64 {
            return Err(DeployError::Capacity(Reason::Memory));
        }
        let token = self.next.fetch_add(1, Ordering::SeqCst);
        reservations.insert(token, (now + hold, cost));
        let held = self.reservations.clone();
        Ok(Release::new(move || {
            held.lock().unwrap_or_else(PoisonError::into_inner).remove(&token);
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI64;

    use super::*;

    fn hunts() -> DeployError {
        DeployError::Capacity(Reason::Hunts)
    }

    #[test]
    fn hard_slots_include_pending_launches_and_release_once() {
        let settings = Settings::from_pairs([("DRAIN_FILE", "no-such-drain-file.flag")]);
        let running = Arc::new(AtomicU64::new(4));
        let count = running.clone();
        let slots = HuntAdmission::with_limit(settings, move || count.load(Ordering::SeqCst), |_| Ok(Some(5)));
        let release = slots.reserve().unwrap();
        assert_eq!(
            slots.status().unwrap(),
            AdmissionStatus {
                running: 4,
                pending: 1,
                limit: Some(5)
            }
        );
        assert_eq!(slots.reserve().err(), Some(hunts()));
        running.fetch_add(1, Ordering::SeqCst);
        release.release();
        release.release();
        assert_eq!(slots.reserve().err(), Some(hunts()));
        running.fetch_sub(1, Ordering::SeqCst);
        slots.reserve().unwrap();
        assert_eq!(slots.reserve().err(), Some(hunts()));
    }

    #[test]
    fn worker_and_primary_limits_and_automatic_capacity() {
        let settings = Settings::from_pairs([("MAX_LOCAL_HUNTS", "5"), ("MAX_HUNTS", "3")]);
        assert_eq!(hunt_limit(&settings), Ok(Some(5)));
        settings.set("HUNT_WORKER", Some("1"));
        assert_eq!(hunt_limit(&settings), Ok(Some(3)));
        settings.set("MAX_HUNTS", Some("invalid"));
        assert!(hunt_limit(&settings).unwrap_err().contains("Invalid"));
        for (name, value) in [
            ("MAX_HUNTS", "auto"),
            ("PORT_RANGE_BEGIN", "8700"),
            ("PORT_RANGE_END", "8797"),
            ("GAMESERVER_CPU_GUARD", "1"),
            ("GAMESERVER_MEMORY_GUARD", "1"),
        ] {
            settings.set(name, Some(value));
        }
        assert_eq!(hunt_limit(&settings), Ok(Some(96)));
        settings.set("GAMESERVER_CPU_GUARD", Some("0"));
        assert!(hunt_limit(&settings).unwrap_err().contains("requires"));
        settings.set("GAMESERVER_CPU_GUARD", Some("1"));
        settings.set("PORT_RANGE_END", Some("8701"));
        assert!(hunt_limit(&settings).unwrap_err().contains("port range"));
    }

    #[test]
    fn memory_floor_reservations_release_and_expiry() {
        let settings = Settings::from_pairs([("GAMESERVER_MEMORY_GUARD", "1")]);
        let free = Arc::new(AtomicU64::new(4608 * MIB));
        let now = Arc::new(AtomicI64::new(0));
        let (f, n) = (free.clone(), now.clone());
        let admission = MemoryAdmission::new(
            settings.clone(),
            move || f.load(Ordering::SeqCst) as f64,
            move || n.load(Ordering::SeqCst),
        );
        let release = admission.reserve(false).unwrap();
        assert!(admission.reserve(false).is_err());
        release.release();
        admission.reserve(false).unwrap();
        now.store(60_000, Ordering::SeqCst);
        admission.reserve(false).unwrap();
        now.store(120_000, Ordering::SeqCst);
        free.fetch_sub(1, Ordering::SeqCst);
        assert_eq!(
            admission.reserve(false).err(),
            Some(DeployError::Capacity(Reason::Memory))
        );
        settings.set("GAMESERVER_MIN_FREE_MB", Some("invalid"));
        assert!(admission.reserve(false).unwrap_err().to_string().contains("Invalid"));
        settings.set("GAMESERVER_MIN_FREE_MB", None);
        settings.set("GAMESERVER_MEMORY_GUARD", Some("0"));
        admission.reserve(false).unwrap();
    }

    #[test]
    fn hunt_estimates_keep_the_floor_and_sum_mixed_reservations() {
        let settings = Settings::from_pairs([
            ("GAMESERVER_MEMORY_GUARD", "1"),
            ("GAMESERVER_MIN_FREE_MB", "1536"),
            ("GAMESERVER_HUNT_STARTUP_MB", "1024"),
        ]);
        let free = Arc::new(AtomicU64::new(2800 * MIB));
        let f = free.clone();
        let admission = MemoryAdmission::new(settings.clone(), move || f.load(Ordering::SeqCst) as f64, || 0);
        assert!(
            admission.reserve(false).is_err(),
            "persistent worlds still require their own estimate"
        );
        let hunt = admission.reserve(true).unwrap();
        assert!(
            admission.reserve(true).is_err(),
            "a concurrent hunt cannot spend the same RAM"
        );
        hunt.release();
        free.store(4096 * MIB, Ordering::SeqCst);
        let persistent = admission.reserve(false).unwrap();
        let small = admission.reserve(true).unwrap();
        assert!(admission.reserve(true).is_err());
        persistent.release();
        small.release();
        free.store(2559 * MIB, Ordering::SeqCst);
        assert!(admission.reserve(true).is_err(), "headroom is never waived");
        settings.set("GAMESERVER_HUNT_STARTUP_MB", Some("invalid"));
        assert!(admission.reserve(true).unwrap_err().to_string().contains("Invalid"));
    }

    #[test]
    fn the_launch_backlog_is_bounded_and_paced() {
        let settings = Settings::from_pairs([("MAX_PENDING_HUNTS", "2"), ("DRAIN_FILE", "no-such-drain-file.flag")]);
        let slots = HuntAdmission::with_limit(settings.clone(), || 0, |_| Ok(Some(96)));
        let (first, second) = (slots.reserve().unwrap(), slots.reserve().unwrap());
        assert!(slots.reserve().is_err());
        first.release();
        first.release();
        let third = slots.reserve().unwrap();
        assert_eq!(slots.pending(), 2);
        second.release();
        third.release();
        assert_eq!(slots.pending(), 0);

        settings.set("MAX_PENDING_HUNTS", Some("8"));
        settings.set("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP", Some("2"));
        assert_eq!(pending_hunt_limit(&settings), Ok(8));
        settings.set("SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP", Some("10"));
        assert_eq!(pending_hunt_limit(&settings), Ok(6));
        settings.set("MAX_PENDING_HUNTS", None);
        assert_eq!(pending_hunt_limit(&settings), Ok(6));
    }

    #[test]
    fn a_drain_file_stops_new_hunts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("drain.flag");
        let settings = Settings::from_pairs([("DRAIN_FILE", file.to_str().unwrap())]);
        let slots = HuntAdmission::with_limit(settings.clone(), || 3, |_| Ok(Some(96)));
        let held = slots.reserve().unwrap();
        assert!(!hunts_draining(&settings));
        std::fs::write(&file, "").unwrap();
        assert!(hunts_draining(&settings));
        assert!(slots.reserve().is_err());
        held.release();
        std::fs::remove_file(&file).unwrap();
        slots.reserve().unwrap().release();
    }
}
