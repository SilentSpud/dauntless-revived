import { RemoveTestDb } from "./setup";
import { after, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { logger } from "../src/logger";
import { NoteRelayedAccountMismatch, RefuseUnlessGameserver } from "../src/middleware/GameServerOnly";

// The one guard for the game-server-only writes (it was two copies, in routes/system.ts and
// routes/loadout.ts), and the log line for a game server's request that carries another account's
// token. Harmonic's fork (github.com/Harmonicrain/Undaunted 895f7c7, routes/escalation.ts) refuses such
// a request with 403; here it is only logged (see middleware/GameServerOnly.ts).

const Warnings: string[] = [];
const Backup = { warn: logger.warn };
(logger as any).warn = (Message: unknown) => { Warnings.push(String(Message)); };

// No database is opened here; only setup's empty folder is removed
after(() => {
    Object.assign(logger, Backup);
    RemoveTestDb(() => undefined);
});

beforeEach(() => { Warnings.length = 0; });

function FakeResponse(){
    const Res: any = { statusCode: 200, sent: false };
    Res.status = (Code: number) => { Res.statusCode = Code; return Res; };
    Res.send = () => { Res.sent = true; return Res; };
    return Res;
}

describe("RefuseUnlessGameserver", () => {
    it("refuses a player's request with 403 and lets a game server's through", () => {
        const Player = FakeResponse();
        assert.equal(RefuseUnlessGameserver({ AuthData: { userId: "UID-p" }, params: { userId: "UID-p" } }, Player, "cooldown batch"), true);
        assert.deepEqual([Player.statusCode, Player.sent], [403, true]);
        assert.deepEqual(Warnings, ["Refusing cooldown batch for UID-p from a player client"], "the system routes' old log line");

        const Loadout = FakeResponse();
        assert.equal(RefuseUnlessGameserver({ AuthData: { userId: "UID-p" }, params: { userId: "UID-p", characterId: "c-1" } }, Loadout, "loadout slot unlock", "characterId c-1"), true);
        assert.equal(Warnings[1], "Refusing loadout slot unlock for characterId c-1 from a player client", "the loadout routes' old log line");

        const GameServer = FakeResponse();
        assert.equal(RefuseUnlessGameserver({ AuthData: { IsGameserver: true }, params: { userId: "UID-p" } }, GameServer, "cooldown batch"), false);
        assert.deepEqual([GameServer.statusCode, GameServer.sent], [200, false]);
        assert.equal(Warnings.length, 2);
    });
});

describe("NoteRelayedAccountMismatch", () => {
    it("logs a game server's request that carries another account's token, once a minute, and never refuses it", () => {
        const Mismatch = { AuthData: { IsGameserver: true, userId: "UID-token-owner" }, params: { userId: "UID-named" } };
        const Res = FakeResponse();

        assert.equal(RefuseUnlessGameserver(Mismatch, Res, "bounty update"), false, "not refused");
        assert.equal(Res.sent, false);
        assert.deepEqual(Warnings, ["Game server bounty update for UID-named carries the token of UID-token-owner: accepted for UID-named, the account the request names"]);

        assert.equal(NoteRelayedAccountMismatch(Mismatch, "UID-named", "bounty update"), true);
        assert.equal(Warnings.length, 1, "the same mismatch again within a minute is not logged again");
        assert.equal(NoteRelayedAccountMismatch(Mismatch, "UID-named", "cooldown batch"), true);
        assert.equal(Warnings.length, 2);
    });

    it("is quiet for the token's own account, no token, and a player's request", () => {
        assert.equal(NoteRelayedAccountMismatch({ AuthData: { IsGameserver: true, userId: "UID-named" } }, "UID-named", "bounty update"), false, "its own token");
        assert.equal(NoteRelayedAccountMismatch({ AuthData: { IsGameserver: true } }, "UID-named", "bounty update"), false, "no token");
        assert.equal(NoteRelayedAccountMismatch({ AuthData: { IsGameserver: true, userId: "" } }, "UID-named", "bounty update"), false, "an empty account");
        assert.equal(NoteRelayedAccountMismatch({ AuthData: { userId: "UID-token-owner" } }, "UID-named", "bounty update"), false, "not a game server");
        assert.equal(NoteRelayedAccountMismatch({}, "UID-named", "bounty update"), false);
        assert.deepEqual(Warnings, []);
    });
});
