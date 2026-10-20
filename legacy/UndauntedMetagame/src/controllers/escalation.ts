// Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only; modified for Dauntless Revived.

import { createHash } from "node:crypto";
import { and, eq } from "drizzle-orm";
import { GetDb } from "../db";
import { escalationprogression, escalationtalents, escalationunlocks } from "../db/schema";
import { logger } from "../logger";
import { EscalationStrict } from "../features";
import { EscalationSeason, ExperienceForNextLevel, GetEscalationSeason, MaxEscalationLevel, TalentRankCost } from "./escalationConfig";
import { Caller, DoesAccountExist, IsPlainObject, RealReply, RecordProgressionEvent } from "./progressionevents";
import { Tx } from "./savehistory";

// Escalation seasons of real-mode accounts with ESCALATION_MODE=real (roadmap 2.16):
// GetSeasonalEscalationEndpoint and UpdateSeasonalEscalationEndpoint, both
// /escalation/{season_id}/{account_id}.
//
// The world server levels players, spends talent points and grants rewards
// natively, then POSTs the whole season. The backend holds the last legitimate
// snapshot and refuses one that no native state could produce, or that would
// erase newer progress.
//
// Two kinds of rules (docs/findings/escalation.md):
// - hard rules, always enforced: known season, ids and ranks in range, the
//   version order, nothing lowered or un-collected, no stub values;
// - soft rules, which depend on how the client's tier gates, level costs and
//   reward levels were modelled: ESCALATION_STRICT=1 refuses a save that
//   breaks one (409); by default it is stored and logged, so a rule modelled
//   wrongly cannot block every later save of a player.
// Every save, accepted or not, is a row in progression_events.

export type WireTalent = { rank: number, talent_id: string };
export type WireUnlock = { collected: boolean, reward_id: string };
export type WireSeason = {
    escalation_level: number,
    next_level_xp: number,
    talents_progress: WireTalent[],
    unlock_progress: WireUnlock[],
    update_version: number
};

const INT32_MAX = 2147483647;

// The old stub answered level 99999 (clamped natively to the last level) and 99999 XP
const STUB_XP = 99999;

const ROUTE = "POST /escalation/:season/:uid";

class EscalationRefusal extends Error {
    constructor(public Status: number, message: string){
        super(message);
        this.name = "EscalationRefusal";
    }
}

function IsInt32Count(Value: unknown): Value is number {
    return Number.isSafeInteger(Value) && (Value as number) >= 0 && (Value as number) <= INT32_MAX;
}

function Envelope(Payload: WireSeason){
    return {code: null, message: "OK", payload: Payload};
}

function RefusalBody(Status: number, Message: string){
    return {code: String(Status), message: Message, payload: null};
}

function SeasonRow(tx: Tx, AccountId: string, SeasonId: string){
    return tx.select().from(escalationprogression)
        .where(and(eq(escalationprogression.accountId, AccountId), eq(escalationprogression.seasonId, SeasonId))).get();
}

// The native default for an account with no stored season: level 0, no XP,
// nothing bought or collected, version 0. Reads never create rows, and an
// account never inherits the old stub's 99999. Talents and unlocks are listed
// in the season table's order, so identical state always serialises identically.
function ReadState(tx: Tx, AccountId: string, Season: EscalationSeason): WireSeason {
    const Row = SeasonRow(tx, AccountId, Season.id);

    const Talents = tx.select().from(escalationtalents)
        .where(and(eq(escalationtalents.accountId, AccountId), eq(escalationtalents.seasonId, Season.id))).all();

    const Unlocks = tx.select().from(escalationunlocks)
        .where(and(eq(escalationunlocks.accountId, AccountId), eq(escalationunlocks.seasonId, Season.id))).all();

    const RankById = new Map<string, number>(Talents.map((Talent) => [Talent.talentId, Talent.rank]));
    const Collected = new Set<string>(Unlocks.map((Unlock) => Unlock.unlockId));

    return {
        escalation_level: Row?.level ?? 0,
        next_level_xp: Row?.xp ?? 0,
        talents_progress: Season.talents
            .filter((Talent) => RankById.has(Talent.id))
            .map((Talent) => ({rank: RankById.get(Talent.id) as number, talent_id: Talent.id})),
        unlock_progress: Season.unlocks
            .filter((Unlock) => Collected.has(Unlock.id))
            .map((Unlock) => ({collected: true, reward_id: Unlock.id})),
        update_version: Row?.updateVersion ?? 0
    };
}

