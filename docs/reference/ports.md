---
title: Ports and network
parent: Reference
nav_order: 2
description: "Every TCP and UDP port of a Dauntless Revived server: who listens where, bind addresses in private and public mode, request paths and the kit firewall rules."
lang: en
ref: reference/ports
---

{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "reference/scripts.md" | first %}
{% assign dev_page = site.pages | where: "path", "reference/development.md" | first %}
{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}

# Ports and network
{: .no_toc }

Every port the Dauntless Revived stack uses, the address each one listens on, who may reach it,
how a friend's traffic travels, and the firewall rules the Windows server kit creates. The
environment variables named here are explained in full on
[Configuration]({{ config_page.url | relative_url }}), and the routes on
[HTTP API]({{ api_page.url | relative_url }}). How to set a server up is in the setup guides; this
page only collects the network facts.

Three setups come up throughout:

- **Dev PC**: everything on one Windows PC, for the owner alone, set up by hand as in
  [Host a server]({{ host_page.url | relative_url }}).
- **Private mode**: friends connect over Tailscale. Either the kit's `-Mode Private`, or a hand setup
  as in [Run it for a group]({{ admin_page.url | relative_url }}).
- **Public mode**: the [Windows server kit]({{ winserver_page.url | relative_url }})'s default. Friends
  connect over the internet through one TLS gateway port.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## At a glance {#at-a-glance}

| Port | Protocol | Who listens | Runs on | Reachable from other machines |
|:-----|:---------|:------------|:--------|:------------------------------|
| 443 | TCP | Gateway (`legacy/UndauntedGateway/`), TLS | Server, public mode only | Everyone. The only public TCP port. |
| 61000 | TCP | Metagame (`legacy/UndauntedMetagame/`) | Server | Public mode: never, only through the gateway. Private mode: Tailscale peers. Dev PC: nobody. |
| 61001 | TCP | Deploy server (`legacy/UndauntedDeployServer/`) | Server | Never. |
| 61002 | TCP | Content server (`legacy/UndauntedContent/`), game files for the launcher | Server, when installed | Public mode: never, only through the gateway. Private mode: Tailscale peers. |
| 61005 | TCP | Allowlist helper (`legacy/UndauntedGateway/`, runs as SYSTEM) | Server, public mode only | Never. |
| 61099 | TCP | Chat, inside the metagame (only with `CHAT=1`), on `127.0.0.1` | Server | Never directly. Public mode: through the gateway. No firewall rule anywhere. |
| 8777 | UDP | Ramsgate game server | Server | Players. Public mode: only addresses of logged-in players. Private mode: Tailscale peers. |
| 8776 | UDP | Training Dojo game server | Server | As 8777. |
| 8770-8775 | UDP | Hunt and tutorial game servers, one process per group | Server | As 8777. |
| 61000 | TCP | Launcher relay (`legacy/UndauntedLauncher/`) | Each player's PC, public mode, only while the game runs | Never: it listens on `127.0.0.1` only. |
| 22 | TCP | OpenSSH (Windows, not part of the stack) | Server | Public mode: allowed by the kit, for key-only administration. Limit it to your own address at the provider. |
| 3389 | TCP, UDP | Remote Desktop (Windows) | Server | Public mode: only from `-AdminIp`; without it the kit turns internet-open rules off, unless `-KeepRdpOpen`. |

