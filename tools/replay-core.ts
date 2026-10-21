// The pure half of the shadow replay (MIGRATION_PLAN.md M35): reading a traffic corpus (M2), putting
// credentials back into its placeholders, and comparing two servers' answers with the values that
// legitimately differ (dates, generated ids, tokens) normalised away. replay.ts runs the servers.

export type Body = null | { json: unknown } | { form: [string, string][] } | { text: string } | { binary: number } | { omitted: number };
export type Headers = Record<string, string | string[]>;

export interface HttpRecord {
    kind: "http";
    t: string;
    src: "gateway" | "metagame";
    conn: number;
    seq: number;
    ip: string;
    method: string;
    target: string;
    route: string;
    reqHeaders: Headers;
    reqBody: Body;
    status: number;
    resHeaders: Headers;
    resBody: Body;
    ms: number;
    aborted?: boolean;
}

export interface ChatRecord {
    kind: "chat";
    t: string;
    src: "metagame";
    conn: number;
    event?: "open" | "close";
    dir?: "in" | "out";
    text?: string;
    ip?: string;
    via?: string;
    reason?: string;
}

export type CaptureRecord = HttpRecord | ChatRecord;

/// The corpus in time order: lines from any number of files, merged; lines that are not records skipped.
export function readCorpus(texts: string[]): CaptureRecord[] {
    const records: CaptureRecord[] = [];
    for (const text of texts) {
        for (const line of text.split("\n")) {
            if (line.trim() === "") continue;
            try {
                const record = JSON.parse(line);
                if (record?.kind === "http" || record?.kind === "chat") records.push(record);
            } catch { /* a cut last line */ }
        }
    }
    // Stable: records of one source keep their written order within the same millisecond
    return records.map((record, index) => ({ record, index })).sort((a, b) => a.record.t.localeCompare(b.record.t) || a.index - b.index).map(item => item.record);
}

export interface Credentials {
    /** A fresh token for the account */
    token(uid: string): string;
    /** The replay's own key for the account whose key has this hash, if the snapshot has it */
    key(hash: string): string | undefined;
    gsKey: string;
}

/** Placeholders the capture wrote (see legacy/UndauntedGateway/src/capture.ts). */
const PLACEHOLDER = /<(token|key|plain):([A-Za-z0-9_-]{1,64})>|<gskey>|<token>|<redacted>/g;

/** The text with every placeholder replaced; `exact` is false when one could not be (a cut secret). */
export function substitute(text: string, credentials: Credentials): { text: string; exact: boolean } {
    let exact = true;
    const out = text.replace(PLACEHOLDER, (whole: string, kind: string | undefined, value: string | undefined) => {
        if (kind === "token") return credentials.token(value!);
        if (kind === "plain") return Buffer.from(`\0${value}\0${credentials.token(value!)}`, "utf8").toString("base64");
        if (kind === "key") {
            const key = credentials.key(value!);
            if (key !== undefined) return key;
        }
        if (whole === "<gskey>") return credentials.gsKey;
        exact = false;
        return whole;
    });
    return { text: out, exact };
}

function substituteJson(value: unknown, credentials: Credentials, state: { exact: boolean }): unknown {
    if (typeof value === "string") {
        const result = substitute(value, credentials);
        if (!result.exact) state.exact = false;
        return result.text;
    }
    if (Array.isArray(value)) return value.map(item => substituteJson(item, credentials, state));
    if (value !== null && typeof value === "object") {
        return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, substituteJson(item, credentials, state)]));
    }
    return value;
}

export interface ReplayRequest {
    method: string;
    path: string;
    headers: Record<string, string>;
    body: string | undefined;
    /** False when the recording cut a secret the request needs: a difference is then expected */
    exact: boolean;
}

const DROPPED_HEADERS = new Set(["host", "content-length", "connection", "transfer-encoding", "keep-alive", "accept-encoding",
    "x-forwarded-for", "x-forwarded-proto", "x-forwarded-host", "x-forwarded-port", "forwarded", "x-real-ip", "x-dauntless-gateway", "expect"]);

