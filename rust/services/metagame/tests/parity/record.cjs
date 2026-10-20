// Records what the Node metagame answers to a request script (<area>.requests.json), for the Rust
// metagame's parity test (tests/parity.rs; MIGRATION_PLAN.md M29, M31, a small M35 per area).
//
//   cd legacy/UndauntedMetagame && npm ci && npx tsc -p tsconfig.test.json
//   node ../../rust/services/metagame/tests/parity/record.cjs accounts
//
// Writes <area>.expected.json next to the script: per request the status, the headers the client or
// the tools read, and the body. Values that differ on every run (tokens, account keys, generated ids,
// times, invite codes) are replaced by placeholders, the same way tests/parity.rs does.
"use strict";
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");

const area = process.argv[2];
if (!area) throw new Error("usage: record.cjs <area>");
const here = __dirname;
const script = JSON.parse(fs.readFileSync(path.join(here, `${area}.requests.json`), "utf8"));
const metagame = path.resolve(here, "../../../../../legacy/UndauntedMetagame");
const build = path.join(metagame, "build", "src");

// The environment before the app loads, as the Node tests set it (test/setup.ts, authenv.ts, appenv.ts).
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "parity-"));
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

const { app } = require(path.join(build, "app.js"));
const { GetDb } = require(path.join(build, "db.js"));
const { SignMetagameJWTForUid } = require(path.join(build, "controllers", "auth.js"));

// Kept in step with normalize() in tests/parity.rs.
function normalize(text) {
  return text
    .replace(/eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g, "<jwt>")
    .replace(/UUK_[0-9a-f]{48}/g, "<uuk>")
    .replace(/UID-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, "<uid>")
    .replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/g, "<date>")
    .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, "<uuid>")
    .replace(/\b(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{1,2}, \d{4}\b/g, "<day>")
    .replace(/[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}/g, "<invite>")
    // The seconds left in a rate-limit window depend on how fast the requests ran
    .replace(/; t=\d+/g, "; t=<s>");
}

const HEADERS = ["content-type", "cache-control", "retry-after", "ratelimit", "ratelimit-policy", "content-security-policy", "x-content-type-options"];

function fill(value) {
  return value
    .replace(/\{\{bearer:([^}]+)\}\}/g, (_, id) => `bearer ${SignMetagameJWTForUid(id)}`)
    .replace(/\{\{gs\}\}/g, script.gameServerKey);
}

async function main() {
  const db = GetDb();
  for (const sql of script.setup) db.$client.exec(sql);
  const server = await new Promise((resolve, reject) => {
    const started = app.listen(0, "127.0.0.1", (error) => (error ? reject(error) : resolve(started)));
  });
  const base = `http://127.0.0.1:${server.address().port}`;
  const results = [];
  try {
    for (const request of script.requests) {
      const headers = {};
      for (const [name, value] of Object.entries(request.headers ?? {})) headers[name] = fill(value);
      let body;
      if (request.json !== undefined) {
        headers["content-type"] = "application/json; charset=utf-8";
        body = JSON.stringify(request.json);
      } else if (request.raw !== undefined) {
        headers["content-type"] = "application/json; charset=utf-8";
        body = request.raw;
      } else if (request.form !== undefined) {
        headers["content-type"] = "application/x-www-form-urlencoded";
        body = request.form;
      }
      const response = await fetch(base + request.path, { method: request.method, headers, body });
      const recorded = {};
      for (const name of HEADERS) {
        const value = response.headers.get(name);
        if (value !== null) recorded[name] = normalize(value);
      }
      results.push({ request: `${request.method} ${request.path}`, status: response.status, headers: recorded, body: normalize(await response.text()) });
    }
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    db.$client.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
  fs.writeFileSync(path.join(here, `${area}.expected.json`), JSON.stringify({ about: "Recorded by record.cjs from the Node metagame; do not edit.", results }, null, 2) + "\n");
  console.log(`${area}.expected.json: ${results.length} replies`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
