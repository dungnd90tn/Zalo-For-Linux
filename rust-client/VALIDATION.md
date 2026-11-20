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

`--doctor --profile rust-live` reports `diagnostic_log_path` as an absolute path without creating a profile or log; include the launch's custom profile flags when applicable. Smoke checks inspect actual lifecycle records, executable SHA-256, file permissions/size limits and schema-six storage. Unit tests cover queue saturation, rotation, preexisting unsafe/oversized files and structural redaction. A demo GUI check must additionally confirm graceful shutdown/log flushing and zero Internet connections. Automatic live startup is tested with synthetic loopback transport, never a real-account QR. Logs support diagnosis; they do not prove production interoperability.

## Existing sessions and handshake evidence

Live requests use the source-verified Linux desktop client type 24. New auth records explicitly persist it in schema version 2. Earlier records missing the type remain `Legacy30`; Resume must return fresh-QR guidance before HTTP creation and preserve saved history/credentials. Close the old Rust window and choose a new QR code rather than repeatedly restoring that session. No deletion or automatic retyping is permitted.

`tests/fixtures/messaging/capture_electron.cjs` records a disposable file renderer against strict pinned loopback WSS. The sanitized Electron 43.3.0 fixture establishes `Origin: file://`, desktop user agent and type-24 query; native integration fixtures check rejection of the previous null/type-30 contract and acceptance of the corrected one. The earlier heartbeat/logging release also has a limited real-server checkpoint: exact executable hash verified, Online at 16:44:53.359 UTC, five heartbeat cycles, two post-commit receive-ready events and normal shutdown at 16:47:28.588 UTC on 2026-11-01; no failure/offline/send events observed. See [MIGRATION_STATUS.md](MIGRATION_STATUS.md) for its boundaries and [PROTOCOL_MESSAGING.md](PROTOCOL_MESSAGING.md) for protocol evidence.

`node rust-client/tests/fixtures/messaging/auth_oracle.cjs` checks stored synthetic frames against the original decoder, bundled Pako and authenticated-queue gate without rewriting fixtures. It covers five accepted formats, four AUTH rejections and three invalid queues; actual runtime TLS tests enforce Online only after valid AUTH. Parser diagnostics expose fixed enum reasons, never response bytes or keys. These fixtures do not replace a real-account retry.

`node rust-client/tests/fixtures/messaging/post_auth_oracle.cjs` independently checks 18 original control/API/encrypted-message cases. The runtime regression `actual_runtime_survives_three_source_pongs_then_sends_via_layered_563_and_logs_invalid_message` uses actual loopback TLS/WSS and controlled time: three PONG/PING_ACTIVE cycles, later encrypted receive, layered capability reply and HTTPS send/ACK, then malformed input without a queue ACK. It verifies the real failure reaches default-mode JSONL with structural fields and no fixture payload/credential markers.

Schema-six storage is additive and preserves the earlier schema-two/three/four/five migrations. Default tests retain messages, outbox, cursors, directory metadata and sticker/information sidecars; v4→v5 and v5→v6 tests add full information/media content without replacing those rows. Optional `signal-store` tests convert bounded typed snapshots/deltas. Dependency checks ensure this DTO feature does not enable QuickJS, libsignal-core or networking. Passing these tests does not enable production E2EE; see [STORE_E2EE.md](STORE_E2EE.md). A pre-v6 executable backup cannot reopen a migrated profile and does not provide a database downgrade path.

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

## Sender names (2026-11-02)

`ZALO_UI_FIXTURE_SENDERS=direct|group` selects the ignored native fixture's synthetic cached-contact cases. The actual SQLite actor resolves names; no network name service is substituted. Wide/compact screenshots verify the same name/initials in message bubbles and existing titles, a long Vietnamese label bounded inside the bubble, and neutral unknown-user fallback. Those fixtures deliberately have no service endpoints and report Offline; close exits 0 with zero Internet connections. IDs and self-message alignment stay unchanged.

The frozen checkpoint passed 137 default Rust, five adapter, seven Python and 328 all-feature tests, with two opt-in tests ignored. Formatting, strict default/all-feature/all-target Clippy and default/live/DTO graph guards passed. Regressions cover indexed bounded/account-scoped user lookup, group-ID collisions, rename during an in-flight lookup, stale page/account/thread replies, 50-sender pages, negative caching and unchanged history after rename/reopen. These results verify local cached labels, not production directory completeness or avatar downloading.

The sender-name LIVE artifact is **15,968,272 bytes**, SHA-256 `6c294dc134b3a74f98a5cd77883e841efec56386e0b67f599d4fb8598ccdd832`. Actual release smoke passed informational/no-write and synthetic demo checks without launching live startup; the prior c29e executable is preserved at `/tmp/zalo-displayname-previous-c29e`.

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

The native renderer embeds these five existing `pc-dist/fonts/` files; the complete source inventory additionally records formats that native rendering does not embed:

| Existing asset | Bytes | SHA-256 |
| --- | --- | --- |
| `Zalo-Segoe-UI.dc1a7d868a0b3a0e2daa8b36567e9ea9.ttf` | 297,496 | `64dadf29976acf817f128f8aefc9a762039097c6c090d9c66a301e5e41610158` |
| `zalo-font.7f2d95cebfac22d16e486bfd1716c591.ttf` | 252,732 | `9a7758f970b8908411351cf7b3c46f62daae05a83dd4f721e5fbc1881102408d` |
| `zalo-pc-screencap.26b46cc0d8549b2db2c3314913a3b308.ttf` | 2,732 | `6a64ee957cb572cf536dfadf42f3ea92cb04bff1339783acfbbba0690be665ab` |
| `Zalo-Segoe-UI-Semibold.8de26695631b05cf79e17378f6d1f134.ttf` | 300,840 | `bb84cebe71072fe700e9ddaec061375e3b97c25bf9f50216e8234413708aa451` |
| `Zalo-Segoe-UI-Bold.18b859523696ff3330adac79b99ef554.ttf` | 303,812 | `0905dc1c76a9f9accffeee85ee4ac02728d477c73663990922914fa0293ee6d8` |

Theme selectors, layout facts and icon mappings are recorded in [UI_THEME.md](UI_THEME.md). Authentication acceptance must additionally follow [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md); offline cryptographic success does not activate a session.


## Theme, catalog and Linux icon checks (2026-11-02)

The latest theme/icon snapshot passed `check.sh`: **154 default Rust**, **five adapter** and **19 Python tests**, whole formatting/default strict Clippy, dependency guards and displayless CLI checks. The complete all-feature suite passed **346 tests**, with two opt-in tests ignored. Final settings-only polish passed focused pointer controls at wide/compact sizes plus final strict all-feature/all-target Clippy. Run both reproducibility checks after changing original assets, palette bindings or the generator:

```sh
python3 scripts/rust/inventory-ui-assets.py --check
python3 scripts/rust/generate-ui-catalog.py --check
```

The runtime catalog preserves 758 source tokens and 1,034 original Zalo/Screencap glyph aliases with verified font cmaps; existing widgets consume 29 semantic roles/32 source tokens. Custom saves unbound catalog tokens without claiming a visual change. Preference tests verify private files, bounded asynchronous I/O, load/edit races, immediate-close persistence, CAS conflicts and bounded lock contention. Queue/catalog/RGBA limits do not measure total process RAM or VRAM.

Synthetic X11 windows verify Light/Dark, both injected OS signals for System, Custom token and gradient rendering, original glyph galleries, bounded advanced settings and wide/compact layouts. Actual release demo button selection wrote Dark to a 0600 profile theme file; normal close exited 0, released the lock, reopened the same synthetic profile and made zero Internet connections. System screenshots use a cfg(test) raw toolkit signal; native Wayland/desktop theme-change behavior remains unverified. Production IME translation is unchanged; five retained adapter regressions pass.

The actual release's `_NET_WM_ICON` is 512×512, RGBA SHA-256 `0ad413509c7122c1d508b9b6ce31425387c9468189422622c55d2ce644f63503`, exactly matching the existing PNG decoded pixels. Its source PNG SHA-256 is `a91651455f5090100dcb3b824c68e4dc8ec8229855c97fbd60f03594cc8362ab`. Actual `WM_CLASS`, desktop stem, StartupWMClass and hicolor icon name agree on `zalo-linux-rust-preview`. The staged entry passes desktop-file validation; five installer tests cover real inert GLib launch quoting, exact icons, dry-run, Electron preservation and unsafe targets. Final user-XDG installation also completed successfully: the executable and both icon hashes were independently compared with their sources, desktop identity/Exec fields were checked, and desktop validation/cache refresh succeeded. No app was launched by installation. Native Wayland dock grouping is not claimed.

Historical theme/icon LIVE artifact: **17,027,920 bytes**, SHA-256 `1d07f96c73751ddf450e8a658576de0dd910ed915ea0e082d5d8c7f551241ef3`. Release smoke passed without launching automatic live startup. Previous 6c remains `/tmp/zalo-theme-icon-previous-6c`; current review screenshots and exact icon-property evidence are recorded in [MIGRATION_STATUS.md](MIGRATION_STATUS.md). No real account or private profile data was used by QA.

## Receive-schema regression checks (2026-11-04)

The user reported another `Synchronization/Protocol` pause. The previous generic
record lacks the rejected field, so it cannot establish the private payload's
cause. The new parser reports fixed message/batch/queue reasons and optional
closed field type/message kind/bounded text-byte metadata. Ordinary logs retain
these failures without `--debug-log`; neither contents nor identifiers are logged.

An actual loopback TLS/WSS regression covers ten cases: nullable optional
collections/client correlations; positive terminal continuation; null/numeric-zero/
string-zero terminal sentinels; the same invalid continuations while `more` is
true; malformed terminal objects; and mixed pages with an invalid sender or
client-ID object. Valid rows commit before publication. Rejected pages retain the
previous cursor and cached rows, publish no valid prefix, request no next page,
keep the local actor usable, and trigger no automatic send replay or poison
reconnect. Another account remains isolated. Existing repeated-cursor, failed
SQLite commit and page-budget regressions remain enabled.

Final gates passed **156 default Rust tests** (153 library + three CLI), **five
IME adapter tests**, **19 Python tests** and **351 all-feature tests** (336
library + three CLI + eight coordinator + four provisioning), with two opt-in
checks ignored and zero failures. Whole-crate formatting, strict default and
all-target/all-feature Clippy, feature dependency guards and both deterministic
asset/catalog checks passed. No UI/input behavior changed, so existing visual/IME
acceptance boundaries remain unchanged. These synthetic tests do not prove the
specific live account's failure is resolved; rich `webchat` objects remain
unsupported and fail closed with `MessageContent`. See the newest artifact in
[MIGRATION_STATUS.md](MIGRATION_STATUS.md).

The historical receive-schema LIVE build is **17,035,792 bytes**, SHA-256
`fb0bdebac34094c79c821d6cf6e72ff0a68b290627feba7afa646d9da38cd5b7`.
Disposable release smoke passed without starting automatic live login: schema
four, 19 synthetic messages/six threads, additive legacy restart, private bounded
logs and executable identity, locking and informational no-write behavior.
Doctor confirms login/messaging compiled and authenticated/network-started false.
Previous 1d07 remains `/tmp/zalo-sync-protocol-previous-1d07`. Original-source AUTH
and post-AUTH Node oracles also passed; no public QR/account request or real
history database was used by QA.

