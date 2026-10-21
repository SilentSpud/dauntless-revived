//! Who is playing (`controllers/undauntedapi.ts` player activity, `controllers/moderation.ts` playing
//! addresses): in memory, fed by the clients' heartbeats.
//!
//! A heartbeat counts as online for 90 s; an account's activity is kept ten minutes after its last
//! heartbeat (a reconnection grace period), then forgotten. The public address an account played
//! from is kept 90 s, for an admin's address ban (it must be the address the account is playing
//! from now).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;

pub const ONLINE_MS: i64 = 90 * 1000;
const KEEP_MS: i64 = 10 * 60 * 1000;
const PRUNE_EVERY_MS: i64 = 60 * 1000;
const ADDRESS_MS: i64 = 90 * 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct Heartbeat {
    /// The heartbeat's `map`, as sent (`None`: it sent none).
    pub map: Option<Value>,
    /// Its `state` (menu, city, island...) when it is a string.
    pub state: Option<String>,
    pub at: i64,
}

#[derive(Debug, Default)]
pub struct State {
    /// In insertion order, as a JavaScript Map iterates (a later heartbeat keeps the place).
    heartbeats: Vec<(String, Heartbeat)>,
    last_prune: i64,
    /// Account -> (public address, last seen).
    playing: HashMap<String, (String, i64)>,
}

#[derive(Debug, Default)]
pub struct Activity(Mutex<State>);

impl Activity {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl State {
    /// `PrunePlayerTracking`: at most once a minute.
    fn prune(&mut self, now: i64) {
        if now >= self.last_prune && now - self.last_prune < PRUNE_EVERY_MS {
            return;
        }
        self.last_prune = now;
        self.heartbeats.retain(|(_, beat)| now - beat.at <= KEEP_MS);
    }

    /// `UpdatePlayerActivity`.
    pub fn heartbeat(&mut self, user: &str, map: Option<Value>, state: Option<&Value>, now: i64) {
        self.prune(now);
        let beat = Heartbeat {
            map,
            state: state.and_then(Value::as_str).map(str::to_owned),
            at: now,
        };
        match self.heartbeats.iter_mut().find(|(id, _)| id == user) {
            Some(slot) => slot.1 = beat,
            None => self.heartbeats.push((user.to_owned(), beat)),
        }
    }

    /// `GetRecentPlayerData` / `GetOnlinePlayerActivity`: heartbeats of the last 90 s.
    pub fn online(&mut self, now: i64) -> Vec<(String, Heartbeat)> {
        self.prune(now);
        self.heartbeats
            .iter()
            .filter(|(_, beat)| now - beat.at <= ONLINE_MS)
            .cloned()
            .collect()
    }

    fn expire_addresses(&mut self, now: i64) {
        self.playing.retain(|_, (_, seen)| now - *seen < ADDRESS_MS);
    }

    /// `RecordPlayingAddress`: only a public address (callers pass `None` for loopback or none).
    pub fn record_address(&mut self, user: &str, address: String, now: i64) {
        self.playing.insert(user.to_owned(), (address, now));
    }

    /// `EndPlayingAddress`.
    pub fn end_address(&mut self, user: &str) {
        self.playing.remove(user);
    }

    /// The address the account is playing from now, and when it was last seen.
    pub fn playing_address(&mut self, user: &str, now: i64) -> Option<(String, i64)> {
        self.expire_addresses(now);
        self.playing.get(user).cloned()
    }
}
