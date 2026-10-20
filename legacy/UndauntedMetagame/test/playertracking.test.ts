import './setup';
import './authenv';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { UpdatePlayerActivity, UpdatePlayerLocation, GetRecentPlayerData, PrunePlayerTracking } from '../src/controllers/undauntedapi';

test('inactive tracking expires without removing a live long-running hunt', async () => {
    const original = Date.now;
    let now = 1000000;
    Date.now = () => now;
    try {
        await UpdatePlayerActivity('old', 'city');
        await UpdatePlayerLocation('old', 'old-hunt');
        await UpdatePlayerActivity('active', 'island');
        await UpdatePlayerLocation('active', 'active-hunt');
        for (let i = 0; i < 11; i++) { now += 60000; await UpdatePlayerActivity('active', 'island'); }
        PrunePlayerTracking();
        assert.equal((await GetRecentPlayerData()).find(p => p.UserId === 'active')?.HuntId, 'active-hunt');
        // Returning after the grace period must not inherit an obsolete hunt location.
        await UpdatePlayerActivity('old', 'city');
        assert.equal((await GetRecentPlayerData()).find(p => p.UserId === 'old')?.HuntId, undefined);
    } finally { Date.now = original; }
});
