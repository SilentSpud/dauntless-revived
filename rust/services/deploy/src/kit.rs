//! The Unix kits' game launchers (`deploy/unix-common/game-runner.mjs`,
//! `linux-server/launch-gameserver.mjs`, `linux-server/worker-launch.mjs`,
//! `openbsd-server/launch-gameserver.mjs`; MIGRATION_PLAN.md S6), as `deploy` subcommands:
//!
//! - `deploy run-game <args…>`: the game server under Proton or Wine (and Xvfb without a display),
//!   with the deploy server's arguments unchanged. The deploy server's `GAMESERVER_BINARY_PATH`.
//! - `deploy worker <payload>`: the same on a worker machine, from one encoded argument list (what
//!   the SSH forced command passes; never a shell string).
//! - `deploy ssh-game <args…>`: OpenBSD's launcher, which starts the game on the Linux worker over
//!   batch SSH with that one payload.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// The whole environment of the game process
    pub env: HashMap<String, String>,
    pub runtime: String,
}

fn executable(file: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        file.is_file()
    }
}

fn find_on_path(names: &[&str], env: &HashMap<String, String>) -> Option<PathBuf> {
    for name in names {
        if name.contains('/') || name.contains('\\') {
            let file = Path::new(name);
            if executable(file) {
                return Some(std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()));
            }
            continue;
        }
        let path = env.get("PATH").map(String::as_str).unwrap_or("");
        for dir in std::env::split_paths(path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(name);
            if executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn directories(parent: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(parent)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Compares names with their digit runs as numbers ("Proton10" after "Proton9"), case-insensitively.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n.trim_start_matches('0').to_owned()
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let order = n.len().cmp(&m.len()).then_with(|| n.cmp(&m));
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Proton builds under the usual Steam folders, newest first.
fn proton_candidates(home: &Path) -> Vec<PathBuf> {
    let roots = [
        home.join(".local").join("share").join("Steam"),
        home.join(".steam").join("root"),
        home.join(".steam").join("steam"),
    ];
    let mut out = Vec::new();
    for root in roots {
        for dir in directories(&root.join("compatibilitytools.d")) {
            out.push(dir.join("proton"));
        }
        for dir in directories(&root.join("steamapps").join("common")) {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name.starts_with("proton") || name.starts_with("ge-proton") {
                out.push(dir.join("proton"));
            }
        }
    }
    out.retain(|p| executable(p));
    out.sort_by(|a, b| natural(&b.to_string_lossy(), &a.to_string_lossy()));
    out
}

fn steam_root_for(proton: &Path) -> Option<PathBuf> {
    let mut current = proton.parent()?.to_path_buf();
    for _ in 0..8 {
        let name = current
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name == "compatibilitytools.d" {
            return current.parent().map(Path::to_path_buf);
        }
        if name == "common"
            && current
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "steamapps")
        {
            return current.parent()?.parent().map(Path::to_path_buf);
        }
        current = current.parent()?.to_path_buf();
    }
    None
}

/// The game's own `dxgi.dll` (the server DLL) first, whatever overrides the host set.
fn dll_overrides(existing: Option<&String>) -> String {
    let mut parts: Vec<String> = existing
        .map(String::as_str)
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.to_ascii_lowercase().starts_with("dxgi="))
        .map(str::to_owned)
        .collect();
    parts.push("dxgi=n,b".into());
    parts.join(";")
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `buildWineLaunch`: what to run for the game server `args` with this environment.
pub fn build_wine_launch(args: &[String], env: &HashMap<String, String>) -> Result<Launch, String> {
    let game = env
        .get("DR_GAME_EXE")
        .map(PathBuf::from)
        .filter(|g| g.is_absolute() && g.exists())
        .ok_or("DR_GAME_EXE must be an existing absolute path to Dauntless-Win64-Shipping.exe")?;
    let home = env
        .get("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"));
    let explicit = |name: &str| {
        env.get(name)
            .filter(|v| !v.is_empty())
            .and_then(|v| find_on_path(&[v], env))
    };
    let explicit_proton = explicit("DR_PROTON_BINARY");
    let explicit_wine = explicit("DR_WINE_BINARY");
    let proton = explicit_proton.or_else(|| {
        if explicit_wine.is_some() {
            None
        } else {
            proton_candidates(&home).into_iter().next()
        }
    });
    let cwd = game.parent().map(Path::to_path_buf);
    let mut launch_env = env.clone();
    let (command, mut game_args, runtime) = if let Some(proton) = proton {
        let compat = env.get("DR_PROTON_COMPAT_DATA").map(PathBuf::from).unwrap_or_else(|| {
            home.join(".local")
                .join("share")
                .join("dauntless-revived")
                .join("proton")
        });
        let compat = std::path::absolute(&compat).unwrap_or(compat);
        std::fs::create_dir_all(&compat).map_err(|e| e.to_string())?;
        launch_env.insert("STEAM_COMPAT_DATA_PATH".into(), compat.to_string_lossy().into_owned());
        if let Some(root) = steam_root_for(&proton) {
            launch_env.insert(
                "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
                root.to_string_lossy().into_owned(),
            );
        }
        launch_env.insert("STEAM_COMPAT_APP_ID".into(), "0".into());
        launch_env.insert("PROTON_LOG".into(), "0".into());
        launch_env.insert("WINEDLLOVERRIDES".into(), dll_overrides(env.get("WINEDLLOVERRIDES")));
        let runtime = format!("Proton ({})", proton.parent().map(base_name).unwrap_or_default());
        (
            proton,
            vec!["run".to_owned(), game.to_string_lossy().into_owned()],
            runtime,
        )
    } else {
        let wine = explicit_wine
            .or_else(|| find_on_path(&["wine64", "wine"], env))
            .ok_or("Proton or Wine was not found; set DR_PROTON_BINARY/DR_WINE_BINARY or install a runtime")?;
        let prefix = env.get("DR_WINEPREFIX").map(PathBuf::from).unwrap_or_else(|| {
            home.join(".local")
                .join("share")
                .join("dauntless-revived")
                .join("wineprefix")
        });
        let prefix = std::path::absolute(&prefix).unwrap_or(prefix);
        std::fs::create_dir_all(&prefix).map_err(|e| e.to_string())?;
        launch_env.insert("WINEPREFIX".into(), prefix.to_string_lossy().into_owned());
        launch_env.insert(
            "WINEDEBUG".into(),
            env.get("WINEDEBUG").cloned().unwrap_or_else(|| "-all".into()),
        );
        launch_env.insert("WINEDLLOVERRIDES".into(), dll_overrides(env.get("WINEDLLOVERRIDES")));
        (wine, vec![game.to_string_lossy().into_owned()], "Wine".to_owned())
    };
    game_args.extend(args.iter().cloned());
    let no_display = launch_env.get("DISPLAY").is_none_or(|d| d.trim().is_empty());
    if env.get("DR_XVFB").map(String::as_str) != Some("0") && no_display {
        let xvfb = match env.get("DR_XVFB_RUN") {
            Some(name) => find_on_path(&[name], env),
            None => find_on_path(&["xvfb-run"], env),
        }
        .ok_or("no DISPLAY and xvfb-run was not found; install Xvfb/xvfb-run or set DR_XVFB=0")?;
        let mut wrapped = vec!["-a".to_owned(), "-s".to_owned(), "-screen 0 1280x720x24".to_owned()];
        wrapped.push(command.to_string_lossy().into_owned());
        wrapped.extend(game_args);
        return Ok(Launch {
            runtime: format!("{runtime} via {}", base_name(&xvfb)),
            command: xvfb,
            args: wrapped,
            cwd,
            env: launch_env,
        });
    }
    Ok(Launch {
        command,
        args: game_args,
        cwd,
        env: launch_env,
        runtime,
    })
}

/// `decodeWorkerPayload`: base64url JSON of 8 to 64 strings, no NULs, nothing longer than 4096.
pub fn decode_worker_payload(value: &str) -> Result<Vec<String>, String> {
    let shaped = (2..=65536).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !shaped {
        return Err("invalid worker payload".into());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value.trim_end_matches('='))
        .map_err(|_| "invalid worker payload".to_owned())?;
    let decoded: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "invalid worker payload".to_owned())?;
    let list = decoded
        .as_array()
        .filter(|l| (8..=64).contains(&l.len()))
        .ok_or("invalid worker arguments")?;
    list.iter()
        .map(|v| {
            v.as_str()
                .filter(|s| s.encode_utf16().count() <= 4096 && !s.contains('\0'))
                .map(str::to_owned)
                .ok_or_else(|| "invalid worker arguments".to_owned())
        })
        .collect()
}

