import crypto from "node:crypto";
import { and, asc, desc, eq, gt, lte, sql } from "drizzle-orm";
import { GetDb } from "../db";
import { guildinvites, guildmembers, guilds, users } from "../db/schema";
import { logger } from "../logger";
import type { Tx } from "./savehistory";
import { IsAccountIdShape } from "./login";
import { IsBlockedEitherWayInTx } from "./friends";
import { SeenWithinMs } from "./party";

// Guilds (roadmap 3.11): the 1.4.4 client's v2 guild API (the eleven *_v2 endpoint keys in
// native/internal-server/dllmain.cpp), stored in SQLite (migration 0013_guilds) so a guild survives
// restarts and an invite waits for a player who is offline. The contract was read from the game's
// executable; docs/findings/social.md has the evidence and routes/guild.ts the routes.
//
// - Every reply is read through the Phoenix envelope {"code", "message", "payload"}. Success is a
//   2xx status AND a JSON body, so a success never answers 204. On an error status the client maps
//   "code" to its EGuildRequestError and shows the matching text; an unknown or empty code shows as
//   "Unable to create guild."
// - Replies that carry a guild or the invite list also copy the payload's fields to the root, in case
//   the client reads them flat (the envelope reader ignores extra root keys).
// - A guild is created by the Ramsgate game server (the Create button is an RPC to it), with the
//   game-server key. The game server passes on the leader id the client sent and no token of that
//   player (a token it sends is its own login's, exe 0x140ac78a4), so a create is only accepted when
//   the leader validated that same name and nameplate with their own token in the last 15 minutes.
//   GUILD_CREATE_ACTIVITY_FALLBACK=1 also accepts a leader heard from in the last minute (party polls,
//   heartbeats), with a warning in the log.
// - A block removes the guild invites between the two players, and an invite whose inviter is no
//   longer a Leader or Officer of the guild (demoted, kicked, left) goes too.
// - Other members and invitees see a change at their next GET /guild or GET /guild/invite/player
//   (login, world load, or their own guild action): the client gets no push.
//
// In memory only (losing them on a restart is harmless): validate tickets, the creation and invite
// rate windows, and the pause on re-inviting a player who declined.

// The codes the server sends, and the EGuildRequestError each maps to in the client
export const GUILD_CODE = {
    InvalidPermission: "SlyAdorableQuillshot",
    NotInAGuild: "ExcludedAdorableQuillshot",
    NameInvalidLength: "ObedientAdorableQuillshot",
    NameTaken: "SeizedAdorableQuillshot",
    NameProfane: "NastyAdorableQuillshot",
    NameTooManyNumbers: "NumberedAdorableQuillshot",
    NameTooManyLetters: "LetteredAdorableQuillshot",
    NameplateInvalidLength: "DutifulAdorableQuillshot",
    NameplateTaken: "CapturedAdorableQuillshot",
    NameplateProfane: "DirtyAdorableQuillshot",
    AlreadyInAGuild: "OccupiedAdorableQuillshot",
    TargetAlreadyInYourGuild: "ClonedAdorableQuillshot",
    TargetAlreadyHasGuildInvite: "RedundantAdorableQuillshot",
    GuildIsFull: "StuffedAdorableQuillshot",
    InviteNotFound: "UninvitedAdorableQuillshot",
    LeaderCannotLeave: "ChiefAdorableQuillshot",
    InvalidRank: "DocileAdorableQuillshot",
    Unknown: ""
} as const;

type GuildCode = typeof GUILD_CODE[keyof typeof GUILD_CODE];

// The client's own texts where it has one (the create-guild widget), else a short reason
const MESSAGES: Record<string, string> = {
    [GUILD_CODE.InvalidPermission]: "You do not have permission to do that.",
    [GUILD_CODE.NotInAGuild]: "Not in that guild.",
    [GUILD_CODE.NameInvalidLength]: "Name is invalid. Must contain 4-15 english letters and digits.",
    [GUILD_CODE.NameTaken]: "Guild name already in use.",
    [GUILD_CODE.NameProfane]: "Guild name contains profanity.",
    [GUILD_CODE.NameTooManyNumbers]: "Guild name must have 6 numbers or less.",
    [GUILD_CODE.NameTooManyLetters]: "Guild name must not have more than 6 of the same letter in a row.",
    [GUILD_CODE.NameplateInvalidLength]: "Nameplate is invalid. Must contain 2-6 english letters and digits.",
    [GUILD_CODE.NameplateTaken]: "Guild nameplate already in use.",
    [GUILD_CODE.NameplateProfane]: "Guild nameplate contains profanity.",
    [GUILD_CODE.AlreadyInAGuild]: "Unable to create guild. You are already in a guild.",
    [GUILD_CODE.TargetAlreadyInYourGuild]: "That player is already in your guild.",
    [GUILD_CODE.TargetAlreadyHasGuildInvite]: "That player already has an invite from your guild.",
    [GUILD_CODE.GuildIsFull]: "The guild is full.",
    [GUILD_CODE.InviteNotFound]: "Guild invite not found.",
    [GUILD_CODE.LeaderCannotLeave]: "The guild leader cannot leave the guild.",
    [GUILD_CODE.InvalidRank]: "Invalid guild rank."
};

export const DEFAULT_MAX_MEMBERS = 100;
export const DEFAULT_INVITE_TTL_DAYS = 7;
export const MAX_OPEN_INVITES_PER_GUILD = 50;
export const MAX_OPEN_INVITES_PER_INVITEE = 20;
export const MAX_INVITES_PER_INVITER_PER_HOUR = 30;
export const CREATE_INTERVAL_MS = 10 * 60 * 1000;
export const VALIDATE_TICKET_MS = 15 * 60 * 1000;
export const CREATE_ACTIVITY_MS = 60 * 1000;
// Validated name and nameplate pairs kept per account, in case Create is pressed before the validate
// of the final name has come back
export const MAX_TICKETS_PER_ACCOUNT = 5;
// After a player declines a guild's invite, that guild cannot invite them again for this long
export const DECLINE_COOLDOWN_MS = 24 * 60 * 60 * 1000;

