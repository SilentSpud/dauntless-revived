import { GetConfiguredPath, GetConfiguredPaths, ProgressionPath, RankRequirement } from "./progressionconfig";

// Rank math of the 1.4.4 client (0x140b40fa0), on the same config we serve at
// /progression/config (controllers/progressionconfig.ts). The server must agree with the
// client exactly: it clamps confirms to the earned rank, and a clamp that disagrees would
// cut a real claim.

export type { ProgressionPath, RankRequirement };

export type EarnedRanks = {
    EarnedFreeRank: number,
    EarnedPremiumRank: number,
    XpInLevel: number
};

export const INT32_MAX = 2147483647;

export function GetProgressionPaths(): ProgressionPath[]{
    return GetConfiguredPaths();
}

export function GetProgressionPath(ProgressionId: string){
    return GetConfiguredPath(ProgressionId);
}

// Requirements are walked in array order, like the client does (the config keeps them sorted by rank_id)
export function ComputeEarnedRanks(Path: ProgressionPath, Progress: number, HasPremium: boolean): EarnedRanks{
    let Cumulative = 0;
    let EarnedFree = 0;
    let EarnedPremium = 0;
    let XpInLevel = Progress;
    let ReachedEnd = true;

    for(const Requirement of Path.requirements ?? []){
        Cumulative += Requirement.xp_required;

        if(Progress < Cumulative){
            XpInLevel = Progress - (Cumulative - Requirement.xp_required);
            ReachedEnd = false;
            break;
        }

        EarnedFree = Requirement.rank_id;

        if(HasPremium){
            EarnedPremium = Requirement.rank_id;
        }
    }

    if(ReachedEnd){
        const XpPerLevel = Path.prestige?.xp_per_level ?? 0;

        if(Path.prestige != undefined && XpPerLevel > 0){
            const Excess = Progress - Cumulative;

            if(Excess > 0){
                EarnedFree += Math.floor(Excess / XpPerLevel);
                XpInLevel = Excess % XpPerLevel;

                if(HasPremium){
                    EarnedPremium = EarnedFree;
                }
            }
        }
        else{
            XpInLevel = 0;
        }
    }

    return {EarnedFreeRank: EarnedFree, EarnedPremiumRank: EarnedPremium, XpInLevel: XpInLevel};
}

// XP needed for the highest rank_id (prestige levels come after it)
export function TotalXpToMaxRank(Path: ProgressionPath){
    return (Path.requirements ?? []).reduce((Total, Requirement) => Total + Requirement.xp_required, 0);
}

export function MaxRankId(Path: ProgressionPath){
    return (Path.requirements ?? []).reduce((Max, Requirement) => Math.max(Max, Requirement.rank_id), 0);
}

export function PremiumGatingEntitlement(Path: ProgressionPath | undefined){
    return Path?.premium_gating_entitlement ?? "";
}