pub fn encode_worker_payload(args: &[String]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(args).unwrap_or_default())
}

fn required<'a>(
    env: &'a HashMap<String, String>,
    name: &str,
    ok: impl Fn(u8) -> bool,
    max: usize,
) -> Result<&'a str, String> {
    env.get(name)
        .map(String::as_str)
        .filter(|v| (1..=max).contains(&v.len()) && v.bytes().all(&ok))
        .ok_or_else(|| format!("{name} is missing or invalid"))
}

/// `buildSshLaunch`: the game on the worker over batch SSH, its arguments as one payload.
pub fn build_ssh_launch(args: &[String], env: &HashMap<String, String>) -> Result<(Launch, String), String> {
    if args.len() < 8
        || !args
            .iter()
            .all(|a| a.encode_utf16().count() <= 4096 && !a.contains('\0'))
    {
        return Err("invalid game-server arguments".into());
    }
    let ssh_name = env.get("DR_WORKER_SSH").map(String::as_str).unwrap_or("ssh");
    let ssh = find_on_path(&[ssh_name], env).ok_or("ssh was not found")?;
    let host = required(
        env,
        "DR_WORKER_HOST",
        |b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b),
        255,
    )?;
    let user = required(
        env,
        "DR_WORKER_USER",
        |b| b.is_ascii_alphanumeric() || b"_.-".contains(&b),
        64,
    )?;
    let command = env
        .get("DR_WORKER_COMMAND")
        .filter(|c| !c.is_empty())
        .map(String::as_str)
        .unwrap_or("dr-game-worker");
    if !(1..=255).contains(&command.len())
        || !command
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
    {
        return Err("DR_WORKER_COMMAND is invalid".into());
    }
    let payload = encode_worker_payload(args);
    let mut ssh_args: Vec<String> = [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
    ]
    .map(str::to_owned)
    .to_vec();
    if let Some(key) = env.get("DR_WORKER_KEY").filter(|k| !k.is_empty()) {
        if !Path::new(key).is_absolute() && !key.starts_with('/') {
            return Err("DR_WORKER_KEY must be an absolute path".into());
        }
        ssh_args.push("-i".into());
        ssh_args.push(key.clone());
    }
    ssh_args.push(format!("{user}@{host}"));
    ssh_args.push(command.to_owned());
    ssh_args.push(payload.clone());
    Ok((
        Launch {
            command: ssh,
            args: ssh_args,
            cwd: None,
            env: env.clone(),
            runtime: "ssh".into(),
        },
        payload,
    ))
}

