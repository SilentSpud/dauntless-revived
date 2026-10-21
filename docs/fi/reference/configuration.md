---
title: Asetukset
parent: Tekninen viite
grand_parent: Dauntless Revived suomeksi
nav_order: 1
description: "Dauntless Revived -palvelinten kaikki ympäristömuuttujat: oletukset, sallitut arvot, mitä kukin tekee, kuka sen asettaa ja mitkä kytkimet ovat oletuksena päällä."
lang: fi
ref: reference/configuration
locale: fi_FI
---

{% assign host_page = site.pages | where: "path", "fi/setup/host.md" | first %}
{% assign admin_page = site.pages | where: "path", "fi/setup/admin.md" | first %}
{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}
{% assign winserver_page = site.pages | where: "path", "fi/setup/windows-server.md" | first %}
{% assign upgrade_page = site.pages | where: "path", "fi/setup/upgrading.md" | first %}
{% assign legal_page = site.pages | where: "path", "fi/legal.md" | first %}
{% assign ports_page = site.pages | where: "path", "fi/reference/ports.md" | first %}
{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign gamesettings_page = site.pages | where: "path", "fi/reference/game-settings.md" | first %}
{% assign scripts_page = site.pages | where: "path", "fi/reference/scripts.md" | first %}
{% assign dev_page = site.pages | where: "path", "fi/reference/development.md" | first %}

# Asetukset
{: .no_toc }

Dauntless Revivedin jokainen palvelinosa saa asetuksensa ympäristömuuttujista. Millään osalla ei ole
omaa asetustiedostomuotoa: kehityskoneella kukin osa lukee oman kansionsa `.env`-tiedoston, ja
Windows-palvelinpaketilla asennetulla palvelimella kukin lukee oman tiedostonsa kansiosta
`C:\DauntlessRevived\data\config\`. Tällä sivulla on jokainen muuttuja, jonka jokin osa lukee: mitä
puuttuva arvo tarkoittaa, kuka arvon yleensä asettaa ja mitkä arvot ovat salaisia.

Tämä on viitesivu. Toimivan asennuksen saat vaihe vaiheelta sivuilta
[Pystytä palvelin]({{ host_page.url | relative_url }}) (yksi kone),
[Palvelin ryhmälle]({{ admin_page.url | relative_url }}) (Tailscale) ja
[Windows-palvelin]({{ winserver_page.url | relative_url }}) (vuokrattu palvelin). Porttinumerot ovat
sivulla [Portit ja verkko]({{ ports_page.url | relative_url }}), pelin omat ini-tiedostot ja
komentorivit sivulla [Pelin asetukset]({{ gamesettings_page.url | relative_url }}) ja skriptien
parametrit sivulla [Skriptit ja parametrit]({{ scripts_page.url | relative_url }}).

**Salaisuudet.** **Salaiseksi** merkityillä riveillä on arvoja, joilla voi kirjautua minä tahansa
pelaajana, esiintyä pelipalvelimena tai ohjata palomuuria hoitavaa apuria. Älä koskaan jaa niitä, älä
koskaan tallenna niitä versionhallintaan äläkä liitä tai tulosta niitä mihinkään. Tämän sivun
esimerkit ovat paikkamerkkejä.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

---

## Miten asetukset luetaan {#how-settings-are-loaded}

| Osa | Kansio | Käynnistys | Tiedosto kehityskoneella | Tiedosto paketin palvelimella |
|:----|:-------|:-----------|:-------------------------|:------------------------------|
| Metagame | `legacy/UndauntedMetagame/` | `npm start` = `node --env-file=.env dist/server.js`; `npm run dev` = `tsx watch --env-file=.env src/server.ts` | `legacy/UndauntedMetagame/.env` (pakollinen, muuten Node ei käynnisty) | `data\config\metagame.env` |
| Deploy-palvelin | `legacy/UndauntedDeployServer/` | `npm start` = `node --env-file=.env dist/server.js`; `npm run dev` = `tsx --env-file=.env src/server.ts` | `legacy/UndauntedDeployServer/.env` (pakollinen) | `data\config\deployserver.env` |
| Yhdyskäytävä | `legacy/UndauntedGateway/` | `npm start` = `node --env-file-if-exists=.env dist/server.js` | `legacy/UndauntedGateway/.env` (valinnainen) | `data\config\gateway.env` (vain julkisessa tilassa) |
| Sallittujen listan apuri | `legacy/UndauntedGateway/` | `npm run start:allowlist` = `node --env-file-if-exists=.env dist/allowlist/server.js` | sama `legacy/UndauntedGateway/.env` | `data\config\allowlist.env` (vain julkisessa tilassa) |
| Sisältöpalvelin | `legacy/UndauntedContent/` | `npm start` = `node --env-file-if-exists=.env dist/server.js` (myös `npm run verify`) | `legacy/UndauntedContent/.env` (valinnainen) | `data\config\content.env` (kun sisältöpalvelin on asennettu) |
| Käynnistin | `legacy/UndauntedLauncher/` | asennettu sovellus tai `npm start` | ei mitään: se lukee vain oman prosessinsa ympäristön | ei palvelimella |

Säännöt, jotka koskevat kaikkia osia:

- **Komentotulkin arvo voittaa tiedoston.** Noden `--env-file` ei koskaan korvaa muuttujaa, joka on
  jo asetettu ympäristössä. PowerShell-istuntoon jäänyt `PORT` tai `NODE_ENV` (`$env:PORT`) ohittaa
  `.env`-tiedoston arvon huomaamatta. Tarkista komennolla `Get-ChildItem Env:` ja poista muuttuja
  esimerkiksi komennolla `Remove-Item Env:PORT`. Metagamen `npm run db:generate` lukee `.env`-tiedoston
  `dotenv`-kirjastolla, joka toimii samoin.
- **Paketti antaa tiedoston voittaa.** `Stack.ps1` käynnistää jokaisen osan komennolla
  `node --env-file=<root>\data\config\<tiedosto>.env <skripti>`, ja työhakemistona on osan kansio
  `<root>\app\`-kansion alla. Sitä ennen se poistaa omasta prosessistaan jokaisen muuttujan, jonka
  tiedosto nimeää, joten mikään peritty arvo ei voi ohittaa tiedostoa.
- **Työhakemistolla on väliä.** Suhteelliset polut (`DB_FILENAME`, `BODY_LOG_FILE`,
  `ALLOWLIST_AUDIT_LOG`, `GATEWAY_CERT` ja niin edelleen) lasketaan siitä, ja metagame löytää
  tietokantamigraationsa polusta `./src/drizzle`. Käynnistä jokainen osa sen omasta kansiosta.
- **Tiedostomuoto.** Rivejä muotoa `KEY=value`, kommenttirivit alkavat merkillä `#`. Tallenna
  tiedosto ASCII-muodossa tai UTF-8-muodossa ilman BOM-merkkiä (byte-order mark). Windows PowerShell
  5.1:ssä kirjoita se komennolla `Set-Content -Encoding ascii` tai `Add-Content -Encoding ascii`;
  `>` ja `>>` kirjoittavat UTF-16-muodossa, jota Node ei osaa lukea.
