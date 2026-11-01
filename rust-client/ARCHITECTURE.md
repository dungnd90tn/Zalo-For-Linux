# Native Rust architecture

The Rust application code is authored independently in this repository; no `ctdozlite` source or assets are adopted. The native UI reuses this repository's existing Zalo theme, Segoe UI and icon font assets as explicitly requested. It does not use a WebView. Registry dependencies remain recorded in `Cargo.lock` with their own licenses. A successful demo is not evidence of Zalo protocol compatibility.

## Design and review contracts

| Boundary | Required behavior and acceptance evidence |
| --- | --- |
| Domain | Typed account, conversation, and message identity; reject missing/oversized fields before persistence. No GUI or transport dependency in core models. |
| UI → worker | Bounded command queue; full/disconnected queue yields a visible retryable state. Never silently lose a send or block the UI waiting for capacity. |
| Worker → UI | Bounded events, bounded processing per frame, repaint triggered by events. No unconditional repaint loop or fixed-rate network polling on the UI thread. |
| Storage | One worker owns SQLite; prepared statements/transactions for writes. Account+conversation+message identity enforces idempotency. Keyset pagination uses a stable timestamp+ID order and matching composite index; loading a conversation does not scan all history. |
| Transport | Default builds exclude network engines. Explicit live mode separates QR/trust/bootstrap from encrypted personal/group text transport. Synthetic test capabilities never exist in application builds; unsupported schemas/encryption fail closed. |
| Reconnect | Bounded exponential backoff, reset after successful connection, cancellation observed during waits. Network errors preserve session data; only an explicit authentication rejection may invalidate credentials. |
| Media | Explicit byte budget and bounded cache entries; reject oversized objects and evict predictably. Do not read entire history or unlimited attachments into memory. |
| Profile | Named isolated paths, process lock, private permissions and atomic secret writes. Enforce the declared account policy before loading history: immutable owner binding must reject a second account; stores supporting multiple accounts must scope every query and identity. |
| Shutdown | Cancellation closes bounded channels and releases worker/SQLite/profile resources without leaving an orphan worker. |

These are review contracts, not blanket claims that every item has passed. Current evidence and missing capabilities are tracked in [MIGRATION_STATUS.md](MIGRATION_STATUS.md).

The optional live path has a dedicated authentication controller and a separate bounded trust worker. That worker constructs, uses and drops the synchronous `Rc`/`RefCell` WASM host on its own thread; only proof/results cross its channel. Live builds embed the existing repository module and verify its hash before instantiation. Default GUI/demo builds do not include it. Authentication binds generation/companion/account context, checks bootstrap identity and commits private session state before publishing a verified runtime capability; see [AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md).

The server's explicit QR flags select `PhoneApproved` or `TrustedDevice`. Phone approval does not manufacture device-proof fields; trusted mode requires the real proof/list checks and never falls back after failure. The persisted tagged assurance survives restore. Both modes require confirmation UID, fresh bootstrap UID and immutable profile ownership to agree before activation. QR polling uses an exact additional origin with no cookie transmission; broader auth and messaging origin policies remain separate.

The messaging worker accepts only that opaque capability. HTTPS sends and WSS receives run outside the UI; SQLite work has its own worker. Outgoing delivery state is persisted, and uncertain sends are not silently retried. Account/thread/message IDs scope incoming deduplication and cursors. Unsupported media in a known legacy batch is represented by an explicit placeholder. E2EE/Signal queues are not consumed or acknowledged, and unknown wire formats fail closed without encryption downgrade. These implementations and local fixtures do not establish successful real-account operation.

## Performance acceptance

Measure release builds using the [validation procedure](VALIDATION.md). Require queue/cache limits to hold under synthetic load, deterministic deduplication after repeated delivery, pagination correctness when timestamps collide, and explicit behavior under queue saturation and cancellation. Use SQLite query plans to verify pagination uses its index rather than a temporary sorting table or full scan.

The replacement target remains at least 50% less aggregate process-tree PSS than Electron under equivalent real workloads. Record cached-UI startup and online-ready startup separately. Neither a smaller demo nor a faster blank window satisfies that target. CPU use at idle, input responsiveness with large histories, and peak cache memory need their own measurements.
