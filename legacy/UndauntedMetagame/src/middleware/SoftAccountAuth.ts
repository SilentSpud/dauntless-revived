import { NextFunction, Request, Response } from "express";
import { eq } from "drizzle-orm";
import { JwtPayload } from "jsonwebtoken";
import { GetDb } from "../db";
import { users } from "../db/schema";
import { ValidateMetagameJWTAndGetPayload } from "../controllers/auth";
import { GetUserInfoForApiKey } from "../controllers/undauntedapi";

// For routes that answer everyone but tell registered players more (GET /undaunted/api/ServerStatus:
// the live player list is for registered players only). The caller counts as a registered player
// with either
// - an account key in x-undaunted-user-api-key, checked exactly like GetUserInfo (the hashed-key
//   lookup, compared in constant time), admin or not, or
// - a valid player bearer token whose account still exists.
// Anything else (no key, a wrong key, an expired token, a deleted account) leaves the request
// anonymous and the route answers its public reply. Never a 401: a launcher that has not
// registered yet still reads the server's name and registration mode. The key is never logged,
// stored or cached here.

const MAX_KEY_LENGTH = 1024;

async function HasValidAccountKey(req: Request){
    const Key = req.headers["x-undaunted-user-api-key"];

    if(typeof Key !== "string" || Key.length === 0 || Key.length > MAX_KEY_LENGTH){
        return false;
    }

    try{
        return (await GetUserInfoForApiKey(Key)) !== undefined;
    }
    catch{
        return false; // an unsupported AUTH_MODE or a database error: answer as for no key
    }
}

function HasValidPlayerToken(req: Request){
    const AuthHeader = req.headers.authorization;

    if(typeof AuthHeader !== "string" || !/^bearer /i.test(AuthHeader)){
        return false;
    }

    try{
        const Payload = ValidateMetagameJWTAndGetPayload(AuthHeader.slice("bearer ".length)) as JwtPayload;
        const UserId = Payload?.userId;

        if(typeof UserId !== "string" || UserId.length === 0){
            return false;
        }

        return GetDb().select({ userId: users.userId }).from(users).where(eq(users.userId, UserId)).get() !== undefined;
    }
    catch{
        return false;
    }
}

export async function IsRegisteredCaller(req: Request){
    return await HasValidAccountKey(req) || HasValidPlayerToken(req);
}

export async function SoftAccountAuth(req: Request, res: Response, next: NextFunction){
    (req as any).RegisteredCaller = await IsRegisteredCaller(req);

    next();
}

export function IsSoftRegisteredCaller(req: Request){
    return (req as any).RegisteredCaller === true;
}
