// The shadow replay (MIGRATION_PLAN.md M35): a traffic corpus recorded on the Node services (M2) sent
// again to the Node metagame and to the Rust one, each on its own copy of the same database snapshot,
// and their answers compared. Chat sessions are replayed stanza by stanza and their output compared too.
//
//   bun --no-env-file tools/replay.ts --corpus <dir or .jsonl> [--corpus ...] --db <snapshot.db>
//       --env <metagame.env> --node <legacy/UndauntedMetagame, built> --rust <metagame binary>
//       [--out <report.json>] [--keep-matchmaking] [--keep-work]
//
// The snapshot is never changed: the replay works on copies, and in them gives every account key the
// corpus used a key of its own (the corpus holds only the key's hash) and registers its own
// game-server key. Tokens are signed fresh with the settings' signing key. Matchmaking is switched off
// (MATCHMAKING_MODE=DISABLED) unless --keep-matchmaking, so no game server is started.
// Exit code: 0 when no exact request differs, 1 when one does, 2 on a usage or start error.
import { Database } from "bun:sqlite";
import crypto from "node:crypto";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import {
    applyIds, buildRequest, comparableFrames, compareReplies, type ChatRecord, type Credentials, type HttpRecord, learnIds, Normalizer, readCorpus, substitute
} from "./replay-core.ts";

type Args = { corpus: string[]; db: string; env: string; node: string; rust: string; out?: string; keepMatchmaking: boolean; keepWork: boolean };

function usage(message?: string): never {
    if (message) console.error(`replay: ${message}`);
    console.error("usage: bun tools/replay.ts --corpus <dir|file> --db <snapshot.db> --env <metagame.env> --node <legacy/UndauntedMetagame> --rust <metagame binary> [--out <report.json>] [--keep-matchmaking] [--keep-work]");
    process.exit(2);
}

function parseArgs(argv: string[]): Args {
    const args: Partial<Args> & { corpus: string[] } = { corpus: [], keepMatchmaking: false, keepWork: false };
    for (let i = 0; i < argv.length; i++) {
        const value = () => argv[++i] ?? usage(`${argv[i - 1]} needs a value`);
        switch (argv[i]) {
            case "--corpus": args.corpus.push(value()); break;
            case "--db": args.db = value(); break;
            case "--env": args.env = value(); break;
            case "--node": args.node = value(); break;
            case "--rust": args.rust = value(); break;
            case "--out": args.out = value(); break;
            case "--keep-matchmaking": args.keepMatchmaking = true; break;
            case "--keep-work": args.keepWork = true; break;
            default: usage(`unknown argument ${argv[i]}`);
        }
    }
    if (args.corpus.length === 0 || !args.db || !args.env || !args.node || !args.rust) usage();
    return args as Args;
}

/// KEY=value lines (quotes removed), as the services read them
function readEnv(file: string): Map<string, string> {
    const env = new Map<string, string>();
    for (const raw of fs.readFileSync(file, "utf8").split(/\r?\n/)) {
        const match = /^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/.exec(raw);
        if (!match || raw.trimStart().startsWith("#")) continue;
        let value = match[2];
        if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) value = value.slice(1, -1);
        env.set(match[1], value);
    }
    return env;
}

function writeEnv(file: string, env: Map<string, string>) {
    fs.writeFileSync(file, [...env].map(([name, value]) => `${name}='${value.replace(/'/g, "")}'`).join("\n") + "\n", { mode: 0o600 });
}

function freePort(): Promise<number> {
    return new Promise((resolve, reject) => {
        const server = net.createServer();
        server.once("error", reject);
        server.listen(0, "127.0.0.1", () => {
            const port = (server.address() as net.AddressInfo).port;
            server.close(() => resolve(port));
        });
    });
}

const sha256 = (text: string) => crypto.createHash("sha256").update(text, "utf8").digest("hex");

/// RS256 with the settings' key, the claims the metagame signs (controllers/auth.ts)
function signToken(privateKeyB64: string, uid: string, epoch: number): string {
    const part = (value: object) => Buffer.from(JSON.stringify(value)).toString("base64url");
    const now = Math.floor(Date.now() / 1000);
    const unsigned = `${part({ alg: "RS256", typ: "JWT" })}.${part({ userId: uid, epoch, iat: now, exp: now + 86400, aud: "undaunted-metagame", iss: "undaunted-metagame" })}`;
    const key = Buffer.from(privateKeyB64, "base64").toString("utf8");
    return `${unsigned}.${crypto.createSign("RSA-SHA256").update(unsigned).sign(key).toString("base64url")}`;
}

interface Server { name: string; port: number; chatPort: number; child: ReturnType<typeof Bun.spawn>; log: string }

