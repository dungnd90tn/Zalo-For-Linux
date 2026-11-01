#!/usr/bin/env python3
"""Independent public E2EE fixtures; no production keys, profiles, or network."""
import hashlib
import hmac
import json
from pathlib import Path


def hkdf(ikm, salt, info, length):
    prk = hmac.digest(salt, ikm, "sha256")
    output, previous = b"", b""
    for counter in range(1, (length + 31) // 32 + 1):
        previous = hmac.digest(prk, previous + info + bytes([counter]), "sha256")
        output += previous
    return output[:length]


def vectors():
    # RFC 5869 test case 1 validates this generator before Zalo labels are used.
    actual = hkdf(bytes([11]) * 22, bytes(range(13)), bytes(range(240, 250)), 42)
    assert actual.hex() == (
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
        "34007208d5b887185865"
    )
    ikm, salt = bytes(range(32)), bytes(32)
    cases = []
    for label in ("ZaloText", "ZaloRatchet", "ZaloMessageKeys", "WhisperMessageKeys"):
        cases.append({"label": label, "ikm": ikm.hex(), "salt": salt.hex(),
                      "output": hkdf(ikm, salt, label.encode(), 96).hex()})
    seed = bytes(range(32, 64))
    message_seed = hmac.digest(seed, b"\x01", "sha256")
    keys = hkdf(message_seed, salt, b"ZaloGroup", 48)
    return {
        "description": "PUBLIC SYNTHETIC FIXTURES ONLY; Python hashlib/hmac RFC5869 checked",
        "hkdf": cases,
        "group": {"chain_seed": seed.hex(), "message_seed": message_seed.hex(),
                  "next_chain": hmac.digest(seed, b"\x02", "sha256").hex(),
                  "iv": keys[:16].hex(), "cipher_key": keys[16:].hex()},
    }


if __name__ == "__main__":
    destination = Path(__file__).with_name("vectors.json")
    destination.write_text(json.dumps(vectors(), indent=2) + "\n")
    print("Wrote public synthetic vectors.json; RFC5869 case 1 passed")
