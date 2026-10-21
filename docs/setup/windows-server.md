---
title: Windows server kit
parent: Setup
nav_order: 6
description: "Install a Dauntless Revived server on a rented Windows Server 2019 VPS in public mode: one TLS gateway port with a pinned certificate, game ports opened only for logged-in players, key-only SSH deployment, invites, updates and backups."
lang: en
ref: setup/windows-server
---

{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}
{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}
{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}
{% assign legal_page = site.pages | where: "path", "legal.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "reference/scripts.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign chat_page = site.pages | where: "path", "findings/chat.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}

# Windows server kit
{: .no_toc }

The folder `deploy/windows-server/` in the repository installs a complete Dauntless Revived server on
a Windows Server 2019 machine, typically a rented VPS with a public IP address. You run one command on
your own PC; it connects to the server over SSH, uploads everything, installs it and prints what
friends need to connect.

The kit has two modes:

- **Public mode** (the default): friends connect over the internet with nothing but the friend
  launcher and an invite. One TLS port is open to the world; everything else stays on the server.
- **Private mode**: friends connect over Tailscale, as described in
  [Run it for a group]({{ admin_page.url | relative_url }}).

**Status (22 September 2026).** The kit is built and tested on a development PC in its sandbox mode (a
full public-mode install into a scratch folder, with the gateway, the pinned certificate, invites and a
restore from backup checked end to end). On 21–22 September 2026 it was deployed in public mode to a
real rented Windows Server 2019 VPS. Checked on that server: the stack starts at boot as the service
account in session 0, Ramsgate runs and sends heartbeats, the gateway answers from the internet with
the pinned certificate, and the hourly backup task runs. On 22 September 2026 the owner played there
over the internet: registered through the launcher with an invite, downloaded the game through the
gateway, and played the tutorial, Ramsgate, the Training Dojo and the first hunt. Three game servers
ran at once, and the UDP allowlist opened the game ports for the player and closed them after they
left. The real server found three problems the sandbox could not, all fixed in the kit: Windows
limits a local account's description to 48 characters, that
image refuses scheduled tasks without a stored password ("S4U") for accounts that are not
administrators, and the provider's image kept the firewall off with policy values (the last two are
explained in the install steps below). A test with a second player is next;
private mode has not been run on a real server. See items 1.15 to 1.17 on the
[roadmap]({{ roadmap_page.url | relative_url }}).

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## How public mode works

The 1.4.4 client speaks plain HTTP and cannot be taught TLS. So each friend's launcher runs a small
relay on their own PC, and only that relay talks to the internet:

| Step | Where | What happens |
|:-----|:------|:-------------|
| 1 | Friend's PC | The game talks plain HTTP to `127.0.0.1:61000`: the launcher's relay. |
| 2 | Internet | The relay forwards every request over **TLS** to the server's gateway (port 443 by default). It accepts only the one certificate whose SHA-256 fingerprint came in the invite, so nobody in between can read or change the traffic, and no domain name or certificate authority is needed. |
| 3 | Server | The **gateway** is the only public TCP port. It passes requests to the metagame and the content server, which listen on `127.0.0.1` only. Of the `/undaunted/api` routes it passes only the four a launcher needs (register, key check, server status, registration mode), so every admin route stays on the server. It refuses anything carrying the game-server key, limits request sizes and rates, and never logs keys or tokens. |
| 4 | Server | When a player logs in (or their game sends its heartbeat), the gateway tells the **allowlist helper** that player's address. The helper keeps one Windows Firewall rule, "Dauntless Revived game ports (allowlist)", that opens UDP 8770-8777 to exactly those addresses. An address drops out 10 minutes after its last heartbeat. Everyone else is dropped by the firewall. |
| 5 | Friend's PC | The game's UDP traffic goes straight to the server's public address, through that rule. |

The metagame hands every player the QoS address `http://127.0.0.1:61000/QoS`, which is each friend's
own relay. That is why the relay port is fixed at 61000.

### What is reachable from the internet

