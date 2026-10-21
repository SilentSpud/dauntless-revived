//! Download / verify / resume against a local fake content server, as `test/downloader.test.ts` and
//! `test/cdn-download.test.ts`.

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};

use http_body_util::StreamBody;
use hyper::body::{Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use super::*;
use crate::net::verify::{FileProblem, VerifiedCache, VerifyOptions, verify_install};

const KEY: &str = "UUK_0123456789abcdef0123456789abcdef0123456789abcdef";

fn pattern(n: usize, seed: usize) -> Bytes {
    Bytes::from((0..n).map(|i| ((i * 31 + seed * 7) & 0xff) as u8).collect::<Vec<_>>())
}

fn files() -> Vec<(String, Bytes)> {
    vec![
        ("Archon/Binaries/Win64/Fake-Game.exe".into(), pattern(300_000, 1)),
        (
            "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak".into(),
            pattern(900_000, 2),
        ),
        (
            "Engine/Config/BaseEngine.ini".into(),
            Bytes::from_static(b"[Core.System]\r\nPaths=../../../Engine/Content\r\n"),
        ),
        ("Version.txt".into(), Bytes::from_static(b"dauntless_rel-1.4.4")),
        ("Archon/Content/Movies/intro (1).bk2".into(), pattern(120_000, 3)),
        ("Engine/Binaries/Win64/empty.dat".into(), Bytes::new()),
    ]
}

fn sha(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn manifest() -> Vec<ManifestFile> {
    let mut list: Vec<ManifestFile> = files()
        .into_iter()
        .map(|(path, data)| ManifestFile {
            path,
            size: data.len() as u64,
            sha256: sha(&data),
        })
        .collect();
    list.sort_by(|a, b| a.path.cmp(&b.path));
    list
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Log {
    path: String,
    range: Option<String>,
    had_key: bool,
    status: u16,
}

#[derive(Default, Clone)]
struct Behaviour {
    corrupt_once: HashSet<String>,
    ignore_range: bool,
    /// Throttling: bytes per tick, and the tick
    chunk: Option<(usize, Duration)>,
}

struct Fake {
    data: HashMap<String, Bytes>,
    behaviour: Mutex<Behaviour>,
    corrupted: Mutex<HashSet<String>>,
    requests: Mutex<Vec<Log>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

type FrameResult = Result<Frame<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

impl Fake {
    fn handle(
        self: &Arc<Self>,
        request: Request<Incoming>,
    ) -> Response<http_body_util::combinators::BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>> {
        let empty = |status: u16| {
            let mut response = Response::new(Empty::new().map_err(|never| match never {}).boxed());
            *response.status_mut() = StatusCode::from_u16(status).unwrap();
            response
        };
        let Some(encoded) = request.uri().path().strip_prefix("/content/v1/files/") else {
            return empty(404);
        };
        let relative: String = encoded
            .split('/')
            .map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let key = request
            .headers()
            .get("x-undaunted-user-api-key")
            .and_then(|v| v.to_str().ok())
            == Some(KEY);
        let range = request
            .headers()
            .get("range")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut log = Log {
            path: relative.clone(),
            range: range.clone(),
            had_key: key,
            status: 0,
        };
        let finish = |log: Log, status: u16| {
            self.requests.lock().unwrap().push(Log { status, ..log });
        };
        if !key {
            finish(log, 401);
            return empty(401);
        }
        let Some(data) = self.data.get(&relative).cloned() else {
            finish(log, 404);
            return empty(404);
        };
        let behaviour = self.behaviour.lock().unwrap().clone();
        let mut body = data.to_vec();
        if behaviour.corrupt_once.contains(&relative) && self.corrupted.lock().unwrap().insert(relative.clone()) {
            for i in (0..body.len()).step_by(997) {
                body[i] ^= 0xff;
            }
        }
        let mut status = 200;
        let mut start = 0;
        let mut response = Response::builder()
            .header("etag", format!("\"{}\"", sha(&data)))
            .header("accept-ranges", "bytes");
        if let Some(from) = range
            .as_deref()
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.strip_suffix('-'))
            .and_then(|n| n.parse::<usize>().ok())
            && !behaviour.ignore_range
        {
            if from >= data.len() {
                finish(log, 416);
                return empty(416);
            }
            start = from;
            status = 206;
            response = response.header(
                "content-range",
                format!("bytes {}-{}/{}", from, data.len() - 1, data.len()),
            );
        }
        let slice = Bytes::from(body).slice(start..);
        log.status = status;
        self.requests.lock().unwrap().push(log);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::mpsc::channel::<FrameResult>(2);
        let fake = self.clone();
        tokio::spawn(async move {
            match behaviour.chunk {
                // A short delay keeps several transfers open at once, so the concurrency limit shows
                None => {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let _ = tx.send(Ok(Frame::data(slice))).await;
                }
                Some((size, tick)) => {
                    let mut at = 0;
                    while at < slice.len() {
                        let end = (at + size).min(slice.len());
                        if tx.send(Ok(Frame::data(slice.slice(at..end)))).await.is_err() {
                            break;
                        }
                        at = end;
                        tokio::time::sleep(tick).await;
                    }
                }
            }
            fake.active.fetch_sub(1, Ordering::SeqCst);
        });
        let length = slice_len_header(&data, start);
        response
            .status(status)
            .header("content-length", length)
            .body(StreamBody::new(tokio_stream::wrappers::ReceiverStream::new(rx)).boxed())
            .unwrap()
    }
}

fn slice_len_header(data: &Bytes, start: usize) -> String {
    (data.len() - start).to_string()
}

async fn fake(behaviour: Behaviour) -> (Arc<Fake>, Endpoint) {
    let fake = Arc::new(Fake {
        data: files().into_iter().collect(),
        behaviour: Mutex::new(behaviour),
        corrupted: Mutex::new(HashSet::new()),
        requests: Mutex::new(Vec::new()),
        active: AtomicUsize::new(0),
        max_active: AtomicUsize::new(0),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = fake.clone();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let server = server.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| {
                    let server = server.clone();
                    async move { Ok::<_, Infallible>(server.handle(request)) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tcp), service)
                    .await;
            });
        }
    });
    (
        fake,
        Endpoint {
            host: "127.0.0.1".into(),
            port,
            pin: None,
        },
    )
}

