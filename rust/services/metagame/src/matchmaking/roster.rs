//! `CandidateRoster` (`controllers/matchmakingroster.ts`): who each allocation expects, so a game
//! server asking who is still expected (POST /candidate/player/alive) learns which players stopped
//! matchmaking. Client activity keeps a waiting place alive; a game server's question does not, or
//! an abandoned player would renew their own place forever.
//!
//! A member record is shared between the player's current place and the allocation snapshots that
//! took it along (Node shared one object); here records live in an arena and both refer to them by
//! key.

use std::collections::{HashMap, HashSet};

pub const QUEUED_PLAYER_LEASE_MS: i64 = 45_000;
pub const PLAYER_LOADING_GRACE_MS: i64 = 180_000;
pub const CONNECTED_PLAYER_ABSENCE_GRACE_MS: i64 = 45_000;
const ROSTER_RETENTION_MS: i64 = 25 * 60 * 60 * 1000;
const SWEEP_EVERY_MS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Queued,
    Allocating,
    Assigned,
    Served,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    Cancelled,
    Replaced,
}

#[derive(Debug, Clone)]
struct Member {
    candidate_id: String,
    hunt_id: String,
    last_activity: i64,
    phase: Phase,
    allocation_id: Option<String>,
    served_at: Option<i64>,
    removal: Option<Removal>,
}

#[derive(Debug, Clone)]
struct Allocation {
    id: String,
    /// Player -> member key, in the order the allocation took them.
    members: Vec<(String, u64)>,
    deadline: i64,
    assigned_at: Option<i64>,
    session_id: Option<String>,
    last_seen: i64,
    arrived: HashSet<String>,
    missing_since: HashMap<String, i64>,
}

impl Allocation {
    fn member(&self, player: &str) -> Option<u64> {
        self.members.iter().find(|(p, _)| p == player).map(|(_, key)| *key)
    }
}

/// One player of an allocation: who, and for which candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub player_id: String,
    pub candidate_id: String,
}

/// What `reconcileExpected` knows about the asking game server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    pub session_id: Option<String>,
    pub allocation_id: Option<String>,
    /// `None`: no complete, authenticated snapshot. An empty list is different: the native game
    /// thread confirmed there are no players.
    pub connected_player_ids: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Roster {
    arena: HashMap<u64, Member>,
    next_key: u64,
    current: HashMap<String, u64>,
    allocations: HashMap<String, Allocation>,
    candidate_allocations: HashMap<String, Vec<String>>,
    sessions: HashMap<String, String>,
    last_sweep: Option<i64>,
}

impl Roster {
    fn add(&mut self, member: Member) -> u64 {
        self.next_key += 1;
        self.arena.insert(self.next_key, member);
        self.next_key
    }

    fn current_member(&mut self, player: &str) -> Option<&mut Member> {
        let key = *self.current.get(player)?;
        self.arena.get_mut(&key)
    }

    /// `recordQueued`.
    pub fn record_queued(&mut self, player: &str, candidate: &str, hunt: &str, now: i64) {
        self.prune(now);
        if let Some(previous) = self.current_member(player) {
            if previous.candidate_id == candidate && previous.phase != Phase::Removed {
                previous.last_activity = now;
                return;
            }
            previous.phase = Phase::Removed;
            previous.removal = Some(Removal::Replaced);
        }
        let key = self.add(Member {
            candidate_id: candidate.to_owned(),
            hunt_id: hunt.to_owned(),
            last_activity: now,
            phase: Phase::Queued,
            allocation_id: None,
            served_at: None,
            removal: None,
        });
        self.current.insert(player.to_owned(), key);
    }

    /// `touchCandidate`.
    pub fn touch_candidate(&mut self, player: &str, candidate: &str, now: i64) -> bool {
        match self.current_member(player) {
            Some(member) if member.candidate_id == candidate && member.phase != Phase::Removed => {
                member.last_activity = now;
                true
            }
            _ => false,
        }
    }

