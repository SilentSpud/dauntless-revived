import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { eq } from "drizzle-orm";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { inventory } from "../src/db/schema";
import { ComputeEarnedRanks, GetProgressionPath, GetProgressionPaths } from "../src/controllers/progressionrank";
import { ConfirmRank, GetObjectiveRecords, GetTrackRecord, GrantProgression } from "../src/controllers/realprogression";
import { Count, MakePlayer } from "./helpers";

// Harmonic's mastery cases (github.com/Harmonicrain/Undaunted 895f7c7, test/mastery.test.js) against our
// real progression; each names the line of the original test. Inverted: 110 (a replayed award adds nothing through
// the retry guard of the grant, not by skipping tracks whose objectives did not move), 147 (a confirm pays
// no Rams: the play-through in progressionextras.test.ts), 204 (one malformed entry does not throw away
// the valid entries' XP; the game server does not retry a refused grant). Skipped: 138 (objective values
// are absolute here, and a value that goes backwards is only noted) and 163 (there is no payout on
// confirm to roll back).

const PLAYER = "MasteryTrack_PlayerLevel";
const SWORD = "MasteryTrack_Weapon_Sword";

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

const Earned = (Id: string, Progress: number) => ComputeEarnedRanks(GetProgressionPath(Id)!, Progress, false).EarnedFreeRank;
const Award = (UserId: string, Tracks: unknown[], Objectives: unknown[] = []) => GrantProgression(UserId, { progress_tracks: Tracks, objectives: Objectives }, "gameserver");
const Get = async (Path: string, UserId: string) => {
    const Reply = await Call("GET", Path, { as: UserId });
    assert.equal(Reply.status, 200, Path);
    return Reply.json.payload;
};

