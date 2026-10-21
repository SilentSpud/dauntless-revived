//! Accounts: names, registration, renames, invites, lookups and the keys registered at start
//! (`controllers/accounts.ts`, `controllers/login.ts`, the account parts of `controllers/undauntedapi.ts`
//! and `controllers/apikeys.ts`; MIGRATION_PLAN.md M29, first area).
//!
//! Names are 3-16 letters, digits or underscores and unique regardless of case; existing accounts
//! keep whatever name they have. Each function takes a connection, so a route runs it inside one
//! `store` closure, and registration and renames are one transaction.

use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use serde_json::{Value, json};

use crate::crypto::{random_below, random_hex, random_uuid, sha256_hex};
use crate::schema::{
    characters, gameserverapikeys, gameserverapikeystoregister, invitecodes, userapikeys, userapikeystoregister, users,
};

pub type Conn = store::Connection;

/// `USERNAME_RULE`.
pub fn is_valid_username(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `ACCOUNT_ID_RULE`: the shape of account ids that are looked up at all.
pub fn is_account_id_shape(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The error codes of the `/undaunted/api` account routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountError {
    UsernameInvalid,
    RegistrationClosed,
    BadRequest,
    InviteInvalid,
    UsernameTaken,
    NotFound,
}

impl AccountError {
    pub fn code(self) -> &'static str {
        match self {
            Self::UsernameInvalid => "username_invalid",
            Self::RegistrationClosed => "registration_closed",
            Self::BadRequest => "bad_request",
            Self::InviteInvalid => "invite_invalid",
            Self::UsernameTaken => "username_taken",
            Self::NotFound => "not_found",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::UsernameInvalid => "Usernames are 3-16 characters: letters, numbers and underscore.",
            Self::RegistrationClosed => "Registration is closed on this server.",
            Self::BadRequest => "The request body is not JSON, or a field is missing or of the wrong type.",
            Self::InviteInvalid => "That invite code is not valid or has already been used.",
            Self::UsernameTaken => "That username is taken.",
            Self::NotFound => "No such account.",
        }
    }

    /// `ErrorBody`: `{"error": code, "message": text}`.
    pub fn body(self) -> Value {
        json!({"error": self.code(), "message": self.message()})
    }
}

/// A refusal with its HTTP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failure {
    pub status: u16,
    pub error: AccountError,
}

fn failure(status: u16, error: AccountError) -> Failure {
    Failure { status, error }
}

/// `lower(name) = lower(?)`; SQLite's lower() folds ASCII only, which covers every allowed name.
fn same_name(name: &str) -> Box<dyn BoxableExpression<users::table, diesel::sqlite::Sqlite, SqlType = Bool>> {
    Box::new(
        sql::<Bool>("lower(name) = lower(")
            .bind::<Text, _>(name.to_owned())
            .sql(")"),
    )
}

fn is_username_taken(conn: &mut Conn, name: &str, except: Option<&str>) -> QueryResult<bool> {
    let mut query = users::table.select(users::userId).filter(same_name(name)).into_boxed();
    if let Some(except) = except {
        query = query.filter(users::userId.ne(except.to_owned()));
    }
    Ok(query.first::<String>(conn).optional()?.is_some())
}

pub fn username_taken(conn: &mut Conn, name: &str) -> QueryResult<bool> {
    is_username_taken(conn, name, None)
}

/// A usable invite: infinite, or uses left.
fn usable_invite(code: &str) -> impl Fn() -> invitecodes::BoxedQuery<'static, diesel::sqlite::Sqlite> {
    let code = code.to_owned();
    move || {
        invitecodes::table
            .filter(invitecodes::invitecode.eq(code.clone()))
            .filter(invitecodes::infiniteUses.eq(true).or(invitecodes::usesRemaining.gt(0)))
            .into_boxed()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub user_id: String,
    pub username: String,
    pub key: String,
}

