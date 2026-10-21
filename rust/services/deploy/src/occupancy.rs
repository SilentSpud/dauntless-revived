//! How many players a game server has (`nativeoccupancy.ts`), from what the game DLL writes into
//! the ready folder: a hunt's count-only diagnostic log, and a pooled city's snapshot file.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

fn millis(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The last complete `net driver=IpNetDriver... connections=N owned=N open=N` line's connections.
pub fn parse_native_occupancy(text: &str) -> Option<u64> {
    let mut last = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(rest) = line.strip_prefix("net driver=IpNetDriver") else {
            continue;
        };
        if rest.contains('\r') {
            continue;
        }
        let fields: Vec<&str> = rest.rsplitn(4, ' ').collect();
        // [open=, owned=, connections=, prefix]
        if fields.len() < 4 {
            continue;
        }
        let number = |field: &str, name: &str| {
            field
                .strip_prefix(name)
                .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|n| n.parse::<u64>().ok())
        };
        if let (Some(connections), Some(_), Some(_)) = (
            number(fields[2], "connections="),
            number(fields[1], "owned="),
            number(fields[0], "open="),
        ) {
            last = Some(connections);
        }
    }
    last
}

/// A hunt's connections from the bounded tail of its own diagnostic log, when that log is fresh.
pub fn native_occupancy(ready_dir: Option<&str>, pid: u32, started_at: i64, now: i64) -> Option<u64> {
    let dir = ready_dir?;
    if pid < 1 {
        return None;
    }
    let mut file = std::fs::File::open(Path::new(dir).join(format!("native-fault-{pid}.log"))).ok()?;
    let meta = file.metadata().ok()?;
    let modified = millis(meta.modified().ok()?);
    if modified < started_at || now - modified > 90_000 {
        return None;
    }
    let size = meta.len();
    let length = size.min(8192);
    file.seek(SeekFrom::Start(size - length)).ok()?;
    let mut buffer = vec![0u8; length as usize];
    let read = file.read(&mut buffer).ok()?;
    parse_native_occupancy(&String::from_utf8_lossy(&buffer[..read]))
}

/// A pooled city's count from its atomic snapshot (`<pid> <count>`), when it is fresh: a stale count
/// never proves free seats.
pub fn native_city_occupancy(ready_dir: Option<&str>, pid: u32, started_at: i64, now: i64) -> Option<u64> {
    let dir = ready_dir?;
    if pid < 1 {
        return None;
    }
    let mut file = std::fs::File::open(Path::new(dir).join(format!("city-occupancy-{pid}.txt"))).ok()?;
    let meta = file.metadata().ok()?;
    let modified = millis(meta.modified().ok()?);
    if meta.len() > 64 || modified < started_at || now - modified > 2500 {
        return None;
    }
    let mut buffer = [0u8; 64];
    let read = file.read(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer[..read]);
    let text = text.trim_end_matches(|c: char| service_kit::js::is_space(c));
    let (owner, count) = text.split_once(' ')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(owner) || !digits(count) || owner.parse::<u64>().ok()? != u64::from(pid) {
        return None;
    }
    count.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_complete_driver_line() {
        assert_eq!(
            parse_native_occupancy("net driver=IpNetDriver_1 connections=35 owned=32 open=32\n"),
            Some(35)
        );
        assert_eq!(
            parse_native_occupancy(
                "net driver=IpNetDriver_1 connections=35 owned=32 open=32\nnet driver=IpNetDriver_1 connections=12 owned=11 open=11\npartial"
            ),
            Some(12)
        );
        assert_eq!(parse_native_occupancy("first_chance code=c0000005\n"), None);
        assert_eq!(
            parse_native_occupancy("net driver=IpNetDriver_1 connections=7 owned=7 open=7\r\n"),
            Some(7)
        );
    }

    #[test]
    fn city_snapshots_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let now = millis(SystemTime::now());
        std::fs::write(dir.path().join("city-occupancy-42.txt"), "42 7\n").unwrap();
        assert_eq!(native_city_occupancy(Some(path), 42, 0, now), Some(7));
        assert_eq!(native_city_occupancy(Some(path), 42, 0, now + 10_000), None, "stale");
        std::fs::write(dir.path().join("city-occupancy-43.txt"), "42 7\n").unwrap();
        assert_eq!(
            native_city_occupancy(Some(path), 43, 0, now),
            None,
            "another process's count"
        );
        assert_eq!(native_city_occupancy(None, 42, 0, now), None);
        std::fs::write(
            dir.path().join("native-fault-42.log"),
            "net driver=IpNetDriver_0 connections=3 owned=3 open=3\n",
        )
        .unwrap();
        assert_eq!(native_occupancy(Some(path), 42, 0, now), Some(3));
    }
}
