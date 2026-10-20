---
title: Palvelin ryhmälle
parent: Asennus
grand_parent: Dauntless Revived suomeksi
nav_order: 4
description: "Dauntless Revived -palvelin kavereille: Tailscale-jako, palomuurisäännöt, kutsukoodit, ylläpitäjän tilit, kapasiteetti ja tietokannan varmuuskopiot."
lang: fi
ref: setup/admin
locale: fi_FI
---

{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}
{% assign legal_page = site.pages | where: "path", "fi/legal.md" | first %}
{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign scripts_page = site.pages | where: "path", "fi/reference/scripts.md" | first %}

# Palvelin ryhmälle
{: .no_toc }

Tällä sivulla kerrotaan, miten avaamme Undauntediin perustuvan palvelinkokonaisuutemme (peliversio
**1.4.4**) muutamalle kaverille: verkkoyhteys, palomuuri, osoitteet, tilit, kapasiteetti ja
varmuuskopiot. Oletuksena on, että kokonaisuus pyörii jo paikallisesti sinulla: metagame
(taustapalvelu, joka hoitaa tilit ja hahmot), deploy-palvelin (ohjelma, joka käynnistää
pelipalvelimet) ja Ramsgate-palvelin.

**Tilanne (22.9.2026).** Omistajan koneella kokonaisuutemme pyörii vain koneen sisällä (loopback) ja
ainoastaan omistajalle. Alla olevaa Tailscale-kokoonpanoa ei ole vielä ajettu alusta loppuun:
ensimmäisessä kaveritestissä käytämme sen sijaan
[Windows-palvelinpakettia]({{ winserver_page.url | relative_url }}) julkisessa tilassa vuokratulla
palvelimella (ks. [tiekartta]({{ roadmap_page.url | relative_url }}), kohdat 1.3, 1.15 ja 1.17).
Kaikki, mikä on suunniteltu mutta ei vielä rakennettu, on merkitty sellaiseksi. Tämän kokoonpanon
toinen puoli on sivu [Liity kaverina]({{ friends_page.url | relative_url }}).

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Mitä palvelinkoneella pyörii {#what-runs-on-the-host}

| Osa | Kuuntelee | Kenen on tavoitettava se |
|:----------|:-----------|:--------------------|
| Metagame (`legacy/UndauntedMetagame`: Node, Express, SQLite) | TCP 61000 | Jokaisen pelaajan peliohjelman, jokaisen pelipalvelimen ja sinun ylläpitäjänä |
| Deploy-palvelin (`legacy/UndauntedDeployServer`) | TCP 61001, vain loopback | Vain samalla koneella olevan metagamen. **Siinä ei ole tunnistautumista.** |
| Ramsgate-palvelin | UDP 8777 (`PORT_RANGE_END`) | Pelaajien. Aina käynnissä. Jos se kuolee, deploy-palvelin käynnistää sen uudelleen, kun pelaaja matkustaa sinne, tai sen vahtikoira (watchdog) tekee sen minuutin sisällä. |
| Training Dojo | UDP 8776 | Pelaajien. Forkimme käynnistää sen ensimmäisellä käyttökerralla; `ENABLE_DOJO=1` palauttaa alkuperäisen aina päällä -toiminnan. |
| Metsästyspalvelimet | UDP 8770–8775 | Pelaajien. Yksi jokaista enintään neljän hengen ryhmää kohden. Kukin sulkeutuu, kun siihen ei ole ollut kukaan yhteydessä yhteensä 50 sekuntiin. |

Pelipalvelimet ovat saman 1.4.4-peliohjelman lisäkopioita. Deploy-palvelin käynnistää ne valitsimilla
`-server -nullrhi`, kaksi DLL-tiedostoa latautuu jokaiseen, eivätkä ne tarvitse näytönohjainta.
Alkuperäisen projektin käynnistin odottaa paikallista metagamea portissa 60000, mutta palvelinkoneellamme
toinen ohjelma varasi jo portin 60000, joten käytämme portteja 61000 ja 61001.

## Miksi Tailscale eikä porttiohjaus {#why-tailscale-instead-of-port-forwarding}

Porttiohjaus (port forwarding, reitittimen asetus, joka päästää internetistä liikennettä suoraan
koneellesi) toisi kaiken seuraavan julkiseen internetiin:

- **Kaikki on tavallista HTTP:tä.** Client-DLL kirjoittaa jokaisen taustapalvelun osoitteen muotoon
  `http://<metagame address>/...`. Koko kokonaisuudessa ei ole TLS-salausta missään.
- **Tiliavaimet kulkevat salaamattomina.** Kirjautuessa peli lähettää pelaajan avaimen
  `exchange_code`-kenttänä osoitteeseen `/account/api/oauth/token`. Käynnistin ja ylläpitorajapinta
  lähettävät sen `x-undaunted-user-api-key`-otsakkeessa. Takaisin tulee 24 tuntia voimassa oleva
  RS256-bearer-tunniste, joka antaa täyden pääsyn kyseisen pelaajan tietoihin.
- `POST /undaunted/api/Register` ei vaadi tunnistautumista, eikä mikään metagamessa rajoita pyyntöjen
  määrää.
- Deploy-palvelimessa ei ole tunnistautumista lainkaan. Se kuuntelee vain loopbackissa ja vastaa 403
  muilta koneilta tuleville kutsujille, ja vain nämä kaksi tarkistusta estävät muita saamasta koneesi
  käynnistämään peliprosesseja TCP-portin 61001 kautta.
- Pelipalvelimet ovat vuoden 2020 peliversio, johon on ladattu DLL-tiedosto, eivätkä ne ole
  turvallisuutta varten vahvistettuja verkkopalveluita.

Tailscale välttää kaiken tämän. Jokaisen kaverin liikenne isännälle salataan WireGuardilla, ja vain
ne, joille olet jakanut koneen, voivat tavoittaa sen. Et tarvitse muutoksia reitittimeen etkä julkista
IP-osoitetta, joten se toimii myös operaattoritason NAT:n (carrier-grade NAT) takana. Jaon poistaminen
katkaisee kaverin yhteyden heti. Hinta: jokainen kaveri asentaa Tailscalen, ja osa yhteyksistä kulkee
Tailscalen DERP-välityspalvelimen kautta suoran reitin sijaan. Välityspalvelimet toimivat, mutta viive
on suurempi.

## Jaa palvelinkone Tailscalella {#share-the-host-pc-with-tailscale}

1. Asenna Tailscale palvelinkoneelle ja kirjaudu sisään. `tailscale ip -4` tulostaa isännän
   `100.x.y.z`-osoitteen. Osoite pysyy samana niin kauan kuin kone pysyy tailnetissäsi
   (Tailscale-verkossasi). Kavereiden skriptit ja `MY_IP` riippuvat molemmat siitä.
2. Avaa Tailscalen hallintakonsolissa palvelinkoneen valikko, valitse **Share** ja kutsu jokainen kaveri
   sähköpostilla tai linkillä. Jokainen kaveri hyväksyy kutsun omalla Tailscale-tilillään. Älä lisää
   kavereita tailnetisi käyttäjiksi. Jako näyttää heille tämän yhden koneen eikä mitään muuta.
3. Aseta Tailscale käynnistymään Windowsin mukana ja toimimaan valvomatta (unattended), jotta osoite on
   olemassa ennen kuin kokonaisuus käynnistyy (katso [Vaihda osoitteet](#switch-the-addresses-to-tailscale)).
4. Valinnainen: tailnetin käytäntösäännöt voivat rajata sen, mihin jaon vastaanottajat pääsevät tällä
   koneella, osoitteisiin `tcp:61000` ja `udp:8770-8777`. Emme ole selvittäneet jaettujen käyttäjien
   tarkkaa käytäntösyntaksia (varmistamatta). Katso Tailscalen pääsynhallinnan ohjeet. Alla olevat
   Windowsin palomuurisäännöt ovat se suoja, johon luotamme.
5. Pyydä jokaista kaveria ajamaan kerran `tailscale ping <host address>`. Vastaus `via DERP(...)`
   tarkoittaa, että yhteys kulkee välityspalvelimen kautta eikä suoraan.

Kaverin jaon poistaminen on tällä hetkellä ainoa täydellinen tapa lukita joku ulos. Katso
[Puuttuvat ylläpitotoiminnot](#missing-admin-functions-and-workarounds).

## Palomuuri: salli vain Tailscale-liitäntä {#firewall-allow-only-the-tailscale-interface}

Windows Defenderin palomuuri estää saapuvan liikenteen oletuksena. Palvelinkoneellamme ei ollut lainkaan
saapuvan liikenteen sääntöjä pelille tai Nodelle. Lisää täsmälleen kaksi sallivaa sääntöä, kumpikin
sidottuna yhteen ohjelmaan, Tailscale-liitäntään ja Tailscalen osoitealueeseen. Aja nämä
järjestelmänvalvojan oikeuksin avatussa PowerShellissä:

```powershell
$game = "C:\D144\Dauntless\Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe"
$node = "C:\Program Files\nodejs\node.exe"

New-NetFirewallRule -DisplayName "Dauntless Revived - metagame (Tailscale)" `
  -Direction Inbound -Action Allow -Protocol TCP -LocalPort 61000 `
  -Program $node -InterfaceAlias Tailscale -RemoteAddress 100.64.0.0/10

New-NetFirewallRule -DisplayName "Dauntless Revived - game servers (Tailscale)" `
  -Direction Inbound -Action Allow -Protocol UDP -LocalPort 8770-8777 `
  -Program $game -InterfaceAlias Tailscale -RemoteAddress 100.64.0.0/10
```

- **Älä koskaan avaa TCP-porttia 61001.** Deploy-palvelimessa ei ole tunnistautumista.
- Tarkista komennolla `Get-NetAdapter`, että verkkosovittimen nimi on todella `Tailscale`.
- Jos olet joskus vastannut Windowsin ”salli käyttö” -kysymykseen `node.exe`-ohjelman tai pelin
  kohdalla, Windows loi ohjelmalle omat sääntönsä. Tuon kysymyksen luoma salliva sääntö avaa ohjelman
  kaikille kyseisen verkkotyypin liitännöille. Estävä sääntö menee yllä olevien sallivien sääntöjen
  edelle. Listaa ne ja poista ne, joita et halua:

  ```powershell
  Get-NetFirewallApplicationFilter -Program $node, $game | Get-NetFirewallRule |
    Format-Table DisplayName, Direction, Action, Enabled, Profile
  ```

- Tarkista, minkä verkkoluokan Windows antoi Tailscale-sovittimelle, komennolla
  `Get-NetConnectionProfile -InterfaceAlias Tailscale`. Jos se on `Private`, kaikki yksityisille
  verkoille sallimasi säännöt (esimerkiksi tiedostojen jakaminen) koskevat myös jaon vastaanottajia.
  Asetus `Public` välttää tämän. Emme ole tarkistaneet, palauttaako Tailscale luokan, kun se
  muodostaa yhteyden uudelleen (varmistamatta).
- Testaa kaverin koneelta komennolla
  `Invoke-RestMethod http://<host address>:61000/undaunted/api/RegistrationStatus`. Saman pyynnön
  pitäisi epäonnistua, kun sen tekee kotiverkkosi (lähiverkon) toiselta laitteelta.

## Vaihda osoitteet Tailscaleen {#switch-the-addresses-to-tailscale}

Kolme asetusta osoittaa nykyään osoitteeseen `127.0.0.1`, ja ne on muutettava:

| Asetus | Tiedosto | Uusi arvo | Miksi |
|:--------|:-----|:----------|:----|
| `MY_IP` | `legacy/UndauntedDeployServer/.env` | `<host 100.x address>` | Ainoa isännän osoite, jonka deploy-palvelin antaa Ramsgatea, Dojoa ja metsästyksiä varten matchmaking-tuloksen kautta. Arvolla `127.0.0.1` kaverin peliohjelma matkaisi omalle koneelleen. |
| `QOS_TARGET_URL` | `legacy/UndauntedMetagame/.env` | `http://<host 100.x address>:61000/QoS` | `/candidate/regions` palauttaa tämän osoitteen alueena, jota pingataan. Arvolla `127.0.0.1` kaverit pingaavat omaa konettaan (localhost). Emme ole testanneet, mitä peliohjelma tekee, kun tuo ping epäonnistuu. |
| `BIND_HOST` | `legacy/UndauntedMetagame/.env` | `<host 100.x address>` | Osoite, jossa metagame kuuntelee. Forkimme oletus on `127.0.0.1`; alkuperäinen projekti kuunteli kaikissa liitännöissä. |
| `BIND_HOST` | `legacy/UndauntedDeployServer/.env` | pysyy arvossa `127.0.0.1` | Ei tunnistautumista. |
| `DEPLOYSERVER_URL` | `legacy/UndauntedMetagame/.env` | pysyy arvossa `127.0.0.1:61001` | Metagame tavoittaa deploy-palvelimen paikallisesti. |

**Hankaluus: metagame kuuntelee yhdessä osoitteessa**, ja myös kaksi paikallista osaa puhuu sille.

1. **Oma peliohjelmasi.** Käynnistä se Tailscale-osoitetta vasten. Isännän käynnistimemme ottaa
   osoitteen muodossa `play.ps1 -Backend "<host 100.x address>:61000"` (kavereiden skriptissä sama
   parametri on nimeltään `-Server`). Liikenne pysyy koneellasi.
2. **Pelipalvelimet.** DLL kirjoittaa osoitteet uudelleen vain client-tilassa. Palvelintilassa se ei
   koskaan kaappaa asetuksia. Pelipalvelinprosessit lukevat sen sijaan osoiteohitukset isäntätilin
   käyttäjäasetuksista, tiedoston `%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\Game.ini` osiosta
   `[OnlineSubsystemPhoenix]`. Palvelinkoneellamme siinä osiossa on 167 merkintää muotoa
   `AuthEndpoint="http://127.0.0.1:61000/game/login"`. Niiden on osoitettava osoitteeseen, jossa
   metagame kuuntelee. Varmuuskopioi tiedosto ja kirjoita se sitten uudelleen:

   ```powershell
   $ts = "100.x.y.z"      # the host's Tailscale address
   $gi = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient\Game.ini"
   Copy-Item $gi "$gi.bak"
   (Get-Content $gi) -replace '127\.0\.0\.1:61000', "${ts}:61000" | Set-Content $gi -Encoding ASCII
   ```

   Emme ole selvittäneet, mikä prosessi kirjoittaa tuon osion. Sen aikaleima osui yksiin sekä
   peliohjelman siirtymisen että palvelimen käynnistymisen kanssa (varmistamatta). Tarkista
   ensimmäisen pelikertasi jälkeen, että se osoittaa yhä sinne, minne odotat. Palvelin, joka ei
   tavoita metagamea, ei voi ladata eikä tallentaa hahmoja.

Tämän vuoksi **Tailscalen yhteyden on oltava päällä ennen kuin metagame käynnistyy.** Jos osoitetta ei
vielä ole, forkimme sulkeutuu viestillä `Could not listen on <address>:61000`. Alkuperäinen projekti
ilmoitti silti onnistuneensa ja sulkeutui sitten hiljaa.

Älä kierrä tätä asetuksella `BIND_HOST=0.0.0.0`. Metagame kuuntelisi silloin myös lähiverkossasi, ja
vain palomuuri olisi lähiverkon laitteiden ja tunnistautumattoman rekisteröintiosoitteen välissä.

Forkiin suunniteltu:

- `BIND_HOST` ottaa listan (`127.0.0.1,<100.x>`), jolloin paikallinen liikenne pysyy loopbackissa
  eikä `Game.ini`-tiedoston uudelleenkirjoitusta enää tarvita.
- Asetusten kaappaus (hook) myös palvelintilassa, jotta pelipalvelimet eivät enää ole riippuvaisia
  omistajan `Game.ini`-tiedostosta.

### Uudelleenkäynnistys muutoksen jälkeen {#restarting-after-a-change}

Molemmat palvelut lukevat `.env`-tiedoston vain käynnistyessään (`npm run start`, joka ajaa
`node --env-file=.env dist/server.js`).

1. Pysäytä deploy-palvelin ensin. Muuten sen vahtikoira tai pelaajan matka Ramsgateen käynnistää uuden
   Ramsgaten.
2. Tarkista, onko pelipalvelimia jäänyt käyntiin. Normaalisti ne päättyvät deploy-palvelimen mukana:
   se käynnistää ne Noden oletusarvoisella (ei irrotetulla eli not detached) `spawn`-kutsulla, ja
   Windowsissa Node laittaa tällaiset lapsiprosessit työobjektiin (job object), joka suljetaan, kun
   emoprosessi sulkeutuu. Varmistimme tämän testiprosessilla sekä normaalissa sulkeutumisessa että
   pakotetussa lopetuksessa. Käsin käynnistetyt palvelimet, esimerkiksi testiskriptillämme
   käynnistetyt, eivät kuulu tähän, ja ne pitävät UDP-porttinsa. Pelipalvelimen komentorivin
   ensimmäinen parametri on pelipalvelinavain, joten älä tulosta niiden komentorivejä. Oma
   peliohjelmasi ajaa samaa exe-tiedostoa ilman `-server`-valitsinta, ja tämä suodatin jättää sen
   rauhaan:

   ```powershell
   Get-CimInstance Win32_Process -Filter "Name = 'Dauntless-Win64-Shipping.exe'" |
     Where-Object { $_.CommandLine -match '\s-server(\s|$)' } |
     ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
   ```

3. Pysäytä metagame.
4. Käynnistä metagame sen omasta kansiosta, koska sen migraatioiden (tietokannan rakennepäivitysten)
   polku on suhteessa työkansioon. Käynnistä sitten deploy-palvelin, joka käynnistää Ramsgaten heti.

Metagamen uudelleenkäynnistys ei kirjaa pelaajia ulos, koska kirjautumistunnisteet ovat tilattomia,
24 tuntia voimassa olevia JWT-tunnisteita. Muistissa oleva tila kuitenkin katoaa: matchmaking-jonot ja
lista siitä, kuka on paikalla.

## Tilit: tunnistautuminen, rekisteröinti ja kutsukoodit {#accounts-authentication-registration-and-invite-codes}

Kaksi metagamen `.env`-asetusta ratkaisee, kuka pääsee sisään:

- `AUTH_MODE=APIKEY` yhdessä asetuksen `NODE_ENV=production` kanssa. Toinen tila, `AUTH_MODE=NONE`,
  hyväksyy käyttäjätunnisteeksi mitä tahansa peliohjelma lähettää, joten kuka tahansa voi kirjautua
  kenenä tahansa. Sitä noudatetaan vain tuotantotilan ulkopuolella. Älä koskaan käytä sitä kavereiden
  kanssa.
- `REGISTRATION_MODE`:

| Tila | `POST /undaunted/api/Register` |
|:-----|:-------------------------------|
| `NONE` | Aina hylätty (400). |
| `INVITECODE` | Vaatii kelvollisen koodin. Väärä tai jo käytetty koodi saa vastauksen 401. |
| `OPEN` | Kuka tahansa, joka tavoittaa metagamen, saa tilin. |

Aseta `.env`-tiedostoon `INVITECODE` **ennen** kuin metagame kuuntelee Tailscalessa.
Sivun [Pystytä palvelin]({{ host_page.url | relative_url }}#metagame) `.env`-tiedostossa on arvo
`OPEN`, mikä on vaaratonta vain niin kauan kuin metagame kuuntelee loopbackissa. (Windows-palvelinpaketti
kirjoittaa aina `INVITECODE`.) Ylläpitorajapinta voi vaihtaa tilan ajon aikana, mutta vain muistissa:
uudelleenkäynnistys palauttaa `.env`-tiedoston arvon.

Rekisteröinti palauttaa uuden pelaajan avaimen (`UUK_` ja perässä 48 heksadesimaalimerkkiä) kerran.
Palvelin säilyttää siitä vain SHA-256-tiivisteen (avaimesta laskettu sormenjälki, josta avainta ei voi
palauttaa).

### Kutsukoodien luominen {#making-invite-codes}

Ylläpitokutsut tunnistetaan ylläpitäjätilin omalla avaimella `x-undaunted-user-api-key`-otsakkeessa,
ja ne toimivat vain suoraan metagamea vasten, eivät koskaan välityspalvelimen (proxy) tai julkisen
yhdyskäytävän kautta. Meidän kokoonpanossamme omistajan avain on tiedostossa `C:\dr\data\owner.key`.
Tämä lohko pyytää palvelimelta uuden kertakäyttöisen koodin, listaa kaikki koodit ja peruu yhden:

```powershell
$M = "100.x.y.z:61000"    # wherever the metagame listens
$h = @{ "x-undaunted-user-api-key" = (Get-Content C:\dr\data\owner.key -Raw).Trim() }

# new single-use code, made by the server (XXXX-XXXX-XXXX)
$code = (Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/CreateInvite" -Headers $h `
  -ContentType "application/json" -Body (@{ uses = 1 } | ConvertTo-Json) -TimeoutSec 10).code
$code        # send this to one friend, privately

# list, and revoke
(Invoke-RestMethod -Uri "http://$M/undaunted/api/InviteCodes" -Headers $h -TimeoutSec 10).InviteCodes
Invoke-RestMethod -Method Delete -Uri "http://$M/undaunted/api/InviteCode/$code" -Headers $h -TimeoutSec 10
```

- `CreateInvite` tekee kolme neljän merkin ryhmää Crockfordin base32-aakkostosta (60 satunnaista
  bittiä). `uses` on 1–1000. Valinnainen `name` on muistiinpano metagamen lokiin, eikä sitä
  tallenneta; lokissa näkyy vain koodin ensimmäinen ryhmä. Vanhempi `RegisterInviteCode` tallentaa
  yhä itse valitsemasi koodin. Paketilla asennetulla palvelimella `New-Invite.ps1` tekee kaiken tämän
  puolestasi.
- Koodit kulutetaan atomisesti eli yhtenä jakamattomana toimintona: yksi SQL-lause `UPDATE` tarkistaa
  ja vähentää jäljellä olevat käyttökerrat, joten kaksi ihmistä ei voi kumpikin lunastaa samaa
  kertakäyttöistä koodia.
- Register tarkistaa ensin koodin, sitten käyttäjänimen, ja vasta sitten kuluttaa koodista yhden
  käyttökerran ja kirjoittaa tilin, kaikki yhdessä tietokantatransaktiossa. Hylätty tai varattu nimi
  ei polta koodia.
- Suosi kertakäyttöisiä koodeja, yksi kullekin kaverille, lähetettynä yksityisesti. Käytä monen
  käyttökerran koodeja (tai `RegisterInviteCode`-reitin `InfiniteUses`-asetusta) vain lyhyen aikaa,
  jos ollenkaan.

### Tilin tekeminen ylläpitäjäksi {#making-an-account-an-admin}

Ylläpitäjä on `users`-taulun rivi, jossa `isAdmin = 1`. Mikään rajapinta ei aseta sitä. Muokkaa
tietokantaa metagamen ollessa pysäytettynä, esimerkiksi pienellä `better-sqlite3`-skriptillä, jota
ajetaan `legacy/UndauntedMetagame`-kansiosta (`UPDATE users SET isAdmin = 1 WHERE userId = ?`). Ylläpitäjän
avain on yksinkertaisesti kyseisen käyttäjän tiliavain, joten pidä se palvelinkoneella.

## Ylläpitorajapinta {#admin-api}

Kaikki reitit ovat metagamen portissa polun `/undaunted/api` alla. ”Ylläpitäjä” tarkoittaa, että
`x-undaunted-user-api-key`-otsakkeen on kuuluttava käyttäjälle, jolla on `isAdmin`. Puuttuva tai
tuntematon avain saa vastauksen 401; kelvollinen avain, joka ei kuulu ylläpitäjälle, saa vastauksen 403.
**Ylläpitokutsu, jossa on välityspalvelimen otsake, saa vastauksen 403** jo ennen kuin avainta
edes tarkistetaan. Siksi ylläpitokutsut toimivat vain suoraan metagamea vasten: palvelinkoneella tai
yksityisessä tilassa koneelta, joka on samassa tailnetissä. Myös julkinen yhdyskäytävä torjuu ne.

Sivu [HTTP-rajapinta]({{ api_page.url | relative_url }}#undaunted-api) luettelee jokaisen reitin
runkoineen ja vastauksineen. Ryhmän palvelimen ylläpidossa eniten käytetyt:

| Metodi ja polku | Tunnistautuminen | Mitä se tekee |
|:----------------|:-----|:-------------|
| `GET /RegistrationStatus` | ei mitään | `{ "RegistrationMode": ... }` |
| `POST /RegistrationStatus` | ylläpitäjä | Runko `{ "RegistrationStatus": <mode> }`, jossa tila on `NONE`, `INVITECODE` tai `OPEN`. Vain muistissa. |
| `POST /CreateInvite` | ylläpitäjä | Runko `{ "uses", "name" }` (kumpikin valinnainen), palauttaa `{ "code" }`. |
| `GET /InviteCodes` | ylläpitäjä | Kaikki koodit jäljellä olevine käyttökertoineen. |
| `POST /RegisterInviteCode` | ylläpitäjä | Runko `{ "NewInviteCode", "Uses", "InfiniteUses" }`: itse valitsemasi koodi. |
| `DELETE /InviteCode/:code` | ylläpitäjä | Peruu koodin. |
| `GET /GetAllUsers` | ylläpitäjä | `{ "Users": [{ "Username", "UserId" }] }` |
| `POST /RenameUser` | ylläpitäjä | Runko `{ "UserId" }` tai `{ "Username" }` sekä `{ "NewUsername" }`. Nimeää tilin ja sen hahmot uudelleen. |
| `POST /GenerateJWTForUserId` | ylläpitäjä | Runko `{ "UserId" }`, palauttaa `{ "JWT" }`. Luo 24 tuntia voimassa olevan pelitunnisteen **kenelle tahansa käyttäjälle**, mikä käytännössä tarkoittaa, että voi pelata hänenä. Suhtaudu ylläpitäjän avaimiin sen mukaisesti. |
| `GET /PrivateOnlineStats` | ylläpitäjä | Pelaajakohtaisesti: kenttä, metsästys ja metsästyksen alkamisaika niille pelaajille, jotka on nähty viimeisten 90 sekunnin aikana. |
| `GET /SaveHistory`, `POST /RollbackCharacter`, `POST /RollbackLoadout` | ylläpitäjä | Pelaajan hahmojen ja varustesarjojen tallennetut versiot sekä yhden version palauttaminen. Pelaajan pitäisi olla poissa pelistä. |
| `POST /GrantEntitlement`, `POST /RevokeEntitlement` | ylläpitäjä | Antaa tai ottaa pois oikeuden (entitlement). |
| `POST /Register` | ei mitään (rekisteröintitila rajaa) | Runko `{ "Username", "InviteCode" }`. Palauttaa `{ "UUK" }`. |
| `GET /GetUserInfo` | käyttäjän avain | `{ "UserId", "Username", "IsAdmin" }` |
| `GET /ServerStatus` | ei mitään | Palvelimen nimi, versio ja lähdekoodi; käyttäjän avaimella myös se, kuka on paikalla. |

Kaverikäynnistimessä ei ole ylläpitonäkymää. Tee ylläpitokutsut suoraan, kuten tämän sivun
esimerkeissä, tai paketilla asennetulla palvelimella paketin skripteillä.

### Eteneminen (vain forkissa) {#progression}

Oikea eteneminen on oletuksena päällä: jokainen tili säilyttää oman Slayer-tasonsa, mestaruutensa
(mastery) ja Hunt Passinsa (`PROGRESSION_MODE`, katso [Pystytä palvelin]({{ host_page.url | relative_url }}#metagame)).
Siihen kuuluu kaksi ylläpitoreittiä:

| Metodi ja polku | Tunnistautuminen | Mitä se tekee |
|:----------------|:-----|:-------------|
| `GET /Progression?UserId=<tunnus>` | ylläpitäjä | Yhden tilin radat niillä tasoilla, jotka peli näyttää, sen tavoitteet (objectives), Hunt Pass ja oikeudet (entitlements), sekä tieto siitä, onko tili oikean etenemisen tilassa. |
| `POST /SeedProgression` | ylläpitäjä | Runko `{ "UserId", "Mode" }`. `grandfather` asettaa jokaisen radan korkeimmalle tasolleen täysin vahvistettuna (mitään ei jaeta); `fresh` asettaa jokaisen radan nollaan ja tyhjentää tavoitteet. Aja se, kun pelaaja ei ole pelissä. |

Palvelin, jolla oli pelaajia jo ennen kuin oikeasta etenemisestä tuli oletus, aloittaa heidät
Slayer-tasolta 1. [Päivitysohjeissa]({{ upgrade_page.url | relative_url }}) kerrotaan vaihtoehdot, ja
niissä on pieni skripti molempia reittejä varten.

### Sosiaaliset varareitit palvelimella (vain forkissa) {#social}

Kaverit, ryhmät ja killat toimivat itse pelissä; katso
[Liity kaverina]({{ friends_page.url | relative_url }}#friends-parties-and-guilds). Nämä viisi reittiä
tekevät samat asiat nimellä palvelimelta, jos pelin omat valikot pettävät (esimerkiksi ensimmäisessä
oikeassa testissä). Kuten kaikki ylläpitokutsut, ne toimivat vain suoraan metagamea vastaan, eivät
koskaan yhdyskäytävän kautta. `PartyInvite`, `Friends` ja `GuildInvite` toimivat avaimen omistajan
nimissä; ylläpitäjän avaimella voi nimetä toisen pelaajan kentässä `From`.

| Metodi ja polku | Tunnistus | Mitä se tekee |
|:----------------|:----------|:--------------|
| `POST /PartyInvite` | tiliavain (ylläpitäjä `From`-kentälle) | Runko `{ "Username", "From" }`: kutsuu pelaajan lähettäjän ryhmään pelin omin tarkistuksin. Pelaaja hyväksyy silti pelissä (PARTY INVITES, noin 10 sekunnin kuluessa). Vastaa `{ "From", "To" }`. |
| `POST /Friends` | tiliavain (ylläpitäjä `From`-kentälle) | Runko `{ "Username", "From" }`: lähettää kaveripyynnön tai hyväksyy sen, jonka pelaaja lähetti. Vastaa `{ "From", "To", "Result" }`. Toinen pelaaja näkee sen seuraavalla kirjautumisellaan. |
| `POST /GuildInvite` | tiliavain (ylläpitäjä `From`-kentälle) | Runko `{ "Username", "From" }`: kutsuu pelaajan lähettäjän kiltaan (lähettäjän on oltava sen johtaja tai upseeri). Pelaaja hyväksyy kohdassa GUILD INVITES seuraavalla kirjautumisellaan tai maailman latautuessa. Vastaa `{ "From", "To", "Guild" }`. |
| `GET /Guilds` | ylläpitäjä | Kaikki killat: `[{ "guildId", "name", "nameplate", "leader", "members" }]`, jossa `members` on lukumäärä. |
| `POST /DisbandGuild` | ylläpitäjä | Runko `{ "Guild" }`, tunnus tai nimi missä tahansa kirjainkoossa: poistaa killan, sen jäsenet ja kutsut. Vastaa `{ "Guild", "Members" }`. |

```powershell
$M = "127.0.0.1:61000"    # vuokrapalvelimella; yksityisessä tilassa Tailscale-osoite
$h = @{ "x-undaunted-user-api-key" = (Get-Content C:\dr\data\owner.key -Raw).Trim() }
$json = @{ ContentType = "application/json"; Headers = $h; TimeoutSec = 10 }

# kutsu kaveri ryhmääsi, lähetä kaveripyyntö, kutsu kiltaasi
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/PartyInvite" @json -Body (@{ Username = "Friend" } | ConvertTo-Json)
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/Friends" @json -Body (@{ Username = "Friend" } | ConvertTo-Json)
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/GuildInvite" @json -Body (@{ Username = "Friend" } | ConvertTo-Json)

# ylläpitäjänä toisen pelaajan puolesta: Friend2 kutsuu Friendin Friend2:n ryhmään
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/PartyInvite" @json -Body (@{ Username = "Friend"; From = "Friend2" } | ConvertTo-Json)

# listaa killat ja poista yksi
Invoke-RestMethod -Uri "http://$M/undaunted/api/Guilds" -Headers $h -TimeoutSec 10
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/DisbandGuild" @json -Body (@{ Guild = "SomeGuild" } | ConvertTo-Json)
```

Torjunta on 4xx-vastaus muodossa `{ "error", "message" }`. `Invoke-RestMethod` heittää siitä
poikkeuksen; viesti on kohdassa `$_.ErrorDetails.Message`.

| Reitti | `error` | Tilakoodi | Merkitys |
|:-------|:--------|:----------|:---------|
| kaikki viisi | ei mitään (tyhjä runko) | 401 | Ei avainta tai tuntematon avain. |
| kaikki viisi | ei mitään (tyhjä runko) | 403 | Ylläpitäjän avain välityspalvelimen kautta; reiteillä `Guilds` ja `DisbandGuild` avain, joka ei ole ylläpitäjän. |
| `PartyInvite`, `Friends`, `GuildInvite` | `forbidden` | 403 | `From` avaimella, joka ei ole ylläpitäjän. |
| `PartyInvite`, `Friends`, `GuildInvite` | `not_found` | 404 | Ei tiliä nimellä `Username` (tai `From`). |
| `PartyInvite` | `party_invite_refused` | 403, 404 tai 409 | Pelin oma torjunta, syy kentässä `message`: ei johtaja, täysi ryhmä, jo kutsuttu, esto, pelaaja hylkäsi kutsusi viimeisten 2 minuutin aikana, tai 20 kutsua lähetetty 10 minuutissa. |
| `Friends` | `self`, `blocked`, `limit`, `pending_limit`, `rate` | 400, 403 tai 409 | Oma tili; esto kumpaan tahansa suuntaan; 200 kaveria; 50 vastaamatonta pyyntöä; 20 uutta pyyntöä 10 minuutissa. |
| `GuildInvite` | `guild_refused` | 403, 404, 409 tai 429 | Killan oma torjunta: `message` alkaa sen koodilla (esimerkiksi `RedundantAdorableQuillshot:`, kun kutsu on jo avoinna; koodit ovat sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#guilds)). Tyhjä koodi kattaa eston, 24 tunnin tauon hylkäyksen jälkeen ja kutsujen rajat. |
| `DisbandGuild` | `not_found` | 404 | Ei kiltaa tällä tunnuksella tai nimellä. |
| kolme kiltareittiä | `guilds_off` | 404 | `GUILDS=0` on asetettu. |

### Puuttuvat ylläpitotoiminnot ja kiertotiet {#missing-admin-functions-and-workarounds}

Rajapintaa ei ole käyttäjän poistamiseen tai estämiseen, avaimen perumiseen tai uusimiseen eikä
ylläpitäjäksi ylentämiseen. Nimen vaihto on olemassa (`RenameUser`, katso
[Käyttäjänimet](#usernames)).

- **Jonkun lukitseminen ulos.** Poista hänen Tailscale-jakonsa, mikä katkaisee verkkoyhteyden heti.
  Poista sitten hänen rivinsä taulusta `userapikeys`. Hänen avaimensa lakkaa toimimasta seuraavalla
  kirjautumisella, mutta jo myönnetty tunniste pysyy voimassa enintään 24 tuntia.
- **Kaveri kadotti avaimensa.** Sitä ei voi palauttaa, koska vain sen tiiviste on tallessa. Anna uusi
  korvaamalla tiiviste. Uuden avaimen antava ylläpitotoiminto on suunnitteilla. Siihen asti tallenna
  tämä nimellä `rekey.js` kansioon `legacy/UndauntedMetagame`, jotta `require` löytää `better-sqlite3`-kirjaston:

  ```js
  // node rekey.js <UserId>   (take the UserId from GET /undaunted/api/GetAllUsers)
  const crypto = require("crypto");
  const fs = require("fs");
  const Database = require("better-sqlite3");
  const db = new Database("C:/dr/data/undaunted.db", { fileMustExist: true });
  const userId = process.argv[2];
  const key = "UUK_" + crypto.randomBytes(24).toString("hex");   // same format as Register
  const hash = crypto.createHash("sha256").update(key, "utf8").digest("hex");
  const r = db.prepare("UPDATE userapikeys SET keyHash = ? WHERE userId = ?").run(hash, userId);
  if (r.changes !== 1) { console.error("no such user"); process.exit(1); }
  fs.writeFileSync(`C:/dr/data/rekey-${userId}.txt`, key);   // hand it over privately, then delete
  console.log("new key written to C:/dr/data/rekey-" + userId + ".txt");
  ```

  **Älä anna uutta avainta `userapikeystoregister`-taulun kautta.** Käynnistyessään metagame ensin
  poistaa odottavat rivit ja sitten lisää jokaisen tauluun `userapikeys`, jossa `userId` on pääavain.
  Olemassa olevan käyttäjän kohdalla lisäys epäonnistuu, metagame ei koskaan ala kuunnella, ja odottava
  avain on jo poissa.

## Ominaisuuksien kytkeminen päälle {#switching-features-on}

Useat ominaisuudet on rakennettu, mutta ne ovat oletuksena pois päältä, ja muutamalla päällä olevalla
on kytkin, jolla sen saa pois. Kukin on yksi rivi metagamen asetuksissa: itse pystytetyllä koneella
`.env` kansiossa `legacy/UndauntedMetagame`, palvelinpaketin palvelimella
`C:\DauntlessRevived\data\config\metagame.env` (muokkaa järjestelmänvalvojana avatulla editorilla).
Paketti säilyttää lisäämäsi rivin uudelleenajoissa ja päivityksissä. Käynnistä sitten metagame
uudelleen **silloin, kun kukaan ei pelaa** (uudelleenkäynnistys pudottaa ryhmät ja jonot): käsin kuten
kohdassa [Uudelleenkäynnistys muutoksen jälkeen](#restarting-after-a-change) tai paketin palvelimella
`C:\DauntlessRevived\bin\Stack.ps1 restart -Only metagame`. Metagamen käynnistysrivi `features: ...`
näyttää käytössä olevan arvon. Jokainen asetus on sivulla [Asetukset]({{ config_page.url | relative_url }}).

**Oletuksena pois, ja mitä tehdä ennen kunkin kytkemistä päälle:**

| Ominaisuus | Rivi | Ennen kuin kytket sen päälle | Miten sen tarkistaa |
|:-----------|:-----|:-----------------------------|:--------------------|
| Oikeat Escalation-tallennukset | `ESCALATION_MODE=real` | Kerro pelaajille: jokainen putoaa tekaistusta maksimista (taso 25) tasolle 0. Käynnistä **koko kokonaisuus** uudelleen (paketin palvelimella `Stack.ps1 restart`), jottei mikään pelipalvelin pidä enää vanhoja arvoja; tee samoin, kun palautat asetuksen arvoon `stub`. | Pelaa Escalation-kierros, kirjaudu uudelleen ja käytä kykypiste: taso säilyy, eikä lokissa ole riviä `Refusing escalation save` tai `breaks a soft rule` ([Escalation]({{ '/fi/findings/escalation.html' | relative_url }}#switching-it-on)). |
| Tiukat Escalation-säännöt | `ESCALATION_STRICT=1` | Vasta kun oikea Escalation on toiminut ilman riviä `breaks a soft rule`. | Kuten yllä. |
| Ilmainen kauppa | `STORE=free` | Päätä, pysyykö kauppa ilmaisena (tiekartan kohta 3.7). Laske tilit, joilla on useampi hahmo (alla): osto menee viimeksi tallennetulle hahmolle. | Avaa jokainen kaupan välilehti, osta yksi tavara kutakin lajia, kirjaudu uudelleen ja pelaa metsästys loppuun ([Pelin kauppa]({{ '/fi/findings/store.html' | relative_url }}#open)). |
| Rajattomat premium-palkkiotehtävätunnisteet | `STORE_REPEATABLE_TOKENS=1` (yhdessä `STORE=free` kanssa) | Sinun päätöksesi: se tarkoittaa rajattomasti ilmaisia premium-palkkiotehtäviä. | Paketti näkyy kaupassa. |
| Kavereiden paikalla olo | `CHAT_PRESENCE=1` (yhdessä `CHAT=1` kanssa) | Chatin itsensä on toimittava ensin. Paketin `Set-Chat.ps1`:ssä ei ole sille kytkintä: lisää rivi käsin. | Läsnäolotesti sivulla [Tekstichat]({{ '/fi/findings/chat.html' | relative_url }}#how-to-verify-presence): ketään ei potkita ryhmästä. |
| Hunt Passin tasojen oikeudet vahvistuksessa | `PROGRESSION_CONFIRM_ENTITLEMENTS=1` | Vain jos pelitesti näyttää, etteivät Elite-tasojen kosmeettiset tavarat (tasot 6, 9, 29 ja 50) koskaan tule itsestään (lokissa ei ole riviä `POST /entitlementv2`). | Kosmeettinen tavara näkyy Claimin jälkeen. |
| Omat Hunt Pass -kausitiedostot | `PROGRESSION_CONFIG_DIR=<kansio>` ja toiselle kaudelle `ACTIVE_HUNT_PASS=<tunnus>` | Lue [Pelin asetukset]({{ '/fi/reference/game-settings.html' | relative_url }}#hunt-pass-seasons); älä koskaan muuta kautta, jossa pelaajilla on jo etenemistä. | Käynnistysrivi `Progression config: ...` nimeää sen, mitä ladattiin; huono tiedosto pysäyttää metagamen ja kertoo syyn. |

**Oletuksena päällä, ja rivi, joka ottaa kunkin pois:** `SLAYER_LINKS=0` (Slayer Links),
`VERIFY_STUB_ACCOUNT=1` (istunnon tarkistuksen vanha vastaus), `BALANCE_FROM_INVENTORY=0` (vanha
valuuttataulukko), `PROGRESSION_REPLAY_WINDOW_S=0` (ei uusintavahtia; aseta se, jos rivi `repeats the
grant of` näkyy tavallisessa pelaamisessa). Deploy-palvelimen asetuksissa `PERSISTENT_WORLD_LIVENESS=0`
jättää kaatuneen Ramsgaten uudelleenkäynnistyksen vahtikoiralle; käynnistä deploy-palvelin uudelleen
muutoksen jälkeen.

**Hahmojen laskeminen tiliä kohden** ennen asetusta `STORE=free` (tulostaa vain lukumäärän, ei nimiä
eikä tunnuksia). Paketin palvelimella, järjestelmänvalvojana avatussa PowerShellissä:

```powershell
Set-Location C:\DauntlessRevived\app\UndauntedMetagame
node -e "const db = new (require('better-sqlite3'))(process.argv[1], { readonly: true }); console.log(db.prepare('SELECT COUNT(*) AS accountsWithSeveralCharacters FROM (SELECT userId FROM characters GROUP BY userId HAVING COUNT(*) > 1)').get())" C:/DauntlessRevived/data/undaunted.db
```

Itse pystytetyllä koneella aja se omasta `legacy/UndauntedMetagame`-kansiostasi omalla `DB_FILENAME`-polullasi.
Tulos 0 tarkoittaa, että jokainen osto menee pelaajan ainoalle hahmolle.

## Käyttäjänimet {#usernames}

- Nimi valitaan rekisteröitäessä. Kaverikäynnistin ja kaveripaketti kysyvät sitä.
- Uudet nimet ovat 3–16 merkkiä, vain kirjaimia, numeroita ja alaviivoja
  (`^[A-Za-z0-9_]{3,16}$`), kun alun ja lopun välilyönnit on ensin poistettu. Muuten `Register`
  vastaa 400 `username_invalid`.
- Nimet ovat ainutlaatuisia kirjainkoosta riippumatta: `Register` vastaa 409 `username_taken`, jos
  toisella tilillä on jo sama nimi missä tahansa kirjainkoossa. Tarkistus tehdään samassa
  tietokantatransaktiossa, joka luo tilin (ainutlaatuista indeksiä ei ole), ja ennen kuin kutsukoodi
  kulutetaan, joten hylätty nimi ei polta koodia.
- Ennen näitä sääntöjä tehdyt tilit säilyttävät nimensä, myös sääntöjä rikkovat.
- `GET /undaunted/api/UsernameAvailable?Username=<name>` tarkistaa nimen rekisteröimättä. Julkinen
  yhdyskäytävä ei päästä sitä läpi, joten se toimii vain palvelinkoneella tai tailnetin kautta.
- Pelaajan ensimmäisellä kirjautumisella metagame luo hänen hahmonsa ja nimeää sen käyttäjänimen
  mukaan. Siksi ylläpitoreitti `POST /undaunted/api/RenameUser` nimeää tilin ja sen hahmot uudelleen
  yhdessä. Pelaaja näkee uuden nimen kirjauduttuaan uudelleen. Päivittyvätkö muiden pelaajien näkemät
  nimikyltit ilman uudelleenkirjautumista, on varmistamatta.
- Pelin sisällä nimeä ei voi vaihtaa: `/account/api/public/account` ilmoittaa
  `canUpdateDisplayName: false`.
- **Nimen vaihdon jälkeen pelaaja käynnistää pelin uudelleen ennen chatin käyttöä.** Peli lukee oman
  nimensä kerran, kirjautuessa, ja liittyy chat-huoneisiin sillä. Siihen asti, kun se kirjautuu
  uudelleen, chat-palvelin voi hylätä nuo liittymiset (`chat: join refused ... reason=nick-name`),
  koska nimi ei enää vastaa tiliä. Muut pelaajat näkevät uuden nimen heti, kun heidän pelinsä hakee
  sen.
- Oma omistajatilimme kantaa yhä paikanpitäjänimeä ”Slayer”.

Nimen vaihtaminen, kun `$M` ja `$h` on asetettu kuten kohdassa
[Kutsukoodien luominen](#making-invite-codes):

```powershell
$body = @{ Username = "OldName"; NewUsername = "New_Name" } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri "http://$M/undaunted/api/RenameUser" -Headers $h `
  -ContentType "application/json" -Body $body -TimeoutSec 10
```

`Username` on nykyinen nimi missä tahansa kirjainkoossa; sen sijaan käy myös `UserId`. Uusi nimi,
joka on jo toisella tilillä, saa vastauksen 409 `username_taken`.

## Kapasiteetti {#capacity}

> Vuokrattua hostingia varten katso selkeästi merkitty [EU Gamehost -kumppanisuositus]({{ "/fi/setup/hosting-partner.html" | relative_url }}).
> Pakettivalinnat perustuvat alla oleviin mittauksiin, eivät pelkkiin markkinointinimiin.

Mitattu palvelinkoneellamme, 8-ytimisellä pöytäkoneella, jossa on 32 Gt keskusmuistia (RAM), kun
1.4.4-kokonaisuus oli käynnissä:

| Prosessi | RAM | Suoritin | Huomiot |
|:--------|----:|----:|:------|
| Ramsgate-palvelin | 1,1 Gt | ~0,2 ydintä | Aina käynnissä. |
| Metsästyspalvelin | ~0,9 Gt | ei mitattu taistelun aikana | Mitattu opetusjakson metsästyksessä. Yksi jokaista enintään neljän hengen ryhmää kohden. |
| Training Dojo | ~0,9 Gt | ~0,2 ydintä | Forkissamme vain käytön aikana. |
| Isännän oma peliohjelma Cinematic-tasolla | ~1,9 Gt | ~2,5 ydintä | Vain jos pelaat myös palvelinkoneella. Working set; sen varattu (yksityinen) muisti oli noin 3,7 Gt. |
| Metagame ja deploy-palvelin (Node) | ~130 Mt yhteensä | vähäinen | |

Mitä se tarkoittaa yhteensä:

- **Portit ovat ehdoton raja, eivät RAM.** Metsästysportteja on 6 (8770–8775), joten enintään 6
  metsästystä voi olla käynnissä kerralla, kussakin enintään 4 pelaajaa. Kaikki jakavat yhden
  Ramsgaten. Pelin mukana tulleet 1.4.4-asetukset määrittävät
  `[/Script/Engine.GameSession] MaxPlayers=32`.
- Kaikki kerralla (Ramsgate, Dojo, 6 metsästystä ja oma peliohjelmasi) tekee näiden kevyen kuorman
  lukujen perusteella noin 9,5 Gt working setiä.
- **Arviomme on, että 8–12 kaveria paikalla yhtä aikaa onnistuu mukavasti.** Emme ole
  kuormitustestanneet sitä. Metsästyspalvelimen suoritinkuorma neljän pelaajan taistellessa ja
  Ramsgaten muistin kasvu pitkillä käyntiajoilla ovat yhä mittaamatta.
- **Lähetyskaista:** pelin mukana tulleet 1.4.4-asetukset rajaavat jokaisen peliohjelmayhteyden
  arvoon `MaxInternetClientRate=100000` tavua sekunnissa. Se on enintään noin 0,8 Mbit/s
  lähetyskaistaa etäpelaajaa kohden, joten kuusi etäpelaajaa tarvitsee enintään noin 5 Mbit/s.

Käytännön rajoituksia, jotka on hyvä tietää:

- Pelipalvelimet käynnistyvät 10 sekunnin välein (`SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP`).
  Matchmaker sulkee ryhmän, kun siinä on 4 pelaajaa tai kun 20 sekuntia kuluu ilman, että kukaan uusi
  liittyy. Kun kuusi ryhmää on valmiina yhtä aikaa, viimeisen ryhmän palvelin käynnistyy noin 50
  sekuntia ensimmäisen jälkeen.
- Metsästyspalvelin sulkeutuu, kun siihen ei ole ollut kukaan yhteydessä **yhteensä 50 sekuntiin**,
  eikä tuo laskuri koskaan nollaudu. Kaveri, jolla on hidas levy, voi saapua vasta, kun hänen
  palvelimensa on jo poissa. Suunniteltu DLL-korjaus tekee aikarajasta säädettävän ja nollaa sen, kun
  joku liittyy.
- **Kun kaikki metsästysportit ovat varattuina**, deploy-palvelin heittää virheen `No free ports left!`
  ja vastaa metagamelle HTTP 500 -virheellä. Metagamemme merkitsee silloin ryhmän haun epäonnistuneeksi
  (tilakysely vastaa `FAILED`); alkuperäinen metagame merkitsi ryhmän valmiiksi, osoitteena `""` ja
  porttina 0, ja pelaajat jäivät jumiin. Suunnitteilla on, että ryhmä odottaa, kunnes portti vapautuu.
- **Lisätäksesi metsästysportteja pienennä `PORT_RANGE_BEGIN`-arvoa. Älä koskaan kasvata
  `PORT_RANGE_END`-arvoa.** DLL pitää jokaista porttia 8776 tai yli pysyvänä palvelimena ja kytkee
  tyhjäkäyntisulkeutumisen niissä pois, joten sellaisessa portissa oleva metsästys ei koskaan
  sulkeutuisi. Laajenna palomuurisääntöä vastaavasti.
- Deploy-palvelin ei vielä rajoita peliprosessien muistinkäyttöä. Suunniteltu suoja: lopeta metsästys,
  joka ylittää noin 2,5 Gt, tai Ramsgate, joka ylittää noin 3 Gt (Ramsgate käynnistyy silloin
  uudelleen), ja kieltäydy uusista palvelimista, kun vapaata keskusmuistia on alle 3 Gt.
- **Ramsgatella ja Dojolla on kummallakin konsoli-ikkuna auki isännän työpöydällä. Ikkunan
  sulkeminen tappaa sen palvelimen** kaikilta, jotka ovat siinä. Metsästyspalvelimet käynnistetään
  ikkuna piilotettuna.

### Mittaa se {#measure-it}

Yllä olevat luvut ovat yhden pelaajan lukuja palvelinkoneeltamme. Palvelin, joka on asennettu
[Windows-palvelinpaketilla]({{ winserver_page.url | relative_url }}), mittaa itse itseään: sen
kokonaisuuden valvoja ottaa minuutin välein mittauksen ja lisää sen tiedostoon
`C:\DauntlessRevived\data\logs\performance\performance-<päivä>.csv`. Tiedosto vaihtuu joka päivä
(UTC-aikaa), ja tiedostot säilyvät 30 päivää. Jokaisessa mittauksessa on:

- yksi `host`-rivi: koneen suoritin, keskusmuisti kaikkiaan ja vapaana, asennusaseman vapaa
  levytila, verkkoliikenne sekä se, montako pelipalvelinta on käynnissä ja montako pelaajaa on
  paikalla
- yksi rivi jokaisesta kokonaisuuden prosessista (metagame, sisältöpalvelin, yhdyskäytävä,
  deploy-palvelin) ja pelipalvelimesta (Ramsgate, harjoitussali, jokainen metsästys): sen UDP-portti,
  käynnistysaika, pelaajat, suoritinkäyttö prosentteina yhdestä ytimestä ja muisti

Tiedostoihin kirjoitetaan vain lukuja: ei pelaajien nimiä, tilitunnuksia eikä avaimia.
`Stack.ps1 status` kertoo, milloin viimeisin mittaus otettiin. Mittauksen saa otettua heti
ajamalla `C:\DauntlessRevived\bin\Write-PerformanceLog.ps1 -Once` järjestelmänvalvojana avatussa
PowerShellissä. Kirjaamisen saa pois asettamalla `data\config\server.json`-tiedostoon
`"PerformanceLog": false` ja ajamalla `Stack.ps1 restart`. Sarakkeet on lueteltu sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}#performance-log). Pidä tiedostot omana
tietonasi: niistä näkee, milloin palvelimella on kiireistä.

Tiedostot ovat CSV-muotoisia, joten taulukkolaskentaohjelma avaa ne. Yhteenvetotyökalu, joka laskee
niistä yhden metsästyksen ja yhden Ramsgaten pelaajan hinnan ja arvion siitä, montako pelaajaa kone
jaksaa, on vielä tekemättä, samoin metagamen omat ajat (pyyntöjen vasteajat, tapahtumasilmukan viive
ja tietokantaan kuluva aika). Molemmat kuuluvat tiekartan kohtaan 4.12. Mittarin pohjana oli
Vvoidddd:n ensimmäinen versio ([#6](https://github.com/mixutin/dauntless-revived/pull/6)). Tällä
sivulla kuvatulla käsin pystytetyllä palvelinkoneella mittaria ei vielä ole.

## Koneen pitäminen käytettävissä {#keeping-the-pc-available}

Palvelin on päällä vain, kun palvelinkone on päällä, hereillä ja kirjautuneena.

- **Ei lepotilaa eikä horrostilaa** verkkovirralla. Näytön sammuminen on kunnossa.

  ```powershell
  powercfg /change standby-timeout-ac 0
  powercfg /change hibernate-timeout-ac 0
  ```

- **Windows Updaten uudelleenkäynnistykset.** Aseta aktiiviset tunnit kattamaan peliaikasi (Windows
  sallii enintään 18 tuntia) tai keskeytä päivitykset ennen pelikertaa. Palvelinkoneellamme aktiiviset
  tunnit olivat 08–17, joten iltojen pelikerrat olivat alttiina automaattisille
  uudelleenkäynnistyksille.
- **Pysy kirjautuneena.** Lukitse näyttö äläkä kirjaudu ulos. Pelipalvelimet pyörivät sinun
  istunnossasi ja lukevat tilisi `Game.ini`-tiedostoa. Niiden ajamista Windows-palveluna (istunto 0) ei
  ole testattu.
- **Käynnistä Tailscale ennen kokonaisuutta**, koska metagame sitoutuu Tailscale-osoitteeseen.
- **Yksi komento käynnistää ja pysäyttää kokonaisuuden, mutta koneen uudelleenkäynnistyksen jälkeen
  mikään ei vielä käynnisty itsestään.** Meidän palvelinkoneellamme `stack.ps1`-skripti (komennot `start`,
  `stop`, `restart` ja `status`) pyörittää jo koko kokonaisuutta. Se ottaa tietokannasta
  varmuuskopion ennen jokaista käynnistystä ja jokaisen pysäytyksen jälkeen. Skripti toimii
  palvelinkoneella, mutta se ei ole vielä repositoriossa (katso
  [tiekartta]({{ roadmap_page.url | relative_url }})). Välitavoitteeseen M4 on yhä suunniteltu kaksi
  asiaa: ajoitettu tehtävä käynnistää skriptin, kun omistaja kirjautuu koneelle, ja valvoja
  käynnistää kaatuneen metagamen tai deploy-palvelimen uudelleen yhä pidemmin odotuksin (backoff).
  Windows-palvelinpaketti tekee jo molemmat vuokrapalvelimella: sen `Stack.ps1` käynnistyy
  ajastetuista tehtävistä koneen käynnistyessä ja käynnistää kaatuneen osan uudelleen yhä pidemmin
  odotuksin (katso [Skriptit ja parametrit]({{ scripts_page.url | relative_url }})). Se toimii vain
  paketilla asennetulla palvelimella, ei tämän kaltaisella käsin pystytetyllä palvelinkoneella.

## Varmuuskopioi tietokanta {#back-up-the-database}

Kaikki, mitä pelaajat omistavat (tilit, hahmot, tavarat, varustesetit, eteneminen, kaverilistat,
kutsukoodit), on yhdessä SQLite-tiedostossa, joka on metagamen `.env`-tiedoston `DB_FILENAME`. Meillä
se on `C:\dr\data\undaunted.db`. Aluksi se on pieni (meillä 132 kt yhdellä pelaajalla), mutta siihen
tallentuu myös jokaisen hahmon tallennushistoria palautuksia varten (koodin arvion mukaan enintään
noin 3,5 Mt hahmoa kohden oletusasetuksilla) sekä tavara- ja etenemislokit, jotka vain kasvavat.
Jokainen taulu on kuvattu sivulla [Tiedostot ja data]({{ files_page.url | relative_url }}#the-database).

**Metagame ajaa kaikki odottavat tietokannan migraatiot jokaisella käynnistyksellä, eikä ota ensin
varmuuskopiota.** Varmuuskopioi aina ennen forkin päivittämistä tai alkuperäisen projektin muutosten
hakemista.

**Näin meidän palvelinkoneellamme tehdään.** Piilotettu ajoitettu tehtävä ottaa tietokannasta
varmuuskopion tunnin välein. Se säilyttää 48 uusinta kopiota sekä viimeisten 30 päivän ajalta kunkin
päivän uusimman kopion. `stack.ps1` ottaa lisäksi varmuuskopion ennen jokaista käynnistystä ja jokaisen
pysäytyksen jälkeen, eikä se käynnistä metagamea, jos varmuuskopio epäonnistuu. Palautustesti
onnistui. Nämä skriptit toimivat palvelinkoneellamme, mutta ne eivät ole vielä repositoriossa (katso
[tiekartta]({{ roadmap_page.url | relative_url }})). Kopioita koneen ulkopuolelle ei vielä ole.
Windows-palvelinpaketilla asennetulla palvelimella `Backup-DauntlessServer.ps1` tekee saman (tunnin
välein, ennen jokaista käynnistystä ja jokaisen pysäytyksen jälkeen; se säilyttää 48 uusinta
varmuuskopiota sekä kunkin viimeisen 30 päivän uusimman). Muilla koneilla käytä jompaakumpaa alla olevista vaihtoehdoista.

**Vaihtoehto 1: kopio pysäytettynä.** Pysäytä metagame ja kopioi tiedosto. Tietokanta käyttää SQLiten
oletusarvoista rollback journal -lokia, joten erillistä `-wal`-tiedostoa ei voi unohtaa.

**Vaihtoehto 2: varmuuskopio käynnissä ollessa.** SQLiten online backup -rajapinta on turvallinen,
vaikka metagame kirjoittaa samaan aikaan, ja `better-sqlite3` (joka on jo metagamen riippuvuus)
tarjoaa sen. Tallenna tämä nimellä `backup-db.js` kansioon `legacy/UndauntedMetagame`, jotta `require` löytää
moduulin:

```js
// node backup-db.js  -> C:/dr/backups/undaunted-YYYYMMDDHHMM.db (UTC time)
const Database = require("better-sqlite3");
const fs = require("fs");
const src = "C:/dr/data/undaunted.db";
const dir = "C:/dr/backups";
fs.mkdirSync(dir, { recursive: true });
const stamp = new Date().toISOString().slice(0, 16).replace(/[-:T]/g, "");
const out = `${dir}/undaunted-${stamp}.db`;
new Database(src, { readonly: true, fileMustExist: true }).backup(out).then(() => {
  const check = new Database(out, { readonly: true }).pragma("integrity_check", { simple: true });
  console.log(out, check);   // prints "ok" for a sound copy
});
```

- Aja se tunnin välein Windowsin Tehtävien ajoituksella (Task Scheduler), kun pyörität palvelinta, ja
  ennen jokaista päivitystä. Säilytä kiertävä joukko kopioita, esimerkiksi 48 tunnittaista ja 90
  päivittäistä, ja kopioi päivittäiset pois koneelta.
- **Palauttaminen:** pysäytä metagame, kopioi varmuuskopio `DB_FILENAME`-tiedoston päälle ja
  käynnistä metagame.
- Varmuuskopioissa on jokaisen pelaajan käyttäjänimi ja tavarat, joten pidä ne yksityisinä.

**Varmuuskopioi salaisuudet erikseen, salattuina, äläkä koskaan gitiin.** Molemmat `.env`-tiedostot
ovat jo gitin ohittamia. Niissä ovat metagamen JWT-allekirjoitusavainpari ja deploy-palvelimen
pelipalvelinavain. Varmuuskopioi myös omistajan tiliavain.

- Allekirjoitusavainten katoaminen vain mitätöi jo myönnetyt tunnisteet. Luo uusi pari, ja kaikki
  kirjautuvat taas normaalisti.
- Omistajan avaimen katoaminen lukitsee sinut ulos ylläpitorajapinnasta, kunnes korvaat sen tiivisteen
  tietokannassa (katso `rekey.js` yllä).

## Ennen kuin ensimmäinen kaveri liittyy {#before-the-first-friend-connects}

1. `.env`: `AUTH_MODE=APIKEY`, `NODE_ENV=production`, `REGISTRATION_MODE=INVITECODE`.
2. Tailscale on asennettu, käynnistyy valvomatta, ja kone on jaettu jokaiselle kaverille.
3. Kaksi palomuurisääntöä on olemassa, rajattuina Tailscale-liitäntään. TCP 61001 ei ole auki.
4. `MY_IP`, `QOS_TARGET_URL` ja metagamen `BIND_HOST` osoittavat Tailscale-osoitteeseen. Samoin
   `Game.ini`-tiedoston osoitelohko ja oman peliohjelmasi taustapalveluosoite.
5. Tietokannasta on varmuuskopio, ja tunnittainen tehtävä on päällä.
6. Lepotila on pois päältä, ja Windows Updaten aktiiviset tunnit kattavat peliaikasi.
7. AGPL-lisenssin vaatimukset on hoidettu. Kaverit käyttävät muokattua metagamea verkon yli, joten
   heillä on oikeus sen lähdekoodiin (kohta 13). Myös heille antamiesi DLL-tiedostojen lähdekoodin on
   oltava saatavilla (kohta 6). Anna heille linkki [tämän sivuston repositorioon]({{ site.github.repository_url }})
   ja kerro, mitä versiota (commit) ajat. Pelinsisäinen tilateksti (`/dauntless-status`) toivottaa
   pelaajat tervetulleiksi palvelimesi nimellä (”Welcome to Dauntless Revived!”, ellet aseta
   metagamen `.env`-tiedostoon muuta nimeä kohtaan `SERVER_NAME`). Samassa vastauksessa on niiden
   kenttien jälkeen, jotka peli lukee, myös palvelimen nimi, versio, commit ja lähdekoodilinkki
   (`SOURCE_URL` ja `GIT_COMMIT`, katso
   [asetusten viitesivu]({{ config_page.url | relative_url }}#metagame-identity)). Jos ajat muokattua
   koodia, osoita `SOURCE_URL` muokattuun lähdekoodiisi. Aiomme laittaa lähdekoodilinkin myös
   pelissä näkyvään tekstiin. Katso [Kiitokset ja lisenssi]({{ legal_page.url | relative_url }}).
   Tämä ei ole oikeudellista neuvontaa.