describe("mastery tracks (test/mastery.test.js)", () => {
    // from Harmonicrain/Undaunted test/mastery.test.js:50
    it("a fresh account starts at Slayer level 1 with no weapon or behemoth mastery", async () => {
        const { UserId } = await MakePlayer();

        assert.equal(Earned(PLAYER, 0), 1);

        const Masteries = (await Get(`/progression/${UserId}`, UserId)).filter((Track: any) => Track.progression_id.startsWith("MasteryTrack_"));
        assert.equal(Masteries.length, 9);
        for(const Track of Masteries){
            assert.deepEqual([Track.progress, Track.confirmed_fremium_rank], [0, 0], Track.progression_id);
            if(Track.progression_id !== PLAYER){
                assert.equal(Earned(Track.progression_id, 0), 0);
            }
        }
        assert.equal(Count("progress_tracks", "accountId = ?", UserId), 0, "reads do not create or max accounts");
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:65, prestige-aware: the mastery tracks have none
    it("each mastery track uses its configured thresholds and cap", () => {
        const Masteries = GetProgressionPaths().filter((Path) => Path.progression_id.startsWith("MasteryTrack_"));
        assert.equal(Masteries.length, 9);

        for(const Path of Masteries){
            assert.equal(Path.prestige ?? null, null, `${Path.progression_id} has no prestige`);
            let Total = 0;

            for(const Requirement of Path.requirements!){
                Total += Requirement.xp_required;
                if(Requirement.xp_required > 0){
                    assert.equal(Earned(Path.progression_id, Total - 1), Requirement.rank_id - 1);
                }
                // Player ranks 0 and 1 share the zero threshold
                assert.ok(Earned(Path.progression_id, Total) >= Requirement.rank_id);
            }

            assert.equal(Earned(Path.progression_id, Total + 1000), Path.progression_id.includes("Weapon") ? 20 : 50);
        }
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:79
    it("mastery awards read back the same through the list, the objectives and the single track, also in a new process", async () => {
        const { UserId } = await MakePlayer();
        const Body = { progress_tracks: [{ progression_id: PLAYER, progress: 2 }, { progression_id: SWORD, progress: 2 }], objectives: [{ objective_id: "MasteryObjective_Sword_Craft", value: 1, completed_count: 1 }] };

        assert.equal((await Call("POST", `/progression/${UserId}`, { gs: true, as: UserId, body: Body })).status, 200);

        const All = await Get(`/progression/${UserId}`, UserId);
        const Objectives = await Get(`/progression/objectives/${UserId}`, UserId);
        const Single = await Get(`/progression/${UserId}/${SWORD}`, UserId);

        assert.deepEqual(Single, All.find((Track: any) => Track.progression_id === SWORD));
        assert.ok(Array.isArray(Objectives), "FindObjectivesEndpoint needs payload to be an array");
        assert.deepEqual([Objectives[0].objective_id, Objectives[0].progress, Objectives[0].completed_count], ["MasteryObjective_Sword_Craft", 1, 1]);
        assert.equal(Single.progress, 2);
        assert.equal(Earned(SWORD, Single.progress), 1);

        const Restart = spawnSync(process.execPath, ["-e", `
            const P = require("./build/src/controllers/realprogression");
            console.log(JSON.stringify({ track: P.GetTrackRecord(process.argv[1], process.argv[2]), objectives: P.GetObjectiveRecords(process.argv[1]) }));
        `, UserId, SWORD], { cwd: process.cwd(), env: process.env, encoding: "utf8" });
        assert.equal(Restart.status, 0, Restart.stderr);

        const Reloaded = JSON.parse(Restart.stdout.trim().split(/\r?\n/).at(-1)!);
        assert.deepEqual(Reloaded.track, Single);
        assert.deepEqual(Reloaded.objectives, Objectives);
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:110, inverted to our mechanism: the same award and
    // objective sent again (a retry) adds nothing; the award with the objective moved on is a new grant
    it("a replayed objective-backed mastery award does not add its points twice", async () => {
        const { UserId } = await MakePlayer();
        const Tracks = [{ progression_id: SWORD, progress: 2 }];

        Award(UserId, Tracks, [{ objective_id: "CraftSword", value: 1, completed_count: 1 }]);
        Award(UserId, Tracks, [{ objective_id: "CraftSword", value: 1, completed_count: 1 }]);
        assert.equal(GetTrackRecord(UserId, SWORD)!.progress, 2);

        Award(UserId, Tracks, [{ objective_id: "CraftSword", value: 2, completed_count: 2 }]);
        assert.equal(GetTrackRecord(UserId, SWORD)!.progress, 4);
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:121, the no-leak part: a game server reads the account
    // it names; another player's token is refused (403; the fork answered that player's own, empty list)
    it("a game server's objective read gets the named account, and another player cannot read it", async () => {
        const A = await MakePlayer(), B = await MakePlayer();
        Award(A.UserId, [{ progression_id: SWORD, progress: 2 }], [{ objective_id: "CraftSword", value: 1, completed_count: 1 }]);

        const AsServer = await Call("GET", `/progression/objectives/${A.UserId}`, { gs: true });
        assert.equal(AsServer.status, 200);
        assert.ok(Array.isArray(AsServer.json.payload));
        assert.deepEqual([AsServer.json.payload[0].phx_account_id, AsServer.json.payload[0].progress, AsServer.json.payload[0].completed_count], [A.UserId, 1, 1]);

        const AsOther = await Call("GET", `/progression/objectives/${A.UserId}`, { as: B.UserId });
        assert.deepEqual([AsOther.status, AsOther.text], [403, ""]);
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:178: the zero-quantity Slayer cores of PlayerLevel
    // ranks 6, 8 and 10 made the fork's claim fail; here a confirm pays nothing, so they cannot block it
    it("PlayerLevel ranks with zero-quantity rewards in the config can be confirmed", async () => {
        const { UserId, CharacterId } = await MakePlayer();
        Award(UserId, [{ progression_id: PLAYER, progress: 62 }]);
        const Reached = Earned(PLAYER, GetTrackRecord(UserId, PLAYER)!.progress);
        assert.ok(Reached >= 10, `reached ${Reached}`);

        const Reply = ConfirmRank(UserId, PLAYER, String(Reached), "public", "gameserver");
        assert.equal(Reply.Status, 200);
        assert.equal((Reply.Body as any).payload.confirmed_fremium_rank, Reached);
        assert.equal(GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get(), undefined, "no stack, empty or not");
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:195, adapted: a confirm above the earned rank is
    // clamped (the fork answered 400), and a player's own XP award is refused
    it("unearned claims and player-authenticated XP awards stay blocked", async () => {
        const { UserId } = await MakePlayer();

        assert.equal((ConfirmRank(UserId, SWORD, "20", "public", "gameserver").Body as any).payload.confirmed_fremium_rank, 0);

        const Reply = await Call("POST", `/progression/${UserId}`, { as: UserId, body: { progress_tracks: [{ progression_id: SWORD, progress: 999 }] } });
        assert.equal(Reply.status, 403);
        assert.equal(GetTrackRecord(UserId, SWORD)!.progress, 0);
    });

    // from Harmonicrain/Undaunted test/mastery.test.js:204, inverted: the fork refused the whole batch for one bad
    // entry, and the game server does not retry, so the valid XP was lost; ours stores the valid entries
    // and notes the rest
    it("a malformed entry does not throw away the valid entries' XP", async () => {
        const { UserId } = await MakePlayer();

        const Reply = Award(UserId, [{ progression_id: SWORD, progress: 2 }, { progression_id: 42, progress: 1 }, { progression_id: PLAYER }],
            [{ objective_id: "Valid", value: 1, completed_count: 0 }, { value: 3 }]);

        assert.equal(Reply.Status, 200);
        assert.equal(GetTrackRecord(UserId, SWORD)!.progress, 2);
        assert.equal(GetTrackRecord(UserId, PLAYER)!.progress, 0);
        assert.deepEqual(GetObjectiveRecords(UserId).map((Objective) => [Objective.objective_id, Objective.progress]), [["Valid", 1]]);

        const Note = (GetDb().$client.prepare("select note from progression_events where accountId = ? order by id desc").get(UserId) as any).note;
        assert.match(Note, /skipped track entry \{"progression_id":42,"progress":1\}/);
        assert.match(Note, /skipped objective entry \{"value":3\}/);
    });
});
