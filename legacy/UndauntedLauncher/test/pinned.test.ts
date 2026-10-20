// Public mode without the relay: ServerStatus, Register, GetUserInfo, the content probe and the
// downloads all go straight to the gateway over TLS pinned to the invite's fingerprint.
// Ports: 62420 fake metagame (TLS), 62421 fake content server (TLS), 62422 an older metagame
// (plain HTTP on loopback).

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { CertificatePinError, pinnedConnect } from "../src/main/pinned";
import { request, HttpError, type Endpoint } from "../src/main/http";
import { fetchServerStatus, fetchUserInfo, probeContent, registerAccount } from "../src/main/hostapi";
import { DownloadError, DownloadJob } from "../src/main/downloader";
import { validateManifest, type GameManifest } from "../src/main/manifest";
import { FakeContentServer, FakeMetagame, makeFiles, manifestFor, TEST_BUILD } from "./fakes";
import { makeTestCert, type TestCert } from "./certs";

const META_PORT = 62420;
const CONTENT_TLS_PORT = 62421;
const LEGACY_PORT = 62422;
const KEY = "UUK_" + "e".repeat(48);

let cert: TestCert;
let other: TestCert;
let meta: FakeMetagame;
let content: FakeContentServer;
const files = makeFiles();
const manifest: GameManifest = (() => {
  const r = validateManifest(manifestFor(files), TEST_BUILD, false);
  if (!r.ok) throw new Error(r.reason);
  return r.manifest;
})();

const metaEp = (): Endpoint => ({ host: "127.0.0.1", port: META_PORT, pin: cert.fingerprint });
const contentEp = (pin?: string): Endpoint => ({ host: "127.0.0.1", port: CONTENT_TLS_PORT, pin: pin ?? cert.fingerprint });

before(async () => {
  cert = makeTestCert();
  other = makeTestCert();
  meta = new FakeMetagame({ validCodes: new Set(["GOOD-CODE"]), existingUsers: new Map([["aurora", KEY]]) });
  await meta.start(META_PORT, { cert: cert.certPem, key: cert.keyPem });
  content = new FakeContentServer({ key: KEY, files });
  await content.start(CONTENT_TLS_PORT, { cert: cert.certPem, key: cert.keyPem });
});

after(async () => {
  await meta.stop();
  await content.stop();
  cert.cleanup();
  other.cleanup();
});

test("pinnedConnect accepts the pinned certificate and refuses any other", async () => {
  const sock = await pinnedConnect({ host: "127.0.0.1", port: META_PORT, fingerprint: cert.fingerprint });
  assert.equal(sock.encrypted, true);
  sock.destroy();
  await assert.rejects(
    pinnedConnect({ host: "127.0.0.1", port: META_PORT, fingerprint: other.fingerprint }),
    (e: unknown) => e instanceof CertificatePinError && e.actual === cert.fingerprint,
  );
});

test("plain requests over the pin; a mismatch is an HttpError of kind pin", async () => {
  const res = await request(metaEp(), "/dauntless-status", { timeoutMs: 5000, maxBytes: 65536 });
  assert.equal(res.status, 200);
  await assert.rejects(
    request({ ...metaEp(), pin: other.fingerprint }, "/dauntless-status", { timeoutMs: 5000, maxBytes: 65536 }),
    (e: unknown) => e instanceof HttpError && e.kind === "pin",
  );
  // A plain-HTTP request to a TLS port is not mistaken for a pin failure.
  await assert.rejects(request({ ...metaEp(), pin: null }, "/dauntless-status", { timeoutMs: 3000, maxBytes: 65536 }), (e: unknown) => e instanceof HttpError && e.kind !== "pin");
});

test("ServerStatus: ok over the pin, cert_mismatch with another fingerprint", async () => {
  const ok = await fetchServerStatus(metaEp());
  assert.equal(ok.kind, "ok");
  if (ok.kind === "ok") assert.equal(ok.status.name, "Test Server");
  const bad = await fetchServerStatus({ ...metaEp(), pin: other.fingerprint });
  assert.equal(bad.kind, "cert_mismatch");
  const down = await fetchServerStatus({ host: "127.0.0.1", port: 62429, pin: cert.fingerprint }, { timeoutMs: 2000 });
  assert.equal(down.kind, "unreachable");
});

