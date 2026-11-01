# Rust preview validation

## Reproducible offline checks

Install the toolchain pinned in `rust-toolchain.toml` with `rustfmt`/`clippy`, Python 3, and a C/C++ compiler. Ubuntu/Debian builders use `build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev` to cover the enabled X11/Wayland/OpenGL backends; a desktop and graphics driver are only needed for GUI checks. CI uses Ubuntu 24.04.

```sh
(cd rust-client && cargo fetch --locked --manifest-path Cargo.toml)
./scripts/rust/check.sh
./scripts/rust/build.sh --offline
python3 scripts/rust/smoke-release.py
./scripts/rust/build.sh --offline --features live
python3 scripts/rust/smoke-release.py --expect-live
```

Fetching dependencies needs network access; subsequent checks use `--offline`. Checks cover formatting of the whole independently written crate, unit/integration tests, Clippy with warnings denied, benchmark-harness tests, `--help`/`--version`/`--doctor` without profile writes, and synthetic headless demo execution in a disposable profile. Release smoke checks restart idempotency, external process-lock refusal, private permissions, compiled-feature reporting, and default-build refusal of live mode. For custom Cargo target directories, pass `--binary /absolute/path/to/zalo-rust` to the smoke script. No real account, Electron profile, or network login is selected. Passing this gate demonstrates local logic only.

Review [ARCHITECTURE.md](ARCHITECTURE.md) alongside tests. Check queue saturation/backpressure, bounded caches, idempotent scoped storage, stable keyset pages, indexed query plans, cancellation/reconnect, and responsive UI behavior. A test that merely returns fixture success cannot verify the real protocol.

Optional protocol adapters are excluded from default builds. Install the `openssl` executable for independent cryptographic cross-checks and local TLS certificate generation; TLS/WSS tests need loopback socket permission. Their separate CI gate runs `cargo test --locked --offline --all-features` and `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` from `rust-client/`. Live builds bundle the existing hash-verified trust WASM from `pc-dist/libs/`; external loading requires an explicit absolute override and the same checksum. Independently constructed positive/negative fixtures verify HMAC, A1, D2 and L3 contracts. See [TRUST_FIXTURES.md](TRUST_FIXTURES.md).

The optional `network` feature implements the HTTP/cookie adapter described in [PROTOCOL_HTTP.md](PROTOCOL_HTTP.md). Tests exercise bounded bodies, real local TLS, certificate/SNI rejection, cookie isolation and deadline/abort socket closure. `live-login` adds the bounded QR/trust/bootstrap/session coordinator; `messaging` adds encrypted personal/group text via HTTPS/WSS; `live` enables both. Test-only DNS/CA overrides keep integration requests on loopback. These fixtures must not be described as verification of a real account or production endpoint flow.

CI also runs `cargo test --release --locked --offline --features trust-wasm protocol::trust`: Wasmi selects different dispatch backends for optimized and unoptimized builds, so both must pass. Preserve its `auto-dispatch` feature; increasing the process stack size would hide a backend configuration error.

## GUI and future live pilot

The launcher delegates default profile names and XDG paths to the CLI, so direct and scripted launches share data. Data lives under `${XDG_DATA_HOME:-$HOME/.local/share}/zalo-rust/profiles/<name>`; configuration and cache use their own XDG roots. The CLI chooses `rust-demo` for demos, `rust-live` for live mode and `rust-pilot` otherwise. Environment overrides below are optional; explicit CLI profile flags override them. A supplied `--profile-root` uses `<root>/<name>/{data,config,cache}` instead of the default XDG layout.

```sh
export ZALO_RUST_PROFILE_ROOT="$(mktemp -d /tmp/zalo-rust-pilot.XXXXXXXX)"
export ZALO_RUST_PROFILE=pilot
./scripts/rust/run.sh --demo
./scripts/rust/run.sh --live --profile live-pilot
```

Keep that directory for restart checks. Record commit, compiler, OS, desktop, Wayland/X11, display scale, and pass/fail evidence. Verify synthetic conversations, Vietnamese text/IME, emoji, keyboard navigation, scaling, large-history scrolling, and shutdown. Unsupported clipboard, notifications, screenshot, or attachment features stay explicitly unaccepted.

