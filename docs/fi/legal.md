---
title: Kiitokset ja lisenssi
parent: Dauntless Revived suomeksi
nav_order: 90
lang: fi
ref: legal
locale: fi_FI
description: "Keiden työn varaan Dauntless Revived on rakennettu (ennen kaikkea Undaunted), mitä AGPL-3.0 käytännössä vaatii ja miksi sivustolla ei ole pelitiedostoja."
---

{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}
{% assign setup_page = site.pages | where: "path", "fi/setup/index.md" | first %}

# Kiitokset ja lisenssi
{: .no_toc }

Tällä sivulla kerromme, keiden työn varaan Dauntless Revived on rakennettu, millä ehdoilla sen
koodia saa käyttää ja miksi projekti on olemassa. Pari sanaa selitykseksi: peliohjelma (client,
toiselta nimeltään asiakasohjelma) on ohjelma, jonka pelaaja käynnistää omalla koneellaan, ja
palvelin (server) on tietokone tai ohjelma, joka pyörittää peliä verkossa.

<details open markdown="block">
  <summary>Tällä sivulla</summary>
  {: .text-delta }
- TOC
{:toc}
</details>

---

## Kiitokset {#credits}

Dauntless Revived on olemassa, koska muut tekivät vaikeat osat ensin ja julkaisivat työnsä.

