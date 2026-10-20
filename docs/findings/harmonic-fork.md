---
title: The Harmonic port
parent: Findings
nav_order: 13
description: "What we took from Harmonic's Dauntless 1.4.4 fork of Undaunted and what we kept of our own, feature by feature, with the reason for every choice: Escalation, the free store, Slayer Links, the deploy-server fixes, the tests, and what we left out."
lang: en
ref: findings/harmonic-fork
---

{% assign store_page = site.pages | where: "path", "findings/store.md" | first %}
{% assign escalation_page = site.pages | where: "path", "findings/escalation.md" | first %}
{% assign social_page = site.pages | where: "path", "findings/social.md" | first %}
{% assign chat_page = site.pages | where: "path", "findings/chat.md" | first %}
{% assign contract_page = site.pages | where: "path", "findings/backend-contract.md" | first %}
{% assign mp_page = site.pages | where: "path", "findings/multiplayer.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "setup/upgrading.md" | first %}
{% assign legal_page = site.pages | where: "path", "legal.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}

# The Harmonic port
{: .no_toc }

**Harmonic** keeps another fork of Undaunted for the Dauntless 1.4.4 client,
[github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), under the same
license (AGPL-3.0-only). In September 2026 Harmonic published a large commit (`895f7c7`, "Preserve Dauntless
1.4.4 backend work") with progression, a store, Escalation, social features and 119 test cases. The
owner asked us to bring everything from it that works into Dauntless Revived.

Both forks start from the same upstream commit (`7f692aa`), and both had built some of the same
things in the same weeks. So we compared them feature by feature. **Where Harmonic's work adds
something we did not have, we took it**, apart from a few platform items listed below with their
reasons (the DLL-side ones wait for the owner's decision on shipping a DLL we build). **Where that fork
rewrote something we had already built and tried in the game, we kept ours**, and each such case
below says why, with the evidence. This page is that record, written for readers who do not know
either code base, Harmonic among them.

Thank you, Harmonic: Escalation and the store exist on our server because of your work.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## In short {#in-short}

**What we took:**

- **Escalation, complete:** the season registry Harmonic exported from the client, the save rules and all
  17 of their tests. We had only a fake "level 99999" reply. [Escalation]({{ escalation_page.url | relative_url }})
- **The free store:** Harmonic's catalogue of 200 free offers, the two-step purchase-token flow, the store
  tabs and the list of how each item is granted. Our store screen got an error.
  [The in-game store]({{ store_page.url | relative_url }})
- **Slayer Links:** Harmonic's contract, the first one that worked, corrected where the client sends or reads
  another shape. [Friends, parties and guilds]({{ social_page.url | relative_url }}#slayer-links)
- **Deploy-server fixes:** a dead Ramsgate is started again when a player travels there, and a
  missing game binary no longer brings the deploy server down.
  [How multiplayer works]({{ mp_page.url | relative_url }}#the-deploy-server)
- **Smaller ideas:** protection against retried progression grants, Hunt Pass seasons from a folder on
  disk, real currency in `/balance`, the player's own account id in `oauth/verify`, a party-invite
  accept that is sent twice, status and duration in the body log, the MinHook build fix and five
  `.gitignore` patterns.
- **Tests:** 110 of the fork's 119 test cases, adapted to our code; 7 of them now assert the opposite
  of what the fork's version did.
- **Friends' online status:** the idea, rebuilt inside our chat server.

**What we did not take, in one line each:**

| Left out | Why |
|:---------|:----|
| Paying Hunt Pass and mastery rewards when a rank is confirmed | The 1.4.4 game server already pays them through `/inventory`; paying on confirm too would pay every reward twice. |
| The fork's wallet table | Currency already lives in the inventory as stacks. |
| The fork's inventory and loadout rewrites | They would undo fixes we verified in the game (a hunt's rewards lost on an overspend, zero-quantity rewards refused, players adding loadout slots themselves). |
| The fork's bounty list and "a full reset clears the board" | Gold bounties would drop from 100 to 60 XP, and players would lose bounties they still hold. |
| The fork's chat and presence server | It never delivers room messages or whispers, and it derives from Mystic Paradox, whose extra license terms we would then carry. |
| The fork's party matchmaking | It brings back the two-player freeze we fixed on 22 September. |
| The heartbeat without a token | Anyone on the internet could open our game ports. |
| The fork's changes to the server DLL | They wait for the owner's decision to ship a DLL we build ourselves. |

## How to read the tables {#legend}

| Word | Meaning |
|:-----|:--------|
| **Adopt** | Taken, rewritten in our code's style, often with small corrections. |
| **Merge** | Harmonic's idea or part of the fork's code, combined with ours. |
| **Keep ours** | We already had this, built and tried; the fork's version is not taken. |
| **Skip** | Not taken, for the reason given. |

The evidence labels are the ones used across these pages: **B** the 1.4.4 executable (addresses with
image base `0x140000000`), **V** verified in game on our server, **C** code, **S** strong inference,
**G** a guess or a design choice.

## Platform {#platform}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| Matchmaking answers FAILED when no world can be started | **Keep ours**, adopt the fork's tests | Ours already answered FAILED in more cases, with `statusReason: null`. The fork's failure tests (a 500, a 200 without a host, a quoted port, a body that is not JSON, a dropped connection) now run against ours on three paths: none needed a code change. |
| Party-aware matchmaking | **Skip** | It sends a member to an old hunt's server, puts the whole party on the one-player tutorial, and brings back the two-player freeze we fixed on 22 September. Its cancel hook fires on the `DELETE /party/member` the client sends at every login. |
| Deploy server: store the restarted Ramsgate and Dojo record; check that the process is alive before handing out its address | **Merge** | It fixes a stale record and a dead Ramsgate that went unnoticed for up to a minute. We added one shared launch, so the watchdog and a player's request can never start two Ramsgates on UDP 8777. Switch `PERSISTENT_WORLD_LIVENESS`, on. |
| Deploy server: log a child process's `error` and `exit` | **Adopt** | Without it, a wrong `GAMESERVER_BINARY_PATH` ended the deploy server with an unhandled error. |
| Deploy server: a failed startup is logged | **Adopt** | One fatal line and a non-zero exit code instead of an unhandled rejection. |
| Deploy server: wait for the game's UDP port before answering | **Skip for now** | It would hold the player's travel request open for the whole server boot; the client's time limit for that call is unknown, and a 60-second deadline could kill a cold Ramsgate. On the roadmap (4.6). |
| Deploy server: game-server log files (`GAMESERVER_LOG_DIR`) | **Skip** | The DLL gives the engine a fixed command line and redirects its output, so the files would stay nearly empty. Log files belong to our own DLL work (4.6). |
| Deploy server: an opt-in watchdog, and `HOST` | **Skip** | With the watchdog off, hunt ports never return to the pool and every launch fails after six hunts. Our `BIND_HOST` already does what `HOST` does. |
| `METAGAME_ADDRESS` argument and an endpoint hook in server mode (deploy server and DLL) | **Skip until the DLL decision** | The two halves must ship together, and our generated `Game.ini` already does the job (V). On the roadmap (4.4, 4.6). |
| HuntDiag, a diagnostic log in the DLL | **Skip until the DLL decision** | Diagnostic only. Adding it to our DLL source while we ship the upstream DLL would make the source no longer match what we ship. |
| MinHook include fix (`hook.c`) | **Adopt** | A pure build fix; the DLL project now compiles with the Visual Studio 2022 Build Tools. Nothing built from it ships. |
| Wire capture (`WireCapture`) | **Merge into `LOG_BODIES`** | Our body log gained the reply's status and duration, an optional cap per path (`BODY_LOG_PER_PATH`), the fork's extra routes and the blanking of account keys. No second capture switch; the server kit still keeps the body log off in public mode. |
| A heartbeat that needs no token | **Skip** | Our gateway opens the game's UDP ports to any address whose heartbeat is answered 2xx; a heartbeat without a token would let anyone open them. A test now guards this. |
| Starting the chat stack always; a `clientError` logger; the start banner | **Skip** | Starting chat always would open a second, unguarded chat stack. |
| Package files and lockfiles | **Keep ours** | Ours were already current. |
| Five `.gitignore` patterns (`*.bak`, `*.bak-*`, `*.drizzle-generated`, `*.sqlite`, `*.sqlite-*`) | **Adopt** | Harmless; no tracked file matches. |

## Inventory and loadouts {#inventory}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| One inventory core that runs inside the caller's database transaction | **Merge (the pattern)** | The store needs to grant items inside its own transaction. Our inventory code was split the same way, with all 26 of our inventory tests unchanged. |
| The fork's replay ledger, quantity rules and reply order | **Keep ours** | Ours keeps the fixes verified in play (V): an overspend is clamped at 0 and logged instead of refused, a zero quantity is accepted, a transaction may spend what it grants, and every change is logged. The fork's rules would lose a whole hunt's rewards on an overspend and refuse zero-quantity mastery rewards. All 11 of the fork's inventory tests run against ours. |
| Loadout slots (up to 8, the count from the array length, unlocked by the player) | **Keep ours** | The client caps loadouts at 6 and has its own slot-count and active-slot routes (B), and a player must not unlock slots themselves. |

## Progression and the Hunt Pass {#progression}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| The XP grant reply (empty; one bad entry fails the whole grant; a server-side multiplier) | **Keep ours** | An empty reply stops rank-ups during the session, the reward payout and the automatic confirm (B `0x141464c90`). A 400 loses XP, because the game server does not retry it (B). Every shipped multiplier is 1. |
| Paying the rewards when a rank is confirmed (a claims ledger) | **Skip** | It pays twice. The game server already pays each rank through `/inventory` (B `0x141472fa0`, `0x1414876a0`, `0x141481920`), and we saw each reward granted once (V, roadmap 2.10). On our database the ledger would start empty and pay again every rank players had already confirmed, premium included. A test guards that a confirm grants nothing. |
| A confirm grants the rank's permanent entitlements | **Merge, off** | Only the game's progression grant names an entitlement grant (B `0x141487aa5`); whether the Elite ranks' cosmetics arrive by themselves is open. Built behind `PROGRESSION_CONFIRM_ENTITLEMENTS` (off) for the in-game test to decide. It never grants items or timed boosts. |
| Rank math capped at 50 | **Keep ours** | The season09b pass has prestige ranks past 50; a cap would hand out the prestige currency again and again. |
| Hunt Pass seasons from a folder on disk, `ACTIVE_HUNT_PASS` | **Merge** | One loader for the config route and the rank math, checked at startup; a bad file stops the start. `PROGRESSION_CONFIG_DIR` and `ACTIVE_HUNT_PASS` (default `season09b`). |
| "Free Elite" mode | **Skip** | Every account already owns the Elite pass on our server. That mode made the server and the client disagree. |
| Protection against replayed grants | **Merge the goal** | The retry risk is real (the game server retries up to 5 times, B). The fork's rule could drop legitimate XP; ours answers a byte-identical grant repeated less than 5 seconds later with the first answer and adds nothing (`PROGRESSION_REPLAY_WINDOW_S`; under the game server's 10-second grant flush). An objective that goes backwards is only logged. |
| A save with an equal version accepted | **Keep ours (409)** | The client retries cleanly after a 409 (V, roadmap 2.1). |
| A log line for progression requests no route answers | **Adopt** | Harmless and useful. |
| Extra confirm kinds (`free`, `normal`, `1`, `2`) | **Skip** | The 1.4.4 client sends only `public` and `premium`. |
| The fork's entitlements code | **Keep ours** | Ours has real grants, expiry, revocation and defaults. |
| A wallet table for `/balance` | **Skip the table, adopt the idea** | `/balance` and `/reconcile` now report the currencies the character actually holds (`BALANCE_FROM_INVENTORY`, on). `CURRENCY_PLATINUM_UNIV` is not mapped: the executable names it only next to the Elite upsell screen, not where balances are read (S). |

## Escalation {#escalation}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| The season registry, the save rules, the version rules and the routes | **Adopt, adapted** | We had only a stub. The fork's level table matches our own reading, and the talent-cost rule matches the executable (B `0x1414ae2a0`, `0x1414c0600`). Our changes: every save is audited in the existing progression log (no second events table), a relayed token of another account is logged, not refused, the stub guard refuses any save at level 25 with 99,999 XP or more unless the stored season is already at 25 (the fork's checked a first save with exactly 99,999), and the rules that depend on modelling only warn until `ESCALATION_STRICT=1`. Off by default: switching it on drops players from the fake maximum to level 0. |

## Bounties and cooldowns {#bounties}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| The bounty board as one document per player, cleared on a "full reset" | **Keep ours** | At a new season the game posts an empty board and an empty draft while players still hold bounties (B `0x1413f8009`, `0x1413f0f60`); the fork's full reset would delete them. Ours stores each bounty with its version and an audit trail. |
| A list of 226 bounty definitions | **Skip** | A bounty missing from the list is allowed (B `0x1413f5a81`), and its reward then falls back to the client's own table (B `0x1413df5e1`): Bronze 20, Silver 40, Gold 100 XP (S). The fork's list would cut Gold to 60. |
| More cooldown routes, and players allowed to write cooldowns | **Keep ours** | Start and batch are `PUT` in the client (B `0x14144d366`, `0x14141e037`), the extra route is never sent, and a player who could write cooldowns could reset their own daily limits. Ours passed its in-game test (V, roadmap 2.5). |

The fork's bounty and cooldown tests were ported; two now assert our behaviour instead (a new-season post keeps
held bounties, and `/bounty/game-data` keeps an empty bounty list).

## The store {#store}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| The free storefront: four routes, the purchase token, the catalogue, the tabs, the item kinds, sheens and hair tints | **Adopt, adapted** | We had no store. Items go through our inventory core and entitlements through our own grant code, so every purchase is in the ledger and the logs. The fork's version refused accounts with more than one character; ours sends the purchase to the character saved last. Off by default (`STORE`) until the owner decides free or priced and a store test passes in game. |
| The ten-rank Hunt Pass offer | **Skip** | It could not be bought even on the fork's server (its currency has no grant kind). A rank skip is a progression grant. |
| The repeatable bounty-token bundle | **Adopt, the owner decides** | Unlimited free premium bounty drafts; hidden unless `STORE_REPEATABLE_TOKENS=1`. |

## Social {#social}

| Harmonic's work | Decision | Why |
|:---------|:---------|:----|
| XMPP transport (WebSocket on the metagame port and raw TCP on 60002) | **Keep ours** | The 1.4.4 client connects over WebSocket only (B `0x143a1eb8e`). The fork's limits were not enforced and every frame was captured. The files derive from Mystic Paradox. |
| Chat rooms and whispers | **Keep ours** | The fork's server never delivered a room message or a whisper. Our usernames fix stays as it is. |
| Friends' online status | **Merge: rebuilt in our chat server** | The fork's service sent no status text, never passed changes on, and marked players offline from an address the client ignores (B `0x143a382b0`), so friends stayed online forever. We kept only the moments that trigger an update. On by default since 30 September (`CHAT_PRESENCE=0` rolls back); the live two-player party test remains required. [Text chat]({{ chat_page.url | relative_url }}#presence) |
| The friends routes | **Keep ours** | The fork's block list is sent under a key the client does not read (`blocklistedUsers`; the client reads `blockedUsers`, B `0x1443fd258`), and its friends route uses the Mystic Paradox-derived presence code. |
| Parties | **Keep ours, merge the accept retry** | The fork's accept looks up an invite id the client never sends (it sends the party id, B `0x140b35384`), and it had no decline, leader removal or party status. We took its tolerance for an accept that arrives twice. |
| Guilds | **Keep ours** | The fork has upstream's stub. |
| Slayer Links | **Adopt, corrected** | A real 1.4.4 feature. Read from the executable: the invite list's other player is `account_id`, not `linked_account_id`; removing a link is `DELETE /slayerlink/links` with a body, not `/slayerlink/link`; `DELETE /slayerlink/invites/<id>` was missing; the status reply nests invites, links and settings; and the slots are numbered 1 to 3, not 0 to 2. It only answers routes that got 404 before, so it is on (`SLAYER_LINKS`). |
| `oauth/verify` names the caller's own account | **Merge (the account id)** | That is what the real service answered. We never answer 401: the client checks every 30 seconds, and after the 24-hour token expiry a 401 could log players out. `VERIFY_STUB_ACCOUNT=1` brings back the placeholder. |
| Account lookups that require a token (401) | **Keep ours** | Our soft check gives the same answers without refusing anyone. |
| `/account/mapping`, `/accountinfo/public` | **Already ours** | We made the same fixes on 22 September. |
| A route for `GET /account127.0.0.1:61000` | **Keep ours (404)** | The client builds one URL without a slash after `/account`. The fork's route answers it with account data; the request carries no credentials, nothing visible breaks today, and the real fix belongs in the DLL (roadmap 4.6). |
| `GET /present/<account>` | **Skip** | No caller in the client. |

## Licence, data and credit {#licence}

| Item | Decision | Why |
|:-----|:---------|:----|
| The fork's realtime files, derived from Mystic Paradox | **Copy none** | Nothing we took needs them. So our statement that Dauntless Revived contains none of Mystic Paradox's code stays true, and Mystic Paradox's additional terms (AGPLv3 section 7) do not apply here. |
| `NOTICE.md` | **Adopt (without the Mystic Paradox part)** | The repository now has a `NOTICE.md` that records what comes from Undaunted and what from Harmonic's fork. |
| `ADDITIONAL_TERMS.md` | **Skip** | It covers only Mystic Paradox material, which we do not carry. |
| The fork's data files: the Escalation registry, the store catalogue, the item kinds, the store art list | **Adopt** | Identifiers and tuning values read from the client for interoperability, about 330 short English names, and no game assets. A provenance note was added to each file. The Escalation registry names the game files it was read from by their SHA-256. |
| The fork's bounty definitions | **Skip** | See [Bounties and cooldowns](#bounties). |

**How Harmonic's work is credited:**

- The four source files ported from the fork (`escalationConfig.ts`, `escalation.ts`, `freestore.ts` and
  `slayerlinks.ts` in `legacy/UndauntedMetagame/src/controllers/`) start with `Ported from
  Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only`, followed by how they
  were modified (or, for Slayer Links, rewritten and corrected) for Dauntless Revived. Code of ours
  built on Harmonic's ideas names the fork in its comments.
- Every commit that carries Harmonic's work names Harmonic as co-author and says "Ported from
  github.com/Harmonicrain/Undaunted 895f7c7."
- Harmonic is listed on [Credits and license]({{ legal_page.url | relative_url }}), in the README and on the
  launcher's Credits page (from launcher 0.1.6).

## Tests {#tests}

The fork's test files hold 119 cases (one of them loops over several failure modes, about 122 when run). We
wrote each again in TypeScript against our own test helpers, and each carries a comment naming the original file
and line.

| The fork's file (cases) | Ported | Inverted (they now assert our behaviour) | Skipped, and why |
|:-----------------|:-------|:-----------------------------------------|:-----------------|
| entitlements (18) | 17 | | 1: covered by our own entitlement tests |
| escalation (17) | 17 | | |
| free store (7) | 7 | | |
| Hunt Pass progress (27) | 21 | 3: a confirm grants nothing; a new-season post keeps held bounties; the bounty list stays empty | 3: we store unknown tracks on purpose; no wallet; no bounty list served |
| Hunt Pass (19) | 18 | 1: season09b goes past rank 50 with prestige | |
| inventory (11) | 11 | | |
| mastery (11) | 6 | 3: a retried grant adds nothing; a confirm pays nothing, and the reward arrives once through `/inventory`; one bad entry does not lose the others' XP | 2: objectives stay as sent; no payout to roll back |
| matchmaking (5) | 4 | | 1: the leader takes the whole party anywhere, which contradicts our party design |
| social and friends (4) | 2 (verify never answers 401; Slayer Links with the client's exact requests) | | 2: they assert the block-list key the client does not read, and an accept by an id the client does not send |
| **Total (119)** | **103** | **7** | **9** |

Our metagame suite went from 362 to 567 tests and the deploy server's from 12 to 26, all passing.

## Migrations {#migrations}

Three new migrations, new tables only: `0014_escalation`, `0015_store_purchases` and
`0016_slayer_links`. No existing table is changed, copied or converted, and none of the fork's tables whose
names clash with ours is ever created. The build before the port still starts on a migrated database
and ignores the new tables. A test fills a database at migration 0013 with rows of every kind, runs the
new migrations twice and checks that every existing row is unchanged. What this means for a server
that already has players is on [Upgrading]({{ upgrade_page.url | relative_url }}#harmonic-port).

## What waits {#waits}

- **Decisions for the owner:** a free or priced store; unlimited bounty tokens; when to switch
  Escalation on; whether an unfriend also ends a running Slayer Link; shipping a DLL we build
  ourselves (which would bring in the fork's DLL changes); and, before the push, confirming that
  "Harmonicrain" is Harmonic's own GitHub account: the push publishes launcher 0.1.6, whose Credits
  page shows that handle, unless automatic launcher releases are paused (`LAUNCHER_AUTO_RELEASE=false`).
- **Tests in the game:** a killed Ramsgate, Escalation, the store, the Elite ranks' entitlements,
  Claim, Slayer Links, online status with two players, and a long session with the new `oauth/verify`.

They are on the [roadmap]({{ roadmap_page.url | relative_url }}), and the switches that turn each part on
or off on [Configuration]({{ config_page.url | relative_url }}#feature-switches).
