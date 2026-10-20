import { RemoveTestDb } from "./setup";
import "./authenv";
import { WithEnv } from "./appenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { and, eq } from "drizzle-orm";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { characters, inventory, storepurchases, users } from "../src/db/schema";
import { CreateStorePurchase, GetStoreOffer, GrantKind, ListStoreOffers, MAX_TOKENS_PER_WINDOW, PruneExpiredStorePurchases, RECEIPT_DAYS, RedeemStorePurchase, SetExtraOffersForTests, SetStoreTokenLimitForTests, StoreOffer } from "../src/controllers/freestore";
import { HasActiveEntitlement, ListEntitlements, RevokeEntitlementInTx } from "../src/controllers/entitlements";
import { CreateCharacterForUid, UpdateCharacterForUid } from "../src/controllers/character";
import StoreCatalog from "../src/vendor/store_catalog.json";
import StoreItemKinds from "../src/vendor/store_item_kinds.json";
import StoreArt from "./data/store_art_skus.json";
import { Count, MakePlayer, StackQuantity } from "./helpers";

// The free store (roadmap 3.7, STORE=free). The catalogue, the purchase-token flow and most of these
// cases come from Harmonic's fork (github.com/Harmonicrain/Undaunted 895f7c7, test/free-store.test.js and
// test/entitlements.test.js); each ported case names the line of the original test. Where ours differs: a token
// goes to the account's active character (the fork refused accounts with more than one), items go through our
// inventory core (ledger and inventorylog, caller "store"), ownership of an entitlement is an active one
// (every account owns the Elite pass by default, so the cases about buying it empty ENTITLEMENTS_DEFAULT),
// and the bounty-token bundle is sold only with STORE_REPEATABLE_TOKENS=1.

const SKU = "single_armour_helm_savant";
const ITEM = "AR_SAINTS19_ROMANTIC_HELM_01";

type Offer = { id: string, tags: string[], platinumPrice: number, items: { catalogId: string, quantity: number }[] | null, entitlements: { name: string }[] | null };
const Catalog = StoreCatalog as unknown as Record<string, Offer[]> & { webstore: Offer[] };
const Sellable = Catalog.webstore.filter(Offer => !Offer.id.startsWith("single_dye_sheen_"));
const Kinds = StoreItemKinds as unknown as Record<string, string>;
const ArtRows = new Set((StoreArt as { skus: string[] }).skus);

// A second connection to the test database (the package ships no type declarations)
const Database: any = require("better-sqlite3");

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    process.env.STORE = "free";
    delete process.env.STORE_REPEATABLE_TOKENS;
    delete process.env.ENTITLEMENTS_DEFAULT;
});

const Token = (UserId: string, Sku = SKU) => CreateStorePurchase(UserId, "platinum", Sku).purchaseToken;
const Redeem = (UserId: string, PurchaseToken: string) => RedeemStorePurchase(UserId, "platinum", PurchaseToken);
const Buy = (UserId: string, Sku: string) => Redeem(UserId, Token(UserId, Sku));

// Every storefront offer that is not owned yet when its turn comes (a bundle bought earlier may own a
// single item's offer), past the per-account token limit
function BuyEveryOffer(UserId: string, Offers: Offer[]){
    SetStoreTokenLimitForTests(10000);
    try{
        for(const Found of Offers){
            if(GetStoreOffer(UserId, Found.id).remaining === 1){
                Buy(UserId, Found.id);
            }
        }
    }
    finally{
        SetStoreTokenLimitForTests();
    }
}

function ReadInventory(CharacterId: string){
    const Row = GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get();

    return { stacked: JSON.parse(Row?.stackedItems ?? "[]") as any[], instanced: JSON.parse(Row?.instancedItems ?? "[]") as any[] };
}

it('paid competing SKUs recheck funds and overlapping bundles never duplicate cosmetics',async()=>{
    const A=await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId,STORE_WEEKLY_ROTATION:undefined},()=>{
        GetDb().insert(inventory).values({characterId:A.CharacterId,instancedItems:'[]',stackedItems:JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity:5000}])}).run();
        const chest=CreateStorePurchase(A.UserId,'notes','single_armour_monk_chest').purchaseToken;
        const kit=CreateStorePurchase(A.UserId,'notes','bundle_armour_monk').purchaseToken;
        RedeemStorePurchase(A.UserId,'notes',chest);
        assert.throws(()=>RedeemStorePurchase(A.UserId,'notes',kit),{Status:409});
        assert.equal(StackQuantity(A.CharacterId,'CURRENCY_NOTES'),4250);
        const held=ReadInventory(A.CharacterId).stacked;
        held.find(i=>i.catalogId==='CURRENCY_NOTES').quantity=5000;
        GetDb().update(inventory).set({stackedItems:JSON.stringify(held)}).where(eq(inventory.characterId,A.CharacterId)).run();
        RedeemStorePurchase(A.UserId,'notes',kit);
        assert.equal(StackQuantity(A.CharacterId,'CURRENCY_NOTES'),0);
        const cosmetics=ReadInventory(A.CharacterId).stacked.filter(i=>i.catalogId!=='CURRENCY_NOTES');
        assert.equal(cosmetics.length,4);
        assert.ok(cosmetics.every(i=>i.quantity===1));
        RedeemStorePurchase(A.UserId,'notes',kit);
        assert.equal(StackQuantity(A.CharacterId,'CURRENCY_NOTES'),0);
    });
});

it('missing, corrupt and invalid Rams balances fail closed without repairing player inventory',async()=>{
    const A=await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId,STORE_WEEKLY_ROTATION:undefined},()=>{
        assert.throws(()=>CreateStorePurchase(A.UserId,'notes',SKU),{Status:409});
        GetDb().insert(inventory).values({characterId:A.CharacterId,instancedItems:'[]',stackedItems:'[]'}).run();
        const valid=JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity:10000}]);
        for(const raw of ['{','null','{}','[]',...[-1,1500.5,'bad',null].map(quantity=>JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity}]))]){
            GetDb().update(inventory).set({stackedItems:valid}).where(eq(inventory.characterId,A.CharacterId)).run();
            const token=CreateStorePurchase(A.UserId,'notes',SKU).purchaseToken;
            GetDb().update(inventory).set({stackedItems:raw}).where(eq(inventory.characterId,A.CharacterId)).run();
            assert.throws(()=>CreateStorePurchase(A.UserId,'notes',SKU),{Status:409});
            assert.throws(()=>RedeemStorePurchase(A.UserId,'notes',token),{Status:409});
            assert.equal(GetDb().select().from(inventory).where(eq(inventory.characterId,A.CharacterId)).get()!.stackedItems,raw);
        }
        assert.ok(GetDb().select().from(storepurchases).where(eq(storepurchases.accountId,A.UserId)).all().every(p=>p.redeemedDate===null));
    });
});

