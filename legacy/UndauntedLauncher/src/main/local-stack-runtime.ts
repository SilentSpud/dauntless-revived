import { utilityProcess } from "electron";
import { setTimeout as delay } from "node:timers/promises";

import { request } from "./http";
import { log } from "./log";
import type {
    LocalServiceHandle,
    LocalServiceName,
    LocalStackRuntime,
} from "./local-stack";

type UtilityChild = ReturnType<typeof utilityProcess.fork>;

export interface LocalServiceSpec {
    serverPath: string;
    envPath: string;
    cwd: string;
    port: number;
    healthPath: string;
    readyTimeoutMs?: number;
}

export interface ElectronLocalStackRuntimeOptions {
    runnerPath: string;
    services: Record<LocalServiceName, LocalServiceSpec>;
    readyTimeoutMs?: number;
    env?: NodeJS.ProcessEnv;
    beforeStart?(name: LocalServiceName): Promise<void>;
}

// Electron implementation of LocalStackRuntime.
//
// LocalStackManager decides WHAT starts and in which order. This class contains the
// Electron-specific HOW: utility processes and loopback HTTP readiness checks.
export class ElectronLocalStackRuntime implements LocalStackRuntime {
    private readonly children = new Map<LocalServiceName, UtilityChild>();
    private readonly readyTimeoutMs: number;

    constructor(private readonly options: ElectronLocalStackRuntimeOptions) {
        this.readyTimeoutMs = options.readyTimeoutMs ?? 20_000;
    }

    async startService(name: LocalServiceName): Promise<LocalServiceHandle> {
        if (this.children.has(name)) {
            throw new Error(`${name} is already running`);
        }

        const spec = this.options.services[name];

        await this.options.beforeStart?.(name);

        const child = utilityProcess.fork(
            this.options.runnerPath,
            [spec.serverPath, spec.envPath],
            {
                cwd: spec.cwd,
                stdio: "inherit",
                serviceName: `Dauntless Revived local ${name}`,
                env: this.options.env,
            },
        );

        this.children.set(name, child);

        child.on("exit", (code) => {
            if (this.children.get(name) === child) {
                this.children.delete(name);
            }

            log.info(`local ${name} exited with code ${code ?? "unknown"}`);
        });

        // fork() returning only means Electron created the UtilityProcess object.
        // Resolve startService only after Electron confirms that the child actually spawned.
        await new Promise<void>((resolve, reject) => {
            const onExitBeforeSpawn = (code: number) => {
                child.removeListener("spawn", onSpawn);
                reject(
                    new Error(
                        `${name} exited before startup completed (code ${code})`,
                    ),
                );
            };

            const onSpawn = () => {
                child.removeListener("exit", onExitBeforeSpawn);
                log.info(`local ${name} spawned (pid ${child.pid ?? "unknown"})`);
                resolve();
            };

            child.once("exit", onExitBeforeSpawn);
            child.once("spawn", onSpawn);
        });

        return {
            stop: async () => {
                // The handle controls exactly the process created above. We never search for
                // arbitrary Node or Dauntless processes by name.
                if (this.children.get(name) !== child) {
                    return;
                }

                await stopUtilityProcess(child);

                if (this.children.get(name) === child) {
                    this.children.delete(name);
                }
            },
        };
    }

    async waitUntilReady(name: LocalServiceName): Promise<void> {
        const spec = this.options.services[name];
        const readyTimeoutMs = spec.readyTimeoutMs ?? this.readyTimeoutMs;
        const deadline = Date.now() + readyTimeoutMs;

        while (Date.now() < deadline) {
            // An exit listener removes the child from the map, so fail immediately instead
            // of wasting the whole readiness timeout when startup has already crashed.
            if (!this.children.has(name)) {
                throw new Error(`${name} exited before becoming ready`);
            }

            try {
                const response = await request(
                    {
                        host: "127.0.0.1",
                        port: spec.port,
                        pin: null,
                    },
                    spec.healthPath,
                    {
                        timeoutMs: 1_000,
                        maxBytes: 64 * 1024,
                    },
                );

                if (response.status === 200) {
                    log.info(`local ${name} is ready on 127.0.0.1:${spec.port}`);
                    return;
                }
            } catch {
                // Connection refused/timeouts are expected while the service is still booting.
            }

            await delay(200);
        }

        throw new Error(
            `${name} did not become ready within ${readyTimeoutMs} ms`,
        );
    }
}

// utilityProcess.kill() is synchronous, but process termination is not.
// Waiting for the exit event prevents us from starting another stack while the old
// process may still own its TCP port.
function stopUtilityProcess(child: UtilityChild): Promise<void> {
    return new Promise((resolve) => {
        let finished = false;
        let fallbackTimer: NodeJS.Timeout | undefined;
        let giveUpTimer: NodeJS.Timeout | undefined;

        const finish = () => {
            if (finished) {
                return;
            }

            finished = true;

            if (fallbackTimer !== undefined) {
                clearTimeout(fallbackTimer);
            }

            if (giveUpTimer !== undefined) {
                clearTimeout(giveUpTimer);
            }

            child.removeListener("exit", finish);
            resolve();
        };

        child.once("exit", finish);

        if (child.pid === undefined) {
            finish();
            return;
        }

        try {
            child.postMessage({ type: "shutdown" });
        } catch {
            child.kill();
        }

        fallbackTimer = setTimeout(() => {
            if (child.pid !== undefined) {
                child.kill();
            }
        }, 7_000);

        fallbackTimer.unref();

        giveUpTimer = setTimeout(finish, 10_000);
        giveUpTimer.unref();
    });
}
