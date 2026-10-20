// The Credits page and the GitHub button: everyone is credited, in both languages, the software the
// launcher ships is listed and its license texts ship too, and the page itself holds no URLs.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { PATRONS, PROJECT_PEOPLE, SOFTWARE, UPSTREAM_PEOPLE, localized, type CreditPerson } from "../src/shared/credits";
import { STRINGS, type StringKey } from "../src/shared/i18n";
import { LANGUAGES, type ExternalTarget } from "../src/shared/types";

const ROOT = path.resolve(__dirname, "..", "..");
// Line endings depend on the checkout (Windows runners and core.autocrlf give CRLF), so compare with LF.
const NOTICES = readFileSync(path.join(ROOT, "THIRD-PARTY-NOTICES.txt"), "utf8").replace(/\r\n/g, "\n");

function person(list: readonly CreditPerson[], github: string): CreditPerson {
  const p = list.find((x) => x.github === github);
  assert.ok(p, `${github} is missing from the credits`);
  return p;
}

test("the credits name every contributor, in order, with their role", () => {
  assert.equal(PROJECT_PEOPLE[0].github, "mixutin", "the maintainer comes first");
  assert.equal(person(PROJECT_PEOPLE, "mixutin").role, "maintainer");
  assert.equal(person(PROJECT_PEOPLE, "mixutin").name, "mixutin");
  assert.match(person(PROJECT_PEOPLE, "LupesHk").note.en, /Brazilian Portuguese/);
  assert.match(person(PROJECT_PEOPLE, "JusMH").note.en, /Offline play runtime/);
  assert.match(person(PROJECT_PEOPLE, "anxo0").note.en, /DualSense/);
  assert.equal(person(PROJECT_PEOPLE, "Vvoidddd").role, "contributor");
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /airship/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /console windows/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /\.gitignore/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /performance sampler/); // #6
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.fi, /suorituskykymittarin/);
  // #8
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /existing Dauntless 1\.4\.4 folder by pasting its path/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.fi, /valmista Dauntless 1\.4\.4 -kansiota liittämällä sen polun/);
  // #9
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /first in-game text chat server \(XMPP\) and tested it with a real 1\.4\.4 client/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.fi, /ensimmäisen pelinsisäisen tekstichat-palvelimen \(XMPP\) ja testasi sen oikealla 1\.4\.4-pelillä/);
  // #7: the exposure setting is an opt-in experiment, off by default; the note must say so.
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /opt-in Basic adaptive exposure setting for the airship \(off by default\)/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.fi, /valinnaisen mukautuvan perusvalotuksen asetuksen ilmalaivaa varten \(oletuksena pois päältä\)/);
  // The exposure change was taken back out in 0.1.1 (it darkened Ramsgate); the note must not
  // suggest the airship is fixed.
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.en, /reverted in launcher 0\.1\.1/);
  assert.match(person(PROJECT_PEOPLE, "Vvoidddd").note.fi, /peruttiin käynnistimen versiossa 0\.1\.1/);

  // Harmonic: the work ported from Harmonic's Dauntless 1.4.4 fork (Harmonicrain/Undaunted 895f7c7),
  // listed after Vvoidddd. The English note names no gender (nothing we have says which).
  const harmonic = person(PROJECT_PEOPLE, "Harmonicrain");
  assert.equal(harmonic.name, "Harmonic");
  assert.equal(harmonic.role, "contributor");
  assert.ok(
    PROJECT_PEOPLE.findIndex((p) => p.github === "Harmonicrain") > PROJECT_PEOPLE.findIndex((p) => p.github === "Vvoidddd"),
    "Harmonic comes after Vvoidddd",
  );
  assert.match(harmonic.note.en, /Dauntless 1\.4\.4 fork of Undaunted/);
  assert.doesNotMatch(harmonic.note.en, /\b(he|his|him|she|her|hers)\b/i);
  for (const part of ["Escalation season registry and save rules", "the free store", "Slayer Links", "restart of a dead Ramsgate", "friends' online status", "test cases"]) {
    assert.ok(harmonic.note.en.includes(part), `Harmonic en: ${part}`);
  }
  assert.match(harmonic.note.fi, /Undauntedin Dauntless 1\.4\.4 -haaraa/);
  for (const part of ["Escalationin kausiluettelo ja tallennussäännöt", "ilmainen kauppa", "Slayer Links", "kaatunut Ramsgate", "kavereiden paikalla olosta", "testitapaukset"]) {
    assert.ok(harmonic.note.fi.includes(part), `Harmonic fi: ${part}`);
  }

  assert.equal(UPSTREAM_PEOPLE[0].github, "SyST3MDeV", "Undaunted's creator comes first");
  const gwog = person(UPSTREAM_PEOPLE, "SyST3MDeV");
  assert.equal(gwog.role, "creator");
  assert.match(gwog.name, /^gwog\b/);
  for (const part of ["server-mode DLL", "deploy server", "metagame", "launcher"]) assert.ok(gwog.note.en.includes(part), part);
  assert.equal(person(UPSTREAM_PEOPLE, "EisigesEis").role, "contributor");

  const handles = [...PROJECT_PEOPLE, ...UPSTREAM_PEOPLE].map((p) => p.github);
  assert.equal(new Set(handles).size, handles.length, "nobody is listed twice");
});

