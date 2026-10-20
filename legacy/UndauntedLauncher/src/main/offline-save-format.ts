import { createHash } from "node:crypto";
import { createReadStream, promises as fsp } from "node:fs";

export const SAVE_MAGIC = Buffer.from("DRSAVE\r\n");
export const SAVE_HEADER_BYTES = 24;
export const MAX_SAVE_METADATA_BYTES = 65536;
export const MAX_SAVE_DATABASE_BYTES = 512 * 1024 * 1024;

export class OfflineSaveInvalid extends Error {
    constructor() { super("Invalid portable Offline save"); }
}

export interface PortableIdentity { userId: string; username: string; key: string }
export interface SaveMetadata {
    format: "dauntless-revived-offline-save";
    version: 1;
    createdAt: string;
    account: { userId: string; username: string };
    database: { bytes: number; sha256: string };
    credential: { version: 1; kind: "uuk-sha256"; key: string };
}

function validateMetadata(value: unknown, bytes: number): SaveMetadata {
    const m = value as SaveMetadata;
    if (!m || m.format !== "dauntless-revived-offline-save" || m.version !== 1 ||
        typeof m.createdAt !== "string" || !Number.isFinite(Date.parse(m.createdAt)) || new Date(m.createdAt).toISOString() !== m.createdAt ||
        !m.account || typeof m.account.userId !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(m.account.userId) ||
        typeof m.account.username !== "string" || m.account.username.length < 1 || m.account.username.length > 128 || /[\r\n\0]/.test(m.account.username) ||
        !m.database || m.database.bytes !== bytes || typeof m.database.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(m.database.sha256) ||
        !m.credential || m.credential.version !== 1 || m.credential.kind !== "uuk-sha256" || typeof m.credential.key !== "string" || !/^UUK_[a-f0-9]{48}$/.test(m.credential.key)) {
        throw new OfflineSaveInvalid();
    }
    return m;
}

export async function databaseDigest(file: string): Promise<string> {
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(file)) hash.update(chunk);
    return hash.digest("hex");
}

export async function writeSave(file: string, database: string, identity: PortableIdentity): Promise<void> {
    const bytes = (await fsp.stat(database)).size;
    if (bytes < 512 || bytes > MAX_SAVE_DATABASE_BYTES) throw new OfflineSaveInvalid();
    const metadata: SaveMetadata = {
        format: "dauntless-revived-offline-save", version: 1, createdAt: new Date().toISOString(),
        account: { userId: identity.userId, username: identity.username },
        database: { bytes, sha256: await databaseDigest(database) },
        credential: { version: 1, kind: "uuk-sha256", key: identity.key },
    };
    validateMetadata(metadata, bytes);
    const json = Buffer.from(JSON.stringify(metadata));
    if (json.length > MAX_SAVE_METADATA_BYTES) throw new OfflineSaveInvalid();
    const header = Buffer.alloc(SAVE_HEADER_BYTES);
    SAVE_MAGIC.copy(header);
    header.writeUInt32BE(1, 8);
    header.writeUInt32BE(json.length, 12);
    header.writeBigUInt64BE(BigInt(bytes), 16);
    const output = await fsp.open(file, "wx", 0o600);
    const digest = createHash("sha256");
    try {
        for (const part of [header, json]) { await output.writeFile(part); digest.update(part); }
        for await (const chunk of createReadStream(database)) { await output.writeFile(chunk); digest.update(chunk); }
        await output.writeFile(digest.digest());
        await output.sync();
    } finally { await output.close(); }
}

// There are no archive names or extraction paths. Exactly one bounded payload is streamed.
export async function readSave(file: string, database: string): Promise<SaveMetadata> {
    const input = await fsp.open(file, "r");
    let output: Awaited<ReturnType<typeof fsp.open>> | undefined;
    try {
        const stat = await input.stat();
        if (!stat.isFile()) throw new OfflineSaveInvalid();
        const size = stat.size;
        if (size < SAVE_HEADER_BYTES + 32) throw new OfflineSaveInvalid();
        const header = Buffer.alloc(SAVE_HEADER_BYTES);
        if ((await input.read(header, 0, header.length, 0)).bytesRead !== header.length) throw new OfflineSaveInvalid();
        const metaBytes = header.readUInt32BE(12);
        const dbBytes = header.readBigUInt64BE(16);
        if (!header.subarray(0, 8).equals(SAVE_MAGIC) || header.readUInt32BE(8) !== 1 ||
            metaBytes < 2 || metaBytes > MAX_SAVE_METADATA_BYTES || dbBytes < 512n || dbBytes > BigInt(MAX_SAVE_DATABASE_BYTES) ||
            BigInt(size) !== BigInt(SAVE_HEADER_BYTES + metaBytes + 32) + dbBytes) throw new OfflineSaveInvalid();
        const json = Buffer.alloc(metaBytes);
        if ((await input.read(json, 0, json.length, SAVE_HEADER_BYTES)).bytesRead !== json.length) throw new OfflineSaveInvalid();
        let metadata: SaveMetadata;
        try { metadata = validateMetadata(JSON.parse(json.toString("utf8")), Number(dbBytes)); }
        catch { throw new OfflineSaveInvalid(); }
        const digest = createHash("sha256").update(header).update(json);
        const dbDigest = createHash("sha256");
        output = await fsp.open(database, "wx", 0o600);
        const buffer = Buffer.alloc(65536);
        let position = SAVE_HEADER_BYTES + metaBytes;
        let remaining = Number(dbBytes);
        while (remaining > 0) {
            const { bytesRead } = await input.read(buffer, 0, Math.min(buffer.length, remaining), position);
            if (!bytesRead) throw new OfflineSaveInvalid();
            const chunk = buffer.subarray(0, bytesRead);
            digest.update(chunk); dbDigest.update(chunk);
            await output.writeFile(chunk);
            remaining -= bytesRead; position += bytesRead;
        }
        const trailer = Buffer.alloc(32);
        if ((await input.read(trailer, 0, 32, position)).bytesRead !== 32 ||
            !trailer.equals(digest.digest()) || dbDigest.digest("hex") !== metadata.database.sha256) throw new OfflineSaveInvalid();
        await output.sync();
        return metadata;
    } finally { await output?.close(); await input.close(); }
}
