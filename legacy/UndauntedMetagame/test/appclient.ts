import crypto from "node:crypto";
import { type Server } from "node:http";
import { AddressInfo } from "node:net";
import { app } from "../src/app";
import { GetDb } from "../src/db";
import { gameserverapikeys } from "../src/db/schema";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { logger } from "../src/logger";

// The whole app over HTTP on a port the system picks, with a game-server key of this test process.
// Import after ./setup, ./authenv and ./appenv.

export const GS_KEY = crypto.randomBytes(24).toString("hex");

let Listening: Server | undefined;
let Base = "";

export async function StartApp(){
    GetDb().insert(gameserverapikeys).values({ keyHash: crypto.createHash("sha256").update(GS_KEY, "utf8").digest("hex") }).run();

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

    const Response = await fetch(Base + Path, {
        method: Method,
        headers: Headers,
        body: Options.raw !== undefined ? Options.raw : Options.body === undefined ? undefined : JSON.stringify(Options.body)
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
