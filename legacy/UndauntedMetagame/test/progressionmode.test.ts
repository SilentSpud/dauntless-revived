import { RemoveTestDb } from "./setup";
import { after, describe, it, afterEach, mock } from "node:test";
import assert from "node:assert/strict";
import { GetDb } from "../src/db";
import { users } from "../src/db/schema";
import { logger } from "../src/logger";
import { DescribeProgressionMode, IsProgressionModeStub, IsRealProgressionAccount } from "../src/controllers/progressionmode";
import { CountPlayersWithoutRealProgression, GrantProgression, ProgressionUpgradeNotice, SeedProgression } from "../src/controllers/realprogression";
import { MakePlayer } from "./helpers";

const Saved = {Mode: process.env.PROGRESSION_MODE, Accounts: process.env.PROGRESSION_REAL_ACCOUNTS};

function SetEnv(Mode: string | undefined, Accounts: string | undefined){
    if(Mode === undefined){ delete process.env.PROGRESSION_MODE; } else { process.env.PROGRESSION_MODE = Mode; }
    if(Accounts === undefined){ delete process.env.PROGRESSION_REAL_ACCOUNTS; } else { process.env.PROGRESSION_REAL_ACCOUNTS = Accounts; }
}

afterEach(() => {
    SetEnv(Saved.Mode, Saved.Accounts);
    mock.restoreAll();
});
after(() => RemoveTestDb(() => GetDb().$client.close()));

describe("IsRealProgressionAccount", () => {
    it("is real for every account by default (PROGRESSION_MODE unset)", () => {
        SetEnv(undefined, undefined);

        assert.equal(IsRealProgressionAccount("UID-a"), true);
        assert.equal(IsRealProgressionAccount("UID-anyone"), true);
        assert.equal(IsProgressionModeStub(), false);
    });

    it("is real for every account when PROGRESSION_MODE is empty or blank", () => {
        SetEnv("", undefined);
        assert.equal(IsRealProgressionAccount("UID-a"), true);

        SetEnv("   ", undefined);
        assert.equal(IsRealProgressionAccount("UID-a"), true);
    });

    it("is stub for everyone with PROGRESSION_MODE=stub and no list, as upstream", () => {
        SetEnv("stub", undefined);

        assert.equal(IsRealProgressionAccount("UID-a"), false);
        assert.equal(IsProgressionModeStub(), true);
    });

    it("accepts STUB and REAL in any case, with spaces around", () => {
        SetEnv(" STUB ", undefined);
        assert.equal(IsRealProgressionAccount("UID-a"), false);

        SetEnv(" Real ", undefined);
        assert.equal(IsRealProgressionAccount("UID-a"), true);
    });

    it("is real only for the listed accounts in stub mode", () => {
        SetEnv("stub", " UID-a , UID-b,,");

        assert.equal(IsRealProgressionAccount("UID-a"), true);
        assert.equal(IsRealProgressionAccount("UID-b"), true);
        assert.equal(IsRealProgressionAccount("UID-c"), false);
        assert.equal(IsRealProgressionAccount(""), false);
        assert.equal(IsRealProgressionAccount(undefined), false);
    });

    it("is real for every account with PROGRESSION_MODE=real, but never for a missing id", () => {
        SetEnv("real", undefined);

        assert.equal(IsRealProgressionAccount("UID-anyone"), true);
        assert.equal(IsRealProgressionAccount(""), false);
        assert.equal(IsRealProgressionAccount(undefined), false);
    });

    it("is real for every account by default even when PROGRESSION_REAL_ACCOUNTS lists some (the list only matters in stub mode)", () => {
        SetEnv(undefined, "UID-a");

        assert.equal(IsRealProgressionAccount("UID-a"), true);
        assert.equal(IsRealProgressionAccount("UID-b"), true);
    });

    it("treats an unknown mode as real, the default, and says so in the log", () => {
        const Warn = mock.method(logger, "warn", () => {});
        SetEnv("yes", undefined);

        assert.equal(IsRealProgressionAccount("UID-a"), true);
        assert.equal(IsProgressionModeStub(), false);
        assert.equal(Warn.mock.callCount(), 1);
        assert.equal(Warn.mock.calls[0].arguments[0], `PROGRESSION_MODE=yes is not "real" or "stub"; using real (the default). Set PROGRESSION_MODE=stub for upstream's fake max ranks`);

        // Read again with the same value: warned once, not on every request
        IsRealProgressionAccount("UID-b");
        assert.equal(Warn.mock.callCount(), 1);
    });

    it("does not warn for the known values or an unset mode", () => {
        const Warn = mock.method(logger, "warn", () => {});

        for(const Mode of [undefined, "", "real", "stub", "REAL"]){
            SetEnv(Mode, undefined);
            IsRealProgressionAccount("UID-a");
        }

        assert.equal(Warn.mock.callCount(), 0);
    });
});

