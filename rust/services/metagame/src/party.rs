//! Parties and party invites (`controllers/party.ts`).
//!
//! In memory, like matchmaking: a party only lives as long as the game sessions around it, and the
//! client drops its party itself at login and at character select (DELETE /party/member). Every
//! member's client polls POST /party and GET /party/invites about every 10 s, so a restart just
//! leaves everyone in a party of one.
//!
//! Reply shapes are the 1.4.4 client's own. A party of one answers exactly what the old stub
//! answered (the fake candidate included), so a player on their own sees no change; only the party
//! id is a random UUID. `PARTY_SOLO_STUB=0` answers a party of one without the fake candidate.
//!
//! Invites: a sender may send at most [`MAX_INVITES_PER_SENDER`] per [`INVITE_WINDOW_MS`], and
//! can't invite a player again for [`DECLINE_COOLDOWN_MS`] after that player declined them. A block
//! drops the invites between the two, and invites between blocked players are never listed.
//!
//! Members that stop polling and stop sending heartbeats for [`MEMBER_TIMEOUT_MS`] are dropped, and
//! the next member (join order) becomes leader if the leader drops.
//!
//! Every operation runs with the state locked and a database connection at hand (names, blocks).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use diesel::QueryResult;
use serde_json::{Value, json};

use crate::accounts::{is_account_id_shape, username_for};
use crate::friends::blocked_either_way;

pub type Conn = store::Connection;

pub const MAX_PARTY_SIZE: usize = 4;
pub const INVITE_TTL_MS: i64 = 5 * 60 * 1000;
pub const MEMBER_TIMEOUT_MS: i64 = 120 * 1000;
/// A party candidate stays visible in the party poll until every member was sent to the server and
/// this long has passed, or until [`CANDIDATE_MAX_AGE_MS`] in any case.
pub const CANDIDATE_LINGER_MS: i64 = 60 * 1000;
pub const CANDIDATE_MAX_AGE_MS: i64 = 5 * 60 * 1000;
/// A member's own join for the hunt the party is already on is answered from the party's last
/// candidate for this long.
pub const LAST_CANDIDATE_REJOIN_MS: i64 = 15 * 60 * 1000;
/// A party of one nobody has heard from in this long is forgotten.
pub const IDLE_PARTY_MS: i64 = 30 * 60 * 1000;
pub const MAX_INVITES_PER_PARTY: usize = 8;
pub const MAX_INVITES_PER_RECIPIENT: usize = 10;
pub const MAX_STATUS_PLAYER_IDS: usize = 16;
pub const MAX_INVITES_PER_SENDER: usize = 20;
pub const INVITE_WINDOW_MS: i64 = 10 * 60 * 1000;
pub const DECLINE_COOLDOWN_MS: i64 = 2 * 60 * 1000;
const LOADING_HUNT_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateState {
    Matching,
    InProgress,
}

impl CandidateState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matching => "MATCHING",
            Self::InProgress => "IN_PROGRESS",
        }
    }
}

/// The whole party matchmade onto one server (`matchmaking.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub region: Option<crate::matchmaking::region::Region>,
    pub candidate_id: String,
    pub state: CandidateState,
    pub game_mode: String,
    pub hunt_id: String,
    /// Who was taken along, in party order.
    pub member_ids: Vec<String>,
    /// Members already told to travel, with when they first were.
    pub served: HashMap<String, i64>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub session_id: Option<String>,
    pub allocated_at: Option<i64>,
    pub launch_deadline: Option<i64>,
    /// Renewed only by active matchmaking status polls.
    pub capacity_deadline: Option<i64>,
    pub created_at: i64,
    pub last_served_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Party {
    pub party_id: String,
    pub leader_id: String,
    /// Join order, at most [`MAX_PARTY_SIZE`].
    pub members: Vec<String>,
    pub created_at: i64,
    /// The candidates by id: the current one and the most recent one that got a server are often
    /// the same (Node shared one object between the two fields).
    candidates: HashMap<String, Candidate>,
    candidate: Option<String>,
    last_candidate: Option<String>,
}

impl Party {
    pub fn candidate(&self) -> Option<&Candidate> {
        self.candidate.as_ref().and_then(|id| self.candidates.get(id))
    }

    pub fn candidate_mut(&mut self) -> Option<&mut Candidate> {
        self.candidate.as_ref().and_then(|id| self.candidates.get_mut(id))
    }

    pub fn last_candidate(&self) -> Option<&Candidate> {
        self.last_candidate.as_ref().and_then(|id| self.candidates.get(id))
    }

    pub fn set_candidate(&mut self, candidate: Candidate) {
        self.candidate = Some(candidate.candidate_id.clone());
        self.candidates.insert(candidate.candidate_id.clone(), candidate);
        self.forget_unused();
    }

    pub fn clear_candidate(&mut self) {
        self.candidate = None;
        self.forget_unused();
    }

    /// The current candidate becomes the last one that got a server.
    pub fn keep_as_last(&mut self) {
        self.last_candidate = self.candidate.clone();
        self.forget_unused();
    }

