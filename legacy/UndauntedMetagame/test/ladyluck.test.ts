import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { inventory } from "../src/db/schema";
import { CreateStorePurchase, GetStoreOffer, GrantKind, ListStoreOffers, RedeemStorePurchase } from "../src/controllers/freestore";
import { MakePlayer, StackQuantity } from "./helpers";
import { StoreImageRateLimit } from "../src/middleware/RateLimits";

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    process.env.STORE = "off";
    process.env.TRIALS_STORE = "1";
    process.env.MIDDLEMAN_STORE = "0";
});

function Credit(CharacterId: string, CatalogId: string, Quantity: number){
    GetDb().insert(inventory).values({
        characterId: CharacterId,
        instancedItems: "[]",
        stackedItems: JSON.stringify([{catalogId: CatalogId, quantity: Quantity}])
    }).onConflictDoUpdate({
        target: inventory.characterId,
        set: {stackedItems: JSON.stringify([{catalogId: CatalogId, quantity: Quantity}])}
    }).run();
}

function ReadInstanced(CharacterId: string){
    const Row = GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get();
    return JSON.parse(Row?.instancedItems ?? "[]") as any[];
}

function Buy(UserId: string, Currency: string, Sku: string){
    const Token = CreateStorePurchase(UserId, Currency, Sku).purchaseToken;
    return RedeemStorePurchase(UserId, Currency, Token);
}