The coordinator also refreshed the existing managed user-XDG launcher with this
artifact and independently compared the installed executable SHA-256. Desktop
identity and original icons are retained. Real-account acceptance is pending the
user reopening **Zalo Native** and the coordinator inspecting only sanitized logs.

## Authenticated queue routing regression (2026-11-04)

After reopening fb0b, the user's sanitized diagnostic identified
`message_scope`, command 510/subcommand 1, with a string field and closed `other`
message kind. The old generic parser assumed a direct packet's recipient must be
the local account. Original preprocessing/default consumers instead select the
non-self sender as the direct peer in authenticated legacy queues, including
peer-addressed `msgs`/`pageMsgs`. This observation identifies the failing parser
boundary without exposing the message subtype, identities or contents.

The corrected path requires an authenticated decoder and a connection account
bound from `VerifiedSession` matching the event account. Every stored message
still belongs to that local account; numeric identifiers, group/direct separation,
batch/cursor atomicity and the generic untrusted parser's recipient checks remain
strict. No Signal parser or control-operation behavior changed.

The original-source routing oracle executes preprocessing and the consumer key
expression for eight cases. Real loopback `510_1` fixtures verify peer-addressed
text and existing unsupported-media placeholders, page messages, self echoes,
numeric group/direct collisions, successful continuation, and account isolation.
A mixed next page with a malformed recipient commits no prefix, retains cursor
100 and cached history, and sends no further pull/replay. Separate tests reject
unauthenticated decoders and mismatched event accounts before storage or events.

Final checks passed **156 default Rust**, **five IME adapter**, **19 Python** and
**354 all-feature tests** (339 library + three CLI + eight coordinator + four
provisioning), with two opt-in checks ignored and zero failures. Formatting,
strict default/all-target/all-feature Clippy, feature guards and the AUTH/post-AUTH/routing source
oracles passed. The optional Signal TLS fixture only gained the required existing
session-account argument in its test constructor. Live acceptance remains pending
the user reopening the final artifact recorded in [MIGRATION_STATUS.md](MIGRATION_STATUS.md).

Final routing LIVE release: **17,036,880 bytes**, SHA-256
`11291eb0ab9628354ce4845d42cb94a14db931c5360432378f9f19b7f2f30ec0`.
Locked offline build and disposable smoke passed schema four, synthetic
19-message/six-thread restart, additive legacy preservation, private bounded
diagnostics and executable identity, locking and informational no-write checks.
Doctor reports login/messaging compiled and authenticated/network-started false;
Signal remains inactive in LIVE. The previous fb0b executable is retained at
`/tmp/zalo-scope-protocol-previous-fb0b`. No real account/network login or history
database was used by QA.

The coordinator independently verified and installed this routing release in the
managed user-XDG **Zalo Native** launcher, with an exact installed executable
hash comparison. Existing desktop identity/icons remain unchanged. User restart
and subsequent sanitized-log acceptance are pending.

## Lossless information-message checkpoint (2026-11-04)

The user's 11291 retry passed the earlier routing boundary, committed many
received messages, then reported command **511/subcommand 1**, `message_content`,
object field and closed `webchat_info` kind. The corrected exact format is
`webchat` / `msginfo.actionlist`. QA used only synthetic source-shaped objects;
no private message contents or history database were read.

The parser produces readable Vietnamese template/name/title text while retaining
the entire content in a bounded immutable sidecar. Original params strings,
actions and unknown nested fields survive receive, preview, replay and restart.
Marker indexing bounds projection work without repeatedly rescanning a large
template. Information events cannot acknowledge outgoing text intents. Native
rendering is read-only: it does not execute actions or fetch their URLs/images.

The actual `511_1` TLS/WSS regressions check durable content before publication,
identical replay, changed action data with an identical visible label, whole-page
rollback and retained cursors. A sidecar SQL trigger failure rolls back the plain
prefix too, retains local actor access and succeeds once after explicit reconnect.
Real encrypted HTTPS previews traverse `DirectoryFetcher` and the existing
SQLite actor in both preview/socket orders; 203 records cross the 200-message
chunk boundary, with direct/group info messages sharing one ID but distinct full
content. Scoped bindings never flatten or cross-link metadata, and previews
neither advance socket cursors nor acknowledge queues.

Default/store regressions verify additive v4→v5 migration preserves history,
outbox, sticker metadata and cursors; deepest accepted JSON round-trips after
restart and the next depth is rejected. Byte limits apply both per item and to
the complete batch/preview response, not independently to each chunk. Changed
raw content and plain/rich type collisions remain strict atomic conflicts.

Final gates passed **165 default Rust** (162 library + three CLI), **five IME
adapter**, **19 Python** and **374 all-feature tests** (359 library + three CLI +
eight coordinator + four provisioning), with two opt-in checks ignored. Whole
formatting, strict default/all-target/all-feature Clippy, dependency guards and
both deterministic asset/catalog checks passed. Original-source oracles passed:
nine info previews, eight routing cases, five positive/four rejected AUTH formats,
three rejected queue schemas and 18 post-AUTH cases.

Actual isolated native X11 windows displayed “Bạn đã thêm Đặng Minh vào nhóm”
through the real parser/store/page path at **1200×780 and 640×480**, including the
sidebar preview and existing contact names. Images:
`/tmp/zalo-rust-gui-check/info-readable-wide.png` and
`/tmp/zalo-rust-gui-check/info-readable-compact.png`. Both were visually reviewed;
normal close exited 0 and strace found zero Internet connects. The fixture's
Offline/ConfigurationUnsupported banner is intentional because it has no
endpoints. UI production/input code is unchanged; Wayland and real-account
acceptance remain separate. See [MIGRATION_STATUS.md](MIGRATION_STATUS.md) for the
final artifact and pending user retry.

Final LIVE release: **17,120,176 bytes**, SHA-256
`1e3dc2544b035165215da5c3ac8ae136aa15ad0523ae03df7ed24d246e0fde9a`.
Locked offline build and disposable release smoke passed schema five, synthetic
19-message/six-thread restart, additive legacy preservation, private bounded
diagnostics/executable identity, locking and informational no-write checks, with
no network use or automatic live startup. Doctor confirms login/messaging
compiled and authenticated/network-started false; Signal remains inactive in
LIVE. Previous 11291 is retained at `/tmp/zalo-rich-protocol-previous-11291` and
cannot reopen a schema-five profile; it is not a database downgrade path.

The coordinator refreshed the actual managed user-XDG **Zalo Native** launcher
and independently verified the installed executable's exact SHA-256, size and
0755 mode, plus the desktop command/icon identity. No app was launched or user
profile edited by installation. The user subsequently confirmed Online on that build; complete synchronization and later media acceptance remain separate.


## Photo/OA media and schema-six checkpoint (2026-11-04)

The source-backed media contract and its limits are in [PROTOCOL_MEDIA.md](PROTOCOL_MEDIA.md).
Final default checks passed **182 Rust** (179 library + three CLI), **five IME**
and **19 Python** tests. Complete optional-feature tests passed **404** (389 library,
three CLI, eight coordinator, four provisioning), with two opt-in checks ignored.
Final focused media tests passed **29/29**; formatting, strict default/all-target/
all-feature Clippy, dependency isolation and both generated-asset checks passed.
Original-source media/info/routing/AUTH/post-AUTH Node oracles passed; the media
oracle also executes the original nullable HTTP collection path.

Private TLS regressions cover 205 HTTP previews crossing chunk boundaries in both
socket arrival orders, scoped same-ID collisions, complete unknown metadata after
restart, unchanged legacy bodies/outbox with cursor 900, immutable raw conflicts,
and whole-page SQL rollback/local-actor access/explicit retry. Store tests retain
prior information/sticker/history/cursor/outbox data through v5→v6. Rich messages
cannot correlate to outgoing text. Codec/cache tests include real PNG/JPEG/WebP/JXL,
malformed/oversize/budget cases, no-progress guards, ancillary-box exclusion,
stale results, upload/cache budgets, global decoder serialization, cancellation
and closed-worker recovery. URL normalization expansion is bounded separately
from source URL length, retaining full raw data when a projection URL is withheld.

Reproduce native synthetic windows using the LIVE library test executable:
`ZALO_UI_FIXTURE_MEDIA=photo|webcontent|opaque|failure|mixed` selects the content
and `ZALO_UI_FIXTURE_THEME=light|dark` selects appearance for
`ui::live::tests::signed_in_offline_native_window_fixture` (`--ignored --exact`).
Use only the private Xvfb fixture workflow; this fixture has synthetic data,
predecoded JXL pixels/fixed failures and prevents asset networking. Reviewed
1200×780/640×480 images and whole-lifetime traces are under
`/tmp/zalo-rust-gui-check/media-*`. Photo/card content, compact scrolling, cached
viewer, explicit retry, unavailable state and opaque-template explanation were
checked. Every fixture closed with exit 0 and zero Internet connects; its TLS
fault banner is intentional, not production evidence. Browser links were not
launched. Earlier actual x-unikey evidence remains unchanged; Wayland is unverified.

Final locked offline LIVE release is **20,295,296 bytes**, SHA-256
`55bc767268b045b469d8de94548dfb28bbd3914d86d9715104538584b44092bd`.
Disposable release smoke passes schema six, 19 messages/six threads, additive
restart/legacy preservation, private bounded diagnostics/build identity, locking
and informational no-write checks with `network_used=false`. Doctor confirms
compiled login/messaging and no authentication/network startup. The 1e3dc2 backup
at `/tmp/zalo-cards-previous-1e3dc2` cannot reopen a schema-six database; it is not
a database downgrade path. Logs use `/tmp/zalo-cards-final-*.log`; full artifact
and production acceptance details are in [MIGRATION_STATUS.md](MIGRATION_STATUS.md).
The user confirmed Online on the prior 1e3dc2 build; this media candidate still
requires real photo/OA acceptance, and no complete-sync claim is made.

The coordinator then independently verified and installed the exact candidate in
the managed user-XDG Zalo Native launcher (same SHA-256/size, 0755 mode, correct
`Exec --live` and icon/window identity). No app/profile/account launch accompanied
installation. Real media acceptance remains pending the user's retry.

## Shared cards, poll notices and schema-seven migration (2026-11-05)

The user confirmed that photos display, while OA/shared cards still need work,
then identified `chat.recommended` and `group.poll` placeholders. These now have
distinct typed projections; complete source content remains immutable. Poll
rendering covers the embedded notice/question and explicit anonymity only;
detail/options/results retrieval and voting remain unavailable. This does not
establish universal OA-template support.

The original-reader oracle passes six recommended/poll cases; existing media
and HTTP-preview oracles pass four/eight cases. Scoped directory/WS tests
exercise both arrival orders, 205 previews across chunks, shared IDs in different
threads, full raw retention, restart, queue cursors and outgoing isolation.
All 80 storage tests, six pane-refresh regressions and 11 closed-diagnostic tests
pass. Populated v6→v7 migration copies metadata/projection/preview bytes exactly,
retains pending text/history/cursors/account scope and foreign-key cascades;
failed copy leaves v6 data/table/version unchanged. Earlier v5 upgrades pass.

Final default checks pass **189 Rust** (186 library + three CLI), **five IME**
and **19 Python** tests; all-feature checks pass **424** (409 library, three CLI,
eight coordinator and four provisioning), with two opt-in fixtures ignored.
Focused LIVE media/recommended/poll groups pass 37/four/12 tests respectively.
The initial sandbox loopback denial was reported; synthetic HTTPS/WSS tests ran
only after scoped escalation approval. No actual account/history was accessed.

