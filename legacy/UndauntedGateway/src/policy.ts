// What the gateway does with one request, decided from the method, the request target and the
// headers only (before any body is read). Pure functions, so every rule has a unit test.

export type Upstream = "metagame" | "content" | "ws";

// Which per-IP rate limit a request counts against. WebSocket upgrades (the game's chat connection)
// have their own bucket, so chat reconnects can never use up the one the game's HTTP traffic needs.
export type LimitClass = "general" | "content" | "register" | "token" | "ws";

// Requests whose 2xx answer proves the peer is a logged-in player: its address goes to the
// allowlist helper, which opens the UDP game ports for it.
export type FeedKind = "heartbeat" | "token";

export type Decision =
    | { action: "reject"; status: 400 | 403 | 405; error: string; reason: string; limitClass: LimitClass }
    | { action: "proxy"; upstream: Upstream; limitClass: LimitClass; feed?: FeedKind };

// The only /undaunted/api routes reachable from outside, and with which methods. Everything else
// under /undaunted/api is an admin route (invites, renames, rollbacks, entitlements, JWTs for any
// account) and answers 403 here, even with a valid admin key.
export const PUBLIC_UNDAUNTED_API: ReadonlyMap<string, readonly string[]> = new Map([
    ["register", ["POST"]],
    ["getuserinfo", ["GET", "HEAD"]],
    ["serverstatus", ["GET", "HEAD"]],
    ["huntregion", ["GET", "HEAD", "POST"]],
    ["devicesignals", ["POST"]],
    ["registrationstatus", ["GET", "HEAD"]],
]);

export const GAMESERVER_KEY_HEADER = "x-undaunted-gameserver-apikey";

const ALLOWED_METHODS = new Set(["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"]);

// The request target must be origin-form ("/path?query"). Absolute-form ("http://host/...") and
// "*" are refused so the routing below always sees the path the upstream will see.
export function SplitTarget(RawUrl: string | undefined): { path: string; query: string } | undefined {
    if(RawUrl === undefined || !RawUrl.startsWith("/")){
        return undefined;
    }
    const Question = RawUrl.indexOf("?");
    return Question === -1
        ? { path: RawUrl, query: "" }
        : { path: RawUrl.slice(0, Question), query: RawUrl.slice(Question + 1) };
}

// Paths the real client never sends and that could make the gateway and the upstream disagree
// about which route is meant. The client's own paths contain letters, digits, "-", "_", ".", ":"
// and "/" only (checked against every path in the metagame logs).
export function CheckPath(Path: string): "ok" | "bad_path" {
    if(/[\\\u0000-\s]/.test(Path)){
        return "bad_path";
    }
    if(/%(2f|5c|00|2e)/i.test(Path)){
        return "bad_path";
    }
    for(const Segment of Path.split("/")){
        if(Segment === "." || Segment === ".."){
            return "bad_path";
        }
    }
    return "ok";
}

// The path as a lenient router would match it: percent-decoded, lower case, repeated slashes
// folded, trailing slashes dropped. Express matches routes case-insensitively and with an
// optional trailing slash, so the admin-route check must be at least that lenient.
export function CanonicalPath(Path: string): string | undefined {
    let Decoded: string;
    try{
        Decoded = decodeURIComponent(Path);
    }
    catch{
        return undefined;
    }
    let Out = Decoded.toLowerCase().replace(/\/{2,}/g, "/");
    while(Out.length > 1 && Out.endsWith("/")){
        Out = Out.slice(0, -1);
    }
    return Out;
}

function Reject(status: 400 | 403 | 405, error: string, reason: string, limitClass: LimitClass): Decision {
    return { action: "reject", status, error, reason, limitClass };
}

export function Classify(Method: string | undefined, RawUrl: string | undefined, Headers: Record<string, string | string[] | undefined>, IsUpgrade: boolean): Decision {
    const Verb = (Method ?? "").toUpperCase();

    const Target = SplitTarget(RawUrl);
    if(Target === undefined){
        return Reject(400, "bad_request", "target_not_origin_form", "general");
    }
    if(CheckPath(Target.path) !== "ok"){
        return Reject(400, "bad_path", "bad_path", "general");
    }
    const Canonical = CanonicalPath(Target.path);
    if(Canonical === undefined){
        return Reject(400, "bad_path", "bad_percent_encoding", "general");
    }

    const LimitClass: LimitClass = IsUpgrade ? "ws"
        : Canonical === "/undaunted/api/register" ? "register"
        : Canonical === "/account/api/oauth/token" ? "token"
        : Canonical === "/content" || Canonical.startsWith("/content/") ? "content"
        : "general";

    if(!ALLOWED_METHODS.has(Verb)){
        return Reject(405, "method_not_allowed", "method", LimitClass);
    }

    if(Headers[GAMESERVER_KEY_HEADER] !== undefined){
        return Reject(403, "forbidden", "gameserver_key_header", LimitClass);
    }

    if(Canonical === "/undaunted" || Canonical.startsWith("/undaunted/")){
        const Match = /^\/undaunted\/api\/([^/]+)$/.exec(Canonical);
        const Methods = Match === null ? undefined : PUBLIC_UNDAUNTED_API.get(Match[1]);
        if(IsUpgrade || Methods === undefined || !Methods.includes(Verb)){
            return Reject(403, "forbidden", "admin_route", LimitClass);
        }
        return { action: "proxy", upstream: "metagame", limitClass: LimitClass };
    }

    if(IsUpgrade){
        const Upgrade = String(Headers["upgrade"] ?? "").toLowerCase();
        if(Verb !== "GET" || Upgrade !== "websocket"){
            return Reject(400, "bad_request", "unsupported_upgrade", LimitClass);
        }
        return { action: "proxy", upstream: "ws", limitClass: LimitClass };
    }

    if(LimitClass === "content"){
        return { action: "proxy", upstream: "content", limitClass: LimitClass };
    }

    // /game/login returns 2xx only after bearer validation and account lookup.
    // Reuse the bearer-required feed so travel need not wait for a heartbeat.
    if(Verb === "POST" && (Canonical === "/heartbeat" || Canonical === "/game/login")){
        return { action: "proxy", upstream: "metagame", limitClass: LimitClass, feed: "heartbeat" };
    }
    if(Verb === "POST" && Canonical === "/account/api/oauth/token"){
        return { action: "proxy", upstream: "metagame", limitClass: LimitClass, feed: "token" };
    }

    return { action: "proxy", upstream: "metagame", limitClass: LimitClass };
}

// A heartbeat only proves a login when it carried the player's bearer token (the upstream
// answers 401 without one; this is a second check in case that ever changes).
export function HasBearer(Headers: Record<string, string | string[] | undefined>): boolean {
    const Value = Headers["authorization"];
    return typeof Value === "string" && /^bearer\s+\S+/i.test(Value);
}
