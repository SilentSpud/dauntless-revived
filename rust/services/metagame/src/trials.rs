//! Trials (`controllers/trials.ts`): the weekly rotation, game-server run submissions with their Mark
//! rewards, the leaderboards, leaderboard profiles, the tuning schedule, and settling finished weeks
//! (which Lady Luck's Champion stock depends on).

use std::collections::{BTreeSet, HashMap, HashSet};

use diesel::prelude::*;
use serde_json::{Map, Value, json};

use crate::characters;
use crate::entitlements;
use crate::inventory::{self, InventoryCaller, TransactionRequest};
use crate::schema::{cooldowns, inventories, leaderboardprofiles, trialruns, trialweeks};
use crate::store::{Context, StoreFail};
use crate::time::iso;

pub type Conn = store::Connection;

/// `TRIAL_ROTATION_START`'s default: the first weekly rotation.
pub const ROTATION_START: &str = "2019-07-18T18:00:00.000Z";
pub const CHAMPION_ENTITLEMENT: &str = "trials_leaderboard_placement";
pub const CHAMPION_TITLE: &str = "TITLE_TRIALS_00";
pub const DAUNTLESS_TITLE: &str = "TITLE_TRIALS_01";
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// `TrialWeekAt`; an unparsable `TRIAL_ROTATION_START` fails, as Node's `TrialsError(500)` did.
pub fn week_at(rotation_start: Option<i64>, at: i64) -> Result<i64, StoreFail> {
    let epoch = rotation_start.ok_or_else(|| StoreFail::Internal("TRIAL_ROTATION_START is invalid".into()))?;
    Ok((at - epoch).div_euclid(WEEK_MS))
}

#[derive(Debug, Clone, Queryable)]
#[allow(non_snake_case, dead_code)]
pub struct Run {
    id: i64,
    trialId: String,
    trialWeek: i64,
    difficulty: i64,
    mode: String,
    runKey: String,
    groupKey: String,
    completionTime: i64,
    objectivesCompleted: i64,
    sessionId: String,
    entries: String,
    submittedDate: String,
}

/// `BestRuns`: each group's fastest run (the earlier one on a tie), fastest first.
fn best_runs(rows: Vec<&Run>) -> Vec<&Run> {
    let mut best: Vec<&Run> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for run in rows {
        match index.get(run.groupKey.as_str()) {
            Some(&at) => {
                let current = best[at];
                if run.completionTime < current.completionTime
                    || (run.completionTime == current.completionTime && run.submittedDate < current.submittedDate)
                {
                    best[at] = run;
                }
            }
            None => {
                index.insert(&run.groupKey, best.len());
                best.push(run);
            }
        }
    }
    best.sort_by(|a, b| {
        a.completionTime
            .cmp(&b.completionTime)
            .then(b.objectivesCompleted.cmp(&a.objectivesCompleted))
            .then_with(|| service_kit::collate::locale_compare(&a.submittedDate, &b.submittedDate))
            .then_with(|| service_kit::collate::locale_compare(&a.groupKey, &b.groupKey))
    });
    best
}

fn accounts_of(run: &Run) -> Result<Vec<String>, StoreFail> {
    let entries: Value =
        serde_json::from_str(&run.entries).map_err(|e| StoreFail::Internal(format!("SyntaxError: {e}")))?;
    let entries = entries
        .as_array()
        .ok_or_else(|| StoreFail::Internal("TypeError: trial run entries are not a list".into()))?;
    Ok(entries
        .iter()
        .map(|entry| crate::js::string(entry.get("phx_account_id")))
        .collect())
}

/// `GrantTrialTitleInTx`: the title stack on the account's active character, unless it holds one.
fn grant_title(conn: &mut Conn, ctx: &Context, account: &str, title: &str, week: i64) -> Result<bool, StoreFail> {
    // A leaderboard entry normally implies a real character. Keep finalization resilient to
    // imported/admin rows that name an account without one; the store entitlement can still be kept.
    let Some(character) = characters::active(conn, account)? else {
        return Ok(false);
    };
    let stacked: Option<String> = inventories::table
        .find(&character.characterId)
        .select(inventories::stackedItems)
        .first(conn)
        .optional()?;
    let stacks: Value = serde_json::from_str(stacked.as_deref().unwrap_or("[]"))
        .map_err(|e| StoreFail::Internal(format!("SyntaxError: {e}")))?;
    let holds = stacks.as_array().is_some_and(|stacks| {
        stacks.iter().any(|item| {
            item.get("catalogId").and_then(Value::as_str) == Some(title)
                && crate::js::number(item.get("quantity")) > 0.0
        })
    });
    if holds {
        return Ok(false);
    }
    let request = TransactionRequest {
        user_id: Some(account.to_owned()),
        character_id: Some(json!(character.characterId)),
        transaction_id: Some(json!(format!("trials-title:{week}:{title}:{account}"))),
        add_instanced: None,
        add_stacked: Some(json!([{"catalogId": title, "quantity": 1}])),
        remove_instanced: None,
        remove_stacked: None,
        save_instanced: None,
    };
    let source = json!(format!("trials:week:{week}"));
    inventory::apply_in_tx(conn, request, InventoryCaller::Trials, Some(&source), &ctx.inventory)
        .map_err(|fail| StoreFail::Internal(fail.message().to_owned()))?;
    Ok(true)
}