const EntitlementNames = (UserId: string) => ListEntitlements(UserId).map((Entitlement) => Entitlement.name);
const NoDefaults = <T>(Body: () => T | Promise<T>) => WithEnv({ ENTITLEMENTS_DEFAULT: "" }, Body);

it('Rams pilot charges only the selected account, rejects wrong currency, replay and duplicate charges', async () => {
    const A = await MakePlayer(), B = await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId}, () => {
        const db=GetDb();
        const balance=()=>StackQuantity(A.CharacterId,'CURRENCY_NOTES');
        assert.equal(GetStoreOffer(A.UserId,SKU).notesPrice,750);
        assert.equal(GetStoreOffer(A.UserId,SKU).platinumPrice,undefined);
        assert.equal(GetStoreOffer(B.UserId,SKU).platinumPrice,0);
        assert.throws(()=>CreateStorePurchase(A.UserId,'platinum',SKU),{Status:400});
        db.insert(inventory).values({characterId:A.CharacterId,instancedItems:'[]',stackedItems:JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity:1500}])}).run();
        const first=CreateStorePurchase(A.UserId,'notes',SKU).purchaseToken;
        const second=CreateStorePurchase(A.UserId,'notes',SKU).purchaseToken;
        assert.throws(()=>RedeemStorePurchase(B.UserId,'notes',first),{Status:403});
        RedeemStorePurchase(A.UserId,'notes',first);
        assert.equal(balance(),750);
        assert.equal(StackQuantity(A.CharacterId,ITEM),1);
        RedeemStorePurchase(A.UserId,'notes',first);
        assert.equal(balance(),750);
        assert.throws(()=>RedeemStorePurchase(A.UserId,'notes',second),{Status:409});
        assert.equal(balance(),750);
    });
});

it('native Rams wire purchase charges Rams once and rejects another account using the bridge',async()=>{
    const A=await MakePlayer(), B=await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId,STORE_RAMS_EVENT_TRANSPORT:'1'},async()=>{
        GetDb().insert(inventory).values({characterId:A.CharacterId,instancedItems:'[]',stackedItems:JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity:1500}])}).run();
        const offer=await Call('GET',`/product/sku/${SKU}`,{as:A.UserId});
        assert.equal(offer.json.event01Price,750);
        assert.equal(offer.json.notesPrice,undefined);
        assert.ok(offer.json.tags.includes('rams-store-pilot'));
        const balance=await Call('GET','/balance',{as:A.UserId});
        assert.equal(balance.json.CURRENCY_NOTES,1500);
        assert.equal(balance.json.CURRENCY_EVENT_01,1500);
        const otherBalance=await Call('GET','/balance',{as:B.UserId});
        assert.equal(otherBalance.json.CURRENCY_EVENT_01,undefined);
        assert.equal((await Call('GET',`/token/event01/${SKU}`,{as:B.UserId})).status,400);
        const issued=await Call('GET',`/token/event_01/${SKU}`,{as:A.UserId});
        assert.equal(issued.status,200);
        for(let i=0;i<2;i++) assert.equal((await Call('POST',`/notification/event_01?token=${issued.json.purchaseToken}`,{as:A.UserId})).status,204);
        assert.equal(StackQuantity(A.CharacterId,'CURRENCY_NOTES'),750);
        assert.equal(StackQuantity(A.CharacterId,ITEM),1);
        assert.equal((await Call('GET','/balance',{as:A.UserId})).json.CURRENCY_EVENT_01,750);
    });
});

it('production Rams shop requires updated clients and charges every account through the native bridge',async()=>{
    const A=await MakePlayer(), B=await MakePlayer();
    await WithEnv({STORE_RAMS:'1',STORE_RAMS_TEST_ACCOUNT:undefined,STORE_RAMS_EVENT_TRANSPORT:'1',STORE_WEEKLY_ROTATION:'1'},async()=>{
        for(const player of [A,B]){
            GetDb().insert(inventory).values({characterId:player.CharacterId,instancedItems:'[]',stackedItems:JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity:100000}])}).run();
            for(const path of ['/product/skus/public?requiredTags=webstore',`/product/sku/${SKU}`,`/token/event01/${SKU}`]){
                assert.equal((await Call('GET',path,{as:player.UserId})).status,426);
            }
            assert.notEqual((await Call('GET','/product/skus/public?requiredTags=ladyluckstore',{as:player.UserId})).status,426);
            const listed=await Call('GET','/product/skus/public?requiredTags=webstore&shop_protocol=rams-v1',{as:player.UserId});
            assert.equal(listed.status,200);
            assert.deepEqual(listed.json.map((o:any)=>o.id),ListStoreOffers(player.UserId,'webstore').map(o=>o.id));
            const offer=listed.json.find((o:any)=>o.event01Price>0 && o.items?.length);
            assert.ok(offer);
            const detail=await Call('GET',`/product/sku/${offer.id}?shop_protocol=rams-v1`,{as:player.UserId});
            assert.equal(detail.status,200);
            assert.equal(detail.json.event01Price,offer.event01Price);
            const issued=await Call('GET',`/token/event01/${offer.id}?shop_protocol=rams-v1`,{as:player.UserId});
            assert.equal(issued.status,200);
            assert.equal((await Call('POST',`/notification/event01?token=${issued.json.purchaseToken}&shop_protocol=rams-v1`,{as:player.UserId})).status,204);
            const balance=await Call('GET','/balance',{as:player.UserId});
            assert.equal(balance.json.CURRENCY_NOTES,100000-offer.event01Price);
            assert.equal(balance.json.CURRENCY_EVENT_01,100000-offer.event01Price);
            assert.ok(ReadInventory(player.CharacterId).stacked.some(i=>i.catalogId===offer.items[0].catalogId)
                || ReadInventory(player.CharacterId).instanced.some(i=>i.catalogId===offer.items[0].catalogId));
        }
    });
});

