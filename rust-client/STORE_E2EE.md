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
remain readable. Planning checks the active SQLite registration ID and identity
fingerprint against engine identity bytes and checks peer session timestamps;
malformed opaque identity records cannot redefine an existing verified epoch.
Pending verified peer transitions activate inside the same SQL
transaction as their ratchet mutations and received message/cursor. Own outgoing
group sender records bind directly to the owner/local platform/epoch; remote
senders require a verified peer allocation. No synthetic self-peer is allocated.

Snapshots request explicit keys and enforce 512 records / 2 MiB before allocating
stored values. Delta writes must fit the planned keyset and byte budget, match an
expected account crypto revision, and pass operation-ID/input/result replay
checks. Durable logical operation reservations survive a failed transaction or
restart; an engine job sequence is separate and remapped only after matching its
sealed result against the submitted account/generation/revision/local/planned
keys. A reservation never consumes ratchet state. Receives commit record changes, consumed prekeys, identity decisions,
messages and queue cursor together. Only a privately constructed postcommit
receipt exposes publication/ACK information. Callers must still check its active
generation before publishing.

Cancellation uses nonblocking atomics. Cancellation before the final commit CAS
rolls back; a commit that linearizes first may finish, but stale-generation
receipts are unusable. Cleanup uses compare-and-swap and cannot resurrect a
cancelled or replaced generation. The coordinator's separate `Cancellation`
token controls preparation only. Its final cancellation/deadline check runs
after the auth CAS and before SQL COMMIT: failure rolls everything back; a
cancellation after that check may leave a durable Ready outbox entry. This token
does not revoke Ready after restart and is not a user Cancel Send feature. Such a
feature needs a future durable actor command/acknowledgment. Auth/logout remains
authoritative through GenerationGate and a fresh receipt check before dispatch.

Sends reserve a stable client ID before encryption. The ratchet delta and exact
inner SMS payload/metadata commit together into Ready. An opaque DispatchReceipt
is produced only by a committed Ready-to-Sending transition and binds account,
generation, operation, revision, epochs, client ID and exact payload hash. It
proves durability. A separate codec-1 verified marker is written atomically only
by `e2ee_commit_verified_send`, which accepts an opaque coordinator candidate
built from genuine sealed engine results. Raw `PreparedSend` bytes/metadata never
mint that marker. Restart dispatch checks the persisted codec, operation,
revision, epochs and exact payload hash; the wire adapter rejects receipts
without that marker. Reserved, Ready, Sending, Unknown, Acknowledged and Committed are
distinct; recovery changes Sending to Unknown. Unknown delivery is never selected
for automatic resend or reencryption. ACK/echo reconciliation converges to one
message. Per-device ciphertext remains bounded at 128 KiB; aggregate prepared
SMS payload, metadata and peer bindings are capped at 1 MiB, with 64 peer bindings.

Receive input identity binds account, target kind, thread, server message ID,
sender UID/platform/durable epoch, raw 14-bit registration ID and ciphertext
hash. Queue is a delivery route, not a second decrypt operation. Before resolving
current peer bindings, `e2ee_lookup_committed_input` uses the indexed account/hash
ledger and checks the wire-visible identity to recover the original epoch.
`e2ee_replay_receive` then atomically commits a new queue delivery/cursor receipt
without engine execution, prekey consumption or UI republication. Changed cipher
under one identity and identical cipher under another identity fail closed.
Read-only receipt lookup cannot authorize a new cursor.

`signal-crypto` adds the embedded engine and an optional preparation coordinator;
it does not enable live Signal registration, send, receive or ACK. Actor integration
must use synchronous plan/reserve/snapshot, owned asynchronous preparation, then
synchronous candidate commit; never hold the sole DB writer across crypto awaits.
Snapshots/overlays are discarded after cancellation, stale revision or failed commit.
Remaining gates include production actor jobs, authenticated bundle decisions,
source-backed initial local provisioning/prekey upload receipts, and live integration.
Initial provisioning needs a pending local epoch before CreateBundle, followed by
atomic private bundle/public upload staging and server-540 ACK activation. Existing
`e2ee_register_local` is a verified-transition primitive, not that provisioning API.
No production Signal path uses this checkpoint yet.


## Pending local provisioning (unreleased)

The optional `provisioning` module reserves a durable PC candidate epoch and
operation ID with an empty snapshot. It creates no local binding or timestamp.
A sealed creation candidate must match the planned namespace/count and source
key-ID policy; private records (2 MiB maximum) and the exact public 540 inner
payload (64 KiB maximum) commit together into separate pending tables. The old
active epoch, private records and history remain available. Restart discovery
returns reservation metadata, never pending private bytes.

Ready-to-Sending commits an opaque one-use upload receipt before network use.
Request IDs combine durable operation/attempt IDs, with a persisted uniqueness
constraint; reconnect cannot reset them. The receipt binds account, generation,
socket epoch and public/private payload hashes. Recovery changes Sending to
Unknown, which cannot be reclaimed or automatically uploaded again. This module
currently permits one unresolved candidate per account; a future activation flow
must distinguish historical resolved candidates before allowing a new rotation.

There is no activation or raw server-proof constructor in this increment.
Authenticated correlated 540/541 evidence and real binding metadata must be
implemented before moving pending records into the active namespace. A 564 IID
and timestamp alone cannot establish identity, and a sampled 541 OPK does not
prove acceptance of an entire uploaded batch. The SQL compatibility fixture
`tests/fixtures/e2ee/released_v4_90b25d5.sql` contains exact released schema SQL
and no profile data; a default-feature test preserves every synthetic row column
across all thirteen pre-existing tables while adding the new tables.

## Schema nine: typed initialization attempts

The v8→v9 upgrade is additive and transactional: `e2ee_peer_init_outbox` stores
only `(account, client_id)`, with a foreign key/cascade to `e2ee_outbox`. A partial
outbox index bounds pending checks by account, local epoch, platform and thread;
the query plan uses indexed searches rather than scanning historical sends.
Existing rows, ciphertext, metadata and message history are not rewritten.
Older binaries reject schema nine; there is no automatic downgrade.

An initialization marks its reserved row before the engine can commit a session.
The pending query joins the typed marker and refuses unresolved attempts only in
the current scoped namespace; ordinary unknown chat is not initialization.
After a correlated ACK, `e2ee_confirm_peer_init` confirms and retires the marked
attempt in one transaction. Repeated confirmation with the same ID is idempotent;
conflicting IDs or a failed write retain the prior state and exact bytes. A
discarded reservation removes its marker through the foreign key. This is intent
metadata, not permission to upload, replace an identity or acknowledge a queue.

Legacy unmarked unknown attempts are ambiguous. Neither draft text nor server-ID
guessing is used to classify them, so the typed guard cannot guarantee that it
blocks a historical unmarked init. Existing first-use peer registration may
leave a new binding staged if later engine signature verification fails; this
checkpoint does not add full rollback of those new bindings.
