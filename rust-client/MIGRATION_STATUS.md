# Migration status

The Rust native implementation is authored independently. Electron remains the supported application; live-message compatibility, data-import compatibility, and comparative performance are not yet established.

The migration target is full parity with the working Linux Electron application. The evidence-based [feature inventory](FEATURE_PARITY.md) tracks current gaps and acceptance criteria; the existing preview has not met that target. The first directory/recent-preview milestone is implemented and tested in isolation; live compatibility and full history remain incomplete.

## Completion gates

| Gate | Required evidence | Status |
| --- | --- | --- |
| Architecture | Independent domain/protocol/storage/runtime/UI boundaries and explicit resource budgets | Passed implementation review and integrated offline/local-TLS QA; real-account acceptance remains separate |
| Offline logic | Locked build, tests, formatting/lint, profiles, scoped storage, bounded queues/cache, cancellation | Passed default and optional-feature checks; default builds exclude network/trust engines |
| Offline protocol | Independent cipher vectors, bounded QR driver, bootstrap staging contracts, optional HTTP adapter | Passed known-answer, bounded-stream and local real-TLS/WSS tests; public QR creation/cancellation observed separately |
| Trust/session | Pinned existing module, cryptographic fixtures, account/attempt/replay/atomic-session checks | HMAC/A1/D2/L3 plus coordinator/atomic-session fixtures passed; user confirmed QR login/saved Resume Online, current binary completed five real-server heartbeats over about 155 seconds and closed normally; longer lifecycle/renewal remain unaccepted |
| Synthetic demonstration | Headless worker/storage smoke and native GUI using clearly labeled fixtures | Themed native UI, additive 19-message upgrade and isolated X11 render/input/store/shutdown checks passed |
| Independent live protocol | Native authentication and personal/group text runtime; verified real-account send/receive | Real Online, five heartbeats and two post-commit receive-ready events observed; incoming content/type/UI and outgoing exchange remain unverified |
| History migration | Source schema evidence, immutable copy inspection, verified decoding/mapping/import | Read-only inspection only; no native importer |
| Linux daily-use/beta | IME/Wayland/X11, clipboard/notifications, packaging and two-week live pilot | Not accepted |
| Replacement decision | All prior gates plus comparable ≥50% total-process PSS reduction target | Not evaluated; no measurements |

## Verification record

Verification on 2026-11-01 with pinned Rust 1.99.1, at the frozen client-24/directory, post-AUTH heartbeat, private-logging and schema-four storage snapshot:

- `./scripts/rust/check.sh`: **108 Rust tests** (106 library + 2 CLI), **seven Python tooling tests**, formatting, strict default Clippy, informational-command no-write checks and headless demo passed independently. Default and optional `signal-store` DTO dependency inventories exclude reqwest, tokio-tungstenite, wasmi, QuickJS and libsignal-core. Launcher regressions cover live-only feature selection, CLI/environment precedence, custom target paths and refusal to execute a stale artifact after build failure.
- `(cd rust-client && cargo test --locked --offline --all-features)`: **234 passed** (232 library + 2 CLI), **two ignored** opt-in checks (public probe and native GUI fixture), zero failures in the independent final run. Strict all-target/all-feature Clippy also passed. Coverage includes real loopback TLS/WSS command loops, three heartbeat cycles with later encrypted receive/send, parser failures reaching private JSONL, durable receive/cursors, directory synchronization through the single SQLite actor, schema-four migration/atomic storage, stale-generation/cancellation and legacy-session refusal. Test CA/DNS injection is absent from production constructors. No public probe was repeated during this checkpoint.
- Earlier unchanged trust-source checkpoint: `cargo test --release --locked --offline --features trust-wasm protocol::trust`: **18 passed**. Debug/release cryptographic fixtures check positive HMAC/A1/D2/L3 against independent RFC/OpenSSL/arithmetic evidence, malformed/context/expiry/resource limits and trap poisoning. See [TRUST_FIXTURES.md](TRUST_FIXTURES.md).
- Authentication fixtures exercise the actual WASM proof and independently signed device list through local TLS, independently encrypted bootstrap, UID matching and atomic commit before Ready. Classic five-request TLS, assurance-preserving resume, cancel identity, backpressure, disk failure and mismatch rejection also pass. `PhoneApproved` carries no fabricated trust payload; `TrustedDevice` requires its proof and cannot downgrade after failure.
- Separately, the auth specialist ran one authorized **public unauthenticated client-24** probe: normal controller Begin → QR → immediate Cancel → SignedOut passed in 0.72 seconds; auth/owner records were absent and the lock was released. No phone scan/approval, authenticated account or message exchange occurred, and no QR/token artifact was retained. Earlier API-681 research observed PhoneApproved and a 512×512 PNG. The poll-only `qr-id.zaloapp.com` policy sends no cookies, verified against actual local TLS.
- Default and live releases passed `scripts/rust/smoke-release.py` (live with `--expect-live`): 19 synthetic messages/six threads, restart idempotency, external lock refusal, private permissions and no informational writes, including `--doctor --debug-log`. A legacy three-message profile plus one local row upgrades additively to 20 without changing existing content. Actual log records match the executable SHA-256; slots are private/bounded and an obstructed log path leaves local operation usable with a fixed warning. Default builds refuse `--live` before creating profile data. Live doctor reports compiled authentication/messaging, `authenticated=false`, `network_started=false` and the absolute diagnostic path.