    fn forget_unused(&mut self) {
        let (current, last) = (self.candidate.clone(), self.last_candidate.clone());
        self.candidates
            .retain(|id, _| Some(id) == current.as_ref() || Some(id) == last.as_ref());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub party_id: String,
    pub sending_player_id: String,
    pub recipient_player_id: String,
    pub sending_display_name: String,
    pub created_at: i64,
}

/// What a party route answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Reply {
    fn empty(status: u16) -> Self {
        Self {
            status,
            body: json!({}),
        }
    }
}

/// How replies show names (`ACCOUNT_DISPLAY_NAME`) and parties of one (`PARTY_SOLO_STUB`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub display_names: bool,
    pub solo_stub: bool,
}

#[derive(Debug, Default)]
pub struct State {
    parties: HashMap<String, Party>,
    party_by_user: HashMap<String, String>,
    invites_by_recipient: HashMap<String, Vec<Invite>>,
    /// Last time each player was heard from: party polls, invite polls, party actions, heartbeats.
    last_seen: HashMap<String, i64>,
    /// What was last logged at info level per player, so the 10 s polls only log changes.
    last_poll_logged: HashMap<String, String>,
    last_invite_count_logged: HashMap<String, usize>,
    /// Times of each sender's recent invites (a sliding window).
    invites_sent_by: HashMap<String, Vec<i64>>,
    /// "<sender>|<recipient>" -> when the recipient declined that sender's invite.
    declined_by: HashMap<String, i64>,
    last_global_sweep: i64,
    /// Matchmaking's state, under the same lock: its steps read and change parties as they go.
    pub mm: crate::matchmaking::State,
}

/// The parties, shared by the party and matchmaking routes.
#[derive(Debug, Default)]
pub struct Parties(Mutex<State>);

impl Parties {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn seconds(ms: Option<i64>) -> String {
    match ms {
        None => "ever".into(),
        Some(ms) => format!("{} s", (ms as f64 / 1000.0).round() as i64),
    }
}

fn shown(id: &str) -> String {
    id.chars().take(64).collect()
}

fn id_shape(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|id| is_account_id_shape(id))
}

impl State {
    /// A new block: the party invites either player sent the other go (`DropPartyInvitesBetween`).
    pub fn drop_invites_between(&mut self, a: &str, b: &str) {
        let mut removed = 0;
        for (recipient, sender) in [(a, b), (b, a)] {
            let list = self.invites_by_recipient.remove(recipient).unwrap_or_default();
            let before = list.len();
            let keep: Vec<Invite> = list.into_iter().filter(|i| i.sending_player_id != sender).collect();
            removed += before - keep.len();
            if !keep.is_empty() {
                self.invites_by_recipient.insert(recipient.to_owned(), keep);
            }
        }
        if removed > 0 {
            tracing::info!("party: {removed} invite(s) between {a} and {b} removed by a block");
        }
    }

    /// `TouchPlayer`.
    pub fn touch(&mut self, user: &str, now: i64) {
        if is_account_id_shape(user) {
            self.last_seen.insert(user.to_owned(), now);
        }
    }

    fn seen_ago(&self, user: &str, now: i64) -> Option<i64> {
        self.last_seen.get(user).map(|seen| now - seen)
    }

    /// `SeenWithinMs`: heard from within `ms` (the game server's guild create checks the leader).
    pub fn seen_within(&self, user: &str, ms: i64, now: i64) -> bool {
        self.seen_ago(user, now).is_some_and(|ago| ago <= ms)
    }

    pub fn party_id_of(&self, user: &str) -> Option<String> {
        self.party_by_user
            .get(user)
            .filter(|id| self.parties.contains_key(*id))
            .cloned()
    }

    pub fn party(&self, id: &str) -> Option<&Party> {
        self.parties.get(id)
    }

    pub fn party_mut(&mut self, id: &str) -> Option<&mut Party> {
        self.parties.get_mut(id)
    }

    fn create_solo(&mut self, user: &str, now: i64) -> String {
        let id = crate::crypto::random_uuid();
        self.parties.insert(
            id.clone(),
            Party {
                party_id: id.clone(),
                leader_id: user.to_owned(),
                members: vec![user.to_owned()],
                created_at: now,
                candidates: HashMap::new(),
                candidate: None,
                last_candidate: None,
            },
        );
        self.party_by_user.insert(user.to_owned(), id.clone());
        id
    }

    fn party_or_solo(&mut self, user: &str, now: i64) -> String {
        match self.party_id_of(user) {
            Some(id) => id,
            None => self.create_solo(user, now),
        }
    }

    /// `RemoveMember`.
    fn remove_member(&mut self, party_id: &str, user: &str, why: &str) {
        let Some(party) = self.parties.get_mut(party_id) else {
            return;
        };
        if !party.members.iter().any(|m| m == user) {
            return;
        }
        party.members.retain(|m| m != user);
        if self.party_by_user.get(user).map(String::as_str) == Some(party_id) {
            self.party_by_user.remove(user);
        }
        let current = party.candidate.clone();
        for candidate in party.candidates.values_mut() {
            if candidate.member_ids.iter().any(|m| m == user) {
                candidate.member_ids.retain(|m| m != user);
                // Matchmaking forgets the entry of a member who leaves before the server is up
                if Some(&candidate.candidate_id) == current.as_ref() {
                    self.mm.candidate_left(user, &candidate.candidate_id);
                }
            }
        }
        if party.members.is_empty() {
            self.parties.remove(party_id);
            return;
        }
        if party.leader_id == user {
            party.leader_id = party.members[0].clone();
            tracing::info!(
                "party: {} is now leader of P={} ({why})",
                party.leader_id,
                party.party_id
            );
        }
        if party.members.len() < 2 {
            party.clear_candidate();
        }
    }

