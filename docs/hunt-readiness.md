# Hunt startup and worker operation

Deploy the matching `UndauntedInternalServer.dll` and deploy-server build together.
The deploy server creates a private per-launch readiness marker automatically and waits
up to 90 seconds before returning a travel address. Newer native servers write the marker
only after `InitListen` succeeds. Existing installs with an older DLL are also supported:
the deploy server verifies that the assigned UDP port is actually bound before matchmaking
can return `IN_PROGRESS`. `GAMESERVER_READY_DIR` may override the marker directory.

Launches retain their configured spacing and memory reservations, but one cold
world no longer blocks every following launch. Client queue/status requests wait
at most 1.5 seconds for an allocation (the party leader's initial join allows 2.5
seconds), then keep reporting matching until it ends.
Temporary worlds allow 180 seconds for the first player to finish connecting.
After a player connects, the native empty-world timer resets and allows 30 seconds after
the last player leaves. Unavailable host capacity still has a bounded one-minute
queue; more time cannot create CPU, RAM or free ports.

## Recovering from failed matchmaking

The metagame bounds an allocation request, including its JSON response body, with
`MATCHMAKING_DEPLOY_TIMEOUT_MS` (240000 milliseconds by default). A stalled request
ends that candidate with `FAILED`. It does not automatically allocate another
server: a request that timed out may already have started a remote process. A new
explicit join can retry. Configure the deadline above the worker startup grace
and launch spacing; it is an upper bound, not an extra delay on successful hunts.

Ready candidates carry the deployment's immutable session ID. A cached, shared
`/gameservers` observation invalidates a dead session even if another process has
reused its port. The deploy server's `complete` flag distinguishes an empty fleet
from an unavailable worker. Missing sessions are treated as dead only in a complete
snapshot; positive observations also work when another worker is unavailable.
Older or unreachable deployments fall back to a fixed reuse lease anchored to the
first travel response, rather than extending it on every retry. Party rejoin
retention remains bounded separately. Status polling never starts a replacement
for a failed allocation by itself.

Ramsgate, Training and tutorial joins install their pending candidate before
waiting on deployment. Completion updates that exact candidate only. A newer hunt
or an explicit cancellation cannot be overwritten by the older response.
Repeated joins also retain the original 20-second park deadline; they cannot keep
restarting that pause.

## Expected players and native compatibility

A queued player's place requires a join or status poll within 45 seconds. Expired
players are removed immediately before a public queue is sent to deployment,
including retries after a capacity refusal. The remaining active players retain
their candidates and region. Allocation and loading use separate grace periods;
stopping matchmaking polls during a normal client load does not immediately remove
the player.

The inspected 1.4.4 executable sends the complete expected-player roster to
`/candidate/player/alive`. Its response handler changes the roster and expected
count only for a different, positive count; it ignores an empty array. Its separate
`KeepAlivePlayerStatusEndpoint` key is absent. Therefore the expected list itself
cannot be used as evidence that those players connected.

The matched native DLL supplies its deployment session and a bounded snapshot of
connected player IDs through the existing authenticated HTTP request hook. Engine
objects are read on the game thread, and the HTTP hook reads only copied strings.
Pending or unidentified connections, a stale snapshot, or missing native support
leave the observation incomplete. The metagame preserves unresolved players in
that case. Complete observations retain connected players, allow an initial
180-second loading grace, and allow a 45-second grace for a previously connected
player's disappearance. This permits a nonempty roster of actual arrivals to
replace one that still contains abandoned players. Unknown sessions after a
metagame restart retain their roster conservatively.

The native channel index also observes the existing actor/channel setter so a
new actor's first lookup does not scan every channel opened before it. Positive
lookups still validate current array slots; removal, replacement and reentrant
assignment are covered by the regression suite. If the setter hook cannot be
installed, lookup keeps its conservative fallback. See
[`CHANNEL_LOOKUP_TESTING.md`](../native/internal-server/tests/CHANNEL_LOOKUP_TESTING.md)
for the exact binary checks and performance regression.

Update the central metagame, the deploy servers on each worker, and the server
DLL together. A client-side DLL replacement alone does not update a remote hunt
host. Test private and public hunts, a party retry, a player disconnect during
loading, and a worker exit before promoting the build. Source tests and a native
startup smoke test do not establish playable end-to-end client loading times.

The installation has two servers: the main host and one overflow worker connected
by an `OVERFLOW_DEPLOYSERVER_URL` loopback SSH tunnel. Existing hunts are not
live-migrated between hosts. Both use the main metagame's account and save database.

For native builds use MSVC 14.44 or newer. On machines with multiple v143 versions,
pass `/p:VCToolsVersion=14.44.35207` to MSBuild. The older 14.36 compiler rejects
the generated SDK's uninstantiated `static_assert(false)` templates.

## Store

The central metagame owns the store and inventory; workers use that same backend.
Set `STORE=free` to enable the shipped catalog. Offers currently cost zero. See
[`UndauntedMetagame/STORE_PRICING.md`](../UndauntedMetagame/STORE_PRICING.md) before
changing earned-currency prices. No real-money payment processing is provided.

Set `STORE_CATALOG_PROFILE=curated30` for the 30-cosmetic storefront listed in
`UndauntedMetagame/src/vendor/store_curated_30.json`. Set `STORE_REPEATABLE_TOKENS=0`;
the curated profile excludes bounty-token bundles even if that flag is enabled.
Other offers cannot be purchased by bypassing the listing and requesting their SKU.
The separate Hunt Pass endpoint remains available.

To revoke the earlier free-store purchases, stop both hosts' game processes and
the metagame, back up the database, then run `scripts/revoke-free-store.cjs <UTC-cutoff>`
from the metagame directory with its protected environment loaded. This previews
the recorded grants. Add `--apply` to revoke them. It preserves pre-owned and
separately earned items, removes only recorded store instance IDs/stack quantities,
and revokes entitlements only where their source is the matching store SKU.
Append-only audit markers prevent a second removal. Unredeemed pre-cutoff tokens
expire. Paid purchases abort the operation. The command checks that character
and escalation progress is unchanged; it is not an account rollback.
