#!/bin/sh
set -eu

usage() {
  cat >&2 <<EOF
usage: $0 --game-dir <Dauntless folder> --my-ip <advertised IP> [options]
  --mode private|public          default private
  --source <repo>                source checkout; default repository containing this script
  --install-root <dir>           default /opt/dauntless-revived
  --data-dir <dir>               default /var/lib/dauntless-revived
  --config-dir <dir>             default /etc/dauntless-revived
  --service-user <user>          default dauntless
  --server-name <name>           default Dauntless Revived
  --gateway-port <port>          external/public port; default 443
  --udp-begin <port>             default 8770
  --udp-end <port>               default 8777
  --proton <path>                explicit Proton/GE-Proton script (preferred over Wine)
  --proton-data <dir>            Proton compat-data dir; default <data-dir>/proton
  --wine <path>                  explicit wine/wine64
  --xvfb 0|1                    use xvfb-run when no DISPLAY; default 1
  --firewall-interface <name>    nftables input interface; optional
  --binaries <dir>               folder with the release binaries (metagame, gateway, allowlist,
                                 content, deploy), e.g. a CI server-binaries download; checked
                                 against its SHA256SUMS when it has one. Default: cargo build
                                 --release in the source checkout
  --no-services                  build/configure only; no root/systemd changes
  --skip-build                   use the binaries already in <install-root>/bin (for tests only)
  --skip-game-prep               do not check the game build or copy the DLLs (for tests only)
EOF
  exit 2
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
install_root=/opt/dauntless-revived
data_dir=/var/lib/dauntless-revived
config_dir=/etc/dauntless-revived
service_user=dauntless
mode=private
server_name="Dauntless Revived"
gateway_port=443
udp_begin=8770
udp_end=8777
game_dir=
my_ip=
proton=
proton_data=
wine=
xvfb=1
firewall_interface=
no_services=0
skip_build=0
skip_game_prep=0
binaries=
bins="metagame gateway allowlist content deploy"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --game-dir) game_dir=$2; shift 2 ;;
    --my-ip) my_ip=$2; shift 2 ;;
    --mode) mode=$2; shift 2 ;;
    --source) source_root=$2; shift 2 ;;
    --install-root) install_root=$2; shift 2 ;;
    --data-dir) data_dir=$2; shift 2 ;;
    --config-dir) config_dir=$2; shift 2 ;;
    --service-user) service_user=$2; shift 2 ;;
    --server-name) server_name=$2; shift 2 ;;
    --gateway-port) gateway_port=$2; shift 2 ;;
    --udp-begin) udp_begin=$2; shift 2 ;;
    --udp-end) udp_end=$2; shift 2 ;;
    --proton) proton=$2; shift 2 ;;
    --proton-data) proton_data=$2; shift 2 ;;
    --wine) wine=$2; shift 2 ;;
    --xvfb) xvfb=$2; shift 2 ;;
    --firewall-interface) firewall_interface=$2; shift 2 ;;
    --binaries) binaries=$2; shift 2 ;;
    --no-services) no_services=1; shift ;;
    --skip-build) skip_build=1; shift ;;
    --skip-game-prep) skip_game_prep=1; shift ;;
    *) usage ;;
  esac
done

[ -n "$game_dir" ] && [ -n "$my_ip" ] || usage
case "$mode" in private|public) ;; *) usage ;; esac
case "$xvfb" in 0|1) ;; *) usage ;; esac
for p in "$source_root" "$game_dir"; do [ -d "$p" ] || { echo "not a directory: $p" >&2; exit 1; }; done
for c in tar sha256sum od; do command -v "$c" >/dev/null 2>&1 || { echo "missing command: $c" >&2; exit 1; }; done
if [ -n "$binaries" ]; then
  [ -d "$binaries" ] || { echo "not a directory: $binaries" >&2; exit 1; }
  binaries=$(CDPATH= cd -- "$binaries" && pwd)
elif [ "$skip_build" -eq 0 ]; then
  command -v cargo >/dev/null 2>&1 || { echo "cargo is required to build the services; or pass --binaries <dir>" >&2; exit 1; }
fi

if [ "$no_services" -eq 0 ]; then
  [ "$(id -u)" -eq 0 ] || { echo "normal install must run as root; use --no-services for an unprivileged test" >&2; exit 1; }
  command -v systemctl >/dev/null 2>&1 || { echo "systemd/systemctl is required for the service install" >&2; exit 1; }
  if ! id "$service_user" >/dev/null 2>&1; then
    useradd --system --create-home --home-dir "$data_dir" --shell /usr/sbin/nologin "$service_user"
  fi
fi

source_root=$(CDPATH= cd -- "$source_root" && pwd)
game_dir=$(CDPATH= cd -- "$game_dir" && pwd)
mkdir -p "$install_root" "$data_dir" "$config_dir"

