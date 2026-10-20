import { RemoveTestDb } from './setup';
import './authenv';
import { after, before, beforeEach, test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import express from 'express';
import { matchmakingRouter } from '../src/routes/matchmaking';
import { SignMetagameJWTForUid } from '../src/controllers/auth';
import {
    CONNECTED_PLAYER_ABSENCE_GRACE_MS, MatchmakingRoster, PLAYER_LOADING_GRACE_MS, QUEUED_PLAYER_LEASE_MS
} from '../src/controllers/matchmakingroster';
import { GetDb } from '../src/db';
import { gameserverapikeys } from '../src/db/schema';

const SERVER_KEY = 'roster-route-test-server-key';
const SESSION = '5f49491e-8a18-41fb-9e6c-5fbd7d655802';
const A = 'UID-roster-a';
const B = 'UID-roster-b';
let now = 0;
let server: http.Server;
let base: string;

before(async () => {
    delete process.env.MISC_ROUTES;
    delete process.env.GATEWAY_SECRET;
    await GetDb().insert(gameserverapikeys).values({keyHash: crypto.createHash('sha256').update(SERVER_KEY).digest('hex')});
    const app = express();
    app.use(express.json());
    app.use(matchmakingRouter);
    server = http.createServer(app);
    await new Promise<void>((resolve, reject) => {
        server.once('error', reject);
        server.listen(0, '127.0.0.1', resolve);
    });
    base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

beforeEach(() => {
    now = 0;
    MatchmakingRoster.reset();
    MatchmakingRoster.setClock(() => now);
});

after(async () => {
    await new Promise<void>(resolve => server.close(() => resolve()));
    MatchmakingRoster.reset();
    RemoveTestDb(() => GetDb().$client.close());
});

async function post(path: string, body: unknown, headers: Record<string, string> = {'x-undaunted-gameserver-apikey': SERVER_KEY}) {
    const response = await fetch(base + path, {method: 'POST', headers: {'content-type': 'application/json', ...headers}, body: JSON.stringify(body)});
    return {status: response.status, body: await response.json()};
}

function assigned() {
    for (const playerId of [A, B]) MatchmakingRoster.recordQueued(playerId, `candidate-${playerId}`, 'hunt');
    MatchmakingRoster.beginAllocation('attempt', [A, B].map(playerId => ({playerId, candidateId: `candidate-${playerId}`})));
    MatchmakingRoster.assignAllocation('attempt', SESSION, '127.0.0.1', 8770);
}

function serverContext(connectedPlayerIds?: string[], sessionId = SESSION) {
    return {
        'x-undaunted-gameserver-apikey': SERVER_KEY,
        'x-dauntless-game-session-id': sessionId,
        ...(connectedPlayerIds === undefined ? {} : {'x-dauntless-connected-player-ids': JSON.stringify(connectedPlayerIds)})
    };
}

test('a game server receives the reconciled roster only for its tagged session', async () => {
    assigned();
    MatchmakingRoster.removePlayer(B, `candidate-${B}`);
    const scoped = await post(`/candidate/player/alive?sessionId=${SESSION}&purpose=expected`, {playerIds: [A, B]});
    assert.equal(scoped.status, 200);
    assert.deepEqual(scoped.body, {expectedPlayerIds: [A]});
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext())).body, {expectedPlayerIds: [A]});
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]})).body, {expectedPlayerIds: [A, B]});
    assert.deepEqual((await post('/candidate/player/alive?sessionId=unknown&purpose=expected', {playerIds: [A, B]})).body, {expectedPlayerIds: [A, B]});
    MatchmakingRoster.reset();
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([]))).body, {expectedPlayerIds: [A, B]});
});

test('a complete native snapshot preserves connected players and removes an expired no-show', async () => {
    assigned();
    now = PLAYER_LOADING_GRACE_MS + 1;
    const result = await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([A]));
    assert.deepEqual(result.body, {expectedPlayerIds: [A]});
    now += PLAYER_LOADING_GRACE_MS * 10;
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([A]))).body, {expectedPlayerIds: [A]});
});

test('a connected player who leaves is eventually omitted while the remaining player is retained', async () => {
    assigned();
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([A, B]))).body, {expectedPlayerIds: [A, B]});
    now = PLAYER_LOADING_GRACE_MS + 1;
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([A]))).body, {expectedPlayerIds: [A, B]});
    now += CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1;
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([A]))).body, {expectedPlayerIds: [A]});
});

