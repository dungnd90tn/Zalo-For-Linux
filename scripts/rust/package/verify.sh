#!/usr/bin/env bash
# Install the built packages through the distribution package managers in clean containers and
# check that the installed program starts. Usage: verify.sh [image ...]  (defaults below)
set -euo pipefail
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT="$REPO_ROOT/dist/rust"
DEBS=(ubuntu:20.04 ubuntu:22.04 ubuntu:24.04 debian:12)
RPMS=(fedora:42 almalinux:9)
[ "$#" -gt 0 ] && { DEBS=(); RPMS=(); for i in "$@"; do case "$i" in ubuntu*|debian*) DEBS+=("$i");; *) RPMS+=("$i");; esac; done; }

check='set -e
zalo-native --version
zalo-native --doctor | python3 -c "import json,sys; d=json.load(sys.stdin); assert d[\"live_auth_compiled\"] and not d[\"authenticated\"] and not d[\"network_started\"]; print(\"doctor ok: live compiled, not authenticated, no network\")"
test -f /usr/share/applications/zalo-linux-rust-preview.desktop
grep -q "^Exec=/usr/bin/zalo-native --live" /usr/share/applications/zalo-linux-rust-preview.desktop
test -f /usr/share/icons/hicolor/256x256/apps/zalo-linux-rust-preview.png
! ldd /usr/bin/zalo-native | grep -q "not found"
echo "files ok"'

for image in "${DEBS[@]}"; do
    echo "=== $image: apt install ./zalo-native_*.deb"
    docker run --rm --network=host -v "$OUT:/pkg:ro" "$image" bash -c "
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq && apt-get install -y -qq python3-minimal ./\$(cd /pkg && ls zalo-native_*.deb | head -1 | sed 's|^|/pkg/|') >/dev/null
        $check
        apt-get remove -y -qq zalo-native >/dev/null && ! test -e /usr/bin/zalo-native && echo 'removed cleanly'"
done
for image in "${RPMS[@]}"; do
    echo "=== $image: dnf install ./zalo-native-*.rpm"
    docker run --rm --network=host -v "$OUT:/pkg:ro" "$image" bash -c "
        dnf install -y -q python3 /pkg/zalo-native-*.rpm >/dev/null
        $check
        rpm -V zalo-native && echo 'rpm -V clean'
        dnf remove -y -q zalo-native >/dev/null && ! test -e /usr/bin/zalo-native && echo 'removed cleanly'"
done
