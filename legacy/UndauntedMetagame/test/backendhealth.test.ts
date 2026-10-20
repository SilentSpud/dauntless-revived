import './setup';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import express from 'express';
import { RequestMetrics, TrackBackendHealth, requestMetrics } from '../src/middleware/BackendHealth';

test('rolling request metrics count errors, exclude aborted latency and expire old samples', () => {
    const metrics = new RequestMetrics();
    metrics.record(200, 4, false, 100000);
    metrics.record(404, 30, false, 100000);
    metrics.record(503, 6000, false, 100000);
    metrics.record(200, 100, true, 100000);
    const value = metrics.snapshot(100000);
    assert.equal(value.completed, 3);
    assert.equal(value.clientErrors, 1);
    assert.equal(value.serverErrors, 1);
    assert.equal(value.aborted, 1);
    assert.equal(value.latencyP50UpperMs, 50);
    assert.equal(value.latencyP95UpperMs, '>5000');
    assert.equal(metrics.snapshot(160000).completed, 0);
    assert.equal(metrics.snapshot(160000).latencyP95UpperMs, null);
});

test('instrumentation keeps HTTP replies intact, counts once and excludes monitoring polls', async () => {
    process.env.BACKEND_HEALTH = '1';
    const app = express();
    app.use(TrackBackendHealth);
    app.get('/fixture', (_req, res) => { res.status(503).json({fixture: true}); });
    app.get('/undaunted/api/ServerStatus', (_req, res) => { res.json({}); });
    const server = app.listen(0, '127.0.0.1');
    await new Promise<void>(resolve => server.once('listening', resolve));
    const base = `http://127.0.0.1:${(server.address() as any).port}`;
    try {
        const before = requestMetrics.snapshot().completed;
        const response = await fetch(base + '/fixture');
        assert.equal(response.status, 503);
        assert.deepEqual(await response.json(), {fixture: true});
        await fetch(base + '/undaunted/api/ServerStatus');
        assert.equal(requestMetrics.snapshot().completed, before + 1);
        process.env.BACKEND_HEALTH = '0';
        await fetch(base + '/fixture');
        assert.equal(requestMetrics.snapshot().completed, before + 1);
    } finally { delete process.env.BACKEND_HEALTH; await new Promise<void>(resolve => server.close(() => resolve())); }
});
