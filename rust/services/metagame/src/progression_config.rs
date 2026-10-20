//! The progression config: Hunt Pass seasons and mastery tracks (`controllers/progressionconfig.ts`,
//! MIGRATION_PLAN.md M26).
//!
//! The bundled `vendor/progression_config.json`, unless `PROGRESSION_CONFIG_DIR` names a folder of
//! `.json` files, each one path, an array of paths or a whole config (`{"payload": {"paths": …}}`). A
//! path replaces the bundled one with the same `progression_id` in its place; a new id is added at
//! the end. `ACTIVE_HUNT_PASS` (default `season09b`) must be a loaded path. Anything wrong stops the
//! start with the same message as Node's. Paths stay JSON values in file order, so
//! `GET /progression/config` serves the same text; the parts the server computes with are typed.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

pub const DEFAULT_ACTIVE_HUNT_PASS: &str = "season09b";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requirement {
    pub rank_id: u64,
    pub xp_required: u64,
}

/// One track: its JSON as loaded, and what the rank math needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressionPath {
    pub id: String,
    pub requirements: Vec<Requirement>,
    pub prestige_xp_per_level: Option<u64>,
    pub premium_gating_entitlement: Option<String>,
    pub json: Value,
}

#[derive(Debug, Clone)]
pub struct ProgressionConfig {
    pub paths: Vec<ProgressionPath>,
    by_id: HashMap<String, usize>,
    /// The body of `GET /progression/config`.
    pub payload: Value,
    pub active_hunt_pass: String,
    /// What was loaded, for the start log.
    pub description: String,
}

impl ProgressionConfig {
    pub fn path(&self, id: &str) -> Option<&ProgressionPath> {
        self.by_id.get(id).map(|&index| &self.paths[index])
    }
}

/// `Number.isSafeInteger(value) && value >= 0`: also `5.0`, which JavaScript reads as 5.
fn whole(value: &Value) -> Option<u64> {
    const MAX_SAFE: f64 = 9_007_199_254_740_991.0;
    value.as_u64().filter(|n| *n as f64 <= MAX_SAFE).or_else(|| {
        value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= MAX_SAFE)
            .map(|n| n as u64)
    })
}