describe("DescribeProgressionMode (the startup line)", () => {
    it("names real as the default when nothing is set", () => {
        SetEnv(undefined, undefined);

        assert.equal(DescribeProgressionMode(), "real for every account (the default)");
    });

    it("does not call an explicit PROGRESSION_MODE=real the default", () => {
        SetEnv("real", undefined);

        assert.equal(DescribeProgressionMode(), "real for every account");
    });

    it("names an unrecognised value instead of calling it the default (off used to mean stub)", () => {
        mock.method(logger, "warn", () => {});

        SetEnv("off", undefined);
        assert.equal(DescribeProgressionMode(), "real for every account (PROGRESSION_MODE=off is not recognised)");

        SetEnv("0", "UID-a");
        assert.equal(DescribeProgressionMode(), "real for every account (PROGRESSION_MODE=0 is not recognised); PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode");

        // Blank is the same as unset
        SetEnv("  ", undefined);
        assert.equal(DescribeProgressionMode(), "real for every account (the default)");
    });

    it("says that PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode", () => {
        SetEnv(undefined, "UID-a,UID-b");

        assert.equal(DescribeProgressionMode(), "real for every account (the default); PROGRESSION_REAL_ACCOUNTS is ignored outside stub mode");
    });

    it("describes stub mode with the number of listed accounts, worded as before", () => {
        SetEnv("stub", "UID-a,UID-b");

        assert.equal(DescribeProgressionMode(), "stub, real for 2 listed account(s)");
    });
});

describe("the upgrade notice for servers that ran in stub mode", () => {
    it("counts players who own a character and have no stored progression, and nothing else", async () => {
        SetEnv(undefined, undefined);
        const Before = CountPlayersWithoutRealProgression();

        // An account without a character (an admin account that never played) is not a player
        await GetDb().insert(users).values({userId: `UID-nochar-${process.pid}`, name: "NoCharacter", notes: 0});
        assert.equal(CountPlayersWithoutRealProgression(), Before);

        const Old = await MakePlayer();
        const Played = await MakePlayer();
        const Seeded = await MakePlayer();
        assert.equal(CountPlayersWithoutRealProgression(), Before + 3);

        assert.equal(GrantProgression(Played.UserId, {progress_tracks: [{progression_id: "MasteryTrack_PlayerLevel", progress: 8}], objectives: []}, "gameserver").Status, 200);
        assert.equal(SeedProgression(Seeded.UserId, "grandfather", "UID-admin").Status, 200);

        assert.equal(CountPlayersWithoutRealProgression(), Before + 1, `only ${Old.UserId} is left`);
    });

    it("warns in real mode while such players exist, names both ways out, and never changes their data", async () => {
        SetEnv(undefined, undefined);
        const {UserId} = await MakePlayer();

        const Notice = ProgressionUpgradeNotice();
        assert.ok(Notice !== undefined);
        assert.match(Notice, /^\d+ player account\(s\) have no stored progression yet: they start at Slayer level 1/);
        assert.match(Notice, /Nothing was migrated/);
        assert.match(Notice, /SeedProgression with Mode "grandfather"/);
        assert.match(Notice, /PROGRESSION_MODE=stub/);

        // Asking twice changes nothing: still no stored tracks for the player
        ProgressionUpgradeNotice();
        assert.equal((GetDb().$client.prepare("select count(*) n from progress_tracks where accountId = ?").get(UserId) as any).n, 0);
    });

    it("stays quiet in stub mode, where nobody's ranks change", async () => {
        SetEnv("stub", undefined);
        await MakePlayer();

        assert.equal(ProgressionUpgradeNotice(), undefined);
    });

    it("stays quiet once every player has stored progression", async () => {
        SetEnv(undefined, undefined);

        for(const Row of GetDb().$client.prepare("select distinct userId from characters").all() as {userId: string}[]){
            SeedProgression(Row.userId, "fresh", "UID-admin");
        }

        assert.equal(CountPlayersWithoutRealProgression(), 0);
        assert.equal(ProgressionUpgradeNotice(), undefined);
    });
});