/// What [`finalize_completed_weeks`] settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalized {
    pub weeks: usize,
    pub awarded_accounts: usize,
}

/// `FinalizeCompletedTrialWeeks`, in the caller's transaction. At the weekly reset, a Dauntless
/// global leaderboard finish permanently awards: top 100, the Trials Champion title and access to
/// Lady Luck's Champion store; top 5, The Dauntless title as well. Solo and group boards are ranked
/// independently; every member of a qualifying group gets the award. Lazy and idempotent: trialweeks
/// records each completed week after all its grants.
pub fn finalize_completed_weeks(conn: &mut Conn, ctx: &Context) -> Result<Finalized, StoreFail> {
    let current = week_at(ctx.settings.trial_rotation_start, ctx.now)?;
    let finalized: HashSet<i64> = trialweeks::table
        .select(trialweeks::week)
        .load::<i64>(conn)?
        .into_iter()
        .collect();
    let old: Vec<Run> = trialruns::table.filter(trialruns::trialWeek.lt(current)).load(conn)?;
    let weeks: BTreeSet<i64> = old
        .iter()
        .map(|run| run.trialWeek)
        .filter(|week| !finalized.contains(week))
        .collect();
    let mut awarded = 0;
    for &week in &weeks {
        // Insertion-ordered sets, as JavaScript's Set iterates
        let mut champions: Vec<String> = Vec::new();
        let mut top_five: Vec<String> = Vec::new();
        let add = |set: &mut Vec<String>, account: String| {
            if !set.contains(&account) {
                set.push(account);
            }
        };
        for mode in ["solo", "group"] {
            let ranked = best_runs(
                old.iter()
                    .filter(|run| run.trialWeek == week && run.difficulty == 1 && run.mode == mode)
                    .collect(),
            );
            for run in ranked.iter().take(100) {
                for account in accounts_of(run)? {
                    add(&mut champions, account);
                }
            }
            for run in ranked.iter().take(5) {
                for account in accounts_of(run)? {
                    add(&mut top_five, account);
                }
            }
        }
        let source = format!("trials:week:{week}");
        for account in &champions {
            entitlements::grant(
                conn,
                &ctx.entitlement_defaults,
                account,
                CHAMPION_ENTITLEMENT,
                0,
                &source,
            )?;
            grant_title(conn, ctx, account, CHAMPION_TITLE, week)?;
        }
        for account in &top_five {
            grant_title(conn, ctx, account, DAUNTLESS_TITLE, week)?;
        }
        diesel::insert_into(trialweeks::table)
            .values((
                trialweeks::week.eq(week),
                trialweeks::finalizedDate.eq(iso(ctx.now)),
                trialweeks::awardedAccounts.eq(champions.len() as i64),
            ))
            .execute(conn)?;
        awarded += champions.len();
    }
    Ok(Finalized {
        weeks: weeks.len(),
        awarded_accounts: awarded,
    })
}

// ---- The weekly rotation ----

/// How far back a leaderboard read may name a Trial row (the client's Current/Previous tabs need 1).
const LOOKBACK_WEEKS: i64 = 104;
const MARK_REWARD: i64 = 100;
const BRONZE_TIME_MS: i64 = 30 * 60 * 1000;
const SILVER_TIME_MS: i64 = 5 * 60 * 1000;
const GOLD_TIME_MS: i64 = 3 * 60 * 1000;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn refused(status: u16, message: impl Into<String>) -> StoreFail {
    StoreFail::Refused(status, message.into())
}

/// `TrialEpoch` for the Trials routes, which answer an unparsable `TRIAL_ROTATION_START` with its
/// own message.
pub fn epoch(rotation_start: Option<i64>) -> Result<i64, StoreFail> {
    rotation_start.ok_or_else(|| refused(500, "TRIAL_ROTATION_START is invalid"))
}

