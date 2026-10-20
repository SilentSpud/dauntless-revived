import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";

import { offlineLayout } from "../src/main/offline-layout";

test("offline layout keeps packaged runtime separate from player state", () => {
    const resourcesPath = path.resolve("test-resources");
    const userDataDir = path.resolve("test-user-data");

    const layout = offlineLayout(resourcesPath, userDataDir);

    const runtimeRoot = path.join(resourcesPath, "offline-runtime");
    const stateRoot = path.join(userDataDir, "offline");
    const runtimeStateDir = path.join(stateRoot, "runtime");

    assert.deepEqual(layout, {
        runtimeRoot,
        runnerPath: path.join(runtimeRoot, "local-service-runner.cjs"),
        metagameDir: path.join(runtimeRoot, "metagame"),
        deployDir: path.join(runtimeRoot, "deploy"),

        stateRoot,
        databasePath: path.join(stateRoot, "undaunted.db"),
        runtimeStateDir,
        metagameEnvPath: path.join(runtimeStateDir, "metagame.env"),
        deployEnvPath: path.join(runtimeStateDir, "deploy.env"),
        readyDir: path.join(stateRoot, "ready"),
    });
});

test("offline database never lives inside the packaged runtime", () => {
    const layout = offlineLayout(
        path.resolve("test-resources"),
        path.resolve("test-user-data"),
    );

    const relative = path.relative(layout.runtimeRoot, layout.databasePath);

    assert.ok(
        relative.startsWith("..") || path.isAbsolute(relative),
        "offline database must live outside the packaged runtime",
    );
});