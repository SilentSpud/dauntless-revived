//! When a temporary world is retired (`huntlifetime.ts`). Each reason is a different kind of proof:
//!
//! - `empty`: continuously observed zero connections. The strongest signal, and the usual one.
//! - `silent`: no telemetry at all while other instances on this host still report. The process is
//!   hung or never listened, so neither its own idle timer nor `empty` can ever end it.
//! - `max-age`: the documented ceiling. Occupancy does not matter: nothing legitimate runs this long.

use std::collections::{HashMap, HashSet};

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retirement {
    Empty,
    Silent,
    MaxAge,
}

impl Retirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Silent => "silent",
            Self::MaxAge => "max-age",
        }
    }
}

/// What the policy needs to know about an instance.
#[derive(Debug, Clone)]
pub struct Hunt {
    pub id: String,
    pub persistent: bool,
    pub start_time: i64,
    pub ready_at: Option<i64>,
}

/// How recently some instance has to have reported for silence to mean anything.
const TELEMETRY_WINDOW_MS: i64 = 10 * 60_000;
/// A gap this long between two observations breaks the interval.
const OBSERVATION_GAP_MS: i64 = 90_000;

#[derive(Debug, Clone, Copy)]
struct Span {
    since: i64,
    last: i64,
}

#[derive(Debug)]
pub struct HuntLifetime {
    max_age_ms: i64,
    empty_ms: i64,
    startup_grace_ms: i64,
    silent_ms: i64,
    unready_grace_ms: i64,
    empty_since: HashMap<String, Span>,
    silent_since: HashMap<String, Span>,
    /// When any instance last reported a real count
    telemetry_at: i64,
}

impl HuntLifetime {
    pub fn new(max_age_ms: i64, empty_ms: i64, startup_grace_ms: i64, silent_ms: i64, unready_grace_ms: i64) -> Self {
        Self {
            max_age_ms,
            empty_ms,
            startup_grace_ms,
            silent_ms,
            unready_grace_ms,
            empty_since: HashMap::new(),
            silent_since: HashMap::new(),
            telemetry_at: i64::MIN,
        }
    }

    /// `HuntLifetimeFromEnv`.
    pub fn from_settings(settings: &Settings) -> Result<Self, String> {
        let minutes = |name: &str, fallback: i64| -> Result<i64, String> {
            let value = settings.number(name).unwrap_or(fallback as f64);
            if value.fract() != 0.0 || !(1.0..=1440.0).contains(&value) {
                return Err(format!("Invalid {name}"));
            }
            Ok(value as i64 * 60_000)
        };
        let grace = if settings.get("HUNT_STARTUP_GRACE_MINUTES").is_none() {
            0
        } else {
            minutes("HUNT_STARTUP_GRACE_MINUTES", 1)?
        };
        Ok(Self::new(
            minutes("HUNT_MAX_AGE_MINUTES", 120)?,
            minutes("HUNT_EMPTY_MINUTES", 2)?,
            grace,
            minutes("HUNT_SILENT_MINUTES", 15)?,
            minutes("HUNT_UNREADY_GRACE_MINUTES", 5)?,
        ))
    }

    pub fn prune(&mut self, ids: &HashSet<String>) {
        self.empty_since.retain(|id, _| ids.contains(id));
        self.silent_since.retain(|id, _| ids.contains(id));
    }

    pub fn reason(&mut self, server: &Hunt, connections: Option<u64>, now: i64) -> Option<Retirement> {
        if server.persistent {
            return None;
        }
        if connections.is_some() && now > self.telemetry_at {
            self.telemetry_at = now;
        }
        if now - server.start_time >= self.max_age_ms {
            self.forget(&server.id);
            return Some(Retirement::MaxAge);
        }
        // A process that never wrote its ready file never got its net driver listening: judge it
        // on occupancy like any other once it has had its grace
        let settled = server.ready_at.unwrap_or(server.start_time + self.unready_grace_ms) + self.startup_grace_ms;
        if now < settled {
            self.forget(&server.id);
            return None;
        }
        let Some(connections) = connections else {
            self.empty_since.remove(&server.id);
            if self.telemetry_at == i64::MIN || now - self.telemetry_at > TELEMETRY_WINDOW_MS {
                self.silent_since.remove(&server.id);
                return None;
            }
            return elapsed(&mut self.silent_since, &server.id, now, self.silent_ms).then_some(Retirement::Silent);
        };
        self.silent_since.remove(&server.id);
        if connections != 0 {
            self.empty_since.remove(&server.id);
            return None;
        }
        elapsed(&mut self.empty_since, &server.id, now, self.empty_ms).then_some(Retirement::Empty)
    }

    fn forget(&mut self, id: &str) {
        self.empty_since.remove(id);
        self.silent_since.remove(id);
    }
}