/// The worker's settings file (`/etc/dauntless-revived-worker.env`): `KEY=value` lines, a value in
/// double quotes as JSON; a variable already set wins.
pub fn load_worker_env(file: &Path, env: &mut HashMap<String, String>) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    for raw in text.lines() {
        let line = raw.trim();
        let Some((key, value)) = line.split_once('=').filter(|_| !line.starts_with('#')) else {
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
        env.entry(key.to_owned()).or_insert(value);
    }
}

/// How a process group (or, failing that, the one child) is ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Group,
    Child,
    None,
}

/// `terminateProcessTree`: the runtime is detached into its own group, so Wine's helper processes
/// end with it.
pub fn terminate_process_tree(
    pid: Option<u32>,
    signal: i32,
    kill_group: impl Fn(i32, i32) -> bool,
    kill_child: impl Fn(i32) -> bool,
) -> Ended {
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()).filter(|p| *p > 0)
        && cfg!(unix)
        && kill_group(-pid, signal)
    {
        return Ended::Group;
    }
    if kill_child(signal) { Ended::Child } else { Ended::None }
}

/// Runs a launch to its end, forwarding stop signals to its process group; the exit code.
pub async fn run(launch: Launch, label: &str) -> i32 {
    let mut command = tokio::process::Command::new(&launch.command);
    command.args(&launch.args).env_clear().envs(&launch.env);
    if let Some(cwd) = &launch.cwd {
        command.current_dir(cwd);
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("{label}: {error}");
            return 127;
        }
    };
    let pid = child.id();
    let status = wait_forwarding(&mut child, pid).await;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        terminate_unix(pid, libc::SIGTERM);
        if let Some(signal) = status.as_ref().ok().and_then(|s| s.signal()) {
            eprintln!("{label}: runtime exited on signal {signal}");
        }
    }
    status.ok().and_then(|s| s.code()).unwrap_or(1)
}

#[cfg(unix)]
#[allow(unsafe_code)] // kill(2)
fn terminate_unix(pid: Option<u32>, signal: i32) {
    terminate_process_tree(
        pid,
        signal,
        |group, signal| unsafe { libc::kill(group, signal) } == 0,
        |signal| {
            pid.and_then(|p| i32::try_from(p).ok())
                .is_some_and(|p| unsafe { libc::kill(p, signal) } == 0)
        },
    );
}

