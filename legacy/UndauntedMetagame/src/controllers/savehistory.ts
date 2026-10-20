import { and, desc, eq, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { characterhistory, characters, loadouthistory, loadouts } from "../db/schema";
import { logger } from "../logger";

type Db = ReturnType<typeof GetDb>;
export type Tx = Parameters<Parameters<Db["transaction"]>[0]>[0];

export type RollbackError = "not_found" | "version_not_found" | "conflict" | "db_error";
export type RollbackResult<T = void> = {success: true, data: T} | {success: false, error: RollbackError};

const DEFAULT_SAVE_HISTORY_KEEP = 100;
const DEFAULT_SAVE_HISTORY_HOURLY = 48;
const DEFAULT_SAVE_HISTORY_DAILY = 30;

// How many of the newest versions of each character's data (and, separately,
// loadouts) to keep. At about two saves a minute that is under an hour of play,
// so the hourly and daily versions below are kept on top of it.
export function GetSaveHistoryKeep(){
    const Keep = Number(process.env.SAVE_HISTORY_KEEP);

    if(!Number.isInteger(Keep) || Keep < 2){
        return DEFAULT_SAVE_HISTORY_KEEP;
    }

    return Keep;
}

function ReadRetention(Value: string | undefined, Default: number){
    const Count = Number(Value);

    if(Value === undefined || Value === "" || !Number.isInteger(Count) || Count < 0){
        return Default;
    }

    return Count;
}

// The last version of each hour for SAVE_HISTORY_HOURLY hours (48) and of each day
// for SAVE_HISTORY_DAILY days (30), like the database backups. Rollback window: any
// version from the last ~50 minutes of play, then hour by hour for two days, then
// day by day for a month. At ~19 KB a version that is at most ~3.5 MB per character.
export function GetSaveHistoryRetention(){
    return {
        Hourly: ReadRetention(process.env.SAVE_HISTORY_HOURLY, DEFAULT_SAVE_HISTORY_HOURLY),
        Daily: ReadRetention(process.env.SAVE_HISTORY_DAILY, DEFAULT_SAVE_HISTORY_DAILY)
    };
}

// Deletes the versions older than OldestKeptId that are not the last one of their
// hour or day inside the retention windows. savedDate is an ISO string (UTC), so
// its first 13 characters are the hour and its first 10 the day.
function PruneBeyondRetention(tx: Tx, Table: typeof characterhistory | typeof loadouthistory, CharacterId: string, OldestKeptId: number){
    const Retention = GetSaveHistoryRetention();
    const Now = Date.now();
    const HourlySince = new Date(Now - Retention.Hourly * 60 * 60 * 1000).toISOString();
    const DailySince = new Date(Now - Retention.Daily * 24 * 60 * 60 * 1000).toISOString();

    tx.run(sql`delete from ${Table} where ${Table.characterId} = ${CharacterId} and ${Table.id} < ${OldestKeptId}
        and ${Table.id} not in (select max(${Table.id}) from ${Table} where ${Table.characterId} = ${CharacterId} and ${Table.savedDate} >= ${HourlySince} group by substr(${Table.savedDate}, 1, 13))
        and ${Table.id} not in (select max(${Table.id}) from ${Table} where ${Table.characterId} = ${CharacterId} and ${Table.savedDate} >= ${DailySince} group by substr(${Table.savedDate}, 1, 10))`);
}

// Same format the characters table has always used for createdDate ("Sep 21, 2026")
export function FormatCharacterDate(When: Date){
    return When.toLocaleDateString("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric"
    });
}

type CharacterRow = typeof characters.$inferSelect;
type LoadoutRow = typeof loadouts.$inferSelect;

function PruneCharacterHistory(tx: Tx, CharacterId: string){
    const Oldest = tx.select({id: characterhistory.id}).from(characterhistory)
        .where(eq(characterhistory.characterId, CharacterId))
        .orderBy(desc(characterhistory.id))
        .limit(1).offset(GetSaveHistoryKeep() - 1).get();

    if(Oldest != undefined){
        PruneBeyondRetention(tx, characterhistory, CharacterId, Oldest.id);
    }
}

function InsertCharacterVersion(tx: Tx, Row: CharacterRow, Reason: string){
    tx.insert(characterhistory).values({
        characterId: Row.characterId,
        userId: Row.userId,
        updateVersion: Row.updateVersion,
        name: Row.name,
        data: Row.data,
        savedDate: new Date().toISOString(),
        reason: Reason
    }).run();
}