async function startServer(name: string, command: string[], cwd: string, envFile: string, port: number, chatPort: number, log: string): Promise<Server> {
    const out = fs.openSync(log, "w");
    const child = Bun.spawn(command, { cwd, stdout: out, stderr: out, stdin: "pipe", env: { PATH: process.env.PATH ?? "", SystemRoot: process.env.SystemRoot ?? "" } });
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
        try {
            const reply = await fetch(`http://127.0.0.1:${port}/dauntless-status`);
            if (reply.ok) return { name, port, chatPort, child, log };
        } catch { /* not up yet */ }
        if (child.exitCode !== null) break;
        await Bun.sleep(250);
    }
    child.kill();
    throw new Error(`the ${name} metagame did not start (see ${log}); envFile ${envFile}`);
}

async function send(server: Server, request: { method: string; path: string; headers: Record<string, string>; body: string | undefined }) {
    try {
        const reply = await fetch(`http://127.0.0.1:${server.port}${request.path}`, {
            method: request.method, headers: request.headers, body: request.method === "GET" || request.method === "HEAD" ? undefined : request.body, redirect: "manual",
        });
        return { status: reply.status, body: await reply.text() };
    } catch (error) {
        return { status: 0, body: `request failed: ${error instanceof Error ? error.message : String(error)}` };
    }
}

class ChatPeer {
    readonly frames: string[] = [];
    private readonly socket: WebSocket;
    readonly opened: Promise<void>;

    constructor(port: number) {
        this.socket = new WebSocket(`ws://127.0.0.1:${port}//`, ["xmpp"]);
        this.socket.onmessage = event => this.frames.push(String(event.data));
        this.opened = new Promise((resolve, reject) => {
            this.socket.onopen = () => resolve();
            this.socket.onerror = () => reject(new Error("chat connection failed"));
        });
    }

    send(text: string) {
        if (this.socket.readyState === WebSocket.OPEN) this.socket.send(text);
    }

    close() {
        this.socket.close();
    }
}

