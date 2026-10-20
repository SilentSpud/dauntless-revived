// The two pinned DLLs (dxgi.dll proxy and UndauntedInternalServer.dll) go next to the game exe.
// They are checked in the launcher's resources before copying and again after, like friend-kit/setup.ps1.

import { promises as fsp } from "node:fs";
import path from "node:path";
import { hashFile } from "./verify";
import { PINNED_DLLS, WIN64_RELATIVE_DIR, type PinnedDll } from "./constants";

export class DllError extends Error {
  constructor(public readonly dll: string, message: string) {
    super(message);
    this.name = "DllError";
  }
}

export function win64Dir(installDir: string): string {
  return path.join(installDir, ...WIN64_RELATIVE_DIR.split("/"));
}

// Clears the "downloaded from the internet" mark (the Zone.Identifier stream), like Unblock-File.
export async function unblock(file: string): Promise<void> {
  try {
    await fsp.unlink(`${file}:Zone.Identifier`);
  } catch {
    /* no mark, or not NTFS */
  }
}

export async function dllStatus(installDir: string, dlls: readonly PinnedDll[] = PINNED_DLLS): Promise<{ name: string; ok: boolean }[]> {
  const dir = win64Dir(installDir);
  const out: { name: string; ok: boolean }[] = [];
  for (const d of dlls) {
    let ok = false;
    try {
      ok = (await hashFile(path.join(dir, d.name))) === d.sha256;
    } catch {
      ok = false;
    }
    out.push({ name: d.name, ok });
  }
  return out;
}

export async function installPinnedDlls(resourcesDir: string, installDir: string, dlls: readonly PinnedDll[] = PINNED_DLLS): Promise<void> {
  const dir = win64Dir(installDir);
  await fsp.mkdir(dir, { recursive: true });
  for (const d of dlls) {
    const target = path.join(dir, d.name);
    try {
      if ((await hashFile(target)) === d.sha256) {
        await unblock(target);
        continue;
      }
    } catch {
      /* missing: copy it */
    }
    const source = path.join(resourcesDir, d.name);
    let sourceHash: string;
    try {
      sourceHash = await hashFile(source);
    } catch {
      throw new DllError(d.name, "missing from the launcher's resources");
    }
    if (sourceHash !== d.sha256) throw new DllError(d.name, "the launcher's copy does not match the pinned hash");
    const tmp = `${target}.new`;
    await fsp.copyFile(source, tmp);
    if ((await hashFile(tmp)) !== d.sha256) {
      await fsp.unlink(tmp).catch(() => undefined);
      throw new DllError(d.name, "changed while copying (antivirus?)");
    }
    await fsp.rename(tmp, target);
    await unblock(target);
    if ((await hashFile(target)) !== d.sha256) throw new DllError(d.name, "changed after copying (antivirus?)");
  }
}