fn elapsed(spans: &mut HashMap<String, Span>, id: &str, now: i64, needed: i64) -> bool {
    let span = spans.entry(id.to_owned()).or_insert(Span { since: now, last: now });
    if now < span.last || now - span.last > OBSERVATION_GAP_MS {
        *span = Span { since: now, last: now };
    }
    span.last = now;
    now - span.since >= needed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunt() -> Hunt {
        Hunt {
            id: "h".into(),
            persistent: false,
            start_time: 0,
            ready_at: Some(0),
        }
    }

    fn defaults() -> HuntLifetime {
        HuntLifetime::from_settings(&Settings::default()).unwrap()
    }

    #[test]
    fn the_ceiling_retires_whatever_the_occupancy_but_never_a_persistent_world() {
        let mut policy = HuntLifetime::new(120_000, 10_000, 0, 15 * 60_000, 5 * 60_000);
        assert_eq!(policy.reason(&hunt(), Some(4), 119_999), None);
        assert_eq!(policy.reason(&hunt(), Some(4), 120_000), Some(Retirement::MaxAge));
        assert_eq!(policy.reason(&hunt(), None, 120_000), Some(Retirement::MaxAge));
        let persistent = Hunt {
            persistent: true,
            ..hunt()
        };
        assert_eq!(policy.reason(&persistent, Some(0), 999_999), None);
    }

    #[test]
    fn empty_needs_continuous_zero_readings() {
        let mut policy = HuntLifetime::new(120_000, 10_000, 0, 15 * 60_000, 5 * 60_000);
        let h = hunt();
        for (connections, at, expected) in [
            (Some(0), 10_000, None),
            (Some(1), 19_000, None),
            (Some(0), 20_000, None),
            (None, 29_000, None),
            (Some(0), 30_000, None),
            (Some(0), 39_999, None),
            (Some(0), 40_000, Some(Retirement::Empty)),
        ] {
            assert_eq!(policy.reason(&h, connections, at), expected, "{at}");
        }
        policy.prune(&HashSet::new());
        assert_eq!(policy.reason(&h, Some(0), 41_000), None);
    }

    #[test]
    fn invalid_configuration_fails() {
        for (name, value) in [
            ("HUNT_MAX_AGE_MINUTES", "0"),
            ("HUNT_EMPTY_MINUTES", "NaN"),
            ("HUNT_SILENT_MINUTES", "0"),
            ("HUNT_UNREADY_GRACE_MINUTES", "1441"),
        ] {
            assert!(
                HuntLifetime::from_settings(&Settings::from_pairs([(name, value)])).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn the_defaults() {
        let mut policy = defaults();
        let unready = Hunt {
            ready_at: None,
            ..hunt()
        };
        assert_eq!(policy.reason(&unready, Some(0), 0), None);
        assert_eq!(policy.reason(&unready, Some(0), 120_000), None);
        let mut policy = defaults();
        for (at, expected) in [(120_000, None), (180_000, None), (240_000, Some(Retirement::Empty))] {
            assert_eq!(policy.reason(&hunt(), Some(0), at), expected);
        }
        let mut policy = defaults();
        for (at, expected) in [
            (0, None),
            (120_000, None),
            (180_000, None),
            (240_000, Some(Retirement::Empty)),
        ] {
            assert_eq!(policy.reason(&hunt(), Some(0), at), expected);
        }
        let mut policy = defaults();
        for (at, expected) in [
            (299_999, None),
            (300_000, None),
            (360_000, None),
            (420_000, Some(Retirement::Empty)),
        ] {
            assert_eq!(policy.reason(&unready, Some(0), at), expected, "{at}");
        }
        let mut policy = defaults();
        for at in (300_000..=600_000).step_by(60_000) {
            assert_eq!(policy.reason(&unready, Some(2), at), None);
        }
    }

    #[test]
    fn silence_counts_only_while_telemetry_works_elsewhere() {
        let mut policy = defaults();
        let neighbour = Hunt {
            id: "n".into(),
            ..hunt()
        };
        let mut last = None;
        for at in (0..=900_000).step_by(60_000) {
            assert_eq!(policy.reason(&neighbour, Some(3), at), None);
            last = policy.reason(&hunt(), None, at);
            if at < 900_000 {
                assert_eq!(last, None);
            }
        }
        assert_eq!(last, Some(Retirement::Silent));

        let mut policy = defaults();
        for at in (0..=1_800_000).step_by(60_000) {
            assert_eq!(policy.reason(&hunt(), None, at), None);
        }

        let mut policy = defaults();
        for at in (0..=840_000).step_by(60_000) {
            policy.reason(&neighbour, Some(3), at);
            assert_eq!(policy.reason(&hunt(), None, at), None);
        }
        assert_eq!(policy.reason(&hunt(), Some(1), 870_000), None);
        assert_eq!(policy.reason(&hunt(), None, 900_000), None);
    }
}
