// Loaded before index.ts in the Tauri build only (vite.renderer.config.ts, mode "tauri"): gives the page
// the same window.launcher the Electron preload gives it, until index.ts imports api.ts itself (L40).

import { api } from "./api";

window.launcher = api;

// No browser context menu, as in the Electron launcher (debug builds still open devtools with F12)
document.addEventListener("contextmenu", (event) => event.preventDefault());
