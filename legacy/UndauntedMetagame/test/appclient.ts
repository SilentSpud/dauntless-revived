import crypto from "node:crypto";
import { type Server } from "node:http";
import { AddressInfo } from "node:net";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { gameserverapikeys } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { logger } from "../src/logger";
import { RustBin, RustRequest, StartRust, StopRust } from "./rustserver";
export { NodeOnly } from "./rustserver";

// The whole app over HTTP on a port the system picks, with a game-server key of this test process.
// Import after ./setup, ./authenv and ./appenv.
// With METAGAME_RUST_BIN set, the same calls go to the Rust metagame (./rustserver), whose warnings
// reach Warnings through the app's logger.

export const GS_KEY = crypto.randomBytes(24).toString("hex");

let Listening: Server | undefined;
let Base = "";

async function RustCall(Method: string, Path: string, Headers: Record<string, string>, Body: string | undefined): Promise<Reply> {
    const Answer = await RustRequest(Method, Path, Headers, Body);
    const Text = Answer.body.toString("utf8");
    let Json: any;

    try{ Json = Text.length > 0 ? JSON.parse(Text) : undefined; } catch { Json = undefined; }

    return { status: Answer.status, json: Json, text: Text };
}

// What the app keeps in memory (parties, rate-limit windows) starts empty again: the Node app's
// own test resets do that in process; the Rust server is started again.
export async function ResetAppMemory(){
    if(RustBin){
        await StopRust();
        await StartRust();
    }
}

export async function StartApp(){
    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY, "utf8").digest("hex") }).run();

    if(RustBin){
        await StartRust("0");
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
