// The Discord status worker: every 5 s it reads the shared backend, the dashboard's fleet and the
// local deploy server, and edits one webhook message with the realm status. See tools/DISCORD-STATUS.md.
//   bun --no-env-file --env-file=<worker.env> tools/discord-status.ts
import { appendFile, readFile, rename, stat, writeFile } from "node:fs/promises";
import type { Fetcher } from "./dashboard-worker.ts";

// Service JSON, checked field by field where it is used.
type Json = any;

export function webhookUrl(value: string): string {
    const u = new URL(value.trim());
    if (u.protocol !== "https:" || u.hostname !== "discord.com" || u.port || u.username || u.password || u.search || u.hash || !/^\/api\/webhooks\/\d+\/[A-Za-z0-9_-]+$/.test(u.pathname)) throw new Error("Invalid webhook configuration");
    return u.href;
}

export interface BackendSample {
    players: number;
    uptime: number;
    ms: number;
}

export interface Capacity {
    running: number;
    pending: number;
    limit: number | null;
    blocked: boolean | null;
    percent: number | null;
    cpuLimit: number | null;
    resume: number | null;
}

export interface FleetRow {
    id: number;
    at: number | null;
    online: boolean;
    huntAt: number | null;
    capacity: Capacity | null;
    cpu: number | null;
    ramUsedMB: number | null;
    ramTotalMB: number | null;
    hunts: number | null;
}

export interface Fleet {
    rows: FleetRow[];
    online: number;
    servers: number;
    knownHunts: number;
    hunts: number | null;
    meanCpu: number | null;
    ramUsedMB: number | null;
    ramTotalMB: number | null;
}

// A deliberate allowlist: never forward backend names, addresses, players or errors.
export function payload(sample: BackendSample | null, now = Date.now(), fleet: Fleet | null = null) {
    const at = Math.floor(now / 1000), online = sample !== null;
    const num = (v: number | null | undefined, suffix = "") => typeof v === "number" && Number.isFinite(v) && v >= 0 ? v.toFixed(1) + suffix : "Unknown";
    const fields = [
        { name: "👥 Slayers online", value: online ? String(sample.players) : "Unknown", inline: true },
        { name: "🌐 Fleet availability", value: fleet ? `${fleet.online} / ${fleet.servers} hosts reporting` : "Monitoring unavailable", inline: true },
        { name: "⚔️ Active hunts", value: Number.isInteger(fleet?.hunts) ? String(fleet!.hunts) : fleet ? `${fleet.knownHunts} reported · partial` : "Unknown", inline: true },
    ] as { name: string; value: string; inline?: boolean }[];
    const names: Record<number, string> = { 1: "EU", 2: "Key bot", 3: "US (NA)", 4: "Germany", 5: "EU overflow" };
    for (const r of fleet?.rows ?? []) {
        const name = `Server #${r.id} · ${names[r.id] ?? "Worker"}`;
        const age = Number.isFinite(r.at) ? `Host sample <t:${Math.floor(r.at! / 1000)}:R>` : "No host sample";
        const c = r.capacity;
        const admission = !c ? "Unknown" : c.blocked === true ? "Waiting for CPU headroom" : c.limit !== null && c.running + c.pending >= c.limit ? "Hunt limit reached" : c.blocked === false ? "Below CPU/count limits" : "CPU status unknown";
        const huntInfo = r.id === 2 ? "Key distribution · no hunt hosting" : `Hunts: ${Number.isInteger(r.hunts) ? r.hunts : "Unknown"} / ${c?.limit ?? "Unknown"} · starting ${c?.pending ?? "Unknown"}\nAdmission: ${admission}${c?.blocked && c.resume !== null && c.resume !== undefined ? ` (resume ≤${num(c.resume, "%")})` : ""}\nHunt sample: ${Number.isFinite(r.huntAt) ? `<t:${Math.floor(r.huntAt! / 1000)}:R>` : "Unknown"}`;
        const ramPercent = typeof r.ramUsedMB === "number" && Number.isFinite(r.ramUsedMB) && r.ramTotalMB !== null && r.ramTotalMB > 0 ? num(r.ramUsedMB / r.ramTotalMB * 100, "%") : "Unknown";
        fields.push({
            name, inline: true, value: r?.online
                ? `🟢 Host reporting\n${huntInfo}\nCPU: ${num(r.cpu, "%")}\nRAM: ${num(r.ramUsedMB === null ? null : r.ramUsedMB / 1024)} / ${num(r.ramTotalMB === null ? null : r.ramTotalMB / 1024)} GB (${ramPercent})\n${age}${r.hunts === null && r.id !== 2 ? "\nHunt telemetry unavailable" : ""}`
                : `🟠 Monitoring unavailable\n${age}\nGame availability unknown`,
        });
    }
    if (fleet) {
        fields.push({ name: "📊 Reporting hosts combined", value: `Mean CPU: ${num(fleet.meanCpu, "%")}\nRAM: ${num(fleet.ramUsedMB === null ? null : fleet.ramUsedMB / 1024)} / ${num(fleet.ramTotalMB === null ? null : fleet.ramTotalMB / 1024)} GB\n${fleet.online}/${fleet.servers} hosts reporting; unavailable hosts excluded.\nHunts are server instances, not players. CPU/count headroom does not guarantee a launch; memory and readiness also apply.` });
    }
    if (online) {
        const seconds = Math.floor(sample.uptime);
        fields.push({ name: "Shared backend", value: `🟢 Online · ${Math.round(sample.ms)} ms local check\nUptime: ${Math.floor(seconds / 86400)}d ${Math.floor(seconds % 86400 / 3600)}h ${Math.floor(seconds % 3600 / 60)}m\nStarted <t:${at - seconds}:R>` });
    } else {
        fields.push({ name: "Shared backend", value: "🟠 Check unavailable · player count and uptime unknown" });
    }
    fields.push({ name: "Region guide", value: "Choose EU, Germany or US (NA) in launcher Settings. EU has an additional overflow worker. Parties follow their leader across regions. CPU/count headroom is not a queue-time estimate; memory and server readiness also affect launches." });
    fields.push({ name: "📥 Launcher · Cloudflare downloads", value: "[Latest launcher / release notes](https://github.com/mixutin/dauntless-revived/releases/latest). Game downloads use Cloudflare with resume support and file verification." });
    fields.push({ name: "Last checked", value: `<t:${at}:F> (<t:${at}:R>)` });
    return {
        allowed_mentions: { parse: [] }, embeds: [{
            title: "Dauntless Revived · Live realm status",
            description: online ? "Shared accounts and progression across **EU, Germany and US (NA)**." : "The shared backend check failed. Please check again shortly.",
            color: online && fleet && fleet.servers > 0 && fleet.online === fleet.servers ? 0x35bc84 : 0xe1a349, fields,
            footer: { text: "Host health and backend activity, not player ping or proof of hunt completion. Stale readings are unknown." },
        }],
    };
}

