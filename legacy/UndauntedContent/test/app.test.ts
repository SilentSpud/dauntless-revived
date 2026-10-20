import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { CreateContentHandler } from "../src/app";
import { AuthCache, LookupResult } from "../src/auth";
import { Branding } from "../src/branding";
import { CheckGameDir, FileState } from "../src/gamedir";
import { StreamLimiter } from "../src/limits";
import { SetLogSink } from "../src/log";
import { IndexManifest, Manifest, ValidateManifest } from "../src/manifest";
import { News } from "../src/news";
import { OpenStalled, Request, WaitFor } from "./helpers";

// The request handler against a small synthetic game folder, on test port 62012.
const PORT = 62012;
const GOOD = "UUK_app_test_good_key_0123456789";
const OTHER = "UUK_app_test_other_key_987654321";
const DOWN = "UUK_app_test_metagame_down_00000";

let Dir: string;
let Server: http.Server;
let Limiter: StreamLimiter;
let States: Map<string, FileState>;
let TheManifest: Manifest;
let Lookups = 0;
const LogLines: string[] = [];

function Sha(Data: Buffer){
    return createHash("sha256").update(Data).digest("hex");
}

const Files: Record<string, Buffer> = {};

before(async () => {
    Dir = fs.mkdtempSync(path.join(os.tmpdir(), "undaunted-content-app-"));
    fs.mkdirSync(path.join(Dir, "Data"));
    Files["Version.txt"] = Buffer.from("test-build \r\n");
    Files["Data/empty.log"] = Buffer.alloc(0);
    Files["Data/small.bin"] = Buffer.from(Array.from({ length: 5000 }, (_, I) => (I * 7) & 0xff));
    // Big enough that a stalled client keeps the server's stream open (socket buffers fill up).
    const Pattern = Buffer.from(Array.from({ length: 65536 }, (_, I) => (I * 31 + 7) & 0xff));
    Files["Data/big.bin"] = Buffer.concat(Array.from({ length: 768 }, () => Pattern));
    Files["Data/changes.bin"] = Buffer.from("will be touched");
    Files["Data/gone.bin"] = Buffer.from("will be deleted");
    for(const [Rel, Data] of Object.entries(Files)){
        fs.writeFileSync(path.join(Dir, ...Rel.split("/")), Data);
    }
    fs.writeFileSync(path.join(Dir, "Data", "not-in-manifest.bin"), "secret");

    const Sorted = Object.keys(Files).sort((A, B) => (A < B ? -1 : A > B ? 1 : 0));
    TheManifest = ValidateManifest({
        build: "test",
        totalBytes: Sorted.reduce((S, P) => S + Files[P].length, 0),
        files: Sorted.map((P) => ({ path: P, size: Files[P].length, sha256: Sha(Files[P]) })),
    });
    fs.rmSync(path.join(Dir, "Data", "gone.bin"));
    States = CheckGameDir(Dir, TheManifest).states;
    Limiter = new StreamLimiter(2, 3);

    SetLogSink((Line) => LogLines.push(Line));

    const Handler = CreateContentHandler({
        manifest: TheManifest,
        index: IndexManifest(TheManifest),
        gameDir: Dir,
        states: States,
        auth: new AuthCache({
            lookup: async (Key): Promise<LookupResult> => {
                Lookups++;
                if(Key === GOOD) return { status: "ok", account: { UserId: "UID-A", Username: "alpha", IsAdmin: false } };
                if(Key === OTHER) return { status: "ok", account: { UserId: "UID-B", Username: "beta", IsAdmin: false } };
                if(Key === DOWN) return { status: "unavailable", reason: "metagame unreachable" };
                return { status: "invalid" };
            },
        }),
        limiter: Limiter,
        branding: new Branding(undefined),
        news: new News(undefined),
    });
    Server = http.createServer(Handler);
    await new Promise<void>((resolve) => Server.listen(PORT, "127.0.0.1", resolve));
});

after(async () => {
    Server.closeAllConnections();
    await new Promise<void>((resolve) => Server.close(() => resolve()));
    SetLogSink(undefined);
    fs.rmSync(Dir, { recursive: true, force: true });
});

const K = { "x-undaunted-user-api-key": GOOD };

