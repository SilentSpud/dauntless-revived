#!/usr/bin/env bash
# Builds the server release archives (MIGRATION_PLAN.md M38) from the binaries of each server platform; run by
# .github/workflows/server-release.yml, and locally to try it (zip is needed for the Windows archive).
#
#   package-server.sh <version> <commit> <binaries> <out>
#
# <binaries> holds server-binaries-<target>/ for the three D14 targets, each with the six service binaries and
# their SHA256SUMS, as the release build jobs upload them. Writes to <out>:
#   DauntlessRevivedServer-<version>-windows-x64.zip, -linux-x64.tar.gz, -linux-arm64.tar.gz
#     each dauntless-revived-server-<version>/: `git archive <commit>`, COMMIT (the kits read it where there is
#     no .git), and server-binaries/ (the binaries and SHA256SUMS, what the kits take and check)
#   SHA256SUMS.txt          the archives
#   BINARIES-SHA256SUMS.txt every binary, as <target>/<name>
# The archives are reproducible: files sorted, owner 0, every time set to the commit's.
set -euo pipefail

fail() { echo "package-server: $*" >&2; exit 1; }
[ "$#" -eq 4 ] || fail "usage: package-server.sh <version> <commit> <binaries> <out>"
version=$1 commit=$2 binaries=$3 out=$4
[[ $version =~ ^[0-9A-Za-z.-]+$ ]] || fail "bad version: $version"
[[ $commit =~ ^[0-9a-f]{40}$ ]] || fail "the commit must be a full hash: $commit"
[ -d "$binaries" ] || fail "no binaries folder: $binaries"
command -v zip > /dev/null || fail "zip is needed for the Windows archive"

root=$(git rev-parse --show-toplevel)
epoch=$(git -C "$root" show -s --format=%ct "$commit")
mkdir -p "$out"
out=$(cd "$out" && pwd)
binaries=$(cd "$binaries" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

names=(metagame gateway allowlist content deploy discord-bot)
top="dauntless-revived-server-$version"
: > "$work/binaries.sums"

for target in x86_64-pc-windows-msvc x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  case "$target" in
    x86_64-pc-windows-msvc) platform=windows-x64 ext=.exe ;;
    x86_64-unknown-linux-gnu) platform=linux-x64 ext= ;;
    aarch64-unknown-linux-gnu) platform=linux-arm64 ext= ;;
  esac
  src="$binaries/server-binaries-$target"
  [ -f "$src/SHA256SUMS" ] || fail "$src/SHA256SUMS is missing"
  # The sums the build job wrote must hold, and list exactly the six binaries
  (cd "$src" && sha256sum --check --strict --quiet SHA256SUMS) || fail "$target: a binary does not match its SHA256SUMS"
  listed=$(awk 'NF { sub(/^\*/, "", $2); print $2 }' "$src/SHA256SUMS" | sort)
  wanted=$(for name in "${names[@]}"; do echo "$name$ext"; done | sort)
  [ "$listed" = "$wanted" ] || fail "$target: SHA256SUMS lists $(tr '\n' ' ' <<< "$listed")instead of $(tr '\n' ' ' <<< "$wanted")"

  stage="$work/$target"
  dir="$stage/$top"
  mkdir -p "$dir"
  git -C "$root" archive --format=tar "$commit" | tar -x -C "$dir"
  printf '%s\n' "$commit" > "$dir/COMMIT"
  mkdir "$dir/server-binaries"
  for name in "${names[@]}"; do
    install -m 755 "$src/$name$ext" "$dir/server-binaries/$name$ext"
  done
  awk 'NF { sub(/^\*/, "", $2); print $1 "  " $2 }' "$src/SHA256SUMS" | sort -k2,2 > "$dir/server-binaries/SHA256SUMS"
  awk -v t="$target" '{ print $1 "  " t "/" $2 }' "$dir/server-binaries/SHA256SUMS" >> "$work/binaries.sums"

  find "$dir" -exec touch -h -d "@$epoch" {} +
  file="DauntlessRevivedServer-$version-$platform"
  if [ "$ext" = .exe ]; then
    (cd "$stage" && find "$top" -print | LC_ALL=C sort | zip -X -q -9 "$out/$file.zip" -@)
  else
    tar -C "$stage" --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$epoch" -cf - "$top" | gzip -n -9 > "$out/$file.tar.gz"
  fi
  echo "$file: $(du -h "$out/$file".* | cut -f1)"
done

sort -k2,2 "$work/binaries.sums" > "$out/BINARIES-SHA256SUMS.txt"
(cd "$out" && sha256sum -- DauntlessRevivedServer-* > SHA256SUMS.txt && cat SHA256SUMS.txt)
