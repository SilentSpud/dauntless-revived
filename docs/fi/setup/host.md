---
title: Pystytä palvelin
parent: Asennus
grand_parent: Dauntless Revived suomeksi
nav_order: 1
description: "Vaiheittainen ohje Dauntless 1.4.4 -yksityispalvelimen pystyttämiseen Windows-koneelle: version tarkistus, kaksi DLL-tiedostoa, metagame ja deploy-palvelin."
lang: fi
ref: setup/host
locale: fi_FI
---

{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}
{% assign roadmap_page = site.pages | where: "path", "fi/roadmap.md" | first %}
{% assign verification_page = site.pages | where: "path", "fi/findings/verification.md" | first %}
{% assign multiplayer_page = site.pages | where: "path", "fi/findings/multiplayer.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}

# Pystytä palvelin
{: .no_toc }

Näin pyöritämme Dauntless Revivedia yhdellä Windows-tietokoneella. Käytössä on aito **Dauntless
1.4.4** -peliohjelma (lokakuu 2020, UE4 eli Unreal Engine 4 -pelimoottori, pak v9), Undauntedin
palvelin-DLL (peliin ladattava ohjelmakirjasto) sekä oma muokattu versiomme (fork) Undauntedin
metagamesta ja deploy-palvelimesta. Kaikki tällä sivulla pysyy osoitteessa `127.0.0.1` (koneen
sisäinen osoite, johon muut koneet eivät yllä). Kavereiden päästäminen mukaan on erillinen vaihe,
josta kerrotaan sivulla [Palvelin ryhmälle]({{ admin_page.url | relative_url }}).

Ellei rivillä sanota toisin, jokainen tieto tällä sivulla koskee **peliversiota 1.4.4**. Versio 2.1.1
(pelin viimeinen ”Awakening”-versio, UE5, IoStore) mainitaan vain silloin, kun aiempi työmme sen
parissa selittää jonkin valinnan.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Miten osat sopivat yhteen {#how-the-pieces-fit}

| Osa | Mikä se on | Kuuntelee |
|---|---|---|
| Peliohjelma (client) | `Dauntless-Win64-Shipping.exe` ja kaksi DLL-tiedostoa. Tavallisessa client-tilassa DLL ohjaa kaikki 167 taustapalvelun osoitetta (endpoint) osoitteeseen `http://<first command-line argument>` eli komentorivin ensimmäisenä annettuun osoitteeseen | ei mitään |
| Metagame | Node + Express + SQLite (`UndauntedMetagame`). Tilit, hahmot, tavarat, varustesetit (loadouts) ja matchmaking-jono (pelaajien yhteen sovittaminen) | TCP `127.0.0.1:61000` |
| Deploy-palvelin | Node (`UndauntedDeployServer`). Käynnistää ja valvoo pelipalvelinprosesseja, kun metagame pyytää | TCP `127.0.0.1:61001` (älä koskaan avaa sitä ulos: siinä ei ole tunnistautumista) |
| Pelipalvelimet | Lisää kopioita **samasta exe-tiedostosta**, käynnistettyinä valitsimilla `-server -nullrhi`. DLL kääntää ne palvelintilaan | UDP 8770–8777 |

Pelikerta etenee näin:

1. Peliohjelma kirjautuu metagameen tiliavaimellasi ja saa kirjautumistunnisteen (token), joka on
   voimassa 24 tuntia.
2. Kun valitset tekemisen, esimerkiksi metsästyksen, metagame pyytää deploy-palvelimelta
   pelipalvelinta.
3. Deploy-palvelin käynnistää sellaisen (tai antaa pysyvän Ramsgate-palvelimen UDP-portista 8777).
4. Peliohjelma siirtyy UDP:llä osoitteeseen `127.0.0.1:<port>`.
5. Pelipalvelin lataa hahmosi metagamesta. Näissä kutsuissa on mukana erillinen pelipalvelinavain,
   ja pyyntöloki merkitsee ne `gs=1`.

Kaupallinen exe-tiedosto ei pysty toimimaan palvelimena yksinään. Sivulla
[Näin moninpeli toimii]({{ multiplayer_page.url | relative_url }}) kerrotaan, miksi peliin ladattu
DLL voi silti pyörittää palvelinta. Päättelimme ensin, että moninpeli on mahdotonta versiolla, joka on
pelkkä peliohjelma. Se oli väärin, ja tuo sivu kertoo miksi.

---

## 1. Vaatimukset {#requirements}

> **Vuokraatko palvelimen?** EU Gamehost on projektin kumppani ja suositeltu hosting-palvelu.
> Katso [EU Gamehost -pakettiopas]({{ "/fi/setup/hosting-partner.html" | relative_url }}), jossa kumppanisisältö ja kapasiteettiperusteet on merkitty selvästi.

| | Mitä käytämme | Huomiot |
|---|---|---|
| Käyttöjärjestelmä | Windows 10 22H2 (koontiversio 19045) | Windows 11:n pitäisi toimia. Emme ole testanneet sitä. |
| Node.js | v24.19.0 (npm 11.17.0) | `better-sqlite3` on natiivimoduuli, joka on sidottu Node-versioon. Älä päivitä Nodea, kun ihmiset pelaavat. |
| Git | 2.55 for Windows | Tarvitaan forkia varten. Katso polkuhuomiot vaiheessa 4. |
| PowerShell | Windows PowerShell 5.1 (Windowsin mukana) | Jokainen tämän sivun komento on kirjoitettu versiolle 5.1. |
| VC++-ajonaikaiset kirjastot | Microsoft Visual C++ 2015-2022, x64 | `UndauntedInternalServer.dll` tarvitsee tiedostot `MSVCP140.dll` ja `VCRUNTIME140_1.dll`. Meidän koneessamme ne olivat jo valmiina. |
| Levytila | Noin 12 Gt | Peli 10,9 Gt (412 tiedostoa, kun DLL-tiedostot on lisätty), fork `node_modules`-kansioineen noin 0,3 Gt ja pieni tietokanta. Arkistosta purkaminen vaatii tilapäisesti suunnilleen saman verran lisää. |
| RAM (keskusmuisti) | Palvelinkoneessamme 32 Gt | Katso mittaukset alla. 16 Gt riittänee yhdelle pelaajalle ja palvelimille. Emme ole testanneet sitä. |
| Näytönohjain | Mikä tahansa DirectX 11 -näytönohjain peliohjelmalle | Pelipalvelimet ajetaan valitsimella `-nullrhi`, eivätkä ne käytä näytönohjainta. |

**Mitattu palvelinkoneellamme** (1.4.4, yksi pelaaja, Windowsin Tehtävienhallinnan working set eli
prosessin käytössä oleva muisti):

| Prosessi | Muisti | Suoritin |
|---|---|---|
| Ramsgate-palvelin (UDP 8777) | noin 1,1 Gt (huippu 1,19 Gt) | noin 0,6 ydintä latauksen aikana, sitten noin 0,2 ydintä |
| Opetusjakson tai metsästyksen palvelin | noin 0,9 Gt | ei vielä mitattu neljän pelaajan taistelussa |
| Training Dojo -palvelin | noin 0,94 Gt | noin 0,23 ydintä |
| Peliohjelma, Cinematic-laatu, 1920x1080 | 1,9–2,3 Gt working set (noin 3,7 Gt yksityistä muistia) | noin 2,5 ydintä |
| Metagame + deploy-palvelin (node) | noin 130 Mt yhteensä | mitätön |

Kun yksi ihminen pelaa palvelinkoneella, pelin prosessit vievät yhteensä noin 4–5 Gt. Jokainen
samanaikainen lisämetsästys lisää noin 1 Gt.

---

## 2. Hanki 1.4.4-versio ja tarkista se {#verify-the-build}

Tällä sivustolla ja lähdekoodissa ei ole pelitiedostoja eikä linkkejä niihin. Mistä saat kopion, on
sinun päätöksesi. Lähteestä riippumatta tarkista kopio ennen kuin ajat siitä mitään. Kaikki kolme alla olevaa tarkistusta ovat
nopeita.

**a. Versiomerkkijono.** Pelin juurikansion `Version.txt`-tiedostossa on oltava täsmälleen tämä
merkkijono:

```
dauntless_rel-1.4.4_Shipping_2020-10-28_20-11-15_239827
```

Tiedosto päättyy välilyöntiin ja rivinvaihtoon, joten vertaa siistittyä tekstiä:
`(Get-Content C:\D144\Dauntless\Version.txt).Trim()`. Lopun `239827` on koontiversion muutoslistan
numero (changelist). Se tulee uudelleen vastaan vaiheessa 9 nimellä `TARGET_CHANGELIST`.

**b. Exe-tiedoston tiiviste.** Tiiviste (hash) on tiedoston sisällöstä laskettu sormenjälki: jos
tiedostosta muuttuu yksikin tavu, tiiviste muuttuu. Jokainen Undauntedin DLL-tiedoston muistiosoite
on laskettu juuri tälle tiedostolle:

```powershell
(Get-FileHash "C:\D144\Dauntless\Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe").Hash
# must be D3D41E614908D2BEFD518B27046D9822D6130EF12BA3504BABBDB786BEF9CFF4  (103,673,520 bytes)
```

**c. Koko asennus Phoenixin oman luettelon (manifest) mukaan.** Pelin juurikansiossa on
`Manifest.bin.json`, jossa on yksi merkintä jokaista tiedostoa kohden (`RelativePath`, `FileSize`,
`MD5Chunks`). `MD5Chunks` on kahden merkin etuliite (tässä versiossa `18`), jota seuraa yksi
MD5-tiiviste jokaista tiedoston 16 MiB:n palaa kohden. Tulkitsemme etuliitteen näin:
2^0x18 = 16 MiB. Se on meidän tulkintamme, mutta se sopii jokaiseen tiedostoon. Tallenna tämä nimellä
`verify-manifest.js` mihin tahansa ja aja se Nodella:

```js
// node verify-manifest.js <game folder>
const fs = require("fs"), path = require("path"), crypto = require("crypto");
const root = process.argv[2] || ".";
const man = JSON.parse(fs.readFileSync(path.join(root, "Manifest.bin.json"), "utf8").replace(/^\uFEFF/, ""));
const CHUNK = 1 << 24; // 16 MiB
let ok = 0, bad = 0, missing = 0;
const buf = Buffer.alloc(CHUNK);
for (const e of man.TargetFiles) {
  const p = path.join(root, e.RelativePath.replace(/\\/g, path.sep));
  if (!fs.existsSync(p)) { missing++; console.log("MISSING  " + e.RelativePath); continue; }
  const want = e.MD5Chunks.slice(2).match(/.{32}/g) || [];
  const fd = fs.openSync(p, "r"); const got = []; let n;
  while ((n = fs.readSync(fd, buf, 0, CHUNK, null)) > 0) got.push(crypto.createHash("md5").update(buf.subarray(0, n)).digest("hex"));
  fs.closeSync(fd);
  if (fs.statSync(p).size === Number(e.FileSize) && got.join() === want.join()) ok++;
  else { bad++; console.log("MISMATCH " + e.RelativePath); }
}
console.log(`manifest files: ${man.TargetFiles.length}  ok: ${ok}  mismatched: ${bad}  missing: ${missing}`);
process.exit(bad || missing ? 1 : 0);
```

```
PS> node verify-manifest.js C:\D144\Dauntless
manifest files: 406  ok: 406  mismatched: 0  missing: 0
```

Meidän kopiollamme se kesti 18 sekuntia. On odotettavaa, että levyllä on tiedostoja, joita luettelossa
ei ole: itse luettelo, `Manifest.bin`, kaksi tyhjää `debug.log`-tiedostoa ja myöhemmin ne kaksi
DLL-tiedostoa, jotka lisäät vaiheessa 5.

Sama luettelo on myös versiossa 2.1.1. Sivulla [Pelitiedostojen tarkistaminen]({{ verification_page.url | relative_url }})
kerrotaan syvemmistä tarkistuksista, jotka teimme molemmille versioille: Authenticode-allekirjoitusten
tiivisteet, miksi `osslsigncode` sanoo ”FAILED” koskemattomista tiedostoista, ja ClamAV-virustarkistukset.

---

## 3. Asenna lyhyeen polkuun: `C:\D144` {#short-install-path}

Pidämme pelin kansiossa `C:\D144\Dauntless`, joten ohjelmatiedostot ovat kansiossa
`C:\D144\Dauntless\Archon\Binaries\Win64`. Jos kopiosi on zip-tiedosto, Windows 10:n mukana tuleva
`tar` purkaa sen:

```powershell
New-Item -ItemType Directory -Force C:\D144 | Out-Null
tar -xf "<path to your archive>.zip" -C C:\D144
Get-ChildItem C:\D144\Dauntless     # Archon, EasyAntiCheat, Engine, Dauntless.exe, Manifest.bin(.json), Version.txt
```

**Miksi lyhyt polku.** Perinteiset Win32-tiedostorajapinnat pysähtyvät 260 merkkiin (`MAX_PATH`),
elleivät sekä Windows että ohjelma ota pitkiä polkuja erikseen käyttöön. Useimmat tämän hankkeen
ympärillä käytettävät työkalut eivät ota.

- Pelin oma kansiorakenne ei ole ongelma. Sen pisin tiedostopolku on 102 merkkiä asennuskansion alla.
  Emme ole koskaan nähneet itse pelin epäonnistuvan polun pituuden takia.
- Ympärillä olevat työkalut sen sijaan epäonnistuivat. Kun kopioimme Undauntedin `git clone`
  -komennolla syvällä sisäkkäisissä kansioissa olevaan työkansioon, se kaatui virheisiin
  `Filename too long` ja sitten `fatal: '$GIT_DIR' too big`. Forkissa on noin 4 300 versionhallinnassa
  olevaa tiedostoa, joista 4 148 on generoidussa Dumper-7 SDK:ssa, ja repositorion sisäiset polut ovat
  jopa 94 merkkiä pitkiä. Palvelin-DLL:n MSVC-käännös tarvitsee saman verran liikkumavaraa.
- Muutaman merkin mittainen juurikansio pitää kaiken reilusti rajan alla. Se pitää myös komentorivit
  ja `.env`-arvot lyhyinä ja välttää välilyönnit poluissa.

Emme ottaneet käyttöön koko järjestelmän pitkien polkujen asetusta (`LongPathsEnabled`). Se on
järjestelmäasetus, ja lyhyt polku tekee siitä tarpeettoman. Pidä asennus poissa OneDrivesta ja muista
synkronoiduista kansioista sekä käyttäjäprofiilistasi.

Käynnistä vain `Archon\Binaries\Win64\Dauntless-Win64-Shipping.exe`. Emme ole koskaan ajaneet
juurikansion `Dauntless.exe`-tiedostoa tai tiedostoa `EasyAntiCheat\EasyAntiCheat_Setup.exe`, eikä
mikään tässä tarvitse niitä.

---

## 4. Hae fork lyhyeen polkuun {#fork}

Fork eli muokattu versiomme on kansiossa `C:\dr\undaunted`, haarassa (branch) `dauntless-revived`.
Ajonaikainen data (SQLite-tietokanta, avaimet, lokit, pid-tiedostot) menee kansioon `C:\dr\data`,
repositorion ulkopuolelle.

```powershell
New-Item -ItemType Directory -Force C:\dr, C:\dr\data | Out-Null
git -c core.longpaths=true clone <URL of this repository> C:\dr\undaunted
git -C C:\dr\undaunted config core.longpaths true
git -C C:\dr\undaunted checkout dauntless-revived
git -C C:\dr\undaunted remote add upstream https://github.com/SyST3MDeV/Undaunted.git   # optional, to follow upstream
```

Repositoriossa (koodivarastossa) on kuusi projektia (`Undaunted*`-kansiot) sekä
Windows-palvelinpaketti, kaveripaketti ja työkalut. Palvelinkoneella käytämme näitä:

- `UndauntedMetagame`: taustapalvelu.
- `UndauntedDeployServer`: pelipalvelimien valvoja.
- Kaksi valmiiksi käännettyä DLL-tiedostoa kansiossa `UndauntedLauncher/assets/`.

Kansion `UndauntedLauncher/src` Electron-sovellus on tämän forkin käynnistin kavereille: pelaajat
liittyvät sillä palvelimelle kutsun avulla, ja se lataa pelin isännän omalta sisältöpalvelimelta.
Tämän sivun palvelinasennus ei tarvitse sitä. Palvelin-DLL:n C++-lähdekoodi on kansiossa
`native/internal-server`. Jokainen kansio ja se, mitä missäkin käännetään, on lueteltu sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}).

