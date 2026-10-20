const fs = require("node:fs");
const path = require("node:path");
const { createHash, randomBytes } = require("node:crypto");
const hash = value => createHash("sha256").update(value).digest("hex");
const invalid = () => { const error = new Error("Invalid Offline database"); error.code = "OFFLINE_SAVE_INVALID"; throw error; };
const clearedTables = ["userapikeystoregister", "gameserverapikeystoregister", "gameserverapikeys",
    "accountkeyrecovery", "recoveryproofs", "recoverychallenges"];

function schema(db) {
    return db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name != 'sqlite_sequence' AND name NOT GLOB 'sqlite_autoindex_*' ORDER BY type,name")
        .all().map(row => ({ ...row, sql: row.sql?.replace(/\s+/g, " ").trim() }));
}

function readMigrations(metagameDir) {
    const folder = path.join(metagameDir, "src", "drizzle");
    const journal = JSON.parse(fs.readFileSync(path.join(folder, "meta", "_journal.json"), "utf8"));
    return journal.entries.map((entry, index) => {
        if (entry.idx !== index || !Number.isSafeInteger(entry.when) || (index && entry.when <= journal.entries[index - 1].when)) invalid();
        const sql = fs.readFileSync(path.join(folder, entry.tag + ".sql"), "utf8");
        return { sql, hash: hash(sql), when: entry.when };
    });
}

function createMigrationTable(db) {
    // Match Drizzle's migration bookkeeping DDL, including normalized boundary spaces.
    db.exec('CREATE TABLE "__drizzle_migrations" ( id SERIAL PRIMARY KEY, hash text NOT NULL, created_at numeric )');
}

function applyMigrations(db, migrations) {
    // Drizzle's SQLite migrator applies the pending statements and history in one transaction.
    db.exec("BEGIN");
    try {
        for (const migration of migrations) {
            for (const statement of migration.sql.split("--> statement-breakpoint")) db.exec(statement);
            db.prepare('INSERT INTO "__drizzle_migrations" (hash,created_at) VALUES (?,?)').run(migration.hash, migration.when);
        }
        db.exec("COMMIT");
    } catch (error) { db.exec("ROLLBACK"); throw error; }
}

function createSchema(db, metagameDir, count) {
    createMigrationTable(db);
    applyMigrations(db, readMigrations(metagameDir).slice(0, count));
}

function migrateDatabase(db, metagameDir, openMemory, credential, declared) {
    db.exec("PRAGMA trusted_schema=OFF");
    const migrations = readMigrations(metagameDir);
    const canonical = openMemory();
    try {
        createMigrationTable(canonical);
        // Validate the bookkeeping object before reading untrusted migration history.
        const bookkeeping = schema(db).filter(row => row.name === "__drizzle_migrations");
        if (JSON.stringify(bookkeeping) !== JSON.stringify(schema(canonical))) invalid();
        if (db.prepare("PRAGMA integrity_check").get().integrity_check !== "ok") invalid();
        const history = db.prepare('SELECT hash,created_at FROM "__drizzle_migrations" ORDER BY created_at LIMIT ?').all(migrations.length + 1);
        if (!history.length || history.length > migrations.length) invalid();
        for (let index = 0; index < history.length; index++) {
            if (history[index].hash !== migrations[index].hash || history[index].created_at !== migrations[index].when) invalid();
        }
        // A history prefix is insufficient alone: its actual schema must match that prefix.
        applyMigrations(canonical, migrations.slice(0, history.length));
        validateDatabase(db, schema(canonical), credential, declared);
        const pending = migrations.slice(history.length);
        applyMigrations(canonical, pending);
        if (pending.length) applyMigrations(db, pending);
        validateDatabase(db, schema(canonical), credential, declared);
    } finally { canonical.close(); }
}

