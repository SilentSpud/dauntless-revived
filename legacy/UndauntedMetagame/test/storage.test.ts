import { RemoveTestDb } from "./setup";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { ApplyJournalMode, GetDb } from "../src/db";
import { AddEncounteredContent, GetBreadcrumbsForCharacterIdAndUserId, QueryEncounteredContent, SetBreadcrumbsForCharacterIdAndUserId } from "../src/controllers/progression";
import { MakePlayer } from "./helpers";

const Database: any = require("better-sqlite3");

after(() => RemoveTestDb(() => GetDb().$client.close()));

describe("database", () => {
    it("keeps the rollback journal, so a copy of the file alone is complete", () => {
        assert.equal(GetDb().$client.pragma("journal_mode", {simple: true}), "delete");
        assert.equal(fs.existsSync(process.env.DB_FILENAME + "-wal"), false);
    });

    it("switches a file left in WAL mode (stopped without a checkpoint) back, keeping every commit", () => {
        const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "undaunted-wal-"));
        const Live = path.join(Dir, "live.db");
        const Copy = path.join(Dir, "copy.db");
        const Writer = new Database(Live);

        try{
            Writer.pragma("journal_mode = WAL");
            Writer.pragma("wal_autocheckpoint = 0");
            Writer.exec("create table t(v integer); insert into t values (1), (2), (3)");
            assert.ok(fs.statSync(Live + "-wal").size > 0);

            // What a forced stop leaves behind: the commits are only in the -wal
            fs.copyFileSync(Live, Copy);
            fs.copyFileSync(Live + "-wal", Copy + "-wal");

            const Reopened = new Database(Copy);

            try{
                assert.equal(ApplyJournalMode(Reopened, false), "delete");
                assert.equal(Reopened.prepare("select count(*) n from t").get().n, 3);
            }
            finally{
                Reopened.close();
            }

            assert.equal(fs.existsSync(Copy + "-wal"), false);

            const Alone = new Database(Copy, {readonly: true});

            try{
                assert.equal(Alone.prepare("select count(*) n from t").get().n, 3);
            }
            finally{
                Alone.close();
            }
        }
        finally{
            Writer.close();
            fs.rmSync(Dir, {recursive: true, force: true});
        }
    });

    it("uses WAL only when asked (DB_WAL=1)", () => {
        const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "undaunted-wal-"));
        const Client = new Database(path.join(Dir, "opt-in.db"));

        try{
            assert.equal(ApplyJournalMode(Client, true), "wal");
            assert.equal(ApplyJournalMode(Client, false), "delete");
        }
        finally{
            Client.close();
            fs.rmSync(Dir, {recursive: true, force: true});
        }
    });

    it("has the save-history tables and the append-only triggers after migrating", () => {
        const Names = (GetDb().$client.prepare("select name from sqlite_master where type in ('table', 'trigger')").all() as any[]).map((Row) => Row.name);

        for(const Name of ["characterhistory", "loadouthistory", "inventorylog", "inventorytransactions", "inventorylog_no_update", "inventorylog_no_delete", "users", "invitecodes"]){
            assert.ok(Names.includes(Name), `${Name} missing`);
        }
    });

    it("has the real-progression tables and the audit triggers after migrating", () => {
        const Names = (GetDb().$client.prepare("select name from sqlite_master where type in ('table', 'trigger')").all() as any[]).map((Row) => Row.name);

        for(const Name of ["progress_tracks", "objectives", "progression_events", "huntpassselection", "entitlements", "cooldowns", "bounties", "bountydraft", "loadoutslots", "progression_events_no_update", "progression_events_no_delete"]){
            assert.ok(Names.includes(Name), `${Name} missing`);
        }
    });
});

describe("encountered content and breadcrumbs", () => {
    it("keeps every entry when many are added at once (a lost update before)", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        const Results = await Promise.all(Array.from({length: 20}, (_, Index) => AddEncounteredContent(UserId, CharacterId, 5, `NPC_${Index}`)));
        assert.ok(Results.every((Result) => Result.success));

        const Query = await QueryEncounteredContent(UserId, CharacterId, [5]);
        assert.ok(Query.success);
        assert.equal(Query.data![0].content.length, 20);
    });

    it("creates the breadcrumbs row once under concurrent reads", async () => {
        const {UserId, CharacterId} = await MakePlayer();

        const Results = await Promise.all(Array.from({length: 5}, () => GetBreadcrumbsForCharacterIdAndUserId(UserId, CharacterId)));

        assert.ok(Results.every((Result) => Result.success));
    });

    it("lets only one of two breadcrumb writes with the same version win", async () => {
        const {UserId, CharacterId} = await MakePlayer();
        await GetBreadcrumbsForCharacterIdAndUserId(UserId, CharacterId);

        const [A, B] = await Promise.all([
            SetBreadcrumbsForCharacterIdAndUserId(UserId, CharacterId, ["a"], 1),
            SetBreadcrumbsForCharacterIdAndUserId(UserId, CharacterId, ["b"], 1)
        ]);

        assert.deepEqual([A.success, B.success].sort(), [false, true]);
    });

    it("refuses another player's character", async () => {
        const A = await MakePlayer();
        const B = await MakePlayer();

        assert.deepEqual(await AddEncounteredContent(A.UserId, B.CharacterId, 5, "NPC"), {success: false, error: "forbidden"});
    });
});
