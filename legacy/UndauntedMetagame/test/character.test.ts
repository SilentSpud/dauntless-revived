import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { ProcessTriggers, UpdateCharacterForUid } from "../src/controllers/character";
import { FormatCharacterDate, GetSaveHistory, RollbackCharacter } from "../src/controllers/savehistory";
import { characterhistory } from "../src/db/schema";
import { Count, MakePlayer, ReadCharacter } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

const FTUE = (Objectives: object) => JSON.stringify(Objectives);
const DOJO_OBJECTIVE = "929A333B40E413C41E47B0A425EC3349";

describe("ProcessTriggers", () => {
    it("injects SERIE_dojo when the FTUE objective is complete", () => {
        const Out = JSON.parse(ProcessTriggers(JSON.stringify({SERIE_cr19_series_1_ftue: FTUE({[DOJO_OBJECTIVE]: {Status: 3}})})));

        assert.equal(typeof Out.SERIE_dojo, "string");
        assert.equal(JSON.parse(Out.SERIE_dojo).ID, "Dojo");
    });

    it("saves unchanged when the FTUE objective is missing (this used to throw and block every save)", () => {
        const In = JSON.stringify({SERIE_cr19_series_1_ftue: FTUE({}), PlayerAccountProgressStep: "EnteredRamsgate"});

        assert.equal(ProcessTriggers(In), In);
    });

    it("saves unchanged when the FTUE value is not JSON", () => {
        const In = JSON.stringify({SERIE_cr19_series_1_ftue: "not json"});

        assert.equal(ProcessTriggers(In), In);
    });

    it("never overwrites an existing SERIE_dojo", () => {
        const In = JSON.stringify({SERIE_cr19_series_1_ftue: FTUE({[DOJO_OBJECTIVE]: {Status: 3}}), SERIE_dojo: "kept"});

        assert.equal(JSON.parse(ProcessTriggers(In)).SERIE_dojo, "kept");
    });

    it("refuses data that is not a JSON object", () => {
        for(const Bad of ["", "{", "null", "[]", "5", "\"text\""]){
            assert.throws(() => ProcessTriggers(Bad), /not valid JSON|not a JSON object/);
        }
    });
});

describe("UpdateCharacterForUid", () => {
    it('acknowledges identical committed retries without writing another history entry', async () => {
        const {UserId, CharacterId} = await MakePlayer();
        assert.deepEqual(UpdateCharacterForUid(CharacterId,UserId,'{"hunt":"complete","reward":1}',1),{success:true});
        const before = Count('characterhistory','characterId = ?',CharacterId);
        assert.deepEqual(UpdateCharacterForUid(CharacterId,UserId,'{"reward":1,"hunt":"complete"}',1),{success:true});
        assert.equal(Count('characterhistory','characterId = ?',CharacterId),before);
        assert.equal(UpdateCharacterForUid(CharacterId,UserId,'{"hunt":"incomplete","reward":0}',1).success,false);
        assert.equal(UpdateCharacterForUid(CharacterId,UserId,'{"hunt":"complete","reward":1}',0).success,false);
    });
    it("answers not_found for a character that is not the caller's", async () => {
        const A = await MakePlayer();
        const B = await MakePlayer();

        assert.deepEqual(UpdateCharacterForUid(B.CharacterId, A.UserId, "{}", 1), {success: false, error: "not_found", storedVersion: undefined});
        assert.equal(UpdateCharacterForUid(crypto.randomUUID(), A.UserId, "{}", 1).success, false);
        assert.equal(ReadCharacter(B.CharacterId).updateVersion, 0);
    });

    it("refuses a version that is not newer, and the second of two saves racing for one version", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        assert.deepEqual(UpdateCharacterForUid(CharacterId, UserId, "{\"a\":\"1\"}", 1), {success: true});
        assert.deepEqual(UpdateCharacterForUid(CharacterId, UserId, "{\"a\":\"2\"}", 1), {success: false, error: "conflict", storedVersion: 1});
        assert.deepEqual(UpdateCharacterForUid(CharacterId, UserId, "{\"a\":\"0\"}", 0), {success: false, error: "conflict", storedVersion: 1});
        assert.equal(ReadCharacter(CharacterId).data, "{\"a\":\"1\"}");
    });

    it("refuses invalid data without touching the stored character", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        assert.deepEqual(UpdateCharacterForUid(CharacterId, UserId, null as any, 1), {success: false, error: "invalid_data", storedVersion: undefined});
        assert.deepEqual(UpdateCharacterForUid(CharacterId, UserId, "{oops", 1), {success: false, error: "invalid_data", storedVersion: undefined});
        assert.equal(ReadCharacter(CharacterId).updateVersion, 0);
        assert.equal(ReadCharacter(CharacterId).data, "{}");
    });

    it("sets lastModifiedDate in the createdDate format", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        UpdateCharacterForUid(CharacterId, UserId, "{}", 1);

        assert.equal(ReadCharacter(CharacterId).lastModifiedDate, FormatCharacterDate(new Date()));
        assert.match(ReadCharacter(CharacterId).lastModifiedDate, /^[A-Z][a-z]{2} \d{1,2}, \d{4}$/);
    });

    it("keeps a baseline plus each saved version, pruned to SAVE_HISTORY_KEEP", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        process.env.SAVE_HISTORY_KEEP = "4";

        try{
            for(let Version = 1; Version <= 6; Version++){
                assert.ok(UpdateCharacterForUid(CharacterId, UserId, JSON.stringify({v: String(Version)}), Version).success);
            }
        }
        finally{
            delete process.env.SAVE_HISTORY_KEEP;
        }

        const [History] = await GetSaveHistory([CharacterId]);
        assert.deepEqual(History.CharacterVersions.map((Version) => Version.Version), [6, 5, 4, 3]);
        assert.equal(Count("characterhistory", "characterId = ?", CharacterId), 4);
    });

    it("also keeps the last version of each hour (48 h) and of each day (30 days)", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        const HourStart = Math.floor(Date.now() / 3600000) * 3600000 - 5 * 3600000;
        const NoonThreeDaysAgo = Math.floor(Date.now() / 86400000) * 86400000 - 3 * 86400000 + 12 * 3600000;
        const Old = (Version: number, Time: number) => GetDb().insert(characterhistory).values({
            characterId: CharacterId, userId: UserId, updateVersion: Version, name: "Old", data: "{}", savedDate: new Date(Time).toISOString(), reason: "save"
        }).run();

        Old(1, Date.now() - 40 * 86400000);
        Old(2, NoonThreeDaysAgo);
        Old(3, NoonThreeDaysAgo + 60000);
        Old(4, HourStart + 10 * 60000);
        Old(5, HourStart + 20 * 60000);

        process.env.SAVE_HISTORY_KEEP = "4";

        try{
            for(let Version = 10; Version <= 15; Version++){
                assert.ok(UpdateCharacterForUid(CharacterId, UserId, JSON.stringify({v: String(Version)}), Version).success);
            }
        }
        finally{
            delete process.env.SAVE_HISTORY_KEEP;
        }

        // Newest 4, the last of 5 hours ago, the last of 3 days ago; not 40 days ago
        const [History] = await GetSaveHistory([CharacterId]);
        assert.deepEqual(History.CharacterVersions.map((Version) => Version.Version), [15, 14, 13, 12, 5, 3]);
    });
});

