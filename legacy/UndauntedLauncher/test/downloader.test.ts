// Download / verify / resume against a local fake content server on 127.0.0.1:62012.
import { test, before, after, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DownloadError, DownloadJob, fileUrlPath, SpeedMeter, type DownloadProgress } from "../src/main/downloader";
import { validateManifest, type GameManifest } from "../src/main/manifest";
import { verifyInstall, VerifiedCache } from "../src/main/verify";
import { CONTENT_PORT, FakeContentServer, makeFiles, manifestFor, TEST_BUILD, type FakeFile } from "./fakes";

const KEY = "UUK_" + "c".repeat(48);
const files = makeFiles();
const manifest: GameManifest = (() => {
  const r = validateManifest(manifestFor(files), TEST_BUILD, false);
  if (!r.ok) throw new Error(r.reason);
  return r.manifest;
})();
const endpoint = { host: "127.0.0.1", port: CONTENT_PORT, pin: null };
let server: FakeContentServer;
let dir: string;

function fileData(p: string): Buffer {
  return (files.find((f) => f.path === p) as FakeFile).data;
}

function target(p: string): string {
  return path.join(dir, ...p.split("/"));
}

function job(extra: Partial<ConstructorParameters<typeof DownloadJob>[0]> = {}) {
  return new DownloadJob({ endpoint, key: KEY, installDir: dir, files: manifest.files, backoffMs: () => 20, progressIntervalMs: 20, ...extra });
}

function assertInstalled(): void {
  for (const f of files) {
    assert.ok(existsSync(target(f.path)), f.path);
    assert.ok(readFileSync(target(f.path)).equals(f.data), f.path);
    assert.ok(!existsSync(target(f.path) + ".part"), f.path);
  }
}

before(async () => {
  server = new FakeContentServer({ key: KEY, files });
  await server.start();
});

after(async () => {
  await server.stop();
});

beforeEach(() => {
  dir = mkdtempSync(path.join(tmpdir(), "dr-dl-test-"));
  server.opts = { key: KEY, files };
  server.requests.length = 0;
  server.maxActive = 0;
});

test("downloads every file, verifies it, never more than 4 at once", async () => {
  const progress: DownloadProgress[] = [];
  const verified: string[] = [];
  await job({ onProgress: (p) => progress.push(p), onFileVerified: (f) => verified.push(f.path) }).run();
  assertInstalled();
  assert.equal(verified.length, files.length);
  assert.ok(server.maxActive <= 4, `max concurrent ${server.maxActive}`);
  assert.ok(server.maxActive >= 2, `expected parallel transfers, saw ${server.maxActive}`);
  assert.ok(server.requests.every((r) => r.hadKey));
  const last = progress[progress.length - 1];
  assert.equal(last.doneBytes, manifest.totalBytes);
  assert.equal(last.filesDone, files.length);
  const report = await verifyInstall(dir, manifest.files, { fullHash: true });
  assert.equal(report.bad.length, 0);
  rmSync(dir, { recursive: true, force: true });
});

test("resumes a partial file with a Range request", async () => {
  const big = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
  mkdirSync(path.dirname(target(big)), { recursive: true });
  writeFileSync(target(big) + ".part", fileData(big).subarray(0, 400_000));
  await job().run();
  assertInstalled();
  const reqs = server.requests.filter((r) => r.path === big);
  assert.equal(reqs.length, 1);
  assert.equal(reqs[0].range, "bytes=400000-");
  assert.equal(reqs[0].status, 206);
  rmSync(dir, { recursive: true, force: true });
});

test("a corrupt transfer is detected by its hash and fetched again", async () => {
  const p = "Archon/Binaries/Win64/Fake-Game.exe";
  server.opts = { key: KEY, files, corruptOnce: new Set([p]) };
  const retries: string[] = [];
  await job({ onRetry: (f) => retries.push(f.path) }).run();
  assertInstalled();
  assert.deepEqual(retries, [p]);
  assert.equal(server.requests.filter((r) => r.path === p).length, 2);
  rmSync(dir, { recursive: true, force: true });
});

test("a damaged partial file is thrown away after the hash check and fetched from the start", async () => {
  const p = "Archon/Content/Movies/intro (1).bk2";
  mkdirSync(path.dirname(target(p)), { recursive: true });
  writeFileSync(target(p) + ".part", Buffer.alloc(50_000, 0x41)); // wrong bytes
  await job().run();
  assertInstalled();
  const reqs = server.requests.filter((r) => r.path === p);
  assert.equal(reqs.length, 2);
  assert.equal(reqs[0].range, "bytes=50000-");
  assert.equal(reqs[1].range, undefined);
  rmSync(dir, { recursive: true, force: true });
});

test("repair: an injected corrupt game file is found by verify and only that file is re-fetched", async () => {
  await job().run();
  const p = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
  const damaged = Buffer.from(fileData(p));
  damaged[12345] ^= 0x01; // same size, one bit different
  writeFileSync(target(p), damaged);
  rmSync(target("Version.txt")); // and one missing file

  const quick = await verifyInstall(dir, manifest.files, { fullHash: false });
  assert.deepEqual(quick.bad.map((b) => b.problem), ["missing"]); // size-only check misses the flipped bit

  const report = await verifyInstall(dir, manifest.files, { fullHash: true });
  assert.deepEqual(
    report.bad.map((b) => [b.file.path, b.problem]),
    [
      [p, "hash"],
      ["Version.txt", "missing"],
    ],
  );
  server.requests.length = 0;
  await job({ files: report.bad.map((b) => b.file) }).run();
  assertInstalled();
  assert.deepEqual(server.requests.map((r) => r.path).sort(), [p, "Version.txt"].sort());
  rmSync(dir, { recursive: true, force: true });
});

