//! The server's identity and status (`controllers/serverstatus.ts`): the in-game banner of GET
//! /dauntless-status, and GET /undaunted/api/ServerStatus for the friend launcher. Nothing in it may
//! identify an account beyond its username: no account ids, keys, tokens or addresses.
//!
//! Two variants of the status, same shape: "full" for a registered player (players, per-server
//! counts and game servers), "limited" for everyone else (the server's name, version, source,
//! registration mode, content port and uptime, with nobody listed). Each has its own 5 s cache.
//!
//! Accuracy is best effort: "online" is a client heartbeat in the last 90 s; "where" comes from the
//! heartbeat's map and state; a player in a hunt is tied to a hunt server by the deploy server's
//! expected players, else by the server matchmaking last sent them to (in memory).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use diesel::prelude::*;
use protocol::status::{InstanceKind, PlayerWhere, RegistrationMode, ServerStatus, StatusInstance, StatusPlayer};
use serde_json::{Value, json};
use service_kit::collate::locale_compare;
use service_kit::env::Env;

use crate::app::{AppError, AppState};
use crate::schema::users;
use crate::time::now_ms;

const DEFAULT_SERVER_NAME: &str = "Dauntless Revived";
const DEFAULT_SOURCE_URL: &str = "https://github.com/mixutin/dauntless-revived";
const CACHE_MS: u128 = 5000;
const DEPLOYSERVER_TIMEOUT_MS: u64 = 2000;
const KINDS: [&str; 4] = ["city", "hunt", "dojo", "tutorial"];

// ---- Identity ----

/// Printable ASCII, trimmed and cut to `max`; anything else is unset (`CleanText`).
fn clean_text(value: Option<&str>, max: usize) -> Option<String> {
    let text: String = value?.chars().filter(|c| (' '..='~').contains(c)).collect();
    let text: String = text.trim().chars().take(max).collect();
    (!text.is_empty()).then_some(text)
}

fn clean_commit(value: Option<&str>) -> Option<String> {
    clean_text(value, 64).filter(|t| t.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)))
}

/// What the status says about the server itself, read once at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `SERVER_NAME`, or "Dauntless Revived".
    pub name: String,
    /// `SERVER_VERSION`, else this build's version.
    pub version: String,
    /// `GIT_COMMIT`, else "unknown".
    pub commit: String,
    /// `SOURCE_URL` (the AGPL source link).
    pub source_url: String,
    /// `CONTENT_PORT`.
    pub content_port: Option<u16>,
    /// `STATUS_EXTRA=0`: /dauntless-status answers the client's nine fields alone.
    pub status_extra: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_env(&Env::from_pairs([]))
    }
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let content_port = env
            .raw("CONTENT_PORT")
            .map(str::trim)
            .filter(|v| (1..=5).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|p| *p >= 1);
        Self {
            name: clean_text(env.raw("SERVER_NAME"), 64).unwrap_or_else(|| DEFAULT_SERVER_NAME.into()),
            version: clean_text(env.raw("SERVER_VERSION"), 32)
                .or_else(|| clean_text(Some(env!("CARGO_PKG_VERSION")), 32))
                .unwrap_or_else(|| "unknown".into()),
            commit: clean_commit(env.raw("GIT_COMMIT")).unwrap_or_else(|| "unknown".into()),
            source_url: clean_text(env.raw("SOURCE_URL"), 256).unwrap_or_else(|| DEFAULT_SOURCE_URL.into()),
            content_port,
            status_extra: env.raw("STATUS_EXTRA") != Some("0"),
        }
    }

    /// `GetServerIdentity`.
    pub fn identity(&self) -> serde_json::Map<String, Value> {
        let mut identity = serde_json::Map::new();
        identity.insert("name".into(), json!(self.name));
        identity.insert("version".into(), json!(self.version));
        identity.insert("commit".into(), json!(self.commit));
        identity.insert("sourceUrl".into(), json!(self.source_url));
        identity
    }
}

