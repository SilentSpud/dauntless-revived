---
title: Linux-palvelin
parent: Asennus
grand_parent: Dauntless Revived suomeksi
nav_order: 7
description: "Dauntless Revived -palvelin Linuxissa: natiivit Rust-palvelut, Proton/Wine-pelipalvelimet, systemd ja nftables."
lang: fi
ref: setup/linux-server
locale: fi_FI
---

{% assign linux_launcher = site.pages | where: "path", "fi/setup/linux.md" | first %}
{% assign trouble_page = site.pages | where: "path", "fi/setup/troubleshooting.md" | first %}

# Linux-palvelin
{: .no_toc }

Linux-palvelin on erillinen porttaus Windows Server -paketista. Metagame, deploy-palvelin,
sisältöpalvelin ja TLS-yhdyskäytävä toimivat **natiivisti Linuxissa**. Ramsgate, Training Dojo ja
metsästykset ovat edelleen alkuperäisen Dauntless 1.4.4:n Windows x86_64 -prosesseja, jotka
käynnistetään **Protonin tai Winen** kautta.

`deploy/windows-server/`-kansion tiedostoja ei käytetä eikä muuteta.

<details open markdown="block">
  <summary>Sisältö</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Tilanne

**1.10.2026:** Linuxin ohjauspuoli toimii ja sitä testataan CI:ssä.

Olemme varmistaneet, että:

- kaikki neljä Node-palvelua kääntyvät natiivisti Linuxissa;
- SQLite-migraatiot ja pelipalvelimen avaimen rekisteröinti toimivat;
- 1.4.4-pelikansion kaikki 410 manifestitiedostoa löytyvät oikean kokoisina;
- EXE:n tiiviste täsmää ja kaksi kiinnitettyä DLL:ää asennetaan ja tarkistetaan;
- metagame, sisältöpalvelin ja deploy-palvelin käynnistyvät yhdessä;
- deploy-rajapinta näyttää Ramsgaten portissa 8777;
- nykyinen pelipalvelimen komentorivi kulkee Linux-kääreelle muuttumattomana.

Seuraava oikea integraatiotesti on **käynnistää aito peli Protonilla/Winellä ja pelata Ramsgate sekä
yksi metsästys**. Ensimmäisessä porttausympäristössä ei ollut asennettua Protonia tai Wineä, joten
prosessin käynnistystesti käytti testiympäristöä.

## Arkkitehtuuri

```text
Pelaajat
   |
   +-- yksityinen: Tailscale -----+
   |                              |
   +-- julkinen: TLS :443 -> Gateway
                                  |
                         Linuxin natiivit palvelut
                         Metagame / SQLite
                         Content / Deploy
                                  |
                                  v
                         Proton tai Wine
                                  |
                         Dauntless 1.4.4 -server
                         UDP 8770-8777
```

Windowsin deploy-palvelimen prosessirajapintaa ei muutettu. Linuxissa
`GAMESERVER_BINARY_PATH` osoittaa `bin/dr-run-game`-linkkiin, joka ajaa `deploy`-ohjelmaa kääreenä.

## Vaatimukset

> **Hostattu Linux:** EU Gamehost on projektin kumppani. [Hosting-kumppanin opas]({{ "/fi/setup/hosting-partner.html" | relative_url }}) sisältää pakettiesimerkkejä; varmista ennen tilausta tarvitsemasi käyttöjärjestelmä ja virtualisointi.

- x86_64- tai aarch64-Linux + glibc
- palveluohjelmat: CI:n `server-<target>`-lataus `--binaries <kansio>`-valitsimella (tarkistetaan
  sen `SHA256SUMS`-tiedostoa vasten) tai Rust (`rustup`), jolla ne käännetään lähdekoodista
- täsmälleen oikea Dauntless **1.4.4**
- Proton/GE-Proton tai 64-bittinen Wine
- päättömässä käytössä `xvfb-run`, ellei oikeaa X-näyttöä ole
- systemd normaalia palvelinasennusta varten
- nftables julkista tilaa varten
- riittävästi muistia palveluille ja samanaikaisille peliprosesseille

Vaadittu EXE:n SHA-256:

```text
d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4
```

Asennin hylkää muun peliversion.

## Yksityinen palvelin

Tailscale-osoitteella:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 100.x.y.z \
  --mode private
