// A worker's health endpoint for the main dashboard (over a loopback SSH tunnel): host load, the
// kit's services, the deploy server's hunts and capacity, and the game processes from
// worker-processes.ps1. Player addresses, ids and secrets never leave the host.
//   bun --no-env-file --env-file=<worker.env> tools/worker-health.ts
import os from "node:os";
import { statfs } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import type { Fetcher } from "./dashboard-worker.ts";
import type { Running } from "./dashboard.ts";

// Service JSON, checked field by field where it is used.
type Json = any;

const cpuTimes = () => os.cpus().reduce((a, c) => ({ idle: a.idle + c.times.idle, total: a.total + Object.values(c.times).reduce((x, y) => x + y, 0) }), { idle: 0, total: 0 });

export interface ProcessScan {
    at: string;
    receivedBytes: number;
    sentBytes: number;
    processes: { pid: number; startedAt: string; cpuSeconds: number; [key: string]: unknown }[];
}

export function processSample(current: ProcessScan, previous: ProcessScan | undefined, cores: number) {
    const seconds = previous ? (Date.parse(current.at) - Date.parse(previous.at)) / 1000 : 0;
    return {
        at: current.at, processes: current.processes.map(p => {
            const old = previous?.processes.find(item => item.pid === p.pid && item.startedAt === p.startedAt);
            return { ...p, cpuPercent: old && seconds > 0 ? Math.max(0, Math.min(100, (p.cpuSeconds - old.cpuSeconds) / seconds / cores * 100)) : null, cpuSeconds: undefined };
        }), netInKbit: seconds > 0 ? Math.max(0, (current.receivedBytes - previous!.receivedBytes) * 8 / 1000 / seconds) : null,
        netOutKbit: seconds > 0 ? Math.max(0, (current.sentBytes - previous!.sentBytes) * 8 / 1000 / seconds) : null,
    };
}

/// worker-processes.ps1's JSON (Windows PowerShell, hidden, 20 s, 64 KB at most).
async function scanProcesses(): Promise<ProcessScan> {
    const script = fileURLToPath(new URL("./worker-processes.ps1", import.meta.url));
    const shell = `${process.env.SystemRoot || "C:/Windows"}/System32/WindowsPowerShell/v1.0/powershell.exe`;
    const child = Bun.spawn([shell, "-NoProfile", "-NonInteractive", "-File", script], { stdout: "pipe", stderr: "ignore", stdin: "ignore", windowsHide: true, timeout: 20000 });
    const stdout = await new Response(child.stdout).text();
    if ((await child.exited) !== 0) throw new Error("worker-processes.ps1 failed");
    if (Buffer.byteLength(stdout) > 65536) throw new Error("worker-processes.ps1 wrote too much");
    return JSON.parse(stdout.replace(/^\uFEFF/, ""));
}

export interface WorkerHealthOptions {
    port?: number;
    root?: string;
    allowlistSecret?: string;
    fetcher?: Fetcher;
    collect?: () => Promise<ProcessScan>;
}

export async function startWorkerHealth({ port = 61111, root = process.cwd(), allowlistSecret, fetcher = fetch, collect }: WorkerHealthOptions = {}): Promise<Running> {
    const gather = collect || scanProcesses;
    let prior = cpuTimes(), priorProcesses: ProcessScan | undefined, processInfo: ReturnType<typeof processSample> | null = null, sample: Json = null, busy = false, collecting = false;
    const eventLoopStart = performance.now();
    async function probe(url: string, headers?: Record<string, string>) {
        try { const r = await fetcher(url, { headers, redirect: "error", signal: AbortSignal.timeout(8000) }); if (!r.ok) return null; return await r.json(); } catch { return null; }
    }
    async function poll() {
        if (busy) return;
        busy = true;
        try {
            const [deploy, allowlist, backend, disk] = await Promise.all([
                probe("http://127.0.0.1:61001/gameservers"),
                probe("http://127.0.0.1:61005/status", { "x-allowlist-secret": allowlistSecret || "" }),
                probe("http://127.0.0.1:61000/dauntless-status"), statfs(root).catch(() => null)]);
            const now = cpuTimes(), elapsed = now.total - prior.total;
            sample = {
                at: new Date().toISOString(), cpu: elapsed > 0 ? 100 * (1 - (now.idle - prior.idle) / elapsed) : null,
                logicalCpus: os.cpus().length, ramUsedMB: (os.totalmem() - os.freemem()) / 1048576, ramTotalMB: os.totalmem() / 1048576,
                diskFreeGB: disk ? disk.bavail * disk.bsize / 1073741824 : null, hostUptimeSeconds: os.uptime(), monitorUptimeSeconds: (performance.now() - eventLoopStart) / 1000,
                services: { deploy: Array.isArray(deploy?.servers), backend: !!backend, allowlist: !!allowlist },
                capacity: Array.isArray(deploy?.servers) ? deploy.capacity ?? null : null,
                capacitySampleAt: Array.isArray(deploy?.servers) && deploy.capacity ? new Date().toISOString() : null,
                firewall: allowlist ? { enabled: allowlist.lastApply?.enabled ?? false, ok: allowlist.lastApply?.ok ?? null, addresses: allowlist.entries?.length ?? 0, pending: allowlist.pending, ports: allowlist.ports } : null,
                hunts: Array.isArray(deploy?.servers) ? deploy.servers.map((s: Json) => ({ id: s.id, port: s.port, kind: s.kind, map: s.map, huntId: s.huntId, expectedPlayers: s.expectedPlayers?.length ?? 0, startedAt: s.startedAt })) : [],
                processes: processInfo, monitorMemoryMB: process.memoryUsage().rss / 1048576,
            };
            prior = now;
        } finally { busy = false; }
    }
    async function scan() {
        if (collecting) return;
        collecting = true;
        try { const current = await gather(); processInfo = processSample(current, priorProcesses, os.cpus().length); priorProcesses = current; }
        catch { /* Keep the last timestamped sample; UI marks it stale. */ }
        finally { collecting = false; }
    }
    void scan();
    await poll();
    const timer = setInterval(() => void poll(), 5000), scanner = setInterval(() => void scan(), 60000);
    timer.unref(); scanner.unref();
    // At most 16 requests at a time, as the Node server's 16 connections allowed
    let inFlight = 0;
    const server = Bun.serve({
        hostname: "127.0.0.1", port, idleTimeout: 5,
        fetch(req, srv) {
            const headers = { "Cache-Control": "no-store" };
            const address = srv.requestIP(req)?.address;
            if (!["127.0.0.1", "::1", "::ffff:127.0.0.1"].includes(address ?? "") || req.headers.get("x-forwarded-for") || req.headers.get("forwarded")) return new Response(null, { status: 403, headers });
            if (req.method !== "GET" || new URL(req.url).pathname + new URL(req.url).search !== "/health") return new Response(null, { status: 404, headers });
            if (inFlight >= 16) return new Response(null, { status: 503, headers });
            inFlight++;
            try { return new Response(JSON.stringify(sample), { headers: { ...headers, "Content-Type": "application/json" } }); }
            finally { inFlight--; }
        },
    });
    return {
        port: server.port!,
        async stop() { clearInterval(timer); clearInterval(scanner); await server.stop(true); },
    };
}

if (import.meta.main) {
    startWorkerHealth({ root: process.env.WORKER_ROOT || process.cwd(), allowlistSecret: process.env.ALLOWLIST_SECRET })
        .then(() => console.log("Worker health ready"))
        .catch(() => { console.error("Worker health startup failed"); process.exitCode = 1; });
}
