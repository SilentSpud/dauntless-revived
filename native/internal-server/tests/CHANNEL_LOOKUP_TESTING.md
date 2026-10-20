# Actor-channel lookup and cold joins

`Networking.cpp` indexes each connection immediately before its custom replication
pass. The old helper scanned the growing `OpenChannels` array for every missing
actor. Opening N actor channels on a fresh connection therefore required
`N * (N - 1) / 2` channel inspections before replication itself.

The index now distinguishes an absent actor from an association whose array slot
may have changed. Missing actors take constant expected time when assignment
observation is active. A setter notification records every new actor association;
the normal appended-channel case records its live array slot in constant time.
Every positive lookup still validates that slot against the current array. A
stale slot triggers one complete rebuild, refreshing the other shifted slots too.

## Native invariants

- `OpenChannels` is the live `TArray<UChannel*>` at `UNetConnection + 0x70` in
  the checked-in 1.4.4 SDK. It provides no mutation generation counter. Array
  length alone cannot establish that a negative lookup is still valid: replacing
  a channel can introduce a new actor without changing that length.
- The server replication pass and actor/channel association changes run on the
  game thread. Array reads in the helper do not invoke engine callbacks. This is
  the same threading requirement as the existing `OpenChannels` iteration.
- Non-null actor assignments must pass through `UActorChannel::SetChannelActor`.
  Its existing 1.4.4 entry point, RVA `0x3283450`, is observed with MinHook. The
  hook forwards all three original arguments unchanged and calls the original
  setter exactly once. It does not replace engine channel creation or cleanup.
- The exact 1.4.4_239827 executable with SHA-256
  `d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4`
  was inspected for this entry point. Its arguments use RCX for the channel, RDX
  for the actor, and R8D for flags. It reads the channel's `Connection` at `0x28`
  and writes the actor at `0x70` (the write is at RVA `0x32836fb`). These match
  the SDK and the signature already used by `Networking.cpp`.
- Assignment observation applies to every engine call to that setter, including
  an indirect channel creation during replication. It is not limited to the
  explicit create-channel call in our actor loop. Per-thread scopes notify the
  matching connection, including outer scopes during reentrant calls.
- The hook notifies before and after the original setter. A reentrant lookup may
  rebuild both before and after the setter writes its actor. Rebuilds retain
  in-progress actor keys until the matching completion notification, including
  nested assignments of the same actor; the second notification records the
  finished assignment's live slot when possible.
- Removals, reordering, null slots, and clearing an actor association cannot
  introduce a new actor. They require no notification. Stale positive slots are
  checked using the current array and refreshed. Replacing a channel for the
  same actor is also safe because the lookup reads the replacement from the live
  slot instead of retaining the former channel pointer.
- No channel pointer survives in the index. Actor addresses are keys only and
  are not dereferenced by the helper. The setter captures its connection before
  calling the engine and never dereferences its channel argument afterward.

If the observer cannot be installed, the server logs that fact and keeps the
conservative scan-on-miss behavior. `DR_ChannelAssignmentTrackingEnabled` is an
exported diagnostic value: 1 means the observer was installed; 0 means the
fallback remains active. A changed game build or another mod that assigns
`UActorChannel::Actor` directly requires reevaluating these invariants.

## Regression verification

Run from the repository root:

```sh
g++ -std=c++20 -Wall -Wextra -Werror -O2 native/internal-server/tests/channel-lookup.cpp -o /tmp/dr-channel-lookup
/tmp/dr-channel-lookup
g++ -std=c++20 -Wall -Wextra -Werror -g -O1 -fsanitize=address,undefined -fno-omit-frame-pointer native/internal-server/tests/channel-lookup.cpp -o /tmp/dr-channel-lookup-asan
/tmp/dr-channel-lookup-asan
```

The cold-join cases cover 1,000, 2,000, 4,000, and 8,000 actors. Counting setter
observation, the initial lookup, and another steady-state lookup, they inspect
exactly 3N channels (24,000 at 8,000 actors). Separate interleaved assignments
verify that adding another channel before looking up the previous one stays
linear too.

Mutation cases cover shifts, swaps, shuffles, deleted allocations, null slots,
same-size replacement with a new actor, reassignment of an existing channel,
indirect additions, a reentrant rebuild, and the missing-observer fallback. A
deterministic sequence compares all results with a fresh linear search of live
channels. These checks establish lookup behavior and memory safety of the helper;
they do not establish full in-game hunt loading or the injected DLL's runtime
compatibility. Verify the built DLL against the exact game binary and real joins
before reporting that integration as tested.
