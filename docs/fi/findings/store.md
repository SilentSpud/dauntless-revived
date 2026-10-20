---
title: Pelin kauppa
parent: Löydökset
grand_parent: Dauntless Revived suomeksi
nav_order: 11
lang: fi
ref: findings/store
locale: fi_FI
description: "Miten Dauntless 1.4.4 -peliohjelman kauppa ostaa tavaran, luettuna ohjelmatiedostosta, ja Harmonicin forkin pohjalta rakennettu ilmainen kauppamme: neljä reittiä ja ostotunniste, valikoima ja sen välilehdet, pinottavat ja yksilölliset tavarat, kuka hahmo ostoksen saa ja asetukset."
---

{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign game_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign contract_page = site.pages | where: "path", "fi/findings/backend-contract.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "fi/findings/harmonic-fork.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}
{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}

# Pelin kauppa versiossa 1.4.4
{: .no_toc }

Tämä sivu kertoo, miten **1.4.4**-peliohjelman kauppanäkymä näyttää tarjoukset ja ostaa niistä yhden,
ja millaisella kaupalla metagame (taustapalvelu) nyt vastaa. Alkuperäinen kauppa myi kosmeettisia
tavaroita Platinumilla, jota ostettiin oikealla rahalla. Sitä kauppaa ei saa enää takaisin. Meidän
kauppamme on **ilmainen**: jokainen tarjous maksaa nolla, ja osto vain avaa tavaran käyttöön.

**Tilanne 23.9.2026: rakennettu ja testattu ilman peliä, oletuksena pois päältä (`STORE=off`).**
Ennen kuin kauppa otetaan käyttöön, kaksi asiaa on auki: ylläpitäjän päätös siitä, pysyykö kauppa
ilmaisena vai saako se hinnat pelin omassa valuutassa (tiekartan kohta 3.7), sekä kaupan testi pelissä
omalla palvelimellamme. Kun `STORE=off`, kauppanäkymä saa saman virheen kuin ennenkin.