Four approved isolated X11 sessions cover Recommended/Poll × Light/Dark at
1200×780 and 640×480. Reviewed long Vietnamese text, original poll glyph,
compact scrolling and per-pane drafts across resize; all closed with exit 0 and
zero Internet connects. Evidence is under `/tmp/zalo-rust-gui-check/`:
`{recommended,poll}-final-{light,dark}-*.png` and
`media-{recommended,poll}-{light,dark}-{app.log,network.trace}`. Browser links
were not launched; Wayland/real IME acceptance boundaries remain unchanged.


Current locked offline LIVE candidate is **20,349,440 bytes**, SHA-256
`f0263f81ec6166d384ba099ef584c2cdba765d5d9e8ad7de64eaa0238f8ba66b`.
Strict default/all-feature/all-target Clippy passes. Disposable release smoke
passes schema seven, 19 messages/six threads, restart/legacy preservation,
private bounded diagnostics/build identity, locking and informational no-write
checks with `network_used=false`. Logs use `/tmp/zalo-rich-final-*.log`.
The previous uninstalled e9a executable at `/tmp/zalo-content-previous-e9a`
cannot reopen schema seven. Production recommended/poll and remaining OA layouts
still require a real retry; synthetic tests establish no full-parity claim.


The coordinator independently verified and installed this exact candidate through
approved scoped tooling into the managed **Zalo Native** user-XDG launcher.
Installed SHA-256 is `f0263f81ec6166d384ba099ef584c2cdba765d5d9e8ad7de64eaa0238f8ba66b`,
size 20,349,440 bytes and mode 0755. Desktop `Exec --live`, original icon and
`StartupWMClass=zalo-linux-rust-preview` are correct; exactly four managed files
were updated. Installation did not launch the app or read/change account/history
data. Doctor reports compiled login/messaging with network startup false.
Recommended/poll and remaining OA production acceptance await the user's restart.

## Native UI contract, typography and fixed frames (2026-11-05)

The native shell now uses the original font roles (Small 12, Body 14, chat 15,
Heading 18; modal titles 16), shared source-backed dialog chrome and scoped
read-only conversation information. Wide information uses a 344-point right
sidebar; compact information replaces the chat page and returns to its draft.
QR action groups share the QR's center axis, including the actual parent login
screen. The explanatory phone tab preserves the current QR attempt and Cancel.
Modal footer rows are bounded; keyboard shortcuts, pointer navigation and direct
send dispatch cannot activate the underlying composer while a modal is open.

Accepted-source checks pass **194 default Rust** (191 library + three CLI),
**five IME adapter** and **19 Python** tests; **438 all-feature tests** (423
library, three CLI, eight coordinator and four provisioning), with two opt-in
fixtures ignored. Formatting, strict default/all-feature/all-target Clippy,
catalog/inventory reproducibility and six original-reader card cases pass.
The original catalog retains 758 tokens and 1,034 glyph aliases, zero missing
font glyphs; native palette bindings cover 31 roles/34 source tokens.

Independent synthetic GUI review covers QR/retry/phone information, direct and
group conversations, unknown/long names, directory, settings, connection faults,
Light/Dark/System/Custom and modal recovery at 1200×780 and 640×480. Actual
pointer/input/resize checks and regressions preserve selection, scoped drafts,
focus restoration and modal isolation. Current screenshots and numeric sidecars
are under `/tmp/zalo-rust-gui-check/`: `ui-contract-*`, `ui-alignment-qr-*` and
`ui-alignment-viewer-final-*`. All sessions close normally and traces contain
zero Internet connections; QR/retry use disposable localhost TLS fixtures.

Measured isolated X11 captures use effective/native pixels-per-point 1, 1.25,
1.5 and 2 without resizing screenshots. Native-window 1200→1920→640→1200 resize
at constant DPI preserves font roles, 64-point rail, 344-point list/information
sidebar, 68-point header and control hit areas. This simulates maximized geometry
without a desktop window manager. Actual texture-paint tests additionally cover
four DPIs, large/small source images, aspect ratio, source-pixel limits and
centered viewer geometry. Inline/sticker caps remain 330×240/130×130; the viewer
frame is at most 740×540, with separately scrolling captions and reachable links.

This establishes legibility/layout evidence, not native Wayland blur acceptance.
The existing font files cover the Vietnamese alphabet; no forced DPI, font
replacement or antialiasing patch was introduced. A fresh private-home x-unikey
attempt could not start its isolated display because of host graphics/inotify
failures. Earlier real IME evidence remains historical; current adapter tests
and synthetic Unicode input do not establish fresh Telex ON/OFF acceptance.
No real account/profile was opened and no Electron performance comparison was
made. New UI code retains existing workers, queues/cache limits and render-time
I/O boundaries.

The final locked offline LIVE artifact is **20,410,880 bytes**, SHA-256
`a398563ad34f6f91b7074b74ad25a3303780f0833b1ec9e62f54f765e1778638`.
Schema-seven release smoke passes 19 messages/six threads, restart/legacy
preservation, locking, bounded private diagnostics and informational no-write
checks with `network_used=false`. A separate disposable doctor check reports
compiled authentication/messaging, no authenticated session/network startup and
no profile writes. Logs/proof use `/tmp/zalo-ui-accepted-*`. The previous
installed executable was archived at `/tmp/zalo-ui-guidelines-previous-f026`;
both builds use schema seven. Production Wayland font clarity and real-account
recommended/poll/OA acceptance remain separate from this synthetic checkpoint.

The coordinator independently verified and installed this exact artifact through
approved scoped tooling into **Zalo Native**: SHA-256
`a398563ad34f6f91b7074b74ad25a3303780f0833b1ec9e62f54f765e1778638`,
20,410,880 bytes, mode 0755. Desktop `Exec --live`, original icon and
`StartupWMClass=zalo-linux-rust-preview` match the managed installation.
Installation did not launch the app or open personal profile/account data.

## Linux Wayland window decorations (2026-11-05)

Linux production dependencies now enable winit's `wayland-csd-adwaita` feature,
restoring its normal Adwaita client decorations when the compositor requests
client-side decoration. Previously, eframe's disabled default features left
winit using the basic fallback frame. Server decorations, native DPI, viewport
defaults and the app's Zalo theme remain unchanged.

Independent default/LIVE feature graphs use only normal/build edges and confirm
Adwaita plus `ab_glyph` title rendering. The lock adds exactly five registry
packages; all existing versions/checksums remain unchanged. Upstream auto mode
reads portal appearance/button layout and GNOME title-font preferences; this is
not full arbitrary GTK CSS support or a live settings-watch implementation.

Final checks pass **194 default Rust**, **410 LIVE Rust** (407 library + three
CLI, two opt-in fixtures ignored), **five IME adapter** and **19 Python** tests,
formatting and strict default/LIVE all-target Clippy. The locked offline LIVE
artifact is **20,771,424 bytes**, SHA-256
`c6520a7b7caa4a68a9027ad68caf46cd0cced034632fef417ba45e8bde0591bd`.
Schema-seven smoke passes restart/legacy preservation, 19 messages/six threads,
locking, private bounded diagnostics and informational no-write checks with
`network_used=false`. Separate disposable doctor confirms compiled live support,
no authentication/network startup and no profile writes.

Approved isolated X11 startup/resize/close at 1200×780 and 640×480 exits 0 with
zero network connects. Images are `gnome-csd-x11-regression-{wide,compact}.png`
under `/tmp/zalo-rust-gui-check/`; they verify client regression, not Wayland
titlebar appearance. Native GNOME Wayland visual acceptance remains pending.
No real profile/account was opened. Logs/proof use `/tmp/zalo-gnome-*`; the
previous schema-seven executable is `/tmp/zalo-gnome-previous-a398`.

The coordinator installed and independently verified this exact candidate with
approved scoped tooling: SHA-256
`c6520a7b7caa4a68a9027ad68caf46cd0cced034632fef417ba45e8bde0591bd`,
20,771,424 bytes and mode 0755. The managed **Zalo Native** desktop launcher
retains its installed `Exec --live`, original icon and matching window identity.
Installation did not launch the app or open account/profile data; native GNOME
Wayland appearance awaits the user's restart.

## Initial 16:9 window (2026-11-06)

Production startup now requests a **1280×720 logical-point content area**;
the minimum remains 640×480 and normal resizing/native DPI remain unchanged.
Formatting, locked offline LIVE all-target `cargo check`, three CLI tests,
release build and schema-seven disposable smoke pass. No new cosmetic assertion
test or repeat of the unrelated full feature matrix was added.

The actual production release, launched with `--demo` and a disposable profile,
initially creates a 1280×720 client window on isolated X11 at scale 1/96 DPI.
The driver sends no resize command and uses no fixture viewport override.
`default169-production.png` and its JSON sidecar under
`/tmp/zalo-rust-gui-check/` record the full unscaled content image. Normal close
exits 0, releases the profile lock and makes zero Internet connections.

Artifact SHA-256 is
`473377efbf4c2ca4f82b13d4b5f3c4816a2c04339331a7ff8a9e77d408ef67bd`,
**20,771,424 bytes**. Smoke preserves schema seven, restart/legacy history,
19 messages/six threads, private diagnostics and informational no-write behavior
with `network_used=false`. Logs/proof use `/tmp/zalo-default169-*`; the previous
compatible executable is `/tmp/zalo-default169-previous-c652`. No personal
profile/account was opened; GNOME Wayland decoration acceptance remains separate.

The coordinator installed and independently verified this exact artifact (SHA-256 `473377efbf4c2ca4f82b13d4b5f3c4816a2c04339331a7ff8a9e77d408ef67bd`, 20,771,424 bytes, mode 0755) without launching the app or opening account/profile data.

## Avatar status and account menus (2026-11-06)

Signed-in status now appears on the rail avatar; its popup exposes connection
details and recovery. The persistent status banner and account/logout footer,
including reserved space, are removed. The gear opens separate appearance and
logout actions; logout still requires confirmation. Signed-out users only see
appearance. The palette adds four original-source roles (35 roles/38 tokens).

Seven pointer/state regressions cover menu routing, confirmation cancellation,
draft/session preservation, blocked background sending/navigation, outside/Esc/
same-anchor dismissal, scoped focus restoration, stale account/auth/thread
invalidation, resize anchoring and fully clipped recovery controls. Final checks
pass **194 default Rust**, **445 all-feature Rust**, **five IME adapter** and
**19 Python** tests, formatting, strict default/all-feature all-target Clippy,
and catalog/inventory reproducibility. Two opt-in native fixtures remain ignored
in the normal suite.

Approved isolated X11 captures cover Light/Dark, synthetic Online/Offline and
1200×780/640×480. Online is a test-only display override, not transport evidence.
The refreshed Offline status popups keep both recovery actions visible after
gear-to-status switching. All sessions close normally with zero network
connects. Images/logs use `/tmp/zalo-rust-gui-check/account-*`; final gate proofs
use `/tmp/zalo-account-final-*`. No personal profile/account was opened. Existing
native Wayland and fresh real x-unikey acceptance boundaries remain unchanged.

