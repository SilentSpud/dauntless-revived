import { RemoveTestDb } from './setup';
import './matchmakingenv';
import { after, afterEach, before, beforeEach, it, TestContext } from 'node:test';
import assert from 'node:assert/strict';
import { GetDb } from '../src/db';
import { users } from '../src/db/schema';
import {
    CancelMatchmaking, CheckAndUpdateQueueStatus, DecideCandidateStatus, GameSessionForCandidate,
    HandlePlayerMatchmaking, JoinPartyCandidateById, ResetMatchmakingForTests
} from '../src/controllers/matchmaking';
import {
    AcceptPartyInvite, GetPartyOf, InviteToParty, PollParty, ResetPartiesForTests,
    SetPartyClockForTests, TouchPlayer
} from '../src/controllers/party';
import { MatchmakingRoster } from '../src/controllers/matchmakingroster';

const HUNT = 'CR19_PlayerHunt_FTUE_Pursuit_Beta_LeRawr';
const CITY = 'ShatteredIsles_ReturnToRamsgate';
const TUTORIAL = '/Game/Maps/islands/1705/dia_moss_triforce?MaxPlayers=1?MonsterClass=/Game/Monsters/mcrollin/mcbeaver_tutorial_bp.mcbeaver_tutorial_bp_C?HuntID=';
const A = 'UID-recovery-a', B = 'UID-recovery-b';
const originalFetch = globalThis.fetch;
let now: number;
let posts: any[];
let healthCalls: number;
let deploy: (body: any, init: RequestInit) => Promise<Response>;
let health: () => Promise<Response>;

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), {status, headers: {'content-type': 'application/json'}});
const ready = (port = 8790, sessionId?: string) => json({host: '127.0.0.1', port, ...(sessionId ? {sessionId} : {})});
const full = () => json({error: 'capacity_unavailable', reason: 'memory'}, 503);
const flush = () => new Promise<void>(resolve => setImmediate(resolve));
function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => {resolve = done;});
    return {promise, resolve};
}
async function advance(t: TestContext, milliseconds: number) {
    now += milliseconds;
    t.mock.timers.tick(milliseconds);
    await flush();
}
async function formParty() {
    const party = await PollParty(A) as any;
    await PollParty(B);
    assert.equal(InviteToParty(A, B, party.partyId).Status, 200);
    assert.equal((await AcceptPartyInvite(B, party.partyId)).Status, 200);
}
async function startHunt(player = A) {
    assert.equal(await HandlePlayerMatchmaking('ISLAND', '', HUNT, player, true), true);
    await flush();
    return await CheckAndUpdateQueueStatus(player);
}

before(() => {
    for (const userId of [A, B]) GetDb().insert(users).values({userId, name: userId, notes: 0}).run();
});
beforeEach(() => {
    ResetMatchmakingForTests();
    ResetPartiesForTests();
    now = Date.parse('2026-10-09T12:00:00Z');
    SetPartyClockForTests(() => now);
    delete process.env.MATCHMAKING_DEPLOY_TIMEOUT_MS;
    posts = [];
    healthCalls = 0;
    deploy = async () => ready();
    health = async () => json({servers: [], complete: false});
    globalThis.fetch = (async (_url: any, init: RequestInit = {}) => {
        if (init.method === 'POST') {
            const body = JSON.parse(String(init.body));
            posts.push(body);
            return deploy(body, init);
        }
        healthCalls++;
        return health();
    }) as typeof fetch;
});
afterEach(() => {
    globalThis.fetch = originalFetch;
    SetPartyClockForTests();
    delete process.env.MATCHMAKING_DEPLOY_TIMEOUT_MS;
    ResetMatchmakingForTests();
    ResetPartiesForTests();
});
after(() => RemoveTestDb(() => GetDb().$client.close()));

