<!--
id: adr/ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned
tier: engineering
status: accepted design · 2026-11-14
answers: what a workarea export carries and what it never carries; the bundle's fixed layout, signature, encryption and hard limits, each with its refusal code; how authors and reviewers who do not exist on the target are represented; where an import lands and why every imported version is held; how a repeated import adds only what is new and refuses an older bundle; what happens when a name or claim key exists on the target; who may export under which tenant policy and who may import; how plaintext is staged; how import and export keep R4, and why an erasure marker reveals nothing; the flow for a new shared target; the same-server copy; which verbs, commands, events, doctor lines and meters it adds; and how it relates to oml-backup and to a tenant move
-->

# ADR-0084 — A workarea moves between servers as a signed bundle that lands held or in progress and is reviewed again

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with its four questions decided at their
recommendations and every other default as drafted · proposed the same day · the "unless its source server is
pinned" of its file name is ADR-0088, which is deferred

**Release:** not scheduled; after ADR-0085, whose release manifest `release` mode exports. A
trusted source server, the "unless" of the title, is the deferred ADR-0088.

**Twin parts** (ADR-0090 d18, PR #388): the import ledger, the erasure notices and the `export`
policy are journal or part, never excluded. The per-import staging keys are excluded: they are
transient and destroyed when the import ends, so a rebuild needs none. The build names each
column's class under ADR-0090 d4.

**Amends:** ADR-0071 (d2: an `exports` meter), ADR-0005 (the erasure inventory gains the class
"import staging", d11), ADR-0063 (d1: `origin` is kept verbatim and an imported version gains a separate
`import_provenance`), ADR-0055 (d2: the review list shows a held imported version's source verdict
as evidence), ADR-0030 (d1: an import is a write into an in-progress workarea and adds no reader
of held rows)

## Context

On 2026-11-14 the operator asked for a development lifecycle in which a workarea is developed,
tested, verified and then promoted to production. "Promoted to production" means two things: the
same-server path, and a move between servers (staging to production, and sharing between
tenants). The
[workarea lifecycle review](https://github.com/e8kor/open-memory-lab/blob/main/design/workarea-lifecycle.md)
walks the whole cycle. ADR-0085 decides releases and binding, which carry the same-server path,
including team delivery through a shared development workarea and its releases. This record decides
the move.

Nothing moves a workarea today. The person export `GET /v2/web/export`
(`crates/oml-app/src/app/signup.rs:678`) is an unsigned JSON dump of a person's root, and nothing
imports it. `oml-backup`, in the operator's private repository, is a whole-database dump that
restores only into an empty cluster. A restore rewinds the chain and can bring back erased content
(`docs/operations/restore.md:32`).

The forces, from [ADR-0001](ADR-0001-the-constitution.md):

- **R1:** an import that served content would be a promotion, so an import promotes nothing. An
  admin key reads claim text one journalled read at a time, with a reason
  ([ADR-0089](ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)
  d10); a bundle is bulk egress, not a read.
- **R2:** the source's reviewers are keys the target cannot see.
- **R3:** a held row leaves no in-progress workarea, and a bundle is a way out.
- **R4:** import and export delete nothing, and a copy is beyond the reach of a source erasure
  ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d2).

Two more constraints apply. Names are unique and never released
([ADR-0026](ADR-0026-names-are-keys.md) d1, d10). Every write is scanned
([ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d1)
and metered in its own transaction
([ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d3).
Decided, not built.

## Decision

- **d1 — A bundle is a tar with a fixed layout, in the format `oml-bundle/1`.** The tar is
  encrypted to its recipients (d5). Its entries appear in this order, and no other entry is
  allowed:

  ```text
  manifest.json            format, source server id, source tenant id and name, workarea paths,
                           mode, release or since cursor, export instant, count and SHA-256 per
                           file, signature
  workareas.jsonl          one line per exported workarea: name, path, status, parent
  versions.jsonl           one line per claim version (d2, d3)
  verdicts.jsonl           one line per verdict on an exported version (d4)
  events.jsonl             each exported version's and verdict's source journal event, as signed
  edges.jsonl              claim and brief edges whose two ends are exported
  bodies/<sha256>          a version's body over 64 KiB and every attachment, by plaintext digest
  ```

  The files are JSON Lines, so export writes and import reads one line at a time. A reader refuses
  an unknown format version as `bundle.format_unknown`.

- **d2 — `release` mode, the default, carries one published release; `history` mode carries one
  workarea's record.**
  - **`release` mode** exports exactly the manifest of a release (ADR-0085 d1): the promoted
    versions with their lineage ids, without the superseded versions themselves.
  - **`history` mode**, for other people's claims, carries every version that was ever promoted,
    including those superseded after promotion. For the exporter's own claims (those of the
    exporting principal) it also carries held, rejected and demoted versions. Other people's held,
    rejected and demoted versions never leave the server.
  - Simulation rows keep their mark (ADR-0011 d8).
  - An erased version is carried as a marker, under d13.
  - `history` is for moving or archiving one workarea's record. It is not a backup (d16).

- **d3 — Claims of every kind ride as versions; grants, keys, crews and settings never do.**
  Memories, steering, terms, guardrails, skills, documents and tests travel as versions. Each
  version carries its format, its anchor (ADR-0057 d1), its valid time (ADR-0059 d1), its kind
  fields, its `derives_from`, and its `origin` verbatim as signed (ADR-0063 d1). Attachments travel
  under `bodies/` (ADR-0004 d2). Never carried: keys, connect codes, connections, grants, denies,
  crews, quotas, `mode`, `accept_federated`, review policy, bindings, release gate records, and
  anything in `artifact_keys`.

- **d4 — Authors and reviewers travel as provenance under per-bundle pseudonyms; the importer is
  the author on the target.**
  - Each version and verdict carries the source key's `held_by`, `same_user` and
    `approved_at`, plus a pseudonym: HMAC-SHA-256 of the source key id under a salt drawn fresh for
    each bundle and never exported. Inside one bundle an author is recognisable as one author;
    across bundles, never. A reader never learns who the reviewer was
    ([ADR-0077](ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md) d2).
  - On the target, every imported version is a new write by the importing key. Its `origin` is the
    source's, verbatim. A separate `import_provenance` object holds the source server id, tenant
    id, workarea, artifact id, version, digest and pseudonyms. This amends ADR-0063 d1, which had
    no place for it.
  - R2 on the target is checked against the importer.
  - The honest limit, in the manner of ADR-0001 d6: a person who wrote a version on the source and
    holds a dreamer key on the target is not recognisable there, and could approve their own work.
    So a reviewer approving an imported version attests "I did not author this on the source",
    journalled with the verdict (`attests_not_source_author: true`); an opt-in link of a person
    across servers is later work (the operator, 2026-11-14).

- **d5 — The manifest is signed by the source's journal key under a domain prefix; the bundle is
  encrypted to named recipients.**
  - The signature is Ed25519 over the RFC 8785 canonical manifest prefixed with `oml-bundle/1\n`.
    The prefix means a manifest signature can never be replayed as a journal event signature, or
    the reverse (`crates/oml-domain/src/envelope.rs:375`, `crates/oml-domain/src/chain.rs:105`).
  - The journal key is deployment-wide (ADR-0073 d1), so the signed manifest names the source
    tenant and verification checks it.
  - **Authenticity.** An import may name `--expect-key <fingerprint>`, the source key's
    fingerprint obtained out of band; a manifest signed by another key is refused
    `bundle.signature_unexpected`. Without it, the signature proves integrity only and
    authenticates nothing until ADR-0088; the plan and the `workarea.imported` event say which.
  - Export requires at least one age recipient and writes the tar encrypted to it; without one the
    export is refused as `bundle.unencrypted`. The server never holds a recipient's identity; the
    CLI decrypts on the importer's machine.

- **d6 — An import lands in an in-progress workarea, and every imported version is held.**
  - The target is a `development` workarea in progress, or a new name the import creates in
    progress where the importer may create workareas (ADR-0044 d6). Any other target is refused as
    `import.target_not_in_progress`.
  - Each version is written held by the importing key. The `tip` keys connected there read it
    marked unreviewed (ADR-0030 d3), and the target's tests and impact report run against it
    (ADR-0086, ADR-0087). The import adds no reader of held rows beyond ADR-0080 d16's in-progress audience of the target.
  - Imported content reaches anyone else only through review on the target and a release there
    (ADR-0085). The review list shows the source verdict as evidence (amends ADR-0055 d2). One
    person may judge the listed set in one act, each item its own verdict.
  - Edges import as proposals by the importer, ratified on the target like any edge
    (ADR-0046 d4, d5).
  - A trusted source server, under whose verdicts a version may land promoted, is ADR-0088 and is
    deferred.

- **d7 — An import verifies before it writes, and the bundle has hard limits, each with a code.**
  - Before decrypted content is parsed, the importer's CLI and the server each check:

    | Check | Refusal |
    |---|---|
    | an entry outside the fixed layout or out of order | `bundle.layout` |
    | a path with `..`, an absolute path, a symlink or a hard link | `bundle.path_unsafe` |
    | decompressed size or file count over its limit | `bundle.too_large` |
    | a line over 2 MiB, twice the 1 MiB bound on a field (ADR-0065 d9) | `bundle.line_too_long` |
    | JSON nesting deeper than 32, more than any `oml-bundle/1` object needs | `bundle.depth` |
    | a duplicate artifact id and version, or a duplicate `bodies/` entry | `bundle.duplicate` |
    | a body no line references | `bundle.unreferenced_body` |

  - Then, in order:
    - the file digests (`bundle.digest_mismatch`);
    - the manifest signature and its source tenant (`bundle.signature_invalid`);
    - each version's recomputed plaintext digest against its source event;
    - each source event's signature.
  - A streaming pre-pass totals the plan against the target's meters (`memories`, `text_bytes`,
    `writes`; ADR-0071 d2). If the plan does not fit, the whole import is refused as
    `import.over_quota` before the first write.
  - Without a trusted source server, the signature proves only that the bundle is intact and was
    made under the key it names, not who holds that key. The plan says so.
  - The size and file-count limits are set at the build from measurement. Until then they follow
    the target's quota.

- **d8 — A re-import adds only what is new, ledgered per target workarea; an older bundle is
  refused.**
  - The idempotency key of an imported version is
    `import:<target workarea id>:<source server id>:<source tenant id>:<artifact id>:<version>`.
    Importing one bundle into two target workareas makes two copies, by intent; importing it twice
    into one target makes none.
  - A ledger per target workarea and source keeps the newest export instant and release imported.
    A bundle older than the ledger is refused as `import.bundle_older` unless `--allow-older` is
    given; an import that used it carries `allow_older: true` on `workarea.imported`, and the
    doctor lists such imports.
  - **`since <label>`** exports the signed difference between that release's manifest and the
    exported release's: the versions added or changed and the claim keys dropped, with both
    manifest digests in the bundle manifest. The target checks that its ledger's last imported
    release from this source has the `since` manifest digest, and refuses `import.since_mismatch`
    otherwise, so an incremental import never applies to the wrong base.
  - Each claim key is planned as `oml sync` plans (ADR-0057 d8):
    - `new`: written held;
    - `changed`: a held supersession of the target tip, with `expected_version`;
    - `removed`: a held retirement;
    - `unchanged` and `equivalent`: nothing is written;
    - `conflict`, when the target tip changed on the target since the last import: a held
      supersession marked `conflict`. The target tip is never written over.
  - `--dry-run` prints the plan and writes nothing.

- **d9 — A name that exists is never reused or overwritten.**
  - A new target name that is taken answers `name.taken` (ADR-0026 d5).
  - Importing into an existing in-progress workarea is a merge under d8.
  - The source's names are provenance and need not match.
  - A skill name or document key that would shadow, or be shadowed, in the target's chains is
    reported in the plan (ADR-0080 d5).

- **d10 — A person's key exports, under a tenant export policy; no agent, admin or service key
  exports.**
  - The exporter is a key held by a human with reader on every node of the subtree. A node it
    cannot read refuses the whole export as `export.not_readable`.
  - The tenant policy `export` takes one of three values:
    - `off`;
    - `own_root`, the default: a person exports only inside their own root;
    - `allowed`: shared workarea subtrees too.
  - Under `own_root`, an admin may allow one shared workarea's export as a policy act,
    `export.allowance_set {workarea, until?}`, journalled. It is a policy change, not a fix
    (ADR-0089), and it reads no content.
  - An admin key sets the policy; that changes what may leave and reads no content. An admin key
    never exports: its reads of claim text are single, reasoned and journalled (ADR-0089 d10), and a
    bundle is the whole subtree's text leaving the server, which no fix needs.
  - An export changes nothing on the source. It is journalled as `workarea.exported` with the
    manifest digest, mode, counts and recipients' fingerprints, and counted in bytes on a new
    `exports` meter.

- **d11 — A person's writer key imports; the target's scan runs on every version; plaintext is
  staged encrypted.**
  - The importer is a key held by a human with writer on the target.
  - Each version passes the target's scan rule set (ADR-0074), which may be newer than the
    source's. A refusal skips that version and names it in the outcome; a hold keeps it
    author-only (ADR-0074 d20).
  - On the server, the decrypted bundle is staged only under a key drawn for that import, with an
    entry in ADR-0005's erasure inventory under the class "import staging". The key is destroyed
    when the import ends or is abandoned, and the doctor names staging older than a day.
  - Writes go in batches, each its own transaction under the chain lock with its meters. An
    interrupted import resumes from its ledger, because d8 makes every write idempotent.
  - The import is journalled as `workarea.imported` with the source manifest digest and the
    outcome counts, and the server records every version the import wrote, so that the import can
    be rolled back.
  - **Rollback.** Developers roll an import back by rejecting its held versions and, for what has
    since been promoted, superseding or retiring through review. When that cannot wait, an admin
    rolls the import back at once as a fix (ADR-0089 d2): every version the import brought in that
    has since been promoted is demoted, which restores the tips it replaced, and every claim key it
    created is retired; keys bound to a release receive the same as a marked overlay
    (ADR-0089 d5). The rollback is marked, journalled with a reason, and confirmed or reverted
    by a person afterwards. Nothing is deleted.

- **d12 — A new shared target is three actors' work.**
  1. An admin creates `payments.dev` and grants the team.
  2. A person marks it in progress with their dreamer key (ADR-0030 d1), then imports with their
     writer key and so becomes the author of every imported version.
  3. A second person reviews the imported versions (R2). With the first person as proposer, the
     second publishes a release (ADR-0085 d4).

  The team's keys bound `latest` then receive the release. No step is open to an agent, and the
  admin's step touches no content.

- **d13 — Import and export never delete; an erasure reaches a copy as a notice, and a marker
  reveals nothing.**
  - An import writes only held versions, supersessions, retirements and edge proposals. An export
    only reads.
  - An erasure marker carries the artifact id, the version and the fact of erasure. It carries no
    digest of the erased text, because a digest would let a bundle holder confirm a guessed
    plaintext.
  - When a later bundle carries a marker for a version already imported, the target raises an
    erasure notice, `import.erasure_notice`, which the doctor counts. The target's own erasure act
    clears it (ADR-0005 d2; an admin's legal act, ADR-0089 d2).
  - Bundle files are outside every server's erasure. The bound is the recipients' custody and the
    encryption of d5, and the export's output says so.

- **d14 — The same-server copy is an import streamed from a release.**
  `oml workareas import --from <workarea>@<label> --into <path>` reads the release manifest as a
  stream into an in-progress workarea, with no encryption and no staging. It keeps `derives_from`
  to each source version, since both are on one journal, so R2's lineage closure excludes the
  source's authors from reviewing the copy. There is no return path into a committed workarea: team
  work is delivered through a shared development workarea's releases (ADR-0085).

- **d15 — Surfaces, events, doctor lines and meters.**
  - HTTP:
    - `POST /v2/workareas/{workarea}/exports {mode, release | since, recipients}` streams the
      encrypted tar.
    - `POST /v2/imports {target, dry_run}` takes the decrypted bundle as a resumable upload
      (ADR-0004 d3) and answers with the plan or the outcome.
    - `GET /v2/imports/{id}` reports progress.
  - CLI:
    - `oml workareas export <workarea> [--release <label> | --since <label> | --history]
      --recipient <age key>... --out <file>`;
    - `oml workareas import <file> --into <path> --identity <age identity> [--dry-run]
      [--allow-older]`;
    - `oml workareas import --from <workarea>@<label> --into <path>`.
  - Console: Export and Import, on the personal side.
  - No MCP tool (R1, as in ADR-0046 d3).
  - Events: `workarea.exported`, `workarea.imported`, `import.erasure_notice`.
  - Doctor lines:
    - verification failures, by code;
    - imports stopped, with their ledger position;
    - staging older than a day;
    - open erasure notices;
    - the tenant's export policy.
  - Meters: imported versions count as writes, memories and text bytes; exports count on `exports`.
  - Metrics: `oml_import_refusals_total{code}`, `oml_exports_total`, `oml_import_versions_total{outcome}`.
  - Throughput and bundle sizes are unmeasured. The batch size, d7's size limits and a
    per-day export limit are set from measurement at the build, recorded as a bottleneck row.

- **d16 — This is neither a backup nor a tenant move.**
  - `oml-backup` is the backup. The operator runs it, and it restores a whole database with keys,
    grants and the chain.
  - A bundle is one subtree's claims, made by a person, with no keys or grants, and it cannot
    restore a server.
  - A tenant move needs keys, grants, crews, accounts and quotas, which d3 refuses to carry. It is
    a later record, which may reuse this format for its claims.

## Consequences

- `release` mode waits for ADR-0085; `history` mode can be built first.
- `oml-bundle/1` gets a published schema under `docs/reference/`, and any change to it is a new
  format version.
- A migration adds import ledgers, staging keys, erasure notices and the `export` tenant policy.
  `import_provenance` is a new column; `origin.kind`'s list is unchanged.
- The operator's answers of 2026-11-14, each the recommendation:
  1. A person's key exports; no CI service key and no `exporter` profile.
  2. A bundle is always encrypted, a same-host move included.
  3. The default export policy is `own_root`.
  4. A person's identity is not carried across servers; d4's attestation stands in for it.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Imported promoted versions arrive promoted** | The target would serve content that no identity it knows reviewed, on the word of any key that can sign a manifest (R2). The verified case is ADR-0088, deferred. |
| **The importer re-signs and approves in one act** | The importer is the author on the target (d4); approving its own writes is exactly the case R2 forbids. |
| **Only promoted tips, never history** | A move needs the superseded versions, which R4 keeps. `release` mode remains the tips-only path. |
| **Every version, including other people's held and rejected ones** | Other people's held and rejected text would leave the server, which R3 and the reviewer's rejection forbid. |
| **A `pg_dump` of the tenant's rows** | Moves keys, grants and the chain, cannot merge into an existing tenant, rewinds erasure, and needs `BYPASSRLS`. That is the backup's job. |
| **An admin key exports** | An admin reads claim text one reasoned, journalled read at a time to fix something (ADR-0089 d10); a whole subtree leaving the server is not a fix. |
| **An agent exports or imports through MCP** | A bulk egress or a bulk write started by the agent is the gate being called by the thing it gates (R1). |
| **Import into a standard workarea, held** | Held rows there are author-only (R3), so the target's agents and tests could not see the import before it is served. |
| **Server-to-server pull (live sync) instead of a file** | Needs a standing credential on the target into the source, and makes every move an online dependency. A file lets people check what moves. |
| **A git repository as the bundle** | Git history, branches and merges would carry meaning the journal does not have, and a git object store does not stream line by line under limits. |
| **Merge by overwriting the target tip** | Would replace reviewed content on the target without review, and drop it from the tip (R4). |
| **A global idempotency key across target workareas** | A second import into a different target, a deliberate copy, would be silently skipped. |
| **An erasure marker carrying the erased text's digest** | Lets a bundle holder confirm a guessed plaintext by hashing it. |
| **A pseudonym stable across bundles** | Links one author across every bundle ever exported, building a profile no reader is entitled to. |
| **Signing with a separate export key** | A second key per server to rotate, back up and pin, with no gain over domain separation. |
| **An unencrypted bundle sent over TLS** | TLS protects the transfer, not the file at rest in a download folder. |