it('weekly shop rejects inactive offers and outstanding tokens without taking Rams',async()=>{
    const A=await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId},async()=>{
        GetDb().insert(inventory).values({characterId:A.CharacterId,instancedItems:'[]',stackedItems:JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity:100000}])}).run();
        const all=ListStoreOffers(A.UserId,'webstore');
        let hidden='';
        await WithEnv({STORE_WEEKLY_ROTATION:'1'},()=>{
            const active=new Set(ListStoreOffers(A.UserId,'webstore').map(o=>o.id));
            hidden=all.find(o=>typeof o.notesPrice==='number'&&!active.has(o.id))!.id;
            assert.throws(()=>CreateStorePurchase(A.UserId,'notes',hidden),{Status:404});
        });
        const token=CreateStorePurchase(A.UserId,'notes',hidden).purchaseToken;
        await WithEnv({STORE_WEEKLY_ROTATION:'1'},()=>{
            assert.throws(()=>RedeemStorePurchase(A.UserId,'notes',token),{Status:409});
            assert.equal(StackQuantity(A.CharacterId,'CURRENCY_NOTES'),100000);
        });
    });
});

it('Rams pilot checks balance again when redeeming and rejects tokens after pricing changes',async()=>{
    const A=await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId},()=>{
        GetDb().insert(inventory).values({characterId:A.CharacterId,instancedItems:'[]',stackedItems:'[]'}).run();
        const setBalance=(quantity:number)=>GetDb().update(inventory).set({stackedItems:JSON.stringify([{catalogId:'CURRENCY_NOTES',quantity}])}).where(eq(inventory.characterId,A.CharacterId)).run();
        setBalance(749);
        assert.throws(()=>CreateStorePurchase(A.UserId,'notes',SKU),{Status:409});
        setBalance(750);
        const token=CreateStorePurchase(A.UserId,'notes',SKU).purchaseToken;
        setBalance(0);
        assert.throws(()=>RedeemStorePurchase(A.UserId,'notes',token),{Status:409});
        assert.equal(StackQuantity(A.CharacterId,ITEM),0);
        setBalance(750);
        delete process.env.STORE_RAMS_TEST_ACCOUNT;
        assert.throws(()=>RedeemStorePurchase(A.UserId,'notes',token),{Status:409});
        assert.equal(StackQuantity(A.CharacterId,'CURRENCY_NOTES'),750);
    });
});

describe("purchases (Harmonic's free-store cases)", () => {
    // from Harmonicrain/Undaunted test/free-store.test.js:29, adapted: the second token is issued before the
    // first is redeemed, since a token for an offer already owned is refused
    it("a free grant persists, reads as owned, and retries or new tokens do not duplicate it", async () => {
        const A = await MakePlayer();
        const First = Token(A.UserId);
        const Second = Token(A.UserId);

        Redeem(A.UserId, First);
        Redeem(A.UserId, First);
        Redeem(A.UserId, Second);
        assert.throws(() => Token(A.UserId), { Status: 409 }, "no new token for what the player owns");

        assert.equal(StackQuantity(A.CharacterId, ITEM), 1);
        assert.equal(ListStoreOffers(A.UserId, "webstore").find((Offer) => Offer.id === SKU)!.remaining, 0);

        // A separate database connection sees the committed item and both receipts
        const Reopened = new Database(process.env.DB_FILENAME!, { readonly: true });
        try{
            const Row = Reopened.prepare("select stackedItems from inventories where characterId = ?").get(A.CharacterId) as any;
            assert.equal(JSON.parse(Row.stackedItems).find((Item: any) => Item.catalogId === ITEM).quantity, 1);
            assert.equal((Reopened.prepare("select count(*) n from storepurchases where accountId = ? and redeemedDate is not null").get(A.UserId) as any).n, 2);
        }
        finally{
            Reopened.close();
        }
    });

    // from Harmonicrain/Undaunted test/free-store.test.js:43
    it("an overlapping bundle and single item unlock each cosmetic once", async () => {
        const A = await MakePlayer();

        Buy(A.UserId, "single_armour_monk_chest");
        Buy(A.UserId, "bundle_armour_monk");

        const Held = ReadInventory(A.CharacterId).stacked;
        assert.equal(Held.length, 4);
        assert.ok(Held.every((Item) => Item.quantity === 1));
    });

    // from Harmonicrain/Undaunted test/free-store.test.js:52
    it("another account's token, a wrong currency, an unknown SKU and a malformed token are refused", async () => {
        const A = await MakePlayer(), B = await MakePlayer();
        const PurchaseToken = Token(A.UserId);

        assert.throws(() => Redeem(B.UserId, PurchaseToken), { Status: 403 });
        assert.throws(() => RedeemStorePurchase(A.UserId, "notes", PurchaseToken), { Status: 400 });
        assert.throws(() => CreateStorePurchase(A.UserId, "notes", SKU), { Status: 400 });
        assert.throws(() => CreateStorePurchase(A.UserId, "platinum", "made_up"), { Status: 404 });
        assert.throws(() => Redeem(A.UserId, "made_up"), { Status: 400 });
        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
        assert.equal(StackQuantity(B.CharacterId, ITEM), 0);
    });

    // from Harmonicrain/Undaunted test/free-store.test.js:61
    it("an expired token and a changed offer cannot grant", async () => {
        const A = await MakePlayer();
        const Expired = Token(A.UserId);

        GetDb().update(storepurchases).set({ expiresDate: "2000-01-01T00:00:00.000Z" }).where(eq(storepurchases.accountId, A.UserId)).run();
        assert.throws(() => Redeem(A.UserId, Expired), { Status: 410 });

        const Changed = Token(A.UserId);
        GetDb().update(storepurchases).set({ offerHash: "changed" }).where(eq(storepurchases.accountId, A.UserId)).run();
        assert.throws(() => Redeem(A.UserId, Changed), { Status: 409 });
        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
    });

    // from Harmonicrain/Undaunted test/free-store.test.js:72, plus the item log and the ledger
    it("the grant, its item log and the receipt roll back together if the receipt cannot be written", async () => {
        const A = await MakePlayer();
        const PurchaseToken = Token(A.UserId);

        GetDb().$client.exec("CREATE TEMP TRIGGER test_store_abort BEFORE UPDATE ON storepurchases BEGIN SELECT RAISE(ABORT, 'test failure'); END");
        try{
            assert.throws(() => Redeem(A.UserId, PurchaseToken), /test failure/);
        }
        finally{
            GetDb().$client.exec("DROP TRIGGER test_store_abort");
        }

        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
        assert.equal(Count("inventorylog", "characterId = ?", A.CharacterId), 0);
        assert.equal(Count("inventorytransactions", "characterId = ?", A.CharacterId), 0);

        Redeem(A.UserId, PurchaseToken);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 1);
    });

    // from Harmonicrain/Undaunted test/free-store.test.js:81, adapted: an account with several characters
    // buys for the one it played last (the fork refused the purchase); a character moved away is refused
    it("buys for the character played last, never an arbitrary one, and not for a character moved away", async () => {
        const A = await MakePlayer();
        const Second = await CreateCharacterForUid(A.UserId, "Second");
        const Third = await CreateCharacterForUid(A.UserId, "Third");

        // The second character is saved last
        assert.deepEqual(UpdateCharacterForUid(A.CharacterId, A.UserId, "{}", 1), { success: true });
        assert.deepEqual(UpdateCharacterForUid(Second.id as string, A.UserId, "{}", 1), { success: true });

        Buy(A.UserId, SKU);
        assert.equal(StackQuantity(Second.id as string, ITEM), 1);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
        assert.equal(StackQuantity(Third.id as string, ITEM), 0);

        // Now the first one is played
        assert.deepEqual(UpdateCharacterForUid(A.CharacterId, A.UserId, "{}", 2), { success: true });
        const PurchaseToken = Token(A.UserId, "single_armour_monk_chest");
        assert.equal(GetDb().select().from(storepurchases).where(eq(storepurchases.accountId, A.UserId)).all().find((Row) => Row.skuId === "single_armour_monk_chest")!.characterId, A.CharacterId);

        // ... and moved to another account before the purchase is confirmed
        const B = await MakePlayer();
        GetDb().update(characters).set({ userId: B.UserId }).where(eq(characters.characterId, A.CharacterId)).run();
        assert.throws(() => Redeem(A.UserId, PurchaseToken), { Status: 403 });
        assert.equal(ReadInventory(A.CharacterId).stacked.length, 0);
    });

    it("an account without a character gets no token (409)", async () => {
        GetDb().insert(users).values({ userId: "UID-store-nochar", name: "NoChar", notes: 0 }).run();

        assert.throws(() => CreateStorePurchase("UID-store-nochar", "platinum", SKU), { Status: 409 });
        assert.equal(Count("storepurchases", "accountId = 'UID-store-nochar'"), 0);
    });

    it("tokens that expired without being redeemed are removed when a token is issued", async () => {
        const A = await MakePlayer();
        Token(A.UserId);
        Buy(A.UserId, "single_armour_monk_chest");
        GetDb().update(storepurchases).set({ expiresDate: "2000-01-01T00:00:00.000Z" }).where(eq(storepurchases.accountId, A.UserId)).run();

        Token(A.UserId, "bundle_armour_iron");

        assert.deepEqual(GetDb().select().from(storepurchases).where(eq(storepurchases.accountId, A.UserId)).all().map((Row) => [Row.skuId, Row.redeemedDate != null]).sort(),
            [["bundle_armour_iron", false], ["single_armour_monk_chest", true]], "the redeemed receipt is kept, the unused expired token removed");
    });
});

