import { RemoveTestDb } from "./setup";
import "./authenv";
import { GUILD_API_PORT, GUILD_DEPLOYSERVER_PORT } from "./guildenv";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import type { Server } from "node:http";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { gameserverapikeys, guildinvites, guildmembers, guilds, userapikeys, users } from "../src/db/schema";
import { HashUserAPIKey, SignMetagameJWTForUid } from "../src/controllers/auth";
import { GuildNow, ResetGuildMemoryForTests, SetGuildClockForTests } from "../src/controllers/guild";
import { FakeDeployServer, StartFakeDeployServer } from "./fakedeploy";
import { GuildOutcome, ParseGuildData, ParseGuildInvites, ParsePhoenixEnvelope, SocialClient, Transport } from "./socialclient";

// Guilds over HTTP (roadmap 3.11): the v2 routes with the 1.4.4 client's own bodies and headers, and
// the create as the Ramsgate game server sends it. Every reply is checked twice: the exact JSON, and
// what the client makes of it (socialclient.ts: success or the EGuildRequestError it shows).
// GUILD_MAX_MEMBERS is 3 in this file. The app listens on a spare loopback port.

const BASE = `http://127.0.0.1:${GUILD_API_PORT}`;
const GS_KEY = crypto.randomBytes(24).toString("hex");
const A = "UID-guild-a", B = "UID-guild-b", C = "UID-guild-c", D = "UID-guild-d", E = "UID-guild-e", F = "UID-guild-f";
const NAMES: Record<string, string> = { [A]: "Alpha", [B]: "Bravo", [C]: "Charlie", [D]: "Delta", [E]: "Echo", [F]: "Foxtrot" };
const Tokens: Record<string, string> = {};

type Reply = { status: number, text: string, json: any };
type CallOptions = { as?: string, token?: string, gs?: boolean, key?: string, raw?: string, body?: unknown, emptyJson?: boolean, headers?: Record<string, string> };

async function Call(Method: string, Path: string, Options: CallOptions = {}): Promise<Reply> {
    // Keep requests independent of idle pooled sockets while slow CI creates test accounts.
    // Otherwise Undici can reuse a connection as the server's keep-alive timeout closes it.
    // Do not retry mutations: that could replay an invite and hide a real assertion failure.
    const Headers: Record<string, string> = { connection: "close", ...(Options.headers ?? {}) };

    if(Options.as !== undefined) Headers["authorization"] = `bearer ${Tokens[Options.as]}`;
    if(Options.token !== undefined) Headers["authorization"] = `bearer ${Options.token}`;
    if(Options.gs) Headers["x-undaunted-gameserver-apikey"] = GS_KEY;
    if(Options.key !== undefined) Headers["x-undaunted-user-api-key"] = Options.key;
    if(Options.body !== undefined || Options.emptyJson || Options.raw !== undefined) Headers["content-type"] = "application/json; charset=utf-8";

    const Response = await fetch(BASE + Path, { method: Method, headers: Headers, body: Options.raw ?? (Options.body === undefined ? (Options.emptyJson ? "" : undefined) : JSON.stringify(Options.body)) });
    const Text = await Response.text();
    let Json: any;

    try{ Json = Text.length > 0 ? JSON.parse(Text) : undefined; } catch { Json = undefined; }

    return { status: Response.status, text: Text, json: Json };
}

// The client's calls. Empty-bodied calls go out with and without the JSON content type.
const Validate = (Who: string, Name: unknown, Nameplate: unknown = "") => Call("POST", "/guild/validate", { as: Who, body: { leader_account_id: Who, name: Name, nameplate: Nameplate } });
const Create = (Leader: string, Name: unknown, Nameplate: unknown = "", Options: CallOptions = {}) => Call("POST", "/guild", { gs: true, body: { leader_account_id: Leader, name: Name, nameplate: Nameplate }, ...Options });
const GetGuild = (Who: string) => Call("GET", "/guild", { as: Who });
const GetInvites = (Who: string) => Call("GET", "/guild/invite/player", { as: Who });
const Invite = (Who: string, Target: string, Json = false) => Call("PUT", `/guild/invite/${Target}`, { as: Who, emptyJson: Json });
const AcceptInvite = (Who: string, InviteId: string, Json = true) => Call("POST", `/guild/invite/accept/${InviteId}`, { as: Who, emptyJson: Json });
const DeclineInvite = (Who: string, InviteId: string) => Call("DELETE", `/guild/invite/${InviteId}`, { as: Who });
const Leave = (Who: string) => Call("DELETE", "/guild/player", { as: Who });
const Kick = (Who: string, Target: string) => Call("DELETE", `/guild/player/${Target}`, { as: Who });
const Rank = (Who: string, Target: string, NewRank: string) => Call("PUT", `/guild/rank/${Target}/${NewRank}`, { as: Who, emptyJson: true });
const Disband = (Who: string, GuildId: string) => Call("DELETE", `/guild/${GuildId}`, { as: Who });
const PartyPoll = (Who: string) => Call("POST", "/party", { as: Who, body: { buildId: "239827_rel-1.4.4_shipping", featureOverrides: [] } });

const ACK = { code: "OK", message: "", payload: {} };

function ExpectAck(Reply: Reply, What: string){
    assert.equal(Reply.status, 200, `${What}: ${Reply.text}`);
    assert.deepEqual(Reply.json, ACK, What);
    assert.equal(GuildOutcome(Reply.status, Reply.text).ok, true, `${What}: the client reads a success`);
}

// A refusal: the status, the envelope with the code and an empty payload, and the client's reading of it
function ExpectRefusal(Reply: Reply, Status: number, ClientError: string, What: string){
    assert.equal(Reply.status, Status, `${What}: ${Reply.text}`);
    assert.deepEqual(Object.keys(Reply.json ?? {}), ["code", "message", "payload"], What);
    assert.equal(typeof Reply.json.message, "string");
    assert.deepEqual(Reply.json.payload, {});
    const Outcome = GuildOutcome(Reply.status, Reply.text);
    assert.deepEqual([Outcome.ok, Outcome.error], [false, ClientError], What);
}

