# Authentication and session contract

This document records the extracted bundle's behavior and proposed requirements
for a future Rust coordinator. It does not establish current server compatibility
or enable authentication. Master ID and account UID must remain distinct.

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

## Proposed coordinator requirements

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
   must preserve the previous session/history without partially installed secrets.

These are next-stage contracts, not implemented authentication capabilities.
Independent fixtures currently prove HMAC/A1/D2/L3 cryptographic behavior; they do
not provide the complete attempt, account, replay or session-commit guarantees.

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