fn display(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// `AssertValidPath`.
fn validate(json: Value, source: &str) -> Result<ProgressionPath, ConfigError> {
    let fail = |message: String| Err(ConfigError(message));
    let Some(object) = json.as_object() else {
        return fail(format!("{source}: not a progression path object"));
    };
    let name = format!("{source} ({})", display(object.get("progression_id")));
    let id = match object.get("progression_id") {
        Some(Value::String(id)) if !id.is_empty() => id.clone(),
        _ => return fail(format!("{source}: a path has no progression_id")),
    };
    let Some(list) = object
        .get("requirements")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
    else {
        return fail(format!("{name}: no requirements, so no rank could ever be reached"));
    };
    let mut requirements = Vec::with_capacity(list.len());
    let mut previous: i64 = -1;
    for entry in list {
        let (Some(rank_id), Some(xp_required)) = (
            entry.get("rank_id").and_then(whole),
            entry.get("xp_required").and_then(whole),
        ) else {
            return fail(format!(
                "{name}: a requirement is not a {{rank_id, xp_required}} pair of whole numbers"
            ));
        };
        if rank_id as i64 <= previous {
            return fail(format!(
                "{name}: requirements are not in rising rank_id order ({rank_id} after {previous})"
            ));
        }
        previous = rank_id as i64;
        requirements.push(Requirement { rank_id, xp_required });
    }
    for field in ["free_rewards", "premium_rewards"] {
        if object.get(field).is_some_and(|value| !value.is_array()) {
            return fail(format!("{name}: {field} is not an array"));
        }
    }
    let premium_gating_entitlement = match object.get("premium_gating_entitlement") {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => return fail(format!("{name}: premium_gating_entitlement is not a string")),
    };
    let prestige_xp_per_level = match object.get("prestige") {
        None | Some(Value::Null) => None,
        Some(prestige) => match prestige.get("xp_per_level").and_then(whole).filter(|n| *n > 0) {
            Some(n) if prestige.is_object() => Some(n),
            _ => return fail(format!("{name}: prestige needs a whole xp_per_level above 0")),
        },
    };
    Ok(ProgressionPath {
        id,
        requirements,
        prestige_xp_per_level,
        premium_gating_entitlement,
        json,
    })
}

/// `ReadPathsFromFolder`.
fn read_folder(folder: &Path) -> Result<Vec<ProgressionPath>, ConfigError> {
    let shown = folder.display();
    let metadata =
        std::fs::metadata(folder).map_err(|_| ConfigError(format!("PROGRESSION_CONFIG_DIR {shown} does not exist")))?;
    if !metadata.is_dir() {
        return Err(ConfigError(format!("PROGRESSION_CONFIG_DIR {shown} is not a folder")));
    }
    let mut files: Vec<String> = std::fs::read_dir(folder)
        .map_err(|e| ConfigError(format!("PROGRESSION_CONFIG_DIR {shown}: {e}")))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.to_lowercase().ends_with(".json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(ConfigError(format!(
            "PROGRESSION_CONFIG_DIR {shown} holds no .json file"
        )));
    }
    let mut paths = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    for name in files {
        let file = folder.join(&name);
        let file_name = file.display().to_string();
        let text =
            std::fs::read_to_string(&file).map_err(|e| ConfigError(format!("{file_name} is not valid JSON: {e}")))?;
        let parsed: Value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text))
            .map_err(|e| ConfigError(format!("{file_name} is not valid JSON: {e}")))?;
        let list = match parsed {
            Value::Array(list) => list,
            Value::Object(ref object)
                if object
                    .get("payload")
                    .and_then(|p| p.get("paths"))
                    .is_some_and(Value::is_array) =>
            {
                parsed["payload"]["paths"].as_array().cloned().unwrap_or_default()
            }
            other => vec![other],
        };
        for entry in list {
            let path = validate(entry, &file_name)?;
            if let Some(other) = seen.get(&path.id) {
                return Err(ConfigError(format!("{file_name}: {} is also in {other}", path.id)));
            }
            seen.insert(path.id.clone(), file_name.clone());
            paths.push(path);
        }
    }
    Ok(paths)
}

/// `Load`: the bundled config, overridden from `folder`, with `active` as the default Hunt Pass.
pub fn load(bundled: &Value, folder: Option<&Path>, active: &str) -> Result<ProgressionConfig, ConfigError> {
    let bundled_paths: Vec<ProgressionPath> = bundled["payload"]["paths"]
        .as_array()
        .ok_or_else(|| ConfigError("the bundled progression config has no payload.paths".into()))?
        .iter()
        .map(|json| validate(json.clone(), "bundled progression_config.json"))
        .collect::<Result<_, _>>()?;
    let mut paths = bundled_paths.clone();
    let mut payload = bundled.clone();
    let mut description = format!("bundled, {} tracks", paths.len());

    if let Some(folder) = folder {
        let from_disk = read_folder(folder)?;
        let mut notes = Vec::new();
        for path in paths.iter_mut() {
            if let Some(replacement) = from_disk.iter().find(|entry| entry.id == path.id) {
                notes.push(format!("{} replaced", path.id));
                *path = replacement.clone();
            }
        }
        for entry in &from_disk {
            if !bundled_paths.iter().any(|path| path.id == entry.id) {
                notes.push(format!("{} added", entry.id));
                paths.push(entry.clone());
            }
        }
        // {...bundled, payload: {...bundled.payload, paths}}: the other keys stay where they are.
        if let Some(Value::Object(inner)) = payload.get_mut("payload") {
            inner.insert(
                "paths".into(),
                Value::Array(paths.iter().map(|path| path.json.clone()).collect()),
            );
        }
        description = format!(
            "{} tracks, from {}: {}",
            paths.len(),
            folder.display(),
            notes.join(", ")
        );
    }

    let by_id: HashMap<String, usize> = paths
        .iter()
        .enumerate()
        .map(|(index, path)| (path.id.clone(), index))
        .collect();
    if !by_id.contains_key(active) {
        let ids: Vec<&str> = paths.iter().map(|path| path.id.as_str()).collect();
        return Err(ConfigError(format!(
            "ACTIVE_HUNT_PASS is \"{active}\", which is not a loaded track ({})",
            ids.join(", ")
        )));
    }
    Ok(ProgressionConfig {
        paths,
        by_id,
        payload,
        active_hunt_pass: active.to_owned(),
        description: format!("{description}; active Hunt Pass {active}"),
    })
}

