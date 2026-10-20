// Runs after `npm run build` (the "postbuild" script): records which commit the
// build came from in dist/build-info.json, so /undaunted/api/ServerStatus and
// /dauntless-status can name the exact source that is running (roadmap 1.13).
// A build with uncommitted changes in this folder is marked "-dirty". Without git the build still
// succeeds and the commit shows as "unknown"; GIT_COMMIT in .env overrides both.
const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

// Optional argument: another output folder (a test build compiled elsewhere)
const OUT_DIR = path.resolve(path.join(__dirname, ".."), process.argv[2] || "dist");

function git(...args) {
    return execFileSync("git", args, { cwd: path.join(__dirname, ".."), encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
}

let commit = "unknown";
try {
    commit = git("rev-parse", "HEAD");
    // Changes (or new files) under UndauntedMetagame only: the rest of the repo is not in this build
    if (git("status", "--porcelain", "--", ".").length > 0) commit += "-dirty";
} catch {
    // no git, or not a checkout
}

const version = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8")).version;

fs.mkdirSync(OUT_DIR, { recursive: true });
fs.writeFileSync(path.join(OUT_DIR, "build-info.json"), JSON.stringify({ commit, version, builtAt: new Date().toISOString() }, null, 2) + "\n");
console.log(`build info: ${commit} -> ${path.join(OUT_DIR, "build-info.json")}`);
