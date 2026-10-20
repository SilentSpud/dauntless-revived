// Writes node-crypto.json: tokens and a recovery blob made by the Node metagame's own libraries, which
// the Rust port must accept or refuse the same way (MIGRATION_PLAN.md M20, M22). Run from the repo root
// with the Node metagame's dependencies installed:
//   node rust/services/metagame/tests/fixtures/make-node-crypto.cjs
const path = require("node:path");
const fs = require("node:fs");
const crypto = require("node:crypto");
const jwt = require(require.resolve("jsonwebtoken", { paths: [path.resolve("legacy/UndauntedMetagame")] }));

const { privateKey, publicKey } = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const privB64 = Buffer.from(privateKey, "utf8").toString("base64");
const pubB64 = Buffer.from(publicKey, "utf8").toString("base64");
const sign = (payload, options) => jwt.sign(payload, privateKey, { algorithm: "RS256", ...options });
const iss = "undaunted-metagame", aud = "undaunted-metagame";

// The same options as SignMetagameJWTForUid, except a lifetime long enough for a fixture.
const valid = sign({ userId: "UID-node", epoch: 3 }, { expiresIn: "100y", issuer: iss, audience: aud });
const noEpoch = sign({ userId: "UID-old" }, { expiresIn: "100y", issuer: iss, audience: aud });
const expired = sign({ userId: "UID-node", epoch: 0, iat: 1700000000, exp: 1700086400 }, { issuer: iss, audience: aud });
const wrongIssuer = sign({ userId: "UID-node", epoch: 0 }, { expiresIn: "100y", issuer: "someone-else", audience: aud });
const wrongAudience = sign({ userId: "UID-node", epoch: 0 }, { expiresIn: "100y", issuer: iss, audience: "someone-else" });
const b64url = (o) => Buffer.from(JSON.stringify(o)).toString("base64url");
const claims = { userId: "UID-node", epoch: 0, exp: 4102444800, iss, aud };
const algNone = `${b64url({ alg: "none", typ: "JWT" })}.${b64url(claims)}.`;
const hsBody = `${b64url({ alg: "HS256", typ: "JWT" })}.${b64url(claims)}`;
const hs256WithPublicKey = `${hsBody}.${crypto.createHmac("sha256", publicKey).update(hsBody).digest("base64url")}`;
const [h, p, s] = valid.split(".");
const badSignature = `${h}.${p}.${s.slice(0, -4)}${s.slice(-4) === "AAAA" ? "BBBB" : "AAAA"}`;

// RememberAccountKey's encryption, with AUTH_SIGNING_PRIVKEY_B64 = privB64.
const key = crypto.createHash("sha256").update("dauntless-account-recovery-v1\0" + privB64).digest();
const accountKey = "UUK_" + crypto.randomBytes(24).toString("hex");
const iv = crypto.randomBytes(12), cipher = crypto.createCipheriv("aes-256-gcm", key, iv);
cipher.setAAD(Buffer.from("UID-node"));
const recoveryBlob = Buffer.concat([iv, cipher.update(accountKey, "utf8"), cipher.final(), cipher.getAuthTag()]).toString("base64");

const out = {
  about: "Made by make-node-crypto.cjs with the Node metagame's jsonwebtoken and node:crypto.",
  privB64, pubB64,
  tokens: { valid, noEpoch, expired, wrongIssuer, wrongAudience, algNone, hs256WithPublicKey, badSignature },
  recovery: { userId: "UID-node", accountKey, accountKeyHash: crypto.createHash("sha256").update(accountKey).digest("hex"), blob: recoveryBlob },
};
fs.writeFileSync(path.join(__dirname, "node-crypto.json"), JSON.stringify(out, null, 2) + "\n");
console.log("node-crypto.json written");