/** The request to send again, or why it cannot be (content downloads, the gateway's own refusals, cut bodies). */
export function buildRequest(record: HttpRecord, credentials: Credentials, gatewaySecret: string | undefined): ReplayRequest | { skip: string } {
    if (record.route !== "metagame") return { skip: `route ${record.route}` };
    const body = record.reqBody;
    if (body !== null && ("binary" in body || "omitted" in body)) return { skip: "body not recorded" };
    const state = { exact: true };
    const headers: Record<string, string> = {};
    for (const [name, value] of Object.entries(record.reqHeaders)) {
        if (DROPPED_HEADERS.has(name)) continue;
        const one = Array.isArray(value) ? value.join(", ") : value;
        const result = substitute(one, credentials);
        if (!result.exact) state.exact = false;
        headers[name] = result.text;
    }
    // What the gateway adds when it forwards a request
    if (record.src === "gateway" && gatewaySecret !== undefined) {
        headers["x-dauntless-gateway"] = gatewaySecret;
        headers["x-forwarded-for"] = record.ip;
        headers["x-forwarded-proto"] = "https";
    }
    const target = substitute(record.target, credentials);
    if (!target.exact) state.exact = false;
    let text: string | undefined;
    if (body !== null && "json" in body) text = JSON.stringify(substituteJson(body.json, credentials, state));
    else if (body !== null && "form" in body) {
        const form = new URLSearchParams();
        for (const [name, value] of body.form) {
            const result = substitute(value, credentials);
            if (!result.exact) state.exact = false;
            form.append(name, result.text);
        }
        text = form.toString();
    } else if (body !== null && "text" in body) {
        const result = substitute(body.text, credentials);
        if (!result.exact) state.exact = false;
        text = result.text;
    }
    return { method: record.method, path: target.text, headers, body: text, exact: state.exact };
}

// ---- Generated ids ----

const ID = /^(?:UID-)?[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$|^[0-9a-fA-F]{32}$/;

/**
 * Learns the ids a server generated: where the recorded answer has an id and this server's answer has
 * another one at the same place, later requests to this server use its own id (a new character's id,
 * a party's, an invite's).
 */
export function learnIds(recorded: unknown, actual: unknown, ids: Map<string, string>): void {
    if (typeof recorded === "string" && typeof actual === "string") {
        if (recorded !== actual && ID.test(recorded) && ID.test(actual) && !ids.has(recorded)) ids.set(recorded, actual);
        return;
    }
    if (Array.isArray(recorded) && Array.isArray(actual)) {
        recorded.forEach((item, index) => learnIds(item, actual[index], ids));
        return;
    }
    if (recorded !== null && actual !== null && typeof recorded === "object" && typeof actual === "object" && !Array.isArray(recorded) && !Array.isArray(actual)) {
        for (const [key, item] of Object.entries(recorded)) learnIds(item, (actual as Record<string, unknown>)[key], ids);
    }
}

/** The request with the recorded ids replaced by this server's own. */
export function applyIds(request: ReplayRequest, ids: Map<string, string>): ReplayRequest {
    if (ids.size === 0) return request;
    const swap = (text: string) => {
        let out = text;
        for (const [recorded, actual] of ids) out = out.split(recorded).join(actual);
        return out;
    };
    return {
        ...request,
        path: swap(request.path),
        headers: Object.fromEntries(Object.entries(request.headers).map(([name, value]) => [name, swap(value)])),
        body: request.body === undefined ? undefined : swap(request.body),
    };
}

// ---- Comparing ----

const ISO_DATE = /\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})\b/g;
const UUID = /\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b/g;
const HEX32 = /\b[0-9a-fA-F]{32}\b/g;
const JWT = /eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g;
const ACCOUNT_KEY = /UUK_[0-9A-Za-z]+/g;

/** Maps values each server generates on its own (ids, keys) to their order of first appearance. */
export class Normalizer {
    private readonly seen = new Map<string, string>();
    private next = 1;