test("patrons are listed separately with localized thanks", () => {
  assert.deepEqual(PATRONS.map((p) => p.name), ["nasagi"]);
  for (const p of PATRONS) {
    assert.ok(p.name.trim(), JSON.stringify(p));
    assert.match(p.note.en, /Patreon/);
    assert.match(p.note.fi, /Patreon/);
    assert.notEqual(p.note.fi, p.note.en, `${p.name}: the Finnish note is not translated`);
  }
});

test("every credit has text in every language", () => {
  for (const p of [...PROJECT_PEOPLE, ...UPSTREAM_PEOPLE]) {
    assert.ok(p.name.trim() && p.github.trim(), JSON.stringify(p));
    assert.match(p.github, /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/, `not a GitHub handle: ${p.github}`);
    for (const lang of LANGUAGES) assert.ok(p.note[lang].trim().length > 10, `${p.github} ${lang}`);
    assert.notEqual(p.note.fi, p.note.en, `${p.github}: the Finnish note is not translated`);
    assert.notEqual(p.note.es, p.note.en, `${p.github}: the Spanish note is not translated`);
  }
  for (const sw of SOFTWARE) {
    assert.ok(sw.name.trim(), JSON.stringify(sw));
    for (const lang of LANGUAGES) {
      assert.ok(localized(sw.author, lang).trim(), `${sw.name} author ${lang}`);
      assert.ok(sw.note[lang].trim(), `${sw.name} note ${lang}`);
    }
    assert.notEqual(sw.note.fi, sw.note.en, `${sw.name}: the Finnish note is not translated`);
    assert.notEqual(sw.note.es, sw.note.en, `${sw.name}: the Spanish note is not translated`);
    if (sw.license !== null) assert.match(sw.license, /^[A-Za-z0-9.-]+$/, `${sw.name}: not an SPDX identifier`);
  }
});

test("the open-source software list covers the server DLL's libraries, the framework and the GitHub mark", () => {
  const by = (name: string) => {
    const sw = SOFTWARE.find((x) => x.name === name);
    assert.ok(sw, `${name} is missing from the credits`);
    return sw;
  };
  assert.equal(by("MinHook").license, "BSD-2-Clause");
  assert.equal(by("MinHook").author, "Tsuda Kageyu");
  assert.equal(by("Hacker Disassembler Engine 64").license, "BSD-2-Clause");
  assert.match(localized(by("Dumper-7").author, "en"), /^Encryqed and contributors$/);
  assert.equal(by("Electron").license, "MIT");
  assert.equal(by("GitHub Octicons").license, "MIT");
  assert.equal(by("GitHub Octicons").author, "GitHub Inc.");
});