async function main() {
    const args = parseArgs(process.argv.slice(2));
    const files = args.corpus.flatMap(item => fs.statSync(item).isDirectory()
        ? fs.readdirSync(item).filter(name => name.endsWith(".jsonl")).map(name => path.join(item, name))
        : [item]);
    const records = readCorpus(files.map(file => fs.readFileSync(file, "utf8")));
    const env = readEnv(args.env);
    const privateKey = env.get("AUTH_SIGNING_PRIVKEY_B64");
    if (!privateKey) usage(`${args.env} has no AUTH_SIGNING_PRIVKEY_B64`);
    const gatewaySecret = env.get("GATEWAY_SECRET") || undefined;

    const work = fs.mkdtempSync(path.join(os.tmpdir(), "dr-replay-"));
    const servers: Server[] = [];
    try {
        // Two copies of the snapshot, with the replay's own keys
        const keyHashes = new Set<string>();
        for (const record of records) {
            for (const match of JSON.stringify(record).matchAll(/<key:([0-9a-f]{64})>/g)) keyHashes.add(match[1]);
        }
        const keys = new Map<string, string>();
        const gsKey = crypto.randomBytes(24).toString("hex");
        const epochs = new Map<string, number>();
        for (const name of ["node", "rust"]) {
            const copy = path.join(work, `${name}.db`);
            fs.copyFileSync(args.db, copy);
            const db = new Database(copy);
            for (const hash of keyHashes) {
                if (!keys.has(hash)) keys.set(hash, `UUK_${crypto.randomBytes(24).toString("hex")}`);
                db.run("UPDATE userapikeys SET keyHash = ? WHERE keyHash = ?", [sha256(keys.get(hash)!), hash]);
            }
            db.run("INSERT INTO gameserverapikeys (keyHash) VALUES (?)", [sha256(gsKey)]);
            if (name === "node") {
                for (const row of db.query("SELECT userId, epoch FROM authepochs").all() as { userId: string; epoch: number }[]) epochs.set(row.userId, row.epoch);
                for (const hash of keyHashes) {
                    const found = db.query("SELECT count(*) AS n FROM userapikeys WHERE keyHash = ?").get(sha256(keys.get(hash)!)) as { n: number };
                    if (found.n === 0) keys.delete(hash);
                }
            }
            db.close();
        }
        const tokens = new Map<string, string>();
        const credentials: Credentials = {
            token: uid => {
                let token = tokens.get(uid);
                if (token === undefined) {
                    token = signToken(privateKey, uid, epochs.get(uid) ?? 0);
                    tokens.set(uid, token);
                }
                return token;
            },
            key: hash => keys.get(hash),
            gsKey,
        };

        // The two metagames
        for (const name of ["node", "rust"] as const) {
            const port = await freePort(), chatPort = await freePort();
            const settings = new Map(env);
            for (const drop of ["CAPTURE_DIR", "BACKUP_ROOT", "BODY_LOG_FILE", "LOG_BODIES"]) settings.delete(drop);
            settings.set("PORT", String(port));
            settings.set("BIND_HOST", "127.0.0.1");
            settings.set("CHAT_PORT", String(chatPort));
            settings.set("CHAT_BIND_HOST", "127.0.0.1");
            settings.set("DB_FILENAME", path.join(work, `${name}.db`));
            settings.set("STORE_IMAGES_DIR", path.resolve(args.node, "store-images"));
            settings.set("LOG_REQUESTS", "0");
            if (!args.keepMatchmaking) settings.set("MATCHMAKING_MODE", "DISABLED");
            const envFile = path.join(work, `${name}.env`);
            writeEnv(envFile, settings);
            const command = name === "node"
                ? ["node", `--env-file=${envFile}`, path.resolve(args.node, "dist", "server.js")]
                : [path.resolve(args.rust), "serve", "--env-file", envFile];
            servers.push(await startServer(name, command, path.resolve(args.node), envFile, port, chatPort, path.join(work, `${name}.log`)));
        }
        const [nodeServer, rustServer] = servers;

        // Replay in recorded order
        const normalizers = { node: new Normalizer(), rust: new Normalizer() };
        const ids = { node: new Map<string, string>(), rust: new Map<string, string>() };
        const report = { replayed: 0, skipped: {} as Record<string, number>, equal: 0, differences: [] as object[], inexact: [] as object[], chat: { sessions: 0, differences: [] as object[] } };
        const chats = new Map<number, { node: ChatPeer; rust: ChatPeer }>();
        for (const record of records) {
            if (record.kind === "chat") {
                const chat = record as ChatRecord;
                if (chat.event === "open") {
                    const pair = { node: new ChatPeer(nodeServer.chatPort), rust: new ChatPeer(rustServer.chatPort) };
                    await Promise.all([pair.node.opened, pair.rust.opened]);
                    chats.set(chat.conn, pair);
                    report.chat.sessions++;
                } else if (chat.dir === "in" && chat.text !== undefined) {
                    const pair = chats.get(chat.conn);
                    if (pair) {
                        const text = substitute(chat.text, credentials).text;
                        pair.node.send(text);
                        pair.rust.send(text);
                        await Bun.sleep(20);
                    }
                } else if (chat.event === "close") {
                    const pair = chats.get(chat.conn);
                    pair?.node.close();
                    pair?.rust.close();
                }
                continue;
            }
            const http = record as HttpRecord;
            const request = buildRequest(http, credentials, gatewaySecret);
            if ("skip" in request) {
                report.skipped[request.skip] = (report.skipped[request.skip] ?? 0) + 1;
                continue;
            }
            const node = await send(nodeServer, applyIds(request, ids.node));
            const rust = await send(rustServer, applyIds(request, ids.rust));
            report.replayed++;
            const recorded = http.resBody !== null && "json" in http.resBody ? http.resBody.json : undefined;
            if (recorded !== undefined) {
                for (const [name, reply] of [["node", node], ["rust", rust]] as const) {
                    try { learnIds(recorded, JSON.parse(reply.body), ids[name]); } catch { /* not JSON */ }
                }
            }
            const difference = compareReplies(node, rust, normalizers);
            const entry = { t: http.t, src: http.src, seq: http.seq, method: http.method, target: http.target, recordedStatus: http.status, node: node.status, rust: rust.status, difference };
            if (difference === undefined) report.equal++;
            else if (request.exact) report.differences.push({ ...entry, nodeBody: node.body.slice(0, 2000), rustBody: rust.body.slice(0, 2000) });
            else report.inexact.push(entry);
        }
        await Bun.sleep(500);
        for (const [conn, pair] of chats) {
            const node = comparableFrames(pair.node.frames, normalizers.node);
            const rust = comparableFrames(pair.rust.frames, normalizers.rust);
            const at = node.findIndex((frame, index) => frame !== rust[index]);
            if (at !== -1 || node.length !== rust.length) {
                const index = at === -1 ? Math.min(node.length, rust.length) : at;
                report.chat.differences.push({ conn, frame: index, node: node[index] ?? null, rust: rust[index] ?? null });
            }
            pair.node.close();
            pair.rust.close();
        }

        const summary = `replayed ${report.replayed} requests: ${report.equal} equal, ${report.differences.length} different, ${report.inexact.length} different where the recording cut a secret; `
            + `skipped ${Object.entries(report.skipped).map(([why, n]) => `${n} (${why})`).join(", ") || "none"}; `
            + `chat: ${report.chat.sessions} sessions, ${report.chat.differences.length} different`;
        console.log(summary);
        if (args.out) fs.writeFileSync(args.out, JSON.stringify({ summary, ...report }, null, 2));
        process.exitCode = report.differences.length > 0 || report.chat.differences.length > 0 ? 1 : 0;
    } finally {
        for (const server of servers) {
            server.child.kill();
            await server.child.exited;
        }
        if (!args.keepWork) fs.rmSync(work, { recursive: true, force: true });
        else console.log(`work folder kept: ${work}`);
    }
}

if (import.meta.main) {
    main().catch(error => {
        console.error(`replay: ${error instanceof Error ? error.message : String(error)}`);
        process.exit(2);
    });
}
