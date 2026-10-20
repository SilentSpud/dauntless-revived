import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { ConfirmRank, GetObjectiveRecord, GetObjectiveRecords, GetSelectedHuntPass, GetTrackRecord, GetTrackRecords, GrantProgression, GrantProgressionInTrack, ResetTrack, SeedProgression, SetSelectedHuntPass, TrackRecord } from "../src/controllers/realprogression";
import { GetDb as Db } from "../src/db";
import { RevokeEntitlementInTx } from "../src/controllers/entitlements";
import { Count, MakePlayer } from "./helpers";

after(() => RemoveTestDb(() => GetDb().$client.close()));

const Grant = (UserId: string, Tracks: any[], Objectives: any[] = []) => GrantProgression(UserId, {progress_tracks: Tracks, objectives: Objectives}, "gameserver");
const Track = (UserId: string, Id: string) => GetTrackRecord(UserId, Id)!;
const Payload = (Reply: any) => Reply.Body.payload;

describe("GetTrackRecords (GET /progression/:uid)", () => {
    it("lists all 10 configured tracks at 0 for a new account, keyed by the URL account", async () => {
        const {UserId} = await MakePlayer();
        const Records = GetTrackRecords(UserId);

        assert.equal(Records.length, 10);
        for(const Record of Records){
            assert.deepEqual(Object.keys(Record), ["phx_account_id", "progression_id", "progress", "confirmed_fremium_rank", "confirmed_premium_rank", "confirmed_date"]);
            assert.equal(Record.phx_account_id, UserId);
            assert.equal(Record.progress, 0);
            assert.equal(Record.confirmed_fremium_rank, 0);
            assert.equal(Record.confirmed_premium_rank, 0);
            assert.equal(Record.confirmed_date, "1970-01-01T00:00:00.000Z");
        }
        assert.equal(Records[0].progression_id, "season09b");
        assert.equal(GetTrackRecord(UserId, "NotATrack"), undefined);
    });
});

