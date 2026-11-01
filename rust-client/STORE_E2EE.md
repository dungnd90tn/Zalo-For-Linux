# Durable E2EE storage checkpoint

`store::e2ee` adds schema version 4 unconditionally to the existing SQLite owner.
It does not create a second connection or enable E2EE in live runtime. Versions 2
and 3 migrate by adding tables; messages, legacy outgoing rows, receive cursors
and directory metadata remain intact. Optional `signal-store` supplies a DTO-only
bridge to the independently hosted engine; embedded QuickJS/native Signal code
is not a default dependency.

Crypto addresses include account, durable local registration epoch, platform
(0 mobile, 1 web, 2 PC), and peer UID/platform/verified registration epoch. The
binding stores 14-bit registration ID (including zero), identity fingerprint and
session timestamp. IID alone never names a device generation. Retired namespaces
remain readable. Pending verified peer transitions activate inside the same SQL
transaction as their ratchet mutations and received message/cursor.

Snapshots request explicit keys and enforce 512 records / 2 MiB before allocating
stored values. Delta writes must fit the planned keyset and byte budget, match an
expected account crypto revision, and pass operation-ID/input/result replay
checks. Receives commit record changes, consumed prekeys, identity decisions,
messages and queue cursor together. Only a privately constructed postcommit
receipt exposes publication/ACK information. Callers must still check its active
generation before publishing.

Cancellation uses nonblocking atomics. Cancellation before the final commit CAS
rolls back; a commit that linearizes first may finish, but stale-generation
receipts are unusable. Cleanup uses compare-and-swap and cannot resurrect a
cancelled or replaced generation.

Sends reserve a stable client ID before encryption. The ratchet delta and exact
inner SMS payload/metadata commit together into Ready. An opaque DispatchReceipt
is produced only by a committed Ready-to-Sending transition and binds account,
generation, operation, revision, epochs, client ID and exact payload hash. It
proves durability, **not cryptographic engine provenance**: the future runtime
adapter must validate and consume a genuine sealed engine result before preparing
the envelope. Reserved, Ready, Sending, Unknown, Acknowledged and Committed are
distinct; recovery changes Sending to Unknown. Unknown delivery is never selected
for automatic resend or reencryption. ACK/echo reconciliation converges to one
message. Per-device ciphertext remains bounded at 128 KiB; aggregate prepared
SMS payload, metadata and peer bindings are capped at 1 MiB, with 64 peer bindings.

This checkpoint is not approved for live E2EE activation. Remaining integration
work includes actual engine/database/network actor jobs, source-backed initial
local registration provisioning and prekey upload receipts, own-group sender
namespace validation, and legitimate online/offline cross-queue replay handling.
The current input ledger conservatively rejects cross-queue repeats; it must be
extended to reuse a committed decrypt for the same server identity while committing
a new queue cursor, without accepting the same cipher under a different identity.
No production Signal ACK, registration or send path uses these primitives yet.
