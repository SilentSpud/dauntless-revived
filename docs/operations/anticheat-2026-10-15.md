# Initial native anti-cheat hardening

## Scope and deployment status

Installed for new game processes on all four game hosts. This is a deny-list for ten explicit developer/debug
server RPCs, not a verified fix for attack-speed, infinite-stamina, movement-speed
or monster-freeze cheats. The supplied videos were inaccessible; the cheat
behaviors come from the operator's description. No automatic bans are enabled.

`native/internal-server/ServerRpcGuard.h` lists exact SDK function names.
The server-only `ProcessEventHook` checks `NetServer`, caches decisions per thread,
and returns before invoking a denied handler. Each denied function is logged at
most once per thread, without arguments, player identifiers or credentials.
Normal combat, ability activation, movement and cell-fusion RPCs are unaffected
by the list. Administrative HTTP inventory grants are separate from these RPCs.

## Evidence and limits

- `SDK/Archon_functions.cpp` declares the listed functions as server RPCs with
  native validation. Their presence does not prove the native validation was
  exploitable, or that these RPCs caused the reported cheating.
- `dllmain.cpp` contains unconditional-success sprint/damage hooks, but their
  registrations are commented out. They are not active bypasses.
- Ability activation still invokes the native activation function. Adding a
  second stamina charge could charge legitimate actions twice.
- `SDK/Archon_classes.hpp` exposes combo state, montage play rate, transition
  conditions and stamina modifiers. A fixed attacks-per-second threshold ignores
  legitimate buffs and combo mechanics.
- `ProcessEvent` does not itself provide authenticated network origin. Blocking
  a named operation cannot safely identify an account to ban.
- `FallRecovery.h` intentionally teleports players and resets movement. Falls,
  corrections, timing spikes and idle monsters are never ban evidence here.

## Validation and rollout gate

The standalone `tests/server-rpc-guard.cpp` exercises every denied name, exact
matching, and permitted combat/ability/movement/cell-fusion names. It is a policy
test, not an end-to-end network test. Build with MSVC 14.44.35207 or newer, as
documented in `docs/hunt-readiness.md`; 14.36 rejects SDK delegate assertions.

Validation on October 15: the new guard test and five existing native policy
executables passed. Release/x64 DLL build passed with MSVC 14.44.35207, including
an incremental check of the final source. Existing generated-SDK enum truncation
warnings remain. Independent agent review found no blocking issue in the guard.
At that initial validation stage no production DLL was replaced and no accounts
were banned. The subsequent rollout is recorded below.

For comprehensive validation, use an isolated 1.4.4 worker and disposable account to prove
the network calls reach this hook, denied handlers do not execute, and ordinary
combat and abilities still work. Back up the previous DLL and configuration;
stage on an empty worker, then use newly started hunts. Do not interrupt active
hunts to replace their loaded DLL. Roll back by restoring the previous DLL for
new processes and draining the candidate worker.

To cover the reported cheats, reproduce each behavior and instrument native
combo transitions, stamina consumption and the actual monster mutation path.
Inspect current movement-manager defaults before evaluating Unreal's native
time-discrepancy resolution under packet loss, server hitches and fall recovery.
Account sanctions additionally require authenticated originating-connection and
session binding; do not give game DLLs the dashboard administrator credential.

## October 15 rollout

Candidate SHA-256:
`87AECD7BEBA33F479F6EF3F335E90E20DCD32FDDD31EBD3A23D6BD542B1156A0`.
Uploaded as `staging/Anticheat-Candidate.dll` beneath main, US, Germany and
new EU game-host roots; all four remote hashes match. A fresh isolated local
APIKEY backend and disposable TESTTFBTA account were used with this exact DLL.
The first backend launch failed because port 61000 was occupied; a generic
HTTP200 health response was insufficient. After retry, the unique server name,
authenticated login, tutorial occupancy of one and character saves were verified.
The operator confirmed attacks, dodges, sprint/stamina and monster attacks worked.
The standalone guard test passed again. This does not reproduce a malicious RPC
over the network or prove protection against the four reported cheats.

On October 15 (America/New_York), the candidate replaced each host's game DLL
using File.Replace after backing up the previous image and configuration.
Previous SHA256: `A090E8B25044647CE409B313525501DA578D632D3FCB9EB351F020CA94FA64FB`.
No game process was restarted. Existing hunts retain the previous loaded image;
new processes load the candidate. A new US tutorial instance started successfully
on port 8772 after replacement, with no assigned players. Main backend HTTP200
and complete fleet inventory were verified after all four replacements.

Rollback directories (timestamps use each host's timezone):
- Main: `C:/DauntlessRevived/backups/native-guard-20261016-021828`
- US: `C:/DauntlessUS/backups/native-guard-20261015-211715`
- Germany: `C:/DauntlessGermany/backups/native-guard-20261016-031832`
- New EU: `E:/DauntlessEU2/backups/native-guard-20261016-031835`

Restore the verified previous DLL for new processes, preserving running hunts.
The test setup is outside the repository at `C:/dr/anticheat-test`; its database
and credentials are isolated from production. Launcher news explicitly describes
the narrow guard, rolling process adoption and remaining unverified cheat coverage.
