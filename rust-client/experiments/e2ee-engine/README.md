# Isolated embedded E2EE engine experiment

This separate Cargo workspace evaluates a lightweight compatibility engine. It
does not change the app, enable Signal registration, send messages, read profiles,
or acknowledge server queues. All input is checked-in public synthetic fixtures.

## Reproduce

Use the repository's pinned Rust toolchain, a C compiler and the existing `/tmp`
Cargo cache when available. Keep build artifacts outside the checkout:

```sh
cd rust-client/experiments/e2ee-engine
export CARGO_TARGET_DIR=/tmp/zalo-e2ee-engine-target
cargo fetch --locked
cargo test --locked --offline --all-features
cargo clippy --locked --offline --all-targets --all-features -- -D warnings
cargo run --locked --offline --release
cargo run --locked --offline --release --features native-curve
node --expose-gc --max-old-space-size=96 --no-warnings node-oracle.cjs
```

`native-curve` is **experimental, optional, and not a distribution decision**.
Its dependency is the official `libsignal-core` at commit
`904c8dc8216073b078a5f2f5090e3da8382e6a92`. The normal dependency graph includes
`libsignal-core`/`libsignal-debug`, not Signal's protocol, networking, PQXDH or
SPQR crates. Neither this feature nor QuickJS is in the main app manifest.

## Implementation and provenance

- [`rquickjs = 0.14.0`](https://github.com/DelSkayn/rquickjs/tree/v0.14.0)
  embeds QuickJS-NG 0.16.2, MIT licensed. Only its `std` feature is enabled;
  there is no module loader, dynamic loader or JS filesystem/network API.
- The exact repository `pc-dist/libs/libsignal-protocol.static.js` is
  SHA-256 checked before evaluation. Its old UMD glue requires non-strict global
  evaluation; its own strict directives remain intact. `build.rs` extracts exact
  original group classes using unique source anchors. No external client code
  or assets are included.
- RustCrypto AES/CBC PKCS#7, HMAC-SHA256 and SHA-256/SHA-512 implement the small
  Promise-compatible WebCrypto facade. Randomness comes from the OS. ArrayBuffer
  views preserve offsets; every native input is bounded to 128 KiB. This permits
  a full 64 KiB application text plus CBC/protobuf/MAC overhead. WebCrypto RNG
  requests retain the separate 64 KiB quota.
- The default variant executes original asm.js Curve primitives. The optional
  variant inserts **one documented leaf-API hook** before `Internal.Curve` is
  constructed. Zalo's KDF labels, ratchet code, message format and session code
  remain unchanged. Native Curve calls delegate to the official mature
  [Signal primitives](https://github.com/signalapp/libsignal/blob/904c8dc8216073b078a5f2f5090e3da8382e6a92/rust/core/src/curve.rs),
  including tagged public keys, clamping and compatible XEdDSA signature sign
  bits. It preserves the existing sync false-on-valid convention and async
  reject-on-invalid convention. Signature randomness uses an OS-seeded mature
  RNG; there is no custom cryptography.
- Group verification explicitly checks the exact header+protobuf body with the
  async Curve API before loading a sender-key record. The unsafe original group
  wrapper's inverted/incorrect check is never relied upon for authenticity.
- `libsignal-core` is AGPL-3.0-only. The repository has no identified top-level
  license grant for its bundled artifacts. Local experimental use is not a claim
  that a combined executable can be distributed under arbitrary terms. No
  publishing, packaging or license declaration is performed here.

## Executed gates and measurements

**8 Rust resource/bridge tests passed**, and strict Clippy passed. Tests prove
64 MiB JS heap rejection, 2 MiB JS stack rejection on a 4 MiB OS worker,
deadline/operator cancellation, poisoned-runtime rejection, bounded promise-job
execution, absent OS/network APIs, typed-array offsets, CSPRNG and data-only JSON.
An initial stack test incorrectly used Rust's default 2 MiB test-thread stack;
matching the actual 4 MiB worker resolves the host stack overflow.

The final native variant passed **36 interoperability checks**, including the
independent Python/OpenSSL prekey fixture, both session directions, Vietnamese
UTF-8, personal/group out-of-order/replay, tamper, restart, storage failure/retry,
identity change, and full 64 KiB personal/group text. Additional checks verify
RFC7748 X25519, native/asm XEdDSA signatures **in both directions**, malformed
signatures, low-order key agreement rejection, and forged group rejection before
storage access. The original 6 Python/21 JS research checks remain separate.

Representative local release observations, same core workload and per-message
payloads (50 personal and 20 group 1 KiB roundtrips). Idle measurements follow
GC; Node's temporary whole-worker source extraction is released before sampling.
This is an engine experiment, not an Electron/app performance comparison.

| Variant | Cold load | Idle RSS | Session establishment | Personal 1 KiB roundtrip | Group 1 KiB roundtrip |
| --- | ---: | ---: | ---: | ---: | ---: |
| Node 22.22.1/V8, original asm | 313 ms | 65,124 KiB | 202 ms | 2.44 ms | 22.78 ms |
| QuickJS, original asm | 102 ms | 14,456 KiB | 16,944 ms | 2.97 ms | 1,619 ms |
| QuickJS + official native Curve | 67 ms | 14,620 KiB | 3.63 ms | 2.99 ms | 2.84 ms |

QuickJS native known-answer decrypt took 1.34 ms; identity generation 0.081 ms;
signed-prekey generation plus valid/invalid verification 0.216 ms; snapshot
serialization 0.0115 ms. Its final normal workload, including both 64 KiB boundary
messages, took 488 ms. Additional original-asm cross-verification took 4.23 s and
is reported separately. Original-asm QuickJS took 57–71 s across two earlier
20-check runs, before boundary checks were added. Its interpreter crypto fails
interactive latency despite lower memory. Node `--lite-mode` used 61,740 KiB idle
but took 4.57 s to establish a session and 401 ms/group roundtrip.

RSS is process memory, not the JS allocation counter. QuickJS reports about
23.96 MB allocated heap, including its mostly unfaulted 16 MiB asm ArrayBuffer;
RSS can therefore be lower. Neither the 64 MiB JS allocator ceiling nor Node's
old-heap flag bounds whole-process RSS. CPU contention and random key values vary
timings; these observations establish feasibility rather than product benchmarks.

## Recommendation and next integration boundary

The measured candidate is **lazy embedded QuickJS with mature native Curve
primitives on a dedicated OS worker**, subject to provenance/distribution review
and production adapter acceptance. Keep Node and original asm as independent
offline oracles. Do not ship interpreter-only Curve or rewrite Zalo's ratchet.

The next adapter takes typed `{account_id, generation, operation_id,
expected_revision, operation, snapshot}` and returns typed
`{account_id, generation, operation_id, expected_revision, output, mutation_delta}`.
Cap complete snapshots/requests to 2 MiB and queued operations to 16. Snapshot
keys include account, peer/group, device and registration generation. No database
or network handles enter JS. Inputs are JSON/ArrayBuffers, never source strings.

Rust must atomically commit ratchet/identity/prekey mutations with the received
message and queue cursor, or with a stable outbox ID and exact send ciphertext.
Only then may it publish plaintext, submit ciphertext or ACK a queue. Stale
revisions, failed commits or cancellation discard the operation's overlay;
retries reuse committed ciphertext. Check generation/cancellation/deadline before
and after each bounded native call and before publication. The current 120 s cap
is only for the many-operation research suite; production operations need their
own deadlines. Interrupted/failed runtimes are discarded, never reused. Rust
actor transaction and live device provisioning acceptance are still outstanding.
