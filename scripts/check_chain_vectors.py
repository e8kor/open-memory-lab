#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "rfc8785==0.1.4",
#     "jcs==0.2.1",
#     "cryptography==50.0.2",
#     "pynacl==1.6.2",
# ]
# ///
"""The chain-format test vectors reproduce outside Rust, byte for byte.

Run by .github/workflows/m0-spikes.yml (job chain-vectors) as

    uv run --locked --script scripts/check_chain_vectors.py

This is the critical path's M0 exit item: the vectors that docs/data/chain-format.md
("Test vectors") states, cross-checked against independent, non-Rust implementations.
The libraries and the reasons for using two of each are on
docs/engineering/spikes/index.md, and the recorded finding is
docs/engineering/spikes/chain-vectors.md. The page is the only copy of the vectors. This
script reads the key, the stated bytes, byte counts, hashes and signatures from the page,
and it holds only the inputs: the nine signed fields per event, the body plaintext and the
stand-in rule, which it also reads from the page. It fails with every disagreement found:

- each vector's signed_bytes, rebuilt from its inputs by rfc8785 and by jcs (two RFC 8785
  implementations), equals the stated bytes and the stated byte count;
- every input the script holds equals the value in the page's bytes;
- plaintext_digest, ciphertext_digest, body_bytes and every prev_hash are recomputed
  (the genesis one from 32 zero bytes), never copied;
- SHA-256 of the bytes, by hashlib and by libsodium, equals the stated hash;
- the test key read from the page's PEM gives the stated raw public key and fingerprint;
- cryptography (OpenSSL) and PyNaCl (libsodium) each sign the bytes with the test key and
  produce the stated signature (pure Ed25519 is deterministic), and each verifies it;
- the page's verification procedure, steps 1-7 and 9, passes over the three events;
- one flipped byte in each event fails steps 3 and 4; every reordering, and the omission
  of any event but the last, fails step 1 or step 5; omitting the last event fails step 9,
  against the stated head, as the page states.

Usage: check_chain_vectors.py [--page PATH]  (default: docs/data/chain-format.md)
"""

from __future__ import annotations

import base64
import hashlib
import itertools
import json
import re
import sys
from pathlib import Path

try:
    import cryptography
    import jcs
    import nacl
    import nacl.encoding
    import nacl.exceptions
    import nacl.hash
    import nacl.signing
    import rfc8785
    from cryptography.exceptions import InvalidSignature
    from cryptography.hazmat.backends.openssl.backend import backend as openssl_backend
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric.ed25519 import (
        Ed25519PrivateKey,
        Ed25519PublicKey,
    )
except ImportError as missing:  # run without uv
    sys.exit(
        f"check_chain_vectors: {missing.name} is not importable; run "
        "`uv run --locked --script scripts/check_chain_vectors.py`"
    )

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PAGE = ROOT / "docs/data/chain-format.md"

ZERO_HASH = "sha256:" + "00" * 32

# RFC 8410 fixed DER prefixes for an Ed25519 PKCS#8 private key and SPKI public key.
PKCS8_ED25519_PREFIX = bytes.fromhex("302e020100300506032b657004220420")
SPKI_ED25519_PREFIX = bytes.fromhex("302a300506032b6570032100")

TENANT = "00000000-0000-4000-8000-0000000000aa"


def sha256(data: bytes) -> bytes:
    """SHA-256 by hashlib and by libsodium (PyNaCl); the two must agree."""
    by_hashlib = hashlib.sha256(data).digest()
    by_sodium = nacl.hash.sha256(data, encoder=nacl.encoding.RawEncoder)
    if by_hashlib != by_sodium:
        raise AssertionError(f"hashlib and libsodium disagree on SHA-256 of {len(data)} bytes")
    return by_hashlib
ARTIFACT = "00000000-0000-4000-8000-0000000000a1"


