import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { ChildProcess, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { OpenStalled, Reply, Request, WaitFor } from "../helpers";

// Integration test: the built server (dist/server.js) as its own process on 127.0.0.1:62002, serving a
// real 1.4.4 install read-only, with a mock metagame on 127.0.0.1:62003 that knows one fake key.
//
//   npm run test:integration
//   CONTENT_IT_GAME_DIR   the install to serve (default C:\D144\Dauntless); the test is skipped without it
//   CONTENT_IT_PORT / CONTENT_IT_MOCK_PORT   override 62002 / 62003

const PORT = Number(process.env.CONTENT_IT_PORT || 62002);
const MOCK_PORT = Number(process.env.CONTENT_IT_MOCK_PORT || 62003);
const GAME_DIR = process.env.CONTENT_IT_GAME_DIR || "C:\\D144\\Dauntless";
const ROOT = path.resolve(__dirname, "..", "..", "..");
const SERVER_JS = path.join(ROOT, "dist", "server.js");
const MANIFEST = JSON.parse(fs.readFileSync(path.join(ROOT, "data", "dauntless-1.4.4.json"), "utf8"));
const EXE = "Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe";

// A made-up key for the mock metagame only. It must never show up in the server's output.
const FAKE_KEY = "UUK_integration_" + createHash("sha256").update(String(process.pid) + Date.now()).digest("hex").slice(0, 32);
const ACCOUNT = { UserId: "UID-integration-0001", Username: "it_tester", IsAdmin: false };
const K = { "x-undaunted-user-api-key": FAKE_KEY };

const HasGame = fs.existsSync(path.join(GAME_DIR, ...EXE.split("/")));
const Skip = HasGame ? false : `no 1.4.4 install at ${GAME_DIR}`;

let Mock: http.Server | undefined;
const MockCalls: { good: number; bad: number } = { good: 0, bad: 0 };
let Child: ChildProcess | undefined;
const Output: string[] = [];
let TempDir = "";

function Sha(Data: Buffer){
    return createHash("sha256").update(Data).digest("hex");
}

function Entry(FilePath: string){
    const Found = MANIFEST.files.find((F: { path: string }) => F.path === FilePath);
    assert.ok(Found, `${FilePath} in the manifest`);
    return Found as { path: string; size: number; sha256: string };
}

function ReadSlice(FilePath: string, Start: number, Length: number): Buffer {
    const Fd = fs.openSync(path.join(GAME_DIR, ...FilePath.split("/")), "r");
    try{
        const Buf = Buffer.alloc(Length);
        const Read = fs.readSync(Fd, Buf, 0, Length, Start);
        return Buf.subarray(0, Read);
    }
    finally{
        fs.closeSync(Fd);
    }
}

function LogEntries(): Record<string, unknown>[] {
    return Output.join("").split(/\r?\n/).filter((L) => L.startsWith("{")).map((L) => {
        try{
            return JSON.parse(L);
        }
        catch{
            return {};
        }
    });
}

function PortIsFree(Port: number): Promise<boolean> {
    return new Promise((resolve) => {
        const Socket = net.connect({ host: "127.0.0.1", port: Port });
        Socket.once("connect", () => { Socket.destroy(); resolve(false); });
        Socket.once("error", () => resolve(true));
    });
}

function StartMock(): Promise<http.Server> {
    const Server = http.createServer((req, res) => {
        if(req.method === "GET" && req.url === "/undaunted/api/GetUserInfo" && req.headers["x-undaunted-user-api-key"] === FAKE_KEY){
            MockCalls.good++;
            res.setHeader("Content-Type", "application/json");
            res.end(JSON.stringify(ACCOUNT));
            return;
        }
        MockCalls.bad++;
        res.statusCode = 401;
        res.end();
    });
    return new Promise((resolve, reject) => {
        Server.once("error", reject);
        Server.listen(MOCK_PORT, "127.0.0.1", () => resolve(Server));
    });
}

before(async () => {
    if(Skip){
        return;
    }
    try{
        os.setPriority(os.constants.priority.PRIORITY_LOW);
    }
    catch{
        // not fatal
    }

    assert.ok(fs.existsSync(SERVER_JS), "run the build first (npm run test:integration does)");
    assert.equal(await PortIsFree(PORT), true, `port ${PORT} is free`);
    assert.equal(await PortIsFree(MOCK_PORT), true, `port ${MOCK_PORT} is free`);

    // A synthetic art pack and news file (signature bytes plus filler; no real artwork).
    TempDir = fs.mkdtempSync(path.join(os.tmpdir(), "undaunted-content-it-"));
    const Art = path.join(TempDir, "art");
    fs.mkdirSync(Art);
    fs.writeFileSync(path.join(Art, "one.png"), Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), Buffer.alloc(100, 9)]));
    fs.writeFileSync(path.join(Art, "two.jpg"), Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xe0]), Buffer.alloc(100, 8)]));
    fs.writeFileSync(path.join(Art, "branding.json"), JSON.stringify({ accent: "#3A7BD5", backgrounds: [{ file: "two.jpg", credit: "Test credit" }, "one.png"] }));
    const NewsFile = path.join(TempDir, "news.json");
    fs.writeFileSync(NewsFile, JSON.stringify({ items: [{ date: "2026-09-21", title: "Test night", body: "Bring snacks." }] }));

    Mock = await StartMock();

    const Env: NodeJS.ProcessEnv = { ...process.env };
    for(const Name of Object.keys(Env)){
        if(/^(PORT|BIND_HOST|METAGAME_URL|CONTENT_.*|LOG_LEVEL)$/i.test(Name)){
            delete Env[Name];
        }
    }
    Object.assign(Env, {
        PORT: String(PORT),
        BIND_HOST: "127.0.0.1",
        METAGAME_URL: `http://127.0.0.1:${MOCK_PORT}`,
        CONTENT_GAME_DIR: GAME_DIR,
        CONTENT_BRANDING_DIR: Art,
        CONTENT_NEWS_FILE: NewsFile,
        LOG_LEVEL: "info",
    });

    Child = spawn(process.execPath, [SERVER_JS], { cwd: ROOT, env: Env, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
    try{
        os.setPriority(Child.pid!, os.constants.priority.PRIORITY_LOW);
    }
    catch{
        // not fatal
    }
    Child.stdout!.setEncoding("utf8").on("data", (Text: string) => Output.push(Text));
    Child.stderr!.setEncoding("utf8").on("data", (Text: string) => Output.push(Text));

    let Exited = false;
    Child.once("exit", () => { Exited = true; });
    await WaitFor(() => Exited || Output.join("").includes('"msg":"content server ready"'), 30_000, "the content server to start");
    assert.equal(Exited, false, `server exited during startup:\n${Output.join("")}`);
});