Alla luotavat `.env`-tiedostot ovat gitin ohittamia (git-ignored). Älä koskaan tallenna niitä
versionhallintaan: niissä ovat allekirjoitusavaimet ja pelipalvelinavain.

---

## 5. Laita kaksi DLL-tiedostoa paikoilleen ja tarkista kiinnitetyt tiivisteet {#dlls}

Undauntedin mukana tulee kaksi valmiiksi käännettyä DLL-tiedostoa:

- `dxgi.dll` on välittäjä (proxy). Windows lataa sen pelikansiosta järjestelmän oman `dxgi.dll`-tiedoston
  sijaan. Se lataa oikean `System32\dxgi.dll`-tiedoston, välittää eteenpäin kutsut
  `CreateDXGIFactory`, `CreateDXGIFactory1` ja `CreateDXGIFactory2` ja lataa
  `UndauntedInternalServer.dll`-tiedoston. Muuta se ei tee.
- `UndauntedInternalServer.dll`-tiedostolla on kaksi tilaa. Jos komentorivillä on `-server`, se
  muuttaa prosessin pelipalvelimeksi. Muuten se toimii peliohjelmana (client) ja ohjaa taustapalvelun
  osoitteet uudelleen.

Kopioi ne vasta, kun niiden tiivisteet täsmäävät:

```powershell
$A = "C:\dr\undaunted\UndauntedLauncher\assets"
$W = "C:\D144\Dauntless\Archon\Binaries\Win64"
$pin = @{
  "dxgi.dll"                    = "9A431D7B6FD20C43FA92BEBD91C3BC023EC7A3FCBC52871C41F4DF293D4B0D1F"  # 11,264 bytes
  "UndauntedInternalServer.dll" = "520EC588A0554E374B2B0D084CD7F7F08D59A9CB80362679845719D64A0D0933"  # 123,392 bytes
}
foreach ($f in $pin.Keys) {
  if ((Get-FileHash "$A\$f").Hash -ne $pin[$f]) { throw "$f does not match its pinned hash - stop here" }
  Copy-Item "$A\$f" $W -Force
}
foreach ($f in $pin.Keys) { "{0,-30} in place, hash ok: {1}" -f $f, ((Get-FileHash "$W\$f").Hash -eq $pin[$f]) }
```

