import { Router } from "express";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { logger } from "../logger";
import { AddEncounteredContent, GetBreadcrumbsForCharacterIdAndUserId, ProgressionError, QueryEncounteredContent, SetBreadcrumbsForCharacterIdAndUserId } from "../controllers/progression";
import { GetProgressionPath } from "../controllers/progressionrank";
import { IsRealProgressionAccount } from "../controllers/progressionmode";
import { ConfirmRank, GetObjectiveRecord, GetObjectiveRecords, GetTrackRecord, GetTrackRecords, GrantProgression, GrantProgressionInTrack, ResetTrack } from "../controllers/realprogression";
import { CallerOf } from "../controllers/progressionevents";
import { RealProgressionOnly, RefuseForeignPlayer, SendRealReply } from "../middleware/RealProgressionOnly";
import { HasUndauntedAdminApiKey } from "../middleware/HasUndauntedAdminApiKey";
import { AdminMutationRateLimit } from "../middleware/RateLimits";
import { Redact } from "../middleware/BodyLog";
import { NoteRelayedAccountMismatch } from "../middleware/GameServerOnly";

// TODO: We will be gaining progression support very soon, but for now just a stub

export const progressionRouter = Router();

const STUB_MAX_PROGRESS = 99999999;
const STUB_SEASON_RANK = 99999999;

const StubbedMasteryTrackIds = [
    "MasteryTrack_PlayerLevel",
    "MasteryTrack_Behemoth",
    "MasteryTrack_Weapon_Strikers",
    "MasteryTrack_Weapon_Hammer",
    "MasteryTrack_Weapon_Repeaters",
    "MasteryTrack_Weapon_ChainBlades",
    "MasteryTrack_Weapon_Axe",
    "MasteryTrack_Weapon_Sword",
    "MasteryTrack_Weapon_Spear",
];

const STUB_CONFIRMED_DATE = new Date().toISOString();

// The stub's mastery ranks are the configured maximum (controllers/progressionconfig.ts)
function GetConfiguredMaxRank(ProgressionId: string){
    const ProgressionPath = GetProgressionPath(ProgressionId);

    if(!ProgressionPath?.requirements?.length){
        return STUB_SEASON_RANK;
    }

    return Math.max(...ProgressionPath.requirements.map((Requirement) => Requirement.rank_id));
}

function StubbedMasteryProgressTrackTemplates(){
    return StubbedMasteryTrackIds.map((ProgressionId) => {
        const ConfirmedRank = GetConfiguredMaxRank(ProgressionId);

        return {
            progression_id: ProgressionId,
            progress: STUB_MAX_PROGRESS,
            confirmed_fremium_rank: ConfirmedRank,
            confirmed_premium_rank: ConfirmedRank,
            confirmed_date: STUB_CONFIRMED_DATE,
        };
    });
}

function StubbedProgressTrackTemplates(){
    return [
        {
            progression_id: "season09b",
            progress: STUB_MAX_PROGRESS,
            confirmed_fremium_rank: STUB_SEASON_RANK,
            confirmed_premium_rank: STUB_SEASON_RANK,
            confirmed_date: STUB_CONFIRMED_DATE,
        },
        ...StubbedMasteryProgressTrackTemplates(),
    ];
}

function StatusForProgressionError(Error: ProgressionError){
    switch(Error){
        case "forbidden":
            return 403;
        case "conflict":
            return 409;
        case "invalid_data":
        case "db_error":
            return 500;
    }
}

progressionRouter.get("/encountered-content/:characterId/:contentType", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestorAccountId = req.AuthData.userId;
    const CharacterId = req.params.characterId;
    const ContentType = req.params.contentType as number;

    logger.info(`Querying encountered content for userId ${RequestorAccountId} and characterId ${CharacterId}`);

    const ContentResult = await QueryEncounteredContent(RequestorAccountId, CharacterId, [ContentType]);

    if(!ContentResult.success){
        res.status(StatusForProgressionError(ContentResult.error));
        res.send();
        return;
    }

    res.status(200);
    res.send({
        code: null,
        message: "OK",
        payload: {
            content_types: ContentResult.data,
            success: true
        }
    });
});

progressionRouter.post("/encountered-content/query/:characterId", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestorAccountId = req.AuthData.userId;
    const CharacterId = req.params.characterId;
    const ContentTypes = req.body.content_types;

    logger.info(`Querying encountered content for userId ${RequestorAccountId} and characterId ${CharacterId}`);

    const ContentResult = await QueryEncounteredContent(RequestorAccountId, CharacterId, ContentTypes);

    if(!ContentResult.success){
        res.status(StatusForProgressionError(ContentResult.error));
        res.send();
        return;
    }

    res.status(200);
    res.send({
        code: null,
        message: "OK",
        payload: {
            content_types: ContentResult.data,
            success: true
        }
    });
});

