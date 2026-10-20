import "./setup";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import type { Server } from "node:http";
import { CheckMatchmakingRequest } from "../src/controllers/matchmakinginput";
import { Gameservers } from "../src/controllers/gameservers";
import { IsDirectLoopbackRequest } from "../src/routes/gameservers";
import { app } from "../src/app";

// The matchmaking call starts game server processes with parts of the client's input on
// their command line. These tests only ever send requests that are refused: nothing here
// may start a game process. A spare port for this test file only.
const TEST_PORT = 62473;
const MATCHMAKING = `http://127.0.0.1:${TEST_PORT}/api/matchmaker/handle-matchmaking-for-player`;
const TUTORIAL_ARGS = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";

describe("CheckMatchmakingRequest", () => {
    it("accepts every shape the metagame sends", () => {
        assert.equal(CheckMatchmakingRequest("ISLAND", TUTORIAL_ARGS, undefined, undefined), undefined);
        assert.equal(CheckMatchmakingRequest("CITY", "", "ShatteredIsles_ReturnToRamsgate", undefined), undefined);
        assert.equal(CheckMatchmakingRequest("SHARED", "", "ShatteredIsles_TrainingDojo", undefined), undefined);
        assert.equal(CheckMatchmakingRequest("ISLAND", "", "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", ["UID-00000000-0000-4000-8000-00000000f609"]), undefined);
        assert.equal(CheckMatchmakingRequest("ISLAND", "", "CR19_PlayerHunt_Patrol_Heroic+_Gem", ["UID-a", "UID-b", "UID-c", "UID-d"]), undefined);
        assert.equal(CheckMatchmakingRequest(undefined, undefined, undefined, undefined), undefined, "an empty call ends in the Ramsgate fallback as before");
    });

    it("refuses anything that could put something else on a game server's command line", () => {
        for(const [GameMode, GameArgs, HuntId, Players] of [
            ["ISLAND", "/Game/Maps/islands/x -ExecCmds=quit?MaxPlayers=1?MonsterClass=/Game/M/b.b_C", undefined, undefined],
            ["ISLAND", "/Game/Maps/islands/x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C \"-log\"", undefined, undefined],
            ["ISLAND", "/Game/Maps/islands/x", undefined, undefined],
            ["ISLAND", "/Game/../Maps/x?MaxPlayers=1?MonsterClass=/Game/M/b.b_C", undefined, undefined],
            ["ISLAND", "-server?MaxPlayers=1?MonsterClass=/Game/M/b.b_C", undefined, undefined],
            ["ISLAND", 42, undefined, undefined],
            ["ISLAND", "", "CR19_PlayerHunt_Arena -ExecCmds=quit", ["UID-a"]],
            ["ISLAND", "", "CR19_Arena:x,UID-evil:y", ["UID-a"]],
            ["ISLAND", "", { toString: 1 }, ["UID-a"]],
            ["ISLAND", "", "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", ["UID-a:CR19_x,UID-b"]],
            ["ISLAND", "", "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", ["UID a"]],
            ["ISLAND", "", "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", "UID-a"],
            ["ISLAND", "", "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", Array.from({ length: 17 }, (_, i) => `UID-${i}`)]
        ] as [unknown, unknown, unknown, unknown][]){
            assert.notEqual(CheckMatchmakingRequest(GameMode, GameArgs, HuntId, Players), undefined, JSON.stringify([GameMode, GameArgs, HuntId, Players]));
        }
    });
});

describe("POST /api/matchmaker/handle-matchmaking-for-player", () => {
    let Listening: Server | undefined;

    before(async () => {
        Listening = await new Promise<Server>((Resolve, Reject) => {
            const Started = app.listen(TEST_PORT, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
        });
    });

    after(() => {
        Listening?.closeAllConnections();
        Listening?.close();
    });

    async function Post(Body: unknown, Headers: Record<string, string> = {}){
        const Reply = await fetch(MATCHMAKING, { method: "POST", headers: { "content-type": "application/json", ...Headers }, body: JSON.stringify(Body) });
        const Text = await Reply.text();
        return { status: Reply.status, text: Text };
    }

    it("answers 400 to bad input and starts nothing", async () => {
        const Reply = await Post({ GameMode: "ISLAND", GameArgs: "/Game/Maps/islands/x -ExecCmds=quit?MaxPlayers=1?MonsterClass=/Game/M/b.b_C" });
        assert.equal(Reply.status, 400);
        assert.equal(JSON.parse(Reply.text).error, "bad_request");

        assert.equal((await Post({ GameMode: "ISLAND", GameArgs: "", HuntId: "CR19_PlayerHunt_Arena -ExecCmds=quit", ExpectedPlayers: ["UID-a"] })).status, 400);
        assert.equal((await Post({ GameMode: "ISLAND", GameArgs: "", HuntId: "CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr", ExpectedPlayers: ["UID-a:x,UID-b"] })).status, 400);
        assert.equal(Gameservers.length, 0, "no game server was started");
    });

    it("refuses anything relayed by a proxy (403), before reading the request", async () => {
        // A body that would get 400: the 403 shows the caller check comes first
        for(const Headers of [{ "x-forwarded-for": "203.0.113.9" }, { "x-dauntless-gateway": "secret" }, { "via": "1.1 relay" }, { "forwarded": "for=203.0.113.9" }] as Record<string, string>[]){
            assert.equal((await Post({ GameMode: "ISLAND", GameArgs: "/Game/Maps/x -ExecCmds=quit" }, Headers)).status, 403, JSON.stringify(Headers));
        }

        assert.equal((await fetch(`http://127.0.0.1:${TEST_PORT}/gameservers`, { headers: { "x-forwarded-for": "203.0.113.9" } })).status, 403);
        assert.equal((await fetch(`http://127.0.0.1:${TEST_PORT}/gameservers`)).status, 200);
        assert.equal(Gameservers.length, 0, "no game server was started");
    });
});

describe("IsDirectLoopbackRequest", () => {
    it("is true only for a loopback caller that no proxy relayed", () => {
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "127.0.0.1" }, headers: {} }), true);
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "::ffff:127.0.0.1" }, headers: {} }), true);
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "::1" }, headers: {} }), true);
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "100.64.0.7" }, headers: {} }), false);
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "203.0.113.5" }, headers: {} }), false);
        assert.equal(IsDirectLoopbackRequest({ socket: {}, headers: {} }), false);
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "127.0.0.1" }, headers: { "x-forwarded-for": "127.0.0.1" } }), false);
        assert.equal(IsDirectLoopbackRequest({ socket: { remoteAddress: "127.0.0.1" }, headers: { "x-real-ip": "127.0.0.1" } }), false);
    });
});
