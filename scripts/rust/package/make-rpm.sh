#!/usr/bin/env bash
# make-rpm.sh BINARY OUTDIR  (runs INSIDE the pinned Fedora container; installs rpm-build)
set -euo pipefail
BINARY="$1"; OUTDIR="$2"
: "${PKG_VERSION:?}" "${PKG_RELEASE:?}" "${HOST_UID:?}" "${HOST_GID:?}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
dnf install -y -q rpm-build >/dev/null
STAGE="$(mktemp -d)"; TOP="$(mktemp -d)"
trap 'rm -rf "$STAGE" "$TOP"' EXIT
"$ROOT/scripts/rust/package/stage-files.sh" "$BINARY" "$STAGE"
sed -e "s|@VERSION@|$PKG_VERSION|" -e "s|@RELEASE@|$PKG_RELEASE|" -e "s|@STAGE@|$STAGE|" \
    "$ROOT/scripts/rust/package/zalo-native.spec.in" > "$TOP/zalo-native.spec"
rpmbuild -bb --define "_topdir $TOP" "$TOP/zalo-native.spec" >/dev/null
cp "$TOP"/RPMS/x86_64/zalo-native-*.rpm "$OUTDIR/"
chown "$HOST_UID:$HOST_GID" "$OUTDIR"/*.rpm
ls "$OUTDIR"/*.rpm
