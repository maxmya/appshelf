#!/usr/bin/env bash
# Package the release build as an AppImage, using the same vendored uruntime
# that AppShelf uses for the applications it manages.
#
#   scripts/build-appimage.sh [output-directory]
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

arch="$(uname -m)"
version="$(scripts/version.sh)"
outdir="${1:-dist}"
runtime="vendor/uruntime-$arch"
output="$outdir/AppShelf-$version-$arch.AppImage"

[ -x target/release/appshelf ] || { echo "build first: cargo build --release --locked" >&2; exit 1; }
[ -f "$runtime" ] || { echo "missing $runtime: run target/release/appshelf --fetch-runtime" >&2; exit 1; }

appdir="$(mktemp -d)"
trap 'rm -rf "$appdir"' EXIT

# Mirror the layout resources() expects: binary beside ui/, vendor/, packaging/.
install -m755 target/release/appshelf "$appdir/appshelf"
cp -r ui vendor packaging "$appdir/"
cp LICENSE README.md "$appdir/"
rm -rf "$appdir/ui/Commons"

install -Dm644 packaging/org.omarchy.appshelf.desktop "$appdir/org.omarchy.appshelf.desktop"
install -Dm644 packaging/org.omarchy.appshelf.svg "$appdir/org.omarchy.appshelf.svg"
ln -sf org.omarchy.appshelf.svg "$appdir/.DirIcon"

cat > "$appdir/AppRun" <<'RUN'
#!/bin/sh
# uruntime mounts the payload read-only; appshelf stages its QML tree into
# XDG_RUNTIME_DIR when it detects that, so no writes land here.
exec "$(dirname "$(readlink -f "$0")")/appshelf" "$@"
RUN
chmod 755 "$appdir/AppRun"

mkdir -p "$outdir"
payload="$(mktemp -u)"
"./$runtime" --appimage-mksquashfs "$appdir" "$payload" -noappend -root-owned -comp zstd

# Point AppShelf's own update checker at this repository's releases. uruntime
# reserves a fixed-size .upd_info section, so patch it in place rather than
# rewriting the ELF and disturbing anything else.
read -r offset size <<<"$(readelf -S -W "$runtime" | awk '$2==".upd_info"{print strtonum("0x"$5), strtonum("0x"$6)}')"
[ -n "${offset:-}" ] && [ "${size:-0}" -gt 0 ] || { echo "no .upd_info section in $runtime" >&2; exit 1; }
info="gh-releases-zsync|maxmya|appshelf|latest|AppShelf-*-$arch.AppImage.zsync"
[ "${#info}" -lt "$size" ] || { echo ".upd_info too long for the reserved section" >&2; exit 1; }

cp "$runtime" "$output"
chmod 755 "$output"
printf '%s' "$info" | dd of="$output" bs=1 seek="$offset" conv=notrunc status=none
# Zero the remainder so a shorter string never leaves a previous tail behind.
dd if=/dev/zero of="$output" bs=1 seek="$((offset + ${#info}))" count="$((size - ${#info}))" conv=notrunc status=none

cat "$payload" >> "$output"
rm -f "$payload"
# The .upd_info pattern names this zsync file; update.rs reads its header for
# the sha1 and length that decide whether a download is needed at all.
base="$(basename "$output")"
if [ -n "${APPSHELF_SKIP_ZSYNC:-}" ]; then
  echo "APPSHELF_SKIP_ZSYNC set: not generating $base.zsync (releases must ship it)" >&2
else
  command -v zsyncmake >/dev/null || { echo "zsyncmake is required (package: zsync); set APPSHELF_SKIP_ZSYNC=1 for a local build" >&2; exit 1; }
  (cd "$outdir" && zsyncmake -u "https://github.com/maxmya/appshelf/releases/download/v$version/$base" -o "$base.zsync" "$base")
fi
(cd "$outdir" && sha256sum "$base" > "$base.sha256")
echo "$output"
