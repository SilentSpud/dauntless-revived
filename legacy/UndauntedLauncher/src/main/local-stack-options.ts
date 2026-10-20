import path from "node:path";
import { offlineLayout, type OfflineLayout } from "./offline-layout";
import type { ElectronLocalStackRuntimeOptions } from "./local-stack-runtime";

export interface LocalStackPaths extends ElectronLocalStackRuntimeOptions {
    layout?: OfflineLayout;
}

export function resolveLocalStackPaths(options: {
    packaged: boolean;
    platform: NodeJS.Platform;
    resourcesPath: string;
    userDataDir: string;
    appPath: string;
    env: NodeJS.ProcessEnv;
}): LocalStackPaths | null {
    let metagameDir: string;
    let deployDir: string;
    let metagameEnv: string;
    let deployEnv: string;
    let runnerPath: string;
    let layout: OfflineLayout | undefined;

    if (options.packaged) {
        if (options.platform !== "win32") return null;
        layout = offlineLayout(options.resourcesPath, options.userDataDir);
        metagameDir = layout.metagameDir;
        deployDir = layout.deployDir;
        metagameEnv = layout.metagameEnvPath;
        deployEnv = layout.deployEnvPath;
        runnerPath = layout.runnerPath;
    } else {
        if (options.env.DAUNTLESS_REVIVED_LOCAL_STACK_DEV !== "1") return null;
        const paths = [
            "DAUNTLESS_REVIVED_LOCAL_METAGAME_DIR",
            "DAUNTLESS_REVIVED_LOCAL_DEPLOY_DIR",
            "DAUNTLESS_REVIVED_LOCAL_METAGAME_ENV",
            "DAUNTLESS_REVIVED_LOCAL_DEPLOY_ENV",
        ].map((key) => options.env[key]?.trim());
        if (paths.some((value) => !value)) {
            throw new Error("local stack development paths are not configured");
        }
        [metagameDir, deployDir, metagameEnv, deployEnv] = paths.map((value) => path.resolve(value!));
        runnerPath = path.join(options.appPath, "runtime", "local-service-runner.cjs");
    }

    return {
        layout,
        runnerPath,
        services: {
            metagame: {
                serverPath: path.join(metagameDir, "dist", "server.js"),
                cwd: metagameDir,
                envPath: metagameEnv,
                port: 61000,
                healthPath: "/undaunted/api/RegistrationStatus",
                // Clean installations may need time to create and migrate the database.
                readyTimeoutMs: options.packaged ? 90_000 : undefined,
            },
            deploy: {
                serverPath: path.join(deployDir, "dist", "server.js"),
                cwd: deployDir,
                envPath: deployEnv,
                port: 61001,
                healthPath: "/gameservers",
                readyTimeoutMs: options.packaged ? 20_000 : undefined,
            },
        },
    };
}

// loadEnvFile leaves inherited values in place. Packaged children receive only OS
// plumbing, so shell settings cannot override authentication, bind addresses or DB paths.
export function localServiceEnvironment(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
    const allowed = new Set([
        "PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP",
        "USERPROFILE", "LOCALAPPDATA", "APPDATA", "PROGRAMDATA", "PROGRAMFILES",
        "PROGRAMFILES(X86)", "SYSTEMDRIVE", "HOMEDRIVE", "HOMEPATH",
    ]);
    return Object.fromEntries(Object.entries(env).filter(([key]) => allowed.has(key.toUpperCase())));
}