// A guild reply: wrapped, with flat copies of the payload at the root; the client parses it
function ExpectGuild(Reply: Reply, Expected: object, What: string){
    assert.equal(Reply.status, 200, `${What}: ${Reply.text}`);
    assert.deepEqual(Reply.json, { code: "OK", message: "", payload: Expected, ...Expected }, What);
    const Envelope = ParsePhoenixEnvelope(Reply.status, Reply.text);
    assert.ok(Envelope.ok, What);
    return ParseGuildData(Envelope.payload)!;
}

function GuildObject(Id: string, Name: string, Nameplate: string, Leader: string, Members: [string, string][]){
    return { id: Id, name: Name, nameplate: Nameplate, leader_account_id: Leader, members: Members.map(([Account, Rank]) => ({ phx_account_id: Account, rank: Rank })), maximum_guild_members: 3 };
}

async function OwnInvites(Who: string){
    const Reply = await GetInvites(Who);
    assert.equal(Reply.status, 200);
    assert.deepEqual(Reply.json.invites, Reply.json.payload.invites, "flat copy");
    return ParseGuildInvites(ParsePhoenixEnvelope(Reply.status, Reply.text).payload)!;
}

function ClientOf(Who: string){
    const Send: Transport = async (Method, Path, Body) => {
        const Reply = await Call(Method, Path, { as: Who, body: Body });
        return { status: Reply.status, text: Reply.text };
    };

    return new SocialClient(Who, Send);
}

function AddAccount(Id: string, Name: string){
    GetDb().insert(users).values({ userId: Id, name: Name, notes: 0 }).run();
    Tokens[Id] = SignMetagameJWTForUid(Id);
}

let Offset = 0;
let Fake: FakeDeployServer | undefined;
let Listening: Server | undefined;
let Slayers = "";

before(async () => {
    for(const [UserId, Name] of Object.entries(NAMES)){
        AddAccount(UserId, Name);
    }

    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY, "utf8").digest("hex") }).run();
    SetGuildClockForTests(() => Date.now() + Offset);

    Fake = await StartFakeDeployServer(GUILD_DEPLOYSERVER_PORT, []);
    Listening = await new Promise<Server>((Resolve, Reject) => {
        const Started = app.listen(GUILD_API_PORT, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
    });
});

after(async () => {
    SetGuildClockForTests();
    Listening?.closeAllConnections();
    await new Promise<void>((Resolve) => Listening ? Listening.close(() => Resolve()) : Resolve());
    await Fake?.Close();
    RemoveTestDb(() => GetDb().$client.close());
});

describe("1. no guild", () => {
    it("GET /guild is 204 with no body (the client clears its guild); GET /guild/invite/player is the wrapped empty list", async () => {
        const Mine = await GetGuild(A);
        assert.deepEqual([Mine.status, Mine.text], [204, ""]);
        assert.equal(ParsePhoenixEnvelope(Mine.status, Mine.text).ok, false, "not in a guild");

        const Invites = await GetInvites(A);
        assert.equal(Invites.status, 200);
        assert.equal(Invites.text, JSON.stringify({ code: "OK", message: "", payload: { invites: [] }, invites: [] }));
        assert.deepEqual(ParseGuildInvites(ParsePhoenixEnvelope(Invites.status, Invites.text).payload), []);

        assert.deepEqual([(await Call("GET", "/guild", { gs: true })).status], [204], "the game-server key alone: no guild");
        assert.deepEqual((await Call("GET", "/guild/invite/player", { gs: true })).json.invites, []);
    });
});

