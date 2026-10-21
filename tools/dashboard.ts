// The owner dashboard: a loopback-only page with the fleet, backend health, accounts, invites,
// moderation and the service logs, behind the owner key. See tools/DASHBOARD.md.
//   bun --no-env-file --env-file=<dashboard.env> tools/dashboard.ts
import os from "node:os";
import { lstat, open, readFile } from "node:fs/promises";
import { createHash, timingSafeEqual } from "node:crypto";
import { readPerformance, type PerformanceSample } from "./dashboard-performance.ts";
import { fleetSummary } from "./dashboard-fleet.ts";
import { monitorWorker, type Fetcher } from "./dashboard-worker.ts";

// Backend JSON, checked field by field where it is used.
type Json = any;

export function scrubLog(text: string): string {
    return text.split(/\r?\n/).slice(-150).map(line => {
        // Omit whole credential-bearing lines, including structured request captures.
        if (/token|password|secret|authorization|api.?key|exchange_code|UUK_|eyJ|cookie/i.test(line)) return "[credential-bearing line omitted]";
        return line.replace(/\x1b\[[0-9;]*m/g, "").slice(0, 2000);
    }).join("\n");
}

export async function tailLog(path: string): Promise<string> {
    if ((await lstat(path)).isSymbolicLink()) throw new Error("Log links are not supported");
    const file = await open(path, "r");
    try {
        const stat = await file.stat();
        if (!stat.isFile()) throw new Error("Not a log file");
        const size = Math.min(stat.size, 32768);
        const buffer = Buffer.alloc(size);
        const { bytesRead } = await file.read(buffer, 0, size, stat.size - size);
        let text = buffer.subarray(0, bytesRead).toString("utf8");
        if (stat.size > size) text = text.slice(text.indexOf("\n") + 1);
        return scrubLog(text);
    } finally { await file.close(); }
}

const digest = (value: string) => createHash("sha256").update(value).digest();
const cpuTimes = () => os.cpus().reduce((sum, cpu) => {
    sum.idle += cpu.times.idle;
    sum.total += Object.values(cpu.times).reduce((a, b) => a + b, 0);
    return sum;
}, { idle: 0, total: 0 });

const CSP = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// A request body as text, or null once it is over `limit` bytes.
async function readLimited(request: Request, limit: number): Promise<string | null> {
    let text = "";
    if (!request.body) return text;
    for await (const chunk of request.body) {
        text += Buffer.from(chunk).toString("utf8");
        if (Buffer.byteLength(text) > limit) return null;
    }
    return text;
}

export interface Running {
    port: number;
    stop(): Promise<void>;
}

export interface ServerConfig {
    Mode: string;
    PublicHost: string;
    Ports: { gateway: number; deploy?: number };
    CertFingerprint: string;
    ServerName?: string;
}

export interface DashboardOptions {
    key: string;
    backend: string;
    port?: number;
    logs?: Record<string, string>;
    serverConfig?: ServerConfig | null;
    performanceDir?: string | null;
    workerUrl?: string | null;
    ausUrl?: string | null;
    germanyUrl?: string | null;
    eu2Url?: string | null;
    fetcher?: Fetcher;
}

export async function startDashboard({ key, backend, port = 61110, logs = {}, serverConfig = null, performanceDir = null, workerUrl = null, ausUrl = null, germanyUrl = null, eu2Url = null, fetcher = fetch }: DashboardOptions): Promise<Running> {
    if (typeof key !== "string" || key.length < 16) throw new Error("An owner key of at least 16 characters is required");
    const target = new URL(backend);
    if (target.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(target.hostname) || target.username || target.password) throw new Error("Backend must be loopback HTTP");
    const page = await readFile(new URL("./dashboard.html", import.meta.url));
    if (serverConfig && (serverConfig.Mode !== "Public" || !/^[a-z0-9.-]+$/i.test(serverConfig.PublicHost || "") || !Number.isInteger(serverConfig.Ports?.gateway) || serverConfig.Ports.gateway < 1 || serverConfig.Ports.gateway > 65535 || !/^[a-f0-9]{64}$/i.test(serverConfig.CertFingerprint || ""))) throw new Error("Invalid public server configuration");
    const [workerMonitor, ausMonitor, germanyMonitor, eu2Monitor] = await Promise.all([monitorWorker(workerUrl, "Server #2", fetcher), monitorWorker(ausUrl, "Server #3", fetcher), monitorWorker(germanyUrl, "Server #4", fetcher), monitorWorker(eu2Url, "Server #5", fetcher)]);
    const monitors = [workerMonitor, ausMonitor, germanyMonitor, eu2Monitor];

    let previous = cpuTimes(), known: Set<string> | null = null, added = 0, sample: Json = null, failure: string | null = null, polling = false;
    const history: Json[] = [];
    let accountCache: Json = null, accountsAt = 0;
    let performanceSample: PerformanceSample | null = null, performanceAt = 0;
    async function poll() {
        if (polling) return;
        polling = true;
        try {
            const started = performance.now();
            const get = async (route: string) => {
                const response = await fetcher(new URL(route, target), { headers: { "x-undaunted-user-api-key": key }, signal: AbortSignal.timeout(30000), redirect: "error" });
                if (!response.ok) throw new Error(response.status === 401 || response.status === 403 ? "Backend rejected the dashboard owner key" : "Backend HTTP " + response.status);
                return response.json();
            };
            const [accounts, status, allocation] = await Promise.all([
                accountCache && Date.now() - accountsAt < 60000 ? accountCache : get("/undaunted/api/GetAllUsers"), get("/undaunted/api/ServerStatus"),
                (async () => {
                    try {
                        const response = await fetcher(new URL(`http://127.0.0.1:${serverConfig?.Ports?.deploy || 61001}/gameservers`), { signal: AbortSignal.timeout(5000), redirect: "error" });
                        if (!response.ok) return null;
                        const data = await response.json();
                        return { capacity: data.capacity, capacitySampleAt: new Date().toISOString() };
                    } catch { return null; }
                })(),
            ]);
            let health: Json = null;
            let healthError: string | null = null;
            let discord: Json = null;
            try { discord = await get("/undaunted/api/DiscordKeyStats"); } catch { /* Older backends: report unavailable, not zero. */ }
            try { health = await get("/undaunted/api/BackendHealth"); if (!health?.requests) { health = null; healthError = "Backend returned no health metrics."; } }
            catch { healthError = "Backend health disabled or unavailable. Set BACKEND_HEALTH=1 and restart the metagame."; }
            if (performanceDir && Date.now() - performanceAt >= 15000) {
                performanceAt = Date.now();
                try { performanceSample = await readPerformance(performanceDir); } catch { performanceSample = null; }
            }
            if (!Array.isArray(accounts.Users) || !Array.isArray(status.players) || !Array.isArray(status.instances)) throw new Error("Unexpected backend response");
            if (accounts !== accountCache) { accountCache = accounts; accountsAt = Date.now(); }
            const ids = new Set<string>(accounts.Users.map((user: Json) => user.UserId));
            if (known) for (const id of ids) if (!known.has(id)) added++;
            known = ids;
            const current = cpuTimes(), elapsed = current.total - previous.total;
            const cpu = elapsed > 0 ? 100 * (1 - (current.idle - previous.idle) / elapsed) : null;
            previous = current;
            const locations: Record<string, number> = { city: 0, hunt: 0, dojo: 0, tutorial: 0, menu: 0, unknown: 0 };
            for (const player of status.players) locations[Object.hasOwn(locations, player.where) ? player.where : "unknown"]!++;
            const point = {
                at: new Date().toISOString(), cpu, ramUsedMB: (os.totalmem() - os.freemem()) / 1048576, ramTotalMB: os.totalmem() / 1048576, players: status.playersOnline, accounts: ids.size, newAccountsObserved: added, backendMs: performance.now() - started,
                requestsPerSecond: health?.requests?.requestsPerSecond ?? null, errorPercent: health?.requests?.serverErrorPercent ?? null, eventLoopP95Ms: health?.eventLoop?.p95Ms ?? null,
                backendRssMB: health?.memoryMB?.rss ?? null, backendHeapMB: health?.memoryMB?.heapUsed ?? null,
            };
            history.push(point);
            if (history.length > 720) history.shift();
            sample = { ...point, capacity: allocation?.capacity ?? null, capacitySampleAt: allocation?.capacitySampleAt ?? null, players: status.players, instances: status.instances, history, uptimeSeconds: status.uptimeSeconds, hostUptimeSeconds: os.uptime(), dashboardUptimeSeconds: process.uptime(), locations, health, healthError, discord, performance: performanceSample, logicalCpus: os.cpus().length, name: status.name };
            failure = null;
        } catch (error) {
            const e = error as Error;
            failure = (e.name === "TimeoutError" ? "Backend response timed out" : /^Backend (HTTP |rejected)/.test(e.message) ? e.message : "Backend unavailable (connection failed)") + "; last readings are stale.";
        } finally { polling = false; }
    }
    await poll();
    const timer = setInterval(poll, 5000);
    timer.unref();
    let requests = 0, windowStart = Date.now(), readingLog = false, inviting = false;
    let inviteCount = 0, inviteWindow = Date.now();
    let readingAccounts = false;

    async function handle(req: Request): Promise<Response> {
        const headers = new Headers({ "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff", "Content-Security-Policy": CSP });
        const reply = (status: number, body: BodyInit | null = null) => new Response(body, { status, headers });
        const host = req.headers.get("host") ?? "", origin = req.headers.get("origin");
        if (!/^(127\.0\.0\.1|localhost|\[::1\]):\d+$/.test(host) || origin && origin !== `http://${host}`) return reply(403);
        const url = new URL(req.url), path = url.pathname + url.search;
        const createInvite = req.method === "POST" && path === "/api/invites";
        const moderate = req.method === "POST" && path === "/api/moderation";
        if (req.method !== "GET" && !createInvite && !moderate) return reply(405);
        if (Date.now() - windowStart > 60000) { requests = 0; windowStart = Date.now(); }
        if (++requests > 300) return reply(429);
        if (path === "/") { headers.set("Content-Type", "text/html; charset=utf-8"); return reply(200, page); }
        if (path === "/dashboard.js") { headers.set("Content-Type", "text/javascript; charset=utf-8"); return reply(200, await readFile(new URL("./dashboard-client.js", import.meta.url))); }
        const supplied = req.headers.get("x-dashboard-key");
        if (typeof supplied !== "string" || !timingSafeEqual(digest(supplied), digest(key))) return reply(401);
        headers.set("Content-Type", "application/json");
        const backendGet = (route: string) => fetcher(new URL(route, target), { headers: { "x-undaunted-user-api-key": key }, signal: AbortSignal.timeout(30000), redirect: "error" });
        if (path.startsWith("/api/moderation-log?")) {
            const kind = url.searchParams.get("kind"), offset = Number(url.searchParams.get("offset") ?? 0);
            if (!["bans", "events"].includes(kind ?? "") || !Number.isSafeInteger(offset) || offset < 0 || offset > 1000000) return reply(400);
            try {
                const response = await backendGet(`/undaunted/api/ModerationLog?kind=${kind}&offset=${offset}`);
                return reply(response.status, JSON.stringify(await response.json()));
            } catch { return reply(502, JSON.stringify({ error: "Moderation history unavailable" })); }
        }
        if (moderate || path.startsWith("/api/moderation?")) {
            try {
                let body: Json;
                if (moderate) {
                    if (origin !== `http://${host}`) return reply(403);
                    if (req.headers.get("content-type") !== "application/json") return reply(415);
                    const text = await readLimited(req, 2048);
                    if (text === null) return reply(413);
                    body = JSON.parse(text);
                    // Resolve optional IP scope from trusted backend observations, never from request input.
                    body.address = null;
                    if (body.active === true) {
                        body.durationHours ??= 168;
                        if (body.includeIp === true) {
                            const observed = await backendGet(`/undaunted/api/Moderation/${encodeURIComponent(body.accountId)}`);
                            const info = await observed.json();
                            if (observed.status !== 200 || !info.addresses?.[0]?.address) return reply(409, JSON.stringify({ error: "No recently observed player IP. Use an account-only ban." }));
                            body.address = info.addresses[0].address;
                        }
                    }
                    delete body.includeIp;
                }
                const id = url.searchParams.get("accountId");
                if (!moderate && (!id || !/^UID-[A-Za-z0-9-]{1,100}$/.test(id))) return reply(400);
                const response = await fetcher(new URL(moderate ? "/undaunted/api/Moderation" : `/undaunted/api/Moderation/${encodeURIComponent(id!)}`, target), {
                    method: moderate ? "POST" : "GET", headers: { "x-undaunted-user-api-key": key, "content-type": "application/json" },
                    body: moderate ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(30000), redirect: "error",
                });
                return reply(response.status, JSON.stringify(await response.json()));
            } catch { return reply(502, JSON.stringify({ error: "Moderation request failed. Refresh status before retrying." })); }
        }
        if (path.startsWith("/api/account-key?")) {
            const id = url.searchParams.get("accountId");
            if (!id || !/^UID-[A-Za-z0-9-]{1,100}$/.test(id)) return reply(400);
            try {
                const response = await backendGet(`/undaunted/api/AccountRecovery/${encodeURIComponent(id)}`);
                return reply(response.status, JSON.stringify(await response.json()));
            } catch { return reply(503, JSON.stringify({ error: "Account recovery unavailable." })); }
        }
        if (path.startsWith("/api/accounts?")) {
            const query = (url.searchParams.get("q") || "").trim();
            if (query.length > 100) return reply(400);
            const offset = url.searchParams.get("offset");
            if (!/^\d{1,8}$/.test(offset || "") || Number(offset) > 10000000) return reply(400);
            if (readingAccounts) return reply(429);
            readingAccounts = true;
            try {
                const response = await backendGet(`/undaunted/api/DashboardAccounts?offset=${Number(offset)}&q=${encodeURIComponent(query)}`);
                if (!response.ok) throw new Error("unavailable");
                const result = await response.json();
                if (!Array.isArray(result.accounts)) throw new Error("invalid");
                // Explicit projection prevents future backend fields (especially credentials) leaking.
                return reply(200, JSON.stringify({
                    accounts: result.accounts.slice(0, 100).map((a: Json) => ({
                        id: String(a.id), name: String(a.name), admin: a.admin === true, developer: a.developer === true,
                        keyFingerprint: /^[a-f0-9]{16}$/.test(a.keyFingerprint || "") ? a.keyFingerprint : null,
                    })),
                    nextOffset: Number.isSafeInteger(result.nextOffset) && result.nextOffset > Number(offset) && result.nextOffset <= 10000000 ? result.nextOffset : null,
                }));
            } catch { return reply(503, JSON.stringify({ error: "Account directory unavailable. Update the metagame if this route is missing." })); }
            finally { readingAccounts = false; }
        }
        if (createInvite) {
            if (origin !== `http://${host}`) return reply(403);
            if (!serverConfig) return reply(503, JSON.stringify({ error: "Invite generation is not configured." }));
            if (Date.now() - inviteWindow > 60000) { inviteCount = 0; inviteWindow = Date.now(); }
            if (inviting || inviteCount >= 10) return reply(429);
            if (req.headers.get("content-type") !== "application/json") return reply(415);
            inviting = true;
            let submitted = false;
            try {
                const text = await readLimited(req, 2048);
                if (text === null) return reply(413);
                let body: Json;
                try { body = JSON.parse(text); } catch { return reply(400); }
                if (!body || !Number.isInteger(body.uses) || body.uses < 1 || body.uses > 100 || typeof body.name !== "string" || body.name.length > 100 || /[\x00-\x1f]/.test(body.name)) return reply(400);
                inviteCount++;
                submitted = true;
                const response = await fetcher(new URL("/undaunted/api/CreateInvite", target), { method: "POST", headers: { "x-undaunted-user-api-key": key, "content-type": "application/json" }, body: JSON.stringify({ uses: body.uses, name: body.name }), signal: AbortSignal.timeout(15000), redirect: "error" });
                if (!response.ok) return reply(response.status === 429 ? 429 : 502, JSON.stringify({ error: "Backend refused invite creation." }));
                const result = await response.json();
                if (!/^[A-Za-z0-9-]{4,64}$/.test(result.code || "")) throw new Error("Invalid invite response");
                const query = new URLSearchParams({ v: "2", mode: "public", host: serverConfig.PublicHost.toLowerCase(), port: String(serverConfig.Ports.gateway), fp: serverConfig.CertFingerprint.toLowerCase(), code: result.code, name: serverConfig.ServerName || "Dauntless Revived" });
                return reply(200, JSON.stringify({ invite: `dauntless-revived://join?${query}`, uses: body.uses }));
            } catch { return reply(502, JSON.stringify({ error: submitted ? "Invite result unknown. It may have been created; do not automatically retry." : "Invalid invite request." })); }
            finally { inviting = false; }
        }
        if (path === "/api/status") return reply(200, JSON.stringify({ sample, worker: workerMonitor.state, aus: ausMonitor.state, germany: germanyMonitor.state, eu2: eu2Monitor.state, fleet: fleetSummary(sample, workerMonitor.state, failure, Date.now(), ausMonitor.state, germanyMonitor.state, eu2Monitor.state), error: failure, logNames: Object.keys(logs), invitesEnabled: !!serverConfig }));
        if (path.startsWith("/api/log?")) {
            const name = url.searchParams.get("name");
            if (!name || !Object.hasOwn(logs, name)) return reply(404);
            if (readingLog) return reply(429);
            readingLog = true;
            try { return reply(200, JSON.stringify({ text: await tailLog(logs[name]!) })); }
            catch { return reply(503, JSON.stringify({ error: "Log unavailable" })); }
            finally { readingLog = false; }
        }
        return reply(404);
    }

    const closeAll = () => { clearInterval(timer); for (const m of monitors) m.close(); };
    let server: ReturnType<typeof Bun.serve>;
    try {
        // A backend answer may take its full 30 s; a connection is cut after 35 s without traffic
        server = Bun.serve({ hostname: "127.0.0.1", port, idleTimeout: 35, fetch: handle });
    } catch (error) { closeAll(); throw error; }
    return {
        port: server.port!,
        async stop() { closeAll(); await server.stop(true); },
    };
}

if (import.meta.main) {
    (async () => {
        const key = (await readFile(process.env.DASHBOARD_OWNER_KEY_FILE ?? "", "utf8")).trim();
        const logs: unknown = JSON.parse(process.env.DASHBOARD_LOG_CONFIG_FILE ? await readFile(process.env.DASHBOARD_LOG_CONFIG_FILE, "utf8") : process.env.DASHBOARD_LOG_FILES || "{}");
        if (!logs || Array.isArray(logs) || typeof logs !== "object" || Object.values(logs).some(path => typeof path !== "string")) throw new Error("DASHBOARD_LOG_FILES must map labels to file paths");
        const port = Number(process.env.DASHBOARD_PORT || 61110);
        if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Invalid dashboard port");
        const serverConfig = process.env.DASHBOARD_SERVER_CONFIG ? JSON.parse(await readFile(process.env.DASHBOARD_SERVER_CONFIG, "utf8")) : null;
        await startDashboard({ key, backend: process.env.DASHBOARD_BACKEND || "http://127.0.0.1:61000", port, logs: logs as Record<string, string>, serverConfig, performanceDir: process.env.DASHBOARD_PERFORMANCE_DIR || null, workerUrl: process.env.DASHBOARD_WORKER_URL || null, ausUrl: process.env.DASHBOARD_AUS_URL || null, germanyUrl: process.env.DASHBOARD_GERMANY_URL || null, eu2Url: process.env.DASHBOARD_EU2_URL || null });
        console.log(`Owner dashboard: http://127.0.0.1:${port} (use an SSH tunnel remotely)`);
    })().catch(() => { console.error("Dashboard startup failed. Check owner key file, settings and port."); process.exitCode = 1; });
}
