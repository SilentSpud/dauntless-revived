import { EventEmitter } from "node:events";
import path from "node:path";
import Module from "node:module";
import express from "express";
import { AskRustReset, RustBin, RustPorts, RustRequest, StartRust, StopRust } from "./rustserver";

// For the HTTP tests that start the app themselves (app.listen) and call it with fetch: preloaded
// with `node --require build/test/rustshim.js` when METAGAME_RUST_BIN is set (MIGRATION_PLAN.md
// M31). The app's listen starts the Rust metagame on the port asked for instead, and fetch to it
// goes through ./rustserver. A test's own small express apps (a router under test) stay in Node.
// The app's Reset...ForTests hooks also start the Rust server again before its next call.

const AppModule = path.join(__dirname, "..", "src", "app.js");

function IsTheApp(Candidate: unknown){
    const Loaded = require.cache[AppModule];
    return Loaded !== undefined && (Loaded.exports as { app?: unknown }).app === Candidate;
}

// What app.listen gives back: what the tests use of an http.Server
class RustListening extends EventEmitter {
    keepAliveTimeout = 0;
    private Port = 0;

    async Start(Port: number){
        this.Port = Number(new URL(await StartRust(String(Port))).port);
        this.emit("listening");
    }

    address(){
        return { address: "127.0.0.1", family: "IPv4", port: this.Port };
    }

    closeAllConnections(){}

    close(Done?: (Error?: Error) => void){
        StopRust().then(() => { this.emit("close"); Done?.(); }, (Error) => Done?.(Error));
        return this;
    }
}

if(RustBin){
    const Listen = express.application.listen;

    express.application.listen = function(this: unknown, ...Args: any[]){
        if(!IsTheApp(this)){
            return (Listen as any).apply(this, Args);
        }
        const Port = typeof Args[0] === "number" ? Args[0] : Number(Args[0] ?? 0);
        const Done = Args.find((Arg) => typeof Arg === "function") as ((Error?: Error) => void) | undefined;
        const Listening = new RustListening();
        Listening.Start(Port).then(() => Done?.(), (Error) => Done ? Done(Error) : Listening.emit("error", Error));
        return Listening;
    } as any;

    const Load = (Module as any)._load;
    const Wrapped = new WeakSet<object>();

    (Module as any)._load = function(this: unknown, ...Args: unknown[]){
        const Loaded = Load.apply(this, Args);
        if(Loaded !== null && typeof Loaded === "object" && !Wrapped.has(Loaded)){
            Wrapped.add(Loaded);
            for(const Name of Object.keys(Loaded)){
                const Hook = Loaded[Name];
                if(/^Reset\w*ForTests$/.test(Name) && typeof Hook === "function"){
                    try{
                        Loaded[Name] = (...HookArgs: unknown[]) => { AskRustReset(); return Hook(...HookArgs); };
                    } catch { /* a read-only export stays as it is */ }
                }
            }
        }
        return Loaded;
    };

    const Fetch = globalThis.fetch;

    globalThis.fetch = async (Input: any, Init?: any) => {
        const Url = new URL(typeof Input === "string" || Input instanceof URL ? String(Input) : Input.url);
        if(Url.hostname !== "127.0.0.1" || !RustPorts.has(Url.port)){
            return Fetch(Input, Init);
        }
        const Headers: Record<string, string> = {};
        new globalThis.Headers(Init?.headers ?? (Input instanceof Request ? Input.headers : undefined)).forEach((Value, Name) => { Headers[Name] = Value; });
        const Body = Init?.body === undefined || Init.body === null ? undefined : typeof Init.body === "string" ? Init.body : Buffer.from(await new Response(Init.body).arrayBuffer());
        const Answer = await RustRequest(Init?.method ?? "GET", Url.pathname + Url.search, Headers, Body);
        const Reply = new globalThis.Headers();
        for(const [Name, Value] of Object.entries(Answer.headers)){
            for(const One of Array.isArray(Value) ? Value : Value === undefined ? [] : [Value]) Reply.append(Name, One);
        }
        const NoBody = Answer.status === 204 || Answer.status === 304 || (Init?.method ?? "GET").toUpperCase() === "HEAD";
        return new Response(NoBody ? null : new Uint8Array(Answer.body), { status: Answer.status, headers: Reply });
    };
}
