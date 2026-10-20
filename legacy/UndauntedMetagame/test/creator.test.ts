import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, before, it } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { characters, inventory, inventorytransactions, onetimejobs, users } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { CreateCharacterForUid } from "../src/controllers/character";
import { GetCreatorSupport, RedeemCreatorCode } from "../src/controllers/creator";
import { Call, StartApp, StopApp } from "./appclient";
import { MakePlayer, StackQuantity } from "./helpers";

before(StartApp);
after(async () => { await StopApp(); RemoveTestDb(() => GetDb().$client.close()); });

it("native POST grants exactly 10000 Rams to the authenticated account; GET never grants", async () => {
    const A = await MakePlayer(), B = await MakePlayer();
    for(let I = 0; I < 3; I++){
        const R = await Call("GET", "/creator?slug=Void", {as: A.UserId});
        assert.equal(R.status, 200);
        assert.equal(R.json.success, false);
    }
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 0);
    const R = await Call("POST", "/creator", {body: {
        slug: "Void", authorization: SignMetagameJWTForUid(A.UserId),
        accountId: B.UserId, characterId: B.CharacterId, amount: 999999
    }});
    assert.equal(R.status, 200);
    assert.equal(R.json.slug, "Void");
    assert.equal(R.json.grantedRams, 10000);
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 10000);
    assert.equal(StackQuantity(B.CharacterId, "CURRENCY_NOTES"), 0);
    const Status = await Call("GET", "/creator", {headers: {"x-epic-authorization": SignMetagameJWTForUid(A.UserId)}});
    assert.equal(Status.json.slug, "Void");
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 10000);
});

it("case/whitespace variants and concurrent submissions cannot give a second reward", async () => {
    const A = await MakePlayer();
    const Replies = await Promise.all([" Void ", "VOID", "void", "Void"].map(slug => Call("POST", "/creator", {as: A.UserId, body: {slug}})));
    assert.equal(Replies.filter(R => R.status === 200).length, 1);
    assert.equal(Replies.filter(R => R.status === 409).length, 3);
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 10000);
});

it("independent backend processes serialize competing account claims", async () => {
    const A = await MakePlayer();
    const Code = `const c=require(${JSON.stringify(require.resolve("../src/controllers/creator"))});
        try { c.RedeemCreatorCode(${JSON.stringify(A.UserId)},'Void'); console.log(200); }
        catch(e) { console.log(e.Status || 500); }`;
    const Claim = () => new Promise<number>((resolve, reject) => {
        const Child = spawn(process.execPath, ["-e", Code], {env: process.env, timeout: 10000});
        let Output = "";
        Child.stdout.on("data", Data => Output += Data);
        Child.on("error", reject);
        Child.on("exit", Exit => Exit === 0 ? resolve(Number(Output.trim())) : reject(new Error("Claim worker failed")));
    });
    assert.deepEqual((await Promise.all([Claim(), Claim()])).sort(), [200, 409]);
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 10000);
    assert.equal(await Claim(), 409, "a fresh process still sees the permanent receipt");
});

it("receipt is account-wide and survives inventory receipt pruning", async () => {
    const A = await MakePlayer();
    RedeemCreatorCode(A.UserId, "Void");
    const Second = await CreateCharacterForUid(A.UserId, "Second");
    GetDb().update(characters).set({updateVersion: 500}).where(eq(characters.characterId, Second.id)).run();
    GetDb().delete(inventorytransactions).where(eq(inventorytransactions.userId, A.UserId)).run();
    assert.throws(() => RedeemCreatorCode(A.UserId, "VOID"), /already been redeemed/);
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 10000);
    assert.equal(StackQuantity(Second.id, "CURRENCY_NOTES"), 0);
    assert.equal(GetCreatorSupport(A.UserId).success, true);
});

it("selects the active owned character and lets different accounts redeem independently", async () => {
    const A = await MakePlayer(), B = await MakePlayer();
    const Second = await CreateCharacterForUid(A.UserId, "Active");
    GetDb().update(characters).set({updateVersion: 500}).where(eq(characters.characterId, Second.id)).run();
    RedeemCreatorCode(A.UserId, "Void");
    RedeemCreatorCode(B.UserId, "Void");
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 0);
    assert.equal(StackQuantity(Second.id, "CURRENCY_NOTES"), 10000);
    assert.equal(StackQuantity(B.CharacterId, "CURRENCY_NOTES"), 10000);
});

