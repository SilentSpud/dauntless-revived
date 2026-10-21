// Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only; modified for Dauntless Revived.

//! Escalation seasons of real-mode accounts with `ESCALATION_MODE=real` (`controllers/escalation.ts`,
//! `escalationConfig.ts`): GetSeasonalEscalationEndpoint and UpdateSeasonalEscalationEndpoint, both
//! `/escalation/{season_id}/{account_id}` (`routes/escalation.rs`).
//!
//! The world server levels players, spends talent points and grants rewards natively, then POSTs the
//! whole season. The backend keeps the last legitimate snapshot and refuses one that no native state
//! could produce, or that would erase newer progress.
//!
//! Two kinds of rules: hard rules, always enforced (known season, ids and ranks in range, the version
//! order, nothing lowered or un-collected, no stub values); soft rules, which depend on how the
//! client's tier gates, level costs and reward levels were modelled: `ESCALATION_STRICT=1` refuses a
//! save that breaks one (409); by default it is stored and logged. Every save, accepted or not, is a
//! row in progression_events.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use diesel::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use service_kit::env::Env;

use crate::loadouts::RealReply;
use crate::progression::{Caller, Event, account_exists, record_event};
use crate::schema::{escalationprogression, escalationtalents, escalationunlocks};
use crate::time::iso_now;

pub type Conn = store::Connection;

const INT32_MAX: f64 = 2_147_483_647.0;
/// The old stub answered level 99999 (clamped natively to the last level) and 99999 XP.
const STUB_XP: i64 = 99999;
const ROUTE: &str = "POST /escalation/:season/:uid";

// ---- Settings ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    /// `ESCALATION_MODE=real` (default stub).
    pub real: bool,
    /// `ESCALATION_STRICT` (on/off, default off).
    pub strict: bool,
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let real = match env.raw("ESCALATION_MODE").map(str::trim).filter(|v| !v.is_empty()) {
            None => false,
            Some(value) => match value.to_lowercase().as_str() {
                "real" => true,
                "stub" => false,
                _ => {
                    tracing::warn!("ESCALATION_MODE={value:?} is not a valid value; using the default (stub)");
                    false
                }
            },
        };
        Self {
            real,
            strict: crate::config::on_off(env, "ESCALATION_STRICT", false),
        }
    }
}

