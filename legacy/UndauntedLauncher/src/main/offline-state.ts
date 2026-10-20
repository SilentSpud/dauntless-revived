import { createPrivateKey, createPublicKey, generateKeyPairSync, randomBytes } from "node:crypto";
import { promises as fsp } from "node:fs";
import path from "node:path";
import type { OfflineLayout } from "./offline-layout";
import { XMPP_PORT } from "./constants";
import { OFFLINE_MEMORY_POLICY } from "./offline-memory";

export interface OfflineSecrets {
    signingPrivateKey: string;
    signingPublicKey: string;
    gameserverKey: string;
}

export function generateOfflineSecrets(): OfflineSecrets {
    const pair = generateKeyPairSync("rsa", {
        modulusLength: 2048,
        privateKeyEncoding: { type: "pkcs8", format: "pem" },
        publicKeyEncoding: { type: "spki", format: "pem" },
    });
    return {
        signingPrivateKey: Buffer.from(pair.privateKey).toString("base64"),
        signingPublicKey: Buffer.from(pair.publicKey).toString("base64"),
        gameserverKey: randomBytes(24).toString("hex"),
    };
}

function validateSecrets(value: OfflineSecrets): void {
    try {
        if (!/^[0-9a-f]{48}$/.test(value.gameserverKey)) throw new Error();
        const privateKey = createPrivateKey(Buffer.from(value.signingPrivateKey, "base64"));
        const publicKey = createPublicKey(Buffer.from(value.signingPublicKey, "base64"));
        if (privateKey.asymmetricKeyType !== "rsa" || (privateKey.asymmetricKeyDetails?.modulusLength ?? 0) < 2048) throw new Error();
        const derived = createPublicKey(privateKey).export({ type: "spki", format: "pem" });
        if (derived !== publicKey.export({ type: "spki", format: "pem" })) throw new Error();
    } catch {
        throw new Error("Offline secrets are invalid; refusing to replace the saved identity");
    }
}

export async function loadOfflineSecrets(stateRoot: string): Promise<OfflineSecrets> {
    await fsp.mkdir(stateRoot, { recursive: true });
    const file = path.join(stateRoot, "secrets.json");
    try {
        const saved: OfflineSecrets = JSON.parse(await fsp.readFile(file, "utf8"));
        validateSecrets(saved);
        return saved;
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
            // JSON/crypto errors must not include key material in startup logs.
            throw new Error("Could not read offline secrets; refusing to replace the saved identity");
        }
    }
    const generated = generateOfflineSecrets();
    // A complete temporary file is published atomically without overwriting another
    // launch's identity. Windows access follows the current user's profile ACL.
    const temporary = `${file}.${randomBytes(8).toString("hex")}.tmp`;
    await fsp.writeFile(temporary, JSON.stringify(generated), { mode: 0o600, flag: "wx" });
    try {
        await fsp.link(temporary, file);
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    } finally {
        await fsp.unlink(temporary);
    }
    return loadOfflineSecrets(stateRoot);
}

export function serializeRuntimeEnv(values: Record<string, string>): string {
    return Object.entries(values).map(([key, value]) => {
        if (!/^[A-Z][A-Z0-9_]*$/.test(key) || /[\r\n\0]/.test(value)) throw new Error("Invalid runtime environment value");
        // Node dotenv supports literal quoted values, not JSON backslash escaping.
        const quote = ["'", '"', "`"].find((candidate) => !value.includes(candidate));
        if (!quote) throw new Error("Runtime environment value cannot be quoted safely");
        return `${key}=${quote}${value}${quote}\n`;
    }).join("");
}

export function offlineServiceConfig(layout: OfflineLayout, gameExecutable: string, secrets: OfflineSecrets, serverConsole = false) {
    if (!path.isAbsolute(gameExecutable) || !path.isAbsolute(layout.databasePath)) throw new Error("Offline runtime paths must be absolute");
    const absolute = (value: string) => value.replace(/\\/g, "/");
    return {
        metagame: {
            NODE_ENV: "production", PORT: "61000", BIND_HOST: "127.0.0.1",
            AUTH_MODE: "APIKEY", REGISTRATION_MODE: "OPEN",
            AUTH_SIGNING_PRIVKEY_B64: secrets.signingPrivateKey,
            AUTH_SIGNING_PUBKEY_B64: secrets.signingPublicKey,
            DB_FILENAME: absolute(layout.databasePath),
            MATCHMAKING_MODE: "DEPLOYSERVER", DEPLOYSERVER_URL: "127.0.0.1:61001",
            QOS_TARGET_URL: "http://127.0.0.1:61000/QoS", TARGET_CHANGELIST: "239827",
            CHAT: "1", CHAT_BIND_HOST: "127.0.0.1", CHAT_PORT: String(XMPP_PORT),
            LOG_BODIES: "0", ACCOUNT_KEY_RECOVERY: "0",
        },
        deploy: {
            NODE_ENV: "production", PORT: "61001", BIND_HOST: "127.0.0.1", MY_IP: "127.0.0.1",
            PORT_RANGE_BEGIN: "8770", PORT_RANGE_END: "8777",
            GAMESERVER_BINARY_PATH: absolute(gameExecutable), METAGAME_API_KEY: secrets.gameserverKey,
            RAMSGATE_POOL: "1", ENABLE_DOJO: "0", SERVER_ROLE: "game",
            SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP: "10",
            GAMESERVER_READY_DIR: absolute(layout.readyDir),
            GAMESERVER_MIN_FREE_MB: String(OFFLINE_MEMORY_POLICY.minFreeMiB),
            GAMESERVER_STARTUP_MB: String(OFFLINE_MEMORY_POLICY.serverStartupMiB),
            GAMESERVER_HUNT_STARTUP_MB: String(OFFLINE_MEMORY_POLICY.serverStartupMiB),
            DR_SHOW_CONSOLE: serverConsole ? "1" : "0",
        },
    };
}

async function writeRuntimeFile(file: string, contents: string): Promise<void> {
    const temporary = `${file}.tmp`;
    await fsp.writeFile(temporary, contents, { mode: 0o600 });
    await fsp.rename(temporary, file);
}

export async function prepareOfflineState(layout: OfflineLayout, gameExecutable: string, serverConsole = false): Promise<string> {
    const secrets = await loadOfflineSecrets(layout.stateRoot);
    const config = offlineServiceConfig(layout, gameExecutable, secrets, serverConsole);
    await fsp.mkdir(layout.runtimeStateDir, { recursive: true });
    await fsp.mkdir(layout.readyDir, { recursive: true });
    await writeRuntimeFile(layout.metagameEnvPath, serializeRuntimeEnv(config.metagame));
    await writeRuntimeFile(layout.deployEnvPath, serializeRuntimeEnv(config.deploy));
    const keyFile = path.join(layout.runtimeStateDir, "gameserver.key");
    await writeRuntimeFile(keyFile, secrets.gameserverKey);
    // The database belongs to Metagame; preparation never creates, truncates or replaces it.
    return keyFile;
}