for (const phase of ['headers', 'body'] as const) {
    it(`an unanswered deploy ${phase} reaches FAILED, aborts, and ignores a late success without retrying`, async t => {
        t.mock.timers.enable({apis: ['setTimeout']});
        process.env.MATCHMAKING_DEPLOY_TIMEOUT_MS = '1000';
        const reply = deferred<Response>();
        const body = deferred<unknown>();
        let signal: AbortSignal | undefined;
        deploy = async (_request, init) => {
            signal = init.signal ?? undefined;
            if (phase === 'headers') return reply.promise;
            return {status: 200, json: () => body.promise} as Response;
        };
        await startHunt();
        assert.equal((await DecideCandidateStatus(A)).Kind, 'matching');
        await advance(t, 1000);
        assert.equal(signal?.aborted, true);
        assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
        reply.resolve(ready());
        body.resolve({host: '127.0.0.1', port: 8790});
        await flush();
        const late = await CheckAndUpdateQueueStatus(A);
        assert.equal(late?.Failed, true);
        assert.equal(late?.Ready, false);
        await advance(t, 60000);
        assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
        assert.equal(posts.length, 1, 'an ambiguous timeout must not allocate another game automatically');
    });
}

it('the default deployment deadline permits a 180 second cold start and ends at 240 seconds', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    const reply = deferred<Response>();
    deploy = () => reply.promise;
    await startHunt();
    await advance(t, 200000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'matching');
    await advance(t, 40000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
    reply.resolve(ready());
    await flush();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
});

it('a cold direct join publishes its candidate immediately, answers within 1.5 seconds, and deduplicates retries', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    const reply = deferred<Response>();
    deploy = () => reply.promise;
    const joining = HandlePlayerMatchmaking('CITY', '', CITY, A);
    await flush();
    const pending = await CheckAndUpdateQueueStatus(A);
    assert.ok(pending);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'matching');
    assert.equal(await HandlePlayerMatchmaking('CITY', '', CITY, A), true);
    assert.equal(posts.length, 1);
    await advance(t, 1500);
    assert.equal(await joining, true);
    assert.equal((await CheckAndUpdateQueueStatus(A))?.CandidateId, pending.CandidateId);
    reply.resolve(ready(8777));
    await flush();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
});

it('a configured long party launch remains visible until its fixed deadline and retains its ready window', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    process.env.MATCHMAKING_DEPLOY_TIMEOUT_MS = '900000';
    await formParty();
    const response = deferred<Response>();
    deploy = () => response.promise;
    const joining = HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    await flush();
    const deadline = GetPartyOf(A)?.Candidate?.LaunchDeadline;
    assert.equal(deadline, now + 900000);
    await advance(t, 2500);
    await joining;
    await advance(t, 300000);
    TouchPlayer(A);
    TouchPlayer(B);
    const pending = await PollParty(B) as any;
    assert.equal(pending.candidateState, 'MATCHING');
    assert.equal(GetPartyOf(A)?.Candidate?.LaunchDeadline, deadline, 'polls do not extend the deadline');
    response.resolve(ready(8790, 'native-slow-party'));
    await flush();
    assert.equal((await PollParty(B) as any).candidateState, 'IN_PROGRESS');
    assert.equal((await DecideCandidateStatus(B)).Kind, 'travel');
});

const DIRECT_PATHS = [
    {name: 'CITY', mode: 'CITY', args: '', hunt: CITY},
    {name: 'SHARED', mode: 'SHARED', args: '', hunt: 'ShatteredIsles_TrainingDojo'},
    {name: 'tutorial', mode: 'ISLAND', args: TUTORIAL, hunt: ''}
];
for (const path of DIRECT_PATHS) {
    it(`a late ${path.name} completion cannot overwrite a newer hunt`, async t => {
        t.mock.timers.enable({apis: ['setTimeout']});
        const old = deferred<Response>();
        deploy = request => request.GameMode === path.mode && request.HuntId === path.hunt ? old.promise : Promise.resolve(ready());
        const joining = HandlePlayerMatchmaking(path.mode, path.args, path.hunt, A);
        await flush();
        const hunt = await startHunt();
        assert.equal(hunt?.HuntId, HUNT);
        old.resolve(ready(8777));
        await joining;
        await flush();
        const actual = await CheckAndUpdateQueueStatus(A);
        assert.equal(actual?.CandidateId, hunt?.CandidateId);
        assert.equal(actual?.HuntId, HUNT);
        assert.equal(actual?.Port, 8790);
    });

    it(`cancelling a pending ${path.name} join prevents a late response from recreating it`, async t => {
        t.mock.timers.enable({apis: ['setTimeout']});
        const reply = deferred<Response>();
        deploy = () => reply.promise;
        const joining = HandlePlayerMatchmaking(path.mode, path.args, path.hunt, A);
        await flush();
        assert.ok(CancelMatchmaking(A));
        reply.resolve(ready());
        await joining;
        await flush();
        assert.equal((await DecideCandidateStatus(A)).Kind, 'unknown');
        assert.equal(posts.length, 1);
    });
}

