import test from "node:test";
import assert from "node:assert/strict";
import { promises as fsp } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { randomUUID, createHash } from "node:crypto";
import { KeyStore, serverId, type Encryptor } from "../src/main/keystore";
import { offlineLayout } from "../src/main/offline-layout";
import { PortableOfflineSave, OFFLINE_KEY_SLOT } from "../src/main/offline-save";
import { OfflineSaveInvalid, readSave, writeSave, MAX_SAVE_DATABASE_BYTES } from "../src/main/offline-save-format";

const fixtures = require(path.resolve("test/offline-save-fixture.cjs"));
function encryptor(machine: string): Encryptor {
    return {
        isAvailable: () => true,
        encrypt: plain => Buffer.from(machine + ":" + Buffer.from(plain).toString("base64")),
        decrypt: buffer => { if (!buffer.toString().startsWith(machine + ":")) throw new Error("Different machine"); return Buffer.from(buffer.toString().slice(machine.length + 1), "base64").toString(); },
    };
}
async function profile(root: string, machine: string, initialized = true) {
    const userData = path.join(root, machine);
    const resources = path.join(root, "resources");
    await fsp.mkdir(userData, { recursive: true }); await fsp.mkdir(resources, { recursive: true });
    const layout = offlineLayout(resources, userData);
    const keys = new KeyStore(path.join(userData, "keys"), encryptor(machine));
    const identity = initialized ? fixtures.fixture(layout.databasePath, "Portable_1") : null;
    if (identity) await keys.save(OFFLINE_KEY_SLOT, identity.key);
    const runtime = new PortableOfflineSave({ layout, userData, keys, database: fixtures.driver });
    const keyFile = path.join(userData, "keys", serverId(OFFLINE_KEY_SLOT.host, OFFLINE_KEY_SLOT.port) + ".key");
    return { userData, layout, keys, identity, runtime, keyFile };
}
async function temporary(fn: (root: string) => Promise<void>) {
    const root = await fsp.mkdtemp(path.join(tmpdir(), "dr-portable-save-"));
    try { await fn(root); } finally { await fsp.rm(root, { recursive: true, force: true }); }
}
async function noStaging(state: string) {
    assert.ok(!(await fsp.readdir(state)).some(name => name.startsWith(".save-")));
}

