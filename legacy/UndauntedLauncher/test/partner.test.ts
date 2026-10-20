import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";

const ROOT = path.resolve(__dirname, "..", "..");
const html = readFileSync(path.join(ROOT, "index.html"), "utf8");
const renderer = readFileSync(path.join(ROOT, "src", "renderer", "index.ts"), "utf8");
const css = readFileSync(path.join(ROOT, "src", "renderer", "styles.css"), "utf8");

test("EU Gamehost partner promotion is present across the launcher", () => {
  assert.match(html, /id="eugamehost-btn"[^>]*data-i18n-aria="partner_banner_label"/);
  assert.match(html, /id="view-partners"[^>]*aria-labelledby="partners-title"/);
  assert.match(renderer, /\{ view: "partners", icon: "people", key: "nav_partners" \}/);
  assert.match(renderer, /function partnerBanner\(\): HTMLElement/);
  assert.match(renderer, /function renderPartners\(\): void/);
  assert.match(renderer, /VDS GAME 1/);
  assert.match(renderer, /eugamehost_game1/);
  assert.match(renderer, /VDS GAME 3/);
  assert.match(renderer, /partnerBanner\(\)\)\];/);
  assert.match(css, /\.partner-banner\s*\{/);
  assert.match(css, /\.partner-plan\.recommended/);
  assert.match(css, /\.partners-page \.card-title\s*\{[\s\S]*display:\s*flex;[\s\S]*align-items:\s*center;/);
  assert.match(css, /\.partners-page \.card-title \.icon\s*\{[\s\S]*width:\s*16px;[\s\S]*height:\s*16px;/);
});

test("Discord and Patreon rail buttons use bundled local brand marks", () => {
  assert.match(html, /class="social-button" id="discord-btn"/);
  assert.match(html, /class="social-icon social-icon-discord"/);
  assert.match(html, /class="social-button" id="patreon-btn"/);
  assert.match(html, /class="social-icon social-icon-patreon"/);
  for (const file of ["discord-mark.svg", "discord.svg", "patreon-mark.svg", "patreon.png"]) {
    const full = path.join(ROOT, "src", "renderer", "brand", file);
    assert.ok(existsSync(full), `${file} is missing`);
    assert.ok(statSync(full).size > 100, `${file} is unexpectedly small`);
  }
  assert.match(css, /url\("\.\/brand\/discord-mark\.svg"\)/);
  assert.match(css, /url\("\.\/brand\/patreon-mark\.svg"\)/);
  assert.doesNotMatch(html, /discord-dot/);
});
