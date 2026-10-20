//! The backend health report (`middleware/BackendHealth.ts`, MIGRATION_PLAN.md M27), read by the
//! owner dashboard from `GET /undaunted/api/BackendHealth` when `BACKEND_HEALTH=1`.
//!
//! The same fields as Node's, with the same meaning where Rust has one:
//! - `requests`: the last 60 s in one-second buckets: completed, client and server errors, aborted
//!   (the client left before the reply), and the latency upper bound of p50 and p95 from the same
//!   histogram bounds (1 ms … 5000 ms, then `">5000"`).
//! - `eventLoop`: Node's event-loop delay is how late a 20 ms timer fires. Here a task on the async
//!   runtime does the same, so the numbers mean the same thing: how long ready work waits for a
//!   thread. Kept since start, at 0.05 ms resolution, as Node's histogram is never reset.
//! - `memoryMB.rss` is the process's resident memory; `heapUsed` is `null`: there is no garbage
//!   collected heap.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use service_kit::json::{js_number, js_number_opt};

const BOUNDS: [u64; 11] = [1, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000];

/// Monitoring polls, which must not make an idle server look busy.
fn is_monitoring_poll(path: &str) -> bool {
    let path = path.strip_suffix('/').unwrap_or(path).to_ascii_lowercase();
    [
        "backendhealth",
        "serverstatus",
        "getallusers",
        "dashboardaccounts",
        "discordkeystats",
    ]
    .iter()
    .any(|name| path == format!("/undaunted/api/{name}"))
}

#[derive(Debug, Clone)]
struct Bucket {
    second: u64,
    completed: u64,
    client_errors: u64,
    server_errors: u64,
    aborted: u64,
    histogram: [u64; BOUNDS.len() + 1],
}

/// Requests of the last 60 seconds (`RequestMetrics`).
#[derive(Debug, Default)]
pub struct RequestMetrics {
    buckets: Mutex<Vec<Bucket>>,
}

/// A latency upper bound: milliseconds, or `">5000"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum UpperBound {
    Ms(u64),
    Over(&'static str),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestsSnapshot {
    pub window_seconds: u64,
    pub completed: u64,
    pub client_errors: u64,
    pub server_errors: u64,
    pub aborted: u64,
    #[serde(serialize_with = "js_number")]
    pub requests_per_second: f64,
    #[serde(serialize_with = "js_number")]
    pub server_error_percent: f64,
    pub latency_p50_upper_ms: Option<UpperBound>,
    pub latency_p95_upper_ms: Option<UpperBound>,
}

impl RequestMetrics {
    fn prune(buckets: &mut Vec<Bucket>, second: u64) {
        buckets.retain(|bucket| bucket.second + 60 > second);
    }

    pub fn record(&self, status: u16, ms: f64, aborted: bool, now_ms: u64) {
        let second = now_ms / 1000;
        let mut buckets = self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune(&mut buckets, second);
        let index = match buckets.iter().position(|bucket| bucket.second == second) {
            Some(index) => index,
            None => {
                buckets.push(Bucket {
                    second,
                    completed: 0,
                    client_errors: 0,
                    server_errors: 0,
                    aborted: 0,
                    histogram: [0; BOUNDS.len() + 1],
                });
                buckets.len() - 1
            }
        };
        let bucket = &mut buckets[index];
        if aborted {
            bucket.aborted += 1;
            return;
        }
        bucket.completed += 1;
        if status >= 500 {
            bucket.server_errors += 1;
        } else if status >= 400 {
            bucket.client_errors += 1;
        }
        let slot = BOUNDS
            .iter()
            .position(|bound| ms <= *bound as f64)
            .unwrap_or(BOUNDS.len());
        bucket.histogram[slot] += 1;
    }

