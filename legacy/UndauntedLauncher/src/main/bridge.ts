// The bridge release (MIGRATION_PLAN.md L50, D2): the last Electron launcher moves a Squirrel install to the
// Tauri launcher. On start it downloads the pinned Tauri installer, checks its SHA-256 against the value
// compiled in below, and installs it. Only once that worked, and after this launcher has exited, does a small
// detached script remove the Squirrel install, run the Tauri installer once more (Squirrel's uninstall takes
// the Start menu shortcut of the same name with it) and start the new launcher.
//
// It fails safe: a failed download, a different hash or a failed install leaves this launcher as it is, and
// the player is pointed at the release page. The settings, keys, logs and game files are not touched: both
// launchers use the same userData folder and game folder, and the Tauri launcher reads the keys Electron
// saved.
//
// Off unless BRIDGE_TARGET is set: the release that turns it on sets the version and hash of the published
// Tauri installer (L50), and nothing else changes.

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import path from "node:path";

export interface BridgeTarget {
  // The Tauri launcher version the bridge installs (the first of the new line: 0.2.0)
  version: string;
  // SHA-256 of its DauntlessRevivedLauncher-Setup.exe, lower-case hex
  sha256: string;
}

// Set by the bridge release (L50); null keeps the bridge off.
export const BRIDGE_TARGET: BridgeTarget | null = null;

export function installerUrl(target: BridgeTarget): string {
  return `https://github.com/mixutin/dauntless-revived/releases/download/launcher-v${target.version}/DauntlessRevivedLauncher-Setup.exe`;
}

export function releasePage(target: BridgeTarget): string {
  return `https://github.com/mixutin/dauntless-revived/releases/tag/launcher-v${target.version}`;
}

export interface BridgeDeps {
  // A Squirrel install of a packaged launcher on Windows: the folder that holds Update.exe
  squirrelRoot: string | null;
  tempDir: string;
  // Where the Tauri installer puts the launcher for the current user
  tauriExe: string;
  pid: number;
  download(url: string, file: string): Promise<void>;
  // Runs the installer and resolves with its exit code
  install(installer: string, args: string[]): Promise<number>;
  exists(file: string): boolean;
  // Starts a detached script that outlives this process
  startDetached(script: string): void;
  remove(file: string): Promise<void>;
  log(message: string): void;
}

export type BridgeOutcome =
  | { kind: "off" }
  | { kind: "moved" }
  | { kind: "failed"; reason: string; page: string };

export async function fileSha256(file: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

const HEX64 = /^[0-9a-f]{64}$/;
const VERSION = /^\d+\.\d+\.\d+$/;

// A PowerShell single-quoted literal
function quote(text: string): string {
  return `'${text.replace(/'/g, "''")}'`;
}

// The script that runs after this launcher has exited: wait, remove Squirrel, install again, start.
export function handoverScript(pid: number, updateExe: string, installer: string, tauriExe: string): string {
  return [
    "$ErrorActionPreference = 'SilentlyContinue'",
    `$deadline = (Get-Date).AddSeconds(60)`,
    `while ((Get-Process -Id ${pid} -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }`,
    `Start-Process -FilePath ${quote(updateExe)} -ArgumentList '--uninstall','-s' -Wait`,
    `Start-Process -FilePath ${quote(installer)} -ArgumentList '/S' -Wait`,
    `Remove-Item -LiteralPath ${quote(installer)} -Force`,
    `Start-Process -FilePath ${quote(tauriExe)}`,
  ].join("\r\n");
}

export async function runBridge(target: BridgeTarget | null, deps: BridgeDeps, hash = fileSha256): Promise<BridgeOutcome> {
  if (!target || !deps.squirrelRoot) return { kind: "off" };
  const page = releasePage(target);
  const fail = (reason: string): BridgeOutcome => {
    deps.log(`bridge: staying on this launcher: ${reason}`);
    return { kind: "failed", reason, page };
  };
  if (!VERSION.test(target.version) || !HEX64.test(target.sha256)) return fail("the bridge target is malformed");
  const updateExe = path.join(deps.squirrelRoot, "Update.exe");
  if (!deps.exists(updateExe)) return fail("this is not a Squirrel install");
  const installer = path.join(deps.tempDir, `DauntlessRevivedLauncher-${target.version}-Setup.exe`);
  try {
    deps.log(`bridge: downloading launcher ${target.version}`);
    await deps.download(installerUrl(target), installer);
    const actual = await hash(installer);
    if (actual !== target.sha256) {
      await deps.remove(installer).catch(() => undefined);
      return fail(`the downloaded installer is not the pinned one (${actual})`);
    }
    const code = await deps.install(installer, ["/S"]);
    if (code !== 0 || !deps.exists(deps.tauriExe)) {
      return fail(`the installer did not finish (exit code ${code})`);
    }
  } catch (error) {
    await deps.remove(installer).catch(() => undefined);
    return fail(error instanceof Error ? error.message : String(error));
  }
  deps.startDetached(handoverScript(deps.pid, updateExe, installer, deps.tauriExe));
  deps.log(`bridge: launcher ${target.version} is installed; handing over`);
  return { kind: "moved" };
}