    /// `ExpireCandidate`.
    fn expire_candidate(&mut self, party_id: &str, now: i64) {
        let Some(party) = self.parties.get_mut(party_id) else {
            return;
        };
        let Some(candidate) = party.candidate() else {
            return;
        };
        let remaining: Vec<&String> = candidate
            .member_ids
            .iter()
            .filter(|m| party.members.contains(m))
            .collect();
        let all_served = remaining.iter().all(|m| candidate.served.contains_key(*m));
        let deadline = candidate.launch_deadline.or(candidate.capacity_deadline);
        let why = if remaining.is_empty() {
            Some("no member left in it")
        } else if all_served
            && candidate
                .last_served_at
                .is_some_and(|at| now - at > CANDIDATE_LINGER_MS)
        {
            Some("every member was sent to the server")
        } else if match deadline {
            Some(deadline) if candidate.state == CandidateState::Matching => now > deadline,
            _ => now - candidate.allocated_at.unwrap_or(candidate.created_at) > CANDIDATE_MAX_AGE_MS,
        } {
            Some("too old")
        } else {
            None
        };
        if let Some(why) = why {
            let id = candidate.candidate_id.clone();
            party.clear_candidate();
            tracing::info!("party: candidate {id} of P={party_id} cleared: {why}");
        }
    }

    /// `IsLoadingHunt`: sent to the server within the last five minutes.
    fn is_loading_hunt(party: &Party, member: &str, now: i64) -> bool {
        party.candidate().or_else(|| party.last_candidate()).is_some_and(|c| {
            c.served.contains_key(member) && c.last_served_at.is_some_and(|at| now - at < LOADING_HUNT_MS)
        })
    }

    /// `Refresh`: drops members of a party of 2+ nobody has heard from (never the caller, and never
    /// the last one), then expires the candidate.
    pub fn refresh(&mut self, party_id: &str, caller: Option<&str>, now: i64) {
        let members = self
            .parties
            .get(party_id)
            .map(|p| p.members.clone())
            .unwrap_or_default();
        if members.len() >= 2 {
            for member in members {
                let Some(party) = self.parties.get(party_id) else {
                    break;
                };
                if party.members.len() < 2 {
                    break;
                }
                if Some(member.as_str()) == caller {
                    continue;
                }
                let ago = self.seen_ago(&member, now);
                if ago.is_none_or(|ago| ago > MEMBER_TIMEOUT_MS) && !Self::is_loading_hunt(party, &member, now) {
                    tracing::info!(
                        "party: swept {member} from P={party_id}: not seen for {} (by={})",
                        seconds(ago),
                        caller.unwrap_or("sweep")
                    );
                    self.remove_member(party_id, &member, "the leader was not seen");
                }
            }
        }
        self.expire_candidate(party_id, now);
    }

    fn is_invite_live(&self, invite: &Invite, now: i64) -> bool {
        now - invite.created_at <= INVITE_TTL_MS
            && self.parties.get(&invite.party_id).is_some_and(|party| {
                party.members.contains(&invite.sending_player_id)
                    && !party.members.contains(&invite.recipient_player_id)
            })
    }

    /// `LiveInvitesFor`: the recipient's live invites; one between players who blocked each other is
    /// dropped here too.
    fn live_invites_for(&mut self, conn: &mut Conn, user: &str, now: i64) -> QueryResult<Vec<Invite>> {
        let list = self.invites_by_recipient.remove(user).unwrap_or_default();
        let mut live = Vec::with_capacity(list.len());
        for invite in list {
            if self.is_invite_live(&invite, now) && !blocked_either_way(conn, &invite.sending_player_id, user)? {
                live.push(invite);
            }
        }
        if !live.is_empty() {
            self.invites_by_recipient.insert(user.to_owned(), live.clone());
        }
        Ok(live)
    }

