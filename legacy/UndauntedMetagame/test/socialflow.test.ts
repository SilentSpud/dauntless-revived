import { RemoveTestDb } from "./setup";
import "./authenv";
import { SOCIAL_API_PORT, SOCIAL_DEPLOYSERVER_PORT } from "./socialenv";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import type { Server } from "node:http";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { blocks, gameserverapikeys, users } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { ResetFriendsMemoryForTests, SetFriendsClockForTests } from "../src/controllers/friends";
import { SetPartyClockForTests } from "../src/controllers/party";
import { RecordingDeploy, StartRecordingDeploy } from "./partyenv";
import {
    AddUserInfo, ParseAccountMappings, ParseBlockList, ParseFriendsList, ParseInvitations, ParsePhoenixEnvelope, GuildErrorOf,
    SocialClient, Transport, UserInfo
} from "./socialclient";

// The social flows end to end, the way the 1.4.4 client walks them (docs/findings/social.md): every
// reply goes through the client model in socialclient.ts, which files user info and account mappings
// exactly as the exe does. Three players' clients (A, B, C) with tokens for throwaway accounts in this
// test's own database; the app listens on a spare loopback port and the deploy server is a recording
// fake on another.

const BASE = `http://127.0.0.1:${SOCIAL_API_PORT}`;
const GS_KEY = crypto.randomBytes(24).toString("hex");
const A = "UID-social-a", B = "UID-social-b", C = "UID-social-c", D = "UID-social-d";
const GONE = "UID-social-gone"; // an id on a blocklist whose account no longer exists
const NAMES: Record<string, string> = { [A]: "Alpha", [B]: "Bravo", [C]: "Charlie", [D]: "Delta" };
const Tokens: Record<string, string> = {};
const HUNT = "CR19_PlayerHunt_FTUE_Pursuit_Beta_Host";

type Reply = { status: number, text: string, json: any };
type CallOptions = { as?: string, body?: unknown, emptyJson?: boolean, headers?: Record<string, string> };

async function Call(Method: string, Path: string, Options: CallOptions = {}): Promise<Reply> {
    // Give every test request its own connection. On slower CI runners Undici can otherwise
    // race the server's keep-alive timeout and reuse a socket just as it is being closed,
    // producing an unrelated ECONNRESET in the middle of this long end-to-end suite.
    const Headers: Record<string, string> = { connection: "close", ...(Options.headers ?? {}) };

    if(Options.as !== undefined) Headers["authorization"] = `bearer ${Tokens[Options.as]}`;
    if(Options.body !== undefined || Options.emptyJson) Headers["content-type"] = "application/json; charset=utf-8";

    const Response = await fetch(BASE + Path, { method: Method, headers: Headers, body: Options.body === undefined ? (Options.emptyJson ? "" : undefined) : JSON.stringify(Options.body) });
    const Text = await Response.text();
    let Json: any;

    try{ Json = Text.length > 0 ? JSON.parse(Text) : undefined; } catch { Json = undefined; }

    return { status: Response.status, text: Text, json: Json };
}

// A player's game client talking to this server with its own token
function ClientOf(Who: string){
    const Send: Transport = async (Method, Path, Body) => {
        const Reply = await Call(Method, Path, { as: Who, body: Body });
        return { status: Reply.status, text: Reply.text };
    };

    return new SocialClient(Who, Send);
}

// The bodies the 1.4.4 client sends
const BUILD = { buildId: "239827_rel-1.4.4_shipping", featureOverrides: [] };
const Poll = (Who: string) => Call("POST", "/party", { as: Who, body: BUILD });
const FriendsList = async (Who: string) => ParseFriendsList((await Call("GET", `/friends/api/public/friends/${Who}?includePending=true`, { as: Who })).text)!;
const Summary = (Entries: { accountId: string, status: string, direction: string }[]) => Entries.map((Entry) => `${Entry.accountId} ${Entry.status} ${Entry.direction}`);

