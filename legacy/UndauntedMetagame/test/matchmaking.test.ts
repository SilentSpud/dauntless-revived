import { RemoveTestDb } from "./setup";
import "./matchmakingenv";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";
import { CancelMatchmaking, CheckAndUpdateQueueStatus, DistinctPlayers, HandlePlayerMatchmaking, ResetMatchmakingForTests } from "../src/controllers/matchmaking";
import { GetDb } from "../src/db";
import { GameSessionForCandidate } from "../src/controllers/matchmaking";
import { SetPartyClockForTests } from "../src/controllers/party";
import { SetRegionReader } from '../src/controllers/huntregion';

// Matchmaking looks up the player's party (controllers/party.ts), which loads the database
after(() => RemoveTestDb(() => GetDb().$client.close()));

describe("CancelMatchmaking (DELETE /candidate)", () => {
    it("takes the player out of the queue, so the status poll can no longer send them to the hunt", async () => {
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-a"), true);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-b"), true);

        const Cancelled = CancelMatchmaking("UID-a");
        assert.equal(Cancelled?.HuntId, "Hunt_Test_A");
        assert.equal(await CheckAndUpdateQueueStatus("UID-a"), undefined);
        assert.equal((await CheckAndUpdateQueueStatus("UID-b"))?.Ready, false);

        // The last player leaving drops the queue, so a new group can start one
        CancelMatchmaking("UID-b");
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-c"), true);
        assert.equal((await CheckAndUpdateQueueStatus("UID-c"))?.HuntId, "Hunt_Test_A");
    });

    it("is a no-op for a player who is not queued", () => {
        assert.equal(CancelMatchmaking("UID-nobody"), undefined);
    });
});

describe("DistinctPlayers (the expected-player list sent to the deploy server)", () => {
    it("lists each account once, in first-seen order", () => {
        // The live list on 22 September 2026 was V, V, O: the hunt server waited for a third player
        assert.deepEqual(DistinctPlayers(["UID-v", "UID-v", "UID-o"]), ["UID-v", "UID-o"]);
        assert.deepEqual(DistinctPlayers(["UID-o", "UID-v", "UID-o", "UID-v"]), ["UID-o", "UID-v"]);
        assert.deepEqual(DistinctPlayers([]), []);
    });
});


