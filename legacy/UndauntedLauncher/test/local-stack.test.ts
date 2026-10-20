import { test } from "node:test";
import assert from "node:assert/strict";

import {
    LocalStackManager,
    type LocalServiceName,
    type LocalStackRuntime,
    type LocalStackState,
} from "../src/main/local-stack";

function fakeRuntime(
    events: string[],
    failReady?: LocalServiceName,
): LocalStackRuntime {
    return {
        async startService(name) {
            events.push(`start:${name}`);

            return {
                async stop() {
                    events.push(`stop:${name}`);
                },
            };
        },

        async waitUntilReady(name) {
            events.push(`ready:${name}`);

            if (name === failReady) {
                throw new Error(`${name} failed`);
            }
        },
    };
}

test("local stack starts services in dependency order and stops them in reverse order", async () => {
    const events: string[] = [];
    const states: LocalStackState[] = [];

    const manager = new LocalStackManager(fakeRuntime(events));

    manager.onChange((snapshot) => {
        states.push(snapshot.state);
    });

    await manager.start();

    assert.deepEqual(events, [
        "start:metagame",
        "ready:metagame",
        "start:deploy",
        "ready:deploy",
    ]);

    assert.deepEqual(states, [
        "starting-metagame",
        "starting-deploy",
        "ready",
    ]);

    assert.deepEqual(manager.snapshot, {
        state: "ready",
        error: null,
    });

    await manager.stop();

    assert.deepEqual(events, [
        "start:metagame",
        "ready:metagame",
        "start:deploy",
        "ready:deploy",
        "stop:deploy",
        "stop:metagame",
    ]);

    assert.equal(manager.snapshot.state, "stopped");
});

test("local stack cleans up both services when deploy fails to become ready", async () => {
    const events: string[] = [];

    const manager = new LocalStackManager(
        fakeRuntime(events, "deploy"),
    );

    await assert.rejects(
        manager.start(),
        /deploy failed/,
    );

    assert.deepEqual(events, [
        "start:metagame",
        "ready:metagame",
        "start:deploy",
        "ready:deploy",
        "stop:deploy",
        "stop:metagame",
    ]);

    assert.deepEqual(manager.snapshot, {
        state: "failed",
        error: "deploy failed",
    });
});