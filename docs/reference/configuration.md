---
title: Configuration
parent: Reference
nav_order: 1
description: "Every environment variable of the Dauntless Revived servers: defaults, allowed values, what each does, who sets it, and which switches are on by default."
lang: en
ref: reference/configuration
---

{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}
{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}
{% assign legal_page = site.pages | where: "path", "legal.md" | first %}
{% assign ports_page = site.pages | where: "path", "reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "reference/scripts.md" | first %}
{% assign dev_page = site.pages | where: "path", "reference/development.md" | first %}

# Configuration
{: .no_toc }

Every server component of Dauntless Revived is configured with environment variables. None of them
has a configuration file format of its own: on a development PC each one reads a `.env` file in its
folder, and on a server installed with the Windows server kit each one reads its own file in
`C:\DauntlessRevived\data\config\`. This page lists every variable each component reads, what an
unset value means, who normally sets it, and which ones are secret.

It is a reference. For a working setup step by step, follow [Host a server]({{ host_page.url | relative_url }})
(one PC), [Run it for a group]({{ admin_page.url | relative_url }}) (Tailscale) or the
[Windows server kit]({{ winserver_page.url | relative_url }}) (a rented server). Port numbers are on
[Ports and network]({{ ports_page.url | relative_url }}), the game's own ini files and command lines on
[Game settings]({{ gamesettings_page.url | relative_url }}), and script parameters on
[Scripts and parameters]({{ scripts_page.url | relative_url }}).

**Secrets.** Rows marked **Secret** hold values that let someone sign in as any player, act as a
game server or talk to the firewall helper. Never share them, never commit them, and never paste or
print them. The examples on this page are placeholders.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## How settings are loaded {#how-settings-are-loaded}

| Component | Folder | Started with | Developer file | File on a kit server |
|:----------|:-------|:-------------|:---------------|:---------------------|
| Metagame | `legacy/UndauntedMetagame/` | `npm start` = `node --env-file=.env dist/server.js`; `npm run dev` = `tsx watch --env-file=.env src/server.ts` | `legacy/UndauntedMetagame/.env` (must exist, or Node refuses to start) | `data\config\metagame.env` |
| Deploy server | `legacy/UndauntedDeployServer/` | `npm start` = `node --env-file=.env dist/server.js`; `npm run dev` = `tsx --env-file=.env src/server.ts` | `legacy/UndauntedDeployServer/.env` (must exist) | `data\config\deployserver.env` |
| Gateway | `legacy/UndauntedGateway/` | `npm start` = `node --env-file-if-exists=.env dist/server.js` | `legacy/UndauntedGateway/.env` (optional) | `data\config\gateway.env` (public mode only) |
| Allowlist helper | `legacy/UndauntedGateway/` | `npm run start:allowlist` = `node --env-file-if-exists=.env dist/allowlist/server.js` | the same `legacy/UndauntedGateway/.env` | `data\config\allowlist.env` (public mode only) |
| Content server | `legacy/UndauntedContent/` | `npm start` = `node --env-file-if-exists=.env dist/server.js` (also `npm run verify`) | `legacy/UndauntedContent/.env` (optional) | `data\config\content.env` (when the content server is installed) |
| Launcher | `legacy/UndauntedLauncher/` | the installed app, or `npm start` | none: it reads its own process environment only | not on the server |

Rules that apply to all of them:

- **The shell wins over the file.** Node's `--env-file` never replaces a variable that is already
  set in the environment. A `PORT` or `NODE_ENV` left in your PowerShell session (`$env:PORT`)
  silently overrides the `.env` value. Check with `Get-ChildItem Env:` and remove one with
  `Remove-Item Env:PORT`. `npm run db:generate` in the metagame reads `.env` through `dotenv`, which
  behaves the same way.
- **The kit makes the file win.** `Stack.ps1` starts each component as
  `node --env-file=<root>\data\config\<file>.env <script>`, with the component's folder under
  `<root>\app\` as the working directory. Before that it clears every variable the file names from its
  own process, so nothing inherited can override the file.
- **The working directory matters.** Relative paths (`DB_FILENAME`, `BODY_LOG_FILE`,
  `ALLOWLIST_AUDIT_LOG`, `GATEWAY_CERT` and so on) resolve against it, and the metagame finds its
  database migrations at `./src/drizzle`. Start each component from its own folder.
- **File format.** `KEY=value` lines, `#` for comments. Save the file as ASCII or UTF-8 without a
  byte-order mark. In Windows PowerShell 5.1, write it with `Set-Content -Encoding ascii` or
  `Add-Content -Encoding ascii`; `>` and `>>` write UTF-16, which Node cannot read.
- **Restart after every change.** Each component reads its environment when it starts (some values
  once, some on every request, but the environment of a running process never changes). Change the
  file, then restart that component: by hand as in [Host a server]({{ host_page.url | relative_url }}#stopping),
  or on a kit server with `C:\DauntlessRevived\bin\Stack.ps1 restart -Only <component>` (`metagame`,
  `content`, `deploy`, `gateway` or `allowlist`). Two things change without a restart: an admin can
  switch `REGISTRATION_MODE` at runtime (in memory only, a restart goes back to the file), and the
  content server re-reads the news file and the art pack folder by itself.
- **Unset and empty are not always the same.** In the gateway, the allowlist helper and the content
  server, `KEY=` (or only spaces) counts as unset. In the metagame and the deploy server most values
  are used as they are: `PORT=` listens on a random free port, and `ENTITLEMENTS_DEFAULT=` means no
  default entitlements at all. Leave a line out to get the default.
- **Start from the `.env.example` file.** Each of the four server folders has a commented
  `.env.example` that lists every variable the component reads, with its default, set up for
  development on one PC (everything on `127.0.0.1`, the allowlist helper in dry-run mode). Copy it to
  `.env` and fill in the empty values: the secrets, and the game folder or exe path where one is
  needed. For a host that other people use, follow [Host a server]({{ host_page.url | relative_url }}#metagame)
  instead. A `.env` file is git-ignored anywhere in the repository, and so are `.env.*` files
  (except `.env.example`) in the metagame, deploy server, gateway and content server folders.

---

## Feature switches {#feature-switches}

Each switch below changes one behaviour. The fork's own fixes are on by default and each has a way
back to upstream's behaviour, for comparisons. Changes that are still unproven, risky or only for
development are off.

**On by default:**

| Switch | Component | Default | To opt out | Notes |
|:-------|:----------|:--------|:-----------|:------|
| `PROGRESSION_MODE` | metagame | real progression for every account | `PROGRESSION_MODE=stub` (upstream's fake max ranks) | The default changed: a server whose settings have no line used to be on the stub. Read the [upgrade notes]({{ upgrade_page.url | relative_url }}#real-progression-default) before updating a server that already has players. |
| `ENTITLEMENTS_DEFAULT` | metagame | every account owns the Elite Hunt Pass | set your own list | Only for accounts in real progression. |
| `INVENTORY_REPORT_REMOVALS` | metagame | on | `0` | `0` brings back upstream's free-upgrade bug. |
| `MISC_ROUTES` | metagame | on | `0` | Friends list, block list and a few other routes answer instead of 404. |
| `STATUS_EXTRA` | metagame | on | `0` | Server name, version, commit and source link in `/dauntless-status`. |
| `ACCOUNT_DISPLAY_NAME` | metagame | on | `0` | Real usernames in account and party replies. |
| `ACCOUNT_MAPPING` | metagame | on | `0` | Account mappings for Add Friends and name-based invites. `0` maps nothing, as before. |
| `GUILDS` | metagame | on | `0` | The guild routes. `0` brings back the old stubs: nobody can create or join a guild. |
| `GUILD_RESERVED_NAMES` | metagame | on | `0` | Staff and project words refused in guild names and nameplates. |
| `PARTY_SOLO_STUB` | metagame | on (upstream's placeholder for a party of one) | `0` | `0` if Invite to Party does nothing for a player on their own. |
| `CHAT_NICK_CHECK` | metagame | `enforce`: a chat room join whose nickname fails the name rules is refused | `log` (admit and warn; another account's id is still refused) | A rollback switch only, in case a real client is refused. Needs `CHAT=1` to matter. |
| `PROGRESSION_CONFIRM` | metagame | on | `off` | Diagnostic only. |
| `LOG_REQUESTS` | metagame | on | `0` | The request log is the main diagnostic; keep it. |
| `SLAYER_LINKS` | metagame | on | `0` | Slayer Links. `0` puts back the 404 every `/slayerlink` route got before; the stored invites and links stay. |
| `BALANCE_FROM_INVENTORY` | metagame | on | `0` | `/balance` and `/reconcile` report the currencies the character holds. `0` puts back the fixed sheet (Notes from the database, the rest 0). |
| `PROGRESSION_REPLAY_WINDOW_S` | metagame | `5` (seconds) | `0` | A game server's grant repeated byte for byte less than 5 seconds later is answered, not added again. |
| `PERSISTENT_WORLD_LIVENESS` | deploy server | on | `0` (only the watchdog restarts Ramsgate; the Dojo starts on demand) | A dead Ramsgate or Dojo is started again when a player travels there. |
| `GATEWAY_ALLOWLIST` | gateway | on | `0` | Kill switch: with it off, nobody's game ports open. |
| `ENABLE_DOJO` | deploy server | Dojo starts on first use | `1` starts it at boot, as upstream did | Saves one game process while nobody trains. |

**Off by default:**

| Switch | Component | To turn it on | Why it is off |
|:-------|:----------|:--------------|:--------------|
| `CHAT` | metagame | `1` | In-game text chat. Built and tested without the game, not yet tried by two players; it goes on by default after that live test. |
| `ESCALATION_MODE` | metagame | `real` | Real Escalation saves. Switching it on drops every player from the fake maximum (level 25) to level 0: the owner's decision after the in-game test. |
| `ESCALATION_STRICT` | metagame | `1` | Makes the Escalation rules that depend on our modelling refuse a save instead of warning. After the logs stay clean. |
| `STORE` | metagame | `free` | The free in-game store. Waits for the decision free or priced (roadmap 3.7) and an in-game store test. |
| `STORE_REPEATABLE_TOKENS` | metagame | `1` | Sells the bounty-token bundle any number of times: unlimited free premium bounty drafts. The owner's decision. |
| `PROGRESSION_CONFIRM_ENTITLEMENTS` | metagame | `1` | A rank confirm also grants the rank's permanent entitlements. Only if the in-game test shows the game server never grants them itself. |
| `VERIFY_STUB_ACCOUNT` | metagame | `1` | A rollback only: the old placeholder account in `oauth/verify`. |
| `MATCHMAKING_CANCEL` | metagame | `1` | Experimental. The client sends a cancel right after every queued join, and hunts start only because that cancel gets a 404. |
| `ACCOUNTINFO_PUBLIC_LEGACY` | metagame | `1` | A rollback only: upstream's account info reply, which hides other players (party invites never show). |
| `GUILD_CREATE_ACTIVITY_FALLBACK` | metagame | `1` | Weaker check on guild creates, only if the live test shows the client never validates the final name. |
| `INVENTORY_REFUSE_OVERSPEND` | metagame | `1` | Unproven. A refusal drops the whole transaction, rewards included, and no hunt-end transaction has been checked against it yet. |
| `PROGRESSION_ALLOW_DELETE` | metagame | `1` | Lets any game-server call wipe a player's progression track. |
| `DB_WAL` | metagame | `1` | The documented backups copy the database file alone. |
| `LOG_BODIES` | metagame | `1` | Development capture of what players send. The kit forces it off in public mode. |
| `AUTH_MODE=NONE` | metagame | `NONE` | Development only: anyone can log in as anyone. Ignored with `NODE_ENV=production`. |
| `REGISTRATION_MODE=OPEN` | metagame | `OPEN` | Anyone who can reach the metagame gets an account. Only while it listens on loopback. |
| `CONTENT_ALLOW_ANY_BIND` | content server | `1` | Lets the content server listen on a public address. |
| `ALLOWLIST_ALLOW_PRIVATE` | allowlist helper | `1` | Tests and LAN setups only; the kit sets it in sandbox installs. |

`ALLOWLIST_DRY_RUN` has no default: the allowlist helper refuses to start until it is `1` (log what
it would do; any test machine) or `0` (change the firewall; the server).

---

## Metagame (`legacy/UndauntedMetagame/`) {#metagame}

The backend the game talks to: logins, characters, inventory, progression, matchmaking and the
`/undaunted/api` management routes. A worked `.env` for one PC is in
[Host a server]({{ host_page.url | relative_url }}#metagame).

**Required, no default:** `PORT`, `AUTH_MODE`, `AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64`,
`DB_FILENAME`, `REGISTRATION_MODE`, `MATCHMAKING_MODE`, `DEPLOYSERVER_URL`, `QOS_TARGET_URL` and
`TARGET_CHANGELIST`. A missing `PORT` or signing key stops the metagame at startup. The others do
not: the metagame starts and then fails in the ways the tables describe.

**When read:** at startup. Restart the metagame after any change. A restart does not log anyone
out (session tokens survive it), but it drops matchmaking queues, parties and the list of who is
online. Guilds, friendships and blocks are stored and survive it.

**At startup** the metagame logs the address it listens on and a line
`Progression mode: real for every account (the default)` (or which mode it is in). In real mode it
also warns while there are players with no stored progression yet; see the
[upgrade notes]({{ upgrade_page.url | relative_url }}#real-progression-default). With
`GATEWAY_SECRET` set it runs the public-mode checks listed under that variable.

After that come two lines for the settings added with the port of Harmonic's fork:

```text
features: bodyLogPerPath=no-cap escalation=stub escalationStrict=off store=off storeRepeatableTokens=off replayWindow=5s confirmEntitlements=off balanceFromInventory=on slayerLinks=on chatPresence=on verifyStubAccount=off
Progression config: bundled, 10 tracks; active Hunt Pass season09b
```

Those switches (`BODY_LOG_PER_PATH`, `ESCALATION_MODE`, `ESCALATION_STRICT`, `STORE`,
`STORE_REPEATABLE_TOKENS`, `PROGRESSION_REPLAY_WINDOW_S`, `PROGRESSION_CONFIRM_ENTITLEMENTS`,
`BALANCE_FROM_INVENTORY`, `SLAYER_LINKS`, `CHAT_PRESENCE` and `VERIFY_STUB_ACCOUNT`) share one parser:
on and off take `1`, `true`, `on` or `yes` and `0`, `false`, `off` or `no`, in any case; counts are
whole numbers; unset or empty is the default. A value that does not parse logs one warning,
`<NAME>="<value>" is not a valid value; using the default (<default>)`, and the default is used. A bad
`PROGRESSION_CONFIG_DIR` or `ACTIVE_HUNT_PASS` stops the metagame instead (`The progression config
could not be loaded: <reason>`, exit code 1), before the database is opened.

### Listening, logins and the database {#metagame-core}

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `PORT` | none. Unset: the metagame stops at startup with a port error. Empty: it listens on a random free port. | 1-65535 | TCP port of the HTTP server. A port that is already taken stops the metagame with `Could not listen on ...`. | You (61000 in the host guide); kit: always (61000, 62000 in sandbox, or `-MetagamePort`) |
| `BIND_HOST` | `127.0.0.1` (also when empty) | one IP address | **Fork only.** The one address the metagame listens on. `127.0.0.1` on one PC and in public mode (behind the gateway); the host's Tailscale address (100.x.y.z) in private mode. Avoid `0.0.0.0`: the metagame would also listen on your LAN. | You; kit: always (`127.0.0.1`, or the Tailscale address in private mode) |
| `NODE_ENV` | unset: development | `production` or anything else | `production`: one JSON object per log line, `AUTH_MODE=NONE` has no effect, and error pages carry no stack traces. Anything else: coloured readable logs. Use `production` on anything other people can reach. | You; kit: always `production` |
| `AUTH_MODE` | none. Unset or unknown: logins get no answer at all (the game waits), and every route that checks an account key answers 500. | `APIKEY` or `NONE` (exact) | How logins and account keys are checked. `APIKEY`: the game's login carries the player's account key (`UUK_...`), checked against stored SHA-256 hashes; the player gets a 24-hour session token. `NONE`: development only. Any id is accepted as a login and any account id as an API key, admin routes included. `NONE` is ignored when `NODE_ENV=production` (logins then get no answer), refused for anything that came through a proxy, and the metagame will not start with it while `GATEWAY_SECRET` is set. | You (`APIKEY`); kit: always `APIKEY` |
| `AUTH_SIGNING_PRIVKEY_B64` | none. Unset: the metagame stops at startup. Empty: it starts, but every login fails. | base64 of a PEM RSA private key (PKCS#8) | **Secret: never share, never commit.** Signs the players' session tokens (RS256, valid 24 hours). Whoever has it can sign in as any account. Replacing the key pair only logs everyone out. Back it up encrypted. | You ([Host a server]({{ host_page.url | relative_url }}#metagame) shows how to generate it); kit: kept from the existing file or the backup, otherwise generated once |
| `AUTH_SIGNING_PUBKEY_B64` | none. Unset: the metagame stops at startup. | base64 of the matching PEM public key (SPKI) | Checks every session token. Not secret, but it must belong to the private key, or every signed-in request fails. | Generated together with the private key |
| `REGISTRATION_MODE` | none. Unset or unknown: account creation answers 500. | `NONE`, `INVITECODE` or `OPEN` (exact) | Who may create an account (`POST /undaunted/api/Register`). `NONE`: nobody (400). `INVITECODE`: only with a usable invite code (401 otherwise). `OPEN`: anyone who can reach the metagame, so use it only while the metagame listens on `127.0.0.1`. An admin can change it at runtime; that lasts until the next restart. | You (`OPEN` on one PC); kit: always `INVITECODE` (a hand-set value is overwritten on every installer run) |
| `DB_FILENAME` | none. Unset or empty: a temporary database that is deleted when the metagame exits. | file path, forward slashes; relative to the working directory | The SQLite database: accounts, key hashes, saves, progression. Its folder must exist. Pending migrations run at every start, without a backup first, so back up before you update. The file is private: it holds usernames, saves, key hashes and, until the next start, keys queued for registration in plain text. `npm run db:generate` reads it from `.env` too. | You (`C:/dr/data/undaunted.db` in the host guide); kit: always (`<root>/data/undaunted.db`) |
| `DB_WAL` | off | `1` or anything else | `1` switches SQLite to WAL journal mode. Off because the documented backups copy the database file alone and the kit stops the metagame hard, so the newest saves could sit only in the separate `-wal` file. With it off, a database left in WAL mode is switched back at startup. | Nobody by default; kit: kept |

### Matchmaking and the client {#metagame-matchmaking}

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `MATCHMAKING_MODE` | none. Unset or unknown: every matchmaking request is refused (400). | `DEPLOYSERVER` or `DISABLED` (exact) | `DEPLOYSERVER`: game servers come from the deploy server. `DISABLED`: refuse all matchmaking. | You; kit: always `DEPLOYSERVER` |
| `DEPLOYSERVER_URL` | none. Unset: every launch fails, and the server status lists no worlds. | `host:port` without `http://` | Where the deploy server listens; the metagame adds `http://` and the path. Keep it on `127.0.0.1`: the deploy server has no authentication and refuses every other caller. | You (`127.0.0.1:61001`); kit: always `127.0.0.1:<deploy port>` |
| `MATCHMAKING_DEPLOY_TIMEOUT_MS` | `240000` (4 minutes) | integer milliseconds, 1000-900000; invalid settings warn and use the default | Bounds the whole allocation request, including an unfinished JSON body. A timeout ends the candidate with `FAILED`; it never silently starts another server. A new explicit join may retry. Keep the deadline above the worker startup grace and configured launch spacing. | Optional; kit: kept |
| `QOS_TARGET_URL` | none. Unset: the region list the client pings holds only `null` (what the client does then is untested). | full URL ending in `/QoS` | The one "region" the client pings before matchmaking; the metagame answers `GET /QoS` itself. One PC: `http://127.0.0.1:61000/QoS`. Private mode: the same with the Tailscale address. Public mode: `http://127.0.0.1:61000/QoS`, which is each friend's own launcher relay. | You; kit: always (public: `http://127.0.0.1:61000/QoS`; private: `http://<Tailscale address>:<metagame port>/QoS`) |
| `TARGET_CHANGELIST` | none. Unset: the build id becomes `undefined_1.4.4_shipping`. | `239827` | Sent to the client as the build id `<changelist>_1.4.4_shipping` in the reply that sends it to a game server. Whether the 1.4.4 client checks it is not known; keep `239827`, the changelist in `Version.txt`. | You; kit: always `239827` |

### Public mode and game-server callers {#metagame-public-mode}

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `GATEWAY_SECRET` | unset or empty: public mode off, forwarding headers are never trusted | 32 to 256 printable characters without spaces (the gateway enforces this; the kit makes 64 hex characters) | **Secret: never share, never commit.** Turns on public mode. Only a request from loopback whose `X-Dauntless-Gateway` header equals this value counts as relayed by the gateway, and only then is its `X-Forwarded-For` taken as the player's address. Must equal `GATEWAY_SECRET` in the gateway's settings. At startup, `AUTH_MODE` other than `APIKEY` stops the metagame; a secret shorter than 16 characters, a `BIND_HOST` that is not loopback, `NODE_ENV` other than `production`, or a `QOS_TARGET_URL` other than `http://127.0.0.1:<port>/QoS` log a warning. | Kit: public mode only (the same value as in `gateway.env`, kept across re-runs, removed in private mode) |
| `GAMESERVER_ALLOW_FROM` | none | comma-separated IP addresses; other entries are ignored | Extra addresses allowed to present the game-server key, for a game server on another machine. By default the key is accepted only from this machine (loopback or one of its own addresses) and never through a proxy. List only machines you control; they still need the key. | Nobody by default; kit: kept |

### Logging {#metagame-logging}

`BACKEND_HEALTH=1` enables optional aggregate request metrics and a 20 ms event-loop-delay monitor.
Default off; restart the metagame after changing it. The owner-only `/undaunted/api/BackendHealth`
route returns 404 when disabled. Memory is bounded to 60 one-second request buckets; no request
URLs, bodies or account identifiers are retained. See `tools/DASHBOARD.md` for dashboard setup.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `LOG_LEVEL` | `info` (also when empty) | `fatal`, `error`, `warn`, `info`, `debug`, `trace`, `silent` | Log threshold. | Nobody by default |
| `LOG_REQUESTS` | on | `0` or anything else | **Fork only.** One line per request: `METHOD /path gs=0` (`gs=1` when the request carried the game-server key). Behind the gateway the line ends with ` via=gateway ip=<player address>`; other forwarded requests get ` via=proxy peer=<address>`. Only the path is logged, never the query string or headers, and token-shaped parts of the path are replaced. `0` turns it off. | Nobody by default |
| `LOG_BODIES` | off | `1` or anything else | **Fork only.** `1` appends the request bodies of unfinished routes (progression, Hunt Pass, bounties, cooldowns, Escalation, entitlements, loadout unlocks, the store (`/product`, `/token`, `/notification`), balance and `/reconcile`, Slayer Links, matchmaking, party, friends, inventory and account lookups) to `BODY_LOG_FILE`: one JSON object per line, `{t, method, url, gs, body, status, ms}` with the arrival time, method, URL (query string included), whether the game-server key was present, the body cut at 8 KB (64 KB for inventory), the reply's status and the time to answer in milliseconds (`"aborted": true` when the connection closed first). The line is written when the answer is done. Tokens, account keys (`UUK_...`) and other long token-shaped strings are removed from the URL and the body. A development aid: the file records what players send, so keep it private and never turn it on for a public server. | Nobody by default; kit: forced to `0` in public mode, kept in private mode |
| `BODY_LOG_PER_PATH` | `0`: no cap | whole number | **Fork only.** With `LOG_BODIES=1`: at most this many lines per method and exact path (ids included) in one run of the metagame; the path's last line is followed by `body log: <METHOD path> reached BODY_LOG_PER_PATH=N; no more lines for it in this run`. For a client stuck in a loop. | Nobody by default |
| `BODY_LOG_FILE` | `bodies.log` in the working directory | file path | Where `LOG_BODIES` writes. The file is never rotated. `legacy/UndauntedMetagame/bodies.log` is not git-ignored: point this outside the repository (for example `C:/dr/data/bodies.log`) and never commit the file. | Kit: always (`<root>/data/logs/bodies.log`) |

### Server identity {#metagame-identity}

These values are **public**: anyone can read them from `GET /dauntless-status`, and registered
players from `GET /undaunted/api/ServerStatus` (see [HTTP API]({{ api_page.url | relative_url }})).

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `SERVER_NAME` | `Dauntless Revived` | printable ASCII, cut to 64 characters | The server's name in the status replies, and in the in-game welcome text of `/dauntless-status` (`Welcome to <name>!`, translated into each of the eight languages the reply carries). | Kit: always, from `-ServerName` (default `Dauntless Revived`). The installer does not remember it: pass `-ServerName` on every run, or the name goes back to the default. |
| `SERVER_VERSION` | the version recorded at build time (`dist/build-info.json`), then `package.json`, then `unknown` | printable ASCII, up to 32 characters | Version in the status replies. | Nobody by default |
| `GIT_COMMIT` | the commit recorded at build time (with `-dirty` if the metagame folder had uncommitted changes), then `unknown` | up to 64 characters of letters, digits and `._+-`; anything else is ignored | The commit the running code came from. | Kit: always (the installed commit); `Update-DauntlessServer.ps1` rewrites it on every update and rollback |
| `SOURCE_URL` | `https://github.com/mixutin/dauntless-revived` | URL, printable ASCII, up to 256 characters | Where the source of the running server is, for the AGPL source offer ([Credits and license]({{ legal_page.url | relative_url }})). If you run modified code for other people, point it at your modified source. | Kit: always the repository URL from `deploy/windows-server/DauntlessServer.Common.ps1`, so a hand edit is overwritten on the next installer run; a fork changes it there |
| `CONTENT_PORT` | unset or invalid: none reported | 1-65535 | Tells launchers which port the content server (game downloads, news, art) uses. | Kit: always when the content server is installed (61002), removed otherwise |
| `STATUS_EXTRA` | on | `0` or anything else | **Fork only.** `GET /dauntless-status` adds `name`, `version`, `commit` and `sourceUrl` after the nine fields the game reads. `0` answers the nine fields only, as upstream did. The route answers anyone and never names players. | Nobody by default |

### Progression {#metagame-progression}

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `PROGRESSION_MODE` | real for every account (unset, empty or `real`) | `real` or `stub`, any case; any other value (such as `off`, `0` or `false`, which used to mean the stub) logs a warning, counts as `real`, and the startup line then reads `real for every account (PROGRESSION_MODE=<value> is not recognised)` | **Fork only.** Real: every account stores its own Slayer level, weapon and behemoth mastery, objectives, Hunt Pass, entitlements, loadout slots, cooldowns and bounties. `stub`: upstream's fake max ranks for everyone, nothing stored. **Changed default:** before, an unset value meant the stub. Nothing is migrated; read the [upgrade notes]({{ upgrade_page.url | relative_url }}#real-progression-default). | Nobody by default; kit: never written, kept |
| `PROGRESSION_REAL_ACCOUNTS` | none | comma-separated account ids (`UID-...`) | Only with `PROGRESSION_MODE=stub`: these accounts get real progression anyway. Ignored in real mode (the startup line says so when it is set). | Nobody by default; kit: kept |
| `PROGRESSION_GRANT_CAP` | `5000` (also when not a positive whole number) | positive whole number | The most XP one request may add to one track. More is cut to the cap and noted in the progression audit table. | Nobody by default |
| `PROGRESSION_CONFIRM` | on | `off` or anything else | `off` makes the rank-confirm route answer 404 again for real-mode accounts, as upstream did. A diagnostic switch; leave it unset. | Nobody by default |
| `PROGRESSION_ALLOW_DELETE` | off | `1` or anything else | `1` lets game servers reset a progression track (the game sends that only from a debug command). Without it only an admin key can. With it on, any game-server call can wipe a player's track. | Nobody by default |
| `ENTITLEMENTS_DEFAULT` | `season09b_premium,season_premium_any,season_free_any` when unset | comma-separated entitlement names; empty means none | Entitlements every real-mode account owns. `season09b_premium` is the Elite Hunt Pass. Each default is added to an account once, the first time its entitlements are read; removing a name later does not take it back, and a default an admin revoked stays revoked. | Nobody by default |
| `PROGRESSION_REPLAY_WINDOW_S` | `5` | whole seconds; `0` turns it off | **Fork only.** The retry guard of `POST /progression/<account>`: a grant whose body is byte for byte the account's last grant, less than this many seconds after it and with no other track write in between, gets the first grant's stored reply and adds nothing (the log says `repeats the grant of N s ago`; an audit row notes it). The game server retries a failed request up to 5 times. Keep the value under 10: the game server sends its queued grants at most once every 10 seconds, and two of those can be identical. If the line shows up in normal play, not right after a network error, set `0`. | Nobody by default |
| `PROGRESSION_CONFIRM_ENTITLEMENTS` | off | on/off | **Fork only.** On: a Hunt Pass or mastery confirm that raises a rank also grants that rank's **permanent** entitlements from the progression config (for season09b: premium ranks 6, 9, 29 and 50 and free rank 50), with the source `confirm:<track>:<rank>`. Never items, currencies or timed entitlements (those are only noted). Off, because whether the game server grants them itself is not known yet (the in-game test: reach Elite rank 6 by hunt XP and watch for `POST /entitlementv2`). The confirm reply is the same either way. | Nobody by default |
| `PROGRESSION_CONFIG_DIR` | unset: the bundled `vendor/progression_config.json` | a folder path | **Fork only.** A folder of `.json` files, each a progression path (a Hunt Pass season or a mastery track), a list of paths, or a whole config. A path replaces the bundled one with the same `progression_id`, a new id is added. Checked at startup; a missing folder, a folder without `.json` files, invalid JSON, a path without `progression_id` or `requirements`, ranks out of order, or an id twice stop the metagame. Read once; a change needs a restart. The formats are on [Game settings]({{ gamesettings_page.url | relative_url }}#hunt-pass-seasons). Never edit a season in place while players have progress in it. | Nobody by default |
| `ACTIVE_HUNT_PASS` | `season09b` | a loaded track id | **Fork only.** The Hunt Pass an account gets when none is stored (`GET /huntpass/<account>`, and the rank math's default). An id that is not a loaded track stops the metagame at startup. | Nobody by default |
| `BALANCE_FROM_INVENTORY` | on | on/off | **Fork only.** On: `GET /balance` and `POST /reconcile` report, for each `CURRENCY_*` key (and its `id_currency_*` twin), the quantity of that stack held by the account's active character (the one saved last), and keep the old values for currencies it does not hold. Off: the old fixed sheet (Notes from the `users` table, weapon tokens 25, the rest 0). | Nobody by default |

### Escalation {#metagame-escalation}

All **fork only**, and read on every request. How Escalation is saved, and what players see, is on
[Escalation]({{ '/findings/escalation.html' | relative_url }}).

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `ESCALATION_MODE` | `stub` | `stub` or `real`, any case; anything else is a warning and `stub` | `stub`: every account reads upstream's fake maximum (`escalation_level` 99999, which the game shows as the last level) and the save route answers 404, as before. `real`: accounts in real progression mode read and save their own seasons, under the save rules; stub-mode accounts keep the stub. **Switching to `real` drops every player from the fake maximum to level 0.** Restart the game servers with the metagame when you switch. | Nobody by default |
| `ESCALATION_STRICT` | off | on/off | Only with `real`. Off: a save that breaks a soft rule (the XP below the next level's cost, the points spent within the level, the talent tier gates, rewards collected at their level) is stored, with a warning line and a note in the audit row. On: such a save is refused with 409. The hard rules always apply. | Nobody by default |

### Store {#metagame-store}

All **fork only**, and read on every request. How the store works is on
[The in-game store]({{ '/findings/store.html' | relative_url }}).

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `STORE` | `off` | `off` or `free`, any case; anything else is a warning and `off` | `off`: the store screen gets the old 400 and the purchase routes 404, as before. `free`: the catalogue of free offers in `vendor/store_catalog.json`, the purchase token and the confirm; items go to the account's active character. Before turning it on for real players, count the characters per account (the purchase goes to the character saved last). | Nobody by default |
| `STORE_REPEATABLE_TOKENS` | off | on/off | Only with `free`. On: the bundle of 20 premium bounty tokens (`bundle_currency_bounty_small`) is listed and can be bought any number of times, which means unlimited free premium bounty drafts. Off: it is not listed and answers 404, and a token issued while it was on is refused (409). | Nobody by default |

### Saves and inventory {#metagame-saves}

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `SAVE_HISTORY_KEEP` | `100` (also when below 2 or not a whole number) | whole number, 2 or more | How many of the newest versions of each character's data, and separately of its loadouts, are always kept for rollbacks. 100 is roughly the last 50 minutes of play. | Nobody by default |
| `SAVE_HISTORY_HOURLY` | `48` (also when empty, negative or not a whole number) | whole number of hours, 0 or more (0 turns this tier off) | Beyond those, the last version of each hour is kept for this many hours. | Nobody by default |
| `SAVE_HISTORY_DAILY` | `30` (also when empty, negative or not a whole number) | whole number of days, 0 or more (0 turns this tier off) | And the last version of each day for this many days. At the defaults that is at most about 3.5 MB per character. | Nobody by default |
| `INVENTORY_REFUSE_OVERSPEND` | off | `1` or anything else | `1` refuses (409) an inventory transaction that removes more than the player has. Off because a refusal drops the whole transaction, rewards included; meanwhile an overspend is clamped at 0 and logged. | Nobody by default; kit: kept |
| `INVENTORY_REPORT_REMOVALS` | on | `0` or anything else | **Fork only.** Inventory replies list every stack the transaction touched with its final count (0 for a used-up stack), so the game server sees what was spent. `0` puts back upstream's additions-only reply, which made upgrades free. | Nobody by default |
| `PATROL_BONUS_DAILY` | `10` (also when empty or not a whole number of at least 1) | whole number, 1 or more | **Fork only.** The daily patrol bonus refill (the game's own is 6) lands the `TOKEN_DAILY_PATROL_BONUS` stack on this many. | Nobody by default |
| `BOUNTY_TOKENS_DAILY` | `6` (also when empty or not a whole number of at least 1) | whole number, 1 or more | **Fork only.** Bounty tokens a player starts with and gets each day; the grant lands the `TOKEN_BOUNTY_DRAFT` stack on this many. Existing characters are set to both amounts once, on the first start of this version. | Nobody by default |

### Friends, parties and guilds {#metagame-social}

All **fork only**. The switches are read on every request, so a restart with the new value takes
effect at once. [Friends, parties and guilds]({{ '/findings/social.html' | relative_url }}) explains what
each reply does in the game.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `ACCOUNT_MAPPING` | on | `0` or anything else | `POST /account/mapping` maps this server's accounts in the shape the client reads, which Add Friends, name-based invites and the friends list need. `0` maps nothing, the old effective behaviour: Add Friends then does nothing in game. | Nobody by default |
| `ACCOUNTINFO_PUBLIC_LEGACY` | off | `1` or anything else | `1` puts back upstream's `POST /accountinfo/public` reply, which describes the caller instead of the asked player. With it, party invites, Hunt Members and friends do not show in game. A rollback in case the new reply disturbs logins. | Nobody by default |
| `GUILDS` | on | `0` or anything else | The guild routes. `0` brings back the old stubs (no guild, no invites) and makes every other guild route answer 404; the stored guilds are kept and come back with the switch. | Nobody by default |
| `GUILD_MAX_MEMBERS` | `100` (also when not a whole number from 1 to 10000) | whole number | Members per guild. The client shows this number and the open positions left. | Nobody by default |
| `GUILD_INVITE_TTL_DAYS` | `7` (also when not a number above 0 and up to 365) | days | How long a guild invite stays open. | Nobody by default |
| `GUILD_NAME_DENYLIST` | empty | comma-separated words | Words refused in guild names and nameplates, on top of a short built-in list. Matched anywhere in the name, regardless of case, after undoing digit swaps such as `0` for `o`. | Nobody by default |
| `GUILD_RESERVED_NAMES` | on | `0` or anything else | Staff and project words (such as `admin`, `moderator`, `official`, and the nameplates `GM`, `DEV`, `MOD`) answer "already in use", so no guild can pose as the server's staff. `0` allows them, for example to create an official guild; the short offensive tags stay refused. The lists are on [HTTP API]({{ api_page.url | relative_url }}#guilds). | Nobody by default |
| `GUILD_CREATE_ACTIVITY_FALLBACK` | off | `1` or anything else | A guild create from the game server is accepted only when the leader validated that exact name and nameplate with their own token in the last 15 minutes. `1` also accepts a leader who validated another name or was heard from in the last minute, with a warning in the log. Only for the case where the live test shows the client never validates the final name (the refusal is logged as "no validate of this name"); with it on, a modified client could name another online player as a guild's leader. | Nobody by default |
| `PARTY_SOLO_STUB` | on | `0` or anything else | A player alone in their party gets upstream's placeholder candidate (`QUEUED_FOR_START`), which is proven harmless for queueing hunts. The client refuses to send a party invite while it thinks its party is matchmaking; if Invite to Party does nothing for a player on their own (no `party: invite` line in the log), `0` answers a party of one with no candidate instead. | Nobody by default |
| `SLAYER_LINKS` | on | on/off | Slayer Links: the eight `/slayerlink` routes ([what they do]({{ '/findings/social.html' | relative_url }}#slayer-links)). Off: every one answers 404, as before; the stored invites and links stay and come back with the switch. | Nobody by default |
| `VERIFY_STUB_ACCOUNT` | off | on/off | `GET /account/api/oauth/verify` (the client's regular session check) names the account of the token it came with, and answers the old placeholder with 200 for a missing or bad token. On: the old placeholder `account_id` for everyone, a rollback in case the new reply causes logouts or reconnect loops. | Nobody by default |

### Chat {#metagame-chat}

All **fork only**. The game's text chat (roadmap 3.10): a listener inside the metagame for the game's
XMPP connection, on loopback. In public mode the gateway forwards the game's chat connection to it
(`GATEWAY_WS_URL`, the same port); on one PC the game connects to it straight. How it works is on
[Text chat]({{ '/findings/chat.html' | relative_url }}), the messages it answers on
[HTTP API]({{ api_page.url | relative_url }}#chat). Read at startup. A bad value or a port in use logs
one `chat: not started ...` error line, and the metagame runs on without chat.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `CHAT` | on | unset/empty or `1` is on; `0` is off; other values are off with a warning | Starts loopback in-game chat. Bind/startup/socket failures are logged without message text or tokens; the HTTP backend stays up. Replaces the ignored `EXPERIMENTAL_CHAT`. Existing `CHAT=0` is preserved on update: run `Set-Chat.ps1 -On` once when the server is empty. | Kit: public installs use `-Chat`, else saved `Chat`, else On. Private kit installs explicitly set `0`. |
| `CHAT_PORT` | `61099` | 1-65535 | The listener's port. Must be the port in the gateway's `GATEWAY_WS_URL`. | Kit: always in public mode (61099; 62099 in sandbox) |
| `CHAT_BIND_HOST` | `127.0.0.1` | `127.0.0.1` or `::1`; with `GATEWAY_SECRET` set (public mode) only `127.0.0.1` | The listener's address. Anything else, `0.0.0.0` included, keeps chat off. Private mode (Tailscale) is not supported yet. | Kit: always `127.0.0.1` in public mode |
| `CHAT_NICK_CHECK` | `enforce` | `enforce` or `log`; anything else counts as `enforce` with a warning | `enforce` refuses a room join whose nickname is not `<the account's username>:<its account id>:<its resource>` ([why]({{ '/findings/chat.html' | relative_url }}#nickname-check)). `log` admits a nickname that fails the resource, format or name rule, with one warning line per connection and room: a rollback switch in case the live test shows a real client being refused. A nickname with another account's id is refused in both modes (a real client never builds one). | Nobody by default |
| `CHAT_TRACE` | off | `1` or anything else | `1` logs every chat frame in and out, cut at 2 KB, with the login, passwords, message text (`[N chars]`) and tokens replaced. For the first live runs only. | Nobody by default |
| `CHAT_PRESENCE` | on | on/off (`1`, `true`, `on`, `yes` / `0`, `false`, `off`, `no`) | Friends' online status ([how]({{ '/findings/chat.html' | relative_url }}#presence)): each player's own presence is relayed to their online, accepted, unblocked friends, never back to the player's own account, and an HTTP friend accept is pushed over chat. Needs `CHAT=1`; without it the metagame logs `chat: CHAT_PRESENCE is on but chat is off (CHAT=1 is needed); nobody shows as online` and nothing else happens. Read when the chat server starts. Off: not one presence stanza outside rooms. On by default; set `CHAT_PRESENCE=0` to roll back. Live two-player party/hunt validation is still required. | Automatic; no player setting needed |

**Limits** (fixed in the code):

| Limit | Value | Over it |
|:------|:------|:--------|
| WebSocket frame | 32 KiB | That connection is closed (1009) |
| Connections in total | 64 | New ones get HTTP 503 |
| Frames before login | 4 (the game needs `<open>`, `<auth>` and, after a refusal, its legacy login); one login attempt | The connection is closed |
| Unfinished logins per address (connections that have not bound yet) | 8 | HTTP 503 |
| Failed logins per address | 10 in 10 minutes | Logins from that address get `temporary-auth-failure` for 10 minutes |
| Connections per account | 2 bound, 3 in all | A third bound one replaces the one silent longest; a login past 3 closes the oldest one that has not bound |
| Time to bind | 10 s from the login | The connection is closed; three in 10 minutes hold the account's logins back for 60 s |
| Rooms per connection | 8 | The join is refused |
| Players per room | 128 | The join is refused |
| Rooms in total | 1000 | The join is refused |
| Joins per connection (refused ones included) | 10 at once, then 1 every 6 s | The join is refused |
| Room lines and whispers per connection | 8 at once, then 1 per second | The line gets an error; a whisper is dropped |
| Messages of any kind per connection | 60 at once, then 30 per second | Dropped; more than 100 drops in a minute ends the connection |
| Message text | 1-2048 characters | The line gets an error; a whisper is dropped |
| Room nickname | 1-1023 bytes | The join is refused |
| Unsent output per connection | 256 KiB | Nothing more is sent; the connection ends (`reason=backlog`) |

A connection is pinged after 50 s of silence and ended when another 100 s pass without an answer:
the game answers only from its own game-thread tick, which a long map load holds up
([why]({{ '/findings/chat.html' | relative_url }}#limits)). A replacement, abuse, an oversized frame
or unsent output past the limit holds that account's next login back for 60 s; a ping timeout does
not. A refused join counts toward the drop limit and is logged once per connection, room kind and
reason every 10 minutes. Message text, tokens, login payloads and request headers are never logged.

### Compatibility switches {#metagame-compatibility}

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `MISC_ROUTES` | on | `0` or anything else | **Fork only.** Answers routes that were 404 upstream: the friends list and block list, recent players, friend settings, `POST /candidate/player/alive` and `GET /motd/trigger`. `0` puts the 404s back. | Nobody by default |
| `ACCOUNT_DISPLAY_NAME` | on | `0` or anything else | **Fork only.** Account and party replies carry the real username as `displayName`. `0` puts back upstream's empty `{}`. | Nobody by default |
| `MATCHMAKING_CANCEL` | off | `1` or anything else | **Fork only, experimental.** `1` answers the client's matchmaking cancel (`DELETE /candidate`, and `DELETE /candidate/leave`). Off because the client sends that cancel right after every queued join, and hunts start only because it gets a 404. | Nobody by default |

---

## Deploy server (`legacy/UndauntedDeployServer/`) {#deploy-server}

### Memory admission

Before starting any new game process, the deploy server requires free physical RAM for both
`GAMESERVER_MIN_FREE_MB` (default 3072 MiB of headroom) and `GAMESERVER_STARTUP_MB` (default
1536 MiB estimated startup memory). Each successful admission reserves that allowance for
`GAMESERVER_RESERVATION_SECONDS` (default 60), so simultaneous launches cannot all claim it.
Reservations end early if spawning fails or the process exits. Values must be positive integers;
invalid values refuse the launch. `GAMESERVER_MEMORY_GUARD=0` disables the check for rollback.
Defaults require at least 4608 MiB free with no reservations. These are conservative estimates,
not a guarantee against later memory growth; tune from measured VPS usage. During startup the
reservation can overlap memory already charged by Windows, deliberately erring toward waiting.
Existing worlds are never killed by this guard and can still be joined. A missing Ramsgate or
Dojo also waits for memory before it can be restarted.

`GAMESERVER_HUNT_STARTUP_MB` optionally sets a separate startup estimate for temporary
hunts/tutorials; it defaults to `GAMESERVER_STARTUP_MB`. Ramsgate and Dojo keep the
standard estimate. Reservations sum their actual estimated byte counts, even when
different world kinds start together. On a measured 8 GiB host, a 1536 MiB floor
plus a 1024 MiB hunt estimate requires 2560 MiB free for a hunt (before reservations),
while a persistent world with a 1536 MiB estimate still requires 3072 MiB. This is
operator tuning, not a new global default or a guarantee against native crashes.

Capacity shortages (RAM or hunt ports) return a distinct 503 to the metagame. Active matchmaking
status polls retry at most once every ten seconds per candidate/queue, for up to one minute,
then report FAILED. There are no background launches or guaranteed FIFO ordering. A full four-player
hunt queue refuses further joins until resolved. Normal startup failures still fail immediately.
The existing `MATCHMAKING_CANCEL` switch still controls in-game cancellation; this change does not
enable it because the client also emits automatic cancel requests during normal joins.
Update both server packages together. No database migration is involved.
The normal public grouping window (5 seconds after the first join) and client polling
are additional; the one-minute bound is specifically the capacity wait, not an
end-to-end loading-time guarantee. Capacity replies now log their reason and hunt ID.

Starts and watches the game-server processes when the metagame asks. A worked `.env` is in
[Host a server]({{ host_page.url | relative_url }}#deploy-server).

**When read:** once, at startup. Restart the deploy server after any change, and stop it before the
metagame; see [Run it for a group]({{ admin_page.url | relative_url }}#restarting-after-a-change).
The deploy server checks none of these values: a missing one is not reported at startup, it only
makes launches fail later. Every game server it starts inherits its whole environment, the
game-server key included.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `PORT` | none. Unset: it stops at startup with a port error. Empty: it listens on a random port the metagame cannot find. | 1-65535 | HTTP port. Only the metagame on the same machine calls it, through `DEPLOYSERVER_URL`. | You (61001); kit: always (61001, 62001 in sandbox, or `-DeployPort`) |
| `BIND_HOST` | `127.0.0.1` (also when empty) | one IP address | **Fork only.** Listen address. Keep `127.0.0.1`: there is no authentication, and anyone who reaches the port can start game processes. Both of its routes also refuse any caller that is not loopback or that came through a proxy. | You; kit: always `127.0.0.1` |
| `MY_IP` | none. Unset: every launch fails. | IPv4 address as players reach it | The address handed to clients for every game server (Ramsgate, the Dojo, hunts). `127.0.0.1` for play on one PC, the Tailscale address in private mode, the server's public IPv4 in public mode (for example `203.0.113.10`). With `127.0.0.1` on a shared server, a friend's game connects to its own PC. | You; kit: always (public mode: the IPv4 of the public host, which is `-PublicHost`, else the one saved at the last install, else the network adapter's only public IPv4; the Tailscale address in private mode; `127.0.0.1` in sandbox) |
| `PORT_RANGE_BEGIN` | none. Unset: no hunt ports, every hunt fails. | UDP port | Lowest game-server port. Hunts use `PORT_RANGE_BEGIN` up to `PORT_RANGE_END - 2`: six at a time with 8770-8777. To allow more at once on a hand-built host, lower it and open the extra ports too. | You (8770); kit: always 8770 |
| `PORT_RANGE_END` | none. Unset: Ramsgate cannot start. | keep `8777` | Ramsgate always runs on this port and the Training Dojo on the one below. The rebuilt DLL identifies the permanent Ramsgate by its map rather than its port. Older pinned DLLs disable the idle watchdog by port, so keep this layout when using them. | You (8777); kit: always 8777 |
| `GAMESERVER_BINARY_PATH` | none. Unset or wrong: the deploy server stops soon after start (from reading the code). | full path of `Dauntless-Win64-Shipping.exe`, forward slashes | The 1.4.4 game exe started as each game server; `dxgi.dll` and `UndauntedInternalServer.dll` must sit next to it. Whatever file this names is run with the game-server key on its command line, so only administrators may edit the file. | You; kit: always (`<root>/game/Dauntless/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe`) |
| `METAGAME_API_KEY` | none. Unset: game servers cannot talk to the metagame. | the game-server key (48 hex characters as generated) | **Secret: never share, never commit.** Passed to every game server as its first command-line argument; the server DLL sends it with every request to the metagame, which stores only its SHA-256. It must match `gameserver.key` and be registered in the metagame's database. Local users can see it in the game servers' command lines. | You ([Host a server]({{ host_page.url | relative_url }}#metagame)); kit: always (kept from the backup, the key file or the old file, otherwise new) |
| `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP` | none. Unset: effectively no gap. | seconds; decimals work | Minimum gap between two game-server launches. All launches wait in one queue, so the third hunt requested at the same moment starts about two gaps later. | You (10); kit: 10 when missing or empty, a hand-set value is kept |
| `ENABLE_DOJO` | on demand | `1` or anything else | **Fork only.** `1` starts the Training Dojo at boot, as upstream did. Anything else: it starts the first time someone is matchmade into it, and it starts again on demand after shutdown. | You (0); kit: 0 when missing, kept otherwise |
| `PERSISTENT_WORLD_LIVENESS` | on | `0`, or anything else for on | **Fork only.** On: before Ramsgate or the Dojo is handed to a player, the deploy server checks that its process is alive and starts a dead one first, through the one launch the boot and the watchdog also use (never two processes on 8777). The startup line says `Ramsgate and Dojo liveness check before handing them out: on`. `0`: existing records are handed out unchecked; the watchdog restarts Ramsgate within a minute and clears a dead Dojo for the next request. | Nobody by default; kit: kept |
| `LOG_LEVEL` | `info` (also when empty) | `fatal`, `error`, `warn`, `info`, `debug`, `trace`, `silent` | Log threshold. At `info` the matchmaking line lists the expected players' account ids; the key is never logged. | Nobody by default; kit: kept |
| `NODE_ENV` | unset: coloured readable logs | `production` or anything else | `production`: JSON log lines, and no stack traces in error pages. | You; kit: always `production` |

---

## Gateway (`legacy/UndauntedGateway/`, `dist/server.js`) {#gateway}

The only public TCP listener in public mode. It terminates TLS and passes requests to the metagame
and the content server on loopback. The gateway's own [README]({{ site.github.repository_url }}/blob/dauntless-revived/legacy/UndauntedGateway/README.md)
explains routing, refusals and how the limits were sized.

**When read:** once, at startup. A value outside its range stops the gateway with an error that names
the variable. It also refuses to start with a certificate that is expired, not yet valid or does not
match the key, and warns when fewer than 30 days remain. `NODE_ENV` has no effect here.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `GATEWAY_BIND` | `0.0.0.0` | an IP address (no host names) | Address of the TLS listener. Keep `0.0.0.0` (IPv4): the address the gateway reports to the allowlist must be the IPv4 the player's game traffic comes from. | Kit: always (`0.0.0.0`; `127.0.0.1` in sandbox) |
| `GATEWAY_PORT` | `443` | 1-65535 | TLS port, and the port in public invites. The only port the kit opens to everyone; the game's UDP ports open only for players who logged in (see the allowlist helper below). | Kit: always (443 or `-GatewayPort`; 62443 in sandbox) |
| `GATEWAY_CERT` | none, required | path of a PEM certificate | The certificate served over TLS. The gateway logs its SHA-256 fingerprint, which every invite pins. A new certificate breaks every invite already sent. | Kit: always (`<root>/data/tls/gateway-cert.pem`) |
| `GATEWAY_KEY` | none, required | path of the PEM private key | **Secret file: never share, never commit.** The certificate's private key. | Kit: always (`<root>/data/tls/gateway-key.pem`) |
| `GATEWAY_SECRET` | none, required | 32 to 256 printable characters without spaces | **Secret: never share, never commit.** Sent as `X-Dauntless-Gateway` with every request the gateway passes on (to the metagame, the content server and the WebSocket service); only the metagame checks it, and it must equal the metagame's `GATEWAY_SECRET`. A copy of that header sent by a client is dropped. Never logged. | Kit: always (64 hex characters, kept across re-runs) |
| `GATEWAY_METAGAME_URL` | `http://127.0.0.1:61000` | `http://host:port` on this machine only (`127.x.x.x`, `::1` or `localhost`), no path | Where every request goes that is not `/content` or a WebSocket upgrade. Loopback is enforced so the secret header never leaves the machine. | Kit: always |
| `GATEWAY_CONTENT_URL` | `http://127.0.0.1:61002` | same rule | Where `/content` and `/content/...` go (the content server). | Kit: always |
| `GATEWAY_WS_URL` | `http://127.0.0.1:61099` | same rule | Where WebSocket upgrades go: the metagame's chat listener ([`CHAT_PORT`](#metagame-chat)). With chat off nothing listens there, and upgrades get 502. | Kit: always (61099; 62099 in sandbox), the same port as `CHAT_PORT` |
| `GATEWAY_ALLOWLIST` | on | exactly `0` turns it off | Kill switch for reporting logged-in players' addresses to the allowlist helper. Off: the `ALLOWLIST_*` values are ignored, a warning is logged, and no player's game ports open unless you manage the firewall another way. | Nobody; never written by the kit |
| `ALLOWLIST_URL` | `http://127.0.0.1:61005` | `http://host:port` on this machine only | Where the gateway reports player addresses. | Kit: always |
| `ALLOWLIST_SECRET` | none; required unless `GATEWAY_ALLOWLIST=0` | 32 to 256 printable characters without spaces | **Secret: never share, never commit.** Sent to the allowlist helper; must equal the helper's `ALLOWLIST_SECRET`. On a kit server `gateway.env` holds this copy and is readable by the service account the gateway runs as. | Kit: always (the same value as in `allowlist.env`, kept across re-runs) |
| `GATEWAY_ALLOWLIST_REFRESH_SECONDS` | `60` | 5-540 | How long the gateway waits before reporting the same address again. Keep it well under the helper's `ALLOWLIST_TTL_SECONDS`. | Nobody by default; kit: kept |
| `GATEWAY_MAX_BODY_BYTES` | `131072` (128 KiB) | 1024-67108864 | Largest request body; a larger one gets 413. The largest body the game sends, a character save, is about 22 KB. | Nobody by default; kit: kept |
| `GATEWAY_MAX_CONNECTIONS` | `2048` | 1-100000 | Open connections in total. | Nobody by default; kit: kept |
| `GATEWAY_MAX_CONNECTIONS_PER_IP` | `128` | 1-100000 | Open connections per IPv4 address or IPv6 /64. | Nobody by default; kit: kept |
| `GATEWAY_RATE_GENERAL` | `300,180` | `<burst>,<per minute>` | Request rate for all game traffic not in the buckets below. An empty bucket answers 429 with `Retry-After`. | Nobody by default; kit: kept |
| `GATEWAY_RATE_CONTENT` | `600,600` | `<burst>,<per minute>` | Request rate for `/content` downloads. | Nobody by default; kit: kept |
| `GATEWAY_RATE_REGISTER` | `5,0.2` | `<burst>,<per minute>` | Request rate for account creation: five at once, then one every five minutes. | Nobody by default; kit: kept |
| `GATEWAY_RATE_TOKEN` | `10,1` | `<burst>,<per minute>` | Request rate for logins. | Nobody by default; kit: kept |
| `GATEWAY_RATE_WS` | `20,12` | `<burst>,<per minute>` | Rate of WebSocket upgrades (the game's chat connection). A bucket of their own, so chat reconnects never use up the one the game's HTTP traffic needs. | Nobody by default; kit: kept |
| `GATEWAY_RATE_CONNECT` | `200,300` | `<burst>,<per minute>` | Rate of new TCP connections (each costs a TLS handshake). An empty bucket closes the connection without an answer. | Nobody by default; kit: kept |
| `GATEWAY_HANDSHAKE_TIMEOUT_MS` | `10000` | 1000-120000 | TLS handshake timeout. | Nobody by default; kit: kept |
| `GATEWAY_HEADERS_TIMEOUT_MS` | `10000` | 1000-120000 | Time allowed for a request's headers (answers 408). Also the upstream timeout for a WebSocket handshake. | Nobody by default; kit: kept |
| `GATEWAY_REQUEST_TIMEOUT_MS` | `30000` | 1000-600000 | Time allowed for a whole request, body included (answers 408). | Nobody by default; kit: kept |
| `GATEWAY_KEEPALIVE_TIMEOUT_MS` | `65000` | 1000-600000 | Idle keep-alive time between requests. | Nobody by default; kit: kept |
| `GATEWAY_IDLE_TIMEOUT_MS` | `120000` | 1000-3600000 | A client connection that moves no data for this long is closed. | Nobody by default; kit: kept |
| `GATEWAY_UPSTREAM_IDLE_TIMEOUT_MS` | `120000` | 1000-3600000 | An upstream request that stays silent this long is aborted (504). | Nobody by default; kit: kept |
| `GATEWAY_WS_IDLE_TIMEOUT_MS` | `300000` | 1000-86400000 | Idle timeout of a joined WebSocket, on both sides. | Nobody by default; kit: kept |
| `GATEWAY_SHUTDOWN_GRACE_MS` | `10000` | 0-300000 | On a stop signal, requests in flight get this long to finish before every connection is cut. | Nobody by default; kit: kept |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` or `error`, any case; anything else means `info` | Log threshold. `debug` also logs failed TLS handshakes. | Nobody by default; kit: kept |

The rate buckets count per IPv4 address or IPv6 /64. `<burst>` is how many requests fit in a bucket at
once and `<per minute>` how many it refills per minute: `<burst>` 1-1000000, `<per minute>` at most
1000000 and decimals allowed. Every request uses up a token before any other check, refused ones
included.

---

## Allowlist helper (`legacy/UndauntedGateway/`, `dist/allowlist/server.js`) {#allowlist-helper}

A small service that keeps one Windows Firewall rule open to the addresses of logged-in players. It
runs with administrator rights (as SYSTEM on a kit server), so its settings file is readable only by
administrators and SYSTEM.

**When read:** once, at startup. Invalid values stop the helper with an error that names the
variable. `NODE_ENV` has no effect here.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `ALLOWLIST_DRY_RUN` | none, required | `0` or `1` | `1`: never touches the firewall; every change it would make is written to the audit log. `0`: really changes the firewall; needs Windows and administrator rights, or the helper exits. Use `1` on every machine that is not the server. | Kit: always (`0`; `1` with `-AllowlistDryRun` or `-Sandbox`) |
| `ALLOWLIST_BIND` | `127.0.0.1` | `127.0.0.1` or `::1` only | Listen address. Anything else is refused at startup. | Kit: always `127.0.0.1` |
| `ALLOWLIST_PORT` | `61005` | 1-65535 | Listen port; must match the gateway's `ALLOWLIST_URL`. | Kit: always (61005, 62005 in sandbox, or `-AllowlistPort`) |
| `ALLOWLIST_SECRET` | none, required | 32 to 256 printable characters without spaces | **Secret: never share, never commit.** Every request to the helper must carry it; must equal the gateway's `ALLOWLIST_SECRET`. Never logged. | Kit: always (kept across re-runs) |
| `ALLOWLIST_PORTS` | `8770-8777` | one port or a range `low-high` | The local UDP ports the firewall rule opens. Must match the deploy server's port range. | Kit: always `8770-8777` |
| `ALLOWLIST_TTL_SECONDS` | `600` | 1-86400 | How long an address stays open after its last report. | Kit: always `600` (a hand-set value is overwritten) |
| `ALLOWLIST_MIN_INTERVAL_MS` | `3000` | 50-600000 | Minimum time between two firewall rule changes; changes in between are combined. | Nobody by default; kit: kept |
| `ALLOWLIST_MAX_ENTRIES` | `256` | 1-1000 | Most addresses open at once; a new one beyond that is refused. | Nobody by default; kit: kept |
| `ALLOWLIST_AUDIT_LOG` | `allowlist-audit.log` in the working directory | file path | Append-only log, one JSON object per line, of every change and refusal (addresses added, expired or refused, rule changes, requests with a wrong secret). It contains player IP addresses, never the secret. | Kit: always (`<root>/data/allowlist/audit.log`) |
| `ALLOWLIST_STATE_FILE` | `allowlist-state.json` in the working directory | file path | The open addresses, saved so a restart re-opens the ports for the same players. Contains player IP addresses. | Kit: always (`<root>/data/allowlist/state.json`) |
| `ALLOWLIST_ALLOW_PRIVATE` | off | exactly `1` turns it on | Also accepts private, CGNAT (including Tailscale), loopback and link-local addresses. For tests and LAN setups; keep it off on a public server. | Kit: `1` in sandbox installs only, removed otherwise |
| `ALLOWLIST_POWERSHELL` | `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe` (`C:\Windows` if `SystemRoot` is unset) | full path of an executable | The PowerShell the helper runs, with administrator rights, for every firewall change. Leave it unset. Anyone who can edit this setting can run any program as administrator, which is why the kit makes `allowlist.env` readable only by administrators and SYSTEM. | Nobody; kit: kept |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` or `error`, any case | Log threshold. | Nobody by default; kit: kept |

---

## Content server (`legacy/UndauntedContent/`) {#content-server}

Serves the verified game files to registered accounts, plus the host's news and art for the
launcher. Details of the news file and the art pack (`branding.json`) are on
[Files and data]({{ files_page.url | relative_url }}).

**When read:** once, at startup. Invalid values stop the content server with an error that names the
variable. The news file and the art pack folder themselves are re-read while it runs (news within
5 seconds, art within 10), so editing them needs no restart. `NODE_ENV` has no effect here.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `PORT` | `61002` | 1-65535 | Listen port. | You; kit: always (61002, 62002 in sandbox, or `-ContentPort`) |
| `BIND_HOST` | `127.0.0.1` | comma-separated IP addresses or `localhost` | Listen addresses, one listener each. Only loopback and Tailscale addresses (100.64.0.0/10, fd7a:115c:a1e0::/48) are accepted; anything else, `0.0.0.0` included, needs `CONTENT_ALLOW_ANY_BIND=1`. | You; kit: always (`127.0.0.1` in public mode, the Tailscale address in private mode) |
| `CONTENT_ALLOW_ANY_BIND` | off | exactly `1` turns it on | Lifts the `BIND_HOST` check. Only if a firewall keeps the internet out: game files must never be reachable from the internet directly. | Nobody; never written by the kit |
| `METAGAME_URL` | `http://127.0.0.1:61000` | `http://` or `https://` URL | Where account keys are checked (`GET /undaunted/api/GetUserInfo`). Every downloader's key is sent here, so it must point at your own metagame. | Kit: always (`http://<metagame address>:<port>`) |
| `CONTENT_GAME_DIR` | none, required | folder path | The verified 1.4.4 install (the folder that contains `Archon\`), opened read-only. Only files listed in the manifest are ever served. `npm run verify -- --game-dir <folder>` overrides it for a check. | You; kit: always (`<root>/game/Dauntless`) |
| `CONTENT_MANIFEST` | the built-in `data/dauntless-1.4.4.json` | file path | Another manifest, for tests with a stand-in game folder. | Kit: only in sandbox with `-ContentManifest`, removed otherwise |
| `CONTENT_BRANDING_DIR` | unset: no art | folder path | The host's art pack for the launcher: images plus an optional `branding.json`. Share only art you have the rights to. | Kit: always (`<root>/data/branding`) |
| `CONTENT_NEWS_FILE` | unset: no news | file path | The host's news for the launcher. A broken edit keeps the last good version online. | Kit: always (`<root>/data/config/news.json`, created empty if missing) |
| `CONTENT_MAX_STREAMS_PER_ACCOUNT` | `6` | 1-64 | Downloads one account may run at once (429 above it). | Nobody by default; kit: kept |
| `CONTENT_MAX_STREAMS_TOTAL` | `48` | 1-1024 | Downloads the whole server runs at once (503 above it). | Nobody by default; kit: kept |
| `CONTENT_AUTH_CACHE_SECONDS` | `300` | 0-3600 | How long a good key's check is cached (keyed by a hash of the key, never the key itself). Refused keys are cached for 30 seconds; metagame outages are never cached. | Nobody by default; kit: kept |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` or `error`, any case | Log threshold. | Nobody by default; kit: kept |

The content server holds no secrets of its own. Account keys pass through it to the metagame and are
never logged.

---

## Launcher environment overrides {#launcher}

The friend launcher has a settings page of its own (graphics, auto exposure, window, language). Where it keeps them
is on [Files and data]({{ files_page.url | relative_url }}), and what the graphics level writes into the game's
ini files on [Game settings]({{ gamesettings_page.url | relative_url }}). From its environment it
reads only the variables below.

| Name | Default | Values | What it does | Set by |
|:-----|:--------|:-------|:-------------|:-------|
| `DAUNTLESS_REVIVED_RELAY_PORT` | unset: 61000 | whole number 1024-65535 without a leading zero; anything else is ignored | For tests only, for example on a PC where the development stack already holds 61000. In public mode it moves the launcher's local relay, the address the game is started with and the game's chat port in `Engine.ini` to this port, and the launcher logs a warning. Private mode ignores it. Friends must keep 61000: the server hands every player `http://127.0.0.1:61000/QoS`. | A developer or tester, in the launcher's environment (`$env:DAUNTLESS_REVIVED_RELAY_PORT = "62100"` before starting it). Never set by any installer or kit. |
| `LOCALAPPDATA` (`USERPROFILE` as a fallback) | set by Windows | folder | Default game folder (`%LOCALAPPDATA%\DauntlessRevived\Game`), the game's user config folder the launcher rewrites, and one place it looks for `tailscale.exe`. | Windows |
| `ProgramW6432`, `ProgramFiles`, `ProgramFiles(x86)`, `PATH` | set by Windows | folders | Where else the launcher looks for `tailscale.exe` (private mode only). `C:\Program Files\Tailscale` is always tried. | Windows |
| `WINDIR` (`SystemRoot` as a fallback) | set by Windows | folder | Where it checks for the Visual C++ runtime the server DLL needs. | Windows |

The installed launcher ignores `NODE_OPTIONS`, `ELECTRON_RUN_AS_NODE` and the Node inspector flags:
they are switched off when the app is packaged, so nobody can inject code into it through the
environment.

---

## What the Windows server kit writes {#server-kit}

`Install-DauntlessServer.ps1` writes the five files in `<root>\data\config\` (default root
`C:\DauntlessRevived`). On every run it reads the existing file, and with `-RestoreFrom` also the
backup's `metagame.env` and `deployserver.env` (the backup's values win), keeps every key it finds,
and then sets the keys that describe this host. So a setting you add by hand, such as
`PROGRESSION_MODE=stub`, survives installer re-runs and updates; a hand-edited key from the "always
set" column does not. Change those through the installer's parameters instead
([Scripts and parameters]({{ scripts_page.url | relative_url }})).

Other rules:

- Comment lines are not kept: the file is rewritten with one header line.
- Values containing a quote, a backslash or a line break are refused, so paths are written with
  forward slashes.
- `Update-DauntlessServer.ps1` changes only `GIT_COMMIT` in `metagame.env`.
- Ports come from `-MetagamePort`, `-DeployPort`, `-ContentPort`, `-AllowlistPort` and
  `-GatewayPort`, else from the existing install's `server.json`, else the defaults on
  [Ports and network]({{ ports_page.url | relative_url }}).
- `NODE_ENV=production` is written into all five files. Only the metagame and the deploy server read
  it.
- Chat: `-Chat On` or `-Chat Off`, else the existing install's `server.json` (`"Chat"`), else On. The
  choice is saved in `server.json`. `Set-Chat.ps1 -On` or `-Off` changes it later, in both
  `metagame.env` and `server.json`. Private mode has no chat yet.
- The installer writes none of the switches added with the port of Harmonic's fork (`ESCALATION_*`,
  `STORE*`, `SLAYER_LINKS`, `CHAT_PRESENCE`, `VERIFY_STUB_ACCOUNT`, `BALANCE_FROM_INVENTORY`,
  `PROGRESSION_REPLAY_WINDOW_S`, `PROGRESSION_CONFIRM_ENTITLEMENTS`, `PROGRESSION_CONFIG_DIR`,
  `ACTIVE_HUNT_PASS`, `BODY_LOG_PER_PATH`, and `PERSISTENT_WORLD_LIVENESS` in `deployserver.env`), so
  their defaults apply. Add one by hand to change it; it is kept across re-runs and updates
  ([Run it for a group]({{ admin_page.url | relative_url }}#switching-features-on)).

| File | Written | Always set by the installer | Set only when missing | Removed | Kept (yours) |
|:-----|:--------|:----------------------------|:----------------------|:--------|:-------------|
| `metagame.env` | every run | `PORT`, `BIND_HOST`, `AUTH_MODE`, `DB_FILENAME`, `TARGET_CHANGELIST`, `QOS_TARGET_URL`, `MATCHMAKING_MODE`, `DEPLOYSERVER_URL`, `REGISTRATION_MODE`, `NODE_ENV`, `SERVER_NAME`, `SOURCE_URL`, `GIT_COMMIT`, `BODY_LOG_FILE`; in public mode also `GATEWAY_SECRET`, `LOG_BODIES=0`, `CHAT` (from `-Chat`, else `server.json`), `CHAT_BIND_HOST=127.0.0.1` and `CHAT_PORT` (the port in `GATEWAY_WS_URL`); with the content server also `CONTENT_PORT` | `AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64` (a new pair) | `GATEWAY_SECRET` in private mode (and `CHAT` there becomes `0`); `CONTENT_PORT` without the content server | everything else, for example `PROGRESSION_*`, `SAVE_HISTORY_*`, `LOG_LEVEL`, `DB_WAL`, `CHAT_NICK_CHECK`, `CHAT_TRACE` |
| `deployserver.env` | every run (also in sandbox, where no deploy server runs) | `PORT`, `BIND_HOST=127.0.0.1`, `MY_IP`, `PORT_RANGE_BEGIN=8770`, `PORT_RANGE_END=8777`, `GAMESERVER_BINARY_PATH`, `METAGAME_API_KEY`, `NODE_ENV` | `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP=10` (also when empty), `ENABLE_DOJO=0` | nothing | `LOG_LEVEL`, your `ENABLE_DOJO` and wait time |
| `content.env` | when the installed code has the content server | `PORT`, `BIND_HOST`, `METAGAME_URL`, `CONTENT_GAME_DIR`, `CONTENT_BRANDING_DIR`, `CONTENT_NEWS_FILE`, `NODE_ENV` | nothing | `CONTENT_MANIFEST` (unless sandbox `-ContentManifest`) | `CONTENT_MAX_STREAMS_*`, `CONTENT_AUTH_CACHE_SECONDS`, `LOG_LEVEL` |
| `gateway.env` | public mode | `GATEWAY_BIND`, `GATEWAY_PORT`, `GATEWAY_CERT`, `GATEWAY_KEY`, `GATEWAY_SECRET`, `GATEWAY_METAGAME_URL`, `GATEWAY_CONTENT_URL`, `GATEWAY_WS_URL`, `ALLOWLIST_URL`, `ALLOWLIST_SECRET`, `NODE_ENV` | nothing | `GATEWAY_CHAT_URL`, `GATEWAY_ACCESS_LOG` (names from early drafts) | limits, rates, timeouts, `GATEWAY_ALLOWLIST_REFRESH_SECONDS`, `LOG_LEVEL` |
| `allowlist.env` | public mode | `ALLOWLIST_BIND`, `ALLOWLIST_PORT`, `ALLOWLIST_SECRET`, `ALLOWLIST_PORTS`, `ALLOWLIST_TTL_SECONDS=600`, `ALLOWLIST_AUDIT_LOG`, `ALLOWLIST_STATE_FILE`, `ALLOWLIST_DRY_RUN`, `NODE_ENV`; in sandbox also `ALLOWLIST_ALLOW_PRIVATE=1` | nothing | `ALLOWLIST_ALLOW_PRIVATE` outside sandbox; `ALLOWLIST_RULE_NAME`, `ALLOWLIST_UDP_PORTS`, `ALLOWLIST_PROGRAM` (names from early drafts) | `ALLOWLIST_MIN_INTERVAL_MS`, `ALLOWLIST_MAX_ENTRIES`, `ALLOWLIST_POWERSHELL`, `LOG_LEVEL` |

**Where the secrets come from.** The token-signing pair is kept from the backup or the existing file,
otherwise generated once (RSA-2048). The game-server key (48 hex characters) is kept from the backup,
then `data\keys\gameserver.key`, then the old `deployserver.env`, otherwise generated. `GATEWAY_SECRET`
and `ALLOWLIST_SECRET` (64 hex characters each) are kept across re-runs and generated fresh on a new
machine; a restore does not need them. The installer never prints any of them.

**Who can read the files.** `data\config\` is read-only for the service account that runs the
stack, and `allowlist.env` is readable only by administrators and SYSTEM. Edit the files from an
elevated editor, then restart the component with `Stack.ps1 restart -Only <component>`.

**Backups.** `Backup-DauntlessServer.ps1` copies `metagame.env`, `deployserver.env`, `content.env` and
`gateway.env` into `backups\<date>_<time>\secrets\`, together with the `*.key` files from
`data\keys\` and the gateway's TLS certificate and key. `allowlist.env` is not copied (only administrators and SYSTEM can read it, and an
install on a new machine makes a new allowlist secret). `gateway.env` carries copies of
`GATEWAY_SECRET` and `ALLOWLIST_SECRET`, so a backup holds every secret of the server: copy backups
off the server only encrypted, and never commit them.

---

## Friend kit and client side {#client-side}

The friend kit (`friend-kit/`) and the launcher set nothing on the server. The friend kit's
`play.ps1` takes the server as `-Server host[:port]` (port 61000 when left out) and passes it to the
game as its first command-line argument; the launcher takes the address from the invite. Both keep
the player's account key on the player's PC. See [Join as a friend]({{ friends_page.url | relative_url }})
and [Game settings]({{ gamesettings_page.url | relative_url }}).

---

## Windows variables the scripts read {#windows-variables}

The server kit's scripts and the friend kit take a few standard Windows variables from their own
process. You do not set these yourself; the table says what each is used for.

| Name | Read by | What it is used for |
|:-----|:--------|:--------------------|
| `SSH_CONNECTION` | `Install-DauntlessServer.ps1` | Set by the OpenSSH server. When the installer runs over SSH (as `Deploy-Remote.ps1` does), the key login has been proven, so it turns SSH password login off. From the console or Remote Desktop it only warns. |
| `COMPUTERNAME` | the server kit | Forms the service account's name, `<COMPUTERNAME>\dauntless`. |
| `WINDIR`, `ProgramFiles`, `ProgramData`, `Path`, `TEMP` | the server kit | `System32` tools (`tar.exe`, `cmd.exe`, OpenSSH), `nodejs\node.exe` and `Tailscale\tailscale.exe`, `ssh\sshd_config`. The installer adds the Node.js folder to `Path` for its own session. `Deploy-Remote.ps1` stages uploads under `%TEMP%`. |
| `APPDATA`, `LOCALAPPDATA`, `WINDIR` | friend kit (`setup.ps1`, `play.ps1`) | `%APPDATA%\DauntlessRevived\` holds `account.key` (**secret: never share it, never commit it**) and `settings.json`; `play.ps1` writes the game's ini files under `%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\`; `setup.ps1` checks `%WINDIR%\System32` for the Visual C++ runtime. |

When the kit builds the code (`npm ci`, `npm run build`) it sets `NODE_ENV=development`, so that
TypeScript is installed, and turns off npm's update, funding and audit messages. That is only the
build step: the running components get `NODE_ENV=production` from their `.env` files.

---

## CI and release settings {#ci-settings}

These are not read by any server component. They are settings of the GitHub repository, read by the
workflows in `.github/workflows/` ([Developer guide]({{ dev_page.url | relative_url }}#ci)). A fork has
its own.

| Name | Kind | Default | Values | What it does | Set by |
|:-----|:-----|:--------|:-------|:-------------|:-------|
| `LAUNCHER_AUTO_RELEASE` | Repository variable (Settings > Secrets and variables > Actions > Variables), read by `ci.yml` | unset: on | `false` pauses automatic releases; unset, they are on | Automatic launcher releases: a push to `dauntless-revived` that passes every check, is still the head of the branch, and has a launcher version with no `launcher-v<version>` release yet publishes that launcher ([Launcher releases]({{ dev_page.url | relative_url }}#launcher-releases)). Only the repository `mixutin/dauntless-revived` publishes automatically. While it is `false`, Actions > **Launcher release** > **Run workflow** still publishes by hand. | The repository owner |

The workflows need no secrets of their own: they use the token GitHub gives every run, with write
access only in the jobs that publish a release. Code scanning with CodeQL is the repository's default
setup, a repository setting, not a workflow file. GitHub's immutable releases setting must stay off,
because the `launcher-updates` release that installed launchers read is updated in place.

---

## Test-only variables {#test-only}

Only the automated tests read these. How to run the tests is in the
[Developer guide]({{ dev_page.url | relative_url }}).

| Name | Component | Default | What it does |
|:-----|:----------|:--------|:-------------|
| `TEST_LOG_LEVEL` | metagame and deploy server tests | `silent` | Log level during `npm test` (copied into `LOG_LEVEL`). The tests set their own `NODE_ENV`, database and ports and never read your `.env`. |
| `CONTENT_IT_GAME_DIR` | content server integration test | `C:\D144\Dauntless` | The real 1.4.4 install to serve; the test is skipped when it is missing. |
| `CONTENT_IT_PORT`, `CONTENT_IT_MOCK_PORT` | content server integration test | `62002`, `62003` | Ports of the server under test and of the mock metagame. 62002 is also the kit sandbox's content port, so do not run both at once. |

---

## Names that are not settings {#not-settings}

- `GAMESERVER_LAUNCHER` appears on the roadmap as an idea for starting game servers through Wine. No
  code reads it.
- A list in the metagame's `BIND_HOST` (`127.0.0.1,<Tailscale address>`) is planned. Today it takes
  one address.
- `GATEWAY_CHAT_URL`, `GATEWAY_ACCESS_LOG`, `ALLOWLIST_RULE_NAME`, `ALLOWLIST_UDP_PORTS` and
  `ALLOWLIST_PROGRAM` are names from early drafts of the server kit. Nothing reads them, and the
  installer deletes them.
- `EXPERIMENTAL_CHAT` switched on the first chat prototype. Nothing reads it now: the switch is
  [`CHAT`](#metagame-chat), and the metagame warns at startup when it finds the old name without the
  new one.
- Harmonic's 1.4.4 fork has settings we did not take ([why]({{ '/findings/harmonic-fork.html' | relative_url }})):
  `WIRE_CAPTURE`, `WIRE_CAPTURE_MAX_BODY` and `WIRE_CAPTURE_MAX_PER_PATH` (our `LOG_BODIES` and
  `BODY_LOG_PER_PATH` do that job), `HUNT_PASS_SEASONS_DIR` (ours is `PROGRESSION_CONFIG_DIR`),
  `HUNT_PASS_PREMIUM_MODE`, `XMPP_PORT`, and in the deploy server `ENABLE_WATCHDOG`, `HOST`,
  `GAMESERVER_LOG_DIR`, `GAMESERVER_LOG_CMDS` and `METAGAME_ADDRESS`; in the fork's DLL, `UNDAUNTED_DIAG_LOG`.
  Nothing here reads them.


The rebuilt server DLL reclaims hunts, tutorials and the Training Dojo after 30 seconds
without a pending or open native connection, measured with a monotonic clock. Before
the first connection it allows 180 seconds for cold client loading. Ramsgate remains
permanent. The Dojo is restarted on the next request rather than immediately by the
watchdog. These native changes require rebuilding and installing the DLL; the pinned
launcher binary does not change when its source is edited.

### Ramsgate instances and automatic region selection

`RAMSGATE_POOL=1` is the deploy server default. A new installation starts no empty
Ramsgate at boot. A CITY request reserves seats in a local instance with fewer than
20 players; when none fits the whole travelling party, another city starts. The
first instance uses `PORT_RANGE_END`; extra cities share the hunt port pool, excluding
the Dojo port. Memory and port capacity can still reject a start. Each instance
advertises and sets a native GameSession limit of 20. Reservations protect concurrent
travellers while they load. Fresh native occupancy snapshots consume arriving
reservations; missing or stale snapshots never prove free seats.

An occupied city exits on its first tick without pending or open connections. A newly
requested city has 180 seconds to receive its first connection, so it cannot exit
before its requesting client loads. Exited cities release their ports and stay stopped
until requested again. `RAMSGATE_POOL=0` restores the old permanent-city behavior for
rollback/older DLLs. In pool mode the old two-city `CITY_OVERFLOW` balancer is bypassed;
regional routing still chooses the worker, which owns its own pool.

The launcher now defaults to **Automatic (closest region)** when no explicit region
is saved. On Play it measures two TCP handshakes per available region and compares the
second handshake latency (avoiding the initial DNS/cold-connection cost). This is a
network-latency estimate, not geographic distance or an in-game UDP RTT measurement.
Players can still choose EU, Australia, or Germany, and existing manual choices stay
manual. Parties still follow their leader. Automatic is reevaluated on each Play.

On the metagame, `REGION_MAIN_PROBE_HOST`, `REGION_AUS_PROBE_HOST`, and
`REGION_GER_PROBE_HOST` identify reachable TCP listeners on the actual regional hosts;
corresponding `REGION_*_PROBE_PORT` values default to 443. Main falls back to the invite
host and port. Australia and Germany are advertised only with `AUS_REGION=1` and
`GERMANY_REGION=1`. Probe sockets transmit no credentials or application data.
Failed probes are ignored; if none works, the current backend region is retained.
Configure a reachable listener before enabling automatic selection of another region.
GET `/undaunted/api/HuntRegion` now includes `probes: [{region, host, port}]` alongside
its existing fields. Region saving and the manually selected region API are unchanged.

Install the rebuilt server DLL and updated launcher to activate native shutdown and
automatic selection. Editing source does not replace the pinned installer binaries.
