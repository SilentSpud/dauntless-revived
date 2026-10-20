---
title: Portit ja verkko
parent: Tekninen viite
grand_parent: Dauntless Revived suomeksi
nav_order: 2
description: "Dauntless Revived -palvelimen kaikki TCP- ja UDP-portit: kuka kuuntelee missäkin, kuunteluosoitteet yksityisessä ja julkisessa tilassa, pyyntöjen reitit ja palvelinpaketin palomuurisäännöt."
lang: fi
ref: reference/ports
locale: fi_FI
---

{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "fi/reference/scripts.md" | first %}
{% assign dev_page = site.pages | where: "path", "fi/reference/development.md" | first %}
{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}

# Portit ja verkko
{: .no_toc }

Tälle sivulle on koottu jokainen Dauntless Revived -kokonaisuuden käyttämä portti: missä osoitteessa
kukin kuuntelee, kuka siihen saa yhteyden, miten kaverin liikenne kulkee ja mitkä palomuurisäännöt
Windows-palvelinpaketti luo. Tässä mainitut ympäristömuuttujat selitetään kokonaisuudessaan sivulla
[Asetukset]({{ config_page.url | relative_url }}) ja reitit sivulla
[HTTP-rajapinta]({{ api_page.url | relative_url }}). Palvelimen pystyttäminen neuvotaan
asennusohjeissa; tämä sivu kokoaa vain verkkoon liittyvät tiedot.

Sivulla toistuu kolme kokoonpanoa:

- **Kehityskone**: kaikki yhdellä Windows-koneella vain omistajan omaan käyttöön, käsin pystytettynä
  sivun [Pystytä palvelin]({{ host_page.url | relative_url }}) mukaan.
- **Yksityinen tila**: kaverit liittyvät Tailscalen kautta. Joko paketin `-Mode Private` tai käsin tehty
  asennus sivun [Palvelin ryhmälle]({{ admin_page.url | relative_url }}) mukaan.
