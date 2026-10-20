# Expected-player snapshots

The 1.4.4 native expected-player request sends the stored expected roster while
the game is in the prematch airship. It does not report only missing arrivals or
act as an individual connected-player heartbeat. The response updates
`ExpectedPlayerCount` and the stored roster only when the new count is positive
and differs from the old count; an empty response is ignored.

These behaviors were checked in the installed `1.4.4_239827` executable with
SHA-256 `d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4`:

- Request producer: VA `0x1414ba740`; copies the gamemode's roster at `+0x4e0`
  into request `playerIds` at `0x1414ba8a6`–`0x1414ba8b9`.
- Response callback: VA `0x1414a6d80`; rejects an empty count at `0x1414a6dce`
  and an unchanged count at `0x1414a6dda`. It writes the game state's
  `ExpectedPlayerCount` at `0x1414a6f00` and copies the returned roster at
  `0x1414a6f15`.
- The native `/gamesession/playerjoined` request is an admission operation from
  `RedeemMatchmakerTicket`, called by `PreLogin`. The `bDisableMatchMakerAuth`
  flag bypasses it, so roster expiry does not rely on this event.

## Wire contract

The existing dedicated-server HTTP request hook adds two headers alongside the
existing game-server API key. It creates no additional requests or endpoints.

| Header | Meaning |
| --- | --- |
| `x-dauntless-game-session-id` | Lowercase deployment UUID from the exact `<uuid>.ready` basename in `DR_SERVER_READY_FILE`. Missing or malformed launch identity sends neither new header. |
| `x-dauntless-connected-player-ids` | Complete JSON array of IDs seen on the game thread. `[]` confirms an empty server; absent, empty-string or invalid JSON means unknown and must not authorize expiry. |

Writing an empty string when a snapshot is unavailable clears a previous header
on a reused HTTP request. The metagame accepts these headers only with game-server
authentication and a matching known deployment session. A client-supplied list or
a list from another session is not evidence of connection.

The native producer allows at most 128 connections, 128 distinct IDs, 128 ASCII
characters per ID (range `0x21`–`0x7e`), and 8192 serialized ASCII bytes. Quotes and
backslashes are escaped as JSON. Exceeding any bound invalidates the whole list.

## Native collection and threading

`GameEngineTickHook` reads the current world's server driver. Pending and open
main connections and their child connections must all have a controller, an
`AArchonPlayerState`, a valid `UniqueId`, and a usable ID string. Unknown states,
missing identities, malformed arrays or unexpected child structures invalidate
the whole snapshot. Closed connections require no ID. This protects a client
that still has no player controller while loading.

Identity validation uses existing reflected SDK methods:
`UArchonGameplayStatics::IsValidNetId` and
`AArchonPlayerState::GetUniqueIdAsString`. Both are checked for availability before
their generated wrappers are called, and both are called only on the game
thread. SDK `FString` results use the engine allocator and release their memory
through the SDK's array destructor.

The game thread checks completeness every tick and captures IDs at most one
second apart. A changed connection, controller, player state or connection state
forces an immediate capture. An unidentified pending/open connection immediately
invalidates the cached value. Historical pointer values are only compared; they
are never dereferenced to recover an old connection or object.

The HTTP hook reads only a copied string under a mutex and rejects snapshots
older than two seconds, including when the game thread stalls. It never accesses
UObjects or connection arrays. A snapshot still describes its capture time; the
metagame's loading and recent-arrival grace periods cover short sampling gaps.

## Verification

From the repository root:

```sh
g++ -std=c++20 -Wall -Wextra -Werror -O2 -pthread native/internal-server/tests/connected-player-snapshot.cpp -o /tmp/dr-connected-player-snapshot
/tmp/dr-connected-player-snapshot
g++ -std=c++20 -Wall -Wextra -Werror -g -O1 -pthread -fsanitize=address,undefined -fno-omit-frame-pointer native/internal-server/tests/connected-player-snapshot.cpp -o /tmp/dr-connected-player-snapshot-asan
/tmp/dr-connected-player-snapshot-asan
```

Tests cover both path separators, UUID rejection, empty versus incomplete state,
pending identities, child invalidation, JSON escaping, ID/connection/wire limits,
stale-cache rejection and concurrent publication/readers. These helper tests do
not establish real-client joins or full DLL compatibility. Verify those with the
target executable and inspect the headers received by the metagame. In a runner
where LeakSanitizer cannot inspect `/proc` under ptrace, use
`ASAN_OPTIONS=detect_leaks=0` for the sanitizer command; address and undefined
behavior checks remain active.
