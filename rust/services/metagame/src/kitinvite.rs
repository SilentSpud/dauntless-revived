//! `metagame invite [--config <dir>] [--name <server name>]` (`deploy/unix-common/new-invite.mjs`,
//! MIGRATION_PLAN.md S6): adds a one-use invite code to this server's database and prints the
//! launcher invite for it, from the kit's `server.conf` and `metagame.env`. A public server's invite
//! carries the fingerprint of its gateway certificate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use protocol::invite::{Invite, ServerMode, format_invite};
use sha2::{Digest, Sha256};

use crate::dbtools::{self, DbCommand};

/// `KEY=value` lines; surrounding double quotes dropped.
fn parse_env(file: &Path) -> Result<HashMap<String, String>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut out = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.strip_prefix('"').unwrap_or(value);
            let value = value.strip_suffix('"').unwrap_or(value);
            out.insert(key.to_owned(), value.to_owned());
        }
    }
    Ok(out)
}

/// The server name in the invite: 1 to 64 printable ASCII characters, none of `&=#`.
pub fn clean_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    let ok = (1..=64).contains(&name.len()) && name.bytes().all(|b| (0x20..=0x7e).contains(&b) && !b"&=#".contains(&b));
    if ok {
        Ok(name.to_owned())
    } else {
        Err("--name must be 1-64 printable characters without &=#".into())
    }
}

/// SHA-256 of a PEM certificate's DER, lower-case hex: an invite's `fp`.
pub fn certificate_fingerprint(pem: &str) -> Result<String, String> {
    let body: String = pem
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END CERTIFICATE-----"))
        .map(str::trim)
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| "the gateway certificate is not PEM".to_owned())?;
    if der.is_empty() {
        return Err("the gateway certificate is not PEM".into());
    }
    Ok(Sha256::digest(&der).iter().map(|b| format!("{b:02x}")).collect())
}

pub fn run(args: &[String]) -> Result<String, String> {
    let mut options = HashMap::new();
    let mut items = args.iter();
    while let Some(key) = items.next() {
        let name = key
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument {key}"))?;
        options.insert(name.to_owned(), items.next().cloned().unwrap_or_default());
    }
    let config = PathBuf::from(options.get("config").map_or("/etc/dauntless-revived", String::as_str));
    let config = std::path::absolute(&config).unwrap_or(config);
    let conf = parse_env(&config.join("server.conf"))?;
    let meta = parse_env(&config.join("metagame.env"))?;
    let get = |key: &str| conf.get(key).filter(|v| !v.is_empty()).cloned();
    let (Some(_root), Some(data), Some(mode), Some(host)) =
        (get("DR_ROOT"), get("DR_DATA"), get("DR_MODE"), get("DR_MY_IP"))
    else {
        return Err("server.conf is incomplete".into());
    };
    let name = clean_name(options.get("name").map_or("Slayer", String::as_str))?;
    let mut random = [0u8; 10];
    rand::fill(&mut random[..]);
    let code: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let db = meta.get("DB_FILENAME").ok_or("metagame.env has no DB_FILENAME")?;
    dbtools::run(&DbCommand::AddInvite {
        db: PathBuf::from(db),
        code: code.clone(),
    })?;
    let invite = if mode == "public" {
        let pem = std::fs::read_to_string(Path::new(&data).join("tls").join("gateway-cert.pem"))
            .map_err(|e| e.to_string())?;
        let port = get("DR_GATEWAY_EXTERNAL_PORT").unwrap_or_else(|| "443".into());
        Invite {
            mode: ServerMode::Public,
            host,
            port: port
                .parse()
                .map_err(|_| "DR_GATEWAY_EXTERNAL_PORT is not a port".to_owned())?,
            code,
            name,
            share: None,
            fp: Some(certificate_fingerprint(&pem)?),
        }
    } else {
        Invite {
            mode: ServerMode::Private,
            host,
            port: 61000,
            code,
            name,
            share: None,
            fp: None,
        }
    };
    format_invite(&invite).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_fingerprints() {
        assert_eq!(clean_name(" Slayer ").unwrap(), "Slayer");
        for bad in ["", "a&b", "x=y", "#1", "naïve", &"x".repeat(65)] {
            assert!(clean_name(bad).is_err(), "{bad}");
        }
        let der = b"not really a certificate";
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            base64::engine::general_purpose::STANDARD.encode(der)
        );
        let expected: String = Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(certificate_fingerprint(&pem).unwrap(), expected);
        assert!(certificate_fingerprint("nothing").is_err());
    }
}
