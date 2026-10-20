import { Router } from "express";
import { logger } from "../logger";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { GetDb } from "../db";
import { users } from "../db/schema";
import { eq } from "drizzle-orm";
import { FindAccountByUsername, FindUsernameForUserId, GetUsernameForUserId, IsAccountIdShape } from "../controllers/login";

export const loginRouter = Router();

loginRouter.get("/features/platform/win", (req, res) => {
    logger.info("Features");

    res.send({
        "code" : null,
        "message" : "OK",
        "payload" : {
           "crossplay" : true,
           "crossprogression" : true
        }
    });
});

loginRouter.get("/account/link/epic/:AccId", (req, res) => {
    logger.info("Account Linking");

    res.json({
        "code" : null,
        "message" : "OK",
        "payload" : {
           "isLinked" : true
        }
    });
});

loginRouter.post("/login", HasUndauntedMetagameAuth, async (req: any, res) => {
    if(req.AuthData.userId !== req.body.email){
        res.status(400);
        res.send();

        logger.error(`UserID from metagame auth ${req.AuthData.userId} didn't match UserID from token ${req.AuthData.email}`);

        return;
    }

    let UserRecord = await GetDb().query.users.findFirst({where: eq(users.userId, req.AuthData.userId)});

    if(UserRecord == undefined){
        res.status(400);
        res.send();

        logger.error(`UserID from metagame auth ${req.AuthData.userId} had no database entry!`);

        return;
    }

    logger.info(`${req.body.email} is logging in!`);

    res.json({
        "error_code": "TicketRateOk",
        "message": "",
        "state": "OPEN",
        "timeout": 8000,
        "title": ""
    });
});

loginRouter.get("/accountinfo", HasUndauntedMetagameAuth, async (req: any, res) => {
    logger.info("Account info")

    const Username = await GetUsernameForUserId(req.AuthData.userId);

    res.json({
        "accountId" : req.AuthData.userId,
        "creationDate" : "2000-01-01 00:00:00",
        "email" : null,
        "preferredLanguage" : null,
        "username" : Username,
        "verified" : true
    });
});

loginRouter.get("/tags", HasUndauntedMetagameAuth, (req: any, res) => {
    logger.info("Tags")

    res.json({
        "accountId" : req.AuthData.userId,
        "tags": []
    });
});

loginRouter.put("/gamesession/epic", HasUndauntedMetagameAuth, (req: any, res) => {
    const AuthHeader = req.headers.authorization;

    const Token = AuthHeader.slice("bearer ".length);

    // A note on auth tokens:
    // The original flow went Epic Launcher -> Epic -> PHX
    // With each step having it's own auth token.
    // Since this is unneeded complexity for us, we just use the same token for all 3
    // Hence this echo endpoint

    res.json({
        "code": null,
        "message": "OK",
        "payload": {
            "error_code": null,
            "sessionid": "SESSION_ID_LOL", // TODO: This is surfaced in the UI, but I don't think it matters for anything else
            "sessionToken": Token 
        }
    })
});

// POST /accountinfo/public: the 1.4.4 client's PublicAccountInfoEndpoint (FOnlineUserPhoenix::QueryUserInfo),
// one call per player it needs to show: the sender of a party invite, party and Hunt Members, friends,
// blocked players, guild members and inviters (docs/findings/social.md). The body is
// {"accountId": "<id to look up>"} or, from a second builder in the exe, {"displayname": "<name>"}.
//
// The reply must describe the LOOKED-UP account. The client files the user info under the reply's
// accountId and keeps the first reply per id for the whole session, and it sets the user's Epic id from
// the "epic" entry of linkedAccounts, which its social toolkit then compares with the Epic id the action
// started from. Upstream answered with the caller's own id in both places, so every other player's info
// was filed under the caller (and dropped: the caller's own entry came first, at login) and the other
// player was invalidated. That is why a party invite that reached the recipient's poll on 22 September
// 2026 never showed under PARTY INVITES. On this server a player's Epic id and Phoenix id are the same
// UID-..., so linkedAccounts carries the same id.
//
// An unknown id, a malformed one or an unknown name answers 404 {}: the client counts a failed query and
// caches nothing (with the old reply it cached nothing useful either). isSubscribed and language are not
// read by the client; they stay because 73 live calls showed them harmless.
//
// ACCOUNTINFO_PUBLIC_LEGACY=1 puts the upstream reply back (caller's id; 200 with "" for an unknown id).
// At login the client looks up its own id, for which the old and new replies are the same.
function AccountInfoPublicLegacy(req: any, res: any){
    const RequestorAccountId = req.AuthData.userId;
    const Username = FindUsernameForUserId(req.body?.accountId) ?? "";

    res.status(200);
    res.json({
        accountId: RequestorAccountId,
        isSubscribed: true,
        language: null,
        linkedAccounts: [
            {
                accountId: RequestorAccountId,
                accountType: "epic"
            }
        ],
        username: Username
    });
}

// The asked account: accountId wins when both fields are present
function AskedAccount(Body: any): { Account: { UserId: string, Username: string } | undefined, Asked: string } {
    const AskedId = Body?.accountId;

    if(AskedId !== undefined && AskedId !== null && AskedId !== ""){
        if(!IsAccountIdShape(AskedId)){
            return { Account: undefined, Asked: "<not an account id>" };
        }

        const Username = FindUsernameForUserId(AskedId);

        return { Account: Username === undefined ? undefined : { UserId: AskedId, Username: Username }, Asked: AskedId };
    }

    const AskedName = Body?.displayname ?? Body?.displayName;

    if(typeof AskedName === "string"){
        return { Account: FindAccountByUsername(AskedName), Asked: `name:${JSON.stringify(AskedName.slice(0, 32))}` };
    }

    return { Account: undefined, Asked: "<nothing asked>" };
}

loginRouter.post("/accountinfo/public", HasUndauntedMetagameAuth, async (req: any, res) => {
    if(process.env.ACCOUNTINFO_PUBLIC_LEGACY === "1"){
        AccountInfoPublicLegacy(req, res);
        return;
    }

    // Any authenticated caller may look up any account (names are not secret on this server)
    const Caller = typeof req.AuthData?.userId === "string" ? req.AuthData.userId : "<game server>";
    const { Account, Asked } = AskedAccount(req.body);

    logger.info(`accountinfo/public by ${Caller} for ${Asked} -> ${Account !== undefined ? "found" : "404"}`);

    if(Account === undefined){
        res.status(404);
        res.json({});
        return;
    }

    res.status(200);
    res.json({
        accountId: Account.UserId,
        username: Account.Username,
        linkedAccounts: [
            {
                accountId: Account.UserId,
                accountType: "epic"
            }
        ],
        isSubscribed: true,
        language: null
    });
});