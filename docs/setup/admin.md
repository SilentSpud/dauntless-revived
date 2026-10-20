---
title: Run it for a group
parent: Setup
nav_order: 4
description: "Opening a Dauntless Revived server to a few friends: Tailscale sharing, firewall rules, invite codes, admin accounts, capacity figures and database backups."
lang: en
ref: setup/admin
---

{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}
{% assign legal_page = site.pages | where: "path", "legal.md" | first %}
{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign scripts_page = site.pages | where: "path", "reference/scripts.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}

# Run it for a group
{: .no_toc }

## Private owner dashboard (2026-09-30)

On an updated Windows kit server, run `C:\DauntlessRevived\bin\Install-OwnerDashboard.ps1 -OpenOnLogon`
as administrator, then restart the metagame during maintenance to activate backend health collection.
Open `http://127.0.0.1:61110` **on the VPS** and unlock with `data\keys\owner.key`.
From your own PC, forward that port through SSH; do not open it in the public firewall.
The dashboard starts at boot and with a full stack start/restart. Browser opening happens only
in the installing operator's interactive session, not in the game service account.

The lightweight views show CPU/RAM, backend request/error/event-loop metrics, uptime, player
locations, process memory, free disk/network, account names and key fingerprints, registration
invites and bounded operational logs. Key matching happens in the browser without sending the
entered player key. Game tick rate is not yet instrumented. Process metrics reuse the kit's
minute CSV rather than repeatedly launching scanners. Re-run the dashboard installer after
updates. See [the dashboard operator guide](https://github.com/mixutin/dauntless-revived/blob/dauntless-revived/tools/DASHBOARD.md)
for security, limitations and disabling autostart.

Keep `REGISTRATION_MODE=INVITECODE`: generate a join link in **Invites** and share
that link, not a login key. Registration automatically creates a personal random
key that the launcher saves encrypted. The server stores only its SHA-256 hash.
**Players & accounts** maps each username and UID to a short key fingerprint;
original keys cannot be recovered from this directory. Editing a key invalidates
it, but anyone who steals a valid key can use it, so keep player keys private.

The Windows dashboard installer grants LocalService read/execute access to the
required parent folders only (not recursively), plus read access to the selected
configuration and log files. This avoids startup failures on hardened installs
without running the dashboard as Administrator or opening another public port.

### Discord status message

The optional `Install-DiscordStatus.ps1` installs a separate boot-started worker.
It checks the local backend every five seconds and edits one Discord message every
15 seconds, with rate-limit backoff. The message includes aggregate player count,
backend availability/uptime and local response time only: no IP or account names.
Discord timestamps adapt to each reader's timezone. A whole-VPS outage leaves a
stale timestamp; detecting that independently needs an external monitor. Keep the
webhook secret out of Git. See [installation and recovery instructions](https://github.com/mixutin/dauntless-revived/blob/dauntless-revived/tools/DISCORD-STATUS.md).

This page covers how we open our Undaunted-based stack (client build **1.4.4**) to a handful of
friends: network access, firewall, addresses, accounts, capacity and backups. It assumes the stack
already runs for you locally, with the metagame, the deploy server and a Ramsgate server.

**Status (22 September 2026).** On the owner's PC our stack runs on loopback for the owner only. The
Tailscale setup below has not been run end to end yet: for the first test with friends we use the
[Windows server kit]({{ winserver_page.url | relative_url }}) in public mode on a rented server instead
(see the [roadmap]({{ roadmap_page.url | relative_url }}), items 1.3, 1.15 and 1.17). Anything planned
but not built yet is marked as such. The other side of this setup is
[Join as a friend]({{ friends_page.url | relative_url }}).

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## What runs on the host

| Component | Listens on | Who has to reach it |
|:----------|:-----------|:--------------------|
| Metagame (`legacy/UndauntedMetagame`: Node, Express, SQLite) | TCP 61000 | Every player's client, every game server, and you as admin |
| Deploy server (`legacy/UndauntedDeployServer`) | TCP 61001, loopback only | Only the metagame on the same PC. **It has no authentication.** |
| Ramsgate server | UDP 8777 (`PORT_RANGE_END`) | Players. Always running. If it dies, the deploy server starts it again when a player travels there, or its watchdog does within a minute. |
| Training Dojo | UDP 8776 | Players. Our fork starts it on first use; `ENABLE_DOJO=1` restores upstream's always-on behaviour. |
| Hunt servers | UDP 8770-8775 | Players. One per group of up to 4. Each exits after 50 seconds in total with nobody connected. |

The game servers are further instances of the same 1.4.4 client executable. The deploy server starts
them with `-server -nullrhi`, the two DLLs load into each one, and they need no GPU. Upstream's
launcher expects a local metagame on port 60000, but on our host another program already held 60000,
so we use 61000 and 61001.

## Why Tailscale instead of port forwarding

Port forwarding would put all of the following on the public internet:

- **Everything is plain HTTP.** The client DLL rewrites every backend endpoint to
  `http://<metagame address>/...`. There is no TLS anywhere in the stack.
- **Account keys travel in the clear.** At login, the game posts the player's key as the
  `exchange_code` to `/account/api/oauth/token`. The launcher and admin API send it in the
  `x-undaunted-user-api-key` header. What comes back is a 24-hour RS256 bearer token that gives full
  access to that player's data.
- `POST /undaunted/api/Register` needs no authentication, and nothing in the metagame limits request
  rates.
- The deploy server has no authentication at all. It listens on loopback and answers 403 to callers
  on other machines, and those two checks are all that stop others from making your PC start game
  processes through TCP 61001.
- The game servers are a 2020 game build with an injected DLL, not hardened network services.

Tailscale avoids all of that. Each friend's traffic to the host is encrypted with WireGuard, and only
people you have shared the machine with can reach it. You need no router changes and no public IP, so
it also works behind carrier-grade NAT. Removing a share cuts a friend off at once. The costs: every
friend installs Tailscale, and some connections go through a Tailscale DERP relay instead of a direct
path. Relays work, with higher latency.

## Share the host PC with Tailscale

1. Install Tailscale on the host and sign in. `tailscale ip -4` prints the host's `100.x.y.z` address.
   The address stays the same as long as the machine stays in your tailnet. Friends' scripts and
   `MY_IP` both depend on it.
2. In the Tailscale admin console, open the host machine's menu, choose **Share**, and invite each
   friend by email or link. Each friend accepts with their own Tailscale account. Do not add friends as
   users of your tailnet. A share exposes this one machine and nothing else.
3. Set Tailscale to start with Windows and to run unattended, so the address exists before the stack
   starts (see [Switch the addresses](#switch-the-addresses-to-tailscale)).
4. Optional: tailnet policy rules can limit what share recipients may reach on this machine to
   `tcp:61000` and `udp:8770-8777`. We have not settled the exact policy syntax for shared users
   (unverified). Check Tailscale's access-control documentation. The Windows firewall rules below are
   the enforcement we rely on.
5. Ask each friend to run `tailscale ping <host address>` once. A reply `via DERP(...)` means they are
   relayed rather than direct.

Removing a friend's share is currently the only complete way to lock someone out. See
[Missing admin functions](#missing-admin-functions-and-workarounds).

## Firewall: allow only the Tailscale interface

Windows Defender Firewall blocks inbound traffic by default. On our host there were no inbound rules
for the game or for Node at all. Add exactly two allow rules, each tied to one program, the Tailscale
interface and Tailscale's address range. Run these in an elevated PowerShell:

```powershell
$game = "C:\D144\Dauntless\Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe"
$node = "C:\Program Files\nodejs\node.exe"

New-NetFirewallRule -DisplayName "Dauntless Revived - metagame (Tailscale)" `
  -Direction Inbound -Action Allow -Protocol TCP -LocalPort 61000 `
  -Program $node -InterfaceAlias Tailscale -RemoteAddress 100.64.0.0/10

New-NetFirewallRule -DisplayName "Dauntless Revived - game servers (Tailscale)" `
  -Direction Inbound -Action Allow -Protocol UDP -LocalPort 8770-8777 `
  -Program $game -InterfaceAlias Tailscale -RemoteAddress 100.64.0.0/10
```

- **Never open TCP 61001.** The deploy server has no authentication.
- Check that the adapter is really called `Tailscale` with `Get-NetAdapter`.
- If you have ever answered Windows' "allow access" prompt for `node.exe` or the game, Windows created
  its own rules for that program. An allow rule from that prompt opens the program on every interface
  of that network type. A block rule overrides the allow rules above. List them and remove the ones you
  do not want:

  ```powershell
  Get-NetFirewallApplicationFilter -Program $node, $game | Get-NetFirewallRule |
    Format-Table DisplayName, Direction, Action, Enabled, Profile
  ```

- Check which network category Windows gave the Tailscale adapter with
  `Get-NetConnectionProfile -InterfaceAlias Tailscale`. If it is `Private`, any rules you allow for
  private networks (file sharing, for example) also apply to share recipients. Setting it to `Public`
  avoids that. We have not checked whether Tailscale resets the category on reconnect (unverified).
- Test from a friend's PC with
  `Invoke-RestMethod http://<host address>:61000/undaunted/api/RegistrationStatus`. The same request
  from another device on your home LAN should fail.

## Switch the addresses to Tailscale

Three settings point at `127.0.0.1` today and have to change:

| Setting | File | New value | Why |
|:--------|:-----|:----------|:----|
| `MY_IP` | `legacy/UndauntedDeployServer/.env` | `<host 100.x address>` | The only host address the deploy server hands out for Ramsgate, the Dojo and hunts, via the matchmaking result. With `127.0.0.1`, a friend's client would travel to its own PC. |
| `QOS_TARGET_URL` | `legacy/UndauntedMetagame/.env` | `http://<host 100.x address>:61000/QoS` | `/candidate/regions` returns this URL as the region to ping. With `127.0.0.1`, friends ping their own localhost. We have not tested what the client does when that ping fails. |
| `BIND_HOST` | `legacy/UndauntedMetagame/.env` | `<host 100.x address>` | The address the metagame listens on. Our fork defaults to `127.0.0.1`; upstream listened on every interface. |
| `BIND_HOST` | `legacy/UndauntedDeployServer/.env` | stays `127.0.0.1` | No authentication. |
| `DEPLOYSERVER_URL` | `legacy/UndauntedMetagame/.env` | stays `127.0.0.1:61001` | The metagame reaches the deploy server locally. |

**The catch: the metagame listens on one address**, and two local things talk to it as well.

1. **Your own client.** Launch it against the Tailscale address. Our host launcher takes it as
   `play.ps1 -Backend "<host 100.x address>:61000"` (the friends' script calls the same parameter
   `-Server`). The traffic stays on your PC.
2. **The game servers.** The DLL rewrites endpoints only in client mode. It never hooks the config in
   server mode. Game-server processes instead read the endpoint overrides from the host account's user
   config, `%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\Game.ini`, section
   `[OnlineSubsystemPhoenix]`. On our host that section has 167 entries of the form
   `AuthEndpoint="http://127.0.0.1:61000/game/login"`. They must point at an address the metagame
   listens on. Back the file up, then rewrite it:

   ```powershell
   $ts = "100.x.y.z"      # the host's Tailscale address
   $gi = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient\Game.ini"
   Copy-Item $gi "$gi.bak"
   (Get-Content $gi) -replace '127\.0\.0\.1:61000', "${ts}:61000" | Set-Content $gi -Encoding ASCII
   ```

   We have not pinned down which process writes that section. Its timestamp matched both the client
   travelling and a server starting (unverified). After your first session, check that it still points
   where you expect. A server that cannot reach the metagame cannot load or save characters.

Because of this, **Tailscale must be connected before the metagame starts.** If the address does not
exist yet, our fork exits with `Could not listen on <address>:61000`. Upstream announced success anyway
and then quit silently.

Do not use `BIND_HOST=0.0.0.0` to sidestep this. The metagame would then listen on your LAN as well,
and only the firewall would stand between LAN devices and an unauthenticated registration endpoint.

Planned in the fork:

- Let `BIND_HOST` take a list (`127.0.0.1,<100.x>`), so local traffic stays on loopback and the
  `Game.ini` rewrite is no longer needed.
- Hook the config in server mode too, so game servers stop depending on the owner's `Game.ini`.

### Restarting after a change

Both services read `.env` only at start (`npm run start`, which runs
`node --env-file=.env dist/server.js`).

1. Stop the deploy server first. Otherwise its watchdog, or a player's trip to Ramsgate, starts a new
   Ramsgate.
2. Check for leftover game servers. Normally they end together with the deploy server: it starts them
   with Node's default (not detached) `spawn`, and on Windows Node places such children in a job
   object that is closed when the parent exits. We confirmed this with a test process, both for a
   normal exit and for a forced kill. Servers started by hand, for example with our test script, are
   not covered and keep their UDP ports. The first argument on a game server's command line is the
   gameserver key, so do not print their command lines. Your own client runs the same executable
   without `-server`, and this filter leaves it alone:

   ```powershell
   Get-CimInstance Win32_Process -Filter "Name = 'Dauntless-Win64-Shipping.exe'" |
     Where-Object { $_.CommandLine -match '\s-server(\s|$)' } |
     ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
   ```

3. Stop the metagame.
4. Start the metagame from its own folder, because its migrations path is relative to the working
   directory. Then start the deploy server, which launches Ramsgate right away.

A metagame restart does not log players out, because tokens are stateless 24-hour JWTs. It does lose
in-memory state: matchmaking queues and the list of who is online.

## Accounts: authentication, registration and invite codes

Two `.env` settings in the metagame decide who can get in:

- `AUTH_MODE=APIKEY` with `NODE_ENV=production`. The other mode, `AUTH_MODE=NONE`, takes whatever the
  client sends as the user id, so anyone can log in as anyone. It is honoured only outside production.
  Never use it with friends.
- `REGISTRATION_MODE`:

| Mode | `POST /undaunted/api/Register` |
|:-----|:-------------------------------|
| `NONE` | Always refused (400). |
| `INVITECODE` | Needs a valid code. A wrong or used-up code gets 401. |
| `OPEN` | Anyone who can reach the metagame gets an account. |

Set `INVITECODE` in `.env` **before** the metagame listens on Tailscale. The `.env` in
[Host a server]({{ host_page.url | relative_url }}#metagame) uses `OPEN`, which is harmless only while
the metagame listens on loopback. (The Windows server kit always writes `INVITECODE`.) The admin API
can switch the mode at runtime, but only in memory: a restart goes back to the `.env` value.

Registration returns the new player's key (`UUK_` followed by 48 hex characters) once. The server keeps
only its SHA-256 hash.

### Making invite codes

Admin calls are authenticated with an admin account's own key in the `x-undaunted-user-api-key`
header, and they work only directly against the metagame, never through a proxy or the public
gateway. In our layout the owner's key lives in `C:\dr\data\owner.key`. This block asks the server
for a new single-use code, lists all codes, and revokes one:

```powershell
$M = "100.x.y.z:61000"    # wherever the metagame listens
$h = @{ "x-undaunted-user-api-key" = (Get-Content C:\dr\data\owner.key -Raw).Trim() }

# new single-use code, made by the server (XXXX-XXXX-XXXX)
$code = (Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/CreateInvite" -Headers $h `
  -ContentType "application/json" -Body (@{ uses = 1 } | ConvertTo-Json) -TimeoutSec 10).code
$code        # send this to one friend, privately

# list, and revoke
(Invoke-RestMethod -Uri "http://$M/undaunted/api/InviteCodes" -Headers $h -TimeoutSec 10).InviteCodes
Invoke-RestMethod -Method Delete -Uri "http://$M/undaunted/api/InviteCode/$code" -Headers $h -TimeoutSec 10
```

- `CreateInvite` makes three groups of four characters from Crockford's base32 alphabet (60 random
  bits). `uses` is 1 to 1000. An optional `name` is a note for the metagame's log and is not stored;
  the log shows only the code's first group. The older `RegisterInviteCode` still stores a code you
  choose yourself. On a kit server, `New-Invite.ps1` does all of this for you.
- Codes are used up atomically: a single SQL `UPDATE` checks and decrements the remaining uses, so two
  people cannot both redeem a single-use code.
- Register checks the code, then the username, and only then spends one use of the code and writes
  the account, all in one database transaction. A rejected or taken name does not burn a code.
- Prefer single-use codes, one per friend, sent privately. Use many uses (or `InfiniteUses` with
  `RegisterInviteCode`) only briefly, if at all.

### Making an account an admin

An admin is a row in the `users` table with `isAdmin = 1`. No API sets it. Edit the database while the
metagame is stopped, for example with a small `better-sqlite3` script run from the `legacy/UndauntedMetagame`
folder (`UPDATE users SET isAdmin = 1 WHERE userId = ?`). An admin key is simply that user's account
key, so keep it on the host.

## The admin API {#admin-api}

All routes live under `/undaunted/api` on the metagame port. "Admin" means the
`x-undaunted-user-api-key` header must belong to a user with `isAdmin`. A missing or unknown key gets
401; a valid key that is not an admin gets 403. **An admin call that carries a proxy header gets 403**
before the key is even checked, so admin calls work only directly against the metagame: on the host,
or in private mode from a machine on the tailnet. The public gateway refuses them as well.

[HTTP API]({{ api_page.url | relative_url }}#undaunted-api) lists every route with its body and
answers. The ones used most for running a group:

| Method and path | Auth | What it does |
|:----------------|:-----|:-------------|
| `GET /RegistrationStatus` | none | `{ "RegistrationMode": ... }` |
| `POST /RegistrationStatus` | admin | Body `{ "RegistrationStatus": <mode> }`, where the mode is `NONE`, `INVITECODE` or `OPEN`. In memory only. |
| `POST /CreateInvite` | admin | Body `{ "uses", "name" }` (both optional), returns `{ "code" }`. |
| `GET /InviteCodes` | admin | All codes with their remaining uses. |
| `POST /RegisterInviteCode` | admin | Body `{ "NewInviteCode", "Uses", "InfiniteUses" }`: a code you choose. |
| `DELETE /InviteCode/:code` | admin | Revoke a code. |
| `GET /GetAllUsers` | admin | `{ "Users": [{ "Username", "UserId" }] }` |
| `POST /RenameUser` | admin | Body `{ "UserId" }` or `{ "Username" }` plus `{ "NewUsername" }`. Renames the account and its characters. |
| `POST /GenerateJWTForUserId` | admin | Body `{ "UserId" }`, returns `{ "JWT" }`. Mints a 24-hour game token **for any user**, which amounts to playing as them. Treat admin keys accordingly. |
| `GET /PrivateOnlineStats` | admin | Per player: map, hunt and hunt start time, for players seen in the last 90 seconds. |
| `GET /SaveHistory`, `POST /RollbackCharacter`, `POST /RollbackLoadout` | admin | The saved versions of a player's characters and loadouts, and rolling one back. The player should be offline. |
| `POST /GrantEntitlement`, `POST /RevokeEntitlement` | admin | Give or take away an entitlement. |
| `POST /Register` | none (mode-gated) | Body `{ "Username", "InviteCode" }`. Returns `{ "UUK" }`. |
| `GET /GetUserInfo` | user key | `{ "UserId", "Username", "IsAdmin" }` |
| `GET /ServerStatus` | none | The server's name, version and source; with a user key also who is online. |

The friend launcher has no admin screen. Make admin calls directly, as in the examples on this page,
or with the kit's scripts on a kit server.

### Progression (fork only) {#progression}

Real progression is on by default: every account keeps its own Slayer level, mastery and Hunt Pass
(`PROGRESSION_MODE`, see [Host a server]({{ host_page.url | relative_url }}#metagame)). Two admin routes
go with it:

| Method and path | Auth | What it does |
|:----------------|:-----|:-------------|
| `GET /Progression?UserId=<id>` | admin | One account's tracks with the ranks the game will show, its objectives, Hunt Pass and entitlements, and whether it is in real mode. |
| `POST /SeedProgression` | admin | Body `{ "UserId", "Mode" }`. `grandfather` sets every track to its maximum rank, fully confirmed (nothing is granted); `fresh` sets every track to 0 and clears the objectives. Run it while the player is offline. |

A server that already had players before real progression became the default starts them at Slayer
level 1. The [upgrade notes]({{ upgrade_page.url | relative_url }}) explain the choice and include a
small script for both routes.

### Social fallbacks on the host (fork only) {#social}

Friends, parties and guilds work in the game itself; see
[Join as a friend]({{ friends_page.url | relative_url }}#friends-parties-and-guilds). These five routes
do the same things by name from the server, for when the game's own menus fail (for example during
the first live test). Like every admin call they work only directly against the metagame, never
through the gateway. `PartyInvite`, `Friends` and `GuildInvite` act as the key's owner; an admin key
may name another player in `From`.

| Method and path | Auth | What it does |
|:----------------|:-----|:-------------|
| `POST /PartyInvite` | account key (admin for `From`) | Body `{ "Username", "From" }`: invites that player to the sender's party, with the game's own checks. The player still accepts in the game (PARTY INVITES, within about 10 seconds). Answers `{ "From", "To" }`. |
| `POST /Friends` | account key (admin for `From`) | Body `{ "Username", "From" }`: sends a friend request, or accepts the one that player sent. Answers `{ "From", "To", "Result" }`. The other player sees it at their next login. |
| `POST /GuildInvite` | account key (admin for `From`) | Body `{ "Username", "From" }`: invites that player to the sender's guild (the sender must be its Leader or an Officer). The player accepts under GUILD INVITES at their next login or world load. Answers `{ "From", "To", "Guild" }`. |
| `GET /Guilds` | admin | Every guild: `[{ "guildId", "name", "nameplate", "leader", "members" }]`, `members` being a count. |
| `POST /DisbandGuild` | admin | Body `{ "Guild" }`, the id or the name in any case: removes the guild, its members and its invites. Answers `{ "Guild", "Members" }`. |

```powershell
$M = "127.0.0.1:61000"    # on the rented server; in private mode the Tailscale address
$h = @{ "x-undaunted-user-api-key" = (Get-Content C:\dr\data\owner.key -Raw).Trim() }
$json = @{ ContentType = "application/json"; Headers = $h; TimeoutSec = 10 }

# invite a friend to your party, send a friend request, invite to your guild
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/PartyInvite" @json -Body (@{ Username = "Friend" } | ConvertTo-Json)
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/Friends" @json -Body (@{ Username = "Friend" } | ConvertTo-Json)
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/GuildInvite" @json -Body (@{ Username = "Friend" } | ConvertTo-Json)

# as an admin, for another player: Friend2 invites Friend to Friend2's party
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/PartyInvite" @json -Body (@{ Username = "Friend"; From = "Friend2" } | ConvertTo-Json)

# list the guilds, and remove one
Invoke-RestMethod -Uri "http://$M/undaunted/api/Guilds" -Headers $h -TimeoutSec 10
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/DisbandGuild" @json -Body (@{ Guild = "SomeGuild" } | ConvertTo-Json)
```

A refusal is a 4xx with `{ "error", "message" }`. `Invoke-RestMethod` throws on it; the message is in
`$_.ErrorDetails.Message`.

| Route | `error` | Status | Meaning |
|:------|:--------|:-------|:--------|
| all five | none (empty body) | 401 | No key, or an unknown one. |
| all five | none (empty body) | 403 | An admin key through a proxy; for `Guilds` and `DisbandGuild`, a key that is not an admin's. |
| `PartyInvite`, `Friends`, `GuildInvite` | `forbidden` | 403 | `From` with a key that is not an admin's. |
| `PartyInvite`, `Friends`, `GuildInvite` | `not_found` | 404 | No account by that `Username` (or `From`). |
| `PartyInvite` | `party_invite_refused` | 403, 404 or 409 | The game's own refusal, with the reason in `message`: not the leader, a full party, already invited, a block, the player declined your invite in the last 2 minutes, or 20 invites sent in 10 minutes. |
| `Friends` | `self`, `blocked`, `limit`, `pending_limit`, `rate` | 400, 403 or 409 | Yourself; a block either way; 200 friends; 50 unanswered requests; 20 new requests in 10 minutes. |
| `GuildInvite` | `guild_refused` | 403, 404, 409 or 429 | The guild's own refusal: `message` starts with its code (for example `RedundantAdorableQuillshot:` for an invite already open; the codes are on [HTTP API]({{ api_page.url | relative_url }}#guilds)). An empty code covers a block, the 24-hour pause after a decline and the invite limits. |
| `DisbandGuild` | `not_found` | 404 | No guild by that id or name. |
| the three guild routes | `guilds_off` | 404 | `GUILDS=0` is set. |

### Missing admin functions and workarounds

There is no API to delete or ban a user, to revoke or reissue a key, or to promote an admin.
Renaming does exist (`RenameUser`, see [Usernames](#usernames)).

- **Locking someone out.** Remove their Tailscale share, which cuts network access at once. Then
  delete their row from `userapikeys`. Their key stops working at the next login, but a token that was
  already issued stays valid for up to 24 hours.
- **A friend lost their key.** It cannot be recovered, because only its hash is stored. Issue a new one
  by replacing the hash. A re-key admin endpoint is planned. Until then, save this as `rekey.js` in the
  `legacy/UndauntedMetagame` folder, so that `require` finds `better-sqlite3`:

  ```js
  // node rekey.js <UserId>   (take the UserId from GET /undaunted/api/GetAllUsers)
  const crypto = require("crypto");
  const fs = require("fs");
  const Database = require("better-sqlite3");
  const db = new Database("C:/dr/data/undaunted.db", { fileMustExist: true });
  const userId = process.argv[2];
  const key = "UUK_" + crypto.randomBytes(24).toString("hex");   // same format as Register
  const hash = crypto.createHash("sha256").update(key, "utf8").digest("hex");
  const r = db.prepare("UPDATE userapikeys SET keyHash = ? WHERE userId = ?").run(hash, userId);
  if (r.changes !== 1) { console.error("no such user"); process.exit(1); }
  fs.writeFileSync(`C:/dr/data/rekey-${userId}.txt`, key);   // hand it over privately, then delete
  console.log("new key written to C:/dr/data/rekey-" + userId + ".txt");
  ```

  **Do not re-key through the `userapikeystoregister` table.** At boot the metagame first deletes the
  pending rows, then inserts each one into `userapikeys`, where `userId` is the primary key. For an
  existing user that insert fails, the metagame never starts listening, and the pending key is
  already gone.

## Switching features on {#switching-features-on}

Several features are built but off by default, and a few that are on have a switch to turn them off.
Each is one line in the metagame's settings: `.env` in the `legacy/UndauntedMetagame` folder on a hand-built
host, `C:\DauntlessRevived\data\config\metagame.env` on a kit server (edit it from an elevated editor).
The kit keeps a line you add across re-runs and updates. Then restart the metagame **when nobody is
playing** (a restart drops parties and matchmaking queues): by hand as in
[Restarting after a change](#restarting-after-a-change), or on a kit server
`C:\DauntlessRevived\bin\Stack.ps1 restart -Only metagame`. The metagame's start line `features: ...`
shows the value it uses. Every setting is on [Configuration]({{ config_page.url | relative_url }}).

**Off by default, and what to do before switching each on:**

| Feature | Line | Before you switch it on | How to check it |
|:--------|:-----|:------------------------|:----------------|
| Real Escalation saves | `ESCALATION_MODE=real` | Tell your players: everyone drops from the fake maximum (level 25) to level 0. Restart the **whole stack** (on a kit server `Stack.ps1 restart`), so no game server still holds the old values; do the same when you switch it back to `stub`. | Play an Escalation run, log in again, spend a talent point: the level stays, and the log has no `Refusing escalation save` and no `breaks a soft rule` line ([Escalation]({{ '/findings/escalation.html' | relative_url }}#switching-it-on)). |
| Strict Escalation rules | `ESCALATION_STRICT=1` | Only after real Escalation has run with no `breaks a soft rule` line. | Same as above. |
| The free store | `STORE=free` | Decide whether the store stays free (roadmap 3.7). Count the accounts with more than one character (below): a purchase goes to the character saved last. | Open every store tab, buy one item of each kind, log in again, finish a hunt ([The in-game store]({{ '/findings/store.html' | relative_url }}#open)). |
| Unlimited premium bounty tokens | `STORE_REPEATABLE_TOKENS=1` (with `STORE=free`) | Your decision: it means unlimited free premium bounty drafts. | The bundle shows in the store. |
| Friends' online status | On by default; `CHAT_PRESENCE=0` disables | Chat itself must work. No manual player status setting; remove an existing operator opt-out to enable. | The presence test on [Text chat]({{ '/findings/chat.html' | relative_url }}#how-to-verify-presence): no player is kicked from a party. |
| Hunt Pass rank entitlements on confirm | `PROGRESSION_CONFIRM_ENTITLEMENTS=1` | Only if an in-game test shows the Elite ranks' cosmetics (ranks 6, 9, 29 and 50) never arrive by themselves (no `POST /entitlementv2` in the log). | The cosmetic shows after Claim. |
| Your own Hunt Pass season files | `PROGRESSION_CONFIG_DIR=<folder>` and, for another season, `ACTIVE_HUNT_PASS=<id>` | Read [Game settings]({{ '/reference/game-settings.html' | relative_url }}#hunt-pass-seasons); never change a season players already have progress in. | The start line `Progression config: ...` names what was loaded; a bad file stops the metagame with the reason. |

**On by default, and the line that turns each off:** `SLAYER_LINKS=0` (Slayer Links),
`VERIFY_STUB_ACCOUNT=1` (the old session-check reply), `BALANCE_FROM_INVENTORY=0` (the old currency
sheet), `PROGRESSION_REPLAY_WINDOW_S=0` (no retry guard; set it if `repeats the grant of` shows up in
normal play). In the deploy server's settings, `PERSISTENT_WORLD_LIVENESS=0` leaves restarting a dead
Ramsgate to the watchdog; restart the deploy server after changing it.

**Counting characters per account** before `STORE=free` (it prints counts only, no names or ids). On a
kit server, in an elevated PowerShell:

```powershell
Set-Location C:\DauntlessRevived\app\UndauntedMetagame
node -e "const db = new (require('better-sqlite3'))(process.argv[1], { readonly: true }); console.log(db.prepare('SELECT COUNT(*) AS accountsWithSeveralCharacters FROM (SELECT userId FROM characters GROUP BY userId HAVING COUNT(*) > 1)').get())" C:/DauntlessRevived/data/undaunted.db
```

On a hand-built host, run it from your `legacy/UndauntedMetagame` folder with your `DB_FILENAME`. A result of
0 means every purchase goes to the player's only character.

## Usernames

- The name is chosen at registration. The friend launcher and the friend kit ask for it.
- New names are 3-16 characters, letters, digits and underscore only (`^[A-Za-z0-9_]{3,16}$`), after
  trimming surrounding spaces. `Register` answers 400 `username_invalid` otherwise.
- Names are unique regardless of case: `Register` answers 409 `username_taken` for a name another
  account already has in any case. The check runs in the same database transaction that creates the
  account (there is no unique index), and before the invite code is used up, so a rejected name does
  not burn a code.
- Accounts made before these rules keep their names, even ones that break them.
- `GET /undaunted/api/UsernameAvailable?Username=<name>` checks a name without registering. The
  public gateway does not pass it, so it works only on the host or over the tailnet.
- On a player's first login the metagame creates their character and names it after the username.
  The admin route `POST /undaunted/api/RenameUser` therefore renames the account and its characters
  together. The player sees the new name after logging in again. Whether other players' nameplates
  update without a relog is unverified.
- There is no rename inside the game: `/account/api/public/account` reports
  `canUpdateDisplayName: false`.
- **After a rename the player restarts the game before using chat.** The game reads its own name
  once, at login, and joins chat rooms with it. Until it logs in again the chat server may refuse
  those joins (`chat: join refused ... reason=nick-name`), because the name no longer matches the
  account. Other players see the new name as soon as their game looks it up.
- Our own owner account still carries the placeholder name "Slayer".

To rename, with `$M` and `$h` set as in [Making invite codes](#making-invite-codes):

```powershell
$body = @{ Username = "OldName"; NewUsername = "New_Name" } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/RenameUser" -Headers $h `
  -ContentType "application/json" -Body $body -TimeoutSec 10
```

`Username` is the current name in any case; `UserId` works instead. A new name another account has
gets 409 `username_taken`.

## Capacity

> For rented hosting, see our clearly-labelled [EU Gamehost partner recommendations]({{ "/setup/hosting-partner.html" | relative_url }}).
> The plan mapping there uses the measurements below rather than provider marketing labels.

Measured on our host, an 8-core desktop with 32 GB of RAM, running the 1.4.4 stack:

| Process | RAM | CPU | Notes |
|:--------|----:|----:|:------|
| Ramsgate server | 1.1 GB | ~0.2 core | Always running. |
| Hunt server | ~0.9 GB | not measured under combat | Measured on the tutorial hunt. One per group of up to 4. |
| Training Dojo | ~0.9 GB | ~0.2 core | Only while in use, in our fork. |
| Host's own client at Cinematic | ~1.9 GB | ~2.5 cores | Only if you also play on the host. Working set; its committed (private) memory was about 3.7 GB. |
| Metagame and deploy server (Node) | ~130 MB together | small | |

What that adds up to:

- **Ports are the hard limit, not RAM.** There are 6 hunt ports (8770-8775), so at most 6 hunts run
  at once, each with up to 4 players. Everyone shares one Ramsgate. The cooked 1.4.4 config sets
  `[/Script/Engine.GameSession] MaxPlayers=32`.
- Everything at once (Ramsgate, the Dojo, 6 hunts and your own client) comes to roughly 9.5 GB of
  working set, going by these light-load figures.
- **Our estimate is that 8-12 friends online is comfortable.** We have not load-tested it. Hunt-server
  CPU with 4 players in combat, and Ramsgate's memory growth over long uptimes, are still unmeasured.
- **Upload bandwidth:** the cooked 1.4.4 config caps each client connection at
  `MaxInternetClientRate=100000` bytes per second. That is at most about 0.8 Mbit/s of upload per remote
  player, so six remote players need up to about 5 Mbit/s.

Operational limits to know about:

- Game servers start 10 seconds apart (`SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP`). The matchmaker
  closes a group when it reaches 4 players, or once 20 seconds pass without anyone new joining it. When
  six groups are ready at once, the last one's server starts about 50 seconds after the first.
- A hunt server exits after **50 seconds in total** with nobody connected, and that counter never
  resets. A friend on a slow disk can arrive after their server is gone. The planned DLL fix makes the
  timeout configurable and resets it on join.
- **When all hunt ports are busy**, the deploy server throws `No free ports left!` and answers the
  metagame with an HTTP 500. Our metagame then marks the group's search as failed (the status poll
  answers `FAILED`); upstream's marked the group ready with host `""` and port 0, and the players hung.
  Keeping the group waiting until a port is free is planned.
- **To add hunt ports, lower `PORT_RANGE_BEGIN`. Never raise `PORT_RANGE_END`.** The DLL treats any
  port of 8776 or above as a persistent server and turns the idle exit off there, so a hunt on such a
  port would never shut down. Widen the firewall rule to match.
- The deploy server puts no memory limit on game processes yet. Planned guard: kill a hunt above about
  2.5 GB or Ramsgate above about 3 GB (Ramsgate then respawns), and refuse new servers when less than
  3 GB of RAM is free.
- **Ramsgate and the Dojo each keep a console window open on the host's desktop. Closing one kills
  that server** for everyone in it. Hunt servers are started with their window hidden.

### Measure it

The figures above are one player's, measured on our host. A server installed with the
[Windows server kit]({{ winserver_page.url | relative_url }}) measures itself: its stack supervisor
takes a sample every minute and adds it to `C:\DauntlessRevived\data\logs\performance\performance-<date>.csv`,
one file per UTC day, kept for 30 days. Each sample has:

- one `host` row: the machine's CPU, total and free RAM, free disk space on the install drive,
  network traffic, and how many game servers run and how many players are online
- one row per stack process (metagame, content server, gateway, deploy server) and per game server
  (Ramsgate, the Training Dojo, each hunt): its UDP port, when it started, its players, its CPU as a
  percentage of one core, and its memory

Only numbers are written: no player names, account ids or keys. `Stack.ps1 status` shows when the
last sample was taken. To take one now, run `C:\DauntlessRevived\bin\Write-PerformanceLog.ps1 -Once`
in an elevated PowerShell. To turn the log off, set `"PerformanceLog": false` in
`data\config\server.json` and run `Stack.ps1 restart`. The columns are listed in
[Files and data]({{ files_page.url | relative_url }}#performance-log). Keep the files private: they
show when the server is busy.

The files are CSV, so a spreadsheet opens them. A summary tool that turns them into a cost per hunt
and per Ramsgate player, and an estimate of how many players a machine can host, is still to come,
as are the metagame's own timings (request latency, event-loop lag and database time). Both are part
of roadmap 4.12. The sampler started from Vvoidddd's first version
([#6](https://github.com/mixutin/dauntless-revived/pull/6)). A hand-built host like the one on this
page has no sampler yet.

## Keeping the PC available

The server is up only while the host PC is on, awake and signed in.

- **No sleep or hibernation** on mains power. Letting the display turn off is fine.

  ```powershell
  powercfg /change standby-timeout-ac 0
  powercfg /change hibernate-timeout-ac 0
  ```

- **Windows Update restarts.** Set active hours to cover your play times (Windows allows up to 18
  hours), or pause updates before a session. On our host the active hours were 08-17, so evening
  sessions were exposed to automatic restarts.
- **Stay signed in.** Lock the screen instead of signing out. The game servers run in your session and
  read your account's `Game.ini`. Running them as a Windows service (session 0) is untested.
- **Start Tailscale before the stack**, because the metagame binds to the Tailscale address.
- **One command starts and stops the stack, but after a reboot nothing restarts on its own yet.** On
  our host, a `stack.ps1` script with `start`, `stop`, `restart` and `status` already runs the whole
  stack and backs up the database before every start and after every stop. It works locally but is
  not in the repository yet (item 1.1 on the [roadmap]({{ roadmap_page.url | relative_url }})). Still
  planned for milestone M4: starting it from a scheduled task at logon, and a supervisor that restarts
  a crashed metagame or deploy server with backoff. The Windows server kit already does both on a
  rented server: its `Stack.ps1` runs from scheduled tasks at startup and restarts a crashed component
  with backoff (see [Scripts and parameters]({{ scripts_page.url | relative_url }})). It works only on a
  server installed with the kit, not on a hand-built host like this one.

## Back up the database

Everything players own (accounts, characters, inventories, loadouts, progression, friends lists,
invite codes) lives in one SQLite file, the `DB_FILENAME` in the metagame `.env`. Ours is
`C:\dr\data\undaunted.db`. It starts small (ours was 132 KB with one player), but it also keeps each
character's save history for rollbacks (the code estimates up to about 3.5 MB per character at the
default settings) and item and progression logs that only grow. Every table is described on
[Files and data]({{ files_page.url | relative_url }}#the-database).

**The metagame runs any pending database migrations on every start, and takes no backup first.** Always
back up before updating the fork or pulling upstream changes.

**What our host does.** A hidden scheduled task backs the database up every hour and keeps the newest
48 copies plus the newest copy of each of the last 30 days. `stack.ps1` also takes a backup before
every start and after every stop, and it won't start the metagame if that backup fails. A restore test
passed. These scripts work on our host but are not in the repository yet (items 0.1 and 1.1 on the
[roadmap]({{ roadmap_page.url | relative_url }})). Copies off the PC are still open. On a server
installed with the Windows server kit, `Backup-DauntlessServer.ps1` does the same (hourly, before every
start and after every stop; it keeps the newest 48 backups plus the newest one of each of the last
30 days). On any other host, use one of the two
options below.

**Option 1: stopped copy.** Stop the metagame and copy the file. The database uses SQLite's default
rollback journal, so there is no separate `-wal` file to forget.

**Option 2: live backup.** SQLite's online backup API is safe while the metagame is writing, and
`better-sqlite3` (already a metagame dependency) exposes it. Save this as `backup-db.js` in the
`legacy/UndauntedMetagame` folder, so that `require` finds the module:

```js
// node backup-db.js  -> C:/dr/backups/undaunted-YYYYMMDDHHMM.db (UTC time)
const Database = require("better-sqlite3");
const fs = require("fs");
const src = "C:/dr/data/undaunted.db";
const dir = "C:/dr/backups";
fs.mkdirSync(dir, { recursive: true });
const stamp = new Date().toISOString().slice(0, 16).replace(/[-:T]/g, "");
const out = `${dir}/undaunted-${stamp}.db`;
new Database(src, { readonly: true, fileMustExist: true }).backup(out).then(() => {
  const check = new Database(out, { readonly: true }).pragma("integrity_check", { simple: true });
  console.log(out, check);   // prints "ok" for a sound copy
});
```

- Run it hourly with Task Scheduler while hosting, and before every update. Keep a rolling set, for
  example 48 hourly and 90 daily copies, and copy the dailies off the PC.
- **To restore:** stop the metagame, copy a backup over `DB_FILENAME`, then start the metagame.
- The backups contain every player's username and inventory, so keep them private.

**Back up the secrets separately, encrypted, and never in git.** Both `.env` files are already
git-ignored. They hold the metagame's JWT signing key pair and the deploy server's gameserver key. Also
back up the owner's account key.

- Losing the signing keys only invalidates tokens that are already issued. Generate a new pair and
  everyone logs in again as normal.
- Losing the owner key locks you out of the admin API until you replace its hash in the database (see
  `rekey.js` above).

## Before the first friend connects

1. `.env`: `AUTH_MODE=APIKEY`, `NODE_ENV=production`, `REGISTRATION_MODE=INVITECODE`.
2. Tailscale is installed, starts unattended, and the machine is shared with each friend.
3. The two firewall rules exist, scoped to the Tailscale interface. TCP 61001 is not open.
4. `MY_IP`, `QOS_TARGET_URL` and the metagame's `BIND_HOST` point at the Tailscale address. So do the
   `Game.ini` endpoint block and your own client's backend.
5. A database backup exists, and the hourly task is on.
6. Sleep is off, and the Windows Update active hours cover your play time.
7. The AGPL requirements are covered. Friends use the modified metagame over the network, so they are
   entitled to its source (section 13). The DLLs you hand them also need their source available
   (section 6). Give them the link to [this site's repository]({{ site.github.repository_url }}) and
   the commit you run. The in-game status text (`/dauntless-status`) welcomes players by your
   server's name ("Welcome to Dauntless Revived!" unless you set `SERVER_NAME` in the metagame's
   `.env`). After the fields the game reads, the same reply also carries the server's name, version,
   commit and source link (`SOURCE_URL` and `GIT_COMMIT`, see
   [Configuration]({{ config_page.url | relative_url }}#metagame-identity)). If you run modified
   code, point `SOURCE_URL` at your modified source. We plan to put the source link in the in-game
   text too. See
   [Credits and license]({{ legal_page.url | relative_url }}). It is not legal advice.
