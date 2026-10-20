import { execFile } from "node:child_process";
import { accessSync, constants as fsConstants, existsSync, statSync } from "node:fs";
import os from "node:os";
import path from "node:path";

export type LinuxDependencyKind = "runtime" | "secure-storage";

export interface LinuxDependencyInstallPlan {
  name: string;
  command: string;
  args: string[];
}

function executable(file: string): boolean {
  try {
    accessSync(file, fsConstants.X_OK);
    return statSync(file).isFile();
  } catch {
    return false;
  }
}

function findExecutable(name: string, env: NodeJS.ProcessEnv = process.env): string | null {
  if (path.isAbsolute(name)) return executable(name) ? name : null;
  for (const dir of (env.PATH ?? "").split(path.delimiter)) {
    if (!dir) continue;
    const candidate = path.join(dir, name);
    if (executable(candidate)) return candidate;
  }
  for (const dir of ["/usr/bin", "/usr/sbin", "/bin", "/sbin", "/usr/local/bin"]) {
    const candidate = path.join(dir, name);
    if (executable(candidate)) return candidate;
  }
  return null;
}

function elevated(command: string, args: string[], env: NodeJS.ProcessEnv): LinuxDependencyInstallPlan | null {
  if (typeof process.getuid === "function" && process.getuid() === 0) {
    return { name: path.basename(command), command, args };
  }
  const pkexec = findExecutable("pkexec", env);
  return pkexec ? { name: path.basename(command), command: pkexec, args: [command, ...args] } : null;
}

export function linuxDependencyInstallPlan(
  kind: LinuxDependencyKind,
  env: NodeJS.ProcessEnv = process.env,
): LinuxDependencyInstallPlan | null {
  const apt = findExecutable("apt-get", env);
  if (apt) {
    return elevated(
      apt,
      kind === "runtime"
        ? ["install", "-y", "wine64"]
        : ["install", "-y", "gnome-keyring", "libsecret-1-0"],
      env,
    );
  }
  const dnf = findExecutable("dnf", env);
  if (dnf) {
    return elevated(
      dnf,
      kind === "runtime" ? ["install", "-y", "wine"] : ["install", "-y", "gnome-keyring", "libsecret"],
      env,
    );
  }

  const zypper = findExecutable("zypper", env);
  if (zypper) {
    return elevated(
      zypper,
      kind === "runtime"
        ? ["--non-interactive", "install", "wine"]
        : ["--non-interactive", "install", "gnome-keyring", "libsecret"],
      env,
    );
  }

  const pacman = findExecutable("pacman", env);
  if (pacman) {
    return elevated(
      pacman,
      kind === "runtime"
        ? ["-S", "--needed", "--noconfirm", "wine"]
        : ["-S", "--needed", "--noconfirm", "gnome-keyring", "libsecret"],
      env,
    );
  }
  const xbps = findExecutable("xbps-install", env);
  if (xbps) {
    return elevated(
      xbps,
      kind === "runtime" ? ["-Sy", "wine"] : ["-Sy", "gnome-keyring", "libsecret"],
      env,
    );
  }

  return null;
}

export function runLinuxDependencyInstall(
  plan: LinuxDependencyInstallPlan,
  env: NodeJS.ProcessEnv = process.env,
  cwd: string = env.HOME ?? os.homedir(),
  timeoutMs = 15 * 60_000,
): Promise<void> {
  return new Promise((resolve, reject) => {
    execFile(plan.command, plan.args, { env, cwd, timeout: timeoutMs, windowsHide: true }, (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
}

export function repairLinuxDesktopEnvironment(
  env: NodeJS.ProcessEnv = process.env,
  uid: number | null = typeof process.getuid === "function" ? process.getuid() : null,
): { forceGnomeLibsecret: boolean } {
  if (process.platform !== "linux") return { forceGnomeLibsecret: false };
  const runtimeDir = env.XDG_RUNTIME_DIR || (uid === null ? "" : `/run/user/${uid}`);
  if (!env.XDG_RUNTIME_DIR && runtimeDir && existsSync(runtimeDir)) env.XDG_RUNTIME_DIR = runtimeDir;
  if (!env.DBUS_SESSION_BUS_ADDRESS && runtimeDir && existsSync(path.join(runtimeDir, "bus"))) {
    env.DBUS_SESSION_BUS_ADDRESS = `unix:path=${path.join(runtimeDir, "bus")}`;
  }

  const desktop = `${env.XDG_CURRENT_DESKTOP ?? ""} ${env.DESKTOP_SESSION ?? ""}`.toLowerCase();
  const gnomeFamily = /(gnome|ubuntu|cinnamon|mate|xfce)/.test(desktop);
  const keyringSocket = runtimeDir ? path.join(runtimeDir, "keyring", "control") : "";
  return { forceGnomeLibsecret: gnomeFamily || (keyringSocket !== "" && existsSync(keyringSocket)) };
}
