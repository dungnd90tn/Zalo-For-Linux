# QR Protocol Evidence

This is an independently authored implementation based on protocol facts in this
repository's local Electron bundles. No Rust implementation was copied. Tests
use synthetic accounts and local fixtures; no user account has been accessed.

## Verified local source facts

Line references below identify the committed Electron bundles, not live servers:

| Fact | Local evidence |
| --- | --- |
| Default auth origin `https://wpa.zaloapp.com`, API version `681`, type `30` | `pc-dist/compact-app-pc.e08d0d44f38873747a6b.js:268324`, `:268390`, `:268407` |
| QR creation is cookie-bearing GET `/api/login/reqqr` | Same bundle `:487560`, `:487619` |
| Plaintext fields: `language`, rounded Unix-second `client_time`, `imei`, `computer_name`, comma-separated `logged_uids`, `self_g`, `qr_form` | Same bundle `:487621`; `pc-dist/lazy/login-startup.cdc0ebe7d54dc91a9c23.js:1171` |
| Signature: lowercase MD5 of `zsecure` + command + parameter values ordered by key; `signkey` added last | Compact bundle `:487640` |
| Encrypted parameters: `zcid`, `zcid_ext`, `enc_ver=v2`, `params`; `type` and `client_version` added before signing | Compact bundle `:487250`, `:487502` |
| Parameter AES is AES-256-CBC, UTF-8 key, zero IV, default Node PKCS#7 padding | Compact bundle `:487306`; `main-dist/preload-render.js:2186`, `:2204` |
| QR reply supplies `base64_qr`, `chk_wait_scan`, `chk_wait_cfirm`, `qr_retry`, `qr_timeout` | Login startup bundle `:1274` |
| Polling GET uses the supplied URLs and timeout `qr_timeout * 1000` | Compact bundle `:489670` |
| Scan URL gets language appended; confirm URL can include device-link signatures | Login startup bundle `:1507`, `:845`, `:1546` |
| Poll codes: `0` success, `-20` second QR layer, `-21` relink failure, `408` timeout; `18010` is surfaced to the login UI | Login startup bundle `:1384` |
| Confirmation result uses `uid`, `dkey`, `profile.phonenum`, `zpw_sek`; trusted linking and refresh token handling follow | Login startup bundle `:1543`, `:1358` |

## Implemented contract

`protocol::qr::create_request` builds an encrypted legacy-format request with
`self_g=0`, `qr_form=0`, a fresh caller-supplied device identifier, and no remembered
accounts. `ResponseCipher` decodes encrypted response data. This is wire-format
functionality, not enabled UI authentication or a promise that the server accepts
the frozen version. An optional transport is available under `network`; it never
activates login automatically. Do not silently disable encryption after a server error.

`QrFlow` separates Requested, Scanned, AwaitingConfirmation and TrustRequired.
Only one poll may run at once; stale/cancelled responses cannot change state.
Each stage has bounded retry counts. Network failures preserve the current stage.
No implemented transition reaches Authenticated: confirmation still requires
verified device-link signatures, account binding and subsequent login bootstrap.
Never persist confirmation credentials or mark an account online before those
steps succeed.

Request specs require GET, a separate cookie jar for the attempt, disabled
redirects, a 2 MiB response cap and bounded timeouts. Poll URLs must use HTTPS,
port 443, and exactly `wpa.zaloapp.com` or `wpa.chat.zalo.me`, without credentials or
fragments. Unknown hosts fail closed. Adapters must revalidate each URL and
enforce these policies during streaming, before reading an entire response.
The scan request appends the validated locale without duplicating `lang` or
changing existing token values. Trusted-device confirmation query extensions
remain blocked on verified signatures.

`QrDriver<T: QrTransport>` runs create/decrypt/scan/confirm asynchronously with a
watch cancellation channel and per-request Tokio timeout. Its synthetic HTTP
fixtures cover full encrypted responses, cancellation of pending requests,
redirect rejection, oversized responses and trust-gated confirmation. There is
an optional original `HttpAdapter` under the `network` feature. It implements the
bounded transport policy and uses a fresh private cookie jar per login attempt;
default GUI/CLI paths never construct it. See [Authentication HTTP Adapter](PROTOCOL_HTTP.md)
for its exact limits and offline verification boundary. This does not establish
trusted-device authentication or install sessions.

## Independent cryptographic fixture

The known-answer vector was generated with Node's `node:crypto`, independently
of the Rust code. Input device string is `30,fixture-imei,1700000000123`, nonce
`abcdef123456`, and UTF-8 plaintext is
`{"fixture":"Vietnamese: Tiếng Việt","value":42}`.

1. AES-CBC encrypt the device string with ASCII key
   `3FC4F0D2AB50057BCE0D90D9187A22B1`, zero IV; encode uppercase hex as `zcid`.
2. Uppercase-hex MD5 of the nonce. Derive the ASCII parameter key from its even
   characters (first 8), `zcid` even characters (first 12), and reversed `zcid`
   odd characters (first 12). Indices are zero-based.
3. AES-CBC encrypt plaintext under that key and zero IV; encode base64.
4. Sign the parameter map including `type=30`, `client_version=681`.

Expected key: `9E2FEF1536FB6489D9B1FEE6480D0186`.
Expected signature: `a5124a84eb34d1757e00d7a6049c2aa1`.
The unit test also pins the full ciphertext and `zcid` produced by Node.

## Remaining live-authentication gates

