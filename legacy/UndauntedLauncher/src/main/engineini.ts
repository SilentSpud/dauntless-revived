// Rewrites the game's user config before each launch, exactly like friend-kit/play.ps1:
//
//  - Engine.ini: [SystemSettings] (memory lines, plus the forced graphics level if one is chosen, plus
//    r.EyeAdaptation.MethodOverride=2 when the player opted into "Basic adaptive" auto exposure)
//    and [OnlineSubsystemMcp.XMPP] (chat/presence pointed at the host instead of Epic's live
//    server, which would otherwise receive the account id and login token) are replaced; every
//    other section of the file is kept as it was.
//    Private mode: ws://<host>:61099, like play.ps1. Public mode: ws://127.0.0.1:<relay port>, the
//    launcher's local relay, which carries the WebSocket to the server over pinned TLS.
//  - GameUserSettings.ini: when a graphics level is forced, its sg.*Quality lines are set to match.

import { repairDisplaySettings } from './display-repair';

import { promises as fsp } from "node:fs";
import path from "node:path";
import { isValidHost } from "../shared/invite";
import { EXPOSURE_MODES, type ExposureMode, type GraphicsPreset } from "../shared/types";
import { XMPP_PORT } from "./constants";

export const SCALABILITY_GROUPS = ["ViewDistance", "AntiAliasing", "Shadow", "PostProcess", "Texture", "Effects", "Foliage", "Shading"];

export function defaultConfigDir(env: NodeJS.ProcessEnv = process.env): string {
  const base = env.LOCALAPPDATA ?? path.join(env.USERPROFILE ?? "C:\\", "AppData", "Local");
  return path.join(base, "Archon", "Saved", "Config", "WindowsClient");
}

type Encoding = "ascii" | "utf16le";

export function decodeIni(data: Buffer): string {
  if (data.length >= 2 && data[0] === 0xff && data[1] === 0xfe) return data.subarray(2).toString("utf16le");
  if (data.length >= 3 && data[0] === 0xef && data[1] === 0xbb && data[2] === 0xbf) return data.subarray(3).toString("utf8");
  return data.toString("utf8");
}

// Plain ASCII (what play.ps1 writes) unless the kept lines contain other characters; then UTF-16
// with a byte order mark, which Unreal reads correctly, instead of turning them into "?".
export function encodeIni(lines: string[]): Buffer {
  const text = lines.map((l) => l + "\r\n").join("");
  const encoding: Encoding = /^[\x00-\x7f]*$/.test(text) ? "ascii" : "utf16le";
  if (encoding === "ascii") return Buffer.from(text, "latin1");
  return Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(text, "utf16le")]);
}