    /// `queueMemberActive`.
    pub fn queue_member_active(&self, player: &str, candidate: &str, now: i64) -> bool {
        self.current
            .get(player)
            .and_then(|key| self.arena.get(key))
            .is_some_and(|m| {
                m.candidate_id == candidate
                    && m.phase == Phase::Queued
                    && now - m.last_activity <= QUEUED_PLAYER_LEASE_MS
            })
    }

    /// `beginAllocation`.
    pub fn begin_allocation(&mut self, allocation: &str, seats: &[Seat], deadline: i64, now: i64) {
        self.prune(now);
        let mut members = Vec::new();
        for seat in seats {
            let Some(&key) = self.current.get(&seat.player_id) else {
                continue;
            };
            let Some(member) = self.arena.get_mut(&key) else {
                continue;
            };
            if member.candidate_id != seat.candidate_id || member.phase == Phase::Removed {
                continue;
            }
            member.phase = Phase::Allocating;
            member.allocation_id = Some(allocation.to_owned());
            if let Some(slot) = members
                .iter_mut()
                .find(|(p, _): &&mut (String, u64)| *p == seat.player_id)
            {
                slot.1 = key;
            } else {
                members.push((seat.player_id.clone(), key));
            }
        }
        for (_, key) in &members {
            if let Some(member) = self.arena.get(key) {
                let ids = self
                    .candidate_allocations
                    .entry(member.candidate_id.clone())
                    .or_default();
                if !ids.iter().any(|id| id == allocation) {
                    ids.push(allocation.to_owned());
                }
            }
        }
        self.allocations.insert(
            allocation.to_owned(),
            Allocation {
                id: allocation.to_owned(),
                members,
                deadline,
                assigned_at: None,
                session_id: None,
                last_seen: now,
                arrived: HashSet::new(),
                missing_since: HashMap::new(),
            },
        );
    }

    /// `assignAllocation`.
    pub fn assign_allocation(&mut self, allocation: &str, session: Option<&str>, now: i64) {
        let Some(found) = self.allocations.get_mut(allocation) else {
            return;
        };
        found.assigned_at = Some(now);
        found.last_seen = now;
        found.session_id = session.map(str::to_owned);
        let members = found.members.clone();
        if let Some(session) = session {
            self.sessions.insert(session.to_owned(), allocation.to_owned());
        }
        for (player, key) in members {
            if self.current.get(&player) == Some(&key)
                && let Some(member) = self.arena.get_mut(&key)
                && member.allocation_id.as_deref() == Some(allocation)
                && member.phase != Phase::Removed
            {
                member.phase = Phase::Assigned;
            }
        }
    }

    /// `releaseAllocation`: an explicit capacity refusal started no server. Its candidates are
    /// restored without new activity for members who stopped polling during the wait.
    pub fn release_allocation(&mut self, allocation: &str) {
        let Some(found) = self.allocations.get(allocation) else {
            return;
        };
        for (player, key) in found.members.clone() {
            if self.current.get(&player) == Some(&key)
                && let Some(member) = self.arena.get_mut(&key)
                && member.allocation_id.as_deref() == Some(allocation)
                && member.phase != Phase::Removed
            {
                member.phase = Phase::Queued;
                member.allocation_id = None;
            }
        }
        self.delete_allocation(allocation);
    }

    /// `served`.
    pub fn served(&mut self, player: &str, candidate: &str, now: i64) {
        if let Some(member) = self.current_member(player)
            && member.candidate_id == candidate
            && member.phase != Phase::Removed
        {
            member.served_at.get_or_insert(now);
            member.phase = Phase::Served;
        }
    }

    fn mark_removed(member: &mut Member, reason: Removal) {
        member.phase = Phase::Removed;
        if member.removal != Some(Removal::Cancelled) {
            member.removal = Some(reason);
        }
    }

