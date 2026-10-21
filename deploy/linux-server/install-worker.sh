#!/bin/sh
set -eu

usage() {
  cat >&2 <<EOF
usage: $0 --game-dir <Dauntless folder> [options]
  --source <repo>               source checkout; default repository containing this script
  --install-root <dir>          default /opt/dauntless-revived
  --worker-user <user>          default dauntless
  --env-file <file>             default /etc/dauntless-revived-worker.env
  --command-link <path>          default /usr/local/bin/dr-game-worker
  --proton <path>               explicit Proton/GE-Proton script (preferred)
  --proton-data <dir>           default /var/lib/dauntless-revived-worker/proton
  --wine <path>                 explicit wine/wine64 path
  --wineprefix <dir>            default /var/lib/dauntless-revived-worker/wineprefix
  --xvfb 0|1                    default 1
  --binaries <dir>              folder with the release deploy binary (checked against its
                                SHA256SUMS when it has one); default: cargo build --release
  --no-system-changes           do not create users or write /etc (test mode)
  --skip-game-prep              do not check the game build or copy the DLLs (for tests only)
EOF
  exit 2
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
install_root=/opt/dauntless-revived
worker_user=dauntless
env_file=/etc/dauntless-revived-worker.env
command_link=/usr/local/bin/dr-game-worker
wineprefix=/var/lib/dauntless-revived-worker/wineprefix
proton_data=/var/lib/dauntless-revived-worker/proton
game_dir=
proton=
wine=
xvfb=1
no_system=0
skip_game_prep=0
binaries=

while [ "$#" -gt 0 ]; do
  case "$1" in
    --game-dir) game_dir=$2; shift 2 ;;
    --source) source_root=$2; shift 2 ;;
    --install-root) install_root=$2; shift 2 ;;
    --worker-user) worker_user=$2; shift 2 ;;
    --env-file) env_file=$2; shift 2 ;;
    --command-link) command_link=$2; shift 2 ;;
    --proton) proton=$2; shift 2 ;;
    --proton-data) proton_data=$2; shift 2 ;;
    --wine) wine=$2; shift 2 ;;
    --wineprefix) wineprefix=$2; shift 2 ;;
    --xvfb) xvfb=$2; shift 2 ;;
    --binaries) binaries=$2; shift 2 ;;
    --no-system-changes) no_system=1; shift ;;
    --skip-game-prep) skip_game_prep=1; shift ;;
    *) usage ;;
  esac
done
[ -n "$game_dir" ] || usage
case "$xvfb" in 0|1) ;; *) usage ;; esac

command -v sha256sum >/dev/null 2>&1 || { echo "missing command: sha256sum" >&2; exit 1; }
if [ -n "$binaries" ]; then
  [ -f "$binaries/deploy" ] || { echo "no deploy binary in $binaries" >&2; exit 1; }
  binaries=$(CDPATH= cd -- "$binaries" && pwd)
else
  command -v cargo >/dev/null 2>&1 || { echo "cargo is required to build the deploy binary; or pass --binaries <dir>" >&2; exit 1; }
fi

if [ -n "$proton" ]; then
  [ -x "$proton" ] || { echo "Proton is not executable: $proton" >&2; exit 1; }
else
  if [ -z "$wine" ]; then wine=$(command -v wine64 2>/dev/null || command -v wine 2>/dev/null || true); fi
  [ -n "$wine" ] || { echo "Proton or Wine is required (use --proton/--wine, or install wine64/wine)." >&2; exit 1; }
  [ -x "$wine" ] || { echo "Wine is not executable: $wine" >&2; exit 1; }
fi
if [ "$xvfb" = 1 ] && ! command -v xvfb-run >/dev/null 2>&1; then
  echo "xvfb-run is required for a headless worker; install Xvfb or use --xvfb 0 with a real DISPLAY." >&2
  exit 1
fi

