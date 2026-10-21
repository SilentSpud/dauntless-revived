//! Friends list and blocklist (`controllers/friends.ts`), stored so a request to an offline player
//! survives restarts. The client reads them over the Epic-style friends routes (`routes/friends.rs`).
//!
//! One friendships row per pair, ids sorted: status PENDING (requesterId asked) or ACCEPTED. A block
//! removes any friendship between the two, the guild invites either one sent the other and the
//! pending Slayer Link invites between them, and stops requests in both directions. An unfriend also
//! cancels the Slayer Link invites.
//!
//! Limits on new requests: at most [`MAX_PENDING_OUTGOING`] unanswered requests an account has sent,
//! and at most [`MAX_REQUESTS_PER_WINDOW`] new requests per [`REQUEST_WINDOW_MS`] (in memory;
//! accepting a request the other player sent never counts). Both answer 409, which the client shows
//! as a failure toast.

use std::collections::HashMap;
use std::sync::Mutex;

use diesel::dsl::count_star;
use diesel::prelude::*;
use serde_json::{Value, json};

use crate::schema::{blocks, friendships, guildinvites, slayerlinkinvites, users};
use crate::time::{iso, now_ms};

pub type Conn = store::Connection;

pub const MAX_FRIENDSHIPS_PER_ACCOUNT: i64 = 200;
pub const MAX_BLOCKS_PER_ACCOUNT: i64 = 200;
pub const MAX_PENDING_OUTGOING: i64 = 50;
pub const MAX_REQUESTS_PER_WINDOW: usize = 20;
pub const REQUEST_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Times of each account's recent new requests (a sliding window).
#[derive(Debug, Default)]
pub struct RecentRequests(Mutex<HashMap<String, Vec<i64>>>);

impl RecentRequests {
    /// The account's requests still in the window; accounts that stopped sending are forgotten once
    /// the map grows.
    fn recent(map: &mut HashMap<String, Vec<i64>>, account: &str, now: i64) -> Vec<i64> {
        let recent: Vec<i64> = map
            .get(account)
            .map(|times| {
                times
                    .iter()
                    .copied()
                    .filter(|at| now - at < REQUEST_WINDOW_MS)
                    .collect()
            })
            .unwrap_or_default();
        if recent.is_empty() {
            map.remove(account);
        } else {
            map.insert(account.to_owned(), recent.clone());
        }
        if map.len() > 1000 {
            map.retain(|_, times| times.iter().any(|at| now - at < REQUEST_WINDOW_MS));
        }
        recent
    }
}

/// A friendship that began or ended (`FriendshipEvent`), for chat presence. Sent once the change is
/// stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendshipEvent {
    Accepted {
        requester: String,
        accepter: String,
        created_at: i64,
    },
    Ended {
        a: String,
        b: String,
    },
}

/// Tells chat presence about a stored friendship change (`NotifyFriendship`).
pub fn notify(state: &crate::app::AppState, event: Option<FriendshipEvent>) {
    if let Some(event) = event {
        // Nobody listening (chat off) is fine
        let _ = state.friendships.send(event);
    }
}

/// A change and the friendship event it makes, if any.
pub type Changed = (Outcome, Option<FriendshipEvent>);

/// What a change did (`FriendResult`): 204, or a refusal's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done(&'static str),
    Refused(u16, &'static str),
}

impl Outcome {
    fn shown(self) -> String {
        match self {
            Outcome::Done(result) => result.to_owned(),
            Outcome::Refused(status, error) => format!("{status} {error}"),
        }
    }
}

fn pair<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a < b { (a, b) } else { (b, a) }
}

pub(crate) fn account_exists(conn: &mut Conn, account: &str) -> QueryResult<bool> {
    Ok(users::table
        .find(account)
        .select(users::userId)
        .first::<String>(conn)
        .optional()?
        .is_some())
}

/// `IsBlockedEitherWayInTx`.
pub fn blocked_either_way(conn: &mut Conn, a: &str, b: &str) -> QueryResult<bool> {
    Ok(blocks::table
        .filter(
            blocks::blockerId
                .eq(a)
                .and(blocks::blockedId.eq(b))
                .or(blocks::blockerId.eq(b).and(blocks::blockedId.eq(a))),
        )
        .select(blocks::blockerId)
        .first::<String>(conn)
        .optional()?
        .is_some())
}

/// `AreFriendsInTx`: friends on both sides (the request was accepted).
pub fn are_friends(conn: &mut Conn, a: &str, b: &str) -> QueryResult<bool> {
    let (low, high) = pair(a, b);
    let status: Option<String> = friendships::table
        .find((low, high))
        .select(friendships::status)
        .first(conn)
        .optional()?;
    Ok(status.as_deref() == Some("ACCEPTED"))
}

