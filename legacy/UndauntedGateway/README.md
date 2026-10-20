# Dauntless Revived gateway

The public front door of a Dauntless Revived server in public mode (roadmap 1.17), in the
`legacy/UndauntedGateway/` folder. Two small processes:

- **The gateway** (`dist/server.js`) is the server's only public TCP listener. It speaks TLS with a
  self-signed certificate, and the friend launcher pins that certificate's fingerprint (the `fp` of a
  v2 invite). Behind it, on 127.0.0.1, are the metagame, the content server and, later, the chat
  service.
- **The allowlist helper** (`dist/allowlist/server.js`) runs with administrator rights. It keeps one
  Windows Firewall rule that opens the game servers' UDP ports only to addresses whose owners
  logged in through the gateway.

```
 friend's PC                                   the server (public IP)
 game ── http ──► launcher relay ══ TLS 443 ══► gateway ──► 127.0.0.1:61000  metagame
      127.0.0.1:61000   (pins fp)                 │    ├──► 127.0.0.1:61002  content server   (/content/*)
                                                  │    └──► 127.0.0.1:61099  chat (WebSocket upgrades)
                                                  └─ "this address logged in" ──► 127.0.0.1:61005 allowlist helper
 game ═══════════════ UDP 8770-8777 ══════════════════════════► game servers  (firewall: allowlisted addresses only)
```

The deploy server (61001) is never reachable through the gateway: no route leads to it.

## The gateway

### Routing

| Request | Goes to |
|---|---|
| `/content` and `/content/*` | the content server (`GATEWAY_CONTENT_URL`) |
| any WebSocket upgrade (`GET` with `Upgrade: websocket`) | the metagame's chat listener (`GATEWAY_WS_URL`); `502 {"error":"bad_gateway"}` while chat is off. The game asks for `//` with the protocol `xmpp`; both pass through unchanged |
| everything else | the metagame (`GATEWAY_METAGAME_URL`) |

The request target goes upstream unchanged: method, path, query and headers (minus the ones below).
Request and response bodies are streamed. That includes the account key header
`x-undaunted-user-api-key`: the launcher sends it with `GET /undaunted/api/ServerStatus`, and the
metagame lists who is online only for a valid key (anyone else gets the same answer with no players
and no servers, `"limited": true`). Like every header, the key is never logged.

### What it refuses

| Answer | When |
|---|---|
| `403 {"error":"forbidden"}` | the request carries an `x-undaunted-gameserver-apikey` header, whatever its value, on any route (game servers run on the server itself and talk to the metagame directly) |
| `403 {"error":"forbidden"}` | any `/undaunted/api/*` route except `POST Register`, `GET GetUserInfo`, `GET ServerStatus` and `GET RegistrationStatus` (and `HEAD` of those three). Most of the rest are admin routes: invites, renames, rollbacks, entitlements, JWTs for any account, `POST RegistrationStatus`. The account-key routes `PublicOnlineStats`, `PartyInvite` and `Friends` are blocked as well (they are used on the server itself), and so is `UsernameAvailable`; `Register` answers `409 username_taken` instead |
| `400 {"error":"bad_path"}` | `.` or `..` segments, backslashes, control characters or whitespace, percent-encoded `/` `\` `.` or NUL, or broken percent-encoding. The 1.4.4 client never sends any of these (checked against every path in the metagame logs) |
| `400 {"error":"bad_request"}` | a request target that is not a plain path (`http://host/...`, `*`), or an upgrade that is not a WebSocket `GET` |
| `405 {"error":"method_not_allowed"}` | methods other than GET, HEAD, POST, PUT, DELETE, PATCH, OPTIONS |
| `413 {"error":"body_too_large"}` | a body over `GATEWAY_MAX_BODY_BYTES`. A `Content-Length` over the cap is refused before anything is read; a chunked body is cut when it crosses the cap, and the upstream request is aborted |
| `429 {"error":"rate_limited"}` + `Retry-After` | the address's bucket for this kind of request is empty (see Limits) |
| `408`, `431`, `400` | the request headers or body arrived too slowly, the headers are too large (16 KB), or the request is malformed |
| `502 {"error":"bad_gateway"}`, `504 {"error":"upstream_timeout"}` | the upstream is down, or went quiet for 120 s |

