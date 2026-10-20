---
title: Näin moninpeli toimii
parent: Löydökset
grand_parent: Dauntless Revived suomeksi
nav_order: 6
lang: fi
ref: findings/multiplayer
locale: fi_FI
description: "Miten Undaunted tekee Dauntless 1.4.4 -peliohjelman kopioista pelipalvelimia DLL-tiedoston avulla, ja miksi luulimme kerran moninpelin olevan mahdoton."
---

{% assign ci_page = site.pages | where: "path", "fi/findings/client-internals.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}

# Näin moninpeli toimii
{: .no_toc }

Dauntlessin peliohjelma on pelkkä asiakasohjelma (client): se on tehty pelaamiseen omalla koneella,
ei pelin pyörittämiseen muille. Yksinään se ei pysty isännöimään peliä. Silti
[Undaunted](https://github.com/SyST3MDeV/Undaunted) pyörittää yhteistä Ramsgatea (pelin kaupunkia,
jossa pelaajat tapaavat toisiaan) ja usean pelaajan metsästysretkiä aidolla **1.4.4**-version
asiakasohjelmalla. Se tekee sen lataamalla DLL-tiedoston (lisäosan, joka ladataan toisen ohjelman
sisään) saman ohjelmatiedoston ylimääräisiin kopioihin. Näin jokaisesta kopiosta tulee pelipalvelin
(ohjelma, joka pyörittää peliä pelaajille verkossa).

Tämä sivu selittää, miksi se toimii ja miten osat sopivat yhteen. Kaikki, mitä tällä sivulla
kerrotaan Undauntedin koodista, koskee **vain versiota 1.4.4**. Todisteet siitä, että käännös on
pelkkä asiakasohjelma, löytyvät sivulta
[Asiakasohjelman sisäosat]({{ ci_page.url | relative_url }}).

> **Korjaus.** Kun olimme purkaneet 2.1.1-version asiakasohjelman, kirjoitimme kerran, että
> moninpeli on mahdoton pelin mukana tulleilla tiedostoilla. Sen taustalla olleet tosiasiat pitivät
> paikkansa: palvelimen aloituskohdat (entry points) on todella jätetty pois käännöksestä.
> Johtopäätös oli väärä. Niiden alla oleva verkkokerros on ehjä, ja ohjelmaan ladattu DLL pystyy
> ohjaamaan sitä. Osio [Miksi sanoimme kerran, että se on mahdotonta](#why-we-once-said-it-was-impossible)
> alempana kertoo, missä menimme vikaan.

<details open markdown="block">
  <summary>Sisällys</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Mitä poistettiin ja mitä ei {#what-was-stripped-and-what-was-not}

`WITH_SERVER_CODE=0`-käännöksestä puuttuvat ne funktiot, jotka tekevät prosessista palvelimen.
Siihen jää kuitenkin suurin osa siitä, mitä palvelin tarvitsee, kun se kerran on palvelin.

| Osa | Tila | Missä (1.4.4:n RVA, ellei toisin mainita) |
|---|---|---|
| `UWorld::Listen` | Poistettu: "palauta epätosi" -tynkä (stub) | `0x789370` (2.1.1: `0x140bfde50`) |
| `UWorld::InternalGetNetMode` | Ilmoittaa aina `NM_Client`, kun verkkoajuri on olemassa | `0x378BDA0` (2.1.1: `0x1453e4ef0`) |
| `GIsServer` / `GIsClient` | Pakotetaan "asiakkaaksi" `FEngineLoop::PreInit`-vaiheessa | globaalit muuttujat osoitteissa `0x5E4BC3A` / `0x5E4BC39` |
| `UEngine::CreateNamedNetDriver` | Mukana | `0x371A5E0` |
| `UIpNetDriver::InitListen` | Ehjä. Se sitoo UDP-pistokkeen (socket). | `0x806F80` (2.1.1: `0x140c6d200`) |
| Ohjausviestien (Hello, Login, Join) käsittely `UWorld::NotifyControlMessage`-funktiossa | Ehjä | 2.1.1: `0x1453e5d40`, täysi `NMT_*`-hyppytaulu |
| Pelitilan kirjautumiskäsittely (`PreLogin`, `PostLogin`, pelaajan luominen) | Ehjä | `AArchonGameMode::PostLogin` osoitteessa `0x14B7460` |
| Actor-kanavat ja `ReplicateActor` | Ehjä | `CreateChannelByName` `0x3449E10`, `ReplicateActor` `0x327E860` |
| Moottorin oma palvelimen replikointisilmukka | Undaunted ei käytä sitä | Sen `ServerReplicateActors_*`-apufunktioiden nimet puuttuvat molemmista ohjelmatiedostoista. Tulkitsemme senkin poistetuksi, mutta puuttuvat nimet ovat heikko todiste (**ei todistettu**). |

Puuttuvat osat ovat siis kutsu, joka avaa kuuntelevan verkkoajurin (net driver), verkkotilan
vastaus, kaksi globaalia lippua ja, sikäli kuin voimme päätellä, palvelimen replikointisilmukka.
(Replikointi tarkoittaa pelimaailman tilan kopioimista palvelimelta pelaajien koneille.) Kaikki muu
on käännetty mukaan, mutta tavallinen asiakasohjelma ei koskaan kutsu sitä.

---

## Undauntedin rakenne {#undaunteds-design}

| Osa | Kieli | Tehtävä |
|---|---|---|
| `legacy/UndauntedLauncher` | Electron | Tarkistaa ohjelmatiedoston SHA-256-tiivisteen, kopioi kaksi DLL-tiedostoa kansioon `Archon\Binaries\Win64` ennen jokaista käynnistystä ja käynnistää asiakasohjelman pelaajan tiliavaimella |
| `legacy/UndauntedMetagame` | TypeScript, SQLite (Drizzle) | Taustapalvelu: kirjautuminen, hahmot, tavaraluettelo, varustukset (loadouts), edistyminen, matchmaking-jono, ylläpidon rajapinta (admin API) |
| `legacy/UndauntedDeployServer` | TypeScript (Express) | Käynnistää ja valvoo pelipalvelinprosesseja |
| `UndauntedInternalServer.dll` | C++ (MinHook, Dumper-7-SDK) | Ladataan jokaiseen peliprosessiin, myös asiakasohjelmaan. Toimii palvelintilassa tai asiakastilassa. |
| `dxgi.dll` | valmiiksi käännetty välitys-DLL (proxy) | Lataa DLL:n prosessiin |

### DLL:n lataaminen: `dxgi.dll`-välitys-DLL {#loading-the-dll-the-dxgidll-proxy}

- `dxgi.dll` on 1.4.4-ohjelmatiedoston staattinen tuonti (import). Ohjelmatiedoston viereen
  sijoitettu `dxgi.dll` ladataan siksi prosessin käynnistyessä, ennen `FEngineLoop::PreInit`-vaihetta,
  jopa `-nullrhi`-valitsimella. Ajoitus on tärkeä, koska `GIsServer`/`GIsClient`-paikkausten on
  oltava paikallaan, ennen kuin moottori alustaa itsensä.
- Välitys-DLL toimitetaan valmiiksi käännettynä (11 264 tavua), eikä sen lähdekoodia ole julkaistu.
  Se, miten se lataa palvelin-DLL:n, on oma päätelmämme: DLL vie (export) funktion `DummyLinkFunc`,
  mikä viittaa tuontiin perustuvaan linkitykseen. Oman välitys-DLL:n kirjoittaminen on
  tiekartallamme, osittain AGPL-lisenssin takia.
- DLL käyttää kiinteitä RVA-osoitteita eikä etsi koodista tunnistekuvioita (signature scanning).
  Sillä ei ole omaa versiotarkistustakaan. Käynnistimen tiivistetarkistus on ainoa suoja. Jos DLL
  ladattaisiin mihin tahansa muuhun ohjelmatiedostoon, se kirjoittaisi väärään koodiin.
  1.4.4-ohjelmatiedoston, jota varten se on tehty, SHA-256 on
  `d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4`, eli sama arvo, joka on
  kiinnitetty Undauntedin käynnistimeen. Yksikään siirtymä (offset) ei täsmää versioon 2.1.1.

### Yksi DLL, kaksi tilaa {#one-dll-two-modes}

DLL valitsee tilansa yhdellä testillä: onko prosessin raa'assa komentorivissä merkkijono
`-server`? Testi on kirjainkoon huomioiva alimerkkijonohaku koko rivistä, ohjelmatiedoston polku
mukaan lukien. Ympäristömuuttujaa tai asetustiedostoa ei ole.

### Palvelintila {#server-mode}

Deploy-palvelin (ohjelma, joka käynnistää pelipalvelimet) käynnistää jokaisen pelipalvelimen näin. Se
antaa ensin paikkasidonnaiset argumentit ja sitten kolme valitsinta:

```text
Dauntless-Win64-Shipping.exe <gameserver key> <port> <map path>
  <behemoth class | NO_BEHEMOTH> <matchmaker hunt id | NO_MM_HUNTID>
  <uid:huntid,uid:huntid,... | NO_EXPECTED_PLAYERS> <ip:port>
  -EpicPortal -server -nullrhi
```

Ohjelmatiedoston nimen jälkeen tarvitaan vähintään kahdeksan argumenttia. Muuten se näyttää
`INVALID GAMESERVER ARGS` ja sulkeutuu. `<ip:port>`-argumentti jäsennetään, mutta sitä ei käytetä
koskaan. DLL tekee sen jälkeen seuraavat asiat:

1. **Saa `Listen`-kutsun onnistumaan.** Se ei korvaa `UWorld::Listen`-funktiota. Funktion
   `UEngine::LoadMap` sisällä, RVA-osoitteessa `0x372E746`, se kirjoittaa tynkään menevän 5-tavuisen
   kutsun päälle `mov al, 1` ja kolme `nop`-käskyä. `LoadMap` jatkaa sitten ikään kuin kuuntelu
   olisi onnistunut.
2. **Asettaa globaalit liput.** `DllMain` kirjoittaa `GIsServer = 1` ja `GIsClient = 0`. Viisi
   tavupaikkausta `FEngineLoop::PreInit`-vaiheessa estää moottoria kirjoittamasta niiden päälle
   myöhemmin. Aiempi versio kirjoitti liput uudelleen silmukassa. Alkuperäisprojektin (upstream)
   muutos "Don't hog a core writing to GIsServer/GIsClient" korvasi tuon silmukan näillä
   paikkauksilla.
3. **Pakottaa verkkotilan.** Se asettaa `InternalGetNetMode`-funktioon koukun (hook: kohta, jossa
   funktion kutsu ohjataan kulkemaan DLL:n oman koodin kautta), joka palauttaa aina
   `NM_DedicatedServer` (1). Tämän lisänneen upstream-muutoksen otsikko on "Force netmode, fixes
   arrivals and basically ALL weapon bugs".
4. **Hallitsee komentoriviä.** Se asettaa koukun `FCommandLine::Get`-funktioon, joten moottori näkee
   kiinteän rivin `-server -unattended -nullrhi -nosound -EpicPortal -RepDriverDisable`. Moottori ei
   koskaan näe paikkasidonnaisia argumentteja. Koska koukku korvaa sen, mitä `FCommandLine::Get`
   palauttaa, se ohittaa myös sivulla
   [Asiakasohjelman sisäosat]({{ ci_page.url | relative_url }}) kuvatun komentorivin sallittujen
   listan.
5. **Käynnistyy suoraan kartalle.** Se asettaa koukun `UGameMapsSettings::GetGameDefaultMap`-funktioon
   niin, että se palauttaa
   `<map>?MonsterClass=<behemoth>?HuntId=<id>?PlayerHuntIds=<uid:huntid,...>` ja jättää pois
   jokaisen `NO_*`-osan. Pelitilan ohitus tulee kartta-argumentin sisällä muodossa `?game=`.
6. **Avaa verkkoajurin.** Kolme sekuntia sen jälkeen, kun maailma (world) on olemassa,
   `UGameEngine::Tick`-funktion koukku kutsuu `CreateNamedNetDriver("GameNetDriver")`. 1.4.4:n
   asetukset liittävät tuon nimen `IpNetDriver`-ajuriin. Koukku kutsuu sitten `SetWorld`-funktiota ja
   sen jälkeen ehjää `UIpNetDriver::InitListen`-funktiota komentoriviltä saadulla portilla.
7. **Ohjaa liittymisliikenteen tavallisille käsittelijöille.** `InitListen` saa väärän
   ilmoitusosoittimen (notify pointer). `UIpNetDriver::TickDispatch`-funktion koukku kirjoittaa
   ajurin ilmoituskenttään arvon `&World->NetworkNotify` jokaisella päivityskierroksella (tick). Näin
   saapuvat Hello-, Login- ja Join-ohjausviestit menevät tavallisille `UWorld`-käsittelijöille.
   DLL:ssä **ei ole** `PreLogin`- tai `Login`-koukkuja, eikä se koske pelaajan tunnistetietoihin.
   Kättely (handshake) ja pelitilan kirjautumiskäsittely ovat Phoenixin omaa koodia.
8. **Replikoi actorit itse.** `SetReplicationDriver` pakotetaan tyhjäksi (null), eikä moottorin
   replikointisilmukkaa käytetä. Jokaisella päivityskierroksella DLL kokoaa listan olennaisista
   actoreista (Unrealin peliobjekteista) ja kutsuu `CallPreReplication`-funktiota. Jokaiselle
   avoimelle yhteydelle se luo actor-kanavan, jos sellainen puuttuu
   (`CreateChannelByName("Actor")`, `SetChannelActor`), ja kutsuu `ReplicateActor`-funktiota.
   Yhteyden omalle pelaajaohjaimelle (player controller) se kutsuu lisäksi funktiota, jonka uskomme
   olevan `SendClientAdjustment`. Kaksi apukoukkua saa jokaisen actorin näyttämään tason
   alustamalta (level-initialized) jokaisen yhteyden kannalta ja estää jokaista yhteyttä
   näyttämästä kyllästyneeltä (saturated). (Näiden kahden koukun nimet ovat lähdekoodissa
   ristiin.)

Muut palvelinpuolen koukut:

- **Pelaajan aloituspaikan valinta (korjattu lähdekoodissa 1.10.2026, oikean pelin vahvistus puuttuu).** Alkuperäinen hook korvasi Phoenixin oman `GetPlayerStartForPlayer`-valinnan UObject-taulukon ensimmäisellä `APlayerStart`-oliolla. 1.4.4:n alkuperäisen funktion käänteinen analyysi RVA:ssa `0x1368660` näyttää, että Phoenix suodattaa `AArchonPlayerStart::GroupName`-arvolla ja valitsee sopivista paikoista `AArchonPlayerState::PlayerStartSlot`-arvon perusteella. Molempien ohittaminen voi valita väärän aloituspaikan tai jopa väärän maailman; tämä on tällä hetkellä vahvin selitys bugille, jossa kartalta pudonnut pelaaja respawnaa kartan alle. Meidän lähdekoodimme kutsuu nyt ensin Phoenixin alkuperäistä valintaa ja käyttää vain null-palautuksessa varavaihtoehtoa, joka rajoittuu samaan `UWorld`-maailmaan, suosii pyydettyä ryhmää ja käyttää samaa slot-sääntöä. VS2022 x64 -testi-DLL kääntyy onnistuneesti, mutta kartalta putoaminen pitää vielä kokeilla oikeassa pelissä ennen kuin uusi DLL korvaa kaikille jaettavan kiinnitetyn version.
- Kykyjen aktivoinnin RPC-kutsut (etäkutsut) käsitellään suoraan ja välitetään sen jälkeen myös
  alkuperäiselle käsittelijälle.
- Kestävyys (stamina) päivitetään jokaiselle pelaajan hahmolle (pawn).
- `HasFinishedLoading` pakotetaan todeksi.
- Tavupaikkaus, jonka nimi on "Fixup Ramsgate Crash", saa yhden kykysyötteiden taulukon näyttämään
  tyhjältä.

**Miten palvelin puhuu taustapalvelulle.** Palvelintila lisää `x-undaunted-gameserver-apikey`-otsakkeen
jokaiseen prosessin tekemään HTTP-pyyntöön. Metagame (Undauntedin taustapalvelu) hyväksyy tämän
otsakkeen, ja jos pyynnössä on myös pelaajan bearer-tunniste, se kirjaa pyynnön sen pelaajan
nimiin. Palvelintila **ei** ohjaa päätepisteiden URL-osoitteita uudelleen, eikä upstream-koodista
selviä, miten Undauntedin palvelimet tavoittavat metagamen. Meidän palvelinkoneellamme
palvelinprosessit ajetaan samalla Windows-tilillä kuin asiakasohjelma. Ne lukevat saman käyttäjän
`Game.ini`-tiedoston, jossa on lainausmerkeissä olevat ohitukset kaikille 167 päätepisteavaimelle,
ja ne osoittavat metagameen. Sivu [Asiakasohjelman sisäosat]({{ ci_page.url | relative_url }})
kertoo, miksi lainausmerkit ovat välttämättömiä.

**Itsestään sammuminen.** Metsästyspalvelin kutsuu `exit(0)`, kun se on ollut yhteensä 50 sekuntia
ilman yhtään yhteydessä olevaa pelaajaa. Laskuria ei nollata koskaan, joten se laskee myös latausajan
ennen ensimmäisen pelaajan liittymistä. Tarkistus on kiinteästi pois päältä porteille 8776 ja siitä
ylöspäin. Kun `PORT_RANGE_END=8777`, kuten meidän asetuksissamme, nämä ovat Training Dojon ja
Ramsgaten portit. Upstreamin repositorio ei kerro omaa arvoaan; tämä tarkistus viittaa siihen, että
sekin on 8777.

### Asiakastila {#client-mode}

Pelaajan asiakasohjelma lataa saman DLL:n. Asiakastilassa ensimmäinen argumentti on metagamen
`host:port` ilman protokollaa (scheme).

- **Päätepisteet.** `FConfigCacheIni::GetString`-funktion koukku kirjoittaa **167 päätepisteavainta**
  muotoon `http://<metagame>/...`. Jokaisessa asetusosiossa, jonka nimessä on `Mcp`, se myös asettaa
  `Protocol`-arvoksi `http` ja `Domain`/`RedirectUrl`-arvoiksi metagamen. Näin myös Epic-tili- ja
  OAuth-liikenne menee metagameen.
- **Pelin apukeinot.** `HasFinishedLoading` pakotetaan todeksi, Arena- ja Escalation-metsästykset
  avataan, ja **F2**-näppäimeen luodaan `UConsole`.
- **Liittyminen ei tarvitse apua.** Asiakasohjelma siirtyy pelipalvelimelle omalla, tavallisella
  verkkokoodillaan.

---

## Deploy-palvelin {#the-deploy-server}

Deploy-palvelin on pieni Express-sovellus. Upstreamissa sillä on yksi päätepiste,
`POST /api/matchmaker/handle-matchmaking-for-player`, jolla ei ole **mitään tunnistautumista**, ja se
kuuntelee kaikissa verkkoliitännöissä. Meidän haaramme (fork, eli oma muokattu kopiomme) sitoo sen,
samoin kuin metagamen, oletuksena osoitteeseen `127.0.0.1`. Haaramme lisää toisen, pelkästään
lukevan reitin, `GET /gameservers`, joka luettelee käynnissä olevat pelipalvelimet metagamen
`ServerStatus`-vastausta varten. Kummallakaan reitillä ei vieläkään ole tunnistautumista, ja
kumpikin vastaa 403 jokaiselle kutsujalle, joka ei ole loopbackissa tai joka tuli välityspalvelimen
kautta. Yksityiskohdat ovat sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#deploy-server).

| Instanssi | Upstream | Meidän haaramme |
|---|---|---|
| Ramsgate | Yksi jaettu, pysyvä prosessi portissa `PORT_RANGE_END`, käynnistetään heti alussa | Sama, ja käynnistetään uudelleen, kun pelaaja matkustaa sinne sen ollessa kaatunut |
| Training Dojo | Yksi pysyvä prosessi portissa `PORT_RANGE_END - 1`, käynnistetään heti alussa | Käynnistetään, kun joku ohjataan matchmakingin kautta sinne ensimmäisen kerran. `ENABLE_DOJO=1` palauttaa upstreamin toiminnan. |
| Metsästykset | **Yksi prosessi jokaista yhteen koottua ryhmää kohden**, portissa väliltä `PORT_RANGE_BEGIN`–`PORT_RANGE_END - 2` | Sama |

- **Reititys.** Pelitila `CITY` menee Ramsgateen. `SHARED` yhdessä Training Dojon metsästystunnuksen
  kanssa menee Dojoon. `ISLAND` käynnistää uuden metsästyspalvelimen. Kaikki muu ohjataan varalta
  Ramsgateen.
- **Metsästykset.** Pelaajan metsästys vastaa satunnaista matchmakerin metsästystä. Siitä saadaan
  behemoth (pelin hirviö) ja satunnainen kartta metsästyksen karttalistasta sekä `?game=`-ohitus,
  jos metsästys määrittelee sellaisen. Trialit (Trials) valitsevat satunnaisen rivin Hard- tai
  Elite-taulukosta ja käyttävät aina `arena_ramsgate_00`-karttaa.
- **Käynnistäminen.** Palvelinten käynnistykset laitetaan jonoon peräkkäin, ja niiden välissä on
  säädettävä viive.
- **Valvonta.** Kerran minuutissa vahtikoira (watchdog) tarkistaa jokaisen prosessin kutsulla
  `process.kill(pid, 0)`. Jos Ramsgate- tai Dojo-prosessi on kadonnut, se käynnistää uuden. Se ei
  koskaan tapa mitään, koska metsästyspalvelimet sammuttavat itse itsensä.
- **Kaatunut Ramsgate tarvittaessa (haaramme, 23.9.2026, Harmonicin haarasta).** Ennen kuin
  deploy-palvelin antaa Ramsgaten tai Dojon osoitteen (`CITY`-matka, Dojo tai varalla Ramsgate), se
  tarkistaa samalla tavalla, että prosessi on yhä elossa, ja käynnistää kaatuneen ensin
  (`PERSISTENT_WORLD_LIVENESS`, oletuksena päällä; `0` jättää sen vahtikoiralle kuten ennen). Ennen
  pelaaja saatettiin lähettää kaatuneeseen Ramsgateen jopa minuutin ajan. Käynnistys, vahtikoira ja
  pelaajien pyynnöt jakavat **yhden käynnistyksen maailmaa kohden**, joten UDP-porteissa 8777 tai
  8776 ei ole koskaan toista prosessia, ja uudelleen käynnistetty prosessi tallennetaan maailman
  tietueeksi (upstream piti vanhan, kaatuneen tietueen). Tarvittaessa tehty uudelleenkäynnistys odottaa
  käynnistysjonossa kuten metsästys, palvelinpaketilla noin 10 sekuntia.
- **Itse deploy-palvelimen kaatumiset.** Jokaisella pelipalvelinprosessilla on `error`- ja
  `exit`-kuuntelijat: väärä `GAMESERVER_BINARY_PATH` kirjataan lokiin ("Game server on port N failed:
  ...") ja metsästyksen portti palaa käyttöön, kun ennen se kaatoi deploy-palvelimen käsittelemättömään
  virheeseen. Jokaisesta prosessin päättymisestä tulee yksi "exited with code" -rivi. Epäonnistunut
  käynnistys heti alussa on yksi fatal-rivi ja paluukoodi 1, kun prosessi myöhemmin päättyy;
  deploy-palvelin jatkaa palvelemista, ja seuraava matka Ramsgateen yrittää uudelleen.
  Matchmaking-pyyntö, jonka pelipalvelinta ei saada käyntiin (ei vapaata porttia, käynnistysvirhe),
  saa vastauksen 500 `{"error": "no_game_server"}`, ja metagame vastaa pelaajien tilakyselyihin
  `FAILED`.
- **Vastaus.** Deploy-palvelin palauttaa `{host, port}` heti, kun se on käynnistänyt prosessin. Se ei
  odota palvelimen valmistumista. Uusi palvelin alkaa kuunnella vasta noin kolme sekuntia sen
  jälkeen, kun sen maailma on olemassa. Harmonicin haara odottaa ensin pelin UDP-porttia; jätimme sen
  toistaiseksi pois, koska se pitäisi pelaajan matkapyynnön auki koko palvelimen käynnistyksen ajan,
  peliohjelman aikaraja tälle pyynnölle on tuntematon, ja 60 sekunnin takaraja voisi kaataa hitaasti
  käynnistyvän Ramsgaten (tiekartan kohta 4.6). Myös hänen pelipalvelinten lokitiedostonsa jätettiin
  pois: palvelin-DLL antaa pelimoottorille kiinteän komentorivin ja ohjaa sen tulosteen muualle, joten
  tiedostot jäisivät lähes tyhjiksi.

Porttijakomme palvelinkoneella:

| Palvelu | Osoite |
|---|---|
| Metagame | TCP `127.0.0.1:61000` |
| Deploy-palvelin | TCP `127.0.0.1:61001` |
| Pelipalvelimet | UDP 8770–8777. Ramsgate portissa 8777, Dojo portissa 8776, metsästykset porteissa 8770–8775. |

Siirsimme metagamen pois Undauntedin kehitysoletusportista 60000, koska toinen sovellus
palvelinkoneellamme käyttää jo sitä porttia.

---

## Matchmakingista pelipalvelimelle {#the-matchmaking-handoff}

Matchmaking tarkoittaa pelaajien kokoamista samaan peliin. Alla oleva kaavio näyttää, miten viestit
kulkevat asiakasohjelman, metagamen, deploy-palvelimen ja pelipalvelimen välillä. Kaavion tekstit ovat
englanniksi: metsästyksissä odotetaan neljää pelaajaa tai 20 sekuntia viimeisestä liittymisestä.

```text
client                     metagame                      deploy server           game server
  |                           |                               |                       |
  |-- POST /candidate/join -->|                               |                       |
  |                           |  hunts: wait for 4 players,   |                       |
  |                           |  or 20 s after the last join  |                       |
  |                           |-- POST /api/matchmaker/... -->|                       |
  |                           |                               |-- spawn exe + DLL --->|
  |                           |<------- {host, port} ---------|                       |
  |-- GET /candidate/status ->|                               |                       |
  |<-- MATCHING --------------|                               |                       |
  |-- GET /candidate/status ->|                               |                       |
  |<-- IN_PROGRESS + serverInfo {host, port}                  |                       |
  |                                                                                   |
  |== UDP: Hello / Login / Join (stock engine handshake) ============================>|
```

- **Tilavastaus.** Undauntedin `/candidate/status` palauttaa `MATCHING`, kunnes palvelin on
  tiedossa, ja sen jälkeen `IN_PROGRESS` sekä `serverInfo {buildId, gameSessionId, host, port}`. Se
  pyytää asiakasta kyselemään 10 sekunnin välein (`candidateStatusPeriodMillis: 10000`). Meidän
  haaramme vastaa `FAILED` (kentän `statusReason: null` kanssa ja ilman `serverInfo`-kenttää), kun
  deploy-palvelin ei pystynyt antamaan palvelinta: 500 tai 503, vastaus ilman palvelinta, lainausmerkeissä
  oleva portti, runko joka ei ole JSONia tai katkennut yhteys, kukin testattuna Ramsgaten, opetusjakson
  ja jonotetun metsästyksen poluilla (virhetapaukset ovat Harmonicin testeistä). Upstream lähetti ryhmän
  palvelimeen `""` ja porttiin 0.
- **Ryhmittely.** Metsästykset, jotka tarvitsevat matchmakingia, ryhmitellään pelaajan
  metsästystunnuksen mukaan. Ryhmä lähetetään deploy-palvelimelle, kun siinä on 4 pelaajaa tai kun
  viimeisen pelaajan liittymisestä on kulunut 20 sekuntia. 20 sekunnin tarkistus ajetaan vain, kun
  joku asiakas kysyy tilaa osoitteesta `/candidate/status`. Metsästystunnukset, joissa on `Ramsgate`
  tai `Dojo`, sekä tyhjät metsästystunnukset ohittavat jonon ja menevät deploy-palvelimelle heti.
- **Yksi jonottava liittyminen pelaajaa kohden.** Metsästyspalvelin odottaa jokaista pelaajaa, jota
  sen on käsketty odottaa. Siksi pelaajan uusi liittyminen korvaa hänen vanhemman, yhä odottavan
  liittymisensä missä tahansa jonossa, ja samoin tekee liittyminen Ramsgateen, Dojoon tai ryhmän
  metsästykseen. Odotettujen pelaajien luettelossa jokainen tili on vain kerran. Ennen 22.9.2026
  liittyminen, jota ei koskaan yhdistetty, jäi jonoon, saman pelaajan seuraava liittyminen lisättiin
  sen viereen, ja kahden pelaajan metsästys odotti kolmea pelaajaa: ilmalaivan lähtölaskenta jäätyi,
  vaikka molemmat oikeat pelaajat olivat valmiina. Ryhmän jäseniä odotetaan edelleen yhdessä, kutakin
  kerran.
- **Asiakasohjelman puoli (2.1.1).** Selvitimme yksityiskohtaisesti, miten asiakasohjelma käsittelee
  tämän vastauksen, mutta vain versiossa 2.1.1:
  - HTTP-tilakoodin on oltava 200–206.
  - `port`-kentän on oltava JSON-luku. Lainausmerkeissä oleva portti jää arvoon 0, ja siirtyminen
    epäonnistuu silloin hiljaa.
  - `host` ei saa olla tyhjä, tai asiakas jatkaa kyselemistä loputtomiin.
  - `CANCELED` ja `FAILED` lopettavat matchmakingin.
  - Asiakas rakentaa siirtymisosoitteensa (travel URL) muodossa
    `%s:%d?ticket=%s?gameSessionId=%s?EncryptionToken=%s`, jossa jokaisen valinnan välissä on `?`.

  Emme ole johtaneet näitä sääntöjä uudelleen versiolle 1.4.4, vaikka Undauntedin vastaukset
  sopivat niihin.

---

## Mitattu resurssien käyttö {#measured-resource-use}

Nämä luvut koskevat versiota **1.4.4** palvelinkoneellamme, joka on pöytäkone, jossa on 8-ytiminen
suoritin ja 32 Gt muistia. Pelaajia oli yksi:

| Prosessi | Muisti (RAM) | Suoritin (CPU) |
|---|---|---|
| Ramsgate-palvelin (`-nullrhi`) | noin 1,1 Gt | noin 0,2 ydintä |
| Opetusmetsästyksen (tutorial) palvelin (`-nullrhi`) | noin 0,9 Gt | ei mitattu |
| Pelaajan asiakasohjelma | 1,5–1,8 Gt | ei mitattu |

- Emme ole vielä mitanneet 4 pelaajan metsästystä.
- Upstreamin historiassa näkyy suorituskykykorjauksia ("Emergency optimizations", "Small
  optimizations to replication loop").
- Deploy-palvelin käynnistää peliprosessit ilman muistirajaa. Palvelinkohtainen muistivahti on
  tiekartallamme.
- Sama riski tuli esiin versiossa 2.1.1. Rajoittamaton asiakasohjelma kasvoi siellä kaupunkia
  ladatessaan 9 gigatavuun. Kun tekstuurien suoratoistopoolia (texture streaming pool) rajoitettiin
  ja skaalautuvuusasetuksia laskettiin, käyttö pysyi noin 2,8 gigatavussa.

---

## Miksi sanoimme kerran, että se on mahdotonta {#why-we-once-said-it-was-impossible}

2.1.1-analyysimme todisti yksittäisiä käskyjä myöten, että `UWorld::Listen` on tynkä, että
`GetNetMode` voi vastata vain "asiakas", että `GIsClient` pakotetaan päälle ja ettei
ohjelmatiedostossa ole `WindowsServer`-alustanimeä. Siitä päättelimme, ettei mikään komentorivi,
asetus tai paikkaus voisi saada asiakasohjelmaa isännöimään ja että oikea moninpeli vaatisi
palvelinkäännöksen, jota Phoenix ei koskaan julkaissut.

Todisteet pitivät paikkansa. Johtopäätös ei. Tulkitsimme lauseen "tavallinen koodipolku ei pääse
tähän koodiin" tarkoittavan "tätä koodia ei voi ajaa". Mielemme muutti tieto, että muut pelaajat
pääsivät matchmakingin kautta Ramsgateen samoilla tiedostoilla. Undaunted näyttää, mitä versiossa
1.4.4 pohjimmiltaan tarvitaan: yksi paikattu kutsu, viisi tavupaikkausta, yksi verkkotilan koukku,
suora kutsu ehjään `InitListen`-funktioon, korjaus verkkoajurin ilmoitusosoittimeen ja käsin
kirjoitettu replikointisilmukka. Sen muut koukut hoitavat komentorivin, käynnistyskartan ja pelin
yksityiskohdat.

Emme ole kokeilleet samaa versiossa 2.1.1. Mikään Undauntedin siirtymistä ei käy sellaisenaan.
Löytämämme vastaavat 2.1.1-funktiot (`Listen`-tynkä osoitteessa `0x140bfde50`, `GetNetMode`
osoitteessa `0x1453e4ef0`, `GIsClient`-kirjoitus osoitteessa `0x140c0f683`, `InitListen` osoitteessa
`0x140c6d200`) viittaavat siihen, että sama lähestymistapa toimisi siellä. Se on
**vahvistamatonta**.

---

## Avoimet kysymykset {#open-questions}

- **Välitys-DLL:n lähdekoodi.** `dxgi.dll`-välitys-DLL:n lähdekoodia ei ole julkaistu. Aiomme
  kirjoittaa oman.
- **Valmiiksi käännetyt tiedostot.** Emme ole varmistaneet, että valmiiksi käännetty
  `UndauntedInternalServer.dll` on käännetty julkaistusta lähdekoodista. Kiinnitämme käyttämiemme
  valmiiden DLL-tiedostojen tiivisteet ja aiomme kääntää ne itse lähdekoodista.
- **Kaksinkertainen kyvyn aktivointi.** Palvelimen RPC-koukku aktivoi kyvyn suoraan ja kutsuu sitten
  myös alkuperäistä RPC:tä. Sitä, aktivoiko tämä kyvyt kahdesti, ei ole testattu.
- **Neljä pelaajaa.** Undauntedin historian mukaan tavalliset ja 4 pelaajan metsästykset
  toimivat, mutta olemme itse ajaneet vain opetusmetsästystä ja Ramsgatea.
