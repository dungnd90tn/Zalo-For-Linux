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

`run.sh --live` builds the feature and starts one asynchronous session check: restore a saved session or create a QR when unsigned. Only verified authentication expiry or legacy-client migration permits automatic fresh QR; network/service failures preserve the record and expose Retry. Cancel/logout must not create another automatic attempt. Informational flags and demo/headless modes stay inert. `--doctor` reports compiled features separately from authenticated/network-started state. Live smoke uses only diagnostics and the offline demo, never a real account. Historical Xvfb release checks, before automatic startup was added, at 1200×780 and 640×480 exercised signed-out startup, Resume without a saved record and graceful close: strace recorded zero Internet connect calls, no authentication record was written, and the process lock was released. See [MIGRATION_STATUS.md](MIGRATION_STATUS.md) for artifact hashes and current test counts.

The user confirmed QR login and saved Resume reaching Online. After an earlier synchronization error, the earlier heartbeat/logging artifact's sanitized logs show five real-server heartbeat cycles over about 155 seconds, normal shutdown and two post-commit receive-ready events, without failure/offline/send events. The user subsequently confirmed incoming plain text; outgoing exchange remains unaccepted. Remaining real-account acceptance requires longer connectivity, verified directory contents, outgoing and full-content personal/group exchange, reconnect without duplication and account isolation. E2EE/Signal and other media remain unsupported; fail closed rather than downgrade or discard an unsupported receive queue. Preserve copied source history and validate IDs/counts/timestamps/media before any import. Never point a preview at `~/.config/ZaloData`. QR codes, credentials, messages and raw profile data must remain outside commits.

## Automatic diagnostic logs

Normal profile runs, including headless demos, use a background logger with a 256-event queue and four 256 KiB JSONL slots at `<profile-cache>/diagnostics/native.log{,.1,.2,.3}`. The directory is owner-only (0700), files are 0600 and symbolic/hard links are rejected. `--debug-log` enables additional structural metadata; failures and lifecycle events are always enabled. Events contain fixed categories, bounded numeric metadata and executable identity, excluding payloads, credentials, URLs and account/message identifiers.

`--doctor --profile rust-live` reports `diagnostic_log_path` as an absolute path without creating a profile or log; include the launch's custom profile flags when applicable. Smoke checks inspect actual lifecycle records, executable SHA-256, file permissions/size limits and schema-four storage. Unit tests cover queue saturation, rotation, preexisting unsafe/oversized files and structural redaction. A demo GUI check must additionally confirm graceful shutdown/log flushing and zero Internet connections. Automatic live startup is tested with synthetic loopback transport, never a real-account QR. Logs support diagnosis; they do not prove production interoperability.

## Existing sessions and handshake evidence

Live requests use the source-verified Linux desktop client type 24. New auth records explicitly persist it in schema version 2. Earlier records missing the type remain `Legacy30`; Resume must return fresh-QR guidance before HTTP creation and preserve saved history/credentials. Close the old Rust window and choose a new QR code rather than repeatedly restoring that session. No deletion or automatic retyping is permitted.

`tests/fixtures/messaging/capture_electron.cjs` records a disposable file renderer against strict pinned loopback WSS. The sanitized Electron 43.3.0 fixture establishes `Origin: file://`, desktop user agent and type-24 query; native integration fixtures check rejection of the previous null/type-30 contract and acceptance of the corrected one. The earlier heartbeat/logging release also has a limited real-server checkpoint: exact executable hash verified, Online at 16:44:53.359 UTC, five heartbeat cycles, two post-commit receive-ready events and normal shutdown at 16:47:28.588 UTC on 2026-11-01; no failure/offline/send events observed. See [MIGRATION_STATUS.md](MIGRATION_STATUS.md) for its boundaries and [PROTOCOL_MESSAGING.md](PROTOCOL_MESSAGING.md) for protocol evidence.

`node rust-client/tests/fixtures/messaging/auth_oracle.cjs` checks stored synthetic frames against the original decoder, bundled Pako and authenticated-queue gate without rewriting fixtures. It covers five accepted formats, four AUTH rejections and three invalid queues; actual runtime TLS tests enforce Online only after valid AUTH. Parser diagnostics expose fixed enum reasons, never response bytes or keys. These fixtures do not replace a real-account retry.

