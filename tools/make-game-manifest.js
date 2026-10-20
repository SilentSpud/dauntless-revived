// Builds the content manifest (legacy/UndauntedContent/data/dauntless-1.4.4.json) from the verified 1.4.4 game zip.
//
// The manifest lists every file of the game: its path relative to the folder that contains Archon/
// (forward slashes), its size and the SHA-256 of its uncompressed bytes, sorted by path. The content
// server only serves files listed in it, and the friend launcher compiles in a copy and refuses any
// downloaded file whose path, size or hash is not in it, so a compromised server can't push other files.
//
// What it does, in order:
//   1. Hashes the whole zip and refuses to go on unless it matches the pinned SHA-256 of the verified build.
//   2. Reads the zip's entries (streaming, nothing is extracted to disk), hashes each file and checks its
//      CRC-32 and size against the zip's own records.
//   3. Checks that every path is safe (see IsSafeManifestPath; the content server applies the same rule),
//      that no two paths differ only in letter case, and that Version.txt names the 1.4.4 build.
//   4. Writes the manifest and prints the file count, total bytes and the manifest's own SHA-256.
//   5. Optionally (--compare-dir) checks an extracted install against the new manifest: sizes of every
//      file, full hashes of the exe and a few others (or of everything with --compare-all). Read-only.
//
// It runs at low CPU priority and reads the zip twice (about 21 GB of reads in total).
//
// Usage (from the repository root; needs `npm install` in legacy/UndauntedContent/ first, for yauzl):
//   node tools/make-game-manifest.js --zip <BaseGame144.zip> [--out <file>] [--compare-dir <game folder>]
//                                    [--compare-all] [--zip-sha256 <hex>] [--skip-zip-hash]
"use strict";
const fs = require("fs");
const os = require("os");
const path = require("path");
const crypto = require("crypto");
const zlib = require("zlib");
const { createRequire } = require("module");

const root = path.join(__dirname, "..");

const PINNED_ZIP_SHA256 = "556b9a648a5e5e7e11b6f8dd3d80ff8e88fceb0d3448297aaf47ce7bf756bc6d";
const EXPECTED_BUILD = "dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827";
const EXE_PATH = "Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe";
const EXE_SHA256 = "d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4";
const DEFAULT_OUT = path.join(root, "legacy", "UndauntedContent", "data", "dauntless-1.4.4.json");

// The rule every manifest path must pass. Keep in sync with IsSafeManifestPath in
// legacy/UndauntedContent/src/manifest.ts (the server refuses to load a manifest that breaks it).
const SEGMENT = /^[A-Za-z0-9_-](?:[A-Za-z0-9_.-]{0,126}[A-Za-z0-9_-])?$/;
const RESERVED = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i;
function IsSafeManifestPath(p) {
    if (typeof p !== "string" || p.length === 0 || p.length > 400) return false;
    const segments = p.split("/");
    return segments.every((s) => SEGMENT.test(s) && !RESERVED.test(s));
}

function Fail(message) {
    console.error(`ERROR: ${message}`);
    process.exit(1);
}

function ParseArgs(argv) {
    const args = { zip: undefined, out: DEFAULT_OUT, compareDir: undefined, compareAll: false, zipSha256: PINNED_ZIP_SHA256, skipZipHash: false };
    for (let i = 0; i < argv.length; i++) {
        const a = argv[i];
        const next = () => {
            if (i + 1 >= argv.length) Fail(`${a} needs a value`);
            return argv[++i];
        };
        if (a === "--zip") args.zip = next();
        else if (a === "--out") args.out = next();
        else if (a === "--compare-dir") args.compareDir = next();
        else if (a === "--compare-all") args.compareAll = true;
        else if (a === "--zip-sha256") args.zipSha256 = next().toLowerCase();
        else if (a === "--skip-zip-hash") args.skipZipHash = true;
        else Fail(`Unknown argument: ${a}`);
    }
    if (!args.zip) Fail("Usage: node tools/make-game-manifest.js --zip <BaseGame144.zip> [--out <file>] [--compare-dir <game folder>] [--compare-all]");
    if (!/^[0-9a-f]{64}$/.test(args.zipSha256)) Fail("--zip-sha256 must be 64 hex characters");
    return args;
}

function LoadYauzl() {
    const candidates = [
        path.join(root, "legacy", "UndauntedContent", "package.json"),
        path.join(root, "legacy", "UndauntedLauncher", "package.json"),
    ];
    for (const candidate of candidates) {
        try {
            return createRequire(candidate)("yauzl");
        } catch {
            // try the next one
        }
    }
    Fail("yauzl not found. Run `npm install` in legacy/UndauntedContent/ first.");
}

