// Repository hygiene check, run by CI on every push and pull request. Fails when:
//
//   - a tracked file is a secret or a game file by its name: .env files (any "*.env" or ".env.*" except
//     .env.example), keys and certificates, databases, logs, game archives and assets (.pak .ucas .utoc
//     .uasset ...), executables and DLLs, archives, or anything under the ignored /data/ and /BaseGame144/
//     folders;
//   - a tracked file matches a .gitignore rule (it was added with --force);
//   - a tracked file is byte for byte a file of the Dauntless 1.4.4 client (its SHA-256 is in the game
//     manifest UndauntedContent/data/dauntless-1.4.4.json);
//   - a tracked file contains something shaped like a real secret: an account key (UUK_ and 48 hex
//     characters, as the metagame makes them), a JWT, a private key block (also base64-encoded, the way
//     AUTH_SIGNING_PRIVKEY_B64 stores one), a Tailscale auth key or a GitHub token;
//   - the two prebuilt server DLLs in UndauntedLauncher/assets/ differ from the SHA-256 pins in the launcher
//     and the Windows Server kit;
//   - UndauntedLauncher/package.json "version" is not a launcher version (a semantic version without build
//     metadata; it names the launcher release tag launcher-v<version>; see tools/ci/launcher-version.js).
//
// With --history <revision range> (CI passes the commits of the push or pull request, e.g. abc123..HEAD),
// every file version those commits added or changed gets the name, game-file and content checks too: a
// secret committed and then deleted again is still public in the history, so it has to be rotated.
//
// Test fixtures that look like secrets on purpose are allowed by path below; an allow-list entry that no
// longer matches anything in the checked-out files fails too, so the list stays exact. Nothing is printed
// of what a match contains. No dependencies. Usage (from anywhere in the checkout):
//   node tools/ci/check-repo.js [--history <revision range>]      (--history HEAD: the whole history)
"use strict";
const { execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const { isLauncherVersion } = require("./launcher-version.js");

const ROOT = path.resolve(__dirname, "..", "..");
const LAUNCHER_PACKAGE = "UndauntedLauncher/package.json";
const GAME_MANIFEST = "UndauntedContent/data/dauntless-1.4.4.json";

// Tracked files that trip a content check on purpose: path -> the checks it may trip.
const ALLOWED_CONTENT = {
    // A made-up token (its signature part reads "signature-not-real-...") for the gateway's redaction tests.
    "UndauntedGateway/test/gateway.test.ts": ["jwt"],
    "UndauntedGateway/test/redact-ratelimit-config.test.ts": ["jwt"],
    "UndauntedLauncher/scripts/portable-save-smoke.cjs": ["private-key"],
    // Throwaway key pair, tokens and recovery blob made by make-node-crypto.cjs for the Rust metagame's
    // compatibility tests; nothing in it is used by a server.
    "rust/services/metagame/tests/fixtures/node-crypto.json": ["private-key-b64", "jwt", "account-key"],
};

// The only DLLs the repository may hold: Undaunted's prebuilt server DLLs, which the launcher ships and the
// kit installs. Each must match its pin in both files below.
const PINNED_DLLS = ["UndauntedLauncher/assets/dxgi.dll", "UndauntedLauncher/assets/UndauntedInternalServer.dll"];
// UE4SS (MIT) and its loader, for the optional client mods in client-mods/. Each must match its SHA-256 in
// client-mods/manifest.json, which the launcher and the friend kit check before installing.
const CLIENT_MODS_MANIFEST = "client-mods/manifest.json";
const CLIENT_MODS_DLLS = ["client-mods/dwmapi.dll", "client-mods/ue4ss/UE4SS.dll"];
const DLL_PIN_SOURCES = [
    { file: "UndauntedLauncher/src/main/constants.ts", pattern: /name:\s*"([^"]+\.dll)",\s*sha256:\s*"([0-9a-fA-F]{64})"/g },
    { file: "deploy/windows-server/DauntlessServer.Common.ps1", pattern: /'([^']+\.dll)'\s*=\s*'([0-9a-fA-F]{64})'/g },
];

