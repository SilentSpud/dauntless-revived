// test/cases: the settings and log tables the Rust platform layer runs too (src-tauri/src/platform,
// MIGRATION_PLAN.md L28, L29).

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { sanitizeSettings } from "../src/main/settings";
import { addSecret, redact, removeSecret } from "../src/main/log";

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