describe("GrantProgression (POST /progression/:uid)", () => {
    it("adds increments, answers the new totals for only the tracks it touched, stored ranks unchanged", async () => {
        const {UserId} = await MakePlayer();

        const First = Grant(UserId, [{progression_id: "MasteryTrack_Weapon_Sword", progress: 4}]);
        assert.equal(First.Status, 200);
        assert.equal((First.Body as any).code, "OK");
        assert.deepEqual(Payload(First).progress_tracks.map((T: TrackRecord) => [T.phx_account_id, T.progression_id, T.progress, T.confirmed_fremium_rank]), [[UserId, "MasteryTrack_Weapon_Sword", 4, 0]]);
        assert.deepEqual(Payload(First).objectives, []);

        const Second = Grant(UserId, [{progression_id: "MasteryTrack_Weapon_Sword", progress: 3}, {progression_id: "MasteryTrack_Weapon_Sword", progress: 2}, {progression_id: "season09b", progress: 150}]);
        assert.deepEqual(Payload(Second).progress_tracks.map((T: TrackRecord) => [T.progression_id, T.progress]), [["MasteryTrack_Weapon_Sword", 9], ["season09b", 150]]);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Sword").progress, 9);
        assert.equal(Track(UserId, "MasteryTrack_PlayerLevel").progress, 0);
    });

    it("stores objectives exactly as sent (absolute, last one wins) and echoes them", async () => {
        const {UserId} = await MakePlayer();

        const Reply = Grant(UserId, [], [{objective_id: "OBJ_A", value: 3, completed_count: 0}, {objective_id: "OBJ_B", value: 1, completed_count: 2}, {objective_id: "OBJ_A", value: 5, completed_count: 1}]);
        const Echo = Payload(Reply).objectives;

        assert.deepEqual(Echo.map((O: any) => [O.phx_account_id, O.objective_id, O.progress, O.completed_count]), [[UserId, "OBJ_A", 5, 1], [UserId, "OBJ_B", 1, 2]]);
        assert.deepEqual(Object.keys(Echo[0]), ["phx_account_id", "objective_id", "progress", "completed_count", "created_date", "last_modified_date"]);

        Grant(UserId, [], [{objective_id: "OBJ_A", value: 2, completed_count: 1}]);
        assert.deepEqual(GetObjectiveRecords(UserId).map((O) => [O.objective_id, O.progress, O.completed_count]), [["OBJ_A", 2, 1], ["OBJ_B", 1, 2]]);
        assert.equal(GetObjectiveRecord(UserId, "OBJ_B").progress, 1);
        assert.equal(GetObjectiveRecord(UserId, "OBJ_A").created_date, Echo[0].created_date);

        const Missing = GetObjectiveRecord(UserId, "OBJ_NEVER");
        assert.deepEqual([Missing.phx_account_id, Missing.objective_id, Missing.progress, Missing.completed_count], [UserId, "OBJ_NEVER", 0, 0]);
    });

    it("clamps negative amounts to 0, caps big ones, reads numeric strings, stores unknown tracks", async () => {
        const {UserId} = await MakePlayer();

        const Reply = Grant(UserId, [
            {progression_id: "MasteryTrack_Behemoth", progress: -5},
            {progression_id: "MasteryTrack_PlayerLevel", progress: 999999},
            {progression_id: "MasteryTrack_Weapon_Axe", progress: "7"},
            {progression_id: "SomeFutureTrack", progress: 1}
        ]);

        assert.deepEqual(Payload(Reply).progress_tracks.map((T: TrackRecord) => [T.progression_id, T.progress]), [["MasteryTrack_Behemoth", 0], ["MasteryTrack_PlayerLevel", 5000], ["MasteryTrack_Weapon_Axe", 7], ["SomeFutureTrack", 1]]);
        assert.equal(GetTrackRecords(UserId).length, 11);
        assert.equal(Track(UserId, "SomeFutureTrack").progress, 1);
    });

    it("caps each track's total in a request, so repeating a track does not add the cap twice", async () => {
        const {UserId} = await MakePlayer();

        const Reply = Grant(UserId, [
            {progression_id: "MasteryTrack_PlayerLevel", progress: 5000},
            {progression_id: "MasteryTrack_PlayerLevel", progress: 5000},
            {progression_id: "MasteryTrack_PlayerLevel", progress: 5000},
            {progression_id: "MasteryTrack_Behemoth", progress: 3000},
            {progression_id: "MasteryTrack_Behemoth", progress: 1000}
        ]);

        assert.deepEqual(Payload(Reply).progress_tracks.map((T: TrackRecord) => [T.progression_id, T.progress]), [["MasteryTrack_PlayerLevel", 5000], ["MasteryTrack_Behemoth", 4000]]);
        assert.equal(Track(UserId, "MasteryTrack_PlayerLevel").progress, 5000);

        const Event = GetDb().$client.prepare("select note from progression_events where accountId = ? order by id desc").get(UserId) as any;
        assert.match(Event.note, /MasteryTrack_PlayerLevel total 15000 is over the cap of 5000, cut to 5000/);
    });

    it("refuses a body that is not the grant shape, and an unknown account, and audits both", async () => {
        const {UserId} = await MakePlayer();

        assert.equal(GrantProgression(UserId, "nonsense", "gameserver").Status, 400);
        assert.equal(GrantProgression(UserId, {progress_tracks: {}, objectives: []}, "gameserver").Status, 400);
        assert.equal(GrantProgression("UID-nobody", {progress_tracks: [], objectives: []}, "gameserver").Status, 404);
        assert.equal(Count("progression_events", "accountId = ? and status = 400", UserId), 2);

        // Missing arrays are taken as empty (noted), not refused
        assert.equal(GrantProgression(UserId, {progress_tracks: [{progression_id: "season09b", progress: 1}]}, "gameserver").Status, 200);
        const Event = GetDb().$client.prepare("select note, body, reply from progression_events where accountId = ? and status = 200 order by id desc").get(UserId) as any;
        assert.match(Event.note, /no objectives key/);
        assert.equal(JSON.parse(Event.body).progress_tracks[0].progress, 1);
        assert.equal(JSON.parse(Event.reply).payload.progress_tracks[0].progress, 1);
    });

    it("keeps the audit log append-only", async () => {
        const {UserId} = await MakePlayer();
        Grant(UserId, [{progression_id: "season09b", progress: 1}]);

        assert.throws(() => GetDb().$client.prepare("update progression_events set status = 0").run(), /append-only/);
        assert.throws(() => GetDb().$client.prepare("delete from progression_events").run(), /append-only/);
    });
});

