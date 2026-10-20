import test from "node:test";
import assert from "node:assert/strict";
import { promises as fsp } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { parseEnv } from "node:util";
import { createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { resolveLocalStackPaths, localServiceEnvironment } from "../src/main/local-stack-options";
import { offlineLayout } from "../src/main/offline-layout";
import { generateOfflineSecrets, loadOfflineSecrets, offlineServiceConfig, prepareOfflineState, serializeRuntimeEnv } from "../src/main/offline-state";
import { mergeIniKeys, offlineEndpointEntries, prepareOfflineGameConfig } from "../src/main/offline-game-config";
import { decodeIni, encodeIni, splitLines } from "../src/main/engineini";
import { OFFLINE_MEMORY_POLICY, OFFLINE_REQUIRED_AVAILABLE_BYTES, hasOfflineMemory } from "../src/main/offline-memory";

const options = {
    packaged: true, platform: "win32" as NodeJS.Platform,
    resourcesPath: path.resolve("packaged-resources"), userDataDir: path.resolve("player-state"),
    appPath: path.resolve("launcher"), env: {},
};

test("Offline memory gate fails closed for invalid readings and accepts the exact byte threshold", () => {
    for (const value of [NaN, Infinity, -1, OFFLINE_REQUIRED_AVAILABLE_BYTES - 1]) assert.equal(hasOfflineMemory(value), false);
    assert.equal(hasOfflineMemory(OFFLINE_REQUIRED_AVAILABLE_BYTES), true);
    assert.equal(hasOfflineMemory(OFFLINE_REQUIRED_AVAILABLE_BYTES + 1), true);
});

test("packaged Windows runtime resolves external code and userData state independently", () => {
    const result = resolveLocalStackPaths(options)!;
    const layout = offlineLayout(options.resourcesPath, options.userDataDir);
    assert.deepEqual(result.layout, layout);
    assert.equal(result.runnerPath, path.join(options.resourcesPath, "offline-runtime", "local-service-runner.cjs"));
    for (const name of ["metagame", "deploy"] as const) {
        assert.equal(result.services[name].cwd, path.join(layout.runtimeRoot, name));
        assert.equal(result.services[name].serverPath, path.join(layout.runtimeRoot, name, "dist", "server.js"));
        assert.equal(result.services[name].envPath, path.join(options.userDataDir, "offline", "runtime", `${name}.env`));
    }
    assert.equal(layout.databasePath, path.join(options.userDataDir, "offline", "undaunted.db"));
    assert.equal(result.services.metagame.healthPath, "/undaunted/api/RegistrationStatus");
    assert.equal(result.services.deploy.healthPath, "/gameservers");
    assert.equal(result.services.metagame.readyTimeoutMs, 90_000);
    assert.equal(result.services.deploy.readyTimeoutMs, 20_000);
});

test("packaged unsupported platforms reject local stacks even with development opt-in", () => {
    for (const platform of ["linux", "darwin"] as const) {
        assert.equal(resolveLocalStackPaths({ ...options, platform, env: { DAUNTLESS_REVIVED_LOCAL_STACK_DEV: "1" } }), null);
    }
});

test("ordinary development remains online-only; explicit local paths still work", () => {
    assert.equal(resolveLocalStackPaths({ ...options, packaged: false }), null);
    const env = {
        DAUNTLESS_REVIVED_LOCAL_STACK_DEV: "1",
        DAUNTLESS_REVIVED_LOCAL_METAGAME_DIR: path.resolve("dev-meta"),
        DAUNTLESS_REVIVED_LOCAL_DEPLOY_DIR: path.resolve("dev-deploy"),
        DAUNTLESS_REVIVED_LOCAL_METAGAME_ENV: path.resolve("meta.env"),
        DAUNTLESS_REVIVED_LOCAL_DEPLOY_ENV: path.resolve("deploy.env"),
    };
    const result = resolveLocalStackPaths({ ...options, packaged: false, env })!;
    assert.equal(result.layout, undefined);
    assert.equal(result.services.metagame.readyTimeoutMs, undefined);
    assert.equal(result.services.deploy.readyTimeoutMs, undefined);
    assert.equal(result.services.metagame.cwd, env.DAUNTLESS_REVIVED_LOCAL_METAGAME_DIR);
    assert.equal(result.services.deploy.envPath, env.DAUNTLESS_REVIVED_LOCAL_DEPLOY_ENV);
    assert.equal(result.runnerPath, path.join(options.appPath, "runtime", "local-service-runner.cjs"));
    assert.throws(() => resolveLocalStackPaths({ ...options, packaged: false, env: { DAUNTLESS_REVIVED_LOCAL_STACK_DEV: "1" } }));
});

test("packaged children cannot inherit shell server config or Node injection", () => {
    assert.deepEqual(localServiceEnvironment({
        Path: "system-path", SYSTEMROOT: "windows", LOCALAPPDATA: "profile",
        AUTH_MODE: "NONE", BIND_HOST: "0.0.0.0", DB_FILENAME: "other.db", CHAT_BIND_HOST: "0.0.0.0",
        NODE_OPTIONS: "--require arbitrary.js", GATEWAY_SECRET: "unexpected", METAGAME_API_KEY: "unexpected",
    }), { Path: "system-path", SYSTEMROOT: "windows", LOCALAPPDATA: "profile" });
});

test("offline config uses loopback, a persistent DB, selected executable and Deploy memory defaults", () => {
    const layout = offlineLayout(options.resourcesPath, options.userDataDir);
    const exe = path.resolve("Games With Spaces", "Dauntless-Win64-Shipping.exe");
    const secrets = generateOfflineSecrets();
    const config = offlineServiceConfig(layout, exe, secrets);
    assert.equal(config.metagame.BIND_HOST, "127.0.0.1");
    assert.equal(config.metagame.AUTH_MODE, "APIKEY");
    assert.equal(config.metagame.REGISTRATION_MODE, "OPEN");
    assert.equal(config.metagame.DB_FILENAME, layout.databasePath.replace(/\\/g, "/"));
    assert.equal(config.metagame.CHAT_BIND_HOST, "127.0.0.1");
    assert.equal(config.metagame.DEPLOYSERVER_URL, "127.0.0.1:61001");
    assert.equal(config.metagame.QOS_TARGET_URL, "http://127.0.0.1:61000/QoS");
    assert.equal(config.deploy.BIND_HOST, "127.0.0.1");
    assert.equal(config.deploy.MY_IP, "127.0.0.1");
    assert.equal(config.deploy.GAMESERVER_BINARY_PATH, exe.replace(/\\/g, "/"));
    assert.equal(config.deploy.METAGAME_API_KEY, secrets.gameserverKey);
    assert.equal(config.deploy.PORT_RANGE_BEGIN, "8770");
    assert.equal(config.deploy.PORT_RANGE_END, "8777");
    assert.ok(!("GAMESERVER_MEMORY_GUARD" in config.deploy));
    assert.equal(config.deploy.GAMESERVER_MIN_FREE_MB, String(OFFLINE_MEMORY_POLICY.minFreeMiB));
    assert.equal(config.deploy.GAMESERVER_STARTUP_MB, String(OFFLINE_MEMORY_POLICY.serverStartupMiB));
    assert.equal(config.deploy.GAMESERVER_HUNT_STARTUP_MB, String(OFFLINE_MEMORY_POLICY.serverStartupMiB));
    assert.equal(config.deploy.DR_SHOW_CONSOLE, "0");
    assert.equal(offlineServiceConfig(layout, exe, secrets, true).deploy.DR_SHOW_CONSOLE, "1");
    const message = Buffer.from("local-session");
    const signature = sign("sha256", message, createPrivateKey(Buffer.from(secrets.signingPrivateKey, "base64")));
    assert.ok(verify("sha256", message, createPublicKey(Buffer.from(secrets.signingPublicKey, "base64")), signature));
    assert.notEqual(generateOfflineSecrets().gameserverKey, secrets.gameserverKey);
});

test("env serialization round-trips Windows paths, spaces, hashes and quotes without injection", () => {
    const values = { PATH_VALUE: 'C:\\Games With Spaces\\Slayer\'s # game.exe', OTHER: 'a"b', BOTH: 'a"b\'c' };
    assert.deepEqual(parseEnv(serializeRuntimeEnv(values)), values);
    assert.throws(() => serializeRuntimeEnv({ VALUE: "value\nBIND_HOST=0.0.0.0" }));
    assert.throws(() => serializeRuntimeEnv({ "INVALID=KEY": "value" }));
});

test("state preparation preserves secrets and database, and refreshes env paths", async () => {
    const directory = await fsp.mkdtemp(path.join(tmpdir(), "dr-offline-state-"));
    try {
        const layout = offlineLayout(path.join(directory, "resources"), path.join(directory, "userData"));
        const first = await loadOfflineSecrets(layout.stateRoot);
        await fsp.writeFile(layout.databasePath, "persistent player data");
        const keyFile = await prepareOfflineState(layout, path.join(directory, "first game.exe"));
        assert.equal(await fsp.readFile(keyFile, "utf8"), first.gameserverKey);
        await fsp.writeFile(layout.metagameEnvPath, "BIND_HOST=0.0.0.0\n");
        await prepareOfflineState(layout, path.join(directory, "selected game.exe"));
        assert.deepEqual(await loadOfflineSecrets(layout.stateRoot), first);
        assert.equal(await fsp.readFile(layout.databasePath, "utf8"), "persistent player data");
        const meta = parseEnv(await fsp.readFile(layout.metagameEnvPath, "utf8"));
        const deploy = parseEnv(await fsp.readFile(layout.deployEnvPath, "utf8"));
        assert.equal(meta.BIND_HOST, "127.0.0.1");
        assert.equal(meta.DB_FILENAME, layout.databasePath.replace(/\\/g, "/"));
        assert.equal(deploy.GAMESERVER_BINARY_PATH, path.join(directory, "selected game.exe").replace(/\\/g, "/"));
        await fsp.writeFile(path.join(layout.stateRoot, "secrets.json"), "invalid-private-content");
        await assert.rejects(loadOfflineSecrets(layout.stateRoot), /refusing to replace/);
        assert.equal(await fsp.readFile(path.join(layout.stateRoot, "secrets.json"), "utf8"), "invalid-private-content");
    } finally {
        await fsp.rm(directory, { recursive: true, force: true });
    }
});

test("server endpoint merges are quoted, idempotent and preserve unrelated keys and sections", async () => {
    const endpoints = offlineEndpointEntries({ AuthEndpoint: "/game/login", MatchmakingEndpoint: "", GuildEndpoint_v2: "/guild" });
    const original = ["[Other]", "PlayerPreference=ä", "[OnlineSubsystemPhoenix]", "AuthEndpoint=old", "CustomValue=kept",
        "[OnlineSubsystemPhoenix]", "AuthEndpoint=duplicate", "[Tail]", "AuthEndpoint=unrelated"];
    const merged = mergeIniKeys(original, "OnlineSubsystemPhoenix", endpoints);
    assert.ok(merged.includes('AuthEndpoint="http://127.0.0.1:61000/game/login"'));
    assert.ok(merged.includes('MatchmakingEndpoint="http://127.0.0.1:61000"'));
    assert.ok(merged.includes("CustomValue=kept"));
    assert.ok(merged.includes("AuthEndpoint=unrelated"));
    assert.ok(!merged.includes("AuthEndpoint=duplicate"));
    assert.equal(merged.filter(line => line === "[OnlineSubsystemPhoenix]").length, 1);
    assert.deepEqual(mergeIniKeys(merged, "OnlineSubsystemPhoenix", endpoints), merged);
    assert.throws(() => offlineEndpointEntries({ AuthEndpoint: '/unsafe"\nkey=value', MatchmakingEndpoint: "" }));
    const directory = await fsp.mkdtemp(path.join(tmpdir(), "dr-offline-config-"));
    try {
        const table = path.join(directory, "endpoints.json");
        await fsp.writeFile(table, JSON.stringify({ AuthEndpoint: "/game/login", MatchmakingEndpoint: "" }));
        await fsp.writeFile(path.join(directory, "Game.ini"), encodeIni(original));
        await fsp.writeFile(path.join(directory, "Engine.ini"), "[SystemSettings]\r\nr.Custom=keep\r\n");
        await prepareOfflineGameConfig(directory, table);
        const firstGame = await fsp.readFile(path.join(directory, "Game.ini"));
        const firstEngine = await fsp.readFile(path.join(directory, "Engine.ini"));
        await prepareOfflineGameConfig(directory, table);
        assert.deepEqual(await fsp.readFile(path.join(directory, "Game.ini")), firstGame);
        assert.deepEqual(await fsp.readFile(path.join(directory, "Engine.ini")), firstEngine);
        const saved = splitLines(decodeIni(await fsp.readFile(path.join(directory, "Game.ini"))));
        assert.ok(saved.includes("PlayerPreference=ä"));
        assert.ok(saved.includes("CustomValue=kept"));
        const engine = await fsp.readFile(path.join(directory, "Engine.ini"), "utf8");
        assert.ok(engine.includes("r.Custom=keep"));
        assert.ok(engine.includes('ServerAddr="ws://127.0.0.1"'));
    } finally {
        await fsp.rm(directory, { recursive: true, force: true });
    }
});
