# Launcher/client integration: PRs 59, 62, 63, 66–69

Originally integrated separately; launcher 0.1.38 combines that contributor history with the production Rams shop client. Offline runtime packaging does not deploy backend changes to live hosts.

| PR | Contribution | Credit |
| --- | --- | --- |
| 59 | Explain locked settings while game/tasks run | anxo0 |
| 62 | Brazilian Portuguese translation, included by 68 (455 original entries; partner-label punctuation differs) | LupesHk |
| 63 | Offline play, local services/account persistence, portable save backup/restore; includes prerequisite 61 | JusMH runtime; anxo0 UI |
| 66 | Remove vendor tab bar that exposed a crashing native purchase path | anxo0 |
| 67 | Animated streamer-mode switch | anxo0 |
| 68 | Launcher/game language options and responsive step labels | anxo0; Portuguese by LupesHk |
| 69 | Native DualSense input, rumble and PS5 prompts | anxo0; modified icons by Zacksly, CC BY 3.0 |

Original contributor commit authorship is retained. Credits and third-party notices include the people and licensed artwork. Missing offline strings in the six newly introduced dictionaries fall back to English; they are not represented as completed translations.

Integration preserves device/account checks for online play, existing server RPC protections and the server locale fix. DualSense processing is client-only. Additional checks cover malformed HID reports, stale icon-table references, reconnect rumble and foreground-window input.

## Verification and rollout gate

Individual offline integration: launcher 297 passed/3 skipped; backend 768 passed; deploy server 98 passed; database helper 2 passed. Combined launcher: 297 passed/3 skipped. Combined native Release x64 builds with MSVC 14.44.35207; DualSense, console visibility, locale and RPC guard native tests pass. Source hygiene checks pass after updating both launcher and server-kit DLL pins.

Source integration alone does not imply a public release. Packaged offline startup, account reuse and backup/restore pass for 0.1.38. Physical DualSense USB/Bluetooth validation remains outstanding; parser/native tests do not substitute for hardware testing. Keep the prior launcher release available for rollback. Offline smoke tests require unused local ports and a closed game.

Bundled combined DLL SHA-256: `106d50f6baa3bfb4b1283440ef486547b1f30bb4f54f8309ef436f455db286a7`.

## Combined 0.1.38 validation

The release merge retains the original contributor ancestry and the production Rams launch flag/native bridge. Launcher tests: 297 passed, 3 skipped; Metagame: 781 passed; Deploy: 98 passed. Combined DLL built successfully; Windows native regressions and 12 portable Linux tests plus sanitizers passed. Physical DualSense USB/Bluetooth behavior has not been tested locally.

The packaged portable-save smoke verifies separate offline account credentials and database, export/import, key replacement, migration of a historical save, DPAPI encryption, account reuse and service cleanup. Its historical fixture must omit tables absent in the older schema: newer migrations can add tables, not just columns. The old fixture otherwise generated an invalid INSERT with no columns. This is a verified test-fixture finding, applicable when synthesizing old saves from the current schema; it does not imply a runtime save migration failure. Anchor: `legacy/UndauntedLauncher/scripts/portable-save-smoke.cjs` historical table-copy loop.

Local diagnostic distributables are not release artifacts. GitHub CI rebuilds the committed source before publication.

Packaged native smoke also passed: actual client launch, authenticated local account, Ramsgate allocation on 127.0.0.1:8777, graceful client close, and complete owned service/native process cleanup. This verifies startup and allocation, not manual combat or physical controller behavior.

Windows launcher CI uses windows-2022: the locked Electron node-gyp rejects Visual Studio18 on windows-latest (CI39006447213, unsupported version18), while local VS2022 builds and packaged offline native ABI checks pass. Revisit this pin when the Electron rebuild dependency supports the newer compiler. Both CI and manual-release build jobs must use the same runner.
