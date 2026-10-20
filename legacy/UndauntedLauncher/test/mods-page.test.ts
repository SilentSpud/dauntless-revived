import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { STRINGS } from "../src/shared/i18n";
import { LANGUAGES } from "../src/shared/types";

const ROOT = path.resolve(__dirname, "..", "..");
const html = readFileSync(path.join(ROOT, "index.html"), "utf8");
const renderer = readFileSync(path.join(ROOT, "src", "renderer", "index.ts"), "utf8");
const css = readFileSync(path.join(ROOT, "src", "renderer", "styles.css"), "utf8");

test("the Mods page sits between Partners and Settings and credits ZFXSTATIC", () => {
  assert.match(html, /id="view-partners"[^>]*><\/section>\s*<section class="view" id="view-mods" aria-labelledby="mods-title" hidden><\/section>\s*<section class="view" id="view-settings"/);
  assert.match(renderer, /\{ view: "partners", icon: "people", key: "nav_partners" \},\s*\{ view: "mods", icon: "puzzle", key: "nav_mods" \},\s*\{ view: "settings"/);
  assert.match(renderer, /function renderMods\(\): void/);
  assert.match(renderer, /if \(state\.view === "mods"\) renderMods\(\);/);
  assert.match(css, /\.mods-author\s*\{/);
  assert.match(css, /\.mods-grid\s*\{/);
  for (const lang of LANGUAGES) {
    for (const k of ["mods_subtitle", "mods_badge", "mods_title", "mods_by", "mods_credit"] as const) {
      assert.match(STRINGS[lang][k], /ZFXSTATIC/, `${lang}.${k} must name ZFXSTATIC`);
    }
  }
});