describe("2. POST /guild/validate", () => {
    it("every name and nameplate rule answers its code; valid pairs answer the ack and create nothing", async () => {
        const Cases: [unknown, unknown, number, string][] = [
            ["Abc", "SLY", 400, "GuildNameInvalidLength"],
            ["Abcdefghijklmnop", "", 400, "GuildNameInvalidLength"],
            ["Ab cd", "", 400, "GuildNameInvalidLength"],
            [" Slayers", "", 400, "GuildNameInvalidLength"],
            ["Slayers!", "", 400, "GuildNameInvalidLength"],
            [undefined, "", 400, "GuildNameInvalidLength"],
            [12345, "", 400, "GuildNameInvalidLength"],
            ["Guild1234567", "", 400, "GuildNameTooManyNumbers"],
            ["Gaaaaaaa", "", 400, "GuildNameTooManyLetters"],
            ["GaAaAaAa", "", 400, "GuildNameTooManyLetters"],
            ["Sh1tHeads", "", 400, "GuildNameProfane"],
            ["Slayers", "ABCDEFG", 400, "GuildNameplateInvalidLength"],
            ["Slayers", "A", 400, "GuildNameplateInvalidLength"],
            ["Slayers", "A-B", 400, "GuildNameplateInvalidLength"],
            ["Slayers", 42, 400, "GuildNameplateInvalidLength"],
            ["Slayers", "P0RN", 400, "GuildNameplateProfane"]
        ];

        for(const [Name, Nameplate, Status, ClientError] of Cases){
            ExpectRefusal(await Validate(A, Name, Nameplate), Status, ClientError, `${JSON.stringify(Name)} / ${JSON.stringify(Nameplate)}`);
        }

        for(const [Name, Nameplate] of [["Guild123456", ""], ["Gaaaaaa", "AB"], ["Slayers", "SLY"], ["Slayers", ""], ["Slayers", null], ["ABCDEFGHIJKLMNO", "ABCDEF"]] as [string, unknown][]){
            ExpectAck(await Validate(A, Name, Nameplate), `${Name} / ${JSON.stringify(Nameplate)}`);
        }

        ExpectAck(await Call("POST", "/guild/validate", { as: A, body: { name: "Slayers" } }), "no nameplate key at all");
        assert.equal((await GetGuild(A)).status, 204, "validating creates nothing");
        assert.equal(GetDb().select().from(guilds).all().length, 0);
    });

    it("GUILD_NAME_DENYLIST adds words, digit swaps included", async () => {
        process.env.GUILD_NAME_DENYLIST = "grief, b4d";

        try{
            ExpectRefusal(await Validate(A, "GriefGuild"), 400, "GuildNameProfane", "a listed word");
            ExpectRefusal(await Validate(A, "Bad1Guild"), 400, "GuildNameProfane", "b4d matches bad");
            ExpectRefusal(await Validate(A, "Fine", "GRIEF"), 400, "GuildNameplateProfane", "in the nameplate");
        }
        finally{
            delete process.env.GUILD_NAME_DENYLIST;
        }

        ExpectAck(await Validate(A, "GriefGuild"), "without the list");
    });

    it("staff and project words are \"already in use\"; short offensive tags are profane; GUILD_RESERVED_NAMES=0 frees only the reserved words", async () => {
        const Reserved: [string, string, number, string][] = [
            ["ServerAdmins", "", 409, "GuildNameTaken"],
            ["Moderators", "", 409, "GuildNameTaken"],
            ["TheOfficials", "", 409, "GuildNameTaken"],
            ["St4ffOnly", "", 409, "GuildNameTaken"],
            ["TheGameMasters", "", 409, "GuildNameTaken"],
            ["Dauntless", "", 409, "GuildNameTaken"],
            ["Phoenix", "", 409, "GuildNameTaken"],
            ["Slayers", "GM", 409, "GuildNameplateTaken"],
            ["Slayers", "dev", 409, "GuildNameplateTaken"],
            ["Slayers", "ADMIN", 409, "GuildNameplateTaken"],
            ["Slayers", "M0D", 409, "GuildNameplateTaken"],
            ["Slayers", "KKK", 400, "GuildNameplateProfane"],
            ["Slayers", "FAG", 400, "GuildNameplateProfane"],
            ["Slayers", "SS", 400, "GuildNameplateProfane"],
            ["Slayers", "1488", 400, "GuildNameplateProfane"],
            ["KkkRiders", "", 400, "GuildNameProfane"]
        ];

        for(const [Name, Nameplate, Status, ClientError] of Reserved){
            ExpectRefusal(await Validate(A, Name, Nameplate), Status, ClientError, `${Name} / ${Nameplate}`);
        }

        for(const [Name, Nameplate] of [["DauntlessCrew", ""], ["PhoenixRising", ""], ["Slayers", "NIGHT"], ["Slayers", "GMX"], ["Slayers", "DEVS1"]]){
            ExpectAck(await Validate(A, Name, Nameplate), `${Name} / ${Nameplate}: only whole words are reserved there`);
        }

        process.env.GUILD_RESERVED_NAMES = "0";

        try{
            ExpectAck(await Validate(A, "Moderators"), "reserved words allowed");
            ExpectAck(await Validate(A, "Slayers", "GM"), "reserved nameplates allowed");
            ExpectRefusal(await Validate(A, "Slayers", "KKK"), 400, "GuildNameplateProfane", "offensive tags stay refused");
        }
        finally{
            delete process.env.GUILD_RESERVED_NAMES;
        }
    });

    it("needs a player's token", async () => {
        assert.equal((await Call("POST", "/guild/validate", { body: { leader_account_id: A, name: "Slayers", nameplate: "" } })).status, 401);
        assert.equal((await Call("POST", "/guild/validate", { gs: true, body: { leader_account_id: A, name: "Slayers", nameplate: "" } })).status, 403);
    });
});

