---
title: How multiplayer works
parent: Findings
nav_order: 6
description: "How Undaunted runs multiplayer on the client-only Dauntless 1.4.4 build: an injected DLL turns extra client copies into game servers for Ramsgate and hunts."
lang: en
ref: findings/multiplayer
---

{% assign ci_page = site.pages | where: "path", "findings/client-internals.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}

# How multiplayer works
{: .no_toc }

The Dauntless executable is a client-only build. On its own it cannot host a game. Even so,
[Undaunted](https://github.com/SyST3MDeV/Undaunted) runs a shared Ramsgate and multi-player hunts on
the genuine **1.4.4** client. It does this by loading a DLL into extra copies of the same exe, which
turns each copy into a game server. This page explains why that works and how the pieces fit
together. Everything here about Undaunted's code applies to **1.4.4 only**. For the evidence that
the build is client-only, see [Client internals]({{ ci_page.url | relative_url }}).

> **Correction.** After disassembling the 2.1.1 client, we once wrote that multiplayer was
> impossible with the shipped files. The facts behind that were right: the server entry points really
> are compiled out. The conclusion was wrong. The network layer beneath those entry points is intact,
> and an injected DLL can drive it. The last section below explains what we got wrong.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## What was stripped, and what was not

A `WITH_SERVER_CODE=0` build loses the functions that make a process become a server. It keeps
most of what a server needs once it is one.

| Piece | State | Where (1.4.4 RVA unless noted) |
|---|---|---|
| `UWorld::Listen` | Stripped: a "return false" stub | `0x789370` (2.1.1: `0x140bfde50`) |
| `UWorld::InternalGetNetMode` | Always reports `NM_Client` when a net driver exists | `0x378BDA0` (2.1.1: `0x1453e4ef0`) |
| `GIsServer` / `GIsClient` | Forced to "client" in `FEngineLoop::PreInit` | globals at `0x5E4BC3A` / `0x5E4BC39` |
| `UEngine::CreateNamedNetDriver` | Present | `0x371A5E0` |
| `UIpNetDriver::InitListen` | Intact. It binds a UDP socket. | `0x806F80` (2.1.1: `0x140c6d200`) |
| Control-message handling (Hello, Login, Join) in `UWorld::NotifyControlMessage` | Intact | 2.1.1: `0x1453e5d40`, with a full `NMT_*` jump table |
| Game-mode login (`PreLogin`, `PostLogin`, spawning the player) | Intact | `AArchonGameMode::PostLogin` at `0x14B7460` |
| Actor channels and `ReplicateActor` | Intact | `CreateChannelByName` `0x3449E10`, `ReplicateActor` `0x327E860` |
| The engine's own server replication loop | Not used by Undaunted | Its `ServerReplicateActors_*` helper names are absent from both exes. We read this as stripped too, but missing names are weak evidence (**not proven**). |

So the missing parts are the call that opens a listening net driver, the net-mode answer, the two
global flags and, as far as we can tell, the server replication loop. Everything else is compiled
in, but the stock client never calls it.

---

## Undaunted's design

| Component | Language | Role |
|---|---|---|
| `legacy/UndauntedLauncher` | Electron | Checks the exe's SHA-256, copies the two DLLs into `Archon\Binaries\Win64` before every launch, and starts the client with the player's account key |
| `legacy/UndauntedMetagame` | TypeScript, SQLite (Drizzle) | The backend: login, characters, inventory, loadouts, progression, the matchmaking queue, an admin API |
| `legacy/UndauntedDeployServer` | TypeScript (Express) | Starts and supervises game-server processes |
| `UndauntedInternalServer.dll` | C++ (MinHook, a Dumper-7 SDK) | Injected into every game process, the client included. Runs in server mode or client mode. |
| `dxgi.dll` | prebuilt proxy | Loads the DLL into the process |

### Loading the DLL: the `dxgi.dll` proxy

- `dxgi.dll` is a static import of the 1.4.4 exe. A `dxgi.dll` placed next to the exe is therefore
  loaded when the process starts, before `FEngineLoop::PreInit`, even with `-nullrhi`. That timing
  matters, because the `GIsServer`/`GIsClient` patches must be in place before the engine
  initialises.
- The proxy ships prebuilt (11,264 bytes), and its source has not been published. How it loads the
  server DLL is our inference: the DLL exports a `DummyLinkFunc`, which points to a link by import.
  Writing our own proxy is on our roadmap, partly because of the AGPL.
- The DLL uses fixed RVAs and does no signature scanning. It has no version check of its own either.
  The launcher's hash check is the only guard. Loaded into any other exe, the DLL would write into
  the wrong code. The 1.4.4 exe it targets has SHA-256
  `d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4`, the value pinned in
  Undaunted's launcher. None of the offsets match 2.1.1.

### One DLL, two modes

The DLL picks its mode with one test: does the raw process command line contain `-server`? The test
is a case-sensitive substring match on the whole line, the exe path included. There is no
environment variable and no config file.

### Server mode

The deploy server starts each game server like this. It passes positional arguments first, then
three switches:

```text
Dauntless-Win64-Shipping.exe <gameserver key> <port> <map path>
  <behemoth class | NO_BEHEMOTH> <matchmaker hunt id | NO_MM_HUNTID>
  <uid:huntid,uid:huntid,... | NO_EXPECTED_PLAYERS> <ip:port>
  -EpicPortal -server -nullrhi
```

It needs at least eight arguments after the exe name. Otherwise it shows `INVALID GAMESERVER ARGS`
and exits. The `<ip:port>` argument is parsed but never used. The DLL then does the following:

1. **Makes `Listen` succeed.** It does not replace `UWorld::Listen`. Inside `UEngine::LoadMap`, at
   RVA `0x372E746`, it overwrites the 5-byte call to the stub with `mov al, 1` plus three `nop`s.
   `LoadMap` then continues as if listening had worked.
2. **Sets the global flags.** `DllMain` writes `GIsServer = 1` and `GIsClient = 0`. Five byte
   patches in `FEngineLoop::PreInit` stop the engine from overwriting them later. An earlier version
   rewrote the flags in a loop. The upstream commit "Don't hog a core writing to GIsServer/GIsClient"
   replaced that loop with these patches.
3. **Forces the net mode.** It hooks `InternalGetNetMode` to always return `NM_DedicatedServer` (1).
   The upstream commit that added this is titled "Force netmode, fixes arrivals and basically ALL
   weapon bugs".
4. **Controls the command line.** It hooks `FCommandLine::Get`, so the engine sees a fixed
   `-server -unattended -nullrhi -nosound -EpicPortal -RepDriverDisable`. The engine never sees the
   positional arguments. Because the hook replaces what `FCommandLine::Get` returns, it also gets
   past the command-line allow list described in
   [Client internals]({{ ci_page.url | relative_url }}).
5. **Boots straight into the map.** It hooks `UGameMapsSettings::GetGameDefaultMap` to return
   `<map>?MonsterClass=<behemoth>?HuntId=<id>?PlayerHuntIds=<uid:huntid,...>`, leaving out every
   `NO_*` part. A game mode override arrives as `?game=` inside the map argument.
6. **Opens the net driver.** Three seconds after a world exists, a hook on `UGameEngine::Tick` calls
   `CreateNamedNetDriver("GameNetDriver")`. The 1.4.4 config maps that name to `IpNetDriver`. The
   hook then calls `SetWorld`, and then the intact `UIpNetDriver::InitListen` with the port from the
   command line.
7. **Routes join traffic to the stock handlers.** `InitListen` is given the wrong notify pointer. A
   hook on `UIpNetDriver::TickDispatch` puts `&World->NetworkNotify` into the driver's notify field
   on every tick. That sends incoming Hello, Login and Join control messages to the stock `UWorld`
   handlers. The DLL has **no** `PreLogin` or `Login` hooks and does not touch player identity. The
   handshake and the game mode's login are Phoenix's own code.
8. **Replicates actors itself.** `SetReplicationDriver` is forced to null, and the engine's
   replication loop is not used. On every tick the DLL builds a list of relevant actors and calls
   `CallPreReplication`. For each open connection, it creates an actor channel where one is missing
   (`CreateChannelByName("Actor")`, `SetChannelActor`) and calls `ReplicateActor`. For the
   connection's own player controller it also calls what we believe is `SendClientAdjustment`. Two
   helper hooks make every actor count as level-initialized for every connection, and no connection
   count as saturated. (The two hooks' names are swapped in the source.)

Other server-side hooks:

- **Player-start selection (fixed in source on 1 October 2026, live confirmation pending).** Upstream's hook replaced Phoenix's native `GetPlayerStartForPlayer` with the first `APlayerStart` in the global UObject table. Reverse engineering the original 1.4.4 function at RVA `0x1368660` shows that Phoenix filters `AArchonPlayerStart::GroupName` and chooses among the matching starts with `AArchonPlayerState::PlayerStartSlot`. Ignoring both can select the wrong start/world; it is the leading cause of the observed bug where a player who falls off the map respawns underneath it. Our source now calls Phoenix's original selector first, and only if it returns null falls back to an `AArchonPlayerStart` in the same `UWorld`, preferring the requested group and the same slot rule. A VS2022 x64 candidate DLL builds successfully; it still needs the fall/respawn case verified in game before replacing the pinned DLL for everyone.
- Ability activation RPCs are handled directly, and then also passed to the original handler.
- Stamina is ticked for every player pawn.
- `HasFinishedLoading` is forced true.
- A byte patch labelled "Fixup Ramsgate Crash" makes one ability-input array read as empty.

**How a server talks to the backend.** Server mode adds an `x-undaunted-gameserver-apikey` header to
every HTTP request the process makes. The metagame accepts that header, and if the request also
carries a player's bearer token, it attributes the request to that player. Server mode does **not**
redirect any endpoint URLs, and the upstream code does not show how Undaunted's servers reach the
metagame. On our host, the server processes run under the same Windows account as the client. They
read the same user `Game.ini`, which has quoted overrides for all 167 endpoint keys pointing at the
metagame. See [Client internals]({{ ci_page.url | relative_url }}) for why the quotes are
required.

**Self-shutdown.** A hunt server calls `exit(0)` once it has spent a total of 50 seconds with no
player connected. The counter is never reset, so it also counts loading time before the first
player joins. The check is hard-coded off for ports 8776 and above. With `PORT_RANGE_END=8777`, as
in our setup, those are the Training Dojo and Ramsgate ports. Upstream's repository does not state
its own value; this check suggests it is 8777 there too.

### Client mode

**Hunt loading investigation (1 October 2026).** Live deploy logs show repeated
native hunt exits with `3221225477` (`0xC0000005`) and `3221226505`
(`0xC0000409`); saved game crash reports independently contain access violations.
This is evidence of crashed game processes, not proof that every long load has
the same cause. A source review also shows hunt allocation returning after process
creation, without a game-ready acknowledgement. A missing destination can therefore
leave a client trying to travel. A proper fix needs crash diagnosis and bounded,
authenticated readiness/liveness handling; do not pretend that spawning a process
means its map is ready, disable memory admission, or restart occupied worlds as a
workaround. No live server/DLL change was made during this investigation.

The separate `IGameClient` popup is from the client's EAC initialization. The
launcher now passes the pinned client's `-NoEAC` switch (0.1.11); that does not
repair a crashed hunt or alter matchmaking. Player-side confirmation is still
needed. The public-airship backfill and respawn candidate DLL tests remain open.

The player's client loads the same DLL. In client mode, the first argument is the metagame's
`host:port`, with no scheme.

- **Endpoints.** A hook on `FConfigCacheIni::GetString` rewrites **167 endpoint keys** to
  `http://<metagame>/...`. In every config section whose name contains `Mcp`, it also sets
  `Protocol` to `http` and `Domain`/`RedirectUrl` to the metagame. That sends Epic account and OAuth
  traffic to the metagame too.
- **Gameplay helpers.** `HasFinishedLoading` is forced true, Arena and Escalation hunts are unlocked,
  and a `UConsole` is created on **F2**.
- **Joining needs no help.** The client travels to the game server with its own, stock networking
  code.

---

## The deploy server

The deploy server is a small Express app. Upstream has a single endpoint,
`POST /api/matchmaker/handle-matchmaking-for-player`, which has **no authentication**, and listens on
all interfaces. Our fork binds it, and the metagame, to `127.0.0.1` by default. It adds a second,
read-only route, `GET /gameservers`, which lists the running game servers for the metagame's
`ServerStatus`. Both routes are still unauthenticated, and both answer 403 to any caller that is not
on loopback or that came through a proxy. [HTTP API]({{ api_page.url | relative_url }}#deploy-server)
has the details.

| Instance | Upstream | Our fork |
|---|---|---|
| Ramsgate | One shared, persistent process on `PORT_RANGE_END`, started at boot | Same, and started again when a player travels there while it is dead |
| Training Dojo | One persistent process on `PORT_RANGE_END - 1`, started at boot | Started the first time someone is matchmade into it. `ENABLE_DOJO=1` restores the upstream behaviour. |
| Hunts | **One process per matched group**, on a port from the pool `PORT_RANGE_BEGIN` to `PORT_RANGE_END - 2` | Same |

- **Routing.** Game mode `CITY` goes to Ramsgate. `SHARED` with the Training Dojo hunt id goes to the
  Dojo. `ISLAND` starts a new hunt server. Anything else falls back to Ramsgate.
- **Hunts.** A player hunt maps to a random matchmaker hunt. That supplies the behemoth and a random
  map from the hunt's map list, plus a `?game=` override where the hunt defines one. Trials pick a
  random row from the Hard or Elite table and always use the `arena_ramsgate_00` map.
- **Launching.** Server launches are queued one after another, with a configurable delay between
  them.
- **Supervision.** Once a minute, a watchdog checks each process with `process.kill(pid, 0)`. If the
  Ramsgate or Dojo process has disappeared, it starts a new one. It never kills anything, because
  hunt servers shut themselves down.
- **A dead Ramsgate on demand (our fork, 23 September 2026, from Harmonic's fork).** Before it hands
  out Ramsgate or the Dojo (a `CITY` trip, the Dojo, or the fallback to Ramsgate), the deploy server
  checks the same way that the process is still alive, and starts a dead one first
  (`PERSISTENT_WORLD_LIVENESS`, on by default; `0` leaves it to the watchdog, as before). Before, a
  player could be sent to a dead Ramsgate for up to a minute. The boot, the watchdog and players'
  requests share **one launch per world**, so there is never a second process on UDP 8777 or 8776, and
  the restarted process is stored as the world's record (upstream kept the old, dead one). A restart on
  demand waits in the launch queue like a hunt does, about 10 seconds with the kit.
- **Crashes of the deploy server itself.** Every game-server process has `error` and `exit` listeners:
  a wrong `GAMESERVER_BINARY_PATH` is logged ("Game server on port N failed: ...") and the hunt's port
  goes back to the pool, where it used to end the deploy server with an unhandled error. Every process
  end is one "exited with code" line. A failed start at boot is one fatal line and exit code 1 when
  the process later ends; the deploy server keeps serving, and the next trip to Ramsgate tries again.
  A matchmaking request whose game server cannot be started (no free port, a spawn failure) answers
  500 `{"error": "no_game_server"}`, and the metagame answers the players' status polls with `FAILED`.
- **The reply.** The deploy server returns `{host, port}` as soon as it has spawned the process. It
  does not wait for the server to be ready. A new server only starts listening about three seconds
  after its world exists. Harmonic's fork waits for the game's UDP port first; we left that out for
  now, because it would hold the player's travel request open for the whole server boot, the client's
  time limit for that request is unknown, and a 60-second deadline could kill a Ramsgate that boots
  slowly (roadmap 4.6). The fork's game-server log files were left out too: the server DLL gives the engine a
  fixed command line and redirects its output, so the files would stay nearly empty.

Our port layout on the host:

| Service | Address |
|---|---|
| Metagame | TCP `127.0.0.1:61000` |
| Deploy server | TCP `127.0.0.1:61001` |
| Game servers | UDP 8770–8777. Ramsgate on 8777, Dojo on 8776, hunts on 8770–8775. |

We moved the metagame off Undaunted's development default of 60000 because another application on
our host already uses that port.

---

## The matchmaking handoff

```text
client                     metagame                      deploy server           game server
  |                           |                               |                       |
  |-- POST /candidate/join -->|                               |                       |
  |                           |  hunts: wait for 4 players,   |                       |
  |                           |  or 20 s after the last join  |                       |
  |                           |-- POST /api/matchmaker/... -->|                       |
  |                           |                               |-- spawn exe + DLL --->|
  |                           |<------- {host, port} ---------|                       |
  |-- GET /candidate/status ->|                               |                       |
  |<-- MATCHING --------------|                               |                       |
  |-- GET /candidate/status ->|                               |                       |
  |<-- IN_PROGRESS + serverInfo {host, port}                  |                       |
  |                                                                                   |
  |== UDP: Hello / Login / Join (stock engine handshake) ============================>|
```

- **The status reply.** Undaunted's `/candidate/status` returns `MATCHING` until the server is
  known, then `IN_PROGRESS` with `serverInfo {buildId, gameSessionId, host, port}`. It asks the
  client to poll every 10 seconds (`candidateStatusPeriodMillis: 10000`). Our fork answers `FAILED`
  (with `statusReason: null` and no `serverInfo`) when the deploy server could not give a server: a
  500 or 503, a reply without a host, a quoted port, a body that is not JSON or a dropped connection,
  each tested on the Ramsgate, tutorial and queued-hunt paths (the failure cases come from Harmonic's
  tests). Upstream sent the group to host `""` and port 0 instead.
- **Grouping.** Hunts that need matchmaking are grouped by player hunt id. A group is sent to the
  deploy server when it reaches 4 players, or 20 seconds after the last player joined. The
  20-second check only runs when a client polls `/candidate/status`. Hunt ids containing `Ramsgate`
  or `Dojo`, and empty hunt ids, skip the queue and go to the deploy server at once.
- **One queued join per player.** The hunt server waits for every player it was told to expect. A
  player's new join therefore replaces any older join of theirs that is still waiting, in any queue,
  and so does a join for Ramsgate, the Dojo or a party's hunt. The expected-player list names each
  account once. Before 22 September 2026 a join that was never matched stayed in the queue, the
  same player's next join was added next to it, and a two-player hunt expected three players: the
  airship countdown froze with both real players ready. A party's members are still expected
  together, once each.
- **The client's side (2.1.1).** We mapped the client's handling of this reply in detail, but only
  on 2.1.1:
  - The HTTP status must be 200–206.
  - `port` must be a JSON number. A quoted port leaves it at 0, and travel then fails silently.
  - `host` must not be empty, or the client keeps polling forever.
  - `CANCELED` and `FAILED` stop matchmaking.
  - The client builds its travel URL as
    `%s:%d?ticket=%s?gameSessionId=%s?EncryptionToken=%s`, with `?` between every option.

  We have not re-derived these rules for 1.4.4, though Undaunted's replies fit them.

---

## Measured resource use

These figures are for **1.4.4** on our host, a desktop with an 8-core CPU and 32 GB of RAM, with one
player:

| Process | RAM | CPU |
|---|---|---|
| Ramsgate server (`-nullrhi`) | about 1.1 GB | about 0.2 of a core |
| Tutorial hunt server (`-nullrhi`) | about 0.9 GB | not measured |
| Player's client | 1.5–1.8 GB | not measured |

- We have not yet measured a 4-player hunt.
- Upstream's history shows performance fixes ("Emergency optimizations", "Small optimizations to
  replication loop").
- The deploy server starts game processes with no memory limit. A per-server memory guard is on our
  roadmap.
- The same risk showed up on 2.1.1. An uncapped client loading the city reached 9 GB there. Capping
  the texture streaming pool and lowering the scalability settings held it to about 2.8 GB.

---

## Why we once said it was impossible

Our 2.1.1 analysis proved, down to single instructions, that `UWorld::Listen` is a stub, that
`GetNetMode` can only answer "client", that `GIsClient` is forced on, and that the exe has no
`WindowsServer` platform name. From that we concluded that no command line, config or patch could
make the client host, and that real multiplayer would need a server build Phoenix never released.

The evidence was right. The conclusion was not. We treated "the stock code path cannot reach this
code" as "this code cannot run". What changed our minds was the report that other people were
matchmaking into Ramsgate with the same files. Undaunted shows the core of what is needed on 1.4.4:
one patched call, five byte patches, one net-mode hook, a direct call into the intact `InitListen`,
a fix for the net driver's notify pointer, and a hand-written replication loop. Its other hooks
cover the command line, the boot map and gameplay details.

We have not tried the same on 2.1.1. None of Undaunted's offsets carry over. The equivalent 2.1.1
functions we found (the `Listen` stub at `0x140bfde50`, `GetNetMode` at `0x1453e4ef0`, the
`GIsClient` write at `0x140c0f683`, `InitListen` at `0x140c6d200`) suggest the same approach would
apply there. That is **unverified**.

---

## Open questions

- **The proxy's source.** The `dxgi.dll` proxy's source is unpublished. We plan to write our own.
- **Prebuilt binaries.** We have not confirmed that the prebuilt `UndauntedInternalServer.dll` was
  built from the published source. We pin the hashes of the prebuilt DLLs we run and plan to build
  from source.
- **Double ability activation.** The server's RPC hook activates an ability directly and then also
  calls the original RPC. Whether that activates abilities twice is untested.
- **Four players.** Normal and 4-player hunts are reported working in Undaunted's history, but we
  have only run the tutorial hunt and Ramsgate ourselves.
