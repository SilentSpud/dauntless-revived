//! When a new game server counts as started (`readiness.ts`): its ready file names the port it
//! listens on, or its UDP port is seen bound twice in a row. It ending first is a failed start.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::process::GameProcess;

fn stopped(child: &dyn GameProcess) -> bool {
    child.exit().borrow().is_some()
}

/// Waits up to `timeout_ms` for the game server on `port` to listen; removes the ready file after.
pub async fn wait_for_server_ready(
    child: Arc<dyn GameProcess>,
    port: u16,
    file: &Path,
    timeout_ms: u64,
) -> Result<(), String> {
    let result = wait(&*child, port, file, timeout_ms).await;
    let _ = tokio::fs::remove_file(file).await;
    result
}

async fn wait(child: &dyn GameProcess, port: u16, file: &Path, timeout_ms: u64) -> Result<(), String> {
    let exited = || format!("Game server on port {port} exited before listening");
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    let mut bound_samples = 0;
    while tokio::time::Instant::now() < deadline {
        if stopped(child) {
            return Err(exited());
        }
        match tokio::fs::read_to_string(file).await {
            Ok(marker) => {
                let marker = service_kit::js::trim(&marker);
                let parts: Vec<&str> = marker.split(':').collect();
                let pid_ok = parts.len() == 2 && !parts[0].is_empty() && parts[0].bytes().all(|b| b.is_ascii_digit());
                if pid_ok && parts[1].parse::<f64>().ok() == Some(f64::from(port)) {
                    if stopped(child) {
                        return Err(exited());
                    }
                    return Ok(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let bound = crate::system::is_udp_port_bound(port).await?;
        if stopped(child) {
            return Err(exited());
        }
        if bound {
            bound_samples += 1;
            if bound_samples >= 2 {
                return Ok(());
            }
        } else {
            bound_samples = 0;
        }
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        tokio::time::sleep(left.clamp(Duration::from_millis(1), Duration::from_millis(200))).await;
    }
    if stopped(child) {
        return Err(exited());
    }
    Err(format!(
        "Game server on port {port} did not start listening within {timeout_ms}ms"
    ))
}