test("manifest: GET, HEAD and a conditional GET", async () => {
    const R = await Request(PORT, "GET", "/content/v1/manifest");
    assert.equal(R.status, 200);
    assert.match(String(R.headers["content-type"]), /application\/json/);
    assert.deepEqual(JSON.parse(R.body.toString()), TheManifest);
    const Head = await Request(PORT, "HEAD", "/content/v1/manifest");
    assert.equal(Head.status, 200);
    assert.equal(Head.body.length, 0);
    const Again = await Request(PORT, "GET", "/content/v1/manifest", { "if-none-match": String(R.headers.etag) });
    assert.equal(Again.status, 304);
});

test("unknown routes and methods", async () => {
    assert.equal((await Request(PORT, "GET", "/")).status, 404);
    assert.equal((await Request(PORT, "GET", "/content/v1/")).status, 404);
    assert.equal((await Request(PORT, "GET", "/content/v1/files")).status, 404);
    const Post = await Request(PORT, "POST", "/content/v1/files/Version.txt", K);
    assert.equal(Post.status, 405);
    assert.equal(Post.headers.allow, "GET, HEAD");
    assert.equal((await Request(PORT, "DELETE", "/content/v1/manifest")).status, 405);
});

test("files need a valid key", async () => {
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Version.txt")).status, 401);
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Version.txt", { "x-undaunted-user-api-key": "UUK_wrong" })).status, 401);
    assert.equal((await Request(PORT, "HEAD", "/content/v1/files/Version.txt")).status, 401);
    const Down = await Request(PORT, "GET", "/content/v1/files/Version.txt", { "x-undaunted-user-api-key": DOWN });
    assert.equal(Down.status, 503);
    assert.deepEqual(JSON.parse(Down.body.toString()), { error: "auth_unavailable" });
});

test("a whole file, with the right headers", async () => {
    const R = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", K);
    assert.equal(R.status, 200);
    assert.equal(R.headers["content-type"], "application/octet-stream");
    assert.equal(R.headers["content-length"], "5000");
    assert.equal(R.headers["accept-ranges"], "bytes");
    assert.equal(R.headers.etag, `"${Sha(Files["Data/small.bin"])}"`);
    assert.equal(R.headers["x-content-type-options"], "nosniff");
    assert.equal(Sha(R.body), Sha(Files["Data/small.bin"]));

    const Head = await Request(PORT, "HEAD", "/content/v1/files/Data/small.bin", K);
    assert.equal(Head.status, 200);
    assert.equal(Head.headers["content-length"], "5000");
    assert.equal(Head.body.length, 0);
});

test("ranges", async () => {
    const Data = Files["Data/small.bin"];
    const R = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, range: "bytes=100-199" });
    assert.equal(R.status, 206);
    assert.equal(R.headers["content-range"], "bytes 100-199/5000");
    assert.equal(R.headers["content-length"], "100");
    assert.ok(R.body.equals(Data.subarray(100, 200)));

    const Tail = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, range: "bytes=-10" });
    assert.ok(Tail.body.equals(Data.subarray(4990)));

    const Open = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, range: "bytes=4000-" });
    assert.equal(Open.headers["content-range"], "bytes 4000-4999/5000");
    assert.ok(Open.body.equals(Data.subarray(4000)));

    for(const Bad of ["bytes=5000-", "bytes=10-5", "bytes=0-1,4-5", "bytes=x", "items=0-1"]){
        const B = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, range: Bad });
        assert.equal(B.status, 416, Bad);
        assert.equal(B.headers["content-range"], "bytes */5000", Bad);
    }

    const Resume = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, range: "bytes=0-9", "if-range": `"${Sha(Data)}"` });
    assert.equal(Resume.status, 206);
    const Stale = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, range: "bytes=0-9", "if-range": '"something-else"' });
    assert.equal(Stale.status, 200);
    assert.equal(Stale.body.length, 5000);

    const NotModified = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { ...K, "if-none-match": `"${Sha(Data)}"` });
    assert.equal(NotModified.status, 304);
    assert.equal(NotModified.body.length, 0);
});

test("zero-byte file", async () => {
    const R = await Request(PORT, "GET", "/content/v1/files/Data/empty.log", K);
    assert.equal(R.status, 200);
    assert.equal(R.headers["content-length"], "0");
    assert.equal(R.body.length, 0);
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Data/empty.log", { ...K, range: "bytes=0-" })).status, 416);
});

