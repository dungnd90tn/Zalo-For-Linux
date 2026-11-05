# Authentication and session contract

This document records bundle evidence and the implemented opt-in native coordinator.
Master ID and account UID remain distinct. `--features live -- --live` activates
user-initiated login; default/demo paths make no authentication requests.

## Verified bundle dataflow

- The confirmation handler reads `uid` and `dkey`, then passes the same response
  through `gr`. That function forwards `devices` and `uid` together to trust
  completion ([L:1555], [L:1563], [L:1361]). Relink rejects a confirmation UID
  different from the previously stored UID; fresh linking has no prior UID
  ([L:915], [L:756]).
- Device-list verification uses the proof's primary identity and current companion
  identity. Storage receives confirmation UID and proof master ID as separate
  arguments ([L:630], [L:638]). It records both fields and atomically writes the
  trusted identity plus link history ([T:1324], [T:1634]). History lookup uses
  master ID; trusted-identity lookup uses UID, joined through history UID and
  matching identity keys ([T:1494], [T:1537], [L:730]). This is a persisted
  association, not evidence that the identifiers are equal.
- QR generation receives the server token and a current-time value. Fresh state
  generates identity/secret; relink may reuse stored identity/secret and derives
  a relink ID from the earlier linking timestamp ([L:1470], [L:865], [L:596],
  [L:743], [L:756]). Proof creation receives identity, secret, decoded linking
  data, HMAC and platform, with no token, attempt identifier or current-clock
  argument ([L:607]). Its outputs are appended to that attempt's confirmation
  URL, polled with server timeout/retry values ([L:845], [L:1571]).
- `getLoginInfo` sends device/language/time parameters, without UID or master ID;
  subsequent application authentication uses returned `config.uid`
  ([N:398523], [N:384907]).

## Implemented coordinator contract

1. Give each attempt an immutable generation, isolated transport/cookies and
   bounded deadline. Cancellation or regeneration invalidates every outstanding
   callback, proof and pending cookie/session plan before a new attempt proceeds.
2. Bind scan/proof/confirmation results to the same attempt and companion keys.
   Validate proof/list master context separately from account UID. Reusing a
   linking secret or passing the guest's device-list TTL does not prevent replay.
   Define and persist an accepted-timestamp/replay policy before activation.
3. Require authenticated bootstrap `config.uid` to match confirmation UID and,
   for relink, the existing profile owner. Reject mismatches before profile
   binding, credential persistence, cache loading or message processing.
4. Commit validated account/trust/session context atomically under the profile
   lock; publish usable state only after durable success. Failure or cancellation
   preserves the previous session before commit linearization; later cancellation
   durably invalidates that accepted attempt. History and immutable ownership persist.

`auth.rs` now implements these contracts with a 300-second overall deadline,
16-event bounded queue, cancellation generations and reliable QR/terminal delivery.
Preparation writes/fsyncs a private temporary record before the generation CAS.
That CAS linearizes commit: cancellation before it preserves the old record;
cancellation after it invalidates the accepted attempt through a durable tombstone.
Repeated cancellation retains the target attempt identity. UI calls perform only
bounded channel/atomic operations; filesystem work runs on the auth worker.

## Client identity and saved-session migration

