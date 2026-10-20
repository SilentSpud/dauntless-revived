import { NextFunction, Request, Response } from "express";
import { IsRealProgressionAccount } from "../controllers/progressionmode";
import { logger } from "../logger";

// Put first on a route that only exists for real-mode accounts. Any other account
// skips the route before auth runs, so it gets exactly what it got before the
// route existed (the catch-all 404).
export function RealProgressionOnly(req: Request, res: Response, next: NextFunction){
    if(IsRealProgressionAccount(req.params.userId)){
        next();
        return;
    }

    next("route");
}

// Real-mode reads: a game server names the player in the URL and is trusted; a
// player client only gets its own account (the stubs these replace always answered
// with the token's user). Answers 403 and returns true for anyone else's.
export function RefuseForeignPlayer(req: any, res: Response){
    if(req.AuthData?.IsGameserver || req.AuthData?.userId === req.params.userId){
        return false;
    }

    logger.warn(`Refusing ${req.method} ${req.path} to userId ${req.AuthData?.userId}: another player's account`);

    res.status(403);
    res.send();
    return true;
}

export function SendRealReply(res: Response, Reply: { Status: number, Body?: unknown }){
    res.status(Reply.Status);

    if(Reply.Body === undefined){
        res.send();
        return;
    }

    res.json(Reply.Body);
}