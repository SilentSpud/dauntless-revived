import { and, eq, inArray, or, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { blocks, friendships, guildinvites, slayerlinkinvites, users } from "../db/schema";
import { logger } from "../logger";
import type { Tx } from "./savehistory";

// Friends list and blocklist (roadmap 1.9, parties plan phase 3), stored in SQLite so a
// request to an offline player survives restarts. The client reads them over the Epic-style
// friends routes (routes/friends.ts). Nobody shows as online unless chat relays presence
// (CHAT=1 and CHAT_PRESENCE=1, realtime/presence.ts).
//
// One friendships row per pair, ids sorted: status PENDING (requesterId asked) or ACCEPTED.
// A block removes any friendship between the two and the guild and party invites either one sent the
// other, and stops requests, party invites and guild invites in both directions. A block or an unfriend
// also cancels the pending Slayer Link invites between the two (controllers/slayerlinks.ts).
//
// Limits on new requests (docs/findings/social.md): at most MAX_PENDING_OUTGOING unanswered
// requests an account has sent, and at most MAX_REQUESTS_PER_WINDOW new requests per
// REQUEST_WINDOW_MS (in memory; accepting a request the other player sent never counts). Both
// answer 409, which the client shows as a failure toast.

export const MAX_FRIENDSHIPS_PER_ACCOUNT = 200;
export const MAX_BLOCKS_PER_ACCOUNT = 200;
export const MAX_PENDING_OUTGOING = 50;
export const MAX_REQUESTS_PER_WINDOW = 20;
export const REQUEST_WINDOW_MS = 10 * 60 * 1000;

// Times of each account's recent new requests (sliding window)
const RecentRequests = new Map<string, number[]>();
let Clock: () => number = () => Date.now();

// Called after a new block, so the in-memory party invites between the two go too
// (controllers/party.ts registers it; party.ts imports this file, so not the other way round)
let BlockHook: (Blocker: string, Blocked: string) => void = () => {};

export function SetBlockHook(Hook: (Blocker: string, Blocked: string) => void){
    BlockHook = Hook;
}

// A friendship that began (a request was accepted) or ended (unfriended, or a block removed it). The chat
// server listens with CHAT_PRESENCE=1 (realtime/presence.ts): friends' online status follows at once.
// These moments are the ones the presence service of Harmonic's fork reacted to
// (github.com/Harmonicrain/Undaunted 895f7c7); none of its code is used.
// Called after the change is stored; a listener that throws is logged and changes nothing here.
export type FriendshipEvent =
    | { Kind: "accepted", Requester: string, Accepter: string, CreatedAt: number }
    | { Kind: "ended", A: string, B: string, Why: "removed" | "blocked" };

const FriendshipListeners = new Set<(Event: FriendshipEvent) => void>();

// Returns the function that removes the listener again
export function AddFriendshipListener(Listener: (Event: FriendshipEvent) => void): () => void {
    FriendshipListeners.add(Listener);

    return () => {
        FriendshipListeners.delete(Listener);
    };
}

function NotifyFriendship(Event: FriendshipEvent){
    for(const Listener of [...FriendshipListeners]){
        try{
            Listener(Event);
        }
        catch(error){
            logger.warn(error, `friends: a listener failed on a friendship that ${Event.Kind === "accepted" ? "began" : "ended"}`);
        }
    }
}

// Tests only: a controllable clock for the request window, and an empty window
export function SetFriendsClockForTests(NewClock?: () => number){
    Clock = NewClock ?? (() => Date.now());
}

export function ResetFriendsMemoryForTests(){
    RecentRequests.clear();
}

function RecentRequestTimes(AccountId: string, Now: number){
    const Recent = (RecentRequests.get(AccountId) ?? []).filter((At) => Now - At < REQUEST_WINDOW_MS);

    if(Recent.length > 0){
        RecentRequests.set(AccountId, Recent);
    }
    else{
        RecentRequests.delete(AccountId);
    }

    // Accounts that stopped sending are forgotten once the map grows
    if(RecentRequests.size > 1000){
        for(const [Other, Times] of [...RecentRequests.entries()]){
            if(Times.every((At) => Now - At >= REQUEST_WINDOW_MS)){
                RecentRequests.delete(Other);
            }
        }
    }

    return Recent;
}

export type FriendEntry = {
    accountId: string,
    status: "ACCEPTED" | "PENDING",
    direction: "INBOUND" | "OUTBOUND",
    created: string
};

function Pair(A: string, B: string): [string, string] {
    return A < B ? [A, B] : [B, A];
}

function PairCondition(A: string, B: string){
    const [Low, High] = Pair(A, B);

    return and(eq(friendships.userLow, Low), eq(friendships.userHigh, High));
}

function AccountExists(tx: Tx, AccountId: string){
    return tx.select({ userId: users.userId }).from(users).where(eq(users.userId, AccountId)).get() != undefined;
}

export function IsBlockedEitherWayInTx(tx: Tx, A: string, B: string){
    return tx.select({ blockerId: blocks.blockerId }).from(blocks).where(or(
        and(eq(blocks.blockerId, A), eq(blocks.blockedId, B)),
        and(eq(blocks.blockerId, B), eq(blocks.blockedId, A))
    )).get() != undefined;
}

export function IsBlockedEitherWay(A: string, B: string){
    return GetDb().transaction((tx) => IsBlockedEitherWayInTx(tx, A, B));
}

// Friends on both sides (the request was accepted)
export function AreFriendsInTx(tx: Tx, A: string, B: string){
    return tx.select({ status: friendships.status }).from(friendships).where(PairCondition(A, B)).get()?.status === "ACCEPTED";
}

// The Slayer Link invites between the two that still wait for an answer (slayerlinks.ts reads the table).
// One update per direction: SQLite's planner fails on an UPDATE of the partial index's column with an OR
// of the two directions and RETURNING ("internal query planner error").
function CancelSlayerLinkInvitesInTx(tx: Tx, A: string, B: string){
    let Cancelled = 0;

    for(const [Sender, Target] of [[A, B], [B, A]]){
        Cancelled += tx.update(slayerlinkinvites).set({ status: "CANCELED" })
            .where(and(eq(slayerlinkinvites.status, "PENDING"), eq(slayerlinkinvites.senderId, Sender), eq(slayerlinkinvites.targetId, Target)))
            .run().changes;
    }

    return Cancelled;
}

function SlayerLinkNote(Cancelled: number){
    return Cancelled > 0 ? ` (${Cancelled} Slayer Link invite(s) between them cancelled)` : "";
}

// Chat (src/realtime/muc.ts): which of these recipients have blocked the sender, in one query per message
export function BlockersAmong(Sender: string, Recipients: string[]): Set<string> {
    const Ids = [...new Set(Recipients)].filter((Id) => Id !== Sender);

    if(Ids.length === 0){
        return new Set();
    }

    return new Set(GetDb().select({ blockerId: blocks.blockerId }).from(blocks)
        .where(and(eq(blocks.blockedId, Sender), inArray(blocks.blockerId, Ids))).all().map((Row) => Row.blockerId));
}

function CountFriendships(tx: Tx, AccountId: string){
    return tx.select({ n: sql<number>`count(*)` }).from(friendships)
        .where(or(eq(friendships.userLow, AccountId), eq(friendships.userHigh, AccountId))).get()?.n ?? 0;
}

// GET /friends/api/public/friends/:id[?includePending=true]: the client wraps the reply as
// {"friends": <reply>}, so this is the bare array it expects
export function ListFriends(AccountId: string, IncludePending: boolean): FriendEntry[] {
    const Rows = GetDb().select().from(friendships)
        .where(or(eq(friendships.userLow, AccountId), eq(friendships.userHigh, AccountId))).all();

    return Rows
        .filter((Row) => Row.status === "ACCEPTED" || (IncludePending && Row.status === "PENDING"))
        .sort((A, B) => A.createdAt - B.createdAt)
        .map((Row) => ({
            accountId: Row.userLow === AccountId ? Row.userHigh : Row.userLow,
            status: Row.status === "ACCEPTED" ? "ACCEPTED" : "PENDING",
            direction: Row.requesterId === AccountId ? "OUTBOUND" : "INBOUND",
            created: new Date(Row.createdAt).toISOString()
        }));
}

export function ListBlocked(AccountId: string): string[] {
    return GetDb().select({ blockedId: blocks.blockedId }).from(blocks).where(eq(blocks.blockerId, AccountId))
        .all().sort((A, B) => A.blockedId.localeCompare(B.blockedId)).map((Row) => Row.blockedId);
}

export type FriendResult =
    | { ok: true, Result: "requested" | "accepted" | "already_friends" | "already_requested" | "removed" | "not_friends" | "blocked" | "unblocked" | "already_blocked" | "not_blocked" }
    | { ok: false, Status: 400 | 403 | 404 | 409, Error: "bad_request" | "self" | "not_found" | "blocked" | "limit" | "pending_limit" | "rate" };

// POST /friends/api/public/friends/:me/:them. A request the other side already sent is
// accepted; asking again, or asking an existing friend, changes nothing.
export function SendOrAcceptFriendRequest(Me: string, Them: string): FriendResult {
    if(Me === Them){
        return { ok: false, Status: 400, Error: "self" };
    }

    let RequestedAt = 0;
    const Result = GetDb().transaction((tx): FriendResult => {
        if(!AccountExists(tx, Them)){
            return { ok: false, Status: 404, Error: "not_found" };
        }

        if(IsBlockedEitherWayInTx(tx, Me, Them)){
            return { ok: false, Status: 403, Error: "blocked" };
        }

        const Existing = tx.select().from(friendships).where(PairCondition(Me, Them)).get();
        const Now = Date.now();

        if(Existing != undefined){
            if(Existing.status === "ACCEPTED"){
                return { ok: true, Result: "already_friends" };
            }

            if(Existing.requesterId === Me){
                return { ok: true, Result: "already_requested" };
            }

            tx.update(friendships).set({ status: "ACCEPTED", updatedAt: Now }).where(PairCondition(Me, Them)).run();
            RequestedAt = Existing.createdAt;
            return { ok: true, Result: "accepted" };
        }

        if(CountFriendships(tx, Me) >= MAX_FRIENDSHIPS_PER_ACCOUNT || CountFriendships(tx, Them) >= MAX_FRIENDSHIPS_PER_ACCOUNT){
            return { ok: false, Status: 409, Error: "limit" };
        }

        const PendingOutgoing = tx.select({ n: sql<number>`count(*)` }).from(friendships)
            .where(and(eq(friendships.requesterId, Me), eq(friendships.status, "PENDING"))).get()?.n ?? 0;

        if(PendingOutgoing >= MAX_PENDING_OUTGOING){
            return { ok: false, Status: 409, Error: "pending_limit" };
        }

        const WindowNow = Clock();
        const Recent = RecentRequestTimes(Me, WindowNow);

        if(Recent.length >= MAX_REQUESTS_PER_WINDOW){
            return { ok: false, Status: 409, Error: "rate" };
        }

        const [Low, High] = Pair(Me, Them);

        tx.insert(friendships).values({ userLow: Low, userHigh: High, requesterId: Me, status: "PENDING", createdAt: Now, updatedAt: Now }).run();
        RecentRequests.set(Me, [...Recent, WindowNow]);
        return { ok: true, Result: "requested" };
    });

    logger.info(`friends: request by=${Me} to=${Them} -> ${Result.ok ? Result.Result : `${Result.Status} ${Result.Error}`}`);

    if(Result.ok && Result.Result === "accepted"){
        NotifyFriendship({ Kind: "accepted", Requester: Them, Accepter: Me, CreatedAt: RequestedAt });
    }

    return Result;
}

// DELETE /friends/api/public/friends/:me/:them: unfriend, withdraw a request or decline one
export function RemoveFriend(Me: string, Them: string): FriendResult {
    const { Removed, Cancelled } = GetDb().transaction((tx) => ({
        Removed: tx.delete(friendships).where(PairCondition(Me, Them)).returning({ status: friendships.status }).all(),
        Cancelled: CancelSlayerLinkInvitesInTx(tx, Me, Them)
    }));
    const Result: FriendResult = { ok: true, Result: Removed.length > 0 ? "removed" : "not_friends" };

    logger.info(`friends: remove by=${Me} other=${Them} -> ${Result.Result}${SlayerLinkNote(Cancelled)}`);

    if(Removed.some((Row) => Row.status === "ACCEPTED")){
        NotifyFriendship({ Kind: "ended", A: Me, B: Them, Why: "removed" });
    }

    return Result;
}

export function BlockPlayer(Me: string, Them: string): FriendResult {
    if(Me === Them){
        return { ok: false, Status: 400, Error: "self" };
    }

    let GuildInvitesDropped = 0;
    let SlayerLinkInvitesCancelled = 0;
    let WereFriends = false;
    const Result = GetDb().transaction((tx): FriendResult => {
        if(!AccountExists(tx, Them)){
            return { ok: false, Status: 404, Error: "not_found" };
        }

        if(tx.select({ blockerId: blocks.blockerId }).from(blocks).where(and(eq(blocks.blockerId, Me), eq(blocks.blockedId, Them))).get() != undefined){
            return { ok: true, Result: "already_blocked" };
        }

        const Count = tx.select({ n: sql<number>`count(*)` }).from(blocks).where(eq(blocks.blockerId, Me)).get()?.n ?? 0;

        if(Count >= MAX_BLOCKS_PER_ACCOUNT){
            return { ok: false, Status: 409, Error: "limit" };
        }

        tx.insert(blocks).values({ blockerId: Me, blockedId: Them, createdAt: Date.now() }).run();
        WereFriends = tx.delete(friendships).where(PairCondition(Me, Them)).returning({ status: friendships.status }).all().some((Row) => Row.status === "ACCEPTED");
        GuildInvitesDropped = tx.delete(guildinvites).where(or(
            and(eq(guildinvites.inviterId, Me), eq(guildinvites.inviteeId, Them)),
            and(eq(guildinvites.inviterId, Them), eq(guildinvites.inviteeId, Me))
        )).returning({ inviteId: guildinvites.inviteId }).all().length;
        SlayerLinkInvitesCancelled = CancelSlayerLinkInvitesInTx(tx, Me, Them);

        return { ok: true, Result: "blocked" };
    });

    if(Result.ok && Result.Result === "blocked"){
        BlockHook(Me, Them);
    }

    logger.info(`friends: block by=${Me} target=${Them} -> ${Result.ok ? Result.Result : `${Result.Status} ${Result.Error}`}${GuildInvitesDropped > 0 ? ` (${GuildInvitesDropped} guild invite(s) between them removed)` : ""}${SlayerLinkNote(SlayerLinkInvitesCancelled)}`);

    if(Result.ok && Result.Result === "blocked" && WereFriends){
        NotifyFriendship({ Kind: "ended", A: Me, B: Them, Why: "blocked" });
    }

    return Result;
}

export function UnblockPlayer(Me: string, Them: string): FriendResult {
    const Removed = GetDb().delete(blocks).where(and(eq(blocks.blockerId, Me), eq(blocks.blockedId, Them))).returning({ blockedId: blocks.blockedId }).all();
    const Result: FriendResult = { ok: true, Result: Removed.length > 0 ? "unblocked" : "not_blocked" };

    logger.info(`friends: unblock by=${Me} target=${Them} -> ${Result.Result}`);

    return Result;
}
