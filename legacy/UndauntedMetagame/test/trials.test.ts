import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { eq } from "drizzle-orm";
import { Call, StartApp, StopApp } from "./appclient";
import { GetDb } from "../src/db";
import { inventory, cooldowns, entitlements, leaderboardprofiles, trialruns, trialweeks } from "../src/db/schema";
import { MakePlayer, StackQuantity } from "./helpers";
import { FinalizeCompletedTrialWeeks, TrialIdForWeek, TrialRewardRank, TrialWeekAt, TrialsWindowForWeek, TRIAL_POOL, TRIAL_ROTATION_START, TrialBehemothForWeek, TRIALS_CHAMPION_ENTITLEMENT, TRIALS_CHAMPION_TITLE, TRIALS_DAUNTLESS_TITLE } from "../src/controllers/trials";
import { GetSeasonalEventSchedule } from "../src/routes/tuning";

const Trial = (Difficulty = 1) => TrialIdForWeek(Difficulty, TrialWeekAt());

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
});

beforeEach(() => {
    process.env.TRIALS_LEADERBOARDS = "1";
    process.env.TRIALS_SCHEDULE = "0";
    process.env.STORE = "off";
    process.env.TRIALS_STORE = "1";
    process.env.MIDDLEMAN_STORE = "0";
    GetDb().delete(trialruns).run();
    GetDb().delete(trialweeks).run();
    GetDb().delete(leaderboardprofiles).run();
    GetDb().delete(cooldowns).run();
});

function Solo(UserId: string, Time: number, Session: string, Difficulty = 1){
    return {
        difficulty: Difficulty,
        trial_id: Trial(Difficulty),
        completion_time: Time,
        objectives_completed: 3,
        phx_account_id: UserId,
        platform: "WIN",
        platform_name: UserId,
        player_role_id: "PR_FRANK",
        weapon: 2,
        secondary_weapon: 1,
        session_id: Session
    };
}

function Query(Extra = {}){
    return {difficulty: 1, page: 0, page_size: 100, trial_id: Trial(), target_platforms: [], ...Extra};
}

