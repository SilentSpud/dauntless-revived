import path from "node:path";
import { CheckSecret } from "../config";

export type AllowlistConfig = {
    bindHost: string;
    port: number;
    secret: string;
    // true: only log the PowerShell it would run. Never touches the firewall.
    dryRun: boolean;
    allowPrivate: boolean;
    ttlMs: number;
    minIntervalMs: number;
    maxEntries: number;
    // Local UDP ports the rule opens, PowerShell -LocalPort syntax ("8770-8777").
    ports: string;
    auditLog: string;
    stateFile: string;
    powershell: string;
};

function Int(Env: NodeJS.ProcessEnv, Name: string, Default: number, Min: number, Max: number): number {
    const Raw = Env[Name];
    if(Raw === undefined || Raw.trim() === ""){
        return Default;
    }
    const Value = Number(Raw);
    if(!Number.isInteger(Value) || Value < Min || Value > Max){
        throw new Error(`${Name} must be a whole number from ${Min} to ${Max}`);
    }
    return Value;
}

function Optional(Env: NodeJS.ProcessEnv, Name: string): string | undefined {
    const Raw = Env[Name];
    return Raw === undefined || Raw.trim() === "" ? undefined : Raw.trim();
}

// "8770-8777" or "8777": ports 1-65535, a range low to high, nothing else.
export function CheckPorts(Text: string): string {
    const Match = /^(\d{1,5})(?:-(\d{1,5}))?$/.exec(Text);
    if(Match === null){
        throw new Error(`ALLOWLIST_PORTS must be a port or a range like 8770-8777 (got ${JSON.stringify(Text)})`);
    }
    const Low = Number(Match[1]);
    const High = Match[2] === undefined ? Low : Number(Match[2]);
    if(Low < 1 || High > 65535 || Low > High){
        throw new Error("ALLOWLIST_PORTS must be within 1-65535, low to high");
    }
    return Match[2] === undefined ? String(Low) : `${Low}-${High}`;
}

export function DefaultPowerShell(): string {
    // A fixed path, never PATH: this process runs with administrator rights.
    const Root = process.env.SystemRoot ?? "C:\\Windows";
    return path.join(Root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
}

export function LoadAllowlistConfig(Env: NodeJS.ProcessEnv = process.env): AllowlistConfig {
    const DryRunRaw = Optional(Env, "ALLOWLIST_DRY_RUN");
    if(DryRunRaw !== "0" && DryRunRaw !== "1"){
        throw new Error("ALLOWLIST_DRY_RUN must be set: 1 only logs what it would do (any test machine), 0 really changes the firewall (the server)");
    }

    const BindHost = Optional(Env, "ALLOWLIST_BIND") ?? "127.0.0.1";
    if(BindHost !== "127.0.0.1" && BindHost !== "::1"){
        throw new Error("ALLOWLIST_BIND must be 127.0.0.1 or ::1: the helper runs with administrator rights and only the gateway on this machine may reach it");
    }

    return {
        bindHost: BindHost,
        port: Int(Env, "ALLOWLIST_PORT", 61005, 1, 65535),
        secret: CheckSecret(Optional(Env, "ALLOWLIST_SECRET"), "ALLOWLIST_SECRET"),
        dryRun: DryRunRaw === "1",
        allowPrivate: Env.ALLOWLIST_ALLOW_PRIVATE === "1",
        ttlMs: Int(Env, "ALLOWLIST_TTL_SECONDS", 600, 1, 86_400) * 1000,
        minIntervalMs: Int(Env, "ALLOWLIST_MIN_INTERVAL_MS", 3000, 50, 600_000),
        maxEntries: Int(Env, "ALLOWLIST_MAX_ENTRIES", 256, 1, 1000),
        ports: CheckPorts(Optional(Env, "ALLOWLIST_PORTS") ?? "8770-8777"),
        auditLog: path.resolve(Optional(Env, "ALLOWLIST_AUDIT_LOG") ?? "allowlist-audit.log"),
        stateFile: path.resolve(Optional(Env, "ALLOWLIST_STATE_FILE") ?? "allowlist-state.json"),
        powershell: Optional(Env, "ALLOWLIST_POWERSHELL") ?? DefaultPowerShell(),
    };
}
