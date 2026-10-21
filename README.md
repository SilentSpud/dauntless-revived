<p align="right"><a href="README.fi.md">🇫🇮 Suomeksi</a></p>

<p align="center">
  <img src=".github/assets/banner.png" width="100%" alt="Dauntless Revived banner. On the left, the project logo: a dragon's head above the words Dauntless Revived. On the right: Private server revival, Dauntless 1.4.4. Dauntless Revived. Play the genuine Dauntless 1.4.4 client on a server you host yourself. Fork of Undaunted by gwog and contributors, AGPL-3.0. Unofficial fan project, not affiliated with Phoenix Labs.">
</p>

<p align="center">
  <a href="LICENSE.txt"><img alt="License: AGPL-3.0-only" src="https://img.shields.io/badge/license-AGPL--3.0--only-0D669C?style=flat-square&labelColor=031523"></a>
  <a href="https://mixutin.github.io/dauntless-revived/"><img alt="Documentation site" src="https://img.shields.io/badge/docs-mixutin.github.io-0D669C?style=flat-square&labelColor=031523"></a>
  <a href="https://discord.gg/ZJRprHzsgu"><img alt="Join the Dauntless Revived Discord" src="https://img.shields.io/badge/Discord-join%20the%20community-5865F2?style=flat-square&logo=discord&logoColor=white"></a>
  <a href="https://patreon.com/DauntlessRevived"><img alt="Optional support on Patreon" src="https://img.shields.io/badge/Patreon-optional%20support-FF424D?style=flat-square&logo=patreon&logoColor=white"></a>
  <a href="https://github.com/mixutin/dauntless-revived/discussions"><img alt="GitHub Discussions" src="https://img.shields.io/badge/discussions-ask%20%26%20share-0D669C?style=flat-square&labelColor=031523&logo=github"></a>
  <a href="https://github.com/mixutin/dauntless-revived/commits/dauntless-revived"><img alt="Last commit on the dauntless-revived branch" src="https://img.shields.io/github/last-commit/mixutin/dauntless-revived/dauntless-revived?style=flat-square&labelColor=031523&color=0D669C"></a>
  <a href="https://github.com/mixutin/dauntless-revived/actions/workflows/ci.yml?query=branch%3Adauntless-revived"><img alt="CI status on the dauntless-revived branch" src="https://img.shields.io/github/actions/workflow/status/mixutin/dauntless-revived/ci.yml?branch=dauntless-revived&event=push&style=flat-square&labelColor=031523&label=CI"></a>
</p>

# Dauntless Revived

