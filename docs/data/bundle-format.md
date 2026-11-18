---
status: Stable
verified: 2026-11-17
answers: "the workspace bundle file oml-bundle/1: its layout, manifest, signature, encryption, the records it carries and never carries, every refusal a reader names, and how an import is planned; which parts are built and which are designed"
---

# Bundle format

A bundle moves a workspace's claims between servers
([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md)).
It is one file: a tar with a fixed layout, a manifest signed by the source's journal key, encrypted
with [age](https://age-encryption.org) to one or more named recipients.

| Part | State |
|---|---|
| The format: layout, manifest, signature, encryption, verification, the import plan (crate `oml-bundle`) | built |
| `POST /v2/workspaces/{workspace}/exports` in `history` mode | built ([#425](https://github.com/e8kor/open-memory-lab/issues/425)) |
| `release` mode and `since` | designed, not built: refused `request.validation`, `details.reason` `releases_unbuilt`, until releases are built |
| `oml workspaces export` | built |
| `POST /v2/imports`, `GET /v2/imports/{id}`, `oml workspaces import` | designed, not built ([#426](https://github.com/e8kor/open-memory-lab/issues/426)) |
| Export and Import on the console's personal side | designed, not built |

## Layout

The tar holds these entries, in this order, and no others. Every entry is a regular file.

```text
manifest.json         the manifest and its signature
workspaces.jsonl      one line per exported workspace
versions.jsonl        one line per claim version, or an erasure marker
verdicts.jsonl        one line per verdict on a version exported whole, never on an erasure marker
events.jsonl          the signed source event of each of the exporter's own versions
edges.jsonl           claim and brief edges whose two ends are exported
bodies/<sha256 hex>   a text over 64 KiB, and every attachment, by plaintext digest
```

The five JSON Lines files are present even when empty. Bodies follow in ascending digest order.
The tar is not compressed, and the same contents always make the same tar.

## Manifest

```json
{
  "format": "oml-bundle/1",
  "source": {"server_id": "…", "tenant_id": "…", "tenant_name": "acme"},
  "workspaces": ["universe.ada.notes"],
  "mode": "history",
  "release": null,
  "since": null,
  "dropped": [],
  "exported_at": "2026-11-17T09:00:00.000000Z",
  "exporter_pseudonym": "…",
  "event_keys": ["<journal public key, hex>"],
  "files": [{"path": "workspaces.jsonl", "bytes": 98, "lines": 1, "sha256": "sha256:…"}],
  "signature": {"alg": "ed25519", "key_fingerprint": "…", "public_key": "…", "sig": "ed25519:…"}
}
```

- `mode` is `release`, which carries one published release's promoted versions, or `history`,
  which carries one workspace's record. `history` is the default in a user root.
- `release` and `since` are `{label, manifest_digest}`. `since` makes an incremental bundle, and
  `dropped` names the claim keys the newer release no longer holds.
- `files` lists every entry after the manifest with its size, its line count (null for a body) and
  its SHA-256.
- The signature is Ed25519 over `oml-bundle/1\n` followed by the
  [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) form of the manifest without `signature`.
  The prefix keeps a manifest signature from being replayed as a journal event's, or the reverse.
  `key_fingerprint` is the lowercase hex SHA-256 of the public key, the value `--expect-key`
  names.

## Records

A version line carries the claim's text inline up to 64 KiB, or a `body` digest. It also carries
its kind, format, anchor, valid time, kind object, `derives_from` (only the ids the bundle exports
whole), `origin` exactly as signed, attachments by digest, state, and an `author` of
`{used_by, same_user, pseudonym}`.

A guardrail version that is not a retirement also carries `guardrail`, its `{severity, rule}` as
signed: `severity` a known severity and `rule`, when not null, an `oml-gr/1` rule the severity
admits. Every other line carries no `guardrail`. A reader refuses a guardrail line without it, or
any other line with it, as `bundle.malformed`, naming `guardrail_fields_missing` or
`guardrail_fields_unexpected`. The kind's other fields (a steering version's text, a term's or
vocabulary release's body) travel in the text.

A pseudonym is HMAC-SHA-256 of the source key id under a salt drawn for that bundle and never
exported. One author is recognisable as one author inside a bundle, and never across bundles. No
line of `workspaces.jsonl`, `versions.jsonl`, `verdicts.jsonl` or `edges.jsonl` names a key or a
person.

Only the exporter's own versions carry their signed source event. A signed event names its key
and principal, so `events.jsonl` names the exporter's principal and `api_key_id`, and nobody
else's. The reader enforces this. Every carried event has one `principal` and one `api_key_id`, is
a write (`submission.remember` or `submission.learn`), and names no reviewer, ratifier or proposer
key other than its own. The events of other people's versions and of verdicts stay on the source
server, and their integrity rests on the manifest signature.

An erased version travels as a marker, exactly `{"artifact_id": "…", "version": 1, "erased": true}`.
It carries no digest, so a holder of the bundle cannot confirm a guessed text.

Keys, grants, crews, quotas, review policy and bindings never travel.

## Refusals

A reader checks the structure before it trusts any content, then the digests, then the
signatures. The first failure refuses the whole bundle.

| Code | When |
|---|---|
| `bundle.format_unknown` | the manifest names a format other than `oml-bundle/1` |
| `bundle.layout` | an entry outside the layout, out of order, missing, or not a regular file; a referenced body missing |
| `bundle.path_unsafe` | an absolute path, a `..` or empty segment, a backslash, a symlink or a hard link |
| `bundle.too_large` | the unpacked size, the entry count or one entry over its limit |
| `bundle.line_too_long` | a line over 2 MiB |
| `bundle.depth` | JSON nested deeper than 32 |
| `bundle.duplicate` | an artifact id and version twice, a body twice, or an event id twice |
| `bundle.unreferenced_body` | a body no line names |
| `bundle.malformed` | a line that does not parse or breaks its closed shape, an `exported_at` outside `YYYY-MM-DDTHH:MM:SS.ffffffZ`, an erasure marker with content, a verdict on an erasure marker or on a version the bundle does not carry, an event on another author's version, or a foreign event |
| `bundle.digest_mismatch` | a file, a body or a version's text that does not hash to what the manifest or its event states |
| `bundle.signature_invalid` | the manifest or a carried event does not verify (strict Ed25519), an event names another tenant, or the manifest names a tenant other than the expected one |
| `bundle.signature_unexpected` | the manifest verifies under a key other than `--expect-key` |
| `bundle.unencrypted` | an export to no recipient or to a recipient that does not parse, or an import of a file that is not age-encrypted |
| `bundle.decrypt_failed` | the identity does not parse or is not a recipient of the file, the file is passphrase-encrypted, or its ciphertext does not authenticate |

A `history` export is assembled in the server's memory and walks the tenant's journal once for
the exported versions' submission events and verdicts, so its cost grows with the chain and the
subtree. A bundle larger than `uploads.max_bytes` is refused `export.too_large`, and one tenant
builds one export at a time (`export.busy`). Its bytes are counted on the exporter's `exports`
meter and on the tenant's. Throughput and a per-day export limit are unmeasured (ADR-0084 d15,
I13, I19).

The size limits follow the target's quota until measurement sets them. The CLI's defaults are
1 GiB unpacked, 100,000 entries and 64 MiB per entry.

Without `--expect-key`, a valid signature proves the bundle is intact and was made under the key it
names, but not who holds that key. The plan says which applies.

## Import plan

An import lands every version held, in an in-progress workspace, and plans each claim key against
the target before it writes:

| Outcome | When | Write |
|---|---|---|
| `new` | the target has no such key | held version |
| `changed` | the text differs, and the target tip is the last import's write | held supersession naming the tip's version |
| `removed` | the bundle retires the key, drops it, or a full release no longer holds it | held retirement |
| `unchanged` | the text is equal | none |
| `equivalent` | the claim's format finds the change equivalent | none |
| `conflict` | the target tip changed on the target since the last import, or never came from it | held supersession marked `conflict` |

A source's key is pinned at its first import. Once a target has imported from a source, a later
bundle claiming that source is refused as `import.source_key_changed` in two cases: it is signed by
another key, or it is read without `--expect-key`. A bundle cannot borrow a source's ids to retire
what an earlier import wrote.

A bundle older than the target's last import from that source is refused as `import.bundle_older`,
unless the import passes `--allow-older`. An incremental bundle whose base is not the last release
imported is refused as `import.since_mismatch`. An imported version's idempotency key is
`import:<target workspace id>:<source server id>:<source tenant id>:<artifact id>:<version>`, so
importing the same bundle twice writes nothing the second time.

## Test vectors

`crates/shared/oml-bundle/tests/vectors/` holds:

- a small signed bundle;
- its age-encrypted form;
- `TEST-ONLY-age-identity.hex`, the 32 raw bytes of the throwaway identity that opens it, from
  which the tests build the `AGE-SECRET-KEY-1…` string at run time;
- the fingerprint of the test signing key (seed `[42; 32]`).

These are test material only, never real keys.
