// The bridge release (src/main/bridge.ts, MIGRATION_PLAN.md L50): it only moves to the pinned installer, and
// anything that goes wrong before the handover leaves the Electron launcher as it is.

import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { BRIDGE_TARGET, handoverScript, installerUrl, runBridge, type BridgeDeps } from "../src/main/bridge";

const TARGET = { version: "0.2.0", sha256: "a".repeat(64) };

function deps(overrides: Partial<BridgeDeps> = {}): BridgeDeps & { calls: string[] } {
  const calls: string[] = [];
  const root = path.join("C:", "Users", "x", "AppData", "Local", "DauntlessRevivedLauncher");
  return {
    calls,
    squirrelRoot: root,
    tempDir: path.join("C:", "Temp"),
    tauriExe: path.join("C:", "Users", "x", "AppData", "Local", "Dauntless Revived Launcher", "dauntless-revived-launcher.exe"),
    pid: 4242,
    download: async (url) => void calls.push(`download ${url}`),
    install: async (installer, args) => (calls.push(`install ${path.basename(installer)} ${args.join(" ")}`), 0),
    exists: () => true,
    startDetached: (script) => void calls.push(`detached ${script.includes("--uninstall") ? "handover" : "other"}`),
    remove: async (file) => void calls.push(`remove ${path.basename(file)}`),
    log: () => undefined,
    ...overrides,
  };
}

test("the bridge ships off", () => {
  assert.equal(BRIDGE_TARGET, null);
});

test("off without a target or outside a Squirrel install", async () => {
  assert.deepEqual(await runBridge(null, deps()), { kind: "off" });
  assert.deepEqual(await runBridge(TARGET, deps({ squirrelRoot: null })), { kind: "off" });
});

test("installs the pinned installer, then hands over to a detached script", async () => {
  const d = deps();
  assert.deepEqual(await runBridge(TARGET, d, async () => TARGET.sha256), { kind: "moved" });
  assert.deepEqual(d.calls, [
    `download ${installerUrl(TARGET)}`,
    "install DauntlessRevivedLauncher-0.2.0-Setup.exe /S",
    "detached handover",
  ]);
});

test("a different installer, a failed install or a failed download changes nothing", async () => {
  const wrong = deps();
  const outcome = await runBridge(TARGET, wrong, async () => "b".repeat(64));
  assert.equal(outcome.kind, "failed");
  assert.ok(outcome.kind === "failed" && outcome.page.endsWith("/launcher-v0.2.0"));
  assert.deepEqual(wrong.calls, [`download ${installerUrl(TARGET)}`, "remove DauntlessRevivedLauncher-0.2.0-Setup.exe"]);

  const refused = deps({ install: async () => 2 });
  assert.equal((await runBridge(TARGET, refused, async () => TARGET.sha256)).kind, "failed");
  assert.ok(!refused.calls.some((c) => c.startsWith("detached")), "no handover after a failed install");

  const offline = deps({ download: async () => { throw new Error("no network"); } });
  assert.equal((await runBridge(TARGET, offline)).kind, "failed");
  assert.ok(!offline.calls.some((c) => c.startsWith("install")));

  const notSquirrel = deps({ exists: (file) => !file.endsWith("Update.exe") });
  assert.equal((await runBridge(TARGET, notSquirrel, async () => TARGET.sha256)).kind, "failed");

  for (const bad of [{ version: "0.2", sha256: TARGET.sha256 }, { version: "0.2.0", sha256: "A".repeat(64) }]) {
    assert.equal((await runBridge(bad, deps(), async () => TARGET.sha256)).kind, "failed");
  }
});

test("the handover script waits, removes Squirrel, reinstalls and starts the new launcher, quoting paths", () => {
  const script = handoverScript(4242, "C:\\Users\\O'Neil\\Update.exe", "C:\\Temp\\setup.exe", "C:\\New\\launcher.exe");
  const lines = script.split("\r\n");
  assert.ok(lines[2].includes("Get-Process -Id 4242"));
  assert.ok(lines[3].includes("'C:\\Users\\O''Neil\\Update.exe'") && lines[3].includes("'--uninstall','-s'"));
  assert.ok(lines[4].includes("'C:\\Temp\\setup.exe'") && lines[4].includes("'/S'"));
  assert.ok(lines.at(-1)!.includes("'C:\\New\\launcher.exe'"));
});
