import type { Request, Response, NextFunction } from 'express';
import { monitorEventLoopDelay, performance } from 'node:perf_hooks';

const bounds = [1, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000];
type Bucket = { second: number, completed: number, clientErrors: number, serverErrors: number, aborted: number, histogram: number[] };

export class RequestMetrics {
    private buckets = new Map<number, Bucket>();
    record(status: number, ms: number, aborted = false, now = Date.now()) {
        const second = Math.floor(now / 1000);
        this.prune(second);
        let bucket = this.buckets.get(second);
        if (!bucket) {
            bucket = { second, completed: 0, clientErrors: 0, serverErrors: 0, aborted: 0, histogram: Array(bounds.length + 1).fill(0) };
            this.buckets.set(second, bucket);
        }
        if (aborted) { bucket.aborted++; return; }
        bucket.completed++;
        if (status >= 500) bucket.serverErrors++;
        else if (status >= 400) bucket.clientErrors++;
        const index = bounds.findIndex(bound => ms <= bound);
        bucket.histogram[index < 0 ? bounds.length : index]++;
    }
    private prune(second: number) {
        for (const key of this.buckets.keys()) if (key <= second - 60) this.buckets.delete(key);
    }
    snapshot(now = Date.now()) {
        this.prune(Math.floor(now / 1000));
        const result = { windowSeconds: 60, completed: 0, clientErrors: 0, serverErrors: 0, aborted: 0 };
        const histogram = Array(bounds.length + 1).fill(0);
        for (const bucket of this.buckets.values()) {
            result.completed += bucket.completed; result.clientErrors += bucket.clientErrors;
            result.serverErrors += bucket.serverErrors; result.aborted += bucket.aborted;
            bucket.histogram.forEach((value, i) => histogram[i] += value);
        }
        const percentile = (p: number) => {
            if (!result.completed) return null;
            const target = Math.ceil(result.completed * p);
            let count = 0;
            for (let i = 0; i < histogram.length; i++) {
                count += histogram[i];
                if (count >= target) return i === bounds.length ? '>5000' : bounds[i];
            }
            return null;
        };
        return { ...result, requestsPerSecond: result.completed / 60, serverErrorPercent: result.completed ? 100 * result.serverErrors / result.completed : 0,
            latencyP50UpperMs: percentile(0.5), latencyP95UpperMs: percentile(0.95) };
    }
}

export const requestMetrics = new RequestMetrics();
let delay: ReturnType<typeof monitorEventLoopDelay> | undefined;
export function BackendHealthEnabled() { return process.env.BACKEND_HEALTH === '1'; }
export function TrackBackendHealth(req: Request, res: Response, next: NextFunction) {
    if (!BackendHealthEnabled()) { next(); return; }
    if (!delay) { delay = monitorEventLoopDelay({ resolution: 20 }); delay.enable(); }
    // Monitoring polls must not make an idle server appear busy. Never store URLs or identifiers.
    if (/^\/undaunted\/api\/(BackendHealth|ServerStatus|GetAllUsers|DashboardAccounts|DiscordKeyStats)\/?$/i.test(req.path)) { next(); return; }
    const started = performance.now();
    let recorded = false;
    const finish = (aborted: boolean) => {
        if (recorded) return;
        recorded = true;
        requestMetrics.record(res.statusCode, performance.now() - started, aborted);
    };
    res.once('finish', () => finish(false));
    res.once('close', () => finish(!res.writableFinished));
    next();
}

export function BackendRuntimeHealth() {
    return {
        uptimeSeconds: Math.floor(process.uptime()),
        memoryMB: { rss: process.memoryUsage().rss / 1048576, heapUsed: process.memoryUsage().heapUsed / 1048576 },
        requests: requestMetrics.snapshot(),
        eventLoop: delay && delay.count > 0 ? { p50Ms: delay.percentile(50) / 1e6, p95Ms: delay.percentile(95) / 1e6, maxMs: delay.max / 1e6 } : null,
        bans: { supported: true, count: null }
    };
}