// ---- The season registry ----

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Talent {
    pub id: String,
    pub points_to_unlock: i64,
    pub rank_costs: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unlock {
    pub id: String,
    pub unlock_level: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Level {
    pub required_experience: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Season {
    pub id: String,
    pub enabled: bool,
    pub levels: Vec<Level>,
    pub talents: Vec<Talent>,
    pub unlocks: Vec<Unlock>,
}

impl Season {
    pub fn max_level(&self) -> i64 {
        self.levels.len() as i64
    }

    /// Cost of going from `level` to `level + 1`, or `None` at the cap.
    fn next_cost(&self, level: i64) -> Option<i64> {
        self.levels.get(level as usize).map(|l| l.required_experience)
    }
}

/// `GetSpentTalentPoints`: the sum of the first `rank` rank costs.
fn rank_cost(talent: &Talent, rank: i64) -> i64 {
    talent.rank_costs.iter().take(rank.max(0) as usize).sum()
}

/// The registry, exported from the installed client's own tables (`vendor/escalation/seasons.json`;
/// checked against the same rules as Node by the test below).
pub fn seasons() -> &'static [Season] {
    static SEASONS: OnceLock<Vec<Season>> = OnceLock::new();
    SEASONS.get_or_init(|| {
        crate::vendor::escalation_seasons()
            .get("seasons")
            .cloned()
            .and_then(|s| serde_json::from_value(s).ok())
            .unwrap_or_default()
    })
}

/// Unknown ids never fall back to another season.
pub fn season(id: &str) -> Option<&'static Season> {
    seasons().iter().find(|s| s.id == id)
}

// ---- Reading ----

fn envelope(payload: Value) -> Value {
    json!({"code": null, "message": "OK", "payload": payload})
}

fn refusal_body(status: u16, message: &str) -> Value {
    json!({"code": status.to_string(), "message": message, "payload": null})
}

#[derive(Debug, Clone, Queryable)]
struct Row {
    #[allow(dead_code)] // read with the row
    account_id: String,
    #[allow(dead_code)] // read with the row
    season_id: String,
    level: i64,
    xp: i64,
    update_version: i64,
    content_hash: String,
    #[allow(dead_code)] // read with the row
    updated_date: String,
}

fn season_row(conn: &mut Conn, account: &str, season: &str) -> QueryResult<Option<Row>> {
    escalationprogression::table
        .find((account, season))
        .first(conn)
        .optional()
}

fn collected_ids(conn: &mut Conn, account: &str, season: &str) -> QueryResult<Vec<String>> {
    escalationunlocks::table
        .filter(escalationunlocks::accountId.eq(account))
        .filter(escalationunlocks::seasonId.eq(season))
        .select(escalationunlocks::unlockId)
        .load(conn)
}

/// The native default for an account with no stored season is level 0, no XP, nothing bought or
/// collected, version 0. Reads never create rows; talents and unlocks follow the season table's
/// order, so identical state always serialises identically.
fn read_state(conn: &mut Conn, account: &str, season: &Season) -> QueryResult<Value> {
    let row = season_row(conn, account, &season.id)?;
    let ranks: HashMap<String, i64> = escalationtalents::table
        .filter(escalationtalents::accountId.eq(account))
        .filter(escalationtalents::seasonId.eq(&season.id))
        .select((escalationtalents::talentId, escalationtalents::rank))
        .load::<(String, i64)>(conn)?
        .into_iter()
        .collect();
    let collected: HashSet<String> = collected_ids(conn, account, &season.id)?.into_iter().collect();
    Ok(json!({
        "escalation_level": row.as_ref().map_or(0, |r| r.level),
        "next_level_xp": row.as_ref().map_or(0, |r| r.xp),
        "talents_progress": season
            .talents
            .iter()
            .filter_map(|t| ranks.get(&t.id).map(|rank| json!({"rank": rank, "talent_id": t.id})))
            .collect::<Vec<_>>(),
        "unlock_progress": season
            .unlocks
            .iter()
            .filter(|u| collected.contains(&u.id))
            .map(|u| json!({"collected": true, "reward_id": u.id}))
            .collect::<Vec<_>>(),
        "update_version": row.as_ref().map_or(0, |r| r.update_version),
    }))
}

/// GET /escalation/:season/:uid in real mode: 404 for a season the client does not have.
pub fn read_reply(conn: &mut Conn, account: &str, season_id: &str) -> QueryResult<RealReply> {
    let Some(season) = season(season_id) else {
        tracing::warn!("Escalation read of unknown season {season_id} for {account}");
        return Ok((
            404,
            Some(refusal_body(404, &format!("Unknown escalation season {season_id}"))),
        ));
    };
    let state = read_state(conn, account, season)?;
    tracing::info!(
        "Escalation {season_id} read for {account}: level {}, v{}",
        state["escalation_level"],
        state["update_version"]
    );
    Ok((200, Some(envelope(state))))
}

// ---- Saving ----

/// `IsInt32Count`: a safe integer from 0 to int32's maximum.
fn int32_count(value: Option<&Value>) -> Option<i64> {
    let n = value?.as_f64()?;
    (n.fract() == 0.0 && (0.0..=INT32_MAX).contains(&n)).then_some(n as i64)
}

/// A snapshot in canonical form: known ids only, season order, rank-0 talents dropped, only
/// collected unlocks kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub level: i64,
    pub xp: i64,
    pub talents: Vec<(String, i64)>,
    pub unlocks: Vec<String>,
    pub version: i64,
}

impl Snapshot {
    fn content(&self) -> Map<String, Value> {
        let mut content = Map::new();
        content.insert("escalation_level".into(), json!(self.level));
        content.insert("next_level_xp".into(), json!(self.xp));
        content.insert(
            "talents_progress".into(),
            Value::Array(
                self.talents
                    .iter()
                    .map(|(id, rank)| json!({"rank": rank, "talent_id": id}))
                    .collect(),
            ),
        );
        content.insert(
            "unlock_progress".into(),
            Value::Array(
                self.unlocks
                    .iter()
                    .map(|id| json!({"collected": true, "reward_id": id}))
                    .collect(),
            ),
        );
        content
    }

    /// `HashSnapshot`: everything but the version.
    fn hash(&self) -> String {
        crate::crypto::sha256_hex(&service_kit::jsjson::stringify(&Value::Object(self.content())))
    }
}

