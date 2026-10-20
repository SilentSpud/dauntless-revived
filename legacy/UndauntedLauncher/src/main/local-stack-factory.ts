import { utilityProcess } from "electron";
import path from "node:path";
import { EXE_RELATIVE_PATH } from "./constants";
import { defaultConfigDir } from "./engineini";
import { LocalStackManager } from "./local-stack";
import { ElectronLocalStackRuntime } from "./local-stack-runtime";
import { localServiceEnvironment, resolveLocalStackPaths } from "./local-stack-options";
import { prepareOfflineState } from "./offline-state";
import { prepareOfflineGameConfig } from "./offline-game-config";

type PathOptions = Parameters<typeof resolveLocalStackPaths>[0];

export function makeLocalStack(options: PathOptions & { gameRoot(): string }): LocalStackManager | null {
    const resolved = resolveLocalStackPaths(options);
    if (!resolved) return null;
    const layout = resolved.layout;
    const env = layout ? localServiceEnvironment(options.env) : undefined;
    let keyFile: string | undefined;
    const runtime = new ElectronLocalStackRuntime({
        ...resolved,
        env,
        beforeStart: layout ? async (name) => {
            if (name === "metagame") {
                // Server diagnostics are opt-in separately from the client showConsole setting.
                keyFile = await prepareOfflineState(layout, path.join(options.gameRoot(), EXE_RELATIVE_PATH),
                    options.env.DAUNTLESS_REVIVED_LOCAL_SERVER_CONSOLE === "1");
                await prepareOfflineGameConfig(defaultConfigDir(options.env), path.join(layout.runtimeRoot, "game-endpoints.json"));
            } else {
                if (!keyFile) throw new Error("Offline state has not been prepared");
                // Metagame readiness proves its migrations ran. The server kit's helper
                // now registers the saved credential in that DB before Deploy can use it.
                await registerGameserverKey(path.join(layout.runtimeRoot, "register-gameserver-key.cjs"),
                    ["gs-key", layout.metagameDir, layout.databasePath, keyFile], env!);
            }
        } : undefined,
    });
    return new LocalStackManager(runtime);
}

function registerGameserverKey(helper: string, args: string[], env: NodeJS.ProcessEnv): Promise<void> {
    return new Promise((resolve, reject) => {
        const child = utilityProcess.fork(helper, args, {
            env, stdio: "inherit", serviceName: "Dauntless Revived local credential initialization",
        });
        const timer = setTimeout(() => {
            child.kill();
            reject(new Error("Offline game-server credential registration timed out"));
        }, 15_000);
        child.once("exit", (code) => {
            clearTimeout(timer);
            if (code === 0) resolve();
            else reject(new Error("Offline game-server credential registration failed"));
        });
    });
}
