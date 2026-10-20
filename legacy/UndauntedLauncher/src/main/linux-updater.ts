import { app } from "electron";
import { existsSync, promises as fs } from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

export type LinuxUpdate = { url: string; kind: "deb" | "rpm" | "appimage"; version: string };

function newer(a: string, b: string): boolean {
  const aa = a.split(".").map(Number), bb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(aa.length, bb.length); i++) {
    if ((aa[i] || 0) !== (bb[i] || 0)) return (aa[i] || 0) > (bb[i] || 0);
  }
  return false;
}

export async function findLinuxUpdate(): Promise<LinuxUpdate | null> {
  const response = await fetch("https://api.github.com/repos/mixutin/dauntless-revived/releases/latest", {
    headers: { Accept: "application/vnd.github+json", "User-Agent": `DauntlessRevivedLauncher/${app.getVersion()}` },
  });
  if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
  const release = await response.json() as { tag_name?: string; assets?: Array<{ name?: string; browser_download_url?: string }> };
  const version = String(release.tag_name || "").replace(/^launcher-v/, "");
  if (!version || !newer(version, app.getVersion())) return null;
  const wanted = process.env.APPIMAGE ? "AppImage" : existsSync("/usr/bin/dpkg") ? ".deb" : existsSync("/usr/bin/rpm") ? ".rpm" : "";
  if (!wanted) return null;
  const asset = release.assets?.find((a) => a.name?.includes(`-${version}-linux-`) && a.name.endsWith(wanted));
  if (!asset?.browser_download_url) return null;
  return { url: asset.browser_download_url, kind: wanted === ".deb" ? "deb" : wanted === ".rpm" ? "rpm" : "appimage", version };
}

export async function installLinuxUpdate(update: LinuxUpdate): Promise<"relaunch" | "quit"> {
  const dir = await fs.mkdtemp(path.join(app.getPath("temp"), "dauntless-revived-update-"));
  const ext = update.kind === "deb" ? ".deb" : update.kind === "rpm" ? ".rpm" : ".AppImage";
  const file = path.join(dir, `DauntlessRevivedLauncher-${update.version}${ext}`);
  const response = await fetch(update.url, { redirect: "follow" });
  if (!response.ok) throw new Error(`update download failed (${response.status})`);
  await fs.writeFile(file, Buffer.from(await response.arrayBuffer()));
  if (update.kind === "appimage") {
    const target = process.env.APPIMAGE;
    if (!target) throw new Error("APPIMAGE path is unavailable");
    const next = `${target}.new`;
    await fs.copyFile(file, next); await fs.chmod(next, 0o755); await fs.rename(next, target);
    app.relaunch({ execPath: target });
    return "relaunch";
  }
  const escaped = file.replaceAll('"', '\\"');
  const command = update.kind === "deb" ? `apt install -y "${escaped}"` : `rpm -U "${escaped}"`;
  spawn("pkexec", ["sh", "-c", command], { detached: true, stdio: "ignore" }).unref();
  return "quit";
}
