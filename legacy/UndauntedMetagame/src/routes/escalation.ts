import { Router } from "express";
import { logger } from "../logger";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { RealProgressionOnly, RefuseForeignPlayer, SendRealReply } from "../middleware/RealProgressionOnly";
import { RefuseUnlessGameserver } from "../middleware/GameServerOnly";
import { IsRealProgressionAccount } from "../controllers/progressionmode";
import { CallerOf } from "../controllers/progressionevents";
import { GetEscalationReply, SaveEscalation } from "../controllers/escalation";
import { EscalationMode } from "../features";

// Escalation (roadmap 2.16): GetSeasonalEscalationEndpoint and UpdateSeasonalEscalationEndpoint,
// both /escalation/{season_id}/{account_id}. Both answer {code, message, payload: <season>}
// (parsed by 0x140aae300). The save rules are in controllers/escalation.ts; the season data and the
// rules come from Harmonic's fork (github.com/Harmonicrain/Undaunted 895f7c7).
//
// ESCALATION_MODE=stub (the default) keeps what every account got before: the fake maximum on
// GET, and the 404 of the catch-all on POST. With ESCALATION_MODE=real, accounts in real
// progression mode read and save their own seasons; stub-mode accounts keep the stub.

export const escalationRouter = Router();

// The save route exists only with ESCALATION_MODE=real; otherwise it falls through to the 404 it always got
function EscalationSavesOn(req: any, res: any, next: any){
    next(EscalationMode() === "real" ? undefined : "route");
}

escalationRouter.get("/escalation/:escalationSeason/:userId", HasUndauntedMetagameAuth, (req: any, res) => {
    const EscalationSeason = req.params.escalationSeason;

    // Real: a player reads only their own account, a game server the one it names
    if(EscalationMode() === "real" && IsRealProgressionAccount(req.params.userId)){
        if(RefuseForeignPlayer(req, res)){
            return;
        }

        SendRealReply(res, GetEscalationReply(req.params.userId, EscalationSeason));
        return;
    }

    logger.info(`Escalation Configuration for season ${EscalationSeason} (stubbed)`);

    res.status(200);
    res.json({
        code: null,
        message: "OK",
        payload: {
            escalation_level: 99999,
            next_level_xp: 99999,
            talents_progress: [],
            unlock_progress: [],
            update_version: 1,
        }
    });
});

// The world server's authoritative snapshot: a game server's key only. A relayed player token
// for another account is logged and the save is kept for the account the URL names.
escalationRouter.post("/escalation/:escalationSeason/:userId", EscalationSavesOn, RealProgressionOnly, HasUndauntedMetagameAuth, (req: any, res) => {
    if(RefuseUnlessGameserver(req, res, "escalation save")){
        return;
    }

    SendRealReply(res, SaveEscalation(req.params.userId, req.params.escalationSeason, req.body, CallerOf(req)));
});
