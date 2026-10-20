const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const { createHash } = require("node:crypto");
const { spawn, spawnSync, execFileSync } = require("node:child_process");
const { setTimeout: delay } = require("node:timers/promises");

const packageRoot = path.resolve(__dirname, "..", "out", "Dauntless Revived Launcher-win32-x64");
const resources = path.join(packageRoot, "resources");
const runtime = path.join(resources, "offline-runtime");
const gameDirIndex = process.argv.indexOf("--game-dir");
const selectedGame = gameDirIndex === -1 ? null : process.argv[gameDirIndex + 1];
const restoredIndex = process.argv.indexOf("--restored-profile");
const restoredProfile = restoredIndex === -1 ? null : path.resolve(process.argv[restoredIndex + 1]);
const expectedUsername = restoredProfile ? "PortableSmoke" : "OfflineSmoke_1";
const listenerScript = 'Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in 61000,61001,61099 } | Select-Object LocalAddress,LocalPort | ConvertTo-Json -Compress; Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -ge 8770 -and $_.LocalPort -le 8777 } | Select-Object LocalAddress,LocalPort | ConvertTo-Json -Compress';
function listeners() {
    return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", listenerScript], { encoding: "utf8", windowsHide: true }).trim();
}
async function waitFor(check, timeout = 30000) {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
        const result = await check();
        if (result) return result;
        await delay(200);
    }
    throw new Error("Smoke check timed out");
}

