import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { inventory } from "../src/db/schema";
import { CreateStorePurchase, GrantKind, ListStoreOffers, MiddlemanWindowStart, RedeemStorePurchase, SelectWeeklyMiddlemanOffers } from "../src/controllers/freestore";
import { MakePlayer, StackQuantity } from "./helpers";
import {ShopLayout} from '../src/controllers/shoplayout';
import {WeeklyStoreWindow} from '../src/controllers/storeclock';
import {WithEnv} from './appenv';
import StoreCatalog from '../src/vendor/store_catalog.json';

after(() => {
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    process.env.STORE = "off";
    process.env.TRIALS_STORE = "0";
    process.env.MIDDLEMAN_STORE = "1";
});

function Credit(CharacterId: string, Quantity: number){
    GetDb().insert(inventory).values({
        characterId: CharacterId,
        instancedItems: "[]",
        stackedItems: JSON.stringify([{catalogId: "CURRENCY_CELLDUST", quantity: Quantity}])
    }).onConflictDoUpdate({
        target: inventory.characterId,
        set: {stackedItems: JSON.stringify([{catalogId: "CURRENCY_CELLDUST", quantity: Quantity}])}
    }).run();
}

describe("Middleman weekly Aetherdust offers", () => {
    it('shares exact availability boundaries with all rotating shop categories', () => WithEnv({STORE_RAMS:'1',STORE_WEEKLY_ROTATION:'1'}, () => {
        const boundary=WeeklyStoreWindow(new Date('2026-10-17T12:00:00Z')).end;
        for(const timestamp of [boundary-1,boundary,boundary+1]){
            const at=new Date(timestamp), window=WeeklyStoreWindow(at);
            const shop=ShopLayout(StoreCatalog.webstore,'test',at);
            const cells=SelectWeeklyMiddlemanOffers(at);
            assert.ok(shop.length>0 && cells.length>0);
            for(const offer of [...shop,...cells]){
                assert.equal(offer.availableFrom,new Date(window.start).toISOString());
                assert.equal(offer.availableTo,new Date(window.end).toISOString());
            }
        }
    }));
    it("rotates three stable families at the Thursday 18:00 UTC reset", () => {
        const Before = new Date("2026-10-01T17:59:59.000Z");
        const After = new Date("2026-10-01T18:00:00.000Z");
        const First = SelectWeeklyMiddlemanOffers(Before);
        const Same = SelectWeeklyMiddlemanOffers(new Date("2026-09-30T12:00:00.000Z"));
        const Next = SelectWeeklyMiddlemanOffers(After);

        assert.equal(First.length, 3);
        assert.deepEqual(Same, First);
        assert.equal(new Set(First.map((Offer) => (Offer.items ?? [])[0].catalogId.replace(/_(UC|R)$/, ""))).size, 3);
        assert.ok(Next.every((Offer) => !First.some((Old) => Old.items?.[0]?.catalogId.replace(/_(UC|R)$/, "") === Offer.items?.[0]?.catalogId.replace(/_(UC|R)$/, ""))));
        assert.equal(MiddlemanWindowStart(Before), Date.parse("2026-09-24T18:00:00.000Z"));
        assert.equal(MiddlemanWindowStart(After), Date.parse("2026-10-01T18:00:00.000Z"));
        assert.deepEqual(First.map((Offer) => Offer.cellDustPrice), [40, 40, 100]);
    });

    it("lists only this week's cells, each available once", async () => {
        const A = await MakePlayer();
        const Offers = ListStoreOffers(A.UserId, "weekly_cell_offering");

        assert.equal(Offers.length, 3);
        assert.ok(Offers.every((Offer) => Offer.remaining === 1));
        assert.ok(Offers.every((Offer) => Offer.tags.includes("weekly_cell_offering")));
        assert.ok(Offers.every((Offer) => typeof Offer.availableFrom === "string" && typeof Offer.availableTo === "string"));
        assert.ok(Offers.every((Offer) => GrantKind((Offer.items ?? [])[0].catalogId) === "stacked"));
        // No other price key at all, or the client offers the cell for Platinum
        for(const Key of ["platinumPrice", "prestigePrice", "event01Price", "steelMarksPrice", "gildedMarksPrice"]){
            assert.ok(Offers.every((Offer) => !Object.prototype.hasOwnProperty.call(Offer, Key)), Key);
        }
        assert.throws(() => CreateStorePurchase(A.UserId, "platinum", Offers[0].id), {Status: 400});
    });

    it("spends Aetherdust atomically and sells each weekly cell once per reset", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, 500);
        const Offer = ListStoreOffers(A.UserId, "weekly_cell_offering")[0];
        const Price = Offer.cellDustPrice as number;

        const Token = CreateStorePurchase(A.UserId, "CURRENCY_CELLDUST", Offer.id).purchaseToken;
        assert.equal(RedeemStorePurchase(A.UserId, "CURRENCY_CELLDUST", Token).Replayed, false);
        assert.equal(RedeemStorePurchase(A.UserId, "CURRENCY_CELLDUST", Token).Replayed, true);

        // Sold out for this account until the reset, even once the cell is no longer held
        assert.equal(ListStoreOffers(A.UserId, "weekly_cell_offering").find((Candidate) => Candidate.id === Offer.id)?.remaining, 0);
        assert.throws(() => CreateStorePurchase(A.UserId, "CURRENCY_CELLDUST", Offer.id), {Status: 409});
        Credit(A.CharacterId, 500 - Price);
        assert.throws(() => CreateStorePurchase(A.UserId, "CURRENCY_CELLDUST", Offer.id), {Status: 409});

        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_CELLDUST"), 500 - Price);

        // The other cells this week are still for sale, and other accounts are unaffected
        const Other = ListStoreOffers(A.UserId, "weekly_cell_offering").filter((Candidate) => Candidate.id !== Offer.id);
        assert.ok(Other.every((Candidate) => Candidate.remaining === 1));
        const B = await MakePlayer();
        assert.equal(ListStoreOffers(B.UserId, "weekly_cell_offering").find((Candidate) => Candidate.id === Offer.id)?.remaining, 1);
    });

    it("lets only the first of two outstanding tokens buy this week's cell", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, 500);
        const Offer = ListStoreOffers(A.UserId, "weekly_cell_offering")[0];
        const CatalogId = (Offer.items ?? [])[0].catalogId;

        const First = CreateStorePurchase(A.UserId, "CURRENCY_CELLDUST", Offer.id).purchaseToken;
        const Second = CreateStorePurchase(A.UserId, "CURRENCY_CELLDUST", Offer.id).purchaseToken;
        RedeemStorePurchase(A.UserId, "CURRENCY_CELLDUST", First);
        assert.throws(() => RedeemStorePurchase(A.UserId, "CURRENCY_CELLDUST", Second), {Status: 409});
        assert.equal(StackQuantity(A.CharacterId, CatalogId), 1);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_CELLDUST"), 500 - (Offer.cellDustPrice as number));
    });
});