test("export rekeys a SQLite copy; restore re-encrypts identity on another machine and preserves player data", () => temporary(async root => {
    const a = await profile(root, "machine-a");
    const oldDb = await fsp.readFile(a.layout.databasePath);
    const oldKey = await fsp.readFile(a.keyFile);
    const db = fixtures.open(a.layout.databasePath);
    const machineSecret = "machine-secret-" + randomUUID();
    try {
        db.prepare("INSERT INTO userapikeystoregister(userId,key) VALUES (?,?)").run("bootstrap", a.identity!.key);
        db.prepare("INSERT INTO gameserverapikeystoregister(key) VALUES (?)").run(machineSecret);
        db.prepare("INSERT INTO gameserverapikeys(keyHash) VALUES (?)").run(createHash("sha256").update(machineSecret).digest("hex"));
        db.prepare("INSERT INTO accountkeyrecovery(userId,keyHash,ciphertext) VALUES (?,?,?)").run(a.identity!.userId, "old", machineSecret);
        db.prepare("INSERT INTO recoveryproofs(discordId,userId,keyHash) VALUES ('123',?,'old')").run(a.identity!.userId);
        db.prepare("INSERT INTO recoverychallenges(tokenHash,discordId,userId,keyHash,expiresAt,used) VALUES ('token','123',?,'old',123,0)").run(a.identity!.userId);
        db.prepare("INSERT INTO recoveryevents(discordId,action,createdAt) VALUES ('123','confirm_attempt',12345)").run();
        db.prepare("INSERT INTO authepochs(userId,epoch) VALUES (?,7)").run(a.identity!.userId);
    } finally { db.close(); }
    const liveBeforeExport = await fsp.readFile(a.layout.databasePath);
    const excluded = ["source-online-key-" + randomUUID(), "source-rsa-secret-" + randomUUID(), "source-env-" + randomUUID(), "source-ready-" + randomUUID()];
    await a.keys.save({ host: "source-online.example", port: 61000, mode: "private", fp: null }, excluded[0]);
    await fsp.writeFile(path.join(a.layout.stateRoot, "secrets.json"), excluded[1]);
    await fsp.mkdir(a.layout.runtimeStateDir, { recursive: true });
    await fsp.writeFile(a.layout.metagameEnvPath, excluded[2]);
    await fsp.mkdir(a.layout.readyDir, { recursive: true });
    await fsp.writeFile(path.join(a.layout.readyDir, "marker"), excluded[3]);
    const state = fixtures.state(a.layout.databasePath);
    const save = path.join(root, "portable.drsave");
    assert.deepEqual(await a.runtime.exportSave(save), { ok: true });
    assert.ok(liveBeforeExport.equals(await fsp.readFile(a.layout.databasePath)), "Live DB must not change");
    assert.ok(oldKey.equals(await fsp.readFile(a.keyFile)), "Live KeyStore must not change");
    const contents = await fsp.readFile(save);
    assert.ok(!contents.includes(Buffer.from(a.identity!.key)), "Active player key must be absent");
    assert.ok(!contents.includes(Buffer.from(machineSecret)), "Deleted credential bytes must be vacuumed away");
    for (const value of excluded) assert.ok(!contents.includes(Buffer.from(value)), "Unrelated profile material must be absent");
    const extracted = path.join(root, "extracted.db");
    const metadata = await readSave(save, extracted);
    assert.equal(metadata.account.userId, a.identity!.userId);
    assert.equal(metadata.account.username, a.identity!.username);
    assert.ok(metadata.credential.key !== a.identity!.key, "Backup key must be fresh");
    assert.equal(fixtures.state(extracted), state);
    const portableDb = fixtures.open(extracted, true);
    try {
        assert.deepEqual(portableDb.prepare("SELECT * FROM recoveryevents").all().map((row: unknown) => ({ ...(row as object) })),
            [{ id: 1, discordId: "123", action: "confirm_attempt", createdAt: 12345 }]);
        assert.equal(portableDb.prepare("SELECT epoch FROM authepochs WHERE userId=?").get(a.identity!.userId).epoch, 8);
    } finally { portableDb.close(); }
    const b = await profile(root, "machine-b", false);
    await fsp.mkdir(b.layout.stateRoot, { recursive: true });
    await fsp.writeFile(path.join(b.layout.stateRoot, "secrets.json"), "destination-runtime-secrets");
    await fsp.writeFile(path.join(b.userData, "settings.json"), "unrelated settings");
    const online = { host: "online.example", port: 61000, mode: "private" as const, fp: null };
    await b.keys.save(online, "online-account-key");
    assert.deepEqual(await b.runtime.importSave(save), { ok: true });
    assert.ok(await b.keys.load(OFFLINE_KEY_SLOT) === metadata.credential.key);
    assert.ok((await fsp.readFile(b.keyFile)).toString().startsWith("machine-b:"), "Destination encryptor must be used");
    assert.equal(fixtures.state(b.layout.databasePath), state);
    assert.equal(await fsp.readFile(path.join(b.layout.stateRoot, "secrets.json"), "utf8"), "destination-runtime-secrets");
    assert.equal(await fsp.readFile(path.join(b.userData, "settings.json"), "utf8"), "unrelated settings");
    assert.ok(await b.keys.load(online) === "online-account-key");
    const c = await profile(root, "machine-c");
    assert.deepEqual(await c.runtime.importSave(save), { ok: true });
    assert.equal(fixtures.state(c.layout.databasePath), state, "Restore replaces the previous Offline profile");
    await noStaging(a.layout.stateRoot); await noStaging(b.layout.stateRoot); await noStaging(c.layout.stateRoot);
    assert.ok(oldDb.length > 0);
}));

