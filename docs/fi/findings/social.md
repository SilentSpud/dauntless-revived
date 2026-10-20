---
title: Kaverit, ryhmät ja killat
parent: Löydökset
grand_parent: Dauntless Revived suomeksi
nav_order: 9
lang: fi
ref: findings/social
locale: fi_FI
description: "Miten Dauntless 1.4.4 hoitaa kaverit, ryhmät, killat ja Slayer Linksin taustapalvelun kanssa, luettuna ohjelmatiedostosta ja oikealta palvelimelta: tunnusketju, tarkat vastaukset, miksi ensimmäisessä kahden pelaajan testissä ei näkynyt mitään ja mikä on vielä vahvistamatta."
---

{% assign api_page = site.pages | where: "path", "fi/reference/api.md" | first %}
{% assign config_page = site.pages | where: "path", "fi/reference/configuration.md" | first %}
{% assign files_page = site.pages | where: "path", "fi/reference/files.md" | first %}
{% assign contract_page = site.pages | where: "path", "fi/findings/backend-contract.md" | first %}
{% assign rev_page = site.pages | where: "path", "fi/findings/json-reversing.md" | first %}
{% assign awakening_page = site.pages | where: "path", "fi/findings/awakening-2-1-1.md" | first %}
{% assign friends_page = site.pages | where: "path", "fi/setup/friends.md" | first %}
{% assign chat_page = site.pages | where: "path", "fi/findings/chat.md" | first %}
{% assign harmonic_page = site.pages | where: "path", "fi/findings/harmonic-fork.md" | first %}

# Kaverit, ryhmät ja killat versiossa 1.4.4
{: .no_toc }

Tämä sivu kertoo, miten **1.4.4**-peliohjelma hoitaa Sosiaalinen-paneelin (Social): kaverit, estetyt
pelaajat, ryhmät (party) ja ryhmäkutsut, killat (guild) sekä Slayer Links. Sivulle on kirjattu, mitä peliohjelma
lähettää palvelimelle, mitä se tarvitsee vastaukseksi ja miksi ensimmäisessä kahden pelaajan
testissä 22.9.2026 ei näkynyt mitään. Lopuksi kerrotaan, mitä korjasimme metagameen (taustapalvelimeen,
jonka kanssa peli keskustelee).

