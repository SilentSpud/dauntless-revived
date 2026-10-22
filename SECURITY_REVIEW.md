# Security review checklists (migration)

The dedicated review each trust-boundary component gets before it ships ([MIGRATION_PLAN.md](MIGRATION_PLAN.md)
D26, X9), on top of normal review and CI. One person writes most of the port, so this list stands in for a second
reviewer where a mistake lets someone in, takes a key, or crashes a public listener.

How to use it: the component's cutover PR copies its section, ticks every item after checking it in the code (not
from memory), and links the code or test that shows it. An item that does not hold is a bug to fix before the
cutover, not a box to leave open. The reviewer signs the section with the date and the commit reviewed.

Parsers of untrusted input have `cargo-fuzz` targets in `rust/fuzz` (see its `Cargo.toml`). CI runs every target
for 45 seconds on each push (the `fuzz` job). Before a cutover, run the component's targets for 24 hours with no
crash:

```bash
cargo +nightly fuzz run <target> -- -max_total_time=86400
```

Reporting a vulnerability found here: [SECURITY.md](SECURITY.md).

## Metagame (Part B)

Code: `rust/services/metagame`, `rust/crates/service-kit`, `rust/crates/store`. Fuzz: `http_body`,
`auth_token`, `chat_frame`, `gateway_policy` (client addresses).

- [ ] Every route that acts for a player takes one of the auth extractors (`Player`, `SoftPlayer`, `UserKey`,
      `SoftAccount`, `GameServer`, `AdminKey`); none reads the key or token by hand. The route inventory
      (`routes.json`) and the permissions tests agree.
- [ ] Session tokens: RS256 only, issuer and audience checked, no leeway, `nbf` checked; the token epoch is
      checked against `authepochs`, so a rotated key or a ban ends existing sessions. A token signed with another
      key, `alg: none` or an HMAC algorithm is refused.
- [ ] Account keys and game-server keys are stored only as SHA-256 hashes; lookups compare hashes, and any
      comparison of secrets is constant-time (`crypto::hashes_equal`).
- [ ] Recovery blobs: AES-256-GCM with a fresh random nonce per seal, the user id bound in, and any tampered,
      truncated or other-user blob refused (`open_recovery`).
- [ ] Random values that guard anything (keys, invite codes, recovery tokens) come from the OS generator.
- [ ] Body and query parsing has the size and depth limits Node had; a body over the limit is refused before it is
      parsed in full.
- [ ] Rate limits apply per route class as in Node, keyed on the right client address; the gateway, proxy and
      local-caller rules (`origin.rs`) cannot be spoofed with `X-Forwarded-For` from outside.
- [ ] Admin routes need the admin key; the admin key and `/internal/backup` are reachable from loopback only where
      Node made them so.
- [ ] `LOG_BODIES` and every log line redact account keys, tokens, invite codes and the `x-undaunted-user-api-key`
      header (`redact.rs`, `bodylog.rs`).
- [ ] SQL: every query goes through Diesel's DSL or binds its values; no string-built SQL takes request input.
- [ ] Chat (XMPP): frames are parsed without recursion limits being hit by deep input, entity expansion stays
      bounded, nicknames and JIDs are checked, and a client only reaches the rooms and presence it is entitled to.
- [ ] Offline saves (`offline_save`): an imported save's schema and history are checked before any of its SQL runs
      (`trusted_schema = OFF`), and an export never carries the live key, game-server keys or recovery material.
- [ ] 24-hour fuzz run of `http_body`, `auth_token` and `chat_frame` with no crash.

Signed: ________ (date, commit)

## Gateway (S2)

Code: `rust/services/gateway` (`gateway.rs`, `policy.rs`, `guard.rs`, `feed.rs`, `redact.rs`), `rust/crates/proxy`.
Fuzz: `gateway_policy`, `proxy_headers`.

- [ ] TLS 1.2 and 1.3 only, with the certificate the invites pin (`fp` unchanged from the Node gateway).
- [ ] The policy is an allow-list: only the metagame, content and chat paths it names reach a backend; any other
      path, method or upgrade is refused before a backend sees it. Paths are compared canonically, so `..`,
      encoded slashes and repeated slashes cannot reach a refused route.
- [ ] Forwarded requests carry no hop-by-hop header and nothing the `Connection` header names; `Host` is the
      backend's; client-supplied `X-Forwarded-*` headers are replaced, not passed on.
- [ ] Slow or oversized requests (headers, bodies, idle connections) are cut off with the limits of the Node
      gateway; WebSocket upgrades are bounded the same way.
- [ ] Per-client rate limits hold before the request reaches a backend.
- [ ] Logs redact keys and tokens in paths, queries and headers.
- [ ] The allowlist feed to the helper is authenticated and backs off from a helper that stops answering.
- [ ] 24-hour fuzz run of `gateway_policy` and `proxy_headers` with no crash.

