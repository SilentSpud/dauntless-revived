// Local stand-ins for the host's content server (contract 4) and metagame (contracts 2 and 3),
// used by the unit tests. They listen on 127.0.0.1 only, on the spare test ports.

import http from "node:http";
import https from "node:https";
import { createHash } from "node:crypto";
import type { AddressInfo } from "node:net";

export const CONTENT_PORT = 62012;
export const METAGAME_PORT = 62013;

export interface TlsOptions {
  cert: string;
  key: string;
}

export function createServer(handler: http.RequestListener, tls?: TlsOptions): http.Server {
  return tls ? https.createServer({ cert: tls.cert, key: tls.key }, handler) : http.createServer(handler);
}
export const TEST_BUILD = "dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827";

export interface FakeFile {
  path: string;
  data: Buffer;
}

export function sha256(data: Buffer): string {
  return createHash("sha256").update(data).digest("hex");
}

export function makeFiles(): FakeFile[] {
  const pattern = (n: number, seed: number) => {
    const b = Buffer.alloc(n);
    for (let i = 0; i < n; i++) b[i] = (i * 31 + seed * 7) & 0xff;
    return b;
  };
  return [
    { path: "Archon/Binaries/Win64/Fake-Game.exe", data: pattern(300_000, 1) },
    { path: "Archon/Content/Paks/pakchunk0-WindowsNoEditor.pak", data: pattern(900_000, 2) },
    { path: "Engine/Config/BaseEngine.ini", data: Buffer.from("[Core.System]\r\nPaths=../../../Engine/Content\r\n") },
    { path: "Version.txt", data: Buffer.from(TEST_BUILD) },
    { path: "Archon/Content/Movies/intro (1).bk2", data: pattern(120_000, 3) },
    { path: "Engine/Binaries/Win64/empty.dat", data: Buffer.alloc(0) },
  ];
}

export function manifestFor(files: FakeFile[]) {
  const list = files
    .map((f) => ({ path: f.path, size: f.data.length, sha256: sha256(f.data) }))
    .sort((a, b) => (a.path < b.path ? -1 : 1));
  return { build: TEST_BUILD, totalBytes: list.reduce((s, f) => s + f.size, 0), files: list };
}

export interface RequestLog {
  path: string;
  range: string | undefined;
  hadKey: boolean;
  status: number;
}

export interface ContentOptions {
  key: string;
  files: FakeFile[];
  corruptOnce?: Set<string>; // first GET of these paths sends flipped bytes (correct length and ETag)
  chunkSize?: number; // throttling: bytes per tick
  tickMs?: number;
  manifestOverride?: unknown;
  ignoreRange?: boolean;
  news?: unknown;
  branding?: unknown;
  brandingFiles?: Record<string, Buffer>;
}

export class FakeContentServer {
  readonly requests: RequestLog[] = [];
  active = 0;
  maxActive = 0;
  private server: http.Server | null = null;
  private readonly byPath: Map<string, Buffer>;
  private readonly corrupted = new Set<string>();

  constructor(public opts: ContentOptions) {
    this.byPath = new Map(opts.files.map((f) => [f.path, f.data]));
  }

  get manifest() {
    return manifestFor(this.opts.files);
  }

  start(port = CONTENT_PORT, tls?: TlsOptions): Promise<number> {
    return new Promise((resolve, reject) => {
      this.server = createServer((req, res) => this.handle(req, res), tls);
      this.server.on("error", reject);
      this.server.listen(port, "127.0.0.1", () => resolve((this.server!.address() as AddressInfo).port));
    });
  }

  stop(): Promise<void> {
    return new Promise((resolve) => {
      if (!this.server) return resolve();
      this.server.closeAllConnections();
      this.server.close(() => resolve());
    });
  }

