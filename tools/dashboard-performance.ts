import { lstat, open, type FileHandle } from "node:fs/promises";
import { join } from "node:path";

const roles = new Set(["host", "metagame", "content", "gateway", "deploy", "allowlist", "ramsgate", "dojo", "hunt", "tutorial", "unknown"]);
const columns = "timestamp_utc,role,pid,udp_port,started_utc,players,cpu_core_percent,working_set_mb,private_mb,host_cpu_percent,logical_cpus,ram_total_mb,ram_free_mb,disk_free_gb,net_in_kbit_s,net_out_kbit_s,game_servers,players_online".split(",");
const numeric = (value: string) => value !== "" && Number.isFinite(Number(value)) && Number(value) >= 0 ? Number(value) : null;

export type PerformanceRow = Record<string, string | number | null>;
export interface PerformanceSample {
    at: string;
    host: PerformanceRow;
    processes: PerformanceRow[];
    stale: boolean;
}

// This is deliberately the kit's numeric-only CSV format, not a general CSV/log parser.
// Keep one complete cohort, with no command lines, paths, player names or arbitrary fields.
export function parsePerformance(text: string, now = Date.now()): PerformanceSample | null {
    const lines = text.slice(0, text.lastIndexOf("\n") + 1).split(/\r?\n/);
    let cohort: Omit<PerformanceSample, "stale"> | null = null;
    for (const line of lines) {
        const cells = line.split(",");
        if (cells.length !== columns.length || !roles.has(cells[1]!) || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/.test(cells[0]!) || !Number.isFinite(Date.parse(cells[0]!))) continue;
        const row: PerformanceRow = Object.fromEntries(columns.map((name, i) => [name, i === 0 || i === 1 || i === 4 ? cells[i]! : numeric(cells[i]!)]));
        if (row.role === "host") cohort = { at: row.timestamp_utc as string, host: row, processes: [] };
        else if (cohort?.at === row.timestamp_utc && cohort.processes.length < 100) {
            const cores = cohort.host.logical_cpus as number | null;
            cohort.processes.push({ ...row, cpu_host_percent: row.cpu_core_percent !== null && cores !== null && cores > 0 ? (row.cpu_core_percent as number) / cores : null });
        }
    }
    return cohort ? { ...cohort, stale: now - Date.parse(cohort.at) > 150000 || Date.parse(cohort.at) > now + 60000 } : null;
}

export async function readPerformance(directory: string, now = Date.now()): Promise<PerformanceSample | null> {
    if ((await lstat(directory)).isSymbolicLink()) throw new Error("Performance links are not supported");
    for (const time of [now, now - 86400000]) {
        const file = join(directory, `performance-${new Date(time).toISOString().slice(0, 10)}.csv`);
        let handle: FileHandle | undefined;
        try {
            if ((await lstat(file)).isSymbolicLink()) continue;
            handle = await open(file, "r");
            const stat = await handle.stat();
            if (!stat.isFile()) continue;
            const length = Math.min(stat.size, 131072), buffer = Buffer.alloc(length);
            const { bytesRead } = await handle.read(buffer, 0, length, stat.size - length);
            let text = buffer.subarray(0, bytesRead).toString("utf8");
            if (stat.size > length) text = text.slice(text.indexOf("\n") + 1);
            const sample = parsePerformance(text, now);
            if (sample) return sample;
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
        } finally {
            await handle?.close();
        }
    }
    return null;
}