    /// `GlobalSweep`, at most once a minute from any party call: expire invites, sweep parties,
    /// forget idle players.
    pub(crate) fn global_sweep(&mut self, conn: &mut Conn, now: i64) -> QueryResult<()> {
        if now - self.last_global_sweep < 60 * 1000 {
            return Ok(());
        }
        self.last_global_sweep = now;
        let ids: Vec<String> = self.parties.keys().cloned().collect();
        for id in ids {
            let Some(party) = self.parties.get(&id) else {
                continue;
            };
            if party.members.len() == 1 {
                let member = party.members[0].clone();
                if self.seen_ago(&member, now).is_none_or(|ago| ago > IDLE_PARTY_MS) {
                    self.parties.remove(&id);
                    self.party_by_user.remove(&member);
                }
            } else {
                self.refresh(&id, None, now);
            }
        }
        let recipients: Vec<String> = self.invites_by_recipient.keys().cloned().collect();
        for recipient in recipients {
            self.live_invites_for(conn, &recipient, now)?;
        }
        let idle: Vec<String> = self
            .last_seen
            .iter()
            .filter(|(user, seen)| {
                now - **seen > IDLE_PARTY_MS
                    && !self.party_by_user.contains_key(*user)
                    && !self.invites_by_recipient.contains_key(*user)
            })
            .map(|(user, _)| user.clone())
            .collect();
        for user in idle {
            self.last_seen.remove(&user);
            self.last_poll_logged.remove(&user);
            self.last_invite_count_logged.remove(&user);
        }
        let senders: Vec<String> = self.invites_sent_by.keys().cloned().collect();
        for sender in senders {
            self.recent_invites_by(&sender, now);
        }
        self.declined_by.retain(|_, at| now - *at < DECLINE_COOLDOWN_MS);
        Ok(())
    }

    /// A sender's invites in the last [`INVITE_WINDOW_MS`] (older ones are forgotten).
    fn recent_invites_by(&mut self, sender: &str, now: i64) -> Vec<i64> {
        let recent: Vec<i64> = self
            .invites_sent_by
            .get(sender)
            .map(|times| times.iter().copied().filter(|at| now - at < INVITE_WINDOW_MS).collect())
            .unwrap_or_default();
        if recent.is_empty() {
            self.invites_sent_by.remove(sender);
        } else {
            self.invites_sent_by.insert(sender.to_owned(), recent.clone());
        }
        recent
    }

    // ---- Replies ----

    /// `PartyReply` (POST /party, and the accept and status replies). A party of one keeps the old
    /// stub's candidate values; a real party sends its real candidate or nulls, otherwise the other
    /// members would follow the fake candidate.
    pub fn party_reply(&self, conn: &mut Conn, settings: Settings, party_id: &str) -> QueryResult<Value> {
        let Some(party) = self.parties.get(party_id) else {
            return Ok(json!({}));
        };
        let mut names = Vec::with_capacity(party.members.len());
        for member in &party.members {
            names.push(if settings.display_names {
                json!(username_for(conn, member)?.unwrap_or_default())
            } else {
                json!({})
            });
        }
        if party.members.len() == 1 && settings.solo_stub {
            return Ok(json!({
                "candidateId": "CANDIDATE_ID_LOL",
                "candidateState": "QUEUED_FOR_START",
                "gauntletLevel": null,
                "leaderPlayerId": party.leader_id,
                "partyId": party.party_id,
                "playerHuntId": null,
                "playerStates": [{
                    "consoleSessionId": null,
                    "displayName": names[0],
                    "isMemberOfCandidate": true,
                    "platform": "win",
                    "playerId": party.members[0]
                }]
            }));
        }
        let candidate = party.candidate();
        let states: Vec<Value> = party
            .members
            .iter()
            .zip(names)
            .map(|(member, name)| {
                json!({
                    "playerId": member,
                    "isMemberOfCandidate": candidate.is_some_and(|c| c.member_ids.contains(member)),
                    "platform": "win",
                    "displayName": name,
                    "consoleSessionId": null
                })
            })
            .collect();
        Ok(json!({
            "partyId": party.party_id,
            "leaderPlayerId": party.leader_id,
            "playerIds": party.members,
            "playerStates": states,
            "candidateState": candidate.map(|c| c.state.as_str()),
            "candidateId": candidate.map(|c| c.candidate_id.as_str()),
            "playerHuntId": candidate.map(|c| c.hunt_id.as_str()),
            "gauntletLevel": null
        }))
    }

    fn describe(&self, party_id: &str) -> String {
        let Some(party) = self.parties.get(party_id) else {
            return format!("P={party_id} size=0");
        };
        let mut line = format!(
            "P={} size={} leader={}",
            party.party_id,
            party.members.len(),
            party.leader_id
        );
        if party.members.len() > 1 {
            line.push_str(&format!(" members={}", party.members.join(",")));
        }
        if let Some(c) = party.candidate() {
            line.push_str(&format!(
                " candidate={} {} hunt={}",
                c.candidate_id,
                c.state.as_str(),
                c.hunt_id
            ));
        }
        line
    }

    fn log_poll(&mut self, user: &str, party_id: &str) {
        let line = format!("party: poll by={user} {}", self.describe(party_id));
        if self.last_poll_logged.get(user) != Some(&line) {
            tracing::info!("{line}");
            self.last_poll_logged.insert(user.to_owned(), line);
        } else {
            tracing::debug!("{line}");
        }
    }

    // ---- The client's party routes ----

    /// `PollParty`, POST /party: the caller's party, a new party of one if they have none.
    pub fn poll(&mut self, conn: &mut Conn, settings: Settings, user: &str, now: i64) -> QueryResult<Value> {
        self.global_sweep(conn, now)?;
        self.touch(user, now);
        let id = self.party_or_solo(user, now);
        self.refresh(&id, Some(user), now);
        self.log_poll(user, &id);
        self.party_reply(conn, settings, &id)
    }