/// `RegisterAccount`: in one transaction (the caller's `store::write`), the invite is checked, then
/// the name, and only then is a use of the invite spent, so a taken name never costs the code; the
/// account and its key hash are written together.
pub fn register(
    conn: &mut Conn,
    mode: &str,
    requested: Option<&Value>,
    invite: Option<&Value>,
) -> QueryResult<Result<Registered, Failure>> {
    if mode == "NONE" {
        return Ok(Err(failure(400, AccountError::RegistrationClosed)));
    }
    let Some(Value::String(requested)) = requested else {
        return Ok(Err(failure(400, AccountError::BadRequest)));
    };
    let username = requested.trim();
    if !is_valid_username(username) {
        return Ok(Err(failure(400, AccountError::UsernameInvalid)));
    }
    let code = match invite {
        Some(Value::String(code)) => code.trim(),
        _ => "",
    };
    let by_invite = mode == "INVITECODE";
    if by_invite && code.is_empty() {
        return Ok(Err(failure(401, AccountError::InviteInvalid)));
    }
    let user_id = format!("UID-{}", random_uuid());
    let key = format!("UUK_{}", random_hex(24));

    if by_invite
        && usable_invite(code)()
            .select(invitecodes::invitecode)
            .first::<String>(conn)
            .optional()?
            .is_none()
    {
        return Ok(Err(failure(401, AccountError::InviteInvalid)));
    }
    if is_username_taken(conn, username, None)? {
        return Ok(Err(failure(409, AccountError::UsernameTaken)));
    }
    if by_invite {
        // The decrement and its condition are one UPDATE.
        let spent = diesel::update(invitecodes::table)
            .filter(invitecodes::invitecode.eq(code))
            .filter(invitecodes::infiniteUses.eq(true).or(invitecodes::usesRemaining.gt(0)))
            .set(invitecodes::usesRemaining.eq(sql::<diesel::sql_types::BigInt>(
                "case when infiniteUses then usesRemaining else usesRemaining - 1 end",
            )))
            .returning(invitecodes::invitecode)
            .get_results::<String>(conn)?;
        if spent.len() != 1 {
            return Ok(Err(failure(401, AccountError::InviteInvalid)));
        }
    }
    diesel::insert_into(users::table)
        .values((users::userId.eq(&user_id), users::name.eq(username), users::notes.eq(0)))
        .execute(conn)?;
    diesel::insert_into(userapikeys::table)
        .values((
            userapikeys::userId.eq(&user_id),
            userapikeys::keyHash.eq(sha256_hex(&key)),
        ))
        .execute(conn)?;
    Ok(Ok(Registered {
        user_id,
        username: username.to_owned(),
        key,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    pub user_id: String,
    pub old_username: String,
    pub username: String,
    pub characters: usize,
}

/// `RenameAccount`: the account, found by id or (case-insensitively, unambiguously) by name, and its
/// characters get the new name together.
pub fn rename(
    conn: &mut Conn,
    user_id: Option<&Value>,
    username: Option<&Value>,
    requested: Option<&Value>,
) -> QueryResult<Result<Renamed, Failure>> {
    let Some(Value::String(requested)) = requested else {
        return Ok(Err(failure(400, AccountError::BadRequest)));
    };
    let new_name = requested.trim();
    if !is_valid_username(new_name) {
        return Ok(Err(failure(400, AccountError::UsernameInvalid)));
    }
    let by_id = match user_id {
        Some(Value::String(id)) if !id.is_empty() => Some(id.as_str()),
        _ => None,
    };
    let by_name = match username {
        Some(Value::String(name)) if !name.is_empty() => Some(name.as_str()),
        _ => None,
    };
    let found: Option<(String, String)> = match (by_id, by_name) {
        (Some(id), _) => users::table
            .find(id)
            .select((users::userId, users::name))
            .first(conn)
            .optional()?,
        (None, Some(name)) => {
            let rows: Vec<(String, String)> = users::table
                .select((users::userId, users::name))
                .filter(same_name(name))
                .load(conn)?;
            if rows.len() == 1 { rows.into_iter().next() } else { None }
        }
        (None, None) => return Ok(Err(failure(400, AccountError::BadRequest))),
    };
    let Some((user_id, old_username)) = found else {
        return Ok(Err(failure(404, AccountError::NotFound)));
    };
    if is_username_taken(conn, new_name, Some(&user_id))? {
        return Ok(Err(failure(409, AccountError::UsernameTaken)));
    }
    diesel::update(users::table.find(&user_id))
        .set(users::name.eq(new_name))
        .execute(conn)?;
    let characters = diesel::update(characters::table.filter(characters::userId.eq(&user_id)))
        .set(characters::name.eq(new_name))
        .execute(conn)?;
    Ok(Ok(Renamed {
        user_id,
        old_username,
        username: new_name.to_owned(),
        characters,
    }))
}

/// Crockford base32: no I, L, O or U, so a code read aloud can't be misread.
const INVITE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const MAX_INVITE_USES: i64 = 1000;

/// `GenerateInviteCode`: three groups of four, 60 random bits.
pub fn generate_invite_code() -> String {
    (0..3)
        .map(|_| {
            (0..4)
                .map(|_| INVITE_ALPHABET[random_below(INVITE_ALPHABET.len())] as char)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// `CleanInviteNote`: printable ASCII, trimmed, at most 64 characters, for the host's log only.
pub fn clean_invite_note(value: Option<&Value>) -> Option<String> {
    let Some(Value::String(text)) = value else { return None };
    let note: String = text.chars().filter(|c| (' '..='~').contains(c)).collect();
    let note: String = note.trim().chars().take(64).collect();
    (!note.is_empty()).then_some(note)
}

/// `RegisterInviteCode`. `uses` is read as JavaScript's `Number()` does for the cases that matter: a
/// number, or a string holding one.
pub fn add_invite_code(
    conn: &mut Conn,
    code: Option<&Value>,
    uses: Option<&Value>,
    infinite: bool,
) -> QueryResult<bool> {
    let Some(Value::String(code)) = code else {
        return Ok(false);
    };
    let code = code.trim();
    if code.is_empty() {
        return Ok(false);
    }
    let parsed = match uses {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(text)) if text.trim().is_empty() => Some(0.0),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(b)) => Some(f64::from(u8::from(*b))),
        Some(Value::Null) => Some(0.0),
        _ => None,
    };
    let uses = match parsed {
        _ if infinite => 0,
        Some(n) if n.fract() == 0.0 && n >= 1.0 && n <= i64::MAX as f64 => n as i64,
        _ => return Ok(false),
    };
    diesel::insert_into(invitecodes::table)
        .values((
            invitecodes::invitecode.eq(code),
            invitecodes::usesRemaining.eq(uses),
            invitecodes::infiniteUses.eq(infinite),
        ))
        .execute(conn)?;
    Ok(true)
}

pub fn delete_invite_code(conn: &mut Conn, code: &str) -> QueryResult<()> {
    diesel::delete(invitecodes::table.filter(invitecodes::invitecode.eq(code)))
        .execute(conn)
        .map(drop)
}

/// `GetInviteCodes`: every code as Drizzle returns it.
pub fn invite_codes(conn: &mut Conn) -> QueryResult<Vec<Value>> {
    let rows: Vec<(String, i64, bool)> = invitecodes::table
        .select((
            invitecodes::invitecode,
            invitecodes::usesRemaining,
            invitecodes::infiniteUses,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(code, uses, infinite)| json!({"inviteCode": code, "usesRemaining": uses, "infiniteUses": infinite}))
        .collect())
}

/// `GetAllUserIds`.
pub fn all_users(conn: &mut Conn) -> QueryResult<Vec<Value>> {
    let rows: Vec<(String, String)> = users::table.select((users::userId, users::name)).load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| json!({"Username": name, "UserId": id}))
        .collect())
}

/// `FindUsernameForUserId`: `None` for an unknown or malformed id.
pub fn username_for(conn: &mut Conn, user_id: &str) -> QueryResult<Option<String>> {
    if !is_account_id_shape(user_id) {
        return Ok(None);
    }
    users::table.find(user_id).select(users::name).first(conn).optional()
}

/// `FindAccount`: an account named by id or by name (the `/undaunted/api` fallbacks take both); a
/// name must name exactly one account. `(id, name)`.
pub fn find_account(conn: &mut Conn, who: Option<&Value>) -> QueryResult<Option<(String, String)>> {
    let Some(who) = who.and_then(Value::as_str) else {
        return Ok(None);
    };
    if let Some(name) = username_for(conn, who)? {
        return Ok(Some((who.to_owned(), name)));
    }
    let name = crate::js::trim(who);
    if name.is_empty() || name.encode_utf16().count() > 64 {
        return Ok(None);
    }
    let rows: Vec<(String, String)> = users::table
        .filter(same_name(name))
        .select((users::userId, users::name))
        .limit(2)
        .load(conn)?;
    Ok(match rows.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    })
}

/// `FindUsernames`: names of several accounts; unknown ids are left out.
pub fn usernames_for(conn: &mut Conn, ids: &[String]) -> QueryResult<std::collections::HashMap<String, String>> {
    let mut wanted: Vec<&String> = ids.iter().filter(|id| is_account_id_shape(id)).collect();
    wanted.sort();
    wanted.dedup();
    if wanted.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<(String, String)> = users::table
        .select((users::userId, users::name))
        .filter(users::userId.eq_any(wanted))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// `FindAccountByUsername`: case-insensitive; two older accounts that differ only in case are ambiguous.
pub fn account_by_username(conn: &mut Conn, name: &str) -> QueryResult<Option<(String, String)>> {
    let name = name.trim();
    if name.is_empty() || name.encode_utf16().count() > 64 {
        return Ok(None);
    }
    let rows: Vec<(String, String)> = users::table
        .select((users::userId, users::name))
        .filter(same_name(name))
        .limit(2)
        .load(conn)?;
    Ok(if rows.len() == 1 { rows.into_iter().next() } else { None })
}

/// `DrainAndRegisterAPIKeys` and `DrainAndRegisterUserAPIKeys`: keys the host scripts left to register
/// are hashed into the key tables, and the plain keys deleted. Returns how many of each.
pub fn drain_keys_to_register(conn: &mut Conn) -> QueryResult<(usize, usize)> {
    let server_keys: Vec<String> = gameserverapikeystoregister::table
        .select(gameserverapikeystoregister::key)
        .load(conn)?;
    diesel::delete(gameserverapikeystoregister::table).execute(conn)?;
    for key in &server_keys {
        diesel::insert_into(gameserverapikeys::table)
            .values(gameserverapikeys::keyHash.eq(sha256_hex(key)))
            .execute(conn)?;
    }
    let user_keys: Vec<(String, String)> = userapikeystoregister::table
        .select((userapikeystoregister::userId, userapikeystoregister::key))
        .load(conn)?;
    diesel::delete(userapikeystoregister::table).execute(conn)?;
    for (user_id, key) in &user_keys {
        diesel::insert_into(userapikeys::table)
            .values((
                userapikeys::userId.eq(user_id),
                userapikeys::keyHash.eq(sha256_hex(key)),
            ))
            .execute(conn)?;
    }
    Ok((server_keys.len(), user_keys.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cases of accounts.test.ts's "username rules".
    #[test]
    fn username_rules() {
        for name in ["abc", "Slayer_2", "A_B_C_D_E_F_G_H_", "0123456789abcdef", "___"] {
            assert!(is_valid_username(name), "{name}");
        }
        for name in [
            "ab",
            &"a".repeat(17),
            "has space",
            " padded",
            "padded ",
            "dash-name",
            "dot.name",
            "émile",
            "名前名前",
            "",
            "tab\tname",
            "new\nline",
        ] {
            assert!(!is_valid_username(name), "{name:?}");
        }
    }

    #[test]
    fn invite_codes_are_three_groups_of_crockford_base32() {
        let code = generate_invite_code();
        assert_eq!(code.len(), 14);
        assert!(
            code.split('-')
                .all(|group| group.len() == 4 && group.bytes().all(|b| INVITE_ALPHABET.contains(&b)))
        );
    }

    #[test]
    fn invite_notes_keep_printable_ascii() {
        assert_eq!(
            clean_invite_note(Some(&json!("  for Alex\u{7}\n "))),
            Some("for Alex".into())
        );
        assert_eq!(clean_invite_note(Some(&json!("\u{1}"))), None);
        assert_eq!(clean_invite_note(Some(&json!(5))), None);
    }
}
