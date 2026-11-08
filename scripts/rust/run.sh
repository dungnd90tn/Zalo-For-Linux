#!/usr/bin/env bash
set -euo pipefail
# rustup installs to ~/.cargo/bin, which a fresh shell may not have on PATH yet.
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
# Keep the CLI's XDG layout and demo/pilot defaults as the single source of truth.
# Explicit CLI options come last and take precedence over environment overrides.
PROFILE_ARGS=()
if [[ -n "${ZALO_RUST_PROFILE_ROOT:-}" ]]; then
    PROFILE_ARGS+=(--profile-root "$ZALO_RUST_PROFILE_ROOT")
fi
if [[ -n "${ZALO_RUST_PROFILE:-}" ]]; then
    PROFILE_ARGS+=(--profile "$ZALO_RUST_PROFILE")
fi
cd "$REPO_ROOT/rust-client"
BUILD_FEATURES=()
for option in "$@"; do
    if [[ "$option" == --live ]]; then
        # Most 1-1 conversations are end-to-end encrypted now; without the embedded Signal engine their messages
        # can be neither sent nor read. ZALO_RUST_NO_E2EE=1 builds the plain `live` client instead.
        if [[ -n "${ZALO_RUST_NO_E2EE:-}" ]]; then
            BUILD_FEATURES=(--features live)
        else
            BUILD_FEATURES=(--features live,signal-crypto)
        fi
        break
    fi
done
# The E2EE experiments (E2EE_LIVE_PLAN.md) always need the engine, even with ZALO_RUST_NO_E2EE set.
if [[ ( -n "${ZALO_E2EE_REPLACE_PC_KEY:-}" || -n "${ZALO_E2EE_SEND_PROBE:-}" || -n "${ZALO_E2EE_INIT_SUBDEVICES:-}" || -n "${ZALO_E2EE_RECEIVE:-}" || -n "${ZALO_E2EE_INIT_PEER:-}" || -n "${ZALO_E2EE_SEND:-}" ) && "${#BUILD_FEATURES[@]}" -gt 0 ]]; then
    BUILD_FEATURES=(--features live,signal-crypto)
fi
cargo build --locked --manifest-path "$REPO_ROOT/rust-client/Cargo.toml" --release "${BUILD_FEATURES[@]}"
TARGET_DIR="$(cargo metadata --locked --offline --no-deps --format-version 1 --manifest-path "$REPO_ROOT/rust-client/Cargo.toml" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
exec "$TARGET_DIR/release/zalo-rust" "${PROFILE_ARGS[@]}" "$@"
