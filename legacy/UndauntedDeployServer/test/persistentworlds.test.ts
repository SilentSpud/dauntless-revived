import "./setup";
import { MISSING_BINARY, RemoveDeployTestDir } from "./deployenv";
import { after, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import type { ChildProcess } from "node:child_process";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import {
    CleanupServer, GameserverStateForTests, Gameservers, GetRamsgateConnectionDetails, GetTrainingDojoConnectionDetails, ResetGameserversForTests,
    Startup, StartupAndReportFailure, StartupGameserverWithArgs, UseProcessFunctionsForTests, ShutdownGameservers,
} from "../src/controllers/gameservers";
import { HandleMatchmakingRequest } from "../src/controllers/matchmaker";
import { RunWatchdog } from "../src/controllers/watchdog";
import { logger } from "../src/logger";
import { app } from "../src/app";

// Ramsgate and the Dojo are restarted when a player asks for them and their process is gone, through
// the one launch the watchdog uses too; a game binary that cannot be started is logged instead of
// taking the deploy server down. Ported from github.com/Harmonicrain/Undaunted 895f7c7
// (UndauntedDeployServer/src/controllers/gameservers.ts, server.ts); the shared launch, the switch and
// these tests are ours. No game process is ever started: the spawn is a stand-in, and the one test that
// uses the real spawn points at a file that does not exist.

const TUTORIAL_ARGS = "/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?TODClass=/Game/World/atmospheres/blueprints/atmospheres/experimental/atmospheres_stormy_00_bp.atmospheres_stormy_00_bp_C?HuntID=?ZonePreset=-1";
const HUNT_POOL = [8770, 8771, 8772, 8773, 8774, 8775];

let NextPid = 41000;
const Alive = new Set<number>();
const Spawned: { Command: string, Args: string[], Pid: number | undefined, windowsHide?: boolean, console?: string }[] = [];

// A game process that starts and runs until the test "kills" it (takes its pid out of Alive)
function FakeSpawn(Command: string, Args: string[], Options?: { windowsHide?: boolean, env?: NodeJS.ProcessEnv }): ChildProcess {
    const Child = new EventEmitter() as any;
    Child.pid = NextPid++;
    Child.unref = () => {};
    Alive.add(Child.pid);
    Spawned.push({ Command, Args, Pid: Child.pid, windowsHide: Options?.windowsHide, console: Options?.env?.DR_SHOW_CONSOLE });
    return Child as ChildProcess;
}

function KillableFakeSpawn(Command: string, Args: string[]): ChildProcess {
    const Child = new EventEmitter() as any;
    Child.pid = NextPid++;
    Child.exitCode = null;
    Child.signalCode = null;
    Child.killed = false;
    Child.unref = () => {};
    Child.kill = (Signal = "SIGTERM") => {
        if(Child.exitCode !== null || Child.signalCode !== null)return false;
        Child.killed = true;
        Child.signalCode = Signal;
        Alive.delete(Child.pid);
        process.nextTick(() => Child.emit("exit", null, Signal));
        return true;
    };
    Alive.add(Child.pid);
    Spawned.push({ Command, Args, Pid: Child.pid });
    return Child as ChildProcess;
}



// What Node does for a binary that is not there: no pid, and an "error" event right after
function FailingSpawn(Command: string, Args: string[]): ChildProcess {
    const Child = new EventEmitter() as any;
    Child.pid = undefined;
    Child.unref = () => {};
    Spawned.push({ Command, Args, Pid: undefined });
    process.nextTick(() => Child.emit("error", Object.assign(new Error(`spawn ${Command} ENOENT`), { code: "ENOENT" })));
    return Child as ChildProcess;
}

const IsAlive = (ProcessId: number) => Alive.has(ProcessId);

// The deploy server's log lines, as "level: message"
const Logged: string[] = [];
const LoggerBackup: Record<string, unknown> = {};
for(const Level of ["fatal", "error", "warn", "info"] as const){
    LoggerBackup[Level] = (logger as any)[Level];
    (logger as any)[Level] = (Message: unknown) => { Logged.push(`${Level}: ${String(Message)}`); };
}

async function WaitFor(Condition: () => boolean){
    for(const Deadline = Date.now() + 5000; Date.now() < Deadline && !Condition();){
        await new Promise((Resolve) => setTimeout(Resolve, 10));
    }

    assert.ok(Condition(), "timed out waiting");
}

function Kill(ProcessId: number | undefined){
    Alive.delete(ProcessId as number);
}

beforeEach(() => {
    ResetGameserversForTests();
    UseProcessFunctionsForTests({ Spawn: FakeSpawn, IsAlive });
    Alive.clear();
    Spawned.length = 0;
    Logged.length = 0;
    delete process.env.PERSISTENT_WORLD_LIVENESS;
    delete process.env.ENABLE_DOJO;
});

after(() => {
    UseProcessFunctionsForTests({});
    ResetGameserversForTests();
    Object.assign(logger, LoggerBackup);
    RemoveDeployTestDir();
});

describe("Ramsgate", () => {
    it('preserves legacy server visibility unless Deploy explicitly hides or shows consoles', async () => {
        const original = process.env.DR_SHOW_CONSOLE;
        try {
            for (const diagnostic of [undefined, '0', '1', 'invalid']) {
                if (diagnostic === undefined) delete process.env.DR_SHOW_CONSOLE;
                else process.env.DR_SHOW_CONSOLE = diagnostic;
                ResetGameserversForTests();
                Spawned.length = 0;
                await Startup();
                await GetTrainingDojoConnectionDetails();
                await StartupGameserverWithArgs(TUTORIAL_ARGS);
                assert.equal(Spawned.length, 3);
                for (const [index, child] of Spawned.entries()) {
                    const expected = diagnostic === '0' ? true : diagnostic === '1' ? false : index === 2;
                    assert.equal(child.windowsHide, expected);
                    assert.equal(child.console, diagnostic);
                }
            }
        } finally {
            if (original === undefined) delete process.env.DR_SHOW_CONSOLE;
            else process.env.DR_SHOW_CONSOLE = original;
        }
    });
    it('hard primary capacity excludes Ramsgate and dojo and admits only one concurrent last slot', async()=>{
        await Startup(); await GetTrainingDojoConnectionDetails();
        process.env.MAX_LOCAL_HUNTS='1';
        try {
            const results=await Promise.allSettled(Array.from({length:8},()=>StartupGameserverWithArgs(TUTORIAL_ARGS)));
            assert.equal(results.filter(r=>r.status==='fulfilled').length,1);
            assert.equal(Spawned.length,3);
            assert.equal(Gameservers.filter(s=>!s.isRamsgate&&!s.isTrainingDojo).length,1);
        } finally {delete process.env.MAX_LOCAL_HUNTS;}
    });
    it('worker limits pending launches to two and running hunts to three',async()=>{
        process.env.HUNT_WORKER='1'; process.env.MAX_HUNTS='3'; process.env.MAX_PENDING_HUNTS='2';
        try {
            await Startup();
            const results=await Promise.allSettled(Array.from({length:8},()=>StartupGameserverWithArgs(TUTORIAL_ARGS)));
            assert.equal(results.filter(r=>r.status==='fulfilled').length,2, 'only two launches may be pending at once');
            assert.equal(results.filter(r=>r.status==='rejected').length,6);
            for (const result of results) {
                if (result.status === 'rejected') assert.match(String(result.reason), /capacity unavailable: hunts/);
            }
            assert.equal(Spawned.length,2);
            // Once the burst has spawned, the worker still has its third running slot.
            await StartupGameserverWithArgs(TUTORIAL_ARGS);
            assert.equal(Spawned.length,3);
            await assert.rejects(StartupGameserverWithArgs(TUTORIAL_ARGS), /capacity unavailable: hunts/);
            assert.equal(Spawned.length,3, 'a full worker must not spawn a fourth hunt');
        } finally {delete process.env.HUNT_WORKER;delete process.env.MAX_HUNTS;delete process.env.MAX_PENDING_HUNTS;}
    });
    it('low memory blocks new worlds before spawn without consuming hunt ports or stopping Ramsgate', async () => {
        await Startup();
        process.env.GAMESERVER_MEMORY_GUARD = '1';
        process.env.GAMESERVER_MIN_FREE_MB = '999999999';
        try {
            assert.deepEqual(await GetRamsgateConnectionDetails(), { host: '127.0.0.1', port: 8777, sessionId: GameserverStateForTests().Ramsgate!.id });
            await assert.rejects(GetTrainingDojoConnectionDetails(), /capacity unavailable: memory/);
            await assert.rejects(StartupGameserverWithArgs(TUTORIAL_ARGS), /capacity unavailable: memory/);
            assert.equal(Spawned.length, 1);
            assert.deepEqual(GameserverStateForTests().FreePorts, HUNT_POOL);
        } finally { process.env.GAMESERVER_MEMORY_GUARD = '0'; delete process.env.GAMESERVER_MIN_FREE_MB; }
    });
    it("refuses to spawn over a stale listener already holding Ramsgate's fixed port", async () => {
        UseProcessFunctionsForTests({
            Spawn: FakeSpawn,
            IsAlive,
            IsPortBound: async (Port) => Port === 8777
        });

        await assert.rejects(Startup(), /port 8777 is already in use/);
        assert.equal(Spawned.length, 0, "no replacement process is spawned over the stale listener");
        assert.equal(Gameservers.length, 0);
    });

    it("starts once at boot and is handed out without another start while it runs", async () => {
        await Startup();

        assert.equal(Spawned.length, 1);
        assert.equal(Spawned[0].Args[1], "8777");
        assert.equal(Spawned[0].Args[2], "/Game/Maps/ramsgate/ramsgate_01_persistent");
        assert.deepEqual(GameserverStateForTests().FreePorts, HUNT_POOL);
        assert.deepEqual(await GetRamsgateConnectionDetails(), { host: '127.0.0.1', port: 8777, sessionId: GameserverStateForTests().Ramsgate!.id });
        assert.deepEqual(await HandleMatchmakingRequest("CITY", "", "ShatteredIsles_ReturnToRamsgate", undefined), { host: '127.0.0.1', port: 8777, sessionId: GameserverStateForTests().Ramsgate!.id });
        assert.equal(Spawned.length, 1);
    });

    it("stores the record of a Ramsgate the watchdog restarted, so the next check sees the new process", async () => {
        await Startup();
        const First = GameserverStateForTests().Ramsgate!;
        Kill(First.processId);

        await CleanupServer(First);

        const Second = GameserverStateForTests().Ramsgate!;
        assert.notEqual(Second.processId, First.processId);
        assert.equal(Second.port, 8777);
        assert.deepEqual(Gameservers.map((Server) => Server.processId), [Second.processId], "the dead record is gone");

        // Upstream kept the dead record: the liveness check would have started a third process here
        assert.deepEqual(await GetRamsgateConnectionDetails(), { host: '127.0.0.1', port: 8777, sessionId: GameserverStateForTests().Ramsgate!.id });
        assert.equal(Spawned.length, 2);
    });

    it("is started again when a player travels there after its process ended", async () => {
        await Startup();
        Kill(GameserverStateForTests().Ramsgate!.processId);

        assert.deepEqual(await HandleMatchmakingRequest("CITY", "", "ShatteredIsles_ReturnToRamsgate", undefined), { host: '127.0.0.1', port: 8777, sessionId: GameserverStateForTests().Ramsgate!.id });
        assert.equal(Spawned.length, 2);
        assert.ok(Logged.includes("warn: Ramsgate is not running any more: starting it again before sending anyone there"));
        assert.equal(Gameservers.length, 1);
        assert.equal(Gameservers[0].processId, Spawned[1].Pid);
    });

    it("a watchdog restart and a CITY request at the same time start one process, in either order", async () => {
        await Startup();
        Kill(GameserverStateForTests().Ramsgate!.processId);

        await RunWatchdog();
        const Many = await Promise.all([GetRamsgateConnectionDetails(), GetRamsgateConnectionDetails(), HandleMatchmakingRequest("CITY", "", "", undefined)]);
        await WaitFor(() => GameserverStateForTests().Ramsgate!.processId === Spawned[1]?.Pid);

        assert.ok(Many.every((Details) => Details.port === 8777));
        assert.equal(Spawned.length, 2);

        // The other way round: the request first, then the watchdog
        Kill(GameserverStateForTests().Ramsgate!.processId);
        const Request = GetRamsgateConnectionDetails();
        await RunWatchdog();
        await Request;

        assert.equal(Spawned.length, 3);
        assert.equal(Gameservers.length, 1);
    });

    it("a watchdog round that still holds the dead record does not start a second Ramsgate after the request did", async () => {
        await Startup();
        const Dead = GameserverStateForTests().Ramsgate!;
        Kill(Dead.processId);

        await GetRamsgateConnectionDetails();
        await CleanupServer(Dead);

        assert.equal(Spawned.length, 2);
        assert.equal(Gameservers.length, 1);
    });

    it("with PERSISTENT_WORLD_LIVENESS=0 is left to the watchdog, as before", async () => {
        process.env.PERSISTENT_WORLD_LIVENESS = "0";
        await Startup();
        const Dead = GameserverStateForTests().Ramsgate!;
        Kill(Dead.processId);

        assert.deepEqual(await GetRamsgateConnectionDetails(), { host: '127.0.0.1', port: 8777, sessionId: GameserverStateForTests().Ramsgate!.id });
        assert.equal(Spawned.length, 1, "no liveness restart");
        assert.equal(GameserverStateForTests().Ramsgate, Dead);

        await RunWatchdog();
        await WaitFor(() => GameserverStateForTests().Ramsgate !== Dead);
        assert.equal(Spawned.length, 2, "the watchdog still restarts it");
    });

    it("waits for the boot's own start instead of starting a second one", async () => {
        const Boot = Startup();
        const Request = GetRamsgateConnectionDetails();

        await Promise.all([Boot, Request]);

        assert.equal(Spawned.length, 1);
    });
});

describe("the Training Dojo", () => {
    it("stays stopped after cleanup and starts once on the next concurrent requests", async () => {
        await Startup();
        await GetTrainingDojoConnectionDetails();
        const dead = GameserverStateForTests().Dojo!;
        Kill(dead.processId);
        await CleanupServer(dead);
        await RunWatchdog();
        assert.equal(Spawned.length, 2);
        assert.equal(GameserverStateForTests().Dojo, undefined);
        assert.equal(Gameservers.includes(dead), false);
        await Promise.all([GetTrainingDojoConnectionDetails(), GetTrainingDojoConnectionDetails()]);
        assert.equal(Spawned.length, 3);
        assert.notEqual(GameserverStateForTests().Dojo!.processId, dead.processId);
    });

    it("starts on first use, once for requests that arrive together, and again after its process ended", async () => {
        await Startup();

        const Together = await Promise.all([GetTrainingDojoConnectionDetails(), GetTrainingDojoConnectionDetails(), HandleMatchmakingRequest("SHARED", "", "ShatteredIsles_TrainingDojo", undefined)]);
        assert.ok(Together.every((Details) => Details.port === 8776));
        assert.equal(Spawned.length, 2);
        assert.ok(Logged.includes("info: Starting the Training Dojo on demand"));

        await GetTrainingDojoConnectionDetails();
        assert.equal(Spawned.length, 2);

        Kill(GameserverStateForTests().Dojo!.processId);
        assert.equal((await GetTrainingDojoConnectionDetails()).port, 8776);
        assert.equal(Spawned.length, 3);
    });

    it("with ENABLE_DOJO=1 starts at boot, and a request during the boot waits for that start", async () => {
        process.env.ENABLE_DOJO = "1";

        const Boot = Startup();
        const Request = GetTrainingDojoConnectionDetails();
        await Promise.all([Boot, Request]);

        assert.deepEqual(Spawned.map((Launch) => Launch.Args[1]).sort(), ["8776", "8777"]);
    });
});

describe("hunts", () => {
    it("the watchdog returns a finished hunt's port to the pool", async () => {
        await Startup();

        const Hunt = await StartupGameserverWithArgs(TUTORIAL_ARGS);
        assert.ok(HUNT_POOL.includes(Hunt.port));
        assert.ok(!GameserverStateForTests().FreePorts.includes(Hunt.port));

        Kill(Gameservers.find((Server) => Server.port === Hunt.port)!.processId);
        await RunWatchdog();

        assert.deepEqual([...GameserverStateForTests().FreePorts].sort(), HUNT_POOL);
        assert.equal(Gameservers.some((Server) => Server.port === Hunt.port), false);
        assert.equal(Spawned.length, 2, "Ramsgate was not touched");
    });
});

describe("a game binary that cannot be started", () => {
    it("is logged, gives a hunt's port back and does not end the deploy server (the real spawn, a missing file)", async () => {
        await Startup();
        UseProcessFunctionsForTests({ IsAlive });

        await assert.rejects(StartupGameserverWithArgs(TUTORIAL_ARGS), /Could not start a game server on port 877\d/);
        await WaitFor(() => Logged.some((Line) => Line.startsWith("error: Game server on port")));

        assert.deepEqual(GameserverStateForTests().FreePorts, HUNT_POOL);
        assert.ok(Logged.some((Line) => Line.includes("ENOENT") && Line.includes(MISSING_BINARY)), Logged.join("\n"));
        assert.equal(Gameservers.length, 1, "only Ramsgate");
    });

    it("a failed restart of Ramsgate is an error for that request and a log line for the watchdog", async () => {
        await Startup();
        const Dead = GameserverStateForTests().Ramsgate!;
        Kill(Dead.processId);
        UseProcessFunctionsForTests({ Spawn: FailingSpawn, IsAlive });

        await assert.rejects(GetRamsgateConnectionDetails(), /Could not start a game server on port 8777/);

        // The watchdog's own restart fails the same way: logged, and nothing is left unhandled
        Gameservers.push(Dead);
        await RunWatchdog();
        await WaitFor(() => Logged.some((Line) => Line.startsWith("error: Could not restart the game server on port 8777")));

        // The next request tries again once the binary is back
        UseProcessFunctionsForTests({ Spawn: FakeSpawn, IsAlive });
        assert.equal((await GetRamsgateConnectionDetails()).port, 8777);
    });

    it("a failed startup is one fatal line and a non-zero exit code, not an unhandled rejection", async () => {
        UseProcessFunctionsForTests({ Spawn: FailingSpawn, IsAlive });

        try{
            await StartupAndReportFailure();

            assert.ok(Logged.includes("fatal: Starting the game servers failed: Could not start a game server on port 8777"), Logged.join("\n"));
            assert.equal(process.exitCode, 1);
        }
        finally{
            process.exitCode = 0;
        }
    });

    it("the matchmaking call answers 500, which the metagame reports as FAILED", async () => {
        await Startup();
        Kill(GameserverStateForTests().Ramsgate!.processId);
        UseProcessFunctionsForTests({ Spawn: FailingSpawn, IsAlive });

        const Listening = await new Promise<Server>((Resolve, Reject) => {
            const Started = app.listen(0, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
        });

        try{
            const Port = (Listening.address() as AddressInfo).port;
            const Reply = await fetch(`http://127.0.0.1:${Port}/api/matchmaker/handle-matchmaking-for-player`, {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: JSON.stringify({ GameMode: "CITY", GameArgs: "", HuntId: "ShatteredIsles_ReturnToRamsgate" })
            });

            assert.equal(Reply.status, 500);
            assert.deepEqual(await Reply.json(), { error: "no_game_server" });
        }
        finally{
            Listening.closeAllConnections();
            await new Promise<void>((Resolve) => Listening.close(() => Resolve()));
        }
    });

});

describe("shutdown", () => {
    it("shutdown stops Deploy-owned game servers without restarting Ramsgate", async () => {
        UseProcessFunctionsForTests({ Spawn: KillableFakeSpawn, IsAlive, IsPortBound: async () => false });
        process.env.RAMSGATE_POOL = "1";

        try {
            await Startup();
            await GetRamsgateConnectionDetails();
            await StartupGameserverWithArgs(TUTORIAL_ARGS);

            const Pids = Gameservers.map(Server => Server.processId);
            const SpawnCount = Spawned.length;

            assert.equal(Pids.length, 2);
            assert.ok(Pids.every(Pid => Alive.has(Pid)));

            await ShutdownGameservers();

            assert.ok(Pids.every(Pid => !Alive.has(Pid)));
            assert.equal(Gameservers.length, 0);
            assert.equal(Spawned.length, SpawnCount, "shutdown must not restart Ramsgate");
        } finally {
            process.env.RAMSGATE_POOL = "0";
        }
    });

    it("shutdown interrupts an in-flight startup and prevents new game servers", async () => {
        UseProcessFunctionsForTests({
            Spawn: KillableFakeSpawn,
            IsAlive,
            IsPortBound: async () => false
        });

        process.env.RAMSGATE_POOL = "0";

        const PreviousExitCode = process.exitCode;
        process.env.GAMESERVER_STARTUP_GRACE_MS = "5000";

        try {
            const Boot = StartupAndReportFailure();

            await WaitFor(() => Spawned.length === 1);

            const RamsgatePid = Spawned[0].Pid!;
            assert.ok(Alive.has(RamsgatePid), "Ramsgate is still starting");

            await ShutdownGameservers();
            await Boot;

            assert.equal(Alive.has(RamsgatePid), false);
            assert.equal(Gameservers.length, 0);
            assert.equal(
                process.exitCode,
                PreviousExitCode,
                "an intentional shutdown is not a startup failure",
            );

            await assert.rejects(
                StartupGameserverWithArgs(TUTORIAL_ARGS),
                /Deploy service is shutting down/,
            );

            assert.equal(
                Spawned.length,
                1,
                "no game server may spawn after shutdown begins",
            );
        } finally {
            process.env.GAMESERVER_STARTUP_GRACE_MS = "0";
            process.env.RAMSGATE_POOL = "0";
        }
    });
});
