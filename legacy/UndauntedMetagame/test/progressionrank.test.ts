import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { ComputeEarnedRanks, GetProgressionPath, GetProgressionPaths, MaxRankId, TotalXpToMaxRank } from "../src/controllers/progressionrank";

const WEAPONS = ["Axe", "ChainBlades", "Hammer", "Repeaters", "Spear", "Sword", "Strikers"].map((Name) => `MasteryTrack_Weapon_${Name}`);

function Earned(ProgressionId: string, Progress: number, HasPremium = false){
    return ComputeEarnedRanks(GetProgressionPath(ProgressionId)!, Progress, HasPremium);
}

describe("progression config", () => {
    it("has the 10 tracks the roadmap lists, requirements sorted by rank_id", () => {
        assert.deepEqual(GetProgressionPaths().map((Path) => Path.progression_id).sort(), ["MasteryTrack_Behemoth", "MasteryTrack_PlayerLevel", ...WEAPONS, "season09b"].sort());

        for(const Path of GetProgressionPaths()){
            const Ranks = Path.requirements!.map((Requirement) => Requirement.rank_id);
            assert.deepEqual(Ranks, [...Ranks].sort((A, B) => A - B), `${Path.progression_id} is not sorted`);
        }
    });

    it("totals: PlayerLevel 1,111, Behemoth 411, weapons 115, season09b 5,000 plus prestige", () => {
        const Totals = (Id: string) => [TotalXpToMaxRank(GetProgressionPath(Id)!), MaxRankId(GetProgressionPath(Id)!)];

        assert.deepEqual(Totals("MasteryTrack_PlayerLevel"), [1111, 50]);
        assert.deepEqual(Totals("MasteryTrack_Behemoth"), [411, 50]);
        for(const Weapon of WEAPONS){
            assert.deepEqual(Totals(Weapon), [115, 20], Weapon);
        }
        assert.deepEqual(Totals("season09b"), [5000, 50]);
        assert.equal(GetProgressionPath("season09b")!.prestige!.xp_per_level, 100);
        assert.equal(GetProgressionPath("season09b")!.premium_gating_entitlement, "season09b_premium");
    });
});

describe("ComputeEarnedRanks (the client's rank math)", () => {
    it("PlayerLevel: progress 0 is rank 1, 1,111 is rank 50, one XP short is 49", () => {
        assert.equal(Earned("MasteryTrack_PlayerLevel", 0).EarnedFreeRank, 1);
        assert.equal(Earned("MasteryTrack_PlayerLevel", 1111).EarnedFreeRank, 50);
        assert.equal(Earned("MasteryTrack_PlayerLevel", 1110).EarnedFreeRank, 49);
        assert.equal(Earned("MasteryTrack_PlayerLevel", 999999).EarnedFreeRank, 50);
    });

    it("weapons: 115 is rank 20, 114 is 19, 0 is 0; Behemoth 411 is 50", () => {
        for(const Weapon of WEAPONS){
            assert.equal(Earned(Weapon, 115).EarnedFreeRank, 20, Weapon);
            assert.equal(Earned(Weapon, 114).EarnedFreeRank, 19, Weapon);
            assert.equal(Earned(Weapon, 0).EarnedFreeRank, 0, Weapon);
        }
        assert.equal(Earned("MasteryTrack_Behemoth", 0).EarnedFreeRank, 0);
        assert.equal(Earned("MasteryTrack_Behemoth", 411).EarnedFreeRank, 50);
    });

    it("season09b: 100 XP per rank, then one prestige level per 100 XP past 5,000", () => {
        // XP in level is only shown by the client; the server decides nothing on it (5,000 exactly is left out)
        const Cases: [number, number, number | undefined][] = [[0, 0, 0], [99, 0, 99], [100, 1, 0], [250, 2, 50], [4999, 49, 99], [5000, 50, undefined], [5099, 50, 99], [5100, 51, 0], [5250, 52, 50]];

        for(const [Progress, Rank, XpInLevel] of Cases){
            const Result = Earned("season09b", Progress);
            assert.equal(Result.EarnedFreeRank, Rank, `progress ${Progress}`);

            if(XpInLevel !== undefined){
                assert.equal(Result.XpInLevel, XpInLevel, `xp in level at ${Progress}`);
            }
        }
    });

    it("premium rank follows the free rank only with premium", () => {
        assert.deepEqual([Earned("season09b", 5100, true).EarnedFreeRank, Earned("season09b", 5100, true).EarnedPremiumRank], [51, 51]);
        assert.equal(Earned("season09b", 5100, false).EarnedPremiumRank, 0);
        assert.equal(Earned("season09b", 300, true).EarnedPremiumRank, 3);
    });

    it("the old stub values: earned never exceeds the stub's confirmed rank, so nothing was pending", () => {
        assert.equal(Earned("season09b", 99999999).EarnedFreeRank, 50 + Math.floor((99999999 - 5000) / 100));
        assert.ok(Earned("season09b", 99999999).EarnedFreeRank <= 99999999);
        assert.equal(Earned("MasteryTrack_Weapon_Sword", 99999999).EarnedFreeRank, 20);
    });
});