export type GuildRank = "Member" | "Officer" | "Leader";

// The guild object of every guild reply (FGuildData / FGuildMemberData in the client). Every field is
// a string except maximum_guild_members, a JSON number.
export type GuildData = {
    id: string,
    name: string,
    nameplate: string,
    leader_account_id: string,
    members: { phx_account_id: string, rank: string }[],
    maximum_guild_members: number
};

export type GuildInviteData = { id: string, guild_id: string, guild_name: string, inviter_account_id: string };

// Status and body for the route; Body undefined answers no body (only GET /guild's "no guild")
export type GuildReply = { Status: number, Body: unknown, Code?: GuildCode };

// ---- Settings ----

export function MaxGuildMembers(){
    const Value = Number(process.env.GUILD_MAX_MEMBERS);

    return Number.isInteger(Value) && Value >= 1 && Value <= 10000 ? Value : DEFAULT_MAX_MEMBERS;
}

export function GuildInviteTtlMs(){
    const Value = Number(process.env.GUILD_INVITE_TTL_DAYS);
    const Days = Number.isFinite(Value) && Value > 0 && Value <= 365 ? Value : DEFAULT_INVITE_TTL_DAYS;

    return Math.round(Days * 24 * 60 * 60 * 1000);
}

// Opt-in: also accept a create for a leader who validated any name, or was heard from in the last
// minute, when there is no validate of the exact name (only if the live test shows the client never
// validates the final name). Read on every request.
function ActivityFallbackOn(){
    return process.env.GUILD_CREATE_ACTIVITY_FALLBACK === "1";
}

// GUILD_RESERVED_NAMES=0 turns off the reserved staff and project words (the offensive tags stay)
function ReservedNamesOn(){
    return process.env.GUILD_RESERVED_NAMES !== "0";
}

// ---- In memory ----

type ValidateTicket = { NameKey: string, NameplateKey: string, At: number };

// Per account, oldest first, at most MAX_TICKETS_PER_ACCOUNT
const Tickets = new Map<string, ValidateTicket[]>();
const LastCreated = new Map<string, number>();
const InvitesSent = new Map<string, number[]>();
// "<guildId>|<inviteeId>" -> when the invitee declined that guild's invite
const Declined = new Map<string, number>();
let Clock: () => number = () => Date.now();
let LastSweep = 0;

export function GuildNow(){
    return Clock();
}

// Tests only: a controllable clock (also used for invite expiry), and empty in-memory state
export function SetGuildClockForTests(NewClock?: () => number){
    Clock = NewClock ?? (() => Date.now());
}

export function ResetGuildMemoryForTests(){
    Tickets.clear();
    LastCreated.clear();
    InvitesSent.clear();
    Declined.clear();
    LastSweep = 0;
}

function LiveTickets(Account: string, Now: number){
    return (Tickets.get(Account) ?? []).filter((Ticket) => Now - Ticket.At <= VALIDATE_TICKET_MS);
}

function ForgetOldMemory(Now: number){
    for(const Account of [...Tickets.keys()]){
        const Live = LiveTickets(Account, Now);

        if(Live.length > 0) Tickets.set(Account, Live);
        else Tickets.delete(Account);
    }

    for(const [Pair, At] of [...Declined.entries()]){
        if(Now - At >= DECLINE_COOLDOWN_MS) Declined.delete(Pair);
    }

    for(const [Account, At] of [...LastCreated.entries()]){
        if(Now - At > CREATE_INTERVAL_MS) LastCreated.delete(Account);
    }

    for(const [Account, Times] of [...InvitesSent.entries()]){
        const Recent = Times.filter((At) => Now - At < 60 * 60 * 1000);

        if(Recent.length > 0) InvitesSent.set(Account, Recent);
        else InvitesSent.delete(Account);
    }
}

// At most once a minute: delete expired invites and forget old in-memory entries
function Sweep(){
    const Now = Clock();

    if(Now - LastSweep < 60 * 1000){
        return;
    }

    LastSweep = Now;
    ForgetOldMemory(Now);

    const Removed = GetDb().delete(guildinvites).where(lte(guildinvites.expiresAt, Now)).returning({ inviteId: guildinvites.inviteId }).all();

    if(Removed.length > 0){
        logger.info(`guild: removed ${Removed.length} expired invite(s)`);
    }
}

// ---- Replies ----

function Envelope(Payload: object){
    return { code: "OK", message: "", payload: Payload };
}

function Ack(): GuildReply {
    return { Status: 200, Body: Envelope({}) };
}

// Wrapped, plus flat copies of the payload's fields at the root
function WithPayload(Payload: Record<string, unknown>): GuildReply {
    return { Status: 200, Body: { ...Envelope(Payload), ...Payload } };
}

function Refuse(Status: number, Code: GuildCode, Message?: string): GuildReply {
    return { Status: Status, Body: { code: Code, message: Message ?? MESSAGES[Code] ?? "", payload: {} }, Code: Code };
}

function Describe(Reply: GuildReply){
    return Reply.Status < 300 ? `${Reply.Status}` : `${Reply.Status} ${Reply.Code === "" ? "(no code)" : Reply.Code}`;
}

// A name as typed, safe for one log line
function Quoted(Value: unknown){
    return typeof Value === "string" ? JSON.stringify(Value.slice(0, 32)) : `<${typeof Value}>`;
}

// ---- Names and nameplates ----