test("every package the launcher bundles is credited and its license text ships", () => {
  const lock = JSON.parse(readFileSync(path.join(ROOT, "package-lock.json"), "utf8")) as { packages: Record<string, { dev?: boolean; devOptional?: boolean; license?: string }> };
  const runtime = new Map<string, string>();
  for (const [where, pkg] of Object.entries(lock.packages)) {
    if (!where || pkg.dev || pkg.devOptional) continue;
    runtime.set(where.slice(where.lastIndexOf("node_modules/") + "node_modules/".length), pkg.license ?? "");
  }
  assert.ok(runtime.size >= 2, "no runtime packages found; the scan is broken");
  for (const [name, license] of runtime) {
    const sw = SOFTWARE.find((x) => x.name === name);
    assert.ok(sw, `${name} is bundled but not on the Credits page`);
    assert.equal(sw.license, license, `${name}: license differs from package-lock.json`);
    assert.ok(new RegExp(`^\\d+\\. ${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`, "m").test(NOTICES), `${name} is missing from THIRD-PARTY-NOTICES.txt`);
  }
});

test("THIRD-PARTY-NOTICES.txt ships with the launcher and has the Octicons, MinHook and Electron notices", () => {
  const forge = readFileSync(path.join(ROOT, "forge.config.ts"), "utf8");
  assert.match(forge, /const launcherResources\s*=\s*\[[^\]]*"THIRD-PARTY-NOTICES\.txt"/);
  assert.match(forge, /extraResource:\s*launcherResources/);
  assert.match(NOTICES, /^\d+\. GitHub Octicons$/m);
  assert.match(NOTICES, /https:\/\/github\.com\/primer\/octicons/);
  assert.match(NOTICES, /Copyright \(c\) \d{4} GitHub Inc\.\n\nPermission is hereby granted, free of charge/);
  assert.match(NOTICES, /Copyright \(C\) 2009-2017 Tsuda Kageyu\./);
  assert.match(NOTICES, /Hacker Disassembler Engine 64, Copyright \(c\) 2008-2009, Vyacheslav Patkov\./);
  assert.match(NOTICES, /^\d+\. Electron$/m);
  assert.match(NOTICES, /LICENSES\.chromium\.html/);
  // Everything the notices list is on the Credits page too.
  const sections = [...NOTICES.matchAll(/^-{78}\n\d+\. (.+)\n-{78}$/gm)].map((m) => m[1]);
  assert.ok(sections.length >= 5, "no sections found; the scan is broken");
  const names = SOFTWARE.map((sw) => sw.name);
  for (const s of sections) {
    if (s.startsWith("UndauntedInternalServer.dll")) continue; // the Undaunted section of the page
    assert.ok(names.includes(s), `${s} is in the notices but not on the Credits page`);
  }
});

test("the credits hold no links and no e-mail addresses", () => {
  const data = JSON.stringify([PATRONS, PROJECT_PEOPLE, UPSTREAM_PEOPLE, SOFTWARE]);
  assert.ok(!data.includes("://"), "the credits must not carry URLs: the page opens links by name");
  assert.ok(!/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/.test(data), "no e-mail addresses on the Credits page");
});

