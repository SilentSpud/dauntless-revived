import { RemoveTestDb } from "./setup";
import "./authenv";
import { WithEnv } from "./appenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { Call, StartApp, StopApp, Warnings } from "./appclient";
import { GetDb } from "../src/db";
import { GetAllEscalationSeasons, GetEscalationSeason } from "../src/controllers/escalationConfig";
import { Count, MakePlayer } from "./helpers";

// Escalation saves (roadmap 2.16). The season registry and the save rules come from Harmonic's fork
// (github.com/Harmonicrain/Undaunted 895f7c7, test/escalation.test.js); each ported case names the line of
// the original test. Here the rules are split in two: the hard ones always refuse, the soft ones (level costs,
// talent budget and tier gates, reward levels) refuse only with ESCALATION_STRICT=1 and are otherwise
// stored and logged, so each soft case runs twice. Refusals of a soft rule answer 409, the fork's 400.

const S1 = "ESC_SEASON_1";
const S2 = "ESC_SEASON_2";
const EMPTY_STATE = { escalation_level: 0, next_level_xp: 0, talents_progress: [], unlock_progress: [], update_version: 0 };

before(async () => {
    process.env.ESCALATION_MODE = "real";
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    process.env.ESCALATION_MODE = "real";
    delete process.env.ESCALATION_STRICT;
    Warnings.length = 0;
});

type Account = { UserId: string };

const Read = (A: Account, Season = S1, As: string | undefined = A.UserId, Gs = false) => Call("GET", `/escalation/${Season}/${A.UserId}`, { as: As, gs: Gs });
const Write = (A: Account, Snapshot: unknown, Season = S1, Options: { as?: string, gs?: boolean } = { gs: true }) => Call("POST", `/escalation/${Season}/${A.UserId}`, { ...Options, body: Snapshot });
const Snap = (Fields: Record<string, unknown> = {}) => ({ escalation_level: 0, next_level_xp: 0, talents_progress: [], unlock_progress: [], update_version: 1, ...Fields });
const Rows = (Table: string, UserId: string) => Count(Table, "accountId = ?", UserId);
const Events = (UserId: string) => GetDb().$client.prepare("select status, body, reply, note from progression_events where accountId = ? and route = 'POST /escalation/:season/:uid' order by id").all(UserId) as any[];
const Strict = <T>(Body: () => Promise<T>) => WithEnv({ ESCALATION_STRICT: "1" }, Body);

describe("the season registry", () => {
    // from Harmonicrain/Undaunted test/escalation.test.js:65
    it("carries the client's five seasons with Frost disabled", () => {
        const Seasons = GetAllEscalationSeasons();

        assert.deepEqual(Seasons.map((S) => S.id), ["ESC_SEASON_1", "ESC_SEASON_2", "ESC_SEASON_3", "ESC_SEASON_4", "ESC_SEASON_5"]);
        assert.deepEqual(Seasons.map((S) => S.enabled), [true, true, true, true, false]);
        for(const Season of Seasons){
            assert.equal(Season.levels.length, 25);
            assert.deepEqual(Season.levels[0], { level: 1, requiredExperience: 500 });
            assert.equal(Season.talents.length, 18);
            assert.equal(Season.unlocks.length, 6);
        }
        assert.equal(GetEscalationSeason("ESC_SEASON_9"), undefined, "unknown ids never fall back");
    });
});

describe("ESCALATION_MODE=stub (the default): what every account got before", () => {
    it("answers the fake maximum to anyone and keeps the save route's 404", async () => {
        await WithEnv({ ESCALATION_MODE: undefined }, async () => {
            const A = await MakePlayer(), B = await MakePlayer();

            for(const Reply of [await Read(A), await Read(A, S1, B.UserId), await Read(A, "ESC_SEASON_9"), await Read(A, S1, undefined, true)]){
                assert.equal(Reply.status, 200);
                assert.deepEqual(Reply.json, { code: null, message: "OK", payload: { escalation_level: 99999, next_level_xp: 99999, talents_progress: [], unlock_progress: [], update_version: 1 } });
            }

            assert.equal((await Write(A, Snap({ escalation_level: 1 }))).status, 404);
            assert.equal((await Write(A, Snap({ escalation_level: 1 }), S1, { gs: true, as: A.UserId })).status, 404);
            assert.equal(Rows("escalationprogression", A.UserId), 0);
            assert.deepEqual(Events(A.UserId), []);
        });
    });

    it("keeps the stub for accounts outside real progression (PROGRESSION_MODE=stub) with ESCALATION_MODE=real", async () => {
        const A = await MakePlayer();

        await WithEnv({ PROGRESSION_MODE: "stub" }, async () => {
            assert.equal((await Read(A)).json.payload.escalation_level, 99999);
            assert.equal((await Write(A, Snap({ escalation_level: 1 }))).status, 404);
        });
        assert.equal(Rows("escalationprogression", A.UserId), 0);
    });
});