after(async () => {
    if(Child && Child.exitCode === null){
        const Exit = new Promise((resolve) => Child!.once("exit", resolve));
        Child.kill();
        await Exit;
    }
    if(Mock){
        Mock.closeAllConnections();
        await new Promise<void>((resolve) => Mock!.close(() => resolve()));
    }
    if(TempDir){
        fs.rmSync(TempDir, { recursive: true, force: true });
    }
    if(!Skip){
        assert.equal(await PortIsFree(PORT), true, "content server stopped");
        assert.equal(await PortIsFree(MOCK_PORT), true, "mock metagame stopped");
    }
});

test("startup check found the whole install", { skip: Skip }, () => {
    const Check = LogEntries().find((E) => E.msg === "game folder checked (sizes only; npm run verify hashes everything)");
    assert.ok(Check, "startup check logged");
    assert.equal(Check.files, 410);
    assert.equal(Check.ok, 410);
    assert.equal(Check.missing, 0);
    assert.equal(Check.mismatched, 0);
});

test("manifest", { skip: Skip }, async () => {
    const R = await Request(PORT, "GET", "/content/v1/manifest");
    assert.equal(R.status, 200);
    const Body = JSON.parse(R.body.toString("utf8"));
    assert.deepEqual(Body, MANIFEST);
    assert.equal(Body.build, "dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827");
    assert.equal(Body.files.length, 410);
    assert.equal(Body.totalBytes, 10893512875);
});

test("401 without a key or with a wrong one", { skip: Skip }, async () => {
    const Before = MockCalls.bad;
    const None = await Request(PORT, "GET", "/content/v1/files/Version.txt");
    assert.equal(None.status, 401);
    assert.deepEqual(JSON.parse(None.body.toString()), { error: "unauthorized" });
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Version.txt", { "x-undaunted-user-api-key": "UUK_not_a_real_key" })).status, 401);
    assert.equal((await Request(PORT, "GET", "/content/v1/files/Version.txt", { "x-undaunted-user-api-key": "UUK_not_a_real_key" })).status, 401);
    assert.equal(MockCalls.bad - Before, 1, "a refused key is cached too");
    assert.equal((await Request(PORT, "HEAD", `/content/v1/files/${EXE}`)).status, 401);
});

test("small files download whole and hash-match the manifest", { skip: Skip }, async () => {
    for(const FilePath of ["Version.txt", "EasyAntiCheat/Licenses/MIT.txt", "Archon/Binaries/Win64/dbgcore.dll", "Engine/Binaries/Win64/debug.log"]){
        const E = Entry(FilePath);
        const R = await Request(PORT, "GET", `/content/v1/files/${FilePath}`, K);
        assert.equal(R.status, 200, FilePath);
        assert.equal(R.headers["content-type"], "application/octet-stream");
        assert.equal(Number(R.headers["content-length"]), E.size);
        assert.equal(R.headers.etag, `"${E.sha256}"`);
        assert.equal(R.body.length, E.size);
        assert.equal(Sha(R.body), E.sha256, FilePath);
    }
});

