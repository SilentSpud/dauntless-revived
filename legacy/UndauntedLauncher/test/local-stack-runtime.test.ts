import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import Module from "node:module";
import * as http from "../src/main/http";
import type { ElectronLocalStackRuntimeOptions } from "../src/main/local-stack-runtime";

// Electron's utilityProcess cannot run in node:test. Only the fork boundary is
// replaced; timeout selection and readiness polling run through the real runtime.
const loader = Module as unknown as { _load: (name: string, ...args: unknown[]) => unknown };
const originalLoad = loader._load;
loader._load = function (name, ...args) {
    if (name === "electron") return { utilityProcess: { fork() {
        const child = Object.assign(new EventEmitter(), {
            pid: 123, postMessage() { queueMicrotask(() => child.emit("exit", 0)); }, kill() {},
        });
        queueMicrotask(() => child.emit("spawn"));
        return child;
    } } };
    return originalLoad.call(this, name, ...args);
};
let Runtime: typeof import("../src/main/local-stack-runtime").ElectronLocalStackRuntime;
try { Runtime = require("../src/main/local-stack-runtime").ElectronLocalStackRuntime; }
finally { loader._load = originalLoad; }

const service = { serverPath: "server.js", envPath: "service.env", cwd: ".", port: 61000, healthPath: "/health" };
const options: ElectronLocalStackRuntimeOptions = { runnerPath: "runner.cjs", services: { metagame: service, deploy: service } };

test("readiness selects service override, general fallback, then the historical default", async (t) => {
    for (const [override, fallback, expected] of [[90_000, 20_000, 90_000], [undefined, 700, 700], [undefined, undefined, 20_000]]) {
        let now = 0;
        t.mock.method(Date, "now", () => { const result = now; now += 100_000; return result; });
        const runtime = new Runtime({ ...options, readyTimeoutMs: fallback,
            services: { ...options.services, metagame: { ...service, readyTimeoutMs: override } } });
        await assert.rejects(runtime.waitUntilReady("metagame"), new RegExp(`within ${expected} ms`));
        t.mock.restoreAll();
    }
});

test("a ready service continues immediately instead of waiting for its maximum", async (t) => {
    t.mock.method(Date, "now", () => 0);
    t.mock.method(http, "request", async () => ({ status: 200 }));
    const runtime = new Runtime({ ...options,
        services: { ...options.services, metagame: { ...service, readyTimeoutMs: 90_000 } } });
    const handle = await runtime.startService("metagame");
    try { await runtime.waitUntilReady("metagame"); }
    finally { await handle.stop(); }
});
