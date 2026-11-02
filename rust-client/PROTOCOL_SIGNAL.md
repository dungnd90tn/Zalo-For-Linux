# Signal transport contract

This module is independently derived from this repository's current PC bundle.
It performs no network I/O, registration, key upload, message send, database write,
queue ACK or startup action. The working `470bc5` release does not include this
future pipeline. A successful login or socket AUTH does not establish Signal keys.

**N** below denotes
[`default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js`](../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js).
Cryptographic interoperability and unsafe original branches are documented in
[E2EE_CONTRACT.md](E2EE_CONTRACT.md); durable transaction rules are in
[STORE_E2EE.md](STORE_E2EE.md).

## Identity and authority

`Scope` binds account UID and transient authentication generation. `Address`
also binds peer UID, Signal device platform, durable local epoch and durable peer
epoch. Platforms are mobile **0**, web **1**, PC **2** (N103452); they are unrelated
to desktop authentication client type **24**. Epochs are allocated durable database
namespaces, not invented server IIDs or authentication generations.

`preKey.iKey.id`, metadata `devices[].iid`, remote encryption registration ID and
optional `sender_iid` have distinct roles (N404223–404255, N164623–164679,
N331491–331509). Registration IDs are 14-bit values, including zero. The original
SMS formatter omits zero-valued IID extensions. Group sender-key messages have
no fabricated per-device IID. Preserve verified identity, registration ID and
session timestamp together when assigning/replacing a peer epoch.

Public key parsing validates canonical base64, 33-byte `05` keys, 64-byte signatures
and bounded key lists. **Shape validation is not signature or identity verification.**
Requests, bundles, encrypted records and business text deliberately omit Debug and
Serialize. Errors contain closed categories, never peer IDs, bodies or key material.

## Request and response contracts

N403800–404299 supplies `signal_req_<sequence>` correlation and these subcommand-0
request bodies, wrapped as `{reqId, data}`:

| Command | Body / interpretation |
| --- | --- |
| 540 | `delSS`, `key:{deviceId,preKey:{iKey:{id,key},spKey:{id,key},signature},opKeys:[{id,key}]}` |
| 541 | `uids:[{uid,deviceId}]`, `isReInit`, `msgInfoNoti`, `isAutoUpgrade`; device `-1` selects all |
| 542 | `deviceId`, local registration `id`, new `opKeys` |
| 543 | `type`, `uidTo`, `data:[{deviceId,data}]`; init requires a type-3 prekey envelope |
| 544 | `type:1`, `uidTo`, `data:[{deviceId,ts,data}]`; zero timestamp is legal |
| 561 | `type` (create 0/reinit 1/response 2), `uidTo`, `tId`, per-device encrypted distribution; absent `ts` stays absent |
| 564 | `platformType`; returns `keyId`, `submitTs`; **correlated inner** `err:-1` means missing |
| 547 | `keyId`, `uids` or `groupIds`, `ts`; explicit key invalidation, never a fallback |
| 560 / 562 | Personal capability `uids`; group capability `uids,groupId,enableEE` |
| 563 | `reqIid:1`, `first`, optional opaque `lastId` |

Responses are already decrypted, authenticated socket inner envelopes:
`{error_code:0,data:{reqId,data:{...}}}`. Require matching command, request ID,
account and generation. Numeric nonzero outer/inner errors remain typed failures;
missing optional inner `err` does not itself imply failure. An outer `-1` cannot
be interpreted as command-564 missing-key permission. Bundle replies must match
requested UID and selected device and contain unique devices; the engine must
verify signed prekeys before establishing a usable session. Source `-9` bundle
errors include per-device `errData`; this parser retains the service failure,
without guessing permission from a partial/missing bundle.

Metadata pages require all four arrays `threads/uids/waitUids/pendingUids`, explicit
boolean `hasMore`, advancing cursors and bounded pages/items. Only a final complete
snapshot can be returned. Conflicting device metadata fails closed; partial data
never proves a peer is eligible for legacy plaintext.

## Receive queues and initialization order

N103388–103425, N165791–165812 and N404551–404625 define:

- **516 / 517**: PC/web offline `eesession` pages. Hold sessions while `more` is true;
  process initialization before dependent encrypted messages.
- **543 / 545 / 544 / 561** session events: preserve original command, message ID,
  sender/device and timestamp. `545` has direction-dependent meaning; never derive
  an outgoing ACK from its numeric value alone.
- **548**: `sessionData.deleteType` 0 stale, 1 downgrade request, 3 force reinit
  (N165654–165691). Parsing does not delete keys or approve a downgrade.
- **549**: `sessionData.type` 1 requests one-time prekeys, 2 requests a bundle
  (N165172–165196). No replenishment occurs until generated private state is durable.
- **518 / 519**: `delivereds` receipt queues. Entries remain opaque descriptors;
  parsing them does not imply success or authorize resend/deletion.
- **551 / 552 / 514**: personal/group/group-offline encrypted message records.
  Preserve full records, routing, platform and timestamp for the engine adapter;
  ciphertext is never a UI text value.

