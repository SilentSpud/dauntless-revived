// Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only; rewritten and
// corrected against the 1.4.4 executable for Dauntless Revived.
import crypto from "node:crypto";
import { and, asc, count, eq, gt, inArray, lte, ne, or } from "drizzle-orm";
import { GetDb } from "../db";
import { slayerlinkinvites, slayerlinks, users } from "../db/schema";
import { logger } from "../logger";
import type { Tx } from "./savehistory";
import { IsAccountIdShape } from "./login";
import { AreFriendsInTx, IsBlockedEitherWayInTx } from "./friends";

// Slayer Links: two friends link up for a week (the social panel's Linked Slayers tab). Stored in SQLite
// (migration 0016_slayer_links), so an invite waits for a player who is offline. Harmonic's fork had the
// first working contract; the routes and shapes below follow the 1.4.4 exe where that contract differed (the
// evidence is in the notes of the port, docs/findings/social.md once written):
//
// - PUT /slayerlink/invite {account_id, slot, action_source}: invite a friend into one of your slots
//   (InviteToLink 0x1415fc870, body serializer 0x1415ff0e0)
// - POST /slayerlink/invite {account_id, action, slot, action_source}: action is "accept", "reject" or
//   "cancel" (0x1415d4820, 0x1415dab50, 0x1415d9330; serializer 0x1415fdc40). account_id names the other
//   player of the invite; a link_id or invite_id, if one is ever sent, is tried first.
// - GET /slayerlink/invites: {invites: [{account_id, slot, direction, status, expires, link_id}]}
//   (entry serializer 0x1415ff170; that fork answered linked_account_id, which the client does not read)
// - GET /slayerlink/links: {links: [{account_id, slot, ends, prize_pool}]} (entry 0x141600170). The status
//   reply's links read linked_account_id and link_id instead (0x1415ffd00), so every link entry carries both.
// - DELETE /slayerlink/links {account_id, slot, delete_pair}: remove a link (FOnlineLinkedSlayer::DeleteLinks
//   0x1415dc1d0, body 0x141600eb0). That fork answered DELETE /slayerlink/link, which the client never sends.
// - DELETE /slayerlink/invites/{account_id}: DeleteAllInvites (0x1415db730); that fork had no route for it.
// - POST /slayerlink/availability {account_ids}: {availability: [{account_id, available}]} (0x1415e33b0)
// - GET /slayerlink/status_good: {invites, links, config: {link_duration_hours, invite_expiry_hours}}
//   (0x1415e5990; reply serializer 0x141600510, read by HandleGetStatusUpdateComplete 0x1415e8b40)
//
// Every reply is the Phoenix envelope {code, message, payload}; an error is {code: "<status>", message,
// payload: null} with that HTTP status. The reward routes (links/rewards) are not answered: a false
// success there could lose a reward, so they wait until they are traced.
//
// Rules: 3 slots each, numbered 1 to 3 as the client numbers them (its slot map is filled with the keys
// 1..MaxLinkSlotsCount, 0x1415ea22d-0x1415ea3a5, and MaxLinkSlotsCount is 3, 0x1415ce5f6; a slot outside
// the map is "Invalid Slot Id"). An invite runs out after 24 h and a link after a week (168 h). Only two
// ACCEPTED friends who have not blocked each other can link. A player sends at most MAX_INVITES_PER_WINDOW
// new invites per INVITE_WINDOW_MS. Unfriending or a block cancels the pending invites between the two (in
// the same transaction, controllers/friends.ts); a link that is already running stays until it ends.

export const LINK_SLOTS = 3;
export const INVITE_EXPIRY_HOURS = 24;
export const LINK_DURATION_HOURS = 168;
export const MAX_AVAILABILITY_IDS = 50;
// New invites one player may send in a sliding window, whatever became of them. Counted from the stored
// invites, so a restart does not reset it; the same limit as friend requests (controllers/friends.ts).
export const MAX_INVITES_PER_WINDOW = 20;
export const INVITE_WINDOW_MS = 10 * 60 * 1000;
// Accepted invites and ended links are kept this long for the log's sake, then removed. Declined,
// cancelled and expired invites are removed once their 24 hours are over.
export const KEEP_DAYS = 30;

