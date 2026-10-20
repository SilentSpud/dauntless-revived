import { RemoveTestDb } from "./setup";
import "./authenv";
import { WithEnv } from "./appenv";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { inventory } from "../src/db/schema";
import { GetActiveHuntPass, GetProgressionConfigPayload } from "../src/controllers/progressionconfig";
import { ComputeEarnedRanks, GetProgressionPath, MaxRankId, PremiumGatingEntitlement } from "../src/controllers/progressionrank";
import { ConfirmRank, GetTrackRecord, GrantProgression, TrackRecord } from "../src/controllers/realprogression";
import { GrantEntitlementInTx, ListEntitlements } from "../src/controllers/entitlements";
import { Count, MakePlayer } from "./helpers";

// Harmonic's Hunt Pass cases (github.com/Harmonicrain/Undaunted 895f7c7, test/huntpass.test.js and the
// progression part of test/huntpass-progress.test.js), run against our real progression. The fork's ran on its
// own progression tables and its confirm-time payout; ours derive ranks from the stored XP exactly as the
// client does (progressionrank.ts), and a confirm records the claim and pays nothing (the game server pays
// through /inventory). Each case names the line of the original test; where one asserted behaviour we do not share,
// the case asserts ours and says so. Skipped from huntpass-progress.test.js: 97 (we store a track the
// config does not know, on purpose), 205 (no wallet: the balance test in progressionextras.test.ts
// replaces it), 465 (we serve no bounty definitions; see bountiesported.test.ts).

const SEASON = "season09b";

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

const Earned = (Id: string, Progress: number) => ComputeEarnedRanks(GetProgressionPath(Id)!, Progress, false);
const Award = (UserId: string, Tracks: { progression_id: string, progress: number }[], Objectives: unknown[] = []) => GrantProgression(UserId, { progress_tracks: Tracks, objectives: Objectives }, "gameserver");
const Track = (UserId: string, Id: string) => GetTrackRecord(UserId, Id)!;
const Claim = (UserId: string, Id: string, Rank: number, Kind: "public" | "premium") => ConfirmRank(UserId, Id, String(Rank), Kind, "gameserver");
const Payload = (Reply: any) => Reply.Body.payload as TrackRecord;
const Get = (Path: string, UserId: string) => Call("GET", Path, { as: UserId });
const NoDefaults = <T>(Body: () => T | Promise<T>) => WithEnv({ ENTITLEMENTS_DEFAULT: "" }, Body);