describe("ConfirmRank (POST .../:rank/confirm/:kind)", () => {
    it("raises the confirmed rank to the earned rank, never above, never down, idempotent", async () => {
        const {UserId} = await MakePlayer();
        Grant(UserId, [{progression_id: "MasteryTrack_Weapon_Sword", progress: 18}]);

        const Earned = 7; // 18 XP on a weapon track
        const Reply = ConfirmRank(UserId, "MasteryTrack_Weapon_Sword", String(Earned), "public", "gameserver");
        assert.equal(Reply.Status, 200);
        assert.deepEqual([Payload(Reply).progress, Payload(Reply).confirmed_fremium_rank, Payload(Reply).confirmed_premium_rank], [18, Earned, 0]);
        assert.notEqual(Payload(Reply).confirmed_date, "1970-01-01T00:00:00.000Z");

        assert.equal(Payload(ConfirmRank(UserId, "MasteryTrack_Weapon_Sword", "20", "public", "gameserver")).confirmed_fremium_rank, Earned);
        assert.equal(Payload(ConfirmRank(UserId, "MasteryTrack_Weapon_Sword", "2", "public", "gameserver")).confirmed_fremium_rank, Earned);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Sword").confirmed_fremium_rank, Earned);
    });

    it("confirms PlayerLevel rank 1 at progress 0 (rank 1 costs nothing)", async () => {
        const {UserId} = await MakePlayer();

        assert.equal(Payload(ConfirmRank(UserId, "MasteryTrack_PlayerLevel", "1", "public", "gameserver")).confirmed_fremium_rank, 1);
    });

    it("confirms premium on season09b only while the account owns season09b_premium", async () => {
        const {UserId} = await MakePlayer();
        GrantProgressionInTrack(UserId, "season09b", "250", "gameserver");

        assert.equal(Payload(ConfirmRank(UserId, "season09b", "2", "premium", "gameserver")).confirmed_premium_rank, 2);

        Db().transaction((tx) => RevokeEntitlementInTx(tx, UserId, "season09b_premium"));
        assert.equal(ConfirmRank(UserId, "season09b", "2", "premium", "gameserver").Status, 403);
        assert.equal(Track(UserId, "season09b").confirmed_premium_rank, 2);
    });

    it("prestige ranks above 50 can be confirmed on season09b", async () => {
        const {UserId} = await MakePlayer();
        GrantProgressionInTrack(UserId, "season09b", "5000", "gameserver");
        GrantProgressionInTrack(UserId, "season09b", "200", "gameserver");

        assert.equal(Payload(ConfirmRank(UserId, "season09b", "52", "public", "gameserver")).confirmed_fremium_rank, 52);
    });

    it("leaves premium on a mastery track alone, and refuses bad input", async () => {
        const {UserId} = await MakePlayer();
        Grant(UserId, [{progression_id: "MasteryTrack_Behemoth", progress: 411}]);

        const Mastery = ConfirmRank(UserId, "MasteryTrack_Behemoth", "50", "premium", "gameserver");
        assert.equal(Mastery.Status, 200);
        assert.equal(Payload(Mastery).confirmed_premium_rank, 0);

        assert.equal(ConfirmRank(UserId, "MasteryTrack_Behemoth", "5", "both", "gameserver").Status, 400);
        assert.equal(ConfirmRank(UserId, "MasteryTrack_Behemoth", "-1", "public", "gameserver").Status, 400);
        assert.equal(ConfirmRank(UserId, "MasteryTrack_Behemoth", "x", "public", "gameserver").Status, 400);
        assert.equal(ConfirmRank(UserId, "NotATrack", "1", "public", "gameserver").Status, 404);
        assert.equal(ConfirmRank("UID-nobody", "MasteryTrack_Behemoth", "1", "public", "gameserver").Status, 404);
    });
});