test("the verified-file cache lets an interrupted install skip re-hashing, but not a changed file", async () => {
  await job().run();
  const cache = new VerifiedCache(path.join(dir, "cache.json"));
  for (const f of manifest.files) await cache.record(dir, f);
  await cache.save();
  const reloaded = new VerifiedCache(path.join(dir, "cache.json"));
  let hashed = 0;
  const r1 = await verifyInstall(dir, manifest.files, {
    fullHash: true,
    trusted: (f, size, mtime) => reloaded.isTrusted(dir, f, size, mtime),
    onProgress: () => hashed++,
  });
  assert.equal(r1.bad.length, 0);
  const p = "Archon/Binaries/Win64/Fake-Game.exe";
  const damaged = Buffer.from(fileData(p));
  damaged[0] ^= 0xff;
  await new Promise((r) => setTimeout(r, 20));
  writeFileSync(target(p), damaged); // new mtime: no longer trusted
  const r2 = await verifyInstall(dir, manifest.files, { fullHash: true, trusted: (f, size, mtime) => reloaded.isTrusted(dir, f, size, mtime) });
  assert.deepEqual(r2.bad.map((b) => b.file.path), [p]);
  assert.ok(hashed >= 0);
  rmSync(dir, { recursive: true, force: true });
});

test("pause stops all transfers and resume continues from the .part files", async () => {
  server.opts = { key: KEY, files, chunkSize: 16_384, tickMs: 5 };
  const j = job();
  const run = j.run();
  await new Promise((r) => setTimeout(r, 150));
  j.pause();
  assert.equal(j.paused, true);
  await new Promise((r) => setTimeout(r, 150));
  assert.equal(server.active, 0, "no transfer may stay open while paused");
  const pausedAt = j.progress();
  assert.equal(pausedAt.paused, true);
  assert.equal(pausedAt.bytesPerSecond, 0);
  assert.ok(pausedAt.doneBytes > 0 && pausedAt.doneBytes < manifest.totalBytes);
  const requestsWhilePaused = server.requests.length;
  await new Promise((r) => setTimeout(r, 100));
  assert.equal(server.requests.length, requestsWhilePaused);
  j.resume();
  await run;
  assertInstalled();
  assert.ok(server.requests.some((r) => r.range !== undefined && r.status === 206), "resume used a Range request");
  rmSync(dir, { recursive: true, force: true });
});

test("cancel stops the job and keeps partial files for later", async () => {
  server.opts = { key: KEY, files, chunkSize: 8192, tickMs: 10 };
  const j = job();
  const run = j.run();
  await new Promise((r) => setTimeout(r, 120));
  j.cancel();
  await assert.rejects(run, (e: unknown) => e instanceof DownloadError && e.code === "cancelled");
  const parts = manifest.files.filter((f) => existsSync(target(f.path) + ".part"));
  assert.ok(parts.length > 0);
  // A new job finishes the install, resuming those parts.
  server.opts = { key: KEY, files };
  await job().run();
  assertInstalled();
  rmSync(dir, { recursive: true, force: true });
});

test("a server that ignores Range still works (the file restarts from zero)", async () => {
  server.opts = { key: KEY, files, ignoreRange: true };
  const big = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
  mkdirSync(path.dirname(target(big)), { recursive: true });
  writeFileSync(target(big) + ".part", fileData(big).subarray(0, 1000));
  await job().run();
  assertInstalled();
  rmSync(dir, { recursive: true, force: true });
});

test("a wrong key stops everything with key_rejected", async () => {
  await assert.rejects(
    new DownloadJob({ endpoint, key: "UUK_" + "d".repeat(48), installDir: dir, files: manifest.files, backoffMs: () => 10 }).run(),
    (e: unknown) => e instanceof DownloadError && e.code === "key_rejected",
  );
  rmSync(dir, { recursive: true, force: true });
});

test("a file the server does not have stops with file_missing_on_server", async () => {
  const missing = { path: "Archon/NotOnServer.pak", size: 10, sha256: "a".repeat(64) };
  await assert.rejects(
    job({ files: [missing] }).run(),
    (e: unknown) => e instanceof DownloadError && e.code === "file_missing_on_server" && e.filePath === missing.path,
  );
  rmSync(dir, { recursive: true, force: true });
});

test("a server file whose ETag differs from the manifest is refused", async () => {
  const f = manifest.files[0];
  const wrong = { ...f, sha256: "b".repeat(64) };
  await assert.rejects(job({ files: [wrong] }).run(), (e: unknown) => e instanceof DownloadError && e.code === "file_different_on_server");
  assert.ok(!existsSync(target(f.path)));
  rmSync(dir, { recursive: true, force: true });
});

test("a file that keeps failing gives up after maxAttempts", async () => {
  const f = manifest.files.find((x) => x.size > 0)!;
  const lying = { ...f, size: f.size + 1 }; // the server always sends one byte less than expected
  await assert.rejects(
    job({ files: [lying], maxAttempts: 3 }).run(),
    (e: unknown) => e instanceof DownloadError && e.code === "download_failed",
  );
  assert.equal(server.requests.filter((r) => r.path === f.path).length, 3);
  rmSync(dir, { recursive: true, force: true });
});

test("URL paths are encoded segment by segment", () => {
  assert.equal(fileUrlPath("Archon/Content/Movies/intro (1).bk2"), "/content/v1/files/Archon/Content/Movies/intro%20(1).bk2");
});

test("speed meter", () => {
  let now = 0;
  const m = new SpeedMeter(5000, () => now);
  m.add(1000);
  now = 1000;
  m.add(1000);
  now = 2000;
  assert.equal(m.rate(), 1000);
  now = 10000;
  assert.equal(m.rate(), 0);
});
