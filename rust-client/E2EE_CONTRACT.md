# E2EE interoperability contract

Status: **offline compatibility research; production Signal sends, registration and
queue acknowledgements remain disabled**. Login success does not provision Signal
sessions. No private profile, real key, server registration or message was used.

Source notation: **S** is [`libsignal-protocol.static.js`](../pc-dist/libs/libsignal-protocol.static.js);
**N** is the [shared worker bundle](../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js).
Line numbers describe this checkout. S is 1,796,140 bytes, SHA-256
`abbb3f578ede0d16c57b792a1c1793eb221dda07cf6f94919a3e76ee8a047966`.

## Personal session and wire contract

- S39091–39148 exports KeyHelper and VERSION=2; this is the library version,
  **not** the message wire version. Tagged Curve25519 public keys are 33 bytes
  starting `05`; private keys are 32 bytes. Registration IDs are 14 bits.
- S39343–39481 implements signature-checked prekey establishment and X3DH.
  HKDF-SHA256 labels are **`ZaloText`, `ZaloRatchet`, `ZaloMessageKeys`**.
  Stock Signal labels differ and produce incompatible ciphertext.
- S39485–39505 emits type 3 prekey or type 1 ordinary messages. Both use wire
  header `33`; ordinary message protobuf contains ratchet key/counter/previous
  counter/ciphertext. AES-256-CBC/PKCS#7 keys and IV come from the message KDF.
  The last 8 bytes authenticate sender identity, receiver identity, header and
  protobuf using HMAC-SHA256. Prekey protobuf adds base key, identity, registration
  ID and prekey IDs.
- S39540–39670 checks MAC/version, consumes a one-time prekey after successful
  session storage, retains skipped keys and rejects reused counters. The default
  future/skipped-key window is 2,000. Preserve per-address serial execution.
- S39363–39395 and S39520–39533 add timestamp-aware session archival/promotion
  through `getSessionTs`; a generic upstream adapter must reproduce these rules.
  Session records are versioned JSON with binary strings, not upstream Rust's
  protobuf storage format. Do not deserialize them as interchangeable records.

## Registration, transport and device lifecycle

N103388–103425 defines authenticated WSS commands:

| Commands | Meaning |
| --- | --- |
| 540 / 541 / 542 | Submit bundle / fetch bundles / replenish one-time prekeys |
| 543 / 544 / 561 | Personal init / init ACK / group init |
| 547 / 548 / 549 | Delete key / key-deletion event / request more prekeys |
| 560 / 562 / 563 / 564 | Capability personal/group / paginated metadata / own uploaded key ID |
| 516 / 517 / 518 / 519 | Session offline queues and offline ACKs |
| 551 / 552 / 514 | Personal/group receive and group offline messages |
| 557 / 558 | Delete-queue ACKs; never emit before durable processing |

N103452–103454 maps mobile=0, web=1, PC=2. Addresses and all storage keys must
include account UID, peer UID, device platform and registration generation.
N403946–404255 specifies `signal_req_*` request correlation, init envelopes and
bundle fields: `deviceId`, `preKey.iKey.{id,key}`, `spKey.{id,key}`, signature,
`opKeys[{id,key}]`. Public keys/signatures are base64. N166000–166108 reconciles
durable local keys with command 564, then submits/reinitializes if necessary;
N166230–166233 generates and registers a new bundle. These are account-changing
operations, not read-only connection initialization.

N331600–331646 encrypts text and text properties separately per destination
device. N399766–399794 posts encrypted form `params` to
`/api/e2ee/pc/message/sms` (16000) or `/api/e2ee/pc/group/sms` (16100). Personal
outer payload includes JSON `listParams`, `destType`, `threadId`, `clientId`;
group uses the first group-specific parameter record plus `clientId`.
This requires verified per-device addressing, registration IDs, IID metadata
and session provisioning, not merely replacing legacy text with base64.

QR trusted-device companion identity, primary signatures and signed device lists
are distinct from Signal registration/peer sessions. Reusing the QR key without
an explicit binding contract is unsupported. IID/device-change handling appears
at N167015–167090 and N167508–167565: old-device data, duplicate messages, unknown
sessions and retry requests have distinct outcomes. N404771 starts Signal before
offline queue processing; session distribution must precede dependent messages.

## Group sender keys and two unsafe source branches

N160575–161057 defines group distribution/message/state protobufs. Group messages
have header `33`, `{id,iteration,ciphertext}` and a 64-byte Curve signature.
HMAC chain seeds `01`/`02` generate message/next-chain seeds; HKDF label
**`ZaloGroup`** yields 48 bytes: IV first 16, AES key next 32. Records retain up
to five sender states and 2,000 skipped keys. Group sender identity also includes
group ID and peer device. Sender-key distribution travels through provisioned
pairwise sessions; group membership changes require reinitialization/invalidation.

Two independently exercised/source-visible defects must **not** be reproduced:

1. N160703–160708 group `verifySignature` includes one signature byte in the
   signed slice and inverts the synchronous Curve return convention. The exact
   extracted wrapper accepts a forged signature in the offline regression.
   Before any ratchet load/mutation, require the verified asynchronous Curve
   API on exactly `wire[..len-64]` and `wire[len-64..]`; reject on failure.
   The harness proves rejection happens before storage access and then decrypts
   the genuine group message. This proves a source-branch defect, not that a
   particular production account executes that branch.