it('repeated solo joins do not extend an existing 20 second park', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    await startHunt();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    await advance(t, 1000);
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, true);
    const parkedAt = (await CheckAndUpdateQueueStatus(A))?.ParkedAt;
    await advance(t, 10000);
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, true);
    assert.equal((await CheckAndUpdateQueueStatus(A))?.ParkedAt, parkedAt);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'matching');
    await advance(t, 10000);
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, true);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal(posts.length, 1);
});

it('a fresh candidate reusing an old hunt port does not inherit its travel park', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    const old = await startHunt();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    await advance(t, 61000);
    const fresh = await startHunt();
    assert.notEqual(fresh?.CandidateId, old?.CandidateId);
    assert.equal(fresh?.Port, old?.Port);
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, true);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal(posts.length, 2);
});

it('party candidate joins do not extend a 20 second park', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    await formParty();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    const initial = await CheckAndUpdateQueueStatus(B);
    assert.ok(initial);
    assert.equal((await DecideCandidateStatus(B)).Kind, 'travel');
    await advance(t, 1000);
    assert.ok(JoinPartyCandidateById(B, initial.CandidateId));
    await advance(t, 10000);
    assert.ok(JoinPartyCandidateById(B, initial.CandidateId));
    assert.equal((await DecideCandidateStatus(B)).Kind, 'matching');
    await advance(t, 10000);
    assert.ok(JoinPartyCandidateById(B, initial.CandidateId));
    assert.equal((await DecideCandidateStatus(B)).Kind, 'travel');
    assert.equal(posts.length, 1);
});

it('legacy repeated travel retries expire from the original first travel and only an explicit join reallocates', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    const initial = await startHunt();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    const firstSent = initial?.FirstSentAt;
    for (let cycle = 0; cycle < 2; cycle++) {
        await advance(t, 1000);
        await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, true);
        await advance(t, 20000);
        assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
        assert.equal((await CheckAndUpdateQueueStatus(A))?.FirstSentAt, firstSent);
    }
    await advance(t, 19000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
    assert.equal(posts.length, 1);
    const fresh = await startHunt();
    assert.notEqual(fresh?.CandidateId, initial?.CandidateId);
    assert.equal(posts.length, 2);
});

it('a healthy native session remains reusable after 60 seconds with immutable first-travel identity', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    deploy = async () => ready(8790, 'native-hunt-1');
    health = async () => json({servers: [{id: 'native-hunt-1', port: 8790}], complete: true});
    const initial = await startHunt();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    const firstSent = initial?.FirstSentAt;
    await advance(t, 61000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, true);
    await advance(t, 20000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal((await CheckAndUpdateQueueStatus(A))?.CandidateId, initial?.CandidateId);
    assert.equal(initial?.FirstSentAt, firstSent);
    assert.equal(posts.length, 1);
});

