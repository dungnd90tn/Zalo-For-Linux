#!/usr/bin/env bash
# stage-files.sh BINARY DESTDIR : lays out the files both package formats install.
set -euo pipefail
BINARY="$1"; DEST="$2"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
APP_ID=zalo-linux-rust-preview
install -D -m 0755 "$BINARY" "$DEST/usr/bin/zalo-native"
install -d "$DEST/usr/share/applications"
sed 's|@EXECUTABLE@|/usr/bin/zalo-native|' "$ROOT/rust-client/linux/$APP_ID.desktop.in" \
    > "$DEST/usr/share/applications/$APP_ID.desktop"
chmod 0644 "$DEST/usr/share/applications/$APP_ID.desktop"
for size in 256 512; do
    install -D -m 0644 "$ROOT/pc-dist/favicon-${size}x${size}.png" \
        "$DEST/usr/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done
install -d "$DEST/usr/share/doc/zalo-native"
cat > "$DEST/usr/share/doc/zalo-native/copyright" <<'COPYRIGHT'
Independent native Linux client preview for Zalo.

The Rust sources are part of the Zalo-For-Linux repository. The package also
contains the original Zalo fonts and icons (c) VNG Corp. that the application
renders with; no license to those assets is granted by this package.
COPYRIGHT
chmod 0644 "$DEST/usr/share/doc/zalo-native/copyright"
