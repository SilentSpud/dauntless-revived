import { Router } from "express";
import { logger } from "../logger";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { PlayerTokenOnly, SoftMetagameAuth, SoftPlayerOf } from "../middleware/PlayerAuth";
import { BlockPlayer, FriendResult, ListBlocked, ListFriends, RemoveFriend, SendOrAcceptFriendRequest, UnblockPlayer } from "../controllers/friends";
import { IsAccountIdShape } from "../controllers/login";

// The Epic-style friends service the 1.4.4 client talks to ([OnlineSubsystemMcp.OnlineFriendsMcp]
// ServiceName="friends"), roadmap 1.9 / parties plan phase 3. Data: controllers/friends.ts.
// Everyone shows as offline: online status needs an XMPP presence server (roadmap 3.10).

export const friendsRouter = Router();

// Replies for routes that 404'd since upstream. MISC_ROUTES=0 puts the 404s back.
function MiscRoutesOn(req: any, res: any, next: any){
    next(process.env.MISC_ROUTES === "0" ? "route" : undefined);
}

let LastForeignListWarning = 0;

// The reads the client makes at login. Without a valid token they answer the old empty
// reply, and so does a token asking for another account's list (it only ever reads its own;
// the empty reply shows nothing and cannot break the client's friends panel).
function OwnListOrStatic(req: any, res: any, Static: unknown): string | undefined {
    const Caller = SoftPlayerOf(req);

    if(Caller !== undefined && Caller !== req.params.userId && Date.now() - LastForeignListWarning > 60 * 1000){
        LastForeignListWarning = Date.now();
        logger.warn(`${req.method} ${req.path} by ${Caller}: another account's list; answering the empty reply`);
    }

    if(Caller === undefined || Caller !== req.params.userId){
        res.status(200);
        res.json(Static);
        return undefined;
    }

    return Caller;
}

// The client wraps this body as {"friends": <body>}, so it must be a bare array:
// [{accountId, status: ACCEPTED|PENDING, direction: INBOUND|OUTBOUND, created}]
friendsRouter.get("/friends/api/public/friends/:userId", MiscRoutesOn, SoftMetagameAuth, (req: any, res) => {
    const Caller = OwnListOrStatic(req, res, []);

    if(Caller === undefined){
        return;
    }

    const Friends = ListFriends(Caller, req.query?.includePending === "true");

    logger.info(`friends: list for ${Caller}: ${Friends.filter((Friend) => Friend.status === "ACCEPTED").length} friend(s), ${Friends.filter((Friend) => Friend.status === "PENDING").length} pending`);

    res.status(200);
    res.json(Friends);
});

friendsRouter.get("/friends/api/public/blocklist/:userId", MiscRoutesOn, SoftMetagameAuth, (req: any, res) => {
    const Caller = OwnListOrStatic(req, res, { blockedUsers: [] });

    if(Caller === undefined){
        return;
    }

    res.status(200);
    res.json({
        blockedUsers: ListBlocked(Caller)
    });
});

// Changes: 204 with no body, as the Epic service answered. The URL's first id must be the caller.
function SendChange(req: any, res: any, Change: (Me: string, Them: string) => FriendResult){
    const Me = req.AuthData.userId;

    if(req.params.userId !== Me){
        logger.warn(`Refusing ${req.method} ${req.path} by ${Me}: another account's list`);
        res.status(403);
        res.send();
        return;
    }

    if(!IsAccountIdShape(req.params.friendId)){
        res.status(404);
        res.send();
        return;
    }

    const Result = Change(Me, req.params.friendId);

    res.status(Result.ok ? 204 : Result.Status);
    res.send();
}

// Send a friend request, or accept the one the other player sent
friendsRouter.post("/friends/api/public/friends/:userId/:friendId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    SendChange(req, res, SendOrAcceptFriendRequest);
});

// Unfriend, withdraw a request, or decline one
friendsRouter.delete("/friends/api/public/friends/:userId/:friendId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    SendChange(req, res, RemoveFriend);
});

// Block. The verb is inferred from the exe (the Block request's code was not fully traced), so PUT on
// the same path does the same; unblock is DELETE (traced).
friendsRouter.post("/friends/api/public/blocklist/:userId/:friendId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    SendChange(req, res, BlockPlayer);
});

friendsRouter.put("/friends/api/public/blocklist/:userId/:friendId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    SendChange(req, res, BlockPlayer);
});

friendsRouter.delete("/friends/api/public/blocklist/:userId/:friendId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    SendChange(req, res, UnblockPlayer);
});

// Recent players are kept in the character data already; the friends service's list stays empty
friendsRouter.get("/friends/api/public/list/:namespace/:userId/recentPlayers", MiscRoutesOn, (req, res) => {
    res.status(200);
    res.json([]);
});

friendsRouter.get("/friends/api/v1/:userId/settings", MiscRoutesOn, (req, res) => {
    res.status(200);
    res.json({
        acceptInvites: "public"
    });
});
