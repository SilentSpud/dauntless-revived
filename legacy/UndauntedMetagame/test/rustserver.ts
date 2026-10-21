import { spawn, type ChildProcess } from "node:child_process";
import http from "node:http";
import path from "node:path";

// The Rust metagame for the HTTP tests (MIGRATION_PLAN.md M31): with METAGAME_RUST_BIN naming its
// binary, the tests' calls go to it instead of the Node app, on the test file's database and with
// this process's environment. It reads its settings once, so a call made after a test changed the
// environment starts it again (the database carries over; what it keeps in memory does not).
// Imports nothing of the app, so ./rustshim can load it before a test file sets its environment.

export const RustBin = process.env.METAGAME_RUST_BIN;

// The server's log lines go to the app's logger (once a test loaded it), at their level, so a
// test that watches the logger sees them as it sees the Node app's
const LoggerModule = path.join(__dirname, "..", "src", "logger.js");
const LEVELS: Record<number, string> = { 10: "trace", 20: "debug", 30: "info", 40: "warn", 50: "error", 60: "fatal" };

function Forward(Level: number, Message: string){
    const Logger = (require.cache[LoggerModule]?.exports as { logger?: Record<string, (Message: string) => void> } | undefined)?.logger;
    const Name = LEVELS[Level];
    if(Logger !== undefined && Name !== undefined) Logger[Name](Message);
}

// For a test that reaches into the Node app in process (its clock, offers it is handed, what it
// holds in memory) or changes a switch Node reads on every request, which a separate server reading
// its settings once can't follow: skipped against the Rust binary, with the reason.
export function NodeOnly(Why: string){
    return { skip: RustBin ? `Node only: ${Why}` : false };
}

let Child: ChildProcess | undefined;
let StartedEnv = "";
let Port = "0";
export let RustBase = "";
// Every port the server has listened on: a test keeps the address it was first given
export const RustPorts = new Set<string>();

function EnvText(){
    return JSON.stringify(Object.entries(process.env).filter(([Name]) => Name !== "METAGAME_RUST_BIN").sort());
}

// port: the one a test asked for, or "0" for one the system picks
export async function StartRust(port = Port){
    Port = port;
    StartedEnv = EnvText();
    const Started = spawn(RustBin!, ["serve"], {
        // The Node tests' app never starts chat (server.ts does); neither does this, unless a test says so
        env: { CHAT: "0", ...process.env, PORT: port, LOG_LEVEL: "info", METAGAME_RUST_BIN: undefined },
        stdio: ["pipe", "pipe", "inherit"]
    });
    Child = Started;
    RustBase = await new Promise<string>((Resolve, Reject) => {
        let Pending = "";
        let Found = false;
        Started.stdout!.setEncoding("utf8");
        Started.stdout!.on("data", (Chunk: string) => {
            Pending += Chunk;
            let End: number;
            while((End = Pending.indexOf("\n")) >= 0){
                const Line = Pending.slice(0, End);
                Pending = Pending.slice(End + 1);
                let Entry: any;
                try{ Entry = JSON.parse(Line); } catch { continue; }
                if(typeof Entry.level === "number" && typeof Entry.msg === "string") Forward(Entry.level, Entry.msg);
                const Bound = !Found && typeof Entry.msg === "string" ? /metagame on ([\d.]+):(\d+)$/.exec(Entry.msg) : null;
                if(Bound){
                    Found = true;
                    Resolve(`http://${Bound[1]}:${Bound[2]}`);
                }
            }
        });
        Started.on("exit", (Code) => { if(!Found) Reject(new Error(`The Rust metagame stopped before it listened (${Code})`)); });
    });
    RustPorts.add(new URL(RustBase).port);
    return RustBase;
}

export async function StopRust(){
    const Stopping = Child;
    Child = undefined;
    Agent.destroy();
    if(Stopping === undefined || Stopping.exitCode !== null) return;
    const Exited = new Promise<void>((Resolve) => Stopping.once("exit", () => Resolve()));
    Stopping.stdin!.end("shutdown\n");
    const Timer = setTimeout(() => Stopping.kill(), 10000);
    await Exited;
    clearTimeout(Timer);
}

// A test reset what the app keeps in memory (its Reset...ForTests hooks): the server starts again
// before the next call
let ResetAsked = false;

export function AskRustReset(){
    ResetAsked = true;
}

// Starts the server again when the environment changed since it started, or a reset was asked for
export async function RustCurrent(){
    if(Child !== undefined && (ResetAsked || EnvText() !== StartedEnv)){
        ResetAsked = false;
        await StopRust();
        await StartRust();
    }
}

// fetch (undici) waits for its next half-second timer tick before reusing an idle keep-alive
// connection (any server's, outside this process's own http.Server), so the calls go through
// node:http, keeping connections alive the same way.
const Agent = new http.Agent({ keepAlive: true });

export type RustAnswer = { status: number, headers: http.IncomingHttpHeaders, body: Buffer };

// Path: the path and query, sent to wherever the server listens now (a restart may move it)
export async function RustRequest(Method: string, Path: string, Headers: Record<string, string>, Body: string | Buffer | undefined): Promise<RustAnswer> {
    await RustCurrent();
    const Answer = await new Promise<RustAnswer>((Resolve, Reject) => {
        // As fetch sends them: node:http would send a DELETE's body with no length at all
        const Sized = Body === undefined ? Headers : { ...Headers, "content-length": String(Buffer.byteLength(Body)) };
        const Request = http.request(RustBase + Path, { method: Method, headers: Sized, agent: Agent }, (Response) => {
            const Chunks: Buffer[] = [];
            Response.on("data", (Chunk: Buffer) => Chunks.push(Chunk));
            Response.on("end", () => Resolve({ status: Response.statusCode ?? 0, headers: Response.headers, body: Buffer.concat(Chunks) }));
            Response.on("error", Reject);
        });
        Request.on("error", Reject);
        Request.end(Body);
    });
    // The server's log lines for this call arrive on their own
    await new Promise((Resolve) => setTimeout(Resolve, 5));
    return Answer;
}