The Linux launcher returns client type 24 ([main.js:109415](../main-dist/main.js#L109415)). Both login and main pages receive it through `Wt` ([110055](../main-dist/main.js#L110055), [110085](../main-dist/main.js#L110085), [110223](../main-dist/main.js#L110223)); renderer startup overrides the fallback API type from that URL ([N:228138](../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js#L228138)). The parameter-cipher device string and signed `type` query use this same runtime setting ([N:397986](../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js#L397986), [N:398235](../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js#L398235)). New native attempts therefore carry the closed typed capability `PcLinux24` through QR, bootstrap, persistence and messaging; the HTTP user agent is `ZaloPC-linux-24v681`.

New private auth records are schema version 2 and include the numeric `client_type`. Existing version-1 records without it decode as `Legacy30`, preserving their actual historical protocol identity. Their resume fails with `FreshApprovalRequired` **before opening the HTTP transport**. No old cookie is silently retyped or given a guessed legacy user agent/origin. The UI asks for another QR scan and explicitly retains saved history. Reading/refusing that session leaves its bytes, owner, replay watermarks and message database unchanged. Fresh approval may replace credentials only after the existing UID, trust and atomic-commit checks. The separate immutable owner record remains version 1. Unknown record versions or client types fail closed.

Actual `LoginController` loopback-TLS tests cover new type-24 QR creation through commit and fresh encrypted resume. An independent Node crypto fixture checks the query type, decrypted `zcid` prefix, request device identity and signature, then encrypts the responses. Historical type-30 known-answer vectors remain separate tests rather than a live fallback.

## Explicit assurance and verification

Only explicit server flags `self_g=0, qr_format_trust_device=0` select
`PhoneApproved`; both `1` select `TrustedDevice`. Missing, mixed or conflicting
flags fail closed. Classic QR uses bounded server PNG pixels and the original
confirmation URL, without invented scan UID or trust proofs. The local bundle
explicitly permits old QR without device linking ([L:854], [L:1354]). Nonempty
primary-link requirements in this mode fail; trusted-proof failures never fall
back to classic mode.

The private record uses a tagged `SessionProof`: `PhoneApproved` carries no trust
payload, while `TrustedDevice` requires actual keys, signed list and timestamps.
Both require confirmation UID to match freshly decrypted bootstrap `config.uid`
and the immutable profile owner before a durable Ready capability is published.
Trusted mode also verifies HMAC/A1/D2/L3, selected companion IDs/keys, timestamp
freshness and a persisted per-master watermark. Resume preserves assurance, checks
stored trust context when applicable and performs fresh UID-matching bootstrap.
Logout retains owner, replay history and message databases. Refresh tokens are
retained privately; renewal exchange is not implemented.

Synthetic actual-TLS tests cover the full classic create/scan/confirm/bootstrap/
commit path, resume, UID mismatch and no downgrade; separate real-WASM fixtures
continue through signed devices, bootstrap and commit. A bounded public probe
verified actual controller Begin → QR → immediate Cancel and lock release for
API version 681, including one source-correct `PcLinux24` run after the client-type
migration. It asserted no auth record or owner marker was persisted. It never
scanned, approved, logged in or sent messages. Real
phone approval, authenticated bootstrap and production messaging still require
manual acceptance. Public polling is separately limited to the exact observed
`qr-id.zaloapp.com` plus the two auth hosts; the poll-only `qr-id.zaloapp.com` origin receives no cookies.


[L:596]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L596
[L:607]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L607
[L:630]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L630
[L:638]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L638
[L:730]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L730
[L:743]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L743
[L:756]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L756
[L:845]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L845
[L:865]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L865
[L:915]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L915
[L:1361]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L1361
[L:1470]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L1470
[L:1555]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L1555
[L:1563]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L1563
[L:1571]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L1571
[T:1324]: ../pc-dist/lazy/default-login-startup-main-startup.3b948fcb90c856ff8381.js#L1324
[T:1494]: ../pc-dist/lazy/default-login-startup-main-startup.3b948fcb90c856ff8381.js#L1494
[T:1537]: ../pc-dist/lazy/default-login-startup-main-startup.3b948fcb90c856ff8381.js#L1537
[T:1634]: ../pc-dist/lazy/default-login-startup-main-startup.3b948fcb90c856ff8381.js#L1634
[N:398523]: ../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js#L398523
[N:384907]: ../pc-dist/lazy/default-login-main-startup-shared-worker-znotification.9e3e92e88644da772301.js#L384907

[L:854]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L854
[L:1354]: ../pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js#L1354

Post-confirmation failures carry sanitized bootstrap stage/category and explicit
numeric server codes. Unused service-map and legacy polling fields neither reject
valid authentication nor authorize cookie destinations; only consumed chat/group
origins and validated WSS routes enter the runtime capability.
