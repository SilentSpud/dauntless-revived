---
title: Harmonicin työn siirto
parent: Löydökset
grand_parent: Dauntless Revived suomeksi
nav_order: 13
lang: fi
ref: findings/harmonic-fork
locale: fi_FI
description: "Mitä otimme Harmonicin Dauntless 1.4.4 -forkista ja missä pidimme oman ratkaisumme, ominaisuus kerrallaan ja jokaisen valinnan syyn kanssa: Escalation, ilmainen kauppa, Slayer Links, deploy-palvelimen korjaukset, testit ja se, mitä jätimme pois."
---

{% assign store_page = site.pages | where: "path", "fi/findings/store.md" | first %}
{% assign escalation_page = site.pages | where: "path", "fi/findings/escalation.md" | first %}
{% assign social_page = site.pages | where: "path", "fi/findings/social.md" | first %}
{% assign chat_page = site.pages | where: "path", "fi/findings/chat.md" | first %}
{% assign contract_page = site.pages | where: "path", "fi/findings/backend-contract.md" | first %}
{% assign mp_page = site.pages | where: "path", "fi/findings/multiplayer.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}
{% assign legal_page = site.pages | where: "path", "fi/legal.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}

# Harmonicin työn siirto
{: .no_toc }

**Harmonic** ylläpitää toista Undaunted-haaraa (fork) Dauntless 1.4.4 -peliohjelmalle,
[github.com/Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), samalla lisenssillä
(AGPL-3.0-only). Syyskuussa 2026 hän julkaisi suuren muutoksen (`895f7c7`, "Preserve Dauntless 1.4.4
backend work"), jossa oli eteneminen, kauppa, Escalation, sosiaalisia ominaisuuksia ja 119
testitapausta. Ylläpitäjä pyysi meitä tuomaan Dauntless Revivediin kaiken hänen toimivan työnsä.

Molemmat haarat lähtevät samasta alkuperäisen projektin muutoksesta (`7f692aa`), ja molemmissa oli
samoina viikkoina rakennettu osin samoja asioita. Siksi vertasimme niitä ominaisuus kerrallaan. **Kun
hänen työnsä toi jotain, mitä meillä ei ollut, otimme sen**, lukuun ottamatta muutamaa alla syineen
lueteltua alustan kohtaa (DLL-puolen kohdat odottavat ylläpitäjän päätöstä itse käännetyn DLL:n
jakamisesta). **Kun hän oli kirjoittanut uudelleen jotain, minkä olimme jo rakentaneet ja kokeilleet
pelissä, pidimme omamme**, ja jokainen tällainen kohta alla kertoo syyn ja todisteet. Tämä sivu on tuo
kirjanpito, kirjoitettu lukijoille, jotka eivät tunne kumpaakaan koodia, Harmonic heidän joukossaan.

Kiitos, Harmonic: Escalation ja kauppa ovat palvelimellamme sinun työsi ansiosta.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Lyhyesti {#in-short}

**Mitä otimme:**

- **Escalation kokonaan:** hänen peliohjelmasta viemänsä kausiluettelo, tallennussäännöt ja niiden
  kaikki 17 testiä. Meillä oli vain tekaistu "taso 99999" -vastaus. [Escalation]({{ escalation_page.url | relative_url }})
- **Ilmainen kauppa:** hänen 200 ilmaisen tarjouksen valikoimansa, kaksivaiheinen ostotunniste,
  kaupan välilehdet ja luettelo siitä, miten kukin tavara annetaan. Meidän kauppanäkymämme sai virheen.
  [Pelin kauppa]({{ store_page.url | relative_url }})
- **Slayer Links:** hänen rajapintakuvauksensa, ensimmäinen toimiva, korjattuna niiltä osin kuin
  peliohjelma lähettää tai lukee toisenlaisen muodon.
  [Kaverit, ryhmät ja killat]({{ social_page.url | relative_url }}#slayer-links)
- **Deploy-palvelimen korjaukset:** kaatunut Ramsgate käynnistetään uudelleen, kun pelaaja matkustaa
  sinne, eikä puuttuva peliohjelma enää kaada deploy-palvelinta.
  [Näin moninpeli toimii]({{ mp_page.url | relative_url }}#the-deploy-server)
- **Pienempiä ideoita:** suoja toistettuja etenemismyöntöjä vastaan, Hunt Pass -kaudet levyllä olevasta
  kansiosta, oikea valuutta `/balance`-vastauksessa, pelaajan oma tilitunnus `oauth/verify`-vastauksessa,
  kahdesti saapuva ryhmäkutsun hyväksyntä, tila ja kesto pyyntörunkojen lokissa, MinHookin
  käännöskorjaus ja viisi `.gitignore`-sääntöä.
- **Testit:** 110 hänen 119 testitapauksestaan, sovitettuina meidän koodiimme; niistä 7 väittää nyt
  päinvastaista kuin hänen versionsa.
- **Kavereiden verkkotila:** idea, rakennettuna uudelleen meidän chat-palvelimeemme.

**Mitä emme ottaneet, yhdellä rivillä kukin:**

| Pois jätetty | Miksi |
|:-------------|:------|
| Hunt Passin ja mestaruuden palkintojen maksu tason vahvistuksessa | 1.4.4-pelipalvelin maksaa ne jo reitin `/inventory` kautta; maksu myös vahvistuksessa maksaisi jokaisen palkinnon kahdesti. |
| Hänen lompakkotaulunsa | Valuutta on jo tavaraluettelossa pinoina. |
| Hänen tavaraluettelonsa ja varustesarjojensa uudelleenkirjoitukset | Ne kumoaisivat pelissä varmistamiamme korjauksia (metsästyksen palkinnot hukkaan ylikulutuksessa, nollamääräiset palkinnot hylättyinä, pelaajat lisäämässä varustesarjapaikkoja itse). |
| Hänen palkkiotehtävälistansa ja "täysi nollaus tyhjentää taulun" | Kultaiset palkkiotehtävät putoaisivat 100:sta 60 XP:hen, ja pelaajat menettäisivät palkkiotehtäviä, jotka heillä on yhä. |
| Hänen chat- ja verkkotilapalvelimensa | Se ei koskaan toimita huoneviestejä eikä kuiskauksia, ja se pohjautuu Mystic Paradoxiin, jonka lisälisenssiehdot joutuisimme silloin kantamaan. |
| Hänen ryhmäkohtainen jonotuksensa | Se tuo takaisin kahden pelaajan jumittumisen, jonka korjasimme 22. syyskuuta. |
| Ilman tunnistetta hyväksytty sydämenlyönti (heartbeat) | Kuka tahansa internetissä voisi avata pelimme portit. |
| Hänen muutoksensa palvelin-DLL:ään | Ne odottavat ylläpitäjän päätöstä jakaa itse käännetty DLL. |

## Taulukoiden sanat {#legend}

| Sana | Merkitys |
|:-----|:---------|
| **Otettu** | Otettu mukaan, kirjoitettu meidän koodityylillämme, usein pienin korjauksin. |
| **Yhdistetty** | Hänen ideansa tai osa hänen koodiaan, yhdistettynä meidän koodiimme. |
| **Pidetty omamme** | Tämä oli jo meillä rakennettuna ja kokeiltuna; hänen versiotaan ei oteta. |
| **Jätetty pois** | Ei otettu, annetusta syystä. |

Todisteiden merkit ovat samat kuin näillä sivuilla muutenkin: **B** 1.4.4-ohjelmatiedosto (osoitteet,
joiden perusosoite on `0x140000000`), **V** varmistettu pelissä palvelimellamme, **C** koodi, **S**
vahva päätelmä, **G** arvaus tai oma suunnitteluratkaisu.

## Alusta {#platform}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| Jonotus vastaa FAILED, kun maailmaa ei saada käyntiin | **Pidetty omamme**, hänen testinsä otettu | Omamme vastasi jo FAILED useammassa tilanteessa, kentän `statusReason: null` kanssa. Hänen virhetestinsä (500, 200 ilman palvelinta, lainausmerkeissä oleva portti, runko joka ei ole JSONia, katkennut yhteys) ajetaan nyt meidän koodiamme vastaan kolmella polulla: yksikään ei vaatinut koodimuutosta. |
| Ryhmäkohtainen jonotus | **Jätetty pois** | Se lähettää jäsenen vanhan metsästyksen palvelimelle, vie koko ryhmän yhden pelaajan opetusjaksoon ja tuo takaisin kahden pelaajan jumittumisen, jonka korjasimme 22. syyskuuta. Sen peruutus laukeaa `DELETE /party/member`-kutsusta, jonka peliohjelma lähettää jokaisella kirjautumisella. |
| Deploy-palvelin: uudelleenkäynnistetyn Ramsgaten ja Dojon tietue tallennetaan; prosessin elossaolo tarkistetaan ennen kuin sen osoite annetaan | **Yhdistetty** | Se korjaa vanhentuneen tietueen ja kaatuneen Ramsgaten, joka jäi huomaamatta jopa minuutiksi. Lisäsimme yhden yhteisen käynnistyksen, jotta vahtikoira ja pelaajan pyyntö eivät voi koskaan käynnistää kahta Ramsgatea UDP-porttiin 8777. Asetus `PERSISTENT_WORLD_LIVENESS`, päällä. |
| Deploy-palvelin: lapsiprosessin `error`- ja `exit`-tapahtumat kirjataan | **Otettu** | Ilman sitä väärä `GAMESERVER_BINARY_PATH` kaatoi deploy-palvelimen käsittelemättömään virheeseen. |
| Deploy-palvelin: epäonnistunut käynnistys kirjataan | **Otettu** | Yksi fatal-rivi ja nollasta poikkeava paluukoodi käsittelemättömän virheen sijaan. |
| Deploy-palvelin: odota pelin UDP-porttia ennen vastausta | **Jätetty toistaiseksi pois** | Se pitäisi pelaajan matkapyynnön auki koko palvelimen käynnistyksen ajan; peliohjelman aikaraja tälle kutsulle on tuntematon, ja 60 sekunnin takaraja voisi kaataa kylmiltään käynnistyvän Ramsgaten. Tiekartalla (4.6). |
| Deploy-palvelin: pelipalvelinten lokitiedostot (`GAMESERVER_LOG_DIR`) | **Jätetty pois** | DLL antaa pelimoottorille kiinteän komentorivin ja ohjaa sen tulosteen muualle, joten tiedostot jäisivät lähes tyhjiksi. Lokitiedostot kuuluvat oman DLL:n työhön (4.6). |
| Deploy-palvelin: valinnainen vahtikoira ja `HOST` | **Jätetty pois** | Kun vahtikoira on pois, metsästysten portit eivät palaa käyttöön, ja jokainen käynnistys epäonnistuu kuuden metsästyksen jälkeen. Meidän `BIND_HOST` tekee jo saman kuin `HOST`. |
| `METAGAME_ADDRESS`-argumentti ja päätepisteiden koukku palvelintilassa (deploy-palvelin ja DLL) | **Jätetty pois DLL-päätökseen asti** | Molemmat puolet on julkaistava yhdessä, ja meidän tuottamamme `Game.ini` hoitaa jo saman asian (V). Tiekartalla (4.4, 4.6). |
| HuntDiag, DLL:n vianetsintäloki | **Jätetty pois DLL-päätökseen asti** | Pelkkä vianetsintä. Sen lisääminen DLL-lähdekoodiimme, kun jaamme alkuperäisen projektin DLL:ää, tekisi lähdekoodista eri asian kuin sen, mitä jaamme. |
| MinHookin include-korjaus (`hook.c`) | **Otettu** | Pelkkä käännöskorjaus; DLL-projekti kääntyy nyt Visual Studio 2022 Build Toolsilla. Mitään siitä käännettyä ei jaeta. |
| Liikenteen tallennus (`WireCapture`) | **Yhdistetty `LOG_BODIES`-lokiin** | Pyyntörunkojen lokimme sai vastauksen tilan ja keston, valinnaisen rajan polkua kohden (`BODY_LOG_PER_PATH`), hänen lisäreittinsä sekä tiliavainten peittämisen. Toista tallennusasetusta ei tullut; palvelinpaketti pitää rungon lokin yhä pois päältä julkisessa tilassa. |
| Sydämenlyönti ilman tunnistetta | **Jätetty pois** | Yhdyskäytävämme avaa pelin UDP-portit jokaiselle osoitteelle, jonka sydämenlyöntiin vastataan 2xx; ilman tunnistetta kuka tahansa voisi avata ne. Testi vartioi tätä nyt. |
| Chat-pinon käynnistys aina; `clientError`-loki; käynnistysbanneri | **Jätetty pois** | Chatin käynnistäminen aina avaisi toisen, suojaamattoman chat-pinon. |
| Pakettitiedostot ja lukitustiedostot | **Pidetty omamme** | Omamme olivat jo ajan tasalla. |
| Viisi `.gitignore`-sääntöä (`*.bak`, `*.bak-*`, `*.drizzle-generated`, `*.sqlite`, `*.sqlite-*`) | **Otettu** | Harmittomia; mikään seurattu tiedosto ei osu niihin. |

## Tavaraluettelo ja varustesarjat {#inventory}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| Yksi tavaraluettelon ydin, joka toimii kutsujan tietokantatapahtuman sisällä | **Yhdistetty (malli)** | Kaupan on annettava tavarat oman tapahtumansa sisällä. Tavarakoodimme jaettiin samalla tavalla, ja kaikki 26 tavaraluettelotestiämme pysyivät muuttumattomina. |
| Hänen toistokirjanpitonsa, määräsääntönsä ja vastausten järjestys | **Pidetty omamme** | Omamme säilyttää pelissä varmistetut korjaukset (V): ylikulutus rajataan nollaan ja kirjataan hylkäämisen sijaan, nollamäärä hyväksytään, tapahtuma saa käyttää sen, minkä se antaa, ja jokainen muutos kirjataan. Hänen sääntönsä hukkaisivat koko metsästyksen palkinnot ylikulutuksessa ja hylkäisivät nollamääräiset mestaruuspalkinnot. Kaikki 11 hänen tavaraluettelotestiään ajetaan meidän koodiamme vastaan. |
| Varustesarjapaikat (enintään 8, määrä taulukon pituudesta, pelaaja avaa itse) | **Pidetty omamme** | Peliohjelma rajaa varustesarjat kuuteen ja sillä on omat reittinsä paikkojen määrälle ja aktiiviselle paikalle (B), eikä pelaaja saa avata paikkoja itse. |

## Eteneminen ja Hunt Pass {#progression}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| XP-myönnön vastaus (tyhjä; yksi huono merkintä kaataa koko myönnön; palvelimen oma kerroin) | **Pidetty omamme** | Tyhjä vastaus pysäyttää tasonnousut kesken istunnon, palkintojen maksun ja automaattisen vahvistuksen (B `0x141464c90`). Vastaus 400 hukkaa XP:n, koska pelipalvelin ei yritä uudelleen (B). Jokainen pelin mukana tullut kerroin on 1. |
| Palkintojen maksu tason vahvistuksessa (lunastuskirjanpito) | **Jätetty pois** | Se maksaa kahdesti. Pelipalvelin maksaa jo jokaisen tason reitin `/inventory` kautta (B `0x141472fa0`, `0x1414876a0`, `0x141481920`), ja näimme jokaisen palkinnon annettavan kerran (V, tiekartan kohta 2.10). Meidän tietokannassamme kirjanpito alkaisi tyhjänä ja maksaisi uudelleen jokaisen tason, jonka pelaajat ovat jo vahvistaneet, premium mukaan lukien. Testi vartioi, ettei vahvistus anna mitään. |
| Vahvistus antaa tason pysyvät oikeudet | **Yhdistetty, pois päältä** | Vain pelin etenemismyöntö nimeää oikeuden myönnön (B `0x141487aa5`); tulevatko Elite-tasojen kosmeettiset tavarat itsestään, on auki. Rakennettu asetuksen `PROGRESSION_CONFIRM_ENTITLEMENTS` (pois) taakse pelitestin ratkaistavaksi. Se ei koskaan anna tavaroita eikä määräaikaisia tehosteita. |
| Tasolaskenta, joka loppuu tasoon 50 | **Pidetty omamme** | season09b-passissa on prestige-tasoja 50:n jälkeen; raja jakaisi prestige-valuuttaa yhä uudelleen. |
| Hunt Pass -kaudet levyllä olevasta kansiosta, `ACTIVE_HUNT_PASS` | **Yhdistetty** | Yksi lataaja sekä asetusreitille että tasolaskennalle, tarkistettuna käynnistyksessä; huono tiedosto pysäyttää käynnistyksen. `PROGRESSION_CONFIG_DIR` ja `ACTIVE_HUNT_PASS` (oletus `season09b`). |
| "Ilmainen Elite" -tila | **Jätetty pois** | Palvelimellamme jokaisella tilillä on jo Elite-passi. Hänen tilassaan palvelin ja peliohjelma olivat eri mieltä. |
| Suoja toistettuja myöntöjä vastaan | **Tavoite yhdistetty** | Uusinnan riski on todellinen (pelipalvelin yrittää uudelleen jopa 5 kertaa, B). Hänen sääntönsä voisi pudottaa aiheellista XP:tä; meidän vastaa alle 5 sekunnin kuluttua tavu tavulta samana toistuvaan myöntöön ensimmäisellä vastauksella eikä lisää mitään (`PROGRESSION_REPLAY_WINDOW_S`; pelipalvelimen 10 sekunnin lähetysvälin alla). Taaksepäin menevä tavoite vain kirjataan. |
| Tallennus samalla versiolla hyväksytään | **Pidetty omamme (409)** | Peliohjelma yrittää siististi uudelleen vastauksen 409 jälkeen (V, tiekartan kohta 2.1). |
| Lokirivi etenemispyynnöistä, joihin mikään reitti ei vastaa | **Otettu** | Harmiton ja hyödyllinen. |
| Ylimääräiset vahvistuslajit (`free`, `normal`, `1`, `2`) | **Jätetty pois** | 1.4.4-peliohjelma lähettää vain `public` ja `premium`. |
| Hänen oikeuskoodinsa | **Pidetty omamme** | Omassamme on oikeat myönnöt, vanheneminen, peruminen ja oletukset. |
| Lompakkotaulu `/balance`-vastausta varten | **Taulu jätetty pois, idea otettu** | `/balance` ja `/reconcile` kertovat nyt valuutat, jotka hahmolla oikeasti on (`BALANCE_FROM_INVENTORY`, päällä). `CURRENCY_PLATINUM_UNIV` ei ole mukana: ohjelmatiedosto nimeää sen vain Elite-tarjousnäkymän yhteydessä, ei siellä, missä saldot luetaan (S). |

## Escalation {#escalation}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| Kausiluettelo, tallennussäännöt, versiosäännöt ja reitit | **Otettu, sovitettuna** | Meillä oli vain tynkä. Hänen tasotaulukkonsa vastaa omaa tulkintaamme, ja kykypisteiden hintasääntö vastaa ohjelmatiedostoa (B `0x1414ae2a0`, `0x1414c0600`). Muutoksemme: jokainen tallennus kirjataan nykyiseen etenemislokiin (ei toista tapahtumataulua), toisen tilin välitetty tunniste kirjataan eikä sitä hylätä, tynkävahti hylkää jokaisen tallennuksen tasolla 25 ja vähintään 99 999 XP:llä, ellei tallennettu kausi ole jo tasolla 25 (hänen tarkisti ensimmäisen tallennuksen, jossa oli tasan 99 999), ja mallinnuksesta riippuvat säännöt vain varoittavat, kunnes `ESCALATION_STRICT=1`. Oletuksena pois: käyttöönotto pudottaa pelaajat tekaistusta maksimista tasolle 0. |

## Palkkiotehtävät ja odotusajat {#bounties}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| Palkkiotehtävätaulu yhtenä asiakirjana pelaajaa kohden, tyhjennettynä "täydessä nollauksessa" | **Pidetty omamme** | Uuden kauden alussa peli lähettää tyhjän taulun ja tyhjän valinnan, vaikka pelaajilla on yhä palkkiotehtäviä (B `0x1413f8009`, `0x1413f0f60`); hänen täysi nollauksensa poistaisi ne. Omamme tallentaa jokaisen palkkiotehtävän versioineen ja kirjanpitoineen. |
| 226 palkkiotehtävän määritelmien lista | **Jätetty pois** | Listalta puuttuva palkkiotehtävä on sallittu (B `0x1413f5a81`), ja sen palkinto otetaan silloin peliohjelman omasta taulukosta (B `0x1413df5e1`): pronssi 20, hopea 40, kulta 100 XP:tä (S). Hänen listansa laskisi kullan 60:een. |
| Lisää odotusaikareittejä ja pelaajien oikeus kirjoittaa odotusaikoja | **Pidetty omamme** | Aloitus ja erä ovat peliohjelmassa `PUT` (B `0x14144d366`, `0x14141e037`), ylimääräistä reittiä ei koskaan lähetetä, ja pelaaja, joka voisi kirjoittaa odotusaikoja, voisi nollata omat päivittäiset rajansa. Omamme läpäisi pelitestinsä (V, tiekartan kohta 2.5). |

Hänen palkkiotehtävä- ja odotusaikatestinsä siirrettiin; kaksi niistä väittää nyt meidän toimintaamme
(uuden kauden viesti säilyttää pelaajien palkkiotehtävät, ja `/bounty/game-data` pitää
palkkiotehtävälistan tyhjänä).

## Kauppa {#store}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| Ilmainen kauppa: neljä reittiä, ostotunniste, valikoima, välilehdet, tavaralajit, kiillot ja hiusten sävyt | **Otettu, sovitettuna** | Meillä ei ollut kauppaa. Tavarat kulkevat tavaraluettelomme ytimen ja oikeudet oman myöntökoodimme kautta, joten jokainen osto näkyy kirjanpidossa ja lokeissa. Hänen versionsa hylkäsi tilit, joilla on useampi hahmo; omamme antaa oston viimeksi tallennetulle hahmolle. Oletuksena pois (`STORE`), kunnes ylläpitäjä päättää ilmaisesta tai hinnoitellusta ja kaupan testi menee läpi pelissä. |
| Kymmenen Hunt Pass -tason tarjous | **Jätetty pois** | Sitä ei voinut ostaa edes hänen palvelimellaan (sen valuutalla ei ole myöntölajia). Tasohyppy on etenemisen myöntö. |
| Toistettavasti ostettava palkkiotehtävien tunnisteiden paketti | **Otettu, ylläpitäjä päättää** | Rajattomasti ilmaisia premium-palkkiotehtäviä; piilossa, ellei `STORE_REPEATABLE_TOKENS=1`. |

## Sosiaaliset ominaisuudet {#social}

| Hänen työnsä | Päätös | Miksi |
|:-------------|:-------|:------|
| XMPP-yhteys (WebSocket metagamen portissa ja raaka TCP portissa 60002) | **Pidetty omamme** | 1.4.4-peliohjelma yhdistää vain WebSocketin kautta (B `0x143a1eb8e`). Hänen rajojaan ei valvottu, ja jokainen kehys tallennettiin. Tiedostot pohjautuvat Mystic Paradoxiin. |
| Chat-huoneet ja kuiskaukset | **Pidetty omamme** | Hänen palvelimensa ei koskaan toimittanut huoneviestiä tai kuiskausta. Käyttäjänimikorjauksemme pysyy ennallaan. |
| Kavereiden verkkotila | **Yhdistetty: rakennettu uudelleen omaan chat-palvelimeemme** | Hänen versionsa ei lähettänyt tilatekstiä, ei välittänyt muutoksia ja merkitsi pelaajat poissa oleviksi osoitteesta, jonka peliohjelma jättää huomiotta (B `0x143a382b0`), joten kaverit pysyivät paikalla ikuisesti. Otimme vain hetket, jotka käynnistävät päivityksen. Pois päältä (`CHAT_PRESENCE`), kunnes kahden pelaajan testi osoittaa, että ryhmän automaattinen potku pysyy unessa. [Tekstichat]({{ chat_page.url | relative_url }}#presence) |
| Kaverireitit | **Pidetty omamme** | Hänen estolistansa lähetetään avaimella, jota peliohjelma ei lue (`blocklistedUsers`; peliohjelma lukee `blockedUsers`, B `0x1443fd258`), ja hänen kaverireittinsä käyttää Mystic Paradoxiin pohjautuvaa verkkotilakoodia. |
| Ryhmät | **Pidetty omamme, hyväksynnän uusinta yhdistetty** | Hänen hyväksyntänsä etsii kutsun tunnusta, jota peliohjelma ei koskaan lähetä (se lähettää ryhmän tunnuksen, B `0x140b35384`), eikä hänellä ollut kutsun hylkäystä, johtajan poistoa eikä ryhmän tilaa. Otimme hänen sietokykynsä kahdesti saapuvalle hyväksynnälle. |
| Killat | **Pidetty omamme** | Hänen haarassaan on alkuperäisen projektin tynkä. |
| Slayer Links | **Otettu, korjattuna** | Aito 1.4.4-ominaisuus. Ohjelmatiedostosta luettuna: kutsulistan toinen pelaaja on `account_id` eikä `linked_account_id`; linkin poisto on `DELETE /slayerlink/links` rungolla eikä `/slayerlink/link`; `DELETE /slayerlink/invites/<tunnus>` puuttui; tilavastaus sisältää kutsut, linkit ja asetukset; ja paikat numeroidaan 1–3 eikä 0–2. Se vastaa vain reitteihin, jotka saivat ennen vastauksen 404, joten se on päällä (`SLAYER_LINKS`). |
| `oauth/verify` nimeää kutsujan oman tilin | **Yhdistetty (tilin tunnus)** | Niin oikea palvelu vastasi. Emme koskaan vastaa 401: peliohjelma tarkistaa 30 sekunnin välein, ja tunnisteen 24 tunnin vanhenemisen jälkeen 401 voisi kirjata pelaajat ulos. `VERIFY_STUB_ACCOUNT=1` palauttaa paikkamerkin. |
| Tilihaut, jotka vaativat tunnisteen (401) | **Pidetty omamme** | Pehmeä tarkistuksemme antaa samat vastaukset kieltäytymättä keneltäkään. |
| `/account/mapping`, `/accountinfo/public` | **Jo meillä** | Teimme samat korjaukset 22. syyskuuta. |
| Reitti pyynnölle `GET /account127.0.0.1:61000` | **Pidetty omamme (404)** | Peliohjelma rakentaa yhden osoitteen ilman kauttaviivaa `/account`-osan perään. Hänen reittinsä vastaa siihen tilitiedoilla; pyynnössä ei ole tunnistetietoja, mikään näkyvä ei tänään hajoa, ja oikea korjaus kuuluu DLL:ään (tiekartan kohta 4.6). |
| `GET /present/<tili>` | **Jätetty pois** | Peliohjelmassa ei ole sille kutsujaa. |

## Lisenssi, data ja kiitokset {#licence}

| Asia | Päätös | Miksi |
|:-----|:-------|:------|
| Hänen reaaliaikatiedostonsa, jotka pohjautuvat Mystic Paradoxiin | **Mitään ei kopioitu** | Mikään ottamamme ei tarvitse niitä. Siksi toteamuksemme, ettei Dauntless Revivedissä ole Mystic Paradoxin koodia, pitää yhä paikkansa, eivätkä Mystic Paradoxin lisäehdot (AGPLv3:n kohta 7) koske tätä projektia. |
| `NOTICE.md` | **Otettu (ilman Mystic Paradox -osaa)** | Repositoriossa on nyt `NOTICE.md`, joka kertoo, mikä on peräisin Undauntedista ja mikä Harmonicin haarasta. |
| `ADDITIONAL_TERMS.md` | **Jätetty pois** | Se koskee vain Mystic Paradoxin aineistoa, jota meillä ei ole. |
| Hänen datatiedostonsa: Escalationin kausiluettelo, kaupan valikoima, tavaralajit, kaupan kuvaluettelo | **Otettu** | Peliohjelmasta yhteentoimivuuden vuoksi luettuja tunnisteita ja viritysarvoja, noin 330 lyhyttä englanninkielistä nimeä eikä lainkaan pelin sisältöä. Jokaiseen tiedostoon lisättiin alkuperämerkintä. Escalationin kausiluettelo nimeää pelitiedostot, joista se luettiin, niiden SHA-256-tiivisteillä. |
| Hänen palkkiotehtävien määritelmänsä | **Jätetty pois** | Katso [Palkkiotehtävät ja odotusajat](#bounties). |

**Näin hänen työnsä mainitaan:**

- Neljä hänen koodistaan siirrettyä lähdetiedostoa (`escalationConfig.ts`, `escalation.ts`,
  `freestore.ts` ja `slayerlinks.ts` kansiossa `legacy/UndauntedMetagame/src/controllers/`) alkavat rivillä
  `Ported from Harmonicrain/Undaunted (895f7c7), Copyright (C) 2026 Harmonic, AGPL-3.0-only`, jonka
  jälkeen kerrotaan, miten niitä muokattiin (tai Slayer Linksin kohdalla kirjoitettiin uudelleen ja
  korjattiin) Dauntless Revivedia varten. Hänen ideoidensa varaan rakennettu oma koodimme mainitsee
  hänen haaransa kommenteissaan.
- Jokainen hänen työtään sisältävä muutos nimeää hänet toiseksi tekijäksi ja kertoo "Ported from
  github.com/Harmonicrain/Undaunted 895f7c7."
- Hänet mainitaan sivulla [Kiitokset ja lisenssi]({{ legal_page.url | relative_url }}), README-tiedostossa
  ja käynnistimen Tekijät-sivulla (käynnistimen versiosta 0.1.6 alkaen).

## Testit {#tests}

Hänen testitiedostoissaan on 119 tapausta (yksi niistä käy läpi useita virhetilanteita, ajettuna noin
122). Kirjoitimme jokaisen uudelleen TypeScriptillä omia testiapuvälineitämme vastaan, ja jokaisessa on
kommentti, joka nimeää hänen tiedostonsa ja rivinsä.

| Hänen tiedostonsa (tapauksia) | Siirretty | Käännetty (väittävät nyt meidän toimintaamme) | Jätetty pois ja miksi |
|:------------------------------|:----------|:----------------------------------------------|:----------------------|
| entitlements (18) | 17 | | 1: omat oikeustestimme kattavat sen |
| escalation (17) | 17 | | |
| free store (7) | 7 | | |
| Hunt Pass progress (27) | 21 | 3: vahvistus ei anna mitään; uuden kauden viesti säilyttää palkkiotehtävät; palkkiotehtävälista pysyy tyhjänä | 3: tallennamme tuntemattomat etenemisradat tarkoituksella; ei lompakkoa; palkkiotehtävälistaa ei tarjoilla |
| Hunt Pass (19) | 18 | 1: season09b menee tason 50 yli prestigellä | |
| inventory (11) | 11 | | |
| mastery (11) | 6 | 3: toistettu myöntö ei lisää mitään; vahvistus ei maksa mitään, ja palkinto tulee kerran reitin `/inventory` kautta; yksi huono merkintä ei hukkaa muiden XP:tä | 2: tavoitteet pysyvät lähetettyinä; ei maksua, jonka voisi perua |
| matchmaking (5) | 4 | | 1: johtaja vie koko ryhmän minne tahansa, mikä on ristiriidassa ryhmäsuunnitelmamme kanssa |
| social ja friends (4) | 2 (verify ei koskaan vastaa 401; Slayer Links peliohjelman täsmällisillä pyynnöillä) | | 2: ne väittävät estolistan avainta, jota peliohjelma ei lue, ja hyväksyntää tunnuksella, jota peliohjelma ei lähetä |
| **Yhteensä (119)** | **103** | **7** | **9** |

Metagamen testisarja kasvoi 362:sta 567 testiin ja deploy-palvelimen 12:sta 26:een, ja kaikki menevät
läpi.

## Tietokantamuutokset {#migrations}

Kolme uutta tietokantamuutosta (migration), pelkkiä uusia tauluja: `0014_escalation`,
`0015_store_purchases` ja `0016_slayer_links`. Mitään olemassa olevaa taulua ei muuteta, kopioida
tai muunneta, eikä yhtään hänen tauluistaan, joiden nimi on sama kuin meidän, luoda koskaan. Siirtoa
edeltävä versio käynnistyy yhä muutetulla tietokannalla ja jättää uudet taulut huomiotta. Testi täyttää
muutoksen 0013 tasolla olevan tietokannan kaikenlaisilla riveillä, ajaa uudet muutokset kahdesti ja
tarkistaa, että jokainen aiempi rivi on ennallaan. Mitä tämä tarkoittaa palvelimelle, jolla on jo
pelaajia, kerrotaan sivulla [Päivittäminen]({{ upgrade_page.url | relative_url }}#harmonic-port).

## Mikä odottaa {#waits}

- **Ylläpitäjän päätökset:** ilmainen vai hinnoiteltu kauppa; rajattomat palkkiotehtävien tunnisteet;
  milloin Escalation otetaan käyttöön; päättyykö käynnissä oleva Slayer Link, kun kaveruus puretaan;
  itse käännetyn DLL:n jakaminen (joka toisi mukaan hänen DLL-muutoksensa); ja ennen muutosten
  lähettämistä GitHubiin sen varmistaminen, että "Harmonicrain" on Harmonicin oma GitHub-tili: lähetys
  julkaisee käynnistimen version 0.1.6, jonka Tekijät-sivu näyttää tämän tunnuksen, ellei käynnistimen
  automaattisia julkaisuja ole keskeytetty (`LAUNCHER_AUTO_RELEASE=false`).
- **Testit pelissä:** kaatunut Ramsgate, Escalation, kauppa, Elite-tasojen oikeudet, Claim, Slayer
  Links, verkkotila kahdella pelaajalla ja pitkä istunto uudella `oauth/verify`-vastauksella.

Ne ovat [tiekartalla]({{ roadmap_page.url | relative_url }}), ja asetukset, joilla kunkin osan saa
päälle tai pois, ovat sivulla [Asetukset]({{ config_page.url | relative_url }}#feature-switches).
