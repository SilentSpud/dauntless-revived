import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { inventory } from "../src/db/schema";
import { ApplyInventoryTransactionInTx, InventoryErrorOf, InventoryForbiddenError, InventoryInsufficientError, InventoryValidationError, RunInventoryTransaction } from "../src/controllers/inventory";
import { Count, MakePlayer, ReadStacks, StackQuantity } from "./helpers";

// The inventory core that runs inside the caller's transaction (ApplyInventoryTransactionInTx). The
// store grants items inside its own purchase transaction with it, so a failed purchase must take the
// grant, its item log and its ledger row back with it. RunInventoryTransaction (POST /inventory) is the
// same core in a transaction of its own; test/inventory.test.ts covers its rules and is unchanged.
// The pattern (one transaction-scoped core under the async entry point) is from
// github.com/Harmonicrain/Undaunted 895f7c7 (UndauntedMetagame/src/controllers/inventory.ts).

after(() => RemoveTestDb(() => GetDb().$client.close()));

const Guid = () => crypto.randomUUID().replace(/-/g, "").toUpperCase();
const Stack = (catalogId: string, quantity: any) => ({catalogId, quantity});
const Store = {Caller: "store" as const, Source: "store:test_offer"};

describe("ApplyInventoryTransactionInTx", () => {
    it("rolls back with an outer transaction that throws: no stack, no item log, no ledger row", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        const Id = Guid();

        assert.throws(() => GetDb().transaction((tx) => {
            const Applied = ApplyInventoryTransactionInTx(tx, {UserId, CharacterId, TransactionId: Id, StackedItemsToAdd: [Stack("ORB", 5)], InstancedItemsToAdd: [{catalogId: "WP_AXE", instanceId: "store-1", updateVersion: 0}]}, Store);

            assert.equal(Applied.replayed, false);
            // Visible inside the transaction
            assert.equal(JSON.parse(tx.select().from(inventory).where(eq(inventory.characterId, CharacterId)).get()!.stackedItems)[0].quantity, 5);

            throw new Error("the purchase receipt could not be written");
        }), /receipt could not be written/);

        assert.deepEqual(ReadStacks(CharacterId), []);
        assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 0);
        assert.equal(Count("inventorytransactions", "transactionId = ?", Id), 0);
    });

    it("commits with the outer transaction: the same ledger and item log as POST /inventory, as caller store", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        const Id = Guid();
        const Request = {UserId, CharacterId, TransactionId: Id, StackedItemsToAdd: [Stack("CURRENCY_RAMS", 20)]};

        const Applied = GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, Request, Store));

        assert.deepEqual(JSON.parse(JSON.stringify(Applied.response)), {updatedInstancedItems: [], updatedStackedItems: [Stack("CURRENCY_RAMS", 20)]});
        assert.deepEqual(Applied.overspent, []);
        assert.equal(StackQuantity(CharacterId, "CURRENCY_RAMS"), 20);

        const Rows = GetDb().$client.prepare("select caller, source, transactionId, operation, catalogId, quantityChange from inventorylog where characterId = ?").all(CharacterId) as any[];
        assert.deepEqual(Rows, [{caller: "store", source: "store:test_offer", transactionId: Id, operation: "add", catalogId: "CURRENCY_RAMS", quantityChange: 20}]);
        assert.equal(Count("inventorytransactions", "transactionId = ?", Id), 1);

        // One ledger for both entry points: the same request again is a replay, whichever way it comes
        const Again = GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, Request, Store));
        assert.equal(Again.replayed, true);
        const ViaRoute = await RunInventoryTransaction(UserId, CharacterId, Id, undefined, [Stack("CURRENCY_RAMS", 20)], undefined, undefined, undefined, {Caller: "gameserver"});
        assert.ok(ViaRoute.success && ViaRoute.data!.replayed);
        assert.equal(StackQuantity(CharacterId, "CURRENCY_RAMS"), 20);
        assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 1);
    });

    it("gives the same reply and the same inventory as RunInventoryTransaction for the same request", async () => {
        const A = await MakePlayer();
        const B = await MakePlayer();
        const Lists = (): [any, any, any, any, any] => [[{catalogId: "WP_SWORD", instanceId: "i-9", updateVersion: 0}], [Stack("ORB", 3), Stack("NOTES", "7")], undefined, [Stack("NOTES", 2)], undefined];

        const [IA, SA, IR, SR, IS] = Lists();
        const ViaRoute = await RunInventoryTransaction(A.UserId, A.CharacterId, Guid(), IA, SA, IR, SR, IS, {Caller: "gameserver"});
        const [IA2, SA2, IR2, SR2, IS2] = Lists();
        const ViaCore = GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {UserId: B.UserId, CharacterId: B.CharacterId, TransactionId: Guid(), InstancedItemsToAdd: IA2, StackedItemsToAdd: SA2, InstancedItemsToRemove: IR2, StackedItemsToRemove: SR2, InstancedItemsToSave: IS2}, {Caller: "gameserver"}));

        assert.ok(ViaRoute.success);
        assert.equal(JSON.stringify(ViaCore.response), JSON.stringify(ViaRoute.data!.response));
        assert.deepEqual(ReadStacks(B.CharacterId), ReadStacks(A.CharacterId));
        assert.deepEqual(ReadStacks(B.CharacterId), [Stack("ORB", 3), Stack("NOTES", 5)]);
    });

    it("a refusal throws and takes the caller's earlier writes in the same transaction back with it", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        let Caught: unknown;

        try{
            GetDb().transaction((tx) => {
                ApplyInventoryTransactionInTx(tx, {UserId, CharacterId, TransactionId: Guid(), StackedItemsToAdd: [Stack("ORB", 1)]}, Store);
                ApplyInventoryTransactionInTx(tx, {UserId, CharacterId, TransactionId: Guid(), StackedItemsToAdd: "not a list"}, Store);
            });
        }
        catch(error){
            Caught = error;
        }

        assert.ok(Caught instanceof InventoryValidationError);
        assert.equal(InventoryErrorOf(Caught), "invalid_inventory_item");
        assert.deepEqual(ReadStacks(CharacterId), []);
        assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 0);
    });

    it("refuses another player's character inside the transaction, even for an empty request", async () => {
        const A = await MakePlayer();
        const B = await MakePlayer();

        for(const Request of [
            {UserId: A.UserId, CharacterId: B.CharacterId, TransactionId: Guid(), StackedItemsToAdd: [Stack("ORB", 1)]},
            {UserId: A.UserId, CharacterId: B.CharacterId, TransactionId: Guid()},
            {UserId: A.UserId, CharacterId: "no-such-character", TransactionId: Guid(), StackedItemsToAdd: [Stack("ORB", 1)]}
        ]){
            let Caught: unknown;

            try{
                GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, Request, Store));
            }
            catch(error){
                Caught = error;
            }

            assert.ok(Caught instanceof InventoryForbiddenError, JSON.stringify(Request));
            assert.equal(InventoryErrorOf(Caught), "forbidden");
        }

        assert.deepEqual(ReadStacks(B.CharacterId), []);
    });

    it("an empty request changes nothing and answers the empty reply", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        const Applied = GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {UserId, CharacterId, TransactionId: Guid(), StackedItemsToAdd: []}, Store));

        assert.deepEqual(JSON.parse(JSON.stringify(Applied.response)), {updatedInstancedItems: [], updatedStackedItems: []});
        assert.equal(Applied.replayed, false);
        assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 0);
    });

    it("overspends follow INVENTORY_REFUSE_OVERSPEND: returned for the caller to log by default, thrown with 1", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        const Overspend = () => GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {UserId, CharacterId, TransactionId: Guid(), StackedItemsToAdd: [Stack("REWARD", 2)], StackedItemsToRemove: [Stack("ORB", 4)]}, Store));

        const Clamped = Overspend();
        assert.deepEqual(Clamped.overspent, ["removes 4 of ORB but only 0 held"]);
        assert.equal(StackQuantity(CharacterId, "REWARD"), 2);

        process.env.INVENTORY_REFUSE_OVERSPEND = "1";

        try{
            let Caught: unknown;

            try{
                Overspend();
            }
            catch(error){
                Caught = error;
            }

            assert.ok(Caught instanceof InventoryInsufficientError);
            assert.equal(InventoryErrorOf(Caught), "insufficient_quantity");
            assert.equal(StackQuantity(CharacterId, "REWARD"), 2, "nothing more was granted");
        }
        finally{
            delete process.env.INVENTORY_REFUSE_OVERSPEND;
        }
    });

    it("maps a stored inventory that is not JSON to invalid_inventory_data, and a database error to nothing", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        GetDb().insert(inventory).values({characterId: CharacterId, instancedItems: "[]", stackedItems: "{broken"}).run();

        let Caught: unknown;

        try{
            GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {UserId, CharacterId, TransactionId: Guid(), StackedItemsToAdd: [Stack("ORB", 1)]}, Store));
        }
        catch(error){
            Caught = error;
        }

        assert.equal(InventoryErrorOf(Caught), "invalid_inventory_data");
        assert.equal(InventoryErrorOf(new Error("SQLITE_BUSY")), undefined);
    });
});