// Get-Content splits on CRLF, LF or CR and drops the final line break.
export function splitLines(text: string): string[] {
  if (text.length === 0) return [];
  const lines = text.split(/\r\n|\n|\r/);
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

// No r.EyeAdaptationQuality=0 (0.1.0 wrote it to fix the dark pre-hunt airship): turning automatic exposure
// off made Ramsgate and every night scene far too dark, so the game's own exposure stays on. Because this
// section is replaced on every launch, the line 0.1.0 wrote disappears from existing Engine.ini files.
// The only exposure line is the opt-in r.EyeAdaptation.MethodOverride=2 ("Basic adaptive", Vvoidddd, #7):
// UE4's basic metering instead of the histogram, with automatic exposure still on. It comes last, so
// with "game" (the default) the section is byte for byte what friend-kit/play.ps1 writes.
export function systemSettingsLines(graphics: GraphicsPreset, exposure: ExposureMode = "game"): string[] {
  if (!EXPOSURE_MODES.includes(exposure)) throw new Error("invalid exposure mode");
  const sys = [
    "[SystemSettings]",
    "r.Streaming.PoolSize=3000",
    "r.Streaming.LimitPoolSizeToVRAM=1",
    "gc.TimeBetweenPurgingPendingKillObjects=10",
    "s.ForceGCAfterLevelStreamedOut=1",
  ];
  if (graphics >= 0) {
    for (const g of SCALABILITY_GROUPS) sys.push(`sg.${g}Quality=${graphics}`);
    sys.push("sg.ResolutionQuality=100", "r.ScreenPercentage=100", "r.MipMapLODBias=0", "r.MaxAnisotropy=16", "r.Tonemapper.Sharpen=0.6");
  }
  // The hash-pinned 1.4.4 executable's help text for this cvar: "2: Auto Basic". Unlike the old
  // EyeAdaptationQuality=0 workaround, it keeps adaptation on in dark maps. Opt-in until the airship,
  // Ramsgate and a night hunt are compared in game (roadmap 4.17).
  if (exposure === "basic") sys.push("r.EyeAdaptation.MethodOverride=2");
  return sys;
}

export function xmppLines(host: string, port: number = XMPP_PORT): string[] {
  if (!isValidHost(host)) throw new Error("invalid host");
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("invalid port");
  return ["[OnlineSubsystemMcp.XMPP]", `ServerAddr="ws://${host}"`, `ServerPort=${port}`, "bUseSSL=false"];
}

export function rewriteEngineIniText(existing: string[], host: string, graphics: GraphicsPreset, xmppPort: number = XMPP_PORT, exposure: ExposureMode = "game"): string[] {
  const keep: string[] = [];
  let skip = false;
  for (const l of existing) {
    if (/^\[(SystemSettings|OnlineSubsystemMcp\.XMPP)\]/i.test(l)) {
      skip = true;
      continue;
    }
    if (skip && /^\[/.test(l)) skip = false;
    if (!skip) keep.push(l);
  }
  return [...systemSettingsLines(graphics, exposure), "", ...xmppLines(host, xmppPort), "", ...keep];
}

export function rewriteGameUserSettingsText(lines: string[], graphics: GraphicsPreset): string[] {
  if (graphics < 0) return lines;
  return lines.map((l) => {
    for (const k of SCALABILITY_GROUPS) {
      if (new RegExp(`^sg\\.${k}Quality=`, "i").test(l)) return `sg.${k}Quality=${graphics}`;
    }
    return l;
  });
}

async function readLines(file: string): Promise<string[] | null> {
  try {
    return splitLines(decodeIni(await fsp.readFile(file)));
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw e;
  }
}

async function writeAtomically(file: string, data: Buffer): Promise<void> {
  const tmp = `${file}.dr-tmp`;
  await fsp.writeFile(tmp, data);
  await fsp.rename(tmp, file);
}

export interface ApplyConfigOptions {
  host: string; // where chat/presence (XMPP) connects
  xmppPort?: number; // default 61099 (private mode); the relay port in public mode
  graphics: GraphicsPreset;
  exposure?: ExposureMode; // default "game": no exposure line
  configDir?: string;
  safeWindow?: boolean;
}

export async function applyGameConfig(opts: ApplyConfigOptions): Promise<{ engineIni: string; displayRepaired: boolean }> {
  const dir = opts.configDir ?? defaultConfigDir();
  await fsp.mkdir(dir, { recursive: true });
  const engine = path.join(dir, "Engine.ini");
  const existing = (await readLines(engine)) ?? [];
  await writeAtomically(engine, encodeIni(rewriteEngineIniText(existing, opts.host, opts.graphics, opts.xmppPort ?? XMPP_PORT, opts.exposure ?? "game")));
  const gus = path.join(dir, "GameUserSettings.ini");
  const lines = await readLines(gus);
  let displayRepaired = false;
  if (lines !== null || opts.safeWindow) {
    const display = repairDisplaySettings(lines ?? [], opts.safeWindow);
    displayRepaired = display.repaired;
    if (displayRepaired && lines !== null) await fsp.copyFile(gus, `${gus}.before-display-repair-${Date.now()}`);
    const next = opts.graphics >= 0 ? rewriteGameUserSettingsText(display.lines, opts.graphics) : display.lines;
    if (displayRepaired || opts.graphics >= 0) await writeAtomically(gus, encodeIni(next));
  }
  return { engineIni: engine, displayRepaired };
}