/// The Slayer Link invites between the two that still wait for an answer, one update per direction.
fn cancel_slayer_link_invites(conn: &mut Conn, a: &str, b: &str) -> QueryResult<usize> {
    let mut cancelled = 0;
    for (sender, target) in [(a, b), (b, a)] {
        cancelled += diesel::update(
            slayerlinkinvites::table
                .filter(slayerlinkinvites::status.eq("PENDING"))
                .filter(slayerlinkinvites::senderId.eq(sender))
                .filter(slayerlinkinvites::targetId.eq(target)),
        )
        .set(slayerlinkinvites::status.eq("CANCELED"))
        .execute(conn)?;
    }
    Ok(cancelled)
}

fn slayer_link_note(cancelled: usize) -> String {
    if cancelled > 0 {
        format!(" ({cancelled} Slayer Link invite(s) between them cancelled)")
    } else {
        String::new()
    }
}

fn count_friendships(conn: &mut Conn, account: &str) -> QueryResult<i64> {
    friendships::table
        .filter(friendships::userLow.eq(account).or(friendships::userHigh.eq(account)))
        .select(count_star())
        .first(conn)
}

type FriendshipRow = (String, String, String, String, i64, i64);

/// `ListFriends`: the bare array the client wraps as `{"friends": ...}`, oldest first.
pub fn list_friends(conn: &mut Conn, account: &str, include_pending: bool) -> QueryResult<Vec<Value>> {
    let mut rows: Vec<FriendshipRow> = friendships::table
        .filter(friendships::userLow.eq(account).or(friendships::userHigh.eq(account)))
        .load(conn)?;
    rows.retain(|row| row.3 == "ACCEPTED" || (include_pending && row.3 == "PENDING"));
    rows.sort_by_key(|row| row.4);
    Ok(rows
        .into_iter()
        .map(|(low, high, requester, status, created, _)| {
            json!({
                "accountId": if low == account { high } else { low },
                "status": if status == "ACCEPTED" { "ACCEPTED" } else { "PENDING" },
                "direction": if requester == account { "OUTBOUND" } else { "INBOUND" },
                "created": iso(created)
            })
        })
        .collect())
}

/// `ListBlocked`, sorted as `localeCompare` sorts.
pub fn list_blocked(conn: &mut Conn, account: &str) -> QueryResult<Vec<String>> {
    let mut ids: Vec<String> = blocks::table
        .filter(blocks::blockerId.eq(account))
        .select(blocks::blockedId)
        .load(conn)?;
    ids.sort_by(|a, b| service_kit::collate::locale_compare(a, b));
    Ok(ids)
}

/// `SendOrAcceptFriendRequest`: a request the other side already sent is accepted; asking again, or
/// asking an existing friend, changes nothing.
pub fn send_or_accept(conn: &mut Conn, recent: &RecentRequests, me: &str, them: &str) -> QueryResult<Changed> {
    send_or_accept_at(conn, recent, me, them, now_ms())
}

/// [`send_or_accept`] at `now`, for the limits' tests (Node's `SetFriendsClockForTests`).
pub fn send_or_accept_at(
    conn: &mut Conn,
    recent: &RecentRequests,
    me: &str,
    them: &str,
    now: i64,
) -> QueryResult<Changed> {
    let (outcome, requested_at) = if me == them {
        (Outcome::Refused(400, "self"), 0)
    } else {
        request_in_tx(conn, recent, me, them, now)?
    };
    tracing::info!("friends: request by={me} to={them} -> {}", outcome.shown());
    let event = (outcome == Outcome::Done("accepted")).then(|| FriendshipEvent::Accepted {
        requester: them.to_owned(),
        accepter: me.to_owned(),
        created_at: requested_at,
    });
    Ok((outcome, event))
}

/// The outcome, and when the accepted request was made
fn request_in_tx(
    conn: &mut Conn,
    recent: &RecentRequests,
    me: &str,
    them: &str,
    now: i64,
) -> QueryResult<(Outcome, i64)> {
    let outcome = |o: Outcome| Ok((o, 0));
    if !account_exists(conn, them)? {
        return outcome(Outcome::Refused(404, "not_found"));
    }
    if blocked_either_way(conn, me, them)? {
        return outcome(Outcome::Refused(403, "blocked"));
    }
    let (low, high) = pair(me, them);
    let existing: Option<FriendshipRow> = friendships::table.find((low, high)).first(conn).optional()?;
    if let Some((_, _, requester, status, created_at, _)) = existing {
        if status == "ACCEPTED" {
            return outcome(Outcome::Done("already_friends"));
        }
        if requester == me {
            return outcome(Outcome::Done("already_requested"));
        }
        diesel::update(friendships::table.find((low, high)))
            .set((friendships::status.eq("ACCEPTED"), friendships::updatedAt.eq(now)))
            .execute(conn)?;
        return Ok((Outcome::Done("accepted"), created_at));
    }
    if count_friendships(conn, me)? >= MAX_FRIENDSHIPS_PER_ACCOUNT
        || count_friendships(conn, them)? >= MAX_FRIENDSHIPS_PER_ACCOUNT
    {
        return outcome(Outcome::Refused(409, "limit"));
    }
    let pending: i64 = friendships::table
        .filter(friendships::requesterId.eq(me))
        .filter(friendships::status.eq("PENDING"))
        .select(count_star())
        .first(conn)?;
    if pending >= MAX_PENDING_OUTGOING {
        return outcome(Outcome::Refused(409, "pending_limit"));
    }
    let mut map = recent.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut times = RecentRequests::recent(&mut map, me, now);
    if times.len() >= MAX_REQUESTS_PER_WINDOW {
        return outcome(Outcome::Refused(409, "rate"));
    }
    diesel::insert_into(friendships::table)
        .values((
            friendships::userLow.eq(low),
            friendships::userHigh.eq(high),
            friendships::requesterId.eq(me),
            friendships::status.eq("PENDING"),
            friendships::createdAt.eq(now),
            friendships::updatedAt.eq(now),
        ))
        .execute(conn)?;
    times.push(now);
    map.insert(me.to_owned(), times);
    outcome(Outcome::Done("requested"))
}

