//! The launcher invite the bot hands out (`invite.mjs`), built from the public server's config
//! (`SERVER_CONFIG_FILE`) with the shared builder of the `protocol` crate.

use protocol::invite::{Invite, ServerMode, format_invite};
use serde_json::Value;

/// Reads and checks the server config before the bot accepts claims.
pub fn load_invite_config(file: Option<&str>) -> Result<Value, String> {
    let file = file.ok_or("Configure SERVER_CONFIG_FILE for launcher invites")?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let config: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
    launcher_invite(&config, "VALIDATION")?;
    Ok(config)
}

fn is_code(code: &str) -> bool {
    (4..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn launcher_invite(config: &Value, code: &str) -> Result<String, String> {
    if !is_code(code) {
        return Err("Registration code is not launcher compatible".into());
    }
    let invalid = || "Invalid public server configuration".to_owned();
    let host = config["PublicHost"].as_str().unwrap_or("");
    let host_ok = (1..=253).contains(&host.len())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port = config["Ports"]["gateway"]
        .as_f64()
        .filter(|p| p.fract() == 0.0 && (1.0..=65535.0).contains(p));
    let fp = config["CertFingerprint"].as_str().unwrap_or("");
    let fp_ok = fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit());
    let (true, true, Some(port), true) = (config["Mode"] == "Public", host_ok, port, fp_ok) else {
        return Err(invalid());
    };
    let name = config["ServerName"]
        .as_str()
        .filter(|n| !n.is_empty())
        .unwrap_or("Dauntless Revived");
    format_invite(&Invite {
        mode: ServerMode::Public,
        host: host.to_ascii_lowercase(),
        port: port as u16,
        code: code.to_owned(),
        name: name.to_owned(),
        share: None,
        fp: Some(fp.to_ascii_lowercase()),
    })
    .map_err(|_| invalid())
}

pub fn invite_message(config: &Value, code: &str) -> Result<String, String> {
    Ok(format!(
        "🔑 **Your Revived Invite**\n```\n{}\n```\nCopy the entire invite above into the launcher's **Join** box, then choose your username on **Register**. The registration code is already included. The launcher saves your account key after registration.\nAlready registered? Keep your original launcher key; use /key link to link it to Discord.\n**Clear skies, Slayer.**",
        launcher_invite(config, code)?
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config() -> Value {
        json!({ "Mode": "Public", "PublicHost": "Game.Example.com", "Ports": { "gateway": 443 }, "CertFingerprint": "A".repeat(64), "ServerName": "Test & Hunt" })
    }

    fn query(invite: &str) -> std::collections::HashMap<String, String> {
        let url = url::Url::parse(invite).unwrap();
        assert_eq!((url.scheme(), url.host_str()), ("dauntless-revived", Some("join")));
        url.query_pairs().into_owned().collect()
    }

    #[test]
    fn complete_launcher_invites() {
        let invite = launcher_invite(&config(), "DR-0123abcd").unwrap();
        let q = query(&invite);
        assert_eq!(q["v"], "2");
        assert_eq!(q["mode"], "public");
        assert_eq!(q["host"], "game.example.com");
        assert_eq!(q["port"], "443");
        assert_eq!(q["code"], "DR-0123abcd");
        assert_eq!(q["fp"], "a".repeat(64));
        assert_eq!(q["name"], "Test & Hunt");
        assert!(invite_message(&config(), "DR-0123abcd").unwrap().contains(&invite));
        assert!(launcher_invite(&config(), "DR-invalid_code").is_err());
        let mut wrong = config();
        wrong["CertFingerprint"] = json!("wrong");
        assert!(launcher_invite(&wrong, "GOOD-CODE").is_err());
        let mut private = config();
        private["Mode"] = json!("Private");
        assert!(launcher_invite(&private, "GOOD-CODE").is_err());
    }
}