| Port | Open to | Notes |
|:-----|:--------|:------|
| TCP 443 (`-GatewayPort`) | Everyone | The gateway, TLS only, `gateway.exe` only. |
| UDP 8770-8777 | Only addresses of players who logged in | The allowlist rule; closed while nobody plays and whenever the stack is stopped. Only the game executable may receive. |
| TCP 22 | Everyone, **key login only** | OpenSSH, for your own administration. A deploy over SSH turns password login off (key login is already proven); otherwise the installer warns. Limit this to your own address at the provider. |
| TCP 3389 | Only `-AdminIp` | Remote Desktop, if you use it. Without `-AdminIp` the installer **turns off** any internet-open RDP rule (use key-only SSH instead); `-KeepRdpOpen` leaves it open. |
| Everything else | Nobody | Every firewall profile is set to block inbound connections by default. |

Not reachable from outside, ever: the metagame (61000), the content server (61002), the deploy server
(61001, which has no authentication), the allowlist helper (61005).

## What you need

- **A Windows Server 2019 VPS** (2022 also works) **with the Desktop Experience**. Server Core cannot
  run the game servers: they are the game's own executable and need the desktop DLLs.
  At least **8 GB RAM**, 4 vCPU and **40 GB of disk**, plus a **public IPv4 address**.
  Ramsgate takes about 1.1 GB of RAM, each hunt about 1 GB.
- In the provider's own firewall or security group (many have one, blocking inbound by default): allow
  **TCP 443 and UDP 8770-8777 from any address**, and TCP 22 (and RDP 3389, if you use it) only from
  your own address. See "Before friends join" below; without the UDP rule friends hang loading Ramsgate.
- **The verified Dauntless 1.4.4 zip** (SHA-256 `556B9A64...BC6D`) on your PC, or an https link the
  server can download it from.
- **Your PC:** a checkout of the repository, Windows 10 or 11 with the built-in OpenSSH client, and an
  SSH key pair (for example `ssh-keygen -t ed25519 -f C:\dr\data\ssh\dauntless_deploy`). Keep the
  private key file readable by your own account only.
- Optional: a backup of your current server (the folder `backup.ps1` or `Backup-DauntlessServer.ps1`
  makes), to move every account, save and key to the new server.

## One-time server preparation

Do this once, over Remote Desktop or the provider's web console, signed in as Administrator. It turns
on SSH with your key only; after that you never need the password again.

```powershell
# 1. Install and start the OpenSSH server
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Set-Service sshd -StartupType Automatic
Start-Service sshd

# 2. Allow your public key (paste the ONE line from dauntless_deploy.pub) for administrators
$k = 'C:\ProgramData\ssh\administrators_authorized_keys'
Set-Content -Path $k -Value 'ssh-ed25519 AAAA... your-key-comment' -Encoding ascii
icacls $k /inheritance:r /grant '*S-1-5-32-544:F' /grant '*S-1-5-18:F'   # Administrators, SYSTEM

# 3. Print this server's SSH host-key fingerprint, to check on the first connection
Get-ChildItem C:\ProgramData\ssh\ssh_host_ed25519_key.pub | ForEach-Object { ssh-keygen -lf $_.FullName }
```

Note the `SHA256:...` fingerprint that step 3 prints (in the provider console, where nothing is in the
middle). Then test from your PC, writing the host key into the same `known_hosts` the kit uses, and
compare the fingerprint it shows with the one from step 3:

```powershell
ssh -o UserKnownHostsFile=C:\dr\data\ssh\known_hosts -i C:\dr\data\ssh\dauntless_deploy Administrator@<server address> hostname
```

If they match, answer `yes`. `Deploy-Remote.ps1 -HostKeyFingerprint SHA256:...` does the same check for
you and refuses to upload a backup to an unverified host. When key login works, turn password login off
in `C:\ProgramData\ssh\sshd_config` (set `PasswordAuthentication no`, then `Restart-Service sshd`). A
public-mode install that runs over SSH, as `Deploy-Remote.ps1` does, makes that change itself, because
the key login is proven by then. Never send your server password to anyone, and never paste it into a
script.

At the provider, allow **TCP 22 only from your own address**. If your home IP address changes, update
that provider rule and re-run the installer with the new `-AdminIp` (over SSH).

## Deploy from your PC

**Before the first deploy.** Public mode ships a `git archive` of your current commit (`HEAD`). If the
gateway and content server are not committed yet, that archive would not contain them and the install
would fail on the server, but only after the ~10.5 GB game zip had already gone up. So either **commit
and push your work first**, or pass **`-WorkingTree`** to deploy your current files (never keys or
`.env`). The example below uses `-WorkingTree`; drop it once your work is committed. `Deploy-Remote.ps1`
now checks for this and stops before uploading anything if `HEAD` is missing the gateway.

