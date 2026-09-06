#!/usr/bin/env bash
# Single source of truth for the AppShelf version is Cargo.toml [package] version.
# scripts/version.sh                -> print the current version
# scripts/version.sh check          -> verify PKGBUILD and Cargo.lock agree with it
# scripts/version.sh set 0.2.0      -> rewrite every file that carries the version
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

current() {
  sed -n '/^\[package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' Cargo.toml | head -1
}

locked() {
  awk '/^name = "appshelf"$/{found=1; next} found && /^version = /{gsub(/[",]/,"",$3); print $3; exit}' Cargo.lock
}

pkgbuild() {
  sed -n 's/^pkgver=\(.*\)$/\1/p' PKGBUILD | head -1
}

case "${1-print}" in
print)
  current
  ;;
check)
  version="$(current)"
  status=0
  if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Cargo.toml version '$version' is not MAJOR.MINOR.PATCH" >&2
    status=1
  fi
  for pair in "PKGBUILD pkgver=$(pkgbuild)" "Cargo.lock $(locked)"; do
    file="${pair%% *}"
    found="${pair#* }"
    found="${found##*=}"
    if [[ $found != "$version" ]]; then
      echo "$file has version '$found' but Cargo.toml has '$version' — run scripts/version.sh set $version" >&2
      status=1
    fi
  done
  [[ $status -eq 0 ]] && echo "Version $version is consistent across Cargo.toml, Cargo.lock and PKGBUILD"
  exit $status
  ;;
set)
  version="${2-}"
  if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "usage: scripts/version.sh set MAJOR.MINOR.PATCH" >&2
    exit 1
  fi
  sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
  sed -i "s/^pkgver=.*/pkgver=$version/" PKGBUILD
  sed -i "s/^pkgrel=.*/pkgrel=1/" PKGBUILD
  cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace
  "$0" check
  ;;
*)
  echo "usage: scripts/version.sh [print|check|set VERSION]" >&2
  exit 1
  ;;
esac