describe("Trials leaderboards", () => {
    it("serves an opt-in current Trials schedule without changing the default stub", () => {
        const At = new Date("2026-10-06T12:00:00.000Z");

        const Stub = GetSeasonalEventSchedule(At);
        assert.deepEqual(Stub.payload.ScheduledItems, []);

        process.env.TRIALS_SCHEDULE = "1";
        const Active = GetSeasonalEventSchedule(At);
        const Rows = Active.payload.ScheduledItems;

        assert.equal(Rows.length, 5);
        assert.deepEqual(Rows.map((Row: any) => Row.ScheduledItems[0].ID), [
            "event_ladyluck_repeatable",
            "CR19_PlayerHunt_Arena_Hard",
            "CR19_PlayerHunt_Arena_Elite",
            TrialIdForWeek(0, TrialWeekAt(At)),
            TrialIdForWeek(1, TrialWeekAt(At))
        ]);
        assert.ok(Rows.every((Row: any) => Row.StartTime === "2026.10.01-18.00.00"));
        assert.ok(Rows.every((Row: any) => Row.EndTime === "2026.10.08-18.00.00"));
        assert.ok(Rows.every((Row: any) => Row.ScheduledItems[0].MaxCompletionPerInterval === -1));
    });

    it("picks the same weekly Trial row as the deploy server", () => {
        // Pinned from UndauntedDeployServer's GetTrialsData; both servers must pick the same row.
        const Expected: [number, string][] = [[0, "045"], [1, "075"], [2, "029"], [68, "064"], [377, "039"], [378, "069"], [379, "087"]];

        for(const [Week, Suffix] of Expected){
            assert.equal(TrialIdForWeek(0, Week), `Arena_MatchmakerHunt_Hard_${Suffix}`);
            assert.equal(TrialIdForWeek(1, Week), `Arena_MatchmakerHunt_Elite_${Suffix}`);
        }

        const DeployPool = JSON.parse(readFileSync(path.join(__dirname, "../../../UndauntedDeployServer/src/vendor/trials_pool.json"), "utf8"));
        assert.deepEqual(DeployPool.behemoths, TRIAL_POOL);
    });

    it("rotates a variant-only pool weekly without repeating a behemoth back to back", () => {
        assert.equal(new Date(TRIAL_ROTATION_START).toISOString(), "2019-07-18T18:00:00.000Z");
        assert.equal(TrialWeekAt(new Date("2019-07-18T17:59:59.999Z")), -1);
        assert.equal(TrialWeekAt(new Date("2019-07-18T18:00:00.000Z")), 0);

        let Previous = "";
        for(let Week = -30; Week < 400; Week++){
            const Behemoth = TrialBehemothForWeek(Week);

            assert.ok(Behemoth.endsWith("_alpha"));
            assert.ok(TRIAL_POOL[Behemoth].includes(TrialIdForWeek(1, Week).slice(-3)));
            assert.notEqual(Behemoth, Previous);
            Previous = Behemoth;
        }
    });

    it("uses the retail Bronze/Silver/Gold time boundaries for reward rank", () => {
        assert.equal(TrialRewardRank(30 * 60 * 1000), -1);
        assert.equal(TrialRewardRank(30 * 60 * 1000 - 1), 0);
        assert.equal(TrialRewardRank(5 * 60 * 1000), 0);
        assert.equal(TrialRewardRank(5 * 60 * 1000 - 1), 1);
        assert.equal(TrialRewardRank(3 * 60 * 1000), 1);
        assert.equal(TrialRewardRank(3 * 60 * 1000 - 1), 2);
    });

    it("awards weekly Steel/Gilded tiers once and exposes them through balance", async () => {
        const A = await MakePlayer();

        // Normal sub-5: Bronze + Silver = 200 Steel.
        const Normal = Solo(A.UserId, 5 * 60 * 1000 - 1, "normal-silver", 0);
        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Normal})).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 200);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_GILDED"), 0);

        // Exact retry pays nothing, then a faster Normal run pays only the newly reached Gold tier.
        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Normal})).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 200);
        assert.equal((await Call("POST", "/trials/leaderboards", {
            gs: true,
            body: Solo(A.UserId, 3 * 60 * 1000 - 1, "normal-gold", 0)
        })).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 300);

        // Dauntless sub-5 pays two Gilded tiers; Steel is already claimed and is not duplicated.
        const Dauntless = Solo(A.UserId, 5 * 60 * 1000 - 1, "dauntless-silver", 1);
        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Dauntless})).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 300);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_GILDED"), 200);

        // A later sub-3 run pays only the final Gilded tier.
        assert.equal((await Call("POST", "/trials/leaderboards", {
            gs: true,
            body: Solo(A.UserId, 3 * 60 * 1000 - 1, "dauntless-gold", 1)
        })).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_GILDED"), 300);

        const Balance = await Call("GET", "/balance", {as: A.UserId});
        assert.equal(Balance.status, 200);
        assert.equal(Balance.json.CURRENCY_MARKS_STEEL, 300);
        assert.equal(Balance.json.id_currency_marks_steel, 300);
        assert.equal(Balance.json.CURRENCY_MARKS_GILDED, 300);
        assert.equal(Balance.json.id_currency_marks_gilded, 300);

        assert.equal(GetDb().select().from(cooldowns).where(eq(cooldowns.accountId, A.UserId)).all()
            .filter((Row) => Row.cooldownId.startsWith("revived_trials_marks:")).length, 6);
    });

    it("rejects a reused result session with changed timing instead of escalating its reward", async () => {
        const A = await MakePlayer();
        const Session = "immutable-result-session";

        assert.equal((await Call("POST", "/trials/leaderboards", {
            gs: true,
            body: Solo(A.UserId, 5 * 60 * 1000 - 1, Session, 0)
        })).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 200);

        const Changed = await Call("POST", "/trials/leaderboards", {
            gs: true,
            body: Solo(A.UserId, 3 * 60 * 1000 - 1, Session, 0)
        });
        assert.equal(Changed.status, 409);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 200);
        assert.equal(GetDb().select().from(trialruns).all().length, 1);
    });

    it("completes the Trial -> Marks -> Lady Luck purchase loop without refilling on retry", async () => {
        const A = await MakePlayer();
        const Run = Solo(A.UserId, 3 * 60 * 1000 - 1, "purchase-loop", 0);

        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Run})).status, 204);
        assert.equal((await Call("GET", "/balance", {as: A.UserId})).json.CURRENCY_MARKS_STEEL, 300);

        const Token = await Call("GET", "/token/CURRENCY_MARKS_STEEL/ladyluck_cb_passive_trials_02", {as: A.UserId});
        assert.equal(Token.status, 200);
        const Bought = await Call("POST", `/notification/CURRENCY_MARKS_STEEL?token=${Token.json.purchaseToken}`, {as: A.UserId});
        assert.equal(Bought.status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 50);
        assert.equal(JSON.parse(GetDb().select().from(inventory).where(eq(inventory.characterId,A.CharacterId)).get()!.instancedItems).filter((item:any)=>item.catalogId==="PART_CB_PASSIVE_TRIALS_02").length, 1);

        // A retried result is still idempotent after the player has spent the Marks.
        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Run})).status, 204);
        assert.equal(StackQuantity(A.CharacterId, "CURRENCY_MARKS_STEEL"), 50);
        assert.equal(JSON.parse(GetDb().select().from(inventory).where(eq(inventory.characterId,A.CharacterId)).get()!.instancedItems).filter((item:any)=>item.catalogId==="PART_CB_PASSIVE_TRIALS_02").length, 1);
    });

    it("reports timing for the Trial week being queried, not always the current week", () => {
        const At = new Date(Date.parse(TRIAL_ROTATION_START) + 10 * 7 * 24 * 60 * 60 * 1000 + 2 * 24 * 60 * 60 * 1000);
        const CurrentWeek = TrialWeekAt(At);
        const Current = TrialsWindowForWeek(CurrentWeek, At);
        const Previous = TrialsWindowForWeek(CurrentWeek - 1, At);

        assert.equal(Current.trial_end - Current.trial_start, 7 * 24 * 60 * 60);
        assert.equal(Current.time_to_refresh, 5 * 24 * 60 * 60);
        assert.equal(Previous.trial_end, Current.trial_start);
        assert.equal(Previous.time_to_refresh, 0);
    });

    it("stores game-server solo results, keeps each player's best time and serves aggregate and individual shapes", async () => {
        const A = await MakePlayer(), B = await MakePlayer();

        for(const Body of [Solo(A.UserId, 42000, "a-slow"), Solo(B.UserId, 39000, "b"), Solo(A.UserId, 37000, "a-fast")]){
            assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Body})).status, 204);
        }

        const SoloBoard = await Call("POST", "/trials/leaderboards/solo", {as: A.UserId, body: Query()});
        assert.equal(SoloBoard.status, 200);
        assert.ok(Number.isInteger(SoloBoard.json.payload.trial_start));
        assert.equal(SoloBoard.json.payload.trial_end - SoloBoard.json.payload.trial_start, 7 * 24 * 60 * 60);
        assert.ok(SoloBoard.json.payload.time_to_refresh > 0 && SoloBoard.json.payload.time_to_refresh <= 7 * 24 * 60 * 60);
        assert.deepEqual(SoloBoard.json.payload.entries.map((Entry: any) => [Entry.phx_account_id, Entry.completion_time, Entry.rank]), [
            [A.UserId, 37000, 1],
            [B.UserId, 39000, 2]
        ]);

        const Individual = await Call("POST", "/trials/leaderboards/solo/individual", {as: A.UserId, body: Query({phx_account_id: B.UserId})});
        assert.equal(Individual.status, 200);
        assert.equal(Individual.json.payload.phx_account_id, B.UserId);
        assert.equal(Individual.json.payload.rank, 2);
        assert.equal(Individual.json.payload.trial_id, Trial());
        assert.equal(Individual.json.payload.difficulty, "1");

        const All = await Call("POST", "/trials/leaderboards", {as: A.UserId, body: Query()});
        assert.equal(All.status, 200);
        assert.deepEqual(Object.keys(All.json.payload).sort(), ["difficulty", "guild", "page", "page_size", "trial_id", "world"]);
        assert.deepEqual(All.json.payload.world.solo.all.entries.map((Entry: any) => Entry.phx_account_id), [A.UserId, B.UserId]);
        assert.deepEqual(All.json.payload.world.group, {
            difficulty: 1,
            entries: [],
            page: 0,
            page_size: 100,
            trial_id: Trial()
        });

        const CapturedAll = await Call("POST", "/trials/leaderboards/all", {as: A.UserId, body: Query()});
        assert.equal(CapturedAll.status, 200);
        assert.deepEqual(CapturedAll.json, All.json);
    });

    it("stores one group run once on retry and serves group and member lookup", async () => {
        const A = await MakePlayer(), B = await MakePlayer();
        const Body = {
            difficulty: 1,
            trial_id: Trial(),
            completion_time: 51000,
            objectives_completed: 3,
            session_id: "group-session",
            entries: [
                {phx_account_id: A.UserId, platform: "WIN", platform_name: "A", player_role_id: "PR_FRANK", weapon: 2},
                {phx_account_id: B.UserId, platform: "WIN", platform_name: "B", player_role_id: "PR_BASTION", weapon: 5}
            ]
        };

        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Body})).status, 204);
        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Body})).status, 204);
        assert.equal(GetDb().select().from(trialruns).all().length, 1);

        const Group = await Call("POST", "/trials/leaderboards/group", {as: A.UserId, body: Query()});
        assert.equal(Group.status, 200);
        assert.equal(Group.json.payload.entries.length, 1);
        assert.deepEqual(Group.json.payload.entries[0].entries.map((Entry: any) => Entry.phx_account_id), [A.UserId, B.UserId]);

        const Individual = await Call("POST", "/trials/leaderboards/group/individual", {as: B.UserId, body: Query({phx_account_id: B.UserId})});
        assert.equal(Individual.status, 200);
        assert.equal(Individual.json.payload.rank, 1);
        assert.equal(Individual.json.payload.session_id, "group-session");
    });

    it("persists the 1.4.4 profile update and uses its latest public identity on leaderboard reads", async () => {
        const A = await MakePlayer();

        const Updated = await Call("POST", "/profile/update", {as: A.UserId, body: {
            dauntlessid: A.UserId,
            epicid: "epic-a",
            platformid: "platform-a",
            currentplatform: "PSN",
            currentdisplayname: "Current Slayer"
        }});
        assert.equal(Updated.status, 200);
        assert.equal(GetDb().select().from(leaderboardprofiles).all().length, 1);

        await Call("POST", "/trials/leaderboards", {gs: true, body: {...Solo(A.UserId, 32000, "profile-run"), platform: "WIN", platform_name: "Old Name"}});
        const Board = await Call("POST", "/trials/leaderboards/solo", {as: A.UserId, body: Query()});

        assert.equal(Board.json.payload.entries[0].platform, "PSN");
        assert.equal(Board.json.payload.entries[0].platform_name, "Current Slayer");

        const Spoof = await Call("POST", "/profile/update", {as: A.UserId, body: {
            dauntlessid: "UID-someone-else",
            epicid: "",
            platformid: "",
            currentplatform: "WIN",
            currentdisplayname: "Nope"
        }});
        assert.equal(Spoof.status, 403);
    });

    it("refuses player-submitted scores and malformed results", async () => {
        const A = await MakePlayer();

        assert.equal((await Call("POST", "/trials/leaderboards", {as: A.UserId, body: Solo(A.UserId, 1, "cheat")})).status, 403);
        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: {...Solo(A.UserId, 1, "bad"), trial_id: "not-a-trial"}})).status, 400);
        assert.equal(GetDb().select().from(trialruns).all().length, 0);
    });

    it("filters by target platform and paginates after ranking", async () => {
        const A = await MakePlayer(), B = await MakePlayer();
        await Call("POST", "/trials/leaderboards", {gs: true, body: Solo(A.UserId, 30000, "a")});
        await Call("POST", "/trials/leaderboards", {gs: true, body: {...Solo(B.UserId, 31000, "b"), platform: "PSN"}});

        const Win = await Call("POST", "/trials/leaderboards/solo", {as: A.UserId, body: Query({target_platforms: ["WIN"]})});
        assert.deepEqual(Win.json.payload.entries.map((Entry: any) => Entry.phx_account_id), [A.UserId]);

        const PageTwo = await Call("POST", "/trials/leaderboards/solo", {as: A.UserId, body: Query({page: 1, page_size: 1})});
        assert.equal(PageTwo.json.payload.entries[0].phx_account_id, B.UserId);
        assert.equal(PageTwo.json.payload.entries[0].rank, 2);
    });

    it("keeps a repeated Trial row isolated by week and rejects stale game-server submissions", async () => {
        const A = await MakePlayer();
        const CurrentWeek = TrialWeekAt();
        const CurrentTrial = TrialIdForWeek(1, CurrentWeek);
        const OldWeek = CurrentWeek - 52;

        GetDb().insert(trialruns).values({
            trialId: CurrentTrial,
            trialWeek: OldWeek,
            difficulty: 1,
            mode: "solo",
            runKey: "old-cycle",
            groupKey: A.UserId,
            completionTime: 1000,
            objectivesCompleted: 3,
            sessionId: "old-cycle",
            entries: JSON.stringify([{phx_account_id: A.UserId, platform: "WIN", platform_name: "Old", player_role_id: "PR_FRANK", weapon: 2}]),
            submittedDate: "2025-01-01T00:00:00.000Z"
        }).run();

        assert.equal((await Call("POST", "/trials/leaderboards", {gs: true, body: Solo(A.UserId, 30000, "current-cycle")})).status, 204);
        const Current = await Call("POST", "/trials/leaderboards/solo", {as: A.UserId, body: Query()});
        assert.deepEqual(Current.json.payload.entries.map((Entry: any) => Entry.completion_time), [30000]);

        const PreviousTrial = TrialIdForWeek(1, CurrentWeek - 1);
        assert.equal((await Call("POST", "/trials/leaderboards", {
            gs: true,
            body: {...Solo(A.UserId, 25000, "stale"), trial_id: PreviousTrial}
        })).status, 409);
    });

    it("finalizes a completed Dauntless week once and permanently unlocks Champion access", () => {
        const CurrentWeek = TrialWeekAt();
        const PreviousWeek = CurrentWeek - 1;
        const TrialId = TrialIdForWeek(1, PreviousWeek);
        const SoloId = "UID-trials-champion-solo";
        const GroupIds = ["UID-trials-champion-g1", "UID-trials-champion-g2"];

        GetDb().insert(trialruns).values([
            {
                trialId: TrialId,
                trialWeek: PreviousWeek,
                difficulty: 1,
                mode: "solo",
                runKey: "champion-solo",
                groupKey: SoloId,
                completionTime: 30000,
                objectivesCompleted: 3,
                sessionId: "champion-solo",
                entries: JSON.stringify([{phx_account_id: SoloId, platform: "WIN", platform_name: "Solo", player_role_id: "PR_FRANK", weapon: 2}]),
                submittedDate: "2026-01-01T00:00:00.000Z"
            },
            {
                trialId: TrialId,
                trialWeek: PreviousWeek,
                difficulty: 1,
                mode: "group",
                runKey: "champion-group",
                groupKey: GroupIds.join(":"),
                completionTime: 31000,
                objectivesCompleted: 3,
                sessionId: "champion-group",
                entries: JSON.stringify(GroupIds.map((Id) => ({phx_account_id: Id, platform: "WIN", platform_name: Id, player_role_id: "PR_FRANK", weapon: 2}))),
                submittedDate: "2026-01-01T00:00:01.000Z"
            }
        ]).run();

        assert.deepEqual(FinalizeCompletedTrialWeeks(), {FinalizedWeeks: 1, AwardedAccounts: 3});
        assert.deepEqual(FinalizeCompletedTrialWeeks(), {FinalizedWeeks: 0, AwardedAccounts: 0});
        assert.equal(GetDb().select().from(trialweeks).all().length, 1);

        for(const AccountId of [SoloId, ...GroupIds]){
            assert.equal(GetDb().select().from(entitlements).where(eq(entitlements.accountId, AccountId)).all()
                .some((Row) => Row.name === TRIALS_CHAMPION_ENTITLEMENT && Row.duration === 0 && Row.revokedDate == null), true);
        }
    });

    it("awards Trials Champion to top 100 and The Dauntless only to top 5", async () => {
        const Players = await Promise.all(Array.from({length: 6}, () => MakePlayer()));
        const CurrentWeek = TrialWeekAt();
        const PreviousWeek = CurrentWeek - 1;
        const TrialId = TrialIdForWeek(1, PreviousWeek);

        GetDb().insert(trialruns).values(Players.map((Player, Index) => ({
            trialId: TrialId,
            trialWeek: PreviousWeek,
            difficulty: 1,
            mode: "solo",
            runKey: `title-rank-${Index + 1}`,
            groupKey: Player.UserId,
            completionTime: 30000 + Index,
            objectivesCompleted: 3,
            sessionId: `title-rank-${Index + 1}`,
            entries: JSON.stringify([{phx_account_id: Player.UserId, platform: "WIN", platform_name: Player.UserId, player_role_id: "PR_FRANK", weapon: 2}]),
            submittedDate: new Date(Date.UTC(2026, 0, 1, 0, 0, Index)).toISOString()
        }))).run();

        assert.deepEqual(FinalizeCompletedTrialWeeks(), {FinalizedWeeks: 1, AwardedAccounts: 6});

        for(const Player of Players){
            assert.equal(StackQuantity(Player.CharacterId, TRIALS_CHAMPION_TITLE), 1);
        }

        for(const Player of Players.slice(0, 5)){
            assert.equal(StackQuantity(Player.CharacterId, TRIALS_DAUNTLESS_TITLE), 1);
        }

        assert.equal(StackQuantity(Players[5].CharacterId, TRIALS_DAUNTLESS_TITLE), 0);

        // Finalization and the inventory transaction ledger make the permanent titles one-copy grants.
        assert.deepEqual(FinalizeCompletedTrialWeeks(), {FinalizedWeeks: 0, AwardedAccounts: 0});
        assert.equal(StackQuantity(Players[0].CharacterId, TRIALS_CHAMPION_TITLE), 1);
        assert.equal(StackQuantity(Players[0].CharacterId, TRIALS_DAUNTLESS_TITLE), 1);
    });

    it("returns 404 while the feature switch is off", async () => {
        const A = await MakePlayer();
        process.env.TRIALS_LEADERBOARDS = "0";

        assert.equal((await Call("POST", "/trials/leaderboards/solo", {as: A.UserId, body: Query()})).status, 404);
    });
});