const NAME_RULE = /^[A-Za-z0-9]{4,15}$/;
const NAMEPLATE_RULE = /^[A-Za-z0-9]{2,6}$/;
const MAX_DIGITS = 6;
const MAX_RUN = 6;

// A short list; GUILD_NAME_DENYLIST adds more. Compared after lowercasing and undoing common digit
// swaps, as a substring, so keep entries long enough not to hit ordinary words.
const BUILT_IN_DENYLIST = [
    "fuck", "shit", "cunt", "bitch", "whore", "slut", "nigger", "nigga", "faggot", "retard",
    "nazi", "hitler", "penis", "vagina", "pussy", "dildo", "twat", "wank", "porn", "kike"
];

function Unleet(Text: string){
    return Text.toLowerCase().replace(/[013457]/g, (Digit) => ({ "0": "o", "1": "i", "3": "e", "4": "a", "5": "s", "7": "t" } as Record<string, string>)[Digit]);
}

function Denylist(){
    const Extra = (process.env.GUILD_NAME_DENYLIST ?? "").split(",").map((Word) => Unleet(Word.trim())).filter((Word) => /^[a-z0-9]{2,32}$/.test(Word));

    return [...BUILT_IN_DENYLIST, ...Extra];
}

// Offensive words too short for the substring list, refused only as the whole name or nameplate
// (compared as typed and after undoing digit swaps). "kkk" never occurs in an ordinary word, so it is
// also refused anywhere in the text.
const OFFENSIVE_WHOLE = ["kkk", "fag", "fags", "nig", "nigs", "fck", "fuk", "rape", "isis", "ss", "1488", "hh88"];
const OFFENSIVE_ANYWHERE = ["kkk"];

export function IsProfaneGuildText(Text: string){
    const Lower = Text.toLowerCase();
    const Plain = Unleet(Text);

    return Denylist().some((Word) => Plain.includes(Word))
        || OFFENSIVE_ANYWHERE.some((Word) => Plain.includes(Word))
        || OFFENSIVE_WHOLE.includes(Lower) || OFFENSIVE_WHOLE.includes(Plain);
}

// Words that would let a guild pose as the server's staff or the project. Compared lowercased and after
// undoing digit swaps: the first list anywhere in a name or nameplate, the second as the whole name,
// the third as the whole nameplate. They answer "already in use". GUILD_RESERVED_NAMES=0 turns them off.
const RESERVED_ANYWHERE = ["admin", "moderator", "official", "gamemaster", "staff", "dauntlessrevived", "phoenixlabs"];
const RESERVED_WHOLE_NAME = ["dauntless", "phoenix", "revived", "support", "system", "server", "servers", "mods", "developer", "developers", "devteam"];
const RESERVED_WHOLE_NAMEPLATE = ["gm", "gms", "dev", "devs", "mod", "mods", "sys", "phx", "dr", "drev", "undt"];

export function IsReservedGuildText(Text: string, IsNameplate: boolean){
    if(!ReservedNamesOn()){
        return false;
    }

    const Forms = [Text.toLowerCase(), Unleet(Text)];
    const Whole = IsNameplate ? RESERVED_WHOLE_NAMEPLATE : RESERVED_WHOLE_NAME;

    return Forms.some((Form) => RESERVED_ANYWHERE.some((Word) => Form.includes(Word)) || Whole.includes(Form));
}

function LongestRun(Text: string){
    const Lower = Text.toLowerCase();
    let Longest = 0;
    let Run = 0;

    for(let Index = 0; Index < Lower.length; Index++){
        Run = Index > 0 && Lower[Index] === Lower[Index - 1] ? Run + 1 : 1;
        Longest = Math.max(Longest, Run);
    }

    return Longest;
}

type CheckedName = { Name: string, NameKey: string, Nameplate: string, NameplateKey: string | null };

// The rules on the name alone, without the database
function CheckNameRules(Name: unknown): GuildReply | undefined {
    if(typeof Name !== "string" || !NAME_RULE.test(Name)){
        return Refuse(400, GUILD_CODE.NameInvalidLength);
    }

    if((Name.match(/[0-9]/g) ?? []).length > MAX_DIGITS){
        return Refuse(400, GUILD_CODE.NameTooManyNumbers);
    }

    if(LongestRun(Name) > MAX_RUN){
        return Refuse(400, GUILD_CODE.NameTooManyLetters);
    }

    if(IsProfaneGuildText(Name)){
        return Refuse(400, GUILD_CODE.NameProfane);
    }

    if(IsReservedGuildText(Name, false)){
        return Refuse(409, GUILD_CODE.NameTaken);
    }

    return undefined;
}

// An empty nameplate is allowed (the client skips it when empty)
function PlateOf(Nameplate: unknown){
    return Nameplate === undefined || Nameplate === null ? "" : Nameplate;
}

// The rules on the nameplate alone, without the database
function CheckNameplateRules(Nameplate: unknown): GuildReply | undefined {
    const Plate = PlateOf(Nameplate);

    if(typeof Plate !== "string" || (Plate.length > 0 && !NAMEPLATE_RULE.test(Plate))){
        return Refuse(400, GUILD_CODE.NameplateInvalidLength);
    }

    if(Plate.length === 0){
        return undefined;
    }

    if(IsProfaneGuildText(Plate)){
        return Refuse(400, GUILD_CODE.NameplateProfane);
    }

    if(IsReservedGuildText(Plate, true)){
        return Refuse(409, GUILD_CODE.NameplateTaken);
    }

    return undefined;
}