describe("RollbackCharacter", () => {
    it("rolls a character back one version under a new, higher updateVersion", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        UpdateCharacterForUid(CharacterId, UserId, "{\"step\":\"one\"}", 1);
        UpdateCharacterForUid(CharacterId, UserId, "{\"step\":\"two\"}", 2);

        const Result = RollbackCharacter(CharacterId, 1, "UID-admin");

        assert.deepEqual(Result, {success: true, data: {CharacterId, RolledBackTo: 1, PreviousVersion: 2, UpdateVersion: 3}});
        assert.equal(ReadCharacter(CharacterId).data, "{\"step\":\"one\"}");
        assert.equal(ReadCharacter(CharacterId).updateVersion, 3);

        // A stale save still in flight from before the rollback is refused
        assert.equal(UpdateCharacterForUid(CharacterId, UserId, "{\"step\":\"stale\"}", 3).success, false);
        assert.equal(ReadCharacter(CharacterId).data, "{\"step\":\"one\"}");

        // The version that was rolled away is still in the history
        const [History] = await GetSaveHistory([CharacterId]);
        assert.deepEqual(History.CharacterVersions.map((Version) => [Version.Version, Version.Reason]), [[3, "rollback:1"], [2, "save"], [1, "save"], [0, "baseline"]]);
    });

    it("can undo a rollback", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        UpdateCharacterForUid(CharacterId, UserId, "{\"step\":\"one\"}", 1);
        UpdateCharacterForUid(CharacterId, UserId, "{\"step\":\"two\"}", 2);
        RollbackCharacter(CharacterId, 1, "UID-admin");

        assert.ok(RollbackCharacter(CharacterId, 2, "UID-admin").success);
        assert.equal(ReadCharacter(CharacterId).data, "{\"step\":\"two\"}");
        assert.equal(ReadCharacter(CharacterId).updateVersion, 4);
    });

    it("answers version_not_found and not_found without changing anything", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        UpdateCharacterForUid(CharacterId, UserId, "{}", 1);

        assert.deepEqual(RollbackCharacter(CharacterId, 42, "UID-admin"), {success: false, error: "version_not_found"});
        assert.deepEqual(RollbackCharacter("no-such-character", 0, "UID-admin"), {success: false, error: "not_found"});
        assert.equal(ReadCharacter(CharacterId).updateVersion, 1);
    });

    it("records the version found in the table before the first tracked save", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        UpdateCharacterForUid(CharacterId, UserId, "{\"x\":\"1\"}", 7);

        assert.ok(RollbackCharacter(CharacterId, 0, "UID-admin").success);
        assert.equal(ReadCharacter(CharacterId).data, "{}");
        assert.equal(ReadCharacter(CharacterId).updateVersion, 8);
    });
});