    /// `removePlayer`.
    pub fn remove_player(&mut self, player: &str, candidate: &str, reason: Removal) {
        if let Some(&key) = self.current.get(player)
            && let Some(member) = self.arena.get_mut(&key)
            && member.candidate_id == candidate
        {
            Self::mark_removed(member, reason);
            self.current.remove(player);
        }
        for id in self.candidate_allocations.get(candidate).cloned().unwrap_or_default() {
            let Some(key) = self.allocations.get(&id).and_then(|a| a.member(player)) else {
                continue;
            };
            if let Some(original) = self.arena.get_mut(&key)
                && original.candidate_id == candidate
            {
                Self::mark_removed(original, reason);
            }
        }
    }

    /// `restoreAssigned`: a ready party candidate rebuilt for a member restores their original
    /// membership; a cancelled one can't come back through the party's old candidate.
    pub fn restore_assigned(
        &mut self,
        player: &str,
        candidate: &str,
        hunt: &str,
        session: Option<&str>,
        now: i64,
    ) -> bool {
        let ids = self.candidate_allocations.get(candidate).cloned().unwrap_or_default();
        let matches: Vec<(String, u64)> = ids
            .iter()
            .filter_map(|id| {
                let allocation = self.allocations.get(id)?;
                let key = allocation.member(player)?;
                let member = self.arena.get(&key)?;
                (member.candidate_id == candidate
                    && member.hunt_id == hunt
                    && member.allocation_id.as_deref() == Some(allocation.id.as_str())
                    && allocation.assigned_at.is_some()
                    && session.is_none_or(|s| allocation.session_id.as_deref() == Some(s)))
                .then(|| (allocation.id.clone(), key))
            })
            .collect();
        let [(allocation, key)] = matches.as_slice() else {
            return false;
        };
        if self
            .arena
            .get(key)
            .is_none_or(|m| m.removal == Some(Removal::Cancelled))
        {
            return false;
        }
        if let Some(&previous) = self.current.get(player)
            && previous != *key
            && let Some(previous) = self.arena.get_mut(&previous)
        {
            previous.phase = Phase::Removed;
            previous.removal = Some(Removal::Replaced);
        }
        if let Some(member) = self.arena.get_mut(key) {
            member.phase = if member.served_at.is_none() {
                Phase::Assigned
            } else {
                Phase::Served
            };
            member.removal = None;
            member.last_activity = now;
        }
        if let Some(found) = self.allocations.get_mut(allocation) {
            found.last_seen = now;
        }
        self.current.insert(player.to_owned(), *key);
        true
    }

    /// `cancelCandidate`.
    pub fn cancel_candidate(&mut self, candidate: &str) {
        let players: Vec<String> = self
            .current
            .iter()
            .filter(|(_, key)| self.arena.get(key).is_some_and(|m| m.candidate_id == candidate))
            .map(|(player, _)| player.clone())
            .collect();
        for player in players {
            self.remove_player(&player, candidate, Removal::Cancelled);
        }
        // A replaced member can still belong to an in-flight or assigned snapshot
        for id in self.candidate_allocations.get(candidate).cloned().unwrap_or_default() {
            let keys: Vec<u64> = self
                .allocations
                .get(&id)
                .map(|a| a.members.iter().map(|(_, key)| *key).collect())
                .unwrap_or_default();
            for key in keys {
                if let Some(member) = self.arena.get_mut(&key)
                    && member.candidate_id == candidate
                {
                    member.phase = Phase::Removed;
                    member.removal = Some(Removal::Cancelled);
                }
            }
        }
    }

