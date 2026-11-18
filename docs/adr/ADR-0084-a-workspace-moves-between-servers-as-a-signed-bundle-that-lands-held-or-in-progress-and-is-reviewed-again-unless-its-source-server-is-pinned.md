<!--
id: adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned
tier: engineering
status: accepted design · 2026-11-14
answers: what a workspace export carries and what it never carries; the bundle's fixed layout, signature, encryption and hard limits, each with its refusal code; how authors and reviewers who do not exist on the target are represented; where an import lands and why every imported version is held; how a repeated import adds only what is new and refuses an older bundle; what happens when a name or claim key exists on the target; who may export under which tenant policy and who may import; how plaintext is staged; how import and export keep R4, and why an erasure marker reveals nothing; the flow for a new shared target; the same-server copy; which verbs, commands, events, doctor lines and meters it adds; and how it relates to oml-backup and to a tenant move
-->

# ADR-0084 — A workspace moves between servers as a signed bundle that lands held or in progress and is reviewed again

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with its four questions decided at their
recommendations and every other default as drafted · proposed the same day · the "unless its source server is
pinned" of its file name is ADR-0088, which is deferred · the format built as the crate `oml-bundle`, with its readings and the
operator's R1 decision of 2026-11-17 and the integration readings I1–I9 recorded at the end of this record; of d15, `history`-mode export (the route and `oml workspaces export`) is built (#425), and the import (`POST /v2/imports`, `GET /v2/imports/{id}`, `oml workspaces import`, the `imports` doctor line) is built (#426); the rest is not built · amendment of d6 proposed 2026-11-18 (imports into any open or protected workspace, the operator's answer D4 to the lifecycle review; #429)

**Release:** not scheduled; after ADR-0085, whose release manifest `release` mode exports. A
trusted source server, the "unless" of the title, is the deferred ADR-0088.