test("the exe's first 1 MB as a range, then the next one, then the tail", { skip: Skip }, async () => {
    const E = Entry(EXE);
    const MB = 1024 * 1024;

    const Head = await Request(PORT, "HEAD", `/content/v1/files/${EXE}`, K);
    assert.equal(Head.status, 200);
    assert.equal(Number(Head.headers["content-length"]), 103673520);
    assert.equal(Head.headers["accept-ranges"], "bytes");

    const First = await Request(PORT, "GET", `/content/v1/files/${EXE}`, { ...K, range: `bytes=0-${MB - 1}` });
    assert.equal(First.status, 206);
    assert.equal(First.headers["content-range"], `bytes 0-${MB - 1}/${E.size}`);
    assert.equal(Number(First.headers["content-length"]), MB);
    assert.equal(First.body.length, MB);
    assert.equal(First.body.toString("latin1", 0, 2), "MZ");
    assert.equal(Sha(First.body), Sha(ReadSlice(EXE, 0, MB)));

    const Second = await Request(PORT, "GET", `/content/v1/files/${EXE}`, { ...K, range: `bytes=${MB}-${2 * MB - 1}`, "if-range": `"${E.sha256}"` });
    assert.equal(Second.status, 206);
    assert.equal(Sha(Second.body), Sha(ReadSlice(EXE, MB, MB)));

    const Tail = await Request(PORT, "GET", `/content/v1/files/${EXE}`, { ...K, range: "bytes=-4096" });
    assert.equal(Tail.status, 206);
    assert.equal(Tail.headers["content-range"], `bytes ${E.size - 4096}-${E.size - 1}/${E.size}`);
    assert.ok(Tail.body.equals(ReadSlice(EXE, E.size - 4096, 4096)));

    for(const Bad of [`bytes=${E.size}-`, "bytes=0-1,10-20", "bytes=9-3"]){
        const R = await Request(PORT, "GET", `/content/v1/files/${EXE}`, { ...K, range: Bad });
        assert.equal(R.status, 416, Bad);
        assert.equal(R.headers["content-range"], `bytes */${E.size}`);
    }
});

test("traversal and lookalike paths are all 400 or 404", { skip: Skip }, async () => {
    const Attempts = [
        "/content/v1/files/../../../../Windows/win.ini",
        "/content/v1/files/../Dauntless/Version.txt",
        "/content/v1/files/Archon/../Version.txt",
        "/content/v1/files/..\\..\\Windows\\win.ini",
        "/content/v1/files/..%2f..%2fWindows%2fwin.ini",
        "/content/v1/files/..%2F..%2FWindows%2Fwin.ini",
        "/content/v1/files/%2e%2e/%2e%2e/Windows/win.ini",
        "/content/v1/files/%2e%2e%2f%2e%2e%2fWindows%2fwin.ini",
        "/content/v1/files/Archon%5c..%5c..%5cWindows%5cwin.ini",
        "/content/v1/files/%252e%252e/%252e%252e/Windows/win.ini",
        "/content/v1/files/%c0%ae%c0%ae/Windows/win.ini",
        "/content/v1/files/%u002e%u002e/Windows/win.ini",
        "/content/v1/files/....//....//Windows/win.ini",
        "/content/v1/files/C:/Windows/win.ini",
        "/content/v1/files/C:%5cWindows%5cwin.ini",
        "/content/v1/files//server/share/x",
        "/content/v1/files/./Version.txt",
        "/content/v1/files/Version.txt/.",
        "/content/v1/files/Version.txt%00.png",
        "/content/v1/files/Version.txt::$DATA",
        `/content/v1/files/${EXE}:Zone.Identifier`,
        "/content/v1/files/Version.txt%20",
        "/content/v1/files/Version.txt.",
        "/content/v1/files/VERSION.TXT",
        "/content/v1/files/Archon",
        "/content/v1/files/Archon/Binaries/Win64/",
        "/content/v1/files/Archon/Binaries/Win64/dxgi.dll",
        "/content/v1/files/Archon/Binaries/Win64/UndauntedInternalServer.dll",
        "/content/v1/files/",
        "/content/v1/files",
        "/content/v1/manifest/../files/Version.txt",
        "/content/v1/branding/../../../../Windows/win.ini",
        "/content/v1/branding/..%2f..%2fwin.ini",
        "/content/v1/",
        "/content/",
        "/",
    ];

    const Results: [string, number][] = [];
    for(const Target of Attempts){
        const R: Reply = await Request(PORT, "GET", Target, K);
        Results.push([Target, R.status]);
    }
    const Wrong = Results.filter(([, Status]) => Status !== 400 && Status !== 404);
    assert.deepEqual(Wrong, [], "every attempt answered 400 or 404");
});

