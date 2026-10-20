import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import vm from "node:vm";
import ts from "typescript";
import { IPC } from "../src/shared/types";
import { SettingsStore } from "../src/main/settings";
import { mkdtempSync, rmSync } from "node:fs";
import os from "node:os";

test("the actual preload bridge forwards US and EU settings across IPC", async () => {
  const root = path.resolve(__dirname, "../..");
  const source = readFileSync(path.join(root, "src/preload.ts"), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
  let api: { setSettings: (v: unknown) => Promise<unknown> } | undefined;
  const calls: unknown[][] = [];
  vm.runInNewContext(js, { exports: {}, require: (id: string) => {
    if (id === "./shared/types") return { IPC };
    if (id === "electron") return {
      contextBridge: { exposeInMainWorld: (_name: string, value: typeof api) => { api = value; } },
      ipcRenderer: { invoke: async (...args: unknown[]) => { calls.push(args); return args[1]; } },
    };
    throw new Error(`Unexpected preload dependency ${id}`);
  }});
  const dir = mkdtempSync(path.join(os.tmpdir(), "dr-region-"));
  try {
    const store = new SettingsStore(dir, "en");
    for (const region of ["auto", "us", "main", "ger"] as const) {
      const patch = await api!.setSettings({ huntRegion: region, unexpected: "discard" });
      assert.equal(JSON.stringify(calls.at(-1)), JSON.stringify([IPC.setSettings, { huntRegion: region }]));
      await store.update(s => Object.assign(s, patch));
      assert.equal(new SettingsStore(dir, "en").get().huntRegion, region);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
