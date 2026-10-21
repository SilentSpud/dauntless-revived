//! The matchmaking call (`controllers/matchmaker.ts`, `matchmakinginput.ts`): where a player's
//! world starts, after a second check of what ends up on a game server's command line.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::capacity::{AdmissionStatus, DeployError, Reason};
use crate::cities::{CityRouter, Connection};
use crate::gameservers::{Deploy, Kind, kind_of};
use crate::remote::{
    HuntRouter, Region, RegionalRouter, Remote, overflow_url, read_worker_snapshot, region_load, remote_fn,
};

const MAX_EXPECTED_PLAYERS: usize = 16;

fn all_bytes(text: &str, ok: impl Fn(u8) -> bool) -> bool {
    !text.is_empty() && text.bytes().all(ok)
}

/// `^[A-Za-z0-9_+]{1,128}$`, e.g. `CR19_PlayerHunt_Patrol_Heroic+_Gem`.
fn is_hunt_id(text: &str) -> bool {
    text.len() <= 128 && all_bytes(text, |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'+')
}

/// `^/Game(/[A-Za-z0-9_]+)+(\.[A-Za-z0-9_]+)?$` (map) or with the `.Class` required (asset).
fn is_game_path(text: &str, class_required: bool) -> bool {
    let Some(rest) = text.strip_prefix("/Game/") else {
        return false;
    };
    let (path, class) = match rest.split_once('.') {
        Some((path, class)) => (path, Some(class)),
        None => (rest, None),
    };
    let word = |s: &str| all_bytes(s, |b| b.is_ascii_alphanumeric() || b == b'_');
    if class_required && class.is_none() {
        return false;
    }
    path.split('/').all(word) && class.is_none_or(word)
}

/// What is wrong with a matchmaking request, or `None` when it is acceptable. Kept in step with the
/// metagame's `CheckMatchmakingInput`.
pub fn check_request(
    game_mode: Option<&Value>,
    game_args: Option<&Value>,
    hunt_id: Option<&Value>,
    players: Option<&Value>,
) -> Option<&'static str> {
    fn present(v: Option<&Value>) -> Option<&Value> {
        v.filter(|v| !v.is_null())
    }
    if let Some(hunt) = present(hunt_id) {
        let ok = hunt.as_str().is_some_and(|h| h.is_empty() || is_hunt_id(h));
        if !ok {
            return Some("the hunt id is not a hunt id");
        }
    }
    if let Some(args) = present(game_args)
        && !args.is_string()
    {
        return Some("the game args are not a string");
    }
    if game_mode.and_then(Value::as_str) == Some("ISLAND")
        && let Some(args) = game_args.and_then(Value::as_str)
        && !service_kit::js::trim(args).is_empty()
    {
        let parts: Vec<&str> = args.split('?').collect();
        let behemoth = parts.get(2).and_then(|p| p.split('=').nth(1));
        let characters =
            args.len() <= 2048 && all_bytes(args, |b| b.is_ascii_alphanumeric() || b"_/.?=+-".contains(&b));
        let ok = characters
            && is_game_path(parts[0], false)
            && behemoth.is_some_and(|b| b.is_empty() || is_game_path(b, true));
        if !ok {
            return Some("the game args are not a map and behemoth this server can start");
        }
    }
    if let Some(players) = present(players) {
        let ok = players.as_array().is_some_and(|list| {
            list.len() <= MAX_EXPECTED_PLAYERS
                && list.iter().all(|p| {
                    p.as_str().is_some_and(|id| {
                        id.len() <= 64 && all_bytes(id, |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    })
                })
        });
        if !ok {
            return Some("the expected players are not a list of account ids");
        }
    }
    None
}

/// The matchmaking routes of one deploy server.
pub struct Matchmaker {
    pub deploy: Arc<Deploy>,
    pub remote: Arc<Remote>,
    hunts: HuntRouter,
    regions: RegionalRouter,
    cities: CityRouter,
}

