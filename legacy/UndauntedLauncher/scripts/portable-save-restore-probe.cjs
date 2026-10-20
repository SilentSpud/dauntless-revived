// Destination-profile actor for portable-save-smoke; no credentials cross argv/output.
const { app, safeStorage } = require("electron");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { createHash } = require("node:crypto");
const jobFile = process.argv.at(-1);
const job = JSON.parse(require("node:fs").readFileSync(jobFile, "utf8"));
app.setPath("userData", job.userData);
let stage = "destination encryption";

async function main() {
    assert.ok(safeStorage.isEncryptionAvailable());
    assert.equal(process.versions.modules, "149");
    const ts = require("typescript");
    require.extensions[".ts"] = (module, file) => module._compile(ts.transpileModule(require("node:fs").readFileSync(file, "utf8"), {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true }, fileName: file,
    }).outputText, file);
    const { PortableOfflineSave, OFFLINE_KEY_SLOT } = require("../src/main/offline-save.ts");
    const { KeyStore } = require("../src/main/keystore.ts");
    const { offlineLayout } = require("../src/main/offline-layout.ts");
    const { electronSaveDatabase } = require("../src/main/offline-save-runtime.ts");
    const layout = offlineLayout(job.resources, job.userData);
    const keys = new KeyStore(path.join(job.userData, "keys"), {
        isAvailable: () => safeStorage.isEncryptionAvailable(), encrypt: plain => safeStorage.encryptString(plain), decrypt: data => safeStorage.decryptString(data),
    });
    async function fingerprint() {
        const files = [layout.databasePath, path.join(layout.stateRoot, "secrets.json"), path.join(job.userData, "settings.json")];
        for (const name of await fs.readdir(path.join(job.userData, "keys"))) files.push(path.join(job.userData, "keys", name));
        return Promise.all(files.sort().map(async file => [file, createHash("sha256").update(await fs.readFile(file)).digest("hex")]));
    }
    const before = await fingerprint();
    const Database = require(path.join(layout.metagameDir, "node_modules/better-sqlite3"));
    const driver = electronSaveDatabase(layout.runtimeRoot, layout.metagameDir);
    let stagedValidated = false;
    const saves = new PortableOfflineSave({ layout, userData: job.userData, keys, database: {
        ...driver, validate: async (work, file, identity) => {
            stage = "private staged migration";
            assert.equal(file, path.join(work, "portable.db"));
            await driver.validate(work, file, identity);
            assert.deepEqual(await fingerprint(), before, "Staged migration changed live destination files");
            const db = new Database(file, { readonly: true });
            try { assert.equal(db.prepare("SELECT count(*) n FROM __drizzle_migrations").get().n, job.expectedMigrations); }
            finally { db.close(); }
            stagedValidated = true;
        },
    } });
    stage = "import";
    const result = await saves.importSave(job.save);
    assert.ok(result.ok && stagedValidated);
    stage = "installed Offline key";
    const key = await keys.load(OFFLINE_KEY_SLOT);
    const db = new Database(layout.databasePath, { readonly: true });
    try { assert.equal(db.prepare("SELECT userId FROM userapikeys WHERE keyHash=?").get(createHash("sha256").update(key).digest("hex")).userId, job.userId); }
    finally { db.close(); }
    await fs.writeFile(job.result, JSON.stringify({ ok: true, stagedValidated, abi: process.versions.modules }), { mode: 0o600, flag: "wx" });
}

app.whenReady().then(main).catch(async () => {
    await fs.writeFile(job.result, JSON.stringify({ ok: false, stage }), { mode: 0o600, flag: "wx" });
    process.exitCode = 1;
}).finally(() => app.exit(process.exitCode ?? 0));