describe("3. the game server's create (POST /guild)", () => {
    it("refuses a player's token, and a leader who did not validate this name: 403 with no code, which the client shows as \"Unable to create guild.\"", async () => {
        const PlayerOnly = await Call("POST", "/guild", { as: D, body: { leader_account_id: D, name: "Deltas", nameplate: "" } });
        ExpectRefusal(PlayerOnly, 403, "Unknown", "a player's token alone");

        ExpectRefusal(await Create(D, "Deltas"), 403, "Unknown", "D never validated");
        await PartyPoll(D);
        ExpectRefusal(await Create(D, "Deltas"), 403, "Unknown", "being online (a party poll a moment ago) is not enough");
        ExpectRefusal(await Create(D, "x"), 400, "GuildNameInvalidLength", "a name the rules refuse keeps its own text");
        ExpectRefusal(await Create(D, "Deltas", "GM"), 409, "GuildNameplateTaken", "so does a reserved nameplate");
        assert.equal((await GetGuild(D)).status, 204);
    });

    it("after A's validate, creates the guild and answers it in full; A's GET /guild answers the same", async () => {
        ExpectAck(await Validate(A, "Slayers", "SLY"), "validate");

        const Created = await Create(A, "Slayers", "SLY");
        Slayers = Created.json?.payload?.id;
        assert.match(Slayers, /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);

        const Expected = GuildObject(Slayers, "Slayers", "SLY", A, [[A, "Leader"]]);
        const Parsed = ExpectGuild(Created, Expected, "create");
        assert.deepEqual(Object.keys(Created.json.payload), ["id", "name", "nameplate", "leader_account_id", "members", "maximum_guild_members"]);
        assert.equal(typeof Created.json.payload.maximum_guild_members, "number");
        assert.deepEqual(Parsed, { id: Slayers, name: "Slayers", nameplate: "SLY", leader: A, members: [{ id: A, rank: "Leader" }], max: 3 });

        const Mine = await GetGuild(A);
        assert.equal(Mine.text, Created.text);
    });

    it("A cannot validate or create a second guild, and names and nameplates are taken regardless of case", async () => {
        await PartyPoll(A);
        ExpectRefusal(await Validate(A, "Another"), 409, "YouAlreadyInAGuild", "validate while in a guild");
        ExpectRefusal(await Validate(A, "x"), 409, "YouAlreadyInAGuild", "checked before the name rules");
        ExpectRefusal(await Create(A, "Another"), 403, "Unknown", "no validate of that name (a member of a guild gets none)");

        ExpectRefusal(await Validate(B, "sLAYERS"), 409, "GuildNameTaken", "name");
        ExpectRefusal(await Validate(B, "Others", "sly"), 409, "GuildNameplateTaken", "nameplate");
        ExpectAck(await Validate(B, "Others", ""), "an empty nameplate is never taken");
    });

    it("the create must be what the leader validated: another name, another nameplate or another leader is refused; the last five pairs count, in any case", async () => {
        ExpectAck(await Validate(B, "Bravos", "BRV"), "B validates");
        ExpectRefusal(await Create(B, "Other", "BRV"), 403, "Unknown", "another name for B");
        ExpectRefusal(await Create(B, "Bravos", "XYZ"), 403, "Unknown", "another nameplate");
        ExpectRefusal(await Create(B, "Bravos", ""), 403, "Unknown", "no nameplate");
        ExpectRefusal(await Create(C, "Bravos", "BRV"), 403, "Unknown", "B's name for C, who validated nothing");
        assert.equal((await GetGuild(C)).status, 204);

        for(const Name of ["BravoOne", "BravoTwo", "BravoThree", "BravoFour", "BravoFive", "BravoSix"]){
            ExpectAck(await Validate(B, Name), Name);
        }

        ExpectRefusal(await Create(B, "BravoOne"), 403, "Unknown", "only the last five validated pairs are kept");
        ExpectRefusal(await Create(B, "Bravos", "BRV"), 403, "Unknown", "older still");

        const Created = await Create(B, "bravotwo");
        assert.equal(Created.status, 200, Created.text);
        assert.equal(Created.json.payload.name, "bravotwo", "the name as the create sends it");
        ExpectAck(await Disband(B, Created.json.payload.id), "B disbands");
        ExpectRefusal(await Create(B, "BravoThree"), 403, "Unknown", "a create uses up the validated names");
    });

    it("an unknown leader is 400; a token that comes along is only logged (it is the game server's own); a bad one never causes a 500", async () => {
        ExpectRefusal(await Create("UID-nobody", "Nobodies"), 400, "Unknown", "no such account");
        ExpectRefusal(await Create("not an id!", "Nobodies"), 400, "Unknown", "not an account id");

        ExpectAck(await Validate(C, "Charlies", "CHR"), "C validates");
        const WithOther = await Create(C, "Charlies", "CHR", { as: A });
        assert.equal(WithOther.status, 200, WithOther.text);
        assert.equal(WithOther.json.payload.leader_account_id, C, "A's token changes nothing");
        ExpectAck(await Disband(C, WithOther.json.payload.id), "C disbands");

        ExpectAck(await Validate(E, "Echoing"), "E validates");
        const BadToken = await Create(E, "Echoing", "", { token: "not.a.token" });
        assert.equal(BadToken.status, 200, BadToken.text);
        ExpectAck(await Disband(E, BadToken.json.payload.id), "E disbands");

        const Leaders = GetDb().select().from(guildmembers).all();
        assert.deepEqual(Leaders.map((Row) => [Row.accountId, Row.rank]), [[A, "Leader"]]);
    });

    it("one guild per leader per 10 minutes (429), even with a validated name", async () => {
        ExpectAck(await Validate(B, "BravosTwo"), "validate");
        ExpectRefusal(await Create(B, "BravosTwo"), 429, "Unknown", "a second guild within 10 minutes");

        Offset += 10 * 60 * 1000 + 1;
        const Later = await Create(B, "BravosTwo");
        assert.equal(Later.status, 200, Later.text);
        ExpectAck(await Disband(B, Later.json.payload.id), "disband");
    });

    it("GUILD_CREATE_ACTIVITY_FALLBACK=1 also accepts a leader heard from in the last minute; nobody else", async () => {
        process.env.GUILD_CREATE_ACTIVITY_FALLBACK = "1";

        try{
            await PartyPoll(D);
            const Created = await Create(D, "Deltas");
            assert.equal(Created.status, 200, Created.text);
            ExpectAck(await Disband(D, Created.json.payload.id), "disband");

            ExpectRefusal(await Create(F, "Foxtrots"), 403, "Unknown", "F never validated and was never heard from");
        }
        finally{
            delete process.env.GUILD_CREATE_ACTIVITY_FALLBACK;
        }

        await PartyPoll(D);
        ExpectRefusal(await Create(D, "Deltas"), 403, "Unknown", "off again");
    });
});

describe("4. invites", () => {
    it("A invites B; B's list shows it with the guild's name and the inviter, whom B's client sets up by id", async () => {
        ExpectAck(await Invite(A, B), "invite without a content type");

        const Raw = await GetInvites(B);
        const Invites = await OwnInvites(B);
        assert.equal(Invites.length, 1);
        assert.deepEqual(Object.keys(Raw.json.payload.invites[0]), ["id", "guild_id", "guild_name", "inviter_account_id"]);
        assert.deepEqual({ ...Invites[0], id: "" }, { id: "", guildId: Slayers, guildName: "Slayers", inviter: A });

        const ClientB = await ClientOf(B).Login();
        assert.equal(await ClientB.ResolveFromPhoenix(Invites[0].inviter), "initialised");
        assert.equal(ClientB.NameOf(A), "Alpha");

        assert.deepEqual(await OwnInvites(A), [], "the inviter sees nothing");
    });

    it("refuses a second invite (Redundant), oneself (Cloned), an unknown account, and a block either way", async () => {
        ExpectRefusal(await Invite(A, B, true), 409, "TargetAlreadyHasGuildInvite", "again");
        ExpectRefusal(await Invite(A, A), 409, "TargetAlreadyInYourGuild", "oneself");
        ExpectRefusal(await Invite(A, "UID-nobody"), 404, "Unknown", "unknown account");
        ExpectRefusal(await Invite(C, D), 404, "NotInAGuild", "C has no guild");

        assert.equal((await Call("POST", `/friends/api/public/blocklist/${D}/${A}`, { as: D })).status, 204);
        ExpectRefusal(await Invite(A, D), 403, "Unknown", "D blocked A");
        assert.equal((await Call("DELETE", `/friends/api/public/blocklist/${D}/${A}`, { as: D })).status, 204);
    });
});

