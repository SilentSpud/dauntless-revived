---
title: Pelitiedostojen tarkistaminen
parent: Löydökset
grand_parent: Dauntless Revived suomeksi
nav_order: 1
lang: fi
ref: findings/verification
locale: fi_FI
description: "Miten varmistimme, että yhteisön arkistoimat Dauntless-versiot 2.1.1 ja 1.4.4 ovat aitoja, ehjiä ja puhtaita: allekirjoitukset, manifestit ja virustarkistus."
---

{% assign ci_page = site.pages | where: "path", "fi/findings/client-internals.md" | first %}
{% assign assets_page = site.pages | where: "path", "fi/findings/assets.md" | first %}
{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}

# Pelitiedostojen tarkistaminen
{: .no_toc }

Dauntlessin viralliset palvelimet suljettiin 30. toukokuuta 2025, eikä peliä voi enää ladata mistään
virallisesta lähteestä. Jokainen kopio, jonka kanssa olemme työskennelleet, on peräisin
pelaajayhteisön jakamasta arkistosta (yhdestä suuresta pakatusta tiedostosta, jonka sisällä koko peli
on). Ennen kuin ajoimme niistä mitään, halusimme vastauksen kolmeen kysymykseen:

1. **Aito:** ovatko ohjelmatiedostot (tiedostot, jotka tietokone suorittaa, kuten `.exe`)
   täsmälleen samat, jotka Phoenix Labs julkaisi?
2. **Täydellinen:** onko version jokainen tiedosto mukana ja ehjä?
3. **Puhdas:** onko arkistossa jotain, minkä ei kuuluisi olla siellä?

Tällä sivulla kerrotaan, miten vastasimme näihin kysymyksiin kahden peliversion kohdalla. Kummastakaan
arkistosta ei ajettu mitään ennen kuin kaikki tarkistukset olivat menneet läpi.

