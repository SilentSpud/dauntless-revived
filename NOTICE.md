# NOTICE

Dauntless Revived is a modified version of Undaunted. The whole repository is licensed under the GNU
Affero General Public License, version 3 only (`AGPL-3.0-only`); see [LICENSE.txt](LICENSE.txt). This
file records where the parts of the repository come from. Suomeksi: [below](#suomeksi).

## Modification notice

This repository, [github.com/mixutin/dauntless-revived](https://github.com/mixutin/dauntless-revived)
(branch `dauntless-revived`), modifies Undaunted from upstream commit `7f692aa`, to run a private
server for the Dauntless 1.4.4 client (`1.4.4_shipping`, changelist 239827). Modifications began on
21 September 2026. Every change is recorded in the commit history, and the planned work is in
[ROADMAP.md](ROADMAP.md). This is not an official release of Undaunted, nor of any other project
named below.

## Where the parts come from

### Undaunted

- By gwog (Gregory Morford, [SyST3MDeV](https://github.com/SyST3MDeV)),
  [EisigesEis](https://github.com/EisigesEis) and
  [its other contributors](https://github.com/SyST3MDeV/Undaunted/graphs/contributors).
- Upstream: [github.com/SyST3MDeV/Undaunted](https://github.com/SyST3MDeV/Undaunted), AGPL-3.0-only.
- The server-mode DLL, the deploy server, the metagame backend and the original launcher are
  Undaunted's work. Our changes are on top of it, commit by commit.

### Harmonic's Dauntless 1.4.4 fork

- By Harmonic. Repository:
  [github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), commit `895f7c7`,
  AGPL-3.0-only.
- Ported into Dauntless Revived in September 2026, rewritten on our code where needed:
  - the Escalation season registry, its save rules and version rules (real Escalation saves);
  - the free store: the catalogue, the two-step purchase token, the store tabs and tile-art list, the
    grant kind of each item, and the sheen and hair-tint entitlements;
  - Slayer Links (the fork's contract, corrected against the 1.4.4 executable);
  - the deploy server's restart of a dead Ramsgate or Dojo on demand, the stored restart record, and
    the handling of a failed game-server start;
  - the idea of friends' online status (rebuilt inside our own chat server, with none of the fork's code);
  - smaller ideas: protection against retried progression grants, Hunt Pass seasons from a folder on
    disk and `ACTIVE_HUNT_PASS`, real currencies in `/balance`, the caller's own account in
    `oauth/verify`, tolerance for a repeated party accept, status and duration in the body log, a log
    line for unhandled progression requests, the MinHook include fix and five `.gitignore` patterns;
  - the fork's test cases: 103 of its 119 ported and 7 turned around to assert our behaviour, each
    marked with the original file and line.
- Files that carry Harmonic's copyright notice (`Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C)
  2026 Harmonic, AGPL-3.0-only`, with how each was modified for Dauntless Revived):
  `UndauntedMetagame/src/controllers/escalationConfig.ts`, `escalation.ts`, `freestore.ts` and
  `slayerlinks.ts`.
- Data files from the fork, each with a provenance note in its `_comment`:
  `UndauntedMetagame/src/vendor/escalation/seasons.json`, `src/vendor/store_catalog.json`,
  `src/vendor/store_item_kinds.json` and `test/data/store_art_skus.json`. They hold identifiers and
  tuning values read from the installed 1.4.4 client for interoperability, and no game assets.
- Code of ours built on Harmonic's ideas names the fork in its comments.
- What was not taken, and why, is recorded on
  [The Harmonic port](https://mixutin.github.io/dauntless-revived/findings/harmonic-fork.html).

### No Mystic Paradox material

Harmonic's fork also contains realtime XMPP code derived from
[Mystic Paradox](https://github.com/pranav158/Mystic-Paradox), which carries additional terms under
AGPLv3 section 7. **Dauntless Revived contains none of that code**: our chat and presence code
(`UndauntedMetagame/src/realtime/`) was written for this project, and nothing from Harmonic's
`src/realtime/` was copied. Those additional terms therefore do not apply to this repository, and it
has no `ADDITIONAL_TERMS.md`. Mystic Paradox is named in our README only as a related project; that
implies no endorsement or affiliation.

### Other material

- [MinHook](https://github.com/TsudaKageyu/minhook) by Tsuda Kageyu (BSD 2-Clause), in
  `native/internal-server/MinHook/`, with its own license.
- The Unreal Engine SDK headers in `native/internal-server/SDK/`, generated with
  [Dumper-7](https://github.com/Encryqed/Dumper-7) from the 1.4.4 client.
- Data read from the client for interoperability, carried from upstream: `progression_config.json`
  and the deploy server's hunt tables (`UndauntedDeployServer/src/vendor/*_table.json`), from which
  our `hunt_titles.json` is generated.
- The launcher ships the license texts of the software it includes in
  `UndauntedLauncher/THIRD-PARTY-NOTICES.txt`.

No game files are distributed in this repository. "Dauntless" is a trademark of its owners;
Dauntless Revived is not affiliated with or endorsed by Phoenix Labs or Epic Games. The full credits
are on [Credits and license](https://mixutin.github.io/dauntless-revived/legal.html).

## Suomeksi

Dauntless Revived on Undauntedin muokattu versio, ja koko repositorio on lisensoitu GNU Affero General
Public License -lisenssin versiolla 3 (`AGPL-3.0-only`). Muutokset alkoivat 21.9.2026 upstream-muutoksesta
`7f692aa`, ja jokainen muutos näkyy muutoshistoriassa. Tämä ei ole Undauntedin eikä minkään muun tässä
mainitun projektin virallinen julkaisu.

- **Undaunted** (gwog eli Gregory Morford, EisigesEis ja muut tekijät,
  [github.com/SyST3MDeV/Undaunted](https://github.com/SyST3MDeV/Undaunted)): palvelintilan DLL,
  deploy-palvelin, metagame ja alkuperäinen käynnistin ovat heidän työtään.
- **Harmonicin Dauntless 1.4.4 -haara**
  ([github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), muutos `895f7c7`):
  siitä siirrettiin syyskuussa 2026 Escalationin kausiluettelo ja tallennussäännöt, ilmainen kauppa,
  Slayer Links (korjattuna 1.4.4-ohjelmatiedostoa vasten), deploy-palvelimen korjaukset, kavereiden
  paikalla olon idea (rakennettu omaan chat-palvelimeemme ilman hänen koodiaan), pienempiä ideoita ja
  hänen testitapauksensa. Tiedostot `escalationConfig.ts`, `escalation.ts`, `freestore.ts` ja
  `slayerlinks.ts` kantavat hänen tekijänoikeusmerkintäänsä, ja hänen datatiedostoissaan on
  alkuperämerkintä. Datatiedostoissa on peliohjelmasta yhteentoimivuuden vuoksi luettuja tunnisteita ja
  viritysarvoja eikä lainkaan pelin sisältöä.
- **Mystic Paradoxin aineistoa ei ole.** Harmonicin haarassa on Mystic Paradoxista johdettua
  XMPP-koodia, jota koskevat AGPLv3:n kohdan 7 lisäehdot. Emme kopioineet siitä mitään: oma chat- ja
  läsnäolokoodimme on kirjoitettu tätä projektia varten. Siksi lisäehdot eivät koske tätä
  repositoriota, eikä siinä ole tiedostoa `ADDITIONAL_TERMS.md`.
- Muu aineisto: MinHook (BSD 2-Clause), Dumper-7:llä tuotetut SDK-otsaketiedostot, alkuperäisestä
  projektista peritty peliohjelmasta luettu data ja käynnistimen `THIRD-PARTY-NOTICES.txt`.

Repositoriossa ei ole pelitiedostoja. Dauntless Revived ei ole sidoksissa Phoenix Labsiin tai Epic
Gamesiin. Kaikki kiitokset ovat sivulla
[Kiitokset ja lisenssi](https://mixutin.github.io/dauntless-revived/fi/legal.html).