const HOUR_MS = 60 * 60 * 1000;

export type SlayerLinkReply = { Status: number, Body: unknown };

type InviteRow = typeof slayerlinkinvites.$inferSelect;
type LinkRow = typeof slayerlinks.$inferSelect;
type Action = "accept" | "reject" | "cancel";

// The status an invite gets from each action, and the one a repeated action finds
const ANSWERED: Record<Action, string> = { accept: "ACCEPTED", reject: "DECLINED", cancel: "CANCELED" };

let Clock: () => number = () => Date.now();

// Tests only: a controllable clock
export function SetSlayerLinkClockForTests(NewClock?: () => number){
    Clock = NewClock ?? (() => Date.now());
}

function Ok(Payload: unknown): SlayerLinkReply {
    return { Status: 200, Body: { code: null, message: "OK", payload: Payload } };
}

function Fail(Status: number, Message: string): SlayerLinkReply {
    return { Status, Body: { code: String(Status), message: Message, payload: null } };
}

function Shown(Value: unknown){
    return IsAccountIdShape(Value) ? Value : typeof Value === "string" ? "<not an account id>" : "<none>";
}

// A slot the client sent: a whole number 1..LINK_SLOTS (a number, or a numeric string). Slots are stored
// and answered with the client's own numbers.
function SlotOf(Value: unknown): number | undefined {
    const Slot = typeof Value === "number" ? Value : typeof Value === "string" && /^\d{1,2}$/.test(Value) ? Number(Value) : NaN;

    return Number.isInteger(Slot) && Slot >= 1 && Slot <= LINK_SLOTS ? Slot : undefined;
}

const ALL_SLOTS = Array.from({ length: LINK_SLOTS }, (_, Index) => Index + 1);

function OtherOf(Row: { senderId: string, targetId: string }, AccountId: string){
    return Row.senderId === AccountId ? Row.targetId : Row.senderId;
}

function LinkSlotOf(Link: LinkRow, AccountId: string){
    return Link.senderId === AccountId ? Link.senderSlot : Link.targetSlot;
}

function Involving(AccountId: string){
    return or(eq(slayerlinkinvites.senderId, AccountId), eq(slayerlinkinvites.targetId, AccountId));
}

function AccountExists(tx: Tx, AccountId: string){
    return tx.select({ userId: users.userId }).from(users).where(eq(users.userId, AccountId)).get() != undefined;
}

// Pending invites of these accounts that ran out become EXPIRED (the pending-pair index only holds
// pending rows, so a new invite between the two is possible again). The updates never combine the two
// sides with OR: SQLite's planner can fail on an UPDATE of the partial index's column with one.
function ExpireInvitesInTx(tx: Tx, AccountIds: string[], Now: number){
    for(const AccountId of new Set(AccountIds)){
        for(const Side of [slayerlinkinvites.senderId, slayerlinkinvites.targetId]){
            tx.update(slayerlinkinvites).set({ status: "EXPIRED" })
                .where(and(eq(slayerlinkinvites.status, "PENDING"), lte(slayerlinkinvites.expiresAt, Now), eq(Side, AccountId))).run();
        }
    }
}

function PendingInvitesOf(tx: Tx, AccountId: string, Now: number): InviteRow[] {
    return tx.select().from(slayerlinkinvites)
        .where(and(eq(slayerlinkinvites.status, "PENDING"), gt(slayerlinkinvites.expiresAt, Now), Involving(AccountId)))
        .orderBy(asc(slayerlinkinvites.createdAt)).all();
}

function ActiveLinksOf(tx: Tx, AccountId: string, Now: number): LinkRow[] {
    return tx.select().from(slayerlinks)
        .where(and(gt(slayerlinks.endsAt, Now), or(eq(slayerlinks.senderId, AccountId), eq(slayerlinks.targetId, AccountId))))
        .orderBy(asc(slayerlinks.createdAt)).all();
}

// Slots not held by a running link
function FreeSlots(tx: Tx, AccountId: string, Now: number): number[] {
    const Used = new Set(ActiveLinksOf(tx, AccountId, Now).map((Link) => LinkSlotOf(Link, AccountId)));

    return ALL_SLOTS.filter((Slot) => !Used.has(Slot));
}

