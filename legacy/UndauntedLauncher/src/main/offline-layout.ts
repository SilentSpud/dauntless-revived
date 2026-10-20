import path from "node:path";

export interface OfflineLayout {
    runtimeRoot: string;
    runnerPath: string;
    metagameDir: string;
    deployDir: string;

    stateRoot: string;
    databasePath: string;
    runtimeStateDir: string;
    metagameEnvPath: string;
    deployEnvPath: string;
    readyDir: string;
}

export function offlineLayout(
    resourcesPath: string,
    userDataDir: string,
): OfflineLayout {
    const runtimeRoot = path.join(resourcesPath, "offline-runtime");
    const stateRoot = path.join(userDataDir, "offline");
    const runtimeStateDir = path.join(stateRoot, "runtime");

    return {
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
    };
}