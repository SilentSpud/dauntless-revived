//! `deploy generate-config …` (`deploy/unix-common/generate-config.mjs`): the Unix kits' settings
//! for every service, written once at install and again on upgrade. Secrets that exist are kept
//! (the metagame's signing key, the game-server key, the gateway and allowlist secrets); missing ones
//! are made. Every file is private (0600) and replaced atomically.
//!
//! For the Rust services the game launcher is `deploy` itself under another name: the kit links
//! `<bin-dir>/dr-run-game` (Linux) or `<bin-dir>/dr-ssh-game` (OpenBSD) to the `deploy` binary,
//! which then runs as `deploy run-game` or `deploy ssh-game` (see `main.rs`).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

type Values = Vec<(&'static str, String)>;

fn parse_args(argv: &[String]) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    let mut items = argv.iter();
    while let Some(key) = items.next() {
        let Some(name) = key.strip_prefix("--") else {
            return Err(format!("unexpected argument {key}"));
        };
        let value = items.next().ok_or_else(|| format!("missing value for {key}"))?;
        out.insert(name.to_owned(), value.clone());
    }
    Ok(out)
}

/// A `.env` file: `KEY=value`, a value in double quotes read as JSON.
pub fn read_env(file: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(file) else {
        return out;
    };
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mut value = value.to_owned();
        if value.len() >= 2
            && value.starts_with('"')
            && value.ends_with('"')
            && let Ok(serde_json::Value::String(parsed)) = serde_json::from_str(&value)
        {
            value = parsed;
        }
        out.insert(key.to_owned(), value);
    }
    out
}

fn env_line(key: &str, value: &str) -> Result<String, String> {
    if value.contains('\n') || value.contains('\r') {
        return Err(format!("env value contains a newline: {key}"));
    }
    let safe = value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_./:@,+-".contains(&b));
    Ok(if safe {
        format!("{key}={value}")
    } else {
        format!("{key}={}", serde_json::Value::String(value.to_owned()))
    })
}

fn write_private(file: &Path, text: &str) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut handle = options.open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    handle.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

fn write_env(file: &Path, values: &Values) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let lines: Vec<String> = values.iter().map(|(k, v)| env_line(k, v)).collect::<Result<_, _>>()?;
    let mut temp = file.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    write_private(&temp, &(lines.join("\n") + "\n"))?;
    std::fs::rename(&temp, file).map_err(|e| e.to_string())
}

/// A secret file of `bytes` random bytes in hex, kept when it already holds one.
fn secret_file(file: &Path, bytes: usize) -> Result<String, String> {
    if let Ok(text) = std::fs::read_to_string(file) {
        let value = text.trim();
        if value.len() >= bytes * 2 {
            return Ok(value.to_owned());
        }
    }
    let mut random = vec![0u8; bytes];
    rand::fill(&mut random[..]);
    let value: String = random.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    write_private(file, &format!("{value}\n"))?;
    Ok(value)
}

/// The metagame's JWT signing pair, kept when the old settings have one: RSA-2048, PKCS#8 and SPKI
/// PEM, each base64 as the metagame reads them.
fn signing(existing: &HashMap<String, String>) -> Result<(String, String), String> {
    use base64::Engine;
    if let (Some(private), Some(public)) = (
        existing.get("AUTH_SIGNING_PRIVKEY_B64"),
        existing.get("AUTH_SIGNING_PUBKEY_B64"),
    ) && !private.is_empty()
        && !public.is_empty()
    {
        return Ok((private.clone(), public.clone()));
    }
    let key = rcgen::KeyPair::generate_rsa_for(&rcgen::PKCS_RSA_SHA256, rcgen::RsaKeySize::_2048)
        .map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok((b64.encode(key.serialize_pem()), b64.encode(key.public_key_pem())))
}

/// A fresh metagame signing pair (base64 of PKCS#8 and SPKI PEM), as the launcher's offline state keeps it.
pub fn signing_pair() -> Result<(String, String), String> {
    signing(&HashMap::new())
}

/// `deploy signing-keys <out file>` (the Windows kit's `dr-keys.js signing`): a fresh signing pair as the
/// two `.env` lines the metagame reads, in a private file. Nothing is printed but a note; the caller
/// merges the lines into the protected `metagame.env` and deletes the file.
pub fn signing_keys_file(out: &Path) -> Result<(), String> {
    let (private, public) = signing(&HashMap::new())?;
    write_private(
        out,
        &format!(
            "AUTH_SIGNING_PRIVKEY_B64={private}
AUTH_SIGNING_PUBKEY_B64={public}
"
        ),
    )
}