function LinkedWith(tx: Tx, A: string, B: string, Now: number){
    return ActiveLinksOf(tx, A, Now).some((Link) => OtherOf(Link, A) === B);
}

// Old rows the client no longer sees: declined, cancelled and expired invites once their 24 hours are
// over; accepted invites and links that ended over KEEP_DAYS ago
function SweepInTx(tx: Tx, Now: number){
    const Before = Now - KEEP_DAYS * 24 * HOUR_MS;

    tx.delete(slayerlinkinvites).where(and(inArray(slayerlinkinvites.status, ["DECLINED", "CANCELED", "EXPIRED"]), lte(slayerlinkinvites.expiresAt, Now))).run();
    tx.delete(slayerlinkinvites).where(and(ne(slayerlinkinvites.status, "PENDING"), lte(slayerlinkinvites.expiresAt, Before))).run();
    tx.delete(slayerlinks).where(lte(slayerlinks.endsAt, Before)).run();
}

// New invites this player sent in the last INVITE_WINDOW_MS, whatever became of them
function RecentInvitesBy(tx: Tx, AccountId: string, Now: number){
    return tx.select({ Sent: count() }).from(slayerlinkinvites)
        .where(and(eq(slayerlinkinvites.senderId, AccountId), gt(slayerlinkinvites.createdAt, Now - INVITE_WINDOW_MS))).get()?.Sent ?? 0;
}

// ---- Replies ----

function InviteEntry(Row: InviteRow, AccountId: string){
    return {
        account_id: OtherOf(Row, AccountId),
        slot: Row.senderSlot,
        direction: Row.senderId === AccountId ? "Sent" : "Received",
        status: "Pending",
        expires: new Date(Row.expiresAt).toISOString(),
        link_id: Row.inviteId
    };
}

function LinkEntry(Link: LinkRow, AccountId: string){
    const Other = OtherOf(Link, AccountId);

    return {
        account_id: Other,
        linked_account_id: Other,
        slot: LinkSlotOf(Link, AccountId),
        ends: new Date(Link.endsAt).toISOString(),
        link_id: Link.linkId,
        prize_pool: []
    };
}

function ReadLists(AccountId: string){
    const Now = Clock();

    return GetDb().transaction((tx) => {
        ExpireInvitesInTx(tx, [AccountId], Now);

        return {
            Invites: PendingInvitesOf(tx, AccountId, Now).map((Row) => InviteEntry(Row, AccountId)),
            Links: ActiveLinksOf(tx, AccountId, Now).map((Link) => LinkEntry(Link, AccountId)).sort((A, B) => A.slot - B.slot)
        };
    });
}

// GET /slayerlink/invites
export function ListSlayerLinkInvites(AccountId: string): SlayerLinkReply {
    return Ok({ invites: ReadLists(AccountId).Invites });
}

// GET /slayerlink/links
export function ListSlayerLinks(AccountId: string): SlayerLinkReply {
    return Ok({ links: ReadLists(AccountId).Links });
}

// GET /slayerlink/status_good (the client polls it for news)
export function SlayerLinkStatus(AccountId: string): SlayerLinkReply {
    const Lists = ReadLists(AccountId);

    return Ok({
        invites: Lists.Invites,
        links: Lists.Links,
        config: { link_duration_hours: LINK_DURATION_HOURS, invite_expiry_hours: INVITE_EXPIRY_HOURS }
    });
}

// ---- Changes ----

