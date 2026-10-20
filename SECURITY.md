# Security policy: Dauntless Revived

🇫🇮 [Suomeksi alempana](#suomeksi)

## Supported versions

| Version | Supported |
|---|---|
| The latest commit on the `dauntless-revived` branch | Yes |
| Older commits, and friend kits built from them | No. Update to the latest commit first and check the problem is still there. |
| Other branches, including `main`, which holds upstream Undaunted's code | No. Report problems in upstream code to [Undaunted](https://github.com/SyST3MDeV/Undaunted). |

## How to report

Report privately through GitHub's private vulnerability reporting:
**[Report a vulnerability](https://github.com/mixutin/dauntless-revived/security/advisories/new)**
(the repository's **Security** tab, then **Report a vulnerability**). Only you and the maintainer
can see the report.

Please do not open a public issue, discussion, pull request, or Discord message about a vulnerability until a fix
is released.

Include what you tested (commit, and whether you used the friend kit), what the problem is and
what an attacker could do with it, how to reproduce it, and how you would like to be credited.
Redact keys, tokens and other people's data.

This is a hobby project run by one person. Responses are best effort: I try to acknowledge
reports within a week. Coordinated disclosure, safe harbor and the general rules follow the
[default security policy](https://github.com/mixutin/.github/blob/HEAD/SECURITY.md) for all of
mixutin's repositories. In short: test only on your own installation, and give me a reasonable
time to fix the problem (90 days by default) before you publish.

## In scope

- **The metagame** (`legacy/UndauntedMetagame/`), as modified in this repository.
- **The deploy server** (`legacy/UndauntedDeployServer/`), as modified in this repository.
- **The friend kit** (`friend-kit/` and `tools/make-friend-kit.ps1`): for example, a way to make
  its hash checks pass for a tampered DLL, or anything it writes to a player's PC that it
  should not.
- **The documentation site** (`docs/`, published at
  [mixutin.github.io/dauntless-revived](https://mixutin.github.io/dauntless-revived/)): for
  example, setup instructions that would leave a host exposed if followed as written.
- **Any change this fork made** on top of upstream Undaunted. The commit history shows what
  those are.

## Design facts to know before you report

These are deliberate. Please read them first, so that your report can focus on what matters.

- **Both services bind to `127.0.0.1` by default** (`BIND_HOST`). Out of the box, neither service
  is reachable from other machines.
- **The deploy server has no authentication, by design.** Anyone who can reach it can start game
  processes on the host. It must never be exposed beyond the host PC, and the docs say so.
  Exposing it on purpose is a misconfiguration, not a vulnerability. A way to reach it from
  another machine in a default setup would be a vulnerability.
- **Friends connect in one of two ways.** The game's traffic to the metagame is plain HTTP, because
  that is what the 1.4.4 client and the DLL speak.
  - **Private mode:** over Tailscale, a private network the host shares by invitation. Friends
    reach the metagame and the game servers through the host's Tailscale address, and encryption
    comes from the Tailscale tunnel.
  - **Public mode** (the default of the Windows server kit in `deploy/windows-server/`): a TLS
    gateway (`legacy/UndauntedGateway/`) is the game's only public TCP port. The client talks plain HTTP only to
    the launcher on the player's own PC, which forwards over TLS pinned to the certificate
    fingerprint in the invite. The gateway refuses admin routes and the game-server key from
    outside, and the game's UDP ports are opened only for the addresses of players who logged in.
  - The deploy server stays on loopback in both modes.
- **Players log in with a personal account key**, and the server stores only a hash of it. A lost
  key cannot be recovered.
- Game servers listen on UDP ports 8770 to 8777.
- The friend kit installs two **prebuilt DLLs from upstream Undaunted** and checks them against
  pinned SHA-256 hashes before and after copying.

## Out of scope

- **The Dauntless game client** itself, and Phoenix Labs' or Epic Games' original online
  services. We do not have, and do not change, the game's code.
- **Upstream Undaunted's prebuilt DLLs** (`dxgi.dll`, `UndauntedInternalServer.dll`) and any
  upstream code this fork has not changed. Report those to
  [Undaunted](https://github.com/SyST3MDeV/Undaunted). How our friend kit checks and installs
  the DLLs is in scope.
- **Third-party services**: GitHub, GitHub Pages, Tailscale, shields.io and the like.
- Setups that go against the documentation, such as `BIND_HOST=0.0.0.0` or an exposed deploy
  server.
- Cheating inside your own save, such as editing your own inventory on your own server. Bugs like
  item duplication are still worth a normal issue.
- Testing against servers or PCs run by other people, including the maintainer's and friends'.

---

## Suomeksi

Tämä ohje kertoo, miten Dauntless Revived -projektin tietoturva-aukoista (virheistä, joita joku
voisi käyttää väärin) ilmoitetaan.

### Mitä versiota tuetaan

Vain `dauntless-revived`-haaran uusinta versiota. Jos käytät vanhempaa versiota, päivitä ensin ja
katso, onko ongelma yhä olemassa.

### Näin ilmoitat

- Ilmoita yksityisesti
  [tämän linkin kautta](https://github.com/mixutin/dauntless-revived/security/advisories/new).
  Silloin vain sinä ja ylläpitäjä näette ilmoituksen.
- Älä kirjoita aukosta julkiseen keskusteluun tai Discordiin ennen kuin korjaus on valmis.
- Kerro, mitä versiota kokeilit, mikä ongelma on ja miten sen saa toistettua.
- Älä lähetä oikeita avaimia, salasanoja tai muiden ihmisten tietoja.

Tämä on harrastusprojekti, jota tekee yksi ihminen vapaa-ajallaan. Yritän vastata viikon sisällä.
Anna aikaa korjata ongelma ennen kuin kerrot siitä julkisesti, tavallisesti 90 päivää. Kokeile vain
omaa asennustasi, älä muiden ihmisten palvelimia tai koneita.

### Mitä ohje koskee

- Metagame-taustapalvelinta ja deploy serveriä siinä muodossa kuin ne ovat tässä projektissa.
- Kavereiden asennuspakettia (`friend-kit/`).
- Ohjesivustoa (`docs/`).
- Kaikkia muutoksia, jotka tämä projekti on tehnyt alkuperäiseen Undauntediin.

### Hyvä tietää ennen ilmoittamista

- Palvelimet kuuntelevat oletuksena vain omaa konetta (`127.0.0.1`).
- Deploy serverissä ei ole tarkoituksella mitään tunnistusta. Sitä ei saa koskaan avata muiden
  koneiden käyttöön.
- Kaverit yhdistävät kahdella tavalla. Peli puhuu metagamelle salaamatonta HTTP:tä, koska 1.4.4-peli
  ja DLL-tiedosto osaavat vain sitä.
  - **Yksityinen tila:** Tailscalen kautta. Tailscale on ohjelma, joka tekee kavereiden koneista
    yksityisen, salatun verkon.
  - **Julkinen tila** (Windows-palvelinpaketin oletus, `deploy/windows-server/`): salattu
    yhdyskäytävä (`legacy/UndauntedGateway/`) on pelin ainoa julkinen TCP-portti. Peli puhuu salaamatonta HTTP:tä
    vain pelaajan omalla koneella olevalle käynnistimelle, joka välittää liikenteen salattuna kutsun
    varmenteeseen kiinnitettyä yhteyttä pitkin. Yhdyskäytävä torjuu ylläpitoreitit ja pelipalvelimen
    avaimen ulkopuolelta, ja pelin UDP-portit avataan vain kirjautuneiden pelaajien osoitteille.
  - Deploy server pysyy molemmissa tiloissa vain omalla koneella.

### Mitä ohje ei koske

- Itse Dauntless-peliä ja sen alkuperäisiä verkkopalveluja.
- Undauntedin valmiita DLL-tiedostoja ja muuta alkuperäistä Undaunted-koodia. Ilmoita niistä
  [Undaunted-projektiin](https://github.com/SyST3MDeV/Undaunted).
- Muita palveluja, kuten GitHubia tai Tailscalea.
- Asennuksia, jotka on tehty ohjeiden vastaisesti.