fn options(endpoint: &Endpoint, dir: &Path, files: Vec<ManifestFile>) -> DownloadOptions {
    let mut options = DownloadOptions::new(endpoint.clone(), KEY.into(), dir.to_path_buf(), files);
    options.backoff = Arc::new(|_| Duration::from_millis(20));
    options.progress_interval = Duration::from_millis(20);
    options
}

fn assert_installed(dir: &Path) {
    for (path, data) in files() {
        let target = resolve_inside(dir, &path).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), data, "{path}");
        assert!(!part_path(&target).exists(), "{path}");
    }
}

fn data_of(path: &str) -> Bytes {
    files().into_iter().find(|(p, _)| p == path).unwrap().1
}

fn write_part(dir: &Path, path: &str, bytes: &[u8]) {
    let target = resolve_inside(dir, path).unwrap();
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(part_path(&target), bytes).unwrap();
}

#[tokio::test]
async fn downloads_every_file_verifies_it_never_more_than_4_at_once() {
    let (fake, endpoint) = fake(Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let verified = Arc::new(Mutex::new(Vec::new()));
    let mut options = options(&endpoint, dir.path(), manifest());
    let (seen, done) = (progress.clone(), verified.clone());
    options.on_progress = Some(Arc::new(move |p| seen.lock().unwrap().push(p)));
    options.on_file_verified = Some(Arc::new(move |f| done.lock().unwrap().push(f.path.clone())));
    DownloadJob::new(options).unwrap().run().await.unwrap();
    assert_installed(dir.path());
    assert_eq!(verified.lock().unwrap().len(), files().len());
    let max = fake.max_active.load(Ordering::SeqCst);
    assert!((2..=4).contains(&max), "max concurrent {max}");
    assert!(fake.requests.lock().unwrap().iter().all(|r| r.had_key));
    let last = progress.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        (last.done_bytes, last.files_done),
        (manifest().iter().map(|f| f.size).sum(), files().len())
    );
    let report = verify_install(
        dir.path(),
        &manifest(),
        VerifyOptions {
            full_hash: true,
            ..VerifyOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(report.bad.is_empty());
}

#[tokio::test]
async fn resumes_a_partial_file_with_a_range_request() {
    let (fake, endpoint) = fake(Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let big = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
    write_part(dir.path(), big, &data_of(big)[..400_000]);
    DownloadJob::new(options(&endpoint, dir.path(), manifest()))
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_installed(dir.path());
    let requests: Vec<Log> = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.path == big)
        .cloned()
        .collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        (requests[0].range.as_deref(), requests[0].status),
        (Some("bytes=400000-"), 206)
    );
}

#[tokio::test]
async fn a_corrupt_transfer_and_a_damaged_part_are_fetched_again() {
    let exe = "Archon/Binaries/Win64/Fake-Game.exe";
    let movie = "Archon/Content/Movies/intro (1).bk2";
    let (fake, endpoint) = fake(Behaviour {
        corrupt_once: HashSet::from([exe.to_owned()]),
        ..Behaviour::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    write_part(dir.path(), movie, &vec![0x41; 50_000]);
    let retries = Arc::new(Mutex::new(Vec::new()));
    let mut options = options(&endpoint, dir.path(), manifest());
    let seen = retries.clone();
    options.on_retry = Some(Arc::new(move |f, _, _| seen.lock().unwrap().push(f.path.clone())));
    DownloadJob::new(options).unwrap().run().await.unwrap();
    assert_installed(dir.path());
    let mut retried = retries.lock().unwrap().clone();
    retried.sort();
    assert_eq!(retried, vec![exe.to_owned(), movie.to_owned()]);
    let requests = fake.requests.lock().unwrap().clone();
    assert_eq!(requests.iter().filter(|r| r.path == exe).count(), 2);
    let movie_requests: Vec<Option<String>> = requests
        .iter()
        .filter(|r| r.path == movie)
        .map(|r| r.range.clone())
        .collect();
    assert_eq!(movie_requests, vec![Some("bytes=50000-".into()), None]);
}

#[tokio::test]
async fn repair_finds_a_corrupt_and_a_missing_file_and_fetches_only_those() {
    let (fake, endpoint) = fake(Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    DownloadJob::new(options(&endpoint, dir.path(), manifest()))
        .unwrap()
        .run()
        .await
        .unwrap();
    let pak = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
    let mut damaged = data_of(pak).to_vec();
    damaged[12345] ^= 0x01;
    std::fs::write(resolve_inside(dir.path(), pak).unwrap(), &damaged).unwrap();
    std::fs::remove_file(resolve_inside(dir.path(), "Version.txt").unwrap()).unwrap();

    let quick = verify_install(dir.path(), &manifest(), VerifyOptions::default())
        .await
        .unwrap();
    assert_eq!(
        quick.bad.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
        vec![FileProblem::Missing],
        "a size check misses the flipped bit"
    );
    let report = verify_install(
        dir.path(),
        &manifest(),
        VerifyOptions {
            full_hash: true,
            ..VerifyOptions::default()
        },
    )
    .await
    .unwrap();
    let bad: Vec<(String, FileProblem)> = report.bad.iter().map(|(f, p)| (f.path.clone(), *p)).collect();
    assert_eq!(
        bad,
        vec![
            (pak.to_owned(), FileProblem::Hash),
            ("Version.txt".to_owned(), FileProblem::Missing)
        ]
    );
    fake.requests.lock().unwrap().clear();
    let again: Vec<ManifestFile> = report.bad.iter().map(|(f, _)| f.clone()).collect();
    DownloadJob::new(options(&endpoint, dir.path(), again))
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_installed(dir.path());
    let mut fetched: Vec<String> = fake.requests.lock().unwrap().iter().map(|r| r.path.clone()).collect();
    fetched.sort();
    assert_eq!(fetched, vec![pak.to_owned(), "Version.txt".to_owned()]);
}

#[tokio::test]
async fn the_verified_cache_skips_re_hashing_but_not_a_changed_file() {
    let (_fake, endpoint) = fake(Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    DownloadJob::new(options(&endpoint, dir.path(), manifest()))
        .unwrap()
        .run()
        .await
        .unwrap();
    let cache_file = dir.path().join("cache.json");
    let mut cache = VerifiedCache::load(cache_file.clone());
    for file in manifest() {
        cache.record(dir.path(), &file).await;
    }
    cache.save().await.unwrap();
    let reloaded = Arc::new(VerifiedCache::load(cache_file));
    let root = dir.path().to_string_lossy().into_owned();
    let trusted: crate::net::verify::Trusted = {
        let (reloaded, root) = (reloaded.clone(), root.clone());
        Arc::new(move |f, size, mtime| reloaded.is_trusted(&root, f, size, mtime))
    };
    let hashed = Arc::new(Mutex::new(0u64));
    let counter = hashed.clone();
    let first = verify_install(
        dir.path(),
        &manifest(),
        VerifyOptions {
            full_hash: true,
            trusted: Some(trusted.clone()),
            on_progress: Some(Arc::new(move |_| *counter.lock().unwrap() += 1)),
            ..VerifyOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(first.bad.is_empty());
    let exe = "Archon/Binaries/Win64/Fake-Game.exe";
    let mut damaged = data_of(exe).to_vec();
    damaged[0] ^= 0xff;
    tokio::time::sleep(Duration::from_millis(20)).await;
    std::fs::write(resolve_inside(dir.path(), exe).unwrap(), damaged).unwrap();
    let second = verify_install(
        dir.path(),
        &manifest(),
        VerifyOptions {
            full_hash: true,
            trusted: Some(trusted),
            ..VerifyOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        second.bad.iter().map(|(f, _)| f.path.clone()).collect::<Vec<_>>(),
        vec![exe.to_owned()]
    );
}

#[tokio::test]
async fn pause_stops_every_transfer_and_resume_continues_from_the_parts() {
    let (fake, endpoint) = fake(Behaviour {
        chunk: Some((16_384, Duration::from_millis(5))),
        ..Behaviour::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let job = DownloadJob::new(options(&endpoint, dir.path(), manifest())).unwrap();
    let run = tokio::spawn({
        let job = job.clone();
        async move { job.run().await }
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    job.pause();
    assert!(job.paused());
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        fake.active.load(Ordering::SeqCst),
        0,
        "no transfer may stay open while paused"
    );
    let paused = job.progress();
    assert!(paused.paused);
    assert_eq!(paused.bytes_per_second, 0.0);
    let total: u64 = manifest().iter().map(|f| f.size).sum();
    assert!(
        paused.done_bytes > 0 && paused.done_bytes < total,
        "{}",
        paused.done_bytes
    );
    let before = fake.requests.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fake.requests.lock().unwrap().len(), before);
    job.resume();
    run.await.unwrap().unwrap();
    assert_installed(dir.path());
    assert!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.range.is_some() && r.status == 206),
        "resume used a Range request"
    );
}

#[tokio::test]
async fn cancel_stops_the_job_and_keeps_partial_files() {
    let (fake, endpoint) = fake(Behaviour {
        chunk: Some((8192, Duration::from_millis(10))),
        ..Behaviour::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let job = DownloadJob::new(options(&endpoint, dir.path(), manifest())).unwrap();
    let run = tokio::spawn({
        let job = job.clone();
        async move { job.run().await }
    });
    tokio::time::sleep(Duration::from_millis(120)).await;
    job.cancel();
    assert_eq!(run.await.unwrap().unwrap_err().code, DownloadErrorCode::Cancelled);
    assert!(
        manifest()
            .iter()
            .any(|f| part_path(&resolve_inside(dir.path(), &f.path).unwrap()).exists())
    );
    *fake.behaviour.lock().unwrap() = Behaviour::default();
    DownloadJob::new(options(&endpoint, dir.path(), manifest()))
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_installed(dir.path());
}

#[tokio::test]
async fn a_server_that_ignores_range_still_works() {
    let (_fake, endpoint) = fake(Behaviour {
        ignore_range: true,
        ..Behaviour::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let big = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
    write_part(dir.path(), big, &data_of(big)[..1000]);
    DownloadJob::new(options(&endpoint, dir.path(), manifest()))
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_installed(dir.path());
}

#[tokio::test]
async fn refusals_stop_the_job() {
    let (fake, endpoint) = fake(Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let mut wrong_key = options(&endpoint, dir.path(), manifest());
    wrong_key.key = format!("UUK_{}", "d".repeat(48));
    assert_eq!(
        DownloadJob::new(wrong_key).unwrap().run().await.unwrap_err().code,
        DownloadErrorCode::KeyRejected
    );

    let missing = ManifestFile {
        path: "Archon/NotOnServer.pak".into(),
        size: 10,
        sha256: "a".repeat(64),
    };
    let error = DownloadJob::new(options(&endpoint, dir.path(), vec![missing.clone()]))
        .unwrap()
        .run()
        .await
        .unwrap_err();
    assert_eq!(
        (error.code, error.file.as_deref()),
        (DownloadErrorCode::FileMissingOnServer, Some(missing.path.as_str()))
    );

    let first = manifest()[0].clone();
    let different = ManifestFile {
        sha256: "b".repeat(64),
        ..first.clone()
    };
    let error = DownloadJob::new(options(&endpoint, dir.path(), vec![different]))
        .unwrap()
        .run()
        .await
        .unwrap_err();
    assert_eq!(error.code, DownloadErrorCode::FileDifferentOnServer);
    assert!(!resolve_inside(dir.path(), &first.path).unwrap().exists());

    // The server always sends one byte less than expected
    let real = manifest().into_iter().find(|f| f.size > 0).unwrap();
    let lying = ManifestFile {
        size: real.size + 1,
        ..real.clone()
    };
    fake.requests.lock().unwrap().clear();
    let mut few = options(&endpoint, dir.path(), vec![lying]);
    few.max_attempts = 3;
    assert_eq!(
        DownloadJob::new(few).unwrap().run().await.unwrap_err().code,
        DownloadErrorCode::DownloadFailed
    );
    assert_eq!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == real.path)
            .count(),
        3
    );
}

#[test]
fn small_rules() {
    assert_eq!(
        file_url_path("Archon/Content/Movies/intro (1).bk2"),
        "/content/v1/files/Archon/Content/Movies/intro%20(1).bk2"
    );
    let start = Instant::now();
    let mut meter = SpeedMeter::new(Duration::from_secs(5));
    meter.add_at(start, 1000);
    meter.add_at(start + Duration::from_secs(1), 1000);
    assert_eq!(meter.rate_at(start + Duration::from_secs(2)), 1000.0);
    assert_eq!(meter.rate_at(start + Duration::from_secs(10)), 0.0);
    assert_eq!(content_download_base(None), Ok(None));
    assert_eq!(
        content_download_base(Some("https://downloads.example/game")),
        Ok(Some("https://downloads.example/game/".into()))
    );
    for bad in [
        "http://example/game",
        "https://u:p@example/game",
        "https://example/game?q=1",
        "https://example/game#x",
        "invalid",
    ] {
        assert!(content_download_base(Some(bad)).is_err(), "{bad}");
    }
    assert_eq!(retry_after(Some("5")), Duration::from_secs(5));
    assert_eq!(retry_after(Some("120")), Duration::from_secs(30));
    assert_eq!(retry_after(Some("0.2")), Duration::from_secs(1));
    assert_eq!(retry_after(Some("soon")), Duration::from_secs(2));
    assert_eq!(retry_after(None), Duration::from_secs(2));
    assert_eq!(parse_content_range(Some("bytes 5-9/10")), Some((5, 9, 10)));
    assert_eq!(parse_content_range(Some("bytes */10")), None);
    assert_eq!(etag_hash(Some("W/\"ABC\"")).as_deref(), Some("abc"));
}

#[tokio::test]
async fn the_cdn_resumes_without_the_key_and_refuses_corrupt_bytes() {
    use crate::net::pinned::tests::{acceptor, certificate};
    let (cert, key) = certificate();
    let tls = acceptor(cert.clone(), key);
    let data = Bytes::from_static(b"verified CDN game bytes");
    let file = ManifestFile {
        path: "game.bin".into(),
        size: data.len() as u64,
        sha256: sha(&data),
    };
    let seen = Arc::new(Mutex::new(
        Vec::<(Option<String>, Option<String>, Option<String>)>::new(),
    ));
    let corrupt = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    {
        let (seen, corrupt, data) = (seen.clone(), corrupt.clone(), data.clone());
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                let (tls, seen, corrupt, data) = (tls.clone(), seen.clone(), corrupt.clone(), data.clone());
                tokio::spawn(async move {
                    let Ok(stream) = tls.accept(tcp).await else { return };
                    let service = service_fn(move |request: Request<Incoming>| {
                        let (seen, corrupt, data) = (seen.clone(), corrupt.clone(), data.clone());
                        async move {
                            let header = |name: &str| {
                                request
                                    .headers()
                                    .get(name)
                                    .and_then(|v| v.to_str().ok())
                                    .map(str::to_owned)
                            };
                            seen.lock().unwrap().push((
                                header("range"),
                                header("x-undaunted-user-api-key"),
                                header("if-range"),
                            ));
                            assert_eq!(request.uri().path(), "/build/game.bin");
                            let start = header("range")
                                .and_then(|r| {
                                    r.strip_prefix("bytes=")
                                        .and_then(|r| r.strip_suffix('-'))
                                        .and_then(|n| n.parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            let mut body = data[start..].to_vec();
                            if corrupt.load(Ordering::SeqCst) {
                                body[0] ^= 1;
                            }
                            let mut response = Response::builder()
                                .status(if start > 0 { 206 } else { 200 })
                                .header("etag", "\"multipart-md5-2\"");
                            if start > 0 {
                                response = response.header(
                                    "content-range",
                                    format!("bytes {start}-{}/{}", data.len() - 1, data.len()),
                                );
                            }
                            Ok::<_, Infallible>(response.body(http_body_util::Full::new(Bytes::from(body))).unwrap())
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
    }
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint {
        host: "127.0.0.1".into(),
        port: 1,
        pin: None,
    };
    let mut cdn = DownloadOptions::new(
        endpoint,
        "must-not-reach-CDN".into(),
        dir.path().to_path_buf(),
        vec![file.clone()],
    );
    cdn.download_base_url = Some(format!("https://127.0.0.1:{port}/build/"));
    cdn.max_attempts = 1;
    // A self-signed CDN is refused by default, unlike the invite's separately pinned gateway
    assert!(DownloadJob::new(cdn.clone()).unwrap().run().await.is_err());
    assert!(seen.lock().unwrap().is_empty());
    cdn.cdn_roots = vec![cert];
    write_part(dir.path(), "game.bin", &data[..5]);
    DownloadJob::new(cdn.clone()).unwrap().run().await.unwrap();
    assert_eq!(std::fs::read(dir.path().join("game.bin")).unwrap(), data);
    assert_eq!(
        seen.lock().unwrap().clone(),
        vec![(Some("bytes=5-".into()), None, None)]
    );
    std::fs::remove_file(dir.path().join("game.bin")).unwrap();
    corrupt.store(true, Ordering::SeqCst);
    assert!(DownloadJob::new(cdn).unwrap().run().await.is_err());
    assert!(!dir.path().join("game.bin").exists());
}