it('a complete snapshot detects a dead native session even when its port was reused', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    deploy = async () => ready(8790, 'native-old');
    const initial = await startHunt();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    health = async () => json({servers: [{id: 'native-replacement', port: 8790}], complete: true});
    await advance(t, 5000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
    assert.ok(initial);
    assert.equal(GameSessionForCandidate(initial), 'native-old');
    assert.equal(posts.length, 1);
    deploy = async () => ready(8791, 'native-new');
    const fresh = await startHunt();
    assert.notEqual(fresh?.CandidateId, initial.CandidateId);
    assert.equal(fresh?.Port, 8791);
});

it('incomplete or legacy snapshots are unknown, not death, and cannot slide the fallback deadline', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    deploy = async () => ready(8790, 'native-unreachable-worker');
    await startHunt();
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    for (const complete of [false, undefined]) {
        health = async () => json({servers: [], ...(complete === undefined ? {} : {complete})});
        await advance(t, 10000);
        assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    }
    await advance(t, 41000);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
    assert.equal(posts.length, 1);
});

it('concurrent status polls share one bounded health snapshot request', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    deploy = async () => ready(8790, 'native-party');
    await formParty();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    const response = deferred<Response>();
    health = () => response.promise;
    await advance(t, 5000);
    const polling = Promise.all([DecideCandidateStatus(A), DecideCandidateStatus(B), DecideCandidateStatus(A)]);
    await flush();
    assert.equal(healthCalls, 1);
    response.resolve(json({servers: [{id: 'native-party'}], complete: true}));
    assert.deepEqual((await polling).map(status => status.Kind), ['travel', 'travel', 'travel']);
    await DecideCandidateStatus(A);
    assert.equal(healthCalls, 1);
});

it('a dead party allocation is removed from both current and last candidate caches', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    deploy = async () => ready(8790, 'native-party-dead');
    await formParty();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    const initial = await CheckAndUpdateQueueStatus(B);
    assert.ok(initial);
    health = async () => json({servers: [], complete: true});
    await advance(t, 5000);
    assert.equal((await DecideCandidateStatus(B)).Kind, 'failed');
    assert.equal(GetPartyOf(A)?.Candidate, null);
    assert.equal(GetPartyOf(A)?.LastCandidate, null);
    assert.equal(JoinPartyCandidateById(B, initial.CandidateId), undefined);
    deploy = async () => ready(8791, 'native-party-new');
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    assert.notEqual((await CheckAndUpdateQueueStatus(B))?.CandidateId, initial.CandidateId);
});

it('a rejected party restoration preserves the current usable city candidate', async t => {
    t.mock.timers.enable({apis: ['setTimeout']});
    deploy = async body => body.GameMode === 'CITY'
        ? ready(8777, 'native-current-city') : ready(8790, 'native-original-party');
    health = async () => json({servers: [{id: 'native-original-party'}, {id: 'native-current-city'}], complete: true});
    await formParty();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    const original = await CheckAndUpdateQueueStatus(B);
    assert.ok(original);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal((await DecideCandidateStatus(B)).Kind, 'travel');
    await advance(t, 61000);
    await PollParty(B);
    assert.equal(GetPartyOf(B)?.Candidate, null);
    assert.equal(GetPartyOf(B)?.LastCandidate?.CandidateId, original.CandidateId);

    assert.equal(await HandlePlayerMatchmaking('CITY', '', CITY, B), true);
    const city = await CheckAndUpdateQueueStatus(B);
    assert.ok(city?.Ready);
    // The old party cache cannot authorize restoring a roster that was invalidated.
    MatchmakingRoster.cancelCandidate(original.CandidateId);
    assert.equal(JoinPartyCandidateById(B, original.CandidateId), undefined);
    assert.equal(await HandlePlayerMatchmaking('ISLAND', '', HUNT, B), false);
    assert.equal(await CheckAndUpdateQueueStatus(B), city);
    assert.equal((await DecideCandidateStatus(B)).Kind, 'travel');
    assert.equal(city.SessionId, 'native-current-city');
    assert.equal(posts.length, 2);
});