if [ "$source_root" != "$install_root" ]; then
  echo "Copying server source to $install_root"
  (cd "$source_root" && tar --exclude=.git --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" --exclude=./legacy --exclude=./rust -cf - .) | (cd "$install_root" && tar -xf -)
  if [ -d "$source_root/legacy" ]; then
    (cd "$source_root/legacy" && tar --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" --exclude="UndauntedLauncher/out" -cf - .) | (cd "$install_root" && tar -xf -)
  fi
fi
# The service folders sit in legacy/ in the source; installed, they stay at the top level, where the
# services run (the metagame's store images, the launcher's pinned DLLs). An install into the
# checkout itself links them there.
if [ -d "$install_root/legacy" ]; then
  for dir in "$install_root"/legacy/*; do
    name=$(basename "$dir")
    [ -e "$install_root/$name" ] || ln -s "legacy/$name" "$install_root/$name"
  done
fi

chmod +x "$install_root/deploy/linux-server/prepare-game.sh" "$install_root/deploy/linux-server/stack.sh"

if [ "$skip_game_prep" -eq 0 ]; then
  "$install_root/deploy/linux-server/prepare-game.sh" --game-dir "$game_dir" --repo "$install_root"
fi

# The services are the Rust binaries in <install-root>/bin (MIGRATION_PLAN.md M39). Each is replaced by
# a rename, so a running service keeps its old file until it restarts.
bin_dir="$install_root/bin"
bin_source=
if [ -n "$binaries" ]; then
  if [ -f "$binaries/SHA256SUMS" ]; then
    for name in $bins; do
      line=$(awk -v n="$name" '$2 == n || $2 == "*" n' "$binaries/SHA256SUMS")
      [ -n "$line" ] || { echo "$binaries/SHA256SUMS does not list $name" >&2; exit 1; }
      expected=${line%% *}
      [ "$(sha256sum "$binaries/$name" | awk '{print $1}')" = "$expected" ] || { echo "$name failed its SHA256SUMS check" >&2; exit 1; }
    done
  fi
  bin_source=$binaries
elif [ "$skip_build" -eq 0 ]; then
  echo "Building the services (cargo build --release)"
  (cd "$source_root/rust" && cargo build --release --locked --bins -p metagame -p gateway -p content -p deploy)
  bin_source="$source_root/rust/target/release"
fi
mkdir -p "$bin_dir"
if [ -n "$bin_source" ]; then
  for name in $bins; do
    [ -f "$bin_source/$name" ] || { echo "missing binary: $bin_source/$name" >&2; exit 1; }
    cp "$bin_source/$name" "$bin_dir/$name.new"
    chmod 755 "$bin_dir/$name.new"
    mv -f "$bin_dir/$name.new" "$bin_dir/$name"
  done
fi
for name in $bins; do
  [ -x "$bin_dir/$name" ] || { echo "missing binary: $bin_dir/$name" >&2; exit 1; }
done
# The deploy server starts game servers through dr-run-game: deploy itself under that name
ln -sf deploy "$bin_dir/dr-run-game"

set -- --platform linux --root "$install_root" --bin-dir "$bin_dir" --data "$data_dir" --config "$config_dir" --game-dir "$game_dir" --my-ip "$my_ip" --mode "$mode" --server-name "$server_name" --gateway-port "$gateway_port" --udp-begin "$udp_begin" --udp-end "$udp_end"
[ -n "$proton" ] && set -- "$@" --proton "$proton"
[ -n "$proton_data" ] && set -- "$@" --proton-data "$proton_data"
[ -n "$wine" ] && set -- "$@" --wine "$wine"
set -- "$@" --xvfb "$xvfb"
[ -n "$firewall_interface" ] && set -- "$@" --firewall-interface "$firewall_interface"
"$bin_dir/deploy" generate-config "$@"

if [ "$mode" = public ]; then
  cert="$data_dir/tls/gateway-cert.pem"
  key="$data_dir/tls/gateway-key.pem"
  if [ ! -f "$cert" ] || [ ! -f "$key" ]; then
    "$bin_dir/gateway" make-cert --host "$my_ip" --out "$data_dir/tls" --json >/dev/null
  fi
fi

# The database: created or adopted and migrated, then the game-server key and the owner's invite
(cd "$install_root/UndauntedMetagame" && "$bin_dir/metagame" migrate --env-file "$config_dir/metagame.env" >/dev/null)
"$bin_dir/metagame" db gs-key "$data_dir/undaunted.db" "$data_dir/gameserver.key" >/dev/null
invite_file="$data_dir/owner-invite-code.txt"
if [ ! -f "$invite_file" ]; then
  (umask 077 && od -An -N10 -tx1 /dev/urandom | tr -d ' \n' > "$invite_file" && echo >> "$invite_file")
  invite=$(cat "$invite_file")
  "$bin_dir/metagame" db add-invite "$data_dir/undaunted.db" "$invite" >/dev/null
fi

if [ "$no_services" -eq 0 ]; then
  group=$(id -gn "$service_user")
  chown -R "$service_user:$group" "$data_dir"
  chown -R root:"$group" "$config_dir"
  chmod 750 "$config_dir"
  find "$config_dir" -type f -exec chmod 640 {} \;
  units="dauntless-metagame dauntless-deploy dauntless-content"
  [ "$mode" = public ] && units="$units dauntless-allowlist dauntless-gateway"
  for unit in $units; do
    src="$install_root/deploy/linux-server/systemd/$unit.service"
    dst="/etc/systemd/system/$unit.service"
    sed -e "s|@ROOT@|$install_root|g" -e "s|@CONFIG@|$config_dir|g" -e "s|@USER@|$service_user|g" "$src" > "$dst"
  done
  systemctl daemon-reload
  systemctl enable --now $units
fi

echo "Linux server port configured."
echo "Mode: $mode"
echo "Game: $game_dir"
echo "Config: $config_dir"
[ -f "$data_dir/owner-invite-code.txt" ] && echo "Owner bootstrap invite code is stored in $data_dir/owner-invite-code.txt (not printed)."
if [ "$mode" = public ] && [ -f "$data_dir/tls/gateway-cert.pem" ]; then
  "$bin_dir/gateway" make-cert --fingerprint "$data_dir/tls/gateway-cert.pem" | sed -n "/Fingerprint:/p"
fi
