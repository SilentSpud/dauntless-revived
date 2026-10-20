import { NextFunction, Request, Response } from "express";
import { GetUserInfoForApiKey, UserInfo } from "../controllers/undauntedapi";
import { RefuseAdminKeyThroughProxy } from "./RequestOrigin";

export async function HasUndauntedAdminApiKey(req: Request, res: Response, next: NextFunction){
    // Admin requests never come through the public gateway (or any proxy): 403 before the
    // key is looked up. Direct callers (the host, or the tailnet in private mode) as before.
    if(RefuseAdminKeyThroughProxy(req, res)){
        return;
    }

    const ApiKey = req.headers["x-undaunted-user-api-key"] as string | undefined;

    if(ApiKey == undefined){
        res.status(401);
        res.send();
        return;
    };

    const UserInfo: UserInfo | undefined = await GetUserInfoForApiKey(ApiKey);

    if(UserInfo == undefined){
        res.status(401);
        res.send();
        return;
    };

    if(!UserInfo.IsAdmin){
        res.status(403);
        res.send();
        return;
    }

    (req as any).UndauntedUserInfo = UserInfo;

    next();

    return;
}