import { RemoveTestDb } from "./setup";
import "./authenv";
import { WithEnv } from "./appenv";
import { after, before, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { eq } from "drizzle-orm";
import { Call, StartApp, StopApp, Warnings } from "./appclient";
import { GetDb } from "../src/db";
import { inventory } from "../src/db/schema";
import { CheckProgressionConfig, GetActiveHuntPass, GetProgressionConfigPayload } from "../src/controllers/progressionconfig";
import { ComputeEarnedRanks, GetProgressionPath } from "../src/controllers/progressionrank";
import { GetSelectedHuntPass, GetTrackRecord } from "../src/controllers/realprogression";
import { ListEntitlements } from "../src/controllers/entitlements";
import { Count, MakePlayer } from "./helpers";
import BundledConfig from "../src/vendor/progression_config.json";

// The progression extras of the Harmonic port (github.com/Harmonicrain/Undaunted 895f7c7, ideas from its
// progressionWrites.ts, huntpass.ts, wallet.ts and routes/progression.ts), rebuilt on our real
// progression: the retry guard of the grant, the progression config folder and ACTIVE_HUNT_PASS, the
// currencies of /balance from the inventory, the optional entitlements on confirm, the log lines. And
// the guard that matters most: a confirm grants nothing (the game server pays rank rewards through
// /inventory; paying on confirm too would pay twice).

const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "dr-progression-extras-"));

before(async () => {
    await StartApp();
});

after(async () => {
    await StopApp();
    RemoveTestDb(() => GetDb().$client.close());
    fs.rmSync(Dir, { recursive: true, force: true });
});

beforeEach(() => {
    Warnings.length = 0;
});

const Grant = (UserId: string, Body: unknown) => Call("POST", `/progression/${UserId}`, { gs: true, as: UserId, body: Body });
const Confirm = (UserId: string, Track: string, Rank: number, Kind = "public") => Call("POST", `/progression/${UserId}/${Track}/${Rank}/confirm/${Kind}`, { gs: true, as: UserId });
const Track = (UserId: string, Id: string) => GetTrackRecord(UserId, Id)!;
const Events = (UserId: string, Route = "POST /progression/:uid") => GetDb().$client.prepare("select status, note, reply from progression_events where accountId = ? and route = ? order by id").all(UserId, Route) as any[];
const Sleep = (Ms: number) => new Promise((Resolve) => setTimeout(Resolve, Ms));

function Snapshot(UserId: string, CharacterId: string){
    return {
        inventory: GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get() ?? null,
        inventorylog: Count("inventorylog", "characterId = ?", CharacterId),
        inventorytransactions: Count("inventorytransactions", "characterId = ?", CharacterId),
        entitlements: GetDb().$client.prepare("select name, duration, source, revokedDate from entitlements where accountId = ? order by name").all(UserId)
    };
}