    /// `ListPartyInvites`, GET /party/invites.
    pub fn list_invites(&mut self, conn: &mut Conn, user: &str, now: i64) -> QueryResult<Value> {
        self.global_sweep(conn, now)?;
        self.touch(user, now);
        let live = self.live_invites_for(conn, user, now)?;
        let line = format!("party: invites for {user} -> {}", live.len());
        if self.last_invite_count_logged.get(user) != Some(&live.len()) {
            self.last_invite_count_logged.insert(user.to_owned(), live.len());
            if live.is_empty() {
                tracing::info!("{line}");
            } else {
                let shown: Vec<String> = live
                    .iter()
                    .map(|i| format!("P={} from={}", i.party_id, i.sending_player_id))
                    .collect();
                tracing::info!("{line} ({})", shown.join("; "));
            }
        } else {
            tracing::debug!("{line}");
        }
        Ok(json!({"invitations": live.iter().map(invite_reply).collect::<Vec<_>>()}))
    }

    /// `InviteToParty`, PUT /party/invite. The client only reads the status.
    pub fn invite(
        &mut self,
        conn: &mut Conn,
        caller: &str,
        recipient: Option<&Value>,
        requested_party: Option<&Value>,
        now: i64,
    ) -> QueryResult<Reply> {
        self.global_sweep(conn, now)?;
        self.touch(caller, now);
        let recipient_id = id_shape(recipient);
        let refuse = |status: u16, why: String| {
            tracing::info!(
                "party: invite by={caller} to={} refused {status}: {why}",
                recipient_id.unwrap_or("<not an account id>")
            );
            Ok(Reply::empty(status))
        };
        let Some(recipient) = recipient_id else {
            return refuse(404, "no recipient".into());
        };
        if recipient == caller {
            return refuse(409, "cannot invite yourself".into());
        }
        if username_for(conn, recipient)?.is_none() {
            return refuse(404, "no such account".into());
        }
        let id = self.party_or_solo(caller, now);
        self.refresh(&id, Some(caller), now);
        if let Some(Value::String(requested)) = requested_party
            && !requested.is_empty()
            && *requested != id
        {
            tracing::info!(
                "party: invite by={caller} names P={} but the caller's party is P={id}; using the caller's own",
                shown(requested)
            );
        }
        let Some(party) = self.parties.get(&id) else {
            return refuse(404, "no party".into());
        };
        if party.leader_id != caller {
            return refuse(403, format!("not the leader of P={id}"));
        }
        if party.members.len() >= MAX_PARTY_SIZE {
            return refuse(409, format!("P={id} is full"));
        }
        if party.members.iter().any(|m| m == recipient) {
            return refuse(409, "already a member".into());
        }
        if blocked_either_way(conn, caller, recipient)? {
            return refuse(403, "blocked".into());
        }
        if let Some(at) = self.declined_by.get(&format!("{caller}|{recipient}"))
            && now - at < DECLINE_COOLDOWN_MS
        {
            return refuse(
                409,
                format!(
                    "{recipient} declined an invite from {caller} {} ago",
                    seconds(Some(now - at))
                ),
            );
        }
        let mut pending = self.live_invites_for(conn, recipient, now)?;
        if pending.iter().any(|i| i.party_id == id) {
            return refuse(409, "already invited".into());
        }
        let outstanding = self
            .invites_by_recipient
            .values()
            .flatten()
            .filter(|i| i.party_id == id && self.is_invite_live(i, now))
            .count();
        if outstanding >= MAX_INVITES_PER_PARTY {
            return refuse(409, "too many open invites".into());
        }
        let mut recent = self.recent_invites_by(caller, now);
        if recent.len() >= MAX_INVITES_PER_SENDER {
            return refuse(
                409,
                format!(
                    "{} invites sent in the last {}",
                    recent.len(),
                    seconds(Some(INVITE_WINDOW_MS))
                ),
            );
        }
        recent.push(now);
        self.invites_sent_by.insert(caller.to_owned(), recent);
        pending.push(Invite {
            party_id: id.clone(),
            sending_player_id: caller.to_owned(),
            recipient_player_id: recipient.to_owned(),
            sending_display_name: username_for(conn, caller)?.unwrap_or_default(),
            created_at: now,
        });
        let keep = pending.len().saturating_sub(MAX_INVITES_PER_RECIPIENT);
        self.invites_by_recipient
            .insert(recipient.to_owned(), pending.split_off(keep));
        tracing::info!("party: invite P={id} from={caller} to={recipient}");
        Ok(Reply::empty(200))
    }

