import { defineConfig, type Plugin } from "vite";

// The Electron page keeps its Content-Security-Policy in a meta tag. The Tauri page (mode "tauri") gets
// the same policy from tauri.conf.json instead: with the meta tag left in, every call to the backend
// hangs, because the tag's default-src 'none' and connect-src 'self' block the isolation frame and the
// IPC endpoint (MIGRATION_PLAN.md L10). The meta tag goes once the Electron launcher does (L51).
function withoutCspMeta(): Plugin {
  return {
    name: "dr-without-csp-meta",
    transformIndexHtml(html) {
      const out = html.replace(/\s*<meta\s+http-equiv="Content-Security-Policy"[^>]*\/>/, "");
      if (out === html) throw new Error("index.html: the Content-Security-Policy meta tag was not found");
      return out;
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
  plugins: mode === "tauri" ? [withoutCspMeta()] : [],
}));
