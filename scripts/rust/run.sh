#!/usr/bin/env bash
set -euo pipefail

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
        BUILD_FEATURES=(--features live)
        break
    fi
done
cargo build --locked --manifest-path "$REPO_ROOT/rust-client/Cargo.toml" --release "${BUILD_FEATURES[@]}"
TARGET_DIR="$(cargo metadata --locked --offline --no-deps --format-version 1 --manifest-path "$REPO_ROOT/rust-client/Cargo.toml" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
exec "$TARGET_DIR/release/zalo-rust" "${PROFILE_ARGS[@]}" "$@"