    pub fn snapshot(&self, now_ms: u64) -> RequestsSnapshot {
        let mut buckets = self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune(&mut buckets, now_ms / 1000);
        let mut histogram = [0u64; BOUNDS.len() + 1];
        let (mut completed, mut client_errors, mut server_errors, mut aborted) = (0, 0, 0, 0);
        for bucket in buckets.iter() {
            completed += bucket.completed;
            client_errors += bucket.client_errors;
            server_errors += bucket.server_errors;
            aborted += bucket.aborted;
            for (sum, value) in histogram.iter_mut().zip(bucket.histogram) {
                *sum += value;
            }
        }
        let percentile = |p: f64| {
            if completed == 0 {
                return None;
            }
            let target = (completed as f64 * p).ceil() as u64;
            let mut count = 0;
            for (index, value) in histogram.iter().enumerate() {
                count += value;
                if count >= target {
                    return Some(
                        BOUNDS
                            .get(index)
                            .map_or(UpperBound::Over(">5000"), |b| UpperBound::Ms(*b)),
                    );
                }
            }
            None
        };
        RequestsSnapshot {
            window_seconds: 60,
            completed,
            client_errors,
            server_errors,
            aborted,
            requests_per_second: completed as f64 / 60.0,
            server_error_percent: if completed > 0 {
                100.0 * server_errors as f64 / completed as f64
            } else {
                0.0
            },
            latency_p50_upper_ms: percentile(0.5),
            latency_p95_upper_ms: percentile(0.95),
        }
    }
}

/// How late a 20 ms timer fires, since start (Node's `monitorEventLoopDelay({resolution: 20})`).
#[derive(Debug)]
pub struct LoopDelay {
    /// Counts per 0.05 ms of lateness, up to 10 s; later is counted in the last slot.
    slots: Mutex<Vec<u64>>,
    max_ns: std::sync::atomic::AtomicU64,
}

const SLOT_NS: u64 = 50_000;
const SLOTS: usize = 200_000;

