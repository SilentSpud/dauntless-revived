# Linux server port

This is a separate Linux deployment port for Dauntless Revived. It does not replace or call the
Windows Server kit in `deploy/windows-server/`.

## Architecture

- The metagame, deploy server, content server, TLS gateway and allowlist helper are the Rust
  binaries `metagame`, `deploy`, `content`, `gateway` and `allowlist` in `<install-root>/bin`
  (MIGRATION_PLAN.md M39). No Node.js is needed.
- Ramsgate, the Training Dojo and hunt processes are still the original Dauntless 1.4.4 Windows
  x86_64 executable. `deploy run-game` runs them through Proton or Wine.
- The deploy server keeps its existing process contract: `GAMESERVER_BINARY_PATH` points at
  `bin/dr-run-game`, a link to `deploy` that runs as the Linux wrapper, instead of directly at the
  Windows EXE.
- Public mode uses the existing TLS gateway and a Linux nftables allowlist. Only player IPs reported
  by authenticated gateway traffic are admitted to UDP 8770-8777.
- systemd supervises the services.

The launcher/server DLL pins and the game EXE pin are checked before the Linux game runner is used.

## Requirements

- x86_64 or aarch64 Linux with glibc
- the service binaries: either a CI `server-x86_64-unknown-linux-gnu` or
  `server-aarch64-unknown-linux-gnu` download passed with `--binaries <dir>` (checked against its
  `SHA256SUMS`), or Rust (`rustup`) to build them from the checkout with `cargo build --release`
- Proton/GE-Proton or 64-bit Wine
- `xvfb-run` for a headless game worker unless a real X display is supplied
- systemd for the normal service install
- nftables for public mode
- about 11 GB for the pinned Dauntless 1.4.4 game tree, plus prefix/cache space

The server needs the exact pinned build:
`d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4` for
`Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe`.

## Install on one Linux host

Private/Tailscale-style deployment:

```sh
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 100.x.y.z \
  --mode private
```

Public deployment:

```sh
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0
```

Use an explicit Proton build:

```sh
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0 \
  --proton /home/dauntless/.local/share/Steam/compatibilitytools.d/GE-Proton10-1/proton
```

Or an explicit Wine binary:

```sh
sudo ./deploy/linux-server/install.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --my-ip 203.0.113.10 \
  --mode public \
  --firewall-interface eth0 \
  --wine /usr/bin/wine64
```

Without `--binaries`, the installer builds the services in the checkout with `cargo`, so run it from
an account that has Rust (for example `sudo env PATH="$PATH" ./deploy/linux-server/install.sh ...`),
or build first with `cargo build --release --locked --bins -p metagame -p gateway -p content -p deploy`
in `rust/` and pass `--binaries rust/target/release`. Running the installer again upgrades: the
binaries are replaced, settings, secrets, the database, the certificate and the owner invite are
kept, and the database is migrated before the services start.

The normal install writes code under `/opt/dauntless-revived`, state under
`/var/lib/dauntless-revived`, secret configuration under `/etc/dauntless-revived`, and systemd
units under `/etc/systemd/system`.

It creates a dedicated `dauntless` service user by default. Make sure that user can read/traverse
the game tree you pass with `--game-dir`.

## Public firewall

The allowlist helper owns only the nftables table `inet dauntless_revived`. It keeps UDP
8770-8777 closed except for currently authenticated player IPs. The TLS gateway listens on TCP 443.

If another firewall manager (UFW, firewalld, a cloud firewall) also filters the host, it must permit
TCP 443 and the UDP game range at its outer layer; the Dauntless nftables table still applies the
per-player restriction. Do not create a broad public UDP rule unless the dynamic allowlist remains
in the packet path.

## Service control

```sh
sudo ./deploy/linux-server/stack.sh status
sudo ./deploy/linux-server/stack.sh restart
sudo ./deploy/linux-server/stack.sh logs
sudo ./deploy/linux-server/stack.sh restart deploy
```

## Create an invite

```sh
sudo -u dauntless /opt/dauntless-revived/bin/metagame invite \
  --config /etc/dauntless-revived \
  --name FriendName
```

The invite code is stored directly in SQLite and the complete v1/v2 invite is printed once.

## Separate Linux game worker

OpenBSD uses this mode, but it is also useful when you want the services and game processes on
different Linux machines. The worker needs only the `deploy` binary (`--binaries <dir>`, or `cargo`).

On the worker:

```sh
sudo ./deploy/linux-server/install-worker.sh \
  --game-dir /srv/dauntless/BaseGame144/Dauntless \
  --proton /home/dauntless/.local/share/Steam/compatibilitytools.d/GE-Proton10-1/proton
```

The installer creates the fixed `dr-game-worker` command. For an SSH worker key, restrict
`authorized_keys` to the printed `restrict,command=".../ssh-worker-command.sh"` prefix. The forced
command accepts only one bounded base64url JSON argv payload and never evaluates it as shell syntax.

## Validation status

`deploy/linux-server/tests/install-test.sh <bin-dir>` runs both installers in test mode, twice, with
the Rust binaries; CI runs it on every change to the kit or the services. The notes below are from
the Node version of this kit.

On 1 October 2026 this port was tested against the local pinned
`/home/mikael-nurminen/Downloads/BaseGame144/Dauntless` tree:

- all 410 manifest files were found at their expected sizes;
- the 1.4.4 EXE hash matched;
- the two pinned server DLLs were installed and hash-checked;
- all four Node services built natively on Linux;
- the SQLite migrations and game-server API key registration succeeded;
- a three-service smoke test started metagame/content/deploy together;
- the deploy API reported a live Ramsgate process on UDP 8777;
- a fake Wine runtime received the exact production game-server argv, including `-server -nullrhi`;
- the Linux CI job passes the full server test suites and the Unix-port tests.

This machine did not have Wine or Proton installed, so the real Dauntless Windows process has not yet
been booted here under Wine/Proton. The next live milestone is Ramsgate plus one hunt with a real
Linux compatibility runtime.
