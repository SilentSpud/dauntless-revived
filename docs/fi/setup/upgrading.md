---
title: Päivitysohjeet
parent: Asennus
grand_parent: Dauntless Revived suomeksi
nav_order: 9
description: "Mitä pelaajille muuttuu, kun päivität jo käytössä olevan Dauntless Revived -palvelimen: Harmonicin forkin siirto (Slayer Links päällä; Escalation, kauppa ja paikalla olo rakennettu mutta pois päältä), tekstichat, kaverit, ryhmät ja killat sekä oletuksena päällä oleva oikea eteneminen. Näin pidät vanhat maksimitasot, aloitat alusta tai jatkat tyngällä."
lang: fi
ref: setup/upgrading
locale: fi_FI
---

{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign roadmap_en = site.pages | where: "path", "roadmap.md" | first %}
{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}
{% assign social_page = site.pages | where: "path", "fi/findings/social.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign chat_page = site.pages | where: "path", "fi/findings/chat.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "fi/findings/harmonic-fork.md" | first %}
{% assign escalation_page = site.pages | where: "path", "fi/findings/escalation.md" | first %}
{% assign store_page = site.pages | where: "path", "fi/findings/store.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}

# Päivitysohjeet
{: .no_toc }

Lue tämä ennen kuin päivität palvelimen, jolla on jo pelaajia. Jokainen kohta kertoo, mikä muuttuu,
mitä pelaajasi näkevät ja mitä voit asialle tehdä. Uusin muutos on ensimmäisenä.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Escalation, kauppa, Slayer Links ja pienemmät korjaukset {#harmonic-port}

**Syyskuu 2026.** Koskee jokaista palvelinta, joka päivitetään versioon, jossa on Harmonicin
1.4.4-forkin siirto ([Harmonicin työn siirto]({{ harmonic_page.url | relative_url }})). Tunnistat
sen metagamen käynnistysrivistä, joka alkaa `features:`.

### Mikä muuttuu {#harmonic-port-what-changes}

- **Kolme uutta tietokannan siirtoa (migraatiota) ajetaan itsestään ensimmäisellä käynnistyksellä:**
  `0014_escalation`, `0015_store_purchases` ja `0016_slayer_links`. Ne vain lisäävät kuusi taulua
  (`escalationprogression`, `escalationtalents`, `escalationunlocks`, `storepurchases`,
  `slayerlinkinvites`, `slayerlinks`); yhtään olemassa olevaa taulua tai riviä ei muuteta, kopioida
  tai muunneta. Ota tietokannasta varmuuskopio ensin kuten minkä tahansa päivityksen yhteydessä
  ([Tietokannan varmuuskopiointi]({{ admin_page.url | relative_url }}#back-up-the-database);
  palvelinpaketti tekee sen itse). Edellinen versio käynnistyy yhä muutetulla tietokannalla: se ei aja
  siirtoja, joita se ei tunne, ja jättää uudet taulut huomiotta, joten palaaminen ei vaadi
  palautusta. Tämä tarkistettiin kopiolla tietokannasta, joka oli siirron `0013` tasolla ja täynnä
  rivejä: jokainen rivi pysyi ennallaan molempien versioiden läpi.
- **Oletuksena päällä**, koska kukin vastaa vain siellä, missä palvelin antoi ennen virheen tai
  tyngän:

  | Muutos | Kytkin, jolla sen saa pois |
  |:-------|:---------------------------|
  | [Slayer Links](#harmonic-port-what-players-see) (My Links -välilehti): kahdeksan uutta reittiä | `SLAYER_LINKS=0` |
  | Pelin säännöllinen istunnon tarkistus (`oauth/verify`) nimeää pelaajan oman tilin paikkamerkin sijaan. Jokaisen pelaajan peli kutsuu sitä. | `VERIFY_STUB_ACCOUNT=1` |
  | `/balance` ja `/reconcile` kertovat valuutat, jotka hahmolla on (Ramsit, platina), nollan sijaan | `BALANCE_FROM_INVENTORY=0` |
  | Pelipalvelimen XP-myöntöön, joka toistuu tavu tavulta alle 5 sekunnin kuluttua, vastataan, mutta sitä ei lisätä uudelleen | `PROGRESSION_REPLAY_WINDOW_S=0` |
  | Kahdesti saapuvaan ryhmäkutsun hyväksyntään vastataan ryhmällä 404:n sijaan | ei kytkintä |
  | Deploy-palvelin käynnistää kaatuneen Ramsgaten tai Dojon uudelleen, kun pelaaja matkustaa sinne, eikä odota vahtikoiraa (jopa minuutti) | `PERSISTENT_WORLD_LIVENESS=0` (deploy-palvelimen asetuksissa) |

- **Rakennettu, mutta oletuksena pois päältä**, kukin päätöksen tai pelitestin jälkeen:

  | Ominaisuus | Kytkin | Miksi se odottaa |
  |:-----------|:-------|:-----------------|
  | Oikeat Escalation-tallennukset | `ESCALATION_MODE=real` | Jokainen pelaaja putoaa tekaistusta maksimista (taso 25) tasolle 0. [Escalation]({{ escalation_page.url | relative_url }}#switching-it-on) |
  | Ilmainen pelin kauppa | `STORE=free` | Ilmaisesta tai hinnoitellusta ei ole vielä päätetty (tiekartan kohta 3.7), eikä kauppaa ole kokeiltu pelissä. [Pelin kauppa]({{ store_page.url | relative_url }}#open) |
  | Rajattomat premium-palkkiotehtävätunnisteet kaupassa | `STORE_REPEATABLE_TOKENS=1` | Sinun päätöksesi. |
  | Kavereiden paikalla olo chatissa | `CHAT_PRESENCE=1` (yhdessä `CHAT=1` kanssa) | Kahden pelaajan testi, joka näyttää, että ryhmän automaattinen potku pysyy unessa. [Tekstichat]({{ chat_page.url | relative_url }}#how-to-verify-presence) |
  | Tason vahvistus antaa myös tason pysyvät oikeudet | `PROGRESSION_CONFIRM_ENTITLEMENTS=1` | Vain jos käy ilmi, ettei pelipalvelin anna niitä itse. |
  | Tiukat Escalation-säännöt | `ESCALATION_STRICT=1` | Kun Escalation on toiminut jonkin aikaa puhtain lokein. |

- **Deploy-palvelin kirjaa enemmän:** rivin aina, kun pelipalvelin sulkeutuu (`... exited with code 0`
  jokaisen metsästyksen jälkeen), fatal-rivin, kun sen käynnistys epäonnistuu, ja epäonnistunut
  pelipalvelimen käynnistys saa metagamelta selvän 500-vastauksen (pelaajat näkevät haun epäonnistuvan
  kuten ennenkin).
- **Muuta ei tarvitse tehdä:** ei palomuurisääntöä, ei uutta porttia eikä uutta käynnistintä
  (käynnistimen 0.1.6:ssa on vain päivitetyt tekijätiedot). Palvelin-DLL on ennallaan. Palvelinpaketti
  ei kirjoita yhtään uusista kytkimistä, joten yllä olevat oletukset ovat voimassa, kunnes lisäät rivin
  tiedostoon `metagame.env` (tai `deployserver.env`).

### Mitä pelaajasi näkevät {#harmonic-port-what-players-see}

- **Slayer Links** saattaa toimia Social-paneelin My Links -välilehdellä: kutsu kaveri johonkin
  kolmesta paikasta, ja linkki kestää viikon. Peliohjelma saattaa pitää välilehden piilossa; kukaan ei
  ole vielä kokeillut sitä pelissä.
- Siellä, missä peli lukee `/balance`-vastauksen, Ramsit näyttävät sen, mitä hahmolla oikeasti on (ennen
  0).
- Jos Ramsgate on kaatunut, seuraava sinne matkustava pelaaja odottaa hieman pidempään (palvelinpaketilla
  noin 10 sekuntia), kun se käynnistyy uudelleen, eikä päädy kaatuneelle palvelimelle.
- Muuta ei muutu, ennen kuin kytket jotain päälle: Escalation näyttää yhä tekaistun maksimin,
  kauppanäkymä saa yhä vanhan virheensä, ja kaikki näkyvät yhä poissa olevina.

### Mitä voit tehdä {#harmonic-port-what-you-can-do}

- Etsi päivityksen jälkeen metagamen lokista `features:`-rivi: se luettelee jokaisen uuden kytkimen
  arvoineen ([Asetukset]({{ config_page.url | relative_url }}#metagame)).
- Ominaisuuden kytkeminen päälle on yksi rivi metagamen asetuksissa ja uudelleenkäynnistys;
  [Palvelin ryhmälle]({{ admin_page.url | relative_url }}#switching-features-on) kertoo vaiheet ja
  kunkin testit.
- Jos jokin toimii väärin, ensimmäisen taulukon kytkimet ottavat kunkin muutoksen pois. Etsittävät
  lokirivit ovat sivulla [Vianetsintä]({{ trouble_page.url | relative_url }}#log-lines-of-the-port).
- **Paluu** päivitystä edeltäneeseen versioon (`Update-DauntlessServer.ps1 -Rollback`) ei vaadi muuta:
  vanhempi koodi jättää uudet kytkimet ja taulut huomiotta. Escalation palaa tyngäksi (tallennetut
  kaudet jäävät käyttämättä tauluihinsa), `/slayerlink` vastaa taas 404, ja jo tehdyt kauppaostot
  pysyvät pelaajien tavaraluetteloissa ja oikeuksissa. Ne löytyvät (`inventorylog.caller = 'store'`,
  `entitlements.source LIKE 'store:%'`), ja ne voi ottaa pois ylläpitoreiteillä.
- Älä koskaan muokkaa tietokannan siirtoa sen jälkeen, kun se on ajettu palvelimellasi.

## Tekstichat {#chat}

**Syyskuu 2026.** Koskee jokaista palvelinta, joka päivitetään tekstichatin sisältävään versioon
(paketissa on silloin `Set-Chat.ps1`, ja `Stack.ps1 status` näyttää `chat`-rivin).

### Mikä muuttuu {#chat-what-changes}

- **Pelaajille ei muutu mitään, ennen kuin kytket chatin päälle.** Chat on metagamen sisällä toimiva
  kuuntelija, joka on oletuksena pois päältä: paketti kirjoittaa `CHAT=0`, ellet valitse `-Chat On`.
  Niin kauan kuin se on pois päältä, pelin chat-yhteys saa yhdyskäytävältä vastauksen 502 ja yrittää
  uudelleen 15-45 sekunnin välein vaarattomasti, kuten ennenkin.
- **`Set-Chat.ps1` tulee päivityksen mukana.** Kytke chat päälle sen jälkeen toisella ajolla, kun
  kukaan ei pelaa (se käynnistää kokonaisuuden uudelleen): omalta koneelta
  `Deploy-Remote.ps1 -Server <osoite> -Chat On` tai palvelimella `Set-Chat.ps1 -On`
  ([Windows-palvelin]({{ winserver_page.url | relative_url }}#chat)). `-Chat` ei käy yhdessä valinnan
  `-Update` kanssa.
- **Yhdyskäytävä antaa pelin chat-yhteyksille oman tahtirajansa** (`GATEWAY_RATE_WS`, oletus `20,12`:
  20 kerralla, sitten 12 minuutissa osoitetta kohden), joten chatin uudelleenyhdistämiset eivät koskaan
  kuluta sitä varaa, jota pelaajan peliliikenne tarvitsee
  ([Asetukset]({{ config_page.url | relative_url }}#gateway)).
- **Ei tietokannan siirtoa, ei palomuurisääntöä eikä uutta käynnistintä.** Jokainen käynnistin
  versiosta v0.1.0 alkaen välittää chatin; v0.1.6:ssa on uusimmat tekijätiedot.

### Mitä pelaajasi näkevät {#chat-what-players-see}

Kun chat on päällä: Ramsgaten ja metsästysten chat, ryhmächat, kiltachat ja kuiskaukset, kaikki
käyttäjänimin. Ramsgaten chat on toistaiseksi istuntokohtainen, joten kaksi pelaajaa jakaa sen vain,
kun he matkustivat Ramsgateen yhdessä ryhmänä. Peli, joka oli jo käynnissä chatin kytkeytyessä
päälle, yhdistää noin 45 sekunnissa. Paikalla olon näyttäminen (kaverit näkyvät paikalla) tuli
myöhemmällä päivityksellä, oletuksena pois päältä ([yllä](#harmonic-port)).

### Mitä voit tehdä {#chat-what-you-can-do}

- Tee ensimmäisellä kerralla kahden pelaajan testi sivulta
  [Tekstichat]({{ chat_page.url | relative_url }}#how-to-verify); siinä luetellaan odotettavat
  lokirivit.
- Jos oikeiden pelaajien huoneisiin liittymiset hylätään syyllä `reason=nick-resource`, `nick-format`
  tai `nick-name`, aseta `CHAT_NICK_CHECK=log` tiedostoon `metagame.env` ja käynnistä uudelleen, kun
  kukaan ei pelaa. Mihin tahansa vakavaan: `Set-Chat.ps1 -Off`.
- **Paluu** päivitystä edeltäneeseen versioon (`Update-DauntlessServer.ps1 -Rollback`) ei vaadi muuta:
  vanhempi koodi ei lue asetuksia `CHAT` ja `GATEWAY_RATE_WS`.

## Kaverit, ryhmät ja killat {#social}

**Syyskuu 2026.** Koskee jokaista palvelinta, joka päivitetään kiltaversioon (metagamen lokiin tulee
`guild:`-rivejä, eikä `GET /guild/invite/player` enää kirjaa riviä "Guild invites (stubbed)").

### Mikä muuttuu {#social-what-changes}

- **Uusi siirto `0013_guilds` ajetaan itsestään ensimmäisellä käynnistyksellä.** Se vain lisää
  kolme taulua (`guilds`, `guildmembers`, `guildinvites`); mitään olemassa olevaa taulua tai riviä ei
  muuteta. Varmuuskopioi tietokanta ensin kuten minkä tahansa päivityksen yhteydessä
  ([Varmuuskopioi tietokanta]({{ admin_page.url | relative_url }}#back-up-the-database)). Edellinen
  versio käynnistyy yhä tietokannalla, jossa `0013` on ajettu (se ei aja siirtoja, joita se ei tunne,
  eikä välitä uusista tauluista), joten paluu ei vaadi palautusta.
- **Kolme kirjautumisen aikaista vastausta muuttuu.** Jokaisen pelaajan peliohjelma kutsuu niitä
  jokaisella kirjautumisella: `POST /accountinfo/public` kuvaa nyt kysyttyä tiliä (ja vastaa 404
  tuntemattomalle), `POST /account/mapping` yhdistää tunnukset muodossa, jota peliohjelma lukee, ja
  `GET /guild/invite/player` vastaa uudessa kuoressa (`{"code": "OK", "message": "", "payload":
  {"invites": []}, "invites": []}` vanhan tyngän sijaan). Testit toistavat peliohjelman jäsennyksen,
  mutta mitään tästä ei ole vielä nähty oikeassa pelissä.
- **Kiltoja voi perustaa ja niihin voi liittyä.** Kilta perustetaan vain, jos johtaja kirjoitti ja
  tarkistutti juuri tämän nimen perustamisikkunassa; henkilökunnan ja projektin sanat ovat varattuja.
- **Kutsut ovat tiukempia:** esto poistaa kahden pelaajan väliset odottavat kutsut, hylätty
  ryhmäkutsu pysäyttää lähettäjän 2 minuutiksi ja hylätty kiltakutsu killan 24 tunniksi, ja pelaaja
  lähettää enintään 20 ryhmäkutsua 10 minuutissa.

### Mitä pelaajasi näkevät {#social-what-players-see}

**Jokaisen pelaajan on käynnistettävä peli kerran uudelleen päivityksen jälkeen.** Peliohjelma pitää
ensimmäiset tilitiedot ja ensimmäisen yhdistämisen kustakin pelaajasta koko istunnon ajan, joten
vanhat, väärät vastaukset jäävät voimaan, kunnes peli käynnistetään uudelleen. Sen jälkeen ryhmäkutsut
näkyvät kohdassa PARTY INVITES, kaverin lisääminen toimii (toinen pelaaja näkee pyynnön seuraavalla
kirjautumisellaan), ja Guilds-välilehdellä voi perustaa kiltoja ja liittyä niihin. Paikalla olo ja EPIC
FRIENDS eivät oletuksena vieläkään toimi. Chat tuli myöhemmällä päivityksellä ([Tekstichat](#chat)) ja
paikalla olo sen jälkeen, oletuksena pois päältä ([yllä](#harmonic-port)).
[Liity kaverina]({{ friends_page.url | relative_url }}#friends-parties-and-guilds) kertoo tämän
pelaajille.

### Mitä voit tehdä {#social-what-you-can-do}

- Seuraa päivityksen jälkeisen ensimmäisen pelikerran aikana metagamen lokista rivejä, jotka on
  lueteltu sivulla [Kaverit, ryhmät ja killat]({{ social_page.url | relative_url }}#how-to-verify).
  `LOG_BODIES=1` tallentaa lisäksi pyyntöjen rungot (tilitunnuksia ja kiltojen nimiä); palvelinpaketti
  pakottaa sen pois päältä julkisessa tilassa, joten aseta se käsin vain sen pelikerran ajaksi ja
  poista runkoloki jälkeenpäin.
- Jos kirjautuminen häiriintyy päivityksen jälkeen, jokaisella muutoksella on kytkin metagamen
  asetuksissa (käynnistä metagame uudelleen muutoksen jälkeen, ja pelaajat käynnistävät pelin
  uudelleen):

  | Oire | Kytkin | Mitä se palauttaa |
  |:-----|:-------|:------------------|
  | Kirjautuminen tai Social-paneeli hajoaa, muiden pelaajien nimet ovat väärin | `ACCOUNTINFO_PUBLIC_LEGACY=1` | Alkuperäisen projektin tilitietovastauksen (ryhmäkutsut lakkaavat taas näkymästä) |
  | Kirjautuminen hajoaa heti tilihakujen jälkeen | `ACCOUNT_MAPPING=0` | Ei yhdistämisiä (kaverin lisääminen ei taas tee mitään) |
  | Kirjautuminen tai maailman lataus hajoaa kiltakutsujen kohdalla | `GUILDS=0` | Vanhat kiltatyngät; tallennetut killat säilyvät ja palaavat kytkimen mukana |
  | Invite to Party ei tee mitään yksin olevalle pelaajalle | `PARTY_SOLO_STUB=0` | Ei palautettavaa: tavallinen vastaus yhden hengen ryhmälle |
  | Killan perustaminen sanoo aina "Unable to create guild." ja lokissa lukee "no validate of this name" | `GUILD_CREATE_ACTIVITY_FALLBACK=1` | Heikomman tarkistuksen (katso [Asetukset]({{ config_page.url | relative_url }}#metagame-social)) |

## Oikea eteneminen on oletuksena päällä {#real-progression-default}

**Syyskuu 2026.** Koskee jokaista palvelinta, joka päivitetään versioon, jossa oikea eteneminen on
oletuksena, jos metagamen asetuksissa ei ole `PROGRESSION_MODE`-riviä, rivi on tyhjä tai sen arvo on
jokin muu kuin `real` tai `stub`. Uuden version tunnistaa käynnistysrivistä, joka alkaa
`Progression mode: real for every account`. Arvot kuten `off`, `0` tai `false` tarkoittivat ennen
tynkää ja tarkoittavat nyt oikeaa etenemistä. Jos asetit vain `PROGRESSION_REAL_ACCOUNTS`-luettelon
kokeillaksesi oikeaa etenemistä valituilla tileillä, nyt myös kaikki muut tilit vaihtuvat: lisää
`PROGRESSION_MODE=stub`, jos haluat säilyttää jaon.

### Mikä muuttui {#what-changed}

Aiemmin metagame (taustapalvelin, jonka kanssa peli keskustelee) vastasi kaikkiin etenemisen
(progression) pyyntöihin alkuperäisen Undauntedin tyngällä (stub), ellei asetuksena ollut
`PROGRESSION_MODE=real`:

- jokaisella tilillä näkyi Slayer-taso 50 ja täydet mestaruudet (mastery), eikä mitään pelaajan
  ansaitsemaa tallennettu;
- Hunt Pass näytti arvoa 99 999 999 ja kaikki palkinnot jo lunastettuina, eikä tynkä kerro tilillä
  olevan yhtään oikeutta (entitlement), joten Elite-rata näkyi todennäköisesti lukittuna;
- opetustehtävä ”Claim your Hunt Pass rewards” ei voinut koskaan valmistua.

Nyt **tyhjä tai puuttuva `PROGRESSION_MODE` tarkoittaa oikeaa etenemistä kaikille tileille**:

- Slayer-taso, aseiden ja hirviöiden mestaruus sekä niiden tavoitteet (objectives) alkavat alusta
  (Slayer-taso 1, ei mestaruutta) ja tallentuvat metsästyksistä ja tehtävistä.
- Hunt Pass (kausi 9b) alkaa tyhjästä. Lunastukset sekä ilmaisella että Elite-radalla tallentuvat.
  Jokaisella tilillä on oletuksena Elite-passi (`ENTITLEMENTS_DEFAULT`-luettelo).
- Tasopalkinnot antaa pelipalvelin kerran, tavaroiden kautta. Metagame vain kirjaa lunastuksen.
- Oikeudet, varustesarjojen paikat (loadout slots), odotusajat (cooldowns) ja palkkiotehtävät
  (bounties) tallentuvat tilikohtaisesti samassa tilassa.

`PROGRESSION_MODE=stub` antaa yhä alkuperäisen toiminnan, ja `PROGRESSION_REAL_ACCOUNTS` luettelee yhä
tilit, jotka saavat oikean etenemisen tynkätilassa (muissa tiloissa sitä ei huomioida). Muu arvo
kirjataan lokiin varoituksena ja tulkitaan oikeaksi etenemiseksi; ennen tätä muutosta se tarkoitti
tynkää.

**Mitä on testattu.** Testaaja pelasi oikealla etenemisellä kertakäyttöisellä testitilillä,
käynnisti sitten kaiken uudelleen ja kirjautui takaisin (englanninkielisen
[tarkistuslistan]({{ roadmap_en.url | relative_url }}) kohdat 2.8–2.12, 2.14 ja 2.15): tasot
alkoivat 1:stä, Slayer-tason ja aseen (kirveen) mestaruuden tasonnousut vahvistettiin ja jokainen
palkinto annettiin kerran, loputonta mestaruusilmoitusta ei tullut, Hunt Passin lunastukset
toimivat sekä ilmaisella että Elite-radalla, pelipalvelimen myöntämä oikeus tallentui, ja kaikki oli
tallessa uudelleenkäynnistyksen jälkeen. **Vielä kokeilematta:** hirviöiden mestaruus (se käyttää
samaa tallennusta kuin aseiden mestaruus), lisävarustesarjojen paikat pelin valikoissa,
palkkiotehtävien valinta ja lunastus pelin valikoissa, odotusajat vuorokauden vaihteen yli, useampi
pelaaja yhtä aikaa, ja tynkäaikana pelanneen tilin siirtäminen valetasolta 50 tasolle 1 (tarkistuslistan
kohta 2.13). Useamman pelaajan kanssa kannattaa huomata, että pelaajan oma peliohjelma saa nyt
vastauksen 403, jos se kysyy toisen tilin etenemistä; tynkä vastasi siihen kysyjän omilla tiedoilla.
Escalation pysyy alkuperäisenä tynkänä kummassakin tilassa, ellet aseta `ESCALATION_MODE=real`
([yllä](#harmonic-port)).

### Mitä pelaajasi näkevät {#what-your-players-will-see}

**Mitään ei siirretä automaattisesti.** Pelaajalla, joka pelasi palvelimellasi ennen päivitystä, ei
ole tallennettua etenemistä, joten hän aloittaa **Slayer-tasolta 1 tyhjällä Hunt Passilla** kuten uusi
pelaaja.

- Tavaroiden pitäisi säilyä, myös kaiken, mitä valetaso 50 jakoi (vaihtoehtoiset aseet, aseiden
  avausmerkit). Päivitys ei koske tavaroihin, mutta tätä ei ole vielä kokeiltu pelissä.
- Taso 1 lukitsee uudelleen sen, minkä valetaso 50 avasi: kulutustavaroiden paikat (tasot 2, 4 ja 7),
  osan valmistuksesta ja Slayer-tasoon sidotut metsästykset. Lisävarustesarjojen paikat eivät koskaan
  toimineet tyngällä (se ilmoittaa yhden paikan); oikean etenemisen kanssa ne avautuvat tasoilla 34,
  38 ja 45 (ei vielä kokeiltu pelin valikoissa).
- Päivittäiset ja viikoittaiset rajoitukset (odotusajat) säilyvät nyt metsästyksestä toiseen ja
  uudelleenkäynnistysten yli. Tynkä unohti ne aina, kun palvelin latasi pelaajan. Jos jokin rajoitus
  ei koskaan vapaudu, `PROGRESSION_MODE=stub` on varakeino sillä välin, kun ilmoitat asiasta.

Käynnistyessään metagame kirjaa lokiin tilansa ja, niin kauan kuin tällaisia pelaajia on, varoituksen
heidän määrästään:

```
Progression mode: real for every account (the default)
3 player account(s) have no stored progression yet: they start at Slayer level 1 with an empty Hunt Pass, not upstream's fake max ranks. Nothing was migrated. ...
```

Luvussa lasketaan tilit, joilla on hahmo mutta ei tallennettua etenemistä. Ylläpitäjän tiliä, jolla
ei ole koskaan pelattu, ei lasketa, ja pelaaja putoaa luvusta pois heti, kun hän ansaitsee jotain tai
kun annat hänelle lähtötason (seed). Uudella palvelimella luvussa ovat mukana myös pelaajat, jotka
ovat juuri luoneet hahmon eivätkä ole vielä ansainneet kokemuspisteitä (XP). Heidän kohdallaan
varoitus on vaaraton, ja se häviää, kun he ansaitsevat ensimmäiset kokemuspisteensä.

### Valitse, mitä tapahtuu, ennen kuin pelaajat kirjautuvat {#choose-what-happens}

| Haluat | Tee näin |
|:-------|:---------|
| Kaikki aloittavat alusta (tämän valitsimme omalle palvelimellemme) | Ei mitään. Kerro pelaajille, että tasot ja Hunt Pass alkavat alusta ja säilyvät tästä eteenpäin. |
| Yksi pelaaja pitää maksimitasot | Anna hänelle lähtötaso `grandfather`, kun hän ei ole pelissä (ohje alla). Jokainen rata asetetaan korkeimmalle tasolleen ja vahvistetaan siihen, joten mitään ei jaeta eikä ilmoituksia tule. Hunt Pass näkyy kokonaan lunastettuna. |
| Jatkaa vanhalla tyngällä toistaiseksi | Lisää metagamen asetuksiin `PROGRESSION_MODE=stub` ja käynnistä metagame uudelleen. `PROGRESSION_REAL_ACCOUNTS=<tunnus>,<tunnus>` antaa valituille tileille oikean etenemisen sillä välin. |

Lähtötaso `fresh` asettaa jokaisen radan nollaan ja tyhjentää tallennetut tavoitteet. Pelaaja, jolla ei
ole tallennettua etenemistä, on jo nollassa, joten `fresh` tarvitaan vain, kun haluat nollata tilin,
jolla on jo oikeaa etenemistä, esimerkiksi testitilin. Lähtötaso muuttaa vain kyseisen tilin
etenemisrivejä ja kirjaa muutoksen `progression_events`-tauluun. Lähtötasolla `grandfather` on
yksikkötestit, mutta sitä ei ole vielä kokeiltu pelissä.

**Ota ensin varmuuskopio.** Metagame ajaa tietokantamuutokset (migraatiot) käynnistyessään.
Palvelinpaketti ottaa varmuuskopion jokaisen päivityksen yhteydessä; itse kootulla koneella katso
[Varmuuskopioi tietokanta]({{ admin_page.url | relative_url }}#back-up-the-database).

### Missä asetus on {#where-the-setting-lives}

- **Itse koottu kone** ([Pystytä palvelin]({{ host_page.url | relative_url }})): `.env`
  `legacy/UndauntedMetagame`-kansiossa. Käynnistä metagame uudelleen muutoksen jälkeen.
- **Windows-palvelinpaketti** ([Windows-palvelin]({{ winserver_page.url | relative_url }})):
  `C:\DauntlessRevived\data\config\metagame.env`. Asennus ja `Update-DauntlessServer.ps1` säilyttävät
  jokaisen tiedostossa jo olevan asetuksen (asennus kirjoittaa uudelleen vain palvelinkohtaiset
  avaimet, eikä kumpikaan säilytä kommenttirivejä), joten palvelin, jolla oli jo `PROGRESSION_MODE=real` tai `stub`,
  pitää sen. Jos haluat jatkaa tyngällä, lisää rivi **ennen** päivitystä, tai lisää se jälkeenpäin ja
  aja `C:\DauntlessRevived\bin\Stack.ps1 restart -Only metagame`. Ellei tiedostossa lue `real` tai
  `stub`, päivitysohjelma toistaa onnistuneen päivityksen jälkeen metagamen varoituksen pelaajista,
  joilla ei ole tallennettua etenemistä.
- **Palautukset ja paluut edelliseen versioon noudattavat samaa sääntöä.** Palautus varmuuskopiosta,
  joka on otettu ennen tätä muutosta, tai paluu tätä vanhempaan versioon (päivitysohjelman
  automaattinen paluu tai `Update-DauntlessServer.ps1 -Rollback`) käyttää asetuksia sellaisinaan.
  Ilman riviä vanhempi versio tarkoittaa taas tynkää (pelaajat näkevät valetason 50; heidän
  tallennettu etenemisensä säilyy tietokannassa), ja uudempi versio tarkoittaa oikeaa etenemistä.
  Aseta `PROGRESSION_MODE=real` tai `stub` erikseen, jos haluat tilan pysyvän samana palautusten ja
  paluiden yli.

### Lähtötason antaminen pelaajalle {#seeding-a-player}

Julkinen yhdyskäytävä (tai mikä tahansa muu välityspalvelin) torjuu ylläpitoreitit, joten aja tämä
itse palvelinkoneella ylläpitäjän tiliavaimella. Tallenna se nimellä `seed-progression.js` mihin
tahansa sillä koneella. Se tarvitsee Noden version 18 tai uudemman eikä mitään muuta:

```js
// node seed-progression.js <admin key file> list
// node seed-progression.js <admin key file> <UserId> grandfather|fresh
// Optional: set METAGAME to the metagame's address if it does not listen on 127.0.0.1:61000.
const fs = require("fs");
const [keyFile, userId, mode] = process.argv.slice(2);
const base = (process.env.METAGAME || "http://127.0.0.1:61000") + "/undaunted/api";
const key = fs.readFileSync(keyFile, "utf8").trim();

async function call(method, path, body) {
  const r = await fetch(base + path, {
    method,
    headers: { "x-undaunted-user-api-key": key, ...(body ? { "content-type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(15000),
  });
  if (!r.ok) throw new Error(`${method} ${path} returned HTTP ${r.status}`);
  return r.json();
}

(async () => {
  if (userId === "list") {
    const { Users } = await call("GET", "/GetAllUsers");
    for (const u of Users) {
      const p = await call("GET", "/Progression?UserId=" + encodeURIComponent(u.UserId));
      const slayer = p.Tracks.find((t) => t.progression_id === "MasteryTrack_PlayerLevel");
      const pass = p.Tracks.find((t) => t.progression_id === "season09b");
      console.log(`${u.UserId}  ${u.Username}  real=${p.RealMode}  slayer=${slayer.earned_free_rank}  huntpass=${pass.progress}`);
    }
    return;
  }
  if (!userId || (mode !== "grandfather" && mode !== "fresh")) throw new Error("usage: see the first lines of this file");
  const r = await call("POST", "/SeedProgression", { UserId: userId, Mode: mode });
  console.log(`${r.UserId}: seeded ${r.Mode}, ${r.Tracks.length} tracks, real mode ${r.RealMode}`);
})().catch((e) => { console.error(e.message); process.exit(1); });
```

**Itse koottu kone.** Ylläpitäjän avaintiedosto on `C:\dr\data\owner.key`, jos seurasit sivua
[Pystytä palvelin]({{ host_page.url | relative_url }}). Jos seurasit sivua
[Palvelin ryhmälle]({{ admin_page.url | relative_url }}), metagame kuuntelee Tailscale-osoitteessasi
eikä osoitteessa `127.0.0.1`, joten aseta ensin `METAGAME`:

```powershell
$env:METAGAME = "http://100.x.y.z:61000"   # metagamen BIND_HOST
node seed-progression.js C:\dr\data\owner.key list
```

**Palvelinpaketin kone.** Aja skripti järjestelmänvalvojan PowerShellissä (Suorita
järjestelmänvalvojana): `owner.key` on vain järjestelmänvalvojien luettavissa. Yksityisessä tilassa
paketin metagame kuuntelee palvelimen Tailscale-osoitteessa; nämä rivit lukevat osoitteen ja portin
paketin asetuksista ja toimivat kummassakin tilassa:

```powershell
$cfg = Get-Content C:\DauntlessRevived\data\config\server.json -Raw | ConvertFrom-Json
$port = if ($cfg.Ports.metagame) { $cfg.Ports.metagame } else { 61000 }
$env:METAGAME = "http://$($cfg.BindAddress):$port"
node seed-progression.js C:\DauntlessRevived\data\keys\owner.key list
node seed-progression.js C:\DauntlessRevived\data\keys\owner.key UID-... grandfather
```

`list` tulostaa jokaisen tilin, sen Slayer-tason sellaisena kuin peli sen näyttää ja Hunt Passin
kokemuspisteet (XP). Se näyttää `real=false` tileille, jotka ovat yhä tyngän varassa
(`PROGRESSION_MODE=stub`); lähtötaso tallentuu joka tapauksessa ja otetaan käyttöön, kun tili on
oikean etenemisen tilassa. Skripti ei koskaan tulosta avainta.
