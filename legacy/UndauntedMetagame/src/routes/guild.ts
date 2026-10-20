import { Router } from "express";
import { logger } from "../logger";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { GameServerKeyAuth } from "../middleware/GameServerKeyAuth";
import { PlayerTokenOnly } from "../middleware/PlayerAuth";
import {
    AcceptGuildInvite, ChangeGuildRank, CreateGuild, DeclineGuildInvite, DisbandGuild, GetOwnGuild, GuildReply, InviteToGuild, KickGuildMember,
    LeaveGuild, ListOwnGuildInvites, ValidateGuildCreate
} from "../controllers/guild";

// Guilds (roadmap 3.11): the eleven v2 routes the 1.4.4 client uses (the *_v2 keys in
// native/internal-server/dllmain.cpp). The rules and reply shapes are in controllers/guild.ts; the
// evidence is in docs/findings/social.md. Every client route acts as the bearer token's own account;
// ids in the URL only name the other player, the invite or the guild. The create comes from the
// Ramsgate game server with its key (the client's Create button is an RPC to that server).
//
// GUILDS=0 puts the old stubs back: GET /guild answers 204, GET /guild/invite/player an empty list,
// and every other guild route 404s as before.
//
// Order matters: DELETE /guild/player and /guild/player/:accountId come before DELETE /guild/:guildId.

export const guildRouter = Router();

function GuildsOff(){
    return process.env.GUILDS === "0";
}

// Like MiscRoutesOn: with GUILDS=0 the route is skipped and the request falls through to the 404
function GuildsOn(req: any, res: any, next: any){
    next(GuildsOff() ? "route" : undefined);
}

function Send(res: any, Reply: GuildReply){
    res.status(Reply.Status);

    if(Reply.Body === undefined){
        res.send();
        return;
    }

    res.json(Reply.Body);
}

function Caller(req: any): string | undefined {
    const UserId = req.AuthData?.userId;

    return typeof UserId === "string" && UserId.length > 0 ? UserId : undefined;
}

// The caller's open invites, read at login, at each world load and after every guild action
guildRouter.get("/guild/invite/player", HasUndauntedMetagameAuth, (req: any, res) => {
    if(GuildsOff()){
        logger.info("Guild invites (stubbed)");
        res.status(200);
        res.json({ code: null, message: "OK", payload: { invites: [] } });
        return;
    }

    Send(res, ListOwnGuildInvites(Caller(req)));
});

// The caller's guild; 204 when there is none (a failure to the client, which then clears its guild)
guildRouter.get("/guild", HasUndauntedMetagameAuth, (req: any, res) => {
    if(GuildsOff()){
        logger.info("Current guild (stubbed)");
        res.status(204);
        res.send();
        return;
    }

    Send(res, GetOwnGuild(Caller(req)));
});

// {leader_account_id, name, nameplate}, sent while the player types in CREATE A GUILD
guildRouter.post("/guild/validate", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, ValidateGuildCreate(req.AuthData.userId, req.body?.leader_account_id, req.body?.name, req.body?.nameplate));
});

// The game server's create: the same body and the game-server key. A token that comes along is the game
// server's own login's, never the leader's (controllers/guild.ts), so it is only logged.
guildRouter.post("/guild", GuildsOn, GameServerKeyAuth, (req: any, res) => {
    Send(res, CreateGuild(req.body?.leader_account_id, req.AuthData?.ForwardedUserId, req.body?.name, req.body?.nameplate));
});

// Leave Guild (no body)
guildRouter.delete("/guild/player", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, LeaveGuild(req.AuthData.userId));
});

// Kick From Guild
guildRouter.delete("/guild/player/:accountId", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, KickGuildMember(req.AuthData.userId, req.params.accountId));
});

// Invite to Guild (no body)
guildRouter.put("/guild/invite/:accountId", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, InviteToGuild(req.AuthData.userId, req.params.accountId));
});

// Accept Guild Invite; the URL carries the invite's id (the client's {guild_invite_id})
guildRouter.post("/guild/invite/accept/:inviteId", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, AcceptGuildInvite(req.AuthData.userId, req.params.inviteId));
});

// Decline Guild Invite
guildRouter.delete("/guild/invite/:inviteId", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, DeclineGuildInvite(req.AuthData.userId, req.params.inviteId));
});

// Promote To Guild Officer (officer), Demote To Guild Member (member), Promote To Guild Leader (leader)
guildRouter.put("/guild/rank/:accountId/:rank", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, ChangeGuildRank(req.AuthData.userId, req.params.accountId, req.params.rank));
});

// DISBAND GUILD (leader only). Last: it would otherwise take DELETE /guild/player.
guildRouter.delete("/guild/:guildId", GuildsOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, DisbandGuild(req.AuthData.userId, req.params.guildId));
});
