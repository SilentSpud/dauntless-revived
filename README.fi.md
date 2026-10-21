<p align="right"><a href="README.md">🇬🇧 In English</a></p>

<p align="center">
  <img src=".github/assets/banner.png" width="100%" alt="Dauntless Revivedin banneri. Vasemmalla projektin logo: lohikäärmeen pää ja sen alla teksti Dauntless Revived. Oikealla englanniksi: Dauntless 1.4.4 -pelin yksityinen palvelin. Aitoa Dauntless 1.4.4 -peliä voi pelata palvelimella, jonka pystytät itse. Pohjana gwogin ja muiden tekijöiden Undaunted, lisenssi AGPL-3.0. Epävirallinen faniprojekti, joka ei liity Phoenix Labsiin.">
</p>

<p align="center">
  <a href="LICENSE.txt"><img alt="Lisenssi: AGPL-3.0-only" src="https://img.shields.io/badge/license-AGPL--3.0--only-0D669C?style=flat-square&labelColor=031523"></a>
  <a href="https://mixutin.github.io/dauntless-revived/fi/"><img alt="Ohjeet suomeksi" src="https://img.shields.io/badge/ohjeet-suomeksi-0D669C?style=flat-square&labelColor=031523"></a>
  <a href="https://discord.gg/ZJRprHzsgu"><img alt="Liity Dauntless Revived Discordiin" src="https://img.shields.io/badge/Discord-liity%20yhteisöön-5865F2?style=flat-square&logo=discord&logoColor=white"></a>
  <a href="https://github.com/mixutin/dauntless-revived/discussions"><img alt="Keskustelupalsta (GitHub Discussions)" src="https://img.shields.io/badge/discussions-kysy%20%26%20kerro-0D669C?style=flat-square&labelColor=031523&logo=github"></a>
  <a href="https://github.com/mixutin/dauntless-revived/commits/dauntless-revived"><img alt="Viimeisin muutos dauntless-revived-haarassa" src="https://img.shields.io/github/last-commit/mixutin/dauntless-revived/dauntless-revived?style=flat-square&labelColor=031523&color=0D669C"></a>
  <a href="https://github.com/mixutin/dauntless-revived/actions/workflows/ci.yml?query=branch%3Adauntless-revived"><img alt="Automaattiset tarkistukset (CI) dauntless-revived-haarassa" src="https://img.shields.io/github/actions/workflow/status/mixutin/dauntless-revived/ci.yml?branch=dauntless-revived&event=push&style=flat-square&labelColor=031523&label=CI"></a>
</p>

# Dauntless Revived

**Dauntless Revived herättää Dauntless-pelin henkiin omalla palvelimella.** Dauntless oli Phoenix
Labsin ilmainen verkkopeli, jossa pelaajat metsästivät yhdessä suuria hirviöitä. Pelin viralliset
palvelimet (tietokoneet, jotka pyörittivät peliä verkossa) suljettiin **30.5.2025**. Sen jälkeen
peliä ei ole voinut pelata.

Tämän projektin avulla alkuperäistä **Dauntless 1.4.4** -versiota (lokakuulta 2020) voi taas pelata.
Peli vain ottaa yhteyttä palvelimeen, jonka pystytät itse. Projekti on jatkoa
**[Undaunted](https://github.com/SyST3MDeV/Undaunted)-projektille**, jonka tekivät gwog (Gregory
Morford) ja muut tekijät. He rakensivat tärkeimmät osat. Yksi niistä on DLL-tiedosto (pieni
ohjelmakirjasto, jonka peli lataa käynnistyessään). Sen avulla pelistä saadaan pelipalvelin. Lisäksi
he tekivät palvelinohjelmat ja käynnistysohjelman. Tämä versio lisää niihin korjauksia,
tallentuvan etenemisen, palvelimen puolen pelaajaryhmät, Undauntedin käynnistimen pohjalta tehdyn
kaverikäynnistimen, asennuspaketin vuokratuille Windows-palvelimille ja ohjeet.

Tässä koodivarastossa (GitHubissa olevassa projektin kansiossa) tai ohjesivustolla ei jaeta
pelitiedostoja. Tarvitset oman kopion Dauntless 1.4.4 -pelistä.

> **Tämä ei ole julkinen palvelin.** Sitä pyöritetään muutamalle kaverille. Kuka tahansa voi
> pystyttää oman palvelimensa tämän koodin avulla.

## Patreon-tukijat

Suuri kiitos kaikille, jotka tukevat Dauntless Revivediä Patreonissa. ❤️

- **nasagi** — kiitos Patreon-tukijaksi liittymisestä ja projektin tukemisesta.

## Hosting-kumppani — EU Gamehost

<p align="center">
  <a href="https://www.eugamehost.com/"><img src=".github/assets/eugamehost-partner.png" width="620" alt="EU Gamehost"></a>
</p>