describe("Hunt Pass tutorial sequence", () => {
    it("season09b/100 leaves rank 1 pending; the claim's confirm records it once", async () => {
        const {UserId} = await MakePlayer();

        const Granted = GrantProgressionInTrack(UserId, "season09b", "100", "gameserver");
        assert.equal(Granted.Status, 200);
        assert.deepEqual([Payload(Granted).progress, Payload(Granted).confirmed_fremium_rank, Payload(Granted).confirmed_premium_rank], [100, 0, 0]);

        assert.equal(Payload(ConfirmRank(UserId, "season09b", "1", "public", "gameserver")).confirmed_fremium_rank, 1);
        assert.equal(Payload(ConfirmRank(UserId, "season09b", "1", "premium", "gameserver")).confirmed_premium_rank, 1);
        assert.deepEqual([Track(UserId, "season09b").progress, Track(UserId, "season09b").confirmed_fremium_rank, Track(UserId, "season09b").confirmed_premium_rank], [100, 1, 1]);
    });

    it("refuses a non-integer amount and caps a large one", async () => {
        const {UserId} = await MakePlayer();

        assert.equal(GrantProgressionInTrack(UserId, "season09b", "1.5", "gameserver").Status, 400);
        assert.equal(Payload(GrantProgressionInTrack(UserId, "season09b", "-10", "gameserver")).progress, 0);
        assert.equal(Payload(GrantProgressionInTrack(UserId, "season09b", "100000", "gameserver")).progress, 5000);
    });

    it("keeps the selected hunt pass consistent with the config", async () => {
        const {UserId} = await MakePlayer();

        assert.equal(GetSelectedHuntPass(UserId), "season09b");
        assert.equal(SetSelectedHuntPass(UserId, {progression_id: "NotATrack"}, "gameserver").Status, 400);
        assert.equal(SetSelectedHuntPass(UserId, {progression_id: "season09b"}, "gameserver").Status, 200);
        assert.equal(GetSelectedHuntPass(UserId), "season09b");
    });
});

describe("ResetTrack and SeedProgression", () => {
    it("resets one track to 0", async () => {
        const {UserId} = await MakePlayer();
        Grant(UserId, [{progression_id: "MasteryTrack_Weapon_Spear", progress: 50}]);

        const Reply = ResetTrack(UserId, "MasteryTrack_Weapon_Spear", "admin");
        assert.deepEqual([Payload(Reply).progress, Payload(Reply).confirmed_fremium_rank], [0, 0]);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Spear").progress, 0);
        assert.equal(ResetTrack(UserId, "NotATrack", "admin").Status, 404);
    });

    it("grandfather: every track at its max, fully confirmed; never lowers stored values", async () => {
        const {UserId} = await MakePlayer();
        GrantProgressionInTrack(UserId, "season09b", "5000", "gameserver");
        GrantProgressionInTrack(UserId, "season09b", "300", "gameserver");

        assert.equal(SeedProgression(UserId, "grandfather", "UID-admin").Status, 200);

        const ByTrack = new Map(GetTrackRecords(UserId).map((Record) => [Record.progression_id, Record]));
        assert.deepEqual([ByTrack.get("MasteryTrack_PlayerLevel")!.progress, ByTrack.get("MasteryTrack_PlayerLevel")!.confirmed_fremium_rank, ByTrack.get("MasteryTrack_PlayerLevel")!.confirmed_premium_rank], [1111, 50, 50]);
        assert.deepEqual([ByTrack.get("MasteryTrack_Behemoth")!.progress, ByTrack.get("MasteryTrack_Behemoth")!.confirmed_fremium_rank], [411, 50]);
        assert.deepEqual([ByTrack.get("MasteryTrack_Weapon_Hammer")!.progress, ByTrack.get("MasteryTrack_Weapon_Hammer")!.confirmed_fremium_rank], [115, 20]);
        assert.deepEqual([ByTrack.get("season09b")!.progress, ByTrack.get("season09b")!.confirmed_fremium_rank, ByTrack.get("season09b")!.confirmed_premium_rank], [5300, 53, 53]);

        // A confirm after grandfathering changes nothing: nothing is pending
        assert.equal(Payload(ConfirmRank(UserId, "MasteryTrack_PlayerLevel", "50", "public", "gameserver")).confirmed_fremium_rank, 50);
    });

    it("fresh: every track at 0 and the objectives cleared; unknown accounts refused", async () => {
        const {UserId} = await MakePlayer();
        Grant(UserId, [{progression_id: "MasteryTrack_Weapon_Sword", progress: 60}], [{objective_id: "OBJ", value: 1, completed_count: 0}]);

        assert.equal(SeedProgression(UserId, "fresh", "UID-admin").Status, 200);
        assert.ok(GetTrackRecords(UserId).every((Record) => Record.progress === 0 && Record.confirmed_fremium_rank === 0));
        assert.equal(Count("progress_tracks", "accountId = ?", UserId), 10);
        assert.deepEqual(GetObjectiveRecords(UserId), []);
        assert.equal(SeedProgression("UID-nobody", "fresh", "UID-admin").Status, 404);
    });
});
