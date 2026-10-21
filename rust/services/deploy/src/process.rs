//! How game processes are started and checked. Tests swap in stand-ins (the Node server's
//! `UseProcessFunctionsForTests`); nothing else changes them.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::watch;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// How a process ended: its exit code, or the signal that ended it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// Hide the console window (Windows)
    pub hide: bool,
    /// The whole environment of the new process
    pub env: Vec<(String, String)>,
}

/// A started game process.
pub trait GameProcess: Send + Sync {
    /// `None`: the start failed, and [`GameProcess::error`] says why (Node's "error" event)
    fn pid(&self) -> Option<u32>;
    fn error(&self) -> Option<String>;
    /// Becomes `Some` when the process has ended
    fn exit(&self) -> watch::Receiver<Option<Exit>>;
    /// Asks it to end (`force`: at once); false when it already ended
    fn kill(&self, force: bool) -> bool;
}

pub trait Platform: Send + Sync {
    /// `Err` is a start that failed on the spot (Node's spawn throwing)
    fn spawn(&self, command: &str, args: &[String], options: &SpawnOptions) -> Result<Arc<dyn GameProcess>, String>;
    fn is_alive(&self, pid: u32) -> bool;
    fn is_port_bound(&self, port: u16) -> BoxFuture<Result<bool, String>>;
}

struct Started {
    pid: Option<u32>,
    error: Option<String>,
    exit: watch::Receiver<Option<Exit>>,
}

impl GameProcess for Started {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn exit(&self) -> watch::Receiver<Option<Exit>> {
        self.exit.clone()
    }

    fn kill(&self, force: bool) -> bool {
        match self.pid {
            Some(pid) if self.exit.borrow().is_none() => crate::system::kill_process(pid, force),
            _ => false,
        }
    }
}

/// The real thing: processes, `process.kill(pid, 0)` and a UDP bind.
pub struct RealPlatform;

impl Platform for RealPlatform {
    fn spawn(&self, command: &str, args: &[String], options: &SpawnOptions) -> Result<Arc<dyn GameProcess>, String> {
        let mut process = tokio::process::Command::new(command);
        process
            .args(args)
            .env_clear()
            .envs(options.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(std::process::Stdio::null());
        #[cfg(windows)]
        if options.hide {
            // CREATE_NO_WINDOW
            process.creation_flags(0x0800_0000);
        }
        let (tell, exit) = watch::channel(None);
        match process.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                tokio::spawn(async move {
                    let status = child.wait().await;
                    let ended = match status {
                        Ok(status) => Exit {
                            code: status.code(),
                            signal: signal_of(&status),
                        },
                        Err(_) => Exit {
                            code: None,
                            signal: None,
                        },
                    };
                    let _ = tell.send(Some(ended));
                });
                Ok(Arc::new(Started { pid, error: None, exit }))
            }
            // A binary that is not there: no process id and an error, as Node reports it
            Err(error) => Ok(Arc::new(Started {
                pid: None,
                error: Some(format!("spawn {command} {error}")),
                exit,
            })),
        }
    }

    fn is_alive(&self, pid: u32) -> bool {
        crate::system::is_process_alive(pid)
    }

    fn is_port_bound(&self, port: u16) -> BoxFuture<Result<bool, String>> {
        Box::pin(crate::system::is_udp_port_bound(port))
    }
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|s| match s {
        libc::SIGTERM => "SIGTERM".to_owned(),
        libc::SIGKILL => "SIGKILL".to_owned(),
        other => format!("signal {other}"),
    })
}

#[cfg(not(unix))]
fn signal_of(_status: &std::process::ExitStatus) -> Option<String> {
    None
}