The kit's sandbox (`-Sandbox`, for tests on a development PC) uses 62000, 62001, 62002, 62005, 62443
and 62099 instead; see [Test and sandbox ports](#test-ports).

## Server ports in detail {#server-ports}

| Component | Default port | Setting | Kit | What else points at it |
|:----------|:-------------|:--------|:----|:-----------------------|
| Metagame | none in code; 61000 by convention | `PORT` (metagame) | `-MetagamePort` (1024-65535), sandbox 62000 | The DLL's first client argument, the game servers' `Game.ini`, `GATEWAY_METAGAME_URL`, the content server's `METAGAME_URL` (default `http://127.0.0.1:61000`), `QOS_TARGET_URL` and invites in private mode, the friend kit's `-Server` (61000 when it names no port) |
| Deploy server | none in code; 61001 by convention | `PORT` (deploy server) | `-DeployPort` (1024-65535), sandbox 62001 | `DEPLOYSERVER_URL` in the metagame (`127.0.0.1:<port>`, no scheme) |
| Content server | 61002 | `PORT` (content server) | `-ContentPort` (1024-65535), sandbox 62002 | `GATEWAY_CONTENT_URL`; the metagame's `CONTENT_PORT`, which tells launchers the port (`contentPort` in ServerStatus) |
| Allowlist helper | 61005 | `ALLOWLIST_PORT` | `-AllowlistPort` (1024-65535), sandbox 62005 | The gateway's `ALLOWLIST_URL` (default `http://127.0.0.1:61005`) |
| Gateway | 443 | `GATEWAY_PORT` | `-GatewayPort` (1-65535), sandbox 62443 | The `port=` of every public-mode invite (the launcher assumes 443 when an invite leaves it out) |
| Chat (in the metagame, `CHAT=1`) | 61099 | `CHAT_PORT` (metagame), `GATEWAY_WS_URL` (gateway), `ServerPort` in `Engine.ini` | fixed; sandbox 62099; the kit writes the same port into `CHAT_PORT` and `GATEWAY_WS_URL` | The game's chat override, see [Chat port 61099](#chat-port) |

Notes:

- **The metagame and the deploy server have no port default.** `PORT` must be set in their `.env`.
  Without the line they stop at startup with Node's `ERR_SOCKET_BAD_PORT` error. An empty `PORT=` makes Node pick a random free port, which nothing else can find. A port
  that is already taken stops them at startup with `Could not listen on ...`. The metagame's
  `DEPLOYSERVER_URL` has no default either: without it every request for a game server, Ramsgate
  included, ends as `FAILED`.
- **Why 61000 and not 60000.** Upstream's development launcher expects `127.0.0.1:60000`. On our host
  another program already held that port, so the fork moved to 61000 and 61001. See
  [Troubleshooting]({{ trouble_page.url | relative_url }}#port-60000-is-taken-and-the-metagame-says-clear-skies-anyway).
- **How the kit picks ports.** An explicit parameter wins, then the value in the install's
  `data\config\server.json` (`Ports`; see [Files and data]({{ files_page.url | relative_url }})), then the
  default. After the first install, `server.json` is the
  source of truth. The ports in use must all differ, and the installer stops if one of them is taken
  (it only warns if something already listens in UDP 8770-8777). `-Sandbox` refuses any port outside
  62000-62499.
- **Keep `-GatewayPort` on every redeploy.** Whenever `Deploy-Remote.ps1` runs the installer in public
  mode, it passes `-GatewayPort`, 443 unless you give it. A redeploy without it moves a custom gateway
  port back to 443. Invites carry the port, so invites made for the old port stop reaching the server.
  The parameters are listed on [Scripts and parameters]({{ scripts_page.url | relative_url }}).

## Game server UDP ports {#game-udp-ports}

Game servers are further copies of the 1.4.4 client executable, started by the deploy server with
`-EpicPortal -server -nullrhi`. Each one listens on one UDP port, given as its second command-line
argument.

| Port | Game server | Started |
|:-----|:------------|:--------|
| 8777 (`PORT_RANGE_END`) | Ramsgate | When the deploy server starts. If it exits, it is started again on the same port when a player travels there (`PERSISTENT_WORLD_LIVENESS`, on) or by the deploy server's watchdog (every 60 seconds), whichever comes first; both share one launch, so there is never a second process on the port. |
| 8776 (`PORT_RANGE_END` - 1) | Training Dojo | On first use, or at startup with `ENABLE_DOJO=1`. Restarted on the same port the same way. |
| 8770-8775 (`PORT_RANGE_BEGIN` to `PORT_RANGE_END` - 2) | Hunts and the tutorial | One process per group of up to 4 players, from a pool of free ports, highest first (8775, then 8774, ...). |

- **Listening address.** The server DLL opens the port with no host, so the game server listens on
  every interface (`0.0.0.0`). The firewall decides who reaches it. The address players are sent to is
  the deploy server's `MY_IP`: `127.0.0.1` on a dev PC, the host's Tailscale IPv4 in private mode, the
  public IPv4 of `-PublicHost` in public mode.
- **The port goes back to the pool** when the watchdog notices that the hunt process has exited, so up
  to a minute later. A hunt server exits after 50 seconds in total with nobody connected.
- **When the pool is empty**, the deploy server answers the metagame with an HTTP 500
  `{"error": "no_game_server"}` (`Matchmaking for ... failed: No free ports left!` in its log), and the
  metagame marks that group's search as failed: its status poll answers `FAILED`. Six hunts can run at
  once with the default range.
- **Keep `PORT_RANGE_END=8777`.** The server DLL turns its 50-second idle exit off for any port of
  8776 or above. That is how Ramsgate and the Dojo stay up. With a higher end, hunts on 8776 and up
  would never exit; with a lower end, Ramsgate and the Dojo would exit when idle and be restarted over
  and over. For more simultaneous hunts, lower `PORT_RANGE_BEGIN` and widen the firewall rules to match.
- **The kit's range is fixed at 8770-8777.** It comes from constants in
  `deploy/windows-server/DauntlessServer.Common.ps1` (`$DRUdpBegin`, `$DRUdpEnd`). There is no
  installer parameter, and every installer run writes the range again into `deployserver.env`, the
  allowlist helper's `ALLOWLIST_PORTS` and the firewall rules. `server.json` records it
  (`UdpPortBegin`, `UdpPortEnd`), and `Stack.ps1 status` uses `UdpPortEnd` to label Ramsgate and the
  Dojo.
- **`PORT_RANGE_BEGIN`, `PORT_RANGE_END` and `MY_IP` must be set.** The deploy server has no defaults
  for them: without the range no hunt can start and Ramsgate gets no usable port, and without `MY_IP`
  the metagame gets no address to send players to, so every launch fails.

## The player's PC {#players-pc}

The game client needs no inbound port on the player's PC. It connects out: to the metagame (private
mode) or the local relay (public mode) over TCP, and to the game server's UDP port.

**Public mode: the launcher relay on `127.0.0.1:61000`.** The 1.4.4 client only speaks plain HTTP.
While the game runs, the launcher listens on `127.0.0.1:61000` and forwards every request, and
WebSocket upgrades, over TLS to the server's gateway, on a connection pinned to the certificate
fingerprint from the invite. The login, which carries the player's account key, goes the same way;
the account key is a secret (never share it, never commit it). The relay:

- listens on `127.0.0.1` only, exclusively, and stops when the game exits;
- answers 403 to callers that are not on the same PC, to a `Host` or `Origin` that is not local, and
  to requests with browser `Sec-Fetch-*` headers, so a web page cannot use it;
- answers 502 `certificate_mismatch` when the server's certificate does not match the invite, and
  502 `upstream_unreachable` when the gateway cannot be reached.

The port is fixed because the metagame hands every player the same QoS address,
`http://127.0.0.1:61000/QoS` (`QOS_TARGET_URL`), which is each player's own relay. If 61000 is taken,
for example by a development stack on the same PC, PLAY fails with `relay_port_busy`. The launcher
reads a hidden override, `DAUNTLESS_REVIVED_RELAY_PORT` (1024-65535), for tests and rehearsals only. It
moves the relay, the game's first argument and the chat port together, but the QoS address from the
server stays on 61000, and whether the region ping then still works has not been tested. Friends
always use 61000.

**Private mode and the friend kit.** Nothing listens on the player's PC. The game talks plain HTTP to
`<host>:61000` across Tailscale. The friend kit (`friend-kit/`) works this way only, so it cannot join a
public-mode server; that needs the launcher's relay.

## Chat port 61099 {#chat-port}

The 1.4.4 client would otherwise open its chat and presence WebSocket to Epic's live service and send
it the account id and login token. Every writer of `Engine.ini` points it elsewhere
(`[OnlineSubsystemMcp.XMPP]`, `bUseSSL=false`):

| Who | `ServerAddr` | `ServerPort` |
|:----|:-------------|:-------------|
| Dev PC (the scripts in [Host a server]({{ host_page.url | relative_url }})) | `ws://127.0.0.1` | 61099 |
| Launcher, private mode | `ws://<invite host>` | 61099 |
| Launcher, public mode | `ws://127.0.0.1` (the relay) | 61000 (the relay port); the gateway forwards the upgrade to `GATEWAY_WS_URL` |
| Friend kit (`play.ps1`) | `ws://<host part of -Server>` | 61099 |
| Kit, for the game servers (service account) | `ws://127.0.0.1` | 61099 |

With `CHAT=1` the metagame itself listens on 61099 for the game's chat
([Configuration]({{ config_page.url | relative_url }}#metagame-chat),
[Text chat]({{ '/findings/chat.html' | relative_url }})). It listens on `127.0.0.1` only, and no
firewall rule opens it in any mode.

- **Public mode** (the rented server): the game's chat connection goes to the launcher relay on the
  player's PC, over TLS to the gateway, and from there to `127.0.0.1:61099` (`GATEWAY_WS_URL`). The
  request target is `//`, with the protocol `xmpp`; the relay and the gateway pass both through, and
  launchers from v0.1.0 on already do this. The kit writes the same port into the metagame's
  `CHAT_PORT` and the gateway's `GATEWAY_WS_URL`, so they cannot disagree. With chat off the gateway
  answers the upgrade with 502, and the game retries every 15-45 s, harmlessly.
- **Dev PC** (one PC): the game connects to `ws://127.0.0.1:61099` straight. Set `CHAT=1` in
  `legacy/UndauntedMetagame/.env` ([Host a server]({{ host_page.url | relative_url }})).
- **Private mode** (Tailscale): not supported yet. The game would connect to `ws://<Tailscale
  address>:61099`, but the listener refuses any address other than loopback, so the connection fails
  harmlessly, as before.
- **Game servers** on a kit server: their `Engine.ini` points at `ws://127.0.0.1:61099` too. Whether
  they log in to chat at all is not known; a refused login shows as `chat: login refused ...` in the
  metagame log at most once per 10 minutes.
- **Kit sandbox**: the gateway and `CHAT_PORT` use 62099 instead, so a sandbox never reaches a
  development PC's own 61099.

The `Engine.ini` keys are on [Game settings]({{ gamesettings_page.url | relative_url }}).

## Bind addresses by setup {#bind-addresses}

| Setting | Dev PC | Private mode | Public mode | Kit sandbox (public mode) |
|:--------|:-------|:-------------|:------------|:------------|
| Metagame `BIND_HOST` | `127.0.0.1` | the host's Tailscale IPv4 | `127.0.0.1` | `127.0.0.1` |
| Deploy server `BIND_HOST` | `127.0.0.1` | `127.0.0.1` | `127.0.0.1` | (not started) |
| Content server `BIND_HOST` | `127.0.0.1` if you run it | the host's Tailscale IPv4 | `127.0.0.1` | `127.0.0.1` |
| `GATEWAY_BIND` | | | `0.0.0.0` | `127.0.0.1` |
| `ALLOWLIST_BIND` | | | `127.0.0.1` | `127.0.0.1` (dry run) |
| Game servers (UDP) | every interface | every interface | every interface | (not started) |
| Deploy server `MY_IP` | `127.0.0.1` | the host's Tailscale IPv4 | public IPv4 of `-PublicHost` | `127.0.0.1` |
| Metagame `QOS_TARGET_URL` | `http://127.0.0.1:61000/QoS` | `http://<Tailscale IPv4>:61000/QoS` | `http://127.0.0.1:61000/QoS` (the player's relay) | `http://127.0.0.1:61000/QoS` |
| Metagame `DEPLOYSERVER_URL` | `127.0.0.1:61001` | `127.0.0.1:61001` | `127.0.0.1:61001` | `127.0.0.1:62001` |

What each listener accepts:

| Listener | Accepts | Default |
|:---------|:--------|:--------|
| Metagame | One address. A list is not supported. With `GATEWAY_SECRET` set (public mode), a non-loopback address gives a startup warning. | `127.0.0.1` (upstream listened on every interface) |
| Deploy server | One address. Keep `127.0.0.1`: its routes answer 403 to any caller that is not on loopback anyway. | `127.0.0.1` |
| Content server | A comma-separated list of IP addresses (or `localhost`), one listener per address. Only loopback, `localhost`, Tailscale's 100.64.0.0/10 and fd7a:115c:a1e0::/48 are allowed, unless `CONTENT_ALLOW_ANY_BIND=1`. | `127.0.0.1` |
| Gateway | One IP address; host names are refused. Keep it IPv4 (`0.0.0.0`): the address the gateway reports to the allowlist must be the one the game's UDP traffic comes from, and `MY_IP` is IPv4. | `0.0.0.0` |
| Allowlist helper | `127.0.0.1` or `::1` only. Anything else is a startup error. | `127.0.0.1` |
| Chat (`CHAT_BIND_HOST`, in the metagame) | `127.0.0.1` or `::1`; with `GATEWAY_SECRET` set (public mode) `127.0.0.1` only. Anything else keeps chat off with an error line; the metagame starts anyway. | `127.0.0.1` |
| Gateway upstreams (`GATEWAY_METAGAME_URL`, `GATEWAY_CONTENT_URL`, `GATEWAY_WS_URL`) and `ALLOWLIST_URL` | Only `http://host:port` on this machine (127.x.x.x, `::1` or `localhost`), no path. The gateway secret never leaves the machine. | `127.0.0.1` with 61000, 61002, 61099 and 61005 |
| Launcher relay | `127.0.0.1` only, fixed in code. | `127.0.0.1:61000` |

In private mode the Tailscale address must exist before the metagame starts, or it exits with
`Could not listen on <address>:61000`. On a kit install, `Stack.ps1` waits up to 5 minutes for the
address to appear. Do not use `BIND_HOST=0.0.0.0` to avoid this: the metagame would then listen on
your LAN as well.

In public mode (`GATEWAY_SECRET` set) the metagame also warns at startup when `QOS_TARGET_URL` is not
`http://127.0.0.1:<port>/QoS`, because every player's client must ping its own relay.

## Request paths {#request-paths}

### Public mode {#request-path-public}

```text
Player's PC                                              Server
game ---HTTP---> relay 127.0.0.1:61000 ===TLS===> gateway :443 ---> metagame 127.0.0.1:61000
launcher ============================TLS=========> gateway :443 ---> content  127.0.0.1:61002
game ---------------------UDP-------------------> game server :8770-8777 (allowlist rule)
```

| Step | From | To | Carries |
|:-----|:-----|:---|:--------|
| 1 | Launcher | Gateway `<PublicHost>:443`, TLS pinned to the invite's fingerprint | Registration, server status, news and art, game file downloads |
| 2 | Game | Relay `127.0.0.1:61000`, plain HTTP on the player's PC | Every backend call, the login with the account key, the QoS ping, the chat WebSocket |
| 3 | Relay | Gateway `<PublicHost>:443`, TLS pinned | Everything from step 2 |
| 4 | Gateway | Metagame `127.0.0.1:61000` | Everything except `/content` and WebSockets. The gateway adds `X-Dauntless-Gateway` (the gateway secret) and `X-Forwarded-For` (the player's address). Only four `/undaunted/api` routes pass; see [HTTP API]({{ api_page.url | relative_url }}). |
| 5 | Gateway | Content server `127.0.0.1:61002` | `/content` and `/content/*` |
| 6 | Gateway | Chat `127.0.0.1:61099`, in the metagame | WebSocket upgrades: the game's chat (request target `//`). With chat off nothing listens there: 502. |
| 7 | Gateway | Allowlist helper `127.0.0.1:61005`, `POST /allow` with the allowlist secret | The player's address, after a successful login (`POST /account/api/oauth/token`) or a successful heartbeat that carried a bearer token |
| 8 | Allowlist helper | Windows Firewall | Opens UDP 8770-8777 for that address until 600 seconds after its last login or heartbeat |
| 9 | Metagame | Deploy server `127.0.0.1:61001` | Start or find a game server. The answer is `MY_IP` (the public IPv4) and the UDP port. |
| 10 | Game | Game server `<public IPv4>:8770-8777`, UDP | The game session, directly, not through the relay or the gateway |
| 11 | Game servers | Metagame `127.0.0.1:61000` | Loading and saving characters, with the game-server key. The address comes from the service account's `Game.ini`, the key from the game server's command line. |
| 12 | Content server | Metagame `127.0.0.1:61000` | The account-key check for each download (`GetUserInfo`, cached) |

### Private mode {#request-path-private}

```text
Player's PC               Tailscale (encrypted)             Server (100.x.y.z)
game     ---HTTP-------------------------------------> metagame 100.x.y.z:61000
launcher ---HTTP-------------------------------------> metagame :61000, content :61002
game     ---UDP--------------------------------------> game server 100.x.y.z:8770-8777
```

| Step | From | To | Carries |
|:-----|:-----|:---|:--------|
| 1 | Launcher | Metagame `http://<100.x.y.z>:61000` | Registration and server status |
| 2 | Launcher | Content server `http://<100.x.y.z>:61002` (the port comes from ServerStatus `contentPort`) | Game file downloads, news and art |
| 3 | Game | Metagame `http://<100.x.y.z>:61000` | Every backend call, the login with the account key, the QoS ping |
| 4 | Game | `ws://<100.x.y.z>:61099` | Chat. Not supported in private mode yet: nothing listens on that address, and the connection fails harmlessly. |
| 5 | Metagame | Deploy server `127.0.0.1:61001` | Start or find a game server. The answer is `MY_IP` (the Tailscale IPv4) and the UDP port. |
| 6 | Game | Game server `<100.x.y.z>:8770-8777`, UDP | The game session |
| 7 | Game servers | Metagame `http://<100.x.y.z>:61000` | Loading and saving characters, with the game-server key |
| 8 | Content server | Metagame `http://<100.x.y.z>:61000` | The account-key check |

Plain HTTP is only acceptable here because Tailscale encrypts the traffic between the machines. A
private-mode invite (`v=1`) only accepts a Tailscale address (100.64.0.0/10), a MagicDNS name
(`*.ts.net`) or a loopback address as its host, so the launcher never sends a key in plain HTTP across
the open internet. The game servers reach the metagame on its Tailscale address because the metagame
listens on that one address; a hand setup has to rewrite `Game.ini` for this (see
[Run it for a group]({{ admin_page.url | relative_url }}#switch-the-addresses-to-tailscale)).

On a **dev PC** every step stays on `127.0.0.1`, and no firewall rule is needed.

## What must never be exposed {#never-exposed}

- **The deploy server (61001).** It has no authentication: whoever can call it starts game processes
  on the machine. It listens on `127.0.0.1`, both of its routes answer 403 to callers that are not on
  loopback and to anything carrying a proxy header, the gateway has no route to it, and no setup opens
  it in the firewall. `DEPLOYSERVER_URL` must therefore be `127.0.0.1:<port>`.
- **The allowlist helper (61005).** It runs as SYSTEM and changes the firewall. It only listens on
  loopback, answers 403 to anything else, and needs `ALLOWLIST_SECRET`, which is a secret: never share
  it, never commit it.
- **The metagame (61000) in public mode.** Only the gateway may reach it. Going around the gateway
  would skip its admin-route block, its request size limit (128 KiB; the metagame itself accepts JSON
  bodies up to 50 MB) and its rate limits. In private mode, only Tailscale peers may reach it.
- **The content server (61002).** It serves the game files to registered accounts. It refuses to
  listen on anything but loopback or Tailscale addresses unless `CONTENT_ALLOW_ANY_BIND=1`; set that
  only if a firewall keeps the internet out.
- **The launcher relay (61000 on a player's PC).** It carries the account key. It only ever listens on
  `127.0.0.1`.
- **UDP 8770-8777 on a public server.** Open them to everyone at the provider, but never in Windows
  Firewall: there, only the allowlist rule may open them. Do not add other rules for the range, and do
  not answer Windows' "allow access" prompt for the game.
- **The admin API and the game-server key.** They never work through the gateway. The gateway
  answers 403 to admin routes and to any request carrying the game-server key header. The metagame
  refuses the admin key on any request with a proxy header, and accepts the game-server key only from
  its own machine (loopback or one of its own addresses; `GAMESERVER_ALLOW_FROM` adds hosts). Send
  admin calls straight to the metagame's own address: on the server, or over Tailscale in private
  mode. An admin account's key and the
  game-server key are secrets: never share them, never commit them.
- **The gateway secret.** `GATEWAY_SECRET` is the same in the metagame's and the gateway's settings
  and proves that a request came from the gateway. It is a secret: never share it, never commit it.

## Firewall rules the kit creates {#kit-firewall}

The installer puts its rules in the Windows Firewall group **Dauntless Revived** and rebuilds them on
every run: in public mode all of them except the allowlist rule, whose addresses and on/off state
belong to the allowlist helper (the installer only resets its protocol, ports and program); in
private mode all of them. Rule names show the actual port numbers; the tables
below use the defaults. `-WhatIf` lists the changes without making them.

### Public mode {#firewall-public}

| Rule | Protocol and port | Program | From | State |
|:-----|:------------------|:--------|:-----|:------|
| Dauntless Revived - gateway (TCP 443) | TCP, the gateway port | `node.exe` | Any address | Enabled, every profile |
| Dauntless Revived game ports (allowlist); rule name `DauntlessRevived-GamePorts-Allowlist` | UDP 8770-8777 | the game executable | Set by the allowlist helper (created with the placeholder 192.0.2.1) | Created disabled. The helper enables it while at least one address is allowed, and disables it when the list is empty. |
| Dauntless Revived - SSH (TCP 22) | TCP 22 | any | Any address | Only created when OpenSSH is installed and no enabled rule already allows TCP 22 |

No rule is created for the metagame, the content server, the deploy server or the allowlist helper.
The installer also changes existing settings, and records the old values under `FirewallChanges` in
`server.json`:

- **Remote Desktop.** With `-AdminIp`, every enabled inbound rule for TCP or UDP 3389 is limited to
  those addresses. Without it, rules open to any address are disabled, unless `-KeepRdpOpen`.
- **Profiles.** Every firewall profile is turned on, with inbound blocked and outbound allowed by
  default.
- **Policy values.** `EnableFirewall`, `DefaultInboundAction` and `AllowLocalPolicyMerge` values of 0
  under `HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall\<Profile>` are removed, because they override
  the local settings. If the firewall in effect is still off, the installer prints
  **RESTART THIS SERVER NOW**.

It only warns about other enabled rules that accept connections from any address on the Public
profile, and about other inbound rules for `node.exe` or the game executable.

The allowlist rule in more detail:

- An address stays in the rule until 600 seconds after its last login or heartbeat
  (`ALLOWLIST_TTL_SECONDS`). Only single public addresses are accepted (a sandbox also accepts private
  ones, `ALLOWLIST_ALLOW_PRIVATE=1`), at most 256 at a time (`ALLOWLIST_MAX_ENTRIES`).
- The helper saves the list, so after a restart the ports reopen for the same players until their
  time runs out.
- The rule closes when the helper stops normally, and a full `Stack.ps1 stop` run as administrator
  disables it too.
- If the rule is ever deleted, the helper recreates it without the program filter and outside the
  group. Run the installer again to put the filter back.
- The helper disables any other rule with the same display name, and the installer deletes such
  rules.
- With `GATEWAY_ALLOWLIST=0` the gateway reports no addresses, so the game ports open for nobody.

### Private mode {#firewall-private}

The group's rules are replaced with these, all enabled for every profile, from 100.64.0.0/10 only, and
tied to the Tailscale network adapter when one is found:

| Rule | Protocol and port | Program |
|:-----|:------------------|:--------|
| Dauntless Revived - metagame (TCP 61000, Tailscale only) | TCP, the metagame port | `node.exe` |
| Dauntless Revived - content server (TCP 61002, Tailscale only) | TCP, the content port (only when the content server is installed) | `node.exe` |
| Dauntless Revived - game servers (UDP 8770-8777, Tailscale only) | UDP 8770-8777 | the game executable |

The deploy server gets no rule. Profiles, SSH and Remote Desktop are left alone; the installer only
warns if a profile is off or allows inbound traffic by default. It also sets the Tailscale adapter's
network category from Private to Public, so rules meant for private networks do not apply to Tailscale
peers.

### Sandbox {#firewall-sandbox}

`-Sandbox` changes no firewall settings. Everything listens on `127.0.0.1`, and the allowlist helper
runs in dry-run mode: it only logs the firewall changes it would make.

### At your provider {#firewall-provider}

Many providers have their own firewall in front of the server. For public mode, allow TCP 443 (or your
`-GatewayPort`) and UDP 8770-8777 from any address there, and TCP 22 and 3389 only from your own
address. If the server's public address is not on its network adapter (1:1 NAT), the provider must
forward the same ports to it. Without the UDP rule, friends log in and then hang loading Ramsgate. See
[Before friends join]({{ winserver_page.url | relative_url }}#before-friends-join).

### Hand setups {#firewall-hand}

A dev PC needs no rule. For a hand-built private-mode host, [Run it for a
group]({{ admin_page.url | relative_url }}#firewall-allow-only-the-tailscale-interface) has the two
rules to add (TCP 61000 and UDP 8770-8777, Tailscale only). If you also run the content server, add a
third rule for TCP 61002 in the same way. Never open TCP 61001.

To remove the kit's rules, see [Uninstall]({{ winserver_page.url | relative_url }}#uninstall).

## Outbound connections {#outbound}

When the kit installs or updates a server, the server connects out over HTTPS: to nodejs.org
(Node.js), Microsoft (the Visual C++ and DirectX runtimes), the npm registry (`npm ci`),
pkgs.tailscale.com (private mode), GitHub (only when the server code is fetched from there) and the
host of `-GameZipUrl`. `Deploy-Remote.ps1` connects from your PC to the server over SSH (`-SshPort`,
default 22). The kit's firewall rule for SSH always uses TCP 22, so if sshd listens on another port,
allow that port yourself. The launcher checks the project's GitHub releases for its own updates.

## Test and sandbox ports {#test-ports}

The test suites and the kit sandbox listen on loopback, on ports from 62000 to 62999, and never on the
live ports 61000-61099. Several suites share port numbers, so run one suite at a time on a PC, and not while a
sandbox install or `Test-Sandbox.ps1` is running. How to run the tests is on
[Developer guide]({{ dev_page.url | relative_url }}).

| Suite | Ports |
|:------|:------|
| `legacy/UndauntedMetagame/`, `npm test` | 62014, 62015-62016, 62471-62472, 62481-62483, 62501-62502, 62901-62904, 62921-62922; the chat tests' own listeners take a random free port |
| `legacy/UndauntedDeployServer/`, `npm test` | 62013, 62473 |
| `legacy/UndauntedGateway/`, `npm test` | 62400-62499 (in use: 62400-62405, 62409-62417, 62420-62422, 62430-62436) |
| `legacy/UndauntedContent/`, `npm test` | 62011, 62012, 62019 |
| `legacy/UndauntedContent/`, `npm run test:integration` | 62002 and 62003 (`CONTENT_IT_PORT`, `CONTENT_IT_MOCK_PORT`) |
| `legacy/UndauntedLauncher/`, `npm test` | 62012, 62013, 62401-62404, 62409, 62420-62422, 62429, 62440-62444 |
| `deploy/windows-server/tests/Test-KitUnit.ps1` | 62450 and 62451 (`-Port` and the port above it, 62000-62499) |
| `-Sandbox` install and `Test-Sandbox.ps1` | Metagame 62000, content 62002, allowlist helper 62005, gateway 62443, chat (the gateway's WebSocket target and the metagame's `CHAT_PORT`) 62099. 62001 is written for the deploy server, which does not run in a sandbox. |

Known overlaps: the content integration test and the sandbox both use 62002; the launcher's
controller test uses 62443 like the sandbox gateway; the launcher tests share 62012, 62013,
62401-62404, 62409 and 62420-62422 with the content, deploy server and gateway tests.

## Checking what listens {#checking}

On a kit server, `C:\DauntlessRevived\bin\Stack.ps1 status` shows every component with its process and
ports, the game servers with their UDP ports (it looks for them in 8700-8799 and names `UdpPortEnd`
Ramsgate and the port below it the Dojo), and the allowlist. `Stack.ps1 start` waits up to 30 seconds
for each component to listen on its port, and up to 60 seconds for Ramsgate on UDP 8777. On any
Windows machine:

```powershell
Get-NetTCPConnection -State Listen -LocalPort 443,61000,61001,61002,61005,61099 -ErrorAction SilentlyContinue |
  ForEach-Object { "TCP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -ge 8770 -and $_.LocalPort -le 8777 } |
  ForEach-Object { "UDP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
```

Expect `node` on the TCP ports that apply to your setup, at the addresses in
[Bind addresses by setup](#bind-addresses), and `Dauntless-Win64-Shipping` on `0.0.0.0` for each
running game server. On 61099, `node` (the metagame) listens on `127.0.0.1` only when chat is on;
`Stack.ps1 status` says so in its `chat` line.
