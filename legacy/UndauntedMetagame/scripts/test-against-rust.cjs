// Runs the HTTP-level tests against the Rust metagame (MIGRATION_PLAN.md M31):
//   npm run test:rust -- <path to the metagame binary>
// (or METAGAME_RUST_BIN set). Run `npm test` first: it builds build/test. The same files pass on the
// Node app; tests marked NodeOnly are skipped here.
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

// Files that start the app themselves (app.listen) and are run through build/test/rustshim.js. Left
// out: guildhttp (its tests share state and move the guild clock in process), the chat tests (chat is
// not in the Rust metagame yet, M32), and the tests that put a router under test in their own app.
const SELF_STARTED = ["friendsapi", "partyhttp", "platform", "progressiondefault", "socialflow"];

const Bin = process.argv[2] ?? process.env.METAGAME_RUST_BIN;
if (!Bin || !fs.existsSync(Bin)) {
    console.error("test:rust needs the Rust metagame binary: npm run test:rust -- <path>, or METAGAME_RUST_BIN");
    process.exit(2);
}

const Root = path.join(__dirname, "..");
const Built = path.join(Root, "build", "test");
const Shared = fs.readdirSync(path.join(Root, "test"))
    .filter((Name) => Name.endsWith(".test.ts") && fs.readFileSync(path.join(Root, "test", Name), "utf8").includes('from "./appclient"'))
    .map((Name) => Name.replace(/\.ts$/, ".js"));
const Files = [...Shared, ...SELF_STARTED.map((Name) => `${Name}.test.js`)].map((Name) => path.join(Built, Name));

const Missing = Files.filter((File) => !fs.existsSync(File));
if (Missing.length > 0) {
    console.error(`Not built (run npm test first): ${Missing.join(", ")}`);
    process.exit(2);
}

const Result = spawnSync(process.execPath, ["--require", path.join(Built, "rustshim.js"), "--test", ...Files], {
    stdio: "inherit",
    env: { ...process.env, METAGAME_RUST_BIN: path.resolve(Bin) }
});
process.exit(Result.status ?? 1);
