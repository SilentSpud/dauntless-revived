//! Characters and their save history (`controllers/character.ts`, `controllers/activecharacter.ts`,
//! `controllers/savehistory.ts`).
//!
//! A character's data is a JSON object the client saves whole, with an `updateVersion` that only
//! grows: the read, the version check, the write and the history entry are one transaction, so two
//! saves racing for the same version can't both win. Every save is kept in `characterhistory` (and
//! loadouts in `loadouthistory`) within the retention settings, so an admin can roll back.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use serde_json::{Value, json};
use service_kit::collate::locale_compare;
use service_kit::jsjson;

use crate::crypto::random_uuid;
use crate::schema::{characterhistory, characters, loadouthistory, loadouts, users};
use crate::time::iso_now;

pub type Conn = store::Connection;

const FTUE_SERIE_KEY: &str = "SERIE_cr19_series_1_ftue";
const FTUE_DOJO_UNLOCK_OBJECTIVE: &str = "929A333B40E413C41E47B0A425EC3349";
const SERIE_DOJO: &str = "{\"ID\":\"Dojo\",\"Status\":0,\"62B91BD94558409B4F7352B5B96F3ED7\":{\"Status\":0,\"6CA2C43B46334BC06F73DEB5F2BFFEC1\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0},\"3A7241AA43743647D3C1E39E8976E4F3\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0}},\"816CBFD94D16EDA252BD1D8461209568\":{\"Status\":1},\"B152371947599B3C2D55BE9B91439C37\":{\"Status\":0,\"D3F19E2248AECEF5C5C3C8B9E2AC2C67\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0},\"0407C2134FE0BEFE3EC791999632D2BC\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0}},\"DFE54F884C6FC60688B6C494D79ADD29\":{\"Status\":0,\"9D1B0D754DBC896034F942AD625F9D93\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0}}}";

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = characters)]
#[allow(non_snake_case)]
pub struct CharacterRow {
    pub characterId: String,
    pub userId: String,
    pub createdDate: String,
    pub lastModifiedDate: String,
    pub name: String,
    pub updateVersion: i64,
    pub data: String,
}

impl CharacterRow {
    /// The character as the client reads it (`TransformDbCharacterToWireCharacter`).
    pub fn wire(&self) -> Value {
        json!({
            "accountId": self.userId,
            "createdDate": self.createdDate,
            "data": self.data,
            "id": self.characterId,
            "lastModifiedDate": self.lastModifiedDate,
            "name": self.name,
            "updateVersion": self.updateVersion,
        })
    }
}

/// `FormatCharacterDate`: the server's local date as `Oct 20, 2026`.
pub fn character_date() -> String {
    Local::now().format("%b %-d, %Y").to_string()
}

/// `DoesCharacterBelongToUserId`.
pub fn belongs(conn: &mut Conn, user_id: &str, character_id: &str) -> QueryResult<bool> {
    Ok(characters::table
        .filter(characters::characterId.eq(character_id))
        .filter(characters::userId.eq(user_id))
        .select(characters::characterId)
        .first::<String>(conn)
        .optional()?
        .is_some())
}

/// `CreateCharacterForUid`: an empty character (`{}`) at version 0.
pub fn create(conn: &mut Conn, user_id: &str, name: &str) -> QueryResult<CharacterRow> {
    let date = character_date();
    diesel::insert_into(characters::table)
        .values((
            characters::characterId.eq(random_uuid()),
            characters::userId.eq(user_id),
            characters::name.eq(name),
            characters::createdDate.eq(&date),
            characters::lastModifiedDate.eq(&date),
            characters::updateVersion.eq(0),
            characters::data.eq("{}"),
        ))
        .returning(CharacterRow::as_returning())
        .get_result(conn)
}

pub fn owned(conn: &mut Conn, user_id: &str) -> QueryResult<Vec<CharacterRow>> {
    characters::table
        .filter(characters::userId.eq(user_id))
        .select(CharacterRow::as_select())
        .load(conn)
}

/// The failures of character reads and writes that are not database errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacterError {
    /// The account has no `users` row (Node dereferences the missing row: a 500).
    NoAccount,
    NotFound,
    Conflict(i64),
    InvalidData(String),
}