source_root=$(CDPATH= cd -- "$source_root" && pwd)
game_dir=$(CDPATH= cd -- "$game_dir" && pwd)
if [ "$source_root" != "$install_root" ]; then
  mkdir -p "$install_root"
  (cd "$source_root" && tar --exclude=.git --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" -cf - deploy) | (cd "$install_root" && tar -xf -)
  # The launcher's pinned DLLs: legacy/UndauntedLauncher/assets in the source, UndauntedLauncher/assets installed
  assets_root=$source_root
  [ -d "$source_root/legacy/UndauntedLauncher/assets" ] && assets_root=$source_root/legacy
  (cd "$assets_root" && tar -cf - UndauntedLauncher/assets) | (cd "$install_root" && tar -xf -)
fi
chmod +x "$install_root/deploy/linux-server/ssh-worker-command.sh" "$install_root/deploy/linux-server/prepare-game.sh"

# The game launch is `deploy worker <payload>` (MIGRATION_PLAN.md M39), in <install-root>/bin
if [ -n "$binaries" ]; then
  if [ -f "$binaries/SHA256SUMS" ]; then
    expected=$(awk '$2 == "deploy" || $2 == "*deploy" {print $1}' "$binaries/SHA256SUMS")
    [ -n "$expected" ] && [ "$(sha256sum "$binaries/deploy" | awk '{print $1}')" = "$expected" ] || { echo "deploy failed its SHA256SUMS check" >&2; exit 1; }
  fi
  deploy_bin="$binaries/deploy"
else
  (cd "$source_root/rust" && cargo build --release --locked --bin deploy -p deploy)
  deploy_bin="$source_root/rust/target/release/deploy"
fi
mkdir -p "$install_root/bin"
cp "$deploy_bin" "$install_root/bin/deploy.new"
chmod 755 "$install_root/bin/deploy.new"
mv -f "$install_root/bin/deploy.new" "$install_root/bin/deploy"
if [ "$skip_game_prep" -eq 0 ]; then
  "$install_root/deploy/linux-server/prepare-game.sh" --game-dir "$game_dir" --repo "$install_root"
fi

game_exe="$game_dir/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe"
if [ "$no_system" -eq 0 ]; then
  [ "$(id -u)" -eq 0 ] || { echo "run as root, or use --no-system-changes for validation" >&2; exit 1; }
  if ! id "$worker_user" >/dev/null 2>&1; then
    useradd --system --create-home --home-dir "$(dirname "$wineprefix")" --shell /usr/sbin/nologin "$worker_user"
  fi
  mkdir -p "$(dirname "$wineprefix")" "$proton_data" "$(dirname "$env_file")"
  chown -R "$worker_user:$(id -gn "$worker_user")" "$(dirname "$wineprefix")"
else
  mkdir -p "$(dirname "$wineprefix")" "$proton_data" "$(dirname "$env_file")"
fi

cat > "$env_file.tmp" <<EOF
DR_GAME_EXE=$game_exe
DR_PROTON_BINARY=$proton
DR_PROTON_COMPAT_DATA=$proton_data
DR_WINE_BINARY=$wine
DR_WINEPREFIX=$wineprefix
DR_XVFB=$xvfb
WINEDEBUG=-all
WINEDLLOVERRIDES=dxgi=n,b
EOF
mv "$env_file.tmp" "$env_file"
chmod 600 "$env_file"
if [ "$no_system" -eq 0 ]; then
  chown root:"$(id -gn "$worker_user")" "$env_file"; chmod 640 "$env_file"
  mkdir -p "$(dirname "$command_link")"
  ln -sf "$install_root/deploy/linux-server/ssh-worker-command.sh" "$command_link"
fi

echo "Linux game worker configured."
echo "Worker launcher: $install_root/bin/deploy worker"
echo "Environment: $env_file"
echo "Game: $game_dir"
if [ -n "$proton" ]; then echo "Proton: $proton"; else echo "Wine: $wine"; fi
echo "Remote command: $command_link"
echo "Recommended authorized_keys prefix:"
echo "restrict,command=\"$install_root/deploy/linux-server/ssh-worker-command.sh\" <your-worker-public-key>"