- **Julkinen tila**: [Windows-palvelinpaketin]({{ winserver_page.url | relative_url }}) oletus. Kaverit
  liittyvät internetin yli yhden TLS-salatun yhdyskäytäväportin kautta.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Yleiskuva {#at-a-glance}

| Portti | Protokolla | Kuka kuuntelee | Missä | Tavoitettavissa muilta koneilta |
|:-------|:-----------|:---------------|:------|:--------------------------------|
| 443 | TCP | Yhdyskäytävä (`legacy/UndauntedGateway/`), TLS | Palvelin, vain julkinen tila | Kaikilta. Ainoa julkinen TCP-portti. |
| 61000 | TCP | Metagame (`legacy/UndauntedMetagame/`) | Palvelin | Julkinen tila: ei koskaan suoraan, vain yhdyskäytävän kautta. Yksityinen tila: Tailscale-verkon koneilta. Kehityskone: ei keneltäkään. |
| 61001 | TCP | Deploy-palvelin (`legacy/UndauntedDeployServer/`) | Palvelin | Ei koskaan. |
| 61002 | TCP | Sisältöpalvelin (`legacy/UndauntedContent/`), pelitiedostot käynnistimelle | Palvelin, jos asennettu | Julkinen tila: ei koskaan suoraan, vain yhdyskäytävän kautta. Yksityinen tila: Tailscale-verkon koneilta. |
| 61005 | TCP | Sallittujen listan apuri (`legacy/UndauntedGateway/`, pyörii SYSTEM-tilillä) | Palvelin, vain julkinen tila | Ei koskaan. |
| 61099 | TCP | Chat metagamen sisällä (vain kun `CHAT=1`) osoitteessa `127.0.0.1` | Palvelin | Ei koskaan suoraan. Julkinen tila: yhdyskäytävän kautta. Palomuurisääntöä ei ole missään. |
| 8777 | UDP | Ramsgaten pelipalvelin | Palvelin | Pelaajilta. Julkinen tila: vain kirjautuneiden pelaajien osoitteista. Yksityinen tila: Tailscale-verkon koneilta. |
| 8776 | UDP | Training Dojon pelipalvelin | Palvelin | Kuten 8777. |
| 8770-8775 | UDP | Metsästysten ja opetusjakson pelipalvelimet, yksi prosessi ryhmää kohden | Palvelin | Kuten 8777. |
| 61000 | TCP | Käynnistimen välitin (`legacy/UndauntedLauncher/`) | Jokaisen pelaajan kone, julkinen tila, vain pelin ollessa käynnissä | Ei koskaan: se kuuntelee vain osoitteessa `127.0.0.1`. |
| 22 | TCP | OpenSSH (Windowsin oma, ei osa kokonaisuutta) | Palvelin | Julkinen tila: paketti sallii sen avainpohjaista ylläpitoa varten. Rajaa se palveluntarjoajalla omaan osoitteeseesi. |
| 3389 | TCP, UDP | Etätyöpöytä (Windows) | Palvelin | Julkinen tila: vain `-AdminIp`-osoitteista. Ilman sitä paketti poistaa käytöstä internetille avoimet säännöt, ellei annettu `-KeepRdpOpen`. |

Paketin hiekkalaatikkotila (`-Sandbox`, testeihin kehityskoneella) käyttää näiden sijaan portteja
62000, 62001, 62002, 62005, 62443 ja 62099; katso [Testien ja hiekkalaatikon portit](#test-ports).

## Palvelimen portit tarkemmin {#server-ports}

| Osa | Oletusportti | Asetus | Paketti | Mikä muu osoittaa siihen |
|:----|:-------------|:-------|:--------|:-------------------------|
| Metagame | koodissa ei oletusta; sovitusti 61000 | `PORT` (metagame) | `-MetagamePort` (1024-65535), hiekkalaatikossa 62000 | Peliohjelman ensimmäinen komentoriviparametri (DLL lukee sen), pelipalvelinten `Game.ini`, `GATEWAY_METAGAME_URL`, sisältöpalvelimen `METAGAME_URL` (oletus `http://127.0.0.1:61000`), yksityisessä tilassa `QOS_TARGET_URL` ja kutsut, kaveripaketin `-Server` (61000, jos siinä ei ole porttia) |
| Deploy-palvelin | koodissa ei oletusta; sovitusti 61001 | `PORT` (deploy-palvelin) | `-DeployPort` (1024-65535), hiekkalaatikossa 62001 | Metagamen `DEPLOYSERVER_URL` (`127.0.0.1:<port>`, ilman skeemaa) |
| Sisältöpalvelin | 61002 | `PORT` (sisältöpalvelin) | `-ContentPort` (1024-65535), hiekkalaatikossa 62002 | `GATEWAY_CONTENT_URL`; metagamen `CONTENT_PORT`, joka kertoo portin käynnistimille (`contentPort` ServerStatus-vastauksessa) |
| Sallittujen listan apuri | 61005 | `ALLOWLIST_PORT` | `-AllowlistPort` (1024-65535), hiekkalaatikossa 62005 | Yhdyskäytävän `ALLOWLIST_URL` (oletus `http://127.0.0.1:61005`) |
| Yhdyskäytävä | 443 | `GATEWAY_PORT` | `-GatewayPort` (1-65535), hiekkalaatikossa 62443 | Jokaisen julkisen tilan kutsun `port=` (käynnistin olettaa 443, jos kutsusta puuttuu portti) |
| Chat (metagamessa, `CHAT=1`) | 61099 | `CHAT_PORT` (metagame), `GATEWAY_WS_URL` (yhdyskäytävä), `ServerPort` tiedostossa `Engine.ini` | kiinteä; hiekkalaatikossa 62099; paketti kirjoittaa saman portin sekä asetukseen `CHAT_PORT` että `GATEWAY_WS_URL` | Pelin chat-ohitus, katso [Chat-portti 61099](#chat-port) |

Huomioita:

- **Metagamella ja deploy-palvelimella ei ole oletusporttia.** `PORT` on asetettava niiden
  `.env`-tiedostoon. Jos rivi puuttuu, ne pysähtyvät käynnistyessään Noden virheeseen
  `ERR_SOCKET_BAD_PORT`. Tyhjä `PORT=` saa Noden valitsemaan
  satunnaisen vapaan portin, jota mikään muu ei löydä. Jos portti on jo varattu, ne pysähtyvät
  käynnistyessään virheeseen `Could not listen on ...`. Myöskään metagamen `DEPLOYSERVER_URL`-asetuksella
  ei ole oletusta: ilman sitä jokainen pelipalvelinpyyntö, myös Ramsgateen, päättyy tilaan `FAILED`.
- **Miksi 61000 eikä 60000.** Alkuperäisen projektin kehityskäynnistin odottaa osoitetta
  `127.0.0.1:60000`. Palvelinkoneellamme toinen ohjelma piti jo sitä porttia, joten fork siirtyi
  portteihin 61000 ja 61001. Katso
  [Vianetsintä]({{ trouble_page.url | relative_url }}#port-60000-is-taken-and-the-metagame-says-clear-skies-anyway).
- **Miten paketti valitsee portit.** Erikseen annettu parametri voittaa, sitten asennuksen tiedoston
  `data\config\server.json` arvo (`Ports`; katso [Tiedostot ja data]({{ files_page.url | relative_url }})), ja
  viimeisenä oletus. Ensimmäisen asennuksen jälkeen `server.json` on määräävä lähde. Käytössä olevien
  porttien on oltava keskenään eri, ja asennusohjelma pysähtyy, jos jokin niistä on varattu (UDP-alueesta
  8770-8777 se vain varoittaa, jos jokin kuuntelee siellä jo). `-Sandbox` hylkää kaikki portit alueen
  62000-62499 ulkopuolelta.
- **Anna `-GatewayPort` jokaisella uudelleenasennuksella.** Aina kun `Deploy-Remote.ps1` ajaa
  asennusohjelman julkisessa tilassa, se välittää sille `-GatewayPort`-arvon, joka on 443, ellet anna
  muuta. Ilman sitä tehty uudelleenasennus palauttaa mukautetun yhdyskäytäväportin arvoon 443. Kutsuissa
  on portti mukana, joten vanhalle portille tehdyt kutsut eivät enää tavoita palvelinta. Parametrit on
  lueteltu sivulla [Skriptit ja parametrit]({{ scripts_page.url | relative_url }}).

## Pelipalvelinten UDP-portit {#game-udp-ports}

Pelipalvelimet ovat saman 1.4.4-peliohjelman erillisiä prosesseja, jotka deploy-palvelin käynnistää
valitsimilla `-EpicPortal -server -nullrhi`. Kukin kuuntelee yhtä UDP-porttia, joka annetaan sille
toisena komentoriviparametrina.

| Portti | Pelipalvelin | Milloin käynnistyy |
|:-------|:-------------|:-------------------|
| 8777 (`PORT_RANGE_END`) | Ramsgate | Kun deploy-palvelin käynnistyy. Jos se sulkeutuu, se käynnistetään uudelleen samaan porttiin, kun pelaaja matkustaa sinne (`PERSISTENT_WORLD_LIVENESS`, päällä), tai deploy-palvelimen vahtikoira (watchdog, 60 sekunnin välein) tekee sen, kumpi ehtii ensin; molemmat jakavat saman käynnistyksen, joten porttiin ei koskaan tule toista prosessia. |
| 8776 (`PORT_RANGE_END` - 1) | Training Dojo | Ensimmäisellä käyttökerralla, tai heti käynnistyksessä, kun `ENABLE_DOJO=1`. Käynnistetään uudelleen samaan porttiin samalla tavalla. |
| 8770-8775 (arvosta `PORT_RANGE_BEGIN` arvoon `PORT_RANGE_END` - 2) | Metsästykset ja opetusjakso | Yksi prosessi enintään 4 pelaajan ryhmää kohden, vapaiden porttien joukosta suurimmasta alkaen (8775, sitten 8774, ...). |

- **Kuunteluosoite.** Palvelin-DLL avaa portin antamatta osoitetta, joten pelipalvelin kuuntelee
  kaikissa liitännöissä (`0.0.0.0`). Palomuuri päättää, kuka siihen yltää. Osoite, johon pelaajat
  ohjataan, on deploy-palvelimen `MY_IP`: kehityskoneella `127.0.0.1`, yksityisessä tilassa
  palvelinkoneen Tailscale-IPv4-osoite ja julkisessa tilassa `-PublicHost`-parametrin julkinen
  IPv4-osoite.
- **Portti palaa vapaiden joukkoon**, kun vahtikoira huomaa metsästysprosessin sulkeutuneen, eli
  enintään noin minuutin viiveellä. Metsästyspalvelin sulkeutuu, kun siihen ei ole ollut kukaan
  yhteydessä yhteensä 50 sekuntiin.
- **Kun vapaat portit loppuvat**, deploy-palvelin vastaa metagamelle HTTP 500 -virheellä
  `{"error": "no_game_server"}` (lokissa `Matchmaking for ... failed: No free ports left!`), ja
  metagame merkitsee ryhmän haun epäonnistuneeksi: sen tilakysely vastaa `FAILED`. Oletusalueella kuusi metsästystä voi olla käynnissä yhtä aikaa.
- **Pidä `PORT_RANGE_END=8777`.** Palvelin-DLL kytkee 50 sekunnin tyhjäkäyntisulkeutumisensa pois
  kaikissa porteissa 8776 tai yli. Siksi Ramsgate ja Dojo pysyvät päällä. Suuremmalla loppuarvolla
  porteissa 8776 ja sen yli pyörivät metsästykset eivät koskaan sulkeutuisi; pienemmällä Ramsgate ja
  Dojo sulkeutuisivat tyhjäkäynnillä ja käynnistyisivät uudelleen kerta toisensa jälkeen. Jos haluat
  enemmän samanaikaisia metsästyksiä, pienennä `PORT_RANGE_BEGIN`-arvoa ja laajenna palomuurisääntöjä
  vastaavasti.
- **Paketin alue on kiinteästi 8770-8777.** Se tulee tiedoston
  `deploy/windows-server/DauntlessServer.Common.ps1` vakioista (`$DRUdpBegin`, `$DRUdpEnd`).
  Asennusohjelmalla ei ole sille parametria, ja jokainen asennusohjelman ajo kirjoittaa alueen
  uudelleen tiedostoon `deployserver.env`, sallittujen listan apurin `ALLOWLIST_PORTS`-asetukseen ja
  palomuurisääntöihin. `server.json` tallentaa sen (`UdpPortBegin`, `UdpPortEnd`), ja
  `Stack.ps1 status` tunnistaa Ramsgaten ja Dojon `UdpPortEnd`-arvon avulla.
- **`PORT_RANGE_BEGIN`, `PORT_RANGE_END` ja `MY_IP` on asetettava.** Deploy-palvelimella ei ole niille
  oletuksia: ilman porttialuetta yksikään metsästys ei käynnisty eikä Ramsgate saa käyttökelpoista
  porttia, ja ilman `MY_IP`-arvoa metagame ei saa osoitetta, johon pelaajat ohjataan, joten jokainen
  käynnistys epäonnistuu.

## Pelaajan kone {#players-pc}

Peliohjelma ei tarvitse pelaajan koneella yhtään saapuvan liikenteen porttia. Se ottaa yhteyden
ulospäin: TCP:llä metagameen (yksityinen tila) tai paikalliseen välittimeen (julkinen tila) ja UDP:llä
pelipalvelimen porttiin.

**Julkinen tila: käynnistimen välitin osoitteessa `127.0.0.1:61000`.** 1.4.4-peliohjelma puhuu vain
salaamatonta HTTP:tä. Pelin ollessa käynnissä käynnistin kuuntelee osoitteessa `127.0.0.1:61000` ja
välittää jokaisen pyynnön, myös WebSocket-yhteyksien avaukset (upgrade), TLS-salattuna palvelimen
yhdyskäytävälle. Yhteys on kiinnitetty kutsussa tulleeseen varmenteen sormenjälkeen. Samaa reittiä
kulkee myös kirjautuminen, jossa on mukana pelaajan tiliavain. Tiliavain on salaisuus: älä koskaan jaa
sitä äläkä tallenna sitä versionhallintaan. Välitin:

- kuuntelee vain osoitteessa `127.0.0.1`, yksinoikeudella, ja pysähtyy, kun peli sulkeutuu;
- vastaa 403 kutsujille, jotka eivät ole samalla koneella, pyynnöille, joiden `Host` tai `Origin` ei
  ole paikallinen, sekä pyynnöille, joissa on selaimen `Sec-Fetch-*`-otsakkeita, joten verkkosivu ei
  voi käyttää sitä;
- vastaa 502 `certificate_mismatch`, kun palvelimen varmenne ei vastaa kutsua, ja 502
  `upstream_unreachable`, kun yhdyskäytävää ei tavoiteta.

Portti on kiinteä, koska metagame antaa jokaiselle pelaajalle saman QoS-osoitteen,
`http://127.0.0.1:61000/QoS` (`QOS_TARGET_URL`), ja se on kunkin pelaajan oma välitin. Jos portti
61000 on varattu, esimerkiksi samalla koneella pyörivän kehityspinon takia, PELAA epäonnistuu
virheellä `relay_port_busy`. Käynnistin lukee piilotetun ohituksen `DAUNTLESS_REVIVED_RELAY_PORT`
(1024-65535), joka on tarkoitettu vain testeihin ja harjoituksiin. Se siirtää välittimen, pelin
ensimmäisen parametrin ja chat-portin kerralla, mutta palvelimelta tuleva QoS-osoite pysyy portissa
61000, eikä ole testattu, toimiiko alueen viivemittaus silloin yhä. Kaverit käyttävät aina porttia
61000.

**Yksityinen tila ja kaveripaketti.** Pelaajan koneella ei kuuntele mikään. Peli puhuu salaamatonta
HTTP:tä osoitteeseen `<host>:61000` Tailscalen yli. Kaveripaketti (`friend-kit/`) toimii vain näin,
joten sillä ei voi liittyä julkisen tilan palvelimelle; siihen tarvitaan käynnistimen välitin.

## Chat-portti 61099 {#chat-port}

Muuten 1.4.4-peliohjelma avaisi chat- ja läsnäoloyhteytensä (WebSocket) Epicin live-palveluun ja
lähettäisi sille tilin tunnisteen (account id) ja kirjautumistunnisteen. Jokainen `Engine.ini`-tiedoston
kirjoittaja ohjaa yhteyden muualle (`[OnlineSubsystemMcp.XMPP]`, `bUseSSL=false`):

| Kuka | `ServerAddr` | `ServerPort` |
|:-----|:-------------|:-------------|
| Kehityskone (sivun [Pystytä palvelin]({{ host_page.url | relative_url }}) skriptit) | `ws://127.0.0.1` | 61099 |
| Käynnistin, yksityinen tila | `ws://<kutsun osoite>` | 61099 |
| Käynnistin, julkinen tila | `ws://127.0.0.1` (välitin) | 61000 (välittimen portti); yhdyskäytävä välittää yhteyden avauksen osoitteeseen `GATEWAY_WS_URL` |
| Kaveripaketti (`play.ps1`) | `ws://<-Server-arvon osoiteosa>` | 61099 |
| Paketti, pelipalvelimille (palvelutili) | `ws://127.0.0.1` | 61099 |

Kun `CHAT=1`, metagame itse kuuntelee porttia 61099 pelin chattia varten
([Asetukset]({{ config_page.url | relative_url }}#metagame-chat),
[Tekstichat]({{ '/fi/findings/chat.html' | relative_url }})). Se kuuntelee vain osoitteessa
`127.0.0.1`, eikä mikään palomuurisääntö avaa sitä missään tilassa.

- **Julkinen tila** (vuokrattu palvelin): pelin chat-yhteys menee pelaajan koneella käynnistimen
  välittimelle, sieltä TLS-salattuna yhdyskäytävälle ja edelleen osoitteeseen `127.0.0.1:61099`
  (`GATEWAY_WS_URL`). Pyynnön polku on `//` ja protokolla `xmpp`; välitin ja yhdyskäytävä päästävät
  molemmat läpi, ja käynnistimet versiosta 0.1.0 alkaen tekevät tämän jo. Paketti kirjoittaa saman
  portin metagamen asetukseen `CHAT_PORT` ja yhdyskäytävän asetukseen `GATEWAY_WS_URL`, joten ne eivät
  voi erota toisistaan. Kun chat on pois päältä, yhdyskäytävä vastaa avaukseen 502, ja peli yrittää
  uudelleen 15-45 sekunnin välein vaarattomasti.
- **Kehityskone** (yksi kone): peli ottaa yhteyden suoraan osoitteeseen `ws://127.0.0.1:61099`. Aseta
  `CHAT=1` tiedostoon `legacy/UndauntedMetagame/.env` ([Pystytä palvelin]({{ host_page.url | relative_url }})).
- **Yksityinen tila** (Tailscale): ei vielä tuettu. Peli ottaisi yhteyden osoitteeseen
  `ws://<Tailscale-osoite>:61099`, mutta kuuntelija ei suostu muuhun kuin paikalliseen osoitteeseen,
  joten yhteys epäonnistuu vaarattomasti kuten ennenkin.
- **Pelipalvelimet** paketin palvelimella: niidenkin `Engine.ini` osoittaa osoitteeseen
  `ws://127.0.0.1:61099`. Ei tiedetä, kirjautuvatko ne chattiin lainkaan; hylätty kirjautuminen näkyy
  metagamen lokissa rivinä `chat: login refused ...` enintään kerran 10 minuutissa.
- **Paketin hiekkalaatikko**: yhdyskäytävä ja `CHAT_PORT` käyttävät porttia 62099, jotta hiekkalaatikko
  ei koskaan yllä kehityskoneen omaan porttiin 61099.

`Engine.ini`-avaimet ovat sivulla [Pelin asetukset]({{ gamesettings_page.url | relative_url }}).

## Kuunteluosoitteet kokoonpanoittain {#bind-addresses}

| Asetus | Kehityskone | Yksityinen tila | Julkinen tila | Paketin hiekkalaatikko (julkinen tila) |
|:-------|:------------|:----------------|:--------------|:---------------------------------------|
| Metagamen `BIND_HOST` | `127.0.0.1` | palvelinkoneen Tailscale-IPv4 | `127.0.0.1` | `127.0.0.1` |
| Deploy-palvelimen `BIND_HOST` | `127.0.0.1` | `127.0.0.1` | `127.0.0.1` | (ei käynnistetä) |
| Sisältöpalvelimen `BIND_HOST` | `127.0.0.1`, jos ajat sitä | palvelinkoneen Tailscale-IPv4 | `127.0.0.1` | `127.0.0.1` |
| `GATEWAY_BIND` | | | `0.0.0.0` | `127.0.0.1` |
| `ALLOWLIST_BIND` | | | `127.0.0.1` | `127.0.0.1` (koeajo) |
| Pelipalvelimet (UDP) | kaikki liitännät | kaikki liitännät | kaikki liitännät | (ei käynnistetä) |
| Deploy-palvelimen `MY_IP` | `127.0.0.1` | palvelinkoneen Tailscale-IPv4 | `-PublicHost`-parametrin julkinen IPv4 | `127.0.0.1` |
| Metagamen `QOS_TARGET_URL` | `http://127.0.0.1:61000/QoS` | `http://<Tailscale IPv4>:61000/QoS` | `http://127.0.0.1:61000/QoS` (pelaajan välitin) | `http://127.0.0.1:61000/QoS` |
| Metagamen `DEPLOYSERVER_URL` | `127.0.0.1:61001` | `127.0.0.1:61001` | `127.0.0.1:61001` | `127.0.0.1:62001` |

Mitä kukin kuuntelija hyväksyy:

| Kuuntelija | Hyväksyy | Oletus |
|:-----------|:---------|:-------|
| Metagame | Yhden osoitteen. Listaa ei tueta. Kun `GATEWAY_SECRET` on asetettu (julkinen tila), muu kuin loopback-osoite antaa käynnistyksessä varoituksen. | `127.0.0.1` (alkuperäinen kuunteli kaikissa liitännöissä) |
| Deploy-palvelin | Yhden osoitteen. Pidä `127.0.0.1`: sen reitit vastaavat joka tapauksessa 403 jokaiselle kutsujalle, joka ei tule loopbackista. | `127.0.0.1` |
| Sisältöpalvelin | Pilkuin erotetun listan IP-osoitteita (tai `localhost`), yksi kuuntelija osoitetta kohden. Vain loopback, `localhost` sekä Tailscalen 100.64.0.0/10 ja fd7a:115c:a1e0::/48 sallitaan, ellei `CONTENT_ALLOW_ANY_BIND=1`. | `127.0.0.1` |
| Yhdyskäytävä | Yhden IP-osoitteen; nimiä ei hyväksytä. Pidä se IPv4-osoitteena (`0.0.0.0`): osoitteen, jonka yhdyskäytävä ilmoittaa sallittujen listalle, on oltava sama, josta pelin UDP-liikenne tulee, ja `MY_IP` on IPv4. | `0.0.0.0` |
| Sallittujen listan apuri | Vain `127.0.0.1` tai `::1`. Kaikki muu on käynnistysvirhe. | `127.0.0.1` |
| Chat (`CHAT_BIND_HOST`, metagamessa) | `127.0.0.1` tai `::1`; kun `GATEWAY_SECRET` on asetettu (julkinen tila), vain `127.0.0.1`. Kaikki muu pitää chatin pois päältä ja kirjoittaa virherivin; metagame käynnistyy silti. | `127.0.0.1` |
| Yhdyskäytävän kohteet (`GATEWAY_METAGAME_URL`, `GATEWAY_CONTENT_URL`, `GATEWAY_WS_URL`) ja `ALLOWLIST_URL` | Vain muoto `http://host:port` tällä koneella (127.x.x.x, `::1` tai `localhost`), ilman polkua. Yhdyskäytävän salaisuus ei koskaan lähde koneelta. | `127.0.0.1` porteilla 61000, 61002, 61099 ja 61005 |
| Käynnistimen välitin | Vain `127.0.0.1`, kiinteästi koodissa. | `127.0.0.1:61000` |

Yksityisessä tilassa Tailscale-osoitteen on oltava olemassa ennen kuin metagame käynnistyy, tai se
sulkeutuu virheeseen `Could not listen on <address>:61000`. Paketin asennuksessa `Stack.ps1` odottaa
osoitteen ilmestymistä enintään 5 minuuttia. Älä kierrä ongelmaa asetuksella `BIND_HOST=0.0.0.0`:
metagame kuuntelisi silloin myös lähiverkossasi.

Julkisessa tilassa (`GATEWAY_SECRET` asetettu) metagame varoittaa käynnistyessään myös, jos
`QOS_TARGET_URL` ei ole muotoa `http://127.0.0.1:<port>/QoS`, koska jokaisen pelaajan pelin on
pingattava omaa välitintään.

## Pyyntöjen reitit {#request-paths}

### Julkinen tila {#request-path-public}

```text
Pelaajan kone                                       Palvelin
peli ---HTTP---> välitin 127.0.0.1:61000 ===TLS===> yhdyskäytävä :443 ---> metagame 127.0.0.1:61000
käynnistin ==================TLS==================> yhdyskäytävä :443 ---> sisältö  127.0.0.1:61002
peli ---------------------UDP---------------------> pelipalvelin :8770-8777 (sallittujen listan sääntö)
```

| Vaihe | Mistä | Mihin | Mitä kulkee |
|:------|:------|:------|:------------|
| 1 | Käynnistin | Yhdyskäytävä `<PublicHost>:443`, TLS kiinnitettynä kutsun sormenjälkeen | Rekisteröityminen, palvelimen tila, uutiset ja kuvat, pelitiedostojen lataukset |
| 2 | Peli | Välitin `127.0.0.1:61000`, salaamaton HTTP pelaajan koneella | Jokainen taustapalvelun kutsu, kirjautuminen tiliavaimella, QoS-ping, chatin WebSocket |
| 3 | Välitin | Yhdyskäytävä `<PublicHost>:443`, TLS kiinnitettynä | Kaikki vaiheesta 2 |
| 4 | Yhdyskäytävä | Metagame `127.0.0.1:61000` | Kaikki paitsi `/content` ja WebSocketit. Yhdyskäytävä lisää otsakkeet `X-Dauntless-Gateway` (yhdyskäytävän salaisuus) ja `X-Forwarded-For` (pelaajan osoite). Vain neljä `/undaunted/api`-reittiä pääsee läpi; katso [HTTP-rajapinta]({{ api_page.url | relative_url }}). |
| 5 | Yhdyskäytävä | Sisältöpalvelin `127.0.0.1:61002` | `/content` ja `/content/*` |
| 6 | Yhdyskäytävä | Chat `127.0.0.1:61099` metagamessa | WebSocket-avaukset: pelin chat (pyynnön polku `//`). Kun chat on pois päältä, siellä ei kuuntele mikään: 502. |
| 7 | Yhdyskäytävä | Sallittujen listan apuri `127.0.0.1:61005`, `POST /allow` apurin salaisuudella | Pelaajan osoite onnistuneen kirjautumisen (`POST /account/api/oauth/token`) tai sellaisen onnistuneen elonmerkin jälkeen, jossa oli bearer-tunniste |
| 8 | Sallittujen listan apuri | Windowsin palomuuri | Avaa UDP-portit 8770-8777 tälle osoitteelle, kunnes sen viimeisestä kirjautumisesta tai elonmerkistä on kulunut 600 sekuntia |
| 9 | Metagame | Deploy-palvelin `127.0.0.1:61001` | Pelipalvelimen käynnistys tai haku. Vastauksena `MY_IP` (julkinen IPv4) ja UDP-portti. |
| 10 | Peli | Pelipalvelin `<julkinen IPv4>:8770-8777`, UDP | Pelisessio suoraan, ei välittimen eikä yhdyskäytävän kautta |
| 11 | Pelipalvelimet | Metagame `127.0.0.1:61000` | Hahmojen lataus ja tallennus pelipalvelinavaimella. Osoite tulee palvelutilin `Game.ini`-tiedostosta, avain pelipalvelimen komentoriviltä. |
| 12 | Sisältöpalvelin | Metagame `127.0.0.1:61000` | Tiliavaimen tarkistus jokaiselle lataukselle (`GetUserInfo`, välimuistissa) |

### Yksityinen tila {#request-path-private}

```text
Pelaajan kone             Tailscale (salattu)            Palvelin (100.x.y.z)
peli       ---HTTP-------------------------------------> metagame 100.x.y.z:61000
käynnistin ---HTTP-------------------------------------> metagame :61000, sisältö :61002
peli       ---UDP--------------------------------------> pelipalvelin 100.x.y.z:8770-8777
```

| Vaihe | Mistä | Mihin | Mitä kulkee |
|:------|:------|:------|:------------|
| 1 | Käynnistin | Metagame `http://<100.x.y.z>:61000` | Rekisteröityminen ja palvelimen tila |
| 2 | Käynnistin | Sisältöpalvelin `http://<100.x.y.z>:61002` (portti tulee ServerStatus-vastauksen kentästä `contentPort`) | Pelitiedostojen lataukset, uutiset ja kuvat |
| 3 | Peli | Metagame `http://<100.x.y.z>:61000` | Jokainen taustapalvelun kutsu, kirjautuminen tiliavaimella, QoS-ping |
| 4 | Peli | `ws://<100.x.y.z>:61099` | Chat. Yksityisessä tilassa ei vielä tuettu: siinä osoitteessa ei kuuntele mikään, joten yhteys epäonnistuu vaarattomasti. |
| 5 | Metagame | Deploy-palvelin `127.0.0.1:61001` | Pelipalvelimen käynnistys tai haku. Vastauksena `MY_IP` (Tailscale-IPv4) ja UDP-portti. |
| 6 | Peli | Pelipalvelin `<100.x.y.z>:8770-8777`, UDP | Pelisessio |
| 7 | Pelipalvelimet | Metagame `http://<100.x.y.z>:61000` | Hahmojen lataus ja tallennus pelipalvelinavaimella |
| 8 | Sisältöpalvelin | Metagame `http://<100.x.y.z>:61000` | Tiliavaimen tarkistus |

Salaamaton HTTP on hyväksyttävää vain tässä, koska Tailscale salaa koneiden välisen liikenteen.
Yksityisen tilan kutsu (`v=1`) hyväksyy osoitteekseen vain Tailscale-osoitteen (100.64.0.0/10),
MagicDNS-nimen (`*.ts.net`) tai loopback-osoitteen, joten käynnistin ei koskaan lähetä avainta
salaamattomana HTTP:nä avoimen internetin yli. Pelipalvelimet tavoittavat metagamen sen
Tailscale-osoitteesta, koska metagame kuuntelee vain siinä yhdessä osoitteessa. Käsin tehdyssä
asennuksessa `Game.ini` on tätä varten kirjoitettava uudelleen (katso
[Palvelin ryhmälle]({{ admin_page.url | relative_url }}#switch-the-addresses-to-tailscale)).

**Kehityskoneella** jokainen vaihe pysyy osoitteessa `127.0.0.1`, eikä palomuurisääntöjä tarvita.

## Mitä ei saa koskaan päästää näkyviin {#never-exposed}

- **Deploy-palvelin (61001).** Siinä ei ole tunnistautumista: kuka tahansa, joka voi kutsua sitä,
  käynnistää peliprosesseja koneella. Se kuuntelee osoitteessa `127.0.0.1`, sen molemmat reitit
  vastaavat 403 kutsujille, jotka eivät tule loopbackista, ja kaikelle, missä on välityspalvelimen
  otsake, yhdyskäytävällä ei ole reittiä siihen, eikä mikään kokoonpano avaa sitä palomuurissa. Siksi
  `DEPLOYSERVER_URL`-arvon on oltava `127.0.0.1:<port>`.
- **Sallittujen listan apuri (61005).** Se pyörii SYSTEM-tilillä ja muuttaa palomuuria. Se kuuntelee
  vain loopbackissa, vastaa 403 kaikelle muulle ja vaatii `ALLOWLIST_SECRET`-arvon. Se on salaisuus:
  älä koskaan jaa sitä äläkä tallenna sitä versionhallintaan.
- **Metagame (61000) julkisessa tilassa.** Vain yhdyskäytävä saa tavoittaa sen. Yhdyskäytävän
  kiertäminen ohittaisi sen ylläpitoreittien eston, pyyntöjen kokorajan (128 KiB; metagame itse
  hyväksyy JSON-runkoja 50 megatavuun asti) ja pyyntömäärien rajoitukset. Yksityisessä tilassa vain
  Tailscale-verkon koneet saavat tavoittaa sen.
- **Sisältöpalvelin (61002).** Se jakaa pelitiedostot rekisteröityneille tileille. Se kieltäytyy
  kuuntelemasta muualla kuin loopback- tai Tailscale-osoitteissa, ellei `CONTENT_ALLOW_ANY_BIND=1`;
  aseta se vain, jos palomuuri pitää internetin ulkona.
- **Käynnistimen välitin (61000 pelaajan koneella).** Sen kautta kulkee tiliavain. Se kuuntelee aina
  vain osoitteessa `127.0.0.1`.
- **UDP 8770-8777 julkisella palvelimella.** Avaa ne kaikille palveluntarjoajan palomuurissa, mutta ei
  koskaan Windowsin palomuurissa: siellä vain sallittujen listan sääntö saa avata ne. Älä lisää alueelle
  muita sääntöjä, äläkä hyväksy Windowsin ”Salli käyttö” -kehotetta pelille.
- **Ylläpitorajapinta ja pelipalvelinavain.** Ne eivät koskaan toimi yhdyskäytävän kautta. Yhdyskäytävä
  vastaa 403 ylläpitoreitteihin ja jokaiseen pyyntöön, jossa on pelipalvelinavaimen otsake. Metagame
  hylkää ylläpitäjän avaimen jokaisessa pyynnössä, jossa on välityspalvelimen otsake, ja hyväksyy
  pelipalvelinavaimen vain omalta koneeltaan (loopback tai jokin sen omista osoitteista;
  `GAMESERVER_ALLOW_FROM` lisää sallittuja koneita). Lähetä ylläpitokutsut suoraan metagamen omaan
  osoitteeseen: palvelimella tai yksityisessä tilassa Tailscalen yli. Ylläpitäjätilin avain ja
  pelipalvelinavain ovat salaisuuksia: älä koskaan jaa niitä äläkä tallenna niitä versionhallintaan.
- **Yhdyskäytävän salaisuus.** `GATEWAY_SECRET` on sama metagamen ja yhdyskäytävän asetuksissa, ja se
  todistaa, että pyyntö tuli yhdyskäytävältä. Se on salaisuus: älä koskaan jaa sitä äläkä tallenna sitä
  versionhallintaan.

## Paketin luomat palomuurisäännöt {#kit-firewall}

Asennusohjelma sijoittaa sääntönsä Windowsin palomuurin ryhmään **Dauntless Revived** ja rakentaa ne
uudelleen jokaisella ajokerralla: julkisessa tilassa kaikki paitsi sallittujen listan säännön, jonka
osoitteista ja päälle/pois-tilasta vastaa sallittujen listan apuri (asennusohjelma palauttaa siitä vain
protokollan, portit ja ohjelman), ja yksityisessä tilassa kaikki. Sääntöjen nimissä näkyvät
todelliset porttinumerot; alla olevissa taulukoissa on oletusportit. `-WhatIf` luettelee muutokset
tekemättä niitä.

### Julkinen tila {#firewall-public}

| Sääntö | Protokolla ja portti | Ohjelma | Mistä | Tila |
|:-------|:---------------------|:--------|:------|:-----|
| Dauntless Revived - gateway (TCP 443) | TCP, yhdyskäytävän portti | `node.exe` | Mistä tahansa osoitteesta | Käytössä, kaikki profiilit |
| Dauntless Revived game ports (allowlist); säännön nimi `DauntlessRevived-GamePorts-Allowlist` | UDP 8770-8777 | pelin exe-tiedosto | Sallittujen listan apuri asettaa (luodaan paikkamerkkiosoitteella 192.0.2.1) | Luodaan pois käytöstä. Apuri ottaa sen käyttöön, kun vähintään yksi osoite on sallittu, ja poistaa käytöstä, kun lista on tyhjä. |
| Dauntless Revived - SSH (TCP 22) | TCP 22 | mikä tahansa | Mistä tahansa osoitteesta | Luodaan vain, jos OpenSSH on asennettu eikä mikään käytössä oleva sääntö jo salli TCP-porttia 22 |

Metagamelle, sisältöpalvelimelle, deploy-palvelimelle tai sallittujen listan apurille ei luoda sääntöä.
Asennusohjelma muuttaa myös olemassa olevia asetuksia ja tallentaa vanhat arvot `server.json`-tiedoston
kohtaan `FirewallChanges`:

- **Etätyöpöytä.** Kun `-AdminIp` on annettu, jokainen käytössä oleva saapuvan liikenteen sääntö TCP-
  tai UDP-portille 3389 rajataan näihin osoitteisiin. Ilman sitä kaikille osoitteille avoimet säännöt
  poistetaan käytöstä, ellei annettu `-KeepRdpOpen`.
- **Profiilit.** Jokainen palomuuriprofiili otetaan käyttöön niin, että saapuva liikenne on oletuksena
  estetty ja lähtevä sallittu.
- **Käytäntöarvot.** Avaimen `HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall\<Profile>` alta
  poistetaan `EnableFirewall`-, `DefaultInboundAction`- ja `AllowLocalPolicyMerge`-arvot, jotka ovat 0,
  koska ne ohittavat paikalliset asetukset. Jos voimassa oleva palomuuri on silti yhä pois päältä,
  asennusohjelma tulostaa **RESTART THIS SERVER NOW** (käynnistä palvelin uudelleen nyt).

Muista käytössä olevista säännöistä, jotka hyväksyvät yhteyksiä mistä tahansa osoitteesta
Public-profiilissa, sekä muista `node.exe`:n tai pelin exe-tiedoston saapuvan liikenteen säännöistä
asennusohjelma vain varoittaa.

Sallittujen listan sääntö tarkemmin:

- Osoite pysyy säännössä, kunnes sen viimeisestä kirjautumisesta tai elonmerkistä on kulunut 600
  sekuntia (`ALLOWLIST_TTL_SECONDS`). Vain yksittäiset julkiset osoitteet hyväksytään (hiekkalaatikko
  hyväksyy myös yksityiset, `ALLOWLIST_ALLOW_PRIVATE=1`), enintään 256 kerrallaan
  (`ALLOWLIST_MAX_ENTRIES`).
- Apuri tallentaa listan, joten uudelleenkäynnistyksen jälkeen portit aukeavat samoille pelaajille,
  kunnes heidän aikansa kuluu umpeen.
- Sääntö sulkeutuu, kun apuri pysähtyy normaalisti, ja myös järjestelmänvalvojana ajettu täysi
  `Stack.ps1 stop` poistaa sen käytöstä.
- Jos sääntö joskus poistetaan, apuri luo sen uudelleen ilman ohjelmasuodatinta ja ryhmän
  ulkopuolelle. Aja asennusohjelma uudelleen, niin suodatin palaa.
- Apuri poistaa käytöstä kaikki muut säännöt, joilla on sama näyttönimi, ja asennusohjelma poistaa
  sellaiset säännöt kokonaan.
- Asetuksella `GATEWAY_ALLOWLIST=0` yhdyskäytävä ei ilmoita osoitteita, joten peliportit eivät aukea
  kenellekään.

### Yksityinen tila {#firewall-private}

Ryhmän säännöt korvataan näillä. Kaikki ovat käytössä kaikissa profiileissa ja sallivat liikenteen
vain osoitteista 100.64.0.0/10, ja ne sidotaan Tailscale-verkkosovittimeen, jos sellainen löytyy:

| Sääntö | Protokolla ja portti | Ohjelma |
|:-------|:---------------------|:--------|
| Dauntless Revived - metagame (TCP 61000, Tailscale only) | TCP, metagamen portti | `node.exe` |
| Dauntless Revived - content server (TCP 61002, Tailscale only) | TCP, sisältöpalvelimen portti (vain jos sisältöpalvelin on asennettu) | `node.exe` |
| Dauntless Revived - game servers (UDP 8770-8777, Tailscale only) | UDP 8770-8777 | pelin exe-tiedosto |

Deploy-palvelin ei saa sääntöä. Profiileihin, SSH:hon ja etätyöpöytään ei kosketa; asennusohjelma vain
varoittaa, jos jokin profiili on pois päältä tai sallii saapuvan liikenteen oletuksena. Se myös vaihtaa
Tailscale-sovittimen verkkoluokan Private-luokasta Public-luokkaan, jotta yksityisille verkoille
tarkoitetut säännöt eivät koske Tailscale-verkon koneita.

### Hiekkalaatikko {#firewall-sandbox}

`-Sandbox` ei muuta palomuurin asetuksia. Kaikki kuuntelee osoitteessa `127.0.0.1`, ja sallittujen
listan apuri toimii koeajotilassa (dry run): se vain kirjaa lokiin palomuurimuutokset, jotka se tekisi.

### Palveluntarjoajalla {#firewall-provider}

Monella palveluntarjoajalla on oma palomuurinsa palvelimen edessä. Salli siellä julkista tilaa varten
TCP 443 (tai oma `-GatewayPort`-arvosi) ja UDP 8770-8777 mistä tahansa osoitteesta, sekä TCP 22 ja 3389
vain omasta osoitteestasi. Jos palvelimen julkinen osoite ei ole sen omassa verkkosovittimessa
(1:1-NAT), palveluntarjoajan on ohjattava samat portit palvelimelle. Ilman UDP-sääntöä kaverit
kirjautuvat ja jäävät sitten jumiin Ramsgatea ladatessa. Katso sivulta
[Windows-palvelin]({{ winserver_page.url | relative_url }}) kohta ”Ennen kuin kaverit liittyvät”.

### Käsin tehdyt asennukset {#firewall-hand}

Kehityskone ei tarvitse sääntöjä. Käsin rakennetulle yksityisen tilan palvelinkoneelle sivulla
[Palvelin ryhmälle]({{ admin_page.url | relative_url }}#firewall-allow-only-the-tailscale-interface)
on kaksi lisättävää sääntöä (TCP 61000 ja UDP 8770-8777, vain Tailscale). Jos ajat myös
sisältöpalvelinta, lisää samalla tavalla kolmas sääntö TCP-portille 61002. Älä koskaan avaa
TCP-porttia 61001.

Paketin sääntöjen poistaminen neuvotaan sivun [Windows-palvelin]({{ winserver_page.url | relative_url }})
kohdassa ”Poistaminen”.

## Lähtevät yhteydet {#outbound}

Kun paketti asentaa tai päivittää palvelimen, palvelin ottaa yhteyksiä ulospäin HTTPS:llä: nodejs.orgiin
(Node.js), Microsoftille (Visual C++- ja DirectX-ajonaikaiset kirjastot), npm-rekisteriin (`npm ci`),
osoitteeseen pkgs.tailscale.com (yksityinen tila), GitHubiin (vain kun palvelinkoodi haetaan sieltä)
sekä `-GameZipUrl`-osoitteen palvelimelle. `Deploy-Remote.ps1` ottaa omalta koneeltasi yhteyden
palvelimeen SSH:lla (`-SshPort`, oletus 22). Paketin SSH-palomuurisääntö käyttää aina TCP-porttia 22,
joten jos sshd kuuntelee muussa portissa, salli se portti itse. Käynnistin tarkistaa omat päivityksensä
projektin GitHub-julkaisuista.

## Testien ja hiekkalaatikon portit {#test-ports}

Testisarjat ja paketin hiekkalaatikko kuuntelevat loopbackissa porteissa 62000-62999, eivätkä koskaan
käytössä olevan palvelimen porteissa 61000-61099. Useampi sarja käyttää samoja porttinumeroita, joten
aja koneella vain yksi sarja kerrallaan, äläkä silloin, kun hiekkalaatikkoasennus tai
`Test-Sandbox.ps1` on käynnissä. Testien ajaminen neuvotaan sivulla
[Kehittäjän opas]({{ dev_page.url | relative_url }}).

| Sarja | Portit |
|:------|:-------|
| `legacy/UndauntedMetagame/`, `npm test` | 62014, 62015-62016, 62471-62472, 62481-62483, 62501-62502, 62901-62904, 62921-62922; chat-testien omat kuuntelijat ottavat satunnaisen vapaan portin |
| `legacy/UndauntedDeployServer/`, `npm test` | 62013, 62473 |
| `legacy/UndauntedGateway/`, `npm test` | 62400-62499 (käytössä: 62400-62405, 62409-62417, 62420-62422, 62430-62436) |
| `legacy/UndauntedContent/`, `npm test` | 62011, 62012, 62019 |
| `legacy/UndauntedContent/`, `npm run test:integration` | 62002 ja 62003 (`CONTENT_IT_PORT`, `CONTENT_IT_MOCK_PORT`) |
| `legacy/UndauntedLauncher/`, `npm test` | 62012, 62013, 62401-62404, 62409, 62420-62422, 62429, 62440-62444 |
| `deploy/windows-server/tests/Test-KitUnit.ps1` | 62450 ja 62451 (`-Port` ja sitä seuraava portti, 62000-62499) |
| `-Sandbox`-asennus ja `Test-Sandbox.ps1` | Metagame 62000, sisältöpalvelin 62002, sallittujen listan apuri 62005, yhdyskäytävä 62443, chat (yhdyskäytävän WebSocket-kohde ja metagamen `CHAT_PORT`) 62099. 62001 kirjoitetaan deploy-palvelimelle, joka ei pyöri hiekkalaatikossa. |

Tunnetut päällekkäisyydet: sisältöpalvelimen integraatiotesti ja hiekkalaatikko käyttävät kumpikin
porttia 62002, käynnistimen controller-testi käyttää porttia 62443 kuten hiekkalaatikon yhdyskäytävä,
ja käynnistimen testit jakavat portit 62012, 62013, 62401-62404, 62409 ja 62420-62422 sisältöpalvelimen,
deploy-palvelimen ja yhdyskäytävän testien kanssa.

## Mikä kuuntelee: tarkistus {#checking}

Paketin palvelimella `C:\DauntlessRevived\bin\Stack.ps1 status` näyttää jokaisen osan prosesseineen ja
portteineen, pelipalvelimet UDP-portteineen (se etsii ne alueelta 8700-8799 ja nimeää portin
`UdpPortEnd` Ramsgateksi ja sitä edeltävän Dojoksi) sekä sallittujen listan. `Stack.ps1 start` odottaa
enintään 30 sekuntia, että kukin osa kuuntelee porttiaan, ja enintään 60 sekuntia Ramsgatea
UDP-porttiin 8777. Millä tahansa Windows-koneella:

```powershell
Get-NetTCPConnection -State Listen -LocalPort 443,61000,61001,61002,61005,61099 -ErrorAction SilentlyContinue |
  ForEach-Object { "TCP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -ge 8770 -and $_.LocalPort -le 8777 } |
  ForEach-Object { "UDP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
```

Kokoonpanoosi kuuluvissa TCP-porteissa pitäisi näkyä `node` kohdan
[Kuunteluosoitteet kokoonpanoittain](#bind-addresses) mukaisissa osoitteissa, ja jokaiselle käynnissä
olevalle pelipalvelimelle `Dauntless-Win64-Shipping` osoitteessa `0.0.0.0`. Portissa 61099 kuuntelee
`node` (metagame) osoitteessa `127.0.0.1` vain, kun chat on päällä; `Stack.ps1 status` kertoo sen
`chat`-rivillään.