describe("Lady Luck Trials store", () => {
    it("uses the 1.4.4 flat-price shape and lists Champion gear without a leaderboard placement", async () => {
        const A = await MakePlayer();
        const Offers = ListStoreOffers(A.UserId, "ladyluckstore");

        assert.equal(Offers.length, 58);
        assert.ok(Offers.every((Offer) => !Object.prototype.hasOwnProperty.call(Offer, "prices")));
        assert.ok(Offers.every((Offer) => Number.isInteger(Offer.steelMarksPrice) !== Number.isInteger(Offer.gildedMarksPrice)));
        assert.equal(GetStoreOffer(A.UserId, "ladyluck_weapon_strikers_normal").gildedMarksPrice, 500);
        assert.equal(GetStoreOffer(A.UserId, "ladyluck_cb_passive_trials_02").steelMarksPrice, 250);
        assert.equal(GetStoreOffer(A.UserId, "ladyluck_weapon_strikers_prestige").gildedMarksPrice, 1000);
        assert.throws(() => CreateStorePurchase(A.UserId, "marksgilded", "ladyluck_weapon_strikers_prestige"), {Status: 409});
    });

    it("uses captured instanced-vs-stacked grant kinds instead of guessing from prefixes", () => {
        assert.equal(GrantKind("WP_AC_TRIALS_00"), "instanced");
        assert.equal(GrantKind("AR_TRIALS_CHEST_00"), "instanced");
        assert.equal(GrantKind("CELL_TRIALS_05_E"), "stacked");
        assert.equal(GrantKind("QI_LANTERN_POTION"), "stacked");
    });

    it("charges Gilded Marks once, grants an instanced cosmetic once, and then marks it owned", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_MARKS_GILDED", 700);

        const Token = CreateStorePurchase(A.UserId, "CURRENCY_MARKS_GILDED", "ladyluck_weapon_strikers_normal").purchaseToken;
        const First = RedeemStorePurchase(A.UserId, "CURRENCY_MARKS_GILDED", Token);
        const Retry = RedeemStorePurchase(A.UserId, "CURRENCY_MARKS_GILDED", Token);

        assert.equal(First.Replayed, false);
        assert.equal(Retry.Replayed, true);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_GILDED"), 200);
        assert.equal(ReadInstanced(A.CharacterId).filter((Item) => Item.catalogId === "WP_AC_TRIALS_00").length, 1);
        assert.equal(GetStoreOffer(A.UserId, "ladyluck_weapon_strikers_normal").remaining, 0);
        assert.throws(() => CreateStorePurchase(A.UserId, "marksgilded", "ladyluck_weapon_strikers_normal"), {Status: 409});
    });

    it("keeps the unlimited cell offer repeatable and charges each purchase atomically", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_MARKS_STEEL", 300);

        Buy(A.UserId, "markssteel", "trials_cell_discipline_e");
        Buy(A.UserId, "steelmarks", "trials_cell_discipline_e");

        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 0);
        assert.equal(StackQuantity(A.CharacterId, "CELL_TRIALS_05_E"), 2);
        assert.equal(GetStoreOffer(A.UserId, "trials_cell_discipline_e").remaining, 1);
        assert.throws(() => CreateStorePurchase(A.UserId, "markssteel", "trials_cell_discipline_e"), {Status: 409});
    });

    it("sells the Twin Suns, the weapon specials, every Trials mod and the six +3 Trials cells for Steel Marks", async () => {
        const A = await MakePlayer();
        const Steel: Record<string, number> = {
            ladyluck_weapon_twin_suns: 1000,
            ladyluck_eb_special_parry: 500, ladyluck_ih_special_islandcracker: 500, ladyluck_ms_special_rocketlunge: 500,
            ladyluck_ac_special_mastery: 500, ladyluck_ga_special_skillshot: 500,
            ladyluck_cb_passive_trials_01: 250, ladyluck_cb_passive_trials_02: 250, ladyluck_dp_passive_trials_01: 250, ladyluck_dp_passive_trials_02: 250,
            ladyluck_eb_passive_trials_01: 250, ladyluck_eb_passive_trials_02: 250, ladyluck_ga_passive_trials_01: 250, ladyluck_ga_passive_trials_02: 250,
            ladyluck_ih_passive_trials_01: 250, ladyluck_ih_passive_trials_02: 250, ladyluck_ms_passive_trials_01: 250, ladyluck_ms_passive_trials_02: 250,
            trials_cell_berserker_e: 150, trials_cell_strategist_e: 150, trials_cell_engineer_e: 150, trials_cell_discipline_e: 150,
            trials_cell_sprinter_e: 150, trials_cell_mender_e: 150
        };
        const Total = Object.values(Steel).reduce((Sum, Price) => Sum + Price, 0);
        Credit(A.CharacterId, "CURRENCY_MARKS_STEEL", Total);

        for(const [Sku, Price] of Object.entries(Steel)){
            assert.equal(GetStoreOffer(A.UserId, Sku).steelMarksPrice, Price, Sku);
            Buy(A.UserId, "markssteel", Sku);
        }

        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 0);
        assert.equal(JSON.parse(GetDb().select().from(inventory).where(eq(inventory.characterId,A.CharacterId)).get()!.instancedItems).filter((item:any)=>item.catalogId==="PART_GA_SPECIAL_SKILLSHOT").length, 1);
        assert.equal(StackQuantity(A.CharacterId, "CELL_TRIALS_01_E"), 1);
        assert.equal(GrantKind("WP_DP_EXOTIC_BOMBS"), "instanced");
        assert.throws(() => CreateStorePurchase(A.UserId, "markssteel", "ladyluck_weapon_twin_suns"), {Status: 409});
    });

    it("charges Steel Marks for one-time gameplay rewards", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_MARKS_STEEL", 300);

        Buy(A.UserId, "CURRENCY_MARKS_STEEL", "ladyluck_cb_passive_trials_02");

        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 50);
        assert.equal(JSON.parse(GetDb().select().from(inventory).where(eq(inventory.characterId,A.CharacterId)).get()!.instancedItems).filter((item:any)=>item.catalogId==="PART_CB_PASSIVE_TRIALS_02").length, 1);
        assert.equal(GetStoreOffer(A.UserId, "ladyluck_cb_passive_trials_02").remaining, 0);
    });

    it("uses its priced routes while the unrelated free cosmetic store stays off", async () => {
        const A = await MakePlayer();

        const Listed = await Call("GET", "/product/skus/public?requiredTags=ladyluckstore", {as: A.UserId});
        assert.equal(Listed.status, 200);
        assert.equal(Listed.json.length, 58);
        // Every offer points at its 1.4.4 icon on this server, and the icon is served without a login
        for(const Offer of Listed.json){
            assert.match(Offer.images.standard, /^http:\/\/[^/]+\/store-images\/[a-z0-9_]+\.png$/, Offer.id);
            assert.equal(Offer.images.feature, Offer.images.standard);
        }
        const Twin = Listed.json.find((Offer: any) => Offer.id === "ladyluck_weapon_twin_suns");
        const Image = await fetch(Twin.images.standard);
        assert.equal(Image.status, 200);
        assert.equal(Image.headers.get("content-type"), "image/png");
        assert.equal((await Call("GET", "/store-images/..%2Fpackage.json")).status, 404);
        assert.equal((await Call("GET", "/store-images/not_an_item.png")).status, 404);

        const Single = await Call("GET", "/product/sku/ladyluck_cb_passive_trials_02", {as: A.UserId});
        assert.equal(Single.status, 200);
        assert.equal(Single.json.steelMarksPrice, 250);

        const WebStore = await Call("GET", "/product/skus/public?requiredTags=webstore", {as: A.UserId});
        assert.equal(WebStore.status, 400);
    });

    it("is hidden behind TRIALS_STORE without exposing single-offer purchases", async () => {
        const A = await MakePlayer();
        process.env.TRIALS_STORE = "0";

        assert.deepEqual(ListStoreOffers(A.UserId, "ladyluckstore"), []);
        assert.throws(() => GetStoreOffer(A.UserId, "ladyluck_weapon_strikers_normal"), {Status: 404});
        assert.throws(() => CreateStorePurchase(A.UserId, "marksgilded", "ladyluck_weapon_strikers_normal"), {Status: 404});
    });
});

