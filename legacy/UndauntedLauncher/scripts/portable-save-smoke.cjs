// Run after npm run package. Only disposable profiles and the packaged runtime are used.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const { createHash } = require("node:crypto");
const { spawn, spawnSync, execFileSync } = require("node:child_process");
const resources = path.resolve(__dirname, "../out/Dauntless Revived Launcher-win32-x64/resources");
const runtime = path.join(resources, "offline-runtime");
const keepRestored = process.argv.includes("--keep-restored-profile");
const mark = message => console.log("[portable-save-smoke] " + message);
let phase = "initialization";
function listeners() {
    return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command",
        "Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in 61000,61001,61099 } | Select-Object LocalPort; Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -ge 8770 -and $_.LocalPort -le 8777 } | Select-Object LocalPort; exit 0"],
    { encoding: "utf8", windowsHide: true }).trim();
}

async function smoke() {
    const { app, safeStorage } = require("electron");
    await app.whenReady();
    const ts = require("typescript");
    // Use the locked compiler in memory; leave Electron's native CommonJS resolution intact.
    require.extensions[".ts"] = (module, file) => module._compile(ts.transpileModule(require("node:fs").readFileSync(file, "utf8"), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true }, fileName: file,
    }).outputText, file);
    const { PortableOfflineSave, OFFLINE_KEY_SLOT } = require("../src/main/offline-save.ts");
    const { offlineLayout } = require("../src/main/offline-layout.ts");
    const { electronSaveDatabase } = require("../src/main/offline-save-runtime.ts");
    const { makeLocalStack } = require("../src/main/local-stack-factory.ts");
    const { KeyStore } = require("../src/main/keystore.ts");
    const { Controller } = require("../src/main/controller.ts");
    const { setSink } = require("../src/main/log.ts");
    const { defaultSettings } = require("../src/main/settings.ts");
    const { loadOfflineSecrets } = require("../src/main/offline-state.ts");
    const { readSave } = require("../src/main/offline-save-format.ts");
    const { drizzle } = require(path.join(runtime, "metagame/node_modules/drizzle-orm/better-sqlite3/index.cjs"));
    const { migrate } = require(path.join(runtime, "metagame/node_modules/drizzle-orm/better-sqlite3/migrator.cjs"));
    const helper = require(path.join(runtime, "offline-save-db.cjs"));
    assert.ok((await fs.readFile(path.join(runtime, "offline-save-db.cjs"))).equals(await fs.readFile(path.resolve(__dirname, "../runtime/offline-save-db.cjs"))));
    const bundled = require("@electron/asar").extractFile(path.join(resources, "app.asar"), path.join(".vite", "build", "main.js"));
    assert.ok(bundled.equals(await fs.readFile(path.resolve(__dirname, "../.vite/build/main.js"))));
    assert.ok(bundled.includes(Buffer.from("exportOfflineSave")) && bundled.includes(Buffer.from("rolled-back")));
    mark("Packaged main bundle and save helper match the current build/source");
    const secretsSeen = [];
    const captured = [];
    setSink(line => captured.push(line));
    assert.equal(process.versions.modules, "149");
    for (const service of ["metagame", "deploy"]) {
        const Database = require(path.join(runtime, service, "node_modules/better-sqlite3"));
        const db = new Database(":memory:");
        try { db.exec("CREATE TABLE test(id INTEGER)"); } finally { db.close(); }
    }
    mark("Both packaged SQLite addons work under Electron ABI 149");
    phase = "initial port check";
    assert.equal(listeners(), "", "Local ports must be clear before the smoke");
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "dr-portable-smoke-"));
    let stack, controller, handoffReady = false;
    try {
        const local = path.join(root, "local");
        await fs.mkdir(local);
        const gameRoot = path.join(root, "empty-game");
        await fs.mkdir(gameRoot);
        function encryptor(machine) {
            return { isAvailable: () => true,
                encrypt: plain => Buffer.from(machine + ":" + Buffer.from(plain).toString("base64")),
                decrypt: data => {
                    assert.ok(data.toString().startsWith(machine + ":"), "Key belongs to a different machine context");
                    return Buffer.from(data.toString().slice(machine.length + 1), "base64").toString();
                } };
        }
        async function profile(machine, selectedResources = resources, selectedCrypto = null) {
            const userData = path.join(root, machine);
            await fs.mkdir(userData);
            const layout = offlineLayout(selectedResources, userData);
            const crypto = selectedCrypto || encryptor(machine);
            const keys = new KeyStore(path.join(userData, "keys"), crypto);
            const saves = new PortableOfflineSave({ layout, userData, keys,
                database: electronSaveDatabase(layout.runtimeRoot, layout.metagameDir) });
            return { userData, layout, crypto, keys, saves };
        }
        function localStack(profile) {
            return makeLocalStack({ packaged: true, platform: "win32", resourcesPath: resources,
                userDataDir: profile.userData, appPath: path.dirname(resources),
                env: { ...process.env, LOCALAPPDATA: local }, gameRoot: () => gameRoot });
        }
        async function info(key) {
            const response = await fetch("http://127.0.0.1:61000/undaunted/api/GetUserInfo", {
                headers: { "X-Undaunted-User-API-Key": key } });
            assert.equal(response.status, 200, "Restored APIKEY authentication must succeed");
            return response.json();
        }
        const a = await profile("machine-a");
        phase = "profile A service startup";
        stack = localStack(a);
        await stack.start();
        mark("Packaged Metagame and Deploy started for disposable profile A");
        phase = "profile A registration";
        const response = await fetch("http://127.0.0.1:61000/undaunted/api/Register", {
            method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ Username: "PortableSmoke" }) });
        assert.equal(response.status, 200);
        const { UUK: activeKey } = await response.json();
        secretsSeen.push(activeKey);
        await a.keys.save(OFFLINE_KEY_SLOT, activeKey);
        const original = await info(activeKey);
        const login = await fetch("http://127.0.0.1:61000/account/api/oauth/token", {
            method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: new URLSearchParams({ exchange_code: activeKey }) });
        assert.equal(login.status, 200);
        const auth = await login.json(); secretsSeen.push(auth.access_token);
        const headers = { Authorization: "Bearer " + auth.access_token };
        const characterResponse = await fetch("http://127.0.0.1:61000/character", { headers });
        assert.equal(characterResponse.status, 200);
        const [character] = await characterResponse.json();
        for (const endpoint of [`/inventory/${original.UserId}/${character.id}`, `/loadout/${original.UserId}/${character.id}/all`]) {
            assert.equal((await fetch("http://127.0.0.1:61000" + endpoint, { headers })).status, 200);
        }
        await stack.stop(); stack = null;
        const Database = require(path.join(runtime, "metagame/node_modules/better-sqlite3"));
        const db = new Database(a.layout.databasePath);
        try {
            db.prepare("UPDATE users SET notes=17 WHERE userId=?").run(original.UserId);
            db.prepare("INSERT INTO recoveryevents(discordId,action,createdAt) VALUES ('12345678901234567','confirm_attempt',12345)").run();
        } finally { db.close(); }
        function state(file) {
            const db = new Database(file, { readonly: true });
            try { return JSON.stringify(["users", "characters", "inventories", "loadouts", "recoveryevents"].map(table => db.prepare(`SELECT * FROM ${table}`).all())); }
            finally { db.close(); }
        }
        const player = state(a.layout.databasePath);
        phase = "profile A export";
        const privateSlot = { host: "online.example", port: 61000, mode: "private", fp: null };
        const publicSlot = { host: "public.example", port: 443, mode: "public", fp: "1".repeat(64) };
        await a.keys.save(privateSlot, "online-private-smoke-key"); await a.keys.save(publicSlot, "online-public-smoke-key");
        await fs.writeFile(path.join(a.userData, "settings.json"), JSON.stringify(defaultSettings("en")));
        async function fingerprint(profile) {
            const files = [profile.layout.databasePath, path.join(profile.layout.stateRoot, "secrets.json"), path.join(profile.userData, "settings.json")];
            for (const name of await fs.readdir(path.join(profile.userData, "keys"))) files.push(path.join(profile.userData, "keys", name));
            return Promise.all(files.sort().map(async file => [file, createHash("sha256").update(await fs.readFile(file)).digest("hex")]));
        }
        const originalProfile = await fingerprint(a);
        const before = await fs.readFile(a.layout.databasePath);
        const save = path.join(root, "portable.drsave");
        assert.deepEqual(await a.saves.exportSave(save), { ok: true });
        assert.ok(before.equals(await fs.readFile(a.layout.databasePath)), "Export changed live DB");
        assert.ok(await a.keys.load(OFFLINE_KEY_SLOT) === activeKey, "Export changed live key");
        const bytes = await fs.readFile(save);
        const secrets = JSON.parse(await fs.readFile(path.join(a.layout.stateRoot, "secrets.json"), "utf8"));
        secretsSeen.push(...Object.values(secrets));
        for (const value of secretsSeen) assert.ok(!bytes.includes(Buffer.from(value)), "Active/machine credential entered backup");
        const b = await profile("machine-b");
        phase = "profile B import";
        assert.deepEqual(await b.saves.importSave(save), { ok: true });
        assert.equal(state(b.layout.databasePath), player);
        const importedKey = await b.keys.load(OFFLINE_KEY_SLOT);
        assert.ok(importedKey && importedKey !== activeKey, "Backup must carry its own player key");
        secretsSeen.push(importedKey);
        mark("Real SQLite backup/export/import preserved account, character, inventory, loadouts and progression across different encryptors");
        stack = localStack(b);
        phase = "profile B authentication";
        await stack.start();
        const restored = await info(importedKey);
        assert.equal(restored.UserId, original.UserId);
        assert.equal(restored.Username, original.Username);
        assert.equal((await fetch("http://127.0.0.1:61000/undaunted/api/GetUserInfo", {
            headers: { "X-Undaunted-User-API-Key": activeKey } })).status, 401);
        await stack.stop(); stack = null;
        mark("Restored packaged Metagame authenticated the backup key; rejected the former live key");

        phase = "real Drizzle historical database";
        const historicalResources = path.join(root, "historical-resources");
        const historicalRuntime = path.join(historicalResources, "offline-runtime");
        const historicalMeta = path.join(historicalRuntime, "metagame");
        const migrationFolder = path.join(historicalMeta, "src/drizzle");
        const packagedFolder = path.join(runtime, "metagame/src/drizzle");
        await fs.cp(packagedFolder, migrationFolder, { recursive: true });
        const journalPath = path.join(migrationFolder, "meta/_journal.json");
        const journal = JSON.parse(await fs.readFile(journalPath, "utf8"));
        const currentCount = journal.entries.length;
        journal.entries = journal.entries.slice(0, -1);
        await fs.writeFile(journalPath, JSON.stringify(journal));
        await fs.symlink(path.join(runtime, "metagame/node_modules"), path.join(historicalMeta, "node_modules"), "junction");
        await fs.copyFile(path.join(runtime, "offline-save-db.cjs"), path.join(historicalRuntime, "offline-save-db.cjs"));
        const old = await profile("historical-source", historicalResources);
        await fs.mkdir(old.layout.stateRoot, { recursive: true });
        const oldDb = new Database(old.layout.databasePath);
        const sourceDb = new Database(a.layout.databasePath, { readonly: true });
        try {
            // Real Drizzle creates every history row; never synthesize bookkeeping.
            migrate(drizzle(oldDb), { migrationsFolder: migrationFolder });
            const quote = value => '"' + value.replace(/"/g, '""') + '"';
            for (const { name } of sourceDb.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != '__drizzle_migrations'").all()) {
                const columns = oldDb.prepare(`PRAGMA table_info(${quote(name)})`).all().map(row => row.name);
                // A later migration can add whole tables; historical saves cannot contain them.
                if (!columns.length) continue;
                const insert = oldDb.prepare(`INSERT INTO ${quote(name)} (${columns.map(quote).join(",")}) VALUES (${columns.map(() => "?").join(",")})`);
                for (const row of sourceDb.prepare(`SELECT * FROM ${quote(name)}`).all()) insert.run(...columns.map(column => row[column]));
            }
            assert.equal(oldDb.prepare("SELECT count(*) n FROM __drizzle_migrations").get().n, currentCount - 1);
        } finally { oldDb.close(); sourceDb.close(); }
        await old.keys.save(OFFLINE_KEY_SLOT, activeKey);
        await old.keys.save(privateSlot, "online-private-smoke-key"); await old.keys.save(publicSlot, "online-public-smoke-key");
        await fs.writeFile(path.join(old.userData, "settings.json"), JSON.stringify(defaultSettings("en")));
        secretsSeen.push(...Object.values(await loadOfflineSecrets(old.layout.stateRoot)));
        const oldFingerprint = await fingerprint(old);
        const historicalSave = path.join(root, "historical.drsave");
        assert.deepEqual(await old.saves.exportSave(historicalSave), { ok: true });
        assert.deepEqual(await fingerprint(old), oldFingerprint);
        const extracted = path.join(root, "historical-export.db");
        const metadata = await readSave(historicalSave, extracted);
        secretsSeen.push(metadata.credential.key);
        const extractedDb = new Database(extracted, { readonly: true });
        try { assert.equal(extractedDb.prepare("SELECT count(*) n FROM __drizzle_migrations").get().n, currentCount - 1); }
        finally { extractedDb.close(); }
        assert.equal((await fs.readFile(historicalSave)).readUInt32BE(8), 1);
        const dpapi = { isAvailable: () => safeStorage.isEncryptionAvailable(), encrypt: plain => safeStorage.encryptString(plain), decrypt: bytes => safeStorage.decryptString(bytes) };
        assert.ok(dpapi.isAvailable());
        const c = await profile("historical-destination", resources, dpapi);
        await fs.mkdir(c.layout.stateRoot, { recursive: true });
        await fs.copyFile(b.layout.databasePath, c.layout.databasePath);
        await c.keys.save(OFFLINE_KEY_SLOT, importedKey);
        await c.keys.save(privateSlot, "online-private-smoke-key"); await c.keys.save(publicSlot, "online-public-smoke-key");
        await fs.writeFile(path.join(c.userData, "settings.json"), JSON.stringify(defaultSettings("en")));
        const destinationSecrets = await loadOfflineSecrets(c.layout.stateRoot);
        secretsSeen.push(...Object.values(destinationSecrets));
        phase = "historical packaged import";
        // Windows safeStorage uses the destination's Chromium Local State key.
        // An actor in that profile matches the eventual packaged Launcher's context.
        const actorJob = path.join(root, "restore-probe.json"), actorResult = actorJob + ".result";
        await fs.writeFile(actorJob, JSON.stringify({ resources, userData: c.userData, save: historicalSave,
            result: actorResult, expectedMigrations: currentCount, userId: original.UserId }), { mode: 0o600, flag: "wx" });
        try {
            const env = { ...process.env }; delete env.ELECTRON_RUN_AS_NODE;
            const actor = spawnSync(process.execPath, ["--user-data-dir=" + c.userData, path.join(__dirname, "portable-save-restore-probe.cjs"), actorJob],
                { env, stdio: "ignore", windowsHide: true, timeout: 90000 });
            const result = JSON.parse(await fs.readFile(actorResult, "utf8"));
            if (!result.ok) mark("Destination-profile restore probe failed at " + result.stage);
            assert.ok(actor.status === 0 && result.ok && result.stagedValidated);
        } finally { await fs.rm(actorJob, { force: true }); await fs.rm(actorResult, { force: true }); }
        assert.ok(await c.keys.load(privateSlot) === "online-private-smoke-key" && await c.keys.load(publicSlot) === "online-public-smoke-key");
        assert.equal(await fs.readFile(path.join(c.layout.stateRoot, "secrets.json"), "utf8"), JSON.stringify(destinationSecrets));
        assert.equal(await fs.readFile(path.join(c.userData, "settings.json"), "utf8"), JSON.stringify(defaultSettings("en")));
        assert.equal(state(c.layout.databasePath), player);
        function history(file) {
            const db = new Database(file, { readonly: true });
            try { return JSON.stringify(db.prepare("SELECT * FROM __drizzle_migrations ORDER BY created_at").all()); } finally { db.close(); }
        }
        const migratedHistory = history(c.layout.databasePath);
        mark(`Real Drizzle prefix ${journal.entries.at(-1).tag} (${currentCount - 1} migrations) exported unchanged; packaged helper privately upgraded to ${currentCount} migrations and DPAPI-restored the same identity`);
        phase = "normal historical Metagame restart";
        stack = localStack(c); await stack.start();
        assert.equal(history(c.layout.databasePath), migratedHistory, "Normal Drizzle reran or altered migration history");
        const historicalAccount = await info(metadata.credential.key);
        assert.equal(historicalAccount.UserId, original.UserId); assert.equal(historicalAccount.Username, original.Username);
        await stack.stop(); stack = null;
        assert.equal(state(c.layout.databasePath), player);
        assert.deepEqual(await fingerprint(a), originalProfile); assert.deepEqual(await fingerprint(old), oldFingerprint);
        for (const profile of [a, b, c, old]) assert.ok(!(await fs.readdir(profile.layout.stateRoot)).some(name => name.startsWith(".save-")));
        mark("Normal packaged Metagame/Deploy restart accepted upgraded history without reruns; APIKEY, player state and recoveryevents preserved; both source profiles unchanged");
        if (keepRestored) { handoffReady = true; mark("Restored profile for established packaged launcher smoke: " + c.userData); }
        const phases = [];
        phase = "restored Controller startup";
        controller = new Controller({ userDataDir: b.userData, resourcesDir: resources, defaultInstallDir: gameRoot,
            hostPlatform: "win32", appVersion: "portable-smoke", packaged: true, defaultLanguage: "en",
            encryptor: b.crypto, manifest: null, runtimeFilesExist: () => true,
            offlineFreeMemory: () => os.freemem(), offlineSave: b.saves,
            chooseFolder: async () => null, chooseSaveFile: async () => null, chooseOpenFile: async () => null,
            openExternal: async () => {}, openPath: async () => {}, emitSnapshot: () => {}, emitProgress: () => {},
            emitOfflineStatus: value => phases.push(value.phase), findTailscale: () => null, findRunningClients: async () => [],
            startOfflineStack: async onPhase => {
                stack = localStack(b);
                stack.onChange(value => { if (["starting-metagame", "starting-deploy"].includes(value.state)) onPhase(value.state); });
                await stack.start();
                const owned = stack;
                return { endpoint: { host: "127.0.0.1", port: 61000, pin: null }, stop: async () => { await owned.stop(); stack = null; } };
            } });
        await controller.init();
        const start = await controller.startOffline();
        assert.ok(!phases.includes("needs-username"), "Restore requested another account");
        if (start.error?.code === "offline_insufficient_memory") mark("Controller reuse check skipped: actual free RAM below preflight threshold");
        else {
            assert.equal(start.error?.code, "game_files_invalid", "Restored account did not reach game preparation");
            mark("Controller reused the restored account without needs-username; empty game install stopped at game validation");
        }
        await controller.shutdown(); controller = null; stack = null;
        assert.equal(listeners(), "", "Shutdown left local ports occupied");
        for (const value of secretsSeen) assert.ok(!captured.join("\n").includes(value), "Credential leaked into Launcher logs");
        mark("Owned stack shutdown cleared TCP 61000/61001/61099 and UDP 8770-8777; no credentials in captured Launcher logs");
    } finally {
        if (controller) await controller.shutdown();
        else if (stack) await stack.stop();
        if (!handoffReady) await fs.rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    }
}

