// Which links the launcher may open in the browser. The renderer never passes a URL: it names a
// target (an ExternalTarget), the main process picks the URL and checks it here before
// shell.openExternal.

import type { ExternalTarget } from "../shared/types";
import {
  PROJECT_CONTRIBUTORS_URL,
  PROJECT_LICENSE_URL,
  PROJECT_URL,
  TAILSCALE_DOWNLOAD_URL,
  UPSTREAM_CONTRIBUTORS_URL,
  UPSTREAM_URL,
  VC_REDIST_URL,
} from "./constants";

// The targets that always open one fixed URL. The other two come from the server side: the
// Tailscale share link in a private invite, and the host's own source link.
export type FixedTarget = Exclude<ExternalTarget, "tailscale_share" | "server_source">;

const EUGAMEHOST_URL = "https://www.eugamehost.com/";
const EUGAMEHOST_GAME1_URL = "https://www.eugamehost.com/clients/cart.php?a=add&pid=238&promocode=SIGNUP6MONTH&skipconfig=1";
const EUGAMEHOST_GAME2_URL = "https://www.eugamehost.com/clients/cart.php?a=add&pid=239&promocode=SIGNUP6MONTH&skipconfig=1";
const EUGAMEHOST_GAME3_URL = "https://www.eugamehost.com/clients/cart.php?a=add&pid=240&promocode=SIGNUP6MONTH&skipconfig=1";
const EUGAMEHOST_GAME5_URL = "https://www.eugamehost.com/clients/cart.php?a=add&pid=242&promocode=SIGNUP6MONTH&skipconfig=1";
const EUGAMEHOST_5800X_URL = "https://www.eugamehost.com/clients/cart.php?a=add&pid=500";
const DISCORD_URL = "https://discord.gg/dauntlessrevived";
const PATREON_URL = "https://patreon.com/DauntlessRevived";

export const FIXED_LINKS: Readonly<Record<FixedTarget, string>> = Object.freeze({
  tailscale_download: TAILSCALE_DOWNLOAD_URL,
  vc_redist: VC_REDIST_URL,
  directx_runtime: 'https://www.microsoft.com/en-us/download/details.aspx?id=35',
  project_source: PROJECT_URL,
  project_license: PROJECT_LICENSE_URL,
  project_contributors: PROJECT_CONTRIBUTORS_URL,
  upstream_source: UPSTREAM_URL,
  upstream_contributors: UPSTREAM_CONTRIBUTORS_URL,
  eugamehost: EUGAMEHOST_URL,
  eugamehost_game1: EUGAMEHOST_GAME1_URL,
  eugamehost_game2: EUGAMEHOST_GAME2_URL,
  eugamehost_game3: EUGAMEHOST_GAME3_URL,
  eugamehost_game5: EUGAMEHOST_GAME5_URL,
  eugamehost_5800x: EUGAMEHOST_5800X_URL,
  discord: DISCORD_URL,
  patreon: PATREON_URL,
});

// The URL of a fixed target, or null for anything else (a raw URL, an unknown name, "__proto__").
export function fixedLinkUrl(target: unknown): string | null {
  if (typeof target !== "string" || !Object.prototype.hasOwnProperty.call(FIXED_LINKS, target)) return null;
  return FIXED_LINKS[target as FixedTarget];
}

// On github.com only these exact pages: the project's and upstream Undaunted's.
const GITHUB_PAGES: ReadonlySet<string> = new Set([PROJECT_URL, PROJECT_LICENSE_URL, PROJECT_CONTRIBUTORS_URL, UPSTREAM_URL, UPSTREAM_CONTRIBUTORS_URL]);

// Plain https without user info. An explicit port only where the caller allows one.
function parse(url: string, allowPort = false): URL | null {
  if (typeof url !== "string" || url.length > 1024) return null;
  try {
    const u = new URL(url);
    if (u.protocol !== "https:" || u.username !== "" || u.password !== "") return null;
    if (u.port !== "" && !allowPort) return null;
    return u;
  } catch {
    return null;
  }
}

export function isAllowedStaticUrl(url: string): boolean {
  const u = parse(url);
  if (!u) return false;
  switch (u.hostname) {
    case 'www.microsoft.com':
      return url === FIXED_LINKS.directx_runtime;
    case "tailscale.com":
    case "login.tailscale.com":
      return true;
    case "aka.ms":
      return u.pathname.startsWith("/vs/");
    case "github.com":
      return GITHUB_PAGES.has(url);
    case "www.eugamehost.com":
      return url === EUGAMEHOST_URL || url === EUGAMEHOST_GAME1_URL || url === EUGAMEHOST_GAME2_URL || url === EUGAMEHOST_GAME3_URL || url === EUGAMEHOST_GAME5_URL || url === EUGAMEHOST_5800X_URL;
    case "discord.gg":
      return url === DISCORD_URL;
    case "patreon.com":
      return url === PATREON_URL;
    default:
      return false;
  }
}

// The host's own source link (from ServerStatus.sourceUrl) is allowed as long as it is plain https,
// the same rule the status check applies (cleanHttpsUrl in src/shared/status.ts). That includes an
// explicit port, as for a self-hosted code host on its own port: the status check keeps such a link
// and the page shows its Source button, so refusing it here would make that button do nothing.
export function isAllowedExternalUrl(url: string, hostSourceUrl: string | null): boolean {
  if (isAllowedStaticUrl(url)) return true;
  if (hostSourceUrl !== null && url === hostSourceUrl) return parse(url, true) !== null;
  return false;
}
