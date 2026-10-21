# Windows Server kit

Installs and runs a Dauntless Revived server on Windows Server 2019 or newer (Desktop Experience).
Full guide: `docs/setup/windows-server.md` (on the site: Setup > Windows server kit). Every parameter
of every script: `docs/reference/scripts.md` (on the site: Reference > Scripts and parameters).

| Script | Runs on | What it does |
|:-------|:--------|:-------------|
| `Deploy-Remote.ps1` | your PC | Uploads the kit, the code, a backup and the game zip (resumable, SHA-256 checked) over key-only SSH, runs the installer, prints the invite fingerprint. Also `-Update`, `-InviteFor <name>`, `-Status`, `-Chat On/Off`. |
| `Install-DauntlessServer.ps1` | the server | `-Mode Public` (TLS gateway, pinned certificate, UDP allowlist) or `-Mode Private` (Tailscale). Idempotent; `-WhatIf` shows every change. |
| `Stack.ps1` | the server | `status`, `start`, `stop`, `restart [-Only <component>]`; `supervise` runs inside the scheduled tasks and writes the performance log every minute. |
| `New-Invite.ps1` | the server | Creates an invite code and prints the v2 (public) or v1 (private) invite line. |
| `Get-ServerStatus.ps1` | anywhere | Players and running worlds, through the gateway with the pinned certificate (`-Invite '<line>'` from any PC). The server lists them to registered players only: on the server the script asks with the owner key (run it as administrator), elsewhere pass `-KeyFile <account.key>`; without a key it says the list is hidden. |
| `Update-DauntlessServer.ps1` | the server | New code and its service binaries, put in place beside the running server, switched in with a health check and automatic rollback. |
| `Backup-DauntlessServer.ps1`, `backup-hidden.vbs` | the server | Hourly backup: database (online backup, checked), settings, keys, gateway certificate. |
| `Set-Chat.ps1` | the server | `-On` / `-Off`: the in-game text chat (roadmap 3.10), the metagame's listener on `127.0.0.1:61099` that the gateway already forwards the game's chat connection to. Restarts the stack; public mode only. Based on Vvoidddd's first chat switch (#9). Friends' online status (`CHAT_PRESENCE=1`) is not set here: add that line to `metagame.env` by hand. |
| `Write-PerformanceLog.ps1` | the server | The performance log (roadmap 4.12): CPU and memory per game server and component, the machine's CPU, RAM, disk and network, and player counts, as CSV per UTC day in `data\logs\performance\` (counts only). The stack supervisor takes a sample every minute; this script takes one now (`-Once`) or samples a sandbox. Based on Vvoidddd's first sampler (#6). |
| `Receive-Upload.ps1` | the server | The server side of the chunked upload. |
| `DauntlessServer.Common.ps1`, `DauntlessServer.Performance.ps1` | both | Shared helpers: pins, invites, pinned HTTPS, the service binaries and their processes; the performance sampler. |

The services are Rust binaries in `<root>\app\bin` (`metagame.exe`, `gateway.exe`, `allowlist.exe`,
`content.exe`, `deploy.exe`; MIGRATION_PLAN.md M40); the server needs no Node.js. The installer, the
updater and `Deploy-Remote.ps1` take them from `-ServerBinaries <folder>`: the
`server-x86_64-pc-windows-msvc` download of the code's CI run, checked against its `SHA256SUMS` (or a
release build in a checkout, `rust\target\release`). The first update of a server that still runs the
Node services stops those and starts the binaries; a rollback brings the Node build back. That first
update must run the new code's `Update-DauntlessServer.ps1` (`Deploy-Remote.ps1 -Update` does): the
installed Node kit's copy would rebuild the code with npm, fail its health check and roll back.

The kit writes none of the metagame's feature switches from the port of Harmonic's fork (Escalation,
the store, Slayer Links, friends' online status and the rest), so they run at their defaults; a line you
add to `data\config\metagame.env` is kept across re-runs and updates. Which line does what:
`docs/setup/admin.md`, "Switching features on".

Tests for a development PC (no system changes, spare ports 62000-62499 only): `tests\Test-KitUnit.ps1`,
`tests\Test-Sandbox.ps1`, `tests\Test-DeployRemote.ps1`. Build the service binaries first
(`cargo build --bins -p metagame -p gateway -p content -p deploy` in `rust\`); the tests take them from
`rust\target\debug`.

Windows PowerShell 5.1, ASCII only. Nothing in the kit prints a key, a token, a password or a `.env`
value.
