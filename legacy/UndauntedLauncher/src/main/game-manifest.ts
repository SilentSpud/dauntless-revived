// The compiled-in game manifest. At build time Vite resolves "@game-manifest" to
// UndauntedContent/data/dauntless-1.4.4.json (generated from the verified 1.4.4 zip), so the
// list of files, sizes and hashes is part of the launcher's public source and cannot be changed
// by a server.

import raw from "@game-manifest";
import { validateManifest, manifestFingerprint, type GameManifest } from "./manifest";

const check = validateManifest(raw);

export const GAME_MANIFEST: GameManifest | null = check.ok ? check.manifest : null;
export const GAME_MANIFEST_PROBLEM: string | null = check.ok ? null : check.reason;
export const GAME_MANIFEST_FINGERPRINT: string | null = check.ok ? manifestFingerprint(check.manifest) : null;
