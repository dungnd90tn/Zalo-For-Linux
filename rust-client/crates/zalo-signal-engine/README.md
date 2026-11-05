# Embedded Signal compatibility candidate

This standalone local library keeps Zalo's existing ratchet/KDF/wire implementation
and replaces its slow interpreted Curve primitives with the pinned official
Signal primitives. It has no Node, Electron, WebView, database, network, filesystem
or server-registration interface. The default feature exposes DTOs only;
`embedded` enables the dedicated native worker. The application does not activate
this engine or publish its output automatically.

## API and transaction ownership

`Engine::new(account, auth_generation, options)` creates one lazy worker.
`submit(Request)` returns a cancellable `OperationTicket` implementing `Future`
and `try_recv`. Every request supplies account, transient auth generation,
monotonically increasing operation ID, expected crypto revision and an immutable
`Snapshot` of committed records. Each operation builds a fresh store overlay;
only a successful result returns its `MutationDelta`. Failed, cancelled, stale
or discarded results never commit anything. Any failed operation discards the
entire JS runtime, including rejected legacy SessionLock cleanup promises.

Operations: CSPRNG-only `CreateBundle`, existing-identity `GeneratePreKeys`,
`ProcessBundle`, personal encrypt/decrypt, group distribution creation,
authenticated personal-envelope distribution processing, group encrypt/decrypt.
`ProcessGroupDistribution` combines personal ratchet/prekey consumption and
group-key installation in one delta. No raw unauthenticated distribution import
is exposed. Group signatures are verified asynchronously over the exact signed
body before any group chain can advance; the original wrapper's inverted,
off-by-one check and falsey sender-ID0 lookup are corrected in adapter glue.

The SQLite actor must atomically CAS `expected_revision`, validate account and
active registration bindings, apply delta and either persist message+cursor+
dedupe+postcommit receipt, or stable client ID+exact ciphertext outbox. It alone
owns commit, rollback, allocation counters, retry and ACK eligibility. A failed
commit retries from the same committed snapshot. Outgoing multi-device fanout
must sequence overlays under one account revision and commit all ratchets and
the complete immutable outbox together. Concurrent independent stale snapshots
must never be accepted. OperationResult has private fields, immutable getters and a consuming
`into_parts()` method; it cannot be constructed or changed through public safe
APIs. The trusted coordinator must consume that original result and bind its
exact bytes to the atomic outbox before transport publication.

## Scope, trust and opaque records

Durable keys include local epoch/platform and, where applicable, peer UID,
platform and peer epoch. Epochs are store-allocated canonical nonzero `u64`
strings, stable across restart, distinct from auth generation and collision-prone
14-bit registration IDs. Registration ID0 is valid. UID/group IDs are canonical
nonzero decimal strings. Constructors, serde and nested snapshot validation all
enforce these rules; unknown key fields are rejected. Request IDs/revisions cross
JS as strings; timestamps are bounded to `2^53-1`.

`TrustDecision` supplies the exact approved peer epoch, identity, registration ID,
session timestamp and `TrustBasis`. AuthenticatedFirstUse records an authenticated
first observation; it is not independent master-key proof. The upstream policy
must correlate authenticated UID/device metadata and verify the signed bundle.
A replacement identity in an existing epoch fails. The engine neither approves
arbitrary keys nor treats IID as proof. Snapshot selection must include the
current identity/session/binding records, local identity and relevant prekey and
signed-prekey IDs from the incoming public header; group operations also require
the specific sender record. Never omit existing bindings to simulate first use.

Record values are opaque JSON bytes, preserved by the store without conversion:
local identity `{publicKey,privateKey,registrationId}`; prekey/signed prekey
`{publicKey,privateKey}`; session is a JSON string containing the original session
serialization; peer identity `{registrationId,identityKey,sessionTimestamp,basis}`;
session timestamp is an integer; group record is
`{registrationId,identityKey,record,distributionTimestamp}`. Byte arrays are JSON
integers. This candidate codec is not a promised long-term migration format.
Secret-bearing DTOs intentionally do not implement Debug. Memory is not promised
to be zeroized by JS GC; isolation is capability restriction, not an OS sandbox.

## Limits and lifecycle