**Twin parts** (ADR-0090 d18, PR #388): the import ledger, the erasure notices and the `export`
policy are journal or part, never excluded. The per-import staging keys are excluded: they are
transient and destroyed when the import ends, so a rebuild needs none. The build names each
column's class under ADR-0090 d4.

**Amends:** ADR-0071 (d2: an `exports` meter), ADR-0005 (the erasure inventory gains the class
"import staging", d11), ADR-0063 (d1: `origin` is kept verbatim and an imported version gains a separate
`import_provenance`), ADR-0055 (d2: the review list shows a held imported version's source verdict
as evidence), ADR-0030 (d1: an import is a write into an in-progress workspace and adds no reader
of held rows)

## Context

On 2026-11-14 the operator asked for a development lifecycle in which a workspace is developed,
tested, verified and then promoted to production. "Promoted to production" means two things: the
same-server path, and a move between servers (staging to production, and sharing between
tenants). The
[workspace lifecycle review](https://github.com/e8kor/open-memory-lab/blob/main/design/workspace-lifecycle.md)
walks the whole cycle. ADR-0085 decides releases and binding, which carry the same-server path,
including team delivery through a shared development workspace and its releases. This record decides
the move.

Nothing moves a workspace today. The person export `GET /v2/web/export`
(`crates/oml-app/src/app/signup.rs:678`) is an unsigned JSON dump of a person's root, and nothing
imports it. `oml-backup`, in the operator's private repository, is a whole-database dump that
restores only into an empty cluster. A restore rewinds the chain and can bring back erased content
(`docs/operations/restore.md:32`).

The forces, from [ADR-0001](ADR-0001-the-constitution.md):

- **R1:** an import that served content would be a promotion, so an import promotes nothing. An
  admin key reads claim text one journalled read at a time, with a reason
  ([ADR-0089](ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)
  d10); a bundle is bulk egress, not a read.
- **R2:** the source's reviewers are keys the target cannot see.
- **R3:** a held row leaves no in-progress workspace, and a bundle is a way out.
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
  manifest.json            format, source server id, source tenant id and name, workspace paths,
                           mode, release or since cursor, export instant, count and SHA-256 per
                           file, signature
  workspaces.jsonl          one line per exported workspace: name, path, status, parent
  versions.jsonl           one line per claim version (d2, d3)
  verdicts.jsonl           one line per verdict on an exported version (d4)
  events.jsonl             each of the exporter's own versions' source journal event, as signed
                           (amended 2026-11-17, R1 below: no other version's and no verdict's)
  edges.jsonl              claim and brief edges whose two ends are exported
  bodies/<sha256>          a version's body over 64 KiB and every attachment, by plaintext digest
  ```

  The files are JSON Lines, so export writes and import reads one line at a time. A reader refuses
  an unknown format version as `bundle.format_unknown`.

- **d2 — `release` mode, the default outside a user root, carries one published release; `history`
  mode, the default in a user root, carries one workspace's record.**
  - **`release` mode** exports exactly the manifest of a release (ADR-0085 d1): the promoted
    versions with their lineage ids, without the superseded versions themselves.
  - **`history` mode**, for other people's claims, carries every version that was ever promoted,
    including those superseded after promotion. For the exporter's own claims (those of the
    exporting principal) it also carries held, rejected and demoted versions. Other people's held,
    rejected and demoted versions never leave the server.
  - Simulation rows keep their mark (ADR-0011 d8).
  - An erased version is carried as a marker, under d13.
  - `history` is for moving or archiving one workspace's record. It is not a backup (d16).
  - **In a user root, `history` is the default** (the operator, 2026-11-14: "History by
    default"). A user root never leaves development and has no release (ADR-0085 d4, d17), so an
    export whose subtree lies in a user root takes `history` mode without a flag. Asking for
    `release` mode there is refused `request.validation`, `details.field: mode`,
    `details.reason: user_root`, and so is `--since` (d8), which compares two releases:
    `details.field: since`, `details.reason: user_root`. A shared workspace keeps `release` as its
    default.

- **d3 — Claims of every kind ride as versions; grants, keys, crews and settings never do.**
  Memories, steering, terms, guardrails, skills, documents and tests travel as versions. Each
  version carries its format, its anchor (ADR-0057 d1), its valid time (ADR-0059 d1), its kind
  fields, its `derives_from`, and its `origin` verbatim as signed (ADR-0063 d1). Attachments travel
  under `bodies/` (ADR-0004 d2). Never carried: keys, connect codes, connections, grants, denies,
  crews, quotas, `mode`, `accept_federated`, review policy, bindings, release gate records, and
  anything in `artifact_keys`.

- **d4 — Authors and reviewers travel as provenance under per-bundle pseudonyms; the importer is
  the author on the target.**
  - Each version and verdict carries the source key's `used_by`, `same_user` and
    `approved_at`, plus a pseudonym: HMAC-SHA-256 of the source key id under a salt drawn fresh for
    each bundle and never exported. Inside one bundle an author is recognisable as one author;
    across bundles, never. A reader never learns who the reviewer was
    ([ADR-0077](ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md) d2).
  - On the target, every imported version is a new write by the importing key. Its `origin` is the
    source's, verbatim. A separate `import_provenance` object holds the source server id, tenant
    id, workspace, artifact id, version, digest and pseudonyms. This amends ADR-0063 d1, which had
    no place for it.
  - R2 on the target is checked against the importer.
  - The honest limit, in the manner of ADR-0001 d6: a person who wrote a version on the source and
    holds a reviewer key on the target is not recognisable there, and could approve their own work.
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
    authenticates nothing until ADR-0088; the plan and the `workspace.imported` event say which.
  - Export requires at least one age recipient and writes the tar encrypted to it; without one the
    export is refused as `bundle.unencrypted`. The server never holds a recipient's identity; the
    CLI decrypts on the importer's machine.

- **d6 — An import lands in an in-progress workspace, and every imported version is held.**
  - The target is a `development` workspace in progress, or a new name the import creates in
    progress where the importer may create workspaces (ADR-0044 d6). Any other target is refused as
    `import.target_not_in_progress`. *(Superseded, proposed 2026-11-18, D4 at the end of this
    record: any open or protected workspace, never a frozen one, with a person's verdict where the
    workspace is protected or released.)*
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

- **d8 — A re-import adds only what is new, ledgered per target workspace; an older bundle is
  refused.**
  - The idempotency key of an imported version is
    `import:<target workspace id>:<source server id>:<source tenant id>:<artifact id>:<version>`.
    Importing one bundle into two target workspaces makes two copies, by intent; importing it twice
    into one target makes none.
  - A ledger per target workspace and source keeps the newest export instant and release imported.
    A bundle older than the ledger is refused as `import.bundle_older` unless `--allow-older` is
    given; an import that used it carries `allow_older: true` on `workspace.imported`, and the
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
  - Importing into an existing in-progress workspace is a merge under d8.
  - The source's names are provenance and need not match.
  - A skill name or document key that would shadow, or be shadowed, in the target's chains is
    reported in the plan (ADR-0080 d5).

- **d10 — A person's key exports, under a tenant export policy; no agent, admin or service key
  exports.**
  - The exporter is a key used by a person with reader on every node of the subtree. A node it
    cannot read refuses the whole export as `export.not_readable`.
  - The tenant policy `export` takes one of three values:
    - `off`;
    - `own_root`, the default: a person exports only inside their own root, in `history` mode
      (d2);
    - `allowed`: shared workspace subtrees too.
  - Under `own_root`, an admin may allow one shared workspace's export as a policy act,
    `export.allowance_set {workspace, until?}`, journalled. It is a policy change, not a fix
    (ADR-0089), and it reads no content.
  - An admin key sets the policy; that changes what may leave and reads no content. An admin key
    never exports: its reads of claim text are single, reasoned and journalled (ADR-0089 d10), and a
    bundle is the whole subtree's text leaving the server, which no fix needs.
  - An export changes nothing on the source. It is journalled as `workspace.exported` with the
    manifest digest, mode, counts and recipients' fingerprints, and counted in bytes on a new
    `exports` meter.

- **d11 — A person's writer key imports; the target's scan runs on every version; plaintext is
  staged encrypted.**
  - The importer is a key used by a person with writer on the target.
  - Each version passes the target's scan rule set (ADR-0074), which may be newer than the
    source's. A refusal skips that version and names it in the outcome; a hold keeps it
    author-only (ADR-0074 d20).
  - On the server, the decrypted bundle is staged only under a key drawn for that import, with an
    entry in ADR-0005's erasure inventory under the class "import staging". The key is destroyed
    when the import ends or is abandoned, and the doctor names staging older than a day.
  - Writes go in batches, each its own transaction under the chain lock with its meters. An
    interrupted import resumes from its ledger, because d8 makes every write idempotent.
  - The import is journalled as `workspace.imported` with the source manifest digest and the
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
  2. A person marks it in progress with their reviewer key (ADR-0030 d1), then imports with their
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

- **d14 — The same-server copy is an import streamed from a release, or from history out of a
  personal root.**
  `oml workspaces import --from <workspace>@<label> --into <path>` reads the release manifest as a
  stream into an in-progress workspace, with no encryption and no staging. From a personal root, or
  part of it, which has no release (ADR-0085 d4, d17), the copy is one step too (the operator,
  2026-11-14: "Yes, from history"): `oml workspaces import --from <workspace> --into <path>` streams
  the subtree's approved versions from its history and lands them held in the target, where they
  are reviewed again under the same rules as any import. Shared workspaces keep streaming from a
  release. It keeps `derives_from`
  to each source version, since both are on one journal, so R2's lineage closure excludes the
  source's authors from reviewing the copy. There is no return path into a protected workspace: team
  work is delivered through a shared development workspace's releases (ADR-0085).

- **d15 — Surfaces, events, doctor lines and meters.**
  - HTTP:
    - `POST /v2/workspaces/{workspace}/exports {mode, release | since, recipients}` streams the
      encrypted tar.
    - `POST /v2/imports {target, dry_run}` takes the decrypted bundle as a resumable upload
      (ADR-0004 d3) and answers with the plan or the outcome.
    - `GET /v2/imports/{id}` reports progress.
  - CLI:
    - `oml workspaces export <workspace> [--release <label> | --since <label> | --history]
      --recipient <age key>... --out <file>`;
    - `oml workspaces import <file> --into <path> --identity <age identity> [--dry-run]
      [--allow-older]`;
    - `oml workspaces import --from <workspace>@<label> --into <path>`.
  - Console: Export and Import, on the personal side.
  - No MCP tool (R1, as in ADR-0046 d3).
  - Events: `workspace.exported`, `workspace.imported`, `import.erasure_notice`.
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
| **Import into a standard workspace, held** | Held rows there are author-only (R3), so the target's agents and tests could not see the import before it is served. |
| **Server-to-server pull (live sync) instead of a file** | Needs a standing credential on the target into the source, and makes every move an online dependency. A file lets people check what moves. |
| **A git repository as the bundle** | Git history, branches and merges would carry meaning the journal does not have, and a git object store does not stream line by line under limits. |
| **Merge by overwriting the target tip** | Would replace reviewed content on the target without review, and drop it from the tip (R4). |
| **A global idempotency key across target workspaces** | A second import into a different target, a deliberate copy, would be silently skipped. |
| **An erasure marker carrying the erased text's digest** | Lets a bundle holder confirm a guessed plaintext by hashing it. |
| **A pseudonym stable across bundles** | Links one author across every bundle ever exported, building a profile no reader is entitled to. |
| **Signing with a separate export key** | A second key per server to rotate, back up and pin, with no gain over domain separation. |
| **An unencrypted bundle sent over TLS** | TLS protects the transfer, not the file at rest in a download folder. |

## The format's readings of 2026-11-17

The crate `crates/shared/oml-bundle` builds the format of d1, d4, d5, d7, d8 and d13. The server
routes, the CLI verbs and the console pages of d15 remain designed, not built (#425, #426). The
record left ten points open, and the operator decided them on 2026-11-17. The first is a decision
that amends d1. The other nine are readings taken as recommended.

- **R1 — Source events travel for the exporter's own versions only (the operator: "Own events
  only"). Amends d1's "each exported version's and verdict's source journal event, as signed".**
  A signed event carries `principal`, the key's name as journalled (`crew-redline/agent-3`,
  `acme/admin-1`), and `api_key_id`. Its payload names `reviewer_key_id`, `ratifier_key_id` or
  `proposer_key_id`. A principal can name a person, and a verdict's names the reviewer. That would
  undo d4's per-bundle pseudonyms and ADR-0077 d2's rule that a reader never learns who the
  reviewer was. So `events.jsonl` holds the signed source event of each version the exporting
  principal wrote, and nothing else. Such a version names its event in `source_event`, and the
  reader checks the event's signature, its tenant and that it signed the line's artifact, version
  and plaintext digest. A version of another author, every verdict and every edge carries no
  event. Their integrity rests on the manifest signature, which covers every file's digest. A
  reader refuses an event on another author's version as `bundle.malformed`
  (`event_on_foreign_version`), and refuses an event no version names. ADR-0077 is not amended.
  - *Rejected: every event, verbatim.* The target could verify every version against its source
    signature, but key ids and principal names would leave the server, and a principal can name
    a person and a reviewer.
  - *Rejected: every event with its identities redacted.* A redacted event no longer verifies
    under its signature, so it would prove nothing the manifest does not already prove.
- **R2 — Container.** The bundle is a POSIX ustar tar, written by the `tar` crate 0.4 with its
  default features off, with no compression in `oml-bundle/1`, encrypted as a binary (not armored)
  age file. Its file suffix is `.oml-bundle`. Every entry is a regular file, mode 0644, owner 0 and
  mtime 0, so the same contents always make the same tar. *Rejected:* gzip, which adds a dependency
  and a decompression bomb to defend against; zstd, which is a C library (ADR-0006); armor, which
  adds a third to the size.
- **R3 — Order.** The files follow d1's order: `manifest.json`, `workspaces.jsonl`,
  `versions.jsonl`, `verdicts.jsonl`, `events.jsonl`, `edges.jsonl`. All six are present, even
  when empty. `bodies/<64 lowercase hex>` follow, in ascending order. Lines are written in source
  journal order, and a reader does not enforce line order. *Rejected:* leaving an empty file out,
  because the layout check would then not be exact.
- **R4 — The manifest.** The manifest is `{format, source {server_id, tenant_id, tenant_name},
  workspaces, mode, release, since, dropped, exported_at, exporter_pseudonym, event_keys, files,
  signature}`. Every member is present, with `null` or empty for an absent value.
  - `release` and `since` are each `{label, manifest_digest}`.
  - `files` lists each entry after the manifest as `{path, bytes, lines, sha256}`, with `lines`
    null for a body.
  - `event_keys` lists the journal public keys, in hex, that the carried events are signed with,
    because a chain may span a key rotation.
  - `signature` is `{alg: "ed25519", key_fingerprint, public_key, sig}`. The signed bytes are
    `oml-bundle/1\n` followed by the RFC 8785 form of the manifest without `signature`. The
    fingerprint is the lowercase hex SHA-256 of the 32-byte key, as `oml audit verify-chain` prints
    it, and `--expect-key` matches it case-insensitively.
  - Digests are spelled `sha256:<hex>`, as the API serves them.
- **R5 — Lines.** Field names follow the API and the event catalog. Every object is closed.
  - A version has `artifact_id`, `version`, `claim_key`, `workspace` (its path), `kind`,
    `format`, `title`, `state` (held, promoted, demoted, rejected, withdrawn), `retirement`,
    `replaces`, `simulation_id`, `text` or `body`, `plaintext_digest`, `anchor`, `valid_from`,
    `valid_until`, `skill`, `document`, `test`, `derives_from`, `origin` (verbatim),
    `attachments [{digest, media_type, size_bytes}]`, `author {used_by, same_user, pseudonym}`,
    `created_at`, `promoted_at` and `source_event`.
  - Text of 64 KiB or less is inline `text`. A larger text is `body`, its digest. A body that
    would fit inline is refused, so each text has one form.
  - A verdict has `artifact_id`, `version`, `verdict` (approve, reject), `reviewer`,
    `approved_at` and `at`.
  - An edge has `kind` (claim, brief), `src`, `dst` and `label`.
  - In `release` mode, every version is promoted.
- **R6 — `bundle.malformed {file, line, reason}`** is the refusal for a line that does not parse,
  breaks a closed shape, or breaks a rule d1–d4 state. Its message names the field and never the
  value. The same reading adds `bundle.decrypt_failed`, for an identity that is not a recipient of
  the file or a passphrase file, never naming the identity; and `bundle.io`, for a failure of the
  caller's reader or writer.
- **R7 — An erasure marker is exactly `{artifact_id, version, erased: true}`.** It carries no
  claim key and no event, because the source event signs the plaintext and ciphertext digests. A
  marker with any other member is `bundle.malformed` (`erasure_marker_content`).
- **R8 — A pseudonym** is the lowercase hex HMAC-SHA-256 of the source key id under the bundle's
  salt (`hmac` 0.12). The caller draws the salt for each bundle; the crate never exports it.
- **R9 — Limits.** d7's line limit (2 MiB) and depth limit (32) are fixed. The total unpacked
  size, the entry count and the size of one entry are passed in by the caller from the target's
  quota. Until measurement sets them, the CLI defaults to 1 GiB, 100,000 entries and 64 MiB per
  entry.
- **R10 — The plan** is a pure function of the manifest, the versions and the caller's view of
  the target. The view gives each claim key's target tip, held versions included; whether the last
  import from this source wrote that tip; and the ledger.
  - The bundle's tip of a key is its newest promoted version, or failing that its newest held one.
  - `removed`: the bundle tip is a retirement, or the key is in `dropped`, or a full release no
    longer holds a key imported from this source.
  - `unchanged`: the digest is equal. A key that is absent on the target and removed in the bundle
    is also `unchanged`, since there is nothing to retire.
  - `conflict`: a differing target tip that the last import from this source did not write. This
    includes a key the target held on its own.
  - `equivalent`: the caller's format diff says so.
  - `changed`: every other case.
  - `import.bundle_older` (unless `allow_older`) and `import.since_mismatch` refuse the whole plan
    before any key is planned.
  - A marker for a version already imported is listed for `import.erasure_notice` (d13).

### Review readings of 2026-11-17

The independent review of the format at `ef3ece0b` found these gaps. Each is now a reading, and
the code follows it.

- **R11 — A source's key is pinned at its first import.** Without this, a bundle signed by any key
  could claim the real `source.server_id` and `tenant_id`. It would then reach the ledger of an
  earlier import and drive `removed` and `changed` against the keys that import wrote. So the
  ledger keeps the fingerprint of the key that signed the last import from each source, and
  `plan_import` takes the verified bundle. Where a ledger for the source exists, the plan refuses
  `import.source_key_changed` in two cases: the bundle was signed by another key, or it was read
  without `--expect-key`, so nothing authenticated it. A first import from a source has no ledger
  and passes. Its plan carries `authenticated`, as d5 asks, and the server records the fingerprint
  in the new ledger row. *Rejected:* trusting the manifest's source ids, which anyone can write;
  and requiring `--expect-key` on every import, which the record does not ask for a first import
  (d5).
  - *First import.* The first import from a source pins whichever key signed it. So the import
    CLI (#426) requires `--expect-key` on a first import, and the pin is chosen deliberately, not
    taken from whatever key the bundle names.
  - *Key rotation.* A legitimate rotation of the source's journal key fails closed with
    `import.source_key_changed`, because the ledger cannot tell a rotation from a forgery. Until
    a re-pin act is designed, with #426 or with ADR-0088's trusted sources, the remedy is to
    import into a new target workspace, which has no ledger for the source.
- **R12 — The carried events are one key's writes.** A pseudonym on a line is the exporter's own
  claim. So the reader also requires these things of `events.jsonl`:
  - every event names one `principal` and one `api_key_id`, the exporter's;
  - its `event_type` is a write: `submission.remember` or `submission.learn` (a supersession
    is a `submission.remember` naming `replaces`);
  - its payload carries no `reviewer_key_id`, `ratifier_key_id` or `proposer_key_id` other than
    the event's own `api_key_id`.

  Anything else is `bundle.malformed` (`foreign_event`). That keeps R1's rule by construction: only
  the exporter's own key id and principal leave the server.
  A version written through `simulation.write` (ADR-0025) travels without an event, even when it
  is the exporter's own, so the server's export must not attach one to a simulation row.
- **R13 — Verification checks the tenant (d5).** `VerifyOptions` takes `expect_tenant`. The server
  passes the source tenant it expects, and the CLI passes `--expect-tenant` when one is given. A
  manifest naming another tenant is `bundle.signature_invalid`.
- **R14 — `exported_at` has the chain's fixed form, `YYYY-MM-DDTHH:MM:SS.ffffffZ`.** It is
  compared as text only in that form. A manifest in any other form is `bundle.malformed`.
- **R15 — Signatures are checked strictly.** The manifest and every event are verified with
  Ed25519's strict verification, which refuses small-order keys and non-canonical signatures.
- **R16 — `bundle.decrypt_failed` covers both decryption failures.** One is an identity that does
  not parse or is not a recipient. The other is ciphertext that does not authenticate. A recipient
  that does not parse is `bundle.unencrypted`, because nothing can be encrypted to it.
- **R17 — The age crate is 0.11, not 0.12.** 0.12's post-quantum recipients pull `hpke` and `p256`,
  and through them `crypto-bigint` 0.5 (RUSTSEC-2026-0387). The bundle needs only X25519 recipients
  and the binary format, which 0.11 has. *Rejected:* an advisory ignore in `deny.toml`.
- **Order of the work (the operator, 2026-11-17).** Resumable upload sessions, #451 (ADR-0004
  d3), come before import, #426. The import route takes the decrypted tar through such a session,
  not in one request body.

### Integration readings of 2026-11-17 (#425, #426)

The coordinator decided these while export and import were built.

- **I1 — An agent or admin key exporting is refused `access.verb_ceiling`.** The refusal's
  `details.reason` is `export_person_only`, naming d10's rule. *Rejected:* a separate code
  `export.agent_key`, which would duplicate the ceiling every other verb uses.
- **I2 — Policy `off` is refused `export.disabled` (403).** It is a new code with a row on the
  errors page. A person exporting outside their own root under `own_root`, with no allowance in
  force, is refused the same code with `details.reason: own_root`.
- **I3 — `mode: release` and `since` are refused `request.validation`,** with `details.reason:
  releases_unbuilt`, until releases (#414) are built. On a user root they stay refused under d2
  after that, with `details.reason: user_root_history_only`. *Rejected:* leaving the fields out of
  the contract until then, which would change it twice.
- **I4 — Import staging is the bound upload session's own key (ADR-0004 d3).** The decrypted
  bundle reaches the server only as the parts of one upload session, encrypted under that
  session's `part_key`. The import binds the session; when the import ends or is abandoned the key
  is nulled, which is d11's shred. The class "import staging" of ADR-0005 is that same row, so the
  inventory names it once, as "upload staging". *Rejected:* drawing a second key for the import
  and re-encrypting the staged bytes, which would hold the plaintext twice.

### Export readings of 2026-11-18 (#425)

The coordinator confirmed these while history-mode export was built.

- **I5 — The source server id is the journal key's fingerprint.** `manifest.source.server_id` is the
  lowercase hex SHA-256 of the deployment's journal public key (ADR-0073 d1), the value
  `--expect-key` names. No other server identity exists. A key rotation therefore makes the server
  read as a new source, which overlaps R11: a rotation already fails closed against an existing
  ledger, and the remedy is the same, an import into a new target workspace. *Rejected:* a new
  `server.id` setting, one more value an operator must set and keep unique; the public URL, which
  changes when the host does.
- **I6 — The exporter's own versions are those of the person's own keys.** Held, rejected,
  demoted and withdrawn versions travel when a key of the exporting person's own key set wrote them
  (the set a session key reads its own rows by, ADR-0045 d16). A linked key acting for another
  person is not in it. A signed event travels only for a version the exporting key itself wrote
  (R12). *Rejected:* the exporting key alone, which would leave a person's console writes out of
  their own export.
- **I7 — `same_user` is what the source journal recorded, never recomputed.** A version's
  `author.same_user` is its promotion's mark; a verdict's `reviewer.same_user` is the same mark on
  the approving verdict that promoted the version. Without a mark either is `false`. `approved_at`
  is set on that approving verdict only.
- **I8 — Only ratified edges travel, and only when both ends are exported.** A brief edge's ends
  are its artifact and its workspace, which must lie in the exported subtree. A pending proposal
  is another key's unratified act and stays.
- **I9 — The admin routes for the policy and allowances come later.** The tenant's policy is the
  default, `own_root`, until `PUT /v2/tenant/export-policy` and the allowance route are built; the
  migration adds the column, the allowance table and the `exports` meter, and the doctor names the
  policy.

### Fix readings of 2026-11-18 (the review of bulk B; for the operator's batch)

Two independent reviews of the bulk B head `634c8e90` asked for revision. The coordinator decided
these on 2026-11-18, and the code follows them. Each is marked **[operator batch]**: it stands
until the operator confirms or reverses it with the other decisions of the batch.

- **I10 — An import runs after its request answers. [operator batch]** `POST /v2/imports`, not
  a dry run, verifies the bundle, plans it, binds the upload and starts the run in a task that
  does not depend on the request; it answers `202 {import}` with the row `running`. A dry run
  still answers `200 {plan}` at once. The CLI polls `GET /v2/imports/{id}` until the import ends.
  *Rejected:* the inline run, which ties an import of up to `uploads.max_bytes` to one HTTP
  request and loses it when a proxy or the client gives up.
- **I11 — An import interrupted for an hour is ended. [operator batch]** At the lead's start and
  in every upload sweep, an import `running` for more than one hour that this process is not
  running ends `failed`, its bound upload's key is nulled in the same transaction and its objects
  are removed by the sweep (d11's shred). What it wrote stays held. The doctor's
  `imports_stopped` warning counts only such imports. A bound upload whose key is still set more
  than 24 hours after it was bound is the doctor warning `upload_staging_stale` (d11). A store
  failure while shredding is logged, never swallowed. The list of imports this process runs is the
  process's own: it holds while one lead serves the database, as ADR-0072 d12's counters do.
- **I12 — The source key is pinned when the import begins (R11). [operator batch]** The
  transaction that binds the upload and inserts the import row also inserts the ledger's pin,
  `INSERT … ON CONFLICT DO NOTHING`, and compares: a pin naming another key refuses
  `import.source_key_changed` and writes nothing. A pin written for an import that never ended
  carries an empty `exported_at`, so it bounds no later bundle's age. The `workspace.imported` fold
  moves the ledger's export instant and release and never its `signer_fingerprint`. So two
  concurrent first imports pin one key, and a failed first import still pins the key it named.
- **I13 — Imports and exports are bounded in memory. [operator batch]** One import or dry run
  per tenant at a time, and at most two across the process; over either limit is
  `import.busy` (409, retryable). One export per tenant at a time, refused `export.busy` (409,
  retryable), and a bundle larger than `uploads.max_bytes` is `export.too_large` (413). The parts
  are not streamed into the reader: the verified bundle holds every body in memory, so a streamed
  read saves only the tar's copy, and the bound above is what holds the peak. The export's
  whole-chain walk and in-memory build, and the per-day limits of d15 that are not built, are
  bottleneck rows on the limits page. Exports are bounded at two in the process too, and an export
  whose stored body bytes already pass the cap is refused before a body is read; the bounds are the
  process's own and hold while one lead serves the database (ADR-0072 d12).
- **I14 — An imported version's provenance is part of its write. [operator batch]** The
  `import_versions` row and `import_provenance` are written in the submit's or supersession's own
  transaction, as a field of its fold, so no version an import wrote lacks them.
  `workspace.imported` lists the imported version ids, so the journal rebuilds the ledger of what
  each import wrote. The doctor warns when a version carries an import's idempotency key and no
  provenance.
- **I15 — A conflict that is a removal writes a retirement (d8). [operator batch]** Where the
  source removed a key and the target's tip moved, the import writes a held retirement marked
  `conflict`, not a superseding text.
- **I16 — A new target is created in progress in one commit (amending ADR-0030 d1).
  [operator batch]** See ADR-0030's note of 2026-11-18. A retry against a `standard`, empty
  workspace that the importing key created sets the mode then.
- **I17 — Verdicts on erased versions do not travel (R1, d2). [operator batch]** A verdict is
  exported only for a version exported whole; `oml-bundle`'s reader refuses a verdict that names an
  erasure marker or a version the bundle does not carry, `bundle.malformed`. A version's
  `derives_from` keeps only the ids the bundle carries.
- **I18 — A crew key never exports (d10, I1). [operator batch]** The export rule excludes a key
  that belongs to a crew, as it excludes an agent key: `access.verb_ceiling`, `details.reason:
  export_person_only`.
- **I6 as written (replacing the build's reading of 2026-11-18). [operator batch]** For any key
  acting for a person, the own set is that person's key set: the person's web keys, the person's
  own agent keys, and the key itself; not the calling key's own set alone. A bearer person key
  exports its person's console writes.
- **I19 — Every export is metered. [operator batch]** An export by a key with no account root is
  counted on the tenant's `exports` meter, so no export goes uncounted.
- **I20 — A guardrail travels with its severity and rule (d1). [operator batch]** The
  `oml-bundle/1` version line gains an optional `guardrail: {severity, rule}` object, as `skill`,
  `document` and `test` carry theirs. Export writes it for every guardrail version that is not a
  retirement; the reader refuses a guardrail line without it, or another line with it, as
  `bundle.malformed`, and checks the severity is known and the rule, when given, parses as
  `oml-gr/1` and is one the severity admits; import passes both to submit, whose caps and scan
  run again. An imported guardrail lands held and is served inside the in-progress target
  unreviewed with `effect_inside: ask` (ADR-0080 d10, d16). The format is unreleased, so no
  reader of an earlier form exists. *Rejected:* leaving guardrails out of bundles, which an
  import refused by name and a person could not tell from a scan veto.
- **I6, a linked key (2026-11-18, the re-verify of bulk B). [operator batch]** A tenant key linked to
  a person (ADR-0085 d16) owns itself alone in an export: the own set widens to a person's keys only
  for a user key or a session key of that person, never through `linked_user`, so a linked key
  exports none of the person's other keys' held, rejected or withdrawn rows (R3).

### The lifecycle review's amendment of 2026-11-18 (D4; proposed, #429)

The operator answered the lifecycle review's import question on 2026-11-18 (ADR-0085's
amendments of the same day hold D1 to D3). The question read "imports into any open workspace,
and into protected ones only if every imported item then gets a person's verdict from someone
other than the importer"; the answer chosen was "Any open; protected needs verdict", described as
"A team can import into its normal workspace, and a protected workspace stays two-person". This
records the answer as proposed; the operator accepts the text.

- **D4 — An import lands held in any open or protected workspace, and a protected or released one
  needs a person's verdict (amends d6; ADR-0030 d1's exception of 2026-11-18).**
  - **The target** is a `development` workspace, in progress or `standard`, a `protected`
    workspace, or a new name the import creates. A `frozen` target is refused, because a frozen
    workspace is read-only for good (ADR-0009 d5).
  - **Every imported version is written held** by the importing key, as d6 says. In a `standard`
    or `protected` workspace only the importer reads them until review (R3); in one in progress,
    d6's `tip` audience reads them marked unreviewed.
  - **A person's verdict where the workspace is protected or released.** In a `protected`
    workspace, or a workspace with a published release, withdrawn or not **[operator batch]**, an approving verdict on
    an imported version comes only from a person's reviewer key (`used_by: person`) of a person
    other than the importer's own. Any other approval is refused `review.person_required`,
    `details.reason: imported_needs_person`, and the version stays held; a reviewer crew's agent
    key does not approve an imported version there **[operator batch]**. The rule binds by where the version is at the
    verdict **[operator batch]**, so it also covers a version still held when its workspace is moved to `protected` or
    first publishes a release. d4's `attests_not_source_author` still travels with the verdict.
    Why: R2 on the target is checked against the importer, and d4's honest limit leaves only an
    attestation between a source author and their own work; an agent's attestation carries
    nothing, so where content is production (ADR-0085 D1) a person judges it, and a protected
    workspace stays two-person.
  - **The refusal for a frozen target is `import.target_frozen`** (409, `details.workspace_id`).
    The errors page groups an import's refusals in the `import` family (`import.over_quota`,
    `import.bundle_older`, `import.busy`), and `workspace.frozen` already names a write refused by
    a frozen workspace; the import's code takes the import family and the word `frozen` from that
    code. *Rejected:* answering `workspace.frozen` itself, which names a write, child, promotion or
    move, so an importer could not tell a refused import from a refused write; keeping
    `import.target_not_in_progress`, which names a rule this amendment retires. The rename is a
    pure cut under ADR-0091 d2.
  - **A new target is created `standard` [operator batch].** The target no longer has to be in
    progress, so ADR-0030 d1's exception of 2026-11-18 (I16), in which the importing writer key
    marked its new workspace in progress, is superseded; a person's reviewer key marks it
    afterwards if the team wants its `tip` keys to read the imported work. This keeps the one mode
    act a person's, at the cost of that second step.
  - **What is built.** v0.8.2 built d6 as first written: in-progress targets only (#426). The wider
    rule and the verdict rule are built with the release work (ADR-0085), not before; until then
    the shipped refusal stands and the pages that describe the built import do not change.
  - **Superseded:** d6's first bullet, from "The target is a `development` workspace in progress"
    to "`import.target_not_in_progress`"; I16's new target created in progress.
