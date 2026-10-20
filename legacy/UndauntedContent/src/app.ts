import fs from "node:fs";
import http from "node:http";
import { createHash } from "node:crypto";
import { pipeline } from "node:stream";
import { AuthCache } from "./auth";
import { Branding, BRANDING_PREFIX, IsSafeImageName } from "./branding";
import { FileState } from "./gamedir";
import { StreamLimiter } from "./limits";
import { logger } from "./log";
import { Manifest, ManifestFile, OnDiskPath } from "./manifest";
import { News } from "./news";
import { FILES_PREFIX, ParseFileRequestPath } from "./paths";
import { IfNoneMatchHits, IfRangeAllowsPartial, ParseRange } from "./range";

// Routes (all GET or HEAD):
//   /content/v1/manifest           the file list, public inside the tailnet (it is in the launcher's source anyway)
//   /content/v1/files/<path>       one game file; needs x-undaunted-user-api-key; single HTTP range
//   /content/v1/branding           the host's art pack: background image URLs and an accent colour
//   /content/v1/branding/<file>    one of those images
//   /content/v1/news               the host's news items
// Anything else is 404; there is no directory listing anywhere.

export type ContentDeps = {
    downloadBaseUrl?: string;
    manifest: Manifest;
    index: Map<string, ManifestFile>;
    gameDir: string;
    states: Map<string, FileState>;
    auth: AuthCache;
    limiter: StreamLimiter;
    branding: Branding;
    news: News;
};

const READ_CHUNK = 1024 * 1024;
const MAX_BRANDING_BYTES = 25 * 1024 * 1024;

type RequestLog = {
    route: string;
    path?: string;
    reason?: string;
    account?: string;
    user?: string;
    range?: string;
    bytes?: number;
    expected?: number;
};

function Base(res: http.ServerResponse){
    res.setHeader("X-Content-Type-Options", "nosniff");
}

// For the public, key-free JSON and images, so the launcher's window can read them directly.
function Public(res: http.ServerResponse){
    res.setHeader("Access-Control-Allow-Origin", "*");
    res.setHeader("Cross-Origin-Resource-Policy", "cross-origin");
}

function SendJson(req: http.IncomingMessage, res: http.ServerResponse, Status: number, Body: unknown, Headers: Record<string, string> = {}){
    const Text = JSON.stringify(Body);
    res.statusCode = Status;
    res.setHeader("Content-Type", "application/json; charset=utf-8");
    res.setHeader("Content-Length", Buffer.byteLength(Text));
    res.setHeader("Cache-Control", "no-cache");
    for(const [Name, Value] of Object.entries(Headers)){
        res.setHeader(Name, Value);
    }
    res.end(req.method === "HEAD" ? undefined : Text);
}

function SendError(req: http.IncomingMessage, res: http.ServerResponse, Status: number, Code: string, Headers: Record<string, string> = {}){
    SendJson(req, res, Status, { error: Code }, Headers);
}

function StripQuery(Target: string): string {
    const At = Target.indexOf("?");
    return At === -1 ? Target : Target.slice(0, At);
}

export function CreateContentHandler(Deps: ContentDeps): http.RequestListener {
    const ManifestText = JSON.stringify({ ...Deps.manifest, downloadBaseUrl: Deps.downloadBaseUrl });
    const ManifestETag = `"${createHash("sha256").update(ManifestText).digest("hex")}"`;

    return (req, res) => {
        const Started = Date.now();
        const Target = req.url ?? "/";
        const PathOnly = StripQuery(Target);
        const Info: RequestLog = { route: "other" };
        let ClientGone = false;

        Base(res);

        res.on("close", () => {
            ClientGone = true;
            const Fields: Record<string, unknown> = {
                method: req.method,
                route: Info.route,
                path: Info.path ?? PathOnly.slice(0, 200),
                status: res.statusCode,
                ms: Date.now() - Started,
                ip: req.socket.remoteAddress,
            };
            if(Info.reason !== undefined) Fields.reason = Info.reason;
            if(Info.account !== undefined) Fields.account = Info.account;
            if(Info.user !== undefined) Fields.user = Info.user;
            if(Info.range !== undefined) Fields.range = Info.range;
            if(Info.bytes !== undefined){
                Fields.bytes = Info.bytes;
                Fields.expected = Info.expected;
                Fields.complete = Info.bytes === Info.expected && res.writableFinished;
            }
            const Level = res.statusCode >= 500 ? "warn" : (res.statusCode === 400 || res.statusCode === 401) ? "warn" : "info";
            logger[Level](Info.route === "files" && req.method === "GET" && Info.bytes !== undefined ? "download" : "request", Fields);
        });

        const Method = req.method ?? "GET";
        const IsKnownRoute =
            PathOnly === "/content/v1/manifest" || PathOnly.startsWith(FILES_PREFIX) ||
            PathOnly === "/content/v1/branding" || PathOnly.startsWith(BRANDING_PREFIX) ||
            PathOnly === "/content/v1/news";

        if(!IsKnownRoute){
            SendError(req, res, 404, "not_found");
            return;
        }

        if(Method !== "GET" && Method !== "HEAD"){
            SendError(req, res, 405, "method_not_allowed", { "Allow": "GET, HEAD" });
            return;
        }

        if(PathOnly === "/content/v1/manifest"){
            Info.route = "manifest";
            Public(res);
            if(IfNoneMatchHits(req.headers["if-none-match"], ManifestETag)){
                res.statusCode = 304;
                res.setHeader("ETag", ManifestETag);
                res.end();
                return;
            }
            res.statusCode = 200;
            res.setHeader("Content-Type", "application/json; charset=utf-8");
            res.setHeader("Content-Length", Buffer.byteLength(ManifestText));
            res.setHeader("Cache-Control", "no-cache");
            res.setHeader("ETag", ManifestETag);
            res.end(Method === "HEAD" ? undefined : ManifestText);
            return;
        }

        if(PathOnly === "/content/v1/branding"){
            Info.route = "branding";
            Public(res);
            SendJson(req, res, 200, Deps.branding.body());
            return;
        }

        if(PathOnly === "/content/v1/news"){
            Info.route = "news";
            Public(res);
            SendJson(req, res, 200, Deps.news.body());
            return;
        }

        if(PathOnly.startsWith(BRANDING_PREFIX)){
            Info.route = "branding-image";
            Public(res);
            ServeBrandingImage(Deps, req, res, PathOnly.slice(BRANDING_PREFIX.length), Info);
            return;
        }

        Info.route = "files";
        ServeGameFile(Deps, req, res, Target, Info, () => ClientGone).catch((error: unknown) => {
            logger.error("file request failed", { error: error instanceof Error ? error.message : String(error) });
            if(!res.headersSent){
                SendError(req, res, 500, "internal_error");
            }
            else{
                res.destroy();
            }
        });
    };
}