The admin-route check is at least as lenient as Express's own routing: it matches regardless of
letter case, trailing or repeated slashes and percent-encoding. The tests read every route from
`legacy/UndauntedMetagame/src/routes/undauntedapi.ts` and try each one through the gateway, so a route
added there later is covered automatically: it is blocked unless it is added to
`PUBLIC_UNDAUNTED_API` in `src/policy.ts`.

### Headers it sets

- `X-Forwarded-For: <peer address>`: **overwritten**, never appended to. The client's own
  `X-Forwarded-For`, `Forwarded`, `X-Real-IP`, `X-Forwarded-Host/Proto/Port` and similar are dropped.
- `X-Forwarded-Proto: https`.
- `X-Dauntless-Gateway: <GATEWAY_SECRET>`. A client-supplied one is dropped. The metagame trusts
  `X-Forwarded-For` only on a request from 127.0.0.1 that carries this secret.
- Hop-by-hop headers (`Connection` and the headers it names, `Keep-Alive`, `TE`, `Upgrade`,
  `Proxy-*`, `Transfer-Encoding`) and `Expect` are not forwarded. On the way back `X-Powered-By` is
  dropped too.

IPv4-mapped peer addresses (`::ffff:1.2.3.4`) are reported as plain IPv4.

### The allowlist feed

When the metagame answers **2xx** to `POST /heartbeat` that carried a bearer token, or to
`POST /account/api/oauth/token` (which only succeeds with a valid account key), the gateway sends
the peer address to the helper (`POST /allow`). It sends one address at most once per
`GATEWAY_ALLOWLIST_REFRESH_SECONDS` (60 s). A playing client sends a heartbeat every 20 s (the
metagame answers `20000`), and the helper keeps an address for 10 minutes. The player's request
never waits for the helper. If the helper is down, the gateway logs one warning a minute and that
player gets no game ports until it answers again.

The gateway listens on IPv4 (`0.0.0.0`) so that the address it reports is the one the game's UDP
traffic comes from: game servers are announced on the server's public IPv4 (`MY_IP`).

### Limits

Sized from real 1.4.4 sessions on the development server. The data: `bodies.log` (718 captured
request bodies of the save routes), five metagame logs (3,395 client requests over 3.5 hours of play,
two accounts) and a fresh backup copy of the save database (for character data, which the client
uploads but `bodies.log` does not capture).

**Request bodies: `GATEWAY_MAX_BODY_BYTES` = 131,072 (128 KiB).**

| Largest measured body | Bytes | Through the gateway? |
|---|---|---|
| `POST /character` (client character save): largest stored character data | 18,782 (about 22,000 when sent as an escaped JSON string) | yes, the largest |
| `POST /candidate/join` | 536 | yes |
| `POST /party` (215 samples) | 61 | yes |
| `POST /progression/<id>` | 3,978 | no (game server, loopback) |
| `POST /inventory` | 2,959 | no (game server) |
| loadouts (saved by the game server) | 5,252 | no |

The largest client body is about 22 KB; 4 times that is 88 KB, rounded up to 128 KiB. `POST /event`
and `POST /encountered-content` bodies were not captured. Each access-log line has `bytesIn`, so
the first public session shows whether anything comes close (and any `413` would show up there).

**Requests per address.** Token buckets per IPv4 address (IPv6: per /64). Each request counts
against one bucket. `burst` is the bucket size and `per minute` is the refill rate.

| Measured client traffic | Peak |
|---|---|
| any request, 1 s window (login burst) | 30 |
| any request, 10 s / 60 s / 10 min windows | 53 / 98 / 356 |
| average | 0.23-0.48 per second |
| steady state | `POST /party` and `GET /party/invites` every 10 s, `POST /heartbeat` every 20 s |
| `POST /account/api/oauth/token` | 2 per 60 s, 4 per 10 min |
| `POST /undaunted/api/Register` | 1 per 10 min |
| `GET /undaunted/api/GetUserInfo` | 3 per 60 s |
| launcher downloads | 410 files, 4 at a time, one request per file (plus Range resumes) |

