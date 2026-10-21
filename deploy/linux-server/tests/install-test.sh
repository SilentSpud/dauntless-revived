#!/bin/sh
# install.sh and install-worker.sh in test mode (MIGRATION_PLAN.md M39): no root, no systemd, no game
# build check, with the service binaries from <bin-dir> (a cargo target folder or a CI download).
# Installs twice, as an upgrade would, and checks what an operator gets: the settings, the database
# with its game-server key and owner invite, the gateway certificate, the units' commands, and the
# worker's launcher.
#   sh deploy/linux-server/tests/install-test.sh <bin-dir>
set -eu

bins=${1:?usage: install-test.sh <bin-dir>}
bins=$(CDPATH= cd -- "$bins" && pwd)
kit=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() { echo "install test: $*" >&2; exit 1; }

mkdir -p "$work/game/Archon/Binaries/Win64"
for round in 1 2; do
  sh "$kit/install.sh" --game-dir "$work/game" --my-ip 203.0.113.10 --mode public \
    --install-root "$work/opt" --data-dir "$work/data" --config-dir "$work/etc" \
    --binaries "$bins" --no-services --skip-game-prep > "$work/install-$round.log"
  cp "$work/data/owner-invite-code.txt" "$work/invite-$round"
done

for env in metagame gateway allowlist content deployserver; do
  [ -f "$work/etc/$env.env" ] || fail "no $env.env"
done
grep -q "^GAMESERVER_BINARY_PATH=$work/opt/bin/dr-run-game$" "$work/etc/deployserver.env" ||
  fail "the deploy server does not launch through bin/dr-run-game"
[ "$(readlink "$work/opt/bin/dr-run-game")" = deploy ] || fail "dr-run-game is not a link to deploy"
for name in metagame gateway allowlist content deploy; do
  [ -x "$work/opt/bin/$name" ] || fail "bin/$name missing"
done

"$work/opt/bin/metagame" db integrity "$work/data/undaunted.db" | grep -q "^ok, " || fail "database integrity"
[ -s "$work/data/gameserver.key" ] || fail "no game-server key"
"$work/opt/bin/metagame" db gs-key "$work/data/undaunted.db" "$work/data/gameserver.key" | grep -qi "already" ||
  fail "the game-server key is not registered"
invite=$(cat "$work/data/owner-invite-code.txt")
case "$invite" in *[!0-9a-f]*|"") fail "bad invite code" ;; esac
[ "${#invite}" -eq 20 ] || fail "the invite code is not 10 random bytes"
cmp -s "$work/invite-1" "$work/invite-2" || fail "the second install made a new invite"
[ -f "$work/data/tls/gateway-cert.pem" ] && [ -f "$work/data/tls/gateway-key.pem" ] || fail "no gateway certificate"
grep -q "^Fingerprint: " "$work/install-2.log" || fail "the install does not print the certificate's fingerprint"
[ "$(grep "^Fingerprint: " "$work/install-1.log")" = "$(grep "^Fingerprint: " "$work/install-2.log")" ] ||
  fail "the second install made a new certificate"

# stack.sh backup: the database through metagame db backup, the settings and secrets beside it
sh "$work/opt/deploy/linux-server/stack.sh" backup --config-dir "$work/etc" > "$work/backup.log" ||
  { cat "$work/backup.log" >&2; fail "stack.sh backup"; }
backup=$(sed -n 's/^backup: //p' "$work/backup.log")
[ -n "$backup" ] && [ -d "$backup" ] || fail "the backup folder is not named"
"$work/opt/bin/metagame" db integrity "$backup/undaunted.db" | grep -q "^ok, " || fail "the backup database"
for f in metagame.env deployserver.env gameserver.key tls/gateway-cert.pem tls/gateway-key.pem; do
  [ -f "$backup/secrets/$f" ] || fail "the backup has no $f"
done

for unit in "$kit"/systemd/*.service; do
  grep -q "@NODE@" "$unit" && fail "$(basename "$unit") still names Node"
  grep -q "^ExecStart=@ROOT@/bin/" "$unit" || fail "$(basename "$unit") does not start a binary from bin/"
done

# A binary that does not match SHA256SUMS is refused
mkdir "$work/tampered"
cp "$bins"/metagame "$bins"/gateway "$bins"/allowlist "$bins"/content "$bins"/deploy "$work/tampered/"
(cd "$work/tampered" && sha256sum metagame gateway allowlist content deploy > SHA256SUMS)
printf x >> "$work/tampered/content"
if sh "$kit/install.sh" --game-dir "$work/game" --my-ip 203.0.113.10 --install-root "$work/opt2" \
  --data-dir "$work/data2" --config-dir "$work/etc2" --binaries "$work/tampered" --no-services \
  --skip-game-prep > "$work/tampered.log" 2>&1; then
  fail "a tampered binary was installed"
fi
grep -q "content failed its SHA256SUMS check" "$work/tampered.log" || fail "no reason given for the refusal"

# The worker: the deploy binary in bin/, launched through ssh-worker-command.sh
sh "$kit/install-worker.sh" --game-dir "$work/game" --install-root "$work/worker" --env-file "$work/worker.env" \
  --wineprefix "$work/worker-data/prefix" --proton-data "$work/worker-data/proton" --wine /bin/true --xvfb 0 \
  --binaries "$bins" --no-system-changes --skip-game-prep > "$work/worker.log" 2>&1 || { cat "$work/worker.log" >&2; fail "worker install"; }
[ -x "$work/worker/bin/deploy" ] || fail "the worker has no deploy binary"
grep -q "^DR_WINE_BINARY=/bin/true$" "$work/worker.env" || fail "the worker settings"
if sh "$work/worker/deploy/linux-server/ssh-worker-command.sh" 'not*a*payload' 2>/dev/null; then
  fail "the worker command took a bad payload"
fi

echo "install test: ok"