// What the server answered before these fixes, recorded literally (upstream /accountinfo/public; the
// two /account/mapping hotfixes of 22 September 2026)
const OLD_ACCOUNTINFO = (Caller: string, Name: string) => JSON.stringify({ accountId: Caller, isSubscribed: true, language: null, linkedAccounts: [{ accountId: Caller, accountType: "epic" }], username: Name });
const OLD_MAPPING_ARRAY = (Id: string, Name: string) => {
    const Entry = { accountType: "phoenix", accountId: Id, id: Id, epic: Id, phoenix: Id, srcAccountType: "epic", srcAccountId: Id, srcId: Id, dstAccountType: "phoenix", dstAccountId: Id, displayName: Name };
    return JSON.stringify({ code: "OK", message: "", payload: { accountMappings: [Entry] }, accountMappings: [Entry] });
};
const OLD_MAPPING_KEYED = (Id: string, Name: string) => JSON.stringify({ [Id]: [{ accountId: Id, displayName: Name, type: "epic", externalAuthId: Id, externalAuthIdType: "epic", externalDisplayName: Name }] });

let Deploy: RecordingDeploy;
let Listening: Server | undefined;

before(async () => {
    for(const [UserId, Name] of Object.entries(NAMES)){
        GetDb().insert(users).values({ userId: UserId, name: Name, notes: 0 }).run();
        Tokens[UserId] = SignMetagameJWTForUid(UserId);
    }

    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY, "utf8").digest("hex") }).run();

    Deploy = await StartRecordingDeploy(SOCIAL_DEPLOYSERVER_PORT);
    Listening = await new Promise<Server>((Resolve, Reject) => {
        const Started = app.listen(SOCIAL_API_PORT, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
    });
});

after(async () => {
    Listening?.closeAllConnections();
    await new Promise<void>((Resolve) => Listening ? Listening.close(() => Resolve()) : Resolve());
    await Deploy?.Close();
    RemoveTestDb(() => GetDb().$client.close());
});

describe("1. the client model reproduces what the players saw on 22 September 2026", () => {
    it("the old /account/mapping replies map nothing; the upstream 404 fails the query", () => {
        for(const Old of [OLD_MAPPING_ARRAY(B, "Bravo"), OLD_MAPPING_KEYED(B, "Bravo")]){
            const Parsed = ParseAccountMappings(200, Old);
            assert.ok(Parsed.ok);
            assert.equal(Parsed.mapped.size, 0, Old);
        }

        assert.deepEqual(ParseAccountMappings(404, ""), { ok: false, error: "Empty response payload" });
        assert.deepEqual(ParseAccountMappings(200, "<html>"), { ok: false, error: "Invalid response payload" });

        const Good = ParseAccountMappings(200, JSON.stringify({ accountMappings: { [B]: { accountId: B, accountType: "Phoenix" } } }));
        assert.ok(Good.ok);
        assert.deepEqual([...Good.mapped], [[B, { type: "Phoenix", id: B }]]);
    });

    it("the old /accountinfo/public reply files another player under the caller's id, so that player is never set up", () => {
        const Cache = new Map<string, UserInfo>();
        assert.equal(AddUserInfo(Cache, 200, OLD_ACCOUNTINFO(A, "Alpha")).result, "stored", "A's own lookup at login");
        assert.deepEqual(AddUserInfo(Cache, 200, OLD_ACCOUNTINFO(A, "Bravo")), { result: "ignored", key: A }, "B's lookup lands on A's entry");
        assert.equal(Cache.has(B), false);
        assert.equal(Cache.get(A)!.username, "Alpha");
        assert.equal(AddUserInfo(Cache, 404, "{}").result, "failed");
    });

    it("a client fed the old replies drops Add Friends and invalidates a party invite's sender", async () => {
        const OldServer: Transport = async (_Method, Path, Body: any) => {
            if(Path === "/account/mapping") return { status: 200, text: OLD_MAPPING_ARRAY(Body.ids[0], NAMES[Body.ids[0]] ?? "") };
            if(Path === "/accountinfo/public") return { status: 200, text: OLD_ACCOUNTINFO(A, NAMES[Body.accountId] ?? "") };
            return { status: 404, text: "" };
        };

        const Client = await new SocialClient(A, OldServer).Login();
        assert.equal((await Client.ResolveFromEpic(B)).outcome, "dropped", "Add Friends: no request is ever sent");
        assert.equal(await Client.ResolveFromPhoenix(B), "invalidated", "the invite never shows under PARTY INVITES");
    });

    it("the guild envelope: a 204 is a failure, and an unknown code is Unknown (\"Unable to create guild.\")", () => {
        assert.equal(ParsePhoenixEnvelope(204, "").ok, false);
        assert.equal(ParsePhoenixEnvelope(200, JSON.stringify({ code: "OK", message: "", payload: {} })).ok, true);
        assert.equal(GuildErrorOf(""), "Unknown");
        assert.equal(GuildErrorOf("SeizedAdorableQuillshot"), "GuildNameTaken");
    });
});