#[cfg(unix)]
async fn wait_forwarding(
    child: &mut tokio::process::Child,
    pid: Option<u32>,
) -> std::io::Result<std::process::ExitStatus> {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
        signal(SignalKind::hangup()),
    ) else {
        return child.wait().await;
    };
    loop {
        tokio::select! {
            status = child.wait() => return status,
            _ = term.recv() => terminate_unix(pid, libc::SIGTERM),
            _ = int.recv() => terminate_unix(pid, libc::SIGINT),
            _ = hup.recv() => terminate_unix(pid, libc::SIGHUP),
        }
    }
}

#[cfg(not(unix))]
async fn wait_forwarding(
    child: &mut tokio::process::Child,
    _pid: Option<u32>,
) -> std::io::Result<std::process::ExitStatus> {
    child.wait().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe(file: &Path) -> PathBuf {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        file.to_path_buf()
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn wine_keeps_the_deploy_arguments_and_forces_native_dxgi() {
        let root = tempfile::tempdir().unwrap();
        let game = exe(&root.path().join("game").join("Dauntless-Win64-Shipping.exe"));
        let wine = exe(&root.path().join("bin").join("wine64"));
        let prefix = root.path().join("prefix");
        let spec = build_wine_launch(
            &strings(&["secret", "8777", "/Game/Maps/ramsgate/ramsgate_01_persistent"]),
            &env(&[
                ("PATH", wine.parent().unwrap().to_str().unwrap()),
                ("DISPLAY", ":99"),
                ("DR_GAME_EXE", game.to_str().unwrap()),
                ("DR_WINE_BINARY", wine.to_str().unwrap()),
                ("DR_WINEPREFIX", prefix.to_str().unwrap()),
                ("WINEDLLOVERRIDES", "foo=n;dxgi=b"),
            ]),
        )
        .unwrap();
        assert_eq!(spec.command, std::path::absolute(&wine).unwrap());
        assert_eq!(
            spec.args,
            [
                game.to_str().unwrap(),
                "secret",
                "8777",
                "/Game/Maps/ramsgate/ramsgate_01_persistent"
            ]
        );
        assert_eq!(spec.env["WINEDLLOVERRIDES"], "foo=n;dxgi=n,b");
        assert_eq!(spec.env["WINEDEBUG"], "-all");
        assert!(prefix.is_dir());
    }

    #[test]
    fn explicit_proton_comes_first_with_its_own_compat_data() {
        let root = tempfile::tempdir().unwrap();
        let game = exe(&root.path().join("game").join("Dauntless-Win64-Shipping.exe"));
        let proton = exe(&root
            .path()
            .join("Steam")
            .join("compatibilitytools.d")
            .join("GE-Proton10-1")
            .join("proton"));
        let compat = root.path().join("compat");
        let spec = build_wine_launch(
            &strings(&["x", "1", "m", "b", "h", "p", "a", "-server"]),
            &env(&[
                ("PATH", ""),
                ("DISPLAY", ":99"),
                ("HOME", root.path().to_str().unwrap()),
                ("DR_GAME_EXE", game.to_str().unwrap()),
                ("DR_PROTON_BINARY", proton.to_str().unwrap()),
                ("DR_PROTON_COMPAT_DATA", compat.to_str().unwrap()),
                ("DR_WINE_BINARY", "/does/not/exist"),
            ]),
        )
        .unwrap();
        assert_eq!(spec.command, std::path::absolute(&proton).unwrap());
        assert_eq!(
            spec.args[..2],
            [strings(&["run"]), vec![game.to_string_lossy().into_owned()]].concat()
        );
        assert_eq!(spec.env["STEAM_COMPAT_DATA_PATH"], compat.to_str().unwrap());
        assert_eq!(
            Path::new(&spec.env["STEAM_COMPAT_CLIENT_INSTALL_PATH"]),
            std::path::absolute(root.path().join("Steam")).unwrap()
        );
        assert!(spec.env["WINEDLLOVERRIDES"].contains("dxgi=n,b"));
        assert_eq!(spec.runtime, "Proton (GE-Proton10-1)");
    }

    #[test]
    fn xvfb_without_a_display() {
        let root = tempfile::tempdir().unwrap();
        let game = exe(&root.path().join("game").join("Dauntless-Win64-Shipping.exe"));
        let wine = exe(&root.path().join("bin").join("wine"));
        let xvfb = exe(&root.path().join("bin").join("xvfb-run"));
        let prefix = root.path().join("prefix");
        let base = [
            ("PATH", wine.parent().unwrap().to_str().unwrap()),
            ("DR_GAME_EXE", game.to_str().unwrap()),
            ("DR_WINE_BINARY", wine.to_str().unwrap()),
            ("DR_WINEPREFIX", prefix.to_str().unwrap()),
        ];
        let spec = build_wine_launch(&strings(&["x"; 8]), &env(&base)).unwrap();
        assert_eq!(spec.command, xvfb);
        assert!(
            spec.args
                .contains(&std::path::absolute(&wine).unwrap().to_string_lossy().into_owned())
        );
        assert!(spec.args.contains(&game.to_string_lossy().into_owned()));
        let mut off = env(&base);
        off.insert("DR_XVFB".into(), "0".into());
        assert_eq!(
            build_wine_launch(&strings(&["x"; 8]), &off).unwrap().command,
            std::path::absolute(&wine).unwrap()
        );
        assert!(build_wine_launch(&strings(&["x"; 8]), &env(&[("DR_GAME_EXE", "relative.exe")])).is_err());
    }

    #[test]
    fn the_process_group_ends_first_then_the_one_child() {
        use std::cell::RefCell;
        let sent = RefCell::new(Vec::new());
        let ended = terminate_process_tree(
            Some(43210),
            15,
            |pid, signal| {
                sent.borrow_mut().push((pid, signal));
                true
            },
            |signal| {
                sent.borrow_mut().push((0, signal));
                true
            },
        );
        if cfg!(unix) {
            assert_eq!((ended, sent.borrow().clone()), (Ended::Group, vec![(-43210, 15)]));
        } else {
            // No process groups: the one child
            assert_eq!((ended, sent.borrow().clone()), (Ended::Child, vec![(0, 15)]));
        }
        let fallback = RefCell::new(Vec::new());
        let ended = terminate_process_tree(
            Some(43211),
            2,
            |_, _| false,
            |signal| {
                fallback.borrow_mut().push(signal);
                true
            },
        );
        assert_eq!(ended, Ended::Child);
        assert_eq!(*fallback.borrow(), [2]);
        assert_eq!(terminate_process_tree(None, 2, |_, _| false, |_| false), Ended::None);
    }

    #[test]
    fn worker_payloads_round_trip_and_bad_ones_are_refused() {
        let args = strings(&[
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "8777",
            "/Game/Map",
            "NO_BEHEMOTH",
            "NO_MM_HUNTID",
            "NO_EXPECTED_PLAYERS",
            "203.0.113.2:8777",
            "-server",
        ]);
        assert_eq!(decode_worker_payload(&encode_worker_payload(&args)).unwrap(), args);
        for bad in [
            "",
            "a",
            "not base64!",
            "W10",
            &encode_worker_payload(&strings(&["a"; 7])),
        ] {
            assert!(decode_worker_payload(bad).is_err(), "{bad}");
        }
        let with_nul =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(&["a\0"; 8]).unwrap());
        assert!(decode_worker_payload(&with_nul).is_err());
    }

    #[test]
    fn openbsd_sends_one_payload_over_batch_ssh() {
        let root = tempfile::tempdir().unwrap();
        let ssh = exe(&root.path().join("bin").join("ssh"));
        let args = strings(&[
            "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk",
            "8777",
            "/Game/Map",
            "NO_BEHEMOTH",
            "NO_MM_HUNTID",
            "NO_EXPECTED_PLAYERS",
            "198.51.100.5:8777",
            "-server",
        ]);
        let (spec, payload) = build_ssh_launch(
            &args,
            &env(&[
                ("PATH", ssh.parent().unwrap().to_str().unwrap()),
                ("DR_WORKER_HOST", "10.0.0.2"),
                ("DR_WORKER_USER", "dauntless"),
                ("DR_WORKER_COMMAND", "dr-game-worker"),
            ]),
        )
        .unwrap();
        assert_eq!(spec.command, ssh);
        assert!(spec.args.contains(&"dauntless@10.0.0.2".to_owned()));
        assert_eq!(decode_worker_payload(&payload).unwrap(), args);
        assert!(!spec.args.contains(&args[0]), "the key never appears as an argument");
        let bad_host = env(&[
            ("PATH", ssh.parent().unwrap().to_str().unwrap()),
            ("DR_WORKER_HOST", "host;rm"),
            ("DR_WORKER_USER", "u"),
        ]);
        assert!(build_ssh_launch(&args, &bad_host).is_err());
    }

    #[test]
    fn worker_env_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("worker.env");
        std::fs::write(
            &file,
            "# comment\nDR_GAME_EXE=/opt/game.exe\nQUOTED=\"a b\"\nSET=file\n",
        )
        .unwrap();
        let mut vars = env(&[("SET", "process")]);
        load_worker_env(&file, &mut vars);
        assert_eq!(vars["DR_GAME_EXE"], "/opt/game.exe");
        assert_eq!(vars["QUOTED"], "a b");
        assert_eq!(vars["SET"], "process");
    }
}
