import { defineConfig } from "vite";
import path from "node:path";

// The game manifest is compiled into the main process (src/main/game-manifest.ts): the list of
// files, sizes and hashes is part of the launcher's public source and cannot come from a server.
export default defineConfig({
  resolve: {
    alias: {
      "@game-manifest": path.resolve(__dirname, "../UndauntedContent/data/dauntless-1.4.4.json"),
    },
  },
});
