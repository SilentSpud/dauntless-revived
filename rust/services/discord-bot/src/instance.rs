//! One bot per state file (`instance.mjs`): a lock the operating system releases when the process
//! dies, so there are no stale lock files and no PID reuse. A named pipe on Windows, a loopback
//! port on other systems, both named after the state file.

use std::path::Path;

use sha2::{Digest, Sha256};

pub struct Instance {
    #[cfg(windows)]
    _pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(not(windows))]
    _listener: std::net::TcpListener,
}

fn hash(state_file: &Path) -> String {
    let absolute = std::path::absolute(state_file).unwrap_or_else(|_| state_file.to_path_buf());
    let text = absolute.to_string_lossy().to_lowercase();
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Takes the lock for `state_file`, or fails when another bot holds it.
pub fn acquire_instance(state_file: &Path) -> Result<Instance, String> {
    let busy = || "Another key bot is running or its instance lock is unavailable".to_owned();
    let hash = hash(state_file);
    #[cfg(windows)]
    {
        let pipe = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .create(format!(r"\\.\pipe\dauntless-key-bot-{hash}"))
            .map_err(|_| busy())?;
        Ok(Instance { _pipe: pipe })
    }
    #[cfg(not(windows))]
    {
        let port = 20000 + u16::from_str_radix(&hash[..4], 16).map_err(|_| busy())? % 40000;
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| busy())?;
        Ok(Instance { _listener: listener })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_second_instance_cannot_use_the_same_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lock");
        let first = acquire_instance(&file).unwrap();
        assert!(acquire_instance(&file).err().unwrap().contains("running"));
        drop(first);
        drop(acquire_instance(&file).unwrap());
    }
}
