import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { RunInventoryTransaction } from "../src/controllers/inventory";
import { MakePlayer, StackQuantity } from "./helpers";

// Harmonic's inventory cases (github.com/Harmonicrain/Undaunted 895f7c7, test/inventory.test.js), run
// against our inventory rules, which stay as verified in play. Where the rules differ, the case asserts
// ours, as the port plan decided:
// - an overspend is refused only with INVENTORY_REFUSE_OVERSPEND=1; by default it is clamped (a twin
//   case checks that), so a hunt's rewards are never lost to a removal the game server got wrong;
// - a negative quantity is refused, and a quantity of 0 is still accepted (mastery rewards send 0);
// - the reply lists every changed stack at its final quantity; the cases compare by catalogId, since
//   ours lists the added stacks first.
// test/inventory.test.ts has our own 26 cases; these overlap with a few of them on purpose.

after(() => RemoveTestDb(() => GetDb().$client.close()));

type Options = { AddInstanced?: any[], AddStacked?: any[], RemoveInstanced?: any[], RemoveStacked?: any[], SaveInstanced?: any[] };
type Account = { UserId: string, CharacterId: string };

function Transaction(Account: Account, TransactionId: string, Options: Options = {}){
    return RunInventoryTransaction(Account.UserId, Account.CharacterId, TransactionId,
        Options.AddInstanced ?? [], Options.AddStacked ?? [], Options.RemoveInstanced ?? [], Options.RemoveStacked ?? [], Options.SaveInstanced ?? [],
        {Caller: "gameserver"});
}

// The reply's stacks, by catalogId
function Reported(Result: Awaited<ReturnType<typeof Transaction>>){
    assert.ok(Result.success, JSON.stringify(Result));

    return [...Result.data!.response.updatedStackedItems].map((Item: any) => ({catalogId: Item.catalogId, quantity: Item.quantity}))
        .sort((A, B) => A.catalogId.localeCompare(B.catalogId));
}

async function WithRefusedOverspend<T>(Body: () => Promise<T>){
    process.env.INVENTORY_REFUSE_OVERSPEND = "1";

    try{
        return await Body();
    }
    finally{
        delete process.env.INVENTORY_REFUSE_OVERSPEND;
    }
}

