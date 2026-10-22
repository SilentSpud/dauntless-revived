# Migration to-do: manual checks and later work

What the automated tests cannot cover: the plan's manual checks and the "not done" notes in
[MIGRATION_PLAN.md](MIGRATION_PLAN.md) (row ids in brackets), and the parts the ports could only test against
stand-ins. Then the things to revisit once the migration is done. Tick an item when it is done and note where
(server, OS, date).

## Manual checks

### Every deploy step

- [ ] The smoke list passes: register, login, character load/save, inventory, store purchase, store image,
      matchmaking, party, friends, guild, chat connect + room message, backup endpoint, clean shutdown. [M38,
      M42]
- [ ] Offline saves exported before the migration import after it. [section 6]

### Launcher (Part A)

- [ ] Update bridge on a clean VM: install 0.1.39 → bridge → Tauri, keys and settings intact; then one Tauri →
      Tauri update. [L3]
- [ ] The same against the real bridge release: a 0.1.39 install reaches the newest 0.2.x in two hops. [L50]
- [ ] Window behaviour on Windows and Linux. [L5]
- [ ] Dialogs, in two languages. [L36]
- [ ] Old account keys read back: real 0.1.39 profiles on Windows, gnome-keyring and KWallet (KWallet and a
      real Linux profile are still missing). [LB1, L27]
- [ ] Windows package: install, update and uninstall on a clean VM. [L45]
- [ ] Packaged offline smoke run against the `--service` modes. [L44]
- [ ] Offline start/stop, save export/import and key registration on both OSes, once offline mode returns.
      [X2]
- [ ] Beta checklist per OS, and a week with no key, save or language regressions before cutover. [L49]
- [ ] The Tauri launcher with a real profile: join, register, install, play, offline save backup and restore.
      The debug build was checked only with a throwaway profile. [L38, L43]
- [ ] Linux without a Secret Service: the packaged launcher installs gnome-keyring and libsecret once and starts
      again. [X3]
- [ ] A Tauri to Tauri update on Windows and on the AppImage, and an update with a wrong signature refused. [L46]
- [ ] deb and rpm install on Ubuntu 22.04/24.04, Debian 12 and Fedora; the AppImage on Arch and SteamOS. [L47]
- [ ] The first run of `launcher-tauri-release.yml` (the beta): files, checksums, attestations. [L48]

### Metagame (Part B)

- [ ] Traffic capture recorded from the live servers. [M2]
- [ ] Shadow replay of that corpus against Node and Rust: no unexplained diffs. [M35]
- [ ] Performance under the same load on the canary and on a Windows host like production. [M36]
- [ ] The 24-hour fuzz run before cutover. [M33]
- [ ] Production cutover in order: the canary, then the first Windows region, then the second. [M42]
- [ ] Rollback rehearsal on the canary: back to Node, smoke list, restore the backup, forward to Rust, smoke list,
      `metagame db integrity`. [M46]
- [ ] A fresh install on the canary that follows only the rewritten docs. [M47]

### Gateway and allowlist helper (S2, S3)

- [ ] A 0.1.x and a 0.2.x launcher both connect through the Rust gateway. [S2]
- [ ] Existing invites still work: the gateway loads the old certificate, so `fp` is unchanged. [S2]
- [ ] The firewall rule changes correctly after login and logout on a real Windows server (administrator
      rights) and on a real Linux server (nft). [S3]
- [ ] On Windows, the helper takes over the rule the Node helper created instead of adding a second one. [S3]

### Content server (S1)

- [ ] The launcher's downloader tests run against the Rust content server. [S1, L23]
- [ ] A full game download and a repair from a real server. [S1]

### DeployServer (S4)

- [ ] A run against the real game binary: Ramsgate, the Training Dojo, a city and a hunt start, become ready,
      and stop with the server. [S4]
- [ ] Overflow and regional workers launch on a real second machine. [S4]
- [ ] A canary soak before offline mode switches to the Rust DeployServer. [X2]

### Discord key bot (S5)

- [ ] A real `/key claim` on a test guild (needs a bot token): the DM arrives and the state file records it.
      [S5]