def vector_inputs(body: bytes, stand_in_prefix: bytes) -> list[dict]:
    """The nine signed fields of each vector, as chain-format.md states them.

    Members are in the envelope table's order, not the canonical order, so the
    canonicalizers do the sorting. prev_hash is filled in by the caller from the
    previous vector's computed hash, and vector 2's digests and byte count are computed
    from the body here, never copied.
    """
    ciphertext = sha256(stand_in_prefix + body)
    return [
        {
            "tenant_id": TENANT,
            "seq": 1,
            "event_id": "00000000-0000-4000-8000-0000000000e1",
            "event_type": "tenant.genesis",
            "prev_hash": None,
            "principal": "provider/operator-1",
            "api_key_id": "00000000-0000-4000-8000-0000000000c1",
            "at": "2026-10-29T14:03:17.000000Z",
            "payload": {
                "tenant_id": TENANT,
                "name": "vector-tenant",
                "created_event_id": "00000000-0000-4000-8000-0000000000d2",
                "provider_seq": 2,
            },
        },
        {
            "tenant_id": TENANT,
            "seq": 2,
            "event_id": "00000000-0000-4000-8000-0000000000e2",
            "event_type": "submission.remember",
            "prev_hash": None,
            "principal": "crew-redline/agent-3",
            "api_key_id": "00000000-0000-4000-8000-0000000000c2",
            "at": "2026-10-29T14:05:00.250000Z",
            "payload": {
                "workarea_id": "00000000-0000-4000-8000-0000000000b1",
                "artifact_id": ARTIFACT,
                "claim_key": "deploy-runbook",
                "version": 1,
                "kind": "memory",
                "provenance": "agent",
                "simulation_id": None,
                "body_bytes": len(body),
                "plaintext_digest": "sha256:" + sha256(body).hex(),
                "ciphertext_digest": "sha256:" + sha256(ciphertext).hex(),
                "scan": {"scanner_version": "vector-scanner/1", "result": "pass"},
                "derives_from": [],
                "attachments": [],
            },
        },
        {
            "tenant_id": TENANT,
            "seq": 3,
            "event_id": "00000000-0000-4000-8000-0000000000e3",
            "event_type": "lifecycle.erase",
            "prev_hash": None,
            "principal": "acme/admin-1",
            "api_key_id": "00000000-0000-4000-8000-0000000000c3",
            "at": "2026-10-29T16:00:00.000000Z",
            "payload": {
                "artifact_id": ARTIFACT,
                "erased_by_key_id": "00000000-0000-4000-8000-0000000000c3",
                "dek_destroyed": True,
                "copy_classes": ["silver.body", "object_store", "fts", "embeddings"],
            },
        },
    ]


# ---------------------------------------------------------------------------
# Reading the page
# ---------------------------------------------------------------------------


class PageError(Exception):
    pass


def one(pattern: str, text: str, what: str, flags: int = 0) -> re.Match:
    found = list(re.finditer(pattern, text, flags))
    if len(found) != 1:
        raise PageError(f"expected exactly one {what} on the page, found {len(found)}")
    return found[0]


