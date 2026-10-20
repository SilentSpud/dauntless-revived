import { NextFunction, Request, Response } from "express";
import { JwtPayload } from "jsonwebtoken";
import { logger } from "../logger";
import { IsValidGameserverAPIKey } from "../controllers/apikeys";
import { ValidateMetagameJWTAndGetPayload } from "../controllers/auth";
import { RefuseGameserverKeyFromOutside } from "./RequestOrigin";

// For routes only a game server may call (POST /guild, the create that follows the client's
// ServerCreateGuild RPC). Unlike HasUndauntedMetagameAuth:
// - a request without the game-server key is refused (403), whatever bearer it carries: a player's
//   token alone never reaches the route;
// - a bearer that comes along is read softly and only for the log: a valid one sets
//   AuthData.ForwardedUserId (never userId, so no route acts as that account), a bad or expired one is
//   ignored (logged at most once a minute) instead of failing the request. The shared middleware would
//   throw there, which is a 500. In 1.4.4 such a token is the game server's own login's (the exe takes
//   the token of the server's local user, 0x140ac78a4), not the player's; game servers do not log in
//   to Phoenix, so normally there is none.
// The key itself works as everywhere else: only from this machine (never through the gateway or any
// proxy), and 401 when it is not a registered key.

let LastBadBearerWarning = 0;

export async function GameServerKeyAuth(req: Request, res: Response, next: NextFunction){
    const Key = req.headers["x-undaunted-gameserver-apikey"];

    if(typeof Key !== "string" || Key.length === 0){
        logger.warn(`Refusing ${req.method} ${req.path}: only a game server may call it`);
        res.status(403);
        res.json({ code: "", message: "Only a game server may call this.", payload: {} });
        return;
    }

    if(RefuseGameserverKeyFromOutside(req, res)){
        return;
    }

    if(!(await IsValidGameserverAPIKey(Key))){
        logger.error(`Invalid Gameserver API Key Auth`);
        res.status(401);
        res.send();
        return;
    }

    const AuthData: Record<string, unknown> = { IsGameserver: true };
    const AuthHeader = req.headers.authorization;

    if(typeof AuthHeader === "string" && /^bearer /i.test(AuthHeader)){
        try{
            const Payload = ValidateMetagameJWTAndGetPayload(AuthHeader.slice("bearer ".length)) as JwtPayload;

            if(typeof Payload?.userId === "string" && Payload.userId.length > 0){
                AuthData.ForwardedUserId = Payload.userId;
            }
        }
        catch{
            if(Date.now() - LastBadBearerWarning > 60 * 1000){
                LastBadBearerWarning = Date.now();
                logger.warn(`${req.method} ${req.path} from a game server with a bad or expired player token: ignoring the token`);
            }
        }
    }

    (req as any).AuthData = AuthData;

    next();
}
