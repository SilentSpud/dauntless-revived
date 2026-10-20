import { Router } from "express";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { logger } from "../logger";
import { GameSessionForCandidate } from "../controllers/matchmaking";
import { CancelMatchmaking, CheckAndUpdateQueueStatus, DecideCandidateStatus, HandlePlayerMatchmaking, JoinPartyCandidateById, LeaveCandidate, MatchmakingResult } from "../controllers/matchmaking";
import { MatchmakingRoster } from "../controllers/matchmakingroster";

export const matchmakingRouter = Router();

// MISC_ROUTES=0 puts back the 404 /candidate/player/alive had since upstream
function MiscRoutesOn(req: any, res: any, next: any){
    next(process.env.MISC_ROUTES === "0" ? "route" : undefined);
}

// Off unless MATCHMAKING_CANCEL=1: the client sends DELETE /candidate 0.2-2.1 s after
// every queued /candidate/join in the logs (4 of 4 hunts, all played to the end), and
// those hunts only started because the cancel 404'd and the queue popped anyway.
// Until a session shows when the client really means it, it keeps the upstream 404.
function CancelOn(req: any, res: any, next: any){
    next(process.env.MATCHMAKING_CANCEL === "1" ? undefined : "route");
}

// The client's cancel. Its reply is only logged by the client; any JSON object works.
// A party leader's cancel also calls off the party's candidate (controllers/matchmaking.ts).
matchmakingRouter.delete("/candidate", CancelOn, HasUndauntedMetagameAuth, (req: any, res) => {
    const UserId = req.AuthData.userId;
    const Cancelled = typeof UserId === "string" ? CancelMatchmaking(UserId) : undefined;

    logger.info(Cancelled != undefined ? `userId ${UserId} cancelled matchmaking for ${Cancelled.HuntId}${Cancelled.Ready ? " (a server was already assigned)" : ""}` : `userId ${UserId} cancelled matchmaking but was not queued`);

    res.status(200);
    res.json({});
});

// The other cancel the client has (never seen in the logs): the caller alone leaves the
// candidate. Same switch as DELETE /candidate, since a party member's client may send it
// automatically the way the leader's sends DELETE /candidate; the 404 stays the default.
matchmakingRouter.delete("/candidate/leave", CancelOn, HasUndauntedMetagameAuth, (req: any, res) => {
    const UserId = req.AuthData.userId;
    const Left = typeof UserId === "string" ? LeaveCandidate(UserId) : undefined;

    logger.info(Left != undefined ? `userId ${UserId} left matchmaking for ${Left.HuntId}` : `userId ${UserId} left matchmaking but was not queued`);

    res.status(200);
    res.json({});
});

function NativeSessionId(value: unknown) {
    return typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value)
        ? value.toLowerCase() : undefined;
}

function ConnectedPlayerSnapshot(value: unknown): string[] | undefined {
    if (typeof value !== 'string' || Buffer.byteLength(value, 'utf8') > 8192) return undefined;
    try {
        const ids: unknown = JSON.parse(value);
        if (!Array.isArray(ids) || ids.length > 128
            || ids.some(id => typeof id !== 'string' || !/^[\x21-\x7e]{1,128}$/.test(id))
            || new Set(ids).size !== ids.length) return undefined;
        return ids;
    } catch {
        return undefined;
    }
}