fn port(text: Option<&String>, default: u32) -> Option<u32> {
    match text {
        None => Some(default),
        Some(t) => t
            .parse::<u32>()
            .ok()
            .filter(|_| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())),
    }
}

fn s(value: impl ToString) -> String {
    value.to_string()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Runs the generator; the line it prints.
/// The `COMMIT` file a server release archive carries (MIGRATION_PLAN.md M38), for an install that is not a
/// git checkout: one full commit hash.
fn release_commit(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("COMMIT")).ok()?;
    let commit = text.trim();
    (commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit())).then(|| commit.to_ascii_lowercase())
}

pub fn generate(argv: &[String]) -> Result<String, String> {
    let args = parse_args(argv)?;
    let required = |name: &str| {
        args.get(name)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| format!("--{name} is required"))
    };
    let absolute = |p: String| std::path::absolute(&p).unwrap_or_else(|_| PathBuf::from(p));
    let platform = required("platform")?;
    if platform != "linux" && platform != "openbsd" {
        return Err("--platform must be linux or openbsd".into());
    }
    let mode = args.get("mode").cloned().unwrap_or_else(|| "private".into());
    if mode != "private" && mode != "public" {
        return Err("--mode must be private or public".into());
    }
    let public = mode == "public";
    let root = absolute(required("root")?);
    let data = absolute(required("data")?);
    let config = absolute(required("config")?);
    let game = absolute(required("game-dir")?);
    let my_ip = required("my-ip")?;
    let bin = args
        .get("bin-dir")
        .map(|b| absolute(b.clone()))
        .unwrap_or_else(|| root.join("bin"));
    let server_name = args
        .get("server-name")
        .cloned()
        .unwrap_or_else(|| "Dauntless Revived".into());
    let gateway_port = port(args.get("gateway-port"), 443)
        .filter(|p| (1..=65535).contains(p))
        .ok_or("bad gateway port")?;
    let udp_begin = port(args.get("udp-begin"), 8770);
    let udp_end = port(args.get("udp-end"), 8777);
    let (Some(udp_begin), Some(udp_end)) = (udp_begin, udp_end) else {
        return Err("bad UDP range".into());
    };
    if udp_begin < 1 || udp_end > 65535 || udp_begin + 2 > udp_end {
        return Err("bad UDP range".into());
    }

    for dir in [
        data.clone(),
        config.clone(),
        data.join("logs"),
        data.join("branding"),
        data.join("tls"),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if !data.join("news.json").exists() {
        std::fs::write(data.join("news.json"), "{ \"items\": [] }\n").map_err(|e| e.to_string())?;
    }
    let commit = std::process::Command::new("git")
        .args(["-C", &path_text(&root), "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .or_else(|| release_commit(&root))
        .unwrap_or_else(|| "unknown".into());

    let meta_file = config.join("metagame.env");
    let (private_key, public_key) = signing(&read_env(&meta_file))?;
    let gameserver_key = secret_file(&data.join("gameserver.key"), 24)?;
    let gateway_secret = secret_file(&data.join("gateway.secret"), 32)?;
    let allowlist_secret = secret_file(&data.join("allowlist.secret"), 32)?;

    let bind = if public { "127.0.0.1".to_owned() } else { my_ip.clone() };
    let mut metagame: Values = vec![
        ("PORT", s(61000)),
        ("BIND_HOST", bind.clone()),
        ("NODE_ENV", "production".into()),
        ("AUTH_MODE", "APIKEY".into()),
        ("AUTH_SIGNING_PRIVKEY_B64", private_key),
        ("AUTH_SIGNING_PUBKEY_B64", public_key),
        ("REGISTRATION_MODE", "INVITECODE".into()),
        ("DB_FILENAME", path_text(&data.join("undaunted.db"))),
        ("MATCHMAKING_MODE", "DEPLOYSERVER".into()),
        ("DEPLOYSERVER_URL", "127.0.0.1:61001".into()),
        (
            "QOS_TARGET_URL",
            if public {
                "http://127.0.0.1:61000/QoS".into()
            } else {
                format!("http://{my_ip}:61000/QoS")
            },
        ),
        ("TARGET_CHANGELIST", s(239_827)),
        ("SERVER_NAME", server_name),
        ("SOURCE_URL", "https://github.com/mixutin/dauntless-revived".into()),
        ("GIT_COMMIT", commit),
        ("CONTENT_PORT", s(61002)),
        ("BODY_LOG_FILE", path_text(&data.join("logs").join("bodies.log"))),
        ("LOG_BODIES", s(0)),
        ("CHAT", s(1)),
        ("CHAT_PORT", s(61099)),
        ("CHAT_BIND_HOST", bind),
    ];
    if public {
        metagame.push(("GATEWAY_SECRET", gateway_secret.clone()));
    }
    write_env(&meta_file, &metagame)?;

    let old_deploy = read_env(&config.join("deployserver.env"));
    let launcher = if platform == "linux" {
        "dr-run-game"
    } else {
        "dr-ssh-game"
    };
    let mut deploy: Values = vec![
        ("PORT", s(61001)),
        ("BIND_HOST", "127.0.0.1".into()),
        ("MY_IP", my_ip.clone()),
        ("PORT_RANGE_BEGIN", s(udp_begin)),
        ("PORT_RANGE_END", s(udp_end)),
        ("GAMESERVER_BINARY_PATH", path_text(&bin.join(launcher))),
        ("METAGAME_API_KEY", gameserver_key),
        (
            "SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP",
            args.get("startup-gap").cloned().unwrap_or_else(|| "10".into()),
        ),
        (
            "RAMSGATE_POOL",
            args.get("ramsgate-pool")
                .or_else(|| old_deploy.get("RAMSGATE_POOL"))
                .cloned()
                .unwrap_or_else(|| "1".into()),
        ),
        (
            "ENABLE_DOJO",
            args.get("enable-dojo")
                .filter(|v| !v.is_empty())
                .cloned()
                .unwrap_or_else(|| "0".into()),
        ),
        ("NODE_ENV", "production".into()),
    ];
    if platform == "linux" {
        deploy.push((
            "DR_GAME_EXE",
            path_text(
                &game
                    .join("Archon")
                    .join("Binaries")
                    .join("Win64")
                    .join("Dauntless-Win64-Shipping.exe"),
            ),
        ));
        deploy.push(("DR_WINEPREFIX", path_text(&data.join("wineprefix"))));
        deploy.push((
            "DR_PROTON_COMPAT_DATA",
            args.get("proton-data")
                .map_or_else(|| path_text(&data.join("proton")), |p| path_text(&absolute(p.clone()))),
        ));
        deploy.push((
            "DR_XVFB",
            args.get("xvfb")
                .filter(|v| !v.is_empty())
                .cloned()
                .unwrap_or_else(|| "1".into()),
        ));
        if let Some(proton) = args.get("proton") {
            deploy.push(("DR_PROTON_BINARY", proton.clone()));
        }
        if let Some(wine) = args.get("wine") {
            deploy.push(("DR_WINE_BINARY", wine.clone()));
        }
    } else {
        deploy.push(("DR_WORKER_HOST", required("worker-host")?));
        deploy.push((
            "DR_WORKER_USER",
            args.get("worker-user").cloned().unwrap_or_else(|| "dauntless".into()),
        ));
        deploy.push(("DR_WORKER_KEY", path_text(&absolute(required("worker-key")?))));
        deploy.push((
            "DR_WORKER_COMMAND",
            args.get("worker-command")
                .cloned()
                .unwrap_or_else(|| "dr-game-worker".into()),
        ));
    }
    write_env(&config.join("deployserver.env"), &deploy)?;

    let local_meta = if public {
        "http://127.0.0.1:61000".to_owned()
    } else {
        format!("http://{my_ip}:61000")
    };
    write_env(
        &config.join("content.env"),
        &vec![
            ("PORT", s(61002)),
            (
                "BIND_HOST",
                if public || my_ip == "127.0.0.1" {
                    "127.0.0.1".into()
                } else {
                    format!("127.0.0.1,{my_ip}")
                },
            ),
            ("METAGAME_URL", local_meta),
            ("CONTENT_GAME_DIR", path_text(&game)),
            ("CONTENT_BRANDING_DIR", path_text(&data.join("branding"))),
            ("CONTENT_NEWS_FILE", path_text(&data.join("news.json"))),
            ("NODE_ENV", "production".into()),
        ],
    )?;

    if public {
        let internal_port = if platform == "openbsd" { 61443 } else { gateway_port };
        write_env(
            &config.join("gateway.env"),
            &vec![
                (
                    "GATEWAY_BIND",
                    if platform == "openbsd" { "127.0.0.1" } else { "0.0.0.0" }.into(),
                ),
                ("GATEWAY_PORT", s(internal_port)),
                ("GATEWAY_CERT", path_text(&data.join("tls").join("gateway-cert.pem"))),
                ("GATEWAY_KEY", path_text(&data.join("tls").join("gateway-key.pem"))),
                ("GATEWAY_SECRET", gateway_secret),
                ("GATEWAY_METAGAME_URL", "http://127.0.0.1:61000".into()),
                ("GATEWAY_CONTENT_URL", "http://127.0.0.1:61002".into()),
                ("GATEWAY_WS_URL", "http://127.0.0.1:61099".into()),
                ("ALLOWLIST_URL", "http://127.0.0.1:61005".into()),
                ("ALLOWLIST_SECRET", allowlist_secret.clone()),
                ("NODE_ENV", "production".into()),
            ],
        )?;
        let mut allowlist: Values = vec![
            ("ALLOWLIST_BIND", "127.0.0.1".into()),
            ("ALLOWLIST_PORT", s(61005)),
            ("ALLOWLIST_SECRET", allowlist_secret),
            ("ALLOWLIST_PORTS", format!("{udp_begin}-{udp_end}")),
            ("ALLOWLIST_TTL_SECONDS", s(600)),
            ("ALLOWLIST_MAX_ENTRIES", s(256)),
            ("ALLOWLIST_DRY_RUN", s(0)),
            (
                "ALLOWLIST_AUDIT_LOG",
                path_text(&data.join("logs").join("allowlist-audit.log")),
            ),
            ("ALLOWLIST_STATE_FILE", path_text(&data.join("allowlist-state.json"))),
        ];
        if platform == "linux" {
            allowlist.push(("DR_NFT_TABLE", "dauntless_revived".into()));
            if let Some(interface) = args.get("firewall-interface") {
                allowlist.push(("DR_FIREWALL_INTERFACE", interface.clone()));
            }
        } else {
            allowlist.push(("DR_PF_TABLE", "dauntless_revived_players".into()));
            allowlist.push(("DR_PF_ANCHOR", "dauntless-revived".into()));
        }
        write_env(&config.join("allowlist.env"), &allowlist)?;
    }
    let conf = [
        format!("DR_PLATFORM={platform}"),
        format!("DR_MODE={mode}"),
        format!("DR_ROOT={}", root.display()),
        format!("DR_DATA={}", data.display()),
        format!("DR_CONFIG={}", config.display()),
        format!("DR_GAME_DIR={}", game.display()),
        format!("DR_MY_IP={my_ip}"),
        format!("DR_GATEWAY_EXTERNAL_PORT={gateway_port}"),
        format!("DR_UDP_BEGIN={udp_begin}"),
        format!("DR_UDP_END={udp_end}"),
        format!("DR_BIN={}", bin.display()),
    ];
    write_private(&config.join("server.conf"), &(conf.join("\n") + "\n"))?;
    Ok(format!("configuration written to {}", config.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_keys_are_two_env_lines_of_base64_pem() {
        use base64::Engine;
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("signing.tmp");
        signing_keys_file(&out).unwrap();
        let env = read_env(&out);
        assert_eq!(env.len(), 2);
        let pem = |name: &str| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(&env[name]).unwrap()).unwrap()
        };
        // The PEM headers, built so the repository check does not take this test for a key
        let header = |kind: &str| format!("-----BEGIN {kind} KEY-----");
        assert!(pem("AUTH_SIGNING_PRIVKEY_B64").starts_with(&header("PRIVATE")));
        assert!(pem("AUTH_SIGNING_PUBKEY_B64").starts_with(&header("PUBLIC")));
    }

    fn run(platform: &str, root: &Path, extra: &[&str]) -> (PathBuf, PathBuf) {
        let (data, config, game) = (root.join("data"), root.join("config"), root.join("game"));
        std::fs::create_dir_all(&game).unwrap();
        let mut argv: Vec<String> = [
            "--platform",
            platform,
            "--root",
            root.to_str().unwrap(),
            "--data",
            data.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--game-dir",
            game.to_str().unwrap(),
            "--my-ip",
            if platform == "linux" {
                "127.0.0.1"
            } else {
                "203.0.113.10"
            },
            "--mode",
            if platform == "linux" { "private" } else { "public" },
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        argv.extend(extra.iter().map(|s| (*s).to_owned()));
        generate(&argv).unwrap();
        (data, config)
    }

    #[test]
    fn a_release_archive_names_its_commit_in_a_file() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(release_commit(root.path()), None);
        std::fs::write(
            root.path().join("COMMIT"),
            "not a commit
",
        )
        .unwrap();
        assert_eq!(release_commit(root.path()), None);
        let commit = "0123456789ABCDEF0123456789abcdef01234567";
        std::fs::write(
            root.path().join("COMMIT"),
            format!(
                "{commit}
"
            ),
        )
        .unwrap();
        assert_eq!(release_commit(root.path()), Some(commit.to_ascii_lowercase()));
    }

    #[test]
    fn linux_uses_the_wine_runner_and_no_worker_ssh() {
        let root = tempfile::tempdir().unwrap();
        let (_, config) = run("linux", root.path(), &["--xvfb", "0"]);
        let deploy = read_env(&config.join("deployserver.env"));
        let content = read_env(&config.join("content.env"));
        assert!(deploy["GAMESERVER_BINARY_PATH"].ends_with("dr-run-game"));
        assert_eq!(
            deploy["DR_GAME_EXE"],
            path_text(
                &root
                    .path()
                    .join("game")
                    .join("Archon")
                    .join("Binaries")
                    .join("Win64")
                    .join("Dauntless-Win64-Shipping.exe")
            )
        );
        assert_eq!(deploy["DR_XVFB"], "0");
        assert_eq!(
            deploy["DR_PROTON_COMPAT_DATA"],
            path_text(&root.path().join("data").join("proton"))
        );
        assert!(!deploy.contains_key("DR_WORKER_HOST"));
        assert_eq!(content["BIND_HOST"], "127.0.0.1");
        let meta = read_env(&config.join("metagame.env"));
        assert!(!meta.contains_key("GATEWAY_SECRET"));
        assert!(!config.join("gateway.env").exists());
    }

    #[test]
    fn openbsd_keeps_the_gateway_unprivileged_and_launches_over_ssh() {
        let root = tempfile::tempdir().unwrap();
        let key = root.path().join("worker.key");
        std::fs::write(&key, "test\n").unwrap();
        let (_, config) = run(
            "openbsd",
            root.path(),
            &[
                "--worker-host",
                "10.0.0.2",
                "--worker-user",
                "dauntless",
                "--worker-key",
                key.to_str().unwrap(),
                "--worker-command",
                "dr-game-worker",
            ],
        );
        let deploy = read_env(&config.join("deployserver.env"));
        let gateway = read_env(&config.join("gateway.env"));
        let allowlist = read_env(&config.join("allowlist.env"));
        assert!(deploy["GAMESERVER_BINARY_PATH"].ends_with("dr-ssh-game"));
        assert_eq!(deploy["DR_WORKER_HOST"], "10.0.0.2");
        assert_eq!(deploy["DR_WORKER_COMMAND"], "dr-game-worker");
        assert_eq!(gateway["GATEWAY_BIND"], "127.0.0.1");
        assert_eq!(gateway["GATEWAY_PORT"], "61443");
        assert_eq!(allowlist["DR_PF_ANCHOR"], "dauntless-revived");
        assert_eq!(
            read_env(&config.join("metagame.env"))["GATEWAY_SECRET"],
            gateway["GATEWAY_SECRET"]
        );
    }

    #[test]
    fn secrets_and_the_signing_key_survive_a_rerun() {
        let root = tempfile::tempdir().unwrap();
        let (data, config) = run("linux", root.path(), &[]);
        let meta = read_env(&config.join("metagame.env"));
        let key = std::fs::read_to_string(data.join("gameserver.key")).unwrap();
        assert!(meta["AUTH_SIGNING_PRIVKEY_B64"].len() > 1000);
        run("linux", root.path(), &[]);
        assert_eq!(
            read_env(&config.join("metagame.env"))["AUTH_SIGNING_PRIVKEY_B64"],
            meta["AUTH_SIGNING_PRIVKEY_B64"]
        );
        assert_eq!(std::fs::read_to_string(data.join("gameserver.key")).unwrap(), key);
        assert_eq!(key.trim().len(), 48);
    }

    #[test]
    fn values_are_quoted_when_needed_and_bad_input_refused() {
        assert_eq!(env_line("A", "plain/path:1").unwrap(), "A=plain/path:1");
        assert_eq!(env_line("A", "has space").unwrap(), "A=\"has space\"");
        assert!(env_line("A", "two\nlines").is_err());
        let root = tempfile::tempdir().unwrap();
        let bad = |extra: &[&str]| {
            let mut argv: Vec<String> = [
                "--platform",
                "linux",
                "--root",
                "r",
                "--data",
                "d",
                "--config",
                "c",
                "--game-dir",
                "g",
                "--my-ip",
                "1.2.3.4",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
            argv.extend(extra.iter().map(|s| (*s).to_owned()));
            let _ = root.path();
            generate(&argv).unwrap_err()
        };
        assert!(bad(&["--udp-begin", "8776"]).contains("UDP"));
        assert!(bad(&["--gateway-port", "70000"]).contains("gateway port"));
        assert!(bad(&["--mode", "open"]).contains("--mode"));
        assert!(
            generate(&["--platform".into(), "windows".into()])
                .unwrap_err()
                .contains("--platform")
        );
    }
}