The locked offline LIVE artifact is **20,778,592 bytes**, SHA-256
`f7d840780939684ede192070bb823c6e917bec372cf990507adcc89ceb61d5f6`.
Schema-seven smoke passes restart/legacy preservation, 19 messages/six threads,
locking, private bounded diagnostics and informational no-write checks with
`network_used=false`. Separate disposable doctor confirms compiled live support,
no authentication/network startup and no profile writes.

The coordinator installed and independently verified this exact artifact (SHA-256 `f7d840780939684ede192070bb823c6e917bec372cf990507adcc89ceb61d5f6`, 20,778,592 bytes, mode 0755) with matching Zalo Native launcher/icon/window identity, without launching the app or opening account/profile data.

## Mobile hidden conversations and launcher feedback (2026-11-06)

Fresh live workers require a successful source-backed hidden-conversation
snapshot before exposing cached names, history or media. Failed checks retain a
neutral workspace. Normal lists exclude every hidden thread; explicit search
returns bounded name/lock candidates without previews. The existing four-digit
Zalo PIN authorizes only the requested thread. Cancellation, navigation, worker
loss, expiry and changed policy revoke access. PIN/verifier material is neither
logged nor persisted. Policy uses Rust memory and SQLite TEMP memory, with no
schema change or SQLite encryption. Periodic refresh waits about 30 seconds
after completion; HTTP completion adds latency, so this is not an instant or
maximum-30-second remote-change guarantee.

The independent original-JavaScript oracle covers snapshot replacement and PIN
semantics. Real loopback TLS tests verify encrypted get-all requests, incorrect
PIN, cancellation, one-thread authorization, unchanged refresh, remote revocation
and fail-closed HTTP errors. Store regressions verify filtering before LIMIT,
account scope, full persistent-row preservation, receive cursors/outbox/ACKs,
memory-only policy and idempotent initialization. Eleven UI regressions exercise
masked input, IME/repeated Enter, stale replies, scoped caches, queued ACKs after
relock, actual row clicks and modal hit layers. Default checks pass 193 library
and three CLI tests, five IME adapter tests and 19 Python tooling tests. The final
all-feature suite passes 450 library, three CLI and 12 integration tests; three
opt-in native fixtures remain ignored. Formatting, strict default/all-feature
all-target Clippy, catalog/inventory reproducibility and the source oracle pass.
After two equivalent lint corrections, ten hidden-flow and eleven UI tests also
pass against the final sources.

Approved isolated X11 captures cover Light/Dark, 1200×780/640×480, minimal search,
wrong PIN, pending cancellation, actual private title/body after unlock, relock,
and bounded single Checking/Failed workspaces. Cancellation traces establish
pending→closed before any grant. Every accepted session closes normally with
zero network connections. These fake-worker presentation fixtures use the real
UI event handler; backend TLS tests provide separate protocol evidence. Images
and binary provenance are under `/tmp/zalo-rust-gui-check/privacy-*`; aborted or
incorrectly named intermediate captures are not acceptance evidence.

The managed launcher now sets `StartupNotify=false` instead of advertising an
unimplemented startup-completion handshake. Five installer tests include actual
GLib parsing and replacement of an older managed entry. GLib launches an
isolated disposable `--demo` entry, closes normally and releases its profile
lock with no Internet connection; its trace includes one refused local X11
startup probe. No real desktop entry/account/profile was launched. Actual GNOME
cursor behavior, mobile hidden/PIN acceptance and native Wayland rendering
remain user-verification boundaries; existing IME evidence is unchanged.

The locked offline LIVE artifact is **20,946,144 bytes**, SHA-256
`23e3a95a8620ddaa6780977354587e5b11c3465570a73096528623d5b437d3dc`.
Schema-seven smoke passes restart/legacy preservation, 19 messages/six threads,
locking and private bounded diagnostics with `network_used=false`. A separate
disposable doctor confirms compiled live support, no authentication/network
startup and an empty profile root. Gate/artifact/doctor proofs use
`/tmp/zalo-hidden-final-*`; the previous compatible executable is
`/tmp/zalo-hidden-previous-f7d8`. No real account, PIN or profile was inspected.

The coordinator installed and independently verified this exact artifact (SHA-256 `23e3a95a8620ddaa6780977354587e5b11c3465570a73096528623d5b437d3dc`, 20,946,144 bytes, mode 0755), matching Zalo Native launcher/icon/window identity and `StartupNotify=false`, without launching the app or opening account/profile data.

## File cards, identity photos and send verification (2026-11-06)

Incoming `share.file` now projects the source filename, extension, known size and
folder status. A supported link opens the browser only after an explicit click;
missing links and encrypted-file markers retain metadata with an unavailable
explanation. The original-reader oracle passes eight synthetic title/URL cases.
Complete content remains immutable in the sidecar. Schema eight widens its CHECK
constraint; populated v6/v7 upgrades preserve existing bytes, account scope,
outbox and cursors. Independent v7 tests cover repeated startup, foreign-key
cascades, failed-copy rollback and refusing a newer schema. A schema-seven binary
cannot reopen an upgraded database; copying back an old executable is not a
database downgrade.

Visible account, direct-contact and group avatars use scoped directory snapshots
and bounded background decoding. The own-profile request is optional and does
not create a friend entry. Metadata batches contain at most 64 identities;
avatar output is capped at 256 pixels, with 64 GPU textures/a 4 MiB pixel budget.
Tests cover direct/group ID collisions, account and policy changes, stale URLs
and results, empty replies and queue saturation. Initials
remain the fallback. A whole conversation row is one interactive widget: actual
pointer checks cover avatar, title, preview, time and both padding areas.

Source terminal eligibility flags absent/null/zero now finish verification;
all four identity lists remain mandatory. The independent 11-case original
pagination oracle confirms that first-page missing/null lists throw in the source
reader, while later-page merging supplies defaults. Real loopback HTTPS/WSS tests
verify successful plaintext sends for the supported terminal flags, and no HTTP
send/outbox preparation for incomplete lists, an encrypted target or intervening
encryption events. Such events are not consumed or acknowledged. Fixed diagnostic
reasons distinguish confirmed encryption from unsupported metadata, mismatched
replies, timeout, pagination and intervening events. No real-account cause or
production E2EE support is established by these fixtures.

Final checks pass **204 default Rust**, **489 all-feature Rust**, **five IME
adapter** and **19 Python** tests, formatting, strict default/all-feature
all-target Clippy, original-reader oracles and catalog/inventory reproducibility.
Three opt-in native fixtures remain ignored in normal suites. An earlier
parallel runtime-only run missed an expected disk diagnostic row; its isolated
rerun and the final full matrix pass. The bounded global logger was unchanged.
Earlier failed assertions and gate logs remain under `/tmp/zalo-files-*`.

Approved isolated X11 captures cover Light/Dark at 1200×780 and 640×480, actual
row navigation, long filenames, unavailable/encrypted cards, identity photos,
scrolling, a short content-sized outgoing bubble and composer heights 100/132.
Resize to 1920×1080 preserves logical typography. Native scales 1/1.25/1.5/2
produce unresampled physical screenshots; sidecars record measured scale and
dimensions. All sessions close normally with zero Internet or loopback connects.
These are synthetic presentation fixtures, not a live login or fresh IME test.
The separate redesign guide remains a future target, not an implemented palette.

GUI proofs use `/tmp/zalo-rust-gui-check/files-avatars-*`. The reviewed immutable
LIVE test fixture has SHA-256 `a1fbd59cec65ea3fcecb2e6bdf23de93a06696adbf10bbf81400478008333d80`;
the final all-feature fixture is `1ad4da03407fecde689030679717e3688742cdbfff29ff7dce1be24b1726ab64`.
Fresh final captures match every wide pixel and compact content pixels; the only
compact difference is the two-pixel scrollbar tint. Cross-check and DPI JSON
proofs document this distinction. Read-only GNOME inspection reports a native
1920×1080 monitor at scale 1, text scale 1; no global font/DPI setting changed.
Original Segoe faces and toolkit rasterization remain in use. This does not
establish that the user's Wayland font-perception complaint is resolved.

The locked offline LIVE release is **21,046,272 bytes**, SHA-256
`25ed50c0d2cfc80db01fc1548597ca0ae1c8e0c100319d11858aaaa624637d71`.
Schema-eight smoke passes 19 messages/six conversations, restart/legacy
preservation, locking and private bounded diagnostics with `network_used=false`.
A separate disposable doctor reports compiled authentication/messaging, no
authentication/network startup and an empty profile root. Artifact, smoke,
doctor and frozen-source manifests use `/tmp/zalo-files-final-*.json`. No personal
account, PIN, conversation history or profile was opened for this checkpoint.

The coordinator installed and independently verified this exact release in the
managed **Zalo Native** launcher: SHA-256
`25ed50c0d2cfc80db01fc1548597ca0ae1c8e0c100319d11858aaaa624637d71`,
21,046,272 bytes, mode 0755. `Exec --live`, icon/`StartupWMClass`, managed ownership
and `StartupNotify=false` match; original 256/512-pixel icons match their source
files. Installed doctor uses a disposable profile and reports compiled live
support with authentication/network startup false. Installation did not launch
the app or open personal profile/login data. Real file/avatar/send acceptance
awaits the user's restart.

## Redesign migration evidence (2026-11-06)

Synthetic evidence for the [redesign guide](UI_REDESIGN_GUIDE.md); not real-account or Wayland evidence.

- `./scripts/rust/check.sh` (formatting, tests, strict Clippy, offline smoke) and
  `cargo test --locked --offline --all-features` plus `cargo clippy --all-features --all-targets -- -D warnings`
  pass with Rust 1.99.1. New regressions: redesign token values, WCAG AA contrast for the main text/surface pairs in
  Light and Dark (`theme::tests`), composer card heights, and the updated rail/header/composer geometry assertions in
  `layout_tests.rs` and `account_tests.rs`.
- Screenshots of the offline demo come from the app's own framebuffer, never the desktop. The ignored fixtures open one
  window for about a second:

  ```text
  env -u WAYLAND_DISPLAY ZALO_UI_SHOT=/tmp/shot.ppm ZALO_UI_SHOT_THEME=dark ZALO_UI_SHOT_SIZE=640x480 \
    cargo test --locked --offline --lib -- --ignored --exact ui::shot_fixture::demo_screenshot
  ```

  Extra switches: `ZALO_UI_SHOT_SCENE=settings|info`, `ZALO_UI_SHOT_POINTER=x,y` (hover), `ZALO_UI_SHOT_PRESS=1`,
  `ZALO_UI_SHOT_TABS=n` (keyboard focus). The signed-in/login fixture (`ui::live::tests::signed_in_offline_native_window_fixture`,
  `--all-features`) honours `ZALO_UI_SHOT` too, with `ZALO_UI_FIXTURE_LOGIN=qr` for the QR screen.
- Reviewed: 1200×780 and 640×480 in both themes; row hover, pressed rail button, keyboard focus ring, the settings modal,
  the conversation information sidebar, QR login and the contact directory (wide and compact). A run with `DISPLAY` unset
  rendered the same demo on native Wayland. The directory capture uses `ZALO_UI_FIXTURE_READY=1`, which forces the verified
  privacy state and invented contact rows each frame because the offline worker can never verify privacy; it is a visual
  state only. Not covered: IME on Wayland and any real account or real desktop session.

## Package evidence (v2.0.0, 2026-11-06)

