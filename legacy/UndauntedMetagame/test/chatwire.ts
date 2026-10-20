import crypto from "node:crypto";
import net from "node:net";
import { once } from "node:events";
import WebSocket from "ws";
import { SignMetagameJWTForUid } from "../src/controllers/auth";
import { logger } from "../src/logger";

// A raw chat client for the chat tests, shaped like the 1.4.4 client's connection (libwebsockets 3.0
// upgrade to "//" with protocol xmpp; libstrophe stanzas without a jabber:client namespace; bind id
// _xmpp_bind1). Every frame it receives is kept in order, so a test can feed them to the client model in
// chatclient.ts. This is a test helper only; nothing in src/ imports it.

export const DOMAIN = "prod.ol.epicgames.com";
export const FRAMING = "urn:ietf:params:xml:ns:xmpp-framing";

// The resource the game binds: V2:<AppId>:WIN::<32 hex>, a new one at every login
export function GameResource(AppId = "MissingGameServiceForAppId"): string {
    return `V2:${AppId}:WIN::${crypto.randomBytes(16).toString("hex").toUpperCase()}`;
}

export function Base64Plain(Authz: string, Authc: string, Token: string): string {
    return Buffer.from(`${Authz}\0${Authc}\0${Token}`, "utf8").toString("base64");
}

let NextBarrier = 1;

export class WireClient {
    readonly Socket: WebSocket;
    // Every frame received, in order
    readonly Frames: string[] = [];
    readonly Closed: Promise<{ Code: number }>;
    Uid = "";
    Resource = "";
    // Answer the server's liveness pings, as the game does (they are kept in Frames, not queued)
    AutoPong = true;
    Domain = DOMAIN;
    private readonly Queue: string[] = [];
    private Waiters: Array<(Frame: string | undefined) => void> = [];
    private IsClosed = false;

    private constructor(Socket: WebSocket) {
        this.Socket = Socket;
        this.Closed = new Promise((Resolve) => {
            Socket.on("close", (Code: number) => {
                this.IsClosed = true;
                for(const Waiter of this.Waiters.splice(0)) Waiter(undefined);
                Resolve({ Code });
            });
        });
        Socket.on("error", () => undefined);
        Socket.on("message", (Data) => {
            const Frame = Data.toString();

            this.Frames.push(Frame);

            const Ping = /^<iq [^>]*type="get" id="(sp\d+)"[^>]*><ping xmlns="urn:xmpp:ping"\/><\/iq>$/.exec(Frame);

            if(Ping !== null && this.AutoPong){
                this.Send(`<iq type="result" id="${Ping[1]}"/>`);
                return;
            }

            const Waiter = this.Waiters.shift();

            if(Waiter !== undefined) Waiter(Frame); else this.Queue.push(Frame);
        });
    }

    static async Connect(Port: number, Headers: Record<string, string> = {}): Promise<WireClient> {
        const Socket = new WebSocket(`ws://127.0.0.1:${Port}//`, ["xmpp"], {
            headers: { Pragma: "no-cache", "Cache-Control": "no-cache", ...Headers },
            origin: "http://127.0.0.1",
            perMessageDeflate: false
        });
        const Client = new WireClient(Socket);

        await once(Socket, "open");
        return Client;
    }

    get IsOpen(): boolean {
        return !this.IsClosed && this.Socket.readyState === WebSocket.OPEN;
    }

    Send(Stanza: string | Buffer, Binary = false): void {
        this.Socket.send(Stanza, { binary: Binary });
    }

    // The next frame, or an error after TimeoutMs (undefined when the socket closed)
    Next(Label = "chat frame", TimeoutMs = 2000): Promise<string | undefined> {
        const Queued = this.Queue.shift();

        if(Queued !== undefined){
            return Promise.resolve(Queued);
        }

        if(this.IsClosed){
            return Promise.resolve(undefined);
        }

        return new Promise((Resolve, Reject) => {
            const Waiter = (Frame: string | undefined) => { clearTimeout(Timer); Resolve(Frame); };
            const Timer = setTimeout(() => {
                this.Waiters = this.Waiters.filter((Other) => Other !== Waiter);
                Reject(new Error(`${Label} timed out`));
            }, TimeoutMs);

            this.Waiters.push(Waiter);
        });
    }

    async Expect(Label = "chat frame"): Promise<string> {
        const Frame = await this.Next(Label);

        if(Frame === undefined){
            throw new Error(`${Label}: the socket closed`);
        }

        return Frame;
    }

    // Everything the server sent before the answer to a fresh ping: the server handles one
    // connection's frames in order, so this collects every frame caused by what was sent before
    async Barrier(): Promise<string[]> {
        const Id = `barrier-${NextBarrier++}`;
        const Before: string[] = [];

        this.Send(`<iq type="get" id="${Id}"><ping xmlns="urn:xmpp:ping"/></iq>`);

        for(;;){
            const Frame = await this.Expect(`answer to ${Id}`);

            if(Frame.includes(`id="${Id}"`) && Frame.includes(`type="result"`)){
                return Before;
            }

            Before.push(Frame);
        }
    }

    // Queued frames not yet read
    Drain(): string[] {
        return this.Queue.splice(0);
    }