// Call with the row as it is before a write. The first write after this code
// ships (or after the history was pruned away) has nothing to roll back to otherwise.
export function EnsureCharacterBaseline(tx: Tx, CurrentRow: CharacterRow){
    const Latest = tx.select({updateVersion: characterhistory.updateVersion}).from(characterhistory)
        .where(eq(characterhistory.characterId, CurrentRow.characterId))
        .orderBy(desc(characterhistory.id)).limit(1).get();

    if(Latest == undefined || Latest.updateVersion !== CurrentRow.updateVersion){
        InsertCharacterVersion(tx, CurrentRow, "baseline");
    }
}

export function RecordCharacterVersion(tx: Tx, NewRow: CharacterRow, Reason: string){
    InsertCharacterVersion(tx, NewRow, Reason);
    PruneCharacterHistory(tx, NewRow.characterId);
}

function PruneLoadoutHistory(tx: Tx, CharacterId: string){
    const Oldest = tx.select({id: loadouthistory.id}).from(loadouthistory)
        .where(eq(loadouthistory.characterId, CharacterId))
        .orderBy(desc(loadouthistory.id))
        .limit(1).offset(GetSaveHistoryKeep() - 1).get();

    if(Oldest != undefined){
        PruneBeyondRetention(tx, loadouthistory, CharacterId, Oldest.id);
    }
}

function LatestLoadoutVersion(tx: Tx, CharacterId: string){
    return tx.select({version: loadouthistory.version, loadouts: loadouthistory.loadouts, persistent: loadouthistory.persistent}).from(loadouthistory)
        .where(eq(loadouthistory.characterId, CharacterId))
        .orderBy(desc(loadouthistory.id)).limit(1).get();
}

function InsertLoadoutVersion(tx: Tx, Row: LoadoutRow, Version: number, Reason: string){
    tx.insert(loadouthistory).values({
        characterId: Row.characterId,
        userId: Row.userId,
        version: Version,
        loadouts: Row.loadouts,
        persistent: Row.persistent,
        savedDate: new Date().toISOString(),
        reason: Reason
    }).run();
}

// Loadout counterpart of EnsureCharacterBaseline
export function EnsureLoadoutBaseline(tx: Tx, CurrentRow: LoadoutRow){
    const Latest = LatestLoadoutVersion(tx, CurrentRow.characterId);

    if(Latest == undefined || Latest.loadouts !== CurrentRow.loadouts || Latest.persistent !== CurrentRow.persistent){
        InsertLoadoutVersion(tx, CurrentRow, (Latest?.version ?? 0) + 1, "baseline");
    }
}

export function RecordLoadoutVersion(tx: Tx, NewRow: LoadoutRow, Reason: string){
    const Latest = LatestLoadoutVersion(tx, NewRow.characterId);

    InsertLoadoutVersion(tx, NewRow, (Latest?.version ?? 0) + 1, Reason);
    PruneLoadoutHistory(tx, NewRow.characterId);
}

export async function GetSaveHistory(CharacterIds: string[]){
    const Result = [];

    for(const CharacterId of CharacterIds){
        const Character = await GetDb().query.characters.findFirst({where: eq(characters.characterId, CharacterId)});

        if(Character == undefined){
            continue;
        }

        const CharacterVersions = await GetDb().select({
            Version: characterhistory.updateVersion,
            SavedDate: characterhistory.savedDate,
            Reason: characterhistory.reason,
            Size: sql<number>`length(${characterhistory.data})`
        }).from(characterhistory).where(eq(characterhistory.characterId, CharacterId)).orderBy(desc(characterhistory.id));

        const LoadoutVersions = await GetDb().select({
            Version: loadouthistory.version,
            SavedDate: loadouthistory.savedDate,
            Reason: loadouthistory.reason
        }).from(loadouthistory).where(eq(loadouthistory.characterId, CharacterId)).orderBy(desc(loadouthistory.id));

        Result.push({
            CharacterId: Character.characterId,
            UserId: Character.userId,
            Name: Character.name,
            UpdateVersion: Character.updateVersion,
            CharacterVersions: CharacterVersions,
            LoadoutVersions: LoadoutVersions
        });
    }

    return Result;
}

export async function GetCharacterIdsForUserId(UserId: string){
    const Rows = await GetDb().select({characterId: characters.characterId}).from(characters).where(eq(characters.userId, UserId));

    return Rows.map((Row) => Row.characterId);
}

class RollbackFailure extends Error {
    constructor(public Reason: RollbackError, message: string){
        super(message);
        this.name = "RollbackFailure";
    }
}

