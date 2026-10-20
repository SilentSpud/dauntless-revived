---
title: Escalation
parent: Löydökset
grand_parent: Dauntless Revived suomeksi
nav_order: 12
lang: fi
ref: findings/escalation
locale: fi_FI
description: "Miten Dauntless 1.4.4 tallentaa Escalation-etenemisen ja Harmonicin forkin pohjalta rakennetut oikeat Escalation-tallennukset: peliohjelmasta luettu kausiluettelo, säännöt, jotka jokaisen tallennuksen on läpäistävä, ja ne, jotka vain varoittavat, vanha tynkä sekä se, mitä pelaajat näkevät, kun ominaisuus otetaan käyttöön."
---

{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign game_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign contract_page = site.pages | where: "path", "fi/findings/backend-contract.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "fi/findings/harmonic-fork.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}

# Escalation versiossa 1.4.4
{: .no_toc }

Escalationit ovat Dauntlessin monivaiheisia behemoth-ketjuja (versiossa 1.4.4 Shock, Blaze, Umbral ja
Terra). Jokaisella on oma kausietenemisensä: Escalation-taso, joka nousee pelaamalla,
kykypisteet, joita käytetään kauden kykyihin (talent), sekä palkinnot, jotka avautuvat tietyillä
tasoilla. Tämä sivu kertoo, miten 1.4.4-peli tallentaa tämän etenemisen, ja millaiset oikeat
tallennukset metagame (taustapalvelu) nyt tarjoaa.

