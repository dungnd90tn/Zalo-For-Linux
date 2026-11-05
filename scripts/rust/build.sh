#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT/rust-client"
exec cargo build --locked --manifest-path "$REPO_ROOT/rust-client/Cargo.toml" --release "$@"