Server config, cookie bootstrap, current endpoint/version acceptance, polling host
discovery, trust-device signatures/HMAC, account key derivation, session refresh
and the complete `getLoginInfo` contract still need independently verified
fixtures and integration. Electron `safeStorage` data is never an input. Live
authentication remains explicitly unavailable until these gates pass.

## Trusted-device implementation boundary

The local login bundle delegates identity generation, QR content, companion
proofs and signed-device-list verification to a worker, which calls the existing
`pc-dist/libs/trusted_device_protocol_bg.c6da592e2114c13d62a77d3e37aa722c.wasm`.
`pc-dist/lazy/default-login-startup-main-startup.3b948fcb90c856ff8381.js:103`
initializes that worker; `pc-dist/trust-worker.e08d0d44f38873747a6b.js:5457`
generates identity/linking keys, `:5479` creates companion proofs, and `:5510`
verifies signed device lists. The login bundle `:605` requires a companion proof;
`:630` verifies the final device list against primary and companion identity keys
before committing trusted-device state.

The binary is 350,023 bytes, has 66 `wbg` function imports and 60 exports, and has
no WASI interface. The optional `trust-wasm` feature now provides an original Rust
host using Wasmi 2.0.0's interpreter. It loads this existing binary lazily from an
explicit path, without copying it or embedding JavaScript. The default client
does not load the runtime. The [engine configuration](https://docs.rs/wasmi/2.0.0/wasmi/struct.Config.html)
and [store limits](https://docs.rs/wasmi/2.0.0/wasmi/struct.StoreLimitsBuilder.html)
define the fuel and memory controls used by the host.
The dependency keeps `std`, `validate` and `auto-dispatch`; the latter selects
portable dispatch for debug builds where LLVM tail calls are unavailable.
WAT parsing and memory64 support are excluded from this feature.

The public loader checks SHA-256
`3d53d066c7f4aaa1af3b01ec88c19e05ff7228584f965754ba65e78c1ddb606f`
before linking. This pins both protocol behavior and the wasm-bindgen import ABI.
Limits are 1 MiB input module, 16 MiB guest memory, one instance/memory/table,
1,024 table entries, 30 million fuel units per operation, 1 MiB materialized
buffers and 1,024 host handles. The host additionally has a conservative monotonic
2 MiB allocation budget. Property maps accept at most 64 primitive values and
128-byte keys; nested objects are rejected to prevent reference cycles. Whole
memory views remain views, with size checks before copying. Unsupported imports
trap with their name. They never return fabricated trust or signature results.
Any runtime trap permanently poisons the instance. Subsequent operations require
a fresh host; resetting fuel cannot resume partially mutated guest state. Cleanup
preserves the original error rather than replacing it with another trap.

The host is intentionally not a general JavaScript runtime. Implemented heap,
typed-array, property, exception and secure-RNG semantics follow the local worker
glue at `:5026–5207`. The host uses `Rc`/`RefCell` and must be created, used and
dropped inside a dedicated blocking worker. Pass bounded owned results across
the application boundary; do not run cryptography on the UI thread.

Real-binary tests exercise identity generation, linking-secret generation,
trusted QR-content generation with a synthetic token, companion-proof rejection
of an invalid MAC, and malformed signed-device-list rejection. Despite the
export name `zprotoEd25519GenerateKeyPair`, the observed identity is a 33-byte
`0x05`-tagged X25519 public key and 32-byte private key. A separate OpenSSL process
derives the same public key using X25519 PKCS#8/SPKI (OID `1.3.101.110`). Ephemeral
private bytes are passed over stdin, never printed or written to disk. OpenSSL
is required for this optional feature test.

Additional raw-WASM fixtures prove actual fuel exhaustion, memory-growth denial
and unsupported-import traps; host-budget, cycle and pinned-hash tests cover the
host boundary. Run `cargo test --features trust-wasm protocol::trust`.

An original Python signer now generates independently verified positive empty
and nonempty signed-device-list fixtures, including a selected PC companion's
D2 signature. Negative variants cover wrong identities, modified signatures,
modified master context and expiration. Positive tests also corrected the
returned-object getter/destructor ABI and prove clean instance reuse. See
[TRUST_FIXTURES.md](TRUST_FIXTURES.md) for exact scope, wire evidence and the
independent RFC/OpenSSL cross-checks.

Positive primary-linking HMAC and A1 account-signature fixtures now reach genuine
companion-proof generation. A separate Python Edwards verifier checks its D2
signature, while OpenSSL cross-checks HMAC. At this WASM boundary
`primary_linking_data` is decoded protobuf, not ciphertext; request/response
transport encryption is separate and callers must not decrypt the protobuf again.
The JS PC platform value 2 maps to protobuf PC value 1. See
[TRUST_FIXTURES.md](TRUST_FIXTURES.md) for exact domains and tamper tests.

A master identifier is not yet proven to equal an authenticated account UID;
the guest verifier also accepts replay inside its TTL. Complete attempt/account
context, persisted replay policy and bootstrap integration remain prerequisites
for accepting authentication. Public inspection still explicitly rejects
activation. No runtime call installs a session, marks authentication usable or
accesses a real account. Live authentication stays disabled. The application
direction remains native Rust using the existing Zalo theme, icons and layout.

A safe future capture uses a disposable account with a manually initiated QR
flow and a local recorder writing only to a private, ignored directory. Keep
cookies, session tokens and private key material local; never paste them into
chat or commit them. Signed fields cannot simply be anonymized without breaking
verification. Publishable fixtures therefore require synthetic primary-side
signing fixtures or vendor test vectors without personal data. No capture or
real-account operation has been performed by this implementation.
