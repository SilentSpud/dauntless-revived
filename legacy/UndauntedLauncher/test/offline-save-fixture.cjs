const { DatabaseSync, backup } = require("node:sqlite");
const path = require("node:path");
const fs = require("node:fs");
const { randomUUID, randomBytes, createHash } = require("node:crypto");
const helper = require("../runtime/offline-save-db.cjs");
const metagame = path.resolve(__dirname, "../../UndauntedMetagame");

function open(file, readonly = false) {
    const db = new DatabaseSync(file, { readOnly: readonly });
    db.exec("PRAGMA foreign_keys=OFF");
    return { exec: sql => db.exec(sql), prepare: sql => db.prepare(sql), close: () => db.close(), backup: target => backup(db, target),
        allowCorruption: () => db.enableDefensive(false) };
}
function fixture(file, name = "Portable_1", count) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const db = open(file);
    const identity = { userId: "UID-" + randomUUID(), username: name, key: "UUK_" + randomBytes(24).toString("hex") };
    try {
        helper.createSchema(db, metagame, count);
        db.prepare("INSERT INTO users(userId,name,notes,isAdmin) VALUES (?,?,17,0)").run(identity.userId, name);
        db.prepare("INSERT INTO userapikeys(userId,keyHash) VALUES (?,?)").run(identity.userId, createHash("sha256").update(identity.key).digest("hex"));
        db.prepare("INSERT INTO characters(characterId,userId,createdDate,lastModifiedDate,name,updateVersion,data) VALUES (?,?, 'created','updated',?,7,?)")
            .run("CHAR-" + identity.userId, identity.userId, name, JSON.stringify({ progression: "ftue-completed", objectives: ["test-objective"] }));
        db.prepare("INSERT INTO inventories(characterId,instancedItems,stackedItems) VALUES (?,?,?)")
            .run("CHAR-" + identity.userId, JSON.stringify({ axe: "recognizable" }), JSON.stringify({ cells: 42 }));
        db.prepare("INSERT INTO loadouts(characterId,userId,loadouts,persistent) VALUES (?,?,?,?)")
            .run("CHAR-" + identity.userId, identity.userId, JSON.stringify({ weapon: "axe" }), JSON.stringify({ selected: 2 }));
    } finally { db.close(); }
    return identity;
}
const canonical = open(":memory:");
helper.createSchema(canonical, metagame);
const expectedSchema = helper.schema(canonical);
canonical.close();
function driverFor(migrationsDir = metagame) { return {
    exportCopy: async (_work, source, destination, key) => helper.exportDatabase(source, destination, key, open, expectedSchema),
    validate: async (_work, file, identity) => {
        const { OfflineSaveInvalid } = require(path.resolve(".test-build/src/main/offline-save-format.js"));
        let db;
        try { db = open(file); helper.migrateDatabase(db, migrationsDir, () => open(":memory:"), identity.key, identity); }
        catch { throw new OfflineSaveInvalid(); }
        finally { db?.close(); }
    },
}; }
const driver = driverFor();
function state(file) {
    const db = open(file, true);
    try { return JSON.stringify([db.prepare("SELECT * FROM users").all(), db.prepare("SELECT * FROM characters").all(),
        db.prepare("SELECT * FROM inventories").all(), db.prepare("SELECT * FROM loadouts").all()]); }
    finally { db.close(); }
}
module.exports = { open, fixture, driver, driverFor, metagame, state, expectedSchema, helper };