The 10.5 GB game zip takes hours over a home connection (about 2.5 h at 10 Mbit/s). `-GameZipUrl
https://...` lets the server download it instead, and `-UploadOnly` uploads the day before without
running the installer.

From the repository folder on your PC:

```powershell
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 `
    -WorkingTree `
    -HostKeyFingerprint SHA256:... `
    -GameZip D:\BaseGame144.zip `
    -RestoreFrom C:\dr\backups\2026-10-01_200000 `
    -AdminIp 198.51.100.20 `
    -ServerName "Saturday Ramsgate" `
    -ServerBinaries C:\dr\server-x86_64-pc-windows-msvc
```

`-ServerBinaries` is the folder with the server's programs for that code: the
`server-x86_64-pc-windows-msvc` download of the commit's CI run (Actions, the run, Artifacts), or
leave it out when `rust\target\release` in your checkout holds a build (`cargo build --release`).
They are checked against their `SHA256SUMS` on the server.

Use `-OwnerName <your username>` instead of `-RestoreFrom` for a brand-new server with an empty
database. What happens:

1. It connects with the key in `C:\dr\data\ssh\dauntless_deploy` (change with `-KeyFile`), in batch
   mode: no password or keyboard prompts. The server's host key is recorded on the first connection in
   `known_hosts` next to the key file, and every later connection must match it.
2. It uploads the kit and the server code (a `git archive` of your current commit; `-WorkingTree`
   sends uncommitted work too, never ignored files, keys or `.env` files).
3. With `-RestoreFrom`, it uploads the backup into a folder only Administrators and SYSTEM can open,
   and deletes that copy after a successful install.
4. It uploads the game zip in 256 MB parts. The zip is checked against the pinned SHA-256 on your PC
   first, each part is checked on the server after it arrives, and the whole file again when it is put
   together. **If the upload stops, run the same command again**: it continues with the parts that are
   missing. `-GameZipUrl https://...` makes the server download the zip itself instead.
5. It runs the installer on the server and shows its output as it goes.
6. It prints the server's address and certificate fingerprint, and checks from your PC that the
   gateway answers with that certificate.

`-WhatIf` shows the plan without connecting. `-UploadOnly` stops before the installer runs.

### What the installer does

`Install-DauntlessServer.ps1` can also be run directly on the server (for example over Remote
Desktop). Every step is safe to repeat, and `-WhatIf` lists every change without making it.

1. **Preflight:** administrator, Windows Server 2019 (build 17763) or newer, Desktop Experience,
   8 GB RAM, 40 GB disk budget, free ports.
2. **Prerequisites:** the Visual C++ 2015-2022 x64 runtime and the DirectX June 2010 runtime (both
   checked by their Microsoft signature). All downloads use TLS 1.2. No Node.js.
3. **Server code:** copied to `C:\DauntlessRevived\app`, with the server's programs in
   `C:\DauntlessRevived\app\bin`: `metagame.exe`, `deploy.exe`, `content.exe`, `gateway.exe` and
   the gateway's `allowlist.exe` (from `-ServerBinaries`, checked against their `SHA256SUMS`).
4. **Game files:** the zip's SHA-256 is checked, it is extracted with Windows' own `tar.exe`, every
   file is checked against the content manifest, and the two server DLLs are installed with their
   pinned hashes.
