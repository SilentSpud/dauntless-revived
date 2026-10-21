---
title: Skriptit ja parametrit
parent: Tekninen viite
grand_parent: Dauntless Revived suomeksi
nav_order: 6
description: "Kaikki Dauntless Revivedin ajettavat skriptit: Windows-palvelinpaketti, kaveripaketti, tools/ ja npm-skriptit, jokainen parametri oletusarvoineen ja esimerkkeineen."
lang: fi
ref: reference/scripts
locale: fi_FI
---

{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign ports_page = site.pages | where: "path", "fi/reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign game_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign dev_page = site.pages | where: "path", "fi/reference/development.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}
{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}

# Skriptit ja parametrit
{: .no_toc }

Tälle sivulle on koottu repositorion jokainen skripti, jota ihminen ajaa, ja jokaisen parametrin
tyyppi, oletusarvo ja esimerkki:

- Windows-palvelinpaketti kansiossa `deploy/windows-server/`,
- kaveripaketti kansiossa `friend-kit/`,
- ylläpitoskriptit kansiossa `tools/`,
- CI-työnkulut kansiossa `.github/workflows/` ja niiden työkalut kansiossa `tools/ci/`,
- jokaisen npm-paketin npm-skriptit.

Sivu on hakutaulukko. Vaiheittaiset ohjeet ovat sivuilla
[Windows-palvelin]({{ winserver_page.url | relative_url }}),
[Pystytä palvelin]({{ host_page.url | relative_url }}),
[Palvelin ryhmälle]({{ admin_page.url | relative_url }}) ja
[Liity kaverina]({{ friends_page.url | relative_url }}). Se, mitä skriptit kirjoittavat, kuvataan
tämän osion muilla sivuilla: `.env`-avaimet sivulla [Asetukset]({{ config_page.url | relative_url }}),
portit sivulla [Portit ja verkko]({{ ports_page.url | relative_url }}), skriptien kutsumat reitit
sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}) sekä tiedostot ja kansiot sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}).

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Ennen kuin ajat paketin skriptin {#before-you-run-a-kit-script}

Nämä säännöt koskevat jokaista kansion `deploy/windows-server/` skriptiä.

- **Windows PowerShell 5.1.** Paketti on kirjoitettu ohjelmalle `powershell.exe`, joka on jokaisessa
  tuetussa Windowsissa. Jos suorituskäytäntö (execution policy) estää skriptit, aja ne samalla tavalla
  kuin paketti ajaa omat skriptinsä:
  `powershell -NoProfile -ExecutionPolicy Bypass -File <skripti> <parametrit>`.
- **Missä skriptit ovat.** Asennetulla palvelimella aja ne kansiosta `<root>\bin` (oletuksena
  `C:\DauntlessRevived\bin`). Tällä sivulla `<root>` tarkoittaa asennuskansiota. Asennusohjelma
  kopioi paketin sinne, ja `Update-DauntlessServer.ps1` päivittää kopion uudesta koodista.
  `staging\kit` on vain lähetyskansio, jota `Deploy-Remote.ps1` käyttää.
- **`-Root`.** `Stack.ps1`, `Update-DauntlessServer.ps1`, `Backup-DauntlessServer.ps1`,
  `New-Invite.ps1`, `Get-ServerStatus.ps1`, `Set-Chat.ps1` ja `Write-PerformanceLog.ps1` ottavat parametrin
  `-Root <asennuskansio>`. Ilman sitä
  ne käyttävät omaa kansiotaan ylempää kansiota, jos siinä on `data\config\server.json`, kuten
  kansion `<root>\bin` kohdalla on. Muuten ne käyttävät kansiota `C:\DauntlessRevived`.
  Asennusohjelma ottaa sen sijaan parametrin `-InstallRoot`.
- **`-WhatIf` ja `-Confirm`** toimivat skripteissä `Deploy-Remote.ps1`, `Install-DauntlessServer.ps1`,
  `Update-DauntlessServer.ps1`, `Stack.ps1`, `Backup-DauntlessServer.ps1` ja `New-Invite.ps1`.
  `-WhatIf` luettelee muutokset tekemättä niitä. `Get-ServerStatus.ps1` ei muuta mitään, eikä siinä
  ole `-WhatIf`-valitsinta.
- **Paluukoodit.** `0` tarkoittaa onnistumista ja `1` epäonnistumista. Epäonnistuminen tulostaa
  punaisen `FAIL`-rivin, ja skriptin heittämä virhe alkaa tekstillä `DRFAIL:`. `Stack.ps1` on
  poikkeus: se palauttaa `1`:n vain, kun se epäonnistuu kokonaan. Jos jokin osa ei käynnisty, se
  kerrotaan tulosteessa.
- **Koneluettavat rivit.** Asennusohjelma tulostaa julkisessa tilassa rivin
  `DRFINGERPRINT=<64 heksamerkkiä>`. Lähetysapuri ja `Deploy-Remote.ps1`:n palvelimella tekemät
  tarkistukset tulostavat kukin yhden `DRJSON:{...}`-rivin.
- **Mitään salaista ei tulosteta.** Mikään paketin skripti ei tulosta avainta, tunnusta (token),
  salasanaa tai `.env`-arvoa.
- **Listat.** `-AdminIp` (asennusohjelma ja `Deploy-Remote.ps1`) ottaa joko taulukon tai yhden
  pilkuilla erotetun merkkijonon: `powershell -File` välittää `a,b`:n yhtenä merkkijonona, ja nämä
  kaksi skriptiä jakavat sen. `Stack.ps1 -Only` ei jaa tällaista merkkijonoa, joten
  `powershell -File` -kutsussa anna yksi osa kerrallaan.

Mitkä skriptit tarvitsevat ylläpitäjänä avatun PowerShellin ("Suorita järjestelmänvalvojana"):

| Skripti | Ylläpitäjänä? |
|:--------|:--------------|
| `Install-DauntlessServer.ps1` | Kyllä. `-Sandbox`:n kanssa se vain varoittaa. |
| `Update-DauntlessServer.ps1` | Kyllä, paitsi `-Sandbox`-asennuksessa. |
| `Stack.ps1` | Asennetulla palvelimella `start`, `stop` ja `restart` toimivat ajastettujen tehtävien kautta ja tarvitsevat sen, samoin `-Direct`. `status` toimii ilmankin, mutta se voi näyttää paikalla olevat pelaajat vain, jos se pystyy lukemaan omistajan avaimen, ja siihen tarvitaan ylläpitäjän oikeudet. |
| `New-Invite.ps1` | Kyllä: se lukee tiedoston `data\keys\owner.key`. |
| `Set-Chat.ps1` | Kyllä, paitsi `-Sandbox`-asennuksessa: se kirjoittaa tiedoston `metagame.env` uudelleen ja käynnistää kokonaisuuden uudelleen. |
| `Get-ServerStatus.ps1` | Palvelimella, jos haluat pelaajalistan: se lukee omistajan avaimen. Ei tarvita `-KeyFile`:n kanssa. |
| `Backup-DauntlessServer.ps1` | Aja se ylläpitäjänä, tai anna varmuuskopiotehtävän ajaa se palvelutilillä. Tili, joka ei voi lukea jotakin tiedostoa, ohittaa sen ja mainitsee sen tulosteessa. |
| `Write-PerformanceLog.ps1` | Asennetulla palvelimella kyllä: ilman ylläpitäjän oikeuksia se ei näe pelipalvelimia (ne pyörivät palvelutilillä) eikä pysty lukemaan omistajan avainta pelaajamääriä varten. `-Sandbox`-asennuksessa ei. |
| `Deploy-Remote.ps1` | Ei. Se pyörii omalla koneellasi tavallisella käyttäjätililläsi. Palvelimen SSH-tilin on oltava ylläpitäjä. |

## Windows-palvelinpaketti {#windows-server-kit}

| Skripti | Missä ajetaan | Mitä se tekee |
|:--------|:--------------|:--------------|
| `Deploy-Remote.ps1` | Oma koneesi | Lähettää paketin, koodin, varmuuskopion ja pelizipin pelkällä avaimella toimivan SSH:n yli ja ajaa sitten asennusohjelman tai päivityksen. Tekee myös kutsuja ja näyttää tilan. |
| `Install-DauntlessServer.ps1` | Palvelin | Asentaa, korjaa tai palauttaa palvelimen julkisessa tai yksityisessä tilassa. Sen voi ajaa uudelleen turvallisesti. |
| `Update-DauntlessServer.ps1` | Palvelin | Uusi palvelinkoodi käännettynä pyörivän palvelimen rinnalle, terveystarkistuksella ja automaattisella paluulla edelliseen versioon. |
| `Stack.ps1` | Palvelin | `status`, `start`, `stop`, `restart` ja `supervise` (ajastettujen tehtävien sisällä). |
| `New-Invite.ps1` | Palvelin | Luo kutsukoodin ja tulostaa kutsurivin. Myös luettelee ja peruu koodeja. |
| `Get-ServerStatus.ps1` | Palvelin tai mikä tahansa kone | Ketkä ovat paikalla ja mitkä maailmat ja metsästykset ovat käynnissä. |
| `Backup-DauntlessServer.ps1` | Palvelin | Varmuuskopio heti, vanhojen kopioiden karsinnalla. |
| `Set-Chat.ps1` | Palvelin | Kytkee pelin tekstichatin päälle tai pois (julkinen tila) ja käynnistää kokonaisuuden uudelleen. |
| `backup-hidden.vbs` | Palvelin | Ajaa varmuuskopion ilman ikkunaa (tuntitehtävän käytössä). |
| `Write-PerformanceLog.ps1` | Palvelin | Suorituskykymittaus heti, tai minuutin välein hiekkalaatikossa. Asennetulla palvelimella kokonaisuuden valvoja ottaa mittauksen jo minuutin välein. |
| `Receive-Upload.ps1` | Palvelin | Osissa tehtävän lähetyksen palvelinpää. `Deploy-Remote.ps1` kutsuu sitä; sinä et. |
| `DauntlessServer.Common.ps1`, `DauntlessServer.Performance.ps1` | Molemmat | Yhteiset apufunktiot, jotka skriptit lataavat: kiinnitykset, kutsut, kiinnitetty HTTPS, prosessit; suorituskykymittari. |
| `lib\dr-db.js`, `lib\dr-keys.js`, `lib\verify-game.js` | Palvelin | Node-apuohjelmat, joita skriptit kutsuvat. |
| `tests\Test-*.ps1` | Kehityskone | Paketin omat testit. |

### Deploy-Remote.ps1 {#deploy-remoteps1}

Ajetaan omalla koneellasi repositorion kopiosta: `.\deploy\windows-server\Deploy-Remote.ps1`. Se
tarvitsee Windowsin OpenSSH-asiakkaan (`ssh.exe` ja `scp.exe` kansiossa `System32\OpenSSH`) ja
koodin lähettämiseen `git`:in. Ohje on kohdassa
[Asennus omalta koneelta]({{ winserver_page.url | relative_url }}#deploy-from-your-pc).

Se ottaa yhteyden pelkällä avaintiedostolla (`BatchMode`, ei salasana- tai näppäimistökyselyjä) ja
kiinnittää palvelimen isäntäavaimen `known_hosts`-tiedostoon. Komennot ajetaan palvelimella muodossa
`powershell -EncodedCommand`. Ilman toimintovalitsinta se asentaa:

1. Se ottaa yhteyden ja tarkistaa, että SSH-istunnolla on ylläpitäjän oikeudet. Sitten se lähettää
   paketin kansioon `<InstallRoot>\staging\kit`. Kansiota `tests` ei lähetetä.
2. Se lähettää koodin tiedostona `staging\source\source-<12 heksamerkkiä>.zip`. Se on `git archive`
   `HEAD`:sta tai `-WorkingTree`:n kanssa työkopiosta. `-Source GitHub`:n kanssa mitään ei lähetetä,
   vaan palvelin lataa refin itse.
3. `-RestoreFrom`:n kanssa se lähettää varmuuskopiokansion kansioon `staging\restore\<kansion nimi>`.
   Vain Administrators ja SYSTEM voivat avata sen, ja se poistetaan onnistuneen asennuksen jälkeen.
4. `-GameZip`:n kanssa se lähettää pelizipin osissa kansioon `staging\upload`. Valmis
   `BaseGame144.zip` jää sinne korjauksia varten.
5. Se ajaa `Install-DauntlessServer.ps1`:n ja näyttää sen tulosteen sitä mukaa.
6. Se tulostaa tuloksen. Julkisessa tilassa se tarkistaa lisäksi yhdyskäytävän omalta koneeltasi
   kiinnitetyllä varmenteella ja muistuttaa, että palveluntarjoajan palomuurissa on sallittava
   saapuva TCP yhdyskäytävän porttiin ja UDP 8770-8777 mistä tahansa osoitteesta. UDP:tä se ei voi
   testata koneeltasi.

**Yhteys**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Server` | IPv4-osoite tai DNS-nimi, **pakollinen** | | SSH-kohde. Julkisessa asennuksessa myös `-PublicHost`:n oletus. |
| `-User` | tilin nimi: kirjaimia, numeroita, `.`, `_`, `-` (1-64) | `Administrator` | SSH-tili. Istunnolla on oltava ylläpitäjän oikeudet, joten käytä ylläpitäjää. |
| `-SshPort` | 1-65535 | `22` | SSH-portti. |
| `-KeyFile` | polku | `C:\dr\data\ssh\dauntless_deploy` | SSH-yksityisavaimesi. Oletus on projektin oman palvelinkoneen polku; anna omasi. **Salainen: älä koskaan jaa, älä koskaan committoi.** Skripti varoittaa, jos Everyone, Users tai Authenticated Users voi lukea tiedoston, koska `ssh` kieltäytyy silloin käyttämästä sitä. |
| `-KnownHostsFile` | polku ilman välilyöntejä | `known_hosts` `-KeyFile`:n kansiossa | Mihin palvelimen isäntäavain kiinnitetään. Ensimmäinen yhteys tallentaa sen (`StrictHostKeyChecking=accept-new`), ja jokaisen myöhemmän yhteyden on täsmättävä siihen. |
| `-HostKeyFingerprint` | `SHA256:` ja 43 base64-merkkiä (etuliitteen voi jättää pois) | ei mitään | Ensimmäistä yhteyttä varten. Skripti hakee palvelimen isäntäavaimet `ssh-keyscan`:illa, vaatii niistä yhden, jolla on tämä sormenjälki, ja kiinnittää sen. Ilman tätä ensimmäinen yhteys, jossa on myös `-RestoreFrom`, torjutaan, koska varmuuskopiossa on kaikki avaimet. Ota sormenjälki palveluntarjoajan konsolista. |

**Mitä asennetaan** (välitetään `Install-DauntlessServer.ps1`:lle)

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Mode` | `Public` tai `Private` | `Public` | Palvelimen tila. Välitetään aina asennusajossa: katso alta "Olemassa olevan palvelimen uudelleenasennus". |
| `-InstallRoot` | absoluuttinen polku ilman välilyöntejä | `C:\DauntlessRevived` | Asennuskansio palvelimella. |
| `-OwnerName` | 3-16 kirjainta, numeroa tai `_` | ei mitään | Uuden palvelimen ylläpitäjätili. Asennus tarvitsee `-OwnerName`:n tai `-RestoreFrom`:n, ellei anneta `-UploadOnly`:a. |
| `-RestoreFrom` | paikallinen varmuuskopiokansio, jolla on pelkkä nimi, kuten `2026-10-01_200000` | ei mitään | Siirtää olemassa olevan palvelimen: tietokannan ja jokaisen avaimen. Kansiossa on oltava `undaunted.db`, `secrets\metagame.env` ja `secrets\deployserver.env`. Lähetyksen jälkeen tiedostojen määrää ja tietokannan tiivistettä verrataan. **Salainen: kansiossa on kaikki avaimet; älä koskaan jaa, älä koskaan committoi.** Se kulkee vain SSH-yhteyden sisällä. |
| `-ServerName` | 1-64 merkkiä: ei rivinvaihtoja, ohjausmerkkejä, lainausmerkkejä (`"`), kenoviivoja tai tekstin suuntaa muuttavia merkkejä, eikä välilyöntejä kummassakaan päässä (asennusohjelman sääntö) | ei välitetä | Nimi, jonka kaverit näkevät. Kun se jätetään pois, asennusohjelma käyttää omaa oletustaan, `Dauntless Revived`. |
| `-PublicHost` | IPv4-osoite tai DNS-nimi | `-Server` | Julkinen tila: osoite, johon kaverit ottavat yhteyden. Välitetään aina julkisessa tilassa. |
| `-GatewayPort` | 1-65535 | `443` | Julkinen tila: yhdyskäytävän TCP-portti. Välitetään aina julkisessa tilassa. |
| `-AdminIp` | IPv4-osoite tai `IPv4/etuliite`, jossa etuliite on 8-32; yksi tai useampi | ei mitään | Osoitteet, joista etätyöpöytä sallitaan. Ilman tätä ja ilman `-KeepRdpOpen`:ia skripti varoittaa, että asennusohjelma laittaa internetille avoimet etätyöpöytäsäännöt pois päältä. |
| `-KeepRdpOpen` | valitsin | pois | Jätä internetille avoimet etätyöpöytäsäännöt ennalleen. |
| `-InteractiveSession` | valitsin | pois | Asennusohjelman istunto 0:n varasuunnitelma. |
| `-Chat` | `On` tai `Off` | ei välitetä | Välitetään asennusohjelman parametriksi `-Chat`. `On` vain julkisessa tilassa. Yksinään, ilman mitään asennettavaa, se on sen sijaan toiminto: katso alta. |
| `-GameZip` | paikallisen zipin polku | ei mitään | Lähettää pelizipin osissa, joiden lähetystä voi jatkaa. Zipin tiiviste lasketaan ensin omalla koneellasi, ja sen on vastattava kiinnitettyä SHA-256:ta. Jokainen osa tarkistetaan palvelimella enintään 3 yrityksellä, ja koko tiedosto uudelleen kokoamisen jälkeen enintään 4 kierroksella. Aja sama komento uudelleen jatkaaksesi katkennutta lähetystä. |
| `-GameZipUrl` | `https://`-osoite | ei mitään | Antaa palvelimen ladata zipin itse. Käytä joko `-GameZip`:iä tai `-GameZipUrl`:ia. |
| `-ChunkSizeMB` | 1-2048 | `256` | Pelizipin lähetyksen osan koko. |

**Mikä koodi**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Source` | `Upload` tai `GitHub` | `Upload` | `Upload` lähettää repositoriokopiosi koodin. `GitHub` saa palvelimen lataamaan `-Ref`:n repositoriosta. |
| `-Ref` | haara, tagi tai commit | `friends-v1` (`$DRRepo.PinnedRef`) | Ref `-Source GitHub`:ia varten. |
| `-WorkingTree` | valitsin | pois | Lähettää nykyiset tiedostosi `HEAD`:n sijaan: seuratut ja seuraamattomat tiedostot, joita git ei ohita, paitsi kansiot `node_modules`, `dist`, `build`, `out` ja `.vite`, kaikki `.env`- ja `.env.*`-tiedostot paitsi `.env.example`, sekä `*.key`, `*.pem`, `*.db`, `*.db-journal`, `*.db-wal`, `*.db-shm` ja `*.log`. Commit kirjataan muodossa `<commit>-worktree`. Ei yhdistettävissä `-Source GitHub`:iin. |

Julkisessa tilassa `HEAD`:n lähetys torjutaan, jos `HEAD`:sta puuttuu `legacy/UndauntedGateway` tai
`legacy/UndauntedContent`. Tarkistus tehdään paketin lähetyksen jälkeen mutta ennen kuin koodi,
varmuuskopio ja pelizip lähetetään. Committoi työsi tai lisää `-WorkingTree`. Committoimattomia
muutoksia ei asenneta ilman `-WorkingTree`:tä, ja skripti varoittaa niistä.

**Toiminnot** (käytä enintään yhtä näistä: `-Update`, `-Status` ja `-InviteFor`)

| Parametri | Tyyppi | Mitä se tekee |
|:----------|:-------|:--------------|
| (ei mitään) | | Asennus: lähettää kaiken ja ajaa `Install-DauntlessServer.ps1`:n. |
| `-Update` | valitsin | Lähettää paketin ja koodin ja ajaa sitten `Update-DauntlessServer.ps1 -Root <InstallRoot>` lähetetyllä zipillä, tai `-Source GitHub`:n kanssa `-Ref`:llä. `-GameZip`, `-GameZipUrl`, `-RestoreFrom` ja `-OwnerName` torjutaan. |
| `-Status` | valitsin | Ajaa palvelimella `<InstallRoot>\bin\Stack.ps1 status` ja `Get-ServerStatus.ps1`. Paluukoodi on niiden. |
| `-InviteFor` | teksti ilman ohjausmerkkejä, lainausmerkkejä, takahipsuja tai `$`-merkkiä | Ajaa palvelimella `New-Invite.ps1 -For <teksti>`, tulostaa kutsurivin ja tarkistaa sen sitten omalta koneeltasi komennolla `Get-ServerStatus.ps1 -Invite`. |
| `-Chat` yksinään | `On` tai `Off` | Ilman asennusparametreja (`-OwnerName`, `-RestoreFrom`, `-GameZip`, `-GameZipUrl`, `-ServerName`, `-AdminIp`, `-Ref`, `-WorkingTree`, `-UploadOnly`) ajaa palvelimella `<InstallRoot>\bin\Set-Chat.ps1 -On` tai `-Off` eikä lähetä mitään. Kokonaisuus käynnistyy uudelleen, joten tee se, kun kukaan ei pelaa. Torjutaan yhdessä parametrien `-Update`, `-Status` tai `-InviteFor` kanssa. |
| `-UploadOnly` | valitsin | Lähettää kaiken, tulostaa komennon, joka palvelimella ajettaisiin (asennusohjelma, tai `-Update`:n kanssa päivitys), ja pysähtyy. |

**Vain testeille**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-TestTargetDir` | kansio | ei mitään | Paikallinen kansio esittää palvelinta: ei SSH:ta, eikä mitään asenneta. Muut testivalinnat torjutaan ilman tätä. |
| `-TestZipSha256` | 64 heksamerkkiä | kiinnitetty zipin tiiviste | Tiiviste, joka zipillä on oltava, jotta testi voi käyttää pientä tiedostoa. |
| `-TestStopAfterChunks` | luku | `0` (pois) | Simuloi katkennutta yhteyttä näin monen osan jälkeen. |
| `-TestCorruptChunk` | osan numero nollasta laskien | `-1` (pois) | Vahingoittaa tämän osan kerran matkalla. |

**Olemassa olevan palvelimen uudelleenasennus.** Asennusajo välittää nämä joka kerta, joten anna ne
uudelleen:

- `-Mode` välitetään aina, oletuksena `Public`. Jos asennat yksityisen tilan palvelimen uudelleen
  ilman `-Mode Private`:a, siitä tulee julkisen tilan palvelin.
- Julkisessa tilassa `-PublicHost` (oletus: `-Server`) ja `-GatewayPort` (oletus `443`) välitetään
  aina. Ne korvaavat sen, mitä palvelimelle on tallennettu.
- `-ServerName` välitetään vain, kun annat sen, ja asennusohjelman oletus on `Dauntless Revived`.
  Uudelleenasennus ilman sitä nimeää palvelimen uudelleen.
- `-InteractiveSession`:ia ei muisteta. Uudelleenasennus ilman sitä laittaa automaattisen
  kirjautumisen taas pois.
- `-Chat` muistetaan tiedostossa `server.json`: uudelleenasennus ilman sitä pitää chatin ennallaan.

Koodimuutoksiin käytä `-Update`:a. Se ei aja asennusohjelmaa, joten mikään tästä ei koske sitä.

```powershell
# Uusi julkinen palvelin: omistajan tili, pelizip tältä koneelta, etätyöpöytä vain omasta osoitteestasi
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -HostKeyFingerprint SHA256:... `
    -OwnerName Slayer -GameZip D:\BaseGame144.zip -AdminIp 198.51.100.20

# Olemassa olevan palvelimen siirto; uusi palvelin lataa pelizipin itse
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -HostKeyFingerprint SHA256:... `
    -RestoreFrom D:\backups\2026-10-01_200000 -GameZipUrl https://example.org/BaseGame144.zip `
    -ServerName "Lauantain Ramsgate"

# Pelkkä suunnitelma: ei yhteyttä mihinkään
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -OwnerName Slayer -GameZip D:\BaseGame144.zip -WhatIf

# Arjessa
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -Update
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -InviteFor friend1
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -Status
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -Chat On
```

### Install-DauntlessServer.ps1 {#install-dauntlessserverps1}

Ajetaan palvelimella ylläpitäjänä avatussa PowerShellissä. `Deploy-Remote.ps1` ajaa sen kansiosta
`<root>\staging\kit`, ja voit ajaa sen myös itse. Jokaisen vaiheen voi toistaa turvallisesti: uusi
ajo korjaa tai viimeistelee asennuksen ja jättää rauhaan sen, mikä on jo kunnossa. Vaiheet kuvataan
kohdassa [Mitä asennus tekee]({{ winserver_page.url | relative_url }}#what-the-installer-does).

Asennusohjelma kieltäytyy toimimasta, kun jokin kokonaisuuden osa on käynnissä. Pysäytä kokonaisuus
ensin komennolla `Stack.ps1 stop`, tai käytä koodipäivityksiin `Update-DauntlessServer.ps1`:ä.
Esitarkistus vaatii ylläpitäjän oikeudet, 64-bittisen Windowsin (koontiversio 17763 tai uudempi)
työpöytäkokemuksella, vähintään 7,5 Gt muistia Windowsin ilmoittamana, 40 Gt:n levytilavarauksen
(hiekkalaatikossa 2 Gt; samalla asemalla oleva pelizip ja jo asennettu peli lasketaan mukaan),
`tar.exe`:n ja vapaat portit. Ongelma pysäyttää oikean ajon. `-WhatIf`:n kanssa jokainen ongelma on
vain varoitus. `-Sandbox`:n kanssa koneen tarkistukset ovat vain varoituksia, mutta varattu portti
pysäyttää silti ajon.

**Tila ja osoite**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Mode` | `Public` tai `Private` | Uusi asennus: `Public`. Uusi ajo: asennettu tila. Vanha `server.json`, jossa ei ole tilaa, lasketaan tilaksi `Private`. | `Public`: TLS-yhdyskäytävä, v2-kutsut. `Private`: Tailscale, v1-kutsut. |
| `-PublicHost` | IPv4-osoite tai DNS-nimi | Tallennettu `PublicHost`. Muuten verkkokorttien ainoa julkinen IPv4-osoite; asennusohjelma pysähtyy, jos sellaista ei ole tai niitä on useampi. Hiekkalaatikossa `127.0.0.1`. | Julkinen tila: osoite, johon kaverit ottavat yhteyden. Se menee kutsuihin ja varmenteeseen. Pelipalvelimet ilmoittavat pelaajille sen IPv4-osoitteen. Sen on osoitettava julkiseen IPv4-osoitteeseen, paitsi hiekkalaatikossa. Anna se, jos palvelin on 1:1-NATin takana. |
| `-GatewayPort` | 1-65535 (hiekkalaatikossa 62000-62499) | Tallennettu yhdyskäytävän portti, muuten `443` (hiekkalaatikossa `62443`) | Julkinen tila: yhdyskäytävän TCP-portti, ainoa julkinen TCP-portti. |
| `-ServerName` | 1-64 merkkiä: ei rivinvaihtoja, ohjausmerkkejä, lainausmerkkejä (`"`), kenoviivoja tai tekstin suuntaa muuttavia merkkejä, eikä välilyöntejä kummassakaan päässä | `Dauntless Revived` | Nimi, jonka kaverit näkevät kutsuissa ja käynnistimessä. **Uusi ajo ei muista sitä:** ajo ilman sitä palauttaa nimeksi `Dauntless Revived`. |
| `-InstallRoot` | polku | `C:\DauntlessRevived` | Minne kaikki asennetaan. Pakollinen `-Sandbox`:n kanssa. |

**Pelitiedostot** (käytä yhtä kolmesta)

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-GameZip` | polku | ei mitään | Tarkistettu 1.4.4-zip. Se tarkistetaan kiinnitettyä SHA-256:ta vasten ja puretaan Windowsin omalla `tar.exe`:llä kansioon `<root>\game`. Ei tarvita, kun peli on jo asennettu. |
| `-GameZipUrl` | `https://`-osoite | ei mitään | Palvelin lataa zipin tiedostoksi `<root>\downloads\BaseGame144.zip`. Lataus jatkuu katkosten jälkeen, enintään 20 uudelleenyritystä yhdellä ajolla, ja uusi ajo jatkaa sitä. Tiedosto, jonka SHA-256 on väärä, poistetaan. |
| `-GameDir` | kansio, jossa on `Archon\` | `<root>\game\Dauntless`; jos peliä ei ole siellä, tallennettu pelikansio, kun se on kelvollinen | Käyttää jo purettua peliä siinä, missä se on. Palvelutili saa siihen lukuoikeuden. Pakollinen `-Sandbox`:n kanssa, jolloin se on pelkkä korvikekansio. |

Jokainen asennus tarkistaa pelin exe-tiedoston kiinnitettyä tiivistettä vasten ja jokaisen
pelitiedoston sisältöluetteloa (manifest) vasten. Sitten se asentaa kaksi palvelimen DLL-tiedostoa
kiinnitetyillä tiivisteillä.

**Palvelinkoodi** (käytä yhtä näistä: `-SourceZip`, `-SourceDir` ja `-Ref`)

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-SourceZip` | lähdekoodizipin polku (`git archive`) | ei mitään | Kääntää lähetetystä zipistä. Tämän `Deploy-Remote.ps1` lähettää. |
| `-SourceCommit` | 7-40 heksamerkkiä, valinnaisesti päätteellä `-dirty` tai `-worktree` | `zip-` ja zipin SHA-256:n 12 ensimmäistä heksamerkkiä | Nimeää `-SourceZip`:n commitin versioraportteja varten. |
| `-SourceDir` | kansio, jossa on `legacy\UndauntedMetagame\package.json` | ei mitään | Kääntää paikallisesta kansiosta. Git-kopiossa commit luetaan gitistä, päätteellä `-dirty`, kun muutoksia on. |
| `-Ref` | haara, tagi tai commit: kirjaimia, numeroita, `.`, `_`, `/`, `-` (1-100) | Katso alta | Lataa tämän refin GitHubista ja kääntää sen. |

Ilman mitään näistä kolmesta asennusohjelma kääntää sen repositoriokopion, josta se ajetaan, jos se
ajetaan sellaisesta (kun `..\..\legacy\UndauntedMetagame\package.json` on olemassa). Muuten se kääntää
GitHubista `$DRRepo.PinnedRef`:n eli `friends-v1`:n. **Varo:** uusi ajo kansiosta `<root>\bin` ei
tapahdu repositoriokopiossa, joten ilman lähdeparametria se lataa `friends-v1`:n. Säilyttääksesi
asennetun koodin asenna uudelleen `Deploy-Remote.ps1`:llä, joka lähettää koodin uudelleen, tai anna
`-Ref <asennettu commit>`, jos se commit on GitHubissa. Jo käännettyä committia ei käännetä
uudelleen, ellei sen nimi pääty `-dirty`- tai `-worktree`-päätteeseen tai ole `unknown`.
Kääntäminen kopioi lähdekoodin kansioon `<root>\app.new` (ilman `node_modules`-kansioita,
käännöstuloksia, `.git`-kansiota, `.env`-tiedostoja, avaimia, varmenteita, tietokantoja ja lokeja),
ajaa `npm ci`:n ja `npm run build`:in neljässä palvelinpaketissa (`legacy/UndauntedMetagame`,
`legacy/UndauntedDeployServer`, `legacy/UndauntedContent` ja `legacy/UndauntedGateway`; käynnistintä ei käännetä) ja
vaihtaa sen sitten käyttöön. Vanha käännös jää talteen nimellä `app.prev`.

**Omistaja, palautus ja varmenne**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-OwnerName` | 3-16 kirjainta, numeroa tai `_` | Kysytään konsolissa, jos puuttuu | Uuden tietokannan ylläpitäjätili. Sen avain menee tiedostoon `data\keys\owner.key`, eikä sitä koskaan tulosteta. **Omistajan avain on salainen: älä koskaan jaa, älä koskaan committoi;** pidä kopio salasanojen hallintaohjelmassa. Ei käytetä, kun `-RestoreFrom` tuo tilit tai kun toimiva `owner.key` on jo olemassa. Jos `owner.key` on olemassa mutta ei toimi, asennus pysähtyy: siirrä tiedosto pois, jos haluat luoda uuden omistajan. |
| `-RestoreFrom` | varmuuskopiokansio | ei mitään | Siirtää palvelimen tänne. Katso [Palautus, paluu edelliseen versioon ja poistaminen](#restore-rollback-and-uninstall). **Salainen: sisältää kaikki avaimet.** |
| `-NewCertificate` | valitsin | pois | Tekee uuden yhdyskäytävän varmenteen, vaikka sellainen on jo olemassa tai varmuuskopiossa on sellainen. **Jokainen aiemmin annettu kutsu lakkaa toimimasta.** |

**Etätyöpöytä ja istunto 0**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-AdminIp` | IPv4-osoite tai `IPv4/etuliite`, jossa etuliite on 8-32; yksi tai useampi | Tallennettu lista | Julkinen tila: jokainen käytössä oleva saapuvan etätyöpöytäliikenteen sääntö (TCP ja UDP 3389) rajataan näihin osoitteisiin. |
| `-KeepRdpOpen` | valitsin | pois | Julkinen tila ilman `-AdminIp`:tä: jätä internetille avoimet etätyöpöytäsäännöt päälle. Ilman tätä ne laitetaan pois päältä ja vanha tila kirjataan tiedostoon `server.json`. |
| `-InteractiveSession` | valitsin | pois | `dauntless`-tili kirjautuu automaattisesti koneen käynnistyessä, kokonaisuus käynnistyy tuossa kirjautumisessa ja istunto lukitaan heti. Salasana säilytetään LSA-salaisuutena, ei koskaan selväkielisenä. **Uusi ajo ei muista sitä:** ajo ilman sitä laittaa automaattisen kirjautumisen pois. Katso [Istunto 0]({{ winserver_page.url | relative_url }}#session-0-and--interactivesession). |

**Portit**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-MetagamePort`, `-DeployPort`, `-ContentPort`, `-AllowlistPort` | 1024-65535 (hiekkalaatikossa 62000-62499) | Tallennettu portti, muuten 61000, 61001, 61002 ja 61005 (hiekkalaatikossa 62000, 62001, 62002 ja 62005) | Osien TCP-portit. Käytössä olevien porttien on oltava keskenään eri portteja ja vapaina. [Portit ja verkko]({{ ports_page.url | relative_url }}) selittää kunkin. |

**Chat**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Chat` | `On` tai `Off` | Tiedostoon `server.json` tallennettu `"Chat"`, muuten `On` (julkinen tila) | Pelin tekstichat. Julkisessa tilassa kirjoittaa tiedostoon `metagame.env` asetukset `CHAT=1` tai `0`, `CHAT_BIND_HOST=127.0.0.1` ja `CHAT_PORT` (61099, hiekkalaatikossa 62099, sama portti kuin yhdyskäytävän `GATEWAY_WS_URL`) ja tallentaa valinnan. Palomuurisääntöä ei lisätä. `-Chat On` yksityisessä tilassa pysäyttää asennuksen: yksityisessä tilassa ei vielä ole chattia. |

**Yksityinen tila (Tailscale)**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-TailscaleAuthKey` | Tailscalen tunnistusavain (auth key), yleensä alkaa `tskey-` | ei mitään | Kirjaa palvelimen Tailscaleen ilman selainta. Torjutaan julkisessa tilassa. **Salainen: älä koskaan jaa, älä koskaan committoi.** Parametri näkyy prosessilistassa ja PowerShellin historiassa, joten jätä se mieluummin pois: asennusohjelma tulostaa silloin ajettavan `tailscale up` -komennon ja odottaa enintään 20 minuuttia. Avaimen kanssa se kirjoittaa avaimen väliaikaiseen tiedostoon, jota vain Administrators ja SYSTEM voivat lukea, välittää sen muodossa `--auth-key=file:<polku>` ja poistaa tiedoston. |
| `-TailscaleHostname` | teksti | `dauntless-server` | Koneen nimi `tailscale up` -komennolle. |
| `-TailscaleShareUrl` | `https://login.tailscale.com/...`, enintään 512 merkkiä | Tallennettu linkki | Koneen jakolinkki, jonka v1-kutsut kuljettavat muodossa `&share=`. Torjutaan julkisessa tilassa. |
| `-AdvertiseHost` | IPv4-osoite tai DNS-nimi | Tallennettu arvo, muuten Tailscale-IPv4-osoite | Isäntä, jonka v1-kutsut kuljettavat, esimerkiksi MagicDNS-nimi. |

**Kehitys ja testit**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Sandbox` | valitsin | pois | Testiasennus kehityskoneelle. Se ohittaa Node.js:n, ajonaikaiset kirjastot, Tailscalen, palomuurin, tilit ja tehtävät. Kaikki kuuntelee osoitteessa `127.0.0.1` 620xx-porteissa, myös yhdyskäytävä. Sallittujen listan apuri pyörii kuivaharjoitustilassa, deploy-palvelinta tai pelipalvelimia ei ajeta, ja kaikki pyörii nykyisellä käyttäjällä. Tarvitsee `-InstallRoot`:n ja `-GameDir`:n. |
| `-ContentManifest` | polku | sisäänrakennettu luettelo | Sisältöluettelo korvikepelikansiolle. Torjutaan ilman `-Sandbox`:ia. |
| `-AllowlistDryRun` | valitsin | pois | Sallittujen listan apuri vain kirjaa lokiin palomuurimuutoksen, jonka se tekisi. `-Sandbox` pakottaa sen päälle. **Uusi ajo ei muista sitä.** |
| `-NoStart` | valitsin | pois | Ei käynnistä kokonaisuutta lopuksi. Ensimmäinen varmuuskopio otetaan silti. |

**Mitä uusi ajo säilyttää.** Kun parametria ei anneta, uusi ajo käyttää `server.json`-tiedostoon
tallennettua arvoa parametreille `-Mode`, `-PublicHost`, `-GatewayPort`, neljälle muulle portille,
`-AdminIp`, `-AdvertiseHost`, `-TailscaleShareUrl`, `-GameDir` ja `-Chat`. Se säilyttää varmenteen, avaimet,
salaisuudet ja jokaisen `.env`-avaimen, jota se ei itse hallitse (katso
[Asetukset]({{ config_page.url | relative_url }})). Se **ei** säilytä parametreja `-ServerName`,
`-InteractiveSession` ja `-AllowlistDryRun`: anna ne joka kerta uudelleen.

Lopuksi asennusohjelma ottaa ensimmäisen varmuuskopion ja käynnistää kokonaisuuden, ellei annettu
`-NoStart`. `-InteractiveSession`:in kanssa kokonaisuus käynnistyy vasta seuraavassa automaattisessa
kirjautumisessa (käynnistä palvelin uudelleen). Sitten se
tulostaa osoitteen, julkisessa tilassa rivin `DRFINGERPRINT=<64 heksamerkkiä>` ja omistajan avaimen
polun (ei koskaan avainta). Julkisessa tilassa se muistuttaa lisäksi avaamaan palveluntarjoajan
palomuurista TCP:n yhdyskäytävän porttiin ja UDP 8770-8777:n, ja se kehottaa käynnistämään
palvelimen uudelleen, jos se poisti käytäntöarvon, joka piti Windowsin palomuurin pois päältä.

```powershell
# Puretusta paketista palvelimella: uusi julkinen palvelin
.\Install-DauntlessServer.ps1 -GameZip D:\BaseGame144.zip -PublicHost 203.0.113.7 -AdminIp 198.51.100.20 -OwnerName Slayer

# Kaikki, mitä se muuttaisi, muuttamatta mitään
.\Install-DauntlessServer.ps1 -Mode Public -GameZipUrl https://example.org/BaseGame144.zip -OwnerName Slayer -WhatIf

# Yksityisen tilan palvelin, joka laittaa MagicDNS-nimensä kutsuihin
.\Install-DauntlessServer.ps1 -Mode Private -GameZip D:\BaseGame144.zip -OwnerName Slayer -AdvertiseHost dauntless-server.example.ts.net

# Uusi ajo asennetulla palvelimella: etätyöpöytä rajataan uuteen osoitteeseen (pysäytä kokonaisuus ensin)
C:\DauntlessRevived\bin\Stack.ps1 stop
C:\DauntlessRevived\bin\Install-DauntlessServer.ps1 -Ref <asennettu commit> -ServerName "Lauantain Ramsgate" -AdminIp 198.51.100.21
```

### Update-DauntlessServer.ps1 {#update-dauntlessserverps1}

Ajetaan palvelimella ylläpitäjänä. Se vaihtaa vain palvelinkoodin. `.env`-tiedostot, avaimet,
varmenne, pelitiedostot ja palomuuri pysyvät ennallaan; niitä varten aja asennusohjelma uudelleen.

1. Se kääntää uuden koodin kansioon `<root>\app.new` palvelimen pyöriessä, normaalia matalammalla
   prioriteetilla.
2. Se ottaa varmuuskopion, pysäyttää kokonaisuuden, vaihtaa `app.new`:n käyttöön (vanhasta
   käännöksestä tulee `app.prev`) ja päivittää kansion `<root>\bin` skriptit uudesta koodista.
3. Se käynnistää kokonaisuuden ja odottaa, kunnes metagame vastaa osoitteessa `/dauntless-status` ja
   julkisessa tilassa yhdyskäytävä vastaa `ServerStatus`-kyselyyn TLS:n yli kutsujen varmenteella.
   Jos näin ei käy `-HealthTimeoutSec`:n kuluessa, se siirtää uuden käännöksen kansioon `app.failed`,
   vaihtaa takaisin `app.prev`:iin, käynnistää sen ja päättyy paluukoodilla `1`.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Root` | polku | Katso [Ennen kuin ajat paketin skriptin](#before-you-run-a-kit-script) | Asennuskansio. |
| `-Ref` | haara, tagi tai commit | `friends-v1` | Lataa tämän refin GitHubista. Tämä on oletusparametrijoukko: ilman yhtään lähdeparametria päivitys kääntää `friends-v1`:n, joka voi olla vanhempi kuin se, mitä palvelin ajaa. **Nimeä lähde aina.** |
| `-SourceZip` | polku | ei mitään | Kääntää lähdekoodizipistä. `Deploy-Remote.ps1 -Update` välittää tämän. |
| `-SourceCommit` | 7-40 heksamerkkiä, valinnaisesti päätteellä `-dirty` tai `-worktree` | `zip-<zipin SHA-256:n 12 heksamerkkiä>` | `-SourceZip`:n commit. |
| `-SourceDir` | kansio, jossa on `legacy\UndauntedMetagame\package.json` | ei mitään | Kääntää paikallisesta kansiosta. |
| `-Rollback` | valitsin | pois | Vaihtaa käsin takaisin `app.prev`:iin: pysäytys (varmuuskopion kanssa), `app` nimetään `app.rolledback`:ksi ja `app.prev` `app`:ksi, kirjattu commit palautetaan, kansion `<root>\bin` skriptit päivitetään, käynnistys ilman uutta varmuuskopiota ja tarkistus. |
| `-Force` | valitsin | pois | Kääntää uudelleen, vaikka sama commit on jo asennettu. Commit, jonka nimi päättyy `-dirty`- tai `-worktree`-päätteeseen, käännetään aina uudelleen. |
| `-HealthTimeoutSec` | sekunteja | `180` | Kuinka kauan uuden käännöksen annetaan tulla toimintakuntoon ennen automaattista paluuta edelliseen. |

Päivitys torjuu uuden koodin, josta puuttuu jokin palvelimen ajama osa, ja varoittaa, kun
pelikansiosta puuttuu palvelimen DLL tai se on muuttunut.

**Oikea eteneminen on tässä versiossa oletus.** `metagame.env`-tiedostosta päivitys muuttaa vain
arvon `GIT_COMMIT`, eikä asennusohjelmakaan koskaan kirjoita `PROGRESSION_MODE`-arvoa: molemmat
säilyttävät sen, mitä tiedostossa on. Palvelin, jonka `metagame.env`-tiedostossa ei ole
`PROGRESSION_MODE`-riviä tai jossa rivi on tyhjä, vaihtaa alkuperäisen projektin valemaksimitasoista
oikeaan etenemiseen, kun se ajaa uutta koodia. Jos haluat pitää valemaksimitasot, lisää
`metagame.env`-tiedostoon rivi `PROGRESSION_MODE=stub` ennen päivitystä. Lue
[päivitysohjeet]({{ upgrade_page.url | relative_url }}) ennen kuin päivität palvelimen, jolla
on jo pelaajia.

Onnistuneen päivityksen jälkeen päivitys odottaa enintään 20 sekuntia metagamen etenemistilan
riviä tiedostossa `datalogsmetagame.out.log`, ellei `metagame.env`-tiedostossa lue
`PROGRESSION_MODE=real` tai `stub`. Jos metagame varoittaa silloin, että joillakin tileillä ei ole
vielä tallennettua etenemistä, päivitys toistaa varoituksen ja linkin päivitysohjeisiin. Tämä
tarkistus ei koskaan kaada päivitystä.

```powershell
C:\DauntlessRevived\bin\Update-DauntlessServer.ps1 -Ref <tagi tai commit>
C:\DauntlessRevived\bin\Update-DauntlessServer.ps1 -SourceZip C:\DauntlessRevived\staging\source\source-<12 heksamerkkiä>.zip -SourceCommit <commit>
C:\DauntlessRevived\bin\Update-DauntlessServer.ps1 -Rollback
```

### Stack.ps1 {#stackps1}

Yksi komento koko kokonaisuudelle. Osat käynnistysjärjestyksessä: sallittujen listan apuri (vain
julkinen tila), metagame, sisältöpalvelin, yhdyskäytävä (vain julkinen tila) ja deploy-palvelin,
joka käynnistää pelipalvelimet. `server.json` luettelee, mitä osia tämä asennus ajaa. Prosessit
tunnistetaan niiden koko komentorivistä, joten saman koneen muihin Node-ohjelmiin ja
Dauntless-peliohjelmaan ei koskaan kosketa.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| toiminto (ensimmäinen parametri) | `status`, `start`, `stop`, `restart` tai `supervise` | `status` | Katso alla oleva taulukko. |
| `-Root` | polku | Katso [Ennen kuin ajat paketin skriptin](#before-you-run-a-kit-script) | Asennuskansio. |
| `-Only` | yksi tai useampi näistä: `metagame`, `content`, `deploy`, `gateway`, `allowlist` | jokainen määritetty osa | Koskee vain näitä osia. Ajastettujen tehtävien kautta vain `restart -Only` on sallittu. |
| `-NoBackup` | valitsin | pois | Ohittaa varmuuskopion, joka muuten vaaditaan ennen metagamen käynnistystä ja otetaan pysäytyksen jälkeen. |
| `-Direct` | valitsin | pois | Käynnistää tai pysäyttää prosessit nykyisellä käyttäjällä, vaikka ajastetut tehtävät ovat olemassa. Vain ylläpitäjille. Asennusohjelma käyttää sitä ajaakseen metagamen kerran. |

| Toiminto | Mitä se tekee |
|:---------|:--------------|
| `status` | Asennus ja sen koodiversio; jokainen osa prosesseineen, osoitteineen ja muistinkäyttöineen, tai `down`; jokainen pelipalvelin UDP-portteineen ja rooleineen (korkein portti, 8777, on Ramsgate, sen alapuolinen on Training Dojo, loput ovat metsästyksiä); yhdyskäytävän TLS-tarkistus; sallittujen listan apuri ja sen palomuurisääntö; ajastetut tehtävät; viimeisin varmuuskopio; pysäytyslippu; milloin viimeisin suorituskykymittaus otettiin (tai että kirjaaminen on pois päältä); ja paikalla olevat pelaajat, jotka metagame näyttää vain omistajan avaimella. |
| `start` | Asennetulla palvelimella ylläpitäjän ajamana: ottaa varmuuskopion, jos metagame ei ole käynnissä (eikä käynnisty ilman sitä, ellei annettu `-NoBackup`), poistaa pysäytyslipun, käynnistää apurin tehtävän ja kokonaisuuden tehtävän ja odottaa enintään 2 minuuttia metagamea ja yhdyskäytävää. Muuten (hiekkalaatikko, palvelutili tai `-Direct`): käynnistää osat itse, ottaa varmuuskopion ennen metagamea, odottaa enintään 30 s kutakin porttia, tarkistaa metagamen ja yhdyskäytävän ja odottaa enintään 60 s Ramsgatea UDP-portissa 8777. |
| `stop` | Asettaa pysäytyslipun, lopettaa ajastetut tehtävät (kun ylläpitäjä ajaa sen asennetulla palvelimella), pysäyttää deploy-palvelimen ja sen pelipalvelimet, sitten yhdyskäytävän, sisältöpalvelimen, metagamen ja sallittujen listan apurin. Se sulkee peliportit (apurin sääntö laitetaan pois käytöstä) ja ottaa varmuuskopion. |
| `restart` | `stop`, varmuuskopio ja sitten `start`. Tehtävien kautta `restart -Only <osa>` vain pysäyttää sen osan; sen valvoja käynnistää sen uudelleen minuutin sisällä ilman varmuuskopiota. |
| `supervise` | Ajetaan vain ajastettujen tehtävien sisällä (tai hiekkalaatikossa). Se käynnistää osat ja tarkistaa ne sitten 15 sekunnin välein. Alhaalla oleva osa käynnistetään uudelleen `min(60, 5 * 2^(n-1))` sekunnin päästä, missä `n` on osan kaatumisten määrä viimeisten 10 minuutin aikana. Yli 5 kaatumisen jälkeen valvoja luopuu siitä osasta seuraavaan käynnistykseen asti. Se lopettaa, kun pysäytyslippu on asetettu; SYSTEM-tilillä pyörivä apurin valvoja ei välitä lipusta, vaan se lopetetaan tehtävänsä kautta. Valvottu käynnistys ohittaa varmuuskopion, jos uusin kopio on alle 10 minuuttia vanha. Joka neljännellä tarkistuksella (noin kerran minuutissa) valvoja ottaa myös suorituskykymittauksen, kuten [Write-PerformanceLog.ps1](#write-performancelogps1) kuvaa, ellei `server.json`-tiedostossa ole `"PerformanceLog": false`; SYSTEM-tilillä pyörivä apurin valvoja ei koskaan mittaa. Mittaus, jota ei voi kirjoittaa (tiedosto on auki toisessa ohjelmassa, tai kansioon `data\logs` on istutettu linkki), jätetään väliin, ja `supervisor.log` kertoo siitä kerran. |

Palvelutili ei koskaan aja sallittujen listan apuria, eikä SYSTEM aja mitään muuta. Siksi
asennetulla palvelimella `start` ja `stop` toimivat kahden ajastetun tehtävän kautta. Jokainen osa
ajetaan omassa kansiossaan muodossa `node --env-file=<data\config\X.env> <aloitusskripti>`. Ennen
osan käynnistämistä `Stack.ps1` poistaa omasta ympäristöstään jokaisen tuon `.env`-tiedoston
muuttujan, jotta tiedoston arvot voittavat.

```powershell
C:\DauntlessRevived\bin\Stack.ps1                     # status
C:\DauntlessRevived\bin\Stack.ps1 restart -Only gateway
C:\DauntlessRevived\bin\Stack.ps1 stop
C:\DauntlessRevived\bin\Stack.ps1 start
```

### New-Invite.ps1 {#new-inviteps1}

Ajetaan palvelimella ylläpitäjänä. Se lukee omistajan avaimen tiedostosta `data\keys\owner.key`
(eikä koskaan tulosta sitä) ja kutsuu metagamea suoraan sen omassa osoitteessa, koska yhdyskäytävä
torjuu ylläpitoreitit. Ennen kuin se tulostaa mitään, se tarkistaa, että omistajan avain kuuluu
ylläpitäjälle.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Root` | polku | Katso [Ennen kuin ajat paketin skriptin](#before-you-run-a-kit-script) | Asennuskansio. |
| `-Uses` | 1-100 | `1` | Montako tiliä uudella koodilla voi luoda. |
| `-For` | teksti ilman ohjausmerkkejä | ei mitään | Merkintä omaa kirjanpitoasi varten. Metagame kirjoittaa sen (tulostettavat ASCII-merkit, enintään 64 merkkiä) uuden koodin lokiriville; sitä ei tallenneta koodin mukana. |
| `-ShareUrl` | `https://login.tailscale.com/...` | tallennettu jakolinkki | Yksityinen tila: Tailscalen koneenjakolinkki, joka lisätään kutsuun muodossa `&share=`. Torjutaan julkisessa tilassa. |
| `-SaveShareUrl` | valitsin | pois | Yksityinen tila: tallentaa `-ShareUrl`:n tiedostoon `server.json`, jolloin myöhemmät kutsut sisältävät sen. |
| `-AdvertiseHost` | IPv4-osoite tai DNS-nimi | julkinen tila: tallennettu `PublicHost`; yksityinen tila: tallennettu ilmoitettava isäntä, muuten sidososoite | Isäntä, joka kirjoitetaan tähän kutsuun. |
| `-SkipGatewayCheck` | valitsin | pois | Julkinen tila: ohittaa tarkistuksen, että yhdyskäytävä vastaa osoitteessa `127.0.0.1` `server.json`-tiedoston varmenteella. |
| `-List` | valitsin | | Luettelee avoimet koodit ja kunkin jäljellä olevat käyttökerrat. |
| `-Revoke` | kutsukoodi | | Poistaa koodin. Sillä luodut tilit säilyvät. |

Se luo koodin metagamen `CreateInvite`-reitillä (vanhemmissa metagameissa `RegisterInviteCode`
skriptin tekemällä koodilla), tarkistaa, että koodi on palvelimen listalla, jäsentää valmiin
kutsurivin takaisin samalla tiukalla jäsentimellä, jota käynnistin käyttää, ja tulostaa yhden rivin:

```text
dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=<64 heksamerkkiä>&code=ABCD-EFGH-JKLM&name=Lauantain%20Ramsgate
dauntless-revived://join?v=1&host=100.x.y.z&port=61000&code=ABCD-EFGH-JKLM&name=Koti&share=<URL-koodattu jakolinkki>
```

`v=2` on julkinen tila: `fp` on yhdyskäytävän varmenteen SHA-256, ja käynnistin hyväksyy sen
varmenteen eikä mitään muuta. `v=1` on yksityinen tila Tailscalen yli. Kutsukoodilla voi luoda
tilin, joten lähetä se yksityisesti. Julkinen tila kieltäytyy tekemästä kutsua, kun varmennetiedosto
ei enää vastaa `server.json`-tiedostoon tallennettua sormenjälkeä.

```powershell
C:\DauntlessRevived\bin\New-Invite.ps1 -For friend1
C:\DauntlessRevived\bin\New-Invite.ps1 -Uses 3 -For "Lauantain porukka"
C:\DauntlessRevived\bin\New-Invite.ps1 -List
C:\DauntlessRevived\bin\New-Invite.ps1 -Revoke ABCD-EFGH-JKLM
```

### Get-ServerStatus.ps1 {#get-serverstatusps1}

Kutsuu `GET /undaunted/api/ServerStatus` ja tulostaa palvelimen nimen ja version, paikalla olevat
pelaajat ja sen, missä he ovat, sekä jokaisen käynnissä olevan maailman ja metsästyksen. Palvelin
näyttää pelaajat ja pelipalvelimet vain rekisteröityneille pelaajille, joten pyynnössä on tiliavain.
Ilman avainta, tai avaimella, jota palvelin ei hyväksy, skripti kertoo, että lista on piilotettu,
sen sijaan että tulostaisi 0 pelaajaa.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| (ei mitään) | | | Kysyy tämän koneen omalta palvelimelta. Julkisessa tilassa kysely kulkee yhdyskäytävän kautta osoitteessa `127.0.0.1` TLS:llä, joka on kiinnitetty varmenteen sormenjälkeen, kuten kaverin käynnistin tekee. Se lähettää omistajan avaimen, kun se pystyy lukemaan sen. |
| `-Root` | polku | Katso [Ennen kuin ajat paketin skriptin](#before-you-run-a-kit-script) | Asennuskansio (paikallinen käyttö). |
| `-Direct` | valitsin | pois | Paikallinen käyttö: kysyy metagamelta sen omasta osoitteesta yhdyskäytävän sijaan. |
| `-Invite` | kutsurivi | | Kysyy kutsussa nimetyltä palvelimelta: v2 TLS:n yli kiinnitettynä sen `fp`:hen, v1 salaamattomalla HTTP:llä. |
| `-Server` | `host` tai `host:port` | | Kysyy miltä tahansa palvelimelta. `-Fingerprint`:n kanssa TLS:n yli (portti 443, ellei muuta anneta); ilman sitä salaamattomalla HTTP:llä (portti 61000, ellei muuta anneta). |
| `-Fingerprint` | 64 heksamerkkiä | ei mitään | Varmenteen sormenjälki `-Server`:ille. |
| `-KeyFile` | polku | paikallisesti omistajan avain | `account.key` tai käynnistimen avainvarmuuskopio (`Key: ...` -rivi hyväksytään). **Salainen: älä koskaan jaa, älä koskaan committoi.** Avain lähetetään vain sormenjälkeen kiinnitetyn TLS:n yli tai salaamattomalla HTTP:llä osoitteisiin `localhost` ja `127.x.x.x`, Tailscale-osoitteeseen (100.64.0.0/10) tai `*.ts.net`-nimeen; kaikki muu torjutaan. |
| `-Json` | valitsin | pois | Tulostaa palvelimen vastauksen sellaisenaan (`"limited": true`, kun lista on piilotettu). |
| `-TimeoutSec` | sekunteja | `8` | HTTP-aikakatkaisu. |

Paluukoodi on `0`, kun palvelin vastasi, ja `1`, kun se ei vastannut tai vastasi HTTP-virheellä.
Metagame, joka on liian vanha tunteakseen `ServerStatus`-reitin, näytetään käynnissä olevana ilman
pelaajia ja pelipalvelimia. Kun kiinnitetty TLS-yhteys
epäonnistuu, koska palvelin näyttää eri varmenteen, skripti kertoo sen: joko palvelin on saanut
uuden varmenteen tai joku on välissä.

```powershell
C:\DauntlessRevived\bin\Get-ServerStatus.ps1
.\Get-ServerStatus.ps1 -Invite 'dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=...&code=...&name=...' -KeyFile .\account.key
.\Get-ServerStatus.ps1 -Server 203.0.113.7:443 -Fingerprint <64 heksamerkkiä> -Json
```

Palvelimella itsellään se tulostaa myös `chat:`-rivin, kuten `Stack.ps1 status`.

### Set-Chat.ps1 {#set-chatps1}

Kytkee pelin tekstichatin päälle tai pois asennetulla julkisen tilan palvelimella
([Chat]({{ winserver_page.url | relative_url }}#chat)). Se tarvitsee järjestelmänvalvojan
PowerShellin, paitsi `-Sandbox`-asennuksessa.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-On` / `-Off` | valitsin, toinen niistä **pakollinen** | | Uusi tila. |
| `-Root` | polku | `C:\DauntlessRevived` | Asennuskansio. |

Se kirjoittaa tiedostoon `metagame.env` asetukset `CHAT=1` tai `0`, `CHAT_BIND_HOST=127.0.0.1` ja
`CHAT_PORT` (61099, hiekkalaatikossa 62099) sekä tiedostoon `server.json` kohdan `"Chat"`. Sitten se
ajaa komennot `Stack.ps1 stop` ja `start` tavallisine varmuuskopioineen ja odottaa enintään 30
sekuntia, että `127.0.0.1:<portti>` alkaa kuunnella (tai lakkaa kuuntelemasta). Jos jokin vaihe
epäonnistuu, se palauttaa molemmat tiedostot ennalleen, käynnistää kokonaisuuden taas uudelleen ja
päättyy koodilla `1`. Jos asetus on jo pyydetty, se ei muuta mitään eikä käynnistä mitään uudelleen;
se katsoo vain kolmea chat-asetusta ja kohtaa `"Chat"`, joten se, mitä asennusohjelma tai päivitys
kirjoitti muualle tiedostoon `metagame.env`, ei vaikuta. Uudelleenkäynnistys pudottaa ryhmät ja
matchmaking-jonot, jotka ovat vain muistissa: aja se, kun kukaan ei pelaa. Omalta koneelta
`Deploy-Remote.ps1 -Server <osoite> -Chat On` ajaa sen SSH:n kautta. Jos palvelin on asennettu ennen
chattia, skripti tulee päivityksen mukana, joten päivitä ensin. Pohjana on Vvoidddd:n ensimmäinen
chat-kytkin ([#9](https://github.com/mixutin/dauntless-revived/pull/9)).

```powershell
C:\DauntlessRevived\bin\Set-Chat.ps1 -On
C:\DauntlessRevived\bin\Set-Chat.ps1 -Off
```

### Backup-DauntlessServer.ps1 ja backup-hidden.vbs {#backup-dauntlessserverps1-and-backup-hiddenvbs}

`Backup-DauntlessServer.ps1` kopioi kaiken palautukseen tarvittavan kansioon
`<root>\backups\yyyy-MM-dd_HHmmss\`: tietokannan (otettu SQLiten online backup -rajapinnalla ja
tarkistettu), `server.json`-tiedoston, uutistiedoston, metagamen, deploy-palvelimen,
sisältöpalvelimen ja yhdyskäytävän `.env`-tiedostot, jokaisen `*.key`-tiedoston sekä yhdyskäytävän
varmenteen yksityisine avaimineen. Rakenne kuvataan sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}). `allowlist.env`-tiedostoa ei kopioida:
asennus tekee uuden. **Varmuuskopiossa on kaikki avaimet: älä koskaan jaa sitä, älä koskaan
committoi sitä, ja kopioi se palvelimelta pois vain salattuna.**

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Root` | polku | Katso [Ennen kuin ajat paketin skriptin](#before-you-run-a-kit-script) | Asennuskansio. |
| `-Hourly` | luku | `48` | Säilytä N uusinta varmuuskopiota. |
| `-Daily` | luku | `30` | Säilytä lisäksi uusin varmuuskopio jokaiselta viimeisestä M päivästä, joilta sellainen on. |

Se pyörii normaalia matalammalla prioriteetilla, lisää jokaisesta ajosta yhden rivin tiedostoon
`backups\backup.log` (joka siirretään nimelle `backup.log.1`, kun se kasvaa yli 5 Mt:n) ja päättyy
paluukoodilla `1` vain, kun tietokannan kopio ei läpäise tarkistusta. Palvelin, jolla ei vielä ole
tietokantaa, ei ole virhe. Se ajetaan tunnin välein varmuuskopiotehtävästä, ennen jokaista
käynnistystä ja jokaisen pysäytyksen jälkeen (`Stack.ps1`) sekä ennen jokaista päivitystä.

`backup-hidden.vbs` ajaa sen ilman ikkunaa, joten kirjautuneen käyttäjän työpöydälle ei välähdä
mitään: `wscript.exe backup-hidden.vbs [asennuskansio]`. Ilman argumenttia asennuskansio on skriptin
kansiota ylempi kansio. Se palauttaa varmuuskopion paluukoodin.

```powershell
C:\DauntlessRevived\bin\Backup-DauntlessServer.ps1
C:\DauntlessRevived\bin\Backup-DauntlessServer.ps1 -Hourly 96 -Daily 60
```

### Write-PerformanceLog.ps1 {#write-performancelogps1}

Kirjaa, mitä palvelin käyttää (tiekartan kohta 4.12). Yksi mittaus on `host`-rivi ja yksi rivi
jokaisesta prosessista: tämän asennuksen metagame, sisältöpalvelin, yhdyskäytävä, deploy-palvelin ja
sallittujen listan apuri (tunnistetaan komentorivistä, kuten `Stack.ps1` ne tunnistaa) sekä jokainen
pelipalvelin, joka on käynnistetty tämän asennuksen pelikansiosta. Rivit lisätään tiedostoon
`<root>\data\logs\performance\performance-<UTC-päivä>.csv`; sarakkeet ovat sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}#performance-log). Parametria `-KeepDays`
vanhemmat tiedostot poistetaan. Pohjana on Vvoidddd:n ensimmäinen mittari
([#6](https://github.com/mixutin/dauntless-revived/pull/6)).

Asennetulla palvelimella kokonaisuuden valvoja ottaa mittauksen jo minuutin välein (katso
`supervise` kohdassa [Stack.ps1](#stackps1)). Aja tämä skripti siis, kun haluat yhden mittauksen heti
(`-Once`), tai `-Sandbox`-asennuksessa tai käsin käynnistetyssä kokonaisuudessa, joissa valvojaa ei
ole.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Root` | polku | Katso [Ennen kuin ajat paketin skriptin](#before-you-run-a-kit-script) | Asennuskansio. |
| `-Once` | valitsin | pois | Ottaa vertailukohdan, odottaa 2 sekuntia, kirjoittaa yhden mittauksen ja lopettaa. |
| `-IntervalSeconds` | 1-3600 | `60` | Sekunteja mittausten välillä. Ensimmäinen mittaus tulee yhden välin kuluttua käynnistyksestä. Pysäytä Ctrl+C:llä. |
| `-KeepDays` | 1-3650 | `30` | Montako päivätiedostoa säilytetään, tämä päivä mukaan lukien. |

Näin luvut saadaan:

- **Prosessin suoritinkäyttö:** sen käyttämä suoritinaika edellisen mittauksen jälkeen prosentteina
  **yhdestä ytimestä** (kaksi täysin kuormitettua ydintä näkyy lukuna 200). Koneen suoritinkäyttö on
  koko koneen (0-100), ja se luetaan suorittimen laskurista, joka toimii samoin Windowsin kielestä
  riippumatta. Käynnistyksen ensimmäisessä mittauksessa suoritinlukuja ei ole.
- **Muisti:** jokaisen prosessin working set ja yksityinen (varattu) muisti; koneen koko ja vapaa
  keskusmuisti.
- **Pelipalvelimet:** UDP-portti luetaan prosessin UDP-päätepisteestä (väliltä 8700-8799), ei koskaan
  sen komentoriviltä, joka alkaa pelipalvelimen avaimella ja luettelee odotettujen pelaajien
  tilitunnukset. Rooli (`ramsgate`, `dojo`, `hunt`, `tutorial`) tulee deploy-palvelimen listasta tai
  portista samalla tavalla kuin `Stack.ps1 status` sen päättelee (`server.json`-tiedoston
  `UdpPortEnd` on Ramsgate, yhtä alempi Dojo).
- **Pelaajat:** metagamen `ServerStatus` omistajan avaimella kysyttynä ja deploy-palvelimen
  `/gameservers`, yhdistettyinä palvelimen tunnisteen perusteella. Vain määrät säilytetään; vastausten
  nimet ja tilitunnukset hylätään. Jos omistajan avainta ei voi lukea, määrät jäävät tyhjiksi, eivät
  koskaan 0:ksi.
- **Verkko:** verkkosovittimien tavut ilman loopbackia, VPN-tunneleita (Tailscale, WireGuard) ja
  virtuaalikytkimiä, joiden liikenne kulkee myös oikean sovittimen kautta.

Se ei kirjoita mitään liitoskohdan (junction), symbolisen linkin tai kovan linkin kautta: jos
`data\logs`, sen `performance`-kansio tai päivän tiedosto on sellainen, mittaus torjutaan. Lukema,
joka epäonnistuu (esimerkiksi metagame ei ole käynnissä), jättää soluunsa tyhjää; muu rivi
kirjoitetaan silti. Paluukoodi on `1`, kun `-Once` ei pystynyt kirjoittamaan mittaustaan.

```powershell
C:\DauntlessRevived\bin\Write-PerformanceLog.ps1 -Once
.\Write-PerformanceLog.ps1 -Root C:\dr\sandbox-ws2019\root -IntervalSeconds 30
```

### Receive-Upload.ps1 {#receive-uploadps1}

`Deploy-Remote.ps1`:n osissa tehtävän lähetyksen palvelinpää. Et aja sitä itse.

| Parametri | Tyyppi | Mitä se tekee |
|:----------|:-------|:--------------|
| `-Action` | `Begin`, `Verify`, `Assemble` tai `Clean`, **pakollinen** | `Begin` ottaa tiedoston `upload.json.incoming` uudeksi `upload.json`-tiedostoksi (ja poistaa vanhat osat, jos se kuvaa eri lähetystä) ja kertoo jo tarkistetut osat. `Verify` laskee yhden osan tiivisteen. `Assemble` yhdistää osat laskien kunkin tiivisteen uudelleen, tarkistaa koko tiedoston ja nimeää sen paikalleen. `Clean` poistaa osat. |
| `-Dir` | kansio, **pakollinen** | Lähetyskansio (`<root>\staging\upload`). |
| `-Index` | osan numero nollasta laskien | Osa, jonka `Verify` tarkistaa. |

`upload.json`-tiedoston osakoon on oltava 1 Mt:n ja 2 Gt:n välillä. Jokainen kutsu tulostaa yhden
`DRJSON:{...}`-rivin.

### Apuskriptit kansiossa lib\ {#helper-scripts-in-lib}

Paketin skriptit kutsuvat näitä palvelimen `node.exe`:llä. Aja niitä käsin vain, kun tiedät miksi.

| Komento | Mitä se tekee |
|:--------|:--------------|
| `node dr-db.js integrity <metagame-kansio> <tietokanta>` | Vain lukeva eheystarkistus. Tulostaa `ok, <n> users`. |
| `node dr-db.js backup <metagame-kansio> <tietokanta> <kohde>` | SQLiten online backup (turvallinen metagamen pyöriessä), sitten kopion tarkistus. |
| `node dr-db.js add-invite <metagame-kansio> <tietokanta> <koodi>` ja `del-invite ...` | Lisää tai poistaa kertakäyttöisen kutsukoodin. Asennusohjelma käyttää tätä omistajan rekisteröimiseen. |
| `node dr-db.js make-admin <metagame-kansio> <tietokanta> <UserId>` | Tekee tilistä ylläpitäjän. |
| `node dr-db.js gs-key <metagame-kansio> <tietokanta> <avaintiedosto>` | Rekisteröi pelipalvelimen avaimen SHA-256:n. Avain luetaan tiedostosta, sen on oltava vähintään 32 merkkiä pitkä, eikä sitä koskaan tulosteta. |
| `node dr-keys.js signing <tulostiedosto>` | Kirjoittaa uuden RSA-2048-avainparin RS256-tunnusten allekirjoitukseen kahtena `.env`-rivinä (`AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64`). Se ei tulosta mitään salaista, mutta **tulostiedosto on salainen: älä koskaan jaa, älä koskaan committoi.** Asennusohjelma yhdistää sen `metagame.env`-tiedostoon ja poistaa tiedoston. |
| `node verify-game.js <manifest.json> <pelikansio> [--quick]` | Tarkistaa jokaisen luettelossa olevan tiedoston koon ja SHA-256:n (`--quick`: vain koon). Ylimääräiset tiedostot ohitetaan. Paluukoodi `0`: kaikki täsmäävät, `1`: poikkeama, `2`: käyttövirhe, vaarallinen polku luettelossa tai lukuvirhe. |

`dr-db.js` lataa `better-sqlite3`:n metagamen omasta `node_modules`-kansiosta ja päättyy
paluukoodilla `1` missä tahansa virheessä. `<metagame-kansio>` on `<root>\app\UndauntedMetagame` ja
tietokanta on `<root>\data\undaunted.db`.

### Palautus, paluu edelliseen versioon ja poistaminen {#restore-rollback-and-uninstall}

Näille ei ole erillisiä skriptejä:

| Tehtävä | Miten |
|:--------|:------|
| Palauta varmuuskopio tai siirrä palvelin | `Install-DauntlessServer.ps1 -RestoreFrom <varmuuskopiokansio>`, tai omalta koneeltasi `Deploy-Remote.ps1 -RestoreFrom <paikallinen varmuuskopiokansio>`. Kansiossa on oltava `undaunted.db`, `secrets\metagame.env` ja `secrets\deployserver.env`. Tietokanta tarkistetaan ensin, eikä olemassa olevaa tietokantaa koskaan korvata: poista se ensin, jos tarkoitus on korvata se. Varmuuskopion `metagame.env`- ja `deployserver.env`-tiedostojen asetukset voittavat olemassa olevat. Tunnusten allekirjoitusavaimet, pelipalvelimen avain, `*.key`-tiedostot (olemassa olevat säilytetään) ja yhdyskäytävän varmenne tulevat mukana, joten jo annetut kutsut toimivat edelleen. Varmuuskopion varmennetta käytetään vain, jos tällä palvelimella ei vielä ole varmennetta eikä `-NewCertificate`-valitsinta ole annettu. Yhdyskäytävän ja sallittujen listan salaisuuksia ei oteta varmuuskopiosta: ne säilyvät tai tehdään uusina. Sekä `Backup-DauntlessServer.ps1`:n että projektin oman palvelinkoneen `backup.ps1`:n tekemät varmuuskopiot kelpaavat. |
| Palaa edelliseen käännökseen | `Update-DauntlessServer.ps1 -Rollback`. |
| Poistaminen | Käsin ajettavat komennot: [Poistaminen]({{ winserver_page.url | relative_url }}#uninstall). |

### Paketin testit {#kit-tests}

Kehityskoneelle. Mikään niistä ei muuta palomuuria, palveluita, ajastettuja tehtäviä, tilejä tai
varmennesäilöjä, eikä mikään käytä pyörivän palvelimen portteja.

Jos muokkaat paketin skriptiä, pidä se pelkkänä ASCIIna: Windows PowerShell 5.1 lukee tiedoston,
jossa ei ole BOM-merkkiä, ANSI-merkistönä. Kansion `deploy/windows-server/` `.gitattributes` pitää
tiedostojen `*.ps1`, `*.vbs` ja `*.md` rivinvaihdot CRLF-muodossa myös `git archive`:ssa, jonka
`Deploy-Remote.ps1` lähettää.

| Skripti | Parametrit | Mitä se testaa |
|:--------|:-----------|:---------------|
| `tests\Test-KitUnit.ps1` | `-WorkDir` (oletus `%TEMP%\dr-kit-unit`; tyhjennetään alussa, poistetaan lopussa); `-Port` (62000-62499, oletus `62450`; suorituskykytesti käyttää myös sitä seuraavaa porttia) | Jokainen paketin skripti jäsentyy PowerShell 5.1:ssä ja on pelkkää ASCIIta; kutsurivit v1 ja v2 ja kaikki, mikä pitää torjua; osoitteet ja `.env`-säännöt; varmenteiden sormenjäljet; TLS-kiinnitys paikallista testipalvelinta vasten; `Get-ServerStatus.ps1` avaimen kanssa ja ilman; avaintiedostot; lähetysapuri; suorituskykymittari (suoritinlaskut, roolit porttialueesta, kiinteä otsikkorivi, päivätiedostot ja niiden karsinta, pelaajamäärät korvikemetagamesta ilman nimiä tai avaimia tiedostossa, desimaalipiste suomenkielisessä Windowsissa sekä liitoskohdan ja kovan linkin torjunta); chat-asetukset (`CHAT_PORT` sama kuin yhdyskäytävän WebSocket-portti tavallisessa asennuksessa ja hiekkalaatikossa, `-Chat` verrattuna tiedostoon `server.json`, avainten säilyminen uudessa ajossa ja `chat`-tilarivi). Tarvitsee `node`:n `PATH`:issa ja `npm ci`:n kansiossa `legacy/UndauntedGateway`. |
| `tests\Test-DeployRemote.ps1` | `-WorkDir` (oletus `%TEMP%\dr-deploy-test`; tyhjennetään alussa, poistetaan lopussa) | `Deploy-Remote.ps1` ilman palvelinta: parametrien torjunnat, `-WhatIf`, paketin, lähdekoodin ja varmuuskopion lähetys, osissa tehtävä lähetys katkenneella yhteydellä ja vahingoittuneilla osilla sekä `-Chat` (välitetään asennusohjelmalle tai yksinään `Set-Chat.ps1`). Ei verkkoa, ei SSH-avainta. |
| `tests\Test-Sandbox.ps1` | `-SandboxDir` (oletus `C:\dr\sandbox-ws2019`; kansion nimessä on oltava `sandbox`, koska kansio poistetaan, ja sen on oltava työkopion ulkopuolella, koska asennusohjelma kopioi työkopion); `-KeepSandbox`; `-SkipRestore` | Täysi julkisen tilan `-Sandbox`-asennus chat päällä väliaikaiseen kansioon, sitten kutsut, tila, yhdyskäytävän torjunnat, rekisteröityminen yhdyskäytävän kautta, pelin chat-yhteys yhdyskäytävän kautta (101), päivitys ja paluu edelliseen, `Set-Chat.ps1 -Off` (taas 502), suorituskykymittaus, varmuuskopio, palautus toiseen kansioon ja siivous. Tarvitsee vapaat portit 62000, 62002, 62005, 62099 ja 62443 ja kääntää koodin `npm ci`:llä. Loki kopioidaan tiedostoon `%TEMP%\dr-sandbox-test.log`, ellei annettu `-KeepSandbox`. |

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-KitUnit.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-DeployRemote.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-Sandbox.ps1 -SandboxDir D:\scratch\dr-sandbox
```

### Paketin vakiot {#kit-constants}

Kiinteät arvot tiedostossa `deploy/windows-server/DauntlessServer.Common.ps1`. Portit voi vaihtaa
asennuskohtaisesti, nimiä ei.

| Vakio | Arvo | Käyttö |
|:------|:-----|:-------|
| `$DRNames.ServiceUser` | `dauntless` | Vähäisin oikeuksin toimiva paikallinen tili, joka ajaa kokonaisuutta. |
| `$DRNames.StackTask` | `Dauntless Revived stack` | Ajastettu tehtävä: `Stack.ps1 supervise` tilillä `dauntless` minuutti koneen käynnistymisen jälkeen (tai `-InteractiveSession`:n kanssa tilin kirjautuessa). |
| `$DRNames.AllowlistTask` | `Dauntless Revived allowlist` | Ajastettu tehtävä, julkinen tila: `Stack.ps1 supervise -Only allowlist` SYSTEM-tilillä koneen käynnistyessä. |
| `$DRNames.BackupTask` | `Dauntless Revived backup` | Ajastettu tehtävä: `backup-hidden.vbs` tilillä `dauntless` tunnin välein. |
| `$DRNames.FirewallGroup` | `Dauntless Revived` | Paketin palomuurisääntöjen ryhmä. |
| `$DRNames.AllowlistRule` | `Dauntless Revived game ports (allowlist)` | UDP-peliporttisäännön näyttönimi. |
| `$DRNames.AllowlistRuleName` | `DauntlessRevived-GamePorts-Allowlist` | Saman säännön nimi. Sen on vastattava tiedostoa `legacy/UndauntedGateway/src/allowlist/firewall.ts`. |
| `$DRDefaultPorts` | metagame 61000, deploy 61001, content 61002, gateway 443, allowlist 61005 | Oletus-TCP-portit. |
| `$DRSandboxPorts` | metagame 62000, deploy 62001, content 62002, gateway 62443, allowlist 62005 | `-Sandbox`-asennuksen TCP-portit. |
| `$DRRelayPort` | 61000 | Kaverikäynnistimen välittimen portti. Julkinen tila lähettää jokaiselle pelaajalle QoS-osoitteen `http://127.0.0.1:61000/QoS`, joten portti on kiinteä. |
| `$DRChatPort` | 61099 | Chat-portti (XMPP) palvelutilin `Engine.ini`-tiedostossa ja yhdyskäytävän WebSocket-kohde (hiekkalaatikossa 62099). |
| `$DRUdpBegin`, `$DRUdpEnd` | 8770, 8777 | Pelipalvelinten UDP-porttialue. |
| `$DRRepo.Url` | `https://github.com/mixutin/dauntless-revived` | Mistä asennusohjelma ja päivitys lataavat koodin, ja lähdekoodilinkki, jonka metagame ilmoittaa. |
| `$DRRepo.PinnedRef` | `friends-v1` | Ref, joka käännetään, kun lähdettä ei nimetä. |

**Kiinnitetyt tiivisteet.** Jokainen arvo on kopioitu useaan tiedostoon. Muuta ne kaikki yhdessä.

| Mikä | SHA-256 | Kiinnitetty myös tiedostoissa |
|:-----|:--------|:------------------------------|
| Pelizip, 1.4.4 (`BaseGame144.zip`, 10 479 214 119 tavua) | `556B9A648A5E5E7E11B6F8DD3D80FF8E88FCEB0D3448297AAF47CE7BF756BC6D` | `friend-kit/setup.ps1`, `tools/make-game-manifest.ts` |
| `Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe` | `D3D41E614908D2BEFD518B27046D9822D6130EF12BA3504BABBDB786BEF9CFF4` | `friend-kit/setup.ps1`, `friend-kit/play.ps1`, `tools/make-game-manifest.ts`, `legacy/UndauntedLauncher/src/main/constants.ts`, sisältöluettelo |
| `dxgi.dll` | `9A431D7B6FD20C43FA92BEBD91C3BC023EC7A3FCBC52871C41F4DF293D4B0D1F` | `friend-kit/setup.ps1`, `friend-kit/play.ps1`, `tools/make-friend-kit.ps1`, `legacy/UndauntedLauncher/src/main/constants.ts` |
| `UndauntedInternalServer.dll` | `520EC588A0554E374B2B0D084CD7F7F08D59A9CB80362679845719D64A0D0933` | samat tiedostot kuin `dxgi.dll`:llä |
| Node.js 24.19.0 MSI (`node-v24.19.0-x64.msi`) | `F0F66C2A80C08A30A5AB5179EE9EA9E45F9B46289436A8CC87FF833B852DB351` | vain paketissa; sen on vastattava myös nodejs.org:n `SHASUMS256.txt`-tiedostoa |
| Tailscale 1.102.4 MSI (yksityinen tila) | `80EB007E39DFEBE17299FA1A09C79A8E1D934F76E0246C0817EBE3AF675B7EF6` | vain paketissa; sen on vastattava myös julkaistua `.sha256`-tiedostoa |

Koontiversion merkkijono on `dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827`:
asennusohjelma vertaa `Version.txt`-tiedostoa siihen ja kirjoittaa muutoslistan numeron
(changelist), `239827`, metagamelle. Visual C++ -ajonaikaista kirjastoa ja DirectX June 2010
-ajonaikaista kirjastoa ei ole kiinnitetty tiivisteellä. Ne hyväksytään vain, jos niissä on kelvollinen
Microsoftin Authenticode-allekirjoitus. Zipin koko on kirjattu pakettiin, mutta mikään skripti ei
tarkista sitä.

## Kaveripaketti {#friend-kit}

`friend-kit/` on vanhempi tapa liittyä kaverina: kaksi PowerShell-skriptiä, kummallakin
kaksoisnapsautettava käynnistystiedosto. Ne puhuvat salaamatonta HTTP:tä osoitteeseen
`<host>:61000`, joten ne toimivat yksityisen tilan (Tailscale) palvelimen tai Tailscalen kautta
tavoitettavan käsin pystytetyn palvelinkoneen kanssa. Julkisen tilan palvelinpaketilla asennettu
palvelin näyttää ulospäin vain TLS-yhdyskäytävänsä; sellaisen palvelimen kaverit käyttävät
käynnistintä. Kaverin ohje on [Liity kaverina]({{ friends_page.url | relative_url }}), ja
`tools/make-friend-kit.ps1` rakentaa zipin.

Molemmat skriptit pitävät tiedostonsa kansiossa `%APPDATA%\DauntlessRevived\`:

- `account.key`: kaverin tiliavain. **Salainen: se on pelaajan salasana; älä koskaan jaa, älä
  koskaan committoi, ja varmuuskopioi se.**
- `settings.json`: `{ Server, Game }`, ei salaisuuksia.

### setup.ps1 ja Setup.cmd {#setupps1-and-setupcmd}

Kertaluonteinen asennus, jonka voi ajaa turvallisesti uudelleen. Se tarkistaa pelin exe-tiedoston
kiinnitettyä tiivistettä vasten, kopioi kaksi DLL-tiedostoa kansioon `Archon\Binaries\Win64`
(tarkistetaan ennen kopiointia ja sen jälkeen; otetaan paketin `dll\`-kansiosta tai
repositoriokopiossa kansiosta `..\legacy\UndauntedLauncher\assets\`) ja varoittaa, jos Visual C++
-ajonaikainen kirjasto (`MSVCP140.dll`, `VCRUNTIME140_1.dll`) puuttuu. Sitten se tarkistaa, että
palvelinkone vastaa portissaan, rekisteröityy kerran (vain jos `account.key`-tiedostoa ei vielä ole),
tarkistaa, että avain toimii, ja tallentaa `settings.json`-tiedoston.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Server` | isäntä tai `host:port` | Kysytään, jos puuttuu | Palvelinkoneen Tailscale-osoite tai -nimi. Ilman porttia se käyttää porttia `61000`. |
| `-Game` | kansio, jossa on `Archon\` | `C:\D144\Dauntless` | Pelin asennus. |
| `-Zip` | ladatun zipin polku | ei mitään | Vain tarkistaa zipin kiinnitettyä SHA-256:ta vasten ja lopettaa. Pura zip itse ja aja uudelleen ilman `-Zip`:iä. |
| `-Username` | 3-16 kirjainta, numeroa tai `_` | Kysytään, jos puuttuu | Rekisteröitävän tilin nimi. |
| `-Invite` | kutsukoodi | Kysytään, jos puuttuu | Palvelinkoneen ylläpitäjältä saatu koodi. |

Rekisteröinnin vastaukset: 401 tarkoittaa, että koodi on väärä tai käytetty loppuun, 409 että nimi
on varattu, ja 400 että palvelin kieltäytyi (rekisteröinti on suljettu tai nimi ei kelpaa).
Onnistumisrivi näyttää `logged in as ''` tyhjällä nimellä, koska palvelin vastaa kentällä `Username`
ja skripti lukee kenttää `name`; itse avaimen tarkistus toimii.

`Setup.cmd` ajaa `powershell -NoProfile -ExecutionPolicy Bypass -File setup.ps1` antamillasi
argumenteilla ja pysähtyy aina lopussa.

### play.ps1 ja Play Dauntless.cmd {#playps1-and-play-dauntlesscmd}

Käynnistää pelin ja yhdistää sen palvelinkoneen palvelimeen. Se kieltäytyy käynnistämästä, jos exe
tai jokin DLL ei vastaa kiinnitettyä tiivistettään.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Server` | isäntä tai `host:port` | `settings.json` | Palvelinkone. Ilman porttia se käyttää porttia `61000`. |
| `-Game` | kansio | `settings.json` | Pelin asennus. |
| `-Graphics` | `-1` tai 0-4 | `-1` (käytä pelin valikkoa) | 0-4 pakottaa tämän laatutason (4 = Cinematic) jokaisella käynnistyksellä tiedostoon `Engine.ini` ja, jos se on olemassa, tiedostoon `GameUserSettings.ini`. Arvo, joka on yli 4, torjutaan. |
| `-Windowed` | valitsin | pois | Käynnistää 1280x720-ikkunassa. |
| `-DryRun` | valitsin | pois | Tarkistaa kaiken ja tulostaa käynnistysrivin avain piilotettuna käynnistämättä peliä. Se kirjoittaa silti `Engine.ini`-tiedoston uudelleen. |

Jokaisella käynnistyksellä se kirjoittaa uudelleen tiedoston
`%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\Engine.ini` osiot `[SystemSettings]` ja
`[OnlineSubsystemMcp.XMPP]` ja säilyttää muun tiedoston ennallaan. Rivit luetellaan sivulla
[Pelin asetukset]({{ game_page.url | relative_url }}). Se välittää tiliavaimen pelin komentorivillä
(`-AUTH_PASSWORD=<avain>`), josta saman koneen muut ohjelmat voivat lukea sen.

`Play Dauntless.cmd` ajaa `play.ps1`:n antamillasi argumenteilla ja pysähtyy vain virheen sattuessa.

```powershell
.\setup.ps1 -Server 100.x.y.z -Game D:\Games\Dauntless144
.\play.ps1 -Graphics 3 -Windowed
.\play.ps1 -DryRun
```

## tools/ {#tools}

### dashboard.ts

Valinnainen ylläpitäjän näkymä: aja VPS:llä Bunilla `bun --no-env-file tools/dashboard.ts`.
`Install-OwnerDashboard.ps1` asentaa kiinnitetyn Bunin kansioon `C:\Program Files\Bun` ja ajaa sitä tehtävänä.
`DASHBOARD_OWNER_KEY_FILE` on nykyisen ylläpitoavaimen tiedosto, `DASHBOARD_BACKEND` oletuksena
`http://127.0.0.1:61000`, `DASHBOARD_PORT` oletuksena `61110` ja `DASHBOARD_LOG_FILES` oletuksena
`{}` (JSON-olio: nimi ja lokitiedoston polku). Kuuntelu on vain loopback-osoitteessa.
Avaa SSH-tunnelin kautta ja syötä ylläpitoavain. Komennot: repositorion `tools/DASHBOARD.md`.
Prosessi käynnistetään erikseen; pelipalvelun asetuksia tai tietokantaa ei muuteta.

CPU-, RAM-, pelaajamäärä- ja paikallisen HTTP-viiveen kuvaajat päivittyvät viiden sekunnin välein.
Muistissa säilyy tunti historiaa. Tilimäärä sisältää ylläpitäjät; uusien tilien laskuri kertoo vain
ensimmäisen mittauksen jälkeen havaitut lisäykset, koska rekisteröintipäiviä ei tallenneta.
Lokit päivitetään painikkeella, enintään 32 KB / 150 riviä; tunnistetietoja sisältävät rivit poistetaan.
HTTP-viive ei ole pelin ping. Käynnistys nollaa historian. Testit: `bun test dashboard` kansiossa `tools/`.

### summarize-performance.ts

Aja repositorion juuressa Bunilla:
`bun tools/summarize-performance.ts "C:\DauntlessRevived\data\logs\performance\performance-2026-09-28.csv"`.
Voit antaa useita saman palvelimen CSV-tiedostoja. Työkalu lukee tiedostot ja tulostaa JSON-raportin;
se ei ota yhteyttä palvelimeen eikä muuta tiedostoja. Virheellinen rivi keskeyttää ajon virhekoodiin.
Syötteen on oltava palvelinpaketin mittaajan tuottamaa lainausmerkitöntä CSV:tä.

Raportti näyttää rooleittain havaintojen määrän, mediaanin, p95:n ja maksimin: prosessin CPU,
muisti, pelaajat, koneen CPU, vapaa RAM ja online-pelaajat. Puuttuvat arvot ohitetaan, nollat lasketaan.
Persentiilit käyttävät nearest-rank-menetelmää; jokaisella prosessihavainnolla on sama paino.
Prosessin CPU on prosenttia yhdestä ytimestä, koneen CPU koko koneesta. Metsästyksen luvut koskevat
yhtä prosessia. Ramsgaten peruskulut estävät laskemasta lisäpelaajan hintaa jakamalla muistin
pelaajamäärällä. Turvallista kapasiteettia ei vielä arvioida: tarvitaan vertailukelpoisia pelisessioita
eri pelaajamäärillä. Pidä raportit paikallisina kuten muutkin palvelinlokit.

Testit: `bun test summarize-performance` kansiossa `tools/`.

Aja nämä repositoriokopiosta Node.js:llä, Bunilla tai Windows PowerShellillä, kuten kunkin kohdalla
kerrotaan. `.ts`-työkalut ajetaan Bunilla, sillä versiolla, joka on tiedostossa `tools/package.json`
(`packageManager`); ne eivät tarvitse `bun install` -komentoa, joka tuo vain tyyppitarkistimen.

### make-friend-kit.ps1 {#make-friend-kitps1}

Rakentaa kaveripaketin zipin.

| Parametri | Tyyppi | Oletus | Mitä se tekee |
|:----------|:-------|:-------|:--------------|
| `-Out` | kansio | `C:\dr\dist` (projektin oman palvelinkoneen rakenne; anna omasi) | Minne pakettikansio ja zip tulevat. |

Se kokoaa kansion `<Out>\DauntlessRevived-FriendKit\` (poistetaan ensin, jos se on olemassa), jossa
ovat `setup.ps1`, `play.ps1`, `Setup.cmd`, `Play Dauntless.cmd`, `README.txt`,
`THIRD-PARTY-NOTICES.txt`, kaksi DLL-tiedostoa kansiossa `dll\` (kopioitu kansiosta
`legacy\UndauntedLauncher\assets` ja tarkistettu kiinnitettyjä tiivisteitä vasten), `LICENSE.txt`,
`SOURCE.txt` ja `SHA256SUMS.txt`. Sitten se pakkaa kaiken tiedostoon
`<Out>\DauntlessRevived-FriendKit-<7-merkkinen commit>.zip` ja tulostaa zipin polun ja SHA-256:n.

`SOURCE.txt` nimeää repositorion ja tarkan commitin; se on AGPL:n edellyttämä lähdekoodin tarjous
pelaajille. Repositorion osoite on `origin`-etärepositoriosi osoite ilman `.git`-päätettä, joten
tarkista, että `origin` osoittaa julkiseen repositorioon, ennen kuin jaat zipin. Skripti tarvitsee
`git`:in ja `origin`-etärepositorion, ja se kieltäytyy toimimasta, kun kansiossa `friend-kit\` on
committoimattomia muutoksia.

```powershell
powershell -ExecutionPolicy Bypass -File tools\make-friend-kit.ps1 -Out D:\dist
```

### sync-roadmap.ts {#sync-roadmapts}

`bun tools/sync-roadmap.ts` ei ota argumentteja. Se tuottaa tiedoston `docs/roadmap.md` uudelleen
tiedostosta `ROADMAP.md`: etuaineisto (front matter), sisällysluettelo ja tarkistuslista Liquidin
`raw`-lohkon sisällä, jotta Jekyll jättää sen tekstin rauhaan. Aja se jokaisen `ROADMAP.md`-muutoksen
jälkeen, äläkä koskaan muokkaa tiedostoa `docs/roadmap.md` käsin. Suomenkielinen
`docs/fi/roadmap.md` on käsin kirjoitettu tiivistelmä, jota skripti ei tuota.

### build-llms.ts {#build-llmsts}

`bun tools/build-llms.ts` ei ota argumentteja. Se kirjoittaa tiedostot `docs/llms.txt` (hakemisto
kaikista sivuista) ja `docs/llms-full.txt` (kaikki englanninkieliset sivut Markdownina yhdessä
tiedostossa) sivujen etuaineistosta ja tekstistä sekä tiedostoista `docs/_data/faq_en.yml` ja
`faq_fi.yml`. Aja se, kun lisäät sivun tai muutat otsikkoa tai kuvausta, sekä `sync-roadmap.ts`:n
jälkeen, koska tiekarttasivu on yksi sen lähteistä.

Se varoittaa sivusta, jolla ei ole `description`-kenttää. Se pysähtyy virheeseen, kun se kohtaa
Liquidia, jota se ei ymmärrä. Sivuilla saa käyttää vain:

- Liquidin `raw`- ja `comment`-lohkoja,
- sivulinkkikaavaa {% raw %}`{% assign x_page = site.pages | where: "path", "..." | first %}`{% endraw %} yhdessä {% raw %}`{{ x_page.url | relative_url }}`{% endraw %}:n kanssa,
- {% raw %}`{{ '/path' | relative_url }}`{% endraw %},
- {% raw %}`{{ site.github.repository_url }}`{% endraw %},
- UKK-silmukkaa, joka käy läpi `site.data.faq_en`:n tai `site.data.faq_fi`:n.

Virhe on myös UKK-datan linkki sivulle, jota ei ole olemassa, sekä sivulinkki, jonka `assign` nimeää
sivun, jota ei ole olemassa.

### make-game-manifest.ts {#make-game-manifestts}

Rakentaa sisältöluettelon `legacy/UndauntedContent/data/dauntless-1.4.4.json` tarkistetusta pelizipistä.
Sisältöpalvelin jakaa vain siinä luetellut tiedostot, ja käynnistin kääntää saman tiedoston
sisäänsä ja torjuu kaikki muut tiedostot. Aja se vain, kun luettelon täytyy muuttua.

```text
bun tools/make-game-manifest.ts --zip <BaseGame144.zip> [--out <file>] [--compare-dir <game folder>]
                                 [--compare-all] [--zip-sha256 <hex>] [--skip-zip-hash]
```

| Argumentti | Oletus | Mitä se tekee |
|:-----------|:-------|:--------------|
| `--zip <file>` | **pakollinen** | Tarkistettu 1.4.4-zip. |
| `--out <file>` | `legacy/UndauntedContent/data/dauntless-1.4.4.json` | Minne luettelo kirjoitetaan. |
| `--compare-dir <folder>` | ei mitään | Tarkistaa lopuksi puretun asennuksen uutta luetteloa vasten: jokaisen tiedoston koon sekä exe-tiedoston ja muutaman muun tiedoston täydet tiivisteet. Vain lukee. |
| `--compare-all` | pois | `--compare-dir`:n kanssa: laskee tiivisteen jokaisesta tiedostosta. |
| `--zip-sha256 <hex>` | kiinnitetty zipin tiiviste | Tiiviste, joka zipillä on oltava. |
| `--skip-zip-hash` | pois | Ohittaa koko zipin tiivisteen laskemisen. Vain jos olet tarkistanut sen itse. |

Se tarkistaa jokaisen merkinnän CRC-32:n ja koon zipin omia tietoja vasten, että jokainen polku on
turvallinen (sama sääntö, jota sisältöpalvelin käyttää), ettei kaksi polkua eroa toisistaan vain
kirjainkoon osalta, että `Version.txt` nimeää 1.4.4-koontiversion ja että exe vastaa kiinnitettyä
tiivistettä. Se tulostaa tiedostojen määrän, tavujen kokonaismäärän ja luettelon oman SHA-256:n. Se
pyörii matalalla prioriteetilla, lukee zipin kahdesti (noin 21 Gt luettavaa), päättyy paluukoodilla
`1` missä tahansa virheessä. Se lukee zipin itse (`tools/zip.ts`, myös Zip64), joten mitään ei
tarvitse asentaa ensin. Kun olet tuottanut luettelon uudelleen, käännä käynnistin uudelleen.

## CI-työnkulut ja -työkalut {#ci-workflows-and-tools}

GitHub Actions ajaa kaksi työnkulkua. Käsin ne käynnistetään repositorion **Actions**-välilehdeltä, ja
työkalut ajetaan omassa checkoutissa. Mitä kukin CI:n työ tarkistaa ja miten käynnistimen julkaisut
toimivat, kerrotaan sivulla [Kehittäjän opas]({{ dev_page.url | relative_url }}#ci). Repositorion
muuttuja `LAUNCHER_AUTO_RELEASE`, joka pysäyttää automaattiset julkaisut, on sivulla
[Asetukset]({{ config_page.url | relative_url }}#ci-settings).

| Työnkulku | Milloin se ajetaan | Syötteet | Mitä se tekee |
|:----------|:-------------------|:---------|:--------------|
| `.github/workflows/ci.yml` (**CI**) | Jokaisella pushilla mihin tahansa haaraan, jokaisella pull requestilla ja Actions > **CI** > **Run workflow** | ei mitään | Kääntää ja testaa neljä palvelinohjelmaa, käynnistimen ja Windows-palvelinpaketin, tarkistaa dokumentaation ja repositorion sekä säilyttää käynnistimen julkaisutiedostot 7 päivää (ei forkeista tulevissa pull requesteissa). Kun `mixutin/dauntless-revived`-repositorion `dauntless-revived`-haaraan tehty push on läpäissyt kaikki tarkistukset, se julkaisee sen jälkeen käynnistimen version, jos versiolla ei ole vielä julkaisua, paitsi jos `LAUNCHER_AUTO_RELEASE` on `false`. |
| `.github/workflows/launcher-release.yml` (**Launcher release**) | Actions > **Launcher release** > **Run workflow** `dauntless-revived`-haaralle; `ci.yml` kutsuu sitä | Käsin ei mitään. `ci.yml` antaa `version`-arvon (sen on oltava sama kuin `legacy/UndauntedLauncher/package.json`-tiedoston versio kyseisessä commitissa) ja `artifact`-arvon (sen kääntämät julkaisutiedostot). | Julkaisee käynnistimen version julkaisuna `launcher-v<versio>`, luo sen tagin, tekee jokaiselle tiedostolle allekirjoitetun käännöksen alkuperätodistuksen (build provenance attestation) ja osoittaa `launcher-updates`-kanavan siihen. Käsin ajettuna se kääntää ja testaa commitin itse; jo julkaistulla versiolla se vain päivittää kanavan siihen. Se kieltäytyy toimimasta muussa haarassa kuin `dauntless-revived`, joka on oletushaara. |

### tools/ci/check-repo.ts {#check-repots}

```text
bun tools/ci/check-repo.ts [--history <revision range>]
```

Repositorion siisteystarkistus. Ilman argumentteja se tarkistaa jokaisen versionhallinnassa olevan
tiedoston:

- ei salaisuus- eikä pelitiedostoa nimen perusteella: muita `.env`-tiedostoja kuin `.env.example`,
  avaimia ja varmenteita, tietokantoja, lokeja, pelin arkistoja ja resursseja, ohjelmatiedostoja,
  pakattuja arkistoja, muita DLL-tiedostoja kuin kaksi kiinnitettyä, eikä mitään kansioissa `data/` tai
  `BaseGame144/`;
- ei versionhallinnassa olevaa tiedostoa, jonka jokin `.gitignore`-sääntö sulkee pois;
- ei tiedostoa, joka on sama kuin jokin 1.4.4-pelin tiedosto (pelin tiedostoluettelon tiivisteiden
  perusteella);
- ei tiedostojen sisällä mitään salaisuuden näköistä: tiliavainta, JWT:tä, yksityistä avainta (myös
  base64-koodattuna), Tailscale-avainta eikä GitHub-tunnistetta;
- kansion `legacy/UndauntedLauncher/assets/` kaksi DLL-tiedostoa vastaavat kiinnityksiään käynnistimessä ja
  palvelinpaketissa, ja `legacy/UndauntedLauncher/package.json`-tiedoston käynnistinversio on kelvollinen.

| Argumentti | Mitä se tekee |
|:-----------|:--------------|
| `--history <revision range>` | Tarkistaa lisäksi jokaisen tiedostoversion, jonka välin commitit lisäsivät tai muuttivat, esimerkiksi `origin/dauntless-revived..HEAD` ennen pushia. `--history HEAD` tarkistaa koko historian. Commitoitu ja taas poistettu salaisuus on yhä historiassa, joten avain on vaihdettava. |

Se ei koskaan tulosta osuman sisältöä, vain tiedoston, rivin ja osuman lajin. Paluukoodi `0` tarkoittaa,
ettei mitään löytynyt, `1` ongelmaa ja `2` käyttövirhettä. Sillä ei ole riippuvuuksia, joten mitään ei
tarvitse asentaa ensin.

### tools/ci/launcher-version.ts {#launcher-versionts}

Käynnistimen versiosäännöt, jotka `check-repo.ts` ja julkaisutyönkulku jakavat. Käynnistimen versio on
semanttinen versio ilman build-metatietoja (`x.y.z` tai `x.y.z-prerelease`), ja se nimeää
julkaisutagin `launcher-v<versio>`.

| Komento | Mitä se tekee |
|:--------|:--------------|
| `bun tools/ci/launcher-version.ts valid <version>` | Paluukoodi `0`, jos se on käynnistimen versio, muuten `1`. |
| `bun tools/ci/launcher-version.ts compare <a> <b>` | Tulostaa `-1`, `0` tai `1` (semanttisten versioiden järjestys). |
| `bun tools/ci/launcher-version.ts newest <version> [<other>...]` | Paluukoodi `0`, jos `<version>` on uudempi kuin jokainen muu huomioon otettava: julkaisua verrataan vain muihin julkaisuihin, esiversiota kaikkiin. Muuten se tulostaa uudemman tai yhtä uuden version ja päättyy paluukoodilla `1`. Muut, jotka eivät ole käynnistimen versioita, ohitetaan varoituksen kera. |
| `bun tools/ci/launcher-version.ts nuget <version>` | Tulostaa version siinä muodossa, jossa se on Squirrelin pakettien nimissä. |
| `bun tools/ci/launcher-version.ts from-nupkg <file name>` | Tulostaa `*-full.nupkg`-tiedostonimessä olevan version, tai päättyy paluukoodilla `1`, jos siinä ei ole versiota. |

Käyttövirhe tai `<version>`, joka ei ole käynnistimen versio siellä, missä sellainen vaaditaan,
päättyy paluukoodilla `2`.

### collect-release.ps1 {#collect-releaseps1}

Kansiossa `legacy/UndauntedLauncher`, komennon `npm run make` jälkeen:

```powershell
pwsh -NoProfile -File scripts\collect-release.ps1
```

Sillä ei ole parametreja. Se tyhjentää kansion `release\` ja kopioi sinne sen, minkä käynnistimen
julkaisu julkaisee: `DauntlessRevivedLauncher-Setup.exe`, Squirrelin päivitystiedostot (`RELEASES` ja
täysi `.nupkg`) sekä siirrettävän zip-version nimellä `DauntlessRevivedLauncher-<versio>-win32-x64.zip`.
Sitten se kirjoittaa niistä kaikista `SHA256SUMS.txt`-tiedoston ja tulostaa sen. Se pysähtyy, jos jokin
tiedostoista puuttuu. CI ja julkaisutyönkulku ajavat sen; git ohittaa kansion `release\`.

## npm-skriptit {#npm-scripts}

Aja kukin npm-paketin kansiossa `npm ci`:n jälkeen. Skriptin omat argumentit tulevat `--`:n jälkeen,
esimerkiksi `npm run verify -- --quick`. Mitkä testit käyttävät mitäkin portteja ja mitä
testisarjoja ei saa ajaa yhtä aikaa, kerrotaan sivulla [Kehittäjän opas]({{ dev_page.url | relative_url }}).

**UndauntedMetagame**

| Skripti | Ajaa | Mitä se tekee |
|:--------|:-----|:--------------|
| `npm run dev` | `tsx watch --env-file=.env src/server.ts` | Ajaa lähdekoodista ja käynnistyy uudelleen muutoksista. Epäonnistuu, jos kansiossa ei ole `.env`-tiedostoa. |
| `npm run build` | `tsc`, sitten `postbuild` | Kääntää kansioon `dist\`. `postbuild` ajaa `scripts/write-build-info.js`:n, joka kirjoittaa tiedostoon `dist\build-info.json` commitin, npm-paketin version ja käännösajan. Commit tulee gitistä, päätteellä `-dirty`, kun kansiossa `legacy/UndauntedMetagame` on muutoksia, tai se on `unknown` ilman gitiä (kuten palvelinpaketin käännöksissä). `.env`-tiedoston `GIT_COMMIT` ja `SERVER_VERSION` ohittavat sen ajon aikana. |
| `npm start` | `node --env-file=.env dist/server.js` | Ajaa käännetyn version. Epäonnistuu ilman `.env`-tiedostoa. Käynnistä se npm-paketin kansiosta: tietokannan migraatiot etsitään työhakemiston suhteen. |
| `npm run db:generate` | `drizzle-kit generate` | Kirjoittaa uuden migraation kansioon `src/drizzle`, kun tiedostoa `src/db/schema.ts` on muutettu. Lukee `DB_FILENAME`:n `.env`-tiedostosta. |
| `npm test` | `tsc -p tsconfig.test.json`, sitten `node --test` | Poistaa kansion `build\`, kääntää testit ja ajaa ne. |

**UndauntedDeployServer**

| Skripti | Ajaa | Mitä se tekee |
|:--------|:-----|:--------------|
| `npm run dev` | `tsx --env-file=.env src/server.ts` | Ajaa lähdekoodista (ei seuraa muutoksia). Epäonnistuu ilman `.env`-tiedostoa. |
| `npm run build` | `tsc` | Kääntää kansioon `dist\`. |
| `npm start` | `node --env-file=.env dist/server.js` | Ajaa käännetyn version. Epäonnistuu ilman `.env`-tiedostoa. |
| `npm test` | kuten metagamessa | Kääntää ja ajaa testit. |

**UndauntedGateway**

| Skripti | Ajaa | Mitä se tekee |
|:--------|:-----|:--------------|
| `npm run build` | `tsc` | Kääntää yhdyskäytävän ja sallittujen listan apurin kansioon `dist\`. |
| `npm start` | `node --env-file-if-exists=.env dist/server.js` | Ajaa yhdyskäytävän. Puuttuva `.env` ei ole virhe. |
| `npm run start:allowlist` | `node --env-file-if-exists=.env dist/allowlist/server.js` | Ajaa sallittujen listan apurin. Se muuttaa palomuurisääntöä, joten se on ajettava ylläpitäjänä, ellei se ole kuivaharjoitustilassa. Kun skriptin perään lisätään `--close-ports` (`node --env-file=.env dist/allowlist/server.js --close-ports`), apuri vain kytkee peliporttien säännön kerran pois päältä ja lopettaa (paluukoodi 0 tai 1). Se on tarkoitettu pysäytysskripteille. Apuri lukee samat asetukset, joten `ALLOWLIST_DRY_RUN` ja `ALLOWLIST_SECRET` on oltava asetettuina. `Stack.ps1` ei käytä tätä, vaan kytkee säännön pois itse. |
| `npm run make-cert -- <argumentit>` | `node tools/make-cert.js` | Tekee yhdyskäytävän itse allekirjoitetun varmenteen. Katso alta. |
| `npm test` | `tsc -p tsconfig.test.json`, sitten `node --test --test-concurrency=1` | Poistaa kansion `build\`, kääntää testit ja ajaa ne tiedosto kerrallaan. |

`tools/make-cert.js` tekee RSA-2048-avaimella ja SHA-256:lla allekirjoitetun varmenteen, joka on
voimassa 10 vuotta ja jonka nimiksi tulevat annetut osoitteet, ja tulostaa sen SHA-256-sormenjäljen.
Se on jokaisen v2-kutsun `fp`:

| Argumentti | Mitä se tekee |
|:-----------|:--------------|
| `--host <IP-osoite tai DNS-nimi>` | Nimi varmenteelle. Toista argumentti, jos nimiä on useampi. Vähintään yksi vaaditaan. |
| `--out <kansio>` | Kirjoittaa tiedostot `gateway-cert.pem` ja `gateway-key.pem` siihen kansioon. |
| `--cert <tiedosto> --key <tiedosto>` | Kirjoittaa näihin kahteen tiedostoon `--out`:n sijaan. |
| `--force` | Korvaa olemassa olevat tiedostot. Ilman sitä työkalu kieltäytyy, koska uusi varmenne rikkoo jokaisen jo lähetetyn kutsun. |
| `--json` | Tulostaa tuloksen yhtenä JSON-rivinä. |
| `--fingerprint <cert.pem>` | Tulostaa vain olemassa olevan varmenteen nimet, voimassaolon ja sormenjäljen. |

**`gateway-key.pem` on salainen: älä koskaan jaa, älä koskaan committoi.** Pidä se kansiossa, jota
vain Administrators ja yhdyskäytävää ajava tili voivat lukea. Windows-palvelinpaketti ajaa tämän
työkalun itse; tarvitset sitä vain käsin pystyttämääsi yhdyskäytävään. Paluukoodi `2` tarkoittaa
käyttövirhettä ja `1` mitä tahansa muuta virhettä.

**UndauntedContent**

| Skripti | Ajaa | Mitä se tekee |
|:--------|:-----|:--------------|
| `npm run build` | `tsc` | Kääntää kansioon `dist\`. |
| `npm start` | `node --env-file-if-exists=.env dist/server.js` | Ajaa sisältöpalvelimen. |
| `npm run verify -- [--quick] [--game-dir <kansio>] [--manifest <tiedosto>]` | `node --env-file-if-exists=.env dist/verify.js` | Laskee jokaisen pelitiedoston tiivisteen ja vertaa sitä luetteloon, ja luettelee, mitä puuttuu, mikä on väärän kokoinen ja minkä tiiviste on väärä. `--quick` vertaa vain kokoja. Oletukset ovat `.env`-tiedoston `CONTENT_GAME_DIR` ja `CONTENT_MANIFEST`, muuten sisäänrakennettu luettelo. Vain lukee, matalalla prioriteetilla. Paluukoodi `0`: kaikki täsmäävät, `1`: ongelma, `2`: käyttövirhe. Aja se, kun olet kopioinut pelin uudelle palvelinkoneelle. |
| `npm test` | `tsc -p tsconfig.test.json`, sitten `node --test` | Poistaa kansion `build\`, kääntää yksikkötestit ja ajaa ne. |
| `npm run test:integration` | `tsc`, `tsc -p tsconfig.test.json`, sitten `node --test --test-concurrency=1` | Kääntää palvelimen ja ajaa sitä oikeaa asennusta vasten (`CONTENT_IT_GAME_DIR`, oletus `C:\D144\Dauntless`; ohitetaan, jos siellä ei ole 1.4.4-asennusta) metagamea jäljittelevän testipalvelimen kanssa porteissa 62002 ja 62003 (`CONTENT_IT_PORT`, `CONTENT_IT_MOCK_PORT`). |

**UndauntedLauncher**

| Skripti | Ajaa | Mitä se tekee |
|:--------|:-----|:--------------|
| `npm start` | `electron-forge start` | Ajaa käynnistintä kehitystilassa. |
| `npm run package` | `electron-forge package --platform win32` | Paketoi sovelluksen ilman asennusohjelmaa. |
| `npm run make` | `electron-forge make --platform win32` | Rakentaa `DauntlessRevivedLauncher-Setup.exe`:n (Squirrel) ja zipin. |
| `npm run typecheck` | `tsc --noEmit -p tsconfig.json` | Tarkistaa tyypit kääntämättä. |
| `npm test [-- <suodatin>]` | `node scripts/run-tests.mjs` | Kääntää testit kansioon `.test-build\` ja ajaa ne tiedosto kerrallaan. Suodattimen kanssa vain ne testitiedostot, joiden nimessä suodatin esiintyy. |
| `npm run icon [-- --check]` | `node scripts/make-icon.mjs` | Kopioi käynnistimen kuvakkeet (`assets/icon.ico`, `assets/icon.png`) ja sen ikkunassa näkyvät logo- ja tunnuskuvat (`src/renderer/brand/`) kansiosta `brand/`, jossa `python3 brand/build.py` tekee ne. Valitsimella `--check` se vain vertaa ja päättyy koodiin 1, jos jokin kopio puuttuu tai on vanhentunut; yksikkötestit ajavat sen. |

Käynnistin tarvitsee Node.js:n version 20.19 tai uudemman (`engines` sen `package.json`-tiedostossa).
Palvelinpaketti asentaa palvelimelle Node.js:n version 24.19.0.

### Muut npm-pakettien skriptit {#other-package-scripts}

| Skripti | Miten ajetaan | Mitä se tekee |
|:--------|:--------------|:--------------|
| `legacy/UndauntedMetagame/scripts/write-build-info.js [kohdekansio]` | `postbuild`-vaiheena; testit antavat toisen kansion | Kirjoittaa `build-info.json`-tiedoston (oletuskansio: `dist`). |
| `legacy/UndauntedMetagame/scripts/make-hunt-titles.js` | `node scripts/make-hunt-titles.js` kansiossa `legacy/UndauntedMetagame` | Rakentaa uudelleen tiedoston `src/vendor/hunt_titles.json`, eli metsästysten nimet, jotka `ServerStatus` näyttää, deploy-palvelimen metsästystaulukoista kansiossa `legacy/UndauntedDeployServer/src/vendor`. Aja se uudelleen, kun ne taulukot muuttuvat. |
| `legacy/UndauntedGateway/tools/make-cert.js` | `npm run make-cert -- ...` | Katso yllä. |
| `legacy/UndauntedLauncher/scripts/run-tests.mjs`, `make-icon.mjs` | `npm test`, `npm run icon` | Katso yllä. |
| `legacy/UndauntedLauncher/scripts/collect-release.ps1` | `pwsh -NoProfile -File scripts\collect-release.ps1` kansiossa `legacy/UndauntedLauncher`, komennon `npm run make` jälkeen | Kokoaa julkaisutiedostot kansioon `release\`; katso [collect-release.ps1](#collect-releaseps1). |

## Ohjeissa olevat skriptit {#scripts-printed-in-the-guides}

Jotkin ohjeet näyttävät lyhyen skriptin, jonka tallennat ja ajat itse. Ne eivät ole repositorion
tiedostoja, ja niiden parametrit kuvataan siellä, missä skripti on:

| Skripti | Missä | Parametrit |
|:--------|:------|:-----------|
| `verify-manifest.js` | [Pystytä palvelin, vaihe 2]({{ host_page.url | relative_url }}#verify-the-build) | `node verify-manifest.js <pelikansio>` (oletus: nykyinen kansio) |
| `make-gameini.ps1` | [Pystytä palvelin, vaihe 8]({{ host_page.url | relative_url }}#game-ini) | `-Metagame` (oletus `127.0.0.1:61000`), `-Source` (DLL:n `dllmain.cpp`), `-Out` (käyttäjän `Game.ini`) |
| `play.ps1` (palvelinkoneen oma) | [Pystytä palvelin, vaihe 13]({{ host_page.url | relative_url }}#launch-the-client) | `-Backend` (oletus `127.0.0.1:61000`), `-Graphics` (oletus `4`; `-1` = käytä valikkoa), `-Windowed`, `-CapMB` (oletus `12000`), `-Seconds` (oletus `0`), `-KeyFile` (tiliavain, jolla kirjaudutaan; oletus `C:\dr\data\owner.key`; **salainen: älä koskaan jaa, älä koskaan committoi**) |
| `play.ps1` (kaverin käsin tehtävä versio) | [Liity kaverina]({{ friends_page.url | relative_url }}) | `-Server` (pakollinen), `-Game` (oletus `C:\D144\Dauntless`), `-Windowed` |
| `rekey.js` | [Palvelin ryhmälle]({{ admin_page.url | relative_url }}#missing-admin-functions-and-workarounds) | `node rekey.js <UserId>`. **Sen tulostiedostossa on voimassa oleva tiliavain: anna se pelaajalle yksityisesti ja poista tiedosto sitten.** |
| `backup-db.js` | [Varmuuskopioi tietokanta]({{ admin_page.url | relative_url }}#back-up-the-database) | ei mitään |
| `seed-progression.js` | [Lähtötason antaminen pelaajalle]({{ upgrade_page.url | relative_url }}#seeding-a-player) | `<ylläpitäjän avaintiedosto> list`, tai `<ylläpitäjän avaintiedosto> <UserId> grandfather` tai `fresh`; muuttuja `METAGAME` asettaa metagamen osoitteen (oletus `http://127.0.0.1:61000`) |

Ohjeissa mainitaan myös projektin oman palvelinkoneen `stack.ps1` ja `backup.ps1` kansiossa
`C:\dr\tools`. Niitä ei ole julkaistu. Palvelinpaketilla asennetulla palvelimella `Stack.ps1` ja
`Backup-DauntlessServer.ps1` tekevät samat työt, mutta ne toimivat vain palvelimella, joka on
asennettu paketilla.
