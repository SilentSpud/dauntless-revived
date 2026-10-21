//! The Node metagame's Drizzle migrations, frozen (D11, X1): what an offline database or `.drsave` save from an
//! older launcher is brought forward with before the Diesel baseline adopts it. `tests/offline_save.rs` checks
//! this list against `legacy/UndauntedMetagame/src/drizzle` while that folder exists.

use store::drizzle::Migration;

macro_rules! frozen {
    ($(($tag:literal, $when:literal)),* $(,)?) => {
        /// (tag, the journal's `when`, SQL), oldest first.
        const FROZEN: &[(&str, i64, &str)] = &[
            $(($tag, $when, include_str!(concat!("../../../../legacy/UndauntedMetagame/src/drizzle/", $tag, ".sql")))),*
        ];
    };
}

frozen![
    ("0000_natural_korg", 1_782_098_648_901),
    ("0001_slow_storm", 1_782_160_527_617),
    ("0002_gorgeous_saracen", 1_782_190_471_658),
    ("0003_slippery_blackheart", 1_782_580_106_809),
    ("0004_silky_lady_mastermind", 1_782_581_556_194),
    ("0005_mighty_zeigeist", 1_782_586_138_792),
    ("0006_woozy_slipstream", 1_782_664_386_398),
    ("0007_last_maximus", 1_783_236_454_860),
    ("0008_vengeful_spirit", 1_783_285_636_434),
    ("0009_nice_dazzler", 1_783_299_942_758),
    ("0010_save_history_and_item_log", 1_789_991_294_333),
    ("0011_real_progression", 1_789_993_643_640),
    ("0012_friends_and_blocks", 1_790_005_698_166),
    ("0013_guilds", 1_790_048_284_578),
    ("0014_escalation", 1_790_160_221_950),
    ("0015_store_purchases", 1_790_161_221_759),
    ("0016_slayer_links", 1_790_163_279_299),
    ("0017_discord_links", 1_791_000_000_000),
    ("0018_trials_leaderboards", 1_791_266_400_000),
    ("0019_trials_profiles", 1_791_309_600_000),
    ("0020_hunt_regions", 1_791_400_000_000),
    ("0021_onetime_jobs", 1_791_500_000_000),
    ("0022_germany_region", 1_791_600_000_000),
    ("0023_moderation", 1_791_700_000_000),
    ("0024_ephemeral_addresses", 1_791_800_000_000),
    ("0025_inventory_expiry_index", 1_791_900_000_000),
    ("0026_account_key_recovery", 1_792_000_000_000),
    ("0027_player_key_rotation", 1_792_100_000_000),
    ("0028_us_region", 1_792_200_000_000),
    ("0029_temporary_bans", 1_792_200_000_001),
    ("0030_device_signals", 1_792_200_000_002),
];

/// Every frozen Drizzle migration, oldest first.
pub fn migrations() -> Vec<Migration> {
    FROZEN
        .iter()
        .map(|(tag, created_at, sql)| Migration {
            tag: (*tag).to_owned(),
            created_at: *created_at,
            sql: (*sql).to_owned(),
        })
        .collect()
}