describe("a confirm grants nothing (PROGRESSION_CONFIRM_ENTITLEMENTS=0, the default)", () => {
    it("leaves the inventory, the item log, the ledger and the entitlements as they were, for every track and kind", async () => {
        const { UserId, CharacterId } = await MakePlayer();

        // Every track far enough for several ranks with items, currencies and entitlements in the config
        assert.equal((await Grant(UserId, { progress_tracks: [
            { progression_id: "season09b", progress: 5000 }, { progression_id: "MasteryTrack_PlayerLevel", progress: 1111 },
            { progression_id: "MasteryTrack_Weapon_Sword", progress: 115 }, { progression_id: "MasteryTrack_Behemoth", progress: 411 }
        ], objectives: [] })).status, 200);
        ListEntitlements(UserId); // the defaults exist before the snapshot
        const Before = Snapshot(UserId, CharacterId);

        for(const [Id, Rank] of [["season09b", 50], ["MasteryTrack_PlayerLevel", 50], ["MasteryTrack_Weapon_Sword", 20], ["MasteryTrack_Behemoth", 50]] as [string, number][]){
            for(const Kind of ["public", "premium"]){
                assert.equal((await Confirm(UserId, Id, Rank, Kind)).status, 200, `${Id} ${Kind}`);
            }
        }

        assert.deepEqual([Track(UserId, "season09b").confirmed_fremium_rank, Track(UserId, "season09b").confirmed_premium_rank], [50, 50]);
        assert.deepEqual(Snapshot(UserId, CharacterId), Before);
    });

    // Harmonic's mastery case 147 (test/mastery.test.js:147) inverted: the Rams of a mastery rank arrive
    // once, through the game server's /inventory, and the confirm after it adds none
    it("a play-through over HTTP: a grant crosses a rank, the game server pays through /inventory, the confirm pays nothing, nothing is left pending", async () => {
        const { UserId, CharacterId } = await MakePlayer();

        const Granted = await Grant(UserId, { progress_tracks: [{ progression_id: "MasteryTrack_Weapon_Sword", progress: 6 }], objectives: [] });
        assert.equal(Granted.status, 200);
        const Earned = ComputeEarnedRanks(GetProgressionPath("MasteryTrack_Weapon_Sword")!, Granted.json.payload.progress_tracks[0].progress, false).EarnedFreeRank;
        assert.equal(Earned, 3);

        // The game server grants the rank rewards itself, from the difference between its cache and the reply
        const Paid = await Call("POST", "/inventory", { gs: true, as: UserId, body: {
            accountId: UserId, characterId: CharacterId, source: "Progression", transactionId: "5E1A0D63-0000-4000-8000-000000000147",
            addStackedItems: [{ catalogId: "CURRENCY_NOTES", quantity: 500 }], addInstancedItems: [], removeStackedItems: [], removeInstancedItems: [], saveInstancedItems: []
        } });
        assert.equal(Paid.status, 200);

        for(let Attempt = 0; Attempt < 2; Attempt++){
            const Confirmed = await Confirm(UserId, "MasteryTrack_Weapon_Sword", 3);
            assert.equal(Confirmed.status, 200);
            assert.equal(Confirmed.json.payload.confirmed_fremium_rank, 3);
        }

        const Stacks = JSON.parse(GetDb().select().from(inventory).where(eq(inventory.characterId, CharacterId)).get()!.stackedItems);
        assert.deepEqual(Stacks, [{ catalogId: "CURRENCY_NOTES", quantity: 500 }], "the Rams arrived once");
        assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 1);

        const Read = await Call("GET", `/progression/${UserId}/MasteryTrack_Weapon_Sword`, { as: UserId });
        assert.deepEqual([Read.json.payload.progress, Read.json.payload.confirmed_fremium_rank], [6, Earned], "confirmed equals earned: nothing pending");
    });

    it("a season09b confirm at a prestige rank above 50 passes over HTTP", async () => {
        const { UserId } = await MakePlayer();

        await Grant(UserId, { progress_tracks: [{ progression_id: "season09b", progress: 5000 }], objectives: [] });
        await Grant(UserId, { progress_tracks: [{ progression_id: "season09b", progress: 250 }], objectives: [] });

        const Confirmed = await Confirm(UserId, "season09b", 52);
        assert.equal(Confirmed.status, 200);
        assert.equal(Confirmed.json.payload.confirmed_fremium_rank, 52);
    });
});

describe("PROGRESSION_CONFIRM_ENTITLEMENTS=1", () => {
    it("a confirm that raises a rank grants that rank's permanent config entitlements, never timed ones or items", async () => {
        await WithEnv({ PROGRESSION_CONFIRM_ENTITLEMENTS: "1" }, async () => {
            const { UserId, CharacterId } = await MakePlayer();
            await Grant(UserId, { progress_tracks: [{ progression_id: "season09b", progress: 1600 }], objectives: [] });

            // Elite ranks up to 16: rank 6 (ent_cchd_hp09b_commando_00) and 9 (ent_ccfp_hp09b_commando_00) are
            // permanent, rank 16's escalation_xp_boost_entitlement is 24 h and is not granted here
            assert.equal((await Confirm(UserId, "season09b", 16, "premium")).status, 200);

            const Names = ListEntitlements(UserId).map((Entitlement) => Entitlement.name);
            assert.ok(Names.includes("ent_cchd_hp09b_commando_00") && Names.includes("ent_ccfp_hp09b_commando_00"), Names.join(", "));
            assert.ok(!Names.includes("escalation_xp_boost_entitlement"));
            assert.equal((GetDb().$client.prepare("select source from entitlements where accountId = ? and name = 'ent_cchd_hp09b_commando_00'").get(UserId) as any).source, "confirm:season09b:6");
            assert.equal(Count("inventorylog", "characterId = ?", CharacterId), 0, "no items");
            assert.match(Events(UserId, "POST /progression/:uid/:track/:rank/confirm/:kind").at(-1).note, /escalation_xp_boost_entitlement \(24 h\) is timed, not granted on confirm/);

            // Nothing again for ranks already confirmed; the free list's rank 50 only when it is reached
            const Count1 = ListEntitlements(UserId).length;
            assert.equal((await Confirm(UserId, "season09b", 16, "premium")).status, 200);
            assert.equal((await Confirm(UserId, "season09b", 16, "public")).status, 200);
            assert.equal(ListEntitlements(UserId).length, Count1);
        });
    });
});