`./scripts/rust/package.sh` compiles the live release inside a pinned Ubuntu 20.04 container (binary needs only
`GLIBC_2.30`), builds the `.deb` there and the `.rpm` in a pinned Fedora container, and writes `dist/rust/SHA256SUMS`.
`./scripts/rust/package/verify.sh` then installs each package through `apt`/`dnf` in clean containers, runs
`zalo-native --version` and `--doctor` (live compiled, not authenticated, no network), checks the desktop entry, icon
and `ldd`, runs `rpm -V` for RPMs and removes the package. Passed on Ubuntu 20.04/22.04/24.04, Debian 12 (`.deb`) and
Fedora 42, AlmaLinux 9 (`.rpm`). Debian 11 could not be tested: its apt mirrors return 404. Container installs say nothing
about GUI behaviour on real desktops; use the demo window evidence above for that.

## Saved-history search pagination (2026-11-09)

The information panel now offers **Tìm cũ hơn** and **Mới nhất**. Search retains one
page of at most 50 hits, evaluates at most 20,000 saved rows per request plus one
lookahead row, and continues strictly before the last evaluated `(sent_at_ms, message_id)`.
An empty scan page can continue. New arrivals do not shift older pages, changing
the query cannot reuse an old boundary, and failed continuation preserves the
previous page for retry. Each request rechecks account/thread privacy; no schema,
remote search, receipt or encryption behavior changes.

- Focused all-feature search checks: **26 passed**. Regressions cover timestamp
  ties, concurrent newer arrivals, empty scan windows, exact exhaustion, hidden
  relocking, malformed boundaries, backpressure, stale replies, worker pagination
  and Light/Dark pointer interaction at sidebar/compact widths.
- Default Rust checks: **244 library, 3 CLI and 3 schema-upgrade tests passed**;
  **five IME adapter** and **19 Python tooling tests** passed. Strict
  all-target/all-feature Clippy passed with Rust 1.99.1. The complete
  `./scripts/rust/check.sh` exited successfully, including informational no-write
  checks and the headless offline demo smoke.
- The unfiltered all-feature command stopped in its library phase with **582 passed,
  one failed and four ignored**. The failure is
  `app::live::runtime::integration_tests::incomplete_or_encrypted_capability_never_sends_or_prepares_outbox`,
  at its expected `đã bật mã hóa` error text. The same exact test fails on an
  unmodified `0c25bc6` HEAD extracted to a disposable directory, with the same
  assertion. HEAD already routes encrypted targets through `send_e2ee` when
  `signal-crypto` is enabled. Neither that route nor its test was changed here.
  A subsequent run with only that exact test excluded passed **601 tests** across
  library, CLI, schema and Signal integration targets (four ignored, one explicitly
  filtered). This checkpoint does **not** claim an entirely passing unfiltered
  all-feature suite.
- Six app-framebuffer screenshots were inspected under X11: Light/Dark at
  **1200×780** and **640×480**, plus older-page/end-of-results captures at wide
  Light and compact Dark. They show only seeded demo messages; no real profile,
  login or desktop capture was used. Compact results remain inside the existing
  scrollable information page. Files/logs use `/tmp/zalo-search-pagination-*`.

To reproduce the pagination screenshots, use the ignored `ui::shot_fixture::demo_screenshot`
with `ZALO_UI_SHOT_SCENE=search-pages`; add `ZALO_UI_SHOT_OLDER=1` for the second
page. The fixture seeds 75 matches in its disposable demo profile. This is local
storage/UI evidence, not real-account or Wayland acceptance.


## Original-photo saving (2026-11-09)

Received photo cards and the cached native viewer now offer **Tải ảnh gốc** for an
available original URL accepted by the existing Zalo CDN policy. They share one
transfer keyed by account/thread/message/original URL, with progress, cancel,
retry and saved location. This preserves original bytes (including JXL), caps
transfers at 100 MiB, uses sanitized collision-safe filenames and never falls
back to the thumbnail. Cancel interrupts a stalled response and cleans the
partial file; unsolicited HTTP 206 is rejected. No schema or sending changes.

Six new regressions cover original-byte fidelity/partial responses, cancellation
before connection and during a stalled body, filenames without signed queries,
card/viewer shared state with no automatic transfer in Light/Dark at both sizes,
and stale account/thread requests/results. Existing image geometry/DPI and file
card checks also pass. Synthetic fixture mode refuses public-CDN downloads.

Verification results:

- Unfiltered all-feature library run: **588 passed, one failed, four ignored**.
  The failure is the previously documented encrypted-capability test at
  `integration_tests.rs:4670` (expected `đã bật mã hóa` text). All six new
  regressions passed.
- A repeat excluding only that known E2EE test, with two test threads, finished
  **587 passed, one failed, four ignored, one filtered**. The additional failure
  is `terminal_transport_failures_keep_one_local_actor_alive_until_explicit_shutdown`
  at line 3252, which expects two or three hidden-policy requests. That test passed
  in the initial full run and in its separate focused run. Its result is unstable;
  this change does not claim an entirely green all-feature suite.
- `./scripts/rust/check.sh` exited successfully: **244 default library, three
  CLI, three schema-upgrade, five IME adapter and 19 Python tooling tests** passed,
  along with formatting, strict default Clippy, dependency guards, informational
  no-write CLI checks and the headless offline demo.
- All-feature schema/Signal integration targets: **16 passed**. Strict
  all-target/all-feature Clippy passed. Logs are in `/tmp/zalo-photo-*`.

Eight native app-framebuffer screenshots were inspected under X11: photo card
and viewer at **1200×780** and **640×480**, each in Light/Dark. Save actions remain
visible, images retain their aspect ratio and are not enlarged above source
pixels. Only synthetic profiles/pixels were used. The fixture does not log in or
capture the desktop. Screenshots/logs use `/tmp/zalo-photo-*`.

Reproduce with the ignored
`ui::live::tests::signed_in_offline_native_window_fixture` in an all-feature test
binary, `ZALO_UI_FIXTURE_READY=1`, `ZALO_UI_FIXTURE_MEDIA=photo`,
`ZALO_UI_FIXTURE_WIDTH=1200|640`, `ZALO_UI_FIXTURE_THEME=light|dark`, and
`ZALO_UI_SHOT=/tmp/zalo-photo-check.ppm`; add
`ZALO_UI_FIXTURE_PHOTO_VIEWER=1` for the viewer. Original CDN signed URLs,
cloud-specific headers and encrypted attachments still need real-service evidence.


## Message clipboard menu (2026-11-09)

The native message bubble now has an explicit secondary-click menu for copying
plain text, photo title/caption, file name, card/poll presentation text and an
available validated http(s) link from a photo/file/card projection. The original counterparts are `_handleCopy`,
`handleCopyPhotoCaption`, `_handleCopyLink` and `_handleCopyOA` in
`pc-dist/compact-app-pc.e08d0d44f38873747a6b.js:5406–5485`.
No raw sidecar/HTML, encryption keys, sticker/unsupported-message placeholders,
image pixels or remote actions are copied. Encrypted file URLs are withheld.

The popup retains IDs/scope rather than a message payload, rechecks the current
page/account/thread/generation/authentication/privacy before rendering, and
closes when another modal opens. Its widget IDs carry the full scope. Escape or
outside click closes it; background composition/sending is blocked and existing
scope checks gate focus restoration. Secondary clicks respect the actual bubble,
clip rectangle and foreground layers. Buttons are 44 logical pixels; the popup
is constrained to the viewport. Tab/Shift+Tab stays within its actions, including
wraparound; this explicitly avoids egui 0.29 disabled-background Tab starvation.

Synthetic regression checks cover exact Vietnamese/emoji/newline fidelity, safe
link handling and encrypted-link suppression, actual pointer-to-clipboard output
at both sizes/themes, stale scopes/locked privacy/modals, draft preservation,
Escape/background Enter, keyboard copy, foreground-window occlusion and disabled
photo-viewer links/download actions behind the menu. These
verify the egui clipboard output, not an external application's OS paste behavior.

Eight native X11 app-framebuffer screenshots were inspected at **1200×780** and
**640×480** in Light/Dark, showing both caption and link actions with no clipping,
including the disabled photo viewer behind the menu.
Only a synthetic profile/photo was used; no login, private account, desktop
capture, network fetch or clipboard write was needed for the screenshots. Files
and logs use `/tmp/zalo-message-menu-*`, `/tmp/zalo-message-viewer-menu-*` and
`/tmp/zalo-message-actions-*`.
Reproduce with the all-feature ignored
`ui::live::tests::signed_in_offline_native_window_fixture`,
`ZALO_UI_FIXTURE_READY=1`, `ZALO_UI_FIXTURE_MEDIA=photo`,
`ZALO_UI_FIXTURE_MESSAGE_MENU=1`, `ZALO_UI_FIXTURE_WIDTH=1200|640`,
`ZALO_UI_FIXTURE_THEME=light|dark`, and `ZALO_UI_SHOT=/tmp/menu.ppm`.

Verification on Rust 1.99.1:

- All-feature UI checks: **126 passed, three ignored**; all eight message-menu
  regressions and the disabled-viewer regression passed.
- Directory checks with two test threads: **eight passed**. Earlier runs under
  higher parallel load hit loopback fixture timeouts; the final complete run
  with four test threads passed all directory checks.
- The unfiltered all-feature library run finished with **597 passed, one failed
  and four ignored**. The sole failure is the previously reproduced baseline
  `app::live::runtime::integration_tests::incomplete_or_encrypted_capability_never_sends_or_prepares_outbox`
  assertion described above. Its E2EE route/test is unchanged. The all-feature
  schema/Signal integration targets separately passed **16 tests**.
- The complete `scripts/rust/check.sh` passed: **247 default library tests,
  three CLI tests, three schema tests, five IME adapter tests and 19 Python
  tooling tests**, formatting/dependency guards, strict default Clippy and
  informational no-write/headless demo smoke checks. An earlier invocation
  exited 143 before IME/lint/smoke; the final complete rerun exited zero.
- Strict `cargo clippy --locked --offline --all-features --all-targets --
  -D warnings` passed. No real-server messaging or external OS paste was tested.

## E2EE send acknowledgement and accepted-send recovery (2026-11-11)

The exact original `parseE2eeResp` normalizer passes ten synthetic metadata
cases in `tests/fixtures/messaging/send_ack_oracle.cjs`. It already supports
`minGlobalMsgId`; this milestone adds request binding when `clientId` is present
and refuses conflicting server IDs. The original helper instead lets an explicit
`msgId` override its normalized ID; native rejection is deliberately stricter.
Replies without `clientId` retain the existing synchronous HTTP-request binding.

A validated server acknowledgement followed by a local persistence failure is
now reported as server-accepted, not unknown delivery. A SQLite insertion-failure
test verifies that the persisted ACK and exact ciphertext survive, dispatch is
not reopened, and later echo reconciliation creates one message. Automatic
startup reconciliation of acknowledged rows remains future work. Actual UI-event
tests verify that accepted failures enter delivery review, retain newer draft
edits, reject stale account/request failures and do not reopen a dismissed review
on duplicate events. No plaintext downgrade or automatic resend was added.

Verification used pinned Rust **1.99.1**, locked offline dependencies and disposable
synthetic storage/local loopback fixtures:

```sh
node tests/fixtures/messaging/send_ack_oracle.cjs
cargo test --locked --offline --features messaging --test e2ee_send_ack
cargo test --locked --offline --all-features e2ee -- --test-threads=2
cargo test --locked --offline --all-features -- --test-threads=2
cargo clippy --locked --offline --all-features --all-targets -- -D warnings
cargo fmt --all -- --check
```

