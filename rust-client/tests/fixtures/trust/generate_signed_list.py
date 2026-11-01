#!/usr/bin/env python3
"""Original fixture-only signer; Python standard library + OpenSSL, no WASM.

The fixed scalar is public test data. This deliberately slow big-integer code is
not constant-time and MUST NOT sign real account/device material. It constructs
valid Schnorr signatures independently of the production verifier. The Edwards
arithmetic is checked against RFC 8032 section 7.1 test 1, while OpenSSL derives
the X25519 public key independently. https://www.rfc-editor.org/rfc/rfc8032#section-7.1

Run from any directory; stdout is deterministic JSON suitable for comparison
with signed_primary_list.json. No private profile or production service is read.
"""
import hashlib
import json
import struct
import subprocess

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
BASE_Y = 4 * pow(5, P - 2, P) % P
BASE_X_SQUARED = (BASE_Y**2 - 1) * pow(D * BASE_Y**2 + 1, P - 2, P) % P
BASE_X = pow(BASE_X_SQUARED, (P + 3) // 8, P)
if BASE_X**2 % P != BASE_X_SQUARED:
    BASE_X = BASE_X * pow(2, (P - 1) // 4, P) % P
if BASE_X & 1:
    BASE_X = P - BASE_X
BASE = (BASE_X, BASE_Y)


def add(left, right):
    x, y = left
    xx, yy = right
    product = D * x * xx * y * yy % P
    return ((x * yy + xx * y) * pow(1 + product, P - 2, P) % P,
            (y * yy + x * xx) * pow(1 - product, P - 2, P) % P)


def multiply(scalar, point=BASE):
    result = (0, 1)
    while scalar:
        if scalar & 1:
            result = add(result, point)
        point = add(point, point)
        scalar >>= 1
    return result


def encode(point):
    x, y = point
    return (y | ((x & 1) << 255)).to_bytes(32, 'little')


def clamp(raw):
    return (int.from_bytes(raw, 'little') & ((1 << 254) - 8)) | (1 << 254)


def hash_scalar(data):
    return int.from_bytes(hashlib.sha512(data).digest(), 'little') % L


def arithmetic_self_test():
    seed = bytes.fromhex('9d61b19deffd5a60ba844af492ec2cc44'
                         '449c5697b326919703bac031cae7f60')
    expanded = hashlib.sha512(seed).digest()
    scalar = clamp(expanded[:32])
    public = encode(multiply(scalar))
    assert public.hex() == ('d75a980182b10ab7d54bfed3c964073a'
                            '0ee172f3daa62325af021a68f707511a')
    nonce = hash_scalar(expanded[32:])
    r_encoded = encode(multiply(nonce))
    challenge = hash_scalar(r_encoded + public)
    signature = r_encoded + ((nonce + challenge * scalar) % L).to_bytes(32, 'little')
    assert signature.hex() == ('e5564300c360ac729086e2cc806e828a'
                               '84877f1eb8e5d974d873e06522490155'
                               '5fb8821590a33bacc61e39701cf9b46b'
                               'd25bf5f0595bbe24655141438e7a100b')


def sign_fixture(scalar, message):
    # Signal's legacy tagged Curve25519 signature carries the Edwards public
    # sign bit in S's unused high bit. The nonce is deterministic TEST DATA,
    # deliberately distinct from Signal's randomized production nonce scheme.
    public = encode(multiply(scalar))
    nonce = hash_scalar(b'zalo-rust-fixture-only-v1' + scalar.to_bytes(32, 'little') + message)
    r_encoded = encode(multiply(nonce))
    challenge = hash_scalar(r_encoded + public + message)
    signature = bytearray(r_encoded + ((nonce + challenge * scalar) % L).to_bytes(32, 'little'))
    signature[63] |= public[31] & 128
    return bytes(signature)


def varint(value):
    encoded = bytearray()
    while value > 127:
        encoded.append((value & 127) | 128)
        value >>= 7
    return bytes(encoded) + bytes([value])


def byte_field(tag, value):
    return varint(tag * 8 + 2) + varint(len(value)) + value


def int_field(tag, value):
    return varint(tag * 8) + varint(value)


def public_for(secret):
    pkcs8 = bytes.fromhex('302e020100300506032b656e04220420') + secret
    result = subprocess.run(['openssl', 'pkey', '-inform', 'DER', '-pubout', '-outform', 'DER'],
                            input=pkcs8, capture_output=True, check=True)
    prefix = bytes.fromhex('302a300506032b656e032100')
    assert result.stdout.startswith(prefix) and len(result.stdout) == len(prefix) + 32
    raw_public = result.stdout[len(prefix):]
    _, edwards_y = multiply(clamp(secret))
    assert int.from_bytes(raw_public, 'little') == (1 + edwards_y) * pow(1 - edwards_y, P - 2, P) % P
    return b'\x05' + raw_public


def generate():
    arithmetic_self_test()
    scalar = clamp(bytes(range(32)))
    companion_scalar = clamp(bytes(range(32, 64)))
    public = public_for(bytes(range(32)))
    companion = public_for(bytes(range(32, 64)))
    master = b'fixture-master-10001'
    timestamp = 1700000000000
    # Original protobuf encoding from observed pinned-module wire facts.
    # This fixture uses a PC companion (protobuf platform=1, JS API PC=2), not a real device/account.
    account_metadata = (byte_field(1, master) + byte_field(2, public) + byte_field(3, companion) +
                        int_field(4, 123) + int_field(5, 42))
    metadata = account_metadata + int_field(6, 1)
    device_message = b'D2' + metadata + companion + public
    device_signature = sign_fixture(companion_scalar, device_message)
    account_message = b'A1' + account_metadata + companion
    account_signature = sign_fixture(scalar, account_message)

    def device_list(device_sig, expiration, status=1):
        # Same independently generated A1 account signature as the linking
        # fixture. This list verifier authenticates its inclusion via L3;
        # the separate linking fixture exercises its own signature check.
        record = (int_field(1, 123) + byte_field(2, companion) + int_field(3, 42) +
                  int_field(4, 1) + (int_field(5, status) if status else b'') +
                  byte_field(6, account_signature) + byte_field(7, device_sig) +
                  int_field(8, expiration))
        wrapper = byte_field(1, record)
        message = b'L3' + master + public + struct.pack('<Q', timestamp) + wrapper
        signature = sign_fixture(scalar, message)
        encoded = (byte_field(1, master) + byte_field(2, public) + byte_field(3, wrapper) +
                   b'\x21' + struct.pack('<Q', timestamp) + byte_field(5, signature))
        return encoded, message

    valid, message = device_list(device_signature, timestamp + 86400000)
    tampered_device_signature = bytes([device_signature[0] ^ 1]) + device_signature[1:]
    invalid_device, _ = device_list(tampered_device_signature, timestamp + 86400000)
    expired_companion, _ = device_list(device_signature, timestamp - 1, status=0)
    empty_message = b'L3' + master + public + struct.pack('<Q', timestamp)
    empty = (byte_field(1, master) + byte_field(2, public) + byte_field(3, b'') +
             b'\x21' + struct.pack('<Q', timestamp) + byte_field(5, sign_fixture(scalar, empty_message)))
    return {
        'fixture_version': 1, 'master_id': master.decode(), 'timestamp_ms': timestamp,
        'primary_public_hex': public.hex(), 'companion_public_hex': companion.hex(),
        'signature_message_hex': message.hex(), 'device_signature_message_hex': device_message.hex(),
        'account_signature_hex': account_signature.hex(), 'account_signature_message_hex': account_message.hex(),
        'device_list_hex': valid.hex(), 'empty_device_list_hex': empty.hex(),
        'invalid_device_signature_resigned_list_hex': invalid_device.hex(),
        'expired_stale_companion_resigned_list_hex': expired_companion.hex(),
        'scope': 'L3 primary signature and D2 selected PC companion signature; account signature is exercised separately; no HMAC or account UID claim.'}


if __name__ == '__main__':
    print(json.dumps(generate(), indent=2, sort_keys=True))