test("the page opens links by name only, and community/source controls are in the rail", () => {
  const TARGETS: Record<ExternalTarget, true> = {
    tailscale_download: true,
    tailscale_share: true,
    vc_redist: true,
    directx_runtime: true,
    server_source: true,
    project_source: true,
    project_license: true,
    project_contributors: true,
    upstream_source: true,
    upstream_contributors: true,
    eugamehost: true,
    eugamehost_game1: true,
    eugamehost_game2: true,
    eugamehost_game3: true,
    eugamehost_game5: true,
    eugamehost_5800x: true,
    discord: true,
    patreon: true,
  };
  const dir = path.join(ROOT, "src", "renderer");
  let opens = 0;
  for (const f of readdirSync(dir).filter((x) => x.endsWith(".ts"))) {
    const text = readFileSync(path.join(dir, f), "utf8");
    assert.ok(!/["'`](?:https?|javascript|file|data):/i.test(text.replace(/"http:\/\/www\.w3\.org\/2000\/svg"/g, "")), `${f} contains a URL`);
    for (const m of text.matchAll(/\bopen\(("[^"]*"|[^)]*)\)/g)) {
      if (!m[1].startsWith('"')) continue; // open(target) inside the helper itself
      opens++;
      assert.ok(Object.prototype.hasOwnProperty.call(TARGETS, m[1].slice(1, -1)), `${f}: open(${m[1]}) is not a link name`);
    }
    const calls = [...text.matchAll(/api\.openExternal\(([^)]*)\)/g)].map((m) => m[1]);
    for (const arg of calls) assert.equal(arg, "target", `${f}: openExternal must only get the typed link name`);
  }
  assert.ok(opens >= 10, `only ${opens} open("...") calls found; the scan is broken`);
  for (const target of ["project_source", "project_contributors", "upstream_source", "upstream_contributors"]) {
    assert.ok(readFileSync(path.join(dir, "index.ts"), "utf8").includes(`open("${target}")`), `nothing opens ${target}`);
  }

  // Both buttons sit in the rail, outside every page, so they show on every screen (also before an
  // invite is entered), and the GitHub button has an accessible name.
  const html = readFileSync(path.join(ROOT, "index.html"), "utf8");
  const rail = html.slice(html.indexOf('<nav class="rail"'), html.indexOf("</nav>"));
  assert.match(rail, /<button type="button" class="icon-btn" id="github-btn" data-i18n-aria="github_link"><\/button>/);
  assert.match(rail, /<button type="button" class="rail-link" id="credits-btn">/);
  assert.match(rail, /<button type="button" class="social-button" id="discord-btn">/);
  assert.match(rail, /<button type="button" class="social-button" id="patreon-btn">/);
  assert.match(html, /<section class="view" id="view-credits" aria-labelledby="credits-title" hidden><\/section>/);
});

test("Escape on the Credits page goes back to the page and button it was opened from", () => {
  const src = readFileSync(path.join(ROOT, "src", "renderer", "index.ts"), "utf8");
  // Opening Credits from another page remembers that page and the focused button.
  const setView = src.slice(src.indexOf("function setView("), src.indexOf("function leaveCredits("));
  assert.match(setView, /if \(v === "credits" && state\.view !== "credits"\) \{[^}]*state\.creditsReturn = \{ view: state\.view, focus: /);
  // Escape with no dialog open, on the Credits page, leaves it.
  const keydown = src.slice(src.indexOf('document.addEventListener("keydown"'));
  assert.match(keydown, /^document\.addEventListener\("keydown", \(e\) => \{\s*if \(!state\.modal\) \{\s*if \(e\.key === "Escape" && state\.view === "credits" && !e\.defaultPrevented\) \{\s*e\.preventDefault\(\);\s*leaveCredits\(\);/);
  // Leaving goes back to that page and puts the focus back, or on the rail's Credits button.
  const leave = src.slice(src.indexOf("function leaveCredits("), src.indexOf("function applyStaticI18n("));
  assert.match(leave, /setView\(back\.view\);/);
  assert.match(leave, /back\.focus\?\.isConnected/);
  assert.match(leave, /\(again \?\? \$\("#credits-btn"\)\)\.focus\(/);
});

test("the new text exists in English and Finnish", () => {
  const keys: StringKey[] = [
    "github_link",
    "nav_credits",
    "credits_title",
    "credits_intro",
    "credits_project_text",
    "credits_upstream_text",
    "credits_role_maintainer",
    "credits_role_creator",
    "credits_role_contributor",
    "credits_github_handle",
    "credits_all_contributors",
    "credits_upstream_source",
    "credits_upstream_contributors",
    "credits_software",
    "credits_software_text",
    "credits_no_license",
    "credits_license_title",
    "credits_license_text",
    "credits_phoenix",
  ];
  for (const k of keys) {
    for (const lang of LANGUAGES) assert.ok(STRINGS[lang][k]?.trim(), `${lang} ${k}`);
  }
  assert.equal(STRINGS.en.github_link, "Source code on GitHub");
  assert.equal(STRINGS.fi.github_link, "Lähdekoodi GitHubissa");
  assert.equal(STRINGS.en.nav_credits, "Credits");
  assert.equal(STRINGS.fi.nav_credits, "Tekijät");
  for (const lang of LANGUAGES) {
    assert.match(STRINGS[lang].credits_license_text, /AGPL-3\.0/);
    assert.match(STRINGS[lang].credits_phoenix, /Phoenix Labs/);
    assert.match(STRINGS[lang].credits_phoenix, /Epic Games/);
  }
});
