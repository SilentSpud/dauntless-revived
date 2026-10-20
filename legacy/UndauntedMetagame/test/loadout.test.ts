import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { loadouts } from "../src/db/schema";
import { GetAllLoadoutsForUserIdAndCharacterId, GetPersistentLoadoutForUserIdAndCharacterId, SetLoadoutDataForUserIdAndCharacterId } from "../src/controllers/loadout";
import { GetSaveHistory, RollbackLoadout } from "../src/controllers/savehistory";
import { MakePlayer } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

const ReadLoadoutRow = (CharacterId: string) => GetDb().select().from(loadouts).where(eq(loadouts.characterId, CharacterId)).get();

describe("SetLoadoutDataForUserIdAndCharacterId", () => {
    it("fails when no loadout row matches (it used to report success and drop the save)", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "persistent", "{}"), false);
        assert.equal(ReadLoadoutRow(CharacterId), undefined);
    });

    it("fails for another player's character", async () => {
        const A = await MakePlayer();
        const B = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(B.UserId, B.CharacterId);
        const Before = ReadLoadoutRow(B.CharacterId);

        assert.equal(SetLoadoutDataForUserIdAndCharacterId(A.UserId, B.CharacterId, "persistent", "{\"x\":1}"), false);
        assert.deepEqual(ReadLoadoutRow(B.CharacterId), Before);
    });

    it("refuses data that is not JSON instead of storing an unloadable loadout", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);
        const Before = ReadLoadoutRow(CharacterId);

        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "persistent", "{broken"), false);
        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "0", "{broken"), false);
        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "0", undefined as any), false);
        assert.deepEqual(ReadLoadoutRow(CharacterId), Before);
    });

    it("still refuses unsupported slot indexes", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);

        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "1", "{}"), false);
    });

    it("saves slot 0 and persistent, keeping each version", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);

        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "0", JSON.stringify({slot_index: 0, flask: "FL_A"})), true);
        assert.equal(SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "persistent", JSON.stringify({banner: "BN_A"})), true);

        const Row = ReadLoadoutRow(CharacterId)!;
        assert.equal(JSON.parse(Row.loadouts)[0].flask, "FL_A");
        assert.equal(JSON.parse(Row.persistent).banner, "BN_A");

        const [History] = await GetSaveHistory([CharacterId]);
        assert.deepEqual(History.LoadoutVersions.map((Version) => [Version.Version, Version.Reason]), [[3, "save"], [2, "save"], [1, "baseline"]]);
    });
});

describe("loadout initialization", () => {
    it("can read persistent data before /all and creates one consistent row", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        const Persistent = await GetPersistentLoadoutForUserIdAndCharacterId(UserId, CharacterId);
        const LoadoutData = await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);

        assert.equal(Persistent.banner, "BN_BEGINNER_00");
        assert.equal(LoadoutData.length, 1);
        assert.equal(ReadLoadoutRow(CharacterId)?.userId, UserId);
    });
});

describe("RollbackLoadout", () => {
    it("rolls the loadouts back one version", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);
        SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "persistent", JSON.stringify({banner: "BN_A"}));
        SetLoadoutDataForUserIdAndCharacterId(UserId, CharacterId, "persistent", JSON.stringify({banner: "BN_B"}));

        assert.deepEqual(RollbackLoadout(CharacterId, 2, "UID-admin"), {success: true, data: {CharacterId, RolledBackTo: 2, Version: 4}});
        assert.equal(JSON.parse(ReadLoadoutRow(CharacterId)!.persistent).banner, "BN_A");
        assert.deepEqual(RollbackLoadout(CharacterId, 99, "UID-admin"), {success: false, error: "version_not_found"});
    });
});