    /// `reconcileExpected`: the expected players a game server should keep waiting for.
    pub fn reconcile_expected(&mut self, players: &[String], context: &Context, now: i64) -> Vec<String> {
        let id = match &context.session_id {
            Some(session) => self.sessions.get(session).cloned(),
            None => context.allocation_id.clone(),
        };
        // Legacy workers and workers surviving a metagame restart can't be identified safely:
        // missing memory is no evidence that players left
        let Some(id) = id.filter(|id| self.allocations.contains_key(id)) else {
            return players.to_vec();
        };
        let connected: Option<HashSet<&String>> = context.connected_player_ids.as_ref().map(|ids| ids.iter().collect());
        let Some(allocation) = self.allocations.get_mut(&id) else {
            return players.to_vec();
        };
        allocation.last_seen = now;
        // An incomplete snapshot says nothing about absence
        if connected.is_none() {
            allocation.missing_since.clear();
        }
        let mut kept = Vec::new();
        for player in players {
            let Some(key) = allocation.member(player) else {
                kept.push(player.clone());
                continue;
            };
            if connected.as_ref().is_some_and(|c| c.contains(player)) {
                allocation.arrived.insert(player.clone());
                allocation.missing_since.remove(player);
                kept.push(player.clone());
                continue;
            }
            if allocation.arrived.contains(player) {
                if connected.is_none() {
                    kept.push(player.clone());
                    continue;
                }
                let since = *allocation.missing_since.entry(player.clone()).or_insert(now);
                if now - since <= CONNECTED_PLAYER_ABSENCE_GRACE_MS {
                    kept.push(player.clone());
                    continue;
                }
            }
            let Some(member) = self.arena.get(&key) else {
                continue;
            };
            if member.phase == Phase::Removed
                || self.current.get(player) != Some(&key)
                || member.allocation_id.as_deref() != Some(allocation.id.as_str())
            {
                continue;
            }
            // The game server sends its whole original roster, connected players included:
            // silence alone can't identify a no-show
            let keep = if connected.is_none() {
                true
            } else if let Some(assigned) = allocation.assigned_at {
                let loading = member.served_at.unwrap_or(assigned);
                now - loading <= PLAYER_LOADING_GRACE_MS || now - member.last_activity <= QUEUED_PLAYER_LEASE_MS
            } else {
                now <= allocation.deadline
            };
            if keep {
                kept.push(player.clone());
            }
        }
        kept
    }

    fn delete_allocation(&mut self, id: &str) {
        let Some(allocation) = self.allocations.remove(id) else {
            return;
        };
        for (_, key) in &allocation.members {
            let Some(member) = self.arena.get(key) else {
                continue;
            };
            if let Some(ids) = self.candidate_allocations.get_mut(&member.candidate_id) {
                ids.retain(|other| other != id);
                if ids.is_empty() {
                    self.candidate_allocations.remove(&member.candidate_id);
                }
            }
        }
        if let Some(session) = &allocation.session_id
            && self.sessions.get(session).map(String::as_str) == Some(id)
        {
            self.sessions.remove(session);
        }
    }

