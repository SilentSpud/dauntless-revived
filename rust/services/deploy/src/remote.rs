//! Other machines' deploy servers, reached through loopback tunnels (`overflow.ts`, `regions.ts`):
//! the overflow worker that takes hunts when this machine is busy, and the regional workers (Germany,
//! US) a party can choose. A worker that refused for capacity is asked again only after its
//! Retry-After; an ambiguous failure never starts a duplicate here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::{Value, json};
use url::Url;

use crate::capacity::{AdmissionStatus, DeployError, Reason, hunt_limit};
use crate::cities::Connection;
use crate::process::BoxFuture;
use crate::settings::Settings;

/// A loopback HTTP tunnel URL from `name`, or `None` when unset.
fn tunnel_url(settings: &Settings, name: &str, problem: &str) -> Result<Option<Url>, DeployError> {
    let Some(value) = settings.non_empty(name) else {
        return Ok(None);
    };
    let url = Url::parse(&value).map_err(|_| DeployError::failed(format!("Invalid URL in {name}")))?;
    let plain = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !plain {
        return Err(DeployError::failed(problem.replace("{name}", name)));
    }
    Ok(Some(url))
}

pub fn overflow_url(settings: &Settings) -> Result<Option<Url>, DeployError> {
    tunnel_url(
        settings,
        "OVERFLOW_DEPLOYSERVER_URL",
        "OVERFLOW_DEPLOYSERVER_URL must be an HTTP loopback tunnel",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Main,
    Aus,
    Ger,
    Us,
    Mixed,
}

impl Region {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "main" => Self::Main,
            "aus" => Self::Aus,
            "ger" => Self::Ger,
            "us" => Self::Us,
            "mixed" => Self::Mixed,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Aus => "aus",
            Self::Ger => "ger",
            Self::Us => "us",
            Self::Mixed => "mixed",
        }
    }
}

/// A regional worker's tunnel. Oceania is retired for good, also when an old environment file is
/// restored.
pub fn region_url(settings: &Settings, region: Region) -> Result<Option<Url>, DeployError> {
    let name = match region {
        Region::Aus | Region::Main | Region::Mixed => return Ok(None),
        Region::Us => "US_DEPLOYSERVER_URL",
        Region::Ger => "GERMANY_DEPLOYSERVER_URL",
    };
    tunnel_url(settings, name, "{name} must be an HTTP loopback tunnel")
}

pub fn utilization(load: &AdmissionStatus) -> f64 {
    match load.limit {
        Some(limit) if limit > 0 => (load.running + load.pending) as f64 / limit as f64,
        _ => f64::INFINITY,
    }
}

/// What came back from another deploy server: its status, Retry-After and JSON body.
pub struct Answer {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: Result<Value, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The connection was refused: the request never reached the worker
    Refused,
    /// Anything else (a timeout, a reset): it may have been handled
    Other(String),
}

/// The HTTP client for the tunnels.
#[derive(Clone)]
pub struct Http {
    client: Client<HttpConnector, Full<Bytes>>,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        Self {
            client: Client::builder(TokioExecutor::new()).build_http(),
        }
    }

    pub async fn call(
        &self,
        method: &str,
        url: &Url,
        body: Option<&Value>,
        timeout: Duration,
        headers: &[(&str, &str)],
    ) -> Result<Answer, CallError> {
        let mut request = http::Request::builder().method(method).uri(url.as_str());
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let request = request
            .body(Full::new(Bytes::from(body.map(Value::to_string).unwrap_or_default())))
            .map_err(|e| CallError::Other(e.to_string()))?;
        let exchange = async {
            let response = self.client.request(request).await.map_err(|error| {
                if is_refused(&error) {
                    CallError::Refused
                } else {
                    CallError::Other(error.to_string())
                }
            })?;
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| CallError::Other(e.to_string()))?
                .to_bytes();
            Ok(Answer {
                status,
                retry_after,
                body: serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            })
        };
        tokio::time::timeout(timeout, exchange)
            .await
            .unwrap_or_else(|_| Err(CallError::Other("The operation was aborted due to timeout".into())))
    }
}

fn is_refused(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(current) = source {
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            return io.kind() == std::io::ErrorKind::ConnectionRefused;
        }
        source = current.source();
    }
    false
}

/// A worker's `GET /gameservers`. An empty result from an unreachable worker is not evidence that
/// its sessions died, so it is `complete: false`.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub servers: Vec<Value>,
    pub complete: bool,
}

