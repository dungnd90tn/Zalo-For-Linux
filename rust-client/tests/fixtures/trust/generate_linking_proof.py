#!/usr/bin/env python3
"""Original synthetic primary-linking fixture; no WASM, network or account.

Uses the independently checked fixture-only Edwards signer from the adjacent
module. All scalars and linking secret below are fixed PUBLIC TEST DATA.
"""
import hashlib
import hmac
import json
import struct
import subprocess
import sys
sys.dont_write_bytecode = True

from generate_signed_list import (
    BASE, D, L, P, add, arithmetic_self_test, byte_field, clamp, encode,
    hash_scalar, int_field, multiply, public_for, sign_fixture,
)


def mac_for(secret, data, primary):
    return hmac.new(secret, data + primary, hashlib.sha256).digest()


def point_from_encoding(encoded):
    if len(encoded) != 32:
        raise ValueError('invalid point length')
    value = int.from_bytes(encoded, 'little')
    sign = value >> 255
    y = value & ((1 << 255) - 1)
    if y >= P:
        raise ValueError('noncanonical point')
    x_squared = (y*y - 1) * pow(D*y*y + 1, P-2, P) % P
    x = pow(x_squared, (P+3)//8, P)
    if x*x % P != x_squared:
        x = x * pow(2, (P-1)//4, P) % P
    if x*x % P != x_squared:
        raise ValueError('point is not on Edwards curve')
    if x & 1 != sign:
        x = P - x
    point = (x, y)
    if encode(point) != encoded:
        raise ValueError('noncanonical sign')
    return point


def verify_signature(public, message, signature):
    if len(public) != 33 or public[0] != 5 or len(signature) != 64:
        return False
    u = int.from_bytes(public[1:], 'little')
    if u >= P or u == P-1:
        return False
    y = (u-1) * pow(u+1, P-2, P) % P
    sign = signature[63] >> 7
    public_edwards = (y | sign << 255).to_bytes(32, 'little')
    scalar_bytes = bytearray(signature[32:])
    scalar_bytes[31] &= 127
    scalar = int.from_bytes(scalar_bytes, 'little')
    if scalar >= L:
        return False
    try:
        public_point = point_from_encoding(public_edwards)
        r_point = point_from_encoding(signature[:32])
    except ValueError:
        return False
    challenge = hash_scalar(signature[:32] + public_edwards + message)
    return encode(multiply(scalar, BASE)) == encode(add(r_point, multiply(challenge, public_point)))


def generate():
    arithmetic_self_test()
    # RFC 4231 case 1 checks the independent HMAC implementation before use.
    assert hmac.new(bytes([0x0b])*20, b'Hi There', hashlib.sha256).hexdigest() == (
        'b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7')
    primary = public_for(bytes(range(32)))
    companion_private = bytes(range(32, 64))
    companion = public_for(companion_private)
    secret = bytes(range(64, 96))
    timestamp = 1700000000000
    master = b'fixture-master-10001'
    # Account-signature canonicalization excludes the platform; the companion
    # later adds its local platform (API PC=2 -> protobuf PC=1).
    metadata = (byte_field(1, master) + byte_field(2, primary) + byte_field(3, companion) +
                int_field(4, 123) + int_field(5, 42))
    account_message = b'A1' + metadata + companion
    account_signature = sign_fixture(clamp(bytes(range(32))), account_message)
    assert verify_signature(primary, account_message, account_signature)

    def envelope(signature):
        return byte_field(1, metadata) + byte_field(2, signature) + b'\x19' + struct.pack('<Q', timestamp)

    data = envelope(account_signature)
    mac = mac_for(secret, data, primary)
    # OpenSSL independently cross-checks HMAC over exactly these wire bytes.
    checked = subprocess.run(['openssl', 'dgst', '-sha256', '-mac', 'HMAC',
                              '-macopt', 'hexkey:' + secret.hex(), '-binary'],
                             input=data + primary, capture_output=True, check=True)
    assert checked.stdout == mac
    bad_account_signature = bytes([account_signature[0] ^ 1]) + account_signature[1:]
    invalid_account_data = envelope(bad_account_signature)
    return {
        'fixture_version': 1, 'master_id': master.decode(), 'timestamp_ms': timestamp,
        'primary_public_hex': primary.hex(), 'companion_public_hex': companion.hex(),
        'companion_fixture_private_hex': companion_private.hex(),
        'fixture_linking_secret_hex': secret.hex(),
        'primary_linking_data_hex': data.hex(), 'linking_hmac_hex': mac.hex(),
        'account_signature_hex': account_signature.hex(),
        'account_signature_message_hex': account_message.hex(),
        'expected_pc_metadata_hex': (metadata + int_field(6, 1)).hex(),
        'invalid_account_data_hex': invalid_account_data.hex(),
        'invalid_account_valid_hmac_hex': mac_for(secret, invalid_account_data, primary).hex(),
        'scope': 'Valid synthetic A1 account signature and HMAC at decoded trust-WASM boundary; no server encryption, UID binding or replay protection.'}


if __name__ == '__main__':
    if sys.argv[1:] == ['--verify-signature']:
        # Only the test harness calls this bounded independent checker with
        # synthetic public bytes; this is not an application verifier API.
        request = json.loads(sys.stdin.buffer.read(8193))
        assert len(json.dumps(request)) <= 8192
        accepted = verify_signature(bytes.fromhex(request['public_hex']),
                                    bytes.fromhex(request['message_hex']),
                                    bytes.fromhex(request['signature_hex']))
        print(json.dumps({'verified': accepted}))
    else:
        assert not sys.argv[1:]
        print(json.dumps(generate(), indent=2, sort_keys=True))