  handle(req: http.IncomingMessage, res: http.ServerResponse): void {
    const url = new URL(req.url ?? "/", "http://x");
    if (url.pathname === "/content/v1/manifest") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(this.opts.manifestOverride ?? this.manifest));
      return;
    }
    if (url.pathname === "/content/v1/news") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(this.opts.news ?? { items: [] }));
      return;
    }
    if (url.pathname === "/content/v1/branding") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(this.opts.branding ?? { backgrounds: [], accent: null }));
      return;
    }
    if (url.pathname.startsWith("/content/v1/branding/")) {
      const data = this.opts.brandingFiles?.[url.pathname.slice("/content/v1/branding/".length)];
      if (!data) {
        res.writeHead(404).end();
        return;
      }
      res.writeHead(200, { "content-type": "image/png" });
      res.end(data);
      return;
    }
    if (!url.pathname.startsWith("/content/v1/files/")) {
      res.writeHead(404).end();
      return;
    }
    const rel = url.pathname
      .slice("/content/v1/files/".length)
      .split("/")
      .map(decodeURIComponent)
      .join("/");
    const key = req.headers["x-undaunted-user-api-key"];
    const log: RequestLog = { path: rel, range: req.headers.range, hadKey: key === this.opts.key, status: 0 };
    this.requests.push(log);
    if (key !== this.opts.key) {
      log.status = 401;
      res.writeHead(401).end();
      return;
    }
    const data = this.byPath.get(rel);
    if (!data) {
      log.status = 404;
      res.writeHead(404).end();
      return;
    }
    let body = data;
    if (this.opts.corruptOnce?.has(rel) && !this.corrupted.has(rel)) {
      this.corrupted.add(rel);
      body = Buffer.from(data);
      for (let i = 0; i < body.length; i += 997) body[i] ^= 0xff;
    }
    const etag = `"${sha256(data)}"`;
    let start = 0;
    let status = 200;
    const headers: http.OutgoingHttpHeaders = { etag, "content-type": "application/octet-stream", "accept-ranges": "bytes" };
    const range = /^bytes=(\d+)-$/.exec(req.headers.range ?? "");
    if (range && !this.opts.ignoreRange) {
      start = Number(range[1]);
      if (start >= data.length) {
        log.status = 416;
        res.writeHead(416, { "content-range": `bytes */${data.length}` }).end();
        return;
      }
      status = 206;
      headers["content-range"] = `bytes ${start}-${data.length - 1}/${data.length}`;
    }
    const slice = body.subarray(start);
    headers["content-length"] = String(slice.length);
    log.status = status;
    res.writeHead(status, headers);
    this.active++;
    this.maxActive = Math.max(this.maxActive, this.active);
    let finished = false;
    const done = () => {
      if (!finished) {
        finished = true;
        this.active--;
      }
    };
    res.on("close", done);
    const chunk = this.opts.chunkSize ?? 0;
    if (!chunk) {
      // A short delay keeps several transfers open at once, so the concurrency limit is observable.
      setTimeout(() => {
        res.end(slice);
        done();
      }, 30);
      return;
    }
    let offset = 0;
    const tick = () => {
      if (res.destroyed) return done();
      if (offset >= slice.length) {
        res.end();
        return done();
      }
      res.write(slice.subarray(offset, offset + chunk));
      offset += chunk;
      setTimeout(tick, this.opts.tickMs ?? 10);
    };
    tick();
  }
}

export interface MetagameOptions {
  name?: string;
  contentPort?: number | null;
  registration?: "OPEN" | "INVITECODE" | "NONE";
  validCodes?: Set<string>;
  existingUsers?: Map<string, string>; // lower-case username -> key
  statusMissing?: boolean; // behave like an older metagame without ServerStatus
  statusForEveryone?: boolean; // behave like a metagame that lists everyone to anyone (no "limited" field)
  sourceUrl?: string; // the host's own source link in ServerStatus (default: the project's repository)
}

export class FakeMetagame {
  readonly registrations: { username: string; code: string }[] = [];
  // Every ServerStatus request: did it carry a key header, and was it a known account's key?
  readonly statusCalls: { withKey: boolean; registered: boolean }[] = [];
  readonly users = new Map<string, { username: string; id: string }>(); // key -> user
  private server: http.Server | null = null;
  private counter = 0;
  private held: { arrived: () => void; gate: Promise<void> } | null = null;

  // The next ServerStatus request is answered (as of its arrival: key checked, list chosen) only
  // after release(): for a launcher action that overtakes a status request on its way.
  holdNextStatus(): { arrived: Promise<void>; release: () => void } {
    let arrived!: () => void;
    let release!: () => void;
    const arrivedP = new Promise<void>((r) => (arrived = r));
    const gate = new Promise<void>((r) => (release = r));
    this.held = { arrived, gate };
    return { arrived: arrivedP, release };
  }

  constructor(public opts: MetagameOptions) {
    for (const [lower, key] of opts.existingUsers ?? []) this.users.set(key, { username: lower, id: `UID-${lower}` });
  }

