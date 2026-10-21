//! The weekly Trials rotation (`gameservers.ts`): which behemoth and which cooked row the Trials run
//! this week. Deterministic, so the metagame and the game DLL (`TrialsRotation.h`) compute the same
//! pick without any shared state.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tables;

/// Trials reset every Thursday 18:00 UTC, counted from the historical row 001 week.
pub const ROTATION_START: &str = "2019-07-18T18:00:00.000Z";
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Behemoth (an `_alpha` variant) to the suffixes of its cooked rows.
pub type Pool = BTreeMap<String, Vec<String>>;

/// The weekly pool (`vendor/trials_pool.json`, mirrored in the metagame), checked against the
/// cooked tables on first use.
pub fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        tables::trials_pool()["behemoths"]
            .as_object()
            .map(|behemoths| {
                behemoths
                    .iter()
                    .map(|(name, rows)| {
                        let suffixes = rows
                            .as_array()
                            .map(|list| list.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
                            .unwrap_or_default();
                        (name.clone(), suffixes)
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn blueprint(row: Option<&Value>) -> String {
    let asset = row.and_then(tables::behemoth_asset).unwrap_or("");
    let last = asset.rsplit('.').next().unwrap_or("");
    last.strip_suffix("_bp_C").unwrap_or(last).to_owned()
}

/// Every pooled row must exist in both cooked tables with the pooled blueprint, so a stale pool file
/// can never send a different behemoth than the one it names.
pub fn validate(
    pool: &Pool,
    hard: &serde_json::Map<String, Value>,
    elite: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    if pool.is_empty() {
        return Err("Trials pool is empty".into());
    }
    for (behemoth, suffixes) in pool {
        if !behemoth.ends_with("_alpha") {
            return Err(format!("Trials pool behemoth {behemoth} is not a variant"));
        }
        if suffixes.is_empty() {
            return Err(format!("Trials pool behemoth {behemoth} has no rows"));
        }
        for suffix in suffixes {
            let hard = blueprint(hard.get(&format!("Arena_MatchmakerHunt_Hard_{suffix}")));
            let elite = blueprint(elite.get(&format!("Arena_MatchmakerHunt_Elite_{suffix}")));
            if hard != *behemoth || elite != *behemoth {
                return Err(format!(
                    "Trials pool row {suffix} is not {behemoth} at both difficulties"
                ));
            }
        }
    }
    Ok(())
}

fn score(seed: &str) -> String {
    Sha256::digest(seed.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Sorted by the hash of `prefix` + item, compared byte for byte like the DLL.
fn sorted_by_score<'a>(items: impl Iterator<Item = &'a String>, prefix: &str) -> Vec<&'a String> {
    let mut scored: Vec<(String, &String)> = items.map(|item| (score(&format!("{prefix}{item}")), item)).collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0));
    scored.into_iter().map(|(_, item)| item).collect()
}

fn cycle_order(cycle: i64) -> Vec<&'static String> {
    sorted_by_score(pool().keys(), &format!("trials-weekly-v1:{cycle}:"))
}

/// A seeded shuffle of the pool per cycle of N weeks: every behemoth once per cycle, in a random
/// order, and never the same behemoth two weeks running (also across a cycle boundary).
pub fn behemoth_for_week(week: i64) -> &'static str {
    let count = pool().len() as i64;
    if count == 0 {
        return "";
    }
    let cycle = week.div_euclid(count);
    let mut order = cycle_order(cycle);
    if count > 1 && order[0] == cycle_order(cycle - 1)[count as usize - 1] {
        order.swap(0, 1);
    }
    order[week.rem_euclid(count) as usize]
}

/// One of the behemoth's authored rows, shared by Normal and Dauntless.
pub fn suffix_for_week(week: i64) -> &'static str {
    let behemoth = behemoth_for_week(week);
    pool()
        .get(behemoth)
        .and_then(|suffixes| {
            sorted_by_score(suffixes.iter(), &format!("trials-weekly-v1:row:{week}:"))
                .first()
                .copied()
        })
        .map_or("", String::as_str)
}

