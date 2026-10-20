import { RemoveTestDb } from "./setup";
import "./authenv";
// Every stanza these tests make the server send is checked: none outside a room from the receiver's own account
import "./chatinvariant";
import { after, afterEach, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import { ChatServer } from "../src/realtime/chat";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import { ChatClientModel, EXIT_PENDING, JOIN_PUBLIC_PENDING, JOINED, LookupFrom, NOT_JOINED, UrlDecode, UrlEncode } from "./chatclient";
import { DOMAIN, Login, WireClient } from "./chatwire";

// The client model (chatclient.ts) against two servers (docs/findings/chat.md):
// - pull request #9's first server, from its stanza templates at 9981955 and its two earlier attempts
//   (54af914, 44e835d). The model reproduces what Vvoidddd saw with a real 1.4.4 client on 22 September
//   2026, and predicts what a second player would have seen.
// - ours, captured from the real ChatServer in this process: both players see usernames.

const A = "UID-model-a";
const B = "UID-model-b";
const NAMES = { [A]: "Alpha", [B]: "Bravo" };
const Lookup = LookupFrom(NAMES);
const ROOM = "City-d147475b-7742-4e7b-8142-6c0f55dda06b"; // a room name seen in the 1.4.4 client's log
const MUC = `muc.${DOMAIN}`;

// ---- Pull request #9's replies (UndauntedMetagame/src/realtime/chat.ts at 9981955) ----
// It kept the room JID lowercased (to.split("/")[0].toLowerCase()), answered from <room>/<account id> with
// no <item jid>, and bound a random hex resource when the game's (67 characters) did not fit its
// 64-character rule.
const PR9 = {
    Room: `${ROOM.toLowerCase()}@${MUC}`,
    Jid: (Uid: string) => `${Uid}@${DOMAIN}/0a1b2c3d4e5f`,
    // `<presence from="${room}/${client.id}" ...><x muc#user><item affiliation="member" role="participant"/><status code="110"/></x></presence>`
    SelfPresence(Uid: string, Resource = Uid, Extra = ""){
        return `<presence from="${this.Room}/${Resource}" to="${this.Jid(Uid)}"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="member" role="participant"/><status code="110"/>${Extra}</x></presence>`;
    },
    // `<message from="${target}/${client.id}" to=... type="groupchat" id=...><body>...</body><nick xmlns="http://jabber.org/protocol/nick">${displayName}</nick></message>`
    Message(SenderResource: string, To: string, Text: string, Nick?: string){
        return `<message from="${this.Room}/${SenderResource}" to="${this.Jid(To)}" type="groupchat" id="m1"><body>${Text}</body>${Nick !== undefined ? `<nick xmlns="http://jabber.org/protocol/nick">${Nick}</nick>` : ""}</message>`;
    }
};

function Model(Uid: string, Resource = "V2:MissingGameServiceForAppId:WIN::00000000000000000000000000000000"){
    return new ChatClientModel({ LocalUid: Uid, Resource, LocalSocialName: NAMES[Uid as keyof typeof NAMES] });
}

describe("the client model reproduces what the first chat server showed (pull request #9)", () => {
    it("M1: as shipped, a player's own line shows its account id, UID-...", async () => {
        const Alpha = Model(A);

        assert.ok(Alpha.JoinPublicRoom(ROOM, "Alpha"));
        Alpha.Receive(PR9.SelfPresence(A));
        assert.equal(Alpha.RoomOf(ROOM)!.State, JOINED, "the join completes: the account id is in the nickname");
        Alpha.Receive(PR9.Message(A, A, "Ready?", "Alpha"));
        assert.deepEqual(await Alpha.ShownLines(Lookup), [`${A}: Ready?`]);
    });

    it("M2: a second player would have seen those lines as [unknown] (it never heard of the sender)", async () => {
        const Bravo = Model(B);

        Bravo.JoinPublicRoom(ROOM, "Bravo");
        Bravo.Receive(PR9.SelfPresence(B));
        assert.equal(Bravo.RoomOf(ROOM)!.State, JOINED);
        Bravo.Receive(PR9.Message(A, B, "Ready?", "Alpha"));
        assert.deepEqual(await Bravo.ShownLines(Lookup), ["[unknown]: Ready?"]);
    });

    it("M3: the username in place of the account id (also with status 210) leaves the join pending: 'Another operation already pending'", () => {
        for(const Extra of ["", `<status code="210"/>`]){
            const Alpha = Model(A);

            Alpha.JoinPublicRoom(ROOM, "Alpha");
            Alpha.Receive(PR9.SelfPresence(A, "Alpha", Extra));
            assert.equal(Alpha.RoomOf(ROOM)!.State, JOIN_PUBLIC_PENDING, "not recognised as its own presence");
            assert.equal(Alpha.JoinPublicRoom(ROOM, "Alpha"), undefined);
            assert.equal(Alpha.Log.at(-1), `MUC: JoinPublicRoom failed. Another operation already pending for room ${ROOM}`);
        }
    });

    it("M4: presence from the account id but the message from the username shows [unknown]", async () => {
        const Alpha = Model(A);

        Alpha.JoinPublicRoom(ROOM, "Alpha");
        Alpha.Receive(PR9.SelfPresence(A));
        Alpha.Receive(PR9.Message("Alpha", A, "Ready?"));
        assert.deepEqual(await Alpha.ShownLines(Lookup), ["[unknown]: Ready?"]);
    });

    it("M5: a message from a nickname with no presence first is shown as [unknown], not dropped", async () => {
        const Alpha = Model(A);

        Alpha.JoinPublicRoom(ROOM, "Alpha");
        Alpha.Receive(`<presence from="${ROOM}@${MUC}/${Alpha.Nickname("Alpha")}" to="x"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant" jid="${A}@${DOMAIN}/${Alpha.Resource}"/><status code="110"/></x></presence>`);
        Alpha.Receive(`<message type="groupchat" from="${ROOM}@${MUC}/Bravo:${B}:V2:X:WIN::1" to="x"><body>hi</body></message>`);
        assert.deepEqual(await Alpha.ShownLines(Lookup), ["[unknown]: hi"]);
        assert.ok(!Alpha.Log.some((Line) => Line.startsWith("could not get user id")));
    });

    it("M9: one member per account: a lingering old connection's leave after the new one's join turns that player's lines to [unknown]", async () => {
        const Res1 = "V2:MissingGameServiceForAppId:WIN::11111111111111111111111111111111";
        const Res2 = "V2:MissingGameServiceForAppId:WIN::22222222222222222222222222222222";
        const Occupant = (Resource: string, Unavailable = false) => `<presence${Unavailable ? ` type="unavailable"` : ""} from="${ROOM}@${MUC}/Bravo:${B}:${Resource}" to="x"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="${Unavailable ? "none" : "participant"}" jid="${B}@${DOMAIN}/${Resource}"/></x></presence>`;
        const Line = `<message type="groupchat" from="${ROOM}@${MUC}/Bravo:${B}:${Res2}" to="x"><body>back</body></message>`;
        const Joined = () => {
            const Alpha = Model(A);

            Alpha.JoinPublicRoom(ROOM, "Alpha");
            Alpha.Receive(`<presence from="${ROOM}@${MUC}/${Alpha.Nickname("Alpha")}" to="x"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant" jid="${A}@${DOMAIN}/${Alpha.Resource}"/><status code="110"/></x></presence>`);
            Alpha.Receive(Occupant(Res1));
            return Alpha;
        };

        // The new connection joins while the old one is still in the room, which leaves later
        const Late = Joined();
        Late.ReceiveAll([Occupant(Res2), Occupant(Res1, true), Line]);
        assert.deepEqual(await Late.ShownLines(Lookup), ["[unknown]: back"]);

        // The old one leaves first, then the new one joins
        const Replaced = Joined();
        Replaced.ReceiveAll([Occupant(Res1, true), Occupant(Res2), Line]);
        assert.deepEqual(await Replaced.ShownLines(Lookup), ["Bravo: back"]);
        assert.equal(Replaced.RoomOf(ROOM)!.Members.size, 2);
    });

    it("the client's URL encoding: UTF-8, unreserved characters kept, %XX in upper case", () => {
        assert.equal(UrlEncode("Alpha_1"), "Alpha_1");
        assert.equal(UrlEncode("Söme Name:1"), "S%C3%B6me%20Name%3A1");
        assert.equal(UrlDecode("S%c3%b6me%20Name%3A1"), "Söme Name:1");
    });
});

describe("our chat server, through the same model", () => {
    let Server: ChatServer;
    const Wires: WireClient[] = [];

    before(async () => {
        GetDb().insert(users).values([{ userId: A, name: "Alpha", notes: 0, isAdmin: false }, { userId: B, name: "Bravo", notes: 0, isAdmin: false }]).run();
        Server = new ChatServer({ AutoTick: false });
        await Server.listen(0);
    });

    // Each test logs its players out, so sessions never pile up across tests
    afterEach(async () => {
        for(const Wire of Wires.splice(0)) await Wire.Logout();
    });

    after(async () => {
        await Server.close();
        RemoveTestDb(() => GetDb().$client.close());
    });

    // A player: the model's own stanzas go over a real connection, and every reply is fed back to it
    async function Player(Uid: string){
        const Wire = await Login(Server.port, Uid);
        Wires.push(Wire);
        return { Wire, Model: Model(Uid, Wire.Resource) };
    }

    async function Settle(...Players: Array<{ Wire: WireClient, Model: ChatClientModel }>){
        for(const Each of Players){
            Each.Model.ReceiveAll(await Each.Wire.Barrier());
        }
    }

    function Send(Who: { Wire: WireClient }, Stanza: string | undefined){
        assert.ok(Stanza, "the model sent something");
        Who.Wire.Send(Stanza);
    }

    it("M6: both players see usernames; a leave completes and a rejoin is accepted", async () => {
        const Alpha = await Player(A);
        const Bravo = await Player(B);

        Send(Alpha, Alpha.Model.JoinPublicRoom(ROOM, "Alpha"));
        await Settle(Alpha);
        Send(Bravo, Bravo.Model.JoinPublicRoom(ROOM, "Bravo"));
        await Settle(Bravo, Alpha);
        assert.equal(Alpha.Model.RoomOf(ROOM)!.State, JOINED);
        assert.equal(Bravo.Model.RoomOf(ROOM)!.State, JOINED);

        Send(Alpha, Alpha.Model.RoomMessage(ROOM, "Ready?", "3F2504E04F8911D39A0C0305E82C3301"));
        await Settle(Alpha, Bravo);
        Send(Bravo, Bravo.Model.RoomMessage(ROOM, "Always ©", "3F2504E04F8911D39A0C0305E82C3302"));
        await Settle(Bravo, Alpha);

        assert.deepEqual(await Alpha.Model.ShownLines(Lookup), ["Alpha: Ready?", "Bravo: Always ©"]);
        assert.deepEqual(await Bravo.Model.ShownLines(Lookup), ["Alpha: Ready?", "Bravo: Always ©"]);

        Send(Bravo, Bravo.Model.ExitRoom(ROOM));
        assert.equal(Bravo.Model.RoomOf(ROOM)!.State, EXIT_PENDING);
        await Settle(Bravo, Alpha);
        assert.equal(Bravo.Model.RoomOf(ROOM)!.State, NOT_JOINED, "the leave completes");
        assert.deepEqual(Bravo.Model.ExitResults, [ROOM]);
        assert.equal(Alpha.Model.RoomOf(ROOM)!.Members.size, 1, "Alpha saw Bravo leave");

        Send(Bravo, Bravo.Model.JoinPublicRoom(ROOM, "Bravo"));
        await Settle(Bravo, Alpha);
        assert.equal(Bravo.Model.RoomOf(ROOM)!.State, JOINED, "the rejoin is accepted");
        assert.ok(!Bravo.Model.Log.some((Line) => Line.includes("already pending")));
    });

    it("M7: a refused join drops the room cleanly, and the next join is allowed", async () => {
        const Alpha = await Player(A);

        Send(Alpha, Alpha.Model.JoinPublicRoom(ROOM, "Bravo")); // a name that is not this account's
        await Settle(Alpha);
        assert.equal(Alpha.Model.RoomOf(ROOM), undefined, "the room is gone");
        assert.deepEqual(Alpha.Model.JoinResults, [{ Room: ROOM, Ok: false }]);

        Send(Alpha, Alpha.Model.JoinPublicRoom(ROOM, "Alpha"));
        await Settle(Alpha);
        assert.equal(Alpha.Model.RoomOf(ROOM)!.State, JOINED);
        assert.ok(!Alpha.Model.Log.some((Line) => Line.includes("already pending")));
    });

    it("M8: a player whose own account read failed (InvalidMCPUser) is still shown by username to others", async () => {
        const Alpha = await Player(A);
        const Bravo = await Player(B);
        const Room = "Hunt-7c3e0a52-1111-4a4a-9c9c-000000000001";

        Send(Alpha, Alpha.Model.JoinPublicRoom(Room, "InvalidMCPUser"));
        await Settle(Alpha);
        Send(Bravo, Bravo.Model.JoinPublicRoom(Room, "Bravo"));
        await Settle(Bravo, Alpha);
        Send(Alpha, Alpha.Model.RoomMessage(Room, "hello", "g1"));
        await Settle(Alpha, Bravo);

        assert.deepEqual(await Bravo.Model.ShownLines(Lookup), ["Alpha: hello"]);
    });

    it("whispers reach the other player with a full sender JID, shown by username", async () => {
        const Alpha = await Player(A);
        const Bravo = await Player(B);

        Send(Alpha, Alpha.Model.WhisperTo(B, "psst"));
        await Settle(Alpha, Bravo);
        assert.deepEqual(await Bravo.Model.ShownWhispers(Lookup), ["Alpha: psst"]);
    });
});
