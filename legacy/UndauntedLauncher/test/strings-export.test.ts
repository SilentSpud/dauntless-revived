// The launcher's text, exported for the Tauri backend (MIGRATION_PLAN.md L14): src-tauri/build.rs makes
// the Rust Language enum and the backend's own text (dialogs) from src-tauri/gen/strings.json. This test
// fails when the export is stale; DR_UPDATE_GENERATED=1 npm test -- strings-export rewrites it.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { STRINGS } from "../src/shared/i18n";

const file = path.join(__dirname, "../../src-tauri/gen/strings.json");

test("src-tauri/gen/strings.json holds every language's text", () => {
  const exported = JSON.stringify(STRINGS, null, 2) + "\n";
  if (process.env.DR_UPDATE_GENERATED === "1") writeFileSync(file, exported);
  let current = "";
  try {
    current = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
  } catch {
    // missing: stale
  }
  assert.ok(current === exported, "stale: run DR_UPDATE_GENERATED=1 npm test -- strings-export");
});
