import { RemoveTestDb } from "./setup";
import "./authenv";
import { BODY_LOG, RemovePlatformTestDir } from "./platformenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import { logger } from "../src/logger";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { Redact, ResetBodyLogForTests } from "../src/middleware/BodyLog";
import { BodyLogPerPath, DescribeFeatures, ParseChoice, ParseCount, ParseOnOff } from "../src/features";

// Groundwork of the Harmonic port (stage 1): the heartbeat's auth, the body log's status, duration,
// per-path cap and key redaction (ideas from the WireCapture middleware of
// github.com/Harmonicrain/Undaunted 895f7c7, folded into LOG_BODIES) and the switch reader. The app
// listens on a port the system picks.

// The metagame's warnings, as plain text
const Warnings: string[] = [];
const Infos: string[] = [];
const Backup = { warn: logger.warn, info: logger.info };
(logger as any).warn = (Message: unknown) => { Warnings.push(String(Message)); };
(logger as any).info = (Message: unknown) => { Infos.push(String(Message)); };

// An account key as the metagame makes them, built here so no key-shaped text is in this file
const AccountKey = () => "UUK_" + crypto.randomBytes(24).toString("hex");

let Listening: Server | undefined;
let BASE = "";
const Token = SignMetagameJWTForUid("UID-platform-a");

