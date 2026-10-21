// Traffic capture for the shadow replay (MIGRATION_PLAN.md M2, M35): every request and answer as one
// redacted JSON line, so the recorded play can be replayed against the Node and the Rust services and
// their answers compared before the cutover.
//
// Off unless CAPTURE_DIR is set. What is written is personal data (usernames, addresses, play
// history): files are private (0600), one per day, and deleted after CAPTURE_RETENTION_DAYS (default
// 14). Nothing secret is ever written:
//   - a login token becomes <token:UID-...> (its subject, read without the signature), so the replay
//     can sign a fresh one for the same account;
//   - an account key becomes <key:sha256 of the key>, the hash the database already stores, so the
//     replay can swap in a key of its own for that account;
//   - the game-server key becomes <gskey>; every other secret-looking value becomes <redacted>.
//
// This file is the same in legacy/UndauntedGateway and legacy/UndauntedMetagame (a test checks it).

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

export type CaptureOptions = { dir: string; retentionDays: number; maxBodyBytes: number; source: string };

export function LoadCaptureOptions(Env: NodeJS.ProcessEnv, Source: string): CaptureOptions | undefined {
    const Dir = (Env.CAPTURE_DIR ?? "").trim();
    if(Dir === ""){
        return undefined;
    }
    const Int = (Name: string, Default: number, Min: number, Max: number) => {
        const Raw = (Env[Name] ?? "").trim();
        if(Raw === ""){
            return Default;
        }
        const Value = Number(Raw);
        if(!Number.isInteger(Value) || Value < Min || Value > Max){
            throw new Error(`${Name} must be a whole number from ${Min} to ${Max} (got ${JSON.stringify(Raw)})`);
        }
        return Value;
    };
    return {
        dir: path.resolve(Dir),
        retentionDays: Int("CAPTURE_RETENTION_DAYS", 14, 1, 90),
        maxBodyBytes: Int("CAPTURE_MAX_BODY_BYTES", 256 * 1024, 0, 16 * 1024 * 1024),
        source: Source,
    };
}

// ---- Redaction ----

const ACCOUNT_KEY = /UUK_[0-9A-Za-z]+/g;
const JWT = /eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g;
const LONG_HEX = /[0-9a-fA-F]{40,}/g;
const LONG_TOKEN = /[A-Za-z0-9_+/=-]{100,}/g;
// JSON fields and form fields whose values are secrets whatever they look like
const SECRET_NAME = /(key|token|secret|pass|auth|jwt|sig|cred|recovery|invite|cookie)/i;

function Sha256(Text: string): string {
    return crypto.createHash("sha256").update(Text, "utf8").digest("hex");
}

function TokenSubject(Token: string): string | undefined {
    try{
        const Payload = JSON.parse(Buffer.from(Token.split(".")[1], "base64url").toString("utf8"));
        const Subject = Payload?.userId ?? Payload?.sub;
        return typeof Subject === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(Subject) ? Subject : undefined;
    }
    catch{
        return undefined;
    }
}

// Tokens and keys become placeholders the replay can substitute; other long secrets are cut.
export function RedactCaptureText(Text: string): string {
    // Placeholders go in last, so the generic rules never cut the hash inside <key:...>
    const Held: string[] = [];
    const Hold = (Placeholder: string) => `\u0001${Held.push(Placeholder) - 1}\u0001`;
    return Text
        .replace(/\u0001/g, "?")
        .replace(JWT, (Token) => {
            const Subject = TokenSubject(Token);
            return Hold(Subject === undefined ? "<token>" : `<token:${Subject}>`);
        })
        .replace(ACCOUNT_KEY, (Key) => Hold(`<key:${Sha256(Key)}>`))
        .replace(LONG_HEX, "<redacted>")
        .replace(LONG_TOKEN, "<redacted>")
        .replace(/\u0001(\d+)\u0001/g, (_, Index: string) => Held[Number(Index)]);
}

function RedactNamed(Name: string, Value: string): string {
    if(!SECRET_NAME.test(Name)){
        return RedactCaptureText(Value);
    }
    // A secret field keeps its placeholder when it is a token or a key (the replay needs those)
    const Redacted = RedactCaptureText(Value);
    return /^<(token|key):[^>]+>$/.test(Redacted) ? Redacted : "<redacted>";
}

export function RedactCaptureJson(Value: unknown, Name = ""): unknown {
    if(typeof Value === "string"){
        return RedactNamed(Name, Value);
    }
    if(Array.isArray(Value)){
        return Value.map((Item) => RedactCaptureJson(Item, Name));
    }
    if(Value !== null && typeof Value === "object"){
        const Out: Record<string, unknown> = {};
        for(const [Key, Item] of Object.entries(Value)){
            Out[Key] = RedactCaptureJson(Item, Key);
        }
        return Out;
    }
    return Value;
}

// Header values kept for the replay: authorization and the account key as placeholders, the
// game-server key as <gskey>, cookies and the gateway's own headers dropped.
export function RedactCaptureHeaders(Headers: Record<string, string | string[] | undefined>): Record<string, string | string[]> {
    const Out: Record<string, string | string[]> = {};
    for(const [Raw, Value] of Object.entries(Headers)){
        if(Value === undefined){
            continue;
        }
        const Name = Raw.toLowerCase();
        if(Name === "cookie" || Name === "set-cookie" || Name === "x-dauntless-gateway"){
            continue;
        }
        const One = (Text: string) => Name === "x-undaunted-gameserver-apikey" ? "<gskey>"
            : Name === "authorization" ? RedactCaptureText(Text)
            : RedactNamed(Name.replace(/^x-undaunted-/, ""), Text);
        Out[Name] = Array.isArray(Value) ? Value.map(One) : One(Value);
    }
    return Out;
}

