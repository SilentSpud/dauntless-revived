import type { NextFunction, Request, Response } from "express";
import type { Socket } from "node:net";
import { BodyTap, CaptureWriter, LoadCaptureOptions, RedactCaptureBody, RedactCaptureHeaders, RedactCaptureTarget } from "../capture";
import { logger } from "../logger";
import { IsTrustedGatewayRequest, NormalizeAddress } from "./RequestOrigin";

// Traffic capture at the metagame's own listener (MIGRATION_PLAN.md M2): the requests that never pass
// the gateway (game servers on game-server-key routes over loopback or Tailscale, and every player in
// private mode). Requests the gateway forwarded are recorded by the gateway, not twice. Off unless
// CAPTURE_DIR is set; see capture.ts for what is kept and what never is.

let Writer: CaptureWriter | null | undefined;
let NextRequest = 1;
let NextConnection = 1;
const Connections = new WeakMap<Socket, number>();

export function MetagameCapture(): CaptureWriter | undefined {
    if(Writer === undefined){
        const Options = LoadCaptureOptions(process.env, "metagame");
        Writer = Options === undefined ? null : new CaptureWriter(Options, (Message) => logger.warn(Message));
        if(Writer !== null){
            Writer.AddSecret(process.env.GATEWAY_SECRET);
            Writer.AddSecret(process.env.AUTH_SIGNING_PRIVKEY_B64);
            logger.warn(`traffic capture is on: requests that do not pass the gateway, and chat, are recorded, redacted, in ${Writer.Options.dir} for ${Writer.Options.retentionDays} days (personal data; see docs/legal.md)`);
        }
    }
    return Writer ?? undefined;
}

// Tests only
export function SetCaptureForTests(Next: CaptureWriter | undefined){
    Writer = Next ?? null;
}

export function ConnectionId(Socket: Socket): number {
    let Id = Connections.get(Socket);
    if(Id === undefined){
        Id = NextConnection++;
        Connections.set(Socket, Id);
    }
    return Id;
}

function ContentType(Value: unknown): string | undefined {
    return typeof Value === "string" ? Value : undefined;
}

export function CaptureRequests(req: Request, res: Response, next: NextFunction){
    const Capture = MetagameCapture();
    if(Capture === undefined || IsTrustedGatewayRequest(req)){
        next();
        return;
    }

    const Started = Date.now();
    const Max = Capture.Options.maxBodyBytes;
    const In = new BodyTap(Max);
    const Out = new BodyTap(Max);
    req.on("data", (Chunk: Buffer) => In.Add(Chunk));

    // The answer's bytes, as the routes write them
    const Tap = (Chunk: unknown, Encoding: unknown) => {
        if(Chunk === undefined || Chunk === null || typeof Chunk === "function"){
            return;
        }
        Out.Add(Buffer.isBuffer(Chunk) ? Chunk : Buffer.from(String(Chunk), typeof Encoding === "string" ? Encoding as BufferEncoding : "utf8"));
    };
    const Write = res.write;
    const End = res.end;
    res.write = function(this: Response, Chunk: unknown, ...Rest: unknown[]){
        Tap(Chunk, Rest[0]);
        return (Write as (...Args: unknown[]) => boolean).apply(this, [Chunk, ...Rest]);
    } as Response["write"];
    res.end = function(this: Response, Chunk?: unknown, ...Rest: unknown[]){
        Tap(Chunk, Rest[0]);
        return (End as (...Args: unknown[]) => Response).apply(this, [Chunk, ...Rest]);
    } as Response["end"];

    const Seq = NextRequest++;
    res.once("close", () => {
        const ResHeaders: Record<string, string | string[] | undefined> = {};
        for(const [Name, Value] of Object.entries(res.getHeaders())){
            ResHeaders[Name] = Value === undefined ? undefined : Array.isArray(Value) ? Value.map(String) : String(Value);
        }
        Capture.Write({
            kind: "http",
            conn: ConnectionId(req.socket),
            seq: Seq,
            ip: NormalizeAddress(req.socket?.remoteAddress) ?? "",
            method: req.method,
            target: RedactCaptureTarget(req.originalUrl),
            route: "metagame",
            reqHeaders: RedactCaptureHeaders(req.headers),
            reqBody: RedactCaptureBody(In.Body(), ContentType(req.headers["content-type"]), In.Total, Max),
            status: res.headersSent ? res.statusCode : 0,
            resHeaders: RedactCaptureHeaders(ResHeaders),
            resBody: RedactCaptureBody(Out.Body(), ContentType(ResHeaders["content-type"]), Out.Total, Max),
            ms: Date.now() - Started,
            ...(res.writableFinished ? {} : { aborted: true }),
        });
    });
    next();
}