**Tilanne 23.9.2026: rakennettu ja testattu ilman peliä, oletuksena pois päältä
(`ESCALATION_MODE=stub`).** Oletuksena jokainen pelaaja saa yhä alkuperäisen projektin tekaistun
maksimin, eikä mitään tallenneta, täsmälleen kuten ennenkin. Kun `ESCALATION_MODE=real`, oikean
etenemisen pelaajat säilyttävät oman Escalation-tasonsa, kykynsä ja palkintonsa. Käyttöönotosta päättää
ylläpitäjä pelitestin jälkeen, koska jokainen pelaaja aloittaa silloin alusta tasolta 0 (katso
[Käyttöönotto](#switching-it-on)).

Kausiluettelo, tallennussäännöt ja niiden testit ovat peräisin **Harmonicin** Dauntless 1.4.4
-forkista ([github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), muutos
`895f7c7`). Sovitimme ne omaan tapahtumalokiimme ja teimme epävarmemmista säännöistä varoituksia
hylkäysten sijaan; yksityiskohdat sivulla [Harmonicin työn siirto]({{ harmonic_page.url | relative_url }}).

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Todisteet ja varmuus {#evidence}

| Merkki | Lähde |
|:-------|:------|
| **B** | 1.4.4-ohjelmatiedosto (`Dauntless-Win64-Shipping.exe`): konekielinen koodi. Osoitteet ovat muistiosoitteita, joiden perusosoite on `0x140000000`. |
| **K** | Päätepisteiden taulukko tiedostossa `native/internal-server/dllmain.cpp` sekä peliohjelman omat datataulukot. |
| **S** | Vahva päätelmä: yksi ketjun lenkki on jäljittämättä. |
| **G** | Oma suunnitteluratkaisumme, kun peliohjelma ei ratkaise asiaa. |
| **C** | Oma koodimme ja testimme. |

## Näin peli tallentaa Escalationin {#contract}

Kaksi päätepistettä, molemmat samassa osoitteessa (K `dllmain.cpp`):

| Päätepiste | Pyyntö | Kuka kutsuu |
|:-----------|:-------|:------------|
| `GetSeasonalEscalationEndpoint` | `GET /escalation/<kauden tunnus>/<tilin tunnus>` | peliohjelma ja pelipalvelimet, kun ne lataavat pelaajan |
| `UpdateSeasonalEscalationEndpoint` | `POST /escalation/<kauden tunnus>/<tilin tunnus>` | pelipalvelin, runkona koko kausi |

Molemmat vastaukset luetaan Phoenixin kuoren `{"code", "message", "payload"}` kautta (B `0x140aae300`).
Sisältö (payload), samoin kuin tallennuksen runko, on yksi kausi:

```json
{
  "escalation_level": 3,
  "next_level_xp": 140,
  "talents_progress": [ { "rank": 1, "talent_id": "ESC_TALENT_S1_TIER1_UPGRADEONE" } ],
  "unlock_progress": [ { "collected": true, "reward_id": "ESC_Reward_5" } ],
  "update_version": 7
}
```

- `escalation_level` on saavutettu taso (0–25) ja `next_level_xp` seuraavaa tasoa varten kerätyt
  kokemuspisteet (XP).
- **Pelipalvelin laskee kaiken** (S, Harmonicin tulkinta pelin omasta koodista): se lisää kokemuksen,
  nostaa tason, antaa pelaajan käyttää pisteitä ja jakaa palkinnot itse, ja lähettää sitten koko
  kauden. Taustapalvelu vain säilyttää viimeisimmän hyvän tilannekuvan ja hylkää sellaisen, jota mikään
  oikea pelaaminen ei voisi tuottaa tai joka pyyhkisi pois uudemman etenemisen.
- `update_version` on laskuri, jota peli kasvattaa ennen jokaista tallennusta, alkaen lataamastaan
  arvosta (S), joten oikea tallennus on vähintään versio 1.
- 2.1.1-peliohjelma pyytää kaudet `ESC_SEASON_1`–`ESC_SEASON_6` jokaisella kaupungin latauksella
  ([Taustapalvelun rajapinta]({{ contract_page.url | relative_url }})); mitä 1.4.4 pyytää, näkyy
  pyyntölokista.

## Kausiluettelo {#registry}

`legacy/UndauntedMetagame/src/vendor/escalation/seasons.json` luettelee jokaisen kauden, joka 1.4.4-peliohjelmassa
on. Harmonic vei sen vain lukien peliohjelman omasta Escalation-etenemistaulukosta ja kunkin kauden
nimeämästä kykytaulukosta (versio CL239827). Metagame tarkistaa tiedoston käynnistyessään, ja
virheellinen tiedosto pysäyttää käynnistyksen.

| Kausi | Nimi | Tallennetaan |
|:------|:-----|:-------------|
| `ESC_SEASON_1` | Shock Escalation | kyllä |
| `ESC_SEASON_2` | Blaze Escalation | kyllä |
| `ESC_SEASON_3` | Umbral Escalation | kyllä |
| `ESC_SEASON_4` | Terra Escalation | kyllä |
| `ESC_SEASON_5` | Frost Escalation, jonka peliohjelma merkitsee "ei käännetä" (keskeneräinen) | ei: pois käytöstä, jokainen tallennus saa vastauksen 409 |

Jokaisessa kaudessa on:

- **25 tasoa.** Taso 1 maksaa 500 XP:tä ja taso 25 10 000 XP:tä; kaikki 25 yhteensä 109 000 XP:tä. Uusi
  pelaaja on tasolla 0.
- **18 kykyä kuudessa kolmen kyvyn portaassa.** Porras aukeaa, kun sen alapuolisiin portaisiin on
  käytetty 0, 4, 8, 12, 16 tai 20 pistettä, ja kyvyn jokainen aste maksaa pisteitä (yksi piste tulee
  tasoa kohden). Asteen hinta on kyvyn ensimmäisten asteiden hintojen summa, kuten pelin oma
  `GetSpentTalentPoints` sen laskee (B `0x1414ae2a0`), ja asteen nosto hylätään, jos portaan raja ei
  täyty (`SharedUpgradeTalent`, B `0x1414c0600`).
- **6 palkintoa** tasoilla 5, 8, 10, 15, 20 ja 25: ominaisuustehosteita, partioarkun bonus, ylimääräinen
  reliikkivalinta ja tasolla 25 tavara.

Näitä arvoja käytetään vain tallennuksen tarkistamiseen. Niillä ei koskaan lasketa pelaajan etenemistä,
se on pelipalvelimen tehtävä. Tasotaulukko vastaa omaa aiempaa tulkintaamme peliohjelmasta (C).

## Säännöt, jotka tallennuksen on läpäistävä {#rules}

Jokainen tallennus tarkistetaan yhtenä tietokantatapahtumana. **Kovia sääntöjä** valvotaan aina:

| Sääntö | Vastaus, jos sääntö rikkoutuu |
|:-------|:------------------------------|
| Tili on olemassa | 404 |
| Kausi on luettelossa | 404 |
| Kausi ei ole pois käytöstä (Frost) | 409 |
| `escalation_level` on kokonaisluku 0–25, `next_level_xp` kokonaisluku nollasta ylöspäin, `update_version` ykkösestä ylöspäin, kaikki 32 bitin rajoissa | 400 |
| Jokainen kyvyn ja palkinnon tunnus kuuluu kyseiseen kauteen, mikään kahdesti; kyvyn aste on enintään sen asteiden määrä; `collected` on tosi tai epätosi | 400 |
| **Versiojärjestys:** tallennettu versio tallennetulla sisällöllä on uusinta, ja se saa tallennetun tilan takaisin kirjoittamatta mitään; tallennettu versio eri sisällöllä tai vanhempi versio hylätään | 200 (uusinta), 409 |
| Mikään ei laske: alempi taso tai vähemmän XP:tä samalla tasolla | 409 |
| Kerätty palkinto pysyy kerättynä | 409 |
| **Tynkävahti:** tallennus ei saa olla tasolla 25 ja vähintään 99 999 XP:tä, ellei tallennettu kausi ole jo tasolla 25: ei pelaajan ensimmäinen tallennus eikä hyppy alemmalta tallennetulta tasolta. Sellaiset arvot pelipalvelimella on, jos se latasi vanhan tyngän, eivätkä ne saa koskaan muuttua pelaajan oikeaksi etenemiseksi. Sellainen pelipalvelin kasvattaa versiotaan, vaikka sen tallennukset saavat tynkätilassa vastauksen 404, joten tilan palattua arvoon `real` sen versio voi olla tallennettua uudempi; vahti hylkää sen myös tallennetun kauden päälle. Vain valmiiksi tasolle 25 tallennettu pelaaja kerää noin paljon XP:tä. | 409 |

Kykyjen asteet saavat laskea: kykyjen nollaus on tavallinen pelin toiminto, joten tallennetut kyvyt
korvataan sillä, mitä tallennus luettelee.

**Pehmeät säännöt** riippuvat siitä, miten mallinsimme kykyportaat, tasojen hinnat ja palkintojen
tasot. Väärin mallinnettu sääntö hylkäisi pelaajan jokaisen myöhemmän tallennuksen, joten oletuksena
ne vain varoittavat:

| Pehmeä sääntö | Oletus (`ESCALATION_STRICT` pois) | `ESCALATION_STRICT=1` |
|:--------------|:----------------------------------|:----------------------|
| `next_level_xp` on pienempi kuin seuraavan tason hinta | tallennetaan, varoitusrivi lokiin ja merkintä tapahtumariviin | 409 |
| Kyvyt käyttävät enintään niin monta pistettä kuin taso on antanut | sama | 409 |
| Kyky on hallussa vain, jos sen portaan raja täyttyy alempiin portaisiin käytetyillä pisteillä | sama | 409 |
| Palkinto kerätään vasta sen tasolla tai sen yläpuolella | sama | 409 |

Tallennettu tila on vakiomuotoinen: kyvyt ja palkinnot kauden omassa järjestyksessä, asteen 0 kyvyt
pois jätettyinä ja vain kerätyt palkinnot mukana. Sama tila luetaan siis aina takaisin samalla tavalla.

**Jokainen tallennus kirjataan.** Jokainen `POST` oikeassa tilassa, hyväksytty, toistettu tai
hylätty, on rivi taulussa `progression_events` (sama vain lisäyksiä hyväksyvä loki kuin Hunt Passilla
ja mestaruudella) kutsujan, rungon, vastauksen ja merkinnän kanssa: `season <tunnus>` ja sitten
`replay of version N, nothing changed`, `accepted although ... (ESCALATION_STRICT=0)` tai hylkäyksen
syy.

**Kuka saa tallentaa.** Vain pelipalvelin (sen avaimella, tältä koneelta): pelaajan oma peliohjelma saa
vastauksen 403, joten kukaan ei voi kirjoittaa omaa Escalationiaan. Jos pelipalvelimen pyynnössä on
toisen pelaajan tunniste kuin osoitteen tili, tallennus pidetään osoitteen tilille ja lokiin kirjataan
varoitus. Sitä ei koskaan hylätä, koska hylkäys voisi hukata tallennuksen kahden pelaajan metsästyksessä
(G). Pelaaja saa lukea vain omat kautensa; pelipalvelin lukee kenen tahansa.

## Tynkä ja oikea {#modes}

| | `ESCALATION_MODE=stub` (oletus) | `ESCALATION_MODE=real` |
|:-|:--------------------------------|:-----------------------|
| `GET` | Kaikki: `{"code": null, "message": "OK", "payload": {"escalation_level": 99999, "next_level_xp": 99999, "talents_progress": [], "unlock_progress": [], "update_version": 1}}`. Peli näyttää viimeisen tason. | Oikean etenemisen tilit: tallennettu kausi tai taso 0 ja versio 0, jos mitään ei ole vielä tallennettu (luku ei luo riviä). Tynkäetenemisen tilit saavat edelleen tyngän. Tuntematon kausi: 404. |
| `POST` | 404, kuten aina: mitään ei tallenneta. | Tallennetaan yllä olevien sääntöjen mukaan. |

Pelaajan tallennetut kaudet pysyvät tauluissaan, kun tila palaa arvoon `stub`, ja tulevat takaisin, kun
tila on taas `real`. Käynnistä pelipalvelimet uudelleen yhdessä metagamen kanssa jokaisella
vaihdolla, kumpaankin suuntaan (alla kohta 3).

## Käyttöönotto {#switching-it-on}

**Mitä pelaajat näkevät:** jokainen oikean etenemisen pelaaja putoaa tekaistusta maksimista (taso 25 ja
25 kykypistettä) tasolle 0 ja nousee sitten oikeasti. Kerro heille etukäteen.

1. Ota varmuuskopio (palvelinpaketti ottaa sellaisen jokaisella käynnistyksellä).
2. Aseta metagamen asetuksiin `ESCALATION_MODE=real`.
3. Käynnistä pelipalvelimet uudelleen yhdessä metagamen kanssa, kun kukaan ei pelaa, jotta mikään
   pelipalvelin ei pidä enää tyngän arvoja eikä kirjoita niitä takaisin. Tynkävahti hylkää sellaisen
   tallennuksen joka tapauksessa, myös tallennetun kauden päälle, mutta silloin pelaajan
   Escalation-eteneminen torjutaan samalla koko istunnon ajan, kunnes pelipalvelin lataa kauden
   uudelleen. Sama uudelleenkäynnistys tarvitaan, kun tila palaa arvoon `stub` tai kun tili poistuu
   asetuksesta `PROGRESSION_REAL_ACCOUNTS` ja palaa myöhemmin.
4. Pelitesti (tiekartan kohta 2.16): pelaa Escalation-kierros, kirjaudu uudelleen ja käytä kykypiste.
   Taso nousee ja säilyy uudelleenkirjautumisen yli, pisteitä näkyy oikea määrä, eikä lokissa ole
   vastausta 409 eikä riviä "breaks a soft rule".
5. Kun lokit pysyvät puhtaina jonkin aikaa, `ESCALATION_STRICT=1` muuttaa pehmeät säännöt hylkäyksiksi.

Kun testi on läpäisty, suunnitelmana on, että asettamaton `ESCALATION_MODE` seuraa asetusta
`PROGRESSION_MODE`.

Lokirivit ovat sivulla [Vianetsintä]({{ trouble_page.url | relative_url }}#log-lines-of-the-port),
asetukset sivulla [Asetukset]({{ config_page.url | relative_url }}#metagame-escalation), reitit sivulla
[HTTP-rajapinta]({{ api_page.url | relative_url }}#progression-hunt-pass-entitlements-cooldowns-and-bounties),
kolme taulua sivulla [Tiedostot ja data]({{ files_page.url | relative_url }}#escalation-and-store-tables)
ja luettelotiedosto sivulla [Pelin asetukset]({{ game_page.url | relative_url }}#escalation-seasons).

## Testit {#tests}

`legacy/UndauntedMetagame/test/escalation.test.ts` sisältää 27 tapausta: Harmonicin 17 (kausiluettelo,
luvut, versiosäännöt, kovat ja pehmeät säännöt, tynkävahti) meidän vastauksiimme sovitettuina (toisen
tilin luku on 403, uusinta on kirjattu 200, toisen tilin välitetty tunniste kirjataan ja hyväksytään,
pehmeät säännöt vastaavat 409 vain, kun `ESCALATION_STRICT=1`, tiukka XP-tapaus nousee tasolle 25
ennen kuin sen XP kasvaa), jokaisen pehmeän säännön varoitusversio, tynkävahti tallennetun kauden
päälle, tynkätila kaikille, tynkätilan tilit kun `ESCALATION_MODE=real`, pelaajan oma
tallennus (403, mitään ei tallenneta), tuntematon tili sekä tapahtumarivi.

## Vielä auki {#open}

| Avoin kysymys | Merkki | Miten se ratkeaa |
|:--------------|:-------|:-----------------|
| Pelipalvelin tekee kaikki Escalationin laskut ja lähettää koko kauden | S | Pelitesti: tallennukset saapuvat eikä mitään hylätä. |
| Mitä kausitunnuksia 1.4.4 pyytää (2.1.1 pyytää kauteen `ESC_SEASON_6` asti, jota luettelossa ei ole) | tuntematon | Yhden kaupunkilatauksen pyyntöloki oikeassa tilassa. |
| Pehmeät säännöt vastaavat peliä | S porrasrajalle ja pistemäärälle (B), G muille | Testissä ei riviä "breaks a soft rule". |
| Milloin ominaisuus otetaan käyttöön | ylläpitäjän päätös | Testin ja ilmoituksen jälkeen. |
