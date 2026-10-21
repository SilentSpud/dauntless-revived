<!-- markdownlint-configure-file { "MD013": { "line_length": 120, "tables": false } } -->

# Dauntless Revived master migration plan: one Rust codebase

Status: in progress. Written 2026-10-20. The spikes that could run on a development PC ran the same day; their results
are in section 3.4.

Done so far:

- Repository: R1, R2, R3, R4 with M4 (the workspace and `crates/protocol`, starting with the invite types and
  builder), R8 and the `cargo` entry of X6. Branch protection still has to be switched to require only `CI ok` (R8).
  The Node projects moved into `legacy/` (section 2); the server kits still install them at the top level.
- Metagame B0: M3 (`legacy/UndauntedMetagame/routes.json`, checked by `test/routes.test.ts`).
- Metagame B1: the `store` crate with M13 (pools, `read` / `write` closures), M14 (`0001_baseline` and Drizzle
  adoption), M16 and M18 (the snapshot is the baseline's `up.sql`); `rust/services/metagame` holds the schema. The
  bundled SQLite is built with the better-sqlite3 compile options that change behaviour (`SQLITE_DQS=0` and others,
  `rust/.cargo/config.toml`), and `schema.rs` maps every `INTEGER` to `BigInt` (patch in `diesel.toml`): SQLite
  integers are 64-bit, and `print-schema` would read them as `i32`.
- Metagame B1: `service-kit` with M6 (an ordered, case-insensitive matcher in front of axum handlers, since axum
  prefers static segments whatever the order), M7 (`application/json; charset=utf-8`, empty 404), M24 (pino JSON
  lines; `fatal!` for level 60), M25 (`Env` from `--env-file` only, process environment wins) and the serving half
  of M5, M10 and X7 (`ConnectionInfo` with both addresses; stop on `shutdown` on stdin, Ctrl+C/Ctrl+Break or
  SIGTERM, then a drain deadline).
- Metagame B1: M11 in `service_kit::ratelimit`, `express-rate-limit` 8.7 rebuilt: per-key windows from the first
  request, draft-8 `RateLimit` and `RateLimit-Policy` headers with its partition key, 429 with `Retry-After` and the
  JSON reply, skip-successful, IPv6 keys per /56; `Router::layer` puts it (or any middleware) on one route.
- Metagame B1: M8 in `service_kit::body`: JSON and form bodies parsed for every request before routing, as Express's
  app-wide parsers do, with body-parser's strict JSON, `qs`-style flat forms, gzip/deflate, the 50 MB limit and
  Express's production error page; a hook lets the metagame answer `JSON_ERROR_ROUTES` its own way. Not supported:
  `br` request bodies and charsets other than UTF-8 (415), nested `qs` keys.
- Metagame B1: M17, `Store::backup`: a read-only `rusqlite` connection copies one snapshot (a read right after
  `BEGIN`, or each step reads anew and the backup restarts after every save: in the test it never finished under
  constant writes, and with the snapshot it takes under a second), checks a deadline between steps, never
  overwrites, and leaves a single-file copy in rollback-journal mode.
- Metagame B1: M19, `metagame db integrity|backup|add-invite|del-invite|make-admin|gs-key|journal delete|wal <db> …`
  with dr-db's checks and messages (`metagame db:` instead of `dr-db:`), never migrating or creating the file; the
  kits switch to it in M39-M41.
- X1 (offline saves' database half): `metagame::offline_save` is `offline-save-db.cjs`, and
  `metagame offline-save export|import` offers it on the command line (the key comes on stdin, the answer is JSON).
  - Export copies a stopped offline database with `store::backup` and changes only the copy. It gets a new random
    `UUK_` key, the six credential tables are emptied, the account's token epoch goes up, and `secure_delete` plus
    `VACUUM` rewrite it. It refuses `-wal`, `-shm` and `-journal` files left beside the database.
  - Import checks a private copy. Its bookkeeping tables must be the ones Drizzle and Diesel make. Its history must
    be a nonempty prefix of the frozen Drizzle migrations (`metagame::drizzle_frozen`, embedded, checked against
    `legacy/UndauntedMetagame/src/drizzle` by a test) or of the Diesel migrations, with the schema that prefix makes.
    The remaining Drizzle SQL is replayed, then `store::migrate` adopts the baseline and runs newer Diesel
    migrations; `store::migrate` is new, the same as `Store::open` but without switching the file to WAL. The
    result must have the current schema, pass `integrity_check`, stay in rollback-journal mode, and hold exactly one
    account for the key (the declared one, with no credentials left).
  - Tests: 5 cases (round trip, an older launcher's database, a forged hash, an extra table, a future migration, no
    history, leftover credentials, journal files beside the database, a missing file).
  - Checked by hand: a save exported by the Node helper itself (`offline-save-fixture.cjs`) and a 20-migration
    offline database both import with the Rust CLI, keeping their characters and inventories.
- Metagame B1: M9 and the rest of M10 in `metagame::auth` and `metagame::origin`: `MetagameAuth`, `Player`,
  `SoftPlayer`, `GameServer`, `AdminKey`, `UserKey` and `SoftAccount` extractors with the Node middleware's refusals
  (tested with the permissions tests' cases), token epoch and ban checks, the gateway/proxy/local-caller rules, the
  ban check of `CheckPlayerAccess` and the recovery copy of player keys. The app-wide layers (backend health, ban
  check, dev-auth refusal, request log) come with the app assembly.
- Metagame B1: M27 in `metagame::health` (the request window and histogram as in Node; `eventLoop` measured as
  the lateness of a 20 ms timer on the async runtime, which is what Node measures; `memoryMB.heapUsed` is `null`, as
  there is no GC heap) and M28 (`App::key_revocations`, a broadcast channel). `service_kit::json::js_number` writes
  floats as `JSON.stringify` does (`1`, not `1.0`).
- Metagame B1: M12 (`metagame::storeimages`, `tower-http` `ServeFile` behind Node's name check), M26
  (`metagame::progression_config` with Node's checks and messages; the vendor files built in by `metagame::vendor`,
  typed per area in M29) and the metagame's limiter set (`metagame::limits`; the per-account `PlayerMutation` limit is
  applied with its routes in M29). Left of B1: M15, which belongs to each area's port.
- Metagame B1: M20-M23 in `metagame::crypto`, tested against tokens and a recovery blob made by the Node
  metagame's own libraries (`tests/fixtures/make-node-crypto.cjs`): Node tokens verify; expired, wrong issuer or
  audience, `alg: none`, HS256 with the public key and bad signatures are refused.
- Metagame B2, area 1 of M29 (accounts, auth, login, API keys): every route of `routes/login.ts` and `routes/eos.ts`
  and the account routes of `/undaunted/api` (register, invites, registration mode, rename, user info, JWTs), the
  keys left by the host scripts registered at start, and `metagame serve --env-file` with the app-wide middleware in
  `app.ts`'s order. Checked by a parity test: `tests/parity/<area>.requests.json` is run against the Node
  metagame by `record.cjs` and against Rust by `tests/parity.rs`, and the 87 replies (status, headers, body) are
  identical; each later area adds its own script. The route table is checked against `routes.json`, and a test runs
  the binary through start, a request and `shutdown`. Deliberate differences: `AUTH_MODE`, `DB_FILENAME` and valid
  signing keys are required at start (Node started and then hung or failed every login), and the feature switches
  are read once at start. `LOG_BODIES` came later, with M31.
- Metagame B2, area 2 of M29, first part: characters (list, create, save with the version check and the
  `SERIE_dojo` trigger), the active character, save history with its retention and both admin rollbacks, and
  entitlements (the real-mode routes, the stub replies and fall-throughs of stub mode, and the admin grant and
  revoke), with the progression-mode and audit-row plumbing the real-mode routes share. Two parity scripts
  (`characters`, `characters-stub`, 77 replies) match Node. Stored and echoed JSON is rewritten as
  `JSON.stringify` writes it (`service_kit::jsjson`) and names sort in ICU's `localeCompare` order
  (`service_kit::collate`). Loadouts followed: the default loadout, real-mode slots (unlock, active slot, slot
  count) and slot saves, and stub mode's slot 0 through SQLite's own `json_set` with its 404 fall-throughs, in two
  more parity scripts (46 replies). Inventory closed the area: the read, the transaction (stacks with removals
  that spend the same transaction's additions, instanced items with their version rules, Middleman tokens, the
  daily token top-ups, retries answered from the stored reply), the single instanced item update, the unlocker
  check that bans a known editor's writes, the per-account `PlayerMutation` limit and the one-time daily token
  job at start. Node's request objects are shared with the stored stacks and the reply, so stacks live in an
  arena of indexes to give the same sharing. Two parity scripts (`inventory`, `inventory-switches` with every
  switch off its default, 78 replies) match Node.
- Metagame B2, area 3 of M29: the store. Balance sheets (with the held currencies and the Rams pilot's Event01
  alias), the creator code, store offer pictures, and the purchase service of the free webstore, Lady Luck and the
  Middleman: offer lists and single offers with `remaining`, Rams pricing and the weekly shop, purchase tokens
  (the per-account window, the hourly sweep and the one at start) and redeems with their refusals and replays,
  plus the part of Trials the store needs (settling finished weeks). Offers stay JSON objects in the catalogue's
  key order, since a token is bound to the hash of the offer's JSON text. The maintenance script
  `revoke-free-store.cjs` is `metagame db revoke-free-store <db> <cutoff> [--apply]`. Three parity scripts
  (`store`, `store-off`, `store-rams`, 75 replies) match Node; the harness gained `capture` / `{{saved:…}}` to
  carry a purchase token into the next request. What depends on the week (the Middleman's cells, the weekly
  shop) is checked against Node's output at fixed times instead (`tests/fixtures/store-weekly.json`).
- Metagame B2, area 4 of M29, first part: `routes/progression.ts`. Real-mode tracks and objectives (reads, the
  grant with its per-request cap and its retry guard, grants in one track, rank confirms clamped to the earned
  rank with the premium gate and the optional rank entitlements, resets by an admin key or an allowed game
  server), upstream's stub for stub-mode accounts with the configured max ranks, encountered content and
  breadcrumbs, the "unhandled progression request" note before the 404, and the start notice about accounts
  without stored progression. The seed (`SeedProgression`) is in place for the admin route. Two parity scripts
  (`progression`, `progression-stub`, 77 replies) match Node. Deliberate difference: a breadcrumbs
  `updateVersion` that is neither an integer nor a string of digits is refused (500) instead of stored as sent.
- Metagame B2, area 4 of M29, second part: from `routes/system.ts`, the progression config, Hunt Pass
  selection, cooldowns (with the 24-hour pruning of gatherable cooldowns) and the bounty board (partial
  upserts by bounty id, slot replacement, draft data, deletes), each with upstream's stub for stub-mode
  accounts; and the admin `/undaunted/api/Progression` and `/undaunted/api/SeedProgression`. Two parity
  scripts (`progression-extras`, `progression-extras-stub`, 52 replies) match Node.
- Metagame B2, area 4 of M29, third part: Trials. The weekly rotation, game-server run submissions with
  their Mark rewards (receipted as cooldowns), the five leaderboard routes with leaderboard profiles
  (`/profile/update`), settling finished weeks before each read, and `routes/tuning.ts` (the Trials
  schedule behind `TRIALS_SCHEDULE`, the Hunt Pass XP config). The rotation, the week windows and the
  schedule are checked against Node at 424 fixed weeks and five fixed times (`tests/trials.rs`,
  `fixtures/make-trials-rotation.cjs`). Two parity scripts (`trials`, `trials-switches`, 56 replies) match
  Node; the harness gained `{{trial:…}}` and `{{week:…}}` for this week's Trial, and normalizes what
  depends on the week. Area 4 is done except CapacityTurns, which is in-memory and comes with matchmaking.
- Metagame B2, area 5 of M29, first part: friends (`routes/friends.ts`). The friends list and blocklist
  with their limits and the in-memory request window, blocks that drop the friendship, the guild invites
  and the Slayer Link invites between the two, and the static reads behind `MISC_ROUTES`. Two parity
  scripts (`friends`, `friends-misc-off`, 64 replies) match Node. Not yet: the friendship events chat
  presence listens to (comes with chat).
- Metagame B2, area 5 of M29, second part: parties (`routes/party.ts`). In-memory parties and invites
  with their limits, the decline cooldown, sweeps of silent members and idle parties, leader eviction,
  `PARTY_SOLO_STUB`, and a block dropping the party invites between the two. The party candidate is
  modelled for matchmaking, which fills it in its part. Two parity scripts (`party`, `party-switches`,
  55 replies) match Node; `tests/party.rs` covers the rules that wait on time.
- Metagame B2, area 5 of M29, third part: matchmaking (`routes/matchmaking.ts`, `matchmakingroster`,
  CapacityTurns, the hunt region reads). Hunt queues per region, private matches, direct starts (Ramsgate,
  the Dojo, the tutorial), capacity waits retried by status polls, party candidates started by the leader
  and joined by id, the cancels behind `MATCHMAKING_CANCEL`, the game servers' expected-player check, and
  the deploy server over hyper with one deadline per exchange. The state shares the party lock, so a step
  Node ran between two awaits still runs whole. Three parity scripts (`matchmaking`,
  `matchmaking-switches`, `matchmaking-unsupported`, 72 replies) match Node, including every request the
  deploy server got: both harnesses now run a scripted fake deploy server. The `/undaunted/api/HuntRegion`
  routes come with undauntedapi.
- Metagame B2, area 5 of M29, fourth part: guilds (`routes/guild.ts`). Names and nameplates with their
  denylists and reserved words, the game server's create tied to the leader's own validate (or recent
  activity behind `GUILD_CREATE_ACTIVITY_FALLBACK`), invites with their limits and the decline pause,
  ranks, hand-overs, leaving, kicks and disbanding, and the `GUILDS=0` stubs. Three parity scripts
  (`guild`, `guild-off`, `guild-switches`, 110 replies) match Node; the harnesses gained `capturePath`
  for values below the reply's root. Area 5 is done; the guild host fallbacks (`ListGuilds`,
  `DisbandGuildAsAdmin`) and the chat room check come with undauntedapi and chat.
- Metagame B2, area 6 of M29, first part: key recovery (`routes/keyrotation.ts`, `discordlinks`). The
  Discord bot's link proof, challenge and confirmation behind `KEY_RECOVERY_SERVICE_SECRET`, with the
  per-id limit, the auth epoch bump and the chat revocation. Two parity scripts (`keyrotation`,
  `keyrotation-off`, 26 replies) match Node; both harnesses normalize recovered 64-digit keys.
- Metagame B2, area 6 of M29, second part: escalation (`routes/escalation.ts`). The season registry,
  `ESCALATION_MODE` and `ESCALATION_STRICT`, real-mode reads and the world server's saves with the hard
  and soft rules, versions, replays and the audit rows. Four parity scripts (`escalation`,
  `escalation-strict`, `escalation-stub`, `escalation-mixed`, 63 replies) match Node, including the
  stored rows: the harnesses gained `finalQueries`, SQL whose one JSON column is compared after the
  requests.
- Metagame B2, area 6 of M29, third part: Slayer Links (`routes/slayerlinks.ts`). Invites, answers,
  links, availability, the deletes, expiry and sweeps, and the invite window, behind `SLAYER_LINKS`. Two
  parity scripts (`slayerlinks`, `slayerlinks-off`, 87 replies) match Node, stored rows included.
- Metagame B2, area 7 of M29, first part: the rest of `routes/system.ts`. The status banner with the
  server's identity (`SERVER_NAME`, `SERVER_VERSION`, `GIT_COMMIT`, `SOURCE_URL`, `STATUS_EXTRA`),
  heartbeats (player activity and playing addresses, kept in memory), and the stubbed client routes.
  The server status builder (`serverstatus.rs`) is ported with it, for `ServerStatus` in the
  undauntedapi part. Without `SERVER_VERSION`, Rust reports its crate version where Node read
  `package.json`. Two parity scripts (`system`, `system-plain`, 25 replies) match Node. The router now
  takes Node's trailing-slash patterns (`/motd/`).
- Metagame B2, area 7 of M29, second part: POST /internal/backup (`routes/backup.ts`) behind
  `BACKUP_ROOT`, on the store's online backup. Two parity scripts (`backup`, `backup-off`, 15 replies)
  match Node's refusals, name checks and host-wide budget; a Rust test covers a full copy.
- Metagame B2, area 7 of M29, third part: the host and launcher routes of `routes/undauntedapi.ts`
  that are not about accounts or save history. Hunt regions, Discord links and key statistics
  (`DISCORD_KEY_STATE_FILE`), backend health, the dashboard's account directory
  (`DEVELOPER_ACCOUNT_IDS`), playing addresses, online stats and `ServerStatus` with its game servers
  and 5 s caches. Two parity scripts (`undaunted`, `undaunted-plain`, 55 replies) match Node, stored
  rows included.
- Metagame B2, area 7 of M29, fourth part: device signals (`DEVICE_SIGNAL_HMAC_KEY`), moderation
  (bans by account, playing address and device, with durations), the moderation log and account key
  recovery (`/undaunted/api/AccountRecovery/:userId`). Two parity scripts (`moderation`,
  `moderation-off`, 75 replies) match Node, stored rows included.
- Metagame B2, area 7 of M29, fifth part: the by-name fallbacks of `routes/undauntedapi.ts`
  (`PartyInvite`, `Friends`, `GuildInvite`, `DisbandGuild`, `Guilds`). Two parity scripts (`byname`,
  `byname-guilds-off`, 43 replies) match Node, stored rows included. With this, every route in
  `routes.json` (175 and the `/progression` fallback) has its Rust port, in Node's order. Left of B2:
  M30 and M31 (the TS tests against the Rust binary).
- Metagame B0, M2: traffic capture in the Node services, which are what runs live until the cutover.
  `CAPTURE_DIR` (off unless set), `CAPTURE_RETENTION_DAYS` (14) and `CAPTURE_MAX_BODY_BYTES` (256 KB). The
  gateway records every request and answer; the metagame records the requests that did not pass the
  gateway (game servers, private-mode players) and every chat stanza in order with the session's open
  and close. One JSON line each, one private file per source and UTC day, files past the retention
  deleted. `capture.ts` is the same file in both packages (a test checks it): a login token becomes
  `<token:UID>` (its subject, read without the signature), an account key `<key:sha256>` (the hash the
  database already stores), a SASL login `<plain:UID>`, the game-server key `<gskey>`, other secret
  fields and long secrets `<redacted>`, and each service scrubs its own secrets from every line; so the
  replay can sign fresh tokens and swap in its own keys from the same database snapshot. The privacy
  section of `docs/legal.md` (and the Finnish copy) says recording may run and what it keeps; the
  settings are in the configuration reference. Left: enabling it on the live servers, which needs a
  release of the Node services with it.
- Launcher P1, first part (L4, L5, L8, L10): `legacy/UndauntedLauncher/src-tauri/` (crate `launcher`, Tauri
  2.12) joined to the workspace with `package.workspace` (R5); `npm run tauri dev|build`, with the page built by
  the same Vite config in mode `tauri` (`page:dev`, `page:build` into `dist-page/`), which drops the CSP meta tag
  there only, so the Electron page keeps it until L51. The window is made in Rust: frameless, hidden until the
  page loads, sized to the primary screen's work area as today, `window-state` pushed on resize; navigation
  only to the app's own origin (WebView2 `http://tauri.localhost`, WebKitGTK `tauri://localhost`, or the dev
  server), new windows refused, no `devtools` feature. The CSP is in `tauri.conf.json`; the capability grants
  only `core:event:default` and the window calls. cargo-deny ignores RUSTSEC-2024-0370 (`proc-macro-error`,
  reached through gtk-rs GTK3 on Linux). CI: the Rust job installs the WebKitGTK build packages; a new
  `launcher-tauri` job builds the debug app on Windows and Ubuntu 22.04. Left in P1: the bridge, types and
  isolation hook (L6, L7, L9); the close-while-playing confirmation (L5) needs the controller (L38).
- Launcher P1, second part (L6, L7, L9): the bridge. `src-tauri/src/bridge/types.rs` mirrors `shared/types.ts`
  (the status types moved to `crates/protocol`, with a `ts` feature); `spec.rs` is the fixed command list with
  each argument's shape and limit. A test generates, and fails when stale, `src/shared/generated/types.ts` (ts-rs,
  plus `CommandName`), `isolation/commands.js` and `permissions/app-commands.toml`; `build.rs` gives each command
  its ACL permission and the capability grants only that set; `test/generated-types.test.ts` fails to compile if
  a generated type and the hand-written one differ. Command names are the IPC names without `dr:` in snake_case;
  the window controls use Tauri's window calls; pushes are the events `snapshot`, `invite-link`, `window-state`,
  `offline-status-changed`, and progress goes over a `Channel` throttled to about 10/s in Rust.
  `src/renderer/api.ts` implements `LauncherApi` with a deadline on every call (an hour for file dialogs);
  `tauri-bridge.ts` sets `window.launcher` from it in the Tauri build only, until L40. The isolation hook allows
  only listed commands and Tauri's event and window calls, with exact argument names, types and lengths.
  Checked in the running app (WebView2, debug build, over the DevTools protocol): listed calls answer; an
  `fs` plugin call, a 1 MB username and an extra argument never settle; no CSP meta tag; `window.open` is
  refused. The backend behind the commands is a stub until P2-P5: actions answer `unknown` with "not in the
  Tauri launcher yet", offline answers `offline_unavailable` (L42). P1 is done apart from the close-while-playing
  confirmation (L5, with the controller) and the manual checks on Linux.
