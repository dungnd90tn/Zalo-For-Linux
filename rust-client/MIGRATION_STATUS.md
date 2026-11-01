# Migration status

The Rust native implementation is authored independently. Electron remains the supported application; live-message compatibility, data-import compatibility, and comparative performance are not yet established.

## Completion gates

| Gate | Required evidence | Status |
| --- | --- | --- |
| Architecture | Independent domain/protocol/storage/runtime/UI boundaries and explicit resource budgets | Baseline implemented and reviewed; media/network adapters remain unwired |
| Offline logic | Locked build, tests, formatting/lint, profiles, scoped storage, bounded queues/cache, cancellation | Passed; optional trust-WASM remains an isolated experiment |
| Offline protocol | Independent cipher vectors, bounded QR driver, bootstrap staging contracts, optional HTTP adapter | Passed synthetic/known-answer, bounded-stream and local real-TLS tests; live endpoint/login unverified |
| Trust interpreter experiment | Pinned existing module, resource limits, independent positive/negative cryptographic fixtures | HMAC/A1/D2/L3 offline contracts passed; account/attempt/replay/session integration remains unaccepted |
| Synthetic demonstration | Headless worker/storage smoke and native GUI using clearly labeled fixtures | Themed native UI, additive 19-message upgrade and isolated X11 render/input/store/shutdown checks passed |
| Independent live protocol | Verified authentication and personal/group send/receive; restart/reconnect/media | Not implemented or accepted |
| History migration | Source schema evidence, immutable copy inspection, verified decoding/mapping/import | Read-only inspection only; no native importer |
| Linux daily-use/beta | IME/Wayland/X11, clipboard/notifications, packaging and two-week live pilot | Not accepted |
| Replacement decision | All prior gates plus comparable ≥50% total-process PSS reduction target | Not evaluated; no measurements |

## Verification record

Final independent verification on 2026-11-01:

- `./scripts/rust/check.sh`: **63 Rust tests** (61 library + 2 CLI), **six Python tooling tests**, whole-crate formatting, strict default Clippy and displayless informational/demo checks passed. Default dependencies exclude reqwest/wasmi. Launcher tests cover argument/path/exec/XDG-default preservation; explicit CLI profile flags override environment settings.
- `cargo test --locked --offline --all-features`: **98 tests** (96 library + 2 CLI) passed. All-target/all-feature Clippy passed with warnings denied. Optional adapters remain inactive in default GUI/CLI paths.
- HTTP coverage includes ten bounded-engine tests and **seven real TLS tests** through reqwest/Rustls against ephemeral loopback servers: synthetic CA/SNI, chunk/body/header limits, redirect refusal, cookie isolation, deadline/abort socket closure and sanitized certificate errors. Test-only CA/DNS overrides are absent from production constructors. Production endpoint compatibility remains unverified.
- **18 trust tests passed in both debug and release mode**, including independently constructed positive HMAC/A1/D2/L3 fixtures checked against RFC/OpenSSL arithmetic and regenerated JSON parity. Negative/context/expiry/resource-limit/poison/reuse tests also passed. The primary-linking boundary is decoded protobuf, not ciphertext; see [TRUST_FIXTURES.md](TRUST_FIXTURES.md).
- Real-worker regression verifies a late durable ACK updates the original sidebar preview without changing another recipient's pane/draft. SQLite snapshot failure rolls back insertion; exact-thread latest metadata and pagination use indexed seeks with stable ties and account scope.
- `./scripts/rust/build.sh --offline` and `python3 scripts/rust/smoke-release.py` passed: **9,923,632-byte** default release, 19 synthetic messages/six conversations, restart idempotency, external process-lock refusal, private permissions and no informational-command writes. Simulated legacy three-message history plus one local row upgrades additively to 20 rows without modifying existing contents, even after repeated restarts.
- Default release SHA-256: `29db6eb289a91c8375cfef3b91b2a75f9fdbaf864c3bd7ae781862c0dbf2740f`.
- Isolated Xvfb X11/Mesa checks rendered the existing Light theme, Segoe UI Vietnamese glyphs and Zalo icon font at **1200×780 and 640×480**. The debug build from the same frozen source passed accent-insensitive search, recipient selection, draft switching, mouse/Ctrl+Enter saves into three correct threads, compact back navigation and graceful window close; an external lock descriptor confirmed release afterward. The release binary rendered both sizes and closed through `WM_DELETE_WINDOW` with exit 0, then reopened the same profile successfully. Actual Vietnamese IME and Wayland remain unverified.
- Clean release screenshots are local review artifacts: `/tmp/zalo-rust-gui-check/final-release-{wide,compact,compact-list}.png`. They contain only fresh synthetic fixtures. Profiles and test processes were removed afterward.
- Source inventory found no `ctdozlite` source/assets or newly vendored fonts/binaries. The UI intentionally embeds two existing repository fonts; the optional host reads the explicit pinned existing WASM without embedding it. Fixture JSON is independently authored. No real Electron profile, private conversation or authenticated session was accessed.

## Synthetic storage measurements

Earlier foundation benchmarks on Ubuntu 26.04.1 with an AMD Ryzen 7 4800H and Rust 1.99.1 ran in `/tmp` on **tmpfs**, with pages queried immediately after insertion/replay (warm data). Pages contain 127 messages, inserts are bounded to batches of 200, and every ID/order/body is checked across 20 timestamp ties. Replaying the same batches inserted zero duplicates. These measurements predate the UI/durable-ACK additions.

| Messages verified | Insert time | Page p50 / p95 | Page queries including EOF |
| --- | --- | --- | --- |
| 20,000 | 108.713 ms | 110 / 156 µs | 159 |
| 100,000 | 591.484 ms | 114 / 169 µs | 789 |

Reproduce with `(cd rust-client && cargo run --release --locked --offline --manifest-path Cargo.toml --example bench_store -- 100000)`. These are single synthetic runs on memory-backed storage, not disk-I/O, online-client, startup, RAM, or Electron comparison measurements.

## Next acceptance work

Implement the bounded authentication coordinator described in [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md): bind each proof/confirmation to its QR attempt and companion keys, persist UID/master association separately, match authenticated bootstrap UID, enforce freshness/replay policy and commit trusted session state atomically before publishing usable authentication. Construct the synchronous trust host inside a dedicated worker. Keep offline fixtures separate from real responses and validate history encryption before any import. Preserve Electron and its data throughout the pilot. Live messaging/media, voice/video, encrypted Electron session transfer, backup decryption and replacement packaging remain future work.