impl Matchmaker {
    pub fn new(deploy: Arc<Deploy>) -> Arc<Self> {
        let remote = Arc::new(Remote::default());
        let settings = deploy.settings.clone();
        let hunts = {
            let (count, status) = (deploy.clone(), deploy.clone());
            HuntRouter::new(
                settings.clone(),
                move || {
                    count
                        .gameservers()
                        .iter()
                        .filter(|s| matches!(kind_of(s), Kind::Hunt | Kind::Tutorial))
                        .count() as u64
                },
                remote_fn(remote.clone()),
                Some(Arc::new(move || status.hunt_admission.status())),
            )
        };
        let regions = {
            let (deploy, http, settings_load) = (deploy.clone(), remote.http.clone(), settings.clone());
            RegionalRouter::new(
                settings.clone(),
                Arc::new(move || {
                    let (deploy, http, settings) = (deploy.clone(), http.clone(), settings_load.clone());
                    Box::pin(async move {
                        let local = deploy.hunt_admission.status()?;
                        let Some(url) = overflow_url(&settings).ok().flatten() else {
                            return Ok(local);
                        };
                        let worker = region_load(http)(url).await;
                        Ok(match worker {
                            Ok(worker) => AdmissionStatus {
                                running: local.running + worker.running,
                                pending: local.pending + worker.pending,
                                limit: local.limit.zip(worker.limit).map(|(a, b)| a + b),
                            },
                            Err(_) => local,
                        })
                    })
                }),
                remote_fn(remote.clone()),
                region_load(remote.http.clone()),
            )
        };
        let cities = {
            let (deploy, http) = (deploy.clone(), remote.http.clone());
            CityRouter::new(
                move || {
                    let (deploy, http) = (deploy.clone(), http.clone());
                    Box::pin(async move { city_counts(&deploy, &http).await })
                },
                crate::now_ms,
            )
        };
        Arc::new(Self {
            deploy,
            remote,
            hunts,
            regions,
            cities,
        })
    }

