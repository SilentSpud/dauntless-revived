// The Tauri bridge (src/renderer/api.ts, MIGRATION_PLAN.md L7) against Tauri's own IPC mocks: every
// LauncherApi method reaches the command of the same name with the same arguments the preload sends.

import { test, mock } from "node:test";
import assert from "node:assert/strict";
import type { InvokeArgs } from "@tauri-apps/api/core";

// The Tauri API reads window.__TAURI_INTERNALS__, which the mocks install on window
(globalThis as unknown as { window: unknown }).window = globalThis;

import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { api, CALL_TIMEOUT_MS } from "../src/renderer/api";

type Call = { cmd: string; args: InvokeArgs | undefined };

function record(answer: (cmd: string, args: InvokeArgs | undefined) => unknown = () => ({ ok: true })): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return answer(cmd, args);
  });
  return calls;
}

test("each call reaches its command with the preload's arguments", async () => {
  const calls = record();
  try {
    await api.checkInvite("dauntless-revived://join?v=1");
    await api.submitInvite("x", true);
    await api.submitInvite("x");
    await api.switchServer("0123456789abcdef01234567");
    await api.register("Slayer");
    await api.useExistingKey("UUK_x");
    await api.useExistingGamePath("C:\\Games");
    await api.startOffline();
    await api.startOffline("Slayer");
    await api.setStatusPolling(true);
    await api.openExternal("discord");
    assert.deepEqual(
      calls.map((c) => [c.cmd, c.args]),
      [
        ["check_invite", { text: "dauntless-revived://join?v=1" }],
        ["submit_invite", { text: "x", acceptNewCertificate: true }],
        ["submit_invite", { text: "x", acceptNewCertificate: false }],
        ["switch_server", { id: "0123456789abcdef01234567" }],
        ["register", { username: "Slayer" }],
        ["use_existing_key", { text: "UUK_x" }],
        ["use_existing_game_path", { gamePath: "C:\\Games" }],
        ["start_offline", {}],
        ["start_offline", { username: "Slayer" }],
        ["set_status_polling", { on: true }],
        ["open_external", { target: "discord" }],
      ],
    );
  } finally {
    clearMocks();
  }
});

test("settings go across without unknown fields, as in preload.ts", async () => {
  const calls = record((_cmd, args) => (args as { patch: unknown }).patch);
  try {
    for (const region of ["auto", "us", "main", "ger"] as const) {
      await api.setSettings({ huntRegion: region, unexpected: "discard" } as never);
      assert.deepEqual(calls.at(-1), { cmd: "set_settings", args: { patch: { huntRegion: region } } });
    }
  } finally {
    clearMocks();
  }
});

test("a failed action answers like the Electron main process did", async () => {
  record(() => {
    throw new Error("refused");
  });
  const errors = mock.method(console, "error", () => undefined);
  try {
    assert.deepEqual(await api.play(), { ok: false, error: { code: "unknown" } });
    await api.pauseTask();
    assert.equal(errors.mock.callCount(), 2);
    await assert.rejects(api.getSnapshot(), /get_snapshot: Error: refused/);
  } finally {
    errors.mock.restore();
    clearMocks();
  }
});

test("a call the isolation hook dropped times out instead of hanging", async () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  mockIPC(() => new Promise(() => undefined));
  const errors = mock.method(console, "error", () => undefined);
  try {
    const snapshot = api.getSnapshot();
    const play = api.play();
    mock.timers.tick(CALL_TIMEOUT_MS);
    await assert.rejects(snapshot, /get_snapshot: no answer/);
    assert.deepEqual(await play, { ok: false, error: { code: "unknown" } });
  } finally {
    errors.mock.restore();
    mock.timers.reset();
    clearMocks();
  }
});

test("pushes arrive as events and progress over a channel", async () => {
  const calls = record(() => null);
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      return null;
    },
    { shouldMockEvents: true },
  );
  try {
    const progress: unknown[] = [];
    api.onProgress((p) => progress.push(p));
    await new Promise((resolve) => setImmediate(resolve));
    const subscribe = calls.find((c) => c.cmd === "subscribe_progress");
    assert.ok(subscribe, "subscribed");
    const channel = (subscribe.args as { onProgress: { id: number } }).onProgress;
    const internals = (globalThis as unknown as { __TAURI_INTERNALS__: { runCallback(id: number, data: unknown): void } }).__TAURI_INTERNALS__;
    internals.runCallback(channel.id, { index: 0, message: { kind: "download" } });
    assert.deepEqual(progress, [{ kind: "download" }]);

    const states: boolean[] = [];
    api.onWindowState((maximized) => states.push(maximized));
    await new Promise((resolve) => setImmediate(resolve));
    const { emit } = await import("@tauri-apps/api/event");
    await emit("window-state", true);
    await emit("window-state", "yes");
    assert.deepEqual(states, [true, false]);
  } finally {
    clearMocks();
  }
});

test("the window controls use Tauri's window calls", async () => {
  mockWindows("main");
  let maximized = false;
  const calls = record((cmd) => {
    if (cmd === "plugin:window|toggle_maximize") maximized = !maximized;
    if (cmd === "plugin:window|is_maximized") return maximized;
    return null;
  });
  try {
    await api.minimize();
    assert.equal(await api.toggleMaximize(), true);
    assert.equal(await api.toggleMaximize(), false);
    await api.close();
    assert.deepEqual(
      calls.map((c) => c.cmd),
      ["plugin:window|minimize", "plugin:window|toggle_maximize", "plugin:window|is_maximized", "plugin:window|toggle_maximize", "plugin:window|is_maximized", "plugin:window|close"],
    );
  } finally {
    clearMocks();
  }
});