test("at most 6 concurrent streams per account", { skip: Skip }, async () => {
    const Streams = [];
    for(let I = 0; I < 6; I++){
        const S = await OpenStalled(PORT, `/content/v1/files/${EXE}`, K);
        assert.equal(S.status, 200, `stream ${I + 1}`);
        Streams.push(S);
    }
    try{
        const Seventh = await Request(PORT, "GET", "/content/v1/files/Version.txt", K);
        assert.equal(Seventh.status, 429);
        assert.equal(Seventh.headers["retry-after"], "2");
        assert.deepEqual(JSON.parse(Seventh.body.toString()), { error: "too_many_streams" });

        assert.equal((await Request(PORT, "HEAD", "/content/v1/files/Version.txt", K)).status, 200, "HEAD takes no slot");

        Streams.shift()!.destroy();
        let Status = 0;
        await WaitFor(async () => {
            Status = (await Request(PORT, "GET", "/content/v1/files/Version.txt", K)).status;
            return Status === 200;
        }, 10_000, "a slot to free up after a client hung up");
        assert.equal(Status, 200);
    }
    finally{
        for(const S of Streams){
            S.destroy();
        }
    }
    await WaitFor(async () => {
        const Burst = await Promise.all(Array.from({ length: 6 }, () => Request(PORT, "GET", "/content/v1/files/Version.txt", K)));
        return Burst.every((R) => R.status === 200);
    }, 10_000, "all slots to free up");
});

test("branding and news", { skip: Skip }, async () => {
    const B = await Request(PORT, "GET", "/content/v1/branding");
    assert.equal(B.status, 200);
    assert.deepEqual(JSON.parse(B.body.toString()), {
        backgrounds: [
            { url: "/content/v1/branding/two.jpg", credit: "Test credit" },
            { url: "/content/v1/branding/one.png", credit: null },
        ],
        accent: "#3a7bd5",
    });
    const Img = await Request(PORT, "GET", "/content/v1/branding/two.jpg");
    assert.equal(Img.status, 200);
    assert.equal(Img.headers["content-type"], "image/jpeg");
    assert.equal(Img.body.length, 104);
    assert.equal((await Request(PORT, "GET", "/content/v1/branding/branding.json")).status, 404);
    assert.equal((await Request(PORT, "GET", "/content/v1/branding/missing.png")).status, 404);

    const N = await Request(PORT, "GET", "/content/v1/news");
    assert.equal(N.status, 200);
    assert.deepEqual(JSON.parse(N.body.toString()), { items: [{ date: "2026-09-21T00:00:00.000Z", title: "Test night", body: "Bring snacks." }] });
});

test("the metagame was asked about the good key once (5-minute cache)", { skip: Skip }, () => {
    assert.equal(MockCalls.good, 1);
});

test("with the metagame gone, cached accounts keep working and new keys get 503", { skip: Skip }, async () => {
    Mock!.closeAllConnections();
    await new Promise<void>((resolve) => Mock!.close(() => resolve()));
    Mock = undefined;

    assert.equal((await Request(PORT, "GET", "/content/v1/files/Version.txt", K)).status, 200);
    const New = await Request(PORT, "GET", "/content/v1/files/Version.txt", { "x-undaunted-user-api-key": "UUK_some_other_key_1234" });
    assert.equal(New.status, 503);
    assert.deepEqual(JSON.parse(New.body.toString()), { error: "auth_unavailable" });
});

test("the log has account, path and bytes for downloads and never the key", { skip: Skip }, async () => {
    await WaitFor(() => LogEntries().some((E) => E.msg === "download" && E.path === "Archon/Binaries/Win64/dbgcore.dll"), 5000, "download log");
    const Entries = LogEntries();
    const Dll = Entries.find((E) => E.msg === "download" && E.path === "Archon/Binaries/Win64/dbgcore.dll" && E.status === 200);
    assert.ok(Dll);
    assert.equal(Dll.account, ACCOUNT.UserId);
    assert.equal(Dll.bytes, 166720);
    assert.equal(Dll.complete, true);

    const Ranged = Entries.find((E) => E.msg === "download" && E.path === EXE && E.status === 206 && E.range === "bytes=0-1048575");
    assert.ok(Ranged);
    assert.equal(Ranged.bytes, 1048576);

    const Refused = Entries.filter((E) => E.route === "files" && E.status === 400);
    assert.ok(Refused.length >= 10, "refused paths are logged");

    const All = Output.join("");
    assert.equal(All.includes(FAKE_KEY), false, "the key leaked into the log");
    assert.equal(All.includes("UUK_not_a_real_key"), false, "a wrong key leaked into the log");
});
