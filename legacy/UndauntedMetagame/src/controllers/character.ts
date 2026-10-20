import { and, eq, lt } from "drizzle-orm";
import { isDeepStrictEqual } from 'node:util';
import { GetDb } from "../db";
import { characters } from "../db/schema";
import { GetUsernameForUserId } from "./login";
import { logger } from "../logger";
import { EnsureCharacterBaseline, FormatCharacterDate, RecordCharacterVersion } from "./savehistory";

const TARGET_CHANGELIST = process.env.TARGET_CHANGELIST;

export type CharacterUpdateError = "not_found" | "conflict" | "invalid_data";
export type CharacterUpdateResult = {success: true} | {success: false, error: CharacterUpdateError, storedVersion?: number};

class CharacterUpdateFailure extends Error {
    constructor(public Reason: CharacterUpdateError, message: string, public StoredVersion?: number){
        super(message);
        this.name = "CharacterUpdateFailure";
    }
}

function TransformDbCharacterToWireCharacter(DbCharacter: any){
    return {
        accountId: DbCharacter.userId,
        createdDate: DbCharacter.createdDate,
        data: DbCharacter.data,
        id: DbCharacter.characterId,
        lastModifiedDate: DbCharacter.lastModifiedDate,
        name: DbCharacter.name,
        updateVersion: DbCharacter.updateVersion
    };
}

export async function DoesCharacterBelongToUserId(UserId: string, CharacterId: string){
    if(typeof UserId !== "string" || typeof CharacterId !== "string"){
        return false;
    }

    const Character = await GetDb().query.characters.findFirst({
        columns: { characterId: true },
        where: and(eq(characters.characterId, CharacterId), eq(characters.userId, UserId))
    });

    return Character != undefined;
}

export async function GetCharactersForUid(userId: string){
    let CharactersFromDb = await GetDb().query.characters.findMany({where: eq(characters.userId, userId)});

    if(CharactersFromDb.length === 0){
        const Username = await GetUsernameForUserId(userId);

        await CreateCharacterForUid(userId, Username);

        CharactersFromDb = await GetDb().query.characters.findMany({where: eq(characters.userId, userId)});
    }

    return CharactersFromDb.map((DbCharacter) => TransformDbCharacterToWireCharacter(DbCharacter));
}

const FTUE_SERIE_KEY = "SERIE_cr19_series_1_ftue";
const FTUE_DOJO_UNLOCK_OBJECTIVE = "929A333B40E413C41E47B0A425EC3349";

// Throws CharacterUpdateFailure("invalid_data") only when the blob itself is not
// a JSON object; storing that would make the character unloadable. A trigger that
// can't read its own inputs is skipped so the save still goes through.
export function ProcessTriggers(CharacterDataToUpdateWith: string){
    let CharacterData: any;

    try{
        CharacterData = JSON.parse(CharacterDataToUpdateWith);
    }
    catch(error){
        throw new CharacterUpdateFailure("invalid_data", `Character data is not valid JSON: ${(error as Error).message}`);
    }

    if(CharacterData == null || typeof CharacterData !== "object" || Array.isArray(CharacterData)){
        throw new CharacterUpdateFailure("invalid_data", `Character data is not a JSON object`);
    }

    try{
        if(CharacterData[FTUE_SERIE_KEY] != undefined){
            const FTUESerieData = JSON.parse(CharacterData[FTUE_SERIE_KEY]);

            if(FTUESerieData?.[FTUE_DOJO_UNLOCK_OBJECTIVE]?.Status === 3 && CharacterData["SERIE_dojo"] == undefined){
                logger.info(`Injecting SERIE_dojo!`);

                CharacterData["SERIE_dojo"] = "{\"ID\":\"Dojo\",\"Status\":0,\"62B91BD94558409B4F7352B5B96F3ED7\":{\"Status\":0,\"6CA2C43B46334BC06F73DEB5F2BFFEC1\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0},\"3A7241AA43743647D3C1E39E8976E4F3\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0}},\"816CBFD94D16EDA252BD1D8461209568\":{\"Status\":1},\"B152371947599B3C2D55BE9B91439C37\":{\"Status\":0,\"D3F19E2248AECEF5C5C3C8B9E2AC2C67\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0},\"0407C2134FE0BEFE3EC791999632D2BC\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0}},\"DFE54F884C6FC60688B6C494D79ADD29\":{\"Status\":0,\"9D1B0D754DBC896034F942AD625F9D93\":{\"Status\":0,\"CurrentAmount\":0,\"LastUpdateAmount\":0}}}";
            }
        }
    }
    catch(error){
        logger.warn(`Skipping the SERIE_dojo trigger, ${FTUE_SERIE_KEY} is unreadable: ${(error as Error).message}`);
    }

    return JSON.stringify(CharacterData);
}