progressionRouter.post("/encountered-content/:characterId", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestorAccountId = req.AuthData.userId;
    const CharacterId = req.params.characterId;
    const ContentType = req.body.content_type;
    const ContentId = req.body.content_id;

    logger.info(`Adding encountered content ${ContentId} for userId ${RequestorAccountId} and characterId ${CharacterId}`);

    const ContentResult = await AddEncounteredContent(RequestorAccountId, CharacterId, ContentType, ContentId);

    if(!ContentResult.success){
        res.status(StatusForProgressionError(ContentResult.error));
        res.send();
        return;
    }

    res.status(200);
    res.send({
        code: null,
        message: "OK",
        payload: {}
    });
});

progressionRouter.get("/progression/objectives/:userId", HasUndauntedMetagameAuth, (req: any, res) => {
    const RequestorAccountId = req.AuthData.userId;

    // Real mode: every stored objective as a payload ARRAY (int code). The stub's
    // payload object fails the client's array check, so it never had any objectives.
    if(IsRealProgressionAccount(req.params.userId)){
        if(RefuseForeignPlayer(req, res)){
            return;
        }

        const Objectives = GetObjectiveRecords(req.params.userId);

        logger.info(`Objective progression fetched for userId ${req.params.userId}: ${Objectives.length} objective(s)`);

        res.status(200);
        res.json({
            code: 200,
            message: "OK",
            payload: Objectives
        });
        return;
    }

    logger.info(`Objective progression fetched for userId ${RequestorAccountId}`);
    
    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: {
            objectives: [
                
            ],
            progress_tracks: StubbedMasteryProgressTrackTemplates().map((TrackTemplate) => ({ phx_account_id: RequestorAccountId, ...TrackTemplate }))
        }
    })
});

progressionRouter.get("/progression/objectives/:userId/:objectiveId", HasUndauntedMetagameAuth, (req: any, res) => {
    const RequestorAccountId = req.AuthData.userId;

    // Real mode: the stored objective, or zeros. The client asks for each objective of
    // a grant and raises the "objective updated" event for every answer it gets.
    if(IsRealProgressionAccount(req.params.userId)){
        if(RefuseForeignPlayer(req, res)){
            return;
        }

        logger.info(`Objective ${req.params.objectiveId} fetched for userId ${req.params.userId}`);

        res.status(200);
        res.json({
            code: "OK",
            message: "OK",
            payload: GetObjectiveRecord(req.params.userId, req.params.objectiveId)
        });
        return;
    }

    logger.info(`Objective progression fetched for userId ${RequestorAccountId}`);
    
    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: {
            phx_account_id: req.params.userId,
            objective_id: req.params.objectiveId,
            progress: 9999999,
            completed_count: 9999999,
            created_date: new Date("1970-1-1").toISOString(),
            last_modified_date: new Date("1970-1-1").toISOString(),
        }
    })
});

progressionRouter.get("/breadcrumbs/:characterId", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestedCharacterId = req.params.characterId;
    const RequestorUserId = req.AuthData.userId;

    logger.info(`Requested breadcrumbs for characterId ${RequestedCharacterId}`);

    const BreadcrumbsResult = await GetBreadcrumbsForCharacterIdAndUserId(RequestorUserId, RequestedCharacterId);

    if(!BreadcrumbsResult.success){
        res.status(StatusForProgressionError(BreadcrumbsResult.error));
        res.send();
        return;
    }

    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: BreadcrumbsResult.data
    });
});

progressionRouter.post("/breadcrumbs/:characterId", HasUndauntedMetagameAuth, async (req: any, res) => {
    const RequestedCharacterId = req.params.characterId;
    const RequestorUserId = req.AuthData.userId;
    const BreadcrumbsFromUser = req.body.breadcrumbs;
    const UpdateVersion = req.body.updateVersion;

    logger.info(`Setting breadcrumbs for characterId ${RequestedCharacterId}`);

    const BreadcrumbsResult = await SetBreadcrumbsForCharacterIdAndUserId(RequestorUserId, RequestedCharacterId, BreadcrumbsFromUser, UpdateVersion);

    if(!BreadcrumbsResult.success){
        res.status(StatusForProgressionError(BreadcrumbsResult.error));
        res.send();
        return;
    }

    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: BreadcrumbsResult.data
    });
});

progressionRouter.post("/progression/:userId", HasUndauntedMetagameAuth, (req: any, res) => {
    const RequestorAccountId = req.params.userId;
    
    // Real mode: store the grant and objectives, answer with the new totals. The pop
    // that made upstream answer 400 came from never storing objectives: every
    // successful grant made the client re-read unchanged zeros and notify again.
    if(IsRealProgressionAccount(RequestorAccountId)){
        if(!req.AuthData.IsGameserver){
            logger.warn(`Refusing progression grant for ${RequestorAccountId} from a player client`);
            res.status(403);
            res.send();
            return;
        }

        NoteRelayedAccountMismatch(req, RequestorAccountId, "progression grant");
        SendRealReply(res, GrantProgression(RequestorAccountId, req.body, CallerOf(req)));
        return;
    }

    logger.info(`Progression set for userId ${RequestorAccountId} (stubbed)`);
    
    res.status(400); // TODO: Figure out how to properly grant progression. If this returns anything other than 400, we get the infinite mastery pop issue
    res.send();
});

