// The backend the page talks to (MIGRATION_PLAN.md L40). In the Tauri launcher it is api.ts, over Tauri's
// IPC; the Electron launcher's preload still gives the page `window.launcher` until the Electron build goes
// (L51). Tauri sets `__TAURI_INTERNALS__` before any page script runs, so the choice is made once, here.

import type { LauncherApi } from "../preload";
import { artSource } from "../shared/art";
import { api as tauriApi } from "./api";

const inTauri = "__TAURI_INTERNALS__" in window;

export const api: LauncherApi = inTauri ? tauriApi : (window as unknown as { launcher: LauncherApi }).launcher;

// The Tauri launcher on Windows draws with WebView2
const webView2 = inTauri && navigator.userAgent.includes("Windows");

/** Where this webview loads an art image the backend named, or null when the name is not art. */
export function artUrl(url: string): string | null {
  return artSource(url, webView2);
}

// No browser context menu, as in the Electron launcher (debug builds still open devtools with F12)
if (inTauri) document.addEventListener("contextmenu", (event) => event.preventDefault());