Signed: ________ (date, commit)

## Allowlist helper (S3)

Code: `rust/services/gateway/src/allowlist`. Runs elevated (administrator on Windows, root or `CAP_NET_ADMIN` on
Linux). Fuzz: `gateway_policy` (address checks).

- [ ] It listens on loopback only, and every request must carry the shared secret; the comparison is constant-time
      (`secret_matches` hashes both sides first).
- [ ] Only addresses that pass `check_allowlist_ip` reach the firewall; private and loopback addresses only where the
      configuration allows them. No request text reaches a shell or a firewall command as anything but a checked
      address.
- [ ] It changes only the one rule (Windows) or set (Linux) it owns, and on Windows takes over the Node helper's rule
      instead of adding a second one.
- [ ] The audit log records each change without the secret.
- [ ] A crash or a stop leaves the firewall in a safe state (the last good list, not wide open).

Signed: ________ (date, commit)

## Content server (S1)

Code: `rust/services/content`. Fuzz: `content_range`.

- [ ] Every file request needs a valid account key; keys are cached by their SHA-256 only, and refused keys are
      cached briefly so a bad key cannot hammer the metagame.
- [ ] Only files the manifest lists are served, resolved inside the game folder; no request path is joined onto
      the folder directly. A manifest with an unsafe path (`..`, absolute, drive letters, device names) is refused at
      start.
- [ ] `Range` handling gives a range inside the file or a 416; `If-Range` and `If-None-Match` use the file's ETag.
- [ ] Branding images and news are served from the configured folder only, with their type checked.
- [ ] 24-hour fuzz run of `content_range` with no crash.

Signed: ________ (date, commit)

## DeployServer (S4)

Code: `rust/services/deploy`.

- [ ] Only the metagame on the same machine may call it: direct loopback connections only, and a request with
      proxy headers is refused (`is_direct_loopback`).
- [ ] Game-server processes start with arguments built from checked values; nothing from a request reaches a shell.
- [ ] The game-server key it hands to servers is the one registered in the metagame, and it is never logged.
- [ ] On stop it ends only the processes it started, found by its own handles, never by name.
- [ ] Remote workers are reached only through the configured SSH forced command, with checked arguments.

Signed: ________ (date, commit)

## Discord key bot (S5)

Code: `rust/services/discord-bot`. Fuzz: `bot_input`.

- [ ] Commands check the member's permissions from Discord's own data; `/key` answers are ephemeral or by DM, never
      in a channel.
- [ ] Account keys are sent only by DM to the member they belong to, and never logged.
- [ ] `/key link` accepts only the key alphabet and refuses an invite link; the metagame decides whose key it is.
- [ ] The state file is written privately and atomically, and a damaged one stops the bot instead of being
      replaced.
- [ ] Rate limits from Discord are respected (retry after 429); a flood of commands cannot make the bot spam
      the metagame.
- [ ] 24-hour fuzz run of `bot_input` with no crash.

Signed: ________ (date, commit)

## Launcher (Part A)

Code: `legacy/UndauntedLauncher/src-tauri`, `isolation/`, `src/renderer/backend.ts`. Fuzz: `launcher_input`
(invites, server status, usernames and keys, manifests).

- [ ] Invites (L11): only the two invite shapes are accepted; a public invite always carries a fingerprint, and a
      private one only a tailnet or loopback host. The account key goes only to the invite's host.
- [ ] Pinned TLS (L19): a public server is accepted only with the invite's certificate; the key is never sent
      before the pin is checked, and a changed certificate needs the player's confirmation before the key moves
      (`cert_changed`).
- [ ] The relay (L22) listens on 127.0.0.1 only, refuses requests from web pages (Origin and Host checks), forwards
      only end-to-end headers, and on Windows only to the game process it started (the owner check).
- [ ] The key store (L27): keys are encrypted with DPAPI (Windows) or a Secret Service key (Linux); the legacy reader
      only reads the Electron launcher's keys and never writes them back unencrypted; keys are added to the log
      redaction as soon as they are loaded.
- [ ] Isolation and capabilities (L8, L9): the page can call only the listed commands with checked arguments; it has
      no file system, shell, dialog or updater permission; the CSP has no `unsafe-*` source; navigation never leaves
      the app's own origin.
- [ ] Downloads (L23, L24): every file is checked against the compiled-in manifest's hash before it is used; a
      server's manifest that differs is refused; a CDN download URL must be https without credentials.
- [ ] Self-update (L46): only bundles signed with the launcher's update key install; the private key is only in the
      release job's secrets.
- [ ] Offline play (L43): the services listen on loopback only, start with only the operating system's environment,
      and their secrets and settings files are private.
- [ ] The bridge release (L50): the Tauri installer is installed only when its SHA-256 matches the value compiled
      in; the handover script takes no input from outside.
- [ ] 24-hour fuzz run of `launcher_input` with no crash.

Signed: ________ (date, commit)
