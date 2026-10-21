---
title: Kehittäjän opas
parent: Tekninen viite
grand_parent: Dauntless Revived suomeksi
nav_order: 7
description: "Dauntless Revivedin kehittäminen: esivaatimukset, pakettien käännös ja testit, palvelinten ja käynnistimen ajo omalla koneella, CI ja koodin sijainti."
lang: fi
ref: reference/development
locale: fi_FI
---

{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign ports_page = site.pages | where: "path", "fi/reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign game_settings_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "fi/reference/scripts.md" | first %}

# Kehittäjän opas
{: .no_toc }

Tämä sivu on koodin parissa työskenteleville: mitä asennetaan, miten kukin paketti käännetään ja
testataan, miten palvelinkokonaisuus ja käynnistin ajetaan omalla koneella, mitä CI tarkistaa ja missä
mikin osa koodista on. Palvelimen pystyttäminen kavereille neuvotaan sivuilla
[Pystytä palvelin]({{ host_page.url | relative_url }}) ja
[Windows-palvelin]({{ winserver_page.url | relative_url }}). Tämä sivu linkittää niihin eikä toista
niiden ohjeita. Jokainen asetus on sivulla [Asetukset]({{ config_page.url | relative_url }}) ja
jokainen portti sivulla [Portit ja verkko]({{ ports_page.url | relative_url }}).

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Esivaatimukset {#prerequisites}

| Mitä | Versio | Mihin tarvitaan |
|---|---|---|
| Windows | 10 tai 11, 64-bittinen | Käynnistin, palvelinpaketti, sen testit ja peli toimivat vain Windowsissa. |
| Node.js ja npm | Node 24 ja npm 11. Palvelinpaketti asentaa täsmälleen Noden 24.19.0. (Käynnistimen `package.json` hyväksyy myös Noden 20.19 tai uudemman; käytä silti versiota 24.) | Jokainen paketti. `better-sqlite3` (metagame ja deploy-palvelin) on natiivimoduuli, joka käännetään yhdelle Node-versiolle: aja `npm ci` uudelleen näissä kahdessa kansiossa aina, kun vaihdat Noden versiota ([Vianetsintä]({{ trouble_page.url | relative_url }}#npm-allow-scripts-warnings)). |
| Git | Ajantasainen Git for Windows, `core.longpaths` päällä | Kloonaus sekä `Test-DeployRemote.ps1`, joka pakkaa kloonin komennolla `git archive`. Ilman Gitiä metagamen käännös onnistuu silti, mutta sen commitiksi kirjataan `unknown`. |
| Windows PowerShell | 5.1, sisältyy Windowsiin | Palvelinpaketti ja sen testit. Paketti on kirjoitettu versiolle 5.1. |
| Visual Studio 2022 | C++-työpöytäkehityksen työkalut (toolset v143), x64 | Vain palvelin-DLL:n kääntämiseen lähdekoodista ([alla](#server-dll)). |
| Dauntless 1.4.4 | Tarkistettu asennus ([näin tarkistat sen]({{ host_page.url | relative_url }}#verify-the-build)) | Vain pelaamiseen oman palvelinkokonaisuuden kanssa, sisältöpalvelimen integraatiotestiin ja uuden sisältömanifestin tekemiseen. Yksikään yksikkötesti ei tarvitse pelin tiedostoja. |

---

## Hae koodi {#clone}

```powershell
New-Item -ItemType Directory -Force C:\dr | Out-Null
git -c core.longpaths=true clone https://github.com/mixutin/dauntless-revived.git C:\dr\undaunted
git -C C:\dr\undaunted config core.longpaths true
git -C C:\dr\undaunted checkout dauntless-revived
```

`dauntless-revived` on oletushaara: pull requestit tehdään siihen, ja dokumentaatiosivusto julkaistaan
sen `docs/`-kansiosta. Pidä klooni lyhyessä polussa: pelkästään palvelin-DLL:n generoidussa SDK:ssa on
yli 4 000 tiedostoa, ja kloonaus syvälle sisäkkäiseen kansioon on kaatunut virheeseen "Filename too
long" ([Vianetsintä]({{ trouble_page.url | relative_url }}#git-filename-too-long)).

### Mitä repositoriossa on {#layout}

`Undaunted*`-kansioiden nimet tulevat alkuperäisestä Undaunted-projektista (upstream), ja ne on pidetty
ennallaan. Kansioiden npm-pakettien nimet ovat `dauntless-revived-metagame`,
`dauntless-revived-deploy-server`, `dauntless-revived-content`, `dauntless-revived-gateway` ja
`dauntless-revived-launcher`.

| Kansio | Mikä se on | Kieli ja työkalut |
|---|---|---|
| `legacy/UndauntedMetagame/` | Taustapalvelu, jonka kanssa peli keskustelee: tilit ja avaimet, hahmot, inventaario, varustukset (loadoutit), eteneminen, ryhmät, kaverit, killat, matchmaking, ylläpitäjän rajapinta ja pelin tekstichat (XMPP-kuuntelija samassa prosessissa, päällä kun `CHAT=1`). | TypeScript, Express 5, SQLite `better-sqlite3`:n ja Drizzlen kautta |
| `legacy/UndauntedDeployServer/` | Käynnistää pelipalvelinprosessit ja valvoo niitä, kun metagame pyytää. Siinä ei ole tunnistautumista, ja se vastaa vain loopback-kutsujille. | TypeScript, Express 5 |
| `legacy/UndauntedContent/` | Tarjoaa käynnistimelle tarkistetut 1.4.4-pelitiedostot, ylläpitäjän kuvapaketin ja uutiset. | TypeScript, Noden oma `http` |
| `legacy/UndauntedGateway/` | Julkinen tila: TLS-yhdyskäytävä (palvelimen ainoa julkinen TCP-portti) ja sallittujen listan apuri (yksi Windowsin palomuurisääntö pelin UDP-porteille). | TypeScript, Noden oma `https`; varmennetyökalussa `node-forge` |
| `legacy/UndauntedLauncher/` | Dauntless Revived Launcher, jonka kaverit asentavat: kutsut, rekisteröinti, lataukset, paikallinen välitin ja pelin käynnistys. Kansiossa `assets/` ovat pelin tarvitsemat kaksi valmiiksi käännettyä DLL:ää. | TypeScript, Electron, Electron Forge ja Vite |
| `native/internal-server/` | Palvelin-DLL:n C++-lähdekoodi upstreamista, generoidun SDK:n kanssa. | C++, Visual Studio 2022 |
| `deploy/windows-server/` | Windows Server -palvelinpaketti: asennus, päivitys, varmuuskopiointi, kutsut, palvelinkokonaisuuden valvoja ja paketin testit kansiossa `tests/`. | Windows PowerShell 5.1, Node-apuohjelmat kansiossa `lib/` |
| `friend-kit/` | Käsin ajettavat skriptit kavereille, jotka eivät käytä käynnistintä (`setup.ps1`, `play.ps1`). | PowerShell |
| `tools/` | Dokumentaation generaattorit, sisältömanifestin generaattori ja kaveripaketin kokoaja ([Skriptit ja parametrit]({{ scripts_page.url | relative_url }})) sekä kansiossa `tools/ci/` CI:n käyttämät repositorion tarkistus ja käynnistimen versiosäännöt ([alla](#ci)). | Node, PowerShell |
| `docs/` | Tämä sivusto. Suomenkieliset sivut ovat kansiossa `docs/fi/`. | Jekyll ja just-the-docs-teema |
| `.github/` | Issue- ja pull request -pohjat, GitHub Actions -työnkulut (CI ja käynnistimen julkaisut) sekä Dependabotin asetukset. | YAML |

---

## Käännä ja testaa kukin paketti {#packages}

Jokainen Node-paketti asennetaan, käännetään ja testataan erikseen omasta kansiostaan. Käytä komentoa
`npm ci`, niin saat täsmälleen ne versiot, jotka paketin `package-lock.json` määrää:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
npm ci --no-audit --no-fund
npm run build
npm test
```

| Paketti | Käännös | Testit | Muut skriptit |
|---|---|---|---|
| `legacy/UndauntedMetagame` | `npm run build`: TypeScript kansioon `dist/`, minkä jälkeen `scripts/write-build-info.js` kirjoittaa tiedoston `dist/build-info.json` (commit, versio, käännösaika) | `npm test` | `npm run dev` (käynnistyy uudelleen, kun muutat lähdetiedostoa), `npm start`, `npm run db:generate` |
| `legacy/UndauntedDeployServer` | `npm run build` (kansioon `dist/`) | `npm test` | `npm run dev`, `npm start` |
| `legacy/UndauntedGateway` | `npm run build` (kansioon `dist/`) | `npm test` | `npm start` (yhdyskäytävä), `npm run start:allowlist` (apuri), `npm run make-cert` |
| `legacy/UndauntedContent` | `npm run build` (kansioon `dist/`) | `npm test`, `npm run test:integration` | `npm start`, `npm run verify` (laskee pelikansion tiivisteet ja vertaa niitä manifestiin) |
| `legacy/UndauntedLauncher` | `npm run make` (asennusohjelma ja zip kansioon `out/make/`), `npm run package` (pelkkä sovelluskansio) | `npm run typecheck`, `npm test` | `npm start`, `npm run icon` |

`dist/`, `build/` sekä käynnistimen `out/`, `.vite/` ja `.test-build/` ovat gitin ohittamia
(git-ignored). Käännä ne itse äläkä koskaan commitoi niitä. Kun olet hakenut uudet muutokset (pull),
aja `npm run build` uudelleen ennen komentoa `npm start`: `npm start` ajaa sitä, mitä `dist/`-kansiossa
on. `tsc` ei koskaan poista tiedostoja `dist/`-kansiosta, joten poista kansio ensin, jos jokin
lähdetiedosto on poistettu tai nimetty uudelleen.

Käännöstiedot kertovat, mikä lähdekoodi tarkalleen on käynnissä. Ne näkyvät reiteissä
`GET /undaunted/api/ServerStatus` ja `/dauntless-status`
([HTTP-rajapinta]({{ api_page.url | relative_url }})). Jos `legacy/UndauntedMetagame/`-kansiossa on
käännöshetkellä commitoimattomia muutoksia, commitin perään tulee `-dirty`. Metagamen `.env`-tiedoston
`GIT_COMMIT` korvaa kirjatun commitin. `npm run dev` ajaa lähdekoodia ilman käännöstä, joten
käännöstietoja ei silloin ole: tilareitit näyttävät commitiksi `unknown`, ellei `GIT_COMMIT` ole
asetettu.

---

## Aja testit {#tests}

### Pakettien testit {#package-tests}

Neljässä palvelinpaketissa `npm test` poistaa `build/`-kansion, kääntää lähdekoodin ja testit
`tsconfig.test.json`-asetuksilla kansioon `build/` ja ajaa ne Noden omalla testiajurilla. Jokainen
testitiedosto ajetaan omassa prosessissaan. Yhdyskäytävä ajaa tiedostonsa yksi kerrallaan. Käynnistimen
`npm test` kääntää testit kansioon `.test-build/` ja ajaa nekin yksi tiedosto kerrallaan.
`npm test -- relay` ajaa vain ne käynnistimen testitiedostot, joiden nimessä on `relay`.

| Paketti | Mitä testit kattavat | Hyvä tietää |
|---|---|---|
| Metagame | Tilit ja käyttäjänimet, mitä kukin avaintyyppi saa tehdä, hahmot ja tallennushistoria, tietokanta, inventaarion tapahtumat, varustukset ja varustuspaikat, eteneminen (oletus, oikea tila ja stub-tila, tasojen laskenta, päivitysilmoitus, uusintavahti, asetuskansio), Escalation, ilmainen kauppa, oikeudet (entitlements), jäähtymisajat (cooldowns) ja bountyt, ryhmät, kaverit, Slayer Links, killat, chat ja kavereiden läsnäolo, matchmaking ja sen virheet, runkoloki, palvelimen tila sekä tietokannan migraatiot (migraation 0013 tasolla oleva, riveillä täytetty tietokanta säilyttää jokaisen rivinsä uusien migraatioiden läpi). Harmonicin haarasta siirretyt tapaukset (`*ported.test.ts`, `escalation`, `freestore`, `matchmakingfailure`, `slayerlinks`) nimeävät kommentissa hänen tiedostonsa ja rivinsä. | Jokainen tiedosto saa oman tyhjän tietokannan kansioon `%TEMP%\undaunted-test-*`, kertakäyttöisen allekirjoitusavainparin sekä asetukset `AUTH_MODE=APIKEY` ja `NODE_ENV=production`. Lokit ovat hiljaa; aseta `TEST_LOG_LEVEL` (esimerkiksi `debug`), niin näet ne. |
| Deploy-palvelin | Pelipalvelintyypit ja lista, jonka metagame lukee, matchmaking-pyynnön tarkistukset, pääsy vain loopbackista, Ramsgaten ja Dojon uudelleenkäynnistys tarvittaessa yhdellä käynnistyksellä, puuttuva peliohjelma ja epäonnistunut käynnistys (korvaavalla käynnistysfunktiolla) | `TEST_LOG_LEVEL` toimii myös tässä. |
| Yhdyskäytävä | TLS ja kiinnitetty sormenjälki, reititys ja hylkäykset, välitysotsakkeet, koko- ja nopeusrajat, WebSocket-yhteyksien välitys, sallittujen listan syöte ja apuri, molemmat käynnistystiedostot oikeina prosesseina | Apuri ajetaan aina kuivaharjoitustilassa (dry run): testit eivät koskaan muuta palomuuria. |
| Sisältöpalvelin | Asetukset, polut, tavualueet (range), avainvälimuisti, rajat, manifesti, kuvapaketti ja uutiset, pyyntöjen käsittelijä | `npm run test:integration` tarvitsee oikean asennuksen ([alla](#content-integration-test)). |
| Käynnistin | Kutsut, välitin, lataukset (jatkaminen, tarkistus, korjaus) HTTP:n ja kiinnitetyn TLS:n yli, `Engine.ini`, käynnistysparametrit, avainvarasto sekä koko julkisen tilan kulku ohjaimen (controller) kautta | Luo kertakäyttöisiä itse allekirjoitettuja varmenteita väliaikaiskansioon. |

Kun haluat ajaa yhden metagamen testitiedoston, käännä testit kerran ja kutsu Nodea paketin kansiosta:

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
npx tsc -p tsconfig.test.json
node --test build/test/party.test.js
```

**Testiportit.** Testit kuuntelevat vain vapaissa loopback-porteissa 62000:sta ylöspäin, eivät koskaan
käynnissä olevan palvelinkokonaisuuden porteissa (61000–61099). Osa testisarjoista käyttää samoja
numeroita: käynnistimen testeillä on yhteinen portti 62012 sisältöpalvelimen testien kanssa, 62013
deploy-palvelimen testien kanssa sekä portit 62401–62404, 62409 ja 62420–62422 yhdyskäytävän testien
kanssa; yksi käynnistimen testi ja hiekkalaatikon yhdyskäytävä käyttävät molemmat porttia 62443; ja
sisältöpalvelimen integraatiotesti ja hiekkalaatikko käyttävät molemmat porttia 62002. **Aja samalla
koneella vain yksi testisarja kerrallaan.** Koko lista on sivulla
[Portit ja verkko]({{ ports_page.url | relative_url }}#test-ports).

### Sisältöpalvelimen integraatiotesti {#content-integration-test}

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedContent
$env:CONTENT_IT_GAME_DIR = "C:\D144\Dauntless"   # oletus; kansio, jossa Archon\ on
npm run test:integration
```

Testi kääntää palvelimen ja käynnistää `dist/server.js`:n omana prosessinaan osoitteeseen
`127.0.0.1:62002` sekä valemetagamen (mock) osoitteeseen `127.0.0.1:62003` (`CONTENT_IT_PORT` ja
`CONTENT_IT_MOCK_PORT` vaihtavat nämä portit). Asennus tarjotaan vain luku -tilassa. Oletuksena
pelikansio on palvelinohjeen [lyhyt asennuspolku]({{ host_page.url | relative_url }}#short-install-path);
osoita `CONTENT_IT_GAME_DIR` omaan asennukseesi. Jos kansiossa ei ole asennusta, testin kaikki
tapaukset ohitetaan. CI:ssä ei ole pelin tiedostoja, joten se ei koskaan aja tätä testiä: aja se itse,
kun muutat sisältöpalvelinta.

### Palvelinpaketin testit {#kit-tests}

Kansiossa `deploy\windows-server\tests\` on kolme skriptiä Windows PowerShell 5.1:lle. Mikään niistä ei
muuta palomuuria, palveluita, ajastettuja tehtäviä, käyttäjätilejä tai varmennevarastoja, eikä mikään
niistä käytä portteja 61000–61099.

```powershell
Set-Location C:\dr\undaunted
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-KitUnit.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-DeployRemote.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-Sandbox.ps1
```

| Skripti | Mitä se tarkistaa | Mitä se tarvitsee | Parametrit |
|---|---|---|---|
| `Test-KitUnit.ps1` | Jokainen paketin skripti jäsentyy PowerShell 5.1:ssä ja on pelkkää ASCII:ta; v1- ja v2-kutsumerkkijonot, myös kaikki, mikä pitää hylätä; varmenteiden sormenjäljet; TLS-kiinnitys paikallista testipalvelinta vasten; `Get-ServerStatus.ps1` tiliavaimen kanssa ja ilman; tiliavaintiedostot; paloina lähettämisen (chunked upload) apuri; suorituskykymittari. | Ensin `npm ci` kansiossa `legacy/UndauntedGateway`: testi ajaa yhdyskäytävän varmennetyökalua. | `-WorkDir` (oletus `%TEMP%\dr-kit-unit`, poistetaan lopuksi), `-Port` (oletus 62450; 62000–62499; suorituskykytesti käyttää myös sitä seuraavaa porttia) |
| `Test-DeployRemote.ps1` | `Deploy-Remote.ps1` ilman palvelinta: ensin argumenttien käsittely, sitten paketin, lähdekoodin, varmuuskopion ja pelin zip-tiedoston lähetys paikalliseen kansioon, joka esittää palvelinta. Lisäksi keskeytynyt lähetys, joka jatkuu; osa, joka vioittui siirrossa tai tarkistuksen jälkeen; toinen ajo, jossa ei ole enää mitään tehtävää; sekä työkopion lähetys, jossa ei ole `.env`-tiedostoa, avainta, tietokantaa, `node_modules`- eikä `dist`-kansiota. | Git. Ei verkkoa, ei SSH-avainta. | `-WorkDir` (oletus `%TEMP%\dr-deploy-test`, poistetaan lopuksi) |
| `Test-Sandbox.ps1` | Tämän kloonin oikea `-Sandbox`-asennus julkisessa tilassa väliaikaiskansioon, jossa on pelikansion korvike: ensin `-WhatIf`, sitten asennus (se kääntää neljä palvelinpakettia, mikä vie muutaman minuutin), kutsut, tila ja rekisteröinti yhdyskäytävän kautta kiinnitetyllä varmenteella, yhdyskäytävän hylkäykset, päivitys ja paluu edelliseen versioon (rollback), tila, varmuuskopio ja pysäytys sekä toinen asennus, joka palautetaan tuosta varmuuskopiosta. Lopuksi se pysäyttää kaiken ja poistaa kansion. Se tarkistaa myös, ettei porteissa 61000/61001 käynnissä olevaan palvelinkokonaisuuteen koskettu. Deploy-palvelinta ja peliä hiekkalaatikossa ei koskaan käynnistetä. | Vapaat loopback-portit 62000, 62002, 62005 ja 62443 (testi pysähtyy, jos jokin niistä on varattu); npm-rekisteri `npm ci`:tä varten | `-SandboxDir` (oletus `C:\dr\sandbox-ws2019`; kansion nimessä on oltava `sandbox`, koska kansio poistetaan), `-KeepSandbox` (säilyttää kansion ja pysäytetyn asennuksen), `-SkipRestore` (jättää toisen asennuksen väliin) |

Oletusarvoinen `-SandboxDir` noudattaa palvelinohjeen `C:\dr`-rakennetta. Mikä tahansa muu kansio
käy, esimerkiksi `-SandboxDir "$env:TEMP\dr-sandbox"`. `Test-Sandbox.ps1` jättää lokinsa tiedostoon
`%TEMP%\dr-sandbox-test.log` (valitsimella `-KeepSandbox` nimellä `test.log` hiekkalaatikkokansioon).
Hiekkalaatikko luo omat avaimensa ja salaisuutensa ja poistaa ne kansion mukana; paketti ei koskaan
tulosta niitä. Mitä paketti itse tekee oikealla palvelimella, kerrotaan sivulla
[Windows-palvelin]({{ winserver_page.url | relative_url }}).

---

## Aja palvelinkokonaisuus omalla koneella {#local-stack}

Palvelinpuolen työssä ajat kloonistasi samoja osia, joita palvelimellakin ajetaan.
[Pystytä palvelin]({{ host_page.url | relative_url }}) käy läpi metagamen ja deploy-palvelimen jokaisen
vaiheen: käännöksen, `.env`-tiedostot, avaimet, ensimmäisen käynnistyksen, pelipalvelinavaimen,
ylläpitäjän tilin ja peliohjelman käynnistämisen. Tämä osio kertoo, mikä on kehityskoneella toisin ja
miten muut osat liittyvät kokonaisuuteen.

**Käytä omaa tietokantaa.** Älä koskaan osoita kehityskäytössä olevaa metagamea tietokantaan, jota
pelaajasi käyttävät. Metagame ajaa käynnistyessään tietokantaan kaikki uudet migraatiot, eikä se ota
siitä ensin varmuuskopiota. (Palvelinpaketti varmuuskopioi ennen jokaista käynnistystä; käsin
käynnistetty metagame ei.)

### Mitä kukin osa tarvitsee {#components}

| Osa | Asetustiedosto | Käynnistys | Tarvitsee ensin | Tarvitaan vain |
|---|---|---|---|---|
| Metagame | `legacy\UndauntedMetagame\.env`. Tiedoston on oltava olemassa. | `npm run dev` tai `npm start` | ei mitään | kaikkeen |
| Deploy-palvelin | `legacy\UndauntedDeployServer\.env`. Tiedoston on oltava olemassa. | `npm run dev` tai `npm start` | metagamen, jossa on rekisteröity pelipalvelinavain; tarkistetun 1.4.4-asennuksen kahden DLL:n kanssa; käyttäjäkohtaisen `Game.ini`-tiedoston ([Pystytä palvelin, vaihe 8]({{ host_page.url | relative_url }}#game-ini)) | pelipalvelinten käynnistämiseen: Ramsgate, Training Dojo, metsästykset |
| Sisältöpalvelin | `legacy\UndauntedContent\.env` (luetaan, jos se on olemassa). `CONTENT_GAME_DIR`-asetuksella ei ole oletusarvoa. | `npm start` | metagamen (se tarkistaa tiliavaimet sieltä); tarkistetun 1.4.4-asennuksen | käynnistimen latauksiin |
| Sallittujen listan apuri | `legacy\UndauntedGateway\.env` (luetaan, jos se on olemassa). Asetuksilla `ALLOWLIST_DRY_RUN` ja `ALLOWLIST_SECRET` ei ole oletusarvoa. | `npm run start:allowlist` | ei mitään | julkiseen tilaan |
| Yhdyskäytävä | `legacy\UndauntedGateway\.env` (luetaan, jos se on olemassa). Varmenteella, sen avaimella ja molemmilla salaisuuksilla ei ole oletusarvoa. | `npm start` | metagamen, sisältöpalvelimen, sallittujen listan apurin ja varmenteen | julkiseen tilaan |

Metagamen ja deploy-palvelimen skriptit lataavat `.env`-tiedoston Noden valitsimella `--env-file`, ja
Node pysähtyy, jos tiedostoa ei ole. Yhdyskäytävän ja sisältöpalvelimen skriptit lataavat sen vain, jos
se on olemassa (`--env-file-if-exists`), joten niiden pakolliset asetukset voivat tulla myös
komentotulkista. Komentotulkissa jo asetetut muuttujat ohittavat tiedoston arvot. Asetukset luetaan
käynnistyksessä: käynnistä osa uudelleen, kun muutat sen tiedostoa. Yhdyskäytävä ja apuri lukevat saman
`.env`-tiedoston, kun käynnistät ne npm:n kautta; palvelimella paketti antaa kummallekin oman
tiedoston. Metagame kutsuu deploy-palvelinta ilman avainta, ja deploy-palvelin vastaa vain suoraan
loopbackista tuleviin kutsuihin (kaikkeen muuhun 403), joten `DEPLOYSERVER_URL`-asetuksen on
osoitettava loopbackiin, esimerkiksi `127.0.0.1:61001`.

`.env`-tiedostoissa on salaisuuksia. Ne ovat gitin ohittamia jokaisessa paketissa: älä koskaan jaa niitä
äläkä koskaan commitoi niitä.

### Metagamen asetukset kehitykseen {#metagame-env}

Samat avaimet kuin [Pystytä palvelin -sivun vaiheessa 9]({{ host_page.url | relative_url }}#metagame),
mutta omalla tietokannalla ja ilman asetusta `NODE_ENV=production`:

```powershell
New-Item -ItemType Directory -Force C:\dr\dev | Out-Null
@"
PORT=61000
BIND_HOST=127.0.0.1
AUTH_MODE=APIKEY
DB_FILENAME=C:/dr/dev/undaunted-dev.db
TARGET_CHANGELIST=239827
QOS_TARGET_URL=http://127.0.0.1:61000/QoS
MATCHMAKING_MODE=DEPLOYSERVER
DEPLOYSERVER_URL=127.0.0.1:61001
REGISTRATION_MODE=OPEN
"@ | Set-Content C:\dr\undaunted\legacy\UndauntedMetagame\.env -Encoding ascii
```

Lisää sen jälkeen allekirjoitusavaimet (seuraava osio): ilman niitä metagame pysähtyy heti
käynnistyessään. Aseta aina `DB_FILENAME` tiedostoon, jonka kansio on olemassa: ilman sitä metagame
käyttää väliaikaista tietokantaa, joka katoaa, kun metagame pysähtyy. Näin kehityskone eroaa
palvelimesta:

- **Ei asetusta `NODE_ENV=production`.** Silloin lokit tulostetaan helppolukuisina JSON-rivien sijaan,
  virhesivuilla näkyvät pinojäljet (stack trace) ja `AUTH_MODE=NONE` tulee mahdolliseksi. Pidä
  tällainen metagame osoitteessa `127.0.0.1`.
- **Eikö peliä ole asennettu?** Aseta `MATCHMAKING_MODE=DISABLED` ja jätä deploy-palvelin pois:
  metagame hylkää silloin jokaisen matchmaking-pyynnön, ja voit työstää muuta ilman pelipalvelimia.
- **Testaatko käynnistimen latauksia?** Lisää `CONTENT_PORT=61002` ja käynnistä sisältöpalvelin.
  Yksityisessä tilassa käynnistin saa sisältöpalvelimen portin metagamen tilareitiltä.
- **Eteneminen on oletuksena oikea.** Jätä `PROGRESSION_MODE` pois. `PROGRESSION_MODE=stub` palauttaa
  upstreamin valemaksimitasot, ja sitä kannattaa käyttää vain, kun haluat verrata upstreamiin
  ([Päivitysohjeet]({{ upgrade_page.url | relative_url }})).
- **Pidä palvelinohjeen portit.** Pelipalvelimet löytävät metagamen käyttäjäkohtaisen
  `Game.ini`-tiedostosi kautta ja peliohjelma ensimmäisen käynnistysparametrinsa kautta; molemmat
  osoittavat osoitteeseen `127.0.0.1:61000`. Aja koneella vain yhtä palvelinkokonaisuutta, joka käynnistää
  pelipalvelimia: ne tarvitsevat myös UDP-portit 8770–8777.

Deploy-palvelimen `.env` [vaiheesta 11]({{ host_page.url | relative_url }}#deploy-server) toimii
sellaisenaan.

### Avaimet ja salaisuudet {#secrets}

**Jokainen tämän taulukon arvo on salaisuus: älä koskaan jaa sitä äläkä koskaan commitoi sitä**, äläkä
koskaan liitä sitä issueen, chattiin tai kuvakaappaukseen. Tee kehitykseen uudet arvot. Älä koskaan
kopioi niitä oikealta palvelimelta äläkä koskaan käytä kehitysarvoja oikealla palvelimella.

| Salaisuus | Kuka käyttää | Näin se tehdään |
|---|---|---|
| `AUTH_SIGNING_PRIVKEY_B64` (ja `AUTH_SIGNING_PUBKEY_B64`) | Metagame. Yksityinen avain allekirjoittaa jokaisen 24 tuntia voimassa olevan istuntotunnisteen, joten kuka tahansa, jolla avain on, voi kirjautua millä tahansa tilillä. | Alla oleva komento |
| Pelipalvelinavain | Metagame (se tallentaa vain SHA-256-tiivisteen), deploy-palvelin (`METAGAME_API_KEY`) ja jokaisen pelipalvelimen komentorivi | [Vaihe 9]({{ host_page.url | relative_url }}#metagame): lisää se tietokannan jonoon ja käynnistä metagame uudelleen |
| Tiliavaimet (`UUK_...`) | Tilin salasana: kirjautuminen ja otsake `x-undaunted-user-api-key` | `POST /undaunted/api/Register` palauttaa avaimen kerran; palvelin säilyttää vain sen SHA-256-tiivisteen ([vaihe 10]({{ host_page.url | relative_url }}#admin-account)) |
| `GATEWAY_SECRET` | Metagame ja yhdyskäytävä, sama arvo molemmissa: 32–256 tulostettavaa merkkiä ilman välilyöntejä | `node -e "console.log(require('crypto').randomBytes(32).toString('hex'))"` |
| `ALLOWLIST_SECRET` | Yhdyskäytävä ja sallittujen listan apuri, sama arvo molemmissa. Käytä eri arvoa kuin yhdyskäytävän salaisuudessa. | Sama komento |
| Yhdyskäytävän TLS-avain (`gateway-key.pem`) | Yhdyskäytävä | `make-cert`, [alla](#public-mode-locally). Pidä se repositorion ulkopuolella. |

Allekirjoitusavainpari lisätään metagamen `.env`-tiedostoon tulostamatta sitä näytölle (sama komento
kuin [vaiheessa 9]({{ host_page.url | relative_url }}#metagame)). Käytä komentoa
`Add-Content -Encoding ascii`, älä `>>`: Windows PowerShell 5.1:ssä `>>` kirjoittaa UTF-16-muotoa,
jota Node ei osaa lukea `.env`-tiedostona.

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedMetagame
node -e "const c=require('crypto');const k=c.generateKeyPairSync('rsa',{modulusLength:2048,privateKeyEncoding:{type:'pkcs8',format:'pem'},publicKeyEncoding:{type:'spki',format:'pem'}});console.log('AUTH_SIGNING_PRIVKEY_B64='+Buffer.from(k.privateKey).toString('base64'));console.log('AUTH_SIGNING_PUBKEY_B64='+Buffer.from(k.publicKey).toString('base64'))" | Add-Content .env -Encoding ascii
```

### AUTH_MODE=NONE on vain kehitykseen {#auth-mode-none}

`AUTH_MODE=NONE` poistaa avaintarkistuksen. Käytä sitä vain metagamessa, joka kuuntelee osoitteessa
`127.0.0.1` omalla koneellasi, ei koskaan palvelimella, johon kukaan muu pääsee.

| Kun `AUTH_MODE=NONE` ja | Mitä tapahtuu |
|---|---|
| `NODE_ENV` ei ole `production` | Kirjautuminen (`POST /account/api/oauth/token`) hyväksyy tilitunnukseksi sen, mitä peliohjelma lähettää, ja allekirjoittaa sille tunnisteen. Reitit, jotka ottavat tiliavaimen, ylläpitäjän reitit mukaan lukien, ottavat sen sijaan tilitunnuksen (`UID-...`), joten jokainen, joka tietää ylläpitäjän tilitunnuksen, on ylläpitäjä. Rekisteröi testitili ensin: reitit, jotka hakevat tilin, vaativat edelleen, että se on olemassa. |
| Pyynnössä on välitysotsake (yhdyskäytävältä tai miltä tahansa muulta välityspalvelimelta) | Pyyntö hylätään vastauksella 403. |
| `GATEWAY_SECRET` on asetettu | Metagame ei käynnisty: julkinen tila vaatii asetuksen `AUTH_MODE=APIKEY`. |
| `NODE_ENV=production` | `NONE` jätetään huomiotta. Kirjautuminen ei silloin saa lainkaan vastausta ja peliohjelma jää odottamaan (lokissa lukee, ettei kirjautumistapaa ole määritetty), ja reitit, jotka tarkistavat tiliavaimen, vastaavat 500. Sama tapahtuu, kun `AUTH_MODE` puuttuu tai on kirjoitettu väärin. |

Kun `AUTH_MODE=NONE`, peliohjelman käynnistysparametrissa `-AUTH_PASSWORD=` on avaimen sijaan
tilitunnus. Kaikki muu [vaiheessa 13]({{ host_page.url | relative_url }}#launch-the-client) pysyy
samana.

### Käynnistysjärjestys {#start-order}

1. **Metagame.** Kansiossa `legacy/UndauntedMetagame` joko `npm run dev` tai ensin `npm run build` ja sitten
   `npm start`. Ensimmäinen käynnistys luo tietokannan ja ajaa kaikki migraatiot. Jokainen käynnistys
   rekisteröi avaimet, jotka odottavat tietokannan jonotauluissa. Käynnistä metagame sen omasta
   kansiosta (npm-skriptit tekevät niin): migraatiot löytyvät suhteellisella polulla `./src/drizzle`.
2. **Kerran kutakin tietokantaa kohden:** pelipalvelinavain ja testitili
   ([vaiheet 9 ja 10]({{ host_page.url | relative_url }}#metagame)). Käynnistä metagame uudelleen, kun
   olet lisännyt pelipalvelinavaimen jonoon.
3. **Sallittujen listan apuri, sitten yhdyskäytävä**, vain julkisen tilan työhön.
4. **Sisältöpalvelin**, vain käynnistimen latauksia varten.
5. **Deploy-palvelin viimeisenä.** Se käynnistää Ramsgate-pelipalvelimen heti, ja se palvelin lataa
   hahmot metagamesta.
6. **Peli**, [vaiheen 13]({{ host_page.url | relative_url }}#launch-the-client) skriptillä tai
   käynnistimen kautta ([alla](#launcher-dev)).

Pysäytä päinvastaisessa järjestyksessä: ensin deploy-palvelin, sitten mahdollinen jäljelle jäänyt
`-server`-peliprosessi ja sitten loput ([vaihe 15]({{ host_page.url | relative_url }}#stopping)).
Palvelinpaketti käyttää samaa järjestystä: se käynnistää sallittujen listan apurin, metagamen,
sisältöpalvelimen, yhdyskäytävän ja deploy-palvelimen ja pysäyttää ne käänteisessä järjestyksessä.

### Julkinen tila yhdellä koneella {#public-mode-locally}

Helpoin tapa kokeilla julkista tilaa on `Test-Sandbox.ps1` ([yllä](#kit-tests)): se tekee varmenteen ja
salaisuudet, käynnistää yhdyskäytävän ja apurin vapaissa porteissa ja poistaa kaiken lopuksi. Käsin se
tehdään näin:

1. **Varmenne.** Aja kansiossa `legacy/UndauntedGateway` (kun olet ajanut `npm ci`)
   `node tools/make-cert.js --host 127.0.0.1 --out C:\dr\dev\tls`. Se kirjoittaa tiedostot
   `gateway-cert.pem` ja `gateway-key.pem` ja tulostaa sormenjäljen kutsun `fp=`-kenttää varten.
   Avaintiedosto on salaisuus: älä koskaan jaa sitä äläkä koskaan commitoi sitä.
2. **Yhdyskäytävä ja apuri.** Kopioi `legacy\UndauntedGateway\.env.example` nimelle `.env`. Se on valmiiksi
   asetettu yhdelle koneelle: `GATEWAY_BIND=127.0.0.1` portissa 61443, `ALLOWLIST_DRY_RUN=1` ja
   `ALLOWLIST_ALLOW_PRIVATE=1` (muuten apuri hylkää pelaajan osoitteen `127.0.0.1`). Täytä
   `GATEWAY_CERT`, `GATEWAY_KEY` ja molemmat salaisuudet. Apuri kirjoittaa tiedostot
   `allowlist-audit.log` ja `allowlist-state.json` kansioon `legacy\UndauntedGateway\` (git ohittaa
   molemmat), ellet aseta arvoja `ALLOWLIST_AUDIT_LOG` ja `ALLOWLIST_STATE_FILE`.
   **Älä koskaan aseta kehityskoneella `ALLOWLIST_DRY_RUN=0`:** se muuttaa Windowsin palomuuria
   oikeasti, ja apuri tarvitsee silloin järjestelmänvalvojan oikeudet.
3. **Metagame.** Sama `GATEWAY_SECRET`, `AUTH_MODE=APIKEY` ja `QOS_TARGET_URL`, joka osoittaa
   käynnistimen välittimen porttiin (`http://127.0.0.1:<välittimen portti>/QoS`). Ilman asetusta
   `NODE_ENV=production` metagame varoittaa käynnistyessään; se on vain varoitus.
4. **Välittimen portti.** Pelin ollessa käynnissä käynnistimen välitin tarvitsee osoitteen
   `127.0.0.1:61000`, jota metagamesi jo käyttää, joten käynnistimen PELAA-painike epäonnistuisi
   virheellä `relay_port_busy`. Käynnistä käynnistin niin, että `DAUNTLESS_REVIVED_RELAY_PORT` osoittaa
   toiseen porttiin ([alla](#launcher-dev)).
5. **Kutsu.** Kirjoita v2-kutsu käsin:
   `dauntless-revived://join?v=2&mode=public&host=127.0.0.1&port=<yhdyskäytävän portti>&fp=<sormenjälki>&code=<koodi>&name=Dev`.

Jokainen yhdyskäytävän ja apurin asetus sallittuine arvoineen on sivulla
[Asetukset]({{ config_page.url | relative_url }}). Mitä yhdyskäytävä päästää läpi, kerrotaan sivulla
[HTTP-rajapinta]({{ api_page.url | relative_url }}).

### Käynnistin kehityskäytössä {#launcher-dev}

```powershell
Set-Location C:\dr\undaunted\legacy/UndauntedLauncher
npm ci
npm start
```

`npm start` ajaa Electron Forgen: se kääntää pääprosessin ja preload-skriptin Vitellä ja lataa sivun
Viten kehityspalvelimelta. Kehitysajo eroaa asennetusta käynnistimestä näin:

- DevTools ovat käytettävissä.
- Se ei rekisteröi `dauntless-revived:`-linkkien käsittelijää, joten liitä kutsut Pelaa-sivulle.
- Se ei koskaan etsi eikä asenna päivityksiä.
- Se ottaa kaksi DLL:ää kansiosta `legacy\UndauntedLauncher\assets\` eikä asennetuista resursseistaan. Ne
  tarkistetaan kiinnitettyjä tiivisteitä vasten kummassakin tapauksessa.

Tämä ei muutu: kehitysajo saa nimensä samasta `productName`-kentästä kuin asennettu käynnistin, joten
se käyttää samaa kansiota `%APPDATA%\Dauntless Revived Launcher\` (asetukset, tallennetut tiliavaimet,
loki) kuin saman Windows-käyttäjän asennettu käynnistin. Sulje asennettu käynnistin ennen komentoa
`npm start`. Käynnistin muistaa yhden palvelimen ja yhden pelikansion: testipalvelin, johon liityt
kehitysajossa, on myös se palvelin, jonka asennettu käynnistin avaa seuraavalla kerralla.

**Paikallista palvelinkokonaisuutta vasten (yksityinen tila).** V1-kutsu saa osoittaa
loopback-osoitteeseen: `dauntless-revived://join?v=1&host=127.0.0.1&port=61000&code=DEVTEST1&name=Dev`. V1-kutsun
isäntänä on oltava loopback-osoite, Tailscale-osoite (100.64.0.0/10) tai `*.ts.net`-nimi, koska v1 on
salaamatonta HTTP:tä. Kun `REGISTRATION_MODE=OPEN`, koodia ei tarkisteta, mutta kutsussa on silti
oltava koodi (4–64 merkkiä: kirjaimia, numeroita tai `-`). Lataukset vaativat metagamelle
`CONTENT_PORT`-asetuksen ja käynnissä olevan sisältöpalvelimen. Käynnistin hyväksyy vain
sisältöpalvelimen, jonka manifesti on täsmälleen sama kuin käynnistimeen käännetty, ja vain tiedostot,
jotka vastaavat sitä.

**`DAUNTLESS_REVIVED_RELAY_PORT`** siirtää julkisen tilan välittimen pois portista 61000, esimerkiksi
kun oma metagamesi varaa sen. Arvot 1024–65535 otetaan käyttöön, kaikki muut jätetään huomiotta, ja
käynnistin kirjaa ohituksen lokiinsa. Palvelimen `QOS_TARGET_URL`-asetuksessa on oltava sama portti.
Kaverit eivät koskaan aseta tätä muuttujaa.

```powershell
$env:DAUNTLESS_REVIVED_RELAY_PORT = "61100"
npm start
```

Missä käynnistin säilyttää asetuksensa, salatut tiliavaimet ja lokinsa, kerrotaan sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}). Julkaisujen kääntäminen ja julkaiseminen
kuvataan tiedostossa `legacy/UndauntedLauncher/README.md` (suomeksi `legacy/UndauntedLauncher/README.fi.md`);
julkaisut kääntää GitHub Actions, ei kehittäjän kone.

### Palvelin-DLL {#server-dll}

Lähdekoodi on kansiossa `native/internal-server/` (upstreamin C++-koodi generoidun SDK:n kanssa; avaa
`UndauntedInternalServer.sln` Visual Studio 2022:ssa ja käännä kokoonpano `Release|x64`). Mikään repositoriossa
ei käännä sitä. Peli ja jokainen pelipalvelin ajavat valmiiksi käännettyjä kopioita kansiosta
`legacy/UndauntedLauncher/assets/`, ja niiden SHA-256-tiivisteet on kiinnitetty käynnistimeen
(`src/main/constants.ts`), palvelinpakettiin (`DauntlessServer.Common.ps1`), kaveripakettiin
(`setup.ps1`, `play.ps1`, `tools/make-friend-kit.ps1`) ja dokumentaatioon. Itse kääntämäsi DLL:n
tiiviste ei vastaa kiinnitettyä, joten kaikki nämä hylkäävät sen. DLL:n vaihtaminen tarkoittaa, että
jokainen kiinnitys muutetaan samassa muutoksessa. DLL toimii vain 1.4.4-version exe-tiedoston kanssa
([Pelin asetukset]({{ game_settings_page.url | relative_url }})).

---

## Mitä CI tarkistaa {#ci}

Työnkulku `.github/workflows/ci.yml` ajetaan jokaiselle pushille mihin tahansa haaraan, jokaiselle pull
requestille sekä käsin (Actions > **CI** > **Run workflow**). Sen työt ajavat samat komennot, jotka voit
ajaa itse tämän sivun ohjeilla:

| Työ | Mitä se ajaa | Ajoympäristö |
|---|---|---|
| Palvelinpaketit | `npm ci`, `npm run build` ja `npm test` jokaisessa neljästä paketista | Windows, Node 24 |
| Käynnistin | `npm ci`, `npm run typecheck`, `npm test` ja `npm run make`, sitten `scripts/collect-release.ps1`. Julkaisutiedostot (asennusohjelma, Squirrelin päivitystiedostot, zip ja `SHA256SUMS.txt`) ovat ladattavissa ajon kohdasta **Artifacts** 7 päivän ajan (ei forkeista tulevissa pull requesteissa). | Windows, Node 24 |
| Palvelinpaketti | Jokaisen versionhallinnassa olevan `.ps1`-tiedoston on jäsennyttävä Windows PowerShell 5.1:ssä, PSScriptAnalyzer ei saa löytää virheitä (kun se on ajoympäristössä), sitten paketin kolme testiä | Windows PowerShell 5.1 |
| Dokumentaatio | `sync-roadmap.ts` ja `build-llms.ts` eivät saa muuttaa mitään, sivuston on käännyttävä samalla työkalulla, jota GitHub Pages käyttää, ja käännetyssä sivustossa on oltava sen pääsivut | Linux |
| Repositorion siisteys | `tools/ci/check-repo.ts`: versionhallinnassa olevissa tiedostoissa tai pushin tai pull requestin tuomissa commiteissa ei saa olla salaisuuksia, avaimia, tietokantoja, lokeja eikä pelin tiedostoja, versionhallinnassa ei saa olla tiedostoa, jonka jokin `.gitignore`-sääntö sulkee pois, kahden DLL:n on vastattava kiinnityksiään, ja käynnistimen versionumeron on oltava kelvollinen | Linux |

Poikkeus on sisältöpalvelimen integraatiotesti: se tarvitsee oikean peliasennuksen, joten CI ei aja
sitä. Siisteystarkistuksen voit ajaa itse ennen pushia:
`bun tools/ci/check-repo.ts --history origin/dauntless-revived..HEAD` (ilman valitsinta `--history`
se tarkistaa vain tiedostot). Se ei koskaan tulosta sitä, mitä se löysi tiedoston sisältä. Sen
parametrit ovat sivulla
[Skriptit ja parametrit]({{ scripts_page.url | relative_url }}#ci-workflows-and-tools).

Kaksi muuta tarkistusta ajetaan tämän työnkulun ulkopuolella. CodeQL etsii koodista tietoturvaongelmia
repositorion koodiskannauksen oletusasetuksen (default setup) kautta, joten repositoriossa ei ole
CodeQL-työnkulkutiedostoa. Dependabot (`.github/dependabot.yml`) ehdottaa riippuvuuksien päivityksiä:
GitHub Actionsille viikoittain, jokaiselle npm-paketille kuukausittain.

### Käynnistimen julkaisut {#launcher-releases}

Käynnistimen julkaisu on GitHub-julkaisu `launcher-v<versio>` sille versiolle, joka on
`legacy/UndauntedLauncher/package.json`-tiedostossa, ja se julkaistaan vain `dauntless-revived`-haarasta.
Asennetut käynnistimet päivittyvät siihen. Julkaistaksesi nosta tuota versiota. Sitten:

- **Automaattisesti (oletuksena päällä).** Kun `dauntless-revived`-haaraan tehty push läpäisee kaikki
  yllä olevat työt, on yhä haaran uusin commit eikä sen käynnistinversiolla ole vielä
  `launcher-v<versio>`-julkaisua, CI julkaisee samassa ajossa kääntämänsä ja testaamansa
  asennusohjelman työnkulun `.github/workflows/launcher-release.yml` kautta. Näin tapahtuu vain
  repositoriossa `mixutin/dauntless-revived`, ei koskaan forkissa. Tauon saat asettamalla repositorion
  muuttujan `LAUNCHER_AUTO_RELEASE` arvoon `false`
  ([Asetukset]({{ config_page.url | relative_url }}#ci-settings)).
- **Käsin.** Actions > **Launcher release** > **Run workflow** `dauntless-revived`-haaralle kääntää
  kyseisen commitin ja julkaisee sen version. Tämä toimii myös silloin, kun automaattiset julkaisut on
  pysäytetty. Jo julkaistulla versiolla se vain päivittää itsepäivityskanavan (`launcher-updates`-julkaisun)
  siihen.

Versio julkaistaan vain, jos se on uudempi kuin kaikki aiemmat, eikä julkaistua versiota koskaan
korvata. Esiversio (kuten `0.2.0-beta.1`) julkaistaan GitHubin esijulkaisuna, johon asennetut
käynnistimet eivät päivity. Tarkemmin asiasta kerrotaan tiedoston
[CONTRIBUTING.md]({{ site.github.repository_url }}/blob/dauntless-revived/CONTRIBUTING.md)
kohdassa "Tarkistukset" ja tiedoston
[legacy/UndauntedLauncher/README.fi.md]({{ site.github.repository_url }}/blob/dauntless-revived/legacy/UndauntedLauncher/README.fi.md)
kohdassa "Julkaisut ja päivitykset".

---

## Pidä generoidut tiedostot ajan tasalla {#generated-files}

| Tiedosto | Mikä sen tekee | Aja uudelleen, kun |
|---|---|---|
| `docs/roadmap.md` | `bun tools/sync-roadmap.ts` | `ROADMAP.md` muuttuu. Älä koskaan muokkaa tiedostoa `docs/roadmap.md` käsin. |
| `docs/llms.txt`, `docs/llms-full.txt` | `bun tools/build-llms.ts` | Mikä tahansa englanninkielinen sivu tai UKK:n data muuttuu (`llms-full.txt` sisältää jokaisen englanninkielisen sivun tekstin; `llms.txt` listaa jokaisen sivun otsikkoineen ja kuvauksineen). |
| `legacy/UndauntedMetagame/src/drizzle/*.sql` ja `meta/` | `npm run db:generate` kansiossa `legacy/UndauntedMetagame` | `src/db/schema.ts` muuttuu. Komento lukee `.env`-tiedoston. Commitoi uusi migraatio samassa muutoksessa skeemamuutoksen kanssa. |
| `legacy/UndauntedMetagame/src/vendor/hunt_titles.json` | `node legacy/UndauntedMetagame/scripts/make-hunt-titles.js` | Deploy-palvelimen metsästystaulukot kansiossa `legacy/UndauntedDeployServer/src/vendor/` muuttuvat. |
| `legacy/UndauntedContent/data/dauntless-1.4.4.json` | `bun tools/make-game-manifest.ts --zip <pelin zip>` | Käytännössä ei koskaan: tiedosto kuvaa kiinnitettyä 1.4.4-versiota. Käynnistin kääntää saman tiedoston sisäänsä, joten käännä käynnistin sen jälkeen uudelleen. |
| `legacy/UndauntedLauncher/assets/icon.png`, `icon.ico` ja `src/renderer/brand/*.png` | `npm run icon` kansiossa `legacy/UndauntedLauncher`; se kopioi ne kansiosta `brand/` (siellä ne tekee `python3 brand/build.py`) | Käynnistimen kuvakkeet kansiossa `brand/launcher/` tai tunnuskuvan koot kansiossa `brand/web/` muuttuvat. Käynnistimen yksikkötestit epäonnistuvat, kun jokin kopio on vanhentunut. |

Aja kaksi dokumentaatiogeneraattoria niin, että `sync-roadmap.ts` on ensin: `build-llms.ts` lukee sen
kirjoittaman tiekarttasivun. Kumpikaan ei ota argumentteja, ja molemmat löytävät repositorion juuren
itse. CI ajaa molemmat ja epäonnistuu, jos jokin tiedosto muuttuu.
`docs/fi/roadmap.md` on käsin kirjoitettu suomenkielinen tiivistelmä, ei generoitu tiedosto.

```powershell
Set-Location C:\dr\undaunted
bun tools/sync-roadmap.ts
bun tools/build-llms.ts
git status --short docs
```

Työkalujen argumentit ovat sivulla [Skriptit ja parametrit]({{ scripts_page.url | relative_url }}).

### Dokumentaatiosivut {#docs-pages}

- Jokaisella englanninkielisellä sivulla `docs/<path>.md` on suomenkielinen pari `docs/fi/<path>.md`.
  Kummankin sivun alkutiedoissa (front matter) on sama `ref`, joka yhdistää sivut kielivalintaa varten;
  `lang` on `en` tai `fi`. Suomenkielisillä sivuilla on lisäksi `locale: fi_FI`, ja ne sijoittuvat
  navigaatiossa kohdan "Dauntless Revived suomeksi" alle: se on sivun `parent` tai, kun sivu kuuluu
  osioon, sen `grand_parent`.
- `description` näkyy hakutuloksissa ja linkkien esikatseluissa: pidä se 120–160 merkin mittaisena.
- Linkitä toiseen sivuun sivun alussa määritellyn sivumuuttujan kautta, älä kovakoodatulla
  osoitteella. Suomenkieliset sivut linkittävät suomenkielisiin sivuihin (`fi/...`).

```liquid
{% raw %}{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
[Pystytä palvelin]({{ host_page.url | relative_url }}){% endraw %}
```

- Usein kysyttyjen kysymysten tekstit ovat tiedostoissa `docs/_data/faq_en.yml` ja
  `docs/_data/faq_fi.yml`, kummankin tiedoston alussa kuvatussa kiinteässä muodossa.
- Repositoriossa ei ole Gemfileä. CI kääntää sivuston samoin kuin GitHub Pages; paikallinen esikatselu
  vaatii oman Jekyll-ympäristön, jossa on tiedostossa `docs/_config.yml` luetellut teema ja lisäosat.

---

## Osallistumisen säännöt {#contributing}

Lyhyt versio on tiedostossa
[CONTRIBUTING.md]({{ site.github.repository_url }}/blob/dauntless-revived/CONTRIBUTING.md).
Käytännössä:

- **Aloita keskustelusta** (GitHub Discussions) ja mainitse [tiekartan]({{ roadmap_page.url | relative_url }})
  vaihe, jos sellainen on.
- **Ei salaisuuksia eikä pelin tiedostoja commiteissa.** Ei tiliavaimia, pelipalvelinavaimia,
  allekirjoitusavaimia, yhdyskäytävän tai sallittujen listan salaisuuksia, TLS-avaimia, `.env`-tiedostoja,
  tietokantoja, varmuuskopioita tai lokeja, eikä mitään pelistä: ohjelmatiedostoja, pak-paketteja,
  grafiikkaa tai pelin asetuksia. Myös generoitu `Game.ini` kuuluu joukkoon: yhdessä sen osoitteista on
  salainen webhook-polku. `.gitignore`-tiedostot pysäyttävät tavalliset nimet (`.env`, `*.key`, `*.db`,
  `*.pak`, yhdyskäytävän `*.pem`), eivät kaikkea: katso `git status` ennen jokaista committia. CI:n
  siisteystarkistus hylkää myös tällaiset tiedostot, mutta vasta pushin jälkeen. Jos salaisuus on
  päätynyt committiin, kerro siitä ja vaihda avain. Salaisuus jää jokaisen kloonin historiaan.
- **Pidä tallennetut pyyntöjen sisällöt poissa työpuusta.** `LOG_BODIES=1` kirjoittaa tiedoston
  `bodies.log` kansioon, josta metagame ajetaan (npm-skripteillä `legacy\UndauntedMetagame\`), ellei
  `BODY_LOG_FILE` määrää muuta, eikä `*.log` ole siellä gitin ohittama. Osoita tiedosto repositorion
  ulkopuolelle, esimerkiksi `BODY_LOG_FILE=C:/dr/dev/bodies.log`. Tiedostossa on se, mitä pelaajat
  lähettivät.
- **Testaa kertakäyttöisellä tilillä.** Älä koskaan kokeile uutta palvelinvastausta ensimmäiseksi
  oikean pelaajan tilillä: väärän muotoinen vastaus voi kaataa 1.4.4-peliohjelman tai vahingoittaa
  tallennusta. Tallenna ensin, mitä peli oikeasti lähettää (`LOG_BODIES=1` omalla kehityspalvelimellasi),
  ennen kuin rakennat vastauksen arvauksen varaan.
- **Varmuuskopioi ennen migraatiota.** Metagame ajaa tietokantansa migraatiot käynnistyessään eikä ota
  itse varmuuskopiota.
- **Englannin- ja suomenkielinen dokumentaatio muuttuvat yhdessä.** Kun muutos muuttaa toimintaa, se
  päivittää englanninkielisen sivun ja sen suomenkielisen parin samassa muutoksessa ja ajaa sitten
  dokumentaatiogeneraattorit uudelleen.
- **Vain yleisiä esimerkkejä** dokumentaatiossa, testeissä ja kommenteissa: osoitteita verkoista
  203.0.113.0/24 ja 198.51.100.0/24 sekä paikkamerkkejä kuten `UUK_...` ja `UID-...`, ei oikeita
  avaimia, osoitteita, nimiä tai henkilökohtaisia polkuja.
- **Palvelinpaketin skriptit** toimivat Windows PowerShell 5.1:ssä, ovat pelkkää ASCII:ta eivätkä
  koskaan tulosta avainta, tunnistetta, salasanaa tai `.env`-arvoa. Kansioiden `deploy/windows-server/`
  ja `friend-kit/` `.gitattributes`-tiedostot pitävät niiden rivinvaihdot CRLF-muodossa.
- **Älä koskaan kirjaa avainta tai tunnistetta lokiin.** Lokit peittävät tunnetun muotoiset avaimet ja
  tunnisteet, mutta älä luota siihen.
- **Pienet pull requestit**, joissa kerrotaan, mitä muutit, miten testasit ja mikä tiekartan vaihe on
  kyseessä.
- **Tietoturvaongelmat** ilmoitetaan GitHubin yksityisellä haavoittuvuusilmoituksella (private
  vulnerability reporting), ei issueina
  ([SECURITY.md]({{ site.github.repository_url }}/blob/dauntless-revived/SECURITY.md)).
- **Lisenssi.** AGPL-3.0-only, kuten upstream Undauntedissa. Säilytä olemassa olevat tekijänoikeus- ja
  lisenssimerkinnät.

---

## Missä koodi on {#code-map}

Kunkin taulukon polut ovat suhteessa taulukon yläpuolella mainittuun kansioon.

**`legacy/UndauntedMetagame/src/`**

| Osa-alue | Koodi | Huomiot |
|---|---|---|
| Käynnistys, reititys, pyyntöloki | `server.ts`, `app.ts`, `logger.ts`, `middleware/BodyLog.ts` (`LOG_BODIES`: reitit, peittäminen, tila ja kesto, polkukohtainen raja) | `app.ts` liittää jokaisen reitittimen; tuntemattomat reitit kirjataan lokiin, ja niihin vastataan 404. |
| Ominaisuuskytkimet | `features.ts` | Harmonicin forkin siirron mukana tulleet kytkimet jäsennettyinä yhdessä paikassa (päällä/pois, lukumäärät, vaihtoehdot) sekä käynnistyksen `features:`-rivi. Vanhemmat kytkimet jäsentävät itse. |
| Pelipalvelimen kirjoitukset | `middleware/GameServerOnly.ts` | `RefuseUnlessGameserver` (pelipalvelimen kirjoitusten yhteinen vahti) ja `NoteRelayedAccountMismatch` (kirjaa toisen tilin välitetyn tunnisteen lokiin, ei koskaan torju). |
| Tietokanta | `db.ts`, `db/schema.ts`, `drizzle/` | Migraatiot ajetaan jokaisessa käynnistyksessä journaalin järjestyksessä; ajetut migraatiot kirjataan tauluun `__drizzle_migrations`. `DB_WAL` otetaan käyttöön tiedostossa `db.ts`. |
| Mistä pyyntö tuli | `middleware/RequestOrigin.ts` | Julkinen tila: yhdyskäytävän salaisuus, pelaajan osoite, `AUTH_MODE=NONE`-tilan hylkäykset. |
| Avaimet ja kirjautuminen | `controllers/auth.ts`, `controllers/apikeys.ts`, `routes/eos.ts`, `routes/login.ts`, `middleware/Has*.ts` | `eos.ts` sisältää tunnistekirjautumisen. |
| Tilit, kutsut, käyttäjänimet | `controllers/accounts.ts`, `controllers/login.ts`, `routes/undauntedapi.ts`, `controllers/undauntedapi.ts` | `/undaunted/api`-reitit ([HTTP-rajapinta]({{ api_page.url | relative_url }})). |
| Hahmot ja tallennushistoria | `controllers/character.ts`, `controllers/savehistory.ts`, `routes/character.ts` | Palautukset aiempaan tallennukseen ovat ylläpitäjän reittejä tiedostossa `routes/undauntedapi.ts`. |
| Eteneminen | `controllers/progressionmode.ts` (oikea tai stub, `PROGRESSION_MODE`), `controllers/realprogression.ts` (radat, tavoitteet, myönnöt ja niiden uusintavahti, tasojen vahvistukset ja valinnaiset vahvistuksen oikeudet, ylläpitäjän alustus (seed), päivitysilmoitus), `controllers/progressionconfig.ts` (etenemisasetusten ainoa lataaja: mukana tuleva `vendor/progression_config.json`, `PROGRESSION_CONFIG_DIR`, `ACTIVE_HUNT_PASS`), `controllers/progressionrank.ts` (tasojen laskenta), `controllers/progressionevents.ts` (auditointirivit), `controllers/progression.ts` (kohdattu sisältö ja vihjepolut eli breadcrumbs), `routes/progression.ts`, `middleware/RealProgressionOnly.ts` | `routes/progression.ts` vastaa sekä oikean että stub-tilan muodossa ja kirjaa `/progression`-pyynnöt, joihin mikään reitti ei vastaa. Taulut ovat migraatiossa `0011_real_progression.sql`. |
| Escalation | `controllers/escalationConfig.ts` (kausiluettelo `vendor/escalation/seasons.json`, tarkistetaan latauksessa), `controllers/escalation.ts` (luvut, tallennussäännöt, auditointirivit), `routes/escalation.ts` (tynkä- ja oikeat reitit, `ESCALATION_MODE`) | Siirretty Harmonicin haarasta. Taulut ovat migraatiossa `0014_escalation.sql`. [Escalation]({{ '/fi/findings/escalation.html' | relative_url }}); testit `test/escalation.test.ts`. |
| Hunt Pass, oikeudet (entitlements), bountyt, jäähtymisajat | `routes/system.ts`, `controllers/entitlements.ts`, `controllers/bounties.ts`, `controllers/cooldowns.ts` | Osa oikeaa etenemistä. |
| Varustukset ja varustuspaikat | `controllers/loadout.ts`, `routes/loadout.ts` | Varustuspaikat ovat olemassa vain tileillä, joilla on oikea eteneminen. |
| Inventaario, valuutta ja kauppa | `controllers/inventory.ts` (tapahtumat, ylikulutukset, uusintayritykset; `ApplyInventoryTransactionInTx` toimii kutsujan tapahtuman sisällä, `RunInventoryTransaction` käärii sen reitille `POST /inventory`), `routes/inventory.ts`, `controllers/store.ts` (valuuttataulukko ja hahmon valuutat), `controllers/activecharacter.ts` (tilin viimeksi tallennettu hahmo), `controllers/freestore.ts` (ilmainen kauppa: valikoima, tunnisteet, lunastus), `routes/store.ts` | Kauppa on siirretty Harmonicin haarasta, pois päältä ellei `STORE=free`; data tiedostoissa `vendor/store_catalog.json` ja `vendor/store_item_kinds.json`, taulu migraatiossa `0015_store_purchases.sql`. Testit `test/freestore.test.ts`, `test/inventorytx.test.ts`. |
| Ryhmät | `controllers/party.ts` (muistissa), `routes/party.ts`, `middleware/PlayerAuth.ts` | Ryhmän metsästys kulkee tiedoston `controllers/matchmaking.ts` kautta; `PartyInvite` on tiedostossa `routes/undauntedapi.ts`. |
| Kaverit ja estot | `controllers/friends.ts` (tallennetaan SQLiteen, migraatio `0012_friends_and_blocks.sql`; `AddFriendshipListener` kertoo chat-palvelimelle hyväksytyistä ja päättyneistä kaveruuksista), `routes/friends.ts` (Epic-tyylinen kaveripalvelu, jota peliohjelma kutsuu) | `Friends` on tiedostossa `routes/undauntedapi.ts`. |
| Slayer Links | `controllers/slayerlinks.ts` (säännöt ja vastausten muodot ohjelmatiedoston osoitteineen), `routes/slayerlinks.ts` (kahdeksan reittiä, `SLAYER_LINKS`) | Rajapinta Harmonicin haarasta, korjattuna. Taulut migraatiossa `0016_slayer_links.sql`. Testit `test/slayerlinks.test.ts`. |
| Killat | `controllers/guild.ts` (tallennetaan SQLiteen, migraatio `0013_guilds.sql`; säännöt ja vastausten muodot), `routes/guild.ts` (yksitoista v2-reittiä rekisteröintijärjestyksessä), `middleware/GameServerKeyAuth.ts` (vain pelipalvelimen tekemä perustaminen) | `GuildInvite`, `Guilds` ja `DisbandGuild` ovat tiedostossa `routes/undauntedapi.ts`. `test/socialclient.ts` mallintaa, miten peliohjelma jäsentää sosiaaliset vastaukset; `test/guildhttp.test.ts` ja `test/socialflow.test.ts` käyttävät sitä. |
| Chat (XMPP) | `realtime/chat.ts` (WebSocket-kuuntelija, SASL-kirjautuminen pelaajan tunnisteella, istunnot ja niiden rajat, pingit, kuiskaukset; `ReadChatConfig` ja `StartChat`, jota `server.ts` kutsuu vain, kun `CHAT=1`), `realtime/muc.ts` (huoneet: liittymiset, poistumiset, huonerivien jakelu, ryhmä- ja kiltajäsenyyden tarkistukset, 60 sekunnin läpikäynti), `realtime/chatnick.ts` (peliohjelman URL-koodaus ja huoneen nimimerkin säännöt), `realtime/xmpp.ts` (jäsennys, merkkien suojaus, JID:t, tahtirajat, jäljityksen peittäminen), `realtime/presence.ts` (kavereiden paikalla olo asetuksella `CHAT_PRESENCE=1`: kuka kuulee kenestä, välitykset, kaverilistan päivitys ja sääntö, ettei yksikään huoneen ulkopuolinen viesti saavu pelaajalle hänen omalta tililtään) | Jäsenyydet ja estot tulevat funktioista `GetPartyOf` tiedostossa `controllers/party.ts`, `IsGuildMember` tiedostossa `controllers/guild.ts` ja `BlockersAmong` tiedostossa `controllers/friends.ts`. [Tekstichat]({{ '/fi/findings/chat.html' | relative_url }}) kertoo, miksi kukin vastaus on muodoltaan sellainen. Testit: `test/chat.test.ts` (päästä päähän WebSocketin yli), `test/chatmodel.test.ts` ja `test/chatclient.ts` (malli siitä, miten 1.4.4-peliohjelma lukee chattia), `test/chathttp.test.ts` ja `test/chatenv.ts` (nimet oikeiden tilireittien kautta), `test/chatwire.ts` (pelin yhteyden kaltainen raaka asiakas), `test/presence.test.ts` ja `test/chatinvariant.ts` (tarkistaa näiden tiedostojen jokaisen viestin oman tilin sääntöä vasten). |
| Matchmaking | `controllers/matchmaking.ts` (jonot, ehdokkaiden tila, ryhmät, kutsu deploy-palvelimelle), `routes/matchmaking.ts` | |
| Palvelimen tila | `controllers/serverstatus.ts`, `middleware/SoftAccountAuth.ts`, `/dauntless-status` tiedostossa `routes/system.ts` | |

**`legacy/UndauntedDeployServer/src/`**

| Osa-alue | Koodi | Huomiot |
|---|---|---|
| Pelipalvelimet | `controllers/gameservers.ts` (portit, Ramsgate ja Dojo yhdellä yhteisellä käynnistyksellä maailmaa kohden ja elossaolon tarkistuksella, prosessien käynnistys sekä niiden `error`- ja `exit`-kuuntelijat, testikoukut), `controllers/watchdog.ts`, `routes/gameservers.ts` | Vain loopback-kutsujat. Testit `test/persistentworlds.test.ts` ja `test/deployenv.ts` (korvaava käynnistysfunktio). |
| Matchmaking | `controllers/matchmaker.ts`, `controllers/matchmakinginput.ts` (toinen tarkistus kaikelle, mikä päätyy pelipalvelimen komentoriville), `routes/matchmaker.ts`, `vendor/*_table.json` | |

**`legacy/UndauntedGateway/`**

| Osa-alue | Koodi | Huomiot |
|---|---|---|
| Yhdyskäytävän välitys | `src/gateway.ts` (TLS-palvelin ja välitys `127.0.0.1`:ssä kuunteleville taustapalveluille, WebSocket-yhteydet mukaan lukien), `src/policy.ts` (mitä kukin pyyntö saa tehdä), `src/ratelimit.ts`, `src/ip.ts` (osoitteiden jäsennys ja osoitealueet), `src/server.ts`, `src/config.ts` | |
| Pääsyloki | `src/log.ts`, `src/redact.ts` | Poluista poistetaan tunnisteet ja avaimet. |
| Sallittujen listan syöte ja apuri | `src/feed.ts` (kertoo apurille, kuka kirjautui), `src/allowlist/` (`server.ts`, `config.ts`, `helper.ts`, `state.ts`, `sync.ts`, `firewall.ts`) | |
| Varmennetyökalu | `tools/make-cert.js` | |

**`legacy/UndauntedContent/`**: `src/server.ts` ja `src/config.ts` (käynnistys ja asetukset), `src/app.ts`
(reitit), `src/auth.ts` (avaintarkistus metagamea vasten),
`src/paths.ts`, `src/range.ts`, `src/limits.ts`, `src/gamedir.ts`, `src/manifest.ts`,
`src/branding.ts`, `src/news.ts`, `src/verify.ts` (`npm run verify`) sekä manifesti tiedostossa
`data/dauntless-1.4.4.json`.

**`legacy/UndauntedLauncher/src/`**

| Osa-alue | Koodi | Huomiot |
|---|---|---|
| Lataaja | `main/downloader.ts` (jatkettavat lataukset, `.part`-tiedostot, neljä kerrallaan), `main/verify.ts` (ensin koko, sitten SHA-256), `main/manifest.ts` ja `main/game-manifest.ts` (käynnistimeen käännetty manifesti) | |
| Paikallinen välitin | `main/relay.ts` | Julkinen tila: `127.0.0.1:61000` yhdyskäytävälle kiinnitetyn TLS:n yli, WebSocket mukaan lukien, pelin ollessa käynnissä. |
| Yhteydet palvelimeen | `main/http.ts` (pyynnöt vain kutsussa nimettyyn palvelimeen: salaamaton HTTP Tailscaleen tai loopbackiin tai kiinnitetty TLS yhdyskäytävälle), `main/pinned.ts` (kiinnitetty TLS), `main/hostapi.ts` (metagamen ja sisältöpalvelimen kutsut) | |
| Tila ja kulku | `main/controller.ts` | Ei tuo mitään Electronista, joten testit ajavat sitä pelkällä Nodella. |
| Avaimet, asetukset, pelin valmistelu | `main/keystore.ts` (Windows DPAPI), `main/settings.ts`, `main/engineini.ts`, `main/launch.ts`, `main/dlls.ts`, `main/constants.ts` (DLL-kiinnitykset ja päivityskanava) | |
| Kutsut ja käyttöliittymän tekstit | `shared/invite.ts`, `shared/i18n/` (kaikki käyttöliittymän tekstit, yksi tiedosto kieltä kohden; siellä oleva `README.md` kertoo, miten kieli lisätään) | |
| Ikkuna ja sivu | `main.ts`, `preload.ts`, `renderer/` | |

**Muualla**

| Osa-alue | Koodi |
|---|---|
| Palvelin-DLL | `native/internal-server/dllmain.cpp` (päätepistetaulukko, asiakas- ja palvelintila), `constants.h` |
| Palvelinpaketti | `deploy/windows-server/*.ps1`; yhteiset kiinnitykset, polut, osien taulukko ja käynnistysjärjestys tiedostossa `DauntlessServer.Common.ps1`; tietokanta-, avain- ja pelikansioapurit kansiossa `lib/` |
| Kaveripaketti | `friend-kit/`, jonka `tools/make-friend-kit.ps1` pakkaa zip-tiedostoksi |
| Dokumentaatiotyökalut | `tools/sync-roadmap.ts`, `tools/build-llms.ts` |
| CI | `.github/workflows/ci.yml`, `.github/workflows/launcher-release.yml`, `.github/dependabot.yml`, `tools/ci/`, `legacy/UndauntedLauncher/scripts/collect-release.ps1` |
