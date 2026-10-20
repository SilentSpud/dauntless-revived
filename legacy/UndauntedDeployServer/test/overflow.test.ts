import './setup';
import assert from 'node:assert/strict';
import { after, test } from 'node:test';
import http from 'node:http';
import { HuntRouter, RemoteLaunch } from '../src/controllers/overflow';
import { CapacityUnavailable } from '../src/controllers/capacity';

process.env.OVERFLOW_DEPLOYSERVER_URL = 'http://127.0.0.1:61011';
process.env.OVERFLOW_AFTER_HUNTS = '4';
after(() => { delete process.env.OVERFLOW_DEPLOYSERVER_URL; delete process.env.OVERFLOW_AFTER_HUNTS; });
const body = {GameMode: 'ISLAND', GameArgs: '', HuntId: 'pursuit', ExpectedPlayers: ['one', 'two']};
const local = {host:'main',port:8770};
const worker = {host:'worker',port:8780};

test('threshold spills whole hunt/party; persistent worlds stay local', async () => {
    let calls = 0;
    const router = new HuntRouter(() => 4, async (_url, request) => { calls++; assert.deepEqual(request, body); return worker; });
    assert.deepEqual(await router.launch(body, async () => local), worker);
    assert.deepEqual(await router.launch({...body, GameMode:'CITY'}, async () => local), local);
    assert.equal(calls, 1);
});
test('below threshold prefers primary and both full refuses without another spawn',async()=>{
    let remoteCalls=0;
    const router=new HuntRouter(()=>0,async()=>{remoteCalls++;return undefined;});
    assert.deepEqual(await router.launch(body,async()=>local),local); assert.equal(remoteCalls,0);
    await assert.rejects(router.launch(body,async()=>{throw new CapacityUnavailable('hunts');}),CapacityUnavailable);
    assert.equal(remoteCalls,1);
});
test('local capacity falls back remotely; explicit worker refusal falls back locally', async () => {
    const router = new HuntRouter(() => 0, async () => worker);
    assert.deepEqual(await router.launch(body, async () => { throw new CapacityUnavailable('ports'); }), worker);
    assert.deepEqual(await new HuntRouter(() => 4, async () => undefined).launch(body, async () => local), local);
});
test('ambiguous worker failure never spawns a duplicate locally', async () => {
    let localCalls = 0;
    await assert.rejects(new HuntRouter(() => 4, async () => { throw new Error('timeout'); }).launch(body, async () => { localCalls++; return local; }), /timeout/);
    assert.equal(localCalls, 0);
});
test('concurrent launches count reservations toward the threshold', async () => {
    let finish!: () => void;
    const gate = new Promise<void>(resolve => finish = resolve);
    const router = new HuntRouter(() => 3, async () => worker);
    const first = router.launch(body, async () => { await gate; return local; });
    assert.deepEqual(await router.launch(body, async () => local), worker);
    finish(); await first;
});
test('remote HTTP contract preserves all hunt data and recognizes capacity', async () => {
    let status = 200;
    const server = http.createServer(async (req, res) => {
        if (req.method === 'GET') { res.setHeader('connection','close'); res.end(JSON.stringify({servers:[]})); return; }
        let data = ''; for await (const chunk of req) data += chunk;
        assert.deepEqual(JSON.parse(data), body);
        res.writeHead(status, {'content-type':'application/json', connection:'close'});
        res.end(JSON.stringify(status === 200 ? worker : {error:'capacity_unavailable'}));
    });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const url = new URL(`http://127.0.0.1:${(server.address() as any).port}`);
    try {
        assert.deepEqual(await RemoteLaunch(url, body), worker);
        status = 503; assert.equal(await RemoteLaunch(url, body), undefined);
    } finally { await new Promise<void>(resolve => server.close(() => resolve())); }
    assert.equal(await RemoteLaunch(url, body), undefined);
});

test('capacity cooldown shares refusals, preserves fallback, and isolates workers and worlds', async t => {
    let now = 1_000_000, probes = 0, posts = 0;
    t.mock.method(Date, 'now', () => now);
    t.mock.method(globalThis, 'fetch', async (_url: unknown, options: RequestInit) => {
        if (options?.method !== 'POST') { probes++; return Response.json({servers:[]}); }
        posts++;
        return Response.json({error:'capacity_unavailable'}, {status:503, headers:{'retry-after':'10'}});
    });
    const url = new URL(process.env.OVERFLOW_DEPLOYSERVER_URL!);
    assert.equal(await RemoteLaunch(url, body), undefined);
    const router = new HuntRouter(() => 4);
    assert.deepEqual(await router.launch(body, async () => local), local);
    await assert.rejects(router.launch(body, async () => { throw new CapacityUnavailable('hunts'); }), CapacityUnavailable);
    assert.equal(probes, 1); assert.equal(posts, 1);
    await RemoteLaunch(new URL('http://127.0.0.1:61012'), body);
    await RemoteLaunch(url, {...body, GameMode:'CITY'});
    assert.equal(posts, 3);
    now += 9999;
    await RemoteLaunch(url, body); assert.equal(posts, 3);
    now++;
    await RemoteLaunch(url, body); assert.equal(posts, 4);
});

test('capacity Retry-After is bounded and invalid values use ten seconds', async t => {
    let now = 2_000_000, posts = 0, header = '';
    t.mock.method(Date, 'now', () => now);
    t.mock.method(globalThis, 'fetch', async (_url: unknown, options: RequestInit) => {
        if (options?.method !== 'POST') return Response.json({servers:[]});
        posts++;
        return Response.json({error:'capacity_unavailable'}, {status:503, headers:{'retry-after':header}});
    });
    for (const [index, [value, delay]] of [['999999',30000], ['0.1',1000], ['invalid',10000], ['-1',10000], ['',10000]].entries()) {
        header = String(value);
        const url = new URL(`http://127.0.0.1:${62000+index}`);
        const before = posts;
        await RemoteLaunch(url, body); assert.equal(posts, before+1);
        now += Number(delay)-1;
        await RemoteLaunch(url, body); assert.equal(posts, before+1);
        now++;
        await RemoteLaunch(url, body); assert.equal(posts, before+2);
    }
});

test('ambiguous POST failures and other HTTP errors never create a capacity cooldown', async t => {
    let posts = 0, ambiguous = true;
    t.mock.method(globalThis, 'fetch', async (_url: unknown, options: RequestInit) => {
        if (options?.method !== 'POST') return Response.json({servers:[]});
        posts++;
        if (ambiguous) throw new Error('ambiguous timeout');
        return Response.json({error:'different_failure'}, {status:503});
    });
    const url = new URL('http://127.0.0.1:63000');
    for (let i=0;i<2;i++) await assert.rejects(RemoteLaunch(url, body), /ambiguous timeout/);
    ambiguous = false;
    for (let i=0;i<2;i++) await assert.rejects(RemoteLaunch(url, body), /Overflow launch failed/);
    assert.equal(posts, 4);
});
