// test/cases: the settings and log tables the Rust platform layer runs too (src-tauri/src/platform,
// MIGRATION_PLAN.md L28 to L30).

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { sanitizeSettings } from "../src/main/settings";
import { addSecret, redact, removeSecret } from "../src/main/log";
import { decodeIni, encodeIni, rewriteEngineIniText, rewriteGameUserSettingsText, splitLines, systemSettingsLines } from "../src/main/engineini";
import { repairDisplaySettings } from "../src/main/display-repair";
import { cleanGameFolderInput, gameRootCandidates } from "../src/main/game-folder";
import { addMissingModLines, disableModLines, safeRelative } from "../src/main/client-mods";

const table = (name: string) => JSON.parse(readFileSync(path.join(__dirname, "../../test/cases", name), "utf8"));

test("the shared settings table, as the file is written", () => {
  const cases = table("settings.json") as { platform: "win32" | "linux"; input: unknown; language: "en"; text: string }[];
  for (const { platform, input, language, text } of cases) {
    assert.equal(JSON.stringify(sanitizeSettings(input, language, platform), null, 2), text, `${platform} ${JSON.stringify(input)?.slice(0, 120)}`);
  }
});

test("the shared redaction table", () => {
  const { secrets, removed, cases } = table("log.json") as { secrets: string[]; removed: string; cases: { line: string; redacted: string }[] };
  for (const secret of secrets) addSecret(secret);
  const last = cases[cases.length - 1];
  for (const { line, redacted } of cases.slice(0, -1)) assert.equal(redact(line), redacted, line);
  removeSecret(removed);
  assert.equal(redact(last.line), last.redacted);
  for (const secret of secrets) removeSecret(secret);
});

test("the shared game config table (Engine.ini, display repair, game folder, mods.txt)", () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const c = table("game-config.json") as any;
  for (const x of c.systemSettings) assert.deepEqual(systemSettingsLines(x.graphics, x.exposure), x.lines);
  for (const x of c.engine) assert.deepEqual(rewriteEngineIniText(x.existing, x.host, x.graphics, x.port, x.exposure), x.out);
  for (const x of c.gus) assert.deepEqual(rewriteGameUserSettingsText(x.lines, x.graphics), x.out);
  for (const x of c.split) assert.deepEqual(splitLines(x.text), x.lines);
  for (const x of c.encode) assert.deepEqual(Array.from(encodeIni(x.lines)), x.bytes);
  for (const x of c.decode) assert.equal(decodeIni(Buffer.from(x.bytes)), x.text);
  for (const x of c.display) assert.deepEqual(repairDisplaySettings(x.lines, x.force), x.out);
  for (const x of c.folders) {
    const clean = cleanGameFolderInput(x.input, x.platform);
    assert.equal(clean, x.clean);
    if (clean !== null) assert.deepEqual(gameRootCandidates(clean, x.platform), x.candidates);
  }
  for (const x of c.disable) assert.deepEqual(disableModLines(x.text, x.names), x.out);
  for (const x of c.add) assert.equal(addMissingModLines(x.text, x.defaults), x.out);
  for (const x of c.safe) {
    let ok = true;
    try {
      safeRelative(x.rel);
    } catch {
      ok = false;
    }
    assert.equal(ok, x.ok, x.rel);
  }
});