describe("concurrent hunt queues", () => {
    it('same hunt queues fill independently in each region', async () => {
        ResetMatchmakingForTests();
        const originalFetch=globalThis.fetch;
        const requests:any[]=[];
        SetRegionReader(id=>id.startsWith('AUS')?'aus':'main');
        globalThis.fetch=(async(_url:any,init:any)=>{
            requests.push(JSON.parse(init.body));
            return new Response(JSON.stringify({host:'127.0.0.1',port:39000+requests.length}),{status:200,headers:{'content-type':'application/json'}});
        }) as typeof fetch;
        try {
            for (let i=0;i<4;i++) {
                await HandlePlayerMatchmaking('ISLAND','','Hunt_Test_A',`AUS${i}`);
                await HandlePlayerMatchmaking('ISLAND','','Hunt_Test_A',`MAIN${i}`);
            }
            await new Promise(resolve=>setTimeout(resolve,20));
            assert.equal(requests.length,2);
            assert.deepEqual(requests[0].ExpectedPlayers,['AUS0','AUS1','AUS2','AUS3']);
            assert.equal(requests[0].Region,'aus');
            assert.deepEqual(requests[1].ExpectedPlayers,['MAIN0','MAIN1','MAIN2','MAIN3']);
            assert.equal(requests[1].Region,undefined);
        } finally {globalThis.fetch=originalFetch;SetRegionReader(()=>'main');ResetMatchmakingForTests();}
    });
    it("separate city candidates share the native game session for zone chat", async () => {
        ResetMatchmakingForTests();
        const originalFetch=globalThis.fetch;
        globalThis.fetch=(async()=>new Response(JSON.stringify({host:"127.0.0.1",port:39009,sessionId:"native-city-1"}),{status:200,headers:{"content-type":"application/json"}})) as typeof fetch;
        try {
            await HandlePlayerMatchmaking("CITY","","","UID-a");
            await HandlePlayerMatchmaking("CITY","","","UID-b");
            const a=await CheckAndUpdateQueueStatus("UID-a"), b=await CheckAndUpdateQueueStatus("UID-b");
            assert.ok(a && b); assert.notEqual(a.CandidateId,b.CandidateId);
            assert.equal(GameSessionForCandidate(a),"native-city-1");
            assert.equal(GameSessionForCandidate(b),"native-city-1");
        } finally {globalThis.fetch=originalFetch; ResetMatchmakingForTests();}
    });
    it("private solo hunts never absorb public players or another private player", async () => {
        ResetMatchmakingForTests();
        const originalFetch = globalThis.fetch;
        const rosters: string[][] = [];
        globalThis.fetch = (async (_url: any, init: any) => {
            rosters.push(JSON.parse(init.body).ExpectedPlayers);
            return new Response(JSON.stringify({host: "127.0.0.1", port: 39000 + rosters.length}), {status:200, headers:{"content-type":"application/json"}});
        }) as typeof fetch;
        try {
            await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-public");
            await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-private", true);
            await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-private2", true);
            await new Promise(resolve=>setTimeout(resolve,20));
            // The second private candidate waits for the older allocation result, then retries alone.
            SetPartyClockForTests(() => Date.now() + 3100);
            await CheckAndUpdateQueueStatus("UID-private2");
            assert.deepEqual(rosters, [["UID-private"], ["UID-private2"]]);
            assert.equal((await CheckAndUpdateQueueStatus("UID-public"))?.Ready, false);
            assert.equal((await CheckAndUpdateQueueStatus("UID-private"))?.Ready, true);
        } finally { globalThis.fetch = originalFetch; SetPartyClockForTests(); ResetMatchmakingForTests(); }
    });
    it("switching a waiting public join to private removes it from the shared roster", async () => {
        ResetMatchmakingForTests();
        const originalFetch = globalThis.fetch;
        const rosters: string[][] = [];
        globalThis.fetch = (async (_url: any, init: any) => {
            rosters.push(JSON.parse(init.body).ExpectedPlayers);
            return new Response(JSON.stringify({host:"127.0.0.1",port:39001}),{status:200,headers:{"content-type":"application/json"}});
        }) as typeof fetch;
        try {
            for (const id of ["UID-a","UID-b"]) await HandlePlayerMatchmaking("ISLAND","","Hunt_Test_A",id);
            await HandlePlayerMatchmaking("ISLAND","","Hunt_Test_A","UID-a",true);
            for (const id of ["UID-c","UID-d","UID-e"]) await HandlePlayerMatchmaking("ISLAND","","Hunt_Test_A",id);
            await new Promise(resolve=>setTimeout(resolve,20));
            assert.deepEqual(rosters,[["UID-a"],["UID-b","UID-c","UID-d","UID-e"]]);
        } finally { globalThis.fetch=originalFetch; ResetMatchmakingForTests(); }
    });
    it("keeps a duplicate join on the same candidate", async () => {
        ResetMatchmakingForTests();
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-a"), true);
        const first = await CheckAndUpdateQueueStatus("UID-a");
        assert.ok(first);
        assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-a"), true);
        const second = await CheckAndUpdateQueueStatus("UID-a");
        assert.equal(second?.CandidateId, first.CandidateId);
    });

    it("opens the next batch while a full batch is still allocating", async () => {
        ResetMatchmakingForTests();
        const originalFetch = globalThis.fetch;
        let release: (() => void) | undefined;
        let calls = 0;

        globalThis.fetch = (async () => {
            calls++;
            await new Promise<void>((resolve) => { release = resolve; });
            return new Response(JSON.stringify({host: "127.0.0.1", port: 39000}), {
                status: 200,
                headers: {"content-type": "application/json"}
            });
        }) as typeof fetch;

        try{
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-a"), true);
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-b"), true);
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-c"), true);

            const fourth = HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-d");
            const returned = await Promise.race([
                fourth.then(() => true),
                new Promise<boolean>((resolve) => setTimeout(() => resolve(false), 250))
            ]);
            assert.equal(returned, true);
            assert.equal(calls, 1);

            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-e"), true);
            const fifth = await CheckAndUpdateQueueStatus("UID-e");
            assert.ok(fifth);
            assert.equal(fifth.Ready, false);

            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_B", "UID-a"), true);
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Test_A", "UID-a"), true);
            const newer = await CheckAndUpdateQueueStatus("UID-a");
            assert.ok(newer);

            release?.();
            await new Promise<void>((resolve) => setTimeout(resolve, 20));

            const afterOldLaunch = await CheckAndUpdateQueueStatus("UID-a");
            assert.equal(afterOldLaunch?.CandidateId, newer.CandidateId);
            assert.equal(afterOldLaunch?.Ready, false);
        }
        finally{
            release?.();
            globalThis.fetch = originalFetch;
            ResetMatchmakingForTests();
        }
    });

    it("keeps a long-running allocation on the same candidate when the client retries", async () => {
        ResetMatchmakingForTests();
        const originalFetch = globalThis.fetch;
        let now = Date.now();
        let release: (() => void) | undefined;
        let calls = 0;
        SetPartyClockForTests(() => now);

        globalThis.fetch = (async () => {
            calls++;
            await new Promise<void>((resolve) => { release = resolve; });
            return new Response(JSON.stringify({host: "127.0.0.1", port: 39002}), {
                status: 200,
                headers: {"content-type": "application/json"}
            });
        }) as typeof fetch;

        try{
            for(const player of ["UID-long-a", "UID-long-b", "UID-long-c", "UID-long-d"]){
                assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Long_A", player), true);
            }

            for(let retry = 0; retry < 40 && calls === 0; retry++){
                await new Promise<void>((resolve) => setTimeout(resolve, 5));
            }

            assert.equal(calls, 1);
            const before = await CheckAndUpdateQueueStatus("UID-long-a");
            assert.ok(before);
            now += 31_000;
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Long_A", "UID-long-a"), true);
            const after = await CheckAndUpdateQueueStatus("UID-long-a");
            assert.equal(after?.CandidateId, before.CandidateId);
            assert.equal(calls, 1);

            release?.();
            for(let retry = 0; retry < 40; retry++){
                const entry = await CheckAndUpdateQueueStatus("UID-long-a");
                if(entry?.Ready){
                    assert.equal(entry.CandidateId, before.CandidateId);
                    return;
                }
                await new Promise<void>((resolve) => setTimeout(resolve, 5));
            }

            assert.fail("allocation never completed");
        }
        finally{
            release?.();
            SetPartyClockForTests();
            globalThis.fetch = originalFetch;
            ResetMatchmakingForTests();
        }
    });

    it("starts a private solo hunt immediately instead of waiting for a public batch", async () => {
        ResetMatchmakingForTests();
        const originalFetch = globalThis.fetch;
        let calls = 0;

        globalThis.fetch = (async () => {
            calls++;
            return new Response(JSON.stringify({host: "127.0.0.1", port: 39001}), {
                status: 200,
                headers: {"content-type": "application/json"}
            });
        }) as typeof fetch;

        try{
            assert.equal(await HandlePlayerMatchmaking("ISLAND", "", "Hunt_Private_A", "UID-private", true), true);

            for(let retry = 0; retry < 40 && calls === 0; retry++){
                await new Promise<void>((resolve) => setTimeout(resolve, 5));
            }

            assert.equal(calls, 1);
            for(let retry = 0; retry < 40; retry++){
                const entry = await CheckAndUpdateQueueStatus("UID-private");
                if(entry?.Ready){
                    assert.equal(entry.Port, 39001);
                    return;
                }
                await new Promise<void>((resolve) => setTimeout(resolve, 5));
            }

            assert.fail("private hunt never became ready");
        }
        finally{
            globalThis.fetch = originalFetch;
            ResetMatchmakingForTests();
        }
    });

});


