//! What the deploy server asks the operating system: free memory (`os.freemem`), CPU times
//! (`os.cpus`), whether a process still runs (`process.kill(pid, 0)`), ending one, and whether a
//! UDP port is taken.

#![allow(unsafe_code)] // OS calls

/// Free physical memory in bytes, as Node's `os.freemem()` (NaN when unknown).
pub fn free_memory() -> f64 {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
            return status.ullAvailPhys as f64;
        }
        f64::NAN
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|line| line.starts_with("MemAvailable:"))
                    .and_then(|line| line.split_whitespace().nth(1))
                    .and_then(|kb| kb.parse::<f64>().ok())
            })
            .map_or(f64::NAN, |kb| kb * 1024.0)
    }
}

/// The machine's cumulative idle and total CPU time (any unit), summed over every CPU.
pub fn cpu_times() -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetSystemTimes;
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
        let value = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)) as f64;
        // Kernel time includes idle time
        Some((value(idle), value(kernel) + value(user)))
    }
    #[cfg(not(windows))]
    {
        let text = std::fs::read_to_string("/proc/stat").ok()?;
        let line = text.lines().find(|l| l.starts_with("cpu "))?;
        let fields: Vec<f64> = line.split_whitespace().skip(1).filter_map(|f| f.parse().ok()).collect();
        let idle = *fields.get(3)?;
        Some((idle, fields.iter().take(7).sum()))
    }
}

/// `process.kill(pid, 0)` succeeding.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
            let _ = CloseHandle(handle);
            alive
        }
    }
    #[cfg(unix)]
    {
        i32::try_from(pid).is_ok_and(|pid| unsafe { libc::kill(pid, 0) } == 0)
    }
}

/// Ends a process: SIGTERM, or SIGKILL when `force` (Windows has only the one way).
pub fn kill_process(pid: u32, force: bool) -> bool {
    #[cfg(windows)]
    {
        let _ = force;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
                return false;
            };
            let done = TerminateProcess(handle, 1).is_ok();
            let _ = CloseHandle(handle);
            done
        }
    }
    #[cfg(unix)]
    {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        i32::try_from(pid).is_ok_and(|pid| unsafe { libc::kill(pid, signal) } == 0)
    }
}

/// Whether something holds UDP `port` on all addresses (a bind that fails with "in use" or "access
/// denied"); other bind errors are errors.
pub async fn is_udp_port_bound(port: u16) -> Result<bool, String> {
    match tokio::net::UdpSocket::bind(("0.0.0.0", port)).await {
        Ok(socket) => {
            drop(socket);
            Ok(false)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok(true)
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_runs_and_the_machine_has_memory_and_cpu_times() {
        assert!(is_process_alive(std::process::id()));
        assert!(!is_process_alive(u32::MAX - 1));
        assert!(free_memory() > 0.0);
        let (idle, total) = cpu_times().unwrap();
        assert!(total >= idle && total > 0.0);
    }

    #[tokio::test]
    async fn a_bound_udp_port_is_seen() {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        assert_eq!(is_udp_port_bound(port).await, Ok(true));
        drop(socket);
        assert_eq!(is_udp_port_bound(port).await, Ok(false));
    }
}