// The name and nameplate rules, in the order the first failure decides. The caller checks
// "already in a guild" first.
function CheckNameAndNameplate(tx: Tx, Name: unknown, Nameplate: unknown): CheckedName | GuildReply {
    const NameRefusal = CheckNameRules(Name);

    if(NameRefusal !== undefined){
        return NameRefusal;
    }

    const NameKey = (Name as string).toLowerCase();

    if(tx.select({ guildId: guilds.guildId }).from(guilds).where(eq(guilds.nameKey, NameKey)).get() !== undefined){
        return Refuse(409, GUILD_CODE.NameTaken);
    }

    const PlateRefusal = CheckNameplateRules(Nameplate);

    if(PlateRefusal !== undefined){
        return PlateRefusal;
    }

    const Plate = PlateOf(Nameplate) as string;

    if(Plate.length === 0){
        return { Name: Name as string, NameKey: NameKey, Nameplate: "", NameplateKey: null };
    }

    const NameplateKey = Plate.toLowerCase();

    if(tx.select({ guildId: guilds.guildId }).from(guilds).where(eq(guilds.nameplateKey, NameplateKey)).get() !== undefined){
        return Refuse(409, GUILD_CODE.NameplateTaken);
    }

    return { Name: Name as string, NameKey: NameKey, Nameplate: Plate, NameplateKey: NameplateKey };
}

// The ticket key of a typed name and nameplate (lowercase; an empty nameplate is "")
function TicketKeys(Name: unknown, Nameplate: unknown){
    const Plate = PlateOf(Nameplate);

    return typeof Name === "string" && typeof Plate === "string" ? { NameKey: Name.toLowerCase(), NameplateKey: Plate.toLowerCase() } : undefined;
}

function IsReply(Value: CheckedName | GuildReply): Value is GuildReply {
    return (Value as GuildReply).Status !== undefined;
}

// ---- Reading ----

const RANK_ORDER: Record<string, number> = { Leader: 0, Officer: 1, Member: 2 };

function NormalizeRank(Rank: unknown): GuildRank | undefined {
    if(typeof Rank !== "string"){
        return undefined;
    }

    switch(Rank.toLowerCase()){
        case "member": return "Member";
        case "officer": return "Officer";
        case "leader": return "Leader";
        default: return undefined;
    }
}

function MembershipOf(tx: Tx, AccountId: string){
    return tx.select().from(guildmembers).where(eq(guildmembers.accountId, AccountId)).get();
}

function MemberCount(tx: Tx, GuildId: string){
    return tx.select({ n: sql<number>`count(*)` }).from(guildmembers).where(eq(guildmembers.guildId, GuildId)).get()?.n ?? 0;
}

function AccountExists(tx: Tx, AccountId: string){
    return tx.select({ userId: users.userId }).from(users).where(eq(users.userId, AccountId)).get() !== undefined;
}

function GuildDataInTx(tx: Tx, GuildId: string): GuildData | undefined {
    const Row = tx.select().from(guilds).where(eq(guilds.guildId, GuildId)).get();

    if(Row === undefined){
        return undefined;
    }

    const Members = tx.select().from(guildmembers).where(eq(guildmembers.guildId, GuildId)).all()
        .sort((A, B) => (RANK_ORDER[A.rank] ?? 3) - (RANK_ORDER[B.rank] ?? 3) || A.joinedAt - B.joinedAt || A.accountId.localeCompare(B.accountId));

    return {
        id: Row.guildId,
        name: Row.name,
        nameplate: Row.nameplate,
        leader_account_id: Row.leaderId,
        members: Members.map((Member) => ({ phx_account_id: Member.accountId, rank: Member.rank })),
        maximum_guild_members: Math.max(MaxGuildMembers(), Members.length)
    };
}

function LiveInvite(tx: Tx, GuildId: string, InviteeId: string, Now: number){
    return tx.select().from(guildinvites).where(and(eq(guildinvites.guildId, GuildId), eq(guildinvites.inviteeId, InviteeId), gt(guildinvites.expiresAt, Now))).get();
}

// GET /guild: the caller's guild, or 204 (no body) when there is none. A 204 fails the client's parse,
// which is how it learns "no guild" (it then clears its guild), as with the old stub.
export function GetOwnGuild(UserId: string | undefined): GuildReply {
    if(UserId === undefined){
        return { Status: 204, Body: undefined };
    }

    const Data = GetDb().transaction((tx) => {
        const Membership = MembershipOf(tx, UserId);

        return Membership === undefined ? undefined : GuildDataInTx(tx, Membership.guildId);
    });

    return Data === undefined ? { Status: 204, Body: undefined } : WithPayload(Data);
}

// The inviter still leads or officers the invite's guild (an invite dies with that right)
function InviterStillMayInvite(tx: Tx, InviterId: string, GuildId: string){
    const Inviter = MembershipOf(tx, InviterId);

    return Inviter !== undefined && Inviter.guildId === GuildId && (Inviter.rank === "Leader" || Inviter.rank === "Officer");
}

// Invites sent by this account for this guild go (it was demoted to Member, kicked or left)
function DropInvitesSentBy(tx: Tx, InviterId: string, GuildId: string){
    return tx.delete(guildinvites).where(and(eq(guildinvites.guildId, GuildId), eq(guildinvites.inviterId, InviterId))).returning({ inviteId: guildinvites.inviteId }).all().length;
}

// GET /guild/invite/player: the caller's open invites, newest first. Invites between players who
// blocked each other, and invites whose inviter lost the right to invite, are left out (they are
// removed when that happens; this is the safety net).
export function ListOwnGuildInvites(UserId: string | undefined): GuildReply {
    Sweep();

    const Now = Clock();
    const Invites: GuildInviteData[] = UserId === undefined ? [] : GetDb().transaction((tx) => tx
        .select({ inviteId: guildinvites.inviteId, guildId: guildinvites.guildId, guildName: guilds.name, inviterId: guildinvites.inviterId })
        .from(guildinvites)
        .innerJoin(guilds, eq(guilds.guildId, guildinvites.guildId))
        .where(and(eq(guildinvites.inviteeId, UserId), gt(guildinvites.expiresAt, Now)))
        .orderBy(desc(guildinvites.createdAt), asc(guildinvites.inviteId))
        .all()
        .filter((Row) => !IsBlockedEitherWayInTx(tx, UserId, Row.inviterId) && InviterStillMayInvite(tx, Row.inviterId, Row.guildId))
        .map((Row) => ({ id: Row.inviteId, guild_id: Row.guildId, guild_name: Row.guildName, inviter_account_id: Row.inviterId })));

    return WithPayload({ invites: Invites });
}

