import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { eq } from "drizzle-orm";
import { GetDb } from "../src/db";
import { loadouts } from "../src/db/schema";
import { GetAllLoadoutsForUserIdAndCharacterId, GetPersistentLoadoutForUserIdAndCharacterId, GetSlotCountReply, RealLoadoutPayload, SetActiveLoadoutSlot, SetLoadoutSlotData, UnlockLoadoutSlots } from "../src/controllers/loadout";
import { GetSaveHistory } from "../src/controllers/savehistory";
import { MakePlayer } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

async function AllPayload(UserId: string, CharacterId: string){
    const Loadouts = await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);
    const Persistent = await GetPersistentLoadoutForUserIdAndCharacterId(UserId, CharacterId);

    return RealLoadoutPayload(CharacterId, Loadouts, Persistent);
}

const Counts = (Payload: any) => [Payload.num_account_slots, Payload.max_account_slots, Payload.num_character_slots, Payload.max_character_slots];

describe("loadout slots (real mode)", () => {
    it("starts at 1 character slot, max 5 plus the account slot, all four counts numbers", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        const Payload = await AllPayload(UserId, CharacterId);

        assert.deepEqual(Counts(Payload), [1, 1, 1, 5]);
        assert.equal(Payload.active_index, 0);
        assert.equal(Payload.needs_migration, false);
        assert.equal(Payload.loadouts.length, 1);
    });

    it("unlock/3 adds 3 (the game server sends the difference), and /all, /slotcount and the reply agree", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);

        const Reply = UnlockLoadoutSlots(UserId, CharacterId, "3", "gameserver");
        assert.equal(Reply.Status, 200);
        assert.deepEqual(Reply.Body, {code: null, message: "OK", payload: {num_account_slots: 1, max_account_slots: 1, num_character_slots: 4, max_character_slots: 5}});
        assert.deepEqual(GetSlotCountReply(CharacterId), Reply.Body);
        assert.deepEqual(Counts(await AllPayload(UserId, CharacterId)), [1, 1, 4, 5]);

        // What the client computes next: 1 + 3 conditions met - 4 = 0, so no more unlocks
        assert.equal(1 + 3 - (Reply.Body as any).payload.num_character_slots, 0);
    });

    it("caps at 5 character slots so the account slot keeps the client total at 6", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        UnlockLoadoutSlots(UserId, CharacterId, "4", "gameserver");
        const Capped = UnlockLoadoutSlots(UserId, CharacterId, "3", "gameserver");
        assert.deepEqual((Capped.Body as any).payload, {num_account_slots: 1, max_account_slots: 1, num_character_slots: 5, max_character_slots: 5});

        for(const Bad of ["0", "x", "-1", "1.5"]){
            assert.equal(UnlockLoadoutSlots(UserId, CharacterId, Bad, "gameserver").Status, 400, Bad);
        }
        assert.equal((GetSlotCountReply(CharacterId).payload as any).num_character_slots, 5);
    });

    it("saves any unlocked slot by its slot index, refuses slots not unlocked yet", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);

        // 2 slots (0 and 1) until an unlock
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "2", JSON.stringify({slot_index: 2, flask: "FL_TWO"})), false);

        UnlockLoadoutSlots(UserId, CharacterId, "3", "gameserver");
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "2", JSON.stringify({slot_index: 2, flask: "FL_TWO"})), true);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "2", JSON.stringify({slot_index: 2, flask: "FL_TWO_B"})), true);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "1", JSON.stringify({slot_index: 7, flask: "FL_ONE"})), true);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "0", JSON.stringify({slot_index: 0, flask: "FL_ZERO"})), true);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "persistent", JSON.stringify({banner: "BN_X"})), true);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "4", "[]"), false);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "5", "{}"), false);

        const Payload = await AllPayload(UserId, CharacterId);
        assert.deepEqual(Payload.loadouts.map((Element: any) => [Element.slot_index, Element.flask]), [[0, "FL_ZERO"], [1, "FL_ONE"], [2, "FL_TWO_B"]]);
        assert.equal(Payload.persistent.banner, "BN_X");

        const [History] = await GetSaveHistory([CharacterId]);
        assert.ok(History.LoadoutVersions.length >= 5);
    });

    it("creates the loadout row when a valid slot save arrives before the initial /all read", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "0", JSON.stringify({slot_index: 0, flask: "FL_EARLY"})), true);
        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "persistent", JSON.stringify({banner: "BN_EARLY"})), true);
        const Payload = await AllPayload(UserId, CharacterId);

        assert.equal(Payload.loadouts[0].flask, "FL_EARLY");
        assert.equal(Payload.persistent.banner, "BN_EARLY");
    });

    it("supports all six client slots without advertising a seventh", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);
        UnlockLoadoutSlots(UserId, CharacterId, "9", "gameserver");

        for(let Index = 0; Index < 6; Index++){
            assert.equal(SetLoadoutSlotData(UserId, CharacterId, String(Index), JSON.stringify({slot_index: Index, custom_name: `Slot ${Index + 1}`})), true);
        }

        assert.equal(SetLoadoutSlotData(UserId, CharacterId, "6", JSON.stringify({slot_index: 6})), false);
        const Payload = await AllPayload(UserId, CharacterId);
        assert.deepEqual(Counts(Payload), [1, 1, 5, 5]);
        assert.deepEqual(Payload.loadouts.map((Element: any) => Element.slot_index), [0, 1, 2, 3, 4, 5]);
    });

    it("stores the active slot, only below the number of slots", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);

        // 1 account slot + 1 character slot = slots 0 and 1 before any unlock
        assert.equal(SetActiveLoadoutSlot(UserId, CharacterId, "1", "gameserver").Status, 200);
        assert.equal(SetActiveLoadoutSlot(UserId, CharacterId, "2", "gameserver").Status, 400);
        UnlockLoadoutSlots(UserId, CharacterId, "3", "gameserver");
        assert.deepEqual(SetActiveLoadoutSlot(UserId, CharacterId, "2", "gameserver"), {Status: 200, Body: {code: null, message: "OK"}});
        assert.equal((await AllPayload(UserId, CharacterId)).active_index, 2);
        assert.equal(SetActiveLoadoutSlot(UserId, CharacterId, "5", "gameserver").Status, 400);
        assert.equal(SetActiveLoadoutSlot(UserId, CharacterId, "x", "gameserver").Status, 400);
    });

    it("an element stored past the slot count is not sent", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetAllLoadoutsForUserIdAndCharacterId(UserId, CharacterId);
        const Row = GetDb().select().from(loadouts).where(eq(loadouts.characterId, CharacterId)).get()!;
        GetDb().update(loadouts).set({loadouts: JSON.stringify([...JSON.parse(Row.loadouts), {slot_index: 3, flask: "FL_STRAY"}])}).where(eq(loadouts.characterId, CharacterId)).run();

        assert.deepEqual((await AllPayload(UserId, CharacterId)).loadouts.map((Element: any) => Element.slot_index), [0]);
    });
});