describe("bounded public grouping window", () => {
    it("launches five seconds after the first join even when another player joins late", async () => {
        ResetMatchmakingForTests();
        let now = 100000;
        SetPartyClockForTests(() => now);
        const originalFetch = globalThis.fetch;
        const requests: any[] = [];
        globalThis.fetch = (async (_url: any, init: any) => {
            requests.push(JSON.parse(init.body));
            return new Response(JSON.stringify({host: '127.0.0.1', port: 39000}), {status: 200});
        }) as typeof fetch;
        try {
            await HandlePlayerMatchmaking('ISLAND', '', 'Hunt_Test_A', 'UID-first');
            now += 4999;
            await HandlePlayerMatchmaking('ISLAND', '', 'Hunt_Test_A', 'UID-late');
            assert.equal((await CheckAndUpdateQueueStatus('UID-first'))?.Ready, false);
            assert.equal(requests.length, 0);
            now += 1;
            assert.equal((await CheckAndUpdateQueueStatus('UID-first'))?.Ready, true);
            assert.equal((await CheckAndUpdateQueueStatus('UID-late'))?.Ready, true);
            assert.equal(requests.length, 1);
            assert.deepEqual(requests[0].ExpectedPlayers, ['UID-first', 'UID-late']);
        } finally {
            globalThis.fetch = originalFetch;
            SetPartyClockForTests();
            ResetMatchmakingForTests();
        }
    });
});