// ---- Creating ----

// POST /guild/validate {leader_account_id, name, nameplate}: the create widget checks the name while
// the player types. The token's account decides; a different leader in the body is only logged.
export function ValidateGuildCreate(UserId: string, BodyLeader: unknown, Name: unknown, Nameplate: unknown): GuildReply {
    Sweep();

    if(typeof BodyLeader === "string" && BodyLeader.length > 0 && BodyLeader !== UserId){
        logger.info(`guild: validate by ${UserId} names leader ${IsAccountIdShape(BodyLeader) ? BodyLeader : "<not an account id>"}; checking the caller`);
    }

    const Now = Clock();
    const Result = GetDb().transaction((tx): CheckedName | GuildReply => {
        if(MembershipOf(tx, UserId) !== undefined){
            return Refuse(409, GUILD_CODE.AlreadyInAGuild);
        }

        return CheckNameAndNameplate(tx, Name, Nameplate);
    });

    if(IsReply(Result)){
        logger.info(`guild: validate by ${UserId} name=${Quoted(Name)} tag=${Quoted(Nameplate ?? "")} -> ${Describe(Result)}`);
        return Result;
    }

    const Keys = { NameKey: Result.NameKey, NameplateKey: Result.NameplateKey ?? "" };
    const Kept = LiveTickets(UserId, Now).filter((Ticket) => Ticket.NameKey !== Keys.NameKey || Ticket.NameplateKey !== Keys.NameplateKey);

    Tickets.set(UserId, [...Kept, { ...Keys, At: Now }].slice(-MAX_TICKETS_PER_ACCOUNT));
    logger.info(`guild: validate by ${UserId} name=${Quoted(Name)} tag=${Quoted(Nameplate ?? "")} -> ok`);

    return Ack();
}

// How a create is tied to the leader's own action: "ticket" when the leader validated this very name
// and nameplate with their own token in the last 15 minutes; with GUILD_CREATE_ACTIVITY_FALLBACK=1 also
// "fallback" for any recent validate or activity; otherwise undefined.
function CreateBinding(Leader: string, Name: unknown, Nameplate: unknown, Now: number): "ticket" | "fallback" | undefined {
    const Keys = TicketKeys(Name, Nameplate);
    const Live = LiveTickets(Leader, Now);

    if(Keys !== undefined && Live.some((Ticket) => Ticket.NameKey === Keys.NameKey && Ticket.NameplateKey === Keys.NameplateKey)){
        return "ticket";
    }

    if(ActivityFallbackOn() && (Live.length > 0 || SeenWithinMs(Leader, CREATE_ACTIVITY_MS))){
        return "fallback";
    }

    return undefined;
}

// POST /guild from the game server (the client's Create button is the RPC ServerCreateGuild on the
// Ramsgate server, which sends this with the game-server key and the leader id the client put in the
// RPC). ForwardedUserId is whose token came along, if any: the game server's own login's (exe
// 0x140ac78a4 takes the token of the server's local user), never the leader's, so it is only logged.
//
// Refusals that are server policy (no validate of this name, the creation pause) answer code ""
// (the client shows "Unable to create guild."); the precise reason is in the log line.
export function CreateGuild(Leader: unknown, ForwardedUserId: string | undefined, Name: unknown, Nameplate: unknown): GuildReply {
    Sweep();

    const Now = Clock();
    const LeaderShown = IsAccountIdShape(Leader) ? Leader : "<not an account id>";
    const LogRefusal = (Reply: GuildReply, Why: string) => {
        logger.info(`guild: create for ${LeaderShown} name=${Quoted(Name)} tag=${Quoted(Nameplate ?? "")} refused ${Describe(Reply)}: ${Why}`);
        return Reply;
    };

    if(ForwardedUserId !== undefined && ForwardedUserId !== Leader){
        logger.info(`guild: create for ${LeaderShown}: the game server's token names ${ForwardedUserId} (its own login, not the leader's); not used`);
    }

    if(!IsAccountIdShape(Leader) || !GetDb().transaction((tx) => AccountExists(tx, Leader))){
        return LogRefusal(Refuse(400, GUILD_CODE.Unknown, "No such account."), "no such account");
    }

    const Binding = CreateBinding(Leader, Name, Nameplate, Now);

    if(Binding === undefined){
        // A name the rules refuse anyway gets that rule's own text; otherwise the plain failure
        const RuleRefusal = CheckNameRules(Name) ?? CheckNameplateRules(Nameplate);
        const Others = LiveTickets(Leader, Now).length;
        const Why = `no validate of this name and nameplate by the leader in the last 15 minutes${Others > 0 ? ` (${Others} other validated name(s))` : ""}`;

        return LogRefusal(RuleRefusal ?? Refuse(403, GUILD_CODE.Unknown, "Check the name in the create window, then try again."), Why);
    }

    if(Binding === "fallback"){
        logger.warn(`guild: create for ${Leader} name=${Quoted(Name)} accepted without a validate of this name (GUILD_CREATE_ACTIVITY_FALLBACK=1)`);
    }

    let Result: GuildData | GuildReply;

    try{
        Result = GetDb().transaction((tx): GuildData | GuildReply => {
            if(MembershipOf(tx, Leader) !== undefined){
                return Refuse(409, GUILD_CODE.AlreadyInAGuild);
            }

            const Checked = CheckNameAndNameplate(tx, Name, Nameplate);

            if(IsReply(Checked)){
                return Checked;
            }

            const Previous = LastCreated.get(Leader);

            if(Previous !== undefined && Now - Previous < CREATE_INTERVAL_MS){
                return Refuse(429, GUILD_CODE.Unknown, "A guild was created a moment ago; try again later.");
            }

            const GuildId = crypto.randomUUID();

            tx.insert(guilds).values({ guildId: GuildId, name: Checked.Name, nameKey: Checked.NameKey, nameplate: Checked.Nameplate, nameplateKey: Checked.NameplateKey, leaderId: Leader, createdAt: Now, updatedAt: Now }).run();
            tx.insert(guildmembers).values({ accountId: Leader, guildId: GuildId, rank: "Leader", joinedAt: Now, updatedAt: Now }).run();
            tx.delete(guildinvites).where(eq(guildinvites.inviteeId, Leader)).run();

            return GuildDataInTx(tx, GuildId)!;
        });
    }
    catch(error: any){
        // A unique index refusing the insert (a name or nameplate taken in between); anything else is a real error
        if(typeof error?.code !== "string" || !error.code.startsWith("SQLITE_CONSTRAINT")){
            throw error;
        }

        logger.warn(`guild: create for ${Leader} refused by the database: ${error.code}`);
        return Refuse(409, GUILD_CODE.NameTaken);
    }

    if((Result as GuildReply).Status !== undefined){
        return LogRefusal(Result as GuildReply, "checks");
    }

    const Data = Result as GuildData;

    LastCreated.set(Leader, Now);
    Tickets.delete(Leader);
    logger.info(`guild: created G=${Data.id} name=${Data.name} tag=${Data.nameplate} leader=${Leader} (${Binding === "ticket" ? "validated name" : "activity fallback"}${ForwardedUserId !== undefined ? `, a token of ${ForwardedUserId} came along` : ", no token"})`);

    return WithPayload(Data);
}