    /// `AcceptPartyInvite`, PUT /party/invite/accept/:inviteId. The client puts the invite's party
    /// id in the URL; in case it's the sender's id, that is tried second. The answer is the joined
    /// party, also for an accept repeated after it succeeded.
    pub fn accept(
        &mut self,
        conn: &mut Conn,
        settings: Settings,
        caller: &str,
        invite_id: &str,
        now: i64,
    ) -> QueryResult<Reply> {
        self.global_sweep(conn, now)?;
        self.touch(caller, now);
        let live = self.live_invites_for(conn, caller, now)?;
        let (matched, invite) = match live.iter().find(|i| i.party_id == invite_id) {
            Some(invite) => ("partyId", Some(invite)),
            None => (
                "sendingPlayerId",
                live.iter().find(|i| i.sending_player_id == invite_id),
            ),
        };
        let shown_id = shown(invite_id);
        let Some(invite) = invite.cloned() else {
            // A repeated accept: the client can send the accept again while it applies the party
            // the first one joined. When the caller already sits in a party of two or more that
            // the id names, the answer is that party instead of a 404.
            if let Some(current) = self.party_id_of(caller)
                && !invite_id.is_empty()
            {
                self.refresh(&current, Some(caller), now);
                if let Some(party) = self.parties.get(&current)
                    && party.members.len() > 1
                    && (party.party_id == invite_id || party.members.iter().any(|m| m != caller && m == invite_id))
                {
                    tracing::info!(
                        "party: accept by {caller} id={shown_id}: already in P={current} size={}; answering that party (a repeated accept)",
                        party.members.len()
                    );
                    let body = self.party_reply(conn, settings, &current)?;
                    return Ok(Reply { status: 200, body });
                }
            }
            tracing::info!(
                "party: accept by {caller} id={shown_id}: no such invite ({} pending)",
                live.len()
            );
            return Ok(Reply::empty(404));
        };
        let target = invite.party_id.clone();
        if !self.parties.contains_key(&target) {
            tracing::info!("party: accept by {caller} matched={matched} P={target}: the party is gone");
            return Ok(Reply::empty(404));
        }
        self.refresh(&target, None, now);
        let drop_target_invites = |state: &mut State| {
            let remaining: Vec<Invite> = state
                .invites_by_recipient
                .remove(caller)
                .unwrap_or_default()
                .into_iter()
                .filter(|i| i.party_id != target)
                .collect();
            if !remaining.is_empty() {
                state.invites_by_recipient.insert(caller.to_owned(), remaining);
            }
        };
        let members = self.parties.get(&target).map(|p| p.members.clone()).unwrap_or_default();
        if members.iter().any(|m| m == caller) {
            drop_target_invites(self);
            let body = self.party_reply(conn, settings, &target)?;
            return Ok(Reply { status: 200, body });
        }
        if members.is_empty() {
            tracing::info!("party: accept by {caller} matched={matched} P={target}: the party is gone");
            return Ok(Reply::empty(404));
        }
        if members.len() >= MAX_PARTY_SIZE {
            tracing::info!("party: accept by {caller} matched={matched} P={target} refused 409: full");
            return Ok(Reply::empty(409));
        }
        let old = self.party_id_of(caller);
        if let Some(old) = &old {
            self.remove_member(old, caller, "the leader joined another party");
        }
        let size = match self.parties.get_mut(&target) {
            Some(party) => {
                party.members.push(caller.to_owned());
                party.members.len()
            }
            None => 0,
        };
        self.party_by_user.insert(caller.to_owned(), target.clone());
        drop_target_invites(self);
        tracing::info!(
            "party: accept by {caller} matched={matched} P={target} size={size}{}",
            match &old {
                Some(old) if *old != target => format!(" (left P={old})"),
                _ => String::new(),
            }
        );
        let body = self.party_reply(conn, settings, &target)?;
        Ok(Reply { status: 200, body })
    }

    /// `DeclinePartyInvites`, DELETE /party/invite. Declining: the recipient is the caller, and the
    /// sender can't invite the caller again for [`DECLINE_COOLDOWN_MS`]. A sender naming someone
    /// else as the recipient withdraws the invites it sent them. Always 200 {}.
    pub fn decline(&mut self, caller: &str, body: Option<&Value>, now: i64) -> Reply {
        self.touch(caller, now);
        let field = |name: &str| body.and_then(|b| b.as_object()).and_then(|b| b.get(name));
        let ids: Vec<String> = [field("partyId"), field("sendingPlayerId")]
            .into_iter()
            .filter_map(|id| {
                id.and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
            })
            .collect();
        let named = |invite: &Invite| ids.contains(&invite.party_id) || ids.contains(&invite.sending_player_id);
        match field("recipientPlayerId") {
            recipient if recipient.is_none() || recipient.and_then(Value::as_str) == Some(caller) => {
                let list = self.invites_by_recipient.remove(caller).unwrap_or_default();
                let before = list.len();
                let (dropped, keep): (Vec<Invite>, Vec<Invite>) = list.into_iter().partition(|i| named(i));
                for invite in &dropped {
                    self.declined_by
                        .insert(format!("{}|{caller}", invite.sending_player_id), now);
                }
                let removed = before - keep.len();
                if !keep.is_empty() {
                    self.invites_by_recipient.insert(caller.to_owned(), keep);
                }
                let shown_ids: Vec<String> = ids.iter().map(|id| shown(id)).collect();
                tracing::info!(
                    "party: decline by={caller} ids={} removed={removed}",
                    if shown_ids.is_empty() {
                        "<none>".to_owned()
                    } else {
                        shown_ids.join("|")
                    }
                );
            }
            recipient => {
                if let Some(recipient) = id_shape(recipient) {
                    let list = self.invites_by_recipient.remove(recipient).unwrap_or_default();
                    let before = list.len();
                    let keep: Vec<Invite> = list
                        .into_iter()
                        .filter(|i| !(i.sending_player_id == caller && (ids.is_empty() || named(i))))
                        .collect();
                    let removed = before - keep.len();
                    if !keep.is_empty() {
                        self.invites_by_recipient.insert(recipient.to_owned(), keep);
                    }
                    tracing::info!("party: withdraw by={caller} to={recipient} removed={removed}");
                }
            }
        }
        Reply::empty(200)
    }