describe("2-3. Add Friends end to end", () => {
    it("A adds Bravo by name: name lookup, mapping, user info, the toolkit check, then the request; B sees it at login and accepts", async () => {
        const ClientA = await ClientOf(A).Login();

        const ByName = await Call("GET", "/account/api/public/account/displayName/bravo", { as: A });
        assert.deepEqual([ByName.status, ByName.json], [200, { id: B, displayName: "Bravo", externalAuths: {} }], "any case");

        const Resolved = await ClientA.ResolveFromEpic(ByName.json.id);
        assert.deepEqual(Resolved, { outcome: "resolved", phoenixId: B });
        assert.deepEqual(ClientA.Calls, ["POST /accountinfo/public", "POST /account/mapping", "POST /accountinfo/public"]);
        assert.deepEqual(ClientA.Users.get(B), { username: "Bravo", epicId: B });

        const Sent = await Call("POST", `/friends/api/public/friends/${A}/${B}`, { as: A });
        assert.deepEqual([Sent.status, Sent.text], [204, ""]);
        assert.deepEqual(Summary(await FriendsList(A)), [`${B} PENDING OUTBOUND`]);

        // B's next login: the friends list, then each listed id is resolved the same way
        const ClientB = await ClientOf(B).Login();
        const Inbound = await FriendsList(B);
        assert.deepEqual(Summary(Inbound), [`${A} PENDING INBOUND`]);
        assert.deepEqual(await ClientB.ResolveFromEpic(Inbound[0].accountId), { outcome: "resolved", phoenixId: A });
        assert.equal(ClientB.NameOf(A), "Alpha");

        // Accept Epic Friend Invite: the same POST from B's side, with an empty JSON body
        const Accepted = await Call("POST", `/friends/api/public/friends/${B}/${A}`, { as: B, emptyJson: true });
        assert.equal(Accepted.status, 204);
        assert.deepEqual(Summary(await FriendsList(A)), [`${B} ACCEPTED OUTBOUND`]);
        assert.deepEqual(Summary(await FriendsList(B)), [`${A} ACCEPTED INBOUND`]);
    });

    it("an unknown name is 404 (the Unknown toast); the player's own name is their own id (the InvitedSelf toast)", async () => {
        assert.equal((await Call("GET", "/account/api/public/account/displayName/Nobody", { as: A })).status, 404);
        assert.equal((await Call("GET", "/account/api/public/account/displayName/ALPHA", { as: A })).json.id, A);
    });
});

describe("4. blocked players", () => {
    it("resolve through the same chain; a blocked account that no longer exists is dropped without breaking the others", async () => {
        assert.equal((await Call("POST", `/friends/api/public/blocklist/${A}/${C}`, { as: A })).status, 204);
        GetDb().insert(blocks).values({ blockerId: A, blockedId: GONE, createdAt: Date.now() }).run();

        const Blocked = ParseBlockList((await Call("GET", `/friends/api/public/blocklist/${A}`, { as: A })).text)!;
        assert.deepEqual(Blocked, [C, GONE]);

        const ClientA = await ClientOf(A).Login();
        const Outcomes = await Promise.all(Blocked.map((Id) => ClientA.ResolveFromEpic(Id)));
        assert.deepEqual(Outcomes.map((Outcome) => Outcome.outcome), ["resolved", "dropped"]);
        assert.equal(ClientA.NameOf(C), "Charlie");
        assert.equal((await Call("POST", "/accountinfo/public", { as: A, body: { accountId: GONE } })).status, 404);

        assert.equal((await Call("DELETE", `/friends/api/public/blocklist/${A}/${C}`, { as: A })).status, 204);
        assert.equal((await Call("DELETE", `/friends/api/public/blocklist/${A}/${GONE}`, { as: A })).status, 204);
        assert.deepEqual(ParseBlockList((await Call("GET", `/friends/api/public/blocklist/${A}`, { as: A })).text), []);
    });
});

