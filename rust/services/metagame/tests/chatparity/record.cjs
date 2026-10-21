// Records what the Node chat sends to scripted WebSocket clients (<name>.chat.json), for the Rust
// chat's parity test (tests/chatparity.rs; MIGRATION_PLAN.md M32-M34).
//
//   cd legacy/UndauntedMetagame && npx tsc -p tsconfig.test.json
//   node ../../rust/services/metagame/tests/chatparity/record.cjs <name>
//
// A script is setup SQL and steps. A step connects a client ({"connect": "a"}), sends it a frame
// ({"send": ["a", "<frame/>"]}), closes it ({"close": "a"}), makes an HTTP request of the metagame
// ({"http": {...}}, as the parity scripts do) or waits ({"wait": ms}). After each step (and its
// "settle", 150 ms by default) the frames each client received are recorded, in order, and whether
// its connection closed (with the code). In frames, {{plain:<account>}} is a SASL PLAIN payload with
// a token for that account, {{bearer:<account>}} a bearer token.
//
// Writes <name>.chat.expected.json next to the script. Kept in step with tests/chatparity.rs.
"use strict";
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");

const name = process.argv[2];
if (!name) throw new Error("usage: record.cjs <name>");
const here = __dirname;
const script = JSON.parse(fs.readFileSync(path.join(here, `${name}.chat.json`), "utf8"));
const metagame = path.resolve(here, "../../../../../legacy/UndauntedMetagame");
const build = path.join(metagame, "build", "src");

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "chatparity-"));
const keys = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
Object.assign(process.env, {
  DB_FILENAME: path.join(dir, "undaunted.db"),
  LOG_LEVEL: "silent",
  LOG_REQUESTS: "0",
  LOG_BODIES: "0",
  MATCHMAKING_MODE: "DISABLED",
  AUTH_SIGNING_PRIVKEY_B64: Buffer.from(keys.privateKey).toString("base64"),
  AUTH_SIGNING_PUBKEY_B64: Buffer.from(keys.publicKey).toString("base64"),
  ...script.env,
});
process.chdir(metagame);
const WebSocket = require(path.join(metagame, "node_modules", "ws"));

// Kept in step with normalize() in tests/chatparity.rs.
function normalize(text) {
  return text
    .replace(/eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g, "<jwt>")
    .replace(/<open (.*?)id="[0-9a-f]{16}"/g, '<open $1id="<id>"')
    .replace(/srv-[0-9a-f]{32}/g, "srv-<hex>")
    .replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/g, "<date>");
}

async function main() {
  const { app } = require(path.join(build, "app.js"));
  const { GetDb } = require(path.join(build, "db.js"));
  const { SignMetagameJWTForUid } = require(path.join(build, "controllers", "auth.js"));
  const { ChatServer, ReadChatConfig } = require(path.join(build, "realtime", "chat.js"));
  for (const sql of script.setup ?? []) GetDb().$client.exec(sql);
  const fill = (text) => text
    .replace(/\{\{plain:([^}]+)\}\}/g, (_, id) => Buffer.from(`\0${id}\0${SignMetagameJWTForUid(id)}`, "utf8").toString("base64"))
    .replace(/\{\{bearer:([^}]+)\}\}/g, (_, id) => `bearer ${SignMetagameJWTForUid(id)}`);

  const config = ReadChatConfig(process.env);
  const chat = new ChatServer({ NickCheck: config.NickCheck });
  await chat.listen(0, "127.0.0.1");
  const web = await new Promise((resolve, reject) => {
    const started = app.listen(0, "127.0.0.1", (error) => (error ? reject(error) : resolve(started)));
  });
  const base = `http://127.0.0.1:${web.address().port}`;

  const clients = {};
  const results = [];
  try {
    for (const step of script.steps) {
      let http;
      if (step.connect !== undefined) {
        const client = { frames: [], closed: undefined };
        clients[step.connect] = client;
        const headers = step.headers ?? {};
        client.socket = new WebSocket(`ws://127.0.0.1:${chat.port}/`, "xmpp", { headers });
        client.socket.on("message", (data) => client.frames.push(normalize(data.toString("utf8"))));
        client.socket.on("close", (code) => (client.closed = code));
        client.socket.on("error", () => {});
        client.socket.on("unexpected-response", (_req, res) => (client.closed = `http ${res.statusCode}`));
        await new Promise((resolve) => {
          client.socket.once("open", resolve);
          client.socket.once("close", resolve);
          client.socket.once("unexpected-response", resolve);
        });
      } else if (step.send !== undefined) {
        const [who, frame] = step.send;
        clients[who].socket.send(fill(frame));
      } else if (step.close !== undefined) {
        clients[step.close].socket.close(1000);
      } else if (step.http !== undefined) {
        const request = step.http;
        const headers = {};
        for (const [header, value] of Object.entries(request.headers ?? {})) headers[header] = fill(value);
        let body;
        if (request.json !== undefined) {
          headers["content-type"] = "application/json; charset=utf-8";
          body = fill(JSON.stringify(request.json));
        }
        const reply = await fetch(base + request.path, { method: request.method, headers, body });
        http = reply.status;
        await reply.text();
      }
      await new Promise((resolve) => setTimeout(resolve, step.wait ?? step.settle ?? 150));
      const seen = {};
      for (const [who, client] of Object.entries(clients)) {
        if (client.frames.length > 0 || client.closed !== undefined) {
          seen[who] = { frames: client.frames.splice(0) };
          if (client.closed !== undefined) {
            seen[who].closed = client.closed;
            delete clients[who];
          }
        }
      }
      results.push({ step: JSON.stringify(step).slice(0, 200), ...(http !== undefined ? { http } : {}), seen });
    }
  } finally {
    for (const client of Object.values(clients)) client.socket.terminate();
    await chat.close();
    web.closeAllConnections();
    await new Promise((resolve) => web.close(resolve));
    GetDb().$client.close();
  }
  const out = path.join(here, `${name}.chat.expected.json`);
  fs.writeFileSync(out, JSON.stringify({ about: script.about, results }, null, 2) + "\n");
  console.log(`${path.basename(out)}: ${results.length} steps`);
  process.exit(0);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
