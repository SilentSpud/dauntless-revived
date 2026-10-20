import { randomUUID } from "node:crypto";
import { promises as fsp } from "node:fs";
import path from "node:path";
import type { ActionResult } from "../shared/types";
import { KeyStore, KeyStoreError, serverId, type KeySlot } from "./keystore";
import type { OfflineLayout } from "./offline-layout";
import { OfflineSaveInvalid, readSave, writeSave, type PortableIdentity } from "./offline-save-format";

// Stable KeyStore namespace for the local profile; this is never used as a network host.
export const OFFLINE_KEY_SLOT: KeySlot = { host: "offline-local", port: 61000, mode: "private", fp: null };
export interface OfflineSaveRuntime {
    exportSave(file: string): Promise<ActionResult>;
    importSave(file: string): Promise<ActionResult>;
    recover(): Promise<void>;
}
export interface OfflineSaveDatabase {
    exportCopy(work: string, source: string, database: string, key: string): Promise<PortableIdentity>;
    validate(work: string, database: string, identity: PortableIdentity): Promise<void>;
}
interface Options {
    layout: OfflineLayout;
    userData: string;
    keys: Pick<KeyStore, "load" | "save">;
    database: OfflineSaveDatabase;
    isActive?(): boolean;
    replaceDatabase?(source: string, destination: string): Promise<void>;
}

async function exists(file: string): Promise<boolean> {
    try { await fsp.lstat(file); return true; }
    catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return false; throw error; }
}
async function durableCopy(source: string, target: string): Promise<void> {
    await fsp.copyFile(source, target);
    await fsp.chmod(target, 0o600);
    const handle = await fsp.open(target, "r+");
    try { await handle.sync(); } finally { await handle.close(); }
}
async function atomicJson(file: string, value: unknown): Promise<void> {
    const temporary = file + ".tmp";
    const handle = await fsp.open(temporary, "w", 0o600);
    try { await handle.writeFile(JSON.stringify(value)); await handle.sync(); } finally { await handle.close(); }
    await fsp.rename(temporary, file);
}

export class PortableOfflineSave implements OfflineSaveRuntime {
    private active = false;
    private readonly keyFile: string;
    private readonly transactionDir: string;
    constructor(private readonly options: Options) {
        this.keyFile = path.join(options.userData, "keys", serverId(OFFLINE_KEY_SLOT.host, OFFLINE_KEY_SLOT.port) + ".key");
        this.transactionDir = path.join(options.layout.stateRoot, ".save-restore");
    }

    private async rollback(dbExisted: boolean, keyExisted: boolean): Promise<void> {
        for (const [existed, previous, target] of [
            [dbExisted, "db.previous", this.options.layout.databasePath],
            [keyExisted, "key.previous", this.keyFile],
        ] as const) {
            if (existed) {
                const temporary = target + ".restore-tmp";
                await durableCopy(path.join(this.transactionDir, previous), temporary);
                await fsp.rename(temporary, target);
            } else await fsp.rm(target, { force: true });
        }
        await fsp.rm(this.keyFile + ".tmp", { force: true });
        // Snapshots must remain intact until retries no longer need to restore them.
        await atomicJson(path.join(this.transactionDir, "journal.json"), { version: 1, phase: "rolled-back", dbExisted, keyExisted });
    }

    async recover(): Promise<void> {
        if (this.active || this.options.isActive?.()) throw new Error("Offline save recovery is busy");
        this.active = true;
        try { await this.recoverJournal(); } finally { this.active = false; }
    }

