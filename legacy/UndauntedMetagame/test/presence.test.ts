import { RemoveTestDb } from "./setup";
import "./authenv";
import { IsSelfStanza, SelfStanzas, StanzasChecked } from "./chatinvariant";
import { after, afterEach, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import { parse } from "ltx";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import { BlockPlayer, RemoveFriend, SendOrAcceptFriendRequest } from "../src/controllers/friends";
import { ChatServer, ReadChatConfig } from "../src/realtime/chat";
import { FRIEND_MESSAGE_TYPE, FriendPresence, PRESENCE_BURST, PRESENCE_REFILL_MS, PresenceSession } from "../src/realtime/presence";
import { EscapeXml } from "../src/realtime/xmpp";
import { CaptureLogs, DOMAIN, Login, LogCapture, WireClient } from "./chatwire";

// Friends' online status (CHAT_PRESENCE=1; realtime/presence.ts, docs/findings/chat.md). Real chat
// servers on port 0 with a test clock, real friendships in the test database, and raw clients shaped like
// the 1.4.4 client (chatwire.ts). The idea of a presence service came from Harmonic's fork
// (github.com/Harmonicrain/Undaunted 895f7c7); these cases check what the client needs from it: its own
// <status> relayed unchanged from its full JID, unavailable from the full JID, only friends, the friends
// list push on an accept, and never a stanza from the receiver's own account (chatinvariant.ts checks
// that over every stanza of this file).

const A = "UID-presence-a", A_NAME = "Alpha";
const B = "UID-presence-b", B_NAME = "Bravo";
const C = "UID-presence-c", C_NAME = "Charlie";
const D = "UID-presence-d", D_NAME = "Delta";
const E = "UID-presence-e", E_NAME = "Echo";

let Now = Date.parse("2026-09-23T12:00:00Z");
let Server: ChatServer;
let Logs: LogCapture;
const Opened: WireClient[] = [];

// The client's <status>: JSON written by 0x140951da0 (presence-friends.md 3.3), with characters that need escaping
function StatusJson(Text: string){
    return JSON.stringify({ Status: Text, bIsPlaying: false, bIsJoinable: false, bHasVoiceSupport: false, SessionId: "", Properties: { RichPresence_s: "InTheCity", PartyPlayerCountData_i: 1, Note_s: "<&'\">" } });
}

const CITY = StatusJson("Dauntless - In the city");
const HUNT = StatusJson("Dauntless - In a hunt");

// A broadcast presence as FXmppPresenceStrophe::UpdatePresence builds it (0x143a3f6c0): no "to", no type
function Broadcast(Client: WireClient, Status: string, Show?: string){
    Client.Send(`<presence>${Show !== undefined ? `<show>${Show}</show>` : ""}<status>${EscapeXml(Status)}</status><delay xmlns="urn:xmpp:delay" stamp="2026-09-23T12:00:00.000Z"/></presence>`);
}

type Seen = { Kind: string, From: string, To: string, Type?: string, Show?: string, Status?: string, Body?: string };

// The stanzas outside rooms among these frames, parsed
function OutsideRooms(Frames: string[]): Seen[] {
    const Out: Seen[] = [];

    for(const Frame of Frames){
        const Node = parse(Frame);
        const From = String(Node.attrs.from ?? "");

        if((Node.name !== "presence" && Node.name !== "message") || /@muc\./.test(From)){
            continue;
        }

        Out.push({
            Kind: Node.name,
            From,
            To: String(Node.attrs.to ?? ""),
            Type: Node.attrs.type as string | undefined,
            Show: Node.getChild("show")?.getText(),
            Status: Node.getChild("status")?.getText(),
            Body: Node.getChild("body")?.getText()
        });
    }

    return Out;
}

async function Frames(Client: WireClient){
    return OutsideRooms(await Client.Barrier());
}

function Jid(Client: WireClient){
    return `${Client.Uid}@${Client.Domain}/${Client.Resource}`;
}

async function Online(Uid: string, Target = Server){
    const Client = await Login(Target.port, Uid);
    Opened.push(Client);
    return Client;
}

function Befriend(X: string, Y: string){
    assert.equal(SendOrAcceptFriendRequest(X, Y).ok, true);
    assert.deepEqual(SendOrAcceptFriendRequest(Y, X), { ok: true, Result: "accepted" });
}

function Unfriend(X: string, Y: string){
    RemoveFriend(X, Y);
}

before(async () => {
    GetDb().insert(users).values([
        { userId: A, name: A_NAME, notes: 0, isAdmin: false }, { userId: B, name: B_NAME, notes: 0, isAdmin: false },
        { userId: C, name: C_NAME, notes: 0, isAdmin: false }, { userId: D, name: D_NAME, notes: 0, isAdmin: false },
        { userId: E, name: E_NAME, notes: 0, isAdmin: false }
    ]).run();
    Logs = CaptureLogs();
    Server = new ChatServer({ Clock: () => Now, AutoTick: false, Presence: true });
    await Server.listen(0);
});

afterEach(async () => {
    for(const Client of Opened.splice(0)){
        if(Client.IsOpen){
            await Client.Logout();
        }
    }

    for(const [X, Y] of [[A, B], [A, C], [A, D], [B, C], [A, E]]){
        Unfriend(X, Y);
    }

    GetDb().$client.prepare("delete from blocks").run();
});

after(async () => {
    await Server.close();
    Logs.Stop();
    RemoveTestDb(() => GetDb().$client.close());
});

describe("friends' online status (CHAT_PRESENCE=1)", () => {
    it("(a) friends exchange presence: <show> and <status> as sent, from the full JID, the late one hears of the early one", async () => {
        Befriend(A, B);

        const Alice = await Online(A);
        Broadcast(Alice, CITY);
        assert.deepEqual(await Frames(Alice), [], "no friend is online yet, and nothing comes back to the sender");

        const Bob = await Online(B);
        Broadcast(Bob, HUNT, "away");

        assert.deepEqual(await Frames(Bob), [{ Kind: "presence", From: Jid(Alice), To: Jid(Bob), Type: undefined, Show: undefined, Status: CITY, Body: undefined }], "Bob hears of Alice at his first presence");
        assert.deepEqual(await Frames(Alice), [{ Kind: "presence", From: Jid(Bob), To: Jid(Alice), Type: undefined, Show: "away", Status: HUNT, Body: undefined }], "Alice hears of Bob, <show> included");
        assert.ok(Jid(Alice).includes("/V2:"), "the from carries the resource the game bound");
    });

    it("(b) never a stanza from the player's own account: a second session hears nothing from the first, and a friend hears both", async () => {
        Befriend(A, B);

        const First = await Online(A);
        const Second = await Online(A);
        const Bob = await Online(B);

        Broadcast(Bob, CITY);
        await Frames(Bob);
        Broadcast(First, CITY);
        assert.deepEqual((await Frames(First)).map((Each) => Each.From), [Jid(Bob)], "only Bob's presence, never its own");
        assert.deepEqual(await Frames(Second), [], "the second session of the same account hears nothing from the first");

        Broadcast(Second, HUNT);
        assert.deepEqual((await Frames(Second)).map((Each) => Each.From), [Jid(Bob)]);
        assert.deepEqual(await Frames(First), [], "nor the first from the second");
        assert.deepEqual((await Frames(Bob)).map((Each) => [Each.From, Each.Status]), [[Jid(First), CITY], [Jid(Second), HUNT]]);

        // The session ends: the friend sees it go, then the other session's presence again
        await First.Logout();
        assert.deepEqual((await Frames(Bob)).map((Each) => [Each.From, Each.Type ?? "available", Each.Status]), [[Jid(First), "unavailable", undefined], [Jid(Second), "available", HUNT]]);
        assert.deepEqual(await Frames(Second), [], "the remaining session is not told about its twin");
    });

    it("(b) the check itself: a stanza from the receiver's own account is caught, room stanzas and server stanzas are not", () => {
        assert.equal(IsSelfStanza(A, `<presence from="${A}@${DOMAIN}/V2:x" to="${A}@${DOMAIN}/V2:y"/>`), true);
        assert.equal(IsSelfStanza(A, `<presence from="${A.toLowerCase()}@${DOMAIN}/r" to="x"/>`), true, "case does not matter to the client");
        assert.equal(IsSelfStanza(A, `<message from="${A}@${DOMAIN}"><body>x</body></message>`), true, "a bare JID too");
        assert.equal(IsSelfStanza(A, `<presence from="City-1@muc.${DOMAIN}/Alpha:${A}:r" to="${A}@${DOMAIN}/r"/>`), false, "a room presence names the player in its nickname");
        assert.equal(IsSelfStanza(A, `<iq type="get" id="sp1" from="${DOMAIN}" to="${A}@${DOMAIN}/r"><ping xmlns="urn:xmpp:ping"/></iq>`), false);
        assert.equal(IsSelfStanza(A, `<presence from="${B}@${DOMAIN}/r" to="${A}@${DOMAIN}/r"><status>from="${A}@${DOMAIN}/r"</status></presence>`), false, "only the stanza's own from counts");
        assert.equal(IsSelfStanza(A, `<message from="xmpp-admin@${DOMAIN}" to="${A}@${DOMAIN}/r"><body/></message>`), false);
    });

    it("(b) a friends lookup that names the player itself sends it nothing", async () => {
        const Own = new ChatServer({ Clock: () => Now, AutoTick: false, Presence: true, Friends: { FriendsOf: (Uid) => [Uid, Uid.toLowerCase()], IsBlockedEitherWay: () => false } });
        await Own.listen(0);

        try{
            const First = await Online(C, Own);
            const Second = await Online(C, Own);

            Broadcast(Second, CITY);
            await Frames(Second);
            Broadcast(First, HUNT);
            assert.deepEqual(await Frames(First), []);
            assert.deepEqual(await Frames(Second), []);
            await First.Logout();
            assert.deepEqual(await Frames(Second), []);
        }
        finally{
            for(const Client of Opened.splice(0)) await Client.Logout();
            await Own.close();
        }
    });

    it("(b) the last guard: a stanza that would reach a session of its own sender's account is refused and logged", () => {
        const Sent: string[] = [];
        const Session = (Id: number, Uid: string) => ({ Id, Uid, Domain: DOMAIN, Resource: `V2:test:WIN::${Id}`, Ended: false, PresenceTokens: { Tokens: 5, At: Now }, PresencePending: false } as PresenceSession);
        const Mine = Session(901, "UID-guard");
        const Twin = Session(902, "uid-GUARD");
        const Before = Logs.Lines.length;
        // A broken host that files the player's own second session under a friend
        const Presence = new FriendPresence({
            SessionsOf: (Uid) => Uid === "UID-guard-friend" ? [{ ...Twin, LastPresence: {} }] : [],
            Sessions: () => [],
            Send: (_To, Stanza) => { Sent.push(Stanza); },
            Clock: () => Now,
            LogOnce: () => true
        }, { FriendsOf: () => ["UID-guard-friend"], IsBlockedEitherWay: () => false });

        Presence.Broadcast(Mine, parse(`<presence><status>${EscapeXml(CITY)}</status></presence>`));
        Presence.Unavailable(Mine, "test");

        assert.deepEqual(Sent, []);
        assert.ok(Logs.Lines.slice(Before).includes("error chat: presence: refused to send c=902 a stanza from its own account"), Logs.Lines.slice(Before).join("\n"));
    });

    it("(c) unavailable from the full JID when a session closes, drops, pings out or says so", async () => {
        Befriend(A, B);

        const Bob = await Online(B);
        Broadcast(Bob, CITY);
        await Frames(Bob);

        const Offline = async (Leave: (Alice: WireClient) => Promise<void>, Label: string) => {
            const Alice = await Online(A);
            Broadcast(Alice, CITY);
            await Frames(Alice);
            assert.deepEqual((await Frames(Bob)).map((Each) => [Each.From, Each.Type]), [[Jid(Alice), undefined]], `${Label}: online first`);

            await Leave(Alice);

            const Seen_ = await Frames(Bob);
            assert.deepEqual(Seen_.map((Each) => [Each.Kind, Each.From, Each.To, Each.Type, Each.Status]), [["presence", Jid(Alice), Jid(Bob), "unavailable", undefined]], Label);
        };

        await Offline(async (Alice) => { await Alice.Logout(); }, "<close/>");
        await Offline(async (Alice) => {
            const Closed = new RegExp(`^info chat: closed c=\\d+ uid=${A} reason=socket `);
            const Count = () => Logs.Lines.filter((Line) => Closed.test(Line)).length;
            const Before = Count();

            Alice.Close();
            await Alice.Closed;
            for(let Wait = 0; Wait < 100 && Count() === Before; Wait++) await new Promise((Resolve) => setTimeout(Resolve, 10));
            assert.equal(Count(), Before + 1, "the server saw the socket go");
        }, "a dropped socket");
        await Offline(async (Alice) => {
            Alice.Send(`<presence type="unavailable"/>`);
            await Alice.Barrier();
            assert.ok(Alice.IsOpen, "the session stays");
        }, "an unavailable broadcast");
        await Offline(async (Alice) => {
            Alice.AutoPong = false;
            Now += 51 * 1000;
            Server.Tick();
            Now += 101 * 1000;
            Bob.Send(`<iq type="get" id="keepalive"><ping xmlns="urn:xmpp:ping"/></iq>`);
            await Bob.Expect("keepalive");
            Server.Tick();
            await Alice.Closed;
        }, "a ping timeout");
    });

    it("(d) nobody but friends hears it: not strangers, not a blocked player, not after an unfriend", async () => {
        Befriend(A, B);

        const Alice = await Online(A);
        const Bob = await Online(B);
        const Carl = await Online(C);
        Broadcast(Carl, CITY);
        Broadcast(Bob, CITY);
        await Frames(Carl);
        await Frames(Bob);
        Broadcast(Alice, CITY);
        await Frames(Alice);
        assert.equal((await Frames(Bob)).length, 1, "the friend hears it");
        assert.deepEqual(await Frames(Carl), [], "a stranger does not");

        // A block ends the friendship: each side sees the other go once, then nothing more
        assert.equal(BlockPlayer(B, A).ok, true);
        assert.deepEqual((await Frames(Bob)).map((Each) => [Each.From, Each.Type]), [[Jid(Alice), "unavailable"]]);
        assert.deepEqual((await Frames(Alice)).map((Each) => [Each.From, Each.Type]), [[Jid(Bob), "unavailable"]]);
        Broadcast(Alice, HUNT);
        await Frames(Alice);
        assert.deepEqual(await Frames(Bob), [], "a blocked player hears no more");

        // A lookup that still listed the pair as friends does not get past the block either
        const Own = new ChatServer({ Clock: () => Now, AutoTick: false, Presence: true, Friends: { FriendsOf: (Uid) => Uid === D ? [E] : [D], IsBlockedEitherWay: () => true } });
        await Own.listen(0);

        try{
            const Dee = await Online(D, Own);
            const Eve = await Online(E, Own);
            Broadcast(Eve, CITY);
            Broadcast(Dee, CITY);
            assert.deepEqual(await Frames(Dee), []);
            assert.deepEqual(await Frames(Eve), []);
            await Dee.Logout();
            assert.deepEqual(await Frames(Eve), []);
        }
        finally{
            await Own.close();
        }

        // An unfriend: the same
        GetDb().$client.prepare("delete from blocks").run();
        Befriend(A, C);
        assert.deepEqual((await Frames(Carl)).map((Each) => [Each.Kind, Each.From]), [["message", `xmpp-admin@${DOMAIN}`], ["presence", Jid(Alice)]]);
        await Frames(Alice);
        Broadcast(Alice, CITY);
        await Frames(Alice);
        assert.equal((await Frames(Carl)).length, 1, "a new friend hears the next change");
        Unfriend(C, A);
        assert.deepEqual((await Frames(Carl)).map((Each) => [Each.From, Each.Type]), [[Jid(Alice), "unavailable"]]);
        Broadcast(Alice, HUNT);
        await Frames(Alice);
        assert.deepEqual(await Frames(Carl), []);
    });

    it("(e) an accepted friend request pushes the friends-list entry from xmpp-admin, then the two presences", async () => {
        const Alice = await Online(A);
        const Carl = await Online(C);
        Broadcast(Alice, CITY, "chat");
        Broadcast(Carl, HUNT);
        await Frames(Alice);
        await Frames(Carl);

        assert.deepEqual(SendOrAcceptFriendRequest(C, A), { ok: true, Result: "requested" });
        assert.deepEqual(await Frames(Alice), [], "a request alone pushes nothing");

        assert.deepEqual(SendOrAcceptFriendRequest(A, C), { ok: true, Result: "accepted" });

        for(const [Me, Other, Direction] of [[Alice, Carl, "INBOUND"], [Carl, Alice, "OUTBOUND"]] as [WireClient, WireClient, string][]){
            const Seen_ = await Frames(Me);

            assert.equal(Seen_.length, 2, JSON.stringify(Seen_));
            assert.deepEqual([Seen_[0].Kind, Seen_[0].From, Seen_[0].To, Seen_[0].Type], ["message", `xmpp-admin@${DOMAIN}`, Jid(Me), undefined], "a plain message from xmpp-admin at the session's own domain");

            const Body = JSON.parse(Seen_[0].Body!);
            assert.deepEqual(Object.keys(Body), ["type", "payload", "timestamp"]);
            assert.equal(Body.type, FRIEND_MESSAGE_TYPE);
            assert.deepEqual({ ...Body.payload, created: undefined }, { accountId: Other.Uid, status: "ACCEPTED", direction: Direction, created: undefined });
            assert.ok(!Number.isNaN(Date.parse(Body.payload.created)) && !Number.isNaN(Date.parse(Body.timestamp)));

            assert.deepEqual([Seen_[1].Kind, Seen_[1].From, Seen_[1].Type], ["presence", Jid(Other), undefined], "then the new friend's presence");
        }
    });

    it("(f) a change is relayed; the same presence again is not; a flood is held back and its latest goes out at the next tick", async () => {
        Befriend(A, B);

        const Alice = await Online(A);
        const Bob = await Online(B);
        Broadcast(Bob, CITY);
        await Frames(Bob);
        Broadcast(Alice, CITY);
        await Frames(Alice);
        await Frames(Bob);

        Broadcast(Alice, HUNT);
        Broadcast(Alice, HUNT);
        await Frames(Alice);
        assert.deepEqual((await Frames(Bob)).map((Each) => Each.Status), [HUNT], "one change, one relay");

        // The first presence and HUNT took two of PRESENCE_BURST tokens
        const Texts = [...Array(PRESENCE_BURST + 2).keys()].map((Index) => StatusJson(`Dauntless - In events ${Index}`));

        for(const Text of Texts){
            Broadcast(Alice, Text);
        }

        await Frames(Alice);
        assert.deepEqual((await Frames(Bob)).map((Each) => Each.Status), Texts.slice(0, PRESENCE_BURST - 2), "held back after the burst");

        Now += PRESENCE_REFILL_MS;
        Server.Tick();
        assert.deepEqual((await Frames(Bob)).map((Each) => Each.Status), [Texts.at(-1)], "the latest one, once");
        Server.Tick();
        assert.deepEqual(await Frames(Bob), []);
    });

    it("(f) going offline and online in a loop is held back like any other change, and the friends lookups stay few", async () => {
        let Lookups = 0, BlockChecks = 0;
        const Own = new ChatServer({ Clock: () => Now, AutoTick: false, Presence: true, Friends: {
            FriendsOf: (Uid) => { Lookups++; return Uid === D ? [E] : Uid === E ? [D] : []; },
            IsBlockedEitherWay: () => { BlockChecks++; return false; }
        } });
        await Own.listen(0);

        try{
            const Eve = await Online(E, Own);
            const Dee = await Online(D, Own);
            Broadcast(Eve, CITY);
            await Frames(Eve);
            Broadcast(Dee, CITY);
            await Frames(Dee);
            assert.equal((await Frames(Eve)).length, 1, "Eve hears Dee come online");
            Lookups = 0;
            BlockChecks = 0;

            // The first presence took one of PRESENCE_BURST tokens; each unavailable and each return takes one
            for(let Round = 0; Round < PRESENCE_BURST + 2; Round++){
                Dee.Send(`<presence type="unavailable"/>`);
                Broadcast(Dee, CITY);
            }

            await Frames(Dee);
            const Heard = await Frames(Eve);
            assert.ok(Heard.length <= PRESENCE_BURST, `${Heard.length} stanzas`);
            assert.deepEqual(Heard.map((Each) => Each.Type ?? "available"), ["unavailable", "available", "unavailable", "available"], "two rounds, then held back");
            assert.ok(Lookups <= PRESENCE_BURST - 1 && BlockChecks <= PRESENCE_BURST - 1, `${Lookups} lookups, ${BlockChecks} block checks`);

            // Only the latest state goes out when its turn comes. Dee ended the loop online as Eve last saw
            // it, so Eve is told nothing; Dee, deaf for a while, hears of Eve again.
            Now += PRESENCE_REFILL_MS;
            Own.Tick();
            assert.deepEqual(await Frames(Eve), []);
            assert.deepEqual((await Frames(Dee)).map((Each) => Each.From), [Jid(Eve)]);

            Dee.Send(`<presence type="unavailable"/>`);
            Broadcast(Dee, HUNT);
            await Frames(Dee);
            assert.deepEqual(await Frames(Eve), [], "no token left");
            Now += PRESENCE_REFILL_MS;
            Own.Tick();
            assert.deepEqual((await Frames(Eve)).map((Each) => [Each.From, Each.Type, Each.Status]), [[Jid(Dee), undefined, HUNT]], "the latest one, once");
            Own.Tick();
            assert.deepEqual(await Frames(Eve), []);

            // Ending the session is never held back
            await Dee.Logout();
            assert.deepEqual((await Frames(Eve)).map((Each) => [Each.From, Each.Type]), [[Jid(Dee), "unavailable"]]);
        }
        finally{
            for(const Client of Opened.splice(0)) await Client.Logout();
            await Own.close();
        }
    });

    it("(g) CHAT_PRESENCE off: not one presence or friends-list stanza outside rooms", async () => {
        const Saved = process.env.CHAT_PRESENCE;
        process.env.CHAT_PRESENCE = "0";

        const Off = new ChatServer({ Clock: () => Now, AutoTick: false });
        await Off.listen(0);

        try{
            const Alice = await Online(A, Off);
            const Bob = await Online(B, Off);

            Broadcast(Alice, CITY);
            Broadcast(Bob, HUNT);
            Befriend(A, B);
            Broadcast(Alice, HUNT);
            Alice.Send(`<presence type="unavailable"/>`);
            await Alice.Barrier();
            await Alice.Logout();
            Off.Tick();

            assert.deepEqual(OutsideRooms(Bob.Frames), []);
            assert.deepEqual(OutsideRooms(Alice.Frames), []);
            assert.ok(Logs.Lines.includes("info chat: friends' online status off: no presence is sent outside rooms"));
        }
        finally{
            if(Saved === undefined) delete process.env.CHAT_PRESENCE; else process.env.CHAT_PRESENCE = Saved;
            await Off.close();
        }
    });

    it("the default: friends automatically see login and logout without a presence setting", async () => {
        const Saved = process.env.CHAT_PRESENCE;
        delete process.env.CHAT_PRESENCE;
        const On = new ChatServer({ Clock: () => Now, AutoTick: false });

        try{
            await On.listen(0);
            Befriend(A, B);
            const Alice = await Online(A, On);
            const Bob = await Online(B, On);
            Broadcast(Alice, CITY);
            await Alice.Barrier();
            Broadcast(Bob, HUNT);
            assert.ok((await Frames(Bob)).some((Frame) => Frame.Kind === "presence" && Frame.From === Jid(Alice) && Frame.Status === CITY));
            const AliceJid = Jid(Alice);
            await Alice.Logout();
            assert.ok((await Frames(Bob)).some((Frame) => Frame.Kind === "presence" && Frame.From === AliceJid && Frame.Type === "unavailable"));
            await Bob.Logout();
            assert.ok(Logs.Lines.some((Line) => Line.startsWith("info chat: friends' online status on (CHAT_PRESENCE=1)")));
        }
        finally{
            await On.close();
            if(Saved === undefined) delete process.env.CHAT_PRESENCE; else process.env.CHAT_PRESENCE = Saved;
        }

        assert.deepEqual(ReadChatConfig({ CHAT: "0", CHAT_PRESENCE: "1" }).Warnings, ["CHAT_PRESENCE is on but chat is off (CHAT=1 is needed); nobody shows as online"]);
        assert.deepEqual(ReadChatConfig({ CHAT: "0" }).Warnings, ["CHAT_PRESENCE is on but chat is off (CHAT=1 is needed); nobody shows as online"]);
        assert.deepEqual(ReadChatConfig({ CHAT_PRESENCE: "1" }).Warnings, []);
        assert.deepEqual(ReadChatConfig({ CHAT: "1", CHAT_PRESENCE: "1" }).Warnings, []);
        assert.deepEqual(ReadChatConfig({ CHAT_PRESENCE: "0" }).Warnings, []);
    });

    it("every stanza of this file kept the rule", () => {
        assert.ok(StanzasChecked() > 50, String(StanzasChecked()));
        assert.deepEqual(SelfStanzas, []);
        const Errors = Logs.Lines.filter((Line) => Line.startsWith("error "));
        assert.ok(Errors.length > 0 && Errors.every((Line) => /^error chat: presence: refused to send c=90[12] a stanza from its own account$/.test(Line)), `only the guard test's lines: ${Errors.join("\n")}`);
    });
});