| Projekti | Tekijä | Mitä olemme sille velkaa |
|---|---|---|
| [Undaunted](https://github.com/SyST3MDeV/Undaunted) | gwog (Gregory Morford, [SyST3MDeV](https://github.com/SyST3MDeV)), [EisigesEis](https://github.com/EisigesEis) ja [sen muut tekijät](https://github.com/SyST3MDeV/Undaunted/graphs/contributors) | Haaramme (fork, eli oma muokattu kopiomme) on rakennettu sen päälle, alkaen upstream-muutoksesta (commit) `7f692aa`. Undauntedista tulevat palvelintilan DLL, joka tekee 1.4.4-peliohjelman toisesta kopiosta pelipalvelimen, deploy-palvelin, joka pyörittää noita prosesseja, metagame-taustapalvelu ja käynnistin, ja kaikki ne on tehnyt gwog. EisigesEis kehitti metagamea: inventaario ja varustelut, eteneminen ja mestaruus, kutsukoodit ja ylläpidon rajapinta. Moninpeli-Ramsgate ja metsästykset versiossa 1.4.4 ovat Undauntedin saavutus. |
| [Harmonicin Dauntless 1.4.4 -haara](https://github.com/Harmonicrain/Undaunted) | Harmonic | Toinen Undaunted-haara 1.4.4-peliohjelmalle (AGPL-3.0-only). Sen muutoksesta `895f7c7` siirsimme Escalationin kausiluettelon ja tallennussäännöt, ilmaisen kaupan (valikoima, ostotapa, kaupan välilehdet), Slayer Linksin, deploy-palvelimen tavan käynnistää kaatunut Ramsgate uudelleen, idean kavereiden paikalla olosta ja hänen testitapauksensa sekä pienempiä ideoita. Mitä otimme ja mitä emme, syineen: [Harmonicin työn siirto]({{ '/fi/findings/harmonic-fork.html' | relative_url }}). |
| [Mystic Paradox](https://github.com/pranav158/Mystic-Paradox) | sen tekijät | Sukulaisprojekti, joka siirtää saman lähestymistavan Dauntlessin 1.12.0-peliohjelmaan, jotta mukana olisi myös 1.4.4:n jälkeen julkaistu sisältö. Haarassamme ei ole sen koodia (Harmonicin haarassa on sitä jonkin verran; me emme kopioineet siitä mitään), emmekä ole testanneet sitä. Mainitsemme sen, jotta myöhempää sisältöä etsivät löytävät sen. |
| [ooz](https://github.com/powzix/ooz) | powzix | Avoimen lähdekoodin Oodle (Kraken) -purkaja. 2.1.1-peliohjelma linkittää Oodlen staattisesti eikä toimita Oodle-DLL:ää, joten tavalliset pak-työkalut eivät pystyneet lukemaan sen asetuksia. Käänsimme ooz:n paikalliseksi kirjastoksi, jotta saimme purettua asetukset analyysiä varten. Se on vain analyysityökalu eikä osa haaraa. |
| [Dumper-7](https://github.com/Encryqed/Dumper-7) | Encryqed ja muut tekijät | Unreal Engine -SDK:n generaattori. Undauntedin palvelin-DLL on käännetty Dumper-7-SDK:ta vasten, joka on generoitu 1.4.4-peliohjelmasta. |
| [MinHook](https://github.com/TsudaKageyu/minhook) | Tsuda Kageyu | Funktioiden koukutuskirjasto (hooking), jota palvelin-DLL käyttää. BSD 2-Clause -lisenssi. |
| [GitHub Octicons](https://github.com/primer/octicons) | GitHub | GitHub-logo käynnistimen GitHub-painikkeessa, muokkaamattomana. MIT-lisenssi. |

Ja **Phoenix Labs**, joka teki Dauntlessin. Mikään tästä ei olisi säilyttämisen arvoista ilman
heidän peliään.

### Dauntless Revivedin osallistujat {#dauntless-revived-contributors}

- **[mixutin](https://github.com/mixutin)** (ylläpitäjä): palvelinpaketti, käynnistin,
  taustapalvelun korjaukset, oikea eteneminen ja nämä ohjeet.
- **[Vvoidddd](https://github.com/Vvoidddd)**: löysi syyn siihen, miksi ilmalaiva on pimeä ennen
  metsästystä (1.4.4:n automaattinen valotus; muutos peruttiin käynnistimen versiossa 0.1.1, koska
  se pimensi Ramsgaten), piilotti tilapäisten metsästyspalvelimien konsoli-ikkunat ja lisäsi
  projektin `.gitignore`-tiedoston ([#5](https://github.com/mixutin/dauntless-revived/pull/5));
  kirjoitti ensimmäisen suorituskykymittarin, jonka pohjalle palvelimen suorituskykyloki rakennettiin
  ([#6](https://github.com/mixutin/dauntless-revived/pull/6)); lisäsi käynnistimeen valinnaisen
  ”mukautuvan perusvalotuksen” asetuksen ilmalaivaa varten, oletuksena pois päältä
  ([#7](https://github.com/mixutin/dauntless-revived/pull/7)); teki käynnistimelle mahdolliseksi
  käyttää valmista Dauntless 1.4.4 -kansiota liittämällä sen polun
  ([#8](https://github.com/mixutin/dauntless-revived/pull/8)); kirjoitti ensimmäisen pelinsisäisen
  tekstichat-palvelimen (XMPP) ja testasi sen oikealla 1.4.4-pelillä
  ([#9](https://github.com/mixutin/dauntless-revived/pull/9)).
- **Harmonic** ([hänen 1.4.4-haaransa](https://github.com/Harmonicrain/Undaunted)): Escalationin
  kausiluettelo ja tallennussäännöt, ilmainen kauppa, ensimmäinen toimiva Slayer Links,
  deploy-palvelimen tapa käynnistää kaatunut Ramsgate uudelleen, idea kavereiden paikalla olosta sekä
  hänen haarastaan siirtämämme testitapaukset (katso yllä oleva taulukko). Jokainen hänen työtään
  sisältävä muutos nimeää hänet toiseksi tekijäksi.

Kaikki osallistujat ovat [osallistujasivulla](https://github.com/mixutin/dauntless-revived/graphs/contributors).
Käynnistin näyttää samat tiedot **Tekijät**-sivullaan, ja sen mukana tulee tiedosto
`THIRD-PARTY-NOTICES.txt`, jossa ovat sen sisältämien ohjelmistojen (Electron, MinHook, GitHub-logo
ja muutama pieni kirjasto) lisenssitekstit.

---

## Lisenssi: AGPL-3.0 {#the-license-agpl-30}

Undauntedin lisenssi on **AGPL-3.0-only**, ja niin on meidän haarammekin. Lisenssin koko teksti on
tiedostossa `LICENSE.txt` [repositoriossa]({{ site.github.repository_url }}) (koodivarastossa
GitHubissa). MinHookin lähdekoodi on mukana palvelin-DLL:n lähdekoodikansiossa
(`native/internal-server/MinHook`), ja sillä on edelleen oma BSD 2-Clause -lisenssinsä.

Tässä on selkokielellä, mitä lisenssi käytännössä vaatii. **Tämä on meidän tulkintamme, ei
oikeudellinen neuvo.** Jos asialla on sinulle merkitystä, lue itse lisenssi.

### Jos ajat sitä vain itsellesi {#if-you-only-run-it-for-yourself}

Koodin ajaminen omalla koneellasi, muokattuna tai muokkaamattomana, ei aiheuta mitään velvoitteita.

### Jos muut pelaavat muokkaamallasi versiolla {#if-other-people-play-on-a-version-you-modified}

AGPL:n verkkoehto (kohta 13) tulee voimaan. Jokaiselle, joka käyttää muokattua palvelintasi verkon
yli, on **tarjottava ajamasi täsmälleen saman version koko lähdekoodi** maksutta. Selkeä linkki
käynnistimessäsi, pelin sisäisessä tilaviestissä tai tämän kaltaisella sivulla riittää. Lähdekoodin
ei tarvitse olla julkinen. Myös yksityinen repositorio, johon pelaajillasi on pääsy, täyttää tämän
ehdon.

### Jos annat ihmisille tiedostoja {#if-you-give-people-files}

Kun annat jollekulle projektin DLL-tiedostot, käynnistimen tai skriptejä, se lasketaan kohdan 6
mukaiseksi objektikoodin levittämiseksi ("conveying"). Jokaisen kopion mukana on oltava:

- kopio lisenssitekstistä ja
- vastaava lähdekoodi tai selkeät ohjeet siihen, mistä sen löytää.
  - Muuttamattomille upstream-tiedostoille riittää linkki tarkkaan upstream-muutokseen, kunhan se
    pysyy saatavilla.
  - Itse kääntämillesi tai muuttamillesi tiedostoille linkitä oma haarasi vastaavaan muutokseen.

MinHookin lisenssi lisää yhden vaatimuksen käännetyille tiedostoille: kun levität DLL:ää, joka
sisältää MinHookin, toista mukana MinHookin tekijänoikeusilmoitus ja vastuuvapauslauseke.

### Jos muutat koodia {#if-you-change-the-code}

- Muokatussa versiossasi on oltava näkyvät ilmoitukset siitä, että olet muuttanut sitä, sekä
  asiaankuuluva päivämäärä (kohta 5a). Me teemme tämän README-tiedoston merkinnällä ja
  muutoshistorialla.
- Säilytä olemassa olevat tekijänoikeus- ja lisenssi-ilmoitukset.
- Muokattu versiosi pysyy **AGPL-3.0-only**-lisenssin alaisena. Et voi lisätä omia rajoituksiasi,
  esimerkiksi kieltää pelaajiasi jakamasta koodia (kohta 10).

### Mitä lisenssi ei kata {#what-the-license-does-not-cover}

- Erillinen ohjelma ei kuulu lisenssin piiriin pelkästään siksi, että se keskustelee palvelimen
  kanssa HTTP:n yli. Kaikki koodi, jonka kopioit Undauntedista, kuuluu.
- **AGPL kattaa vain projektin oman koodin. Se ei koskaan kata Phoenix Labsin pelitiedostoja, eikä
  se anna kenellekään mitään oikeuksia niihin.**

### Missä itse olemme {#where-we-stand-ourselves}

- Tämä repositorio on haaran koko lähdekoodi. [Tiekartta]({{ roadmap_page.url | relative_url }})
  luettelee muutoksemme upstreamiin verrattuna, ja sen juuressa oleva `NOTICE.md` kertoo, mistä kukin
  osa on peräisin: Undaunted, Harmonicin 1.4.4-haarasta siirretty työ (neljä hänen koodistaan
  siirrettyä lähdetiedostoa kantavat hänen tekijänoikeusmerkintäänsä), MinHook ja SDK-otsaketiedostot.
- Harmonicin haarassa on Mystic Paradoxista johdettua reaaliaikaista chat-koodia, jota koskevat AGPL:n
  kohdan 7 lisäehdot. Emme kopioineet siitä mitään: oma chat- ja läsnäolokoodimme on kirjoitettu tätä
  projektia varten. Siksi nuo lisäehdot eivät koske tätä repositoriota.
- Yksi puute on tiedossa. Upstream toimittaa valmiiksi käännetyn `dxgi.dll`-välitys-DLL:n (proxy),
  jonka lähdekoodi ei ole sen repositoriossa, eikä sitä tietääksemme ole julkaistu missään muualla.
  Konekielen purun (disassembly) perusteella se lataa järjestelmän oman `dxgi.dll`-tiedoston,
  välittää eteenpäin kolme sen funktiota (`CreateDXGIFactory`, `CreateDXGIFactory1`,
  `CreateDXGIFactory2`) ja lataa palvelin-DLL:n.
  Aiomme korvata sen omalla välitys-DLL:llä, joka on käännetty julkaistusta lähdekoodista. Aiomme
  myös kääntää `UndauntedInternalServer.dll`-tiedoston itse lähdekoodista valmiin kopion
  käyttämisen sijaan. Molemmat kohdat ovat tiekartalla.

---

## Ei pelitiedostoja {#no-game-files}

- Tämä sivusto ja repositorio eivät sisällä **yhtään Dauntlessin tiedostoa**: ei
  ohjelmatiedostoja, ei pak-tiedostoja tai IoStore-säiliöitä, ei grafiikkaa tai muuta pelisisältöä
  eikä pelin asetuksia. Ne eivät myöskään linkitä pelin latauksiin.
  - Yksi peritty asia, joka on hyvä tietää: upstream-Undauntedin metagame (taustapalvelu)
    sisältää tiedoston `UndauntedMetagame/src/vendor/progression_config.json` (noin 190 kt), jonka
    se antaa peliohjelmalle edistymis- ja palkintotietoina. Se on muodoltaan Phoenixin
    taustapalvelun tallennettu vastaus, ei pelin asennuksesta peräisin oleva tiedosto. Haaramme
    sisältää sen muuttamattomana upstreamista.
  - Upstream sisältää myös neljä pelin datataulukkoa JSON-muotoon vietyinä,
    `UndauntedDeployServer/src/vendor/*_table.json` (noin 2,2 Mt: pelaajien metsästykset,
    matchmakerin metsästykset ja kaksi Trials-taulukkoa), joista deploy-palvelin valitsee metsästyksen
    hirviön ja kartan. Ne ovat vientejä taulukoiden riveistä, eivät asennuksen tiedostoja. Haaramme
    sisältää ne muuttamattomina, ja oma `UndauntedMetagame/src/vendor/hunt_titles.json` tuotetaan
    niistä (metsästysten tunnukset ja englanninkieliset nimet).
  - Harmonicin haarasta mukana on neljä datatiedostoa: `UndauntedMetagame/src/vendor/escalation/seasons.json`
    (Escalation-kaudet), `store_catalog.json` ja `store_item_kinds.json` (ilmainen kauppa) sekä
    `UndauntedMetagame/test/data/store_art_skus.json` (kaupan tarjoustunnuksia testiä varten). Niissä on
    asennetusta peliohjelmasta yhteentoimivuuden vuoksi luettuja tunnisteita ja viritysarvoja sekä noin
    330 lyhyttä englanninkielistä nimeä; ei tekstuureja, malleja, ääniä tai muuta pelin sisältöä.
    `seasons.json` nimeää pelitiedostot, joista se luettiin, niiden SHA-256-tiivisteillä, kuten
    pelin tiedostoluettelomme tekee. Jokaisen tiedoston `_comment` kertoo, mistä se on peräisin.
- Jokainen pelaaja tarvitsee **oman kopionsa** Dauntless 1.4.4 -peliohjelmasta.
  [Asennus]({{ setup_page.url | relative_url }}) -osio kertoo, miten tarkistetaan, että kopio on
  aito ja muuttamaton.
- Emme muuta pelin tiedostoja levyllä. Ohjelmatiedoston viereen lisätään kaksi DLL-tiedostoa, ja
  joitakin asetuksia kirjoitetaan käyttäjäkohtaiseen asetuskansioon
  (`%LOCALAPPDATA%\Archon\Saved\Config`), johon peli itsekin kirjoittaa. Ennen kuin asensimme
  vertailukopiomme, tarkistimme sen kaikki 406 tiedostoa Phoenix Labsin omaa tiedostoluetteloa
  (manifest) vasten, eikä mikään, mitä teemme, muuta niitä.
- Löydökset-sivut lainaavat päätepisteiden URL-malleja, kenttien nimiä, funktioiden nimiä,
  osoitteita ja lyhyitä katkelmia. Lainaamme vain sen verran kuin kukin selitys vaatii, emmekä
  koskaan suuria osia pelin asetuksista tai koodista.
- Pelin asetuksissa, jotka Phoenix Labs toimitti peliohjelman mukana, on tunnistetietoja.
  Molemmissa versioissa on aito Slack-webhook-osoite, eli oikea tuotantokäytössä ollut osoite eikä
  paikkamerkki. Emme ole koskaan testanneet, toimiiko se yhä. 2.1.1:n asetuksissa on lisäksi Epic
  Online Servicesin asiakassalaisuus (client secret) ja salausavain selväkielisinä. **Emme ole
  koskaan käyttäneet niistä yhtäkään emmekä toista niitä missään.**

---

## Tavaramerkit {#trademarks}

"Dauntless" sekä siihen liittyvät nimet ja logot ovat omistajiensa tavaramerkkejä. Epic Games, Epic
Online Services ja Unreal Engine ovat Epic Games, Inc.:n tavaramerkkejä. Kaikki muut tavaramerkit
kuuluvat omistajilleen. Käytämme näitä nimiä vain kertoaksemme, minkä ohjelmiston kanssa tämä
projekti toimii.

Dauntless Revived **ei ole sidoksissa Phoenix Labsiin tai Epic Gamesiin, eivätkä ne ole
hyväksyneet tai tukeneet sitä**. Projekti on ei-kaupallinen: mitään ei myydä, eikä siinä ole oikealla
rahalla toimivaa kauppaa.

---

## Miksi tämä projekti on olemassa {#why-this-project-exists}

Phoenix Labs sulki Dauntlessin viralliset palvelimet 30. toukokuuta 2025. Peliohjelmaa ei voi
pelata ilman sen verkossa toimivaa taustapalvelua. Ilman korvaavaa palvelinta peliä ei siis voi
pelata lainkaan, ei edes niiden, joilla se on yhä asennettuna.

Teemme tätä **pelin säilyttämisen ja yhteentoimivuuden** vuoksi. Tutkimme, miten peliohjelma
keskustelee taustapalvelunsa kanssa, jotta itsenäisesti kirjoitettu palvelin voisi toimia
muuttamattoman peliohjelman kanssa. Näin ne, jotka jo omistavat pelin, voivat jatkaa sen
pelaamista ystäviensä kanssa. Julkaisemme oppimamme, jotta tieto säilyy, vaikka tämä projekti ei
säilyisikään.

Jos sinulla on oikeuksia johonkin tästä ja jokin asia huolestuttaa sinua, avaa ilmoitus (issue)
[repositoriossa]({{ site.github.repository_url }}).
