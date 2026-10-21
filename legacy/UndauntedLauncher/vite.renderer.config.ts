import { defineConfig, type Plugin } from "vite";

// The Tauri page (mode "tauri") is today's page with two changes:
// - The Electron page keeps its Content-Security-Policy in a meta tag. The Tauri page gets the same
//   policy from tauri.conf.json instead: with the meta tag left in, every call to the backend hangs,
//   because the tag's default-src 'none' and connect-src 'self' block the isolation frame and the IPC
//   endpoint (MIGRATION_PLAN.md L10). The meta tag goes once the Electron launcher does (L51).
// - The Tauri bridge (src/renderer/tauri-bridge.ts) loads before the page's own script (L7).
function forTauri(): Plugin {
  return {
    name: "dr-tauri-page",
    // Before Vite bundles the scripts the page names
    transformIndexHtml: {
      order: "pre",
      handler(html) {
        const withoutMeta = html.replace(/\s*<meta\s+http-equiv="Content-Security-Policy"[^>]*\/>/, "");
        if (withoutMeta === html) throw new Error("index.html: the Content-Security-Policy meta tag was not found");
        const page = '<script type="module" src="/src/renderer/index.ts"></script>';
        if (!withoutMeta.includes(page)) throw new Error("index.html: the page script was not found");
        const bridge = '<script type="module" src="/src/renderer/tauri-bridge.ts"></script>';
        return withoutMeta.replace(page, `${bridge}\n    ${page}`);
      },
    },
  };
}

// The page is plain TypeScript and CSS (no framework). Assets are never inlined as data URLs,
// so the Content-Security-Policy can stay strict.
export default defineConfig(({ mode }) => ({
  build: {
    assetsInlineLimit: 0,
  },
  // Tauri's dev server address is fixed in tauri.conf.json (devUrl)
  server: { port: 5173, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  plugins: mode === "tauri" ? [forTauri()] : [],
}));