/// `RemoveFriend`: unfriend, withdraw a request or decline one.
pub fn remove(conn: &mut Conn, me: &str, them: &str) -> QueryResult<Changed> {
    let (low, high) = pair(me, them);
    let removed: Vec<String> = diesel::delete(friendships::table.find((low, high)))
        .returning(friendships::status)
        .get_results(conn)?;
    let cancelled = cancel_slayer_link_invites(conn, me, them)?;
    let outcome = Outcome::Done(if removed.is_empty() { "not_friends" } else { "removed" });
    tracing::info!(
        "friends: remove by={me} other={them} -> {}{}",
        outcome.shown(),
        slayer_link_note(cancelled)
    );
    let event = removed.iter().any(|s| s == "ACCEPTED").then(|| FriendshipEvent::Ended {
        a: me.to_owned(),
        b: them.to_owned(),
    });
    Ok((outcome, event))
}

/// `BlockPlayer`.
pub fn block(conn: &mut Conn, me: &str, them: &str) -> QueryResult<Changed> {
    let (outcome, guild_invites, links, were_friends) = if me == them {
        (Outcome::Refused(400, "self"), 0, 0, false)
    } else {
        block_in_tx(conn, me, them)?
    };
    tracing::info!(
        "friends: block by={me} target={them} -> {}{}{}",
        outcome.shown(),
        if guild_invites > 0 {
            format!(" ({guild_invites} guild invite(s) between them removed)")
        } else {
            String::new()
        },
        slayer_link_note(links)
    );
    let event = (outcome == Outcome::Done("blocked") && were_friends).then(|| FriendshipEvent::Ended {
        a: me.to_owned(),
        b: them.to_owned(),
    });
    Ok((outcome, event))
}

/// The outcome, the guild invites and Slayer Link invites removed, and whether the two were friends
fn block_in_tx(conn: &mut Conn, me: &str, them: &str) -> QueryResult<(Outcome, usize, usize, bool)> {
    if !account_exists(conn, them)? {
        return Ok((Outcome::Refused(404, "not_found"), 0, 0, false));
    }
    let existing: Option<String> = blocks::table
        .find((me, them))
        .select(blocks::blockerId)
        .first(conn)
        .optional()?;
    if existing.is_some() {
        return Ok((Outcome::Done("already_blocked"), 0, 0, false));
    }
    let count: i64 = blocks::table
        .filter(blocks::blockerId.eq(me))
        .select(count_star())
        .first(conn)?;
    if count >= MAX_BLOCKS_PER_ACCOUNT {
        return Ok((Outcome::Refused(409, "limit"), 0, 0, false));
    }
    diesel::insert_into(blocks::table)
        .values((
            blocks::blockerId.eq(me),
            blocks::blockedId.eq(them),
            blocks::createdAt.eq(now_ms()),
        ))
        .execute(conn)?;
    let (low, high) = pair(me, them);
    let ended: Vec<String> = diesel::delete(friendships::table.find((low, high)))
        .returning(friendships::status)
        .get_results(conn)?;
    let guild_invites = diesel::delete(
        guildinvites::table.filter(
            guildinvites::inviterId
                .eq(me)
                .and(guildinvites::inviteeId.eq(them))
                .or(guildinvites::inviterId.eq(them).and(guildinvites::inviteeId.eq(me))),
        ),
    )
    .execute(conn)?;
    let links = cancel_slayer_link_invites(conn, me, them)?;
    Ok((
        Outcome::Done("blocked"),
        guild_invites,
        links,
        ended.iter().any(|s| s == "ACCEPTED"),
    ))
}

/// `UnblockPlayer`.
pub fn unblock(conn: &mut Conn, me: &str, them: &str) -> QueryResult<Outcome> {
    let removed = diesel::delete(blocks::table.find((me, them))).execute(conn)?;
    let outcome = Outcome::Done(if removed > 0 { "unblocked" } else { "not_blocked" });
    tracing::info!("friends: unblock by={me} target={them} -> {}", outcome.shown());
    Ok(outcome)
}
