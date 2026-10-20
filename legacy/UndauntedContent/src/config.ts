import net from "node:net";
import path from "node:path";
import { DefaultManifestPath } from "./manifest";

export type Config = {
    port: number;
    bindHosts: string[];
    allowAnyBind: boolean;
    metagameUrl: string;
    gameDir: string;
    manifestPath: string;
    downloadBaseUrl?: string;
    brandingDir: string | undefined;
    newsFile: string | undefined;
    maxStreamsPerAccount: number;
    maxStreamsTotal: number;
    authCacheSeconds: number;
};

function Int(Env: NodeJS.ProcessEnv, Name: string, Default: number, Min: number, Max: number): number {
    const Raw = Env[Name];
    if(Raw === undefined || Raw.trim() === ""){
        return Default;
    }
    const Value = Number(Raw);
    if(!Number.isInteger(Value) || Value < Min || Value > Max){
        throw new Error(`${Name} must be a whole number from ${Min} to ${Max}`);
    }
    return Value;
}

function Optional(Env: NodeJS.ProcessEnv, Name: string): string | undefined {
    const Raw = Env[Name];
    return Raw === undefined || Raw.trim() === "" ? undefined : Raw.trim();
}

// Addresses a friend can only reach through the VPN: loopback, and Tailscale's own ranges
// (100.64.0.0/10 for IPv4, fd7a:115c:a1e0::/48 for IPv6). Anything else, 0.0.0.0 included,
// needs CONTENT_ALLOW_ANY_BIND=1: game files must never be reachable from the internet.
export function IsPrivateBindHost(Host: string): boolean {
    if(Host === "localhost"){
        return true;
    }

    if(net.isIPv4(Host)){
        const [A, B] = Host.split(".").map(Number);
        return A === 127 || (A === 100 && B >= 64 && B <= 127);
    }

    if(net.isIPv6(Host)){
        const Lower = Host.toLowerCase();
        return Lower === "::1" || /^fd7a:115c:a1e0:/.test(Lower);
    }

    return false;
}

export function LoadConfig(Env: NodeJS.ProcessEnv = process.env): Config {
    const Port = Int(Env, "PORT", 61002, 1, 65535);

    const BindHosts = (Optional(Env, "BIND_HOST") ?? "127.0.0.1").split(",").map((Host) => Host.trim()).filter((Host) => Host.length > 0);
    if(BindHosts.length === 0){
        throw new Error("BIND_HOST is empty");
    }
    for(const Host of BindHosts){
        if(Host !== "localhost" && net.isIP(Host) === 0){
            throw new Error(`BIND_HOST entries must be IP addresses (got ${JSON.stringify(Host)})`);
        }
    }

    const AllowAnyBind = Env.CONTENT_ALLOW_ANY_BIND === "1";
    if(!AllowAnyBind){
        const Public = BindHosts.filter((Host) => !IsPrivateBindHost(Host));
        if(Public.length > 0){
            throw new Error(`Refusing to listen on ${Public.join(", ")}: only loopback and Tailscale addresses (100.64.0.0/10) are allowed. Set CONTENT_ALLOW_ANY_BIND=1 only if a firewall keeps the internet out.`);
        }
    }

    const MetagameUrl = (Optional(Env, "METAGAME_URL") ?? "http://127.0.0.1:61000").replace(/\/+$/, "");
    let Parsed: URL;
    try{
        Parsed = new URL(MetagameUrl);
    }
    catch{
        throw new Error("METAGAME_URL is not a URL");
    }
    if(Parsed.protocol !== "http:" && Parsed.protocol !== "https:"){
        throw new Error("METAGAME_URL must be http:// or https://");
    }

    const GameDir = Optional(Env, "CONTENT_GAME_DIR");
    if(GameDir === undefined){
        throw new Error("CONTENT_GAME_DIR is not set (the folder that contains Archon\\)");
    }

    const DownloadBase = Optional(Env, "CONTENT_DOWNLOAD_BASE_URL");
    let DownloadBaseUrl: string | undefined;
    if(DownloadBase){
        const Url = new URL(DownloadBase);
        if(Url.protocol !== "https:" || Url.username || Url.password || Url.search || Url.hash){
            throw new Error("CONTENT_DOWNLOAD_BASE_URL must be HTTPS without credentials, query or fragment");
        }
        DownloadBaseUrl = Url.href.replace(/\/+$/, "") + "/";
    }

    return {
        port: Port,
        bindHosts: BindHosts,
        allowAnyBind: AllowAnyBind,
        metagameUrl: MetagameUrl,
        gameDir: path.resolve(GameDir),
        manifestPath: path.resolve(Optional(Env, "CONTENT_MANIFEST") ?? DefaultManifestPath()),
        downloadBaseUrl: DownloadBaseUrl,
        brandingDir: Optional(Env, "CONTENT_BRANDING_DIR") === undefined ? undefined : path.resolve(Optional(Env, "CONTENT_BRANDING_DIR")!),
        newsFile: Optional(Env, "CONTENT_NEWS_FILE") === undefined ? undefined : path.resolve(Optional(Env, "CONTENT_NEWS_FILE")!),
        maxStreamsPerAccount: Int(Env, "CONTENT_MAX_STREAMS_PER_ACCOUNT", 6, 1, 64),
        maxStreamsTotal: Int(Env, "CONTENT_MAX_STREAMS_TOTAL", 48, 1, 1024),
        authCacheSeconds: Int(Env, "CONTENT_AUTH_CACHE_SECONDS", 300, 0, 3600),
    };
}