2. N167646–167652 derives an identity seed from SHA256(`uid:deviceId`). S's key
   generator deterministically uses that input, so the secret is predictable.
   New native identities must use OS CSPRNG; never derive secrets from IDs or
   copy this branch. The existing store also returns unconditional trust at
   N158745–158750. Preserve verified identity/device binding and require explicit
   identity-change handling rather than silently accepting replacement keys.

S's SessionLock also leaves a rejected cleanup promise on expected failures.
The research harness records and checks these rejections; a production wrapper
needs handled cleanup and typed failures without exposing key material.

## Engine decision and transaction boundary

**No production engine has been enabled.** The measured implementation candidate
is lazy embedded QuickJS with official native Curve primitives; see the executed
[isolated experiment](experiments/e2ee-engine/README.md). It passes 36
interoperability checks and 8 resource/bridge tests. The optional primitive-only
dependency remains confined to that separate workspace; production transactional
integration and distribution review remain outstanding.

The original compatibility
oracle executes the repository's exact library in Node/V8 without Electron,
WebView or DOM. Node 22.22.1 supplies WebCrypto and the host only reads checked-in
fixtures. Node's VM is **not a security sandbox**.

Next compare these paths before adding dependencies:

- Lazy isolated Node helper: strongest immediate wire oracle; requires Node 22+
  and a documented missing-runtime diagnostic. `--max-old-space-size=96` limits
  only V8 old heap. This full research suite peaked at **260,288 KiB RSS**, 1.38 s
  wall time on this environment; it includes library compilation, group extracts
  and fixture allocations. It is neither a cold-idle benchmark nor evidence of
  production performance. Node is not approved as the default shipping engine.
- Embedded JS with a mature Rust WebCrypto/CSPRNG bridge: preserves original
  ratchet code, but QuickJS compatibility with asm.js, promise jobs, cancellation
  and actual throughput/memory must be proven. QuickJS does not JIT asm.js like
  V8. No handmade curve/signature/AES implementation is acceptable.
- Audited native library adaptation: official Rust libsignal implements current
  Signal, not this Zalo fork. Its current
  [message/ratchet labels](https://github.com/signalapp/libsignal/blob/main/rust/protocol/src/ratchet/keys.rs)
  differ, and [current handshakes](https://github.com/signalapp/libsignal/blob/main/rust/protocol/src/ratchet.rs)
  include PQXDH/SPQR. Select and pin a compatible revision and audit all deltas
  before claiming interoperability. The [official project](https://github.com/signalapp/libsignal)
  has AGPL licensing; do not assume the bundled artifact grants the same license
  or silently introduce a new distribution obligation. These links are research
  evidence, not pinned build dependencies.

Proposed worker API: bounded length-prefixed request/response (2 MiB maximum,
16 queued jobs), one account/generation, typed operation, expected state revision,
bounded immutable snapshots and deterministic mutation delta. Operations are
`CreateBundle`, `ProcessBundle`, `EncryptPersonal`, `DecryptPersonal`,
`ProcessGroupDistribution`, `EncryptGroup`, `DecryptGroup`. Production randomness
comes exclusively from CSPRNG. Logs contain stages/error categories, never keys,
plaintext, ciphertext or account-sensitive URLs.

The helper owns **no database or network**. Rust loads snapshots and commits
ratchet delta + identity decision + consumed prekey + message + queue cursor in
one SQLite transaction after revision/account checks. For sends, the same
transaction persists stable client ID and exact ciphertext in the outbox before
network submission. Retry reuses ciphertext, never re-encrypts an accepted/unknown
send. Failed commit discards mutations; stale revisions recompute from committed
state. Publish plaintext/ciphertext or ACK queues only after commit. Tests must
inject disk failure, stale revisions, replay, cancellation, reconnect and device
transition. Deadline expiry kills **and reaps** an isolated child and discards
uncommitted results; logout invalidates generation before publication.

An OS helper needs real resource limits and confinement review: CPU/address-space
or cgroup memory bounds, restricted inherited descriptors, explicit executable
path, no credential environment and pipe backpressure. Landlock/seccomp/network
namespace feasibility must be tested on supported Linux systems; a VM context or
Node heap flag does not establish these guarantees. No helper is shipping yet.

## Reproducible offline checks

```sh
python3 rust-client/tests/fixtures/e2ee/test_independent.py
node --max-old-space-size=96 --no-warnings rust-client/tests/fixtures/e2ee/contract_test.cjs
```

Results: **6 Python tests and 21 JS contract checks**. Python validates RFC5869,
reproduces KDF vectors, parses the exported prekey message, independently derives
receiver-side X3DH/ratchet/message keys using OpenSSL X25519, verifies the MAC and
decrypts AES-CBC. Stock Signal label and tampered ciphertext fail. JS additionally
checks published RFC7748 X25519 vectors, both session directions, out-of-order
messages, replay, tamper without ratchet loss, serialized restart, storage failure
and identity change, original group KDF and the forged-signature regression.

All secrets in [`tests/fixtures/e2ee`](tests/fixtures/e2ee) are deliberately public
synthetic fixtures. Deterministic fixture randomness must never enter production.
Regenerate only intentionally with `generate_vectors.py` and the JS
`--write-session` flag, then rerun the independent oracle. These checks establish
offline interoperability evidence, **not production account feature acceptance**.
