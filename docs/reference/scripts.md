---
title: Scripts and parameters
parent: Reference
nav_order: 6
description: "Every script you run for Dauntless Revived: the Windows server kit, the friend kit, tools/ and the npm scripts, with each parameter, its default and examples."
lang: en
ref: reference/scripts
---

{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign ports_page = site.pages | where: "path", "reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign game_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign dev_page = site.pages | where: "path", "reference/development.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}
{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}

# Scripts and parameters
{: .no_toc }

This page lists every script in the repository that a person runs, with each parameter, its type,
its default and an example:

- the Windows server kit in `deploy/windows-server/`,
- the friend kit in `friend-kit/`,
- the maintenance scripts in `tools/`,
- the CI workflows in `.github/workflows/` and their tools in `tools/ci/`,
- the npm scripts of every package.

It is a lookup table. The step-by-step guides are
[Windows server kit]({{ winserver_page.url | relative_url }}),
[Host a server]({{ host_page.url | relative_url }}),
[Run it for a group]({{ admin_page.url | relative_url }}) and
[Join as a friend]({{ friends_page.url | relative_url }}). What the scripts write is described
elsewhere in this section: the `.env` keys in [Configuration]({{ config_page.url | relative_url }}),
the ports in [Ports and network]({{ ports_page.url | relative_url }}), the routes the scripts call in
[HTTP API]({{ api_page.url | relative_url }}), and the files and folders in
[Files and data]({{ files_page.url | relative_url }}).

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Before you run a kit script

These rules apply to every script in `deploy/windows-server/`.

- **Windows PowerShell 5.1.** The kit is written for `powershell.exe`, which every supported Windows
  has. If the execution policy blocks scripts, run them the way the kit runs its own scripts:
  `powershell -NoProfile -ExecutionPolicy Bypass -File <script> <arguments>`.
- **Where the scripts live.** On an installed server, run them from `<root>\bin`
  (`C:\DauntlessRevived\bin` by default). On this page `<root>` means the install root. The
  installer copies the kit there, and
  `Update-DauntlessServer.ps1` refreshes that copy from the new code. `staging\kit` is only the upload
  folder that `Deploy-Remote.ps1` uses.
- **`-Root`.** `Stack.ps1`, `Update-DauntlessServer.ps1`, `Backup-DauntlessServer.ps1`,
  `New-Invite.ps1`, `Get-ServerStatus.ps1`, `Set-Chat.ps1` and `Write-PerformanceLog.ps1` take `-Root <install root>`. Without it they use the
  folder above their own folder if that folder holds `data\config\server.json`, which is the case for
  `<root>\bin`. Otherwise they use `C:\DauntlessRevived`. The installer takes `-InstallRoot` instead.
- **`-WhatIf` and `-Confirm`** work on `Deploy-Remote.ps1`, `Install-DauntlessServer.ps1`,
  `Update-DauntlessServer.ps1`, `Stack.ps1`, `Backup-DauntlessServer.ps1` and `New-Invite.ps1`.
  `-WhatIf` lists the changes without making them. `Get-ServerStatus.ps1` changes nothing and has no
  `-WhatIf`.
- **Exit codes.** `0` means success and `1` means failure. A failure prints a red `FAIL` line, and
  the error the script throws starts with `DRFAIL:`. `Stack.ps1` is the exception: it returns `1`
  only when it fails outright. A component that does not start is reported in its output.
- **Machine-readable lines.** The installer prints `DRFINGERPRINT=<64 hex>` in public mode. The
  upload helper and `Deploy-Remote.ps1`'s checks on the server print one `DRJSON:{...}` line each.
- **Nothing secret is printed.** No kit script prints a key, a token, a password or a `.env` value.
- **Lists.** `-AdminIp` (installer and `Deploy-Remote.ps1`) takes either an array or one
  comma-separated string: `powershell -File` passes `a,b` as one string, and those two scripts split
  it. `Stack.ps1 -Only` does not split such a string, so under `powershell -File` name one component
  per call.

Which scripts need an elevated PowerShell ("Run as administrator"):

| Script | Elevated? |
|:-------|:----------|
| `Install-DauntlessServer.ps1` | Yes. With `-Sandbox` it only warns. |
| `Update-DauntlessServer.ps1` | Yes, except on a `-Sandbox` install. |
| `Stack.ps1` | `start`, `stop` and `restart` on an installed server work through the scheduled tasks and need it, and so does `-Direct`. `status` runs without it, but it can show the players online only when it can read the owner key, which needs elevation. |
| `New-Invite.ps1` | Yes: it reads `data\keys\owner.key`. |
| `Set-Chat.ps1` | Yes, except on a `-Sandbox` install: it rewrites `metagame.env` and restarts the stack. |
| `Get-ServerStatus.ps1` | On the server, if you want the player list: it reads the owner key. Not needed with `-KeyFile`. |
| `Backup-DauntlessServer.ps1` | Run it elevated, or let the backup task run it as the service account. An account that cannot read a file skips it and names it in the output. |
| `Write-PerformanceLog.ps1` | On an installed server, yes: without elevation it cannot see the game servers (they run as the service account) or read the owner key for the player counts. Not on a `-Sandbox` install. |
| `Deploy-Remote.ps1` | No. It runs on your PC as your normal account. The SSH account on the server must be an administrator. |

## Windows server kit

| Script | Runs on | What it does |
|:-------|:--------|:-------------|
| `Deploy-Remote.ps1` | Your PC | Uploads the kit, the code, a backup and the game zip over key-only SSH, then runs the installer or the updater. Also makes invites and shows the status. |
| `Install-DauntlessServer.ps1` | The server | Installs, repairs or restores a server in public or private mode. Idempotent. |
| `Update-DauntlessServer.ps1` | The server | New server code, built beside the running server, with a health check and an automatic rollback. |
| `Stack.ps1` | The server | `status`, `start`, `stop`, `restart`, and `supervise` (inside the scheduled tasks). |
| `New-Invite.ps1` | The server | Creates an invite code and prints the invite line. Also lists and revokes codes. |
| `Get-ServerStatus.ps1` | The server or any PC | Who is online and which worlds and hunts run. |
| `Backup-DauntlessServer.ps1` | The server | A backup now, with retention. |
| `Set-Chat.ps1` | The server | Turns the in-game text chat on or off (public mode), and restarts the stack. |
| `backup-hidden.vbs` | The server | Runs the backup without a window (used by the hourly task). |
| `Write-PerformanceLog.ps1` | The server | A performance sample now, or every minute in a sandbox. The stack supervisor already takes one every minute on an installed server. |
| `Receive-Upload.ps1` | The server | The server side of the chunked upload. `Deploy-Remote.ps1` calls it; you do not. |
| `DauntlessServer.Common.ps1`, `DauntlessServer.Performance.ps1` | Both | Shared helpers the scripts dot-source: pins, invites, pinned HTTPS, processes; the performance sampler. |
| `lib\dr-db.js`, `lib\dr-keys.js`, `lib\verify-game.js` | The server | Node helpers the scripts call. |
| `tests\Test-*.ps1` | A development PC | The kit's own tests. |

### Deploy-Remote.ps1