describe("inventory cases from Harmonic's fork", () => {
    // from Harmonicrain/Undaunted test/inventory.test.js:45
    it("grants a stacked item", async () => {
        const Account = await MakePlayer();

        const Result = await Transaction(Account, "tx-grant-1", {AddStacked: [{catalogId: "BREAK_TEST_HIDE", quantity: 5}]});

        assert.equal(Result.success, true);
        assert.equal(StackQuantity(Account.CharacterId, "BREAK_TEST_HIDE"), 5);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:58
    it("accumulates repeated grants of the same catalog id", async () => {
        const Account = await MakePlayer();

        await Transaction(Account, "tx-acc-1", {AddStacked: [{catalogId: "ORB_TEST", quantity: 3}]});
        await Transaction(Account, "tx-acc-2", {AddStacked: [{catalogId: "ORB_TEST", quantity: 4}]});

        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 7);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:68
    it("removes a stacked item", async () => {
        const Account = await MakePlayer();

        await Transaction(Account, "tx-rem-1", {AddStacked: [{catalogId: "ORB_TEST", quantity: 10}]});
        await Transaction(Account, "tx-rem-2", {RemoveStacked: [{catalogId: "ORB_TEST", quantity: 4}]});

        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 6);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:78
    it("refuses a transaction against a character the account does not own, and writes nothing", async () => {
        const Mine = await MakePlayer();
        const Theirs = await MakePlayer();

        const Result = await RunInventoryTransaction(Mine.UserId, Theirs.CharacterId, "tx-cross-1", [], [{catalogId: "ORB_TEST", quantity: 1}], [], [], []);

        assert.deepEqual(Result, {success: false, error: "forbidden"});
        assert.equal(StackQuantity(Theirs.CharacterId, "ORB_TEST"), 0);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:96
    it("a replayed transaction id grants only once", async () => {
        const Account = await MakePlayer();

        await Transaction(Account, "tx-replay-1", {AddStacked: [{catalogId: "ORB_TEST", quantity: 5}]});
        const Replay = await Transaction(Account, "tx-replay-1", {AddStacked: [{catalogId: "ORB_TEST", quantity: 5}]});

        assert.ok(Replay.success && Replay.data!.replayed);
        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 5);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:107, under INVENTORY_REFUSE_OVERSPEND=1
    it("with INVENTORY_REFUSE_OVERSPEND=1, refuses to remove more of a stacked item than is held", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "tx-over-1", {AddStacked: [{catalogId: "ORB_TEST", quantity: 2}]});

        const Result = await WithRefusedOverspend(() => Transaction(Account, "tx-over-2", {RemoveStacked: [{catalogId: "ORB_TEST", quantity: 5}]}));

        assert.deepEqual(Result, {success: false, error: "insufficient_quantity"});
        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 2, "a refused removal leaves the held quantity untouched");
    });

    // The default-mode twin of line 107: ours clamps, so the rest of the transaction (a hunt's rewards) lands
    it("by default clamps that removal at 0 and still applies the rest of the transaction", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "tx-over-3", {AddStacked: [{catalogId: "ORB_TEST", quantity: 2}]});

        const Result = await Transaction(Account, "tx-over-4", {AddStacked: [{catalogId: "HUNT_REWARD", quantity: 3}], RemoveStacked: [{catalogId: "ORB_TEST", quantity: 5}]});

        assert.deepEqual(Reported(Result), [{catalogId: "HUNT_REWARD", quantity: 3}, {catalogId: "ORB_TEST", quantity: 0}]);
        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 0);
        assert.equal(StackQuantity(Account.CharacterId, "HUNT_REWARD"), 3);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:123, with our zero-quantity rule added
    it("refuses a negative grant quantity, and still accepts a quantity of 0", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "tx-neg-1", {AddStacked: [{catalogId: "ORB_TEST", quantity: 10}]});

        const Negative = await Transaction(Account, "tx-neg-2", {AddStacked: [{catalogId: "ORB_TEST", quantity: -8}]});
        assert.deepEqual(Negative, {success: false, error: "invalid_inventory_item"});
        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 10);

        const Zero = await Transaction(Account, "tx-neg-3", {AddStacked: [{catalogId: "ORB_TEST", quantity: 0}]});
        assert.equal(Zero.success, true, "a reward of 0 is not an error");
        assert.equal(StackQuantity(Account.CharacterId, "ORB_TEST"), 10);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:144
    it("crafting-style removals report the final quantity of every changed stack", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "seed-craft", {AddStacked: [{catalogId: "CURRENCY_NOTES", quantity: 4160}, {catalogId: "MAT_TEST_ORE", quantity: 14}]});

        const Result = await Transaction(Account, "craft-1", {
            AddStacked: [{catalogId: "MAT_TEST_PART", quantity: 1}],
            RemoveStacked: [{catalogId: "CURRENCY_NOTES", quantity: 10}, {catalogId: "MAT_TEST_ORE", quantity: 1}]
        });

        assert.deepEqual(Reported(Result), [
            {catalogId: "CURRENCY_NOTES", quantity: 4150},
            {catalogId: "MAT_TEST_ORE", quantity: 13},
            {catalogId: "MAT_TEST_PART", quantity: 1}
        ]);
        assert.equal(StackQuantity(Account.CharacterId, "CURRENCY_NOTES"), 4150);
        assert.equal(StackQuantity(Account.CharacterId, "MAT_TEST_ORE"), 13);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:166
    it("a stack used up entirely is reported at zero, not omitted", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "seed-deplete", {AddStacked: [{catalogId: "MAT_TEST_ORE", quantity: 2}]});

        const Result = await Transaction(Account, "deplete-1", {RemoveStacked: [{catalogId: "MAT_TEST_ORE", quantity: 2}]});

        assert.deepEqual(Reported(Result), [{catalogId: "MAT_TEST_ORE", quantity: 0}]);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:175
    it("a stack touched more than once is reported once, at its final quantity", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "seed-twice", {AddStacked: [{catalogId: "MAT_TEST_ORE", quantity: 5}]});

        const Result = await Transaction(Account, "twice-1", {
            RemoveStacked: [{catalogId: "MAT_TEST_ORE", quantity: 3}],
            AddStacked: [{catalogId: "MAT_TEST_ORE", quantity: 1}, {catalogId: "MAT_TEST_ORE", quantity: 1}]
        });

        assert.deepEqual(Reported(Result), [{catalogId: "MAT_TEST_ORE", quantity: 4}]);
    });

    // from Harmonicrain/Undaunted test/inventory.test.js:187
    it("a retried transaction returns the same reported quantities, not the current ones, and does not deduct again", async () => {
        const Account = await MakePlayer();
        await Transaction(Account, "seed-retry", {AddStacked: [{catalogId: "CURRENCY_NOTES", quantity: 100}]});

        const Options = {RemoveStacked: [{catalogId: "CURRENCY_NOTES", quantity: 30}]};
        const First = await Transaction(Account, "retry-craft", Options);
        await Transaction(Account, "later-spend", {RemoveStacked: [{catalogId: "CURRENCY_NOTES", quantity: 5}]});
        const Retry = await Transaction(Account, "retry-craft", {RemoveStacked: [{catalogId: "CURRENCY_NOTES", quantity: 30}]});

        assert.deepEqual(Reported(First), [{catalogId: "CURRENCY_NOTES", quantity: 70}]);
        assert.deepEqual(Reported(Retry), Reported(First), "a replay answers with what the original reported");
        assert.ok(Retry.success && Retry.data!.replayed);
        assert.equal(StackQuantity(Account.CharacterId, "CURRENCY_NOTES"), 65);
    });
});
