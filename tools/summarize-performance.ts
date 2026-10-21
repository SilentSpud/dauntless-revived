// Summarizes the kit's performance CSVs: per role, the sample count, median, p95 and maximum of each metric.
//   bun tools/summarize-performance.ts <performance.csv> [more.csv ...]
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const metrics = ["cpu_core_percent", "working_set_mb", "players", "host_cpu_percent", "ram_free_mb", "players_online"];
const roles = new Set(["host", "metagame", "content", "gateway", "deploy", "allowlist", "ramsgate", "dojo", "hunt", "tutorial", "unknown"]);

export type Sample = Record<string, string | number | null> & { timestamp_utc: string; role: string };

// The kit writes unquoted, comma-separated numeric fields with a fixed header.
export function parseSamples(csv: string): Sample[] {
    const lines = csv.replace(/^﻿/, "").split(/\r?\n/).filter(line => line.trim());
    if (!lines.length) throw new Error("Empty performance CSV");
    const header = lines.shift()!.split(",");
    for (const name of ["timestamp_utc", "role", ...metrics]) {
        if (!header.includes(name)) throw new Error(`Missing column: ${name}`);
    }
    if (new Set(header).size !== header.length) throw new Error("Duplicate CSV column");
    return lines.map((line, index) => {
        const fields = line.split(",");
        if (fields.length !== header.length) throw new Error(`Invalid CSV row ${index + 2}`);
        const row: Record<string, string | number | null> = Object.fromEntries(header.map((name, i) => [name, fields[i]!]));
        if (!Number.isFinite(Date.parse(row.timestamp_utc as string)) || !roles.has(row.role as string)) {
            throw new Error(`Invalid timestamp or role at row ${index + 2}`);
        }
        for (const name of metrics) {
            const value = (row[name] as string).trim();
            row[name] = value === "" ? null : Number(value);
            if (value !== "" && (!Number.isFinite(row[name]) || (row[name] as number) < 0)) {
                throw new Error(`Invalid ${name} at row ${index + 2}`);
            }
        }
        return row as Sample;
    });
}

export function statistics(values: (number | null)[]) {
    const sorted = values.filter((value): value is number => value !== null).sort((a, b) => a - b);
    if (!sorted.length) return { samples: 0, median: null, p95: null, max: null };
    const quantile = (p: number) => sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)]!;
    return { samples: sorted.length, median: quantile(0.5), p95: quantile(0.95), max: sorted.at(-1)! };
}

export function summarize(rows: Sample[]) {
    if (!rows.length) throw new Error("No performance samples");
    let first = Infinity;
    let last = -Infinity;
    for (const row of rows) {
        const time = Date.parse(row.timestamp_utc);
        first = Math.min(first, time);
        last = Math.max(last, time);
    }
    const groups: Record<string, Record<string, ReturnType<typeof statistics>>> = {};
    for (const role of roles) {
        const selected = rows.filter(row => row.role === role);
        if (!selected.length) continue;
        groups[role] = Object.fromEntries(metrics.map(name => [name, statistics(selected.map(row => row[name] as number | null))]));
    }
    return {
        from: new Date(first).toISOString(),
        to: new Date(last).toISOString(),
        rows: rows.length,
        groups,
        notes: [
            "Percentiles use nearest rank and weight each process sample equally; missing readings are excluded.",
            "Process CPU is percent of one core; host CPU is percent of the whole machine.",
            "Hunt memory and CPU describe one sampled hunt process, not the sum of simultaneous hunts.",
            "Ramsgate includes fixed world overhead: dividing by players does not measure the cost of an additional player.",
            "These observations do not establish a safe player capacity. Compare sessions at different player counts first.",
        ],
    };
}

async function main(paths: string[]) {
    if (!paths.length) throw new Error("Usage: bun tools/summarize-performance.ts <performance.csv> [more.csv ...]");
    const rows: Sample[] = [];
    const seen = new Set<string>();
    for (const path of paths) {
        const absolute = resolve(path);
        if (seen.has(absolute)) throw new Error("The same input file was supplied twice");
        seen.add(absolute);
        for (const row of parseSamples(await readFile(absolute, "utf8"))) rows.push(row);
    }
    process.stdout.write(JSON.stringify(summarize(rows), null, 2) + "\n");
}

if (import.meta.main) {
    main(process.argv.slice(2)).catch((error: Error) => {
        process.stderr.write(error.message + "\n");
        process.exitCode = 1;
    });
}