// PUT /slayerlink/invite {account_id, slot, action_source}
export function InviteToSlayerLink(Caller: string, Body: any): SlayerLinkReply {
    const Target = Body?.account_id;
    const Slot = SlotOf(Body?.slot);
    const Refuse = (Status: number, Why: string) => {
        logger.info(`slayerlink: invite by=${Caller} to=${Shown(Target)} slot=${Slot ?? "?"} refused ${Status}: ${Why}`);
        return Fail(Status, Why);
    };

    if(!IsAccountIdShape(Target)){
        return Refuse(400, "no account to invite");
    }

    if(Slot === undefined){
        return Refuse(400, `the slot must be 1 to ${LINK_SLOTS}`);
    }

    if(Target === Caller){
        return Refuse(409, "you cannot link with yourself");
    }

    const Now = Clock();

    const Result = GetDb().transaction((tx): SlayerLinkReply | { InviteId: string, Again: boolean } => {
        if(!AccountExists(tx, Target)){
            return Refuse(404, "no such account");
        }

        if(IsBlockedEitherWayInTx(tx, Caller, Target)){
            return Refuse(403, "blocked");
        }

        if(!AreFriendsInTx(tx, Caller, Target)){
            return Refuse(403, "only friends can link");
        }

        ExpireInvitesInTx(tx, [Caller, Target], Now);

        const Pending = PendingInvitesOf(tx, Caller, Now);
        const ToTarget = Pending.find((Row) => Row.senderId === Caller && Row.targetId === Target);

        if(ToTarget !== undefined){
            return { InviteId: ToTarget.inviteId, Again: true };
        }

        if(Pending.some((Row) => Row.senderId === Target && Row.targetId === Caller)){
            return Refuse(409, "this player already invited you");
        }

        if(LinkedWith(tx, Caller, Target, Now)){
            return Refuse(409, "already linked with this player");
        }

        if(!FreeSlots(tx, Caller, Now).includes(Slot)){
            return Refuse(409, "the slot is already linked");
        }

        if(Pending.some((Row) => Row.senderId === Caller && Row.senderSlot === Slot)){
            return Refuse(409, "the slot already has an invite waiting");
        }

        if(FreeSlots(tx, Target, Now).length === 0){
            return Refuse(409, "the player has no free slot");
        }

        if(RecentInvitesBy(tx, Caller, Now) >= MAX_INVITES_PER_WINDOW){
            return Refuse(409, `too many invites: at most ${MAX_INVITES_PER_WINDOW} in ${INVITE_WINDOW_MS / 60000} minutes`);
        }

        const InviteId = crypto.randomUUID();

        tx.insert(slayerlinkinvites).values({
            inviteId: InviteId,
            senderId: Caller,
            targetId: Target,
            senderSlot: Slot,
            createdAt: Now,
            expiresAt: Now + INVITE_EXPIRY_HOURS * HOUR_MS,
            status: "PENDING"
        }).run();
        SweepInTx(tx, Now);

        return { InviteId, Again: false };
    });

    if(!("InviteId" in Result)){
        return Result;
    }

    logger.info(`slayerlink: invite by=${Caller} to=${Target} slot=${Slot} -> ${Result.Again ? "already invited" : "sent"} id=${Result.InviteId}`);

    return Ok({ link_id: Result.InviteId });
}

// The invite an answer is about. accept and reject are the invited player's; cancel is the sender's.
function FindInviteInTx(tx: Tx, Caller: string, Action_: Action, Ids: string[], Other: unknown): InviteRow | undefined {
    const Mine = Action_ === "cancel" ? slayerlinkinvites.senderId : slayerlinkinvites.targetId;
    const Theirs = Action_ === "cancel" ? slayerlinkinvites.targetId : slayerlinkinvites.senderId;

    for(const Id of Ids){
        const ById = tx.select().from(slayerlinkinvites).where(and(eq(slayerlinkinvites.inviteId, Id), eq(Mine, Caller))).get();

        if(ById !== undefined){
            return ById;
        }
    }

    if(!IsAccountIdShape(Other)){
        return undefined;
    }

    // The pending one if there is one, else the latest (a repeated answer finds it)
    const Rows = tx.select().from(slayerlinkinvites).where(and(eq(Mine, Caller), eq(Theirs, Other))).orderBy(asc(slayerlinkinvites.createdAt)).all();

    return Rows.find((Row) => Row.status === "PENDING") ?? Rows.at(-1);
}

