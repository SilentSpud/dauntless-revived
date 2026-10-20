import { RemoveTestDb } from "./setup";
import "./authenv";
import { WithEnv } from "./appenv";
import { after, afterEach, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { Call, Reply, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { slayerlinkinvites, slayerlinks, users } from "../src/db/schema";
import { INVITE_EXPIRY_HOURS, INVITE_WINDOW_MS, LINK_DURATION_HOURS, MAX_INVITES_PER_WINDOW, SetSlayerLinkClockForTests } from "../src/controllers/slayerlinks";
import { Count } from "./helpers";

// Slayer Links over HTTP, with the bodies and paths the 1.4.4 client sends (controllers/slayerlinks.ts has
// the exe addresses). Harmonic's fork (github.com/Harmonicrain/Undaunted 895f7c7) had the first working
// contract; the fork's case is ported below with the client's own shapes where they differed: invites list the
// other player as account_id (not linked_account_id), an answer names the other player in account_id,
// a link is removed with DELETE /slayerlink/links, and DELETE /slayerlink/invites/{account_id} exists.
// Slots are 1 to 3, the keys of the client's slot map (0x1415ea22d; see the controller).

const HOUR = 60 * 60 * 1000;
let Counter = 0;

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

afterEach(() => {
    SetSlayerLinkClockForTests();
    delete process.env.SLAYER_LINKS;
});

function Player(Name: string){
    const UserId = `UID-link-${++Counter}-${Name}`;

    GetDb().insert(users).values({ userId: UserId, name: `${Name}${Counter}`, notes: 0, isAdmin: false }).run();
    return UserId;
}

// Friends the way the client makes them: a request, then the other side's request accepts it
async function Befriend(A: string, B: string){
    assert.equal((await Call("POST", `/friends/api/public/friends/${A}/${B}`, { as: A })).status, 204);
    assert.equal((await Call("POST", `/friends/api/public/friends/${B}/${A}`, { as: B })).status, 204);
}

const Invite = (From: string, To: string, Slot: unknown) => Call("PUT", "/slayerlink/invite", { as: From, body: { account_id: To, slot: Slot, action_source: "social_panel" } });
const Answer = (By: string, Other: string, Action: string, Slot = 1) => Call("POST", "/slayerlink/invite", { as: By, body: { account_id: Other, action: Action, slot: Slot, action_source: "social_panel" } });
const Invites = async (Of: string) => (await Call("GET", "/slayerlink/invites", { as: Of })).json.payload.invites as any[];
const Links = async (Of: string) => (await Call("GET", "/slayerlink/links", { as: Of })).json.payload.links as any[];
const Available = async (Of: string, Ids: string[]) => (await Call("POST", "/slayerlink/availability", { as: Of, body: { account_ids: Ids } })).json.payload.availability as any[];
const RemoveLink = (By: string, Other: string | undefined, Slot: number | undefined) => Call("DELETE", "/slayerlink/links", { as: By, body: { account_id: Other, slot: Slot, delete_pair: true } });

function AssertOk(Reply: Reply, Message?: string){
    assert.equal(Reply.status, 200, `${Message ?? ""} ${Reply.text}`);
    assert.equal(Reply.json.code, null);
    assert.equal(Reply.json.message, "OK");
    return Reply.json.payload;
}

function AssertRefused(Reply: Reply, Status: number, Message?: string){
    assert.equal(Reply.status, Status, `${Message ?? ""} ${Reply.text}`);
    assert.deepEqual(Object.keys(Reply.json), ["code", "message", "payload"]);
    assert.equal(Reply.json.code, String(Status));
    assert.equal(typeof Reply.json.message, "string");
    assert.equal(Reply.json.payload, null);
}

describe("Slayer Links", () => {
    // from Harmonicrain/Undaunted test/social-friends.test.js:76, rewritten to the client's paths and bodies
    it("a friend is invited, accepts, both see the link, outsiders cannot touch it, and it is removed for both", async () => {
        const John = Player("John"), Manda = Player("Manda"), Todd = Player("Todd");

        AssertRefused(await Invite(John, Manda, 1), 403, "not friends yet");
        assert.deepEqual(await Available(John, [Manda]), [{ account_id: Manda, available: false }]);

        await Befriend(John, Manda);
        assert.deepEqual(await Available(John, [Manda, Todd, John]), [
            { account_id: Manda, available: true },
            { account_id: Todd, available: false },
            { account_id: John, available: false }
        ]);

        const Id = AssertOk(await Invite(John, Manda, 1)).link_id;
        assert.match(Id, /^[0-9a-f-]{36}$/);
        assert.equal(AssertOk(await Invite(John, Manda, 1)).link_id, Id, "inviting again answers the same invite");
        assert.deepEqual(await Available(John, [Manda]), [{ account_id: Manda, available: false }], "an invite is already waiting");

        const Received = await Invites(Manda);
        assert.equal(Received.length, 1);
        assert.deepEqual(Object.keys(Received[0]), ["account_id", "slot", "direction", "status", "expires", "link_id"]);
        assert.deepEqual({ ...Received[0], expires: undefined }, { account_id: John, slot: 1, direction: "Received", status: "Pending", expires: undefined, link_id: Id });
        assert.ok(Math.abs(Date.parse(Received[0].expires) - (Date.now() + INVITE_EXPIRY_HOURS * HOUR)) < 60 * 1000, Received[0].expires);
        assert.deepEqual((await Invites(John)).map((Entry) => [Entry.account_id, Entry.direction]), [[Manda, "Sent"]]);

        AssertRefused(await Answer(Todd, John, "accept"), 404, "an outsider has no invite from John");
        AssertRefused(await Call("POST", "/slayerlink/invite", { as: Todd, body: { link_id: Id, action: "accept" } }), 404, "nor by the invite's id");

        AssertOk(await Answer(Manda, John, "accept", 3));
        assert.equal(AssertOk(await Answer(Manda, John, "accept", 3)).link_id, Id, "a repeated accept answers the same link");

        const OfJohn = await Links(John);
        const OfManda = await Links(Manda);
        assert.equal(OfJohn.length, 1);
        assert.deepEqual(Object.keys(OfJohn[0]), ["account_id", "linked_account_id", "slot", "ends", "link_id", "prize_pool"]);
        assert.deepEqual({ ...OfJohn[0], ends: undefined }, { account_id: Manda, linked_account_id: Manda, slot: 1, ends: undefined, link_id: Id, prize_pool: [] });
        assert.deepEqual({ ...OfManda[0], ends: undefined }, { account_id: John, linked_account_id: John, slot: 3, ends: undefined, link_id: Id, prize_pool: [] }, "the slot Manda chose, the client's third");
        assert.ok(Math.abs(Date.parse(OfJohn[0].ends) - (Date.now() + LINK_DURATION_HOURS * HOUR)) < 60 * 1000, OfJohn[0].ends);
        assert.equal(Count("slayerlinks"), 1);
        assert.deepEqual(await Invites(Manda), []);

        AssertRefused(await Invite(John, Manda, 2), 409, "already linked");

        const Status = AssertOk(await Call("GET", "/slayerlink/status_good", { as: John }));
        assert.deepEqual(Object.keys(Status), ["invites", "links", "config"]);
        assert.deepEqual(Status.config, { link_duration_hours: 168, invite_expiry_hours: 24 });
        assert.deepEqual(Status.links, OfJohn);
        assert.deepEqual(Status.invites, []);

        AssertOk(await RemoveLink(Todd, John, 1), "someone without that link");
        assert.equal((await Links(John)).length, 1, "Todd removed nothing");

        AssertOk(await RemoveLink(John, Manda, 1));
        assert.deepEqual(await Links(John), []);
        assert.deepEqual(await Links(Manda), [], "a link ends for both players");
        AssertOk(await RemoveLink(John, Manda, 1), "removing it again is still 200");
    });

    it("reject and cancel are resolved by the other player's account id", async () => {
        const Ann = Player("Ann"), Ben = Player("Ben");

        await Befriend(Ann, Ben);

        AssertOk(await Invite(Ann, Ben, 2));
        AssertRefused(await Answer(Ben, Ann, "cancel"), 404, "only the sender cancels");
        AssertOk(await Answer(Ben, Ann, "reject"));
        AssertOk(await Answer(Ben, Ann, "reject"), "a repeated reject");
        AssertRefused(await Answer(Ben, Ann, "accept"), 409, "the invite was declined");
        assert.deepEqual(await Invites(Ann), []);
        assert.deepEqual(await Invites(Ben), []);

        const Second = AssertOk(await Invite(Ann, Ben, 2)).link_id;
        AssertOk(await Answer(Ann, Ben, "cancel"));
        assert.deepEqual(await Invites(Ben), []);
        assert.equal(GetDb().select().from(slayerlinkinvites).where(eq(slayerlinkinvites.inviteId, Second)).get()?.status, "CANCELED");

        AssertRefused(await Call("POST", "/slayerlink/invite", { as: Ben, body: { account_id: Ann, action: "decline" } }), 400, "the client sends reject");
        AssertRefused(await Call("POST", "/slayerlink/invite", { as: Ben, body: { action: "accept" } }), 404, "no account, no invite");
        assert.equal(Count("slayerlinks"), 0);
    });

    it("DELETE /slayerlink/invites/{account_id}: another player's id clears the invites between the two, the caller's own id all of them", async () => {
        const Ivy = Player("Ivy"), Jon = Player("Jon"), Kim = Player("Kim"), Lea = Player("Lea");

        await Befriend(Ivy, Jon);
        await Befriend(Ivy, Kim);
        await Befriend(Lea, Ivy);
        AssertOk(await Invite(Ivy, Jon, 1));
        AssertOk(await Invite(Ivy, Kim, 2));
        AssertOk(await Invite(Lea, Ivy, 1));

        AssertOk(await Call("DELETE", `/slayerlink/invites/${Jon}`, { as: Ivy }));
        assert.deepEqual((await Invites(Ivy)).map((Entry) => Entry.account_id).sort(), [Kim, Lea].sort());
        assert.deepEqual(await Invites(Jon), []);

        AssertOk(await Call("DELETE", `/slayerlink/invites/${Ivy}`, { as: Ivy }));
        assert.deepEqual(await Invites(Ivy), []);
        assert.deepEqual(await Invites(Lea), []);
        assert.deepEqual(GetDb().select({ status: slayerlinkinvites.status }).from(slayerlinkinvites).where(eq(slayerlinkinvites.senderId, Lea)).all(), [{ status: "DECLINED" }]);
        AssertRefused(await Call("DELETE", "/slayerlink/invites/not%20an%20id", { as: Ivy }), 400);
    });

    it("three slots each, numbered 1 to 3 like the client's, one waiting invite per slot, and the slot and self checks", async () => {
        const Max = Player("Max"), Friends = [Player("Fa"), Player("Fb"), Player("Fc"), Player("Fd")];

        for(const Friend of Friends){
            await Befriend(Max, Friend);
        }

        for(const Bad of [0, 4, -1, "0", "x", 1.5, null]){
            AssertRefused(await Invite(Max, Friends[0], Bad), 400, `slot ${JSON.stringify(Bad)}`);
        }

        AssertRefused(await Invite(Max, Max, 1), 409, "yourself");
        AssertRefused(await Invite(Max, "UID-link-nobody", 1), 404);

        AssertOk(await Invite(Max, Friends[0], 1));
        AssertRefused(await Invite(Max, Friends[1], 1), 409, "slot 1 already has an invite waiting");
        AssertRefused(await Invite(Friends[0], Max, 2), 409, "the other way round while Max's invite waits");

        AssertOk(await Answer(Friends[0], Max, "accept"));
        AssertOk(await Invite(Max, Friends[1], "2"), "a numeric string is a slot too");
        AssertOk(await Answer(Friends[1], Max, "accept"));
        AssertOk(await Invite(Max, Friends[2], 3), "the client's third slot");
        AssertOk(await Answer(Friends[2], Max, "accept"));

        assert.deepEqual((await Links(Max)).map((Link) => Link.slot), [1, 2, 3]);
        assert.deepEqual(AssertOk(await Call("GET", "/slayerlink/status_good", { as: Max })).links.map((Link: any) => Link.slot), [1, 2, 3]);
        assert.deepEqual(await Available(Max, [Friends[3]]), [{ account_id: Friends[3], available: false }], "no free slot");
        AssertRefused(await Invite(Max, Friends[3], 1), 409, "the slot is linked");
        AssertRefused(await Invite(Friends[3], Max, 1), 409, "Max has no free slot");
        assert.deepEqual(await Available(Friends[3], [Max]), [{ account_id: Max, available: false }]);

        // The accepting player's chosen slot is used when it is free, otherwise the first free one; a slot
        // the client does not have (0) is never stored
        await Befriend(Friends[3], Friends[0]);
        AssertOk(await Invite(Friends[3], Friends[0], 1));
        AssertOk(await Answer(Friends[0], Friends[3], "accept", 1));
        assert.deepEqual((await Links(Friends[0])).map((Link) => [Link.account_id, Link.slot]), [[Max, 1], [Friends[3], 2]]);
        await Befriend(Friends[3], Friends[1]);
        AssertOk(await Invite(Friends[3], Friends[1], 2));
        AssertOk(await Answer(Friends[1], Friends[3], "accept", 0));
        assert.deepEqual((await Links(Friends[1])).map((Link) => [Link.account_id, Link.slot]), [[Max, 1], [Friends[3], 2]]);
        assert.equal(Count("slayerlinks", "senderSlot not between 1 and 3 or targetSlot not between 1 and 3"), 0);
    });

    it("an invite runs out after 24 hours and a link after a week", async () => {
        const Ola = Player("Ola"), Pia = Player("Pia");
        let Now = Date.now();

        SetSlayerLinkClockForTests(() => Now);
        await Befriend(Ola, Pia);

        AssertOk(await Invite(Ola, Pia, 1));
        Now += INVITE_EXPIRY_HOURS * HOUR + 1;
        assert.deepEqual(await Invites(Pia), []);
        AssertRefused(await Answer(Pia, Ola, "accept"), 409, "it ran out");
        assert.deepEqual(GetDb().select({ status: slayerlinkinvites.status }).from(slayerlinkinvites).where(eq(slayerlinkinvites.senderId, Ola)).all(), [{ status: "EXPIRED" }]);

        const Fresh = AssertOk(await Invite(Ola, Pia, 1), "a new invite after the old one ran out").link_id;
        AssertOk(await Answer(Pia, Ola, "accept"));
        assert.equal((await Links(Ola))[0].link_id, Fresh);

        Now += LINK_DURATION_HOURS * HOUR - 1;
        assert.equal((await Links(Ola)).length, 1, "a week less a moment");
        Now += 2;
        assert.deepEqual(await Links(Ola), []);
        assert.deepEqual(await Links(Pia), []);
        AssertOk(await Invite(Ola, Pia, 1), "the slot and the pair are free again");
    });

    it("an unfriend or a block cancels the waiting invites; a running link stays until it ends", async () => {
        const Rex = Player("Rex"), Sam = Player("Sam"), Tia = Player("Tia");

        await Befriend(Rex, Sam);
        await Befriend(Rex, Tia);
        AssertOk(await Invite(Rex, Sam, 1));
        AssertOk(await Invite(Tia, Rex, 1));
        AssertOk(await Answer(Rex, Tia, "accept", 2));

        assert.equal((await Call("DELETE", `/friends/api/public/friends/${Rex}/${Sam}`, { as: Rex })).status, 204);
        assert.deepEqual(await Invites(Sam), [], "the unfriend cancelled Rex's invite");
        assert.equal(GetDb().select().from(slayerlinkinvites).where(eq(slayerlinkinvites.senderId, Rex)).all().filter((Row) => Row.targetId === Sam)[0].status, "CANCELED");
        AssertRefused(await Answer(Sam, Rex, "accept"), 409);
        AssertRefused(await Invite(Rex, Sam, 1), 403, "no longer friends");

        assert.equal((await Call("POST", `/friends/api/public/blocklist/${Tia}/${Rex}`, { as: Tia })).status, 204);
        assert.equal((await Links(Rex)).length, 1, "the running link stays (ending it is the owner's decision)");
        AssertRefused(await Invite(Rex, Tia, 1), 403, "blocked");
        assert.deepEqual(await Available(Rex, [Tia, Sam]), [{ account_id: Tia, available: false }, { account_id: Sam, available: false }]);

        // A waiting invite between players who then block each other is cancelled too
        const Uma = Player("Uma");
        await Befriend(Uma, Sam);
        AssertOk(await Invite(Uma, Sam, 1));
        assert.equal((await Call("POST", `/friends/api/public/blocklist/${Sam}/${Uma}`, { as: Sam })).status, 204);
        assert.deepEqual(await Invites(Uma), []);
        assert.equal(Count("slayerlinkinvites", "senderId = ? and status = 'CANCELED'", Uma), 1);
    });

    it("a player sends at most 20 new invites in 10 minutes; declined and cancelled invites go after their day", async () => {
        const Nia = Player("Nia"), Oto = Player("Oto");
        let Now = Date.now();

        SetSlayerLinkClockForTests(() => Now);
        await Befriend(Nia, Oto);

        for(let Round = 0; Round < MAX_INVITES_PER_WINDOW; Round++){
            const Id = AssertOk(await Invite(Nia, Oto, 1), `invite ${Round + 1}`).link_id;
            assert.equal(AssertOk(await Invite(Nia, Oto, 1)).link_id, Id, "the same invite again is not a new one");
            AssertOk(await Answer(Nia, Oto, "cancel"));
            Now += 1000;
        }

        AssertRefused(await Invite(Nia, Oto, 1), 409, "the 21st new invite in 10 minutes");
        assert.equal(Count("slayerlinkinvites", "senderId = ?", Nia), MAX_INVITES_PER_WINDOW, "the refused one is not stored");
        AssertOk(await Invite(Oto, Nia, 1), "the limit is per sender");
        AssertOk(await Answer(Nia, Oto, "reject"));

        Now += INVITE_WINDOW_MS;
        AssertOk(await Invite(Nia, Oto, 1), "the window has moved on");
        assert.equal(Count("slayerlinkinvites", "status in ('CANCELED', 'DECLINED') and ? in (senderId, targetId)", Nia), MAX_INVITES_PER_WINDOW + 1, "kept while their day lasts");

        Now += INVITE_EXPIRY_HOURS * HOUR;
        const Fresh = AssertOk(await Invite(Oto, Nia, 2), "the next new invite sweeps").link_id;
        assert.deepEqual(GetDb().$client.prepare("select inviteId, status from slayerlinkinvites where ? in (senderId, targetId)").all(Nia), [{ inviteId: Fresh, status: "PENDING" }],
            "declined, cancelled and run-out invites are gone once their day is over");
    });

    it("needs a player's token: a game server's key alone is 403 and no auth is 401", async () => {
        const Routes: [string, string, unknown][] = [
            ["GET", "/slayerlink/status_good", undefined], ["GET", "/slayerlink/invites", undefined], ["GET", "/slayerlink/links", undefined],
            ["PUT", "/slayerlink/invite", { account_id: "UID-x", slot: 1 }], ["POST", "/slayerlink/invite", { account_id: "UID-x", action: "accept" }],
            ["DELETE", "/slayerlink/invites/UID-x", undefined], ["DELETE", "/slayerlink/links", { slot: 1 }], ["POST", "/slayerlink/availability", { account_ids: [] }]
        ];

        for(const [Method, Path, Body] of Routes){
            assert.equal((await Call(Method, Path, { gs: true, body: Body })).status, 403, `${Method} ${Path} with the game server's key`);
            assert.equal((await Call(Method, Path, { body: Body })).status, 401, `${Method} ${Path} without auth`);
        }
    });

    it("SLAYER_LINKS=0 answers every route with the old 404 and keeps the data", async () => {
        const Val = Player("Val"), Wes = Player("Wes");

        await Befriend(Val, Wes);
        AssertOk(await Invite(Val, Wes, 1));
        AssertOk(await Answer(Wes, Val, "accept"));

        await WithEnv({ SLAYER_LINKS: "0" }, async () => {
            for(const [Method, Path, Body] of [
                ["GET", "/slayerlink/status_good", undefined], ["GET", "/slayerlink/invites", undefined], ["GET", "/slayerlink/links", undefined],
                ["PUT", "/slayerlink/invite", { account_id: Wes, slot: 2 }], ["POST", "/slayerlink/invite", { account_id: Wes, action: "cancel" }],
                ["DELETE", `/slayerlink/invites/${Wes}`, undefined], ["DELETE", "/slayerlink/links", { account_id: Wes, slot: 1 }],
                ["POST", "/slayerlink/availability", { account_ids: [Wes] }]
            ] as [string, string, unknown][]){
                const Reply = await Call(Method, Path, { as: Val, body: Body });
                assert.deepEqual([Reply.status, Reply.text], [404, ""], `${Method} ${Path}`);
            }
        });

        assert.equal((await Links(Val)).length, 1, "the link is still there once the switch is back on");
        assert.equal((await Call("GET", "/slayerlink/links/rewards/UID-x/1", { as: Val })).status, 404, "the reward routes are not answered");
    });
});
