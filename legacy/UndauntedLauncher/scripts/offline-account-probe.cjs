// Run with the development Electron binary, against a disposable packaged profile.
// DPAPI decryption stays in a main process; credentials never cross renderer IPC.
const { app, safeStorage } = require("electron");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { setTimeout: delay } = require("node:timers/promises");

let stage = "profile";
async function main() {
    const [profile, runtime] = process.argv.slice(-2);
    assert.ok(path.isAbsolute(profile) && path.isAbsolute(runtime));
    const slot = createHash("sha256").update("offline-local:61000").digest("hex").slice(0, 24);
    stage = "encryption availability";
    assert.ok(safeStorage.isEncryptionAvailable());
    stage = "DPAPI decryption";
    const plain = safeStorage.decryptString(fs.readFileSync(path.join(profile, "keys", slot + ".key")));
    const parts = plain.split("\n");
    stage = "encrypted key format";
    assert.ok(parts.length === 3 && parts[0] === "DRK2" && parts[1] === "-" && /^UUK_[a-f0-9]{48}$/.test(parts[2]), "Invalid encrypted offline key slot");
    const key = parts[2];
    const Database = require(path.join(runtime, "metagame", "node_modules", "better-sqlite3"));
    const db = new Database(path.join(profile, "offline", "undaunted.db"), { readonly: true, fileMustExist: true });
    let userId;
    stage = "database identity";
    try {
        const account = db.prepare("SELECT userId FROM userapikeys WHERE keyHash = ?").get(createHash("sha256").update(key).digest("hex"));
        assert.ok(account, "Encrypted player key must match the local database");
        userId = account.userId;
    } finally { db.close(); }
    stage = "APIKEY login";
    const login = await fetch("http://127.0.0.1:61000/account/api/oauth/token", {
        method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({ exchange_code: key }), signal: AbortSignal.timeout(15000),
    });
    assert.equal(login.status, 200, "Local APIKEY login failed");
    const auth = await login.json();
    assert.equal(auth.account_id, userId);
    const headers = { "Content-Type": "application/json", Authorization: "Bearer " + auth.access_token };
    // Observe the existing loopback allocation response, including admission
    // refusal, before the client's status poll turns it into generic FAILED.
    stage = "Deploy admission";
    const allocation = await fetch("http://127.0.0.1:61001/api/matchmaker/handle-matchmaking-for-player", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ GameMode: "CITY", GameArgs: "", HuntId: "", ExpectedPlayers: [userId] }),
        signal: AbortSignal.timeout(120000),
    });
    if (allocation.status !== 200) {
        const refusal = await allocation.json();
        console.log(JSON.stringify({ authenticated: true, allocationStatus: allocation.status,
            error: refusal.error === "capacity_unavailable" ? refusal.error : "allocation_failed",
            reason: ["memory", "ports", "hunts", "cpu"].includes(refusal.reason) ? refusal.reason : undefined }));
        process.exitCode = 1;
        return;
    }
    stage = "native matchmaking";
    const joined = await fetch("http://127.0.0.1:61000/candidate/join", {
        method: "POST", headers, body: JSON.stringify({ gameMode: "CITY", gameArgs: "", playerHuntId: "" }), signal: AbortSignal.timeout(120000),
    });
    assert.equal(joined.status, 200, "Authenticated local matchmaking request failed");
    const deadline = Date.now() + 120000;
    let status;
    while (Date.now() < deadline) {
        const response = await fetch("http://127.0.0.1:61000/candidate/status", { headers, signal: AbortSignal.timeout(15000) });
        assert.equal(response.status, 200);
        status = await response.json();
        if (status.status === "FAILED" || status.serverInfo) break;
        await delay(1000);
    }
    if (!status?.serverInfo) {
        const deploy = await (await fetch("http://127.0.0.1:61001/gameservers")).json();
        console.log(JSON.stringify({ authenticated: true, matchmaking: status?.status ?? "timeout", servers: deploy.servers.length }));
        process.exitCode = 1;
        return;
    }
    assert.equal(status.serverInfo.host, "127.0.0.1");
    assert.ok(status.serverInfo.port >= 8770 && status.serverInfo.port <= 8777);
    const deploy = await (await fetch("http://127.0.0.1:61001/gameservers")).json();
    assert.ok(deploy.servers.some(server => server.port === status.serverInfo.port));
    console.log(JSON.stringify({ authenticated: true, matchmaking: status.status, host: status.serverInfo.host, port: status.serverInfo.port, servers: deploy.servers.length }));
}

app.whenReady().then(main).catch(() => {
    // HTTP bodies and decrypted material may contain credentials. Keep failures generic.
    console.error("Offline account/native allocation probe failed at " + stage);
    process.exitCode = 1;
}).finally(() => app.exit(process.exitCode ?? 0));
