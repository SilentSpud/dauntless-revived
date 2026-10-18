import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { ApplyInventoryTransactionInTx, InventoryConflictError } from "../src/controllers/inventory";
import { Count, MakePlayer, StackQuantity } from "./helpers";

// A purchase is a debit and a grant in one transaction. Whatever stops the grant must stop the debit
// with it: a player who is charged and receives nothing has no way to tell us what went wrong.
//
// The case these cover: an instanced add whose instance the character already holds is skipped (so a
// retry does not duplicate the item). If that request also takes payment it is a repeat of a purchase
// that already succeeded, and before this it debited again for one delivery.

after(() => RemoveTestDb(() => GetDb().$client.close()));

const Guid = () => crypto.randomUUID().replace(/-/g, "").toUpperCase();
const Vendor = {Caller: "gameserver" as const, Source: "vendor:test"};
const Helm = (instanceId: string) => ({catalogId: "AR_HELM_TEST", instanceId, updateVersion: 0});

function InstancedItems(CharacterId: string){
    const Row = GetDb().$client.prepare("select instancedItems from inventories where characterId = ?").get(CharacterId) as any;
    return JSON.parse(Row?.instancedItems ?? "[]");
}

async function PlayerHolding(InstanceId: string){
    const Player = await MakePlayer();

    GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {
        ...Player, TransactionId: Guid(),
        StackedItemsToAdd: [{catalogId: "CURRENCY_NOTES", quantity: 5000}],
        InstancedItemsToAdd: [Helm(InstanceId)]
    }, Vendor));

    return Player;
}

describe("paying for an instanced item the character already holds", () => {
    it("takes no currency and writes nothing when the grant would be skipped", async () => {
        const Player = await PlayerHolding("collide-1");
        const LogRows = Count("inventorylog", "characterId = ?", Player.CharacterId);

        assert.throws(() => GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {
            ...Player, TransactionId: Guid(),
            StackedItemsToRemove: [{catalogId: "CURRENCY_NOTES", quantity: 750}],
            InstancedItemsToAdd: [Helm("collide-1")]
        }, Vendor)), InventoryConflictError);

        assert.equal(StackQuantity(Player.CharacterId, "CURRENCY_NOTES"), 5000);
        assert.equal(InstancedItems(Player.CharacterId).length, 1);
        assert.equal(Count("inventorylog", "characterId = ?", Player.CharacterId), LogRows);
    });

    it("still delivers and charges when the same request also brings a new instance", async () => {
        const Player = await PlayerHolding("collide-2");

        GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {
            ...Player, TransactionId: Guid(),
            StackedItemsToRemove: [{catalogId: "CURRENCY_NOTES", quantity: 750}],
            InstancedItemsToAdd: [Helm("collide-2"), Helm("fresh-2")]
        }, Vendor));

        assert.equal(StackQuantity(Player.CharacterId, "CURRENCY_NOTES"), 4250);
        assert.equal(InstancedItems(Player.CharacterId).length, 2);
    });

    it("leaves a free re-add alone: nothing is paid, so nothing is refused", async () => {
        const Player = await PlayerHolding("collide-3");

        const Applied = GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, {
            ...Player, TransactionId: Guid(),
            InstancedItemsToAdd: [Helm("collide-3")]
        }, Vendor));

        assert.equal(Applied.replayed, false);
        assert.equal(StackQuantity(Player.CharacterId, "CURRENCY_NOTES"), 5000);
        assert.equal(InstancedItems(Player.CharacterId).length, 1);
    });

    it("answers a true retry from the ledger, without reaching the refusal", async () => {
        const Player = await PlayerHolding("collide-4");
        const Request = {
            ...Player, TransactionId: Guid(),
            StackedItemsToRemove: [{catalogId: "CURRENCY_NOTES", quantity: 750}],
            InstancedItemsToAdd: [Helm("fresh-4")]
        };

        GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, Request, Vendor));
        assert.equal(StackQuantity(Player.CharacterId, "CURRENCY_NOTES"), 4250);

        const Again = GetDb().transaction((tx) => ApplyInventoryTransactionInTx(tx, Request, Vendor));

        assert.equal(Again.replayed, true);
        assert.equal(StackQuantity(Player.CharacterId, "CURRENCY_NOTES"), 4250);
        assert.equal(InstancedItems(Player.CharacterId).length, 2);
    });
});