describe("5. a received party invite, the party, and one hunt", () => {
    let PartyB = "";

    it("B invites A and C with the client's body; each recipient's client sets up B from /accountinfo/public and shows the invite", async () => {
        PartyB = (await Poll(B)).json.partyId;
        await Poll(A);
        await Poll(C);

        for(const Recipient of [A, C]){
            const Sent = await Call("PUT", "/party/invite", { as: B, body: { recipientPlayerId: Recipient, partyId: PartyB, ...BUILD } });
            assert.deepEqual([Sent.status, Sent.json], [200, {}]);

            const Invitations = ParseInvitations((await Call("GET", "/party/invites", { as: Recipient })).text)!;
            assert.deepEqual(Invitations, [{ recipientPlayerId: Recipient, sendingPlayerId: B, partyId: PartyB, sendingPlatform: "win", sendingDisplayName: "Bravo" }]);

            const Client = await ClientOf(Recipient).Login();
            assert.equal(await Client.ResolveFromPhoenix(Invitations[0].sendingPlayerId), "initialised", "the entry shows under PARTY INVITES");
            assert.equal(Client.NameOf(B), "Bravo");
            assert.equal(Client.NameOf(Recipient), NAMES[Recipient], "the recipient's own entry is untouched");
        }
    });

    it("both accept with the client's body; every client resolves every member to that member's own name", async () => {
        for(const Who of [A, C]){
            const Accepted = await Call("PUT", `/party/invite/accept/${PartyB}`, { as: Who, body: { recipientPlayerId: Who, partyId: PartyB, ...BUILD } });
            assert.equal(Accepted.status, 200);
        }

        for(const Who of [A, B, C]){
            const Party = (await Poll(Who)).json;
            assert.deepEqual([Party.partyId, Party.leaderPlayerId, Party.playerIds], [PartyB, B, [B, A, C]]);

            const Client = await ClientOf(Who).Login();

            for(const State of Party.playerStates){
                assert.equal(await Client.ResolveFromPhoenix(State.playerId), "initialised");
                assert.equal(Client.NameOf(State.playerId), NAMES[State.playerId], `${Who} sees ${State.playerId}`);
                assert.equal(State.displayName, NAMES[State.playerId]);
            }

            assert.equal(new Set([...Client.Users.values()].map((Info) => Info.username)).size, 3, "no member filed under the caller's name");
        }
    });

    it("the leader picks a hunt: one deploy call, every member expected exactly once", async () => {
        Deploy.Calls.length = 0;

        const Joined = await Call("POST", "/candidate/join", { as: B, body: { buildId: BUILD.buildId, gameArgs: "", playerId: "", allow_crossplay: true, session_id: "SESSION_ID_LOL", regionUrlsPings: {}, gameMode: "ISLAND", isPrivate: false, partyId: PartyB, hunts: ["CR19_MatchmakerHunt_Host_Beta"], playerHuntId: HUNT } });
        assert.equal(Joined.status, 200);
        assert.equal(Deploy.Calls.length, 1);
        assert.deepEqual(Deploy.Calls[0].ExpectedPlayers, [B, A, C]);
        assert.equal(new Set(Deploy.Calls[0].ExpectedPlayers).size, Deploy.Calls[0].ExpectedPlayers.length);

        for(const Member of [A, C]){
            assert.equal((await Poll(Member)).json.candidateId, Joined.json.candidateId, `${Member} follows the leader`);
        }

        for(const Who of [A, B, C]){
            assert.equal((await Call("DELETE", "/party/member", { as: Who })).status, 200);
        }
    });

    it("with ACCOUNTINFO_PUBLIC_LEGACY=1 the same invite is lost again, exactly as on 22 September 2026", async () => {
        process.env.ACCOUNTINFO_PUBLIC_LEGACY = "1";

        try{
            const PartyD = (await Poll(D)).json.partyId;
            assert.equal((await Call("PUT", "/party/invite", { as: D, body: { recipientPlayerId: A, partyId: PartyD, ...BUILD } })).status, 200);

            const Invitations = ParseInvitations((await Call("GET", "/party/invites", { as: A })).text)!;
            assert.equal(Invitations.length, 1, "the poll delivers it ...");

            const Client = await ClientOf(A).Login();
            assert.equal(await Client.ResolveFromPhoenix(D), "invalidated", "... and the client drops it");
            assert.equal(Client.NameOf(A), "Alpha");

            assert.equal((await Call("DELETE", "/party/invite", { as: A, body: { sendingPlayerId: D, recipientPlayerId: A, partyId: PartyD } })).status, 200);
        }
        finally{
            delete process.env.ACCOUNTINFO_PUBLIC_LEGACY;
        }
    });
});

