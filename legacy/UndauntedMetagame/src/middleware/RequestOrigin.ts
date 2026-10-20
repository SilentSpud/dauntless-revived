import { NextFunction, Request, Response } from "express";
import crypto from "node:crypto";
import net from "node:net";
import os from "node:os";
import { logger } from "../logger";

// Where a request came from (roadmap 1.17, public mode).
//
// In public mode the metagame listens on 127.0.0.1 behind UndauntedGateway, the only public
// TCP listener. The gateway connects from 127.0.0.1, overwrites X-Forwarded-For with the
// player's real address and adds "X-Dauntless-Gateway: <GATEWAY_SECRET>". From that:
//
// - The player's address comes from X-Forwarded-For only when the connection is from loopback
//   AND carries the right secret. With GATEWAY_SECRET unset there is no gateway, and the
//   header is never trusted: the address is the socket's.
// - A request is "proxied" when it carries any forwarding header (the gateway's, or another
//   proxy's). Proxied requests may never use the admin key or the game-server key, whatever
//   the key: those only work from inside the host, never through the front door.
// - The game-server key also needs a caller on this machine: loopback, or a connection to
//   one of this machine's own addresses (in private mode the game servers call the
//   Tailscale address, and the connection then comes from that same address).
//   GAMESERVER_ALLOW_FROM (comma-separated IPs) adds hosts for a game server on another
//   machine, e.g. the Wine experiment in a WSL VM (roadmap 4.11).
//
// Nothing here changes a reply to a direct request: without forwarding headers everything
// answers as before, and GATEWAY_SECRET unset keeps the gateway handling off.

export const GATEWAY_HEADER = "x-dauntless-gateway";

// Any of these means some proxy relayed the request. The game servers, the host scripts
// and the game client never send them on their own.
const PROXY_HEADERS = [GATEWAY_HEADER, "x-forwarded-for", "forwarded", "x-real-ip", "x-forwarded-host", "x-forwarded-proto", "via"];

// 127.0.0.0/8 and ::1, also as IPv4-mapped IPv6
export function IsLoopbackAddress(Address: unknown){
    return typeof Address === "string" && (/^(::ffff:)?127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/i.test(Address) || Address === "::1");
}

// "::ffff:1.2.3.4" -> "1.2.3.4"; anything that is not an IP -> undefined
export function NormalizeAddress(Address: unknown){
    if(typeof Address !== "string"){
        return undefined;
    }

    const Plain = /^::ffff:(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})$/i.exec(Address)?.[1] ?? Address;

    return net.isIP(Plain) !== 0 ? Plain.toLowerCase() : undefined;
}

function GatewaySecret(){
    const Secret = process.env.GATEWAY_SECRET;

    return typeof Secret === "string" && Secret.length > 0 ? Secret : undefined;
}

function Digest(Value: string){
    return crypto.createHash("sha256").update(Value, "utf8").digest();
}

export function IsProxiedRequest(req: Request){
    return PROXY_HEADERS.some((Header) => req.headers[Header] !== undefined);
}

// From loopback and with the right X-Dauntless-Gateway secret
export function IsTrustedGatewayRequest(req: Request){
    const Secret = GatewaySecret();
    const Presented = req.headers[GATEWAY_HEADER];

    if(Secret == undefined || typeof Presented !== "string" || !IsLoopbackAddress(req.socket?.remoteAddress)){
        return false;
    }

    return crypto.timingSafeEqual(Digest(Presented), Digest(Secret));
}

// The gateway writes exactly one address; if more ever arrive, the right-most is the one
// the last hop (the gateway) wrote
function ForwardedAddress(req: Request){
    const Header = req.headers["x-forwarded-for"];
    const Value = Array.isArray(Header) ? Header.join(",") : Header;

    if(typeof Value !== "string"){
        return undefined;
    }

    const Entries = Value.split(",").map((Entry) => Entry.trim()).filter((Entry) => Entry.length > 0);

    return NormalizeAddress(Entries[Entries.length - 1]);
}

let LastBadForwardWarning = 0;

// The address to log and account the request to: the player's real address behind the
// gateway, else the peer of the TCP connection
export function ClientAddressOf(req: Request){
    const Peer = NormalizeAddress(req.socket?.remoteAddress) ?? "unknown";

    if(!IsTrustedGatewayRequest(req)){
        return Peer;
    }

    const Forwarded = ForwardedAddress(req);

    if(Forwarded == undefined){
        if(Date.now() - LastBadForwardWarning > 60 * 1000){
            LastBadForwardWarning = Date.now();
            logger.warn(`Gateway request without a usable X-Forwarded-For; using the connection's address`);
        }

        return Peer;
    }

    return Forwarded;
}

let OwnAddresses: { Addresses: Set<string>, ReadAt: number } | undefined;