    Close(): void {
        this.Socket.terminate();
    }

    // A clean logout: <close/>, which the server has fully handled once its own <close/> comes back
    async Logout(): Promise<void> {
        if(!this.IsOpen){
            return;
        }

        this.Send(`<close xmlns="${FRAMING}"/>`);
        await this.Closed;
    }
}

export type LoginOptions = { Resource?: string, OpenTo?: string | null, Token?: string, Headers?: Record<string, string> };

// Open, SASL PLAIN with the account's own token, open again, bind: exactly as the game does
export async function Login(Port: number, Uid: string, Options: LoginOptions = {}): Promise<WireClient> {
    const Client = await WireClient.Connect(Port, Options.Headers);
    const OpenTo = Options.OpenTo === undefined ? DOMAIN : Options.OpenTo;
    const Open = `<open xmlns="${FRAMING}"${OpenTo !== null ? ` to="${OpenTo}"` : ""} version="1.0"/>`;

    Client.Send(Open);
    await Client.Expect("open");
    await Client.Expect("features");
    Client.Send(`<auth xmlns="urn:ietf:params:xml:ns:xmpp-sasl" mechanism="PLAIN">${Base64Plain("", Uid, Options.Token ?? SignMetagameJWTForUid(Uid))}</auth>`);

    const Success = await Client.Expect("success");

    if(!Success.includes("<success")){
        throw new Error(`login of ${Uid} was refused`);
    }

    Client.Send(Open);
    await Client.Expect("open again");
    await Client.Expect("bind features");

    const Resource = Options.Resource ?? GameResource();

    Client.Send(`<iq type="set" id="_xmpp_bind1"><bind xmlns="urn:ietf:params:xml:ns:xmpp-bind"><resource>${Resource}</resource></bind></iq>`);

    const Bound = await Client.Expect("bind result");
    const Jid = /<jid>([^<]*)<\/jid>/.exec(Bound)?.[1];

    if(Jid === undefined){
        throw new Error(`bind of ${Uid} failed`);
    }

    Client.Uid = Uid;
    Client.Resource = Jid.slice(Jid.indexOf("/") + 1);
    Client.Domain = Jid.slice(Jid.indexOf("@") + 1, Jid.indexOf("/"));
    return Client;
}

// A raw TCP connection upgraded to WebSocket by hand, for frames the ws client refuses to send
export async function RawUpgrade(Port: number): Promise<net.Socket> {
    const Socket = net.connect(Port, "127.0.0.1");

    await once(Socket, "connect");
    Socket.write([
        "GET // HTTP/1.1",
        "Host: 127.0.0.1",
        "Upgrade: websocket",
        "Connection: Upgrade",
        `Sec-WebSocket-Key: ${crypto.randomBytes(16).toString("base64")}`,
        "Sec-WebSocket-Protocol: xmpp",
        "Sec-WebSocket-Version: 13",
        "", ""
    ].join("\r\n"));

    let Head = "";

    while(!Head.includes("\r\n\r\n")){
        const [Chunk] = await once(Socket, "data") as [Buffer];

        Head += Chunk.toString("latin1");
    }

    if(!Head.startsWith("HTTP/1.1 101")){
        throw new Error(`no upgrade: ${Head.split("\r\n")[0]}`);
    }

    return Socket;
}

// One masked client frame (payload under 126 bytes)
export function ClientFrame(Opcode: number, Payload: Buffer, Masked = true): Buffer {
    const Mask = crypto.randomBytes(4);
    const Head = Buffer.from([0x80 | Opcode, (Masked ? 0x80 : 0) | Payload.length]);
    const Body = Buffer.from(Payload.map((Byte, Index) => Masked ? Byte ^ Mask[Index % 4] : Byte));

    return Buffer.concat(Masked ? [Head, Mask, Body] : [Head, Body]);
}

// Resolves when the peer closes the raw socket (or after TimeoutMs with false)
export function RawClosed(Socket: net.Socket, TimeoutMs = 2000): Promise<boolean> {
    return new Promise((Resolve) => {
        const Timer = setTimeout(() => Resolve(false), TimeoutMs);

        Socket.on("error", () => undefined);
        Socket.once("close", () => { clearTimeout(Timer); Resolve(true); });
        Socket.resume();
    });
}

// Every log line written while capturing, as text ("info chat: ..."). The logger is silent in tests,
// so its methods are swapped for recorders.
export type LogCapture = { Lines: string[], Stop(): void };

export function CaptureLogs(): LogCapture {
    const Lines: string[] = [];
    const Target = logger as unknown as Record<string, unknown>;
    const Levels = ["fatal", "error", "warn", "info", "debug", "trace"];
    const Saved = new Map(Levels.map((Level) => [Level, Target[Level]]));

    for(const Level of Levels){
        Target[Level] = (...Args: unknown[]) => {
            Lines.push(`${Level} ${Args.map((Arg) => typeof Arg === "string" ? Arg : Arg instanceof Error ? `${Arg.name}: ${Arg.message}` : JSON.stringify(Arg)).join(" ")}`);
        };
    }

    return {
        Lines,
        Stop(){
            for(const [Level, Method] of Saved) Target[Level] = Method;
        }
    };
}
