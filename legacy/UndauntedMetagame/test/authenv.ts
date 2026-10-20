import { generateKeyPairSync } from "node:crypto";

// Import after ./setup and before anything that loads controllers/auth, which reads the
// token signing keys at load. A throwaway key pair made for this test process only.
const Pair = generateKeyPairSync("rsa", {
    modulusLength: 2048,
    publicKeyEncoding: { type: "spki", format: "pem" },
    privateKeyEncoding: { type: "pkcs8", format: "pem" }
});

process.env.AUTH_SIGNING_PRIVKEY_B64 = Buffer.from(Pair.privateKey, "utf8").toString("base64");
process.env.AUTH_SIGNING_PUBKEY_B64 = Buffer.from(Pair.publicKey, "utf8").toString("base64");
process.env.AUTH_MODE = "APIKEY";
