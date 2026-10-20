import { NextFunction, Request, Response } from "express";
import { JwtPayload } from "jsonwebtoken";
import { ValidateMetagameJWTAndGetPayload } from "../controllers/auth";
import { logger } from "../logger";

// Player-facing auth for the party and friends routes (roadmap 1.9).

// After HasUndauntedMetagameAuth: the request must name a player through its bearer token.
// A game server's key alone names nobody, and every party or friends action is the token's
// own account acting (403 otherwise).
export function PlayerTokenOnly(req: Request, res: Response, next: NextFunction){
    const UserId = (req as any).AuthData?.userId;

    if(typeof UserId === "string" && UserId.length > 0){
        next();
        return;
    }

    logger.warn(`Refusing ${req.method} ${req.path}: it needs a player's token`);

    res.status(403);
    res.send();
}

let LastSoftAuthWarning = 0;

// Routes the client polls or reads before anything else, which used to answer everyone the
// same static reply: a valid bearer token sets req.AuthData as HasUndauntedMetagameAuth
// would; no token or a bad one leaves it unset and the route answers its old static reply.
// The game-server key is never looked at here.
export function SoftMetagameAuth(req: Request, res: Response, next: NextFunction){
    const AuthHeader = req.headers.authorization;

    if(typeof AuthHeader === "string" && (AuthHeader.startsWith("bearer ") || AuthHeader.startsWith("Bearer ") || AuthHeader.startsWith("BEARER "))){
        try{
            const Payload = ValidateMetagameJWTAndGetPayload(AuthHeader.slice("bearer ".length)) as JwtPayload;

            if(typeof Payload?.userId === "string" && Payload.userId.length > 0){
                (req as any).AuthData = Payload;
            }
        }
        catch{
            if(Date.now() - LastSoftAuthWarning > 60 * 1000){
                LastSoftAuthWarning = Date.now();
                logger.warn(`${req.method} ${req.path} with a bad or expired token: answering the static reply`);
            }
        }
    }

    next();
}

export function SoftPlayerOf(req: Request): string | undefined {
    const UserId = (req as any).AuthData?.userId;

    return typeof UserId === "string" && UserId.length > 0 ? UserId : undefined;
}