function validateDatabase(db, expectedSchema, credential, declared) {
    db.exec("PRAGMA trusted_schema=OFF");
    if (JSON.stringify(schema(db)) !== JSON.stringify(expectedSchema)) invalid();
    if (db.prepare("PRAGMA integrity_check").get().integrity_check !== "ok") invalid();
    if (db.prepare("PRAGMA journal_mode").get().journal_mode !== "delete") invalid();
    const matches = db.prepare("SELECT u.userId,u.name FROM userapikeys k JOIN users u ON u.userId=k.userId WHERE k.keyHash=?").all(hash(credential));
    if (matches.length !== 1 || (declared && (matches[0].userId !== declared.userId || matches[0].name !== declared.username))) invalid();
    if (declared) {
        for (const table of clearedTables) {
            if (expectedSchema.some(row => row.type === "table" && row.name === table) && db.prepare(`SELECT count(*) n FROM ${table}`).get().n !== 0) invalid();
        }
    }
    return { userId: matches[0].userId, username: matches[0].name };
}

async function exportDatabase(source, destination, credential, open, expectedSchema) {
    // Refuse unexpected external journal state rather than omit committed frames.
    for (const suffix of ["-wal", "-shm", "-journal"]) if (fs.existsSync(source + suffix)) invalid();
    const live = open(source, true);
    try {
        validateDatabase(live, expectedSchema, credential);
        await live.backup(destination);
        fs.chmodSync(destination, 0o600);
    } finally { live.close(); }
    const copy = open(destination, false);
    try {
        const account = validateDatabase(copy, expectedSchema, credential);
        const key = "UUK_" + randomBytes(24).toString("hex");
        copy.exec("PRAGMA secure_delete=ON; BEGIN IMMEDIATE");
        try {
            if (copy.prepare("UPDATE userapikeys SET keyHash=? WHERE userId=?").run(hash(key), account.userId).changes !== 1) invalid();
            for (const table of clearedTables) copy.exec(`DELETE FROM ${table}`);
            copy.prepare("INSERT INTO authepochs(userId,epoch) VALUES (?,1) ON CONFLICT(userId) DO UPDATE SET epoch=epoch+1").run(account.userId);
            copy.exec("COMMIT");
        } catch (error) { copy.exec("ROLLBACK"); throw error; }
        // Deleted/bootstrap credentials can remain in free pages; rewrite the copy.
        copy.exec("VACUUM");
        validateDatabase(copy, expectedSchema, key, account);
        return { ...account, key };
    } finally { copy.close(); }
}

async function runJob(file) {
    const job = JSON.parse(fs.readFileSync(file, "utf8"));
    try {
        const Database = require(path.join(job.metagameDir, "node_modules", "better-sqlite3"));
        const open = (file, readonly) => new Database(file, { readonly, fileMustExist: true });
        const canonical = new Database(":memory:");
        let expected;
        try { createSchema(canonical, job.metagameDir); expected = schema(canonical); } finally { canonical.close(); }
        let identity;
        if (job.operation === "export") identity = await exportDatabase(job.source, job.database, job.key, open, expected);
        else if (job.operation === "validate") {
            // The caller supplies only a private extracted copy; never migrate live state.
            const db = open(job.database, false);
            try { migrateDatabase(db, job.metagameDir, () => new Database(":memory:"), job.key, job.account); } finally { db.close(); }
        } else invalid();
        fs.writeFileSync(job.result, JSON.stringify({ ok: true, identity }), { mode: 0o600, flag: "wx" });
    } catch (error) {
        const io = ["ENOENT", "EACCES", "EPERM", "ENOSPC", "EIO", "SQLITE_FULL", "SQLITE_CANTOPEN", "SQLITE_IOERR"].some(code => error.code?.startsWith(code));
        fs.writeFileSync(job.result, JSON.stringify({ ok: false, error: io ? "io" : "invalid" }), { mode: 0o600, flag: "wx" });
    }
}

module.exports = { schema, createSchema, readMigrations, migrateDatabase, validateDatabase, exportDatabase, clearedTables };
if (require.main === module) runJob(process.argv[2]).then(() => process.exit(0)).catch(() => process.exit(1));