/// `TrialRewardRank`: Bronze (0) under 30 minutes, Silver (1) under 5, Gold (2) under 3; -1 for none.
pub fn reward_rank(completion_time: i64) -> i64 {
    if completion_time < GOLD_TIME_MS {
        2
    } else if completion_time < SILVER_TIME_MS {
        1
    } else if completion_time < BRONZE_TIME_MS {
        0
    } else {
        -1
    }
}

/// `vendor/trials_pool.json`: each behemoth's rows, behemoths sorted. Kept identical to the deploy
/// server's, which launches the hunt.
fn pool() -> &'static [(String, Vec<String>)] {
    static POOL: std::sync::OnceLock<Vec<(String, Vec<String>)>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let mut pool: Vec<(String, Vec<String>)> = crate::vendor::trials_pool()["behemoths"]
            .as_object()
            .map(|behemoths| {
                behemoths
                    .iter()
                    .map(|(name, rows)| {
                        let rows = rows
                            .as_array()
                            .map(|rows| rows.iter().map(|row| crate::js::string(Some(row))).collect())
                            .unwrap_or_default();
                        (name.clone(), rows)
                    })
                    .collect()
            })
            .unwrap_or_default();
        pool.sort_by(|a, b| a.0.cmp(&b.0));
        pool
    })
}

/// Sorted by the SHA-256 of prefix + item, compared byte for byte like the DLL (TrialsRotation.h).
fn sorted_by_score<'a>(items: impl Iterator<Item = &'a String>, prefix: &str) -> Vec<&'a String> {
    let mut scored: Vec<(String, &String)> = items
        .map(|item| (crate::crypto::sha256_hex(&format!("{prefix}{item}")), item))
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0));
    scored.into_iter().map(|(_, item)| item).collect()
}

fn cycle_order(cycle: i64) -> Vec<&'static String> {
    sorted_by_score(
        pool().iter().map(|(name, _)| name),
        &format!("trials-weekly-v1:{cycle}:"),
    )
}

/// `TrialBehemothForWeek`: a seeded shuffle of the pool per cycle of N weeks, every behemoth once per
/// cycle, never the same one two weeks running.
pub fn behemoth_for_week(week: i64) -> Result<&'static str, StoreFail> {
    let count = pool().len() as i64;
    if count == 0 {
        return Err(refused(500, "Trials pool is empty"));
    }
    let cycle = week.div_euclid(count);
    let mut order = cycle_order(cycle);
    if count > 1 && order[0] == cycle_order(cycle - 1)[count as usize - 1] {
        order.swap(0, 1);
    }
    Ok(order[week.rem_euclid(count) as usize])
}

/// `TrialIdForWeek`: difficulty 0 is Hard (Normal), 1 Elite (Dauntless).
pub fn trial_id_for_week(difficulty: i64, week: i64) -> Result<String, StoreFail> {
    let prefix = match difficulty {
        0 => "Hard",
        1 => "Elite",
        _ => return Err(refused(400, "difficulty is invalid")),
    };
    let behemoth = behemoth_for_week(week)?;
    let rows = pool()
        .iter()
        .find(|(name, _)| name == behemoth)
        .map_or(&[][..], |(_, rows)| rows.as_slice());
    let suffix = sorted_by_score(rows.iter(), &format!("trials-weekly-v1:row:{week}:"))
        .first()
        .map_or("undefined".to_owned(), |row| (*row).clone());
    Ok(format!("Arena_MatchmakerHunt_{prefix}_{suffix}"))
}

/// `/^Arena_MatchmakerHunt_(Hard|Elite)_\d{3}$/`, with the difficulty word.
fn trial_id_kind(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("Arena_MatchmakerHunt_")?;
    let (kind, digits) = rest.split_once('_')?;
    (matches!(kind, "Hard" | "Elite") && digits.len() == 3 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some(kind)
}

/// `TrialWeekForId`: the most recent week that ran the row, so a repeated row never leaks a score
/// from an earlier week into the current board.
fn week_for_id(id: &str, difficulty: i64, current: i64) -> Result<i64, StoreFail> {
    let kind = trial_id_kind(id).ok_or_else(|| refused(400, "trial_id is invalid"))?;
    if kind != if difficulty == 1 { "Elite" } else { "Hard" } {
        return Err(refused(400, "trial_id does not match difficulty"));
    }
    let mut week = current;
    while week > current - LOOKBACK_WEEKS {
        if trial_id_for_week(difficulty, week)? == id {
            return Ok(week);
        }
        week -= 1;
    }
    Err(refused(400, "trial_id is outside the recent rotation"))
}

/// `TrialsWindowForWeek`: the week's start and end (seconds) and the seconds until it ends.
pub fn window_for_week(epoch: i64, week: i64, now: i64) -> (i64, i64, i64) {
    let start = epoch + week * WEEK_MS;
    let end = start + WEEK_MS;
    let refresh = -(-(end - now)).div_euclid(1000);
    (start.div_euclid(1000), end.div_euclid(1000), refresh.max(0))
}

fn insert_window(map: &mut Map<String, Value>, (start, end, refresh): (i64, i64, i64)) {
    map.insert("trial_start".into(), json!(start));
    map.insert("trial_end".into(), json!(end));
    map.insert("time_to_refresh".into(), json!(refresh));
}

// ---- Bodies ----

/// JavaScript's `.length` of a string.
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `.slice(0, n)` of a string, in UTF-16 units.
fn js_prefix(text: &str, n: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= n
        })
        .collect()
}

