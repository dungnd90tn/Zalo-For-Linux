# Optional trusted personal crypto preparation

`signal-crypto` adds an inactive preparation API; neither `live` nor startup enables
it. The released470bc5 artifact is unchanged. No account lookup, key upload,
registration, HTTP send or ACK occurs in this module.

## Actor and capability boundary

1. The sole SQLite actor calls `Coordinator::plan_personal`: load the Reserved
   draft, validate its exact text/local epoch and every active peer's IID,
   fingerprint, platform, epoch and session timestamp; reserve one stable logical
   operation ID; read the complete planned committed snapshot.
2. Move opaque `PreparedPersonalInput` into asynchronous `prepare_personal`.
   It owns all data and borrows no database connection/transaction. The lazy
   dedicated engine thread processes recipients sequentially against cloned
   overlays using **the same base revision**. Every sealed result is checked
   against its actual transient job/account/generation/revision/planned keyset.
   Intermediates never enter the database or a hidden engine store.
3. Return opaque `VerifiedPreparedSend`. The actor calls
   `e2ee_commit_verified_send`: one CAS commits every ratchet, the complete SMS
   bytes, durable operation ledger, outbox and versioned verification marker.
   A failed recipient or SQL transaction discards the entire preparation.
4. `e2ee_begin_dispatch` claims READY→SENDING once and verifies the exact payload
   hash and active epochs. `authorize_dispatch` consumes that non-Clone receipt
   and returns a non-Clone envelope retaining its authority. A future HTTP adapter
   must consume the envelope once and recheck generation immediately before an
   attempt. This module performs no attempt. Reopened READY rows recover proof
   from the trusted SQL marker; raw `PreparedSend` cannot mint that marker.

Cancellation and the15-second whole-preparation deadline remain attached to the
sealed candidate. The trusted commit checks them at entry and after acquiring the
generation CAS lease, before COMMIT; an error rolls back all effects. Cancellation
after the transaction's linearization may leave committed READY bytes, but stale
generation cannot authorize publication. The engine also bounds individual jobs
and cancels dropped tickets. No SQL transaction spans an engine await.
This token cancels preparation only. It does not cancel a durable READY send,
revoke restored dispatch authority or implement a user-facing CancelSend action;
such an action needs a separate durable actor command before network use.

## Source behavior and retry identity

Own bundle N331596–610 sends `[text, stylesJSONOrEmpty]`; N166812–834 skips an
empty string and N166463 converts nonempty text directly to UTF-8. Ordinary text
therefore advances each recipient ratchet once and omits `textProperties`.
Independent original-JS oracle fixtures cover Unicode, escaping and64KiB text.
Styled text is not yet accepted by this coordinator.

The stable input digest binds composer text, client ID, thread, local epoch and
verified recipient identity/IID/platform/epoch/session timestamp. It excludes
ephemeral541 prekey selections: a known-unsent failed-commit retry may fetch a
fresh one-time key while retaining its durable ID. Every retry uses a fresh
monotonic engine job sequence and the newly committed snapshot. READY, SENDING
and UNKNOWN sends cannot re-enter preparation. The actual accepted ciphertext
and complete delta are bound by the commit digest, revision CAS and payload hash.

Snapshot selection includes existing local identity and every selected peer's
identity/session/timestamp, including absent planned keys. It never omits an
existing binding to simulate first use. The public prekey-header planner selects
only referenced one-time/signed keys and rejects duplicate/noncanonical fields,
zero record IDs, malformed tagged keys, uncorrelated identities/IIDs and IID
overflow. Header parsing is not authentication; the engine remains authoritative.

## Pending local creation

The actor can now reserve an opaque pending `ProvisionPlan` without an active
identity, registration ID or invented server timestamp. `PendingProvisioner`
consumes that owned empty-snapshot plan asynchronously and returns a sealed
`EngineCreationCandidate` from the genuine bounded engine's CSPRNG CreateBundle.
The candidate verifies account/generation/job/revision, the exact private record
whitelist and source IDs (`SPK=reg+1`, `OPKs=reg+3..reg+2+count`). The pinned mature
Signal Core derives and checks **every** private/public key pair and verifies the
signed-prekey signature independently of JSON field equality. Private DTOs do
not implement Debug; malformed private records return static errors.

The exact public inner540 payload comes from the existing source-backed wire
builder with `delSS=0`; no request ID, auth generation or socket epoch is baked
into staged bytes. Its domain-separated SHA256 binds all public fields, including
OPKs, for durable staging integrity. That full payload hash is not a claim that
a future541 query returns an unchanged unconsumed OPK pool.

`e2ee_stage_verified_bundle` atomically commits pending private records, public
outbox and input ledger. Current active keys/bindings remain unchanged. Reopened
READY candidates retain those exact keys/bytes. The separate one-use upload claim
binds its persisted attempt ID, request ID, generation and socket epoch; crash
recovery changes SENDING to UNKNOWN and refuses automatic re-upload/regeneration.
Cancellation/deadline checks use the same pre-COMMIT boundary as personal sends.

Create **one** `AccountEngine` for an account/auth lifecycle and derive both
`worker.coordinator(active_local)` and `worker.provisioner()` handles. They share
one lazy JS runtime, bounded queue and monotonic job allocator. Allocation and
`Engine.submit` are serialized in one short synchronous mutex section; no await,
JS execution or SQL runs under that lock. Convenience constructors still create
an independent account factory for callers that have no other worker; the future
orchestrator must reuse its factory rather than invoke both constructors. This
is explicit lifecycle ownership, not a global account/key cache.

## Executed and remaining gates

`cargo test --locked --offline --features signal-crypto --test signal_crypto`
exercises real engine + real SQLite + pure wire builders: two-device roundtrip,
one revision increment, failure on a later device, SQL rollback then fresh-key
retry, restart dispatch, one-attempt claims, canonical header fixtures, sealed
scope negatives, cancellation/generation/CAS failures and misbound local/peer
records. Test setup explicitly imports pre-existing public synthetic fixture
keys; it does not implement production local provisioning.

This checkpoint covers personal plain-text preparation only. Runtime wiring,
complete eligible-device discovery, receive/decrypt-to-message/cursor orchestration,
nonempty styles, group preparation and authenticated bundle/trust orchestration
remain inactive. Source broadcast addresses comprise up to3 remote platforms and
2 other own devices, excluding the current device; wire capacity6 is not a claim
that6 valid recipients exist. The future orchestrator must supply **all** eligible
devices rather than an arbitrary subset.

Pending local-epoch reservation, actual CSPRNG creation, atomic staging and
durable public-upload claims are implemented and inactive. Authenticated540 ACK
reconciliation and activation are still absent; UNKNOWN recovery must verify
the pending identity/SPK using source-supported correlated evidence rather than
trusting564's collision-prone14-bit ID alone. No provisioning path changes active
keys or invents timestamps. Existing `register_local` is not used as a fake
prebinding step for new generated identities.
No fabricated IID, master proof or first-contact approval flow is introduced.
Embedded engine/source licensing and distribution review remain prerequisites to
packaging this optional candidate.