| Versio | Mitä saimme | Tulos |
|:------|:-----------------|:-------|
| **2.1.1** (viimeinen "Awakening"-versio, UE5, IoStore). Koontiversio 682486, allekirjoitettu 18.12.2024 | RAR5-arkisto, 13,53 GiB, 833 tiedostoa, purettuna 15,52 GB | Aito, täydellinen, puhdas |
| **1.4.4** (UE4, pak v9). `dauntless_rel-1.4.4_Shipping_2020-10-28`, allekirjoitettu 29.10.2020 | `BaseGame144.zip`, 10,48 GB, 410 tiedostoa (471 zip-merkintää, kun mukaan lasketaan 61 kansiomerkintää). Juuri tämän arkiston alkuperäisen [Undaunted](https://github.com/SyST3MDeV/Undaunted)-projektin käynnistin lataa | Aito, täydellinen, puhdas |

<details open markdown="block">
  <summary>Sisällys</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Perussäännöt {#ground-rules}

- **Älä koskaan aja mitään tarkistamattomasta arkistosta.** Tämä koskee myös asennusohjelmia,
  käynnistimiä ja itse peliä.
- **Tee tutkiminen Linuxissa.** Käytimme Ubuntua WSL2:n alla (WSL2 on Windowsin sisällä toimiva
  Linux-ympäristö). Windowsin `.exe`- tai `.dll`-tiedosto ei voi käynnistyä siellä vahingossa. Tämä
  suojaa vahingossa ajamiselta, mutta se ei ole tietoturvaraja: WSL2 pääsee oletuksena käsiksi
  Windowsin levyihin polun `/mnt/c` kautta. Erillinen virtuaalikone eristää paremmin.
- **Puretuilta tiedostoilta poistetaan suoritusoikeus** (`chmod -R a-x+X <dir>`).
- **Tarkista ensin koodi, vasta sitten muu massa.** Versiosta 2.1.1 purimme ensin vain `.exe`- ja
  `.dll`-tiedostot (75 tiedostoa, noin 490 MiB) ja tarkistimme niiden allekirjoitukset. Vasta sen
  jälkeen purimme loput.

Käyttämämme työkalut:

| Työkalu | Versio | Käyttötarkoitus |
|:-----|:--------|:---------|
| `osslsigncode` | 2.13 (OpenSSL 3.5) | Authenticode-allekirjoitusten tarkistus Linuxissa |
| ClamAV (`clamscan`, `freshclam`) | 1.5.3, päivittäiset tunnisteet 28129 (20.9.2026) | Haittaohjelmatarkistus |
| `unrar` | 7.20 | RAR5-arkiston purku (2.1.1) |
| Python 3 `zipfile`, `hashlib`, `pefile` | Ubuntun paketit | Zip-purku, manifestin tiivisteet, PE-tiedostojen (Windows-ohjelmatiedostojen) tutkiminen |

---

## Turvallinen purkaminen {#safe-extraction}

### Zip-arkistot (1.4.4) {#zip-archives-144}

Purimme Pythonin `zipfile`-kirjastolla emmekä käyttäneet graafista työkalua. Arkiston sisällä olevat
tiedostonimet ratkaisevat, minne tiedostot kirjoitetaan. Tarkoituksella muotoiltu nimi, kuten
`../../somewhere/evil.dll`, voi kirjoittaa kohdekansion ulkopuolelle. Tätä hyökkäystä kutsutaan
nimellä zip-slip. Purkajamme selvittää jokaisen kohdepolun ja kieltäytyy kaikesta, mikä osuisi
kohdekansion ulkopuolelle:

```python
import zipfile, os, shutil, sys

z = zipfile.ZipFile(sys.argv[1])
out = os.path.realpath(sys.argv[2])
for i in z.infolist():
    dest = os.path.realpath(os.path.join(out, i.filename))
    if not dest.startswith(out + os.sep):          # zip-slip guard
        print("REFUSED path escaping target:", i.filename)
        continue
    if i.is_dir():
        os.makedirs(dest, exist_ok=True)
        continue
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with z.open(i) as src, open(dest, "wb") as dst:
        shutil.copyfileobj(src, dst, 1 << 22)
```

**CRC-tarkistukset tulevat kaupan päälle.** Jokaiseen zip-merkintään on tallennettu pakkaamattoman
datan CRC-32-tarkistussumma. Kun `zipfile` lukee merkinnän loppuun, se vertaa tarkistussummaa ja
nostaa virheen `BadZipFile`, jos summat eroavat. Purku, joka päättyy ilman poikkeusta, on siis
tarkistanut myös jokaisen merkinnän CRC:n. Kaikki `BaseGame144.zip`-arkiston 410 tiedostoa
purkautuivat moitteetta, eikä yhtään merkintää hylätty.

CRC todistaa vain, ettei arkisto vioittunut siirron aikana. Se ei kerro mitään siitä, kuka tiedostot
on tehnyt. Siihen vastaavat myöhemmät tarkistukset.

### RAR5-arkistot (2.1.1) {#rar5-archives-211}

- Ubuntun `7zip`-paketti (7-Zip 26.00, paketoitu ilman ei-vapaata RAR-purkajaa) osaa listata RAR5-arkiston
  sisällön mutta ei pura sitä. Jokainen kokeilemamme tiedosto epäonnistui virheellä
  `Unsupported Method`. Asensimme sen sijaan `unrar`-ohjelman Ubuntun multiverse-ohjelmistolähteestä.
- Tarkistimme `unrar`-version: se on 7.20. CVE-2022-30333 on `unrar`-ohjelman polunläpikulkuvirhe
  (path traversal) Linuxissa, ja se korjattiin versiossa 6.12.
- Luimme arkiston sisällysluettelon ennen purkamista. Siinä ei ollut absoluuttisia polkuja eikä
  `..`-osia.
- `unrar` tarkistaa purkaessaan jokaisen tiedoston tallennetun tarkistussumman ja päättyy nollasta
  poikkeavaan paluukoodiin, jos summa ei täsmää. Meidän purkumme päättyi paluukoodiin 0, ja kaikki
  833 tiedostoa olivat sen jälkeen paikallaan.

---

## Authenticode-allekirjoitukset ja osslsigncode {#authenticode-signatures-with-osslsigncode}

Phoenix Labs allekirjoitti Windows-ohjelmatiedostonsa Authenticodella (Microsoftin tapa liittää
ohjelmaan digitaalinen allekirjoitus, joka kertoo, kuka ohjelman teki). Tämä on vahvin yksittäinen
tarkistuksemme, koska se perustuu salaustekniikkaan. `osslsigncode` tarkistaa
Authenticode-allekirjoitukset Linuxissa:

```bash
osslsigncode verify -CAfile /etc/ssl/certs/ca-certificates.crt -in Dauntless-Win64-Shipping.exe
```

Ensimmäiseksi kannattaa lukea nämä kaksi riviä:

```text
Current message digest    : B15C6A3F8900BD367F049260D9E0A005531C54F2C26C668CC16F54A3A67BD0E4
Calculated message digest : B15C6A3F8900BD367F049260D9E0A005531C54F2C26C668CC16F54A3A67BD0E4
```

- **Current** on allekirjoituksen sisään tallennettu tiiviste (tiedoston sisällöstä laskettu
  "sormenjälki"). Allekirjoittaja laski sen, kun tiedosto allekirjoitettiin.
- **Calculated** on tiiviste, jonka `osslsigncode` laskee tiedostosta sellaisena kuin se nyt on.

Jos ne täsmäävät, tiedoston allekirjoitetut osat eivät ole muuttuneet tavuakaan allekirjoittamisen
jälkeen. Jos ne eroavat, tiedostoa on muutettu jälkikäteen. Tulosteen loppuosa kertoo, **kuka**
tiedoston allekirjoitti: allekirjoittajan varmenne, ketju luotettuun juurivarmenteeseen asti ja
aikaleiman vastatallekirjoitus.

### 2.1.1: jokainen allekirjoitus on kunnossa

Kaikki **75/75** ohjelmatiedostoa läpäisivät tarkistuksen tuloksella `Signature verification: ok`.
Joukossa on 7 ohjelmaa ja 68 DLL-tiedostoa (ohjelmakirjastoa), eikä yksikään epäonnistunut tai ollut
allekirjoittamaton. Pääohjelman tiedot:

| Tarkistus | Tulos |
|:------|:-------|
| Tiiviste | Tallennettu ja laskettu tiiviste täsmäävät |
| Allekirjoittaja | `CN=Phoenix Labs Canada ULC`, Burnaby, British Columbia |
| Myöntäjä | DigiCert Trusted G4 Code Signing RSA4096 SHA384 2021 CA1 |
| Ketju | Varmistettu juurivarmenteeseen DigiCert Trusted Root G4 asti |
| Mitätöinti | Sulkulista (CRL) haettu DigiCertiltä; varmennetta ei ole mitätöity |
| Aikaleima | 18.12.2024 klo 02:03:21 GMT, vastatallekirjoittajana DigiCertin aikaleimavarmentaja |

Allekirjoitusvarmenne vanheni lokakuussa 2025. Sillä ei ole merkitystä. Aikaleima todistaa, että
allekirjoitus tehtiin varmenteen ollessa voimassa, ja juuri niin koodin allekirjoittamisen on
tarkoitus toimia.

Version 2.1.1 arkistossa on täsmälleen seitsemän ohjelmaa: `Dauntless-Win64-Shipping.exe`,
`Dauntless.exe`, `start_protected_game.exe`, `EasyAntiCheat_EOS_Setup.exe`, `CrashReportClient.exe`,
`EpicWebHelper.exe` ja `UEPrereqSetup_x64.exe`. Kaikki seitsemän on Phoenix Labsin allekirjoittamia.
EAC:n (EasyAntiCheat, huijauksenestojärjestelmä) asennusohjelma on tässä tärkein, koska se asentaa
Windows-palvelun, joka toimii järjestelmäoikeuksin.

### 1.4.4: miksi "FAILED" voi silti tarkoittaa koskematonta

Versiossa 1.4.4 `osslsigncode` ilmoitti **FAILED jokaisen 39 ohjelmatiedoston kohdalla**. Joukossa
olivat myös Microsoftin omat `dbghelp.dll` ja `d3dcompiler_47.dll`. Oikeasti peukaloitukin tiedosto
epäonnistuu, joten pelkkä epäonnistuminen ei kerro meille mitään. Pääohjelman koko tuloste näyttää,
mistä oikeasti on kyse:

```text
Current message digest    : D71ACDA54152B287C361B23B9D60AC45B590982C502D4648EA6600CD618E15AD
Calculated message digest : D71ACDA54152B287C361B23B9D60AC45B590982C502D4648EA6600CD618E15AD
  Subject: CN=Phoenix Labs Canada ULC ...   Issuer: CN=thawte SHA256 Code Signing CA
Countersignatures:
  Timestamp time: Oct 29 04:23:10 2020 GMT      Hash Algorithm: sha1
  Issuer: CN=Symantec Time Stamping Services CA - G2
Timestamp verified using:
  Subject: CN=Symantec Time Stamping Services CA - G2
  Issuer : CN=Thawte Timestamping CA ...
  Error: unable to get local issuer certificate
Timestamp Server Signature verification: failed
Signing certificate chain verified using:
  Subject: CN=thawte SHA256 Code Signing CA
  Issuer : CN=thawte Primary Root CA ...
  Error: unable to get local issuer certificate
Signature verification: failed
```

Tiivisteet täsmäävät. Molemmat virheet ovat **ketjuvirheitä**, ja molemmilla on sama syy:
juurivarmenne, jota CA-kokoelmassa (luotettujen varmentajien luettelossa) ei ole.

- Vuoden 2020 aikaleima myönnettiin varmenteella `Symantec Time Stamping Services CA - G2` (SHA-1),
  joka ketjuuntuu vanhaan juurivarmenteeseen `Thawte Timestamping CA`.
- Phoenixin vuoden 2019 koodinallekirjoitusvarmenteen myönsi `thawte SHA256 Code Signing CA`, joka
  ketjuuntuu juureen `thawte Primary Root CA`.

Molemmat juuret kuuluvat käytöstä poistettuun thawte/Symantec-hierarkiaan. Nykyisen Ubuntun
CA-kokoelmassa ei ole ainuttakaan thawte-varmennetta (tarkistimme), joten OpenSSL ei pysty viemään
kumpaakaan ketjua loppuun ja ilmoittaa koko tarkistuksen epäonnistuneen. Vika on meidän
tarkistusympäristössämme. Se ei kerro tiedostosta mitään.

Koska kokonaistuomio oli käyttökelvoton, vertasimme jokaisen ohjelmatiedoston tiivisteitä:

```bash
find . -type f \( -iname '*.exe' -o -iname '*.dll' \) | sort | while read -r f; do
  out=$(osslsigncode verify -in "$f" 2>&1)
  cur=$(echo "$out" | grep -m1 'Current message digest'    | awk '{print $NF}')
  cal=$(echo "$out" | grep -m1 'Calculated message digest' | awk '{print $NF}')
  if   [ -z "$cur" ];       then echo "UNSIGNED  $f"
  elif [ "$cur" = "$cal" ]; then echo "intact    $f"
  else                           echo "MODIFIED  $f"; fi
done
```

Tulos: **39 koskematonta, 0 muutettua, 0 allekirjoittamatonta.**

| Allekirjoittaja | Tiedostoja | Aikaleimat |
|:-------|------:|:-----------|
| Phoenix Labs Canada ULC | 22 | 29.10.2020 klo 04:23:10–04:23:19 GMT: yksi noin yhdeksän sekunnin allekirjoituskerta, päivä `Version.txt`-tiedostossa mainitun koontipäivän jälkeen |
| Microsoft Corporation | 10 | 2009–2020, valmistajan omat päivämäärät jaettaville DLL-tiedostoille |
| EasyAntiCheat Oy | 4 | 7.9.2020 |
| Mercer Road Corp (Vivox) | 2 | 8.1.2020 |
| Overwolf Ltd | 1 | 15.8.2017 |

Versiossa 1.4.4 `Dauntless.exe` on EasyAntiCheatin käynnistysohjelma, joten sen allekirjoitti
EasyAntiCheat Oy eikä Phoenix. Selvitimme FAILED-tuloksen syyn vain pääohjelmasta. Muista 38
tiedostosta tarkistimme vain, että tiivisteet täsmäävät. Niiden epäonnistumiset johtuvat oletettavasti
samanlaisista vanhoista ketjuista, mutta sitä emme ole varmistaneet.

Näin `osslsigncode`-tulostetta luetaan:

| Tuloste | Merkitys |
|:-------|:--------|
| `Signature verification: ok` | Muuttamaton, ja ketju johtaa luotettuun juureen |
| FAILED, tiivisteet **täsmäävät**, ketjuvirhe | Muuttamaton. Ketju epäonnistuu vain sinun ympäristössäsi, esimerkiksi käytöstä poistetun juuren takia |
| FAILED, tiivisteet **eroavat** | Muutettu allekirjoittamisen jälkeen. Älä aja sitä |
| Ei `Current message digest` -riviä | Allekirjoittamaton. Ei julkaistu ohjelmatiedosto, ellei valmistaja toimittanut sitä allekirjoittamattomana |

Mitä tiivisteiden täsmääminen ei yksinään todista: että tiedostoon upotettu varmenne kuuluu sille,
jonka se väittää. Version 1.4.4 kohdalla hyväksymme sen neljästä syystä. Allekirjoittaja on Phoenixin
thawten myöntämä varmenne, joka oli voimassa syyskuusta 2019 lokakuuhun 2022. Pääohjelmassa
`osslsigncode` ilmoittaa ainoina virheinä kaksi puuttuvaa juurta. Phoenixin 22 tiedostoa muodostavat
yhden yhtenäisen allekirjoituskerran, ja jokaisessa kolmannen osapuolen tiedostossa on sen oman
valmistajan allekirjoitus. Lisäksi alla kuvatut manifesti- ja tiivistetarkistukset ovat samaa mieltä.
Emme kokeilleet lisätä kahta käytöstä poistettua thawte-juurta CA-tiedostoon puhtaan `ok`-tuloksen
saamiseksi, joten emme tiedä, toimisiko se.

---

## Täydellisyys: Phoenixin asennusmanifesti (1.4.4) {#completeness-phoenixs-install-manifest-144}

Version 1.4.4 juurikansiossa on tiedostot `Manifest.bin` ja `Manifest.bin.json`. Tämä on Phoenixin
oma asennusmanifesti (luettelo kaikista asennukseen kuuluvista tiedostoista), ei Unrealin tai Epic
Games Storen tiedosto. JSON-muoto on helppolukuinen:

```text
{ "TargetFiles": [ ...406 entries... ], "DeployedPaths": ["**"] }

entry: { "RelativePath": "Archon\\Binaries\\Win64\\dbgcore.dll",
         "FileSize": 166720,
         "IsSelfSource": true,
         "MD5Chunks": "185bfb1bfb9315633cfaa60e2473772745" }
```

`MD5Chunks` on kaksimerkkinen etuliite, aina `18`, jota seuraa yksi 32-heksanumeroinen MD5 tiedoston
jokaista palaa kohden. Manifesti ei kerro palan kokoa. Kokeilimme kaikkia kahden potensseja väliltä
2^16–2^26. Vain **2^24 tavua (16 MiB)** antaa jokaiselle 406 tiedostolle tiivisteiden määrän, joka on
yhtä suuri kuin `ceil(FileSize / chunk)`. Heksaluku `0x18` on 24, joten etuliite luultavasti ilmaisee
juuri tämän eksponentin. Tätä ei ole vahvistettu, mutta se täsmää.

Itse tarkistus laskee jokaisen tiedoston tiivisteet 16 MiB:n paloissa ja vertaa luetteloa:

```python
import json, hashlib, os

d = json.load(open("Manifest.bin.json", encoding="utf-8-sig"))
CHUNK = 1 << 24
for e in d["TargetFiles"]:
    rel = e["RelativePath"].replace(chr(92), "/")          # backslashes -> slashes
    want = e["MD5Chunks"][2:]                              # drop the "18" prefix
    want = [want[i:i+32] for i in range(0, len(want), 32)]
    got = []
    with open(rel, "rb") as f:
        while (b := f.read(CHUNK)):
            got.append(hashlib.md5(b).hexdigest())
    got = got or [hashlib.md5(b"").hexdigest()]
    assert os.path.getsize(rel) == e["FileSize"] and got == want, rel
```

**Tulos: 406/406 tiedostoa täsmää. Yksikään ei poikennut, yhdenkään koko ei ollut väärä eikä
yksikään puuttunut.** Ainoat levyllä olevat tiedostot, joita manifesti ei luettele, ovat
`Manifest.bin` ja `Manifest.bin.json` itse sekä kaksi tyhjää `debug.log`-tiedostoa, yksi kummassakin
`Binaries\Win64`-kansiossa.

Manifesti kulkee samassa arkistossa, joten yksinään se todistaa yhtenäisyyden, ei alkuperää. Joku,
joka on muuttanut pak-tiedostoa, voisi luoda manifestin uudelleen vastaamaan muutosta. Sen arvo syntyy
yhdessä allekirjoitustarkistuksen kanssa: pak-tiedostoja lukeva koodi on todistetusti aito, ja koko
asennus vastaa Phoenixin omaa tiedostoluetteloa.

Kaksi lisätarkistusta versiolle 1.4.4:

- Pääohjelman ja zip-arkiston SHA-256-tiivisteet ovat samat kuin alkuperäisen Undaunted-käynnistimen
  kiinnittämät arvot (`legacy/UndauntedLauncher/src/main.ts`):

  | Tiedosto | SHA-256 |
  |:-----|:--------|
  | `Dauntless-Win64-Shipping.exe` (103 673 520 tavua) | `D3D41E614908D2BEFD518B27046D9822D6130EF12BA3504BABBDB786BEF9CFF4` |
  | `BaseGame144.zip` | `556B9A648A5E5E7E11B6F8DD3D80FF8E88FCEB0D3448297AAF47CE7BF756BC6D` |

- `EasyAntiCheat/Certificates/game.cer` on EasyAntiCheatin vakiovarmenne. Asennukseen ei ollut
  ujutettu ylimääräistä CA-varmennetta.

## Täydellisyys: Unrealin paketointimanifestit (2.1.1) {#completeness-unreals-staging-manifests-211}

Version 2.1.1 kopio oli peräisin Epic Games Store -asennuksesta. Siinä ei ole
`Manifest.bin.json`-tiedostoa, mutta Unrealin paketointivaihe jättää juurikansioon kolme manifestia:

| Manifesti | Luettelee | Paikalla |
|:---------|:------|:--------|
| `Manifest_NonUFSFiles_Win64.txt` | 352 irrallista tiedostoa: ohjelmatiedostot, EasyAntiCheat, CEF ja muut säiliöiden ulkopuoliset tiedostot | **352/352** |
| `Manifest_DebugFiles_Win64.txt` | 6 virheenjäljitystiedostoa | 5. Vain `Dauntless-Win64-Shipping.pdb` puuttuu, kuten julkaisuasennuksessa kuuluukin |
| `Manifest_UFSFiles_Win64.txt` | 152 035 paketoidun sisällön polkua | Nämä ovat säiliöiden sisällä, eikä niitä voi tarkistaa tiedosto kerrallaan |

Nämä manifestit luettelevat polkuja ja aikaleimoja, eivät tiivisteitä. Ne osoittavat, että asennus on
täydellinen, mutta eivät sitä, että sen sisältö on ehjä. Säiliöistä (IoStore-tiedostoista, joihin
pelin sisältö on pakattu) tarkistimme rakenteen:

- Kaikissa 157 `.utoc`-tiedostossa on IoStoren tunnistetavut `-==--==--==--==-` ja TOC-versio 5.
- Jokaisella `.utoc`-tiedostolla on `.ucas`-parinsa ja jokaisella `.ucas`-tiedostolla `.utoc`-parinsa:
  kumpiakin 157, eikä yhtään orpoa. Lisäksi on 157 Unrealin `.pak`-tiedostoa. Niistä 156 on
  säiliöiden vieressä, koska `global`-säiliöllä ei ole `.pak`-tiedostoa, ja viimeinen on
  `CrashReportClient.pak`. Jokainen päättyy pak-alatunnisteen tunnistetavuihin `0x5A6F12E1`.
- Numeroinnissa ei ole aukkoja yhdessäkään 18 säiliöryhmästä (`Archon_0..50`,
  `Archon_ArmourC_0..40`, `Archon_UI_0..11` ja muut). Puuttuva säiliö jättäisi aukon.
- **Korjaus:** ensimmäinen alatunnistetarkistuksemme merkitsi 56 `.pak`-tiedostoa virheellisiksi.
  Kaikki 56 ovat kansiossa `Engine/Binaries/ThirdParty/CEF3/.../Resources`, ja ne ovat Chromiumin
  resurssipakkauksia: eri muoto, jolla on sama tiedostopääte. Jokaisella on kelvollinen
  Chromium-pak-otsake (versio 5). Vika oli tarkistuksessa, ei tiedostoissa.
- 187 `.chroma`-tiedostoa (Razer Chroma -valotehosteita) on lueteltu irrallisten tiedostojen
  manifestissa, ja kaikilla on sama otsake.

Säiliöitä lukuun ottamatta levyllä on vain kahdeksan tiedostoa, joita ei mainita missään manifestissa,
ja kaikki kahdeksan ovat odotettuja: Epic Games Storen asennustietue (`.egstore/<id>.manifest`), kolme
asennuksen aikana kirjoitettua EasyAntiCheatin ajonaikaista tiedostoa (`base.bin`, `base.cer`,
`runtime.conf`), kolme manifestitiedostoa itse sekä kaatumisraportoijan oma `CrashReportClient.pak`.
Arkistossa ei ole skriptejä (`.bat`, `.cmd`, `.ps1`, `.vbs`, `.scr`, `.lnk`, `.hta`). EasyAntiCheatin
`Settings.json` osoittaa julkaistuun pääohjelmaan
`Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe`.

---

## ClamAV ja 2 GiB:n ansa {#clamav-and-the-2-gib-trap}

Virustarkistus on näistä tarkistuksista heikoin. Puhdas tulos tarkoittaa vain, ettei mikään tunnettu
haittaohjelman tunniste osunut. Teimme sen silti, koska sisältötiedostojen allekirjoituksia ei voi
tarkistaa, mutta virustarkistus kattaa nekin.

### Ansa {#the-trap}

Ensimmäinen yrityksemme versiolla 2.1.1 tarkisti RAR-arkiston suoraan ja "meni läpi":

```text
LibClamAV Warning: Max file-size was set to 4293918720 bytes. Unfortunately, scanning files
greater than 2147483647 bytes (2 GiB - 1) is not supported.
Scanned files: 1
Infected files: 0
Data scanned: 0 B
Data read: 13.53 GiB (ratio 0.00:1)
```

**Tuo tulos on merkityksetön.** ClamAV ei pysty tarkistamaan yhtään yksittäistä tiedostoa, joka on
suurempi kuin 2 GiB − 1. Jos `--max-filesize`- tai `--max-scansize`-rajaa nostaa sen yli, tuloksena on
vain varoitus. Liian suuri tiedosto ohitetaan, ja yhteenvedossa lukee silti `Infected files: 0`.
Paljastava rivi on `Data scanned: 0 B`. Pidimme tätä ajoa hetken puhtaana tuloksena, ennen kuin
huomasimme asian.

Näin sen välttää:

1. **Tarkista purettu kansiopuu, älä arkistoa.**
2. **Varmista, ettei mikään yksittäinen purettu tiedosto ole yli 2 GiB.** Kummankin version suurin
   tiedosto on noin 272 MB:n äänipakkaus.
3. **Pidä rajat enintään arvossa 2047M**, ja lue yhteenvedosta rivi `Data scanned`, älä pelkkää
   riviä `Infected files`.
4. Halutessasi lisää `--alert-exceeds-max=yes`. ClamAV ilmoittaa silloin tiedostoista, jotka se
   ohitti rajan takia, joten ne eivät mene läpi huomaamatta. Emme käyttäneet tätä valintaa alla
   olevissa ajoissa.

```bash
clamscan -r -i --alert-broken \
  --max-filesize=2047M --max-scansize=2047M --max-files=10000 --max-recursion=16 \
  /root/analysis/full
```

Version 1.4.4 ajossa annoimme arvon `4000M`. Se tulosti saman varoituksen, mutta haittaa ei syntynyt,
koska yksikään tiedosto ei ollut lähelläkään 2 GiB:tä.

### Tulokset {#results}

| Versio | Tarkistettuja tiedostoja | Luettu data (`Data read`) | Tarkistettu data (`Data scanned`) | Tartunnat | Aika |
|:------|--------------:|----------:|-------------:|---------:|-----:|
| 2.1.1, vain ohjelmatiedostot (ensimmäinen kierros) | 75 | 491 MiB | 344 MiB | 0 | 2 min 18 s |
| 2.1.1, koko purettu kansiopuu | 833 | 14,46 GiB | 30,37 GiB | **0** | 22 min 28 s |
| 1.4.4, ohjelmatiedostot ja tekstitiedostot | 66 | | 257 MiB | 0 | |
| 1.4.4, koko purettu kansiopuu | 408 | 10,15 GiB | 21,31 GiB | **0** | 16 min 23 s |

`Data scanned` on suurempi kuin `Data read`, koska ClamAV purkaa tunnistamansa pakatun sisällön ja
tarkistaa lopputuloksen. Version 1.4.4 ero 408 tarkistetun ja 410 puretun tiedoston välillä johtuu
kahdesta nollatavuisesta `debug.log`-tiedostosta: ClamAV ilmoitti kummastakin `Empty file` eikä
laskenut niitä mukaan.

---

## Valmiiksi käännetyt Undaunted-DLL:t {#the-prebuilt-undaunted-dlls}

Version 1.4.4 asennus lisää pelikansioon kaksi valmiiksi käännettyä tiedostoa alkuperäisestä
Undaunted-koodivarastosta: `dxgi.dll` ja `UndauntedInternalServer.dll`. Ne eivät ole pelitiedostoja,
mutta ne toimivat pelin prosessin sisällä, joten tarkistimme ne samalla tavalla. Kumpikaan ei ole
allekirjoitettu, mikä on tavallista harrastusprojektille.

| Tarkistus | `dxgi.dll` | `UndauntedInternalServer.dll` |
|:------|:-----------|:------------------------------|
| Koko | 11 264 tavua | 123 392 tavua |
| SHA-256 | `9A431D7B…4B0D1F` | `520EC588…0D0933` |
| Vastaa koodivaraston git-blobia (ei Git LFS -osoitinta) | Kyllä | Kyllä |
| Pakkaus, overlay, TLS-takaisinkutsut, RWX-osiot | Ei mitään | Ei mitään |
| Tuonnit (imports) | Vain `GetSystemDirectoryA`, `LoadLibraryA`, `GetProcAddress` ja MSVC-ajonaikaisen kirjaston tavalliset käynnistystuonnit | Vastaavat tiedostoja `dllmain.cpp` ja `Networking.cpp`, MinHookia ja C-ajonaikaista kirjastoa. Ei verkko-, rekisteri-, prosessinluonti- eikä tiedostonkirjoitustuonteja (sen ainoa `freopen_s`-kutsu liittää vakiotulosteen ja -syötteen sen omaan konsoliin) |
| ClamAV | OK | OK |

Mitä `dxgi.dll` tekee, sen `DllMain`-funktion täydellisen disassemblyn (konekielen purkamisen
luettavaan muotoon) perusteella: se lataa oikean `dxgi.dll`-tiedoston System32-kansiosta, hakee
funktiot `CreateDXGIFactory`, `CreateDXGIFactory1` ja `CreateDXGIFactory2` ja ohjaa omat kolme
vientiään niille. Sen jälkeen se lataa `UndauntedInternalServer.dll`-tiedoston nimen perusteella.
Muuta se ei tee. Nuo kolme vientiä ovat täsmälleen se joukko, jota peli tarvitsee: version 1.4.4
ohjelmatiedosto tuo funktiot `CreateDXGIFactory` ja `CreateDXGIFactory1`, ja järjestelmän `d3d11.dll`
(meidän Windows 10 -koneellamme) tuo funktion `CreateDXGIFactory2`.

Jokainen palvelin-DLL:ään kovakoodattu pelin muistiosoite joko näkyy sen julkaistussa lähdekoodissa
tai tulee luodun SDK:n siirtymistä. Emme löytäneet yhtään, jota lähdekoodi ei selittäisi. Windows
Defender ei havainnut mitään, kun kopioimme tiedostot pelikansioon.

Yksi avoin kysymys: **`dxgi.dll`-tiedoston lähdekoodi ei ole Undaunted-koodivarastossa.** Sen
virheenjäljityspolku viittaa erilliseen projektiin, jonka lähdekoodia emme ole löytäneet julkaistuna
mistään. Sillä on merkitystä AGPL-lisenssin kannalta. Sen korvaaminen omalla välitys-DLL:llämme ja
`UndauntedInternalServer.dll`-tiedoston kääntäminen itse lähdekoodista ovat molemmat tiekartalla.
Siihen asti tarkistamme molemmat tiedostot kiinnitettyjä tiivisteitä vasten ennen jokaista kopiointia.
Täydet tiivisteet löytyvät [kavereiden asennusohjeesta]({{ friends_page.url | relative_url }}).

---

## Tarkistuslista {#checklist}

Näin toistat tämän omalle 1.4.4-kopiollesi:

1. Laske arkiston tiiviste ja vertaa sitä arvoon `556B9A64…6BC6D`.
2. Pura arkisto Linux-ympäristössä zip-slip-suojauksen kanssa ja anna CRC-tarkistusten tehdä työnsä.
3. Aja `osslsigncode verify` jokaiselle `.exe`- ja `.dll`-tiedostolle ja vertaa tallennettua ja
   laskettua tiivistettä. FAILED, jonka ainoat virheet ovat puuttuvat thawte-juuret, on ketjuongelma
   eikä peukalointia, kunhan tiivisteet täsmäävät.
4. Tarkista jokainen tiedosto `Manifest.bin.json`-tiedostoa vasten 16 MiB:n MD5-paloissa.
5. Tarkista purettu kansiopuu ClamAV:lla. Varmista, ettei `Data scanned` ole nolla.
6. Kopioi asennus Windowsiin vasta sitten.

Sitä, mitä tarkistetut tiedostot sisältävät, käsitellään sivulla
[Pelin sisältö ja asetukset]({{ assets_page.url | relative_url }}). Sitä, mitä ohjelmatiedosto pystyy
tekemään ja mitä ei, käsitellään sivulla [Asiakasohjelman sisäosat]({{ ci_page.url | relative_url }}).