/// `CanonicaliseSnapshot`: the snapshot and the soft rules it breaks; `Err` is a hard-rule refusal
/// (400).
pub fn canonicalise(season: &Season, body: Option<&Value>) -> Result<(Snapshot, Vec<String>), String> {
    let Some(Value::Object(body)) = body else {
        return Err("Escalation snapshot must be an object".into());
    };
    let mut soft = Vec::new();
    let max = season.max_level();
    let Some(level) = int32_count(body.get("escalation_level")).filter(|l| *l <= max) else {
        return Err(format!("escalation_level must be an integer from 0 to {max}"));
    };
    let Some(xp) = int32_count(body.get("next_level_xp")) else {
        return Err("next_level_xp must be a non-negative int32".into());
    };
    // The native counter is incremented before every POST, so a write is at least version 1
    let Some(version) = int32_count(body.get("update_version")).filter(|v| *v >= 1) else {
        return Err("update_version must be a positive int32".into());
    };
    // Soft: after GrantExperience the remainder is below the next level's cost
    if let Some(cost) = season.next_cost(level)
        && xp >= cost
    {
        soft.push(format!("next_level_xp {xp} is not below the level {level} cost {cost}"));
    }
    let (Some(Value::Array(talents)), Some(Value::Array(unlocks))) =
        (body.get("talents_progress"), body.get("unlock_progress"))
    else {
        return Err("talents_progress and unlock_progress must be arrays".into());
    };
    let mut ranks: HashMap<&str, i64> = HashMap::new();
    for entry in talents {
        let id = entry.get("talent_id");
        let Some(talent) = season
            .talents
            .iter()
            .find(|t| id.and_then(Value::as_str) == Some(t.id.as_str()))
        else {
            return Err(format!("Unknown talent {} for {}", crate::js::string(id), season.id));
        };
        if ranks.contains_key(talent.id.as_str()) {
            return Err(format!("Talent {} listed twice", talent.id));
        }
        let Some(rank) = int32_count(entry.get("rank")).filter(|r| *r <= talent.rank_costs.len() as i64) else {
            return Err(format!(
                "Talent {} rank must be 0..{}",
                talent.id,
                talent.rank_costs.len()
            ));
        };
        ranks.insert(&talent.id, rank);
    }
    let rank_of = |t: &Talent| ranks.get(t.id.as_str()).copied().unwrap_or(0);
    // Soft: one point per level
    let spent: i64 = season.talents.iter().map(|t| rank_cost(t, rank_of(t))).sum();
    if spent > level {
        soft.push(format!(
            "talents spend {spent} points but level {level} only earns {level}"
        ));
    }
    // Soft: a rank-up needs the points of the earlier tiers; a talent never pays for its own gate
    for talent in &season.talents {
        if rank_of(talent) == 0 || talent.points_to_unlock == 0 {
            continue;
        }
        let below: i64 = season
            .talents
            .iter()
            .filter(|other| other.points_to_unlock < talent.points_to_unlock)
            .map(|other| rank_cost(other, rank_of(other)))
            .sum();
        if below < talent.points_to_unlock {
            soft.push(format!(
                "talent {} needs {} points spent in earlier tiers, found {below}",
                talent.id, talent.points_to_unlock
            ));
        }
    }
    let (mut listed, mut collected) = (HashSet::new(), HashSet::new());
    for entry in unlocks {
        let id = entry.get("reward_id");
        let Some(unlock) = season
            .unlocks
            .iter()
            .find(|u| id.and_then(Value::as_str) == Some(u.id.as_str()))
        else {
            return Err(format!("Unknown unlock {} for {}", crate::js::string(id), season.id));
        };
        if listed.contains(unlock.id.as_str()) {
            return Err(format!("Unlock {} listed twice", unlock.id));
        }
        let Some(Value::Bool(taken)) = entry.get("collected") else {
            return Err(format!("Unlock {} collected must be a boolean", unlock.id));
        };
        // Soft: a reward is collected at or above its level
        if *taken && level < unlock.unlock_level {
            soft.push(format!(
                "unlock {} is collected at level {level} but needs level {}",
                unlock.id, unlock.unlock_level
            ));
        }
        listed.insert(unlock.id.as_str());
        if *taken {
            collected.insert(unlock.id.as_str());
        }
    }
    Ok((
        Snapshot {
            level,
            xp,
            talents: season
                .talents
                .iter()
                .filter(|t| rank_of(t) > 0)
                .map(|t| (t.id.clone(), rank_of(t)))
                .collect(),
            unlocks: season
                .unlocks
                .iter()
                .filter(|u| collected.contains(u.id.as_str()))
                .map(|u| u.id.clone())
                .collect(),
            version,
        },
        soft,
    ))
}

