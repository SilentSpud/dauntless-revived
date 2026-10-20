# Fall-recovery candidate: in-game acceptance test

Status: candidate, not a verified live fix. The pure C++ policy tests do not prove Unreal collision,
Blueprint timing, client replication, or camera/input recovery. Do not promote the bundled DLL pins
or publish a launcher release until the checks below pass.

## What changed

The previous server loop saved Blueprint `LastValidPlayerTransform` when the Blueprint `IsFalling`
flag was false, then retried `RecoverFromFall` only on a rising `InFallRecovery` edge. That is not
proof of ground contact and does not handle a stuck flag or repeated bad teleports.

The candidate retains the original group/slot-aware player-start fix and adds an authoritative,
10 Hz fall watchdog. It remembers four recent actor/capsule-centre transforms only after sustained
walkable floor contact and a fresh collision check. Destinations are checked again before use,
including a full-capsule clearance sweep. Missing navigation projection is not a reason to use
an uninitialized transform or the world origin.

Native recovery gets a three-second grace period. Recovery that remains unresolved across flag
changes can retry every three seconds; a falling pawn without a recovery flag is considered only
after ten seconds, a 3000-unit drop, and downward velocity. Crossing the world's KillZ while falling
can trigger earlier recovery. Flying/custom movement does not trigger the long-fall fallback.

The fallback tries recent safe positions, then the actual native-selected player start in the same
world. It clears movement velocity/forces, restores capsule collision, and uses the owning-client
location/recovery RPCs. It does not heal/revive, spend items, reload maps, restart hunts or modify
accounts. State is discarded on disconnect, controller/pawn replacement and world change. Failed
join start records expire. No safe placement means no forced teleport.

## Deploy for testing

This is a **server-side** fix. Installing it only in a player's launcher/game folder cannot fix a
remote host that is still running the old server DLL.

1. Obtain `internal-server-dll` from the CI run for this exact candidate commit. Compare its SHA-256
   with the staged candidate checksum. The launcher artifacts from that run still contain the
   existing production-pinned DLL; they are NOT the fall-recovery candidate.
2. On an isolated test host, stop its deploy supervisor before its Ramsgate/training/hunt workers.
   Back up the existing `Archon/Binaries/Win64/UndauntedInternalServer.dll` and record the game root.
3. Copy only the candidate DLL to that game root. On Linux this is the Windows x64 DLL used by the
   Wine worker; rebuilding just the Node services does not update the native game server.
4. Restart the test host/workers. Use a disposable test account and an expendable hunt. Do not run
   a launcher repair or `prepare-game.sh` afterward: production checksum pins can replace/reject
   the candidate. Keep production pins unchanged until acceptance.
5. Verify `[fall-recovery] restored` in the worker console/log for a watchdog rescue. A native
   successful recovery may not need the watchdog and therefore may not produce that line.
   `[fall-recovery] no safe destination` is a failed attempt, not a successful repair.

## Required cases

| Case | Pass condition |
| --- | --- |
| Ramsgate cliff beside Lady Luck, walking and jumping off | Return fully above the floor; camera visible, movement/jump/dodge usable; no travel or Help menu needed. Repeat at least ten times. |
| Other Ramsgate edges, including falling again immediately after recovery | No under-map loop or poisoned return position. |
| Training grounds, multiple edges | Same result; training session is preserved. |
| Private hunt, preferably with a second player | Both clients see the rescued player above ground. Behemoth state, hunt timer/progress, inventory and remaining revives are unchanged by the fallback. |
| Two players falling together and alternately | Each returns to their own position; neither inherits another player's history. |
| Ordinary jumps, drops, aether jets, attacks/knockback and legitimate traversal | No unwanted rescue or movement interruption. Include the longest normal traversal available. |
| Arrival cinematic and loading into each map | No premature teleport or interference with arrival. |
| Death/bleedout followed by legitimate resurrection | No automatic revival; fresh valid ground is recorded afterward. |
| Reconnect, Ramsgate/training travel and new hunt | No recovery to the previous pawn/world's coordinates. |
| Help > My player is stuck | Existing manual recovery still works. |
| Slow client/high latency and respawn near other players | No persistent snap-back, fade-to-black, lost input or capsule overlap. |

If camera or input remains stuck, report that separately from placement: a successful server
teleport alone is not acceptance. Save the map/location, server DLL SHA-256, time, whether the
watchdog logged success/failure, and a short video. Do not post account tokens or full private logs.

## Rollback

Stop the test supervisor/workers, restore the backed-up DLL, then restart them. No database or save
migration is part of this candidate. Keep the previous DLL until all acceptance cases pass.

## Automated checks

From the repo root, compile and run every `native/internal-server/tests/*.cpp` with C++20. The policy
suite checks invalid time/coordinates/floors, safe capture timing, normal traversal, missing anchors,
prolonged falls, persistent/flickering recovery flags, cooldowns, settling, KillZ, death and independent
player state. The CI native DLL job verifies compilation against the actual 1.4.4 SDK. Neither replaces
the in-game cases above.
