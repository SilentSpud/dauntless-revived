import { RemoveTestDb } from "./setup";
import "./authenv";
import { CHAT_HTTP_API_PORT } from "./chatenv";
// Every stanza these tests make the server send is checked: none outside a room from the receiver's own account
import "./chatinvariant";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import type { Server } from "node:http";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { ChatServer } from "../src/realtime/chat";
import { ChatClientModel, JOINED, Lookup } from "./chatclient";
import { CaptureLogs, LogCapture, Login, WireClient } from "./chatwire";

// Names through the real account routes (docs/findings/chat.md, "Where each name comes from"): the
// metagame app on a spare port, chat on port 0, and two client models that read their names the way
// the 1.4.4 client does. Nothing in the metagame changed for this: both lookups already answer
// displayName to a caller with a valid token.

const BASE = `http://127.0.0.1:${CHAT_HTTP_API_PORT}`;
const A = "UID-chathttp-a";
const B = "UID-chathttp-b";
const ROOM = "City-9e8d7c6b-0000-4000-8000-000000000001";

let Listening: Server | undefined;
let Chat: ChatServer;
let Logs: LogCapture;
const Wires: WireClient[] = [];

async function Get(Path: string, As?: string){
    const Response = await fetch(BASE + Path, { headers: As !== undefined ? { authorization: `bearer ${SignMetagameJWTForUid(As)}` } : {} });
    return { status: Response.status, json: await Response.json() as unknown };
}

// The client's own name: GET /account/api/public/account/<own id> at login (QueryLoggedInUserInfo,
// 0x140937614 -> 0x1409405e0); GetPlayerNickname then reads its displayName, or "InvalidMCPUser"
async function PlayerNickname(Uid: string){
    const Reply = await Get(`/account/api/public/account/${Uid}`, Uid);
    const Name = (Reply.json as { displayName?: unknown }).displayName;

    return typeof Name === "string" && Name.length > 0 ? Name : "InvalidMCPUser";
}

// Other players: GET /account/api/public/account?accountId=<id> with the caller's own token, read as an
// array of {id, displayName} (the {"users": ...} wrap at 0x1409897b0)
function LookupAs(Caller: string): Lookup {
    return async (Id) => {
        const Reply = await Get(`/account/api/public/account?accountId=${encodeURIComponent(Id)}`, Caller);
        const Found = Array.isArray(Reply.json) ? (Reply.json as Array<{ id?: unknown, displayName?: unknown }>).find((Entry) => Entry.id === Id) : undefined;

        return typeof Found?.displayName === "string" ? Found.displayName : undefined;
    };
}

before(async () => {
    GetDb().insert(users).values([{ userId: A, name: "Alpha", notes: 0, isAdmin: false }, { userId: B, name: "Bravo", notes: 0, isAdmin: false }]).run();
    Logs = CaptureLogs();
    Listening = await new Promise<Server>((Resolve, Reject) => {
        const Started = app.listen(CHAT_HTTP_API_PORT, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
    });
    Chat = new ChatServer({ AutoTick: false });
    await Chat.listen(0);
});

after(async () => {
    for(const Wire of Wires) Wire.Close();
    await Chat.close();
    Listening?.closeAllConnections();
    await new Promise<void>((Resolve) => Listening ? Listening.close(() => Resolve()) : Resolve());
    Logs.Stop();
    RemoveTestDb(() => GetDb().$client.close());
});

describe("chat names through the real account routes", () => {
    it("each player's own name comes from its account route, joins with it, and sees the other's name from the lookup", async () => {
        const Players = [];

        for(const Uid of [A, B]){
            const Name = await PlayerNickname(Uid);
            const Wire = await Login(Chat.port, Uid);
            Wires.push(Wire);
            Players.push({ Uid, Name, Wire, Model: new ChatClientModel({ LocalUid: Uid, Resource: Wire.Resource, LocalSocialName: Name }) });
        }

        const [Alpha, Bravo] = Players;
        assert.equal(Alpha.Name, "Alpha");
        assert.equal(Bravo.Name, "Bravo");

        // The sender's own connection settles first, so everything it caused is in the others' queues
        for(const Each of Players){
            Each.Wire.Send(Each.Model.JoinPublicRoom(ROOM, Each.Name)!);
            for(const Other of [Each, ...Players.filter((Player) => Player !== Each)]) Other.Model.ReceiveAll(await Other.Wire.Barrier());
        }

        assert.equal(Alpha.Model.RoomOf(ROOM)!.State, JOINED, "the join built from the account route's name is accepted");
        assert.equal(Bravo.Model.RoomOf(ROOM)!.State, JOINED);

        Alpha.Wire.Send(Alpha.Model.RoomMessage(ROOM, "hello", "g1"));
        for(const Each of Players) Each.Model.ReceiveAll(await Each.Wire.Barrier());

        assert.deepEqual(await Bravo.Model.ShownLines(LookupAs(B)), ["Alpha: hello"]);
        assert.deepEqual(await Alpha.Model.ShownLines(LookupAs(A)), ["Alpha: hello"], "its own line: the self path");
        assert.ok(Logs.Lines.includes(`info Account info for 1 account(s) by userId ${B}: 1 found`));
    });

    it("without a token the per-id route answers {}, and the client falls back to the nickname's name part, which the server checked", async () => {
        const Reply = await Get(`/account/api/public/account/${A}`);
        assert.deepEqual(Reply.json, {});

        const Unsigned: Lookup = async (Id) => {
            const Answer = await Get(`/account/api/public/account/${encodeURIComponent(Id)}`);
            const Name = (Answer.json as { displayName?: unknown }).displayName;
            return typeof Name === "string" ? Name : undefined;
        };
        const Bravo = new ChatClientModel({ LocalUid: B, Resource: "r", LocalSocialName: "Bravo" });
        Bravo.JoinPublicRoom(ROOM, "Bravo");
        Bravo.Receive(`<presence from="${ROOM}@muc.prod.ol.epicgames.com/Alpha:${A}:rA"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant" jid="${A}@prod.ol.epicgames.com/rA"/></x></presence>`);
        Bravo.Receive(`<message type="groupchat" from="${ROOM}@muc.prod.ol.epicgames.com/Alpha:${A}:rA"><body>hi</body></message>`);
        assert.deepEqual(await Bravo.ShownLines(Unsigned), ["Alpha: hi"]);
    });
});

// Regression: the fleet outgrew the old realm-wide 64-socket ceiling.
it('admits 100 authenticated players on the shared chat service', async () => {
    for(let i=0;i<100;i++){
        const userId=`UID-chat-capacity-${i}`;
        GetDb().insert(users).values({userId,name:`Capacity${i}`,notes:0,isAdmin:false}).run();
        Wires.push(await Login(Chat.port,userId));
    }
});