test('absent, malformed, or oversized connection snapshots cannot expire unresolved players', async () => {
    assigned();
    now = PLAYER_LOADING_GRACE_MS + 1;
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext())).body, {expectedPlayerIds: [A, B]});
    for (const snapshot of [
        '', '[', '{}', JSON.stringify([A, A]), JSON.stringify(['']), JSON.stringify(['space in id']),
        JSON.stringify(['nonascii-\u00e9']), JSON.stringify(['line\nfeed']), JSON.stringify(['x'.repeat(129)]),
        JSON.stringify(Array.from({length: 129}, (_, i) => `player-${i}`)),
        JSON.stringify(Array.from({length: 70}, (_, i) => `${i}-${'x'.repeat(125)}`))
    ]) {
        const headers = {...serverContext(), 'x-dauntless-connected-player-ids': snapshot};
        assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, headers)).body, {expectedPlayerIds: [A, B]}, snapshot.slice(0, 80));
    }
});

test('a snapshot cannot be applied to a conflicting, absent, or malformed session tag', async () => {
    assigned();
    now = PLAYER_LOADING_GRACE_MS + 1;
    const body = {playerIds: [A, B]};
    const otherSession = '9c7e8098-d2ad-4d5d-9df4-4028da3a21ae';
    assert.deepEqual((await post(`/candidate/player/alive?sessionId=${otherSession}&purpose=expected`, body, serverContext([]))).body, {expectedPlayerIds: [A, B]});
    assert.deepEqual((await post(`/candidate/player/alive?sessionId=${SESSION}&purpose=expected`, body, serverContext([], 'invalid'))).body, {expectedPlayerIds: [A, B]});
    assert.deepEqual((await post('/candidate/player/alive?purpose=connected', body, serverContext([]))).body, {expectedPlayerIds: [A, B]});
    assert.deepEqual((await post(`/candidate/player/alive?sessionId=${SESSION}&purpose=expected`, body, {
        'x-undaunted-gameserver-apikey': SERVER_KEY, 'x-dauntless-connected-player-ids': '[]'
    })).body, {expectedPlayerIds: [A, B]});
});

test('player tokens cannot use native headers to prune players or claim that they arrived', async () => {
    assigned();
    MatchmakingRoster.removePlayer(B, `candidate-${B}`);
    const caller = {
        authorization: `bearer ${SignMetagameJWTForUid(A)}`,
        'x-dauntless-game-session-id': SESSION, 'x-dauntless-connected-player-ids': JSON.stringify([B])
    };
    const result = await post(`/candidate/player/alive?sessionId=${SESSION}&purpose=expected`, {playerIds: [A, B]}, caller);
    assert.deepEqual(result.body, {expectedPlayerIds: [A, B]});
    assert.deepEqual((await post('/candidate/player/alive', {playerIds: [A, B]}, serverContext([]))).body, {expectedPlayerIds: [A]});
});

test('platform registrations do not renew waiting leases or resurrect cancelled candidates', async () => {
    for (const playerId of [A, B]) MatchmakingRoster.recordQueued(playerId, `candidate-${playerId}`, 'hunt');
    now = QUEUED_PLAYER_LEASE_MS + 1;
    const caller = {authorization: `bearer ${SignMetagameJWTForUid(A)}`};
    assert.equal((await post('/candidate/player/register', {userId: B, playerIds: [B]}, caller)).status, 200);
    assert.equal(MatchmakingRoster.queueMemberActive(A, `candidate-${A}`), false);
    assert.equal(MatchmakingRoster.queueMemberActive(B, `candidate-${B}`), false);
    MatchmakingRoster.removePlayer(A, `candidate-${A}`);
    await post('/candidate/player/register', {}, caller);
    assert.equal(MatchmakingRoster.queueMemberActive(A, `candidate-${A}`), false);
    await post('/candidate/player/register', {}, {
        'x-undaunted-gameserver-apikey': SERVER_KEY, authorization: `bearer ${SignMetagameJWTForUid(B)}`
    });
    assert.equal(MatchmakingRoster.queueMemberActive(B, `candidate-${B}`), false);
});
