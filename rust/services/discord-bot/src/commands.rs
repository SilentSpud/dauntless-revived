//! The slash commands (`commands.mjs`, `counter.mjs`). Registration keeps command ids and every
//! other command: an unchanged `/key` is left alone (clients cache its version), a changed one is
//! patched in place, and the old per-guild copies are removed.

use serde_json::{Value, json};

use crate::keys::{BoxFuture, Failure};

/// Discord's REST API, as far as registration needs it.
pub trait Rest: Send + Sync {
    fn get(&self, route: &str) -> BoxFuture<'_, Result<Value, Failure>>;
    fn post(&self, route: &str, body: &Value) -> BoxFuture<'_, Result<Value, Failure>>;
    fn patch(&self, route: &str, body: &Value) -> BoxFuture<'_, Result<Value, Failure>>;
    fn delete(&self, route: &str) -> BoxFuture<'_, Result<(), Failure>>;
}

fn string_option(name: &str, description: &str, required: bool, min: u32, max: u32) -> Value {
    json!({ "type": 3, "name": name, "description": description, "required": required, "min_length": min, "max_length": max })
}

fn subcommand(name: &str, description: &str, options: Vec<Value>) -> Value {
    let mut out = json!({ "type": 1, "name": name, "description": description });
    if !options.is_empty() {
        out["options"] = json!(options);
    }
    out
}

/// `/key`. `recover` is only offered when its separately authenticated backend is configured.
pub fn key_command(recovery: bool) -> Value {
    let mut options = vec![
        subcommand(
            "resend",
            "Resend your saved unused invite privately; never creates another",
            vec![],
        ),
        subcommand(
            "recover",
            "Privately rotate your verified linked account key",
            vec![string_option(
                "confirm",
                "Your private five-minute recovery challenge",
                false,
                64,
                64,
            )],
        ),
        subcommand("claim", "Receive your launcher invite by direct message", vec![]),
        subcommand("status", "Check your account link or code redemption", vec![]),
        subcommand(
            "link",
            "Link your existing account using its saved account key, not a Join invite",
            vec![string_option(
                "key",
                "Settings > Save a backup of your key > copy the Key: value (not your Join invite)",
                true,
                8,
                2048,
            )],
        ),
    ];
    if !recovery {
        options.retain(|o| o["name"] != "recover");
    }
    json!({ "name": "key", "description": "Your Dauntless Revived access code", "contexts": [0, 1], "options": options })
}

/// The owner's `/counter` (guild only).
pub fn counter_command() -> Value {
    json!({
        "name": "counter",
        "description": "Configure the live member counter (owner only)",
        "contexts": [0],
        "options": [{ "type": 3, "name": "category_id", "description": "Discord category ID", "required": true }],
    })
}

fn option_shape(option: &Value) -> Value {
    let options: Vec<Value> = option["options"]
        .as_array()
        .map(|o| o.iter().map(option_shape).collect())
        .unwrap_or_default();
    json!({
        "type": option["type"],
        "name": option["name"],
        "description": option["description"],
        "required": option["required"].as_bool().unwrap_or(false),
        "min_length": option["min_length"],
        "max_length": option["max_length"],
        "options": options,
    })
}

