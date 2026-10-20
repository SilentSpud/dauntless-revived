//! The vendor data the Node metagame imports as JSON modules (MIGRATION_PLAN.md M26): built into the
//! binary and parsed once, on first use. Each domain area gives its file typed structs when it is
//! ported (M29); until then they are JSON values, keys in file order.

use std::sync::OnceLock;

use serde_json::Value;

macro_rules! vendor {
    ($($name:ident => $file:literal),* $(,)?) => {
        /// The vendor files, by the name the Node code imports them under.
        pub const FILES: &[(&str, &str)] = &[$(($file, include_str!(concat!("../../../../UndauntedMetagame/src/vendor/", $file)))),*];

        $(
            pub fn $name() -> &'static Value {
                static PARSED: OnceLock<Value> = OnceLock::new();
                PARSED.get_or_init(|| parse(include_str!(concat!("../../../../UndauntedMetagame/src/vendor/", $file))))
            }
        )*
    };
}

vendor! {
    escalation_seasons => "escalation/seasons.json",
    hunt_titles => "hunt_titles.json",
    progression_config => "progression_config.json",
    store_catalog => "store_catalog.json",
    store_curated_30 => "store_curated_30.json",
    store_item_kinds => "store_item_kinds.json",
    store_ladyluck => "store_ladyluck.json",
    store_middleman => "store_middleman.json",
    store_middleman_cells => "store_middleman_cells.json",
    trials_pool => "trials_pool.json",
}

fn parse(text: &str) -> Value {
    // The files are checked by the test below; a broken one is a build defect, not a runtime case.
    serde_json::from_str(text).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_vendor_file_parses() {
        for (name, text) in super::FILES {
            assert!(serde_json::from_str::<serde_json::Value>(text).is_ok(), "{name}");
        }
        assert!(super::progression_config()["payload"]["paths"].is_array());
    }
}
