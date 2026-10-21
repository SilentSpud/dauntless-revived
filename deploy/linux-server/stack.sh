#!/bin/sh
set -eu

usage() {
  echo "usage: $0 {start|stop|restart|status|logs} [component]" >&2
  echo "       $0 backup [--config-dir <dir>] [--dest <dir>]" >&2
  exit 2
}
[ "$#" -ge 1 ] || usage
action=$1

# backup (MIGRATION_PLAN.md M45): the database through metagame db backup (an online copy, safe while
# the metagame runs, checked after), and the settings, secrets and certificate beside it, in
# <dest>/<yyyy-mm-dd_HHMMSS>. Never a file copy of the database: that would miss what is still in the WAL.
if [ "$action" = backup ]; then
  shift
  config_dir=/etc/dauntless-revived
  dest=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --config-dir) [ "$#" -ge 2 ] || usage; config_dir=$2; shift 2 ;;
      --dest) [ "$#" -ge 2 ] || usage; dest=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
  metagame="$script_dir/../../bin/metagame"
  [ -x "$metagame" ] || { echo "no metagame binary at $metagame" >&2; exit 1; }
  env_file="$config_dir/metagame.env"
  [ -r "$env_file" ] || { echo "cannot read $env_file" >&2; exit 1; }
  db=$(sed -n 's/^DB_FILENAME=//p' "$env_file" | tail -n 1)
  db=${db#\"}; db=${db%\"}
  [ -n "$db" ] || { echo "$env_file names no DB_FILENAME" >&2; exit 1; }
  [ -n "$dest" ] || dest="$(dirname -- "$db")/backups"
  stamp=$(date +%Y-%m-%d_%H%M%S)
  out="$dest/$stamp"
  [ ! -e "$out" ] || { echo "$out exists" >&2; exit 1; }
  (umask 077 && mkdir -p "$out/secrets")
  if [ -f "$db" ]; then
    "$metagame" db backup "$db" "$out/undaunted.db" || { rm -rf "$out"; echo "backup FAILED: the database copy did not pass its check" >&2; exit 1; }
  else
    echo "no database yet"
  fi
  for f in "$config_dir"/*.env; do
    [ -f "$f" ] && cp -p "$f" "$out/secrets/"
  done
  data_dir=$(dirname -- "$db")
  for f in gameserver.key owner-invite-code.txt; do
    [ -f "$data_dir/$f" ] && cp -p "$data_dir/$f" "$out/secrets/"
  done
  if [ -f "$data_dir/tls/gateway-cert.pem" ] && [ -f "$data_dir/tls/gateway-key.pem" ]; then
    mkdir -p "$out/secrets/tls"
    cp -p "$data_dir/tls/gateway-cert.pem" "$data_dir/tls/gateway-key.pem" "$out/secrets/tls/"
  fi
  echo "backup: $out"
  exit 0
fi

component=${2:-all}
case "$component" in
  all) units="dauntless-metagame dauntless-deploy dauntless-content dauntless-allowlist dauntless-gateway" ;;
  metagame|deploy|content|allowlist|gateway) units="dauntless-$component" ;;
  *) usage ;;
esac

case "$action" in
  start|stop|restart) systemctl "$action" $units ;;
  status) systemctl --no-pager --full status $units ;;
  logs) journalctl -f -u $(printf "%s " $units | sed "s/ $//") ;;
  *) usage ;;
esac