before(async () => {
    // The account behind Token (POST /reconcile reads its notes)
    GetDb().insert(users).values({ userId: "UID-platform-a", name: "PlatformA", notes: 0 }).run();

    Listening = await new Promise<Server>((Resolve, Reject) => {
        const Started = app.listen(0, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
    });
    BASE = `http://127.0.0.1:${(Listening.address() as AddressInfo).port}`;
});

after(async () => {
    Listening?.closeAllConnections();
    await new Promise<void>((Resolve) => Listening ? Listening.close(() => Resolve()) : Resolve());
    Object.assign(logger, Backup);
    RemovePlatformTestDir();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    Warnings.length = 0;
    Infos.length = 0;
    delete process.env.BODY_LOG_PER_PATH;
});

async function Call(Method: string, Path: string, Options: { token?: string, auth?: string, gs?: string, body?: unknown } = {}){
    const Headers: Record<string, string> = {};

    if(Options.token !== undefined) Headers["authorization"] = `bearer ${Options.token}`;
    if(Options.auth !== undefined) Headers["authorization"] = Options.auth;
    if(Options.gs !== undefined) Headers["x-undaunted-gameserver-apikey"] = Options.gs;
    if(Options.body !== undefined) Headers["content-type"] = "application/json";

    const Reply = await fetch(BASE + Path, { method: Method, headers: Headers, body: Options.body === undefined ? undefined : JSON.stringify(Options.body) });

    return { status: Reply.status, type: Reply.headers.get("content-type"), text: await Reply.text() };
}

function BodyLogLines(): any[] {
    if(!fs.existsSync(BODY_LOG)){
        return [];
    }

    return fs.readFileSync(BODY_LOG, "utf8").split("\n").filter((Line) => Line.length > 0).map((Line) => JSON.parse(Line));
}

// The line is written once the answer is done, and the write itself is asynchronous
async function WaitForLines(Match: (Line: any) => boolean, Count: number){
    for(let i = 0; i < 200; i++){
        if(BodyLogLines().filter(Match).length >= Count){
            break;
        }

        await new Promise((Resolve) => setTimeout(Resolve, 5));
    }

    return BodyLogLines().filter(Match);
}

describe("POST /heartbeat keeps its auth", () => {
    // Harmonic's fork answers an unauthenticated heartbeat (HasOptionalUndauntedMetagameAuth). Not ported:
    // in public mode the gateway opens the game ports (UDP 8770-8777) to any address whose heartbeat
    // answers 2xx (UndauntedGateway/src/policy.ts), so a heartbeat without a valid token must stay 401.
    it("a bogus or missing bearer and a wrong game-server key get 401", async () => {
        assert.equal((await Call("POST", "/heartbeat", { auth: "bearer not-a-token", body: { map: "/Game/Maps/ramsgate/ramsgate_01_persistent" } })).status, 401);
        assert.equal((await Call("POST", "/heartbeat", { auth: "Basic dXNlcjpwYXNz", body: {} })).status, 401);
        assert.equal((await Call("POST", "/heartbeat", { body: {} })).status, 401);
        assert.equal((await Call("POST", "/heartbeat", { gs: crypto.randomBytes(24).toString("hex"), body: {} })).status, 401);
    });

    it("a valid token gets 200 and the plain-text 20000", async () => {
        const Beat = await Call("POST", "/heartbeat", { token: Token, body: { map: "/Game/Maps/ramsgate/ramsgate_01_persistent" } });

        assert.equal(Beat.status, 200);
        assert.equal(Beat.text, "20000");
        assert.match(Beat.type ?? "", /^text\/plain/);
    });
});

describe("LOG_BODIES", () => {
    it("records the answer's status and duration with the request", async () => {
        const Before = Date.now();
        const Refused = await Call("POST", "/progression/UID-platform-status", { token: Token, body: { progress_tracks: [] } });
        assert.equal(Refused.status, 403, "a player may not grant progression");

        const [Line] = await WaitForLines((Entry) => Entry.url === "/progression/UID-platform-status", 1);

        assert.ok(Line, "the line was written");
        assert.deepEqual(Object.keys(Line), ["t", "method", "url", "gs", "body", "status", "ms"]);
        assert.equal(Line.method, "POST");
        assert.equal(Line.gs, 0);
        assert.equal(Line.body, JSON.stringify({ progress_tracks: [] }));
        assert.equal(Line.status, 403);
        assert.ok(Number.isInteger(Line.ms) && Line.ms >= 0 && Line.ms < 60000, String(Line.ms));
        assert.ok(Date.parse(Line.t) >= Before - 1000);
    });

    it("also records the store, Slayer Link and reconcile routes, and still leaves other routes out", async () => {
        const Key = crypto.randomBytes(32).toString("hex");

        assert.equal((await Call("GET", "/product/skus/public?requiredTags=Store", { token: Token })).status, 400);
        await Call("GET", "/product/sku/SKU_TEST", { token: Token });
        await Call("GET", "/token/platinum/SKU_TEST", { token: Token });
        await Call("POST", `/notification/platinum?token=${Key}`, { token: Token, body: {} });
        await Call("GET", "/slayerlink/invites", { token: Token });
        await Call("POST", "/reconcile", { token: Token, body: {} });
        await Call("POST", "/trials/leaderboards", { token: Token, body: {difficulty: 1, trial_id: "Arena_MatchmakerHunt_Elite_001"} });
        await Call("POST", "/profile/update", { token: Token, body: {dauntlessid: "UID-platform-a", currentdisplayname: "PlatformA"} });
        await Call("GET", "/playertreatments/UID-platform-a", { token: Token });

        const Lines = await WaitForLines((Entry) => /^\/(product|token|notification|slayerlink|reconcile|trials|profile\/update|playertreatments)/.test(Entry.url), 8);
        const Urls = Lines.map((Entry) => Entry.url);

        assert.deepEqual(Urls.slice().sort(), [
            "/notification/platinum?token=<redacted>",
            "/product/sku/SKU_TEST",
            "/product/skus/public?requiredTags=Store",
            "/profile/update",
            "/reconcile",
            "/slayerlink/invites",
            "/token/platinum/SKU_TEST",
            "/trials/leaderboards"
        ]);
        assert.equal(Lines.find((Entry) => Entry.url.startsWith("/product/skus"))!.status, 400);
        assert.ok(!BodyLogLines().some((Entry) => Entry.url.includes(Key)), "the purchase token is never written");
    });

    it("blanks account keys (UUK_ and 48 hex characters) in the url and the body", async () => {
        const Key = AccountKey();
        assert.equal(Key.length, 52);

        await Call("POST", `/progression/UID-platform-key?key=${Key}`, { token: Token, body: { apiKey: Key, nested: [`x ${Key} y`] } });

        const [Line] = await WaitForLines((Entry) => Entry.url.startsWith("/progression/UID-platform-key"), 1);

        assert.equal(Line.url, "/progression/UID-platform-key?key=<redacted>");
        assert.equal(Line.body, JSON.stringify({ apiKey: "<redacted>", nested: ["x <redacted> y"] }));
        assert.ok(!fs.readFileSync(BODY_LOG, "utf8").includes(Key));
    });

    it("BODY_LOG_PER_PATH caps the lines per method and path; unset (0) is no cap", async () => {
        ResetBodyLogForTests();
        process.env.BODY_LOG_PER_PATH = "2";

        for(let i = 0; i < 4; i++){
            await Call("POST", "/progression/UID-platform-cap", { token: Token, body: { i } });
        }

        await Call("POST", "/progression/UID-platform-cap-other", { token: Token, body: { i: 0 } });
        await WaitForLines((Entry) => Entry.url === "/progression/UID-platform-cap-other", 1);

        const Capped = BodyLogLines().filter((Entry) => Entry.url === "/progression/UID-platform-cap");
        assert.deepEqual(Capped.map((Entry) => JSON.parse(Entry.body).i), [0, 1]);
        assert.ok(Infos.includes("body log: POST /progression/UID-platform-cap reached BODY_LOG_PER_PATH=2; no more lines for it in this run"), Infos.join("\n"));

        delete process.env.BODY_LOG_PER_PATH;
        await Call("POST", "/progression/UID-platform-cap", { token: Token, body: { i: 9 } });

        const Uncapped = await WaitForLines((Entry) => Entry.url === "/progression/UID-platform-cap", 3);
        assert.deepEqual(Uncapped.map((Entry) => JSON.parse(Entry.body).i), [0, 1, 9]);
    });

    it("Redact keeps the old rules for tokens and long runs", () => {
        const Jwt = ["eyJhbGciOiJSUzI1NiJ9", "eyJ1c2VySWQiOiJVSUQtYSJ9", "c2lnbmF0dXJlLW5vdC1yZWFs"].join(".");

        assert.equal(Redact(`/account/api/oauth/sessions/kill/${Jwt}`), "/account/api/oauth/sessions/kill/<token>");
        assert.equal(Redact("x".repeat(64)), "<redacted>");
        assert.equal(Redact("x".repeat(63)), "x".repeat(63));
        assert.equal(Redact("UUK_" + "0".repeat(47)), "UUK_" + "0".repeat(47), "not a whole key");
        assert.equal(Redact("/party/UID-00000000-0000-4000-8000-00000000f609"), "/party/UID-00000000-0000-4000-8000-00000000f609");
    });
});

describe("the switch reader (src/features.ts)", () => {
    it("parses on/off, counts and choices, and refuses anything else", () => {
        for(const On of ["1", "true", "TRUE", "on", "yes"]) assert.equal(ParseOnOff(On), true, On);
        for(const Off of ["0", "false", "Off", "no"]) assert.equal(ParseOnOff(Off), false, Off);
        for(const Bad of ["2", "enabled", ""]) assert.equal(ParseOnOff(Bad), undefined, Bad);

        assert.equal(ParseCount("0"), 0);
        assert.equal(ParseCount("25"), 25);
        for(const Bad of ["-1", "1.5", "ten", "1e3", "9999999999"]) assert.equal(ParseCount(Bad), undefined, Bad);

        const Mode = ParseChoice(["off", "free"] as const);
        assert.equal(Mode("FREE"), "free");
        assert.equal(Mode("priced"), undefined);
    });

    it("uses the default for an unset, empty or invalid value, with one warning per invalid value", () => {
        assert.equal(BodyLogPerPath(), 0);
        process.env.BODY_LOG_PER_PATH = " ";
        assert.equal(BodyLogPerPath(), 0);
        process.env.BODY_LOG_PER_PATH = " 7 ";
        assert.equal(BodyLogPerPath(), 7);

        process.env.BODY_LOG_PER_PATH = "lots";
        assert.equal(BodyLogPerPath(), 0);
        assert.equal(BodyLogPerPath(), 0);
        assert.deepEqual(Warnings, ['BODY_LOG_PER_PATH="lots" is not a valid value; using the default (no-cap)']);
    });

    it("describes every switch in one boot line", () => {
        assert.equal(DescribeFeatures(), "features: bodyLogPerPath=no-cap escalation=stub escalationStrict=off store=off storeRepeatableTokens=off trialsStore=on middlemanStore=on replayWindow=5s confirmEntitlements=off balanceFromInventory=on middlemanFusionGuards=off trialsLeaderboards=on trialsSchedule=off slayerLinks=on chatPresence=on verifyStubAccount=off");
        process.env.BODY_LOG_PER_PATH = "5";
        assert.equal(DescribeFeatures(), "features: bodyLogPerPath=5 escalation=stub escalationStrict=off store=off storeRepeatableTokens=off trialsStore=on middlemanStore=on replayWindow=5s confirmEntitlements=off balanceFromInventory=on middlemanFusionGuards=off trialsLeaderboards=on trialsSchedule=off slayerLinks=on chatPresence=on verifyStubAccount=off");
    });
});