test("container rejects bad magic, versions, truncation, corruption and hostile lengths without touching state", () => temporary(async root => {
    const a = await profile(root, "a");
    const save = path.join(root, "save.drsave");
    assert.deepEqual(await a.runtime.exportSave(save), { ok: true });
    const original = await fsp.readFile(save);
    const dbBefore = await fsp.readFile(a.layout.databasePath);
    const keyBefore = await fsp.readFile(a.keyFile);
    const cases = [original.subarray(0, 12), original.subarray(0, original.length - 40)];
    for (const mutate of [
        (b: Buffer) => { b[0] = 0; }, (b: Buffer) => { b.writeUInt32BE(2, 8); },
        (b: Buffer) => { b[b.length - 50] ^= 1; },
        (b: Buffer) => { b.writeBigUInt64BE(BigInt(MAX_SAVE_DATABASE_BYTES) + 1n, 16); },
        (b: Buffer) => { b.writeUInt32BE(0xffffffff, 12); },
    ]) { const bad = Buffer.from(original); mutate(bad); cases.push(bad); }
    const badDigest = Buffer.from(original);
    const jsonLength = badDigest.readUInt32BE(12);
    const json = JSON.parse(badDigest.subarray(24, 24 + jsonLength).toString());
    json.database.sha256 = (json.database.sha256[0] === "0" ? "1" : "0") + json.database.sha256.slice(1);
    Buffer.from(JSON.stringify(json)).copy(badDigest, 24);
    createHash("sha256").update(badDigest.subarray(0, badDigest.length - 32)).digest().copy(badDigest, badDigest.length - 32);
    cases.push(badDigest);
    for (const bytes of cases) {
        const bad = path.join(root, randomUUID() + ".drsave"); await fsp.writeFile(bad, bytes);
        assert.deepEqual(await a.runtime.importSave(bad), { ok: false, error: { code: "offline_save_invalid" } });
        assert.ok(dbBefore.equals(await fsp.readFile(a.layout.databasePath)));
        assert.ok(keyBefore.equals(await fsp.readFile(a.keyFile)));
        await noStaging(a.layout.stateRoot);
    }
}));

test("supported older migration prefixes upgrade privately before restore without changing save version or player identity", () => temporary(async root => {
    const dest = await profile(root, "dest");
    const older = path.join(root, "older.db");
    const count = fixtures.helper.readMigrations(fixtures.metagame).length - 1;
    const identity = fixtures.fixture(older, "OlderPlayer", count);
    const oldDb = fixtures.open(older);
    try { oldDb.prepare("INSERT INTO huntregions(userId,region) VALUES (?,'main')").run(identity.userId); }
    finally { oldDb.close(); }
    const before = await fsp.readFile(older), player = fixtures.state(older);
    const save = path.join(root, "older.drsave"); await writeSave(save, older, identity);
    assert.deepEqual(await dest.runtime.importSave(save), { ok: true });
    assert.ok(before.equals(await fsp.readFile(older)), "Migration must not modify the source DB");
    assert.equal(fixtures.state(dest.layout.databasePath), player);
    assert.ok(await dest.keys.load(OFFLINE_KEY_SLOT) === identity.key);
    const restored = fixtures.open(dest.layout.databasePath);
    try {
        assert.equal(restored.prepare('SELECT count(*) n FROM "__drizzle_migrations"').get().n, count + 1);
        assert.equal(restored.prepare("SELECT region FROM huntregions WHERE userId=?").get(identity.userId).region, "main");
        restored.prepare("UPDATE huntregions SET region='us' WHERE userId=?").run(identity.userId);
        assert.deepEqual(fixtures.helper.schema(restored), fixtures.expectedSchema);
    } finally { restored.close(); }
    assert.equal((await fsp.readFile(save)).readUInt32BE(8), 1);
    await noStaging(dest.layout.stateRoot);
}));

test("unknown, future, incomplete and structurally inconsistent migration claims cannot replace a profile", () => temporary(async root => {
    const dest = await profile(root, "dest");
    const oldDb = await fsp.readFile(dest.layout.databasePath), oldKey = await fsp.readFile(dest.keyFile);
    const mutations = [
        "UPDATE __drizzle_migrations SET hash='unknown' WHERE created_at=(SELECT min(created_at) FROM __drizzle_migrations)",
        "INSERT INTO __drizzle_migrations(hash,created_at) VALUES ('future',9999999999999)",
        "DELETE FROM __drizzle_migrations WHERE created_at=(SELECT min(created_at) FROM __drizzle_migrations)",
        "DELETE FROM __drizzle_migrations WHERE created_at=(SELECT max(created_at) FROM __drizzle_migrations)",
        "DROP TABLE recoveryevents",
    ];
    for (const sql of mutations) {
        const file = path.join(root, randomUUID() + ".db");
        const identity = fixtures.fixture(file);
        const db = fixtures.open(file);
        try { db.exec(sql); } finally { db.close(); }
        const save = path.join(root, randomUUID() + ".drsave"); await writeSave(save, file, identity);
        assert.deepEqual(await dest.runtime.importSave(save), { ok: false, error: { code: "offline_save_invalid" } });
        assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
        assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
        await noStaging(dest.layout.stateRoot);
    }
}));