// Forbidden by name. Tested against the lower-cased path with forward slashes.
const NAME_RULES = [
    { why: "a .env file (secrets)", test: (p, base) => (base === ".env" || base.startsWith(".env.") || base.endsWith(".env")) && base !== ".env.example" },
    { why: "a key or certificate file", test: (p, base) => /\.(key|pem|pfx|p12|jks|keystore|ppk)$/.test(base) || /^id_(rsa|dsa|ecdsa|ed25519)(_sk)?$/.test(base) },
    { why: "a database", test: (p, base) => /\.(db|db-wal|db-shm|db-journal|sqlite|sqlite3)$/.test(base) },
    { why: "a log file", test: (p, base) => base.endsWith(".log") },
    { why: "a game archive or asset", test: (p, base) => /\.(pak|ucas|utoc|uasset|uexp|umap|ubulk|ufont)$/.test(base) },
    { why: "an executable or installer", test: (p, base) => /\.(exe|msi)$/.test(base) },
    { why: "an archive (it could hold game files)", test: (p, base) => /\.(zip|7z|rar|tar|tgz|gz|xz|bz2|zst|nupkg)$/.test(base) },
    { why: "a DLL other than the pinned server and client-mods DLLs", test: (p, base) => base.endsWith(".dll") && ![...PINNED_DLLS, ...CLIENT_MODS_DLLS].some((d) => d.toLowerCase() === p) },
    { why: "under /data/ (runtime data, ignored)", test: (p) => p.startsWith("data/") },
    { why: "under /BaseGame144/ (the game, ignored)", test: (p) => p.startsWith("basegame144/") || p.startsWith("basegame144.") },
    { why: "in a game folder (Archon/)", test: (p) => /(^|\/)archon\//.test(p) },
];

// Content shapes. Each regex is global; `confirm` can reject a candidate match.
const CONTENT_RULES = [
    { id: "account-key", why: "an account key (UUK_ + 48 hex)", re: /UUK_[0-9a-fA-F]{48}(?![0-9a-fA-F])/g },
    { id: "jwt", why: "a JWT", re: /eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{16,}/g },
    { id: "private-key", why: "a private key block", re: /-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----/g },
    {
        id: "private-key-b64",
        why: "a base64-encoded private key block",
        // "-----BEGIN " in base64; decode what follows and look for PRIVATE KEY.
        re: /LS0tLS1CRUdJTi[A-Za-z0-9+/]{20,}/g,
        confirm: (m) => /-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY/.test(Buffer.from(m.slice(0, 80), "base64").toString("latin1")),
    },
    { id: "tailscale-key", why: "a Tailscale key", re: /tskey-(?:auth|api|client|scim|webhook)-[A-Za-z0-9]{6,}-[A-Za-z0-9]{16,}/g },
    { id: "github-token", why: "a GitHub token", re: /\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{60,})\b/g },
];

const args = process.argv.slice(2);
let historyRange = null;
if (args.length === 2 && args[0] === "--history" && args[1] !== "") historyRange = args[1];
else if (args.length !== 0) {
    console.error("usage: node tools/ci/check-repo.js [--history <revision range>]");
    process.exit(2);
}

const problems = [];
const seenProblems = new Set();
function fail(file, message) {
    const problem = file ? `${file}: ${message}` : message;
    if (!seenProblems.has(problem)) {
        seenProblems.add(problem);
        problems.push(problem);
    }
}

function git(gitArgs, options = {}) {
    return execFileSync("git", gitArgs, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...options });
}

function sha256(buffer) {
    return crypto.createHash("sha256").update(buffer).digest("hex");
}

// A text file can differ from the original only in its line endings (git normalizes them), so a game
// file is also recognized with LF or CRLF line endings.
function lineEndingVariants(buffer) {
    if (!buffer.includes(0x0a)) return [];
    const text = buffer.toString("latin1");
    const lf = text.replace(/\r\n/g, "\n");
    return [Buffer.from(lf, "latin1"), Buffer.from(lf.replace(/\n/g, "\r\n"), "latin1")];
}

function lineOf(text, index) {
    let line = 1;
    for (let i = text.indexOf("\n"); i !== -1 && i < index; i = text.indexOf("\n", i + 1)) line++;
    return line;
}

// ---------------------------------------------------------------------------------------------------------
const files = git(["ls-files", "-z", "--cached"]).split("\0").filter(Boolean);
if (files.length === 0) fail(null, "git ls-files returned nothing; run this inside the checkout");

// Tracked files that a .gitignore rule covers.
for (const file of git(["ls-files", "-z", "--cached", "--ignored", "--exclude-standard"]).split("\0").filter(Boolean)) {
    fail(file, "is tracked although a .gitignore rule excludes it");
}