describe("the retry guard of POST /progression/:uid (PROGRESSION_REPLAY_WINDOW_S)", () => {
    const Body = { progress_tracks: [{ progression_id: "MasteryTrack_Weapon_Axe", progress: 2 }], objectives: [{ objective_id: "OBJ_RETRY", value: 1, completed_count: 1 }] };

    it("an identical grant within the window answers the stored reply and adds nothing", async () => {
        const { UserId } = await MakePlayer();

        const First = await Grant(UserId, Body);
        const Again = await Grant(UserId, Body);
        const Third = await Grant(UserId, Body);

        assert.deepEqual([First.status, Again.status, Third.status], [200, 200, 200]);
        assert.equal(Again.text, First.text, "the same bytes");
        assert.equal(Third.text, First.text);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 2);

        const Audit = Events(UserId);
        assert.equal(Audit.length, 3, "every request is audited");
        assert.match(Audit[1].note, /^retry of event \d+ within 5 s: its reply, nothing added/);
        assert.ok(Warnings.some((Line) => /repeats the grant of .* s ago: answered its stored reply, nothing added \(PROGRESSION_REPLAY_WINDOW_S=5\)/.test(Line)));

        // A different body is a new grant, and after it the first body counts again
        await Grant(UserId, { ...Body, objectives: [{ objective_id: "OBJ_RETRY", value: 2, completed_count: 2 }] });
        await Grant(UserId, Body);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 6);
    });

    it("an identical grant after the window adds its XP", async () => {
        await WithEnv({ PROGRESSION_REPLAY_WINDOW_S: "1" }, async () => {
            const { UserId } = await MakePlayer();

            await Grant(UserId, Body);
            await Sleep(1300);
            await Grant(UserId, Body);

            assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 4);
        });
    });

    it("the default window is shorter than the game server's 10 s grant flush: the same body at the next flush adds its XP", async () => {
        // UProgressionComponent sends its queued grants at most once per QueuedGrantTimeout (10 s,
        // DefaultGame.ini), so two grants it meant can be 10 s apart and still identical. The guard reads
        // the clock through Date.now; the audit rows (append-only) keep the real time.
        const { UserId } = await MakePlayer();
        const RealNow = Date.now;
        const Later = async (Ms: number) => {
            Date.now = () => RealNow() + Ms;
            try{
                await Grant(UserId, Body);
            }
            finally{
                Date.now = RealNow;
            }
        };

        // The stored time is taken when the first grant's handler ends, so the next flush, sent 10 s after
        // the first, can reach us when that grant looks only 9.9 s old
        await Grant(UserId, Body);
        await Later(9900);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 4, "the next flush is a new grant");

        await Later(4000);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 4, "a retry 4 s later is still caught");

        await Later(5000);
        assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 6, "the end of the window is outside it");
    });

    it("PROGRESSION_REPLAY_WINDOW_S=0 turns the guard off", async () => {
        await WithEnv({ PROGRESSION_REPLAY_WINDOW_S: "0" }, async () => {
            const { UserId } = await MakePlayer();

            await Grant(UserId, Body);
            await Grant(UserId, Body);

            assert.equal(Track(UserId, "MasteryTrack_Weapon_Axe").progress, 4);
            assert.ok(Events(UserId).every((Event) => !/retry of event/.test(Event.note ?? "")));
        });
    });

    it("a confirm or a reset in between means the game server had the answer: the same body again is a new grant", async () => {
        const { UserId } = await MakePlayer();
        const Season = { progress_tracks: [{ progression_id: "season09b", progress: 100 }], objectives: [] };

        await Grant(UserId, Season);
        assert.equal((await Confirm(UserId, "season09b", 1)).status, 200);
        await Grant(UserId, Season);
        assert.equal(Track(UserId, "season09b").progress, 200);

        await WithEnv({ PROGRESSION_ALLOW_DELETE: "1" }, async () => {
            assert.equal((await Call("DELETE", `/progression/${UserId}/season09b`, { gs: true })).status, 200);
        });
        await Grant(UserId, Season);
        assert.equal(Track(UserId, "season09b").progress, 100, "after the reset");
        assert.ok(Events(UserId).every((Event) => !/retry of event/.test(Event.note ?? "")));
    });

    it("a refused grant is never the one a retry repeats", async () => {
        const { UserId } = await MakePlayer();

        assert.equal((await Grant(UserId, { progress_tracks: {}, objectives: [] })).status, 400);
        assert.equal((await Grant(UserId, { progress_tracks: {}, objectives: [] })).status, 400);
        assert.equal(Events(UserId).length, 2);
    });
});