it("rejects missing/unknown/structured codes without consuming the reward", async () => {
    const A = await MakePlayer();
    for(const slug of [undefined, null, "", "Other", "Void2", ["Void"], {code: "Void"}, "V".repeat(100)]){
        const R = await Call("POST", "/creator", {as: A.UserId, body: {slug}});
        assert.equal(R.status, 404);
    }
    assert.equal(GetCreatorSupport(A.UserId).success, false);
    assert.equal((await Call("POST", "/creator", {as: A.UserId, body: {slug: "Void"}})).status, 200);
});

it("requires a valid player token, including native token fields", async () => {
    const A = await MakePlayer();
    assert.equal((await Call("POST", "/creator", {body: {slug: "Void", accountId: A.UserId}})).status, 401);
    assert.equal((await Call("POST", "/creator", {gs: true, body: {slug: "Void"}})).status, 403);
    assert.equal((await Call("POST", "/creator", {body: {slug: "Void", authorization: "invalid"}})).status, 401);
    assert.equal((await Call("GET", "/creator", {headers: {"x-epic-authorization": "invalid"}})).status, 401);
    assert.equal((await Call("POST", "/creator", {body: {slug: "Void", authorization: SignMetagameJWTForUid(A.UserId)}, headers: {authorization: "bearer invalid"}})).status, 401);
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 0);
});

it("failed inventory grants roll back the account receipt and can be retried", async () => {
    const A = await MakePlayer();
    GetDb().insert(inventory).values({characterId: A.CharacterId, stackedItems: "{", instancedItems: "[]"}).run();
    const R = await Call("POST", "/creator", {as: A.UserId, body: {slug: "Void"}});
    assert.equal(R.status, 409);
    assert.equal(GetCreatorSupport(A.UserId).success, false);
    GetDb().update(inventory).set({stackedItems: "[]"}).where(eq(inventory.characterId, A.CharacterId)).run();
    assert.equal((await Call("POST", "/creator", {as: A.UserId, body: {slug: "Void"}})).status, 200);
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 10000);
});

it("a missing character does not burn the account's code", async () => {
    const UserId = `UID-no-character-${process.pid}`;
    GetDb().insert(users).values({userId: UserId, name: "NoCharacter", notes: 0}).run();
    assert.throws(() => RedeemCreatorCode(UserId, "Void"), /Create a character/);
    assert.equal(GetCreatorSupport(UserId).success, false);
    const C = await CreateCharacterForUid(UserId, "NowCreated");
    RedeemCreatorCode(UserId, "Void");
    assert.equal(StackQuantity(C.id, "CURRENCY_NOTES"), 10000);
});

it("invalid or overflowing native Rams balances do not consume a reward", async () => {
    const A = await MakePlayer();
    GetDb().insert(inventory).values({characterId: A.CharacterId, stackedItems: "[]", instancedItems: "[]"}).run();
    for(const quantity of [-1, "68000", 2147483647, 1.5]){
        GetDb().update(inventory).set({stackedItems: JSON.stringify([{catalogId: "CURRENCY_NOTES", quantity}])}).where(eq(inventory.characterId, A.CharacterId)).run();
        assert.equal((await Call("POST", "/creator", {as: A.UserId, body: {slug: "Void"}})).status, 409);
        assert.equal(GetCreatorSupport(A.UserId).success, false);
    }
    GetDb().update(inventory).set({stackedItems: JSON.stringify([{catalogId: "CURRENCY_NOTES", quantity: 2147473647}])}).where(eq(inventory.characterId, A.CharacterId)).run();
    RedeemCreatorCode(A.UserId, "Void");
    assert.equal(StackQuantity(A.CharacterId, "CURRENCY_NOTES"), 2147483647);
});

it("the audit log records the grant source and only one permanent receipt", async () => {
    const A = await MakePlayer();
    RedeemCreatorCode(A.UserId, "Void");
    const Log = GetDb().$client.prepare("SELECT catalogId, quantityChange, caller, source FROM inventorylog WHERE userId=?").all(A.UserId);
    assert.deepEqual(Log, [{catalogId: "CURRENCY_NOTES", quantityChange: 10000, caller: "store", source: "creator:Void"}]);
    assert.equal(GetDb().select().from(onetimejobs).where(eq(onetimejobs.jobId, `creator:void:${A.UserId}`)).all().length, 1);
});