    /// `HandleMatchmakingRequest`.
    pub async fn handle(&self, body: &Value, region: Region) -> Result<Connection, DeployError> {
        let settings = &self.deploy.settings;
        let mode = body["GameMode"].as_str().unwrap_or("");
        let worker = settings.is("HUNT_WORKER", "1");
        if body["Overflow"] == true && worker && mode == "ISLAND" {
            let status = self.deploy.hunt_admission.status()?;
            let reserved = settings.number("AUS_RESERVED_HUNTS").unwrap_or(0.0);
            if !(reserved.fract() == 0.0 && reserved >= 0.0) {
                return Err(DeployError::failed("Invalid AUS_RESERVED_HUNTS"));
            }
            if let Some(limit) = status.limit
                && (status.running + status.pending) as f64 >= (limit as f64 - reserved).max(0.0)
            {
                return Err(DeployError::Capacity(Reason::Hunts));
            }
        }
        let accepted = mode == "ISLAND"
            || (mode == "CITY" && settings.is("WORKER_RAMSGATE", "1"))
            || (mode == "SHARED" && settings.is("WORKER_DOJO", "1"));
        if worker && !accepted {
            return Err(DeployError::failed("Hunt worker does not accept this world"));
        }
        let request = json!({
            "GameMode": body.get("GameMode"),
            "GameArgs": body.get("GameArgs"),
            "HuntId": body.get("HuntId"),
            "ExpectedPlayers": body.get("ExpectedPlayers"),
        });
        // JSON.stringify leaves out what is undefined
        let request = Value::Object(
            request
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        let party = body["ExpectedPlayers"].as_array().map_or(1, |p| p.len() as u64);
        self.regions
            .launch(&request, region, || async {
                if mode == "CITY" && !self.deploy.city_pool_enabled() && settings.is("CITY_OVERFLOW", "1") {
                    let url = overflow_url(settings)?
                        .ok_or_else(|| DeployError::failed("City overflow needs the worker tunnel"))?;
                    let limit = settings.number("RAMSGATE_PLAYER_LIMIT").unwrap_or(28.0);
                    return self
                        .cities
                        .launch(
                            party,
                            self.deploy.ramsgate_connection(party),
                            self.remote.launch(&url, &request),
                            limit,
                        )
                        .await;
                }
                self.hunts.launch(&request, self.local(body)).await
            })
            .await
    }

    async fn local(&self, body: &Value) -> Result<Connection, DeployError> {
        let mode = body["GameMode"].as_str();
        let args = body["GameArgs"].as_str();
        let hunt = body["HuntId"].as_str();
        let players: Option<Vec<String>> = body["ExpectedPlayers"]
            .as_array()
            .map(|list| list.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect());
        let party = players.as_ref().map_or(1, |p| p.len() as u64);
        tracing::info!(
            "Handling matchmaking with GameMode: {} HuntId: {} and GameArgs: {} and ExpectedPlayers {}",
            mode.unwrap_or("undefined"),
            hunt.unwrap_or("undefined"),
            args.unwrap_or("undefined"),
            players.as_ref().map_or_else(|| "undefined".to_owned(), |p| p.join(","))
        );
        let filled = |s: Option<&str>| s.is_some_and(|s| !service_kit::js::trim(s).is_empty());
        match mode {
            Some("CITY") => return self.deploy.ramsgate_connection(party).await,
            Some("SHARED") if filled(hunt) && hunt == Some("ShatteredIsles_TrainingDojo") => {
                return self.deploy.dojo_connection().await;
            }
            Some("ISLAND") => {
                if filled(args) {
                    return self.deploy.start_with_args(args.unwrap_or("")).await;
                }
                if let (true, Some(players)) = (filled(hunt), &players) {
                    return self.deploy.start_with_hunt(hunt.unwrap_or(""), players).await;
                }
            }
            _ => {}
        }
        if self.deploy.settings.is("HUNT_WORKER", "1") {
            return Err(DeployError::failed("No hunt could be resolved on worker"));
        }
        tracing::error!("Matchmaking failed, sending you to Ramsgate!");
        self.deploy.ramsgate_connection(party).await
    }
}

/// Players in this machine's city and in the worker's, for Ramsgate balancing.
async fn city_counts(deploy: &Deploy, http: &crate::remote::Http) -> Result<(f64, f64), DeployError> {
    let key = deploy
        .settings
        .non_empty("CITY_STATUS_KEY")
        .ok_or_else(|| DeployError::failed("CITY_STATUS_KEY is required for Ramsgate balancing"))?;
    let status_url = url::Url::parse("http://127.0.0.1:61000/undaunted/api/ServerStatus")
        .map_err(|e| DeployError::failed(e.to_string()))?;
    let worker = async {
        match overflow_url(&deploy.settings).ok().flatten() {
            Some(url) => read_worker_snapshot(http, &url).await.servers,
            None => Vec::new(),
        }
    };
    let key_header = [("x-undaunted-user-api-key", key.as_str())];
    let (answer, worker) = tokio::join!(
        http.call("GET", &status_url, None, Duration::from_secs(5), &key_header),
        worker
    );
    let unavailable = || DeployError::failed("City occupancy unavailable");
    let answer = answer.map_err(|_| unavailable())?;
    let status = answer.body.map_err(|_| unavailable())?;
    if !(200..300).contains(&answer.status) || status["limited"] == true || !status["instances"].is_array() {
        return Err(unavailable());
    }
    let count = |servers: &[Value]| -> f64 {
        servers.iter().find(|s| s["kind"] == "city").map_or(0.0, |city| {
            city["connectedPlayers"]
                .as_f64()
                .or_else(|| {
                    status["instances"]
                        .as_array()
                        .and_then(|list| list.iter().find(|i| i["id"] == city["id"]))
                        .and_then(|i| i["players"].as_f64())
                })
                .unwrap_or(0.0)
        })
    };
    let city = deploy.gameservers().into_iter().find(|s| s.is_ramsgate);
    let local = city
        .and_then(|c| deploy.connected_players(&c))
        .map_or_else(|| count(&deploy.describe()), |n| n as f64);
    Ok((local, count(&worker)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TUTORIAL_ARGS: &str = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";

    fn check(mode: Value, args: Value, hunt: Value, players: Value) -> Option<&'static str> {
        let opt = |v: &Value| (!v.is_null()).then_some(v.clone());
        let (mode, args, hunt, players) = (opt(&mode), opt(&args), opt(&hunt), opt(&players));
        check_request(mode.as_ref(), args.as_ref(), hunt.as_ref(), players.as_ref())
    }

    #[test]
    fn accepts_every_shape_the_metagame_sends() {
        let n = Value::Null;
        assert_eq!(check(json!("ISLAND"), json!(TUTORIAL_ARGS), n.clone(), n.clone()), None);
        assert_eq!(
            check(
                json!("CITY"),
                json!(""),
                json!("ShatteredIsles_ReturnToRamsgate"),
                n.clone()
            ),
            None
        );
        assert_eq!(
            check(
                json!("SHARED"),
                json!(""),
                json!("ShatteredIsles_TrainingDojo"),
                n.clone()
            ),
            None
        );
        assert_eq!(
            check(
                json!("ISLAND"),
                json!(""),
                json!("CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr"),
                json!(["UID-00000000-0000-4000-8000-00000000f609"])
            ),
            None
        );
        assert_eq!(
            check(
                json!("ISLAND"),
                json!(""),
                json!("CR19_PlayerHunt_Patrol_Heroic+_Gem"),
                json!(["UID-a", "UID-b", "UID-c", "UID-d"])
            ),
            None
        );
        assert_eq!(check(n.clone(), n.clone(), n.clone(), n), None);
    }

    #[test]
    fn refuses_anything_else_on_a_command_line() {
        let n = Value::Null;
        let hunt = json!("CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr");
        let seventeen: Vec<String> = (0..17).map(|i| format!("UID-{i}")).collect();
        for (mode, args, hunt, players) in [
            (
                json!("ISLAND"),
                json!("/Game/Maps/islands/x -ExecCmds=quit?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"),
                n.clone(),
                n.clone(),
            ),
            (
                json!("ISLAND"),
                json!("/Game/Maps/islands/x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C \"-log\""),
                n.clone(),
                n.clone(),
            ),
            (json!("ISLAND"), json!("/Game/Maps/islands/x"), n.clone(), n.clone()),
            (
                json!("ISLAND"),
                json!("/Game/../Maps/x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"),
                n.clone(),
                n.clone(),
            ),
            (
                json!("ISLAND"),
                json!("-server?MaxPlayers=1?MonsterClass=/Game/M/b.b_C"),
                n.clone(),
                n.clone(),
            ),
            (json!("ISLAND"), json!(42), n.clone(), n.clone()),
            (
                json!("ISLAND"),
                json!(""),
                json!("CR19_PlayerHunt_Arena -ExecCmds=quit"),
                json!(["UID-a"]),
            ),
            (
                json!("ISLAND"),
                json!(""),
                json!("CR19_Arena:x,UID-evil:y"),
                json!(["UID-a"]),
            ),
            (json!("ISLAND"), json!(""), json!({ "toString": 1 }), json!(["UID-a"])),
            (json!("ISLAND"), json!(""), hunt.clone(), json!(["UID-a:CR19_x,UID-b"])),
            (json!("ISLAND"), json!(""), hunt.clone(), json!(["UID a"])),
            (json!("ISLAND"), json!(""), hunt.clone(), json!("UID-a")),
            (json!("ISLAND"), json!(""), hunt, json!(seventeen)),
        ] {
            assert!(
                check(mode, args.clone(), hunt.clone(), players.clone()).is_some(),
                "{args} {hunt} {players}"
            );
        }
    }
}