| Release | Bytes | SHA-256 |
| --- | --- | --- |
| Default checkpoint (this stage) | 10,029,968 | `01e6a46f27a885985f3559bec1dcbb8808e6ef6e8efb3748a9702986d59c282b` |
| Earlier live AUTH parser; user confirmed Online | 15,722,480 | `b06d0d628578c7fc2770eb0a22eba9c091d1d49a23b043cb2002e20c461048fe` |
| Current live (heartbeat + private logs + schema 4) | 15,780,592 | `470bc5a5ef94a58e8afe4ee11947f96846e3994018e126d71f5b25672e8fabd6` |

These gates apply to the current heartbeat/logging release hash above. Additive schema-four E2EE storage and optional `signal-store` DTO conversion are included and tested, but no production Signal encryption engine, key registration or encrypted-message dispatch is activated. The separate embedded-engine candidate and unlinked Signal protocol work are outside this release's live-capability acceptance.

The final local artifact is the live build at `rust-client/target/release/zalo-rust`; run `./rust-client/target/release/zalo-rust --live` or rebuild with `./scripts/rust/run.sh --live`.

GUI checks before the bootstrap compatibility fix, with unchanged UI source, under isolated Xvfb/X11/Mesa rendered the existing Zalo Light theme/fonts at **1200×780 and 640×480**. The offline demo saved mouse-submitted text to the correct thread, resized and closed with exit 0, then reopened the profile. Earlier functional checks also covered accent-insensitive search, recipient/draft switching, Ctrl+Enter and compact navigation. Final live GUI started signed out; Resume on a fresh profile showed a retryable failure with login/restore controls. A whole-lifetime strace of startup plus that Resume recorded **zero Internet connect calls**; no authentication/owner record was written. `WM_DELETE_WINDOW` exited 0 and an external lock acquisition confirmed release. Vietnamese IME and Wayland remain unverified.

Review images contain only synthetic/signed-out data: `/tmp/zalo-rust-gui-check/final-live-{wide,compact}.png`, `final-live-resume-empty.png` and `next-demo-{wide,compact}.png`. Temporary profiles and GUI processes were removed afterward.

The current release also passed a fresh signed-out Xvfb run: startup/shutdown records identify the actual `470bc5…fabd6` executable, log directory/file modes are 0700/0600, close exits 0 and releases the profile lock, and strace reports zero Internet connections. No login/restore action or authenticated record was created. Review artifacts are `/tmp/zalo-rust-gui-check/final-logger-live-wide.png` and the private synthetic `/tmp/zalo-rust-gui-check/qa-diagnostics.jsonl`.

Source inventory found no adopted `ctdozlite` implementation/assets. Embedding intentionally reuses two existing repository fonts and, for live builds, the existing pinned trust WASM with checksum verification. Fixture JSON/generators are independently authored. No real Electron profile, private conversation or authenticated session was accessed during QA.