    /// `LeaveParty`, DELETE /party/member. The next member in join order leads if the leader left.
    pub fn leave(&mut self, caller: &str, now: i64) -> Reply {
        self.touch(caller, now);
        let Some(id) = self.party_id_of(caller) else {
            tracing::debug!("party: leave by={caller}: in no party");
            return Reply::empty(200);
        };
        let size = self.parties.get(&id).map_or(0, |p| p.members.len());
        self.remove_member(&id, caller, "the leader left");
        let (after, leader) = self
            .parties
            .get(&id)
            .map_or((0, String::new()), |p| (p.members.len(), p.leader_id.clone()));
        let line = format!(
            "party: leave by={caller} P={id} size={size}->{after}{}",
            if after > 0 {
                format!(" leader={leader}")
            } else {
                String::new()
            }
        );
        if size > 1 {
            tracing::info!("{line}");
        } else {
            tracing::debug!("{line}");
        }
        Reply::empty(200)
    }

    /// The caller's party, when it holds `member`; 404 or 403 refusals otherwise.
    fn led_party_with(&self, caller: &str, member: &str, what: &str) -> Result<String, Reply> {
        let Some(party) = self
            .party_id_of(caller)
            .and_then(|id| self.parties.get(&id))
            .filter(|p| p.members.iter().any(|m| m == member))
        else {
            tracing::info!(
                "party: {what} by={caller} member={} refused 404: not in the caller's party",
                shown(member)
            );
            return Err(Reply::empty(404));
        };
        if party.leader_id != caller {
            tracing::info!(
                "party: {what} by={caller} member={member} refused 403: not the leader of P={}",
                party.party_id
            );
            return Err(Reply::empty(403));
        }
        Ok(party.party_id.clone())
    }

    /// `KickPartyMember`, DELETE /party/member/:memberId (anyone may name themself).
    pub fn kick(&mut self, caller: &str, member: &str, now: i64) -> Reply {
        if member == caller {
            return self.leave(caller, now);
        }
        self.touch(caller, now);
        let id = match self.led_party_with(caller, member, "kick") {
            Ok(id) => id,
            Err(refused) => return refused,
        };
        self.remove_member(&id, member, "kicked");
        tracing::info!("party: kick by={caller} member={member} {}", self.describe(&id));
        Reply::empty(200)
    }

    /// `PromotePartyMember`, PUT /party/member/promote/:memberId.
    pub fn promote(&mut self, caller: &str, member: &str, now: i64) -> Reply {
        self.touch(caller, now);
        let id = match self.led_party_with(caller, member, "promote") {
            Ok(id) => id,
            Err(refused) => return refused,
        };
        if let Some(party) = self.parties.get_mut(&id) {
            party.leader_id = member.to_owned();
        }
        tracing::info!("party: promote by={caller} member={member} {}", self.describe(&id));
        Reply::empty(200)
    }

    /// `EvictPartyLeader`, DELETE /party/leader/:leaderId: honoured only when the leader wasn't
    /// heard from for [`MEMBER_TIMEOUT_MS`] either. Always 200 {}.
    pub fn evict_leader(&mut self, caller: &str, leader: &str, now: i64) -> Reply {
        self.touch(caller, now);
        let Some(id) = self
            .party_id_of(caller)
            .filter(|id| self.parties.get(id).is_some_and(|p| p.leader_id == leader) && leader != caller)
        else {
            tracing::info!(
                "party: leader eviction of {} by={caller} ignored: not the caller's party leader",
                shown(leader)
            );
            return Reply::empty(200);
        };
        let ago = self.seen_ago(leader, now);
        let loading = self
            .parties
            .get(&id)
            .is_some_and(|p| Self::is_loading_hunt(p, leader, now));
        if ago.is_none_or(|ago| ago > MEMBER_TIMEOUT_MS) && !loading {
            self.remove_member(&id, leader, "the leader was not seen");
            tracing::info!(
                "party: leader {leader} removed by={caller}: not seen for {}; {}",
                seconds(ago),
                self.describe(&id)
            );
        } else {
            tracing::info!(
                "party: leader eviction of {leader} by={caller} ignored: seen {} ago",
                seconds(ago)
            );
        }
        Reply::empty(200)
    }