    private async recoverJournal(): Promise<void> {
        if (await exists(this.transactionDir)) {
            if ((await fsp.lstat(this.transactionDir)).isSymbolicLink()) throw new Error("Unsafe Offline restore journal");
            const marker = path.join(this.transactionDir, "journal.json");
            if (await exists(marker)) {
                const journal = JSON.parse(await fsp.readFile(marker, "utf8"));
                if (journal.version !== 1 || !["prepared", "committed", "rolled-back"].includes(journal.phase) ||
                    typeof journal.dbExisted !== "boolean" || typeof journal.keyExisted !== "boolean") throw new Error("Invalid Offline restore journal");
                if (journal.phase === "prepared") await this.rollback(journal.dbExisted, journal.keyExisted);
            }
            // Terminal journals need cleanup only, even if a previous pass deleted snapshots.
            await fsp.rm(this.transactionDir, { recursive: true, force: true });
        }
        if (await exists(this.options.layout.stateRoot)) {
            for (const name of await fsp.readdir(this.options.layout.stateRoot)) {
                if (!/^\.save-stage-[A-Za-z0-9]+$/.test(name)) continue;
                const directory = path.join(this.options.layout.stateRoot, name);
                if ((await fsp.lstat(directory)).isSymbolicLink()) throw new Error("Unsafe Offline save staging directory");
                await fsp.rm(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
            }
        }
    }

    private async destination(file: string): Promise<string> {
        if (!path.isAbsolute(file) || path.extname(file).toLowerCase() !== ".drsave") throw new OfflineSaveInvalid();
        const resolved = path.resolve(file);
        const parent = await fsp.realpath(path.dirname(resolved));
        const canonical = path.join(parent, path.basename(resolved));
        for (const root of [this.options.userData, path.dirname(this.options.layout.runtimeRoot)]) {
            const real = await fsp.realpath(root);
            const relative = path.relative(real, canonical);
            if (relative === "" || (!relative.startsWith(".." + path.sep) && relative !== ".." && !path.isAbsolute(relative))) throw new OfflineSaveInvalid();
        }
        if (await exists(resolved)) {
            if (!(await fsp.lstat(resolved)).isFile()) throw new OfflineSaveInvalid();
        }
        return canonical;
    }

    private async operate(action: (work: string) => Promise<void>): Promise<ActionResult> {
        if (this.active || this.options.isActive?.()) return { ok: false, error: { code: "busy" } };
        this.active = true;
        let work: string | undefined;
        try {
            await fsp.mkdir(this.options.layout.stateRoot, { recursive: true, mode: 0o700 });
            await this.recoverJournal();
            work = await fsp.mkdtemp(path.join(this.options.layout.stateRoot, ".save-stage-"));
            await fsp.chmod(work, 0o700);
            await action(work);
            return { ok: true };
        } catch (error) {
            return { ok: false, error: { code: error instanceof OfflineSaveInvalid ? "offline_save_invalid" :
                error instanceof KeyStoreError ? "key_storage_unavailable" : "unknown" } };
        } finally {
            if (work) await fsp.rm(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(() => undefined);
            this.active = false;
        }
    }

    exportSave(file: string): Promise<ActionResult> {
        return this.operate(async work => {
            const destination = await this.destination(file);
            const key = await this.options.keys.load(OFFLINE_KEY_SLOT);
            if (!key) throw new OfflineSaveInvalid();
            const database = path.join(work, "portable.db");
            const identity = await this.options.database.exportCopy(work, this.options.layout.databasePath, database, key);
            const temporary = destination + "." + randomUUID() + ".tmp";
            try {
                await writeSave(temporary, database, identity);
                const verified = path.join(work, "verified.db");
                const metadata = await readSave(temporary, verified);
                await this.options.database.validate(work, verified, { ...metadata.account, key: metadata.credential.key });
                await fsp.rename(temporary, destination);
            } finally { await fsp.rm(temporary, { force: true }).catch(() => undefined); }
        });
    }

    importSave(file: string): Promise<ActionResult> {
        return this.operate(async work => {
            const database = path.join(work, "portable.db");
            let metadata;
            metadata = await readSave(file, database);
            await this.options.database.validate(work, database, { ...metadata.account, key: metadata.credential.key });
            for (const suffix of ["-wal", "-shm", "-journal"]) {
                if (await exists(this.options.layout.databasePath + suffix)) throw new OfflineSaveInvalid();
            }
            await fsp.mkdir(path.dirname(this.keyFile), { recursive: true, mode: 0o700 });
            await fsp.mkdir(this.transactionDir, { mode: 0o700 });
            const dbExisted = await exists(this.options.layout.databasePath);
            const keyExisted = await exists(this.keyFile);
            let prepared = false;
            let committed = false;
            try {
                if (dbExisted) await durableCopy(this.options.layout.databasePath, path.join(this.transactionDir, "db.previous"));
                if (keyExisted) await durableCopy(this.keyFile, path.join(this.transactionDir, "key.previous"));
                const marker = path.join(this.transactionDir, "journal.json");
                await atomicJson(marker, { version: 1, phase: "prepared", dbExisted, keyExisted });
                prepared = true;
                await (this.options.replaceDatabase ?? fsp.rename)(database, this.options.layout.databasePath);
                await this.options.keys.save(OFFLINE_KEY_SLOT, metadata.credential.key);
                if (await this.options.keys.load(OFFLINE_KEY_SLOT) !== metadata.credential.key) throw new KeyStoreError("unavailable");
                const keyHandle = await fsp.open(this.keyFile, "r+");
                try { await keyHandle.sync(); } finally { await keyHandle.close(); }
                await atomicJson(marker, { version: 1, phase: "committed", dbExisted, keyExisted });
                committed = true;
            } catch (error) {
                if (prepared) await this.rollback(dbExisted, keyExisted);
                await fsp.rm(this.transactionDir, { recursive: true, force: true });
                throw error;
            }
            if (committed) await fsp.rm(this.transactionDir, { recursive: true, force: true }).catch(() => undefined);
        });
    }
}