if (!process.versions.electron) {
    const env = { ...process.env }; delete env.ELECTRON_RUN_AS_NODE;
    const child = spawn(require("electron"), [__filename, ...process.argv.slice(2)], { env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", data => { output += data; });
    child.stderr.on("data", data => { output += data; });
    child.on("error", () => { console.error("Portable save smoke could not start Electron"); process.exitCode = 1; });
    child.on("exit", code => {
        const leaked = /UUK_[a-f0-9]{48}|\b[a-f0-9]{48}\b|-----BEGIN PRIVATE KEY-----|LS0tLS1CRUdJTiBQUklWQVRF|\beyJ[\w-]+\.[\w-]+\.[\w-]+/.test(output);
        if (!leaked) for (const line of output.split(/\r?\n/)) if (line.startsWith("[portable-save-smoke]")) console.log(line);
        if (code !== 0 || leaked) console.error("Portable save smoke failed; raw output withheld to protect credentials");
        process.exitCode = code === 0 && !leaked ? 0 : 1;
    });
} else smoke().then(() => require("electron").app.exit(0)).catch(error => {
    mark("FAILED at " + phase + " (diagnostic contents withheld)");
    for (const frame of String(error.stack).split("\n").slice(1, 4)) if (/^\s+at [A-Za-z]/.test(frame)) mark(frame.trim());
    require("electron").app.exit(1);
});