## Phone-approval failure regression

A user reported successful phone confirmation followed by native rejection. The precise real response was not captured or inspected; the fixes below resolve independently reproduced parser/lock defects and do not establish that this account now completes login.

The post-login parser selects consumed chat/group services before validating their endpoints; directory profile/conversation capabilities are validated separately so an unsupported optional route cannot reject authentication. Unused extension/CDN schemas and legacy polling routes cannot reject login or authorize cookies. Consumed HTTP origins retain strict validation; advertised query-free WSS paths are preserved with trusted host, TLS, port, credential and fragment restrictions intact. Independently Node-encrypted fixtures containing over 100 unused entries pass the actual bootstrap/coordinator through durable Ready. Unsafe consumed URLs still fail.

Post-confirmation errors now display fixed `bootstrap/<stage>/<category>` diagnostics, with a numeric server code only when present. Tests distinguish service configuration, decryption/envelope, identity and key failures from actual authentication rejection (for example `bootstrap/inner-envelope/authentication-rejected code=600`), without exposing responses, URLs, cookies or keys. The full suite also exposed a lock-release defect when a duplicated/inherited file description remained open. A deterministic regression failed before explicit unlock on profile drop and passed afterward; an old duplicate cannot release a replacement profile's lock.

Close the existing Rust window before starting the updated live executable. If login fails, report only the displayed stage/category/code; do not send a QR code, session file, cookie or raw response. The user subsequently confirmed login/saved Resume Online and five real-server heartbeat cycles on the current binary; longer operation and full-content send/receive remain unaccepted.

## Signed-in offline recovery regression

The user subsequently reported login success followed by an offline, unresponsive workspace. An intermediate desktop WebSocket header change was followed by a user-provided `[Connect/Http; HTTP 403]` diagnostic before upgrade. The subsequent audit captured the installed Electron 43.3.0 file renderer against strict pinned loopback WSS: `Origin: file://`, user agent `ZaloPC-linux-24v681` and query type 24. This replaces the earlier incorrect null-origin inference. Authentication, parameter encryption, signed bootstrap, persistence and HTTP/WSS now share a closed client-24 identity. A real local TLS/WSS regression rejects the old null/type-30 handshake with 403 and accepts the captured contract with 101. The user's next attempt passed upgrade but reported `[Authentication/Protocol]`, leading to the AUTH-parser checkpoint below.

The preceding AUTH-parser release corrected independently reproduced decoding differences: the first frame byte is metadata, and the original Pako decoder accepts gzip as well as zlib. An oracle executing the existing bundle's decoder and authenticated-queue gate verifies five positive formats, four rejected AUTH cases and three invalid queue cases. Eight actual runtime TLS cases verify Online only for valid AUTH and usable local history after invalid queues. Key, authenticated-envelope and nonempty queue checks remain strict. Fixed parser-reason diagnostics identify failures without logging payloads, URLs or credentials. The real user's AUTH response was not captured. Close the old Rust window and **Resume the existing client-24 session** with the updated executable; obtain a fresh QR only for actual expiry or the historical Legacy30 case below.

Subsequent user feedback confirmed QR login and saved Resume reached **Online** with the AUTH-parser artifact, then showed `[Synchronization/Protocol; parser OuterEnvelope]`. The current heartbeat/control correction passed local acceptance and the limited real-server checkpoint below; longer synchronization and message exchange remain unaccepted.

On 2026-11-01, the user reopened the current `470bc5…fabd6` executable. At the user's request, the coordinator read only its sanitized diagnostic log: QR/phone approval → bootstrap → Ready → Online completed at **16:44:53.359 UTC**. Five consecutive heartbeat request/ready pairs completed at roughly 30-second intervals, with 67–229 ms response intervals. The user closed the application normally: shutdown cancellation occurred at **16:47:28.585 UTC**, followed by lifecycle Stopped at **16:47:28.588 UTC**, about **155 seconds** after Online. Two receive-ready events at **16:47:01.228** and **16:47:03.377 UTC** follow the runtime's committed-incoming-row path. The log does not distinguish ordinary text from unsupported-content placeholders or establish UI rendering. No failure, offline or send events were observed. The startup hash and feature mask 7 identify this exact LIVE artifact. This verifies the earlier post-AUTH failure did not recur over the five observed cycles; it does not verify production directory contents, outgoing messages, full-content personal/group exchange or daily-use reliability. No raw log, account data or credentials are copied into this repository.