// POST /slayerlink/invite {account_id, action, slot, action_source}
export function AnswerSlayerLinkInvite(Caller: string, Body: any): SlayerLinkReply {
    const Raw = typeof Body?.action === "string" ? Body.action.toLowerCase() : "";
    const Other = Body?.account_id;
    const Refuse = (Status: number, Why: string) => {
        logger.info(`slayerlink: ${Raw || "answer"} by=${Caller} other=${Shown(Other)} refused ${Status}: ${Why}`);
        return Fail(Status, Why);
    };

    if(Raw !== "accept" && Raw !== "reject" && Raw !== "cancel"){
        return Refuse(400, "the action must be accept, reject or cancel");
    }

    const Action_: Action = Raw;
    const Ids = [Body?.link_id, Body?.invite_id].filter((Id): Id is string => typeof Id === "string" && Id.length > 0 && Id.length <= 64);
    const Now = Clock();

    const Result = GetDb().transaction((tx): SlayerLinkReply | { Row: InviteRow, Again: boolean, Slot?: number } => {
        const Row = FindInviteInTx(tx, Caller, Action_, Ids, Other);

        if(Row === undefined){
            return Refuse(404, "no such invite");
        }

        if(Row.status !== "PENDING"){
            if(Row.status === ANSWERED[Action_]){
                return { Row, Again: true };
            }

            return Refuse(409, Row.status === "EXPIRED" ? "the invite ran out" : `the invite was already ${Row.status.toLowerCase()}`);
        }

        if(Row.expiresAt <= Now){
            tx.update(slayerlinkinvites).set({ status: "EXPIRED" }).where(eq(slayerlinkinvites.inviteId, Row.inviteId)).run();
            return Refuse(409, "the invite ran out");
        }

        let TargetSlot: number | undefined;

        if(Action_ === "accept"){
            if(IsBlockedEitherWayInTx(tx, Row.senderId, Row.targetId) || !AreFriendsInTx(tx, Row.senderId, Row.targetId)){
                return Refuse(403, "only friends can link");
            }

            if(LinkedWith(tx, Row.senderId, Row.targetId, Now)){
                return Refuse(409, "already linked with this player");
            }

            if(!FreeSlots(tx, Row.senderId, Now).includes(Row.senderSlot)){
                return Refuse(409, "the other player's slot is taken");
            }

            const Free = FreeSlots(tx, Row.targetId, Now);
            const Asked = SlotOf(Body?.slot);

            TargetSlot = Asked !== undefined && Free.includes(Asked) ? Asked : Free[0];

            if(TargetSlot === undefined){
                return Refuse(409, "no free slot");
            }

            tx.insert(slayerlinks).values({
                linkId: Row.inviteId,
                senderId: Row.senderId,
                targetId: Row.targetId,
                senderSlot: Row.senderSlot,
                targetSlot: TargetSlot,
                createdAt: Now,
                endsAt: Now + LINK_DURATION_HOURS * HOUR_MS
            }).run();
        }

        tx.update(slayerlinkinvites).set({ status: ANSWERED[Action_] }).where(eq(slayerlinkinvites.inviteId, Row.inviteId)).run();

        return { Row, Again: false, Slot: TargetSlot };
    });

    if(!("Row" in Result)){
        return Result;
    }

    logger.info(`slayerlink: ${Action_} by=${Caller} other=${OtherOf(Result.Row, Caller)} id=${Result.Row.inviteId} -> ${Result.Again ? "already done" : ANSWERED[Action_].toLowerCase()}${Result.Slot !== undefined ? ` (slots ${Result.Row.senderSlot} and ${Result.Slot})` : ""}`);

    return Ok({ link_id: Result.Row.inviteId });
}

// DELETE /slayerlink/invites/{account_id}: the caller's own id withdraws every invite the caller sent and
// declines every one they got; another player's id does that for the invites between the two
export function DeleteSlayerLinkInvites(Caller: string, AccountId: unknown): SlayerLinkReply {
    if(!IsAccountIdShape(AccountId)){
        logger.info(`slayerlink: delete invites by=${Caller} refused 400: no account`);
        return Fail(400, "no account");
    }

    const Now = Clock();
    const Counts = GetDb().transaction((tx) => {
        ExpireInvitesInTx(tx, [Caller], Now);

        const Sent = AccountId === Caller ? eq(slayerlinkinvites.senderId, Caller) : and(eq(slayerlinkinvites.senderId, Caller), eq(slayerlinkinvites.targetId, AccountId));
        const Got = AccountId === Caller ? eq(slayerlinkinvites.targetId, Caller) : and(eq(slayerlinkinvites.targetId, Caller), eq(slayerlinkinvites.senderId, AccountId));

        return {
            Cancelled: tx.update(slayerlinkinvites).set({ status: "CANCELED" }).where(and(eq(slayerlinkinvites.status, "PENDING"), Sent)).run().changes,
            Declined: tx.update(slayerlinkinvites).set({ status: "DECLINED" }).where(and(eq(slayerlinkinvites.status, "PENDING"), Got)).run().changes
        };
    });

    logger.info(`slayerlink: delete invites by=${Caller} of=${AccountId === Caller ? "all" : AccountId} -> ${Counts.Cancelled} withdrawn, ${Counts.Declined} declined`);

    return Ok({});
}

