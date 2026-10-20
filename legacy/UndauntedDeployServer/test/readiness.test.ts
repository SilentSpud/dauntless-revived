import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, writeFile, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { EventEmitter } from 'node:events';
import { createSocket } from 'node:dgram';
import type { ChildProcess } from 'node:child_process';
import { IsUdpPortBound, WaitForServerReady } from '../src/controllers/readiness';

function FakeChild(pid = 123) {
    return Object.assign(new EventEmitter(), { pid, exitCode: null, signalCode: null }) as unknown as ChildProcess;
}

test('readiness accepts a launch marker by port, cleans it, and detects startup exits', async () => {
    const dir = await mkdtemp(path.join(tmpdir(), 'hunt-ready-'));
    const file = path.join(dir, 'ready');
    const child = FakeChild();
    try {
        await writeFile(file, '999:8764');
        await WaitForServerReady(child, 8764, file, 100);
        await assert.rejects(access(file));
        await writeFile(file, '999:8765');
        await assert.rejects(WaitForServerReady(child, 8764, file, 50), /did not start listening/);
        const exiting = FakeChild();
        const waiting = WaitForServerReady(exiting, 8764, file, 1000);
        setTimeout(() => {
            (exiting as any).exitCode = 1;
            exiting.emit('exit', 1, null);
        }, 10);
        await assert.rejects(waiting, /exited before listening/);
        await assert.rejects(access(file));
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test('UDP readiness detects a bound port and waits for a listener when no marker is written', async () => {
    const dir = await mkdtemp(path.join(tmpdir(), 'hunt-udp-ready-'));
    const file = path.join(dir, 'ready');
    const reserve = createSocket('udp4');
    await new Promise<void>((resolve, reject) => {
        reserve.once('error', reject);
        reserve.bind(0, '127.0.0.1', () => resolve());
    });
    const port = (reserve.address() as { port: number }).port;
    await new Promise<void>(resolve => reserve.close(() => resolve()));
    assert.equal(await IsUdpPortBound(port), false);

    const listener = createSocket('udp4');
    const waiting = WaitForServerReady(FakeChild(), port, file, 1500);
    setTimeout(() => listener.bind(port, '0.0.0.0'), 100);

    try {
        await waiting;
        assert.equal(await IsUdpPortBound(port), true);
    } finally {
        await new Promise<void>(resolve => listener.close(() => resolve()));
        await rm(dir, { recursive: true, force: true });
    }
});