  start(port = METAGAME_PORT, tls?: TlsOptions): Promise<number> {
    return new Promise((resolve, reject) => {
      this.server = createServer((req, res) => this.handle(req, res), tls);
      this.server.on("error", reject);
      this.server.listen(port, "127.0.0.1", () => resolve((this.server!.address() as AddressInfo).port));
    });
  }

  stop(): Promise<void> {
    return new Promise((resolve) => {
      if (!this.server) return resolve();
      this.server.closeAllConnections();
      this.server.close(() => resolve());
    });
  }

  private json(res: http.ServerResponse, status: number, body: unknown): void {
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  }

  handle(req: http.IncomingMessage, res: http.ServerResponse): void {
    const url = new URL(req.url ?? "/", "http://x");
    if (url.pathname === "/dauntless-status") return this.json(res, 200, { "show-status": true, en: "Welcome to Dauntless Revived!" });
    if (url.pathname === "/undaunted/api/ServerStatus") {
      if (this.opts.statusMissing) return this.json(res, 404, {});
      // Like the metagame: the player list only for a known account key, never a 401.
      const key = req.headers["x-undaunted-user-api-key"];
      const registered = typeof key === "string" && this.users.has(key);
      this.statusCalls.push({ withKey: key !== undefined, registered });
      const full = registered || this.opts.statusForEveryone === true;
      const held = this.held;
      this.held = null;
      const answer = () => this.json(res, 200, {
        name: this.opts.name ?? "Test Server",
        online: true,
        version: "1.0.0",
        commit: "abc1234",
        sourceUrl: this.opts.sourceUrl ?? "https://github.com/mixutin/dauntless-revived",
        registration: this.opts.registration ?? "INVITECODE",
        playersOnline: full ? 2 : 0,
        players: full
          ? [
              { name: "Aurora", where: "city", instance: "ramsgate" },
              { name: "Borealis", where: "hunt", instance: "hunt-1" },
            ]
          : [],
        instances: full
          ? [
              { id: "ramsgate", kind: "city", title: "Ramsgate", map: "Hub", behemoth: null, players: 1, maxPlayers: 60, startedAt: new Date(Date.now() - 3600_000).toISOString() },
              { id: "hunt-1", kind: "hunt", title: "Hunt: Shrike", map: "Island", behemoth: "Shrike", players: 1, maxPlayers: 4, startedAt: new Date().toISOString() },
            ]
          : [],
        contentPort: this.opts.contentPort === undefined ? CONTENT_PORT : this.opts.contentPort,
        uptimeSeconds: 7200,
        ...(this.opts.statusForEveryone === true ? {} : { limited: !full }),
      });
      if (!held) return answer();
      held.arrived();
      void held.gate.then(answer);
      return;
    }
    if (url.pathname === "/undaunted/api/GetUserInfo") {
      const key = req.headers["x-undaunted-user-api-key"];
      const user = typeof key === "string" ? this.users.get(key) : undefined;
      if (!user) return this.json(res, 401, {});
      return this.json(res, 200, { UserId: user.id, Username: user.username, IsAdmin: false });
    }
    if (url.pathname === "/undaunted/api/Register" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        let parsed: { Username?: unknown; InviteCode?: unknown };
        try {
          parsed = JSON.parse(body);
        } catch {
          return this.json(res, 400, { error: "bad_request", message: "bad json" });
        }
        const username = parsed.Username;
        const code = parsed.InviteCode;
        if (this.opts.registration === "NONE") return this.json(res, 400, { error: "registration_closed", message: "closed" });
        if (typeof username !== "string" || !/^[A-Za-z0-9_]{3,16}$/.test(username)) {
          return this.json(res, 400, { error: "username_invalid", message: "bad name" });
        }
        for (const u of this.users.values()) {
          if (u.username.toLowerCase() === username.toLowerCase()) return this.json(res, 409, { error: "username_taken" });
        }
        if ((this.opts.registration ?? "INVITECODE") === "INVITECODE") {
          if (typeof code !== "string" || !this.opts.validCodes?.has(code)) return this.json(res, 401, { error: "invite_invalid" });
          this.opts.validCodes.delete(code);
        }
        this.registrations.push({ username, code: String(code) });
        const key = `UUK_${(++this.counter).toString(16).padStart(48, "0")}`;
        this.users.set(key, { username, id: `UID-${this.counter}` });
        return this.json(res, 200, { UUK: key });
      });
      return;
    }
    res.writeHead(404).end();
  }
}
