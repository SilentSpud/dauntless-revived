import { RemoveTestDb } from "./setup";
import "./authenv";
import { PROGRESSION_DEFAULT_API_PORT } from "./progressiondefaultenv";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import { type Server } from "node:http";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { gameserverapikeys } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { MakePlayer } from "./helpers";

// What a server with no progression settings answers, over HTTP. Real progression is the
// default: a new account starts at 0 and keeps what it earns, owns the Elite Hunt Pass,
// and its claims are stored. Upstream's stub (PROGRESSION_MODE=stub) answered 99,999,999
// everywhere, no entitlements (the Elite track showed locked) and refused every grant.

const BASE = `http://127.0.0.1:${PROGRESSION_DEFAULT_API_PORT}`;
const GS_KEY = crypto.randomBytes(24).toString("hex");

type Reply = { status: number, json: any };

async function Call(Method: string, Path: string, Options: { token?: string, gs?: boolean, body?: unknown } = {}): Promise<Reply> {
    const Headers: Record<string, string> = {};

    if(Options.token !== undefined) Headers["authorization"] = `bearer ${Options.token}`;
    if(Options.gs) Headers["x-undaunted-gameserver-apikey"] = GS_KEY;
    if(Options.body !== undefined) Headers["content-type"] = "application/json; charset=utf-8";

    const Response = await fetch(BASE + Path, { method: Method, headers: Headers, body: Options.body === undefined ? undefined : JSON.stringify(Options.body) });
    const Text = await Response.text();

    return { status: Response.status, json: Text.length > 0 ? JSON.parse(Text) : undefined };
}

let Listening: Server | undefined;

before(async () => {
    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY, "utf8").digest("hex") }).run();

    Listening = await new Promise<Server>((Resolve, Reject) => {
        const Started = app.listen(PROGRESSION_DEFAULT_API_PORT, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
    });
});

after(async () => {
    Listening?.closeAllConnections();
    await new Promise<void>((Resolve) => Listening ? Listening.close(() => Resolve()) : Resolve());
    RemoveTestDb(() => GetDb().$client.close());
});

describe("progression with no settings (real is the default)", () => {
    it("a new account reads its own tracks at 0 as a plain list, not the stub's 99,999,999", async () => {
        const { UserId } = await MakePlayer();
        const Token = SignMetagameJWTForUid(UserId);

        const Tracks = await Call("GET", `/progression/${UserId}`, { token: Token });
        assert.equal(Tracks.status, 200);
        assert.equal(Tracks.json.code, 200);
        assert.ok(Array.isArray(Tracks.json.payload), "real mode sends an array");
        assert.equal(Tracks.json.payload.length, 10);
        for(const Track of Tracks.json.payload){
            assert.equal(Track.phx_account_id, UserId);
            assert.deepEqual([Track.progress, Track.confirmed_fremium_rank, Track.confirmed_premium_rank], [0, 0, 0], Track.progression_id);
        }

        const Objectives = await Call("GET", `/progression/objectives/${UserId}`, { token: Token });
        assert.equal(Objectives.status, 200);
        assert.deepEqual(Objectives.json.payload, []);

        const HuntPass = await Call("GET", `/huntpass/${UserId}`, { token: Token });
        assert.deepEqual([HuntPass.status, HuntPass.json.payload], [200, "season09b"]);
    });

    it("every account owns the Elite Hunt Pass (season09b_premium), so the Elite track is not locked", async () => {
        const { UserId } = await MakePlayer();

        const Entitlements = await Call("GET", "/entitlementsv2", { token: SignMetagameJWTForUid(UserId) });
        assert.equal(Entitlements.status, 200);
        assert.ok(Entitlements.json.entitlements.some((Entitlement: any) => Entitlement.name === "season09b_premium"));
    });

    it("stores XP from the game server and gives it back after a fresh read", async () => {
        const { UserId } = await MakePlayer();
        const Token = SignMetagameJWTForUid(UserId);

        const Grant = await Call("POST", `/progression/${UserId}`, { gs: true, token: Token, body: { progress_tracks: [{ progression_id: "MasteryTrack_PlayerLevel", progress: 8 }], objectives: [{ objective_id: "OBJ_DEFAULT", value: 2, completed_count: 0 }] } });
        assert.equal(Grant.status, 200, "the stub refused every grant with 400");
        assert.deepEqual(Grant.json.payload.objectives.map((Objective: any) => [Objective.objective_id, Objective.progress]), [["OBJ_DEFAULT", 2]], "objectives are echoed (the endless mastery pop fix)");

        const Tracks = await Call("GET", `/progression/${UserId}`, { token: Token });
        assert.equal(Tracks.json.payload.find((Track: any) => Track.progression_id === "MasteryTrack_PlayerLevel").progress, 8);
    });

    it("a Hunt Pass claim on the free and the Elite track is stored (the confirm routes exist)", async () => {
        const { UserId } = await MakePlayer();
        const Token = SignMetagameJWTForUid(UserId);

        assert.equal((await Call("POST", `/progression/${UserId}/season09b/100`, { gs: true, token: Token })).status, 200);

        const Free = await Call("POST", `/progression/${UserId}/season09b/1/confirm/public`, { gs: true, token: Token });
        const Elite = await Call("POST", `/progression/${UserId}/season09b/1/confirm/premium`, { gs: true, token: Token });
        assert.deepEqual([Free.status, Elite.status], [200, 200]);

        const Track = (await Call("GET", `/progression/${UserId}/season09b`, { token: Token })).json.payload;
        assert.deepEqual([Track.progress, Track.confirmed_fremium_rank, Track.confirmed_premium_rank], [100, 1, 1]);
    });

    it("a player's own client still cannot grant itself XP", async () => {
        const { UserId } = await MakePlayer();

        const Grant = await Call("POST", `/progression/${UserId}`, { token: SignMetagameJWTForUid(UserId), body: { progress_tracks: [{ progression_id: "MasteryTrack_PlayerLevel", progress: 1000 }], objectives: [] } });
        assert.equal(Grant.status, 403);
    });
});
