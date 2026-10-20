//! Hunt regions (`controllers/huntregion.ts`, `regionpreferences.ts`, `regionprobes.ts`): which
//! regional server set a player's hunts start on, saved per account.

use diesel::prelude::*;
use serde_json::{Value, json};
use service_kit::env::Env;

use crate::schema::huntregions;

pub type Conn = store::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    Main,
    Aus,
    Ger,
    Us,
}

impl Region {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Aus => "aus",
            Self::Ger => "ger",
            Self::Us => "us",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "main" => Some(Self::Main),
            "aus" => Some(Self::Aus),
            "ger" => Some(Self::Ger),
            "us" => Some(Self::Us),
            _ => None,
        }
    }
}

/// `GetHuntRegion`: a saved AUS choice outlives the AUS host and now means US.
pub fn player_region(conn: &mut Conn, user: &str) -> QueryResult<Region> {
    let saved: Option<String> = huntregions::table
        .find(user)
        .select(huntregions::region)
        .first(conn)
        .optional()?;
    Ok(match saved.as_deref() {
        Some("aus") | Some("us") => Region::Us,
        Some("ger") => Region::Ger,
        _ => Region::Main,
    })
}

/// `SaveHuntRegion`.
pub fn save_region(conn: &mut Conn, user: &str, region: Region) -> QueryResult<()> {
    diesel::insert_into(huntregions::table)
        .values((huntregions::userId.eq(user), huntregions::region.eq(region.as_str())))
        .on_conflict(huntregions::userId)
        .do_update()
        .set(huntregions::region.eq(region.as_str()))
        .execute(conn)
        .map(drop)
}

/// `RegionQueueKey`: main-region queues keep the bare hunt id.
pub fn queue_key(hunt: &str, region: Region) -> String {
    if region == Region::Main {
        hunt.to_owned()
    } else {
        format!("{hunt}:{}", region.as_str())
    }
}

/// The regional switches and probe endpoints (`GERMANY_REGION`, `US_REGION`,
/// `REGION_<MAIN|GER|US>_PROBE_HOST` and `_PORT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub germany: bool,
    pub us: bool,
    /// `RegionProbes()`, or the error it threw for an invalid endpoint.
    pub probes: Result<Vec<(Region, String, u16)>, String>,
}

impl Settings {
    pub fn from_env(env: &Env) -> Self {
        let germany = env.raw("GERMANY_REGION") == Some("1");
        let us = env.raw("US_REGION") == Some("1");
        let mut probes = Ok(Vec::new());
        for region in [Region::Main, Region::Ger, Region::Us] {
            if (region == Region::Ger && !germany) || (region == Region::Us && !us) {
                continue;
            }
            let prefix = format!("REGION_{}_PROBE", region.as_str().to_ascii_uppercase());
            let Some(host) = env.raw(&format!("{prefix}_HOST")).filter(|h| !h.is_empty()) else {
                continue;
            };
            let port = match env.raw(&format!("{prefix}_PORT")) {
                None => Some(443.0),
                Some(text) => Some(crate::js::number(Some(&json!(text)))),
            };
            let valid_host = host.len() <= 253
                && host.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
                && host.bytes().all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b));
            match port {
                Some(port) if valid_host && port.fract() == 0.0 && (1.0..=65535.0).contains(&port) => {
                    if let Ok(list) = &mut probes {
                        list.push((region, host.to_owned(), port as u16));
                    }
                }
                _ => {
                    probes = Err(format!("Invalid {prefix} endpoint"));
                    break;
                }
            }
        }
        Self { germany, us, probes }
    }

    pub fn probes_json(&self) -> Result<Value, String> {
        self.probes.clone().map(|list| {
            Value::Array(
                list.into_iter()
                    .map(|(region, host, port)| json!({"region": region.as_str(), "host": host, "port": port}))
                    .collect(),
            )
        })
    }
}
