# Native Rust architecture

The Rust application code is authored independently in this repository; no `ctdozlite` source or assets are adopted. The native UI reuses this repository's existing Zalo theme, Segoe UI and icon font assets as explicitly requested. It does not use a WebView. Registry dependencies remain recorded in `Cargo.lock` with their own licenses. A successful demo is not evidence of Zalo protocol compatibility.

## Design and review contracts

| Boundary | Required behavior and acceptance evidence |
| --- | --- |
| Domain | Typed account, conversation, and message identity; reject missing/oversized fields before persistence. No GUI or transport dependency in core models. |
| UI → worker | Bounded command queue; full/disconnected queue yields a visible retryable state. Never silently lose a send or block the UI waiting for capacity. |
| Worker → UI | Bounded events, bounded processing per frame, repaint triggered by events. No unconditional repaint loop or fixed-rate network polling on the UI thread. |
| Storage | One worker owns SQLite; prepared statements/transactions for writes. Account+conversation+message identity enforces idempotency. Keyset pagination uses a stable timestamp+ID order and matching composite index; loading a conversation does not scan all history. |
| Transport | Protocol trait separates independently verified wire behavior from fixtures. Demo transport must be clearly synthetic. Unsupported live authentication returns an explicit capability/error, never fake success. |
| Reconnect | Bounded exponential backoff, reset after successful connection, cancellation observed during waits. Network errors preserve session data; only an explicit authentication rejection may invalidate credentials. |
| Media | Explicit byte budget and bounded cache entries; reject oversized objects and evict predictably. Do not read entire history or unlimited attachments into memory. |
| Profile | Named isolated paths, process lock, private permissions and atomic secret writes. Enforce the declared account policy before loading history: immutable owner binding must reject a second account; stores supporting multiple accounts must scope every query and identity. |
| Shutdown | Cancellation closes bounded channels and releases worker/SQLite/profile resources without leaving an orphan worker. |

These are review contracts, not blanket claims that every item has passed. Current evidence and missing capabilities are tracked in [MIGRATION_STATUS.md](MIGRATION_STATUS.md).

The optional trust-WASM host is an isolated experiment and is not loaded by the default GUI or demo. Its `Rc`/`RefCell` state is thread-bound and its cryptographic calls are synchronous. Future integration must construct, use, and drop it inside a dedicated blocking worker, returning only validated bytes/results across bounded channels. Running it on the UI or Tokio network event loop would violate the responsiveness contract. Positive independently signed fixtures verify the pinned cryptographic contract; authenticated UID/master binding, attempt freshness, replay policy and session bootstrap remain required before activation.

## Performance acceptance

Measure release builds using the [validation procedure](VALIDATION.md). Require queue/cache limits to hold under synthetic load, deterministic deduplication after repeated delivery, pagination correctness when timestamps collide, and explicit behavior under queue saturation and cancellation. Use SQLite query plans to verify pagination uses its index rather than a temporary sorting table or full scan.

The replacement target remains at least 50% less aggregate process-tree PSS than Electron under equivalent real workloads. Record cached-UI startup and online-ready startup separately. Neither a smaller demo nor a faster blank window satisfies that target. CPU use at idle, input responsiveness with large histories, and peak cache memory need their own measurements.