/// `GetCharactersForUid`: a new account gets its first character, named after the account.
pub fn for_user(conn: &mut Conn, user_id: &str) -> QueryResult<Result<Vec<CharacterRow>, CharacterError>> {
    let mut rows = owned(conn, user_id)?;
    if rows.is_empty() {
        let Some(name) = users::table
            .find(user_id)
            .select(users::name)
            .first::<String>(conn)
            .optional()?
        else {
            return Ok(Err(CharacterError::NoAccount));
        };
        create(conn, user_id, &name)?;
        rows = owned(conn, user_id)?;
    }
    Ok(Ok(rows))
}

pub fn find(conn: &mut Conn, character_id: &str, user_id: &str) -> QueryResult<Option<CharacterRow>> {
    characters::table
        .filter(characters::characterId.eq(character_id))
        .filter(characters::userId.eq(user_id))
        .select(CharacterRow::as_select())
        .first(conn)
        .optional()
}

/// `ProcessTriggers`: the saved data must be a JSON object; it is stored as `JSON.stringify` writes
/// it, with `SERIE_dojo` added once the FTUE's Dojo objective is done.
pub fn process_triggers(data: &str) -> Result<String, CharacterError> {
    let mut parsed = jsjson::parse(data)
        .map_err(|e| CharacterError::InvalidData(format!("Character data is not valid JSON: {e}")))?;
    let Value::Object(object) = &mut parsed else {
        return Err(CharacterError::InvalidData(
            "Character data is not a JSON object".into(),
        ));
    };
    let unlocked = match object.get(FTUE_SERIE_KEY) {
        Some(Value::String(serie)) => match jsjson::parse(serie) {
            Ok(serie) => {
                serie
                    .get(FTUE_DOJO_UNLOCK_OBJECTIVE)
                    .and_then(|o| o.get("Status"))
                    .and_then(Value::as_f64)
                    == Some(3.0)
            }
            Err(error) => {
                tracing::warn!("Skipping the SERIE_dojo trigger, {FTUE_SERIE_KEY} is unreadable: {error}");
                false
            }
        },
        _ => false,
    };
    if unlocked && object.get("SERIE_dojo").is_none_or(Value::is_null) {
        tracing::info!("Injecting SERIE_dojo!");
        object.insert("SERIE_dojo".into(), SERIE_DOJO.into());
    }
    Ok(jsjson::stringify(&parsed))
}

/// `UpdateCharacterForUid`, inside the caller's write transaction.
pub fn update(
    conn: &mut Conn,
    history: &HistorySettings,
    character_id: Option<&str>,
    user_id: Option<&str>,
    data: Option<&Value>,
    version: i64,
    reason: &str,
) -> QueryResult<Result<(), CharacterError>> {
    let current = match (character_id, user_id) {
        (Some(c), Some(u)) => find(conn, c, u)?,
        _ => None,
    };
    let Some(current) = current else {
        return Ok(Err(CharacterError::NotFound));
    };
    // A lost reply can make the world retry an already committed save: an identical snapshot at the
    // exact stored version is acknowledged.
    if current.updateVersion == version
        && let Some(Value::String(data)) = data
    {
        let processed = match process_triggers(data) {
            Ok(processed) => processed,
            Err(error) => return Ok(Err(error)),
        };
        if let (Ok(stored), Ok(incoming)) = (jsjson::parse(&current.data), jsjson::parse(&processed))
            && jsjson::deep_equal(&stored, &incoming)
        {
            return Ok(Ok(()));
        }
    }
    if current.updateVersion >= version {
        return Ok(Err(CharacterError::Conflict(current.updateVersion)));
    }
    let Some(Value::String(data)) = data else {
        let kind = match data {
            None => "undefined",
            Some(Value::Null) => "null",
            Some(Value::Bool(_)) => "boolean",
            Some(Value::Number(_)) => "number",
            _ => "object",
        };
        return Ok(Err(CharacterError::InvalidData(format!(
            "Character data is a {kind}, not a string"
        ))));
    };
    let processed = match process_triggers(data) {
        Ok(processed) => processed,
        Err(error) => return Ok(Err(error)),
    };
    ensure_character_baseline(conn, &current)?;
    let updated = diesel::update(
        characters::table
            .filter(characters::userId.eq(&current.userId))
            .filter(characters::characterId.eq(&current.characterId))
            .filter(characters::updateVersion.lt(version)),
    )
    .set((
        characters::data.eq(&processed),
        characters::updateVersion.eq(version),
        characters::lastModifiedDate.eq(character_date()),
    ))
    .returning(CharacterRow::as_returning())
    .get_result(conn)
    .optional()?;
    let Some(updated) = updated else {
        return Ok(Err(CharacterError::Conflict(current.updateVersion)));
    };
    record_character_version(conn, history, &updated, reason)?;
    Ok(Ok(()))
}