Runs on your own PC, from a checkout of the repository:
`.\deploy\windows-server\Deploy-Remote.ps1`. It needs the Windows OpenSSH client (`ssh.exe` and
`scp.exe` in `System32\OpenSSH`) and, to upload your code, `git`. The guide is
[Deploy from your PC]({{ winserver_page.url | relative_url }}#deploy-from-your-pc).

It connects with a key file only (`BatchMode`, no password or keyboard prompts) and pins the
server's host key in a `known_hosts` file. Commands run on the server as
`powershell -EncodedCommand`. Without an action switch it installs:

1. It connects and checks that the SSH session is elevated. Then it uploads the kit to
   `<InstallRoot>\staging\kit`. The `tests` folder is not uploaded.
2. It uploads the code as `staging\source\source-<12 hex>.zip`. That is a `git archive` of `HEAD`,
   or of the working tree with `-WorkingTree`. With `-Source GitHub` nothing is uploaded, and the
   server downloads the ref itself.
3. With `-RestoreFrom`, it uploads the backup folder to `staging\restore\<folder name>`. Only
   Administrators and SYSTEM can open that folder, and it is deleted after a successful install.
4. With `-GameZip`, it uploads the game zip to `staging\upload` in parts. The finished
   `BaseGame144.zip` stays there for repairs.
5. It runs `Install-DauntlessServer.ps1` and streams its output.
6. It prints the result. In public mode it also checks the gateway from your PC, with the pinned
   certificate, and reminds you to allow inbound TCP on the gateway port and UDP 8770-8777 from any
   address in the provider's firewall. It cannot test UDP from your PC.

**Connection**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Server` | IPv4 address or DNS name, **required** | | The SSH target. In a public install it is also the default `-PublicHost`. |
| `-User` | account name: letters, digits, `.`, `_`, `-` (1-64) | `Administrator` | The SSH account. The session must be elevated, so use an administrator. |
| `-SshPort` | 1-65535 | `22` | The SSH port. |
| `-KeyFile` | path | `C:\dr\data\ssh\dauntless_deploy` | Your SSH private key. The default is the original host's path; pass your own. **Secret: never share, never commit.** The script warns if Everyone, Users or Authenticated Users can read the file, because `ssh` then refuses it. |
| `-KnownHostsFile` | path without spaces | `known_hosts` in the folder of `-KeyFile` | Where the server's host key is pinned. The first connection records it (`StrictHostKeyChecking=accept-new`), and every later one must match. |
| `-HostKeyFingerprint` | `SHA256:` and 43 base64 characters (the prefix may be left out) | none | For the first connection. The script fetches the server's host keys with `ssh-keyscan`, requires one with this fingerprint and pins it. Without it, a first connection that also has `-RestoreFrom` is refused, because the backup holds every key. Take the fingerprint from the provider's console. |

**What to install** (passed on to `Install-DauntlessServer.ps1`)

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Mode` | `Public` or `Private` | `Public` | The server's mode. Always passed on an install run: see "Re-deploying" below. |
| `-InstallRoot` | absolute path without spaces | `C:\DauntlessRevived` | The install root on the server. |
| `-OwnerName` | 3-16 letters, digits or `_` | none | The admin account of a new server. An install needs `-OwnerName` or `-RestoreFrom`, unless you pass `-UploadOnly`. |
| `-RestoreFrom` | a local backup folder with a plain name, such as `2026-10-01_200000` | none | Moves an existing server: the database and every key. The folder must hold `undaunted.db`, `secrets\metagame.env` and `secrets\deployserver.env`. After the upload the file count and the database hash are compared. **Secret: the folder holds every key; never share, never commit.** It travels only inside the SSH connection. |
| `-ServerName` | 1-64 characters: no line breaks, control characters, double quotes, backslashes or direction-override characters, and no spaces at either end (the installer's rule) | not passed | The name friends see. When it is left out, the installer uses its own default, `Dauntless Revived`. |
| `-PublicHost` | IPv4 address or DNS name | `-Server` | Public mode: the address friends connect to. Always passed in public mode. |
| `-GatewayPort` | 1-65535 | `443` | Public mode: the gateway's TCP port. Always passed in public mode. |
| `-AdminIp` | IPv4 address or `IPv4/prefix` with a prefix from 8 to 32; one or more | none | The addresses allowed to use Remote Desktop. Without it and without `-KeepRdpOpen`, the script warns that the installer turns internet-open Remote Desktop rules off. |
| `-KeepRdpOpen` | switch | off | Leave internet-open Remote Desktop rules as they are. |
| `-InteractiveSession` | switch | off | The session-0 fallback of the installer. |
| `-Chat` | `On` or `Off` | not passed | Passed on as the installer's `-Chat`. Public mode only for `On`. On its own, without anything to install, it is an action instead: see below. |
| `-GameZip` | path of the local zip | none | Uploads the game zip in resumable parts. The zip is hashed on your PC first and must match the pinned SHA-256. Each part is checked on the server, with up to 3 tries, and the whole file again after assembly, with up to 4 rounds. Run the same command again to resume an interrupted upload. |
| `-GameZipUrl` | `https://` URL | none | Lets the server download the zip itself. Use either `-GameZip` or `-GameZipUrl`. |
| `-ChunkSizeMB` | 1-2048 | `256` | Part size of the game-zip upload. |

**Which code**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Source` | `Upload` or `GitHub` | `Upload` | `Upload` sends your checkout's code. `GitHub` makes the server download `-Ref` from the repository. |
| `-Ref` | branch, tag or commit | `friends-v1` (`$DRRepo.PinnedRef`) | The ref for `-Source GitHub`. |
| `-WorkingTree` | switch | off | Sends your current files instead of `HEAD`: tracked and untracked files that git does not ignore, minus the folders `node_modules`, `dist`, `build`, `out` and `.vite`, every `.env` and `.env.*` file except `.env.example`, and `*.key`, `*.pem`, `*.db`, `*.db-journal`, `*.db-wal`, `*.db-shm` and `*.log`. The commit is recorded as `<commit>-worktree`. Cannot be combined with `-Source GitHub`. |

In public mode an upload of `HEAD` is refused when `HEAD` lacks `legacy/UndauntedGateway` or
`legacy/UndauntedContent`. The check runs after the kit upload, before the code, the backup and the game
zip are sent. Commit your work, or add `-WorkingTree`. Uncommitted changes are not deployed without
`-WorkingTree`, and the script warns about them.

**Actions** (use at most one of `-Update`, `-Status` and `-InviteFor`)

| Parameter | Type | What it does |
|:----------|:-----|:-------------|
| (none) | | Install: uploads everything and runs `Install-DauntlessServer.ps1`. |
| `-Update` | switch | Uploads the kit and the code, then runs `Update-DauntlessServer.ps1 -Root <InstallRoot>` with the uploaded zip, or with `-Ref` for `-Source GitHub`. `-GameZip`, `-GameZipUrl`, `-RestoreFrom` and `-OwnerName` are refused. |
| `-Status` | switch | Runs `<InstallRoot>\bin\Stack.ps1 status` and `Get-ServerStatus.ps1` on the server. The exit code is theirs. |
| `-InviteFor` | text without control characters, quotes, backticks or `$` | Runs `New-Invite.ps1 -For <text>` on the server, prints the invite line, then checks it from your PC with `Get-ServerStatus.ps1 -Invite`. |
| `-Chat` alone | `On` or `Off` | With no install parameter (`-OwnerName`, `-RestoreFrom`, `-GameZip`, `-GameZipUrl`, `-ServerName`, `-AdminIp`, `-Ref`, `-WorkingTree`, `-UploadOnly`), runs `<InstallRoot>\bin\Set-Chat.ps1 -On` or `-Off` on the server and uploads nothing. The stack restarts, so do it when nobody is playing. Refused together with `-Update`, `-Status` or `-InviteFor`. |
| `-UploadOnly` | switch | Uploads everything, prints the command that would run on the server (the installer, or the updater with `-Update`), and stops. |

**Tests only**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-TestTargetDir` | folder | none | A local folder stands in for the server: no SSH, and nothing is installed. The other test options are refused without it. |
| `-TestZipSha256` | 64 hex characters | the pinned zip hash | The hash the zip must have, so a test can use a small file. |
| `-TestStopAfterChunks` | number | `0` (off) | Simulates a dropped connection after that many parts. |
| `-TestCorruptChunk` | part number, counted from 0 | `-1` (off) | Damages that part in transit once. |

**Re-deploying an existing server.** An install run passes these on every time, so pass them again:

- `-Mode` is always passed, with `Public` as the default. Re-deploying a private-mode server without
  `-Mode Private` turns it into a public-mode server.
- In public mode `-PublicHost` (default: `-Server`) and `-GatewayPort` (default `443`) are always
  passed. They replace what the server has stored.
- `-ServerName` is passed only when you give it, and the installer's default is
  `Dauntless Revived`. Re-deploying without it renames the server.
- `-InteractiveSession` is not remembered. Re-deploying without it turns the auto-logon off again.
- `-Chat` is remembered in `server.json`: re-deploying without it keeps chat as it was.

For code changes, use `-Update`. It does not run the installer, so none of this applies.

```powershell
# New public server: an owner account, the game zip from this PC, Remote Desktop only from your address
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -HostKeyFingerprint SHA256:... `
    -OwnerName Slayer -GameZip D:\BaseGame144.zip -AdminIp 198.51.100.20

# Move an existing server; the new server downloads the game zip itself
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -HostKeyFingerprint SHA256:... `
    -RestoreFrom D:\backups\2026-10-01_200000 -GameZipUrl https://example.org/BaseGame144.zip `
    -ServerName "Saturday Ramsgate"

# The plan only: connects to nothing
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -OwnerName Slayer -GameZip D:\BaseGame144.zip -WhatIf

# Day to day
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -Update
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -InviteFor friend1
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -Status
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -Chat On
```

### Install-DauntlessServer.ps1

Runs on the server, in an elevated PowerShell. `Deploy-Remote.ps1` runs it from
`<root>\staging\kit`, and you can run it yourself. Every step is safe to repeat: a re-run repairs or
completes an install and leaves alone what is already right. The steps are described in
[What the installer does]({{ winserver_page.url | relative_url }}#what-the-installer-does).

The installer refuses to run while any part of the stack is running. Stop it first with
`Stack.ps1 stop`, or use `Update-DauntlessServer.ps1` for code updates. The preflight checks for an
administrator, a 64-bit Windows build 17763 or newer with the Desktop Experience, at least 7.5 GB of
RAM as Windows reports it, a 40 GB disk budget (2 GB in a sandbox; the game zip on the same drive and
an installed game count towards it), `tar.exe` and free ports. A problem stops a real run.
Under `-WhatIf` every problem is only a warning. With `-Sandbox` the machine checks are only warnings,
but a port that is taken still stops the run.

**Mode and address**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Mode` | `Public` or `Private` | A new install: `Public`. A re-run: the installed mode. An old `server.json` with no mode counts as `Private`. | `Public`: a TLS gateway, v2 invites. `Private`: Tailscale, v1 invites. |
| `-PublicHost` | IPv4 address or DNS name | The stored `PublicHost`. Otherwise the one public IPv4 address on the network adapters; the installer stops if there is none or more than one. `127.0.0.1` in a sandbox. | Public mode: the address friends connect to. It goes into invites and into the certificate. Its IPv4 address is what game servers advertise. It must resolve to a public IPv4 address, except in a sandbox. Behind 1:1 NAT, pass it. |
| `-GatewayPort` | 1-65535 (a sandbox: 62000-62499) | The stored gateway port, else `443` (`62443` in a sandbox) | Public mode: the gateway's TCP port, the only public TCP port. |
| `-ServerName` | 1-64 characters: no line breaks, control characters, double quotes, backslashes or direction-override characters, and no spaces at either end | `Dauntless Revived` | The name friends see in invites and in the launcher. **Not remembered by re-runs:** a re-run without it sets the name back to `Dauntless Revived`. |
| `-InstallRoot` | path | `C:\DauntlessRevived` | Where everything goes. Required with `-Sandbox`. |

**Game files** (use one of the three)

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-GameZip` | path | none | The verified 1.4.4 zip. It is checked against the pinned SHA-256 and extracted with Windows' own `tar.exe` into `<root>\game`. Not needed once the game is installed. |
| `-GameZipUrl` | `https://` URL | none | The server downloads the zip to `<root>\downloads\BaseGame144.zip`. The download resumes after interruptions, up to 20 retries in one run, and a re-run continues it. A file with the wrong SHA-256 is deleted. |
| `-GameDir` | the folder that contains `Archon\` | `<root>\game\Dauntless`; if the game is not there, the stored game folder when it is valid | Uses an already extracted game where it is. The service account gets read access to it. Required with `-Sandbox`, where it is a stand-in folder. |

Every install checks the game exe against its pin and every game file against the content
manifest. Then it installs the two server DLLs with their pinned hashes.

**Server code** (use one of `-SourceZip`, `-SourceDir` and `-Ref`)

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-SourceZip` | path of a source zip (`git archive`) | none | Builds from an uploaded zip. This is what `Deploy-Remote.ps1` sends. |
| `-SourceCommit` | 7-40 hex characters, optionally with `-dirty` or `-worktree` | `zip-` and the first 12 hex characters of the zip's SHA-256 | Names the commit of `-SourceZip` for the version reports. |
| `-SourceDir` | a folder with `legacy\UndauntedMetagame\package.json` | none | Builds from a local folder. In a git checkout the commit is read from git, with `-dirty` when there are changes. |
| `-Ref` | branch, tag or commit: letters, digits, `.`, `_`, `/`, `-` (1-100) | See below | Downloads that ref from GitHub and builds it. |

With none of the three, the installer builds the checkout it runs in, if it runs from one (when
`..\..\legacy\UndauntedMetagame\package.json` exists). Otherwise it builds `$DRRepo.PinnedRef`, which is
`friends-v1`, from GitHub. **Watch out:** a re-run from `<root>\bin` is not in a checkout, so without
a source parameter it downloads `friends-v1`. To keep the installed code, re-deploy with
`Deploy-Remote.ps1`, which uploads the code again, or pass `-Ref <the installed commit>` if that
commit is on GitHub. A commit that is already built is not rebuilt, unless it ends in `-dirty` or
`-worktree` or is `unknown`. The build copies the source into `<root>\app.new` (without
`node_modules`, build output, `.git`, `.env` files, keys, certificates, databases and logs), runs
`npm ci` and `npm run build` in the four server packages (`legacy/UndauntedMetagame`,
`legacy/UndauntedDeployServer`, `legacy/UndauntedContent` and `legacy/UndauntedGateway`; the launcher is not built), then
swaps it in, keeping the old build as `app.prev`.

**Owner, restore and certificate**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-OwnerName` | 3-16 letters, digits or `_` | Asked for on a console when missing | The admin account of a new database. Its key goes to `data\keys\owner.key` and is never printed. **The owner key is secret: never share, never commit;** keep a copy in a password manager. Not used when `-RestoreFrom` brings the accounts, or when a working `owner.key` exists. An `owner.key` that exists but does not work stops the install: move it away to create a new owner. |
| `-RestoreFrom` | a backup folder | none | Moves a server here. See [Restore, rollback and uninstall](#restore-rollback-and-uninstall). **Secret: holds every key.** |
| `-NewCertificate` | switch | off | Makes a new gateway certificate even though one exists or the backup has one. **Every invite handed out before stops working.** |

**Remote Desktop and session 0**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-AdminIp` | IPv4 address or `IPv4/prefix` with a prefix from 8 to 32; one or more | The stored list | Public mode: every enabled inbound Remote Desktop rule (TCP and UDP 3389) is limited to these addresses. |
| `-KeepRdpOpen` | switch | off | Public mode without `-AdminIp`: leave internet-open Remote Desktop rules on. Without it they are turned off and the old state is recorded in `server.json`. |
| `-InteractiveSession` | switch | off | The `dauntless` account signs in automatically at boot, the stack starts at that logon, and the session locks right away. The password is kept as an LSA secret, never in plain text. **Not remembered by re-runs:** a re-run without it turns the auto-logon off. See [Session 0]({{ winserver_page.url | relative_url }}#session-0-and--interactivesession). |

**Ports**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-MetagamePort`, `-DeployPort`, `-ContentPort`, `-AllowlistPort` | 1024-65535 (a sandbox: 62000-62499) | The stored port, else 61000, 61001, 61002 and 61005 (a sandbox: 62000, 62001, 62002 and 62005) | The components' TCP ports. The ports in use must all differ and be free. [Ports and network]({{ ports_page.url | relative_url }}) explains each one. |

**Chat**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Chat` | `On` or `Off` | The stored `"Chat"` in `server.json`, else `On` (public mode) | The in-game text chat. Public mode writes `CHAT=1` or `0`, `CHAT_BIND_HOST=127.0.0.1` and `CHAT_PORT` (61099, or 62099 in a sandbox, the same port as the gateway's `GATEWAY_WS_URL`) into `metagame.env`, and saves the choice. No firewall rule is added. `-Chat On` in private mode stops the installer: private mode has no chat yet. |

**Private mode (Tailscale)**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-TailscaleAuthKey` | Tailscale auth key, normally starting with `tskey-` | none | Signs the server in to Tailscale without a browser. Refused in public mode. **Secret: never share, never commit.** A parameter shows up in the process list and in the PowerShell history, so prefer leaving it out: the installer then prints the `tailscale up` command to run and waits up to 20 minutes. With the key, it writes it to a temporary file only Administrators and SYSTEM can read, passes it as `--auth-key=file:<path>` and deletes the file. |
| `-TailscaleHostname` | text | `dauntless-server` | The machine name for `tailscale up`. |
| `-TailscaleShareUrl` | `https://login.tailscale.com/...`, at most 512 characters | The stored link | The machine-share link that v1 invites carry as `&share=`. Refused in public mode. |
| `-AdvertiseHost` | IPv4 address or DNS name | The stored value, else the Tailscale IPv4 address | The host that v1 invites carry, for example the MagicDNS name. |

**Development and tests**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Sandbox` | switch | off | A test install on a development PC. It skips Node.js, the runtimes, Tailscale, the firewall, the accounts and the tasks. Everything listens on `127.0.0.1` on the 620xx ports, including the gateway. The allowlist helper runs in dry-run, no deploy server or game server runs, and everything runs as the current user. Needs `-InstallRoot` and `-GameDir`. |
| `-ContentManifest` | path | the built-in manifest | A manifest for a stand-in game folder. Refused without `-Sandbox`. |
| `-AllowlistDryRun` | switch | off | The allowlist helper only logs the firewall change it would make. Forced on by `-Sandbox`. **Not remembered by re-runs.** |
| `-NoStart` | switch | off | Does not start the stack at the end. The first backup is still taken. |

**What a re-run keeps.** Without the parameter, a re-run uses what `server.json` holds for `-Mode`,
`-PublicHost`, `-GatewayPort`, the four other ports, `-AdminIp`, `-AdvertiseHost`,
`-TailscaleShareUrl`, `-GameDir` and `-Chat`. It keeps the certificate, the keys, the secrets and every `.env`
key it does not manage (see [Configuration]({{ config_page.url | relative_url }})). It does **not**
keep `-ServerName`, `-InteractiveSession` or `-AllowlistDryRun`: pass them again every time.

At the end the installer takes the first backup and starts the stack. It does not start it with
`-NoStart`, or with `-InteractiveSession`, where the stack starts at the next automatic logon (restart
the server). Then it prints the address, `DRFINGERPRINT=<64 hex>` in public mode, and the path of the
owner key (never the key). In public mode it also reminds you to open TCP on the gateway port and UDP
8770-8777 in the provider's firewall, and it tells you to restart the server when it removed a policy
value that kept Windows Firewall off.

```powershell
# From an unpacked kit on the server: a new public server
.\Install-DauntlessServer.ps1 -GameZip D:\BaseGame144.zip -PublicHost 203.0.113.7 -AdminIp 198.51.100.20 -OwnerName Slayer

# Everything it would change, without changing anything
.\Install-DauntlessServer.ps1 -Mode Public -GameZipUrl https://example.org/BaseGame144.zip -OwnerName Slayer -WhatIf

# A private-mode server that puts its MagicDNS name into invites
.\Install-DauntlessServer.ps1 -Mode Private -GameZip D:\BaseGame144.zip -OwnerName Slayer -AdvertiseHost dauntless-server.example.ts.net

# Re-run on an installed server to limit Remote Desktop to a new address (stop the stack first)
C:\DauntlessRevived\bin\Stack.ps1 stop
C:\DauntlessRevived\bin\Install-DauntlessServer.ps1 -Ref <installed commit> -ServerName "Saturday Ramsgate" -AdminIp 198.51.100.21
```

### Update-DauntlessServer.ps1

Runs on the server, elevated. It replaces the server code only. The `.env` files, keys, the
certificate, the game files and the firewall stay as they are; run the installer again for those.

1. It builds the new code into `<root>\app.new` while the server keeps running, at below-normal
   priority.
2. It takes a backup, stops the stack, swaps `app.new` in (the old build becomes `app.prev`) and
   refreshes the scripts in `<root>\bin` from the new code.
3. It starts the stack and waits until the metagame answers `/dauntless-status` and, in public
   mode, the gateway answers `ServerStatus` over TLS with the certificate from the invites. If that
   does not happen within `-HealthTimeoutSec`, it moves the new build to `app.failed`, switches back
   to `app.prev`, starts it and exits with `1`.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Root` | path | See [Before you run a kit script](#before-you-run-a-kit-script) | The install root. |
| `-Ref` | branch, tag or commit | `friends-v1` | Downloads that ref from GitHub. This is the default parameter set: with no source parameter at all, the updater builds `friends-v1`, which may be older than what the server runs. **Always name the source.** |
| `-SourceZip` | path | none | Builds from a source zip. `Deploy-Remote.ps1 -Update` passes this. |
| `-SourceCommit` | 7-40 hex characters, optionally with `-dirty` or `-worktree` | `zip-<12 hex of the zip's SHA-256>` | The commit of `-SourceZip`. |
| `-SourceDir` | a folder with `legacy\UndauntedMetagame\package.json` | none | Builds from a local folder. |
| `-Rollback` | switch | off | Switches back to `app.prev` by hand: stop (with a backup), rename `app` to `app.rolledback` and `app.prev` to `app`, restore the recorded commit, refresh the scripts in `<root>\bin`, start without another backup, check. |
| `-Force` | switch | off | Rebuilds even when that commit is already installed. A commit ending in `-dirty` or `-worktree` is always rebuilt. |
| `-HealthTimeoutSec` | seconds | `180` | How long the new build may take to become healthy before the automatic rollback. |

The updater refuses new code that lacks a component the server runs, and warns when a server DLL in
the game folder is missing or changed.

**Real progression is the default in this version.** In `metagame.env` the updater changes only
`GIT_COMMIT`, and the installer never writes `PROGRESSION_MODE` either: both keep whatever the file
has. A server whose `metagame.env` has no `PROGRESSION_MODE` line, or an empty one, switches from
upstream's fake max ranks to real progression when it runs the new code. To keep the fake max ranks,
add `PROGRESSION_MODE=stub` to `metagame.env` before you update. Read the
[upgrade notes]({{ upgrade_page.url | relative_url }}) before you update a server that already has
players.

After a successful update, unless `metagame.env` says `PROGRESSION_MODE=real` or `stub`, the updater
waits up to 20 seconds for the metagame's progression-mode line in `datalogsmetagame.out.log`. If
the metagame then warns that some player accounts have no stored progression yet, the updater
repeats that warning with a link to the upgrade notes. This check never fails the update.

```powershell
C:\DauntlessRevived\bin\Update-DauntlessServer.ps1 -Ref <tag or commit>
C:\DauntlessRevived\bin\Update-DauntlessServer.ps1 -SourceZip C:\DauntlessRevived\staging\source\source-<12 hex>.zip -SourceCommit <commit>
C:\DauntlessRevived\bin\Update-DauntlessServer.ps1 -Rollback
```

### Stack.ps1

One command for the whole stack. The components, in start order: the allowlist helper (public mode
only), the metagame, the content server, the gateway (public mode only) and the deploy server,
which starts the game servers. `server.json` lists which ones this install runs. Processes are
found by their full command line, so other Node programs and a Dauntless client on the same machine
are never touched.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| action (first position) | `status`, `start`, `stop`, `restart` or `supervise` | `status` | See the table below. |
| `-Root` | path | See [Before you run a kit script](#before-you-run-a-kit-script) | The install root. |
| `-Only` | one or more of `metagame`, `content`, `deploy`, `gateway`, `allowlist` | every configured component | Acts on these components only. Through the scheduled tasks, only `restart -Only` is allowed. |
| `-NoBackup` | switch | off | Skips the backup that is otherwise required before the metagame starts and taken after a stop. |
| `-Direct` | switch | off | Starts or stops the processes as the current user even when the scheduled tasks exist. Administrators only. The installer uses it to run the metagame once. |

| Action | What it does |
|:-------|:-------------|
| `status` | The install and its code version; each component with its process, addresses and memory, or `down`; each game server with its UDP port and role (the highest port, 8777, is Ramsgate, the one below is the Training Dojo, the rest are hunts); the gateway's TLS check; the allowlist helper and its firewall rule; the scheduled tasks; the last backup; the stop flag; when the last performance sample was taken (or that the performance log is off); and the players online, which the metagame shows only with the owner key. |
| `start` | On an installed server, run by an administrator: takes a backup if the metagame is down (and refuses to start without one, unless `-NoBackup`), clears the stop flag, starts the allowlist task and the stack task, and waits up to 2 minutes for the metagame and the gateway. Otherwise (a sandbox, the service account, or `-Direct`): starts the components itself, takes the backup before the metagame, waits up to 30 s for each port, checks the metagame and the gateway, and waits up to 60 s for Ramsgate on UDP 8777. |
| `stop` | Sets the stop flag, ends the scheduled tasks (when run by an administrator on an installed server), stops the deploy server and its game servers, then the gateway, the content server, the metagame and the allowlist helper. It closes the game ports (the allowlist rule is disabled) and takes a backup. |
| `restart` | `stop`, a backup, then `start`. `restart -Only <component>` through the tasks only stops that component; its supervisor starts it again within a minute, without a backup. |
| `supervise` | Runs only inside the scheduled tasks (or in a sandbox). It starts the components, then checks every 15 s. A component that is down is restarted after `min(60, 5 * 2^(n-1))` seconds, where `n` is its crashes in the last 10 minutes. After more than 5 it gives up on that component until the next start. It exits when the stop flag is set; the SYSTEM allowlist supervisor ignores the flag and is ended through its task. A supervised start skips the backup if the newest one is less than 10 minutes old. Every 4th check (about once a minute) the supervisor also takes a performance sample, as [Write-PerformanceLog.ps1](#write-performancelogps1) describes, unless `server.json` has `"PerformanceLog": false`; the SYSTEM allowlist supervisor never does. A sample that cannot be written (the file is open in another program, or a link was planted in `data\logs`) is skipped, and `supervisor.log` says so once. |

The service account never runs the allowlist helper, and SYSTEM runs nothing else. On an installed
server, `start` and `stop` therefore work through the two scheduled tasks. Each component runs as
`node --env-file=<data\config\X.env> <entry script>` in its package folder. Before starting it,
`Stack.ps1` clears every variable of that `.env` file from its own environment, so the file wins.

```powershell
C:\DauntlessRevived\bin\Stack.ps1                     # status
C:\DauntlessRevived\bin\Stack.ps1 restart -Only gateway
C:\DauntlessRevived\bin\Stack.ps1 stop
C:\DauntlessRevived\bin\Stack.ps1 start
```

### New-Invite.ps1

Runs on the server, elevated. It reads the owner key from `data\keys\owner.key` (never printed) and
calls the metagame directly on its own address, because the gateway refuses admin routes. Before it
prints anything, it checks that the owner key belongs to an admin.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Root` | path | See [Before you run a kit script](#before-you-run-a-kit-script) | The install root. |
| `-Uses` | 1-100 | `1` | How many accounts the new code can create. |
| `-For` | text without control characters | none | A label for your own records. The metagame writes it (printable ASCII, at most 64 characters) to its log line for the new code; it is not stored with the code. |
| `-ShareUrl` | `https://login.tailscale.com/...` | the stored share link | Private mode: a Tailscale machine-share link added to the invite as `&share=`. Refused in public mode. |
| `-SaveShareUrl` | switch | off | Private mode: stores `-ShareUrl` in `server.json`, so later invites include it. |
| `-AdvertiseHost` | IPv4 address or DNS name | public mode: the stored `PublicHost`; private mode: the stored advertised host, else the bind address | The host written into this invite. |
| `-SkipGatewayCheck` | switch | off | Public mode: skips the check that the gateway answers on `127.0.0.1` with the certificate from `server.json`. |
| `-List` | switch | | Lists the open codes and how many uses each has left. |
| `-Revoke` | invite code | | Deletes that code. Accounts made with it stay. |

It creates the code with the metagame's `CreateInvite` route (older metagames: `RegisterInviteCode`
with a code made by the script), checks that the code is in the server's list, round-trips the
invite line through the same strict parser the launcher uses, and prints one line:

```text
dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=<64 hex>&code=ABCD-EFGH-JKLM&name=Saturday%20Ramsgate
dauntless-revived://join?v=1&host=100.x.y.z&port=61000&code=ABCD-EFGH-JKLM&name=Home&share=<url-encoded share link>
```

`v=2` is public mode: `fp` is the SHA-256 of the gateway certificate, and the launcher accepts that
certificate and no other. `v=1` is private mode over Tailscale. The invite code lets someone create
an account: send it privately. Public mode refuses to make an invite when the certificate file no
longer matches the fingerprint in `server.json`.

```powershell
C:\DauntlessRevived\bin\New-Invite.ps1 -For friend1
C:\DauntlessRevived\bin\New-Invite.ps1 -Uses 3 -For "Saturday group"
C:\DauntlessRevived\bin\New-Invite.ps1 -List
C:\DauntlessRevived\bin\New-Invite.ps1 -Revoke ABCD-EFGH-JKLM
```

### Get-ServerStatus.ps1

Calls `GET /undaunted/api/ServerStatus` and prints the server's name and version, the players online
and where they are, and every running world and hunt. The server shows the players and game servers
to registered players only, so the request carries an account key. Without a key, or with one the
server does not accept, the script says that the list is hidden instead of printing 0 players.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| (none) | | | Asks this machine's own server. In public mode it goes through the gateway on `127.0.0.1` with TLS pinned to the certificate fingerprint, as a friend's launcher does. It sends the owner key when it can read it. |
| `-Root` | path | See [Before you run a kit script](#before-you-run-a-kit-script) | The install root (local use). |
| `-Direct` | switch | off | Local use: asks the metagame on its own address instead of the gateway. |
| `-Invite` | an invite line | | Asks the server in that invite: v2 over TLS pinned to its `fp`, v1 over plain HTTP. |
| `-Server` | `host` or `host:port` | | Asks any server. With `-Fingerprint` over TLS (port 443 unless given); without it over plain HTTP (port 61000 unless given). |
| `-Fingerprint` | 64 hex characters | none | The certificate fingerprint for `-Server`. |
| `-KeyFile` | path | the owner key, locally | An `account.key` or the launcher's key backup (a `Key: ...` line is accepted). **Secret: never share, never commit.** The key goes only over TLS pinned to a fingerprint, or over plain HTTP to `localhost`, `127.x.x.x`, a Tailscale address (100.64.0.0/10) or a `*.ts.net` name; anything else is refused. |
| `-Json` | switch | off | Prints the server's answer unchanged (`"limited": true` when the list is hidden). |
| `-TimeoutSec` | seconds | `8` | The HTTP timeout. |

The exit code is `0` when the server answered and `1` when it did not answer or answered with an
HTTP error. A metagame too old to have `ServerStatus` is reported as online, without players or game
servers. When a pinned TLS connection
fails because the server shows a different certificate, the script says so: either the server got a
new certificate, or someone is in between.

```powershell
C:\DauntlessRevived\bin\Get-ServerStatus.ps1
.\Get-ServerStatus.ps1 -Invite 'dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=...&code=...&name=...' -KeyFile .\account.key
.\Get-ServerStatus.ps1 -Server 203.0.113.7:443 -Fingerprint <64 hex> -Json
```

On the server itself it also prints a `chat:` line, as `Stack.ps1 status` does.

### Set-Chat.ps1

Turns the in-game text chat on or off on an installed public-mode server
([Chat]({{ winserver_page.url | relative_url }}#chat)). It needs an elevated PowerShell, except on a
`-Sandbox` install.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-On` / `-Off` | switch, one of them **required** | | The new state. |
| `-Root` | path | `C:\DauntlessRevived` | The install root. |

It writes `CHAT=1` or `0`, `CHAT_BIND_HOST=127.0.0.1` and `CHAT_PORT` (61099, 62099 in a sandbox) into
`metagame.env` and `"Chat"` into `server.json`. Then it runs `Stack.ps1 stop` and `start`, with the
usual backups around them, and waits up to 30 seconds for `127.0.0.1:<port>` to listen (or to stop
listening). If a step fails, it puts both files back as they were, restarts the stack again and
exits with `1`. When the setting is already as asked, it changes nothing and restarts nothing; it
goes by the three chat keys and `"Chat"`, so what the installer or an update wrote elsewhere in
`metagame.env` does not count. The restart drops the parties and matchmaking queues, which live in
memory: run it when nobody is playing. From your PC, `Deploy-Remote.ps1 -Server <address> -Chat On`
runs it over SSH. On a server installed before chat, the script arrives with the update, so update
first. Based on Vvoidddd's first chat switch
([#9](https://github.com/mixutin/dauntless-revived/pull/9)).

```powershell
C:\DauntlessRevived\bin\Set-Chat.ps1 -On
C:\DauntlessRevived\bin\Set-Chat.ps1 -Off
```

### Backup-DauntlessServer.ps1 and backup-hidden.vbs

`Backup-DauntlessServer.ps1` copies everything a restore needs into
`<root>\backups\yyyy-MM-dd_HHmmss\`: the database, taken with SQLite's online backup and checked;
`server.json`; the news file; the `.env` files of the metagame, deploy server, content server and
gateway; every `*.key` file; and the gateway certificate with its private key.
[Files and data]({{ files_page.url | relative_url }}) lists the layout. `allowlist.env` is not
copied: an install makes a new one. **A backup holds every key: never share it, never commit it,
and copy it off the server only encrypted.**

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Root` | path | See [Before you run a kit script](#before-you-run-a-kit-script) | The install root. |
| `-Hourly` | number | `48` | Keep the newest N backups. |
| `-Daily` | number | `30` | Also keep the newest backup of each of the last M days that have one. |

It runs at below-normal priority, appends one line per run to `backups\backup.log` (moved to
`backup.log.1` above 5 MB), and exits with `1` only when the database copy fails its check. A server
with no database yet is not an error. It runs hourly from the backup task, before every start and
after every stop (`Stack.ps1`), and before every update.

`backup-hidden.vbs` runs it without a window, so nothing flashes up on a signed-in desktop:
`wscript.exe backup-hidden.vbs [install root]`. Without an argument the root is the folder above the
script's folder. It returns the backup's exit code.

```powershell
C:\DauntlessRevived\bin\Backup-DauntlessServer.ps1
C:\DauntlessRevived\bin\Backup-DauntlessServer.ps1 -Hourly 96 -Daily 60
```

### Write-PerformanceLog.ps1

Records what the server uses (roadmap 4.12). One sample is a `host` row plus one row per process: the
metagame, content server, gateway, deploy server and allowlist helper of this install (found by
their command line, like `Stack.ps1` finds them), and every game server started from this install's
game folder. The rows are appended to `<root>\data\logs\performance\performance-<UTC date>.csv`; the
columns are in [Files and data]({{ files_page.url | relative_url }}#performance-log). Files older than
`-KeepDays` are deleted. Based on the first sampler by Vvoidddd
([#6](https://github.com/mixutin/dauntless-revived/pull/6)).

On an installed server the stack supervisor already takes a sample every minute (see `supervise`
under [Stack.ps1](#stackps1)), so run this script for one sample now (`-Once`), or for a `-Sandbox`
install or a stack started by hand, which have no supervisor.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Root` | path | See [Before you run a kit script](#before-you-run-a-kit-script) | The install root. |
| `-Once` | switch | off | Takes a baseline, waits 2 seconds, writes one sample and exits. |
| `-IntervalSeconds` | 1-3600 | `60` | Seconds between samples. The first sample comes one interval after the start. Stop it with Ctrl+C. |
| `-KeepDays` | 1-3650 | `30` | Day files to keep, today included. |

How the numbers are taken:

- **CPU** of a process: its CPU time since the previous sample, as a percentage of **one core** (two
  busy cores read 200). The machine's CPU is the whole machine (0-100), from the processor counter,
  which reads the same on every Windows language. The first sample of a run has no CPU figures.
- **Memory:** the working set and the private (committed) memory of each process; the machine's
  total and free RAM.
- **Game servers:** the UDP port comes from the process's UDP endpoint (in 8700-8799), never from
  its command line, which starts with the game-server key and lists the expected players' account
  ids. The role (`ramsgate`, `dojo`, `hunt`, `tutorial`) comes from the deploy server's list, or from
  the port the way `Stack.ps1 status` decides it (`UdpPortEnd` from `server.json` is Ramsgate, one
  below is the Dojo).
- **Players:** the metagame's `ServerStatus`, asked with the owner key, and the deploy server's
  `/gameservers`, joined by server id. Only the counts are kept; the names and account ids in those
  answers are dropped. Without a readable owner key the counts stay empty, never 0.
- **Network:** bytes on the network adapters, without loopback, VPN tunnels (Tailscale, WireGuard)
  and virtual switches, whose traffic also crosses a real adapter.

It writes nothing through a junction, symbolic link or hard link: when `data\logs`, its
`performance` folder or the day's file is one, the sample is refused. A reading that fails (for
example the metagame is down) leaves its cells empty; the rest of the row is still written. The exit
code is `1` when `-Once` could not write its sample.

```powershell
C:\DauntlessRevived\bin\Write-PerformanceLog.ps1 -Once
.\Write-PerformanceLog.ps1 -Root C:\dr\sandbox-ws2019\root -IntervalSeconds 30
```

### Receive-Upload.ps1

The server side of `Deploy-Remote.ps1`'s chunked upload. You do not run it yourself.

| Parameter | Type | What it does |
|:----------|:-----|:-------------|
| `-Action` | `Begin`, `Verify`, `Assemble` or `Clean`, **required** | `Begin` takes `upload.json.incoming` as the new `upload.json` (dropping old parts if it describes a different upload) and reports the parts already verified. `Verify` hashes one part. `Assemble` joins the parts, hashing each again, checks the whole file and renames it into place. `Clean` deletes the parts. |
| `-Dir` | folder, **required** | The upload folder (`<root>\staging\upload`). |
| `-Index` | part number, counted from 0 | The part for `Verify`. |

The part size in `upload.json` must be between 1 MB and 2 GB. Each call prints one `DRJSON:{...}` line.

### Helper scripts in lib\

The kit scripts call these with the server's `node.exe`. Run them by hand only when you know why.

| Command | What it does |
|:--------|:-------------|
| `node dr-db.js integrity <metagame folder> <db>` | Read-only integrity check. Prints `ok, <n> users`. |
| `node dr-db.js backup <metagame folder> <db> <destination>` | SQLite online backup (safe while the metagame runs), then checks the copy. |
| `node dr-db.js add-invite <metagame folder> <db> <code>` and `del-invite ...` | Adds or deletes a one-use invite code. The installer uses this to register the owner. |
| `node dr-db.js make-admin <metagame folder> <db> <UserId>` | Makes an account an admin. |
| `node dr-db.js gs-key <metagame folder> <db> <key file>` | Registers the SHA-256 of the game-server key. The key is read from the file, needs at least 32 characters and is never printed. |
| `node dr-keys.js signing <out file>` | Writes a new RSA-2048 key pair for RS256 token signing as two `.env` lines (`AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64`). Prints nothing secret, but **the output file is secret: never share, never commit.** The installer merges it into `metagame.env` and deletes the file. |
| `node verify-game.js <manifest.json> <game folder> [--quick]` | Checks every file in the manifest for size and SHA-256 (`--quick`: size only). Extra files are ignored. Exit code `0` all match, `1` a mismatch, `2` a usage error, an unsafe path in the manifest or a read error. |

`dr-db.js` loads `better-sqlite3` from the metagame's own `node_modules`, and exits with `1` on any
failure. The `<metagame folder>` is `<root>\app\UndauntedMetagame` and the database is
`<root>\data\undaunted.db`.

### Restore, rollback and uninstall

There are no separate scripts for these:

| Task | How |
|:-----|:----|
| Restore a backup, or move a server | `Install-DauntlessServer.ps1 -RestoreFrom <backup folder>`, or from your PC `Deploy-Remote.ps1 -RestoreFrom <local backup folder>`. The folder needs `undaunted.db`, `secrets\metagame.env` and `secrets\deployserver.env`. The database is checked first, and an existing database is never overwritten: delete it first if you mean to replace it. Settings from the backup's `metagame.env` and `deployserver.env` win over the existing ones. The token-signing keys, the game-server key, the `*.key` files (existing ones are kept) and the gateway certificate come along, so invites already handed out keep working. The backup's certificate is used only when this server has none yet and `-NewCertificate` is not given. The gateway and allowlist secrets are not taken from the backup: they stay, or are made new. Backups from `Backup-DauntlessServer.ps1` and from the original host's `backup.ps1` are both accepted. |
| Go back to the previous build | `Update-DauntlessServer.ps1 -Rollback`. |
| Uninstall | A manual snippet: [Uninstall]({{ winserver_page.url | relative_url }}#uninstall). |

### Kit tests

For a development PC. None of them changes the firewall, services, scheduled tasks, accounts or
certificate stores, and none uses the ports of a running server.

If you edit a kit script, keep it ASCII only: Windows PowerShell 5.1 reads a file without a byte
order mark as ANSI. `.gitattributes` in `deploy/windows-server/` keeps `*.ps1`, `*.vbs` and `*.md`
at CRLF line endings, also in `git archive`, which is what `Deploy-Remote.ps1` uploads.

| Script | Parameters | What it tests |
|:-------|:-----------|:--------------|
| `tests\Test-KitUnit.ps1` | `-WorkDir` (default `%TEMP%\dr-kit-unit`; emptied at the start, deleted at the end); `-Port` (62000-62499, default `62450`; the performance check also uses the port above it) | Every kit script parses on PowerShell 5.1 and is ASCII only; invite lines v1 and v2 and everything that must be refused; addresses and `.env` rules; certificate fingerprints; TLS pinning against a local test server; `Get-ServerStatus.ps1` with and without a key; key files; the upload helper; the performance sampler (CPU arithmetic, roles from the port range, the fixed header, day files and their pruning, player counts from a stand-in metagame with no names or keys in the file, a decimal point on a Finnish Windows, and refusing a junction or a hard link); the chat settings (`CHAT_PORT` equal to the gateway's WebSocket port in normal and sandbox installs, `-Chat` against `server.json`, the keys surviving a re-run, and the `chat` status line). Needs `node` on `PATH` and `npm ci` in `legacy/UndauntedGateway`. |
| `tests\Test-DeployRemote.ps1` | `-WorkDir` (default `%TEMP%\dr-deploy-test`; emptied at the start, deleted at the end) | `Deploy-Remote.ps1` without a server: argument refusals, `-WhatIf`, the kit, source and backup uploads, the chunked upload with a dropped connection and damaged parts, and `-Chat` (passed to the installer, or `Set-Chat.ps1` on its own). No network, no SSH key. |
| `tests\Test-Sandbox.ps1` | `-SandboxDir` (default `C:\dr\sandbox-ws2019`; the folder name must contain `sandbox`, because it is deleted, and it must be outside the checkout, which the installer copies); `-KeepSandbox`; `-SkipRestore` | A full `-Sandbox` public-mode install with chat on into a scratch folder, then invites, status, the gateway's refusals, registration through the gateway, the game's chat connection through the gateway (101), an update and a rollback, `Set-Chat.ps1 -Off` (502 again), a performance sample, a backup, a restore into a second folder, and cleanup. Needs ports 62000, 62002, 62005, 62099 and 62443 free, and builds the code with `npm ci`. The log is copied to `%TEMP%\dr-sandbox-test.log` unless `-KeepSandbox`. |

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-KitUnit.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-DeployRemote.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-Sandbox.ps1 -SandboxDir D:\scratch\dr-sandbox
```

### Kit constants

Fixed values in `deploy/windows-server/DauntlessServer.Common.ps1`. The ports can be changed per
install; the names cannot.

| Constant | Value | Used for |
|:---------|:------|:---------|
| `$DRNames.ServiceUser` | `dauntless` | The low-privilege local account that runs the stack. |
| `$DRNames.StackTask` | `Dauntless Revived stack` | Scheduled task: `Stack.ps1 supervise` as `dauntless`, one minute after startup (or at its logon with `-InteractiveSession`). |
| `$DRNames.AllowlistTask` | `Dauntless Revived allowlist` | Scheduled task, public mode: `Stack.ps1 supervise -Only allowlist` as SYSTEM, at startup. |
| `$DRNames.BackupTask` | `Dauntless Revived backup` | Scheduled task: `backup-hidden.vbs` as `dauntless`, hourly. |
| `$DRNames.FirewallGroup` | `Dauntless Revived` | The group of the kit's firewall rules. |
| `$DRNames.AllowlistRule` | `Dauntless Revived game ports (allowlist)` | Display name of the UDP game-port rule. |
| `$DRNames.AllowlistRuleName` | `DauntlessRevived-GamePorts-Allowlist` | Name of that rule. It must match `legacy/UndauntedGateway/src/allowlist/firewall.ts`. |
| `$DRDefaultPorts` | metagame 61000, deploy 61001, content 61002, gateway 443, allowlist 61005 | Default TCP ports. |
| `$DRSandboxPorts` | metagame 62000, deploy 62001, content 62002, gateway 62443, allowlist 62005 | TCP ports of a `-Sandbox` install. |
| `$DRRelayPort` | 61000 | The friend launcher's relay port. Public mode sends every player the QoS address `http://127.0.0.1:61000/QoS`, so it is fixed. |
| `$DRChatPort` | 61099 | The chat (XMPP) port in the service account's `Engine.ini`, and the gateway's WebSocket upstream (62099 in a sandbox). |
| `$DRUdpBegin`, `$DRUdpEnd` | 8770, 8777 | The game servers' UDP range. |
| `$DRRepo.Url` | `https://github.com/mixutin/dauntless-revived` | Where the installer and the updater download code, and the source link the metagame reports. |
| `$DRRepo.PinnedRef` | `friends-v1` | The ref built when no source is named. |

**Pinned hashes.** Each value is copied into several files. Change them together.

| What | SHA-256 | Also pinned in |
|:-----|:--------|:---------------|
| Game zip, 1.4.4 (`BaseGame144.zip`, 10,479,214,119 bytes) | `556B9A648A5E5E7E11B6F8DD3D80FF8E88FCEB0D3448297AAF47CE7BF756BC6D` | `friend-kit/setup.ps1`, `tools/make-game-manifest.ts` |
| `Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe` | `D3D41E614908D2BEFD518B27046D9822D6130EF12BA3504BABBDB786BEF9CFF4` | `friend-kit/setup.ps1`, `friend-kit/play.ps1`, `tools/make-game-manifest.ts`, `legacy/UndauntedLauncher/src/main/constants.ts`, the content manifest |
| `dxgi.dll` | `9A431D7B6FD20C43FA92BEBD91C3BC023EC7A3FCBC52871C41F4DF293D4B0D1F` | `friend-kit/setup.ps1`, `friend-kit/play.ps1`, `tools/make-friend-kit.ps1`, `legacy/UndauntedLauncher/src/main/constants.ts` |
| `UndauntedInternalServer.dll` | `520EC588A0554E374B2B0D084CD7F7F08D59A9CB80362679845719D64A0D0933` | the same files as `dxgi.dll` |
| Node.js 24.19.0 MSI (`node-v24.19.0-x64.msi`) | `F0F66C2A80C08A30A5AB5179EE9EA9E45F9B46289436A8CC87FF833B852DB351` | only the kit; it must also match nodejs.org's `SHASUMS256.txt` |
| Tailscale 1.102.4 MSI (private mode) | `80EB007E39DFEBE17299FA1A09C79A8E1D934F76E0246C0817EBE3AF675B7EF6` | only the kit; it must also match the published `.sha256` file |

The build string is `dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827`: the installer
compares `Version.txt` with it and writes the changelist, `239827`, to the metagame. The Visual C++
runtime and the DirectX June 2010 runtime are not pinned by hash. They are accepted only with a
valid Microsoft Authenticode signature. The zip size is declared in the kit but not checked by any
script.

## Friend kit

`friend-kit/` is the older way for a friend to join: two PowerShell scripts, each with a
double-click wrapper. They talk plain HTTP to `<host>:61000`, so they work with a private-mode
(Tailscale) server or a hand-built host reached over Tailscale. A public-mode kit server exposes only
its TLS gateway; friends of such a server use the launcher. The friend's guide is
[Join as a friend]({{ friends_page.url | relative_url }}), and `tools/make-friend-kit.ps1` builds
the zip.

Both scripts keep their files in `%APPDATA%\DauntlessRevived\`:

- `account.key`: the friend's account key. **Secret: it is the player's password; never share,
  never commit, and back it up.**
- `settings.json`: `{ Server, Game }`, with no secrets.

### setup.ps1 and Setup.cmd

A one-time setup, safe to run again. It checks the game exe against its pin, copies the two DLLs
into `Archon\Binaries\Win64` (checked before and after copying; taken from the kit's `dll\` folder,
or from `..\legacy\UndauntedLauncher\assets\` in a checkout), and warns when the Visual C++ runtime
(`MSVCP140.dll`, `VCRUNTIME140_1.dll`) is missing. Then it checks that the host answers on its port,
registers once (only when `account.key` does not exist yet), checks that the key works and saves
`settings.json`.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Server` | host, or `host:port` | Asked for when missing | The host's Tailscale address or name. Without a port it uses `61000`. |
| `-Game` | the folder that contains `Archon\` | `C:\D144\Dauntless` | The game install. |
| `-Zip` | path of the downloaded zip | none | Only checks the zip against the pinned SHA-256, then exits. Extract it yourself and run again without `-Zip`. |
| `-Username` | 3-16 letters, digits or `_` | Asked for when missing | The account name to register. |
| `-Invite` | invite code | Asked for when missing | The code from the host. |

Registration answers: 401 means the code is wrong or used up, 409 means the name is taken, and 400
means the server refused (registration closed, or the name is not allowed). The success line shows
`logged in as ''` with an empty name, because the server answers with `Username` and the script
reads `name`; the key check itself works.

`Setup.cmd` runs `powershell -NoProfile -ExecutionPolicy Bypass -File setup.ps1` with any arguments
you give it, and always pauses at the end.

### play.ps1 and Play Dauntless.cmd

Starts the game against the host. It refuses to start when the exe or a DLL does not match its pin.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Server` | host, or `host:port` | `settings.json` | The host. Without a port it uses `61000`. |
| `-Game` | folder | `settings.json` | The game install. |
| `-Graphics` | `-1` or 0-4 | `-1` (use the in-game menu) | 0-4 forces that quality level (4 = Cinematic) on every launch, in `Engine.ini` and, if that file exists, `GameUserSettings.ini`. A value above 4 is refused. |
| `-Windowed` | switch | off | Starts in a 1280x720 window. |
| `-DryRun` | switch | off | Checks everything and prints the launch line with the key hidden, without starting the game. It still rewrites `Engine.ini`. |

On every launch it rewrites the `[SystemSettings]` and `[OnlineSubsystemMcp.XMPP]` sections of
`%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\Engine.ini` and keeps the rest of the file.
[Game settings]({{ game_page.url | relative_url }}) lists the lines. It passes the account key on the
game's command line (`-AUTH_PASSWORD=<key>`), where other programs on the same PC can read it.

`Play Dauntless.cmd` runs `play.ps1` with any arguments you give it and pauses only on an error.

```powershell
.\setup.ps1 -Server 100.x.y.z -Game D:\Games\Dauntless144
.\play.ps1 -Graphics 3 -Windowed
.\play.ps1 -DryRun
```

## tools/

### dashboard.ts

Optional owner dashboard, run on the VPS with Bun: `bun --no-env-file tools/dashboard.ts`.
`Install-OwnerDashboard.ps1` installs the pinned Bun in `C:\Program Files\Bun` and runs it as a task.
Set `DASHBOARD_OWNER_KEY_FILE` to the existing owner key file; `DASHBOARD_BACKEND` defaults to
`http://127.0.0.1:61000`, `DASHBOARD_PORT` to `61110`, and `DASHBOARD_LOG_FILES` to `{}`
(an explicit JSON map of labels to log paths). The listener is loopback-only. Open it through
an SSH tunnel and enter the owner key. See the repository's `tools/DASHBOARD.md` for commands.
No installer or game service changes are required; this process is started separately.

Live five-second CPU/RAM, player counts and local backend response graphs retain one hour in memory.
Account totals include admins; newly observed accounts are counted since the first poll, because
historical registration dates do not exist. Logs refresh on demand, capped at 32 KB / 150 lines,
with credential-bearing lines omitted. Backend latency is not game ping. The dashboard reads only
`GetAllUsers` and `ServerStatus`; it performs no writes. History resets on restart.

Tests: `bun test dashboard` in `tools/`.

### summarize-performance.ts

Run `bun tools/summarize-performance.ts "C:\DauntlessRevived\data\logs\performance\performance-2026-09-28.csv"`
from a checkout with Bun. Pass additional CSV paths to combine days from the same host.
The tool only reads files and prints JSON; it does not contact or modify the server. Invalid rows
fail with a nonzero exit code. Input must use the kit sampler's unquoted CSV format.

For each role, the report gives the number of available readings, median, p95 and maximum for
process CPU, working-set memory, players, host CPU, free RAM and online players. Empty readings
are excluded, while measured zeroes count. Percentiles use nearest rank and weight each process
sample equally. Process CPU is percent of one core; host CPU is percent of the machine.
Hunt figures describe one process. Ramsgate's fixed overhead prevents interpreting memory divided
by players as the cost of adding a player. No safe capacity estimate is made yet; collect comparable
sessions at different player counts. Keep reports local with the other operational logs.

Tests: `bun test summarize-performance` in `tools/`.

Run these from a checkout, with Node.js, Bun or Windows PowerShell as noted. The `.ts` tools run on
Bun, the version in `tools/package.json` (`packageManager`); they need no `bun install`, which only
brings the type checker.

### make-friend-kit.ps1

Builds the friend-kit zip.

| Parameter | Type | Default | What it does |
|:----------|:-----|:--------|:-------------|
| `-Out` | folder | `C:\dr\dist` (the original host's layout; pass your own) | Where the kit folder and the zip go. |

It stages `<Out>\DauntlessRevived-FriendKit\` (deleted first if it exists) with `setup.ps1`,
`play.ps1`, `Setup.cmd`, `Play Dauntless.cmd`, `README.txt`, `THIRD-PARTY-NOTICES.txt`, the two DLLs
in `dll\` (copied from `legacy\UndauntedLauncher\assets` and checked against their pins), `LICENSE.txt`, a
`SOURCE.txt` and a `SHA256SUMS.txt`. Then it zips everything to
`<Out>\DauntlessRevived-FriendKit-<7-character commit>.zip` and prints the zip's path and SHA-256.

`SOURCE.txt` names the repository and the exact commit, which is the AGPL source offer for players.
The repository address is your `origin` remote without `.git`, so check that `origin` points at the
public repository before you hand the zip out. The script needs `git` and an `origin` remote, and it
refuses to run while `friend-kit\` has uncommitted changes.

```powershell
powershell -ExecutionPolicy Bypass -File tools\make-friend-kit.ps1 -Out D:\dist
```

### sync-roadmap.ts

`bun tools/sync-roadmap.ts` takes no arguments. It regenerates `docs/roadmap.md` from `ROADMAP.md`:
front matter, a table of contents, and the checklist inside a Liquid `raw` block, so Jekyll leaves
its text alone. Run it after every change to `ROADMAP.md`, and never edit `docs/roadmap.md` by hand. The Finnish
`docs/fi/roadmap.md` is a hand-written summary, not generated.

### build-llms.ts

`bun tools/build-llms.ts` takes no arguments. It writes `docs/llms.txt` (an index of every page) and
`docs/llms-full.txt` (every English page as Markdown in one file) from the pages' front matter and
text and from `docs/_data/faq_en.yml` and `faq_fi.yml`. Run it after adding a page or changing a
title or description, and after `sync-roadmap.ts`, because the roadmap page is one of its inputs.

It warns about a page without a `description`. It stops with an error on Liquid it does not
understand. Pages may use only:

- Liquid `raw` and `comment` blocks,
- the page-link pattern {% raw %}`{% assign x_page = site.pages | where: "path", "..." | first %}`{% endraw %} with {% raw %}`{{ x_page.url | relative_url }}`{% endraw %},
- {% raw %}`{{ '/path' | relative_url }}`{% endraw %},
- {% raw %}`{{ site.github.repository_url }}`{% endraw %},
- the FAQ loop over `site.data.faq_en` or `site.data.faq_fi`.

A link from the FAQ data to a page that does not exist is also an error, and so is a page link whose
`assign` names a page that does not exist.

### make-game-manifest.ts

Builds the content manifest, `legacy/UndauntedContent/data/dauntless-1.4.4.json`, from the verified game
zip. The content server serves only the files in it, and the launcher compiles the same file in and
refuses any other file. Run it only when the manifest must change.

```text
bun tools/make-game-manifest.ts --zip <BaseGame144.zip> [--out <file>] [--compare-dir <game folder>]
                                 [--compare-all] [--zip-sha256 <hex>] [--skip-zip-hash]
```

| Argument | Default | What it does |
|:---------|:--------|:-------------|
| `--zip <file>` | **required** | The verified 1.4.4 zip. |
| `--out <file>` | `legacy/UndauntedContent/data/dauntless-1.4.4.json` | Where the manifest goes. |
| `--compare-dir <folder>` | none | Afterwards, checks an extracted install against the new manifest: every file's size, and full hashes of the exe and a few others. Read-only. |
| `--compare-all` | off | With `--compare-dir`: hashes every file. |
| `--zip-sha256 <hex>` | the pinned zip hash | The hash the zip must have. |
| `--skip-zip-hash` | off | Skips hashing the whole zip. Only when you checked it yourself. |

It checks each entry's CRC-32 and size against the zip's own records, that every path is safe (the
same rule the content server applies), that no two paths differ only in letter case, that
`Version.txt` names the 1.4.4 build and that the exe matches its pin. It prints the file count, the
total bytes and the manifest's own SHA-256. It runs at low priority, reads the zip twice (about 21 GB
of reads), and exits with `1` on any failure. It reads the zip itself (`tools/zip.ts`, Zip64 included), so
nothing needs installing first. After regenerating the manifest, rebuild the launcher.

## CI workflows and tools {#ci-workflows-and-tools}

GitHub Actions runs the two workflows. You start them by hand from the repository's **Actions** tab,
and run the tools from a checkout. What each CI job checks, and how launcher releases work, is in the
[Developer guide]({{ dev_page.url | relative_url }}#ci). The repository variable
`LAUNCHER_AUTO_RELEASE`, which pauses automatic releases, is in
[Configuration]({{ config_page.url | relative_url }}#ci-settings).

| Workflow | Runs on | Inputs | What it does |
|:---------|:--------|:-------|:-------------|
| `.github/workflows/ci.yml` (**CI**) | Every push to any branch, every pull request, and Actions > **CI** > **Run workflow** | none | Builds and tests the four server packages, the launcher and the server kit, checks the docs and the repository, and keeps the launcher's release files for 7 days (not for pull requests from forks). Then, on a push to `dauntless-revived` in `mixutin/dauntless-revived` that passed every check, it publishes the launcher version if that version has no release yet, unless `LAUNCHER_AUTO_RELEASE` is `false`. |
| `.github/workflows/launcher-release.yml` (**Launcher release**) | Actions > **Launcher release** > **Run workflow** on `dauntless-revived`; called by `ci.yml` | None by hand. `ci.yml` passes `version` (it must equal the `legacy/UndauntedLauncher/package.json` version at that commit) and `artifact` (the release files it built). | Publishes the launcher version as the release `launcher-v<version>`, creates that tag, attests the build provenance of every file, and points the `launcher-updates` feed at it. Run by hand, it builds and tests the commit itself; for a version that is published already, it only brings the feed up to it. It refuses to run on any branch but `dauntless-revived`, the default branch. |

### tools/ci/check-repo.ts {#check-repots}

```text
bun tools/ci/check-repo.ts [--history <revision range>]
```

The repository hygiene check. Without arguments it checks every tracked file:

- no secret or game file by its name: `.env` files other than `.env.example`, keys and certificates,
  databases, logs, game archives and assets, executables, archives, DLLs other than the two pinned
  ones, anything under `data/` or `BaseGame144/`;
- no tracked file that a `.gitignore` rule excludes;
- no file identical to a file of the 1.4.4 game (by the hashes in the game manifest);
- nothing inside a file that is shaped like a secret: an account key, a JWT, a private key (also
  base64-encoded), a Tailscale key or a GitHub token;
- the two DLLs in `legacy/UndauntedLauncher/assets/` match their pins in the launcher and the server kit,
  and the launcher version in `legacy/UndauntedLauncher/package.json` is valid.

| Argument | What it does |
|:---------|:-------------|
| `--history <revision range>` | Also checks every file version that the commits in the range added or changed, for example `origin/dauntless-revived..HEAD` before a push. `--history HEAD` checks the whole history. A secret that was committed and deleted again is still in the history, so the key has to be replaced. |

It never prints what a match contains, only the file, the line and the kind of match. Exit code `0`
means nothing was found, `1` a problem, `2` a usage error. It has no dependencies, so nothing needs
installing first.

### tools/ci/launcher-version.ts {#launcher-versionts}

The launcher version rules that `check-repo.ts` and the release workflow share. A launcher version is
a semantic version without build metadata (`x.y.z` or `x.y.z-prerelease`), and it names the release
tag `launcher-v<version>`.

| Command | What it does |
|:--------|:-------------|
| `bun tools/ci/launcher-version.ts valid <version>` | Exit code `0` if it is a launcher version, `1` if not. |
| `bun tools/ci/launcher-version.ts compare <a> <b>` | Prints `-1`, `0` or `1` (semantic version order). |
| `bun tools/ci/launcher-version.ts newest <version> [<other>...]` | Exit code `0` if `<version>` is newer than every other one that counts: a release is compared with the other releases only, a prerelease with everything. Otherwise it prints the newer or equal one and exits with `1`. Others that are not launcher versions are skipped with a warning. |
| `bun tools/ci/launcher-version.ts nuget <version>` | Prints the version as Squirrel's package names spell it. |
| `bun tools/ci/launcher-version.ts from-nupkg <file name>` | Prints the version in a `*-full.nupkg` file name, or exits with `1` if there is none. |

A usage error, or a `<version>` that is not a launcher version where one is required, exits with `2`.

### collect-release.ps1 {#collect-releaseps1}

From `legacy/UndauntedLauncher`, after `npm run make`:

```powershell
pwsh -NoProfile -File scripts\collect-release.ps1
```

It takes no parameters. It empties `release\` and copies into it what a launcher release publishes:
`DauntlessRevivedLauncher-Setup.exe`, the Squirrel update files (`RELEASES` and the full `.nupkg`) and
the portable zip, renamed `DauntlessRevivedLauncher-<version>-win32-x64.zip`. Then it writes
`SHA256SUMS.txt` for all of them and prints it. It stops if one of the files is missing. CI and the
release workflow run it; `release\` is git-ignored.

## npm scripts

Run each one in its package folder after `npm ci`. Arguments for the script itself go after `--`,
for example `npm run verify -- --quick`. Which tests use which ports, and which suites must not run
at the same time, is in [Developer guide]({{ dev_page.url | relative_url }}).

**UndauntedMetagame**

| Script | Runs | What it does |
|:-------|:-----|:-------------|
| `npm run dev` | `tsx watch --env-file=.env src/server.ts` | Runs from source and restarts on changes. Fails without a `.env` in the folder. |
| `npm run build` | `tsc`, then `postbuild` | Compiles to `dist\`. `postbuild` runs `scripts/write-build-info.js`, which writes `dist\build-info.json` with the commit, the package version and the build time. The commit comes from git, with `-dirty` when `legacy/UndauntedMetagame` has changes, or is `unknown` without git (as in the kit's builds). `GIT_COMMIT` and `SERVER_VERSION` in `.env` override it at run time. |
| `npm start` | `node --env-file=.env dist/server.js` | Runs the build. Fails without a `.env`. Start it from the package folder: the database migrations are found relative to the working directory. |
| `npm run db:generate` | `drizzle-kit generate` | Writes a new migration to `src/drizzle` after a change to `src/db/schema.ts`. Reads `DB_FILENAME` from `.env`. |
| `npm test` | `tsc -p tsconfig.test.json`, then `node --test` | Deletes `build\`, compiles the tests and runs them. |

**UndauntedDeployServer**

| Script | Runs | What it does |
|:-------|:-----|:-------------|
| `npm run dev` | `tsx --env-file=.env src/server.ts` | Runs from source (no watching). Fails without a `.env`. |
| `npm run build` | `tsc` | Compiles to `dist\`. |
| `npm start` | `node --env-file=.env dist/server.js` | Runs the build. Fails without a `.env`. |
| `npm test` | as in the metagame | Compiles and runs the tests. |

**UndauntedGateway**

| Script | Runs | What it does |
|:-------|:-----|:-------------|
| `npm run build` | `tsc` | Compiles the gateway and the allowlist helper to `dist\`. |
| `npm start` | `node --env-file-if-exists=.env dist/server.js` | Runs the gateway. A missing `.env` is not an error. |
| `npm run start:allowlist` | `node --env-file-if-exists=.env dist/allowlist/server.js` | Runs the allowlist helper. It changes a firewall rule, so it must run elevated unless it is in dry-run. With `--close-ports` after the script (`node --env-file=.env dist/allowlist/server.js --close-ports`) it only turns the game-port rule off once and exits (exit code 0 or 1), for stop scripts; it reads the same settings, so `ALLOWLIST_DRY_RUN` and `ALLOWLIST_SECRET` must be set. `Stack.ps1` does not use it: it turns the rule off itself. |
| `npm run make-cert -- <arguments>` | `node tools/make-cert.js` | Makes the gateway's self-signed certificate. See below. |
| `npm test` | `tsc -p tsconfig.test.json`, then `node --test --test-concurrency=1` | Deletes `build\`, compiles the tests and runs them one file at a time. |

`tools/make-cert.js` makes an RSA-2048, SHA-256 certificate valid for 10 years, with the given
addresses as its names, and prints its SHA-256 fingerprint, which is the `fp` of every v2 invite:

| Argument | What it does |
|:---------|:-------------|
| `--host <IP or DNS name>` | A name for the certificate. Repeat it for more names. At least one is required. |
| `--out <folder>` | Writes `gateway-cert.pem` and `gateway-key.pem` into that folder. |
| `--cert <file> --key <file>` | Writes to these two files instead of `--out`. |
| `--force` | Overwrites existing files. Without it the tool refuses, because a new certificate breaks every invite already sent. |
| `--json` | Prints the result as one JSON line. |
| `--fingerprint <cert.pem>` | Only prints the names, validity and fingerprint of an existing certificate. |

**`gateway-key.pem` is secret: never share, never commit.** Keep it in a folder only
Administrators and the gateway's account can read. The Windows server kit runs this tool itself; you
need it only for a gateway you set up by hand. Exit code `2` means a usage error, and `1` any other
failure.

**UndauntedContent**

| Script | Runs | What it does |
|:-------|:-----|:-------------|
| `npm run build` | `tsc` | Compiles to `dist\`. |
| `npm start` | `node --env-file-if-exists=.env dist/server.js` | Runs the content server. |
| `npm run verify -- [--quick] [--game-dir <folder>] [--manifest <file>]` | `node --env-file-if-exists=.env dist/verify.js` | Hashes every game file against the manifest and lists what is missing, the wrong size or the wrong hash. `--quick` compares sizes only. The defaults are `CONTENT_GAME_DIR` and `CONTENT_MANIFEST` from `.env`, else the built-in manifest. Read-only and at low priority. Exit code `0` all match, `1` a problem, `2` a usage error. Run it after copying the game onto a new host. |
| `npm test` | `tsc -p tsconfig.test.json`, then `node --test` | Deletes `build\`, compiles the unit tests and runs them. |
| `npm run test:integration` | `tsc`, `tsc -p tsconfig.test.json`, then `node --test --test-concurrency=1` | Builds the server, then runs it against a real install (`CONTENT_IT_GAME_DIR`, default `C:\D144\Dauntless`; skipped when there is no 1.4.4 install there) with a mock metagame, on ports 62002 and 62003 (`CONTENT_IT_PORT`, `CONTENT_IT_MOCK_PORT`). |

**UndauntedLauncher**

| Script | Runs | What it does |
|:-------|:-----|:-------------|
| `npm start` | `electron-forge start` | Runs the launcher in development. |
| `npm run package` | `electron-forge package --platform win32` | Packages the app without an installer. |
| `npm run make` | `electron-forge make --platform win32` | Builds `DauntlessRevivedLauncher-Setup.exe` (Squirrel) and a zip. |
| `npm run typecheck` | `tsc --noEmit -p tsconfig.json` | Type-checks without building. |
| `npm test [-- <filter>]` | `node scripts/run-tests.mjs` | Compiles the tests into `.test-build\` and runs them one file at a time. With a filter, only test files whose name contains it. |
| `npm run icon [-- --check]` | `node scripts/make-icon.mjs` | Copies the launcher's icons (`assets/icon.ico`, `assets/icon.png`) and the logo and emblem images its window shows (`src/renderer/brand/`) from `brand/`, where `python3 brand/build.py` makes them. With `--check` it only compares, and exits with 1 if a copy is missing or out of date; the unit tests run that. |

The launcher needs Node.js 20.19 or newer (`engines` in its `package.json`). The server kit installs
Node.js 24.19.0 on a server.

### Other package scripts

| Script | Run as | What it does |
|:-------|:-------|:-------------|
| `legacy/UndauntedMetagame/scripts/write-build-info.js [out folder]` | the `postbuild` step; tests pass another folder | Writes `build-info.json` (default folder: `dist`). |
| `legacy/UndauntedMetagame/scripts/make-hunt-titles.js` | `node scripts/make-hunt-titles.js` in `legacy/UndauntedMetagame` | Rebuilds `src/vendor/hunt_titles.json`, the hunt names that `ServerStatus` shows, from the deploy server's hunt tables in `legacy/UndauntedDeployServer/src/vendor`. Rerun it after those tables change. |
| `legacy/UndauntedGateway/tools/make-cert.js` | `npm run make-cert -- ...` | See above. |
| `legacy/UndauntedLauncher/scripts/run-tests.mjs`, `make-icon.mjs` | `npm test`, `npm run icon` | See above. |
| `legacy/UndauntedLauncher/scripts/collect-release.ps1` | `pwsh -NoProfile -File scripts\collect-release.ps1` in `legacy/UndauntedLauncher`, after `npm run make` | Collects the release files in `release\`; see [collect-release.ps1](#collect-releaseps1). |

## Scripts printed in the guides

Some guides print a short script for you to save and run. They are not files in the repository, and
their parameters are documented where they are printed:

| Script | Where | Parameters |
|:-------|:------|:-----------|
| `verify-manifest.js` | [Host a server, step 2]({{ host_page.url | relative_url }}#verify-the-build) | `node verify-manifest.js <game folder>` (default: the current folder) |
| `make-gameini.ps1` | [Host a server, step 8]({{ host_page.url | relative_url }}#game-ini) | `-Metagame` (default `127.0.0.1:61000`), `-Source` (the DLL's `dllmain.cpp`), `-Out` (the user `Game.ini`) |
| `play.ps1` (the host's) | [Host a server, step 13]({{ host_page.url | relative_url }}#launch-the-client) | `-Backend` (default `127.0.0.1:61000`), `-Graphics` (default `4`; `-1` = use the menu), `-Windowed`, `-CapMB` (default `12000`), `-Seconds` (default `0`), `-KeyFile` (the account key to log in with; default `C:\dr\data\owner.key`; **secret: never share, never commit**) |
| `play.ps1` (manual friend version) | [Join as a friend]({{ friends_page.url | relative_url }}) | `-Server` (required), `-Game` (default `C:\D144\Dauntless`), `-Windowed` |
| `rekey.js` | [Run it for a group]({{ admin_page.url | relative_url }}#missing-admin-functions-and-workarounds) | `node rekey.js <UserId>`. **Its output file holds a live account key: hand it over privately, then delete it.** |
| `backup-db.js` | [Back up the database]({{ admin_page.url | relative_url }}#back-up-the-database) | none |
| `seed-progression.js` | [Seeding a player]({{ upgrade_page.url | relative_url }}#seeding-a-player) | `<admin key file> list`, or `<admin key file> <UserId> grandfather` or `fresh`; the `METAGAME` variable sets the metagame address (default `http://127.0.0.1:61000`) |

The guides also mention the original host's own `stack.ps1` and `backup.ps1` in `C:\dr\tools`. Those
are not published. On a kit server, `Stack.ps1` and `Backup-DauntlessServer.ps1` do the same jobs,
but they work only on a server installed with the kit.