`run.sh --live` builds the feature automatically and opens signed out; opening this window must not start authentication or restore a session. An explicit login/restore action starts network work. `--doctor` reports compiled features separately from authenticated/network-started state. Live smoke uses only diagnostics and the offline demo, never a real account. Independent Xvfb release checks at 1200×780 and 640×480 also exercised signed-out startup, Resume without a saved record and graceful close: strace recorded zero Internet connect calls, no authentication record was written, and the process lock was released. See [MIGRATION_STATUS.md](MIGRATION_STATUS.md) for artifact hashes and current test counts.

The user confirmed QR login and saved Resume reaching Online. After an earlier synchronization error, the current artifact's sanitized logs show five real-server heartbeat cycles over about 155 seconds, normal shutdown and two post-commit receive-ready events, without failure/offline/send events. Receive content/type and UI rendering were not inspected. Remaining real-account acceptance requires longer connectivity, verified directory contents, outgoing and full-content personal/group exchange, reconnect without duplication and account isolation. E2EE/Signal/media are unsupported; fail closed rather than downgrade or discard an unsupported receive queue. Preserve copied source history and validate IDs/counts/timestamps/media before any import. Never point a preview at `~/.config/ZaloData`. QR codes, credentials, messages and raw profile data must remain outside commits.

## Automatic diagnostic logs

Normal profile runs, including headless demos, use a background logger with a 256-event queue and four 256 KiB JSONL slots at `<profile-cache>/diagnostics/native.log{,.1,.2,.3}`. The directory is owner-only (0700), files are 0600 and symbolic/hard links are rejected. `--debug-log` enables additional structural metadata; failures and lifecycle events are always enabled. Events contain fixed categories, bounded numeric metadata and executable identity, excluding payloads, credentials, URLs and account/message identifiers.

`--doctor --profile rust-live` reports `diagnostic_log_path` as an absolute path without creating a profile or log; include the launch's custom profile flags when applicable. Smoke checks inspect actual lifecycle records, executable SHA-256, file permissions/size limits and schema-four storage. Unit tests cover queue saturation, rotation, preexisting unsafe/oversized files and structural redaction. A real signed-out GUI check must additionally confirm graceful shutdown/log flushing and zero Internet connections. Logs support diagnosis; they do not prove production interoperability.

## Existing sessions and handshake evidence

Live requests use the source-verified Linux desktop client type 24. New auth records explicitly persist it in schema version 2. Earlier records missing the type remain `Legacy30`; Resume must return fresh-QR guidance before HTTP creation and preserve saved history/credentials. Close the old Rust window and choose a new QR code rather than repeatedly restoring that session. No deletion or automatic retyping is permitted.

`tests/fixtures/messaging/capture_electron.cjs` records a disposable file renderer against strict pinned loopback WSS. The sanitized Electron 43.3.0 fixture establishes `Origin: file://`, desktop user agent and type-24 query; native integration fixtures check rejection of the previous null/type-30 contract and acceptance of the corrected one. The current heartbeat/logging release also has a limited real-server checkpoint: exact executable hash verified, Online at 16:44:53.359 UTC, five heartbeat cycles, two post-commit receive-ready events and normal shutdown at 16:47:28.588 UTC on 2026-11-01; no failure/offline/send events observed. See [MIGRATION_STATUS.md](MIGRATION_STATUS.md) for its boundaries and [PROTOCOL_MESSAGING.md](PROTOCOL_MESSAGING.md) for protocol evidence.

`node rust-client/tests/fixtures/messaging/auth_oracle.cjs` checks stored synthetic frames against the original decoder, bundled Pako and authenticated-queue gate without rewriting fixtures. It covers five accepted formats, four AUTH rejections and three invalid queues; actual runtime TLS tests enforce Online only after valid AUTH. Parser diagnostics expose fixed enum reasons, never response bytes or keys. These fixtures do not replace a real-account retry.