| Bucket | Requests | burst, per minute | Why |
|---|---|---|---|
| general | everything not below | 300, 180 | at least 4 times every measured peak: 300 in 1 s (vs 120), 330 in 10 s (vs 212), 480 in 60 s (vs 392), 2,100 in 10 min (vs 1,424). That leaves room for a few players behind one home connection |
| content | `/content/*` | 600, 600 | a whole 410-file download without waiting |
| register | `POST /undaunted/api/Register` | 5, 0.2 (one per 5 min) | 4 times the observed rate; typos and taken names included |
| token | `POST /account/api/oauth/token` | 10, 1 | 8 in 60 s and 16 in 10 min are 4 times the observed peaks |
| ws | WebSocket upgrades (the game's chat connection) | 20, 12 | the game reconnects its chat at most every 15-45 s after a failure; its own bucket, so chat reconnects can never use up the general one the game's HTTP traffic needs |
| connect | new TCP connections (each costs a TLS handshake) | 200, 300 | the relay keeps connections alive; this only stops floods |

A refused request gets `429` with `Retry-After` in seconds and never reaches the upstream.

**Connections.** At most `GATEWAY_MAX_CONNECTIONS_PER_IP` = 128 open connections per address (the
login burst opens up to about 30 at once; four players behind one address fit), and
`GATEWAY_MAX_CONNECTIONS` = 2,048 in total. Extra connections are closed before the TLS handshake.

**Timeouts.**

| Setting | Default | Against |
|---|---|---|
| TLS handshake | 10 s | connections that never finish the handshake |
| request headers | 10 s | slowloris (headers dripped in one line at a time) |
| whole request (headers and body) | 30 s | slow bodies; the upstream request is aborted too |
| idle keep-alive connection | 65 s | long enough that a client polling every 10-20 s keeps one connection. A relay should drop its own idle connections sooner (for example with the agent's `timeout: 30000`), so it never reuses a connection just as the gateway closes it |
| no traffic on a connection | 120 s | stalled downloads (the content server uses the same value) |
| upstream silent | 120 s | a hung metagame (`504`) |
| WebSocket idle | 5 min | dead chat connections |

The header and request timeouts are checked every second, so they can fire up to a second late.

### Access log

One JSON line per request on stdout (warnings and errors on stderr):

```json
{"t":"2026-09-21T18:30:00.000Z","level":"info","msg":"request","ip":"203.0.113.7","method":"DELETE",
 "target":"/account/api/oauth/sessions/kill/<token>","route":"metagame","status":200,"bytesIn":0,
 "bytesOut":0,"ms":4,"ua":"..."}
```

`route` is `metagame`, `content`, `ws` or `gateway` (answered by the gateway itself, with `reason`:
`admin_route`, `gameserver_key_header`, `rate_limited:<bucket>`, `body_too_large`, `bad_path`,
`upstream_ECONNREFUSED`, ...). Requests the client gave up on have `"aborted": true`. WebSockets get a
`websocket closed` line with their byte counts.

**Never logged:** request or response headers (the user agent is the only exception), bodies, and the
secrets. In the target, JWTs become `<token>`, account keys `<key>`, and long hex or token-like
strings `<redacted>`. That covers percent-encoded ones and the client's own
`DELETE /account/api/oauth/sessions/kill/<jwt>`. So do query parameters whose name contains key,
token, secret, pass, auth, code, session, jwt, sig or cred. Control characters become `?`. Repeated
connection-level events (TLS failures, limits) are logged at most once a minute per address.

### Shutdown

On `SIGINT`/`SIGTERM`/`SIGBREAK` the gateway stops accepting connections. Requests in flight
finish, and their answers carry `Connection: close`. WebSockets are closed at once (the chat client
reconnects). Whatever still runs after `GATEWAY_SHUTDOWN_GRACE_MS` (10 s) is cut.

## The allowlist helper

HTTP on `127.0.0.1:61005` only. Every request needs the header `x-allowlist-secret`; the comparison
takes constant time. It also refuses any peer that is not loopback.

| Request | Answer |
|---|---|
| `POST /allow` `{"ip": "<address>"}` | `200 {"ip": <canonical>, "added": true/false, "ttlSeconds": 600}`: added, or its 10 minutes restarted |
| | `400 {"error":"invalid_ip","reason": ...}`; reasons: `not_a_string`, `range_not_allowed` (CIDR, `a-b`, lists, `*`, `Any`, `LocalSubnet`, ...), `not_an_ip` (garbage, leading zeros, spaces, zone ids), `unspecified` (`0.0.0.0/8`, `::`), `multicast`, `reserved` (`255.255.255.255`, `240.0.0.0/4`, IPv6 outside `2000::/3`, NAT64), `private` (RFC 1918, `100.64.0.0/10`, loopback, link-local, IPv6 ULA; accepted only with `ALLOWLIST_ALLOW_PRIVATE=1`) |
| | `503 {"error":"allowlist_full"}` past `ALLOWLIST_MAX_ENTRIES` (256) |
| `GET /status` | `{dryRun, allowPrivate, ttlSeconds, ports, entries: [{ip, expiresAt}], pending, lastApply: {ok, enabled, ips, at, error?}}` |
| anything else | `401` (secret), `404`, `405`, `413` (body over 1 KB), `400 bad_request` (not JSON) |

### The firewall rule

One rule: Name `DauntlessRevived-GamePorts-Allowlist`, display name
`Dauntless Revived game ports (allowlist)`. It allows inbound UDP on `ALLOWLIST_PORTS`
(8770-8777) with `RemoteAddress` set to the live set, and it is disabled when the set is empty.
The server installer creates it first, disabled and limited to the game exe with `-Program`. The
helper then only changes its addresses and whether it is enabled (`Set-NetFirewallRule` leaves the
program alone), and creates it only if it is missing. Any other rule with the same display name is
disabled. For two addresses the helper runs exactly this, in Windows PowerShell 5.1, fed through
stdin (no temp file, no command-line limit):

```powershell
$ErrorActionPreference = 'Stop'
$Name = 'DauntlessRevived-GamePorts-Allowlist'
$Display = 'Dauntless Revived game ports (allowlist)'
Get-NetFirewallRule -DisplayName $Display -ErrorAction SilentlyContinue | Where-Object { $_.Name -ne $Name } | Disable-NetFirewallRule
$Rule = Get-NetFirewallRule -Name $Name -ErrorAction SilentlyContinue
$Addresses = @('203.0.113.7', '2001:db8::1')
if ($null -eq $Rule) {
    New-NetFirewallRule -Name $Name -DisplayName $Display -Direction Inbound -Action Allow -Protocol UDP -LocalPort '8770-8777' -RemoteAddress $Addresses -Profile Any -Enabled True | Out-Null
} else {
    Set-NetFirewallRule -Name $Name -NewDisplayName $Display -Direction Inbound -Action Allow -Protocol UDP -LocalPort '8770-8777' -RemoteAddress $Addresses -Profile Any -Enabled True
}
```

and for an empty set, after the same first five lines:

```powershell
if ($null -ne $Rule) {
    Set-NetFirewallRule -Name $Name -Enabled False
}
```

Addresses are validated twice, when they arrive and again when the script is built. A script can
only ever contain canonical single addresses in single quotes. PowerShell is started by its full
path under `%SystemRoot%`, never from `PATH`.

- **Debounced:** the first change after a quiet period applies at once. Changes that arrive during
  a run, or less than `ALLOWLIST_MIN_INTERVAL_MS` (3 s) after the last one, are folded into one
  later run. Refreshing an address that is already open changes nothing. A failed run is retried
  with backoff (3 s, doubling, up to 1 minute).
- **Expiry:** an address is dropped 10 minutes after its last report, and the rule is updated.
- **Restarts:** the live set is saved to `ALLOWLIST_STATE_FILE` when it changes and every 30 s.
  When the helper stops it disables the rule (fail closed) and saves the set. When it starts it
  restores the addresses that have not expired and applies them at once, so players in a match
  lose their UDP only for the restart. Without a state file it starts by disabling the rule.
- **Hard stops:** a supervisor on Windows usually ends a process with TerminateProcess, and then the
  helper's own fail-closed step never runs. A stop script should run
  `node dist\allowlist\server.js --close-ports` (same env file) after stopping the helper. It
  disables the rule once and exits (`closed_ports` in the audit log).
- **Audit log** (`ALLOWLIST_AUDIT_LOG`, JSON lines): `start`, `added`, `expired`, `refused`, `full`,
  `unauthorized` (at most once a minute), `rule_applied` / `rule_failed` (with the address list),
  `dry_run` (with the exact script) and `stop`. The secret is never written.

### DRY-RUN

`ALLOWLIST_DRY_RUN` has no default. `1` never touches the firewall: every change is written to the
audit log as `{"event":"dry_run","script":"..."}` with the exact script above. `0` runs the scripts
for real; that needs Windows and administrator rights, and the helper refuses to start without
them. Every test on a development machine uses `1`.

### Running it on the server

The installer (`deploy/windows-server`, public mode) registers the helper as the scheduled task
`Dauntless Revived allowlist`: at startup, as SYSTEM, with highest privileges, through
`Stack.ps1 supervise -Only allowlist`. It also writes `config\allowlist.env` and `config\gateway.env`
with both secrets. What else must be true on the server:

- Windows Firewall on for every profile, with the default inbound action **Block**
  (`Get-NetFirewallProfile | Select Name, Enabled, DefaultInboundAction`).
- No other rule allows UDP 8770-8777 or the game exe: no program rule left behind by an "allow
  access" prompt, and no leftover Tailscale-only rules if the machine was a private-mode host.
  Check with `Get-NetFirewallPortFilter -Protocol UDP | Where-Object LocalPort -match '877'`.
- Inbound TCP 443 open for the gateway. No rule for 61000-61005 or 61099: those listen on loopback.
- The metagame runs with `AUTH_MODE=APIKEY` (with `NONE`, `/account/api/oauth/token` logs anyone in
  as anyone). It needs `GATEWAY_SECRET` set to the gateway's value, `MY_IP` = the public IPv4, and
  `QOS_TARGET_URL=http://127.0.0.1:61000/QoS`.

## The certificate

```
node tools/make-cert.js --host 203.0.113.7 [--host play.example.org] --out C:\DauntlessServer\tls
node tools/make-cert.js --fingerprint C:\DauntlessServer\tls\gateway-cert.pem [--json]
```

This makes an RSA-2048, SHA-256, self-signed certificate. It is valid for 10 years (from one day
back, for slow clocks), with each `--host` as a subject alternative name (IP or DNS), and is marked
for server authentication only. It writes `gateway-cert.pem` and `gateway-key.pem` (PKCS#8) and
prints the SHA-256 fingerprint of the certificate's DER encoding: 64 lowercase hex characters, the
invite's `fp=`. It only writes those two files, never into any certificate store. It refuses to
overwrite them without `--force`, because a new certificate changes the fingerprint and every
invite already sent stops working. Keep the key in a folder that only Administrators and the
gateway's account can read.

The gateway logs the fingerprint at startup (`gateway ready`, field `fingerprint`), refuses a key
that does not belong to the certificate or a certificate that is not valid now, and warns 30 days
before it expires.

## Configuration

All settings, with their defaults, are in [`.env.example`](.env.example). The ones that must be set:

| Variable | Process | Meaning |
|---|---|---|
| `GATEWAY_CERT`, `GATEWAY_KEY` | gateway | the PEM files from `make-cert.js` |
| `GATEWAY_SECRET` | gateway | 32-256 printable characters; the metagame's `GATEWAY_SECRET` must match |
| `ALLOWLIST_SECRET` | both | shared by the gateway and the helper (`GATEWAY_ALLOWLIST=0` runs the gateway without the feed) |
| `ALLOWLIST_DRY_RUN` | helper | `1` or `0`, no default |

The gateway refuses upstream and helper URLs that are not `http://` on 127.0.0.0/8 or `::1`. The
helper refuses any bind address but `127.0.0.1` and `::1`.

## Development

```
npm install
npm run build          # dist/
npm test               # build/ (git-ignored), 89 tests, about 25 s
```

The tests listen on 127.0.0.1 ports 62400-62436 only, and some use other loopback source addresses
(127.0.0.2-127.0.0.42) to act as different players. What they cover:

- TLS with a certificate made by `make-cert.js` in a temp folder: the pinned fingerprint, the IP SAN,
  and no TLS below 1.2.
- Routing to fake upstreams, and the 403s: the game-server key header, every metagame admin route
  and the spelling tricks.
- `X-Forwarded-For` and the secret header; the 413, 429, slowloris, slow-body, handshake and
  keep-alive limits.
- WebSocket proxying, 502s, graceful shutdown, the allowlist feed, and the access-log redaction.
- The helper's validation, TTL, debounce, restart and exact dry-run scripts.
- A check with PowerShell's own parser that those scripts parse and use only cmdlets and parameters
  that exist. Nothing is executed: `Get-Command` only reads metadata.
- Both entry points as real processes.

The helper always runs in DRY-RUN in the tests: **they never change the firewall of the machine
they run on.**

When the relay pins a certificate, it must check the fingerprint on `secureConnect`, before any
request byte is sent. With `rejectUnauthorized: false`, Node never calls `checkServerIdentity` for a
self-signed certificate, so a check placed there would accept any certificate. The launcher's
`src/main/pinned.ts` and this package's test client (`test/helpers.ts`) both check on
`secureConnect`.