// The game manifest: SHA-256 -> game path (empty files excluded: every empty file has the same hash).
const gameHashes = new Map();
let gameFiles = 0;
try {
    for (const entry of JSON.parse(fs.readFileSync(path.join(ROOT, GAME_MANIFEST), "utf8")).files) {
        gameFiles++;
        if (entry.size > 0) gameHashes.set(entry.sha256.toLowerCase(), entry.path);
    }
} catch (error) {
    fail(GAME_MANIFEST, `could not be read (${error.message})`);
}
if (gameFiles < 400) fail(GAME_MANIFEST, `lists only ${gameFiles} game files; expected the full 1.4.4 manifest (410)`);

const allowUsed = new Set();
const fileHashes = new Map();
let scanned = 0;
let scannedBytes = 0;

function checkName(file, where) {
    const lower = file.toLowerCase();
    const base = lower.slice(lower.lastIndexOf("/") + 1);
    for (const rule of NAME_RULES) {
        if (rule.test(lower, base)) fail(file, `is ${rule.why}${where}`);
    }
}

// `where` names the commit for a file version from the history; allow-list use is counted for the
// checked-out files only.
function checkData(file, data, where) {
    if (data.length > 0 && file !== GAME_MANIFEST) {
        for (const variant of [data, ...lineEndingVariants(data)]) {
            const game = gameHashes.get(sha256(variant));
            if (game) {
                fail(file, `is identical to the game file ${game}${where}`);
                break;
            }
        }
    }

    // Every file is scanned as bytes, so a text file with stray control characters is not skipped as binary.
    const text = data.toString("latin1");
    for (const rule of CONTENT_RULES) {
        rule.re.lastIndex = 0;
        let match;
        while ((match = rule.re.exec(text)) !== null) {
            if (rule.confirm && !rule.confirm(match[0])) continue;
            if ((ALLOWED_CONTENT[file] || []).includes(rule.id)) {
                if (!where) allowUsed.add(`${file}|${rule.id}`);
                continue;
            }
            fail(file, `line ${lineOf(text, match.index)} contains ${rule.why}${where}`);
        }
    }
}

for (const file of files) {
    checkName(file, "");

    const full = path.join(ROOT, ...file.split("/"));
    let data;
    try {
        const stat = fs.lstatSync(full);
        if (!stat.isFile()) continue;   // a symlink or a submodule
        data = fs.readFileSync(full);
    } catch {
        continue;   // deleted in the working tree
    }
    scanned++;
    scannedBytes += data.length;
    fileHashes.set(file, sha256(data));
    checkData(file, data, "");
}

// ---------------------------------------------------------------------------------------------------------
// The history: every file version that the commits in historyRange added or changed.
let historyCommits = 0;
let historyVersions = 0;
if (historyRange !== null) {
    let commits = [];
    try {
        commits = git(["rev-list", "--no-merges", historyRange, "--"]).split("\n").filter(Boolean);
    } catch (error) {
        fail(null, `--history ${historyRange}: git rev-list failed (${String(error.stderr || error.message).trim()})`);
    }
    historyCommits = commits.length;

    // The blobs of the checked-out commit were checked above (under their current names).
    const tipFiles = new Set(files);
    const tipBlobs = new Set(git(["ls-files", "-z", "--stage"]).split("\0").filter(Boolean).map((line) => line.split(" ")[1]));
    const blobs = new Map();   // blob id -> { file, commit }
    for (const commit of commits) {
        // Records of ":oldmode newmode oldblob newblob status", each followed by its path.
        const out = git(["diff-tree", "-r", "--root", "--no-commit-id", "--no-renames", "--no-abbrev", "-z", "--diff-filter=AMT", commit]).split("\0");
        for (let i = 0; i + 1 < out.length; i += 2) {
            const meta = out[i].split(" ");
            const file = out[i + 1];
            if (meta.length < 5 || !file) continue;
            const mode = meta[1], blob = meta[3];
            if (!tipFiles.has(file)) checkName(file, ` (in commit ${commit.slice(0, 12)})`);
            if (mode === "160000" || mode === "120000") continue;   // a submodule or a symlink
            if (!tipBlobs.has(blob) && !blobs.has(blob)) blobs.set(blob, { file, commit });
        }
    }

    // Read the blobs in batches with git cat-file: "<id> blob <size>\n<content>\n" each.
    const ids = [...blobs.keys()];
    for (let start = 0; start < ids.length; start += 200) {
        const batch = ids.slice(start, start + 200);
        const out = execFileSync("git", ["cat-file", "--batch"], { cwd: ROOT, input: batch.join("\n") + "\n", maxBuffer: 1024 * 1024 * 1024 });
        let pos = 0;
        for (const id of batch) {
            const eol = out.indexOf(0x0a, pos);
            const header = out.toString("latin1", pos, eol).split(" ");
            if (header[0] !== id || header[1] !== "blob") {
                fail(null, `--history: git cat-file could not read ${id} (${header.join(" ")})`);
                break;
            }
            const size = Number(header[2]);
            const data = out.subarray(eol + 1, eol + 1 + size);
            pos = eol + 1 + size + 1;
            const { file, commit } = blobs.get(id);
            historyVersions++;
            checkData(file, data, ` (in commit ${commit.slice(0, 12)})`);
        }
    }
}