describe("6-7. user info is filed under the right id, whatever the order", () => {
    it("C looks up A, B and itself in different orders; each id holds its own name and Epic id", async () => {
        for(const Order of [[A, B, C], [C, B, A], [B, C, A]]){
            const Client = ClientOf(C);

            for(const Id of Order){
                assert.equal((await Client.QueryUserInfo(Id)).key, Id);
            }

            for(const Id of [A, B, C]){
                assert.deepEqual(Client.Users.get(Id), { username: NAMES[Id], epicId: Id }, `${Order.join(",")}: ${Id}`);
            }
        }
    });

    it("the {\"displayname\"} body answers the named account (any case); an unknown name is 404; accountId wins over a name", async () => {
        const ByName = await Call("POST", "/accountinfo/public", { as: A, body: { displayname: "bRAVO" } });
        assert.deepEqual([ByName.status, ByName.json.accountId, ByName.json.username], [200, B, "Bravo"]);
        assert.deepEqual([(await Call("POST", "/accountinfo/public", { as: A, body: { displayname: "Nobody" } })).status], [404]);
        assert.equal((await Call("POST", "/accountinfo/public", { as: A, body: { accountId: C, displayname: "Bravo" } })).json.accountId, C);
        assert.equal((await Call("POST", "/accountinfo/public", { as: A, body: {} })).status, 404);
        assert.equal((await Call("POST", "/accountinfo/public", { as: A })).status, 404, "no body at all");
    });
});

describe("8-9. mapping details and the legacy switch", () => {
    it("phoenix maps to epic, at most 100 ids, each once, unknown ids left out, nothing without a token or with ACCOUNT_MAPPING=0", async () => {
        const Reverse = await Call("POST", "/account/mapping", { as: A, body: { srcAccountType: "phoenix", ids: [B] } });
        const Parsed = ParseAccountMappings(Reverse.status, Reverse.text);
        assert.ok(Parsed.ok);
        assert.deepEqual([...Parsed.mapped], [[B, { type: "MCP", id: B }]]);

        const Many: string[] = [];

        for(let Index = 0; Index < 105; Index++){
            const Id = `UID-social-many-${String(Index).padStart(3, "0")}`;
            GetDb().insert(users).values({ userId: Id, name: `Many${Index}`, notes: 0 }).run();
            Many.push(Id);
        }

        const Capped = await Call("POST", "/account/mapping", { as: A, body: { srcAccountType: "epic", ids: [...Many, Many[0], "UID-nobody"] } });
        assert.deepEqual(Object.keys(Capped.json.accountMappings), Many.slice(0, 100));
        assert.deepEqual(Capped.json.payload.accountMappings, Capped.json.accountMappings);

        const Twice = await Call("POST", "/account/mapping", { as: A, body: { srcAccountType: "epic", ids: [B, B, "UID-nobody"] } });
        assert.deepEqual(Twice.json.accountMappings, { [B]: { accountId: B, accountType: "phoenix" } });

        assert.deepEqual((await Call("POST", "/account/mapping", { body: { srcAccountType: "epic", ids: [B] } })).json.accountMappings, {});

        process.env.ACCOUNT_MAPPING = "0";

        try{
            const Off = await Call("POST", "/account/mapping", { as: A, body: { srcAccountType: "epic", ids: [B] } });
            assert.equal(ParseAccountMappings(Off.status, Off.text).ok, true);
            assert.deepEqual(Off.json.accountMappings, {});
            assert.equal((await ClientOf(A).ResolveFromEpic(B)).outcome, "dropped", "the switch puts the old outcome back");
        }
        finally{
            delete process.env.ACCOUNT_MAPPING;
        }
    });

    it("ACCOUNTINFO_PUBLIC_LEGACY=1 restores the upstream reply byte for byte", async () => {
        process.env.ACCOUNTINFO_PUBLIC_LEGACY = "1";

        try{
            assert.equal((await Call("POST", "/accountinfo/public", { as: C, body: { accountId: A } })).text, OLD_ACCOUNTINFO(C, "Alpha"));
            assert.equal((await Call("POST", "/accountinfo/public", { as: C, body: { accountId: "UID-nobody" } })).text, OLD_ACCOUNTINFO(C, ""));
        }
        finally{
            delete process.env.ACCOUNTINFO_PUBLIC_LEGACY;
        }
    });
});