export async function CreateCharacterForUid(userId: string, characterName: string){
    let CharacterUUID = crypto.randomUUID();

    let FormattedCurrentDate = FormatCharacterDate(new Date());

    let NewCharacter = await GetDb().insert(characters).values({
        characterId: CharacterUUID,
        userId: userId,
        name: characterName,
        createdDate: FormattedCurrentDate,
        lastModifiedDate: FormattedCurrentDate,
        updateVersion: 0,
        data: "{}"
    }).returning();

    return TransformDbCharacterToWireCharacter(NewCharacter[0]);
}

// The read, the version check, the write and the history entry are one SQLite
// transaction, so two saves racing for the same version can't both win.
export function UpdateCharacterForUid(CharacterId: string, UserId: string, CharacterDataToUpdateWith: string, UpdateVersion: number, Reason: string = "save"): CharacterUpdateResult{
    try{
        return GetDb().transaction((tx) => {
            const Current = typeof CharacterId === "string" && typeof UserId === "string"
                ? tx.select().from(characters).where(and(eq(characters.characterId, CharacterId), eq(characters.userId, UserId))).get()
                : undefined;

            if(Current == undefined){
                throw new CharacterUpdateFailure("not_found", `characterId ${CharacterId} does not exist or does not belong to userId ${UserId}`);
            }

            // A lost HTTP reply can make the world retry an already committed save.
            // Acknowledge only an identical snapshot at the exact stored version.
            if (Current.updateVersion === UpdateVersion && typeof CharacterDataToUpdateWith === 'string') {
                const Processed = ProcessTriggers(CharacterDataToUpdateWith);
                if (isDeepStrictEqual(JSON.parse(Current.data), JSON.parse(Processed))) return {success:true} as CharacterUpdateResult;
            }
            if(Current.updateVersion >= UpdateVersion){
                throw new CharacterUpdateFailure("conflict", `stored updateVersion ${Current.updateVersion}, incoming ${UpdateVersion}`, Current.updateVersion);
            }

            if(typeof CharacterDataToUpdateWith !== "string"){
                throw new CharacterUpdateFailure("invalid_data", `Character data is a ${CharacterDataToUpdateWith === null ? "null" : typeof CharacterDataToUpdateWith}, not a string`);
            }

            const ProcessedData = ProcessTriggers(CharacterDataToUpdateWith);

            EnsureCharacterBaseline(tx, Current);

            const Updated = tx.update(characters).set({
                data: ProcessedData,
                updateVersion: UpdateVersion,
                lastModifiedDate: FormatCharacterDate(new Date())
            }).where(and(and(eq(characters.userId, UserId), eq(characters.characterId, CharacterId)), lt(characters.updateVersion, UpdateVersion))).returning().get();

            if(Updated == undefined){
                throw new CharacterUpdateFailure("conflict", `the update matched no row`, Current.updateVersion);
            }

            RecordCharacterVersion(tx, Updated, Reason);

            return {success: true} as CharacterUpdateResult;
        });
    }
    catch(error){
        if(error instanceof CharacterUpdateFailure){
            if(error.Reason === "invalid_data"){
                logger.warn(`Refusing update of characterId ${CharacterId} for userId ${UserId} with updateVersion ${UpdateVersion}: ${error.message}`);
            }

            return {success: false, error: error.Reason, storedVersion: error.StoredVersion};
        }

        throw error;
    }
}

export async function GetCharacterWithUid(CharacterId: string, UserId: string){
    const Character = await GetDb().query.characters.findFirst({where: and(eq(characters.characterId, CharacterId), eq(characters.userId, UserId))});

    if(Character == undefined){
        return undefined;
    }

    return TransformDbCharacterToWireCharacter(Character);
}