`node rust-client/tests/fixtures/messaging/post_auth_oracle.cjs` independently checks 18 original control/API/encrypted-message cases. The runtime regression `actual_runtime_survives_three_source_pongs_then_sends_via_layered_563_and_logs_invalid_message` uses actual loopback TLS/WSS and controlled time: three PONG/PING_ACTIVE cycles, later encrypted receive, layered capability reply and HTTPS send/ACK, then malformed input without a queue ACK. It verifies the real failure reaches default-mode JSONL with structural fields and no fixture payload/credential markers.

Schema-four storage is additive. Default tests preserve schema-two/three messages, outbox, cursors and directory metadata across migration; optional `signal-store` tests convert bounded typed snapshots/deltas. Dependency checks ensure this DTO feature does not enable QuickJS, libsignal-core or networking. Passing these tests does not enable production E2EE; see [STORE_E2EE.md](STORE_E2EE.md).

## Directory milestone checks

`cargo test --locked --offline --all-features directory` covers the directory parser, isolated TLS/Node cipher oracle, SQLite cache and UI reducers. Optional native-window QA runs the ignored `ui::live::tests::signed_in_offline_native_window_fixture` with an isolated X11 display and `ZALO_UI_FIXTURE_DIRECTORY=1`. It creates a temporary synthetic account with 107 contacts, one group and two cached histories; it is absent from production binaries. Verify 100-row next/back pages, whole-cache Vietnamese search, named open, per-recipient drafts, compact navigation and close. Empty-endpoint diagnostics are expected; no network credentials or production endpoints are supplied.

`cargo run --release --locked --offline --example bench_directory -- 20000` exercises the actual store APIs using bounded 200-entry transactions, replay, keyset pages and accent-folded searches against a last-page contact. Repeat with `30000` for the capacity rollback check. It removes its private profile on exit. Record filesystem and cache state; this measures local storage work, not network sync, UI latency or comparative client memory.

## Performance harness

Build release mode first. `run.sh` builds then executes the actual binary, honoring Cargo's configured target directory. Use the application root PID, not Cargo or a shell. For Electron, select an already running disposable test instance explicitly; this harness does not launch apps or alter profiles.

```sh
python3 scripts/rust/benchmark.py --pid APP_ROOT_PID --duration 60 --interval 1 > /tmp/zalo-pss.csv
```

The Linux `/proc` harness sums **PSS of the root and all observed descendants**. Compare only rows with `complete=1`; missing/unreadable children invalidate that sample. Detached/reparented processes are outside the tree and must be documented. PID reuse is rejected. Sum PSS, not RSS, to account for shared memory.

Use the same machine, account, history volume, display, network, and equivalent idle/chat/media workload; record median/peak PSS across repeated runs. Benchmark actual protocol implementations only after feature parity: synthetic demo numbers cannot substantiate an Electron replacement claim. Separately measure launch → usable cached UI and launch → first successful online send/receive. A painted window does not prove online readiness. No comparative figures are available yet.

## Original-source provenance audit

Review `src/`, manifest, build scripts, and tracked assets against authorship boundaries. Do not vendor `ctdozlite` source, assets or licenses. The native UI intentionally reuses existing repository Zalo CSS/theme facts, Segoe UI and icon fonts; verify every embedded asset's source path and hash. Live builds intentionally embed the repository's pinned trust WASM and recheck its SHA-256 before instantiation; no external binary is downloaded. Independently authored fixture JSON may be embedded in tests. Registry dependencies retain their own metadata and licenses. Record actual inventory results; text searches alone cannot prove authorship.

The final font inventory is unchanged from `pc-dist/fonts/`:

| Existing asset | Bytes | SHA-256 |
| --- | --- | --- |
| `Zalo-Segoe-UI.dc1a7d868a0b3a0e2daa8b36567e9ea9.ttf` | 297,496 | `64dadf29976acf817f128f8aefc9a762039097c6c090d9c66a301e5e41610158` |
| `zalo-font.7f2d95cebfac22d16e486bfd1716c591.ttf` | 252,732 | `9a7758f970b8908411351cf7b3c46f62daae05a83dd4f721e5fbc1881102408d` |

Theme selectors, layout facts and icon mappings are recorded in [UI_THEME.md](UI_THEME.md). Authentication acceptance must additionally follow [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md); offline cryptographic success does not activate a session.
