import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, before, describe, it } from "node:test";
import assert from "node:assert/strict";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { GetBountyReply, SetBounties } from "../src/controllers/bounties";
import { MakePlayer } from "./helpers";

// Harmonic's bounty and cooldown cases (github.com/Harmonicrain/Undaunted 895f7c7,
// test/huntpass-progress.test.js), run against ours: the board is stored per bounty with its
// update_version, and only a game server writes bounties and cooldowns (the fork let a player's token write
// them). Each case names the line of the original test. Inverted: 409 (a post with an empty board and zeroed draft
// data keeps the bounties players still hold; the fork cleared them) and 443 (we serve no bounty definitions: an
// id without one is eligible, and the fork's list would cut the Gold reward from 100 to 60 Hunt Pass XP). Skipped:
// 465 (the fields of a definition list we do not serve).

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

const Save = (UserId: string, Body: unknown) => SetBounties(UserId, Body, "gameserver");
const Board = (UserId: string) => GetBountyReply(UserId).payload;
const Draft = (Previous: string[], Counts: [number, number, number], Choices: string[] = []) => ({ current_draft_choices: Choices, previous_draft_selections: Previous, bronze_count: Counts[0], silver_count: Counts[1], gold_count: Counts[2] });

describe("the bounty board (test/huntpass-progress.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:226
    it("bounties persist across a read", async () => {
        const { UserId } = await MakePlayer();
        assert.deepEqual(Board(UserId).bounties, []);

        Save(UserId, { bounties: [{ bounty_id: "BountyA", progress: 3 }], draft_data: Draft(["X"], [1, 0, 0]) });

        const Stored = Board(UserId);
        assert.equal(Stored.bounties.length, 1);
        assert.equal(Stored.bounties[0].bounty_id, "BountyA");
        assert.equal(Stored.draft_data.bronze_count, 1);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:243
    it("drafting several bounties keeps all of them", async () => {
        const { UserId } = await MakePlayer();
        const Drafted = (Id: string, Slot: number, Previous: string[]) => Save(UserId, {
            bounties: [{ bounty_id: Id, slot_index: Slot, objectives: [{ objective_id: Id, progress: 0 }], update_version: 0 }],
            draft_data: Draft(Previous, [7, 2, 1])
        });

        Drafted("Bounty_Bronze_KillAxePike", 0, ["Bounty_Bronze_KillAxePike"]);
        Drafted("Bounty_Bronze_HuntsSwordTwo", 1, ["Bounty_Bronze_KillAxePike", "Bounty_Bronze_HuntsSwordTwo"]);
        Drafted("Bounty_Silver_HuntsSwordTwo", 2, ["Bounty_Bronze_KillAxePike", "Bounty_Bronze_HuntsSwordTwo", "Bounty_Silver_HuntsSwordTwo"]);

        const Stored = Board(UserId);
        assert.equal(Stored.bounties.length, 3, "all three drafted bounties survive");
        assert.deepEqual(Stored.bounties.map((Entry: any) => Entry.slot_index).sort(), [0, 1, 2]);
        assert.equal(Stored.draft_data.previous_draft_selections.length, 3, "draft_data is the full state and replaces");
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:265
    it("a refresh of the draft options does not wipe the held board", async () => {
        const { UserId } = await MakePlayer();
        Save(UserId, { bounties: [{ bounty_id: "Held", slot_index: 0 }], draft_data: Draft(["Held"], [7, 3, 1]) });

        // Between drafts the game server posts an empty bounties list with its new draft choices
        Save(UserId, { bounties: [], draft_data: Draft(["Held"], [6, 3, 1], ["A", "B", "C"]) });

        const Stored = Board(UserId);
        assert.equal(Stored.bounties.length, 1, "the held bounty survives a draft refresh");
        assert.deepEqual(Stored.draft_data.current_draft_choices, ["A", "B", "C"]);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:286
    it("posting a bounty again updates it in place", async () => {
        const { UserId } = await MakePlayer();
        Save(UserId, { bounties: [{ bounty_id: "Tracked", slot_index: 0, objectives: [{ objective_id: "Tracked", progress: 0 }] }] });
        Save(UserId, { bounties: [{ bounty_id: "Tracked", slot_index: 0, objectives: [{ objective_id: "Tracked", progress: 5 }] }] });

        const Stored = Board(UserId);
        assert.equal(Stored.bounties.length, 1, "progress updates must not duplicate the bounty");
        assert.equal(Stored.bounties[0].objectives[0].progress, 5);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:339, adapted: our envelope's code is "OK"
    it("the board is served inside the code/message/payload envelope", async () => {
        const { UserId } = await MakePlayer();
        Save(UserId, { bounties: [{ bounty_id: "Wire", slot_index: 0 }], draft_data: Draft(["Wire"], [7, 3, 1]) });

        const Reply = await Call("GET", `/bounty/${UserId}`, { as: UserId });
        assert.equal(Reply.status, 200);
        assert.equal(Reply.json.message, "OK");
        assert.ok(Reply.json.payload, "the board sits inside payload");
        assert.equal(Reply.json.bounties, undefined, "and not also at the root");
        assert.equal(Reply.json.payload.bounties[0].bounty_id, "Wire");
        assert.equal(Reply.json.payload.draft_data.bronze_count, 7);

        // Neither key occurs in the 1.4.4 executable
        assert.equal(Reply.json.payload.draft_data_daily, undefined);
        assert.equal(Reply.json.payload.draft_data_weekly, undefined);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:367, adapted: only a game server abandons
    it("abandoning a bounty removes it instead of failing", async () => {
        const { UserId } = await MakePlayer();
        Save(UserId, { bounties: [{ bounty_id: "Keep", slot_index: 0 }, { bounty_id: "Abandon", slot_index: 1 }] });

        assert.equal((await Call("POST", `/bounty/delete/${UserId}`, { as: UserId, body: { bounty_ids: ["Abandon"] } })).status, 403, "not a player's own client");

        const Reply = await Call("POST", `/bounty/delete/${UserId}`, { gs: true, as: UserId, body: { bounty_ids: ["Abandon"] } });
        assert.equal(Reply.status, 200);
        assert.deepEqual(Board(UserId).bounties.map((Entry: any) => Entry.bounty_id), ["Keep"]);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:394
    it("abandoning an unknown bounty is harmless", async () => {
        const { UserId } = await MakePlayer();
        Save(UserId, { bounties: [{ bounty_id: "Held", slot_index: 0 }] });

        assert.equal((await Call("POST", `/bounty/delete/${UserId}`, { gs: true, body: { bounty_ids: ["NeverHeld"] } })).status, 200);
        assert.equal(Board(UserId).bounties.length, 1);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:409, inverted: the post with an empty board
    // and zeroed draft data is what the game server sends when a new bounty season starts (ResetDraftData,
    // 0x1413f0f60), while players still hold bounties; ours keeps them and takes the new draft data
    it("a post with an empty board and zeroed draft data keeps the bounties held", async () => {
        const { UserId } = await MakePlayer();
        Save(UserId, { bounties: [{ bounty_id: "Held_A", slot_index: 2 }, { bounty_id: "Held_B", slot_index: 3 }], draft_data: Draft(["Held_A", "Held_B"], [7, 2, 1]) });

        Save(UserId, { bounties: [], draft_data: Draft([], [0, 0, 0]) });

        const Stored = Board(UserId);
        assert.deepEqual(Stored.bounties.map((Entry: any) => Entry.bounty_id), ["Held_A", "Held_B"]);
        assert.deepEqual(Stored.draft_data, Draft([], [0, 0, 0]));

        // Claims and abandons go through /bounty/delete
        Save(UserId, { bounties: [{ bounty_id: "Fresh", slot_index: 0 }] });
        assert.deepEqual(Board(UserId).bounties.map((Entry: any) => Entry.bounty_id), ["Fresh", "Held_A", "Held_B"]);
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:443, inverted: no definitions are served. An
    // id without a definition is eligible (0x1413f5a81), and its reward falls back to the bounty table's
    // HuntPassXPReward (0x1413df5e1)
    it("the bounty game data serves no definitions and keeps its settings", async () => {
        const { UserId } = await MakePlayer();
        const Reply = await Call("GET", "/bounty/game-data", { as: UserId });

        assert.equal(Reply.status, 200);
        assert.deepEqual(Reply.json.payload.bounty_data, []);
        assert.deepEqual([Reply.json.payload.max_slots, Reply.json.payload.bounty_token_id, Reply.json.payload.premium_bounty_token_id, Reply.json.payload.new_season_reset_bounties], [4, "TOKEN_BOUNTY_DRAFT", "TOKEN_BOUNTY_DRAFT_PREMIUM", false]);
    });
});

describe("cooldowns (test/huntpass-progress.test.js)", () => {
    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:488, adapted: the game server's key and PUT
    it("cooldowns persist, so the bounty-token marker survives a login", async () => {
        const { UserId } = await MakePlayer();

        const Put = await Call("PUT", `/cooldown/batch/${UserId}`, { gs: true, as: UserId, body: { cooldowns: [{ cooldown_id: "BountyTokens", cooldown_started_date: "2026-09-21T13:38:13.654Z" }] } });
        assert.equal(Put.status, 200);

        // GET is an id -> date map, not the {cooldowns: [...]} of the batch write
        const Got = await Call("GET", `/cooldown/${UserId}`, { as: UserId });
        assert.equal(Got.json.message, "OK");
        assert.deepEqual(Got.json.payload, { BountyTokens: "2026-09-21T13:38:13.654Z" });
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:510
    it("a batch updates the cooldowns it names and keeps the rest", async () => {
        const { UserId } = await MakePlayer();
        const Batch = (Entries: unknown[]) => Call("PUT", `/cooldown/batch/${UserId}`, { gs: true, body: { cooldowns: Entries } });

        await Batch([{ cooldown_id: "A", cooldown_started_date: "2026-01-01T00:00:00.000Z" }, { cooldown_id: "B", cooldown_started_date: "2026-01-01T00:00:00.000Z" }]);
        await Batch([{ cooldown_id: "A", cooldown_started_date: "2026-06-01T00:00:00.000Z" }]);

        const ById = (await Call("GET", `/cooldown/${UserId}`, { as: UserId })).json.payload;
        assert.equal(ById.A, "2026-06-01T00:00:00.000Z", "a named cooldown is updated");
        assert.equal(ById.B, "2026-01-01T00:00:00.000Z", "an unnamed cooldown is kept");
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:527
    it("the batch route is not taken by the start-cooldown route", async () => {
        const { UserId } = await MakePlayer();

        await Call("PUT", `/cooldown/batch/${UserId}`, { gs: true, body: { cooldowns: [{ cooldown_id: "Real", cooldown_started_date: "2026-01-01T00:00:00.000Z" }] } });

        assert.deepEqual(Object.keys((await Call("GET", `/cooldown/${UserId}`, { as: UserId })).json.payload), ["Real"]);
        assert.deepEqual((await Call("GET", "/cooldown/batch", { gs: true })).json.payload, {}, "nothing stored under an account called batch");
    });

    // from Harmonicrain/Undaunted test/huntpass-progress.test.js:544, adapted: the game server's PUT (the
    // client never sends the fork's POST), and a player's own client may not start its cooldowns
    it("starting a cooldown by id records it now", async () => {
        const { UserId } = await MakePlayer();

        assert.equal((await Call("PUT", `/cooldown/${UserId}/DailyThing`, { as: UserId })).status, 403);
        assert.equal((await Call("POST", `/cooldown/${UserId}/DailyThing`, { gs: true })).status, 404, "no POST");

        const Before = Date.now();
        assert.equal((await Call("PUT", `/cooldown/${UserId}/DailyThing`, { gs: true })).status, 200);

        const Got = (await Call("GET", `/cooldown/${UserId}`, { as: UserId })).json.payload;
        assert.deepEqual(Object.keys(Got), ["DailyThing"]);
        assert.ok(Date.parse(Got.DailyThing) >= Before - 1000);
    });
});
