#!/usr/bin/env bash
# Runs INSIDE the pinned Ubuntu 20.04 container: compiles the native client against glibc 2.31 so
# the resulting package runs on every distribution at least that old, then builds the .deb.
set -euo pipefail

: "${RUSTUP_VERSION:?}" "${RUSTUP_SHA256:?}" "${PKG_VERSION:?}" "${PKG_RELEASE:?}" "${HOST_UID:?}" "${HOST_GID:?}"
export DEBIAN_FRONTEND=noninteractive CARGO_HOME=/cache/cargo RUSTUP_HOME=/cache/rustup
export PATH="$CARGO_HOME/bin:$PATH"

apt-get update -qq
apt-get install -y -qq --no-install-recommends ca-certificates curl build-essential pkg-config binutils dpkg-dev >/dev/null

if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
    curl -fsSL -o /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init"
    echo "$RUSTUP_SHA256  /tmp/rustup-init" | sha256sum -c - >/dev/null
    chmod +x /tmp/rustup-init
    /tmp/rustup-init -y --no-modify-path --default-toolchain none --profile minimal >/dev/null
fi

cd /src/rust-client
# rust-toolchain.toml pins the compiler; this installs exactly that one.
rustup toolchain install --profile minimal "$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)" >/dev/null 2>&1 || true
cargo --version
# Separate target dir per base release: reusing objects built against another glibc would
# silently raise the minimum version.
TARGET="/cache/target-$(. /etc/os-release; echo "$VERSION_ID")"
cargo build --release --locked --features "${PKG_FEATURES:-live}" --target-dir "$TARGET"
install -m 0755 "$TARGET/release/zalo-rust" /out/zalo-rust
strip --strip-unneeded /out/zalo-rust

echo "glibc symbols required: $(objdump -T /out/zalo-rust | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)"
/src/scripts/rust/package/make-deb.sh /out/zalo-rust /out
chown -R "$HOST_UID:$HOST_GID" /out /cache