describe("5. accept and decline", () => {
    it("B accepts A's invite: B is a Member, and every other invite B had is gone", async () => {
        ExpectAck(await Validate(E, "Echoes", "ECH"), "E validates");
        assert.equal((await Create(E, "Echoes", "ECH")).status, 200);
        ExpectAck(await Invite(E, B), "E invites B");

        const Both = await OwnInvites(B);
        assert.deepEqual(Both.map((Entry) => Entry.guildName), ["Echoes", "Slayers"], "newest first");

        ExpectAck(await AcceptInvite(B, Both[1].id), "accept, empty JSON body");
        assert.deepEqual(await OwnInvites(B), []);
        ExpectGuild(await GetGuild(B), GuildObject(Slayers, "Slayers", "SLY", A, [[A, "Leader"], [B, "Member"]]), "B's guild");
        ExpectRefusal(await AcceptInvite(B, Both[0].id), 404, "GuildInviteNotFound", "E's invite went with the accept");
    });

    it("C declines; nobody accepts or declines another player's invite; a member of a guild cannot accept another", async () => {
        ExpectAck(await Invite(E, C), "E invites C");
        const ForC = (await OwnInvites(C))[0];
        ExpectRefusal(await AcceptInvite(D, ForC.id, false), 404, "GuildInviteNotFound", "D accepting C's invite");
        ExpectRefusal(await DeclineInvite(D, ForC.id), 404, "GuildInviteNotFound", "D declining C's invite");
        ExpectAck(await DeclineInvite(C, ForC.id), "decline, no body");
        assert.deepEqual(await OwnInvites(C), []);
        ExpectRefusal(await DeclineInvite(C, ForC.id), 404, "GuildInviteNotFound", "declined twice");

        ExpectAck(await Invite(E, B), "a player in another guild may be invited");
        const ForB = (await OwnInvites(B))[0];
        ExpectRefusal(await AcceptInvite(B, ForB.id), 409, "YouAlreadyInAGuild", "B is in Slayers");
        ExpectAck(await DeclineInvite(B, ForB.id), "B declines");
        ExpectRefusal(await AcceptInvite(C, "not-an-invite"), 404, "GuildInviteNotFound", "a made-up id");
    });

    it("an invite expires after GUILD_INVITE_TTL_DAYS (7): it leaves the list and cannot be accepted", async () => {
        ExpectAck(await Invite(A, C), "A invites C");
        const Old = (await OwnInvites(C))[0];

        Offset += 7 * 24 * 60 * 60 * 1000 + 60 * 1000;
        assert.deepEqual(await OwnInvites(C), []);
        ExpectRefusal(await AcceptInvite(C, Old.id), 404, "GuildInviteNotFound", "expired");
        ExpectAck(await Invite(A, C), "a new invite replaces the expired one");
    });

    it("the guild is full at GUILD_MAX_MEMBERS (3): accepting and inviting answer 409 GuildIsFull", async () => {
        ExpectAck(await Invite(A, D), "A invites D while there is room");
        ExpectAck(await AcceptInvite(C, (await OwnInvites(C))[0].id), "C joins: three members");

        ExpectRefusal(await AcceptInvite(D, (await OwnInvites(D))[0].id), 409, "GuildIsFull", "D's accept");
        ExpectRefusal(await Invite(A, F), 409, "GuildIsFull", "a new invite");

        const Full = ExpectGuild(await GetGuild(A), GuildObject(Slayers, "Slayers", "SLY", A, [[A, "Leader"], [B, "Member"], [C, "Member"]]), "full");
        assert.equal(Full.max - Full.members.length, 0, "the client shows 0 open positions");
    });
});

describe("6. ranks", () => {
    it("the leader promotes and demotes (rank in any case); Members cannot invite, Officers can", async () => {
        ExpectRefusal(await Invite(C, F), 403, "InvalidPermission", "a Member invites");

        ExpectAck(await Rank(A, B, "officer"), "Promote To Guild Officer");
        ExpectRefusal(await Invite(B, F), 409, "GuildIsFull", "an Officer may invite (the guild is full)");
        ExpectAck(await Rank(A, C, "OFFICER"), "upper case");
        ExpectGuild(await GetGuild(C), GuildObject(Slayers, "Slayers", "SLY", A, [[A, "Leader"], [B, "Officer"], [C, "Officer"]]), "two officers");
        ExpectAck(await Rank(A, C, "Member"), "Demote To Guild Member");
        ExpectAck(await Rank(A, B, "officer"), "the same rank again changes nothing");

        ExpectRefusal(await Rank(A, C, "captain"), 400, "InvalidGuildRank", "no such rank");
        ExpectRefusal(await Rank(B, C, "officer"), 403, "InvalidPermission", "an Officer changes ranks");
        ExpectRefusal(await Rank(A, A, "member"), 403, "InvalidPermission", "the leader's own rank");
        ExpectRefusal(await Rank(A, D, "officer"), 404, "NotInAGuild", "not a member");
        ExpectRefusal(await Rank(D, A, "member"), 404, "NotInAGuild", "the caller has no guild");
    });

    it("Promote To Guild Leader hands the guild over; the old leader becomes an Officer", async () => {
        ExpectAck(await Rank(A, B, "leader"), "hand-over");
        const After = ExpectGuild(await GetGuild(A), GuildObject(Slayers, "Slayers", "SLY", B, [[B, "Leader"], [A, "Officer"], [C, "Member"]]), "after");
        assert.equal(After.leader, B);
        assert.equal(GetDb().select().from(guilds).all().find((Row) => Row.guildId === Slayers)?.leaderId, B);
        ExpectRefusal(await Rank(A, C, "officer"), 403, "InvalidPermission", "the old leader no longer changes ranks");
    });

    it("everything is read back from the database after the in-memory state is cleared (a restart)", async () => {
        const Before = (await GetGuild(C)).text;
        ResetGuildMemoryForTests();
        assert.equal((await GetGuild(C)).text, Before);
        assert.equal((await OwnInvites(D)).length, 1, "D's invite survived too");
    });
});

