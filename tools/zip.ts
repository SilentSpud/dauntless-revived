// A streaming ZIP reader for make-game-manifest.ts: the central directory (Zip64 included, as the
// 1.4.4 zip is about 10 GB), and each entry's uncompressed bytes as a stream, stored or deflated.
// Nothing is extracted to disk. Encrypted entries, other compression methods, entries split over
// several disks and file names with backslashes are refused, as yauzl's strict mode refused them.
import type { BunFile } from "bun";

export interface ZipEntry {
    fileName: string;
    method: number;
    crc32: number;
    compressedSize: number;
    uncompressedSize: number;
    localHeaderOffset: number;
}

const EOCD = 0x06054b50;
const ZIP64_LOCATOR = 0x07064b50;
const ZIP64_EOCD = 0x06064b50;
const CENTRAL = 0x02014b50;
const LOCAL = 0x04034b50;

async function bytes(file: BunFile, start: number, end: number): Promise<DataView> {
    const data = new Uint8Array(await file.slice(start, end).arrayBuffer());
    if (data.length !== end - start) throw new Error(`zip: could not read bytes ${start}-${end}`);
    return new DataView(data.buffer, data.byteOffset, data.byteLength);
}

function u64(view: DataView, at: number): number {
    const value = view.getBigUint64(at, true);
    if (value > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error("zip: a 64-bit field is too large");
    return Number(value);
}

function text(view: DataView, at: number, length: number, utf8: boolean): string {
    const raw = new Uint8Array(view.buffer, view.byteOffset + at, length);
    // Names without the UTF-8 flag are CP437; the game's are ASCII, which reads the same in both
    if (!utf8 && raw.some((b) => b > 0x7f)) throw new Error("zip: a file name is neither UTF-8 nor ASCII");
    return new TextDecoder("utf-8", { fatal: true }).decode(raw);
}

/// The entries of the central directory, in its order.
export async function readEntries(file: BunFile): Promise<ZipEntry[]> {
    const size = file.size;
    const tailStart = Math.max(0, size - (22 + 0xffff));
    const tail = await bytes(file, tailStart, size);
    let eocd = -1;
    for (let i = tail.byteLength - 22; i >= 0; i--) {
        if (tail.getUint32(i, true) === EOCD) { eocd = i; break; }
    }
    if (eocd < 0) throw new Error("zip: no end of central directory record");
    if (tail.getUint16(eocd + 4, true) !== 0 || tail.getUint16(eocd + 6, true) !== 0) throw new Error("zip: multi-disk archives are not supported");
    let count = tail.getUint16(eocd + 10, true);
    let directorySize = tail.getUint32(eocd + 12, true);
    let directoryOffset = tail.getUint32(eocd + 16, true);
    if (count === 0xffff || directorySize === 0xffffffff || directoryOffset === 0xffffffff) {
        const locator = eocd - 20;
        if (locator < 0 || tail.getUint32(locator, true) !== ZIP64_LOCATOR) throw new Error("zip: no Zip64 locator");
        const recordOffset = u64(tail, locator + 8);
        const record = await bytes(file, recordOffset, recordOffset + 56);
        if (record.getUint32(0, true) !== ZIP64_EOCD) throw new Error("zip: no Zip64 end of central directory record");
        count = u64(record, 32);
        directorySize = u64(record, 40);
        directoryOffset = u64(record, 48);
    }

    const directory = await bytes(file, directoryOffset, directoryOffset + directorySize);
    const entries: ZipEntry[] = [];
    let at = 0;
    for (let n = 0; n < count; n++) {
        if (at + 46 > directory.byteLength || directory.getUint32(at, true) !== CENTRAL) throw new Error(`zip: central directory entry ${n} is damaged`);
        const flags = directory.getUint16(at + 8, true);
        const nameLength = directory.getUint16(at + 28, true);
        const extraLength = directory.getUint16(at + 30, true);
        const commentLength = directory.getUint16(at + 32, true);
        const fileName = text(directory, at + 46, nameLength, (flags & 0x800) !== 0);
        let uncompressedSize = directory.getUint32(at + 24, true);
        let compressedSize = directory.getUint32(at + 20, true);
        let localHeaderOffset = directory.getUint32(at + 42, true);
        let disk = directory.getUint16(at + 34, true);
        // The Zip64 extra field holds, in this order, only the values the fixed fields could not
        for (let e = at + 46 + nameLength, end = e + extraLength; e + 4 <= end;) {
            const id = directory.getUint16(e, true), length = directory.getUint16(e + 2, true);
            if (id === 0x0001) {
                let field = e + 4;
                const take = () => { const v = u64(directory, field); field += 8; return v; };
                if (uncompressedSize === 0xffffffff) uncompressedSize = take();
                if (compressedSize === 0xffffffff) compressedSize = take();
                if (localHeaderOffset === 0xffffffff) localHeaderOffset = take();
                if (disk === 0xffff) disk = directory.getUint32(field, true);
            }
            e += 4 + length;
        }
        if (flags & 0x1) throw new Error(`zip: ${fileName} is encrypted`);
        if (disk !== 0) throw new Error(`zip: ${fileName} starts on another disk`);
        if (fileName.includes("\\")) throw new Error(`zip: ${fileName} has a backslash in its name`);
        entries.push({
            fileName,
            method: directory.getUint16(at + 10, true),
            crc32: directory.getUint32(at + 16, true),
            compressedSize,
            uncompressedSize,
            localHeaderOffset,
        });
        at += 46 + nameLength + extraLength + commentLength;
    }
    return entries;
}

/// An entry's uncompressed bytes.
export async function openEntry(file: BunFile, entry: ZipEntry): Promise<ReadableStream<Uint8Array>> {
    const local = await bytes(file, entry.localHeaderOffset, entry.localHeaderOffset + 30);
    if (local.getUint32(0, true) !== LOCAL) throw new Error(`zip: ${entry.fileName} has no local header`);
    const start = entry.localHeaderOffset + 30 + local.getUint16(26, true) + local.getUint16(28, true);
    const data = file.slice(start, start + entry.compressedSize).stream();
    if (entry.method === 0) return data;
    if (entry.method === 8) return data.pipeThrough(new DecompressionStream("deflate-raw"));
    throw new Error(`zip: ${entry.fileName} uses compression method ${entry.method}`);
}