// Puts the data blob of history version TargetVersion back. The row gets a NEW
// updateVersion above anything seen so far, so a stale save still in flight from
// a client or game server is refused (409) instead of undoing the rollback.
// Do this while the player is offline: a running client keeps its own copy.
export function RollbackCharacter(CharacterId: string, TargetVersion: number, Admin: string): RollbackResult<{CharacterId: string, RolledBackTo: number, PreviousVersion: number, UpdateVersion: number}>{
    try{
        return GetDb().transaction((tx) => {
            const Current = tx.select().from(characters).where(eq(characters.characterId, CharacterId)).get();

            if(Current == undefined){
                throw new RollbackFailure("not_found", `Rollback refused: characterId ${CharacterId} does not exist`);
            }

            const Target = tx.select().from(characterhistory)
                .where(and(eq(characterhistory.characterId, CharacterId), eq(characterhistory.updateVersion, TargetVersion)))
                .orderBy(desc(characterhistory.id)).limit(1).get();

            if(Target == undefined){
                throw new RollbackFailure("version_not_found", `Rollback refused: characterId ${CharacterId} has no saved version ${TargetVersion}`);
            }

            EnsureCharacterBaseline(tx, Current);

            const Highest = tx.select({v: sql<number>`max(${characterhistory.updateVersion})`}).from(characterhistory)
                .where(eq(characterhistory.characterId, CharacterId)).get();
            const NewVersion = Math.max(Current.updateVersion, Highest?.v ?? 0) + 1;

            const Updated = tx.update(characters).set({
                data: Target.data,
                updateVersion: NewVersion,
                lastModifiedDate: FormatCharacterDate(new Date())
            }).where(and(eq(characters.characterId, CharacterId), eq(characters.updateVersion, Current.updateVersion))).returning().get();

            if(Updated == undefined){
                throw new RollbackFailure("conflict", `Rollback refused: characterId ${CharacterId} changed while rolling back`);
            }

            RecordCharacterVersion(tx, Updated, `rollback:${TargetVersion}`);

            logger.warn(`Admin ${Admin} rolled back characterId ${CharacterId} of userId ${Current.userId} to version ${TargetVersion} (was updateVersion ${Current.updateVersion}, now ${NewVersion})`);

            return {success: true, data: {CharacterId: CharacterId, RolledBackTo: TargetVersion, PreviousVersion: Current.updateVersion, UpdateVersion: NewVersion}};
        });
    }
    catch(error){
        if(error instanceof RollbackFailure){
            logger.warn(error.message);
            return {success: false, error: error.Reason};
        }

        logger.error(error, `Rollback of characterId ${CharacterId} to version ${TargetVersion} failed`);
        return {success: false, error: "db_error"};
    }
}

export function RollbackLoadout(CharacterId: string, TargetVersion: number, Admin: string): RollbackResult<{CharacterId: string, RolledBackTo: number, Version: number}>{
    try{
        return GetDb().transaction((tx) => {
            const Current = tx.select().from(loadouts).where(eq(loadouts.characterId, CharacterId)).get();

            if(Current == undefined){
                throw new RollbackFailure("not_found", `Loadout rollback refused: characterId ${CharacterId} has no loadouts`);
            }

            const Target = tx.select().from(loadouthistory)
                .where(and(eq(loadouthistory.characterId, CharacterId), eq(loadouthistory.version, TargetVersion)))
                .orderBy(desc(loadouthistory.id)).limit(1).get();

            if(Target == undefined){
                throw new RollbackFailure("version_not_found", `Loadout rollback refused: characterId ${CharacterId} has no saved loadout version ${TargetVersion}`);
            }

            EnsureLoadoutBaseline(tx, Current);

            const Updated = tx.update(loadouts).set({
                loadouts: Target.loadouts,
                persistent: Target.persistent
            }).where(eq(loadouts.characterId, CharacterId)).returning().get();

            RecordLoadoutVersion(tx, Updated!, `rollback:${TargetVersion}`);

            const NewVersion = LatestLoadoutVersion(tx, CharacterId)!.version;

            logger.warn(`Admin ${Admin} rolled back the loadouts of characterId ${CharacterId} of userId ${Current.userId} to loadout version ${TargetVersion} (now loadout version ${NewVersion})`);

            return {success: true, data: {CharacterId: CharacterId, RolledBackTo: TargetVersion, Version: NewVersion}};
        });
    }
    catch(error){
        if(error instanceof RollbackFailure){
            logger.warn(error.message);
            return {success: false, error: error.Reason};
        }

        logger.error(error, `Loadout rollback of characterId ${CharacterId} to version ${TargetVersion} failed`);
        return {success: false, error: "db_error"};
    }
}