// The native request contains the complete expected roster, including players who
// already arrived. Only a complete connection snapshot from that deployment can
// distinguish a missing player from a connected player who stopped matchmaking.
// An absent/incomplete snapshot preserves unresolved members; legacy or unknown
// deployments retain their roster, including after a metagame restart.
matchmakingRouter.post("/candidate/player/alive", MiscRoutesOn, HasUndauntedMetagameAuth, (req: any, res) => {
    const PlayerIds = Array.isArray(req.body?.playerIds) ? req.body.playerIds.filter((Id: unknown) => typeof Id === "string") : [];
    const SessionHeader = req.headers['x-dauntless-game-session-id'];
    const HeaderSessionId = NativeSessionId(SessionHeader);
    const QuerySessionId = NativeSessionId(req.query.sessionId);
    const SessionId = SessionHeader === undefined ? QuerySessionId : HeaderSessionId;
    const MatchingSession = req.query.sessionId === undefined || QuerySessionId === SessionId;
    const ExpectedPurpose = req.query.purpose === 'expected' || (HeaderSessionId !== undefined && req.query.purpose === undefined);
    const ExpectedPlayerIds = req.AuthData.IsGameserver === true && SessionId && MatchingSession && ExpectedPurpose
        ? MatchmakingRoster.reconcileExpected(PlayerIds, {
            sessionId: SessionId,
            connectedPlayerIds: HeaderSessionId ? ConnectedPlayerSnapshot(req.headers['x-dauntless-connected-player-ids']) : undefined
        }) : PlayerIds;

    res.status(200);
    res.json({
        expectedPlayerIds: ExpectedPlayerIds
    });
});

const QOS_TARGET_URL = process.env.QOS_TARGET_URL;
const TARGET_CHANGELIST = process.env.TARGET_CHANGELIST;

matchmakingRouter.post("/candidate/player/register", HasUndauntedMetagameAuth, (req: any, res) => {
    logger.info(`userId ${req.AuthData.userId} is registering for matchmaking!`);

    res.status(200);
    res.json({});
});

matchmakingRouter.get("/candidate/regions", HasUndauntedMetagameAuth, (req: any, res) => {
    logger.info(`Querying regions for QoS (userId ${req.AuthData.userId})`);

    res.status(200);
    res.json({
        code: 200,
        message: "success",
        payload: {
            maxPingingStepTime: 3,
            pingCount: 5,
            pingFrequency: 0.25,
            regionUrls: [
                QOS_TARGET_URL
            ]
        }
    });
});

matchmakingRouter.post("/key/generate", HasUndauntedMetagameAuth, async (req: any, res) => {
    res.status(400);
    res.send();
});

// playerStates is only read by the client's newer matchmaker (isNewMatchmaker, never sent
// here). Solo replies keep upstream's literal "UserId" key; a party candidate lists its
// members by id, as the live service did.
function PlayerStatesOf(MatchmakingResult: MatchmakingResult): Record<string, {}> {
    if(MatchmakingResult.PartyMemberIds !== undefined){
        return Object.fromEntries(MatchmakingResult.PartyMemberIds.map((MemberId) => [MemberId, {}]));
    }

    return { UserId: {} };
}

matchmakingRouter.get("/candidate/status", HasUndauntedMetagameAuth, async (req: any, res) => {
    const UserId = req.AuthData.userId;

    const Decision = await DecideCandidateStatus(UserId);

    if(Decision.Kind !== "unknown"){
        const MatchmakingResult = Decision.Entry;
        const Party = MatchmakingResult.PartyCandidate ? `, party candidate ${MatchmakingResult.CandidateId}` : "";

        if(Decision.Kind === "failed"){
            // A status the client knows ends matchmaking; the old reply sent it to ":0"
            logger.warn(`Telling userId ${UserId} matchmaking FAILED for ${MatchmakingResult.HuntId}${Party}`);

            res.status(200);
            res.json({
                candidateId: MatchmakingResult.CandidateId,
                candidateStatusPeriodMillis: MatchmakingResult.Private || MatchmakingResult.PartyCandidate ? 1000 : 10000,
                gameMode: "ISLAND",
                huntId: MatchmakingResult.HuntId,
                playerStates: PlayerStatesOf(MatchmakingResult),
                status: "FAILED",
                statusDuration: 0.0,
                statusReason: null
            });
        }
        else if(Decision.Kind === "travel"){
            logger.info(`Telling client to travel to ${MatchmakingResult.Host}:${MatchmakingResult.Port} (userId ${UserId}${Party})`);

            res.status(200);
            res.json({
                candidateId: MatchmakingResult.CandidateId,
                candidateStatusPeriodMillis: MatchmakingResult.Private || MatchmakingResult.PartyCandidate ? 1000 : 10000,
                gameMode: "ISLAND",
                huntId: MatchmakingResult.HuntId,
                playerStates: PlayerStatesOf(MatchmakingResult),
                serverInfo: {
                    buildId: TARGET_CHANGELIST + "_1.4.4_shipping", // TODO: pull the end of the buildstring from somewhere nonstatic
                    gameSessionId: GameSessionForCandidate(MatchmakingResult),
                    host: MatchmakingResult.Host,
                    port: MatchmakingResult.Port
                },
                status: "IN_PROGRESS",
                statusDuration: 0.0,
                statusReason: null
            });
        }
        else{
            logger.info(`MM not ready yet! (userId ${UserId}${Party}${Decision.Parked ? ", joined again after being sent" : ""})`);

            res.status(200);
            res.json({
                candidateId: MatchmakingResult.CandidateId,
                candidateStatusPeriodMillis: MatchmakingResult.Private || MatchmakingResult.PartyCandidate ? 1000 : 10000,
                gameMode: "ISLAND",
                huntId: MatchmakingResult.HuntId,
                playerStates: PlayerStatesOf(MatchmakingResult),
                status : "MATCHING",
                statusDuration : 0.0,
                statusReason : null
            })
        }
    }
    else{
        logger.error(`UserId ${UserId} was not found in the MatchmakingMap`);

        res.status(404);
        res.send();
    }
});

