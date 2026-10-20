import './setup';
import { RemoveDeployTestDir } from './deployenv';
import assert from 'node:assert/strict';
import http from 'node:http';
import { after, test } from 'node:test';
import { ReadWorkerSnapshot, DescribeOverflowSnapshot } from '../src/controllers/overflow';
import { DescribeAusSnapshot } from '../src/controllers/regions';
import { app } from '../src/app';

after(RemoveDeployTestDir);

test('worker snapshots distinguish missing sessions from incomplete observations', async () => {
    let status = 200;
    let body: unknown = {servers: [], complete: true};
    let hang = false;
    const worker = http.createServer((_req, res) => {
        res.writeHead(status, {'content-type': 'application/json', connection: 'close'});
        if (hang) { res.write('{"servers":'); return; }
        res.end(JSON.stringify(body));
    });
    const api = http.createServer(app);
    const listen = (server: http.Server) => new Promise<number>(resolve => server.listen(0, '127.0.0.1', () => resolve((server.address() as any).port)));
    const url = new URL(`http://127.0.0.1:${await listen(worker)}`);
    const apiUrl = `http://127.0.0.1:${await listen(api)}/gameservers`;
    const variables = ['OVERFLOW_DEPLOYSERVER_URL', 'AUS_DEPLOYSERVER_URL', 'GERMANY_DEPLOYSERVER_URL'];
    const old = variables.map(name => process.env[name]);
    for (const name of variables) delete process.env[name];
    try {
        assert.deepEqual(await DescribeOverflowSnapshot(), {servers: [], complete: true});
        assert.deepEqual(await DescribeAusSnapshot(), {servers: [], complete: true});
        assert.deepEqual(await ReadWorkerSnapshot(url), {servers: [], complete: true});
        assert.equal((await (await fetch(apiUrl)).json()).complete, true);

        body = {servers: [{id: 'still-alive', port: 8790}], complete: false};
        assert.deepEqual(await ReadWorkerSnapshot(url), {servers: [{id: 'still-alive', port: 8790}], complete: false});
        process.env.OVERFLOW_DEPLOYSERVER_URL = url.href;
        const partial = await (await fetch(apiUrl)).json();
        assert.equal(partial.complete, false, 'a missing worker snapshot must not certify a session dead');
        assert.ok(partial.servers.some((server: any) => server.id === 'still-alive'));

        body = {servers: []}; // Older workers cannot certify a complete aggregate.
        assert.equal((await ReadWorkerSnapshot(url)).complete, false);
        body = {servers: {}, complete: true};
        assert.deepEqual(await ReadWorkerSnapshot(url), {servers: [], complete: false});
        status = 500;
        assert.deepEqual(await ReadWorkerSnapshot(url), {servers: [], complete: false});

        status = 200;
        body = {servers: [], complete: true};
        assert.equal((await (await fetch(apiUrl)).json()).complete, true);
        hang = true;
        assert.deepEqual(await ReadWorkerSnapshot(url), {servers: [], complete: false}, 'the timeout covers an unfinished JSON body');
    } finally {
        variables.forEach((name, i) => { if (old[i] === undefined) delete process.env[name]; else process.env[name] = old[i]; });
        for (const server of [api, worker]) {
            server.closeAllConnections();
            await new Promise<void>(resolve => server.close(() => resolve()));
        }
    }
    assert.deepEqual(await ReadWorkerSnapshot(url), {servers: [], complete: false}, 'a closed worker is unknown, not an empty fleet');
});
