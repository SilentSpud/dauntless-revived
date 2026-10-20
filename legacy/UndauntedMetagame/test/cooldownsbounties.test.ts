import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { GetCooldownReply, SetCooldownBatch, StartCooldown } from "../src/controllers/cooldowns";
import { DeleteBounties, GetBountyReply, SetBounties } from "../src/controllers/bounties";
import { MakePlayer } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

describe("cooldowns", () => {
    it("the batch PUT is stored exactly as sent and read back as an object map with an int code", async () => {
        const {UserId} = await MakePlayer();

        assert.deepEqual(GetCooldownReply(UserId), {code: 200, message: "OK", payload: {}});
        assert.equal(SetCooldownBatch(UserId, {cooldowns: [{cooldown_id: "bounty_tokens_season09b", cooldown_started_date: "2026-09-21T14:10:39.154Z"}]}, "gameserver").Status, 200);
        SetCooldownBatch(UserId, {cooldowns: [{cooldown_id: "bounty_tokens_season09b", cooldown_started_date: "2026-09-22T08:00:00.000Z"}, {cooldown_id: "arena_rank_1", cooldown_started_date: "2026-09-21T15:00:00.000Z"}]}, "gameserver");

        assert.deepEqual(GetCooldownReply(UserId).payload, {"bounty_tokens_season09b": "2026-09-22T08:00:00.000Z", "arena_rank_1": "2026-09-21T15:00:00.000Z"});
        assert.equal(SetCooldownBatch(UserId, {nothing: 1}, "gameserver").Status, 400);
    });

    it("a start PUT (no body) stamps now; old harvest cooldowns are pruned, bounty tokens never", async () => {
        const {UserId} = await MakePlayer();
        const Old = new Date(Date.now() - 3 * 24 * 3600 * 1000).toISOString();
        SetCooldownBatch(UserId, {cooldowns: [{cooldown_id: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", cooldown_started_date: Old}, {cooldown_id: "bounty_tokens_season09b", cooldown_started_date: Old}]}, "gameserver");

        const Before = Date.now();
        assert.equal(StartCooldown(UserId, "1E3ADD6243A0C2A96C9F8B83AABF5C85", "gameserver").Status, 200);

        const Map = GetCooldownReply(UserId).payload;
        assert.deepEqual(Object.keys(Map).sort(), ["1E3ADD6243A0C2A96C9F8B83AABF5C85", "bounty_tokens_season09b"]);
        assert.ok(Date.parse(Map["1E3ADD6243A0C2A96C9F8B83AABF5C85"]) >= Before - 1000);
        assert.equal(Map["bounty_tokens_season09b"], Old);
    });

    it("keeps accounts apart", async () => {
        const A = await MakePlayer();
        const B = await MakePlayer();
        StartCooldown(A.UserId, "X", "gameserver");

        assert.deepEqual(GetCooldownReply(B.UserId).payload, {});
    });
});

const Bounty = (Id: string, Slot: number, Version: number, Progress = 0) => ({bounty_id: Id, premium_bounty: false, slot_index: Slot, objectives: [{objective_id: `${Id}_OBJ`, progress: Progress}], drafted_timestamp: "2026-09-21T14:31:05.123Z", update_version: Version});

describe("bounties", () => {
    it("an empty board for a new account, with draft_data", async () => {
        const {UserId} = await MakePlayer();

        assert.deepEqual(GetBountyReply(UserId), {code: "OK", message: "OK", payload: {bounties: [], draft_data: {current_draft_choices: [], previous_draft_selections: [], bronze_count: 0, silver_count: 0, gold_count: 0}}});
    });

    it("partial upserts: select, progress, draft saves with empty bounty lists never remove anything", async () => {
        const {UserId} = await MakePlayer();

        // Select a drafted bounty
        SetBounties(UserId, {bounties: [Bounty("Bounty_Bronze_KillRadiant", 0, 0)], draft_data: {current_draft_choices: [], previous_draft_selections: ["Bounty_Bronze_KillRadiant"], bronze_count: 1, silver_count: 0, gold_count: 0}}, "gameserver");
        // Progress (no draft_data key)
        SetBounties(UserId, {bounties: [Bounty("Bounty_Bronze_KillRadiant", 0, 1, 2)]}, "gameserver");
        // A second bounty, then a draft save with an empty list
        SetBounties(UserId, {bounties: [Bounty("Bounty_Silver_X", 1, 0)]}, "gameserver");
        SetBounties(UserId, {bounties: [], draft_data: {current_draft_choices: ["A", "B", "C"], previous_draft_selections: ["Bounty_Bronze_KillRadiant"], bronze_count: 1, silver_count: 1, gold_count: 0}}, "gameserver");

        const Board = GetBountyReply(UserId).payload;
        assert.deepEqual(Board.bounties.map((B: any) => [B.bounty_id, B.slot_index, B.update_version, B.objectives[0].progress]), [["Bounty_Bronze_KillRadiant", 0, 1, 2], ["Bounty_Silver_X", 1, 0, 0]]);
        assert.deepEqual(Board.draft_data.current_draft_choices, ["A", "B", "C"]);
        assert.deepEqual(Board.bounties[0], Bounty("Bounty_Bronze_KillRadiant", 0, 1, 2));
    });

    it("keeps a newer stored bounty over a stale write, and a new bounty takes over its slot", async () => {
        const {UserId} = await MakePlayer();
        SetBounties(UserId, {bounties: [Bounty("Bounty_A", 0, 5, 4)]}, "gameserver");

        assert.equal(SetBounties(UserId, {bounties: [Bounty("Bounty_A", 0, 3, 1)]}, "gameserver").Status, 200);
        assert.equal(GetBountyReply(UserId).payload.bounties[0].update_version, 5);

        SetBounties(UserId, {bounties: [Bounty("Bounty_B", 0, 0)]}, "gameserver");
        assert.deepEqual(GetBountyReply(UserId).payload.bounties.map((B: any) => B.bounty_id), ["Bounty_B"]);
    });

    it("delete removes claimed or abandoned bounties; unknown ids are fine", async () => {
        const {UserId} = await MakePlayer();
        SetBounties(UserId, {bounties: [Bounty("Bounty_A", 0, 0), Bounty("Bounty_B", 1, 0)]}, "gameserver");

        assert.equal(DeleteBounties(UserId, {bounty_ids: ["Bounty_A", "Bounty_Unknown"]}, "gameserver").Status, 200);
        assert.deepEqual(GetBountyReply(UserId).payload.bounties.map((B: any) => B.bounty_id), ["Bounty_B"]);
        assert.equal(DeleteBounties(UserId, {}, "gameserver").Status, 400);
    });

    it("always sends drafted_timestamp, even if the stored element lacks it", async () => {
        const {UserId} = await MakePlayer();
        SetBounties(UserId, {bounties: [{bounty_id: "Bounty_NoTime", slot_index: 2, objectives: [], update_version: 0}]}, "gameserver");

        assert.match(GetBountyReply(UserId).payload.bounties[0].drafted_timestamp, /^\d{4}-\d\d-\d\dT/);
    });
});