// The routes below only exist for real-mode accounts (RealProgressionOnly); for
// everyone else they fall through to the 404 they always got.

// PROGRESSION_CONFIRM=off gives real-mode accounts the old 404 here (roadmap 2.12 runs
// one session with confirm off to tell the two causes of the mastery pop apart)
function ConfirmOn(req: any, res: any, next: any){
    next(process.env.PROGRESSION_CONFIRM === "off" ? "route" : undefined);
}

progressionRouter.post("/progression/:userId/:progressionId/:rank/confirm/:kind", RealProgressionOnly, ConfirmOn, HasUndauntedMetagameAuth, (req: any, res) => {
    if(!req.AuthData.IsGameserver){
        logger.warn(`Refusing rank confirm for ${req.params.userId} from a player client`);
        res.status(403);
        res.send();
        return;
    }

    NoteRelayedAccountMismatch(req, req.params.userId, "rank confirm");
    SendRealReply(res, ConfirmRank(req.params.userId, req.params.progressionId, req.params.rank, req.params.kind, CallerOf(req)));
});

progressionRouter.post("/progression/:userId/:progressionId/:amount", RealProgressionOnly, HasUndauntedMetagameAuth, (req: any, res) => {
    if(!req.AuthData.IsGameserver){
        logger.warn(`Refusing progression grant in ${req.params.progressionId} for ${req.params.userId} from a player client`);
        res.status(403);
        res.send();
        return;
    }

    NoteRelayedAccountMismatch(req, req.params.userId, "progression grant in a track");
    SendRealReply(res, GrantProgressionInTrack(req.params.userId, req.params.progressionId, req.params.amount, CallerOf(req)));
});

progressionRouter.get("/progression/:userId/:progressionId", RealProgressionOnly, HasUndauntedMetagameAuth, (req: any, res) => {
    if(RefuseForeignPlayer(req, res)){
        return;
    }

    const Track = GetTrackRecord(req.params.userId, req.params.progressionId);

    if(Track == undefined){
        res.status(404);
        res.send();
        return;
    }

    res.status(200);
    res.json({
        code: "OK",
        message: "OK",
        payload: Track
    });
});

// Resets one track. The game server only sends it from a debug command, so it needs
// an admin key (x-undaunted-user-api-key), or PROGRESSION_ALLOW_DELETE=1 for game servers.
progressionRouter.delete("/progression/:userId/:progressionId", AdminMutationRateLimit, RealProgressionOnly, async (req: any, res) => {
    if(req.headers["x-undaunted-user-api-key"] !== undefined){
        await HasUndauntedAdminApiKey(req, res, () => SendRealReply(res, ResetTrack(req.params.userId, req.params.progressionId, "admin")));
        return;
    }

    await HasUndauntedMetagameAuth(req, res, () => {
        if(!req.AuthData.IsGameserver || process.env.PROGRESSION_ALLOW_DELETE !== "1"){
            logger.warn(`Refusing reset of track ${req.params.progressionId} of ${req.params.userId}: needs an admin key, or PROGRESSION_ALLOW_DELETE=1 for game servers`);
            res.status(403);
            res.send();
            return;
        }

        SendRealReply(res, ResetTrack(req.params.userId, req.params.progressionId, CallerOf(req)));
    });
});

progressionRouter.get("/progression/:userId", HasUndauntedMetagameAuth, (req: any, res) => {
    const RequestorAccountId = req.AuthData.userId;

    // Real mode: every configured track, stored or at 0, with phx_account_id = the URL
    // account (the stub uses the token's user, which a game server may not carry)
    if(IsRealProgressionAccount(req.params.userId)){
        if(RefuseForeignPlayer(req, res)){
            return;
        }

        logger.info(`Progression fetched for userId ${req.params.userId}`);

        res.status(200);
        res.json({
            code: 200,
            message: "OK",
            payload: GetTrackRecords(req.params.userId)
        });
        return;
    }

    // TODO: Impl proper progression. Right now this is the minimum to not block the Boreal crafting reqs

    logger.info(`Progression fetched for userId ${RequestorAccountId} (stubbed)`);
    
    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: StubbedProgressTrackTemplates().map((TrackTemplate) => ({ phx_account_id: RequestorAccountId, ...TrackTemplate }))
    })
});

// Anything under /progression that no route above answered goes on to the catch-all 404; this names it
// first (from Harmonic's fork, github.com/Harmonicrain/Undaunted 895f7c7): several progression endpoints of
// the client are still unanswered, and stub-mode accounts get 404 on the real-mode-only routes.
progressionRouter.use("/progression", (req: any, res, next) => {
    logger.warn(`Unhandled progression request ${req.method} ${Redact(req.originalUrl)} from a ${req.headers["x-undaunted-gameserver-apikey"] !== undefined ? "game server" : "player"}`);
    next();
});