function HashFile(file) {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash("sha256");
        let bytes = 0;
        fs.createReadStream(file, { highWaterMark: 4 << 20 })
            .on("data", (chunk) => {
                hash.update(chunk);
                bytes += chunk.length;
            })
            .on("error", reject)
            .on("end", () => resolve({ sha256: hash.digest("hex"), bytes }));
    });
}

function OpenZip(yauzl, file) {
    return new Promise((resolve, reject) => {
        yauzl.open(file, { lazyEntries: true, autoClose: false, decodeStrings: true, validateEntrySizes: true, strictFileNames: true }, (err, zip) => {
            if (err) reject(err);
            else resolve(zip);
        });
    });
}

// Hashes one entry's uncompressed bytes and checks them against the zip's CRC-32 and size.
function HashEntry(zip, entry) {
    return new Promise((resolve, reject) => {
        zip.openReadStream(entry, (err, stream) => {
            if (err) return reject(err);
            const hash = crypto.createHash("sha256");
            let crc = 0;
            let bytes = 0;
            let text = entry.uncompressedSize <= 4096 ? [] : undefined;
            stream.on("data", (chunk) => {
                hash.update(chunk);
                crc = zlib.crc32(chunk, crc);
                bytes += chunk.length;
                if (text) text.push(chunk);
            });
            stream.on("error", reject);
            stream.on("end", () => {
                if (bytes !== entry.uncompressedSize) return reject(new Error(`${entry.fileName}: ${bytes} bytes, zip says ${entry.uncompressedSize}`));
                if ((crc >>> 0) !== (entry.crc32 >>> 0)) return reject(new Error(`${entry.fileName}: CRC-32 mismatch`));
                resolve({ sha256: hash.digest("hex"), size: bytes, small: text ? Buffer.concat(text) : undefined });
            });
        });
    });
}

async function ReadEntries(yauzl, file) {
    const zip = await OpenZip(yauzl, file);
    const entries = [];
    await new Promise((resolve, reject) => {
        zip.on("entry", (entry) => {
            entries.push(entry);
            zip.readEntry();
        });
        zip.on("end", resolve);
        zip.on("error", reject);
        zip.readEntry();
    });
    return { zip, entries };
}

function CompareStrings(a, b) {
    return a < b ? -1 : a > b ? 1 : 0;
}

async function CompareWithInstall(manifest, dir, all) {
    console.log(`\nComparing with the install at ${dir} (read-only)`);
    let sizeOk = 0;
    const problems = [];
    for (const f of manifest.files) {
        const onDisk = path.join(dir, ...f.path.split("/"));
        let st;
        try {
            st = fs.statSync(onDisk);
        } catch {
            problems.push(`MISSING  ${f.path}`);
            continue;
        }
        if (!st.isFile() || st.size !== f.size) problems.push(`SIZE     ${f.path} (${st.size}, manifest ${f.size})`);
        else sizeOk++;
    }
    console.log(`  sizes: ${sizeOk}/${manifest.files.length} match`);

    // The exe, the smallest and largest pak, Version.txt and a spread of others, unless --compare-all.
    let toHash;
    if (all) {
        toHash = manifest.files;
    } else {
        const paks = manifest.files.filter((f) => f.path.endsWith(".pak")).sort((a, b) => a.size - b.size);
        const picks = new Set([EXE_PATH, "Version.txt", "Dauntless.exe", "Manifest.bin.json"]);
        if (paks.length) {
            picks.add(paks[0].path);
            picks.add(paks[paks.length - 1].path);
        }
        for (let i = 0; i < manifest.files.length; i += 50) picks.add(manifest.files[i].path);
        toHash = manifest.files.filter((f) => picks.has(f.path));
    }
    let hashOk = 0;
    for (const f of toHash) {
        const onDisk = path.join(dir, ...f.path.split("/"));
        if (!fs.existsSync(onDisk)) continue;
        const { sha256 } = await HashFile(onDisk);
        if (sha256 === f.sha256) hashOk++;
        else problems.push(`HASH     ${f.path}`);
    }
    console.log(`  full hashes: ${hashOk}/${toHash.length} match (${toHash.map((f) => path.posix.basename(f.path)).slice(0, 12).join(", ")}${toHash.length > 12 ? ", ..." : ""})`);
    for (const p of problems) console.log(`  ${p}`);
    return problems.length === 0;
}