def read_page(page: Path) -> dict:
    text = page.read_text(encoding="utf-8")
    section = text.split("## Test vectors", 1)
    if len(section) != 2:
        raise PageError("no '## Test vectors' section")
    vectors_text = section[1].split("\n## ", 1)[0]

    private_pem = one(
        r"-----BEGIN PRIVATE KEY-----\n.*?\n-----END PRIVATE KEY-----",
        vectors_text, "private key PEM block", re.S,
    ).group(0)
    public_pem = one(
        r"-----BEGIN PUBLIC KEY-----\n.*?\n-----END PUBLIC KEY-----",
        vectors_text, "public key PEM block", re.S,
    ).group(0)
    raw_public = one(
        r"Raw public key \(32 bytes, hex\):\s*`([0-9a-f]{64})`", vectors_text, "raw public key"
    ).group(1)
    fingerprint = one(
        r"Key id / fingerprint \(SHA-256 of those 32 bytes\):\s*`([0-9a-f]{64})`",
        vectors_text, "key fingerprint",
    ).group(1)
    body_match = one(
        r"the body plaintext is the (\d+) ASCII bytes\s+`([^`]+)` \+ `\\n`",
        vectors_text, "vector 2 body statement",
    )
    body = (body_match.group(2) + "\n").encode("ascii")
    stated_body_bytes = int(body_match.group(1))
    stand_in = one(
        r"SHA-256\(`([^`]+)` \+\s*body bytes\)", vectors_text, "ciphertext stand-in rule"
    ).group(1).encode("ascii")

    vectors = []
    parts = re.split(r"\n### Vector (\d+) — [^\n]*\n", vectors_text)
    # parts = [preamble, "1", body1, "2", body2, ...]
    for number, part in zip(parts[1::2], parts[2::2]):
        count = one(r"\(one line,\s+(\d+) bytes\)", part, f"vector {number} byte count").group(1)
        line = one(r"```json\n([^\n]*)\n```", part, f"vector {number} signed_bytes block").group(1)
        hash_hex = one(r"hash = sha256:([0-9a-f]{64})", part, f"vector {number} hash").group(1)
        sig_hex = one(r"sig  = ed25519:([0-9a-f]{128})", part, f"vector {number} sig").group(1)
        vectors.append(
            {
                "number": int(number),
                "signed_bytes": line.encode("utf-8"),
                "byte_count": int(count),
                "hash": bytes.fromhex(hash_hex),
                "sig": bytes.fromhex(sig_hex),
            }
        )
    if [v["number"] for v in vectors] != [1, 2, 3]:
        raise PageError(f"expected vectors 1, 2, 3; found {[v['number'] for v in vectors]}")
    return {
        "private_pem": private_pem,
        "public_pem": public_pem,
        "raw_public": bytes.fromhex(raw_public),
        "fingerprint": fingerprint,
        "body": body,
        "stated_body_bytes": stated_body_bytes,
        "stand_in": stand_in,
        "vectors": vectors,
    }


def pem_der(pem: str) -> bytes:
    lines = [line for line in pem.splitlines() if not line.startswith("-----")]
    return base64.b64decode("".join(lines), validate=True)


# ---------------------------------------------------------------------------
# The verification procedure (chain-format.md "Verification", steps 1-7 and 9)
# ---------------------------------------------------------------------------

SIGNED_FIELDS = (
    "tenant_id", "seq", "event_id", "event_type", "prev_hash",
    "principal", "api_key_id", "at", "payload",
)


def verify_ed25519_any(public_keys: list[bytes], sig: bytes, message: bytes) -> bool:
    """True iff some key verifies sig, under both Ed25519 implementations."""
    for raw in public_keys:
        try:
            Ed25519PublicKey.from_public_bytes(raw).verify(sig, message)
            openssl_ok = True
        except InvalidSignature:
            openssl_ok = False
        try:
            nacl.signing.VerifyKey(raw).verify(message, sig)
            sodium_ok = True
        except nacl.exceptions.BadSignatureError:
            sodium_ok = False
        if openssl_ok != sodium_ok:
            raise AssertionError("cryptography and PyNaCl disagree on one signature")
        if openssl_ok:
            return True
    return False