The original-bundle post-AUTH oracle passes 18 control/API/encrypted-message cases. Missing/null success status is allowed only for PONG/PING_ACTIVE control frames; AUTH and ordinary data envelopes remain strict. Command 563 capability replies now pass through the normal outer decryption before parsing inner metadata. An actual local TLS/WSS command loop completes three heartbeat cycles with encrypted receive after each, performs layered capability lookup and HTTPS send/ACK, then rejects a malformed message without an ACK. That real parser failure reaches a default-mode log with fixed command/reason/envelope types; synthetic bodies, URLs, keys and tokens are absent.

Normal launches now write bounded, private, rotating logs automatically; `--debug-log` adds structural metadata. `--doctor --profile rust-live` prints the absolute `diagnostic_log_path` without opening the profile. Match custom profile flags when used. Invalid log paths do not disable the client, and writer failures produce a fixed warning. See [DIAGNOSTICS.md](DIAGNOSTICS.md) for path, retention and redaction details.

Network configuration, authentication, crypto/protocol and deterministic HTTP failures now pause transport while retaining the same SQLite actor and local command loop. An explicit reconnect resumes transport without replaying an outgoing send. Actual local TLS/WSS regressions cover HTTP 403, stalled handshakes, local Open/Load, cancellation, and paused error cases without automatic poison retries. Missing endpoints and expired authentication also retain offline history access; sends remain blocked appropriately.

The signed-in UI exposes reconnect, open-conversation and logout controls above both wide and compact workspaces. Typed failure diagnostics survive a stopped worker, cached selection and drafts remain available, and unknown-send review state survives retry. Independent actual-window QA used only a `cfg(test)` synthetic session with **zero external endpoints**, isolated Xvfb and strace: Open ID 22 reached a real local SQLite conversation/pane, offline typing and reconnect preserved its draft, logout confirmation could be cancelled, and both 1200×780/640×480 layouts remained usable. A second actual-window fixture read two real synthetic SQLite histories, switched recipients, restored each separate draft (including after reconnect), and navigated the compact list/detail views. Its test-only seed path was corrected to match the live database filename; the focused UI gate then passed 14 tests with the native-window fixture opt-in. Both windows closed with exit 0 and **zero Internet connect calls**. There is no public authentication bypass or real profile fixture.

Review artifacts: `/tmp/zalo-rust-gui-check/offline-final-wide.png`, `offline-final-thread-compact.png`, `offline-cached-first-restored.png`, `offline-cached-second-compact.png` and `offline-cached-list-compact.png`. This intermediate bug-fix artifact does not complete feature parity; automatic directory/conversation/history work follows [FEATURE_PARITY.md](FEATURE_PARITY.md).

## Synthetic storage measurements

Earlier foundation benchmarks on Ubuntu 26.04.1 with an AMD Ryzen 7 4800H and Rust 1.99.1 ran in `/tmp` on **tmpfs**, with pages queried immediately after insertion/replay (warm data). Pages contain 127 messages, inserts are bounded to batches of 200, and every ID/order/body is checked across 20 timestamp ties. Replaying the same batches inserted zero duplicates. These measurements predate the UI/durable-ACK additions.

| Messages verified | Insert time | Page p50 / p95 | Page queries including EOF |
| --- | --- | --- | --- |
| 20,000 | 108.713 ms | 110 / 156 µs | 159 |
| 100,000 | 591.484 ms | 114 / 169 µs | 789 |

Reproduce with `(cd rust-client && cargo run --release --locked --offline --manifest-path Cargo.toml --example bench_store -- 100000)`. These are single synthetic runs on memory-backed storage, not disk-I/O, online-client, startup, RAM, or Electron comparison measurements.

## Directory milestone checkpoint

