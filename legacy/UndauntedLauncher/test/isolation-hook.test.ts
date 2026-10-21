// The isolation hook (isolation/hook.js, MIGRATION_PLAN.md L9): only the backend's commands, with
// arguments of the right shape and size, reach Rust; anything else never settles.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import vm from "node:vm";

type Spec = Record<string, { kind: string; max?: number }>;
type Hook = (payload: unknown) => unknown;

const dir = path.resolve(__dirname, "../../isolation");

function load(): { hook: Hook; commands: Record<string, Spec> } {
  const window: Record<string, unknown> = {};
  const context = vm.createContext({ window });
  vm.runInContext(readFileSync(path.join(dir, "commands.js"), "utf8"), context);
  vm.runInContext(readFileSync(path.join(dir, "hook.js"), "utf8"), context);
  return { hook: window.__TAURI_ISOLATION_HOOK__ as Hook, commands: window.__DR_COMMANDS__ as Record<string, Spec> };
}

const { hook, commands } = load();

function payload(cmd: unknown, args: unknown) {
  return { cmd, callback: 1, error: 2, payload: args, options: undefined };
}

function passes(cmd: unknown, args: unknown): boolean {
  const p = payload(cmd, args);
  return hook(p) === p;
}

function goodArgs(spec: Spec): Record<string, unknown> {
  const args: Record<string, unknown> = {};
  for (const [name, arg] of Object.entries(spec)) {
    if (arg.kind === "text") args[name] = "a".repeat(arg.max ?? 1);
    else if (arg.kind === "flag") args[name] = false;
    else if (arg.kind === "settingsPatch") args[name] = { graphics: 4, huntRegion: "us", windowed: true };
    else if (arg.kind === "channel") args[name] = "__CHANNEL__:123";
  }
  return args;
}

test("every listed command passes with arguments of the right shape", () => {
  assert.ok(Object.keys(commands).length > 30);
  for (const [cmd, spec] of Object.entries(commands)) {
    assert.ok(passes(cmd, goodArgs(spec)), cmd);
  }
  assert.ok(passes("start_offline", {}));
  assert.ok(passes("plugin:event|listen", { event: "snapshot", target: { kind: "Any" }, handler: 7 }));
  assert.ok(passes("plugin:window|start_dragging", undefined));
  assert.ok(passes("plugin:window|minimize", { label: "main" }));
});

test("a forged command name never reaches the backend", () => {
  for (const cmd of ["plugin:fs|read_file", "plugin:shell|execute", "plugin:opener|open_url", "get_snapshot ", "__proto__", "constructor", "toString", 7, null]) {
    const p = payload(cmd, {});
    const result = hook(p);
    // The hook's own pending promise (from the hook's realm, so not instanceof this realm's Promise)
    assert.notEqual(result, p, String(cmd));
    assert.equal(typeof (result as { then?: unknown }).then, "function", String(cmd));
  }
});

test("a 1 MB string or a wrong argument never reaches the backend", () => {
  const big = "x".repeat(1024 * 1024);
  assert.equal(passes("check_invite", { text: big }), false);
  assert.equal(passes("register", { username: "a".repeat(33) }), false);
  assert.equal(passes("register", { username: "a".repeat(32) }), true);
  assert.equal(passes("register", {}), false, "missing");
  assert.equal(passes("register", { username: 7 }), false, "not a string");
  assert.equal(passes("register", { username: "a", extra: 1 }), false, "unknown argument");
  assert.equal(passes("submit_invite", { text: "x", acceptNewCertificate: "yes" }), false);
  assert.equal(passes("set_settings", { patch: { unexpected: true } }), false);
  assert.equal(passes("set_settings", { patch: { language: big } }), false);
  assert.equal(passes("set_settings", { patch: [] }), false);
  assert.equal(passes("subscribe_progress", { onProgress: "https://example.com" }), false);
  assert.equal(passes("plugin:event|listen", { event: big, handler: 1 }), false);
  assert.equal(passes("get_snapshot", []), false);
});

test("a dropped call stays pending", async () => {
  const result = hook(payload("plugin:fs|read_file", {})) as Promise<unknown>;
  const settled = await Promise.race([result.then(() => true), new Promise((resolve) => setTimeout(() => resolve(false), 20))]);
  assert.equal(settled, false);
});
