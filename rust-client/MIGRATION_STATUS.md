# Migration status

The Rust native implementation is authored independently. Electron remains the supported application; live-message compatibility, data-import compatibility, and comparative performance are not yet established.

## Completion gates

| Gate | Required evidence | Status |
| --- | --- | --- |
| Architecture | Independent domain/protocol/storage/runtime/UI boundaries and explicit resource budgets | Passed implementation review and integrated offline/local-TLS QA; real-account acceptance remains separate |
| Offline logic | Locked build, tests, formatting/lint, profiles, scoped storage, bounded queues/cache, cancellation | Passed default and optional-feature checks; default builds exclude network/trust engines |
| Offline protocol | Independent cipher vectors, bounded QR driver, bootstrap staging contracts, optional HTTP adapter | Passed known-answer, bounded-stream and local real-TLS/WSS tests; public QR creation/cancellation observed separately |
| Trust/session | Pinned existing module, cryptographic fixtures, account/attempt/replay/atomic-session checks | HMAC/A1/D2/L3 plus actual coordinator/atomic-session fixtures passed; phone approval and real-account restore unverified |
| Synthetic demonstration | Headless worker/storage smoke and native GUI using clearly labeled fixtures | Themed native UI, additive 19-message upgrade and isolated X11 render/input/store/shutdown checks passed |
| Independent live protocol | Native authentication and personal/group text runtime; verified real-account send/receive | Opt-in runtime and encrypted command-loop fixtures passed; real-account send/receive remains unverified |
| History migration | Source schema evidence, immutable copy inspection, verified decoding/mapping/import | Read-only inspection only; no native importer |
| Linux daily-use/beta | IME/Wayland/X11, clipboard/notifications, packaging and two-week live pilot | Not accepted |
| Replacement decision | All prior gates plus comparable ≥50% total-process PSS reduction target | Not evaluated; no measurements |

## Verification record

Independent verification on 2026-11-01 with pinned Rust 1.99.1, after authentication/messaging source freeze:

- `./scripts/rust/check.sh`: **72 Rust tests** (70 library + 2 CLI), **seven Python tooling tests**, formatting, strict default Clippy, informational-command no-write checks and headless demo passed. Default dependency inventory excludes reqwest, tokio-tungstenite and wasmi. Launcher regressions cover live-only feature selection, CLI/environment precedence, custom target paths and refusal to execute a stale artifact after build failure.
- `(cd rust-client && cargo test --locked --offline --all-features)`: **150 passed** (148 library + 2 CLI), **one ignored** public probe, zero failures. Strict all-target/all-feature Clippy passed. The suite includes 16 authentication tests, 18 HTTP tests (eight actual local TLS), 18 trust tests, six messaging-protocol tests and 15 live-runtime tests. Local TLS/WSS servers drive the actual command loop, encrypted personal/group requests, ACK/outbox persistence, duplicate receives, SQLite-before-Received publication, cancellation and offline continuation without `queueStatus`. Test CA/DNS injection is absent from production constructors.
- `cargo test --release --locked --offline --features trust-wasm protocol::trust`: **18 passed**. Debug/release cryptographic fixtures check positive HMAC/A1/D2/L3 against independent RFC/OpenSSL/arithmetic evidence, malformed/context/expiry/resource limits and trap poisoning. See [TRUST_FIXTURES.md](TRUST_FIXTURES.md).
- Authentication fixtures exercise the actual WASM proof and independently signed device list through local TLS, independently encrypted bootstrap, UID matching and atomic commit before Ready. Classic five-request TLS, assurance-preserving resume, cancel identity, backpressure, disk failure and mismatch rejection also pass. `PhoneApproved` carries no fabricated trust payload; `TrustedDevice` requires its proof and cannot downgrade after failure.
- Separately, the auth specialist's bounded **public unauthenticated** probe reached the actual controller's Begin → QR → immediate Cancel → SignedOut and released the profile lock. API 681 advertised PhoneApproved, a 512×512 PNG, timeout 180 seconds and retry 3. No phone scan/approval, authenticated account, message exchange or saved authentication record was involved; no QR/token artifact was retained. The exact poll-only `qr-id.zaloapp.com` policy sends no cookies, verified with a populated jar against actual local TLS.
- Both release builds passed `scripts/rust/smoke-release.py` (live with `--expect-live`): 19 synthetic messages/six threads, restart idempotency, external lock refusal, private permissions and no informational writes. A legacy three-message profile plus one local row upgrades additively to 20 without changing existing content. Default builds refuse `--live` before creating profile data. Live doctor reports compiled authentication/messaging, `authenticated=false` and `network_started=false`.