    /// `PartiesOfPlayers`, POST /party/status: the parties those players are in, and the caller's
    /// own invitations (never anyone else's).
    pub fn status(
        &mut self,
        conn: &mut Conn,
        settings: Settings,
        caller: &str,
        players: Option<&Value>,
        now: i64,
    ) -> QueryResult<Reply> {
        self.global_sweep(conn, now)?;
        self.touch(caller, now);
        let mut ids: Vec<&str> = Vec::new();
        if let Some(Value::Array(list)) = players {
            for id in list.iter().filter_map(|v| id_shape(Some(v))) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids.truncate(MAX_STATUS_PLAYER_IDS);
        let mut found: Vec<String> = Vec::new();
        for id in &ids {
            if let Some(party) = self.party_id_of(id) {
                self.refresh(&party, (*id == caller).then_some(caller), now);
                if self.parties.get(&party).is_some_and(|p| !p.members.is_empty()) && !found.contains(&party) {
                    found.push(party);
                }
            }
        }
        let mut parties = Vec::with_capacity(found.len());
        for id in &found {
            parties.push(self.party_reply(conn, settings, id)?);
        }
        tracing::info!(
            "party: status by={caller} asked={} parties={}",
            ids.len(),
            parties.len()
        );
        let invitations: Vec<Value> = self
            .live_invites_for(conn, caller, now)?
            .iter()
            .map(invite_reply)
            .collect();
        Ok(Reply {
            status: 200,
            body: json!({"parties": parties, "invitations": invitations}),
        })
    }
}

// ---- For matchmaking ----

impl State {
    /// `PartyForMatchmaking`: the caller's party, refreshed, before a join is matched.
    pub fn party_for_matchmaking(&mut self, conn: &mut Conn, user: &str, now: i64) -> QueryResult<Option<String>> {
        self.global_sweep(conn, now)?;
        self.touch(user, now);
        let id = self.party_id_of(user);
        if let Some(id) = &id {
            self.refresh(id, Some(user), now);
        }
        Ok(id)
    }

    pub fn set_candidate(&mut self, party_id: &str, candidate: Candidate) {
        if let Some(party) = self.parties.get_mut(party_id) {
            party.set_candidate(candidate);
        }
    }

    /// `ClearPartyCandidate`.
    pub fn clear_candidate_if(&mut self, party_id: &str, candidate_id: &str, why: &str) {
        if let Some(party) = self.parties.get_mut(party_id)
            && party.candidate().is_some_and(|c| c.candidate_id == candidate_id)
        {
            party.clear_candidate();
            tracing::info!("party: candidate {candidate_id} of P={party_id} cleared: {why}");
        }
    }

    /// The party's last candidate is forgotten when it is this one.
    pub fn drop_last_candidate_if(&mut self, party_id: &str, candidate_id: &str) {
        if let Some(party) = self.parties.get_mut(party_id)
            && party.last_candidate().is_some_and(|c| c.candidate_id == candidate_id)
        {
            party.last_candidate = None;
            party.forget_unused();
        }
    }

    /// The last candidate no longer counts `user` in.
    pub fn leave_last_candidate(&mut self, user: &str, candidate_id: &str) {
        let Some(id) = self.party_id_of(user) else {
            return;
        };
        if let Some(party) = self.parties.get_mut(&id)
            && let Some(last) = party.last_candidate.clone()
            && last == candidate_id
            && let Some(candidate) = party.candidates.get_mut(&last)
        {
            candidate.member_ids.retain(|m| m != user);
        }
    }

    /// `MarkPartyCandidateReady`: members' polls now show IN_PROGRESS.
    pub fn mark_candidate_ready(&mut self, party_id: &str, candidate_id: &str, host: &str, port: u16) {
        let Some(party) = self.parties.get_mut(party_id) else {
            return;
        };
        if let Some(candidate) = party.candidate_mut()
            && candidate.candidate_id == candidate_id
        {
            candidate.state = CandidateState::InProgress;
            candidate.capacity_deadline = None;
            candidate.host = Some(host.to_owned());
            candidate.port = Some(port);
            party.keep_as_last();
        }
    }

    /// `MarkPartyCandidateServed`: a member was told to travel to the candidate's server.
    pub fn mark_candidate_served(&mut self, user: &str, candidate_id: &str, first_sent_at: i64, now: i64) {
        let Some(id) = self.party_id_of(user) else {
            return;
        };
        if let Some(candidate) = self.parties.get_mut(&id).and_then(Party::candidate_mut)
            && candidate.candidate_id == candidate_id
            && !candidate.served.contains_key(user)
        {
            candidate.served.insert(user.to_owned(), first_sent_at);
            candidate.last_served_at = Some(now);
        }
    }

    /// `RemoveFromPartyCandidate`: a member leaves the candidate but stays in the party.
    pub fn remove_from_candidate(&mut self, user: &str, candidate_id: &str, now: i64) {
        let Some(id) = self.party_id_of(user) else {
            return;
        };
        if let Some(candidate) = self.parties.get_mut(&id).and_then(Party::candidate_mut)
            && candidate.candidate_id == candidate_id
        {
            candidate.member_ids.retain(|m| m != user);
            self.expire_candidate(&id, now);
        }
    }
}

fn invite_reply(invite: &Invite) -> Value {
    json!({
        "recipientPlayerId": invite.recipient_player_id,
        "sendingPlayerId": invite.sending_player_id,
        "partyId": invite.party_id,
        "sendingPlatform": "win",
        "sendingDisplayName": invite.sending_display_name
    })
}
