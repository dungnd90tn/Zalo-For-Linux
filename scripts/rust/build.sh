#!/usr/bin/env bash
set -euo pipefail
# rustup installs to ~/.cargo/bin, which a fresh shell may not have on PATH yet.
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT/rust-client"
exec cargo build --locked --manifest-path "$REPO_ROOT/rust-client/Cargo.toml" --release "$@"
