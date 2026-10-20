import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { and, eq } from "drizzle-orm";
import { StartApp, StopApp, Call } from "./appclient";
import { GetDb } from "../src/db";
import { entitlements } from "../src/db/schema";
import { CreateStorePurchase, GetStoreOffer, ListStoreOffers, RedeemStorePurchase } from "../src/controllers/freestore";
import { MakePlayer } from "./helpers";

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    process.env.STORE = "off";
    process.env.TRIALS_STORE = "0";
    process.env.MIDDLEMAN_STORE = "1";
});

function HasEntitlement(AccountId: string, Name: string){
    return GetDb().select().from(entitlements).where(and(
        eq(entitlements.accountId, AccountId),
        eq(entitlements.name, Name)
    )).get() != undefined;
}

describe("Middleman store", () => {
    it("serves the two exchange-slot SKUs the 1.4.4 client has art for", async () => {
        const A = await MakePlayer();
        const Slot2 = ListStoreOffers(A.UserId, "exchange_vendor_slot_2");
        const Slot3 = ListStoreOffers(A.UserId, "exchange_vendor_slot_3");

        assert.deepEqual(Slot2.map((Offer) => Offer.id), ["single_exchange_slot_2"]);
        assert.deepEqual(Slot3.map((Offer) => Offer.id), ["single_exchange_slot_3"]);
        assert.equal(Slot2[0].platinumPrice, 0);
        assert.deepEqual(Slot2[0].entitlements, [{name: "exchange_slot_2", duration: 0}]);
        assert.deepEqual(Slot3[0].entitlements, [{name: "exchange_slot_3", duration: 0}]);
    });

    it("unlocks each slot once through the normal two-step store transaction", async () => {
        const A = await MakePlayer();

        for(const [Sku, Entitlement] of [["single_exchange_slot_2", "exchange_slot_2"], ["single_exchange_slot_3", "exchange_slot_3"]] as const){
            const Token = CreateStorePurchase(A.UserId, "platinum", Sku).purchaseToken;
            const First = RedeemStorePurchase(A.UserId, "platinum", Token);
            const Retry = RedeemStorePurchase(A.UserId, "platinum", Token);

            assert.equal(First.Replayed, false);
            assert.equal(Retry.Replayed, true);
            assert.ok(HasEntitlement(A.UserId, Entitlement));
            assert.equal(GetStoreOffer(A.UserId, Sku).remaining, 0);
            assert.throws(() => CreateStorePurchase(A.UserId, "platinum", Sku), {Status: 409});
        }
    });

    it("is reachable by the authenticated 1.4.4 store routes while STORE remains off", async () => {
        const A = await MakePlayer();

        const Listed = await Call("GET", "/product/skus/public?requiredTags=exchange_vendor_slot_2", {as: A.UserId});
        assert.equal(Listed.status, 200);
        assert.deepEqual(Listed.json.map((Offer: any) => Offer.id), ["single_exchange_slot_2"]);

        const Single = await Call("GET", "/product/sku/single_exchange_slot_3", {as: A.UserId});
        assert.equal(Single.status, 200);
        assert.equal(Single.json.id, "single_exchange_slot_3");

        const WebStore = await Call("GET", "/product/skus/public?requiredTags=webstore", {as: A.UserId});
        assert.equal(WebStore.status, 400);
    });

    it("hides direct Middleman offers when MIDDLEMAN_STORE is off", async () => {
        const A = await MakePlayer();
        process.env.MIDDLEMAN_STORE = "0";

        assert.deepEqual(ListStoreOffers(A.UserId, "exchange_vendor_slot_2"), []);
        assert.throws(() => GetStoreOffer(A.UserId, "single_exchange_slot_2"), {Status: 404});

        const Listed = await Call("GET", "/product/skus/public?requiredTags=exchange_vendor_slot_2", {as: A.UserId});
        assert.equal(Listed.status, 400);
        assert.equal((await Call("GET", "/product/sku/single_exchange_slot_2", {as: A.UserId})).status, 404);
    });
});