// ---- Membership ----

// DELETE /guild/:guildId: the leader disbands their own guild
export function DisbandGuild(UserId: string, GuildId: unknown): GuildReply {
    const Result = GetDb().transaction((tx): GuildReply => {
        const Membership = MembershipOf(tx, UserId);

        if(Membership === undefined || Membership.guildId !== GuildId){
            return Refuse(404, GUILD_CODE.NotInAGuild);
        }

        if(Membership.rank !== "Leader"){
            return Refuse(403, GUILD_CODE.InvalidPermission);
        }

        RemoveGuildInTx(tx, Membership.guildId);
        return Ack();
    });

    logger.info(`guild: disband G=${typeof GuildId === "string" ? GuildId.slice(0, 64) : "<none>"} by=${UserId} -> ${Describe(Result)}`);

    return Result;
}

function RemoveGuildInTx(tx: Tx, GuildId: string){
    const Members = tx.delete(guildmembers).where(eq(guildmembers.guildId, GuildId)).returning({ accountId: guildmembers.accountId }).all();

    tx.delete(guildinvites).where(eq(guildinvites.guildId, GuildId)).run();
    tx.delete(guilds).where(eq(guilds.guildId, GuildId)).run();

    return Members.length;
}

function RecentInvitesBy(InviterId: string, Now: number){
    return (InvitesSent.get(InviterId) ?? []).filter((At) => Now - At < 60 * 60 * 1000);
}

// PUT /guild/invite/:accountId: a Leader or Officer invites a player to their guild
export function InviteToGuild(UserId: string, TargetId: unknown): GuildReply {
    Sweep();

    const Now = Clock();
    let Why = "";
    const Result = GetDb().transaction((tx): GuildReply => {
        const Membership = MembershipOf(tx, UserId);

        if(Membership === undefined){
            return Refuse(404, GUILD_CODE.NotInAGuild);
        }

        if(Membership.rank !== "Leader" && Membership.rank !== "Officer"){
            return Refuse(403, GUILD_CODE.InvalidPermission);
        }

        if(!IsAccountIdShape(TargetId) || !AccountExists(tx, TargetId)){
            return Refuse(404, GUILD_CODE.Unknown, "No such account.");
        }

        const Target = MembershipOf(tx, TargetId);

        if(TargetId === UserId || Target?.guildId === Membership.guildId){
            return Refuse(409, GUILD_CODE.TargetAlreadyInYourGuild);
        }

        // The same answer as the friends and party routes (403), with a message that does not say why
        if(IsBlockedEitherWayInTx(tx, UserId, TargetId)){
            Why = "blocked";
            return Refuse(403, GUILD_CODE.Unknown, "The invite could not be sent.");
        }

        if(MemberCount(tx, Membership.guildId) >= MaxGuildMembers()){
            return Refuse(409, GUILD_CODE.GuildIsFull);
        }

        if(LiveInvite(tx, Membership.guildId, TargetId, Now) !== undefined){
            return Refuse(409, GUILD_CODE.TargetAlreadyHasGuildInvite);
        }

        const DeclinedAt = Declined.get(`${Membership.guildId}|${TargetId}`);

        if(DeclinedAt !== undefined && Now - DeclinedAt < DECLINE_COOLDOWN_MS){
            Why = "declined this guild's invite in the last 24 hours";
            return Refuse(429, GUILD_CODE.Unknown, "That player turned down an invite from this guild recently; try again later.");
        }

        const OpenForGuild = tx.select({ n: sql<number>`count(*)` }).from(guildinvites)
            .where(and(eq(guildinvites.guildId, Membership.guildId), gt(guildinvites.expiresAt, Now))).get()?.n ?? 0;

        if(OpenForGuild >= MAX_OPEN_INVITES_PER_GUILD){
            return Refuse(429, GUILD_CODE.Unknown, "Too many open invites for this guild.");
        }

        if(RecentInvitesBy(UserId, Now).length >= MAX_INVITES_PER_INVITER_PER_HOUR){
            return Refuse(429, GUILD_CODE.Unknown, "Too many guild invites sent in the last hour; try again later.");
        }

        // An expired invite for the same pair gives way; the invitee keeps at most the newest 20
        tx.delete(guildinvites).where(and(eq(guildinvites.guildId, Membership.guildId), eq(guildinvites.inviteeId, TargetId))).run();

        const Pending = tx.select({ inviteId: guildinvites.inviteId }).from(guildinvites)
            .where(and(eq(guildinvites.inviteeId, TargetId), gt(guildinvites.expiresAt, Now)))
            .orderBy(asc(guildinvites.createdAt), asc(guildinvites.inviteId)).all();

        for(const Old of Pending.slice(0, Math.max(0, Pending.length - (MAX_OPEN_INVITES_PER_INVITEE - 1)))){
            tx.delete(guildinvites).where(eq(guildinvites.inviteId, Old.inviteId)).run();
        }

        tx.insert(guildinvites).values({ inviteId: crypto.randomUUID(), guildId: Membership.guildId, inviteeId: TargetId, inviterId: UserId, createdAt: Now, expiresAt: Now + GuildInviteTtlMs() }).run();

        return Ack();
    });

    if(Result.Status === 200){
        InvitesSent.set(UserId, [...RecentInvitesBy(UserId, Now), Now]);
    }

    logger.info(`guild: invite by=${UserId} to=${IsAccountIdShape(TargetId) ? TargetId : "<not an account id>"} -> ${Describe(Result)}${Why !== "" ? `: ${Why}` : ""}`);

    return Result;
}

