---
title: Escalation
parent: Findings
nav_order: 12
description: "How Dauntless 1.4.4 saves Escalation progress, and the real Escalation saves we built from Harmonic's fork: the season registry read from the client, the rules every save must pass and the ones that only warn, the old stub, and what players see when it is switched on."
lang: en
ref: findings/escalation
---

{% assign api_page = site.pages | where: "path", "reference/api.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "reference/files.md" | first %}
{% assign game_page = site.pages | where: "path", "reference/game-settings.md" | first %}
{% assign contract_page = site.pages | where: "path", "findings/backend-contract.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "findings/harmonic-fork.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}

# Escalation in 1.4.4
{: .no_toc }

Escalations are the multi-stage behemoth runs of Dauntless (Shock, Blaze, Umbral and Terra in 1.4.4).
Each has a season track of its own: an Escalation level that rises with play, talent points to spend
on the season's talents, and rewards unlocked at certain levels. This page describes how the 1.4.4
game saves that track, and the real saves the metagame now offers.

**Status (23 September 2026): built and tested without the game, off by default
(`ESCALATION_MODE=stub`).** By default every player still gets upstream's fake maximum and nothing is
saved, exactly as before. With `ESCALATION_MODE=real`, players in real progression mode keep their own
Escalation level, talents and rewards. Switching it on is the owner's decision after an in-game test,
because every player then starts again from level 0 (see [Switching it on](#switching-it-on)).

The season registry, the save rules and their tests come from **Harmonic's** Dauntless 1.4.4 fork
([github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), commit `895f7c7`).
We adapted them to our audit log and made the less certain rules warn instead of refuse; details on
[The Harmonic port]({{ harmonic_page.url | relative_url }}).

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Evidence and confidence {#evidence}

| Label | Source |
|:------|:-------|
| **B** | The 1.4.4 executable (`Dauntless-Win64-Shipping.exe`): disassembly. Addresses are virtual addresses with image base `0x140000000`. |
| **K** | The endpoint table in `native/internal-server/dllmain.cpp`, and the client's own data tables. |
| **S** | Strong inference: one link in the chain was not traced. |
| **G** | A design decision of ours, where nothing in the client decides it. |
| **C** | Our own code and tests. |

## How the game saves Escalation {#contract}

Two endpoint keys, both on the same path (K `dllmain.cpp`):

| Key | Request | Caller |
|:----|:--------|:-------|
| `GetSeasonalEscalationEndpoint` | `GET /escalation/<season id>/<account id>` | the client and the game servers, when they load a player |
| `UpdateSeasonalEscalationEndpoint` | `POST /escalation/<season id>/<account id>` | the game server, with the whole season as the body |

Both replies are read through the Phoenix envelope `{"code", "message", "payload"}` (B `0x140aae300`).
The payload, and the body of a save, is one season:

```json
{
  "escalation_level": 3,
  "next_level_xp": 140,
  "talents_progress": [ { "rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADEONE" } ],
  "unlock_progress": [ { "collected": true, "reward_id": "ESC_Reward_5" } ],
  "update_version": 7
}
```

- `escalation_level` is the level reached (0 to 25), and `next_level_xp` the XP gathered towards the
  next one.
- **The game server does the arithmetic** (S, Harmonic's reading of the native code): it adds the XP,
  raises the level, lets the player spend points and hands out the rewards itself, then posts the
  whole season. The backend only keeps the last good snapshot and refuses one that no real play could
  produce, or that would erase newer progress.
- `update_version` is a counter the game raises before every save, starting from the value it loaded
  (S), so a real save is at least version 1.
- The 2.1.1 client asks for `ESC_SEASON_1` to `ESC_SEASON_6` at every city load
  ([Backend contract]({{ contract_page.url | relative_url }})); which ones 1.4.4 asks for is seen in the
  request log.

## The season registry {#registry}

`legacy/UndauntedMetagame/src/vendor/escalation/seasons.json` lists every season the 1.4.4 client has,
exported read-only from the client's own Escalation progression table and the talent table each
season names (by Harmonic, from build CL239827). The metagame checks the file when it starts, and a
malformed one stops the start.

| Season | Name | Saved |
|:-------|:-----|:------|
| `ESC_SEASON_1` | Shock Escalation | yes |
| `ESC_SEASON_2` | Blaze Escalation | yes |
| `ESC_SEASON_3` | Umbral Escalation | yes |
| `ESC_SEASON_4` | Terra Escalation | yes |
| `ESC_SEASON_5` | Frost Escalation, marked "do not translate" in the client (unfinished) | no: disabled, every save gets 409 |

Each season has:

- **25 levels.** Level 1 costs 500 XP and level 25 costs 10,000; all 25 add up to 109,000 XP. A new
  player is at level 0.
- **18 talents in 6 tiers** of three. A tier opens once 0, 4, 8, 12, 16 or 20 points have been spent in
  the tiers below it, and each rank of a talent costs points (one point is earned per level). The cost
  of a rank is the sum of the talent's first rank costs, as the game's own `GetSpentTalentPoints`
  counts it (B `0x1414ae2a0`), and a rank-up is refused while the tier gate is not met
  (`SharedUpgradeTalent`, B `0x1414c0600`).
- **6 rewards**, at levels 5, 8, 10, 15, 20 and 25: stat boosts, a patrol chest bonus, an extra relic
  choice and an item grant at level 25.

These values are used only to check a save. They are never used to work out a player's progress,
which stays the game server's job. The level table agrees with our own earlier reading of the client
(C).

## The rules a save must pass {#rules}

Every save is checked in one database transaction. **Hard rules** are always enforced:

| Rule | Answer when broken |
|:-----|:-------------------|
| The account exists | 404 |
| The season is in the registry | 404 |
| The season is not disabled (Frost) | 409 |
| `escalation_level` is a whole number from 0 to 25, `next_level_xp` a whole number from 0, `update_version` from 1, all within 32 bits | 400 |
| Every talent and reward id belongs to that season, none twice; a talent's rank is at most its number of ranks; `collected` is true or false | 400 |
| **Version order:** the stored version with the stored content is a retry and gets the stored state back, with nothing written; the stored version with other content, or an older version, is refused | 200 (retry), 409 |
| Nothing goes down: a lower level, or less XP at the same level | 409 |
| A collected reward stays collected | 409 |
| **The stub guard:** a save may not be level 25 with 99,999 XP or more unless the stored season is already at level 25: not a player's first save, and not a jump from a lower stored level. That is what a game server holds when it loaded the old stub, and it must never become a player's real progress. Such a game server keeps counting its version while its saves get 404 in stub mode, so after a switch back to `real` its version can be newer than the stored one; the guard refuses it over a stored season too. Only a player already stored at level 25 builds up that much XP. | 409 |

Talent ranks may go down: a talent reset is a normal game action, so the stored talents are replaced
by what the save lists.

**Soft rules** depend on how we modelled the talent tiers, the level costs and the reward levels. A
wrongly modelled rule would refuse every later save of a player, so by default they only warn:

| Soft rule | Default (`ESCALATION_STRICT` off) | `ESCALATION_STRICT=1` |
|:----------|:----------------------------------|:----------------------|
| `next_level_xp` is below the cost of the next level | stored, with a warning line and a note in the audit row | 409 |
| The talents spend no more points than the level has earned | the same | 409 |
| A talent is held only when its tier's gate is covered by points spent in lower tiers | the same | 409 |
| A reward is collected only at or above its level | the same | 409 |

The stored state is canonical: talents and rewards in the season's own order, talents at rank 0 left
out, only collected rewards kept. The same state therefore always reads back the same way.

**Every save is audited.** Each `POST` in real mode, accepted, repeated or refused, is a row in
`progression_events` (the same append-only log as the Hunt Pass and mastery), with the caller, the
body, the reply and a note: `season <id>`, then `replay of version N, nothing changed`,
`accepted although ... (ESCALATION_STRICT=0)` or the reason for a refusal.

**Who may save.** Only a game server (its key, from this machine): a player's own client gets 403, so
nobody can write their own Escalation. When a game server's request carries the token of another
player than the one in the URL, the save is kept for the URL's account and a warning is logged; it is
never refused, because a refusal could lose a save in a two-player hunt (G). A player may read only
their own seasons; a game server reads any.

## Stub and real {#modes}

| | `ESCALATION_MODE=stub` (default) | `ESCALATION_MODE=real` |
|:-|:---------------------------------|:-----------------------|
| `GET` | Everyone: `{"code": null, "message": "OK", "payload": {"escalation_level": 99999, "next_level_xp": 99999, "talents_progress": [], "unlock_progress": [], "update_version": 1}}`. The game shows the last level. | Accounts in real progression mode: their stored season, or level 0 with version 0 when nothing is stored yet (reading creates no row). Accounts in stub progression mode keep the stub. An unknown season: 404. |
| `POST` | 404, as always: nothing is saved. | Saved under the rules above. |

A player's stored seasons stay in their tables when the mode goes back to `stub`, and come back when it
is `real` again. Restart the game servers together with the metagame at every switch, in either
direction (step 3 below).

## Switching it on {#switching-it-on}

**What players see:** every player in real progression mode drops from the fake maximum (level 25 and
25 talent points) to level 0, and then levels up for real. Tell them before.

1. Take a backup (the kit takes one at every start).
2. Set `ESCALATION_MODE=real` in the metagame's settings.
3. Restart the game servers together with the metagame, when nobody is playing, so that no game server
   still holds the stub's values and writes them back. The stub guard refuses such a save anyway, also
   over a stored season, but then that player's Escalation progress in the session is refused with it
   until the game server loads the season again. The same restart is needed when the mode goes back to
   `stub`, or an account leaves `PROGRESSION_REAL_ACCOUNTS`, and later returns.
4. The in-game test (roadmap 2.16): play an Escalation run, relog, spend a talent point. The level
   rises and survives the relog, the right number of points shows, and the log has no 409 and no
   "breaks a soft rule" line.
5. Once the logs stay clean for a while, `ESCALATION_STRICT=1` turns the soft rules into refusals.

When the test has passed, the plan is for an unset `ESCALATION_MODE` to follow `PROGRESSION_MODE`.

The log lines are on [Troubleshooting]({{ trouble_page.url | relative_url }}#log-lines-of-the-port),
the switches on [Configuration]({{ config_page.url | relative_url }}#metagame-escalation), the routes on
[HTTP API]({{ api_page.url | relative_url }}#progression-hunt-pass-entitlements-cooldowns-and-bounties),
the three tables on [Files and data]({{ files_page.url | relative_url }}#escalation-and-store-tables) and the
registry file on [Game settings]({{ game_page.url | relative_url }}#escalation-seasons).

## Tests {#tests}

`legacy/UndauntedMetagame/test/escalation.test.ts` holds 27 cases: Harmonic's 17 (the registry, reads, the
version rules, the hard and soft rules, the stub guard), adapted to our answers (a foreign read is 403,
a replay is an audited 200, a relayed token of another account is logged and accepted, the soft rules
answer 409 only with `ESCALATION_STRICT=1`, the strict XP case reaches level 25 before its XP grows), a
warn-only twin of each soft-rule case, the stub guard over a stored season, stub mode for everyone, stub-mode accounts with `ESCALATION_MODE=real`, a player's own save (403, nothing stored),
an unknown account, and the audit row.

## Still open {#open}

| Open point | Label | How it is settled |
|:-----------|:------|:------------------|
| The game server does all the Escalation arithmetic and posts the whole season | S | The in-game test: the saves arrive and nothing is refused. |
| Which season ids 1.4.4 asks for (2.1.1 asks up to `ESC_SEASON_6`, which the registry does not have) | unknown | The request log of one city load in real mode. |
| The soft rules match the game | S for the tier gate and the point count (B), G for the rest | No "breaks a soft rule" line in the test. |
| When to switch it on | the owner's decision | After the test and an announcement. |