Actual native-window QA under isolated Xvfb used a test-only private SQLite profile with 107 named contacts, one group, two cached histories and no external endpoints. Wide/compact layouts passed 100-row next/back paging, full-cache accent-folded search beyond the first page, named opening of existing and empty histories, direct/group identity separation, per-recipient draft restoration across retry, compact back navigation and logout cancellation. A separate empty-cache window opened Contacts through its empty-history action. Both windows exited 0 with zero Internet connect calls. Screenshots are `/tmp/zalo-rust-gui-check/directory-{wide,page-two-wide,accent-search-wide,nguyen-cached-wide,groups-compact,new-contact-compact,draft-restored-compact,group-draft-restored-compact,empty-opened-compact}.png`.

The final combined suite and default/formatting/lint gates passed as recorded above. Actual TLS/Node fixtures cover friends, multi-page groups and recent previews; runtime fixtures exercise automatic synchronization without WSS, partial failures, concurrent receive and a final dirty-cache flush. The release includes the client-contract and directory changes. Full cloud/mobile history, avatar downloads, friend/admin actions and production directory interoperability remain unaccepted; see [PROTOCOL_DIRECTORY.md](PROTOCOL_DIRECTORY.md).

Separate offline research checks passed six Python/OpenSSL tests and 21 JavaScript E2EE contract checks. These independent fixtures do not enable or accept production native Signal/E2EE messaging; see [E2EE_CONTRACT.md](E2EE_CONTRACT.md).

## Existing-session compatibility

New sessions persist schema version 2 with explicit numeric `client_type:24`. Earlier native version-1 records that omitted the type retain their historical `Legacy30` identity. Resume refuses them with `FreshApprovalRequired` **before opening the HTTP adapter**; tests preserve auth/history bytes, immutable owner and replay state. The separate owner record remains version 1, and future record versions fail closed.

For these historical Legacy30 records only, close the old Rust window, run the updated live executable, then choose a **new QR code** and approve it on the phone. Repeated Resume cannot convert an old session. Current client-24 records can Resume normally. No credential rewrite, history deletion or profile reset is needed; only successful fresh UID-matching authentication can replace credentials. Real-account reconnect, production directory loading and bidirectional messaging still require a manual retry.

## Synthetic directory storage measurements

`bench_directory` exercises the actual `MessageStore` API with 200-record transactions, replay, 100-record keyset pages and 30 repeated searches for an accent-folded final contact and a missing name. IDs/content/counts are checked; the 30,000-record run also verifies atomic capacity refusal. These single release runs used Rust 1.99.1, Ubuntu 26.04.1, Ryzen 7 4800H and `/tmp` **tmpfs**, while other development/GUI work was active. Searches follow insertion/replay/pagination and are **warm**, without a cold-cache or persistent-disk run.

| Contacts verified | Insert / replay total | Batch insert p95 | Last-contact folded search p50 / p95 | Missing search p95 |
| --- | --- | --- | --- | --- |
| 20,000 | 206.082 / 219.107 ms | 2.580 ms | 5.299 / 5.953 ms | 5.756 ms |
| 30,000 | 328.412 / 354.066 ms | 2.696 ms | 7.731 / 8.542 ms | 8.240 ms |

Reproduce with `(cd rust-client && cargo run --release --locked --offline --example bench_directory -- 30000)`. Results measure synchronous local database work, excluding HTTP/decryption, actor queue latency, UI rendering and avatar fetches. They establish no full-client, Electron memory, startup or throughput advantage.

## Next acceptance work

Extend the confirmed QR-login/saved-Resume Online, five-heartbeat and post-commit receive-event checkpoint to longer synchronization, verified directory contents, personal/group text exchange, reconnect, account isolation and unknown-delivery recovery using [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md) and [PROTOCOL_MESSAGING.md](PROTOCOL_MESSAGING.md). Public QR creation and local encrypted fixtures do not satisfy those remaining gates. Refresh-token renewal is not implemented. Validate history encryption before any importer and preserve Electron/data throughout the pilot. E2EE/Signal, media, voice/video, encrypted Electron session transfer, backup decryption and replacement packaging remain future work. HTTP sends are serial with a 15-second deadline and can pause socket processing; throughput and daily-use behavior need real-workload validation.