for (const [file, ids] of Object.entries(ALLOWED_CONTENT)) {
    for (const id of ids) {
        if (!allowUsed.has(`${file}|${id}`)) fail("tools/ci/check-repo.js", `allow-list entry ${file} (${id}) no longer matches anything; remove it`);
    }
}

// The prebuilt DLLs against their pins.
for (const source of DLL_PIN_SOURCES) {
    let text = "";
    try {
        text = fs.readFileSync(path.join(ROOT, source.file), "utf8");
    } catch (error) {
        fail(source.file, `could not be read (${error.message})`);
        continue;
    }
    const pins = new Map([...text.matchAll(source.pattern)].map((m) => [m[1].toLowerCase(), m[2].toLowerCase()]));
    for (const dll of PINNED_DLLS) {
        const name = dll.slice(dll.lastIndexOf("/") + 1).toLowerCase();
        if (!fileHashes.has(dll)) fail(dll, "is missing (it must be tracked)");
        else if (!pins.has(name)) fail(source.file, `has no SHA-256 pin for ${name}`);
        else if (pins.get(name) !== fileHashes.get(dll)) fail(dll, `does not match its SHA-256 pin in ${source.file}`);
    }
}

// The client-mods DLLs against client-mods/manifest.json.
try {
    const pins = new Map(JSON.parse(fs.readFileSync(path.join(ROOT, CLIENT_MODS_MANIFEST), "utf8")).files.map((f) => [`client-mods/${f.path}`, String(f.sha256).toLowerCase()]));
    for (const dll of CLIENT_MODS_DLLS) {
        if (!fileHashes.has(dll)) fail(dll, "is missing (it must be tracked)");
        else if (!pins.has(dll)) fail(CLIENT_MODS_MANIFEST, `has no SHA-256 pin for ${dll}`);
        else if (pins.get(dll) !== fileHashes.get(dll)) fail(dll, `does not match its SHA-256 pin in ${CLIENT_MODS_MANIFEST}`);
    }
} catch (error) {
    fail(CLIENT_MODS_MANIFEST, `could not be read (${error.message})`);
}

// The launcher version names the release tag launcher-v<version>.
try {
    const version = JSON.parse(fs.readFileSync(path.join(ROOT, LAUNCHER_PACKAGE), "utf8")).version;
    if (!isLauncherVersion(version)) fail(LAUNCHER_PACKAGE, `"version" ${JSON.stringify(version)} is not a launcher version (x.y.z or x.y.z-prerelease, no +build metadata)`);
} catch (error) {
    fail(LAUNCHER_PACKAGE, `could not be read (${error.message})`);
}

// ---------------------------------------------------------------------------------------------------------
console.log(`${scanned} tracked files checked (${(scannedBytes / 1024 / 1024).toFixed(1)} MB)`);
if (historyRange !== null) console.log(`history ${historyRange}: ${historyCommits} commit(s), ${historyVersions} earlier file version(s) checked`);
if (problems.length > 0) {
    console.error(`\n${problems.length} problem(s):`);
    for (const problem of problems) console.error(`  - ${problem}`);
    console.error("\nSecrets and game files must never be committed (CONTRIBUTING.md, rule 3). If a key was committed, rotate it.");
    process.exit(1);
}
console.log("no secrets or game files found; the launcher version is valid");