/// `GetActiveCharacter`: the character saved last (history ids only grow), then the later
/// `lastModifiedDate`, then the higher version, then the id in `localeCompare` order.
pub fn active(conn: &mut Conn, account: &str) -> QueryResult<Option<CharacterRow>> {
    let mut owned = owned(conn, account)?;
    if owned.len() <= 1 {
        return Ok(owned.pop());
    }
    let ids: Vec<&String> = owned.iter().map(|row| &row.characterId).collect();
    let latest: Vec<(String, Option<i64>)> = characterhistory::table
        .filter(characterhistory::characterId.eq_any(ids))
        .group_by(characterhistory::characterId)
        .select((characterhistory::characterId, diesel::dsl::max(characterhistory::id)))
        .load(conn)?;
    let save = |id: &str| latest.iter().find(|(c, _)| c == id).and_then(|(_, n)| *n).unwrap_or(0);
    let day = |text: &str| {
        NaiveDate::parse_from_str(text, "%b %d, %Y")
            .map(|d| d.num_days_from_ce() as i64)
            .unwrap_or(0)
    };
    owned.sort_by(|a, b| {
        save(&b.characterId)
            .cmp(&save(&a.characterId))
            .then(day(&b.lastModifiedDate).cmp(&day(&a.lastModifiedDate)))
            .then(b.updateVersion.cmp(&a.updateVersion))
            .then_with(|| locale_compare(&a.characterId, &b.characterId))
    });
    Ok(owned.into_iter().next())
}

/// How much save history is kept: `SAVE_HISTORY_KEEP` newest versions (100), plus the last version of
/// each hour for `SAVE_HISTORY_HOURLY` hours (48) and of each day for `SAVE_HISTORY_DAILY` days (30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistorySettings {
    pub keep: i64,
    pub hourly: i64,
    pub daily: i64,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            keep: 100,
            hourly: 48,
            daily: 30,
        }
    }
}

impl HistorySettings {
    pub fn from_env(env: &service_kit::env::Env) -> Self {
        let number = |name: &str| {
            env.raw(name)
                .map(|text| text.trim().parse::<f64>().ok().filter(|n| n.fract() == 0.0))
        };
        let keep = match number("SAVE_HISTORY_KEEP") {
            Some(Some(n)) if n >= 2.0 => n as i64,
            _ => 100,
        };
        let retention = |name: &str, default: i64| match (env.raw(name), number(name)) {
            (Some(text), Some(Some(n))) if !text.is_empty() && n >= 0.0 => n as i64,
            _ => default,
        };
        Self {
            keep,
            hourly: retention("SAVE_HISTORY_HOURLY", 48),
            daily: retention("SAVE_HISTORY_DAILY", 30),
        }
    }
}

