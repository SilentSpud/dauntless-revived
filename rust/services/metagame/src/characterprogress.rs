//! Per-character progression state (`controllers/progression.ts`): encountered content and
//! breadcrumbs. Each read-and-write is one transaction (they used to be separate awaited queries, so
//! two requests could both read the old list and the second write dropped the first one's entry).

use diesel::prelude::*;
use serde_json::{Value, json};

use crate::characters::belongs;
use crate::js;
use crate::schema::{breadcrumbs, encounteredcontent};

pub type Conn = store::Connection;

/// `ProgressionError`, with the message Node logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The character isn't the user's.
    Forbidden(String),
    /// A stale breadcrumbs update.
    Conflict(String),
    /// Stored data that isn't JSON.
    InvalidData(String),
    /// Anything else (Node's TypeErrors and database errors).
    Db(String),
}

impl Fail {
    pub fn status(&self) -> u16 {
        match self {
            Fail::Forbidden(_) => 403,
            Fail::Conflict(_) => 409,
            Fail::InvalidData(_) | Fail::Db(_) => 500,
        }
    }
}

impl From<diesel::result::Error> for Fail {
    fn from(error: diesel::result::Error) -> Self {
        Fail::Db(error.to_string())
    }
}

impl From<store::Error> for Fail {
    fn from(error: store::Error) -> Self {
        Fail::Db(error.to_string())
    }
}

/// `DoesCharacterBelongToUserId`, before every operation.
pub fn check_owner(conn: &mut Conn, user: Option<&str>, character: &str) -> Result<(), Fail> {
    let owned = match user {
        Some(user) => belongs(conn, user, character)?,
        None => false,
    };
    if owned {
        Ok(())
    } else {
        Err(Fail::Forbidden(format!(
            "Specified characterId {character} does not belong to user {}",
            user.unwrap_or("undefined")
        )))
    }
}

fn parse(text: &str, invalid: &str) -> Result<Value, Fail> {
    service_kit::jsjson::parse(text).map_err(|e| Fail::InvalidData(format!("{invalid}: SyntaxError: {e}")))
}

/// JavaScript's `a == b` on JSON values: `null` only equals `null`; numbers, strings and booleans
/// compare as numbers when their types differ; objects and arrays are never equal to a fresh value.
pub fn loose_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None | Some(Value::Null), None | Some(Value::Null)) => true,
        (None | Some(Value::Null), _) | (_, None | Some(Value::Null)) => false,
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        (Some(Value::Object(_) | Value::Array(_)), _) | (_, Some(Value::Object(_) | Value::Array(_))) => false,
        _ => js::number(a) == js::number(b),
    }
}