describe("7. kick, leave, disband", () => {
    it("the leader cannot leave; an Officer cannot kick; a Member leaves through DELETE /guild/player (not the disband route)", async () => {
        ExpectRefusal(await Leave(B), 409, "GuildLeaderCannotLeaveGuild", "the leader leaves");
        ExpectRefusal(await Kick(B, B), 409, "GuildLeaderCannotLeaveGuild", "the leader kicks themself");
        ExpectRefusal(await Kick(A, C), 403, "InvalidPermission", "an Officer kicks");

        ExpectAck(await Leave(C), "Leave Guild");
        assert.equal((await GetGuild(C)).status, 204);
        ExpectGuild(await GetGuild(B), GuildObject(Slayers, "Slayers", "SLY", B, [[B, "Leader"], [A, "Officer"]]), "the guild is still there");
        ExpectRefusal(await Leave(C), 404, "NotInAGuild", "leaving again");
    });

    it("the leader kicks; a kick of someone outside the guild is 404; a member naming themself leaves", async () => {
        ExpectAck(await Kick(B, A), "Kick From Guild");
        assert.equal((await GetGuild(A)).status, 204);
        ExpectRefusal(await Kick(B, D), 404, "NotInAGuild", "not a member");
        ExpectRefusal(await Kick(C, B), 404, "NotInAGuild", "the caller has no guild");

        ExpectAck(await Invite(B, A), "B invites A back");
        ExpectAck(await AcceptInvite(A, (await OwnInvites(A))[0].id), "A accepts");
        ExpectAck(await Kick(A, A), "a Member naming themself leaves");
        assert.equal((await GetGuild(A)).status, 204);
    });

    it("only the leader disbands, only their own guild; afterwards nobody is in it, its invites are gone and the name is free", async () => {
        ExpectAck(await Invite(B, A), "B invites A");
        ExpectAck(await AcceptInvite(A, (await OwnInvites(A))[0].id), "A accepts");
        ExpectAck(await Invite(B, C), "B invites C (left open)");

        ExpectRefusal(await Disband(A, Slayers), 403, "InvalidPermission", "a Member disbands");
        ExpectRefusal(await Disband(B, crypto.randomUUID()), 404, "NotInAGuild", "a wrong id");
        ExpectRefusal(await Disband(C, Slayers), 404, "NotInAGuild", "not a member");

        ExpectAck(await Disband(B, Slayers), "DISBAND GUILD");

        for(const Who of [A, B]){
            assert.equal((await GetGuild(Who)).status, 204, Who);
        }

        assert.deepEqual(await OwnInvites(C), []);
        assert.deepEqual(await OwnInvites(D), []);
        assert.equal(GetDb().select().from(guildinvites).all().filter((Row) => Row.guildId === Slayers).length, 0);
        ExpectAck(await Validate(A, "slayers", "SLY"), "the name and nameplate are free again");
    });
});

describe("8. limits on invites", () => {
    it("30 invites per inviter per hour, then at most 50 open invites per guild (429)", async () => {
        ExpectAck(await Validate(F, "Foxes", "FOX"), "F validates");
        assert.equal((await Create(F, "Foxes", "FOX")).status, 200);

        const Many: string[] = [];

        for(let Index = 0; Index < 52; Index++){
            const Id = `UID-guild-many-${String(Index).padStart(2, "0")}`;
            AddAccount(Id, `Many${Index}`);
            Many.push(Id);
        }

        for(let Index = 0; Index < 30; Index++){
            ExpectAck(await Invite(F, Many[Index]), `invite ${Index + 1}`);
        }

        ExpectRefusal(await Invite(F, Many[30]), 429, "Unknown", "the 31st within an hour");

        Offset += 60 * 60 * 1000 + 1;
        for(let Index = 30; Index < 50; Index++){
            ExpectAck(await Invite(F, Many[Index]), `invite ${Index + 1}, an hour later`);
        }

        ExpectRefusal(await Invite(F, Many[50]), 429, "Unknown", "the 51st open invite");
        ExpectAck(await DeclineInvite(Many[0], (await OwnInvites(Many[0]))[0].id), "one declines");
        ExpectAck(await Invite(F, Many[50]), "room again");
    });

    it("a player keeps at most 20 open invites; the oldest gives way", async () => {
        const Target = "UID-guild-target";
        AddAccount(Target, "Target");

        const Leaders: string[] = [];

        for(let Index = 0; Index < 21; Index++){
            const Leader = `UID-guild-lead-${String(Index).padStart(2, "0")}`;
            AddAccount(Leader, `Lead${Index}`);
            ExpectAck(await Validate(Leader, `Band${String.fromCharCode(65 + Index)}Guild`), `validate ${Index}`);
            assert.equal((await Create(Leader, `Band${String.fromCharCode(65 + Index)}Guild`)).status, 200);
            Offset += 1000;
            ExpectAck(await Invite(Leader, Target), `invite ${Index + 1}`);
            Leaders.push(Leader);
        }

        const Open = await OwnInvites(Target);
        assert.equal(Open.length, 20);
        assert.equal(Open[0].inviter, Leaders[20], "newest first");
        assert.ok(!Open.some((Entry) => Entry.inviter === Leaders[0]), "the oldest was dropped");
    });
});

describe("9. permissions", () => {
    it("no token is 401 and the game-server key alone is 403 on every client route; the reads answer only the token's own data", async () => {
        const Routes: [string, string][] = [["POST", "/guild/validate"], ["PUT", `/guild/invite/${A}`], ["POST", "/guild/invite/accept/x"], ["DELETE", "/guild/invite/x"], ["DELETE", "/guild/player"], ["DELETE", `/guild/player/${A}`], ["PUT", `/guild/rank/${A}/officer`], ["DELETE", "/guild/x"]];

        for(const [Method, Path] of Routes){
            assert.equal((await Call(Method, Path, { emptyJson: true })).status, 401, `${Method} ${Path}`);
            assert.equal((await Call(Method, Path, { gs: true, emptyJson: true })).status, 403, `${Method} ${Path}`);
        }

        assert.equal((await Call("GET", "/guild")).status, 401);
        assert.equal((await Call("GET", "/guild/invite/player")).status, 401);
        assert.equal((await Call("GET", "/guild", { token: "not.a.token" })).status, 401);

        const Foxes = await GetGuild(F);
        assert.equal(Foxes.json.payload.leader_account_id, F);
        assert.equal((await GetGuild(E)).json.payload.name, "Echoes", "each token its own guild");
        assert.equal((await Call("GET", "/guild", { gs: true, as: F })).text, Foxes.text, "a game server forwarding F's token reads F's guild");
    });
});

