//! `progressionrank.test.ts`: the built-in progression config and the client's rank math. Node's
//! `XpInLevel` (shown only by the client) has no Rust counterpart: the server decides nothing on it.

use metagame::progression_config::ProgressionConfig;
use metagame::realprogression::{earned_ranks, max_rank_id, total_xp_to_max_rank};
use service_kit::env::Env;

const WEAPONS: [&str; 7] = [
    "Axe",
    "ChainBlades",
    "Hammer",
    "Repeaters",
    "Spear",
    "Sword",
    "Strikers",
];

fn config() -> ProgressionConfig {
    metagame::progression_config::from_env(&Env::from_pairs([])).unwrap()
}

fn weapons() -> Vec<String> {
    WEAPONS.iter().map(|w| format!("MasteryTrack_Weapon_{w}")).collect()
}

fn earned(config: &ProgressionConfig, id: &str, progress: i64, premium: bool) -> (i64, i64) {
    earned_ranks(config.path(id).unwrap(), progress, premium)
}

#[test]
fn has_the_10_tracks_the_roadmap_lists_requirements_sorted_by_rank_id() {
    let config = config();
    let mut ids: Vec<String> = config.paths.iter().map(|p| p.id.clone()).collect();
    ids.sort();
    let mut want = vec![
        "MasteryTrack_Behemoth".to_owned(),
        "MasteryTrack_PlayerLevel".to_owned(),
        "season09b".to_owned(),
    ];
    want.extend(weapons());
    want.sort();
    assert_eq!(ids, want);
    for path in &config.paths {
        let ranks: Vec<u64> = path.requirements.iter().map(|r| r.rank_id).collect();
        let mut sorted = ranks.clone();
        sorted.sort_unstable();
        assert_eq!(ranks, sorted, "{} is not sorted", path.id);
    }
}

#[test]
fn totals_playerlevel_1_111_behemoth_411_weapons_115_season09b_5_000_plus_prestige() {
    let config = config();
    let totals = |id: &str| {
        let path = config.path(id).unwrap();
        (total_xp_to_max_rank(path), max_rank_id(path))
    };
    assert_eq!(totals("MasteryTrack_PlayerLevel"), (1111, 50));
    assert_eq!(totals("MasteryTrack_Behemoth"), (411, 50));
    for weapon in weapons() {
        assert_eq!(totals(&weapon), (115, 20), "{weapon}");
    }
    assert_eq!(totals("season09b"), (5000, 50));
    let season = config.path("season09b").unwrap();
    assert_eq!(season.prestige_xp_per_level, Some(100));
    assert_eq!(season.premium_gating_entitlement.as_deref(), Some("season09b_premium"));
}

#[test]
fn playerlevel_progress_0_is_rank_1_1_111_is_rank_50_one_xp_short_is_49() {
    let config = config();
    let rank = |progress| earned(&config, "MasteryTrack_PlayerLevel", progress, false).0;
    assert_eq!([rank(0), rank(1111), rank(1110), rank(999_999)], [1, 50, 49, 50]);
}

#[test]
fn weapons_115_is_rank_20_114_is_19_0_is_0_behemoth_411_is_50() {
    let config = config();
    for weapon in weapons() {
        let rank = |progress| earned(&config, &weapon, progress, false).0;
        assert_eq!([rank(115), rank(114), rank(0)], [20, 19, 0], "{weapon}");
    }
    assert_eq!(earned(&config, "MasteryTrack_Behemoth", 0, false).0, 0);
    assert_eq!(earned(&config, "MasteryTrack_Behemoth", 411, false).0, 50);
}

#[test]
fn season09b_100_xp_per_rank_then_one_prestige_level_per_100_xp_past_5_000() {
    let config = config();
    for (progress, rank) in [
        (0, 0),
        (99, 0),
        (100, 1),
        (250, 2),
        (4999, 49),
        (5000, 50),
        (5099, 50),
        (5100, 51),
        (5250, 52),
    ] {
        assert_eq!(
            earned(&config, "season09b", progress, false).0,
            rank,
            "progress {progress}"
        );
    }
}

#[test]
fn premium_rank_follows_the_free_rank_only_with_premium() {
    let config = config();
    assert_eq!(earned(&config, "season09b", 5100, true), (51, 51));
    assert_eq!(earned(&config, "season09b", 5100, false).1, 0);
    assert_eq!(earned(&config, "season09b", 300, true).1, 3);
}

#[test]
fn the_old_stub_values_earned_never_exceeds_the_stub_s_confirmed_rank_so_nothing_was_pending() {
    let config = config();
    let big = 99_999_999;
    assert_eq!(earned(&config, "season09b", big, false).0, 50 + (big - 5000) / 100);
    assert!(earned(&config, "season09b", big, false).0 <= big);
    assert_eq!(earned(&config, "MasteryTrack_Weapon_Sword", big, false).0, 20);
}