**EU Gamehost on Dauntless Revivedin kumppani ja projektin suosittelema hosting-palvelu.** Jos haluat vuokratun palvelimen oman koneen sijaan, katso [hosting-kumppanin opas](https://mixutin.github.io/dauntless-revived/fi/setup/hosting-partner.html).

Kevyen kuorman koko yhden koneen kokoonpanon mitattu arvio on noin **9,5 Gt**, joten **VDS GAME 3 (12 Gt)** on suositeltu lähtötaso koko oletuspinolle ja **VDS GAME 5 (24 Gt)** antaa selvästi enemmän varaa. **GAME 1 (4 Gt)** sopii testaukseen/ohjauspalveluille ja **GAME 2 (8 Gt)** tarkoituksella kevennettyyn kokoonpanoon, mutta kumpikaan ei riitä mitattuun täyteen oletusasetukseen. Dedicated-vaihtoehdoksi nostamme **Ryzen 7 5800X / 32 Gt** -palvelimen.

> **Kumppanuus / mainos:** EU Gamehost on projektin kumppani. Suositukset perustuvat projektin omiin palvelinmittauksiin, eivät riippumattomaan hosting-vertailuun. Hinnat, laitteisto ja saatavuus voivat muuttua, joten tarkista ajantasainen tarjous ennen tilausta.

## Lyhyet vastaukset

### Voiko Dauntlessia vielä pelata?
Virallisilla palvelimilla ei: Phoenix Labs sulki ne 30.5.2025. Jos sinulla on 1.4.4-version kopio,
voit pelata sitä omalla palvelimellasi tämän projektin avulla.

### Onko Dauntlessille yksityispalvelinta?
On. Tämä projekti on sellainen: itse ylläpidettävä palvelin 1.4.4-versiolle, Undauntedin pohjalta.
Se ei ole avoin kaikille, joten pystytät oman ja kutsut kaverisi.

### Pääsenkö pelaamaan teidän palvelimellanne?
Et. Palvelimemme pyörii vuokratulla koneella, mutta tilin voi luoda vain se, jonka omistaja kutsuu.
[Asennusohjeet](https://mixutin.github.io/dauntless-revived/fi/setup/) kertovat, miten pystytät oman.

### Saako täältä pelin?
Ei. Tässä koodivarastossa tai ohjesivustolla ei ole pelitiedostoja eikä linkkejä niihin.

### Mikä pelin versio toimii?
Vain **1.4.4**. Viimeinen versio 2.1.1 ei toimi, koska Undauntedin DLL-tiedosto on tehty juuri
1.4.4-version ohjelmatiedostoa varten.

### Tarvitsenko Epic Games -tilin?
Et. Jokainen pelaaja kirjautuu omalla henkilökohtaisella avaimella (se toimii kuin salasana). Avaimen
antaa oma palvelimesi.

### Onko tämä virallinen?
Ei. Tämä on epävirallinen fanien tekemä projekti, josta ei makseta eikä tienata mitään. Sillä ei ole
mitään yhteyttä Phoenix Labsiin eikä Epic Gamesiin.

Lisää vastauksia on sivulla
[Usein kysyttyjä kysymyksiä](https://mixutin.github.io/dauntless-revived/fi/faq.html).

## Tilanne

Pelaamista koskevat rivit kuvaavat viimeisimpiä oikeita pelitestejä **22.9.2026**; infrastruktuurin
riveillä on mukana myös **1.10.2026** valmistunut Linux/OpenBSD-porttaustyö. Peliä kokeiltiin ensin
yksin palvelinkoneella ja vuokratulla palvelimella, ja 22.9. myös kahdella pelaajalla internetin yli.
Merkintä ”(yksin)” tarkoittaa juuri sitä.

| Ominaisuus | Tila | Lisätietoa |
|---|---|---|
| Kirjautuminen omalla avaimella | Toimii | Epic-tiliä ei tarvita |
| Opetusjakso | Toimii | Pelipalvelin käynnistyy, kun sitä tarvitaan |
| Ramsgate (pelin kaupunki, jossa pelaajat tapaavat) | Toimii (yksin) | Ramsgaten palvelin on koko ajan päällä |
| Training Dojo (harjoitussali) | Toimii | Käynnistyy, kun joku menee sinne |
| Metsästykset | Toimii (yksin) | Pelattu: Lesser Boreus -hirviön metsästys ja takaa-ajo (pursuit) palvelinkoneella sekä uuden pelaajan takaa-ajo internetin yli vuokratulla palvelimella |
| Varusteiden valmistus | Toimii | |
| Tavarat, varusteet ja tehtävät | Toimii (yksin) | Tallentuvat tietokantaan ja säilyvät, vaikka peli ja palvelin käynnistetään uudelleen |
| Pelaajan taso, mestaruus ja Hunt Pass (palkintojärjestelmä) | Toimii (yksin), oletuksena päällä | Alkavat alusta (Slayer-taso 1, ei mestaruutta, tyhjä Hunt Pass) ja tallentuvat; jokaisella tilillä on Elite Hunt Pass. Kokeiltu pelissä testitilillä, myös palvelimen uudelleenkäynnistyksen yli. Vuokratulla palvelimella 22.9.2026: Slayer-taso 3, aseen mestaruus ja hirviön mestaruus (taso 2, ensimmäistä kertaa pelissä nähty), ja pelipalvelin vahvisti tasopalkinnot. `PROGRESSION_MODE=stub` palauttaa alkuperäisen kiinteän tason 50 ([päivitysohjeet](https://mixutin.github.io/dauntless-revived/fi/setup/upgrading.html)) |
| Palvelin vuokratulla koneella | Käynnissä | [Windows-palvelinpaketti](https://mixutin.github.io/dauntless-revived/fi/setup/windows-server.html) asennettiin vuokratulle Windows Server 2019 -virtuaalipalvelimelle julkiseen tilaan 21.–22.9.2026. Siellä tarkistettu: palvelinkokonaisuus käynnistyy koneen käynnistyessä palvelutilillä, Ramsgate pyörii ja lähettää elonmerkkejä (heartbeat), yhdyskäytävä vastaa internetistä kiinnitetyllä varmenteella, ja tunnin välein otettava varmuuskopio toimii. Ensimmäisessä oikeassa testissä (22.9.2026) kolme pelipalvelinta pyöri yhtä aikaa, ja sallittujen lista avasi peliportit pelaajalle ja sulki ne, kun hän lähti |
| Linux-palvelinporttaus | Ohjauspuoli toimii; aito Wine/Proton-pelitesti seuraavana | [Linux-palvelin](https://mixutin.github.io/dauntless-revived/fi/setup/linux-server.html): metagame, sisältö, deploy, gateway ja SQLite toimivat natiivisti Linuxissa/systemd:llä, nftables tekee dynaamisen UDP-sallittujen listan ja Ramsgate/metsästykset käynnistyvät Proton/Wine-kääreen kautta. Paikallinen kiinnitetty 1.4.4-pelikansio läpäisi kaikki 410 manifestitiedoston kokotarkistukset, ja smoke-testissä deploy näki Ramsgaten portissa 8777 oikealla tuotantokomentorivillä. |
| OpenBSD-palvelinporttaus | Natiivi ohjauspuoli CI-testattu; kahden koneen pelitesti seuraavana | [OpenBSD-palvelin](https://mixutin.github.io/dauntless-revived/fi/setup/openbsd-server.html): Node/SQLite-palvelut toimivat natiivisti OpenBSD 7.9:ssa rc.d:n alla, PF hoitaa TLS-ohjauksen ja pelaajataulun, ja Windows x86_64 -peliprosessi ajetaan rajatulla SSH-yhteydellä Linux/Proton/Wine-työntekijällä. Aito OpenBSD 7.9 -virtuaalikone rakentaa ja testaa portin CI:ssä. |
| Kaverikäynnistin | Julkaistu | CI julkaisi ensimmäisen version, 0.1.0:n, [GitHubin julkaisuihin](https://github.com/mixutin/dauntless-revived/releases/latest) `SHA256SUMS.txt`-tiedoston ja käännöksen alkuperätodistuksen (build provenance attestation) kanssa; asennetut käynnistimet päivittävät itsensä. Omistaja rekisteröityi sillä vuokratulle palvelimelle ja latasi pelin (noin 11 Gt) yhdyskäytävän kautta. Samana yönä julkaistu 0.1.1 ei enää laita pelin automaattista valotusta pois; se oli tehnyt Ramsgatesta aivan liian pimeän. Ei vielä allekirjoitettu: omistajan koneella SmartScreen esti asennusohjelman kokonaan, ja tarkistus `SHA256SUMS.txt`-tiedostoa vasten ja eston poistaminen toimivat ([ohje](https://mixutin.github.io/dauntless-revived/fi/setup/friends.html)) |
| Pelaaminen kavereiden kanssa internetin yli | Toimii (kaksi pelaajaa) | Testattu 22.9.2026 vuokratulla palvelimella. Ensin yksi pelaaja: kutsu, rekisteröityminen, pelin lataus, opetusjakso, Ramsgate, Training Dojo ja ensimmäinen metsästys. Sitten kaksi pelaajaa yhdessä: he näkivät toisensa Ramsgatessa ja metsästivät yhdessä, kun he jonottivat samaan metsästykseen muutaman sekunnin sisällä toisistaan (ryhmät eivät vielä toimi pelissä, ks. seuraava rivi) |
| Pelaajaryhmät, kaverit ja killat | Rakennettu, odottaa testiä pelissä | Palvelimen puoli: ryhmäkutsut, hyväksyminen ja hylkääminen, johtajaksi nostaminen, poistaminen ja lähteminen, koko ryhmä samalle metsästyspalvelimelle, yhdessä takaisin Ramsgateen, haku nimellä, kaverilista ja estolista sekä killat (perustaminen, kutsut, arvot, erottaminen, lähteminen ja lakkauttaminen); kaikki paitsi ryhmät tallentuvat SQLiteen. Kahden pelaajan testissä 22.9.2026 ryhmäkutsu tuli toisen pelaajan peliin mutta ei näkynyt, eikä kaverin lisääminen tehnyt mitään: palvelin kuvasi kutsussa `POST /accountinfo/public` väärää pelaajaa ja vastasi kutsuun `POST /account/mapping` muodossa, jota peli ei lue. Molemmat on korjattu ja ovat vuokratulla palvelimellamme 22.9.2026 alkaen, ja korjaukset läpäisevät testit, jotka toistavat pelin omat pyynnöt; pelissä niitä ei ole vielä kokeiltu ([tarkemmin](https://mixutin.github.io/dauntless-revived/fi/findings/social.html)). Kutsuminen ei vaadi kaveruutta. Pelaajien näkyminen paikalla vaatii chat-yhteyden läsnäolotiedot: rakennettu 23.9.2026, oletuksena pois päältä (ks. chat-rivi) |
| Varmuuskopiot | Toimii palvelimillamme | Joka tunti sekä aina palvelimen käynnistyessä ja sammuessa. Windows-palvelinpaketissa on oma varmuuskopiotehtävä (käynnissä vuokratulla palvelimella). Alkuperäisen palvelinkoneemme varmuuskopio-ohjelmat, joilla palautus on kokeiltu, eivät ole tässä koodivarastossa ([ohje oman varmuuskopion tekemiseen](https://mixutin.github.io/dauntless-revived/fi/setup/admin.html#back-up-the-database)) |
| Kavereiden asennuspaketti | Valmis | Vain Tailscalea käyttävä varavaihtoehto: tiedostot tarkistava asennus ja käynnistys. Kukaan kaveri ei ole vielä käyttänyt sitä |
| Bounty-tehtävät (lisätehtävät, joista saa palkintoja) | Ei vielä | Tallentuvat oikean etenemisen kanssa; valintaa ja lunastusta ei ole vielä kokeiltu pelissä |
| Tekstichat ja paikalla olo | Rakennettu, odottaa testiä pelissä; molemmat oletuksena pois päältä | Ramsgaten, metsästysten, ryhmän ja killan chat sekä kuiskaukset käyttäjänimin, metagamen itsensä sisällä; oletuksena pois päältä, kunnes kaksi pelaajaa on kokeillut sitä (palvelinpaketissa `Set-Chat.ps1 -On`). Ensimmäisen chat-palvelimen kirjoitti ja testasi Vvoidddd; miksi se näytti nimien sijaan `UID-...`, ja korjaus: [Tekstichat](https://mixutin.github.io/dauntless-revived/fi/findings/chat.html). Kavereiden näyttäminen paikalla on rakennettu chat-palvelimeen 23.9.2026 (idea Harmonicin haarasta), ja se pysyy pois päältä (`CHAT_PRESENCE=1`), kunnes kaksi pelaajaa on näyttänyt, että ryhmän automaattinen potku pysyy unessa. Käytä sillä välin [Dauntless Revived Discordia](https://discord.gg/ZJRprHzsgu) |
| Slayer Links | Rakennettu, oletuksena päällä, odottaa testiä pelissä | My Links -välilehti: kaksi kaveria liittoutuu viikoksi. Harmonicin haarasta, korjattuna 1.4.4-ohjelmatiedostoa vasten (23.9.2026); ei vielä kokeiltu pelissä, ja peli saattaa pitää välilehden piilossa ([tarkemmin](https://mixutin.github.io/dauntless-revived/fi/findings/social.html#slayer-links)) |
| Escalation | Rakennettu, oletuksena pois päältä | Oikeat Escalation-tallennukset Harmonicin haarasta: peliohjelmasta luettu kausiluettelo ja hänen tallennussääntönsä (23.9.2026). Pois päältä (`ESCALATION_MODE=real`), koska käyttöönotto pudottaa jokaisen pelaajan tekaistusta maksimista tasolle 0; ei kokeiltu pelissä ([tarkemmin](https://mixutin.github.io/dauntless-revived/fi/findings/escalation.html)) |
| Kauppa | Rakennettu, oletuksena pois päältä | Ilmainen kauppa Harmonicin haarasta: 200 ilmaista kosmeettista tavaraa, jotka ostetaan pelin omalla ostotavalla (23.9.2026). Pois päältä (`STORE=free`), kunnes omistaja päättää, pysyykö kauppa ilmaisena, ja sitä on kokeiltu pelissä ([tarkemmin](https://mixutin.github.io/dauntless-revived/fi/findings/store.html)) |
| Useampi varustesarja | Taustapalvelu valmis, pelin käyttöliittymätesti puuttuu | Kaikkien kuuden asiakaspaikan avaukset, aktiivinen paikka ja erilliset tallennukset säilyvät oikean etenemisen kanssa. Palvelin ei enää ilmoita virheellistä seitsemättä paikkaa. Lisäpaikkoja ei ole vielä kokeiltu pelissä |

Tarkka tehtävälista on tiedostossa [ROADMAP.md](ROADMAP.md) (englanniksi). Lyhyempi selitys suomeksi on
ohjesivuston sivulla [Tiekartta](https://mixutin.github.io/dauntless-revived/fi/roadmap.html).

## Tärkeimmät linkit

| Linkki | Mitä sieltä löytyy |
|---|---|
| [Ohjeet suomeksi](https://mixutin.github.io/dauntless-revived/fi/) | Projektin ohjesivut suomeksi |
| [Ohjesivusto englanniksi](https://mixutin.github.io/dauntless-revived/) | Samat ohjesivut englanniksi (in English) |
| [Asennusohjeet](https://mixutin.github.io/dauntless-revived/fi/setup/) | Oman palvelimen pystytys, kaverina liittyminen, palvelin ryhmälle ja ongelmien ratkaisu |
| [Tekninen viite](https://mixutin.github.io/dauntless-revived/fi/reference/) | Jokainen asetus, portti, HTTP-reitti, tiedosto ja skripti oletusarvoineen palvelimen ylläpitäjille ja kehittäjille |
| [Windows-palvelinpaketti](https://mixutin.github.io/dauntless-revived/fi/setup/windows-server.html) | Aina päällä oleva palvelin vuokratulla Windows Server 2019 -koneella yhdellä komennolla asennettuna, julkisessa tai yksityisessä tilassa |
| [Käynnistimen lataus](https://github.com/mixutin/dauntless-revived/releases/latest) | Dauntless Revived Launcher kutsutuille kavereille ja `SHA256SUMS.txt` |
| [Kavereiden asennuspaketti](friend-kit/) | Kertaluonteinen asennus ja pelin käynnistin kutsutuille pelaajille ([ohje](https://mixutin.github.io/dauntless-revived/fi/setup/friends.html)) |
| [Tehtävälista](ROADMAP.md) | Välitavoitteet M0–M4 ja mitä on jo tehty |
| [Usein kysytyt kysymykset](https://mixutin.github.io/dauntless-revived/fi/faq.html) | Lyhyet vastaukset yleisiin kysymyksiin |
| [Discord](https://discord.gg/ZJRprHzsgu) | Yhteisön chat, pelikavereiden etsiminen, apu ja projektin päivitykset |
| [Keskustelupalsta](https://github.com/mixutin/dauntless-revived/discussions) | Kysymykset, ideat ja omat asennukset |

## Miten tämä toimii

1. Jokainen pelaaja käynnistää alkuperäisen, muuttamattoman 1.4.4-pelin. Pelin kansiossa on kaksi Undauntedin DLL-tiedostoa, jotka ohjaavat pelin yhteydet omalle palvelimellesi.
2. **Metagame** on taustapalvelin eli ohjelma, jonka kanssa peli keskustelee taustalla (TypeScript, Express ja SQLite-tietokanta, portti 61000; portti on kuin oven numero verkossa). Se hoitaa tilit, hahmot, tavarat, varusteet, etenemisen, pelien järjestämisen (matchmaking), pelaajaryhmät ja kaverilistan.
3. **Deploy server** (portti 61001, vain palvelinkoneella) käynnistää pelipalvelimia tarpeen mukaan. Ne ovat saman pelin kopioita, jotka DLL-tiedosto muuttaa palvelimiksi.
4. Pelipalvelimet (UDP-portit 8770–8777: Ramsgate portissa 8777, harjoitussali portissa 8776 ja enintään 6 metsästystä porteissa 8770–8775) pyörittävät itse peliä.
5. Kaverit yhdistävät kahdella tavalla. **Yksityisessä tilassa** he tulevat palvelinkoneelle Tailscalen kautta. **Julkisessa tilassa** salattu **yhdyskäytävä** (gateway) on pelin ainoa julkinen TCP-portti: kaverikäynnistin tarkistaa sen varmenteen kutsun sormenjälkeä vasten, ja pelin UDP-portit avautuvat vain kirjautuneiden pelaajien osoitteille. Windows käyttää Windows Firewallia, Linux nftablesia ja OpenBSD PF:n pelaajataulua sekä ohjausta/NAT:ia Linux-pelityöntekijälle. Sen takana oleva **sisältöpalvelin** antaa pelitiedostot rekisteröityneille tileille, ja käynnistin tarkistaa jokaisen tiedoston siihen sisäänrakennettua listaa vasten.

## Mitä projektissa on

Koodivarasto siirtyy rakenteeseen, jossa kullakin kieliperheellä on oma kansionsa: `rust/` kaikille
Rust-osille (käynnistimen taustaosa, palvelimet ja niiden yhteiset kirjastot), `native/` C++:lle,
`client-mods/` ja `tools/`. Tavoiteltu rakenne ja siirtojen järjestys ovat tiedostossa
[`MIGRATION_PLAN.md`](MIGRATION_PLAN.md#2-repository-and-workspace-layout) (englanniksi). Kukin
Node-projekti (Electron-käynnistin ja TypeScript-palvelimet) pitää alkuperäisen `Undaunted...`-kansionsa
`legacy/`-kansion alla, kunnes sen Rust-versio korvaa sen, joten vanha ja uusi koodi erottuvat
toisistaan; niissä olevien npm-pakettien nimet ovat `dauntless-revived-*`. Palvelinpaketit asentavat ne
edelleen asennuskansion ylätasolle kuten ennenkin.

| Kansio | Mikä se on |
|---|---|
| `legacy/UndauntedMetagame/` | Taustapalvelin, jonka kanssa peli keskustelee: tilit, hahmot, tavarat, varusteet, eteneminen, pelien järjestäminen, pelaajaryhmät, kaverilista ja ylläpitorajapinta `/undaunted/api` |
| `legacy/UndauntedDeployServer/` | Käynnistää ja valvoo pelipalvelimia (Ramsgate, metsästykset, harjoitussali) |
| `legacy/UndauntedGateway/` | Vain julkisessa tilassa: salattu yhdyskäytävä (pelin ainoa julkinen TCP-portti) ja apuohjelma, joka avaa peliportit kirjautuneille pelaajille |
| `legacy/UndauntedContent/` | Sisältöpalvelin: pelitiedostot (tarkistetaan 410 tiedoston luetteloa vasten), uutiset ja kuvapaketti, vain rekisteröityneille käynnistimille |
| `legacy/UndauntedLauncher/` | Dauntless Revived Launcher Windowsille ja x86_64-Linuxille, pohjana alkuperäisen Undaunted-projektin käynnistin. Kansiossa `assets/` ovat ne kaksi tiivisteillä kiinnitettyä valmista DLL-tiedostoa, jotka jokainen asennustapa asentaa |
| `rust/` | Cargo-työtila, johon kaikki Rust-osat rakennetaan. Toistaiseksi siinä on `crates/protocol`, käynnistimen ja palvelinten yhteiset tietorakenteet |
| `native/internal-server/` | Undauntedin palvelin-DLL:n C++-lähdekoodi. DLL:n avulla peli toimii pelipalvelimena, ja se ohjaa pelaajat omalle taustapalvelimelle. Se kääntyy Visual Studio 2022 Build Toolsilla, mutta mikään tässä koodivarastossa ei käännä sitä; jokainen asennustapa käyttää valmiita DLL-tiedostoja kansiosta `legacy/UndauntedLauncher/assets/` |
| `client-mods/` | Valinnaiset UE4SS-modit, jotka käynnistin asentaa peliohjelman viereen; `manifest.json` luettelee niiden tiedostot |
| `deploy/windows-server/` | Windows-palvelinpaketti: asentaa ja pyörittää koko palvelimen Windows Server 2019 -koneella julkisessa tai yksityisessä tilassa, varmuuskopioineen, kutsuineen ja päivityksineen |
| `deploy/linux-server/` | Erillinen Linux-palvelinporttaus: systemd-palvelut, nftables-sallittujen lista, yhden koneen Proton/Wine-kääre ja Linux-pelityöntekijän asennin |
| `deploy/openbsd-server/` | Erillinen OpenBSD 7.9 -ohjauspuoli: rc.d-palvelut, PF-säännöt ja rajattu SSH-käynnistin Linux-pelityöntekijälle |
| `deploy/unix-common/` | Linuxin ja OpenBSD:n yhteiset asetusten, SQLite-bootstrapin, kutsujen ja yhteensopivuusajon apuohjelmat; Windows-paketti ei käytä niitä |
| `friend-kit/` | Vain Tailscalea käyttävät asennus- ja käynnistysohjelmat kavereiden koneille |
| `tools/` | `sync-roadmap.ts` ja `build-llms.ts` (ohjesivuston tuotetut tiedostot), `make-friend-kit.ps1`, `make-game-manifest.ts` sekä kansiossa `ci/` CI:n tarkistukset |
| `brand/` | Logo, tunnus ja väripaletti sekä `build.py`, joka tekee niistä kaikki käynnistimen, ohjesivuston ja GitHubin käyttämät koot |
| `docs/` | Ohjesivusto (GitHub Pages); suomenkieliset sivut ovat kansiossa `docs/fi/` |
| `.github/` | CI:n työnkulut (`ci.yml`, `launcher-release.yml`), Dependabot sekä ilmoitus- ja muutospyyntöpohjat |
| `ROADMAP.md` | Suunnitelma ja tehtävälista |
| `MIGRATION_PLAN.md` | Suunnitelma käynnistimen siirrosta Tauriin, palvelinten siirrosta Rustiin ja koodivaraston siirrosta tähän rakenteeseen (englanniksi) |

## Muutokset alkuperäiseen Undauntediin

- Metagame ja deploy server kuuntelevat oletuksena vain omaa konetta (`BIND_HOST`). Alkuperäinen
  versio kuunteli kaikkia verkkoja, eikä deploy serverissä ole salasanaa tai muuta tunnistusta.
- Jos palvelin ei saa käyttöönsä porttia, se kertoo virheestä selvästi. Alkuperäinen versio ilmoitti
  onnistuneensa ja sulkeutui hiljaa.
- Jokainen metagamen saama pyyntö kirjataan lokiin (tapahtumaluetteloon). Pelipalvelimien pyynnöt
  on merkitty erikseen.
- Harjoitussali käynnistyy vasta, kun joku menee sinne, eikä heti alussa (`ENABLE_DOJO=1` palauttaa
  vanhan toiminnan).
- Kirjautumistunnisteet poistetaan lokista. Valinnainen tallennus (`LOG_BODIES=1`) kirjaa, mitä peli
  lähettää niihin tallennuksiin, jotka eivät vielä toimi. Kustakin pyynnöstä tallennetaan enintään 8 kt
  (`/inventory`-pyynnöistä 64 kt), ja tunnisteet poistetaan. Windows-palvelinpaketti pitää sen pois
  päältä julkisessa tilassa.
- **Turvallisemmat tallennukset.** Hahmon tallennus tehdään yhtenä tapahtumana (transaktio), ja
  vanhentunut versio hylätään (409). Kahdesti saapuva tavaratapahtuma tehdään vain kerran, ja jokainen
  tavaramuutos kirjataan lokiin, jota ei voi muuttaa jälkikäteen. Hahmon ja varustesarjan aiempaan
  versioon voi palata ylläpitorajapinnan kautta. Alkuperäinen versio ei myöskään kertonut peliin
  tapahtuman jälkeen poistetuista pinoista, joten käytetyt Ramsit ja materiaalit jäivät näkyviin ja
  toinen päivitys meni läpi ilmaiseksi; tämä on korjattu.
- **Käyttäjänimet ja kutsut.** Nimessä on 3–16 kirjainta, numeroa tai alaviivaa, se on yksilöllinen
  isoista ja pienistä kirjaimista riippumatta, ja se tarkistetaan yhdessä kutsukoodin kanssa.
  Ylläpitäjä voi vaihtaa pelaajan nimen.
- **Oikeuksien tarkistus jokaisessa reitissä.** Pelaaja voi lukea ja muuttaa vain omaa tiliään ja omia
  hahmojaan. Pelipalvelimien reitit vaativat pelipalvelimen avaimen ja suoran yhteyden samalta koneelta.
- **Pelaajaryhmät, kaverit ja killat** (palvelimen puoli, ei vielä kokeiltu kahdella oikealla
  peliohjelmalla): kutsut, johtajaksi nostaminen, poistaminen ja lähteminen, koko ryhmä samalle
  metsästyspalvelimelle ja yhdessä takaisin Ramsgateen, haku nimellä, kaverilista ja estolista sekä
  1.4.4-peliohjelman killat, tallennettuina SQLiteen. Alkuperäisen projektin tilitietovastaus kuvasi
  kysyjää eikä kysyttyä pelaajaa, joten kaikki muut pelaajat jäivät piiloon Sosiaalinen-paneelista;
  meidän vastauksemme kertoo kysytystä tilistä.
- **Harmonicin 1.4.4-haarasta siirretty työ** ([Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted),
  syyskuu 2026): oikeat Escalation-tallennukset ja ilmainen pelin kauppa (molemmat rakennettu,
  oletuksena pois päältä), Slayer Links, kavereiden paikalla olo omassa chat-palvelimessamme
  (oletuksena pois päältä), kaatuneen Ramsgaten uudelleenkäynnistys pelaajan matkustaessa sinne, suoja
  toistettuja etenemismyöntöjä vastaan ja muuta. Mitä otettiin ja mitä ei, syineen:
  [Harmonicin työn siirto](https://mixutin.github.io/dauntless-revived/fi/findings/harmonic-fork.html).
- **Palvelimen tila käynnistimelle.** Lista paikalla olevista pelaajista ja käynnissä olevista
  metsästyksistä näytetään vain rekisteröityneille pelaajille. `/dauntless-status` kertoo palvelimen
  nimen, version, lähdekoodin osoitteen ja koodiversion (AGPL:n takia), mutta ei pelaajamäärää.
- **Oikea eteneminen oletuksena.** Alkuperäinen versio vastasi etenemistä koskeviin kyselyihin
  kiinteällä mallilla (jokainen tili tasolla 50, Hunt Passissa ei mitään lunastettavaa ja Elite-rata
  todennäköisesti lukittuna) eikä tallentanut mitään. Meidän metagamemme tallentaa Slayer-tason, mestaruuden
  (mastery), Hunt Passin, oikeudet (Elite-passi kaikille), varustesarjojen paikat, odotusajat ja
  palkkiotehtävät jokaiselle tilille erikseen. `PROGRESSION_MODE=stub` palauttaa alkuperäisen
  toiminnan. Päivitätkö palvelinta, jolla on jo pelaajia? Lue ensin
  [päivitysohjeet](https://mixutin.github.io/dauntless-revived/fi/setup/upgrading.html).
- **Kavereiden asennuspaketti** (`friend-kit/`, kootaan ohjelmalla `tools/make-friend-kit.ps1`).
  Se tarkistaa Undauntedin kaksi DLL-tiedostoa tarkistussummilla (tiedoston ”sormenjäljillä”),
  rekisteröi pelaajan ja ohjaa pelin chat-yhteyden palvelinkoneelle, jotta peli ei ota yhteyttä
  Epicin vanhaan chat-palvelimeen. Paketin mukana tulevat lisenssi, muiden tekijöiden
  tekijänoikeustiedot ja `SOURCE.txt`, joka kertoo tarkan koodiversion.
- Oma **kaverikäynnistin** (`legacy/UndauntedLauncher/`), tehty alkuperäisen käynnistimen pohjalta, joka oli
  kytketty kiinteästi Undauntedin omiin palvelimiin. Se liittyy palvelimelle kutsulla, rekisteröi
  pelaajan (se säilyttää avaimen käyttöjärjestelmän suojatussa tallennuksessa: Windowsissa DPAPI,
  Linuxissa työpöydän salaisuustallennus; ainoa selväkielinen
  kopio on varmuuskopio, jonka pelaaja itse päättää tallentaa), lataa pelin isännältä ja tarkistaa
  jokaisen tiedoston siihen sisäänrakennettua luetteloa vasten, asentaa kaksi kiinnitettyä
  DLL-tiedostoa ja pelin asetukset sekä välittää julkisessa tilassa pelin salaamattomat HTTP-kutsut salattuna yhteytenä, joka on kiinnitetty
  kutsun varmenteeseen. CI julkaisee jokaisen uuden version GitHubin julkaisuihin, ja asennetut
  käynnistimet päivittävät itsensä.
- **Sisältöpalvelin** (`legacy/UndauntedContent/`), joka antaa pelitiedostot vain rekisteröityneille tileille
  ja jatkaa keskeytyneitä latauksia, sekä julkisen tilan **yhdyskäytävä** (`legacy/UndauntedGateway/`): pelin
  ainoa julkinen TCP-portti, salaus kiinnitetyllä itse allekirjoitetulla varmenteella, ylläpitoreitit ja
  pelipalvelimen avain torjutaan ulkopuolelta, pyyntöjen koko- ja määrärajat sekä apuohjelma, joka
  avaa pelin UDP-portit vain kirjautuneiden pelaajien osoitteille.
- **Windows-palvelinpaketti** (`deploy/windows-server/`): yksi komento omalta koneelta asentaa koko
  palvelimen Windows Server 2019 -koneelle pelkällä avaimella toimivan SSH-yhteyden yli. Mukana ovat
  vähäoikeuksinen palvelutili, käynnistys koneen käynnistyessä, valvonta, tunnin välein otettavat
  varmuuskopiot, kutsut ja päivitykset.
- Erillinen **Linux-palvelinporttaus** (`deploy/linux-server/`): Node/SQLite-ohjauspuoli toimii
  natiivisti systemd:n alla, julkiset peliportit suodatetaan dynaamisella nftables-listalla ja
  kiinnitetyn Windows 1.4.4 -pelin palvelinprosessit ajetaan Protonilla/Winellä muuttamatta Windows-polkuja.
- Erillinen **OpenBSD-palvelinporttaus** (`deploy/openbsd-server/`): ohjauspuoli toimii natiivisti
  OpenBSD 7.9:ssa rc.d:n alla, PF hoitaa TLS-ohjauksen ja pelaajakohtaisen peli-UDP:n, ja rajattu SSH
  käynnistää varsinaiset Windows-peliprosessit Linux/Proton/Wine-työntekijällä.
- Metsästyspalvelimet käynnistyvät konsoli-ikkuna piilotettuna.
- **CI** jokaisesta muutoksesta: jokaisen paketin käännös ja testit, Windows-palvelinpaketti,
  erillinen Linux-palvelinportti, OpenBSD-portin rakennus/testi aidossa OpenBSD 7.9 -virtuaalikoneessa,
  ohjesivuston käännös sekä tarkistus, ettei projektiin ole lisätty salaisuuksia, avaimia,
  tietokantoja tai pelitiedostoja.
- **Oma nimi.** Käynnistimessä, pelin tervetulotekstissä ja palvelimen viesteissä lukee Dauntless
  Revived, ja kiitoksissa mainitaan Undaunted. Kansiot, palvelin-DLL:n tiedostonimi
  (`UndauntedInternalServer.dll`), `/undaunted/api`-reitit ja `x-undaunted-*`-otsakkeet pitävät
  toistaiseksi Undaunted-nimet (tehtävälistan kohta 4.15).
- **Ohjesivusto** kansiossa `docs/`, osoitteessa
  [mixutin.github.io/dauntless-revived](https://mixutin.github.io/dauntless-revived/), englanniksi ja
  suomeksi: asennusohjeet, tekninen viite jokaisesta asetuksesta, portista, reitistä ja tiedostosta,
  tutkimustulokset pelin taustapalveluista ja tehtävälista.

Varmuuskopiot: Windows-palvelinpaketti ottaa ne itse (joka tunti sekä aina käynnistyksen ja
pysäytyksen yhteydessä), ja jokainen tietokannan kopio tarkistetaan `PRAGMA integrity_check`
-komennolla. Alkuperäisellä palvelinkoneellamme on omat varmuuskopio-ohjelmansa (tehtävälistan kohta
0.1), jotka eivät ole tässä koodivarastossa. Metagame ei itse ota varmuuskopioita. Jos pystytät
palvelimen käsin, huolehdi varmuuskopioista itse. Ohje on sivulla
[Palvelin ryhmälle](https://mixutin.github.io/dauntless-revived/fi/setup/admin.html#back-up-the-database).

## Tietoturva

- Metagame ja deploy server kuuntelevat oletuksena vain osoitetta `127.0.0.1` eli omaa konetta.
- Deploy serverissä **ei ole tunnistusta**. Se on tehty niin tarkoituksella. Älä koskaan avaa sitä
  muiden koneiden käyttöön.
- Kaverit yhdistävät joko Tailscalen kautta (yksityinen tila) tai yhdyskäytävän kautta (julkinen tila).
  1.4.4-peli itse käyttää salaamatonta HTTP:tä, joten julkisessa tilassa se keskustelee vain pelaajan
  omalla koneella olevan käynnistimen kanssa, ja käynnistin välittää liikenteen salattuna palvelimen
  varmenteeseen kiinnitettyä yhteyttä pitkin. Palvelimen pelipalveluista vain yhdyskäytävän TCP-portti
  on auki kaikille; pelin UDP-portit avautuvat vain kirjautuneille pelaajille.

Löysitkö tietoturva-aukon (virheen, jota joku voisi käyttää väärin)? Ilmoita siitä yksityisesti
[tästä linkistä](https://github.com/mixutin/dauntless-revived/security/advisories/new), älä
julkisessa keskustelussa. Lisätietoa on tiedostossa [SECURITY.md](SECURITY.md) (lopussa suomeksi).

## Osallistuminen

Ideat ja korjaukset ovat tervetulleita. Aloita isommat muutokset avaamalla keskustelu
[keskustelupalstalla](https://github.com/mixutin/dauntless-revived/discussions). Kokeile muutoksia
erillisellä testitilillä, älä omalla. Älä koskaan lisää projektiin avaimia, `.env`-tiedostoja tai
pelin tiedostoja. Tarkemmat ohjeet ovat tiedostoissa [CONTRIBUTING.md](CONTRIBUTING.md) ja
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) (molemmissa suomenkielinen osio).

## Muut vastaavat projektit

- **[Undaunted](https://github.com/SyST3MDeV/Undaunted)**: alkuperäinen projekti, jonka pohjalta
  tämä on tehty.
- **[Mystic Paradox](https://github.com/pranav158/Mystic-Paradox)**: vie saman idean pelin
  1.12.0-versioon, jossa on myöhemmin julkaistua sisältöä. Emme ole kokeilleet sitä, eikä tässä
  projektissa ole sen koodia.

## Kiitokset

- **[Undaunted](https://github.com/SyST3MDeV/Undaunted)**: gwog (Gregory Morford,
  [SyST3MDeV](https://github.com/SyST3MDeV)), [EisigesEis](https://github.com/EisigesEis) ja
  [muut tekijät](https://github.com/SyST3MDeV/Undaunted/graphs/contributors). gwog teki
  palvelintilan DLL-tiedoston, deploy serverin, metagamen ja käynnistysohjelman; EisigesEis kehitti
  metagamea (inventaario ja varustelut, eteneminen ja mestaruus, kutsukoodit, ylläpidon
  rajapinta). Moninpeli Ramsgatessa ja metsästyksissä 1.4.4-versiolla on heidän saavutuksensa.
- **[MinHook](https://github.com/TsudaKageyu/minhook)**, tekijä Tsuda Kageyu (BSD 2-Clause
  -lisenssi): ohjelmakirjasto, jota palvelimen DLL-tiedosto käyttää.
- **[Dumper-7](https://github.com/Encryqed/Dumper-7)**, tekijät Encryqed ja muut: työkalu, jonka
  avulla DLL-tiedosto on rakennettu Unreal Engine -pelimoottoria varten.
- **Phoenix Labs**, joka teki Dauntlessin.

### Osallistujat

- **[mixutin](https://github.com/mixutin)** (ylläpitäjä): palvelinpaketti, käynnistin,
  taustapalvelun korjaukset, oikea eteneminen ja ohjeet.
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
- **Harmonic** ([Harmonicrain/Undaunted](https://github.com/Harmonicrain/Undaunted), hänen Dauntless
  1.4.4 -haaransa Undauntedista): Escalationin kausiluettelo ja tallennussäännöt, ilmainen kauppa
  (valikoima, ostotapa ja kaupan välilehdet), ensimmäinen toimiva Slayer Links, deploy-palvelimen
  tapa käynnistää kaatunut Ramsgate uudelleen, idea kavereiden paikalla olosta sekä hänen haarastaan
  siirtämämme testitapaukset. [NOTICE.md](NOTICE.md) luettelee, mitä hänen haarastaan tuli.

Kaikki osallistujat näkyvät [osallistujasivulla](https://github.com/mixutin/dauntless-revived/graphs/contributors).

Koko luettelo on ohjesivuston sivulla
[Kiitokset ja lisenssi](https://mixutin.github.io/dauntless-revived/fi/legal.html).

## Lisenssi

Tämä on **muokattu versio [Undaunted](https://github.com/SyST3MDeV/Undaunted)-projektista**, jonka
tekivät gwog (Gregory Morford) ja muut tekijät. Muutokset aloitettiin **21.9.2026**. Jokainen muutos
näkyy tämän projektin muutoshistoriassa, ja suunnitellut muutokset ovat tiedostossa
[ROADMAP.md](ROADMAP.md).

Lisenssi on GNU Affero General Public License v3.0 only, ks. [LICENSE.txt](LICENSE.txt). Lisenssi on
käyttöehdot, jotka kertovat, miten koodia saa käyttää. Tärkein sääntö: jos pyörität muokattua
versiota muille ihmisille, sinun pitää tarjota heille sen lähdekoodi (ohjelman ihmisen luettava
muoto). [NOTICE.md](NOTICE.md) kertoo, mistä kukin koodivaraston osa on peräisin, myös Harmonicin
1.4.4-haarasta siirretty työ.

Tällä projektilla ei ole yhteyttä Phoenix Labsiin eikä Epic Gamesiin, eivätkä ne ole hyväksyneet
sitä. ”Dauntless” on omistajiensa tavaramerkki. Tässä projektissa tai sen ohjesivustolla ei jaeta
pelin tiedostoja.