describe("configuration (test/huntpass.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass.test.js:68
    it("the active season is loaded and is a known track", async () => {
        assert.equal(GetActiveHuntPass(), SEASON);
        assert.ok(GetProgressionPath(GetActiveHuntPass()), "the active season exists in the loaded config");

        await WithEnv({ ACTIVE_HUNT_PASS: SEASON }, () => assert.equal(GetActiveHuntPass(), SEASON));
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:75
    it("the config payload carries the season and the mastery tracks", () => {
        const Paths = (GetProgressionConfigPayload() as any).payload.paths;

        assert.ok(Paths.length >= 10, "season plus nine mastery tracks");
        assert.ok(Paths.some((Path: any) => Path.progression_id === SEASON));
        assert.ok(Paths.some((Path: any) => Path.progression_id === "MasteryTrack_PlayerLevel"));
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:83
    it("the premium gate is read from config, not hardcoded", () => {
        assert.equal(PremiumGatingEntitlement(GetProgressionPath(SEASON)), "season09b_premium");
        assert.equal(PremiumGatingEntitlement(GetProgressionPath("MasteryTrack_Behemoth")), "", "no gate: no premium tier");
    });
});

describe("rank derivation (test/huntpass.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass.test.js:91
    it("requirements are per-rank XP, not cumulative", () => {
        assert.equal(Earned(SEASON, 0).EarnedFreeRank, 0);
        assert.equal(Earned(SEASON, 100).EarnedFreeRank, 1);
        assert.equal(Earned(SEASON, 250).EarnedFreeRank, 2);
        assert.equal(Earned(SEASON, 500).EarnedFreeRank, 5);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:100
    it("the XP within a rank is the remainder", () => {
        const Derived = Earned(SEASON, 250);

        assert.equal(Derived.EarnedFreeRank, 2);
        assert.equal(Derived.XpInLevel, 50);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:107, inverted: the fork clamped season09b at 50, which made
    // Claim hand out the prestige reward again and again; the client counts prestige ranks past 50
    it("season09b goes past 50 with prestige; a track without prestige stays at its maximum", () => {
        assert.equal(Earned(SEASON, 5000).EarnedFreeRank, 50);
        assert.equal(Earned(SEASON, 5000 + 250).EarnedFreeRank, 52);
        assert.equal(Earned(SEASON, 99999999).EarnedFreeRank, 50 + Math.floor((99999999 - 5000) / 100));

        const Behemoth = GetProgressionPath("MasteryTrack_Behemoth")!;
        assert.equal(Earned("MasteryTrack_Behemoth", 99999999).EarnedFreeRank, MaxRankId(Behemoth));
        assert.equal(MaxRankId(Behemoth), 50);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:114
    it("a rank costing zero XP is reached immediately", () => {
        assert.ok(Earned("MasteryTrack_PlayerLevel", 0).EarnedFreeRank >= 1);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:120, adapted: stored progress is a whole number of at
    // least 0 (grants clamp at 0), so the fork's NaN case cannot occur here
    it("a negative total does not produce a negative rank", () => {
        assert.equal(Earned(SEASON, -500).EarnedFreeRank, 0);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:125, adapted: an unknown track has no config and no record
    it("an unknown track derives nothing rather than throwing", async () => {
        const { UserId } = await MakePlayer();

        assert.equal(GetProgressionPath("NotATrack"), undefined);
        assert.equal(GetTrackRecord(UserId, "NotATrack"), undefined);
    });
});

describe("the read path (test/huntpass.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass.test.js:131
    it("a player with no progression reads as rank 0 (Slayer level 1), and no row is created", async () => {
        const { UserId } = await MakePlayer();
        const Record = Track(UserId, SEASON);

        assert.deepEqual([Record.progress, Record.confirmed_fremium_rank, Record.confirmed_premium_rank], [0, 0, 0]);
        assert.equal(Earned("MasteryTrack_PlayerLevel", Track(UserId, "MasteryTrack_PlayerLevel").progress).EarnedFreeRank, 1, "Slayer level 1 at 0 XP");
        assert.equal(Count("progress_tracks", "accountId = ?", UserId), 0, "a read does not create rows");
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:144
    it("stored points drive the derived rank", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 1000 }]);

        assert.equal(Track(UserId, SEASON).progress, 1000);
        assert.equal(Earned(SEASON, Track(UserId, SEASON).progress).EarnedFreeRank, 10);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:154
    it("the wire track uses the field names the client parses", async () => {
        const { UserId } = await MakePlayer();
        const Wire = Track(UserId, SEASON);

        // "fremium" is the original spelling, not a typo to fix
        assert.deepEqual(Object.keys(Wire).sort(), ["confirmed_date", "confirmed_fremium_rank", "confirmed_premium_rank", "phx_account_id", "progress", "progression_id"]);
        assert.equal(Wire.progression_id, SEASON);
        assert.ok(!Number.isNaN(Date.parse(Wire.confirmed_date)));
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:170, adapted: premium is the account's
    // season09b_premium entitlement (every account owns it by default; here the defaults are emptied)
    it("premium is withheld without the entitlement and released with it", async () => {
        await NoDefaults(async () => {
            const { UserId } = await MakePlayer();
            Award(UserId, [{ progression_id: SEASON, progress: 200 }]);

            assert.equal(Claim(UserId, SEASON, 2, "premium").Status, 403);
            GetDb().transaction((tx) => GrantEntitlementInTx(tx, UserId, "season09b_premium", 0, "test"));
            assert.equal(Payload(Claim(UserId, SEASON, 2, "premium")).confirmed_premium_rank, 2);
        });
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:183
    it("a track with no gate has no premium tier to unlock", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: "MasteryTrack_Behemoth", progress: 411 }]);

        const Reply = Claim(UserId, "MasteryTrack_Behemoth", 50, "premium");
        assert.equal(Reply.Status, 200);
        assert.equal(Payload(Reply).confirmed_premium_rank, 0);
    });
});

describe("route order (test/huntpass.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass.test.js:191
    it("GET /progression/config is not shadowed by /progression/:userId", async () => {
        const { UserId } = await MakePlayer();
        const Reply = await Get("/progression/config", UserId);

        assert.equal(Reply.status, 200);
        assert.ok(Array.isArray(Reply.json.payload?.paths), "the config, not a progression track");
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:201
    it("GET /progression/objectives/:userId is not taken for a track lookup", async () => {
        const { UserId } = await MakePlayer();
        const Reply = await Get(`/progression/objectives/${UserId}`, UserId);

        assert.equal(Reply.status, 200);
        assert.deepEqual(Reply.json.payload, []);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:213
    it("GET /progression/:userId/:trackId answers instead of 404", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 300 }]);

        const Reply = await Get(`/progression/${UserId}/${SEASON}`, UserId);
        assert.equal(Reply.status, 200);
        assert.equal(Reply.json.payload.progression_id, SEASON);
        assert.equal(Reply.json.payload.progress, 300);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:227
    it("an unknown track is still a 404", async () => {
        const { UserId } = await MakePlayer();

        assert.equal((await Get(`/progression/${UserId}/NotATrack`, UserId)).status, 404);
    });

    // from Harmonicrain/Undaunted test/huntpass.test.js:235
    it("the progression list carries the Hunt Pass track plus every mastery track", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 700 }]);

        const Payload = (await Get(`/progression/${UserId}`, UserId)).json.payload;
        assert.equal(Payload.find((Entry: any) => Entry.progression_id === SEASON).progress, 700, "the season comes from the database");

        const Mastery = Payload.find((Entry: any) => Entry.progression_id === "MasteryTrack_Behemoth");
        assert.deepEqual([Mastery.progress, Mastery.confirmed_fremium_rank], [0, 0]);
        assert.deepEqual(Payload.map((Entry: any) => Entry.progression_id), (GetProgressionConfigPayload() as any).payload.paths.map((Path: any) => Path.progression_id), "every configured track, in config order");
    });
});

describe("XP and claims (test/huntpass-progress.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:76
    it("XP accrues and drives the derived rank", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 250 }]);

        assert.equal(Track(UserId, SEASON).progress, 250);
        assert.equal(Earned(SEASON, 250).EarnedFreeRank, 2);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:87, with the retry guard off: three identical
    // grants in a row within 5 s, with nothing in between, would otherwise count as one grant and two retries
    it("progress is a delta, so repeated awards add up", async () => {
        await WithEnv({ PROGRESSION_REPLAY_WINDOW_S: "0" }, async () => {
            const { UserId } = await MakePlayer();

            for(let Index = 0; Index < 3; Index++){
                Award(UserId, [{ progression_id: SEASON, progress: 100 }]);
            }

            assert.equal(Earned(SEASON, Track(UserId, SEASON).progress).EarnedFreeRank, 3);
        });
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:106, the persistence part: objectives persist.
    // The fork kept the higher of two values; ours stores the game server's absolute value as sent and notes a
    // value that went backwards (see progressionextras.test.ts)
    it("objectives persist", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [], [{ objective_id: "Obj_A", value: 5, completed_count: 1 }]);

        const Stored = (await Get(`/progression/objectives/${UserId}/Obj_A`, UserId)).json.payload;
        assert.deepEqual([Stored.progress, Stored.completed_count], [5, 1]);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:121, inverted: the fork's confirm granted the rank's
    // rewards; the 1.4.4 game server pays them through /inventory, so ours records the claim and grants nothing
    it("confirming a rank records the claim and grants nothing", async () => {
        const { UserId, CharacterId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 300 }]);

        const Reply = Claim(UserId, SEASON, 3, "public");
        assert.equal(Payload(Reply).confirmed_fremium_rank, 3);
        assert.equal(GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get(), undefined, "no TITLE_HP09B_COMMANDO_00, no inventory at all");
        assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 0);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:136, adapted: nothing is granted either time
    it("confirming twice changes nothing the second time", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 300 }]);

        const First = Payload(Claim(UserId, SEASON, 3, "public"));
        const Second = Payload(Claim(UserId, SEASON, 3, "public"));
        assert.deepEqual(Second, First);
        assert.match((GetDb().$client.prepare("select note from progression_events where accountId = ? order by id desc").get(UserId) as any).note, /already confirmed/);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:147
    it("a confirm cannot run ahead of the earned rank", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 200 }]);

        assert.equal(Payload(Claim(UserId, SEASON, 40, "public")).confirmed_fremium_rank, 2);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:160
    it("two accounts can claim the same rank", async () => {
        const First = await MakePlayer(), Second = await MakePlayer();

        for(const Account of [First, Second]){
            Award(Account.UserId, [{ progression_id: SEASON, progress: 100 }]);
            assert.equal(Payload(Claim(Account.UserId, SEASON, 1, "public")).confirmed_fremium_rank, 1);
        }
        assert.deepEqual([Track(First.UserId, SEASON).confirmed_fremium_rank, Track(Second.UserId, SEASON).confirmed_fremium_rank], [1, 1]);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:180 (no default entitlements)
    it("premium ranks are refused without the entitlement", async () => {
        await NoDefaults(async () => {
            const { UserId } = await MakePlayer();
            Award(UserId, [{ progression_id: SEASON, progress: 200 }]);

            assert.equal(Claim(UserId, SEASON, 2, "premium").Status, 403);
            assert.equal(Track(UserId, SEASON).confirmed_premium_rank, 0);
        });
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:190 (the entitlement granted later)
    it("an Elite pass bought late opens the premium ranks already earned", async () => {
        await NoDefaults(async () => {
            const { UserId } = await MakePlayer();
            Award(UserId, [{ progression_id: SEASON, progress: 500 }]);
            Claim(UserId, SEASON, 5, "public");
            assert.equal(Claim(UserId, SEASON, 5, "premium").Status, 403);

            GetDb().transaction((tx) => GrantEntitlementInTx(tx, UserId, "season09b_premium", 0, "test"));

            assert.equal(Payload(Claim(UserId, SEASON, 5, "premium")).confirmed_premium_rank, 5, "all five earned premium ranks at once");
            assert.deepEqual(ListEntitlements(UserId).map((Entitlement) => Entitlement.name), ["season09b_premium"], "and nothing else granted");
        });
    });
});

describe("authorisation and reset (test/huntpass-progress.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:304
    it("a player's token cannot award itself progression", async () => {
        const { UserId } = await MakePlayer();

        const Reply = await Call("POST", `/progression/${UserId}`, { as: UserId, body: { progress_tracks: [{ progression_id: SEASON, progress: 99999 }], objectives: [] } });
        assert.equal(Reply.status, 403);
        assert.equal(Track(UserId, SEASON).progress, 0);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:321, adapted: a reset is a game server's only
    // with PROGRESSION_ALLOW_DELETE=1 (or an admin key); there is no claim ledger to start again
    it("a reset zeroes progress and lets ranks be earned and claimed again", async () => {
        const { UserId } = await MakePlayer();
        Award(UserId, [{ progression_id: SEASON, progress: 200 }]);
        Claim(UserId, SEASON, 2, "public");

        assert.equal((await Call("DELETE", `/progression/${UserId}/${SEASON}`, { gs: true })).status, 403, "not without PROGRESSION_ALLOW_DELETE");

        await WithEnv({ PROGRESSION_ALLOW_DELETE: "1" }, async () => {
            const Reset = await Call("DELETE", `/progression/${UserId}/${SEASON}`, { gs: true });
            assert.equal(Reset.status, 200);
            assert.deepEqual([Reset.json.payload.progress, Reset.json.payload.confirmed_fremium_rank], [0, 0]);
        });

        Award(UserId, [{ progression_id: SEASON, progress: 200 }]);
        assert.equal(Payload(Claim(UserId, SEASON, 2, "public")).confirmed_fremium_rank, 2, "ranks are claimable again");
    });
});