fn number_value(n: f64) -> Value {
    if n.is_nan() || n.is_infinite() {
        Value::Null
    } else if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

/// `QueryEncounteredContent`: per requested category, the content ids stored under it.
/// `categories` is what the request named; anything but a list fails as Node's `.map` did.
pub fn query_encountered(conn: &mut Conn, user: &str, character: &str, categories: &Value) -> Result<Value, Fail> {
    let stored: Option<String> = encounteredcontent::table
        .find(character)
        .filter(encounteredcontent::userId.eq(user))
        .select(encounteredcontent::encounteredcontent_)
        .first(conn)
        .optional()?;
    let content = parse(
        stored.as_deref().unwrap_or("[]"),
        &format!("Invalid encountered content data for characterId {character} and userId {user}"),
    )?;
    let failed = || {
        Fail::Db(format!(
            "Failed to query encountered content for characterId {character} and userId {user}"
        ))
    };
    let categories = categories.as_array().ok_or_else(failed)?;
    let content = content.as_array().ok_or_else(failed)?;
    Ok(Value::Array(
        categories
            .iter()
            .map(|category| {
                let wanted = js::number(Some(category));
                let ids: Vec<Value> = content
                    .iter()
                    .filter(|c| js::number(c.get("category")) == wanted)
                    .map(|c| c.get("content").cloned().unwrap_or(Value::Null))
                    .collect();
                json!({"content": ids, "content_type": number_value(wanted)})
            })
            .collect(),
    ))
}

/// `AddEncounteredContent`: adds `{content, category}` once.
pub fn add_encountered(
    conn: &mut Conn,
    user: &str,
    character: &str,
    content_type: Option<&Value>,
    content_id: Option<&Value>,
) -> Result<(), Fail> {
    let stored: Option<String> = encounteredcontent::table
        .find(character)
        .filter(encounteredcontent::userId.eq(user))
        .select(encounteredcontent::encounteredcontent_)
        .first(conn)
        .optional()?;
    if stored.is_none() {
        diesel::insert_into(encounteredcontent::table)
            .values((
                encounteredcontent::userId.eq(user),
                encounteredcontent::characterId.eq(character),
                encounteredcontent::encounteredcontent_.eq("[]"),
            ))
            .execute(conn)?;
    }
    let id = js::string(content_id);
    let mut content = parse(
        stored.as_deref().unwrap_or("[]"),
        &format!("Invalid encountered content data while adding {id} for characterId {character} and userId {user}"),
    )?;
    let failed = || {
        Fail::Db(format!(
            "Failed to add encountered content {id} for characterId {character} and userId {user}"
        ))
    };
    let list = content.as_array_mut().ok_or_else(failed)?;
    let wanted = js::number(content_type);
    if list
        .iter()
        .any(|c| js::number(c.get("category")) == wanted && loose_eq(c.get("content"), content_id))
    {
        return Ok(());
    }
    let mut entry = serde_json::Map::new();
    if let Some(id) = content_id {
        entry.insert("content".into(), id.clone());
    }
    if let Some(category) = content_type {
        entry.insert("category".into(), category.clone());
    }
    list.push(Value::Object(entry));
    diesel::update(
        encounteredcontent::table
            .find(character)
            .filter(encounteredcontent::userId.eq(user)),
    )
    .set(encounteredcontent::encounteredcontent_.eq(service_kit::jsjson::stringify(&content)))
    .execute(conn)?;
    Ok(())
}

fn stored_breadcrumbs(conn: &mut Conn, user: &str, character: &str) -> QueryResult<Option<(String, i64)>> {
    breadcrumbs::table
        .find(character)
        .filter(breadcrumbs::userId.eq(user))
        .select((breadcrumbs::breadcrumbs_, breadcrumbs::updateVersion))
        .first(conn)
        .optional()
}

/// `GetBreadcrumbsForCharacterIdAndUserId`: `{breadcrumbs, updateVersion}`, created empty on first use.
pub fn get_breadcrumbs(conn: &mut Conn, user: &str, character: &str) -> Result<Value, Fail> {
    let stored = stored_breadcrumbs(conn, user, character)?;
    if stored.is_none() {
        tracing::info!("Creating new breadcrumbs entry for character {character}");
        diesel::insert_into(breadcrumbs::table)
            .values((
                breadcrumbs::breadcrumbs_.eq("[]"),
                breadcrumbs::updateVersion.eq(0),
                breadcrumbs::userId.eq(user),
                breadcrumbs::characterId.eq(character),
            ))
            .execute(conn)?;
    }
    let (text, version) = stored.unwrap_or_else(|| ("[]".into(), 0));
    let list = parse(
        &text,
        &format!("Invalid breadcrumb data for characterId {character} and userId {user}"),
    )?;
    Ok(json!({"breadcrumbs": list, "updateVersion": version}))
}

/// The version a request sent, as SQLite would store it: an integer, or digits in a string. `None`
/// for a missing one; anything else can't be stored (Node's bind or NOT NULL failure).
fn version_of(value: Option<&Value>) -> Result<Option<i64>, ()> {
    match value {
        None => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or(()),
        Some(Value::String(text)) => text.trim().parse::<i64>().map(Some).map_err(|_| ()),
        _ => Err(()),
    }
}

/// `SetBreadcrumbsForCharacterIdAndUserId`: stored when the row is new or the version is newer;
/// a version at or below the stored one is a conflict.
pub fn set_breadcrumbs(
    conn: &mut Conn,
    user: &str,
    character: &str,
    list: Option<&Value>,
    version: Option<&Value>,
) -> Result<Value, Fail> {
    let failed = || {
        Fail::Db(format!(
            "Failed to set breadcrumbs for characterId {character} and userId {user}"
        ))
    };
    let stored = stored_breadcrumbs(conn, user, character)?;
    let number = version_of(version).map_err(|()| failed())?;
    match stored {
        None => {
            tracing::info!("Creating new breadcrumbs entry for character {character}");
            let (Some(list), Some(number)) = (list, number) else {
                return Err(failed());
            };
            diesel::insert_into(breadcrumbs::table)
                .values((
                    breadcrumbs::breadcrumbs_.eq(service_kit::jsjson::stringify(list)),
                    breadcrumbs::updateVersion.eq(number),
                    breadcrumbs::userId.eq(user),
                    breadcrumbs::characterId.eq(character),
                ))
                .execute(conn)?;
        }
        Some((_, current)) if number.is_some_and(|n| current >= n) => {
            return Err(Fail::Conflict(format!(
                "Refusing stale breadcrumbs update for characterId {character} and userId {user}: current updateVersion {current}, incoming updateVersion {}",
                js::string(version)
            )));
        }
        Some(_) => {
            tracing::info!(
                "Updating breadcrumbs entry for character {character} with updateVersion {}",
                js::string(version)
            );
            let target = breadcrumbs::table.find(character).filter(breadcrumbs::userId.eq(user));
            let text = list.map(service_kit::jsjson::stringify);
            // A missing field leaves the stored one, as Drizzle leaves out undefined values (and
            // refuses an update with none)
            match (text, number) {
                (Some(text), Some(number)) => diesel::update(target)
                    .set((
                        breadcrumbs::breadcrumbs_.eq(text),
                        breadcrumbs::updateVersion.eq(number),
                    ))
                    .execute(conn)?,
                (Some(text), None) => diesel::update(target)
                    .set(breadcrumbs::breadcrumbs_.eq(text))
                    .execute(conn)?,
                (None, Some(number)) => diesel::update(target)
                    .set(breadcrumbs::updateVersion.eq(number))
                    .execute(conn)?,
                (None, None) => return Err(failed()),
            };
        }
    }
    let mut reply = serde_json::Map::new();
    if let Some(list) = list {
        reply.insert("breadcrumbs".into(), list.clone());
    }
    if let Some(version) = version {
        reply.insert("updateVersion".into(), version.clone());
    }
    Ok(Value::Object(reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_equality_like_javascript() {
        assert!(loose_eq(Some(&json!("5")), Some(&json!(5))));
        assert!(loose_eq(None, Some(&Value::Null)));
        assert!(!loose_eq(Some(&json!(0)), Some(&Value::Null)));
        assert!(loose_eq(Some(&json!(true)), Some(&json!(1))));
        assert!(!loose_eq(Some(&json!([1])), Some(&json!([1]))));
        assert!(!loose_eq(Some(&json!("a")), Some(&json!("b"))));
    }
}