/// The config from the environment's `PROGRESSION_CONFIG_DIR` and `ACTIVE_HUNT_PASS`.
pub fn from_env(env: &service_kit::env::Env) -> Result<ProgressionConfig, ConfigError> {
    let folder = env
        .get("PROGRESSION_CONFIG_DIR")
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let active = env
        .get("ACTIVE_HUNT_PASS")
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(DEFAULT_ACTIVE_HUNT_PASS);
    load(crate::vendor::progression_config(), folder.map(Path::new), active)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn bundled() -> &'static Value {
        crate::vendor::progression_config()
    }

    #[test]
    fn the_bundled_config_loads_and_serves_unchanged() {
        let config = load(bundled(), None, DEFAULT_ACTIVE_HUNT_PASS).unwrap();
        assert_eq!(&config.payload, bundled());
        assert!(config.path("season09b").is_some());
        assert!(config.description.starts_with("bundled, "));
    }

    #[test]
    fn a_folder_replaces_in_place_and_adds_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let first_id = bundled()["payload"]["paths"][0]["progression_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let replaced = json!({"progression_id": first_id, "requirements": [{"rank_id": 1, "xp_required": 5.0}]});
        let added = json!({"payload": {"paths": [{"progression_id": "season99", "requirements": [{"rank_id": 0, "xp_required": 0}], "prestige": {"xp_per_level": 10}}]}});
        std::fs::write(dir.path().join("a.json"), format!("\u{feff}{replaced}")).unwrap();
        std::fs::write(dir.path().join("b.JSON"), added.to_string()).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        let config = load(bundled(), Some(dir.path()), "season99").unwrap();
        assert_eq!(
            config.paths[0].requirements,
            [Requirement {
                rank_id: 1,
                xp_required: 5
            }]
        );
        assert_eq!(config.paths.last().unwrap().prestige_xp_per_level, Some(10));
        assert_eq!(config.payload["payload"]["paths"][0], replaced);
        assert!(
            config
                .description
                .contains(&format!("{first_id} replaced, season99 added"))
        );
    }

    #[test]
    fn bad_configs_stop_the_start_with_nodes_messages() {
        let check = |path: Value| validate(path, "f.json").unwrap_err().0;
        assert_eq!(check(json!([])), "f.json: not a progression path object");
        assert_eq!(
            check(json!({"requirements": []})),
            "f.json: a path has no progression_id"
        );
        assert_eq!(
            check(json!({"progression_id": "s", "requirements": []})),
            "f.json (s): no requirements, so no rank could ever be reached"
        );
        assert_eq!(
            check(
                json!({"progression_id": "s", "requirements": [{"rank_id": 2, "xp_required": 0}, {"rank_id": 2, "xp_required": 1}]})
            ),
            "f.json (s): requirements are not in rising rank_id order (2 after 2)"
        );
        assert_eq!(
            check(json!({"progression_id": "s", "requirements": [{"rank_id": 1.5, "xp_required": 0}]})),
            "f.json (s): a requirement is not a {rank_id, xp_required} pair of whole numbers"
        );
        assert_eq!(
            check(
                json!({"progression_id": "s", "requirements": [{"rank_id": 0, "xp_required": 0}], "prestige": {"xp_per_level": 0}})
            ),
            "f.json (s): prestige needs a whole xp_per_level above 0"
        );
        let error = load(bundled(), None, "season00").unwrap_err().0;
        assert!(
            error.starts_with("ACTIVE_HUNT_PASS is \"season00\", which is not a loaded track ("),
            "{error}"
        );
        let missing = load(bundled(), Some(Path::new("no-such-folder")), DEFAULT_ACTIVE_HUNT_PASS)
            .unwrap_err()
            .0;
        assert_eq!(missing, "PROGRESSION_CONFIG_DIR no-such-folder does not exist");
    }
}
