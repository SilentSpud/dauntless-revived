//! Downloads game files from the host's content server (contract 4, row L23): `src/main/downloader.ts`.
//!
//! - Only files from the compiled-in manifest are requested, and each one is written to `<file>.part`
//!   first. A file only gets its real name after its size and SHA-256 match.
//! - Interrupted files resume with an HTTP Range request (the `.part` is hashed again first).
//! - Up to 4 files at once, retries with exponential backoff, pause / resume / cancel.
//! - In public mode every connection is TLS pinned to the invite's certificate; a certificate that
//!   does not match stops the whole job before any request is sent.
//! - A host may name a CDN (`downloadBaseUrl`): plain HTTPS with the usual CA checks, never the key.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::header::{HeaderMap, HeaderValue};
use hyper::{Request, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::pki_types::CertificateDer;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::connector::{Host, HostConnector, pin_error};
use super::http::{Endpoint, host_header, pinned_target};
use crate::core::manifest::{ManifestFile, resolve_inside};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadErrorCode {
    KeyRejected,
    FileMissingOnServer,
    FileDifferentOnServer,
    DownloadFailed,
    DiskSpace,
    DiskError,
    CertMismatch,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadError {
    pub code: DownloadErrorCode,
    pub file: Option<String>,
    pub message: String,
}

impl DownloadError {
    fn new(code: DownloadErrorCode, file: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code,
            file: file.map(str::to_owned),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DownloadError {}

/// Why one try at one file ended.
enum Failure {
    /// Paused or cancelled
    Interrupted,
    Attempt {
        retry: bool,
        message: String,
        fatal: Option<DownloadError>,
        /// The server is busy (429 / 503 with Retry-After): wait this long, not counted as a failure
        busy_wait: Option<Duration>,
    },
    Io(std::io::Error),
}

fn retry(message: impl Into<String>) -> Failure {
    Failure::Attempt {
        retry: true,
        message: message.into(),
        fatal: None,
        busy_wait: None,
    }
}

fn fatal(error: DownloadError) -> Failure {
    Failure::Attempt {
        retry: false,
        message: error.message.clone(),
        fatal: Some(error),
        busy_wait: None,
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Retry-After in seconds, kept between 1 and 30 s; 2 s when it is not a positive number.
pub fn retry_after(value: Option<&str>) -> Duration {
    let seconds = value
        .and_then(|text| crate::core::text::js_trim(text).parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0);
    seconds.map_or(Duration::from_secs(2), |n| Duration::from_secs_f64(n.clamp(1.0, 30.0)))
}

const MAX_BUSY_WAITS: u32 = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadProgress {
    pub paused: bool,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub files_total: usize,
    pub files_done: usize,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<f64>,
    pub current_file: Option<String>,
}

pub type Backoff = Arc<dyn Fn(u32) -> Duration + Send + Sync>;
pub type OnFile = Arc<dyn Fn(&ManifestFile) + Send + Sync>;
pub type OnRetry = Arc<dyn Fn(&ManifestFile, u32, &str) + Send + Sync>;

#[derive(Clone)]
pub struct DownloadOptions {
    pub endpoint: Endpoint,
    pub download_base_url: Option<String>,
    pub key: String,
    pub install_dir: PathBuf,
    pub files: Vec<ManifestFile>,
    pub concurrency: usize,
    pub max_attempts: u32,
    pub backoff: Backoff,
    pub stall_timeout: Duration,
    pub progress_interval: Duration,
    pub on_progress: Option<Arc<dyn Fn(DownloadProgress) + Send + Sync>>,
    pub on_file_verified: Option<OnFile>,
    pub on_retry: Option<OnRetry>,
    /// Certificates trusted for the CDN besides the usual roots (tests)
    pub cdn_roots: Vec<CertificateDer<'static>>,
}

impl DownloadOptions {
    pub fn new(endpoint: Endpoint, key: String, install_dir: PathBuf, files: Vec<ManifestFile>) -> Self {
        Self {
            endpoint,
            download_base_url: None,
            key,
            install_dir,
            files,
            concurrency: crate::core::pins::DOWNLOAD_CONCURRENCY,
            max_attempts: 6,
            backoff: Arc::new(default_backoff),
            stall_timeout: Duration::from_secs(30),
            progress_interval: Duration::from_millis(250),
            on_progress: None,
            on_file_verified: None,
            on_retry: None,
            cdn_roots: Vec::new(),
        }
    }
}

/// Game hashes stay pinned in the launcher even when the host names a CDN: only an https URL without
/// credentials, query or fragment, ending in one `/`.
pub fn content_download_base(raw: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(raw) = raw else { return Ok(None) };
    let url = url::Url::parse(raw).map_err(|_| "Invalid content download URL")?;
    let empty = |part: Option<&str>| part.is_none_or(str::is_empty);
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !empty(url.query())
        || !empty(url.fragment())
    {
        return Err("Invalid content download URL");
    }
    Ok(Some(format!("{}/", url.as_str().trim_end_matches('/'))))
}

/// `encodeURIComponent`, segment by segment.
fn encode_path(relative: &str) -> String {
    const COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    relative
        .split('/')
        .map(|segment| percent_encoding::utf8_percent_encode(segment, COMPONENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn file_url_path(relative: &str) -> String {
    format!("/content/v1/files/{}", encode_path(relative))
}

pub fn default_backoff(attempt: u32) -> Duration {
    let base = (1000.0 * 2f64.powi(attempt.saturating_sub(1).min(16) as i32)).min(30000.0);
    Duration::from_millis((base * (0.75 + rand::random::<f64>() * 0.5)).round() as u64)
}

/// Bytes per second over the last few seconds.
pub struct SpeedMeter {
    buckets: VecDeque<(Instant, u64)>,
    window: Duration,
}

impl SpeedMeter {
    pub fn new(window: Duration) -> Self {
        Self {
            buckets: VecDeque::new(),
            window,
        }
    }

    pub fn add_at(&mut self, now: Instant, bytes: u64) {
        match self.buckets.back_mut() {
            Some((at, total)) if now.duration_since(*at) < Duration::from_millis(250) => *total += bytes,
            _ => self.buckets.push_back((now, bytes)),
        }
        self.trim(now);
    }

    pub fn rate_at(&mut self, now: Instant) -> f64 {
        self.trim(now);
        let Some((first, _)) = self.buckets.front() else {
            return 0.0;
        };
        let span = now.duration_since(*first).max(Duration::from_secs(1));
        let bytes: u64 = self.buckets.iter().map(|(_, bytes)| bytes).sum();
        bytes as f64 / span.as_secs_f64()
    }

    pub fn reset(&mut self) {
        self.buckets.clear();
    }

    fn trim(&mut self, now: Instant) {
        while self
            .buckets
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > self.window)
        {
            self.buckets.pop_front();
        }
    }
}

fn parse_content_range(value: Option<&str>) -> Option<(u64, u64, u64)> {
    let rest = value?.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let number = |text: &str| {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse::<u64>().ok())
            .flatten()
    };
    Some((number(start)?, number(end)?, number(total)?))
}

fn etag_hash(value: Option<&str>) -> Option<String> {
    let text = value?.trim();
    let text = text.strip_prefix("W/").unwrap_or(text);
    let text = text.strip_prefix('"').unwrap_or(text);
    let text = text.strip_suffix('"').unwrap_or(text);
    Some(text.to_ascii_lowercase())
}

fn disk_error(error: &std::io::Error) -> Option<DownloadError> {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::StorageFull => Some(DownloadError::new(DownloadErrorCode::DiskSpace, None, "disk_space")),
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem | ErrorKind::ResourceBusy => Some(
            DownloadError::new(DownloadErrorCode::DiskError, None, error.to_string()),
        ),
        _ => None,
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    PathBuf::from(part)
}

enum Fetcher {
    Host(Client<HostConnector, Empty<Bytes>>),
    Cdn {
        client: Client<hyper_rustls::HttpsConnector<HttpConnector>, Empty<Bytes>>,
        base: String,
    },
}

struct State {
    queue: VecDeque<ManifestFile>,
    completed_bytes: u64,
    files_done: usize,
    in_flight: HashMap<String, (u64, CancellationToken)>,
    /// Bytes already on disk (in .part files) for files that are queued but not being fetched
    partial: HashMap<String, u64>,
    attempts: HashMap<String, u32>,
    busy_waits: HashMap<String, u32>,
    speed: SpeedMeter,
    paused: bool,
    cancelled: bool,
    fatal: Option<DownloadError>,
}

struct Inner {
    options: DownloadOptions,
    total_bytes: u64,
    state: Mutex<State>,
    /// Woken on resume, cancel and a fatal error
    changed: Notify,
    fetcher: Fetcher,
}

#[derive(Clone)]
pub struct DownloadJob {
    inner: Arc<Inner>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn progress(&self) -> DownloadProgress {
        let mut state = self.state();
        let mut done = state.completed_bytes + state.partial.values().sum::<u64>();
        let mut current = None;
        let mut biggest = None;
        for (path, (bytes, _)) in &state.in_flight {
            done += bytes;
            if biggest.is_none_or(|most| *bytes > most) {
                biggest = Some(*bytes);
                current = Some(path.clone());
            }
        }
        let rate = if state.paused {
            0.0
        } else {
            state.speed.rate_at(Instant::now())
        };
        let left = self.total_bytes.saturating_sub(done);
        DownloadProgress {
            paused: state.paused,
            total_bytes: self.total_bytes,
            done_bytes: done.min(self.total_bytes),
            files_total: self.options.files.len(),
            files_done: state.files_done,
            bytes_per_second: rate,
            eta_seconds: (rate > 0.0).then(|| left as f64 / rate),
            current_file: current,
        }
    }

    fn emit(&self) {
        if let Some(on_progress) = &self.options.on_progress {
            on_progress(self.progress());
        }
    }

    fn stopped(&self) -> bool {
        let state = self.state();
        state.cancelled || state.fatal.is_some()
    }

    fn stop_for(&self, error: DownloadError) {
        let mut state = self.state();
        if state.fatal.is_none() {
            state.fatal = Some(error);
        }
        for (_, token) in state.in_flight.values() {
            token.cancel();
        }
        drop(state);
        self.changed.notify_waiters();
    }

    async fn wait_while_paused(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = self.state();
                if !state.paused || state.cancelled || state.fatal.is_some() {
                    return;
                }
            }
            notified.await;
        }
    }

    /// Waits, but returns early on pause, cancel or a fatal error.
    async fn pausable_sleep(&self, duration: Duration) {
        let deadline = tokio::time::Instant::now() + duration;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = self.state();
                if state.paused || state.cancelled || state.fatal.is_some() {
                    return;
                }
            }
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => return,
                () = notified => {}
            }
        }
    }

    async fn worker(self: Arc<Self>) {
        loop {
            self.wait_while_paused().await;
            let (file, token) = {
                let mut state = self.state();
                if state.cancelled || state.fatal.is_some() {
                    return;
                }
                let Some(file) = state.queue.pop_front() else { return };
                let token = CancellationToken::new();
                state.in_flight.insert(file.path.clone(), (0, token.clone()));
                (file, token)
            };
            let result = self.attempt(&file, &token).await;
            let slot_bytes = self.state().in_flight.remove(&file.path).map_or(0, |(bytes, _)| bytes);
            let failure = match result {
                Ok(()) => {
                    {
                        let mut state = self.state();
                        state.completed_bytes += file.size;
                        state.files_done += 1;
                    }
                    if let Some(on_verified) = &self.options.on_file_verified {
                        on_verified(&file);
                    }
                    continue;
                }
                Err(failure) => failure,
            };
            if self.stopped() {
                return;
            }
            let paused = self.state().paused;
            if matches!(failure, Failure::Interrupted) || paused {
                let mut state = self.state();
                if slot_bytes > 0 {
                    state.partial.insert(file.path.clone(), slot_bytes);
                }
                // Paused: picked up again on resume, from its .part
                state.queue.push_front(file);
                continue;
            }
            let (retryable, message, busy_wait) = match failure {
                Failure::Io(error) => match disk_error(&error) {
                    Some(mut disk) => {
                        disk.file = Some(file.path.clone());
                        self.stop_for(disk);
                        return;
                    }
                    None => (true, error.to_string(), None),
                },
                Failure::Attempt { fatal: Some(error), .. } => {
                    self.stop_for(error);
                    return;
                }
                Failure::Attempt {
                    retry,
                    message,
                    busy_wait,
                    ..
                } => (retry, message, busy_wait),
                Failure::Interrupted => (true, "interrupted".into(), None),
            };
            if let Some(wait) = busy_wait {
                let waits = {
                    let mut state = self.state();
                    let waits = state.busy_waits.entry(file.path.clone()).or_insert(0);
                    *waits += 1;
                    *waits
                };
                if waits <= MAX_BUSY_WAITS {
                    if slot_bytes > 0 {
                        self.state().partial.insert(file.path.clone(), slot_bytes);
                    }
                    self.pausable_sleep(wait).await;
                    self.state().queue.push_front(file);
                    continue;
                }
            }
            let attempt = {
                let mut state = self.state();
                let attempts = state.attempts.entry(file.path.clone()).or_insert(0);
                *attempts += 1;
                *attempts
            };
            if attempt >= self.options.max_attempts || !retryable {
                self.stop_for(DownloadError::new(
                    DownloadErrorCode::DownloadFailed,
                    Some(&file.path),
                    message,
                ));
                return;
            }
            if let Some(on_retry) = &self.options.on_retry {
                on_retry(&file, attempt, &message);
            }
            self.pausable_sleep((self.options.backoff)(attempt)).await;
            self.state().queue.push_front(file);
        }
    }

    fn set_slot(&self, path: &str, bytes: u64) {
        if let Some((slot, _)) = self.state().in_flight.get_mut(path) {
            *slot = bytes;
        }
    }

    /// One try at one file: done once `<file>` exists with the right size and hash.
    async fn attempt(&self, file: &ManifestFile, token: &CancellationToken) -> Result<(), Failure> {
        let dest = resolve_inside(&self.options.install_dir, &file.path).map_err(retry)?;
        let part = part_path(&dest);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut offset = tokio::fs::metadata(&part).await.map_or(0, |metadata| metadata.len());
        if offset > file.size {
            tokio::fs::remove_file(&part).await?;
            offset = 0;
        }
        self.set_slot(&file.path, offset);
        self.state().partial.remove(&file.path);
        // Empty files need no transfer; the hash check below still applies
        if file.size == 0 {
            tokio::fs::write(&part, b"").await?;
        }
        let mut hash = Sha256::new();
        if offset > 0 {
            let mut existing = tokio::fs::File::open(&part).await?;
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                if token.is_cancelled() {
                    return Err(Failure::Interrupted);
                }
                let n = existing.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
        }
        if offset < file.size {
            self.fetch_into(file, &part, offset, &mut hash, token).await?;
        }
        let size = tokio::fs::metadata(&part).await?.len();
        let digest: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        if size != file.size || digest != file.sha256 {
            let _ = tokio::fs::remove_file(&part).await;
            self.set_slot(&file.path, 0);
            return Err(retry(format!("verification failed for {}", file.path)));
        }
        tokio::fs::rename(&part, &dest).await?;
        Ok(())
    }

    async fn fetch_into(
        &self,
        file: &ManifestFile,
        part: &Path,
        offset: u64,
        hash: &mut Sha256,
        token: &CancellationToken,
    ) -> Result<(), Failure> {
        let mut headers = HeaderMap::new();
        let value = |text: String| HeaderValue::from_str(&text).map_err(|_| retry("invalid header"));
        headers.insert("accept-encoding", HeaderValue::from_static("identity"));
        let (request, cdn) = match &self.fetcher {
            Fetcher::Cdn { base, .. } => {
                let url = format!("{base}{}", encode_path(&file.path));
                (
                    Request::get(url.parse::<Uri>().map_err(|_| retry("invalid URL"))?),
                    true,
                )
            }
            Fetcher::Host(_) => {
                // Never send account credentials or the gateway Host header to the CDN: only here
                headers.insert("host", value(host_header(&self.options.endpoint))?);
                headers.insert("x-undaunted-user-api-key", value(self.options.key.clone())?);
                let uri = format!("http://content-host{}", file_url_path(&file.path));
                (
                    Request::get(uri.parse::<Uri>().map_err(|_| retry("invalid URL"))?),
                    false,
                )
            }
        };
        if offset > 0 {
            headers.insert("range", value(format!("bytes={offset}-"))?);
            if !cdn {
                headers.insert("if-range", value(format!("\"{}\"", file.sha256))?);
            }
        }
        let mut request = request.body(Empty::new()).map_err(|_| retry("invalid request"))?;
        *request.headers_mut() = headers;

        let stall = self.options.stall_timeout;
        let sent = async {
            match &self.fetcher {
                Fetcher::Host(client) => client.request(request).await.map_err(|error| match pin_error(&error) {
                    Some(_) => fatal(DownloadError::new(
                        DownloadErrorCode::CertMismatch,
                        Some(&file.path),
                        "certificate mismatch",
                    )),
                    None => retry(error.to_string()),
                }),
                Fetcher::Cdn { client, .. } => client.request(request).await.map_err(|error| retry(error.to_string())),
            }
        };
        let response = tokio::select! {
            () = token.cancelled() => return Err(Failure::Interrupted),
            result = tokio::time::timeout(stall, sent) => result.map_err(|_| retry("stalled"))??,
        };

        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let failure = |code, message: &str| Err(fatal(DownloadError::new(code, Some(&file.path), message)));
        match status {
            401 | 403 => return failure(DownloadErrorCode::KeyRejected, "key rejected"),
            404 => return failure(DownloadErrorCode::FileMissingOnServer, "not on server"),
            416 => {
                let _ = tokio::fs::remove_file(part).await;
                self.set_slot(&file.path, 0);
                return Err(retry("range not satisfiable"));
            }
            // 429 too_many_streams / 503 server_busy or auth_unavailable: come back later
            429 | 503 => {
                return Err(Failure::Attempt {
                    retry: true,
                    message: format!("HTTP {status}"),
                    fatal: None,
                    busy_wait: Some(retry_after(header("retry-after").as_deref())),
                });
            }
            200 | 206 => {}
            _ => return Err(retry(format!("HTTP {status}"))),
        }
        // A CDN's ETags (R2: MD5 or multipart ids) are not the manifest's SHA-256
        if !cdn && etag_hash(header("etag").as_deref()).is_some_and(|tag| tag != file.sha256) {
            return failure(DownloadErrorCode::FileDifferentOnServer, "server file differs");
        }
        let mut start = offset;
        if status == 206 {
            match parse_content_range(header("content-range").as_deref()) {
                Some((first, last, total)) if first == offset && last + 1 == file.size && total == file.size => {}
                _ => return Err(retry("bad content-range")),
            }
        } else if offset > 0 {
            // The server sent the whole file: start over
            start = 0;
            *hash = Sha256::new();
            self.set_slot(&file.path, 0);
        }
        let expected = file.size - start;
        if let Some(length) = header("content-length")
            && length.trim().parse::<u64>().ok() != Some(expected)
        {
            return Err(retry("unexpected content-length"));
        }

        let mut out = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(start > 0)
            .truncate(start == 0)
            .open(part)
            .await?;
        let mut body = response.into_body();
        let mut received = 0u64;
        let mut slot = start;
        loop {
            let frame = tokio::select! {
                () = token.cancelled() => {
                    let _ = out.flush().await;
                    return Err(Failure::Interrupted);
                }
                frame = tokio::time::timeout(stall, body.frame()) => match frame {
                    Err(_) => return Err(retry("stalled")),
                    Ok(None) => break,
                    Ok(Some(Err(error))) => return Err(retry(error.to_string())),
                    Ok(Some(Ok(frame))) => frame,
                },
            };
            let Some(chunk) = frame.data_ref() else { continue };
            received += chunk.len() as u64;
            if received > expected {
                return Err(retry("server sent too much"));
            }
            hash.update(chunk);
            out.write_all(chunk).await?;
            slot += chunk.len() as u64;
            {
                let mut state = self.state();
                state.speed.add_at(Instant::now(), chunk.len() as u64);
                if let Some((bytes, _)) = state.in_flight.get_mut(&file.path) {
                    *bytes = slot;
                }
            }
        }
        out.flush().await?;
        if received != expected {
            return Err(retry("connection closed early"));
        }
        Ok(())
    }
}

impl DownloadJob {
    pub fn new(options: DownloadOptions) -> Result<Self, DownloadError> {
        let invalid = |message: &str| DownloadError::new(DownloadErrorCode::DownloadFailed, None, message);
        let concurrency = options.concurrency.clamp(1, 4);
        let fetcher = match content_download_base(options.download_base_url.as_deref()).map_err(invalid)? {
            Some(base) => {
                let mut roots = rustls::RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                };
                for certificate in &options.cdn_roots {
                    roots
                        .add(certificate.clone())
                        .map_err(|_| invalid("invalid CDN root"))?;
                }
                let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::aws_lc_rs::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .map_err(|_| invalid("TLS setup failed"))?
                .with_root_certificates(roots)
                .with_no_client_auth();
                let connector = hyper_rustls::HttpsConnectorBuilder::new()
                    .with_tls_config(config)
                    .https_only()
                    .enable_http1()
                    .build();
                let client = Client::builder(TokioExecutor::new())
                    .pool_max_idle_per_host(concurrency)
                    .build(connector);
                Fetcher::Cdn { client, base }
            }
            None => {
                let host = match pinned_target(&options.endpoint) {
                    Some(target) => Host::Pinned(target),
                    None => Host::Plain {
                        host: options.endpoint.host.clone(),
                        port: options.endpoint.port,
                    },
                };
                let connector = HostConnector::new(host, Duration::from_secs(10));
                Fetcher::Host(
                    Client::builder(TokioExecutor::new())
                        .pool_max_idle_per_host(concurrency)
                        .set_host(false)
                        .build(connector),
                )
            }
        };
        // Biggest files first, so the long transfers overlap instead of finishing last
        let mut queue: Vec<ManifestFile> = options.files.clone();
        queue.sort_by_key(|file| std::cmp::Reverse(file.size));
        let total_bytes = options.files.iter().map(|file| file.size).sum();
        let options = DownloadOptions { concurrency, ..options };
        let state = State {
            queue: queue.into(),
            completed_bytes: 0,
            files_done: 0,
            in_flight: HashMap::new(),
            partial: HashMap::new(),
            attempts: HashMap::new(),
            busy_waits: HashMap::new(),
            speed: SpeedMeter::new(Duration::from_secs(5)),
            paused: false,
            cancelled: false,
            fatal: None,
        };
        Ok(Self {
            inner: Arc::new(Inner {
                options,
                total_bytes,
                state: Mutex::new(state),
                changed: Notify::new(),
                fetcher,
            }),
        })
    }

    pub fn paused(&self) -> bool {
        self.inner.state().paused
    }

    pub fn pause(&self) {
        {
            let mut state = self.inner.state();
            if state.paused || state.cancelled {
                return;
            }
            state.paused = true;
            state.speed.reset();
            for (_, token) in state.in_flight.values() {
                token.cancel();
            }
        }
        self.inner.changed.notify_waiters();
        self.inner.emit();
    }

    pub fn resume(&self) {
        {
            let mut state = self.inner.state();
            if !state.paused || state.cancelled {
                return;
            }
            state.paused = false;
        }
        self.inner.changed.notify_waiters();
        self.inner.emit();
    }

    pub fn cancel(&self) {
        {
            let mut state = self.inner.state();
            if state.cancelled {
                return;
            }
            state.cancelled = true;
            for (_, token) in state.in_flight.values() {
                token.cancel();
            }
        }
        self.inner.changed.notify_waiters();
    }

    pub fn progress(&self) -> DownloadProgress {
        self.inner.progress()
    }

    pub async fn run(&self) -> Result<(), DownloadError> {
        let inner = &self.inner;
        let queued: Vec<ManifestFile> = inner.state().queue.iter().cloned().collect();
        for file in queued {
            if let Ok(dest) = resolve_inside(&inner.options.install_dir, &file.path)
                && let Ok(metadata) = tokio::fs::metadata(part_path(&dest)).await
                && metadata.len() > 0
                && metadata.len() <= file.size
            {
                inner.state().partial.insert(file.path.clone(), metadata.len());
            }
        }
        let ticker = {
            let inner = inner.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(inner.options.progress_interval);
                loop {
                    interval.tick().await;
                    inner.emit();
                }
            })
        };
        let mut workers = tokio::task::JoinSet::new();
        for _ in 0..inner.options.concurrency {
            workers.spawn(inner.clone().worker());
        }
        while workers.join_next().await.is_some() {}
        ticker.abort();
        inner.emit();
        let state = inner.state();
        if state.cancelled {
            return Err(DownloadError::new(DownloadErrorCode::Cancelled, None, "cancelled"));
        }
        match &state.fatal {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
