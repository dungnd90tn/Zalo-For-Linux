# Authentication HTTP Adapter

`protocol::http::HttpAdapter` is an independently written, optional concrete
`reqwest`/Rustls transport for QR and bootstrap `RequestSpec`s. Enable it with
`--features network`. Default GUI/CLI paths do not construct it or contact Zalo;
this feature does not enable authentication, sending or session persistence.

Each adapter instance represents one fresh login attempt. Its cookie store and
client are private and are not cloned, exported or populated from Electron or a
`PendingCookies` plan. Create a **new adapter** for each new attempt: resetting a
`QrDriver` generation does not reset its transport's cookie jar. Server cookies
remain transient attempt state, never proof of authentication.

The transport accepts GET requests only to HTTPS `wpa.zaloapp.com` or
`wpa.chat.zalo.me`, port 443, with no userinfo or fragments. URLs are capped at
8192 bytes. It disables redirects, referrer forwarding, environment proxies and
HTTP/2. An exclusive mutable borrow permits one logical request at a time.

Resource policies are enforced before body processing:

- Request-specific deadlines must be 1–60,000 ms; connect timeout is at most
  10 seconds. An outer Tokio deadline includes headers and the entire streamed
  body. Dropping the request future cancels its work; no detached fetch is spawned.
- Accepted response headers are capped at 128 entries and 32 KiB including names
  and values. This is a **postparse policy**, not a promise that TLS/parser
  allocations fit 32 KiB. The locked HTTP/1 parser has its own transient buffer
  limits; those belong to the dependency implementation.
- Known oversized content lengths are rejected immediately. Unknown/chunked
  bodies are checked cumulatively before append, with a maximum 2 MiB payload.
  Buffer capacity grows geometrically up to that application budget. Periodic
  cooperative yields and deadline checks also bound always-ready empty streams.
- Attempt cookie intake is capped at 128 Set-Cookie headers, 64 KiB cumulative
  header bytes and 4096 bytes per header. Replacements consume this monotonic
  budget too. Exhaustion fails the attempt explicitly; the caller must start a
  fresh adapter.

Only HTTP 200 proceeds to protocol parsing. Redirects and other statuses are
sanitized transport failures, never automatic credential expiration. Public
errors retain no `reqwest::Error` source, request URL, cookies or response body.

## Verification boundary

Offline tests exercise the same bounded fetch engine through deterministic fake
response streams: declared/unknown lengths, chunk overflow, tiny chunks,
header/status rejection, stalled and always-ready deadlines, dropped futures,
URL policy, cookie isolation/budgets and error-chain redaction. The concrete
reqwest client constructs successfully and rejects forbidden requests before
DNS/connect. No test contacts production servers or accesses a real account.

Seven additional integration tests exercise the actual reqwest backend against
an ephemeral localhost Rustls server. OpenSSL generates a temporary synthetic CA
and a leaf certificate for the allowed authentication hostname. Only test code
installs that CA and resolves both allowed hosts to the loopback listener;
production constructors expose neither override. The tests verify successful
TLS/SNI and chunked streaming, body/header limits, status and redirect refusal,
actual Cookie transmission and attempt isolation, deadline and aborted-future
socket closure, untrusted CA rejection, hostname mismatch and sanitized errors.
They require OpenSSL and permission to bind loopback sockets:

```sh
cargo test --locked --offline --features network protocol::http::tls_tests
```

The certificate generation invokes no network service. Fixture keys are synthetic,
created in a private temporary directory and removed after loading. The tests do
not verify Zalo endpoint compatibility, production TLS or a real account flow.
Trusted-device positive HMAC/signed-list fixtures, account validation and an
atomic trusted-session commit remain separate gates before UI login activation.
