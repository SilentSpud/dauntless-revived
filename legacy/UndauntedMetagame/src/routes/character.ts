import { Router } from "express";
import { logger } from "../logger";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { CreateCharacterForUid, GetCharactersForUid, GetCharacterWithUid, UpdateCharacterForUid } from "../controllers/character";
import express from "express";

export const characterRouter = Router();

characterRouter.get("/character", HasUndauntedMetagameAuth, async (req: any, res) => {
    const UserId = req.AuthData.userId;

    const CharactersForUid = await GetCharactersForUid(UserId);

    logger.info(`Retrieved ${CharactersForUid.length} characters for ${UserId}`);

    res.status(200);
    res.json(CharactersForUid);
});

characterRouter.put("/character", HasUndauntedMetagameAuth, async (req: any, res) => {
    const CharacterNameToCreate = req.body.name;

    logger.info(`Creating a character named ${CharacterNameToCreate} for user ${req.AuthData.userId}`);

    let NewCharacter = await CreateCharacterForUid(req.AuthData.userId, CharacterNameToCreate);

    res.status(200);
    res.json(NewCharacter);
})

// The client sends a JSON number; a numeric string compared and stored the same way before, so keep accepting it
function ParseUpdateVersion(Value: unknown){
    const Version = typeof Value === "string" && /^\d+$/.test(Value) ? Number(Value) : Value;

    return Number.isSafeInteger(Version) && (Version as number) >= 0 ? Version as number : undefined;
}

characterRouter.post("/character", HasUndauntedMetagameAuth, async (req: any, res) => {
    const CharacterIdToUpdate = req.body.characterId;
    const UserId = req.AuthData.userId;
    const DataToUpdateWith = req.body.data;
    const UpdateVersion = ParseUpdateVersion(req.body.updateVersion);

    logger.info(`Updating characterId ${CharacterIdToUpdate} for userId ${UserId} with updateVersion ${req.body.updateVersion}`);

    if(UpdateVersion == undefined){
        logger.warn(`Refusing update of characterId ${CharacterIdToUpdate} for userId ${UserId}: updateVersion ${JSON.stringify(req.body.updateVersion)} is not a version number`);

        res.status(400);
        res.send();
        return;
    }

    const Result = UpdateCharacterForUid(CharacterIdToUpdate, UserId, DataToUpdateWith, UpdateVersion, req.AuthData.IsGameserver ? "save:gameserver" : "save:client");

    if(!Result.success){
        if(Result.error === "not_found"){
            logger.warn(`Failed to update characterId ${CharacterIdToUpdate} for userId ${UserId}: no such character for this user`);

            res.status(404);
            res.send();
            return;
        }

        if(Result.error === "invalid_data"){
            res.status(400);
            res.send();
            return;
        }

        logger.warn(`Failed to update characterId ${CharacterIdToUpdate} for userId ${UserId} due to conflict (stored updateVersion ${Result.storedVersion}, incoming ${UpdateVersion})`);

        res.status(409);
        res.send();
        return;
    }

    const UpdatedCharacter = await GetCharacterWithUid(CharacterIdToUpdate, UserId);

    res.status(200);
    res.json(UpdatedCharacter);
});