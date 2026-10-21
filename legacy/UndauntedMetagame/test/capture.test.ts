import { RemoveTestDb } from "./setup";
import "./authenv";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import { after, before, describe, it } from "node:test";
import { app } from "../src/app";
import { CaptureWriter, RedactChatFrame } from "../src/capture";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { GetDb } from "../src/db";
import { gameserverapikeys, users } from "../src/db/schema";
import { SetCaptureForTests } from "../src/middleware/Capture";
import { ChatServer } from "../src/realtime/chat";
import { Login } from "./chatwire";

// Traffic capture at the metagame (MIGRATION_PLAN.md M2): requests that do not pass the gateway, and
// chat, recorded with no token or key in them. The app is served in process (not through appclient:
// the capture exists only in the Node metagame, whose corpus the replay compares).

const A = "UID-capture-meta-a";
const GS_KEY = crypto.randomBytes(24).toString("hex");
const GATEWAY_SECRET = "capture-test-gateway-secret-0123456789abcdef";
const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "dr-capture-"));
let Writer: CaptureWriter;
let Listening: Server;
let Base = "";
let Chat: ChatServer;

function Lines(): any[] {
    return fs.readdirSync(Dir).flatMap((File) => fs.readFileSync(path.join(Dir, File), "utf8").trim().split("\n").filter(Boolean).map((Line) => JSON.parse(Line)));
}

before(async () => {
    process.env.GATEWAY_SECRET = GATEWAY_SECRET;
    GetDb().insert(users).values({ userId: A, name: "Alpha", notes: 0, isAdmin: false }).run();
    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY).digest("hex") }).run();
    Writer = new CaptureWriter({ dir: Dir, retentionDays: 14, maxBodyBytes: 65536, source: "metagame" });
    Writer.AddSecret(GATEWAY_SECRET);
    SetCaptureForTests(Writer);
    Listening = await new Promise<Server>((resolve) => { const S = app.listen(0, "127.0.0.1", () => resolve(S)); });
    Base = `http://127.0.0.1:${(Listening.address() as AddressInfo).port}`;
    Chat = new ChatServer({ AutoTick: false });
    await Chat.listen(0);
});

after(async () => {
    await Chat.close();
    await new Promise<void>((resolve) => Listening.close(() => resolve()));
    SetCaptureForTests(undefined);
    delete process.env.GATEWAY_SECRET;
    fs.rmSync(Dir, { recursive: true, force: true });
    RemoveTestDb(() => GetDb().$client.close());
});

describe("metagame capture", () => {
    it("records a game server's call and a player's, but not what the gateway forwarded", async () => {
        const Token = SignMetagameJWTForUid(A);
        const Game = await fetch(`${Base}/undaunted/api/ServerStatus`, { headers: { "x-undaunted-gameserver-apikey": GS_KEY } });
        assert.equal(Game.status, 200);
        await Game.text();
        const Player = await fetch(`${Base}/account/api/public/account/${A}`, { headers: { authorization: `bearer ${Token}` } });
        await Player.text();
        const Forwarded = await fetch(`${Base}/account/api/public/account/${A}`, { headers: { authorization: `bearer ${Token}`, "x-dauntless-gateway": GATEWAY_SECRET, "x-forwarded-for": "203.0.113.9" } });
        await Forwarded.text();
        await new Promise((resolve) => setTimeout(resolve, 100));
        await Writer.Close();
        Writer = new CaptureWriter(Writer.Options);
        Writer.AddSecret(GATEWAY_SECRET);
        SetCaptureForTests(Writer);

        const Http = Lines().filter((Line) => Line.kind === "http");
        assert.equal(Http.length, 2, JSON.stringify(Http));
        const [First, Second] = Http;
        assert.equal(First.target, "/undaunted/api/ServerStatus");
        assert.equal(First.reqHeaders["x-undaunted-gameserver-apikey"], "<gskey>");
        assert.equal(First.status, 200);
        assert.ok(First.resBody.json !== undefined);
        assert.equal(Second.reqHeaders.authorization, `bearer <token:${A}>`);
        assert.equal(Second.resBody.json.displayName, "Alpha");
        const Text = fs.readdirSync(Dir).map((File) => fs.readFileSync(path.join(Dir, File), "utf8")).join("");
        assert.ok(!Text.includes(Token) && !Text.includes(GS_KEY) && !Text.includes(GATEWAY_SECRET));
    });

    it("records chat in order, the login as the account only", async () => {
        const Before = Lines().length;
        const Wire = await Login(Chat.port, A);
        Wire.Send(`<iq type="get" id="ping-capture"><ping xmlns="urn:xmpp:ping"/></iq>`);
        await Wire.Expect("pong");
        await Wire.Logout();
        await new Promise((resolve) => setTimeout(resolve, 100));
        await Writer.Close();
        const Chats = Lines().slice(Before).filter((Line) => Line.kind === "chat");
        assert.equal(Chats[0].event, "open");
        assert.ok(Chats.some((Line) => Line.event === "close" && Line.conn === Chats[0].conn), "the session's end");
        const Auth = Chats.find((Line) => Line.dir === "in" && Line.text.includes("<auth"));
        assert.match(Auth.text, new RegExp(`>\\<plain:${A}>\\</auth>$`.replace(/\\</g, "<")));
        assert.ok(Chats.some((Line) => Line.dir === "out" && Line.text.includes('id="ping-capture"')));
        const Text = JSON.stringify(Chats);
        assert.ok(!/eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\./.test(Text), "no token");
        assert.equal(RedactChatFrame(`<auth mechanism="PLAIN">${Buffer.from(`\0${A}\0secret`).toString("base64")}</auth>`), `<auth mechanism="PLAIN"><plain:${A}></auth>`);
        assert.equal(RedactChatFrame("<query><password>hunter2</password></query>"), "<query><password><redacted></password></query>");
    });
});