/// `StatusWelcome`: the in-game banner, one real translation per language the client has.
pub fn welcome(name: &str) -> serde_json::Map<String, Value> {
    let mut texts = serde_json::Map::new();
    for (language, text) in [
        ("en", format!("Welcome to {name}!")),
        ("fr", format!("Bienvenue sur {name} !")),
        ("it", format!("Ti diamo il benvenuto su {name}!")),
        ("es", format!("¡Te damos la bienvenida a {name}!")),
        ("de", format!("Willkommen bei {name}!")),
        ("pt", format!("Boas-vindas ao {name}!")),
        ("ru", format!("Добро пожаловать в {name}!")),
        ("ja", format!("{name}へようこそ！")),
    ] {
        texts.insert(language.into(), json!(text));
    }
    texts
}

/// When the process started, for `uptimeSeconds`.
pub fn started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

// ---- The deploy server's game servers ----

#[derive(Debug, Clone, PartialEq)]
struct Gameserver {
    connected_players: Option<i64>,
    id: String,
    port: i64,
    kind: &'static str,
    map: String,
    behemoth: Option<String>,
    hunt_id: Option<String>,
    matchmaker_hunt_id: Option<String>,
    expected_players: Vec<Value>,
    max_players: Option<i64>,
    started_at: String,
}

fn whole(value: &Value) -> Option<i64> {
    value
        .as_f64()
        .filter(|n| n.fract() == 0.0 && n.is_finite())
        .map(|n| n as i64)
}

/// `IsDeployGameserver`.
fn read_gameserver(value: &Value) -> Option<Gameserver> {
    let text_or_null = |name: &str| -> Option<Option<String>> {
        match value.get(name)? {
            Value::Null => Some(None),
            Value::String(text) => Some(Some(text.clone())),
            _ => None,
        }
    };
    let kind = KINDS
        .iter()
        .find(|k| value.get("kind").and_then(Value::as_str) == Some(**k))?;
    let max_players = match value.get("maxPlayers")? {
        Value::Null => None,
        other => Some(whole(other)?),
    };
    Some(Gameserver {
        connected_players: value
            .get("connectedPlayers")
            .and_then(whole)
            .filter(|n| *n >= 0 && *n < (1 << 53)),
        id: value.get("id")?.as_str()?.to_owned(),
        port: whole(value.get("port")?)?,
        kind,
        map: value.get("map")?.as_str()?.to_owned(),
        behemoth: text_or_null("behemoth")?,
        hunt_id: text_or_null("huntId")?,
        matchmaker_hunt_id: text_or_null("matchmakerHuntId")?,
        expected_players: value.get("expectedPlayers")?.as_array()?.clone(),
        max_players,
        started_at: value.get("startedAt")?.as_str()?.to_owned(),
    })
}

/// Whether the last fetch answered, to log only the changes (`NoteDeployServer`).
static REACHABLE: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);

fn note_deploy_server(reachable: bool, why: &str) {
    let mut last = REACHABLE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *last != Some(reachable) {
        if reachable {
            tracing::info!("ServerStatus: the deploy server's /gameservers answers");
        } else {
            tracing::warn!("ServerStatus: no game server list from the deploy server ({why}); showing none");
        }
    }
    *last = Some(reachable);
}

/// `FetchGameservers`.
async fn fetch_gameservers(app: &AppState) -> Vec<Gameserver> {
    let Some(authority) = app.matchmaking.settings.deploy_url.clone().filter(|u| !u.is_empty()) else {
        note_deploy_server(false, "DEPLOYSERVER_URL is not set");
        return Vec::new();
    };
    let reply = crate::matchmaking::deploy::request(
        &authority,
        hyper::Method::GET,
        "/gameservers",
        None,
        &[200],
        DEPLOYSERVER_TIMEOUT_MS,
    )
    .await;
    match reply {
        Err(error) => {
            note_deploy_server(false, &error);
            Vec::new()
        }
        Ok(reply) if reply.status != 200 => {
            note_deploy_server(false, &format!("status {}", reply.status));
            Vec::new()
        }
        Ok(reply) => match reply
            .body
            .as_ref()
            .and_then(|b| b.get("servers"))
            .and_then(Value::as_array)
        {
            None => {
                note_deploy_server(false, "unexpected reply");
                Vec::new()
            }
            Some(servers) => {
                note_deploy_server(true, "");
                servers.iter().filter_map(read_gameserver).collect()
            }
        },
    }
}