test("only manifest paths, exactly", async () => {
    for(const [Target, Want] of [
        ["/content/v1/files/Data/not-in-manifest.bin", 404],
        ["/content/v1/files/version.txt", 404],
        ["/content/v1/files/Data", 404],
        ["/content/v1/files/Version.txt.", 404],
        ["/content/v1/files/../Data/small.bin", 400],
        ["/content/v1/files/Data/../Version.txt", 400],
        ["/content/v1/files/Data%2fsmall.bin", 400],
        ["/content/v1/files/Data%5csmall.bin", 400],
        ["/content/v1/files/%2e%2e/x", 400],
        ["/content/v1/files/Data\\small.bin", 400],
        ["/content/v1/files/Version.txt%00", 400],
        ["/content/v1/files/Version.txt::$DATA", 400],
    ] as [string, number][]){
        const R = await Request(PORT, "GET", Target, K);
        assert.equal(R.status, Want, Target);
    }
});

test("files missing or changed on disk are not served", async () => {
    const Gone = await Request(PORT, "GET", "/content/v1/files/Data/gone.bin", K);
    assert.equal(Gone.status, 503);
    assert.deepEqual(JSON.parse(Gone.body.toString()), { error: "file_unavailable" });

    assert.equal((await Request(PORT, "GET", "/content/v1/files/Data/changes.bin", K)).status, 200);
    const Later = new Date(Date.now() + 60_000);
    fs.utimesSync(path.join(Dir, "Data", "changes.bin"), Later, Later);
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Data/changes.bin", K)).status, 503);
    assert.equal(States.get("Data/changes.bin")?.status, "changed");
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Data/changes.bin", K)).status, 503);
});

test("per-account and server-wide stream limits", async () => {
    const A1 = await OpenStalled(PORT, "/content/v1/files/Data/big.bin", K);
    const A2 = await OpenStalled(PORT, "/content/v1/files/Data/big.bin", K);
    assert.equal(A1.status, 200);
    assert.equal(A2.status, 200);
    assert.equal(Limiter.active("UID-A"), 2);

    const A3 = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", K);
    assert.equal(A3.status, 429);
    assert.equal(A3.headers["retry-after"], "2");
    assert.deepEqual(JSON.parse(A3.body.toString()), { error: "too_many_streams" });

    // HEAD doesn't take a slot.
    assert.equal((await Request(PORT, "HEAD", "/content/v1/files/Data/small.bin", K)).status, 200);

    const B1 = await OpenStalled(PORT, "/content/v1/files/Data/big.bin", { "x-undaunted-user-api-key": OTHER });
    assert.equal(B1.status, 200);
    const B2 = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", { "x-undaunted-user-api-key": OTHER });
    assert.equal(B2.status, 503, "server-wide limit of 3 reached");
    assert.deepEqual(JSON.parse(B2.body.toString()), { error: "server_busy" });

    A1.destroy();
    await WaitFor(() => Limiter.active("UID-A") === 1, 5000, "slot release");
    const A4 = await Request(PORT, "GET", "/content/v1/files/Data/small.bin", K);
    assert.equal(A4.status, 200);

    A2.destroy();
    B1.destroy();
    await WaitFor(() => Limiter.active() === 0, 5000, "all slots released");
});

test("the log names the account and bytes, never the key", async () => {
    await Request(PORT, "GET", "/content/v1/files/Data/small.bin", K);
    await WaitFor(() => LogLines.some((L) => L.includes('"msg":"download"') && L.includes("Data/small.bin")), 2000, "download log");

    const Download = LogLines.map((L) => JSON.parse(L)).find((E) => E.msg === "download" && E.path === "Data/small.bin" && E.status === 200);
    assert.ok(Download);
    assert.equal(Download.account, "UID-A");
    assert.equal(Download.user, "alpha");
    assert.equal(Download.bytes, 5000);
    assert.equal(Download.complete, true);

    const All = LogLines.join("\n");
    for(const Key of [GOOD, OTHER, DOWN, "UUK_wrong"]){
        assert.equal(All.includes(Key), false, "a key leaked into the log");
    }
    assert.ok(Lookups <= 5, `lookups are cached (${Lookups})`);
});