    private alias(kind: string, value: string): string {
        const key = `${kind}:${value.toLowerCase()}`;
        let alias = this.seen.get(key);
        if (alias === undefined) {
            alias = `<${kind}#${this.next++}>`;
            this.seen.set(key, alias);
        }
        return alias;
    }

    text(value: string): string {
        return value
            .replace(JWT, "<token>")
            .replace(ACCOUNT_KEY, match => this.alias("key", match))
            .replace(ISO_DATE, "<date>")
            .replace(UUID, match => this.alias("uuid", match))
            .replace(HEX32, match => this.alias("hex", match));
    }

    value(value: unknown): unknown {
        if (typeof value === "string") return this.text(value);
        // Epoch milliseconds within ten years of now are times, not counts
        if (typeof value === "number" && Number.isInteger(value) && Math.abs(value - Date.now()) < 10 * 365 * 86_400_000) return "<time>";
        if (Array.isArray(value)) return value.map(item => this.value(item));
        if (value !== null && typeof value === "object") {
            return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, this.value(item)]));
        }
        return value;
    }
}

/** The path of the first difference between two JSON values, or undefined when they are equal. */
export function firstDifference(a: unknown, b: unknown, path = "$"): string | undefined {
    if (a === b) return undefined;
    if (typeof a !== typeof b || a === null || b === null || typeof a !== "object") return path;
    if (Array.isArray(a) !== Array.isArray(b)) return path;
    if (Array.isArray(a)) {
        const other = b as unknown[];
        if (a.length !== other.length) return `${path}.length`;
        for (let i = 0; i < a.length; i++) {
            const difference = firstDifference(a[i], other[i], `${path}[${i}]`);
            if (difference !== undefined) return difference;
        }
        return undefined;
    }
    const left = a as Record<string, unknown>, right = b as Record<string, unknown>;
    const keys = [...new Set([...Object.keys(left), ...Object.keys(right)])];
    for (const key of keys) {
        if (!(key in left) || !(key in right)) return `${path}.${key}`;
        const difference = firstDifference(left[key], right[key], `${path}.${key}`);
        if (difference !== undefined) return difference;
    }
    // Same values, other key order: JSON text differs (MIGRATION_PLAN.md SB1)
    if (Object.keys(left).join("\n") !== Object.keys(right).join("\n")) return `${path} (key order)`;
    return undefined;
}

export interface Reply { status: number; body: string }

/** The status answers name the build that served them; two builds always differ there. */
function withoutBuild(value: unknown): unknown {
    if (value === null || typeof value !== "object" || Array.isArray(value) || !("version" in value) || !("commit" in value)) return value;
    const { version: _version, commit: _commit, ...rest } = value as Record<string, unknown>;
    return rest;
}

/** Where two answers differ once the values each server generates are normalised, or undefined. */
export function compareReplies(node: Reply, rust: Reply, normalizers: { node: Normalizer; rust: Normalizer }): string | undefined {
    if (node.status !== rust.status) return `status ${node.status} vs ${rust.status}`;
    let left: unknown, right: unknown, json = true;
    try {
        left = node.body === "" ? null : JSON.parse(node.body);
        right = rust.body === "" ? null : JSON.parse(rust.body);
    } catch { json = false; }
    if (json) {
        const difference = firstDifference(normalizers.node.value(withoutBuild(left)), normalizers.rust.value(withoutBuild(right)));
        return difference === undefined ? undefined : `body at ${difference}`;
    }
    return normalizers.node.text(node.body) === normalizers.rust.text(rust.body) ? undefined : "body text";
}

/** Chat frames as compared: server liveness pings and the replay's own barriers left out, values normalised. */
export function comparableFrames(frames: string[], normalizer: Normalizer): string[] {
    return frames
        .filter(frame => !/<ping xmlns="urn:xmpp:ping"\/>/.test(frame) && !/id="replay-barrier-\d+"/.test(frame))
        .map(frame => normalizer.text(frame.replace(/ id="[0-9a-f]{16}"/g, ' id="<id>"').replace(/srv-[0-9a-f]{32}/g, "srv-<hex>")));
}