describe("objective values that go backwards", () => {
    it("are stored as sent and logged, unless a new completion cycle started", async () => {
        const { UserId } = await MakePlayer();

        await Grant(UserId, { progress_tracks: [], objectives: [{ objective_id: "OBJ_BACK", value: 5, completed_count: 1 }] });
        await Grant(UserId, { progress_tracks: [], objectives: [{ objective_id: "OBJ_BACK", value: 2, completed_count: 1 }] });

        assert.equal((await Call("GET", `/progression/objectives/${UserId}/OBJ_BACK`, { as: UserId })).json.payload.progress, 2, "stored as sent");
        assert.match(Events(UserId)[1].note, /objective OBJ_BACK went backwards \(5\/1 -> 2\/1\), stored as sent/);
        assert.ok(Warnings.some((Line) => Line.startsWith("progression: objective went backwards: OBJ_BACK")));

        Warnings.length = 0;
        await Grant(UserId, { progress_tracks: [], objectives: [{ objective_id: "OBJ_BACK", value: 0, completed_count: 2 }] });
        assert.ok(!Warnings.some((Line) => Line.startsWith("progression: objective went backwards")), "a new cycle is not backwards");
    });
});

describe("the progression config (PROGRESSION_CONFIG_DIR, ACTIVE_HUNT_PASS)", () => {
    const Season = (BundledConfig as any).payload.paths.find((Path: any) => Path.progression_id === "season09b");

    function Folder(Name: string, Files: Record<string, string>){
        const Where = path.join(Dir, Name);

        fs.mkdirSync(Where, { recursive: true });
        for(const [File, Text] of Object.entries(Files)){
            fs.writeFileSync(path.join(Where, File), Text);
        }

        return Where;
    }

    it("serves the bundled file unchanged when nothing is set", async () => {
        const { UserId } = await MakePlayer();

        assert.equal(CheckProgressionConfig(), "bundled, 10 tracks; active Hunt Pass season09b");
        assert.equal(GetProgressionConfigPayload(), BundledConfig, "the very same object");

        const Served = await Call("GET", "/progression/config", { as: UserId });
        assert.equal(Served.text, JSON.stringify(BundledConfig));
    });

    it("an override from the folder feeds both GET /progression/config and the rank math", async () => {
        const Doubled = { ...Season, requirements: Season.requirements.map((Requirement: any) => ({ ...Requirement, xp_required: Requirement.xp_required * 2 })) };
        const Extra = { progression_id: "season_test", premium_gating_entitlement: "", requirements: [{ rank_id: 0, xp_required: 0 }, { rank_id: 1, xp_required: 10 }] };
        const Where = Folder("override", { "10-season09b.json": JSON.stringify(Doubled), "20-extra.json": JSON.stringify([Extra]) });

        await WithEnv({ PROGRESSION_CONFIG_DIR: Where }, async () => {
            const { UserId } = await MakePlayer();

            assert.match(CheckProgressionConfig(), /^11 tracks, from .*override: season09b replaced, season_test added; active Hunt Pass season09b$/);

            const Paths = (await Call("GET", "/progression/config", { as: UserId })).json.payload.paths;
            assert.equal(Paths.length, 11);
            assert.equal(Paths[0].progression_id, "season09b", "an override keeps its place");
            assert.equal(Paths[0].requirements[1].xp_required, 200);
            assert.equal(Paths[10].progression_id, "season_test");

            // 250 XP is rank 2 with the bundled costs, rank 1 with the doubled ones: a confirm is clamped to 1
            await Grant(UserId, { progress_tracks: [{ progression_id: "season09b", progress: 250 }], objectives: [] });
            assert.equal((await Confirm(UserId, "season09b", 2)).json.payload.confirmed_fremium_rank, 1);
            assert.equal((await Call("GET", `/progression/${UserId}`, { as: UserId })).json.payload.length, 11);
        });

        assert.equal(CheckProgressionConfig(), "bundled, 10 tracks; active Hunt Pass season09b", "back to the bundled file");
    });

    it("refuses a missing folder, bad JSON, a path without ranks, a duplicate id and an unknown ACTIVE_HUNT_PASS", async () => {
        const Cases: [Record<string, string | undefined>, RegExp][] = [
            [{ PROGRESSION_CONFIG_DIR: path.join(Dir, "does-not-exist") }, /does-not-exist does not exist/],
            [{ PROGRESSION_CONFIG_DIR: Folder("empty", {}) }, /holds no \.json file/],
            [{ PROGRESSION_CONFIG_DIR: Folder("badjson", { "a.json": "{ not json" }) }, /a\.json is not valid JSON/],
            [{ PROGRESSION_CONFIG_DIR: Folder("noranks", { "a.json": JSON.stringify({ progression_id: "x", requirements: [] }) }) }, /\(x\): no requirements/],
            [{ PROGRESSION_CONFIG_DIR: Folder("order", { "a.json": JSON.stringify({ progression_id: "x", requirements: [{ rank_id: 2, xp_required: 1 }, { rank_id: 1, xp_required: 1 }] }) }) }, /not in rising rank_id order/],
            [{ PROGRESSION_CONFIG_DIR: Folder("twice", { "a.json": JSON.stringify(Season), "b.json": JSON.stringify(Season) }) }, /season09b is also in/],
            [{ ACTIVE_HUNT_PASS: "season99z" }, /ACTIVE_HUNT_PASS is "season99z", which is not a loaded track/]
        ];

        for(const [Env, Message] of Cases){
            await WithEnv(Env, () => assert.throws(() => CheckProgressionConfig(), Message));
        }
    });

    it("a bad folder stops the metagame at boot with a fatal line naming it", () => {
        const Started = spawnSync(process.execPath, ["build/src/server.js"], {
            cwd: process.cwd(),
            env: { ...process.env, PROGRESSION_CONFIG_DIR: path.join(Dir, "not-there"), LOG_LEVEL: "info", PORT: "9" },
            encoding: "utf8",
            timeout: 60000
        });

        assert.equal(Started.status, 1, Started.stderr);
        assert.match(Started.stdout, /The progression config could not be loaded: PROGRESSION_CONFIG_DIR .*not-there does not exist/);
        assert.doesNotMatch(Started.stdout, /Clear Skies/);
    });

    it("ACTIVE_HUNT_PASS is the Hunt Pass an account has before it chooses one", async () => {
        await WithEnv({ ACTIVE_HUNT_PASS: "MasteryTrack_PlayerLevel" }, async () => {
            const { UserId } = await MakePlayer();

            assert.equal(GetActiveHuntPass(), "MasteryTrack_PlayerLevel");
            assert.equal(GetSelectedHuntPass(UserId), "MasteryTrack_PlayerLevel");
            assert.equal((await Call("GET", `/huntpass/${UserId}`, { as: UserId })).json.payload, "MasteryTrack_PlayerLevel");
        });

        const { UserId } = await MakePlayer();
        assert.equal((await Call("GET", `/huntpass/${UserId}`, { as: UserId })).json.payload, "season09b", "the default");
    });
});