```

Yksityinen tila ei käynnistä julkista TLS-yhdyskäytävää eikä nftables-allowlist-palvelua.

## Julkinen palvelin

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0
```

Julkisessa tilassa:

- TCP 443 on julkinen TLS-yhdyskäytävä;
- metagame/content/deploy ovat sen takana;
- UDP 8770-8777 on kiinni kaikilta paitsi kirjautuneiden pelaajien osoitteilta.

Allowlist omistaa oman nftables-taulun `inet dauntless_revived`.

## Proton tai Wine

Proton:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0 \
  --proton /home/dauntless/.local/share/Steam/compatibilitytools.d/GE-Proton10-1/proton
```

Wine:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0 \
  --wine /usr/bin/wine64
```

Kääre pakottaa asetuksen `WINEDLLOVERRIDES=dxgi=n,b`, jotta kiinnitetty `dxgi.dll` latautuu.

## Tiedostot ja palvelut

| Asia | Oletuspolku |
|---|---|
| Sovellus | `/opt/dauntless-revived` |
| Tietokanta, TLS ja tila | `/var/lib/dauntless-revived` |
| Salaiset asetukset | `/etc/dauntless-revived` |
| systemd-yksiköt | `/etc/systemd/system/dauntless-*.service` |

Hallinta:

```bash
sudo ./deploy/linux-server/stack.sh status
sudo ./deploy/linux-server/stack.sh restart
sudo ./deploy/linux-server/stack.sh restart deploy
sudo ./deploy/linux-server/stack.sh logs
```

## Tee kutsu

```bash
sudo -u dauntless /opt/dauntless-revived/bin/metagame invite \
  --config /etc/dauntless-revived \
  --name KaverinNimi
```

Työkalu lisää kertakäyttöisen kutsukoodin SQLiteen ja tulostaa valmiin v1/v2-kutsun.

Linux-pelaajan asennusohje on sivulla
[Linux-käynnistin]({{ linux_launcher.url | relative_url }}).

## Erillinen Linux-pelityöntekijä

Peliprosessit voidaan ajaa toisella Linux-koneella. OpenBSD-porttaus käyttää juuri tätä mallia.

```bash
sudo ./deploy/linux-server/install-worker.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --proton /home/dauntless/.local/share/Steam/compatibilitytools.d/GE-Proton10-1/proton
```

Asennin tekee kiinteän `dr-game-worker`-komennon. Rajoita SSH-avain asennuksen tulostamalla
`restrict,command=".../ssh-worker-command.sh"`-asetuksella. Avaimella ei pidä olla sudo-oikeuksia
eikä yleistä shelliä.

## Varmuuskopio ja päivitys

Linux-portissa ei vielä ole Windows-paketin transaktionaalista päivitystä ja automaattista
palautusta.

Varmuuskopio (turvallinen myös palvelujen ollessa käynnissä):

```bash
sudo ./deploy/linux-server/stack.sh backup
```

Se kirjoittaa kansion `/var/lib/dauntless-revived/backups/<päivä_aika>/`: tietokannan (online-kopio
`metagame db backup` -komennolla, tarkistettuna) ja `secrets/`-kansioon asetukset,
pelipalvelinavaimen ja yhdyskäytävän varmenteen. Kopioi kansio pois palvelimelta. Älä kopioi
`undaunted.db`:tä käsin metagamen ollessa käynnissä: viimeisimmät muutokset voivat olla vielä sen
WAL-tiedostossa.

Päivitä lähdekoodi ja aja asennin uudelleen samoilla asetuksilla (se kääntää tai ottaa uudet
ohjelmat ja päivittää tietokannan ennen palvelujen käynnistystä). Säilytä samat data- ja
config-kansiot.

## Vianetsintä

**Protonia/Wineä ei löydy:** anna absoluuttinen `--proton`- tai `--wine`-polku.

**DISPLAY puuttuu:** asenna Xvfb/`xvfb-run` tai käytä `--xvfb 0` vain, jos oikea näyttö on jo
käytössä.

**Gateway toimii mutta metsästys ei:** tarkista palveluntarjoajan palomuuri ja nftables-taulu
`dauntless_revived`.

**Ramsgate käynnistyy jatkuvasti uudelleen:** tarkista `journalctl -u dauntless-deploy` ja
Proton/Wine-lokit ennen pelitiedostojen muuttamista.

Katso myös [Vianetsintä]({{ trouble_page.url | relative_url }}).
