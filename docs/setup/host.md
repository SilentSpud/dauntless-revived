---
title: Host a server
parent: Setup
nav_order: 1
description: "Step-by-step guide to hosting a Dauntless 1.4.4 private server on one Windows PC: verify the build, place the two DLLs, set up the metagame and deploy server."
lang: en
ref: setup/host
---

{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}
{% assign verification_page = site.pages | where: "path", "findings/verification.md" | first %}
{% assign multiplayer_page = site.pages | where: "path", "findings/multiplayer.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}

# Host a server
{: .no_toc }

This is how we run Dauntless Revived on one Windows PC. It uses the genuine **Dauntless 1.4.4** client
(October 2020, UE4, pak v9), Undaunted's server DLL, and our fork of Undaunted's metagame and deploy
server. Everything on this page stays on `127.0.0.1`. Letting friends connect is a separate step,
covered in [Run it for a group]({{ admin_page.url | relative_url }}).

Unless a line says otherwise, every fact here is about **client build 1.4.4**. Build 2.1.1 (the final
"Awakening" client, UE5, IoStore) comes up only where our earlier work on it explains a choice.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## How the pieces fit {#how-the-pieces-fit}

| Piece | What it is | Listens on |
|---|---|---|
| Game client | `Dauntless-Win64-Shipping.exe` plus two DLLs. In client mode the DLL points all 167 backend endpoints at `http://<first command-line argument>` | nothing |
| Metagame | Node + Express + SQLite (`legacy/UndauntedMetagame`). Accounts, characters, inventory, loadouts, matchmaking queue | TCP `127.0.0.1:61000` |
| Deploy server | Node (`legacy/UndauntedDeployServer`). Starts and watches game-server processes when the metagame asks | TCP `127.0.0.1:61001` (never expose it: it has no authentication) |
| Game servers | More copies of the **same exe**, started with `-server -nullrhi`. The DLL switches them into server mode | UDP 8770-8777 |

A session goes like this:

1. The client logs in to the metagame with your account key and gets a 24-hour token.
2. When you pick an activity, the metagame asks the deploy server for a game server.
3. The deploy server starts one (or returns the permanent Ramsgate server on UDP 8777).
4. The client travels to `127.0.0.1:<port>` over UDP.
5. The game server loads your character from the metagame. These calls carry a separate game-server
   key, and the request log marks them `gs=1`.

The retail exe can't host on its own. Why an injected DLL can still run a server is explained in
[How multiplayer works]({{ multiplayer_page.url | relative_url }}). We first concluded that multiplayer was impossible
with a client-only build. That was wrong, and that page explains why.

---

## 1. Requirements {#requirements}

> **Renting a server?** EU Gamehost is a partnered and recommended hosting provider for this project.
> See the [EU Gamehost plan guide]({{ "/setup/hosting-partner.html" | relative_url }}) for clearly labelled partner content, plan examples and our capacity rationale.

| | What we use | Notes |
|---|---|---|
| OS | Windows 10 22H2 (build 19045) | Windows 11 should work. We have not tested it. |
| Node.js | v24.19.0 (npm 11.17.0) | `better-sqlite3` is a native module tied to the Node version. Don't upgrade Node while people are playing. |
| Git | 2.55 for Windows | Needed for the fork. See the path notes in step 4. |
| PowerShell | Windows PowerShell 5.1 (built in) | Every command on this page is written for 5.1. |
| VC++ runtime | Microsoft Visual C++ 2015-2022, x64 | `UndauntedInternalServer.dll` imports `MSVCP140.dll` and `VCRUNTIME140_1.dll`. Our PC already had it. |
| Disk | About 12 GB | Game 10.9 GB (412 files once the DLLs are in), fork with `node_modules` about 0.3 GB, and a small database. Extracting from an archive needs roughly the same space again, temporarily. |
| RAM | 32 GB on our host | See the measurements below. 16 GB is probably enough for one player plus servers. We have not tested that. |
| GPU | Any DirectX 11 card for the client | Game servers run with `-nullrhi` and use no GPU. |

**Measured on our host** (1.4.4, one player, Windows Task Manager working set):

| Process | Memory | CPU |
|---|---|---|
| Ramsgate server (UDP 8777) | about 1.1 GB (peak 1.19 GB) | about 0.6 core while loading, then about 0.2 core |
| Tutorial / hunt server | about 0.9 GB | not yet measured under 4-player combat |
| Training Dojo server | about 0.94 GB | about 0.23 core |
| Client, Cinematic quality, 1920x1080 | 1.9-2.3 GB working set (about 3.7 GB private) | about 2.5 cores |
| Metagame + deploy server (node) | about 130 MB together | negligible |

For one person playing on the host, that is about 4-5 GB of game processes in total. Each extra
concurrent hunt adds about 1 GB.

---

## 2. Get the 1.4.4 build and verify it {#verify-the-build}

This site and the repository don't host or link game files. Where you get a copy is your decision.
Whatever the source, check it before you run anything from it. All three checks below are cheap.

**a. Version string.** `Version.txt` in the game root must contain exactly this string:

```
dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827
```

The file ends with a space and a line break, so compare trimmed text:
`(Get-Content C:\D144\Dauntless\Version.txt).Trim()`. The trailing `239827` is the build changelist.
It comes back as `TARGET_CHANGELIST` in step 9.

**b. The exe hash.** Every offset in Undaunted's DLL is compiled for this exact file:

```powershell
(Get-FileHash "C:\D144\Dauntless\Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe").Hash
# must be D3D41E614908D2BEFD518B27046D9822D6130EF12BA3504BABBDB786BEF9CFF4  (103,673,520 bytes)
```

**c. The whole install against Phoenix's own manifest.** The game root holds `Manifest.bin.json`, with
one entry per file (`RelativePath`, `FileSize`, `MD5Chunks`). `MD5Chunks` is a two-character prefix
(`18` in this build) followed by one MD5 per 16 MiB chunk of the file. We read the prefix as
2^0x18 = 16 MiB. That is our interpretation, but it fits every file. Save this as `verify-manifest.js`
anywhere and run it with Node:

```js
// node verify-manifest.js <game folder>
const fs = require("fs"), path = require("path"), crypto = require("crypto");
const root = process.argv[2] || ".";
const man = JSON.parse(fs.readFileSync(path.join(root, "Manifest.bin.json"), "utf8").replace(/^\uFEFF/, ""));
const CHUNK = 1 << 24; // 16 MiB
let ok = 0, bad = 0, missing = 0;
const buf = Buffer.alloc(CHUNK);
for (const e of man.TargetFiles) {
  const p = path.join(root, e.RelativePath.replace(/\\/g, path.sep));
  if (!fs.existsSync(p)) { missing++; console.log("MISSING  " + e.RelativePath); continue; }
  const want = e.MD5Chunks.slice(2).match(/.{32}/g) || [];
  const fd = fs.openSync(p, "r"); const got = []; let n;
  while ((n = fs.readSync(fd, buf, 0, CHUNK, null)) > 0) got.push(crypto.createHash("md5").update(buf.subarray(0, n)).digest("hex"));
  fs.closeSync(fd);
  if (fs.statSync(p).size === Number(e.FileSize) && got.join() === want.join()) ok++;
  else { bad++; console.log("MISMATCH " + e.RelativePath); }
}
console.log(`manifest files: ${man.TargetFiles.length}  ok: ${ok}  mismatched: ${bad}  missing: ${missing}`);
process.exit(bad || missing ? 1 : 0);
```

```
PS> node verify-manifest.js C:\D144\Dauntless
manifest files: 406  ok: 406  mismatched: 0  missing: 0
```

On our copy it took 18 seconds. Files on disk that are not in the manifest are expected: the manifest
itself, `Manifest.bin`, two empty `debug.log` files, and later the two DLLs you add in step 5.

The same manifest also exists in 2.1.1. [Verifying game files]({{ verification_page.url | relative_url }}) covers the
deeper checks we ran on both builds: Authenticode digests, why `osslsigncode` says "FAILED" on
untouched files, and the ClamAV scans.

---

## 3. Install at a short path: `C:\D144` {#short-install-path}

We keep the game at `C:\D144\Dauntless`, so the binaries are in
`C:\D144\Dauntless\Archon\Binaries\Win64`. If your copy is a zip, the `tar` built into Windows 10
unpacks it:

```powershell
New-Item -ItemType Directory -Force C:\D144 | Out-Null
tar -xf "<path to your archive>.zip" -C C:\D144
Get-ChildItem C:\D144\Dauntless     # Archon, EasyAntiCheat, Engine, Dauntless.exe, Manifest.bin(.json), Version.txt
```

**Why a short path.** Classic Win32 file APIs stop at 260 characters (`MAX_PATH`) unless both
Windows and the program opt in to long paths. Most of the tools around this project don't opt in.

- The game's own tree is not the problem. Its longest file path is 102 characters below the install
  root. We have never seen the game itself fail because of path length.
- The tools around it did fail. `git clone` of Undaunted into a deeply nested working folder broke
  with `Filename too long` and then `fatal: '$GIT_DIR' too big`. The fork has about 4,300 tracked
  files, 4,148 of them in the generated Dumper-7 SDK, and paths inside the repository run up to 94
  characters. The MSVC build of the server DLL will need the same headroom.
- A root of a few characters keeps everything far inside the limit. It also keeps command lines and
  `.env` values short, and avoids spaces in paths.

We avoided the system-wide long-path switch (`LongPathsEnabled`). It is a system setting, and a short
path makes it unnecessary. Keep the install out of OneDrive or other synced folders, and away from
your user profile.

Launch only `Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe`. We have never run the root
`Dauntless.exe` or `EasyAntiCheat\EasyAntiCheat_Setup.exe`, and nothing here needs them.

---

## 4. Get the fork onto a short path {#fork}

The fork lives at `C:\dr\undaunted`, branch `dauntless-revived`. Runtime data (the SQLite database,
keys, logs, pid files) goes in `C:\dr\data`, outside the repository.

```powershell
New-Item -ItemType Directory -Force C:\dr, C:\dr\data | Out-Null
git -c core.longpaths=true clone <URL of this repository> C:\dr\undaunted
git -C C:\dr\undaunted config core.longpaths true
git -C C:\dr\undaunted checkout dauntless-revived
git -C C:\dr\undaunted remote add upstream https://github.com/SyST3MDeV/Undaunted.git   # optional, to follow upstream
```

The repository contains six projects (the `Undaunted*` folders), plus the Windows server kit, the
friend kit and tools. On the host we use:

- `legacy/UndauntedMetagame`: the backend.
- `legacy/UndauntedDeployServer`: the game-server supervisor.
- The two prebuilt DLLs kept in `legacy/UndauntedLauncher/assets/`.

The Electron app in `legacy/UndauntedLauncher/src` is this fork's friend launcher: players use it to join a
server from an invite, and it downloads the game from the host's own content server. The host setup
on this page doesn't need it. The C++ source of the server DLL is in `native/internal-server`. Every
folder, and what builds where, is listed on [Files and data]({{ files_page.url | relative_url }}).

The `.env` files you create below are git-ignored. Never commit them: they hold the signing keys and
the game-server key.

---

## 5. Place the two DLLs, checking the pinned hashes {#dlls}

Undaunted ships two prebuilt DLLs:

- `dxgi.dll` is a proxy. Windows loads it from the game folder instead of the system `dxgi.dll`. It
  loads the real `System32\dxgi.dll`, forwards `CreateDXGIFactory`, `CreateDXGIFactory1` and
  `CreateDXGIFactory2`, and loads `UndauntedInternalServer.dll`. That is all it does.
- `UndauntedInternalServer.dll` has two modes. If the command line contains `-server`, it turns the
  process into a game server. Otherwise it runs as a client and redirects the backend endpoints.

Copy them only after their hashes match:

```powershell
$A = "C:\dr\undaunted\legacy\UndauntedLauncher\assets"
$W = "C:\D144\Dauntless\Archon\Binaries\Win64"
$pin = @{
  "dxgi.dll"                    = "9A431D7B6FD20C43FA92BEBD91C3BC023EC7A3FCBC52871C41F4DF293D4B0D1F"  # 11,264 bytes
  "UndauntedInternalServer.dll" = "520EC588A0554E374B2B0D084CD7F7F08D59A9CB80362679845719D64A0D0933"  # 123,392 bytes
}
foreach ($f in $pin.Keys) {
  if ((Get-FileHash "$A\$f").Hash -ne $pin[$f]) { throw "$f does not match its pinned hash - stop here" }
  Copy-Item "$A\$f" $W -Force
}
foreach ($f in $pin.Keys) { "{0,-30} in place, hash ok: {1}" -f $f, ((Get-FileHash "$W\$f").Hash -eq $pin[$f]) }
```

Things to know before you rely on them:

- **1.4.4 only.** The server DLL patches fixed addresses in the exe from step 2b and has no version
  check. In any other build, including 2.1.1, it would crash the game or corrupt its memory. Never
  copy these files into another install.
- **Both files are unsigned**, and a proxy DLL that hooks the game is the kind of thing heuristic
  antivirus flags. On our PC, Windows Defender reported no detections after the copy. See
  [Troubleshooting]({{ trouble_page.url | relative_url }}#windows-defender-and-the-unsigned-dlls) if yours does.
- We analysed both binaries statically before using them. A disassembly of `dxgi.dll`'s `DllMain`
  shows it does only what is listed above. The imports of `UndauntedInternalServer.dll` and every
  hard-coded game address in it match the published source or its generated SDK, and it has no
  networking, registry, process-creation or file-write imports of its own. The source of `dxgi.dll`
  is not in the Undaunted repository and, as far as we know, has not been published. Building the
  server DLL from source and writing our own `dxgi` proxy are on the
  [roadmap]({{ roadmap_page.url | relative_url }}).
- In client mode the DLL opens a console window and enables an in-game console on **F2**.

---

## 6. Keep the config folder separate from other builds {#config-folder}

Both builds we have used, 1.4.4 and 2.1.1, share one user config folder, because the project name is
`Archon` in both:

```
%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\
```

The game servers run as your Windows user, so they read this folder too. If you have ever run
another build, especially 2.1.1 with experimental settings, park its folder before you set up 1.4.4.
Leftovers such as a `GameDefaultMap` override or an `[OnlineSubsystemPhoenix]` block in `Engine.ini`
would otherwise apply to 1.4.4 as well.

```powershell
$U = "$env:LOCALAPPDATA\Archon\Saved\Config"
if (Test-Path "$U\WindowsClient") { Rename-Item "$U\WindowsClient" "WindowsClient.211" }
New-Item -ItemType Directory -Force "$U\WindowsClient" | Out-Null
```

Swap the two folders back before you run the other build again. The game fills this folder with its
own files on first start. You only write `Engine.ini` (step 7) and `Game.ini` (step 8).

Run everything (metagame, deploy server, game servers, client) as the **same Windows user**. Game
servers started under another account or as a service would read a different `%LOCALAPPDATA%`. We
have not tested running them as a service.

---

## 7. Engine.ini {#engine-ini}

`[SystemSettings]` in the user `Engine.ini` overrides the in-game menu. Our launcher script
(`play.ps1`, step 13) rewrites two sections on every launch, `[SystemSettings]` and
`[OnlineSubsystemMcp.XMPP]` (below), and leaves the rest of the file alone. The game itself writes
sections such as `[Core.System]` and `[WindowsApplication.Accessibility]`; leave those alone. You
don't need to create `Engine.ini` by hand. After a launch with the default `-Graphics 4`,
`[SystemSettings]` looks like this:

```ini
[SystemSettings]
r.Streaming.PoolSize=3000
r.Streaming.LimitPoolSizeToVRAM=1
gc.TimeBetweenPurgingPendingKillObjects=10
s.ForceGCAfterLevelStreamedOut=1
sg.ViewDistanceQuality=4
sg.AntiAliasingQuality=4
sg.ShadowQuality=4
sg.PostProcessQuality=4
sg.TextureQuality=4
sg.EffectsQuality=4
sg.FoliageQuality=4
sg.ShadingQuality=4
sg.ResolutionQuality=100
r.ScreenPercentage=100
r.MipMapLODBias=0
r.MaxAnisotropy=16
r.Tonemapper.Sharpen=0.6
```

| Setting | Why |
|---|---|
| `r.Streaming.PoolSize=3000`, `r.Streaming.LimitPoolSizeToVRAM=1` | Texture streaming pool in MB, never larger than the GPU's memory. This bounds the biggest memory consumer without lowering quality. |
| `gc.TimeBetweenPurgingPendingKillObjects=10`, `s.ForceGCAfterLevelStreamedOut=1` | Collect garbage more often and after each streamed-out level. Affects memory, not image quality. |
| `sg.*Quality=N` | The forced quality level (see step 14). |
| `sg.ResolutionQuality=100`, `r.ScreenPercentage=100` | Render at full native resolution. |
| `r.MipMapLODBias=0`, `r.MaxAnisotropy=16` | Full-resolution texture mips; sharp textures at glancing angles. |
| `r.Tonemapper.Sharpen=0.6` | Counters the softness of UE4's temporal anti-aliasing. |

Game servers share this file but run with `-nullrhi`, so only the memory lines matter to them.

**Chat and presence (XMPP).** As shipped, 1.4.4's chat and presence connection points at Epic's live
server (`wss://xmpp-service-prod.ol.epicgames.com:443`). The client keeps reconnecting to it and
sends the account id and our login token. `play.ps1` points it at this PC instead:

```ini
[OnlineSubsystemMcp.XMPP]
ServerAddr="ws://127.0.0.1"
ServerPort=61099
bUseSSL=false
```

With `CHAT=1` in the metagame's `.env` (step 9), the metagame listens on port 61099 and in-game text
chat works: Ramsgate, party, guild and whispers, with usernames. Without it nothing listens there, so
the connection fails, the same way it already fails against Epic's server, and the game carries on.
The address is quoted for the same reason as the endpoints in step 8. We have
confirmed that the game keeps this section when it rewrites `Engine.ini`. **Verified 2026-09-21:** in
the first 90 seconds after launch there were no connections outside the PC; the client tried local
port 61099 instead (item 0.5 on the [roadmap]({{ roadmap_page.url | relative_url }})).

Never put map overrides or `[OnlineSubsystemPhoenix]` endpoints in `Engine.ini`. Those endpoints
belong in `Game.ini`, and they must be quoted (step 8).

---

## 8. Game.ini: 167 quoted endpoint overrides for the game servers {#game-ini}

**Why the servers need it.** In client mode the DLL hooks the engine's config lookup and answers 167
endpoint keys with `http://<metagame>/...`. In server mode it installs no such hook. A game server
therefore reads its endpoints from the normal config chain: the cooked defaults, which point at
Phoenix's `https://*.steelyard.ca` hosts (gone since 2025-05-30), and then this user's `Game.ini`.
Without the overrides, a game server can't load or save anyone's character.

The client's hook takes precedence for the same keys, so the file doesn't change anything for the
client. Friends connecting from another PC don't need it.

**Generate it from the DLL's own endpoint table.** Don't hand-edit it. The table is in
`native/internal-server/dllmain.cpp` as entries shaped like
`{L"AuthEndpoint", L"http://" + Globals::MetagameAddress + L"/game/login"}`. Save this as
`C:\dr\tools\make-gameini.ps1`:

```powershell
param([string]$Metagame = "127.0.0.1:61000",
      [string]$Source = "C:\dr\undaunted\native\internal-server\dllmain.cpp",
      [string]$Out = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient\Game.ini")
# Every endpoint the client-mode DLL rewrites: {L"Key", L"http://" + Globals::MetagameAddress [+ L"/path"]}
$re = '^\s*\{L"([A-Za-z0-9_]+)", L"http://" \+ Globals::MetagameAddress(?: \+ L"([^"]*)")?\},?'
$lines = foreach ($l in Get-Content $Source) {
  if ($l -match $re) { '{0}="http://{1}{2}"' -f $Matches[1], $Metagame, $Matches[2] }
}
Set-Content $Out -Encoding ASCII -Value (@("[OnlineSubsystemPhoenix]") + $lines)
"wrote $($lines.Count) quoted endpoint overrides to $Out"
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\make-gameini.ps1
# wrote 167 quoted endpoint overrides to ...\WindowsClient\Game.ini
```

The result starts like this:

```ini
[OnlineSubsystemPhoenix]
AuthEndpoint="http://127.0.0.1:61000/game/login"
AuthAvailableEndpoint="http://127.0.0.1:61000/checkavailable"
AuthTagsEndpoint="http://127.0.0.1:61000/tags"
...
```

Checks we ran on our copy:

- All 167 keys exist in the cooked 1.4.4 `DefaultGame.ini` `[OnlineSubsystemPhoenix]` section, with
  no duplicates.
- Two of them, `MatchmakingEndpoint` and `TrackingEndpoint`, are the bare host with no path.
  Keep that in mind if you ever edit the port by hand (see below).
- In Phoenix's shipped config, one key, `PhoenixEventsMessageEndpoint`, held a live Slack webhook URL.
  We don't reproduce it. With this override it points at our metagame, so nothing is ever sent to
  Slack. The DLL's table keeps that URL's path, though, and the path contains the webhook's secret
  part, so the generated `Game.ini` does too. Treat your `Game.ini` as private: don't paste it into
  chats, issues or screenshots.
- Phoenix's cooked section quotes all 167 of these URLs, as our generated file does.

**Why quoting matters.** The engine's ini parser treats `//` in an *unquoted* value as the start of a
comment and cuts the value there. We saw the result on 2.1.1: an `[OnlineSubsystemPhoenix]` block in
the user `Engine.ini` had all 163 of its values reduced to the bare string `https:` (for example
`AccountInfoEndpoint=https:`). The game writes its config files back, so the damage becomes
permanent. Phoenix's own cooked config quotes every URL. With quotes, the values survive the round trip. Our 1.4.4 `Game.ini`
has been rewritten by the game after many sessions, and all 167 values are still intact. We have not
tested unquoted values on 1.4.4. There is no reason to.

**Changing the port later.** Regenerate the file with `-Metagame 127.0.0.1:<new port>` instead of
searching and replacing. When we moved from port 60000 to 61000, our first search-and-replace matched
`:60000/` and missed the two bare-host keys, which would have kept matchmaking and telemetry on the
old port. A search for leftover `60000` strings caught them.

Quick health check:

```powershell
$G = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient\Game.ini"
$e = Get-Content $G | Select-Object -Skip 1 | Where-Object { $_ }
"entries: $($e.Count)  not fully quoted: $(@($e | Where-Object { $_ -notmatch '^[A-Za-z0-9_]+="[^"]*"$' }).Count)  not on 127.0.0.1:61000: $(@($e | Where-Object { $_ -notmatch '"http://127\.0\.0\.1:61000' }).Count)"
# entries: 167  not fully quoted: 0  not on 127.0.0.1:61000: 0
```

A server-side version of the endpoint hook would remove the need for this file. It is on the
[roadmap]({{ roadmap_page.url | relative_url }}).

---

## 9. Metagame {#metagame}

**Install and build.** `dist/` is git-ignored, so you always build it yourself:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
npm ci --no-audit --no-fund
npm run build
Test-Path node_modules\better-sqlite3\build\Release\better_sqlite3.node   # must be True
```

npm 11 prints `npm warn allow-scripts` lines for `esbuild` and `better-sqlite3`. Our build worked
anyway. See [Troubleshooting]({{ trouble_page.url | relative_url }}#npm-allow-scripts-warnings) if the last line prints
`False`.

**Write `.env`.** These are the values we run with. None of them is secret; the signing keys are
appended in the next step.

```powershell
@"
PORT=61000
BIND_HOST=127.0.0.1
AUTH_MODE=APIKEY
DB_FILENAME=C:/dr/data/undaunted.db
TARGET_CHANGELIST=239827
QOS_TARGET_URL=http://127.0.0.1:61000/QoS
MATCHMAKING_MODE=DEPLOYSERVER
DEPLOYSERVER_URL=127.0.0.1:61001
REGISTRATION_MODE=OPEN
NODE_ENV=production
"@ | Set-Content C:\dr\undaunted\legacy\UndauntedMetagame\.env -Encoding ascii
```

**Generate the token-signing keys** and append them without printing them. Use `Add-Content -Encoding
ascii`, not `>>`: by default, `>>` in Windows PowerShell 5.1 writes UTF-16, which Node can't read as
a `.env` file.

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
node -e "const c=require('crypto');const k=c.generateKeyPairSync('rsa',{modulusLength:2048,privateKeyEncoding:{type:'pkcs8',format:'pem'},publicKeyEncoding:{type:'spki',format:'pem'}});console.log('AUTH_SIGNING_PRIVKEY_B64='+Buffer.from(k.privateKey).toString('base64'));console.log('AUTH_SIGNING_PUBKEY_B64='+Buffer.from(k.publicKey).toString('base64'))" | Add-Content .env -Encoding ascii
```

| Key | Meaning |
|---|---|
| `PORT` | Metagame HTTP port. We use **61000**. See the port note below. |
| `BIND_HOST` | **Fork only.** Address to listen on. Defaults to `127.0.0.1`. Upstream listened on every interface, which with `REGISTRATION_MODE=OPEN` let anyone who could reach the PC create accounts. Change it only when you follow [Run it for a group]({{ admin_page.url | relative_url }}). |
| `AUTH_MODE` | `APIKEY`: players log in with a per-account key. `NONE` takes whatever the client sends as the user id. It is honoured only outside production; with `NODE_ENV=production` the login request then gets no answer at all and the game waits. Never use it. |
| `AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64` | RSA key pair, PEM, base64. Signs the 24-hour RS256 session tokens. Generate your own; never reuse anyone else's. |
| `DB_FILENAME` | SQLite file. Use forward slashes. `C:\dr\data` must exist. |
| `TARGET_CHANGELIST` | `239827`, the changelist from `Version.txt`. The matchmaking response reports it to the client as build id `239827_1.4.4_shipping`. |
| `QOS_TARGET_URL` | URL the client pings to "choose a region". |
| `MATCHMAKING_MODE`, `DEPLOYSERVER_URL` | `DEPLOYSERVER` with host:port and no scheme: hand matchmaking to the deploy server. |
| `REGISTRATION_MODE` | `OPEN`, `INVITECODE` or `NONE`. `OPEN` is fine while the metagame listens on loopback only. Switch to `INVITECODE` in this file before anyone else can reach it. Changing it through the admin API lasts only until the next restart. |
| `NODE_ENV` | `production`. The logs are then plain JSON lines. |
| `LOG_REQUESTS` | **Fork only**, optional. Every request is logged as `METHOD /path gs=0/1` unless this is `0`. Behind the public-mode gateway the line ends with ` via=gateway ip=<player address>`. It is our main diagnostic. |
| `PROGRESSION_MODE` | **Fork only**, optional. Unset (the default) or `real`: every account keeps its own Slayer level, mastery, Hunt Pass (with the Elite pass for everyone), loadout slots, cooldowns and bounties. `stub`: upstream's fake max ranks, nothing stored. Any other value is logged and treated as real. Updating a server that already has players? Read the [upgrade notes]({{ upgrade_page.url | relative_url }}) first. |
| `PROGRESSION_REAL_ACCOUNTS` | **Fork only**, optional. Only with `PROGRESSION_MODE=stub`: comma-separated account ids that get real progression anyway. |

**Other optional switches (fork only).** Leave them out to get the default. Every variable, with
what an unset value means and who sets it, is on [Configuration]({{ config_page.url | relative_url }}#metagame).

| Key | Default | What it does |
|---|---|---|
| `CHAT` | off | `1` turns on the game's text chat: the metagame also listens on `127.0.0.1:61099`, where step 7 points the game's chat connection, and logs `chat: listening on 127.0.0.1:61099 (nick check enforce)` at startup. Off by default until the two-player test on the rented server passes. If the port is taken, the metagame logs one error line and runs without chat. Details: [Configuration]({{ config_page.url | relative_url }}#metagame-chat). |
| `ENTITLEMENTS_DEFAULT` | `season09b_premium,season_premium_any,season_free_any` | The entitlements every account owns. The first one is the Elite Hunt Pass. |
| `INVENTORY_REFUSE_OVERSPEND` | off | `1` refuses an inventory transaction that removes more than the player has. Off because a refusal drops the whole transaction, rewards included, and no hunt-end transaction has been checked against it yet. Meanwhile an overspend is clamped at 0 and logged as "Allowing overspend". |
| `DB_WAL` | off | `1` switches the database to WAL mode. Off because the backups described here copy the database file alone, and a hard stop leaves the newest saves in a separate `-wal` file. |
| `LOG_BODIES`, `BODY_LOG_FILE` | off, `bodies.log` | `1` appends the request bodies of unfinished save routes to the file (8 KB each, 64 KB for inventory, tokens removed). A development aid; it records what players send. |
| `MATCHMAKING_CANCEL` | off | `1` answers the client's matchmaking cancel. Off because the client sends a cancel right after every queued join, and hunts only start because that cancel is answered 404. |
| `PROGRESSION_ALLOW_DELETE` | off | `1` lets game servers reset a progression track (a debug command). An admin key can always do it. |
| `PROGRESSION_GRANT_CAP` | `5000` | The most XP one request may add to one track. |
| `SAVE_HISTORY_KEEP`, `SAVE_HISTORY_HOURLY`, `SAVE_HISTORY_DAILY` | `100`, `48`, `30` | Character and loadout versions kept for rollbacks: the newest ones, then one per hour, then one per day. |
| `INVENTORY_REPORT_REMOVALS`, `MISC_ROUTES`, `STATUS_EXTRA`, `ACCOUNT_DISPLAY_NAME` (`0`), `PROGRESSION_CONFIRM` (`off`) | on | Each value in brackets puts back one piece of upstream's old behaviour, for comparisons. Leave them unset. |

**Why ports 61000/61001 and not 60000.** Upstream's launcher uses `127.0.0.1:60000` in development
mode, and our first plan used 60000/60001. On our PC, `127.0.0.1:60000` is already held by
`ShadowUSB`, part of the Shadow client app. Upstream's metagame then printed its success line and
exited, and clients hung on the other program. Check that your ports are free before you pick them:

```powershell
Get-NetTCPConnection -LocalPort 61000,61001 -State Listen -ErrorAction SilentlyContinue |
  ForEach-Object { "{0}:{1} is taken by {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
```

No output means both ports are free. Our fork now exits with an error if it can't bind, instead of
announcing success. See [Troubleshooting]({{ trouble_page.url | relative_url }}#port-60000-is-taken-and-the-metagame-says-clear-skies-anyway).

**First start.** Always start it from its own folder, because the database migrations are found through
a relative path (`./src/drizzle`). The first start creates the database and runs all migrations:

```powershell
$meta = @{ FilePath = "node"; ArgumentList = "--env-file=.env", "dist/server.js"
           WorkingDirectory = "C:\dr\undaunted\legacy/UndauntedMetagame"; PassThru = $true; WindowStyle = "Hidden"
           RedirectStandardOutput = "C:\dr\data\metagame.log"; RedirectStandardError = "C:\dr\data\metagame.err" }
$p = Start-Process @meta; Set-Content C:\dr\data\metagame.pid $p.Id
Start-Sleep 5
Get-Content C:\dr\data\metagame.log | ForEach-Object { ($_ | ConvertFrom-Json).msg }
```

Expected:

```
Registered 0 new Gameserver API Key(s) on boot!
Registered 0 new User API Key(s) on boot!
Dauntless Revived metagame on 127.0.0.1:61000
Progression mode: real for every account (the default)
Clear Skies, Slayer.
```

`-RedirectStandardOutput` starts a new log each time. Copy the old log first if you want to keep it.

**Create the game-server key.** Game servers authenticate to the metagame with a separate key, sent as
the `x-undaunted-gameserver-apikey` header. The metagame stores only a SHA-256 of it. You put the key
into a queue table, and the metagame hashes and registers it at the next start:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
$key = node -e "console.log(require('crypto').randomBytes(24).toString('hex'))"
Set-Content C:\dr\data\gameserver.key -Value $key -Encoding ascii -NoNewline
node -e "new (require('better-sqlite3'))(process.argv[1]).prepare('INSERT INTO gameserverapikeystoregister(key) VALUES (?)').run(process.argv[2])" C:/dr/data/undaunted.db $key
$key = $null
Stop-Process -Id (Get-Content C:\dr\data\metagame.pid)
$p = Start-Process @meta; Set-Content C:\dr\data\metagame.pid $p.Id
Start-Sleep 5
Get-Content C:\dr\data\metagame.log | ForEach-Object { ($_ | ConvertFrom-Json).msg }
# Registered 1 new Gameserver API Key(s) on boot!
```

The plain key now exists only in `C:\dr\data\gameserver.key` and, after step 11, in the deploy
server's `.env`. Game servers receive it as their first command-line argument.

---

## 10. Create the admin account {#admin-account}

Accounts are created through `POST /undaunted/api/Register`. The response contains the account key.
**That key is the password.** The server keeps only its SHA-256, and there is no recovery tool yet,
so if you lose the key you lose the account. Admin rights are a flag in the database.

We use Node's `fetch` with a timeout here, not `Invoke-RestMethod` (see
[Troubleshooting]({{ trouble_page.url | relative_url }}#invoke-restmethod-hangs)). Run this from the metagame folder so
`better-sqlite3` resolves, and replace `YourName`:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
node -e "(async () => { const name = process.argv[1]; const base = 'http://127.0.0.1:61000/undaunted/api'; const r = await fetch(base + '/Register', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ Username: name }), signal: AbortSignal.timeout(15000) }); if (!r.ok) throw new Error('Register returned HTTP ' + r.status); const { UUK } = await r.json(); require('fs').writeFileSync('C:/dr/data/owner.key', UUK); const info = await (await fetch(base + '/GetUserInfo', { headers: { 'x-undaunted-user-api-key': UUK }, signal: AbortSignal.timeout(15000) })).json(); new (require('better-sqlite3'))('C:/dr/data/undaunted.db').prepare('UPDATE users SET isAdmin = 1 WHERE userId = ?').run(info.UserId); console.log('created ' + name + ' as admin; key saved to C:/dr/data/owner.key (not shown)'); })().catch(e => { console.error(e.message); process.exit(1); })" YourName
```

Notes:

- The name must be 3-16 letters, digits or underscores, and unique regardless of case. Otherwise
  `Register` answers 400 (`username_invalid`) or 409 (`username_taken`) and the script stops. An admin
  can rename an account later with `RenameUser`.
- The admin API (invite codes, registration mode, renames, online stats) takes this key in the
  `x-undaunted-user-api-key` header, and only directly, never through a proxy.
  [Run it for a group]({{ admin_page.url | relative_url }}) covers it, and
  [HTTP API]({{ api_page.url | relative_url }}) lists every route.
- Keep `owner.key` out of the repository, screenshots and chat. Back it up together with the two
  `.env` files and `gameserver.key`, encrypted and separately from the database.

---

## 11. Deploy server {#deploy-server}

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedDeployServer
npm ci --no-audit --no-fund
npm run build
$key = (Get-Content C:\dr\data\gameserver.key -Raw).Trim()
@"
PORT=61001
BIND_HOST=127.0.0.1
MY_IP=127.0.0.1
PORT_RANGE_BEGIN=8770
PORT_RANGE_END=8777
GAMESERVER_BINARY_PATH=C:/D144/Dauntless/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe
METAGAME_API_KEY=$key
SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP=10
ENABLE_DOJO=0
NODE_ENV=production
"@ | Set-Content .env -Encoding ascii
$key = $null
```

| Key | Meaning |
|---|---|
| `PORT` | 61001. Only the metagame on the same PC talks to it. |
| `BIND_HOST` | **Fork only.** Defaults to `127.0.0.1`. Keep it there. The deploy server has no authentication: whoever can call it starts game processes on your PC. As a second check it answers 403 to any caller that is not on this PC. |
| `MY_IP` | The address handed to clients for game servers. `127.0.0.1` for local play. |
| `PORT_RANGE_BEGIN`, `PORT_RANGE_END` | UDP ports for game servers. Ramsgate always takes `END` (8777) and the Dojo `END-1` (8776). Hunts use the rest (8770-8775, so six at a time). |
| `GAMESERVER_BINARY_PATH` | The 1.4.4 exe, forward slashes. |
| `METAGAME_API_KEY` | The game-server key from step 9. |
| `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP` | Gap between server launches. |
| `ENABLE_DOJO` | **Fork only.** `0` starts the Training Dojo the first time someone enters it. `1` restores upstream behaviour (start it at boot). |

Keep the port layout. The server DLL disables its idle shutdown for any port **8776 or higher**. A hunt
server exits after 50 seconds in total with nobody connected, and only below 8776. To allow more
simultaneous hunts, lower `PORT_RANGE_BEGIN` and keep `PORT_RANGE_END=8777`.

Start it:

```powershell
$dep = @{ FilePath = "node"; ArgumentList = "--env-file=.env", "dist/server.js"
          WorkingDirectory = "C:\dr\undaunted\legacy/UndauntedDeployServer"; PassThru = $true; WindowStyle = "Hidden"
          RedirectStandardOutput = "C:\dr\data\deploy.log"; RedirectStandardError = "C:\dr\data\deploy.err" }
$p = Start-Process @dep; Set-Content C:\dr\data\deploy.pid $p.Id
```

It starts the permanent Ramsgate server straight away. Every game server it starts is a normal game
process with its own console. Ramsgate and the Dojo show theirs as a **console window** ("Running as
a server!"). **Don't close those windows.** Closing one kills that server for everyone on it. Hunt
servers are started with their window hidden, so a new window doesn't flash up for every hunt.

To test a game server on its own without the deploy server, start one by hand with the same
arguments the deploy server uses:

```powershell
$W = "C:\D144\Dauntless\Archon\Binaries\Win64"
$key = (Get-Content C:\dr\data\gameserver.key -Raw).Trim()
Start-Process "$W\Dauntless-Win64-Shipping.exe" -WorkingDirectory $W -ArgumentList @(
  $key, "8777", "/Game/Maps/ramsgate/ramsgate_01_persistent",
  "NO_BEHEMOTH", "NO_MM_HUNTID", "NO_EXPECTED_PLAYERS", "127.0.0.1:8777",
  "-EpicPortal", "-server", "-nullrhi")
```

The positional arguments are: key, UDP port, map, behemoth, matchmaker hunt id, expected players, and
the advertised address. With fewer than eight arguments after the exe name, the DLL shows an
"INVALID GAMESERVER ARGS" message box and exits. Stop a hand-started server before you start the deploy server, or they will
fight over port 8777.

---

## 12. First boot checks {#first-boot-checks}

With the metagame and deploy server running:

```powershell
# 1. both node processes listen on loopback only
Get-NetTCPConnection -LocalPort 61000,61001 -State Listen | ForEach-Object { "TCP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
#    expect: TCP 127.0.0.1:61000 node / TCP 127.0.0.1:61001 node

# 2. the metagame answers HTTP (curl.exe ships with Windows 10)
curl.exe -s -m 5 http://127.0.0.1:61000/dauntless-status
#    expect JSON starting {"show-status":true,...

# 3. Ramsgate is up: about 10 s after the deploy server starts, UDP 8777 is bound by the game
Get-NetUDPEndpoint -LocalPort 8777 | ForEach-Object { "UDP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
#    expect: UDP 0.0.0.0:8777 Dauntless-Win64-Shipping

# 4. which game processes are servers (the client is the same exe)
Get-CimInstance Win32_Process -Filter "Name='Dauntless-Win64-Shipping.exe'" | ForEach-Object { "{0,6}  server={1}  {2} MB" -f $_.ProcessId, ($_.CommandLine -match ' -server'), [int]($_.WorkingSetSize/1MB) }

# 5. the deploy server log
Get-Content C:\dr\data\deploy.log | ForEach-Object { try { ($_ | ConvertFrom-Json).msg } catch { $_ } }
#    expect: Dauntless Revived deploy server on port 61001 / Clear Skies, Slayer. / Running Gameserver Watchdog! (every 60 s)
```

The Ramsgate server should settle at about 1.1 GB and 0.2 of a core. Game servers bind UDP on all
interfaces (`0.0.0.0`). On our PC, Windows Firewall blocks inbound traffic on every profile and had
no allow rule for the game or for Node, so nothing outside the PC could reach them. If you have ever
clicked "Allow" in a firewall prompt for these programs, check your rules. For local play you need
no rules at all: traffic to `127.0.0.1` is loopback. If Windows asks whether to allow `node.exe` or
the game on networks, you can decline for a local-only setup.

**No hosts-file entries, certificates or trust-store changes are needed for 1.4.4.** Every endpoint is
plain HTTP to the metagame. (Our 2.1.1 work did use hosts entries and a private CA. None of that is
part of this setup.) We have only run this on a PC that still had those old hosts entries. We believe
they are unused, but we have not verified this on a clean machine.

---

## 13. Launch the client {#launch-the-client}

We launch with `C:\dr\tools\play.ps1`. It writes the graphics and chat settings (steps 7 and 14),
reads the account key from `C:\dr\data\owner.key` (or the file you name with `-KeyFile`) without
printing it, starts the game, and can optionally watch its memory. Here it is in full:

```powershell
# Launch the 1.4.4 client against our own Dauntless Revived backend.
#   -Graphics 4     FORCE this quality level on every launch (4 = Cinematic = max, 3 = Epic).
#   -Graphics -1    don't force anything; use whatever you pick in the in-game menu.
#   -Windowed       1280x720 window instead of your saved display mode.
#   -KeyFile        account key to log in with (default: the owner's key).
param([string]$Backend = "127.0.0.1:61000", [int]$Graphics = 4, [switch]$Windowed,
      [int]$CapMB = 12000, [int]$Seconds = 0, [string]$KeyFile = "C:\dr\data\owner.key")

$U = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient"

# Engine-level settings. [SystemSettings] in the user Engine.ini overrides the
# menu, so this is where "force" lives. The texture pool is bounded by the GPU's
# VRAM and the two gc lines only affect memory cleanup, not image quality.
$sys = @("[SystemSettings]",
         "r.Streaming.PoolSize=3000", "r.Streaming.LimitPoolSizeToVRAM=1",
         "gc.TimeBetweenPurgingPendingKillObjects=10", "s.ForceGCAfterLevelStreamedOut=1")
$groups = "ViewDistance","AntiAliasing","Shadow","PostProcess","Texture","Effects","Foliage","Shading"
if ($Graphics -ge 0) {
  $sys += ($groups | ForEach-Object { "sg.${_}Quality=$Graphics" })
  $sys += @(
    "sg.ResolutionQuality=100",
    "r.ScreenPercentage=100",      # render at full native resolution
    "r.MipMapLODBias=0",           # full-resolution texture mips
    "r.MaxAnisotropy=16",          # sharp textures on floors and walls at an angle
    "r.Tonemapper.Sharpen=0.6"     # counter temporal-AA softness (the main UE4 blur)
  )
}
# Chat/presence (XMPP): 1.4.4 ships pointed at Epic's live server
# (wss://xmpp-service-prod.ol.epicgames.com:443) and keeps reconnecting to it,
# sending the account id and our login token. Point it at this PC instead.
# With CHAT=1 in the metagame's .env the metagame's chat listens on 61099;
# without it the connection fails harmlessly, exactly as it does against
# Epic (the game tolerates that). URLs MUST be quoted in a user ini.
$xmpp = @("[OnlineSubsystemMcp.XMPP]", 'ServerAddr="ws://127.0.0.1"', "ServerPort=61099", "bUseSSL=false")

$eng = "$U\Engine.ini"
$lines = if (Test-Path $eng) { Get-Content $eng } else { @() }
$keep = New-Object System.Collections.Generic.List[string]; $skip = $false
foreach ($l in $lines) {
  if ($l -match '^\[(SystemSettings|OnlineSubsystemMcp\.XMPP)\]') { $skip = $true; continue }
  if ($skip -and $l -match '^\[') { $skip = $false }
  if (-not $skip) { $keep.Add($l) }
}
Set-Content $eng -Encoding ASCII -Value ($sys + "" + $xmpp + "" + $keep)

# Mirror the level into the menu settings so the options screen shows it too.
$gus = "$U\GameUserSettings.ini"
if ($Graphics -ge 0 -and (Test-Path $gus)) {
  $g = Get-Content $gus
  foreach ($k in $groups) { $g = $g -replace "^sg\.${k}Quality=.*", "sg.${k}Quality=$Graphics" }
  $g = $g -replace '^sg\.ResolutionQuality=.*', 'sg.ResolutionQuality=100.000000'
  Set-Content $gus -Encoding ASCII -Value $g
}
if ($Graphics -ge 0) { "graphics FORCED to level $Graphics (4 = Cinematic/max), native res, sharpened" }
else { "graphics: using your in-game menu choice" }

$W   = "C:\D144\Dauntless\Archon\Binaries\Win64"
$UUK = (Get-Content $KeyFile -Raw).Trim()      # your account key; never printed
$a = @($Backend, "-AUTH_PASSWORD=$UUK", "-AUTH_LOGIN=unused", "-AUTH_TYPE=exchangecode",
  "-epicapp=appidlol", "-epicenv=Prod", "-EpicPortal", "-epicusername=usernamelol",
  "-epicuserid=useridlol", "-epiclocale=en-US", "-epicsandboxid=sandboxidlol",
  "-epicdeploymentid=deploymentidlol")
if ($Windowed) { $a += @("-windowed", "-ResX=1280", "-ResY=720") }
$cl = Start-Process "$W\Dauntless-Win64-Shipping.exe" -WorkingDirectory $W -PassThru -ArgumentList $a
Set-Content C:\dr\data\client.pid -Value $cl.Id
"client pid=$($cl.Id) backend=$Backend"
if ($Seconds -le 0) { return }

$peak = 0; $t = 0
while ($t -lt $Seconds) {
  Start-Sleep -Seconds 5; $t += 5
  $p = Get-Process -Id $cl.Id -ErrorAction SilentlyContinue
  if (-not $p) { "client EXITED at ${t}s (peak ${peak}MB)"; return }
  $mb = [int]($p.WorkingSet64 / 1MB); if ($mb -gt $peak) { $peak = $mb }
  if ($mb -gt $CapMB) { "CAP HIT at ${t}s: ${mb}MB - killing client"; Stop-Process -Id $cl.Id -Force; return }
  if ($t % 30 -eq 0) { "  t=${t}s  client RAM ${mb}MB" }
}
"client running, peak ${peak}MB over ${Seconds}s"
```

Run it without changing the machine's execution policy:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\play.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\play.ps1 -Graphics 3 -Windowed -Seconds 180   # Epic, windowed, watch RAM for 3 min
```

What the arguments mean:

- The **first argument** (`127.0.0.1:61000`, no scheme) is read by the client-mode DLL as the metagame
  address.
- `-AUTH_TYPE=exchangecode` with `-AUTH_PASSWORD=<account key>` makes the game post the key to
  `POST /account/api/oauth/token` as an exchange code. The metagame checks it and returns a 24-hour
  token.
- The `...lol` values are placeholders. Undaunted's launcher passes exactly these. No Epic account is
  involved, because 1.4.4 predates Epic Online Services.
- Traffic is plain HTTP, and the key is visible in the process command line to programs running as
  you. On loopback that is fine. Over a network, use a VPN ([Run it for a group]({{ admin_page.url | relative_url }})).

**What you should see in the metagame log** (`gs=0` is the client, `gs=1` a game server). In order,
trimmed:

```
POST /account/api/oauth/token gs=0      -> Logging in <account id>!
GET  /dauntless-status gs=0
POST /login gs=0                        -> <account id> is logging in!
PUT  /gamesession/epic gs=0
GET  /accountinfo gs=0
GET  /character gs=0                    (the first character is created automatically)
POST /candidate/player/register gs=0
GET  /candidate/regions gs=0, then several GET /QoS
POST /candidate/join gs=0               -> Querying DeployServer for GameMode: ISLAND ... (new character's tutorial)
                                           or GameMode: CITY ... (Ramsgate)
GET /character gs=1, ..., POST /character gs=1    (the game server loading and saving you)
```

A new character goes to the tutorial island (`/Game/Maps/islands/1705/dia_moss_triforce`) on a hunt
server the deploy server starts (on our PC, UDP 8775), and from there to Ramsgate on 8777. The client
also opens a console window. Leave it open.

Some warnings in this log are expected. See
[Troubleshooting]({{ trouble_page.url | relative_url }}#log-lines-that-look-alarming-but-are-known).

---

## 14. Graphics {#graphics}

`play.ps1 -Graphics <n>` forces one Unreal scalability level on every launch:

| `-Graphics` | Level |
|---|---|
| `4` (default) | Cinematic, the highest level in this UE4 build |
| `3` | Epic |
| `2` | High |
| `1` | Medium |
| `0` | Low |
| `-1` | Force nothing; use what you pick in the in-game menu |

With any level of 0 or higher, the script also forces native resolution (`r.ScreenPercentage=100`),
full-resolution mips, 16x anisotropic filtering and a light sharpen. It mirrors the level into
`GameUserSettings.ini`, so the options screen matches. `-Windowed` gives a 1280x720 window.

At Cinematic and 1920x1080 on an RX 6600-class GPU, the client uses 1.9-2.3 GB of RAM. The memory
lines in `[SystemSettings]` stay on at every level as a precaution. They come from our 2.1.1 work,
where an uncapped client reached 9 GB. We have not measured 1.4.4 without them. See
[Troubleshooting]({{ trouble_page.url | relative_url }}#memory-spikes-and-caps).

---

## 15. Stopping {#stopping}

The order matters:

1. The deploy server's watchdog restarts a dead Ramsgate within about a minute, and so does a player's
   trip to Ramsgate, so stop the deploy server before the game servers.
2. The game servers it started normally end together with it. It uses Node's default (not detached)
   `spawn`, and on Windows Node puts such children in a job object that is closed when Node exits. We
   confirmed this with a test process, for both a normal exit and a forced kill. A server you started
   by hand (step 11) is not covered and keeps running, so the command below still sweeps up any
   leftover `-server` process.
3. The client is the same exe as the servers, so only stop processes whose command line contains
   `-server`. Don't print those command lines: the first argument is the game-server key.

```powershell
# 1. quit the game from its menu, then:
Stop-Process -Id (Get-Content C:\dr\data\deploy.pid)
Get-CimInstance Win32_Process -Filter "Name='Dauntless-Win64-Shipping.exe'" |
  Where-Object { $_.CommandLine -match ' -server' } | ForEach-Object { Stop-Process -Id $_.ProcessId }
Stop-Process -Id (Get-Content C:\dr\data\metagame.pid)
```

While everything is stopped, copy `C:\dr\data\undaunted.db` somewhere safe. It holds every account
and character. [Run it for a group]({{ admin_page.url | relative_url }}) has a proper backup routine.

---

## Start everything: one-page checklist {#checklist}

Once steps 1-12 are done, this is the whole routine.

| # | Do | Check |
|---|---|---|
| 1 | Close heavy programs. If you use WSL, run `wsl --shutdown`. | Several GB of RAM free |
| 2 | Make sure nothing old is running: no `node` on 61000/61001, no `-server` game processes. | Step 12, commands 1 and 4 show nothing |
| 3 | `Game.ini` is intact. | Step 8 health check: `167 / 0 / 0` |
| 4 | Start the metagame from `C:\dr\undaunted\legacy/UndauntedMetagame` (`Start-Process @meta`, step 9). | Log: `Dauntless Revived metagame on 127.0.0.1:61000` |
| 5 | Start the deploy server from `C:\dr\undaunted\legacy/UndauntedDeployServer` (`Start-Process @dep`, step 11). | Log: `Dauntless Revived deploy server on port 61001` |
| 6 | Wait for Ramsgate. | A server console window opens; UDP 8777 bound by `Dauntless-Win64-Shipping` |
| 7 | `powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\play.ps1` | Metagame log shows `POST /account/api/oauth/token`, then `POST /login` |
| 8 | Play. Don't close any console window. | `gs=1` lines appear when a server loads you |
| 9 | Stop in order: game, deploy server, `-server` processes, metagame. | Step 15 |
| 10 | Back up `C:\dr\data\undaunted.db`. | A dated copy exists |

If something doesn't match, see [Troubleshooting]({{ trouble_page.url | relative_url }}).
