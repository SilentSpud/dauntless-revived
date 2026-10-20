import { Router } from "express";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { PlayerTokenOnly, SoftMetagameAuth, SoftPlayerOf } from "../middleware/PlayerAuth";
import { logger } from "../logger";
import {
    AcceptPartyInvite, DeclinePartyInvites, EvictPartyLeader, InviteToParty, KickPartyMember, LeaveParty, ListPartyInvites,
    PartiesOfPlayers, PartyActionResult, PollParty, PromotePartyMember
} from "../controllers/party";

// Parties (roadmap 1.9; C:\dr\data\plans\parties-friends.md phase 1). Every route acts as the
// bearer token's own account: ids in the URL or body only ever name the other player or the
// party, never who is acting. The state lives in controllers/party.ts.

export const partyRouter = Router();

function Send(res: any, Result: PartyActionResult){
    res.status(Result.Status);
    res.json(Result.Body);
}

// The client's poll, about every 10 s: the caller's party (a party of one if none)
partyRouter.post("/party", HasUndauntedMetagameAuth, PlayerTokenOnly, async (req: any, res) => {
    res.status(200);
    res.json(await PollParty(req.AuthData.userId));
});

// Polled with POST /party. Without a valid token it answers the old empty list, so the
// poll never breaks; nobody ever sees another account's invites.
let LastAnonymousInvitesWarning = 0;

partyRouter.get("/party/invites", SoftMetagameAuth, (req: any, res) => {
    const UserId = SoftPlayerOf(req);

    if(UserId === undefined){
        if(Date.now() - LastAnonymousInvitesWarning > 60 * 1000){
            LastAnonymousInvitesWarning = Date.now();
            logger.warn("GET /party/invites without a valid player token: answering an empty list");
        }

        res.status(200);
        res.json({ invitations: [] });
        return;
    }

    res.status(200);
    res.json(ListPartyInvites(UserId));
});

// {recipientPlayerId, partyId, buildId, featureOverrides}
partyRouter.put("/party/invite", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, InviteToParty(req.AuthData.userId, req.body?.recipientPlayerId, req.body?.partyId));
});

// {recipientPlayerId: <caller>, partyId: <invite id>, buildId, featureOverrides}. Only the
// token account's own invites are looked up, whatever the body says; a body naming someone
// else is logged (the client should never send one) and changes nothing.
partyRouter.put("/party/invite/accept/:inviteId", HasUndauntedMetagameAuth, PlayerTokenOnly, async (req: any, res) => {
    const Recipient = req.body?.recipientPlayerId;

    if(typeof Recipient === "string" && Recipient.length > 0 && Recipient !== req.AuthData.userId){
        logger.warn(`PUT /party/invite/accept by ${req.AuthData.userId}: the body names recipient ${Recipient.slice(0, 64)}; only the caller's own invites are used`);
    }

    Send(res, await AcceptPartyInvite(req.AuthData.userId, req.params.inviteId));
});

// {sendingPlayerId, recipientPlayerId, partyId}: decline (or withdraw one's own invite)
partyRouter.delete("/party/invite", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, DeclinePartyInvites(req.AuthData.userId, req.body));
});

// No body: the caller leaves their party
partyRouter.delete("/party/member", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, LeaveParty(req.AuthData.userId));
});

partyRouter.delete("/party/member/:memberId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, KickPartyMember(req.AuthData.userId, req.params.memberId));
});

partyRouter.put("/party/member/promote/:memberId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, PromotePartyMember(req.AuthData.userId, req.params.memberId));
});

// Only the client's auto-eviction of a leader it thinks is offline sends this
partyRouter.delete("/party/leader/:leaderId", HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, EvictPartyLeader(req.AuthData.userId, req.params.leaderId));
});

// {playerIds: [...]} -> {parties, invitations}
partyRouter.post("/party/status", HasUndauntedMetagameAuth, PlayerTokenOnly, async (req: any, res) => {
    Send(res, await PartiesOfPlayers(req.AuthData.userId, req.body?.playerIds));
});
