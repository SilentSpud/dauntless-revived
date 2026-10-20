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
  --no-services                  build/configure only; no root/systemd changes
  --skip-build                   do not run npm ci/build (for tests only)
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
    --no-services) no_services=1; shift ;;
    --skip-build) skip_build=1; shift ;;
    *) usage ;;
  esac
done

[ -n "$game_dir" ] && [ -n "$my_ip" ] || usage
case "$mode" in private|public) ;; *) usage ;; esac
case "$xvfb" in 0|1) ;; *) usage ;; esac
for p in "$source_root" "$game_dir"; do [ -d "$p" ] || { echo "not a directory: $p" >&2; exit 1; }; done
for c in node npm tar sha256sum; do command -v "$c" >/dev/null 2>&1 || { echo "missing command: $c" >&2; exit 1; }; done
node -e "const [a,b]=process.versions.node.split(\".\").map(Number); if(a<20 || (a===20 && b<19)) process.exit(1)" || { echo "Node.js 20.19+ is required" >&2; exit 1; }

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
  (cd "$source_root" && tar --exclude=.git --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" --exclude=./legacy -cf - .) | (cd "$install_root" && tar -xf -)
  if [ -d "$source_root/legacy" ]; then
    (cd "$source_root/legacy" && tar --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" --exclude="UndauntedLauncher/out" -cf - .) | (cd "$install_root" && tar -xf -)
  fi
fi
# The Node packages sit in legacy/ in the source; installed, they stay at the top level, where the
# services and tools expect them (an install into the checkout itself links them there).
if [ -d "$install_root/legacy" ]; then
  for dir in "$install_root"/legacy/*; do
    name=$(basename "$dir")
    [ -e "$install_root/$name" ] || ln -s "legacy/$name" "$install_root/$name"
  done
fi

chmod +x "$install_root/deploy/linux-server/launch-gameserver.mjs" "$install_root/deploy/linux-server/worker-launch.mjs" "$install_root/deploy/linux-server/prepare-game.sh" "$install_root/deploy/linux-server/allowlist.mjs" "$install_root/deploy/linux-server/stack.sh"

"$install_root/deploy/linux-server/prepare-game.sh" --game-dir "$game_dir" --repo "$install_root"

if [ "$skip_build" -eq 0 ]; then
  for pkg in UndauntedMetagame UndauntedGateway UndauntedContent UndauntedDeployServer; do
    echo "Building $pkg"
    (cd "$install_root/$pkg" && npm ci --no-audit --no-fund && npm run build)
  done
fi

set -- --platform linux --root "$install_root" --data "$data_dir" --config "$config_dir" --game-dir "$game_dir" --my-ip "$my_ip" --mode "$mode" --server-name "$server_name" --gateway-port "$gateway_port" --udp-begin "$udp_begin" --udp-end "$udp_end"
[ -n "$proton" ] && set -- "$@" --proton "$proton"
[ -n "$proton_data" ] && set -- "$@" --proton-data "$proton_data"
[ -n "$wine" ] && set -- "$@" --wine "$wine"
set -- "$@" --xvfb "$xvfb"
[ -n "$firewall_interface" ] && set -- "$@" --firewall-interface "$firewall_interface"
node "$install_root/deploy/unix-common/generate-config.mjs" "$@"

if [ "$mode" = public ]; then
  cert="$data_dir/tls/gateway-cert.pem"
  key="$data_dir/tls/gateway-key.pem"
  if [ ! -f "$cert" ] || [ ! -f "$key" ]; then
    (cd "$install_root/UndauntedGateway" && node tools/make-cert.js --host "$my_ip" --out "$data_dir/tls" --json >/dev/null)
  fi
fi

if [ -f "$install_root/UndauntedMetagame/dist/db.js" ]; then
  (cd "$install_root/UndauntedMetagame" && node --env-file="$config_dir/metagame.env" -e "require(\"./dist/db.js\").GetDb()")
  node "$install_root/deploy/unix-common/dr-db.cjs" gs-key "$install_root/UndauntedMetagame" "$data_dir/undaunted.db" "$data_dir/gameserver.key"
  invite_file="$data_dir/owner-invite-code.txt"
  if [ ! -f "$invite_file" ]; then
    node -e "process.stdout.write(require(\"crypto\").randomBytes(10).toString(\"hex\")+\"\\n\")" > "$invite_file"
    chmod 600 "$invite_file"
    invite=$(cat "$invite_file")
    node "$install_root/deploy/unix-common/dr-db.cjs" add-invite "$install_root/UndauntedMetagame" "$data_dir/undaunted.db" "$invite" >/dev/null
  fi
else
  echo "metagame build missing: database bootstrap skipped" >&2
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
    sed -e "s|@ROOT@|$install_root|g" -e "s|@CONFIG@|$config_dir|g" -e "s|@USER@|$service_user|g" -e "s|@NODE@|$(command -v node)|g" "$src" > "$dst"
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
  (cd "$install_root/UndauntedGateway" && node tools/make-cert.js --fingerprint "$data_dir/tls/gateway-cert.pem" | sed -n "/Fingerprint:/p")
fi
