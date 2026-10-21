//! Account key recovery through a linked Discord account (`controllers/keyrotation.ts`,
//! `discordlinks.ts`), for the Discord bot's `/internal/key-recovery/:action` (`routes/keyrotation.rs`).
//!
//! A player first proves a link by presenting their current key (`link`); a historical link made by
//! an administrator alone is deliberately not enough. Recovery is then a challenge (`begin`, the same
//! answer whether a trusted link exists or not) and its confirmation (`confirm`), which replaces the
//! key, bumps the account's auth epoch and closes its chat sessions. Each Discord id gets at most five
//! recovery events per 15 minutes.

use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

use crate::crypto::{random_hex, sha256_hex};
use crate::schema::{
    accountkeyrecovery, authepochs, discordlinks, recoverychallenges, recoveryevents, recoveryproofs, userapikeys,
    userapikeystoregister, users,
};

pub type Conn = store::Connection;

const WINDOW_MS: i64 = 15 * 60 * 1000;
const MAX_EVENTS: i64 = 5;
const CHALLENGE_MS: i64 = 5 * 60 * 1000;

/// `/^\d{17,20}$/`.
pub fn is_discord_id(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// `/^UID-[A-Za-z0-9-]{1,100}$/`.
pub fn is_link_user_id(id: &str) -> bool {
    id.strip_prefix("UID-").is_some_and(|rest| {
        (1..=100).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// `DiscordAccount`: the account linked to a Discord id, and its name.
pub fn discord_account(conn: &mut Conn, discord: &str) -> QueryResult<Option<(String, String)>> {
    discordlinks::table
        .inner_join(users::table.on(users::userId.eq(discordlinks::userId)))
        .filter(discordlinks::discordId.eq(discord))
        .select((discordlinks::userId, users::name))
        .first(conn)
        .optional()
}

/// `LinkDiscordAccount`'s status: `linked`, `invalid_key`, `discord_already_linked` or
/// `account_already_linked`. Additive: linking never writes keys, characters or progression.
pub fn link_discord_account(conn: &mut Conn, discord: Option<&str>, user: Option<&str>) -> QueryResult<&'static str> {
    let (Some(discord), Some(user)) = (
        discord.filter(|d| is_discord_id(d)),
        user.filter(|u| is_link_user_id(u)),
    ) else {
        return Ok("invalid_key");
    };
    conn.transaction(|conn| {
        let exists = users::table
            .find(user)
            .select(users::userId)
            .first::<String>(conn)
            .optional()?
            .is_some();
        if !exists {
            return Ok("invalid_key");
        }
        if discord_account(conn, discord)?.is_some_and(|(linked, _)| linked != user) {
            return Ok("discord_already_linked");
        }
        let other = discordlinks::table
            .filter(discordlinks::userId.eq(user))
            .filter(discordlinks::discordId.ne(discord))
            .select(discordlinks::discordId)
            .first::<String>(conn)
            .optional()?;
        if other.is_some() {
            return Ok("account_already_linked");
        }
        diesel::insert_or_ignore_into(discordlinks::table)
            .values((
                discordlinks::discordId.eq(discord),
                discordlinks::userId.eq(user),
                discordlinks::linkedAt.eq(crate::time::iso_now()),
            ))
            .execute(conn)?;
        Ok("linked")
    })
}

fn limited(conn: &mut Conn, discord: &str, now: i64) -> QueryResult<bool> {
    let events: i64 = recoveryevents::table
        .filter(recoveryevents::discordId.eq(discord))
        .filter(recoveryevents::createdAt.gt(now - WINDOW_MS))
        .select(count_star())
        .first(conn)?;
    Ok(events >= MAX_EVENTS)
}

fn audit(conn: &mut Conn, discord: &str, action: &str, now: i64) -> QueryResult<()> {
    diesel::insert_into(recoveryevents::table)
        .values((
            recoveryevents::discordId.eq(discord),
            recoveryevents::action.eq(action),
            recoveryevents::createdAt.eq(now),
        ))
        .execute(conn)
        .map(drop)
}

/// `ProveRecoveryLink` (`link`): the current key of a player account links the Discord id and
/// becomes the recovery proof.
pub fn prove_link(conn: &mut Conn, discord: Option<&str>, key: Option<&str>, now: i64) -> QueryResult<bool> {
    let (Some(discord), Some(key)) = (
        discord.filter(|d| is_discord_id(d)),
        key.filter(|k| k.encode_utf16().count() <= 2048),
    ) else {
        return Ok(false);
    };
    if limited(conn, discord, now)? {
        return Ok(false);
    }
    audit(conn, discord, "link_attempt", now)?;
    let digest = sha256_hex(key);
    let user = userapikeys::table
        .inner_join(users::table.on(users::userId.eq(userapikeys::userId)))
        .filter(userapikeys::keyHash.eq(&digest))
        .filter(users::isAdmin.eq(false))
        .select(userapikeys::userId)
        .first::<String>(conn)
        .optional()?;
    let Some(user) = user else {
        return Ok(false);
    };
    if link_discord_account(conn, Some(discord), Some(&user))? != "linked" {
        return Ok(false);
    }
    diesel::insert_into(recoveryproofs::table)
        .values((
            recoveryproofs::discordId.eq(discord),
            recoveryproofs::userId.eq(&user),
            recoveryproofs::keyHash.eq(&digest),
        ))
        .on_conflict(recoveryproofs::discordId)
        .do_update()
        .set((recoveryproofs::userId.eq(&user), recoveryproofs::keyHash.eq(&digest)))
        .execute(conn)?;
    Ok(true)
}

#[derive(QueryableByName)]
struct Proof {
    #[diesel(sql_type = Text)]
    #[diesel(column_name = userId)]
    user_id: String,
    #[diesel(sql_type = Text)]
    #[diesel(column_name = keyHash)]
    key_hash: String,
}

/// `BeginKeyRecovery` (`begin`): a challenge, bound to the proven account when there is one. The
/// answer is the same either way.
pub fn begin(conn: &mut Conn, discord: Option<&str>, now: i64) -> QueryResult<Option<String>> {
    let Some(discord) = discord.filter(|d| is_discord_id(d)) else {
        return Ok(None);
    };
    if limited(conn, discord, now)? {
        return Ok(None);
    }
    audit(conn, discord, "begin", now)?;
    let proof: Option<Proof> = diesel::sql_query(
        "SELECT p.userId, p.keyHash FROM recoveryproofs p JOIN discordlinks d ON d.discordId=p.discordId AND d.userId=p.userId \
         JOIN userapikeys k ON k.userId=p.userId AND k.keyHash=p.keyHash JOIN users u ON u.userId=p.userId \
         WHERE p.discordId=? AND u.isAdmin=0",
    )
    .bind::<Text, _>(discord)
    .get_result(conn)
    .optional()?;
    let token = random_hex(32);
    diesel::delete(
        recoverychallenges::table.filter(
            recoverychallenges::discordId
                .eq(discord)
                .or(recoverychallenges::expiresAt.lt(now)),
        ),
    )
    .execute(conn)?;
    diesel::insert_into(recoverychallenges::table)
        .values((
            recoverychallenges::tokenHash.eq(sha256_hex(&token)),
            recoverychallenges::discordId.eq(discord),
            recoverychallenges::userId.eq(proof.as_ref().map(|p| p.user_id.clone())),
            recoverychallenges::keyHash.eq(proof.as_ref().map(|p| p.key_hash.clone())),
            recoverychallenges::expiresAt.eq(now + CHALLENGE_MS),
        ))
        .execute(conn)?;
    Ok(Some(token))
}

/// `CompleteKeyRecovery` (`confirm`): the new key and its account, for a live challenge of a
/// proven link whose key is still the proven one.
pub fn complete(
    conn: &mut Conn,
    discord: Option<&str>,
    token: Option<&str>,
    now: i64,
) -> QueryResult<Option<(String, String)>> {
    let (Some(discord), Some(token)) = (
        discord.filter(|d| is_discord_id(d)),
        token.filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
    ) else {
        return Ok(None);
    };
    if limited(conn, discord, now)? {
        return Ok(None);
    }
    audit(conn, discord, "confirm_attempt", now)?;
    let token_hash = sha256_hex(token);
    let found: Option<Proof> = diesel::sql_query(
        "SELECT c.userId, c.keyHash FROM recoverychallenges c JOIN recoveryproofs p ON p.discordId=c.discordId AND p.userId=c.userId AND p.keyHash=c.keyHash \
         JOIN discordlinks d ON d.discordId=p.discordId AND d.userId=p.userId JOIN userapikeys k ON k.userId=c.userId AND k.keyHash=c.keyHash \
         JOIN users u ON u.userId=c.userId WHERE c.tokenHash=? AND c.discordId=? AND c.used=0 AND c.expiresAt>? AND u.isAdmin=0",
    )
    .bind::<Text, _>(&token_hash)
    .bind::<Text, _>(discord)
    .bind::<BigInt, _>(now)
    .get_result(conn)
    .optional()?;
    let Some(found) = found else {
        return Ok(None);
    };
    let key = format!("UUK_{}", random_hex(32));
    let digest = sha256_hex(&key);
    diesel::update(
        userapikeys::table
            .filter(userapikeys::userId.eq(&found.user_id))
            .filter(userapikeys::keyHash.eq(&found.key_hash)),
    )
    .set(userapikeys::keyHash.eq(&digest))
    .execute(conn)?;
    diesel::delete(userapikeystoregister::table.find(&found.user_id)).execute(conn)?;
    diesel::delete(accountkeyrecovery::table.find(&found.user_id)).execute(conn)?;
    diesel::update(recoveryproofs::table.find(discord))
        .set(recoveryproofs::keyHash.eq(&digest))
        .execute(conn)?;
    diesel::update(recoverychallenges::table.find(&token_hash))
        .set(recoverychallenges::used.eq(1))
        .execute(conn)?;
    diesel::insert_into(authepochs::table)
        .values((authepochs::userId.eq(&found.user_id), authepochs::epoch.eq(1)))
        .on_conflict(authepochs::userId)
        .do_update()
        .set(authepochs::epoch.eq(authepochs::epoch + 1))
        .execute(conn)?;
    audit(conn, discord, "rotated", now)?;
    Ok(Some((key, found.user_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_checked_like_node() {
        assert!(is_discord_id("12345678901234567"));
        assert!(!is_discord_id("1234567890123456"));
        assert!(!is_discord_id("123456789012345678901"));
        assert!(!is_discord_id("1234567890123456x"));
        assert!(is_link_user_id("UID-abc-DEF-1"));
        assert!(!is_link_user_id("UID-"));
        assert!(!is_link_user_id("UID-a_b"));
    }
}