// POST /guild/invite/accept/:guild_invite_id: the invitee joins as a Member; all their other invites go
export function AcceptGuildInvite(UserId: string, InviteId: unknown): GuildReply {
    const Now = Clock();
    let GuildId = "";
    let Why = "";

    const Result = GetDb().transaction((tx): GuildReply => {
        const Invite = typeof InviteId === "string" ? tx.select().from(guildinvites).where(eq(guildinvites.inviteId, InviteId)).get() : undefined;

        if(Invite === undefined || Invite.inviteeId !== UserId || Invite.expiresAt <= Now){
            return Refuse(404, GUILD_CODE.InviteNotFound);
        }

        GuildId = Invite.guildId;

        if(MembershipOf(tx, UserId) !== undefined){
            return Refuse(409, GUILD_CODE.AlreadyInAGuild, "You need to leave your guild before accepting another guild invite.");
        }

        // Gone with its guild, a block between the two, or an inviter no longer Leader or Officer there
        const Stale = tx.select({ guildId: guilds.guildId }).from(guilds).where(eq(guilds.guildId, Invite.guildId)).get() === undefined ? "the guild is gone"
            : IsBlockedEitherWayInTx(tx, UserId, Invite.inviterId) ? "blocked"
            : !InviterStillMayInvite(tx, Invite.inviterId, Invite.guildId) ? "the inviter may no longer invite"
            : undefined;

        if(Stale !== undefined){
            Why = Stale;
            tx.delete(guildinvites).where(eq(guildinvites.inviteId, Invite.inviteId)).run();
            return Refuse(404, GUILD_CODE.InviteNotFound);
        }

        if(MemberCount(tx, Invite.guildId) >= MaxGuildMembers()){
            return Refuse(409, GUILD_CODE.GuildIsFull);
        }

        tx.insert(guildmembers).values({ accountId: UserId, guildId: Invite.guildId, rank: "Member", joinedAt: Now, updatedAt: Now }).run();
        tx.delete(guildinvites).where(eq(guildinvites.inviteeId, UserId)).run();

        return Ack();
    });

    logger.info(`guild: accept by=${UserId}${GuildId !== "" ? ` G=${GuildId}` : ""} -> ${Describe(Result)}${Why !== "" ? `: ${Why}` : ""}`);

    return Result;
}

// DELETE /guild/invite/:guild_invite_id: the invitee declines (an expired invite of theirs is simply
// removed). A declined guild cannot invite the same player again for DECLINE_COOLDOWN_MS.
export function DeclineGuildInvite(UserId: string, InviteId: unknown): GuildReply {
    const Now = Clock();
    const Result = GetDb().transaction((tx): GuildReply => {
        const Invite = typeof InviteId === "string" ? tx.select().from(guildinvites).where(eq(guildinvites.inviteId, InviteId)).get() : undefined;

        if(Invite === undefined || Invite.inviteeId !== UserId){
            return Refuse(404, GUILD_CODE.InviteNotFound);
        }

        tx.delete(guildinvites).where(eq(guildinvites.inviteId, Invite.inviteId)).run();

        if(Invite.expiresAt > Now){
            Declined.set(`${Invite.guildId}|${UserId}`, Now);
        }

        return Ack();
    });

    logger.info(`guild: decline by=${UserId} -> ${Describe(Result)}`);

    return Result;
}

function LeaveInTx(tx: Tx, UserId: string, Membership: { rank: string, guildId: string }): GuildReply {
    if(Membership.rank === "Leader"){
        return Refuse(409, GUILD_CODE.LeaderCannotLeave);
    }

    tx.delete(guildmembers).where(eq(guildmembers.accountId, UserId)).run();
    DropInvitesSentBy(tx, UserId, Membership.guildId);
    return Ack();
}

// DELETE /guild/player: a Member or Officer leaves (the leader disbands, or hands over first)
export function LeaveGuild(UserId: string): GuildReply {
    const Result = GetDb().transaction((tx): GuildReply => {
        const Membership = MembershipOf(tx, UserId);

        return Membership === undefined ? Refuse(404, GUILD_CODE.NotInAGuild) : LeaveInTx(tx, UserId, Membership);
    });

    logger.info(`guild: leave by=${UserId} -> ${Describe(Result)}`);

    return Result;
}