test("a data-dependent migration failure rolls back staged SQL and never touches destination state", () => temporary(async root => {
    const dest = await profile(root, "dest");
    const source = path.join(root, "older.db");
    const identity = fixtures.fixture(source);
    const save = path.join(root, "older.drsave"); await writeSave(save, source, identity);
    const nextMetagame = path.join(root, "next-metagame");
    const folder = path.join(nextMetagame, "src", "drizzle");
    await fsp.cp(path.join(fixtures.metagame, "src", "drizzle"), folder, { recursive: true });
    const journalFile = path.join(folder, "meta", "_journal.json");
    const journal = JSON.parse(await fsp.readFile(journalFile, "utf8"));
    const entry = { idx: journal.entries.length, when: journal.entries.at(-1).when + 1, tag: "test_failure" };
    journal.entries.push(entry);
    await fsp.writeFile(journalFile, JSON.stringify(journal));
    // The schema-only canonical DB succeeds; populated staged data fails the CHECK.
    await fsp.writeFile(path.join(folder, entry.tag + ".sql"), "CREATE TABLE migration_guard (notes INTEGER CHECK(notes < 0));\n--> statement-breakpoint\nINSERT INTO migration_guard SELECT notes FROM users;");
    const oldDb = await fsp.readFile(dest.layout.databasePath), oldKey = await fsp.readFile(dest.keyFile);
    const sourceBefore = await fsp.readFile(source);
    const runtime = new PortableOfflineSave({ layout: dest.layout, userData: dest.userData, keys: dest.keys, database: fixtures.driverFor(nextMetagame) });
    assert.deepEqual(await runtime.importSave(save), { ok: false, error: { code: "offline_save_invalid" } });
    assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
    assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
    assert.ok(sourceBefore.equals(await fsp.readFile(source)));
    const staged = path.join(root, "staged.db"); await fsp.copyFile(source, staged);
    const db = fixtures.open(staged);
    try {
        assert.throws(() => fixtures.helper.migrateDatabase(db, nextMetagame, () => fixtures.open(":memory:"), identity.key, identity));
        assert.equal(db.prepare("SELECT count(*) n FROM sqlite_schema WHERE name='migration_guard'").get().n, 0);
        assert.equal(db.prepare("SELECT count(*) n FROM __drizzle_migrations").get().n, entry.idx);
    } finally { db.close(); }
    await noStaging(dest.layout.stateRoot);
}));

test("checksummed containers still require real SQLite integrity/schema and credential/account validation", () => temporary(async root => {
    const a = await profile(root, "a");
    const save = path.join(root, "good.drsave"); await a.runtime.exportSave(save);
    const copied = path.join(root, "db"); const metadata = await readSave(save, copied);
    const wrong = path.join(root, "wrong.drsave");
    await writeSave(wrong, copied, { ...metadata.account, key: "UUK_" + "1".repeat(48) });
    assert.deepEqual(await a.runtime.importSave(wrong), { ok: false, error: { code: "offline_save_invalid" } });
    const damaged = path.join(root, "damaged.db");
    await fsp.writeFile(damaged, Buffer.alloc(4096, 7));
    const invalid = path.join(root, "invalid.drsave");
    await writeSave(invalid, damaged, { ...metadata.account, key: metadata.credential.key });
    assert.deepEqual(await a.runtime.importSave(invalid), { ok: false, error: { code: "offline_save_invalid" } });
    const corrupt = fixtures.open(copied);
    try {
        corrupt.allowCorruption();
        corrupt.exec("PRAGMA writable_schema=ON");
        corrupt.exec("UPDATE sqlite_schema SET rootpage=(SELECT rootpage FROM sqlite_schema WHERE name='users') WHERE name='characters'");
    } finally { corrupt.close(); }
    const integrity = path.join(root, "integrity.drsave");
    await writeSave(integrity, copied, { ...metadata.account, key: metadata.credential.key });
    assert.deepEqual(await a.runtime.importSave(integrity), { ok: false, error: { code: "offline_save_invalid" } });
    const db = fixtures.open(copied);
    try { db.exec("CREATE TABLE unexpected(data TEXT)"); } finally { db.close(); }
    const schema = path.join(root, "schema.drsave");
    await writeSave(schema, copied, { ...metadata.account, key: metadata.credential.key });
    assert.deepEqual(await a.runtime.importSave(schema), { ok: false, error: { code: "offline_save_invalid" } });
    await noStaging(a.layout.stateRoot);
}));

