# Independent trust fixtures

This stage tests the existing, hash-pinned trust WASM through an original Rust
host. It adds no authenticated session, production signer, network request or
account access. Fixtures contain deterministic public test scalars and synthetic
identifiers. No upstream Rust source or external binary was copied.

## Independent construction

`tests/fixtures/trust/generate_signed_list.py` constructs protobuf bytes and
Curve25519-compatible Schnorr signatures using original Python integer
arithmetic. Its Edwards arithmetic reproduces [RFC 8032 test 1](https://www.rfc-editor.org/rfc/rfc8032#section-7.1);
OpenSSL independently derives both X25519 public keys from fixed test scalars.
It never calls the WASM to generate expected signatures. The nonce recipe is
fixture-only, variable-time and unsuitable for production signing.

The checked-in JSON is regenerated and compared in a Rust test. Run:

```sh
python3 tests/fixtures/trust/generate_signed_list.py
cargo test --locked --offline --features trust-wasm protocol::trust
```

## Observed pinned-module contract

`zprotoTrustVerifyDeviceListFromBytes` returns **ZprotoTrustDeviceList**. The local
worker's wrapper `D` returns class `K`, whose getters and destructor use
`zprototrustdevicelist_*`; the `devicelistbytes` type has a different layout.
Positive tests exposed and corrected the previous wrong getter/destructor ABI.

The decoded outer protobuf fields are master identifier (tag 1), tagged primary
key (2), a present nested companion-data wrapper (3), fixed64 millisecond
timestamp (4), and signature (5). The signed bytes are `L3`, UTF-8 master ID,
33-byte `0x05`-tagged primary key, timestamp in little-endian order, then encoded
companion records. The fixture includes one synthetic PC companion. Its device
signature covers `D2`, encoded linking metadata, tagged companion key, then tagged
primary key. These facts were checked against the pinned module's decoder,
serializer and verifier; they are not a claim about current server compatibility.

Tests exercise an empty list and nonempty selected companion, clean object
release/reuse, a modified master identifier, modified outer signature, wrong
primary key, missing selected companion, expired list, expired stale companion,
and a changed D2 signature inside an independently re-signed valid outer list.
The last case passes primary-only verification and fails selected-companion
verification, isolating the signature check under test.

## Primary-linking HMAC and account signature

`generate_linking_proof.py` independently constructs a valid synthetic
`LinkingDataProto`. At the trust-WASM boundary this is **decoded protobuf**, not
ciphertext: tag 1 contains linking metadata, tag 2 the account signature, and tag
3 a fixed64 timestamp. Request/response encryption belongs to the transport
layer; callers must not decrypt this protobuf a second time.

The pinned module validates `HMAC-SHA256(linkingSecret32,
rawLinkingData || taggedPrimaryPublicKey33)`. The generator checks HMAC against
[RFC 4231 case 1](https://www.rfc-editor.org/rfc/rfc4231#section-4.2) and
separately against OpenSSL over the exact fixture bytes.
The account signature covers `A1 || canonicalMetadataWithoutPlatform ||
taggedCompanionPublicKey`. The companion adds its local platform afterward.
JS API PC has value **2**; protobuf PC has value **1**. These enum domains must
not be interchanged. The signed-list and primary-linking fixtures share the
same independently generated account signature.

A positive real-WASM operation returns the expected master identifier, primary
key, account signature and PC metadata. An independent Python Edwards verifier
then checks the generated companion D2 signature; a modified D2 signature fails
that checker. Additional tests reject modified wire data, MAC or linking secret,
an invalid account signature inside data carrying a valid recomputed HMAC, and
a different current companion public key. Domain failures leave the host usable
for a following genuine positive operation. No expected signature is generated
by the WASM under test.

The production wrapper names these inputs `primary_linking_data` and
`linking_hmac` and documents their decoded boundary. It returns cryptographic
proof material only; it cannot install or activate a session.

## Remaining authentication gates

A master identifier is not proven to equal the authenticated numeric account UID.
The guest list verifier is stateless and accepts repeated valid lists inside its
21-day TTL. Creating a linking proof also has no application clock or attempt
context parameter. The opt-in coordinator adds authenticated confirmation/bootstrap UID matching,
attempt freshness, a persisted watermark and atomic session commit; see
[AUTH_SESSION_CONTRACT.md](AUTH_SESSION_CONTRACT.md). The fixture host alone does
not establish those protocol claims. Public inspection always returns an explicit
trust-not-established error, even for a valid signature.

These offline fixtures prove the pinned bundle's cryptographic contracts; they
do not verify current production endpoints or a complete live login. No transport
encryption was invented for the primary-linking protobuf. The final application
direction remains native Rust with the existing Zalo theme, icons and layout.
Live login is explicitly opt-in and preserves PhoneApproved versus TrustedDevice
assurance. Actual public startup reached classic QR creation/cancellation only;
no real phone approval/account login or production message exchange was tested.
