#!/bin/ksh
set -eu

usage() {
  cat >&2 <<EOF
usage: $0 --game-dir <Dauntless folder> --my-ip <advertised IP> --worker-host <host> --worker-ip <IPv4> --worker-key <SSH private key> --interface <pf interface> --worker-interface <pf interface> [options]
  --mode private|public          default public
  --worker-user <user>           default dauntless
  --worker-command <command>     remote command; default dr-game-worker
  --source <repo>                source checkout; default repository containing this script
  --install-root <dir>           default /opt/dauntless-revived
  --data-dir <dir>               default /var/dauntless-revived
  --config-dir <dir>             default /etc/dauntless-revived
  --service-user <user>          default _dauntless
  --server-name <name>           default Dauntless Revived
  --gateway-port <port>          default 443
  --udp-begin <port>             default 8770
  --udp-end <port>               default 8777
  --configure-pf                 install PF anchor/rules and enable IPv4 forwarding
  --no-services                  build/configure only; permits validation on another Unix host
  --skip-build                   do not npm ci/build (tests only)
EOF
  exit 2
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
install_root=/opt/dauntless-revived
data_dir=/var/dauntless-revived
config_dir=/etc/dauntless-revived
service_user=_dauntless
mode=public
worker_user=dauntless
worker_command=dr-game-worker
server_name="Dauntless Revived"
gateway_port=443
udp_begin=8770
udp_end=8777
game_dir=
my_ip=
worker_host=
worker_ip=
worker_key=
interface=
worker_interface=
configure_pf=0
no_services=0
skip_build=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --game-dir) game_dir=$2; shift 2 ;;
    --my-ip) my_ip=$2; shift 2 ;;
    --mode) mode=$2; shift 2 ;;
    --worker-host) worker_host=$2; shift 2 ;;
    --worker-ip) worker_ip=$2; shift 2 ;;
    --worker-key) worker_key=$2; shift 2 ;;
    --worker-user) worker_user=$2; shift 2 ;;
    --worker-command) worker_command=$2; shift 2 ;;
    --interface) interface=$2; shift 2 ;;
    --worker-interface) worker_interface=$2; shift 2 ;;
    --source) source_root=$2; shift 2 ;;
    --install-root) install_root=$2; shift 2 ;;
    --data-dir) data_dir=$2; shift 2 ;;
    --config-dir) config_dir=$2; shift 2 ;;
    --service-user) service_user=$2; shift 2 ;;
    --server-name) server_name=$2; shift 2 ;;
    --gateway-port) gateway_port=$2; shift 2 ;;
    --udp-begin) udp_begin=$2; shift 2 ;;
    --udp-end) udp_end=$2; shift 2 ;;
    --configure-pf) configure_pf=1; shift ;;
    --no-services) no_services=1; shift ;;
    --skip-build) skip_build=1; shift ;;
    *) usage ;;
  esac
done

for value in "$game_dir" "$my_ip" "$worker_host" "$worker_ip" "$worker_key" "$interface" "$worker_interface"; do [ -n "$value" ] || usage; done
case "$mode" in public|private) ;; *) usage ;; esac
[ -d "$game_dir" ] || { echo "game directory not found: $game_dir" >&2; exit 1; }
[ -f "$worker_key" ] || { echo "worker SSH key not found: $worker_key" >&2; exit 1; }
if [ "$no_services" -eq 0 ]; then
  [ "$(uname -s)" = OpenBSD ] || { echo "live install must run on OpenBSD" >&2; exit 1; }
  [ "$(id -u)" -eq 0 ] || { echo "live install must run as root" >&2; exit 1; }
  [ "$configure_pf" -eq 1 ] || { echo "live OpenBSD install requires explicit --configure-pf" >&2; exit 1; }
fi
for c in node npm tar git ssh; do command -v "$c" >/dev/null 2>&1 || { echo "missing command: $c" >&2; exit 1; }; done
node -e "const [a,b]=process.versions.node.split(\".\").map(Number); if(a<20 || (a===20 && b<19)) process.exit(1)" || { echo "Node.js 20.19+ is required" >&2; exit 1; }