- [ ] `/key status`, `/key link`, `/key resend` and `/key recover` on the test guild. [S5]
- [ ] `/counter` sets up the category, the channel names update after members join and leave, and the
      channels stay locked. [S5]
- [ ] The bot's own gateway client in real use: it reconnects and resumes after a network drop, and the
      two-minute check restarts it if it stays disconnected. [S5]
- [ ] Without the Server Members intent the key commands still work, and the bot restarts within a minute once
      the intent is turned on. [S5]
- [ ] The existing state file from production loads unchanged. [S5]

### Kits and server tools (S6, M40, T7, T8)

- [ ] The kits' install scripts and service files start the Rust binaries instead of the Node tools, on Windows
      and Linux. [S6, B5]
- [ ] The two-machine worker launch (`dr-ssh-game`). [S6]
- [ ] `tests/Test-Sandbox.ps1` for the Windows kit, including stack and launcher start/stop. [M40]
- [ ] One real Discord status post to a test channel. [T7]
- [ ] The owner dashboard and Discord status installs: the tasks start, and an upgrade from a Node install leaves
      no Node task behind. [T8]
- [ ] Each component's security checklist in `SECURITY_REVIEW.md` ticked and signed in its cutover PR, after a
      24-hour run of its fuzz targets with no crash. [X9]

### Tools (Part E)

- [ ] Regenerate the game manifest from the real 1.4.4 zip with `bun tools/make-game-manifest.ts --zip
      <BaseGame144.zip>`: `git diff` stays empty. [T5]
- [ ] On a Windows server that runs the Node dashboard and Discord status worker: run
      `Install-OwnerDashboard.ps1` and `Install-DiscordStatus.ps1` from the updated kit. Bun lands in
      `C:\Program Files\Bun`, both tasks run `bun.exe`, the old `.mjs` copies are gone, the page lists the fleet and
      the Discord message keeps updating (the same message, not a new one). [T6, T8]
- [ ] `worker-health.ts` on a hunt worker through its tunnel: the Server #2 tab fills in. [T6]

## Later work

- **SolidJS page (P10, L52 to L59).** Waits for another maintainer's rework of the launcher UI, then is done
  against the reworked page rather than today's `index.ts`. The rows in the plan stay the method: one Solid
  store fed by `api.onSnapshot`, one component per page, `t()` on a language signal, listeners in
  `onMount`/`onCleanup`, the scene kept imperative, Vitest component tests, then `dom.ts` deleted. Nothing in
  the Tauri port depends on it: `src/renderer/backend.ts` and `src/shared/art.ts` are the page's only Tauri
  pieces, so the rework keeps working with both launchers.
- **Serenity.** The bot dropped serenity because 0.12.5, its latest release, pins rustls 0.22, whose webpki has
  open advisories, so cargo-deny fails. Serenity's `next` branch already uses rustls 0.23 and has a
  `rustls_backend_no_provider` feature (so aws-lc-rs works), but it is unreleased, would need a git dependency
  that `deny.toml` refuses, and its API is a breaking change. When it ships on crates.io (likely 0.13), consider
  switching back: only `discord.rs`, `gateway.rs` and the dispatch part of `bot.rs` would change, and the bot
  would get per-route rate limits instead of its retry on 429.
- **Gateway parser errors.** Requests the HTTP parser rejects get hyper's own 400 or 431 with an empty body,
  not Node's JSON body. Nothing is known to depend on it; recheck if a client does. [S2]
- **Linux allowlist helper behaviour.** It now needs `ALLOWLIST_DRY_RUN` set, answers 200 before the rule changes
  and retries instead of answering 503. Confirm operators and kits expect this. [S3]
- **Node `fetch` keep-alive delay.** On Windows, Node's `fetch` takes about half a second to reuse an idle
  connection. This no longer matters once the Node tools are retired.
- **OpenBSD hosts** move to the Linux kit. [D14]
- **0.1.x installs** that skip the bridge before 0.3.0 must reinstall by hand; keep the README note. [D2]
- **Containers** for the game server (Docker/Podman with Proton/Wine) stay outside this plan. [D24]
