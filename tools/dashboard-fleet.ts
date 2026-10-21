// The fleet overview: one row per host from the main dashboard's sample and the workers' health
// samples, with totals over the rows. A reading older than 30 s (hunt counts: 150 s) is unknown.

// Backend and worker JSON, checked field by field below.
type Json = any;

export interface WorkerState {
    configured: boolean;
    online: boolean;
    sample: Json;
    [key: string]: unknown;
}

const finite = (value: unknown): number | null => (typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null);
const fresh = (at: unknown, limit: number, now: number) =>
    Number.isFinite(Date.parse(at as string)) && now - Date.parse(at as string) <= limit && Date.parse(at as string) <= now + 60000;
const count = (value: unknown): number | null => (Number.isInteger(value) && (value as number) >= 0 ? (value as number) : null);

export function validCapacity(sample: Json, now = Date.now()) {
    const value = sample?.capacity;
    if (!value || !fresh(sample.capacitySampleAt, 30000, now) || ["running", "pending", "limit"].some(key => count(value[key]) === null)) return null;
    const percent = value.cpu?.percent, limit = value.cpu?.limit, resume = value.cpu?.resume;
    const cpu = value.cpu && typeof value.cpu.blocked === "boolean" && [percent, limit, resume].every(n => finite(n) !== null && n <= 100)
        ? { blocked: value.cpu.blocked as boolean, percent: percent as number, limit: limit as number, resume: resume as number } : null;
    return { running: value.running as number, pending: value.pending as number, limit: value.limit as number, cpu };
}

export function fleetSummary(main: Json, worker: WorkerState | null, mainError: string | null = null, now = Date.now(), aus: WorkerState | null = null, germany: WorkerState | null = null, eu2: WorkerState | null = null) {
    const mainOnline = !!main && !mainError && fresh(main.at, 30000, now);
    const perf = main?.performance;
    const mainCapacity = mainOnline ? validCapacity(main, now) : null;
    const mainHunts = mainCapacity?.running ?? (mainOnline && perf && !perf.stale && fresh(perf.at, 150000, now) && Array.isArray(perf.processes)
        ? perf.processes.filter((p: Json) => p.role === "hunt" || p.role === "tutorial").length : null);
    const row = (name: string, online: boolean, sample: Json, hunts: number | null, huntSampleAt: string | null) => ({
        name, online, at: (sample?.at ?? null) as string | null, huntSampleAt,
        cpu: online ? finite(sample.cpu) : null, logicalCpus: (sample?.logicalCpus ?? null) as number | null,
        ramUsedMB: online ? finite(sample.ramUsedMB) : null, ramTotalMB: online ? finite(sample.ramTotalMB) : null, hunts: online ? hunts : null,
        capacity: online ? validCapacity(sample, now) : null, capacitySampleAt: (sample?.capacitySampleAt ?? null) as string | null,
        diskFreeGB: online ? finite(sample.diskFreeGB) : null, hostUptimeSeconds: online ? finite(sample.hostUptimeSeconds) : null,
    });
    const rows = [row("Server #1 · EU", mainOnline, main, mainHunts, mainCapacity ? main.capacitySampleAt : perf?.at ?? null)];
    for (const [name, node] of [["Server #2 · Key bot", worker], ["Server #3 · US (NA)", aus], ["Server #4 · Germany", germany], ["Server #5 · EU overflow", eu2]] as const) {
        if (!node?.configured) continue;
        const online = node.online && !!node.sample && fresh(node.sample.at, 30000, now);
        const capacity = online ? validCapacity(node.sample, now) : null;
        const hunts = capacity?.running ?? (node.sample?.services?.deploy !== false && Array.isArray(node.sample?.hunts)
            ? node.sample.hunts.filter((h: Json) => !["city", "dojo"].includes(h.kind)).length : name === "Server #2 · Key bot" ? 0 : null);
        rows.push(row(name, online, node.sample, hunts, capacity ? node.sample.capacitySampleAt : node.sample?.at ?? null));
    }
    type Row = (typeof rows)[number];
    const sum = (field: "cpu" | "ramUsedMB" | "ramTotalMB" | "hunts") =>
        rows.every((r: Row) => r[field] !== null) ? rows.reduce((n: number, r: Row) => n + (r[field] as number), 0) : null;
    const cpu = sum("cpu"), ramUsedMB = sum("ramUsedMB"), ramTotalMB = sum("ramTotalMB");
    return {
        rows, totals: {
            servers: rows.length, online: rows.filter(r => r.online).length,
            meanCpu: cpu === null ? null : cpu / rows.length, ramUsedMB, ramTotalMB,
            ramPercent: ramUsedMB !== null && ramTotalMB !== null && ramTotalMB > 0 ? ramUsedMB / ramTotalMB * 100 : null, hunts: sum("hunts"),
        },
    };
}