for (const kind of ['private', 'direct', 'party'] as const) {
    it(`a ${kind} capacity retry cannot revive an attempt after its renewed idle lease`, async t => {
        t.mock.timers.enable({apis: ['setTimeout']});
        deploy = async () => full();
        if (kind === 'party') await formParty();
        if (kind === 'direct') await HandlePlayerMatchmaking('CITY', '', CITY, A);
        else await HandlePlayerMatchmaking('ISLAND', '', HUNT, A, kind === 'private');
        await flush();
        const initial = await CheckAndUpdateQueueStatus(A);
        assert.ok(initial);
        const response = deferred<Response>();
        deploy = () => response.promise;
        await advance(t, 10000);
        const retry = CheckAndUpdateQueueStatus(A);
        await flush();
        assert.equal(posts.length, 2);
        await advance(t, 1500);
        await retry;
        TouchPlayer(A);
        TouchPlayer(B);
        // The retry poll at 10 seconds renews the lease to 70 seconds.
        await advance(t, 58500);
        assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
        response.resolve(ready());
        await flush();
        assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
        assert.equal((await CheckAndUpdateQueueStatus(A))?.Ready, false);
        if (kind === 'party') assert.equal((await DecideCandidateStatus(B)).Kind, 'failed');
        assert.equal(posts.length, 2);
    });
}

for (const [header, delay] of [[null,3000],['invalid',3000],['10',10000],['999999',30000]] as const) {
    it(`party polls honor bounded Retry-After ${header} without duplicate launches`, async () => {
        deploy = async () => { const response=full(); if(header!==null)response.headers.set('Retry-After',header); return response; };
        await formParty();
        await HandlePlayerMatchmaking('ISLAND','',HUNT,A);
        await flush();
        assert.equal(posts.length,1);
        now += delay-1;
        TouchPlayer(A); TouchPlayer(B);
        await Promise.all([CheckAndUpdateQueueStatus(A),CheckAndUpdateQueueStatus(B),CheckAndUpdateQueueStatus(A)]);
        assert.equal(posts.length,1);
        const pending=deferred<Response>(); deploy=()=>pending.promise;
        now += 1;
        const polls=Promise.all([CheckAndUpdateQueueStatus(A),CheckAndUpdateQueueStatus(B),CheckAndUpdateQueueStatus(A)]);
        await flush();
        assert.equal(posts.length,2);
        pending.resolve(ready());
        await polls;
        assert.equal((await CheckAndUpdateQueueStatus(A))?.Ready,true);
        assert.equal((await CheckAndUpdateQueueStatus(B))?.Ready,true);
    });
}

it('Ramsgate capacity waits retain a polling player for twenty minutes, then admit them', async () => {
    deploy = async () => full();
    await HandlePlayerMatchmaking('CITY', '', CITY, A);
    await flush();
    const candidate = (await CheckAndUpdateQueueStatus(A))!.CandidateId;
    for (let elapsed = 0; elapsed < 20 * 60; elapsed += 10) {
        now += 10000;
        const result = await DecideCandidateStatus(A);
        assert.equal(result.Kind, 'matching');
        assert.equal((await CheckAndUpdateQueueStatus(A))?.CandidateId, candidate);
    }
    deploy = async () => ready();
    now += 10000;
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal((await CheckAndUpdateQueueStatus(A))?.CandidateId, candidate);
});

it('a later Ramsgate request cannot overtake the oldest waiting player', async () => {
    deploy = async () => full();
    await HandlePlayerMatchmaking('CITY', '', CITY, A);
    await flush();
    now += 1000;
    deploy = async () => ready();
    await HandlePlayerMatchmaking('CITY', '', CITY, B);
    await flush();
    assert.equal(posts.length, 1);
    now += 10000;
    assert.equal((await CheckAndUpdateQueueStatus(B))?.Ready, false);
    assert.equal(posts.length, 1);
    assert.equal((await CheckAndUpdateQueueStatus(A))?.Ready, true);
    assert.equal(posts[1].ExpectedPlayers[0], A);
    now += 10000;
    assert.equal((await CheckAndUpdateQueueStatus(B))?.Ready, true);
    assert.equal(posts[2].ExpectedPlayers[0], B);
});

