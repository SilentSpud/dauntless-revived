import { test } from "node:test";
import assert from "node:assert/strict";
import https from "node:https";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { contentDownloadBase, DownloadJob } from "../src/main/downloader";
import { makeTestCert } from "./certs";

test("CDN URLs require HTTPS and contain no credentials or query", () => {
  assert.equal(contentDownloadBase({}), undefined);
  assert.equal(contentDownloadBase({downloadBaseUrl: "https://downloads.example/game"}), "https://downloads.example/game/");
  for (const url of ["http://example/game", "https://u:p@example/game", "https://example/game?q=1", "https://example/game#x", "invalid", 123]) {
    assert.throws(() => contentDownloadBase({downloadBaseUrl: url}));
  }
});

test("CDN resumes without a key or SHA If-Range, accepts R2 ETags, and rejects corrupt bytes", async (t) => {
  const cert = makeTestCert();
  const dir = mkdtempSync(path.join(tmpdir(), "dr-cdn-"));
  const data = Buffer.from("verified CDN game bytes");
  const file = {path: "game.bin", size: data.length, sha256: createHash("sha256").update(data).digest("hex")};
  const seen: {range?: string; key?: string; ifRange?: string}[] = [];
  let corrupt = false;
  const server = https.createServer({key: cert.keyPem, cert: cert.certPem}, (req, res) => {
    seen.push({range: req.headers.range, key: req.headers["x-undaunted-user-api-key"] as string, ifRange: req.headers["if-range"] as string});
    assert.equal(req.url, "/build/game.bin");
    const start = Number(/^bytes=(\d+)-$/.exec(req.headers.range ?? "")?.[1] ?? 0);
    const body = Buffer.from(data.subarray(start));
    if (corrupt) body[0] ^= 1;
    res.writeHead(start ? 206 : 200, {
      "content-length": body.length, etag: '"multipart-md5-2"',
      ...(start ? {"content-range": `bytes ${start}-${data.length - 1}/${data.length}`} : {}),
    });
    res.end(body);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address() as {port: number};
  const opts = {endpoint: {host: "127.0.0.1", port: 1, pin: null}, key: "must-not-reach-CDN", installDir: dir, files: [file], downloadBaseUrl: `https://127.0.0.1:${address.port}/build/`, maxAttempts: 1};
  try {
    // A self-signed CDN is rejected by default, unlike the invite's separately pinned gateway.
    await assert.rejects(new DownloadJob(opts).run());
    assert.equal(seen.length, 0);
    const request = https.request;
    t.mock.method(https, "request", (url: URL, options: https.RequestOptions) => request(url, {...options, ca: cert.certPem}));
    writeFileSync(path.join(dir, "game.bin.part"), data.subarray(0, 5));
    await new DownloadJob(opts).run();
    assert.deepEqual(readFileSync(path.join(dir, "game.bin")), data);
    assert.deepEqual(seen, [{range: "bytes=5-", key: undefined, ifRange: undefined}]);
    rmSync(path.join(dir, "game.bin"));
    corrupt = true;
    await assert.rejects(new DownloadJob(opts).run());
    assert.equal(existsSync(path.join(dir, "game.bin")), false);
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    rmSync(dir, {recursive: true, force: true});
    cert.cleanup();
  }
});