// DELETE /slayerlink/links {account_id, slot, delete_pair}: ends the caller's link in that slot, or with
// that player. The link is one row for both players, so it ends for both whatever delete_pair says
// (logged). Nothing to remove still answers 200.
export function DeleteSlayerLink(Caller: string, Body: any, Query: any): SlayerLinkReply {
    const Slot = SlotOf(Body?.slot ?? Query?.slot);
    const Other = Body?.account_id ?? Query?.account_id;
    const HasOther = IsAccountIdShape(Other);

    if(Slot === undefined && !HasOther){
        logger.info(`slayerlink: delete link by=${Caller} refused 400: no slot or account`);
        return Fail(400, "no slot or account");
    }

    const Now = Clock();
    const Removed = GetDb().transaction((tx) => {
        const Links = ActiveLinksOf(tx, Caller, Now);
        const Link = Links.find((Each) => (Slot === undefined || LinkSlotOf(Each, Caller) === Slot) && (!HasOther || OtherOf(Each, Caller) === Other))
            ?? Links.find((Each) => HasOther && OtherOf(Each, Caller) === Other)
            ?? Links.find((Each) => Slot !== undefined && LinkSlotOf(Each, Caller) === Slot);

        if(Link !== undefined){
            tx.delete(slayerlinks).where(eq(slayerlinks.linkId, Link.linkId)).run();
        }

        return Link;
    });

    logger.info(`slayerlink: delete link by=${Caller} slot=${Slot ?? "-"} other=${HasOther ? Other : "-"} delete_pair=${JSON.stringify(Body?.delete_pair ?? null)} -> ${Removed !== undefined ? `removed ${Removed.linkId} (with ${OtherOf(Removed, Caller)})` : "nothing to remove"}`);

    return Ok({});
}

// POST /slayerlink/availability {account_ids}: which of these friends the caller could invite now
export function SlayerLinkAvailability(Caller: string, Body: any): SlayerLinkReply {
    const Ids = [...new Set((Array.isArray(Body?.account_ids) ? Body.account_ids : []).filter((Id: unknown): Id is string => typeof Id === "string" && Id.length > 0 && Id.length <= 128))].slice(0, MAX_AVAILABILITY_IDS) as string[];
    const Now = Clock();

    const Availability = GetDb().transaction((tx) => {
        ExpireInvitesInTx(tx, [Caller], Now);

        const Pending = PendingInvitesOf(tx, Caller, Now);
        // A slot of the caller's that neither a link nor a waiting invite holds
        const CallerHasSlot = FreeSlots(tx, Caller, Now).some((Slot) => !Pending.some((Row) => Row.senderId === Caller && Row.senderSlot === Slot));

        return Ids.map((Id) => ({
            account_id: Id,
            available: CallerHasSlot
                && Id !== Caller
                && IsAccountIdShape(Id)
                && AccountExists(tx, Id)
                && AreFriendsInTx(tx, Caller, Id)
                && !IsBlockedEitherWayInTx(tx, Caller, Id)
                && !LinkedWith(tx, Caller, Id, Now)
                && !Pending.some((Row) => OtherOf(Row, Caller) === Id)
                && FreeSlots(tx, Id, Now).length > 0
        }));
    });

    logger.debug(`slayerlink: availability for ${Caller}: ${Availability.filter((Entry) => Entry.available).length} of ${Availability.length}`);

    return Ok({ availability: Availability });
}
