// The local relay for public mode (contract C4).
//
// The 1.4.4 client only speaks plain HTTP, so in public mode it is pointed at this relay on
// http://127.0.0.1:61000 on the friend's own PC. Every request (and later the chat WebSocket) is
// forwarded over TLS to the server's gateway, on a connection pinned to the certificate fingerprint
// from the invite. Nothing leaves the PC unencrypted, and nothing is sent to a server whose
// certificate does not match.
//
// The relay runs only while the game runs. It listens on 127.0.0.1 only and refuses requests that
// look like they come from a web browser (a page could otherwise use it through DNS rebinding).

import http from "node:http";
import net from "node:net";
import type { Duplex } from "node:stream";
import { isPinError, PinnedAgent, pinnedCreateConnection, type PinnedTarget } from "./pinned";

export const DEFAULT_RELAY_PORT = 61000;

// Headers that belong to one connection and are never forwarded (RFC 9110 7.6.1), plus the
// proxy-specific ones. Anything named in a Connection header is dropped as well.
const HOP_BY_HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-connection",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "http2-settings",
]);

export class RelayError extends Error {
  constructor(
    public readonly code: "port_busy" | "listen_failed",
    message: string,
  ) {
    super(message);
    this.name = "RelayError";
  }
}

export type RelayLogLevel = "info" | "warn" | "error";

export interface RelayOptions {
  target: PinnedTarget;
  port?: number;
  connectTimeoutMs?: number;
  maxSockets?: number;
  onPinError?: (actual: string | null) => void;
  onLog?: (level: RelayLogLevel, message: string) => void;
  authorizeConnection?: (socket: net.Socket) => Promise<boolean>;
}

// Filters a rawHeaders list ([name, value, name, value, ...]): drops hop-by-hop headers and Host.
// keepUpgrade keeps the Upgrade header and sends "Connection: Upgrade" for a WebSocket handshake.
export function filterHeaders(raw: readonly string[], opts: { keepUpgrade?: boolean; host?: string } = {}): string[] {
  const named = new Set<string>();
  for (let i = 0; i + 1 < raw.length; i += 2) {
    if (raw[i].toLowerCase() === "connection") {
      for (const token of raw[i + 1].split(",")) named.add(token.trim().toLowerCase());
    }
  }
  const out: string[] = [];
  if (opts.host !== undefined) out.push("Host", opts.host);
  for (let i = 0; i + 1 < raw.length; i += 2) {
    const name = raw[i];
    const lower = name.toLowerCase();
    if (lower === "host") continue;
    if (opts.keepUpgrade && lower === "upgrade") {
      out.push(name, raw[i + 1]);
      continue;
    }
    if (HOP_BY_HOP.has(lower) || named.has(lower)) continue;
    out.push(name, raw[i + 1]);
  }
  if (opts.keepUpgrade) out.push("Connection", "Upgrade");
  return out;
}

function isLoopbackAddress(addr: string | undefined): boolean {
  if (!addr) return false;
  return addr === "::1" || /^(::ffff:)?127\.\d+\.\d+\.\d+$/.test(addr);
}

function isLoopbackName(name: string): boolean {
  const n = name.toLowerCase();
  return n === "localhost" || n === "::1" || /^127\.\d+\.\d+\.\d+$/.test(n);
}

// "127.0.0.1:61000", "localhost", "[::1]:61000" -> the host part
function hostPart(value: string): string {
  const v = value.trim();
  const bracket = /^\[([^\]]+)\](?::\d+)?$/.exec(v);
  if (bracket) return bracket[1];
  return v.replace(/:\d+$/, "");
}

// Why a request must be refused, or null. The game sends Host 127.0.0.1:<port> and no
// browser headers. A web page cannot avoid sending Sec-Fetch-* on plain requests, and a browser
// WebSocket always carries the page's Origin; DNS rebinding shows up as a foreign Host.
export function localRequestProblem(req: http.IncomingMessage, upgrade = false): string | null {
  if (!isLoopbackAddress(req.socket.remoteAddress)) return "not_local";
  const host = req.headers.host;
  if (typeof host !== "string" || !isLoopbackName(hostPart(host))) return "host";
  if (!upgrade) for (const name of Object.keys(req.headers)) if (name.startsWith("sec-fetch-")) return "browser";
  const origin = req.headers.origin;
  if (origin !== undefined) {
    const m = /^(?:[a-z][a-z0-9+.-]*:\/\/)?(\[[^\]]+\]|[^/:]+)(?::\d+)?\/?$/i.exec(String(origin).trim());
    if (!m || !isLoopbackName(m[1].replace(/^\[|\]$/g, ""))) return "origin";
  }
  return null;
}

function sendError(res: http.ServerResponse, status: number, code: string): void {
  if (res.headersSent) {
    res.destroy();
    return;
  }
  const body = JSON.stringify({ error: code });
  res.writeHead(status, { "content-type": "application/json", "content-length": Buffer.byteLength(body), "cache-control": "no-store" });
  res.end(body);
}

