import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
    CandidateRoster, CONNECTED_PLAYER_ABSENCE_GRACE_MS, PLAYER_LOADING_GRACE_MS, QUEUED_PLAYER_LEASE_MS
} from '../src/controllers/matchmakingroster';

function fixture() {
    let now = 0;
    const roster = new CandidateRoster(() => now);
    const advance = (milliseconds: number) => { now += milliseconds; };
    const start = (allocationId = 'allocation', sessionId = 'session', playerId = 'a', candidateId = 'candidate-a') => {
        roster.recordQueued(playerId, candidateId, 'hunt');
        roster.beginAllocation(allocationId, [{playerId, candidateId}]);
        roster.assignAllocation(allocationId, sessionId, '127.0.0.1', 8770);
    };
    return { roster, advance, start };
}

test('only actual activity of that candidate renews its waiting place', () => {
    const {roster, advance} = fixture();
    roster.recordQueued('gone', 'gone-candidate', 'hunt');
    roster.recordQueued('active', 'active-candidate', 'hunt');
    advance(QUEUED_PLAYER_LEASE_MS);
    assert.equal(roster.touchCandidate('gone', 'different-candidate'), false);
    assert.equal(roster.touchCandidate('unknown', 'unknown-candidate'), false);
    assert.equal(roster.touchCandidate('active', 'active-candidate'), true);
    advance(1);
    assert.equal(roster.queueMemberActive('gone', 'gone-candidate'), false);
    assert.equal(roster.queueMemberActive('active', 'active-candidate'), true);
});

test('an in-flight launch gets its full deadline and a refusal does not revive silent members', () => {
    const {roster, advance} = fixture();
    roster.recordQueued('a', 'candidate-a', 'hunt');
    roster.recordQueued('b', 'candidate-b', 'hunt');
    roster.beginAllocation('attempt', [
        {playerId: 'a', candidateId: 'candidate-a'}, {playerId: 'b', candidateId: 'candidate-b'}
    ], 300_000);
    advance(250_000);
    assert.deepEqual(roster.reconcileExpected(['a', 'b'], {allocationId: 'attempt', connectedPlayerIds: []}), ['a', 'b']);
    roster.touchCandidate('b', 'candidate-b');
    roster.releaseAllocation('attempt');
    assert.equal(roster.queueMemberActive('a', 'candidate-a'), false);
    assert.equal(roster.queueMemberActive('b', 'candidate-b'), true);
});

test('long cold startup does not use up the grace granted after the first travel reply', () => {
    const {roster, advance} = fixture();
    roster.recordQueued('a', 'candidate-a', 'hunt');
    roster.beginAllocation('attempt', [{playerId: 'a', candidateId: 'candidate-a'}], 300_000);
    advance(240_000);
    roster.assignAllocation('attempt', 'session');
    advance(20_000);
    roster.served('a', 'candidate-a');
    advance(PLAYER_LOADING_GRACE_MS - 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), ['a']);
    // The server's polling and duplicate travel replies are not player activity.
    roster.served('a', 'candidate-a');
    advance(2);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('expected-player polling cannot renew an abandoned loading player', () => {
    const {roster, advance, start} = fixture();
    start();
    for (let i = 0; i < 6; i++) {
        advance(30_000);
        assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), ['a']);
    }
    advance(1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('complete session snapshots retain connected players and eventually remove a player who left', () => {
    const {roster, advance, start} = fixture();
    start();
    advance(PLAYER_LOADING_GRACE_MS + 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: ['a']}), ['a']);
    advance(PLAYER_LOADING_GRACE_MS);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: ['a']}), ['a']);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), ['a']);
    advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS - 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), ['a']);
    advance(2);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('missing snapshots preserve unresolved players instead of treating the full roster as absent', () => {
    const {roster, advance, start} = fixture();
    start();
    advance(PLAYER_LOADING_GRACE_MS * 10);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session'}), ['a']);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('an incomplete snapshot resets the missing interval for a previously arrived player', () => {
    const {roster, advance, start} = fixture();
    start();
    roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: ['a']});
    advance(PLAYER_LOADING_GRACE_MS + 1);
    roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []});
    advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS - 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session'}), ['a']);
    advance(2);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), ['a']);
    advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('replacement-candidate activity cannot keep the old server waiting for the same player', () => {
    const {roster, advance, start} = fixture();
    start('old-attempt', 'old-session');
    advance(1);
    start('new-attempt', 'new-session', 'a', 'new-candidate');
    roster.touchCandidate('a', 'new-candidate');
    roster.removePlayer('a', 'candidate-a'); // late cancellation of the old candidate
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'old-session'}), []);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'new-session'}), ['a']);
    assert.equal(roster.touchCandidate('a', 'new-candidate'), true);
});

