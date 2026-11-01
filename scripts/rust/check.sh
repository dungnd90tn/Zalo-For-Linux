#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="$REPO_ROOT/rust-client/Cargo.toml"
cd "$REPO_ROOT/rust-client"
for script in "$REPO_ROOT"/scripts/rust/*.sh; do
    bash -n "$script"
done
python3 -m unittest discover -s "$REPO_ROOT/scripts/rust/tests" -v
cargo fmt --manifest-path "$MANIFEST" --all -- --check
cargo tree --locked --offline --manifest-path "$MANIFEST" -e normal --prefix none | python3 -c 'import sys; lines=sys.stdin.read().splitlines(); assert not any(line.startswith(("wasmi ", "reqwest ", "tokio-tungstenite ")) for line in lines), "optional protocol engines entered the default dependency graph"; print("Default dependencies exclude optional HTTP/WebSocket/WASM engines.")'
cargo test --locked --manifest-path "$MANIFEST" --offline
cargo clippy --locked --manifest-path "$MANIFEST" --offline --all-targets -- -D warnings
SMOKE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/zalo-rust-smoke.XXXXXXXX")"
trap 'rm -rf -- "$SMOKE_ROOT"' EXIT
SMOKE=(cargo run --quiet --locked --manifest-path "$MANIFEST" --offline -- --profile-root "$SMOKE_ROOT" --profile smoke)
env -u DISPLAY -u WAYLAND_DISPLAY "${SMOKE[@]}" --help
env -u DISPLAY -u WAYLAND_DISPLAY "${SMOKE[@]}" --version
env -u DISPLAY -u WAYLAND_DISPLAY "${SMOKE[@]}" --doctor | python3 -c 'import json,sys; d=json.load(sys.stdin); assert not any(d[k] for k in ("display_available", "live_auth_available", "live_send_available", "live_auth_compiled", "live_messaging_compiled", "authenticated", "network_started", "history_import_available")); print("Offline diagnostics: capabilities correctly unavailable.")'
if find "$SMOKE_ROOT" -mindepth 1 -print -quit | read -r _; then
    echo "Smoke check failed: informational CLI flags wrote profile data." >&2
    exit 1
fi
env -u DISPLAY -u WAYLAND_DISPLAY "${SMOKE[@]}" --demo --headless | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["mode"]=="offline-demo" and d["network_used"] is False and d["live_messages_sent"]==0 and d["messages_loaded"]>0; print("Headless demo: synthetic data loaded without live messaging.")'
