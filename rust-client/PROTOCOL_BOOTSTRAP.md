# Bootstrap Protocol Evidence

The implementation in `src/protocol/bootstrap.rs` was written independently from facts in this repository's local JavaScript bundles. No external Rust implementation was copied. Fixtures are synthetic; they establish wire-format behavior, not acceptance by current Zalo servers. Live authentication and sending remain disabled.

## Local evidence

All compact-bundle references identify `pc-dist/compact-app-pc.e08d0d44f38873747a6b.js`:

| Contract | Local source |
| --- | --- |
| API version `681`, desktop type `30`, session secret field `zpw_enk` | Compact bundle `:268407` |
| `GET /api/login/getServerInfo`: `imei`, `type`, `client_version`, `computer_name`, `sp_mtn`, `bkt`; optional UI/viewer fields | Compact bundle `:487744` |
| Sign command `getserverinfo`; defaults for maintenance and bucket are `-1` | Compact bundle `:487744` |
| `GET /api/login/getLoginInfo`: ParamCipher plaintext contains `imei`, `computer_name`, `language`, millisecond `ts`; sign command `getlogininfo` | Compact bundle `:487783` |
| Optional IP/screen/system/install/identity/new-registration fields exist but are not mandatory | Compact bundle `:487790` |
| Default request timeout is 10 seconds; GET includes credentials | Compact bundle `:487394`, `:487560`, `:487587` |
| ParamCipher response decrypts a complete inner JSON envelope, not only inner `data` | Compact bundle `:487520` |
| `getLoginInfo` returns `dkey` as legacy `UIN` and `zpw_enk` as `decryptKey` | Compact bundle `:381061` |
| Pre-login encryption setting uses actual spelling `setttings.authen.encryption.enable`; post-login uses `settings` | Compact bundle `:428628`, `:471795` |
| Service-map V3 values are arrays; polling and WebSocket endpoint lists are separate | Compact bundle `:585143`, `:487612`, `:489644` |
| Session AES decodes `zpw_enk` from base64, CBC zero IV, PKCS#7 padding | Compact bundle `:687255`, `:687287` |
| `ERR_KICK_OUT=600`, `ERR_SESSION_TIMEOUT=102`; getLoginInfo explicitly handles `600` or positive `102` | Compact bundle `:56860`, `:57746`, `:471902` |
| Confirmation installs `zpw_sek` and completes trust; `zpw_kt` is separately persisted as a refresh token | `pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js:1373`, `:1571` |
| Cookie name `zpw_sek`, secure, HttpOnly, scopes `zaloapp.com`, `zalo.me`, `chat.zalo.me` | `main-dist/main.js:13715` |

`dkey` is deliberately treated as opaque storage/bootstrap material. The bundle's legacy `UIN` variable name does not establish that `dkey` is a numeric account ID. The confirmed `uid` remains a separate, still unverified account claim.

## Implemented scope

- Minimal signed server-info and encrypted login-info request specifications reuse the independently authored QR module's parameter cipher and signer. No identity-submission extras are invented.
- Bounded outer and decrypted inner envelope parsing distinguishes HTTP failures, malformed responses, service errors, encryption incompatibility and locally verified authentication rejection codes. HTTP failures, parse errors, unknown codes and negative `-102` never expire credentials. `19000` does not trigger plaintext fallback.
- Configuration parsing validates service lists, separates chat/control polling from WebSockets, limits counts and URL lengths, and accepts only secure Zalo origins without userinfo, arbitrary ports, paths, queries or fragments. Unknown schemas are refused; no observed host is silently replaced by a hardcoded alternative.
- Session response decryption supports standard AES key lengths of 16, 24 and 32 bytes after base64 decoding. Independent Node crypto fixtures cover each key length and Unicode plaintext; the encrypted login-info fixture pins the nested envelope structure.
- `PendingCookies` represents a staging plan only. It validates confirmation fields and cookie bytes, scopes a header to secure Zalo hosts, and keeps the refresh token separate. It does not install a cookie jar, persist a session, bind an account, or mark authentication successful. Secret-bearing structs have no `Debug`/`Serialize` implementation.

## Remaining gates

An optional concrete HTTP adapter is available behind the `network` feature; default GUI/CLI paths never activate it. It owns an isolated attempt-specific cookie jar and enforces streaming limits, redirect refusal, sanitized errors and deadlines/cancellation. See [PROTOCOL_HTTP.md](PROTOCOL_HTTP.md) for resource policies and the offline fixture boundary. Real TLS-server and live endpoint integration remain unverified.

Positive trusted-device linking/HMAC and signed-device-list fixtures, account/profile identity verification, refresh-token behavior, current endpoint/version acceptance and an atomic trusted-session/profile commit are still required. The session AES primitive is a compatibility implementation of observed protocol facts, not a recommendation to reuse this cipher format elsewhere. Electron credentials and `safeStorage` are never read.