// A body as the capture keeps it: JSON (redacted by field), a form (redacted by field), text
// (redacted), or only its size for binary or oversized bodies.
export function RedactCaptureBody(Body: Buffer, ContentType: string | undefined, Total: number, Max: number): unknown {
    if(Total === 0){
        return null;
    }
    if(Total > Max || Body.length < Total){
        return { omitted: Total };
    }
    const Type = (ContentType ?? "").toLowerCase();
    const Text = Body.toString("utf8");
    if(Type.includes("json") || /^\s*[[{]/.test(Text)){
        try{
            return { json: RedactCaptureJson(JSON.parse(Text)) };
        }
        catch{ /* not JSON after all */ }
    }
    if(Type.includes("x-www-form-urlencoded")){
        const Form: Array<[string, string]> = [];
        for(const [Name, Value] of new URLSearchParams(Text)){
            Form.push([Name, RedactNamed(Name, Value)]);
        }
        return { form: Form };
    }
    if(!/[\x00-\x08\x0e-\x1f]/.test(Text) && Buffer.from(Text, "utf8").equals(Body)){
        return { text: RedactCaptureText(Text) };
    }
    return { binary: Total };
}

// A chat frame: a SASL PLAIN login (base64 of "\0<account>\0<token>") becomes <plain:<account>>, so the
// replay can log the same account in with a fresh token; a legacy password element is cut; the rest
// is redacted like any text.
export function RedactChatFrame(Frame: string): string {
    const Logins = Frame
        .replace(/(<auth\b[^>]*>)([^<]*)(<\/auth>)/g, (_, Open: string, Payload: string, Close: string) => {
            const Account = Buffer.from(Payload.trim(), "base64").toString("utf8").split("\0")[1];
            return Open + (Account !== undefined && /^[A-Za-z0-9_-]{1,64}$/.test(Account) ? `<plain:${Account}>` : "<redacted>") + Close;
        })
        .replace(/(<password\b[^>]*>)[^<]*(<\/password>)/g, "$1<redacted>$2");
    return RedactCaptureText(Logins);
}

// The request target with its secrets replaced (paths carry tokens on some routes).
export function RedactCaptureTarget(Target: string): string {
    return RedactCaptureText(Target);
}

// ---- Writing ----

// Appends records to <dir>/<source>-<yyyy-mm-dd>.jsonl (UTC days) and removes files older than the
// retention. A write error stops the capture with one warning; the service itself never fails for it.
export class CaptureWriter {
    private Day = "";
    private Stream: fs.WriteStream | undefined;
    private Broken = false;
    private readonly Secrets: string[] = [];

    // The service's own secrets (the gateway secret and the like) are cut from every line, wherever
    // an answer might echo them.
    AddSecret(Secret: string | undefined): void {
        if(Secret !== undefined && Secret.length >= 8){
            this.Secrets.push(JSON.stringify(Secret).slice(1, -1));
        }
    }

    constructor(readonly Options: CaptureOptions, private readonly Warn: (Message: string) => void = () => undefined,
        private readonly Now: () => number = Date.now){
        fs.mkdirSync(Options.dir, { recursive: true, mode: 0o700 });
        this.Prune();
    }

    Write(Record: Record<string, unknown>): void {
        if(this.Broken){
            return;
        }
        const Day = new Date(this.Now()).toISOString().slice(0, 10);
        if(Day !== this.Day){
            this.Stream?.end();
            this.Day = Day;
            const File = path.join(this.Options.dir, `${this.Options.source}-${Day}.jsonl`);
            this.Stream = fs.createWriteStream(File, { flags: "a", mode: 0o600 });
            this.Stream.on("error", (Error) => {
                this.Broken = true;
                this.Warn(`traffic capture stopped: ${Error.message}`);
            });
            this.Prune();
        }
        let Line = JSON.stringify({ t: new Date(this.Now()).toISOString(), src: this.Options.source, ...Record });
        for(const Secret of this.Secrets){
            Line = Line.split(Secret).join("<redacted>");
        }
        this.Stream!.write(Line + "\n");
    }

    Prune(): void {
        const Oldest = new Date(this.Now() - this.Options.retentionDays * 86_400_000).toISOString().slice(0, 10);
        let Files: string[] = [];
        try{
            Files = fs.readdirSync(this.Options.dir);
        }
        catch{
            return;
        }
        for(const File of Files){
            const Match = /^[a-z]+-(\d{4}-\d{2}-\d{2})\.jsonl$/.exec(File);
            if(Match !== null && Match[1] < Oldest){
                try{ fs.rmSync(path.join(this.Options.dir, File), { force: true }); } catch{ /* next time */ }
            }
        }
    }

    Close(): Promise<void> {
        return new Promise((resolve) => this.Stream === undefined ? resolve() : this.Stream.end(() => resolve()));
    }
}

// Collects up to Max bytes of a stream's chunks, counting the rest.
export class BodyTap {
    private readonly Chunks: Buffer[] = [];
    private Kept = 0;
    Total = 0;

    constructor(private readonly Max: number){}

    Add(Chunk: Buffer): void {
        this.Total += Chunk.length;
        if(this.Kept < this.Max){
            const Part = Chunk.subarray(0, this.Max - this.Kept);
            this.Chunks.push(Part);
            this.Kept += Part.length;
        }
    }

    Body(): Buffer {
        return Buffer.concat(this.Chunks);
    }
}