describe("10. GUILDS=0", () => {
    it("brings back the old stubs, and every other guild route 404s", async () => {
        process.env.GUILDS = "0";

        try{
            const Mine = await GetGuild(F);
            assert.deepEqual([Mine.status, Mine.text], [204, ""]);
            assert.equal((await GetInvites(A)).text, JSON.stringify({ code: null, message: "OK", payload: { invites: [] } }));

            for(const [Method, Path, Options] of [
                ["POST", "/guild/validate", { as: A, body: { leader_account_id: A, name: "Slayers", nameplate: "" } }],
                ["POST", "/guild", { gs: true, body: { leader_account_id: A, name: "Slayers", nameplate: "" } }],
                ["PUT", `/guild/invite/${B}`, { as: F }],
                ["POST", "/guild/invite/accept/x", { as: A }],
                ["DELETE", "/guild/invite/x", { as: A }],
                ["DELETE", "/guild/player", { as: F }],
                ["DELETE", `/guild/player/${A}`, { as: F }],
                ["PUT", `/guild/rank/${A}/officer`, { as: F }],
                ["DELETE", "/guild/x", { as: F }]
            ] as [string, string, CallOptions][]){
                const Reply = await Call(Method, Path, Options);
                assert.deepEqual([Reply.status, Reply.text], [404, ""], `${Method} ${Path}`);
            }
        }
        finally{
            delete process.env.GUILDS;
        }

        assert.equal((await GetGuild(F)).status, 200, "and back on");
        assert.equal(GetDb().select().from(guilds).all().some((Row) => Row.name === "Foxes"), true, "nothing was lost while off");
    });
});

describe("11. host fallbacks (/undaunted/api)", () => {
    const Keys: Record<string, string> = {};
    const ADMIN = "UID-guild-admin";

    before(() => {
        // Past the invite lifetime: Foxes' 50 open invites from section 8 have expired
        Offset += 8 * 24 * 60 * 60 * 1000;
        GetDb().insert(users).values({ userId: ADMIN, name: "Boss", notes: 0, isAdmin: true }).run();

        for(const Who of [F, D, ADMIN]){
            Keys[Who] = `UUK_${crypto.randomBytes(24).toString("hex")}`;
            GetDb().insert(userapikeys).values({ userId: Who, keyHash: HashUserAPIKey(Keys[Who]) }).run();
        }
    });

    it("GuildInvite: the key's owner invites by name; an admin may name the inviter, directly only", async () => {
        const ByKey = await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[F], body: { Username: "delta" } });
        assert.deepEqual([ByKey.status, ByKey.json], [200, { From: "Foxtrot", To: "Delta", Guild: "Foxes" }]);
        assert.deepEqual((await OwnInvites(D)).map((Entry) => [Entry.guildName, Entry.inviter]), [["Foxes", F]]);

        const Again = await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[F], body: { Username: "Delta" } });
        assert.equal(Again.status, 409);
        assert.equal(Again.json.error, "guild_refused");
        assert.match(Again.json.message, /^RedundantAdorableQuillshot: /);

        const NoGuild = await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[D], body: { Username: "Foxtrot" } });
        assert.deepEqual([NoGuild.status, NoGuild.json.error], [404, "guild_refused"], "D has no guild");

        assert.equal((await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[D], body: { Username: "Echo", From: "Foxtrot" } })).status, 403, "not an admin");
        const ForF = await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[ADMIN], body: { Username: "Echo", From: F } });
        assert.deepEqual([ForF.status, ForF.json], [200, { From: "Foxtrot", To: "Echo", Guild: "Foxes" }]);
        assert.equal((await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[ADMIN], body: { Username: "Echo", From: F }, headers: { "x-forwarded-for": "203.0.113.9" } })).status, 403, "admin through a proxy");

        assert.equal((await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[F], body: { Username: "Nobody" } })).status, 404);
        assert.equal((await Call("POST", "/undaunted/api/GuildInvite", { body: { Username: "Delta" } })).status, 401);
        assert.deepEqual((await Call("POST", "/undaunted/api/GuildInvite", { key: Keys[F], raw: "{nope" })).json, { error: "bad_request", message: "The request body is not valid JSON." });
    });

    it("Guilds and DisbandGuild: admin only; disband by name or id removes members and invites", async () => {
        assert.equal((await Call("GET", "/undaunted/api/Guilds", { key: Keys[F] })).status, 403);
        const List = await Call("GET", "/undaunted/api/Guilds", { key: Keys[ADMIN] });
        assert.equal(List.status, 200);
        const Foxes = List.json.find((Entry: any) => Entry.name === "Foxes");
        assert.deepEqual(Foxes, { guildId: Foxes.guildId, name: "Foxes", nameplate: "FOX", leader: F, members: 1 });

        assert.equal((await Call("POST", "/undaunted/api/DisbandGuild", { key: Keys[F], body: { Guild: "Foxes" } })).status, 403);
        assert.deepEqual((await Call("POST", "/undaunted/api/DisbandGuild", { key: Keys[ADMIN], body: { Guild: "NoSuchGuild" } })).json.error, "not_found");

        const Done = await Call("POST", "/undaunted/api/DisbandGuild", { key: Keys[ADMIN], body: { Guild: "fOXES" } });
        assert.deepEqual([Done.status, Done.json], [200, { Guild: "Foxes", Members: 1 }]);
        assert.equal((await GetGuild(F)).status, 204);
        assert.deepEqual(await OwnInvites(D), [], "its invites went with it");

        const Echoes = (await Call("GET", "/undaunted/api/Guilds", { key: Keys[ADMIN] })).json.find((Entry: any) => Entry.name === "Echoes");
        assert.deepEqual((await Call("POST", "/undaunted/api/DisbandGuild", { key: Keys[ADMIN], body: { Guild: Echoes.guildId } })).json, { Guild: "Echoes", Members: 1 });
    });

    it("GUILDS=0 turns them off too", async () => {
        process.env.GUILDS = "0";

        try{
            for(const [Method, Path, Body] of [["POST", "/undaunted/api/GuildInvite", { Username: "Delta" }], ["POST", "/undaunted/api/DisbandGuild", { Guild: "x" }], ["GET", "/undaunted/api/Guilds", undefined]] as [string, string, unknown][]){
                const Reply = await Call(Method, Path, { key: Keys[ADMIN], body: Body });
                assert.deepEqual([Reply.status, Reply.json.error], [404, "guilds_off"], Path);
            }
        }
        finally{
            delete process.env.GUILDS;
        }
    });
});

