// Owns the lifecycle of the local offline stack.
//
// The manager deliberately knows nothing about Electron, utilityProcess, paths or HTTP.
// It only coordinates service order and cleanup. LocalStackRuntime provides the
// platform-specific operations, which also keeps this state machine testable with plain Node.


export type LocalServiceName = "metagame" | "deploy";

export type LocalStackState =
    | "stopped"
    | "starting-metagame"
    | "starting-deploy"
    | "ready"
    | "stopping"
    | "failed";

export interface LocalStackSnapshot {
    state: LocalStackState;
    error: string | null;
}

export interface LocalServiceHandle {
    stop(): Promise<void>;
}

export interface LocalStackRuntime {
    startService(name: LocalServiceName): Promise<LocalServiceHandle>;
    waitUntilReady(name: LocalServiceName): Promise<void>;
}

type LocalStackListener = (snapshot: LocalStackSnapshot) => void;

export class LocalStackManager {
    private state: LocalStackState = "stopped";
    private error: string | null = null;

    private metagame: LocalServiceHandle | null = null;
    private deploy: LocalServiceHandle | null = null;

    private readonly listeners = new Set<LocalStackListener>();

    constructor(private readonly runtime: LocalStackRuntime) {}

    get snapshot(): LocalStackSnapshot {
        return {
            state: this.state,
            error: this.error,
        };
    }

    onChange(listener: LocalStackListener): () => void {
        this.listeners.add(listener);

        return () => {
            this.listeners.delete(listener);
        };
    }

    async start(): Promise<void> {
        if (this.state !== "stopped" && this.state !== "failed") {
            throw new Error("local stack is already active");
        }

        this.error = null;

        try {
            this.setState("starting-metagame");

            this.metagame = await this.runtime.startService("metagame");
            await this.runtime.waitUntilReady("metagame");

            this.setState("starting-deploy");

            this.deploy = await this.runtime.startService("deploy");
            await this.runtime.waitUntilReady("deploy");

            this.setState("ready");
        } catch (error) {
            await this.cleanup();

            this.error =
                error instanceof Error ? error.message : String(error);

            this.setState("failed");
            throw error;
        }
    }

    async stop(): Promise<void> {
        if (this.state === "stopped") {
            return;
        }

        this.setState("stopping");

        await this.cleanup();

        this.error = null;
        this.setState("stopped");
    }


    // Stop in reverse dependency order: Deploy uses Metagame, so it must disappear first.
// Only handles returned by startService are stopped; unrelated system processes are never touched.
    private async cleanup(): Promise<void> {
        const deploy = this.deploy;
        const metagame = this.metagame;

        this.deploy = null;
        this.metagame = null;

        if (deploy !== null) {
            await deploy.stop().catch(() => undefined);
        }

        if (metagame !== null) {
            await metagame.stop().catch(() => undefined);
        }
    }

    private setState(state: LocalStackState): void {
        this.state = state;

        const snapshot = this.snapshot;

        for (const listener of this.listeners) {
            listener(snapshot);
        }
    }
}