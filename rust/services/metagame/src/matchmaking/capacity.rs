//! `CapacityTurns` (`controllers/capacityturns.ts`): allocation attempts per region run one at a
//! time, oldest candidate first, until that candidate has a result. The deploy server still owns
//! the actual capacity admission.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
struct Turn {
    region: String,
    queued_at: i64,
    order: u64,
    seen: i64,
    busy: bool,
}

#[derive(Debug, Default)]
pub struct CapacityTurns {
    turns: HashMap<String, Turn>,
    sequence: u64,
    idle_ms: i64,
}

impl CapacityTurns {
    pub fn new(idle_ms: i64) -> Self {
        Self {
            idle_ms,
            ..Self::default()
        }
    }

    pub fn touch(&mut self, id: &str, now: i64) {
        if let Some(turn) = self.turns.get_mut(id) {
            turn.seen = now;
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.turns.remove(id);
    }

    /// Whether `id` may ask the deploy server now; it waits while an older candidate of the region
    /// is waiting or asking.
    pub fn start(&mut self, id: &str, region: &str, now: i64, active: &HashSet<String>, queued_at: i64) -> bool {
        let idle = self.idle_ms;
        self.turns
            .retain(|key, turn| active.contains(key) && (turn.busy || now - turn.seen < idle));
        if !self.turns.contains_key(id) {
            self.sequence += 1;
            self.turns.insert(
                id.to_owned(),
                Turn {
                    region: region.to_owned(),
                    queued_at,
                    order: self.sequence,
                    seen: now,
                    busy: false,
                },
            );
        }
        let Some(turn) = self.turns.get(id).cloned() else {
            return false;
        };
        if turn.busy {
            return false;
        }
        if let Some(own) = self.turns.get_mut(id) {
            own.seen = now;
        }
        let waits = self.turns.iter().any(|(key, other)| {
            other.region == region
                && key != id
                && (other.busy
                    || other.queued_at < turn.queued_at
                    || (other.queued_at == turn.queued_at && other.order < turn.order))
        });
        if waits {
            return false;
        }
        if let Some(own) = self.turns.get_mut(id) {
            own.busy = true;
        }
        true
    }

    /// The attempt is over: a capacity refusal keeps the turn for the next try.
    pub fn finish(&mut self, id: &str, capacity: bool, now: i64) {
        if capacity {
            if let Some(turn) = self.turns.get_mut(id) {
                turn.busy = false;
                turn.seen = now;
            }
        } else {
            self.turns.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn the_oldest_candidate_of_a_region_goes_first() {
        let mut turns = CapacityTurns::new(60_000);
        let all = active(&["a", "b", "c"]);
        assert!(turns.start("a", "main:ISLAND", 0, &all, 10));
        // b waits while a asks; c in another region doesn't
        assert!(!turns.start("b", "main:ISLAND", 1, &all, 5));
        assert!(turns.start("c", "us:ISLAND", 1, &all, 20));
        turns.finish("a", true, 2);
        // a is not busy now, but b queued earlier
        assert!(!turns.start("a", "main:ISLAND", 3, &all, 10));
        assert!(turns.start("b", "main:ISLAND", 3, &all, 5));
        turns.finish("b", false, 4);
        assert!(turns.start("a", "main:ISLAND", 5, &all, 10));
    }

    #[test]
    fn idle_and_inactive_turns_are_dropped() {
        let mut turns = CapacityTurns::new(100);
        let all = active(&["a", "b"]);
        assert!(turns.start("a", "r", 0, &all, 0));
        turns.finish("a", true, 0);
        // a stopped polling: after the idle time b no longer waits for it
        assert!(!turns.start("b", "r", 50, &all, 1));
        assert!(turns.start("b", "r", 200, &all, 1));
        turns.finish("b", true, 200);
        // b is no longer an active candidate
        assert!(turns.start("a", "r", 201, &active(&["a"]), 0));
    }
}