Valikoima, kaksivaiheinen ostotunniste, välilehtien järjestys ja luettelo siitä, miten kukin tavara
annetaan, ovat peräisin **Harmonicin** Dauntless 1.4.4 -forkista
([github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), muutos `895f7c7`),
jossa hän kokeili kauppaa pelissä. Rakensimme reitit omien tavara-, oikeus- ja lokikoodiemme päälle.
Mitä otimme ja mitä muutimme, kerrotaan sivulla [Harmonicin työn siirto]({{ harmonic_page.url | relative_url }}).

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Todisteet ja varmuus {#evidence}

| Merkki | Lähde |
|:-------|:------|
| **B** | 1.4.4-ohjelmatiedosto (`Dauntless-Win64-Shipping.exe`): merkkijonot ja konekielinen koodi. Osoitteet ovat muistiosoitteita, joiden perusosoite on `0x140000000`. |
| **K** | Päätepisteiden taulukko tiedostossa `native/internal-server/dllmain.cpp` sekä peliohjelman omat datataulukot. |
| **O** | Harmonic näki tämän pelissä omalla forkillaan (yksi peliohjelma), ei vielä meidän palvelimellamme. |
| **S** | Vahva päätelmä: yksi ketjun lenkki on jäljittämättä. |
| **G** | Oma suunnitteluratkaisumme, kun peliohjelma ei ratkaise asiaa. |
| **C** | Oma koodimme ja testimme. |

## Näin peliohjelma ostaa tavaran {#flow}

Kauppa käyttää neljää päätepistettä (päätepiste on verkko-osoite, johon peli lähettää tietyn pyynnön).
Palvelin-DLL ohjaa ne kaikki jo metagamelle (K `dllmain.cpp`, `Store*`-avaimet), joten DLL:ää ei
tarvinnut muuttaa.

| Vaihe | Päätepiste | Pyyntö | Vastaus, jonka peliohjelma lukee |
|:------|:-----------|:-------|:---------------------------------|
| 1. Kaupan sivu aukeaa | `StoreGetItemByTagEndpoint` | `GET /product/skus/public?requiredTags=<tunniste>` | **Pelkkä taulukko** tarjouksia (ei kuorta). Kauppanäkymä pyytää tunnistetta `webstore`; versiossa 1.4.4 nähdyt tunnisteet ovat kohdassa [Valikoima](#catalogue). |
| 2. Ostoikkuna aukeaa | `StoreGetItemByIdEndpoint` | `GET /product/sku/<tarjouksen tunnus>` | Yksi tarjous. |
| 3. Osta-painike | `StorePurchaseItemEndpoint` | `GET /token/<valuutta>/<tarjouksen tunnus>` | `{"purchaseToken": "<ostotunniste>"}` |
| 4. Vahvistus | `StorePurchaseItemConfirmEndpoint` | `POST /notification/<valuutta>?token=<ostotunniste>` | Mikä tahansa 2xx; me vastaamme 204 ilman sisältöä. |

- Ostokoodi tekee kaksi ostokutsua tässä järjestyksessä (B `0x140b39657` tunnisteelle ja `0x140b55011`
  vahvistukselle). Se, että `POST` on vahvistus eli hetki, jolloin osto tapahtuu, on vahva päätelmä (S):
  sen jälkeen ei tule muuta.
- Polun valuutta on `platinum` jokaisessa myymässämme tarjouksessa (S: peliohjelma valitsee sen
  tarjouksen hintakentistä; muu valuutta torjutaan vastauksella 400).
- Tarjouksen hintakentät ovat **litteitä**: `platinumPrice`, `platinumSalePrice`, `cellDustPrice`,
  `prestigePrice`, `event01Price`, `steelMarksPrice` ja `gildedMarksPrice` (B, kenttien nimet
  1.4.4-ohjelmatiedostossa, Harmonicin lukemina). Myöhempien versioiden `prices: [{currencyId, price}]`
  -taulukolle 1.4.4:ssä ei ole lukijaa, joten sellaisella tarjouksella ei ole hintaa, jonka peliohjelma
  tunnistaisi, ja se jätetään pois (S, Harmonicin muistiinpanoista: kauppa jää tyhjäksi).
- `remaining` kertoo peliohjelmalle, voiko tarjouksen vielä ostaa. Lähetämme 0, kun hahmolla on jo
  jokainen tarjouksen tavara ja tilillä jokainen oikeus (entitlement), jonka tarjous antaa.

Näkyykö uusi tavara heti vai vasta seuraavalla tavaroiden luvulla (uudelleenkirjautuminen tai kartan
lataus), selviää pelitestissä.

## Valikoima {#catalogue}

Tarjoukset ovat tiedostossa `UndauntedMetagame/src/vendor/store_catalog.json` sen tunnisteen alla,
jota peliohjelma pyytää. Mikään peliohjelman lähettämä ei ratkaise, mitä tarjous antaa tai maksaa:
metagame hakee tarjouksen joka kerta sen tunnuksella.

| Tunniste | Tarjouksia | Mitä |
|:---------|:-----------|:-----|
| `webstore` | 200 | Kauppanäkymä: panssarit, aseiden ulkoasut, lyhdyt, eleet ja saapumisanimaatiot, soihdut ja sinetit, lipun kankaat ja tangot, värit, kiillot (sheen) ja hiusten sävyt sekä palkkiotehtävien tunnisteiden paketti (piilotettu, katso alta). |
| `season09b_pass` | 1 | `season09b_premium` eli Elite Hunt Pass. Jokaisella tilillä on se jo (`ENTITLEMENTS_DEFAULT`), joten se näkyy omistettuna. |
| `season09b_rank` | 0 | Hunt Passin tasohypyt. Vastaus on tyhjä: tasohyppy olisi etenemisen myöntö eikä tavara (katso alta). |
| `loadout_slots`, `fountain_daily_free_bundle` | 0 | Tunnisteita, joita 1.4.4-peliohjelma pyytää; niiden alla ei myydä mitään. |

Nämä viisi ovat tunnisteet, joita Harmonic näki 1.4.4-peliohjelman pyytävän (O). Muu tunniste saa
tyhjän listan ja varoitusrivin lokiin. Jokaisen tarjouksen `platinumPrice` on 0. Muutama asia
valikoimasta, Harmonicin työn pohjalta:

- **Kaupan välilehdet.** Jokaisella tarjouksella on tunnisteen `webstore` lisäksi luokkatunnisteita
  (`skin_armour`, `feature`, `social_emote`, `personality_fabric` ja niin edelleen). Kaupan
  välilehtipalkki numeroi kaikki välilehdet, myös piilotetut, joten tyhjä välilehti täytetyn edellä
  siirtää kaikkien myöhempien välilehtien sisällön (O: EMOTES-välilehti avasi aseet). Siksi valikoima
  antaa välilehdille `your_offers`, `supplies_boost`, `personality_stylekit` ja `personality_character`
  vähintään yhden tarjouksen kullekin.
- **Ruutujen kuvat.** Peliohjelma piirtää tarjousruudun omasta kauppatavarataulukostaan
  (`StoreItemsTable`, 1 405 riviä tarjouksen tunnuksen mukaan). Tarjous, jonka tunnus ei ole kuvallinen
  rivi, näyttää varakuvan, ja ruudut ovat kiinteästi 2:1-muotoisia, joten neliönmuotoiset Hunt Pass
  -kuvakkeet venyvät. Siksi kauppa myy vain tunnuksia, joiden ruutukuva on 2:1, ja testi pitää jokaisen
  tarjouksen tällaisten tunnusten luettelossa (`UndauntedMetagame/test/data/store_art_skus.json`, 994
  tunnusta, ei kuvia).
- **Kiillot ja hiusten sävyt ovat oikeuksia** eivätkä tavaroita. Niiden tarjous- ja oikeusparit tulevat
  peliohjelman omista taulukoista (värien kiiltotaulukko ja premium-hiusvärien taulukko): viisi kiiltoa
  (`ent_dye_sheen_glossy` ja neljä muuta) ja neljä hiusväritarjousta.
- **Väripalettipaketteja ei myydä.** Peliohjelma määrittää niiden sarjat, mutta ei anna niille yhtään
  väriä, joten niiden sisältö oli vain vanhoilla palvelimilla.

Luettelossa on peliohjelmasta luettuja tunnisteita ja lyhyitä englanninkielisiä nimiä eikä lainkaan
pelin sisältöä (grafiikkaa tai muuta); katso [Kiitokset ja lisenssi]({{ '/fi/legal.html' | relative_url }}#no-game-files).

## Pinottavat ja yksilölliset tavarat {#grants}

Tavara tulee tavaraluetteloon (inventaario) jommallakummalla tavalla: **pinona** (tavaran tunnus ja
määrä) tai **yksilönä** (tavara, jolla on oma yksilötunnuksensa). Peliohjelman tavaraluettelo
ratkaisee, kumpi, kunkin tavaran pinottavuusmerkinnän kautta, ja väärä laji jättää tavaran
näkymättömäksi tai rikkoo tavaraluettelon. `UndauntedMetagame/src/vendor/store_item_kinds.json` kirjaa
merkinnän kaikille 211 kaupan tavaralle (95 pinottavaa, 116 yksilöllistä), Harmonicin peliohjelman
tavaraluettelosta lukemana. Nimen alkuun perustuva sääntö menisi yksittäisissä tavaroissa pieleen:
useimmat aseiden ulkoasut ja muutama saapumisanimaatio ovat yksilöllisiä, kun taas muutama kangas ja
lyhty on pinottava.

- **Pinottavan panssarin nähtiin toimivan pelissä** (O). Kauppa myy panssaria vain siellä, missä se on
  pinottavaa.
- **Yksilöllisiä tavaroita ei ole todistettu pelissä**, ei edes Harmonicin forkissa. Yksilötunnus on
  `sha256(<tunnisteen tiiviste>:<tavaran tunnus>)` 32 heksamerkkiin lyhennettynä, joten toistettu
  vahvistus nimeää saman yksilön. Pelitestissä ostetaan yksi yksilöllinen aseen ulkoasu ja katsotaan,
  että se näkyy ja sen voi ottaa käyttöön uudelleenkirjautumisen jälkeen.
- Vain tällaisia tavaroita annetaan koskaan, luetteli tarjous mitä tahansa: `AR_` `WP_` `EM_` `DYE_`
  `QI_FLARE_` `BNC_FABRIC_` `BNC_STANDARD_` `BNC_SIGIL_` `LT_`, yksi kutakin, sekä palkkiotehtävien
  tunnisteiden paketti. Tarjous, jossa on muuta (valuuttaa, tehostetta tai oikea ase), hylätään (G).

## Kuka hahmo ostoksen saa {#character}

Ostoreitit eivät nimeä hahmoa, ja palvelimellamme tilillä voi olla useita hahmoja (jokainen
`POST /character` luo yhden). Metagame sitoo ostotunnisteen tilin **aktiiviseen hahmoon**, kun
tunniste annetaan: siihen hahmoon, jonka tiedot on tallennettu viimeksi (peli tallentaa pelattavan
hahmon noin kerran minuutissa, ja jokainen tallennus on rivi taulussa `characterhistory`). Jos
tallennettuja versioita ei ole, ratkaisee myöhempi `lastModifiedDate` ja sitten suurempi
`updateVersion`. Se, että tämä on ruudulla oleva hahmo, on vahva päätelmä (S). Tili, jolla ei ole
hahmoa, saa tunnisteesta vastauksen 409, ja vahvistus hylätään (403), jos tunnisteen hahmo ei enää
kuulu tilille.

Harmonicin versio hylkäsi tilit, joilla on useampi hahmo. Ennen kuin `STORE=free` otetaan käyttöön
oikeille pelaajille, ylläpitäjä laskee hahmojen määrän tiliä kohden elävästä tietokannasta (vain lukeva
kysely on sivulla [Palvelin ryhmälle]({{ admin_page.url | relative_url }}#switching-features-on));
sitä, monellako tilillä on useampi hahmo, ei ole vielä laskettu.

## Osto vaihe vaiheelta {#redeem}

1. **Tunniste.** `GET /token/platinum/<tarjous>` tarkistaa, että tarjous on olemassa, ilmainen ja
   antaa vain sallittuja asioita, ettei pelaajalla ole jo kaikkea, mitä se antaa (409; palkkiotunnisteiden
   nippu ei ole koskaan omistettu), ja että tili on saanut alle 60 tunnistetta viimeisten 10 minuutin
   aikana (409), ja tallentaa sitten rivin tauluun `storepurchases`: satunnaisen 64-heksamerkkisen
   tunnisteen SHA-256-tiivisteen (itse tunnistetta ei tallenneta koskaan), tilin, aktiivisen hahmon,
   tarjouksen tunnuksen, tiivisteen tarjouksesta sellaisena kuin se nyt on, ja vanhenemisajan 10
   minuutin päähän. Tilin omat lunastamattomat, vanhentuneet tunnisteet poistetaan, kun se saa uuden;
   koko taulu siivotaan jokaisella käynnistyksellä ja sen jälkeen enintään kerran tunnissa (vanhentuneet,
   lunastamattomat tunnisteet sekä yli 30 päivää sitten lunastettujen kuitit).
2. **Vahvistus.** `POST /notification/platinum?token=<tunniste>` tehdään yhtenä tietokantatapahtumana:
   - toisen tilin tai tuntematon tunniste: 403; tunniste, jonka hahmo on siirtynyt: 403; vanhentunut
     tunniste: 410; tarjous, joka on muuttunut tunnisteen jälkeen tai jota ei enää myydä: 409;
   - **jo lunastettu** tunniste: taas 204, eikä mitään anneta (uusinta kadonneen vastauksen jälkeen);
   - tavarat kulkevat saman tavarakoodin kautta kuin `POST /inventory`, kutsujana `store`, lähteenä
     `store:<tarjous>` ja tapahtuman tunnuksena `store:<tunnisteen tiiviste>`: yksi rivi tapahtumien
     kirjanpidossa ja yksi `inventorylog`-rivi tavaraa kohden. Mitä hahmolla jo on, sitä ei anneta
     uudelleen;
   - oikeudet kulkevat saman koodin kautta kuin pelipalvelimen omat myönnöt, lähteenä `store:<tarjous>`;
   - tunnisteen rivi merkitään lunastetuksi. Jos jokin epäonnistuu, mitään siitä ei jää voimaan. Rivi on
     kuitti, jolla uusittuun vahvistukseen vastataan; se poistetaan 30 päivää lunastuksen jälkeen, ja
     annettu säilyy tauluissa `inventorylog` (säilytetään aina) ja `entitlements`.
3. **Omistus.** Tarjous näyttää `remaining: 0`, kun jokainen tavara on hahmolla ja jokainen oikeus on
   voimassa. Peruttu tai vanhentunut oikeus voidaan siis ostaa uudelleen, ja Elite-passi näkyy
   omistettuna oletusoikeuksien kautta.

Jokainen reitti toimii sen pelaajan puolesta, jonka tunniste pyynnössä on: ilman tunnistetta 401, ja
pelkkä pelipalvelimen avain saa 403. Ostot voi jäljittää (`inventorylog.caller = 'store'`,
`entitlements.source LIKE 'store:%'`) ja perua nykyisillä ylläpitoreiteillä
([HTTP-rajapinta]({{ api_page.url | relative_url }}#undaunted-api)).

## Mitä jätimme pois ja miksi {#left-out}

| Pois jätetty | Miksi |
|:-------------|:------|
| Harmonicin `season09b_10_ranks`-tarjous (kymmenen Hunt Pass -tasoa) | Se antoi `CURRENCY_PRESTIGE`-valuuttaa, jota hänen oma kauppakoodinsa ei pystynyt myymään (ei myöntölajia: 409). Oikea tasohyppy on etenemisen myöntö tasosääntöjen kautta, ei tavara. |
| Hinnat | Jokainen tarjous on ilmainen, kunnes ylläpitäjä päättää toisin (3.7). Koodi hylkää tarjouksen, jonka `platinumPrice` ei ole 0. |
| Palkkiotehtävien tunnisteiden paketti, oletuksena | `bundle_currency_bounty_small` antaa 20 `TOKEN_BOUNTY_DRAFT_PREMIUM`-tunnistetta (premium-palkkiotehtävien tunniste, joka säilyy seuraavaan kauteen), ja sen voi ostaa kuinka monta kertaa tahansa: rajattomasti ilmaisia premium-palkkiotehtäviä. Se on listalla vain, kun `STORE_REPEATABLE_TOKENS=1`, ja siitä päättää ylläpitäjä. Tunniste, joka annettiin asetuksen ollessa päällä, hylätään (409), kun asetus on pois. |
| Trials-, tapahtuma- ja prestige-kaupat, kennopölyn vaihto, varustesarjapaikkojen ostot | Sama palvelu pyöritti niitä; niille ei ole vielä rakennettu mitään (3.7, 3.9). |

## Asetukset {#switches}

| Asetus | Oletus | Mitä se tekee |
|:-------|:-------|:--------------|
| `STORE` | `off` | `off`: kauppanäkymä saa vanhan vastauksen 400 (`{"code": "400", "message": "The store is not available on Dauntless Revived yet."}`) ja kolme ostoreittiä vastauksen 404, jonka ne ovat aina saaneet. `free`: yllä kuvattu valikoima, tunniste ja vahvistus. |
| `STORE_REPEATABLE_TOKENS` | pois | `1` näyttää ja myy palkkiotehtävien tunnisteiden paketin rajattomasti. |

Tarkat kuvaukset ovat sivulla [Asetukset]({{ config_page.url | relative_url }}#metagame-store), reitit
sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#store), taulu sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}) ja valikoiman tiedostot sivulla
[Pelin asetukset]({{ game_page.url | relative_url }}#store-catalogue).

## Pelitesti ja avoimet kysymykset {#open}

Ennen kuin `STORE=free` tulee oletukseksi, testataan kertakäyttöisellä tilillä vuokrapalvelimella:

1. Ylläpitäjä päättää, onko kauppa ilmainen vai hinnoiteltu (3.7) ja myydäänkö tunnistepakettia.
2. Lasketaan hahmot tiliä kohden elävästä tietokannasta (osto menee viimeksi tallennetulle hahmolle).
3. Asetetaan `STORE=free` metagamen asetuksiin ja käynnistetään se uudelleen, kun kukaan ei pelaa.
4. Avataan jokainen välilehti: kukin näyttää omanlaisiaan tavaroita (EMOTES näyttää eleet).
5. Ostetaan yksi pinottava panssarin osa, yksi yksilöllinen aseen ulkoasu, yksi lyhty ja yksi kiilto
   (sekä tunnistepaketti, jos se on sallittu). Lokissa näkyy `Store purchase token for <tarjous> issued
   to <tili>` ja `Store purchase <tarjous> for <tili> (character <tunnus>): N item(s), M
   entitlement(s)`.
6. Kirjaudutaan uudelleen: kaikki ostettu näkyy ja sen voi ottaa käyttöön, ja tarjoukset näkyvät
   omistettuina.
7. Pelataan metsästys loppuun: sen jälkeen ei tule tavaroiden 409-virheitä.

| Avoin kysymys | Merkki |
|:--------------|:-------|
| `POST /notification` on vahvistus | S |
| Kaupasta ostetut yksilölliset tavarat näkyvät ja ne voi ottaa käyttöön | ei vielä nähty |
| Aktiivinen hahmo on ruudulla oleva hahmo | S |
| Tavara näkyy heti ilman uudelleenkirjautumista | ei vielä nähty |
| Ilmainen vai hinnoiteltu | ylläpitäjän päätös (3.7) |

Kaupan vaiheet ovat [tiekartalla]({{ roadmap_page.url | relative_url }}) (3.7), ja sivu
[Taustapalvelun rajapinta]({{ contract_page.url | relative_url }}) kuvaa muut palvelut, joiden kanssa
peliohjelma keskustelee.