| Release | Bytes | SHA-256 |
| --- | --- | --- |
| Default | 9,961,072 | `f67643f606ba0aa46ced3364c53b4f2e2c0e0bc104e6bc8a3ab86263af2d63ca` |
| Live | 15,490,288 | `ece46b7b84a28ee7e44275cc667f592da1c68c7cf08b3a7884a079144213a1f5` |

The final local artifact is the live build at `rust-client/target/release/zalo-rust`; run `./rust-client/target/release/zalo-rust --live` or rebuild with `./scripts/rust/run.sh --live`.

Current release GUI checks under isolated Xvfb/X11/Mesa rendered the existing Zalo Light theme/fonts at **1200×780 and 640×480**. The offline demo saved mouse-submitted text to the correct thread, resized and closed with exit 0, then reopened the profile. Earlier functional checks also covered accent-insensitive search, recipient/draft switching, Ctrl+Enter and compact navigation. Final live GUI started signed out; Resume on a fresh profile showed a retryable failure with login/restore controls. A whole-lifetime strace of startup plus that Resume recorded **zero Internet connect calls**; no authentication/owner record was written. `WM_DELETE_WINDOW` exited 0 and an external lock acquisition confirmed release. Vietnamese IME and Wayland remain unverified.

Review images contain only synthetic/signed-out data: `/tmp/zalo-rust-gui-check/final-live-{wide,compact}.png`, `final-live-resume-empty.png` and `next-demo-{wide,compact}.png`. Temporary profiles and GUI processes were removed afterward.

Source inventory found no adopted `ctdozlite` implementation/assets. Embedding intentionally reuses two existing repository fonts and, for live builds, the existing pinned trust WASM with checksum verification. Fixture JSON/generators are independently authored. No real Electron profile, private conversation or authenticated session was accessed during QA.

## Synthetic storage measurements

Earlier foundation benchmarks on Ubuntu 26.04.1 with an AMD Ryzen 7 4800H and Rust 1.99.1 ran in `/tmp` on **tmpfs**, with pages queried immediately after insertion/replay (warm data). Pages contain 127 messages, inserts are bounded to batches of 200, and every ID/order/body is checked across 20 timestamp ties. Replaying the same batches inserted zero duplicates. These measurements predate the UI/durable-ACK additions.

| Messages verified | Insert time | Page p50 / p95 | Page queries including EOF |
| --- | --- | --- | --- |
| 20,000 | 108.713 ms | 110 / 156 µs | 159 |
| 100,000 | 591.484 ms | 114 / 169 µs | 789 |

Reproduce with `(cd rust-client && cargo run --release --locked --offline --manifest-path Cargo.toml --example bench_store -- 100000)`. These are single synthetic runs on memory-backed storage, not disk-I/O, online-client, startup, RAM, or Electron comparison measurements.

## Next acceptance work

Verify authorized real-account phone approval/QR login, restart/restore, personal/group text exchange, reconnect, account isolation and unknown-delivery recovery using [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md) and [PROTOCOL_MESSAGING.md](PROTOCOL_MESSAGING.md). Public QR creation and local encrypted fixtures do not satisfy those gates. Refresh-token renewal is not implemented. Validate history encryption before any importer and preserve Electron/data throughout the pilot. E2EE/Signal, media, voice/video, encrypted Electron session transfer, backup decryption and replacement packaging remain future work. HTTP sends are serial with a 15-second deadline and can pause socket processing; throughput and daily-use behavior need real-workload validation.
