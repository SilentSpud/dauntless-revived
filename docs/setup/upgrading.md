---
title: Upgrade notes
parent: Setup
nav_order: 9
description: "What changes for players when you update an existing Dauntless Revived server: the port of Harmonic's fork (Slayer Links on; Escalation, the store and online status built but off), text chat, friends, parties and guilds, and real progression on by default. How to keep old max ranks, start fresh, or stay on the stub."
lang: en
ref: setup/upgrading
---

{% assign host_page = site.pages | where: "path", "setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "setup/admin.md" | first %}
{% assign winserver_page = site.pages | where: "path", "setup/windows-server.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}
{% assign friends_page = site.pages | where: "path", "setup/friends.md" | first %}
{% assign social_page = site.pages | where: "path", "findings/social.md" | first %}
{% assign config_page = site.pages | where: "path", "reference/configuration.md" | first %}
{% assign chat_page = site.pages | where: "path", "findings/chat.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "findings/harmonic-fork.md" | first %}
{% assign escalation_page = site.pages | where: "path", "findings/escalation.md" | first %}
{% assign store_page = site.pages | where: "path", "findings/store.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}

# Upgrade notes
{: .no_toc }

Read this before you update a server that already has players. Each note says what changes, what
your players will see, and what you can do about it. The newest change is first.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Escalation, the store, Slayer Links and smaller fixes {#harmonic-port}

**September 2026.** Applies to every server updated to the version with the port of Harmonic's 1.4.4
fork ([The Harmonic port]({{ harmonic_page.url | relative_url }})). You can tell it by the metagame's
start line that begins `features:`.

### What changes {#harmonic-port-what-changes}

- **Three new migrations run by themselves at the first start:** `0014_escalation`,
  `0015_store_purchases` and `0016_slayer_links`. They only add six tables (`escalationprogression`,
  `escalationtalents`, `escalationunlocks`, `storepurchases`, `slayerlinkinvites`, `slayerlinks`); no
  existing table or row is changed, copied or converted. Back the database up first as for any update
  ([Back up the database]({{ admin_page.url | relative_url }}#back-up-the-database); the kit does it by
  itself). The previous version still starts on a migrated database: it applies nothing it does not
  know and ignores the new tables, so going back needs no restore. That was checked on a copy of a
  database at `0013` full of rows: every row stayed as it was, through both builds.
- **On by default**, because each only answers where the server gave an error or a stub before:

  | Change | Switch to turn it off |
  |:-------|:----------------------|
  | [Slayer Links](#harmonic-port-what-players-see) (the My Links tab): eight new routes | `SLAYER_LINKS=0` |
  | The game's regular session check (`oauth/verify`) names the player's own account instead of a placeholder. Every player's game calls it. | `VERIFY_STUB_ACCOUNT=1` |
  | `/balance` and `/reconcile` report the currencies the character holds (Rams, platinum) instead of 0 | `BALANCE_FROM_INVENTORY=0` |
  | A game server's XP grant repeated byte for byte less than 5 seconds later is answered, not added again | `PROGRESSION_REPLAY_WINDOW_S=0` |
  | A party accept that arrives twice is answered with the party instead of 404 | none |
  | The deploy server starts a dead Ramsgate or Dojo again when a player travels there, instead of waiting for the watchdog (up to a minute) | `PERSISTENT_WORLD_LIVENESS=0` (in the deploy server's settings) |

- **Built but off by default**, each until a decision or an in-game test:

  | Feature | Switch | Why it waits |
  |:--------|:-------|:-------------|
  | Real Escalation saves | `ESCALATION_MODE=real` | Every player drops from the fake maximum (level 25) to level 0. [Escalation]({{ escalation_page.url | relative_url }}#switching-it-on) |
  | The free in-game store | `STORE=free` | Free or priced is still to decide (roadmap 3.7), and the store has not been tried in game. [The in-game store]({{ store_page.url | relative_url }}#open) |
  | Unlimited premium bounty tokens in the store | `STORE_REPEATABLE_TOKENS=1` | Your decision. |
  | Friends' online status in chat | On by default; `CHAT_PRESENCE=0` disables | Existing opt-outs are preserved. Run the two-player party test. [Text chat]({{ chat_page.url | relative_url }}#how-to-verify-presence) |
  | A rank confirm also grants the rank's permanent entitlements | `PROGRESSION_CONFIRM_ENTITLEMENTS=1` | Only if the game server turns out not to grant them itself. |
  | Strict Escalation rules | `ESCALATION_STRICT=1` | After Escalation has run a while with clean logs. |

- **The deploy server logs more:** one line each time a game server exits (`... exited with code 0`
  after every hunt), a fatal line when its start fails, and a failed game-server start answers the
  metagame with a clear 500 (the players see their search fail, as before).
- **Nothing else to do:** no firewall rule, no new port, and no new launcher (launcher 0.1.6 only has
  updated credits). The server DLL is unchanged. The server kit writes none of the new switches, so the
  defaults above apply until you add a line to `metagame.env` (or `deployserver.env`).

### What your players will see {#harmonic-port-what-players-see}

- **Slayer Links** may work in the Social panel's My Links tab: invite a friend into one of three slots,
  and the link runs for a week. The client may keep that tab hidden; nobody has tried it in game yet.
- Where the game reads `/balance`, the Rams show what the character really holds (it was 0).
- If Ramsgate has crashed, the next player who travels there waits a little longer (about 10 seconds
  with the kit) while it starts again, instead of landing on a dead server.
- Nothing else until you switch something on: Escalation still shows the fake maximum, the store screen
  still gets its old error, and everyone still shows as offline.

### What you can do {#harmonic-port-what-you-can-do}

- After the update, look for the `features:` line in the metagame's log: it lists every new switch
  with its value ([Configuration]({{ config_page.url | relative_url }}#metagame)).
- Switching a feature on is one line in the metagame's settings and a restart;
  [Run it for a group]({{ admin_page.url | relative_url }}#switching-features-on) lists the steps and
  the tests for each.
- If something misbehaves, the switches in the first table above turn each change off. The log lines to
  look for are on [Troubleshooting]({{ trouble_page.url | relative_url }}#log-lines-of-the-port).
- **Going back** to the build before the update (`Update-DauntlessServer.ps1 -Rollback`) needs nothing
  else: the older code ignores the new switches and tables. Escalation goes back to the stub (saved
  seasons stay unused in their tables), `/slayerlink` answers 404 again, and store purchases already
  made stay in the players' inventories and entitlements. They can be found (`inventorylog.caller =
  'store'`, `entitlements.source LIKE 'store:%'`) and taken back with the admin routes.
- Never edit a migration after it has run on your server.

## Text chat {#chat}

**September 2026.** Applies to every server updated to the version with text chat (the kit then has
`Set-Chat.ps1`, and `Stack.ps1 status` has a `chat` line).

### What changes {#chat-what-changes}

- **Nothing for players until you switch chat on.** Chat is a listener inside the metagame, off by
  default: the kit writes `CHAT=0` unless you choose `-Chat On`. While it is off, the game's chat
  connection gets 502 from the gateway and retries every 15-45 s, harmlessly, as before.
- **`Set-Chat.ps1` arrives with the update.** Switch chat on afterwards, in a second run when nobody
  is playing (it restarts the stack): `Deploy-Remote.ps1 -Server <address> -Chat On` from your PC, or
  `Set-Chat.ps1 -On` on the server ([Windows server kit]({{ winserver_page.url | relative_url }}#chat)).
  `-Chat` cannot go with `-Update`.
- **The gateway gives the game's chat connections a rate bucket of their own** (`GATEWAY_RATE_WS`,
  default `20,12`: 20 at once, then 12 per minute per address), so chat reconnects never use up the
  budget a player's game traffic needs ([Configuration]({{ config_page.url | relative_url }}#gateway)).
- **No database migration, no firewall rule and no new launcher.** Every launcher from v0.1.0 on
  relays chat; v0.1.6 has the newest credits.

### What your players will see {#chat-what-players-see}

With chat on: Ramsgate and hunt chat, party chat, guild chat and whispers, all with usernames. Ramsgate
chat is per session for now, so two players share it only when they travelled to Ramsgate together as
a party. A game that was already running when chat went on connects within about 45 s. Online status
(friends showing as online) came with a later update, off by default ([above](#harmonic-port)).

### What you can do {#chat-what-you-can-do}

- The first time, run the two-player test under
  [Text chat]({{ chat_page.url | relative_url }}#how-to-verify), which lists the log lines to expect.
- If real players are refused room joins with `reason=nick-resource`, `nick-format` or `nick-name`, set
  `CHAT_NICK_CHECK=log` in `metagame.env` and restart when nobody is playing. For anything serious,
  `Set-Chat.ps1 -Off`.
- **Going back** to the build before the update (`Update-DauntlessServer.ps1 -Rollback`) needs
  nothing else: the older code ignores `CHAT` and `GATEWAY_RATE_WS`.

## Friends, parties and guilds {#social}

**September 2026.** Applies to every server updated to the version with guilds (the metagame's log
shows `guild:` lines, and `GET /guild/invite/player` no longer logs "Guild invites (stubbed)").

### What changes {#social-what-changes}

- **A new migration, `0013_guilds`, runs by itself at the first start.** It only adds three tables
  (`guilds`, `guildmembers`, `guildinvites`); no existing table or row is changed. Back the database
  up first as for any update ([Back up the database]({{ admin_page.url | relative_url }}#back-up-the-database)).
  The previous version still starts on a database that has `0013` (it runs no migration it does not
  know and ignores the new tables), so going back needs no restore.
- **Three replies on the login path change.** Every player's client calls them at each login:
  `POST /accountinfo/public` now describes the account that was asked about (and answers 404 for an
  unknown one), `POST /account/mapping` now maps ids in the shape the client reads, and
  `GET /guild/invite/player` has a new envelope (`{"code": "OK", "message": "", "payload": {"invites":
  []}, "invites": []}` instead of the old stub). The tests replay the client's parsing, but none of
  this has been seen in a real game yet.
- **Guilds can be created and joined.** A guild is created only when the leader typed and validated
  that exact name in the create window; staff and project words are reserved.
- **Invites are stricter:** a block removes the pending invites between two players, a declined party
  invite pauses that sender for 2 minutes and a declined guild invite pauses that guild for 24 hours,
  and a player sends at most 20 party invites in 10 minutes.

### What your players will see {#social-what-players-see}

**Every player must restart the game once after the update.** The client keeps the first account
info and the first mapping it got for each player for the whole session, so the old, wrong answers
stay until the game restarts. After that: party invites appear under PARTY INVITES, Add Friends
works (the other player sees the request at their next login), and the Guilds tab can create and
join guilds. Online status and EPIC FRIENDS still do not work by default. Chat came with a later
update ([Text chat](#chat)), and online status after that, off by default ([above](#harmonic-port)).
[Join as a friend]({{ friends_page.url | relative_url }}#friends-parties-and-guilds) explains it to
players.

### What you can do {#social-what-you-can-do}

- For the first session after the update, watch the metagame's log for the lines listed on
  [Friends, parties and guilds]({{ social_page.url | relative_url }}#how-to-verify). `LOG_BODIES=1`
  also records the request bodies (account ids and guild names); the server kit forces it off in
  public mode, so set it by hand for that session only and delete the body log afterwards.
- If logins misbehave after the update, each change has a switch in the metagame's settings (restart
  the metagame after changing it, and the players restart the game):

  | Symptom | Switch | What it puts back |
  |:--------|:-------|:------------------|
  | Login or the Social panel breaks, other players' names are wrong | `ACCOUNTINFO_PUBLIC_LEGACY=1` | Upstream's account info reply (party invites stop showing again) |
  | Login breaks right after the account lookups | `ACCOUNT_MAPPING=0` | No mappings (Add Friends does nothing again) |
  | Login or world loads break around the guild calls | `GUILDS=0` | The old guild stubs; stored guilds are kept and come back with the switch |
  | Invite to Party does nothing for a player on their own | `PARTY_SOLO_STUB=0` | Nothing to undo: a plain reply for a party of one |
  | Creating a guild always says "Unable to create guild." and the log says "no validate of this name" | `GUILD_CREATE_ACTIVITY_FALLBACK=1` | The weaker check (see [Configuration]({{ config_page.url | relative_url }}#metagame-social)) |

## Real progression is on by default {#real-progression-default}

**September 2026.** Applies to every server updated to a version where real progression is the
default, if the metagame's settings have no `PROGRESSION_MODE` line, an empty one, or a value other
than `real` or `stub`. You can tell the new version by its startup line, which begins
`Progression mode: real for every account`. Values such as `off`, `0` or `false` used to mean the
stub and now mean real. If you only set `PROGRESSION_REAL_ACCOUNTS` to try real progression on
chosen accounts, every other account now switches too: add `PROGRESSION_MODE=stub` to keep that
split.

### What changed

Before, the metagame answered every progression request with upstream Undaunted's stub unless
`PROGRESSION_MODE=real` was set:

- every account showed Slayer level 50 and maxed mastery, and nothing a player earned was stored;
- the Hunt Pass reported 99,999,999 with every reward already claimed, and the stub lists no
  entitlements, so the Elite track most likely showed as locked;
- the "Claim your Hunt Pass rewards" tutorial quest could never complete.

Now **an unset or empty `PROGRESSION_MODE` means real progression for every account**:

- Slayer level, weapon and behemoth mastery and their objectives start from the beginning
  (Slayer level 1, no mastery) and are saved from hunts and quests.
- The Hunt Pass (season 9b) starts empty. Claims on the free and the Elite track are saved. Every
  account owns the Elite pass by default (the `ENTITLEMENTS_DEFAULT` list).
- Rank rewards are granted once, by the game server, through the inventory. The metagame only
  records the claim.
- Entitlements, loadout slots, cooldowns and bounties are stored per account in the same mode.

`PROGRESSION_MODE=stub` still gives upstream's behaviour, and `PROGRESSION_REAL_ACCOUNTS` still
lists accounts that get real progression in stub mode (outside stub mode it is ignored). Any other
value is logged as a warning and treated as real; before this change it meant the stub.

**What has been tested.** A tester played in real mode on a throwaway account, then restarted
everything and logged in again ([roadmap]({{ roadmap_page.url | relative_url }}) items 2.8 to 2.12,
2.14 and 2.15): levels from 1, Slayer and weapon (axe) mastery rank-ups confirmed with each reward
granted once, no endless mastery pop-up, Hunt Pass claims on the free and the Elite track, an
entitlement granted by the game server, and all of it still there after the restart. **Not tried
yet:** behemoth mastery (it uses the same storage as weapon mastery), the extra loadout slots in the
UI, drafting and claiming bounties in the UI, cooldowns across a daily reset, several players at
once, and moving an account that already played under the stub's fake level 50 down to level 1
(roadmap item 2.13). With several players, note that a player's own client now gets 403 when it
asks for another account's progression, where the stub answered with the asking player's own data.
Escalation stays upstream's stub in both modes unless you set `ESCALATION_MODE=real`
([above](#harmonic-port)).

### What your players will see

**Nothing is migrated automatically.** A player who played on your server before the update has no
stored progression, so they start at **Slayer level 1 with an empty Hunt Pass**, like a new player.

- Their items should stay, including anything the fake level 50 handed out (alternate weapons,
  weapon unlock tokens). The update does not touch the inventory, but this has not been tried in game
  yet.
- Level 1 locks again what the fake 50 unlocked: consumable slots (ranks 2, 4 and 7), some crafting,
  and the hunts gated on Slayer level. Extra loadout slots never worked on the stub (it reports one
  slot); with real progression they come with ranks 34, 38 and 45 (not tried in the UI yet).
- Daily and weekly limits (cooldowns) now carry over between hunts and restarts. The stub forgot them
  each time a server loaded the player. If a limit never clears, `PROGRESSION_MODE=stub` is the
  fallback while you report it.

At startup the metagame logs its mode and, while such players exist, a warning with their count:

```
Progression mode: real for every account (the default)
3 player account(s) have no stored progression yet: they start at Slayer level 1 with an empty Hunt Pass, not upstream's fake max ranks. Nothing was migrated. ...
```

The count is of accounts that own a character but have no stored progression. An admin account that
never played is not counted, and a player drops out of the count once they earn anything or you
seed them. On a new server the count also includes players who have just created a character and
not earned any XP yet. For them the warning is harmless, and it goes away once they earn their first
XP.

### Choose what happens, before your players log in

| You want | Do this |
|:---------|:--------|
| Everyone starts fresh (what we chose for our own server) | Nothing. Tell your players that levels and the Hunt Pass start over and are kept from now on. |
| One player keeps max ranks | Seed them with `grandfather` while they are offline (below). Every track is set to its maximum rank and confirmed there, so nothing is granted and nothing pops up. Their Hunt Pass reads fully claimed. |
| Keep the old stub for now | Add `PROGRESSION_MODE=stub` to the metagame's settings and restart the metagame. `PROGRESSION_REAL_ACCOUNTS=<id>,<id>` gives chosen accounts real progression meanwhile. |

A `fresh` seed sets every track to 0 and clears the stored objectives. A player with no stored
progression is already at 0, so you only need `fresh` to reset an account that has real
progression already, such as a test account. The seed changes only that account's progression rows
and records the change in the `progression_events` table. The `grandfather` seed is covered by unit
tests but has not been tried in game yet.

**Take a backup first.** The metagame runs its database migrations at startup. The server kit backs
up around every update; on a hand-built host, see
[Back up the database]({{ admin_page.url | relative_url }}#back-up-the-database).

### Where the setting lives

- **Hand-built host** ([Host a server]({{ host_page.url | relative_url }})): `.env` in the
  `legacy/UndauntedMetagame` folder. Restart the metagame after a change.
- **Windows server kit** ([Windows server kit]({{ winserver_page.url | relative_url }})):
  `C:\DauntlessRevived\data\config\metagame.env`. The installer and `Update-DauntlessServer.ps1` keep
  every setting already in that file (the installer rewrites only the keys that describe the host,
  and neither keeps comment lines), so a server that already had `PROGRESSION_MODE=real` or `stub`
  keeps it. To stay on the stub, add the line **before** you update, or add it afterwards and run
  `C:\DauntlessRevived\bin\Stack.ps1 restart -Only metagame`. Unless the file says `real` or `stub`,
  the updater repeats the metagame's warning about players without stored progression after a
  successful update.
- **Restores and rollbacks follow the same rule.** A restore from a backup taken before this change,
  or a rollback to a build from before it (the updater's automatic rollback, or
  `Update-DauntlessServer.ps1 -Rollback`), uses the settings as they are. With no line, an older
  build means the stub again (players see the fake level 50; their stored progression stays in the
  database), and a newer build means real. Set `PROGRESSION_MODE=real` or `stub` explicitly if you
  want the mode fixed across restores and rollbacks.

### Seeding a player

Admin routes are refused through the public gateway (or any other proxy), so run this on the server
itself, with an admin account key. Save it as `seed-progression.js` anywhere on that machine; it
needs Node 18 or newer and nothing else:

```js
// node seed-progression.js <admin key file> list
// node seed-progression.js <admin key file> <UserId> grandfather|fresh
// Optional: set METAGAME to the metagame's address if it does not listen on 127.0.0.1:61000.
const fs = require("fs");
const [keyFile, userId, mode] = process.argv.slice(2);
const base = (process.env.METAGAME || "http://127.0.0.1:61000") + "/undaunted/api";
const key = fs.readFileSync(keyFile, "utf8").trim();

async function call(method, path, body) {
  const r = await fetch(base + path, {
    method,
    headers: { "x-undaunted-user-api-key": key, ...(body ? { "content-type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(15000),
  });
  if (!r.ok) throw new Error(`${method} ${path} returned HTTP ${r.status}`);
  return r.json();
}

(async () => {
  if (userId === "list") {
    const { Users } = await call("GET", "/GetAllUsers");
    for (const u of Users) {
      const p = await call("GET", "/Progression?UserId=" + encodeURIComponent(u.UserId));
      const slayer = p.Tracks.find((t) => t.progression_id === "MasteryTrack_PlayerLevel");
      const pass = p.Tracks.find((t) => t.progression_id === "season09b");
      console.log(`${u.UserId}  ${u.Username}  real=${p.RealMode}  slayer=${slayer.earned_free_rank}  huntpass=${pass.progress}`);
    }
    return;
  }
  if (!userId || (mode !== "grandfather" && mode !== "fresh")) throw new Error("usage: see the first lines of this file");
  const r = await call("POST", "/SeedProgression", { UserId: userId, Mode: mode });
  console.log(`${r.UserId}: seeded ${r.Mode}, ${r.Tracks.length} tracks, real mode ${r.RealMode}`);
})().catch((e) => { console.error(e.message); process.exit(1); });
```

**Hand-built host.** The admin key file is `C:\dr\data\owner.key` if you followed
[Host a server]({{ host_page.url | relative_url }}). If you followed
[Run it for a group]({{ admin_page.url | relative_url }}), the metagame listens on your Tailscale
address instead of `127.0.0.1`, so set `METAGAME` first:

```powershell
$env:METAGAME = "http://100.x.y.z:61000"   # the metagame's BIND_HOST
node seed-progression.js C:\dr\data\owner.key list
```

**Kit server.** Run it in an elevated PowerShell (Run as administrator): `owner.key` is readable only
by Administrators. In private mode the kit's metagame listens on the server's Tailscale address;
these lines read the address and port from the kit's configuration and work in both modes:

```powershell
$cfg = Get-Content C:\DauntlessRevived\data\config\server.json -Raw | ConvertFrom-Json
$port = if ($cfg.Ports.metagame) { $cfg.Ports.metagame } else { 61000 }
$env:METAGAME = "http://$($cfg.BindAddress):$port"
node seed-progression.js C:\DauntlessRevived\data\keys\owner.key list
node seed-progression.js C:\DauntlessRevived\data\keys\owner.key UID-... grandfather
```

`list` prints every account with its Slayer level as the game will show it and its Hunt Pass XP. It
answers `real=false` for accounts that are still on the stub (with `PROGRESSION_MODE=stub`); a seed
is stored either way and used once the account is in real mode. The script never prints the key.
