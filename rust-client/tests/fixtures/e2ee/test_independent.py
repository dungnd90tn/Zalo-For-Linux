#!/usr/bin/env python3
"""Independent Python/OpenSSL oracle for the untouched bundled JS engine."""
import hashlib
import hmac
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from generate_vectors import hkdf, vectors

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def protobuf(raw):
    if len(raw) > 65536:
        raise ValueError("oversized fixture")
    offset, fields = 0, {}

    def varint():
        nonlocal offset
        value = 0
        for shift in range(0, 70, 7):
            if offset >= len(raw):
                raise ValueError("truncated fixture")
            byte = raw[offset]
            offset += 1
            value |= (byte & 127) << shift
            if byte < 128:
                return value
        raise ValueError("oversized varint")

    while offset < len(raw):
        tag = varint()
        number, wire = tag >> 3, tag & 7
        if not number or number in fields:
            raise ValueError("invalid/duplicate fixture field")
        if wire == 0:
            value = varint()
        elif wire == 2:
            length = varint()
            if length > len(raw) - offset:
                raise ValueError("truncated fixture field")
            value = raw[offset:offset + length]
            offset += length
        else:
            raise ValueError("unexpected fixture wire type")
        fields[number] = value
    return fields


def agreement(private, public):
    if len(private) != 32 or len(public) != 33 or public[0] != 5:
        raise ValueError("invalid tagged Curve25519 fixture key")
    with tempfile.TemporaryDirectory(prefix="zalo-public-e2ee-") as tmp:
        private_path, peer_path = Path(tmp) / "private.der", Path(tmp) / "peer.der"
        private_path.write_bytes(bytes.fromhex("302e020100300506032b656e04220420") + private)
        peer_path.write_bytes(bytes.fromhex("302a300506032b656e032100") + public[1:])
        return subprocess.run(
            ["openssl", "pkeyutl", "-derive", "-inkey", str(private_path),
             "-keyform", "DER", "-peerkey", str(peer_path), "-peerform", "DER"],
            check=True, capture_output=True, timeout=5,
        ).stdout


def decode_first_message(fixture, message_label=b"ZaloMessageKeys"):
    get = lambda key: bytes.fromhex(fixture[key])
    outer_raw = get("initial_prekey_message")
    if outer_raw[0] != 0x33:
        raise ValueError("unexpected prekey version")
    outer = protobuf(outer_raw[1:])
    assert set(outer) == {1, 2, 3, 4, 5, 6}
    assert (outer[1], outer[5], outer[6]) == (9, 1001, 7)
    assert outer[3] == get("alice_identity_public")
    inner_raw = outer[4]
    assert inner_raw[0] == 0x33
    inner = protobuf(inner_raw[1:-8])
    assert set(inner) == {1, 2, 3, 4}
    assert (inner[2], inner[3]) == (0, 0)
    # Independent receiver-side X3DH: no session state exported by the engine.
    initial = b"\xff" * 32 + b"".join([
        agreement(get("bob_signed_prekey_private"), outer[3]),
        agreement(get("bob_identity_private"), outer[2]),
        agreement(get("bob_signed_prekey_private"), outer[2]),
        agreement(get("bob_one_time_private"), outer[2]),
    ])
    root = hkdf(initial, bytes(32), b"ZaloText", 96)[:32]
    dh = agreement(get("bob_signed_prekey_private"), inner[1])
    chain = hkdf(dh, root, b"ZaloRatchet", 96)[32:64]
    seed = hmac.digest(chain, b"\x01", "sha256")
    keys = hkdf(seed, bytes(32), message_label, 96)
    aad = outer[3] + get("bob_identity_public") + inner_raw[:-8]
    expected_mac = hmac.digest(keys[32:64], aad, "sha256")[:8]
    if not hmac.compare_digest(expected_mac, inner_raw[-8:]):
        raise ValueError("independent MAC rejection")
    plaintext = subprocess.run(
        ["openssl", "enc", "-d", "-aes-256-cbc", "-K", keys[:32].hex(),
         "-iv", keys[64:80].hex()], input=inner[4], capture_output=True,
        check=True, timeout=5,
    ).stdout
    return plaintext


class IndependentE2eeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = json.loads((HERE / "session.json").read_text())

    def test_vectors_reproduce(self):
        self.assertEqual(json.loads((HERE / "vectors.json").read_text()), vectors())

    def test_actual_engine_source_pin(self):
        source = ROOT / "pc-dist/libs/libsignal-protocol.static.js"
        self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), self.fixture["library_sha256"])

    def test_prekey_message_derived_and_decrypted_independently(self):
        self.assertEqual(decode_first_message(self.fixture).decode(), self.fixture["plaintext"])

    def test_stock_signal_label_is_not_compatible(self):
        with self.assertRaisesRegex(ValueError, "MAC rejection"):
            decode_first_message(self.fixture, b"WhisperMessageKeys")

    def test_tampered_ciphertext_rejected(self):
        corrupt = dict(self.fixture)
        value = bytearray.fromhex(corrupt["initial_prekey_message"])
        # Last byte is signedPreKeyId, so alter the embedded MAC through decode.
        outer = protobuf(value[1:])
        message_start = bytes(value).index(outer[4])
        value[message_start + len(outer[4]) - 1] ^= 1
        corrupt["initial_prekey_message"] = value.hex()
        with self.assertRaisesRegex(ValueError, "MAC rejection"):
            decode_first_message(corrupt)

    def test_malformed_fields_and_tagged_keys_rejected(self):
        for raw in (b"\x0a\x04x", b"\x08\x01\x08\x02", b"\x00"):
            with self.assertRaises(ValueError):
                protobuf(raw)
        with self.assertRaises(ValueError):
            agreement(bytes(32), bytes(33))


if __name__ == "__main__":
    unittest.main()