/// POST /escalation/:season/:uid (game server only). A newer version is accepted if it does not
/// lower progress or un-collect a reward; the stored version with the stored content is a retry,
/// answered with the stored state; the stored version with other content, or an older version, is
/// 409, so a stale or reordered save never overwrites newer state.
pub fn save(
    conn: &mut Conn,
    settings: Settings,
    account: &str,
    season_id: &str,
    body: Option<&Value>,
    caller: Caller,
) -> QueryResult<RealReply> {
    let mut notes = vec![format!("season {season_id}")];
    let refuse = |conn: &mut Conn, notes: &[String], status: u16, why: &str| -> QueryResult<RealReply> {
        let mut all = notes.to_vec();
        all.push(why.to_owned());
        record_event(
            conn,
            Event {
                account,
                caller: caller.as_str(),
                route: ROUTE,
                body,
                status: i64::from(status),
                reply: None,
                notes: &all,
            },
        )?;
        tracing::warn!("Refusing escalation save of {season_id} for {account} ({status}): {why}");
        Ok((status, Some(refusal_body(status, why))))
    };
    if !account_exists(conn, account)? {
        return refuse(conn, &notes, 404, "unknown account");
    }
    let Some(season) = season(season_id) else {
        return refuse(conn, &notes, 404, &format!("unknown escalation season {season_id}"));
    };
    if !season.enabled {
        return refuse(
            conn,
            &notes,
            409,
            &format!("escalation season {season_id} is disabled in this client"),
        );
    }
    let (snapshot, soft) = match canonicalise(season, body) {
        Ok(canonical) => canonical,
        Err(why) => return refuse(conn, &notes, 400, &why),
    };
    let hash = snapshot.hash();
    let existing = season_row(conn, account, &season.id)?;
    if let Some(existing) = &existing {
        if snapshot.version == existing.update_version && hash == existing.content_hash {
            let stored = envelope(read_state(conn, account, season)?);
            let mut all = notes.clone();
            all.push(format!(
                "replay of version {}, nothing changed",
                existing.update_version
            ));
            record_event(
                conn,
                Event {
                    account,
                    caller: caller.as_str(),
                    route: ROUTE,
                    body,
                    status: 200,
                    reply: Some(&stored),
                    notes: &all,
                },
            )?;
            tracing::info!("Escalation {} v{} for {account} replayed", season.id, snapshot.version);
            return Ok((200, Some(stored)));
        }
        if snapshot.version == existing.update_version {
            return refuse(
                conn,
                &notes,
                409,
                &format!(
                    "update_version {} was already saved with other content",
                    snapshot.version
                ),
            );
        }
        if snapshot.version < existing.update_version {
            return refuse(
                conn,
                &notes,
                409,
                &format!(
                    "stale snapshot: version {} is older than the stored {}",
                    snapshot.version, existing.update_version
                ),
            );
        }
        // A talent reset may lower ranks; nothing native lowers the level or the XP within a level
        if snapshot.level < existing.level || (snapshot.level == existing.level && snapshot.xp < existing.xp) {
            return refuse(
                conn,
                &notes,
                409,
                &format!(
                    "snapshot would lower progress from level {} with {} XP to level {} with {} XP",
                    existing.level, existing.xp, snapshot.level, snapshot.xp
                ),
            );
        }
    }
    // What a world server holds when it loaded the old stub: never a first real save, and never a
    // jump from a stored level below the last one
    let max = season.max_level();
    if snapshot.level == max && snapshot.xp >= STUB_XP && existing.as_ref().is_none_or(|e| e.level < max) {
        let first = match &existing {
            None => "first save".to_owned(),
            Some(e) => format!("save over level {}", e.level),
        };
        return refuse(
            conn,
            &notes,
            409,
            &format!(
                "{first} carries the old stub values (level {} with {} XP); the world server must load the season again",
                snapshot.level, snapshot.xp
            ),
        );
    }
    let already = collected_ids(conn, account, &season.id)?;
    let lost: Vec<&str> = already
        .iter()
        .filter(|id| !snapshot.unlocks.contains(id))
        .map(String::as_str)
        .collect();
    if !lost.is_empty() {
        return refuse(
            conn,
            &notes,
            409,
            &format!("snapshot would un-collect {}", lost.join(", ")),
        );
    }
    if !soft.is_empty() {
        let issues = soft.join("; ");
        if settings.strict {
            return refuse(conn, &notes, 409, &format!("{issues} (ESCALATION_STRICT=1)"));
        }
        notes.push(format!("accepted although {issues} (ESCALATION_STRICT=0)"));
        tracing::warn!(
            "Escalation save of {} for {account} breaks a soft rule, stored anyway (ESCALATION_STRICT=0): {issues}",
            season.id
        );
    }
    let now = iso_now();
    diesel::insert_into(escalationprogression::table)
        .values((
            escalationprogression::accountId.eq(account),
            escalationprogression::seasonId.eq(&season.id),
            escalationprogression::level.eq(snapshot.level),
            escalationprogression::xp.eq(snapshot.xp),
            escalationprogression::updateVersion.eq(snapshot.version),
            escalationprogression::contentHash.eq(&hash),
            escalationprogression::updatedDate.eq(&now),
        ))
        .on_conflict((escalationprogression::accountId, escalationprogression::seasonId))
        .do_update()
        .set((
            escalationprogression::level.eq(snapshot.level),
            escalationprogression::xp.eq(snapshot.xp),
            escalationprogression::updateVersion.eq(snapshot.version),
            escalationprogression::contentHash.eq(&hash),
            escalationprogression::updatedDate.eq(&now),
        ))
        .execute(conn)?;
    // Ranks are replaced wholesale: a talent reset legitimately drops them
    diesel::delete(
        escalationtalents::table
            .filter(escalationtalents::accountId.eq(account))
            .filter(escalationtalents::seasonId.eq(&season.id)),
    )
    .execute(conn)?;
    for (talent, rank) in &snapshot.talents {
        diesel::insert_into(escalationtalents::table)
            .values((
                escalationtalents::accountId.eq(account),
                escalationtalents::seasonId.eq(&season.id),
                escalationtalents::talentId.eq(talent),
                escalationtalents::rank.eq(rank),
            ))
            .execute(conn)?;
    }
    for unlock in &snapshot.unlocks {
        if !already.contains(unlock) {
            diesel::insert_into(escalationunlocks::table)
                .values((
                    escalationunlocks::accountId.eq(account),
                    escalationunlocks::seasonId.eq(&season.id),
                    escalationunlocks::unlockId.eq(unlock),
                    escalationunlocks::collectedDate.eq(&now),
                ))
                .execute(conn)?;
        }
    }
    let reply = envelope(read_state(conn, account, season)?);
    record_event(
        conn,
        Event {
            account,
            caller: caller.as_str(),
            route: ROUTE,
            body,
            status: 200,
            reply: Some(&reply),
            notes: &notes,
        },
    )?;
    tracing::info!(
        "Escalation {} for {account}: v{} level {} xp {}, {} talent(s), {} collected",
        season.id,
        snapshot.version,
        snapshot.level,
        snapshot.xp,
        snapshot.talents.len(),
        snapshot.unlocks.len()
    );
    Ok((200, Some(reply)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Validate` of `escalationConfig.ts`: the registry Node refuses to boot with is refused here.
    #[test]
    fn the_registry_is_valid() {
        let all = seasons();
        assert_eq!(all.len(), 5);
        let mut ids = HashSet::new();
        for season in all {
            assert!(!season.id.is_empty() && ids.insert(&season.id));
            assert!(season.levels.iter().all(|l| l.required_experience > 0));
            let raw = crate::vendor::escalation_seasons()["seasons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == season.id.as_str())
                .unwrap();
            for (index, level) in raw["levels"].as_array().unwrap().iter().enumerate() {
                assert_eq!(level["level"], json!(index + 1));
            }
            let mut talents = HashSet::new();
            for talent in &season.talents {
                assert!(talents.insert(&talent.id) && !talent.rank_costs.is_empty());
                assert!(talent.points_to_unlock >= 0 && talent.rank_costs.iter().all(|c| *c >= 0));
            }
            let mut unlocks = HashSet::new();
            for unlock in &season.unlocks {
                assert!(unlocks.insert(&unlock.id));
                assert!((0..=season.max_level()).contains(&unlock.unlock_level));
            }
        }
    }
}