`node rust-client/tests/fixtures/messaging/post_auth_oracle.cjs` independently checks 18 original control/API/encrypted-message cases. The runtime regression `actual_runtime_survives_three_source_pongs_then_sends_via_layered_563_and_logs_invalid_message` uses actual loopback TLS/WSS and controlled time: three PONG/PING_ACTIVE cycles, later encrypted receive, layered capability reply and HTTPS send/ACK, then malformed input without a queue ACK. It verifies the real failure reaches default-mode JSONL with structural fields and no fixture payload/credential markers.

Schema-four storage is additive. Default tests preserve schema-two/three messages, outbox, cursors and directory metadata across migration; optional `signal-store` tests convert bounded typed snapshots/deltas. Dependency checks ensure this DTO feature does not enable QuickJS, libsignal-core or networking. Passing these tests does not enable production E2EE; see [STORE_E2EE.md](STORE_E2EE.md).

## Unreleased Signal preparation checks

The explicit `signal-crypto` feature enables the embedded engine and offline coordinator. Consequently, `--all-features` now includes this engine; ordinary default, `live` and DTO-only `signal-store` builds retain their separate dependency guards. The earlier source-only checkpoint preserved the user-tested `470bc5…fabd6` executable. The subsequent composer/sticker LIVE build still excludes the engine and does not activate Signal in its runtime/UI.

Run these inside `rust-client/`, with fetched locked dependencies, a C compiler, OpenSSL and Node.js available:

```sh
node tests/fixtures/signal/oracle.cjs
node tests/fixtures/signal/receive_oracle.cjs
node --max-old-space-size=96 --no-warnings tests/fixtures/signal/business_oracle.cjs
cargo test --locked --offline --features signal-crypto --test signal_crypto
cargo test --locked --offline --all-features
cargo clippy --locked --offline --all-targets --all-features -- -D warnings
```

The source oracles compare 13 requests/two SMS envelopes, five ordered session events, and real synthetic crypto round-trips including 64 KiB text. Coordinator tests must use real engine outputs and private temporary SQLite: one account-scoped CAS for all devices, authenticated identity binding, durable verified dispatch provenance, restart, failed-SQL retry, cancellation and stale-result rejection. Actor planning and commit are synchronous; asynchronous engine work owns its snapshot and never borrows `MessageStore` across an await. This is local cryptographic/storage evidence, not live provisioning, send, receive or ACK acceptance. See [PROTOCOL_SIGNAL.md](PROTOCOL_SIGNAL.md).

## Composer and incoming sticker checks (2026-11-02)

The frozen composer/sticker source passed **296 tests** (282 library, two CLI, eight real-engine personal/coordinator and four provisioning integration tests), with two opt-in tests ignored. Whole-crate formatting and strict default/all-feature Clippy passed. The default check script passed 121 Rust and seven Python tests before the final placeholder-width-only adjustment; final formatting/lint and all-feature tests include that adjustment. Default, live and DTO-only dependency guards exclude the embedded engine; only explicit `signal-crypto` selects it. Independent original-source sticker and three Signal Node oracles passed. No public QR/account request or private profile read was performed by QA.

Actual isolated X11 windows at 1200×780 and 640×480 verified clicks across the full Online editor, Vietnamese/ASCII input, scrolling/clipping of long drafts, a synthetic PNG through the production decoder into a texture, and a same-ID/different-catalog failure fallback. Both sticker surfaces are 130 pixels wide. Windows closed with exit 0 and zero Internet connections. Test-only image cache guards prohibit HTTP even on failure/eviction; those Unicode-injection checks did not establish IME or production sticker delivery; the subsequent x-unikey section records actual X11 IME evidence.

Only newly received typed stickers, or replayed rows retaining recoverable descriptors, can show static PNG thumbnails. Earlier releases discarded sticker catalog IDs: existing unsupported placeholders remain unchanged unless replay/future history fetching supplies metadata. No automatic history refetch is implemented. Animation, GIF/WebP, sticker catalog/search/send and general attachments remain unsupported. See [PROTOCOL_STICKERS.md](PROTOCOL_STICKERS.md) for URL restrictions, decoder/queue/cache bounds and source evidence.

