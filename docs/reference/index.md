---
title: Reference
nav_order: 3
has_children: true
has_toc: false
description: "Reference for Dauntless Revived: every setting, port, HTTP route, file, game setting and script, with defaults and who sets them, checked against the code."
lang: en
ref: reference/index
---

{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign ports_page = site.pages | where: "path", "reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "reference/scripts.md" | first %}
{% assign dev_page = site.pages | where: "path", "reference/development.md" | first %}
{% assign setup_page = site.pages | where: "path", "setup/index.md" | first %}
{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}

# Reference

These pages list the facts about the Dauntless Revived servers and scripts: every setting, port,
route, file and parameter, with its default, the values it takes and who sets it. Each fact was
checked against the code. Where a guide and the code disagreed, the code won and the guide was
corrected.

The [Setup]({{ setup_page.url | relative_url }}) pages are the step-by-step guides. Start there if
you want a working server: [Host a server]({{ host_page.url | relative_url }}) for one PC, or the
[Windows server kit]({{ winserver_page.url | relative_url }}) for a rented server. Come here when you
need to know exactly what a setting does or why something behaves the way it does.

**Real progression is the default.** These pages describe the current code, where each account
stores its own Slayer level, mastery and Hunt Pass unless you set `PROGRESSION_MODE=stub` (upstream's
fake max ranks). Before you update a server that already has players, read the
[upgrade notes]({{ upgrade_page.url | relative_url }}#real-progression-default).

## Pages

| Page | What it covers |
|:-----|:---------------|
| [Configuration]({{ config_page.url | relative_url }}) | Every environment variable of the metagame, deploy server, gateway, allowlist helper and content server, plus the launcher's overrides: default, allowed values, what it does, who sets it, and which ones are secret. Also what the Windows server kit writes into its `.env` files, which switches are on by default, and the repository variable that pauses automatic launcher releases. |
| [Ports and network]({{ ports_page.url | relative_url }}) | Every TCP and UDP port, which address each component binds in private and public mode, the path a request takes, what must never be exposed, and the firewall rules the kit creates. |
| [HTTP API]({{ api_page.url | relative_url }}) | Every route of the metagame (the game routes and the management API under `/undaunted/api`), the deploy server, the content server, the gateway, the allowlist helper and the launcher's relay: who may call it, the headers it needs, and what it answers. |
| [Files and data]({{ files_page.url | relative_url }}) | The repository layout, a hand-built host's folders, the kit's install root, the SQLite database table by table, logs, backups, and the files on a friend's PC. Which files hold secrets. |
| [Game settings]({{ gamesettings_page.url | relative_url }}) | What changes on the 1.4.4 client and its game servers: the user `Engine.ini`, `Game.ini` and `GameUserSettings.ini` lines and who writes them, the client and game-server command lines, the two DLLs and the pinned build. |
| [Scripts and parameters]({{ scripts_page.url | relative_url }}) | Every script of the Windows server kit, the friend kit and `tools/`, every npm script, and the CI workflows and their tools, with each parameter, its default and examples. |
| [Developer guide]({{ dev_page.url | relative_url }}) | Working on the code: prerequisites, building and testing each package, running the stack and the launcher on one PC, CI and launcher releases, keeping generated files current, and where the code lives. |

## The components

Components are named by their folder in the repository.

| Folder | What it is | Settings | Routes and scripts |
|:-------|:-----------|:---------|:-------|
| `legacy/UndauntedMetagame/` | The backend the game talks to: accounts, characters, inventory, progression, matchmaking, parties, and the management API. | [Metagame]({{ config_page.url | relative_url }}#metagame) | [Game routes]({{ api_page.url | relative_url }}#game-routes), [management API]({{ api_page.url | relative_url }}#undaunted-api) |
| `legacy/UndauntedDeployServer/` | Starts and watches the game servers: Ramsgate, the Training Dojo and one per hunt. | [Deploy server]({{ config_page.url | relative_url }}#deploy-server) | [Deploy server]({{ api_page.url | relative_url }}#deploy-server) |
| `native/internal-server/` | The DLL that turns a copy of the 1.4.4 client into a game server and points clients at the metagame. | [Game settings]({{ gamesettings_page.url | relative_url }}#dlls) | none |
| `legacy/UndauntedGateway/` | Public mode only: the TLS gateway, the one port open to everyone, and the allowlist helper that opens the game ports for players who logged in. | [Gateway]({{ config_page.url | relative_url }}#gateway), [allowlist helper]({{ config_page.url | relative_url }}#allowlist-helper) | [Gateway]({{ api_page.url | relative_url }}#gateway), [allowlist helper]({{ api_page.url | relative_url }}#allowlist-helper) |
| `legacy/UndauntedContent/` | Serves the game files, news and art pack to the launcher. | [Content server]({{ config_page.url | relative_url }}#content-server) | [Content server]({{ api_page.url | relative_url }}#content-server) |
| `legacy/UndauntedLauncher/` | The friend launcher: invites, account key, game download and repair, and the local relay in public mode. | [Launcher]({{ config_page.url | relative_url }}#launcher) | [Launcher relay]({{ api_page.url | relative_url }}#launcher-relay) |
| `deploy/windows-server/` | The Windows server kit: installs, runs, updates and backs up everything on a Windows Server 2019 machine. | [What the kit writes]({{ config_page.url | relative_url }}#server-kit) | [Kit scripts]({{ scripts_page.url | relative_url }}#windows-server-kit) |
| `friend-kit/` | PowerShell scripts for a friend joining a private-mode (Tailscale) server without the launcher. | [Client side]({{ config_page.url | relative_url }}#client-side) | [Friend kit scripts]({{ scripts_page.url | relative_url }}#friend-kit) |

Each server component has a commented `.env.example` in its folder that lists every variable it
reads, with its default. Copy it to `.env` and fill in what you need; the
[Configuration]({{ config_page.url | relative_url }}) page explains each line.

## Conventions on these pages

- **Secrets** are marked as secret: account keys (`UUK_...`), the game-server key, the signing keys,
  the gateway and allowlist secrets, and the TLS key. Never share them and never commit them. The
  examples on these pages are placeholders.
- `C:\DauntlessRevived` is the kit's default install root, written `<root>` where the root can be
  another folder. `C:\dr\...` paths are the original host's layout, which the setup guides use as
  their example; use your own folders.
- Example addresses come from the ranges reserved for documentation (`203.0.113.x`,
  `198.51.100.x`), and `100.x.y.z` stands for a Tailscale address.
- "Upstream" means [Undaunted](https://github.com/SyST3MDeV/Undaunted), the project this fork is based
  on. On the Configuration page, **Fork only** marks settings that upstream does not have.