test("ServerStatus: the account key goes along over the pin (full list), never to a mismatching server, never in a malformed shape", async () => {
  const start = meta.statusCalls.length;
  const anonymous = await fetchServerStatus(metaEp());
  assert.ok(anonymous.kind === "ok" && anonymous.status.limited === true && anonymous.status.players.length === 0);

  const full = await fetchServerStatus(metaEp(), { key: KEY });
  assert.ok(full.kind === "ok");
  if (full.kind === "ok") {
    assert.equal(full.status.limited, false);
    assert.deepEqual(full.status.players.map((p) => p.name), ["Aurora", "Borealis"]);
    assert.equal(full.status.instances.length, 2);
  }

  // A key the server does not know: the limited status, not an error.
  const unknown = await fetchServerStatus(metaEp(), { key: "UUK_" + "1".repeat(48) });
  assert.ok(unknown.kind === "ok" && unknown.status.limited === true);

  // Not a plausible key: not sent at all.
  const junk = await fetchServerStatus(metaEp(), { key: "has space\r\nx-evil: 1" });
  assert.ok(junk.kind === "ok" && junk.status.limited === true);

  assert.deepEqual(meta.statusCalls.slice(start), [
    { withKey: false, registered: false },
    { withKey: true, registered: true },
    { withKey: true, registered: false },
    { withKey: false, registered: false },
  ]);

  // Another certificate: refused before any request, so the key never left.
  const before = meta.statusCalls.length;
  assert.deepEqual(await fetchServerStatus({ ...metaEp(), pin: other.fingerprint }, { key: KEY }), { kind: "cert_mismatch" });
  assert.equal(meta.statusCalls.length, before);

  // Plain HTTP to a public address is refused before connecting (a public server is only ever pinned TLS).
  assert.deepEqual(await fetchServerStatus({ host: "203.0.113.10", port: META_PORT, pin: null }, { key: KEY, timeoutMs: 2000 }), { kind: "unreachable" });
});

test("ServerStatus: an older server (404) is recognised through /dauntless-status, which never gets the key", async () => {
  const seen: { url: string; key: unknown }[] = [];
  const legacy = http.createServer((req, res) => {
    seen.push({ url: req.url ?? "", key: req.headers["x-undaunted-user-api-key"] });
    if (req.url === "/dauntless-status") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ "show-status": true }));
      return;
    }
    res.writeHead(404);
    res.end();
  });
  await new Promise<void>((resolve) => legacy.listen(LEGACY_PORT, "127.0.0.1", resolve));
  try {
    const r = await fetchServerStatus({ host: "127.0.0.1", port: LEGACY_PORT, pin: null }, { key: KEY });
    assert.deepEqual(r, { kind: "unsupported" });
    assert.deepEqual(seen, [
      { url: "/undaunted/api/ServerStatus", key: KEY },
      { url: "/dauntless-status", key: undefined },
    ]);
  } finally {
    legacy.closeAllConnections();
    await new Promise<void>((resolve) => legacy.close(() => resolve()));
  }
});

test("Register and GetUserInfo over the pin; nothing is sent to a mismatching server", async () => {
  const before = meta.registrations.length;
  const refused = await registerAccount({ ...metaEp(), pin: other.fingerprint }, "Borealis", "GOOD-CODE");
  assert.deepEqual(refused, { ok: false, error: "cert_mismatch" });
  assert.equal(meta.registrations.length, before, "the mismatching server never saw the registration");
  const r = await registerAccount(metaEp(), "Borealis", "GOOD-CODE");
  assert.equal(r.ok, true);
  if (!r.ok) return;
  const info = await fetchUserInfo(metaEp(), r.key);
  assert.equal(info.ok, true);
  if (info.ok) assert.equal(info.info.username, "Borealis");
  const wrong = await fetchUserInfo({ ...metaEp(), pin: other.fingerprint }, r.key);
  assert.deepEqual(wrong, { ok: false, error: "cert_mismatch" });
});

test("content probe over the pin", async () => {
  assert.equal(await probeContent(contentEp()), true);
  assert.equal(await probeContent(contentEp(other.fingerprint)), false);
});

test("downloads over the pin: full install, resume with Range, then verify", async () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dr-dl-tls-"));
  try {
    content.requests.length = 0;
    const big = "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak";
    const data = files.find((f) => f.path === big)!.data;
    const target = path.join(dir, ...big.split("/"));
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target + ".part", data.subarray(0, 123_456));
    await new DownloadJob({ endpoint: contentEp(), key: KEY, installDir: dir, files: manifest.files, backoffMs: () => 20 }).run();
    for (const f of files) {
      const p = path.join(dir, ...f.path.split("/"));
      assert.ok(readFileSync(p).equals(f.data), f.path);
      assert.ok(!existsSync(p + ".part"));
    }
    const resumed = content.requests.find((r) => r.path === big);
    assert.equal(resumed?.range, "bytes=123456-");
    assert.equal(resumed?.status, 206);
    assert.ok(content.requests.every((r) => r.hadKey));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("downloads refuse a server whose certificate does not match, before any request", async () => {
  const dir = mkdtempSync(path.join(tmpdir(), "dr-dl-tls-"));
  try {
    content.requests.length = 0;
    await assert.rejects(
      new DownloadJob({ endpoint: contentEp(other.fingerprint), key: KEY, installDir: dir, files: manifest.files, backoffMs: () => 10 }).run(),
      (e: unknown) => e instanceof DownloadError && e.code === "cert_mismatch",
    );
    assert.equal(content.requests.length, 0, "the key never reached the server");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