async function Main() {
    const args = ParseArgs(process.argv.slice(2));
    try {
        os.setPriority(os.constants.priority.PRIORITY_LOW);
    } catch {
        // not fatal
    }
    const yauzl = LoadYauzl();
    const zipPath = path.resolve(args.zip);
    if (!fs.existsSync(zipPath)) Fail(`Zip not found: ${zipPath}`);

    // 1. The zip itself.
    if (args.skipZipHash) {
        console.log("Skipping the zip hash (--skip-zip-hash). Only do this when you checked it yourself.");
    } else {
        console.log(`Hashing ${zipPath} (${fs.statSync(zipPath).size} bytes)...`);
        const t0 = Date.now();
        const { sha256 } = await HashFile(zipPath);
        if (sha256 !== args.zipSha256) Fail(`The zip's SHA-256 is ${sha256}, expected ${args.zipSha256}. Not the verified build.`);
        console.log(`  zip SHA-256 matches (${((Date.now() - t0) / 1000).toFixed(1)} s)`);
    }

    // 2. Its entries.
    const { zip, entries } = await ReadEntries(yauzl, zipPath);
    const fileEntries = entries.filter((e) => !e.fileName.endsWith("/"));
    const prefixes = new Set();
    for (const e of fileEntries) {
        const i = e.fileName.indexOf("Archon/");
        if (i === 0 || (i > 0 && e.fileName[i - 1] === "/")) prefixes.add(e.fileName.slice(0, i));
    }
    if (prefixes.size !== 1) Fail(`Expected exactly one folder containing Archon/, found ${prefixes.size}: ${[...prefixes].join(", ")}`);
    const prefix = [...prefixes][0];
    console.log(`Game root inside the zip: "${prefix || "(zip root)"}"; ${fileEntries.length} files`);

    const outside = fileEntries.filter((e) => !e.fileName.startsWith(prefix));
    if (outside.length) Fail(`Files outside the game root: ${outside.slice(0, 5).map((e) => e.fileName).join(", ")}`);

    const files = [];
    const lower = new Set();
    let version;
    const t1 = Date.now();
    let done = 0;
    let doneBytes = 0;
    const totalBytesInZip = fileEntries.reduce((s, e) => s + e.uncompressedSize, 0);
    for (const e of fileEntries) {
        const rel = e.fileName.slice(prefix.length);
        if (!IsSafeManifestPath(rel)) Fail(`Unsafe path in the zip: ${JSON.stringify(rel)}`);
        if (lower.has(rel.toLowerCase())) Fail(`Two files differ only in letter case: ${rel}`);
        lower.add(rel.toLowerCase());
        const r = await HashEntry(zip, e);
        if (rel === "Version.txt") version = r.small.toString("utf8").trim();
        files.push({ path: rel, size: r.size, sha256: r.sha256 });
        done++;
        doneBytes += r.size;
        if (done % 25 === 0 || done === fileEntries.length) {
            const pct = totalBytesInZip ? ((doneBytes / totalBytesInZip) * 100).toFixed(1) : "100";
            console.log(`  ${done}/${fileEntries.length} files, ${pct}% of bytes, ${((Date.now() - t1) / 1000).toFixed(0)} s`);
        }
    }
    zip.close();

    // 3. Sanity checks on the result.
    if (version !== EXPECTED_BUILD) Fail(`Version.txt says ${JSON.stringify(version)}, expected ${EXPECTED_BUILD}`);
    files.sort((a, b) => CompareStrings(a.path, b.path));
    const exe = files.find((f) => f.path === EXE_PATH);
    if (!exe) Fail(`${EXE_PATH} is not in the zip`);
    if (exe.sha256 !== EXE_SHA256) Fail(`${EXE_PATH} hashes to ${exe.sha256}, expected ${EXE_SHA256}`);

    // 4. Write it.
    const manifest = {
        build: EXPECTED_BUILD,
        totalBytes: files.reduce((s, f) => s + f.size, 0),
        files,
    };
    const text = JSON.stringify(manifest, null, 2) + "\n";
    fs.mkdirSync(path.dirname(path.resolve(args.out)), { recursive: true });
    fs.writeFileSync(args.out, text);
    console.log(`\nWrote ${path.resolve(args.out)}`);
    console.log(`  build:      ${manifest.build}`);
    console.log(`  files:      ${files.length}`);
    console.log(`  totalBytes: ${manifest.totalBytes}`);
    console.log(`  exe:        ${exe.sha256} (${exe.size} bytes)`);
    console.log(`  manifest SHA-256 (as written): ${crypto.createHash("sha256").update(text).digest("hex")}`);

    // 5. Optional cross-check against an extracted install.
    if (args.compareDir) {
        const ok = await CompareWithInstall(manifest, path.resolve(args.compareDir), args.compareAll);
        if (!ok) Fail("The install does not match the manifest (see above).");
        console.log("  install matches");
    }
}

Main().catch((err) => Fail(err && err.stack ? err.stack : String(err)));
