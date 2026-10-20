// Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only; modified for Dauntless Revived.

import registry from "../vendor/escalation/seasons.json";

// The Escalation season registry, exported from the installed client's own
// tables (see the _comment in seasons.json).
//
// The world server does the arithmetic. These values exist so the backend can
// refuse a snapshot that no legitimate native state could produce; they are
// never used to compute a player's progress.

export type EscalationTalent = { id: string, name: string | null, type: string, pointsToUnlock: number, rankCosts: number[] };
export type EscalationUnlock = { id: string, unlockLevel: number, rewardType: string, title: string | null, items: { catalogId: string, quantity: number }[] };
export type EscalationSeason = {
    id: string,
    context: string,
    displayName: string,
    enabled: boolean,
    persistentPowerPerLevel: number,
    levels: { level: number, requiredExperience: number }[],
    talentsTable: string,
    talents: EscalationTalent[],
    unlocks: EscalationUnlock[]
};

const INT32_MAX = 2147483647;

function IsCount(Value: unknown): Value is number {
    return Number.isSafeInteger(Value) && (Value as number) >= 0 && (Value as number) <= INT32_MAX;
}

// Validated once, at module load, so a malformed registry fails the boot
// rather than the first player request.
function Validate(Seasons: EscalationSeason[]){
    const SeasonIds = new Set<string>();

    for(const Season of Seasons){
        if(typeof Season.id !== "string" || Season.id.length === 0 || SeasonIds.has(Season.id)){
            throw new Error(`Escalation registry: missing or duplicate season id ${Season.id}`);
        }
        SeasonIds.add(Season.id);

        // Native leveling walks Levels[level] in order and sets level to
        // Levels[level].Level, so the table must be exactly 1..N.
        Season.levels.forEach((Entry, Index) => {
            if(Entry.level !== Index + 1 || !IsCount(Entry.requiredExperience) || Entry.requiredExperience === 0){
                throw new Error(`Escalation registry: ${Season.id} level table is not 1..N with positive costs`);
            }
        });

        const TalentIds = new Set<string>();
        for(const Talent of Season.talents){
            if(TalentIds.has(Talent.id) || Talent.rankCosts.length === 0 || !Talent.rankCosts.every(IsCount) || !IsCount(Talent.pointsToUnlock)){
                throw new Error(`Escalation registry: ${Season.id} talent ${Talent.id} is duplicate or malformed`);
            }
            TalentIds.add(Talent.id);
        }

        const UnlockIds = new Set<string>();
        for(const Unlock of Season.unlocks){
            if(UnlockIds.has(Unlock.id) || !IsCount(Unlock.unlockLevel) || Unlock.unlockLevel > Season.levels.length){
                throw new Error(`Escalation registry: ${Season.id} unlock ${Unlock.id} is duplicate or out of range`);
            }
            UnlockIds.add(Unlock.id);
        }
    }
}

const Seasons: EscalationSeason[] = (registry as any).seasons;
Validate(Seasons);

const ById = new Map(Seasons.map((Season) => [Season.id, Season]));

export const ESCALATION_CONTENT_REVISION: string = (registry as any).revision;

// Unknown ids never fall back to another season.
export function GetEscalationSeason(SeasonId: string): EscalationSeason | undefined {
    return ById.get(SeasonId);
}

export function GetAllEscalationSeasons(){
    return Seasons;
}

export function MaxEscalationLevel(Season: EscalationSeason){
    return Season.levels.length;
}

// Cost of going from Level to Level + 1, or undefined at the cap.
export function ExperienceForNextLevel(Season: EscalationSeason, Level: number){
    return Season.levels[Level]?.requiredExperience;
}

// GetSpentTalentPoints (0x1414ae2a0): the sum of the first `rank` rank costs.
export function TalentRankCost(Talent: EscalationTalent, Rank: number){
    return Talent.rankCosts.slice(0, Rank).reduce((Total, Cost) => Total + Cost, 0);
}
