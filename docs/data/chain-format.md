---
status: Stable
verified: 2026-11-06
answers: "the byte-level chain spec: the signed envelope fields, canonical JSON, SHA-256 hash, Ed25519 signature, the genesis rule and the verification procedure an auditor can implement from this page alone"
---

# Journal chain format

This page specifies the per-tenant signed hash chain byte by byte. An auditor can implement verification from it alone. For the commit point and key custody, see [storage](../architecture/storage.md#the-per-tenant-hash-chain) and [security](../architecture/security.md). For event payloads, see the [event catalog](event-catalog.md).

## The signed fields

The [envelope](event-catalog.md#the-envelope) has eleven fields. Nine are signed. Two are derived from the signed bytes, so they are excluded.

| Envelope field | Signed | Encoding in `signed_bytes` |
|---|---|---|
| `tenant_id` | yes | lowercase hyphenated UUID string |
| `seq` | yes | JSON integer |
| `event_id` | yes | lowercase hyphenated UUID string |
| `event_type` | yes | string, exactly the catalog spelling |
| `prev_hash` | yes | string `sha256:` + 64 lowercase hex characters |
| `principal` | yes | string |
| `api_key_id` | yes | lowercase hyphenated UUID string, or JSON `null` |
| `at` | yes | RFC 3339 UTC, exactly six fractional digits, `Z` suffix |
| `payload` | yes | the event's payload object, canonicalized in the same document |
| `hash` | no | SHA-256 of `signed_bytes` |
| `sig` | no | the signature over `signed_bytes` |

- **All nine signed fields are always present.** A field with no value is JSON `null`, never omitted.
- **Digests are strings.** Every digest is `sha256:` + 64 lowercase hex characters, the spelling the API serves.
- **Numbers are integers below 2^53.** RFC 8785 serializes numbers as doubles, so larger values would canonicalize lossily. No float appears in an envelope or payload.
- **`at` is fixed-width.** UTC, `Z`, exactly six fractional digits, for example `2026-10-29T14:03:17.000000Z`.

## signed_bytes: canonical JSON

`signed_bytes` is the [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) canonicalization of the JSON object that holds exactly the nine signed fields. Members sort by code point. There is no insignificant whitespace. Encoding is UTF-8.

The stored `signed_bytes` column is authoritative. The signature is verified over the stored bytes, never over a re-serialization of the row. A verifier also checks that the stored bytes re-canonicalize to themselves and that their parsed fields match the row (steps 6 and 7 below).

## hash and sig

- **`hash`** is SHA-256 of `signed_bytes`, displayed as `sha256:` + 64 lowercase hex characters. The next event's `prev_hash` names it.
- **`sig`** is pure Ed25519 ([RFC 8032](https://www.rfc-editor.org/rfc/rfc8032)) over the full `signed_bytes`. It is not over the hash, and it is not Ed25519ph. Any raw-input Ed25519 verify call checks it, `openssl pkeyutl -verify -rawin` among them. It is displayed as `ed25519:` + 128 lowercase hex characters.

Both are stored as `bytea`. The prefixes are the display and API spelling.

## Chaining

- `prev_hash` is the previous event's `hash` in the same tenant's chain. Nothing links across tenants.
- `seq` starts at 1 and increases by exactly 1. A gap is a verification failure.
- **Genesis.** The first event of every chain is `tenant.genesis` at `seq` 1 with `prev_hash` of 32 zero bytes (`sha256:` + 64 `0` characters). A tenant's genesis payload references the provider chain's `tenant.created` event. The provider tenant's own genesis has a null reference. See [event catalog](event-catalog.md).

## The signing key and rotation

A signing key's id is the lowercase hex SHA-256 of its 32 raw public key bytes.

- **Rotation is journaled** as `system.signing_key_rotated` in the provider tenant's chain. It is the first event the new key signs. See [event catalog](event-catalog.md).
- **Planned rotation** carries `cross_sig`: a pure Ed25519 signature by the old key over the ASCII bytes `oml-signing-key-rotation:<new_key_id>:<new_public_key_hex>`. The old key vouches for the new one.
- **Unplanned rotation** (old key lost or compromised) has no cross-signature. It carries a ceremony reference, and trust in the new key restarts out of band.
- **The verifier holds a key set.** Events carry no key id. An event verifies if its `sig` verifies under some key in the authenticated set. The rotation events bound which key was current when.

## Authenticating the public key

A bundle that embeds its own key authenticates nothing by itself. Confirm one key fingerprint over a channel independent of the bundle. `oml audit verify-chain` takes the confirmed key with `--signing-key`.

1. One confirmed fingerprint is enough.
2. Cross-signatures extend the confirmed key across planned rotations, in either direction.
3. An unplanned rotation breaks the extension. Confirm the new key out of band again.

## Verification

Input: one tenant's events by ascending `seq`, each with its stored `signed_bytes`, `hash`, `sig` and envelope columns; the stated chain head; and the authenticated key set.

1. Check `seq` starts at 1 and increases by exactly 1.
2. Check event 1 is `tenant.genesis` with `prev_hash` of 32 zero bytes.
3. Verify each `sig` over the stored `signed_bytes` with pure Ed25519 against the key set. The event passes if some key verifies it.
4. Compute SHA-256 of each `signed_bytes` and check it equals the stored `hash`.
5. For each event after the first, check `prev_hash` equals the previous event's recomputed hash from step 4.
6. Parse `signed_bytes` and check it is canonical: re-canonicalize and byte-compare.
7. Check the nine parsed fields match the row's envelope.
8. For a non-provider tenant, check the genesis payload's `created_event_id` names a `tenant.created` event in the provider chain whose `tenant_id` matches.
9. Check the stated chain head equals the last event's `seq` and step 4 hash, and verify the head's signature.

The result names the verified head: tenant, `seq` and hash. Passing proves the integrity and order of the record under the deployment's key. It does not prove that what agents wrote is true. Order is proven by `seq` alone. Timestamps are informational.

## Test vectors

A three-event chain with every byte shown: genesis, a submission, an erasure. Digests and signatures were computed over exactly these bytes with python3 `hashlib` and OpenSSL 3.5.5 on 2026-10-29. An implementation must reproduce them byte for byte.

**Test key. Never deployed.** Its private half is published here, so it can never sign a real chain.

```text
-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIAmHRZSvyTxEr5xIbW3Ui2hRS1c2p9ZMrHLZBy+Ypjcc
-----END PRIVATE KEY-----
-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA7oyg5b8Ll0cMmM6O5vr1R1+Qte1pdSoTx92cZuSik4M=
-----END PUBLIC KEY-----
```

Raw public key (32 bytes, hex):
`ee8ca0e5bf0b97470c98ce8ee6faf5475f90b5ed69752a13c7dd9c66e4a29383`
Key id / fingerprint (SHA-256 of those 32 bytes):
`d92de0b418b6e0b3b6b10cb2b2392586ce1e02f3c11237dbaf39a283882c1676`

Signatures were produced with `openssl pkeyutl -sign -rawin` over each vector's `signed_bytes`
and verify with `openssl pkeyutl -verify -pubin -rawin`.

### Vector 1 — genesis

`tenant.genesis` at seq 1, `prev_hash` all zeros. `signed_bytes` (one line, 508 bytes):

```json
{"api_key_id":"00000000-0000-4000-8000-0000000000c1","at":"2026-10-29T14:03:17.000000Z","event_id":"00000000-0000-4000-8000-0000000000e1","event_type":"tenant.genesis","payload":{"created_event_id":"00000000-0000-4000-8000-0000000000d2","name":"vector-tenant","provider_seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"},"prev_hash":"sha256:0000000000000000000000000000000000000000000000000000000000000000","principal":"provider/operator-1","seq":1,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}
```

```text
hash = sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a
sig  = ed25519:3d22e7cf20b6db79eeb11479c505aeafab69209198fc2d8649c37032297150c56cef410f240fb97f3e603dc445fcd63da1abf36fe29f5633cc6247707b2f0c04
```

### Vector 2 — a submission

`submission.remember` at seq 2. Its `prev_hash` is vector 1's hash, and the body plaintext is the 53 ASCII bytes `The deploy runbook: drain, upgrade, verify, undrain.` + `\n`, and `plaintext_digest` is the SHA-256 of those bytes. The vector tests the chain format, not a cipher, so the "ciphertext" is a stand-in: the 32 bytes SHA-256(`vector-ciphertext:` + body bytes). `ciphertext_digest` is the SHA-256 of those 32 bytes. `signed_bytes` (one line, 870 bytes):

```json
{"api_key_id":"00000000-0000-4000-8000-0000000000c2","at":"2026-10-29T14:05:00.250000Z","event_id":"00000000-0000-4000-8000-0000000000e2","event_type":"submission.remember","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","attachments":[],"body_bytes":53,"ciphertext_digest":"sha256:505007c2183a32f9a29b6635dc1a56d89711b658d0624eb662dc986141213688","claim_key":"deploy-runbook","derives_from":[],"kind":"memory","plaintext_digest":"sha256:6e04a033aab31ee48e5bf89a4a6ef03e658f25f9b428de3e45c2ac7519dd2532","provenance":"agent","scan":{"result":"pass","scanner_version":"vector-scanner/1"},"simulation_id":null,"version":1,"workarea_id":"00000000-0000-4000-8000-0000000000b1"},"prev_hash":"sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a","principal":"crew-redline/agent-3","seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}
```

```text
hash = sha256:72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1
sig  = ed25519:068b0527c06897539882dfd22dc1970adbef83ca955559f80d529b76f7f26df64b14166b0a934eff9829704b3516f8a7b2c0bfa1a9a0ac529d1ec0e554756908
```

### Vector 3 — an erasure, verifying after the DEK is gone

`lifecycle.erase` at seq 3 erases vector 2's artifact and destroys its key. Verifying the chain needs no key and no plaintext, because the signed bytes carry digests only. The chain verifies after erasure exactly as before. `signed_bytes` (one line, 550 bytes):

```json
{"api_key_id":"00000000-0000-4000-8000-0000000000c3","at":"2026-10-29T16:00:00.000000Z","event_id":"00000000-0000-4000-8000-0000000000e3","event_type":"lifecycle.erase","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","copy_classes":["silver.body","object_store","fts","embeddings"],"dek_destroyed":true,"erased_by_key_id":"00000000-0000-4000-8000-0000000000c3"},"prev_hash":"sha256:72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1","principal":"acme/admin-1","seq":3,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}
```

```text
hash = sha256:e7837fc16aa96fbffe4cb6c0b2551522fa44d2c89006e7014bfc67d932512536
sig  = ed25519:2c38c30d096ee592ad0e26166665382a998775c7efb8c4165935c1c4db50cac03995324750bad240e0c0bb3b4177a132059ad57386d0514fb60f44271fafd804
```

Running the [verification procedure](#verification) over these three events against the test
public key passes steps 1–7 and 9 (step 8 needs a provider chain these vectors deliberately do
not include; the referenced `created_event_id` is a placeholder). Any single changed byte in any
`signed_bytes` fails step 3 and step 4; any reordering, and the omission of any event but the
last, fails step 1 or step 5; omitting the last event fails step 9 alone, against the stated
head.

Running the [verification procedure](#verification) over these events against the test public key passes steps 1 to 7 and 9. Step 8 needs a provider chain the vectors do not include. Any changed byte in any `signed_bytes` fails steps 3 and 4. Any reordering, or omitting any event but the last, fails step 1 or 5. Omitting the last event fails step 9 alone.

`scripts/check_chain_vectors.py` reproduces these vectors byte for byte with independent RFC 8785 and Ed25519 implementations.
