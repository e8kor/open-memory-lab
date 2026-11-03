<!--
id: engineering/spikes/chain-vectors
tier: engineering
status: recorded finding · 2026-10-31 — the chain-format test vectors reproduce outside Rust
answers: whether the chain-format test vectors (RFC 8785 canonical bytes, SHA-256 hashes, RFC 8032 Ed25519 signatures, the genesis event) reproduce byte for byte in independent non-Rust implementations, with which libraries and versions, whether the Rust tests' own copies of the vectors agree with the page, and what the cross-check found on the page itself
-->

# Finding: the chain-format test vectors, cross-checked

**Status:** recorded finding · 2026-10-31 — the chain-format test vectors reproduce outside Rust

This is the finding for the critical path's M0 exit item: the vectors that
[chain format](../../data/chain-format.md#test-vectors) states, cross-checked against
independent implementations so that they are corroborated rather than self-confirmed. The
harness and the reason it uses two implementations of each primitive are on the
[M0 spikes](index.md) page.

## The finding

```text
spike-finding: M0 exit, chain-format test vectors
date:          2026-10-31
versions:      Python 3.13.13 (uv-managed CPython), uv 0.11.6; rfc8785 0.1.4; jcs 0.2.1;
               cryptography 50.0.2 (statically linked OpenSSL 4.0.3, 29 Sep 2026);
               PyNaCl 1.6.2 (bundled libsodium; the binding does not expose its version);
               hashlib openssl_sha256 (OpenSSL 3.5.5, 27 Jan 2026); all pinned in
               scripts/check_chain_vectors.py.lock
settings:      none; inputs and stated values as docs/data/chain-format.md holds them on
               2026-10-31
environment:   Linux 7.0.0-38-generic x86_64, 8 vCPU AMD EPYC-Genoa, 15.2 GiB RAM; no
               Rust toolchain involved
verdict:       pass
adr-clause:    ADR-0002 d3 (per-tenant chain: seq, prev_hash, Ed25519 signature), through
               chain format, its byte-level annex
action:        none for the vectors; one sentence of chain format corrected (below), one
               wording question left for the maintainer (below), and the Rust tests'
               copies of the vectors compared with the page (below)
```

## Evidence

`uv run --locked --script scripts/check_chain_vectors.py` printed:

```text
check_chain_vectors: OK: docs/data/chain-format.md: 3 vectors reproduce byte for byte (508, 870, 550 bytes); steps 1-7 and 9 pass; 11 mutation controls fail as the page states; the Rust tests' copies in 3 files (6 signed_bytes literals, 34 hex values) equal the page
```

What that line covers:

- **Canonical bytes.** For each of the three vectors, `rfc8785` and `jcs` each rebuild
  `signed_bytes` from the nine signed fields. The inputs are given in a non-canonical member
  order, and both outputs equal the page's bytes, at the page's byte counts (508, 870, 550).
  Each field the script holds equals the value in the page's bytes.
- **Derived values.** Vector 2's `body_bytes` (53), `plaintext_digest` and `ciphertext_digest`
  are computed from the body sentence and the stand-in rule the page states. The genesis
  `prev_hash` is computed from 32 zero bytes, and each later `prev_hash` from the hash the
  script computed for the previous vector. None of them is copied from the page.
- **Hashes.** SHA-256 by `hashlib` and SHA-256 by libsodium agree with each other and with the
  stated `hash` of every vector, and with the stated key fingerprint
  `d92de0b4…2c1676`.
- **Key.** The private PEM read as RFC 8410 PKCS#8 gives the stated raw public key under
  `cryptography`, and the 32-byte seed gives the same key under PyNaCl. The public PEM decodes to
  the same 32 bytes.
- **Signatures.** Pure Ed25519 is deterministic, so `cryptography` and PyNaCl each sign each
  vector's bytes with the test key, and each produces exactly the stated `sig`. Each also
  verifies the stated `sig` under the stated public key.
- **Procedure.** Steps 1–7 and 9 of the [verification procedure](../../data/chain-format.md#verification)
  pass over the three events. Step 8 needs a provider chain, which the vectors leave out, as the
  page says.
- **Mutations.** One flipped byte in each event fails steps 3 and 4. Each of the five
  reorderings fails step 1 or step 5. Omitting event 1 or event 2 fails step 1 or step 5.
  Omitting event 3 fails step 9 alone.
- **Tampered copies.** The check failed, naming the disagreement, on eight copies of the page,
  each with one change: a principal, a hash, a signature, a byte count, the public PEM, the
  private PEM, the body sentence, and the member order of vector 1's payload.
- **The Rust tests' copies.** `crates/oml-domain/tests/vectors.rs`,
  `crates/bins/oml-cli/tests/vectors.rs` and `crates/oml-domain/tests/keys_and_format.rs` hold 6
  `signed_bytes` literals and 34 hex values between them, and every one equals the page. The
  harness decision is on the [M0 spikes](index.md#the-rust-tests-copies-are-compared-with-the-page)
  page.

## The Rust tests' copies

The Rust tests keep their own copies of the vectors, and each Rust test checks the Rust code
against its own copy. Until 2026-10-31 nothing compared those copies with the page. They agreed,
checked by hand against the page on that date, but a copy that drifted would have left the Rust
tests and this check both green. The first version of this finding did not say so. Run from a
scratch tree holding the page, the script and the three files, the check as first committed
passed with vector 3's `principal` changed in the CLI test's copy. The check as it stands
compares the copies. With `--rust-root` pointed at a scratch copy of the three files, it failed,
naming the file, the line and the disagreement, on six copies, each with one change:

| Change in the scratch copy | What the check printed |
|---|---|
| vector 3's `principal` in the CLI test's `signed_bytes` | `crates/bins/oml-cli/tests/vectors.rs:31: the copy of vector 3's signed_bytes differs from the page's; first difference at byte 488` |
| one digit of `V2_HASH` in the domain test | `crates/oml-domain/tests/vectors.rs:27: 72f71452f67a618a… (64 hex digits) is no value the page states or this script derives from it` |
| one digit of vector 1's `sig` in the CLI test | `crates/bins/oml-cli/tests/vectors.rs:22: 3d22e7ce20b6db79… (128 hex digits) is no value …` |
| one digit of vector 1's hash in `keys_and_format.rs` | `crates/oml-domain/tests/keys_and_format.rs:53: c3f4bfb7ef9e00df… (64 hex digits) is no value …` |
| one digit of the domain test's `TEST_SEED_HEX` | `crates/oml-domain/tests/vectors.rs:16: 09884594afc93c44… (64 hex digits) is no value …` |
| vector 2's `signed_bytes` as an escaped string in a new file, `crates/x/src/lib.rs` | `crates/x/src/lib.rs:1: vector 2's signed_bytes appear outside a raw string literal, where this check does not compare them` |

On a scratch tree with no copy, the check passed and printed `no Rust file holds a copy of the
vectors`. The scratch trees were removed afterwards. The check reads `crates/` and changes nothing
there. Having the Rust tests read the page, so that no copy remains, is a follow-up for
`feat/iteration-3`, which is rewriting `crates/`.

## What the cross-check found on the page

- **An omitted last event fails step 9, not step 1 or step 5.** The page said "any reordering
  or omission fails step 1 or step 5". The first run of the check failed on exactly that claim:
  `omitting vector 3 fails steps [9], not 1 or 5`. A chain cut short at its tail is still
  contiguous and correctly linked, and only the stated head, which comes from outside the chain,
  shows that it is short. The sentence in [chain format](../../data/chain-format.md#test-vectors)
  now says so. This is a page edit and leaves no decision changed: the procedure already had
  step 9.
- **RFC 8785 orders members by UTF-16 code units, and the page says "by code point".**
  [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) §3.2.3 sorts property names by their UTF-16
  code units. The two orders differ when a name holds a character above U+FFFF and another holds
  one in U+E000–U+FFFF. Both libraries canonicalize `{"": 1, "\U0001F600": 2}` as
  `{"\U0001f600":2,"":1}`, while code-point order would put `` first. Every member
  name in the vectors is ASCII, and there the two orders agree, so the vectors cannot tell them
  apart. This finding does not check which order the Rust canonicalizer in `crates/oml-domain`
  uses. Correcting the page's wording, and adding a vector with such a name, wait on that check
  and on the maintainer.

## How independent this is

- The page's signatures came from OpenSSL 3.5.5, and `cryptography` also links OpenSSL, a
  different release (4.0.3) of the same project. PyNaCl calls libsodium, which shares no code
  with OpenSSL or with the Rust implementation's `ed25519-dalek`. The signature result rests on
  libsodium agreeing.
- The page's hashes came from `hashlib`, and this run's `hashlib` calls OpenSSL 3.5.5. libsodium's
  SHA-256 is the independent hash here.
- `rfc8785` (Trail of Bits) and `jcs` (Anders Rundgren's reference code, Rundgren being an author of
  RFC 8785) are written independently of each other and of the Rust canonicalizer.
- The comparison of the Rust tests' copies shows only that the copies equal the page. That the
  Rust code reproduces its copies is what `cargo test` in `ci.yml` checks, and this finding does
  not run it.
- CI job `chain-vectors` in `.github/workflows/m0-spikes.yml` re-runs the check on every pull
  request. That job has not run yet: this finding comes from the local run above.
