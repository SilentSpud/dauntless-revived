#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
payload=

if [ "$#" -eq 1 ]; then
  payload=$1
elif [ "$#" -eq 0 ] && [ -n "${SSH_ORIGINAL_COMMAND:-}" ]; then
  case "$SSH_ORIGINAL_COMMAND" in
    "dr-game-worker "*)
      payload=${SSH_ORIGINAL_COMMAND#dr-game-worker }
      ;;
    *)
      echo "dauntless worker: refused SSH command" >&2
      exit 126
      ;;
  esac
else
  echo "dauntless worker: one encoded payload is required" >&2
  exit 2
fi

case "$payload" in
  ""|*[!A-Za-z0-9_-]*)
    echo "dauntless worker: invalid payload" >&2
    exit 2
    ;;
esac
[ "${#payload}" -le 65536 ] || { echo "dauntless worker: payload too large" >&2; exit 2; }

exec "$script_dir/../../bin/deploy" worker "$payload"