test('a still-connected player is preserved during replacement, with bounded grace after leaving', () => {
    const {roster, advance, start} = fixture();
    start();
    roster.recordQueued('a', 'new-candidate', 'other-hunt');
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: ['a']}), ['a']);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), ['a']);
    advance(CONNECTED_PLAYER_ABSENCE_GRACE_MS + 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('cancellation removes a missing party member and late activity cannot resurrect it', () => {
    const {roster} = fixture();
    for (const playerId of ['a', 'b']) roster.recordQueued(playerId, 'party-candidate', 'hunt');
    roster.beginAllocation('party-attempt', ['a', 'b'].map(playerId => ({playerId, candidateId: 'party-candidate'})));
    roster.assignAllocation('party-attempt', 'party-session');
    roster.removePlayer('b', 'party-candidate');
    assert.equal(roster.touchCandidate('b', 'party-candidate'), false);
    assert.deepEqual(roster.reconcileExpected(['a', 'b'], {sessionId: 'party-session'}), ['a']);
    roster.cancelCandidate('party-candidate');
    assert.deepEqual(roster.reconcileExpected(['a', 'b'], {sessionId: 'party-session'}), []);
});

test('legacy, unknown and restart-surviving sessions retain their supplied roster', () => {
    const {roster, start} = fixture();
    start();
    roster.removePlayer('a', 'candidate-a');
    assert.deepEqual(roster.reconcileExpected(['a']), ['a']);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'unknown'}), ['a']);
    assert.deepEqual(roster.reconcileExpected(['untracked-player'], {sessionId: 'session'}), ['untracked-player']);
    roster.reset();
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session'}), ['a']);
});

test('connected evidence for another session cannot rescue an abandoned allocation', () => {
    const {roster, advance, start} = fixture();
    start('old-attempt', 'old-session');
    start('new-attempt', 'new-session', 'a', 'new-candidate');
    roster.reconcileExpected(['a'], {sessionId: 'new-session', connectedPlayerIds: ['a']});
    advance(1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'old-session', connectedPlayerIds: []}), []);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'new-session'}), ['a']);
});

test('recreating a ready party entry restores the original member without restarting loading grace', () => {
    const {roster, advance, start} = fixture();
    start();
    roster.served('a', 'candidate-a');
    advance(1000);
    roster.removePlayer('a', 'candidate-a', 'replaced');
    roster.recordQueued('a', 'city-candidate', 'city');
    advance(170_000);
    // The party's healthy old candidate is selected by the controller again.
    roster.recordQueued('a', 'candidate-a', 'hunt');
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt', 'session'), true);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session'}), ['a']);
    roster.served('a', 'candidate-a');
    advance(QUEUED_PLAYER_LEASE_MS + 1);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session', connectedPlayerIds: []}), []);
});

test('an explicit member cancellation cannot be undone by ready-candidate restoration', () => {
    const {roster, start} = fixture();
    start();
    roster.removePlayer('a', 'candidate-a');
    roster.recordQueued('a', 'candidate-a', 'hunt');
    // Replacing the recreated map entry must not erase the earlier cancellation.
    roster.removePlayer('a', 'candidate-a', 'replaced');
    roster.recordQueued('a', 'candidate-a', 'hunt');
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt', 'session'), false);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session'}), []);
});

test('restoration requires the original candidate and native session membership', () => {
    const {roster, start} = fixture();
    start();
    start('other-attempt', 'other-session', 'b', 'candidate-b');
    roster.recordQueued('a', 'replacement', 'city');
    roster.recordQueued('a', 'candidate-a', 'hunt');
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt', 'other-session'), false);
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'wrong-hunt', 'session'), false);
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt'), true);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'session'}), ['a']);
});

test('cancelling a candidate also invalidates its replaced historical members', () => {
    const {roster, start} = fixture();
    start();
    roster.recordQueued('a', 'replacement', 'city');
    roster.cancelCandidate('candidate-a');
    roster.recordQueued('a', 'candidate-a', 'hunt');
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt', 'session'), false);
});

test('restoration finds the original candidate when another allocation shares its pooled native session', () => {
    const {roster, start} = fixture();
    start('older-attempt', 'pooled-session', 'a', 'candidate-a');
    start('later-attempt', 'pooled-session', 'b', 'candidate-b');
    roster.recordQueued('a', 'temporary-candidate', 'other-hunt');
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt', 'pooled-session'), true);
    assert.equal(roster.touchCandidate('a', 'candidate-a'), true);
    assert.deepEqual(roster.reconcileExpected(['a'], {allocationId: 'older-attempt'}), ['a']);
    assert.deepEqual(roster.reconcileExpected(['b'], {allocationId: 'later-attempt'}), ['b']);
});

test('a failed restoration leaves the current unrelated membership usable', () => {
    const {roster, start} = fixture();
    start('original-attempt', 'original-session', 'a', 'candidate-a');
    start('current-attempt', 'current-session', 'a', 'current-candidate');
    assert.equal(roster.restoreAssigned('a', 'candidate-a', 'hunt', 'wrong-session'), false);
    assert.equal(roster.touchCandidate('a', 'current-candidate'), true);
    assert.deepEqual(roster.reconcileExpected(['a'], {sessionId: 'current-session'}), ['a']);
});
