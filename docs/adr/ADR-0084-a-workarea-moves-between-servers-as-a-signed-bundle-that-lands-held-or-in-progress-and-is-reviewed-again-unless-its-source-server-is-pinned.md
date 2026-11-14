<!--
id: adr/ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned
tier: engineering
status: proposed
answers: what a workarea export carries and what it never carries; how a bundle is signed, encrypted, verified and streamed; how authors and reviewers who do not exist on the target are represented; whether an imported promoted claim arrives promoted or is reviewed again, and what a pinned source server changes; what happens when a workarea name or a claim key already exists on the target, and how a repeated import adds only what is new; who may export and who may import, given R1; where an import lands; how import and export keep R4, including erasure after a copy exists; which verbs, commands, console pages, journal events, doctor lines and meters it adds; how it relates to oml-backup and to a tenant move; and the questions left for the operator
-->

# ADR-0084 — A workarea moves between servers as a signed bundle that lands held or in progress and is reviewed again unless its source server is pinned

**Status:** proposed · 2026-11-14

**Release:** not scheduled; depends on workarea releases (see the
[workarea lifecycle review](https://github.com/e8kor/open-memory-lab/blob/main/design/workarea-lifecycle.md), Stage 4), which is a separate record still to be written

## Context

On 2026-11-14 the operator asked for a development lifecycle in which a workarea is developed,
tested, verified "and then promote[d] to production", and answered that promotion means both the
same-server path (development, in progress, commit, validated) and a move between servers: staging
to production, backup of one workarea, and sharing between tenants. The
[workarea lifecycle review](https://github.com/e8kor/open-memory-lab/blob/main/design/workarea-lifecycle.md)
walks the whole cycle; this record decides the move.

Nothing moves a workarea today. The person export `GET /v2/web/export`
(`crates/oml-app/src/app/signup.rs:678`) is a JSON dump of a person's root with no import, no
signature and no verdicts. `oml guardrails export` writes rule text for another harness
(ADR-0037 d11). `oml-backup`, in the operator's private repository, is a whole-database `pg_dump`
restored only into an empty cluster; restoring it rewinds the chain and can bring back erased
content (`docs/operations/restore.md:32`). ADR-0011 d8 and ADR-0047 d3 mention exports without
deciding a format.

The forces:

- **R1** ([ADR-0001](ADR-0001-the-constitution.md) d1): an import that wrote promoted rows would be
  a promotion, so it is a person's act or it promotes nothing. An admin key never recalls claim
  text, so it may not export it either.
- **R2** (ADR-0001 d2): the reviewers on the source are keys the target has never seen. The target
  cannot check the lineage closure against them.
- **R3** (ADR-0001 d3, d5; [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md) d3, d7):
  a held row leaves no in-progress workarea, and a bundle is a way out.
- **R4** (ADR-0001 d4; [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d2): import and export
  delete nothing, and a copy on another server is beyond the reach of an erasure on the source.
- Names are unique, immutable and never released ([ADR-0026](ADR-0026-names-are-keys.md) d1, d10);
  claim keys are unique per workarea (`migrations/0005_claim_versions.sql:159`).
- Every write is scanned ([ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d1),
  metered in its own transaction ([ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d3),
  and signed by the lead ([ADR-0073](ADR-0073-a-leader-signs-every-write-and-nodes-serve-the-workareas-placed-on-them-each-read-routed-to-the-closest-less-busy-replica-fresh-enough-for-it.md) d1).

Decided, not built.

## Decision

- **d1 — A bundle is a directory of JSON Lines files and content-addressed bodies, named by a
  signed manifest, in the format `oml-bundle/1`.** Layout:

  ```text
  manifest.json            format, source, scope, counts, file digests, signature
  workareas.jsonl          one line per workarea in the exported subtree: name, path, status, mode, parent
  versions.jsonl           one line per claim version (d2)
  verdicts.jsonl           one line per verdict on an exported version (d4)
  events.jsonl             the source journal events for every exported version and verdict, as signed
  edges.jsonl              claim edges and brief edges whose both ends are exported
  bodies/<sha256>          a version's body when it exceeds 64 KiB, and every attachment, by plaintext digest
  ```

  The manifest names the format version, the source server's identity (its journal signing public
  key and a server id), the source tenant and workarea paths, the export mode (d2), the release or
  the `since` cursor (d8), the export instant, a count per file, and each file's SHA-256. JSON
  Lines streams: an export writes and an import reads one line at a time, so neither holds the
  bundle in memory. A tar of the directory is the transport form. A reader refuses a format version
  it does not know, by name.

- **d2 — Two modes: `release`, the default, carries the promoted versions of one release, and
  `history` carries every version.** `release` exports exactly the manifest of a named workarea
  release (the review's Stage 4), with each version's lineage ids but not the superseded versions
  themselves; it is the promotion path. `history` exports every version of every claim in the
  subtree that the exporter may read (d10): held ones the exporter could review, promoted,
  superseded, rejected, retired and demoted versions, each with its status. It is the backup and
  move path. Erased versions are carried as a marker with their digests and no bytes, since the
  bytes no longer exist (ADR-0005 d2). Simulation rows keep their mark (ADR-0011 d8). Held versions
  of an in-progress workarea are exported only in `history` mode and import held (d6).

- **d3 — Claims, anchors, skills, guardrails, documents, steering and terms ride as versions;
  grants, keys, crews and settings never do.** Every claim kind travels as its versions, with its
  format, anchor (ADR-0057 d1), valid time (ADR-0059 d1), kind fields and `derives_from`. Attachments
  travel under `bodies/` by plaintext digest (ADR-0004 d2). Never carried: keys, connect codes,
  connections, grants, denies, crews, quotas, the workarea's `mode` and `accept_federated`, review
  policy, and anything in `artifact_keys`. The target decides all of those. Bodies are exported in
  plaintext, decrypted under the source's keys; d5 protects them in transit.

- **d4 — Authors and reviewers travel as provenance, pseudonymous and never named; the importer is
  the author on the target.** Each version and verdict carries the source key's `held_by` kind,
  `same_user`, `approved_at`, and a pseudonym: HMAC-SHA-256 of the source key id under a per-server
  secret, so lines by one key are recognisable as one author within one source without naming it.
  This keeps ADR-0077 d2's rule that a reader never learns the reviewer. On the target, every
  imported version is a new write by the importing key, journalled with
  `origin: {kind: "import", server, tenant, workarea, artifact_id, version, digest}`. R2 on the
  target is therefore checked against the importer (d6). The honest limit, in the manner of
  ADR-0001 d6: a person who wrote a version on the source and holds a dreamer key on the target is
  not recognisable there, and could approve their own work on the target. The bundle cannot close
  this without carrying a person's identity across servers.

- **d5 — The manifest is signed by the source server's journal key under a domain-separation
  prefix; the bundle is encrypted to named recipients.** The signature is Ed25519 over the RFC 8785
  canonical manifest prefixed with `oml-bundle/1\n`, so a manifest signature can never be replayed
  as a journal event signature or the reverse (`crates/oml-domain/src/envelope.rs:375`,
  `crates/oml-domain/src/chain.rs:105`). Export requires at least one age recipient public key and
  writes the tar encrypted to them; the server never holds a recipient's identity, and the CLI
  decrypts locally before upload (d12). An unencrypted bundle is refused at export.

- **d6 — An import lands in an in-progress workarea and every imported version is held.** The
  importer names a target workarea that is `development` and in progress, or a new name that the
  import creates in progress under the importer's root or a parent where the importer may create
  workareas (ADR-0044 d6, ADR-0030 d1). Each imported version is written held by the importing key.
  The keys connected there read it marked unreviewed (ADR-0030 d3), so the target's agents, tests
  and impact report (review Stages 5, 6) run against it before anything is served outside. It
  leaves only by review on the target and the commit (ADR-0030 d6). The reviewer is shown the
  source verdict as evidence (kind, `same_user`, `approved_at`), and ADR-0055 lets one person judge
  the listed set in one act, each item its own verdict. This is the default, and it is the only
  mode until the operator decides d7.

- **d7 — A pinned source server is an option, decided by the operator, under which verified source
  verdicts count on the target.** A target tenant may pin a source: a server id, its bundle public
  key, and the source workarea path, proposed by one person's dreamer key and ratified by another's
  (as ADR-0009 d3 ratifies a transition), journalled as `import.source_pinned`; never by an admin
  key (R1). An import from a pinned source, by a person's key, writes each version whose source
  verdict verifies (the event signature in `events.jsonl` checks against the pinned key, the
  verdict's key is `held_by: human` and not `same_user`, its reviewed digest equals the version's)
  as promoted, marked `approver: "imported"` with the pin's id, and still into an in-progress
  workarea, so the commit by a person remains the act that serves it. A version that fails any
  check lands held as under d6. A skill or a guardrail never arrives promoted, even from a pinned
  source, because ADR-0080 d3 requires a person's verdict on the server that serves it. A rotated
  source key breaks the pin until it is pinned again.

- **d8 — A re-import adds only what is new: each version is imported once, by its source identity,
  and compared with the target's tip by digest.** The idempotency key of an imported version is
  `import:<server id>:<source artifact id>:<version>`; a second import of the same bundle writes
  nothing. `--since <release>` exports only the versions after that release, and the target keeps a
  cursor per pinned or named source. Each claim key is planned as `oml sync` plans
  (ADR-0057 d8): `new` (written held), `changed` (a held supersession of the target tip with
  `expected_version`), `removed` (a held retirement), `unchanged` or `equivalent` (nothing written),
  and `conflict` when the target tip was changed on the target since the last import (a held
  supersession marked `conflict`, never written over). `oml workarea import --dry-run` prints the
  plan and writes nothing.

- **d9 — A name that exists is never reused or overwritten.** A new target workarea whose name is
  taken answers `name.taken` (ADR-0026 d5); importing into an existing workarea is a merge under
  d8, allowed only when the workarea is in progress. The source workarea's name is carried as
  provenance and is not required to match. A skill name or a document key that would be shadowed
  or would shadow in the target's chains is reported in the plan (ADR-0080 d5).

- **d10 — A person's dreamer key exports, with reader access on every node of the subtree; no agent
  key, admin key or service key exports.** An export is a bulk read of claim text out of the
  server, so it belongs with the acts ADR-0030 d1 and ADR-0055 d1 already keep for a person
  (`crates/oml-domain/src/in_progress.rs:174`). An admin key reads structure, never text
  (ADR-0026 d15), so it cannot export a bundle. A node the exporter cannot read refuses the whole
  export by name rather than exporting a partial subtree. `history` mode carries held versions of
  other authors only where the exporter's key would see them in its review list. An export changes
  nothing on the source and is journalled as `workarea.exported` with the manifest digest, the mode,
  the counts and the recipients' fingerprints.

- **d11 — A person's dreamer key imports; the scan runs on every version; the plan must fit the
  target's quotas before the first write.** Each version passes the target's scan rule set
  (ADR-0074), which may be newer than the source's: a refusal skips that version and names it in
  the outcome, a hold keeps it author-only as ADR-0074 d20 says. Before writing, the import checks
  the plan against the target account's meters (`memories`, `text_bytes`, `writes`; ADR-0071 d2)
  and refuses as a whole if it does not fit. Writes go in batches, each its own transaction under
  the chain lock with its meters (ADR-0071 d3), and an interrupted import resumes from its cursor
  because d8 makes every write idempotent. The import is journalled as `workarea.imported` with the
  source manifest digest, the trust mode and the outcome counts.

- **d12 — An import verifies the bundle before it writes.** In order: the format version; each
  file's SHA-256 against the manifest; the manifest signature against the key in the manifest,
  and under d7 against the pinned key; each version's plaintext digest recomputed and compared with
  the digest in its source event; each source event's signature. A failure refuses the whole
  import, names the file and line, and writes nothing. Without a pin, the signature proves the
  bundle is intact and came from the key it names, not who holds that key; the plan says so.

- **d13 — Import and export never delete; a source erasure reaches a copy as a notice the target
  acts on.** An import writes only held versions, held supersessions and held retirements (d6, d8);
  nothing on the target is removed or overwritten. An export reads. A version erased on the source
  after a bundle was made survives in the bundle and in the target, which the source cannot reach.
  A later bundle carries the erasure marker (d2); the target records it as an erasure notice
  against the imported version, the doctor counts open notices, and the target's own erasure act
  (ADR-0005 d2, an admin verb under ADR-0010 d6) clears it. The bundle files themselves are outside
  every server's erasure: the age encryption of d5 and the recipient's custody are the bound, and
  that is said in the export's output.

- **d14 — The surfaces are HTTP, the CLI and the personal console; there is no MCP tool.**
  - HTTP: `POST /v2/workareas/{workarea}/exports {mode, release | since, recipients}` streams the
    encrypted tar; `POST /v2/imports {target, dry_run}` takes a decrypted bundle as a resumable
    upload (ADR-0004 d3) and answers the plan or the outcome; `GET /v2/imports/{id}` reports a
    running import; `POST /v2/import-sources` and `.../ratify` pin a source (d7).
  - CLI: `oml workarea export <workarea> [--release <label> | --since <label> | --history]
    --recipient <age public key>... --out <file>`; `oml workarea import <file> --into <path>
    --identity <age identity file> [--dry-run]`; `oml workarea sources pin|ratify|list`.
  - Console: Export on a workarea's page and an Import page, on the personal side only.
  - No MCP tool, as ADR-0046 d3 kept edges off MCP: an agent neither exports nor imports (R1).
  - Journal: `workarea.exported`, `workarea.imported`, `import.source_pinned`,
    `import.erasure_notice`; each imported version's own write events carry `origin`.
  - Doctor: bundle verification failures by reason; imports stopped mid-way with their cursor; pins
    whose source key changed; open erasure notices.
  - Metering: imported versions count as writes, memories and text bytes; an export is counted in
    the request log and adds no meter.

- **d15 — A same-server import is the development copy and the return path.** The source may be a
  workarea on the same server: `oml workarea import --from <workarea>@<release> --into <path>`
  builds the bundle in memory, skips encryption, and adds a `derives_from` edge from each imported
  version to its source version, since both are on one journal. This is how a person develops
  against a copy of a live workarea and brings the result back (review Stage 2), with the same
  review on the way back as any import.

- **d16 — This is not a backup and not a tenant move.** `oml-backup` restores a whole database,
  every tenant, every key and the chain, and is an operator's act. A bundle is one workarea subtree,
  logical, made by a person, with no keys and no grants. A tenant move between servers needs keys,
  grants, crews, accounts and quotas carried, which d3 refuses; it is a later record that may use
  this format for its claims.

## Consequences

- The workarea releases record comes first: `release` mode exports a release manifest. Until it
  exists, `history` mode is the only buildable mode.
- `oml-bundle/1` is a stable format with a published schema under `docs/reference/`, and a change
  to it is a new format version.
- `origin.kind` gains `import` (ADR-0063 d1's list).
- The review list (ADR-0055 d2) shows a held imported version's source verdict as evidence.
- New journal events, error codes (`bundle.format_unknown`, `bundle.digest_mismatch`,
  `bundle.signature_invalid`, `bundle.unencrypted`, `import.over_quota`, `import.target_not_in_progress`,
  `export.not_readable`), doctor lines and a migration for import cursors, pins and erasure notices.
- Bundle sizes, export and import throughput, and the batch size are unmeasured; tracked as an
  issue to be filed with the build.
- Operator questions, recommendation first:
  1. **R2 on import:** re-review on the target always (recommended for now), or allow a pinned
     source (d7)?
  2. **Who exports:** a person's key only (recommended), or also a CI service key under a new
     `exporter` profile, for scripted staging-to-production moves?
  3. **Mandatory encryption:** always (recommended), or allow a plaintext bundle for a same-host
     move?
  4. **`history` mode for held versions of other authors:** allowed where the exporter could review
     them (recommended), or never?
  5. **Carrying a person's identity across servers** to close d4's honest limit: no (recommended),
     or an opt-in sign-in identity for moves inside one organisation?

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Imported promoted versions arrive promoted, with no pin** | The target would serve content no identity it knows reviewed, on the word of any server that can sign a manifest. R2 could not be checked, and anyone able to upload a bundle could promote. |
| **The importer re-signs and approves in one act** | The importer is the author on the target (d4); an approval by the same key is R2's exact case. |
| **Only promoted tips, never history** | Backup and moves need rejected and superseded versions, which R4 keeps; a tips-only format would lose them on the second server. `release` mode keeps the tips-only path. |
| **A `pg_dump` of the tenant's rows** | Moves keys, grants and the chain, cannot merge into an existing tenant, rewinds erasure on restore (`docs/operations/restore.md:32`), and needs `BYPASSRLS`. That is the backup's job, not a person's. |
| **An admin key exports** | An admin key never reads claim text (ADR-0026 d15, ADR-0010 d6); a bundle is claim text. |
| **An agent exports or imports through MCP** | A bulk egress or a bulk write the agent starts is the gate called by the thing gated (R1); the scan cannot see where the bundle goes. |
| **Import into a standard workarea, held** | Held rows there are author-only (R3), so the target's agents and tests could not run against the import before it is served. An in-progress workarea gives the staging step for free. |
| **Sign the manifest with a new export key** | A second key per server to rotate, back up and pin, for no gain over domain separation under the journal key the target would pin anyway. |
| **One bundle file in a custom binary format** | JSON Lines and content-addressed files are readable with standard tools, stream line by line, and diff between two exports. |
