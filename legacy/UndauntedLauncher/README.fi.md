# Dauntless Revived Launcher

*[In English](README.md)*

Windows- ja Linux-sovellus, jonka kaverit asentavat pelatakseen Dauntless Revived -palvelimella. Se liittyy
palvelimelle kutsulla, luo tilin, lataa ja tarkistaa Dauntless 1.4.4:n pelitiedostot ylläpitäjän
omalta palvelimelta, laittaa kaiken valmiiksi ja käynnistää pelin. Kielinä englanti ja suomi.

**Linux:** x86_64-Linuxille julkaistaan AppImage-, deb-, rpm- ja siirrettävät paketit. Windowsin 1.4.4-peliasiakas käynnistyy Protonin tai Winen kautta. Katso [jakelukohtainen Linux-ohje](https://mixutin.github.io/dauntless-revived/fi/setup/linux.html) tai [LINUX.md](LINUX.md).

**Sovelluksessa tai tässä repositoriossa ei ole pelitiedostoja.** Ne tulevat ylläpitäjän
sisältöpalvelimelta, ja jokainen tiedosto tarkistetaan käynnistimeen käännettyä listaa vasten
(410 tiedostoa, koot ja SHA-256-tiivisteet, `legacy/UndauntedContent/data/dauntless-1.4.4.json`).
Palvelin, joka tarjoaa jotain muuta, hylätään.

**Yhteisö:** [Liity Dauntless Revived Discordiin](https://discord.gg/ZJRprHzsgu) saadaksesi apua, pelikavereita ja projektin päivityksiä. Discord-painike näkyy myös aina käynnistimen sivupalkissa.

## Kavereille

1. Asenna käynnistin projektin julkaisusta. Windowsissa käytä
   `DauntlessRevivedLauncher-Setup.exe`-asennusohjelmaa. Linuxissa valitse jakelullesi sopiva
   AppImage-, `.deb`-, `.rpm`- tai siirrettävä paketti [Linux-käynnistimen ohjeesta](https://mixutin.github.io/dauntless-revived/fi/setup/linux.html). Tarkista lataus saman julkaisun
   `SHA256SUMS.txt`-tiedostoa vasten. Windows-asennusohjelmaa ei ole vielä allekirjoitettu, joten
   SmartScreen voi pyytää vahvistusta; jos se estää ohjelman kokonaan, tarkista tiiviste, poista
   tiedoston esto (Ominaisuudet > Poista esto tai `Unblock-File`) ja käynnistä se uudelleen.
   Allekirjoitus on tiekartan kohta 4.16.
2. Avaa ylläpitäjän lähettämä kutsulinkki tai liitä se Pelaa-sivulle ja paina **LIITY**.
3. Valitse käyttäjänimi ja paina **REKISTERÖIDY**. Tallenna avaimesta varmuuskopio, kun käynnistin
   tarjoaa sitä.
4. Paina **ASENNA** (noin 11 Gt). Voit pitää tauon, sulkea käynnistimen ja jatkaa myöhemmin. Jos
   sinulla on jo Dauntless 1.4.4, valitse sen sijaan **Minulla on jo pelitiedostot** ja liitä
   `BaseGame144`-kansiosi (tai sen sisällä olevan `Dauntless`-kansion) polku, tai selaa siihen.
   Käynnistin käyttää peliä siinä kansiossa, jossa se on: se tarkistaa jokaisen tiedoston
   kiinnitettyä luetteloa vasten, korvaa tiedostot, jotka eroavat 1.4.4:stä, ja laittaa omat
   `dxgi.dll`- ja `UndauntedInternalServer.dll`-tiedostonsa kansioon `Archon\Binaries\Win64`.
   Puuttuvat ja poikkeavat tiedostot ladataan ylläpitäjän palvelimelta, joten se onnistuu vain, jos
   ylläpitäjä on avannut pelin latauksen. Kopioi kansio ensin, jos jokin toinen asennus käyttää sitä
   yhä.
5. Paina **PELAA**.

Vasemman reunan palkin alaosassa, joka sivulla (myös ennen kuin liityt palvelimelle), GitHub-painike
avaa tämän repositorion selaimeen, ja **Tekijät** kertoo, ketkä tekivät käynnistimen ja palvelimen,
mitä avoimen lähdekoodin ohjelmistoja ne käyttävät ja millä lisenssillä.

## Kaksi palvelintyyppiä

| | Yksityinen (kutsu v1) | Julkinen (kutsu v2) |
|---|---|---|
| Missä | ylläpitäjän koneella | palvelimella, jolla on julkinen IP |
| Miten kaverit yhdistävät | Tailscale | suoraan, TLS-yhteydellä palvelimen yhdyskäytävään |
| Kutsu | `dauntless-revived://join?v=1&host=…&port=61000&code=…&name=…[&share=…]` | `dauntless-revived://join?v=2&mode=public&host=…&port=443&fp=…&code=…&name=…` |

Julkisessa tilassa kutsussa on `fp`, palvelimen TLS-varmenteen SHA-256. Jokainen yhteys (tila,
rekisteröinti, lataukset ja itse peli) tarkistetaan sitä vasten ennen kuin mitään lähetetään.
Palvelin, jolla on eri varmenne, ei saa mitään, ja käynnistin kertoo, ettei tämä ole kaverisi palvelin.

Julkisen palvelimen tiliavain on sidottu tähän sormenjälkeen. Kutsu samaan osoitteeseen eri
varmenteella ei koskaan saa avainta: käynnistin näyttää molemmat sormenjäljet ja kysyy ensin, ja avain
siirtyy uudelle varmenteelle vain, jos vahvistat sen (tee niin vain, kun ylläpitäjä kertoo
asentaneensa palvelimen uudelleen). Yksityiset (v1) kutsut toimivat vain Tailscale-osoitteilla
(100.64.0.0/10) ja `*.ts.net`-nimillä, koska se yhteys on salaamatonta HTTP:tä tailnetin sisällä.
(Myös koneen omat loopback-osoitteet, kuten `127.0.0.1`, hyväksytään, jotta käynnistintä voi kokeilla
samalla koneella olevaa palvelinta vasten.)

Dauntless 1.4.4 osaa vain salaamatonta HTTP:tä, joten julkisessa tilassa käynnistin pitää pelin ajan
käynnissä **paikallista välitintä**: peli puhuu omalla koneellasi osoitteeseen
`http://127.0.0.1:61000`, ja välitin kuljettaa jokaisen pyynnön (ja chatin WebSocketin) lukitulla
TLS-yhteydellä palvelimelle. Pidä käynnistin auki pelatessasi; se kysyy ennen sulkemista, jos peli on
käynnissä. Välitin kuuntelee vain osoitteessa 127.0.0.1 ja hylkää verkkosivuilta tulevat pyynnöt.

## Tietoturva

- Sivu on eristetty (sandbox, context isolation, ei Node-oikeuksia). Se ei pääse verkkoon lainkaan:
  pääprosessi tekee jokaisen pyynnön, ja vain kutsun palvelimelle. Sivu puhuu pääprosessille pienen
  tyypitetyn rajapinnan kautta (`src/preload.ts`), ja jokainen argumentti tarkistetaan uudelleen.
- Tiukka Content-Security-Policy. Kuvat tulevat vain sovelluksesta itsestään tai ylläpitäjän
  kuvapaketista, jonka pääprosessi hakee lukitulla yhteydellä.
- Siirtymät ja uudet ikkunat on estetty. Linkit avautuvat selaimeen vain kiinteältä sallittujen
  listalta: sivu nimeää linkin (esimerkiksi `project_source`), ei koskaan anna osoitetta, ja
  pääprosessi avaa linkin kiinteän osoitteen (`src/main/links.ts`).
- Tiliavain tallennetaan Electronin `safeStorage`-rajapinnalla käyttöjärjestelmän suojattuun
  tallennustilaan. Sitä ei näytetä, sitä ei kirjoiteta tavalliseen tiedostoon (paitsi itse
  tallentamaasi varmuuskopioon), ja se peitetään lokissa.
- Avain lähetetään vain kutsun palvelimelle (julkisessa tilassa vain lukitun yhteyden kautta): tilin
  tarkistuksen, latausten ja palvelimen tilan mukana. Palvelin näyttää paikalla olijat vain
  rekisteröityneille pelaajille, joten ennen rekisteröitymistä palvelinpaneeli pyytää kirjautumaan
  eikä listaa ketään. Ennen tätä muutosta julkaistut käynnistimet eivät lähetä avainta eivätkä tunne
  tätä vastausta: muutoksen sisältävää palvelinta vasten ne näyttävät 0 pelaajaa paikalla eikä yhtään
  maailmaa, kunnes ne päivittävät itsensä. Päivitä vanhempi palvelin vasta, kun muutoksen sisältävä
  käynnistin on julkaistu.
- Electron-fuset: ei `ELECTRON_RUN_AS_NODE`-, `NODE_OPTIONS`- eikä debug-lippuja, ja sovelluskoodi
  ladataan vain eheystarkistetusta asar-paketista.
- Pelin tarvitsemat kaksi DLL:ää tulevat käynnistimen mukana, ja ne tarkistetaan lukittuja tiivisteitä
  vasten ennen kopiointia ja ennen jokaista käynnistystä, kuten `friend-kit/play.ps1` tekee.

## Kehitys

Vaatimukset: Windows tai x86_64 Linux ja Node.js 24. Linux-pakettien tekeminen tarvitsee lisäksi
`rpmbuild`-, `fakeroot`- ja `mksquashfs`-työkalut.

```text
npm ci
npm run typecheck
npm test            # yksikkötestit (node:test), paikalliset testipalvelimet vain porteissa 62012-62013 ja 624xx
npm run make        # Windows: Squirrel-asennusohjelma + siirrettävä zip
npm run make:linux  # Linux: AppImage + deb + rpm + siirrettävä zip
npm run collect:linux
```

`DAUNTLESS_REVIVED_RELAY_PORT` siirtää välittimen pois portista 61000 harjoituksia varten koneella,
jolla portti 61000 on varattu. Palvelimen `QOS_TARGET_URL` pitää silloin osoittaa samaan porttiin.

Ulkoasu noudattaa projektin brändiä (`brand/`, katso sen README): tiedoston `brand/palette.json`
kahdeksan väriä `--dr-*`-muuttujina tiedostossa `src/renderer/styles.css`, logo ja tunnuskuva sekä
tiedoston `src/renderer/scene.ts` piirtämä oma tausta (vuoret, havumetsä, usva, himmeät revontulet ja
lumi), jonka palvelimen kuvapaketti korvaa. Se liikuttaa vain kokonaisia kerroksia hitaasti; se pysyy
paikallaan pelin ollessa käynnissä eikä liiku lainkaan, kun käyttöjärjestelmä pyytää
vähennettyä liikettä. `npm run icon` kopioi kuvakkeet ja ikkunan kuvat kansiosta `brand/`,
jossa `python3 brand/build.py` tekee ne. `npm test` epäonnistuu, jos jokin kopio on vanhentunut, jos
tyylitiedostossa on väri, joka ei ole muuttuja, tai jos jokin sen käyttämä väripari ei läpäise WCAG
AA -vaatimusta.

## Julkaisut ja päivitykset

CI (`.github/workflows/ci.yml`) kääntää ja testaa käynnistimen Windowsissa ja Linuxissa jokaisen
pushin yhteydessä. Windows-asennusohjelma/ZIP sekä Linuxin AppImage/DEB/RPM/ZIP säilytetään viikon
ajan. `.github/workflows/launcher-release.yml` julkaisee nämä samat testatut tiedostot
`dauntless-revived`-haarasta GitHub-julkaisuna `launcher-v<versio>`, yhdellä
`SHA256SUMS.txt`-tiedostolla ja käännöstodistuksella (build provenance attestation), ja luo tagin
käännettyyn committiin. Uusi julkaisu syntyy siis nostamalla versiota:

- oletuksena `dauntless-revived`-haaran push, joka läpäisee kaikki tarkistukset ja jonka versiolla ei
  ole vielä `launcher-v<versio>`-julkaisua, julkaisee CI:n kääntämän asennusohjelman. Tauon saat
  asettamalla repositorion muuttujan `LAUNCHER_AUTO_RELEASE` arvoon `false` (Settings > Secrets and
  variables > Actions > Variables);
- tai aja Actions > **Launcher release** > **Run workflow** `dauntless-revived`-haaralle.

Julkaisun jälkeen työnkulku siirtää jatkuvan `launcher-updates`-julkaisun uuden Windowsin
Squirrel-paketin kohdalle; asennetut Windows-käynnistimet tarkistavat sen tunnin välein. Linuxissa
päivitetään pakettimuodon kautta tai lataamalla uudempi julkaisu. Versio julkaistaan vain, jos se on
uudempi kuin kaikki aiemmat, eikä julkaistua versiota koskaan korvata. Esiversio (kuten
`0.2.0-beta.1`) julkaistaan GitHubin esijulkaisuna, eikä se koskaan päädy `launcher-updates`-julkaisuun.
Jos ajo epäonnistuu kesken, aja sen epäonnistuneet työt uudelleen: se viimeistelee aloittamansa. Jo
julkaistulle versiolle ajettu työnkulku vain päivittää Windowsin päivityssyötteen siihen. Pidä
GitHubin muuttumattomat julkaisut (immutable releases) pois päältä, koska `launcher-updates`-julkaisua
päivitetään paikallaan.

## Lisenssi

AGPL-3.0-only. Perustuu gwogin (Gregory Morford) ja muiden tekijöiden Undaunted-käynnistimeen.
Epävirallinen faniprojekti, jolla ei ole yhteyttä Phoenix Labsiin tai Epic Gamesiin.

Käynnistimen ja sen kahden DLL:n sisältämien kolmansien osapuolten ohjelmistojen (muun muassa
MinHook, Electron ja GitHub Octicons -logo) lisenssitekstit tulevat sen mukana tiedostossa
`THIRD-PARTY-NOTICES.txt`. Tekijät-sivu lukee luettelonsa tiedostosta `src/shared/credits.ts`.