matchmakingRouter.post("/candidate/join", HasUndauntedMetagameAuth, async (req: any, res) => {
    const UserId = req.AuthData.userId;
    const GameMode = req.body.gameMode;
    const GameArgs = req.body.gameArgs;
    const HuntId = req.body.playerHuntId;

    logger.info(`UserId ${UserId} wants to join a game with GameMode ${GameMode} & GameArgs ${GameArgs} & HuntId ${HuntId}`);

    // TODO: We put a LOT of faith in our authenticated users not abusing the matchmaking system right now
    // A reasonable addition would be checks on frequency of MM/server spinup
    // Best scenario is 1-1 for server session<->player and a new server cooldown

    const Private = req.body.isPrivate === true || req.body.privateMatch === true;
    const MatchmakingResult = await HandlePlayerMatchmaking(GameMode, GameArgs, HuntId, UserId, Private);

    if(!MatchmakingResult){
        res.status(400);
        res.send();
        return;
    }

    const MatchmakingEntry = await CheckAndUpdateQueueStatus(UserId);

    if(MatchmakingEntry == undefined){
        logger.error(`UserId ${UserId} has no matchmaking entry after joining`);
        res.status(400);
        res.send();
        return;
    }

    res.status(200);
    res.json({
        candidateId: MatchmakingEntry.CandidateId,
        candidateStatusPeriodMillis: MatchmakingEntry.Private || MatchmakingEntry.PartyCandidate ? 1000 : 10000,
        gameMode: GameMode,
        huntId: HuntId,
        status: "MATCHING",
        statusReason: null
    });
});

// Joining a given candidate (the exe's "CandidateJoin"): a party member following the leader's
// candidate. Anything else keeps the 404 it always got.
matchmakingRouter.post("/candidate/join/:candidateId", HasUndauntedMetagameAuth, (req: any, res) => {
    const UserId = req.AuthData.userId;
    const Candidate = typeof UserId === "string" ? JoinPartyCandidateById(UserId, req.params.candidateId) : undefined;

    if(Candidate === undefined){
        logger.warn(`UserId ${UserId} asked to join candidate ${String(req.params.candidateId).slice(0, 64)}, which is not their party's`);

        res.status(404);
        res.send();
        return;
    }

    logger.info(`UserId ${UserId} joins their party's candidate ${Candidate.CandidateId} (${Candidate.GameMode} ${Candidate.HuntId})`);

    res.status(200);
    res.json({
        candidateId: Candidate.CandidateId,
        candidateStatusPeriodMillis: 1000,
        gameMode: Candidate.GameMode,
        huntId: Candidate.HuntId,
        status: "MATCHING",
        statusReason: null
    });
});

matchmakingRouter.get("/QoS", (req, res) => {
    logger.info(`QoS Ping`);

    res.status(200);
    res.send("<!DOCTYPE html><html><body>pong</body></html>");
})