    /// At most every 30 s: allocations and places nobody has seen for 25 hours go, and so do the
    /// member records nothing refers to any more.
    fn prune(&mut self, now: i64) {
        if self.last_sweep.is_some_and(|at| now - at < SWEEP_EVERY_MS) {
            return;
        }
        self.last_sweep = Some(now);
        let old: Vec<String> = self
            .allocations
            .values()
            .filter(|a| now - a.last_seen > ROSTER_RETENTION_MS)
            .map(|a| a.id.clone())
            .collect();
        for id in old {
            self.delete_allocation(&id);
        }
        let stale: Vec<String> = self
            .current
            .iter()
            .filter(|(_, key)| {
                self.arena.get(key).is_some_and(|m| {
                    now - m.last_activity > ROSTER_RETENTION_MS
                        && m.allocation_id
                            .as_ref()
                            .is_none_or(|a| !self.allocations.contains_key(a))
                })
            })
            .map(|(player, _)| player.clone())
            .collect();
        for player in stale {
            self.current.remove(&player);
        }
        let used: HashSet<u64> = self
            .current
            .values()
            .copied()
            .chain(
                self.allocations
                    .values()
                    .flat_map(|a| a.members.iter().map(|(_, key)| *key)),
            )
            .collect();
        self.arena.retain(|key, _| used.contains(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(player: &str, candidate: &str) -> Seat {
        Seat {
            player_id: player.into(),
            candidate_id: candidate.into(),
        }
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn a_complete_snapshot_drops_players_who_stopped_matchmaking() {
        let mut roster = Roster::default();
        roster.record_queued("a", "ca", "hunt", 0);
        roster.record_queued("b", "cb", "hunt", 0);
        roster.begin_allocation("alloc", &[seat("a", "ca"), seat("b", "cb")], 100_000, 0);
        roster.assign_allocation("alloc", Some("sess"), 1_000);
        roster.remove_player("b", "cb", Removal::Cancelled);
        let context = Context {
            session_id: Some("sess".into()),
            connected_player_ids: Some(vec![]),
            ..Context::default()
        };
        // a is still loading; b cancelled; c is not the roster's to judge
        assert_eq!(
            roster.reconcile_expected(&ids(&["a", "b", "c"]), &context, 2_000),
            ids(&["a", "c"])
        );
        // Without a snapshot nobody is dropped for silence, but the cancelled one is
        let unknown = Context {
            session_id: Some("sess".into()),
            ..Context::default()
        };
        assert_eq!(
            roster.reconcile_expected(&ids(&["a", "b"]), &unknown, 2_000),
            ids(&["a"])
        );
        // An unknown session keeps the list as it is
        let other = Context {
            session_id: Some("other".into()),
            ..Context::default()
        };
        assert_eq!(
            roster.reconcile_expected(&ids(&["a", "b"]), &other, 2_000),
            ids(&["a", "b"])
        );
    }

    #[test]
    fn an_arrived_player_gets_a_grace_period_after_leaving() {
        let mut roster = Roster::default();
        roster.record_queued("a", "ca", "hunt", 0);
        roster.begin_allocation("alloc", &[seat("a", "ca")], 100_000, 0);
        roster.assign_allocation("alloc", Some("sess"), 0);
        let snapshot = |ids: &[&str]| Context {
            session_id: Some("sess".into()),
            connected_player_ids: Some(ids.iter().map(|id| (*id).to_owned()).collect()),
            ..Context::default()
        };
        assert_eq!(
            roster.reconcile_expected(&ids(&["a"]), &snapshot(&["a"]), 1_000),
            ids(&["a"])
        );
        assert_eq!(
            roster.reconcile_expected(&ids(&["a"]), &snapshot(&[]), 400_000),
            ids(&["a"])
        );
        assert_eq!(
            roster.reconcile_expected(
                &ids(&["a"]),
                &snapshot(&[]),
                400_000 + CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_capacity_refusal_restores_the_queued_place() {
        let mut roster = Roster::default();
        roster.record_queued("a", "ca", "hunt", 0);
        roster.begin_allocation("alloc", &[seat("a", "ca")], 100_000, 0);
        assert!(!roster.queue_member_active("a", "ca", 1));
        roster.release_allocation("alloc");
        assert!(roster.queue_member_active("a", "ca", 1));
        assert!(!roster.queue_member_active("a", "ca", QUEUED_PLAYER_LEASE_MS + 1));
    }

    #[test]
    fn a_ready_party_membership_can_be_restored_once_assigned() {
        let mut roster = Roster::default();
        roster.record_queued("a", "ca", "hunt", 0);
        roster.begin_allocation("alloc", &[seat("a", "ca")], 100_000, 0);
        assert!(!roster.restore_assigned("a", "ca", "hunt", None, 1));
        roster.assign_allocation("alloc", Some("sess"), 1);
        roster.record_queued("a", "other", "hunt", 2);
        assert!(roster.restore_assigned("a", "ca", "hunt", Some("sess"), 3));
        assert!(!roster.restore_assigned("a", "ca", "hunt", Some("wrong"), 3));
        roster.cancel_candidate("ca");
        assert!(!roster.restore_assigned("a", "ca", "hunt", Some("sess"), 4));
    }
}
