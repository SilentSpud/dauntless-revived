import crypto from "node:crypto";
import { spawn, type ChildProcess } from "node:child_process";
import http, { type Server } from "node:http";
import { AddressInfo } from "node:net";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { gameserverapikeys } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { logger } from "../src/logger";

// The whole app over HTTP on a port the system picks, with a game-server key of this test process.
// Import after ./setup, ./authenv and ./appenv.
//
// With METAGAME_RUST_BIN set to the Rust metagame's binary, the same calls go to it instead
// (MIGRATION_PLAN.md M31): it runs on this test file's database with this process's environment.
// The Rust server reads its settings once, so a call made after a test changed the environment
// starts it again (the database carries over; what it keeps in memory does not). Its warnings are
// collected in Warnings as the app's are.

export const GS_KEY = crypto.randomBytes(24).toString("hex");

let Listening: Server | undefined;
let Base = "";

const RustBin = process.env.METAGAME_RUST_BIN;
let Rust: ChildProcess | undefined;
let RustEnv = "";

function EnvText(){
    return JSON.stringify(Object.entries(process.env).filter(([Name]) => Name !== "METAGAME_RUST_BIN").sort());
}

async function StartRust(){
    RustEnv = EnvText();
    const Child = spawn(RustBin!, ["serve"], {
        env: { ...process.env, PORT: "0", LOG_LEVEL: "info", METAGAME_RUST_BIN: undefined },
        stdio: ["pipe", "pipe", "inherit"]
    });
    Rust = Child;
    Base = await new Promise<string>((Resolve, Reject) => {
        let Pending = "";
        let Found = false;
        Child.stdout!.setEncoding("utf8");
        Child.stdout!.on("data", (Chunk: string) => {
            Pending += Chunk;
            let End: number;
            while((End = Pending.indexOf("\n")) >= 0){
                const Line = Pending.slice(0, End);
                Pending = Pending.slice(End + 1);
                let Entry: any;
                try{ Entry = JSON.parse(Line); } catch { continue; }
                if(Entry.level === 40 && typeof Entry.msg === "string") Warnings.push(Entry.msg);
                const Bound = !Found && typeof Entry.msg === "string" ? /metagame on ([\d.]+):(\d+)$/.exec(Entry.msg) : null;
                if(Bound){
                    Found = true;
                    Resolve(`http://${Bound[1]}:${Bound[2]}`);
                }
            }
        });
        Child.on("exit", (Code) => { if(!Found) Reject(new Error(`The Rust metagame stopped before it listened (${Code})`)); });
    });
}

async function StopRust(){
    const Child = Rust;
    Rust = undefined;
    RustAgent.destroy();
    if(Child === undefined || Child.exitCode !== null) return;
    const Exited = new Promise<void>((Resolve) => Child.once("exit", () => Resolve()));
    Child.stdin!.end("shutdown\n");
    const Timer = setTimeout(() => Child.kill(), 10000);
    await Exited;
    clearTimeout(Timer);
}

// fetch (undici) waits for its next half-second timer tick before reusing a connection to hyper, so
// the Rust calls go through node:http, keeping connections alive the same way.
const RustAgent = new http.Agent({ keepAlive: true });

async function RustCall(Method: string, Path: string, Headers: Record<string, string>, Body: string | undefined): Promise<Reply> {
    const Answer = await new Promise<{ status: number, text: string }>((Resolve, Reject) => {
        // As fetch sends them: node:http would send a DELETE's body with no length at all
        const Sized = Body === undefined ? Headers : { ...Headers, "content-length": String(Buffer.byteLength(Body)) };
        const Request = http.request(Base + Path, { method: Method, headers: Sized, agent: RustAgent }, (Response) => {
            const Chunks: Buffer[] = [];
            Response.on("data", (Chunk: Buffer) => Chunks.push(Chunk));
            Response.on("end", () => Resolve({ status: Response.statusCode ?? 0, text: Buffer.concat(Chunks).toString("utf8") }));
            Response.on("error", Reject);
        });
        Request.on("error", Reject);
        Request.end(Body);
    });
    let Json: any;

    try{ Json = Answer.text.length > 0 ? JSON.parse(Answer.text) : undefined; } catch { Json = undefined; }

    // The server's log lines for this call arrive on their own
    await new Promise((Resolve) => setTimeout(Resolve, 5));

    return { status: Answer.status, json: Json, text: Answer.text };
}

// What the app keeps in memory (parties, rate-limit windows) starts empty again: the Node app's
// own test resets do that in process; the Rust server is started again.
export async function ResetAppMemory(){
    if(RustBin){
        await StopRust();
        await StartRust();
    }
}

// For a test that reaches into the Node app in process (its clock, offers it is handed), which a
// separate server can't follow: skipped against the Rust binary, with the reason.
export function NodeOnly(Why: string){
    return { skip: RustBin ? `Node only: ${Why}` : false };
}

export async function StartApp(){
    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY, "utf8").digest("hex") }).run();

    if(RustBin){
        await StartRust();
        return;
    }

    Listening = await new Promise<Server>((Resolve, Reject) => {
        const Started = app.listen(0, "127.0.0.1", (Error?: Error) => Error ? Reject(Error) : Resolve(Started));
    });

    // The server never closes an idle connection itself. Client and server share this process, so a test
    // that keeps the event loop busy for seconds (buying the whole catalogue) could let the server's
    // keep-alive timer close a pooled connection just as the next request reuses it (ECONNRESET). The
    // fetch client still retires idle connections on its own.
    Listening.keepAliveTimeout = 0;

    Base = `http://127.0.0.1:${(Listening.address() as AddressInfo).port}`;
}

export async function StopApp(){
    if(RustBin){
        await StopRust();
        return;
    }
    Listening?.closeAllConnections();
    await new Promise<void>((Resolve) => Listening ? Listening.close(() => Resolve()) : Resolve());
}

export type Reply = { status: number, json: any, text: string };

// as: the account whose token is sent (the bearer); gs: send the game-server key; headers: any others
export async function Call(Method: string, Path: string, Options: { as?: string, gs?: boolean, body?: unknown, raw?: string, headers?: Record<string, string> } = {}): Promise<Reply> {
    const Headers: Record<string, string> = { ...(Options.headers ?? {}) };

    if(RustBin && EnvText() !== RustEnv){
        await StopRust();
        await StartRust();
    }

    if(Options.as !== undefined) Headers["authorization"] = `bearer ${SignMetagameJWTForUid(Options.as)}`;
    if(Options.gs) Headers["x-undaunted-gameserver-apikey"] = GS_KEY;
    if(Options.body !== undefined || Options.raw !== undefined) Headers["content-type"] = "application/json; charset=utf-8";

    const Body = Options.raw !== undefined ? Options.raw : Options.body === undefined ? undefined : JSON.stringify(Options.body);

    if(RustBin){
        return await RustCall(Method, Path, Headers, Body);
    }

    const Response = await fetch(Base + Path, {
        method: Method,
        headers: Headers,
        body: Body
    });
    const Text = await Response.text();
    let Json: any;

    try{ Json = Text.length > 0 ? JSON.parse(Text) : undefined; } catch { Json = undefined; }


    return { status: Response.status, json: Json, text: Text };
}

// Every warning the app logs while a test runs (the logger itself stays silent)
export const Warnings: string[] = [];
const OriginalWarn = logger.warn.bind(logger);

(logger as any).warn = (...Args: unknown[]) => {
    const Message = Args.find((Arg) => typeof Arg === "string");
    Warnings.push(typeof Message === "string" ? Message : JSON.stringify(Args[0]));
    return (OriginalWarn as any)(...Args);
};
