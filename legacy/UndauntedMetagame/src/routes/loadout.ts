import { Router } from "express";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { GetAllLoadoutsForUserIdAndCharacterId, GetPersistentLoadoutForUserIdAndCharacterId, GetSlotCountReply, RealLoadoutPayload, SetActiveLoadoutSlot, SetLoadoutDataForUserIdAndCharacterId, SetLoadoutSlotData, UnlockLoadoutSlots } from "../controllers/loadout";
import { DoesCharacterBelongToUserId } from "../controllers/character";
import { logger } from "../logger";
import { IsRealProgressionAccount } from "../controllers/progressionmode";
import { CallerOf } from "../controllers/progressionevents";
import { RealProgressionOnly, SendRealReply } from "../middleware/RealProgressionOnly";
import { RefuseUnlessGameserver } from "../middleware/GameServerOnly";

export const loadoutRouter = Router();

// Loadout slots (unlock, slot count, active slot) only exist for real-mode accounts;
// everyone else falls through to the 404 they always got. The game server's
// unlock/{n} asks for n MORE slots; see controllers/loadout.ts.
async function ResolveRealLoadoutCharacter(req: any, res: any){
    const RequestorAccountId = req.AuthData.IsGameserver ? req.params.userId : req.AuthData.userId;
    const CharacterId = req.params.characterId;

    if(!IsRealProgressionAccount(RequestorAccountId)){
        return undefined;
    }

    if(!await DoesCharacterBelongToUserId(RequestorAccountId, CharacterId)){
        logger.warn(`Refusing loadout slots of characterId ${CharacterId} to userId ${RequestorAccountId}: no such character for this user`);

        res.status(404);
        res.send();
        return null;
    }

    return {AccountId: RequestorAccountId as string, CharacterId: CharacterId as string};
}

// Unlocks and the active slot are the game server's (unlock follows the level and quest
// conditions it checks); a player's own client may not set them (middleware/GameServerOnly.ts)
loadoutRouter.post("/loadout/:userId/:characterId/unlock/:numSlots", RealProgressionOnly, HasUndauntedMetagameAuth, async (req: any, res, next) => {
    const Target = await ResolveRealLoadoutCharacter(req, res);

    if(Target === undefined){
        next("route");
        return;
    }

    if(Target !== null && !RefuseUnlessGameserver(req, res, "loadout slot unlock", `characterId ${req.params.characterId}`)){
        SendRealReply(res, UnlockLoadoutSlots(Target.AccountId, Target.CharacterId, req.params.numSlots, CallerOf(req)));
    }
});

loadoutRouter.get("/loadout/:userId/:characterId/slotcount", RealProgressionOnly, HasUndauntedMetagameAuth, async (req: any, res, next) => {
    const Target = await ResolveRealLoadoutCharacter(req, res);

    if(Target === undefined){
        next("route");
        return;
    }

    if(Target !== null){
        res.status(200);
        res.json(GetSlotCountReply(Target.CharacterId));
    }
});

loadoutRouter.post("/loadout/:userId/:characterId/active/:index", RealProgressionOnly, HasUndauntedMetagameAuth, async (req: any, res, next) => {
    const Target = await ResolveRealLoadoutCharacter(req, res);

    if(Target === undefined){
        next("route");
        return;
    }

    if(Target !== null && !RefuseUnlessGameserver(req, res, "active loadout slot", `characterId ${req.params.characterId}`)){
        SendRealReply(res, SetActiveLoadoutSlot(Target.AccountId, Target.CharacterId, req.params.index, CallerOf(req)));
    }
});

// Real mode: the stored slot counts and active slot, one row for /all, /slotcount and unlock
async function LoadoutPayload(RequestorAccountId: string, CharacterId: string){
    const Loadouts: any[] = await GetAllLoadoutsForUserIdAndCharacterId(RequestorAccountId, CharacterId);
    const Persistent: any = await GetPersistentLoadoutForUserIdAndCharacterId(RequestorAccountId, CharacterId);

    logger.info(`Fetched ${Loadouts.length} loadout(s) for userId ${RequestorAccountId} and characterId ${CharacterId}`);

    if(IsRealProgressionAccount(RequestorAccountId)){
        return RealLoadoutPayload(CharacterId, Loadouts, Persistent);
    }

    return {
        loadouts: Loadouts,
        persistent: Persistent,
        num_account_slots: 1, // TODO: Make these loadout slots add-able, support multiple loadouts
        max_account_slots: 1,
        num_character_slots: 1,
        max_character_slots: 1,
        active_index: 0, // TODO: Make this support multi-loadouts
        needs_migration: false
    };
}

loadoutRouter.get("/loadout/:userId/:characterId/all", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestorAccountId = req.AuthData.IsGameserver ? req.params.userId : req.AuthData.userId;
    const CharacterId = req.params.characterId;

    // Upstream created a loadout row for any id it was given, and crashed on the primary key when the character was someone else's
    if(!await DoesCharacterBelongToUserId(RequestorAccountId, CharacterId)){
        logger.warn(`Refusing loadouts of characterId ${CharacterId} to userId ${RequestorAccountId}: no such character for this user`);

        res.status(404);
        res.send();
        return;
    }

    const Payload = await LoadoutPayload(RequestorAccountId, CharacterId);

    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: Payload
    });
});

loadoutRouter.post("/loadout/:userId/:characterId/:index", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestorAccountId = req.AuthData.IsGameserver ? req.params.userId : req.AuthData.userId;
    const CharacterId = req.params.characterId;
    const Data = req.body.data;
    const Index = req.params.index;

    if(!await DoesCharacterBelongToUserId(RequestorAccountId, CharacterId)){
        logger.warn(`Failed to update loadout index ${Index} for userId ${RequestorAccountId} and characterId ${CharacterId}: no such character for this user`);

        res.status(404);
        res.send();
        return;
    }

    // Real mode saves any unlocked slot by its slot index; the stub only knows slot 0 and persistent
    const Reason = req.AuthData.IsGameserver ? "save:gameserver" : "save:client";
    const Success = IsRealProgressionAccount(RequestorAccountId)
        ? SetLoadoutSlotData(RequestorAccountId, CharacterId, Index, Data, Reason)
        : SetLoadoutDataForUserIdAndCharacterId(RequestorAccountId, CharacterId, Index, Data, Reason);

    if(Success){
        logger.info(`Successfully updated loadout index ${Index} for userId ${RequestorAccountId} and characterId ${CharacterId}`);
        // TODO: RE success shape, below is a complete guess

        const Payload = await LoadoutPayload(RequestorAccountId, CharacterId);

        res.status(200);
        res.json({
            code: null,
            message: "OK",
            payload: Payload
        });
    }
    else{
        logger.error(`Failed to update loadout index ${Index} for userId ${RequestorAccountId} and characterId ${CharacterId}`);

        res.status(400);
        res.send();
    }
});