// The stored season of one account (a new process reads the same state)
export function GetEscalationState(AccountId: string, SeasonId: string): WireSeason | undefined {
    const Season = GetEscalationSeason(SeasonId);

    return Season == undefined ? undefined : GetDb().transaction((tx) => ReadState(tx, AccountId, Season));
}

// GET /escalation/:season/:uid: 404 for a season the client does not have
export function GetEscalationReply(AccountId: string, SeasonId: string): RealReply {
    const State = GetEscalationState(AccountId, SeasonId);

    if(State == undefined){
        logger.warn(`Escalation read of unknown season ${SeasonId} for ${AccountId}`);
        return {Status: 404, Body: RefusalBody(404, `Unknown escalation season ${SeasonId}`)};
    }

    logger.info(`Escalation ${SeasonId} read for ${AccountId}: level ${State.escalation_level}, v${State.update_version}`);

    return {Status: 200, Body: Envelope(State)};
}

// Validates a POSTed snapshot and returns it in canonical form (known ids only,
// season order, rank-0 talents dropped, only collected unlocks kept), with the
// soft rules it breaks. A hard-rule failure throws (400).
export function CanonicaliseSnapshot(Season: EscalationSeason, Body: unknown): { Snapshot: WireSeason, SoftIssues: string[] } {
    if(!IsPlainObject(Body)){
        throw new EscalationRefusal(400, "Escalation snapshot must be an object");
    }

    const SoftIssues: string[] = [];
    const Level = Body.escalation_level;
    const Xp = Body.next_level_xp;
    const Version = Body.update_version;
    const MaxLevel = MaxEscalationLevel(Season);

    if(!IsInt32Count(Level) || Level > MaxLevel){
        throw new EscalationRefusal(400, `escalation_level must be an integer from 0 to ${MaxLevel}`);
    }

    if(!IsInt32Count(Xp)){
        throw new EscalationRefusal(400, "next_level_xp must be a non-negative int32");
    }

    // The native counter is incremented before every POST, so a write is at least version 1
    if(!IsInt32Count(Version) || Version < 1){
        throw new EscalationRefusal(400, "update_version must be a positive int32");
    }

    // Soft: after GrantExperience the remainder is below the next level's cost; only the cap accumulates
    const NextCost = ExperienceForNextLevel(Season, Level);
    if(NextCost !== undefined && Xp >= NextCost){
        SoftIssues.push(`next_level_xp ${Xp} is not below the level ${Level} cost ${NextCost}`);
    }

    if(!Array.isArray(Body.talents_progress) || !Array.isArray(Body.unlock_progress)){
        throw new EscalationRefusal(400, "talents_progress and unlock_progress must be arrays");
    }

    const Ranks = new Map<string, number>();
    for(const Entry of Body.talents_progress as any[]){
        const Talent = Season.talents.find((Candidate) => Candidate.id === Entry?.talent_id);

        if(Talent == undefined){
            throw new EscalationRefusal(400, `Unknown talent ${Entry?.talent_id} for ${Season.id}`);
        }
        if(Ranks.has(Talent.id)){
            throw new EscalationRefusal(400, `Talent ${Talent.id} listed twice`);
        }
        if(!IsInt32Count(Entry.rank) || Entry.rank > Talent.rankCosts.length){
            throw new EscalationRefusal(400, `Talent ${Talent.id} rank must be 0..${Talent.rankCosts.length}`);
        }

        Ranks.set(Talent.id, Entry.rank);
    }

    // Soft: one point per level (GetUnspentTalentPoints = level - spent)
    const Spent = Season.talents.reduce((Total, Talent) => Total + TalentRankCost(Talent, Ranks.get(Talent.id) ?? 0), 0);
    if(Spent > Level){
        SoftIssues.push(`talents spend ${Spent} points but level ${Level} only earns ${Level}`);
    }

    // Soft: SharedUpgradeTalent (0x1414c0600) refuses a rank-up while the points
    // already spent are below the talent's PointsToUnlock. Spent points only
    // grow, so a snapshot is reachable exactly when every held tier is covered
    // by what the lower tiers cost; a talent can never pay for its own gate.
    for(const Talent of Season.talents){
        if((Ranks.get(Talent.id) ?? 0) === 0 || Talent.pointsToUnlock === 0){
            continue;
        }

        const SpentBelow = Season.talents
            .filter((Other) => Other.pointsToUnlock < Talent.pointsToUnlock)
            .reduce((Total, Other) => Total + TalentRankCost(Other, Ranks.get(Other.id) ?? 0), 0);

        if(SpentBelow < Talent.pointsToUnlock){
            SoftIssues.push(`talent ${Talent.id} needs ${Talent.pointsToUnlock} points spent in earlier tiers, found ${SpentBelow}`);
        }
    }

    const Collected = new Set<string>();
    const Listed = new Set<string>();
    for(const Entry of Body.unlock_progress as any[]){
        const Unlock = Season.unlocks.find((Candidate) => Candidate.id === Entry?.reward_id);

        if(Unlock == undefined){
            throw new EscalationRefusal(400, `Unknown unlock ${Entry?.reward_id} for ${Season.id}`);
        }
        if(Listed.has(Unlock.id)){
            throw new EscalationRefusal(400, `Unlock ${Unlock.id} listed twice`);
        }
        if(typeof Entry.collected !== "boolean"){
            throw new EscalationRefusal(400, `Unlock ${Unlock.id} collected must be a boolean`);
        }

        // Soft: a reward is collected at or above its level
        if(Entry.collected && Level < Unlock.unlockLevel){
            SoftIssues.push(`unlock ${Unlock.id} is collected at level ${Level} but needs level ${Unlock.unlockLevel}`);
        }

        Listed.add(Unlock.id);
        if(Entry.collected){
            Collected.add(Unlock.id);
        }
    }

    return {
        Snapshot: {
            escalation_level: Level,
            next_level_xp: Xp,
            talents_progress: Season.talents
                .filter((Talent) => (Ranks.get(Talent.id) ?? 0) > 0)
                .map((Talent) => ({rank: Ranks.get(Talent.id) as number, talent_id: Talent.id})),
            unlock_progress: Season.unlocks
                .filter((Unlock) => Collected.has(Unlock.id))
                .map((Unlock) => ({collected: true, reward_id: Unlock.id})),
            update_version: Version
        },
        SoftIssues: SoftIssues
    };
}

