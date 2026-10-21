// The types generated from the Rust backend (src/shared/generated/types.ts, MIGRATION_PLAN.md L6) and
// the hand-written ones the page uses today must stay the same. Checked when this file compiles: a
// field that differs in name, type or optionality on either side is a type error here.

import { test } from "node:test";
import assert from "node:assert/strict";
import type * as Generated from "../src/shared/generated/types";
import type * as Status from "../src/shared/status";
import type * as Shared from "../src/shared/types";

// True only when each type is assignable to the other
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

const checks: Record<string, true> = {
  Snapshot: true satisfies Same<Generated.Snapshot, Shared.Snapshot>,
  Settings: true satisfies Same<Generated.Settings, Shared.Settings>,
  ActionResult: true satisfies Same<Generated.ActionResult, Shared.ActionResult>,
  InviteCheck: true satisfies Same<Generated.InviteCheck, Shared.InviteCheck>,
  RegisterOutcome: true satisfies Same<Generated.RegisterOutcome, Shared.RegisterOutcome>,
  OfflineStatus: true satisfies Same<Generated.OfflineStatus, Shared.OfflineStatus>,
  TaskProgress: true satisfies Same<Generated.TaskProgress, Shared.TaskProgress>,
  NewsItem: true satisfies Same<Generated.NewsItem, Shared.NewsItem>,
  Branding: true satisfies Same<Generated.Branding, Shared.Branding>,
  ExternalTarget: true satisfies Same<Generated.ExternalTarget, Shared.ExternalTarget>,
  ErrorCode: true satisfies Same<Generated.ErrorCode, Shared.ErrorCode>,
  Language: true satisfies Same<Generated.Language, Shared.Language>,
  GameLanguage: true satisfies Same<Generated.GameLanguage, Shared.GameLanguage>,
  GraphicsPreset: true satisfies Same<Generated.GraphicsPreset, Shared.GraphicsPreset>,
  ServerStatus: true satisfies Same<Generated.ServerStatus, Status.ServerStatus>,
};

test("the generated types match the hand-written ones", () => {
  assert.equal(Object.keys(checks).length, 15);
});