export async function sampleBackend(backend: string, key: string, fetcher: Fetcher = fetch): Promise<BackendSample | null> {
    const url = new URL(backend);
    if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || url.username || url.password) throw new Error("Backend must use loopback HTTP");
    const start = performance.now();
    try {
        const res = await fetcher(new URL("/undaunted/api/ServerStatus", url), { headers: { "x-undaunted-user-api-key": key }, signal: AbortSignal.timeout(8000), redirect: "error" });
        if (!res.ok) return null;
        const data = await res.json();
        if (data.online !== true || data.limited !== false || !Number.isInteger(data.playersOnline) || data.playersOnline < 0 || !Number.isFinite(data.uptimeSeconds) || data.uptimeSeconds < 0) return null;
        return { players: data.playersOnline, uptime: data.uptimeSeconds, ms: performance.now() - start };
    } catch { return null; }
}

export interface PublisherState {
    id?: string;
    creating?: boolean;
    lastSuccess?: string;
}

export class Publisher {
    url: string;
    state: PublisherState;
    save: (state: PublisherState) => Promise<void>;
    fetcher: Fetcher;
    next = 0;
    failures = 0;
    disabled = false;

    constructor(url: string, state: PublisherState, save: (state: PublisherState) => Promise<void>, fetcher: Fetcher = fetch) {
        this.url = webhookUrl(url); this.state = state; this.save = save; this.fetcher = fetcher;
        if (state.id !== undefined && !/^\d+$/.test(state.id)) throw new Error("Invalid message ID");
        // An interrupted initial POST may have reached Discord. Never blindly duplicate it.
        if (state.creating && !state.id) this.disabled = true;
    }

