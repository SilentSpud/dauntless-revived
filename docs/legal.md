---
title: Credits and license
nav_order: 90
description: "Credits for Undaunted by gwog and contributors, what the AGPL-3.0 asks of anyone who hosts a modified server, and why no Dauntless game files are shared here."
lang: en
ref: legal
---

{% assign roadmap_page = site.pages | where: "path", "roadmap.md" | first %}

# Credits and license
{: .no_toc }

<details open markdown="block">
  <summary>On this page</summary>
  {: .text-delta }
- TOC
{:toc}
</details>

---

## Credits

Dauntless Revived exists because other people did the hard parts first and published their work.

| Project | By | What we owe it |
|---|---|---|
| [Undaunted](https://github.com/SyST3MDeV/Undaunted) | gwog (Gregory Morford, [SyST3MDeV](https://github.com/SyST3MDeV)), [EisigesEis](https://github.com/EisigesEis) and [its other contributors](https://github.com/SyST3MDeV/Undaunted/graphs/contributors) | Our fork is built on it, starting from upstream commit `7f692aa`. It supplies the server-mode DLL that turns a second copy of the 1.4.4 client into a game server, the deploy server that runs those processes, the metagame backend, and the launcher, all created by gwog. EisigesEis worked on the metagame: inventory and loadouts, progression and mastery, invite codes and the admin API. Multiplayer Ramsgate and hunts on 1.4.4 are Undaunted's achievement. |
| [Harmonic's Dauntless 1.4.4 fork](https://github.com/Harmonicrain/Undaunted) | Harmonic | Another fork of Undaunted for the 1.4.4 client (AGPL-3.0-only). From its commit `895f7c7` we ported the Escalation season registry and save rules, the free store (catalogue, purchase flow, store tabs), Slayer Links, the deploy server's restart of a dead Ramsgate, the idea of friends' online status and its test cases, among smaller ideas. What we took and what we did not, with the reasons: [The Harmonic port]({{ '/findings/harmonic-fork.html' | relative_url }}). |
| [Mystic Paradox](https://github.com/pranav158/Mystic-Paradox) | its authors | A related project porting the same approach to the Dauntless 1.12.0 client, for content released after 1.4.4. Our fork contains none of its code (Harmonic's fork carries some; we copied none of it), and we have not tested it. We list it so that people looking for later content can find it. |
| [ooz](https://github.com/powzix/ooz) | powzix | An open-source Oodle (Kraken) decompressor. The 2.1.1 client links Oodle statically and ships no Oodle DLL, so the usual pak tools could not read its config. We built ooz as a local library to extract the config for analysis. It is an analysis tool only and is not part of the fork. |
| [Dumper-7](https://github.com/Encryqed/Dumper-7) | Encryqed and contributors | An Unreal Engine SDK generator. Undaunted's server DLL is built against a Dumper-7 SDK generated from the 1.4.4 client. |
| [MinHook](https://github.com/TsudaKageyu/minhook) | Tsuda Kageyu | The function-hooking library the server DLL uses. BSD 2-Clause license. |
| [GitHub Octicons](https://github.com/primer/octicons) | GitHub | The GitHub mark on the launcher's GitHub button, used unmodified. MIT license. |

And **Phoenix Labs**, who made Dauntless. Nothing here would be worth preserving without their game.

### Dauntless Revived contributors

- **[mixutin](https://github.com/mixutin)** (maintainer): the server kit, the launcher, backend
  fixes, real progression and these docs.
- **[Vvoidddd](https://github.com/Vvoidddd)**: found the cause of the dark pre-hunt airship (1.4.4's
  automatic exposure; the setting was reverted in launcher 0.1.1 because it darkened Ramsgate), hid
  the console windows of temporary hunt servers, and added the repository's `.gitignore`
  ([#5](https://github.com/mixutin/dauntless-revived/pull/5)); wrote the first performance sampler,
  the base of the server's performance log ([#6](https://github.com/mixutin/dauntless-revived/pull/6));
  added the launcher's opt-in "Basic adaptive" auto exposure setting for the airship, off by default
  ([#7](https://github.com/mixutin/dauntless-revived/pull/7)); let the launcher use an existing
  Dauntless 1.4.4 folder by pasting its path ([#8](https://github.com/mixutin/dauntless-revived/pull/8));
  wrote the first in-game text chat server (XMPP) and tested it with a real 1.4.4 client
  ([#9](https://github.com/mixutin/dauntless-revived/pull/9)).
- **Harmonic** ([Harmonic's 1.4.4 fork](https://github.com/Harmonicrain/Undaunted)): the Escalation season
  registry and save rules, the free store, the first working Slayer Links, the deploy server's restart
  of a dead Ramsgate, the idea of friends' online status, and the test cases we ported from that fork
  (see the table above). Every commit that carries Harmonic's work names Harmonic as co-author.

Everyone who has contributed is on the
[contributors page](https://github.com/mixutin/dauntless-revived/graphs/contributors). The launcher
shows the same credits on its **Credits** page, and ships the license texts of the software it
includes (Electron, MinHook, the GitHub mark and a few small libraries) in
`THIRD-PARTY-NOTICES.txt`.

---

## The license: AGPL-3.0

Undaunted is licensed **AGPL-3.0-only**, and our fork is too. The full text is in `LICENSE.txt` in
[the repository]({{ site.github.repository_url }}). MinHook's source is included in the server DLL's
source folder (`native/internal-server/MinHook`), and it keeps its own BSD 2-Clause license.

Here is what the license asks in practice, in plain words. **This is our reading, not legal advice.**
If it matters to you, read the license itself.

### If you only run it for yourself

Running the code on your own machine, modified or not, triggers nothing.

### If other people play on a version you modified

The AGPL's network clause (section 13) applies. Everyone who uses your modified server over a network
must be **offered the complete source code of the exact version you run**, free of charge. A clear
link in your launcher, in the in-game status message, or on a page like this one is enough. The
source does not have to be public. A private repository that your players can access also satisfies
this clause.

### If you give people files

Handing someone the DLLs, a launcher or scripts from the project counts as distributing ("conveying")
object code under section 6. Each copy must come with:

- a copy of the license text, and
- the corresponding source, or clear directions to where it is.
  - For unmodified upstream files, a link to the exact upstream commit is enough, as long as that
    commit stays available.
  - For files you built or changed yourself, link to your own fork at the matching commit.

The MinHook license adds one requirement for binaries: when you redistribute a DLL that contains
MinHook, reproduce MinHook's copyright notice and disclaimer.

### If you change the code

- Your modified version must carry prominent notices saying that you changed it, with a relevant
  date (section 5a). We do this with a note in the README and the commit history.
- Keep the existing copyright and license notices.
- Your modified version stays **AGPL-3.0-only**. You cannot add restrictions of your own, such as
  forbidding your players from sharing the code (section 10).

### What the license does not cover

- A separate program is not covered just because it talks to the server over HTTP. Any code you copy
  out of Undaunted is covered.
- **The AGPL covers only the project's own code. It never covers Phoenix Labs' game files, and it
  gives nobody any right to them.**

### Where we stand ourselves

- This repository is the fork's complete source. The [roadmap]({{ roadmap_page.url | relative_url }})
  lists our changes against upstream, and `NOTICE.md` at its root records where each part comes from:
  Undaunted, the work ported from Harmonic's 1.4.4 fork (the four source files ported from it carry
  Harmonic's copyright notice), MinHook and the SDK headers.
- Harmonic's fork contains realtime chat code derived from Mystic Paradox, which carries additional
  terms under section 7 of the AGPL. We copied none of it: our chat and presence code was written for
  this project. Those terms therefore do not apply to this repository.
- One gap is known. Upstream ships a prebuilt `dxgi.dll` proxy whose source is not in its repository,
  and as far as we know it has not been published anywhere. From its disassembly, it loads the
  system `dxgi.dll`, forwards three exports (`CreateDXGIFactory`, `CreateDXGIFactory1`,
  `CreateDXGIFactory2`) and loads the server DLL.
  We plan to replace it with a proxy of our own, built from published source. We also plan to build
  `UndauntedInternalServer.dll` from source ourselves instead of using the prebuilt copy. Both items
  are on the roadmap.

---

## No game files

- This site and the repository contain **no files from Dauntless**: no executables, no paks or
  IoStore containers, no assets and no game configuration. They do not link to downloads of the game.
  - One inherited item to be aware of: upstream Undaunted's metagame includes
    `UndauntedMetagame/src/vendor/progression_config.json` (about 190 KB), which it serves to the
    client as progression and reward data. It has the shape of a recorded Phoenix backend response,
    not of a file from the game install. Our fork carries it unchanged from upstream.
  - Upstream also includes four of the game's data tables as JSON exports,
    `UndauntedDeployServer/src/vendor/*_table.json` (about 2.2 MB: player hunts, matchmaker hunts and
    the two Trials tables), which the deploy server reads to pick a hunt's behemoth and map. They are
    exports of table rows, not files from the install. Our fork carries them unchanged, and our
    `UndauntedMetagame/src/vendor/hunt_titles.json` is generated from them (hunt ids and their English
    titles).
  - From Harmonic's fork we carry four data files: `UndauntedMetagame/src/vendor/escalation/seasons.json`
    (the Escalation seasons), `store_catalog.json` and `store_item_kinds.json` (the free store), and
    `UndauntedMetagame/test/data/store_art_skus.json` (store SKU ids, for a test). They hold identifiers
    and tuning values read from the installed client for interoperability, and about 330 short English
    names; no textures, models, sounds or other assets. `seasons.json` names the game files it was read
    from by their SHA-256, as our game manifest does. Each file says where it comes from in its
    `_comment`.
- Each player needs **their own copy** of the Dauntless 1.4.4 client. The
  [Setup]({{ '/setup/' | relative_url }}) section explains how to check that a copy is genuine and
  unmodified.
- We do not modify the game's files on disk. Two DLLs are added next to the executable, and some
  settings are written to the per-user config folder (`%LOCALAPPDATA%\Archon\Saved\Config`), which
  the game writes to as well. Before installing our reference copy, we checked all 406 of its files
  against Phoenix Labs' own manifest, and nothing we do changes them.
- The findings pages quote endpoint URL templates, field names, function names, addresses and short
  snippets. We quote only as much as each explanation needs, and never large parts of the game's
  config or code.
- The game config that Phoenix Labs shipped inside the client contains credentials. Both builds
  include a live Slack webhook URL, meaning a real production URL rather than a placeholder. We have
  never tested whether it still works. The 2.1.1 config also has the Epic
  Online Services client secret and encryption key in plain text. **We have never used any of them
  and do not reproduce them anywhere.**

---

## Trademarks

"Dauntless" and the related names and logos are trademarks of their owners. Epic Games, Epic Online
Services and Unreal Engine are trademarks of Epic Games, Inc. All other trademarks belong to their
owners. We use these names only to say what software this project works with.

Dauntless Revived is **not affiliated with, endorsed by, or supported by Phoenix Labs or Epic
Games**. It is non-commercial: nothing is sold, and there is no real-money store.

---

## Why this project exists

Phoenix Labs shut down the official Dauntless servers on 30 May 2025. The client cannot be played
without its online backend, so without a replacement server the game cannot be played at all, even
by people who still have it installed.

We do this for **preservation and interoperability**. We studied how the client talks to its backend
so that an independently written server can work with the unmodified client. That lets people who
already own the game keep playing it with friends. We publish what we learned so that the knowledge
survives even if this project does not.

If you hold rights in any of this and have a concern, please open an issue in
[the repository]({{ site.github.repository_url }}).
