// tools/zip.ts and make-game-manifest.ts's entry hashing, against archives written here: stored and
// deflated entries, and the Zip64 records the 10 GB game zip needs (forced on small entries).
import { afterAll, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { deflateRawSync } from "node:zlib";
import { hashEntry, isSafeManifestPath } from "./make-game-manifest.ts";
import { openEntry, readEntries } from "./zip.ts";

const dir = mkdtempSync(path.join(tmpdir(), "dr-zip-"));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

interface Source {
    name: string;
    data: Buffer;
    deflate?: boolean;
}

function u16(n: number) { const b = Buffer.alloc(2); b.writeUInt16LE(n); return b; }
function u32(n: number) { const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b; }
function u64(n: number) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n)); return b; }

// A zip of `sources`; with `zip64`, every size and offset goes in Zip64 records instead.
function writeZip(sources: Source[], zip64 = false, crcOf = (data: Buffer) => Bun.hash.crc32(data)): Buffer {
    const parts: Buffer[] = [];
    const central: Buffer[] = [];
    let offset = 0;
    for (const s of sources) {
        const name = Buffer.from(s.name);
        const body = s.deflate ? deflateRawSync(s.data) : s.data;
        const crc = crcOf(s.data);
        const sized = (n: number) => (zip64 ? 0xffffffff : n);
        const local = Buffer.concat([u32(0x04034b50), u16(45), u16(0x800), u16(s.deflate ? 8 : 0), u32(0), u32(crc),
            u32(sized(body.length)), u32(sized(s.data.length)), u16(name.length), u16(0), name]);
        const extra = zip64 ? Buffer.concat([u16(1), u16(24), u64(s.data.length), u64(body.length), u64(offset)]) : Buffer.alloc(0);
        central.push(Buffer.concat([u32(0x02014b50), u16(45), u16(45), u16(0x800), u16(s.deflate ? 8 : 0), u32(0), u32(crc),
            u32(sized(body.length)), u32(sized(s.data.length)), u16(name.length), u16(extra.length), u16(0), u16(0), u16(0),
            u32(0), u32(sized(offset)), name, extra]));
        parts.push(local, body);
        offset += local.length + body.length;
    }
    const directory = Buffer.concat(central);
    const tail: Buffer[] = [];
    if (zip64) {
        const recordOffset = offset + directory.length;
        tail.push(Buffer.concat([u32(0x06064b50), u64(44), u16(45), u16(45), u32(0), u32(0), u64(sources.length),
            u64(sources.length), u64(directory.length), u64(offset)]));
        tail.push(Buffer.concat([u32(0x07064b50), u32(0), u64(recordOffset), u32(1)]));
        tail.push(Buffer.concat([u32(0x06054b50), u16(0), u16(0), u16(0xffff), u16(0xffff), u32(0xffffffff), u32(0xffffffff), u16(0)]));
    } else {
        tail.push(Buffer.concat([u32(0x06054b50), u16(0), u16(0), u16(sources.length), u16(sources.length),
            u32(directory.length), u32(offset), u16(0)]));
    }
    return Buffer.concat([...parts, directory, ...tail]);
}

const big = Buffer.alloc(300_000);
for (let i = 0; i < big.length; i++) big[i] = (i * 7919) % 251;
const sources: Source[] = [
    { name: "Game/", data: Buffer.alloc(0) },
    { name: "Game/Version.txt", data: Buffer.from("dauntless_rel-1.4.4\r\n") },
    { name: "Game/Archon/Content/pakchunk0.pak", data: big, deflate: true },
    { name: "Game/Archon/empty.bin", data: Buffer.alloc(0), deflate: true },
];

function sha256(data: Buffer) {
    return new Bun.CryptoHasher("sha256").update(data).digest("hex");
}

for (const zip64 of [false, true]) {
    test(`entries, sizes and hashes${zip64 ? " (Zip64)" : ""}`, async () => {
        const file = path.join(dir, `game${zip64 ? "64" : ""}.zip`);
        await Bun.write(file, writeZip(sources, zip64));
        const zip = Bun.file(file);
        const entries = await readEntries(zip);
        expect(entries.map((e) => e.fileName)).toEqual(sources.map((s) => s.name));
        for (const [i, entry] of entries.entries()) {
            if (entry.fileName.endsWith("/")) continue;
            const source = sources[i]!;
            const hashed = await hashEntry(zip, entry);
            expect([hashed.size, hashed.sha256]).toEqual([source.data.length, sha256(source.data)]);
            expect(entry.method).toBe(source.deflate ? 8 : 0);
        }
        const version = entries.find((e) => e.fileName.endsWith("Version.txt"))!;
        expect((await hashEntry(zip, version)).small?.toString()).toBe("dauntless_rel-1.4.4\r\n");
        expect(Buffer.from(await new Response(await openEntry(zip, version)).arrayBuffer()).toString()).toBe("dauntless_rel-1.4.4\r\n");
    });
}

test("a CRC-32 mismatch and refused archives", async () => {
    const file = path.join(dir, "bad-crc.zip");
    await Bun.write(file, writeZip([{ name: "a.txt", data: Buffer.from("hello") }], false, () => 1234));
    const zip = Bun.file(file);
    const [entry] = await readEntries(zip);
    await expect(hashEntry(zip, entry!)).rejects.toThrow("CRC-32 mismatch");

    const backslash = path.join(dir, "backslash.zip");
    await Bun.write(backslash, writeZip([{ name: "Archon\\x.txt", data: Buffer.from("x") }]));
    await expect(readEntries(Bun.file(backslash))).rejects.toThrow("backslash");

    const notZip = path.join(dir, "not.zip");
    await Bun.write(notZip, "not a zip at all");
    await expect(readEntries(Bun.file(notZip))).rejects.toThrow("no end of central directory");
});

test("manifest paths", () => {
    for (const ok of ["Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe", "Version.txt", "a/b_c-d.e"]) expect(isSafeManifestPath(ok)).toBe(true);
    for (const bad of ["", "../x", "a//b", "a/.hidden", "con.txt", "a/COM1", "a\\b", "x".repeat(401), 7]) expect(isSafeManifestPath(bad)).toBe(false);
});