**Dauntless Revived is a private-server revival of Dauntless**, Phoenix Labs' free-to-play co-op
monster-hunting game. The official servers shut down on **30 May 2025**. This project lets you play
the genuine **Dauntless 1.4.4** client (October 2020, Unreal Engine 4) again, on a server you host
yourself. It is a modified fork of **[Undaunted](https://github.com/SyST3MDeV/Undaunted) by gwog
(Gregory Morford) and contributors**, who built the parts that make this possible: the DLL that
turns the retail client into a game server, the deploy server, the metagame backend and the
launcher. This fork adds fixes, saved progression, server-side parties, a friend launcher built on
Undaunted's, a server kit for rented Windows machines and documentation. No game files are
distributed in this repository or on the docs site: you need your own copy of the 1.4.4 client.

> **This is not a public server.** It is run for a few friends. Anyone can host their own copy from
> this repository.

## Hosting partner — EU Gamehost

<p align="center">
  <a href="https://www.eugamehost.com/"><img src=".github/assets/eugamehost-partner.png" width="620" alt="EU Gamehost"></a>
</p>

**EU Gamehost is a partnered and recommended hosting provider for Dauntless Revived.** If you want a rented server instead of keeping your own PC online, see the [hosting partner guide](https://mixutin.github.io/dauntless-revived/setup/hosting-partner.html).

The measured light-load all-in-one footprint is roughly **9.5 GB**, so **VDS GAME 3 (12 GB)** is our recommended starting point for the full default stack and **VDS GAME 5 (24 GB)** is the headroom option. **GAME 1 (4 GB)** is useful for testing/control-plane-only deployments and **GAME 2 (8 GB)** for deliberately trimmed setups, but neither has enough RAM for our measured full default layout. Operators wanting dedicated hardware can look at the **Ryzen 7 5800X / 32 GB** option.

> **Partner / advertising disclosure:** EU Gamehost partners with this project. These recommendations use the project's own measured server footprint, not an independent hosting benchmark. Prices, hardware and availability can change; verify the current offer with EU Gamehost before ordering.

## Quick answers

### Can you still play Dauntless?
Not on the official servers: Phoenix Labs shut them down on 30 May 2025. If you have a copy of the
1.4.4 client, you can play it on your own server with this project.

### Is there a Dauntless private server?
Yes, this is one: a self-hosted server for the 1.4.4 client, based on Undaunted. It is not open to
the public, so you run your own and invite your friends.

### Can I join your server?
No. Our server runs on a rented machine, but only people the owner invites can make an account. The
[setup guide](https://mixutin.github.io/dauntless-revived/setup/) explains how to host your own.

### Does this project give out the game?
No. Neither this repository nor the docs site contains or links to game files. You need your own
copy of the client.

### Which version of the game does it work with?
Only **1.4.4**. The final client, 2.1.1, does not work, because Undaunted's server DLL hooks fixed
addresses inside the 1.4.4 executable.

### Do I need an Epic Games account?
No. Each player logs in with a personal account key issued by your own server.

### Is it official?
No. It is an unofficial, non-commercial fan project, not affiliated with Phoenix Labs or Epic Games.

More answers are in the [FAQ](https://mixutin.github.io/dauntless-revived/faq.html).

## Status

Gameplay rows below describe the latest live game tests from **22 September 2026**; infrastructure
rows also include the Linux/OpenBSD porting work completed on **1 October 2026**. The game was first
played solo on the host and rented server, then tested with two players over the internet on
22 September. Rows marked "(solo)" mean exactly that.

| Feature | State | Notes |
|---|---|---|
| Log in with a personal account key | Works | No Epic account needed |
| Tutorial | Works | Started on demand by the deploy server |
| Ramsgate (the hub city) | Works (solo) | A permanent Ramsgate server runs next to the backend |
| Training Dojo | Works | Starts the first time someone goes there |
| Hunts | Works (solo) | Played: a Lesser Boreus hunt and a pursuit on the host PC, and the new-player pursuit over the internet on the rented server |
| Crafting | Works | |
| Inventory, gear and quests | Works (solo) | Saved to SQLite; survives a client restart and a full server restart |
| Slayer level, mastery and the Hunt Pass | Works (solo), on by default | Start from the beginning (Slayer level 1, no mastery, an empty Hunt Pass) and are saved; every account owns the Elite Hunt Pass. Tested in game on a throwaway account, including a full restart. On the rented server on 22 September 2026: Slayer level 3, weapon mastery and behemoth mastery (rank 2, the first time behemoth mastery was seen in game), with the rank rewards confirmed by the game server. `PROGRESSION_MODE=stub` brings back upstream's fixed level 50 ([upgrade notes](https://mixutin.github.io/dauntless-revived/setup/upgrading.html)) |
| A server on a rented machine | Running | The [Windows server kit](https://mixutin.github.io/dauntless-revived/setup/windows-server.html) was deployed to a rented Windows Server 2019 VPS in public mode on 21–22 September 2026. Checked there: the stack starts at boot as the service account, Ramsgate runs and sends heartbeats, the gateway answers from the internet with the pinned certificate, and the hourly backup runs. In the first real test (22 September 2026) three game servers ran at once, and the UDP allowlist opened the game ports for the player and closed them after they left |
| Linux server port | Control plane working; real Wine/Proton game session next | [Linux server](https://mixutin.github.io/dauntless-revived/setup/linux-server.html): metagame, content, deploy, gateway and SQLite run natively under Linux/systemd; nftables provides the dynamic UDP allowlist; Ramsgate/hunts launch through a separate Proton/Wine wrapper. The local pinned 1.4.4 tree passed all 410 manifest size checks, the Node stack and database bootstrap pass, and a smoke test reached Ramsgate on UDP 8777 with the exact production argv. A real Proton/Wine Ramsgate + hunt session is still the next milestone. |
| OpenBSD server port | Native control plane CI-tested; live two-machine game session next | [OpenBSD server](https://mixutin.github.io/dauntless-revived/setup/openbsd-server.html): Node/SQLite services run natively on OpenBSD 7.9 under rc.d with PF for TLS redirect and a dynamic player table. The Windows x86_64 game process runs on a restricted Linux Proton/Wine worker over SSH. A real OpenBSD 7.9 VM builds/tests this port in CI. |
| Friend launcher | Released on Windows; Linux support added | The first release, 0.1.0, was published on [GitHub Releases](https://github.com/mixutin/dauntless-revived/releases/latest) by CI, with `SHA256SUMS.txt` and a build provenance attestation; installed launchers update themselves. The owner registered with it on the rented server and downloaded the game (about 11 GB) through the gateway. 0.1.1, published the same night, stopped turning off the game's automatic exposure, which had made Ramsgate far too dark. Not code-signed yet: on the owner's PC SmartScreen blocked the installer outright, and checking it against `SHA256SUMS.txt` and unblocking it worked ([how](https://mixutin.github.io/dauntless-revived/setup/friends.html)) |
| Playing with friends over the internet | Works (two players) | Tested on 22 September 2026 on the rented server. First one player: an invite, registration, the game download, the tutorial, Ramsgate, the Training Dojo and the first hunt. Then two players together: they saw each other in Ramsgate and hunted together, after queueing the same hunt within a few seconds of each other (parties don't work in game yet, see the next row) |
| Parties, friends and guilds | Built, awaiting a live test | Server side: party invites, accept and decline, promote, kick and leave, the whole party on one hunt server, back to Ramsgate together, lookups by name, a friends list and blocklist, and guilds (create, invite, ranks, kick, leave, disband), all but parties saved in SQLite. In the two-player test on 22 September 2026 a party invite reached the other player's game but never showed, and Add Friends did nothing: the server described the wrong player in `POST /accountinfo/public` and answered `POST /account/mapping` in a shape the game does not read. Both are fixed, running on our rented server since 22 September 2026, and pass tests that replay the game's own requests; nobody has tried them in the game yet ([details](https://mixutin.github.io/dauntless-revived/findings/social.html)). Invites don't require being friends. Showing players as online needs presence over the chat connection: built since 23 September 2026, off by default (see the chat row) |
| Backups | Works on our hosts | Hourly, plus one around every server start and stop. The Windows server kit has its own backup task (running on the rented server). The scripts of our original host PC, where a restore was tested, are not in this repository ([do-it-yourself version](https://mixutin.github.io/dauntless-revived/setup/admin.html#back-up-the-database)) |
| Friend kit | Built | The Tailscale-only fallback: hash-checked setup and play scripts. Not used by a friend yet |
| Bounties | Not yet | Stored with real progression; drafting and claiming not yet tried in game |
| Text chat and online status | Built, awaiting a live test; both off by default | Ramsgate, hunt, party and guild chat and whispers, with usernames, in the metagame itself; off by default until two players have tried it (`Set-Chat.ps1 -On` on a kit server). The first chat server was written and tested by Vvoidddd; why it showed `UID-...` instead of names, and the fix: [Text chat](https://mixutin.github.io/dauntless-revived/findings/chat.html). Friends' online status is built into the chat server since 23 September 2026 (the idea came from Harmonic's fork) and stays off (`CHAT_PRESENCE=1`) until two players have shown that the party's automatic kick stays asleep. Use the [Dauntless Revived Discord](https://discord.gg/ZJRprHzsgu) meanwhile |
| Slayer Links | Built, on by default, awaiting a live test | The My Links tab: two friends link up for a week. From Harmonic's fork, corrected against the 1.4.4 executable (23 September 2026); not tried in game yet, and the game may keep the tab hidden ([details](https://mixutin.github.io/dauntless-revived/findings/social.html#slayer-links)) |
| Escalation | Built, off by default | Real Escalation saves from Harmonic's fork: the season registry read from the client and its save rules (23 September 2026). Off (`ESCALATION_MODE=real`) because switching it on drops every player from the fake maximum to level 0; not tried in game ([details](https://mixutin.github.io/dauntless-revived/findings/escalation.html)) |
| Store | Built, off by default | A free store from Harmonic's fork: 200 free cosmetics, bought with the game's own purchase flow (23 September 2026). Off (`STORE=free`) until the owner decides whether it stays free and it has been tried in game ([details](https://mixutin.github.io/dauntless-revived/findings/store.html)) |
| Multiple loadouts | Backend complete, awaiting an in-game UI test | Unlocks, active-slot selection and independent saves for all six client slots are stored with real progression. The server never advertises the invalid seventh slot, and a save no longer depends on `/all` having run first. The extra slots have not yet been tried in the game UI |

The live checklist, with every step and what "done" means for it, is [ROADMAP.md](ROADMAP.md).

## Quick links

| Link | What you'll find |
|---|---|
| [Documentation site](https://mixutin.github.io/dauntless-revived/) | Status, setup, findings and credits |
| [Setup guide](https://mixutin.github.io/dauntless-revived/setup/) | Hosting a server, joining as a friend, running it for a group, troubleshooting |
| [Windows server kit](https://mixutin.github.io/dauntless-revived/setup/windows-server.html) | An always-on server on a rented Windows Server 2019 machine, installed with one command, in public or private mode |
| [Launcher download](https://github.com/mixutin/dauntless-revived/releases/latest) | The Dauntless Revived Launcher for invited friends, with `SHA256SUMS.txt` |
| [Reference](https://mixutin.github.io/dauntless-revived/reference/) | Every setting, port, HTTP route, file and script, with defaults, for self-hosters and developers |
| [Friend kit](friend-kit/) | One-time setup and launcher for invited players ([guide](https://mixutin.github.io/dauntless-revived/setup/friends.html)) |
| [Roadmap](ROADMAP.md) | Milestones M0 to M4 and the live checklist |
| [FAQ](https://mixutin.github.io/dauntless-revived/faq.html) | Short answers to common questions |
| [Suomeksi](https://mixutin.github.io/dauntless-revived/fi/) | The documentation in Finnish |
| [Discord](https://discord.gg/ZJRprHzsgu) | Community chat, finding players, help and project updates |
| [Discussions](https://github.com/mixutin/dauntless-revived/discussions) | Questions, ideas and your own setups |

## How it works

1. Each player runs the unmodified 1.4.4 client with two DLLs from Undaunted next to it; they point the game's backend calls at your server.
2. The **metagame** (TypeScript, Express, SQLite; TCP 61000) handles accounts, characters, inventory, loadouts, progression, matchmaking, parties and the friends list.
3. The **deploy server** (TCP 61001, this machine only) starts game servers on demand: more copies of the same client, switched into server mode by the DLL.
4. Those game servers (UDP 8770 to 8777: Ramsgate on 8777, the Training Dojo on 8776, up to 6 hunts on 8770 to 8775) host the game itself.
5. Friends connect in one of two ways. In **private mode** they reach the host over Tailscale. In **public mode** the TLS **gateway** is the game's only public TCP port: the friend launcher checks its certificate against the fingerprint in the invite, and the game's UDP ports open only for the addresses of players who logged in. Windows uses Windows Firewall, Linux uses nftables, and OpenBSD uses a PF player table plus redirect/NAT to its Linux game worker. A **content server** behind the gateway hands the game files to registered accounts, and the launcher checks every file against a list built into it.

## Repository layout

The repository is moving to one folder per code family: `rust/` for every Rust component (the
launcher backend, the servers and their shared crates), `native/` for C++, `client-mods/` and
`tools/`. The target layout and the order of the moves are in
[`MIGRATION_PLAN.md`](MIGRATION_PLAN.md#2-repository-and-workspace-layout). Until its Rust port
replaces it, each Node project (the Electron launcher and the TypeScript servers) keeps its upstream
`Undaunted...` folder under `legacy/`, so the old code and the new are easy to tell apart; the npm
packages in them are named `dauntless-revived-*`. The server kits still install them at the top of the
install root, as before.

| Folder | What it is |
|---|---|
| `legacy/UndauntedMetagame/` | The backend the game talks to: accounts, characters, inventory, loadouts, progression, matchmaking, parties, the friends list and the `/undaunted/api` admin API |
| `legacy/UndauntedDeployServer/` | Starts and supervises game-server processes (Ramsgate, hunts, the Training Dojo) |
| `legacy/UndauntedGateway/` | Public mode only: the TLS gateway (the game's one public TCP port) and the helper that opens the game ports for logged-in players |
| `legacy/UndauntedContent/` | The content server: the game files (checked against a manifest of 410 files), news and the art pack, for registered launchers only |
| `legacy/UndauntedLauncher/` | The Dauntless Revived Launcher for Windows and x86_64 Linux; on Linux the pinned Windows game client runs through Proton/Wine. `assets/` holds the two pinned prebuilt DLLs every setup installs |
| `rust/` | The cargo workspace every Rust component is being built in. So far: `crates/protocol`, the wire types the launcher and the servers share |
| `native/internal-server/` | The C++ source of Undaunted's server DLL, which lets the retail client run as a game server and points clients at the backend. It compiles with the Visual Studio 2022 Build Tools, but nothing in this repository builds it; every setup uses the prebuilt DLLs from `legacy/UndauntedLauncher/assets/` |
| `client-mods/` | The optional UE4SS client mods the launcher installs next to the game exe, with `manifest.json` listing their files |
| `deploy/windows-server/` | The Windows server kit: installs and runs the whole server on Windows Server 2019, in public or private mode, with backups, invites and updates |
| `deploy/linux-server/` | Separate Linux server port: systemd service templates, nftables allowlist, one-host Proton/Wine launch wrapper and the reusable Linux game-worker installer |
| `deploy/openbsd-server/` | Separate OpenBSD 7.9 control-plane port: rc.d services, PF redirect/allowlist rules and the restricted SSH launcher for a Linux game worker |
| `deploy/unix-common/` | Shared Linux/OpenBSD config, SQLite/bootstrap, invite and compatibility-runner helpers; the Windows kit does not use these |
| `friend-kit/` | The Tailscale-only setup and play scripts for invited friends' PCs |
| `tools/` | `sync-roadmap.ts` and `build-llms.ts` (the generated docs files), `make-friend-kit.ps1`, `make-game-manifest.ts`, and in `ci/` the checks CI runs |
| `brand/` | The logo, emblem and palette, and `build.py`, which renders every size the launcher, the docs and GitHub use |
| `docs/` | The documentation site (GitHub Pages), with the Finnish pages in `docs/fi/` |
| `.github/` | The CI workflows (`ci.yml`, `launcher-release.yml`), Dependabot, and the issue and pull request templates |
| `ROADMAP.md` | The plan and live checklist |
| `MIGRATION_PLAN.md` | The plan for moving the launcher to Tauri and every server to Rust, and the repository to this layout |

## Changes from upstream

- The metagame and deploy server listen on this machine only unless told otherwise (`BIND_HOST`).
  Upstream listened on every network interface, and the deploy server has no authentication.
- Failing to bind a port is now a hard error. Upstream reported success anyway and exited silently.
- Every request the metagame receives is logged, marking calls made by game servers.
- The Training Dojo starts the first time someone goes there instead of at boot (`ENABLE_DOJO=1`
  restores the old behaviour).
- Tokens are removed from the request log. An optional capture of request bodies for the save
  routes that are not finished yet (`LOG_BODIES=1`) records what the game sends, capped at 8 KB per
  request (64 KB for `/inventory`) and with tokens removed. The Windows server kit keeps it off in
  public mode.
- **Safer saves.** Character saves are transactional and refuse a stale version (409). An inventory
  transaction that arrives twice is applied once, and every item change goes into an append-only
  log. Character and loadout history can be rolled back from the admin API. Upstream also never
  reported removed stacks after a transaction, so spent Rams and materials stayed on screen and a
  second upgrade went through for free; that is fixed.
- **Usernames and invites.** Names of 3 to 16 letters, digits and underscores, unique regardless of
  case, checked together with the invite code; the admin can rename a player.
- **A permission check on every route.** Players can only read and write their own account and
  characters; game-server routes need the game-server key and a direct local connection.
- **Parties, friends and guilds** (server side, not yet tried with two real game clients): invites,
  promote, kick and leave, the whole party on one hunt server and back to Ramsgate together, lookups
  by name, a friends list and blocklist, and the 1.4.4 client's guilds, saved in SQLite. Upstream's
  account info reply described the caller instead of the asked player, which hid every other player
  in the Social panel; ours answers for the asked account.
- **Work ported from Harmonic's 1.4.4 fork** ([Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted),
  September 2026): real Escalation saves and a free in-game store (both built, off by default),
  Slayer Links, friends' online status inside our chat server (off by default), a dead Ramsgate
  started again when a player travels there, protection against retried progression grants, and
  more. What was taken and what was not, with the reasons:
  [The Harmonic port](https://mixutin.github.io/dauntless-revived/findings/harmonic-fork.html).
- **Live server status for the launcher.** The list of players online and running hunts is shown to
  registered players only. `/dauntless-status` gives the server's name, version, source URL and
  commit (for the AGPL), and no player count.
- **Real progression by default.** Upstream answered progression with a fixed template (every
  account at level 50, a Hunt Pass with nothing to claim, and most likely the Elite track locked) and saved
  nothing. Our metagame stores Slayer level, mastery, the Hunt Pass, entitlements (the Elite pass
  for everyone), loadout slots, cooldowns and bounties per account. `PROGRESSION_MODE=stub` restores
  upstream's behaviour. Updating a server that already has players? Read the
  [upgrade notes](https://mixutin.github.io/dauntless-revived/setup/upgrading.html) first.
- A **friend kit** (`friend-kit/`, packaged by `tools/make-friend-kit.ps1`): a one-time setup and a
  launcher for invited players. It checks the two Undaunted DLLs against pinned SHA-256 hashes,
  registers the player, points the game's chat connection at the host so it never contacts Epic's
  old chat server, and ships with the license, third-party notices and a `SOURCE.txt` naming the
  exact commit.
- Our own **friend launcher** (`legacy/UndauntedLauncher/`), built on upstream's, which was hard-wired to
  Undaunted's own servers. It joins a server from an invite, registers the player (it keeps the key
  only through Electron safe storage (DPAPI on Windows, the desktop secret store on Linux); the one plain copy is a backup the player chooses to save),
  downloads the game from the host and checks every file against a manifest
  built into it, installs the two pinned DLLs and the game settings, and in public mode relays the
  game's plain-HTTP calls over TLS pinned to the invite's certificate. CI publishes each new version on GitHub Releases, and installed launchers
  update themselves.
- A **content server** (`legacy/UndauntedContent/`) that hands the game files only to registered accounts,
  with resumable downloads, and a **gateway** (`legacy/UndauntedGateway/`) for public mode: the game's only
  public TCP port, TLS with a pinned self-signed certificate, admin routes and the game-server key refused
  from outside, body limits and rate limits, and a helper that opens the game's UDP ports only for
  the addresses of players who logged in.
- A **Windows server kit** (`deploy/windows-server/`): one command from your PC installs the whole
  server on a Windows Server 2019 machine over key-only SSH, with a low-privilege service account,
  start at boot, supervision, hourly backups, invites and updates.
- A separate **Linux server port** (`deploy/linux-server/`): the Node/SQLite control plane runs
  natively under systemd, public game ports use a dynamic nftables allowlist, and the pinned Windows
  1.4.4 game processes run through Proton/Wine without changing the Windows deployment path.
- A separate **OpenBSD server port** (`deploy/openbsd-server/`): the control plane runs natively on
  OpenBSD 7.9 under rc.d, PF handles public TLS redirect and per-player game UDP, and a restricted SSH
  command starts each proprietary Windows game process on a Linux Proton/Wine worker.
- Hunt servers start with their console window hidden.
- **CI** on every push: the builds and tests of every package, the Windows server kit, the separate
  Linux server port, a real OpenBSD 7.9 VM build/test for the OpenBSD port, the docs build, and a check
  that no secrets, keys, databases or game files are committed.
- **Our own name.** The launcher, the in-game welcome text and the server's messages say Dauntless
  Revived, and the credits name Undaunted. The folders, the server DLL's file name
  (`UndauntedInternalServer.dll`), the `/undaunted/api` routes and the `x-undaunted-*` headers keep
  the Undaunted names for now (roadmap item 4.15).
- A **documentation site** in `docs/`, published at
  [mixutin.github.io/dauntless-revived](https://mixutin.github.io/dauntless-revived/), in English
  and Finnish: setup guides, a reference of every setting, port, route and file, findings about the
  game's backend, and the roadmap.

Backups: the Windows server kit takes them itself (hourly, and around every start and stop), each
database copy checked with `PRAGMA integrity_check`. Our original host PC runs its own backup scripts
(roadmap item 0.1), which are not part of this repository. The metagame itself takes no backup, so if
you host by hand, set one up yourself:
[Run it for a group](https://mixutin.github.io/dauntless-revived/setup/admin.html#back-up-the-database)
has a do-it-yourself version.

## Security

- The metagame and the deploy server bind to `127.0.0.1` by default.
- The deploy server has **no authentication by design**. Never expose it beyond the host machine.
- Friends connect either over Tailscale (private mode) or through the gateway (public mode). The 1.4.4
  client itself speaks plain HTTP, so in public mode it only ever talks to the launcher on the
  player's own PC, which forwards over TLS pinned to the server's certificate. Of the server's game
  services, only the gateway's TCP port is open to everyone; the game's UDP ports open only for
  players who logged in.

Found a vulnerability? Please report it privately through
[private vulnerability reporting](https://github.com/mixutin/dauntless-revived/security/advisories/new),
not in a public issue. [SECURITY.md](SECURITY.md) has the details and the scope.

## Contributing

Ideas and fixes are welcome. Please open a [Discussion](https://github.com/mixutin/dauntless-revived/discussions)
before you start on anything big, test on a throwaway account, and never commit keys, `.env` files
or game files. [CONTRIBUTING.md](CONTRIBUTING.md) has the short version of the rules, and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) covers how we treat each other.

## Related projects

- **[Undaunted](https://github.com/SyST3MDeV/Undaunted)**: the upstream project this fork is based on.
- **[Mystic Paradox](https://github.com/pranav158/Mystic-Paradox)**: ports the same approach to the
  Dauntless 1.12.0 client, for content released after 1.4.4. We have not tested it, and this fork
  contains none of its code.

## Credits

- **[Undaunted](https://github.com/SyST3MDeV/Undaunted)** by gwog (Gregory Morford,
  [SyST3MDeV](https://github.com/SyST3MDeV)), [EisigesEis](https://github.com/EisigesEis) and
  [its other contributors](https://github.com/SyST3MDeV/Undaunted/graphs/contributors). gwog
  created the server-mode DLL, the deploy server, the metagame and the launcher; EisigesEis worked
  on the metagame (inventory and loadouts, progression and mastery, invite codes, the admin API).
  Multiplayer Ramsgate and hunts on 1.4.4 are their achievement.
- **[MinHook](https://github.com/TsudaKageyu/minhook)** by Tsuda Kageyu (BSD 2-Clause): the
  function-hooking library in the server DLL.
- **[Dumper-7](https://github.com/Encryqed/Dumper-7)** by Encryqed and contributors: the Unreal
  Engine SDK generator the server DLL is built against.
- **Phoenix Labs**, who made Dauntless.

### Contributors

- **[mixutin](https://github.com/mixutin)** (maintainer): the server kit, the launcher, backend
  fixes, real progression and the docs.
- **[Vvoidddd](https://github.com/Vvoidddd)**: found the cause of the dark pre-hunt airship (1.4.4's
  automatic exposure; the setting was reverted in launcher 0.1.1 because it darkened Ramsgate), hid
  the console windows of temporary hunt servers, and added the repository's `.gitignore`
  ([#5](https://github.com/mixutin/dauntless-revived/pull/5)); wrote the first performance sampler,
  the base of the server's performance log ([#6](https://github.com/mixutin/dauntless-revived/pull/6));
  added the launcher's opt-in "Basic adaptive" auto exposure setting for the airship, off by default
  ([#7](https://github.com/mixutin/dauntless-revived/pull/7)); let the launcher use an existing
  Dauntless 1.4.4 folder by pasting its path ([#8](https://github.com/mixutin/dauntless-revived/pull/8));
  wrote the first in-game text chat server (XMPP) and tested it with a real 1.4.4 client
  ([#9](https://github.com/mixutin/dauntless-revived/pull/9)).
- **Harmonic** ([Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), Harmonic's
  Dauntless 1.4.4 fork of Undaunted): the Escalation season registry and save rules, the free store (its
  catalogue, purchase flow and store tabs), the first working Slayer Links, the deploy server's restart
  of a dead Ramsgate, the idea of friends' online status, and the test cases we ported from that fork.
  [NOTICE.md](NOTICE.md) lists what came from it.

Everyone who has contributed is listed on the
[contributors page](https://github.com/mixutin/dauntless-revived/graphs/contributors).

The full list is on the docs site's
[Credits and license](https://mixutin.github.io/dauntless-revived/legal.html) page.

## License

This is a **modified version of [Undaunted](https://github.com/SyST3MDeV/Undaunted)** by gwog
(Gregory Morford) and contributors. Modifications began on **21 September 2026**; every change is
recorded in this repository's commit history, and planned work is in [ROADMAP.md](ROADMAP.md).

GNU Affero General Public License v3.0 only — see [LICENSE.txt](LICENSE.txt). If you run a modified
version for other people, you must offer them its source code. [NOTICE.md](NOTICE.md) records where
each part of the repository comes from, including the work ported from Harmonic's 1.4.4 fork.

Not affiliated with or endorsed by Phoenix Labs or Epic Games. "Dauntless" is a trademark of its owners.
No game files are distributed in this repository or on the docs site.
