---
title: Developer guide
parent: Reference
nav_order: 7
description: "How to work on Dauntless Revived: prerequisites, building and testing each package, running the stack and the launcher locally, CI, and where the code lives."
lang: en
ref: reference/development
---

{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign ports_page = site.pages | where: "path", "reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign game_settings_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "reference/scripts.md" | first %}

# Developer guide
{: .no_toc }

How to work on the code: what to install, how to build and test each package, how to run the stack
and the launcher on your own PC, what CI checks, and where each part of the code lives. Hosting a
server for friends is covered in [Host a server]({{ host_page.url | relative_url }}) and
[Windows server kit]({{ winserver_page.url | relative_url }}). This page links to them instead of
repeating them. Every setting is in [Configuration]({{ config_page.url | relative_url }}), every port
in [Ports and network]({{ ports_page.url | relative_url }}).

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Prerequisites {#prerequisites}

| What | Version | Needed for |
|---|---|---|
| Windows | 10 or 11, 64-bit | The launcher, the server kit, its tests and the game run only on Windows. |
| Node.js and npm | Node 24 with npm 11. The server kit installs exactly Node 24.19.0. (The launcher's `package.json` also accepts Node 20.19 or newer; use 24 anyway.) | Every package. `better-sqlite3` (metagame and deploy server) is a native module built for one Node version: run `npm ci` again in those two folders after you change Node ([Troubleshooting]({{ trouble_page.url | relative_url }}#npm-allow-scripts-warnings)). |
| Git | A current Git for Windows, with `core.longpaths` on | Cloning, and `Test-DeployRemote.ps1`, which packs the checkout with `git archive`. Without Git a metagame build still works, but records its commit as `unknown`. |
| Windows PowerShell | 5.1, built into Windows | The server kit and its tests. The kit is written for 5.1. |
| Visual Studio 2022 | C++ desktop tools (toolset v143), x64 | Only to build the server DLL from source ([below](#server-dll)). |
| Dauntless 1.4.4 | A verified install ([how to check it]({{ host_page.url | relative_url }}#verify-the-build)) | Only for playing against your own stack, the content server's integration test and a new content manifest. No unit test needs game files. |

---

## Get the code {#clone}

```powershell
New-Item -ItemType Directory -Force C:\dr | Out-Null
git -c core.longpaths=true clone https://github.com/mixutin/dauntless-revived.git C:\dr\undaunted
git -C C:\dr\undaunted config core.longpaths true
git -C C:\dr\undaunted checkout dauntless-revived
```

`dauntless-revived` is the default branch: pull requests go to it, and the docs site is published
from its `docs/` folder. Keep the clone at a short path: the server DLL's generated SDK alone has over
4,000 files, and a clone into a deeply nested folder has failed with "Filename too long"
([Troubleshooting]({{ trouble_page.url | relative_url }}#git-filename-too-long)).

### What is in the repository {#layout}

The `Undaunted*` folder names come from upstream Undaunted and are kept as they are. The npm packages
inside them are named `dauntless-revived-metagame`, `dauntless-revived-deploy-server`,
`dauntless-revived-content`, `dauntless-revived-gateway` and `dauntless-revived-launcher`.

| Folder | What it is | Language and tools |
|---|---|---|
| `legacy/UndauntedMetagame/` | The backend the game talks to: accounts and keys, characters, inventory, loadouts, progression, parties, friends, guilds, matchmaking, the admin API, and the game's text chat (an XMPP listener in the same process, on when `CHAT=1`). | TypeScript, Express 5, SQLite through `better-sqlite3` and Drizzle |
| `legacy/UndauntedDeployServer/` | Starts and watches the game-server processes when the metagame asks. It has no authentication and answers loopback callers only. | TypeScript, Express 5 |
| `legacy/UndauntedContent/` | Serves the verified 1.4.4 game files, the host's art pack and news to the launcher. | TypeScript, Node's own `http` |
| `legacy/UndauntedGateway/` | Public mode: the TLS gateway (the server's only public TCP port) and the allowlist helper (one Windows Firewall rule for the game's UDP ports). | TypeScript, Node's own `https`; `node-forge` for the certificate tool |
| `legacy/UndauntedLauncher/` | The Dauntless Revived Launcher that friends install: invites, registration, downloads, the local relay, starting the game. `assets/` holds the two prebuilt DLLs the game needs. | TypeScript, Electron, Electron Forge and Vite |
| `native/internal-server/` | The C++ source of the server DLL, from upstream, with a generated SDK. | C++, Visual Studio 2022 |
| `deploy/windows-server/` | The Windows Server kit: install, update, backup, invites, the stack supervisor, and its tests in `tests/`. | Windows PowerShell 5.1, Node helpers in `lib/` |
| `friend-kit/` | The manual scripts for friends without the launcher (`setup.ps1`, `play.ps1`). | PowerShell |
| `tools/` | The docs generators, the content manifest generator and the friend kit builder ([Scripts and parameters]({{ scripts_page.url | relative_url }})), and in `tools/ci/` the repository check and the launcher version rules CI uses ([below](#ci)). | Node, PowerShell |
| `docs/` | This site. Finnish pages are in `docs/fi/`. | Jekyll with the just-the-docs theme |
| `.github/` | Issue and pull request templates, the GitHub Actions workflows (CI and launcher releases) and the Dependabot settings. | YAML |

---

## Build and test each package {#packages}

Each Node package is installed, built and tested on its own, from its own folder. Use `npm ci`, so
you get exactly the versions in that package's `package-lock.json`:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
npm ci --no-audit --no-fund
npm run build
npm test
```

| Package | Build | Test | Other scripts |
|---|---|---|---|
| `legacy/UndauntedMetagame` | `npm run build`: TypeScript into `dist/`, then `scripts/write-build-info.js` writes `dist/build-info.json` (commit, version, build time) | `npm test` | `npm run dev` (restarts when you change a source file), `npm start`, `npm run db:generate` |
| `legacy/UndauntedDeployServer` | `npm run build` (into `dist/`) | `npm test` | `npm run dev`, `npm start` |
| `legacy/UndauntedGateway` | `npm run build` (into `dist/`) | `npm test` | `npm start` (the gateway), `npm run start:allowlist` (the helper), `npm run make-cert` |
| `legacy/UndauntedContent` | `npm run build` (into `dist/`) | `npm test`, `npm run test:integration` | `npm start`, `npm run verify` (hashes a game folder against the manifest) |
| `legacy/UndauntedLauncher` | `npm run make` (the installer and a zip, in `out/make/`), `npm run package` (the app folder only) | `npm run typecheck`, `npm test` | `npm start`, `npm run icon` |

`dist/`, `build/` and the launcher's `out/`, `.vite/` and `.test-build/` are git-ignored. Build them
yourself and never commit them. After a pull, run `npm run build` again before `npm start`: `npm start`
runs whatever is in `dist/`. `tsc` never deletes files from `dist/`, so delete the folder first when a
source file was removed or renamed.

The build info names the exact source that is running, in `GET /undaunted/api/ServerStatus` and
`/dauntless-status` ([HTTP API]({{ api_page.url | relative_url }})). A build from a folder with
uncommitted changes under `legacy/UndauntedMetagame/` gets `-dirty` after the commit. `GIT_COMMIT` in the
metagame's `.env` overrides the recorded commit. `npm run dev` runs the sources without a build, so it
has no build info: the status routes then show the commit as `unknown` unless `GIT_COMMIT` is set.

---

## Run the tests {#tests}

### Package tests {#package-tests}

`npm test` in the four server packages deletes `build/`, compiles the sources and the tests with
`tsconfig.test.json` into `build/`, and runs them with Node's own test runner. Each test file runs in
its own process. The gateway runs its files one at a time. The launcher's `npm test` compiles into
`.test-build/` and also runs one file at a time. `npm test -- relay` runs only the launcher test
files whose name contains `relay`.

| Package | What the tests cover | Good to know |
|---|---|---|
| Metagame | Accounts and usernames, what each kind of key may do, characters and save history, the database and its migrations (a database at migration 0013 full of rows keeps every row through the new ones), inventory transactions, loadouts and slots, progression (the default, real mode and stub mode, rank math, the upgrade notice, the retry guard, the config folder), Escalation, the free store, entitlements, cooldowns and bounties, parties, friends, Slayer Links, guilds, chat and friends' presence, matchmaking and its failures, the body log, server status. The cases ported from Harmonic's fork (`*ported.test.ts`, `escalation`, `freestore`, `matchmakingfailure`, `slayerlinks`) name the original file and line in a comment. | Each file gets its own empty database in `%TEMP%\undaunted-test-*`, a throwaway signing key pair, `AUTH_MODE=APIKEY` and `NODE_ENV=production`. Logs are silent; set `TEST_LOG_LEVEL` (for example `debug`) to see them. |
| Deploy server | Game-server kinds and the list the metagame reads, the checks on a matchmaking request, loopback-only access, Ramsgate and the Dojo started again on demand with one launch, a missing game binary and a failed startup (with a stand-in spawn) | `TEST_LOG_LEVEL` works here too. |
| Gateway | TLS and the pinned fingerprint, routing and refusals, forwarding headers, size and rate limits, WebSocket proxying, the allowlist feed and helper, both entry points as real processes | The helper always runs in dry-run mode: the tests never change the firewall. |
| Content server | Settings, paths, ranges, the key cache, limits, the manifest, art pack and news, the request handler | `npm run test:integration` needs a real install ([below](#content-integration-test)). |
| Launcher | Invites, the relay, downloads (resume, verify, repair) over HTTP and pinned TLS, `Engine.ini`, launch arguments, the key store, the whole public-mode flow through the controller | Makes throwaway self-signed certificates in a temp folder. |

To run one metagame test file, build the tests once and call Node from the package folder:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
npx tsc -p tsconfig.test.json
node --test build/test/party.test.js
```

**Test ports.** Tests listen only on spare loopback ports from 62000 up, never on the ports of a
running stack (61000-61099). Some suites use the same numbers: the launcher's tests share 62012 with
the content server's, 62013 with the deploy server's, and 62401-62404, 62409 and 62420-62422 with the
gateway's; a launcher test and the sandbox's gateway both use 62443; and the content integration test
and the sandbox both use 62002. **Run one suite at a time on a PC.** The full list is in
[Ports and network]({{ ports_page.url | relative_url }}#test-ports).

### Content server integration test {#content-integration-test}

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedContent
$env:CONTENT_IT_GAME_DIR = "C:\D144\Dauntless"   # the default; the folder that contains Archon\
npm run test:integration
```

It builds the server and starts `dist/server.js` as its own process on `127.0.0.1:62002`, with a mock
metagame on `127.0.0.1:62003` (`CONTENT_IT_PORT` and `CONTENT_IT_MOCK_PORT` change them), and serves
the install read-only. The default game folder is the host guide's
[short install path]({{ host_page.url | relative_url }}#short-install-path); point `CONTENT_IT_GAME_DIR`
at yours. Without an install there every test in it is skipped. CI has no game files, so it never runs
this test: run it yourself when you change the content server.

### Server kit tests {#kit-tests}

Three scripts in `deploy\windows-server\tests\`, for Windows PowerShell 5.1. None of them changes the
firewall, services, scheduled tasks, accounts or certificate stores, and none uses ports 61000-61099.

```powershell
Set-Location C:\dr\undaunted
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-KitUnit.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-DeployRemote.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-Sandbox.ps1
```

| Script | What it checks | Needs | Parameters |
|---|---|---|---|
| `Test-KitUnit.ps1` | Every kit script parses in PowerShell 5.1 and is ASCII only; invite strings v1 and v2, including everything that must be refused; certificate fingerprints; TLS pinning against a local test server; `Get-ServerStatus.ps1` with and without an account key; account key files; the chunked-upload helper; the performance sampler. | `npm ci` in `legacy/UndauntedGateway` first: it runs the gateway's certificate tool. | `-WorkDir` (default `%TEMP%\dr-kit-unit`, deleted at the end), `-Port` (default 62450; 62000-62499; the performance check also uses the port above it) |
| `Test-DeployRemote.ps1` | `Deploy-Remote.ps1` without a server: argument handling, then the kit, source, backup and game-zip uploads into a local folder that stands in for the server. An interrupted upload that resumes, a part damaged in transit or after it was checked, a second run with nothing left to do, and an upload of the working tree with no `.env`, key, database, `node_modules` or `dist` in it. | Git. No network, no SSH key. | `-WorkDir` (default `%TEMP%\dr-deploy-test`, deleted at the end) |
| `Test-Sandbox.ps1` | A real `-Sandbox` install of this checkout in public mode, into a scratch folder with a stand-in game folder: `-WhatIf` first, then the install (it builds the four server packages, which takes a few minutes), invites, status and registration through the gateway with the pinned certificate, the gateway's refusals, an update and a rollback, status, backup and stop, and a second install restored from that backup. Then it stops everything and deletes the folder. It also checks that a stack running on 61000/61001 was left alone. The deploy server and the game never run in the sandbox. | Free loopback ports 62000, 62002, 62005 and 62443 (it stops if one is taken); the npm registry, for `npm ci` | `-SandboxDir` (default `C:\dr\sandbox-ws2019`; the folder name must contain `sandbox`, because the folder is deleted), `-KeepSandbox` (keep the folder and the stopped install), `-SkipRestore` (skip the second install) |

The default `-SandboxDir` follows the `C:\dr` layout of the host guide. Any other folder works, for
example `-SandboxDir "$env:TEMP\dr-sandbox"`. `Test-Sandbox.ps1` leaves its log in
`%TEMP%\dr-sandbox-test.log` (with `-KeepSandbox`, as `test.log` in the sandbox folder). The sandbox
makes its own keys and secrets and deletes them with the folder; the kit never prints them. What the
kit itself does on a real server is on [Windows server kit]({{ winserver_page.url | relative_url }}).

---

## Run the stack locally {#local-stack}

For work on the server side you run the same components a host runs, from your clone.
[Host a server]({{ host_page.url | relative_url }}) walks through every step for the metagame and the
deploy server: building, the `.env` files, the keys, the first start, the game-server key, the admin
account and launching the client. This section covers what is different on a development PC and how
the other components fit in.

**Use a database of its own.** Never point a development metagame at the database your players use.
The metagame runs any new migration on it at start, and it takes no backup first. (The server kit
backs up before every start; a hand-started metagame does not.)

### What each component needs {#components}

| Component | Settings file | Start with | Needs first | Only needed for |
|---|---|---|---|---|
| Metagame | `legacy\UndauntedMetagame\.env`. It must exist. | `npm run dev` or `npm start` | nothing | everything |
| Deploy server | `legacy\UndauntedDeployServer\.env`. It must exist. | `npm run dev` or `npm start` | the metagame with a registered game-server key; a verified 1.4.4 install with the two DLLs; the user `Game.ini` ([host step 8]({{ host_page.url | relative_url }}#game-ini)) | starting game servers: Ramsgate, the Training Dojo, hunts |
| Content server | `legacy\UndauntedContent\.env`, read if it exists. `CONTENT_GAME_DIR` has no default. | `npm start` | the metagame (it checks account keys there); a verified 1.4.4 install | the launcher's downloads |
| Allowlist helper | `legacy\UndauntedGateway\.env`, read if it exists. `ALLOWLIST_DRY_RUN` and `ALLOWLIST_SECRET` have no default. | `npm run start:allowlist` | nothing | public mode |
| Gateway | `legacy\UndauntedGateway\.env`, read if it exists. The certificate, its key and both secrets have no default. | `npm start` | the metagame, the content server, the allowlist helper, a certificate | public mode |

The metagame's and the deploy server's scripts load `.env` with Node's `--env-file`, and Node stops
if the file is missing. The gateway's and the content server's scripts load it only if it exists
(`--env-file-if-exists`), so their required settings can also come from your shell. Variables already
set in your shell win over the file. Settings are read at start: restart a component after you change
its file. The gateway and the helper read the same `.env` when you start them through npm; on a server
the kit gives each its own file. The metagame calls the deploy server without any key, and the deploy
server answers only direct callers on loopback (403 for anything else), so `DEPLOYSERVER_URL` must point
at loopback, such as `127.0.0.1:61001`.

`.env` files hold secrets. They are git-ignored in every package: never share them, never commit them.

### Metagame settings for development {#metagame-env}

The same keys as in [host step 9]({{ host_page.url | relative_url }}#metagame), with a database of
its own and without `NODE_ENV=production`:

```powershell
New-Item -ItemType Directory -Force C:\dr\dev | Out-Null
@"
PORT=61000
BIND_HOST=127.0.0.1
AUTH_MODE=APIKEY
DB_FILENAME=C:/dr/dev/undaunted-dev.db
TARGET_CHANGELIST=239827
QOS_TARGET_URL=http://127.0.0.1:61000/QoS
MATCHMAKING_MODE=DEPLOYSERVER
DEPLOYSERVER_URL=127.0.0.1:61001
REGISTRATION_MODE=OPEN
"@ | Set-Content C:\dr\undaunted\legacy\UndauntedMetagame\.env -Encoding ascii
```

Then append the signing keys (next section): without them the metagame stops at start. Always set
`DB_FILENAME` to a file in a folder that exists: without it, the metagame runs on a temporary database
that is gone when it stops. What is different from a host:

- **No `NODE_ENV=production`.** The logs are then pretty-printed instead of JSON lines, error pages
  include stack traces, and `AUTH_MODE=NONE` becomes possible. Keep such a metagame on `127.0.0.1`.
- **No game install?** Set `MATCHMAKING_MODE=DISABLED` and leave the deploy server out: the metagame
  then refuses every matchmaking request, and you can work on the rest without game servers.
- **Testing the launcher's downloads?** Add `CONTENT_PORT=61002` and run the content server. In
  private mode the launcher learns the content server's port from the metagame's status route.
- **Progression is real by default.** Leave `PROGRESSION_MODE` out. `PROGRESSION_MODE=stub` brings
  back upstream's fake max ranks, and is only worth setting to compare with upstream
  ([Upgrade notes]({{ upgrade_page.url | relative_url }})).
- **Keep the host guide's ports.** Game servers find the metagame through your user `Game.ini`, and
  the client through its first launch argument; both point at `127.0.0.1:61000`. Run only one stack
  that starts game servers on a PC: they also need UDP 8770-8777.

The deploy server's `.env` from [host step 11]({{ host_page.url | relative_url }}#deploy-server) works
unchanged.

### Keys and secrets {#secrets}

**Every value in this table is a secret: never share it, never commit it**, and never paste it into an
issue, a chat or a screenshot. Make fresh ones for development. Never copy them from a real server,
and never reuse development ones on a real server.

| Secret | Used by | How to make it |
|---|---|---|
| `AUTH_SIGNING_PRIVKEY_B64` (with `AUTH_SIGNING_PUBKEY_B64`) | The metagame. The private key signs every 24-hour session token, so anyone who has it can log in as any account. | The command below |
| Game-server key | The metagame (it stores only a SHA-256), the deploy server (`METAGAME_API_KEY`) and every game server's command line | [Host step 9]({{ host_page.url | relative_url }}#metagame): queue it in the database, then restart the metagame |
| Account keys (`UUK_...`) | The account's password: the login, and the `x-undaunted-user-api-key` header | Returned once by `POST /undaunted/api/Register`; the server keeps only a SHA-256 ([host step 10]({{ host_page.url | relative_url }}#admin-account)) |
| `GATEWAY_SECRET` | The metagame and the gateway, the same value in both: 32 to 256 printable characters without spaces | `node -e "console.log(require('crypto').randomBytes(32).toString('hex'))"` |
| `ALLOWLIST_SECRET` | The gateway and the allowlist helper, the same value in both. Use a separate value, not the gateway secret. | The same command |
| Gateway TLS key (`gateway-key.pem`) | The gateway | `make-cert`, [below](#public-mode-locally). Keep it outside the repository. |

The signing key pair, appended to the metagame's `.env` without printing it (the same command as in
[host step 9]({{ host_page.url | relative_url }}#metagame)). Use `Add-Content -Encoding ascii`, not
`>>`: in Windows PowerShell 5.1, `>>` writes UTF-16, which Node can't read as a `.env` file.

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
node -e "const c=require('crypto');const k=c.generateKeyPairSync('rsa',{modulusLength:2048,privateKeyEncoding:{type:'pkcs8',format:'pem'},publicKeyEncoding:{type:'spki',format:'pem'}});console.log('AUTH_SIGNING_PRIVKEY_B64='+Buffer.from(k.privateKey).toString('base64'));console.log('AUTH_SIGNING_PUBKEY_B64='+Buffer.from(k.publicKey).toString('base64'))" | Add-Content .env -Encoding ascii
```

### AUTH_MODE=NONE is for development only {#auth-mode-none}

`AUTH_MODE=NONE` turns off the key check. Use it only on a metagame that listens on `127.0.0.1` on
your own PC, never on a server anyone else can reach.

| With `AUTH_MODE=NONE` | What happens |
|---|---|
| `NODE_ENV` is not `production` | The login (`POST /account/api/oauth/token`) takes whatever the client sends as the account id and signs a token for it. Routes that take an account key, the admin routes included, take an account id (`UID-...`) instead, so anyone who knows an admin's account id is an admin. Register the test account first: routes that look the account up still need it to exist. |
| A request carries a forwarding header (from the gateway or any other proxy) | Refused with 403. |
| `GATEWAY_SECRET` is set | The metagame refuses to start: public mode needs `AUTH_MODE=APIKEY`. |
| `NODE_ENV=production` | `NONE` is ignored. The login then gets no answer at all and the client waits (the log says no login method is configured), and routes that check an account key answer 500. The same happens when `AUTH_MODE` is unset or misspelled. |

With `AUTH_MODE=NONE`, the launch argument `-AUTH_PASSWORD=` of the client carries the account id
instead of the key. Everything else in [host step 13]({{ host_page.url | relative_url }}#launch-the-client)
stays the same.

### Start order {#start-order}

1. **Metagame.** From `legacy/UndauntedMetagame`, `npm run dev`, or `npm run build` and then `npm start`.
   The first start creates the database and runs every migration. Every start registers the keys
   waiting in the database's queue tables. Start it from its own folder (the npm scripts do): the
   migrations are found through the relative path `./src/drizzle`.
2. **Once per database:** the game-server key and a test account
   ([host steps 9 and 10]({{ host_page.url | relative_url }}#metagame)). Restart the metagame after you
   queue the game-server key.
3. **Allowlist helper, then gateway**, for public-mode work only.
4. **Content server**, for the launcher's downloads only.
5. **Deploy server, last.** It starts the Ramsgate game server straight away, and that server loads
   characters from the metagame.
6. **The game**, with the script from [host step 13]({{ host_page.url | relative_url }}#launch-the-client),
   or through the launcher ([below](#launcher-dev)).

Stop in the reverse order: the deploy server first, then any leftover `-server` game process, then the
rest ([host step 15]({{ host_page.url | relative_url }}#stopping)). The server kit uses the same order:
it starts the allowlist helper, the metagame, the content server, the gateway and the deploy server,
and stops them in reverse.

### Public mode on one PC {#public-mode-locally}

The easiest way to try public mode is `Test-Sandbox.ps1` ([above](#kit-tests)): it sets up the
certificate, the secrets, the gateway and the helper on spare ports and removes everything
afterwards. To run it by hand:

1. **Certificate.** In `legacy/UndauntedGateway` (after `npm ci`), run
   `node tools/make-cert.js --host 127.0.0.1 --out C:\dr\dev\tls`. It writes `gateway-cert.pem` and
   `gateway-key.pem` and prints the fingerprint for the invite's `fp=`. The key file is a secret:
   never share it, never commit it.
2. **Gateway and helper.** Copy `legacy\UndauntedGateway\.env.example` to `.env`. It is already set up for
   one PC: `GATEWAY_BIND=127.0.0.1` on port 61443, `ALLOWLIST_DRY_RUN=1`, and
   `ALLOWLIST_ALLOW_PRIVATE=1` (otherwise the helper refuses a player address of `127.0.0.1`). Fill in
   `GATEWAY_CERT`, `GATEWAY_KEY` and both secrets. The helper writes `allowlist-audit.log` and
   `allowlist-state.json` into `legacy\UndauntedGateway\` (both git-ignored) unless you set
   `ALLOWLIST_AUDIT_LOG` and `ALLOWLIST_STATE_FILE`.
   **Never set `ALLOWLIST_DRY_RUN=0` on a development PC:** that changes Windows Firewall for real,
   and the helper then needs administrator rights.
3. **Metagame.** The same `GATEWAY_SECRET`, `AUTH_MODE=APIKEY`, and `QOS_TARGET_URL` on the
   launcher's relay port (`http://127.0.0.1:<relay port>/QoS`). Without `NODE_ENV=production` it
   warns at start; that is only a warning.
4. **Relay port.** While the game runs, the launcher's relay needs `127.0.0.1:61000`, which your
   metagame already uses, so the launcher's PLAY would fail with `relay_port_busy`. Start the launcher
   with `DAUNTLESS_REVIVED_RELAY_PORT` set to another port ([below](#launcher-dev)).
5. **Invite.** Write a v2 invite by hand:
   `dauntless-revived://join?v=2&mode=public&host=127.0.0.1&port=<gateway port>&fp=<fingerprint>&code=<code>&name=Dev`.

Every gateway and helper setting, with its range, is in
[Configuration]({{ config_page.url | relative_url }}). What the gateway lets through is in
[HTTP API]({{ api_page.url | relative_url }}).

### The launcher in development {#launcher-dev}

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedLauncher
npm ci
npm start
```

`npm start` runs Electron Forge: it builds the main process and the preload script with Vite and loads
the page from Vite's development server. A development run differs from an installed launcher:

- DevTools are available.
- It does not register the `dauntless-revived:` link handler, so paste invites on the Play page.
- It never looks for or installs updates.
- It takes the two DLLs from `legacy\UndauntedLauncher\assets\` instead of its installed resources. They are
  checked against the pinned hashes either way.

What does not change: a development run is named after the same `productName` as the installed
launcher, so it uses the same `%APPDATA%\Dauntless Revived Launcher\` folder (settings, saved account
keys, log) as an installed launcher of the same Windows user. Close an installed launcher before
`npm start`. The launcher remembers one server and one game folder: a test server you join in a
development run is also the server the installed launcher opens next time.

**Against your local stack (private mode).** A v1 invite may point at loopback:
`dauntless-revived://join?v=1&host=127.0.0.1&port=61000&code=DEVTEST1&name=Dev`. A v1 host must be a
loopback address, a Tailscale address (100.64.0.0/10) or a `*.ts.net` name, because v1 is plain HTTP.
With `REGISTRATION_MODE=OPEN` the code is not checked, but the invite must still carry one (4 to 64
letters, digits or `-`). Downloads need `CONTENT_PORT` on the metagame and a running content server.
The launcher accepts only a content server whose manifest is identical to the one compiled into it,
and only files that match it.

**`DAUNTLESS_REVIVED_RELAY_PORT`** moves the public-mode relay off 61000, for example when your own
metagame holds that port. Values from 1024 to 65535 are used, anything else is ignored, and the
launcher logs the override. The server's `QOS_TARGET_URL` must use the same port. Friends never set
it.

```powershell
$env:DAUNTLESS_REVIVED_RELAY_PORT = "61100"
npm start
```

Where the launcher keeps its settings, the encrypted account keys and its log is in
[Files and data]({{ files_page.url | relative_url }}). How releases are built and published is in
`legacy/UndauntedLauncher/README.md`; releases are built by GitHub Actions, not on a developer PC.

### The server DLL {#server-dll}

The source is in `native/internal-server/` (upstream's C++ with a generated SDK; open
`UndauntedInternalServer.sln` in Visual Studio 2022 and build `Release|x64`). Nothing in the
repository builds it. The game and every game server run the prebuilt copies in
`legacy/UndauntedLauncher/assets/`, and their SHA-256 hashes are pinned in the launcher
(`src/main/constants.ts`), the server kit (`DauntlessServer.Common.ps1`), the friend kit
(`setup.ps1`, `play.ps1`, `tools/make-friend-kit.ps1`) and the docs. A DLL you build yourself
won't have the pinned hash, so all of them refuse it. Replacing it means changing every pin in one
change. The DLL works only with the 1.4.4 exe
([Game settings]({{ game_settings_page.url | relative_url }})).

---

## What CI checks {#ci}

The workflow `.github/workflows/ci.yml` runs on every push to any branch, on every pull request and by
hand (Actions > **CI** > **Run workflow**). Its jobs run the same commands you can run yourself from
this page:

| Job | What it runs | Runner |
|---|---|---|
| Server packages | `npm ci`, `npm run build` and `npm test` in each of the four packages | Windows, Node 24 |
| Launcher | `npm ci`, `npm run typecheck`, `npm test` and `npm run make`, then `scripts/collect-release.ps1`. The release files (the installer, the Squirrel update files, the zip and `SHA256SUMS.txt`) stay downloadable from the run's **Artifacts** for 7 days (not for pull requests from forks). | Windows, Node 24 |
| Server kit | Every tracked `.ps1` must parse in Windows PowerShell 5.1, PSScriptAnalyzer must find no errors (when the runner has it), then the three kit tests | Windows PowerShell 5.1 |
| Docs | `sync-roadmap.ts` and `build-llms.ts` must change nothing, the site must build with the builder GitHub Pages uses, and the built site must have its main pages | Linux |
| Repository hygiene | `tools/ci/check-repo.ts`: no secrets, keys, databases, logs or game files in the tracked files or in any commit the push or pull request brings, no tracked file that a `.gitignore` rule excludes, the two DLLs match their pins, and the launcher version is valid | Linux |

The content server's integration test is the exception: it needs a real game install, so CI does
not run it. To run the hygiene check yourself before a push:
`bun tools/ci/check-repo.ts --history origin/dauntless-revived..HEAD` (without `--history` it checks
the files only). It never prints what it found inside a file. Its arguments are in
[Scripts and parameters]({{ scripts_page.url | relative_url }}#ci-workflows-and-tools).

Two more checks run outside this workflow. CodeQL scans the code for security problems through the
repository's code scanning default setup, so there is no CodeQL workflow file in the repository.
Dependabot (`.github/dependabot.yml`) proposes dependency updates: for the GitHub Actions weekly, for
each npm package monthly.

### Launcher releases {#launcher-releases}

A launcher release is the GitHub release `launcher-v<version>` for the version in
`legacy/UndauntedLauncher/package.json`, published from `dauntless-revived` only. Installed launchers update
to it. To release, raise that version. Then:

- **Automatically (on by default).** When a push to `dauntless-revived` passes every job above, is
  still the head of the branch, and its launcher version has no `launcher-v<version>` release yet, CI
  publishes the installer it built and tested in that run, through
  `.github/workflows/launcher-release.yml`. This happens only in the repository
  `mixutin/dauntless-revived`, never in a fork. To pause it, set the repository variable
  `LAUNCHER_AUTO_RELEASE` to `false`
  ([Configuration]({{ config_page.url | relative_url }}#ci-settings)).
- **By hand.** Actions > **Launcher release** > **Run workflow** on `dauntless-revived` builds that
  commit and publishes its version. This works while automatic releases are paused too. For a version
  that is published already, it only brings the self-update feed (the `launcher-updates` release) up
  to it.

A version is published only if it is newer than every earlier one, and it is never replaced. A
prerelease version (such as `0.2.0-beta.1`) becomes a GitHub prerelease that installed launchers do
not update to. The details are in the "Checks" section of
[CONTRIBUTING.md]({{ site.github.repository_url }}/blob/dauntless-revived/CONTRIBUTING.md) and in
"Releases and updates" in
[legacy/UndauntedLauncher/README.md]({{ site.github.repository_url }}/blob/dauntless-revived/legacy/UndauntedLauncher/README.md).

---

## Keep generated files current {#generated-files}

| File | Made by | Run it again when |
|---|---|---|
| `docs/roadmap.md` | `bun tools/sync-roadmap.ts` | `ROADMAP.md` changes. Never edit `docs/roadmap.md` by hand. |
| `docs/llms.txt`, `docs/llms-full.txt` | `bun tools/build-llms.ts` | Any English page or the FAQ data changes (`llms-full.txt` holds the text of every English page; `llms.txt` lists every page with its title and description). |
| `legacy/UndauntedMetagame/src/drizzle/*.sql` and `meta/` | `npm run db:generate` in `legacy/UndauntedMetagame` | `src/db/schema.ts` changes. It reads `.env`. Commit the new migration with the schema change. |
| `legacy/UndauntedMetagame/src/vendor/hunt_titles.json` | `node legacy/UndauntedMetagame/scripts/make-hunt-titles.js` | The deploy server's hunt tables in `legacy/UndauntedDeployServer/src/vendor/` change. |
| `legacy/UndauntedContent/data/dauntless-1.4.4.json` | `bun tools/make-game-manifest.ts --zip <game zip>` | Practically never: it describes the pinned 1.4.4 build. The launcher compiles this same file in, so rebuild the launcher afterwards. |
| `legacy/UndauntedLauncher/assets/icon.png`, `icon.ico` and `src/renderer/brand/*.png` | `npm run icon` in `legacy/UndauntedLauncher`, which copies them from `brand/` (`python3 brand/build.py` makes them there) | The launcher icons in `brand/launcher/` or the emblem sizes in `brand/web/` change. The launcher's unit tests fail while a copy is out of date. |

Run the two docs generators with `sync-roadmap.ts` first: `build-llms.ts` reads the roadmap page it
writes. Neither takes arguments; both find the repository root themselves. CI runs both and fails if
that changes any file.
`docs/fi/roadmap.md` is a hand-written Finnish summary, not generated.

```powershell
Set-Location C:\dr\undaunted
bun tools/sync-roadmap.ts
bun tools/build-llms.ts
git status --short docs
```

The arguments of the tools are in [Scripts and parameters]({{ scripts_page.url | relative_url }}).

### Docs pages {#docs-pages}

- Every English page `docs/<path>.md` has a Finnish twin `docs/fi/<path>.md`. Both have the same
  `ref` in their front matter, which pairs them for the language switch; `lang` is `en` or `fi`.
  Finnish pages also set `locale: fi_FI` and sit under "Dauntless Revived suomeksi" in the
  navigation: as their `parent`, or as their `grand_parent` when the page is in a section.
- `description` is what search results and link previews show: keep it to 120-160 characters.
- Link to another page through the page variable at the top of the page, not a hard-coded URL. Finnish
  pages link to the Finnish pages (`fi/...`).

```liquid
{% raw %}{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
[Host a server]({{ host_page.url | relative_url }}){% endraw %}
```

- The FAQ text is in `docs/_data/faq_en.yml` and `docs/_data/faq_fi.yml`, in the fixed format described
  at the top of each file.
- The repository has no Gemfile. CI builds the site like GitHub Pages; a local preview needs your own
  Jekyll setup with the theme and plugins listed in `docs/_config.yml`.

---

## Contribution rules {#contributing}

The short version is in
[CONTRIBUTING.md]({{ site.github.repository_url }}/blob/dauntless-revived/CONTRIBUTING.md).
In practice:

- **Start with a Discussion**, and name the [roadmap]({{ roadmap_page.url | relative_url }}) step if
  there is one.
- **No secrets and no game files in commits.** No account keys, game-server keys, signing keys,
  gateway or allowlist secrets, TLS keys, `.env` files, databases, backups or logs, and nothing from
  the game: executables, paks, assets or its config. A generated `Game.ini` counts too: one of its URLs
  carries a secret webhook path. The `.gitignore` files catch the usual names (`.env`, `*.key`, `*.db`,
  `*.pak`, the gateway's `*.pem`), not everything: look at `git status` before every commit. CI's
  hygiene check refuses such files too, but only after the push. If a secret was committed, say so,
  and replace the key. It stays in the history of every clone.
- **Keep captured bodies out of the tree.** `LOG_BODIES=1` writes `bodies.log` into the folder the
  metagame runs from (`legacy\UndauntedMetagame\` with the npm scripts) unless `BODY_LOG_FILE` says
  otherwise, and `*.log` is not ignored there. Point it outside the repository, for example
  `BODY_LOG_FILE=C:/dr/dev/bodies.log`. The file holds what players sent.
- **Test on a throwaway account.** Never try a new server response on a real player's account first: a
  wrong response shape can crash the 1.4.4 client or damage a save. Capture what the game really sends
  (`LOG_BODIES=1` on your own development server) before you build a response on a guess.
- **Back up before a migration.** The metagame migrates its database at start, with no backup of its
  own.
- **English and Finnish docs change together.** A change that alters behaviour updates the English
  page and its Finnish twin in the same change, then reruns the docs generators.
- **Generic examples only** in docs, tests and comments: addresses from 203.0.113.0/24 and
  198.51.100.0/24, placeholders such as `UUK_...` and `UID-...`, no real keys, addresses, names or
  personal paths.
- **Server kit scripts** run on Windows PowerShell 5.1, are ASCII only, and never print a key, token,
  password or `.env` value. The `.gitattributes` files in `deploy/windows-server/` and `friend-kit/`
  keep them in CRLF line endings.
- **Never log a key or a token.** The logs mask known key and token shapes; don't rely on that.
- **Small pull requests**, with what you changed, how you tested it and the roadmap step.
- **Security problems** go through private vulnerability reporting, not issues
  ([SECURITY.md]({{ site.github.repository_url }}/blob/dauntless-revived/SECURITY.md)).
- **License.** AGPL-3.0-only, like upstream Undaunted. Keep existing copyright and license notices.

---

## Where the code lives {#code-map}

Paths in each table are relative to the folder named above it.

**`legacy/UndauntedMetagame/src/`**

| Subsystem | Code | Notes |
|---|---|---|
| Startup, routing, request log | `server.ts`, `app.ts`, `logger.ts`, `middleware/BodyLog.ts` (`LOG_BODIES`: the routes, redaction, status and duration, the per-path cap) | `app.ts` mounts every router; unknown routes are logged and answered 404. |
| Feature switches | `features.ts` | The switches added with the port of Harmonic's fork, parsed in one place (on/off, counts, choices), with the `features:` boot line. Older switches keep their own parsing. |
| Game-server writes | `middleware/GameServerOnly.ts` | `RefuseUnlessGameserver` (the shared guard of the game-server-only writes) and `NoteRelayedAccountMismatch` (logs a relayed token of another account, never refuses). |
| Database | `db.ts`, `db/schema.ts`, `drizzle/` | Migrations run at every start, in journal order; the applied ones are recorded in the `__drizzle_migrations` table. `DB_WAL` is applied in `db.ts`. |
| Where a request came from | `middleware/RequestOrigin.ts` | Public mode: the gateway secret, the player's address, `AUTH_MODE=NONE` refusals. |
| Keys and login | `controllers/auth.ts`, `controllers/apikeys.ts`, `routes/eos.ts`, `routes/login.ts`, `middleware/Has*.ts` | `eos.ts` holds the token login. |
| Accounts, invites, usernames | `controllers/accounts.ts`, `controllers/login.ts`, `routes/undauntedapi.ts`, `controllers/undauntedapi.ts` | The `/undaunted/api` routes ([HTTP API]({{ api_page.url | relative_url }})). |
| Characters and save history | `controllers/character.ts`, `controllers/savehistory.ts`, `routes/character.ts` | Rollbacks are admin routes in `routes/undauntedapi.ts`. |
| Progression | `controllers/progressionmode.ts` (real or stub, `PROGRESSION_MODE`), `controllers/realprogression.ts` (tracks, objectives, grants and their retry guard, rank confirmations and the optional confirm entitlements, the admin seed, the upgrade notice), `controllers/progressionconfig.ts` (the one loader of the progression config: the bundled `vendor/progression_config.json`, `PROGRESSION_CONFIG_DIR`, `ACTIVE_HUNT_PASS`), `controllers/progressionrank.ts` (rank math), `controllers/progressionevents.ts` (audit rows), `controllers/progression.ts` (encountered content and breadcrumbs), `routes/progression.ts`, `middleware/RealProgressionOnly.ts` | `routes/progression.ts` answers both the real and the stub shapes, and logs requests under `/progression` that no route answers. Tables in migration `0011_real_progression.sql`. |
| Hunt Pass, entitlements, bounties, cooldowns | `routes/system.ts`, `controllers/entitlements.ts`, `controllers/bounties.ts`, `controllers/cooldowns.ts` | Part of real progression. |
| Escalation | `controllers/escalationConfig.ts` (the season registry `vendor/escalation/seasons.json`, checked at load), `controllers/escalation.ts` (reads, the save rules, the audit rows), `routes/escalation.ts` (the stub and the real routes, `ESCALATION_MODE`) | Ported from Harmonic's fork. Tables in migration `0014_escalation.sql`. [Escalation]({{ '/findings/escalation.html' | relative_url }}); tests `test/escalation.test.ts`. |
| Loadouts and loadout slots | `controllers/loadout.ts`, `routes/loadout.ts` | Slots exist for real-progression accounts only. |
| Inventory, currency and store | `controllers/inventory.ts` (transactions, overspends, retries; `ApplyInventoryTransactionInTx` runs inside a caller's transaction, `RunInventoryTransaction` wraps it for `POST /inventory`), `routes/inventory.ts`, `controllers/store.ts` (the currency sheet and the held currencies), `controllers/activecharacter.ts` (the account's character saved last), `controllers/freestore.ts` (the free store: catalogue, tokens, redeem), `routes/store.ts` | The store is ported from Harmonic's fork, off unless `STORE=free`; data in `vendor/store_catalog.json` and `vendor/store_item_kinds.json`, table in migration `0015_store_purchases.sql`. Tests `test/freestore.test.ts`, `test/inventorytx.test.ts`. |
| Parties | `controllers/party.ts` (in memory), `routes/party.ts`, `middleware/PlayerAuth.ts` | A party's hunt goes through `controllers/matchmaking.ts`; `PartyInvite` is in `routes/undauntedapi.ts`. |
| Friends and blocks | `controllers/friends.ts` (stored in SQLite, migration `0012_friends_and_blocks.sql`; `AddFriendshipListener` tells the chat server about accepted and ended friendships), `routes/friends.ts` (the Epic-style friends service the client calls) | `Friends` is in `routes/undauntedapi.ts`. |
| Slayer Links | `controllers/slayerlinks.ts` (the rules, the reply shapes, with the executable's addresses), `routes/slayerlinks.ts` (the eight routes, `SLAYER_LINKS`) | Contract from Harmonic's fork, corrected. Tables in migration `0016_slayer_links.sql`. Tests `test/slayerlinks.test.ts`. |
| Guilds | `controllers/guild.ts` (stored in SQLite, migration `0013_guilds.sql`; the rules and reply shapes), `routes/guild.ts` (the eleven v2 routes, in registration order), `middleware/GameServerKeyAuth.ts` (the game-server-only create) | `GuildInvite`, `Guilds` and `DisbandGuild` are in `routes/undauntedapi.ts`. `test/socialclient.ts` models how the client parses the social replies; `test/guildhttp.test.ts` and `test/socialflow.test.ts` use it. |
| Chat (XMPP) | `realtime/chat.ts` (the WebSocket listener, SASL login with the player token, sessions and their limits, pings, whispers; `ReadChatConfig` and `StartChat`, which `server.ts` calls only with `CHAT=1`), `realtime/muc.ts` (rooms: joins, leaves, the fan-out of room lines, party and guild membership checks, the 60 s sweep), `realtime/chatnick.ts` (the client's URL encoding and the room nickname rules), `realtime/xmpp.ts` (parsing, escaping, JIDs, token buckets, the trace redaction), `realtime/presence.ts` (friends' online status with `CHAT_PRESENCE=1`: who hears whom, the relays, the friends-list push, and the rule that no stanza outside a room reaches a player from their own account) | Membership and blocks come from `GetPartyOf` in `controllers/party.ts`, `IsGuildMember` in `controllers/guild.ts` and `BlockersAmong` in `controllers/friends.ts`. [Text chat]({{ '/findings/chat.html' | relative_url }}) explains why each answer has its shape. Tests: `test/chat.test.ts` (end to end over WebSocket), `test/chatmodel.test.ts` and `test/chatclient.ts` (a model of how the 1.4.4 client reads chat), `test/chathttp.test.ts` with `test/chatenv.ts` (names through the real account routes), `test/chatwire.ts` (a raw client shaped like the game's), `test/presence.test.ts`, and `test/chatinvariant.ts` (checks every stanza of those files against the own-account rule). |
| Matchmaking | `controllers/matchmaking.ts` (queues, candidate status, parties, the call to the deploy server), `routes/matchmaking.ts` | |
| Server status | `controllers/serverstatus.ts`, `middleware/SoftAccountAuth.ts`, `/dauntless-status` in `routes/system.ts` | |

**`legacy/UndauntedDeployServer/src/`**

| Subsystem | Code | Notes |
|---|---|---|
| Game servers | `controllers/gameservers.ts` (ports, Ramsgate and the Dojo with one shared launch per world and the liveness check, starting processes and their `error` and `exit` listeners, the test hooks), `controllers/watchdog.ts`, `routes/gameservers.ts` | Loopback callers only. Tests `test/persistentworlds.test.ts` with `test/deployenv.ts` (a stand-in spawn). |
| Matchmaking | `controllers/matchmaker.ts`, `controllers/matchmakinginput.ts` (a second check of everything that ends up on a game server's command line), `routes/matchmaker.ts`, `vendor/*_table.json` | |

**`legacy/UndauntedGateway/`**

| Subsystem | Code | Notes |
|---|---|---|
| The gateway relay | `src/gateway.ts` (the TLS server and the relay to the `127.0.0.1` upstreams, WebSocket upgrades included), `src/policy.ts` (what each request may do), `src/ratelimit.ts`, `src/ip.ts` (address parsing and ranges), `src/server.ts`, `src/config.ts` | |
| Access log | `src/log.ts`, `src/redact.ts` | Paths are scrubbed of tokens and keys. |
| Allowlist feed and helper | `src/feed.ts` (tells the helper who logged in), `src/allowlist/` (`server.ts`, `config.ts`, `helper.ts`, `state.ts`, `sync.ts`, `firewall.ts`) | |
| Certificate tool | `tools/make-cert.js` | |

**`legacy/UndauntedContent/`**: `src/server.ts` and `src/config.ts` (start and settings), `src/app.ts`
(routes), `src/auth.ts` (key check against the metagame),
`src/paths.ts`, `src/range.ts`, `src/limits.ts`, `src/gamedir.ts`, `src/manifest.ts`,
`src/branding.ts`, `src/news.ts`, `src/verify.ts` (`npm run verify`), and the manifest in
`data/dauntless-1.4.4.json`.

**`legacy/UndauntedLauncher/src/`**

| Subsystem | Code | Notes |
|---|---|---|
| The downloader | `main/downloader.ts` (resumable, `.part` files, four at a time), `main/verify.ts` (size first, then SHA-256), `main/manifest.ts` and `main/game-manifest.ts` (the compiled-in manifest) | |
| The local relay | `main/relay.ts` | Public mode: `127.0.0.1:61000` to the gateway over pinned TLS, WebSocket included, while the game runs. |
| Connections to the host | `main/http.ts` (requests to the invite's host only: plain HTTP to Tailscale or loopback, or pinned TLS to a gateway), `main/pinned.ts` (pinned TLS), `main/hostapi.ts` (metagame and content calls) | |
| State and flow | `main/controller.ts` | Imports nothing from Electron, so the tests drive it with plain Node. |
| Keys, settings, game setup | `main/keystore.ts` (Windows DPAPI), `main/settings.ts`, `main/engineini.ts`, `main/launch.ts`, `main/dlls.ts`, `main/constants.ts` (the DLL pins and the update feed) | |
| Invites and UI text | `shared/invite.ts`, `shared/i18n/` (every UI text, one file per language; `README.md` there says how to add a language) | |
| Window and page | `main.ts`, `preload.ts`, `renderer/` | |

**Elsewhere**

| Subsystem | Code |
|---|---|
| Server DLL | `native/internal-server/dllmain.cpp` (the endpoint table, client and server mode), `constants.h` |
| Server kit | `deploy/windows-server/*.ps1`; shared pins, paths, the component table and the start order in `DauntlessServer.Common.ps1`; database, key and game-folder helpers in `lib/` |
| Friend kit | `friend-kit/`, zipped by `tools/make-friend-kit.ps1` |
| Docs tools | `tools/sync-roadmap.ts`, `tools/build-llms.ts` |
| CI | `.github/workflows/ci.yml`, `.github/workflows/launcher-release.yml`, `.github/dependabot.yml`, `tools/ci/`, `legacy/UndauntedLauncher/scripts/collect-release.ps1` |