describe("offers and entitlements (Harmonic's entitlement cases)", () => {
    // from Harmonicrain/Undaunted test/entitlements.test.js:42 (no default entitlements: the default Elite
    // pass reads as owned, and no token is issued for an offer already owned)
    it("an offer outside the webstore tag is found rather than reported unknown", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();

            assert.match(Token(A.UserId, "season09b_premium"), /^[a-f0-9]{64}$/);
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:53
    it("an unknown sku is still refused", async () => {
        const A = await MakePlayer();

        assert.throws(() => Token(A.UserId, "not_a_real_sku"), { Status: 404 });
        assert.throws(() => GetStoreOffer(A.UserId, "not_a_real_sku"), { Status: 404 });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:61 (no default entitlements)
    it("a single-offer lookup finds a sku outside the webstore tag", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            const Found = GetStoreOffer(A.UserId, "season09b_premium");

            assert.equal(Found.id, "season09b_premium");
            assert.equal(Found.remaining, 1, "not yet owned");
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:72 (no default entitlements)
    it("an entitlement offer reads as owned once granted", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            Buy(A.UserId, "season09b_premium");

            assert.equal(GetStoreOffer(A.UserId, "season09b_premium").remaining, 0);
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:85 (no default entitlements)
    it("redeeming the pass grants the entitlement", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            assert.equal(GetDb().transaction((tx) => HasActiveEntitlement(tx, A.UserId, "season09b_premium")), false, "starts without it");

            Buy(A.UserId, "season09b_premium");

            assert.equal(GetDb().transaction((tx) => HasActiveEntitlement(tx, A.UserId, "season09b_premium")), true);
            assert.equal((GetDb().$client.prepare("select source from entitlements where accountId = ? and name = 'season09b_premium'").get(A.UserId) as any).source, "store:season09b_premium");
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:111 (no default entitlements)
    it("GET /entitlementsv2 matches the 1.4.4 QueryEntitlements response contract", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer(), Other = await MakePlayer();
            Buy(A.UserId, "season09b_premium");

            for(const [UserId, Expected] of [[A.UserId, ["season09b_premium"]], [Other.UserId, []]] as [string, string[]][]){
                const Reply = await Call("GET", "/entitlementsv2", { as: UserId });
                assert.equal(Reply.status, 200);
                assert.deepEqual(Object.keys(Reply.json), ["entitlements"]);
                assert.deepEqual(Reply.json.entitlements.map((Entry: any) => Entry.name), Expected);
                for(const Entry of Reply.json.entitlements){
                    assert.deepEqual(Object.keys(Entry).sort(), ["activatedDate", "duration", "name"]);
                    assert.equal(Entry.duration, 0);
                    assert.ok(!Number.isNaN(Date.parse(Entry.activatedDate)));
                }
            }
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:145, adapted: both tokens are issued first, since a
    // token for an entitlement already held is refused
    it("a second purchase of the same entitlement does not duplicate it", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            const First = Token(A.UserId, "season09b_premium"), Second = Token(A.UserId, "season09b_premium");
            Redeem(A.UserId, First);
            Redeem(A.UserId, Second);

            assert.deepEqual(EntitlementNames(A.UserId), ["season09b_premium"]);
            assert.throws(() => Token(A.UserId, "season09b_premium"), { Status: 409 });
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:157
    it("redeeming the same token twice is idempotent", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            const PurchaseToken = Token(A.UserId, "season09b_premium");

            assert.equal(Redeem(A.UserId, PurchaseToken).Replayed, false);
            assert.equal(Redeem(A.UserId, PurchaseToken).Replayed, true);
            assert.deepEqual(EntitlementNames(A.UserId), ["season09b_premium"]);
        });
    });

    it("a revoked entitlement can be bought again", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            Buy(A.UserId, "single_dye_hairtint_01");
            GetDb().transaction((tx) => RevokeEntitlementInTx(tx, A.UserId, "ent_cchd_hp07a_01"));

            assert.equal(GetStoreOffer(A.UserId, "single_dye_hairtint_01").remaining, 1);
            Buy(A.UserId, "single_dye_hairtint_01");
            assert.deepEqual(EntitlementNames(A.UserId), ["ent_cchd_hp07a_01"]);
            assert.equal(GetStoreOffer(A.UserId, "single_dye_hairtint_01").remaining, 0);
        });
    });

    it("the Elite Hunt Pass offer reads as owned through the default Elite every account has", async () => {
        const A = await MakePlayer();

        assert.equal(GetStoreOffer(A.UserId, "season09b_premium").remaining, 0);
        assert.deepEqual(ListStoreOffers(A.UserId, "season09b_pass").map((Found) => [Found.id, Found.remaining]), [["season09b_premium", 0]]);
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:169, plus the item log
    it("a cosmetic purchase is recorded in the inventory ledger and the item log, as the store", async () => {
        const A = await MakePlayer();
        assert.equal(Count("inventorytransactions", "characterId = ?", A.CharacterId), 0);

        Buy(A.UserId, "bundle_armour_iron");

        assert.equal(Count("inventorytransactions", "characterId = ?", A.CharacterId), 1, "the grant went through the inventory core");
        const Log = GetDb().$client.prepare("select caller, source, operation, transactionId, quantityChange from inventorylog where characterId = ?").all(A.CharacterId) as any[];
        assert.ok(Log.length > 0);
        assert.ok(Log.every((Row) => Row.caller === "store" && Row.source === "store:bundle_armour_iron" && Row.operation === "add" && Row.quantityChange === 1 && /^store:[0-9a-f]{64}$/.test(Row.transactionId)), JSON.stringify(Log[0]));
        assert.ok(ReadInventory(A.CharacterId).stacked.length > 0, "items were granted");
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:186, adapted: both tokens are issued first, since a
    // token for an offer already owned is refused
    it("an already-owned cosmetic is not granted a second copy", async () => {
        const A = await MakePlayer();
        const First = Token(A.UserId, "bundle_armour_iron"), Second = Token(A.UserId, "bundle_armour_iron");
        Redeem(A.UserId, First);
        Redeem(A.UserId, Second);
        assert.throws(() => Token(A.UserId, "bundle_armour_iron"), { Status: 409 });

        for(const Stack of ReadInventory(A.CharacterId).stacked){
            assert.equal(Stack.quantity, 1, `${Stack.catalogId} should be held once, cosmetics are unlocks`);
        }
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:204 (with the bounty-token bundle on sale; an
    // offer an earlier bundle already owns is not bought again)
    it("every storefront offer can be bought and redeemed", async () => {
        await WithEnv({ STORE_REPEATABLE_TOKENS: "1" }, async () => {
            const A = await MakePlayer();

            BuyEveryOffer(A.UserId, Sellable);

            const Held = ReadInventory(A.CharacterId);
            const Ids = new Set([...Held.stacked.map((Stack) => Stack.catalogId), ...Held.instanced.map((Instance) => Instance.catalogId)]);

            for(const Offer of Sellable){
                for(const Item of Offer.items ?? []){
                    assert.ok(Ids.has(Item.catalogId), `${Offer.id} should have granted ${Item.catalogId}`);
                }
            }

            // Every cosmetic reads as owned; the consumable bundle stays on sale
            for(const Listed of ListStoreOffers(A.UserId, "webstore")){
                const Repeatable = (Listed.items ?? []).length > 0 && Listed.items!.every((Item) => Item.catalogId === "TOKEN_BOUNTY_DRAFT_PREMIUM");
                assert.equal(Listed.remaining, Repeatable ? 1 : 0, `${Listed.id} ownership`);
            }
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:239 (STORE_REPEATABLE_TOKENS=1)
    it("bounty tokens can be bought repeatedly and each purchase adds the full bundle", async () => {
        await WithEnv({ STORE_REPEATABLE_TOKENS: "1" }, async () => {
            const A = await MakePlayer();
            const Sku = "bundle_currency_bounty_small";

            for(let Purchase = 0; Purchase < 2; Purchase++){
                const PurchaseToken = Token(A.UserId, Sku);
                Redeem(A.UserId, PurchaseToken);
                Redeem(A.UserId, PurchaseToken); // a retried redeem adds nothing
            }

            assert.equal(StackQuantity(A.CharacterId, "TOKEN_BOUNTY_DRAFT_PREMIUM"), 40);
            assert.equal(GetStoreOffer(A.UserId, Sku).remaining, 1);
        });
    });

    it("without STORE_REPEATABLE_TOKENS the bounty-token bundle is neither listed nor sold", async () => {
        const A = await MakePlayer();

        assert.ok(!ListStoreOffers(A.UserId, "webstore").some((Listed) => Listed.id === "bundle_currency_bounty_small"));
        assert.equal(ListStoreOffers(A.UserId, "webstore").length, Sellable.length - 1);
        assert.throws(() => Token(A.UserId, "bundle_currency_bounty_small"), { Status: 404 });
        assert.throws(() => GetStoreOffer(A.UserId, "bundle_currency_bounty_small"), { Status: 404 });

        // A token issued while it was on sale is not redeemed after it was taken off
        const Early = await WithEnv({ STORE_REPEATABLE_TOKENS: "1" }, () => Token(A.UserId, "bundle_currency_bounty_small"));
        assert.throws(() => Redeem(A.UserId, Early), { Status: 409 });
        assert.equal(StackQuantity(A.CharacterId, "TOKEN_BOUNTY_DRAFT_PREMIUM"), 0);
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:255
    it("hair tint offers grant the entitlements the client's own tables name", async () => {
        await NoDefaults(async () => {
            const A = await MakePlayer();
            const Expected: Record<string, string[]> = {
                single_dye_hairtint_01: ["ent_cchd_hp07a_01"],
                bundle_hairtint_springtime: ["ent_cchd_hp09a_assassins_01", "ent_cchd_hp09a_assassins_02", "ent_cchd_hp09a_assassins_03", "ent_cchd_hp09a_assassins_04", "ent_cchd_hp09a_assassins_05"]
            };

            for(const [Sku, Names] of Object.entries(Expected)){
                Buy(A.UserId, Sku);

                const Held = EntitlementNames(A.UserId);
                for(const Name of Names){
                    assert.ok(Held.includes(Name), `${Sku} should grant ${Name}`);
                }
                assert.equal(GetStoreOffer(A.UserId, Sku).remaining, 0, `${Sku} should read as owned`);
            }
        });
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:279
    it("store grants land stacked or instanced exactly as the catalogue flag says", async () => {
        const A = await MakePlayer();

        BuyEveryOffer(A.UserId, Sellable.filter((Found) => Found.id !== "bundle_currency_bounty_small"));

        const Held = ReadInventory(A.CharacterId);
        const Stacked = new Set(Held.stacked.map((Stack) => Stack.catalogId));
        const Instanced = new Set(Held.instanced.map((Instance) => Instance.catalogId));

        for(const Offer of Sellable.filter((Found) => Found.id !== "bundle_currency_bounty_small")){
            for(const Item of Offer.items ?? []){
                const Expected = Kinds[Item.catalogId];
                assert.ok(Expected === "stacked" || Expected === "instanced", `${Item.catalogId} has no recorded kind`);
                assert.equal(GrantKind(Item.catalogId), Expected);
                assert.ok((Expected === "stacked" ? Stacked : Instanced).has(Item.catalogId), `${Item.catalogId} should be ${Expected}`);
                assert.ok(!(Expected === "stacked" ? Instanced : Stacked).has(Item.catalogId), `${Item.catalogId} granted both ways`);
            }
        }

        // An instanced grant carries an id of its own, 32 hex characters, and a version of 0
        assert.ok(Held.instanced.every((Instance) => /^[0-9a-f]{32}$/.test(Instance.instanceId) && Instance.updateVersion === 0));
    });
});

describe("limits", () => {
    it("no token for an offer the player already owns (409, over HTTP too); the bounty-token bundle is never owned", async () => {
        const A = await MakePlayer();
        Buy(A.UserId, SKU);

        assert.throws(() => Token(A.UserId), { Status: 409 });
        const Refused = await Call("GET", `/token/platinum/${SKU}`, { as: A.UserId });
        assert.deepEqual([Refused.status, Refused.json], [409, { code: "409", message: "You already own everything this offer grants" }]);
        assert.equal(Count("storepurchases", "accountId = ?", A.UserId), 1, "only the first token was stored");

        await WithEnv({ STORE_REPEATABLE_TOKENS: "1" }, async () => {
            Buy(A.UserId, "bundle_currency_bounty_small");
            assert.match(Token(A.UserId, "bundle_currency_bounty_small"), /^[a-f0-9]{64}$/);
        });
    });

    it(`at most ${MAX_TOKENS_PER_WINDOW} tokens per account in 10 minutes`, async () => {
        const A = await MakePlayer(), B = await MakePlayer();

        for(let Issued = 0; Issued < MAX_TOKENS_PER_WINDOW; Issued++){
            Token(A.UserId);
        }

        assert.throws(() => Token(A.UserId), { Status: 409 });
        assert.equal(Count("storepurchases", "accountId = ?", A.UserId), MAX_TOKENS_PER_WINDOW);
        assert.match(Token(B.UserId), /^[a-f0-9]{64}$/, "per account");

        GetDb().update(storepurchases).set({ createdDate: new Date(Date.now() - 10 * 60 * 1000).toISOString() }).where(eq(storepurchases.accountId, A.UserId)).run();
        assert.match(Token(A.UserId), /^[a-f0-9]{64}$/, "the window has moved on");
    });

    it(`the sweep removes tokens never redeemed and receipts older than ${RECEIPT_DAYS} days; a recent receipt stays`, async () => {
        const A = await MakePlayer(), B = await MakePlayer();
        const Old = Token(A.UserId);
        Redeem(A.UserId, Old);
        Buy(A.UserId, "single_armour_monk_chest");
        Token(B.UserId);

        GetDb().update(storepurchases).set({ redeemedDate: new Date(Date.now() - (RECEIPT_DAYS + 1) * 24 * 60 * 60 * 1000).toISOString() }).where(and(eq(storepurchases.accountId, A.UserId), eq(storepurchases.skuId, SKU))).run();
        GetDb().update(storepurchases).set({ expiresDate: "2000-01-01T00:00:00.000Z" }).where(eq(storepurchases.accountId, B.UserId)).run();

        const Removed = PruneExpiredStorePurchases();
        assert.ok(Removed.Expired >= 1 && Removed.Receipts >= 1, JSON.stringify(Removed));
        assert.deepEqual(GetDb().select().from(storepurchases).where(eq(storepurchases.accountId, A.UserId)).all().map((Row) => Row.skuId), ["single_armour_monk_chest"]);
        assert.equal(Count("storepurchases", "accountId = ?", B.UserId), 0);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 1, "the grant stays");
        assert.equal(Count("inventorylog", "characterId = ? and source = ?", A.CharacterId, `store:${SKU}`), 1, "and so does its log");
    });
});

describe("the catalogue", () => {
    // from Harmonicrain/Undaunted test/entitlements.test.js:306
    it("offer ids are unique", () => {
        const Ids = Object.entries(Catalog).filter(([Key, Value]) => !Key.startsWith("_") && Array.isArray(Value)).flatMap(([, Value]) => Value.map((Found) => Found.id));

        assert.equal(new Set(Ids).size, Ids.length, "duplicate offer ids break the single-offer lookup");
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:315
    it("every storefront offer id is a store item table row with 2:1 art, so its tile is neither blank nor stretched", () => {
        const Missing = Catalog.webstore.filter((Offer) => !ArtRows.has(Offer.id)).map((Offer) => Offer.id);

        assert.deepEqual(Missing, [], `offers without store art: ${Missing.join(", ")}`);
    });

    // from Harmonicrain/Undaunted test/entitlements.test.js:330
    it("no store tab before a populated tab is left empty", () => {
        const Tags = new Set(Catalog.webstore.flatMap((Offer) => Offer.tags));

        // The tab bar indexes the full tab list including hidden (empty) tabs, so an empty tab ahead of a
        // populated one shifts every later tab: EMOTES opened the weapons page
        for(const Required of ["feature", "your_offers", "supplies_boost", "skin_weapon_ac", "skin_weapon_eb", "skin_weapon_dp", "skin_weapon_ih", "skin_weapon_ga", "skin_weapon_ms", "skin_weapon_cb",
            "skin_armour", "skin_lantern", "social_arrival", "social_emote", "dye_armour", "personality_stylekit", "personality_character", "personality_flaresigil", "personality_fabric"]){
            assert.ok(Tags.has(Required), `${Required} must not be empty`);
        }
    });

    it("sells no rank skip, and every offer is free", () => {
        const Offers = Object.entries(Catalog).filter(([Key, Value]) => !Key.startsWith("_") && Array.isArray(Value)).flatMap(([, Value]) => Value);

        assert.deepEqual(Catalog.season09b_rank, []);
        assert.ok(Offers.every((Found) => Found.platinumPrice === 0));
        assert.ok(!JSON.stringify(Offers).includes("CURRENCY_"), "no offer grants a currency");
    });
});

describe("over HTTP", () => {
    // from Harmonicrain/Undaunted test/free-store.test.js:89
    it("authenticates, ignores asked-for grants and prices, and follows the client's token flow", async () => {
        const A = await MakePlayer();

        assert.equal((await Call("GET", `/token/platinum/${SKU}`)).status, 401);

        const Issued = await Call("GET", `/token/platinum/${SKU}?price=-100&quantity=999&catalogId=anything`, { as: A.UserId });
        assert.equal(Issued.status, 200);
        const PurchaseToken = Issued.json.purchaseToken;
        assert.match(PurchaseToken, /^[a-f0-9]{64}$/);

        const Confirm = () => Call("POST", `/notification/platinum?token=${PurchaseToken}`, { as: A.UserId, body: { quantity: 999, price: -100, catalogId: "anything" } });
        const First = await Confirm(), Second = await Confirm();
        assert.deepEqual([First.status, First.text, Second.status], [204, "", 204]);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 1);
        assert.equal(StackQuantity(A.CharacterId, "anything"), 0);

        // Every listed offer is free and sits in a real store category, not just the webstore request filter
        const Listed = await Call("GET", "/product/skus/public?requiredTags=webstore", { as: A.UserId });
        assert.equal(Listed.status, 200);
        assert.ok(Array.isArray(Listed.json), "a bare array");
        assert.ok(Listed.json.every((Found: any) => Found.platinumPrice === 0 && Found.tags.some((Tag: string) => Tag !== "webstore")));
        assert.equal(Listed.json.find((Found: any) => Found.id === SKU).remaining, 0);

        const Single = await Call("GET", `/product/sku/${SKU}`, { as: A.UserId });
        assert.deepEqual([Single.status, Single.json.id, Single.json.remaining], [200, SKU, 0]);
    });

    it("a game server's key alone gets 403 on every store route", async () => {
        const A = await MakePlayer();
        const PurchaseToken = Token(A.UserId);

        for(const [Method, Path] of [["GET", "/product/skus/public?requiredTags=webstore"], ["GET", `/product/sku/${SKU}`], ["GET", `/token/platinum/${SKU}`], ["POST", `/notification/platinum?token=${PurchaseToken}`]]){
            assert.equal((await Call(Method, Path, { gs: true })).status, 403, `${Method} ${Path}`);
        }
        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
    });

    it("answers the store's refusals with a code and a message", async () => {
        const A = await MakePlayer();

        const Missing = await Call("GET", "/product/skus/public", { as: A.UserId });
        assert.deepEqual([Missing.status, Missing.json], [400, { code: "400", message: "missing requiredTags query parameter" }]);

        const Unknown = await Call("GET", "/product/skus/public?requiredTags=not_a_tag", { as: A.UserId });
        assert.deepEqual([Unknown.status, Unknown.json], [200, []]);

        assert.deepEqual((await Call("GET", "/product/skus/public?requiredTags=season09b_rank", { as: A.UserId })).json, [], "no rank skip");
        assert.equal((await Call("GET", "/product/sku/season09b_10_ranks", { as: A.UserId })).status, 404);
        assert.equal((await Call("GET", "/token/notes/single_armour_monk_chest", { as: A.UserId })).status, 400);
        assert.equal((await Call("POST", "/notification/platinum?token=nope", { as: A.UserId })).status, 400);
        assert.equal((await Call("POST", "/notification/platinum", { as: A.UserId })).status, 400);
    });

    // Lady Luck and the Middleman (on by default) share the purchase routes, so they are off here too
    it("with STORE=off (the default) the store answers as it did before", async () => {
        await WithEnv({ STORE: undefined, TRIALS_STORE: "off", MIDDLEMAN_STORE: "off" }, async () => {
            const A = await MakePlayer();

            const Listed = await Call("GET", "/product/skus/public?requiredTags=webstore", { as: A.UserId });
            assert.deepEqual([Listed.status, Listed.json], [400, { code: "400", message: "The store is not available on Dauntless Revived yet." }]);
            assert.equal((await Call("GET", "/product/skus/public?requiredTags=webstore", { gs: true })).status, 400, "the game server's key, as before");

            for(const [Method, Path] of [["GET", `/product/sku/${SKU}`], ["GET", `/token/platinum/${SKU}`], ["POST", `/notification/platinum?token=${"a".repeat(64)}`]]){
                assert.equal((await Call(Method, Path, { as: A.UserId })).status, 404, `${Method} ${Path}`);
                assert.equal((await Call(Method, Path)).status, 404, `${Method} ${Path} without a token`);
            }
            assert.equal(Count("storepurchases", "accountId = ?", A.UserId), 0);
        });
    });
});

describe("non-platinum currency pricing (mechanism only; the shipped catalogue stays free)", () => {
    const CellDustOffer: StoreOffer = {
        id: "test_celldust_offer",
        tags: ["webstore"],
        platinumPrice: null as unknown as number,
        cellDustPrice: 500,
        items: [{ catalogId: ITEM, quantity: 1 }],
        entitlements: null,
        remaining: 1
    };

    const AmbiguousOffer: StoreOffer = {
        ...CellDustOffer,
        id: "test_ambiguous_offer",
        platinumPrice: 0
    };

    beforeEach(() => {
        SetExtraOffersForTests([CellDustOffer, AmbiguousOffer]);
    });

    after(() => {
        SetExtraOffersForTests();
    });

    function Credit(CharacterId: string, CatalogId: string, Quantity: number){
        GetDb().insert(inventory).values({ characterId: CharacterId, instancedItems: "[]", stackedItems: JSON.stringify([{ catalogId: CatalogId, quantity: Quantity }]) }).run();
    }

    it("charges the exact amount from the matching currency stack and grants the item", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_CELLDUST", 750);

        const Token = (await Call("GET", `/token/celldust/${CellDustOffer.id}`, { as: A.UserId })).json.purchaseToken;
        const Confirm = await Call("POST", `/notification/celldust?token=${Token}`, { as: A.UserId });

        assert.equal(Confirm.status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_CELLDUST"), 250);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 1);
    });

    it("refuses the token when the balance is short, and grants or charges nothing", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_CELLDUST", 100);

        assert.equal((await Call("GET", `/token/celldust/${CellDustOffer.id}`, { as: A.UserId })).status, 409);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_CELLDUST"), 100);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
        assert.equal(Count("storepurchases", "accountId = ?", A.UserId), 0);
    });

    it("refuses redeem if the balance drops below the price after the token is issued", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_CELLDUST", 500);

        const Token = (await Call("GET", `/token/celldust/${CellDustOffer.id}`, { as: A.UserId })).json.purchaseToken;

        GetDb().update(inventory).set({ stackedItems: JSON.stringify([{ catalogId: "CURRENCY_CELLDUST", quantity: 10 }]) }).where(eq(inventory.characterId, A.CharacterId)).run();

        const Confirm = await Call("POST", `/notification/celldust?token=${Token}`, { as: A.UserId });
        assert.equal(Confirm.status, 409);
        assert.equal(StackQuantity(A.CharacterId, ITEM), 0);
    });

    it("refuses a currency that does not match the offer's own price field", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_PLATINUM_UNIV", 1000);

        assert.equal((await Call("GET", `/token/platinum/${CellDustOffer.id}`, { as: A.UserId })).status, 400);
    });

    it("refuses an offer with more than one price field set", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_CELLDUST", 1000);

        assert.equal((await Call("GET", `/token/celldust/${AmbiguousOffer.id}`, { as: A.UserId })).status, 500);
    });

    it("refuses an unrecognised currency string", async () => {
        const A = await MakePlayer();

        assert.equal((await Call("GET", `/token/event01/${CellDustOffer.id}`, { as: A.UserId })).status, 400);
    });

    it("a retried confirm does not charge twice", async () => {
        const A = await MakePlayer();
        Credit(A.CharacterId, "CURRENCY_CELLDUST", 500);

        const Token = (await Call("GET", `/token/celldust/${CellDustOffer.id}`, { as: A.UserId })).json.purchaseToken;

        const First = await Call("POST", `/notification/celldust?token=${Token}`, { as: A.UserId });
        const Second = await Call("POST", `/notification/celldust?token=${Token}`, { as: A.UserId });

        assert.deepEqual([First.status, Second.status], [204, 204]);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_CELLDUST"), 0);
    });
});

it("sheens cannot be listed, looked up, issued or redeemed from an outstanding token", async () => {
    const A = await MakePlayer();
    const Sheen = "single_dye_sheen_glossy";
    for (const Tag of Object.keys(Catalog)) {
        assert.ok(!ListStoreOffers(A.UserId, Tag).some(Offer => Offer.id.startsWith("single_dye_sheen_") || Offer.entitlements?.some(Grant => Grant.name.startsWith("ent_dye_sheen_"))), Tag);
    }
    assert.throws(() => GetStoreOffer(A.UserId, Sheen), { Status: 404 });
    assert.throws(() => Token(A.UserId, Sheen), { Status: 404 });
    const Pending = Token(A.UserId, "single_dye_hairtint_01");
    GetDb().update(storepurchases).set({ skuId: Sheen }).where(eq(storepurchases.accountId, A.UserId)).run();
    assert.throws(() => Redeem(A.UserId, Pending), { Status: 409, message: "Offer is no longer sold; request a new token" });
    assert.ok(!EntitlementNames(A.UserId).includes("ent_dye_sheen_glossy"));
    assert.equal(GetDb().select().from(storepurchases).where(eq(storepurchases.accountId, A.UserId)).get()!.redeemedDate, null);
    const Disguised: StoreOffer = {id:"renamed_sheen_bundle",tags:["webstore"],platinumPrice:0,items:null,entitlements:[{name:"ent_dye_sheen_metallic"}],remaining:1};
    SetExtraOffersForTests([Disguised]);
    try {
        assert.throws(() => GetStoreOffer(A.UserId, Disguised.id), { Status: 404 });
        assert.throws(() => Token(A.UserId, Disguised.id), { Status: 404 });
    } finally { SetExtraOffersForTests(); }
});
it("a real dye pack charges 5000 Rams once and cannot grant while funds are short", async () => {
    const A = await MakePlayer();
    await WithEnv({STORE_RAMS_TEST_ACCOUNT:A.UserId, STORE_WEEKLY_ROTATION:undefined}, () => {
        const Sku = "bundle_dyes_palette_01";
        const Colours = ["DYE_BLUE07_DURABLE", "DYE_BLUE08_DURABLE"];
        GetDb().insert(inventory).values({characterId:A.CharacterId,instancedItems:"[]",stackedItems:JSON.stringify([{catalogId:"CURRENCY_NOTES",quantity:4999}])}).run();
        const Before = ReadInventory(A.CharacterId);
        assert.equal(GetStoreOffer(A.UserId, Sku).notesPrice, 5000);
        assert.throws(() => CreateStorePurchase(A.UserId, "notes", Sku), {Status:409});
        assert.deepEqual(ReadInventory(A.CharacterId), Before);
        GetDb().update(inventory).set({stackedItems:JSON.stringify([{catalogId:"CURRENCY_NOTES",quantity:10000}])}).where(eq(inventory.characterId,A.CharacterId)).run();
        const Pending = CreateStorePurchase(A.UserId,"notes",Sku).purchaseToken;
        RedeemStorePurchase(A.UserId,"notes",Pending);
        assert.equal(StackQuantity(A.CharacterId,"CURRENCY_NOTES"),5000);
        for(const Colour of Colours) assert.equal(StackQuantity(A.CharacterId,Colour),1);
        const Purchased = ReadInventory(A.CharacterId);
        RedeemStorePurchase(A.UserId,"notes",Pending);
        assert.deepEqual(ReadInventory(A.CharacterId), Purchased);
    });
});