test("ordinary import failures restore the exact previous DB and encrypted key; prepared crash journal recovers", () => temporary(async root => {
    const source = await profile(root, "source");
    const save = path.join(root, "portable.drsave"); await source.runtime.exportSave(save);
    const dest = await profile(root, "dest");
    const oldDb = await fsp.readFile(dest.layout.databasePath), oldKey = await fsp.readFile(dest.keyFile);
    for (const keyFails of [true, false]) {
        const runtime = new PortableOfflineSave({ layout: dest.layout, userData: dest.userData, database: fixtures.driver,
            keys: keyFails ? { load: slot => dest.keys.load(slot), save: async () => { throw new Error("Simulated key write failure"); } } : dest.keys,
            replaceDatabase: keyFails ? undefined : async () => { throw new Error("Simulated DB replacement failure"); },
        });
        assert.equal((await runtime.importSave(save)).ok, false);
        assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
        assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
        await noStaging(dest.layout.stateRoot);
    }
    const journal = path.join(dest.layout.stateRoot, ".save-restore"); await fsp.mkdir(journal);
    await fsp.writeFile(path.join(journal, "db.previous"), oldDb); await fsp.writeFile(path.join(journal, "key.previous"), oldKey);
    await fsp.writeFile(path.join(journal, "journal.json"), JSON.stringify({ version: 1, phase: "prepared", dbExisted: true, keyExisted: true }));
    await fsp.writeFile(dest.layout.databasePath, "half-restored database");
    await dest.runtime.recover();
    assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
    assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
    await noStaging(dest.layout.stateRoot);
}));

test("post-write key failure rolls back; committed crash journal keeps new state; fresh rollback removes both files", () => temporary(async root => {
    const source = await profile(root, "source");
    const save = path.join(root, "portable.drsave"); await source.runtime.exportSave(save);
    const dest = await profile(root, "dest");
    const oldDb = await fsp.readFile(dest.layout.databasePath), oldKey = await fsp.readFile(dest.keyFile);
    const failed = new PortableOfflineSave({ layout: dest.layout, userData: dest.userData, database: fixtures.driver,
        keys: { load: slot => dest.keys.load(slot), save: async (slot, key) => { await dest.keys.save(slot, key); throw new Error("Post-write failure"); } } });
    assert.equal((await failed.importSave(save)).ok, false);
    assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
    assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
    assert.deepEqual(await dest.runtime.importSave(save), { ok: true });
    const committedDb = await fsp.readFile(dest.layout.databasePath), committedKey = await fsp.readFile(dest.keyFile);
    const journal = path.join(dest.layout.stateRoot, ".save-restore"); await fsp.mkdir(journal);
    await fsp.writeFile(path.join(journal, "db.previous"), oldDb); await fsp.writeFile(path.join(journal, "key.previous"), oldKey);
    await fsp.writeFile(path.join(journal, "journal.json"), JSON.stringify({ version: 1, phase: "committed", dbExisted: true, keyExisted: true }));
    await dest.runtime.recover();
    assert.ok(committedDb.equals(await fsp.readFile(dest.layout.databasePath)));
    assert.ok(committedKey.equals(await fsp.readFile(dest.keyFile)));
    await fsp.mkdir(journal);
    await fsp.writeFile(path.join(journal, "journal.json"), JSON.stringify({ version: 1, phase: "prepared", dbExisted: false, keyExisted: false }));
    await dest.runtime.recover();
    await assert.rejects(fsp.access(dest.layout.databasePath));
    await assert.rejects(fsp.access(dest.keyFile));
    await noStaging(dest.layout.stateRoot);
}));