it('full Ramsgate capacity does not prevent a hunt allocation', async () => {
    deploy = async body => body.GameMode === 'CITY' ? full() : ready();
    await HandlePlayerMatchmaking('CITY', '', CITY, A);
    await flush();
    assert.equal((await startHunt(B))?.Ready, true);
    assert.equal((await CheckAndUpdateQueueStatus(A))?.Ready, false);
    assert.deepEqual(posts.map(body => body.GameMode), ['CITY', 'ISLAND']);
});

it('cancelling the oldest Ramsgate waiter releases the next player', async () => {
    deploy = async () => full();
    await HandlePlayerMatchmaking('CITY', '', CITY, A);
    await flush();
    now += 1000;
    await HandlePlayerMatchmaking('CITY', '', CITY, B);
    await flush();
    assert.equal(posts.length, 1);
    CancelMatchmaking(A);
    deploy = async () => ready();
    now += 10000;
    assert.equal((await CheckAndUpdateQueueStatus(B))?.Ready, true);
    assert.equal(await CheckAndUpdateQueueStatus(A), undefined);
    assert.equal(posts.length, 2);
    assert.deepEqual(posts[1].ExpectedPlayers, [B]);
});

it('actively polling parties retain capacity queue membership beyond five minutes', async () => {
    await formParty();
    deploy = async () => full();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    await flush();
    const candidate = GetPartyOf(A)!.Candidate!;
    const createdAt = candidate.CreatedAt;
    for (let elapsed = 0; elapsed < 20 * 60; elapsed += 10) {
        now += 10000;
        TouchPlayer(A); TouchPlayer(B);
        await PollParty(A);
        await PollParty(B);
        assert.equal(GetPartyOf(A)?.Candidate?.CandidateId, candidate.CandidateId);
        assert.equal((await DecideCandidateStatus(A)).Kind, 'matching');
        assert.equal((await DecideCandidateStatus(B)).Kind, 'matching');
        assert.equal(candidate.CreatedAt, createdAt);
    }
    deploy = async () => ready();
    now += 10000;
    assert.equal((await DecideCandidateStatus(A)).Kind, 'travel');
    assert.equal((await DecideCandidateStatus(B)).Kind, 'travel');
    assert.equal(candidate.CapacityDeadline, undefined);
});

it('party polling without matchmaking polls cannot keep a capacity lease alive', async () => {
    await formParty();
    deploy = async () => full();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    await flush();
    assert.ok(GetPartyOf(A)?.Candidate);
    for (let elapsed = 0; elapsed < 70; elapsed += 10) {
        now += 10000;
        TouchPlayer(A); TouchPlayer(B);
        await PollParty(A);
        await PollParty(B);
    }
    assert.equal(GetPartyOf(A)?.Candidate, null);
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
    assert.equal((await DecideCandidateStatus(B)).Kind, 'failed');
});

it('an orphaned party capacity retry fails its entries and releases the next hunt', async () => {
    await formParty();
    deploy = async () => full();
    await HandlePlayerMatchmaking('ISLAND', '', HUNT, A);
    await flush();
    const old = (await CheckAndUpdateQueueStatus(A))!.CandidateId;
    // Simulate the old party expiry path independently of the new lease guard.
    GetPartyOf(A)!.Candidate = null;
    const nextPlayer = 'UID-recovery-next';
    now += 1000;
    await startHunt(nextPlayer);
    assert.equal(posts.length, 1);
    deploy = async () => ready();
    now += 10000;
    assert.equal((await DecideCandidateStatus(A)).Kind, 'failed');
    assert.equal((await DecideCandidateStatus(B)).Kind, 'failed');
    assert.equal((await CheckAndUpdateQueueStatus(A))?.CandidateId, old);
    assert.equal((await CheckAndUpdateQueueStatus(nextPlayer))?.Ready, true);
    assert.equal(posts.length, 2);
    assert.deepEqual(posts[1].ExpectedPlayers, [nextPlayer]);
});