- **Käynnistä uudelleen jokaisen muutoksen jälkeen.** Jokainen osa lukee ympäristönsä
  käynnistyessään (osan arvoista kerran, osan jokaisella pyynnöllä, mutta käynnissä olevan prosessin
  ympäristö ei koskaan muutu). Muuta tiedostoa ja käynnistä sitten kyseinen osa uudelleen: käsin kuten
  sivulla [Pystytä palvelin]({{ host_page.url | relative_url }}#stopping), tai paketin palvelimella
  komennolla `C:\DauntlessRevived\bin\Stack.ps1 restart -Only <osa>` (`metagame`, `content`,
  `deploy`, `gateway` tai `allowlist`). Kaksi asiaa muuttuu ilman uudelleenkäynnistystä: ylläpitäjä
  voi vaihtaa `REGISTRATION_MODE`-tilan ajon aikana (vain muistissa; uudelleenkäynnistys palauttaa
  tiedoston arvon), ja sisältöpalvelin lukee uutistiedoston ja kuvapaketin kansion uudelleen itse.
- **Puuttuva ja tyhjä arvo eivät aina ole sama asia.** Yhdyskäytävässä, sallittujen listan apurissa
  ja sisältöpalvelimessa `KEY=` (tai pelkät välilyönnit) lasketaan asettamattomaksi. Metagamessa ja
  deploy-palvelimessa useimmat arvot käytetään sellaisinaan: `PORT=` kuuntelee satunnaisessa vapaassa
  portissa, ja `ENTITLEMENTS_DEFAULT=` tarkoittaa, ettei oletusoikeuksia ole lainkaan. Jätä rivi
  kokonaan pois, niin saat oletuksen.
- **Aloita `.env.example`-tiedostosta.** Jokaisessa neljässä palvelinkansiossa on kommentoitu
  `.env.example`, jossa on jokainen osan lukema muuttuja oletusarvoineen. Se on valmiiksi asetettu
  kehitykseen yhdellä koneella (kaikki osoitteessa `127.0.0.1`, sallittujen listan apuri
  kuivaharjoitustilassa). Kopioi se nimelle `.env` ja täytä tyhjät kohdat: salaisuudet sekä
  pelikansio tai pelin ohjelmatiedoston polku, kun sellaista tarvitaan. Palvelimelle, jota muutkin
  käyttävät, seuraa sen sijaan sivua [Pystytä palvelin]({{ host_page.url | relative_url }}#metagame).
  Git ohittaa `.env`-tiedoston missä tahansa repositoriossa, ja metagamen, deploy-palvelimen,
  yhdyskäytävän ja sisältöpalvelimen kansioissa myös `.env.*`-tiedostot (paitsi `.env.example`).

---

## Ominaisuuskytkimet {#feature-switches}

Jokainen alla oleva kytkin muuttaa yhtä toimintoa. Forkin omat korjaukset ovat oletuksena päällä, ja
jokaisesta pääsee vertailuja varten takaisin alkuperäisen projektin toimintaan. Muutokset, jotka ovat
vielä todistamattomia, riskialttiita tai vain kehitystä varten, ovat pois päältä.

**Oletuksena päällä:**

| Kytkin | Osa | Oletus | Näin pois päältä | Huomioita |
|:-------|:----|:-------|:-----------------|:----------|
| `PROGRESSION_MODE` | metagame | oikea eteneminen jokaiselle tilille | `PROGRESSION_MODE=stub` (alkuperäisen projektin valemaksimitasot) | Oletus muuttui: palvelin, jonka asetuksissa ei ole tätä riviä, oli ennen tyngän varassa. Lue [päivitysohjeet]({{ upgrade_page.url | relative_url }}#real-progression-default) ennen kuin päivität palvelimen, jolla on jo pelaajia. |
| `ENTITLEMENTS_DEFAULT` | metagame | jokaisella tilillä on Elite Hunt Pass | aseta oma luettelosi | Vain tileille, joilla on oikea eteneminen. |
| `INVENTORY_REPORT_REMOVALS` | metagame | päällä | `0` | `0` palauttaa alkuperäisen projektin virheen, jonka takia varusteiden parantaminen ei maksanut mitään. |
| `MISC_ROUTES` | metagame | päällä | `0` | Kaverilista, estolista ja muutama muu reitti vastaavat 404:n sijaan. |
| `STATUS_EXTRA` | metagame | päällä | `0` | Palvelimen nimi, versio, commit ja lähdekoodilinkki `/dauntless-status`-vastauksessa. |
| `ACCOUNT_DISPLAY_NAME` | metagame | päällä | `0` | Oikeat käyttäjänimet tili- ja ryhmävastauksissa. |
| `ACCOUNT_MAPPING` | metagame | päällä | `0` | Tilien yhdistäminen kaverin lisäämistä ja nimellä kutsumista varten. `0` ei yhdistä mitään, kuten ennen. |
| `GUILDS` | metagame | päällä | `0` | Kiltareitit. `0` palauttaa vanhat tyngät: kukaan ei voi perustaa kiltaa tai liittyä siihen. |
| `GUILD_RESERVED_NAMES` | metagame | päällä | `0` | Henkilökunnan ja projektin sanat torjutaan kiltojen nimissä ja nimikylteissä. |
| `PARTY_SOLO_STUB` | metagame | päällä (alkuperäisen projektin paikkamerkki yhden hengen ryhmälle) | `0` | `0`, jos Invite to Party ei tee mitään yksin olevalle pelaajalle. |
| `CHAT_NICK_CHECK` | metagame | `enforce`: chat-huoneeseen liittyminen hylätään, jos nimimerkki rikkoo nimisääntöjä | `log` (päästä sisään ja varoita; toisen tilin tunnus hylätään silti) | Vain paluukytkin siltä varalta, että oikea peliohjelma hylätään. Merkitsee jotain vain, kun `CHAT=1`. |
| `PROGRESSION_CONFIRM` | metagame | päällä | `off` | Vain vianetsintään. |
| `LOG_REQUESTS` | metagame | päällä | `0` | Pyyntöloki on tärkein vianetsintävälineemme; pidä se päällä. |
| `SLAYER_LINKS` | metagame | päällä | `0` | Slayer Links. `0` palauttaa 404-vastauksen, jonka jokainen `/slayerlink`-reitti sai ennen; tallennetut kutsut ja linkit säilyvät. |
| `BALANCE_FROM_INVENTORY` | metagame | päällä | `0` | `/balance` ja `/reconcile` kertovat valuutat, jotka hahmolla on. `0` palauttaa kiinteän taulukon (Notes tietokannasta, muut 0). |
| `PROGRESSION_REPLAY_WINDOW_S` | metagame | `5` (sekuntia) | `0` | Pelipalvelimen myöntöön, joka toistuu tavu tavulta alle 5 sekunnin kuluttua, vastataan, mutta sitä ei lisätä uudelleen. |
| `PERSISTENT_WORLD_LIVENESS` | deploy-palvelin | päällä | `0` (vahtikoira käynnistää Ramsgaten; Dojo käynnistyy tarvittaessa) | Kaatunut Ramsgate tai Dojo käynnistetään uudelleen, kun pelaaja matkustaa sinne. |
| `GATEWAY_ALLOWLIST` | yhdyskäytävä | päällä | `0` | Hätäkatkaisin: kun se on pois päältä, kenenkään peliportit eivät aukea. |
| `ENABLE_DOJO` | deploy-palvelin | Dojo käynnistyy ensimmäisellä käyttökerralla | `1` käynnistää sen heti alussa, kuten alkuperäinen projekti | Säästää yhden peliprosessin silloin, kun kukaan ei harjoittele. |

**Oletuksena pois päältä:**

| Kytkin | Osa | Näin päälle | Miksi se on pois päältä |
|:-------|:----|:------------|:------------------------|
| `CHAT` | metagame | `1` | Pelin tekstichat. Rakennettu ja testattu ilman peliä, kaksi pelaajaa ei ole vielä kokeillut; se tulee oletuksena päälle tämän testin jälkeen. |
| `ESCALATION_MODE` | metagame | `real` | Oikeat Escalation-tallennukset. Käyttöönotto pudottaa jokaisen pelaajan tekaistusta maksimista (taso 25) tasolle 0: ylläpitäjän päätös pelitestin jälkeen. |
| `ESCALATION_STRICT` | metagame | `1` | Saa mallinnuksestamme riippuvat Escalation-säännöt torjumaan tallennuksen varoittamisen sijaan. Kun lokit pysyvät puhtaina. |
| `STORE` | metagame | `free` | Ilmainen pelin kauppa. Odottaa päätöstä ilmaisesta tai hinnoitellusta (tiekartan kohta 3.7) ja kaupan testiä pelissä. |
| `STORE_REPEATABLE_TOKENS` | metagame | `1` | Myy palkkiotehtävien tunnisteiden pakettia rajattomasti: rajattomasti ilmaisia premium-palkkiotehtäviä. Ylläpitäjän päätös. |
| `PROGRESSION_CONFIRM_ENTITLEMENTS` | metagame | `1` | Tason vahvistus antaa myös tason pysyvät oikeudet. Vain jos pelitesti näyttää, ettei pelipalvelin koskaan anna niitä itse. |
| `VERIFY_STUB_ACCOUNT` | metagame | `1` | Vain paluuta varten: vanha paikkamerkkitili vastauksessa `oauth/verify`. |
| `MATCHMAKING_CANCEL` | metagame | `1` | Kokeellinen. Peliohjelma lähettää perumisen heti jokaisen jonoon liittymisen jälkeen, ja metsästykset alkavat vain siksi, että perumiseen vastataan 404. |
| `ACCOUNTINFO_PUBLIC_LEGACY` | metagame | `1` | Vain paluuta varten: alkuperäisen projektin tilitietovastaus, joka piilottaa muut pelaajat (ryhmäkutsut eivät koskaan näy). |
| `GUILD_CREATE_ACTIVITY_FALLBACK` | metagame | `1` | Heikompi tarkistus killan perustamiselle, vain jos oikea testi näyttää, ettei peliohjelma koskaan tarkista lopullista nimeä. |
| `INVENTORY_REFUSE_OVERSPEND` | metagame | `1` | Todistamaton. Torjunta hylkää koko tavarapyynnön palkintoineen, eikä yhtään metsästyksen lopun pyyntöä ole vielä tarkistettu sitä vasten. |
| `PROGRESSION_ALLOW_DELETE` | metagame | `1` | Antaa minkä tahansa pelipalvelinkutsun pyyhkiä pelaajan etenemisradan. |
| `DB_WAL` | metagame | `1` | Ohjeiden varmuuskopiot kopioivat pelkän tietokantatiedoston. |
| `LOG_BODIES` | metagame | `1` | Kehitysaikainen tallenne siitä, mitä pelaajat lähettävät. Paketti pakottaa sen pois päältä julkisessa tilassa. |
| `AUTH_MODE=NONE` | metagame | `NONE` | Vain kehitykseen: kuka tahansa voi kirjautua kenenä tahansa. Ei vaikutusta asetuksella `NODE_ENV=production`. |
| `REGISTRATION_MODE=OPEN` | metagame | `OPEN` | Kuka tahansa, joka tavoittaa metagamen, saa tilin. Vain niin kauan kuin metagame kuuntelee loopbackissa. |
| `CONTENT_ALLOW_ANY_BIND` | sisältöpalvelin | `1` | Antaa sisältöpalvelimen kuunnella julkisessa osoitteessa. |
| `ALLOWLIST_ALLOW_PRIVATE` | sallittujen listan apuri | `1` | Vain testeihin ja lähiverkkoasennuksiin; paketti asettaa sen hiekkalaatikkoasennuksissa. |

`ALLOWLIST_DRY_RUN`-asetuksella ei ole oletusta: sallittujen listan apuri ei käynnisty, ennen kuin
arvo on `1` (kirjaa lokiin, mitä se tekisi; mikä tahansa testikone) tai `0` (muuttaa palomuuria;
palvelin).

---

## Metagame (`legacy/UndauntedMetagame/`) {#metagame}

Taustapalvelu, jonka kanssa peli keskustelee: kirjautumiset, hahmot, tavarat, eteneminen, matchmaking
ja `/undaunted/api`-hallintareitit. Valmis `.env` yhdelle koneelle on sivulla
[Pystytä palvelin]({{ host_page.url | relative_url }}#metagame).

**Pakolliset, ei oletusta:** `PORT`, `AUTH_MODE`, `AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64`,
`DB_FILENAME`, `REGISTRATION_MODE`, `MATCHMAKING_MODE`, `DEPLOYSERVER_URL`, `QOS_TARGET_URL` ja
`TARGET_CHANGELIST`. Puuttuva `PORT` tai allekirjoitusavain pysäyttää metagamen käynnistyessä. Muut
eivät pysäytä: metagame käynnistyy ja epäonnistuu sitten taulukoissa kuvatuilla tavoilla.

**Milloin luetaan:** käynnistyessä. Käynnistä metagame uudelleen jokaisen muutoksen jälkeen.
Uudelleenkäynnistys ei kirjaa ketään ulos (istuntotunnisteet säilyvät), mutta matchmaking-jonot,
ryhmät (party) ja tieto paikalla olevista pelaajista katoavat. Killat, kaveruudet ja estot on
tallennettu, ja ne säilyvät.

**Käynnistyessään** metagame kirjaa lokiin osoitteen, jossa se kuuntelee, sekä rivin
`Progression mode: real for every account (the default)` (tai sen, missä tilassa se on). Oikean
etenemisen tilassa se myös varoittaa niin kauan kuin on pelaajia, joilla ei vielä ole tallennettua
etenemistä; katso [päivitysohjeet]({{ upgrade_page.url | relative_url }}#real-progression-default).
Kun `GATEWAY_SECRET` on asetettu, se tekee julkisen tilan tarkistukset, jotka on lueteltu kyseisen
muuttujan kohdalla.

Sen jälkeen tulee kaksi riviä Harmonicin forkin siirron mukana tulleista asetuksista:

```text
features: bodyLogPerPath=no-cap escalation=stub escalationStrict=off store=off storeRepeatableTokens=off replayWindow=5s confirmEntitlements=off balanceFromInventory=on slayerLinks=on chatPresence=on verifyStubAccount=off
Progression config: bundled, 10 tracks; active Hunt Pass season09b
```

Nuo kytkimet (`BODY_LOG_PER_PATH`, `ESCALATION_MODE`, `ESCALATION_STRICT`, `STORE`,
`STORE_REPEATABLE_TOKENS`, `PROGRESSION_REPLAY_WINDOW_S`, `PROGRESSION_CONFIRM_ENTITLEMENTS`,
`BALANCE_FROM_INVENTORY`, `SLAYER_LINKS`, `CHAT_PRESENCE` ja `VERIFY_STUB_ACCOUNT`) käyttävät yhtä
jäsennintä: päälle käy `1`, `true`, `on` tai `yes` ja pois `0`, `false`, `off` tai `no`,
kirjainkoolla ei ole väliä; lukumäärät ovat kokonaislukuja; puuttuva tai tyhjä on oletus. Arvo, jota
ei voi jäsentää, kirjaa yhden varoituksen `<NIMI>="<arvo>" is not a valid value; using the default
(<oletus>)`, ja käytetään oletusta. Virheellinen `PROGRESSION_CONFIG_DIR` tai `ACTIVE_HUNT_PASS`
sen sijaan pysäyttää metagamen (`The progression config could not be loaded: <syy>`, paluukoodi 1)
ennen kuin tietokanta avataan.

### Kuuntelu, kirjautumiset ja tietokanta {#metagame-core}

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `PORT` | ei oletusta. Puuttuu: metagame pysähtyy käynnistyessään porttivirheeseen. Tyhjä: se kuuntelee satunnaisessa vapaassa portissa. | 1-65535 | HTTP-palvelimen TCP-portti. Jos portti on jo varattu, metagame pysähtyy virheeseen `Could not listen on ...`. | Sinä (Pystytä palvelin -ohjeessa 61000); paketti: aina (61000, hiekkalaatikkotilassa 62000, tai `-MetagamePort`) |
| `BIND_HOST` | `127.0.0.1` (myös tyhjänä) | yksi IP-osoite | **Vain forkissa.** Se yksi osoite, jossa metagame kuuntelee. `127.0.0.1` yhdellä koneella ja julkisessa tilassa (yhdyskäytävän takana); palvelinkoneen Tailscale-osoite (100.x.y.z) yksityisessä tilassa. Vältä osoitetta `0.0.0.0`: metagame kuuntelisi silloin myös lähiverkossasi. | Sinä; paketti: aina (`127.0.0.1`, yksityisessä tilassa Tailscale-osoite) |
| `NODE_ENV` | puuttuu: kehitystila | `production` tai mikä tahansa muu | `production`: jokainen lokirivi on yksi JSON-olio, `AUTH_MODE=NONE` ei vaikuta mitään, eikä virhesivuilla ole pinojälkiä (stack trace). Mikä tahansa muu: värilliset, luettavat lokit. Käytä arvoa `production` aina, kun muut ihmiset voivat tavoittaa palvelimen. | Sinä; paketti: aina `production` |
| `AUTH_MODE` | ei oletusta. Puuttuu tai tuntematon: kirjautumiset eivät saa vastausta lainkaan (peli jää odottamaan), ja jokainen tiliavaimen tarkistava reitti vastaa 500. | `APIKEY` tai `NONE` (tarkalleen) | Miten kirjautumiset ja tiliavaimet tarkistetaan. `APIKEY`: pelin kirjautumisessa on mukana pelaajan tiliavain (`UUK_...`), jota verrataan tallennettuihin SHA-256-tiivisteisiin; pelaaja saa 24 tuntia voimassa olevan istuntotunnisteen. `NONE`: vain kehitykseen. Mikä tahansa tunniste kelpaa kirjautumiseen ja mikä tahansa tilitunnus API-avaimeksi, myös ylläpitoreiteillä. `NONE` ohitetaan, kun `NODE_ENV=production` (kirjautumiset eivät silloin saa vastausta), se torjutaan kaikelta välityspalvelimen (proxy) kautta tulleelta, eikä metagame käynnisty sen kanssa, kun `GATEWAY_SECRET` on asetettu. | Sinä (`APIKEY`); paketti: aina `APIKEY` |
| `AUTH_SIGNING_PRIVKEY_B64` | ei oletusta. Puuttuu: metagame pysähtyy käynnistyessään. Tyhjä: se käynnistyy, mutta jokainen kirjautuminen epäonnistuu. | PEM-muotoisen RSA-yksityisavaimen (PKCS#8) base64 | **Salainen: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Allekirjoittaa pelaajien istuntotunnisteet (RS256, voimassa 24 tuntia). Sen haltija voi kirjautua minä tahansa tilinä. Avainparin vaihtaminen vain kirjaa kaikki ulos. Varmuuskopioi se salattuna. | Sinä ([Pystytä palvelin]({{ host_page.url | relative_url }}#metagame) näyttää, miten se luodaan); paketti: otetaan olemassa olevasta tiedostosta tai varmuuskopiosta, muuten luodaan kerran |
| `AUTH_SIGNING_PUBKEY_B64` | ei oletusta. Puuttuu: metagame pysähtyy käynnistyessään. | vastaavan PEM-muotoisen julkisen avaimen (SPKI) base64 | Tarkistaa jokaisen istuntotunnisteen. Ei salainen, mutta sen on oltava yksityisavaimen pari, tai jokainen kirjautuneen pelaajan pyyntö epäonnistuu. | Luodaan yhdessä yksityisavaimen kanssa |
| `REGISTRATION_MODE` | ei oletusta. Puuttuu tai tuntematon: tilin luonti vastaa 500. | `NONE`, `INVITECODE` tai `OPEN` (tarkalleen) | Kuka saa luoda tilin (`POST /undaunted/api/Register`). `NONE`: ei kukaan (400). `INVITECODE`: vain käyttökelpoisella kutsukoodilla (muuten 401). `OPEN`: kuka tahansa, joka tavoittaa metagamen, joten käytä sitä vain, kun metagame kuuntelee osoitteessa `127.0.0.1`. Ylläpitäjä voi vaihtaa tilan ajon aikana; muutos kestää seuraavaan uudelleenkäynnistykseen. | Sinä (`OPEN` yhdellä koneella); paketti: aina `INVITECODE` (käsin asetettu arvo kirjoitetaan yli jokaisella asennuskerralla) |
| `DB_FILENAME` | ei oletusta. Puuttuu tai tyhjä: väliaikainen tietokanta, joka poistetaan, kun metagame sulkeutuu. | tiedostopolku kauttaviivoin; suhteellinen polku lasketaan työhakemistosta | SQLite-tietokanta: tilit, avainten tiivisteet, tallennukset, eteneminen. Sen kansion on oltava olemassa. Odottavat migraatiot ajetaan jokaisella käynnistyksellä ottamatta ensin varmuuskopiota, joten ota varmuuskopio ennen päivitystä. Tiedosto on yksityinen: siinä ovat käyttäjänimet, tallennukset, avainten tiivisteet ja seuraavaan käynnistykseen asti rekisteröintiä odottavat avaimet selväkielisinä. Myös `npm run db:generate` lukee sen `.env`-tiedostosta. | Sinä (Pystytä palvelin -ohjeessa `C:/dr/data/undaunted.db`); paketti: aina (`<root>/data/undaunted.db`) |
| `DB_WAL` | pois | `1` tai mikä tahansa muu | `1` vaihtaa SQLiten WAL-lokitilaan (journal mode). Pois päältä, koska ohjeiden varmuuskopiot kopioivat pelkän tietokantatiedoston ja paketti pysäyttää metagamen kovakouraisesti, jolloin uusimmat tallennukset voisivat jäädä vain erilliseen `-wal`-tiedostoon. Kun asetus on pois, WAL-tilaan jäänyt tietokanta vaihdetaan takaisin käynnistyksessä. | Oletuksena ei kukaan; paketti: säilyttää |

### Matchmaking ja peliohjelma {#metagame-matchmaking}

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `MATCHMAKING_MODE` | ei oletusta. Puuttuu tai tuntematon: jokainen matchmaking-pyyntö torjutaan (400). | `DEPLOYSERVER` tai `DISABLED` (tarkalleen) | `DEPLOYSERVER`: pelipalvelimet tulevat deploy-palvelimelta. `DISABLED`: kaikki matchmaking torjutaan. | Sinä; paketti: aina `DEPLOYSERVER` |
| `DEPLOYSERVER_URL` | ei oletusta. Puuttuu: jokainen pelipalvelimen käynnistys epäonnistuu, eikä palvelimen tilassa näy maailmoja. | `host:port` ilman `http://`-alkua | Missä deploy-palvelin kuuntelee; metagame lisää `http://`-alun ja polun. Pidä se osoitteessa `127.0.0.1`: deploy-palvelimessa ei ole tunnistautumista, ja se torjuu kaikki muut kutsujat. | Sinä (`127.0.0.1:61001`); paketti: aina `127.0.0.1:<deploy-palvelimen portti>` |
| `MATCHMAKING_DEPLOY_TIMEOUT_MS` | `240000` (4 minuuttia) | kokonaisluku millisekunteina, 1000-900000; virheellinen arvo tuottaa varoituksen ja käyttää oletusta | Rajoittaa koko pelipalvelimen varauspyynnön kestoa, myös kesken jäävää JSON-vastausta. Aikakatkaisu päättää ehdokkaan tilaan `FAILED`, eikä käynnistä automaattisesti toista palvelinta. Pelaajan uusi liittymispyyntö voi yrittää uudelleen. Varaa aikaa työntekijän käynnistymiselle ja asetetulle käynnistysvälille. | Valinnainen; paketti säilyttää |
| `QOS_TARGET_URL` | ei oletusta. Puuttuu: alueluettelossa, jota peliohjelma pingaa, on vain `null` (mitä peliohjelma silloin tekee, on testaamatta). | täysi URL, joka päättyy `/QoS` | Se yksi ”alue”, jota peliohjelma pingaa ennen matchmakingia; metagame vastaa itse `GET /QoS`-pyyntöön. Yksi kone: `http://127.0.0.1:61000/QoS`. Yksityinen tila: sama Tailscale-osoitteella. Julkinen tila: `http://127.0.0.1:61000/QoS`, joka on jokaisen kaverin oma käynnistimen välitin. | Sinä; paketti: aina (julkinen: `http://127.0.0.1:61000/QoS`; yksityinen: `http://<Tailscale-osoite>:<metagamen portti>/QoS`) |
| `TARGET_CHANGELIST` | ei oletusta. Puuttuu: koontiversion tunnisteeksi tulee `undefined_1.4.4_shipping`. | `239827` | Lähetetään peliohjelmalle koontiversion tunnisteena `<changelist>_1.4.4_shipping` siinä vastauksessa, joka ohjaa sen pelipalvelimelle. Ei tiedetä, tarkistaako 1.4.4-peliohjelma sitä; pidä arvona `239827`, `Version.txt`-tiedoston muutoslistan numero. | Sinä; paketti: aina `239827` |

### Julkinen tila ja pelipalvelinten kutsut {#metagame-public-mode}

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `GATEWAY_SECRET` | puuttuu tai tyhjä: julkinen tila pois päältä, välitysotsakkeisiin ei koskaan luoteta | 32-256 tulostettavaa merkkiä ilman välilyöntejä (yhdyskäytävä vaatii tämän; paketti tekee 64 heksamerkkiä) | **Salainen: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Ottaa julkisen tilan käyttöön. Vain loopbackista tuleva pyyntö, jonka `X-Dauntless-Gateway`-otsake on sama kuin tämä arvo, lasketaan yhdyskäytävän välittämäksi, ja vain silloin sen `X-Forwarded-For` otetaan pelaajan osoitteeksi. Arvon on oltava sama kuin yhdyskäytävän asetusten `GATEWAY_SECRET`. Käynnistyksessä muu `AUTH_MODE` kuin `APIKEY` pysäyttää metagamen; alle 16 merkin salaisuus, `BIND_HOST`, joka ei ole loopback-osoite, muu `NODE_ENV` kuin `production` tai muu `QOS_TARGET_URL` kuin `http://127.0.0.1:<portti>/QoS` kirjaa varoituksen. | Paketti: vain julkisessa tilassa (sama arvo kuin `gateway.env`-tiedostossa, säilyy uudelleenajoissa, poistetaan yksityisessä tilassa) |
| `GAMESERVER_ALLOW_FROM` | ei oletusta | pilkuilla erotetut IP-osoitteet; muut merkinnät ohitetaan | Lisäosoitteet, joista pelipalvelinavaimen saa esittää, jos pelipalvelin on toisella koneella. Oletuksena avain hyväksytään vain tältä koneelta (loopback tai jokin koneen omista osoitteista) eikä koskaan välityspalvelimen kautta. Luettele vain koneita, joita itse hallitset; niiltäkin vaaditaan avain. | Oletuksena ei kukaan; paketti: säilyttää |

### Lokit {#metagame-logging}

`BACKEND_HEALTH=1` ottaa käyttöön pyyntöjen koontimittarit ja tapahtumasilmukan viiveen mittauksen
20 ms tarkkuudella. Oletus pois; käynnistä metagame uudelleen muutoksen jälkeen.
Ylläpitäjän `/undaunted/api/BackendHealth` vastaa 404, kun mittaus on pois.
Muistiin jää enintään 60 sekuntikohtaista laskuria, ei URL-osoitteita, pyyntöjen sisältöä tai tilitunnuksia.
Näkymän ohje: `tools/DASHBOARD.md`.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `LOG_LEVEL` | `info` (myös tyhjänä) | `fatal`, `error`, `warn`, `info`, `debug`, `trace`, `silent` | Lokitaso. | Oletuksena ei kukaan |
| `LOG_REQUESTS` | päällä | `0` tai mikä tahansa muu | **Vain forkissa.** Yksi rivi pyyntöä kohden: `METHOD /path gs=0` (`gs=1`, kun pyynnössä oli pelipalvelinavain). Yhdyskäytävän takana rivin loppuun tulee ` via=gateway ip=<pelaajan osoite>`; muut välitetyt pyynnöt saavat lopun ` via=proxy peer=<osoite>`. Lokiin kirjataan vain polku, ei koskaan kyselymerkkijonoa (query string) tai otsakkeita, ja polun tunnisteen näköiset osat korvataan. `0` ottaa sen pois päältä. | Oletuksena ei kukaan |
| `LOG_BODIES` | pois | `1` tai mikä tahansa muu | **Vain forkissa.** `1` kirjoittaa keskeneräisten reittien (eteneminen, Hunt Pass, palkkiotehtävät, odotusajat, Escalation, oikeudet, varustesarjojen paikkojen avaukset, kauppa (`/product`, `/token`, `/notification`), saldot ja `/reconcile`, Slayer Links, matchmaking, ryhmä, kaverit, tavarat ja tilihaut) pyyntöjen sisällöt tiedoston `BODY_LOG_FILE` loppuun: yksi JSON-olio riviä kohden, `{t, method, url, gs, body, status, ms}`, eli saapumisaika, metodi, URL (kyselymerkkijono mukaan lukien), tieto pelipalvelinavaimen mukanaolosta, sisältö katkaistuna 8 kt:hen (tavaroilla 64 kt), vastauksen tila ja vastaamiseen kulunut aika millisekunteina (`"aborted": true`, jos yhteys katkesi ensin). Rivi kirjoitetaan, kun vastaus on valmis. Tunnisteet, tiliavaimet (`UUK_...`) ja muut pitkät tunnisteen näköiset merkkijonot poistetaan sekä URL:sta että sisällöstä. Kehitysapu: tiedosto tallentaa, mitä pelaajat lähettävät, joten pidä se yksityisenä äläkä koskaan ota asetusta käyttöön julkisella palvelimella. | Oletuksena ei kukaan; paketti: pakotetaan arvoon `0` julkisessa tilassa, säilytetään yksityisessä tilassa |
| `BODY_LOG_PER_PATH` | `0`: ei rajaa | kokonaisluku | **Vain forkissa.** Asetuksella `LOG_BODIES=1`: enintään näin monta riviä metodia ja tarkkaa polkua kohden (tunnukset mukaan lukien) yhden metagamen ajon aikana; polun viimeisen rivin jälkeen tulee `body log: <METHOD polku> reached BODY_LOG_PER_PATH=N; no more lines for it in this run`. Silmukkaan jääneen peliohjelman varalle. | Oletuksena ei kukaan |
| `BODY_LOG_FILE` | `bodies.log` työhakemistossa | tiedostopolku | Mihin `LOG_BODIES` kirjoittaa. Tiedostoa ei koskaan kierrätetä (rotate). `legacy/UndauntedMetagame/bodies.log` ei ole gitin ohittama: osoita tämä repositorion ulkopuolelle (esimerkiksi `C:/dr/data/bodies.log`) äläkä koskaan tallenna tiedostoa versionhallintaan. | Paketti: aina (`<root>/data/logs/bodies.log`) |
| `CAPTURE_DIR` | pois | kansion polku | **Vain forkissa, Rust-siirtymää varten.** Tallentaa liikenteen varjotoistoa varten (MIGRATION_PLAN.md M2): metagamessa jokaisen pyynnön, joka ei tullut yhdyskäytävän kautta (pelipalvelimet, yksityisen tilan pelaajat), ja koko chatin; yhdyskäytävässä jokaisen pyynnön. Yksi redaktoitu JSON-rivi kustakin tiedostoon `<dir>/metagame-<vvvv-kk-pp>.jsonl` tai `gateway-<vvvv-kk-pp>.jsonl`; tunnukset, tiliavaimet ja salaisuudet korvataan paikkamerkeillä. **Henkilötietoja:** katso [Yksityisyys]({{ legal_page.url | relative_url }}#traffic-recording). Virheellinen arvo estää käynnistyksen. | Ei kukaan oletuksena |
| `CAPTURE_RETENTION_DAYS` | `14` | 1-90 | `CAPTURE_DIR`-asetuksella: tätä vanhemmat tallennetiedostot poistetaan. | Ei kukaan oletuksena |
| `CAPTURE_MAX_BODY_BYTES` | `262144` | 0-16777216 | `CAPTURE_DIR`-asetuksella: suuremmasta rungosta tallennetaan vain koko. | Ei kukaan oletuksena |

### Palvelimen tiedot {#metagame-identity}

Nämä arvot ovat **julkisia**: kuka tahansa voi lukea ne osoitteesta `GET /dauntless-status`, ja
rekisteröityneet pelaajat osoitteesta `GET /undaunted/api/ServerStatus` (katso
[HTTP-rajapinta]({{ api_page.url | relative_url }})).

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `SERVER_NAME` | `Dauntless Revived` | tulostettavia ASCII-merkkejä, katkaistaan 64 merkkiin | Palvelimen nimi tilavastauksissa sekä `/dauntless-status`-vastauksen pelissä näkyvässä tervetulotekstissä (`Welcome to <nimi>!`, käännettynä jokaiselle vastauksen kahdeksasta kielestä). | Paketti: aina, parametrista `-ServerName` (oletus `Dauntless Revived`). Asennusohjelma ei muista sitä: anna `-ServerName` joka ajokerralla, tai nimi palaa oletukseksi. |
| `SERVER_VERSION` | käännöksen aikana kirjattu versio (`dist/build-info.json`), sitten `package.json`, sitten `unknown` | tulostettavia ASCII-merkkejä, enintään 32 merkkiä | Versio tilavastauksissa. | Oletuksena ei kukaan |
| `GIT_COMMIT` | käännöksen aikana kirjattu commit (perässä `-dirty`, jos metagamen kansiossa oli committoimattomia muutoksia), sitten `unknown` | enintään 64 merkkiä: kirjaimia, numeroita ja merkkejä `._+-`; muunlainen arvo ohitetaan | Commit, josta käynnissä oleva koodi on peräisin. | Paketti: aina (asennettu commit); `Update-DauntlessServer.ps1` kirjoittaa sen uudelleen jokaisessa päivityksessä ja palautuksessa |
| `SOURCE_URL` | `https://github.com/mixutin/dauntless-revived` | URL, tulostettavia ASCII-merkkejä, enintään 256 merkkiä | Missä käynnissä olevan palvelimen lähdekoodi on, AGPL-lisenssin lähdekooditarjousta varten ([Kiitokset ja lisenssi]({{ legal_page.url | relative_url }})). Jos ajat muokattua koodia muille, osoita se muokattuun lähdekoodiisi. | Paketti: aina repositorion osoite tiedostosta `deploy/windows-server/DauntlessServer.Common.ps1`, joten käsin tehty muutos kirjoitetaan yli seuraavalla asennuskerralla; fork muuttaa osoitteen siellä |
| `CONTENT_PORT` | puuttuu tai virheellinen: porttia ei ilmoiteta | 1-65535 | Kertoo käynnistimille, mitä porttia sisältöpalvelin (pelin lataukset, uutiset, kuvat) käyttää. | Paketti: aina, kun sisältöpalvelin on asennettu (61002), muuten poistetaan |
| `STATUS_EXTRA` | päällä | `0` tai mikä tahansa muu | **Vain forkissa.** `GET /dauntless-status` lisää kentät `name`, `version`, `commit` ja `sourceUrl` niiden yhdeksän kentän perään, jotka peli lukee. `0` vastaa pelkillä yhdeksällä kentällä, kuten alkuperäinen projekti. Reitti vastaa kenelle tahansa eikä koskaan nimeä pelaajia. | Oletuksena ei kukaan |

### Eteneminen {#metagame-progression}

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `PROGRESSION_MODE` | oikea eteneminen jokaiselle tilille (puuttuu, tyhjä tai `real`) | `real` tai `stub`, kirjainkoolla ei väliä; muu arvo (kuten `off`, `0` tai `false`, jotka tarkoittivat ennen tynkää) kirjaa varoituksen ja lasketaan arvoksi `real`, ja käynnistysrivillä lukee silloin `real for every account (PROGRESSION_MODE=<arvo> is not recognised)` | **Vain forkissa.** Oikea eteneminen: jokainen tili tallentaa oman Slayer-tasonsa, aseiden ja hirviöiden (behemoth) mestaruuden, tavoitteet, Hunt Passin, oikeudet, varustesarjojen paikat, odotusajat ja palkkiotehtävät. `stub`: alkuperäisen projektin valemaksimitasot kaikille, mitään ei tallenneta. **Oletus muuttui:** ennen puuttuva arvo tarkoitti tynkää. Mitään ei siirretä; lue [päivitysohjeet]({{ upgrade_page.url | relative_url }}#real-progression-default). | Oletuksena ei kukaan; paketti: ei koskaan kirjoita, säilyttää |
| `PROGRESSION_REAL_ACCOUNTS` | ei oletusta | pilkuilla erotetut tilitunnukset (`UID-...`) | Vain asetuksen `PROGRESSION_MODE=stub` kanssa: nämä tilit saavat silti oikean etenemisen. Oikean etenemisen tilassa muuttuja ohitetaan (käynnistysrivi kertoo sen, kun muuttuja on asetettu). | Oletuksena ei kukaan; paketti: säilyttää |
| `PROGRESSION_GRANT_CAP` | `5000` (myös kun arvo ei ole positiivinen kokonaisluku) | positiivinen kokonaisluku | Suurin kokemuspistemäärä (XP), jonka yksi pyyntö voi lisätä yhdelle radalle. Ylimenevä osa leikataan pois, ja leikkaus merkitään etenemisen tarkastustauluun. | Oletuksena ei kukaan |
| `PROGRESSION_CONFIRM` | päällä | `off` tai mikä tahansa muu | `off` saa tasonvahvistusreitin vastaamaan taas 404 oikean etenemisen tileille, kuten alkuperäisessä projektissa. Vianetsintäkytkin; jätä se asettamatta. | Oletuksena ei kukaan |
| `PROGRESSION_ALLOW_DELETE` | pois | `1` tai mikä tahansa muu | `1` antaa pelipalvelinten nollata etenemisradan (peli lähettää sen vain vianetsintäkomennosta). Ilman sitä nollaus onnistuu vain ylläpitäjän avaimella. Kun asetus on päällä, mikä tahansa pelipalvelinkutsu voi pyyhkiä pelaajan radan. | Oletuksena ei kukaan |
| `ENTITLEMENTS_DEFAULT` | `season09b_premium,season_premium_any,season_free_any`, kun muuttuja puuttuu | pilkuilla erotetut oikeuksien nimet; tyhjä tarkoittaa, ettei yhtään | Oikeudet (entitlements), jotka jokaisella oikean etenemisen tilillä on. `season09b_premium` on Elite Hunt Pass. Jokainen oletus lisätään tilille kerran, ensimmäisellä kerralla kun tilin oikeudet luetaan; nimen poistaminen myöhemmin ei ota oikeutta takaisin, ja ylläpitäjän perumana oletusoikeus pysyy perutuksi. | Oletuksena ei kukaan |
| `PROGRESSION_REPLAY_WINDOW_S` | `5` | sekunteja kokonaislukuna; `0` ottaa pois käytöstä | **Vain forkissa.** Reitin `POST /progression/<tili>` uusintavahti: myöntö, jonka runko on tavu tavulta sama kuin tilin edellinen myöntö, alle näin monta sekuntia sen jälkeen eikä välissä ole muuta radan kirjoitusta, saa ensimmäisen myönnön tallennetun vastauksen eikä lisää mitään (loki kertoo `repeats the grant of N s ago`; tapahtumariviin tulee merkintä). Pelipalvelin yrittää epäonnistunutta pyyntöä uudelleen jopa 5 kertaa. Pidä arvo alle 10:n: pelipalvelin lähettää jonoon kertyneet myöntönsä enintään kerran 10 sekunnissa, ja kaksi niistä voi olla samanlaisia. Jos rivi näkyy tavallisessa pelaamisessa eikä vain verkkovirheen jälkeen, aseta `0`. | Oletuksena ei kukaan |
| `PROGRESSION_CONFIRM_ENTITLEMENTS` | pois | päällä/pois | **Vain forkissa.** Päällä: Hunt Passin tai mestaruuden vahvistus, joka nostaa tasoa, antaa myös tason **pysyvät** oikeudet etenemisen asetuksista (season09b:ssä premium-tasot 6, 9, 29 ja 50 sekä ilmainen taso 50), lähteenä `confirm:<rata>:<taso>`. Ei koskaan tavaroita, valuuttoja eikä määräaikaisia oikeuksia (ne vain merkitään). Pois päältä, koska ei vielä tiedetä, antaako pelipalvelin ne itse (pelitesti: saavuta Elite-taso 6 metsästysten XP:llä ja katso, tuleeko `POST /entitlementv2`). Vahvistuksen vastaus on sama kummin päin tahansa. | Oletuksena ei kukaan |
| `PROGRESSION_CONFIG_DIR` | puuttuu: mukana tuleva `vendor/progression_config.json` | kansion polku | **Vain forkissa.** Kansio `.json`-tiedostoja, joista kukin on etenemisrata (Hunt Pass -kausi tai mestaruusrata), ratojen luettelo tai kokonainen asetustiedosto. Rata korvaa mukana tulevan radan, jolla on sama `progression_id`, ja uusi tunnus lisätään. Tarkistetaan käynnistyksessä; puuttuva kansio, kansio ilman `.json`-tiedostoja, virheellinen JSON, rata ilman kenttää `progression_id` tai `requirements`, väärässä järjestyksessä olevat tasot tai sama tunnus kahdesti pysäyttävät metagamen. Luetaan kerran; muutos vaatii uudelleenkäynnistyksen. Muodot ovat sivulla [Pelin asetukset]({{ gamesettings_page.url | relative_url }}#hunt-pass-seasons). Älä koskaan muokkaa kautta paikallaan, kun pelaajilla on siinä etenemistä. | Oletuksena ei kukaan |
| `ACTIVE_HUNT_PASS` | `season09b` | ladatun radan tunnus | **Vain forkissa.** Hunt Pass, jonka tili saa, kun mitään ei ole tallennettu (`GET /huntpass/<tili>` ja tasolaskennan oletus). Tunnus, joka ei ole ladattu rata, pysäyttää metagamen käynnistyksessä. | Oletuksena ei kukaan |
| `BALANCE_FROM_INVENTORY` | päällä | päällä/pois | **Vain forkissa.** Päällä: `GET /balance` ja `POST /reconcile` kertovat jokaiselle `CURRENCY_*`-avaimelle (ja sen `id_currency_*`-parille) sen pinon määrän, joka tilin aktiivisella hahmolla on (viimeksi tallennettu hahmo), ja pitävät vanhat arvot valuutoille, joita hahmolla ei ole. Pois: vanha kiinteä taulukko (Notes `users`-taulusta, asepoletit 25, muut 0). | Oletuksena ei kukaan |

### Escalation {#metagame-escalation}

Kaikki ovat **vain forkissa**, ja ne luetaan jokaisella pyynnöllä. Miten Escalation tallennetaan ja
mitä pelaajat näkevät, kerrotaan sivulla [Escalation]({{ '/fi/findings/escalation.html' | relative_url }}).

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `ESCALATION_MODE` | `stub` | `stub` tai `real`, kirjainkoolla ei väliä; muu arvo on varoitus ja `stub` | `stub`: jokainen tili lukee alkuperäisen projektin tekaistun maksimin (`escalation_level` 99999, jonka peli näyttää viimeisenä tasona), ja tallennusreitti vastaa 404, kuten ennen. `real`: oikean etenemisen tilit lukevat ja tallentavat omat kautensa tallennussääntöjen mukaan; tynkätilan tilit saavat edelleen tyngän. **Arvoon `real` vaihtaminen pudottaa jokaisen pelaajan tekaistusta maksimista tasolle 0.** Käynnistä pelipalvelimet uudelleen metagamen kanssa, kun vaihdat. | Oletuksena ei kukaan |
| `ESCALATION_STRICT` | pois | päällä/pois | Vain arvolla `real`. Pois: tallennus, joka rikkoo pehmeän säännön (XP alle seuraavan tason hinnan, käytetyt pisteet tason rajoissa, kykyportaiden rajat, palkinnot kerätty tasollaan), tallennetaan varoitusrivin ja tapahtumarivin merkinnän kanssa. Päällä: tällainen tallennus torjutaan vastauksella 409. Kovat säännöt ovat aina voimassa. | Oletuksena ei kukaan |

### Kauppa {#metagame-store}

Kaikki ovat **vain forkissa**, ja ne luetaan jokaisella pyynnöllä. Miten kauppa toimii, kerrotaan
sivulla [Pelin kauppa]({{ '/fi/findings/store.html' | relative_url }}).

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `STORE` | `off` | `off` tai `free`, kirjainkoolla ei väliä; muu arvo on varoitus ja `off` | `off`: kauppanäkymä saa vanhan 400:n ja ostoreitit 404, kuten ennen. `free`: ilmaisten tarjousten valikoima tiedostosta `vendor/store_catalog.json`, ostotunniste ja vahvistus; tavarat menevät tilin aktiiviselle hahmolle. Ennen kuin otat sen käyttöön oikeille pelaajille, laske hahmot tiliä kohden (osto menee viimeksi tallennetulle hahmolle). | Oletuksena ei kukaan |
| `STORE_REPEATABLE_TOKENS` | pois | päällä/pois | Vain arvolla `free`. Päällä: 20 premium-palkkiotehtävätunnisteen paketti (`bundle_currency_bounty_small`) näkyy listalla ja sen voi ostaa kuinka monta kertaa tahansa, eli rajattomasti ilmaisia premium-palkkiotehtäviä. Pois: sitä ei näytetä, se vastaa 404, ja tunniste, joka annettiin asetuksen ollessa päällä, torjutaan (409). | Oletuksena ei kukaan |

### Tallennukset ja tavarat {#metagame-saves}

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `SAVE_HISTORY_KEEP` | `100` (myös kun arvo on alle 2 tai ei ole kokonaisluku) | kokonaisluku, 2 tai enemmän | Montako uusinta versiota kunkin hahmon tiedoista, ja erikseen sen varustesarjoista, säilytetään aina palautuksia varten. 100 on suunnilleen viimeiset 50 minuuttia pelaamista. | Oletuksena ei kukaan |
| `SAVE_HISTORY_HOURLY` | `48` (myös kun arvo on tyhjä, negatiivinen tai ei ole kokonaisluku) | tuntien määrä kokonaislukuna, 0 tai enemmän (0 ottaa tämän portaan pois käytöstä) | Niiden lisäksi kunkin tunnin viimeinen versio säilytetään näin monta tuntia. | Oletuksena ei kukaan |
| `SAVE_HISTORY_DAILY` | `30` (myös kun arvo on tyhjä, negatiivinen tai ei ole kokonaisluku) | päivien määrä kokonaislukuna, 0 tai enemmän (0 ottaa tämän portaan pois käytöstä) | Ja kunkin päivän viimeinen versio näin monta päivää. Oletusarvoilla tämä tekee enintään noin 3,5 Mt hahmoa kohden. | Oletuksena ei kukaan |
| `INVENTORY_REFUSE_OVERSPEND` | pois | `1` tai mikä tahansa muu | `1` torjuu (409) tavarapyynnön, joka poistaa enemmän kuin pelaajalla on. Pois päältä, koska torjunta hylkää koko pyynnön palkintoineen. Kun asetus on pois, ylitys pysäytetään nollaan ja kirjataan lokiin. | Oletuksena ei kukaan; paketti: säilyttää |
| `INVENTORY_REPORT_REMOVALS` | päällä | `0` tai mikä tahansa muu | **Vain forkissa.** Tavaravastaukset luettelevat jokaisen tavarapinon, jota pyyntö muutti, lopullisine määrineen (0, jos pino käytettiin loppuun), joten pelipalvelin näkee, mitä kului. `0` palauttaa alkuperäisen projektin vastauksen, jossa oli vain lisäykset ja jonka takia varusteiden parantaminen ei maksanut mitään. | Oletuksena ei kukaan |
| `PATROL_BONUS_DAILY` | `10` (myös kun arvo on tyhjä tai ei ole kokonaisluku, joka on vähintään 1) | kokonaisluku, 1 tai enemmän | **Vain forkissa.** Päivittäinen partiobonuksen täydennys (pelin oma on 6) nostaa `TOKEN_DAILY_PATROL_BONUS`-pinon näin suureksi. | Oletuksena ei kukaan |
| `BOUNTY_TOKENS_DAILY` | `6` (myös kun arvo on tyhjä tai ei ole kokonaisluku, joka on vähintään 1) | kokonaisluku, 1 tai enemmän | **Vain forkissa.** Palkkiopoletit, jotka pelaaja saa aluksi ja joka päivä; jako nostaa `TOKEN_BOUNTY_DRAFT`-pinon näin suureksi. Olemassa olevat hahmot asetetaan molempiin määriin kerran, tämän version ensimmäisellä käynnistyksellä. | Oletuksena ei kukaan |

### Kaverit, ryhmät ja killat {#metagame-social}

Kaikki ovat **vain forkissa**. Kytkimet luetaan jokaisella pyynnöllä, joten uudelleenkäynnistys
uudella arvolla tulee voimaan heti. Sivu [Kaverit, ryhmät ja killat]({{ '/fi/findings/social.html' | relative_url }})
kertoo, mitä kukin vastaus tekee pelissä.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `ACCOUNT_MAPPING` | päällä | `0` tai mikä tahansa muu | `POST /account/mapping` yhdistää tämän palvelimen tilit muodossa, jota peliohjelma lukee; sitä tarvitsevat kaverin lisääminen, nimellä kutsuminen ja kaverilista. `0` ei yhdistä mitään, kuten ennen: kaverin lisääminen ei silloin tee pelissä mitään. | Oletuksena ei kukaan |
| `ACCOUNTINFO_PUBLIC_LEGACY` | pois | `1` tai mikä tahansa muu | `1` palauttaa alkuperäisen projektin `POST /accountinfo/public` -vastauksen, joka kuvaa kysyjää eikä kysyttyä pelaajaa. Silloin ryhmäkutsut, Hunt Members ja kaverit eivät näy pelissä. Paluutie siltä varalta, että uusi vastaus häiritsee kirjautumista. | Oletuksena ei kukaan |
| `GUILDS` | päällä | `0` tai mikä tahansa muu | Kiltareitit. `0` palauttaa vanhat tyngät (ei kiltaa, ei kutsuja), ja kaikki muut kiltareitit vastaavat 404; tallennetut killat säilyvät ja palaavat kytkimen mukana. | Oletuksena ei kukaan |
| `GUILD_MAX_MEMBERS` | `100` (myös, jos arvo ei ole kokonaisluku 1–10000) | kokonaisluku | Jäseniä kiltaa kohden. Peliohjelma näyttää tämän luvun ja jäljellä olevat vapaat paikat. | Oletuksena ei kukaan |
| `GUILD_INVITE_TTL_DAYS` | `7` (myös, jos arvo ei ole luku väliltä yli 0 ja enintään 365) | päiviä | Kuinka kauan kiltakutsu on voimassa. | Oletuksena ei kukaan |
| `GUILD_NAME_DENYLIST` | tyhjä | pilkuilla erotettuja sanoja | Sanat, jotka torjutaan kiltojen nimissä ja nimikylteissä lyhyen sisäänrakennetun listan lisäksi. Sana löydetään mistä kohtaa nimeä tahansa kirjainkoosta riippumatta, kun numerokorvaukset (kuten `0` kirjaimen `o` tilalla) on purettu. | Oletuksena ei kukaan |
| `GUILD_RESERVED_NAMES` | päällä | `0` tai mikä tahansa muu | Henkilökunnan ja projektin sanat (kuten `admin`, `moderator`, `official` sekä nimikyltit `GM`, `DEV` ja `MOD`) vastaavat "already in use", joten mikään kilta ei voi esiintyä palvelimen henkilökuntana. `0` sallii ne, esimerkiksi virallisen killan perustamista varten; lyhyet loukkaavat tunnukset torjutaan silti. Listat ovat sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#guilds). | Oletuksena ei kukaan |
| `GUILD_CREATE_ACTIVITY_FALLBACK` | pois | `1` tai mikä tahansa muu | Pelipalvelimen lähettämä killan perustaminen hyväksytään vain, jos johtaja on tarkistanut juuri tämän nimen ja nimikyltin omalla tunnisteellaan viimeisten 15 minuutin aikana. `1` hyväksyy myös johtajan, joka tarkisti jonkin toisen nimen tai näkyi palvelimelle viimeisen minuutin aikana, ja kirjaa lokiin varoituksen. Vain siltä varalta, että oikea testi näyttää, ettei peliohjelma koskaan tarkista lopullista nimeä (torjunta kirjataan tekstillä "no validate of this name"); päällä ollessaan muokattu peliohjelma voisi nimetä toisen paikalla olevan pelaajan killan johtajaksi. | Oletuksena ei kukaan |
| `PARTY_SOLO_STUB` | päällä | `0` tai mikä tahansa muu | Yksin ryhmässään oleva pelaaja saa alkuperäisen projektin paikkamerkkiehdokkaan (`QUEUED_FOR_START`), jonka on todettu olevan harmiton metsästyksiin jonottamiselle. Peliohjelma ei lähetä ryhmäkutsua, kun se luulee ryhmänsä olevan jonossa; jos Invite to Party ei tee mitään yksin olevalle pelaajalle (lokiin ei tule riviä `party: invite`), `0` vastaa yhden hengen ryhmälle ilman ehdokasta. | Oletuksena ei kukaan |
| `SLAYER_LINKS` | päällä | päällä/pois | Slayer Links: kahdeksan `/slayerlink`-reittiä ([mitä ne tekevät]({{ '/fi/findings/social.html' | relative_url }}#slayer-links)). Pois: jokainen vastaa 404, kuten ennen; tallennetut kutsut ja linkit säilyvät ja palaavat kytkimen mukana. | Oletuksena ei kukaan |
| `VERIFY_STUB_ACCOUNT` | pois | päällä/pois | `GET /account/api/oauth/verify` (peliohjelman säännöllinen istunnon tarkistus) nimeää sen tunnisteen tilin, jonka kanssa pyyntö tuli, ja vastaa puuttuvalle tai huonolle tunnisteelle vanhalla paikkamerkillä ja tilalla 200. Päällä: vanha paikkamerkki-`account_id` kaikille; paluutie siltä varalta, että uusi vastaus aiheuttaa uloskirjautumisia tai yhteyssilmukoita. | Oletuksena ei kukaan |

### Chat {#metagame-chat}

Kaikki ovat **vain forkissa**. Pelin tekstichat (tiekartta 3.10): metagamen sisällä toimiva kuuntelija
pelin XMPP-yhteydelle paikallisessa osoitteessa. Julkisessa tilassa yhdyskäytävä välittää pelin
chat-yhteyden sille (`GATEWAY_WS_URL`, sama portti); yhdellä koneella peli ottaa siihen yhteyden
suoraan. Miten se toimii, kerrotaan sivulla [Tekstichat]({{ '/fi/findings/chat.html' | relative_url }}),
ja viestit, joihin se vastaa, sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#chat). Luetaan
käynnistyksessä. Huono arvo tai varattu portti kirjoittaa yhden virherivin `chat: not started ...`, ja
metagame jatkaa ilman chattia.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `CHAT` | päällä | asettamaton/tyhjä tai `1` on päällä; `0` pois; muu arvo pois ja varoitus | Käynnistää loopback-pelichatin. Käynnistys-, portti- ja socket-virheet kirjataan ilman tunnisteita tai viestejä; HTTP jatkaa toimintaansa. Korvaa ohitetun `EXPERIMENTAL_CHAT`-asetuksen. Vanha `CHAT=0` säilyy päivityksessä: aja `Set-Chat.ps1 -On`, kun palvelin on tyhjä. | Julkinen paketti: `-Chat`, tallennettu `Chat`, muuten On. Yksityinen paketti asettaa `0`. |
| `CHAT_PORT` | `61099` | 1-65535 | Kuuntelijan portti. Sen on oltava sama kuin yhdyskäytävän `GATEWAY_WS_URL`-osoitteen portti. | Paketti: aina julkisessa tilassa (61099; hiekkalaatikossa 62099) |
| `CHAT_BIND_HOST` | `127.0.0.1` | `127.0.0.1` tai `::1`; kun `GATEWAY_SECRET` on asetettu (julkinen tila), vain `127.0.0.1` | Kuuntelijan osoite. Mikä tahansa muu, myös `0.0.0.0`, pitää chatin pois päältä. Yksityistä tilaa (Tailscale) ei vielä tueta. | Paketti: aina `127.0.0.1` julkisessa tilassa |
| `CHAT_NICK_CHECK` | `enforce` | `enforce` tai `log`; muu arvo lasketaan arvoksi `enforce` ja kirjoittaa varoituksen | `enforce` hylkää huoneeseen liittymisen, jos nimimerkki ei ole `<tilin käyttäjänimi>:<sen tilitunnus>:<sen resurssi>` ([miksi]({{ '/fi/findings/chat.html' | relative_url }}#nickname-check)). `log` päästää sisään nimimerkin, joka rikkoo resurssi-, muoto- tai nimisääntöä, ja kirjoittaa yhden varoitusrivin yhteyttä ja huonetta kohden: paluukytkin siltä varalta, että oikea peliohjelma hylätään. Nimimerkki, jossa on toisen tilin tunnus, hylätään molemmissa tiloissa (oikea peliohjelma ei koskaan rakenna sellaista). | Oletuksena ei kukaan |
| `CHAT_TRACE` | pois | `1` tai mikä tahansa muu | `1` kirjaa jokaisen chat-kehyksen molempiin suuntiin 2 kt:n mittaan leikattuna niin, että kirjautuminen, salasanat, viestien teksti (`[N chars]`) ja tunnisteet on korvattu. Vain ensimmäisiin oikeisiin ajoihin. | Oletuksena ei kukaan |
| `CHAT_PRESENCE` | päällä | päällä/pois (`1`, `true`, `on`, `yes` / `0`, `false`, `off`, `no`) | Kavereiden paikalla olo ([miten]({{ '/fi/findings/chat.html' | relative_url }}#presence)): jokaisen pelaajan oma läsnäolotieto välitetään hänen paikalla oleville, hyväksytyille, estämättömille kavereilleen, ei koskaan takaisin pelaajan omalle tilille, ja HTTP:n kautta hyväksytty kaveripyyntö lähetetään chatin kautta. Tarvitsee asetuksen `CHAT=1`; ilman sitä metagame kirjaa rivin `chat: CHAT_PRESENCE is on but chat is off (CHAT=1 is needed); nobody shows as online`, eikä muuta tapahdu. Luetaan chat-palvelimen käynnistyessä. Pois: huoneiden ulkopuolella ei yhtäkään läsnäoloviestiä. Oletuksena päällä; `CHAT_PRESENCE=0` poistaa käytöstä. Kahden pelaajan pelitesti tarvitaan edelleen. | Automaattisesti; pelaajan ei tarvitse muuttaa asetuksia |

**Rajat** (kiinteät koodissa):

| Raja | Arvo | Kun se ylittyy |
|:-----|:-----|:---------------|
| WebSocket-kehys | 32 KiB | Se yhteys suljetaan (1009) |
| Yhteyksiä yhteensä | 64 | Uudet saavat HTTP 503:n |
| Kehyksiä ennen kirjautumista | 4 (peli tarvitsee `<open>`-, `<auth>`- ja hylkäyksen jälkeen vanhan kirjautumistapansa viestin); yksi kirjautumisyritys | Yhteys suljetaan |
| Keskeneräisiä kirjautumisia osoitetta kohden (yhteydet, jotka eivät ole vielä sitoutuneet) | 8 | HTTP 503 |
| Epäonnistuneita kirjautumisia osoitetta kohden | 10 kymmenessä minuutissa | Osoitteen kirjautumiset saavat 10 minuutin ajan vastauksen `temporary-auth-failure` |
| Yhteyksiä tiliä kohden | 2 sidottua, kaikkiaan 3 | Kolmas sidottu korvaa pisimpään hiljaa olleen; kolmannen jälkeinen kirjautuminen sulkee vanhimman, joka ei ole sitoutunut |
| Aikaa sitoutua | 10 sekuntia kirjautumisesta | Yhteys suljetaan; kolme kertaa kymmenessä minuutissa pidättää tilin kirjautumisia 60 sekuntia |
| Huoneita yhteyttä kohden | 8 | Liittyminen hylätään |
| Pelaajia huonetta kohden | 128 | Liittyminen hylätään |
| Huoneita yhteensä | 1000 | Liittyminen hylätään |
| Liittymisiä yhteyttä kohden (hylätyt mukaan lukien) | 10 kerralla, sitten 1 kuuden sekunnin välein | Liittyminen hylätään |
| Huonerivejä ja kuiskauksia yhteyttä kohden | 8 kerralla, sitten 1 sekunnissa | Rivi saa virheen; kuiskaus pudotetaan |
| Kaikkia viestejä yhteyttä kohden | 60 kerralla, sitten 30 sekunnissa | Pudotetaan; yli 100 pudotusta minuutissa päättää yhteyden |
| Viestin teksti | 1-2048 merkkiä | Rivi saa virheen; kuiskaus pudotetaan |
| Huoneen nimimerkki | 1-1023 tavua | Liittyminen hylätään |
| Lähtemätöntä liikennettä yhteyttä kohden | 256 KiB | Mitään ei enää lähetetä; yhteys päättyy (`reason=backlog`) |

Yhteyttä pingataan 50 sekunnin hiljaisuuden jälkeen, ja se päätetään, jos vastausta ei tule
seuraavien 100 sekunnin aikana: peli vastaa vain oman pelisäikeensä kierroksella, ja pitkä kartan
lataus pysäyttää sen ([miksi]({{ '/fi/findings/chat.html' | relative_url }}#limits)). Korvaaminen,
väärinkäyttö, liian suuri kehys tai rajan ylittävä lähtemätön liikenne pidättää tilin seuraavaa
kirjautumista 60 sekuntia; vastaamaton ping ei pidätä. Hylätty liittyminen lasketaan pudotusten
rajaan, ja se kirjataan lokiin kerran yhteyttä, huonetyyppiä ja syytä kohden 10 minuutin välein.
Viestien tekstiä, tunnisteita, kirjautumistietoja tai pyyntöjen otsakkeita ei kirjata lokiin koskaan.

### Yhteensopivuuskytkimet {#metagame-compatibility}

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `MISC_ROUTES` | päällä | `0` tai mikä tahansa muu | **Vain forkissa.** Vastaa reitteihin, jotka antoivat alkuperäisessä projektissa 404:n: kaverilista ja estolista, viimeaikaiset pelaajat, kaveriasetukset, `POST /candidate/player/alive` ja `GET /motd/trigger`. `0` palauttaa 404-vastaukset. | Oletuksena ei kukaan |
| `ACCOUNT_DISPLAY_NAME` | päällä | `0` tai mikä tahansa muu | **Vain forkissa.** Tili- ja ryhmävastauksissa on oikea käyttäjänimi kentässä `displayName`. `0` palauttaa alkuperäisen projektin tyhjän `{}`-arvon. | Oletuksena ei kukaan |
| `MATCHMAKING_CANCEL` | pois | `1` tai mikä tahansa muu | **Vain forkissa, kokeellinen.** `1` vastaa peliohjelman matchmaking-perumiseen (`DELETE /candidate` ja `DELETE /candidate/leave`). Pois päältä, koska peliohjelma lähettää perumisen heti jokaisen jonoon liittymisen jälkeen, ja metsästykset alkavat vain siksi, että se saa vastaukseksi 404. | Oletuksena ei kukaan |

---

## Deploy-palvelin (`legacy/UndauntedDeployServer/`) {#deploy-server}

### Muistin tarkistus ennen käynnistystä

Uusi peliprosessi vaatii vapaata fyysistä RAM-muistia vähintään `GAMESERVER_MIN_FREE_MB`
(oletus 3072 MiB) sekä `GAMESERVER_STARTUP_MB` (1536 MiB käynnistysvaraus).
Varaus kestää `GAMESERVER_RESERVATION_SECONDS` sekuntia (60), jotta rinnakkaiset käynnistykset
eivät varaa samaa muistia. Virhe tai prosessin poistuminen vapauttaa varauksen aikaisemmin.
Arvojen on oltava positiivisia kokonaislukuja. `GAMESERVER_MEMORY_GUARD=0` poistaa tarkistuksen.
Oletuksena tarvitaan 4608 MiB vapaata ilman muita varauksia. Rajat ovat varovaisia arvioita;
ne eivät estä myöhempää muistinkulutuksen kasvua. Käynnistyksen aikana varaus voi laskea jo
käytetyn muistin toistamiseen. Käynnissä olevia maailmoja ei lopeteta ja niihin voi liittyä.

Muistin tai porttien loppuminen tuottaa metagamelle erillisen 503-vastauksen. Odottavien pelaajien
tilakysely yrittää uudelleen enintään kymmenen sekunnin välein, viiden minuutin ajan; sitten FAILED.
Taustakäynnistyksiä tai taattua FIFO-järjestystä ei ole. Neljän pelaajan täysi jono torjuu lisäliittyjät.
`MATCHMAKING_CANCEL` ohjaa edelleen peruutusta; sitä ei oteta automaattisesti käyttöön.
Päivitä molemmat palvelinpaketit yhdessä. Tietokantamuutoksia ei tarvita.

Käynnistää ja valvoo pelipalvelinprosesseja, kun metagame pyytää. Valmis `.env` on sivulla
[Pystytä palvelin]({{ host_page.url | relative_url }}#deploy-server).

**Milloin luetaan:** kerran, käynnistyessä. Käynnistä deploy-palvelin uudelleen jokaisen muutoksen
jälkeen, ja pysäytä se ennen metagamea; katso
[Palvelin ryhmälle]({{ admin_page.url | relative_url }}#restarting-after-a-change). Deploy-palvelin
ei tarkista yhtäkään näistä arvoista: puuttuvasta arvosta ei ilmoiteta käynnistyksessä, vaan se saa
käynnistykset epäonnistumaan myöhemmin. Jokainen sen käynnistämä pelipalvelin perii sen koko
ympäristön, pelipalvelinavain mukaan lukien.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `PORT` | ei oletusta. Puuttuu: deploy-palvelin pysähtyy käynnistyessään porttivirheeseen. Tyhjä: se kuuntelee satunnaisessa portissa, jota metagame ei löydä. | 1-65535 | HTTP-portti. Vain samalla koneella oleva metagame kutsuu sitä `DEPLOYSERVER_URL`-osoitteen kautta. | Sinä (61001); paketti: aina (61001, hiekkalaatikkotilassa 62001, tai `-DeployPort`) |
| `BIND_HOST` | `127.0.0.1` (myös tyhjänä) | yksi IP-osoite | **Vain forkissa.** Kuunteluosoite. Pidä `127.0.0.1`: tunnistautumista ei ole, ja kuka tahansa porttiin yltävä voi käynnistää peliprosesseja. Lisäksi sen molemmat reitit torjuvat jokaisen kutsujan, joka ei tule loopbackista tai joka tuli välityspalvelimen kautta. | Sinä; paketti: aina `127.0.0.1` |
| `MY_IP` | ei oletusta. Puuttuu: jokainen käynnistys epäonnistuu. | IPv4-osoite sellaisena kuin pelaajat sen tavoittavat | Osoite, joka annetaan peliohjelmille jokaista pelipalvelinta varten (Ramsgate, Dojo, metsästykset). `127.0.0.1` yhden koneen peliin, Tailscale-osoite yksityisessä tilassa, palvelimen julkinen IPv4 julkisessa tilassa (esimerkiksi `203.0.113.10`). Jos jaetulla palvelimella on `127.0.0.1`, kaverin peli ottaa yhteyden omaan koneeseensa. | Sinä; paketti: aina (julkisessa tilassa julkisen osoitteen IPv4: `-PublicHost`, muuten edellisellä asennuskerralla tallennettu osoite, muuten verkkosovittimen ainoa julkinen IPv4; yksityisessä tilassa Tailscale-osoite; hiekkalaatikkotilassa `127.0.0.1`) |
| `PORT_RANGE_BEGIN` | ei oletusta. Puuttuu: metsästysportteja ei ole, ja jokainen metsästys epäonnistuu. | UDP-portti | Pelipalvelinten alin portti. Metsästykset käyttävät portteja `PORT_RANGE_BEGIN`:stä `PORT_RANGE_END - 2`:een: kuusi kerrallaan välillä 8770-8777. Jos haluat itse kootulla koneella useampia yhtä aikaa, pienennä arvoa ja avaa myös lisäportit. | Sinä (8770); paketti: aina 8770 |
| `PORT_RANGE_END` | ei oletusta. Puuttuu: Ramsgate ei käynnisty. | pidä `8777` | Ramsgate pyörii aina tässä portissa ja Training Dojo sen alapuolisessa. Uudelleen käännetty DLL tunnistaa pysyvän Ramsgaten kartasta. Vanha mukana tuleva DLL poistaa vahtikoiran käytöstä portin perusteella, joten sen kanssa portit on pidettävä ennallaan. | Sinä (8777); paketti: aina 8777 |
| `GAMESERVER_BINARY_PATH` | ei oletusta. Puuttuu tai väärä: deploy-palvelin pysähtyy pian käynnistyksen jälkeen (päätelty koodista). | `Dauntless-Win64-Shipping.exe`-tiedoston koko polku kauttaviivoin | 1.4.4:n exe-tiedosto, joka käynnistetään jokaiseksi pelipalvelimeksi; `dxgi.dll` ja `UndauntedInternalServer.dll` on oltava sen vieressä. Mikä tahansa tiedosto tässä nimetäänkin, se ajetaan pelipalvelinavain komentorivillään, joten vain ylläpitäjät saavat muokata asetustiedostoa. | Sinä; paketti: aina (`<root>/game/Dauntless/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe`) |
| `METAGAME_API_KEY` | ei oletusta. Puuttuu: pelipalvelimet eivät voi puhua metagamelle. | pelipalvelinavain (luotuna 48 heksamerkkiä) | **Salainen: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Annetaan jokaiselle pelipalvelimelle sen ensimmäisenä komentoriviparametrina; palvelin-DLL lähettää sen jokaisen metagamelle menevän pyynnön mukana, ja metagame tallentaa siitä vain SHA-256-tiivisteen. Sen on vastattava `gameserver.key`-tiedostoa ja oltava rekisteröity metagamen tietokantaan. Koneen paikalliset käyttäjät näkevät sen pelipalvelinten komentoriveiltä. | Sinä ([Pystytä palvelin]({{ host_page.url | relative_url }}#metagame)); paketti: aina (otetaan varmuuskopiosta, avaintiedostosta tai vanhasta tiedostosta, muuten luodaan uusi) |
| `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP` | ei oletusta. Puuttuu: käytännössä ei taukoa. | sekunteja; desimaalit toimivat | Pienin tauko kahden pelipalvelimen käynnistyksen välillä. Kaikki käynnistykset odottavat samassa jonossa, joten kolmas samalla hetkellä pyydetty metsästys käynnistyy noin kaksi taukoa myöhemmin. | Sinä (10); paketti: 10, kun arvo puuttuu tai on tyhjä, käsin asetettu arvo säilytetään |
| `ENABLE_DOJO` | tarvittaessa | `1` tai mikä tahansa muu | **Vain forkissa.** `1` käynnistää Training Dojon heti alussa, kuten alkuperäinen projekti. Mikä tahansa muu: Dojo käynnistyy ensimmäisellä kerralla, kun joku ohjataan sinne matchmakingissa, ja sulkeutumisen jälkeen se käynnistyy seuraavasta pyynnöstä. | Sinä (0); paketti: 0, kun arvo puuttuu, muuten säilytetään |
| `PERSISTENT_WORLD_LIVENESS` | päällä | `0`, tai mikä tahansa muu päälle | **Vain forkissa.** Päällä: ennen kuin Ramsgate tai Dojo annetaan pelaajalle, deploy-palvelin tarkistaa, että sen prosessi on elossa, ja käynnistää kaatuneen ensin saman yhden käynnistyksen kautta, jota palvelimen käynnistys ja vahtikoira käyttävät (portissa 8777 ei koskaan kahta prosessia). Käynnistysrivi kertoo `Ramsgate and Dojo liveness check before handing them out: on`. `0`: olemassa olevat tietueet annetaan tarkistamatta; vahtikoira käynnistää Ramsgaten minuutin sisällä ja poistaa kuolleen Dojon tietueen seuraavaa pyyntöä varten. | Oletuksena ei kukaan; paketti: säilyttää |
| `LOG_LEVEL` | `info` (myös tyhjänä) | `fatal`, `error`, `warn`, `info`, `debug`, `trace`, `silent` | Lokitaso. `info`-tasolla matchmaking-rivi luettelee odotettujen pelaajien tilitunnukset; avainta ei koskaan kirjata. | Oletuksena ei kukaan; paketti: säilyttää |
| `NODE_ENV` | puuttuu: värilliset, luettavat lokit | `production` tai mikä tahansa muu | `production`: JSON-lokirivit, eikä virhesivuilla ole pinojälkiä. | Sinä; paketti: aina `production` |

---

## Yhdyskäytävä (`legacy/UndauntedGateway/`, `dist/server.js`) {#gateway}

Julkisen tilan ainoa julkinen TCP-kuuntelija. Se purkaa TLS-salauksen ja välittää pyynnöt metagamelle
ja sisältöpalvelimelle loopbackissa. Yhdyskäytävän oma
[README (englanniksi)]({{ site.github.repository_url }}/blob/dauntless-revived/legacy/UndauntedGateway/README.md)
selittää reitityksen, torjunnat ja sen, miten rajat on mitoitettu.

**Milloin luetaan:** kerran, käynnistyessä. Sallitun alueen ulkopuolinen arvo pysäyttää
yhdyskäytävän virheeseen, joka nimeää muuttujan. Yhdyskäytävä ei myöskään käynnisty varmenteella,
joka on vanhentunut, ei vielä voimassa tai ei sovi avaimeen, ja se varoittaa, kun voimassaoloa on
jäljellä alle 30 päivää. `NODE_ENV` ei vaikuta tässä mitään.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `GATEWAY_BIND` | `0.0.0.0` | IP-osoite (ei nimiä) | TLS-kuuntelijan osoite. Pidä `0.0.0.0` (IPv4): osoitteen, jonka yhdyskäytävä ilmoittaa sallittujen listalle, on oltava se IPv4-osoite, josta pelaajan peliliikenne tulee. | Paketti: aina (`0.0.0.0`; hiekkalaatikkotilassa `127.0.0.1`) |
| `GATEWAY_PORT` | `443` | 1-65535 | TLS-portti ja julkisten kutsujen portti. Ainoa portti, jonka paketti avaa kaikille; pelin UDP-portit aukeavat vain kirjautuneille pelaajille (katso sallittujen listan apuri alempana). | Paketti: aina (443 tai `-GatewayPort`; hiekkalaatikkotilassa 62443) |
| `GATEWAY_CERT` | ei oletusta, pakollinen | PEM-varmenteen polku | Varmenne, jonka yhdyskäytävä esittää TLS-yhteyksissä. Yhdyskäytävä kirjaa lokiin sen SHA-256-sormenjäljen, jonka jokainen kutsu kiinnittää. Uusi varmenne rikkoo jokaisen jo lähetetyn kutsun. | Paketti: aina (`<root>/data/tls/gateway-cert.pem`) |
| `GATEWAY_KEY` | ei oletusta, pakollinen | PEM-yksityisavaimen polku | **Salainen tiedosto: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Varmenteen yksityinen avain. | Paketti: aina (`<root>/data/tls/gateway-key.pem`) |
| `CAPTURE_DIR` | pois | kansion polku | **Vain forkissa, Rust-siirtymää varten.** Tallentaa liikenteen varjotoistoa varten (MIGRATION_PLAN.md M2): metagamessa jokaisen pyynnön, joka ei tullut yhdyskäytävän kautta (pelipalvelimet, yksityisen tilan pelaajat), ja koko chatin; yhdyskäytävässä jokaisen pyynnön. Yksi redaktoitu JSON-rivi kustakin tiedostoon `<dir>/metagame-<vvvv-kk-pp>.jsonl` tai `gateway-<vvvv-kk-pp>.jsonl`; tunnukset, tiliavaimet ja salaisuudet korvataan paikkamerkeillä. **Henkilötietoja:** katso [Yksityisyys]({{ legal_page.url | relative_url }}#traffic-recording). Virheellinen arvo estää käynnistyksen. | Ei kukaan oletuksena |
| `CAPTURE_RETENTION_DAYS` | `14` | 1-90 | `CAPTURE_DIR`-asetuksella: tätä vanhemmat tallennetiedostot poistetaan. | Ei kukaan oletuksena |
| `CAPTURE_MAX_BODY_BYTES` | `262144` | 0-16777216 | `CAPTURE_DIR`-asetuksella: suuremmasta rungosta tallennetaan vain koko. | Ei kukaan oletuksena |
| `GATEWAY_SECRET` | ei oletusta, pakollinen | 32-256 tulostettavaa merkkiä ilman välilyöntejä | **Salainen: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Lähetetään otsakkeena `X-Dauntless-Gateway` jokaisen pyynnön mukana, jonka yhdyskäytävä välittää eteenpäin (metagamelle, sisältöpalvelimelle ja WebSocket-palvelulle); vain metagame tarkistaa sen, ja arvon on oltava sama kuin metagamen `GATEWAY_SECRET`. Asiakkaan itse lähettämä samanniminen otsake pudotetaan pois. Ei koskaan lokiin. | Paketti: aina (64 heksamerkkiä, säilyy uudelleenajoissa) |
| `GATEWAY_METAGAME_URL` | `http://127.0.0.1:61000` | `http://host:port` vain tällä koneella (`127.x.x.x`, `::1` tai `localhost`), ilman polkua | Minne menee jokainen pyyntö, joka ei ole `/content` eikä WebSocket-yhteyden avaus (upgrade). Loopback vaaditaan, jotta salainen otsake ei koskaan poistu koneelta. | Paketti: aina |
| `GATEWAY_CONTENT_URL` | `http://127.0.0.1:61002` | sama sääntö | Minne `/content` ja `/content/...` menevät (sisältöpalvelin). | Paketti: aina |
| `GATEWAY_WS_URL` | `http://127.0.0.1:61099` | sama sääntö | Minne WebSocket-yhteyksien avaukset menevät: metagamen chat-kuuntelijalle ([`CHAT_PORT`](#metagame-chat)). Kun chat on pois päältä, siellä ei kuuntele mikään, ja avaukset saavat vastauksen 502. | Paketti: aina (61099; hiekkalaatikkotilassa 62099), sama portti kuin `CHAT_PORT` |
| `GATEWAY_ALLOWLIST` | päällä | tarkalleen `0` ottaa sen pois päältä | Hätäkatkaisin, joka lopettaa kirjautuneiden pelaajien osoitteiden ilmoittamisen sallittujen listan apurille. Pois päältä: `ALLOWLIST_*`-arvot ohitetaan, lokiin kirjataan varoitus, eikä yhdenkään pelaajan peliportit aukea, ellet hoida palomuuria muulla tavalla. | Ei kukaan; paketti ei koskaan kirjoita sitä |
| `ALLOWLIST_URL` | `http://127.0.0.1:61005` | `http://host:port` vain tällä koneella | Minne yhdyskäytävä ilmoittaa pelaajien osoitteet. | Paketti: aina |
| `ALLOWLIST_SECRET` | ei oletusta; pakollinen, ellei `GATEWAY_ALLOWLIST=0` | 32-256 tulostettavaa merkkiä ilman välilyöntejä | **Salainen: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Lähetetään sallittujen listan apurille; arvon on oltava sama kuin apurin `ALLOWLIST_SECRET`. Paketin palvelimella tämä kopio on `gateway.env`-tiedostossa, jota yhdyskäytävää ajava palvelutili voi lukea. | Paketti: aina (sama arvo kuin `allowlist.env`-tiedostossa, säilyy uudelleenajoissa) |
| `GATEWAY_ALLOWLIST_REFRESH_SECONDS` | `60` | 5-540 | Kuinka kauan yhdyskäytävä odottaa ennen kuin ilmoittaa saman osoitteen uudelleen. Pidä arvo selvästi apurin `ALLOWLIST_TTL_SECONDS`-arvoa pienempänä. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_MAX_BODY_BYTES` | `131072` (128 KiB) | 1024-67108864 | Pyynnön sisällön enimmäiskoko; suurempi saa vastauksen 413. Suurin sisältö, jonka peli lähettää, hahmon tallennus, on noin 22 kt. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_MAX_CONNECTIONS` | `2048` | 1-100000 | Avoimet yhteydet yhteensä. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_MAX_CONNECTIONS_PER_IP` | `128` | 1-100000 | Avoimet yhteydet IPv4-osoitetta tai IPv6-/64-verkkoa kohden. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_RATE_GENERAL` | `300,180` | `<kerralla>,<minuutissa>` | Pyyntökiintiö kaikelle peliliikenteelle, joka ei kuulu alla oleviin kiintiöihin. Kun kiintiö on tyhjä, vastaus on 429 ja `Retry-After`-otsake. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_RATE_CONTENT` | `600,600` | `<kerralla>,<minuutissa>` | Pyyntökiintiö `/content`-latauksille. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_RATE_REGISTER` | `5,0.2` | `<kerralla>,<minuutissa>` | Pyyntökiintiö tilin luonnille: viisi kerralla, sitten yksi viiden minuutin välein. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_RATE_TOKEN` | `10,1` | `<kerralla>,<minuutissa>` | Pyyntökiintiö kirjautumisille. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_RATE_WS` | `20,12` | `<kerralla>,<minuutissa>` | WebSocket-avausten (pelin chat-yhteyden) kiintiö. Oma kiintiönsä, joten chatin uudelleenyhdistämiset eivät koskaan kuluta sitä kiintiötä, jota pelin HTTP-liikenne tarvitsee. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_RATE_CONNECT` | `200,300` | `<kerralla>,<minuutissa>` | Uusien TCP-yhteyksien kiintiö (jokainen vaatii TLS-kättelyn). Kun kiintiö on tyhjä, yhteys suljetaan vastaamatta. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_HANDSHAKE_TIMEOUT_MS` | `10000` | 1000-120000 | TLS-kättelyn aikaraja. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_HEADERS_TIMEOUT_MS` | `10000` | 1000-120000 | Aika, jonka pyynnön otsakkeet saavat viedä (vastaus 408). Myös taustapalvelun aikaraja WebSocket-kättelylle. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_REQUEST_TIMEOUT_MS` | `30000` | 1000-600000 | Aika koko pyynnölle sisältöineen (vastaus 408). | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_KEEPALIVE_TIMEOUT_MS` | `65000` | 1000-600000 | Kuinka kauan keep-alive-yhteys saa olla jouten pyyntöjen välillä. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_IDLE_TIMEOUT_MS` | `120000` | 1000-3600000 | Asiakasyhteys, jossa ei liiku dataa näin pitkään aikaan, suljetaan. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_UPSTREAM_IDLE_TIMEOUT_MS` | `120000` | 1000-3600000 | Taustapalvelulle menevä pyyntö, josta ei kuulu mitään näin pitkään aikaan, keskeytetään (504). | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_WS_IDLE_TIMEOUT_MS` | `300000` | 1000-86400000 | Yhdistetyn WebSocket-yhteyden tyhjäkäynnin aikaraja, molemmilla puolilla. | Oletuksena ei kukaan; paketti: säilyttää |
| `GATEWAY_SHUTDOWN_GRACE_MS` | `10000` | 0-300000 | Kun yhdyskäytävä saa pysäytyssignaalin, keskeneräiset pyynnöt saavat näin paljon aikaa valmistua, ennen kuin jokainen yhteys katkaistaan. | Oletuksena ei kukaan; paketti: säilyttää |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` tai `error`, kirjainkoolla ei väliä; mikä tahansa muu tarkoittaa `info` | Lokitaso. `debug` kirjaa myös epäonnistuneet TLS-kättelyt. | Oletuksena ei kukaan; paketti: säilyttää |

Kiintiöt lasketaan IPv4-osoitetta tai IPv6-/64-verkkoa kohden. `<kerralla>` on se, montako pyyntöä
mahtuu kiintiöön kerralla, ja `<minuutissa>` se, montako kiintiöön palaa minuutissa: `<kerralla>`
1-1000000, `<minuutissa>` enintään 1000000, ja desimaalit sallitaan. Jokainen pyyntö kuluttaa
kiintiöstä yhden ennen mitään muuta tarkistusta, myös pyynnöt, jotka sitten torjutaan.

---

## Sallittujen listan apuri (`legacy/UndauntedGateway/`, `dist/allowlist/server.js`) {#allowlist-helper}

Pieni palvelu, joka pitää yhden Windowsin palomuurisäännön auki kirjautuneiden pelaajien osoitteille.
Se pyörii ylläpitäjän oikeuksin (paketin palvelimella SYSTEM-tilillä), joten sen asetustiedosto on
vain ylläpitäjien ja SYSTEMin luettavissa.

**Milloin luetaan:** kerran, käynnistyessä. Virheellinen arvo pysäyttää apurin virheeseen, joka
nimeää muuttujan. `NODE_ENV` ei vaikuta tässä mitään.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `ALLOWLIST_DRY_RUN` | ei oletusta, pakollinen | `0` tai `1` | `1`: ei koskaan koske palomuuriin; jokainen muutos, jonka apuri tekisi, kirjoitetaan tarkastuslokiin. `0`: muuttaa palomuuria oikeasti; vaatii Windowsin ja ylläpitäjän oikeudet, muuten apuri sulkeutuu. Käytä arvoa `1` jokaisella koneella, joka ei ole palvelin. | Paketti: aina (`0`; `1` valinnalla `-AllowlistDryRun` tai `-Sandbox`) |
| `ALLOWLIST_BIND` | `127.0.0.1` | vain `127.0.0.1` tai `::1` | Kuunteluosoite. Muu arvo torjutaan käynnistyksessä. | Paketti: aina `127.0.0.1` |
| `ALLOWLIST_PORT` | `61005` | 1-65535 | Kuunteluportti; sen on vastattava yhdyskäytävän `ALLOWLIST_URL`-arvoa. | Paketti: aina (61005, hiekkalaatikkotilassa 62005, tai `-AllowlistPort`) |
| `ALLOWLIST_SECRET` | ei oletusta, pakollinen | 32-256 tulostettavaa merkkiä ilman välilyöntejä | **Salainen: älä koskaan jaa, älä koskaan tallenna versionhallintaan.** Jokaisessa apurille menevässä pyynnössä on oltava tämä; arvon on oltava sama kuin yhdyskäytävän `ALLOWLIST_SECRET`. Ei koskaan lokiin. | Paketti: aina (säilyy uudelleenajoissa) |
| `ALLOWLIST_PORTS` | `8770-8777` | yksi portti tai väli muodossa `alin-ylin` | Paikalliset UDP-portit, jotka palomuurisääntö avaa. Arvon on vastattava deploy-palvelimen porttiväliä. | Paketti: aina `8770-8777` |
| `ALLOWLIST_TTL_SECONDS` | `600` | 1-86400 | Kuinka kauan osoite pysyy auki viimeisen ilmoituksen jälkeen. | Paketti: aina `600` (käsin asetettu arvo kirjoitetaan yli) |
| `ALLOWLIST_MIN_INTERVAL_MS` | `3000` | 50-600000 | Lyhin aika kahden palomuurisäännön muutoksen välillä; välissä tulleet muutokset yhdistetään. | Oletuksena ei kukaan; paketti: säilyttää |
| `ALLOWLIST_MAX_ENTRIES` | `256` | 1-1000 | Yhtä aikaa auki olevien osoitteiden enimmäismäärä; sen yli menevä uusi osoite torjutaan. | Oletuksena ei kukaan; paketti: säilyttää |
| `ALLOWLIST_AUDIT_LOG` | `allowlist-audit.log` työhakemistossa | tiedostopolku | Loki, johon vain lisätään: yksi JSON-olio riviä kohden jokaisesta muutoksesta ja torjunnasta (lisätyt, vanhentuneet ja torjutut osoitteet, sääntömuutokset, väärällä salaisuudella tulleet pyynnöt). Siinä on pelaajien IP-osoitteita, ei koskaan salaisuutta. | Paketti: aina (`<root>/data/allowlist/audit.log`) |
| `ALLOWLIST_STATE_FILE` | `allowlist-state.json` työhakemistossa | tiedostopolku | Auki olevat osoitteet tallennettuina, jotta uudelleenkäynnistys avaa portit samoille pelaajille. Siinä on pelaajien IP-osoitteita. | Paketti: aina (`<root>/data/allowlist/state.json`) |
| `ALLOWLIST_ALLOW_PRIVATE` | pois | tarkalleen `1` ottaa sen käyttöön | Hyväksyy myös yksityiset, CGNAT- (Tailscale mukaan lukien), loopback- ja link-local-osoitteet. Testeihin ja lähiverkkoasennuksiin; pidä se pois päältä julkisella palvelimella. | Paketti: `1` vain hiekkalaatikkoasennuksissa, muuten poistetaan |
| `ALLOWLIST_POWERSHELL` | `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe` (`C:\Windows`, jos `SystemRoot` puuttuu) | ohjelman koko polku | PowerShell, jonka apuri ajaa ylläpitäjän oikeuksin jokaista palomuurimuutosta varten. Jätä asettamatta. Kuka tahansa, joka voi muokata tätä asetusta, voi ajaa minkä tahansa ohjelman ylläpitäjänä, ja siksi paketti tekee `allowlist.env`-tiedostosta vain ylläpitäjien ja SYSTEMin luettavan. | Ei kukaan; paketti: säilyttää |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` tai `error`, kirjainkoolla ei väliä | Lokitaso. | Oletuksena ei kukaan; paketti: säilyttää |

---

## Sisältöpalvelin (`legacy/UndauntedContent/`) {#content-server}

Jakaa tarkistetut pelitiedostot rekisteröityneille tileille sekä isännän uutiset ja kuvat
käynnistimelle. Uutistiedoston ja kuvapaketin (`branding.json`) yksityiskohdat ovat sivulla
[Tiedostot ja data]({{ files_page.url | relative_url }}).

**Milloin luetaan:** kerran, käynnistyessä. Virheellinen arvo pysäyttää sisältöpalvelimen
virheeseen, joka nimeää muuttujan. Itse uutistiedosto ja kuvapaketin kansio luetaan uudelleen ajon
aikana (uutiset 5 sekunnin ja kuvat 10 sekunnin sisällä), joten niiden muokkaaminen ei vaadi
uudelleenkäynnistystä. `NODE_ENV` ei vaikuta tässä mitään.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `PORT` | `61002` | 1-65535 | Kuunteluportti. | Sinä; paketti: aina (61002, hiekkalaatikkotilassa 62002, tai `-ContentPort`) |
| `BIND_HOST` | `127.0.0.1` | pilkuilla erotetut IP-osoitteet tai `localhost` | Kuunteluosoitteet, kullekin oma kuuntelija. Vain loopback- ja Tailscale-osoitteet (100.64.0.0/10, fd7a:115c:a1e0::/48) hyväksytään; kaikki muu, myös `0.0.0.0`, vaatii asetuksen `CONTENT_ALLOW_ANY_BIND=1`. | Sinä; paketti: aina (julkisessa tilassa `127.0.0.1`, yksityisessä tilassa Tailscale-osoite) |
| `CONTENT_ALLOW_ANY_BIND` | pois | tarkalleen `1` ottaa sen käyttöön | Poistaa `BIND_HOST`-tarkistuksen. Vain jos palomuuri pitää internetin ulkona: pelitiedostojen ei koskaan pidä olla saatavilla suoraan internetistä. | Ei kukaan; paketti ei koskaan kirjoita sitä |
| `METAGAME_URL` | `http://127.0.0.1:61000` | `http://`- tai `https://`-URL | Missä tiliavaimet tarkistetaan (`GET /undaunted/api/GetUserInfo`). Jokaisen lataajan avain lähetetään tänne, joten osoitteen on oltava oma metagamesi. | Paketti: aina (`http://<metagamen osoite>:<portti>`) |
| `CONTENT_GAME_DIR` | ei oletusta, pakollinen | kansion polku | Tarkistettu 1.4.4-asennus (kansio, jossa on `Archon\`), joka avataan vain luku -tilassa. Vain sisältöluettelossa (manifest) mainittuja tiedostoja jaetaan koskaan. `npm run verify -- --game-dir <kansio>` ohittaa sen tarkistusta varten. | Sinä; paketti: aina (`<root>/game/Dauntless`) |
| `CONTENT_MANIFEST` | sisäänrakennettu `data/dauntless-1.4.4.json` | tiedostopolku | Toinen sisältöluettelo testeihin, joissa oikean pelikansion paikalla on korvike. | Paketti: vain hiekkalaatikkotilassa valinnalla `-ContentManifest`, muuten poistetaan |
| `CONTENT_BRANDING_DIR` | puuttuu: ei kuvia | kansion polku | Isännän kuvapaketti käynnistimelle: kuvat sekä valinnainen `branding.json`. Jaa vain kuvia, joihin sinulla on oikeudet. | Paketti: aina (`<root>/data/branding`) |
| `CONTENT_NEWS_FILE` | puuttuu: ei uutisia | tiedostopolku | Isännän uutiset käynnistimelle. Jos muokkaus rikkoo tiedoston, viimeisin toimiva versio pysyy tarjolla. | Paketti: aina (`<root>/data/config/news.json`, luodaan tyhjänä, jos puuttuu) |
| `CONTENT_MAX_STREAMS_PER_ACCOUNT` | `6` | 1-64 | Montako latausta yksi tili voi ajaa kerralla (sen yli 429). | Oletuksena ei kukaan; paketti: säilyttää |
| `CONTENT_MAX_STREAMS_TOTAL` | `48` | 1-1024 | Montako latausta koko palvelin ajaa kerralla (sen yli 503). | Oletuksena ei kukaan; paketti: säilyttää |
| `CONTENT_AUTH_CACHE_SECONDS` | `300` | 0-3600 | Kuinka kauan hyväksytyn avaimen tarkistus pidetään välimuistissa (hakuavaimena avaimen tiiviste, ei koskaan itse avain). Torjutut avaimet pidetään välimuistissa 30 sekuntia; metagamen katkoja ei koskaan tallenneta välimuistiin. | Oletuksena ei kukaan; paketti: säilyttää |
| `LOG_LEVEL` | `info` | `debug`, `info`, `warn` tai `error`, kirjainkoolla ei väliä | Lokitaso. | Oletuksena ei kukaan; paketti: säilyttää |

Sisältöpalvelimella ei ole omia salaisuuksia. Tiliavaimet kulkevat sen läpi metagamelle, eikä niitä
koskaan kirjata lokiin.

---

## Käynnistimen ympäristömuuttujat {#launcher}

Kaverikäynnistimellä on oma asetussivunsa (grafiikka, automaattinen valotus, ikkuna, kieli). Missä se säilyttää asetukset,
kerrotaan sivulla [Tiedostot ja data]({{ files_page.url | relative_url }}), ja mitä grafiikkataso
kirjoittaa pelin ini-tiedostoihin, sivulla [Pelin asetukset]({{ gamesettings_page.url | relative_url }}).
Ympäristöstään käynnistin lukee vain alla olevat muuttujat.

| Nimi | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-------|:------|:--------------|:-------------|
| `DAUNTLESS_REVIVED_RELAY_PORT` | puuttuu: 61000 | kokonaisluku 1024-65535 ilman etunollaa; kaikki muu ohitetaan | Vain testeihin, esimerkiksi koneella, jolla kehityspino jo varaa portin 61000. Julkisessa tilassa se siirtää tähän porttiin käynnistimen paikallisen välittimen, osoitteen, jolla peli käynnistetään, ja pelin chat-portin `Engine.ini`-tiedostossa, ja käynnistin kirjaa lokiin varoituksen. Yksityinen tila ohittaa sen. Kavereiden on pidettävä portti 61000: palvelin antaa jokaiselle pelaajalle osoitteen `http://127.0.0.1:61000/QoS`. | Kehittäjä tai testaaja, käynnistimen ympäristössä (`$env:DAUNTLESS_REVIVED_RELAY_PORT = "62100"` ennen käynnistimen avaamista). Mikään asennusohjelma tai paketti ei koskaan aseta sitä. |
| `LOCALAPPDATA` (varalla `USERPROFILE`) | Windows asettaa | kansio | Pelin oletuskansio (`%LOCALAPPDATA%\DauntlessRevived\Game`), pelin käyttäjäasetusten kansio, jonka käynnistin kirjoittaa uudelleen, ja yksi paikka, josta käynnistin etsii `tailscale.exe`-ohjelmaa. | Windows |
| `ProgramW6432`, `ProgramFiles`, `ProgramFiles(x86)`, `PATH` | Windows asettaa | kansiot | Mistä muualta käynnistin etsii `tailscale.exe`-ohjelmaa (vain yksityisessä tilassa). Kansio `C:\Program Files\Tailscale` kokeillaan aina. | Windows |
| `WINDIR` (varalla `SystemRoot`) | Windows asettaa | kansio | Mistä käynnistin tarkistaa Visual C++ -ajonaikaisen kirjaston, jota palvelin-DLL tarvitsee. | Windows |

Asennettu käynnistin ei välitä muuttujista `NODE_OPTIONS` ja `ELECTRON_RUN_AS_NODE` eikä Noden
inspector-valitsimista: ne kytketään pois, kun sovellus paketoidaan, joten kukaan ei voi syöttää
siihen koodia ympäristön kautta.

---

## Mitä Windows-palvelinpaketti kirjoittaa {#server-kit}

`Install-DauntlessServer.ps1` kirjoittaa viisi tiedostoa kansioon `<root>\data\config\` (oletusjuuri
`C:\DauntlessRevived`). Joka ajokerralla se lukee olemassa olevan tiedoston, ja `-RestoreFrom`-valinnan
kanssa myös varmuuskopion `metagame.env`- ja `deployserver.env`-tiedostot (varmuuskopion arvot
voittavat), säilyttää jokaisen löytämänsä avaimen ja asettaa sitten avaimet, jotka kuvaavat tätä
konetta. Käsin lisäämäsi asetus, kuten `PROGRESSION_MODE=stub`, säilyy siis asennuksen
uudelleenajoissa ja päivityksissä; käsin muokattu avain sarakkeesta ”Asennus asettaa aina” ei säily.
Muuta niitä sen sijaan asennusohjelman parametreilla
([Skriptit ja parametrit]({{ scripts_page.url | relative_url }})).

Muut säännöt:

- Kommenttirivejä ei säilytetä: tiedosto kirjoitetaan uudelleen yhden otsikkorivin kanssa.
- Arvot, joissa on lainausmerkki, kenoviiva tai rivinvaihto, torjutaan, joten polut kirjoitetaan
  kauttaviivoin.
- `Update-DauntlessServer.ps1` muuttaa vain `metagame.env`-tiedoston `GIT_COMMIT`-arvoa.
- Portit tulevat parametreista `-MetagamePort`, `-DeployPort`, `-ContentPort`, `-AllowlistPort` ja
  `-GatewayPort`, muuten olemassa olevan asennuksen `server.json`-tiedostosta, muuten sivun
  [Portit ja verkko]({{ ports_page.url | relative_url }}) oletuksista.
- `NODE_ENV=production` kirjoitetaan kaikkiin viiteen tiedostoon. Vain metagame ja deploy-palvelin
  lukevat sen.
- Chat: `-Chat On` tai `-Chat Off`, muuten olemassa olevan asennuksen `server.json` (`"Chat"`), muuten
  On. Valinta tallennetaan tiedostoon `server.json`. `Set-Chat.ps1 -On` tai `-Off` muuttaa sitä
  myöhemmin sekä tiedostossa `metagame.env` että `server.json`. Yksityisessä tilassa chattia ei vielä
  ole.
- Asennusohjelma ei kirjoita yhtään Harmonicin forkin siirron mukana tullutta kytkintä
  (`ESCALATION_*`, `STORE*`, `SLAYER_LINKS`, `CHAT_PRESENCE`, `VERIFY_STUB_ACCOUNT`,
  `BALANCE_FROM_INVENTORY`, `PROGRESSION_REPLAY_WINDOW_S`, `PROGRESSION_CONFIRM_ENTITLEMENTS`,
  `PROGRESSION_CONFIG_DIR`, `ACTIVE_HUNT_PASS`, `BODY_LOG_PER_PATH` sekä `PERSISTENT_WORLD_LIVENESS`
  tiedostossa `deployserver.env`), joten niiden oletukset ovat voimassa. Muuttaaksesi jotakin lisää se
  käsin; se säilyy uudelleenajoissa ja päivityksissä
  ([Palvelin ryhmälle]({{ admin_page.url | relative_url }}#switching-features-on)).

| Tiedosto | Kirjoitetaan | Asennus asettaa aina | Asetetaan vain, jos puuttuu | Poistetaan | Säilytetään (sinun) |
|:---------|:-------------|:---------------------|:----------------------------|:-----------|:--------------------|
| `metagame.env` | joka ajokerralla | `PORT`, `BIND_HOST`, `AUTH_MODE`, `DB_FILENAME`, `TARGET_CHANGELIST`, `QOS_TARGET_URL`, `MATCHMAKING_MODE`, `DEPLOYSERVER_URL`, `REGISTRATION_MODE`, `NODE_ENV`, `SERVER_NAME`, `SOURCE_URL`, `GIT_COMMIT`, `BODY_LOG_FILE`; julkisessa tilassa myös `GATEWAY_SECRET`, `LOG_BODIES=0`, `CHAT` (valinnasta `-Chat`, muuten `server.json`), `CHAT_BIND_HOST=127.0.0.1` ja `CHAT_PORT` (`GATEWAY_WS_URL`-osoitteen portti); sisältöpalvelimen kanssa myös `CONTENT_PORT` | `AUTH_SIGNING_PRIVKEY_B64`, `AUTH_SIGNING_PUBKEY_B64` (uusi pari) | `GATEWAY_SECRET` yksityisessä tilassa (ja `CHAT` muuttuu siellä arvoksi `0`); `CONTENT_PORT` ilman sisältöpalvelinta | kaikki muu, esimerkiksi `PROGRESSION_*`, `SAVE_HISTORY_*`, `LOG_LEVEL`, `DB_WAL`, `CHAT_NICK_CHECK`, `CHAT_TRACE` |
| `deployserver.env` | joka ajokerralla (myös hiekkalaatikkotilassa, jossa deploy-palvelinta ei ajeta) | `PORT`, `BIND_HOST=127.0.0.1`, `MY_IP`, `PORT_RANGE_BEGIN=8770`, `PORT_RANGE_END=8777`, `GAMESERVER_BINARY_PATH`, `METAGAME_API_KEY`, `NODE_ENV` | `SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP=10` (myös tyhjän tilalle), `ENABLE_DOJO=0` | ei mitään | `LOG_LEVEL`, oma `ENABLE_DOJO`-arvosi ja käynnistystaukosi |
| `content.env` | kun asennetussa koodissa on sisältöpalvelin | `PORT`, `BIND_HOST`, `METAGAME_URL`, `CONTENT_GAME_DIR`, `CONTENT_BRANDING_DIR`, `CONTENT_NEWS_FILE`, `NODE_ENV` | ei mitään | `CONTENT_MANIFEST` (paitsi hiekkalaatikkotilassa valinnalla `-ContentManifest`) | `CONTENT_MAX_STREAMS_*`, `CONTENT_AUTH_CACHE_SECONDS`, `LOG_LEVEL` |
| `gateway.env` | julkisessa tilassa | `GATEWAY_BIND`, `GATEWAY_PORT`, `GATEWAY_CERT`, `GATEWAY_KEY`, `GATEWAY_SECRET`, `GATEWAY_METAGAME_URL`, `GATEWAY_CONTENT_URL`, `GATEWAY_WS_URL`, `ALLOWLIST_URL`, `ALLOWLIST_SECRET`, `NODE_ENV` | ei mitään | `GATEWAY_CHAT_URL`, `GATEWAY_ACCESS_LOG` (nimiä varhaisista luonnoksista) | rajat, kiintiöt, aikarajat, `GATEWAY_ALLOWLIST_REFRESH_SECONDS`, `LOG_LEVEL` |
| `allowlist.env` | julkisessa tilassa | `ALLOWLIST_BIND`, `ALLOWLIST_PORT`, `ALLOWLIST_SECRET`, `ALLOWLIST_PORTS`, `ALLOWLIST_TTL_SECONDS=600`, `ALLOWLIST_AUDIT_LOG`, `ALLOWLIST_STATE_FILE`, `ALLOWLIST_DRY_RUN`, `NODE_ENV`; hiekkalaatikkotilassa myös `ALLOWLIST_ALLOW_PRIVATE=1` | ei mitään | `ALLOWLIST_ALLOW_PRIVATE` hiekkalaatikkotilan ulkopuolella; `ALLOWLIST_RULE_NAME`, `ALLOWLIST_UDP_PORTS`, `ALLOWLIST_PROGRAM` (nimiä varhaisista luonnoksista) | `ALLOWLIST_MIN_INTERVAL_MS`, `ALLOWLIST_MAX_ENTRIES`, `ALLOWLIST_POWERSHELL`, `LOG_LEVEL` |

**Mistä salaisuudet tulevat.** Tunnisteiden allekirjoitusavainpari otetaan varmuuskopiosta tai
olemassa olevasta tiedostosta, muuten se luodaan kerran (RSA-2048). Pelipalvelinavain
(48 heksamerkkiä) otetaan varmuuskopiosta, sitten tiedostosta `data\keys\gameserver.key`, sitten
vanhasta `deployserver.env`-tiedostosta, muuten se luodaan. `GATEWAY_SECRET` ja `ALLOWLIST_SECRET`
(kumpikin 64 heksamerkkiä) säilyvät uudelleenajoissa ja luodaan uusina uudella koneella; palautus ei
tarvitse niitä. Asennusohjelma ei koskaan tulosta yhtäkään niistä.

**Kuka voi lukea tiedostoja.** Pinoa ajava palvelutili voi vain lukea kansiota `data\config\`, ja
`allowlist.env` on vain ylläpitäjien ja SYSTEMin luettavissa. Muokkaa tiedostoja ylläpitäjänä avatulla
editorilla ja käynnistä osa sitten uudelleen komennolla `Stack.ps1 restart -Only <osa>`.

**Varmuuskopiot.** `Backup-DauntlessServer.ps1` kopioi tiedostot `metagame.env`, `deployserver.env`,
`content.env` ja `gateway.env` kansioon `backups\<päivä>_<aika>\secrets\`, samoin kansion `data\keys\`
`*.key`-tiedostot sekä yhdyskäytävän TLS-varmenteen ja sen avaimen. `allowlist.env`-tiedostoa ei
kopioida (vain ylläpitäjät ja SYSTEM voivat lukea sen, ja asennus uudelle koneelle luo uuden
sallittujen listan salaisuuden). `gateway.env`-tiedostossa on kopiot arvoista `GATEWAY_SECRET` ja
`ALLOWLIST_SECRET`, joten varmuuskopiossa on palvelimen kaikki salaisuudet: kopioi varmuuskopiot
palvelimelta pois vain salattuina äläkä koskaan tallenna niitä versionhallintaan.

---

## Kaveripaketti ja peliohjelman puoli {#client-side}

Kaveripaketti (`friend-kit/`) ja käynnistin eivät aseta palvelimelle mitään. Kaveripaketin
`play.ps1` ottaa palvelimen muodossa `-Server host[:port]` (portti 61000, jos se jätetään pois) ja
antaa sen pelille ensimmäisenä komentoriviparametrina; käynnistin ottaa osoitteen kutsusta. Molemmat
pitävät pelaajan tiliavaimen pelaajan omalla koneella. Katso
[Liity kaverina]({{ friends_page.url | relative_url }}) ja
[Pelin asetukset]({{ gamesettings_page.url | relative_url }}).

---

## Windowsin muuttujat, joita skriptit lukevat {#windows-variables}

Palvelinpaketin skriptit ja kaveripaketti ottavat omasta prosessistaan muutaman Windowsin
vakiomuuttujan. Niitä ei tarvitse asettaa itse; taulukko kertoo, mihin kutakin käytetään.

| Nimi | Kuka lukee | Mihin sitä käytetään |
|:-----|:-----------|:---------------------|
| `SSH_CONNECTION` | `Install-DauntlessServer.ps1` | OpenSSH-palvelin asettaa sen. Kun asennusohjelma ajetaan SSH:n kautta (kuten `Deploy-Remote.ps1` tekee), avainkirjautuminen on todistetusti toiminut, joten asennusohjelma ottaa SSH:n salasanakirjautumisen pois käytöstä. Konsolista tai etätyöpöydältä ajettuna se vain varoittaa. |
| `COMPUTERNAME` | palvelinpaketti | Muodostaa palvelutilin nimen `<COMPUTERNAME>\dauntless`. |
| `WINDIR`, `ProgramFiles`, `ProgramData`, `Path`, `TEMP` | palvelinpaketti | `System32`-kansion työkalut (`tar.exe`, `cmd.exe`, OpenSSH), `nodejs\node.exe` ja `Tailscale\tailscale.exe`, `ssh\sshd_config`. Asennusohjelma lisää Node.js-kansion oman istuntonsa `Path`-muuttujaan. `Deploy-Remote.ps1` kokoaa siirrettävät tiedostot `%TEMP%`-kansioon. |
| `APPDATA`, `LOCALAPPDATA`, `WINDIR` | kaveripaketti (`setup.ps1`, `play.ps1`) | Kansiossa `%APPDATA%\DauntlessRevived\` ovat `account.key` (**salainen: älä koskaan jaa sitä, älä koskaan tallenna sitä versionhallintaan**) ja `settings.json`; `play.ps1` kirjoittaa pelin ini-tiedostot kansioon `%LOCALAPPDATA%\Archon\Saved\Config\WindowsClient\`; `setup.ps1` tarkistaa Visual C++ -ajonaikaisen kirjaston kansiosta `%WINDIR%\System32`. |

Kun paketti kääntää koodin (`npm ci`, `npm run build`), se asettaa `NODE_ENV=development`, jotta
TypeScript asentuu, ja hiljentää npm:n päivitys-, rahoitus- ja tarkastusilmoitukset. Tämä koskee vain
käännösvaihetta: käynnissä olevat osat saavat `.env`-tiedostoistaan arvon `NODE_ENV=production`.

---

## CI:n ja julkaisujen asetukset {#ci-settings}

Mikään palvelimen osa ei lue näitä. Ne ovat GitHub-repositorion asetuksia, joita kansion
`.github/workflows/` työnkulut lukevat ([Kehittäjän opas]({{ dev_page.url | relative_url }}#ci)).
Forkilla on omansa.

| Nimi | Laji | Oletus | Arvot | Mitä se tekee | Kuka asettaa |
|:-----|:-----|:-------|:------|:--------------|:-------------|
| `LAUNCHER_AUTO_RELEASE` | Repositorion muuttuja (Settings > Secrets and variables > Actions > Variables), jonka `ci.yml` lukee | puuttuu: päällä | `false` pysäyttää automaattiset julkaisut; asettamattomana ne ovat päällä | Käynnistimen automaattiset julkaisut: `dauntless-revived`-haaraan tehty push, joka läpäisee kaikki tarkistukset, on yhä haaran uusin commit ja jonka käynnistinversiolla ei ole vielä `launcher-v<versio>`-julkaisua, julkaisee kyseisen käynnistimen ([Käynnistimen julkaisut]({{ dev_page.url | relative_url }}#launcher-releases)). Vain repositorio `mixutin/dauntless-revived` julkaisee automaattisesti. Kun arvo on `false`, Actions > **Launcher release** > **Run workflow** julkaisee yhä käsin. | Repositorion omistaja |

Työnkulut eivät tarvitse omia salaisuuksia: ne käyttävät tunnistetta (token), jonka GitHub antaa
jokaiselle ajolle, ja kirjoitusoikeus on vain niillä töillä, jotka tekevät julkaisun. CodeQL-koodiskannaus
on repositorion oletusasetus (default setup) eli repositorion asetus, ei työnkulkutiedosto. GitHubin
muuttumattomat julkaisut (immutable releases) on pidettävä pois päältä, koska
`launcher-updates`-julkaisua, jota asennetut käynnistimet lukevat, päivitetään paikallaan.

---

## Vain testien muuttujat {#test-only}

Vain automaattiset testit lukevat näitä. Testien ajamisesta kerrotaan sivulla
[Kehittäjän opas]({{ dev_page.url | relative_url }}).

| Nimi | Osa | Oletus | Mitä se tekee |
|:-----|:----|:-------|:--------------|
| `TEST_LOG_LEVEL` | metagamen ja deploy-palvelimen testit | `silent` | Lokitaso `npm test` -ajon aikana (kopioidaan muuttujaan `LOG_LEVEL`). Testit asettavat oman `NODE_ENV`-arvonsa, tietokantansa ja porttinsa eivätkä koskaan lue `.env`-tiedostoasi. |
| `CONTENT_IT_GAME_DIR` | sisältöpalvelimen integraatiotesti | `C:\D144\Dauntless` | Oikea 1.4.4-asennus, jota testi jakaa; testi ohitetaan, jos kansiota ei ole. |
| `CONTENT_IT_PORT`, `CONTENT_IT_MOCK_PORT` | sisältöpalvelimen integraatiotesti | `62002`, `62003` | Testattavan palvelimen ja valemetagamen portit. 62002 on myös paketin hiekkalaatikkotilan sisältöportti, joten älä aja molempia yhtä aikaa. |

---

## Nimet, jotka eivät ole asetuksia {#not-settings}

- `GAMESERVER_LAUNCHER` mainitaan tiekartalla ideana pelipalvelinten käynnistämisestä Winen kautta.
  Mikään koodi ei lue sitä.
- Metagamen `BIND_HOST`-asetukselle on suunniteltu luetteloa (`127.0.0.1,<Tailscale-osoite>`).
  Nykyään se ottaa yhden osoitteen.
- `GATEWAY_CHAT_URL`, `GATEWAY_ACCESS_LOG`, `ALLOWLIST_RULE_NAME`, `ALLOWLIST_UDP_PORTS` ja
  `ALLOWLIST_PROGRAM` ovat nimiä palvelinpaketin varhaisista luonnoksista. Mikään ei lue niitä, ja
  asennusohjelma poistaa ne.
- `EXPERIMENTAL_CHAT` kytki päälle ensimmäisen chat-kokeiluversion. Mikään ei lue sitä enää: kytkin on
  [`CHAT`](#metagame-chat), ja metagame varoittaa käynnistyessään, jos se löytää vanhan nimen ilman
  uutta.
- Harmonicin 1.4.4-haarassa on asetuksia, joita emme ottaneet
  ([miksi]({{ '/fi/findings/harmonic-fork.html' | relative_url }})): `WIRE_CAPTURE`,
  `WIRE_CAPTURE_MAX_BODY` ja `WIRE_CAPTURE_MAX_PER_PATH` (meidän `LOG_BODIES` ja `BODY_LOG_PER_PATH`
  hoitavat saman), `HUNT_PASS_SEASONS_DIR` (meillä `PROGRESSION_CONFIG_DIR`),
  `HUNT_PASS_PREMIUM_MODE`, `XMPP_PORT` sekä deploy-palvelimessa `ENABLE_WATCHDOG`, `HOST`,
  `GAMESERVER_LOG_DIR`, `GAMESERVER_LOG_CMDS` ja `METAGAME_ADDRESS`; hänen DLL:ssään
  `UNDAUNTED_DIAG_LOG`. Mikään täällä ei lue niitä.


Julkisen metsästysjonon ryhmittely kestää enintään 5 sekuntia ensimmäisestä liittymisestä
(tila tarkistetaan asiakkaan kyselyllä). Uudelleen käännetty palvelin-DLL sulkee metsästykset,
opetusjaksot ja Dojon, kun niissä ei ole odottavaa tai avointa verkkoyhteyttä 30 sekuntiin.
Ennen ensimmäistä yhteyttä lataamiselle annetaan 180 sekuntia. Aika mitataan monotonisella
kellolla. Ramsgate pysyy käynnissä; Dojo käynnistyy seuraavasta pyynnöstä. Muutos vaatii DLL:n
kääntämisen ja asentamisen: lähdekoodin muokkaus ei päivitä käynnistimen mukana tulevaa DLL:ää.

### Ramsgate-instanssit ja automaattinen aluevalinta

Deploy-palvelimen oletus on `RAMSGATE_POOL=1`. Tyhjä Ramsgate ei käynnisty asennuksen tai
palvelimen käynnistyksen yhteydessä. CITY-pyyntö varaa koko matkustavalle ryhmälle paikat
instanssista, jonka kapasiteetti on 20. Tarvittaessa käynnistyy uusi instanssi. Ensimmäinen
käyttää `PORT_RANGE_END`-porttia; muut jakavat metsästysportit. Dojon portti pysyy varattuna.
Muisti- ja porttirajat koskevat uusia instansseja. Uusi kaupunki odottaa ensimmäistä yhteyttä
180 sekuntia. Sen jälkeen viimeisen odottavan tai avoimen yhteyden poistuminen sulkee sen
seuraavalla pelitickillä. Se ei käynnisty uudelleen ilman pyyntöä. `RAMSGATE_POOL=0` palauttaa
vanhan pysyvän kaupungin. Poolissa vanha `CITY_OVERFLOW`-tasain ohitetaan; alueellinen reititys
valitsee edelleen työntekijän, jolla on oma kaupunkipooli.

Käynnistimen oletus on Automatic (closest region), ellei pelaajalla ole tallennettua käsin
valittua aluetta. Play mittaa saatavilla olevien alueiden TCP-yhteyden muodostuksen kahdesti
ja vertaa toista mittausta. Tämä arvioi verkkoviivettä, ei maantieteellistä etäisyyttä tai
pelin UDP-viivettä. EU, Australia ja Saksa ovat edelleen valittavissa käsin; ryhmä seuraa
johtajaa. Automaattinen valinta arvioidaan uudelleen jokaisella Play-painalluksella.

Metagamen `REGION_MAIN_PROBE_HOST`, `REGION_AUS_PROBE_HOST` ja `REGION_GER_PROBE_HOST`
määrittävät alueiden saavutettavat TCP-kuuntelijat. `REGION_*_PROBE_PORT` on oletuksena 443.
Main käyttää ilman asetusta kutsun osoitetta ja porttia. Australia ja Saksa näytetään vain,
kun `AUS_REGION=1` ja `GERMANY_REGION=1`. Mittaus ei lähetä tunnuksia tai sovellusdataa.
Epäonnistuneet mittaukset ohitetaan; jos kaikki epäonnistuvat, nykyinen backend-alue säilyy.
GET `/undaunted/api/HuntRegion` palauttaa lisäksi `probes: [{region, host, port}]`.

Toiminta vaatii uudelleen käännetyn palvelin-DLL:n ja päivitetyn käynnistimen asentamisen.
Lähdekoodin muutos ei vaihda asennuspaketin mukana tulevia binäärejä.