Hyvä tietää ennen kuin luotat niihin:

- **Vain 1.4.4.** Palvelin-DLL muokkaa kiinteitä muistiosoitteita vaiheen 2b exe-tiedostossa, eikä se
  tarkista versiota. Missä tahansa muussa versiossa, myös 2.1.1:ssä, se kaataisi pelin tai sotkisi sen
  muistin. Älä koskaan kopioi näitä tiedostoja toiseen asennukseen.
- **Kumpaakaan tiedostoa ei ole allekirjoitettu**, ja peliin koukkuja asentava (hook) välittäjä-DLL on
  juuri sellainen asia, jonka virustorjunnan heuristiikka merkitsee epäilyttäväksi. Meidän koneellamme
  Windows Defender ei ilmoittanut mistään kopioinnin jälkeen. Katso
  [Vianetsintä]({{ trouble_page.url | relative_url }}#windows-defender-and-the-unsigned-dlls), jos
  sinun koneesi ilmoittaa.
- Analysoimme molemmat tiedostot staattisesti (ajamatta niitä) ennen käyttöä. `dxgi.dll`-tiedoston
  `DllMain`-funktion disassemblointi (konekielen purkaminen luettavaan muotoon) näyttää, että se tekee
  vain yllä luetellut asiat. `UndauntedInternalServer.dll`-tiedoston tuonnit (imports) ja jokainen
  siihen kovakoodattu pelin muistiosoite vastaavat julkaistua lähdekoodia tai sen generoitua SDK:ta,
  eikä sillä ole omia verkko-, rekisteri-, prosessinluonti- tai tiedostonkirjoitustuonteja.
  `dxgi.dll`-tiedoston lähdekoodi ei ole Undauntedin repositoriossa, eikä sitä tietääksemme ole
  julkaistu. Palvelin-DLL:n kääntäminen lähdekoodista ja oman `dxgi`-välittäjän kirjoittaminen ovat
  [tiekartalla]({{ roadmap_page.url | relative_url }}).
- Client-tilassa DLL avaa konsoli-ikkunan (mustan tekstiruudun) ja ottaa käyttöön pelinsisäisen
  konsolin näppäimellä **F2**.

---

## 6. Pidä asetuskansio erillään muista versioista {#config-folder}

Molemmat käyttämämme versiot, 1.4.4 ja 2.1.1, käyttävät samaa käyttäjän asetuskansiota, koska
projektin nimi on kummassakin `Archon`:

```
%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\
```

Pelipalvelimet pyörivät sinun Windows-käyttäjänäsi, joten nekin lukevat tätä kansiota. Jos olet
joskus ajanut toista versiota, varsinkin 2.1.1:tä kokeellisilla asetuksilla, siirrä sen kansio sivuun
ennen kuin valmistelet 1.4.4:n. Muuten jäänteet, kuten `GameDefaultMap`-ohitus tai
`[OnlineSubsystemPhoenix]`-lohko `Engine.ini`-tiedostossa, vaikuttaisivat myös 1.4.4:ään.

```powershell
$U = "$env:LOCALAPPDATA\Archon\Saved\Config"
if (Test-Path "$U\WindowsClient") { Rename-Item "$U\WindowsClient" "WindowsClient.211" }
New-Item -ItemType Directory -Force "$U\WindowsClient" | Out-Null
```

Vaihda kansiot takaisin ennen kuin ajat toista versiota taas. Peli täyttää tämän kansion omilla
tiedostoillaan ensimmäisellä käynnistyskerralla. Sinä kirjoitat vain tiedostot `Engine.ini`
(vaihe 7) ja `Game.ini` (vaihe 8).

Aja kaikki (metagame, deploy-palvelin, pelipalvelimet, peliohjelma) **samana Windows-käyttäjänä**.
Toisella tilillä tai Windows-palveluna (service) käynnistetyt pelipalvelimet lukisivat eri
`%LOCALAPPDATA%`-kansiota. Emme ole testanneet niiden ajamista palveluna.

---

## 7. Engine.ini {#engine-ini}

Käyttäjän `Engine.ini`-tiedoston `[SystemSettings]`-osio ohittaa pelin valikon asetukset.
Käynnistysskriptimme (`play.ps1`, vaihe 13) kirjoittaa jokaisella käynnistyksellä uudelleen kaksi
osiota, `[SystemSettings]` ja `[OnlineSubsystemMcp.XMPP]` (alla), ja jättää muun tiedoston rauhaan.
Peli itse kirjoittaa osioita kuten `[Core.System]` ja `[WindowsApplication.Accessibility]`; jätä ne
rauhaan. `Engine.ini`-tiedostoa ei tarvitse luoda käsin. Kun peli on käynnistetty oletusarvolla
`-Graphics 4`, `[SystemSettings]` näyttää tältä:

```ini
[SystemSettings]
r.Streaming.PoolSize=3000
r.Streaming.LimitPoolSizeToVRAM=1
gc.TimeBetweenPurgingPendingKillObjects=10
s.ForceGCAfterLevelStreamedOut=1
sg.ViewDistanceQuality=4
sg.AntiAliasingQuality=4
sg.ShadowQuality=4
sg.PostProcessQuality=4
sg.TextureQuality=4
sg.EffectsQuality=4
sg.FoliageQuality=4
sg.ShadingQuality=4
sg.ResolutionQuality=100
r.ScreenPercentage=100
r.MipMapLODBias=0
r.MaxAnisotropy=16
r.Tonemapper.Sharpen=0.6
```

| Asetus | Miksi |
|---|---|
| `r.Streaming.PoolSize=3000`, `r.Streaming.LimitPoolSizeToVRAM=1` | Tekstuurien suoratoistopooli megatavuina, ei koskaan suurempi kuin näytönohjaimen muisti. Tämä rajaa suurinta muistin kuluttajaa heikentämättä laatua. |
| `gc.TimeBetweenPurgingPendingKillObjects=10`, `s.ForceGCAfterLevelStreamedOut=1` | Siivoa roskat (vapauta käyttämätön muisti) useammin ja aina, kun jokin kentän osa poistuu muistista. Vaikuttaa muistiin, ei kuvanlaatuun. |
| `sg.*Quality=N` | Pakotettu laatutaso (katso vaihe 14). |
| `sg.ResolutionQuality=100`, `r.ScreenPercentage=100` | Piirrä täydellä natiiviresoluutiolla. |
| `r.MipMapLODBias=0`, `r.MaxAnisotropy=16` | Täyden resoluution tekstuuritasot (mipit); terävät tekstuurit myös vinossa katselukulmassa. |
| `r.Tonemapper.Sharpen=0.6` | Kumoaa UE4:n ajallisen reunanpehmennyksen (temporal anti-aliasing) aiheuttamaa pehmeyttä. |

Pelipalvelimet käyttävät samaa tiedostoa, mutta ne ajetaan valitsimella `-nullrhi`, joten niille
merkitsevät vain muistirivit.

**Chat ja paikallaolotieto (XMPP).** Sellaisenaan 1.4.4:n chat- ja paikallaoloyhteys (presence eli
tieto siitä, kuka on paikalla) osoittaa Epicin yhä toiminnassa olevaan palvelimeen
(`wss://xmpp-service-prod.ol.epicgames.com:443`). Peliohjelma yrittää jatkuvasti ottaa siihen
uudelleen yhteyttä ja lähettää tilin tunnisteen (account id) ja meidän kirjautumistunnisteemme. `play.ps1`
osoittaa yhteyden sen sijaan tähän koneeseen:

```ini
[OnlineSubsystemMcp.XMPP]
ServerAddr="ws://127.0.0.1"
ServerPort=61099
bUseSSL=false
```

Kun metagamen `.env`-tiedostossa on `CHAT=1` (vaihe 9), metagame kuuntelee porttia 61099 ja pelin
tekstichat toimii: Ramsgate, ryhmä, kilta ja kuiskaukset käyttäjänimin. Ilman sitä portissa ei
kuuntele mikään, joten yhteys epäonnistuu samalla tavalla kuin se jo epäonnistuu Epicin palvelinta
vastaan, ja peli jatkaa normaalisti. Osoite on lainausmerkeissä samasta syystä kuin vaiheen 8
osoitteet. Olemme varmistaneet, että peli säilyttää tämän osion, kun se kirjoittaa
`Engine.ini`-tiedoston uudelleen. **Varmistettu 21.9.2026:** ensimmäisten 90 sekunnin aikana
käynnistyksestä peliohjelma ei ottanut yhtään yhteyttä koneen ulkopuolelle. Se yritti sen sijaan
ottaa yhteyttä saman koneen porttiin 61099. Tämä on merkitty valmiiksi
[tiekartalle]({{ roadmap_page.url | relative_url }}).

Älä koskaan laita kenttäohituksia (map overrides) tai `[OnlineSubsystemPhoenix]`-osoitteita
`Engine.ini`-tiedostoon. Nuo osoitteet kuuluvat `Game.ini`-tiedostoon, ja niiden on oltava
lainausmerkeissä (vaihe 8).

---

## 8. Game.ini: 167 lainausmerkeissä olevaa osoiteohitusta pelipalvelimille {#game-ini}

**Miksi palvelimet tarvitsevat sitä.** Client-tilassa DLL kaappaa pelimoottorin asetushaun (hook) ja
vastaa 167 osoiteavaimeen arvolla `http://<metagame>/...`. Palvelintilassa se ei asenna tällaista
koukkua. Siksi pelipalvelin lukee osoitteensa tavallisesta asetusketjusta: ensin pelin mukana
tulleet (cooked) oletusarvot, jotka osoittavat Phoenixin `https://*.steelyard.ca`-palvelimiin (poissa
30.5.2025 alkaen), ja sitten tämän käyttäjän `Game.ini`-tiedosto. Ilman ohituksia pelipalvelin ei voi
ladata eikä tallentaa kenenkään hahmoa.

Peliohjelman koukku menee samoissa avaimissa tiedoston edelle, joten tiedosto ei muuta mitään
peliohjelman kannalta. Toiselta koneelta liittyvät kaverit eivät tarvitse sitä.

**Luo se DLL:n omasta osoitetaulukosta.** Älä muokkaa sitä käsin. Taulukko on tiedostossa
`native/internal-server/dllmain.cpp`, merkintöinä muotoa
`{L"AuthEndpoint", L"http://" + Globals::MetagameAddress + L"/game/login"}`. Tallenna tämä nimellä
`C:\dr\tools\make-gameini.ps1`:

```powershell
param([string]$Metagame = "127.0.0.1:61000",
      [string]$Source = "C:\dr\undaunted\native\internal-server\dllmain.cpp",
      [string]$Out = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient\Game.ini")
# Every endpoint the client-mode DLL rewrites: {L"Key", L"http://" + Globals::MetagameAddress [+ L"/path"]}
$re = '^\s*\{L"([A-Za-z0-9_]+)", L"http://" \+ Globals::MetagameAddress(?: \+ L"([^"]*)")?\},?'
$lines = foreach ($l in Get-Content $Source) {
  if ($l -match $re) { '{0}="http://{1}{2}"' -f $Matches[1], $Metagame, $Matches[2] }
}
Set-Content $Out -Encoding ASCII -Value (@("[OnlineSubsystemPhoenix]") + $lines)
"wrote $($lines.Count) quoted endpoint overrides to $Out"
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\make-gameini.ps1
# wrote 167 quoted endpoint overrides to ...\WindowsClient\Game.ini
```

Tulos alkaa näin:

```ini
[OnlineSubsystemPhoenix]
AuthEndpoint="http://127.0.0.1:61000/game/login"
AuthAvailableEndpoint="http://127.0.0.1:61000/checkavailable"
AuthTagsEndpoint="http://127.0.0.1:61000/tags"
...
```

Tarkistukset, jotka teimme omalle kopiollemme:

- Kaikki 167 avainta löytyvät pelin mukana tulleen 1.4.4:n `DefaultGame.ini`-tiedoston
  `[OnlineSubsystemPhoenix]`-osiosta, eikä kaksoiskappaleita ole.
- Kaksi niistä, `MatchmakingEndpoint` ja `TrackingEndpoint`, on pelkkä palvelinosoite ilman polkua.
  Pidä se mielessä, jos joskus muokkaat porttia käsin (katso alla).
- Phoenixin toimittamissa asetuksissa yksi avain, `PhoenixEventsMessageEndpoint`, sisälsi toimivan
  Slack-webhook-osoitteen (osoite, johon lähetetty viesti päätyy Slack-keskustelukanavalle). Emme
  toista sitä tässä. Tällä ohituksella avain osoittaa meidän metagameemme, joten Slackiin ei koskaan
  lähetetä mitään. DLL:n taulukko kuitenkin säilyttää tuon osoitteen polun, ja polussa on webhookin
  salainen osa, joten myös luotu `Game.ini` sisältää sen. Pidä `Game.ini` yksityisenä: älä liitä sitä
  keskusteluihin, issueihin tai kuvakaappauksiin.
- Phoenixin mukana tulleessa osiossa kaikki nämä 167 osoitetta ovat lainausmerkeissä, kuten meidän
  luomassamme tiedostossa.

**Miksi lainausmerkit ovat tärkeitä.** Pelimoottorin ini-jäsennin tulkitsee `//`-merkit
*lainausmerkittömässä* arvossa kommentin aluksi ja katkaisee arvon siihen. Näimme seurauksen
2.1.1:ssä: käyttäjän `Engine.ini`-tiedostossa olleen `[OnlineSubsystemPhoenix]`-lohkon kaikki 163 arvoa
olivat kutistuneet pelkäksi merkkijonoksi `https:` (esimerkiksi `AccountInfoEndpoint=https:`). Peli
kirjoittaa asetustiedostonsa takaisin levylle, joten vahingosta tulee pysyvä. Phoenixin omissa
asetuksissa jokainen osoite on lainausmerkeissä. Lainausmerkkien kanssa arvot säilyvät tallennuksessa
ehjinä. Peli on kirjoittanut meidän 1.4.4:n `Game.ini`-tiedostomme uudelleen monen pelikerran jälkeen,
ja kaikki 167 arvoa ovat yhä ehjiä. Emme ole testanneet lainausmerkittömiä arvoja 1.4.4:ssä. Siihen ei
ole syytä.

**Portin vaihtaminen myöhemmin.** Luo tiedosto uudelleen valitsimella `-Metagame 127.0.0.1:<new port>`
sen sijaan, että etsisit ja korvaisit tekstiä. Kun siirryimme portista 60000 porttiin 61000,
ensimmäinen etsi ja korvaa -yrityksemme osui merkkijonoon `:60000/` ja ohitti ne kaksi pelkän
palvelinosoitteen avainta, jolloin matchmaking ja telemetria (pelin lähettämät käyttötiedot) olisivat
jääneet vanhaan porttiin. Haku jäljelle jääneistä `60000`-merkkijonoista paljasti ne.

Pikainen kuntotarkistus:

```powershell
$G = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient\Game.ini"
$e = Get-Content $G | Select-Object -Skip 1 | Where-Object { $_ }
"entries: $($e.Count)  not fully quoted: $(@($e | Where-Object { $_ -notmatch '^[A-Za-z0-9_]+="[^"]*"$' }).Count)  not on 127.0.0.1:61000: $(@($e | Where-Object { $_ -notmatch '"http://127\.0\.0\.1:61000' }).Count)"
# entries: 167  not fully quoted: 0  not on 127.0.0.1:61000: 0
```

Palvelinpuolen versio osoitekoukusta poistaisi tämän tiedoston tarpeen. Se on
[tiekartalla]({{ roadmap_page.url | relative_url }}).

---

## 9. Metagame {#metagame}

Metagame on Undauntedin taustapalvelu: se hoitaa tilit, hahmot, tavarat ja pelaajien yhteen
sovittamisen.

**Asenna ja käännä.** `dist/`-kansio on gitin ohittama, joten käännät sen aina itse:

```powershell
Set-Location C:\dr\undaunted\UndauntedMetagame
npm ci --no-audit --no-fund
npm run build
Test-Path node_modules\better-sqlite3\build\Release\better_sqlite3.node   # must be True
```

npm 11 tulostaa rivejä `npm warn allow-scripts` paketeista `esbuild` ja `better-sqlite3`. Meidän
käännöksemme toimi silti. Katso
[Vianetsintä]({{ trouble_page.url | relative_url }}#npm-allow-scripts-warnings), jos viimeinen rivi
tulostaa `False`.

**Kirjoita `.env`.** Nämä ovat arvot, joilla ajamme. Mikään niistä ei ole salainen;
allekirjoitusavaimet lisätään seuraavassa vaiheessa.

```powershell
@"
PORT=61000
BIND_HOST=127.0.0.1
AUTH_MODE=APIKEY
DB_FILENAME=C:/dr/data/undaunted.db
TARGET_CHANGELIST=239827
QOS_TARGET_URL=http://127.0.0.1:61000/QoS
MATCHMAKING_MODE=DEPLOYSERVER
DEPLOYSERVER_URL=127.0.0.1:61001
REGISTRATION_MODE=OPEN
NODE_ENV=production
"@ | Set-Content C:\dr\undaunted\UndauntedMetagame\.env -Encoding ascii
```

**Luo kirjautumistunnisteiden allekirjoitusavaimet** ja lisää ne tiedoston loppuun tulostamatta niitä.
Käytä komentoa `Add-Content -Encoding ascii`, älä `>>`-merkkejä: Windows PowerShell 5.1:ssä `>>`
kirjoittaa oletuksena UTF-16-muodossa, jota Node ei osaa lukea `.env`-tiedostona.

```powershell
Set-Location C:\dr\undaunted\UndauntedMetagame
node -e "const c=require('crypto');const k=c.generateKeyPairSync('rsa',{modulusLength:2048,privateKeyEncoding:{type:'pkcs8',format:'pem'},publicKeyEncoding:{type:'spki',format:'pem'}});console.log('AUTH_SIGNING_PRIVKEY_B64='+Buffer.from(k.privateKey).toString('base64'));console.log('AUTH_SIGNING_PUBKEY_B64='+Buffer.from(k.publicKey).toString('base64'))" | Add-Content .env -Encoding ascii
```

| Avain | Merkitys |
|---|---|
| `PORT` | Metagamen HTTP-portti. Käytämme porttia **61000**. Katso porttihuomautus alla. |
| `BIND_HOST` | **Vain forkissa.** Osoite, jossa kuunnellaan. Oletus on `127.0.0.1`. Alkuperäinen projekti kuunteli kaikissa verkkoliitännöissä, mikä yhdessä asetuksen `REGISTRATION_MODE=OPEN` kanssa antoi kenen tahansa koneen tavoittavan luoda tilejä. Muuta sitä vain, kun seuraat sivua [Palvelin ryhmälle]({{ admin_page.url | relative_url }}). |
| `AUTH_MODE` | `APIKEY`: pelaajat kirjautuvat tilikohtaisella avaimella. `NONE` hyväksyy käyttäjätunnisteeksi mitä tahansa peliohjelma lähettää. Sitä noudatetaan vain tuotantotilan ulkopuolella; asetuksella `NODE_ENV=production` kirjautumispyyntö ei silloin saa vastausta lainkaan, ja peli jää odottamaan. Älä koskaan käytä sitä. |
| `AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64` | RSA-avainpari, PEM, base64. Allekirjoittaa 24 tuntia voimassa olevat RS256-istuntotunnisteet. Luo omasi; älä koskaan käytä kenenkään muun avaimia. |
| `DB_FILENAME` | SQLite-tiedosto. Käytä kauttaviivoja (`/`). Kansion `C:\dr\data` on oltava olemassa. |
| `TARGET_CHANGELIST` | `239827`, muutoslistan numero `Version.txt`-tiedostosta. Matchmaking-vastaus kertoo sen peliohjelmalle koontiversion tunnisteena `239827_1.4.4_shipping`. |
| `QOS_TARGET_URL` | Osoite, jota peliohjelma pingaa ”valitakseen alueen”. |
| `MATCHMAKING_MODE`, `DEPLOYSERVER_URL` | `DEPLOYSERVER` sekä osoite:portti ilman protokollaa (scheme): matchmaking annetaan deploy-palvelimen hoidettavaksi. |
| `REGISTRATION_MODE` | `OPEN`, `INVITECODE` tai `NONE`. `OPEN` on kunnossa niin kauan kuin metagame kuuntelee vain koneen sisäisessä osoitteessa (loopback). Vaihda tässä tiedostossa arvoksi `INVITECODE` ennen kuin kukaan muu voi tavoittaa sen. Ylläpitorajapinnan kautta tehty muutos kestää vain seuraavaan uudelleenkäynnistykseen. |
| `NODE_ENV` | `production`. Lokit ovat silloin pelkkiä JSON-rivejä. |
| `LOG_REQUESTS` | **Vain forkissa**, valinnainen. Jokainen pyyntö kirjataan muodossa `METHOD /path gs=0/1`, ellei arvo ole `0`. Julkisen tilan yhdyskäytävän takana rivin loppuun tulee ` via=gateway ip=<pelaajan osoite>`. Se on tärkein vianetsintävälineemme. |
| `PROGRESSION_MODE` | **Vain forkissa**, valinnainen. Puuttuva tai tyhjä (oletus) tai `real`: jokainen tili säilyttää oman Slayer-tasonsa, mestaruutensa (mastery), Hunt Passinsa (Elite-passi kaikille), varustesarjojen paikkansa, odotusaikansa (cooldowns) ja palkkiotehtävänsä (bounties). `stub`: alkuperäisen projektin valemaksimitasot, mitään ei tallenneta. Muu arvo kirjataan lokiin ja tulkitaan arvoksi `real`. Päivitätkö palvelinta, jolla on jo pelaajia? Lue ensin [päivitysohjeet]({{ upgrade_page.url | relative_url }}). |
| `PROGRESSION_REAL_ACCOUNTS` | **Vain forkissa**, valinnainen. Vain asetuksen `PROGRESSION_MODE=stub` kanssa: pilkuilla erotetut tilitunnukset, jotka saavat silti oikean etenemisen. |

**Muut valinnaiset kytkimet (vain forkissa).** Jätä ne pois, niin saat oletuksen. Kaikki muuttujat,
se mitä puuttuva arvo tarkoittaa ja kuka kunkin asettaa, ovat [asetusten viitesivulla]({{ config_page.url | relative_url }}#metagame).

| Avain | Oletus | Mitä se tekee |
|---|---|---|
| `CHAT` | pois | `1` kytkee pelin tekstichatin päälle: metagame kuuntelee myös osoitteessa `127.0.0.1:61099`, johon vaihe 7 ohjaa pelin chat-yhteyden, ja kirjoittaa käynnistyessään rivin `chat: listening on 127.0.0.1:61099 (nick check enforce)`. Oletuksena pois, kunnes kahden pelaajan testi vuokratulla palvelimella on läpäisty. Jos portti on varattu, metagame kirjoittaa yhden virherivin ja toimii ilman chattia. Yksityiskohdat: [Asetukset]({{ config_page.url | relative_url }}#metagame-chat). |
| `ENTITLEMENTS_DEFAULT` | `season09b_premium,season_premium_any,season_free_any` | Oikeudet (entitlements), jotka jokaisella tilillä on. Ensimmäinen on Elite Hunt Pass. |
| `INVENTORY_REFUSE_OVERSPEND` | pois | `1` torjuu tavarapyynnön, joka poistaa enemmän kuin pelaajalla on. Pois päältä, koska torjunta hylkää koko pyynnön palkintoineen, eikä yhtään metsästyksen lopun pyyntöä ole vielä tarkistettu sitä vasten. Siihen asti ylitys pysäytetään nollaan ja kirjataan lokiin tekstillä "Allowing overspend". |
| `DB_WAL` | pois | `1` vaihtaa tietokannan WAL-tilaan. Pois päältä, koska tässä kuvatut varmuuskopiot kopioivat pelkän tietokantatiedoston, ja äkillinen pysäytys jättää uusimmat tallennukset erilliseen `-wal`-tiedostoon. |
| `LOG_BODIES`, `BODY_LOG_FILE` | pois, `bodies.log` | `1` kirjoittaa keskeneräisten tallennusreittien pyyntöjen sisällön tiedostoon (8 kt kukin, tavaroilla 64 kt, tunnisteet poistettuina). Kehitysapu; se tallentaa, mitä pelaajat lähettävät. |
| `MATCHMAKING_CANCEL` | pois | `1` vastaa peliohjelman jonon perumiseen. Pois päältä, koska peliohjelma lähettää perumisen heti jokaisen jonoon liittymisen jälkeen, ja metsästykset alkavat vain siksi, että perumiseen vastataan 404. |
| `PROGRESSION_ALLOW_DELETE` | pois | `1` antaa pelipalvelimien nollata etenemisradan (vianetsintäkomento). Ylläpitäjän avaimella se onnistuu aina. |
| `PROGRESSION_GRANT_CAP` | `5000` | Suurin kokemuspistemäärä (XP), jonka yksi pyyntö voi lisätä yhdelle radalle. |
| `SAVE_HISTORY_KEEP`, `SAVE_HISTORY_HOURLY`, `SAVE_HISTORY_DAILY` | `100`, `48`, `30` | Hahmon ja varustesarjojen versiot, jotka säilytetään palautuksia varten: uusimmat, sitten yksi tunnissa, sitten yksi päivässä. |
| `INVENTORY_REPORT_REMOVALS`, `MISC_ROUTES`, `STATUS_EXTRA`, `ACCOUNT_DISPLAY_NAME` (`0`), `PROGRESSION_CONFIRM` (`off`) | päällä | Sulkeissa oleva arvo palauttaa yhden osan alkuperäisen projektin vanhasta toiminnasta vertailuja varten. Jätä ne asettamatta. |

**Miksi portit 61000/61001 eikä 60000.** Alkuperäisen projektin käynnistin käyttää kehitystilassa
osoitetta `127.0.0.1:60000`, ja ensimmäinen suunnitelmamme käytti portteja 60000/60001. Meidän
koneellamme `127.0.0.1:60000` on jo `ShadowUSB`:n varaama; se on osa Shadow-sovellusta. Alkuperäinen
metagame tulosti silloin onnistumisrivinsä ja sulkeutui, ja peliohjelmat jäivät odottamaan toista
ohjelmaa. Tarkista, että porttisi ovat vapaina, ennen kuin valitset ne:

```powershell
Get-NetTCPConnection -LocalPort 61000,61001 -State Listen -ErrorAction SilentlyContinue |
  ForEach-Object { "{0}:{1} is taken by {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
```

Jos mitään ei tulostu, molemmat portit ovat vapaina. Forkimme sulkeutuu nyt virheilmoitukseen, jos se
ei saa porttia käyttöönsä, sen sijaan että ilmoittaisi onnistuneensa. Katso
[Vianetsintä]({{ trouble_page.url | relative_url }}#port-60000-is-taken-and-the-metagame-says-clear-skies-anyway).

**Ensimmäinen käynnistys.** Käynnistä metagame aina sen omasta kansiosta, koska tietokannan
migraatiot (tietokannan rakenteen päivitykset) löydetään suhteellisen polun kautta (`./src/drizzle`).
Ensimmäinen käynnistys luo tietokannan ja ajaa kaikki migraatiot:

```powershell
$meta = @{ FilePath = "node"; ArgumentList = "--env-file=.env", "dist/server.js"
           WorkingDirectory = "C:\dr\undaunted\UndauntedMetagame"; PassThru = $true; WindowStyle = "Hidden"
           RedirectStandardOutput = "C:\dr\data\metagame.log"; RedirectStandardError = "C:\dr\data\metagame.err" }
$p = Start-Process @meta; Set-Content C:\dr\data\metagame.pid $p.Id
Start-Sleep 5
Get-Content C:\dr\data\metagame.log | ForEach-Object { ($_ | ConvertFrom-Json).msg }
```

Odotettu tulos:

```
Registered 0 new Gameserver API Key(s) on boot!
Registered 0 new User API Key(s) on boot!
Dauntless Revived metagame on 127.0.0.1:61000
Progression mode: real for every account (the default)
Clear Skies, Slayer.
```

`-RedirectStandardOutput` aloittaa joka kerta uuden lokin. Kopioi vanha loki ensin talteen, jos haluat
säilyttää sen.

**Luo pelipalvelinavain.** Pelipalvelimet tunnistautuvat metagameen erillisellä avaimella, joka
lähetetään `x-undaunted-gameserver-apikey`-otsakkeessa. Metagame tallentaa siitä vain SHA-256-tiivisteen.
Laitat avaimen jonotauluun, ja metagame laskee sen tiivisteen ja rekisteröi sen seuraavalla
käynnistyksellä:

```powershell
Set-Location C:\dr\undaunted\UndauntedMetagame
$key = node -e "console.log(require('crypto').randomBytes(24).toString('hex'))"
Set-Content C:\dr\data\gameserver.key -Value $key -Encoding ascii -NoNewline
node -e "new (require('better-sqlite3'))(process.argv[1]).prepare('INSERT INTO gameserverapikeystoregister(key) VALUES (?)').run(process.argv[2])" C:/dr/data/undaunted.db $key
$key = $null
Stop-Process -Id (Get-Content C:\dr\data\metagame.pid)
$p = Start-Process @meta; Set-Content C:\dr\data\metagame.pid $p.Id
Start-Sleep 5
Get-Content C:\dr\data\metagame.log | ForEach-Object { ($_ | ConvertFrom-Json).msg }
# Registered 1 new Gameserver API Key(s) on boot!
```

Selväkielinen avain on nyt vain tiedostossa `C:\dr\data\gameserver.key` ja vaiheen 11 jälkeen
deploy-palvelimen `.env`-tiedostossa. Pelipalvelimet saavat sen ensimmäisenä komentoriviparametrinaan.

---

## 10. Luo ylläpitäjän tili {#admin-account}

Tilit luodaan kutsulla `POST /undaunted/api/Register`. Vastauksessa on tiliavain. **Avain on
salasana.** Palvelin säilyttää siitä vain SHA-256-tiivisteen, eikä palautustyökalua vielä ole, joten
jos kadotat avaimen, menetät tilin. Ylläpitäjän oikeudet ovat lippu tietokannassa.

Käytämme tässä Noden `fetch`-funktiota aikarajan kanssa emmekä `Invoke-RestMethod`-komentoa (katso
[Vianetsintä]({{ trouble_page.url | relative_url }}#invoke-restmethod-hangs)). Aja tämä metagamen
kansiosta, jotta `better-sqlite3` löytyy, ja korvaa `YourName` omalla nimelläsi:

```powershell
Set-Location C:\dr\undaunted\UndauntedMetagame
node -e "(async () => { const name = process.argv[1]; const base = 'http://127.0.0.1:61000/undaunted/api'; const r = await fetch(base + '/Register', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ Username: name }), signal: AbortSignal.timeout(15000) }); if (!r.ok) throw new Error('Register returned HTTP ' + r.status); const { UUK } = await r.json(); require('fs').writeFileSync('C:/dr/data/owner.key', UUK); const info = await (await fetch(base + '/GetUserInfo', { headers: { 'x-undaunted-user-api-key': UUK }, signal: AbortSignal.timeout(15000) })).json(); new (require('better-sqlite3'))('C:/dr/data/undaunted.db').prepare('UPDATE users SET isAdmin = 1 WHERE userId = ?').run(info.UserId); console.log('created ' + name + ' as admin; key saved to C:/dr/data/owner.key (not shown)'); })().catch(e => { console.error(e.message); process.exit(1); })" YourName
```

Huomioita:

- Nimen on oltava 3–16 kirjainta, numeroa tai alaviivaa ja ainutlaatuinen kirjainkoosta
  riippumatta. Muuten `Register` vastaa 400 (`username_invalid`) tai 409 (`username_taken`), ja
  skripti pysähtyy. Ylläpitäjä voi myöhemmin vaihtaa tilin nimen reitillä `RenameUser`.
- Ylläpitorajapinta (kutsukoodit, rekisteröintitila, nimenvaihdot, tilastot paikalla olevista
  pelaajista) ottaa tämän avaimen `x-undaunted-user-api-key`-otsakkeessa, ja vain suoraan, ei
  koskaan välityspalvelimen (proxy) kautta. Sivu [Palvelin ryhmälle]({{ admin_page.url | relative_url }})
  kertoo siitä, ja [HTTP-rajapinta]({{ api_page.url | relative_url }}) luettelee jokaisen reitin.
- Pidä `owner.key` poissa repositoriosta, kuvakaappauksista ja keskusteluista. Varmuuskopioi se
  yhdessä kahden `.env`-tiedoston ja `gameserver.key`-tiedoston kanssa, salattuna ja erillään
  tietokannasta.

---

## 11. Deploy-palvelin {#deploy-server}

Deploy-palvelin on ohjelma, joka käynnistää pelipalvelimet ja pitää niitä silmällä.

```powershell
Set-Location C:\dr\undaunted\UndauntedDeployServer
npm ci --no-audit --no-fund
npm run build
$key = (Get-Content C:\dr\data\gameserver.key -Raw).Trim()
@"
PORT=61001
BIND_HOST=127.0.0.1
MY_IP=127.0.0.1
PORT_RANGE_BEGIN=8770
PORT_RANGE_END=8777
GAMESERVER_BINARY_PATH=C:/D144/Dauntless/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe
METAGAME_API_KEY=$key
SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP=10
ENABLE_DOJO=0
NODE_ENV=production
"@ | Set-Content .env -Encoding ascii
$key = $null
```

| Avain | Merkitys |
|---|---|
| `PORT` | 61001. Vain samalla koneella oleva metagame puhuu sille. |
| `BIND_HOST` | **Vain forkissa.** Oletus on `127.0.0.1`. Pidä se siinä. Deploy-palvelimessa ei ole tunnistautumista: kuka tahansa, joka voi kutsua sitä, käynnistää peliprosesseja koneellasi. Toisena tarkistuksena se vastaa 403 jokaiselle kutsujalle, joka ei ole tällä koneella. |
| `MY_IP` | Osoite, joka annetaan peliohjelmille pelipalvelimia varten. Paikallisessa pelissä `127.0.0.1`. |
| `PORT_RANGE_BEGIN`, `PORT_RANGE_END` | Pelipalvelimien UDP-portit. Ramsgate ottaa aina `END`-portin (8777) ja Dojo portin `END-1` (8776). Metsästykset käyttävät loput (8770–8775 eli kuusi kerrallaan). |
| `GAMESERVER_BINARY_PATH` | 1.4.4:n exe-tiedosto, kauttaviivoin. |
| `METAGAME_API_KEY` | Pelipalvelinavain vaiheesta 9. |
| `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP` | Tauko palvelinten käynnistysten välillä. |
| `ENABLE_DOJO` | **Vain forkissa.** `0` käynnistää Training Dojon vasta, kun joku menee sinne ensimmäisen kerran. `1` palauttaa alkuperäisen toiminnan (käynnistys heti alussa). |

Pidä porttijako ennallaan. Palvelin-DLL poistaa käytöstä tyhjäkäyntisulkeutumisensa kaikissa
porteissa **8776 tai yli**. Metsästyspalvelin sulkeutuu, kun siihen ei ole ollut kukaan yhteydessä
yhteensä 50 sekuntiin, ja vain porttien 8776 alapuolella. Jos haluat sallia useamman samanaikaisen
metsästyksen, pienennä `PORT_RANGE_BEGIN`-arvoa ja pidä `PORT_RANGE_END=8777`.

Käynnistä se:

```powershell
$dep = @{ FilePath = "node"; ArgumentList = "--env-file=.env", "dist/server.js"
          WorkingDirectory = "C:\dr\undaunted\UndauntedDeployServer"; PassThru = $true; WindowStyle = "Hidden"
          RedirectStandardOutput = "C:\dr\data\deploy.log"; RedirectStandardError = "C:\dr\data\deploy.err" }
$p = Start-Process @dep; Set-Content C:\dr\data\deploy.pid $p.Id
```

Se käynnistää pysyvän Ramsgate-palvelimen heti. Jokainen sen käynnistämä pelipalvelin on tavallinen
peliprosessi, jolla on oma konsolinsa. Ramsgate ja Dojo näyttävät omansa **konsoli-ikkunana**
(”Running as a server!”). **Älä sulje niitä ikkunoita.** Ikkunan sulkeminen tappaa sen palvelimen
kaikilta, jotka ovat siinä. Metsästyspalvelimet käynnistetään ikkuna piilotettuna, jotta jokaisesta
metsästyksestä ei välähdä uutta ikkunaa.

Jos haluat testata pelipalvelinta yksinään ilman deploy-palvelinta, käynnistä se käsin samoilla
parametreilla, joita deploy-palvelin käyttää:

```powershell
$W = "C:\D144\Dauntless\Archon\Binaries\Win64"
$key = (Get-Content C:\dr\data\gameserver.key -Raw).Trim()
Start-Process "$W\Dauntless-Win64-Shipping.exe" -WorkingDirectory $W -ArgumentList @(
  $key, "8777", "/Game/Maps/ramsgate/ramsgate_01_persistent",
  "NO_BEHEMOTH", "NO_MM_HUNTID", "NO_EXPECTED_PLAYERS", "127.0.0.1:8777",
  "-EpicPortal", "-server", "-nullrhi")
```

Parametrit järjestyksessä ovat: avain, UDP-portti, kenttä (map), behemoth (hirviö), matchmakerin
metsästystunniste, odotetut pelaajat ja peliohjelmille kerrottava osoite. Jos exe-tiedoston nimen
jälkeen on alle kahdeksan parametria, DLL näyttää ”INVALID GAMESERVER ARGS” -ilmoitusikkunan ja
sulkeutuu. Pysäytä käsin käynnistetty palvelin ennen kuin käynnistät deploy-palvelimen, muuten ne
tappelevat portista 8777.

---

## 12. Ensimmäisen käynnistyksen tarkistukset {#first-boot-checks}

Kun metagame ja deploy-palvelin ovat käynnissä:

```powershell
# 1. both node processes listen on loopback only
Get-NetTCPConnection -LocalPort 61000,61001 -State Listen | ForEach-Object { "TCP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
#    expect: TCP 127.0.0.1:61000 node / TCP 127.0.0.1:61001 node

# 2. the metagame answers HTTP (curl.exe ships with Windows 10)
curl.exe -s -m 5 http://127.0.0.1:61000/dauntless-status
#    expect JSON starting {"show-status":true,...

# 3. Ramsgate is up: about 10 s after the deploy server starts, UDP 8777 is bound by the game
Get-NetUDPEndpoint -LocalPort 8777 | ForEach-Object { "UDP {0}:{1} {2}" -f $_.LocalAddress, $_.LocalPort, (Get-Process -Id $_.OwningProcess).ProcessName }
#    expect: UDP 0.0.0.0:8777 Dauntless-Win64-Shipping

# 4. which game processes are servers (the client is the same exe)
Get-CimInstance Win32_Process -Filter "Name='Dauntless-Win64-Shipping.exe'" | ForEach-Object { "{0,6}  server={1}  {2} MB" -f $_.ProcessId, ($_.CommandLine -match ' -server'), [int]($_.WorkingSetSize/1MB) }

# 5. the deploy server log
Get-Content C:\dr\data\deploy.log | ForEach-Object { try { ($_ | ConvertFrom-Json).msg } catch { $_ } }
#    expect: Dauntless Revived deploy server on port 61001 / Clear Skies, Slayer. / Running Gameserver Watchdog! (every 60 s)
```

Ramsgate-palvelimen pitäisi asettua noin 1,1 gigatavuun ja 0,2 suoritinytimeen. Pelipalvelimet
varaavat UDP-porttinsa kaikista verkkoliitännöistä (`0.0.0.0`). Meidän koneellamme Windowsin
palomuuri estää saapuvan liikenteen kaikissa profiileissa, eikä sillä ollut sallivaa sääntöä pelille
tai Nodelle, joten mikään koneen ulkopuolelta ei voinut tavoittaa niitä. Jos olet joskus vastannut
palomuurin kysymykseen ”Salli” näiden ohjelmien kohdalla, tarkista sääntösi. Paikalliseen peliin et
tarvitse sääntöjä lainkaan: liikenne osoitteeseen `127.0.0.1` on loopback-liikennettä, joka kulkee
vain koneen sisällä. Jos Windows kysyy, sallitaanko `node.exe`-ohjelman tai pelin käyttää verkkoja,
voit kieltäytyä, kun kokoonpano on vain paikallinen.

**1.4.4 ei tarvitse hosts-tiedoston merkintöjä, varmenteita eikä muutoksia luotettujen varmenteiden
säilöön.** Jokainen osoite on tavallista HTTP:tä metagameen. (2.1.1-työssämme käytimme
hosts-merkintöjä ja omaa varmentajaa (CA). Mikään siitä ei kuulu tähän kokoonpanoon.) Olemme ajaneet
tätä vain koneella, jossa noita vanhoja hosts-merkintöjä yhä oli. Uskomme, ettei niitä käytetä, mutta
emme ole varmistaneet sitä puhtaalla koneella.

---

## 13. Käynnistä peliohjelma {#launch-the-client}

Käynnistämme pelin skriptillä `C:\dr\tools\play.ps1`. Se kirjoittaa grafiikka- ja chat-asetukset
(vaiheet 7 ja 14), lukee tiliavaimen tiedostosta `C:\dr\data\owner.key` (tai valitsimella
`-KeyFile` annetusta tiedostosta) tulostamatta sitä,
käynnistää pelin ja voi halutessasi seurata sen muistinkäyttöä. Tässä se kokonaisuudessaan:

```powershell
# Launch the 1.4.4 client against our own Dauntless Revived backend.
#   -Graphics 4     FORCE this quality level on every launch (4 = Cinematic = max, 3 = Epic).
#   -Graphics -1    don't force anything; use whatever you pick in the in-game menu.
#   -Windowed       1280x720 window instead of your saved display mode.
#   -KeyFile        account key to log in with (default: the owner's key).
param([string]$Backend = "127.0.0.1:61000", [int]$Graphics = 4, [switch]$Windowed,
      [int]$CapMB = 12000, [int]$Seconds = 0, [string]$KeyFile = "C:\dr\data\owner.key")

$U = "$env:LOCALAPPDATA\Archon\Saved\Config\WindowsClient"

# Engine-level settings. [SystemSettings] in the user Engine.ini overrides the
# menu, so this is where "force" lives. The texture pool is bounded by the GPU's
# VRAM and the two gc lines only affect memory cleanup, not image quality.
$sys = @("[SystemSettings]",
         "r.Streaming.PoolSize=3000", "r.Streaming.LimitPoolSizeToVRAM=1",
         "gc.TimeBetweenPurgingPendingKillObjects=10", "s.ForceGCAfterLevelStreamedOut=1")
$groups = "ViewDistance","AntiAliasing","Shadow","PostProcess","Texture","Effects","Foliage","Shading"
if ($Graphics -ge 0) {
  $sys += ($groups | ForEach-Object { "sg.${_}Quality=$Graphics" })
  $sys += @(
    "sg.ResolutionQuality=100",
    "r.ScreenPercentage=100",      # render at full native resolution
    "r.MipMapLODBias=0",           # full-resolution texture mips
    "r.MaxAnisotropy=16",          # sharp textures on floors and walls at an angle
    "r.Tonemapper.Sharpen=0.6"     # counter temporal-AA softness (the main UE4 blur)
  )
}
# Chat/presence (XMPP): 1.4.4 ships pointed at Epic's live server
# (wss://xmpp-service-prod.ol.epicgames.com:443) and keeps reconnecting to it,
# sending the account id and our login token. Point it at this PC instead.
# With CHAT=1 in the metagame's .env the metagame's chat listens on 61099;
# without it the connection fails harmlessly, exactly as it does against
# Epic (the game tolerates that). URLs MUST be quoted in a user ini.
$xmpp = @("[OnlineSubsystemMcp.XMPP]", 'ServerAddr="ws://127.0.0.1"', "ServerPort=61099", "bUseSSL=false")

$eng = "$U\Engine.ini"
$lines = if (Test-Path $eng) { Get-Content $eng } else { @() }
$keep = New-Object System.Collections.Generic.List[string]; $skip = $false
foreach ($l in $lines) {
  if ($l -match '^\[(SystemSettings|OnlineSubsystemMcp\.XMPP)\]') { $skip = $true; continue }
  if ($skip -and $l -match '^\[') { $skip = $false }
  if (-not $skip) { $keep.Add($l) }
}
Set-Content $eng -Encoding ASCII -Value ($sys + "" + $xmpp + "" + $keep)

# Mirror the level into the menu settings so the options screen shows it too.
$gus = "$U\GameUserSettings.ini"
if ($Graphics -ge 0 -and (Test-Path $gus)) {
  $g = Get-Content $gus
  foreach ($k in $groups) { $g = $g -replace "^sg\.${k}Quality=.*", "sg.${k}Quality=$Graphics" }
  $g = $g -replace '^sg\.ResolutionQuality=.*', 'sg.ResolutionQuality=100.000000'
  Set-Content $gus -Encoding ASCII -Value $g
}
if ($Graphics -ge 0) { "graphics FORCED to level $Graphics (4 = Cinematic/max), native res, sharpened" }
else { "graphics: using your in-game menu choice" }

$W   = "C:\D144\Dauntless\Archon\Binaries\Win64"
$UUK = (Get-Content $KeyFile -Raw).Trim()      # your account key; never printed
$a = @($Backend, "-AUTH_PASSWORD=$UUK", "-AUTH_LOGIN=unused", "-AUTH_TYPE=exchangecode",
  "-epicapp=appidlol", "-epicenv=Prod", "-EpicPortal", "-epicusername=usernamelol",
  "-epicuserid=useridlol", "-epiclocale=en-US", "-epicsandboxid=sandboxidlol",
  "-epicdeploymentid=deploymentidlol")
if ($Windowed) { $a += @("-windowed", "-ResX=1280", "-ResY=720") }
$cl = Start-Process "$W\Dauntless-Win64-Shipping.exe" -WorkingDirectory $W -PassThru -ArgumentList $a
Set-Content C:\dr\data\client.pid -Value $cl.Id
"client pid=$($cl.Id) backend=$Backend"
if ($Seconds -le 0) { return }

$peak = 0; $t = 0
while ($t -lt $Seconds) {
  Start-Sleep -Seconds 5; $t += 5
  $p = Get-Process -Id $cl.Id -ErrorAction SilentlyContinue
  if (-not $p) { "client EXITED at ${t}s (peak ${peak}MB)"; return }
  $mb = [int]($p.WorkingSet64 / 1MB); if ($mb -gt $peak) { $peak = $mb }
  if ($mb -gt $CapMB) { "CAP HIT at ${t}s: ${mb}MB - killing client"; Stop-Process -Id $cl.Id -Force; return }
  if ($t % 30 -eq 0) { "  t=${t}s  client RAM ${mb}MB" }
}
"client running, peak ${peak}MB over ${Seconds}s"
```

Aja se muuttamatta koneen suorituskäytäntöä (execution policy):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\play.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\play.ps1 -Graphics 3 -Windowed -Seconds 180   # Epic, windowed, watch RAM for 3 min
```

Mitä parametrit tarkoittavat:

- **Ensimmäisen parametrin** (`127.0.0.1:61000`, ilman protokollaa) client-tilan DLL lukee
  metagamen osoitteeksi.
- `-AUTH_TYPE=exchangecode` yhdessä parametrin `-AUTH_PASSWORD=<account key>` kanssa saa pelin
  lähettämään avaimen osoitteeseen `POST /account/api/oauth/token` vaihtokoodina (exchange code).
  Metagame tarkistaa sen ja palauttaa 24 tuntia voimassa olevan kirjautumistunnisteen.
- `...lol`-arvot ovat paikanpitäjiä. Undauntedin käynnistin antaa juuri nämä. Epic-tiliä ei tarvita,
  koska 1.4.4 on Epic Online Servicesiä vanhempi.
- Liikenne on tavallista, salaamatonta HTTP:tä, ja avain näkyy prosessin komentorivillä ohjelmille,
  joita ajetaan sinun käyttäjänäsi. Koneen sisällä (loopback) se on kunnossa. Verkon yli käytä
  VPN:ää eli salattua yhteyttä ([Palvelin ryhmälle]({{ admin_page.url | relative_url }})).

**Mitä metagamen lokissa pitäisi näkyä** (`gs=0` on peliohjelma, `gs=1` pelipalvelin). Järjestyksessä,
lyhennettynä:

```
POST /account/api/oauth/token gs=0      -> Logging in <account id>!
GET  /dauntless-status gs=0
POST /login gs=0                        -> <account id> is logging in!
PUT  /gamesession/epic gs=0
GET  /accountinfo gs=0
GET  /character gs=0                    (the first character is created automatically)
POST /candidate/player/register gs=0
GET  /candidate/regions gs=0, then several GET /QoS
POST /candidate/join gs=0               -> Querying DeployServer for GameMode: ISLAND ... (new character's tutorial)
                                           or GameMode: CITY ... (Ramsgate)
GET /character gs=1, ..., POST /character gs=1    (the game server loading and saving you)
```

Uusi hahmo menee opetussaarelle (`/Game/Maps/islands/1705/dia_moss_triforce`) metsästyspalvelimelle,
jonka deploy-palvelin käynnistää (meidän koneellamme UDP 8775), ja sieltä Ramsgateen porttiin 8777.
Peliohjelma avaa myös konsoli-ikkunan. Jätä se auki.

Jotkin varoitukset tässä lokissa ovat odotettuja. Katso
[Vianetsintä]({{ trouble_page.url | relative_url }}#log-lines-that-look-alarming-but-are-known).

---

## 14. Grafiikka {#graphics}

`play.ps1 -Graphics <n>` pakottaa yhden Unrealin skaalautuvuustason (laatutason) jokaisella
käynnistyksellä:

| `-Graphics` | Taso |
|---|---|
| `4` (oletus) | Cinematic, tämän UE4-version korkein taso |
| `3` | Epic |
| `2` | High |
| `1` | Medium |
| `0` | Low |
| `-1` | Älä pakota mitään; käytä sitä, mitä valitset pelin valikosta |

Millä tahansa tasolla 0 tai yli skripti pakottaa myös natiiviresoluution (`r.ScreenPercentage=100`),
täyden resoluution mipit, 16x anisotrooppisen suodatuksen ja kevyen terävöinnin. Se kopioi tason myös
`GameUserSettings.ini`-tiedostoon, jotta asetusvalikko näyttää saman. `-Windowed` antaa
1280x720-kokoisen ikkunan.

Cinematic-tasolla ja resoluutiolla 1920x1080 RX 6600 -luokan näytönohjaimella peliohjelma käyttää
1,9–2,3 Gt RAM-muistia. `[SystemSettings]`-osion muistirivit pysyvät päällä jokaisella tasolla
varotoimena. Ne ovat peräisin 2.1.1-työstämme, jossa rajoittamaton peliohjelma nousi 9 gigatavuun.
Emme ole mitanneet 1.4.4:ää ilman niitä. Katso
[Vianetsintä]({{ trouble_page.url | relative_url }}#memory-spikes-and-caps).

---

## 15. Pysäyttäminen {#stopping}

Järjestyksellä on väliä:

1. Deploy-palvelimen vahtikoira (watchdog) käynnistää kuolleen Ramsgaten uudelleen noin minuutissa,
   ja niin tekee myös pelaajan matka Ramsgateen, joten pysäytä deploy-palvelin ennen pelipalvelimia.
2. Sen käynnistämät pelipalvelimet päättyvät yleensä sen mukana. Se käyttää Noden oletusarvoista
   (ei irrotettua eli not detached) `spawn`-kutsua, ja Windowsissa Node laittaa tällaiset
   lapsiprosessit työobjektiin (job object), joka suljetaan, kun Node sulkeutuu. Varmistimme tämän
   testiprosessilla sekä normaalissa sulkeutumisessa että pakotetussa lopetuksessa. Käsin
   käynnistetty palvelin (vaihe 11) ei kuulu tähän ja jää käyntiin, joten alla oleva komento siivoaa
   silti jäljelle jääneet `-server`-prosessit.
3. Peliohjelma on sama exe-tiedosto kuin palvelimet, joten pysäytä vain prosessit, joiden
   komentorivillä on `-server`. Älä tulosta niiden komentorivejä: ensimmäinen parametri on
   pelipalvelinavain.

```powershell
# 1. quit the game from its menu, then:
Stop-Process -Id (Get-Content C:\dr\data\deploy.pid)
Get-CimInstance Win32_Process -Filter "Name='Dauntless-Win64-Shipping.exe'" |
  Where-Object { $_.CommandLine -match ' -server' } | ForEach-Object { Stop-Process -Id $_.ProcessId }
Stop-Process -Id (Get-Content C:\dr\data\metagame.pid)
```

Kun kaikki on pysäytetty, kopioi `C:\dr\data\undaunted.db` turvaan. Siinä on jokainen tili ja hahmo.
Sivulla [Palvelin ryhmälle]({{ admin_page.url | relative_url }}) on kunnollinen
varmuuskopiointirutiini.

---

## Käynnistä kaikki: yhden sivun tarkistuslista {#checklist}

Kun vaiheet 1–12 on tehty, tämä on koko rutiini.

| # | Tee | Tarkista |
|---|---|---|
| 1 | Sulje raskaat ohjelmat. Jos käytät WSL:ää, aja `wsl --shutdown`. | Useita gigatavuja RAM-muistia vapaana |
| 2 | Varmista, ettei mitään vanhaa ole käynnissä: ei `node`-prosessia porteissa 61000/61001, ei `-server`-peliprosesseja. | Vaiheen 12 komennot 1 ja 4 eivät näytä mitään |
| 3 | `Game.ini` on ehjä. | Vaiheen 8 kuntotarkistus: `167 / 0 / 0` |
| 4 | Käynnistä metagame kansiosta `C:\dr\undaunted\UndauntedMetagame` (`Start-Process @meta`, vaihe 9). | Loki: `Dauntless Revived metagame on 127.0.0.1:61000` |
| 5 | Käynnistä deploy-palvelin kansiosta `C:\dr\undaunted\UndauntedDeployServer` (`Start-Process @dep`, vaihe 11). | Loki: `Dauntless Revived deploy server on port 61001` |
| 6 | Odota Ramsgatea. | Palvelimen konsoli-ikkuna aukeaa; UDP 8777 on prosessin `Dauntless-Win64-Shipping` varaama |
| 7 | `powershell -NoProfile -ExecutionPolicy Bypass -File C:\dr\tools\play.ps1` | Metagamen lokissa näkyy `POST /account/api/oauth/token` ja sitten `POST /login` |
| 8 | Pelaa. Älä sulje yhtään konsoli-ikkunaa. | `gs=1`-rivejä ilmestyy, kun palvelin lataa sinut |
| 9 | Pysäytä järjestyksessä: peli, deploy-palvelin, `-server`-prosessit, metagame. | Vaihe 15 |
| 10 | Varmuuskopioi `C:\dr\data\undaunted.db`. | Päivätty kopio on olemassa |

Jos jokin ei täsmää, katso [Vianetsintä]({{ trouble_page.url | relative_url }}).
