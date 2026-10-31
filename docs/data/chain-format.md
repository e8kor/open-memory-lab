<!--
id: data/chain-format
tier: engineering
status: design draft for review · decided, not built
answers: the byte-level chain spec — the signed envelope fields, JCS signed_bytes, SHA-256 hash, Ed25519 signature, the genesis rule, and the verification procedure an auditor can implement from this page alone
-->

# Journal chain format

**Status:** design draft for review · decided, not built

This page is the normative byte-level specification of the per-tenant signed hash chain: which
envelope fields are signed, how `signed_bytes` is constructed, how `hash` and `sig` are computed,
how events chain, how a recipient obtains and authenticates the deployment public key, and the
verification procedure an auditor can implement from this page alone. It is the byte-level annex
of D1 ([ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md)); the commit point, the
chain-head lock and key custody are declared in
[storage](../architecture/storage.md#the-per-tenant-hash-chain) and
[security](../architecture/security.md); the journal DDL is in
[postgres-schema](postgres-schema.md); event payload shapes are in
[event-catalog](event-catalog.md). The test vectors at the end are computed, not sketched: every
digest and signature below was produced over the exact bytes shown.

## The signed fields

The [envelope](event-catalog.md#the-envelope) has eleven fields. Nine are signed; two are derived
from the signed bytes and therefore excluded. The decision is stated for every field — an
implementer never guesses:

| Envelope field | Signed | Canonical encoding in `signed_bytes` |
|---|---|---|
| `tenant_id` | yes | lowercase hyphenated UUID string |
| `seq` | yes | JSON integer |
| `event_id` | yes | lowercase hyphenated UUID string |
| `event_type` | yes | string, exactly the catalog spelling |
| `prev_hash` | yes | string `sha256:` + 64 lowercase hex characters |
| `principal` | yes | string |
| `api_key_id` | yes | lowercase hyphenated UUID string, or JSON `null` |
| `at` | yes | RFC 3339 UTC, exactly six fractional digits, `Z` suffix |
| `payload` | yes | the event's payload object, canonicalized as part of the same document |
| `hash` | **no** | derived: SHA-256 of `signed_bytes` — including it would be circular |
| `sig` | **no** | derived: the signature over `signed_bytes` — including it would be circular |

Rules that make the set unambiguous:

- **The nine signed fields are always present.** A field with no value is JSON `null`
  (`api_key_id` for events no API key authored), never omitted. A fixed member set means a
  verifier checks one shape, not a family of shapes.
- **Digests are strings, everywhere.** Every digest in the envelope and in payloads is the string
  `sha256:` + 64 lowercase hex characters — the same spelling the API serves, so the signed form
  and the displayed form never need a translation step.
- **Numbers are integers below 2⁵³.** RFC 8785 serializes numbers as IEEE-754 doubles, so a
  value at or above 2⁵³ would canonicalize lossily. `seq`, `version`, `body_bytes` and every
  other numeric field are JSON integers and must stay below 2⁵³; for `seq` that is a stated
  bound of roughly 9×10¹⁵ events per tenant, accepted rather than worked around. No float ever
  appears in an envelope or payload.
- **`at` is fixed-width.** Always UTC, always `Z`, always exactly six fractional digits
  (microseconds, Postgres `timestamptz` precision) — e.g. `2026-10-29T14:03:17.000000Z`. A
  canonical byte form cannot tolerate optional precision. Display surfaces may trim; the signed
  form never does. The clock that produces it is declared in
  [storage](../architecture/storage.md#time).

## signed_bytes: RFC 8785 canonical JSON

`signed_bytes` is the [RFC 8785 (JCS)](https://www.rfc-editor.org/rfc/rfc8785) canonicalization
of the JSON object holding exactly the nine signed fields: members sorted by code point, no
insignificant whitespace, UTF-8, JCS string escaping, JCS number serialization. The `payload`
object is canonicalized as part of the same document — payload members sort by the same rule.

The stored `signed_bytes` column ([postgres-schema](postgres-schema.md)) is the authoritative
byte string: the signature is verified over the stored bytes, never over a re-serialization of
the row. The construction rule above is the writer's obligation; the verifier's corresponding
check is that the stored bytes re-canonicalize to themselves and that their parsed fields match
the row's columns (steps 6–7 [below](#verification)). This split is deliberate: verification
survives any future serializer change, and a writer that drifts from canonical form is caught as
a verification failure, not absorbed silently.

## hash and sig

- **`hash` = SHA-256(`signed_bytes`)** — 32 bytes, displayed as `sha256:` + 64 lowercase hex.
  The hash exists for chaining: it is what the next event's `prev_hash` names.
- **`sig` = pure Ed25519 per [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032) over
  `signed_bytes`** — over the full canonical bytes, **not** over the hash, and not Ed25519ph.
  Pure Ed25519 hashes the message internally, so signing the message itself leaves exactly one
  answer to "what was signed", verifiable with every mainstream raw-input Ed25519 verify call
  (`openssl pkeyutl -verify -rawin` among them). Signing the 32-byte hash instead would make the
  signature attest a digest rather than the bytes, adding a construction a verifier must get
  right for no gain. 64 bytes, displayed as `ed25519:` + 128 lowercase hex.

Both are stored as `bytea`; the `sha256:` / `ed25519:` prefixes are the display and API spelling
of the same bytes.

## The chaining rule

- `prev_hash` = the previous event's `hash` — the previous `seq` in the **same tenant's** chain.
  Nothing links across tenants; cross-chain anchoring is by reference inside payloads
  (`tenant.genesis` → `tenant.created`), never by hash linkage.
- `seq` starts at 1 and increments by exactly 1 — a gap is a verification failure, which is what
  makes omission detectable, not just edits.
- **Genesis:** the first event of every tenant chain is `tenant.genesis` at `seq` 1 with
  `prev_hash` = 32 zero bytes (`sha256:` + 64 `0` characters). A non-provider tenant's genesis
  payload references the provider-chain `tenant.created` event that created it; the reserved
  provider tenant's own chain opens the same way with that reference null — the deployment root
  anchors to nothing because nothing precedes it ([event-catalog](event-catalog.md#tenantgenesis)).

## The signing key, rotation, and the verifier's key set

The deployment signing key's custody — lead-only, takeover journalled — is
[storage](../architecture/storage.md#the-per-tenant-hash-chain)'s and
[security](../architecture/security.md)'s. This page owns its identity and rotation as the
verifier sees them:

- **A signing key's id is its fingerprint:** the lowercase hex SHA-256 of the 32 raw public key
  bytes. Key id and fingerprint are one value, so the id printed in a ceremony and the id in a
  rotation event cannot disagree about which key they name.
- **Rotation is journalled** as `system.signing_key_rotated` in the reserved provider tenant's
  chain — the first event the **new** key signs
  ([event-catalog](event-catalog.md#systemsigning_key_rotated)). A planned rotation carries a
  cross-signature: `cross_sig` = pure Ed25519 by the **old** key over the ASCII bytes of
  `oml-signing-key-rotation:<new_key_id>:<new_public_key_hex>` — old attests new, so trust
  extends forward without a new ceremony. An unplanned rotation (old key compromised or lost)
  has no cross-signature by definition; it carries a ceremony reference instead, and trust in
  the new key re-roots in that out-of-band ceremony. This asymmetry is the honest one: a lost
  key cannot vouch for its successor.
- **The verifier holds a key set, not a key.** The journal rows carry no per-event signing-key
  id (the envelope's fields are closed, above), so per-event key attribution is by trial
  verification: an event verifies iff its `sig` verifies under some key in the authenticated
  set. The rotation events bound which key was current when, and the set is small — one key per
  rotation. What this proves, stated plainly: every event was signed by a deployment signing
  key, in the chained order; which specific key signed a given event is established by trial
  verification, and the rotation history itself is chained evidence.

## How the public key reaches an export recipient

An [evidence export](../architecture/audit-and-lineage.md#the-evidence-export) embeds the
deployment's signing key set: each key's 32 raw public key bytes (hex), its fingerprint-id, and
every `system.signing_key_rotated` event with its cross-signature. These are the evidence-export
terms for key authentication:

1. **An embedded key authenticates nothing by itself.** A tamperer who can rewrite the bundle
   can rewrite the embedded key. The recipient must confirm one key fingerprint over a channel
   independent of the bundle — the operator reads the fingerprint from the deployment
   (`oml audit verify-chain` prints the key set; the verb is the
   [CLI reference](../cli/reference.md)'s) and the recipient compares. One confirmed fingerprint
   is enough.
2. **Cross-signatures extend the confirmed key across planned rotations**, in either direction:
   from any confirmed key, every cross-signed successor is authenticated by verifying the
   rotation events' `cross_sig` chain.
3. **An unplanned rotation breaks the extension deliberately.** The recipient must confirm the
   post-incident key's fingerprint out of band again, against the ceremony the rotation event
   references. A bundle spanning an unplanned rotation says so rather than implying one
   confirmation covered it.

## Verification

The procedure, numbered so an auditor can implement it from this page. Input: one tenant's
events ordered by `seq` ascending (each with its stored `signed_bytes`, `hash`, `sig` and
envelope columns), the stated chain head, and the authenticated key set (above).

1. Check `seq` starts at 1 and increments by exactly 1 — no gaps, no duplicates.
2. Check event 1 has `event_type` `tenant.genesis` and `prev_hash` = 32 zero bytes.
3. For each event, verify `sig` over the stored `signed_bytes` with pure Ed25519 (RFC 8032)
   against the authenticated key set; the event passes iff some key in the set verifies it.
4. For each event, compute SHA-256(`signed_bytes`) and check it equals the stored `hash`.
5. For each event after the first, check `prev_hash` equals the previous event's hash as
   computed in step 4 — the recomputed value, not the stored column.
6. Parse `signed_bytes` as JSON and check it is in RFC 8785 canonical form: re-canonicalize the
   parsed value and byte-compare with the stored bytes.
7. Check the nine parsed fields match the row's served envelope — `tenant_id`, `seq`,
   `event_id`, `event_type`, `prev_hash`, `principal`, `api_key_id`, `at`, `payload` — so the
   displayed record and the signed record cannot diverge.
8. For a non-provider tenant, check the genesis payload's `created_event_id` names a
   `tenant.created` event in the provider chain whose payload `tenant_id` matches — which
   requires the provider chain (or that referenced event with its own verified linkage) in the
   bundle.
9. Check the stated chain head equals the last event's (`seq`, hash from step 4), and verify
   the head's signature.

The result names the verified head (tenant, seq, hash). What passing proves — and does not —
is stated in [audit-and-lineage](../architecture/audit-and-lineage.md): integrity and order of
the record under the deployment's key, not the truth of what agents wrote, and not which
process held the key beyond the journalled lead record. Ordering is proven by `seq` alone;
timestamps are informational ([storage](../architecture/storage.md#time)).

## Test vectors

A three-event chain — genesis, a submission, an erasure — with every byte shown. Digests and
signatures were computed over exactly these bytes (python3 `hashlib` and OpenSSL 3.5.5,
2026-10-29); the M0 reference implementation must reproduce them byte-for-byte.

**TEST KEY — never deployed.** This key pair exists only for these vectors. Its private half is
published here on purpose, which disqualifies it from ever signing a real chain.

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

`submission.remember` at seq 2; its `prev_hash` is vector 1's hash. The content digests are
real: the body plaintext is the 53 ASCII bytes
`The deploy runbook: drain, upgrade, verify, undrain.` + `\n`, so
`plaintext_digest` = SHA-256 of those bytes. The vector exercises the chain format, not a
cipher, so the "ciphertext" is a defined stand-in: the 32 bytes SHA-256(`vector-ciphertext:` +
body bytes), and `ciphertext_digest` is the SHA-256 of those 32 bytes — every value below is
reproducible from this paragraph. `signed_bytes` (one line, 870 bytes):

```json
{"api_key_id":"00000000-0000-4000-8000-0000000000c2","at":"2026-10-29T14:05:00.250000Z","event_id":"00000000-0000-4000-8000-0000000000e2","event_type":"submission.remember","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","attachments":[],"body_bytes":53,"ciphertext_digest":"sha256:505007c2183a32f9a29b6635dc1a56d89711b658d0624eb662dc986141213688","claim_key":"deploy-runbook","derives_from":[],"kind":"memory","plaintext_digest":"sha256:6e04a033aab31ee48e5bf89a4a6ef03e658f25f9b428de3e45c2ac7519dd2532","provenance":"agent","scan":{"result":"pass","scanner_version":"vector-scanner/1"},"simulation_id":null,"version":1,"workarea_id":"00000000-0000-4000-8000-0000000000b1"},"prev_hash":"sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a","principal":"crew-redline/agent-3","seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}
```

```text
hash = sha256:72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1
sig  = ed25519:068b0527c06897539882dfd22dc1970adbef83ca955559f80d529b76f7f26df64b14166b0a934eff9829704b3516f8a7b2c0bfa1a9a0ac529d1ec0e554756908
```

### Vector 3 — an erasure, verifying after the DEK is gone

`lifecycle.erase` at seq 3 erases vector 2's artifact; its DEK is destroyed. Nothing in this
vector — or in re-verifying vectors 1–2 — needs a DEK or any plaintext: the signed bytes carry
digests only (D1), so the full three-event chain verifies after shredding exactly as before.
That is the property the digest-only rule buys
([storage](../architecture/storage.md#digest-only-journal-payloads)). `signed_bytes` (one line,
550 bytes):

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

On 2026-10-31 these vectors were reproduced byte for byte outside Rust, by two RFC 8785 and two
Ed25519 implementations, and `scripts/check_chain_vectors.py` keeps them reproduced; the
[recorded finding](../engineering/spikes/chain-vectors.md) names the implementations and
versions.

## Decision record

This page is the byte-level annex of D1, registered as
[ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md) in the
[decision register](../adr/index.md); chain custody and the commit point remain declared in
[storage](../architecture/storage.md).