function HashSnapshot(Snapshot: WireSeason){
    const { update_version: _Version, ...Content } = Snapshot;

    return createHash("sha256").update(JSON.stringify(Content)).digest("hex");
}

// POST /escalation/:season/:uid (game server only). Version rules, from the
// native counter that is incremented before every POST and starts from the
// value last loaded:
// - newer version: accepted if it does not lower progress or un-collect a reward;
// - the stored version with the stored content: a retry, answered with the stored state;
// - the stored version with other content, or an older version: 409, so a stale or
//   reordered save can never overwrite newer state.
export function SaveEscalation(AccountId: string, SeasonId: string, Body: unknown, Who: Caller): RealReply {
    const Notes: string[] = [`season ${SeasonId}`];
    const Strict = EscalationStrict();

    return GetDb().transaction((tx) => {
        const Refuse = (Status: number, Why: string) => {
            RecordProgressionEvent(tx, {AccountId, Caller: Who, Route: ROUTE, Body, Status, Notes: [...Notes, Why]});
            logger.warn(`Refusing escalation save of ${SeasonId} for ${AccountId} (${Status}): ${Why}`);
            return {Status: Status, Body: RefusalBody(Status, Why)};
        };

        if(!DoesAccountExist(tx, AccountId)){
            return Refuse(404, "unknown account");
        }

        const Season = GetEscalationSeason(SeasonId);

        if(Season == undefined){
            return Refuse(404, `unknown escalation season ${SeasonId}`);
        }

        if(!Season.enabled){
            return Refuse(409, `escalation season ${SeasonId} is disabled in this client`);
        }

        let Canonical: { Snapshot: WireSeason, SoftIssues: string[] };

        try{
            Canonical = CanonicaliseSnapshot(Season, Body);
        }
        catch(error){
            if(error instanceof EscalationRefusal){
                return Refuse(error.Status, error.message);
            }

            throw error;
        }

        const { Snapshot, SoftIssues } = Canonical;
        const Hash = HashSnapshot(Snapshot);
        const Existing = SeasonRow(tx, AccountId, Season.id);

        if(Existing != undefined){
            if(Snapshot.update_version === Existing.updateVersion && Hash === Existing.contentHash){
                const Stored = Envelope(ReadState(tx, AccountId, Season));

                RecordProgressionEvent(tx, {AccountId, Caller: Who, Route: ROUTE, Body, Status: 200, Reply: Stored, Notes: [...Notes, `replay of version ${Existing.updateVersion}, nothing changed`]});
                logger.info(`Escalation ${Season.id} v${Snapshot.update_version} for ${AccountId} replayed`);

                return {Status: 200, Body: Stored};
            }

            if(Snapshot.update_version === Existing.updateVersion){
                return Refuse(409, `update_version ${Snapshot.update_version} was already saved with other content`);
            }

            if(Snapshot.update_version < Existing.updateVersion){
                return Refuse(409, `stale snapshot: version ${Snapshot.update_version} is older than the stored ${Existing.updateVersion}`);
            }

            // A talent reset may lower ranks; nothing native lowers the level or the XP within a level
            if(Snapshot.escalation_level < Existing.level || (Snapshot.escalation_level === Existing.level && Snapshot.next_level_xp < Existing.xp)){
                return Refuse(409, `snapshot would lower progress from level ${Existing.level} with ${Existing.xp} XP to level ${Snapshot.escalation_level} with ${Snapshot.next_level_xp} XP`);
            }
        }

        // What a world server holds when it loaded the old stub (level 99999 clamped to the last level,
        // 99999 XP, perhaps more XP since). Never a player's first real save, and never a real jump from
        // a stored level below the last one: the game gives a season XP by the hunt, not 99999 at once.
        // Only a player already stored at the last level can reach that much XP (it builds up there).
        // Without this, a world server still holding the stub, whose version counter kept counting while
        // its saves got 404, could overwrite a stored season, and the rule that nothing is lowered would
        // then keep the stub values for good.
        if(Snapshot.escalation_level === MaxEscalationLevel(Season) && Snapshot.next_level_xp >= STUB_XP
            && (Existing == undefined || Existing.level < MaxEscalationLevel(Season))){
            return Refuse(409, `${Existing == undefined ? "first save" : `save over level ${Existing.level}`} carries the old stub values (level ${Snapshot.escalation_level} with ${Snapshot.next_level_xp} XP); the world server must load the season again`);
        }

        const AlreadyCollected = tx.select().from(escalationunlocks)
            .where(and(eq(escalationunlocks.accountId, AccountId), eq(escalationunlocks.seasonId, Season.id))).all()
            .map((Row) => Row.unlockId);

        const NowCollected = new Set(Snapshot.unlock_progress.map((Unlock) => Unlock.reward_id));
        const Lost = AlreadyCollected.filter((UnlockId) => !NowCollected.has(UnlockId));

        if(Lost.length > 0){
            return Refuse(409, `snapshot would un-collect ${Lost.join(", ")}`);
        }

        if(SoftIssues.length > 0){
            if(Strict){
                return Refuse(409, `${SoftIssues.join("; ")} (ESCALATION_STRICT=1)`);
            }

            Notes.push(`accepted although ${SoftIssues.join("; ")} (ESCALATION_STRICT=0)`);
            logger.warn(`Escalation save of ${Season.id} for ${AccountId} breaks a soft rule, stored anyway (ESCALATION_STRICT=0): ${SoftIssues.join("; ")}`);
        }

        const Now = new Date().toISOString();
        const Values = {
            level: Snapshot.escalation_level,
            xp: Snapshot.next_level_xp,
            updateVersion: Snapshot.update_version,
            contentHash: Hash,
            updatedDate: Now
        };

        tx.insert(escalationprogression).values({accountId: AccountId, seasonId: Season.id, ...Values})
            .onConflictDoUpdate({target: [escalationprogression.accountId, escalationprogression.seasonId], set: Values}).run();

        // Ranks are replaced wholesale: a talent reset legitimately drops them
        tx.delete(escalationtalents)
            .where(and(eq(escalationtalents.accountId, AccountId), eq(escalationtalents.seasonId, Season.id))).run();

        for(const Talent of Snapshot.talents_progress){
            tx.insert(escalationtalents).values({accountId: AccountId, seasonId: Season.id, talentId: Talent.talent_id, rank: Talent.rank}).run();
        }

        for(const Unlock of Snapshot.unlock_progress){
            if(!AlreadyCollected.includes(Unlock.reward_id)){
                tx.insert(escalationunlocks).values({accountId: AccountId, seasonId: Season.id, unlockId: Unlock.reward_id, collectedDate: Now}).run();
            }
        }

        const Reply = Envelope(ReadState(tx, AccountId, Season));

        RecordProgressionEvent(tx, {AccountId, Caller: Who, Route: ROUTE, Body, Status: 200, Reply, Notes});
        logger.info(`Escalation ${Season.id} for ${AccountId}: v${Snapshot.update_version} level ${Snapshot.escalation_level} xp ${Snapshot.next_level_xp}, ${Snapshot.talents_progress.length} talent(s), ${Snapshot.unlock_progress.length} collected`);

        return {Status: 200, Body: Reply};
    }, {behavior: "immediate"});
}