    async send(body: unknown, now = Date.now()) {
        if (this.disabled || now < this.next) return;
        this.next = now + 15000;
        const editing = Boolean(this.state.id);
        if (!editing) { this.state.creating = true; await this.save(this.state); }
        let res: Json;
        try {
            res = await this.fetcher(editing ? `${this.url}/messages/${this.state.id}` : `${this.url}?wait=true`, {
                method: editing ? "PATCH" : "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
                signal: AbortSignal.timeout(8000), redirect: "error",
            });
        } catch {
            if (!editing) this.disabled = true;
            this.next = now + Math.min(300000, 15000 * 2 ** Math.min(++this.failures, 5));
            console.error("Discord status delivery failed; backing off. Initial delivery uncertainty requires operator review.");
            return;
        }
        if (res.status === 429) {
            const data = await res.json().catch(() => ({}));
            const retry = Math.max(Number(data.retry_after) || 0, Number(res.headers.get("retry-after")) || 0, Number(res.headers.get("x-ratelimit-reset-after")) || 0, 15);
            this.next = now + Math.ceil(retry * 1000) + 1000;
            if (!editing) { delete this.state.creating; await this.save(this.state); }
            return;
        }
        if (!res.ok) {
            if (!editing) this.disabled = true;
            this.next = now + Math.min(300000, 15000 * 2 ** Math.min(++this.failures, 5));
            await res.body?.cancel();
            console.error(`Discord status rejected with HTTP ${res.status}; retaining message ID and backing off.`);
            return;
        }
        if (!editing) {
            const data = await res.json().catch(() => ({}));
            if (!/^\d+$/.test(data.id ?? "")) { this.disabled = true; return; }
            this.state = { id: data.id }; await this.save(this.state);
        } else { await res.body?.cancel(); }
        this.failures = 0;
        this.state.lastSuccess = new Date(now).toISOString();
        await this.save(this.state);
        if (res.headers.get("x-ratelimit-remaining") === "0") this.next = Math.max(this.next, now + (Number(res.headers.get("x-ratelimit-reset-after")) || 15) * 1000 + 1000);
    }
}

export async function sampleLocalCapacity(deploy: string, fetcher: Fetcher = fetch): Promise<Capacity | null> {
    const url = new URL(deploy);
    if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || url.username || url.password) throw new Error("Deploy must use loopback HTTP");
    try {
        const response = await fetcher(new URL("/gameservers", url), { signal: AbortSignal.timeout(8000), redirect: "error" });
        if (!response.ok) return null;
        const { capacity: c } = await response.json();
        return publicCapacity(c);
    } catch { return null; }
}

function publicCapacity(c: Json): Capacity | null {
    if (!c || ![c.running, c.pending].every(n => Number.isSafeInteger(n) && n >= 0) || !(c.limit === null || Number.isSafeInteger(c.limit) && c.limit >= 0)) return null;
    const n = (v: unknown) => typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : null;
    return { running: c.running, pending: c.pending, limit: c.limit, blocked: typeof c.cpu?.blocked === "boolean" ? c.cpu.blocked : null, percent: n(c.cpu?.percent), cpuLimit: n(c.cpu?.limit), resume: n(c.cpu?.resume) };
}

export function applyLocalCapacity(fleet: Fleet | null, capacity: Capacity | null, now = Date.now()) {
    const main = fleet?.rows.find(r => r.id === 1);
    if (!fleet || !main?.online || !capacity) return;
    main.hunts = capacity.running; main.capacity = capacity; main.huntAt = now;
    fleet.knownHunts = fleet.rows.filter(r => r.online).reduce((n, r) => n + (r.hunts ?? 0), 0);
    fleet.hunts = fleet.rows.every(r => r.online && r.hunts !== null) ? fleet.knownHunts : null;
}

