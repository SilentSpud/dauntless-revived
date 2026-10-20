import { RemoveTestDb } from './setup';
import './authenv';
import assert from 'node:assert/strict';
import { after, before, beforeEach, test } from 'node:test';
import http from 'node:http';
import crypto from 'node:crypto';
import type { AddressInfo } from 'node:net';
import { GetDb } from '../src/db';
import { users } from '../src/db/schema';
import { MatchmakingRoster, QUEUED_PLAYER_LEASE_MS } from '../src/controllers/matchmakingroster';
import {
    AcceptPartyInvite, GetPartyOf, InviteToParty, PollParty, ResetPartiesForTests, SetPartyClockForTests
} from '../src/controllers/party';

const HUNT = 'CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr';
const OTHER_HUNT = 'CR19_PlayerHunt_Pursuit_Alpha_McBeaver';
const A = 'UID-roster-queue-a';
const B = 'UID-roster-queue-b';
let now = 0;
let server: http.Server;
let matchmaking: typeof import('../src/controllers/matchmaking');
let refuse = false;
let holdFirst = false;
let held: http.ServerResponse | undefined;
const calls: { body: any; sessionId: string; port: number }[] = [];

function reply(response: http.ServerResponse, index: number) {
    response.writeHead(200, {'content-type': 'application/json'});
    response.end(JSON.stringify({host: '127.0.0.1', port: calls[index].port, sessionId: calls[index].sessionId}));
}

before(async () => {
    for (const userId of [A, B]) GetDb().insert(users).values({userId, name: userId, notes: 0}).run();
    server = http.createServer(async (request, response) => {
        if (request.method === 'GET') {
            response.writeHead(200, {'content-type': 'application/json'});
            response.end(JSON.stringify({servers: calls.map(call => ({id: call.sessionId, port: call.port}))}));
            return;
        }
        let body = '';
        for await (const part of request) body += part;
        const index = calls.length;
        calls.push({body: JSON.parse(body), sessionId: crypto.randomUUID(), port: 8700 + index});
        if (holdFirst && index === 0) { held = response; return; }
        if (refuse) {
            response.writeHead(503, {'content-type': 'application/json'});
            response.end(JSON.stringify({error: 'capacity_unavailable', reason: 'hunts'}));
            return;
        }
        reply(response, index);
    });
    await new Promise<void>((resolve, reject) => {
        server.once('error', reject);
        server.listen(0, '127.0.0.1', resolve);
    });
    process.env.MATCHMAKING_MODE = 'DEPLOYSERVER';
    process.env.DEPLOYSERVER_URL = `127.0.0.1:${(server.address() as AddressInfo).port}`;
    matchmaking = await import('../src/controllers/matchmaking');
});

beforeEach(() => {
    now = 0;
    SetPartyClockForTests(() => now);
    ResetPartiesForTests();
    matchmaking.ResetMatchmakingForTests();
    calls.length = 0;
    refuse = false;
    holdFirst = false;
    held = undefined;
});

after(async () => {
    held?.destroy();
    server.closeAllConnections();
    await new Promise<void>(resolve => server.close(() => resolve()));
    SetPartyClockForTests();
    RemoveTestDb(() => GetDb().$client.close());
});

async function ready(playerId: string) {
    const deadline = Date.now() + 2000;
    do {
        const entry = await matchmaking.CheckAndUpdateQueueStatus(playerId);
        if (entry?.Ready && !entry.Failed) return entry;
        await new Promise<void>(resolve => setTimeout(resolve, 5));
    } while (Date.now() < deadline);
    assert.fail(`player ${playerId} did not receive a ready allocation`);
}

test('a later public traveller starts without the player who stopped polling', async () => {
    assert.equal(await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, A), true);
    now = QUEUED_PLAYER_LEASE_MS + 1;
    assert.equal(await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, B), true);
    now += 5000;
    const entry = await ready(B);
    assert.equal(calls.length, 1);
    assert.deepEqual(calls[0].body.ExpectedPlayers, [B]);
    assert.equal(entry.Port, calls[0].port);
    assert.equal((await matchmaking.CheckAndUpdateQueueStatus(A))?.Failed, true);
});

