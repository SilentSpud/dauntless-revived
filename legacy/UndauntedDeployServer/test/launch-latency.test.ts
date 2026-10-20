import "./setup";
import { RemoveDeployTestDir } from "./deployenv";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import type { ChildProcess } from "node:child_process";
import { after, test } from "node:test";
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createSocket, type Socket } from 'node:dgram';

process.env.SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP = "0.3";
const game = require("../src/controllers/gameservers") as typeof import("../src/controllers/gameservers");
const args = "/Game/Test?MaxPlayers=1?MonsterClass=test";
let pid = 50000;
const children: EventEmitter[] = [];
const spawn = () => {
    const child = Object.assign(new EventEmitter(), { pid: pid++, unref() {} });
    children.push(child);
    return child as unknown as ChildProcess;
};
after(RemoveDeployTestDir);

test('cold starts overlap while spawn pacing and listener readiness are preserved', async () => {
    game.ResetGameserversForTests();
    const dir = await mkdtemp(path.join(tmpdir(), 'launch-ready-'));
    const markers: { file: string; pid: number; port: string }[] = [];
    game.UseProcessFunctionsForTests({ Spawn: spawn, IsAlive: () => true });
    await game.Startup();
    process.env.GAMESERVER_READY_DIR = dir;
    game.UseProcessFunctionsForTests({ Spawn: (_command, args, options) => {
        const child = spawn();
        markers.push({file: options.env!.DR_SERVER_READY_FILE!, pid: child.pid!, port: args[1]});
        return child;
    }, IsAlive: () => true });
    try {
        const first = game.StartupGameserverWithArgs(args);
        const second = game.StartupGameserverWithArgs(args);
        await new Promise(resolve => setTimeout(resolve, 850));
        assert.equal(markers.length, 2, 'one cold start blocked the entire launch queue');
        for (const marker of markers) await writeFile(marker.file, `${marker.pid}:${marker.port}`);
        await Promise.all([first, second]);
    } finally {
        delete process.env.GAMESERVER_READY_DIR;
        await rm(dir, {recursive: true, force: true});
    }
});

test("failed spawns do not add cooldowns, successful launches remain spaced", async () => {
    game.ResetGameserversForTests();
    game.UseProcessFunctionsForTests({ Spawn: () => { throw new Error("test failure"); } });
    const started = Date.now();
    for (let i = 0; i < 3; i++) await assert.rejects(game.Startup(), /Could not start/);
    assert.ok(Date.now() - started < 500, "failed spawns accumulated cooldowns");
    game.ResetGameserversForTests();
    game.UseProcessFunctionsForTests({ Spawn: spawn, IsAlive: () => true });
    await game.Startup();
    const launched = Date.now();
    await game.StartupGameserverWithArgs(args);
    assert.ok(Date.now() - launched >= 250, "successful launches were not spaced");
});

test("hunt exit frees its port immediately and watchdog cleanup cannot duplicate it", async () => {
    game.ResetGameserversForTests();
    game.UseProcessFunctionsForTests({ Spawn: spawn, IsAlive: () => true });
    await game.Startup();
    await game.StartupGameserverWithArgs(args);
    const hunt = game.Gameservers.find(server => !server.isRamsgate)!;
    children.at(-1)!.emit("exit", 0, null);
    assert.ok(!game.Gameservers.includes(hunt));
    assert.equal(game.GameserverStateForTests().FreePorts.filter(port => port === hunt.port).length, 1);
    await game.CleanupServer(hunt);
    assert.equal(game.GameserverStateForTests().FreePorts.filter(port => port === hunt.port).length, 1);
});


test("hunt startup failure during readiness grace is rejected and returns its port", async () => {
    game.ResetGameserversForTests();
    game.UseProcessFunctionsForTests({ Spawn: spawn, IsAlive: () => true });
    await game.Startup();
    const before = game.GameserverStateForTests().FreePorts.length;
    process.env.GAMESERVER_STARTUP_GRACE_MS = "50";
    game.UseProcessFunctionsForTests({ Spawn: () => {
        const child = Object.assign(new EventEmitter(), { pid: pid++, unref() {} });
        setTimeout(() => child.emit("exit", 1, null), 10);
        return child as unknown as ChildProcess;
    }, IsAlive: () => true });
    try {
        await assert.rejects(game.StartupGameserverWithArgs(args), /exited before listening/);
        assert.equal(game.GameserverStateForTests().FreePorts.length, before);
    } finally {
        process.env.GAMESERVER_STARTUP_GRACE_MS = "0";
    }
});


test("hunt allocation waits for the UDP listener when no readiness marker is written", async () => {
    game.ResetGameserversForTests();
    game.UseProcessFunctionsForTests({ Spawn: spawn, IsAlive: () => true });
    await game.Startup();

    let listener: Socket | undefined;
    process.env.GAMESERVER_STARTUP_GRACE_MS = "1500";
    game.UseProcessFunctionsForTests({
        Spawn: (_command, launchArgs) => {
            const child = spawn();
            const port = Number(launchArgs[1]);
            setTimeout(() => {
                listener = createSocket("udp4");
                listener.bind(port, "0.0.0.0");
            }, 100);
            return child;
        },
        IsAlive: () => true
    });

    const started = Date.now();

    try {
        const result = await game.StartupGameserverWithArgs(args);
        assert.ok(Date.now() - started >= 300);
        assert.ok(result.port > 0);
    }
    finally {
        process.env.GAMESERVER_STARTUP_GRACE_MS = "0";
        if(listener !== undefined){
            await new Promise<void>(resolve => listener!.close(() => resolve()));
        }
    }
});