// Project only numeric fleet telemetry; no addresses, player names, keys or raw errors leave the host.
export async function sampleFleet(dashboard: string, key: string, fetcher: Fetcher = fetch, now = Date.now()): Promise<Fleet | null> {
    const url = new URL(dashboard);
    if (url.protocol !== "http:" || url.hostname !== "127.0.0.1" || url.username || url.password) throw new Error("Dashboard must use loopback HTTP");
    try {
        const r = await fetcher(new URL("/api/status", url), { headers: { "x-dashboard-key": key }, signal: AbortSignal.timeout(8000), redirect: "error" });
        if (!r.ok) return null;
        const data = await r.json(), f = data.fleet;
        if (!Array.isArray(f?.rows) || f.rows.length < 1 || f.rows.length > 16) return null;
        const n = (v: unknown) => typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : null;
        const rows: FleetRow[] = f.rows.map((row: Json, i: number) => {
            const id = Number(/^Server #(\d+)(?:\D|$)/.exec(row.name ?? "")?.[1] ?? i + 1);
            const at = Date.parse(row.at), huntAt = Date.parse(row.huntSampleAt ?? row.at);
            const fresh = (t: number) => Number.isFinite(t) && now - t <= 30000 && t <= now + 60000;
            const online = row.online === true && fresh(at);
            const capacityAt = Date.parse(row.capacitySampleAt ?? row.huntSampleAt ?? row.at);
            return {
                id, at: Number.isFinite(at) ? at : null, online, huntAt: Number.isFinite(huntAt) ? huntAt : null,
                capacity: online && fresh(capacityAt) ? publicCapacity(row.capacity) : null,
                cpu: online ? n(row.cpu) : null, ramUsedMB: online ? n(row.ramUsedMB) : null, ramTotalMB: online ? n(row.ramTotalMB) : null,
                hunts: online && Number.isFinite(huntAt) && now - huntAt <= 150000 && huntAt <= now + 60000 && Number.isInteger(row.hunts) && row.hunts >= 0 ? row.hunts : null,
            };
        });
        const reporting = rows.filter(r => r.online), cpus = reporting.filter(r => r.cpu !== null);
        return {
            rows, online: reporting.length, servers: rows.length,
            knownHunts: reporting.reduce((v, r) => v + (r.hunts ?? 0), 0),
            hunts: rows.every(r => r.online && r.hunts !== null) ? rows.reduce((v, r) => v + r.hunts!, 0) : null,
            meanCpu: cpus.length ? cpus.reduce((v, r) => v + r.cpu!, 0) / cpus.length : null,
            ramUsedMB: reporting.length && reporting.every(r => r.ramUsedMB !== null) ? reporting.reduce((v, r) => v + r.ramUsedMB!, 0) : null,
            ramTotalMB: reporting.length && reporting.every(r => r.ramTotalMB !== null) ? reporting.reduce((v, r) => v + r.ramTotalMB!, 0) : null,
        };
    } catch { return null; }
}

let logChain = Promise.resolve();

async function main() {
    if (process.env.STATUS_ERROR_FILE) {
        // Only static messages from this module; never exception objects or HTTP bodies.
        console.error = (message: string) => {
            logChain = logChain.then(async () => {
                const path = process.env.STATUS_ERROR_FILE!;
                if (((await stat(path).catch(() => null))?.size ?? 0) > 262144) await writeFile(path, "");
                await appendFile(path, `${new Date().toISOString()} ${message}\n`, { mode: 0o600 });
            }).catch(() => {});
        };
    }
    const key = (await readFile(process.env.STATUS_OWNER_KEY_FILE ?? "", "utf8")).trim();
    const webhook = await readFile(process.env.STATUS_WEBHOOK_FILE ?? "", "utf8");
    const stateFile = process.env.STATUS_STATE_FILE ?? "";
    let state: PublisherState;
    try { state = JSON.parse(await readFile(stateFile, "utf8")); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; state = {}; }
    const save = async (value: PublisherState) => { await writeFile(`${stateFile}.tmp`, JSON.stringify(value), { mode: 0o600 }); await rename(`${stateFile}.tmp`, stateFile); };
    const publisher = new Publisher(webhook, state, save);
    if (publisher.disabled) console.error("Status delivery paused: review persisted initial-message state.");
    while (true) {
        try {
            const [sample, fleet, capacity] = await Promise.all([
                sampleBackend(process.env.STATUS_BACKEND || "http://127.0.0.1:61000", key),
                sampleFleet(process.env.STATUS_DASHBOARD || "http://127.0.0.1:61110", key),
                sampleLocalCapacity(process.env.STATUS_DEPLOY || "http://127.0.0.1:61001"),
            ]);
            applyLocalCapacity(fleet, capacity);
            await publisher.send(payload(sample, Date.now(), fleet));
        } catch { console.error("Status cycle failed; retrying without creating another message."); }
        await new Promise(resolve => setTimeout(resolve, 5000));
    }
}

if (import.meta.main) {
    main().catch(() => { console.error("Status worker stopped: check private configuration and file permissions."); process.exitCode = 1; });
}