/// `Integer`: a safe integer in [min, max], or a string of digits holding one.
fn integer(value: Option<&Value>, name: &str, min: i64, max: i64) -> Result<i64, StoreFail> {
    let number = match value {
        Some(Value::String(text)) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse::<f64>().ok()
        }
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    };
    match number {
        Some(n) if n.fract() == 0.0 && n.abs() <= MAX_SAFE_INTEGER && n >= min as f64 && n <= max as f64 => {
            Ok(n as i64)
        }
        _ => Err(refused(400, format!("{name} is invalid"))),
    }
}

/// `value ?? fallback`.
fn or_default(value: Option<&Value>, fallback: Value) -> Value {
    match value {
        None | Some(Value::Null) => fallback,
        Some(value) => value.clone(),
    }
}

fn body_field<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body.as_object().and_then(|map| map.get(name))
}

fn trial_id(body: &Value) -> Result<String, StoreFail> {
    match body_field(body, "trial_id") {
        Some(Value::String(id)) if trial_id_kind(id).is_some() => Ok(id.clone()),
        _ => Err(refused(400, "trial_id is invalid")),
    }
}

fn difficulty(body: &Value) -> Result<i64, StoreFail> {
    integer(body_field(body, "difficulty"), "difficulty", 0, 1)
}

fn profile_text(value: Option<&Value>, name: &str, required: bool) -> Result<String, StoreFail> {
    match value {
        Some(Value::String(text)) if js_length(text) <= 128 && !(required && text.is_empty()) => Ok(text.clone()),
        _ => Err(refused(400, format!("{name} is invalid"))),
    }
}

/// `UpdateLeaderboardProfile`: the public leaderboard identity the 1.4.4 client reports at login.
/// The private-server account name is never rewritten from it.
pub fn update_profile(conn: &mut Conn, account: Option<&str>, body: Option<&Value>, now: i64) -> Result<(), StoreFail> {
    let Some(Value::Object(raw)) = body else {
        return Err(refused(400, "profile is invalid"));
    };
    let dauntless_id = profile_text(raw.get("dauntlessid"), "dauntlessid", true)?;
    if Some(dauntless_id.as_str()) != account {
        return Err(refused(403, "profile account does not match the authenticated account"));
    }
    let optional = |name: &str| profile_text(Some(&or_default(raw.get(name), json!(""))), name, false);
    let epic = optional("epicid")?;
    let platform_id = optional("platformid")?;
    let platform = optional("currentplatform")?;
    let display = profile_text(raw.get("currentdisplayname"), "currentdisplayname", true)?;
    let updated = iso(now);
    diesel::insert_into(leaderboardprofiles::table)
        .values((
            leaderboardprofiles::accountId.eq(&dauntless_id),
            leaderboardprofiles::epicId.eq(&epic),
            leaderboardprofiles::platformId.eq(&platform_id),
            leaderboardprofiles::platform.eq(&platform),
            leaderboardprofiles::displayName.eq(&display),
            leaderboardprofiles::updatedDate.eq(&updated),
        ))
        .on_conflict(leaderboardprofiles::accountId)
        .do_update()
        .set((
            leaderboardprofiles::epicId.eq(&epic),
            leaderboardprofiles::platformId.eq(&platform_id),
            leaderboardprofiles::platform.eq(&platform),
            leaderboardprofiles::displayName.eq(&display),
            leaderboardprofiles::updatedDate.eq(&updated),
        ))
        .execute(conn)?;
    Ok(())
}