// ---- Names ----

fn titles(table: &str, key: Option<&str>) -> Option<String> {
    crate::vendor::hunt_titles()
        .get(table)?
        .get(key?)?
        .as_str()
        .map(str::to_owned)
}

/// "/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C" -> "lerawr_beta_bp".
fn blueprint_of(asset: Option<&str>) -> Option<String> {
    let asset = asset.filter(|a| !a.is_empty())?;
    let name = asset
        .split('?')
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    (!name.is_empty()).then_some(name)
}

/// "/Game/Maps/islands/1702/cora_jamima?game=..." -> "cora_jamima".
pub fn map_codename(map: &str) -> String {
    map.split('?')
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('.')
        .next()
        .unwrap_or("")
        .to_owned()
}

/// Display name where the hunt tables know it, else the codename.
fn behemoth_name(server: &Gameserver) -> Option<String> {
    let blueprint = blueprint_of(server.behemoth.as_deref());
    titles("matchmakerHunts", server.matchmaker_hunt_id.as_deref())
        .or_else(|| titles("behemoths", blueprint.as_deref()))
        .or_else(|| blueprint.map(|b| b.strip_suffix("_bp").map(str::to_owned).unwrap_or(b)))
}

fn instance_title(server: &Gameserver, behemoth: Option<&str>) -> String {
    match server.kind {
        "city" => "Ramsgate".into(),
        "dojo" => "Training Dojo".into(),
        "tutorial" => "Tutorial".into(),
        _ => format!(
            "Hunt: {}",
            behemoth
                .map(str::to_owned)
                .or_else(|| titles("playerHunts", server.hunt_id.as_deref()))
                .unwrap_or_else(|| map_codename(&server.map))
        ),
    }
}

fn default_max_players(kind: &str) -> i64 {
    match kind {
        "city" => 32,
        "dojo" => 12,
        "tutorial" => 1,
        _ => 4,
    }
}

// ---- Where a player is ----

/// `/\/dia_moss_triforce(?:$|[.?])|(?:^|\/)tutorial/i` on lowercased text.
fn is_tutorial_map(place: &str) -> bool {
    let triforce = place.match_indices("/dia_moss_triforce").any(|(at, found)| {
        let rest = &place[at + found.len()..];
        rest.is_empty() || rest.starts_with('.') || rest.starts_with('?')
    });
    triforce || place.starts_with("tutorial") || place.contains("/tutorial")
}

/// `ClassifyPlace`: from the heartbeat's map (a package path or a bare map name) and its state.
pub fn classify_place(map: Option<&Value>, state: Option<&str>) -> &'static str {
    let place = map
        .and_then(Value::as_str)
        .map(|m| m.trim().to_lowercase())
        .unwrap_or_default();
    if !place.is_empty() {
        if ["login", "frontend", "mainmenu"].iter().any(|w| place.contains(w)) {
            return "menu";
        }
        if place.contains("dojo") {
            return "dojo";
        }
        if is_tutorial_map(&place) {
            return "tutorial";
        }
        if place.contains("/islands/") || place.contains("arena_") {
            return "hunt";
        }
        if place.contains("ramsgate") {
            return "city";
        }
        if ["transition", "entry", "loading"].iter().any(|w| place.contains(w)) {
            return "unknown";
        }
    }
    match state.map(str::to_lowercase).as_deref() {
        Some("menu" | "lobby") => return "menu",
        Some("city") => return "city",
        Some("island") => return "hunt",
        _ => {}
    }
    // Every other map in the game is a hunt island
    if place.is_empty() { "unknown" } else { "hunt" }
}

// ---- The status ----