fn iso(when: DateTime<Utc>) -> String {
    when.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// `PruneBeyondRetention`: older than the kept versions and not the last of its hour or day inside the
/// windows. `savedDate` is ISO (UTC), so its first 13 characters are the hour and 10 the day.
fn prune_beyond_retention(
    conn: &mut Conn,
    table: &str,
    character_id: &str,
    oldest_kept: i64,
    history: &HistorySettings,
) -> QueryResult<()> {
    let now = Utc::now();
    let hourly_since = iso(now - Duration::hours(history.hourly));
    let daily_since = iso(now - Duration::days(history.daily));
    diesel::sql_query(format!(
        "delete from {table} where characterId = ? and id < ?
           and id not in (select max(id) from {table} where characterId = ? and savedDate >= ? group by substr(savedDate, 1, 13))
           and id not in (select max(id) from {table} where characterId = ? and savedDate >= ? group by substr(savedDate, 1, 10))"
    ))
    .bind::<Text, _>(character_id)
    .bind::<BigInt, _>(oldest_kept)
    .bind::<Text, _>(character_id)
    .bind::<Text, _>(hourly_since)
    .bind::<Text, _>(character_id)
    .bind::<Text, _>(daily_since)
    .execute(conn)
    .map(drop)
}

fn insert_character_version(conn: &mut Conn, row: &CharacterRow, reason: &str) -> QueryResult<()> {
    diesel::insert_into(characterhistory::table)
        .values((
            characterhistory::characterId.eq(&row.characterId),
            characterhistory::userId.eq(&row.userId),
            characterhistory::updateVersion.eq(row.updateVersion),
            characterhistory::name.eq(&row.name),
            characterhistory::data.eq(&row.data),
            characterhistory::savedDate.eq(iso_now()),
            characterhistory::reason.eq(reason),
        ))
        .execute(conn)
        .map(drop)
}

/// `EnsureCharacterBaseline`: the row as it is before a write, unless history already has it.
pub fn ensure_character_baseline(conn: &mut Conn, current: &CharacterRow) -> QueryResult<()> {
    let latest: Option<i64> = characterhistory::table
        .filter(characterhistory::characterId.eq(&current.characterId))
        .order(characterhistory::id.desc())
        .select(characterhistory::updateVersion)
        .first(conn)
        .optional()?;
    if latest != Some(current.updateVersion) {
        insert_character_version(conn, current, "baseline")?;
    }
    Ok(())
}

pub fn record_character_version(
    conn: &mut Conn,
    history: &HistorySettings,
    row: &CharacterRow,
    reason: &str,
) -> QueryResult<()> {
    insert_character_version(conn, row, reason)?;
    let oldest: Option<i64> = characterhistory::table
        .filter(characterhistory::characterId.eq(&row.characterId))
        .order(characterhistory::id.desc())
        .select(characterhistory::id)
        .offset(history.keep - 1)
        .first(conn)
        .optional()?;
    if let Some(oldest) = oldest {
        prune_beyond_retention(conn, "characterhistory", &row.characterId, oldest, history)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = loadouts)]
#[allow(non_snake_case)]
pub struct LoadoutRow {
    pub characterId: String,
    pub userId: String,
    pub loadouts_: String,
    pub persistent: String,
}

fn latest_loadout_version(conn: &mut Conn, character_id: &str) -> QueryResult<Option<(i64, String, String)>> {
    loadouthistory::table
        .filter(loadouthistory::characterId.eq(character_id))
        .order(loadouthistory::id.desc())
        .select((
            loadouthistory::version,
            loadouthistory::loadouts,
            loadouthistory::persistent,
        ))
        .first(conn)
        .optional()
}

fn insert_loadout_version(conn: &mut Conn, row: &LoadoutRow, version: i64, reason: &str) -> QueryResult<()> {
    diesel::insert_into(loadouthistory::table)
        .values((
            loadouthistory::characterId.eq(&row.characterId),
            loadouthistory::userId.eq(&row.userId),
            loadouthistory::version.eq(version),
            loadouthistory::loadouts.eq(&row.loadouts_),
            loadouthistory::persistent.eq(&row.persistent),
            loadouthistory::savedDate.eq(iso_now()),
            loadouthistory::reason.eq(reason),
        ))
        .execute(conn)
        .map(drop)
}

/// `EnsureLoadoutBaseline`.
pub fn ensure_loadout_baseline(conn: &mut Conn, current: &LoadoutRow) -> QueryResult<()> {
    let latest = latest_loadout_version(conn, &current.characterId)?;
    let same = latest
        .as_ref()
        .is_some_and(|(_, l, p)| *l == current.loadouts_ && *p == current.persistent);
    if !same {
        insert_loadout_version(conn, current, latest.map_or(0, |(v, _, _)| v) + 1, "baseline")?;
    }
    Ok(())
}

/// `RecordLoadoutVersion`.
pub fn record_loadout_version(
    conn: &mut Conn,
    history: &HistorySettings,
    row: &LoadoutRow,
    reason: &str,
) -> QueryResult<()> {
    let latest = latest_loadout_version(conn, &row.characterId)?;
    insert_loadout_version(conn, row, latest.map_or(0, |(v, _, _)| v) + 1, reason)?;
    let oldest: Option<i64> = loadouthistory::table
        .filter(loadouthistory::characterId.eq(&row.characterId))
        .order(loadouthistory::id.desc())
        .select(loadouthistory::id)
        .offset(history.keep - 1)
        .first(conn)
        .optional()?;
    if let Some(oldest) = oldest {
        prune_beyond_retention(conn, "loadouthistory", &row.characterId, oldest, history)?;
    }
    Ok(())
}

/// `GetSaveHistory`: each existing character's versions, newest first.
pub fn save_history(conn: &mut Conn, character_ids: &[String]) -> QueryResult<Vec<Value>> {
    let mut result = Vec::new();
    for id in character_ids {
        let Some(character) = characters::table
            .find(id)
            .select(CharacterRow::as_select())
            .first::<CharacterRow>(conn)
            .optional()?
        else {
            continue;
        };
        let versions: Vec<(i64, String, String, Option<i64>)> = characterhistory::table
            .filter(characterhistory::characterId.eq(id))
            .order(characterhistory::id.desc())
            .select((
                characterhistory::updateVersion,
                characterhistory::savedDate,
                characterhistory::reason,
                diesel::dsl::sql::<diesel::sql_types::Nullable<BigInt>>("length(data)"),
            ))
            .load(conn)?;
        let loadout_versions: Vec<(i64, String, String)> = loadouthistory::table
            .filter(loadouthistory::characterId.eq(id))
            .order(loadouthistory::id.desc())
            .select((
                loadouthistory::version,
                loadouthistory::savedDate,
                loadouthistory::reason,
            ))
            .load(conn)?;
        result.push(json!({
            "CharacterId": character.characterId,
            "UserId": character.userId,
            "Name": character.name,
            "UpdateVersion": character.updateVersion,
            "CharacterVersions": versions.into_iter().map(|(v, d, r, s)| json!({"Version": v, "SavedDate": d, "Reason": r, "Size": s})).collect::<Vec<_>>(),
            "LoadoutVersions": loadout_versions.into_iter().map(|(v, d, r)| json!({"Version": v, "SavedDate": d, "Reason": r})).collect::<Vec<_>>(),
        }));
    }
    Ok(result)
}

pub fn character_ids_of(conn: &mut Conn, user_id: &str) -> QueryResult<Vec<String>> {
    characters::table
        .filter(characters::userId.eq(user_id))
        .select(characters::characterId)
        .load(conn)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackError {
    NotFound,
    VersionNotFound,
    Conflict,
}

impl RollbackError {
    pub fn status(self) -> u16 {
        match self {
            Self::NotFound | Self::VersionNotFound => 404,
            Self::Conflict => 409,
        }
    }
}

/// `RollbackCharacter`: the data of history version `target` back, under a new version above any
/// seen so far, so a stale save still in flight is refused instead of undoing the rollback.
pub fn rollback_character(
    conn: &mut Conn,
    history: &HistorySettings,
    character_id: &str,
    target: i64,
    admin: &str,
) -> QueryResult<Result<Value, RollbackError>> {
    let Some(current) = characters::table
        .find(character_id)
        .select(CharacterRow::as_select())
        .first::<CharacterRow>(conn)
        .optional()?
    else {
        tracing::warn!("Rollback refused: characterId {character_id} does not exist");
        return Ok(Err(RollbackError::NotFound));
    };
    let Some(data) = characterhistory::table
        .filter(characterhistory::characterId.eq(character_id))
        .filter(characterhistory::updateVersion.eq(target))
        .order(characterhistory::id.desc())
        .select(characterhistory::data)
        .first::<String>(conn)
        .optional()?
    else {
        tracing::warn!("Rollback refused: characterId {character_id} has no saved version {target}");
        return Ok(Err(RollbackError::VersionNotFound));
    };
    ensure_character_baseline(conn, &current)?;
    let highest: Option<i64> = characterhistory::table
        .filter(characterhistory::characterId.eq(character_id))
        .select(diesel::dsl::max(characterhistory::updateVersion))
        .first(conn)?;
    let new_version = current.updateVersion.max(highest.unwrap_or(0)) + 1;
    let updated = diesel::update(
        characters::table
            .filter(characters::characterId.eq(character_id))
            .filter(characters::updateVersion.eq(current.updateVersion)),
    )
    .set((
        characters::data.eq(&data),
        characters::updateVersion.eq(new_version),
        characters::lastModifiedDate.eq(character_date()),
    ))
    .returning(CharacterRow::as_returning())
    .get_result(conn)
    .optional()?;
    let Some(updated) = updated else {
        tracing::warn!("Rollback refused: characterId {character_id} changed while rolling back");
        return Ok(Err(RollbackError::Conflict));
    };
    record_character_version(conn, history, &updated, &format!("rollback:{target}"))?;
    tracing::warn!(
        "Admin {admin} rolled back characterId {character_id} of userId {} to version {target} (was updateVersion {}, now {new_version})",
        current.userId,
        current.updateVersion
    );
    Ok(Ok(
        json!({"CharacterId": character_id, "RolledBackTo": target, "PreviousVersion": current.updateVersion, "UpdateVersion": new_version}),
    ))
}

/// `RollbackLoadout`: the loadouts of loadout version `target` back.
pub fn rollback_loadout(
    conn: &mut Conn,
    history: &HistorySettings,
    character_id: &str,
    target: i64,
    admin: &str,
) -> QueryResult<Result<Value, RollbackError>> {
    let Some(current) = loadouts::table
        .find(character_id)
        .select(LoadoutRow::as_select())
        .first::<LoadoutRow>(conn)
        .optional()?
    else {
        tracing::warn!("Loadout rollback refused: characterId {character_id} has no loadouts");
        return Ok(Err(RollbackError::NotFound));
    };
    let Some((target_loadouts, target_persistent)) = loadouthistory::table
        .filter(loadouthistory::characterId.eq(character_id))
        .filter(loadouthistory::version.eq(target))
        .order(loadouthistory::id.desc())
        .select((loadouthistory::loadouts, loadouthistory::persistent))
        .first::<(String, String)>(conn)
        .optional()?
    else {
        tracing::warn!("Loadout rollback refused: characterId {character_id} has no saved loadout version {target}");
        return Ok(Err(RollbackError::VersionNotFound));
    };
    ensure_loadout_baseline(conn, &current)?;
    let updated: LoadoutRow = diesel::update(loadouts::table.find(character_id))
        .set((
            loadouts::loadouts_.eq(&target_loadouts),
            loadouts::persistent.eq(&target_persistent),
        ))
        .returning(LoadoutRow::as_returning())
        .get_result(conn)?;
    record_loadout_version(conn, history, &updated, &format!("rollback:{target}"))?;
    let version = latest_loadout_version(conn, character_id)?.map_or(0, |(v, _, _)| v);
    tracing::warn!(
        "Admin {admin} rolled back the loadouts of characterId {character_id} of userId {} to loadout version {target} (now loadout version {version})",
        current.userId
    );
    Ok(Ok(
        json!({"CharacterId": character_id, "RolledBackTo": target, "Version": version}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_rewrite_like_json_stringify_and_inject_the_dojo() {
        assert_eq!(
            process_triggers(r#"{"a":1.0,"b":[2e0]}"#).unwrap(),
            r#"{"a":1,"b":[2]}"#
        );
        assert!(matches!(process_triggers("[1]"), Err(CharacterError::InvalidData(_))));
        assert!(matches!(process_triggers("nope"), Err(CharacterError::InvalidData(_))));
        let ftue = serde_json::json!({FTUE_SERIE_KEY: serde_json::json!({FTUE_DOJO_UNLOCK_OBJECTIVE: {"Status": 3}}).to_string()});
        let processed: Value = serde_json::from_str(&process_triggers(&ftue.to_string()).unwrap()).unwrap();
        assert_eq!(processed["SERIE_dojo"], SERIE_DOJO);
        let bad = serde_json::json!({FTUE_SERIE_KEY: "{not json"});
        assert!(
            process_triggers(&bad.to_string()).is_ok(),
            "an unreadable trigger input skips the trigger"
        );
    }

    #[test]
    fn character_dates_look_like_to_locale_date_string() {
        let date = character_date();
        assert!(NaiveDate::parse_from_str(&date, "%b %d, %Y").is_ok(), "{date}");
    }
}
