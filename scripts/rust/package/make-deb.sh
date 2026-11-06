#!/usr/bin/env bash
# make-deb.sh BINARY OUTDIR  (needs dpkg-deb; run as root so files are root-owned in the archive)
set -euo pipefail
BINARY="$1"; OUTDIR="$2"
: "${PKG_VERSION:?}" "${PKG_RELEASE:?}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
PKG=zalo-native
APP_ID=zalo-linux-rust-preview
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

"$ROOT/scripts/rust/package/stage-files.sh" "$BINARY" "$STAGE"
mkdir -p "$STAGE/DEBIAN"
SIZE=$(du -sk "$STAGE/usr" | cut -f1)
cat > "$STAGE/DEBIAN/control" <<CTRL
Package: $PKG
Version: $PKG_VERSION-$PKG_RELEASE
Section: net
Priority: optional
Architecture: amd64
Maintainer: Zalo Native contributors <noreply@github.com>
Homepage: https://github.com/dungnd90tn/Zalo-For-Linux
Installed-Size: $SIZE
Depends: libc6 (>= 2.31), libgcc-s1, libgl1 | libegl1, libxkbcommon0
Recommends: libwayland-client0, libx11-6, libxkbcommon-x11-0, libxcursor1, libxi6, libxrandr2, libegl1, xdg-desktop-portal
Conflicts: zalo-native-preview
Description: Zalo native client (independent Rust preview)
 Native Linux client for Zalo written in Rust with egui, without Electron.
 This is an unofficial preview: login and messaging are optional features, and
 the offline demo (zalo-native --demo) uses only synthetic data.
 It is independent from the Electron-based "zalo" package and can be installed
 next to it.
CTRL
fakeroot_cmd=()
command -v fakeroot >/dev/null 2>&1 && [ "$(id -u)" != 0 ] && fakeroot_cmd=(fakeroot)
"${fakeroot_cmd[@]}" dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$OUTDIR/${PKG}_${PKG_VERSION}-${PKG_RELEASE}_amd64.deb" >/dev/null
echo "built $OUTDIR/${PKG}_${PKG_VERSION}-${PKG_RELEASE}_amd64.deb"