describe("GET /balance and POST /reconcile (BALANCE_FROM_INVENTORY)", () => {
    async function Holding(Stacks: { catalogId: string, quantity: number }[]){
        const Player = await MakePlayer();

        GetDb().insert(inventory).values({ characterId: Player.CharacterId, instancedItems: "[]", stackedItems: JSON.stringify(Stacks) }).run();

        return Player;
    }

    it("reports the currencies the character holds, under both spellings, and keeps the sheet for the rest", async () => {
        const { UserId } = await Holding([{ catalogId: "CURRENCY_NOTES", quantity: 1260 }, { catalogId: "CURRENCY_PLATINUM", quantity: 75 }, { catalogId: "ORB_SHOCK", quantity: 5 }]);

        const Sheet = (await Call("GET", "/balance", { as: UserId })).json;
        assert.deepEqual([Sheet.CURRENCY_NOTES, Sheet.id_currency_notes, Sheet.CURRENCY_PLATINUM, Sheet.id_currency_platinum], [1260, 1260, 75, 75]);
        assert.deepEqual([Sheet.CURRENCY_WEAPON_TOKEN, Sheet.id_currency_weapon_token, Sheet.CURRENCY_PRESTIGE], [25, 25, 0], "not held: the sheet as before");

        const Fixed = await WithEnv({ BALANCE_FROM_INVENTORY: "0" }, async () => (await Call("GET", "/balance", { as: UserId })).json);
        assert.deepEqual(Object.keys(Sheet), Object.keys(Fixed), "no key added, removed or moved");
        assert.deepEqual(Object.keys(Sheet).filter((Key) => Sheet[Key] !== Fixed[Key]).sort(), ["CURRENCY_NOTES", "CURRENCY_PLATINUM", "id_currency_notes", "id_currency_platinum"]);

        const Reconciled = (await Call("POST", "/reconcile", { as: UserId, body: {} })).json;
        assert.deepEqual(Reconciled, { balances: { id_currency_notes: 1260, CURRENCY_NOTES: 1260 }, refreshInventory: true });
    });

    it("BALANCE_FROM_INVENTORY=0 answers the fixed sheet, as before", async () => {
        await WithEnv({ BALANCE_FROM_INVENTORY: "0" }, async () => {
            const { UserId } = await Holding([{ catalogId: "CURRENCY_NOTES", quantity: 1260 }, { catalogId: "CURRENCY_PLATINUM", quantity: 75 }]);

            const Sheet = (await Call("GET", "/balance", { as: UserId })).json;
            assert.deepEqual([Sheet.CURRENCY_NOTES, Sheet.CURRENCY_PLATINUM, Sheet.CURRENCY_WEAPON_TOKEN], [0, 0, 25]);
            assert.deepEqual((await Call("POST", "/reconcile", { as: UserId, body: {} })).json.balances, { id_currency_notes: 0, CURRENCY_NOTES: 0 });
        });
    });

    it("a player with nothing held gets the sheet unchanged", async () => {
        const { UserId } = await MakePlayer();

        const Sheet = (await Call("GET", "/balance", { as: UserId })).json;
        assert.deepEqual([Sheet.CURRENCY_NOTES, Sheet.CURRENCY_PLATINUM, Sheet.CURRENCY_WEAPON_TOKEN], [0, 0, 25]);
    });
});