`SessionPages` holds bounded pages until complete, rejects cursor cycles, deduplicates
identical IDs and rejects conflicting replays. Unsupported control events fail
without advancing a cursor. These pure parsers create **no ACK authority**. The
544 request formatter is test-only; no production queue/initialization ACK builder
exists yet. Future ACK emission must consume an input-bound post-COMMIT receipt,
including replay lookup, account/generation and exact queue identity.

Group distribution is a type-1/3 pairwise encrypted envelope containing the original
`0x33` sender-key distribution, not raw publicly trusted group key material
(N161524–161528, N162030–162060, N162271–162300). Pairwise decrypt, group distribution
install and message/cursor changes must share one engine overlay and SQL transaction.

## Ordinary text and SMS

N331596–331646 passes `[text, styleJSONOrEmpty]` to encryption. N166812–166834
encrypts each nonempty element; empty styles stay empty and are omitted from
`textProperties`. N166463 passes raw UTF-8 to SessionCipher: there is no extra
JSON/protobuf/thread/client-ID wrapper around ordinary text. The receiver obtains
text from string content or an RTF title (N166849–166856, N548349). Nonempty styles
are a separate encrypted JSON string. `BusinessText` validates immutable thread,
client ID and UTF-8 length before cryptographic work; the current bound is 64 KiB.

Personal `/api/e2ee/pc/message/sms` (metric 16000) carries JSON-string `listParams`,
`destType:3`, `threadId`, JavaScript-safe stable `clientId`. Each record binds
`message,toId,platform,e2eeExt`, optionally encrypted `textProperties`. Peer and own
subdevices share one local epoch; no duplicate destination/platform is allowed.
Group `/api/e2ee/pc/group/sms` (16100) carries `message,grid,clientId,visibility,e2eeExt`
(N399766–399794). Session AES/form encoding is a separate authenticated HTTP layer.
Bounds: at most six personal destinations, 128 KiB per ciphertext, 1 MiB aggregate
wire payload; larger results produce explicit resource errors before dispatch.

The test-only raw ciphertext constructors are unavailable in production. The optional
child adapter accepts only the crypto coordinator's sealed, job-correlated engine
results. SQL durability alone does not prove cryptographic provenance. A verified
outbox codec marker plus immutable payload/metadata/epoch binding restores authority
after restart. Actual network dispatch must use the opaque READY→SENDING
`DispatchReceipt`, never a caller boolean or arbitrary stored `Outgoing` bytes.

## Provisioning and remaining integration

The source reconciles durable local identity/prekeys with command 564 before
submitting/reinitializing (N166000–166108). A new local epoch must first be pending:
CSPRNG engine bundle → atomic private records/public submission outbox → correlated
540 ACK → activate that exact epoch. Reconnect/timeout cannot generate a fresh
identity or blindly re-upload. Mismatch/missing server state needs an explicit,
verified reconciliation transition; retain old epochs needed for queued messages.

Still required before live activation: coordinator-owned bounded WSS request routing,
registration/submission reconciliation, peer trust/epoch transitions, ordered session
queues, engine revision retries, atomic message/cursor/ratchet/outbox commits, receipt-
gated ACKs, exact ciphertext dispatch, unknown-delivery reconciliation and group
membership/capability validation. The current module does not claim these paths work
on a production account.

## Inactive provisioning proof checkpoint

The optional `signal-crypto` child keeps at most 16 correlated requests / 256 KiB,
with socket epoch, authentication generation, durable attempt ID and exact public
payload hash checks. Upload writes consume a durable READY→SENDING receipt and
recheck cancellation/socket authority immediately before sending. A successful 540
without a positive server timestamp remains `AcceptedNeedsMetadata`. Lost ACKs
become `Unknown`; neither state permits uploading the same prekeys again.

Query-only reconciliation requires 564 metadata, a complete own-541 response with
exactly one matching PC identity/signed prekey (other valid platforms may exist),
and a second unchanged, positive 564 timestamp. Registration-ID equality alone
never proves identity. Unknown uploads retain unknown OPK acceptance. Terminal
metadata races, stale scopes and malformed replies cannot mint a proof.

This child is **not connected to the live run loop**. Six focused tests include
strict local TLS/private Connection ACK, missing timestamp and lost-ACK cases.
Reconnection can retain the original attempt capability in memory; process restart
currently leaves recovered uploads held `Unknown`. A future opaque, query-only
restore capability and atomic proof-acceptance/activation transaction are still
required. No active epoch is installed by this checkpoint.

## Offline acceptance

```sh
node tests/fixtures/signal/oracle.cjs
node tests/fixtures/signal/receive_oracle.cjs
node --max-old-space-size=96 --no-warnings tests/fixtures/signal/business_oracle.cjs
cargo test --locked --offline --features messaging protocol::signal
```

The original request methods yield **13 requests and two SMS envelopes**. The original
offline handler holds a partial page then dispatches five ordered synthetic session
events. The business oracle executes original sender/encryption/receiver methods
with the bundled Signal SessionCipher: Unicode/escaping, separate nonempty styles,
and a full 65,536-byte text round-trip. Its Node ByteBuffer UTF-8 bridge and RNG are
fixture-only; keys/signatures in wire-shape fixtures are public synthetic bytes and
are explicitly **not** cryptographically verified production bundles.
