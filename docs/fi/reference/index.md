---
title: Tekninen viite
parent: Dauntless Revived suomeksi
nav_order: 3
has_children: true
has_toc: false
description: "Dauntless Revivedin tekninen viite: jokainen asetus, portti, HTTP-reitti, tiedosto, pelin asetus ja skripti oletusarvoineen, tarkistettuna koodia vasten."
lang: fi
ref: reference/index
locale: fi_FI
---

{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign ports_page = site.pages | where: "path", "fi/reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "fi/reference/scripts.md" | first %}
{% assign dev_page = site.pages | where: "path", "fi/reference/development.md" | first %}
{% assign setup_page = site.pages | where: "path", "fi/setup/index.md" | first %}
{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}

# Tekninen viite

Näillä sivuilla on Dauntless Revivedin palvelinten ja skriptien tarkat tiedot: jokainen asetus,
portti, reitti, tiedosto ja parametri, sen oletusarvo, sallitut arvot ja se, kuka sen asettaa. Jokainen
tieto on tarkistettu koodia vasten. Jos ohje ja koodi olivat eri mieltä, koodi voitti ja ohje
korjattiin.

[Asennus]({{ setup_page.url | relative_url }})-osion sivut ovat vaihe vaiheelta eteneviä ohjeita.
Aloita niistä, jos haluat saada palvelimen toimimaan: [Pystytä palvelin]({{ host_page.url | relative_url }})
yhdelle koneelle tai [Windows-palvelin]({{ winserver_page.url | relative_url }}) vuokratulle
palvelimelle. Tule tänne, kun haluat tietää tarkalleen, mitä jokin asetus tekee tai miksi jokin
toimii niin kuin toimii.

**Oikea eteneminen on oletus.** Nämä sivut kuvaavat nykyistä koodia, jossa jokainen tili tallentaa
oman Slayer-tasonsa, mestaruutensa ja Hunt Passinsa, ellet aseta arvoa `PROGRESSION_MODE=stub`
(alkuperäisen projektin valemaksimitasot). Lue [päivitysohjeet]({{ upgrade_page.url | relative_url }}#real-progression-default)
ennen kuin päivität palvelimen, jolla on jo pelaajia.

## Sivut {#pages}

| Sivu | Mitä se kattaa |
|:-----|:---------------|
| [Asetukset]({{ config_page.url | relative_url }}) | Metagamen, deploy-palvelimen, yhdyskäytävän, sallittujen listan apurin ja sisältöpalvelimen jokainen ympäristömuuttuja sekä käynnistimen omat ohitukset: oletus, sallitut arvot, mitä muuttuja tekee, kuka sen asettaa ja mitkä ovat salaisia. Lisäksi se, mitä Windows-palvelinpaketti kirjoittaa `.env`-tiedostoihinsa, mitkä kytkimet ovat oletuksena päällä, ja repositorion muuttuja, joka pysäyttää käynnistimen automaattiset julkaisut. |
| [Portit ja verkko]({{ ports_page.url | relative_url }}) | Jokainen TCP- ja UDP-portti, mihin osoitteeseen kukin osa sitoutuu yksityisessä ja julkisessa tilassa, mitä reittiä pyyntö kulkee, mitä ei saa koskaan avata internetiin ja mitkä palomuurisäännöt palvelinpaketti tekee. |
| [HTTP-rajapinta]({{ api_page.url | relative_url }}) | Metagamen jokainen reitti (pelin reitit ja hallintarajapinta polun `/undaunted/api` alla) sekä deploy-palvelimen, sisältöpalvelimen, yhdyskäytävän, sallittujen listan apurin ja käynnistimen välittimen reitit: kuka saa kutsua, mitä otsakkeita tarvitaan ja mitä vastataan. |
| [Tiedostot ja data]({{ files_page.url | relative_url }}) | Repositorion rakenne, käsin pystytetyn palvelinkoneen kansiot, palvelinpaketin asennuskansio, SQLite-tietokanta taulu taululta, lokit, varmuuskopiot ja kaverin koneen tiedostot. Mitkä tiedostot sisältävät salaisuuksia. |
| [Pelin asetukset]({{ gamesettings_page.url | relative_url }}) | Mitä 1.4.4-peliohjelmassa ja sen pelipalvelimissa muuttuu: käyttäjän `Engine.ini`-, `Game.ini`- ja `GameUserSettings.ini`-rivit ja kuka ne kirjoittaa, peliohjelman ja pelipalvelimen komentorivit, kaksi DLL-tiedostoa ja kiinnitetty versio. |
| [Skriptit ja parametrit]({{ scripts_page.url | relative_url }}) | Windows-palvelinpaketin, kaveripaketin ja `tools/`-kansion jokainen skripti, jokainen npm-skripti sekä CI-työnkulut ja niiden työkalut, parametrit oletusarvoineen ja esimerkkeineen. |
| [Kehittäjän opas]({{ dev_page.url | relative_url }}) | Koodin parissa työskentely: esivaatimukset, kunkin paketin käännös ja testit, palvelinten ja käynnistimen ajaminen yhdellä koneella, CI ja käynnistimen julkaisut, generoitujen tiedostojen päivitys ja se, missä mikäkin koodi on. |

## Osat {#the-components}

Osat on nimetty niiden repositoriokansion mukaan.

| Kansio | Mikä se on | Asetukset | Reitit ja skriptit |
|:-------|:-----------|:----------|:-------------------|
| `legacy/UndauntedMetagame/` | Taustapalvelu, jonka kanssa peli keskustelee: tilit, hahmot, tavarat, eteneminen, pelaajien yhteen sovittaminen (matchmaking), ryhmät ja hallintarajapinta. | [Metagame]({{ config_page.url | relative_url }}#metagame) | [Pelin reitit]({{ api_page.url | relative_url }}#game-routes), [hallintarajapinta]({{ api_page.url | relative_url }}#undaunted-api) |
| `legacy/UndauntedDeployServer/` | Käynnistää ja valvoo pelipalvelimet: Ramsgaten, Training Dojon ja yhden kutakin metsästystä kohden. | [Deploy-palvelin]({{ config_page.url | relative_url }}#deploy-server) | [Deploy-palvelin]({{ api_page.url | relative_url }}#deploy-server) |
| `native/internal-server/` | DLL, joka tekee 1.4.4-peliohjelman kopiosta pelipalvelimen ja ohjaa peliohjelmat metagameen. | [Pelin asetukset]({{ gamesettings_page.url | relative_url }}#dlls) | ei reittejä |
| `legacy/UndauntedGateway/` | Vain julkisessa tilassa: salattu yhdyskäytävä, ainoa kaikille avoin portti, ja sallittujen listan apuri, joka avaa peliportit kirjautuneille pelaajille. | [Yhdyskäytävä]({{ config_page.url | relative_url }}#gateway), [apuri]({{ config_page.url | relative_url }}#allowlist-helper) | [Yhdyskäytävä]({{ api_page.url | relative_url }}#gateway), [apuri]({{ api_page.url | relative_url }}#allowlist-helper) |
| `legacy/UndauntedContent/` | Jakaa käynnistimelle pelitiedostot, uutiset ja kuvapaketin. | [Sisältöpalvelin]({{ config_page.url | relative_url }}#content-server) | [Sisältöpalvelin]({{ api_page.url | relative_url }}#content-server) |
| `legacy/UndauntedLauncher/` | Kavereiden käynnistin: kutsut, tiliavain, pelin lataus ja korjaus sekä julkisen tilan paikallinen välitin. | [Käynnistin]({{ config_page.url | relative_url }}#launcher) | [Käynnistimen välitin]({{ api_page.url | relative_url }}#launcher-relay) |
| `deploy/windows-server/` | Windows-palvelinpaketti: asentaa, ajaa, päivittää ja varmuuskopioi kaiken Windows Server 2019 -koneella. | [Mitä paketti kirjoittaa]({{ config_page.url | relative_url }}#server-kit) | [Paketin skriptit]({{ scripts_page.url | relative_url }}#windows-server-kit) |
| `friend-kit/` | PowerShell-skriptit kaverille, joka liittyy yksityisen tilan (Tailscale) palvelimelle ilman käynnistintä. | [Peliohjelman puoli]({{ config_page.url | relative_url }}#client-side) | [Kaveripaketin skriptit]({{ scripts_page.url | relative_url }}#friend-kit) |

Jokaisen palvelinosan kansiossa on kommentoitu `.env.example`, jossa on jokainen osan lukema muuttuja
oletusarvoineen. Kopioi se nimelle `.env` ja täytä tarvitsemasi kohdat. Sivu
[Asetukset]({{ config_page.url | relative_url }}) selittää jokaisen rivin.

## Merkinnät näillä sivuilla {#conventions-on-these-pages}

- **Salaisuudet** on merkitty salaisiksi: tiliavaimet (`UUK_...`), pelipalvelimen avain,
  allekirjoitusavaimet, yhdyskäytävän ja sallittujen listan salaisuudet sekä TLS-avain. Älä koskaan
  jaa niitä äläkä tallenna niitä versionhallintaan. Sivujen esimerkit ovat paikkamerkkejä.
- `C:\DauntlessRevived` on palvelinpaketin oletusasennuskansio. Sen paikalla lukee `<juuri>`, kun
  kansio voi olla muukin. `C:\dr\...`-polut ovat alkuperäisen palvelinkoneen kansioita, joita
  asennusohjeet käyttävät esimerkkinä; käytä omia kansioitasi.
- Esimerkkiosoitteet ovat dokumentaatiota varten varatuista osoitteista (`203.0.113.x`,
  `198.51.100.x`), ja `100.x.y.z` tarkoittaa Tailscale-osoitetta.
- ”Alkuperäinen projekti” tarkoittaa [Undauntedia](https://github.com/SyST3MDeV/Undaunted), jonka
  pohjalta tämä fork on tehty. Asetukset-sivulla **Vain forkissa** merkitsee asetuksia, joita
  alkuperäisessä projektissa ei ole.