describe("reads (ESCALATION_MODE=real)", () => {
    // from Harmonicrain/Undaunted test/escalation.test.js:80
    it("a fresh account reads the native default for every season and no row is created", async () => {
        const A = await MakePlayer();

        for(const Season of GetAllEscalationSeasons()){
            const { status, json } = await Read(A, Season.id);
            assert.equal(status, 200);
            assert.equal(json.code, null);
            assert.equal(json.message, "OK");
            assert.deepEqual(json.payload, EMPTY_STATE, "never the old stub's 99999");
        }
        assert.equal(Rows("escalationprogression", A.UserId), 0);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:92
    it("an unknown season is a 404, not another season's data", async () => {
        const A = await MakePlayer();

        assert.equal((await Read(A, "ESC_SEASON_9")).status, 404);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:97, adapted: another player's account is refused
    // (403, as every real-mode read here), where the fork answered the asking player's own state
    it("a player reads only their own season; a game server reads the named account", async () => {
        const A = await MakePlayer(), B = await MakePlayer();
        assert.equal((await Write(A, Snap({ escalation_level: 1, next_level_xp: 10 }))).status, 200);

        const AsB = await Read(A, S1, B.UserId);
        assert.equal(AsB.status, 403);
        assert.equal(AsB.text, "");

        assert.equal((await Read(A)).json.payload.escalation_level, 1, "the account's own client");
        assert.equal((await Read(A, S1, undefined, true)).json.payload.escalation_level, 1, "a game server with its key alone");
        assert.equal((await Read(A, S1, B.UserId, true)).json.payload.escalation_level, 1, "a game server relaying another token reads the named account");
    });
});

describe("saves (ESCALATION_MODE=real)", () => {
    // from Harmonicrain/Undaunted test/escalation.test.js:108, adapted: a game server's save that relays
    // another player's token is logged and kept for the named account (the fork refused it with 403)
    it("only a game server may save; a relayed token for another account is logged, not refused", async () => {
        const A = await MakePlayer(), B = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 25 }), S1, { as: A.UserId })).status, 403);
        assert.equal(Rows("escalationprogression", A.UserId), 0);

        assert.equal((await Write(A, Snap(), S1, { gs: true, as: B.UserId })).status, 200);
        assert.ok(Warnings.some((Line) => Line === `Game server escalation save for ${A.UserId} carries the token of ${B.UserId}: accepted for ${A.UserId}, the account the request names`), Warnings.join("\n"));

        assert.equal((await Write(A, Snap(), S1, { gs: true, as: A.UserId })).status, 200, "the same save again is a retry");
        assert.equal(Rows("escalationprogression", A.UserId), 1);
    });

    it("a player's own client cannot save, and nothing is stored or audited", async () => {
        const A = await MakePlayer();

        const Reply = await Write(A, Snap({ escalation_level: 3 }), S1, { as: A.UserId });
        assert.equal(Reply.status, 403);
        assert.equal(Rows("escalationprogression", A.UserId), 0);
        assert.deepEqual(Events(A.UserId), []);
    });

    it("an unknown account is a 404, audited", async () => {
        const Reply = await Write({ UserId: "UID-nobody-escalation" }, Snap());

        assert.equal(Reply.status, 404);
        assert.equal(Count("escalationprogression", "accountId = 'UID-nobody-escalation'"), 0);
        assert.deepEqual(Events("UID-nobody-escalation").map((Event) => Event.status), [404]);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:116
    it("an accepted snapshot reads back identically, including from a new process", async () => {
        const A = await MakePlayer();
        const Snapshot = Snap({
            escalation_level: 6, next_level_xp: 250, update_version: 3,
            talents_progress: [{ rank: 2, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }, { rank: 1, talent_id: "ESC_TALENT_S1_TIER1_UPGRADETWO" }],
            unlock_progress: [{ collected: true, reward_id: "ESC_Reward_3" }, { collected: false, reward_id: "ESC_Reward_2" }]
        });

        const Written = await Write(A, Snapshot);
        assert.equal(Written.status, 200);

        const Read1 = (await Read(A)).json.payload;
        assert.deepEqual(Written.json.payload, Read1);
        assert.deepEqual([Read1.escalation_level, Read1.next_level_xp, Read1.update_version], [6, 250, 3]);
        // Canonical: season order, uncollected unlocks omitted (unlocked-ness is derived natively)
        assert.deepEqual(Read1.talents_progress, [{ rank: 1, talent_id: "ESC_TALENT_S1_TIER1_UPGRADETWO" }, { rank: 2, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }]);
        assert.deepEqual(Read1.unlock_progress, [{ collected: true, reward_id: "ESC_Reward_3" }]);

        const Restart = spawnSync(process.execPath, ["-e",
            "console.log(JSON.stringify(require('./build/src/controllers/escalation').GetEscalationState(process.argv[1], process.argv[2])))",
            A.UserId, S1], { cwd: process.cwd(), env: process.env, encoding: "utf8" });
        assert.equal(Restart.status, 0, Restart.stderr);
        assert.deepEqual(JSON.parse(Restart.stdout.trim().split(/\r?\n/).at(-1)!), Read1);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:141, adapted: there is no escalation event table;
    // the replay is a progression_events row that says so, and the stored row is not written again
    it("an exact retry is a replay; a reused version with different content is refused", async () => {
        const A = await MakePlayer();
        const First = Snap({ escalation_level: 1, next_level_xp: 100, update_version: 1 });

        const Saved = await Write(A, First);
        assert.equal(Saved.status, 200);
        const StoredDate = (GetDb().$client.prepare("select updatedDate from escalationprogression where accountId = ?").get(A.UserId) as any).updatedDate;

        const Again = await Write(A, First);
        assert.equal(Again.status, 200);
        assert.deepEqual(Again.json, Saved.json, "the stored state, as the first answer");
        assert.equal((GetDb().$client.prepare("select updatedDate from escalationprogression where accountId = ?").get(A.UserId) as any).updatedDate, StoredDate, "a retry writes nothing");
        assert.match(Events(A.UserId)[1].note, /replay of version 1, nothing changed/);

        assert.equal((await Write(A, { ...First, next_level_xp: 120 })).status, 409);
        assert.equal((await Read(A)).json.payload.next_level_xp, 100);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:151
    it("stale and reordered saves cannot overwrite newer state", async () => {
        const A = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 2, next_level_xp: 50, update_version: 5 }))).status, 200);
        assert.equal((await Write(A, Snap({ escalation_level: 1, next_level_xp: 0, update_version: 4 }))).status, 409);
        assert.equal((await Write(A, Snap({ escalation_level: 1, next_level_xp: 0, update_version: 6 }))).status, 409, "a newer version still may not lower progress");
        assert.equal((await Write(A, Snap({ escalation_level: 2, next_level_xp: 10, update_version: 7 }))).status, 409);

        const State = (await Read(A)).json.payload;
        assert.deepEqual([State.escalation_level, State.next_level_xp, State.update_version], [2, 50, 5]);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:162
    it("a talent reset lowers ranks and keeps level, XP and collections", async () => {
        const A = await MakePlayer();
        const Unlocks = [{ collected: true, reward_id: "ESC_Reward_3" }];

        assert.equal((await Write(A, Snap({ escalation_level: 5, update_version: 1, talents_progress: [{ rank: 3, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }], unlock_progress: Unlocks }))).status, 200);
        assert.equal((await Write(A, Snap({ escalation_level: 5, update_version: 2, talents_progress: [], unlock_progress: Unlocks }))).status, 200);

        const State = (await Read(A)).json.payload;
        assert.deepEqual(State.talents_progress, []);
        assert.equal(State.escalation_level, 5);
        assert.deepEqual(State.unlock_progress, Unlocks);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:174 (ESCALATION_STRICT=1), adapted: the cap is
    // reached first, since one save that jumps from a lower level to the cap with 99999 XP or more is the
    // stub guard's case
    it("strict: XP follows native level arithmetic, below the next cost and unbounded only at the cap", async () => {
        await Strict(async () => {
            const A = await MakePlayer();

            assert.equal((await Write(A, Snap({ escalation_level: 0, next_level_xp: 500 }))).status, 409, "500 XP at level 0 is level 1");
            assert.equal((await Write(A, Snap({ escalation_level: 0, next_level_xp: 499 }))).status, 200);
            assert.equal((await Write(A, Snap({ escalation_level: 26, update_version: 2 }))).status, 400);
            assert.equal((await Write(A, Snap({ escalation_level: -1, update_version: 2 }))).status, 400);
            assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 10, update_version: 2 }))).status, 200);
            assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 123456, update_version: 3 }))).status, 200);
            assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 2 ** 31, update_version: 4 }))).status, 400, "int32 limit");
        });
    });

    it("warn-only twin: XP at or above the next level's cost is stored and logged", async () => {
        const A = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 0, next_level_xp: 500 }))).status, 200);
        assert.equal((await Read(A)).json.payload.next_level_xp, 500);
        assert.match(Events(A.UserId)[0].note, /accepted although next_level_xp 500 is not below the level 0 cost 500 \(ESCALATION_STRICT=0\)/);
        assert.ok(Warnings.some((Line) => /breaks a soft rule, stored anyway/.test(Line)));

        assert.equal((await Write(A, Snap({ escalation_level: 26, update_version: 2 }))).status, 400, "the hard rules still refuse");
        assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 2 ** 31, update_version: 2 }))).status, 400);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:184 (ESCALATION_STRICT=1)
    it("strict: talents are limited to one point per level, known ids and their max rank", async () => {
        await Strict(async () => {
            const A = await MakePlayer();
            const Over = [{ rank: 3, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }, { rank: 1, talent_id: "ESC_TALENT_S1_TIER1_UPGRADETWO" }];

            assert.equal((await Write(A, Snap({ escalation_level: 3, talents_progress: Over }))).status, 409);
            assert.equal((await Write(A, Snap({ escalation_level: 4, talents_progress: Over }))).status, 200);
            assert.equal((await Write(A, Snap({ escalation_level: 25, update_version: 2, talents_progress: [{ rank: 4, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }] }))).status, 400);
            assert.equal((await Write(A, Snap({ escalation_level: 25, update_version: 2, talents_progress: [{ rank: 1, talent_id: "ESC_TALENT_S2_TIER1_PASSIVE" }] }))).status, 400, "another season's talent");
            assert.equal((await Write(A, Snap({ escalation_level: 25, update_version: 2, talents_progress: [{ rank: 1, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }, { rank: 1, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }] }))).status, 400);
        });
    });

    it("warn-only twin: more talent points than levels are stored and logged; ids and max ranks still refuse", async () => {
        const A = await MakePlayer();
        const Over = [{ rank: 3, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }, { rank: 1, talent_id: "ESC_TALENT_S1_TIER1_UPGRADETWO" }];

        assert.equal((await Write(A, Snap({ escalation_level: 3, talents_progress: Over }))).status, 200);
        assert.match(Events(A.UserId)[0].note, /talents spend 4 points but level 3 only earns 3/);
        assert.equal((await Write(A, Snap({ escalation_level: 25, update_version: 2, talents_progress: [{ rank: 4, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }] }))).status, 400);
        assert.equal((await Write(A, Snap({ escalation_level: 25, update_version: 2, talents_progress: [{ rank: 1, talent_id: "ESC_TALENT_S2_TIER1_PASSIVE" }] }))).status, 400);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:196 (ESCALATION_STRICT=1)
    it("strict: a talent tier needs its PointsToUnlock spent in earlier tiers, as SharedUpgradeTalent requires", async () => {
        await Strict(async () => {
            const A = await MakePlayer();
            // TIER2 talents have PointsToUnlock 4
            const Tier2 = { rank: 1, talent_id: "ESC_TALENT_S1_TIER2_PASSIVE" };

            assert.equal((await Write(A, Snap({ escalation_level: 1, talents_progress: [Tier2] }))).status, 409, "a level 1 player cannot hold a tier 2 talent");
            assert.equal((await Write(A, Snap({ escalation_level: 25, talents_progress: [Tier2, { rank: 3, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }] }))).status, 409, "3 tier-1 points do not open tier 2");
            assert.equal((await Write(A, Snap({ escalation_level: 25, talents_progress: [Tier2, { rank: 2, talent_id: "ESC_TALENT_S1_TIER2_UPGRADEONE" }, { rank: 1, talent_id: "ESC_TALENT_S1_TIER2_UPGRADETWO" }] }))).status, 409, "a tier cannot pay for its own gate");
            assert.equal((await Write(A, Snap({ escalation_level: 5, talents_progress: [Tier2, { rank: 3, talent_id: "ESC_TALENT_S1_TIER1_PASSIVE" }, { rank: 1, talent_id: "ESC_TALENT_S1_TIER1_UPGRADETWO" }] }))).status, 200);
        });
    });

    it("warn-only twin: a tier held without its gate is stored and logged", async () => {
        const A = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 1, talents_progress: [{ rank: 1, talent_id: "ESC_TALENT_S1_TIER2_PASSIVE" }] }))).status, 200);
        assert.match(Events(A.UserId)[0].note, /talent ESC_TALENT_S1_TIER2_PASSIVE needs 4 points spent in earlier tiers, found 0/);
        assert.deepEqual((await Read(A)).json.payload.talents_progress, [{ rank: 1, talent_id: "ESC_TALENT_S1_TIER2_PASSIVE" }]);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:211 (ESCALATION_STRICT=1)
    it("strict: a collection needs its unlock level and can never be undone", async () => {
        await Strict(async () => {
            const A = await MakePlayer();
            const Collect = (Level: number, Version: number, Ids: string[]) => Write(A, Snap({ escalation_level: Level, update_version: Version, unlock_progress: Ids.map((Id) => ({ collected: true, reward_id: Id })) }));

            assert.equal((await Collect(4, 1, ["ESC_Reward_3"])).status, 409, "ESC_Reward_3 unlocks at level 5");
            assert.equal((await Collect(5, 1, ["ESC_Reward_3"])).status, 200);
            const CollectedAt = (GetDb().$client.prepare("select collectedDate from escalationunlocks where accountId = ?").get(A.UserId) as any).collectedDate;
            assert.equal((await Collect(8, 2, ["ESC_Reward_3", "ESC_Reward_2"])).status, 200);
            assert.equal((await Collect(9, 3, ["ESC_Reward_2"])).status, 409, "ESC_Reward_3 may not be un-collected");
            assert.equal((GetDb().$client.prepare("select collectedDate from escalationunlocks where accountId = ? and unlockId = 'ESC_Reward_3'").get(A.UserId) as any).collectedDate, CollectedAt, "the first collection time is kept");
            assert.equal((await Write(A, Snap({ update_version: 9, unlock_progress: [{ collected: true, reward_id: "ESC_Reward_404" }] }))).status, 400);
        });
    });

    it("warn-only twin: a reward collected below its level is stored and logged; un-collecting still refuses", async () => {
        const A = await MakePlayer();
        const Collect = (Level: number, Version: number, Ids: string[]) => Write(A, Snap({ escalation_level: Level, update_version: Version, unlock_progress: Ids.map((Id) => ({ collected: true, reward_id: Id })) }));

        assert.equal((await Collect(4, 1, ["ESC_Reward_3"])).status, 200);
        assert.match(Events(A.UserId)[0].note, /unlock ESC_Reward_3 is collected at level 4 but needs level 5/);
        assert.equal((await Collect(5, 2, [])).status, 409, "a collection is never undone");
        assert.equal((await Write(A, Snap({ escalation_level: 5, update_version: 3, unlock_progress: [{ collected: true, reward_id: "ESC_Reward_404" }] }))).status, 400);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:225
    it("seasons and accounts are isolated", async () => {
        const A = await MakePlayer(), B = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 7 }), S1)).status, 200);
        assert.equal((await Write(A, Snap({ escalation_level: 3 }), S2)).status, 200, "version 1 is independent per season");
        assert.equal((await Write(B, Snap({ escalation_level: 1 }), S1)).status, 200, "and per account");
        assert.equal((await Read(A, S1)).json.payload.escalation_level, 7);
        assert.equal((await Read(A, S2)).json.payload.escalation_level, 3);
        assert.equal((await Read(B, S1)).json.payload.escalation_level, 1);
        assert.equal((await Read(B, S2)).json.payload.escalation_level, 0);
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:236
    it("the disabled Frost season reads normally but refuses saves", async () => {
        const A = await MakePlayer();

        assert.equal((await Read(A, "ESC_SEASON_5")).status, 200);
        assert.equal((await Write(A, Snap(), "ESC_SEASON_5")).status, 409);
        assert.equal((await Write(A, Snap(), "ESC_SEASON_9")).status, 404, "an unknown season");
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:242, adapted: the guard covers any first save at
    // the last level with 99999 XP or more (a world server that loaded the stub may have gained XP since)
    it("a world server still holding the old stub values cannot save them", async () => {
        const A = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 99999, update_version: 2 }))).status, 409);
        assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 99999 + 4321, update_version: 3 }))).status, 409, "the fork's guard checked 99999 exactly");
        assert.equal(Rows("escalationprogression", A.UserId), 0);
        assert.match(Events(A.UserId)[0].note, /first save carries the old stub values/);

        // A player who really is at the cap keeps gaining XP there
        const B = await MakePlayer();
        assert.equal((await Write(B, Snap({ escalation_level: 25, next_level_xp: 10 }))).status, 200);
        assert.equal((await Write(B, Snap({ escalation_level: 25, next_level_xp: 150000, update_version: 2 }))).status, 200);
    });

    // A world server that loaded the stub during a stub period kept counting versions while its saves got
    // 404, so after a switch back to real its version can be above the stored one
    it("a world server still holding the old stub values cannot overwrite a stored season either", async () => {
        const A = await MakePlayer();

        assert.equal((await Write(A, Snap({ escalation_level: 2, next_level_xp: 100, update_version: 1 }))).status, 200);
        assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 99999, update_version: 4 }))).status, 409, "a newer version with the stub values");
        assert.equal((await Write(A, Snap({ escalation_level: 25, next_level_xp: 120000, update_version: 4 }))).status, 409, "or more XP since");
        assert.match(Events(A.UserId).at(-1).note, /save over level 2 carries the old stub values/);
        assert.deepEqual((await Read(A)).json.payload, { escalation_level: 2, next_level_xp: 100, talents_progress: [], unlock_progress: [], update_version: 1 });

        assert.equal((await Write(A, Snap({ escalation_level: 2, next_level_xp: 150, update_version: 5 }))).status, 200, "the player's real save afterwards");
        assert.deepEqual((await Read(A)).json.payload, { escalation_level: 2, next_level_xp: 150, talents_progress: [], unlock_progress: [], update_version: 5 });
    });

    // from Harmonicrain/Undaunted test/escalation.test.js:249
    it("malformed snapshots are refused without storing anything", async () => {
        const A = await MakePlayer();

        for(const Bad of [[], Snap({ update_version: 0 }), Snap({ talents_progress: null }), Snap({ unlock_progress: [{ reward_id: "ESC_Reward_2" }] }), Snap({ next_level_xp: 1.5 }), Snap({ escalation_level: "3" })]){
            assert.equal((await Write(A, Bad)).status, 400, JSON.stringify(Bad));
        }
        assert.equal(Rows("escalationprogression", A.UserId), 0);
        assert.ok(Events(A.UserId).every((Event) => Event.status === 400), "each refusal is audited");
    });

    it("every save is a progression_events row with the body and the reply", async () => {
        const A = await MakePlayer();
        const Body = Snap({ escalation_level: 2, next_level_xp: 40, talents_progress: [{ rank: 1, talent_id: "ESC_TALENT_S1_TIER1_UPGRADEONE" }] });

        const Reply = await Write(A, Body, S1, { gs: true, as: A.UserId });
        assert.equal(Reply.status, 200);

        const [Event] = Events(A.UserId);
        assert.equal(Event.status, 200);
        assert.deepEqual(JSON.parse(Event.body), Body);
        assert.deepEqual(JSON.parse(Event.reply), Reply.json);
        assert.equal(Event.note, `season ${S1}`);
        assert.equal((GetDb().$client.prepare("select caller from progression_events where accountId = ? order by id desc").get(A.UserId) as any).caller, "gameserver");
    });
});