/// The rotation week at `at` (milliseconds), from `TRIAL_ROTATION_START` when the host set one.
pub fn week_at(rotation_start: Option<&str>, at: i64) -> Result<i64, String> {
    let epoch = service_kit::js::date_parse(rotation_start.unwrap_or(ROTATION_START))
        .ok_or_else(|| "Invalid TRIAL_ROTATION_START".to_owned())?;
    Ok((at - epoch).div_euclid(WEEK_MS))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialsData {
    pub behemoth: String,
    pub hunt_id: String,
}

/// This week's Trial at Normal (`Hard`) or Dauntless (`Elite`) difficulty.
pub fn trials_data(elite: bool, rotation_start: Option<&str>, at: i64) -> Result<TrialsData, String> {
    let difficulty = if elite { "Elite" } else { "Hard" };
    let hunt_id = format!(
        "Arena_MatchmakerHunt_{difficulty}_{}",
        suffix_for_week(week_at(rotation_start, at)?)
    );
    let table = if elite {
        tables::trials_elite()
    } else {
        tables::trials_hard()
    };
    let row = tables::rows(table)
        .get(&hunt_id)
        .ok_or_else(|| format!("Trials rotation row {hunt_id} is missing"))?;
    Ok(TrialsData {
        behemoth: tables::behemoth_asset(row).unwrap_or("").to_owned(),
        hunt_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> i64 {
        service_kit::js::date_parse(text).unwrap()
    }

    #[test]
    fn pools_only_variants_with_a_heroic_hunt_of_the_same_variant() {
        let matchmaker = tables::rows(tables::matchmaker_hunts());
        let of = |suffix: &str| -> std::collections::HashSet<String> {
            matchmaker
                .iter()
                .filter(|(id, _)| id.ends_with(suffix))
                .map(|(_, row)| blueprint(Some(row)))
                .collect()
        };
        let (alpha, heroic) = (of("_Alpha"), of("_Heroic"));
        assert_eq!(pool().len(), 12);
        let (hard, elite) = (
            tables::rows(tables::trials_hard()),
            tables::rows(tables::trials_elite()),
        );
        validate(pool(), hard, elite).unwrap();
        for behemoth in pool().keys() {
            assert!(alpha.contains(behemoth) && heroic.contains(behemoth), "{behemoth}");
        }
    }

    #[test]
    fn never_a_base_behemoth_never_the_same_two_weeks_running() {
        let mut previous = "";
        for week in -30..400 {
            let behemoth = behemoth_for_week(week);
            assert!(behemoth.ends_with("_alpha"));
            assert!(pool()[behemoth].iter().any(|s| s == suffix_for_week(week)));
            assert_ne!(behemoth, previous, "week {week}");
            previous = behemoth;
        }
        let count = pool().len() as i64;
        let seen: std::collections::HashSet<&str> = (0..count).map(|i| behemoth_for_week(5 * count + i)).collect();
        assert_eq!(seen.len() as i64, count);
    }

    #[test]
    fn matches_the_pool_compiled_into_the_dll() {
        let header = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../native/internal-server/TrialsRotation.h"),
        )
        .unwrap();
        let mut compiled = Pool::new();
        let mut rest = header.as_str();
        // {"name", {"001", "002"}}
        while let Some(start) = rest.find("{\"") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find('"') else { break };
            let name = &rest[..end];
            let after = &rest[end + 1..];
            if !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') || !after.starts_with(", {") {
                continue;
            }
            let close = after.find('}').unwrap();
            let suffixes = after[3..close]
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_owned())
                .filter(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
                .collect();
            compiled.insert(name.to_owned(), suffixes);
            rest = &after[close..];
        }
        assert_eq!(&compiled, pool());
    }

    #[test]
    fn rejects_a_pool_row_that_does_not_match_its_behemoth() {
        let (hard, elite) = (
            tables::rows(tables::trials_hard()),
            tables::rows(tables::trials_elite()),
        );
        let mut wrong = Pool::new();
        let mut rows = pool()["lerawr_alpha"].clone();
        rows.push(pool()["host_alpha"][0].clone());
        wrong.insert("lerawr_alpha".into(), rows);
        assert!(
            validate(&wrong, hard, elite)
                .unwrap_err()
                .contains("is not lerawr_alpha")
        );
        let mut base = Pool::new();
        base.insert("lerawr_normal".into(), pool()["lerawr_alpha"].clone());
        assert!(validate(&base, hard, elite).unwrap_err().contains("not a variant"));
    }

    #[test]
    fn changes_at_the_thursday_reset_and_holds_all_week() {
        let start = at(ROTATION_START);
        assert_eq!(week_at(None, start), Ok(0));
        assert_eq!(week_at(None, start - 1), Ok(-1));
        let first = trials_data(false, None, start).unwrap();
        assert_ne!(trials_data(false, None, start - 1).unwrap().behemoth, first.behemoth);
        assert_eq!(
            trials_data(false, None, start + 6 * 86_400_000 + 23 * 3_600_000).unwrap(),
            first
        );
        assert_ne!(
            trials_data(false, None, start + WEEK_MS).unwrap().behemoth,
            first.behemoth
        );
        for week in 0..50 {
            let when = start + week * WEEK_MS;
            let (normal, dauntless) = (
                trials_data(false, None, when).unwrap(),
                trials_data(true, None, when).unwrap(),
            );
            assert_eq!(
                normal.hunt_id.replace("_Hard_", "_"),
                dauntless.hunt_id.replace("_Elite_", "_")
            );
            assert_eq!(normal.behemoth, dauntless.behemoth);
            assert!(normal.behemoth.contains("_alpha_bp."));
        }
    }

    #[test]
    fn a_host_override_for_the_epoch() {
        let overridden = trials_data(false, Some("2026-10-01T00:00:00.000Z"), at("2026-10-01T12:00:00.000Z")).unwrap();
        assert!(overridden.hunt_id.ends_with(&format!("_{}", suffix_for_week(0))));
        assert_eq!(
            trials_data(false, Some("not-a-date"), 0).unwrap_err(),
            "Invalid TRIAL_ROTATION_START"
        );
    }
}
