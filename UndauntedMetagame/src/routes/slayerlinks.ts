import { Router } from "express";
import { HasUndauntedMetagameAuth } from "../middleware/HasUndauntedMetagameAuth";
import { PlayerTokenOnly } from "../middleware/PlayerAuth";
import { SlayerLinks } from "../features";
import {
    AnswerSlayerLinkInvite, DeleteSlayerLink, DeleteSlayerLinkInvites, InviteToSlayerLink, ListSlayerLinkInvites, ListSlayerLinks,
    SlayerLinkAvailability, SlayerLinkReply, SlayerLinkStatus
} from "../controllers/slayerlinks";

// Slayer Links: the eight routes the 1.4.4 client sends (the LinkedSlayers* keys of
// native/internal-server/dllmain.cpp that the exe reads). The contract came from Harmonic's fork
// (github.com/Harmonicrain/Undaunted 895f7c7); the rules, the shapes and the exe evidence are in
// controllers/slayerlinks.ts. Every route acts as the bearer token's own account; ids in the body or URL
// only name the other player. A game server's key alone gets 403.
//
// SLAYER_LINKS=0 skips every route here, and the requests fall through to the 404 they got before. The
// reward routes (/slayerlink/links/rewards...) are not answered either way.

export const slayerLinksRouter = Router();

// Like GuildsOn: with the switch off the route is skipped and the request falls through to the 404
function SlayerLinksOn(req: any, res: any, next: any){
    next(SlayerLinks() ? undefined : "route");
}

function Send(res: any, Reply: SlayerLinkReply){
    res.status(Reply.Status);
    res.json(Reply.Body);
}

// The social panel's poll for news: {invites, links, config}
slayerLinksRouter.get("/slayerlink/status_good", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, SlayerLinkStatus(req.AuthData.userId));
});

slayerLinksRouter.get("/slayerlink/invites", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, ListSlayerLinkInvites(req.AuthData.userId));
});

slayerLinksRouter.get("/slayerlink/links", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, ListSlayerLinks(req.AuthData.userId));
});

// {account_id, slot, action_source}: invite a friend
slayerLinksRouter.put("/slayerlink/invite", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, InviteToSlayerLink(req.AuthData.userId, req.body));
});

// {account_id, action: accept|reject|cancel, slot, action_source}
slayerLinksRouter.post("/slayerlink/invite", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, AnswerSlayerLinkInvite(req.AuthData.userId, req.body));
});

// DeleteAllInvites: the caller's own id clears all of the caller's invites, another id those between the two
slayerLinksRouter.delete("/slayerlink/invites/:accountId", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, DeleteSlayerLinkInvites(req.AuthData.userId, req.params.accountId));
});

// {account_id, slot, delete_pair}: remove a link
slayerLinksRouter.delete("/slayerlink/links", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, DeleteSlayerLink(req.AuthData.userId, req.body, req.query));
});

// {account_ids}: which friends can be invited now
slayerLinksRouter.post("/slayerlink/availability", SlayerLinksOn, HasUndauntedMetagameAuth, PlayerTokenOnly, (req: any, res) => {
    Send(res, SlayerLinkAvailability(req.AuthData.userId, req.body));
});