it('store images reject encoded path escapes and enforce their independent request budget', async () => {
    StoreImageRateLimit.resetKey('127.0.0.1');
    try {
        for(const value of ['..%2Fpackage.json','..%5Cpackage.json','%252e%252e%252fpackage.json','C%3A%5CWindows%5Cwin.ini']) {
            assert.equal((await Call('GET','/store-images/'+value)).status,404);
        }
        StoreImageRateLimit.resetKey('127.0.0.1');
        for(let i=0;i<600;i++) assert.equal((await Call('GET','/store-images/not_an_item.png')).status,404);
        assert.equal((await Call('GET','/store-images/not_an_item.png')).status,429);
    } finally { StoreImageRateLimit.resetKey('127.0.0.1'); }
});

it('grants both reported Trials unlocks as equipable instances and never stacks or duplicates them',async()=>{
    const A=await MakePlayer();
    Credit(A.CharacterId,'CURRENCY_MARKS_STEEL',750);
    for(const [sku,id] of [
      ['ladyluck_eb_special_parry','PART_EB_SPECIAL_PARRY'],
      ['ladyluck_ms_passive_trials_02','PART_MS_PASSIVE_TRIALS_02']
    ]){
      assert.equal(GrantKind(id),'instanced');
      const token=CreateStorePurchase(A.UserId,'markssteel',sku).purchaseToken;
      RedeemStorePurchase(A.UserId,'markssteel',token);
      RedeemStorePurchase(A.UserId,'markssteel',token);
      const instances=ReadInstanced(A.CharacterId).filter(i=>i.catalogId===id);
      assert.equal(instances.length,1);
      assert.ok(instances[0].instanceId);
      assert.equal(StackQuantity(A.CharacterId,id),0);
      assert.equal(GetStoreOffer(A.UserId,sku).remaining,0);
    }
    assert.equal(StackQuantity(A.CharacterId,'CURRENCY_MARKS_STEEL'),0);
});