**Tilanne 22.9.2026: rakennettu, testattu ilman peliä ja käytössä vuokratulla palvelimellamme (60955e1), kaksi pelaajaa ei ole vielä kokeillut.**
Jokainen alla oleva korjaus läpäisee HTTP-testit, jotka toistavat peliohjelman omat pyynnöt ja
tarkistavat jokaisen vastauksen peliohjelman tulkintaa jäljittelevällä mallilla. Seuraava kahden
pelaajan testi vuokratulla palvelimella vahvistaa tai korjaa ne; [Näin se tarkistetaan](#how-to-verify)
luettelee sen vaiheet ja odotetut lokirivit. Myös tekstichat on rakennettu, ja sillä on oma sivunsa:
[Tekstichat]({{ chat_page.url | relative_url }}).

**Päivitys 23.9.2026, Harmonicin 1.4.4-forkin siirrosta
([Harmonicin työn siirto]({{ harmonic_page.url | relative_url }})), rakennettu ja testattu ilman peliä,
ei vielä palvelimella:** [Slayer Links](#slayer-links) (oletuksena päällä), kavereiden paikalla olo
chat-palvelimen sisällä (oletuksena pois, [Tekstichat]({{ chat_page.url | relative_url }}#presence)),
[kahdesti saapuva ryhmäkutsun hyväksyntä](#a-repeated-accept) onnistuu nyt, ja peliohjelman
[istunnon tarkistus](#verify) nimeää pelaajan oman tilin.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Todisteet ja varmuus {#evidence-and-confidence}

Jokaisesta tiedosta on merkitty, mistä se tulee ja kuinka varmoja olemme.

| Merkki | Lähde |
|:-------|:------|
| **B** | 1.4.4-ohjelmatiedosto (`Dauntless-Win64-Shipping.exe`, 103 Mt): sen merkkijonot (ASCII ja UTF-16) sekä pyyntöjä rakentavien ja vastauksia lukevien funktioiden konekielinen koodi (disassembly). Osoitteet ovat muistiosoitteita, joiden perusosoite on `0x140000000`. |
| **L** | Oikea palvelin: luettelo kaikista reiteistä, joita oikeat peliohjelmat ja pelipalvelimet kutsuivat 22.9.2026, määrineen ja vastaamattomine reitteineen, sekä se, mitä kaksi pelaajaa kertoi nähneensä. |
| **R** | Pyynnöt, jotka versio 2.1.1 lähetti tutkimuspalvelimellemme (samat Phoenixin palvelut, tallennettu runkoineen). |
| **K** | Pelin luokkien ja rakenteiden nimet (SDK-vedos) sekä päätepistetaulukko tiedostossa `native/internal-server/dllmain.cpp`. |
| **C** | Oma koodimme ja testimme. |
| **G** | Oma suunnitteluratkaisumme, kun peliohjelma ei ratkaise asiaa. |

Varmuus: **H** korkea (luettu koodista tai nähty oikeasti), **M** keskitaso (yksi lenkki jäljittämättä),
**L** matala.

Menetelmä, jolla JSON-kenttien nimet luetaan ohjelmatiedostosta, on kuvattu sivulla
[JSON-rakenteen lukeminen ohjelmatiedostosta]({{ rev_page.url | relative_url }}).

## Mitä pelaajat näkivät ja miksi {#what-the-players-saw}

| Mitä pelaajat näkivät | Syy | Korjaus | Varmuus |
|:----------------------|:----|:--------|:--------|
| Ryhmäkutsu tuli toisen pelaajan peliohjelmaan (kutsukysely palautti sen kahdesti), mutta se ei koskaan näkynyt PARTY INVITES -kohdassa. | Ennen kuin paneeli näyttää kutsun, peliohjelma kysyy lähettäjästä reitiltä `POST /accountinfo/public`. Alkuperäiseltä projektilta peritty vastauksemme kuvasi **kysyjää**: kysyjän oma tunnus oli kentissä `accountId` ja `linkedAccounts`. Peliohjelma tallentaa käyttäjätiedot vastauksen `accountId`-tunnuksen alle ja pitää ensimmäisen vastauksen, joten lähettäjä ei koskaan saanut tietoja, se mitätöitiin ja kutsu pudotettiin (B `0x140b74870`, `0x1409ede50`). | `/accountinfo/public` vastaa kysytystä tilistä. | H koodissa, M sille, ettei muuta estettä ole |
| Kaverin lisääminen (Add Friends) "ei löytänyt mitään". | Add Friends ei ole haku vaan nimikenttä ja Add-painike. Nimi löytyi (`GET /account/api/public/account/displayName/<nimi>`), mutta seuraava vaihe, `POST /account/mapping`, sai ensin 404:n ja sitten vastauksen muodossa, jota peliohjelma ei lue. Ilman yhdistämistä peliohjelma pudottaa kaveripyynnön ilmoittamatta mitään. Yksikään kaveripyyntö ei tullut palvelimelle (0 kutsua, L). | `/account/mapping` vastaa muodossa, jota peliohjelma lukee. | H |
| Kaikki, myös pelaajat itse, näkyivät tilassa Offline. | Paikalla olo tulee vain XMPP-viestipalvelimen läsnäolotiedoista chat-yhteyden kautta. HTTP-reittiä paikalla ololle ei ole, eikä chat-palvelimemme lähettänyt läsnäolotietoja chat-huoneiden ulkopuolella. | Rakennettu 23.9.2026, oletuksena pois: kavereiden läsnäolotiedot chat-palvelimessa (`CHAT=1` ja `CHAT_PRESENCE=1`, [Tekstichat]({{ chat_page.url | relative_url }}#presence)); odottaa kahden pelaajan testiä, joka näyttää, että ryhmän automaattinen potku pysyy unessa. | H syylle, M korjaukselle |
| "Unable to create guild." | `POST /guild/validate` (peliohjelmalta) ja `POST /guild` (**pelipalvelimelta**) saivat kumpikin 404:n. | Yksitoista kiltareittiä (versio 2). | H |
| Kukaan ei käyttänyt pelin omaa Invite to Party -toimintoa (illan molemmat kutsut tulivat ylläpitäjän varareitiltä). | Pelaajat eivät löytäneet toisiaan: Hunt Members ja kaverit tarvitsevat saman käyttäjätietovaiheen kuin kutsun lähettäjä. | Sama korjaus kuin ensimmäisellä rivillä. | M |

**Kun korjaukset on viety palvelimelle, kummankin pelaajan on käynnistettävä peli kerran
uudelleen.** Peliohjelma pitää ensimmäiset käyttäjätiedot ja ensimmäisen yhdistämisen kustakin
tunnuksesta koko istunnon ajan, joten aiemmat väärät tiedot jäävät voimaan uudelleenkäynnistykseen
asti.

**Korjaus aiempaan.** Aiemmissa muistiinpanoissamme ja tiekartan kohdassa 1.11 syytettiin reittiä
`/account/mapping` sekä kaverihausta että ryhmäkutsusta. Se on vain puoliksi totta. Kaverin lisääminen
pysähtyi reitille `/account/mapping` (ja olisi seuraavaksi pysähtynyt reitille `/accountinfo/public`).
Vastaanotettu ryhmäkutsu ei käytä yhdistämistä lainkaan: sen lähettäjän tunnus on jo Phoenix-tunnus,
ja kutsu pudotettiin reitillä `/accountinfo/public`. Kutsun ympärillä illalla näkyneet
yhdistämiskutsut sopivat muihin kutsujiin: kahteen kaverin lisäysyritykseen ja peliohjelman omaan
yhdistämiseen, jonka se tekee kirjautuessaan omalle tunnukselleen. Tämä kirjautumisen aikainen kutsu
perustuu ohjelmatiedoston lokitekstiin ja 2.1.1-tallenteeseen (B, R); 1.4.4-reittiluettelossa on vain
6 yhdistämiskutsua 8 kirjautumista ja 2 kaverin lisäysyritystä kohden (L), joten sitä ei tehdä
jokaisella kirjautumisella. Oikea testi laskee ne (katso [Näin se tarkistetaan](#how-to-verify)).

## Yksi tunnus, kaksi vaihetta: tunnusketju {#the-identity-chain}

Jokainen merkintä jokaisessa sosiaalisessa listassa (kaverit, kaveripyynnöt, estetyt, ryhmän jäsenet,
Hunt Members, ryhmäkutsut, killan jäsenet, kiltakutsujen lähettäjät) on "sosiaalinen käyttäjä", jonka
avaimena on sen **Phoenix**-tilitunnus (B `0x1415af620`). Merkintä näkyy vasta, kun kaksi hakua on
onnistunut:

```
Epic-tunnus (kaverilista, estolista, Add Friends, /invite <nimi>, killan jäsenen lisäyskenttä)
   |  POST /account/mapping {"srcAccountType": "epic", "ids": [<Epic-tunnus>]}  -> Phoenix-tunnus
   v
Phoenix-tunnus (ryhmäkutsun lähettäjä, ryhmän jäsenet, Hunt Members, killan jäsenet ja kutsujat)
   |  POST /accountinfo/public {"accountId": <Phoenix-tunnus>}                  -> käyttäjätiedot
   |     vastauksen accountId:n on oltava kysytty tunnus (välimuistin avain, ensimmäinen voittaa)
   |     linkedAccounts [{accountType: "epic", accountId: X}] asettaa käyttäjän Epic-tunnukseksi X
   v
sosiaalinen työkalupakki tarkistaa, että käyttäjän Epic-tunnus on se, josta toiminto alkoi
   v
merkintä näytetään tai jonossa ollut toiminto suoritetaan (kaveripyyntö lähtee, kutsu näkyy ...)
```

Kun yhdistäminen ei tuota mitään, toiminto pudotetaan hiljaa (B, lokiteksti "Mapping primary Id for
unknown, unmapped external Id [%s] for user action"). Kun käyttäjätiedot eivät tuota mitään, käyttäjä
mitätöidään (B, "SocialToolkit - HandleUserInvalidated called for [%s]").

**Tällä palvelimella jokaisella tilillä on täsmälleen yksi tunnus**, `UID-<uuid>`. Se on yhtä aikaa
Epic-tilitunnus (kirjautuminen palauttaa sen kentässä `account_id`), Phoenix-tilitunnus (ryhmän ja
killan jäsenten tunnukset) ja chat-tunnus. Jokainen yhdistäminen on siis tunnus itse.
Alkuperäisessä palvelussa Epic-tunnus oli erillinen 32-merkkinen tunnus (R: versio 2.1.1 yhdisti
sellaisen kirjautuessaan), ja peliohjelma käy yhä läpi molemmat vaiheet, joten kummankin on
vastattava oikein, vaikka tunnukset ovat samat.

### `POST /account/mapping` {#post-account-mapping}

Peliohjelman `QueryAccountMappingsEndpoint` (K `dllmain.cpp`) Phoenixin omassa käyttäjäpalvelussa.

- **Pyyntö** (B, R, L): `{"srcAccountType": "epic", "ids": ["<tunnus>"]}`, JSONina pelaajan
  tunnisteella, enintään 100 tunnusta pyynnössä. Versio 2.1.1 lähetti juuri tämän kirjautuessaan (R),
  ja niin lähettivät oikeat 1.4.4-peliohjelmatkin (L), tosin ei jokaisella kirjautumisella (6 kutsua 8
  kirjautumisessa, niistä 2 kaverin lisäyksiä).
- **Vastaus, jonka jäsennin lukee** (B `0x140b09f60`..`0x140b0afd4`): juuriavain `accountMappings`,
  jossa on **olio, jonka avaimina ovat kysytyt tunnukset**. Jokainen arvo on olio, jossa on
  ei-tyhjät merkkijonot `accountId` ja `accountType`; `accountType` verrataan kirjainkoosta
  riippumatta arvoihin `epic` ja `phoenix`. Muuta merkinnästä ei lueta. HTTP-tilakoodia ei
  tarkisteta; tyhjä runko epäonnistuu ("Empty response payload"), samoin virheellinen JSON ("Invalid
  response payload"). Ylimääräiset juuriavaimet ohitetaan.
- **Vastauksemme**: `{"accountMappings": {"<tunnus>": {"accountId": "<tunnus>", "accountType": "phoenix"}},
  "code": "OK", "message": "", "payload": {"accountMappings": {...}}}`. Kääritty kopio `payload`-kentässä
  ei maksa mitään ja kattaa lukijan, joka odottaa Phoenixin kuorta. Kun `srcAccountType` on
  `phoenix`, merkinnöissä lukee `epic`. Tuntemattomat tunnukset jätetään pois; ilman kelvollista
  tunnistetta olio on tyhjä.
- **Miksi kaksi aiempaa vastausta epäonnistuivat**: ensimmäinen oli juuressa tunnuksilla avattu olio
  ilman `accountMappings`-avainta, joten jäsennin ei löytänyt mitään. Toinen lähetti `accountMappings`-
  kentän **taulukkona**, jonka jäsennin lukee tyhjäksi olioksi. Kummassakaan tapauksessa yhdistämistä ei
  tallennettu. Peliohjelman mallimme toistaa kummankin epäonnistumisen (katso
  [Testaus ilman peliä](#testing-without-the-game)).
- Varmuus **H** muodolle (jäsennin jäljitettiin alusta loppuun); toimivuutta pelissä ei ole vielä nähty.

### `POST /accountinfo/public` {#post-accountinfo-public}

Peliohjelman `PublicAccountInfoEndpoint`, yksi kutsu jokaista näytettävää pelaajaa kohden (73 kutsua
oikeasti, kaikkiin vastattiin, L). Tästä vaiheesta riippuu jokainen muu pelaaja Sosiaalinen-paneelissa.

- **Pyyntö** (B, R): `{"accountId": "<tunnus>"}`, tai toiselta pyynnön rakentajalta
  `{"displayname": "<nimi>"}`.
- **Vastaus** (B `0x140b69ac0`, tavallinen olio ilman kuorta, kuten muutkin tilipalvelut sivulla
  [Taustapalvelun rajapinta]({{ contract_page.url | relative_url }})):

  ```json
  {
    "accountId": "UID-B",
    "username": "Bravo",
    "linkedAccounts": [ { "accountId": "UID-B", "accountType": "epic" } ],
    "isSubscribed": true,
    "language": null
  }
  ```

- `accountId`:n on oltava **haettu** tili: se on avain, jonka alle peliohjelma tallentaa tiedot, ja
  ensimmäinen vastaus kutakin avainta kohden voittaa (B `0x140b74870`, varhainen paluu kohdassa
  `0x140b748f9`).
- `linkedAccounts`-kentän `epic`-merkinnästä tulee käyttäjän Epic-tunnus (B `0x140b7499f`), jota
  työkalupakki vertaa siihen Epic-tunnukseen, josta toiminto alkoi.
- Tuntematon tunnus tai nimi saa vastauksen **404 `{}`**: peliohjelma laskee epäonnistuneen haun eikä
  tallenna mitään (B, "Batch Succeeded, Total = %d, FailureCount = %d"). Vanhalla vastauksella se ei
  tallentanut mitään hyödyllistäkään.
- Peliohjelma ei lue kenttiä `isSubscribed` ja `language`; ne jätettiin, koska ne olivat oikeasti
  harmittomia.
- **Miksi alkuperäisen projektin vastaus epäonnistui**: se laittoi kysyjän oman tunnuksen kenttiin
  `accountId` ja `linkedAccounts`. Kirjautuessaan peliohjelma hakee ensin oman tunnuksensa, joten
  jokainen myöhempi toisen pelaajan haku osui kysyjän jo olemassa olevaan merkintään ja ohitettiin.
  Toinen pelaaja ei koskaan saanut käyttäjätietoja.
- Varmuus **H** muodolle ja välimuistin avaimelle; **M** sille, että tämä yksin saa kutsujen
  lähettäjät, Hunt Membersin ja kaverit näkyviin.

### Haut nimellä (ennallaan) {#lookups-by-name}

| Reitti | Käyttäjä | Varmuus |
|:-------|:---------|:--------|
| `GET /account/api/public/account/displayName/:nimi` → `{id, displayName, externalAuths: {}}` tai 404 | Add Friends, chatin `/invite <nimi>`, killan jäsenen lisäyskenttä (B, L: 2 kutsua) | M |
| `GET /account/api/public/account?accountId=A&accountId=B` → taulukko samoja | Sosiaalisen käyttäjän Epic-puolen nimi (B) | M |

Kirjoitettu nimi, jossa on `@`, menee sähköpostihakuun, johon ei vastata: käyttäjänimissämme ei voi
olla merkkiä `@`, ja 404 antaa tavallisen "ei löytynyt" -ilmoituksen.

## Kaverit {#friends}

Epicin kaveripalvelu (`[OnlineSubsystemMcp.OnlineFriendsMcp]`, B), jossa tunnisteena on
pelaajan tunnisteemme. Kaveruudet ja estot tallennetaan SQLiteen (rakennettu tiekartan kohtaa 1.9
varten). Reitit ovat sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#friends).

| Kutsu | Milloin | Pyyntö ja vastaus | Todiste | Varmuus |
|:------|:--------|:------------------|:--------|:--------|
| `GET /friends/api/public/friends/:id?includePending=true` | kerran kirjautuessa | Pelkkä **taulukko** `{accountId, status: ACCEPTED tai PENDING, direction: INBOUND tai OUTBOUND, created}` (peliohjelma käärii sen itse muotoon `{"friends": ...}`) | B, L: 8 kirjautumista, 8 kutsua | H |
| `GET /friends/api/public/blocklist/:id` | kerran kirjautuessa | `{"blockedUsers": [...]}` | B, L | H |
| `POST /friends/api/public/friends/:minä/:toinen` | Add Friends, Add Friend -valikkokohta, Accept | ei runkoa (Accept lähettää tyhjän JSON-rungon); mikä tahansa 2xx | B | H |
| `DELETE /friends/api/public/friends/:minä/:toinen` | Decline, Remove Friend | mikä tahansa 2xx | B | H |
| `POST` (ja nyt myös `PUT`) `/friends/api/public/blocklist/:minä/:toinen` | Block vahvistusikkunan jälkeen | mikä tahansa 2xx | B, metodi päätelty | M |
| `DELETE /friends/api/public/blocklist/:minä/:toinen` | Unblock | mikä tahansa 2xx | B | H |

Peliohjelma lukee kummankin listan **vain kirjautuessaan** (L). Kaveripyyntö tai hyväksyntä näkyy
siksi toiselle pelaajalle vasta hänen seuraavalla kirjautumisellaan. Kun kavereiden paikalla olo on
päällä (`CHAT=1` ja `CHAT_PRESENCE=1`), hyväksyntä lähetetään lisäksi heti chat-yhteyden kautta
([Tekstichat]({{ chat_page.url | relative_url }}#presence)); uutta pyyntöä ei lähetetä.

**Mitä pelaajan pitäisi nähdä** (M, ellei toisin merkitty):

| Vaihe | Kutsut | Tulos |
|:------|:-------|:------|
| Kirjautuminen | kaverilista, estolista; jokaiselle listan tunnukselle yhdistäminen ja käyttäjätiedot | Hyväksytyt kaverit kohdassa OFFLINE (ei koskaan kohdassa EPIC FRIENDS ilman paikalla oloa); jonkun lähettämä pyyntö painikkeineen Accept ja Decline; estetyt kohdassa BLOCKED; itse lähettämäsi pyynnöt eivät näy |
| Add Friends: kirjoita nimi, paina Add | nimihaku (404: ilmoitus tuntemattomasta pelaajasta; oma nimi: ilmoitus, ettet voi kutsua itseäsi), yhdistäminen, käyttäjätiedot, `POST .../friends/<minä>/<toinen>` | ilmoitus "friend invite sent"; listoihin ei tule mitään uutta |
| Toinen pelaaja kirjautuu uudelleen | kirjautumisen kutsut | pyyntö painikkeineen Accept ja Decline (missä kohdassa tarkalleen, jää nähtäväksi) |
| Accept | `POST .../friends/<minä>/<toinen>` | kaveri näkyy kohdassa OFFLINE seuraavan kirjautumisen jälkeen |
| Decline, Remove Friend | `DELETE .../friends/<minä>/<toinen>` | merkintä katoaa (H) |
| Block, Unblock | `POST`/`DELETE .../blocklist/<minä>/<toinen>` | merkintä siirtyy kohtaan BLOCKED tai sieltä pois |

**Lisäämämme rajat** (G): enintään 50 vastaamatonta lähetettyä pyyntöä tiliä kohden ja enintään 20
uutta pyyntöä 10 minuutissa (409, jonka peliohjelma näyttää epäonnistumisena). Pyynnön hyväksymistä ei
koskaan rajoiteta. Vanhat rajat pysyvät: 200 kaveruutta ja 200 estoa tiliä kohden. Esto poistaa myös
kahden pelaajan väliset odottavat ryhmä- ja kiltakutsut (G), ja kutsulistat jättävät pois jokaisen
kutsun toisensa estäneiden pelaajien välillä.

## Ryhmät {#parties}

Ryhmäpalvelu oli jo rakennettu (tiekartan kohta 1.9), ja se vastasi peliohjelman pyyntöjä ja
vastauksia. Oikean palvelimen reittiluettelo näyttää sen toimineen: 399 ryhmäkyselyä ja 399
kutsukyselyä, ja kutsu päätyi toisen pelaajan kyselyyn (L). **Ainoa palvelinmuutos, jota ryhmät
tarvitsivat, on `/accountinfo/public`-korjaus**; 23.9.2026 alkaen myös
[kahdesti saapuva hyväksyntä](#a-repeated-accept) sallitaan.

| Kutsu | Milloin | Todiste | Varmuus |
|:------|:--------|:--------|:--------|
| `POST /party` `{buildId, featureOverrides: []}` | kysely noin 10 sekunnin välein ja kirjautuessa | B `0x140b57600`, L: 399 kutsua | H |
| `GET /party/invites` → `{"invitations": [{recipientPlayerId, sendingPlayerId, partyId, sendingPlatform, sendingDisplayName}]}` | kutsukysely | B `0x140b675d0`, L: 399 kutsua | H |
| `PUT /party/invite` `{recipientPlayerId, partyId, buildId, featureOverrides}` | Invite to Party; chatin `/invite <nimi>` | B | H |
| `PUT /party/invite/accept/:partyId` | Accept; tunnus on ryhmän, ei kutsun tunnus (B `0x140b35384`) | B | H |
| `DELETE /party/invite` | Decline | B | H |
| `DELETE /party/member`, `DELETE /party/member/:id`, `PUT /party/member/promote/:id` | lähteminen (myös jokaisella kirjautumisella), poistaminen, johtajaksi nostaminen | B, L: 12 lähtöä 8 kirjautumisessa | H |
| `POST /candidate/join` johtajalta | johtaja valitsee metsästyksen; jäsenet seuraavat ryhmäkyselynsä kautta | C, testit | M |

**Mitä pelaajan pitäisi nähdä**, kun korjaus on palvelimella: kutsu näkyy kohdassa PARTY INVITES
noin 10 sekunnin kuluessa (seuraava kutsukysely), kun peliohjelma on hakenut lähettäjän tiedot.
Hyväksymisen jälkeen kummankin ryhmäpaneelissa näkyvät molemmat nimet. Kun johtaja valitsee
metsästyksen, koko ryhmä päätyy samalle metsästyspalvelimelle. Ryhmät ovat muistissa: metagamen
uudelleenkäynnistys jättää jokaisen yhden hengen ryhmään, minkä peliohjelma hyväksyy normaalina.

Kaksi asiaa jätimme tarkoituksella ennalleen: pelaajan itse **lähettämiä** kutsuja ei listata
reitillä `GET /party/invites` (peliohjelma voisi luulla niitä saapuneiksi), ja yhden hengen ryhmä pitää
vanhat paikkamerkkiarvot, jotka olivat oikeasti harmittomia metsästyksiin jonottamiselle.

**Yksi riski pelin omalle kutsulle: paikkamerkki voi näyttää jonottamiselta.** Ennen kuin peliohjelma
lähettää pyynnön `PUT /party/invite`, se torjuu kutsun, jos kutsuja on itse kutsuttu, ei ole johtaja
tai jos ryhmä ei ole joutilas (B `0x1415b2280`; viimeinen tarkistus kutsuu kohdassa `0x1415b27aa`
funktiota `0x1415a98c0` ja kirjaa lokiin "Player %s tried to send an invite to player %s, but party %s
was matchmaking"). Tila luetaan ryhmän ehdokkaasta, ja paikkamerkissä lukee `QUEUED_FOR_START`
ehdokastunnuksen kera. Yksin pelaavat jonottivat metsästyksiin sen kanssa ongelmitta (L: 27
jonoonliittymistä), eikä tilan lukemisen yhtä vaihetta jäljitetty, joten on avoinna, harmaantuuko
Invite to Party sen takia (M). Kukaan ei käyttänyt pelin omaa kutsua oikeasti (L: 0 pyyntöä
`PUT /party/invite`), joten mikään ei myöskään kumoa riskiä. Jos se toteutuu, `PARTY_SOLO_STUB=0`
vastaa yhden hengen ryhmälle ilman ehdokasta (`candidateState: null`, joka luetaan joutilaaksi); katso
[Asetukset]({{ config_page.url | relative_url }}#metagame-social).

**Lisäämämme rajat** (G): pelaaja lähettää enintään 20 kutsua 10 minuutissa, ja kun pelaaja hylkää
jonkun kutsun, tämä lähettäjä ei voi kutsua häntä uudelleen 2 minuuttiin (kumpikin 409, epäonnistuminen
peliohjelmalle). Esto poistaa kahden pelaajan väliset odottavat kutsut.

### Kahdesti saapuva hyväksyntä {#a-repeated-accept}

Hyväksyntä voi saapua kahdesti: peliohjelma yrittää uudelleen pyyntöä, jonka vastaus katosi, ja
Harmonicin haara salli sen (G sille, kuinka usein näin käy versiossa 1.4.4). 23.9.2026 alkaen pyyntö
`PUT /party/invite/accept/<tunnus>`, joka ei vastaa mitään voimassa olevaa kutsua, saa silti vastauksen
**200 ja kutsujan ryhmän** (saman rungon kuin ryhmäkysely), kun kutsuja on jo vähintään kahden hengen
ryhmässä ja tunnus on tuon ryhmän tunnus tai sen toisen jäsenen tunnus (esimerkiksi kutsun lähettäjän).
Lokirivi on `party: accept by <A> id=<tunnus>: already in P=<ryhmä> size=<n>; answering that party (a
repeated accept)`. Yhden hengen ryhmä, ryhmä josta kutsuja on lähtenyt, kutsujan oma tunnus ja
vieraan tunnus saavat yhä vastauksen 404 kuten ennenkin. Kytkintä ei ole: muutos vastaa vain siellä,
missä vastaus oli ennen 404.

### Automaattinen potku {#the-automatic-kick}

Peliohjelman automaattinen "offline"-jäsenten poisto ryhmästä ei koskaan käynnisty ilman paikalla olotietoa (B `0x1415f6f60`, 10 sekunnin
raja). Chat-palvelin pitää sen niin: oletuksena se ei lähetä läsnäolotietoja chat-huoneiden
ulkopuolella lainkaan, ja kun kavereiden paikalla olo on päällä (`CHAT_PRESENCE=1`), se ei silloinkaan
koskaan lähetä pelaajalle läsnäolotietoa hänen omalta tililtään, ei edes hänen toisesta
istunnostaan ([Tekstichat]({{ chat_page.url | relative_url }}#party-safety)).

## Killat {#guilds}

### Sopimus {#guild-contract}

- **Päätepisteet** (K `dllmain.cpp`, paketoitu `DefaultGame.ini`): yksitoista `*_v2`-avainta.
  Vanhemmat `v1`-avaimet on käännetty mukaan, mutta niitä ei koskaan kutsuta, joten rakensimme vain
  version 2.
- **Jokainen vastaus** luetaan Phoenixin kuoren `{"code": merkkijono, "message": merkkijono,
  "payload": olio}` kautta (B `0x140b12170`). Onnistuminen vaatii 2xx-tilakoodin **ja** jäsentyvän
  JSON-rungon (B `0x140aae300`), joten 204 on epäonnistuminen. `GET /guild` -reitin 204 tarkoittaa
  "ei kiltaa" juuri tämän vuoksi: peliohjelma tyhjentää silloin kiltansa (B `0x1415c2c90`; L: 34
  kutsua, joihin vastattiin 204).
- **Virhetilakoodilla** peliohjelma lukee silti `code`-kentän ja muuttaa sen kiltavirheekseen (B
  `0x140ade3f0`, K `EGuildRequestError`). Tuntematon tai tyhjä koodi näkyy tekstinä "Unable to create
  guild."
- **Rungon kentät** ovat merkkijonoja, paitsi `maximum_guild_members` (kokonaisluku). Arvot ovat
  `Member`, `Officer` ja `Leader`, ja ne verrataan kirjainkoosta riippumatta; peliohjelma laittaa arvon
  URL:iin pienaakkosin.
- **Kilta perustetaan pelipalvelimen kautta.** Create-painike lähettää etäkutsun `ServerCreateGuild`
  Ramsgaten pelipalvelimelle (B, K), joka lähettää `POST /guild` -pyynnön avaimellaan (L: yksi
  tällainen kutsu, johon vastattiin 404). Etäkutsun tarkistus palauttaa toden tarkistamatta mitään
  esinettä, joten **killan perustaminen ei maksa mitään** versiossa 1.4.4.
- Versiossa 2 ei ole päivän viestiä, lippua, kiltasalia, etuja eikä killan kokemuspisteitä. Kiltachat on
  XMPP-huone `Guild-<guildId>` (rakennettu, vain killan jäsenille: [Tekstichat]({{ chat_page.url | relative_url }}#rooms)). `[TAG]` pelaajien päiden yllä toimii itsestään, kun
  `GET /guild` palauttaa nimikyltin.

Lähetämme jokaisen vastauksen käärittynä ja kopioimme lisäksi rungon kentät juureen (G). Kuoren lukija
ohittaa ylimääräiset juuriavaimet, joten kopiot eivät maksa mitään ja kattavat litteän lukijan siltä
varalta, että luimme koodia väärin.

### Virhekoodit {#guild-error-codes}

Palvelin lähettää koodin (`code`); peliohjelma näyttää oman tekstinsä (killan perustamisikkunasta).

| `code` | Peliohjelman virhe | Lähettämämme tilakoodi | Teksti, jonka peliohjelma näyttää |
|:-------|:-------------------|:-----------------------|:----------------------------------|
| `SlyAdorableQuillshot` | InvalidPermission | 403 | |
| `ExcludedAdorableQuillshot` | NotInAGuild | 404 | |
| `ObedientAdorableQuillshot` | GuildNameInvalidLength | 400 | Name is invalid. Must contain 4-15 english letters and digits. |
| `SeizedAdorableQuillshot` | GuildNameTaken | 409 | Guild name already in use. |
| `NastyAdorableQuillshot` | GuildNameProfane | 400 | Guild name contains profanity. |
| `NumberedAdorableQuillshot` | GuildNameTooManyNumbers | 400 | Guild name must have 6 numbers or less. |
| `LetteredAdorableQuillshot` | GuildNameTooManyLetters | 400 | Guild name must not have more than 6 of the same letter in a row. |
| `DutifulAdorableQuillshot` | GuildNameplateInvalidLength | 400 | Nameplate is invalid. Must contain 2-6 english letters and digits. |
| `CapturedAdorableQuillshot` | GuildNameplateTaken | 409 | Guild nameplate already in use. |
| `DirtyAdorableQuillshot` | GuildNameplateProfane | 400 | Guild nameplate contains profanity. |
| `OccupiedAdorableQuillshot` | YouAlreadyInAGuild | 409 | Unable to create guild. You are already in a guild. |
| `ClonedAdorableQuillshot` | TargetAlreadyInYourGuild | 409 | |
| `RedundantAdorableQuillshot` | TargetAlreadyHasGuildInvite | 409 | |
| `StuffedAdorableQuillshot` | GuildIsFull | 409 | |
| `UninvitedAdorableQuillshot` | GuildInviteNotFound | 404 | |
| `ChiefAdorableQuillshot` | GuildLeaderCannotLeaveGuild | 409 | |
| `DocileAdorableQuillshot` | InvalidGuildRank | 400 | |
| `""` (tyhjä) | Unknown | tilanteen mukaan | Unable to create guild. |

Seitsemäntoista koodia ovat ohjelmatiedostossa peräkkäin (B, tiedostokohdat `0x447b950`–`0x447bca0`)
samassa järjestyksessä kuin SDK:n virheluettelo (K). Pariutus tulee koodin muuntajasta (B, H).
Nimisäännöt tulevat yllä olevista teksteistä ja peliohjelman omista pituusrajoista (nimille 4 ja 15,
kylteille 2 ja 6, B). Ohjelmatiedostossa on myös vanhempia tekstejä eri rajoin ("4-32" ja "2-7"); ne
kuuluvat käyttämättömään versioon 1.

### Nimen ja nimikyltin säännöt {#name-rules}

Tarkistetaan tässä järjestyksessä; ensimmäinen hylkäävä sääntö ratkaisee.

1. Pyytäjä (perustettaessa johtaja) ei ole jo killassa: `Occupied`.
2. Nimi: 4–15 englannin kirjainta ja numeroa, ei muuta (välilyönti ei kelpaa): `Obedient`.
3. Nimi: enintään 6 numeroa: `Numbered`.
4. Nimi: sama kirjain enintään 6 kertaa peräkkäin kirjainkoosta riippumatta: `Lettered`.
5. Nimi: ei kieltolistan sanaa (G): `Nasty`.
6. Nimi: ei varattu kirjainkoosta riippumatta (G): `Seized`.
7. Nimikyltti: tyhjä tai 2–6 englannin kirjainta ja numeroa: `Dutiful`. Tyhjä kyltti kelpaa
   (peliohjelma ohittaa oman tarkistuksensa, kun kyltti on tyhjä, B), ja usealla killalla voi olla
   tyhjä kyltti.
8. Nimikyltti: ei kieltolistan sanaa: `Dirty`.
9. Nimikyltti: ei varattu kirjainkoosta riippumatta: `Captured`.

Peliohjelma ei koskaan kutsu kirosanapalvelua (paketoitu asetus kytkee sen pois), joten palvelimella
on lyhyt sisäänrakennettu kieltolista; `GUILD_NAME_DENYLIST` lisää sanoja (katso
[Asetukset]({{ config_page.url | relative_url }})). Muutama lyhyt loukkaava sana torjutaan koko nimenä
tai nimikylttinä samoilla koodeilla. **Varatut sanat** (G) estävät kiltaa esiintymästä palvelimen
henkilökuntana tai projektina: `admin`, `moderator`, `official`, `staff` ja muutama muu missä tahansa
sekä nimikyltit kuten `GM`, `DEV` ja `MOD`. Ne vastaavat "already in use" (`Seized` tai `Captured`);
`GUILD_RESERVED_NAMES=0` sallii ne. Täydet listat ovat sivulla
[HTTP-rajapinta]({{ api_page.url | relative_url }}#guilds).

### Reitit {#guild-routes}

| Reitti (päätepisteavain) | Kutsuja | Mitä se tekee | Varmuus |
|:-------------------------|:--------|:--------------|:--------|
| `GET /guild` (`GuildEndpoint_v2`) | peliohjelma kirjautuessa, jokaisella maailman latauksella ja jokaisen kiltatoiminnon jälkeen (L: 34 kutsua 8 kirjautumisessa) | pyytäjän kilta tai 204 | H |
| `GET /guild/invite/player` (`GuildViewInvitesEndpoint_v2`) | samoina hetkinä | pyytäjän avoimet kutsut: `{id, guild_id, guild_name, inviter_account_id}` | H |
| `POST /guild/validate` (`GuildCreateValidateEndpoint_v2`) | peliohjelma, kun pelaaja kirjoittaa CREATE A GUILD -ikkunaan (L: 2 kutsua) | `{leader_account_id, name, nameplate}`: tarkistaa säännöt | H |
| `POST /guild` (`GuildEndpoint_v2`) | **pelipalvelin** etäkutsun `ServerCreateGuild` jälkeen (L: 1 kutsu) | perustaminen; vastaa uudella killalla, jonka peliohjelma ottaa heti käyttöön | H |
| `DELETE /guild/:guildId` (`GuildDisbandEndpoint_v2`) | DISBAND GUILD, vain johtaja | lakkauttaa killan | H |
| `PUT /guild/invite/:accountId` (`GuildInviteEndpoint_v2`) | jäsenen lisäyskenttä, minkä tahansa pelaajan valikon "Invite to Guild" | kutsuu; johtaja tai upseeri | H |
| `POST /guild/invite/accept/:guild_invite_id` | Accept Guild Invite | liittyy jäseneksi | H |
| `DELETE /guild/invite/:guild_invite_id` | Decline Guild Invite | poistaa kutsun | H |
| `DELETE /guild/player` (`GuildLeaveEndpoint_v2`) | Leave Guild | lähtee; ei johtaja | H |
| `DELETE /guild/player/:accountId` (`GuildKickEndpoint_v2`) | Kick From Guild, vain johtaja | poistaa jäsenen | H |
| `PUT /guild/rank/:accountId/:rank` (`GuildChangeRankEndpoint_v2`) | Promote To Guild Officer (`officer`), Demote To Guild Member (`member`), Promote To Guild Leader (`leader`) | muuttaa arvon; `leader` luovuttaa killan | H (reitti), M (entisestä johtajasta tulee upseeri on oma valintamme) |

Vastausten kiltaolio on `{id, name, nameplate, leader_account_id, members: [{phx_account_id, rank}],
maximum_guild_members}` (B `0x140b13de0`, K `FGuildData`). Kunkin reitin tarkat tarkistukset ja koodit
ovat sivulla [HTTP-rajapinta]({{ api_page.url | relative_url }}#guilds).

### Killan perustaminen pelipalvelimen kautta {#creating-through-the-game-server}

Create-painike lähettää etäkutsun `ServerCreateGuild(LeaderPlayerId, name, nameplate)` Ramsgaten
pelipalvelimelle (K `Archon_parameters.hpp`), jonka tarkistus palauttaa aina toden, ja pelipalvelin
lähettää pyynnön `POST /guild`, jossa `leader_account_id` on tämä tunnus. **Pelipalvelin ei lähetä
pelaajan tunnistetta.** `CreateGuild` (B `0x140ac7270`) ottaa tunnisteensa kohdassa `0x140ac78a4`
funktiolta `0x140b461d0`, joka pyytää alijärjestelmän tunnistusrajapinnalta alijärjestelmän **oman**
paikallisen käyttäjän (`Subsystem+0x2c0`) tunnisteen; pyyntö saa `Authorization`-otsakkeen vain, jos
tunniste ei ole tyhjä (`0x140b3b561`). Pelipalvelimet eivät koskaan kirjaudu Phoenixiin (L: jokainen
`POST /login` tuli peliohjelmalta), joten perustamispyynnössä ei tavallisesti ole tunnistetta, ja jos
olisi, se olisi pelipalvelimen oma, sama jokaiselle sen pelaajalle (H). Johtajatunnus on siis vain
jonkin peliohjelman väite, ja metagame sitoo perustamisen johtajan omaan toimintaan (G):

- `POST /guild` hyväksyy vain pelipalvelinavaimen ja vain tältä koneelta. Pelaajan tunniste yksinään
  torjutaan.
- Mukana tuleva tunniste vain kirjataan lokiin ("the game server's token names ...", ja
  perustamisrivi päättyy "a token of X came along" tai "no token"); kelvoton tunniste ohitetaan eikä
  kaada pyyntöä.
- **Johtajan on pitänyt tarkistaa juuri tämä nimi ja nimikyltti** omalla tunnisteellaan
  (`POST /guild/validate`, jonka perustamisikkuna lähettää pelaajan kirjoittaessa) viimeisten 15
  minuutin aikana. Pelaajan viisi viimeksi tarkistettua paria kelpaavat kirjainkoosta riippumatta
  siltä varalta, että Create painetaan ennen kuin viimeinen tarkistus on palannut. Muuten perustaminen
  torjutaan tyhjällä koodilla, jonka peliohjelma näyttää tekstinä "Unable to create guild." (vastauksen
  viesti ei koskaan päädy ruudulle: virhetilakoodille peliohjelma muodostaa viestinsä HTTP-tilasta, B
  `0x140aae447`), ja kirjataan lokiin tekstillä "no validate of this name and nameplate by the leader
  in the last 15 minutes". Nimi, jonka säännöt torjuvat joka tapauksessa, saa sen säännön oman
  tekstin. Muokattu peliohjelma ei siis voi tehdä toisesta pelaajasta sellaisen killan johtajaa, jota
  tämä ei koskaan nimennyt, eikä paikalla oleminen riitä.
- `GUILD_CREATE_ACTIVITY_FALLBACK=1` hyväksyy myös johtajan, joka tarkisti jonkin toisen nimen tai
  näkyi palvelimelle viimeisen minuutin aikana, ja kirjaa lokiin varoituksen. Se on olemassa vain
  siltä varalta, että oikea testi näyttää, ettei peliohjelma koskaan tarkista lopullista nimeä.
- Enintään yksi uusi kilta johtajaa kohden 10 minuutissa, ja ylläpitäjä voi lakkauttaa minkä tahansa
  killan. Onnistunut perustaminen kuluttaa johtajan tarkistamat nimet.

### Tallennus, rajat ja oikeudet {#guild-storage-limits-permissions}

Killat, jäsenet ja kutsut tallennetaan SQLiteen (siirto `0013_guilds`, kolme uutta taulua; katso
[Tiedostot ja data]({{ files_page.url | relative_url }})), joten kilta säilyy uudelleenkäynnistysten
yli ja kutsu odottaa pelaajaa, joka ei ole paikalla.

| Raja | Arvo |
|:-----|:-----|
| Jäseniä kiltaa kohden | `GUILD_MAX_MEMBERS`, oletus 100 (peliohjelma ottaa minkä tahansa lähettämämme luvun) |
| Kutsun voimassaolo | `GUILD_INVITE_TTL_DAYS`, oletus 7 päivää |
| Avoimia kutsuja kiltaa kohden | 50 |
| Lähetettyjä kutsuja kutsujaa kohden | 30 tunnissa |
| Avoimia kutsuja pelaajaa kohden | 20; vanhin poistetaan (yhdeltä killalta niistä voi olla vain yksi) |
| Hylänneen pelaajan kutsuminen uudelleen | sama kilta odottaa 24 tuntia |
| Perustettuja kiltoja johtajaa kohden | 1 kymmenessä minuutissa |

| Toiminto | Kuka |
|:---------|:-----|
| Oman killan ja omien kutsujen lukeminen | sinä |
| Kutsuminen | johtaja, upseeri |
| Hyväksyminen, hylkääminen | kutsuttu pelaaja |
| Lähteminen | jäsen, upseeri |
| Erottaminen, arvojen muuttaminen, lakkauttaminen | johtaja |

Esto kumpaan tahansa suuntaan torjuu kiltakutsun ja poistaa kahden pelaajan väliset avoimet kutsut.
Upseerin kutsut poistetaan, kun hänet alennetaan jäseneksi, erotetaan tai hän lähtee; lista jättää
pois, ja hyväksyminen torjuu (`Uninvited`), jokaisen kutsun, jonka kutsuja ei ole enää killan johtaja
tai upseeri. Toisen killan jäsenen voi kutsua, mutta hänen on lähdettävä killastaan ennen
hyväksymistä (peliohjelma kertoo sen itse, B).

**Mitään ei lähetetä itsestään.** Muut jäsenet ja kutsutut näkevät muutoksen seuraavassa
`GET /guild` -kyselyssään (kirjautuminen, maailman lataus tai oma kiltatoiminto). Paneelit eivät
päivitä itseään (B).

## Slayer Links {#slayer-links}

Slayer Linksin (peliohjelmassa nimellä Linked Slayers; Social-paneelin My Links -välilehti, K
`USocialPanelTabLinkedSlayers`) avulla kaksi kaveria voi liittoutua viikoksi. Jokaisella pelaajalla on
kolme linkkipaikkaa. Pelaaja kutsuu kaverin johonkin paikkaan, kaveri hyväksyy kutsun johonkin omaan
paikkaansa, ja linkki kestää 168 tuntia.

**Tilanne 23.9.2026: rakennettu ja testattu ilman peliä, oletuksena päällä (`SLAYER_LINKS`), ei vielä
kokeiltu pelissä.** Rajapinnan kuvaus on Harmonicin haarasta, jossa oli ensimmäinen toimiva versio;
tarkistimme jokaisen reitin ja rungon ohjelmatiedostoa vasten ja korjasimme viisi kohtaa (alla).
22.9.2026 reittiluettelossa ei näkynyt yhtään Slayer Link -kutsua (L), joten peliohjelma saattaa pitää
välilehden piilossa (`ULinkedSlayersFeature`); jos se ei koskaan kutsu näitä reittejä, pelaajille ei
muutu mitään.

### Reitit {#slayer-link-routes}

Jokainen vastaus käyttää Phoenixin kuorta `{"code": null, "message": "OK", "payload": ...}`. Jokainen
reitti toimii tunnisteen (bearer token) tilin puolesta; rungon tai polun tunnukset vain nimeävät toisen
pelaajan.

| Reitti (päätepiste) | Pyyntö | Vastauksen sisältö (payload) | Todiste |
|:--------------------|:-------|:-----------------------------|:--------|
| `GET /slayerlink/status_good` (`LinkedSlayersStatusEndpoint`) | ei runkoa | `{invites: [...], links: [...], config: {link_duration_hours: 168, invite_expiry_hours: 24}}` | B: `FOnlineLinkedSlayer::GetStatusUpdate` `0x1415e5990` ja sen käsittelijä `0x1415e8b40` lukevat olion, jossa on avaimet `invites`, `links` ja `config` (jäsennin `0x141600510`); config lukee kaksi tuntimäärää (`0x1415ffc90`). Se, että tämä olio on kuoren sisältö: S. Peliohjelma kyselee uutisia säännöllisesti (S: että kysely on tämä reitti). |
| `GET /slayerlink/invites` (`LinkedSlayersAllInvitesEndpoint`) | ei runkoa | `{invites: [{account_id, slot, direction, status, expires, link_id}]}`: odottavat, voimassa olevat kutsut; `account_id` on toinen pelaaja, `direction` on `Sent` tai `Received`, `status` on `Pending`, `expires` ISO-päivämäärä | B `0x1415e7d00`, merkinnät `0x1415ff170`; luetteloiden kirjoitusasut SDK:sta (S) |
| `GET /slayerlink/links` (`LinkedSlayersAllLinkSlotsDataEndpoint`) | ei runkoa | `{links: [{account_id, linked_account_id, slot, ends, link_id, prize_pool: []}]}`: käynnissä olevat linkit kutsujan paikan mukaan | B `0x1415e8690`, merkinnät `0x141600170` lukevat `account_id`, `slot`, `ends`, `prize_pool`; tilavastauksen linkit lukevat `linked_account_id`, `slot`, `ends`, `link_id`, `prize_pool`. Jokaisessa merkinnässä on kummatkin. |
| `PUT /slayerlink/invite` (`LinkedSlayersInviteEndpoint`) | `{account_id, slot, action_source}` | `{link_id}`: kutsun tunnus; saman pelaajan kutsuminen uudelleen antaa saman tunnuksen | B `0x1415fcba2`, runko `0x1415ff0e0` |
| `POST /slayerlink/invite` (`LinkedSlayersInviteAcceptDeclineEndpoint`) | `{account_id, action, slot, action_source}`, `action` = `accept`, `reject` tai `cancel` | `{link_id}` | B: accept `0x1415d4aa6`, cancel `0x1415d96cf`, reject `0x1415dadd6`, runko `0x1415fdc40` |
| `DELETE /slayerlink/invites/:account_id` (`LinkedSlayersDeleteAllInvitesEndpoint`) | ei runkoa | `{}` | B `0x1415db890`; kenen tunnuksen peliohjelma laittaa polkuun, jäi jäljittämättä (G), joten molemmat merkitykset käsitellään |
| `DELETE /slayerlink/links` (sama päätepiste kuin linkkilista) | `{account_id, slot, delete_pair}` tai samat kyselyparametreina | `{}` | B: `FOnlineLinkedSlayer::DeleteLinks` `0x1415dc442`, runko `0x141600eb0` |
| `POST /slayerlink/availability` (`LinkedSlayersGetFriendsAvailabilityEndpoint`) | `{account_ids: [...]}`, enintään 50 | `{availability: [{account_id, available}]}` | B `0x1415e3608`, `0x1415fdf30`, `0x1415fdf90` |

Virheet ovat muotoa `{"code": "<tila>", "message": ..., "payload": null}`: 400 (ei tilitunnusta, paikka
muu kuin 1–3, tuntematon toiminto), 403 (ei kavereita tai esto), 404 (tiliä tai kutsua ei ole), 409
(oma itse, paikassa on linkki tai odottava kutsu, jo linkitetty, toinen pelaaja on jo kutsunut sinut,
ei vapaata paikkaa, yli 20 uutta kutsua 10 minuutissa, kutsu on vanhentunut tai siihen on jo vastattu).
Ilman tunnistetta vastaus on 401;
pelkkä pelipalvelimen avain saa 403.

**Ei vastata:** palkintoreitit `PUT /slayerlink/links/rewards` ja
`GET /slayerlink/links/rewards/:account_id/:slot` pysyvät 404:nä, kunnes ne on jäljitetty, koska
väärä onnistumisvastaus voisi hukata palkinnon. Avaimiin `LinkedSlayersInviteCancelEndpoint`,
`...AllLinksProgressEndpoint`, `...AddLinkProgressEndpoint`, `...DeleteInviteDataEndpoint`
(`/slayerlink/link`), `...SetEndTimeEndpoint` ja `...SetRemainingTimeEndpoint` ei viitata missään
ohjelmatiedostossa (B): peliohjelma ei koskaan lähetä niitä.

### Säännöt {#slayer-link-rules}

- **Kuka.** Kummankin pelaajan on oltava hyväksyttyjä kavereita, eikä kumpikaan saa olla estänyt toista.
- **Paikat.** Kolme pelaajaa kohden, numeroituina 1–3 niin kuin peliohjelma ne numeroi, ja yksi
  odottava kutsu paikkaa kohden. Peliohjelma täyttää paikkataulunsa avaimilla ykkösestä arvoon
  `MaxLinkSlotsCount` asti, joka on 3 (B: silmukka `0x1415ea22d`–`0x1415ea3a5`, määrä asetetaan
  kohdassa `0x1415ce5f6`), ja hakee jokaisen paikan tällä avaimella; paikasta, jota taulussa ei ole, se
  kirjaa "Invalid Slot Id" (B `0x1415ef4a0`). Paikka 0 torjutaan siksi (400) eikä sitä koskaan
  tallenneta. Hyväksyntä käyttää rungon paikkaa, jos se on vapaa, muuten ensimmäistä vapaata.
- **Kutsuraja.** Pelaaja lähettää enintään 20 uutta kutsua 10 minuutissa (sen jälkeen 409, sama raja
  kuin kaveripyynnöillä). Raja lasketaan tallennetuista kutsuista, joten uudelleenkäynnistys ei nollaa
  sitä. Saman pelaajan kutsuminen uudelleen vastaa odottavalla kutsulla eikä lasketa.
- **Vastaukset.** Hyväksyntä ja hylkäys kuuluvat kutsutulle (`account_id` = lähettäjä), peruutus
  lähettäjälle (`account_id` = kutsuttu). Jos peliohjelma joskus lähettää rungossa kentän `link_id` tai
  `invite_id`, sitä kokeillaan ensin. Samanlaisen vastauksen toistaminen antaa taas 200.
- **Ajat.** Kutsu on voimassa 24 tuntia ja linkki 168 tuntia; vanhentunut kutsu muuttuu tilaan
  `EXPIRED`, kun sitä seuraavan kerran katsotaan.
- **Kutsujen tyhjennys.** `DELETE /slayerlink/invites/<oma tunnus>` peruu kaikki kutsujan lähettämät
  kutsut ja hylkää kaikki hänen saamansa; toisen pelaajan tunnuksella se tekee saman vain näiden
  kahden välisille kutsuille.
- **Linkin poisto** päättää kutsujan linkin kyseisessä paikassa tai kyseisen pelaajan kanssa
  kummaltakin pelaajalta (pari on yksi rivi; `delete_pair` vain kirjataan lokiin). Jos poistettavaa ei
  ole, vastaus on silti 200.
- **Kaveruuden purku tai esto** peruu kahden pelaajan väliset odottavat kutsut samassa
  tietokantatapahtumassa. Käynnissä oleva linkki jatkuu loppuunsa: päättyykö se myös kaveruuden
  purkuun, on ylläpitäjän päätös.
- Hylätyt, perutut ja vanhentuneet kutsut poistetaan, kun niiden 24 tuntia on kulunut, hyväksytyt
  kutsut ja päättyneet linkit 30 päivän kuluttua (kumpikin seuraavan uuden kutsun yhteydessä). Kutsut ja
  linkit tallennetaan SQLiteen (tietokantamuutos `0016_slayer_links`, taulut `slayerlinkinvites` ja
  `slayerlinks`; katso [Tiedostot ja data]({{ files_page.url | relative_url }})).

### Mitä korjasimme Harmonicin versiosta {#slayer-link-corrections}

| Hänen haaransa | Peliohjelma (B) |
|:---------------|:----------------|
| Kutsulista nimeää toisen pelaajan kentällä `linked_account_id` | Se lukee kentän `account_id` (`0x1415ff170`) |
| Linkki poistetaan osoitteessa `/slayerlink/link` | Tätä avainta ei käytetä; peliohjelma lähettää `DELETE /slayerlink/links` rungon kanssa (`0x1415dc442`) |
| Reittiä `DELETE /slayerlink/invites/<tunnus>` ei ollut | Peliohjelma lähettää sen (`0x1415db890`) |
| Tilavastaus oli litteä | Se sisältää kentät `invites`, `links` ja `config` (`0x141600510`) |
| Paikat numeroidaan 0–2 | Paikkataulun avaimet ovat 1–3 (`0x1415ea22d`), ja jokainen haku käyttää niitä |

**Lokit.** Jokainen toiminto on yksi `slayerlink:`-rivi, esimerkiksi
`slayerlink: invite by=<A> to=<B> slot=1 -> sent id=<tunnus>` tai
`slayerlink: accept by=<B> other=<A> id=<tunnus> -> accepted (slots 1 and 2)`; torjunta päättyy
`refused <tila>: <syy>`. `SLAYER_LINKS=0` palauttaa jokaiselle reitille vastauksen 404, jonka ne saivat
ennen; tallennetut kutsut ja linkit säilyvät. Reitit ovat myös sivulla
[HTTP-rajapinta]({{ api_page.url | relative_url }}#slayer-links).

## Istunnon tarkistus (`oauth/verify`) {#verify}

Käynnissä ollessaan peliohjelma tarkistaa istuntonsa säännöllisesti pyynnöllä
`GET /account/api/oauth/verify` (noin 30 sekunnin välein, G tarkalle välille). Alkuperäinen projekti
vastasi jokaiselle samalla paikkamerkkitilillä, jota peliohjelman käsittelijä ei pysty yhdistämään
käyttäjäänsä: ohjelmatiedosto kirjaa siitä rivin "Verify auth response ignored, can't find
UserAccount for %s" (B `0x140965ab8`, käsittelijässä `0x140964040`).

23.9.2026 alkaen vastaus nimeää sen tunnisteen tilin, jonka kanssa pyyntö tuli, kuten oikea palvelu
teki:

- kelvollinen pelaajan tunniste: `account_id` on pelaajan oma tili;
- puuttuva, virheellinen, vanhentunut tai vieras tunniste: vanha paikkamerkkivastaus, edelleen
  vastauksella **200**. Se ei koskaan vastaa 401: tunnisteen 24 tunnin jälkeen 401 voisi kirjata
  pelaajan ulos (G). Huono tai vanhentunut tunniste kirjataan lokiin enintään kerran minuutissa ("with a
  bad or expired token: answering the static reply"), mikä on odotettua yli 24 tunnin istunnoissa;
- `expires_at` pysyy kaukana tulevaisuudessa ja `active` on tosi.

Pelaajan omalla tunnuksella vastaus kulkee peliohjelman "tili löytyi" -polkua; mitä kenttiä se lukee
ja päivittääkö se vanhenemisajan, jäi jäljittämättä (G). `VERIFY_STUB_ACCOUNT=1` palauttaa
paikkamerkin kaikille. Pelitesti on yli 30 minuutin istunto, mieluiten yli tunnisteen 24 tunnin, ilman
uloskirjautumista tai yhteyden uudelleenyrityssilmukkaa.

## Testaus ilman peliä {#testing-without-the-game}

Korjauksia testataan HTTP:n yli oikeaa metagamea vastaan peliohjelman omilla rungoilla ja otsakkeilla:

- **Peliohjelman malli** (`legacy/UndauntedMetagame/test/socialclient.ts`) toteuttaa yllä kuvatut
  jäsennyssäännöt uudelleen: yhdistämisen jäsentimen, käyttäjätietojen välimuistin, jossa ensimmäinen
  vastaus voittaa, työkalupakin Epic-tunnuksen tarkistuksen, Phoenixin kuoren, kiltavirheiden
  muunnoksen, kaverilistan ja ryhmäkutsut. Kun sille syötetään palvelimemme 22.9.2026 lähettämät
  vastaukset, se toistaa sen, mitä pelaajat näkivät: taulukkomuotoinen yhdistäminen ei yhdistä mitään,
  ja vanha käyttäjätietovastaus jättää kutsun lähettäjän mitätöidyksi. Tämä tarkistaa mallin ja estää
  samoja virheitä palaamasta.
- **Sosiaaliset kulut** (`test/socialflow.test.ts`): kaverin lisääminen nimellä koko ketjun läpi,
  odottava pyyntö toisen pelaajan kirjautuessa, estetyt pelaajat, vastaanotettu ryhmäkutsu, jonka
  lähettäjä saadaan näkyviin, hyväksyminen, jokaisen ryhmän jäsenen nimi oikein, johtajan jonoon
  liittyminen niin, että jokaista jäsentä odotetaan täsmälleen kerran, käyttäjätiedot oikean tunnuksen
  alla missä tahansa järjestyksessä, `/accountinfo/public`-reitin nimimuoto, yhdistämisen
  yksityiskohdat, molemmat paluukytkimet, uudet kaverirajat ja ryhmäkutsujen säännöt (esto poistaa
  kutsut kumpaankin suuntaan, tauko hylkäyksen jälkeen, lähettäjän raja). `test/partyhttp.test.ts`
  kattaa myös kytkimen `PARTY_SOLO_STUB=0`.
- **Killat** (`test/guildhttp.test.ts`): jokainen reitti ja virhekoodi, varatut sanat, pelipalvelimen
  tekemä perustaminen sidottuna siihen, että johtaja on itse tarkistanut juuri tämän nimen (toinen
  nimi, nimikyltti tai johtaja torjutaan; mukana tuleva tunniste ei muuta mitään; aktiivisuuteen
  perustuva varakytkin), kutsut, vanheneminen, rajat, arvot ja johtajuuden luovutus, erottaminen,
  lähteminen ja lakkauttaminen (reittien järjestys mukaan lukien), mitä esto, hylkäys ja alennettu,
  erotettu tai lähtenyt upseeri tekevät avoimille kutsuille, uudelleenkäynnistys, oikeudet ja
  `GUILDS=0`. Jokainen vastaus tarkistetaan tarkkana JSONina ja mallin kautta.
- **Slayer Links** (`test/slayerlinks.test.ts`, kirjoitettu uudelleen Harmonicin testistä peliohjelman
  täsmällisillä poluilla ja rungoilla): jokainen reitti avaimineen, kutsulistan kumpikin suunta,
  hyväksyntä valittuun tai ensimmäiseen vapaaseen paikkaan, hylkäys ja peruutus `account_id`-kentällä,
  kutsujen poiston kumpikin merkitys, linkin poisto kummaltakin pelaajalta, kolme paikkaa numeroilla 1–3
  (0 ja 4 torjutaan, kolmatta paikkaa käytetään) ja yksi odottava kutsu paikkaa kohden, kutsuraja ja
  vastattujen kutsujen siivous, vanheneminen testikellolla, kaveruuden purku tai esto perumassa
  odottavat kutsut samalla kun käynnissä oleva linkki jatkuu, 401 ja 403 kaikilla kahdeksalla
  reitillä, `SLAYER_LINKS=0` ja vastaamattomat palkintoreitit.
- **Pienemmät korjaukset** (`test/socialported.test.ts`): kahdesti saapuva ryhmäkutsun hyväksyntä
  (ryhmän tunnuksella ja lähettäjän tunnuksella; ei omalla tunnuksella, vieraalla, yhden hengen ryhmässä
  eikä ryhmässä, josta on lähdetty) sekä `oauth/verify` (kutsujan oma tunnus kelvollisella
  tunnisteella; paikkamerkki ja 200 ilman tunnistetta sekä huonolla, vanhentuneella tai vieraalla
  tunnisteella; `VERIFY_STUB_ACCOUNT=1`).

## Avoimet kysymykset ja oikea testi {#open-questions}

Tarkistetaan seuraavassa kahden pelaajan testissä (vaiheet ovat kohdassa
[Näin se tarkistetaan](#how-to-verify)):

1. Saako `/accountinfo/public`-korjaus yksin vastaanotetut ryhmäkutsut näkyviin? Lokissa pitäisi
   näkyä kutsukysely ja sen jälkeen `accountinfo/public by <vastaanottaja> for <lähettäjä> -> found`.
2. Missä kohdassa saapunut kaveripyyntö näkyy, ja onko kohta piilossa, kun se on tyhjä?
3. Tuleeko Block-valikosta ylipäätään pyyntö kaverireiteillemme (rivi `POST` tai `PUT .../blocklist/...`)?
4. Onko pelipalvelimen pyynnössä `POST /guild` mitään `Authorization`-otsaketta (perustamisrivi
   päättyy "no token" tai "a token of X came along"), ja tarkistaako peliohjelma lopullisen nimen ja
   nimikyltin ennen Create-painiketta (torjunta "no validate of this name" kertoo, ettei tarkistanut)?
5. Kutsutaanko `GET /guild` jokaisella maailman latauksella (34 kutsua 8 kirjautumisessa viittaa
   siihen)?
6. Ketään ei saa poistaa ryhmästä minuutin jälkeen (ei rivejä `DELETE /party/member/<tunnus>` tai
   `/party/leader/<tunnus>`).
7. Toimiiko Invite to Party yksin olevalle johtajalle paikkamerkkiehdokkaan kanssa (lokiin tulee rivi
   `PUT /party/invite`), vai tarvitaanko `PARTY_SOLO_STUB=0`?
8. Montako `account/mapping`-kutsua kukin kirjautuminen tekee (reittiluettelossa 6 kutsua 8
   kirjautumisessa)?
9. Lähettääkö peliohjelma koskaan ryhmäkutsun hyväksynnän kahdesti (rivi `(a repeated accept)`)?
10. Kutsuuko My Links -välilehti Slayer Link -reittejä lainkaan, ja kuinka usein se kyselee reittiä
    `GET /slayerlink/status_good`?
11. Pysyykö yli 30 minuutin istunto kirjautuneena uudella `oauth/verify`-vastauksella?

**Jokainen pelaaja kohtaa kolme muuttunutta vastausta jokaisella kirjautumisella**, käytti hän
Social-paneelia tai ei: `POST /accountinfo/public` (kuvaa nyt kysyttyä tiliä, 404 tuntemattomalle),
`POST /account/mapping` (yhdistää nyt paikallisen pelaajan, kun ennen ei yhdistänyt mitään) ja
`GET /guild/invite/player` (uusi kuori `{"code": "OK", "message": "", "payload": {"invites": []},
"invites": []}` vanhan tyngän `{"code": null, "message": "OK", "payload": {"invites": []}}` sijaan).
Peliohjelman mallimme lukee kaikki kolme tarkoitetulla tavalla, mutta yhtäkään ei ole nähty oikeassa
pelissä. Paluukytkimet, jos jokin niistä häiritsee: `ACCOUNTINFO_PUBLIC_LEGACY=1`, `ACCOUNT_MAPPING=0`
ja `GUILDS=0` (katso [Asetukset]({{ config_page.url | relative_url }})). 23.9.2026 alkaen neljäs
vastaus muuttuu kaikille: `oauth/verify` nimeää pelaajan oman tilin (paluukytkin
`VERIFY_STUB_ACCOUNT=1`).

## Näin se tarkistetaan {#how-to-verify}

Testi kahdella pelaajalla, A ja B, vuokrapalvelimella. Jokainen vaihe kertoo, mitä tehdään, mitä
metagamen lokiin (palvelinpaketin palvelimella `data\logs\metagame.out.log`) pitäisi tulla ja mitä
tehdä, jos ei tule. `<A>` ja `<B>` ovat kahden tilin tunnukset (`UID-...`); pyyntöloki kirjaa jokaisen
kutsun muodossa `METODI /polku gs=0|1` (`gs=1`: pelipalvelimelta).

**0. Ennen testiä (isäntä).**

1. Vie päivitys palvelimelle. Ensimmäisellä käynnistyksellä metagame ajaa siirron `0013_guilds` ja
   alkaa kuunnella tavalliseen tapaan; `guild:`-rivejä ei tule, ennen kuin joku käyttää kiltoja.
2. Valinnainen: aseta metagamen asetuksiin `LOG_BODIES=1` vain tämän testin ajaksi, jotta pyyntöjen
   rungot tallentuvat (tilitunnuksia ja kiltojen nimiä; paketti kytkee sen taas pois julkisessa
   tilassa). Poista runkoloki jälkeenpäin.
3. Kumpikin pelaaja **sulkee pelin kokonaan ja käynnistää sen uudelleen** käynnistimestä. Peliohjelma
   pitää vanhat vastaukset uudelleenkäynnistykseen asti.

**1. Kirjautuminen (kumpikin pelaaja).** Kullekin pelaajalle X: `POST /login`,
`friends: list for <X>: 0 friend(s), 0 pending`, `GET /friends/api/public/blocklist/<X>`,
`accountinfo/public by <X> for <X> -> found`, `GET /guild gs=0` ja `GET /guild/invite/player gs=0`
(eikä riviä "Guild invites (stubbed)"), `POST /party gs=0` noin 10 sekunnin välein sekä
`party: poll by=<X> P=... size=1 leader=<X>` (kyselyrivi kirjataan uudelleen vain, kun ryhmä muuttuu).
Laske rivit `account/mapping by <X>: ... -> 1 of 1 mapped` (kysymys 8). Pelaaja pääsee Ramsgateen ja
Social-paneeli aukeaa; kaikki näkyvät tilassa Offline, mikä on odotettua.
Jos kirjautuminen jumittuu tai Social-paneeli hajoaa, kytke `ACCOUNTINFO_PUBLIC_LEGACY=1`, käynnistä
metagame ja kumpikin peli uudelleen ja yritä uudelleen; sitten `ACCOUNT_MAPPING=0` ja sitten
`GUILDS=0`, yksi kerrallaan, syyllisen muutoksen löytämiseksi.

**2. A kutsuu B:n ryhmään.** A: Social, B kohdasta Hunt Members (tai chatissa `/invite <B:n nimi>`),
Invite to Party. Lokiin `PUT /party/invite gs=0` ja `party: invite P=<PA> from=<A> to=<B>`, sitten
noin 10 sekunnin kuluessa `party: invites for <B> -> 1 (P=<PA> from=<A>)` ja
`accountinfo/public by <B> for <A> -> found`. B näkee ilmoituksen ja merkinnän kohdassa PARTY INVITES
(kysymys 1).

- Riviä `PUT /party/invite` ei tule lainkaan, ja valikkokohta on harmaana tai ei tee mitään:
  paikkamerkki näyttää jonottamiselta (kysymys 7). Aseta `PARTY_SOLO_STUB=0`, käynnistä metagame
  uudelleen (pelit voivat jäädä auki; ryhmät alkavat alusta) ja yritä uudelleen.
- `party: invite by=<A> to=<B> refused ...`: syy on rivillä.
- Kutsukysely näyttää 1 ja `accountinfo/public ... -> found` tulee, mutta B ei näe mitään: diagnoosi
  on jossain kohdin väärä; kirjaa se ylös ja jatka muita vaiheita isännän `PartyInvite`-reitillä.

**3. B hyväksyy.** Lokiin `party: accept by <B> matched=partyId P=<PA> size=2` ja sitten
`party: poll by=<A> P=<PA> size=2 leader=<A> members=<A>,<B>`. Kummankin ryhmäpaneelissa näkyvät
molemmat nimet. Jos peliohjelma lähettää saman ryhmän hyväksynnän toisen kerran, lokiin tulee
`... (a repeated accept)` ja vastauksena ryhmä (kysymys 9). (Hylkäys kirjaa sen sijaan rivin `party: decline by=<B> ... removed=1`, eikä A voi
kutsua B:tä uudelleen 2 minuuttiin.)

**4. Yhteinen metsästys.** A (johtaja) valitsee metsästyksen. Lokiin `mm: party P=<PA> candidate <C>
mode=... hunt=... members=<A>,<B> by=<A>` ja sitten
`mm: party P=<PA> candidate <C> ready at <osoite>:<portti> for 2 member(s)`; kumpikin päätyy samaan
metsästykseen ja palaa johtajan mukana Ramsgateen. Sen jälkeen kyselyt näyttävät yhä `size=2`. Koko testin aikana ei saa tulla rivejä
`DELETE /party/member/<tunnus>` tai `DELETE /party/leader/<tunnus>` (kysymys 6).

**5. Kaverin lisääminen.** A: Social, Add Friends, B:n käyttäjänimi, Add. Lokiin
`EOS Account by name by <A>: <B>`, `account/mapping by <A>: ... ids=[1]; ... -> 1 of 1 mapped`,
`accountinfo/public by <A> for <B> -> found` ja `friends: request by=<A> to=<B> -> requested`; A näkee
ilmoituksen "friend invite sent". B käynnistää pelin uudelleen (listat luetaan vain kirjautuessa):
lokiin `friends: list for <B>: 0 friend(s), 1 pending`; kirjaa ylös, missä pyyntö näkyy (kysymys 2).
B hyväksyy: `friends: request by=<B> to=<A> -> accepted`; seuraavan kirjautumisen jälkeen kumpikin
näkee toisen kohdassa OFFLINE (`1 friend(s)`).

- `EOS Account by name by <A>: not found`: nimi kirjoitettiin väärin (sen on oltava tarkka,
  kirjainkoolla ei väliä).
- Nimihaun jälkeen ei tule `account/mapping`-riviä, tai siinä lukee `0 of 1 mapped`: yhdistäminen
  epäonnistui; kirjaa se ylös (`ACCOUNT_MAPPING` ei saa olla `0`).
- Valinnainen, kysymys 3: B estää A:n A:n valikosta; lokiin rivi `POST` tai `PUT`
  `/friends/api/public/blocklist/<B>/<A>` ja `friends: block by=<B> target=<A> -> blocked`; kirjaa
  ylös, kumpi metodi. Eston poisto kirjaa `-> unblocked`.

**6. A perustaa killan.** A: Guilds-välilehti, CREATE GUILD, nimi (4–15 kirjainta ja numeroa) ja
nimikyltti, **odota sekunti**, paina Create. Lokiin yksi tai useampi
`guild: validate by <A> name="..." tag="..." -> ok`, sitten `POST /guild gs=1` ja
`guild: created G=<tunnus> name=... tag=... leader=<A> (validated name, no token)`. Kirjaa ylös "no
token" tai "a token of X came along" (kysymys 4). A näkee killan näkymän ("Members: 1 / 100") ja
`[TAG]`-tunnuksen päänsä yllä.

- `guild: create for <A> ... refused 403 (no code): no validate of this name and nameplate ...`:
  perustaminen nimesi jotain, mitä A ei ollut tarkistanut. Kirjoita uudelleen, odota, että ikkuna on
  tarkistanut nimen, ja paina Create uudelleen. Jos näin käy toistuvasti, peliohjelma ei tarkista
  lopullista nimeä: aseta `GUILD_CREATE_ACTIVITY_FALLBACK=1`, käynnistä metagame uudelleen ja yritä
  uudelleen (kysymys 4).
- Nimitarkistus torjutaan koodilla (esimerkiksi `409 SeizedAdorableQuillshot`): ikkuna näyttää syyn;
  valitse toinen nimi.
- Riviä `POST /guild gs=1` ei tule lainkaan: pelipalvelin ei lähettänyt perustamista; katso
  Ramsgaten pelipalvelimen loki.
- `guild: create for <joku muu>`: johtajatunnus ei ole A:n oma; kirjaa se ylös.

**7. Kiltakutsu, hyväksyminen, arvot.** A: Guilds-välilehden jäsenen lisäyskenttä, B:n käyttäjänimi
(tai Invite to Guild B:n valikosta). Lokiin `EOS Account by name by <A>: <B>`, yhdistämisrivi ja
`guild: invite by=<A> to=<B> -> 200`. B matkustaa (metsästykseen ja takaisin) tai kirjautuu
uudelleen: lokiin `GET /guild/invite/player gs=0` ja `accountinfo/public by <B> for <A> -> found`; B
näkee kutsun kohdassa GUILD INVITES. B hyväksyy: `guild: accept by=<B> G=<tunnus> -> 200`, ja B näkee
killan näkymän. A näkee B:n oman seuraavan `GET /guild` -kyselynsä jälkeen (maailman lataus). Sitten
halutessa: Promote To Guild Officer (`guild: rank by=<A> target=<B> rank="officer" -> 200`), Leave
Guild (`guild: leave by=<B> -> 200`), DISBAND GUILD (`guild: disband G=<tunnus> by=<A> -> 200`). Laske
`GET /guild` -rivit maailman latausta kohden (kysymys 5).

**8. Slayer Links (vaiheiden 1–7 jälkeen, kun pelaajat ovat kavereita; kun 23.9.2026 päivitys on
palvelimella).** A: Social, My Links, kutsu B johonkin paikkaan. Lokiin `PUT /slayerlink/invite gs=0`
ja `slayerlink: invite by=<A> to=<B> slot=<n> -> sent id=<tunnus>`. B avaa My Links -välilehden:
lokiin `GET /slayerlink/invites` (tai `/slayerlink/status_good`), ja B näkee kutsun A:n nimellä. B
hyväksyy: `slayerlink: accept by=<B> other=<A> id=<tunnus> -> accepted (slots X and Y)`; kumpikin näkee
linkin. Sitten halutessa: poista linkki (`slayerlink: delete link by=... -> removed <tunnus> (with
...)`), ja uudella kierroksella hylkää (`-> declined`) ja peru (`-> canceled`). Jos yhtään
`/slayerlink`-riviä ei tule, peliohjelma pitää välilehden piilossa: kirjaa se ylös. Jos jokin toimii
väärin, aseta `SLAYER_LINKS=0` ja käynnistä metagame uudelleen.

**9. Jälkeenpäin.** Etsi pyyntölokista sosiaalisia reittejä, jotka saivat vastauksen 404, ja
`refused`-rivejä, joita et odottanut. Kytke `LOG_BODIES` taas pois ja poista runkoloki. Kirjaa
vastaukset yllä oleviin avoimiin kysymyksiin; korjaukset tulevat tälle sivulle ja tiekarttaan.

## Myöhemmin {#deferred}

| Asia | Miksi |
|:-----|:------|
| **Paikalla olo oletuksena** (chat-yhteyden läsnäolotiedot) | Paikalla olo, EPIC FRIENDS, "In Ramsgate" ja uuden kaverin näkyminen ilman uutta kirjautumista kulkevat kaikki XMPP:n läsnäolotietojen kautta. Se on rakennettu chat-palvelimeen 23.9.2026 ([Tekstichat]({{ chat_page.url | relative_url }}#presence)), eikä se koskaan lähetä pelaajalle hänen omaa läsnäolotietoaan, mutta se pysyy pois päältä (`CHAT_PRESENCE=0`), kunnes chat itse on käytössä ja kahden hengen ryhmä on pitänyt molemmat jäsenensä minuutin ajan asetus päällä (tiekartan kohta 3.10). |
| Kaveripalvelun viimeaikaiset pelaajat | Ei koskaan kutsuttu oikeasti; pelin Recent Players -lista on hahmon tiedoissa. |
| Muut kaverireitit (asetusten lähteet, kaikkien poisto, sähköpostihaut) | Ei koskaan kutsuttu oikeasti. |
| Lähetettyjen ryhmäkutsujen listaaminen | Peliohjelma voisi luulla niitä saapuneiksi. |
| Ryhmänhaku (party finder), konsoli-istunnot, Phoenixin paikalla olon yhteys | Ei koskaan kutsuttu; kyselyt kuljettavat kaiken ryhmän tilan. |
| Puhe | Vivox on poissa; kiltapuhetta ei ole toteutettu versiossa 1.4.4 lainkaan. |
| Slayer Linksin palkinnot | Kahta palkintoreittiä ei ole vielä jäljitetty; väärä onnistumisvastaus voisi hukata palkinnon. Muu osa [Slayer Linksistä](#slayer-links) on rakennettu. |
| Kiltarajapinnan versio 1 | Versio 1.4.4 ei koskaan kutsu sitä. |

[Version 2.1.1 yksinpelikokeilu]({{ awakening_page.url | relative_url }}) kuvaa samat Phoenixin
palvelut viimeisestä peliversiosta, mukaan lukien käärityt ja litteät vastausmuodot.
[Liity kaverina]({{ friends_page.url | relative_url }}) kertoo pelaajille, mikä toimii nyt.