// DELETE /guild/player/:accountId: the leader removes a member (naming oneself is a leave)
export function KickGuildMember(UserId: string, TargetId: unknown): GuildReply {
    const Result = GetDb().transaction((tx): GuildReply => {
        const Membership = MembershipOf(tx, UserId);

        if(Membership === undefined){
            return Refuse(404, GUILD_CODE.NotInAGuild);
        }

        if(TargetId === UserId){
            return LeaveInTx(tx, UserId, Membership);
        }

        if(Membership.rank !== "Leader"){
            return Refuse(403, GUILD_CODE.InvalidPermission);
        }

        const Target = IsAccountIdShape(TargetId) ? MembershipOf(tx, TargetId) : undefined;

        if(Target === undefined || Target.guildId !== Membership.guildId){
            return Refuse(404, GUILD_CODE.NotInAGuild);
        }

        tx.delete(guildmembers).where(eq(guildmembers.accountId, Target.accountId)).run();
        DropInvitesSentBy(tx, Target.accountId, Membership.guildId);
        return Ack();
    });

    logger.info(`guild: kick by=${UserId} target=${IsAccountIdShape(TargetId) ? TargetId : "<not an account id>"} -> ${Describe(Result)}`);

    return Result;
}

// PUT /guild/rank/:accountId/:rank (member, officer or leader, any case): leader only. Making someone
// leader hands the guild over; the old leader becomes an Officer.
export function ChangeGuildRank(UserId: string, TargetId: unknown, RankValue: unknown): GuildReply {
    const Now = Clock();
    const Result = GetDb().transaction((tx): GuildReply => {
        const Membership = MembershipOf(tx, UserId);

        if(Membership === undefined){
            return Refuse(404, GUILD_CODE.NotInAGuild);
        }

        if(Membership.rank !== "Leader"){
            return Refuse(403, GUILD_CODE.InvalidPermission);
        }

        const Rank = NormalizeRank(RankValue);

        if(Rank === undefined){
            return Refuse(400, GUILD_CODE.InvalidRank);
        }

        const Target = IsAccountIdShape(TargetId) ? MembershipOf(tx, TargetId) : undefined;

        if(Target === undefined || Target.guildId !== Membership.guildId){
            return Refuse(404, GUILD_CODE.NotInAGuild);
        }

        if(Target.accountId === UserId){
            return Refuse(403, GUILD_CODE.InvalidPermission);
        }

        if(Target.rank === Rank){
            return Ack();
        }

        tx.update(guildmembers).set({ rank: Rank, updatedAt: Now }).where(eq(guildmembers.accountId, Target.accountId)).run();

        // A Member may not invite: the invites they sent as an Officer go
        if(Rank === "Member"){
            DropInvitesSentBy(tx, Target.accountId, Membership.guildId);
        }

        if(Rank === "Leader"){
            tx.update(guildmembers).set({ rank: "Officer", updatedAt: Now }).where(eq(guildmembers.accountId, UserId)).run();
            tx.update(guilds).set({ leaderId: Target.accountId, updatedAt: Now }).where(eq(guilds.guildId, Membership.guildId)).run();
        }

        return Ack();
    });

    logger.info(`guild: rank by=${UserId} target=${IsAccountIdShape(TargetId) ? TargetId : "<not an account id>"} rank=${Quoted(RankValue)} -> ${Describe(Result)}`);

    return Result;
}

// ---- Host fallbacks (routes/undauntedapi.ts) ----

export type GuildSummary = { guildId: string, name: string, nameplate: string, leader: string, members: number };

export function GuildNameOf(UserId: string): string | undefined {
    return GetDb().transaction((tx) => {
        const Membership = MembershipOf(tx, UserId);

        return Membership === undefined ? undefined : tx.select({ name: guilds.name }).from(guilds).where(eq(guilds.guildId, Membership.guildId)).get()?.name;
    });
}

// Every guild, by name
// Chat (src/realtime/muc.ts): whether this account may use the room Guild-<GuildId>
export function IsGuildMember(AccountId: string, GuildId: string): boolean {
    return GetDb().select({ guildId: guildmembers.guildId }).from(guildmembers)
        .where(and(eq(guildmembers.accountId, AccountId), eq(guildmembers.guildId, GuildId))).get() !== undefined;
}

export function ListGuilds(): GuildSummary[] {
    return GetDb().transaction((tx) => tx.select().from(guilds).orderBy(asc(guilds.nameKey)).all().map((Row) => ({
        guildId: Row.guildId,
        name: Row.name,
        nameplate: Row.nameplate,
        leader: Row.leaderId,
        members: MemberCount(tx, Row.guildId)
    })));
}

// An admin removes a guild named by id or by name (any case): its members, invites and the guild
export function DisbandGuildAsAdmin(Guild: unknown): { Name: string, Members: number } | undefined {
    if(typeof Guild !== "string" || Guild.length === 0 || Guild.length > 64){
        return undefined;
    }

    const Result = GetDb().transaction((tx) => {
        const Row = tx.select().from(guilds).where(eq(guilds.guildId, Guild)).get()
            ?? tx.select().from(guilds).where(eq(guilds.nameKey, Guild.toLowerCase())).get();

        return Row === undefined ? undefined : { Name: Row.name, Members: RemoveGuildInTx(tx, Row.guildId), GuildId: Row.guildId };
    });

    if(Result !== undefined){
        logger.info(`guild: G=${Result.GuildId} name=${Result.Name} disbanded by an admin (${Result.Members} member(s))`);
    }

    return Result === undefined ? undefined : { Name: Result.Name, Members: Result.Members };
}