// This machine's interface addresses, re-read at most every 30 s
function IsOwnAddress(Address: string){
    if(OwnAddresses == undefined || Date.now() - OwnAddresses.ReadAt > 30 * 1000){
        const Addresses = new Set<string>();

        for(const List of Object.values(os.networkInterfaces())){
            for(const Entry of List ?? []){
                const Normalized = NormalizeAddress(Entry.address);

                if(Normalized != undefined){
                    Addresses.add(Normalized);
                }
            }
        }

        OwnAddresses = { Addresses: Addresses, ReadAt: Date.now() };
    }

    return OwnAddresses.Addresses.has(Address);
}

function ExtraGameserverAddresses(){
    return new Set((process.env.GAMESERVER_ALLOW_FROM ?? "").split(",").map((Entry) => NormalizeAddress(Entry.trim())).filter((Entry): Entry is string => Entry != undefined));
}

// A caller on this machine (or listed in GAMESERVER_ALLOW_FROM) that came straight to the
// metagame, not through any proxy
export function IsDirectLocalRequest(req: Request){
    if(IsProxiedRequest(req)){
        return false;
    }

    const Remote = req.socket?.remoteAddress;

    if(IsLoopbackAddress(Remote)){
        return true;
    }

    const Peer = NormalizeAddress(Remote);

    if(Peer == undefined){
        return false;
    }

    return Peer === NormalizeAddress(req.socket?.localAddress) || IsOwnAddress(Peer) || ExtraGameserverAddresses().has(Peer);
}

// Answers 403 and returns true when this request may not present the game-server key
export function RefuseGameserverKeyFromOutside(req: Request, res: Response){
    if(IsDirectLocalRequest(req)){
        return false;
    }

    logger.warn(`Refusing the game-server key on ${req.method} ${req.path} from ${ClientAddressOf(req)}${IsProxiedRequest(req) ? " (relayed by a proxy)" : ""}: it is only accepted from this machine`);

    res.status(403);
    res.send();
    return true;
}

// Answers 403 and returns true when this request may not present the admin key: admin
// requests never go through the gateway (or any proxy); run them on the host itself
export function RefuseAdminKeyThroughProxy(req: Request, res: Response){
    if(!IsProxiedRequest(req)){
        return false;
    }

    logger.warn(`Refusing an admin request on ${req.method} ${req.path} from ${ClientAddressOf(req)}: admin keys are not accepted through a proxy`);

    res.status(403);
    res.send();
    return true;
}

// AUTH_MODE=NONE (development only, never with NODE_ENV=production) takes any account id as
// a login and as an API key. It must never answer anything that came through a proxy.
export function RefuseProxiedInDevAuthMode(req: Request, res: Response, next: NextFunction){
    if(process.env.AUTH_MODE === "NONE" && process.env.NODE_ENV !== "production" && IsProxiedRequest(req)){
        logger.warn(`Refusing proxied ${req.method} request: AUTH_MODE=NONE does not serve proxied requests`);

        res.status(403);
        res.send();
        return;
    }

    next();
}

let LastBadSecretWarning = 0;

// Persist only the origin category. Player addresses belong in expiring session memory.
export function DescribeOrigin(req: Request){
    if(!IsProxiedRequest(req)){
        return "";
    }

    if(IsTrustedGatewayRequest(req)){
        return " via=gateway";
    }

    if(req.headers[GATEWAY_HEADER] !== undefined && Date.now() - LastBadSecretWarning > 60 * 1000){
        LastBadSecretWarning = Date.now();
        logger.warn(`A request carried ${GATEWAY_HEADER} but ${GatewaySecret() == undefined ? "GATEWAY_SECRET is not set" : "not the right secret, or not from loopback"}; its forwarding headers are ignored`);
    }

    return " via=proxy";
}

// Startup check for public mode. Errors stop the server; warnings are logged.
export function CheckGatewayConfig(Env: NodeJS.ProcessEnv = process.env){
    const Errors: string[] = [];
    const Warnings: string[] = [];
    const Secret = Env.GATEWAY_SECRET;

    if(Secret === undefined || Secret.length === 0){
        return { Enabled: false, Errors, Warnings };
    }

    if(Env.AUTH_MODE !== "APIKEY"){
        Errors.push("GATEWAY_SECRET is set (public mode) but AUTH_MODE is not APIKEY; refusing to start");
    }

    if(Secret.length < 16){
        Warnings.push("GATEWAY_SECRET is shorter than 16 characters; use a long random value");
    }

    if(!IsLoopbackAddress(Env.BIND_HOST || "127.0.0.1")){
        Warnings.push("GATEWAY_SECRET is set but BIND_HOST is not loopback; in public mode the gateway should be the only way in");
    }

    if(Env.NODE_ENV !== "production"){
        Warnings.push("GATEWAY_SECRET is set but NODE_ENV is not production; error pages would include stack traces");
    }

    const Qos = Env.QOS_TARGET_URL ?? "";

    if(!/^http:\/\/127\.0\.0\.1:\d+\/QoS$/.test(Qos)){
        Warnings.push("GATEWAY_SECRET is set but QOS_TARGET_URL is not http://127.0.0.1:<port>/QoS; in public mode the client pings the launcher's local relay");
    }

    return { Enabled: true, Errors, Warnings };
}
