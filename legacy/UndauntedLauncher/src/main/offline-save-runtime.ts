import { utilityProcess } from "electron";
import { promises as fsp } from "node:fs";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { OfflineSaveInvalid, type PortableIdentity } from "./offline-save-format";
import type { OfflineSaveDatabase } from "./offline-save";
import { localServiceEnvironment } from "./local-stack-options";

export function electronSaveDatabase(runtimeRoot: string, metagameDir: string): OfflineSaveDatabase {
    async function run(work: string, job: Record<string, unknown>) {
        const file = path.join(work, randomUUID() + ".job.json");
        const result = file + ".result";
        await fsp.writeFile(file, JSON.stringify({ ...job, result, metagameDir }), { mode: 0o600, flag: "wx" });
        // Only the private job filename crosses argv; secret material stays in files.
        await new Promise<void>((resolve, reject) => {
            const child = utilityProcess.fork(path.join(runtimeRoot, "offline-save-db.cjs"), [file], {
                env: localServiceEnvironment(process.env), stdio: "ignore", serviceName: "Dauntless Revived portable Offline save",
            });
            let timedOut = false;
            const timer = setTimeout(() => { timedOut = true; child.kill(); }, 60000);
            // Wait for exit before the caller removes private files/native DB handles.
            child.once("exit", code => { clearTimeout(timer); if (code === 0 && !timedOut) resolve(); else reject(new OfflineSaveInvalid()); });
        });
        const data = JSON.parse(await fsp.readFile(result, "utf8"));
        if (data.ok !== true) {
            if (data.error === "io") throw new Error("Offline save I/O failed");
            throw new OfflineSaveInvalid();
        }
        return data;
    }
    return {
        exportCopy: async (work, source, database, key) => (await run(work, { operation: "export", source, database, key })).identity as PortableIdentity,
        validate: async (work, database, identity) => { await run(work, { operation: "validate", database, key: identity.key, account: { userId: identity.userId, username: identity.username } }); },
    };
}