The new external response suite passes **two tests**; the focused E2EE run passes
**50** library tests. The complete matrix passes **624 tests**: 603 library,
three CLI, two acknowledgement integration, three schema, eight crypto, four
provisioning and one receive; four native fixtures remain ignored. The previously
documented capability-test failure also passes in this run with two test threads;
its production behavior was not changed by this milestone. Strict Clippy and
formatting pass. Logs are in ignored `target/e2ee-qa-{targeted,full,clippy}.log`.

The sandbox run invoked installed toolchain binaries directly, with explicit
`RUSTC`/`RUSTDOC` and isolated `CARGO_HOME=target/e2ee-qa-cargo` reusing the existing
registry/Git cache. No dependency download or home-directory toolchain update
was needed. No personal account, key replacement, real-server message/ACK,
GUI capture, packaging, installation, commit or push was performed. These checks
do not establish production E2EE interoperability or completion of the full plan.

## Local acknowledged-send startup repair (2026-11-11)

Startup now repairs current-account acknowledged chat outbox rows before loading
the existing provisional privacy state or publishing history. The local database
actor processes at most 64 rows per job with a client-ID keyset cursor. It reuses
the atomic, idempotent echo commit and never dispatches HTTP, encrypts again or
emits `Sent`/`SendSettled`. Initialization acknowledgements (`init-*`), unconfirmed
states and other accounts are excluded. The existing remembered-hidden-policy
behavior is unchanged; this milestone does not impose a fresh-server privacy
check. Because ACK rows do not retain the server timestamp, repair keeps any
existing message timestamp or uses the original local draft creation time.

Three independent disposable-database integration tests cover restart and
account/init/state isolation, unchanged ciphertext/metadata/ratchet/cursor bytes,
a 64+3-row backlog across reopening, and an insertion failure that retains its
ACK while keeping prior committed progress. Three inline store tests additionally
cover invalid batch bounds and rejecting an oversized stored body before loading
it. Two real local TLS runtime tests verify all 130 accepted rows are restored
before the first privacy-filtered `Ready`, and that a failed insertion stops
startup before any network request, upgrade or history publication. No recovered
message settles a current UI draft. Cancellation can interrupt between actor
jobs; an already running bounded job may finish its durable commits.

Verification used Rust 1.99.1 and the isolated cache/direct toolchain setup from
the preceding checkpoint:

```sh
cargo test --locked --offline --test e2ee_echo_recovery -- --test-threads=2
cargo test --locked --offline --all-features acknowledgement_repair -- --test-threads=2
cargo test --locked --offline --all-features startup_ -- --test-threads=2
cargo test --locked --offline --all-features -- --test-threads=2
cargo clippy --locked --offline --all-features --all-targets -- -D warnings
cargo check --locked --offline --features live --all-targets
cargo fmt --all -- --check
```

The default-feature external suite passes **three tests**; store repair passes
**three** and the startup filter passes **ten**, with one public-network fixture
ignored. The final all-feature matrix passes **632 tests**: 608 library, three
CLI, three recovery integration, two ACK integration, three schema, eight crypto,
four provisioning and one receive. Four opt-in fixtures remain ignored. Strict
Clippy, the `live` build without Signal features, formatting and diff checks pass.
Logs are in ignored `target/ack-recovery-qa-{full,clippy,live-check}.log`.

An initial socket-test invocation lacked sandbox permission and stopped at
loopback bind with `PermissionDenied`. Work stopped until the user explicitly
authorized loopback testing; the runtime tests and full matrix then ran with the
appropriate scoped permission and passed. Only local fixtures and synthetic data
were used. No Zalo service, personal profile, real-server send/ACK, key rotation,
GUI capture, installation, commit or push was involved. ACKs never saved locally
still cannot be reconstructed, and per-device E2EE session readiness remains a
separate unfinished plan item.

## Checked composer device readiness (2026-11-11)

The normal encrypted composer now verifies a complete, bounded fresh 563 device
inventory, correlated 560 compatibility and fresh 541 bundles before planning a
send. A missing peer session initializes only that device; cached unknown/stale
IIDs, changed identities/registrations, missing own sessions, incomplete inventory
and non-inactivity refusals fail closed. Only peer refusal `-7` permits omission;
`-8` does not. Engine output must contain exactly the verified peer and own-device
copies with matching registration IDs. The coordinator's canonical namespace
order is preserved. An actual loopback HTTPS test verifies all three destinations,
IIDs, exact durable ciphertext bytes and a correlated scoped `Sent`; a stale
follow-up sends no HTTP and reserves no additional outbox row.

Real-engine regressions retain exact account, binding, ratchet, ciphertext, outbox,
message and cursor snapshots when an opaque session disagrees with its fresh
registration or a typed initialization remains unknown across restart. The engine
rejects that genuine session mismatch with `IdentityChanged` before returning
ciphertext; independent malformed-IID, duplicate and missing-record checks exercise
the additional prepared-record guard. Supported intervening messages are processed
during readiness and initialization. A failed incoming SQL write emits one scoped
`SendFailed`, propagates `StorageContext` to the existing transport-pause handler,
and sends no queue ACK, initialization or chat HTTP request.

Schema nine adds typed initialization intent and a scoped index. Its marker is
written before session commit; ACK confirmation and retirement are atomic.
Independent disk fixtures verify v8→v9 byte preservation and reopen, rollback after
table creation, account-scoped foreign-key cascade and unchanged refusal of future
schema ten. Ordinary unknown chat rows are never classified by their body. Legacy
unmarked unknown initialization cannot be reconstructed by this migration.

Verification used pinned Rust 1.99.1, direct `RUSTC`/`RUSTDOC`, the existing isolated
`CARGO_HOME=target/e2ee-qa-cargo`, locked offline dependencies and synthetic profiles:

```sh
node tests/fixtures/messaging/device_readiness_oracle.cjs
cargo test --locked --offline --all-features readiness -- --test-threads=2
cargo test --locked --offline --all-features typed_init -- --test-threads=2
cargo test --locked --offline --features messaging --test e2ee_device_readiness
cargo test --locked --offline --test e2ee_readiness_schema_upgrade
cargo test --locked --offline --all-features -- --test-threads=2
cargo clippy --locked --offline --all-features --all-targets -- -D warnings
cargo check --locked --offline --features live --all-targets
```

The original-source oracle passes **10 refusal and seven metadata cases**. Targeted
readiness passes **seven**, typed-init **four** (including the restart case), parser
**four** and default-feature migration **four**. The complete matrix passes **652
tests**: 620 library, three CLI, four parser, three recovery, four readiness-schema,
two ACK, three other schema, eight crypto, four provisioning and one receive.
Four opt-in fixtures remain ignored. Strict Clippy and the `live` all-target check
pass; its dependency graph excludes Signal, QuickJS and libsignal-core. A final
test-only lexical mutex scope correction was rechecked with Clippy and the actual
composer target. Logs use ignored `target/device-readiness-qa-*-final.log` plus
`device-readiness-qa-live-{check,tree}.log`.

The full-run fixture corrections wait for fresh startup privacy completion before
advancing virtual time and for the second completed HTTP response before checking
the original request bounds. Expected peer disconnects during initial fixture
AUTH are accepted only for broken pipe, connection reset or unexpected EOF; other
errors still fail. Production timers and route/count assertions are unchanged.

An automatic permission review timed out before launching a final test command.
Work stopped under Rule #0.6 until the user explicitly authorized retry; all socket
gates above subsequently ran with scoped loopback permission. No real Zalo service,
personal profile, installation, commit or push was used. This is partial D2, not
real-server acceptance: automatic stale-session renewal, identity-change prompts,
full offline-queue integration and atomic staging of a new first-use peer binding
remain unimplemented. Historical explicit probe selection paths retain their prior
coverage helpers; ordinary `live` builds still exclude the optional Signal engine.

A fresh `cargo build --locked --offline --release --features live` and
`python3 scripts/rust/smoke-release.py --expect-live` (from the repository root)
pass with schema **nine**, 19 synthetic messages and six conversations across
restart. The smoke verifies private permissions, external process-lock refusal,
informational-command inertness and preservation of legacy/local history; it
reports `network_used=false`. The new 21,591,264-byte artifact is
`target/release/zalo-rust`, SHA-256
`a7699adf8c6096cb7f51fdb21f4da51f309eb6ccf0e24330862a8335b3d17e17`.
It was not installed or opened with a real profile. Final whole-crate formatting
and diff checks also pass; release/smoke logs are
`target/device-readiness-qa-{release,smoke}.log`.

## Essential send continuation: checked renewal and own devices (2026-11-12)

The optional `signal-crypto` normal composer now renews an existing session when
fresh 563 metadata has no IID, provided fresh re-init 541 data matches its trusted
IID and identity. Only selected devices renew; the original engine archives their
previous sessions without changing trusted namespaces or known-good sessions.
A stored engine timestamp newer than its binding rejects renewal before any
reservation, because the engine would retain the old open session in that case.
Missing own-device sessions initialize first, followed by peer initialization,
with a separate correlated 543 service confirmation for each UID group.

The original-library oracle independently exercises equal/older timestamp
behavior, closed-session retention, and trust/signature rejection. Real-engine
runtime cases check targeted renewal, unmodified unaffected devices, prepared
recipient coverage, changed IID/identity and invalid-signature refusal, the newer
timestamp guard, and interrupted peer/own initialization. The disk integration
case checks that a failed renewal leaves existing trust/ratchet bytes untouched;
successful renewed init ciphertext decrypts correctly; an unknown dispatch keeps
its exact bytes and typed intent across restart, refuses redispatch and wrong-scope
confirmation, and never becomes chat history. No real account or server was used.

Verification used the same pinned Rust 1.99.1, direct compiler paths, isolated
`target/e2ee-qa-cargo`, locked offline dependencies and synthetic loopback fixtures:

```sh
node tests/fixtures/messaging/session_renewal_oracle.cjs
node tests/fixtures/messaging/device_readiness_oracle.cjs
cargo test --locked --offline --all-features --lib readiness -- --test-threads=2
cargo test --locked --offline --all-features --test signal_crypto forced_renewal_preserves_trust_and_durable_unconfirmed_intent
cargo test --locked --offline --all-features -- --test-threads=2
cargo clippy --locked --offline --all-features --all-targets -- -D warnings
cargo check --locked --offline --features live --all-targets
```

Focused readiness/renewal tests pass **seven**; the independent disk integration
case passes, including a final rerun after test-only lint corrections. The original
session-renewal oracle and the existing **ten refusal/seven metadata cases** pass.
The full matrix passes **656 tests**, with **four ignored**: 623 library and 33
other tests. Strict all-feature/all-target Clippy, formatting and diff checks pass.
The `live` all-target compile check passes, and its dependency tree excludes Signal,
QuickJS and libsignal-core. Logs are ignored
`target/essential-renewal-{focused,all-tests,store-final,clippy,live-check,live-tree}.log`.

No schema or package version changed; the database remains schema nine. This
checkpoint did not build a new release, install an app, open a personal profile,
rotate real keys, send a real message, commit or push. Existing 2.0.4 packages do
not contain this workspace change. Standard `live` continues to exclude the engine.
548 processing, persistent stale state, general stale renewal, identity-change
confirmation and full E2EE offline-queue ordering remain pending. First-use
registration, including a newly discovered own device, can still leave a new
binding without a session after signature failure; old trusted bindings are not
silently replaced. Legacy unmarked unknown init attempts remain outside the typed
pending-intent guarantee. These synthetic results do not certify production E2EE.