describe("12. blocks, declines and officers", () => {
    const L = "UID-guild-l1", O = "UID-guild-o1", P = "UID-guild-p1", Q = "UID-guild-p2", R = "UID-guild-p3";
    let Lancers = "";

    before(async () => {
        for(const [Id, Name] of [[L, "Lancer"], [O, "Oscar"], [P, "Papa"], [Q, "Quebec"], [R, "Romeo"]]){
            AddAccount(Id, Name);
        }

        ExpectAck(await Validate(L, "Lancers", "LNC"), "L validates");
        const Created = await Create(L, "Lancers", "LNC");
        assert.equal(Created.status, 200, Created.text);
        Lancers = Created.json.payload.id;
    });

    it("a block removes the guild invites between the two; an older invite from a blocked inviter is never listed and cannot be accepted", async () => {
        ExpectAck(await Invite(L, P), "L invites P");
        assert.equal((await OwnInvites(P)).length, 1);

        assert.equal((await Call("POST", `/friends/api/public/blocklist/${P}/${L}`, { as: P })).status, 204);
        assert.deepEqual(await OwnInvites(P), [], "gone with the block");
        assert.equal(GetDb().select().from(guildinvites).all().filter((Row) => Row.inviteeId === P).length, 0, "deleted, not only hidden");

        const Blocked = await Invite(L, P);
        ExpectRefusal(Blocked, 403, "Unknown", "no new invite while blocked");
        assert.equal(Blocked.json.message, "The invite could not be sent.", "the reply does not say why");

        // An invite written before the block existed (as after an update): hidden, and accepting it is 404
        const Old = crypto.randomUUID();
        GetDb().insert(guildinvites).values({ inviteId: Old, guildId: Lancers, inviteeId: P, inviterId: L, createdAt: GuildNow(), expiresAt: GuildNow() + 60 * 60 * 1000 }).run();
        assert.deepEqual(await OwnInvites(P), []);
        ExpectRefusal(await AcceptInvite(P, Old), 404, "GuildInviteNotFound", "blocked");
        assert.equal(GetDb().select().from(guildinvites).all().some((Row) => Row.inviteId === Old), false, "and removed");

        assert.equal((await Call("DELETE", `/friends/api/public/blocklist/${P}/${L}`, { as: P })).status, 204);
        ExpectAck(await Invite(L, P), "unblocked: invites work again");
    });

    it("a guild whose invite was declined cannot invite that player again for 24 hours", async () => {
        ExpectAck(await DeclineInvite(P, (await OwnInvites(P))[0].id), "P declines");
        ExpectRefusal(await Invite(L, P), 429, "Unknown", "at once");
        Offset += 23 * 60 * 60 * 1000;
        ExpectRefusal(await Invite(L, P), 429, "Unknown", "23 hours later");
        Offset += 60 * 60 * 1000 + 1;
        ExpectAck(await Invite(L, P), "a day later");
    });

    it("an Officer's invites go when the Officer is demoted, kicked or leaves; an invite whose inviter lost the right cannot be accepted", async () => {
        ExpectAck(await Invite(L, O), "L invites O");
        ExpectAck(await AcceptInvite(O, (await OwnInvites(O))[0].id), "O joins");
        ExpectAck(await Rank(L, O, "officer"), "O is an Officer");

        ExpectAck(await Invite(O, Q), "the Officer invites Q");
        assert.equal((await OwnInvites(Q)).length, 1);
        ExpectAck(await Rank(L, O, "member"), "demoted");
        assert.deepEqual(await OwnInvites(Q), [], "demoted: the invite is gone");

        ExpectAck(await Rank(L, O, "officer"), "an Officer again");
        ExpectAck(await Invite(O, Q), "invites Q again");
        ExpectAck(await Kick(L, O), "kicked");
        assert.deepEqual(await OwnInvites(Q), [], "kicked: gone");

        ExpectAck(await Invite(L, O), "back in");
        ExpectAck(await AcceptInvite(O, (await OwnInvites(O))[0].id), "O joins again");
        ExpectAck(await Rank(L, O, "officer"), "Officer");
        ExpectAck(await Invite(O, Q), "invites Q a third time");
        ExpectAck(await Leave(O), "O leaves");
        assert.deepEqual(await OwnInvites(Q), [], "left: gone");
        assert.equal((await OwnInvites(P)).length, 1, "the leader's own invite to P stays");

        // A row whose inviter is only a Member (as if written before this rule): hidden, and 404 on accept
        ExpectAck(await Invite(L, O), "O comes back as a Member");
        ExpectAck(await AcceptInvite(O, (await OwnInvites(O))[0].id), "joins");
        const Stale = crypto.randomUUID();
        GetDb().insert(guildinvites).values({ inviteId: Stale, guildId: Lancers, inviteeId: R, inviterId: O, createdAt: GuildNow(), expiresAt: GuildNow() + 60 * 60 * 1000 }).run();
        assert.deepEqual(await OwnInvites(R), []);
        ExpectRefusal(await AcceptInvite(R, Stale), 404, "GuildInviteNotFound", "the inviter is only a Member");
        assert.equal((await GetGuild(R)).status, 204);
    });
});
