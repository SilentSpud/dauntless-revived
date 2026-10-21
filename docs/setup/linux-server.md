---
title: Linux server
parent: Setup
nav_order: 7
description: "Run the Dauntless Revived server stack on Linux: native Rust services, Proton/Wine game servers, systemd and nftables."
lang: en
ref: setup/linux-server
---

{% assign linux_launcher = site.pages | where: "path", "setup/linux.md" | first %}
{% assign trouble_page = site.pages | where: "path", "setup/troubleshooting.md" | first %}

# Linux server
{: .no_toc }

The Linux server port is separate from the Windows Server kit. It runs the metagame, deploy server,
content server and TLS gateway **natively on Linux** as Rust binaries, while Ramsgate, the Training Dojo and hunts use
the original Dauntless 1.4.4 Windows x86_64 executable through **Proton or Wine**.

Nothing under `deploy/windows-server/` is used or changed by this port.

<details open markdown="block">
  <summary>Contents</summary>
  {: .text-delta }
1. TOC
{:toc}
</details>

## Status

As of **1 October 2026**, the Linux control plane is working and continuously tested on Linux.

We have verified locally that:

- all four Node services build natively on Linux;
- SQLite migrations and game-server key registration work;
- the pinned 1.4.4 game tree passes all 410 manifest size checks;
- the exact 1.4.4 EXE hash is accepted and both pinned DLLs are installed;
- metagame, content and deploy run together;
- the deploy API reports Ramsgate on UDP 8777;
- the existing game-server argument contract reaches the Linux compatibility wrapper unchanged.

The final live milestone is still **running the real game process under Proton/Wine and joining
Ramsgate plus one hunt**. The development PC used for the first porting pass did not have a real
Proton/Wine runtime installed, so the launch-contract smoke test used a stand-in runtime.

## Architecture

```text
Players
   |
   +-- private: Tailscale ----------+
   |                                |
   +-- public: TLS :443 ---> Gateway|
                                    v
                         Native Linux services (Rust)
                         + Metagame / SQLite
                         + Content
                         + Deploy
                                    |
                                    v
                         bin/dr-run-game (deploy)
                                    |
                              Proton / Wine
                                    |
                         Dauntless 1.4.4 -server
                         UDP 8770-8777
```

The existing deploy server still starts one executable path. On Linux that path is
`bin/dr-run-game`, a link to the `deploy` binary that runs as the wrapper; Windows continues
pointing directly at the EXE.

## Requirements

> **Hosted Linux:** EU Gamehost is a project partner. Our [hosting partner guide]({{ "/setup/hosting-partner.html" | relative_url }}) lists example plans; confirm that the exact OS/virtualisation setup you need is available before ordering.

- x86_64 or aarch64 Linux with glibc
- the service binaries: a CI `server-<target>` download passed with `--binaries <dir>` (checked
  against its `SHA256SUMS`), or Rust (`rustup`) to build them from the checkout
- the exact Dauntless **1.4.4** game tree
- Proton/GE-Proton or 64-bit Wine
- `xvfb-run` for a headless game worker unless you provide a real X display
- systemd for the normal service install
- nftables for public mode
- enough RAM for the services plus however many game processes you allow

The required game EXE SHA-256 is:

```text
d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4
```

The installer refuses a different build.

## Prepare the host

Clone the repository and put your own 1.4.4 game tree somewhere the service account can traverse,
for example:

```text
/srv/dauntless/BaseGame144/Dauntless
```

On a Debian/Ubuntu-style server, a typical prerequisite set is:

```bash
sudo apt update
sudo apt install nftables xvfb
```

Install Proton or Wine separately. For a headless dedicated host, a known Proton/GE-Proton or Wine
build should be pinned instead of changing runtimes on every update.

## Private server

For a host reachable through Tailscale:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 100.x.y.z \
  --mode private
```

Private mode exposes the metagame/content/chat services on the address you provide and does not start
the public TLS gateway or nftables allowlist helper.

## Public server

For a public IPv4 host:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0
```

Public mode uses the same pinned-TLS launcher flow as the Windows server:

- TCP 443 is the public gateway;
- metagame/content/deploy stay behind it;
- UDP 8770-8777 is fail-closed except for player addresses reported by authenticated gateway traffic.

The helper owns its own nftables table, `inet dauntless_revived`.

## Choose Proton or Wine

Explicit Proton:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0 \
  --proton /home/dauntless/.local/share/Steam/compatibilitytools.d/GE-Proton10-1/proton
```

Explicit Wine:

```bash
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0 \
  --wine /usr/bin/wine64
```

The wrapper forces `WINEDLLOVERRIDES=dxgi=n,b` so the pinned proxy `dxgi.dll` loads. It keeps a
dedicated compatibility prefix/data directory under the server data tree.

## Files and services

Default locations:

| What | Path |
|---|---|
| Application | `/opt/dauntless-revived` |
| State / database / TLS | `/var/lib/dauntless-revived` |
| Secret environment files | `/etc/dauntless-revived` |
| systemd units | `/etc/systemd/system/dauntless-*.service` |

Services:

- `dauntless-metagame`
- `dauntless-deploy`
- `dauntless-content`
- public mode only: `dauntless-allowlist`, `dauntless-gateway`

Manage them with:

```bash
sudo ./deploy/linux-server/stack.sh status
sudo ./deploy/linux-server/stack.sh restart
sudo ./deploy/linux-server/stack.sh restart deploy
sudo ./deploy/linux-server/stack.sh logs
```

## Create an invite

```bash
sudo -u dauntless /opt/dauntless-revived/bin/metagame invite \
  --config /etc/dauntless-revived \
  --name FriendName
```

It writes a single-use code directly into SQLite and prints the complete v1/v2 invite.

Friends on Linux can use the [Linux launcher]({{ linux_launcher.url | relative_url }}).

## Separate Linux game worker

The game processes can live on another Linux machine. This is also the worker mode the OpenBSD port
uses.

On the worker:

```bash
sudo ./deploy/linux-server/install-worker.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --proton /home/dauntless/.local/share/Steam/compatibilitytools.d/GE-Proton10-1/proton
```

The worker installer creates the fixed `dr-game-worker` command. Give the control-plane host a
dedicated SSH key and restrict its `authorized_keys` entry to the forced command printed by the
installer. That key should have no sudo access and no general shell.

## Backups and updates

The Linux port does not yet have the Windows kit's transactional update/rollback script.

Before updating, back up the data directory. The database can be copied safely while the metagame
runs:

```bash
sudo -u dauntless /opt/dauntless-revived/bin/metagame db backup \
  /var/lib/dauntless-revived/undaunted.db \
  /var/lib/dauntless-revived/undaunted-backup.db
```

Then pull/copy the newer source and re-run the installer with the same arguments (it builds or
takes the new binaries and migrates the database before the services start). Keep the same
`/var/lib/dauntless-revived` and `/etc/dauntless-revived` directories to preserve keys and data.

## Troubleshooting

**No Proton or Wine found:** pass `--proton /absolute/path/to/proton` or
`--wine /absolute/path/to/wine64`.

**No DISPLAY:** install Xvfb/`xvfb-run`, or deliberately pass `--xvfb 0` only when a display is
already supplied.

**Public players reach the gateway but not hunts:** check that your provider/cloud firewall permits
the UDP game range and that the nftables `dauntless_revived` table is still in the packet path.

**Ramsgate repeatedly restarts:** inspect `journalctl -u dauntless-deploy` and the compatibility
runtime output first. Do not replace the pinned EXE/DLLs with another game build.

See [Troubleshooting]({{ trouble_page.url | relative_url }}) for shared server/client issues.