function statusLine(code: number, message: string): string {
  return `HTTP/1.1 ${code} ${message.replace(/[\r\n]/g, "")}\r\n`;
}

export class Relay {
  private server: http.Server | null = null;
  private readonly agent: PinnedAgent;
  private readonly tunnels = new Set<Duplex>();
  private readonly authorized = new WeakMap<net.Socket, Promise<boolean>>();
  private readonly hostHeader: string;
  private listeningPort: number | null = null;
  requests = 0;
  upgrades = 0;
  failures = 0;

  constructor(private readonly opts: RelayOptions) {
    const t = opts.target;
    this.hostHeader = t.port === 443 ? t.host : `${t.host}:${t.port}`;
    this.agent = new PinnedAgent(
      t,
      { keepAlive: true, maxSockets: opts.maxSockets ?? 32, maxFreeSockets: 8, scheduling: "lifo" },
      opts.connectTimeoutMs ?? 10000,
    );
  }

  get port(): number | null {
    return this.listeningPort;
  }

  get running(): boolean {
    return this.server !== null;
  }

  private log(level: RelayLogLevel, message: string): void {
    this.opts.onLog?.(level, message);
  }

  start(): Promise<void> {
    if (this.server) return Promise.resolve();
    const port = this.opts.port ?? DEFAULT_RELAY_PORT;
    const server = http.createServer({ keepAliveTimeout: 60000, maxHeaderSize: 64 * 1024 });
    server.on("request", (req, res) => {
      void this.authorize(req.socket).then((allowed) => {
        if (req.socket.destroyed) return;
        if (allowed) this.handleRequest(req, res);
        else {
          req.resume();
          res.setHeader("Connection", "close");
          sendError(res, 403, "local_process_denied");
        }
      });
    });
    server.on("upgrade", (req, socket, head) => {
      void this.authorize(req.socket).then((allowed) => {
        if (socket.destroyed) return;
        if (allowed) this.handleUpgrade(req, socket, head);
        else socket.end("HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
      });
    });
    return new Promise((resolve, reject) => {
      const onError = (e: NodeJS.ErrnoException) => {
        server.removeAllListeners();
        if (e.code === "EADDRINUSE" || e.code === "EACCES") reject(new RelayError("port_busy", `port ${port} is in use`));
        else reject(new RelayError("listen_failed", e.message));
      };
      server.once("error", onError);
      server.listen({ port, host: "127.0.0.1", exclusive: true }, () => {
        server.removeListener("error", onError);
        server.on("error", (e) => this.log("error", `relay server error: ${e.message}`));
        this.server = server;
        this.listeningPort = (server.address() as net.AddressInfo).port;
        this.log("info", `relay listening on 127.0.0.1:${this.listeningPort} for ${this.hostHeader}`);
        resolve();
      });
    });
  }

  private authorize(socket: net.Socket): Promise<boolean> {
    if (!this.opts.authorizeConnection) return Promise.resolve(true);
    let verdict = this.authorized.get(socket);
    if (!verdict) {
      verdict = Promise.resolve().then(() => this.opts.authorizeConnection!(socket)).catch(() => false)
        .then((allowed) => {
          if (!allowed) this.log("warn", "local connection rejected: game process ownership could not be verified");
          return allowed;
        });
      this.authorized.set(socket, verdict);
    }
    return verdict;
  }

  stop(): Promise<void> {
    const server = this.server;
    this.server = null;
    this.listeningPort = null;
    for (const t of this.tunnels) t.destroy();
    this.tunnels.clear();
    this.agent.destroy();
    if (!server) return Promise.resolve();
    return new Promise((resolve) => {
      server.close(() => resolve());
      server.closeAllConnections();
    });
  }

  private pinFailed(e: unknown): void {
    if (isPinError(e)) {
      this.log("error", "the server's certificate does not match the invite; request refused");
      this.opts.onPinError?.(e.actual);
    }
  }

  // ---------------------------------------------------------------- plain requests

  private handleRequest(req: http.IncomingMessage, res: http.ServerResponse): void {
    this.requests++;
    const problem = localRequestProblem(req);
    if (problem) {
      this.log("warn", `refused a local request (${problem})`);
      req.resume();
      return sendError(res, 403, "forbidden");
    }
    const te = req.headers["transfer-encoding"];
    if (te !== undefined && te.toLowerCase() !== "chunked") {
      req.resume();
      return sendError(res, 501, "unsupported_transfer_encoding");
    }
    const headers = filterHeaders(req.rawHeaders, { host: this.hostHeader });
    if (te !== undefined) headers.push("Transfer-Encoding", "chunked");
    const bodiless = te === undefined && (req.headers["content-length"] === undefined || req.headers["content-length"] === "0");
    this.forward(req, res, headers, bodiless, 0);
  }

  private forward(req: http.IncomingMessage, res: http.ServerResponse, headers: string[], bodiless: boolean, attempt: number): void {
    const t = this.opts.target;
    const up = http.request({
      host: t.host,
      port: t.port,
      method: req.method,
      path: req.url,
      headers,
      agent: this.agent,
      setHost: false,
    });
    let answered = false;
    up.on("response", (ures) => {
      answered = true;
      res.writeHead(ures.statusCode ?? 502, ures.statusMessage, filterHeaders(ures.rawHeaders));
      ures.pipe(res);
      ures.on("error", () => res.destroy());
      ures.on("close", () => {
        if (!ures.complete) res.destroy();
      });
    });
    up.on("error", (e: NodeJS.ErrnoException) => {
      if (answered) {
        res.destroy();
        return;
      }
      // A kept-alive connection that the gateway closed at the same moment: try a bodiless
      // request once more on a fresh connection.
      if (bodiless && attempt === 0 && up.reusedSocket && (e.code === "ECONNRESET" || e.code === "EPIPE") && !res.headersSent) {
        this.forward(req, res, headers, bodiless, 1);
        return;
      }
      this.failures++;
      this.pinFailed(e);
      if (!isPinError(e)) this.log("warn", `upstream request failed: ${e.code ?? e.message}`);
      sendError(res, 502, isPinError(e) ? "certificate_mismatch" : "upstream_unreachable");
    });
    res.on("close", () => {
      if (!res.writableFinished) up.destroy();
    });
    if (attempt === 0) {
      req.on("error", () => up.destroy());
      if (bodiless) {
        req.resume();
        up.end();
      } else {
        req.pipe(up);
      }
    } else {
      up.end();
    }
  }

  // ---------------------------------------------------------------- WebSocket (and other) upgrades

  private handleUpgrade(req: http.IncomingMessage, socket: Duplex, head: Buffer): void {
    this.upgrades++;
    socket.on("error", () => undefined);
    const problem = localRequestProblem(req, true);
    if (problem) {
      this.log("warn", `refused a local upgrade (${problem})`);
      socket.end(statusLine(403, "Forbidden") + "Connection: close\r\nContent-Length: 0\r\n\r\n");
      return;
    }
    const t = this.opts.target;
    const up = http.request({
      host: t.host,
      port: t.port,
      method: req.method,
      path: req.url,
      headers: filterHeaders(req.rawHeaders, { keepUpgrade: true, host: this.hostHeader }),
      setHost: false,
      createConnection: pinnedCreateConnection(t, this.opts.connectTimeoutMs ?? 10000) as never,
    });
    up.on("upgrade", (ures, usock, uhead) => {
      const lines: string[] = [];
      for (let i = 0; i + 1 < ures.rawHeaders.length; i += 2) {
        const lower = ures.rawHeaders[i].toLowerCase();
        if (lower === "connection" || lower === "upgrade" || !HOP_BY_HOP.has(lower)) lines.push(`${ures.rawHeaders[i]}: ${ures.rawHeaders[i + 1]}`);
      }
      socket.write(statusLine(ures.statusCode ?? 101, ures.statusMessage ?? "Switching Protocols") + lines.map((l) => l + "\r\n").join("") + "\r\n");
      if (uhead.length > 0) socket.write(uhead);
      if (head.length > 0) usock.write(head);
      this.tunnels.add(socket);
      this.tunnels.add(usock);
      const close = () => {
        this.tunnels.delete(socket);
        this.tunnels.delete(usock);
        usock.destroy();
        socket.destroy();
      };
      usock.on("error", close);
      socket.on("error", close);
      usock.on("close", close);
      socket.on("close", close);
      usock.pipe(socket);
      socket.pipe(usock);
    });
    up.on("response", (ures) => {
      // The server did not switch protocols: pass its answer on and close.
      const lines: string[] = [];
      for (let i = 0; i + 1 < ures.rawHeaders.length; i += 2) {
        const lower = ures.rawHeaders[i].toLowerCase();
        if (!HOP_BY_HOP.has(lower) && lower !== "content-length") lines.push(`${ures.rawHeaders[i]}: ${ures.rawHeaders[i + 1]}`);
      }
      const chunks: Buffer[] = [];
      let size = 0;
      ures.on("data", (c: Buffer) => {
        size += c.length;
        if (size <= 64 * 1024) chunks.push(c);
      });
      ures.on("end", () => {
        const body = Buffer.concat(chunks);
        socket.end(
          Buffer.concat([
            Buffer.from(statusLine(ures.statusCode ?? 502, ures.statusMessage ?? "") + lines.map((l) => l + "\r\n").join("") + `Content-Length: ${body.length}\r\nConnection: close\r\n\r\n`),
            body,
          ]),
        );
      });
      ures.on("error", () => socket.destroy());
    });
    up.on("error", (e) => {
      this.failures++;
      this.pinFailed(e);
      if (!isPinError(e)) this.log("warn", `upstream upgrade failed: ${(e as NodeJS.ErrnoException).code ?? e.message}`);
      socket.end(statusLine(502, "Bad Gateway") + "Connection: close\r\nContent-Length: 0\r\n\r\n");
    });
    socket.on("close", () => up.destroy());
    up.end();
  }
}