## Atomic first-use initialization (2026-11-12)

Peer/own-device initialization now reserves untrusted inactive candidates and uses
the typed-init-only planner. It binds candidate account/local epoch, address, IID,
fingerprint and timestamp to the actual recipients, requires signed bundles and
commits activation with verified engine records and exact init outbox bytes in one
revision-CAS transaction. The regular personal-message planner stays active-only.
Repeated failed proposals reuse a candidate per scoped device; persisted engine or
outbox references and unresolved typed-init intent prevent rebinding.

Eight focused tests pass: five library and three direct coordinator cases. They
cover invalid first-use peer/own/multi-device signatures, bounded candidate reuse,
all personal/group-sender record references and outbox references, typed-purpose
and bundle requirements, cancellation, revision/local-epoch changes, SQL abort,
successful atomic activation, restart and unknown-init protection. The full
all-feature suite passes **664 tests with four ignored** (628 library, 36 other).
Commands, using the existing pinned offline toolchain/cache and synthetic profiles:

```sh
cargo test --locked --offline --all-features first_use -- --test-threads=2
cargo test --locked --offline --all-features -- --test-threads=2
```

Strict all-feature/all-target Clippy (`-D warnings`), standard-`live` all-target
compile, formatting and diff checks also pass. Logs are ignored
`target/atomic-first-use-{focused,all-tests,clippy,live-check}.log`.

No schema/version change or new release/install was made. Inactive proposals may
remain after failure/restart; no new active init trust is established before the
verified commit. Existing active bindings are not replaced. Inbound receive and
the explicit diagnostic dry probe retain their older eager first-use registration;
this checkpoint does not claim atomicity for those paths. Standard `live` still
excludes the optional engine. No real account/server was used, and these tests do
not establish production interoperability.

## Ordinary personal/group text replies (2026-11-12)

The native message menu and composer now support ordinary text replies with
account/thread-scoped quote drafts. The worker resolves the original message ID
against canonical SQLite data before reserving a quote request; UI text/name
snapshots are not wire authority. Real source client IDs are retained, without
substituting server IDs. Missing IDs, nonzero/unknown TTL, formatting/mentions,
information/media/encrypted sources and E2EE reply sending remain unsupported.
Explicit source ineligibility is sticky across replay, while an ordinary replay
merely lacking optional metadata keeps known client IDs.

Schema ten adds `message_reply` and `outgoing_reply` sidecars, preserving legacy
history, cursors, ordinary/E2EE outbox bytes and typed init intent. Complete bounded
quote JSON stays worker-side (128 KiB); history receives normalized projections
(256 KiB ceiling), under the existing aggregate rich-batch budget. Unknown layouts
render an honest fallback. No downgrade is provided; older binaries refuse this
schema. Existing rows without source IDs stay unavailable for reply until a
consistent actual replay enriches them.

The original JavaScript oracle passes eight cases for direct/group endpoints,
payload fields, default TTLs, missing source client IDs and group `imei` omission.
Independent protocol/store cases exercise account/thread isolation, canonical
target rereading, hidden-policy and E2EE rejection before reservation, raw quote
retention, numeric/string reference normalization across transports, enrichment,
conflicting-ID/cursor rollback, sticky source revocation, SQL reservation/echo
failure, schema-nine migration rollback and restart. Durable outgoing quote data
repairs own echoes missing quote fields. The existing account/thread/body/server-ID
ACK proof also preserves the canonical outgoing client ID when a later own echo
carries a foreign correlation hint; unrelated messages cannot use that fallback.
Composer regressions keep newer text/quote edits across late ACKs and preserve text
when cancelling a reply.

The all-feature matrix passes **686 tests with four ignored**: 638 library and
48 CLI/integration tests. A final integration-only run passes 32 tests after a
test-only future-schema expectation correction; the successful unchanged library
result remains valid. Logs are ignored `target/ordinary-reply-all-tests.log` and
`target/ordinary-reply-integration-final.log`; fixture verification is
`node tests/fixtures/messaging/reply_oracle.cjs`. Strict all-feature/all-target
Clippy, formatting and diff checks pass. `check.sh` passes the default library
suite (259 passed, one ignored), default integrations, five IME adapter tests,
19 Python tests and dependency/headless CLI guards. The separate standard-`live`
all-target compile check passes. Logs are ignored
`target/ordinary-reply-{clippy,default-check,live-check}.log`.

Synthetic native-window captures cover Light/Dark at **1200×780** and **640×480**;
the manager inspected the reply banner and quote bubbles in all four. These are
app-only synthetic views from a private Xvfb fixture at native pixels-per-point
one, without personal profiles or network access. Files are
`/tmp/zalo-reply-{1200,640}-{light,dark}.png`; a fifth capture,
`/tmp/zalo-reply-menu-640-dark.png`, shows the enabled reply context menu. Logs use
`target/ordinary-reply-ui-{1200,640}-{light,dark}.log` and
`target/ordinary-reply-ui-menu.log`.

An optimized standard-`live` build and schema-ten release smoke pass: 19 synthetic
messages in six conversations, restart/legacy preservation, private profile
permissions and external process-lock refusal, with `network_used=false`. The
21,707,456-byte `target/release/zalo-rust` reports version 2.0.4; SHA-256 is
`8da01b478b8462122af8f90d7fbefe14eacf332f6b81bf97e5f39d53a2a7de05`.
Build/smoke logs are `target/ordinary-reply-{live-release,live-smoke}.log`.
The binary was not installed or opened with a real account, and existing
`dist/rust` DEB/RPM artifacts were not rebuilt. These are synthetic proofs, not
real-server reply acceptance. Forwarding, reactions, mentions and recall/delete
are outside this milestone.

## Ordinary single-file upload (2026-11-12)

Single ordinary files now use the current FILE chunk/resource/async-message flow;
photo upload remains pending. The exact-source VM oracle passes ten cases,
including original filename spelling, NFC final names and uppercase Unicode
extensions. Six local TLS test functions cover 16 flow variants, including
PING/history/reconnect cooperation while HTTP upload is pending. Scoped storage
checks cover exact immutable final payloads, one active upload, restart
classification, unknown duplicate protection, account isolation, ACK-before-echo
repair, real own-file reconciliation and schema-ten-to-eleven migration/rollback.
Neither resource completion nor placeholder text can settle a send. These checks
use synthetic metadata and private profiles, not real-service uploads.

The final all-feature suite passes **720 tests, four ignored** (663 library and
57 CLI/integration). Strict all-feature/all-target Clippy, messaging-only and
standard-live all-target compile checks, formatting and diff checks pass.
`check.sh` passes 293 Rust tests with one ignored, including five IME regressions,
19 Python tests, dependency guards and headless CLI checks. Logs are
`target/file-upload-{all-features,fmt,check,messaging-check,live-check,clippy}-qa.log`.
Four actual native-window captures at 1200×780 and 640×480 in Light/Dark were
reviewed by the manager: `/tmp/zalo-file-{light,dark}-{1200,640}.png`. They verify
synthetic selection/progress/failure presentation, without personal account data.

The optimized standard-live executable reports version 2.0.4, is **22,114,592
bytes**, and has SHA-256
`de0982ad9b77bf85cd388ed9ddf5c662ee8277600ca605353dd32cbb0e3da1fe`.
The manager independently verified its size/hash. Schema-eleven release smoke
passes with 19 synthetic messages in six conversations, restart idempotency,
legacy preservation, private permissions, process locking and
`network_used=false`. Logs are `target/file-upload-{release,smoke}-qa.log`.
The executable was not installed, existing DEB/RPM artifacts were not rebuilt,
and no commit or real-account upload acceptance occurred. Photo upload, transfer
resume and encrypted attachments are outside this milestone.

## Ordinary single-photo HD upload (2026-11-12)

PHOTO sending has synthetic verification for bounded static-image preparation,
exact-processed-JPEG preview, caption/revision preservation and typed durable
resource/ACK recovery. Eight original-source PHOTO oracle cases pass; the ten
FILE oracle cases remain green. Six local TLS functions cover 15 PHOTO flow
variants. Codec, protocol, scoped storage and recovery checks cover orientation,
metadata removal, alpha compositing, input/output budgets, immutable payloads,
cross-FILE isolation, caption-changed unknown duplicates, SQL rollback and
schema-eleven preservation. These are local fixtures, not real-service photo
acceptance. Original-byte sending, albums, animation and encrypted photos remain
outside this milestone.

The final all-feature suite passes **755 tests, four ignored**. `check.sh`
passes 301 Rust tests and 19 Python tests. Formatting, strict all-feature/all-target
Clippy, messaging-only and standard-live all-target checks pass. The manager
reviewed all eight native Light/Dark wide/compact captures:
`/tmp/zalo-photo-{preview,uploading}-{light,dark}-{1200,640}.png`.
They show synthetic exact-byte preview/caption and upload state using isolated
profiles; no personal image or account was inspected.

The optimized version-2.0.4 standard-live executable is **22,515,520 bytes**,
SHA-256 `df08be7037f3f2757c210d67793434a18f6a0f3899f5e09fb47022a58b265bb2`.
Schema-twelve release smoke passes without network access, including synthetic
restart, legacy preservation, private permissions and process locking. The
executable was not installed, packages were not rebuilt and no commit or
real-account photo upload acceptance occurred.

## Ordinary unread, read status and Linux desktop alerts (2026-11-19)

The locked, offline all-feature suite passed **765 tests, five ignored**.
The focused storage cases verify that schema twelve upgrades without adding
unread to history, and that new unread keys, the message and its cursor roll
back together. They cover replay, hidden chats, separate account preferences,
explicit visible IDs, missing client ID enrichment, monotonic remote
watermarks and delivery updates that arrive before the owned message.
Three protocol cases check the direct/group V2 fields, fail-closed mute
parsing using server seconds, and rejection of ambiguous status and
subchatbox watermarks. The original-source reply (8), file (10) and photo (8)
oracle cases still pass.

A local TLS/WSS worker test loads a timeline with two unread rows and confirms
that loading does not send a read receipt. An explicit visible-ID command
clears both local unread rows and issues exactly one HTTP receipt, for the row
that carries its actual source client ID. The focused presentation test
requires a previously painted frame and current focus/scope before
submitting visible IDs.

The ignored desktop integration test was run separately against a new private
D-Bus daemon and a fake `org.freedesktop.Notifications` service. It verifies
the generic alert, replacement ID, opt-in preview, activation and dismissal.
The ten synthetic native-window captures exercise the conversation and
notification settings at 1200×780 and 640×480 in Light and Dark, plus the
compact unread filter in both themes. Capture files are
`/tmp/zalo-attn-{chat,settings}-{light,dark}-{1200,640}.ppm` and
`/tmp/zalo-attn-filter-{light,dark}-640.ppm`.

These checks use isolated profiles and local services. Real-service receipt
interoperability and operation across Linux desktop environments remain to be
confirmed.

Formatting and strict all-target, all-feature Clippy passed. The standard-live
version-2.0.4 release executable was **24,965,056 bytes**, SHA-256
`6e1833077f39d3cc0cfb9e3e6e43ed8ee57e567b0d23290313f5de3088597c8b`.
The schema-thirteen smoke test passed with 19 synthetic messages in six
conversations, restart and legacy-history preservation, private permissions,
process locking and `network_used=false`.

