import { existsSync } from 'node:fs';
import path from 'node:path';
export const DIRECTX_FILES = ['XINPUT1_3.dll','X3DAudio1_7.dll','XAPOFX1_5.dll'];
export function missingDirectX(env: NodeJS.ProcessEnv = process.env, exists: (p:string) => boolean = existsSync, platform: NodeJS.Platform = process.platform): string[] {
  if (platform !== 'win32') return [];
  return DIRECTX_FILES.filter(file => !exists(path.join(env.WINDIR ?? env.SystemRoot ?? 'C:\\Windows','System32',file)));
}
export function gameExitDetail(code: number): string {
  const hex = `0x${(code >>> 0).toString(16).toUpperCase().padStart(8,'0')}`;
  if ((code >>> 0) === 0xC0000135) return `${hex}: A required DLL is missing. Install the Microsoft Visual C++ x64 and DirectX runtimes shown below, then retry.`;
  if ((code >>> 0) === 0xC000007B) return `${hex}: Windows could not load a game dependency. Repair the Microsoft x64 runtimes and use Repair game files.`;
  return `${hex}: The game closed unexpectedly. Use Repair game files; if it repeats, send the latest Archon game log to support.`;
}