/// `Player`: one entry of a submission.
fn player(raw: &Value) -> Result<Value, StoreFail> {
    let id = raw
        .as_object()
        .and_then(|map| map.get("phx_account_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| refused(400, "trial player is missing phx_account_id"))?;
    let text = |name: &str| match raw.get(name) {
        Some(Value::String(text)) => js_prefix(text, 128),
        _ => String::new(),
    };
    let mut entry = Map::new();
    entry.insert("phx_account_id".into(), json!(id));
    entry.insert("platform".into(), json!(text("platform")));
    entry.insert("platform_name".into(), json!(text("platform_name")));
    entry.insert("player_role_id".into(), json!(text("player_role_id")));
    let weapon = or_default(raw.get("weapon"), json!(0));
    entry.insert("weapon".into(), json!(integer(Some(&weapon), "weapon", 0, 255)?));
    if let Some(secondary) = raw.get("secondary_weapon") {
        entry.insert(
            "secondary_weapon".into(),
            json!(integer(Some(secondary), "secondary_weapon", 0, 255)?),
        );
    }
    Ok(Value::Object(entry))
}

fn submission_players(body: &Value) -> Result<Vec<Value>, StoreFail> {
    if let Some(Value::Array(entries)) = body_field(body, "entries") {
        if entries.is_empty() || entries.len() > 4 {
            return Err(refused(400, "entries must contain one to four players"));
        }
        return entries.iter().map(player).collect();
    }
    Ok(vec![player(body)?])
}

// ---- Submissions ----

/// What [`save_run`] stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub trial_id: String,
    pub mode: &'static str,
    pub accounts: Vec<String>,
}

/// `GrantTrialMarkTierInTx`: 100 Marks once per week, currency and tier, receipted as a cooldown.
fn grant_mark_tier(
    conn: &mut Conn,
    ctx: &Context,
    account: &str,
    week: i64,
    currency: &str,
    rank: i64,
) -> Result<bool, StoreFail> {
    // Backend receipts, not the opaque retail cooldown ids the game server may also send through
    // /cooldown: server-side grants stay idempotent without pretending to know those ids.
    let receipt = format!("revived_trials_marks:{week}:{currency}:{rank}");
    let existing: Option<String> = cooldowns::table
        .find((account, &receipt))
        .select(cooldowns::cooldownId)
        .first(conn)
        .optional()?;
    if existing.is_some() {
        return Ok(false);
    }
    let Some(character) = characters::active(conn, account)? else {
        return Err(refused(
            409,
            format!("no active character for Trial reward account {account}"),
        ));
    };
    let catalog = if currency == "steel" {
        "CURRENCY_MARKS_STEEL"
    } else {
        "CURRENCY_MARKS_GILDED"
    };
    let request = TransactionRequest {
        user_id: Some(account.to_owned()),
        character_id: Some(json!(character.characterId)),
        transaction_id: Some(json!(format!("trials-marks:{week}:{currency}:{rank}:{account}"))),
        add_instanced: None,
        add_stacked: Some(json!([{"catalogId": catalog, "quantity": MARK_REWARD}])),
        remove_instanced: None,
        remove_stacked: None,
        save_instanced: None,
    };
    let source = json!(format!("trials:week:{week}"));
    inventory::apply_in_tx(conn, request, InventoryCaller::Trials, Some(&source), &ctx.inventory)
        .map_err(|fail| StoreFail::Internal(fail.message().to_owned()))?;
    let stamp = iso(ctx.now);
    diesel::insert_into(cooldowns::table)
        .values((
            cooldowns::accountId.eq(account),
            cooldowns::cooldownId.eq(&receipt),
            cooldowns::startedDate.eq(&stamp),
            cooldowns::updatedDate.eq(&stamp),
        ))
        .execute(conn)?;
    Ok(true)
}

/// `AwardTrialMarksInTx`: every tier up to the run's, Steel always and Gilded on Dauntless. A
/// Dauntless run also pays any unclaimed Normal tier; the shared Steel receipt stops a later Normal
/// run or a retry from paying it twice.
fn award_marks(
    conn: &mut Conn,
    ctx: &Context,
    accounts: &[String],
    difficulty: i64,
    week: i64,
    time: i64,
) -> Result<(), StoreFail> {
    let rank = reward_rank(time);
    for account in accounts {
        for tier in 0..=rank {
            grant_mark_tier(conn, ctx, account, week, "steel", tier)?;
            if difficulty == 1 {
                grant_mark_tier(conn, ctx, account, week, "gilded", tier)?;
            }
        }
    }
    Ok(())
}

/// `SaveTrialRun`: a game server's result for this week's Trial. A retry with the same session id
/// is idempotent; the same session id with a different result is refused. Rewards are bound to the
/// stored run.
pub fn save_run(conn: &mut Conn, ctx: &Context, body: &Value) -> Result<Saved, StoreFail> {
    let trial = trial_id(body)?;
    let difficulty = difficulty(body)?;
    let week = week_at(Some(epoch(ctx.settings.trial_rotation_start)?), ctx.now)?;
    if trial != trial_id_for_week(difficulty, week)? {
        return Err(refused(409, "trial_id is not the active Trial"));
    }
    let completion = integer(
        body_field(body, "completion_time"),
        "completion_time",
        0,
        24 * 60 * 60 * 1000,
    )?;
    let objectives_value = or_default(body_field(body, "objectives_completed"), json!(0));
    let objectives = integer(Some(&objectives_value), "objectives_completed", 0, 99)?;
    let players = submission_players(body)?;
    let mode = if players.len() == 1 { "solo" } else { "group" };
    let accounts: Vec<String> = players
        .iter()
        .map(|p| crate::js::string(p.get("phx_account_id")))
        .collect();
    if accounts.iter().collect::<HashSet<_>>().len() != accounts.len() {
        return Err(refused(400, "the same player appears more than once"));
    }
    let entries = service_kit::jsjson::stringify(&Value::Array(players.clone()));
    let session = match body_field(body, "session_id") {
        Some(Value::String(id)) => Some(js_prefix(id, 256)),
        _ => None,
    };
    let run_key = match &session {
        Some(id) if !id.is_empty() => id.clone(),
        _ => {
            let raw_difficulty = body_field(body, "difficulty").cloned().unwrap_or(Value::Null);
            let seed = json!([trial, raw_difficulty, completion, objectives, players]);
            crate::crypto::sha256_hex(&service_kit::jsjson::stringify(&seed))
        }
    };
    let mut sorted = accounts.clone();
    sorted.sort();
    let group_key = sorted.join(":");
    let session = session.unwrap_or_else(|| run_key.clone());
    diesel::insert_into(trialruns::table)
        .values((
            trialruns::trialId.eq(&trial),
            trialruns::trialWeek.eq(week),
            trialruns::difficulty.eq(difficulty),
            trialruns::mode.eq(mode),
            trialruns::runKey.eq(&run_key),
            trialruns::groupKey.eq(&group_key),
            trialruns::completionTime.eq(completion),
            trialruns::objectivesCompleted.eq(objectives),
            trialruns::sessionId.eq(&session),
            trialruns::entries.eq(&entries),
            trialruns::submittedDate.eq(iso(crate::time::now_ms())),
        ))
        .on_conflict_do_nothing()
        .execute(conn)?;
    let stored: Option<Run> = trialruns::table
        .filter(trialruns::trialWeek.eq(week))
        .filter(trialruns::trialId.eq(&trial))
        .filter(trialruns::difficulty.eq(difficulty))
        .filter(trialruns::runKey.eq(&run_key))
        .first(conn)
        .optional()?;
    let stored = stored.ok_or_else(|| refused(500, "Trial result was not persisted"))?;
    if stored.mode != mode
        || stored.groupKey != group_key
        || stored.completionTime != completion
        || stored.objectivesCompleted != objectives
        || stored.sessionId != session
        || stored.entries != entries
    {
        return Err(refused(409, "Trial session id was already used for a different result"));
    }
    let stored_accounts = accounts_of(&stored)?;
    award_marks(
        conn,
        ctx,
        &stored_accounts,
        stored.difficulty,
        stored.trialWeek,
        stored.completionTime,
    )?;
    Ok(Saved {
        trial_id: trial,
        mode,
        accounts: stored_accounts,
    })
}

// ---- Leaderboards ----

/// A leaderboard read: which board, which page, which platforms.
#[derive(Debug, Clone)]
pub struct Query {
    trial_id: String,
    week: i64,
    difficulty: i64,
    page: i64,
    page_size: i64,
    platforms: HashSet<String>,
    epoch: i64,
}

/// `QueryFrom`; `individual` reads always take the first 100 runs (`page: 0, page_size: 100`).
pub fn query_from(body: &Value, epoch: i64, now: i64, individual: bool) -> Result<Query, StoreFail> {
    let mut platforms = HashSet::new();
    if let Some(value) = body_field(body, "target_platforms") {
        let list = value
            .as_array()
            .filter(|list| list.iter().all(Value::is_string))
            .ok_or_else(|| refused(400, "target_platforms is invalid"))?;
        platforms.extend(list.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    let id = trial_id(body)?;
    let difficulty = difficulty(body)?;
    let current = week_at(Some(epoch), now)?;
    let week = week_for_id(&id, difficulty, current)?;
    let (page, page_size) = if individual {
        (0, 100)
    } else {
        let page = or_default(body_field(body, "page"), json!(0));
        let size = or_default(body_field(body, "page_size"), json!(100));
        (
            integer(Some(&page), "page", 0, 1_000_000)?,
            integer(Some(&size), "page_size", 1, 100)?,
        )
    };
    Ok(Query {
        trial_id: id,
        week,
        difficulty,
        page,
        page_size,
        platforms,
        epoch,
    })
}

/// `ParseEntries`: the stored players, with the platform and display name their leaderboard profile
/// reports.
fn parse_entries(conn: &mut Conn, run: &Run) -> Result<Vec<Value>, StoreFail> {
    let entries =
        service_kit::jsjson::parse(&run.entries).map_err(|e| StoreFail::Internal(format!("SyntaxError: {e}")))?;
    let Value::Array(entries) = entries else {
        return Err(StoreFail::Internal(
            "TypeError: trial run entries are not a list".into(),
        ));
    };
    let mut out = Vec::with_capacity(entries.len());
    for mut entry in entries {
        let account = entry.get("phx_account_id").and_then(Value::as_str).map(str::to_owned);
        let profile: Option<(String, String)> = match account {
            Some(account) => leaderboardprofiles::table
                .find(account)
                .select((leaderboardprofiles::platform, leaderboardprofiles::displayName))
                .first(conn)
                .optional()?,
            None => None,
        };
        if let (Some((platform, name)), Some(map)) = (profile, entry.as_object_mut()) {
            // `Profile.platform || Entry.platform`: an empty profile value keeps the entry's
            for (key, value) in [("platform", platform), ("platform_name", name)] {
                if !value.is_empty() {
                    map.insert(key.into(), json!(value));
                }
            }
        }
        out.push(entry);
    }
    Ok(out)
}

/// `Runs`: the board's best run per group, fastest first.
fn runs(conn: &mut Conn, query: &Query, mode: &str) -> Result<Vec<Run>, StoreFail> {
    let rows: Vec<Run> = trialruns::table
        .filter(trialruns::trialId.eq(&query.trial_id))
        .filter(trialruns::trialWeek.eq(query.week))
        .filter(trialruns::difficulty.eq(query.difficulty))
        .filter(trialruns::mode.eq(mode))
        .order(trialruns::id.asc())
        .load(conn)?;
    let mut kept = Vec::with_capacity(rows.len());
    for run in &rows {
        if query.platforms.is_empty()
            || parse_entries(conn, run)?.iter().all(|entry| {
                entry
                    .get("platform")
                    .and_then(Value::as_str)
                    .is_some_and(|p| query.platforms.contains(p))
            })
        {
            kept.push(run);
        }
    }
    Ok(best_runs(kept).into_iter().cloned().collect())
}

fn solo_entry(conn: &mut Conn, run: &Run, rank: usize) -> Result<Map<String, Value>, StoreFail> {
    let mut entry = Map::new();
    entry.insert("completion_time".into(), json!(run.completionTime));
    entry.insert("objectives_completed".into(), json!(run.objectivesCompleted));
    if let Some(Value::Object(player)) = parse_entries(conn, run)?.into_iter().next() {
        for (key, value) in player {
            entry.insert(key, value);
        }
    }
    entry.insert("rank".into(), json!(rank));
    entry.insert("session_id".into(), json!(run.sessionId));
    Ok(entry)
}

fn group_entry(conn: &mut Conn, run: &Run, rank: usize) -> Result<Map<String, Value>, StoreFail> {
    let mut entry = Map::new();
    entry.insert("completion_time".into(), json!(run.completionTime));
    entry.insert("entries".into(), Value::Array(parse_entries(conn, run)?));
    entry.insert("objectives_completed".into(), json!(run.objectivesCompleted));
    entry.insert("rank".into(), json!(rank));
    entry.insert("session_id".into(), json!(run.sessionId));
    Ok(entry)
}

/// The requested page of a board's entries.
fn page(conn: &mut Conn, query: &Query, mode: &str) -> Result<Vec<Value>, StoreFail> {
    let runs = runs(conn, query, mode)?;
    let from = (query.page * query.page_size) as usize;
    let mut entries = Vec::new();
    for (index, run) in runs.iter().enumerate().skip(from).take(query.page_size as usize) {
        let entry = if mode == "solo" {
            solo_entry(conn, run, index + 1)?
        } else {
            group_entry(conn, run, index + 1)?
        };
        entries.push(Value::Object(entry));
    }
    Ok(entries)
}

fn ok_reply(payload: Value) -> Value {
    json!({"code": null, "message": "OK", "payload": payload})
}

/// `SoloLeaderboard` and `GroupLeaderboard`.
pub fn board(conn: &mut Conn, query: &Query, mode: &str, now: i64) -> Result<Value, StoreFail> {
    let entries = page(conn, query, mode)?;
    let mut payload = Map::new();
    payload.insert("difficulty".into(), json!(query.difficulty));
    payload.insert("entries".into(), Value::Array(entries));
    payload.insert("page".into(), json!(query.page));
    payload.insert("page_size".into(), json!(query.page_size));
    payload.insert("trial_id".into(), json!(query.trial_id));
    insert_window(&mut payload, window_for_week(query.epoch, query.week, now));
    Ok(ok_reply(Value::Object(payload)))
}

/// `AllLeaderboards`: the aggregate the 1.4.4 client reads.
pub fn all_boards(conn: &mut Conn, query: &Query) -> Result<Value, StoreFail> {
    let solo = page(conn, query, "solo")?;
    let group = page(conn, query, "group")?;
    Ok(ok_reply(json!({
        "difficulty": query.difficulty,
        "guild": {},
        "page": query.page,
        "page_size": query.page_size,
        "trial_id": query.trial_id,
        "world": {
            "group": {
                "difficulty": query.difficulty,
                "entries": group,
                "page": query.page,
                "page_size": query.page_size,
                "trial_id": query.trial_id
            },
            "solo": {
                "all": {
                    "difficulty": query.difficulty,
                    "entries": solo
                }
            }
        }
    })))
}

/// `SoloIndividual` and `GroupIndividual`: the account's own entry, or an empty payload.
pub fn individual(conn: &mut Conn, query: &Query, body: &Value, mode: &str) -> Result<Value, StoreFail> {
    let account = body_field(body, "phx_account_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| refused(400, "phx_account_id is invalid"))?
        .to_owned();
    let runs = runs(conn, query, mode)?;
    let mut found = None;
    for (index, run) in runs.iter().enumerate() {
        let hit = if mode == "solo" {
            run.groupKey == account
        } else {
            parse_entries(conn, run)?
                .iter()
                .any(|entry| entry.get("phx_account_id").and_then(Value::as_str) == Some(account.as_str()))
        };
        if hit {
            found = Some((index, run));
            break;
        }
    }
    let Some((index, run)) = found else {
        return Ok(ok_reply(json!({})));
    };
    let mut entry = if mode == "solo" {
        solo_entry(conn, run, index + 1)?
    } else {
        group_entry(conn, run, index + 1)?
    };
    entry.insert("difficulty".into(), json!(query.difficulty.to_string()));
    entry.insert("trial_id".into(), json!(query.trial_id));
    Ok(ok_reply(Value::Object(entry)))
}

// ---- The tuning schedule ----

/// `ServiceDate`: `YYYY.MM.DD-HH.MM.SS` in UTC.
fn service_date(seconds: i64) -> String {
    let text = iso(seconds * 1000);
    format!(
        "{}.{}.{}-{}.{}.{}",
        &text[0..4],
        &text[5..7],
        &text[8..10],
        &text[11..13],
        &text[14..16],
        &text[17..19]
    )
}

/// `GetSeasonalEventSchedule`: with `TRIALS_SCHEDULE` on, this week's Trials (and the keys the
/// native arena code might ask for) as one-time items for the Thursday-to-Thursday window.
pub fn seasonal_event_schedule(settings: &crate::store::Settings, now: i64) -> Result<Value, StoreFail> {
    if !settings.trials_schedule {
        return Ok(ok_reply(json!({"ScheduledItems": []})));
    }
    let epoch = epoch(settings.trial_rotation_start)?;
    let week = week_at(Some(epoch), now)?;
    let (start, end, _) = window_for_week(epoch, week, now);
    let (start, end) = (service_date(start), service_date(end));
    // Captured Phoenix tuning keeps event_ladyluck_repeatable active for Lady Luck/Trials; the four
    // hunt ids stay alongside it for the 1.4.4 A/B test, since the native
    // GetCurrentNormalHuntScheduleDetails isn't symbolized.
    let ids = [
        "event_ladyluck_repeatable".to_owned(),
        "CR19_PlayerHunt_Arena_Hard".to_owned(),
        "CR19_PlayerHunt_Arena_Elite".to_owned(),
        trial_id_for_week(0, week)?,
        trial_id_for_week(1, week)?,
    ];
    let items: Vec<Value> = ids
        .iter()
        .map(|id| {
            json!({
                "EndTime": end,
                "IsRepeatable": false,
                "Name": id,
                "RepeatableDetails": {"EachDay": false, "EachWeek": false},
                "ScheduledItems": [{"ID": id, "MaxCompletionPerInterval": -1}],
                "StartTime": start
            })
        })
        .collect();
    Ok(ok_reply(json!({"ScheduledItems": items})))
}