def verify_chain(events: list[dict], public_keys: list[bytes], head: tuple[int, bytes]) -> set[int]:
    """Steps 1-7 and 9 of the page's procedure; returns the numbers of the steps that fail."""
    failed: set[int] = set()
    if [e["envelope"]["seq"] for e in events] != list(range(1, len(events) + 1)):
        failed.add(1)
    if not events or events[0]["envelope"]["event_type"] != "tenant.genesis" \
            or events[0]["envelope"]["prev_hash"] != ZERO_HASH:
        failed.add(2)
    computed: list[bytes] = []
    for i, event in enumerate(events):
        stored = event["signed_bytes"]
        if not verify_ed25519_any(public_keys, event["sig"], stored):
            failed.add(3)
        digest = sha256(stored)
        computed.append(digest)
        if digest != event["hash"]:
            failed.add(4)
        if i > 0 and event["envelope"]["prev_hash"] != "sha256:" + computed[i - 1].hex():
            failed.add(5)
        try:
            parsed = json.loads(stored.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            failed.update({6, 7})
            continue
        if rfc8785.dumps(parsed) != stored or jcs.canonicalize(parsed) != stored:
            failed.add(6)
        if not isinstance(parsed, dict) or set(parsed) != set(SIGNED_FIELDS) or any(
            parsed[f] != event["envelope"][f] for f in SIGNED_FIELDS
        ):
            failed.add(7)
    if events:
        last = events[-1]
        if (last["envelope"]["seq"], computed[-1]) != head or not verify_ed25519_any(
            public_keys, last["sig"], last["signed_bytes"]
        ):
            failed.add(9)
    else:
        failed.add(9)
    return failed


# ---------------------------------------------------------------------------
# The check
# ---------------------------------------------------------------------------


def first_difference(a: bytes, b: bytes) -> str:
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return f"first difference at byte {i}: {a[max(0, i - 20):i + 20]!r} vs {b[max(0, i - 20):i + 20]!r}"
    return f"one is a prefix of the other ({len(a)} vs {len(b)} bytes)"


def main(argv: list[str]) -> int:
    page_path = DEFAULT_PAGE
    if argv[:1] == ["--page"] and len(argv) == 2:
        page_path = Path(argv[1])
    elif argv:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2

    errors: list[str] = []
    try:
        page = read_page(page_path)
    except PageError as e:
        print(f"check_chain_vectors: FAIL: {page_path}: {e}", file=sys.stderr)
        return 1

    # --- the test key --------------------------------------------------------
    pkcs8 = pem_der(page["private_pem"])
    if len(pkcs8) != 48 or not pkcs8.startswith(PKCS8_ED25519_PREFIX):
        errors.append("the private PEM is not an RFC 8410 Ed25519 PKCS#8 key")
    seed = pkcs8[-32:]
    spki = pem_der(page["public_pem"])
    if len(spki) != 44 or not spki.startswith(SPKI_ED25519_PREFIX):
        errors.append("the public PEM is not an RFC 8410 Ed25519 SubjectPublicKeyInfo")

    openssl_key = serialization.load_pem_private_key(page["private_pem"].encode(), password=None)
    if not isinstance(openssl_key, Ed25519PrivateKey):
        errors.append("cryptography does not read the private PEM as an Ed25519 key")
        print_errors(errors)
        return 1
    sodium_key = nacl.signing.SigningKey(seed)

    public_from = {
        "the public PEM": spki[-32:],
        "cryptography's key from the private PEM": openssl_key.public_key().public_bytes(
            serialization.Encoding.Raw, serialization.PublicFormat.Raw
        ),
        "PyNaCl's key from the PKCS#8 seed": bytes(sodium_key.verify_key),
    }
    for source, raw in public_from.items():
        if raw != page["raw_public"]:
            errors.append(f"{source} gives public key {raw.hex()}, the page states {page['raw_public'].hex()}")
    fingerprint = sha256(page["raw_public"]).hex()
    if fingerprint != page["fingerprint"]:
        errors.append(f"SHA-256 of the raw public key is {fingerprint}, the page states {page['fingerprint']}")

    # --- the vectors ---------------------------------------------------------
    if len(page["body"]) != page["stated_body_bytes"]:
        errors.append(
            f"vector 2's body is {len(page['body'])} bytes, the page says {page['stated_body_bytes']}"
        )
    inputs = vector_inputs(page["body"], page["stand_in"])
    prev_hash = ZERO_HASH  # genesis: 32 zero bytes
    events = []
    for fields, stated in zip(inputs, page["vectors"]):
        n = stated["number"]
        fields["prev_hash"] = prev_hash
        by_rfc8785 = rfc8785.dumps(fields)
        by_jcs = jcs.canonicalize(fields)
        if by_rfc8785 != by_jcs:
            errors.append(f"vector {n}: rfc8785 and jcs disagree; {first_difference(by_rfc8785, by_jcs)}")
        if by_rfc8785 != stated["signed_bytes"]:
            errors.append(
                f"vector {n}: the canonical bytes rebuilt from the inputs differ from the page's; "
                f"{first_difference(by_rfc8785, stated['signed_bytes'])}"
            )
        try:
            parsed = json.loads(stated["signed_bytes"])
        except json.JSONDecodeError as e:
            errors.append(f"vector {n}: the page's signed_bytes are not JSON: {e}")
            parsed = {}
        for field in SIGNED_FIELDS:
            if parsed.get(field) != fields[field]:
                errors.append(
                    f"vector {n}: input {field} is {fields[field]!r}, the page's bytes carry {parsed.get(field)!r}"
                )
        signed_bytes = stated["signed_bytes"]
        if len(signed_bytes) != stated["byte_count"]:
            errors.append(f"vector {n}: signed_bytes are {len(signed_bytes)} bytes, the page says {stated['byte_count']}")
        digest = sha256(signed_bytes)
        if digest != stated["hash"]:
            errors.append(f"vector {n}: SHA-256 is {digest.hex()}, the page states {stated['hash'].hex()}")
        by_openssl = openssl_key.sign(signed_bytes)
        by_sodium = sodium_key.sign(signed_bytes).signature
        for library, sig in (("cryptography", by_openssl), ("PyNaCl", by_sodium)):
            if sig != stated["sig"]:
                errors.append(f"vector {n}: {library} signs {sig.hex()}, the page states {stated['sig'].hex()}")
        if not verify_ed25519_any([page["raw_public"]], stated["sig"], signed_bytes):
            errors.append(f"vector {n}: the stated sig does not verify under the stated public key")
        events.append(
            {"signed_bytes": signed_bytes, "hash": stated["hash"], "sig": stated["sig"],
             "envelope": dict(fields)}
        )
        prev_hash = "sha256:" + digest.hex()

    # --- the verification procedure, and the page's claim about mutations ---
    keys = [page["raw_public"]]
    head = (events[-1]["envelope"]["seq"], events[-1]["hash"])
    failed = verify_chain(events, keys, head)
    if failed:
        errors.append(f"the verification procedure fails steps {sorted(failed)} over the page's chain")

    controls = 0
    for i in range(len(events)):
        mutated = [dict(e) for e in events]
        b = bytearray(mutated[i]["signed_bytes"])
        b[len(b) // 2] ^= 0x01
        mutated[i]["signed_bytes"] = bytes(b)
        steps = verify_chain(mutated, keys, head)
        controls += 1
        if not {3, 4} <= steps:
            errors.append(f"one flipped byte in vector {i + 1} fails steps {sorted(steps)}, not 3 and 4")
    for order in itertools.permutations(range(len(events))):
        if list(order) == list(range(len(events))):
            continue
        steps = verify_chain([events[j] for j in order], keys, head)
        controls += 1
        if not steps & {1, 5}:
            errors.append(f"reordering {[j + 1 for j in order]} fails steps {sorted(steps)}, not 1 or 5")
    for i in range(len(events)):
        kept = [e for j, e in enumerate(events) if j != i]
        steps = verify_chain(kept, keys, head)
        controls += 1
        # A dropped tail leaves a contiguous, correctly linked chain: only the stated
        # head (step 9) can tell it is short.
        expected = {9} if i == len(events) - 1 else {1, 5}
        if not steps & expected:
            errors.append(
                f"omitting vector {i + 1} fails steps {sorted(steps)}, not {' or '.join(map(str, sorted(expected)))}"
            )

    if errors:
        print_errors(errors)
        return 1

    print(
        f"check_chain_vectors: OK: {page_path.relative_to(ROOT) if page_path.is_relative_to(ROOT) else page_path}: "
        f"3 vectors reproduce byte for byte ({', '.join(str(e['signed_bytes'].__len__()) for e in events)} bytes); "
        f"steps 1-7 and 9 pass; {controls} mutation controls fail as the page states"
    )
    print(
        "check_chain_vectors: versions: "
        f"Python {sys.version.split()[0]}; rfc8785 {version_of('rfc8785')}; jcs {version_of('jcs')}; "
        f"cryptography {cryptography.__version__} ({openssl_backend.openssl_version_text()}); "
        f"PyNaCl {nacl.__version__} (bundled libsodium: Ed25519, SHA-256); hashlib {hashlib_backend()}"
    )
    return 0


def version_of(dist: str) -> str:
    from importlib.metadata import version

    return version(dist)


def hashlib_backend() -> str:
    import ssl

    name = hashlib.sha256.__name__  # openssl_sha256 when CPython links OpenSSL
    return f"{name} ({ssl.OPENSSL_VERSION})" if name.startswith("openssl") else name


def print_errors(errors: list[str]) -> None:
    print(f"check_chain_vectors: FAIL: {len(errors)} disagreement(s):", file=sys.stderr)
    for e in errors:
        print(f"  - {e}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