describe("10. friends limits and the block alias", () => {
    after(() => SetFriendsClockForTests());

    it("PUT on the blocklist path blocks like POST (friend requests refused), and DELETE unblocks", async () => {
        assert.equal((await Call("PUT", `/friends/api/public/blocklist/${A}/${D}`, { as: A })).status, 204);
        assert.deepEqual(ParseBlockList((await Call("GET", `/friends/api/public/blocklist/${A}`, { as: A })).text), [D]);
        assert.equal((await Call("POST", `/friends/api/public/friends/${D}/${A}`, { as: D })).status, 403);
        assert.equal((await Call("PUT", `/friends/api/public/blocklist/${D}/${A}`, { as: A })).status, 403, "only the caller's own list");
        assert.equal((await Call("PUT", `/friends/api/public/blocklist/${A}/${D}`)).status, 401);
        assert.equal((await Call("DELETE", `/friends/api/public/blocklist/${A}/${D}`, { as: A })).status, 204);
        assert.deepEqual(ParseBlockList((await Call("GET", `/friends/api/public/blocklist/${A}`, { as: A })).text), []);
    });

    it("20 new requests per 10 minutes, at most 50 unanswered; accepting an inbound request is never limited", async () => {
        let Offset = 0;
        SetFriendsClockForTests(() => Date.now() + Offset);
        ResetFriendsMemoryForTests();

        const E = "UID-social-e", X = "UID-social-x";
        const Targets: string[] = [];

        for(const [Id, Name] of [[E, "Echo"], [X, "Xray"]]){
            GetDb().insert(users).values({ userId: Id, name: Name, notes: 0 }).run();
            Tokens[Id] = SignMetagameJWTForUid(Id);
        }

        for(let Index = 0; Index < 52; Index++){
            const Id = `UID-social-cap-${String(Index).padStart(2, "0")}`;
            GetDb().insert(users).values({ userId: Id, name: `Cap${Index}`, notes: 0 }).run();
            Targets.push(Id);
        }

        const Ask = async (Target: string) => (await Call("POST", `/friends/api/public/friends/${E}/${Target}`, { as: E })).status;

        // X asks E first: E's accept later must not count against E's limits
        assert.equal((await Call("POST", `/friends/api/public/friends/${X}/${E}`, { as: X })).status, 204);

        for(let Index = 0; Index < 20; Index++){
            assert.equal(await Ask(Targets[Index]), 204, `request ${Index + 1}`);
        }

        assert.equal(await Ask(Targets[20]), 409, "the 21st in 10 minutes");
        assert.equal(await Ask(Targets[0]), 204, "asking again changes nothing and is not refused");
        assert.equal((await Call("POST", `/friends/api/public/friends/${E}/${X}`, { as: E, emptyJson: true })).status, 204, "accepting X's request while the window is full");

        Offset += 10 * 60 * 1000 + 1;
        for(let Index = 20; Index < 40; Index++){
            assert.equal(await Ask(Targets[Index]), 204, `request ${Index + 1}, after the window moved`);
        }

        Offset += 10 * 60 * 1000 + 1;
        for(let Index = 40; Index < 50; Index++){
            assert.equal(await Ask(Targets[Index]), 204, `request ${Index + 1}`);
        }

        assert.equal(await Ask(Targets[50]), 409, "the 51st unanswered request");
        assert.equal((await FriendsList(E)).filter((Entry) => Entry.status === "PENDING" && Entry.direction === "OUTBOUND").length, 50);

        // Withdrawing one frees a place
        assert.equal((await Call("DELETE", `/friends/api/public/friends/${E}/${Targets[0]}`, { as: E })).status, 204);
        assert.equal(await Ask(Targets[50]), 204);

        assert.equal(await Ask(Targets[51]), 409, "full again");
        assert.equal((await Call("POST", `/friends/api/public/friends/${Targets[51]}/${E}`, { as: E })).status, 403, "the URL's first id must be the caller");
    });
});

