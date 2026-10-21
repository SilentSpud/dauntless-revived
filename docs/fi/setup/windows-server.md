---
title: Windows-palvelin
parent: Asennus
grand_parent: Dauntless Revived suomeksi
nav_order: 6
description: "Dauntless Revived -palvelin vuokratulle Windows Server 2019 -koneelle julkisessa tilassa: yksi salattu portti, kiinnitetty varmenne, peliportit auki vain kirjautuneille, asennus SSH:lla, kutsut ja varmuuskopiot."
lang: fi
ref: setup/windows-server
locale: fi_FI
---

{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}
{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}
{% assign legal_page = site.pages | where: "path", "fi/legal.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign scripts_page = site.pages | where: "path", "fi/reference/scripts.md" | first %}
{% assign chat_page = site.pages | where: "path", "fi/findings/chat.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}

# Windows-palvelin
{: .no_toc }

Repositorion kansio `deploy/windows-server/` asentaa kokonaisen Dauntless Revived -palvelimen
Windows Server 2019 -koneelle, yleensä vuokrattuun virtuaalipalvelimeen, jolla on julkinen
IP-osoite. Ajat omalla koneellasi yhden komennon. Se ottaa palvelimeen yhteyden SSH:lla, lähettää
kaiken tarvittavan, asentaa palvelimen ja kertoo lopuksi, mitä kaverit tarvitsevat liittyäkseen.

Tiloja on kaksi:

- **Julkinen tila** (oletus): kaverit liittyvät internetin yli pelkällä kaverikäynnistimellä ja
  kutsulla. Vain yksi salattu portti on auki maailmalle, kaikki muu pysyy palvelimen sisällä.
- **Yksityinen tila**: kaverit liittyvät Tailscalen kautta, kuten sivulla
  [Palvelin ryhmälle]({{ admin_page.url | relative_url }}) kerrotaan.

**Tilanne (22.9.2026).** Paketti on rakennettu ja testattu kehityskoneella sen hiekkalaatikkotilassa:
koko julkisen tilan asennus väliaikaiseen kansioon, ja yhdyskäytävä, kiinnitetty varmenne, kutsut ja
varmuuskopiosta palauttaminen tarkistettu alusta loppuun. 21.–22.9.2026 se asennettiin julkiseen
tilaan oikealle vuokratulle Windows Server 2019 -virtuaalipalvelimelle. Sillä palvelimella on
tarkistettu: palvelinkokonaisuus käynnistyy koneen käynnistyessä palvelutilillä istunnossa 0,
Ramsgate pyörii ja lähettää elonmerkkejä (heartbeat), yhdyskäytävä vastaa internetistä kiinnitetyllä
varmenteella, ja tunnin välein ajettava varmuuskopiotehtävä toimii. 22.9.2026 omistaja pelasi siellä
internetin yli: hän rekisteröityi käynnistimellä kutsun avulla, latasi pelin yhdyskäytävän kautta ja
pelasi opetusjakson, Ramsgaten, Training Dojon ja ensimmäisen metsästyksen. Kolme pelipalvelinta pyöri
yhtä aikaa, ja sallittujen lista avasi UDP-peliportit pelaajalle ja sulki ne, kun hän lähti. Oikea
palvelin paljasti kolme ongelmaa, joita hiekkalaatikko ei voinut löytää, ja ne kaikki on korjattu
paketissa: Windows sallii paikallisen tilin
kuvaukseen enintään 48 merkkiä, se levykuva ei hyväksy ilman tallennettua salasanaa ajettavia
ajastettuja tehtäviä (”S4U”) muille kuin ylläpitäjille, ja palveluntarjoajan levykuva piti palomuurin
pois päältä käytäntöarvoilla (kaksi viimeistä selitetään alla asennuksen vaiheissa). Testi toisen
pelaajan kanssa on seuraavana vuorossa; yksityistä tilaa ei ole ajettu oikealla palvelimella. Katso
[tiekartan]({{ roadmap_page.url | relative_url }}) kohdat 1.15–1.17.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Näin julkinen tila toimii {#how-public-mode-works}

Pelin versio 1.4.4 puhuu salaamatonta HTTP:tä, eikä sitä saa muutettua. Siksi jokaisen kaverin
käynnistin pitää hänen omalla koneellaan pientä välitintä, ja vain välitin puhuu internetiin:

| Vaihe | Missä | Mitä tapahtuu |
|:------|:------|:--------------|
| 1 | Kaverin kone | Peli puhuu HTTP:tä osoitteeseen `127.0.0.1:61000`, eli käynnistimen välittimelle. |
| 2 | Internet | Välitin lähettää kaiken **salattuna (TLS)** palvelimen yhdyskäytävälle (oletuksena portti 443). Se hyväksyy vain sen yhden varmenteen, jonka sormenjälki tuli kutsun mukana. Kukaan välissä ei voi lukea tai muuttaa liikennettä, eikä verkkotunnusta tai varmenteen myöntäjää tarvita. |
| 3 | Palvelin | **Yhdyskäytävä** on ainoa julkinen TCP-portti. Se välittää pyynnöt metagamelle ja sisältöpalvelimelle, jotka kuuntelevat vain osoitteessa `127.0.0.1`. `/undaunted/api`-reiteistä se päästää läpi vain ne neljä, joita käynnistin tarvitsee (rekisteröinti, avaimen tarkistus, palvelimen tila, rekisteröintitila), joten jokainen ylläpitoreitti jää palvelimen sisälle. Se torjuu kaiken, missä on pelipalvelimen avain, rajoittaa pyyntöjen koon ja määrän, eikä kirjaa lokiin avaimia eikä tunnisteita. |
| 4 | Palvelin | Kun pelaaja kirjautuu (tai hänen pelinsä lähettää elonmerkin), yhdyskäytävä kertoo pelaajan osoitteen **sallittujen listan apurille**. Apuri pitää yllä yhtä palomuurisääntöä, joka avaa UDP-portit 8770-8777 juuri näille osoitteille. Osoite putoaa pois 10 minuuttia viimeisen elonmerkin jälkeen, ja kaikki muut palomuuri pudottaa. |
| 5 | Kaverin kone | Pelin UDP-liikenne kulkee suoraan palvelimen julkiseen osoitteeseen tämän säännön läpi. |

Metagame antaa jokaiselle pelaajalle QoS-osoitteen `http://127.0.0.1:61000/QoS`, eli kaverin
oman välittimen. Siksi välittimen portti on aina 61000.

### Mikä näkyy internetiin

| Portti | Kenelle auki | Huomioita |
|:-------|:-------------|:----------|
| TCP 443 (`-GatewayPort`) | Kaikille | Yhdyskäytävä, vain TLS, vain `gateway.exe`. |
| UDP 8770-8777 | Vain kirjautuneiden pelaajien osoitteille | Sallittujen listan sääntö; kiinni kun kukaan ei pelaa ja aina kun palvelin on pysäytetty. Vain pelin ohjelma voi vastaanottaa. |
| TCP 22 | Kaikille, **vain avaimella** | OpenSSH omaa ylläpitoasi varten. SSH:n yli tehty asennus laittaa salasanakirjautumisen pois (avainkirjautuminen on jo todistettu); muuten asennus varoittaa. Rajaa tämä omaan osoitteeseesi palveluntarjoajalla. |
| TCP 3389 | Vain `-AdminIp` | Etätyöpöytä, jos käytät sitä. Ilman `-AdminIp`:tä asennus **laittaa pois** koko internetille auki olevan etätyöpöytäsäännön (käytä avainpohjaista SSH:ta); `-KeepRdpOpen` jättää sen auki. |
| Kaikki muu | Ei kenellekään | Jokainen palomuuriprofiili estää saapuvat yhteydet oletuksena. |

Näihin ei pääse ulkoa koskaan: metagame (61000), sisältöpalvelin (61002), deploy-palvelin (61001,
jossa ei ole lainkaan tunnistusta) ja sallittujen listan apuri (61005).

## Mitä tarvitset

- **Windows Server 2019 -virtuaalipalvelin** (myös 2022 käy) **työpöytäkokemuksella** (Desktop
  Experience). Server Core ei pysty ajamaan pelipalvelimia, koska ne ovat pelin omaa ohjelmaa ja
  tarvitsevat työpöydän DLL-tiedostot. Vähintään **8 Gt muistia**, 4 prosessoriydintä ja
  **40 Gt levytilaa**, sekä **julkinen IPv4-osoite**. Ramsgate vie noin 1,1 Gt muistia ja jokainen
  metsästys noin 1 Gt.
- Palveluntarjoajan omassa palomuurissa (moni estää saapuvan oletuksena): salli **TCP 443 ja
  UDP 8770-8777 mistä tahansa osoitteesta**, ja TCP 22 (sekä etätyöpöytä 3389, jos käytät sitä) vain
  omasta osoitteestasi. Katso "Ennen kuin kaverit liittyvät" alta; ilman UDP-sääntöä kaverit jäävät
  jumiin Ramsgatea ladatessa.
- **Tarkistettu Dauntless 1.4.4 -zip** (SHA-256 `556B9A64...BC6D`) omalla koneellasi, tai
  https-linkki, josta palvelin voi ladata sen.
- **Oma koneesi:** repositorion kopio, Windows 10 tai 11 sisäänrakennetulla OpenSSH-asiakkaalla, ja
  SSH-avainpari (esimerkiksi `ssh-keygen -t ed25519 -f C:\dr\data\ssh\dauntless_deploy`). Pidä
  yksityinen avain vain oman käyttäjätilisi luettavana.
- Halutessasi: varmuuskopio nykyisestä palvelimestasi (kansio, jonka `backup.ps1` tai
  `Backup-DauntlessServer.ps1` tekee). Sen avulla kaikki tilit, tallennukset ja avaimet siirtyvät
  uudelle palvelimelle.

## Palvelimen valmistelu (kerran)

Tee tämä kerran etätyöpöydän tai palveluntarjoajan selainkonsolin kautta Administrator-tunnuksella.
Näin SSH aukeaa vain sinun avaimellesi, eikä salasanaa tarvita tämän jälkeen enää.

```powershell
# 1. Asenna ja käynnistä OpenSSH-palvelin
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Set-Service sshd -StartupType Automatic
Start-Service sshd

# 2. Salli julkinen avaimesi (liitä YKSI rivi tiedostosta dauntless_deploy.pub) ylläpitäjille
$k = 'C:\ProgramData\ssh\administrators_authorized_keys'
Set-Content -Path $k -Value 'ssh-ed25519 AAAA... avaimen-kommentti' -Encoding ascii
icacls $k /inheritance:r /grant '*S-1-5-32-544:F' /grant '*S-1-5-18:F'   # Administrators, SYSTEM

# 3. Tulosta palvelimen SSH-isäntäavaimen sormenjälki tarkistettavaksi ensimmäisellä yhteydellä
Get-ChildItem C:\ProgramData\ssh\ssh_host_ed25519_key.pub | ForEach-Object { ssh-keygen -lf $_.FullName }
```

Merkitse muistiin `SHA256:...`-sormenjälki, jonka vaihe 3 tulostaa (selainkonsolissa, jossa mikään ei
ole välissä). Kokeile sitten omalta koneeltasi niin, että isäntäavain tallentuu samaan `known_hosts`-
tiedostoon, jota paketti käyttää, ja vertaa näytettyä sormenjälkeä vaiheen 3 arvoon:

```powershell
ssh -o UserKnownHostsFile=C:\dr\data\ssh\known_hosts -i C:\dr\data\ssh\dauntless_deploy Administrator@<palvelimen osoite> hostname
```

Jos ne täsmäävät, vastaa `yes`. `Deploy-Remote.ps1 -HostKeyFingerprint SHA256:...` tekee saman
tarkistuksen puolestasi eikä lähetä varmuuskopiota tarkistamattomalle palvelimelle. Kun avainkirjautuminen
toimii, laita salasanakirjautuminen pois tiedostossa `C:\ProgramData\ssh\sshd_config`
(`PasswordAuthentication no`, sitten `Restart-Service sshd`). Julkisen tilan asennus, joka ajetaan
SSH:n yli, kuten `Deploy-Remote.ps1` sen ajaa, tekee tämän muutoksen itse, koska avainkirjautuminen on
silloin jo todistettu. Älä koskaan lähetä palvelimen salasanaa kenellekään, äläkä kirjoita sitä
mihinkään skriptiin.

Salli palveluntarjoajalla **TCP 22 vain omasta osoitteestasi**. Jos kotisi IP-osoite muuttuu, päivitä
palveluntarjoajan sääntö ja aja asennus uudelleen uudella `-AdminIp`-arvolla (SSH:n kautta).

## Asennus omalta koneelta {#deploy-from-your-pc}

**Ennen ensimmäistä asennusta.** Julkinen tila lähettää `git archive`n nykyisestä commitistasi
(`HEAD`). Jos yhdyskäytävää ja sisältöpalvelinta ei ole vielä committoitu, arkistossa ei ole niitä ja
asennus epäonnistuu palvelimella, mutta vasta kun ~10,5 Gt:n pelizip on jo lähetetty. Joko **committoi
ja pushaa työsi ensin**, tai anna **`-WorkingTree`**, joka lähettää nykyiset tiedostosi (ei koskaan
avaimia tai `.env`-tiedostoja). Alla oleva esimerkki käyttää `-WorkingTree`:tä; poista se, kun työsi on
committoitu. `Deploy-Remote.ps1` tarkistaa tämän nyt ja pysähtyy ennen lähetystä, jos `HEAD`:sta puuttuu
yhdyskäytävä.

10,5 Gt:n pelizip vie tunteja kotiyhteydellä (noin 2,5 t nopeudella 10 Mbit/s). `-GameZipUrl
https://...` antaa palvelimen ladata sen itse, ja `-UploadOnly` lähettää edellisenä päivänä ajamatta
asennusta.

Repositorion kansiossa omalla koneellasi:

```powershell
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 `
    -WorkingTree `
    -HostKeyFingerprint SHA256:... `
    -GameZip D:\BaseGame144.zip `
    -RestoreFrom C:\dr\backups\2026-10-01_200000 `
    -AdminIp 198.51.100.20 `
    -ServerName "Lauantain Ramsgate" `
    -ServerBinaries C:\dr\server-x86_64-pc-windows-msvc
```

`-ServerBinaries` on kansio, jossa ovat palvelimen ohjelmat tälle koodille: commitin CI-ajon
`server-x86_64-pc-windows-msvc`-lataus (Actions, ajo, Artifacts). Sen voi jättää pois, kun
työkopiosi `rust\target\release` sisältää käännöksen (`cargo build --release`). Ohjelmat
tarkistetaan palvelimella niiden `SHA256SUMS`-tiedostoa vasten.

Uudelle palvelimelle, jolla ei ole vielä tietokantaa, käytä `-RestoreFrom`:n sijaan
`-OwnerName <käyttäjänimesi>`. Näin komento etenee:

1. Se ottaa yhteyden avaimella `C:\dr\data\ssh\dauntless_deploy` (vaihda `-KeyFile`:llä), eikä kysy
   mitään salasanaa. Palvelimen oma avain tallennetaan ensimmäisellä kerralla tiedostoon
   `known_hosts` avaintiedoston viereen, ja jokaisen myöhemmän yhteyden täytyy täsmätä siihen.
2. Se lähettää paketin ja palvelinkoodin (`git archive` nykyisestä commitistasi; `-WorkingTree`
   lähettää myös keskeneräiset muutokset, mutta ei koskaan ohitettuja tiedostoja, avaimia tai
   `.env`-tiedostoja).
3. `-RestoreFrom`:n kanssa se lähettää varmuuskopion kansioon, jonka vain Administrators ja SYSTEM
   voivat avata, ja poistaa kopion onnistuneen asennuksen jälkeen.
4. Se lähettää pelin zip-tiedoston 256 megatavun osissa. Zip tarkistetaan ensin omalla koneellasi,
   jokainen osa tarkistetaan palvelimella perille tultuaan, ja koko tiedosto vielä kerran koottuna.
   **Jos lähetys katkeaa, aja sama komento uudestaan**: se jatkaa puuttuvista osista.
   `-GameZipUrl https://...` antaa palvelimen ladata zipin itse.
5. Se ajaa asennusohjelman palvelimella ja näyttää sen tulosteen sitä mukaa.
6. Se kertoo palvelimen osoitteen ja varmenteen sormenjäljen, ja tarkistaa omalta koneeltasi, että
   yhdyskäytävä vastaa juuri tällä varmenteella.

`-WhatIf` näyttää suunnitelman ottamatta yhteyttä. `-UploadOnly` pysähtyy ennen asennusta.

### Mitä asennus tekee {#what-the-installer-does}

`Install-DauntlessServer.ps1`:n voi ajaa myös suoraan palvelimella (esimerkiksi etätyöpöydän
kautta). Jokaisen vaiheen voi toistaa turvallisesti, ja `-WhatIf` luettelee kaikki muutokset
tekemättä niitä.

1. **Esitarkistus:** ylläpitäjän oikeudet, Windows Server 2019 (koontiversio 17763) tai uudempi,
   työpöytäkokemus, 8 Gt muistia, 40 Gt levytilaa, vapaat portit.
2. **Esivaatimukset:** Visual C++ 2015-2022 x64 -ajonaikainen kirjasto ja DirectX June 2010
   (molemmat tarkistetaan Microsoftin allekirjoituksesta). Kaikki lataukset käyttävät TLS 1.2:ta.
   Node.js:ää ei tarvita.
3. **Palvelinkoodi:** kopioidaan kansioon `C:\DauntlessRevived\app`, ja palvelimen ohjelmat
   kansioon `C:\DauntlessRevived\app\bin`: `metagame.exe`, `deploy.exe`, `content.exe`,
   `gateway.exe` ja yhdyskäytävän `allowlist.exe` (`-ServerBinaries`-kansiosta, tarkistettuina
   niiden `SHA256SUMS`-tiedostoa vasten).
4. **Pelitiedostot:** zipin SHA-256 tarkistetaan, se puretaan Windowsin omalla `tar.exe`:llä,
   jokainen tiedosto tarkistetaan sisältöluetteloa vasten, ja kaksi palvelimen DLL-tiedostoa
   asennetaan kiinnitetyillä tiivisteillä.
5. **Palvelutili:** paikallinen käyttäjä `dauntless` satunnaisella salasanalla, jota ei koskaan
   näytetä. Palvelin pyörii tällä tilillä, ei koskaan ylläpitäjänä. Tehtävien ajoitus (Task Scheduler)
   säilyttää salasanan salattuna tilin ajastettuja tehtäviä varten, ja asennusohjelma vaihtaa sen joka
   ajokerralla: jotkin Server 2019 -levykuvat eivät salli ilman tallennettua salasanaa ajettavia
   tehtäviä ("S4U") muille kuin ylläpitäjille. Pelipalvelimet pyörivät tällä tilillä ja lukevat sen
   pelin asetukset, joten asennusohjelma kirjoittaa myös tilin `Game.ini`-tiedoston (167
   osoiteohitusta, jotka osoittavat tämän palvelimen metagameen) ja `Engine.ini`-tiedoston
   (muistirivit ja chat osoitteeseen `127.0.0.1`). Katso
   [Pelin asetukset]({{ gamesettings_page.url | relative_url }}#game-ini).
6. **Varmenne:** itse allekirjoitettu varmenne yhdyskäytävälle (10 vuotta, nimenä julkinen osoite),
   yhdyskäytävän omalla työkalulla. Windowsin varmennesäilöihin ei kosketa. Sormenjälki tulostetaan,
   ja se kulkee jokaisessa kutsussa.
7. **Asetukset:** uudet tunnusten allekirjoitusavaimet, pelipalvelimen avain, yhdyskäytävän salaisuus
   ja apurin salaisuus (tai varmuuskopiosta tulevat), rekisteröityminen vain kutsukoodilla.
   Salaisuuksia sisältävät tiedostot ovat vain ylläpitäjien, SYSTEMin ja palvelutilin luettavissa;
   apurin asetukset vain ylläpitäjien ja SYSTEMin. Mitään salaista ei tulosteta koskaan.
8. **Tietokanta ja omistajan tili:** omistajan (ylläpitäjän) tili luodaan ja sen avain tallennetaan
   tiedostoon `data\keys\owner.key`; tai tietokanta ja avaimet tulevat varmuuskopiosta.
9. **Palomuuri** yllä olevan taulukon mukaan. Asennusohjelma tarkistaa palomuurin, joka on oikeasti
   voimassa, eikä vain sen tavallisia asetuksia: jotkin VPS-levykuvat pitävät Windowsin palomuurin
   pois päältä käytäntöarvolla (`EnableFirewall = 0` avaimen `HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall`
   alla), vaikka tavalliset asetukset näyttävät sen olevan päällä. Silloin kaikki portit ovat auki
   internetiin. Asennusohjelma poistaa tällaiset arvot. Windows ottaa muutoksen käyttöön vasta
   uudelleenkäynnistyksessä, joten asennusohjelma tulostaa silloin punaisella **RESTART THIS SERVER
   NOW** (käynnistä palvelin uudelleen nyt).
10. **Ajastetut tehtävät:** palvelin käynnistyksessä tilillä `dauntless` (valvottu: kaatunut osa
    käynnistetään uudelleen), apuri käynnistyksessä SYSTEM-tilillä (se muuttaa vain omaa
    palomuurisääntöään), ja varmuuskopio kerran tunnissa.

Alla ovat käytetyimmät parametrit. [Skriptit ja parametrit]({{ scripts_page.url | relative_url }})
luettelee ne kaikki sekä paketin kaikkien muiden skriptien parametrit.

| Parametri | Oletus | Merkitys |
|:----------|:-------|:---------|
| `-Mode` | uusi asennus: `Public`; uusi ajo: asennettu tila | `Public` (julkinen) tai `Private` (Tailscale). |
| `-GameZip` / `-GameZipUrl` | | Tarkistettu 1.4.4-zip, tai https-linkki siihen (lataus jatkuu katkoksen jälkeen). |
| `-PublicHost` | edellisen ajon tallentama osoite, muuten verkkokorttien ainoa julkinen IPv4-osoite | Osoite tai verkkotunnus, johon kaverit liittyvät. Tarvitaan, jos palvelin on 1:1-NATin takana. |
| `-GatewayPort` | edellisen ajon tallentama portti, muuten `443` | Yhdyskäytävän TCP-portti. |
| `-AdminIp` | edellisen ajon tallentama lista | Osoitteet, joista etätyöpöytä sallitaan, esimerkiksi `198.51.100.20` tai `198.51.100.0/24`. |
| `-KeepRdpOpen` | | Julkinen tila ilman `-AdminIp`:tä: jätä internetille auki olevat etätyöpöytäsäännöt auki poistamisen sijaan. |
| `-RestoreFrom` | | Varmuuskopiokansio, jolla olemassa oleva palvelin siirretään tänne. |
| `-OwnerName` | | Uuden palvelimen ylläpitäjätilin käyttäjänimi (3-16 kirjainta, numeroa tai `_`). |
| `-ServerName` | `Dauntless Revived` | Nimi, jonka kaverit näkevät käynnistimessä ja pelin tervetulotekstissä. Ei muisteta: uusi ajo ilman sitä palauttaa nimeksi `Dauntless Revived`. |
| `-InstallRoot` | `C:\DauntlessRevived` | Minne kaikki asennetaan. |
| `-InteractiveSession` | | Istunto 0:n varasuunnitelma, katso alempaa. Ei muisteta: uusi ajo ilman sitä laittaa automaattisen kirjautumisen pois. |
| `-NewCertificate` | | Uusi varmenne. **Kaikki aiemmin annetut kutsut lakkaavat toimimasta.** |

`Deploy-Remote.ps1` välittää jokaisessa asennusajossa `-Mode`:n (oletus `Public`) ja julkisessa
tilassa myös `-PublicHost`:n (oletus: `-Server`-osoite) ja `-GatewayPort`:n (oletus `443`). Kun
asennat uudelleen palvelimelle, jonka olet jo pystyttänyt, anna uudelleen `-Mode Private`,
SSH-osoitteesta poikkeava `-PublicHost`, muu yhdyskäytävän portti kuin 443, `-ServerName` ja
`-InteractiveSession`, jos käytit niitä. `-Update` ei aja asennusohjelmaa.

## Pelaa itse ensin

Todista koko polku oikeasta ulkoisesta osoitteesta ennen kuin kutsut ketään. Huomaa, että oman koneesi
käynnistimen välitin tarvitsee portin 61000, samaa porttia käyttää paikallinen kehityspino, joten oma
**PELAA epäonnistuu virheellä `relay_port_busy`, kun tuo pino on käynnissä**.

1. Niinä iltoina kun pelaat vuokrapalvelimella, pysäytä paikallinen pino (`C:\dr\tools\stack.ps1 stop`)
   tai jätä käynnistämättä.
2. Asenna käynnistin ja tee kutsu itsellesi: `Deploy-Remote.ps1 -Server <osoite> -InviteFor <sinä>`.
3. Rekisteröidy, asenna ja pelaa, kunnes Ramsgate latautuu.
4. Tarkista palvelimella, että `Stack.ps1 status` listaa osoitteesi sallittujen listalla ja näyttää
   Ramsgaten UDP-portissa 8777.
5. Vasta sitten lähetä kutsut kavereille.

`DAUNTLESS_REVIVED_RELAY_PORT` siirtää välittimen pois portista 61000, mutta se on vain testejä varten;
kaverit käyttävät aina porttia 61000.

## Ennen kuin kaverit liittyvät

Käy tämä lista läpi, kun palvelin on asennettu:

1. **Palveluntarjoajalla** (pilvipalomuuri / security group, esim. AWS security groups, Azure NSG,
   Hetznerin/Vultrin pilvipalomuurit) salli saapuva **mistä tahansa osoitteesta**: TCP `-GatewayPort`
   (oletuksena 443) ja **UDP 8770-8777**. Moni tarjoaja estää saapuvan oletuksena, ja tämän palvelimen
   oma sallittujen lista rajaa UDP:n jo kirjautuneisiin pelaajiin. Oma koneesi ei voi testata UDP-polkua.
2. Rajaa palveluntarjoajalla **TCP 22** ja **TCP 3389** omaan osoitteeseesi.
3. Jaa käynnistin. Käynnistimessä ei ole latauslinkkiä: kaverit hakevat sen osoitteesta
   **`https://github.com/mixutin/dauntless-revived/releases/latest`**
   (`DauntlessRevivedLauncher-Setup.exe`). Sitä ei ole allekirjoitettu, joten Windows SmartScreen
   varoittaa ensimmäisellä kerralla: **Lisätietoja > Suorita silti**. Koneella, jossa tunnistamattomat
   sovellukset on asetettu estettäviksi, SmartScreen estää sen kokonaan eikä tarjoa Suorita silti
   -vaihtoehtoa; silloin kaveri tarkistaa tiedoston SHA-256:n saman julkaisun `SHA256SUMS.txt`-tiedostoa
   vasten ja poistaa eston (hiiren oikea > **Ominaisuudet** > **Poista esto**, tai `Unblock-File`),
   kuten sivulla [Liity kaverina]({{ friends_page.url | relative_url }}) kerrotaan. Allekirjoitus on
   tiekartan kohta 4.16. CI julkaisee jokaisen
   `legacy/UndauntedLauncher/package.json`-tiedoston uuden version itse, kun kaikki tarkistukset menevät läpi
   (repositorion muuttuja `LAUNCHER_AUTO_RELEASE` arvolla `false` pysäyttää sen), ja Actions >
   **Launcher release** > **Run workflow** `dauntless-revived`-haaralle julkaisee käsin (ks.
   käynnistimen README, kohta "Julkaisut ja päivitykset"). Tai lähetä `Setup.exe` SHA-256-summineen yksityisesti.

Jos kaveri **kirjautuu ja jää sitten jumiin Ramsgatea ladatessa**, palveluntarjoaja lähes varmasti estää
UDP:n 8770-8777. Lisää yllä oleva sääntö ja tarkista palvelimen oma sallittujen lista:
`Get-NetFirewallRule -Name DauntlessRevived-GamePorts-Allowlist | Get-NetFirewallAddressFilter`.

## Kutsut

Omalta koneeltasi:

```powershell
.\deploy\windows-server\Deploy-Remote.ps1 -Server 203.0.113.7 -InviteFor Alex
```

tai palvelimella: `C:\DauntlessRevived\bin\New-Invite.ps1 -For Alex`. Se tekee kertakäyttöisen
kutsukoodin (`-Uses` useampaan), tarkistaa ensin että yhdyskäytävä vastaa oikealla varmenteella, ja
tulostaa yhden rivin:

```
dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=<64 heksamerkkiä>&code=ABCD-EFGH-JKLM&name=Lauantain%20Ramsgate
```

Kaveri liittää rivin käynnistimeen. Koodilla voi luoda yhden tilin, joten lähetä se yksityisesti.
`-List` näyttää vielä käyttämättömät koodit, `-Revoke <koodi>` poistaa koodin.

Kaverit hakevat käynnistimen osoitteesta
[github.com/mixutin/dauntless-revived/releases/latest](https://github.com/mixutin/dauntless-revived/releases/latest)
(`DauntlessRevivedLauncher-Setup.exe`). Sitä ei ole allekirjoitettu, joten Windows voi varoittaa, ettei
sovellusta tunnisteta: **Lisätietoja > Suorita silti**. Jos Windows estää sen eikä tarjoa Suorita silti
-vaihtoehtoa, kaveri tarkistaa sen `SHA256SUMS.txt`-tiedostoa vasten ja poistaa eston (ks.
[Liity kaverina]({{ friends_page.url | relative_url }})). Pidä käynnistin auki pelatessasi.

`fp` on varmenteen sormenjälki. Niin kauan kuin varmenne pysyy samana, vanhat kutsurivit toimivat,
myös kun palvelin palautetaan uudelle koneelle samalla osoitteella tai verkkotunnuksella
(varmuuskopiot sisältävät varmenteen). Kaverit, joilla on jo tili (siirretty mukana `-RestoreFrom`:lla),
tarvitsevat silti kutsurivin löytääkseen palvelimen: vain kutsu sisältää osoitteen ja varmenteen
sormenjäljen. Käynnistimessä he valitsevat **"Minulla on jo tiliavain"** ja liittävät vanhan
`account.key`-tiedostonsa sisällön rekisteröitymisen sijaan. Jos he painavat REKISTERÖIDY, kutsukoodi
kuluu ja syntyy toinen, tyhjä tili.

## Arjessa

Nämä ajetaan palvelimella (SSH:n kautta: `ssh -i <avain> Administrator@<palvelin>`, sitten `powershell`):

| Komento | Mitä se tekee |
|:--------|:--------------|
| `C:\DauntlessRevived\bin\Stack.ps1 status` | Jokainen osa prosesseineen ja portteineen, yhdyskäytävän TLS-tarkistus, sallittujen lista, pelipalvelimet, ajastetut tehtävät ja viimeisin varmuuskopio. |
| `Stack.ps1 stop` / `start` / `restart` | Pysäyttää tai käynnistää kaiken ajastettujen tehtävien kautta. Pysäytys sulkee myös peliportit. `restart -Only gateway` käynnistää yhden osan uudelleen. |
| `Get-ServerStatus.ps1` | Ketkä ovat paikalla ja mitkä maailmat ja metsästykset ovat käynnissä, kysyttynä yhdyskäytävän kautta kuten kaveri kysyisi. Palvelin näyttää listan vain rekisteröityneille pelaajille, joten skripti kysyy omistajan avaimella (aja se järjestelmänvalvojana). Omalta koneelta: `Get-ServerStatus.ps1 -Invite '<kutsurivi>' -KeyFile <oma account.key>`. Ilman avainta se kertoo, että lista on piilotettu. |
| `Backup-DauntlessServer.ps1` | Varmuuskopio heti (tehdään myös tunnin välein sekä jokaisen käynnistyksen ja pysäytyksen yhteydessä). |
| `Write-PerformanceLog.ps1 -Once` | Suorituskykymittaus heti (aja järjestelmänvalvojana). Kokonaisuus ottaa mittauksen itse minuutin välein kansioon `data\logs\performance\`: pelipalvelinten ja osien suoritin ja muisti, koneen suoritin, keskusmuisti, levy ja verkko sekä pelaajamäärät. Katso [Mittaa se]({{ admin_page.url | relative_url }}#measure-it). |
| `Update-DauntlessServer.ps1 -Ref <tagi>` | Uusi palvelinkoodi, katso alta. |
| `Set-Chat.ps1 -On` / `-Off` | Kytkee pelin tekstichatin päälle tai pois; katso [Chat](#chat) alta. Käynnistää kokonaisuuden uudelleen. |

Omalta koneelta `Deploy-Remote.ps1 -Server <osoite> -Status` näyttää tilanteen kirjautumatta.

### Chat {#chat}

Pelin tekstichat (Ramsgaten ja metsästysten chat, ryhmächat, kiltachat ja kuiskaukset käyttäjänimin)
on metagamen sisällä toimiva kuuntelija osoitteessa `127.0.0.1:61099`. Yhdyskäytävä välittää pelin
chat-yhteyden sille jo valmiiksi, joten **palomuurista ei tarvitse avata mitään**, eivätkä kaverit
tarvitse uutta käynnistintä. Uudessa julkisessa asennuksessa se on oletuksena päällä. Vanha tallennettu
Off säilyy: ota chat käyttöön komennolla `Set-Chat.ps1 -On`, kun kukaan ei pelaa. Miten se toimii:
[Tekstichat]({{ chat_page.url | relative_url }}).

**Kytke se, kun kukaan ei pelaa.** Chatin kytkeminen päälle tai pois käynnistää kokonaisuuden
uudelleen, ja uudelleenkäynnistys pudottaa ryhmät ja matchmaking-jonot, jotka ovat vain muistissa.

**Jos palvelin on asennettu ennen chattia, päivitä ensin.** `Set-Chat.ps1` tulee chatin sisältävän
koodin mukana, joten vanhemmassa versiossa sitä ei vielä ole: aja
`Deploy-Remote.ps1 -Server <osoite> -Update` (tai palvelimella `Update-DauntlessServer.ps1`) ja kytke
chat sitten päälle. `-Chat` ei käy yhdessä valinnan `-Update` kanssa, joten ajoja ja
uudelleenkäynnistyksiä on kaksi.

- Palvelimella: `C:\DauntlessRevived\bin\Set-Chat.ps1 -On` (tai `-Off`). Se kirjoittaa tiedostoon
  `metagame.env` asetukset `CHAT`, `CHAT_BIND_HOST=127.0.0.1` ja `CHAT_PORT` sekä tiedostoon
  `server.json` kohdan `"Chat"`, käynnistää kokonaisuuden uudelleen tavallisine varmuuskopioineen ja
  odottaa kuuntelijaa enintään 30 sekuntia. Jos se ei onnistu, se palauttaa vanhat asetukset ja
  käynnistää uudelleen.
- Omalta koneelta: `Deploy-Remote.ps1 -Server <osoite> -Chat On` ajaa saman skriptin SSH:n kautta.
- Asennuksessa: `-Chat On` tai `-Chat Off` (sekä asennusohjelma että `Deploy-Remote.ps1` ottavat sen).
  Uusi ajo ilman valintaa `-Chat` pitää tallennetun valinnan, eikä päivitys koskaan muuta sitä.

`Stack.ps1 status` näyttää `chat`-rivin: `listening 127.0.0.1:61099`, `off` tai
`on in metagame.env but not listening (see the metagame log)`; `Get-ServerStatus.ps1` näyttää saman
palvelimella. Metagamen loki alkaa rivillä `chat: listening on 127.0.0.1:61099 (nick check enforce)`,
ja sen jälkeen siinä on `chat:`-rivejä yhteyksistä, kirjautumisista, huoneisiin liittymisistä ja
viesteistä (ei koskaan niiden tekstiä). [Vianetsintä]({{ trouble_page.url | relative_url }}#chat-not-connected)
selittää jokaisen.

Ensimmäisissä oikeissa ajoissa `CHAT_TRACE=1` tiedostossa `metagame.env` kirjaa jokaisen chat-kehyksen
niin, että kirjautumiset, viestien teksti ja tunnisteet on korvattu; lisää se ennen komentoa
`Set-Chat.ps1 -On`, jonka uudelleenkäynnistys ottaa sen käyttöön, ja poista se sen jälkeen. Jos oikeiden
pelaajien huoneisiin liittymiset hylätään syyllä `reason=nick-resource`, `nick-format` tai
`nick-name`, `CHAT_NICK_CHECK=log` päästää heidät sisään varoituksen kera sillä aikaa, kun raportoit
rivin. `Set-Chat.ps1 -Off` on paluutie mistä tahansa vakavasta: peli yrittää silloin uudelleen 15-45
sekunnin välein vaarattomasti, kuten ennen chattia. Jos vika on itse chat-koodissa,
`Update-DauntlessServer.ps1 -Rollback` palaa päivitystä edeltäneeseen versioon; vanhempi koodi ei lue
asetusta `CHAT`. Yksityisessä tilassa chattia ei vielä ole.

**Kavereiden paikalla olo** on toinen kytkin, `CHAT_PRESENCE=1`, jota `Set-Chat.ps1` ei aseta: lisää
se käsin tiedostoon `metagame.env`, kun chat toimii, ja käynnistä metagame uudelleen, kun kukaan ei
pelaa (`Stack.ps1 restart -Only metagame`). Metagamen lokissa lukee silloin `chat: friends' online
status on`. Testaa se sivun [Tekstichat]({{ chat_page.url | relative_url }}#how-to-verify-presence)
ohjeen mukaan ennen kuin jätät sen päälle; rivin poistaminen kytkee sen taas pois.

### Muut ominaisuudet, jotka kytketään tiedostossa `metagame.env` {#other-switches}

Paketti ei kirjoita yhtään Harmonicin forkin siirron mukana tulleista kytkimistä, joten paketin
palvelimella ne ovat oletusarvoissaan: Slayer Links päällä; Escalation (`ESCALATION_MODE`), kauppa
(`STORE`) ja kavereiden paikalla olo pois. Muuttaaksesi jotakin lisää rivi tiedostoon
`C:\DauntlessRevived\data\config\metagame.env` (deploy-palvelimelle `deployserver.env`)
järjestelmänvalvojana avatulla editorilla ja käynnistä kyseinen osa uudelleen, kun kukaan ei pelaa.
Paketti säilyttää rivin uudelleenajoissa ja päivityksissä. Mitä kukin tekee ja mitä tarkistaa ensin,
kerrotaan sivulla [Palvelin ryhmälle]({{ admin_page.url | relative_url }}#switching-features-on).

### Päivitykset

`Deploy-Remote.ps1 -Server <osoite> -Update` lähettää nykyisen commitisi ja ajaa palvelimella
`Update-DauntlessServer.ps1`:n (anna uuden commitin `-ServerBinaries`, kuten asennuksessa). Se
laittaa uuden koodin ja ohjelmat paikoilleen palvelimen pyöriessä, ottaa varmuuskopion, vaihtaa
uuteen ja tarkistaa, että metagame vastaa ja yhdyskäytävä vastaa kutsujen varmenteella. Jos
tarkistus ei onnistu kolmessa minuutissa, se palaa itse edelliseen versioon.
`Update-DauntlessServer.ps1 -Rollback` tekee saman käsin. Asetuksiin (paitsi `metagame.env`-tiedoston
`GIT_COMMIT`-arvoon, johon kirjataan uusi commit), avaimiin, varmenteeseen,
pelitiedostoihin eikä palvelutilin `Game.ini`- ja `Engine.ini`-tiedostoihin kosketa; niitä varten
aja asennus uudelleen. Tee niin päivityksen jälkeen, joka muuttaa DLL:n osoitetaulukkoa, ja
yksityisessä tilassa silloin, kun palvelimen Tailscale-osoite vaihtuu.

**Pelaajalista on vain rekisteröityneille pelaajille: käynnistin ensin.** Palvelin näyttää paikalla
olijat vain kysyjälle, jolla on tiliavain, ja käynnistin lähettää pelaajan avaimen nähdäkseen listan.
Ennen tätä muutosta julkaistut käynnistimet eivät lähetä avainta. Muutoksen sisältävää palvelinta
vasten ne näyttävät "0 pelaajaa paikalla" ja "Yhtään maailmaa ei ole nyt käynnissä.", kunnes ne
päivittävät itsensä. Kun päivität palvelimen vanhemmasta versiosta, tee se vasta, kun muutoksen
sisältävä käynnistin on julkaistu, tai kerro kavereille, että luku on väärin, kunnes heidän
käynnistimensä on päivittynyt.

**Oikea eteneminen on nyt oletus: päätä ennen päivitystä.** Palvelin, jonka `metagame.env`-tiedostossa
ei ole `PROGRESSION_MODE`-riviä (tai rivi on tyhjä tai sen arvo on muu kuin `real` tai `stub`),
vaihtaa alkuperäisen projektin valemaksimitasoista oikeaan etenemiseen, ja aiemmin pelanneet
aloittavat Slayer-tasolta 1. Päivitys ei siirrä mitään. Pidä valittujen pelaajien maksimitasot
antamalla heille lähtötaso `grandfather`, tai lisää ensin `PROGRESSION_MODE=stub` tiedostoon
`C:\DauntlessRevived\data\config\metagame.env`, jos haluat jatkaa tyngällä. Ellei tiedostossa lue
`real` tai `stub`, päivitysohjelma toistaa onnistuneen päivityksen jälkeen metagamen varoituksen
pelaajista, joilla ei ole tallennettua etenemistä. Paluu edelliseen versioon ja palautus
varmuuskopiosta noudattavat samaa sääntöä, joten erikseen asetettu rivi pitää tilan samana.
[Päivitysohjeissa]({{ upgrade_page.url | relative_url }}) on yksityiskohdat ja skripti.

### Varmuuskopiot

Varmuuskopiot ovat kansiossa `C:\DauntlessRevived\backups\<päivä>_<aika>\`: tietokanta (kopioitu
SQLiten online backup -rajapinnalla palvelimen pyöriessä ja tarkistettu), asetukset avaimineen,
ylläpitäjätilin avain, yhdyskäytävän varmenne, `server.json` ja käynnistimen uutiset. Tallessa
pidetään 48 uusinta varmuuskopiota (tunnin välein otetut sekä käynnistysten ja pysäytysten yhteydessä
otetut yhteensä) ja lisäksi viimeisten 30 päivän jokaisen päivän uusin kopio. **Niissä on avaimia**:
kopioi ne palvelimelta pois vain salattuina. Samaa kansiota `-RestoreFrom` käyttää. Mitä
varmuuskopiossa on ja mitä siinä ei ole: [Tiedostot ja data]({{ files_page.url | relative_url }}#backups).

## Istunto 0 ja `-InteractiveSession` {#session-0-and--interactivesession}

Käynnistyksen jälkeen palvelin pyörii ilman kirjautunutta käyttäjää (Windowsin "istunto 0").
Pelipalvelimet avaavat käynnistyessään konsoli-ikkunan. Sen pitäisi toimia ilman työpöytää, mutta
oikealla palvelimella tätä ei ole vielä kokeiltu. Jos `Stack.ps1 status` ei minuutin kuluttua
käynnistyksestä näytä Ramsgatea UDP-portissa 8777, aja asennus uudelleen valinnalla
`-InteractiveSession`. Silloin `dauntless`-tili kirjautuu automaattisesti koneen käynnistyessä (sen
salasana tallennetaan samoin kuin Windows tallentaa automaattisen kirjautumisen salasanan,
LSA-salaisuutena, ei koskaan selväkielisenä), palvelin käynnistyy siinä istunnossa, ja istunto
lukitaan heti.

## Yksityinen tila (Tailscale)

`-Mode Private` asentaa Tailscalen (kiinnitetty MSI-tiiviste), kirjaa palvelimen sisään, laittaa
metagamen ja sisältöpalvelimen kuuntelemaan vain Tailscale-osoitteessa ja rajaa palomuurin
Tailscale-osoitteisiin. Tässä tilassa ei ole yhdyskäytävää eikä sallittujen listaa, ja
`New-Invite.ps1` tekee v1-kutsuja:
`dauntless-revived://join?v=1&host=<tailnet-osoite>&port=61000&code=...&name=...`.
Koneen jakamisesta kerrotaan sivulla [Palvelin ryhmälle]({{ admin_page.url | relative_url }}).

## Tiedostot palvelimella

| Polku | Mitä se on |
|:------|:-----------|
| `C:\DauntlessRevived\bin\` | Paketin skriptit. |
| `app\` (ja `app.prev\`) | Käännetty palvelinkoodi (ja edellinen versio paluuta varten). |
| `game\Dauntless\` | Tarkistetut 1.4.4-tiedostot. |
| `data\config\` | `server.json` ja `.env`-asetukset (salaisuuksia). Mitkä avaimet asennus kirjoittaa ja mitkä se säilyttää: [asetusten viitesivu]({{ config_page.url | relative_url }}#server-kit). |
| `data\keys\`, `data\tls\` | Ylläpitäjätilin avain ja pelipalvelimen avain; yhdyskäytävän varmenne ja sen yksityinen avain. |
| `data\undaunted.db` | Tietokanta: tilit ja tallennukset. |
| `data\logs\` | Kaikkien osien lokit; `gateway.out.log` on pääsyloki (ei avaimia). `performance\` sisältää suorituskykylokin, yhden CSV-tiedoston päivässä (vain lukuja). |
| `data\allowlist\` | Apurin tarkastusloki ja tila (vain ylläpitäjät ja SYSTEM voivat kirjoittaa sinne). |
| `backups\` | Tunnin välein otetut varmuuskopiot. |
| `staging\` | Mitä `Deploy-Remote.ps1` lähettää; pelin zip jää tänne korjauksia varten. |

Jokainen kansio ja tiedosto sekä se, kuka saa lukea ja kirjoittaa niitä:
[Tiedostot ja data]({{ files_page.url | relative_url }}#kit-install-root).

## Paketin testaus ilman palvelinta

Kansiossa `deploy\windows-server\tests\` on kolme testiskriptiä kehityskoneelle. Mikään niistä ei
muuta palomuuria, palveluita, ajastettuja tehtäviä, käyttäjätilejä tai varmennesäilöjä, eikä käytä
pyörivän palvelimen portteja:

- `Test-KitUnit.ps1`: jokainen skripti jäsentyy PowerShell 5.1:ssä, kutsurivit (v1 ja v2, myös
  kaikki mitkä pitää hylätä), varmenteiden sormenjäljet, TLS-kiinnitys paikallista testipalvelinta
  vasten, ja lähetysapuri.
- `Test-Sandbox.ps1`: oikea `-Sandbox`-asennus kansioon `C:\dr\sandbox-ws2019` vapaisiin
  paikallisiin portteihin (metagame 62000, sisältö 62002, apuri 62005 kuivaharjoitustilassa,
  yhdyskäytävä 62443), sitten kutsut, tila ja rekisteröityminen yhdyskäytävän kautta kiinnitetyllä
  varmenteella, yhdyskäytävän torjunnat, palautus toiseen kansioon ja siivous.
- `Test-DeployRemote.ps1`: parametrien käsittely, ja lähetyslogiikka paikallista kansiota vasten,
  joka esittää palvelinta (katkennut lähetys, matkalla vioittunut osa, tarkistuksen jälkeen
  vioittunut osa).

## Poistaminen {#uninstall}

Palvelimella, ylläpitäjänä avatussa PowerShellissä. **Ota ja kopioi varmuuskopio ensin talteen**,
jos haluat säilyttää jotain.

```powershell
C:\DauntlessRevived\bin\Stack.ps1 stop
'Dauntless Revived stack', 'Dauntless Revived allowlist', 'Dauntless Revived backup' |
    ForEach-Object { Unregister-ScheduledTask -TaskName $_ -Confirm:$false -ErrorAction SilentlyContinue }
# Säilytä 'Dauntless Revived - SSH (TCP 22)', jos asennusohjelma teki sen: jokainen profiili estää yhä
# saapuvat yhteydet oletuksena, joten ilman tätä sääntöä SSH lakkaisi toimimasta.
Get-NetFirewallRule -Group 'Dauntless Revived' -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -ne 'Dauntless Revived - SSH (TCP 22)' } | Remove-NetFirewallRule
Remove-NetFirewallRule -Name 'DauntlessRevived-GamePorts-Allowlist' -ErrorAction SilentlyContinue
Get-CimInstance Win32_UserProfile | Where-Object { $_.LocalPath -like '*\dauntless' } | Remove-CimInstance
Remove-LocalUser -Name dauntless
Remove-Item -Recurse -Force C:\DauntlessRevived
```

Joitakin palomuurimuutoksia tämä ei peru: `-AdminIp`:hen rajattuja tai (julkinen tila ilman
`-AdminIp`:tä) pois käytöstä laitettuja etätyöpöytäsääntöjä, ja palomuuriprofiileja, jotka on asetettu
estämään saapuvat yhteydet oletuksena. Tiedoston `data\config\server.json` kohta `FirewallChanges`
kertoo, mitä ne olivat ennen (katso ennen kuin poistat kansion); ottaaksesi etätyöpöydän taas käyttöön
ota sen sääntö takaisin käyttöön (`Enable-NetFirewallRule`) tai aja asennus uudelleen `-AdminIp <IP>`. Jos käytit `-InteractiveSession`:ia, laita myös automaattinen kirjautuminen pois:
`Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' AutoAdminLogon 0`.
Visual C++ -kirjaston (ja Node.js:n palvelimelta, joka asennettiin ennen Rust-palveluja) voi
poistaa kohdasta "Sovellukset ja ominaisuudet".

Nämä asennusohjelman muutokset jäävät myös voimaan:

- Palomuurin käytäntöarvot, jotka asennusohjelma poisti, koska ne pitivät Windowsin palomuurin pois
  päältä. Ne on lueteltu kohdassa `FirewallChanges` `policy|...`-alkuisina merkintöinä.
- `PasswordAuthentication no` tiedostossa `C:\ProgramData\ssh\sshd_config`, jonka SSH:n yli ajettu
  julkisen tilan asennus kirjoitti.
- Ensimmäisen julkisen tilan asennuksen asettama tilien lukituskäytäntö: 10 väärää salasanaa lukitsee
  tilin 15 minuutiksi (`net accounts` näyttää sen).
- `-InteractiveSession`:in kanssa tallennettu automaattisen kirjautumisen salasana (LSA-salaisuus
  `DefaultPassword`). Tili, jolle se kuuluu, poistetaan yllä olevilla komennoilla, joten salasana ei
  enää avaa mitään.
- Tilille `dauntless` annettu oikeus "Kirjaudu erätyönä" (Log on as a batch job), DirectX June 2010
  -ajonaikainen kirjasto sekä yksityisessä tilassa Tailscale, jonka verkkokortin verkkoluokaksi
  asennusohjelma asetti Julkinen (Public).

Kun ylläpidät muokattua palvelinta muille, AGPL-lisenssi velvoittaa tarjoamaan lähdekoodin; katso
[Kiitokset ja lisenssi]({{ legal_page.url | relative_url }}).