test('capacity retry resnapshots the roster and omits a silent public member', async () => {
    refuse = true;
    await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, B);
    now = 5000;
    await matchmaking.CheckAndUpdateQueueStatus(B);
    assert.deepEqual(calls[0].body.ExpectedPlayers, [A, B]);
    refuse = false;
    now += QUEUED_PLAYER_LEASE_MS + 1;
    await ready(B);
    assert.equal(calls.length, 2);
    assert.deepEqual(calls[1].body.ExpectedPlayers, [B]);
    assert.equal((await matchmaking.CheckAndUpdateQueueStatus(A))?.Failed, true);
});

test('replacing a candidate during startup removes it from the original server roster', async () => {
    holdFirst = true;
    await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, B);
    now = 5000;
    await matchmaking.CheckAndUpdateQueueStatus(B);
    assert.ok(held, 'the first allocation must still be in flight');
    await matchmaking.HandlePlayerMatchmaking('ISLAND', '', OTHER_HUNT, A, true);
    const replacement = await ready(A);
    assert.equal(calls.length, 2);
    reply(held!, 0);
    held = undefined;
    await ready(B);
    assert.deepEqual(MatchmakingRoster.reconcileExpected([A, B], {sessionId: calls[0].sessionId}), [B]);
    assert.deepEqual(MatchmakingRoster.reconcileExpected([A], {sessionId: calls[1].sessionId}), [A]);
    assert.equal((await matchmaking.CheckAndUpdateQueueStatus(A))?.CandidateId, replacement.CandidateId);
});

test('a party member returning from a temporary city restores the healthy original hunt membership', async () => {
    const party = await PollParty(A) as any;
    await PollParty(B);
    assert.equal(InviteToParty(A, B, party.partyId).Status, 200);
    assert.equal((await AcceptPartyInvite(B, party.partyId)).Status, 200);
    assert.equal(await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, A), true);
    const initial = await ready(B);
    assert.equal((await matchmaking.DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal((await matchmaking.DecideCandidateStatus(B)).Kind, 'travel');
    now = 61_000;
    await PollParty(B);
    assert.equal(GetPartyOf(B)?.Candidate, null);
    assert.equal(GetPartyOf(B)?.LastCandidate?.CandidateId, initial.CandidateId);

    assert.equal(await matchmaking.HandlePlayerMatchmaking('CITY', '', 'Ramsgate', B), true);
    assert.notEqual((await ready(B)).CandidateId, initial.CandidateId);
    assert.deepEqual(MatchmakingRoster.reconcileExpected([A, B], {sessionId: calls[0].sessionId}), [A]);
    assert.equal(await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, B), true);
    const restored = await ready(B);
    assert.equal(restored.CandidateId, initial.CandidateId);
    assert.equal(restored.SessionId, calls[0].sessionId);
    assert.equal(restored.FirstSentAt, initial.FirstSentAt);
    assert.deepEqual(MatchmakingRoster.reconcileExpected([A, B], {sessionId: calls[0].sessionId}), [A, B]);
    assert.equal(calls.length, 2, 'returning to the healthy party hunt must not create a third server');
});

test('a party member departing during startup is omitted from the original allocation', async () => {
    const party = await PollParty(A) as any;
    await PollParty(B);
    assert.equal(InviteToParty(A, B, party.partyId).Status, 200);
    assert.equal((await AcceptPartyInvite(B, party.partyId)).Status, 200);
    holdFirst = true;
    assert.equal(await matchmaking.HandlePlayerMatchmaking('ISLAND', '', HUNT, A), true);
    assert.ok(held);
    const initial = await matchmaking.CheckAndUpdateQueueStatus(B);
    assert.equal(initial?.Ready, false);
    assert.ok(initial?.LaunchDeadline);

    assert.equal(await matchmaking.HandlePlayerMatchmaking('CITY', '', 'Ramsgate', B), true);
    const city = await ready(B);
    assert.notEqual(city.CandidateId, initial.CandidateId);
    assert.equal(matchmaking.JoinPartyCandidateById(B, initial.CandidateId), undefined);
    reply(held!, 0);
    held = undefined;
    assert.equal((await ready(A)).SessionId, calls[0].sessionId);
    assert.equal((await ready(B)).CandidateId, city.CandidateId);
    assert.deepEqual(MatchmakingRoster.reconcileExpected([A, B], {sessionId: calls[0].sessionId}), [A]);
    assert.equal(calls.length, 2, 'a late party launch must not overwrite the departing member\'s city');
});