source_root=$(CDPATH= cd -- "$source_root" && pwd)
game_dir=$(CDPATH= cd -- "$game_dir" && pwd)
worker_key=$(CDPATH= cd -- "$(dirname "$worker_key")" && pwd)/$(basename "$worker_key")
mkdir -p "$install_root" "$data_dir" "$config_dir" "$data_dir/keys"
worker_key_installed="$data_dir/keys/worker-ssh"
if [ "$worker_key" != "$worker_key_installed" ]; then cp "$worker_key" "$worker_key_installed"; fi
chmod 600 "$worker_key_installed"
if [ "$source_root" != "$install_root" ]; then
  (cd "$source_root" && tar --exclude=.git --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" --exclude=./legacy -cf - .) | (cd "$install_root" && tar -xf -)
  if [ -d "$source_root/legacy" ]; then
    (cd "$source_root/legacy" && tar --exclude="*/node_modules" --exclude="*/dist" --exclude="*/build" -cf - .) | (cd "$install_root" && tar -xf -)
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
chmod +x "$install_root/deploy/openbsd-server/launch-gameserver.mjs" "$install_root/deploy/openbsd-server/allowlist.mjs" "$install_root/deploy/openbsd-server/stack.ksh" "$install_root/deploy/openbsd-server/configure-pf.ksh"

pin=$(node -e "const fs=require(\"fs\"),c=require(\"crypto\");const p=process.argv[1];console.log(c.createHash(\"sha256\").update(fs.readFileSync(p)).digest(\"hex\"))" "$game_dir/Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe")
[ "$pin" = d3d41e614908d2befd518b27046d9822d6130ef12ba3504babbdb786bef9cff4 ] || { echo "local content game is not the pinned 1.4.4 build" >&2; exit 1; }

if [ "$skip_build" -eq 0 ]; then
  for pkg in UndauntedMetagame UndauntedGateway UndauntedContent UndauntedDeployServer; do
    echo "Building $pkg"
    (cd "$install_root/$pkg" && npm ci --no-audit --no-fund && npm run build)
  done
fi

node "$install_root/deploy/unix-common/generate-config.mjs" --platform openbsd --root "$install_root" --data "$data_dir" --config "$config_dir" --game-dir "$game_dir" --my-ip "$my_ip" --mode "$mode" --server-name "$server_name" --gateway-port "$gateway_port" --udp-begin "$udp_begin" --udp-end "$udp_end" --worker-host "$worker_host" --worker-user "$worker_user" --worker-key "$worker_key_installed" --worker-command "$worker_command"

if [ "$mode" = public ]; then
  cert="$data_dir/tls/gateway-cert.pem"; key="$data_dir/tls/gateway-key.pem"
  if [ ! -f "$cert" ] || [ ! -f "$key" ]; then (cd "$install_root/UndauntedGateway" && node tools/make-cert.js --host "$my_ip" --out "$data_dir/tls" --json >/dev/null); fi
fi

if [ -f "$install_root/UndauntedMetagame/dist/db.js" ]; then
  (cd "$install_root/UndauntedMetagame" && node --env-file="$config_dir/metagame.env" -e "require(\"./dist/db.js\").GetDb()")
  node "$install_root/deploy/unix-common/dr-db.cjs" gs-key "$install_root/UndauntedMetagame" "$data_dir/undaunted.db" "$data_dir/gameserver.key"
  invite_file="$data_dir/owner-invite-code.txt"
  if [ ! -f "$invite_file" ]; then
    node -e "process.stdout.write(require(\"crypto\").randomBytes(10).toString(\"hex\")+\"\\n\")" > "$invite_file"; chmod 600 "$invite_file"
    node "$install_root/deploy/unix-common/dr-db.cjs" add-invite "$install_root/UndauntedMetagame" "$data_dir/undaunted.db" "$(cat "$invite_file")" >/dev/null
  fi
fi

if [ "$configure_pf" -eq 1 ]; then
  "$install_root/deploy/openbsd-server/configure-pf.ksh" --mode "$mode" --interface "$interface" --worker-interface "$worker_interface" --worker-ip "$worker_ip" --gateway-port "$gateway_port" --udp-begin "$udp_begin" --udp-end "$udp_end" --install
fi

if [ "$no_services" -eq 0 ]; then
  if ! id "$service_user" >/dev/null 2>&1; then useradd -m -d "$data_dir" -s /sbin/nologin "$service_user"; fi
  group=$(id -gn "$service_user")
  chown -R "$service_user:$group" "$data_dir"
  chown -R root:"$group" "$config_dir"; chmod 750 "$config_dir"; find "$config_dir" -type f -exec chmod 640 {} \;
  node_bin=$(command -v node)
  daemons="dauntless_metagame dauntless_deploy dauntless_content"
  [ "$mode" = public ] && daemons="$daemons dauntless_allowlist dauntless_gateway"
  for daemon in $daemons; do
    src="$install_root/deploy/openbsd-server/rc.d/$daemon"
    dst="/etc/rc.d/$daemon"
    sed -e "s|@ROOT@|$install_root|g" -e "s|@CONFIG@|$config_dir|g" -e "s|@USER@|$service_user|g" -e "s|@NODE@|$node_bin|g" "$src" > "$dst"
    chmod 555 "$dst"; chown root:wheel "$dst"
    rcctl enable "$daemon"
  done
  rcctl order $daemons
  rcctl start $daemons
fi

echo "OpenBSD control-plane port configured."
echo "Game worker: $worker_user@$worker_host ($worker_ip)"
echo "PF external interface: $interface"
echo "PF worker interface: $worker_interface"
echo "Config: $config_dir"
echo "The game process itself runs on the Linux/Wine worker; OpenBSD runs the Node control plane natively."
if [ "$mode" = public ] && [ -f "$data_dir/tls/gateway-cert.pem" ]; then (cd "$install_root/UndauntedGateway" && node tools/make-cert.js --fingerprint "$data_dir/tls/gateway-cert.pem" | sed -n "/Fingerprint:/p"); fi