/// `SelectPersistentInstance`: a known destination wins when several cities share a map.
fn select_persistent<'a>(worlds: &[&'a Gameserver], port: Option<i64>) -> Option<&'a Gameserver> {
    port.and_then(|p| worlds.iter().find(|w| w.port == p).copied())
        .or_else(|| worlds.first().copied())
}

fn registration(app: &AppState) -> RegistrationMode {
    let mode = app
        .registration_mode
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    match mode.as_deref() {
        Some("OPEN") => RegistrationMode::Open,
        Some("INVITECODE") => RegistrationMode::Invitecode,
        _ => RegistrationMode::None,
    }
}

/// A count for the status: what the deploy server reported, never below 0.
fn count(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn place_of(place: &str) -> PlayerWhere {
    match place {
        "menu" => PlayerWhere::Menu,
        "city" => PlayerWhere::City,
        "hunt" => PlayerWhere::Hunt,
        "dojo" => PlayerWhere::Dojo,
        "tutorial" => PlayerWhere::Tutorial,
        _ => PlayerWhere::Unknown,
    }
}

fn kind_of(kind: &str) -> InstanceKind {
    match kind {
        "city" => InstanceKind::City,
        "dojo" => InstanceKind::Dojo,
        "tutorial" => InstanceKind::Tutorial,
        _ => InstanceKind::Hunt,
    }
}

/// The status as the launcher reads it: one type for both (`protocol::status`, X4).
fn status_with(app: &AppState, players: Vec<StatusPlayer>, instances: Vec<StatusInstance>, limited: bool) -> Value {
    let identity = &app.server_status;
    let status = ServerStatus {
        name: identity.name.clone(),
        online: true,
        version: identity.version.clone(),
        commit: identity.commit.clone(),
        source_url: Some(identity.source_url.clone()),
        registration: Some(registration(app)),
        players_online: u32::try_from(players.len()).unwrap_or(u32::MAX),
        players,
        instances,
        content_port: identity.content_port,
        uptime_seconds: u32::try_from(started().elapsed().as_secs()).unwrap_or(u32::MAX),
        limited,
    };
    serde_json::to_value(status).unwrap_or(Value::Null)
}

/// `BuildServerStatus`: the full variant.
async fn build_full(app: &AppState) -> Result<Value, AppError> {
    let mut servers = fetch_gameservers(app).await;
    let kind_index = |k: &str| KINDS.iter().position(|x| *x == k).unwrap_or(KINDS.len());
    servers.sort_by(|a, b| {
        kind_index(a.kind)
            .cmp(&kind_index(b.kind))
            .then_with(|| locale_compare(&a.started_at, &b.started_at))
            .then(a.port.cmp(&b.port))
    });
    // The newest server wins when a port was reused
    let mut newest: Vec<&Gameserver> = servers.iter().collect();
    newest.sort_by(|a, b| locale_compare(&b.started_at, &a.started_at));
    let online = app.activity.lock().online(now_ms());
    let ids: Vec<String> = online.iter().map(|(id, _)| id.clone()).collect();
    let names: std::collections::HashMap<String, String> = app
        .store
        .read(move |conn| {
            users::table
                .filter(users::userId.eq_any(&ids))
                .select((users::userId, users::name))
                .load::<(String, String)>(conn)
                .map_err(AppError::from)
        })
        .await?
        .into_iter()
        .collect();
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut players: Vec<(String, &'static str, Option<String>)> = Vec::new();
    for (user, beat) in &online {
        let Some(name) = names.get(user) else {
            continue; // an account that no longer exists
        };
        let mut place = classify_place(beat.map.as_ref(), beat.state.as_deref());
        let last = app.parties.lock().mm.last_result(user);
        let ready_port = last.filter(|(ready, _, _)| *ready).map(|(_, _, port)| port);
        let mut instance = None;
        if place == "city" || place == "dojo" {
            let worlds: Vec<&Gameserver> = newest.iter().copied().filter(|s| s.kind == place).collect();
            instance = select_persistent(&worlds, ready_port);
        } else if place == "hunt" || place == "tutorial" {
            let session: Vec<&Gameserver> = newest
                .iter()
                .copied()
                .filter(|s| s.kind == "hunt" || s.kind == "tutorial")
                .collect();
            instance = session
                .iter()
                .copied()
                .find(|s| s.expected_players.iter().any(|p| p.as_str() == Some(user)))
                .or_else(|| ready_port.and_then(|port| session.iter().copied().find(|s| s.port == port)));
            if let Some(found) = instance {
                place = found.kind;
            }
        }
        if let Some(found) = instance {
            *counts.entry(found.id.clone()).or_default() += 1;
        }
        players.push((name.clone(), place, instance.map(|s| s.id.clone())));
    }
    players.sort_by(|a, b| {
        locale_compare(&a.0.to_lowercase(), &b.0.to_lowercase()).then_with(|| locale_compare(&a.0, &b.0))
    });
    let instances = servers
        .iter()
        .map(|server| {
            let behemoth = if server.kind == "city" || server.kind == "dojo" {
                None
            } else {
                behemoth_name(server)
            };
            StatusInstance {
                id: server.id.clone(),
                kind: kind_of(server.kind),
                title: instance_title(server, behemoth.as_deref()),
                map: map_codename(&server.map),
                behemoth,
                players: count(
                    server
                        .connected_players
                        .unwrap_or_else(|| counts.get(&server.id).copied().unwrap_or(0)),
                ),
                max_players: count(server.max_players.unwrap_or_else(|| default_max_players(server.kind))),
                started_at: Some(server.started_at.clone()),
            }
        })
        .collect();
    let players = players
        .into_iter()
        .map(|(name, place, instance)| StatusPlayer {
            name,
            location: place_of(place),
            instance,
        })
        .collect();
    Ok(status_with(app, players, instances, false))
}

/// The status cache: each variant answers from its own 5 s cache, and callers asking at once share
/// one build. Nothing about the caller goes in, only the variant.
#[derive(Debug, Default)]
pub struct Cache {
    full: tokio::sync::Mutex<Option<(Instant, Value)>>,
    limited: tokio::sync::Mutex<Option<(Instant, Value)>>,
}

/// `GetServerStatus`.
pub async fn status(app: &AppState, full: bool) -> Result<Value, AppError> {
    let slot = if full {
        &app.status_cache.full
    } else {
        &app.status_cache.limited
    };
    let mut cached = slot.lock().await;
    if let Some((at, status)) = cached.as_ref()
        && at.elapsed() < Duration::from_millis(CACHE_MS as u64)
    {
        return Ok(status.clone());
    }
    let status = if full {
        build_full(app).await?
    } else {
        status_with(app, Vec::new(), Vec::new(), true)
    };
    *cached = Some((Instant::now(), status.clone()));
    Ok(status)
}

/// `ClearServerStatusCache`.
pub async fn clear_cache(app: &AppState) {
    *app.status_cache.full.lock().await = None;
    *app.status_cache.limited.lock().await = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_classified_like_node() {
        let place = |map: &str, state: Option<&str>| classify_place(Some(&json!(map)), state);
        assert_eq!(place("/Game/Maps/Frontend/login", None), "menu");
        assert_eq!(place("/Game/Maps/dojo_map", None), "dojo");
        assert_eq!(place("/Game/Maps/islands/1705/dia_moss_triforce", None), "tutorial");
        assert_eq!(place("/Game/Maps/islands/1705/dia_moss_triforce_b", None), "hunt");
        assert_eq!(place("tutorial_island", None), "tutorial");
        assert_eq!(place("/Game/Maps/islands/1702/cora_jamima", None), "hunt");
        assert_eq!(place("/Game/Maps/Ramsgate/ramsgate_p", None), "city");
        assert_eq!(place("/Game/Maps/Transition", None), "unknown");
        assert_eq!(place("", Some("Lobby")), "menu");
        assert_eq!(place("", Some("island")), "hunt");
        assert_eq!(place("somewhere", Some("city")), "city");
        assert_eq!(place("somewhere", None), "hunt");
        assert_eq!(classify_place(None, None), "unknown");
        assert_eq!(classify_place(Some(&json!(5)), Some("city")), "city");
    }

    #[test]
    fn names_are_read_like_node() {
        assert_eq!(
            map_codename("/Game/Maps/islands/1702/cora_jamima?game=x"),
            "cora_jamima"
        );
        assert_eq!(
            blueprint_of(Some("/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C")),
            Some("lerawr_beta_bp".into())
        );
        assert_eq!(clean_text(Some("  a\u{7}b  "), 64), Some("ab".into()));
        assert_eq!(clean_commit(Some("abc def")), None);
    }

    // serverstatus.test.ts's helper-level cases (M31)

    #[test]
    fn reads_the_heartbeat_s_map_as_a_package_path_or_a_bare_name() {
        let cases: [(Value, Option<&str>, &str); 16] = [
            (json!("/Game/Maps/Map_LoginMenu"), None, "menu"),
            (json!("Map_LoginMenu"), None, "menu"),
            (json!("/Game/Maps/ramsgate/ramsgate_01_persistent"), None, "city"),
            (json!("ramsgate_01_persistent"), None, "city"),
            (json!("/Game/Maps/islands/dojo/training_dojo_persistent"), None, "dojo"),
            (json!("/Game/Maps/islands/1705/dia_moss_triforce"), None, "tutorial"),
            (json!("/Game/Maps/islands/1705/dia_moss_triforce_2"), None, "hunt"),
            (json!("/Game/Maps/islands/1702/cora_jamima"), None, "hunt"),
            (json!("/Game/Maps/islands/arenas/arena_ramsgate_00"), None, "hunt"),
            (json!("cora_jamima"), None, "hunt"),
            (json!(""), Some("city"), "city"),
            (Value::Null, Some("menu"), "menu"),
            (Value::Null, Some("lobby"), "menu"),
            (Value::Null, Some("island"), "hunt"),
            (Value::Null, None, "unknown"),
            (json!({}), None, "unknown"),
        ];
        for (map, state, expected) in cases {
            let map = (!map.is_null()).then_some(map);
            assert_eq!(classify_place(map.as_ref(), state), expected, "{map:?} / {state:?}");
        }
    }

    fn server(
        kind: &'static str,
        map: &str,
        behemoth: Option<&str>,
        hunt_id: Option<&str>,
        matchmaker: Option<&str>,
        port: i64,
    ) -> Gameserver {
        Gameserver {
            connected_players: None,
            id: format!("server-{port}"),
            port,
            kind,
            map: map.into(),
            behemoth: behemoth.map(str::to_owned),
            hunt_id: hunt_id.map(str::to_owned),
            matchmaker_hunt_id: matchmaker.map(str::to_owned),
            expected_players: Vec::new(),
            max_players: None,
            started_at: String::new(),
        }
    }

    #[test]
    fn names_behemoths_from_the_hunt_tables_else_by_codename() {
        let name = |behemoth: Option<&str>, matchmaker: Option<&str>| {
            behemoth_name(&server("hunt", "", behemoth, None, matchmaker, 1))
        };
        assert_eq!(
            name(
                Some("/Game/Monsters/lerawr/lerawr_beta_bp.lerawr_beta_bp_C"),
                Some("CR19_MatchmakerHunt_Lerawr_Beta")
            )
            .as_deref(),
            Some("Lesser Embermane")
        );
        assert_eq!(
            name(
                Some("/Game/Monsters/mcrollin/mcbeaver_alpha_bp.mcbeaver_alpha_bp_C"),
                Some("CR19_MatchmakerHunt_Beaver_Heroic")
            )
            .as_deref(),
            Some("Heroic Ragetail Gnasher")
        );
        assert_eq!(
            name(
                Some("/Game/Monsters/mcrollin/mcbeaver_alpha_bp.mcbeaver_alpha_bp_C"),
                None
            )
            .as_deref(),
            Some("Ragetail Gnasher")
        );
        assert_eq!(
            name(
                Some("/Game/Monsters/sq/electroquill/electroquill_alpha_bp.electroquill_alpha_bp_C"),
                Some("Arena_MatchmakerHunt_Hard_001")
            )
            .as_deref(),
            Some("Shockjaw Nayzaga")
        );
        assert_eq!(
            name(
                Some("/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C"),
                None
            )
            .as_deref(),
            Some("Gnasher")
        );
        assert_eq!(
            name(Some("/Game/Monsters/fulgar/fulgar_umbral_bp.fulgar_umbral_bp_C"), None).as_deref(),
            Some("fulgar_umbral")
        );
        assert_eq!(name(None, None), None);
        assert_eq!(name(None, Some("toString")), None);
    }

    #[test]
    fn titles_ramsgate_the_dojo_the_tutorial_and_hunts() {
        let title = |kind: &'static str, map: &str, hunt: Option<&str>, behemoth: Option<&str>| {
            instance_title(&server(kind, map, None, hunt, None, 1), behemoth)
        };
        assert_eq!(
            title("city", "/Game/Maps/ramsgate/ramsgate_01_persistent", None, None),
            "Ramsgate"
        );
        assert_eq!(
            title("dojo", "/Game/Maps/islands/dojo/training_dojo_persistent", None, None),
            "Training Dojo"
        );
        assert_eq!(
            title(
                "tutorial",
                "/Game/Maps/islands/1705/dia_moss_triforce",
                None,
                Some("Gnasher")
            ),
            "Tutorial"
        );
        assert_eq!(
            title(
                "hunt",
                "/Game/Maps/islands/1702/cora_jamima",
                None,
                Some("Lesser Embermane")
            ),
            "Hunt: Lesser Embermane"
        );
        assert_eq!(
            title(
                "hunt",
                "/Game/Maps/islands/1702/cora_jamima",
                Some("CR19_PlayerHunt_Escalation_Randall_Easy"),
                None
            ),
            "Hunt: Umbral Escalation"
        );
        assert_eq!(
            title("hunt", "/Game/Maps/islands/1702/cora_jamima?game=X", None, None),
            "Hunt: cora_jamima"
        );
    }

    /// Node's default version is its package.json's 0.0.5; Rust's is this build's version.
    #[test]
    fn has_defaults_and_takes_server_name_server_version_git_commit_and_source_url() {
        let defaults = Settings::from_env(&Env::from_pairs([]));
        assert_eq!(defaults.name, "Dauntless Revived");
        assert_eq!(defaults.source_url, "https://github.com/mixutin/dauntless-revived");
        assert_eq!(defaults.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(defaults.commit, "unknown");
        let set = Settings::from_env(&Env::from_pairs([
            ("SERVER_NAME", "Friday Hunts"),
            ("SERVER_VERSION", "friends-v1"),
            ("GIT_COMMIT", "0123456789abcdef0123456789abcdef01234567-dirty"),
            ("SOURCE_URL", "https://example.org/src"),
        ]));
        assert_eq!(
            Value::Object(set.identity()),
            json!({"name": "Friday Hunts", "version": "friends-v1", "commit": "0123456789abcdef0123456789abcdef01234567-dirty", "sourceUrl": "https://example.org/src"})
        );
        let odd = Settings::from_env(&Env::from_pairs([("GIT_COMMIT", "not a commit; rm -rf")]));
        assert_ne!(odd.commit, "not a commit; rm -rf");
    }

    #[test]
    fn attributes_multiple_ramsgates_by_the_player_s_allocated_port() {
        let (main, worker) = (
            server("city", "", None, None, None, 8777),
            server("city", "", None, None, None, 8761),
        );
        let worlds = [&main, &worker];
        assert_eq!(select_persistent(&worlds, Some(8761)).map(|s| s.port), Some(8761));
        assert_eq!(select_persistent(&worlds, Some(8777)).map(|s| s.port), Some(8777));
        assert_eq!(select_persistent(&worlds, None).map(|s| s.port), Some(8777));
        assert!(select_persistent(&[], Some(8761)).is_none());
    }
}