impl Default for LoopDelay {
    fn default() -> Self {
        Self {
            slots: Mutex::new(vec![0; SLOTS]),
            max_ns: 0.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopDelaySnapshot {
    #[serde(serialize_with = "js_number")]
    pub p50_ms: f64,
    #[serde(serialize_with = "js_number")]
    pub p95_ms: f64,
    #[serde(serialize_with = "js_number")]
    pub max_ms: f64,
}

impl LoopDelay {
    pub fn record(&self, lateness: Duration) {
        let ns = lateness.as_nanos().min(u64::MAX as u128) as u64;
        let slot = ((ns / SLOT_NS) as usize).min(SLOTS - 1);
        self.slots.lock().unwrap_or_else(|poisoned| poisoned.into_inner())[slot] += 1;
        self.max_ns.fetch_max(ns, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> Option<LoopDelaySnapshot> {
        let slots = self.slots.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let total: u64 = slots.iter().sum();
        if total == 0 {
            return None;
        }
        let percentile = |p: f64| {
            let target = ((total as f64) * p).ceil().max(1.0) as u64;
            let mut count = 0;
            let slot = slots.iter().position(|value| {
                count += value;
                count >= target
            });
            slot.map_or(0.0, |slot| (slot as u64 * SLOT_NS) as f64 / 1e6)
        };
        Some(LoopDelaySnapshot {
            p50_ms: percentile(0.5),
            p95_ms: percentile(0.95),
            max_ms: self.max_ns.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e6,
        })
    }

    /// Measures the runtime's timer lateness until the returned task is aborted.
    pub fn spawn_monitor(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let delay = Arc::downgrade(self);
        tokio::spawn(async move {
            let resolution = Duration::from_millis(20);
            loop {
                let started = Instant::now();
                tokio::time::sleep(resolution).await;
                let Some(delay) = delay.upgrade() else { return };
                delay.record(started.elapsed().saturating_sub(resolution));
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMb {
    #[serde(serialize_with = "js_number_opt")]
    pub rss: Option<f64>,
    pub heap_used: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bans {
    pub supported: bool,
    pub count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealth {
    pub uptime_seconds: u64,
    #[serde(rename = "memoryMB")]
    pub memory_mb: MemoryMb,
    pub requests: RequestsSnapshot,
    pub event_loop: Option<LoopDelaySnapshot>,
    pub bans: Bans,
}

/// Health tracking for one metagame process.
pub struct Health {
    pub enabled: bool,
    started: Instant,
    pub requests: RequestMetrics,
    pub loop_delay: Arc<LoopDelay>,
    monitor: std::sync::OnceLock<()>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Health {
    /// `BACKEND_HEALTH=1` turns tracking on; the delay monitor starts with the first tracked request,
    /// as in Node.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            started: Instant::now(),
            requests: RequestMetrics::default(),
            loop_delay: Arc::default(),
            monitor: std::sync::OnceLock::new(),
        }
    }

    pub fn report(&self) -> RuntimeHealth {
        RuntimeHealth {
            uptime_seconds: self.started.elapsed().as_secs(),
            memory_mb: MemoryMb {
                rss: memory_stats::memory_stats().map(|stats| stats.physical_mem as f64 / 1_048_576.0),
                heap_used: None,
            },
            requests: self.requests.snapshot(now_ms()),
            event_loop: self.loop_delay.snapshot(),
            bans: Bans {
                supported: true,
                count: None,
            },
        }
    }
}

/// Records a request when its reply is done, or as aborted when the client goes away first.
struct Pending<'a> {
    health: &'a Health,
    started: Instant,
    done: bool,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.health.requests.record(0, 0.0, true, now_ms());
        }
    }
}

/// Middleware: `TrackBackendHealth` (`axum::middleware::from_fn_with_state(health, track)`).
pub async fn track(State(health): State<Arc<Health>>, request: Request, next: Next) -> Response {
    if !health.enabled || is_monitoring_poll(request.uri().path()) {
        return next.run(request).await;
    }
    health.monitor.get_or_init(|| drop(health.loop_delay.spawn_monitor()));
    let mut pending = Pending {
        health: &health,
        started: Instant::now(),
        done: false,
    };
    let response = next.run(request).await;
    pending.done = true;
    let ms = pending.started.elapsed().as_secs_f64() * 1000.0;
    health.requests.record(response.status().as_u16(), ms, false, now_ms());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cases of backendhealth.test.ts.
    #[test]
    fn rolling_metrics_count_errors_exclude_aborted_latency_and_expire() {
        let metrics = RequestMetrics::default();
        metrics.record(200, 4.0, false, 100_000);
        metrics.record(404, 30.0, false, 100_000);
        metrics.record(503, 6000.0, false, 100_000);
        metrics.record(200, 100.0, true, 100_000);
        let value = metrics.snapshot(100_000);
        assert_eq!(
            (value.completed, value.client_errors, value.server_errors, value.aborted),
            (3, 1, 1, 1)
        );
        assert_eq!(value.latency_p50_upper_ms, Some(UpperBound::Ms(50)));
        assert_eq!(value.latency_p95_upper_ms, Some(UpperBound::Over(">5000")));
        assert_eq!(metrics.snapshot(160_000).completed, 0);
        assert_eq!(metrics.snapshot(160_000).latency_p95_upper_ms, None);
    }

    #[test]
    fn the_report_has_nodes_field_names_and_number_format() {
        let metrics = RequestMetrics::default();
        for _ in 0..60 {
            metrics.record(200, 3.0, false, 100_000);
        }
        let json = serde_json::to_string(&metrics.snapshot(100_000)).unwrap();
        assert_eq!(
            json,
            r#"{"windowSeconds":60,"completed":60,"clientErrors":0,"serverErrors":0,"aborted":0,"requestsPerSecond":1,"serverErrorPercent":0,"latencyP50UpperMs":5,"latencyP95UpperMs":5}"#
        );
        let health = Health::new(true);
        let report = serde_json::to_value(health.report()).unwrap();
        assert_eq!(report["memoryMB"]["heapUsed"], serde_json::Value::Null);
        assert_eq!(report["eventLoop"], serde_json::Value::Null, "nothing measured yet");
        assert_eq!(report["bans"], serde_json::json!({"supported": true, "count": null}));
    }

    #[test]
    fn loop_delay_percentiles() {
        let delay = LoopDelay::default();
        for micros in [100, 100, 200, 300, 12_000] {
            delay.record(Duration::from_micros(micros));
        }
        let snapshot = delay.snapshot().unwrap();
        // The third of five samples, the fifth, and the largest.
        assert_eq!((snapshot.p50_ms, snapshot.p95_ms, snapshot.max_ms), (0.2, 12.0, 12.0));
    }

    #[test]
    fn monitoring_polls_are_not_counted() {
        assert!(is_monitoring_poll("/undaunted/api/ServerStatus/"));
        assert!(is_monitoring_poll("/UNDAUNTED/api/backendhealth"));
        assert!(!is_monitoring_poll("/undaunted/api/GetUserInfo"));
    }
}