test("completed rollback survives partial cleanup and repeated startup recovery without snapshots", () => temporary(async root => {
    const dest = await profile(root, "dest");
    const oldDb = await fsp.readFile(dest.layout.databasePath), oldKey = await fsp.readFile(dest.keyFile);
    const online = { host: "online.example", port: 61000, mode: "private" as const, fp: null };
    await dest.keys.save(online, "online-account-key");
    await fsp.writeFile(path.join(dest.userData, "settings.json"), "current settings");
    const journal = path.join(dest.layout.stateRoot, ".save-restore"); await fsp.mkdir(journal);
    await fsp.writeFile(path.join(journal, "db.previous"), oldDb); await fsp.writeFile(path.join(journal, "key.previous"), oldKey);
    await fsp.writeFile(path.join(journal, "journal.json"), JSON.stringify({ version: 1, phase: "prepared", dbExisted: true, keyExisted: true }));
    await fsp.writeFile(dest.layout.databasePath, "new database"); await fsp.writeFile(dest.keyFile, "new key");
    const remove = fsp.rm;
    fsp.rm = async (file, options) => {
        if (file === journal) {
            await remove(path.join(journal, "db.previous"));
            throw new Error("Interrupted cleanup after deleting one snapshot");
        }
        return remove(file, options);
    };
    try { await assert.rejects(dest.runtime.recover()); } finally { fsp.rm = remove; }
    assert.equal(JSON.parse(await fsp.readFile(path.join(journal, "journal.json"), "utf8")).phase, "rolled-back");
    assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
    assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
    await dest.runtime.recover(); await dest.runtime.recover();
    assert.ok(oldDb.equals(await fsp.readFile(dest.layout.databasePath)));
    assert.ok(oldKey.equals(await fsp.readFile(dest.keyFile)));
    assert.ok(await dest.keys.load(online) === "online-account-key");
    assert.equal(await fsp.readFile(path.join(dest.userData, "settings.json"), "utf8"), "current settings");
    await noStaging(dest.layout.stateRoot);
}));

test("recovery retries incomplete rollback and terminal cleanup; pre-journal interruption preserves live files", () => temporary(async root => {
    const dest = await profile(root, "dest");
    const oldDb = await fsp.readFile(dest.layout.databasePath), oldKey = await fsp.readFile(dest.keyFile);
    const journal = path.join(dest.layout.stateRoot, ".save-restore");
    for (const phase of ["snapshots", "prepared", "db-replaced", "key-replaced", "partial-rollback", "committed"]) {
        await fsp.mkdir(journal);
        const terminal = phase === "committed";
        if (!terminal) {
            await fsp.writeFile(path.join(journal, "db.previous"), oldDb); await fsp.writeFile(path.join(journal, "key.previous"), oldKey);
        }
        if (phase !== "snapshots") await fsp.writeFile(path.join(journal, "journal.json"), JSON.stringify({ version: 1,
            phase: terminal ? "committed" : "prepared", dbExisted: true, keyExisted: true }));
        const expectedDb = terminal ? Buffer.from("committed database") : oldDb;
        const expectedKey = terminal ? Buffer.from("committed encrypted key") : oldKey;
        await fsp.writeFile(dest.layout.databasePath, ["db-replaced", "key-replaced"].includes(phase) ? "new database" : expectedDb);
        await fsp.writeFile(dest.keyFile, ["key-replaced", "partial-rollback"].includes(phase) ? "new encrypted key" : expectedKey);
        await dest.runtime.recover(); await dest.runtime.recover();
        assert.ok(expectedDb.equals(await fsp.readFile(dest.layout.databasePath)));
        assert.ok(expectedKey.equals(await fsp.readFile(dest.keyFile)));
        await noStaging(dest.layout.stateRoot);
    }
}));

test("export refuses critical destinations and external journal state and never publishes a partial file", () => temporary(async root => {
    const a = await profile(root, "a");
    assert.equal((await a.runtime.exportSave(path.join(a.userData, "critical.drsave"))).ok, false);
    const marker = a.layout.databasePath + "-wal"; await fsp.writeFile(marker, "unexpected WAL");
    const save = path.join(root, "save.drsave");
    assert.equal((await a.runtime.exportSave(save)).ok, false);
    await assert.rejects(fsp.access(save));
    await fsp.writeFile(save, "previous destination");
    assert.equal((await a.runtime.exportSave(save)).ok, false);
    assert.equal(await fsp.readFile(save, "utf8"), "previous destination");
    await noStaging(a.layout.stateRoot);
}));

test("raw save runtime refuses active stacks and games", () => temporary(async root => {
    const a = await profile(root, "a");
    const runtime = new PortableOfflineSave({ layout: a.layout, userData: a.userData, keys: a.keys, database: fixtures.driver, isActive: () => true });
    assert.deepEqual(await runtime.exportSave(path.join(root, "save.drsave")), { ok: false, error: { code: "busy" } });
    assert.deepEqual(await runtime.importSave(path.join(root, "save.drsave")), { ok: false, error: { code: "busy" } });
    await noStaging(a.layout.stateRoot);
}));
