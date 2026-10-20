# Contributing to Dauntless Revived

Thanks for wanting to help keep Dauntless playable. This is a small hobby project, so the rules
are short.

🇫🇮 [Suomeksi alempana](#suomeksi)

Community chat: **[Dauntless Revived Discord](https://discord.gg/ZJRprHzsgu)**. For code changes, still start with a GitHub Discussion so decisions stay searchable.

1. **Start with a Discussion.** Before you write code, open a
   [Discussion](https://github.com/mixutin/dauntless-revived/discussions) that says what you want to
   change and why. Check [ROADMAP.md](ROADMAP.md) first: much of the work is already planned, and
   each step has an id (such as `1.9` or `2.7`). Mention the id if there is one.
2. **Test on a throwaway account.** Never try a new server response on a real player's account
   first. A wrong response shape can crash the 1.4.4 client ("Trying to resize TArray to an invalid
   size") or damage a save. Back up the database before anything that migrates it.
3. **Never commit secrets or game files.** That means no account or server keys, no `.env` files,
   no databases, backups or logs, and nothing from the game: executables, paks, assets or its
   config. If you commit something like that by accident, say so in the pull request, and rotate
   any key it exposed.
4. **Keep pull requests small and focused.** Say what you changed, how you tested it, and which
   roadmap step it belongs to. If you change `ROADMAP.md`, run `node tools/sync-roadmap.js` so the
   docs site copy matches. Update the docs when behaviour changes. A new or changed setting, route,
   port or script belongs in the [Reference](https://mixutin.github.io/dauntless-revived/reference/)
   pages (`docs/reference/`, English and Finnish) and in the component's `.env.example`.
5. **License.** This project is AGPL-3.0-only, like upstream Undaunted. By contributing, you agree
   that your contribution is licensed under the same terms. Keep existing copyright and license
   notices.
6. **Security problems** go through
   [private vulnerability reporting](https://github.com/mixutin/dauntless-revived/security/advisories/new),
   not issues or pull requests. See [SECURITY.md](SECURITY.md).
7. **Be kind.** See the [Code of Conduct](CODE_OF_CONDUCT.md).

The launcher's Credits page reads every name from one file, `legacy/UndauntedLauncher/src/shared/credits.ts`,
so crediting a new contributor there is a one-line change (with the note in English and Finnish).

If a problem is in upstream Undaunted's code and not in a change this fork made, consider
contributing the fix to [Undaunted](https://github.com/SyST3MDeV/Undaunted) as well.

## Checks

GitHub Actions runs these on every push and pull request ([`ci.yml`](.github/workflows/ci.yml)). Run
the ones your change touches before you push. They need Windows and Node.js 24, except the docs and
hygiene checks, which need only Node.js.

| Check | Run it yourself |
| --- | --- |
| Server packages (`legacy/UndauntedMetagame`, `legacy/UndauntedGateway`, `legacy/UndauntedContent`, `legacy/UndauntedDeployServer`): build and unit tests | In the package folder: `npm ci`, `npm run build`, `npm test` |
| Launcher: typecheck, unit tests, installer | In `legacy/UndauntedLauncher`: `npm ci`, `npm run typecheck`, `npm test`, `npm run make` |
| Windows Server kit, in Windows PowerShell 5.1: every script parses, PSScriptAnalyzer finds no errors, and the kit's tests pass | `npm ci` in `legacy/UndauntedGateway` first. Then, in PowerShell from the repository root, `powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-KitUnit.ps1`, and the same for `Test-DeployRemote.ps1`. Last, the same for `Test-Sandbox.ps1` with `-SandboxDir "$env:TEMP\dr-sandbox"` added (a few minutes; it uses loopback ports 62000-62499 only and deletes that folder at the end) |
| Docs: generated files are current, and the site builds the way GitHub Pages builds it | `node tools/sync-roadmap.js` and `node tools/build-llms.js` must leave `git diff` empty |
| No secrets or game files are committed, not even in a commit that a later one undoes, and the launcher version is valid | `node tools/ci/check-repo.js --history origin/dauntless-revived..HEAD` (without `--history`: the files only) |

CodeQL (the repository's code scanning default setup) also scans the code for security problems, and
Dependabot proposes dependency updates.

Every push shows its launcher installer under the run's **Artifacts** for a week (not for pull
requests from forks). Launcher releases are published from `dauntless-revived` only, as release
`launcher-v<version>` for the version in `legacy/UndauntedLauncher/package.json`, and installed launchers
update to it. To release a new launcher, raise that version. Then:

- **Automatically (the default):** a push that passes every check, is still the head of the branch
  and has a version with no `launcher-v<version>` release yet publishes that exact installer. To pause
  this, set the repository variable `LAUNCHER_AUTO_RELEASE` to `false`.
- **By hand:** Actions > **Launcher release** > **Run workflow** on `dauntless-revived` builds and
  publishes the version. For a version that is published already, it only brings the self-update feed
  up to it.

A version is published only if it is newer than every earlier one, and never replaced. A prerelease
version (such as `0.2.0-beta.1`) becomes a GitHub prerelease that installed launchers do not update
to. Keep GitHub's immutable releases setting off: the `launcher-updates` release that installed
launchers read is updated in place.

---

## Suomeksi

Kiitos, että haluat auttaa pitämään Dauntlessin pelattavana. Säännöt ovat lyhyet:

Yhteisön chat: **[Dauntless Revived Discord](https://discord.gg/ZJRprHzsgu)**. Koodimuutokset aloitetaan silti GitHub-keskustelusta, jotta päätökset löytyvät myöhemmin.

1. **Aloita keskustelusta.** Ennen kuin kirjoitat koodia, avaa keskustelu
   [keskustelupalstalla](https://github.com/mixutin/dauntless-revived/discussions) ja kerro, mitä
   haluat muuttaa. Katso ensin tehtävälista [ROADMAP.md](ROADMAP.md): moni asia on jo suunniteltu.
2. **Kokeile erillisellä testitilillä.** Älä kokeile uutta palvelimen vastausta kenenkään oikealla
   tilillä. Väärin muotoiltu vastaus voi kaataa pelin tai rikkoa tallennuksen. Ota varmuuskopio
   tietokannasta ennen muutoksia.
3. **Älä koskaan lisää projektiin salaisuuksia tai pelin tiedostoja.** Ei avaimia, ei
   `.env`-tiedostoja, ei tietokantoja, varmuuskopioita tai lokeja, eikä mitään pelistä.
4. **Tee pieniä muutoksia kerrallaan.** Kerro, mitä muutit ja miten kokeilit sitä. Päivitä ohjeet,
   kun toiminta muuttuu. Uusi tai muuttunut asetus, reitti, portti tai skripti kuuluu
   [teknisen viitteen](https://mixutin.github.io/dauntless-revived/fi/reference/) sivuille
   (`docs/reference/`, englanniksi ja suomeksi) ja osan `.env.example`-tiedostoon.
5. **Lisenssi.** Projekti käyttää AGPL-3.0-only-lisenssiä. Kun lähetät muutoksen, se julkaistaan
   samalla lisenssillä.
6. **Tietoturva-aukoista** ilmoitetaan yksityisesti, ks. [SECURITY.md](SECURITY.md).
7. **Ole ystävällinen.** Ks. [käytösohjeet](CODE_OF_CONDUCT.md).

Käynnistimen Tekijät-sivu lukee kaikki nimet yhdestä tiedostosta,
`legacy/UndauntedLauncher/src/shared/credits.ts`, joten uuden osallistujan lisääminen sinne on yhden rivin
muutos (kuvaus englanniksi ja suomeksi).

### Tarkistukset

GitHub Actions ajaa jokaiselle pushille ja pull requestille samat tarkistukset
([`ci.yml`](.github/workflows/ci.yml)): palvelinohjelmien ja käynnistimen käännöksen ja testit,
Windows Server -paketin testit Windows PowerShell 5.1:llä, ohjesivuston käännöksen sekä tarkistuksen,
ettei mukana ole salaisuuksia tai pelin tiedostoja, ei edes commitissa, jonka myöhempi commit kumoaa.
Aja ennen pushia ne, joihin muutoksesi vaikuttaa. Komennot niiden ajamiseen omalla koneella ovat yllä
englanninkielisen kohdan "Checks" taulukossa. CodeQL (repositorion koodiskannauksen oletusasetus)
etsii lisäksi tietoturvaongelmia, ja Dependabot ehdottaa riippuvuuksien päivityksiä.

Jokaisen pushin käynnistimen asennusohjelma on ladattavissa ajon kohdasta **Artifacts** viikon ajan
(ei forkeista tulevissa pull requesteissa). Käynnistin julkaistaan vain `dauntless-revived`-haarasta
julkaisuna `launcher-v<versio>` sillä versiolla, joka on `legacy/UndauntedLauncher/package.json`-tiedostossa,
ja asennetut käynnistimet päivittyvät siihen. Uuden käynnistimen julkaisemiseksi nosta versiota.
Sitten:

- **Automaattisesti (oletus):** push, joka läpäisee kaikki tarkistukset, on yhä haaran uusin commit ja
  jonka versiolla ei ole vielä `launcher-v<versio>`-julkaisua, julkaisee juuri sen asennusohjelman.
  Tauon saat asettamalla repositorion muuttujan `LAUNCHER_AUTO_RELEASE` arvoon `false`.
- **Käsin:** Actions > **Launcher release** > **Run workflow** `dauntless-revived`-haaralle kääntää ja
  julkaisee version. Jo julkaistulla versiolla se vain päivittää itsepäivityskanavan siihen.

Versio julkaistaan vain, jos se on uudempi kuin kaikki aiemmat, eikä julkaistua versiota koskaan
korvata. Esiversio (kuten `0.2.0-beta.1`) julkaistaan GitHubin esijulkaisuna, johon asennetut
käynnistimet eivät päivity. Pidä GitHubin muuttumattomat julkaisut (immutable releases) pois päältä:
`launcher-updates`-julkaisua, jota asennetut käynnistimet lukevat, päivitetään paikallaan.