- Launcher P2, L11 and L12: `src-tauri/src/core/` holds `invite.rs` (`parseInvite`), `status.rs`
  (`parseServerStatus`, with the status types in `crates/protocol`) and `username.rs` (`checkUsername`,
  `isPlausibleAccountKey`, `extractAccountKey`), on JavaScript's string rules in `text.rs` (UTF-16 lengths,
  ECMAScript `trim` and `\s`, `decodeURIComponent`'s strict UTF-8, WHATWG URLs, the ECMAScript date-time format).
  The tables are in `test/cases/invite.json` (38 accepted, 121 refused, including where Rust and JavaScript string
  handling differ), `status.json` (68) and `username.json` (48), read by `cargo test` and by the node tests; they
  were made from the TypeScript functions' own answers. Differences kept on purpose: `startedAt` in a format other
  than ECMAScript's (which a browser's own date parser might still read) and years past ±262143 are refused. The
  invite, register and key commands check with these now.
- Launcher P2, L13 and L15: `src/shared/i18n.ts` is now `src/shared/i18n/<lang>.ts`, moved verbatim, with
  `index.ts` holding the registry that `Language` and `LANGUAGES` (re-exported by `types.ts`) come from;
  `STRINGS` is byte-identical before and after (287,444 bytes of JSON). `src/shared/i18n/README.md` is the
  checklist for a new language: the file and one registry line. The game's text languages stay a fixed list.
- Launcher P2, L14: `test/strings-export.test.ts` writes the page's `STRINGS` to `src-tauri/gen/strings.json`
  (and fails when it is stale); `build.rs` makes the `Language` enum (also the bridge's, so its TypeScript type)
  and a sorted lookup table per language from it. `core/i18n.rs` has `translate` and `systemLanguage` as the page
  has them, the 8 keys the backend shows (close confirmation, folder dialogs) with a test that each exists in
  every language, and the first start's language from `sys-locale`.
- Launcher P2, L16 to L18 (P2 done): `core/game_language.rs` (`gameLocale` and the `-epiclocale` guard, with
  `launch.ts`'s cases); `core/pins.rs` (constants.ts's pins and limits) and `core/manifest_check.rs`
  (`validateManifest`, `compareManifests`, `manifestFingerprint`, `isSafeRelativePath`, same reasons), both
  also `include!`d by `build.rs`, which stops the build when a DLL in `assets/` is not the pinned one or the
  compiled-in manifest is malformed, and writes its fingerprint as a constant; `core/manifest.rs` holds the
  manifest (`include_bytes!`, read once) and `resolve_inside`. `test/cases/manifest.json` (34 validations,
  10 comparisons, 35 paths, made from the TypeScript) runs on both sides. The DLLs are `bundle.resources`.
- Launcher P3, L19 and L20: `net/pinned.rs` is a rustls `ServerCertVerifier` that accepts only the certificate
  whose SHA-256 is the invite's fingerprint (the handshake signature is still checked with its key; SNI only for
  names; ALPN `http/1.1`), so nothing is written to a wrong server, and a mismatch stays its own error with the
  fingerprint the server showed. `net/http.rs` is `http.ts`: the endpoint rules (plain HTTP only into the tailnet or
  to loopback), the `Host` header, one connection per request, no redirects, a size cap, one deadline for the whole
  request, and cancelling. It is built on hyper 1 and tokio-rustls rather than reqwest: reqwest 0.13's rustls
  feature brings the platform certificate verifier the launcher never uses, and the relay (L22) needs a hyper
  client over the same pinned connector. Tests: rcgen certificates, right, wrong and rotated, against local servers.
