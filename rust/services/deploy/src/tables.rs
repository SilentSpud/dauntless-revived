//! The hunt tables the Node deploy server imports as JSON modules: built into the binary and parsed
//! once, on first use. Rows are looked up by id; `None` where Node would read a missing row.

use std::sync::OnceLock;

use serde_json::{Map, Value};

macro_rules! vendor {
    ($($name:ident => $file:literal),* $(,)?) => {
        /// The vendor files, by name.
        pub const FILES: &[(&str, &str)] = &[$(($file, include_str!(concat!("../../../../legacy/UndauntedDeployServer/src/vendor/", $file)))),*];

        $(
            pub fn $name() -> &'static Value {
                static PARSED: OnceLock<Value> = OnceLock::new();
                PARSED.get_or_init(|| {
                    serde_json::from_str(include_str!(concat!("../../../../legacy/UndauntedDeployServer/src/vendor/", $file)))
                        .unwrap_or(Value::Null)
                })
            }
        )*
    };
}

vendor! {
    player_hunts => "player_hunts_table.json",
    matchmaker_hunts => "matchmaker_hunts_table.json",
    trials_hard => "trials_hard_table.json",
    trials_elite => "trials_elite_table.json",
    trials_pool => "trials_pool.json",
}

/// `Table[0].Rows`.
pub fn rows(table: &'static Value) -> &'static Map<String, Value> {
    static EMPTY: OnceLock<Map<String, Value>> = OnceLock::new();
    table[0]["Rows"]
        .as_object()
        .unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}

pub fn matchmaker_row(id: &str) -> Option<&'static Value> {
    rows(matchmaker_hunts()).get(id)
}

/// The row of a hunt in the matchmaker table or either trials table (`MaxPlayersFromTables`).
pub fn any_hunt_row(id: &str) -> Option<&'static Value> {
    matchmaker_row(id)
        .or_else(|| rows(trials_hard()).get(id))
        .or_else(|| rows(trials_elite()).get(id))
}

/// `Row.SpecificBehemoth.BehemothAsset.AssetPathName`.
pub fn behemoth_asset(row: &Value) -> Option<&str> {
    row["SpecificBehemoth"]["BehemothAsset"]["AssetPathName"].as_str()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_table_parses() {
        for (name, text) in super::FILES {
            assert!(serde_json::from_str::<serde_json::Value>(text).is_ok(), "{name}");
        }
        assert!(super::rows(super::matchmaker_hunts()).len() > 100);
        assert!(super::matchmaker_row("CR19_MatchmakerHunt_ShatteredIsles_TrainingDojo").is_some());
    }
}
