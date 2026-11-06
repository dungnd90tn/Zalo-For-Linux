#!/usr/bin/env bash
# Build installable .deb and .rpm packages of the native Rust client into dist/rust/.
# Needs docker and network (images and crates are fetched; every external input is pinned).
# The binary is compiled in Ubuntu 20.04 so it only needs glibc 2.31, not the host's glibc: that
# covers Ubuntu 20.04+, Debian 11+, RHEL/Alma/Rocky 9+ and Fedora.
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
UBUNTU_IMAGE="ubuntu@sha256:8feb4d8ca5354def3d8fce243717141ce31e2c428701f6682bd2fafe15388214"
FEDORA_IMAGE="fedora@sha256:99e203b80b1c3d8f7e161ec10a68fd02b081ef83a3963553e513c82846b97814"
RUSTUP_VERSION="1.29.3"
RUSTUP_SHA256="b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55"

PKG_VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$REPO_ROOT/rust-client/Cargo.toml" | head -1)"
# Bump the release for every package you hand over, even a rebuild of the same code.
PKG_RELEASE="${ZALO_PKG_RELEASE:-1}"
OUT="$REPO_ROOT/dist/rust"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/zalo-linux-build/rust-pkg"
mkdir -p "$OUT" "$CACHE"; chmod 700 "$CACHE"
rm -f "$OUT"/zalo-native*.deb "$OUT"/zalo-native*.rpm

docker run --rm --network=host \
    -v "$REPO_ROOT:/src:ro" -v "$OUT:/out" -v "$CACHE:/cache" \
    -e RUSTUP_VERSION="$RUSTUP_VERSION" -e RUSTUP_SHA256="$RUSTUP_SHA256" \
    -e PKG_VERSION="$PKG_VERSION" -e PKG_RELEASE="$PKG_RELEASE" \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    "$UBUNTU_IMAGE" /src/scripts/rust/package/container-build.sh

docker run --rm --network=host \
    -v "$REPO_ROOT:/src:ro" -v "$OUT:/out" \
    -e PKG_VERSION="$PKG_VERSION" -e PKG_RELEASE="$PKG_RELEASE" \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    "$FEDORA_IMAGE" /src/scripts/rust/package/make-rpm.sh /out/zalo-rust /out

rm -f "$OUT/zalo-rust"
( cd "$OUT" && sha256sum zalo-native*.deb zalo-native*.rpm | tee SHA256SUMS )