describe("log lines", () => {
    it("a game server's grant, confirm or track grant that relays another player's token is logged and kept for the named account", async () => {
        const A = await MakePlayer(), B = await MakePlayer();

        assert.equal((await Call("POST", `/progression/${A.UserId}`, { gs: true, as: B.UserId, body: { progress_tracks: [{ progression_id: "season09b", progress: 100 }], objectives: [] } })).status, 200);
        assert.equal((await Call("POST", `/progression/${A.UserId}/season09b/100`, { gs: true, as: B.UserId })).status, 200);
        assert.equal((await Call("POST", `/progression/${A.UserId}/season09b/2/confirm/public`, { gs: true, as: B.UserId })).status, 200);

        assert.deepEqual([Track(A.UserId, "season09b").progress, Track(A.UserId, "season09b").confirmed_fremium_rank, Track(B.UserId, "season09b").progress], [200, 2, 0]);
        for(const What of ["progression grant", "progression grant in a track", "rank confirm"]){
            assert.ok(Warnings.includes(`Game server ${What} for ${A.UserId} carries the token of ${B.UserId}: accepted for ${A.UserId}, the account the request names`), What);
        }
    });

    it("names a progression request no route answered, then the catch-all answers 404", async () => {
        const { UserId } = await MakePlayer();

        assert.equal((await Call("PUT", `/progression/${UserId}/season09b`, { gs: true })).status, 404);
        assert.ok(Warnings.includes(`Unhandled progression request PUT /progression/${UserId}/season09b from a game server`), Warnings.join("\n"));

        await WithEnv({ PROGRESSION_MODE: "stub" }, async () => {
            assert.equal((await Call("GET", `/progression/${UserId}/season09b`, { as: UserId })).status, 404);
        });
        assert.ok(Warnings.includes(`Unhandled progression request GET /progression/${UserId}/season09b from a player`));
    });
});