describe("11. party invites: blocks, declines and the sender's limit", () => {
    const P = "UID-social-p", Q = "UID-social-q", R = "UID-social-r", S = "UID-social-s", W = "UID-social-w";
    const Targets: string[] = [];
    const PartyOf = async (Who: string) => (await Poll(Who)).json.partyId as string;
    const SendInvite = async (From: string, To: string) => (await Call("PUT", "/party/invite", { as: From, body: { recipientPlayerId: To, partyId: await PartyOf(From), ...BUILD } })).status;
    const InvitesOf = async (Who: string) => ParseInvitations((await Call("GET", "/party/invites", { as: Who })).text)!.map((Entry) => Entry.sendingPlayerId);
    const Decline = async (Who: string, From: string) => {
        const Party = (await PartyOf(From));
        return (await Call("DELETE", "/party/invite", { as: Who, body: { sendingPlayerId: Party, recipientPlayerId: Who, partyId: Party } })).status;
    };

    before(() => {
        for(const [Id, Name] of [[P, "Papa"], [Q, "Quebec"], [R, "Romeo"], [S, "Sierra"], [W, "Whiskey"]]){
            GetDb().insert(users).values({ userId: Id, name: Name, notes: 0 }).run();
            Tokens[Id] = SignMetagameJWTForUid(Id);
        }

        for(let Index = 0; Index < 21; Index++){
            const Id = `UID-social-t${String(Index).padStart(2, "0")}`;
            GetDb().insert(users).values({ userId: Id, name: `Tango${Index}`, notes: 0 }).run();
            Tokens[Id] = SignMetagameJWTForUid(Id);
            Targets.push(Id);
        }
    });

    after(() => SetPartyClockForTests());

    it("a block removes the invites between the two, both ways; an invite between blocked players is never listed and cannot be accepted", async () => {
        assert.equal(await SendInvite(P, Q), 200);
        assert.equal(await SendInvite(Q, P), 200);
        assert.deepEqual([await InvitesOf(Q), await InvitesOf(P)], [[P], [Q]]);

        assert.equal((await Call("POST", `/friends/api/public/blocklist/${Q}/${P}`, { as: Q })).status, 204);
        assert.deepEqual([await InvitesOf(Q), await InvitesOf(P)], [[], []], "both directions");
        assert.equal((await Call("DELETE", `/friends/api/public/blocklist/${Q}/${P}`, { as: Q })).status, 204);
        assert.deepEqual(await InvitesOf(Q), [], "removed, not only hidden");

        // A block written straight to the database (as one made before this rule): the invite is hidden,
        // and accepting it is 404
        const PartyP = await PartyOf(P);
        assert.equal(await SendInvite(P, R), 200);
        GetDb().insert(blocks).values({ blockerId: R, blockedId: P, createdAt: Date.now() }).run();
        assert.deepEqual(await InvitesOf(R), []);
        const Accepted = await Call("PUT", `/party/invite/accept/${PartyP}`, { as: R, body: { recipientPlayerId: R, partyId: PartyP, ...BUILD } });
        assert.deepEqual([Accepted.status, Accepted.json], [404, {}]);
        assert.deepEqual((await Poll(R)).json.playerStates.map((State: any) => State.playerId), [R], "R stays in their own party");
    });

    it("a declined sender waits 2 minutes before inviting the same player again; other senders do not", async () => {
        assert.equal(await SendInvite(P, Q), 200);
        assert.equal(await Decline(Q, P), 200);
        assert.deepEqual(await InvitesOf(Q), []);
        assert.equal(await SendInvite(P, Q), 409, "at once");
        assert.equal(await SendInvite(S, Q), 200, "another sender");

        SetPartyClockForTests(() => Date.now() + 2 * 60 * 1000 + 1000);
        assert.equal(await SendInvite(P, Q), 200, "two minutes later");
    });

    it("a sender sends at most 20 invites per 10 minutes (409), counted across declines", async () => {
        let Shift = 2 * 60 * 1000 + 1000;
        SetPartyClockForTests(() => Date.now() + Shift);

        for(let Index = 0; Index < 20; Index++){
            assert.equal(await SendInvite(W, Targets[Index]), 200, `invite ${Index + 1}`);
            assert.equal(await Decline(Targets[Index], W), 200);
        }

        assert.equal(await SendInvite(W, Targets[20]), 409, "the 21st in 10 minutes");

        Shift += 10 * 60 * 1000;
        assert.equal(await SendInvite(W, Targets[20]), 200, "after the window moved");
    });
});
