import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import { AllowlistFeed } from '../src/feed';

// A helper that is down must not be asked again on every single login. One live host logged
// 222,875 suppressed warnings in four hours against two endpoints it was never going to reach
// (workers that host does not run), because every player login retried all of them.

function Clock(){
    let now = 1_000_000;
    return { now: () => now, advance: (ms: number) => { now += ms; } };
}

test('a helper that keeps failing is left alone instead of retried on every login', async () => {
    let requests = 0;
    const server = http.createServer((_req, res) => { requests++; res.writeHead(503); res.end(); });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${(server.address() as any).port}`;
    const clock = Clock();
    const feed = new AllowlistFeed({url, secret: 'test-secret', refreshMs: 60_000, timeoutMs: 1000}, clock.now);

    try {
        // Distinct addresses, so the refresh interval never hides the behaviour under test.
        for (let i = 0; i < 12; i++) { feed.Report(`8.8.8.${i}`); await feed.Idle(); }
        // Three attempts before the backoff engages, and nothing after it does.
        assert.equal(requests, 3, `expected 3 attempts before backing off, got ${requests}`);

        // Still inside the first backoff window: nothing is sent.
        clock.advance(1_500);
        feed.Report('9.9.9.9'); await feed.Idle();
        assert.equal(requests, 3, 'sent a request while backing off');

        // Past it: exactly one probe, which fails and extends the window again.
        clock.advance(1_000);
        feed.Report('9.9.9.10'); await feed.Idle();
        assert.equal(requests, 4, 'did not probe once the backoff elapsed');
    } finally {
        feed.Close();
        await new Promise<void>(resolve => server.close(() => resolve()));
    }
});

test('an answer clears the backoff, so a recovered helper is used again at once', async () => {
    let requests = 0;
    let status = 503;
    const server = http.createServer((_req, res) => { requests++; res.writeHead(status); res.end(); });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${(server.address() as any).port}`;
    const clock = Clock();
    const feed = new AllowlistFeed({url, secret: 'test-secret', refreshMs: 60_000, timeoutMs: 1000}, clock.now);

    try {
        for (let i = 0; i < 6; i++) { feed.Report(`8.8.8.${i}`); await feed.Idle(); }
        assert.equal(requests, 3);

        status = 200;
        clock.advance(2_500);
        feed.Report('9.9.9.9'); await feed.Idle();
        assert.equal(requests, 4, 'did not probe after the backoff elapsed');

        // The helper answered, so the next login goes straight through with no waiting.
        feed.Report('9.9.9.10'); await feed.Idle();
        assert.equal(requests, 5, 'still backing off after the helper recovered');
    } finally {
        feed.Close();
        await new Promise<void>(resolve => server.close(() => resolve()));
    }
});