## Ordinary direct text forwarding (2026-11-19)

The locked, offline all-feature suite passed **776 tests, five ignored**.
Formatting and strict all-feature, all-target Clippy passed. The focused
forward suite passed **11 tests**. The original-source forward oracle passed
five synthetic cases. Scope and storage regressions cover verified source and
friend destination, hidden/blocked/cross-account rejection, cursor rollback,
typed echo and delayed ACK binding, unknown-intent persistence and local ACK
repair. A local TLS/WSS worker test exercises destination TTL proof, the exact
forward request and an unknown result surviving restart without a resend.

The manager reviewed all eight synthetic native-window captures at 1200×780
and 640×480 in Light and Dark. They cover the recipient picker and its
unknown-result state:
`/tmp/zalo-forward-{picker,unknown}-{light,dark}-{1200,640}.ppm`.
They exercise presentation and scope without accessing a production account.
The supported slice is one first-generation ordinary direct text message to
one direct friend. Production interoperability remains to be confirmed.

The standard-live version-2.0.4 release executable was **25,199,424 bytes**,
SHA-256 `10742ea85d4b0ad9e0b2faa77d8240eb527acb8d43c213a3511ce2fc22cb8588`.
The schema-fourteen release smoke passed, with `network_used=false`.

## Ordinary direct text recall (2026-11-19)

The focused suite passed **10 tests**, including four storage cases. The
three-case original-source oracle passed. The regressions cover exact
direct request fields, eligible source and age, authenticated original/event
ID separation, quote and outgoing projection redaction, same-thread scope,
late replay, cursor rollback, and restart repair after a durable ACK.
A message that literally contains the recall label remains ordinary until
the store records verified recall evidence. The native UI cases preserve
draft text and reject settlement for unrelated account or request IDs.

A synthetic TLS/WSS worker drives an ordinary direct recall through the live
actor. It verifies the authenticated request, sticky redaction on replay,
and an unknown result that can settle only with verified notice/ACK evidence.
These checks establish local behavior. Production interoperability remains
to be confirmed.

The locked, offline all-feature suite passed **786 tests, five ignored**.
Formatting and strict all-feature/all-target Clippy passed. The manager
reviewed all 12 native-window captures for confirmation, unknown outcome
and recalled content in Light and Dark at 1200×780 and 640×480:
`/tmp/zalo-recall-{confirm,unknown,recalled}-{light,dark}-{1200,640}.ppm`.

The standard-live version-2.0.4 release binary was **25,353,280 bytes**,
SHA-256 `f8dab32bc9b8259f24d655321ebad5e0556d91f574782bcc56efb4eac6511060`.
Schema-fifteen release smoke passed without using the network.

## Ordinary group roster, invite and leave (2026-11-19)

The focused group suite passed **28 tests**, with three roster and two
group-action original-source oracle cases passing separately. Coverage
includes bound ordinary membership and permissions, filtering private
member IDs before profile lookup, invalid or stale continuation, exact
invite and leave forms, scoped SQLite settlement, restart uncertainty,
absence without membership proof, and confirmed archive with unchanged
history. The actual local TLS/WSS worker exercises the authenticated
roster/profile/mutation requests, permission filtering and durable
outcome handling.

The manager reviewed all 24 synthetic native-window captures in Light
and Dark at 1200×780 and 640×480. The scenes cover details, roster, invite,
leave confirmation, unknown outcome and confirmed read-only history:
`/tmp/zalo-group-{details,roster,invite,leave,unknown,left}-{light,dark}-{1200,640}.ppm`.
The supported slice is an ordinary group with at most 100 members,
a page of at most 50 visible members, and one recipient per invitation.
Production interoperability remains to be confirmed.

The locked, offline all-feature suite passed **801 tests, five ignored** with
`RUST_TEST_THREADS=4`. Strict all-feature/all-target Clippy, formatting and
all 19 Python tests passed. The heartbeat fixture uses an isolated test
logger and retains its exact structural diagnostic assertions.

The default-concurrency run had one `[Receive/Network]` socket error in the
existing `media_preview_and_socket_orders_preserve_complete_metadata_across_chunks_and_restart`
fixture. Its assertions were kept. The same fixture and the complete suite
passed with four test threads.

The standard-live version-2.0.4 release executable was **25,578,752 bytes**,
SHA-256 `d08ddca1b344fabb01ccec5c7da837c45ecb85ec051cfcbde814a9068c9a5c73`.
Schema-sixteen release smoke passed without using the network. It used
19 synthetic messages in six conversations and checked restart, preservation
of legacy and local history, process locking, profile permissions and
informational diagnostics. The separate
`cargo check --all-targets --no-default-features` passed.

## Structured chat sync and disabled send (2026-11-19)

The installed 2.0.4-2 client reached Online, then a structural diagnostic
recorded `Synchronization/Protocol` while processing command 510, subcommand 1.
The reason `MessageContent`, message kind `webchat` and field type `object`
identified the rejected queue row. Stalled synchronization disabled Send.
The original renderer's `getTextFromContent` recognizes an `rtf` object's
`title`. Authenticated queue and verified preview now display that bounded
title or a placeholder for other structured objects. These rows carry no
ordinary send, reply, forward, recall or remote-read proof.
[Original renderer](../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js#L548349)

Three focused regressions passed. The parser covers a rich-text title, an
opaque object, an oversized title and normal neighbors, with strict and
encrypted parsing preserved. The SQLite test verifies replay, cursor advance
and rejection of a structured row as evidence of an outgoing send, even when
the IDs match. The local TLS/WSS worker commits a mixed page, stays Online and
sends an ordinary message through a synthetic HTTP endpoint. Real-account
acceptance remains to be verified.

The 2.0.4-2 executable was also exercised in offline demo mode with actual
IBus x-unikey on an isolated X11 display. With x-unikey off, Enter submitted
ASCII. With the Telex word still composing, the first Enter committed the word
and enabled Send, and the second Enter submitted exactly one synthetic row.
After a space committed the composition, one Enter submitted. Extra Enter
presses produced no duplicate rows.

## Built-in sticker sending and message reactions (2.0.5 preview)

The all-feature suite passed 817 Rust tests with five opt-in tests ignored.
Strict all-feature Clippy, default all-target compilation, formatting, the
original sticker oracle, 19 Python tooling tests and five IME adapter tests
also passed. Loopback TLS/WSS covers sticker requests for direct chats and
groups, ACK and echo correlation, persistence of ambiguous attempts after
restart, and reaction acceptance, replay, and removal. The schema 16 to 17
regression checks saved history, the cursor, and unresolved text sends.

The private X11 fixture `ui::live::tests::signed_in_offline_native_window_fixture`
shows the picker, reaction badge, or reaction menu with
`ZALO_UI_FIXTURE_INTERACTIONS=stickers|reactions|reaction-menu`. Review covered
Light and Dark at 640×480 and 1200×780 with synthetic content. Connection traces
showed zero Internet connections for every capture. Keyboard regressions verify
that a single selection dispatches to the current conversation and preserves
the composer draft. The screenshots use synthetic sticker thumbnails.

An optimized standard `live` build passed the schema 17 release smoke with
19 synthetic messages across six conversations, restart preservation, profile
locking and private diagnostic files. The workspace preview still reports
version 2.0.4 until the 2.0.5 release is finalized.

Real-account acceptance is pending for both workflows. Reaction totals cover
confirmed local sends and authenticated notices the worker has processed.
Receiving updates through the separate control stream and historical reaction
replay require further work.

## Catalog search, reaction catch-up and message selection (2.0.5 preview)

The full offline all-feature suite passed 828 Rust tests with five opt-in tests
ignored. Strict Clippy, formatting, the sticker source oracle and 19 Python
tooling tests passed. Local TLS checks verify the authorized sticker search,
matching detail response and typed send. Local WSS checks both advertised
reaction queues, retry from the last committed cursor and later reconciliation
when the referenced message arrives. The 17 to 18 upgrade covers preserved
built-in sticker intents and atomic reaction cursor rollback on write failure.

Private X11 captures cover `stickers`, `stickers-search` and `message-selection`
in Light and Dark at 640×480 and 1200×780. Connection traces confirm zero
Internet requests. Selection reveals the entry message once, then allows normal
scrolling. Keyboard tests cover ordered plaintext copy, draft preservation,
Escape and page changes.

The optimized standard `live` preview passed the schema 18 release smoke with
19 synthetic messages across six conversations, restart preservation, profile
locking and private diagnostic files.

Real-account checks remain in [PLAN_2.0.5.md](PLAN_2.0.5.md). Reaction catch-up
requires advertised queues and retained ordinary history. The legacy control
poll, queue resets and evictions are outside this implementation.

The 2.0.5-1 packages in `dist/rust/` use the same optimized `live` executable
and require at most the `GLIBC_2.30` symbol set. The Debian package upgraded
2.0.4-3 on Ubuntu 20.04 and retained a synthetic history row across schema
16 to 18. Installation, payload integrity and removal of the RPM passed on
AlmaLinux 9. The extracted binary reports 2.0.5 and passed schema 18 release
smoke. SHA-256 sums were verified for both packages.

## Compact message context menu (2.0.5-2)

Private X11 captures cover the menu with and without reactions in Light and
Dark at 640×480 and 1200×780. The full menu measures about 230×255 pixels at
native scale one. The capture connection traces contain no Internet connections.
Keyboard tests cover Tab, both arrows, wrapping between enabled actions, narrow
viewport placement and preservation of the composer draft. The all-feature
suite passed 828 Rust tests with five opt-in tests ignored. Strict all-feature
Clippy and formatting passed.

The 2.0.5-2 Debian package upgraded 2.0.5-1 in Ubuntu 20.04 and preserved a
synthetic history row at schema 18. The RPM installed, verified and removed
cleanly on AlmaLinux 9. Both packages carried the same executable, and both
SHA-256 checks and the schema 18 release smoke passed.

## Packaged build pilot (2.0.5-2)

The same checked packages passed clean installation, CLI diagnostics, desktop
launcher and icon checks, and removal on Ubuntu 22.04, Ubuntu 24.04, Debian 12
and Fedora 42. Ubuntu 20.04 upgrade and AlmaLinux 9 results are recorded above.
Packaging now reads the tested revision from Cargo metadata. A disposable script
fixture checked the default revision 2, explicit override 3, and rejection of
invalid revisions before writing package outputs.

The extracted 2.0.5-2 binary ran with the installed x-unikey IBus engine on
isolated X11 in offline demo mode. Enter committed and saved `chào` once in two
trials, and subsequent presses added no rows. Enter and mouse Send worked with
x-unikey off and after committing Telex text with a space. Switching between two
conversations preserved both Unicode drafts and saved each only on explicit
submission. The profile, X server and IBus bus were private to the test.

The same extracted binary ran on a headless GNOME Wayland compositor with
`DISPLAY` unset, a private IBus x-unikey engine and negotiated text input v3.
With active Telex preedit, the first Enter committed the word and a second
Enter saved it once. Extra Enter presses added no rows. A space-committed
draft and ASCII with the engine off each saved on a single Enter. A native
pointer click on the demo composer button saved exactly one committed draft.

The user confirmed sticker sending and adding/removing reactions between Linux
and phone on 2.0.5-2. Detailed direct/group, catalog search and offline reaction
catch-up acceptance remain tracked in the [release plan](PLAN_2.0.5.md).