/// What registration compares: names, descriptions, limits and contexts.
fn shape(command: &Value, guild: bool) -> Value {
    let mut out = option_shape(command);
    out["type"] = json!(command["type"].as_u64().unwrap_or(1));
    let mut contexts: Vec<u64> = if guild {
        Vec::new()
    } else {
        command["contexts"]
            .as_array()
            .map(|c| c.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default()
    };
    contexts.sort_unstable();
    out["contexts"] = json!(contexts);
    out
}

/// Registers `/key` globally (one command serves guilds and DMs) and removes old guild copies;
/// how many scopes changed.
pub async fn sync_key_commands(
    rest: &dyn Rest,
    application: &str,
    guilds: &[String],
    command: &Value,
) -> Result<usize, Failure> {
    let route = format!("/applications/{application}/commands");
    let existing = rest.get(&route).await?;
    let existing = existing
        .as_array()
        .and_then(|list| list.iter().find(|c| c["name"] == "key" && c["type"] == 1))
        .cloned();
    let mut changed = 0;
    match existing {
        Some(existing) if shape(&existing, false) == shape(command, false) => {}
        Some(existing) => {
            rest.patch(&format!("{route}/{}", existing["id"].as_str().unwrap_or("")), command)
                .await?;
            changed += 1;
        }
        None => {
            rest.post(&route, command).await?;
            changed += 1;
        }
    }
    let mut seen = std::collections::HashSet::new();
    for guild in guilds.iter().filter(|g| seen.insert(g.to_string())) {
        let guild_route = format!("/applications/{application}/guilds/{guild}/commands");
        let commands = rest.get(&guild_route).await?;
        for command in commands.as_array().into_iter().flatten() {
            if command["name"] == "key" && command["type"] == 1 {
                rest.delete(&format!("{guild_route}/{}", command["id"].as_str().unwrap_or("")))
                    .await?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

pub async fn sync_counter_command(rest: &dyn Rest, application: &str) -> Result<(), Failure> {
    let route = format!("/applications/{application}/commands");
    let command = counter_command();
    let existing = rest.get(&route).await?;
    let old = existing
        .as_array()
        .and_then(|list| list.iter().find(|c| c["name"] == "counter" && c["type"] == 1))
        .cloned();
    match old {
        None => {
            rest.post(&route, &command).await?;
        }
        Some(old) if old["description"] != command["description"] || old["options"][0]["name"] != "category_id" => {
            rest.patch(&format!("{route}/{}", old["id"].as_str().unwrap_or("")), &command)
                .await?;
        }
        Some(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    type Calls = Arc<Mutex<Vec<(String, String, Value)>>>;

    struct Fake {
        answer: Box<dyn Fn(&str) -> Value + Send + Sync>,
        calls: Calls,
    }

    impl Rest for Fake {
        fn get(&self, route: &str) -> BoxFuture<'_, Result<Value, Failure>> {
            let answer = (self.answer)(route);
            Box::pin(async move { Ok(answer) })
        }

        fn post(&self, route: &str, body: &Value) -> BoxFuture<'_, Result<Value, Failure>> {
            self.calls
                .lock()
                .unwrap()
                .push(("post".into(), route.into(), body.clone()));
            Box::pin(async { Ok(Value::Null) })
        }

        fn patch(&self, route: &str, body: &Value) -> BoxFuture<'_, Result<Value, Failure>> {
            self.calls
                .lock()
                .unwrap()
                .push(("patch".into(), route.into(), body.clone()));
            Box::pin(async { Ok(Value::Null) })
        }

        fn delete(&self, route: &str) -> BoxFuture<'_, Result<(), Failure>> {
            self.calls
                .lock()
                .unwrap()
                .push(("delete".into(), route.into(), Value::Null));
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn an_unchanged_command_keeps_its_version_and_guild_copies_go() {
        let command = key_command(false);
        let calls: Calls = Arc::default();
        let registered = command.clone();
        let rest = Fake {
            answer: Box::new(move |route| {
                let mut c = registered.clone();
                c["id"] = json!("existing");
                c["version"] = json!("cached-version");
                c["type"] = json!(1);
                c["contexts"] = if route.contains("/guilds/") {
                    Value::Null
                } else {
                    json!([1, 0])
                };
                json!([c])
            }),
            calls: calls.clone(),
        };
        assert_eq!(
            sync_key_commands(&rest, "app", &["guild".into(), "guild".into()], &command)
                .await
                .unwrap(),
            1
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "delete");
    }

    #[tokio::test]
    async fn an_outdated_command_is_patched_in_place() {
        let command = key_command(false);
        let calls: Calls = Arc::default();
        let registered = command.clone();
        let rest = Fake {
            answer: Box::new(move |route| {
                if route.contains("/guilds/") {
                    return json!([{ "type": 1, "name": "other", "id": "other" }]);
                }
                let mut c = registered.clone();
                c["type"] = json!(1);
                c["id"] = json!("keep-this-id");
                c["options"] = json!(registered["options"].as_array().unwrap()[..2]);
                json!([c])
            }),
            calls: calls.clone(),
        };
        assert_eq!(
            sync_key_commands(&rest, "app", &["guild".into()], &command)
                .await
                .unwrap(),
            1
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.ends_with("/keep-this-id"));
        assert_eq!(
            calls[0].2["options"].as_array().unwrap().last().unwrap()["name"],
            "link"
        );
    }

    #[test]
    fn recover_only_with_its_backend() {
        let names = |c: Value| {
            c["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o["name"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(key_command(false)), ["resend", "claim", "status", "link"]);
        assert_eq!(
            names(key_command(true)),
            ["resend", "recover", "claim", "status", "link"]
        );
    }
}