- Launcher P3, L21: `net/hostapi.rs` is `hostapi.ts`: device signals, hunt region, ServerStatus (with the older
  servers' `/dauntless-status` fallback), Register, GetUserInfo, the content probe and manifest, news, branding and
  art, with the same answers per status code and the key header sent only to the invite's host over a pinned or
  tailnet connection. `test/cases/hostapi.json` (news, branding, image sniffing, news bodies; from the TypeScript)
  runs on both sides; the calls are tested against a local fake host.
- Launcher P3, L22: `net/relay.rs` is `relay.ts` on hyper 1: 127.0.0.1 only, `port_busy` for a taken port,
  `DAUNTLESS_REVIVED_RELAY_PORT`, the per-connection owner check hook, the browser checks (`Sec-Fetch-*`,
  foreign Host or Origin), end-to-end headers only (`crates/proxy`), the upstream `Host`, chunked bodies, streamed
  bodies both ways, custom reason phrases, keep-alive through a pool of pinned TLS connections (hyper-util), and
  WebSocket upgrades on a fresh pinned connection joined with `proxy::tunnel` (including the game's chat:
  `GET //`, a loopback Origin without a port, protocol `xmpp`); a wrong certificate answers 502
  `certificate_mismatch` and reports the fingerprint the server showed, and nothing reaches the server. The 9
  tests mirror `relay.test.ts` against a local TLS gateway. The owner check itself (`relay-owner.ts`, the Windows
  TCP table) comes with the launch code (L31).
- Launcher P3, L23 to L25 (P3 done): `net/downloader.rs` is `downloader.ts` (biggest files first, up to 4 at once,
  `.part` files hashed again and resumed with `Range` and `If-Range`, ETag check against the manifest, retries with
  backoff, busy waits on 429/503 with `Retry-After`, a stall timeout, pause/resume/cancel, progress with a 5 s speed
  meter), over a pool of pinned or tailnet connections (`net/connector.rs`, shared with the relay) or, when the host
  names a CDN, plain HTTPS with the Mozilla roots (`webpki-roots`) and never the key. `net/verify.rs` is `verify.ts`
  (size, then SHA-256 in 1 MiB pieces on the blocking pool, and the verified-file cache the Electron launcher
  writes). `net/autoregion.rs` is `autoregion.ts`. Tests: the downloader and CDN suites against Rust fakes of the
  content server (resume, corrupt transfer, damaged part, repair, cache, pause/resume, cancel, a server that ignores
  Range, wrong key, missing file, wrong ETag, attempts running out).
- Launcher P4, L26, L28 and L29: `platform/paths.rs` uses the Electron launcher's folders (userData under the
  roaming app data folder or `$XDG_CONFIG_HOME`, the default game folder), not Tauri's. `platform/settings.rs` is
  `settings.ts`: every value checked when read, the servers list rules, and the file written with the same key
  order and indent, so `test/cases/settings.json` (66 inputs on both platforms, made from the TypeScript) matches
  byte for byte. `platform/log.rs` is `log.ts`: registered secrets and anything shaped like a key, an
  `-AUTH_PASSWORD=` argument or a key header masked (with JavaScript's `\s`), `launcher.log` moving to
  `launcher.old.log` past 2 MiB, and library events and panics through the same mask (`test/cases/log.json`).
- Launcher P4, L27: `platform/keystore/` is `keystore.ts` (one key per server id, the `DRK2` format, the
  certificate binding and `rebind`, the Documents recovery copy, the backup file) over an `Encryptor`: DPAPI for the
  user on Windows, and on Linux AES-256-GCM with a random key the Secret Service keeps (`dbus-secret-service` on libdbus
  with pure-Rust crypto; zbus 5's parts were declared unmaintained in October 2026). New files are `keys/<id>.key2`; an Electron `keys/<id>.key` is read once (`legacy.rs`: the `Local State`
  key through DPAPI and AES-256-GCM on Windows; `v11` with the Secret Service password found by its attributes, or
  `v10` with "peanuts", and AES-128-CBC on Linux), written in the new format, read back, and left in place; one it
  cannot read loads as "no key", so the import flow takes over. Tested with synthetic blobs (and Chromium's known
  `peanuts` key), a real DPAPI round trip on Windows, and on Linux in WSL. Left: the KWallet route for Plasma 5,
  and reading real 0.1.39 profiles on each desktop (L2).
- Launcher P4, L30: `platform/game_config.rs` (`engineini.ts` and `display-repair.ts`: the two replaced Engine.ini
  sections, GameUserSettings.ini's quality lines, ASCII or UTF-16 files, the safe 1280x720 window with a backup of
  the file it repairs), `platform/game_folder.rs` (the four layouts, drive-letter paths only, the 150-character root
  limit) and `platform/client_mods.rs` (the pinned DLLs and the UE4SS bundle, hash-checked before and after each
  copy, kept files, removals and `disable`). `test/cases/game-config.json` (from the TypeScript, both platforms'
  path rules) runs on both sides, with `Number()`'s rules in `core::text::js_number`.
- Launcher P4, L31 and L33: `platform/launch.rs` (contract 6's arguments with `-epiclocale`, the masked launch line,
  the one game process and its exit code, running clients through tasklist and PowerShell or pgrep that never read
  other command lines back, the DirectX and Visual C++ runtime checks, the exit-code messages), `net/relay_owner.rs`
  (`relay-owner.ts`: the client row of the netstat table and the game's exact process id, shared fresh snapshots,
  one retry for a missing row, 64 waiting at most, closed elsewhere; checked against a real loopback socket on
  Windows) and `platform/system.rs` (Tailscale's usual places, free space via `fs4`, the device signals' two fixed
  PowerShell queries and their placeholders).
- Launcher P4, L32: `platform/linux_runtime.rs` is `linux-runtime.ts` and `linux-dependencies.ts`: Proton from
  Steam's compatibility tools (native or Flatpak Steam, newest by number) or `DAUNTLESS_REVIVED_PROTON`, else Wine
  (`DAUNTLESS_REVIVED_WINE`, the PATH, Lutris), each with its own prefix under userData and `dxgi=n,b`, GNOME's
  XWayland authority when `XAUTHORITY` is unset, the first-start prefix setup, and Wine or gnome-keyring installed
  through the package manager and pkexec when missing. Its tests run on Linux (WSL here, the Rust job in CI).
- Launcher P4, L35 and L36, and the rest of L5: `platform/links.rs` is `links.ts` (the fixed pages, the allow-list,
  the host's own source link), opened from Rust with the `open` crate (ShellExecuteW, xdg-open) rather than
  `tauri-plugin-opener`, which brings zbus 5 on Linux; `test/cases/links.json` (the TS
  test's 51 refused URLs and every fixed page, against five host links) runs on both sides. `platform/dialogs.rs`
  shows the folder, open and save dialogs (`tauri-plugin-dialog`) and the close confirmation with the backend's
  text; only a press of "Close anyway" closes. The window's `CloseRequested` hook asks when the controller says the
  relay or the offline services carry a running game, minimises on "keep open" and lets the controller show its
  notice. The page has no dialog permission, and in the running app a page call to any plugin outside the list never
  settles.
- Launcher P4, L34: `platform/invite_link.rs`, without plugins (tauri-plugin-single-instance brings zbus 5 on Linux,
  tauri-plugin-deep-link a CC0-licensed crate the licence list does not take): the first launcher listens on a
  per-user named pipe (Windows) or a socket in `$XDG_RUNTIME_DIR` (Linux), a second one hands it its arguments and
  exits, and the first brings its window forward and keeps the invite link for `takeInviteLink` with the
  `invite-link` event. Packaged builds point the scheme at the launcher (the current user's registry, or a desktop
  entry for the AppImage). Checked in the running app: a second launcher started with an invite exits, and the
  first one's page shows "Join ...?" for it.
- Launcher P4, L37 (P4 done in code): `platform/art.rs` answers the `dr-art` scheme from the art the controller
  registers under its SHA-256, in both webviews' forms (`dr-art://bg/<id>`, `http://dr-art.localhost/bg/<id>`), with
  `no-store` and `nosniff`, and an empty 404 for anything else. Left in P4: the KWallet route on Plasma 5 (L27),
  the manual checks with real profiles and desktops (L2, L5, L34, L36), and the page's one URL helper for the two art
  forms (with L41).
- Launcher P5, L38 and L39 (P5 done in code): `src-tauri/src/controller/` is `controller.ts`. The state sits behind
  one mutex that is held only between awaits, so each locked stretch is one synchronous stretch of the TypeScript
  controller on Node's event loop. That allows the same interleavings and no others, without an actor task
  (row L38's sketch). The `busy`/`task` checks and the actions that follow them happen under one lock.
  `controller/stored.rs` is a typed view of the settings map, which `platform/settings.rs` keeps as JSON so it
  writes the Electron launcher's bytes. `controller/platform.rs` is the `Platform` interface: the dialogs, the
  browser, snapshot and progress pushes, the art store, Tailscale, running clients, the Linux runtime and device
  signals; the app passes `TauriPlatform`. Every command except offline (L42) now forwards to the controller, and
  the backend keeps only the progress channel and the waiting invite link. The close guard asks the controller
  whether the relay carries a game, and "keep open" sets its notice. On exit the controller stops polling, the
  task and the relay, then waits for the background news and art loads. Status polling (L39) is a
  `tokio::time::interval` task that the page switches on and off, as today. `GameProcess` takes an optional
  stand-in spawn (as `Platform.spawn` did), so 11 controller tests run the whole public-mode flow (join, register,
  install, device ban, play through the relay, Engine.ini, game exit and exit codes, key rejected) and more:
  settings at PLAY, saved servers, certificate mismatch and change, busy relay port, the ServerStatus key rules,
  logout overtaking a status request, private mode with news, and Change folder. They run against a fake TLS
  gateway; on Windows the relay's netstat owner check passes because the test process is the stand-in game.
  Offline flows stay with L42–L44. Not ported yet: the Linux secure-storage bootstrap (`main.ts`
  `ensureLinuxSecureStorageDependency`) and the updater (P9). No check yet of the controller in the running app
  with a real profile.
- Launcher P6, L40 and the page's part of L41: the page imports its backend from `src/renderer/backend.ts`.
  That module returns `api.ts` when Tauri's `__TAURI_INTERNALS__` is present, and otherwise the
  `window.launcher` that the Electron preload still sets until L51. The page is the same in both launchers, and
  the injected `tauri-bridge.ts` script and the global `window.launcher` declaration are gone (`vite-env.d.ts`
  keeps the Vite types). `src/shared/art.ts` is the one helper for the two art URL forms: `dr-art://bg/<id>` in
  Electron and WebKitGTK, `http://dr-art.localhost/bg/<id>` in WebView2. Anything else is never loaded
  (`test/art.test.ts`). Left in L41: the visual check on both webviews and the distro font checks.
  Checked in the running debug app (WebView2, with the debug-only `DAUNTLESS_REVIVED_DEV_PROFILE` throwaway
  profile, so a developer's real settings and keys stay untouched). The page draws the controller's snapshot.
  `set_settings` writes `settings.json` in the Electron layout. Bad invites and register without a server answer
  as in Electron, and closing the window exits through the controller's shutdown. That check found that sync
  commands run outside the Tokio runtime, so the controller now spawns on a stored runtime handle, with a test.
- Launcher P7, L42: offline play and the offline save backup and restore answer `offline_unavailable` with the
  detail `later_update`. The page shows `err_offline_later` for that detail ("returns in a later update; your
  offline saves stay on this PC"; en, es and fi, like the other offline strings). The Tauri backend never names
  the `offline/` folder, so a profile's offline data and `.drsave` files stay as they were. The Cargo feature
  `offline` comes with L43, which has code to put behind it.
- Launcher P7, L43 (offline orchestration, compiled in but off): the launcher's Cargo feature `offline` (off by
  default, so 0.2.0 ships without it) links the metagame and DeployServer crates.
  - **Services:** their `serve` moved into `metagame::service` and `deploy::service`.
    `dauntless-revived-launcher --service metagame|deploy --env-file <file>` runs one of them (D12).
  - **`src-tauri/src/offline/`**
    - `layout` keeps the Electron launcher's `userData/offline` folders.
    - `state` (`offline-state.ts`):
      - The profile's secrets are made once (an RSA-2048 signing pair from the deploy crate's generator and a
        game-server key). A pair whose halves don't match, or a file that can't be read, is never replaced.
      - The two settings files are written single-quoted, so the services' `.env` reader takes them literally.
    - `game_config` (`offline-game-config.ts`) points Game.ini's Phoenix endpoints and Engine.ini's chat at this
      PC. build.rs copies the DLL's 167 endpoints out of `native/internal-server/dllmain.cpp` without the vendor
      webhook path, so nothing else of that file reaches the binary, as the stage script did.
    - `save_format` and `save` are the `.drsave` container and the restore journal, with the database half in
      process from `metagame::offline_save` (X1).
    - `stack` (`local-stack*.ts`):
      - The start order, the stop order and cleanup after a failed start.
      - `ProcessRuntime` starts this binary as each service with only the operating system's variables, logs to
        `offline/runtime/<service>.log`, and waits for the health paths.
      - It stops a service with `shutdown` on stdin (X7), then kills it after 7 s.
      - Between the two starts it registers the game-server key in-process (`metagame db gs-key`).
  - **Controller:** `controller/offline.rs` is `startOffline`, `stopOffline` and the save operations, with
    cancellation between steps. The game's exit stops the stack, online play waits while a stack runs, closing
    asks first while an offline game runs, and shutdown waits for a start in progress. The `.drsave` dialogs are
    the Electron launcher's.
  - **Startup:** only on Windows, as in the Electron launcher.
  - **Tests:**
    - 20 with the feature on: the offline modules, the save runtime's cases from `offline-save.test.ts` (round
      trip across machines with credentials scrubbed, damaged saves, crash journals, refused destinations), and
      4 controller flows (register once and reuse, memory admission, stop during a start, shutdown during a
      start).
    - `tests/offline_stack.rs`, where the launcher binary really runs both Rust services on free ports and stops
      them.
    - CI runs clippy and the tests with the feature too.
- L44 waits for L51: `scripts/stage-offline-runtime.mjs` is still how the Electron launcher 0.1.x packages its
  Node services, so it goes with the Electron build, not before. The Tauri launcher stages nothing.
- M35 tool: `tools/replay.ts` (pure parts in `replay-core.ts`, 9 tests) replays a corpus against the
  Node and Rust metagames, each on its own copy of one database snapshot: the account keys in the copies
  are swapped for replay keys by hash, tokens are signed fresh, a game-server key is added, matchmaking
  is off by default. Answers are compared after normalising dates, epoch times, generated ids (learned
  per server, so a new character's id is used in later calls), tokens and the build's version/commit;
  key order is compared too (SB1). Chat sessions are replayed stanza by stanza. End to end on a recorded
  session (register, token exchange, login, account, character create/list, inventory, loadout,
  friends, party, game-server status, chat login/bind/room/message): 14 of 14 requests and the chat
  session equal; the corpus held no key, token or game-server key. Left: the real corpus from the live
  servers, and the final run after the catch-up (M44).
- Metagame B2, M31, first part: `test/appclient.ts` runs its calls against the Rust binary when
  `METAGAME_RUST_BIN` names it (same database, same environment; restarted when a test changes the
  environment or resets what the app keeps in memory). The 16 test files on it: Node 256/256; Rust 246
  pass, 0 fail, and 10 marked Node-only (`NodeOnly`: they move the app's clock or hand it store offers
  in process). Found on the way: the Rust listener now sets `TCP_NODELAY` like Node's; and on this
  Windows host Node's `fetch` (undici 8.9, Node 26.7) takes about half a second to reuse an idle
  keep-alive connection to any server outside its own process (a raw socket server shows it too),
  so the test calls go through `node:http`. Node tools that call the metagame with `fetch` (the
  Discord bot) will see that delay against any server.
- Metagame B2, M31, second part: `LOG_BODIES` (`bodylog.rs`: the routes, the redaction, the size caps,
  `BODY_LOG_PER_PATH`, aborted answers), and `test/rustshim.ts` for the tests that start the app
  themselves: preloaded, it starts the Rust binary in `app.listen`'s place, sends `fetch` to it through
  `node:http`, passes the server's log lines to the app's logger, and restarts it after a
  `Reset...ForTests` hook. `npm run test:rust -- <binary>` runs 21 files (the 16 on `appclient.ts`,
  then friendsapi, partyhttp, platform, progressiondefault and socialflow): Rust 272 pass, 0 fail, 10
  Node-only; the same files pass on Node. Not run against Rust: guildhttp (its tests share state and
  move the guild clock, so skipping some breaks the rest; the guild parity scripts cover the routes),
  the tests that put one router in their own app (backup, health, moderation, Discord stats), the
  matchmaking tests with their own servers, and chat (M32). Then: the port to Rust tests.
- Metagame B2, M31, third part: every `node:test` test of `legacy/UndauntedMetagame/test` has its Rust port
  or a reason it does not apply, listed in `rust/services/metagame/tests/ported.json` (791 of 791) and checked
  by `tests/ported.rs`. The ports are integration tests in one binary (`tests/m31`, 764 tests) on a temp
  database per test, with the Node tests' helpers ported beside them: the store kit, scripted deploy servers
  (raw TCP, so an answer can be held, dropped or slowed), the social client, and for chat the client model
  (`chatclient.rs`) and the raw client (`chatwire.rs`). Every chat server a test starts checks that no stanza
  outside a room reached a player from their own account (`chatinvariant.ts`). For the tests the app gained
  clocks it can move (`party_clock`, the guild memory's), and chat a manual tick, an output observer, a friends
  lookup for presence and its largest backlog. Bugs found and fixed on the way: two identical direct joins at
  once both launched a server; a repeated game-server key header counted as a key; the `features:` start line
  and the `BODY_LOG_PER_PATH` warning were missing; chat's task picked among ready events at random, so a
  friendship change could be handled after a frame that came later.
- Catch-up (D15, X3), first pass: no `rust-port-start` tag was cut, so the lists run from where this branch
  last met upstream (`175f4e9`). Upstream's two later Node fixes are in `legacy/` and ported with their tests:
  `7c1191d` (the inventory core refuses a transaction that pays for instanced items the character already
  holds, so a purchase retried under a new transactionId cannot charge twice; M44) and `14351fc` (the
  gateway's allowlist feed leaves a helper alone for a doubling interval, 2 s up to a minute, after three
  failures in a row, and any answer clears it; S2). Each later pass starts from the last upstream commit taken.
- Metagame B5, M38, first part: CI's `server-binaries` job builds the release binaries (`metagame`, `gateway`,
  `allowlist`, `content`, `deploy`, `discord-bot`; fat LTO, the release profile) for the three D14 targets,
  runs the shared HTTP-level metagame tests (`npm run test:rust`, M31) against each release `metagame`, and
  keeps each target's files with their `SHA256SUMS` as a 7-day artifact; nothing is published. Left: the
  chat, backup and shutdown items of the smoke list on every target, and release archives for the kits
  (M39, M40), which also carry the store images and progression config next to the binary.
- Metagame B5, M39: the Linux kit runs the Rust services. `install.sh` takes the binaries from
  `--binaries <dir>` (a CI `server-<target>` download, checked against its `SHA256SUMS`) or builds them with
  `cargo`, puts them in `<install-root>/bin` by rename, links `bin/dr-run-game` to `deploy`, writes the
  settings with `deploy generate-config`, makes the certificate with `gateway make-cert`, and bootstraps the
  database with the new `metagame migrate --env-file` (create or adopt, migrate, exit; the `db` tools still
  never migrate) and `metagame db gs-key` / `add-invite`. The systemd units start `bin/<service> serve
  --env-file=…`; `install-worker.sh` installs `deploy` and the SSH forced command runs `deploy worker`.
  Node is no longer needed on a Linux host; the Linux-only `.mjs` helpers are gone (the OpenBSD kit keeps
  its own until M41). CI's `linux-kit` job runs `tests/install-test.sh`: both installers in test mode,
  twice, as an upgrade, plus a tampered binary that must be refused. The setup page (both languages) and
  the kit README say so. Left: an install on the canary itself (D23), which is the cutover (M42).
- Metagame B5, M40 and M45: the Windows kit runs the Rust services. The installer, `Update-DauntlessServer.ps1`
  and `Deploy-Remote.ps1` take the binaries from `-ServerBinaries <folder>` (the CI
  `server-x86_64-pc-windows-msvc` download, checked against its `SHA256SUMS`; `Deploy-Remote.ps1` uploads
  them and compares their SHA-256 on the server) or a release build in the source, and put them in
  `<root>\app\bin` with the rest of the code. `Stack.ps1` starts `bin\<service>.exe serve --env-file=...` and
  finds a component's processes by their full executable path, and also by the old Node entry script, so
  the first update of a Node install stops the Node processes (and a rollback to the Node build still
  works, with the Node kit it brings along). The firewall rules name `gateway.exe`, `metagame.exe` and
  `content.exe`. The Node helpers are gone: `metagame.exe db` for the database steps, `deploy.exe
  signing-keys` (new) for the token-signing pair, `content.exe verify` for the game files, `gateway.exe
  make-cert` for the certificate; the installer installs no Node.js and `NodePath` is no longer written.
  The backup (M45) asks a running metagame for its live copy (`POST /internal/backup`) and otherwise
  copies with `metagame.exe db backup`; whether the metagame runs is read from its port, not from a
  localized error text. The weekly receiver checks its copy with the host's own `metagame.exe`, or with
  Node's SQLite on a host still on Node. The Linux kit gains `stack.sh backup` (M45's Linux half): the
  database through `metagame db backup` and the settings, game-server key and certificate beside it, in
  `<data>/backups/<date_time>`; `tests/install-test.sh` checks it. CI's `server-kit` job builds the binaries and runs the three kit
  tests on them, on every change to the kit or the services; `Test-Sandbox.ps1` installs, updates, rolls
  back and restores the Rust stack. The reference docs (both languages) and the kit README say so. Left:
  `tests\Test-Sandbox.ps1` on a real Windows Server and the first update of a production Node install
  (the cutover, M42).
- Metagame B3, M32-M34: chat (`chat/`, from `realtime/*.ts`), on by default on `CHAT_PORT` as in
  Node, started before the HTTP listener and closed with it (`<close/>`, at most 1 s). One task owns
  all of chat and answers frames one at a time, in order, as Node's thread did; each connection has a
  reader and a writer, and the writer counts what waits unsent for the 256 KB output limit. The frame
  parser is a port of ltx 3.1.2's (the parser Node used), not quick-xml, so a frame parses exactly as
  it did, leniencies included; it is linear and keeps no recursion on the input. Friendship changes
  reach presence through a broadcast channel once they are stored. Parity: `tests/chatparity`
  scripts drive WebSocket clients against Node's chat (`record.cjs`) and Rust's, and every frame each
  client receives matches, with how its connection closed: five scripts, 229 steps (logins, binding,
  IQs, rooms with their refusals and limits, whispers, guild rooms, sessions and ghosts, the throttles,
  friends' presence over HTTP friendships, held-back changes, `CHAT_NICK_CHECK=log`, an oversized
  frame). The timers (login and bind deadlines, idle pings, ping timeouts) and shutdown are checked
  on a clock the tests move. Left of B3: the TS chat tests themselves, which build the chat in
  process and stay Node's.
- Metagame B3, M33: `rust/fuzz` (cargo-fuzz, nightly, its own workspace) fuzzes the frame parser and
  the helpers that read client text (JIDs, nicknames, the trace redaction, log text) for panics, and
  checks that a parsed tree holds nothing longer than its input. First run: 10 minutes, about 470,000
  inputs from the parity scripts' frames, nothing found. Left: the 24-hour run before cutover.
- Part C, S1: the content server (`rust/services/content`, binary `content`: `serve` and `verify`).
  Every route, the path, key, range and conditional checks, the startup check of the game folder and
  the files that change after it, the per-account and server-wide download slots, the two-minute idle
  cut, the art pack and the news (file and GitHub), and its own JSON log format, kept as it was. The
  1.4.4 manifest is built into the binary (`CONTENT_MANIFEST` names another file), so the binary needs
  no `data/` folder next to it. No zip handling was needed (`yauzl` was a dependency the server never
  used). Parity: `tests/parity` builds the same game folder, art pack and news for Node
  (`record.cjs`) and Rust with a fake metagame and sends raw HTTP, the path as written: two scripts,
  56 replies, statuses, headers and bodies equal. The stalled-client slot test and the download log
  test are ported. Throughput, 4 parallel downloads of a 512 MB file on loopback: Rust about
  5.5 GB/s, Node about 3.8 GB/s. `service_kit::js` now holds the JavaScript-semantics helpers the
  metagame had, shared with the content server. Left: the launcher downloader tests (L23) against it.
- Part C, S2: the gateway (`rust/services/gateway`, binary `gateway`: `serve` and `make-cert`). TLS 1.2
  and 1.3 with `rustls` (`aws-lc-rs`), HTTP/1.1 only; the same policy (admin-route block, game-server
  key refusal, path checks), per-address rate limits and connection limits, redaction, allowlist
  feed, forwarded headers and WebSocket tunnelling. `serve` loads the existing certificate and key,
  checks that they belong together, and never makes new ones, so invites keep their `fp`. `make-cert`
  (RSA-2048, SHA-256, ten years, as `tools/make-cert.js`) is for new installs and still refuses to
  overwrite without `--force`. X5 landed with it: `crates/proxy` holds the hop-by-hop rules and the
  tunnel, and `service_kit::linelog` the JSON-lines log shared with the content server. The keep-alive
  (65 s), header (10 s) and idle timers are the gateway's own, because hyper's header timer also runs
  between keep-alive requests and would cut idle connections at 10 s; a head that is too slow still
  gets Node's 408. Tests: the unit tests of `ip`, `policy`, `redact`, `ratelimit`, `config` and the
  feed, and the over-TLS suites `gateway.test.ts` and `limits.test.ts` ported (pinning, routing,
  forwarded headers, the feed, the access log, upgrades and the 1.4.4 chat request, body caps,
  100-continue, rate limits, connection limits, slow heads and bodies, keep-alive, shutdown). Parity:
  `tests/parity` runs 40 raw requests through the Node and the Rust gateway against the same fake
  upstreams: statuses, headers, bodies, access-log lines and the feed's addresses equal. Known
  difference: requests the HTTP parser rejects (bad syntax, oversized heads) get hyper's own 400 or
  431 with an empty body instead of Node's JSON one. Left: traffic capture (M2), and connecting a
  0.1.x and a 0.2.x launcher through it.
- Part C, S3: the allowlist helper, one binary for both platforms (`allowlist` in the gateway
  package; `--close-ports` as before). The Windows helper's design is the shared one: the debounced
  firewall sync (one change per interval, retried with backoff), the audit log, the saved state and
  the loopback HTTP endpoint with the shared secret, unchanged for the gateway. Windows changes its
  rule through the COM API (`INetFwPolicy2`) instead of PowerShell: the rule is found by the name the
  Node helper gave it, so an existing rule is taken over, never duplicated. Linux feeds `nft -f -`
  the same table and sets as `allowlist.mjs`, with a fixed argument list. Real mode on Windows still
  refuses to start without administrator rights. Differences from the Linux Node helper: it now needs
  `ALLOWLIST_DRY_RUN` set like the Windows one (both kits already set it), answers 200 before the
  rule change runs and retries a failed change instead of answering 503, and `--close-ports` keeps
  the saved set. A dry run writes the change it would make (enabled, addresses, ports; on Linux the
  nft script) instead of the PowerShell script. Tests: the state, sync, config and script tests, the
  helper over HTTP (secret, paths, refusals, debouncing, expiry, restart) and the e2e tests with the
  gateway, in process and as the two binaries, all in dry run. Left: checking the rule after login
  and logout on a real Windows server and a real Linux server (needs administrator rights on a
  machine whose firewall may change), and the kits' service files, at cutover (B5).
- Part C, S4: the DeployServer (`rust/services/deploy`, binary `deploy`). The table row below is out
  of date: the Node server has no database (no SQLite, Drizzle or JWT code is left in it), so there
  are no migrations. Ported: Ramsgate and the Training Dojo (one shared launch per world, liveness
  restarts, `ENABLE_DOJO`), the city pool and its caps, hunts from game args, catalog hunts and the
  weekly Trials rotation (checked against the DLL's `TrialsRotation.h`), the launch queue and
  spacing, readiness (ready file or UDP bind), hunt, CPU, memory and backlog admission and the drain
  file, the watchdog and hunt lifetimes, the overflow and regional workers with their capacity
  cooldowns, Ramsgate balancing, the loopback-only and no-proxy checks, and shutdown that stops the
  game processes it started. Settings are read on each use, as Node read `process.env`. Processes
  sit behind a `Platform` trait, so the tests use stand-ins, as the Node tests did. Tests: every Node
  suite ported (persistent worlds, launch latency, city instances and pool, city router, hunt
  contract, capacity, CPU, lifetime, occupancy, readiness, matchmaking input and routes, worker
  snapshots, overflow, regions, database-only role); 80 tests. Not done: a run against the real game
  binary, and the offline stack in the launcher (D12, X2) still starts the Node server.
- Part C, S5: the Discord key bot (`rust/services/discord-bot`, binary `discord-bot`). The state
  file loads unchanged and an unreadable one still stops issuance; codes are saved before the
  metagame hears of them, DMs are reserved durably before they are sent and reconciled from the DM
  history, and every operation runs one at a time, as in `keys.mjs`. `/key` (claim, status, link,
  resend, recover), the owner's `/counter`, command registration that keeps command ids, the
  one-instance lock and the two-minute connection check are ported. Discord is reached without
  serenity or poise: serenity 0.12.5, its latest release, pins rustls 0.22, whose webpki has open
  advisories (cargo-deny fails) with no fixed 0.102 release, and its other TLS backend is OpenSSL,
  which is banned (D10). The bot keeps its own small gateway client (`gateway.rs`: identify,
  heartbeat, resume, a new session when Discord asks, fatal close codes end the process) on the
  workspace's tungstenite and rustls, calls the REST API directly with a wait on 429s, and keeps
  what it needs of each guild (owner, member count, role permissions) from the gateway's events to
  work out its channel permissions as discord.js does. Registration was already the bot's own (it
  compares and patches commands in place). The invite string now comes from
  `protocol::format_invite`, which encodes the server name as `encodeURIComponent` does (`%20`)
  where the Node bot used `URLSearchParams` (`+`); the launcher reads both the same. The logic sits
  behind traits (metagame, Discord REST, interaction, guild), so every Node test is ported against
  stand-ins: keys, delivery, commands, counter, recovery, instance; the gateway is tested against a
  local stand-in for Discord's. Not done: a real `/key claim` on a test guild (needs a bot token).
- Part C, S6: the Unix kit helpers. `deploy run-game` is `game-runner.mjs` (Proton first, else
  Wine, Xvfb without a display, the game's own `dxgi` forced, the runtime's process group ended
  with it), `deploy worker <payload>` is `worker-launch.mjs` (one base64url JSON argument list,
  checked, plus the worker's env file), `deploy ssh-game` is OpenBSD's `launch-gameserver.mjs` (the
  same payload over batch SSH), and `deploy generate-config` is `generate-config.mjs` (the same
  files and secrets; the signing key comes from rcgen). The `deploy` binary linked as
  `dr-run-game` or `dr-ssh-game` is that launcher, so `GAMESERVER_BINARY_PATH` points at a link
  in `<bin-dir>` instead of a Node script. `new-invite.mjs` is `metagame invite`, with the invite
  from `protocol::format_invite`; `dr-db.cjs` was already `metagame db`. `ssh-worker-command.sh`,
  `stack.sh`, `prepare-game.sh` and `configure-pf.ksh` stay shell: they only check input and control
  processes. Tests: the runner, worker payload, SSH and config tests, and an invite into a real
  database. Not done: the kits' install scripts and service files still start the Node tools, and
  the two-machine worker launch in a CI container; both belong to the kits' cutover (B5).
- Part E, T1-T5: `tools/` is a Bun project (`package.json` with Bun 1.4.3 in `packageManager`, `bun.lock`,
  strict `tsconfig.json`, `bunfig.toml`; TypeScript and `@types/bun` are its only packages, both for the type
  check). `check-repo.ts` and `launcher-version.ts` give the JS versions' findings and output (compared on the
  tree and on a planted token); `sync-roadmap.ts` and `build-llms.ts` regenerate the docs files byte for
  byte apart from the generator's own name in `docs/roadmap.md`. `make-game-manifest.ts` no longer needs
  `yauzl` from another project's `node_modules`: `tools/zip.ts` reads the central directory (Zip64
  included) and streams stored or deflated entries, refusing what yauzl's strict mode refused, and
  `zip.test.ts` covers it on archives it writes; an archive from Python's `zipfile` with forced Zip64
  reads back with the source files' hashes. CI runs the type check, the tests and the generators on
  Bun (`oven-sh/setup-bun` pinned by commit, the version read from `tools/package.json`), the release
  workflow calls `launcher-version.ts`, and Dependabot has a `bun` entry for `tools/`. Not done: T5's
  regeneration from the real 1.4.4 zip (it is not on this machine); T6-T10.
- Part E, T6, T7 and T10, and T8 as changed (D20): the dashboard, its fleet, performance and worker
  modules, worker health, Discord status and the performance summary are TypeScript on Bun; the two
  servers use `Bun.serve` (`idleTimeout` 35 s for the dashboard, whose backend answers may take 30 s;
  worker health allows 16 requests at a time, as Node's 16 connections did) and worker health runs
  `worker-processes.ps1` through `Bun.spawn`. The browser script `dashboard-client.js` stays plain
  JavaScript, served as it is: with Bun on the servers there is nothing to bundle. All six test files
  are on `bun:test` and in CI (53 tests); three of them had never run in CI, and the client test had
  been failing on a fake DOM without `document.head`. The tests are type-checked with a looser
  `tsconfig.test.json` (their fixtures are partial on purpose), the tools with the strict one. T8:
  `Install-DRBun` in the kit downloads Bun's baseline Windows build, checks it against the pinned
  SHA-256 and the release's `SHASUMS256.txt`, and puts it in `Program Files\Bun`;
  `Install-OwnerDashboard.ps1` and `Install-DiscordStatus.ps1` call it, copy the `.ts` files, remove
  the Node version's `.mjs` copies, and register the same tasks with `bun --no-env-file --env-file=…`,
  so an upgrade leaves no Node task. Checked: `Install-DRBun` against the real release in Windows
  PowerShell 5.1 (with `ProgramFiles` in a scratch folder), and the installed layout started with that
  `bun.exe` (the page, the client, the key check, a stray `.env` ignored). Not done: the installers on a
  real server (`MIGRATION_TODO.md`). `NodePath` stays: the services still run on Node until B5.
Goal: one Rust codebase for the launcher and every server, running as fast as possible. The launcher moves from
Electron to Tauri 2 (its TypeScript page stays). The metagame, gateway, content server, DeployServer and Discord bot
move from Node to Rust. This replaces the earlier plan to move the metagame to Bun first: the Bun step would rewrite
the router, SQLite layer, chat, auth and logging once for Bun and again for Rust. Its compatibility findings carry over
into Part B, marked *(from the Bun analysis)*.

Row ids: **L** = launcher (Part A), **M** = metagame (Part B), **S** = other services (Part C), **R** = repository
layout (Part D), **T** = tools (Part E), **X** = cross-project (section 4). D1–D7 are launcher decisions,
D8–D26 server-side, workspace, repository, tools, testing, release and security decisions.

## 1. End state

| Component | Today | After |
| --- | --- | --- |
| UndauntedLauncher | Electron 44 + Node main process, Squirrel, Forge | Tauri 2: Rust backend, TS page rewritten on SolidJS after the cutover (P10), isolation pattern, `tauri-plugin-updater`, NSIS + AppImage/deb/rpm |
| Launcher offline mode | Metagame + DeployServer on Electron's Node | Off in 0.2.0; it returns with the Rust metagame and DeployServer as service modes of the launcher binary (D1, D12, X2); no Node |
| UndauntedMetagame | Node, Express, `better-sqlite3`, Drizzle, `ws`, `ltx`, `pino`, `jsonwebtoken` (~19.8k lines, 69 test files, 31 migrations) | Rust: `tokio`, `axum`, Diesel on SQLite, `quick-xml`, `tracing`, `jsonwebtoken` |
| UndauntedGateway (+ allowlist helper) | Node, `node-forge` (~2.6k lines) | Rust: `rustls`, `hyper` reverse proxy, `windows` crate firewall helper |
| UndauntedContent | Node, `yauzl` (~1.9k lines) | Rust: `axum` with range requests, `zip` |
| UndauntedDeployServer | Node, Express, `better-sqlite3`, Drizzle, `jsonwebtoken` (~1.8k lines) | Rust on the same shared crates as the metagame |
| UndauntedDiscordBot | Node `.mjs`, `discord.js` | Rust: own gateway client and REST calls (last; D18) |
| Server kits (`deploy/`) | Node + `npm ci`, `dr-db` scripts, `.mjs` helpers (Linux kit, `deploy/unix-common/`, OpenBSD port) | Release binaries + `metagame db …` subcommands; Linux helpers in Rust; OpenBSD port removed (D14) |
| UndauntedInternalServer, `dxgi.dll` | C++ | Unchanged code, moved to `native/internal-server/` (R2) |
| Repository | One folder per project at the root, mixed languages | Four code families at the root: `rust/` (one cargo workspace: launcher, servers, shared crates), `native/` (C++), `client-mods/` (UE4SS mods and assets), `tools/` (TypeScript on Bun), plus `deploy/`, `brand/`, `docs/`, `friend-kit/` (Part D) |
| `tools/` | Node `.js`/`.mjs` scripts (~3k lines: owner dashboard, Discord status, worker health, performance summaries, repo checks, docs generators, game-manifest builder), PowerShell, one Python script | TypeScript on Bun; the dashboard, worker agent and Discord status ship to servers as one `bun build --compile` executable (Part E, D20) |

## 2. Repository and workspace layout

The repository becomes a monorepo with one folder per code family (D19, Part D). Every Rust component lives in one
cargo workspace under `rust/`; the C++ client DLL lives under `native/`; the client mods stay in `client-mods/`; the
operator and repository tools are TypeScript on Bun in `tools/` (D20, Part E). Final
layout, once every Node project is gone:

```text
rust/                      cargo workspace (D9)
  Cargo.toml, Cargo.lock
  rust-toolchain.toml      pinned stable toolchain
  deny.toml                cargo-deny licences + advisories
  crates/
    protocol/              wire types shared by launcher
                           and servers: invite, status,
                           register, manifest, server.json
    service-kit/           axum setup, Express-compatible
                           routing, body limits, rate
                           limits, pino-style logs, config,
                           health, shutdown
    store/                 Diesel pools (SQLite now,
                           Postgres-ready), embedded
                           migrations, Drizzle adoption,
                           online backup
    proxy/                 hop-by-hop rules, header copy,
                           WebSocket tunnelling (relay and
                           gateway)
  services/
    metagame/              lib + bin `metagame`
    gateway/               lib + bins `gateway`, `allowlist`
    content/               lib + bin `content`
    deploy/                lib + bin `deploy`
    discord-bot/           bin `discord-bot`
  launcher/
    src-tauri/             launcher backend (Part A)
    ui/                    TS page, Vite, package.json
native/
  internal-server/         C++ DLL: .sln, .vcxproj,
                           MinHook, SDK, tests
client-mods/               UE4SS mods, PS5Icons, manifest
deploy/                    server kits (Windows, Linux)
tools/                     TS on Bun: operator tools,
                           CI helpers, generators (Part E)
brand/                     brand source + build.py
docs/                      GitHub Pages site (stays here)
friend-kit/                PowerShell friend kit
.github/                   workflows, templates
```

Until each Node project is deleted, it keeps its old folder under `legacy/` (`legacy/UndauntedMetagame/`,
`legacy/UndauntedLauncher/`, …), so the code being replaced is kept apart from the new code, which grows under `rust/`
(R5, R6). The server kits flatten `legacy/` when they install (`Build-DRApp`, `install.sh`, `install.ksh`), so an
install root keeps `UndauntedMetagame/` and its siblings at the top, as before; the kits also accept older code
with the packages at the top level. The tables in Parts A–C use the paths that exist at the
time of each row.

Release profile: `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`, `panic = "unwind"` (servers catch a panicking
request with `tower-http`'s `CatchPanicLayer` instead of dying). Optional PGO with `cargo-pgo` on the shadow-traffic
corpus (M2) once the port is stable.

## 3. Decisions, blockers and spike results

### 3.1 Launcher decisions (D1–D7)

| # | Topic | Decision | Consequence |
| --- | --- | --- | --- |
| D1 | Offline services | **No Node sidecar.** 0.2.0 ships with offline mode turned off (L42); it comes back when the Rust metagame and DeployServer run as `--service` modes of the launcher binary (D12, X2) | Players have no offline play from the bridge release until X2. Their `offline/` data and `.drsave` files stay untouched on disk and open again with X2. No throwaway Node packaging, staging script or 30–40 MB runtime |
| D2 | Self-update | **`tauri-plugin-updater`** on Windows (NSIS) and the AppImage, with mandatory ed25519 update signatures. Existing Squirrel installs reach it through a one-time bridge release: a last Electron 0.1.x that downloads the Tauri NSIS installer, checks its SHA-256 against a value compiled into the bridge, runs it, and removes the Squirrel install. The bridge always installs the first release of the new major version (0.2.0; under 0.x versioning the minor number is the major line), which then updates itself to the newest version through the Tauri feed. The Squirrel feed stays up for one major version: it is retired when 0.3.0 ships. | One extra Electron release. 0.1.x installs that haven't started the launcher before 0.3.0 reinstall by hand (README note). The updater signing key and its password are kept privately by the maintainers and as release-job secrets |
| D3 | Linux webview | **WebKitGTK 4.1 is required.** The `webkit2gtk-4.1` API (libsoup 3) has been in distributions since 2022; distributions without it are dropped. | Supported: Ubuntu 22.04+, Debian 12+, Fedora, Arch, openSUSE Tumbleweed/Leap 15.5+, SteamOS 3 (AppImage). Not: Ubuntu 20.04, Debian 11, RHEL/Alma 8. deb/rpm declare the dependency; the AppImage bundles it |
| D4 | Isolation pattern | **On.** `app.security.pattern = { "use": "isolation", "options": { "dir": "../dist-isolation" } }`. Every `invoke` passes through a sandboxed isolation iframe that checks it before Tauri encrypts and forwards it. | A small extra bundle and a little latency per call; progress streams (backend → page) are unaffected |
| D5 | Languages | All **nine** UI languages that exist today ship: en, fi, de, es, fr, it, ja, pt, ru, plus the nine game-client languages (`GAME_LANGUAGES`). Adding a language later is data only (one strings file + one list entry, no Rust or renderer code). No new strings are written as part of this migration. | README/README.fi/LINUX.md updated to list all languages (they still say English and Finnish) |
| D6 | Windows WebView2 | `bundle.windows.webviewInstallMode = embedBootstrapper` | Windows 10 PCs without the Evergreen runtime get it during install |
| D7 | Reading old safeStorage keys | **Own implementation** (~60 lines) in `src-tauri/src/platform/keystore/legacy.rs` on crates the launcher needs anyway: `windows` (`CryptUnprotectData`), `aes-gcm`, `cbc`, `pbkdf2` + `sha1`, `serde_json`, `secret-service`, `zbus` (KWallet). **Not** `chromium-safestorage-core` as a runtime dependency: its Windows path needs the offline DPAPI master key (it is a forensics tool), it has no live Secret Service lookup, and a single 0.1.0 release with one maintainer is too new for the code that handles account keys. It is a dev-dependency that cross-checks our decryption in tests (Apache-2.0, AGPL-compatible). **KWallet is supported**: Electron uses it on KDE Plasma, which includes SteamOS desktop mode, so sending those players through "import key backup" would hit a large share of Linux users | Legacy reader is read-only and runs once per key; it never writes the Chromium format. The old Electron copy (the `keys/` blob, `Local State`, the Secret Service item / KWallet entry) is **kept for one major version** so a rollback to the Electron launcher still works through 0.2.x, and deleted by 0.3.0 (row L50a). Keys registered or rebound in 0.2.x exist only in the new format, so a rollback can't see them |

### 3.2 Server-side and workspace decisions (D8–D26)

| # | Topic | Decision | Consequence |
| --- | --- | --- | --- |
| D8 | Bun step | **Dropped.** The Node metagame stays in production, receiving fixes only, until the Rust metagame passes shadow traffic (M35) | No interim speed-up; Node fixes made during the port are ported to Rust at the end (D15, M44) |
| D9 | One workspace | Section 2. Launcher and servers share `protocol` and `proxy`; all servers share `service-kit` and `store` | A change to a wire type breaks the build on both sides at once instead of at runtime |
| D10 | Stack | `tokio` + `axum` + `tower-http`; **synchronous Diesel 2.x behind `store`'s closure API**: a request's database work is one closure, `store::read(\|conn\| …)` or `store::write(\|conn\| …)`, run on tokio's blocking pool (`spawn_blocking`) with a connection from Diesel's `r2d2` pool; `schema.rs` generated from the adopted database with `diesel print-schema`; boxed queries for dynamic filters; `diesel::sql_query` only where the DSL can't express a query or the performance check (M36) asks for it. Not `diesel-async`: a cancelled `diesel-async` transaction stays open (section 3.4). Also `jsonwebtoken` (RS256), `aes-gcm`, `sha2`, `rand`, `subtle`; `quick-xml` for XMPP; `tracing` with a pino-compatible JSON formatter; `clap` for subcommands; `rustls` everywhere with its default `aws-lc-rs` provider (faster than `ring` in rustls's own benchmarks; post-quantum X25519MLKEM768 key exchange on by default; `aws-lc-sys` builds on Windows without NASM or CMake). SQLite rules: a write pool with one connection plus a read pool; `busy_timeout` set first on every connection; `journal_mode=WAL` set once at start, on the write connection; `BEGIN IMMEDIATE` (`immediate_transaction`) for writes; no `.await` inside a transaction, which the compiler enforces because the closure is synchronous | Queries are checked against `schema.rs` at compile time: a wrong column, type or nullable field doesn't compile, which catches the `null` / `undefined` class of bugs (SB1, M15) before the shadow replay. One blocking-pool hop per closure instead of one per query: in the spike, 64 tasks on 4 read connections did 92k primary-key reads/s with 10 queries per closure, against 40k/s through `diesel-async` and its pool. A dropped request can't leave a transaction open: the closure runs to its end, like a `better-sqlite3` transaction today. Costs: slower compiles, long type errors, and more porting work than a builder close to Drizzle's style |
| D11 | Migrations | **Diesel migrations** (`diesel_migrations`, embedded in the binary with `embed_migrations!`, tracked in `__diesel_schema_migrations`). The first one, `0001_baseline`, creates the schema exactly as the latest Drizzle migration leaves it. Existing databases are adopted, not migrated: `store` checks that the last row of `__drizzle_migrations` is the latest Drizzle migration (its `created_at` is that migration's journal `when`, which is all Drizzle itself compares, and its `hash` the SHA-256 of its SQL file as checked out, so the LF or the CRLF text) and that the schema matches the snapshot (M18), then records the baseline as applied without running it; `__drizzle_migrations` stays in the file, untouched. A database at any other Drizzle state is refused, except offline databases and `.drsave` saves from older launchers, which first get the remaining frozen Drizzle SQL replayed (X1). The Drizzle migration set is frozen after the Node catch-up (M44) | Safe because there is one operator, with two regional server sets, always on the latest code. A Node rollback stays possible until the first Diesel migration after the baseline |
| D12 | Offline mode | The launcher links the metagame and DeployServer crates and starts each as a child process of itself (`launcher --service metagame --env-file …`). Same binary, separate processes | No sidecar files and no Node in the launcher; a crashing service can't take the launcher (and the relay) down with it |
| D13 | Verification | **Shadow traffic.** Redacted request/response pairs are recorded at the gateway and at the metagame's own listener, plus recorded chat sessions (M2); before cutover the corpus is replayed against Node and Rust from identical DB snapshots and the responses are diffed (M35) | The game client's undocumented expectations are caught before players see them |
| D14 | Platforms | Servers: `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. **OpenBSD is dropped**: `deploy/openbsd-server/` is not ported to Rust and is removed with the Node services (M41). OS-specific code lives only in the firewall helpers (S3) and behind `cfg(windows)` / `cfg(unix)`. Launcher: Windows x64 and Linux x86_64, as before | OpenBSD hosts move to the Linux kit, which can run the control plane and the game on one machine (OpenBSD always needed a separate Linux game worker). No tier-3 Rust target or OpenBSD CI VM to maintain |
| D15 | Node and Electron fixes during the port | Fixes keep landing in the old code as needed and are **ported in one catch-up per component** before that component's final check, not PR by PR. One tag, `rust-port-start`, starts every list; the lists are in X3 and cover every folder that is still Node or Electron | No freeze or CI check. Each final check (shadow replay M35, the launcher beta L49, each service's cutover, T10) runs after its catch-up, so a missed fix shows up there |
| D16 | ETags | **Dropped**, once the traffic corpus (M2) shows no client sending `If-None-Match` to the metagame. If one does, recreate Express's weak ETags for those routes only | Fewer bytes hashed per response; verified before cutover (M7) |
| D17 | SQLite journal mode | **WAL from the cutover.** The first Rust start sets `journal_mode=WAL` (it stays set in the file); a clean shutdown checkpoints (`wal_checkpoint(TRUNCATE)`). Backups go only through `metagame db backup` (the online backup API, which includes what is still in the WAL). The kits' file-copy backups change in the same release (M45) | Readers no longer block the writer, so the read pool (D10) pays off. A copy of `undaunted.db` alone is no longer a complete backup: the `-wal` and `-shm` files sit next to it. After a forced kill (`Stop-Process -Force`) the next start recovers from the WAL |
| D18 | Discord bot | **Ported to Rust** (S5), last of the services | No Node left anywhere once it ships |
| D19 | Repository layout | **Monorepo by code family** (section 2): `rust/` holds the cargo workspace with every Rust component (launcher backend and page, services, shared crates); `native/` holds the C++ client DLL; `client-mods/` holds the UE4SS mods and their assets; `tools/` holds the TypeScript tools (D20). Each family builds and tests on its own, with its own toolchain and CI path filter (R8). `docs/` stays at the root because GitHub Pages serves `/docs` | A change in one family only runs that family's CI; no C++ or mod files inside the Rust tree and the other way round. Moves use `git mv` so history follows the files |
| D20 | Tools | **`tools/` moves to TypeScript on Bun** (Part E): strict TS, no npm runtime dependencies, Bun built-ins for HTTP, files, hashing, subprocesses and tests. The tools that run on servers (owner dashboard, worker agent, Discord status) ship as one `bun build --compile` executable (T8), so servers need neither Node nor Bun. Exceptions: the PowerShell scripts that drive Windows or the friend kit stay PowerShell, and `xref.py` stays Python (T9) | One TypeScript runtime for the repository's own tooling; the launcher page keeps its Vite build. The Bun analysis findings that apply to scripts (explicit `--env-file`, `.env` autoload off) carry over. **Changed 2026-10-21 (owner's decision):** the servers run the tools on a pinned Bun instead of a compiled `dr-tools.exe`, as long as there are no server release archives to ship one in; see the Part E progress note |
| D21 | Postgres readiness | **Not now, but kept possible.** All queries go through Diesel's DSL, which doesn't depend on the backend, and `store` names its connection type in one alias: a Postgres port swaps it for a `#[derive(MultiConnection)]` enum of SQLite and Postgres (synchronous Diesel supports this; `diesel-async` 0.9 has no `MultiConnection`), so today's binaries don't link `libpq`; SQLite-only pieces stay in `store` behind one module: PRAGMAs, the online backup (M17), Drizzle adoption (D11) and the offline-save tools (X1). A Postgres server would need its own baseline migration and a one-time data transfer, as a separate project | The launcher's offline mode stays on SQLite in any case: no database server on a player's PC |
| D22 | Admin panel | **Not in this repository.** A database management panel (for example on SeaORM Pro) is a separate, private, operator-only project under its own licence. It contains no code from this repository: its entities are generated from the live schema (with SeaORM Pro, `sea-orm-cli generate entity`), independently of the Diesel code here, and it reaches the servers only through the interfaces in X8 | Keeps non-AGPL components out of the AGPL code and its releases; SeaORM Pro's free tier or its single-project Plus licence fits one private project. This repository's only obligation is to keep the X8 interfaces stable |
| D23 | Testing environment | **No staging; a canary.** A small-scale server set on the maintainer's own server, on the Linux kit, with a copy of a production database and invited testers. Every "measure" and "soak" row runs there. Cutover order: canary, then the first Windows region, then the second (M42) | Production runs on Windows, so performance (M36) is also measured on a Windows host like production. The Linux kit gets tested first instead of last |
| D24 | Containers (later goal) | **Outside this plan:** running the game server in Docker/Podman with Proton/Wine. The plan keeps the way open: every service follows X7 (env-file config, logs to stdout, loopback health check), no host paths are compiled in, and the Linux game-worker launch (S6) keeps taking one validated argument payload that a container entrypoint can take over | No container files are written here; the canary on Linux is the first step towards it |
| D25 | Windows code signing | **Not for now** (no budget; roadmap 4.16 stays open). Instead, every release's Windows executables (launcher installer and exe, server binaries, the T8 tools) are submitted to Microsoft's false-positive portal (Security Intelligence file submission) and test-installed on a clean Windows with Defender and SmartScreen on (X6) | Unsigned new executables, and `bun build --compile` output especially, get flagged more often; SmartScreen still asks first-time users to confirm. README keeps its "More info, then Run anyway" note |
| D26 | Security pass | **A dedicated security review before each trust-boundary component ships** (X9), on top of normal review and CI: auth and tokens, TLS pinning, the gateway and relay, the elevated firewall helper, the launcher's key handling, and every parser of untrusted input. Parsers get `cargo-fuzz` targets that run in CI for a short time on every PR and longer before a cutover | One person writes most of the port, so this replaces a second reviewer on the code where a mistake costs the most |

### 3.3 Blockers

| # | Blocker | Why it blocks | Plan |
| --- | --- | --- | --- |
| LB1 | Existing account keys | Keys in `userData/keys` are Electron `safeStorage` blobs: Windows = `v10` AES-256-GCM with the key DPAPI-wrapped in `userData/Local State` (`os_crypt.encrypted_key`); Linux = `v11` AES-128-CBC with a PBKDF2 key from the Secret Service item with `application=Dauntless Revived Launcher` (label "Chromium Safe Storage") (or `v10` with the fixed key on basic_text). Lose this and every friend re-registers. | One-time reader for the Chromium os_crypt formats in Rust (D7); re-store with native DPAPI / Secret Service; keep the old file until the new one reads back. P0 spike done on Windows against a real 0.1.39 profile and on gnome-keyring against Electron 44.4.3 (section 3.4); KWallet and a real Linux profile remain. Fallback if it fails on some system: the existing "import key backup" flow |

| # | Blocker | Why it blocks | Plan |
| --- | --- | --- | --- |
| SB1 | JSON output compatibility | `JSON.stringify` omits `undefined` fields, keeps `null`, prints `1.0` as `1`, and has JS number semantics; `serde_json` prints `1.0` for an `f64` and emits every field unless told otherwise | Integer fields are integer types; `Option` fields that TS left `undefined` get `skip_serializing_if`; a JS-compatible float formatter for the few real floats; JSON diff in M35 compares values *and* text |

### 3.4 Spike results

Run on 2026-10-20 on the maintainer's Windows 11 PC and in WSL (Ubuntu 26.04 with WSLg), from a scratch folder
outside the repository.

| Spike | Rows | Setup | Result | Plan change |
| --- | --- | --- | --- | --- |
| Key read, Windows | L2, D7 | Our own reader on a real 0.1.39 profile | `os_crypt.encrypted_key` starts with `DPAPI`; `CryptUnprotectData` returns a 32-byte key; the key file is `v10` + nonce + AES-256-GCM, the tag verifies, the content is `DRK2` with a fingerprint. About 60 lines on `windows` and `aes-gcm` | None |
| Key read, Linux | L2, LB1 | Electron 44.4.3 on gnome-keyring with a test key, then our reader | Backend `gnome_libsecret`, blob `v11`. The secret is the item labelled "Chromium Safe Storage" with `xdg:schema=chrome_libsecret_os_crypt_password_v2` and `application=Dauntless Revived Launcher`; the `secret-service` crate finds it by those attributes; PBKDF2 + AES-128-CBC decrypts the file | The plan's "`<app> Safe Storage`" item name was wrong; lookup and the 0.3.0 deletion go by attributes (D7, L2, L50a) |
| Webview | L1, L9, L10, L37, L41 | Tauri 2 with isolation, today's built page and a stub `window.launcher`; WebView2 155 on Windows 11 and WebKitGTK 2.52.6 in WSLg; all nine languages | The page runs unchanged on both: 659 CSS rules, 615 DOM nodes, no CSS feature dropped; ja and ru render. Every `invoke` hangs while `index.html` keeps its CSP meta tag, and works with the policy only in `tauri.conf.json`. A command the isolation hook refuses never reaches Rust, and its promise never settles. Art loads from `http://dr-art.localhost/` on WebView2 and from `dr-art://` on WebKitGTK. Canvas: 58 fps on WebView2; in WSLg without a GPU 4 fps, 14 fps with `WEBKIT_DISABLE_DMABUF_RENDERER=1` | CSP meta tag removed (L10); a timeout on every call (L9); font notes (L41) |
| Diesel on the Drizzle schema | M13, M14, M18 | The 31 Drizzle migrations applied to an empty database | Every table has a primary key. `diesel print-schema` gives 51 tables (it skips `__drizzle_migrations`), keeps the camelCase column names, maps every column type, and its output compiles | Adoption checks `hash` and `created_at` (D11) |
| Query overhead | D10, M36 | 10k-row table, primary-key reads, release build, Windows | Sequential, per query: `rusqlite` 6.2 µs, synchronous Diesel 10 µs, `diesel-async`'s `SyncConnectionWrapper` 36–39 µs. 64 tasks on 4 read connections: `diesel-async` with deadpool 40k reads/s; synchronous Diesel in `spawn_blocking` 61k/s with 1 query per hop, 92k/s with 10 | Synchronous Diesel in `store` closures (D10, M13) |
| Transaction cancellation | M16 | A timeout drops the future mid-transaction | `diesel-async`'s `immediate_transaction`: the transaction stays open; other connections get `database is locked`, the same connection `AlreadyInTransaction`. A synchronous transaction in `spawn_blocking` runs to its end, and the next one works | Same as above (M16) |
| One SQLite per binary | M17 | `cargo tree` | Diesel 2.3.14 and `rusqlite` 0.40.2 share `libsqlite3-sys` 0.38.2 | None |
| `diesel-async` and Postgres | D21 | `diesel-async` 0.9.2 source | No `MultiConnection` support | `MultiConnection` only once Postgres is wanted (D21) |
| TLS provider | D10 | `rustls` 0.23 with `aws-lc-rs` on Windows, no NASM or CMake on `PATH` | Builds (`aws-lc-sys` 0.46); the default key exchange starts with X25519MLKEM768 | No NASM step in CI (D10). rustls turns on `aws-lc-sys`'s prebuilt NASM objects through a feature; `jsonwebtoken`'s `aws_lc_rs` feature doesn't, so `rust/.cargo/config.toml` sets `AWS_LC_SYS_PREBUILT_NASM=1` |
| Bun | T1, T5, T8 | Bun 1.4.3 | `Bun.hash.crc32` and a streaming `Bun.CryptoHasher` give the same values as Node's `zlib.crc32` and `crypto`. `bun run` and compiled executables load `.env` from the working directory unless run with `--no-env-file` or built with `--no-compile-autoload-dotenv`. A compiled Windows executable is about 92 MB | One `dr-tools.exe` instead of three (T8) |

Still to run: L1 on Ubuntu 22.04 and 24.04, Fedora, SteamOS and NVIDIA + Wayland, on real hardware; L2 on KWallet
(Plasma 5 and 6) and against a real Linux 0.1.39 profile; L3 on a clean Windows VM; M1 on the canary.

## 4. Cross-project work (X)

| # | Area | What | Why | Waits for | Verify |
| --- | --- | --- | --- | --- | --- |
| X1 | DB tools | `metagame db integrity`, `metagame db gs-key` and `metagame offline-save import\|export` subcommands built on `store` (Diesel; the save import first replays frozen Drizzle SQL for saves from older launchers, D11). They replace `deploy/unix-common/dr-db.cjs`, `deploy/windows-server/lib/dr-db.js` and the launcher's `offline-save-db.cjs` / `register-gameserver-key.cjs` (the launcher calls the same functions in-process) | All of these `require()` `better-sqlite3` from the Node metagame and read migrations from disk | M14, M19 | `dr-db.test.cjs`, `offline-save` and `portable-save-*` cases pass against the Rust tools; a save exported by launcher 0.1.39 imports |
| X2 | Offline switch-over | Launcher 0.2.x turns offline mode back on, now on `--service` modes (D12) | Offline play returns without any Node in the launcher (D1) | The Rust metagame has passed M35 and M37 on the canary, and the Rust DeployServer (S4) its tests and a canary soak; production cutover is not needed | Offline start/stop, save export/import and key registration on both OSes; offline DBs made by the Node metagame open and migrate forward |
| X3 | Catch-up lists (D15) | From `rust-port-start`, one list per component: `legacy/UndauntedMetagame/` (ported in M44); `legacy/UndauntedGateway/`, `legacy/UndauntedContent/`, `legacy/UndauntedDeployServer/`, `legacy/UndauntedDiscordBot/` and the Node helpers in `deploy/` (each before its cutover in Part C); the Electron main process `legacy/UndauntedLauncher/src/main*`, `src/preload.ts`, `runtime/` (before the beta, L49); `tools/` (before T10). Each list is `git log rust-port-start.. -- <folders>` | Fixes made in the old code while its replacement is being written | The tag | Every commit on a list is linked to a ported change or marked not applicable before that component ships |
| X4 | Shared wire types | `crates/protocol` owns invite, server status, registration and manifest types; the launcher's Rust backend (L6) and the servers use it; `ts-rs` generates the launcher page's TS types from it | Today the launcher and the servers each have their own copy | M4 | One change to a status field updates server, launcher backend and page types in one PR |
| X5 | Shared proxy code | `crates/proxy` holds the hop-by-hop list, `Connection`-named header removal and WebSocket tunnelling used by the launcher relay (L22) and the gateway (S2) | The two are mirror images of each other | L22 or S2, whichever lands first | Relay and gateway tests both run against it |
| X6 | CI + release | One workflow graph: `cargo fmt`, `clippy`, `cargo test --workspace`, `cargo deny`, per-target builds, then launcher bundles and server release archives. `SHA256SUMS.txt` and provenance attestations cover everything. `.github/dependabot.yml` gains `cargo` (`/rust`) and `bun` (`/tools`, `/rust/launcher/ui` after R7) entries under the same policy as today's npm ones: monthly, minor and patch grouped per folder, every major update its own PR, a 7-day cooldown; GitHub Actions stay weekly and pinned by commit. Each npm entry is removed when its folder is deleted | One toolchain, one lockfile; `cargo deny` reports known advisories, Dependabot keeps the versions moving | M4 | Dependabot opens `cargo` and `bun` PRs; every release lists hashes for launcher and server binaries; its Windows executables are submitted to the false-positive portal and pass a clean-Windows install with Defender and SmartScreen on (D25) |
| X7 | Service contract | Every service: explicit `--env-file` (no `.env` autoload), typed config validated at start, readiness = HTTP health on loopback, shutdown = `shutdown\n` on stdin then SIGTERM / `CTRL_BREAK`, exit code 0 on clean stop | Server kits, systemd and the launcher drive every service the same way | M25 | The same start/stop test runs against every binary |
| X8 | External admin panel (D22) | Interfaces kept stable for the private panel: the metagame's admin HTTP API (admin-key routes) for every write; read-only database access on the server host (`mode=ro` connection, or a recent backup copy) for browsing; the latest `__diesel_schema_migrations` entry as the schema version the panel checks before it starts; access on loopback only (SSH tunnel, or the gateway with admin authentication) | Writes through the API keep the game's own checks in charge and don't compete with game saves for the write lock (D17); the schema version tells the panel when to regenerate its entities | M14, M42 | Admin API tests; a read-only connection is refused writes; schema version readable without the write pool |
| X9 | Security pass (D26) | Review checklist per component, each with its fuzz targets where it parses untrusted input: metagame auth extractors and JWT (M9, M20) and recovery crypto (M22); body and form parsing (M8) and XMPP (M33, fuzz); the gateway's TLS, policy and proxy (S2) and the launcher relay (L22), with the shared `proxy` crate fuzzed on header and upgrade handling (X5); the elevated allowlist helper and its local IPC (S3); the launcher's pinned TLS (L19), invite parsing (L11, fuzz), legacy key reader and keystore (L2, L27), isolation hook and capabilities (L8, L9); the content server's range handling and manifest checks (S1, fuzz); the Discord bot's command input and state file (S5) | These are the places where a bug lets someone in, takes a key, or crashes a public listener | Each component's first release; fuzz targets added with the parser | Checklist signed off in the component's cutover PR; fuzz targets in CI; no crash in the longer pre-cutover fuzz run |

## 5. Combined order of work

Three tracks: launcher (Part A), metagame (Part B), other services (Part C). The sync points are the X rows.

| Step | Launcher | Metagame | Services |
| --- | --- | --- | --- |
| 1 | P0 spikes (L1–L3; partly run, section 3.4) | B0: tag `rust-port-start` (D15); baseline numbers, traffic capture in the gateway, route inventory, workspace (M1–M4) | Repository: move the C++ DLL, set up `rust/`, CI by family (R1–R4, R8) |
| 2 | P1–P2 skeleton, Rust core (L4–L18), using `protocol` (X4) | B1: `service-kit`, `store`, migrator, auth, logging (M5–M28) | Tools to TypeScript on Bun (T1–T10) |
| 3 | P3–P4 net + platform (L19–L37), `proxy` crate (X5) | B2: domain areas one PR each (M29–M31) | Content (S1) |
| 4 | P5–P7 controller, page, offline orchestration ready but off (L38–L44) | B2 continued; B3 chat (M32–M34) | Gateway + allowlist (S2–S3) |
| 5 | P8–P9: 0.2.0, Squirrel bridge (L45–L50) | B4: Node catch-up, shadow traffic, performance, soak (M44, M35–M37) | DeployServer (S4) |
| 6 | 0.2.x: offline mode back on `--service` modes once the canary has passed (X2) | B5: binaries, server kits, backups, docs, cutover: canary, rollback rehearsal, then the two Windows regions (M38–M43, M45–M47) | Cut over S1–S4 with the metagame |
| 7 | 0.2.x: SolidJS page (P10, L52–L59) | Delete the TS metagame (M43) | Discord bot (S5) |
| 8 | 0.3.0: retire Squirrel feed, delete old key copies (L50a), cleanup (L51); move the launcher to `rust/launcher/` (R7) | — | Delete the TS services; path sweep (R9, R10) |

## 6. Things that must not change

- Launcher: every file outside the launcher (`settings.json`, `keys/` after the one-time re-wrap, `offline/`, the game
  folder, `logs/`); invite formats v1/v2 and the key-to-fingerprint binding; relay port 61000,
  `DAUNTLESS_REVIVED_RELAY_PORT` and the browser-request refusal; the 410-file manifest and the DLL/EXE pins; the page
  has no network, file, shell or URL access; the key never crosses the bridge or reaches logs; all nine UI and nine game
  languages, word for word; the page's look, keyboard behaviour and accessible names through the SolidJS rewrite.
- Servers: the data in `undaunted.db` (adopted in place, D11, with `__drizzle_migrations` left as it is; the journal
  mode becomes WAL at the cutover, D17); every route the game client, the launcher and the bot use, matched
  case-insensitively and with or without a trailing slash; JSON responses equal in value and in the text the client
  parses (SB1); pino log field names; recovery blobs, JWTs and API keys issued by Node keep working; the gateway's TLS
  certificate and therefore every invite's `fp`; the content server's file layout and range behaviour.
- Both: offline saves exported before the migration import after it; the smoke list (under the metagame migration table)
  passes at every deploy step.

## 7. Open questions

None at the moment.

---

## Part A: Launcher → Tauri 2

Scope: replace Electron, Electron Forge and the Node main process with a Tauri 2 app. The renderer (`src/renderer`,
the page-side parts of `src/shared`, the CSS and the brand assets) stays TypeScript built by Vite. Everything in
`src/main` and `src/main.ts` is rewritten in Rust as a member of the workspace (section 2). Windows x64 and Linux
x86_64, as today.

### A1. Target layout

```text
legacy/UndauntedLauncher/
  package.json             Vite, TS, Tauri CLI + JS API/plugins
  index.html               renderer entry
  vite.config.ts           renderer build only
  isolation/               isolation hook → dist-isolation/
  src/
    renderer/              api.ts; SolidJS from P10
    shared/
      i18n/                en.ts … ru.ts + index.ts
      generated/           types + command list from Rust
      format.ts, credits.ts, ui-model.ts,
      status.ts, username.ts, invite.ts
                           kept for display and form feedback
  src-tauri/
    Cargo.toml
    build.rs
    tauri.conf.json
    capabilities/main.json the page's only permissions
    permissions/           one per app command
    src/
      main.rs, lib.rs      builder, plugins, protocol, setup
      commands.rs          one command per LauncherApi method
      core/                invite, status, username, manifest,
                           pins, i18n lookup (authoritative)
      net/                 pinned TLS, http, hostapi, relay,
                           downloader, verify, autoregion
      platform/            keystore, settings, paths, log,
                           launch, linux runtime, game folder,
                           links
      controller.rs        state machine
      offline/             --service runner, save DB
    resources/             the two DLLs
```

The bridge stays a fixed list of typed calls. `src/preload.ts` becomes `src/renderer/api.ts`, a thin wrapper whose
methods call `invoke("<command>", …)` and `listen("<event>", …)` with the same names and types as `LauncherApi`.
`index.ts` keeps calling `api.play()` etc., so the renderer diff is mostly the import. The account key never crosses
the bridge.

Three layers check every call from the page: the isolation hook (command allow-list, argument shape and size), the
capability ACL (only our commands are granted), and the Rust command itself (`serde` types plus today's
`ipc-validate.ts` rules). Anything that gates I/O (invite, username and key checks, status parsing of server data,
settings patches, external targets) is authoritative in Rust; the TS copies in `src/shared` stay only for display.
Both run the same JSON test tables.

### A2. Migration table

Order = PR order within each phase. Every row ends with its ported tests green.

| # | Phase | Area | Today | Tauri replacement | Notes / risks | Verify |
| --- | --- | --- | --- | --- | --- | --- |
| L1 | P0 Spikes | Webview look | Electron Chromium | Bare Tauri app loading today's built renderer with a stub `api.ts` | Check `backdrop-filter`, fonts, `prefers-reduced-motion`, canvas speed in WebKitGTK. Render every language: ja needs a CJK font and ru Cyrillic in the font stack on Linux (fontconfig fallback). First run on WebView2 and WebKitGTK 2.52: section 3.4 | Screenshots match on Win11, Ubuntu 22.04 + 24.04, Fedora, SteamOS (incl. NVIDIA + Wayland, `WEBKIT_DISABLE_DMABUF_RENDERER` fallback) in all 9 languages |
| L2 | P0 Spikes | Key read (D7) | `safeStorage.decryptString` | Windows: read `userData/Local State`, base64-decode `os_crypt.encrypted_key`, strip the `DPAPI` prefix, `CryptUnprotectData` → 32-byte key; blob = `v10` + 12-byte nonce + ciphertext + 16-byte tag → AES-256-GCM. Linux: get the secret from the Secret Service item with `xdg:schema=chrome_libsecret_os_crypt_password_v2` and `application=Dauntless Revived Launcher` (gnome-keyring; its label, "Chromium Safe Storage", is the same for every Electron app, so never search by label) or, on KDE, from KWallet over D-Bus (`org.kde.kwalletd6`, falling back to `kwalletd5`; Electron's "`<app> Keys`" folder), PBKDF2-HMAC-SHA1(secret, `saltysalt`, 1 round, 16 bytes) → AES-128-CBC with an IV of 16 spaces, PKCS#7; `v10` blobs use the fixed `peanuts` password | LB1. Windows and the Secret Service lookup are confirmed (section 3.4). Still to record: the KWallet folder and entry names Electron 44 uses on Plasma 5 and 6 | Decrypt keys saved by 0.1.39 on Windows 11, GNOME (gnome-keyring), KDE Plasma 5 and 6 (KWallet) and SteamOS desktop mode; results match `chromium-safestorage-core` in a test |
| L3 | P0 Spikes | Update bridge | Squirrel | Electron 0.1.x test build that installs a Tauri NSIS build and removes the Squirrel install (D2) | Shortcuts, `dauntless-revived://` handler and Start-menu entry point at the new exe afterwards; userData untouched | Clean VM: install 0.1.39 → bridge → Tauri, keys/settings intact; then one Tauri → Tauri update |
| L4 | P1 Skeleton | Project, CI | Forge, `vite.main/preload` configs, `electron` | `legacy/UndauntedLauncher/src-tauri/` as a workspace member (R5); `npm run tauri dev/build`; renderer Vite config unchanged apart from `server.strictPort` and `envPrefix` | Linux CI needs `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf` | CI green on windows-latest + ubuntu-22.04 |
| L5 | P1 Skeleton | Window | Frameless `BrowserWindow`, custom min/max/close, `onWindowState` | `tauri.conf.json` window with `decorations: false`; drag via `data-tauri-drag-region`; `getCurrentWindow().minimize()/toggleMaximize()`; resize event → `onWindowState` | Close-while-game-runs: `WindowEvent::CloseRequested` + `api.prevent_close()` in Rust, then today's confirmation | Manual on both OSes |
| L6 | P1 Skeleton | Shared types | `shared/types.ts`, `ipc-validate.ts` | `serde` structs/enums mirroring `Snapshot`, `Settings`, `ErrorCode`, `ExternalTarget`, `TaskProgress`, `OfflineStatus`, `Language`, `GameLanguage`; wire types (invite, status, registration, manifest) come from `crates/protocol` (X4); `ts-rs` generates TS types and the command list into `src/shared/generated/` | One source of truth for the bridge; CI fails if the generated files are stale | `cargo test` + `tsc` on the generated types |
| L7 | P1 Skeleton | Bridge | `preload.ts` (`contextBridge`, `ipcRenderer.invoke/on`) | `api.ts` with `invoke`; pushes (`snapshot`, `progress`, `offlineStatus`, `inviteLink`, `windowState`) as `app.emit` events; download progress over a `tauri::ipc::Channel` | Throttle progress (~10/s) in Rust. Command names = today's `IPC` names without `dr:` | `preload-region.test.ts` → `api.test.ts` with `@tauri-apps/api/mocks` |
| L8 | P1 Skeleton | Capabilities | Context isolation, sandbox, no Node | `capabilities/main.json` grants only our app commands (via `permissions/`), `core:event:default` and `core:window:allow-minimize/-toggle-maximize/-start-dragging/-close`. No fs, shell, http, opener or dialog permissions for the page | A new command is denied until it has a permission and is listed | Calling an unlisted command or plugin from devtools (debug build) fails |
| L9 | P1 Skeleton | Isolation hook (D4) | — | `isolation/hook.js`: `window.__TAURI_ISOLATION_HOOK__ = (payload) => …` allows only commands from the generated list, checks argument types and caps string lengths (`boundedString` limits), drops anything else | The hook runs in a sandboxed iframe with its own CSP and no network. Keep it dependency-free and generated-list driven so it can't drift from Rust. A refused call never settles (spike): `api.ts` puts a timeout on every call and reports it as an error | Test: a forged command name or a 1 MB string never reaches Rust |
| L10 | P1 Skeleton | CSP + navigation lock | Strict CSP, `will-navigate` / `setWindowOpenHandler` deny | `app.security.csp` (same policy; Tauri adds nonces/hashes for bundled scripts and the IPC and isolation sources), and the `<meta http-equiv="Content-Security-Policy">` tag is removed from `index.html`: with it, every `invoke` hangs, because its `default-src 'none'` and `connect-src 'self'` block the isolation frame and the IPC endpoint (spike); `on_navigation` allows only the app origin; new windows blocked; release builds without the `devtools` feature; context menu disabled in the page | Electron fuses have no equivalent need: no Node in the app process, assets embedded in the binary | Try `window.open`, `location =`, `<a target=_blank>` |
| L11 | P2 Core | Invite parsing | `shared/invite.ts` | `core::invite` with `url` + `ipnet` (100.64.0.0/10) | Exact accept/reject set (v1 Tailscale/`*.ts.net`/loopback only, v2 needs `fp`). Cases move to `test/cases/invite.json`, read by `cargo test` and `node:test` | Both implementations pass the same table |
| L12 | P2 Core | Status, username | `shared/status.ts`, `shared/username.ts` | `core::status`, `core::username` | Same shared-case approach | `username-status` cases in both |
| L13 | P2 Core | i18n split (D5) | `shared/i18n.ts`: 9 language objects in one 4.4k-line file, `LANGUAGES` in `types.ts` | `src/shared/i18n/<lang>.ts`, one file per language, moved verbatim; `index.ts` builds `STRINGS` from them; `LANGUAGES` derives from the same registry | Pure move, no text changes; diff of `STRINGS` before/after must be empty | `ui.test.ts` parity test (same keys, same `{placeholders}`, no empty text) unchanged and green |
| L14 | P2 Core | i18n in Rust | `translate()` 6× in main, `systemLanguage(app.getLocale())` | A build step exports `STRINGS` to `src-tauri/gen/strings.json`; `build.rs` generates the `Language` enum and a lookup for the backend's strings (dialogs, errors). System language from `sys-locale` → same `systemLanguage` rule (first two letters, else `en`) | Rust never hard-codes a language list; a new language appears in Rust by rebuilding | Rust test: every key Rust uses exists in every language |
| L15 | P2 Core | Adding a language | Edit `types.ts`, `i18n.ts`, and the settings menu | Checklist in `src/shared/i18n/README.md`: add `<lang>.ts` (copy of `en.ts`), add the code to the registry, optionally map it to a `GameLanguage`. Nothing else | The parity test keeps every shipped language complete. No new strings are written in this migration | Add a throwaway language in a test branch; only those two files change |
| L16 | P2 Core | Game language | `engineini.ts` writes the game-client culture from `gameLanguage` | Same mapping in Rust (`auto` → from the UI language when the game supports it, else `en-US`) | All 9 `GAME_LANGUAGES` unchanged | `engineini-launch` port |
| L17 | P2 Core | Game manifest | `@game-manifest` import of `legacy/UndauntedContent/data/dauntless-1.4.4.json` | `build.rs` validates it, `include_bytes!` + `serde_json` at start, fingerprint as a const | Build fails on a malformed manifest (like `GAME_MANIFEST_PROBLEM`) | `manifest.test.ts` port |
| L18 | P2 Core | Pins + DLLs | `constants.ts`, `assets/*.dll` | `const` items; DLLs as `bundle.resources`, SHA-256 checked in `build.rs` and before every copy/launch | Same pins as `friend-kit/play.ps1` | `dlls` hash test |
| L19 | P3 Net | Pinned TLS | `pinned.ts` | `rustls` (`aws-lc-rs` provider, D10) with a custom `ServerCertVerifier` comparing SHA-256 of the leaf DER to the invite `fp`; shared `tokio-rustls` connector | Copy today's semantics exactly. Pin errors stay distinguishable for the "not your friend's server" message. **Not** `tauri-plugin-http`: the page gets no network access | `pinned.test.ts` with `rcgen` certs: right, wrong, rotated |
| L20 | P3 Net | HTTP client | `http.ts` | `reqwest` (rustls, no default features), pinned for v2, plain HTTP for v1; `CONNECT_TIMEOUT_MS`; `redirect::Policy::none()` | Only the invite's host | `controller` public-flow test |
| L21 | P3 Net | Host API, news, art | `hostapi.ts` | `net::hostapi` | Response size limits as today | Covered by controller tests |
| L22 | P3 Net | Relay | `relay.ts` (node:http server + pinned upstream, WebSocket upgrade) | `hyper` 1.x server on `127.0.0.1:61000` with the shared `crates/proxy` header and tunnelling rules (X5) (`DAUNTLESS_REVIVED_RELAY_PORT`), `hyper` client over the pinned connector with keep-alive; upgrades via `hyper::upgrade::on` + `tokio::io::copy_bidirectional` | Keep hop-by-hop stripping, `Connection`-named header drop, `sec-fetch-*`/Origin refusal, `port_busy`, streaming bodies. Riskiest single port | `relay.test.ts` port (headers, bodies, streaming, keep-alive, WebSocket, wrong cert) |
| L23 | P3 Net | Downloader | `downloader.ts` (resume, verify, repair, 4 parallel, pause/resume/cancel) | `tokio` + `reqwest` streaming, `Range` resume into `.part`, `sha2` while writing, `Semaphore(DOWNLOAD_CONCURRENCY)`, `CancellationToken` + `watch` for pause; free space via `fs4` + `DISK_MARGIN_BYTES` | — | `downloader.test.ts`, `cdn-download.test.ts` ports |
| L24 | P3 Net | Verify | `verify.ts` | `spawn_blocking` (or `rayon`) hashing of the 410 files with a 1 MiB buffer | — | `verify` tests + one full 11 GB run |
| L25 | P3 Net | Autoregion | `autoregion.ts` | TCP connect timing (`tokio::net::TcpStream` + `Instant`) | — | `autoregion.test.ts` port |
| L26 | P4 Platform | Paths | `app.getPath("userData" / "documents" / "home")` | Tauri's `app_data_dir()` would be `…/io.github.mixutin.dauntlessrevived`: **not used**. Hard-code Electron's userData (`%APPDATA%\Dauntless Revived Launcher`, `~/.config/Dauntless Revived Launcher`); `dirs` for Documents/home | Same dirs = settings, keys, `logs/`, `offline/` keep working | Fresh vs. upgraded profile both start |
| L27 | P4 Platform | Keystore | `keystore.ts` + `safeStorage` | `Encryptor` trait kept; Windows: DPAPI (`CryptProtectData`, user scope); Linux: Secret Service via `keyring` (Plasma 6's KWallet serves that API too; on Plasma 5 without it, store through KWallet over D-Bus so KDE users don't lose key storage); reads legacy Electron blobs once through `keystore/legacy.rs` (D7, row L2), writes them in the new format, reads the new copy back, and only then marks the old file migrated; it does not delete it (D7) | Keep the fingerprint binding and `rebind()` confirm flow; keep "unavailable" when Linux has no secret store. A key that can't be migrated (locked or replaced keyring) shows the "import key backup" flow for that server, never a silent re-register | `keystore.test.ts` port; legacy-blob fixtures from real 0.1.39 profiles (Windows, gnome-keyring, KWallet) decrypted by both our reader and `chromium-safestorage-core` (dev-dependency) with equal results |
| L28 | P4 Platform | Settings | `settings.ts` | `serde_json`, atomic write (temp + rename), `#[serde(rename_all = "camelCase")]`; unknown `language` values fall back to `systemLanguage` | Byte-compatible with today's `settings.json` | Load a 0.1.39 file, save, diff |
| L29 | P4 Platform | Log | `log.ts` (`addSecret` masking, file log) | `tracing` + `tracing-appender` in `userData/logs` with a masking `Layer`; panic hook through the same layer | Page-side logs, if any, go through the same mask | A registered key never appears in the log file |
| L30 | P4 Platform | Game folder, Engine.ini, client mods, display repair | `game-folder.ts`, `engineini.ts`, `client-mods.ts`, `display-repair.ts` | `std::fs` + an INI writer that keeps unknown lines | — | `game-folder`, `client-mods`, `display-repair` ports |
| L31 | P4 Platform | Launch (Windows) | `launch.ts`, `windows-runtime.ts`, `findRunningClients` | `std::process::Command` (`CREATE_NO_WINDOW` unless `showConsole`), `sysinfo` for running clients, VC++ runtime check | Same arguments; key masked in the log | `engineini-launch`, `windows-runtime` ports |
| L32 | P4 Platform | Launch (Linux) | `linux-runtime.ts`, `linux-dependencies.ts` | Proton/Wine discovery + prefix setup via `Command`, `which` | Keep the `LinuxRuntimeMissingError` UX | `linux-runtime`, `linux-dependencies` ports |
| L33 | P4 Platform | Tailscale, device signals, system | `system.ts`, `device-signals.ts` | `Command` / `sysinfo` / `windows` crate | — | `device-signals`, `device-check` ports |
| L34 | P4 Platform | Invite links + single instance | `setAsDefaultProtocolClient`, `requestSingleInstanceLock`, `second-instance` | `tauri-plugin-deep-link` (`desktop.schemes = ["dauntless-revived"]`) + `tauri-plugin-single-instance` with its `deep-link` feature | `takeInviteLink` / `onInviteLink` keep their shape. Linux: `.desktop` `MimeType` (deb/rpm) or `register_all()` at runtime (AppImage) | Click an invite with the launcher closed and open, both OSes |
| L35 | P4 Platform | External links | `links.ts` allow-list + `shell.openExternal` | `tauri-plugin-opener` used **from Rust only** behind the same `ExternalTarget` → fixed URL map | The page passes an enum, never a URL | `links.test.ts` port |
| L36 | P4 Platform | Dialogs | `dialog.showOpenDialog/SaveDialog/MessageBox` | `tauri-plugin-dialog` called from Rust; button and title text from the Rust i18n lookup (row L14) | Default paths: Documents, as today | Manual, in two languages |
| L37 | P4 Platform | Host art | `dr-art://` privileged scheme via `protocol.handle` | `register_asynchronous_uri_scheme_protocol("dr-art", …)` serving from the controller's art cache | WebView2 serves it as `http://dr-art.localhost/…`, WebKitGTK as `dr-art://…`: one TS URL helper; both forms in `img-src` (both confirmed by the spike) | Art pack shows on both OSes |
| L38 | P5 Controller | State machine | `controller.ts` (1.7k) | `Controller` actor task with a command channel; `commands.rs` forwards to it; `app.manage(ControllerHandle)` | Port last in P5 so every dependency exists; `Snapshot` shape unchanged | `controller.test.ts` port (whole public-mode flow) with fakes from `test/fakes.ts` |
| L39 | P5 Controller | Status polling | `setStatusPolling`, `STATUS_POLL_MS` | `tokio::time::interval`, paused via `watch` when hidden or the game runs | — | Controller test |
| L40 | P6 Renderer | API swap | `window.launcher` (global from preload) | `import { api } from "./api"`; remove `global.d.ts` | Mechanical | `ui.test.ts`, `mods-page.test.ts`, `brand.test.ts` keep running on Node |
| L41 | P6 Renderer | WebKit differences | Chromium-only CSS/JS | Fix what row L1 finds on the real distros. The spike found no CSS that WebKitGTK 2.52 drops (`backdrop-filter`, `:has()`, `scrollbar-*`, `color-mix()`, `text-wrap` all work). The font stacks (`Bahnschrift`, `Segoe UI`, `Cascadia Mono`) name only Windows fonts, so Linux shows the distro's sans-serif, as the Electron Linux build does; ja needs any CJK font through fontconfig, so deb/rpm recommend one (`fonts-noto-cjk`, `google-noto-sans-cjk-fonts`) | Fixes in the shared CSS, not per-OS forks | Visual check on both webviews, all languages |
| L42 | P7 Offline | Offline mode off in 0.2.0 (D1) | `utilityProcess.fork` + `local-service-runner.cjs` on Electron's Node | Offline mode behind a Cargo feature `offline`, off in 0.2.0; the offline page says it returns in a later update; nothing in `offline/` or the player's saves is touched | Players lose offline play from the bridge release until X2 | The offline page shows the notice; `offline/` is byte-identical after the update |
| L43 | P7 Offline | Offline orchestration | `offline-save*.ts`, `offline-state.ts`, `offline-game-config.ts`, `local-stack*.ts` | Ported to Rust and wired to the `--service` modes (D12) and the metagame crate's save import/export and key registration (X1); compiled in but off until X2 | The `.cjs` helpers are not ported: the metagame crate replaces them (X1) | `offline-save`, `offline-runtime`, `local-stack` ports run against the Rust services; `portable-save-*` probes against the fixture |
| L44 | P7 Offline | Staging script removed | `scripts/stage-offline-runtime.mjs` (`@electron/rebuild` for `better-sqlite3`) | Deleted: the services are part of the launcher binary, nothing to stage | — | Packaged offline smoke runs against the `--service` modes |
| L45 | P8 Ship | Windows package | Squirrel installer + portable zip | `bundle.targets = ["nsis"]` (+ zip of the exe for portable), `installMode: currentUser`, `embedBootstrapper` (D6); signing hook ready for roadmap 4.16 | Unsigned for now (D25): false-positive submission each release | Install, update, uninstall on a clean VM |
| L46 | P8 Ship | Updater (D2) | `update-electron-app`, Squirrel `RELEASES` in `launcher-updates` | `tauri-plugin-updater` with `createUpdaterArtifacts: true`; public key in `tauri.conf.json`; `latest.json` + signed NSIS/AppImage bundles published to `launcher-updates`; hourly check as today, install on user consent (`installUpdate`) | Private key + password as repo secrets used only by the release job; the maintainers keep a private copy. Prereleases never reach `launcher-updates` | Tauri → Tauri update on Windows and AppImage; a bundle with a wrong signature is refused |
| L47 | P8 Ship | Linux packages | Forge AppImage/deb/rpm/zip | `bundle.targets = ["appimage", "deb", "rpm"]` + tarball | deb `depends: libwebkit2gtk-4.1-0`, rpm `webkit2gtk4.1`; AppImage bundles WebKitGTK. deb/rpm update through "download a newer release" (today's `linux-updater.ts` check, ported) | Install on Ubuntu 22.04/24.04, Debian 12, Fedora, Arch (AppImage), SteamOS |
| L48 | P8 Ship | Release workflow | `ci.yml` launcher jobs, `launcher-release.yml` | Same jobs on `tauri build` (or `tauri-action`); keep `SHA256SUMS.txt`, provenance attestations, the version gate (version from `tauri.conf.json`/`Cargo.toml`, checked equal to `package.json`) | `LAUNCHER_AUTO_RELEASE` unchanged | Dry-run release on a fork |
| L49 | P9 Cutover | Beta | — | Tauri 0.2.0-beta as a GitHub prerelease; testers install it over 0.1.x by hand | After the Electron catch-up list (X3) is done; only after a week with no key, save or language regressions | Checklist per OS |
| L50 | P9 Cutover | Bridge release (D2) | — | Electron 0.1.40 to the Squirrel files in `launcher-updates`: on its next start it fetches the pinned 0.2.0 installer, checks the hash, runs it, removes the Squirrel install; 0.2.0 then updates to the newest 0.2.x through `latest.json` in the same release | Fails safe: if the download or hash check fails it stays on 0.1.40 and links the release page. The Squirrel `RELEASES` + `.nupkg` and the Tauri `latest.json` sit side by side until 0.3.0 | Row L3 rerun against the real release; a 0.1.39 install reaches the newest 0.2.x in two hops |
| L50a | P9 Cutover | Retire the Squirrel feed | — | With the 0.3.0 release: remove `RELEASES` and the `.nupkg` files from `launcher-updates`; the release workflow stops publishing them. 0.3.0 on first start deletes the old Electron key copies marked migrated (D7): the `keys/` blobs, `Local State`, and the Secret Service item / KWallet entry; unmigrated ones are left alone. The Secret Service item is deleted by its attributes (D7), never by its label, which every Electron app's item shares | README/README.fi: 0.1.x installs reinstall from the release page; no rollback to Electron from 0.3.0 | 0.1.39 finds no update; 0.2.x still updates; after 0.3.0 starts, only new-format keys remain and all still work |
| L51 | P9 Cutover | Cleanup | `src/main*`, `src/preload.ts`, `runtime/*.cjs`, Forge/Vite main+preload configs, `electron*` deps | Delete; update `README.md`, `README.fi.md`, `LINUX.md` (supported distros per D3, all languages per D5), `THIRD-PARTY-NOTICES.txt` (crate licenses via `cargo about`), Credits data | AGPL-3.0-only stays; `cargo deny` checks crate licenses | `cargo deny check` in CI |
| L52 | P10 SolidJS | Setup | Vanilla TS with `h()` / `s()` helpers (`dom.ts`) | `solid-js` + `vite-plugin-solid`; `.tsx` with `jsx: "preserve"`, `jsxImportSource: "solid-js"` | Solid compiles JSX to plain DOM calls (no `eval`), so the CSP and the isolation hook stay as they are | Build and start under today's CSP |
| L53 | P10 SolidJS | State | `latest` snapshot + `renderAll()` re-building regions on every `onSnapshot` | One `createStore<Snapshot>` updated with `reconcile()` from `api.onSnapshot`; download progress as its own signal (already throttled in Rust, L7); menus, modals and settings tabs as local signals | Fine-grained updates instead of rebuilding regions: focus, scroll and open menus survive a snapshot | Replay a recorded snapshot sequence; no DOM node outside the changed fields is replaced |
| L54 | P10 SolidJS | Components | 14 `render*` functions in `index.ts` (~2.5k lines) | One component per function: `Rail`, `PlayPage` (`ActionBar`, `Steps`, `Progress`, `PlayMenu`), `Panel`, `News`, `Partners`, `Mods`, `Server`, `Settings`, `Credits`, `Banners`, `Modal`. Ported one page per PR: Solid's `render()` mounts a finished component into its existing container while the rest stays vanilla | **Same markup**: element types, ids, classes and `aria-*` attributes as today, so both stylesheets and accessibility need no change. Text only as JSX children (escaped, like `h()`); never `innerHTML` | Screenshot diff per page on WebView2 and WebKitGTK in all nine languages |
| L55 | P10 SolidJS | Text | `data-i18n` / `data-i18n-aria` / `data-i18n-alt` pass over `index.html`, `tk()` | `t()` reading a language signal, so a language change re-renders only text | Keys and strings unchanged | `ui.test.ts` parity test; switch language at runtime |
| L56 | P10 SolidJS | Behaviour | Document listeners (outside-click `mousedown`, settings tab focus, keyboard handling) added and removed by hand | Same handlers in `onMount` / `onCleanup` per component | Listener leaks are the usual regression here | Keyboard-only walk through every page; listener count stable after opening/closing menus 100× |
| L57 | P10 SolidJS | Background scene | `scene.ts` canvas | Stays imperative, wrapped in a `Scene` component: starts in `onMount`, stops in `onCleanup`, pauses from the snapshot's game-running state and on reduced motion | No rewrite of the drawing code | Holds still while the game runs |
| L58 | P10 SolidJS | Tests | `node:test` on shared logic; `ui.test.ts` scans `src/renderer` for used keys | Key scan includes `.tsx`; component tests with `vitest` + `@solidjs/testing-library` + `happy-dom` (dev only); shared-logic tests stay on `node:test` | Vitest reuses the Vite config the page already builds with | All tests green; each component has a render test |
| L59 | P10 SolidJS | Cleanup | `dom.ts`, `renderAll()`, the vanilla `render*` functions | Delete once the last page is ported | — | No import of `dom.ts` left |

### A3. Phases

| Phase | Content | Depends on |
| --- | --- | --- |
| P0 | Spikes: look + fonts in all languages, key read, update bridge (L1–L3); go/no-go. Partly run (section 3.4) | — |
| P1 | Tauri skeleton, bridge, capabilities, isolation, CSP (L4–L10) | P0 |
| P2 | Rust core, i18n split and export, manifest, pins (L11–L18) | P1 |
| P3 | Pinned TLS, HTTP, relay, downloader, verify (L19–L25); the relay is the riskiest code | P2 |
| P4 | Platform layer (L26–L37); parallel with P3 | P2 |
| P5 | Controller (L38–L39) | P3, P4 |
| P6 | Renderer swap + WebKit fixes (L40–L41) | P1 (fake backend), P5 (real) |
| P7 | Offline mode off in 0.2.0, Rust orchestration ready for X2 (L42–L44) | P5 |
| P8 | Packaging, updater, release (L45–L48) | P6 |
| P9 | Beta, bridge release, cleanup (L49–L51) | P8 |
| P10 | SolidJS page (L52–L59), shipped in 0.2.x releases | P9 |

P3 and P4 can run in parallel, and P6 can start on a fake backend right after P1. 0.2.0 ships without offline mode
(L42).

Offline mode returns with the Rust services in X2 (section 4).

---

## Part B: Metagame → Rust

Scope: port `legacy/UndauntedMetagame` (~19.8k lines in 90 files: 47 controllers, 19 route files, 12 middleware, 5 realtime
modules) to Rust on the shared crates, with identical behaviour for every client, and make it as fast as practical.
Rows marked *(from the Bun analysis)* carry over compatibility findings from that earlier Bun plan.

### Metagame migration table

Order = PR order within each phase. Every row ends with its tests green.

| # | Phase | Area | Today | Rust replacement | Notes / risks | Verify |
| --- | --- | --- | --- | --- | --- | --- |
| M1 | B0 Groundwork | Baseline | — | Measure the Node metagame on the canary (D23) with a production DB copy, next to the live Windows servers' numbers from the owner dashboard: p50/p99 per route, throughput, RSS, startup time, event-loop delay | The numbers the Rust port is judged against | Numbers recorded in this file |
| M2 | B0 Groundwork | Traffic capture (D13) | `legacy/UndauntedGateway` `feed.ts` / `redact.ts` | Recording in three places, all with the existing redaction rules (account keys, tokens, passwords, recovery data): the gateway (player and launcher HTTP); the metagame's own listener (game-server calls on game-server-key routes, which use loopback or Tailscale and never pass the gateway); and chat sessions (the XMPP stanzas of each WebSocket in order, with timing) | Opt-in per server, off by default. The corpus is personal data under GDPR (usernames, IP addresses, play history): stored only on the operator's machines, a retention limit set by the operator, deleted after the cutover (M43); the privacy section of `docs/legal.md` and its Finnish copy says that recording may run | Corpus from a play session replays against Node with no diffs; no key, token or password appears in it |
| M3 | B0 Groundwork | Route inventory | 19 route files, Express routers | Script that dumps every route (method, path, middleware chain, auth kind) from the Express app into `routes.json` | The Rust router test checks that every entry exists with the same auth | `routes.json` checked in; CI fails on drift until cutover |
| M4 | B0 Groundwork | Workspace | npm per project | `rust/Cargo.toml`, `rust/crates/protocol`, CI jobs (X6, R4) | Node projects keep building next to it until each is cut over | CI green |
| M5 | B1 Foundations | HTTP server | `express()` + `app.listen` | `axum` on `tokio` via `service-kit`: bind host/port, 50 MB body limit, timeouts, graceful shutdown | No 255 s idle cap like Bun: keep today's long requests (backup, matchmaking waits) as they are | `shutdown.test.ts` behaviour |
| M6 | B1 Foundations | Routing compatibility | Express: case-insensitive, trailing slash ignored, `:params`, auto `HEAD` | Own matcher in `service-kit` on lowercased path segments, taking `:param` values from the original segments; `HEAD` answered through `GET` *(from the Bun analysis)* | `axum`'s router is case-sensitive, so the normalisation happens before it; params keep their case (usernames, ids) | Every `routes.json` entry matches in upper/lower/trailing-slash variants |
| M7 | B1 Foundations | Response compatibility | `res.json/send`, weak ETags + 304, `X-Powered-By`, default 404/500 HTML | `application/json; charset=utf-8` exactly as Express sends it; no `X-Powered-By`; no ETags (D16); empty 500 body | *(from the Bun analysis)* Before dropping ETags, search the corpus (M2) for `If-None-Match` and 304s | Corpus check recorded; header diff (M35) |
| M8 | B1 Foundations | Body parsing | `express.json({limit: "50mb"})`, `express.urlencoded({extended: true})` | `Json<T>` / `Form<T>` by `Content-Type`; flat form fields only; keep the `JSON_ERROR_ROUTES` 400 on bad JSON | Check the OAuth token route only sends flat fields *(from the Bun analysis)* | Login tests with form and JSON bodies |
| M9 | B1 Foundations | Middleware → extractors | `TrackBackendHealth`, ban check, `RefuseProxiedInDevAuthMode`, request log, `BodyLog`, `PlayerAuth`, `GameServerKeyAuth`, `HasUndaunted*`, `SoftAccountAuth`, `RealProgressionOnly`, `GameServerOnly` | `tower` layers for cross-cutting ones (health, log, body log, ban, dev-auth refusal); typed extractors for auth (`Player`, `GameServer`, `AdminKey`, `UserKey`, `SoftAccount`) so a handler without the right extractor doesn't compile | Route table (M3) says which extractor each route needs | `permissions.test.ts`, `backendhealth.test.ts` cases |
| M10 | B1 Foundations | Client address | `req.socket.remoteAddress` / `localAddress` | `ConnectInfo` with a custom `Connected` impl that captures both peer and local address | Rust keeps per-connection `localAddress`, which Bun didn't | `RequestOrigin` + gateway tests for loopback, LAN, gateway, `0.0.0.0` binds |
| M11 | B1 Foundations | Rate limits | `express-rate-limit` (8 limiters) | Fixed-window limiter in `service-kit` (sharded `DashMap`, sweep task), draft-8 `RateLimit` / `RateLimit-Policy` headers, IPv6 /56 keys *(from the Bun analysis)* | Same limits and keys as today | Rate-limit tests |
| M12 | B1 Foundations | Store images | `res.sendFile(..., {dotfiles: "deny"})` | `tower-http` `ServeFile` after today's name check | Images stay a directory next to the binary | Store image tests |
| M13 | B1 Foundations | Database access | `better-sqlite3` via Drizzle | `store`: synchronous Diesel on `r2d2` pools (a one-connection write pool and a read pool), PRAGMAs set by a `CustomizeConnection` hook on every new connection; `store::read` / `store::write` run a closure on the blocking pool (D10); `schema.rs` from `diesel print-schema` on the adopted database (51 tables at today's Drizzle state, column names such as `accountId` kept as they are under `#[allow(non_snake_case)]`); models with `Queryable` / `Selectable` / `Insertable`; PRAGMAs as today | WAL (D17). `NULL` → `Nullable` / `Option<T>`, BLOB → `Vec<u8>`, booleans stored as integers, JSON columns as text: model types match the columns, not the TS types, and Diesel refuses a mismatch at compile time | Controller tests; schema snapshot (M18) |
| M14 | B1 Foundations | Migrations (D11) | Drizzle migrator reading `src/drizzle` | `diesel_migrations` with `0001_baseline` (`up.sql` / `down.sql`), embedded; `store::adopt` for existing databases; frozen Drizzle SQL embedded only for offline databases and saves from older launchers | The baseline is cut from the Drizzle state after the catch-up (M44), so it includes any migration added during the port | Adopt both regional databases' copies: data unchanged, `__diesel_schema_migrations` holds the baseline; a fresh database built from the baseline equals the snapshot |
| M15 | B1 Foundations | Raw queries | `db.prepare(sql).get/all/run` in 9 controllers | Diesel DSL where it expresses the query; `diesel::sql_query` with `QueryableByName` for the rest, kept in `store`; `.get()` → `.optional()` → `Option` | *(from the Bun analysis)* Watch every `=== undefined` branch. SQLite-only SQL marked for D21 | Controller tests |
| M16 | B1 Foundations | Transactions | `db.transaction(fn)`, Drizzle sync transactions (~20 controllers) | `store::write(\|conn\| …)`: Diesel's `immediate_transaction` (`BEGIN IMMEDIATE`) on the write connection inside `spawn_blocking`; the closure is synchronous and receives only the connection, so nothing else can be awaited inside (D10) | A dropped request future doesn't stop the closure: the transaction commits or rolls back by itself, as `better-sqlite3`'s do today. In the spike, a cancelled `diesel-async` transaction stayed open and locked out every other writer | `inventorytx`, store tests; a test that cancels a request mid-transaction and then writes from another request |
| M17 | B1 Foundations | Live backup | `client.backup(dest, {progress})` | SQLite's online backup API through a separate `rusqlite` connection (`backup` feature), copying N pages per step with short sleeps so saves keep going; 15-minute deadline | Diesel has no backup API. `rusqlite` and Diesel must resolve to the same `libsqlite3-sys` version (one bundled SQLite per binary); `cargo deny` bans a second copy (today Diesel 2.3.14 and `rusqlite` 0.40.2 both resolve to `libsqlite3-sys` 0.38.2). Better than the Bun plan's `VACUUM INTO`: no long lock | Backup of a multi-GB copy with save latency measured during it |
| M18 | B1 Foundations | Schema snapshot | Drizzle TS schema | Checked-in `sqlite_schema` snapshot of a database at the latest Drizzle migration; tests compare it with a database built from `0001_baseline` and with an adopted one | Becomes the reference once the TS goes | Snapshot equal in both cases |
| M19 | B1 Foundations | DB tools (X1) | `dr-db.cjs`, `dr-db.js` | `metagame db …` subcommands (`clap`) | Server kits switch to them in M39–M41 | `dr-db.test.cjs` cases |
| M20 | B1 Foundations | JWT RS256 | `jsonwebtoken` (npm) | `jsonwebtoken` crate, `Validation` pinned to RS256, same `iss` / `aud` / `exp` rules, no clock skew (as today) | Negative tests from the Bun analysis: `alg: none`, HS256 with the public key, wrong `iss`/`aud`, expired, bad signature | Tokens issued by Node verify; forged ones don't |
| M21 | B1 Foundations | Hashing, random | `createHash("sha256")`, `randomUUID`, `randomBytes`, `randomInt` | `sha2`, `uuid` v4, `rand::rngs::OsRng` | Same hex formats | Equality tests against values from today's code |
| M22 | B1 Foundations | AES-256-GCM | `createCipheriv` / `createDecipheriv` | `aes-gcm` | Keep the stored layout (tag and IV separate) | Decrypt a recovery blob written by Node |
| M23 | B1 Foundations | Constant-time compare | `crypto.timingSafeEqual` | `subtle::ConstantTimeEq` | — | — |
| M24 | B1 Foundations | Logging | `pino` (402 call sites) | `tracing` with a JSON formatter writing pino's fields (`level` numbers, `time`, `msg`, `err`, `pid`, `hostname`); `LOG_LEVEL`; pretty output in dev | Discord bot and log tooling keep parsing it *(from the Bun analysis)* | Line diff old vs new for sample events |
| M25 | B1 Foundations | Config (X7) | `dotenv`, `process.env` reads across files | `dotenvy` reading only the `--env-file` given, into one typed `Config` validated at start | A typo in an env var fails at start instead of at first use | Start with today's `.env.example` |
| M26 | B1 Foundations | Vendor JSON, progression config | `resolveJsonModule` imports, `PROGRESSION_CONFIG_DIR` | `include_bytes!` + `serde` for vendor data; progression config read from disk at start, as today | Parse once into typed structs; no per-request JSON parsing | `CheckProgressionConfig` cases |
| M27 | B1 Foundations | Backend health | `monitorEventLoopDelay`, `performance` | Request timing with `Instant`; `tokio-metrics` runtime stats for the health report, under the field names consumers read today | No event loop in Rust: map the fields, don't invent new semantics | `backendhealth.test.ts` cases |
| M28 | B1 Foundations | Events | `EventEmitter` (`KeyRevocations`) | `tokio::sync::broadcast` | — | `keyrotation.test.ts` cases |
| M29 | B2 Domain | Controllers by area | 47 controllers, 19 route files | One PR per area, in this order: accounts/auth/login/apikeys → character/activecharacter/loadout/inventory/entitlements/savehistory → store/freestore/storeclock/storerevocation/shoplayout/ramspricing → progression*/realprogression/bounties/trials/cooldowns/capacityturns → matchmaking/matchmakingroster/party/friends/guild/huntregion/regionprobes/regionpreferences → moderation/escalation/devicesignals/unlocker-detection/keyrotation/accountrecovery/discordlinks/discordstats/slayerlinks/creator/middleman → undauntedapi/serverstatus/system/tuning/eos/backup | The Rust server serves only finished areas in shadow runs; each PR also ports that area's replay subset (M35) | Area tests + replay subset with no diffs |
| M30 | B2 Domain | Deep equality | `isDeepStrictEqual` | `PartialEq` on typed structs, `serde_json::Value` equality where inputs are free-form | — | `character.test.ts` cases |
| M31 | B2 Domain | Tests | 69 `node:test` files | First: run the existing HTTP-level TS tests (`test/appclient.ts`) against the Rust binary, so the same suite checks both servers. Then port them to Rust integration tests (temp DB per test, `tower::ServiceExt::oneshot`, a spawned server for chat) | Running one suite against both servers is the strongest equivalence check before the TS goes | Same pass count on both servers |
| M32 | B3 Chat | WebSocket server | `node:http` + `ws` (`noServer`) | `axum` WebSocket upgrade (`tokio-tungstenite`) on the same separate port; frame limit, no deflate; bounded per-session send queue (drop + close when full) | Shutdown: close XMPP streams, ≤1 s wait, then stop | `chat*.test.ts`, `presence.test.ts` cases |
| M33 | B3 Chat | XMPP parsing | `ltx` | `quick-xml` streaming reader + small stanza types for the XMPP subset the game uses | Untrusted input: `cargo-fuzz` target on the parser before cutover | `chatwire` cases; fuzzing finds nothing in 24 h |
| M34 | B3 Chat | Rooms, presence | Per-session loops in `muc.ts`, `presence.ts` | One `broadcast` channel per room for identical stanzas; per-recipient sends otherwise | `chatinvariant` checks | `chatinvariant`, `presence` cases |
| M44 | B4 Verify | Node catch-up (D15) | Node commits since `rust-port-start` | Port every commit from the metagame catch-up list (X3) to Rust, ticking each off in the PR; then cut the final corpus and run M35 | Runs right before the final M35 replay; any change to the Node metagame after that waits for the Rust version | Every commit in the list linked to a Rust change or marked not applicable |
| M35 | B4 Verify | Shadow traffic (D13) | — | Replay the corpus (M2) against Node and Rust from the same DB snapshot: HTTP from both capture points, and the recorded chat sessions stanza by stanza (order and content of what each client receives); diff status, normalised headers, JSON values and JSON text (SB1); triage every diff | Ship gate: no unexplained diffs | Diff report empty or every entry accepted in review |
| M36 | B4 Verify | Performance | M1 numbers | Same load on Rust, on the canary and on a Windows host like production (D23); profile (`perf`, `cargo flamegraph`; ETW on Windows), measure the cost of the blocking-pool hop per `store` closure on production queries (the spike's numbers in section 3.4 come from a development PC); move hot queries to raw SQL where it pays; optionally PGO (section 2) | Ship gate: no route slower than Node at p99 | Numbers recorded next to M1 |
| M37 | B4 Verify | Soak | — | Rust metagame on the canary for several days with invited players | Watch memory, file handles, WebSocket counts | No growth, no panics in the log |
| M38 | B5 Ship | Binaries | `dist/` + `node_modules` | Release builds for the D14 targets; store images and progression config stay next to the binary | One file per platform, no runtime | Binary passes the smoke list on every target |
| M39 | B5 Ship | Linux kit | `@NODE@ --env-file … dist/server.js` | `ExecStart=@ROOT@/UndauntedMetagame/metagame --env-file=@CONFIG@/metagame.env` | `deploy/linux-server/install.sh`, `systemd/dauntless-metagame.service` | `install.sh` in a CI container |
| M40 | B5 Ship | Windows kit | Node MSI, `npm ci`, `better_sqlite3.node` check; `Stack.ps1` finds `node.exe` + entry script | Download the release binary (pinned + SHA-256); `Stack.ps1` matches `metagame.exe` by full path | **Process lookup must change or stopping the stack misses the metagame** *(from the Bun analysis)* | `tests/Test-Sandbox.ps1` |
| M41 | B5 Ship | OpenBSD port removal (D14) | `deploy/openbsd-server/`, the `server-port-openbsd` CI job, the OpenBSD docs pages (`docs/setup/openbsd-server`, `docs/fi/setup/openbsd-server`), README rows | Delete together with the TS services (M43); until then it stays as it is, unmaintained. README and docs point OpenBSD hosts to the Linux kit | Nothing else in the repo uses `deploy/unix-common/` code that only OpenBSD needed; remove what becomes unused | Docs build has no broken links; CI has no OpenBSD job |
| M42 | B5 Ship | Production cutover | — | Order: the canary, then the first Windows region, then the second (D23). Per server set: stop, back up `undaunted.db`, swap binary, start (the first start adopts the database, D11, and switches it to WAL, D17) | Rollback = `metagame db journal delete` (checkpoint, back to the rollback journal, so the Node-era file-copy backups stay complete), then the previous Node build; possible until the first Diesel migration after the baseline | Smoke list |
| M46 | B5 Ship | Rollback rehearsal | — | On the canary, after its cutover and before the first Windows region: run M42's rollback (`metagame db journal delete`, then the Node build), check the smoke list on Node, then restore a `metagame db backup` and go forward to Rust again | The rollback is otherwise never tried before it is needed | Smoke list passes on Node after the rollback and on Rust after the restore; the restored database passes `metagame db integrity` |
| M43 | B5 Ship | Remove the TS metagame | `legacy/UndauntedMetagame/src`, `package.json`, Drizzle TS schema | Delete once every server runs Rust for one release; delete the traffic corpus (M2) at the same time | The frozen Drizzle SQL moves into `store` for the offline-save replay (D11) | CI no longer installs Node for the metagame |
| M45 | B5 Ship | Kit backups (D17) | `Backup-DauntlessServer.ps1`, the Linux kit's backup in `stack.sh`, `Update-*.ps1` copying `undaunted.db` | Call `metagame db backup <dest>`; stop the service cleanly (checkpoint) before any file-level step; ship in the same release as the cutover | A copy of the `.db` alone misses what is still in the WAL | Restore a backup taken under load on the canary; `metagame db integrity` passes |
| M47 | B5 Ship | Operator docs | `docs/setup/` and `docs/operations/` (and their Finnish copies under `docs/fi/`): Node, `npm ci`, file-copy backups | Rewritten per kit (Windows, Linux): binary install and update, the `metagame db` subcommands, WAL backups through `metagame db backup`, the rollback from M46, and the Rust services from Part C; English and Finnish in the same PR | R9 only fixes paths; this rewrites the content | Docs build; a fresh install on the canary following only the docs |

Smoke list for every deploy step: register, login, character load/save, inventory, store purchase, store image,
matchmaking, party, friends, guild, chat connect + room message, backup endpoint, clean shutdown.

### Metagame phases

| Phase | Content |
| --- | --- |
| B0 | Groundwork (M1–M4) |
| B1 | Foundations: `service-kit`, `store`, auth, logging, config (M5–M28) |
| B2 | Domain areas (M29–M31) |
| B3 | Chat (M32–M34) |
| B4 | Node catch-up, shadow traffic, performance, soak (M44, M35–M37) |
| B5 | Binaries, kits, kit backups, cutover, rollback rehearsal, docs, removal (M38–M43, M45–M47) |

### Metagame dependencies after the migration

| npm package | Rust replacement |
| --- | --- |
| `express`, `express-rate-limit` | `axum`, `tower-http`, `service-kit` limiter |
| `better-sqlite3`, `drizzle-orm`, `drizzle-kit` | Diesel 2.x (`sqlite` with bundled `libsqlite3-sys`, `r2d2`), `diesel_migrations`, `diesel_cli` for `print-schema`; `rusqlite` only for the online backup |
| `ws` | `axum` WebSocket (`tokio-tungstenite`) |
| `ltx` | `quick-xml` |
| `jsonwebtoken` | `jsonwebtoken` crate |
| `pino`, `pino-pretty` | `tracing` + pino-compatible formatter |
| `dotenv` | `dotenvy` |
| `node:crypto` | `sha2`, `aes-gcm`, `rand`, `uuid`, `subtle` |
| `typescript`, `tsx`, `@types/*` | — |

### Metagame risks

| Risk | Rows | Mitigation |
| --- | --- | --- |
| Game client sends paths in another case or with a trailing slash | M6 | Route-table variants test; corpus replay |
| JSON text differs (`1.0` vs `1`, missing vs `null`, key order) | SB1, M35 | Typed integers, `skip_serializing_if`, JS-compatible float output; text-level diff |
| `null` vs `undefined` from `.get()` flips a branch silently | M15 | `Option` everywhere; grep every raw `.get(` in the TS while porting |
| A database isn't at the latest Drizzle migration when adopted | D11, M14 | `store::adopt` refuses it; the operator runs the Node metagame once to catch up, or restores a newer backup |
| Backup stalls saves | M17 | Stepwise online backup; measured on production size |
| A backup copies the database file without its WAL | D17, M45 | Backups only through `metagame db backup`; every kit script changed in the cutover release |
| JWT port accepts something Node rejected | M20 | Negative tests |
| Recovery blobs written by Node don't decrypt | M22 | Fixture from today's code |
| The write pool or the blocking-pool hop becomes a bottleneck | M13, M36 | Measured in M36; one closure for a request's queries; raw SQL on hot paths |
| A cancelled request leaves a transaction open, or an `.await` holds the write lock | M16 | Synchronous `store::write` closure on the blocking pool; test |
| A panic kills a server | Section 2 | `CatchPanicLayer`; `clippy` lints against `unwrap`/`expect` in handlers |
| A Node fix made during the port is missed in Rust | D15, M44 | Catch-up list from `git log rust-port-start..`; final replay runs after the catch-up |
| Stack scripts or the launcher can't stop the new process | M40, X7 | Sandbox and launcher start/stop tests |
| Log format change breaks consumers | M24 | pino field names; sample-line diff |

---

## Part C: Other services → Rust

Scope: the gateway (+ allowlist helper), content server, DeployServer and Discord bot, on the same shared crates.
Each is small enough for one or two PRs; each is cut over together with the metagame on a server (M42) or right after.
Most of Part C runs in parallel with Part B. Before each cutover, that service's catch-up list (X3) is done.

### C1. Migration table

| # | Service | Today | Rust replacement | Notes / risks | Verify |
| --- | --- | --- | --- | --- | --- |
| S1 | Content (`legacy/UndauntedContent`, ~1.9k lines) | Node HTTP, `yauzl`, range handling (`range.ts`), manifest verify, news/branding, account check against the metagame | `axum` + `tower-http` `ServeFile` (ranges), `zip` crate, `sha2` verify, `protocol` types for manifest and branding; account check through the metagame's API as today | Streams 11 GB per player: measure throughput with 4 parallel downloads per client; keep it a separate process so downloads never slow the game backend | Existing content tests ported; launcher downloader tests (L23) pass against it |
| S2 | Gateway (`legacy/UndauntedGateway`, ~2.6k lines) | Node TLS listener with a self-signed certificate (`node-forge`), policy, IP checks, rate limits, redaction, feed, proxy to metagame/content/chat | `rustls` server (`aws-lc-rs`, D10), `hyper` reverse proxy with `crates/proxy` (X5), WebSocket tunnelling, the same policy/limit/redaction rules; traffic capture option (M2) | **Load the existing certificate and key; never generate a new one on upgrade**, or every invite's `fp` breaks. `rcgen` only for new installs | Gateway tests; a 0.1.x and a 0.2.x launcher both connect through it |
| S3 | Allowlist helpers: Windows (elevated) and Linux (`deploy/linux-server/allowlist.mjs`, nftables; shared `deploy/unix-common/allowlist-core.mjs`) | Node processes keeping one Windows Firewall rule / nftables set in sync with logged-in addresses | `gateway` workspace bin `allowlist`: Windows Firewall through the `windows` crate COM API (`INetFwPolicy2`), Linux through `nft` with a fixed argument list (no shell); local IPC from the gateway only | Runs with admin rights: smallest possible surface, no network listener beyond the local IPC | Firewall rule / set contents after login and logout on Windows and Linux |
| S4 | DeployServer (`legacy/UndauntedDeployServer`, ~1.8k lines) | Express, `better-sqlite3`, Drizzle, `jsonwebtoken`, `pino` | Same stack as the metagame on `service-kit` and `store`; its own Diesel migrations, starting from a baseline adopted from its latest Drizzle migration like the metagame's (D11) | Also runs in the launcher's offline mode (D12, X2) | Ported tests; offline stack start in the launcher |
| S5 | Discord bot (`legacy/UndauntedDiscordBot`, `.mjs`) | `discord.js` 14, state file, `/key claim` / `/key status`, DM delivery and retries | Own gateway client and REST calls (serenity's rustls has open advisories); same state file format and the same invite builder from `protocol` | Last of the services (D18). State file must load unchanged: a corrupt state stops issuance (as today) | Ported `*.test.mjs` cases; a real `/key claim` on a test guild |
| S6 | Linux kit helpers (`deploy/linux-server/*.mjs`: `launch-gameserver`, `worker-launch`; `deploy/unix-common/*.mjs`: `game-runner`, `generate-config`, `new-invite`) | Node scripts | Subcommands of the matching Rust bins (`deploy`, `metagame`), or plain `sh` where it's only process control; the SSH worker keeps taking one validated argv payload, never a shell string | — | `install.sh` and a two-machine worker launch in a CI container |

---

## Part D: Repository layout (monorepo)

Scope: turn the repository into the layout of section 2 (D19) without breaking a build, a server kit or a docs link
along the way. Each row is one PR; nothing moves while something still builds from the old path.

### Repository migration table

| # | Phase | Area | Today | Change | Notes / risks | Verify |
| --- | --- | --- | --- | --- | --- | --- |
| R1 | Early | Layout record | Layout implied by folder names | Section 2 as the agreed target; root `README.md` / `README.fi.md` "Repository layout" table rewritten to it, marking folders that are still on their way | One place describes the layout; other docs link to it | README table matches `ls` |
| R2 | Early | C++ client DLL | `UndauntedInternalServer/` at the root | `git mv UndauntedInternalServer native/internal-server`; update the CI job's paths (`Server DLL (Windows x64)`, native regression tests), `.sln`/`.vcxproj` relative paths if any reach outside the folder, and the docs/README links to the source folder | The built file keeps its name `UndauntedInternalServer.dll`, so the launcher's pinned hash, the server kits and the many docs that name the DLL don't change. Only references to the *source folder* move | DLL job builds and its tests pass; `git log --follow` shows history |
| R3 | Early | Client mods | `client-mods/` at the root, read by the launcher, `friend-kit/setup.ps1` and `tools/make-friend-kit.ps1` | Stays where it is as its own family. No Rust or C++ inside; `update-manifest.ps1` keeps `manifest.json` current; each consumer reads it through one path constant | PS5Icons are used by the C++ DLL at runtime only (installed next to the game exe), so there is no build-time link between the families | CI check that `manifest.json` matches the files |
| R4 | Early | Rust workspace | — | `rust/Cargo.toml` (workspace, shared `[profile]` and `[workspace.lints]`), `rust-toolchain.toml`, `deny.toml`, `.cargo/config.toml`, `crates/` and `services/` (with M4) | Before any Rust code lands, so nothing has to move later | `cargo check --workspace` in CI |
| R5 | During | Launcher backend | — | Tauri backend created at `legacy/UndauntedLauncher/src-tauri/` and joined to the workspace with `package.workspace = "../../../rust"` | Cargo allows a member outside the workspace folder only with that key; temporary until R7. The Electron app and the Tauri app share `legacy/UndauntedLauncher/src/` until the cutover | `cargo test --workspace` includes the launcher |
| R6 | During | New services | Node projects at `Undaunted*/` | Each Rust service is created at `rust/services/<name>/` beside its Node project; the Node folder keeps its path until it is deleted (M43, Part C cutovers) | Server kits, docs and scripts that use the old paths keep working through the port | Both builds green while they coexist |
| R7 | After cutover | Launcher move | `legacy/UndauntedLauncher/` (after L51: Tauri only) | `git mv` to `rust/launcher/`: `src-tauri/` as is, the page (`src/`, `index.html`, `vite.config.ts`, `package.json`, `isolation/`, assets) into `ui/`; drop `package.workspace` | Release workflow, `tauri.conf.json` (`frontendDist`, `beforeBuildCommand`), docs and README paths change; update URLs, the bundle id and installer names do not | A release dry run builds identical installers from the new path |
| R8 | Early | CI by family | One `ci.yml` with per-project jobs | Jobs per family (`rust`, `native`, `client-mods`, `tools`, `deploy`, `docs`) behind a pinned `dorny/paths-filter` step, plus one always-running `ci-ok` job that fails if any selected job failed; branch protection requires only `ci-ok`. Release jobs always run the full set | Path-filtered jobs that are skipped would otherwise block merges as missing required checks | A docs-only PR skips the Rust and C++ jobs and still merges |
| R9 | End | Docs and ownership | Paths spread over READMEs, CONTRIBUTING, docs pages | `CONTRIBUTING.md` gets per-family build steps (cargo, MSBuild, PowerShell for mods); `.github/CODEOWNERS` per family; docs pages link to source by the new paths (the operator guides' content is rewritten in M47) | `docs/` stays at the root (GitHub Pages serves `/docs`; `baseurl` unchanged) | Docs build has no broken links |
| R10 | End | Path sweep | — | CI step that fails on references to removed folders (`legacy/UndauntedMetagame/`, `legacy/UndauntedGateway/`, `legacy/UndauntedContent/`, `legacy/UndauntedDeployServer/`, `legacy/UndauntedDiscordBot/`, `legacy/UndauntedLauncher/`, `UndauntedInternalServer/`), each added once its folder is gone; also the old `tools/*.js` / `*.mjs` names once Part E is done (`node tools/`) | Matches folder paths and script names only, not the DLL file name | Step green on the final tree |

### Repository order

R1–R4 and R8 come first, before any Rust code lands. R5–R6 need no separate move: new code goes straight to its final
place. R7 and R9–R10 follow the Tauri cutover and the service deletions.

---

## Part E: Tools → TypeScript on Bun

Scope: port the Node scripts in `tools/` (~3k lines, Node built-ins only, no npm dependencies) to strict TypeScript run
by Bun, keep every output byte-identical where something else reads it, and ship the server-side tools as one
executable. Independent of Parts A–C; it can start at any time. The `tools/` catch-up list (X3) is done before T10.

### Tools migration table

| # | Area | Today | TypeScript on Bun | Notes / risks | Verify |
| --- | --- | --- | --- | --- | --- |
| T1 | Setup | Loose `.js` / `.mjs` files, run with `node` | `tools/package.json` (no runtime deps; `@types/bun` and `typescript` as dev deps), `bun.lock`, `tsconfig.json` (`strict`, `module: "preserve"`, `moduleResolution: "bundler"`, `noEmit`, `types: ["bun"]`), `bunfig.toml` (`[install] exact = true`); Bun version pinned (`packageManager` field) and installed in CI with `oven-sh/setup-bun` at a pinned SHA | **Bun auto-loads `.env` files**: every script runs with `--no-env-file` and, where it needs one, an explicit `--env-file` *(from the Bun analysis)* | `bun install --frozen-lockfile`; `tsc --noEmit` |
| T2 | Repo checks | `tools/ci/check-repo.js` (318 lines: secrets, game files, history scan via `git`) | `check-repo.ts`; `Bun.$` with fixed arguments for `git`, `Bun.CryptoHasher` for SHA-256, `Bun.file` reads | Same rules and exit codes; CI's `--history "$since..HEAD"` call unchanged apart from `bun` | Same findings as the JS version on a branch seeded with a fake secret and a fake game file |
| T3 | Launcher versions | `tools/ci/launcher-version.js` (126 lines; `valid`, `nuget`, `newest`), used by `launcher-release.yml`, including two sparse checkouts | `launcher-version.ts`; release jobs add `setup-bun` and check out the `.ts` path | The `nuget` command exists only for Squirrel; it goes with the Squirrel feed at 0.3.0 (L50a) | Same output for every version string in the release history |
| T4 | Docs generators | `build-llms.js` (310), `sync-roadmap.js` (53) | `build-llms.ts`, `sync-roadmap.ts` on `Bun.file` / `Bun.write` | CI's "generated files are current" step keeps working | `docs/roadmap.md`, `docs/llms.txt`, `docs/llms-full.txt` byte-identical when regenerated |
| T5 | Game manifest | `make-game-manifest.js` (294; SHA-256 and CRC32 over the 410 files) | `make-game-manifest.ts`; streaming `Bun.CryptoHasher("sha256")`, `Bun.hash.crc32` for CRC32 | Output is the pinned `legacy/UndauntedContent/data/dauntless-1.4.4.json`, which the launcher compiles in and the content server checks: it must not change by a byte. `Bun.hash.crc32` and `Bun.CryptoHasher` give Node's values (section 3.4) | Regenerate from a 1.4.4 folder; `git diff` empty; manifest fingerprint unchanged |
| T6 | Owner dashboard | `dashboard.mjs` (253, `node:http`), `dashboard-fleet.mjs`, `dashboard-performance.mjs`, `dashboard-worker.mjs`, `dashboard.html` + browser script `dashboard-client.js` (324) | `Bun.serve` server in TS; the browser script becomes `dashboard-client.ts`, bundled by `Bun.build` (`target: "browser"`) and embedded with the HTML in the compiled executable (T8) | Same bind address, auth (`timingSafeEqual` on the hashed key stays `node:crypto`), headers and API routes; the inline styles stay as they are | Ported dashboard tests; the page loads and lists the fleet against the canary |
| T7 | Worker health, Discord status, summaries | `worker-health.mjs` (68, calls `worker-processes.ps1`), `discord-status.mjs` (207), `summarize-performance.mjs` (90) | `.ts` versions; `Bun.spawn(["powershell.exe", "-NoProfile", "-File", …])` with fixed arguments for the process list | Discord status keeps its message format and posting interval | Ported tests; one real status post to a test channel |
| T8 | Server installs | `Install-OwnerDashboard.ps1`, `Install-DiscordStatus.ps1` schedule `node.exe --env-file=… dashboard.mjs` / `worker.mjs` (`NodePath` setting) | one `bun build --compile --target=bun-windows-x64-baseline --no-compile-autoload-dotenv` executable, `dr-tools.exe`, with `dashboard`, `worker` and `discord-status` subcommands, published with its SHA-256 sum in the release (X6); the installers download and check it, schedule `dr-tools.exe <subcommand> --env-file=…`, and replace existing Node tasks on upgrade | Servers lose their last Node dependency from `tools/`; the `NodePath` setting is retired. One executable instead of three: each compiled exe is about 92 MB and needs its own antivirus submission. Without the flag, a compiled exe loads `.env` from its working directory (section 3.4). Bun-compiled executables are often flagged by antivirus: submitted with every release (D25) | `tests/Test-Sandbox.ps1` installs both and the tasks start; an upgrade from a Node install leaves no Node task behind |
| T9 | Kept as they are | `make-friend-kit.ps1`, `worker-processes.ps1`, `Deploy-NativeLocale.ps1`; `xref.py` | Unchanged | The PowerShell scripts drive Windows itself or build the PowerShell friend kit. `xref.py` needs the `pefile` and `capstone` Python libraries for disassembly; Bun has no equivalent, and it is a research tool, not shipped | — |
| T10 | Tests and docs | 6 `node:test` files, of which CI runs 3 (`summarize-performance`, `dashboard`, `discord-status`) | All 6 on `bun:test` (`dashboard-client`, `dashboard-fleet`, `worker-health` join CI); `bun test tools/` in the `tools` CI job (R8). `tools/DASHBOARD.md`, `tools/DISCORD-STATUS.md`, `docs/reference/scripts.md` and its Finnish copy, `CONTRIBUTING.md` and the PR template switch `node tools/….js` to `bun tools/….ts` | `dashboard-client.test` runs the browser script in `node:vm` today; it tests the TS module directly instead | All tests green; docs build; path sweep (R10) finds no old names |

### Tools risk

T1–T5 and T10 are mechanical; T6–T8 carry the risk (dashboard bundling, compiled executables, installer upgrade path).
