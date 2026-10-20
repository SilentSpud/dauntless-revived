import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
const version = pkg.version;
const out = path.join(root, "out");
const make = path.join(out, "make");
const release = path.join(root, "release-linux");
const skipRpm = process.env.DAUNTLESS_REVIVED_SKIP_RPM === "1";

function one(dir, ext) {
  const files = readdirSync(dir).filter((name) => name.endsWith(ext));
  if (files.length !== 1) throw new Error(`expected one ${ext} in ${dir}, found ${files.length}`);
  return path.join(dir, files[0]);
}

function copy(source, name) {
  cpSync(source, path.join(release, name));
}

rmSync(release, { recursive: true, force: true });
mkdirSync(release, { recursive: true });

copy(one(path.join(make, "AppImage", "x64"), ".AppImage"), `DauntlessRevivedLauncher-${version}-linux-x86_64.AppImage`);
copy(one(path.join(make, "deb", "x64"), ".deb"), `DauntlessRevivedLauncher-${version}-linux-amd64.deb`);
if (!skipRpm) copy(one(path.join(make, "rpm", "x64"), ".rpm"), `DauntlessRevivedLauncher-${version}-linux-x86_64.rpm`);
copy(one(path.join(make, "zip", "linux", "x64"), ".zip"), `DauntlessRevivedLauncher-${version}-linux-x64.zip`);

const tarName = `DauntlessRevivedLauncher-${version}-linux-x64.tar.gz`;
const packaged = path.join(out, "Dauntless Revived Launcher-linux-x64");
const tar = spawnSync("tar", ["-czf", path.join(release, tarName), "-C", packaged, "."], { stdio: "inherit" });
if (tar.status !== 0) throw new Error("tar failed");

const checksumName = "SHA256SUMS-LINUX.txt";
const names = readdirSync(release).filter((name) => name !== checksumName).sort();
const sums = names.map((name) => {
  const hash = createHash("sha256").update(readFileSync(path.join(release, name))).digest("hex");
  return `${hash}  ${name}`;
}).join("\n");
await import("node:fs/promises").then(({ writeFile }) => writeFile(path.join(release, checksumName), sums + "\n"));

console.log(`Linux release ${version}: ${names.length} artifacts + ${checksumName}`);
for (const name of [...names, checksumName]) console.log(name);
