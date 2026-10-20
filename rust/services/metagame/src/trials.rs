//! Trials (`controllers/trials.ts`). So far only what the store needs: settling finished weeks, which
//! Lady Luck's Champion stock depends on. The leaderboards and run submissions come with their area.

use std::collections::{BTreeSet, HashMap, HashSet};

use diesel::prelude::*;
use serde_json::{Value, json};

use crate::characters;
use crate::entitlements;
use crate::inventory::{self, InventoryCaller, TransactionRequest};
use crate::schema::{inventories, trialruns, trialweeks};
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
struct Run {
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