pub async fn read_worker_snapshot(http: &Http, url: &Url) -> Snapshot {
    let unknown = Snapshot {
        servers: Vec::new(),
        complete: false,
    };
    let Ok(target) = url.join("/gameservers") else {
        return unknown;
    };
    match http.call("GET", &target, None, Duration::from_millis(1500), &[]).await {
        Ok(Answer {
            status, body: Ok(body), ..
        }) if (200..300).contains(&status) => match body["servers"].as_array() {
            // Older workers do not report whether their own remote snapshots succeeded
            Some(servers) => Snapshot {
                servers: servers.clone(),
                complete: body["complete"] == true,
            },
            None => unknown,
        },
        _ => unknown,
    }
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// `RemoteLaunch`, with its shared capacity cooldowns.
pub struct Remote {
    pub http: Http,
    retry_at: Mutex<HashMap<String, i64>>,
    now: Clock,
}

impl Default for Remote {
    fn default() -> Self {
        Self::new(Arc::new(crate::now_ms))
    }
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

impl Remote {
    pub fn new(now: Clock) -> Self {
        Self {
            http: Http::new(),
            retry_at: Mutex::default(),
            now,
        }
    }

    /// Starts `body` on the worker at `url`: the connection, `None` when the worker cannot take it
    /// (down, or full), an error when the outcome is unknown.
    pub async fn launch(&self, url: &Url, body: &Value) -> Result<Option<Connection>, DeployError> {
        let hunt = body["HuntId"].clone();
        let key = format!("{}|{}", origin(url), body["GameMode"].as_str().unwrap_or("undefined"));
        let now = (self.now)();
        {
            let mut retry_at = self.retry_at.lock().unwrap_or_else(PoisonError::into_inner);
            retry_at.retain(|_, until| *until > now);
            if retry_at.contains_key(&key) {
                return Ok(None);
            }
        }
        // A tunnel can accept TCP while its remote target is down: probe without side effects first
        let probe = url
            .join("/gameservers")
            .map_err(|e| DeployError::failed(e.to_string()))?;
        match self.http.call("GET", &probe, None, Duration::from_secs(2), &[]).await {
            Ok(Answer {
                status,
                body: Ok(status_body),
                ..
            }) if (200..300).contains(&status) && status_body["servers"].is_array() => {
                tracing::info!(huntId = %hunt, "routing: worker capacity");
            }
            Ok(_) => return Ok(None),
            Err(_) => {
                tracing::warn!(huntId = %hunt, "routing: worker probe unavailable");
                return Ok(None);
            }
        }
        let target = url
            .join("/api/matchmaker/handle-matchmaking-for-player")
            .map_err(|e| DeployError::failed(e.to_string()))?;
        let answer = match self
            .http
            .call("POST", &target, Some(body), Duration::from_secs(120), &[])
            .await
        {
            Ok(answer) => answer,
            // A refused connection never reached the worker
            Err(CallError::Refused) => return Ok(None),
            Err(CallError::Other(error)) => {
                tracing::warn!(huntId = %hunt, "routing: worker timeout or ambiguous transport failure; no duplicate fallback");
                return Err(DeployError::Failed(error));
            }
        };
        let result = answer.body.unwrap_or(Value::Null);
        if answer.status == 503 && result["error"] == "capacity_unavailable" {
            let seconds = answer
                .retry_after
                .map(|r| service_kit::js::number(Some(&Value::String(r))))
                .unwrap_or(f64::NAN);
            let delay = if seconds.is_finite() && seconds > 0.0 {
                seconds.clamp(1.0, 30.0)
            } else {
                10.0
            };
            let until = (self.now)() + (delay * 1000.0) as i64;
            self.retry_at
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, until);
            tracing::info!(huntId = %hunt, reason = %result["reason"], "routing: worker capacity rejection");
            return Ok(None);
        }
        let port = result["port"]
            .as_f64()
            .filter(|p| p.fract() == 0.0 && (1.0..=65535.0).contains(p));
        let (Some(host), Some(port), true) = (result["host"].as_str(), port, (200..300).contains(&answer.status))
        else {
            tracing::warn!(huntId = %hunt, status = answer.status, "routing: worker HTTP error");
            return Err(DeployError::failed(format!(
                "Overflow launch failed ({})",
                answer.status
            )));
        };
        tracing::info!(huntId = %hunt, host, port, selected = "worker", "routing: selected");
        Ok(Some(Connection {
            host: host.to_owned(),
            port: port as u16,
            session_id: result["sessionId"].as_str().map(str::to_owned),
        }))
    }
}

pub type RemoteFn = Arc<dyn Fn(Url, Value) -> BoxFuture<Result<Option<Connection>, DeployError>> + Send + Sync>;

pub fn remote_fn(remote: Arc<Remote>) -> RemoteFn {
    Arc::new(move |url, body| {
        let remote = remote.clone();
        Box::pin(async move { remote.launch(&url, &body).await })
    })
}

type LocalStatus = Arc<dyn Fn() -> Result<AdmissionStatus, String> + Send + Sync>;

/// Spills whole hunts (with their party) to the overflow worker past `OVERFLOW_AFTER_HUNTS`;
/// persistent worlds stay here.
pub struct HuntRouter {
    pending: AtomicU64,
    local_count: Arc<dyn Fn() -> u64 + Send + Sync>,
    remote: RemoteFn,
    local_status: Option<LocalStatus>,
    settings: Settings,
}

impl HuntRouter {
    pub fn new(
        settings: Settings,
        local_count: impl Fn() -> u64 + Send + Sync + 'static,
        remote: RemoteFn,
        local_status: Option<LocalStatus>,
    ) -> Self {
        Self {
            pending: AtomicU64::new(0),
            local_count: Arc::new(local_count),
            remote,
            local_status,
            settings,
        }
    }

    pub async fn launch<F>(&self, body: &Value, local: F) -> Result<Connection, DeployError>
    where
        F: std::future::Future<Output = Result<Connection, DeployError>>,
    {
        let url = overflow_url(&self.settings)?;
        let (Some(url), true) = (url, body["GameMode"] == "ISLAND") else {
            return local.await;
        };
        let threshold = self.settings.number("OVERFLOW_AFTER_HUNTS").unwrap_or(4.0);
        if threshold.fract() != 0.0 || threshold < 0.0 {
            return Err(DeployError::failed("Invalid OVERFLOW_AFTER_HUNTS"));
        }
        let status = match &self.local_status {
            Some(status) => status()?,
            None => AdmissionStatus {
                running: (self.local_count)(),
                pending: self.pending.load(Ordering::SeqCst),
                limit: hunt_limit(&self.settings)?,
            },
        };
        let prefer_remote = (status.running + status.pending) as f64 >= threshold;
        let hunt = body["HuntId"].clone();
        tracing::info!(
            huntId = %hunt,
            localRunning = status.running,
            pendingLocal = status.pending,
            localCapacity = ?status.limit,
            threshold,
            preferred = if prefer_remote { "worker" } else { "primary" },
            "routing: preference"
        );
        self.pending.fetch_add(1, Ordering::SeqCst);
        let result = async {
            if prefer_remote && let Some(connection) = (self.remote)(url.clone(), body.clone()).await? {
                return Ok(connection);
            }
            match local.await {
                Ok(connection) => {
                    tracing::info!(huntId = %hunt, selected = "primary", host = %connection.host, port = connection.port, "routing: selected");
                    Ok(connection)
                }
                Err(error) => {
                    let DeployError::Capacity(reason) = &error else {
                        return Err(error);
                    };
                    tracing::info!(huntId = %hunt, reason = reason.as_str(), "routing: local capacity rejection");
                    if prefer_remote {
                        return Err(error);
                    }
                    match (self.remote)(url.clone(), body.clone()).await? {
                        Some(connection) => Ok(connection),
                        None => Err(error),
                    }
                }
            }
        }
        .await;
        self.pending.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

type MainLoad = Arc<dyn Fn() -> BoxFuture<Result<AdmissionStatus, DeployError>> + Send + Sync>;
type RegionLoad = Arc<dyn Fn(Url) -> BoxFuture<Result<AdmissionStatus, DeployError>> + Send + Sync>;

/// A regional worker's capacity, from its `GET /gameservers`.
pub fn region_load(http: Http) -> RegionLoad {
    Arc::new(move |url: Url| {
        let http = http.clone();
        Box::pin(async move {
            let target = url
                .join("/gameservers")
                .map_err(|e| DeployError::failed(e.to_string()))?;
            let answer = http
                .call("GET", &target, None, Duration::from_secs(2), &[])
                .await
                .map_err(|e| DeployError::failed(format!("{e:?}")))?;
            if !(200..300).contains(&answer.status) {
                return Err(DeployError::failed("AUS capacity unavailable"));
            }
            let body = answer.body.map_err(DeployError::Failed)?;
            let capacity = &body["capacity"];
            let (Some(running), Some(pending)) = (capacity["running"].as_f64(), capacity["pending"].as_f64()) else {
                return Err(DeployError::failed("Invalid AUS capacity"));
            };
            Ok(AdmissionStatus {
                running: running as u64,
                pending: pending as u64,
                limit: capacity["limit"].as_f64().map(|l| l as u64),
            })
        })
    })
}

/// Where a party's world starts: its chosen region's worker, or this machine (spilling to Germany
/// or the US when this machine is full).
pub struct RegionalRouter {
    main_load: MainLoad,
    remote: RemoteFn,
    region_load: RegionLoad,
    settings: Settings,
}

impl RegionalRouter {
    pub fn new(settings: Settings, main_load: MainLoad, remote: RemoteFn, region_load: RegionLoad) -> Self {
        Self {
            main_load,
            remote,
            region_load,
            settings,
        }
    }

    pub async fn launch<F>(
        &self,
        body: &Value,
        choice: Region,
        main: impl FnOnce() -> F,
    ) -> Result<Connection, DeployError>
    where
        F: std::future::Future<Output = Result<Connection, DeployError>>,
    {
        if matches!(choice, Region::Ger | Region::Us) {
            let Some(url) = region_url(&self.settings, choice)? else {
                return Err(DeployError::Capacity(Reason::Hunts));
            };
            let Some(connection) = (self.remote)(url, body.clone()).await? else {
                return Err(DeployError::Capacity(Reason::Hunts));
            };
            let expected = self.settings.non_empty(if choice == Region::Us {
                "US_PUBLIC_HOST"
            } else {
                "GERMANY_PUBLIC_HOST"
            });
            if expected.is_some_and(|host| connection.host != host) {
                return Err(DeployError::failed(
                    "Regional worker returned a destination outside the selected region",
                ));
            }
            return Ok(connection);
        }
        let overflowed = || {
            let mut body = body.clone();
            body["Overflow"] = json!(true);
            body
        };
        // This machine, then Germany when it is full
        let main_then_germany = async {
            match main().await {
                Err(DeployError::Capacity(reason)) => {
                    if let Some(germany) = region_url(&self.settings, Region::Ger)?
                        && let Some(connection) = (self.remote)(germany, overflowed()).await?
                    {
                        return Ok(connection);
                    }
                    Err(DeployError::Capacity(reason))
                }
                other => other,
            }
        };
        if choice == Region::Aus {
            return Err(DeployError::Capacity(Reason::Hunts));
        }
        let us = region_url(&self.settings, Region::Us)?;
        let mode = body["GameMode"].as_str().unwrap_or("");
        let (Some(us), true) = (us, ["ISLAND", "CITY", "SHARED"].contains(&mode)) else {
            return main_then_germany.await;
        };
        let mut us_first = false;
        if choice == Region::Mixed
            && let (Ok(primary), Ok(regional)) = tokio::join!((self.main_load)(), (self.region_load)(us.clone()))
        {
            us_first = utilization(&regional) < utilization(&primary);
        }
        if us_first {
            if let Some(connection) = (self.remote)(us.clone(), body.clone()).await? {
                return Ok(connection);
            }
            return main_then_germany.await;
        }
        match main_then_germany.await {
            Err(DeployError::Capacity(reason)) => {
                let error = DeployError::Capacity(reason);
                // Overflow must leave slots for players who explicitly chose the US
                let reserve = self.settings.number("US_RESERVED_HUNTS").unwrap_or(0.0);
                if !(reserve.fract() == 0.0 && reserve >= 0.0) {
                    return Err(DeployError::failed("Invalid US_RESERVED_HUNTS"));
                }
                if reserve > 0.0 && mode == "ISLAND" {
                    match (self.region_load)(us.clone()).await {
                        Ok(load) => match load.limit {
                            Some(limit) if (limit as f64) - (load.running + load.pending) as f64 > reserve => {}
                            _ => return Err(error),
                        },
                        Err(_) => return Err(error),
                    }
                }
                match (self.remote)(us, overflowed()).await? {
                    Some(connection) => Ok(connection),
                    None => Err(error),
                }
            }
            other => other,
        }
    }
}