- One dedicated 4MiB OS-thread stack; lazy QuickJS heap64MiB/JS stack2MiB.
- At most16 outstanding tickets, including completed uncollected results.
- Snapshot/delta/encoded request/response2MiB, maximum512 records; keys+32bytes
  overhead count toward snapshot/delta budgets. Select only required records.
- Application text64KiB; ciphertext/native primitive input128KiB accounts for
  CBC padding and protobuf/MAC/signature framing. OS random requests64KiB.
- Default per-operation deadline5s, configurable1ms–60s. Busy JS and promise jobs
  check deadline/cancellation/generation; native calls are cooperative, bounded
  by fixed key sizes and128KiB input and checked between JS jobs.
- Cancellation, dropped tickets, logout-generation advances and shutdown reject
  late result collection. Runtime/thread teardown is asynchronous; no UI join.
  JS heap and stack limits are not total process RSS limits.

## Prekey maintenance and unresolved lifecycle

Own source N165174–203 handles command549/type1 by generating **20** one-time keys
on the existing identity, persisting private keys, then uploading command542.
N167654–658 allocates IDs as `registrationId+2+(++currentOpId)`; initial20 are
`reg+3..reg+22`. `GeneratePreKeys {local,reserved_ids}` accepts1–100 unique uint32
IDs, rejects existing-key overwrite and changes only those private-key records.
100 is this worker's resource cap, not a protocol field limit. The actor must
durably reserve IDs/counters and retain consumed-ID history; absence of a consumed
private record does not authorize reuse. Public keys become upload-eligible only
after private delta+allocation metadata+upload outbox commit. Counter exhaustion
requires a controlled registration transition, never wraparound.

N167660–680 generates signed prekey ID`reg+1` with a new bundle. Command549/type2
calls a full new-bundle flow online. No independent signed-prekey rotation
schedule/server acceptance was established: **RotateSignedPreKey is not
implemented**. Existing signed keys must be retained for late incoming messages;
neither replenishment nor retry clears identity, sessions or old keys. Real
registration, authenticated bundle fetching/trust policy, transaction coordinator,
device fanout, ACKs and live end-to-end interoperability remain integration gates.

## Verification and provenance

```sh
CARGO_TARGET_DIR=/tmp/zalo-signal-engine-target cargo test --locked --offline
CARGO_TARGET_DIR=/tmp/zalo-signal-engine-target cargo test --locked --offline --all-features
CARGO_TARGET_DIR=/tmp/zalo-signal-engine-target cargo clippy --locked --offline --all-targets --all-features -- -D warnings
cargo fmt --check
```

Executed:14 tests (3 actual-host resource/bridge tests +11 public API tests).
They cover original-asm public wire fixtures independently checked by Python/
OpenSSL, bidirectional personal/group, out-of-order/replay/tamper,64KiB personal/
group, restart, failed caller commit,20-key replenishment, CSPRNG identities,
registration0, canonical serde/nested scopes, lossless u64 IDs, backpressure,
cancel/deadline and heap/stack rejection. Synthetic tests do not verify live
account provisioning or messaging.

Pinned [rquickjs0.14.0](https://github.com/DelSkayn/rquickjs/tree/v0.14.0)
embeds QuickJS-NG0.16.2 (MIT); optional
[libsignal-core904c8dc](https://github.com/signalapp/libsignal/tree/904c8dc8216073b078a5f2f5090e3da8382e6a92/rust/core)
is AGPL-3.0. RustCrypto AES-CBC/HMAC/SHA2 and OS CSPRNG provide the narrow WebCrypto
bridge; no custom cryptographic primitive or ratchet is implemented. Exact own
repository library SHA256 is `abbb3f578ede0d16c57b792a1c1793eb221dda07cf6f94919a3e76ee8a047966`;
extracted group+adapter prelude SHA256 is
`cd00830ac06f57ecf23c1162b650dbfa738af85a806b8b6a184942d63b502427`.
Runtime verifies both before evaluation. Build output stays under `/tmp`.
See `../../E2EE_CONTRACT.md` and the independent experiment for exact source
anchors and measured14.6MiB idle/3.63ms session/approximately3ms1KiB round trips.
Those measurements are experiment results, not a fresh library benchmark.
Local implementation/testing approval does not resolve ownership or distribution
obligations of the bundled source or AGPL dependency. No publishing/packaging or
repository-wide license declaration is included.