## Actual x-unikey regression (2026-11-02)

The installed `x-unikey` 1.0.4-16 package provides a modern IBus engine, not the historical `ukxim` binary. Its installed README describes general-field PREEDIT/commit behavior. Baseline physical ASCII Telex `tieengs vieetj ` produced real engine commits but no native text; Ctrl+Shift OFF restored ASCII input. `egui-winit` 0.29.1 discarded all Linux IME events after upstream [issue #5008](https://github.com/emilk/egui/issues/5008). The licensed, pinned [adapter patch](vendor/egui-winit/LINUX_IME_PATCH.md) restores payload handling while keeping context availability distinct from composition, and resets the Linux input context when the editor identity changes.

The final isolated X11/IBus/XIM checks used the actual installed engine, normal ASCII keysyms and Telex, not Unicode remapping/paste. Bubblewrap hid the real home behind a private mount without changing `HOME`; D-Bus, IBus, display and configuration were private. Verified: `tiếng việt` once, OFF arrows/Backspace, compact input, active composer `chào` → search `việt` without a foreign prefix, and separate conversation drafts. Both actual native-window close and strace zero-Internet checks passed. Earlier Unicode-injection screenshots were not IME evidence. Native Wayland and the user's real desktop retry remain unaccepted.

`check.sh` explicitly runs `cargo test --locked --offline -p egui-winit --lib linux_ime`, because dependency tests are not included in the application's test suite. Final checks passed 121 default Rust, five adapter regressions, seven Python tests and 27 optional UI tests; formatting, feature-graph guards and strict default/all-feature Clippy passed. The full all-feature suite passed 298 tests/two ignored before the final context-reset-only adjustment; the final adapter/UI/default checks cover that adjustment. No unrelated network suite was repeated afterward.

The actual LIVE executable was also tested in isolated offline demo mode with real x-unikey and `--debug-log`. Enter committed `chào` plus a newline without saving; a subsequent explicit Ctrl+Enter saved exactly one synthetic SQLite row. The real JSONL contained `kind=debug`, `stage=input_method`, `reason=ready`, `bytes=5`, with no input text. Startup identified SHA-256 `d1ea87aba26330be96f3d2af19e56dbda187cfde9664d79a29fe64f1c5478911`; normal close flushed diagnostics, exited 0 and made zero Internet connections. Synthetic evidence: `/tmp/zalo-rust-gui-check/ime-demo-private.jsonl` and `unikey-release-demo-explicit-save.png`. This verifies local input/logging/demo persistence, not a real-account outgoing send.

## ACK/echo storage regression (2026-11-02)

The user reported `[Storage/Storage]` after the first send. The earlier sanitized log recorded send-ready followed by synchronization/storage I/O labels, but those generic labels did not distinguish logical replay conflicts from SQLite errors. No real database or payload was inspected, so the actual account failure remains unclassified.

A deterministic regression failed on the old store: HTTP ACK persisted a server ID/local timestamp, then an own WebSocket echo with that ID, a different timestamp and missing/unrecognized client ID conflicted. The fix uses the already durable, account/thread/server-ID binding with exact own sender/body and no sticker substitution. Known contradictory client IDs, conflicting payloads, ambiguous bindings and unrelated messages still fail atomically. Tests include restart, echo-before-ACK, replay and cursor rollback. No schema migration or history reset is required.

Actual loopback TLS/WSS verifies ACK → duplicated own echo → subsequent send through the real command loop. Separate failed-batch coverage preserves the old cursor and local history while pausing transport until explicit retry; unsafe data is not skipped. Default-mode failure logs now add closed `storage_operation`/`storage_failure` labels and an optional numeric extended `sqlite_code`. A real SQLite trigger regression checks code 1811 and that SQL, paths, trigger text, bodies and credential markers never reach private JSONL. This local evidence does not classify the old user's event or prove the production retry.

Final independent checks passed **127 default Rust tests** (125 library + two CLI), **five vendored IME regressions**, **seven Python tests**, formatting, default strict Clippy, feature-graph guards and displayless/no-write CLI smoke. The complete frozen all-feature suite passed **307 tests** (293 library + two CLI + eight coordinator + four provisioning integration tests), with two opt-in checks ignored and zero failures. The runtime owner's whole all-feature/all-target strict Clippy result applies to this exact freeze. UI/vendor IME behavior was unchanged and its earlier actual-engine evidence was retained; no additional real-account, public QR or IME session was run.

## Canonical text and automatic startup (2026-11-02)

Run `node rust-client/tests/fixtures/messaging/plain_text_oracle.cjs` from the repository root to check the 38 independently generated original-Electron trim vectors; CI runs it separately, and ordinary `check.sh` does not require Node. Rust consumes the same checked-in fixture. Verify exact ECMAScript boundary trimming, preserved internal spaces/decomposed Unicode, ACK-bound legacy repair, missing/contradictory binding negatives, rollback and unresolved-send protection without allocating a new ID. Default-mode mismatch diagnostics expose only a 1–7 bitmask (thread/body/rich content), never the values.

The frozen suite passed 132 default Rust, five adapter, seven Python and 319 all-feature tests, with two ignored opt-in tests. Formatting and strict default/all-feature/all-target Clippy passed. The real runtime TLS/WSS test checks canonical decrypted HTTP and ACK/echo followed by a second send; it is not a real-account delivery test.

Actual isolated startup GUI uses the same constructor with a cfg(test) loopback transport: `ZALO_UI_FIXTURE_LOGIN=qr|retry` plus the ignored `signed_in_offline_native_window_fixture`. At 1200×780 and 640×480, unsigned startup opens a synthetic QR automatically; a saved session's service error preserves Retry without a forced QR. Both close with exit 0 and zero non-loopback connections. Controller tests cover successful saved restore/Ready, expiry-to-QR, transient failure/retry, stale generations and cancel/logout; interim loading visibility is state-tested. Do not launch a fresh production `--live` profile merely for offline GUI smoke: startup now intentionally performs authentication work. Release smoke continues to use only informational flags and synthetic demo mode. No phone-number login or production retry is accepted by this checkpoint.

The new LIVE-only release is **15,929,104 bytes**, SHA-256 `c29e1e6c071106808269ed55b2b882d618eac092b670c5d021893bf77899986a`. Its informational/no-write and synthetic demo release smoke passed, including private bounded diagnostics, schema four, process locking, restart and additive legacy upgrade. No production live startup was executed for QA. Previous d14 is retained at `/tmp/zalo-autologin-storage-previous-d14`.

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

Review `src/`, manifest, build scripts, and tracked assets against authorship boundaries. Do not vendor `ctdozlite` source, assets or licenses. The native UI intentionally reuses existing repository Zalo CSS/theme facts, Segoe UI and icon fonts; verify every embedded asset's source path and hash. Live builds intentionally embed the repository's pinned trust WASM and recheck its SHA-256 before instantiation; no external binary is downloaded. Independently authored fixture JSON may be embedded in tests. The explicitly vendored `egui-winit` 0.29.1 adapter retains its MIT/Apache licenses and exact archive/upstream provenance in `vendor/egui-winit/LINUX_IME_PATCH.md`. Other registry dependencies retain their own metadata and licenses. Record actual inventory results; text searches alone cannot prove authorship.

The final font inventory is unchanged from `pc-dist/fonts/`:

| Existing asset | Bytes | SHA-256 |
| --- | --- | --- |
| `Zalo-Segoe-UI.dc1a7d868a0b3a0e2daa8b36567e9ea9.ttf` | 297,496 | `64dadf29976acf817f128f8aefc9a762039097c6c090d9c66a301e5e41610158` |
| `zalo-font.7f2d95cebfac22d16e486bfd1716c591.ttf` | 252,732 | `9a7758f970b8908411351cf7b3c46f62daae05a83dd4f721e5fbc1881102408d` |

Theme selectors, layout facts and icon mappings are recorded in [UI_THEME.md](UI_THEME.md). Authentication acceptance must additionally follow [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md); offline cryptographic success does not activate a session.
