// Copies the launcher's icons and in-app brand images from the repository's brand/ folder. They are
// made there by brand/build.py from the emblem and logo masters (see brand/README.md); this script
// only copies them into the launcher, byte for byte:
//
//   assets/icon.ico                    the exe, the Squirrel installer (Setup.exe and setupIcon), the
//                                      shortcuts and the portable zip's exe (16 to 256 px)
//   assets/icon.png                    the window and taskbar icon (256 px)
//   src/renderer/brand/logo-inapp*.png the logo in the rail (1x and 2x)
//   src/renderer/brand/emblem-*.png    the title bar emblem (32 and 64 px for 1x and 2x) and the faded
//                                      emblem in the built-in background (512 px)
//
//   npm run icon               copy
//   npm run icon -- --check    only compare; exit code 1 if a copy is missing or differs
//
// To change the artwork, change the masters in brand/ and run python3 brand/build.py, then this.
import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const brand = path.resolve(root, "..", "..", "brand");

// Launcher path -> brand/ path.
export const BRAND_COPIES = {
  "assets/icon.ico": "launcher/icon.ico",
  "assets/icon.png": "launcher/icon-256.png",
  "src/renderer/brand/logo-inapp.png": "launcher/logo-inapp.png",
  "src/renderer/brand/logo-inapp@2x.png": "launcher/logo-inapp@2x.png",
  "src/renderer/brand/emblem-32.png": "web/emblem-32.png",
  "src/renderer/brand/emblem-64.png": "web/emblem-64.png",
  "src/renderer/brand/emblem-512.png": "web/emblem-512.png",
};

const check = process.argv.includes("--check");
let stale = 0;
for (const [dest, src] of Object.entries(BRAND_COPIES)) {
  const from = path.join(brand, src);
  const to = path.join(root, dest);
  if (!existsSync(from)) {
    console.error(`brand/${src} is missing; run python3 brand/build.py`);
    process.exit(1);
  }
  const same = existsSync(to) && readFileSync(to).equals(readFileSync(from));
  if (check) {
    if (!same) {
      console.error(`${dest} is missing or not a copy of brand/${src}; run npm run icon`);
      stale++;
    }
    continue;
  }
  if (!same) {
    mkdirSync(path.dirname(to), { recursive: true });
    copyFileSync(from, to);
  }
  console.log(`${same ? "current" : "copied "} ${dest} <- brand/${src}`);
}
if (check) {
  if (stale > 0) process.exit(1);
  console.log(`${Object.keys(BRAND_COPIES).length} brand copies are current`);
}