5. **Service account:** a local user `dauntless` with a random password that is never shown. The stack
   runs as this account, never as an administrator. Task Scheduler keeps the password, encrypted, for
   the account's tasks, and every run of the installer sets a new one: some Server 2019 images refuse
   tasks that run without a stored password ("S4U") for any account that is not an administrator.
   The game servers run as this account and read its game config, so the installer also writes the
   account's `Game.ini` (the 167 endpoint overrides, pointing at this server's metagame) and
   `Engine.ini` (memory lines, chat pointed at `127.0.0.1`). See
   [Game settings]({{ gamesettings_page.url | relative_url }}#game-ini).
6. **Certificate:** a self-signed certificate for the gateway (10 years, the public address as its
   name), made by the gateway's own tool. No Windows certificate store is touched. The fingerprint is
   printed; it goes into every invite.
7. **Settings:** fresh token-signing keys, a game-server key, the gateway secret and the allowlist
   secret (or the ones from the backup), registration by invite code only. Files holding secrets are
   readable only by Administrators, SYSTEM and the service account; the allowlist helper's settings
   only by Administrators and SYSTEM. Nothing secret is ever printed.
8. **Database and owner account:** the owner (admin) account is created, its key saved to
   `data\keys\owner.key`; or the database and keys come from the backup.
9. **Firewall** as in the table above. The installer checks the firewall that is actually in effect,
   not only its normal settings: some VPS images keep Windows Firewall off with a policy value
   (`EnableFirewall = 0` under `HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall`) while the normal
   settings say it is on, which leaves every port open to the internet. The installer removes such
   values. Windows applies the change only after a restart, so the installer then prints **RESTART
   THIS SERVER NOW** in red.
10. **Scheduled tasks:** the stack at startup as `dauntless` (supervised: a crashed part is restarted),
    the allowlist helper at startup as SYSTEM (it changes only its one firewall rule), and an hourly
    backup.

The most used parameters are below. [Scripts and parameters]({{ scripts_page.url | relative_url }})
lists all of them, and those of every other kit script.

| Parameter | Default | Meaning |
|:----------|:--------|:--------|
| `-Mode` | a new install: `Public`; a re-run: the installed mode | `Public` or `Private` (Tailscale). |
| `-GameZip` / `-GameZipUrl` | | The verified 1.4.4 zip, or an https link to it (resumable download). |
| `-PublicHost` | the address stored by the last run, else the one public IPv4 on the network adapters | The address or DNS name friends connect to. Needed when the VPS is behind 1:1 NAT. |
| `-GatewayPort` | the port stored by the last run, else `443` | The gateway's TCP port. |
| `-AdminIp` | the list stored by the last run | Address(es) allowed to use Remote Desktop, for example `198.51.100.20` or `198.51.100.0/24`. |
| `-KeepRdpOpen` | | Public mode without `-AdminIp`: leave internet-open RDP rules open instead of disabling them. |
| `-RestoreFrom` | | A backup folder to move an existing server here. |
| `-OwnerName` | | Username of the admin account on a new server (3-16 letters, digits or `_`). |
| `-ServerName` | `Dauntless Revived` | The name friends see, in the launcher and in the game's welcome text. Not remembered: a re-run without it sets the name back to `Dauntless Revived`. |
| `-InstallRoot` | `C:\DauntlessRevived` | Where everything goes. |
| `-InteractiveSession` | | The session-0 fallback, see below. Not remembered: a re-run without it turns the auto-logon off. |
| `-NewCertificate` | | Make a new certificate. **Every invite handed out before stops working.** |

`Deploy-Remote.ps1` passes `-Mode` on every install run (default `Public`), and in public mode also
`-PublicHost` (default: the `-Server` address) and `-GatewayPort` (default `443`). When you deploy
again to a server you already set up, repeat `-Mode Private`, a `-PublicHost` that differs from the
SSH address, a gateway port other than 443, `-ServerName` and `-InteractiveSession` if you used them.
`-Update` does not run the installer.

## Play yourself first

Prove the whole path from a real outside address before you invite anyone. Note that your own PC's
launcher needs port 61000 for its relay, which is the same port the local development stack uses, so
your own **PLAY fails with `relay_port_busy` while that stack is running**.

1. On nights you play on the rented server, stop the local stack (`C:\dr\tools\stack.ps1 stop`), or
   simply do not start it.
2. Install the launcher and make an invite for yourself: `Deploy-Remote.ps1 -Server <address>
   -InviteFor <you>`.
3. Register, install and play until Ramsgate loads.
4. On the server, check that `Stack.ps1 status` lists your address in the allowlist and shows Ramsgate
   on UDP 8777.
5. Only then send invites to friends.

`DAUNTLESS_REVIVED_RELAY_PORT` moves the relay off 61000, but that is only for tests; friends always use
61000.

## Before friends join

Run through this checklist once the server is deployed:

1. **At your provider** (cloud firewall / security group, e.g. AWS security groups, Azure NSGs,
   Hetzner/Vultr cloud firewalls), allow inbound **from any address**: TCP `-GatewayPort` (443 by
   default) and **UDP 8770-8777**. Many providers block inbound by default, and this server's own
   allowlist already limits UDP to logged-in players. Your own PC cannot test the UDP path.
2. At the provider, limit **TCP 22** and **TCP 3389** to your own address.
3. Hand out the launcher. There is no in-launcher download link: friends get it from
   **`https://github.com/mixutin/dauntless-revived/releases/latest`**
   (`DauntlessRevivedLauncher-Setup.exe`). It is unsigned, so Windows SmartScreen warns the first time:
   **More info > Run anyway**. On a PC set to block unrecognised apps, SmartScreen blocks it outright
   with no Run anyway; then the friend checks the file's SHA-256 against `SHA256SUMS.txt` from the same
   release and unblocks it (right-click > **Properties** > **Unblock**, or `Unblock-File`), as
   [Join as a friend]({{ friends_page.url | relative_url }}) explains. Code-signing is roadmap item
   4.16. CI publishes every new version in `legacy/UndauntedLauncher/package.json` by itself once all
   checks pass (the repository variable `LAUNCHER_AUTO_RELEASE` set to `false` pauses that), and
   Actions > **Launcher release** > **Run workflow** on `dauntless-revived` publishes by hand (see
   "Releases and updates" in the launcher's README). Or send the `Setup.exe` with its SHA-256 through
   a private channel.

If a friend **logs in and then hangs loading Ramsgate**, the provider is almost certainly blocking
UDP 8770-8777. Add the provider rule above, and check the server's own allowlist:
`Get-NetFirewallRule -Name DauntlessRevived-GamePorts-Allowlist | Get-NetFirewallAddressFilter`.

## Invites

From your PC:

```powershell
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -InviteFor Alex
```

or on the server: `C:\DauntlessRevived\bin\New-Invite.ps1 -For Alex`. It creates a one-use invite
code (`-Uses` for more), first checks that the gateway answers with the right certificate, and prints
one line:

```
dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=<64 hex characters>&code=ABCD-EFGH-JKLM&name=Saturday%20Ramsgate
```

The friend pastes it into the launcher. The code lets one person create an account; send it privately.
`-List` shows the codes that are still open, `-Revoke <code>` deletes one.

Friends get the launcher from
[github.com/mixutin/dauntless-revived/releases/latest](https://github.com/mixutin/dauntless-revived/releases/latest)
(`DauntlessRevivedLauncher-Setup.exe`). It is unsigned, so Windows may warn that the app is
unrecognised: **More info > Run anyway**. If Windows blocks it with no Run anyway, the friend checks
it against `SHA256SUMS.txt` and unblocks it (see [Join as a friend]({{ friends_page.url | relative_url }})).
Keep the launcher open while playing.

The `fp` is the certificate's fingerprint. As long as you keep the certificate, old invite lines keep
working, even after a restore to a new server with the same address or DNS name (backups include the
certificate). Friends who already have an account (moved over with `-RestoreFrom`) still need an invite
line to find the server: only the invite carries the host and certificate fingerprint. In the launcher
they choose **"I already have an account key"** and paste the contents of their old `account.key`
instead of registering. If they press REGISTER, they spend the invite code and end up with a second,
empty account.

## Day to day

All of these run on the server (over SSH: `ssh -i <key> Administrator@<server>`, then `powershell`):

| Command | What it does |
|:--------|:-------------|
| `C:\DauntlessRevived\bin\Stack.ps1 status` | Every component with its process and ports, the gateway's TLS check, the allowlist, the game servers, the scheduled tasks and the last backup. |
| `Stack.ps1 stop` / `start` / `restart` | Stops or starts everything through the scheduled tasks. Stopping also closes the game ports. `restart -Only gateway` restarts one part. |
| `Get-ServerStatus.ps1` | Who is online and which worlds and hunts are running, asked through the gateway like a friend would. The server shows that list to registered players only, so the script asks with the owner key (run it as administrator). From your PC: `Get-ServerStatus.ps1 -Invite '<invite line>' -KeyFile <your account.key>`. Without a key it says the list is hidden. |
| `Backup-DauntlessServer.ps1` | A backup now (also runs every hour, and around every start and stop). |
| `Write-PerformanceLog.ps1 -Once` | A performance sample now (run it as administrator). The stack takes one every minute by itself, into `data\logs\performance\`: CPU and memory per game server and component, the machine's CPU, RAM, disk and network, and player counts. See [Measure it]({{ admin_page.url | relative_url }}#measure-it). |
| `Update-DauntlessServer.ps1 -Ref <tag>` | New server code; see below. |
| `Set-Chat.ps1 -On` / `-Off` | Turns the in-game text chat on or off; see [Chat](#chat) below. Restarts the stack. |

From your PC, `Deploy-Remote.ps1 -Server <address> -Status` shows the status without logging in.

### Chat {#chat}

The game's text chat (Ramsgate and hunt chat, party chat, guild chat and whispers, with usernames) is
a listener inside the metagame on `127.0.0.1:61099`. The gateway already forwards the game's chat
connection to it, so there is **no firewall rule to open**, and friends need no new launcher. It is
on by default on a new public install. Existing saved Off settings remain off; use `Set-Chat.ps1 -On`
once to enable an older install. How it works: [Text chat]({{ chat_page.url | relative_url }}).

**Switch it when nobody is playing.** Turning chat on or off restarts the stack, and a restart drops
the parties and matchmaking queues, which live in memory.

**On a server installed before chat, update first.** `Set-Chat.ps1` comes with the code that has chat,
so a server on an older version does not have it yet: run `Deploy-Remote.ps1 -Server <address> -Update`
(or `Update-DauntlessServer.ps1` on the server), then switch chat on. `-Chat` cannot go with
`-Update`, so that is two runs and two restarts.

- On the server: `C:\DauntlessRevived\bin\Set-Chat.ps1 -On` (or `-Off`). It writes `CHAT`,
  `CHAT_BIND_HOST=127.0.0.1` and `CHAT_PORT` in `metagame.env` and `"Chat"` in `server.json`, restarts
  the stack with the usual backups, and waits up to 30 s for the listener. If that fails, it puts the
  old settings back and restarts again.
- From your PC: `Deploy-Remote.ps1 -Server <address> -Chat On` runs the same script over SSH.
- On an install: `-Chat On` or `-Chat Off` (the installer and `Deploy-Remote.ps1` both take it). A re-run
  without `-Chat` keeps the saved choice, and an update never changes it.

`Stack.ps1 status` has a `chat` line: `listening 127.0.0.1:61099`, `off`, or
`on in metagame.env but not listening (see the metagame log)`; `Get-ServerStatus.ps1` on the server
shows the same. The metagame log starts with `chat: listening on 127.0.0.1:61099 (nick check enforce)`
and then has `chat:` lines for connections, logins, room joins and messages (never their text).
[Troubleshooting]({{ trouble_page.url | relative_url }}#chat-not-connected) explains each one.

For the first live runs, `CHAT_TRACE=1` in `metagame.env` logs every chat frame with logins, message
text and tokens replaced; add it before `Set-Chat.ps1 -On`, whose restart picks it up, and take it out
again afterwards. If real players are refused room joins with `reason=nick-resource`, `nick-format`
or `nick-name`, `CHAT_NICK_CHECK=log` admits them with a warning while you report the line.
`Set-Chat.ps1 -Off` is the way back from anything serious: the game then retries every 15-45 s,
harmlessly, as before chat existed. If the chat code itself is at fault,
`Update-DauntlessServer.ps1 -Rollback` goes back to the build before the update; older code ignores
`CHAT`. Private mode has no chat yet.

**Friends' online status** is automatic and on by default. Players need no manual status setting.
An existing `CHAT_PRESENCE=0` in `metagame.env` remains an explicit opt-out: remove it and restart
the metagame when nobody is playing (`Stack.ps1 restart -Only metagame`). The log says
`chat: friends' online status on`. Run the two-player party/hunt test on
[Text chat]({{ chat_page.url | relative_url }}#how-to-verify-presence); set `CHAT_PRESENCE=0` to roll back.

### Other features you switch in `metagame.env` {#other-switches}

The kit writes none of the switches that came with the port of Harmonic's fork, so a kit server runs
them at their defaults: Slayer Links and friends' online status on; Escalation (`ESCALATION_MODE`)
and the store (`STORE`) off. To change one, add the line to `C:\DauntlessRevived\data\config\metagame.env`
(deploy server: `deployserver.env`) from an elevated editor and restart that component when nobody is
playing. The kit keeps the line across re-runs and updates. What each does and what to check first is
on [Run it for a group]({{ admin_page.url | relative_url }}#switching-features-on).

### Updates

`Deploy-Remote.ps1 -Server <address> -Update` uploads your current commit and runs
`Update-DauntlessServer.ps1` on the server (pass `-ServerBinaries` for the new commit, as for the
install). It puts the new code and programs in place while the server keeps running, takes a backup,
switches over, and checks that the metagame answers and the gateway answers with the
invite certificate. If that check fails within 3 minutes, it switches back to the previous build by
itself. `Update-DauntlessServer.ps1 -Rollback` does the same by hand. Settings (apart from `GIT_COMMIT` in
`metagame.env`, which records the new commit), keys, the certificate,
the game files and the service account's `Game.ini` and `Engine.ini` are left alone; run the
installer again for those. Do that after an update that changes the DLL's endpoint table, and in
private mode when the server's Tailscale address changes.

**The player list is for registered players only: the launcher comes first.** The server shows who
is online only to a caller with an account key, and the launcher sends the player's key to see the
list. Launchers released before that change send no key. Against a server that has it, they show
"0 players online" and "No worlds are running right now." until they update themselves. When you
update a server from an older version, do it only after the launcher release with the change is
out, or tell your friends that the count is wrong until their launcher has updated.

**Real progression is now the default: decide before you update.** A server whose `metagame.env`
has no `PROGRESSION_MODE` line (or an empty one, or a value other than `real` or `stub`) switches
from upstream's fake max ranks to real progression, and players who played before start at Slayer
level 1. Nothing is migrated by the update. Keep max ranks for chosen players with a `grandfather`
seed, or add `PROGRESSION_MODE=stub` to `C:\DauntlessRevived\data\config\metagame.env` first to stay
on the stub. Unless that file says `real` or `stub`, the updater repeats the metagame's warning about
players without stored progression after a successful update. A rollback or a restore follows the
same rule, so an explicit line keeps the mode fixed. The
[upgrade notes]({{ upgrade_page.url | relative_url }}) have the details and a script.

### Backups

Backups are in `C:\DauntlessRevived\backups\<date>_<time>\`: the database (taken with SQLite's
online backup and checked), the settings with their keys, the admin account's key, the gateway
certificate, `server.json` and the launcher news. The newest 48 backups (the hourly ones and the ones
around starts and stops together) and the newest one of each of the last 30 days are kept. **They
contain keys**: copy them off the server only encrypted. The same folder is what `-RestoreFrom` takes.
What is and is not in a backup: [Files and data]({{ files_page.url | relative_url }}#backups).

## Session 0 and `-InteractiveSession`

At startup the stack runs without anyone signed in (Windows "session 0"). The game servers open a
console window when they start; that should work without a desktop, but it has not been tested on a
real server yet. If `Stack.ps1 status` never shows Ramsgate on UDP 8777 a minute after a start, run
the installer again with `-InteractiveSession`: the `dauntless` account then signs in automatically
at boot (its password is stored the way Windows stores auto-logon passwords, as an LSA secret, never
in plain text), the stack starts in that session, and the session is locked straight away.

## Private mode (Tailscale)

`-Mode Private` installs Tailscale (pinned MSI hash), signs the server in, has the metagame and
content server listen on the Tailscale address only, and limits the firewall to Tailscale addresses.
There is no gateway and no allowlist in this mode, and `New-Invite.ps1` prints v1 invites:
`dauntless-revived://join?v=1&host=<tailnet address>&port=61000&code=...&name=...`. The details of
sharing the machine are on [Run it for a group]({{ admin_page.url | relative_url }}).

## Files on the server

| Path | What it is |
|:-----|:-----------|
| `C:\DauntlessRevived\bin\` | The kit's scripts. |
| `app\` (and `app.prev\`) | The built server code (and the previous build, for rollbacks). |
| `game\Dauntless\` | The verified 1.4.4 files. |
| `data\config\` | `server.json` and the `.env` settings (secrets). Which keys the installer writes and which it keeps: [Configuration]({{ config_page.url | relative_url }}#server-kit). |
| `data\keys\`, `data\tls\` | The admin account's key and the game-server key; the gateway certificate and its private key. |
| `data\undaunted.db` | The database: accounts and saves. |
| `data\logs\` | Logs of every component; `gateway.out.log` is the access log (no keys or tokens). `performance\` holds the performance log, one CSV file per day (counts only). |
| `data\allowlist\` | The allowlist helper's audit log and state (only Administrators and SYSTEM can write there). |
| `backups\` | Hourly backups. |
| `staging\` | What `Deploy-Remote.ps1` uploads; the game zip stays here for repairs. |

Every folder and file, with who may read and write it: [Files and data]({{ files_page.url | relative_url }}#kit-install-root).

## Test the kit without a server

`deploy\windows-server\tests\` holds three test scripts for a development PC. None of them changes
the firewall, services, scheduled tasks, accounts or certificate stores, and none uses the ports of a
running server:

- `Test-KitUnit.ps1`: every script parses on PowerShell 5.1, invite strings (v1 and v2, including
  everything that must be refused), certificate fingerprints, TLS pinning against a local test server,
  and the upload helper.
- `Test-Sandbox.ps1`: a real `-Sandbox` install into `C:\dr\sandbox-ws2019` on spare loopback ports
  (metagame 62000, content 62002, allowlist helper 62005 in dry-run mode, gateway 62443), then invites,
  status and registration through the gateway with the pinned certificate, the gateway's refusals, a
  restore into a second folder, and cleanup.
- `Test-DeployRemote.ps1`: argument handling, and the upload logic against a local folder standing in
  for the server (an interrupted upload, a part damaged in transit, a part damaged after it was checked).

## Uninstall

On the server, in an elevated PowerShell. **Take and copy off a backup first** if you want to keep
anything.

```powershell
C:\DauntlessRevived\bin\Stack.ps1 stop
'Dauntless Revived stack', 'Dauntless Revived allowlist', 'Dauntless Revived backup' |
    ForEach-Object { Unregister-ScheduledTask -TaskName $_ -Confirm:$false -ErrorAction SilentlyContinue }
# Keep 'Dauntless Revived - SSH (TCP 22)' if the installer made it: every profile still blocks
# inbound connections by default, so without that rule SSH would stop working.
Get-NetFirewallRule -Group 'Dauntless Revived' -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -ne 'Dauntless Revived - SSH (TCP 22)' } | Remove-NetFirewallRule
Remove-NetFirewallRule -Name 'DauntlessRevived-GamePorts-Allowlist' -ErrorAction SilentlyContinue
Get-CimInstance Win32_UserProfile | Where-Object { $_.LocalPath -like '*\dauntless' } | Remove-CimInstance
Remove-LocalUser -Name dauntless
Remove-Item -Recurse -Force C:\DauntlessRevived
```

Some firewall changes are not undone by that: Remote Desktop rules limited to `-AdminIp` or disabled
(public mode without `-AdminIp`), and firewall profiles switched to "block inbound by default".
`data\config\server.json` lists what they were before under `FirewallChanges` (look before you delete
the folder); to use Remote Desktop again, re-enable its rule (`Enable-NetFirewallRule`) or re-run the
installer with `-AdminIp <your IP>`. If you used `-InteractiveSession`, also
turn auto-logon off: `Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
AutoAdminLogon 0`. The Visual C++ runtime (and Node.js, on a server installed before the Rust services) can be removed
in "Apps and features".

The snippet also leaves these installer changes in place:

- Firewall policy values the installer removed because they kept Windows Firewall off. They are
  listed as `policy|...` entries under `FirewallChanges`.
- `PasswordAuthentication no` in `C:\ProgramData\ssh\sshd_config`, written by a public-mode install
  that ran over SSH.
- The account lockout policy of the first public-mode install: 10 wrong passwords lock an account for
  15 minutes (`net accounts` shows it).
- With `-InteractiveSession`, the stored auto-logon password (the LSA secret `DefaultPassword`). The
  account it belongs to is deleted by the snippet, so the password no longer opens anything.
- The "Log on as a batch job" right given to `dauntless`, the DirectX June 2010 runtime, and in
  private mode Tailscale, whose network adapter the installer set to the Public network category.

Hosting a modified server for other people comes with the AGPL's source-code obligation; see
[Credits and license]({{ legal_page.url | relative_url }}).

## Optional owner dashboard

Run `C:\DauntlessRevived\bin\Install-OwnerDashboard.ps1 -OpenOnLogon` after updating the kit.
It installs a separate loopback-only LocalService task and sets `BACKEND_HEALTH=1` for the next
metagame restart. It never opens a public firewall port or restarts games itself. The optional
browser task opens the locked page at `http://127.0.0.1:61110` in the installing operator's RDP
session. `Stack.ps1 start` and full `restart` also start the installed dashboard; stopping the
game stack leaves monitoring available. Use an SSH tunnel from a remote PC.

After code updates, re-run the dashboard installer to refresh its copied files. See
[tools/DASHBOARD.md](https://github.com/mixutin/dauntless-revived/blob/dauntless-revived/tools/DASHBOARD.md)
for account-key fingerprints, invitations, performance CSVs, permissions and disabling the tasks.