function ServeBrandingImage(Deps: ContentDeps, req: http.IncomingMessage, res: http.ServerResponse, Name: string, Info: RequestLog){
    if(!IsSafeImageName(Name)){
        SendError(req, res, 404, "not_found");
        return;
    }

    const Found = Deps.branding.image(Name);
    if(Found === undefined){
        SendError(req, res, 404, "not_found");
        return;
    }

    Info.path = Name;

    fs.promises.open(Found.fullPath, "r").then(async (Handle) => {
        let Stat: fs.Stats;
        try{
            Stat = await Handle.stat();
        }
        catch{
            await Handle.close().catch(() => undefined);
            SendError(req, res, 404, "not_found");
            return;
        }

        if(!Stat.isFile() || Stat.size === 0 || Stat.size > MAX_BRANDING_BYTES){
            await Handle.close().catch(() => undefined);
            SendError(req, res, 404, "not_found");
            return;
        }

        const ETag = `W/"${Stat.size.toString(16)}-${Math.floor(Stat.mtimeMs).toString(16)}"`;
        res.setHeader("ETag", ETag);
        res.setHeader("Cache-Control", "public, max-age=300");

        if(IfNoneMatchHits(req.headers["if-none-match"], ETag.replace(/^W\//, ""))){
            await Handle.close().catch(() => undefined);
            res.statusCode = 304;
            res.end();
            return;
        }

        res.statusCode = 200;
        res.setHeader("Content-Type", Found.image.contentType);
        res.setHeader("Content-Length", Stat.size);

        if(req.method === "HEAD"){
            await Handle.close().catch(() => undefined);
            res.end();
            return;
        }

        const Stream = Handle.createReadStream({ start: 0, end: Stat.size - 1, highWaterMark: 256 * 1024 });
        pipeline(Stream, res, () => undefined);
    }, () => {
        SendError(req, res, 404, "not_found");
    }).catch((error: unknown) => {
        logger.warn("branding image failed", { file: Name, error: error instanceof Error ? error.message : String(error) });
        res.destroy();
    });
}

async function ServeGameFile(Deps: ContentDeps, req: http.IncomingMessage, res: http.ServerResponse, Target: string, Info: RequestLog, IsClientGone: () => boolean){
    const Parsed = ParseFileRequestPath(Target);
    if(!Parsed.ok){
        Info.reason = Parsed.reason;
        SendError(req, res, 400, "bad_path");
        return;
    }

    const Entry = Deps.index.get(Parsed.path);
    if(Entry === undefined){
        Info.reason = "not_in_manifest";
        SendError(req, res, 404, "not_found");
        return;
    }
    Info.path = Entry.path;

    const Key = req.headers["x-undaunted-user-api-key"];
    if(typeof Key !== "string" || Key.length === 0){
        Info.reason = "no_key";
        SendError(req, res, 401, "unauthorized");
        return;
    }

    const Auth = await Deps.auth.check(Key);
    if(Auth.status === "unavailable"){
        Info.reason = Auth.reason;
        SendError(req, res, 503, "auth_unavailable", { "Retry-After": "5" });
        return;
    }
    if(Auth.status === "invalid"){
        Info.reason = "bad_key";
        SendError(req, res, 401, "unauthorized");
        return;
    }
    Info.account = Auth.account.UserId;
    Info.user = Auth.account.Username;

    const State = Deps.states.get(Entry.path);
    if(State === undefined || State.status !== "ok"){
        Info.reason = `file_${State?.status ?? "unknown"}`;
        SendError(req, res, 503, "file_unavailable");
        return;
    }

    const ETag = `"${Entry.sha256}"`;
    res.setHeader("ETag", ETag);
    res.setHeader("Accept-Ranges", "bytes");
    res.setHeader("Cache-Control", "private, no-transform");

    if(IfNoneMatchHits(req.headers["if-none-match"], ETag)){
        res.statusCode = 304;
        res.end();
        return;
    }

    // A stale If-Range means the client's partial copy is of something else: send it all.
    const IfRange = req.headers["if-range"];
    const RangeHeader = IfRangeAllowsPartial(Array.isArray(IfRange) ? "" : IfRange, ETag) ? req.headers.range : undefined;
    const Range = ParseRange(RangeHeader, Entry.size);
    if(RangeHeader !== undefined){
        Info.range = RangeHeader.slice(0, 64);
    }

    if(Range.kind === "unsatisfiable"){
        Info.reason = "bad_range";
        SendError(req, res, 416, "range_not_satisfiable", { "Content-Range": `bytes */${Entry.size}` });
        return;
    }

    const Start = Range.kind === "partial" ? Range.start : 0;
    const End = Range.kind === "partial" ? Range.end : Entry.size - 1;
    const Length = End - Start + 1;

    res.statusCode = Range.kind === "partial" ? 206 : 200;
    res.setHeader("Content-Type", "application/octet-stream");
    res.setHeader("Content-Length", Math.max(0, Length));
    if(Range.kind === "partial"){
        res.setHeader("Content-Range", `bytes ${Start}-${End}/${Entry.size}`);
    }

    if(req.method === "HEAD"){
        res.end();
        return;
    }

    const Slot = Deps.limiter.tryAcquire(Auth.account.UserId);
    if(!Slot.ok){
        Info.reason = Slot.reason;
        res.removeHeader("Content-Range");
        res.removeHeader("ETag");
        res.removeHeader("Accept-Ranges");
        if(Slot.reason === "account_limit"){
            SendError(req, res, 429, "too_many_streams", { "Retry-After": "2" });
        }
        else{
            SendError(req, res, 503, "server_busy", { "Retry-After": "10" });
        }
        return;
    }

    if(IsClientGone()){
        Slot.release();
        return;
    }
    res.on("close", Slot.release);

    let Handle: fs.promises.FileHandle;
    try{
        Handle = await fs.promises.open(OnDiskPath(Deps.gameDir, Entry), "r");
    }
    catch(error){
        Deps.states.set(Entry.path, { status: "missing" });
        Info.reason = "open_failed";
        logger.warn("game file could not be opened; no longer serving it", { path: Entry.path, error: error instanceof Error ? error.message : String(error) });
        ResetForError(res);
        SendError(req, res, 503, "file_unavailable");
        return;
    }

    let Stat: fs.Stats;
    try{
        Stat = await Handle.stat();
    }
    catch{
        await Handle.close().catch(() => undefined);
        ResetForError(res);
        SendError(req, res, 503, "file_unavailable");
        return;
    }

    // Changed on disk since the startup check: stop serving it until the host verifies and restarts.
    if(!Stat.isFile() || Stat.size !== Entry.size || Stat.mtimeMs !== State.mtimeMs){
        await Handle.close().catch(() => undefined);
        Deps.states.set(Entry.path, { status: "changed" });
        Info.reason = "changed_on_disk";
        logger.warn("game file changed since startup; no longer serving it (run npm run verify, then restart)", { path: Entry.path });
        ResetForError(res);
        SendError(req, res, 503, "file_unavailable");
        return;
    }

    Info.bytes = 0;
    Info.expected = Math.max(0, Length);

    if(IsClientGone()){
        await Handle.close().catch(() => undefined);
        return;
    }

    if(Length <= 0){
        await Handle.close().catch(() => undefined);
        res.end();
        return;
    }

    const Stream = Handle.createReadStream({ start: Start, end: End, highWaterMark: READ_CHUNK });
    let Sent = 0;
    Stream.on("data", (Chunk: Buffer | string) => {
        Sent += Chunk.length;
        Info.bytes = Sent;
    });
    pipeline(Stream, res, (error) => {
        if(error && (error as NodeJS.ErrnoException).code !== "ERR_STREAM_PREMATURE_CLOSE"){
            logger.warn("download stream error", { path: Entry.path, error: error.message });
        }
    });
}

function ResetForError(res: http.ServerResponse){
    for(const Name of ["Content-Range", "ETag", "Accept-Ranges", "Content-Type", "Content-Length"]){
        res.removeHeader(Name);
    }
}