async function main() {
    for (const file of ["local-service-runner.cjs", "dr-db.js", "backup-policy.cjs", "game-endpoints.json",
        "metagame/dist/server.js", "deploy/dist/server.js",
        "metagame/node_modules/better-sqlite3/build/Release/better_sqlite3.node",
        "deploy/node_modules/better-sqlite3/build/Release/better_sqlite3.node"]) {
        await fs.access(path.join(runtime, file));
    }
    for (const file of ["dxgi.dll", "UndauntedInternalServer.dll", "icon.png", "THIRD-PARTY-NOTICES.txt", "client-mods", "app.asar"]) await fs.access(path.join(resources, file));
    console.log("Physical packaged runtime, both native addons and existing resources verified");
    assert.equal(listeners(), "", "Required local ports were occupied before this test");
    const root = restoredProfile ? path.dirname(restoredProfile) : await fs.mkdtemp(path.join(os.tmpdir(), "dr-packaged-offline-"));
    // Only the disposable profile handed off by portable-save-smoke may be removed here.
    if (restoredProfile) assert.ok(path.basename(root).startsWith("dr-portable-smoke-") && path.dirname(root).toLowerCase() === os.tmpdir().toLowerCase() && path.basename(restoredProfile) === "historical-destination");
    const profile = restoredProfile || path.join(root, "profile");
    const local = path.join(root, "local");
    await fs.mkdir(profile, { recursive: true });
    await fs.mkdir(local, { recursive: true });
    let socket, child, evaluate;
    let childExited = false;
    let successful = false;
    let exitCode;
    let nativeFailed = false;
    let capturedOutput = "";
    let initialIdentity, encryptedKey;
    const keySlot = createHash("sha256").update("offline-local:61000").digest("hex").slice(0, 24);
    const keyFile = path.join(profile, "keys", keySlot + ".key");
    if (restoredProfile) encryptedKey = await fs.readFile(keyFile);
    try {
        child = spawn(path.join(packageRoot, "DauntlessRevivedLauncher.exe"), [
            "--user-data-dir=" + profile, "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", "--safe-mode",
        ], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"], env: {
            ...process.env, LOCALAPPDATA: local, APPDATA: path.join(root, "roaming"),
            DAUNTLESS_REVIVED_LOCAL_SERVER_CONSOLE: "0",
            BIND_HOST: "0.0.0.0", AUTH_MODE: "NONE", CHAT_BIND_HOST: "0.0.0.0",
        } });
        child.once("exit", (code) => { childExited = true; exitCode = code; });
        child.stderr.on("data", data => { capturedOutput += data; });
        child.stdout.on("data", data => { capturedOutput += data; });
        const port = await waitFor(async () => {
            if (childExited) throw new Error("Packaged launcher exited before debugging became ready");
            try { return Number((await fs.readFile(path.join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0]); } catch { return false; }
        });
        const page = await waitFor(async () => {
            const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
            return pages.find((page) => page.type === "page" && page.url.startsWith("file:"));
        });
        socket = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, { once: true }); socket.addEventListener("error", reject, { once: true }); });
        let nextId = 0;
        const pending = new Map();
        socket.addEventListener("message", (event) => {
            const data = JSON.parse(event.data);
            if (data.id && pending.has(data.id)) { pending.get(data.id)(data); pending.delete(data.id); }
        });
        evaluate = async (expression) => {
            const id = ++nextId;
            const response = new Promise((resolve) => pending.set(id, resolve));
            socket.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
            let timer;
            const timeout = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("Renderer test call timed out")), 120000); });
            const data = await Promise.race([response, timeout]).finally(() => clearTimeout(timer));
            if (data.error || data.result.exceptionDetails) throw new Error("Renderer smoke expression failed");
            return data.result.result.value;
        };
        await waitFor(async () => evaluate('typeof window.launcher === "object"'));
        await fs.access(path.join(profile, "logs", "launcher.log"));
        const snapshot = await evaluate("window.launcher.getSnapshot()");
        assert.equal(snapshot.settings.showConsole, false, "Fresh profiles must not enable client consoles");
        assert.ok(snapshot.install.dir.startsWith(local), "Selected game path must belong to the isolated profile");
        await evaluate('window.smokePhases=[]; window.launcher.onOfflineStatus(s=>window.smokePhases.push(s.phase)); true');
        console.log("Packaged launcher started with isolated userData and game config profile");
        const secretsFile = path.join(profile, "offline", "secrets.json");
        let initialSecrets;
        for (let cycle = 0; cycle < 2; cycle++) {
            // A second launch uses the same profile, including DPAPI storage and DB.
            // The empty game install deliberately stops at real game validation.
            const result = await evaluate("window.launcher.startOffline()");
            if (result.error?.code === "offline_insufficient_memory") {
                assert.equal(await evaluate("window.smokePhases.at(-1)"), "error");
                assert.ok(!(await evaluate("window.smokePhases")).includes("starting-metagame"));
                if (cycle === 0 && !restoredProfile) await assert.rejects(fs.access(path.join(profile, "offline")));
                assert.equal(listeners(), "");
                assert.deepEqual(await evaluate("window.launcher.stopOffline()"), { ok: true });
                console.log("Packaged memory preflight rejected before starting services; full runtime smoke blocked by available RAM");
                socket.send(JSON.stringify({ id: ++nextId, method: "Runtime.evaluate", params: { expression: "window.close()" } }));
                await waitFor(() => childExited, 15000);
                assert.equal(exitCode, 0);
                successful = true;
                process.exitCode = 2;
                return;
            }
            if (cycle === 0 && !restoredProfile) {
                assert.deepEqual(result, { ok: true });
                assert.equal(await evaluate("window.smokePhases.at(-1)"), "needs-username");
                assert.equal((await evaluate('window.launcher.startOffline("!")')).error.code, "username_invalid");
                const registered = await evaluate('window.launcher.startOffline("OfflineSmoke_1")');
                assert.equal(registered.error?.code, "game_files_invalid", "Registration must proceed to actual game validation");
                encryptedKey = await fs.readFile(keyFile);
                assert.ok(!encryptedKey.includes(Buffer.from("UUK_")), "Player key must be encrypted");
            } else {
                assert.equal(result.error?.code, "game_files_invalid", "Existing account must proceed without username selection");
                assert.ok(!(await evaluate("window.smokePhases")).includes("needs-username"));
                assert.ok(encryptedKey.equals(await fs.readFile(keyFile)), "Stable offline key slot must be reused");
            }
            // Allow owned local services to finish releasing their listeners before checking cleanup.
            const active = await waitFor(() => { const value = listeners(); return value === "" ? { value } : false; }, 5000).then(result => result.value);
            assert.equal(active, "", "Game validation failure must release the owned local stack");
            assert.ok(!active.includes("0.0.0.0"));
            const secrets = await fs.readFile(secretsFile, "utf8");
            const deployConfig = require("node:util").parseEnv(await fs.readFile(path.join(profile, "offline", "runtime", "deploy.env"), "utf8"));
            const { OFFLINE_MEMORY_POLICY } = require("jiti")(__filename)("../src/main/offline-memory.ts");
            assert.equal(deployConfig.DR_SHOW_CONSOLE, "0");
            assert.equal(deployConfig.GAMESERVER_MIN_FREE_MB, String(OFFLINE_MEMORY_POLICY.minFreeMiB));
            assert.equal(deployConfig.GAMESERVER_STARTUP_MB, String(OFFLINE_MEMORY_POLICY.serverStartupMiB));
            assert.equal(deployConfig.GAMESERVER_HUNT_STARTUP_MB, String(OFFLINE_MEMORY_POLICY.serverStartupMiB));
            if (initialSecrets) assert.ok(secrets === initialSecrets, "Runtime secrets must survive restart");
            else initialSecrets = secrets;
            const verification = `const assert=require("node:assert/strict"),fs=require("node:fs"),path=require("node:path"),crypto=require("node:crypto"); assert.equal(process.versions.modules,"149"); const state=${JSON.stringify(path.join(profile, "offline"))}; const runtime=${JSON.stringify(runtime)}; const key=JSON.parse(fs.readFileSync(path.join(state,"secrets.json"),"utf8")).gameserverKey; let identity; for(const name of ["metagame","deploy"]){const Database=require(path.join(runtime,name,"node_modules","better-sqlite3"));const memory=new Database(":memory:");memory.exec("CREATE TABLE test(id INTEGER)");memory.close(); if(name==="metagame"){const db=new Database(path.join(state,"undaunted.db"),{readonly:true,fileMustExist:true}); try {assert.equal(db.prepare("SELECT count(*) AS n FROM gameserverapikeys WHERE keyHash = ?").get(crypto.createHash("sha256").update(key).digest("hex")).n,1); const users=db.prepare("SELECT userId,name,notes FROM users ORDER BY userId").all(); assert.equal(users.length,1); assert.equal(users[0].name,${JSON.stringify(expectedUsername)}); assert.equal(db.prepare("SELECT count(*) AS n FROM userapikeys").get().n,1); identity=crypto.createHash("sha256").update(JSON.stringify([users,db.prepare("SELECT * FROM characters ORDER BY characterId").all(),db.prepare("SELECT * FROM userapikeys ORDER BY userId").all()])).digest("hex");}finally{db.close();}}} console.log(JSON.stringify({abi:process.versions.modules,identity}));`;
            const native = spawnSync(require("electron"), ["-e", verification], { encoding: "utf8", env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" } });
            assert.equal(native.status, 0, "Native addon and credential verification failed");
            const identity = JSON.parse(native.stdout.trim());
            if (initialIdentity) assert.equal(identity.identity, initialIdentity);
            else initialIdentity = identity.identity;
            console.log(`Run ${cycle + 1}: one local account, encrypted stable key slot, unchanged player/progression identity, native ABI ${identity.abi}`);
            assert.deepEqual(await evaluate("window.launcher.stopOffline()"), { ok: true });
            await waitFor(() => listeners() === "");
            console.log(`Cycle ${cycle + 1}: packaged Metagame and Deploy started and stopped; TCP 61000/61001/61099 and UDP 8770-8777 are clear`);
            if (cycle === 0) {
                socket.send(JSON.stringify({ id: ++nextId, method: "Runtime.evaluate", params: { expression: "window.close()" } }));
                await waitFor(() => childExited, 15000);
                socket.close();
                // Relaunch the actual packaged exe; reuse only mutable player state.
                childExited = false;
                await fs.rm(path.join(profile, "DevToolsActivePort"), { force: true });
                child = spawn(path.join(packageRoot, "DauntlessRevivedLauncher.exe"), ["--user-data-dir=" + profile, "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", "--safe-mode"], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, LOCALAPPDATA: local, APPDATA: path.join(root, "roaming"), DAUNTLESS_REVIVED_LOCAL_SERVER_CONSOLE: "0" } });
                child.once("exit", code => { childExited = true; exitCode = code; });
                child.stderr.on("data", data => { capturedOutput += data; });
                child.stdout.on("data", data => { capturedOutput += data; });
                const restartedPort = await waitFor(async () => {
                    try { return Number((await fs.readFile(path.join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0]); } catch { return false; }
                });
                const restartedPage = await waitFor(async () => (await (await fetch(`http://127.0.0.1:${restartedPort}/json/list`)).json()).find(page => page.type === "page" && page.url.startsWith("file:")));
                socket = new WebSocket(restartedPage.webSocketDebuggerUrl);
                await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, { once: true }); socket.addEventListener("error", reject, { once: true }); });
                socket.addEventListener("message", event => {
                    const data = JSON.parse(event.data);
                    if (data.id && pending.has(data.id)) { pending.get(data.id)(data); pending.delete(data.id); }
                });
                await waitFor(async () => evaluate('typeof window.launcher === "object"'));
                await evaluate('window.smokePhases=[]; window.launcher.onOfflineStatus(s=>window.smokePhases.push(s.phase)); true');
            }
        }
        const endpoints = await fs.readFile(path.join(local, "Archon", "Saved", "Config", "WindowsClient", "Game.ini"), "utf8");
        assert.equal((endpoints.match(/="http:\/\/127\.0\.0\.1:61000/g) || []).length, 167);
        assert.ok(endpoints.includes('PhoenixEventsMessageEndpoint="http://127.0.0.1:61000/offline-events"'));
        console.log("All 167 server endpoints generated; secrets preserved across both starts");
        if (selectedGame) {
            const jiti = require("jiti")(__filename);
            const { locateExistingGame } = jiti("../src/main/game-folder.ts");
            const { verifyInstall, hashFile } = jiti("../src/main/verify.ts");
            const { EXE_RELATIVE_PATH, PINNED_EXE_SHA256 } = jiti("../src/main/constants.ts");
            const found = await locateExistingGame(selectedGame);
            assert.ok(found.ok, "Installed game discovery failed");
            const manifest = JSON.parse(await fs.readFile(path.resolve(__dirname, "../../UndauntedContent/data/dauntless-1.4.4.json"), "utf8"));
            assert.equal((await verifyInstall(found.root, manifest.files, { fullHash: false })).bad.length, 0, "Installed game is missing or invalid");
            assert.equal(await hashFile(path.join(found.root, EXE_RELATIVE_PATH)), PINNED_EXE_SHA256);
            // The existing action stores this selection even when online repair has
            // no selected server. Offline launch performs its own pinned validation.
            const selection = await evaluate(`window.launcher.useExistingGamePath(${JSON.stringify(found.root)})`);
            assert.ok(selection.ok || selection.error?.code === "server_unreachable");
            assert.equal((await evaluate("window.launcher.getSnapshot()")).install.dir, found.root);
            await evaluate("window.smokePhases=[]; true");
            assert.deepEqual(await evaluate("window.launcher.startOffline()"), { ok: true });
            assert.equal(await evaluate("window.smokePhases.at(-1)"), "running");
            console.log("Existing validated game selected through preload; actual packaged client launch reached running");
            const probeEnv = { ...process.env };
            delete probeEnv.ELECTRON_RUN_AS_NODE;
            const probe = spawnSync(require("electron"), ["--user-data-dir=" + profile, path.join(__dirname, "offline-account-probe.cjs"), profile, runtime], { encoding: "utf8", windowsHide: true, timeout: 180000, env: probeEnv });
            console.log(probe.stdout.trim());
            const diagnostic = probe.stderr.match(/Offline account\/native allocation probe failed at [^\r\n]*/)?.[0];
            if (diagnostic) console.log(diagnostic);
            if (probe.status !== 0) {
                nativeFailed = true;
                console.log(`Native allocation did not reach readiness (probe exit ${probe.status}); memory guard and authentication remain enabled`);
                console.log(JSON.stringify({ memoryRefusal: /capacity_unavailable|CapacityUnavailable|not enough.*memory/i.test(capturedOutput),
                    nativeExits: [...capturedOutput.matchAll(/Game server on port (\d+) \(pid (\d+)\) exited (with code -?\d+|on [A-Z]+)/g)].map(match => ({ port: Number(match[1]), pid: Number(match[2]), result: match[3] })),
                    nativeStartupFailure: /Could not start a game server|exited before becoming ready|no_game_server/i.test(capturedOutput) }));
            }
            else assert.ok(listeners().includes('"LocalPort":' + JSON.parse(probe.stdout.trim()).port), "Allocated native UDP listener missing");
            // Close only this launcher's client, gracefully. Never kill by image name.
            const closeScript = `$ownedClients = Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.ParentProcessId -eq ${child.pid} -and $_.Name -eq 'Dauntless-Win64-Shipping.exe' -and $_.CommandLine -notmatch '(^|\\s)-server(\\s|$)' }; foreach ($ownedClient in $ownedClients) { $ownedProcess = Get-Process -Id $ownedClient.ProcessId -ErrorAction SilentlyContinue; if ($ownedProcess) { [void]$ownedProcess.CloseMainWindow() } }; exit 0`;
            await waitFor(async () => {
                if ((await evaluate("window.smokePhases.at(-1)")) === "stopped") return true;
                execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", closeScript], { windowsHide: true });
                return false;
            }, 60000);
            await waitFor(() => listeners() === "");
            console.log("Owned client closed gracefully; Controller stopped Deploy/native children and Metagame; all local ports clear");
        }
        const log = capturedOutput + await fs.readFile(path.join(profile, "logs", "launcher.log"), "utf8");
        const secrets = JSON.parse(initialSecrets);
        assert.ok(!/UUK_[a-f0-9]{48}/i.test(log), "Player key leaked into logs");
        for (const secret of [secrets.gameserverKey, secrets.signingPrivateKey]) assert.ok(!log.includes(secret), "Runtime secret leaked into logs");
        console.log("Captured launcher/service output contains no player key or generated secrets");
        socket.send(JSON.stringify({ id: ++nextId, method: "Runtime.evaluate", params: { expression: "window.close()" } }));
        await waitFor(() => childExited, 15000);
        assert.equal(exitCode, 0);
        assert.equal(listeners(), "");
        console.log("Packaged launcher exited cleanly; no local service listeners remain");
        successful = true;
        if (nativeFailed) {
            process.exitCode = 1;
            console.log("Native diagnostic profile retained at " + root);
        }
    } catch (error) {
        console.error("Isolated diagnostic profile retained at " + root);
        throw error;
    } finally {
        if (!childExited && evaluate) {
            try { await evaluate("window.launcher.stopOffline()"); } catch {}
            try { socket.send(JSON.stringify({ id: 9999, method: "Runtime.evaluate", params: { expression: "window.close()" } })); await waitFor(() => childExited, 15000); } catch {}
        }
        socket?.close();
        if (child && !childExited) child.kill();
        // Preserve evidence when startup fails; it is outside the repository and
        // no generated configuration or secrets are printed by this diagnostic.
        if (successful && !nativeFailed && childExited && listeners() === "") await fs.rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    }
}

main().catch((error) => { console.error(error.message); process.exitCode = 1; });
