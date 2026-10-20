import { promises as fsp } from "node:fs";
import path from "node:path";
import { decodeIni, encodeIni, splitLines } from "./engineini";
import { XMPP_PORT } from "./constants";

export function mergeIniKeys(lines: string[], section: string, entries: Record<string, string>): string[] {
    const managed = new Set(Object.keys(entries).map((key) => key.toLowerCase()));
    const replacement = Object.entries(entries).map(([key, value]) => `${key}=${value}`);
    const output: string[] = [];
    const keep: string[] = [];
    let inSection = false;
    let insertion = -1;
    for (const line of lines) {
        const header = /^\s*\[([^\]]+)\]\s*(?:[;#].*)?$/.exec(line);
        if (header) {
            inSection = header[1].toLowerCase() === section.toLowerCase();
            if (!inSection) output.push(line);
            else if (insertion === -1) {
                output.push(line);
                insertion = output.length;
            }
        } else {
            const key = /^\s*([^=]+?)\s*=/.exec(line)?.[1].toLowerCase();
            if (!inSection) output.push(line);
            else if (!(key && managed.has(key))) keep.push(line);
        }
    }
    if (insertion === -1) output.push(`[${section}]`, ...replacement, ...keep);
    else output.splice(insertion, 0, ...replacement, ...keep);
    return output;
}

export function offlineEndpointEntries(endpoints: Record<string, string>): Record<string, string> {
    const entries: Record<string, string> = {};
    for (const [key, suffix] of Object.entries(endpoints)) {
        if (!/^[A-Za-z0-9_]+$/.test(key) || typeof suffix !== "string" ||
            (suffix !== "" && !suffix.startsWith("/")) || /["\r\n\0\\]/.test(suffix)) {
            throw new Error("Invalid offline endpoint table");
        }
        entries[key] = `"http://127.0.0.1:61000${suffix}"`;
    }
    if (!entries.AuthEndpoint || !entries.MatchmakingEndpoint) throw new Error("Incomplete offline endpoint table");
    return entries;
}

async function updateIni(file: string, section: string, entries: Record<string, string>): Promise<void> {
    let lines: string[] = [];
    try {
        lines = splitLines(decodeIni(await fsp.readFile(file)));
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    const temporary = `${file}.dr-tmp`;
    await fsp.writeFile(temporary, encodeIni(mergeIniKeys(lines, section, entries)));
    await fsp.rename(temporary, file);
}

export async function prepareOfflineGameConfig(configDir: string, endpointFile: string): Promise<void> {
    const endpoints = offlineEndpointEntries(JSON.parse(await fsp.readFile(endpointFile, "utf8")));
    await fsp.mkdir(configDir, { recursive: true });
    // Current DLLs hook Phoenix endpoints in both modes. Game.ini remains the
    // documented fallback for older DLLs; unrelated player configuration survives.
    await updateIni(path.join(configDir, "Game.ini"), "OnlineSubsystemPhoenix", endpoints);
    await updateIni(path.join(configDir, "Engine.ini"), "OnlineSubsystemMcp.XMPP", {
        ServerAddr: '"ws://127.0.0.1"', ServerPort: String(XMPP_PORT), bUseSSL: "false",
    });
}
