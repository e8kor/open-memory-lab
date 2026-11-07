<!--
id: adr/ADR-0062-oml-sync-maps-a-delta-lakes-tables-into-anchored-claims-from-the-log-alone-on-the-client-and-catalogs-and-reviewed-terms-say-what-they-mean
tier: engineering
status: proposed
answers: how a client's Delta Lake tables become memories an agent can recall (what lives where, its schema, its business meaning and its terms); how this differs from ADR-0020's row import and from OML's own lake (ADR-0003); what the `delta` adapter of `oml sync` reads from `_delta_log` and what it never reads; why a minimal log reader in a separately released CLI build and not delta-rs; the anchor of a table and a column, the hashers and the claim keys; which schema changes make which claim stale; how renames are followed through column mapping; what is retired and when; how often a lake is re-checked; who may approve what; how DataHub, OpenMetadata, Glue and Hive metastore are imported, and how their lineage becomes edges; how glossary terms attach to tables and columns, who curates them, and why the mapping is deterministic; the test lake and the eval; volume, cost and review burden at 2,000 tables; workarea layout; privacy and credentials; doctor lines, tests, open questions and the v0.6.x build
-->

# ADR-0062 — `oml sync` maps a Delta Lake's tables into anchored claims read from the log alone on the client, and catalogs and reviewed terms say what they mean

**Status:** proposed · 2026-11-05

**Release:** planned for v0.6.1 to v0.6.4, not assigned until accepted ([releases](../product/releases.md#planned-releases))

**Amends:** ADR-0057 (d3: `uri` admits `s3` and `gs` beside `https`, and a Delta version is a
`rev`; d4: `anchor_uri_prefix` joins the origin filters; d5: a lake source check is recorded at
most once a day for an unchanged claim; d6: a `uri` source is read on the client from object
storage, never from a git tree; d21: the fragment form `col=`), ADR-0061 on its branch (d4: the
set extraction reads a table's log files; d7: four hashers join the closed set; d10: the `delta/`
and `catalog/` namespaces), ADR-0020 (d1: a Delta snapshot's locator is this record's anchor; d8:
a manifest derives from the table's schema claim; d7: the client reads the snapshot and streams
the rows to the server, which never touches a client lake; d4 and d11 unchanged),
ADR-0051 on its branch (d3: the author's `about` in the write is how a sync proposes terms; d10:
catalog glossaries are imported client-side as facets), ADR-0027 on its archived branch (d1–d6,
d8 and d12–d14 unparked for lake vocabularies), and ADR-0046 (d6: a standalone `oml:about`
proposal is admitted from a key carrying ADR-0027's `classifier` profile, and refused for every
other key as before)

**Depends on:** [ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
(anchors, `oml sync`, its diff, batches and source checks, accepted for v0.5.8);
[ADR-0061](https://github.com/e8kor/open-memory-lab/pull/70) (the set extraction and
namespace-scoped retirement, proposed for v0.5.9);
[ADR-0056](https://github.com/e8kor/open-memory-lab/pull/63) (the JSON format, proposed);
[ADR-0051](https://github.com/e8kor/open-memory-lab/pull/58) (reviewed terms and `about`
edges, proposed); [ADR-0055](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
(bulk review).

**Numbering.** ADR-0058 to ADR-0060 are left to lanes in flight on 2026-11-05 and ADR-0061 is the
Rust adapter's, so this record takes 0062; its open work is #161 and its use
cases UC-SM-07 to UC-SM-10, after ADR-0061's #160 and UC-SM-06. Either is renumbered at merge
if a gap is not taken.

## Context

**The operator's decision of 2026-11-05.** OML maps Delta Lake datasets into memories with
anchors, so an agent can recall which table holds what, where it lives, its schema, its terms and
its business meaning. The sources are Delta tables on plain object storage (S3, ADLS, GCS) read
through `_delta_log`, open catalogs (DataHub, OpenMetadata, Hive metastore and Glue) as
importers, and a test lake generated in the repository. Metadata only, never data: no per-file
statistics, sample values only by an explicit per-table opt-in and through the scanner, which the
operator later withdrew (d15). Lake
credentials stay on the client and the server never reaches the lake. One table claim (purpose,
owner, grain, partitioning, freshness), one schema claim in ADR-0056's JSON format, and a
column claim only where business meaning is stated. Mechanical facts may be approved by an
automated reviewer only when it re-derives them from the log at that version (ADR-0047 d9);
business meaning and ontology need a person. ADR-0051's vocabularies and ADR-0027's crews are
revived for the ontology.

**What OML already decided about datasets, and why this is not a duplicate.**
[ADR-0020](ADR-0020-dataset-onboarding.md) (accepted design, post-v1, not scheduled) imports a
dataset's **rows** as memories under one reviewed manifest: per-row claims, Merkle roots, seeded
sampling, per-manifest keys, rows materialised into OML's own lake. This record imports a
table's **description**: a few claims per table, each reviewed like any memory, and no row. The
two meet at one point: a row import names the table it reads, and that table is the one this
record describes (d1). ADR-0020 d7 had a runner inside `oml-server` stream the pinned snapshot,
which for a client's lake means the server reads it. *Decided by the operator on 2026-11-05:* the
server never touches a client lake, for rows as for descriptions, and only receives what a client
sends; d1 amends ADR-0020 d7 to that.

**Which lake.** [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) is OML's own lakehouse:
bronze, silver and gold Delta tables of OML's journal and memories, written by the keyless
singleton writer with delta-rs ([ADR-0006](ADR-0006-pure-rust.md) d2). This record reads
**client** lakes, on the client, under the client's credentials, and writes only through the
HTTP API. Nothing here reads or writes ADR-0003's tables, and no name here reuses theirs: the
server crate `oml-lake` is ADR-0003's, so the reader below is `oml-delta-log` and the CLI feature
is `lake-sync`.

**What the Delta log holds.** From the protocol
([PROTOCOL.md](https://github.com/delta-io/delta/blob/master/PROTOCOL.md), fetched 2026-11-05):
a `metaData` action carries `id`, `name`, `description`, `format`, `schemaString`,
`partitionColumns`, `createdTime` and `configuration`; with column mapping (`delta.columnMapping.mode`
one of `none`, `id`, `name`) "every column - nested or leaf - is assigned a _physical_ name, and a
unique 32-bit integer as an id", in the field metadata keys `delta.columnMapping.physicalName` and
`delta.columnMapping.id`, which is what allows "the renaming and dropping of columns without
having to rewrite all the data"; readers find a recent checkpoint through
`_delta_log/_last_checkpoint`, among classic, multi-part (deprecated) and UUID-named V2
checkpoints, whose sidecar files "can have only" `add` and `remove` entries; `commitInfo` is free
form ("Implementations are free to store any valid JSON-formatted data"); and under the
`catalogManaged` table feature "Filesystem-based access to catalog-managed tables is not
supported." The Delta table properties page gives `delta.logRetentionDuration` a default of
"interval 30 days" ([table properties](https://docs.delta.io/latest/table-properties.html),
fetched 2026-11-05), so a table older than that may have no JSON commit before its last
checkpoint, and a reader that cannot read a checkpoint's Parquet cannot read its schema.

**What a delta-rs build costs.** A probe crate on this host on 2026-11-05 depending on `deltalake`
1.1.2 with its `datafusion` and `rustls` features resolved 434 packages in its lock, against 425
for OML's whole workspace at this branch; its build ran 4 min 46 s and failed in
`deltalake-core`, so its binary size is unknown. The released `oml` 0.5.7 CLI installed here is
5,997,256 bytes. The CLI ships for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and
`x86_64-pc-windows-msvc` (`.github/workflows/release.yml`), and the musl lane already compiles
ring's C with Ubuntu's musl wrapper.

**What a competitor does.** Cognee builds a graph from a DLT source's relational schema with no
model call (`get_dlt_tasks`, `cognify.py:760-803`; foreign-key edges in
`extract_dlt_source_edges.py`) and purges the source's earlier artifacts on re-emit; its ontology
matching is `difflib` fuzzy matching at a 0.8 cutoff, unreviewed (competitor study B,
2026-11-05, Cognee at `525b1d1`). The study's recommendation is adopted here in OML's form: the
mapping is deterministic, a re-emit supersedes through review instead of purging, and no fuzzy
match attaches a term.

**What it is held against.** The constitution ([ADR-0001](ADR-0001-the-constitution.md)): R1, no
agent key promotes, proposes or ratifies; R2, the reviewer is never the author; R3, a held row is
read by its author and its reviewer; R4, nothing is deleted. The Occam rule
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8): every new call traces to a use case,
and a capability is a parameter before it is a tool. ADR-0057 d2: the server never fetches a
source.

Decided, not built: this lane wrote no code and ran no build.

## Decision

- **d1 — A table's description is memories under this record; its rows stay ADR-0020's, and the
  two are joined by the anchor.** *Amends ADR-0020 d1 and d8.* This record writes claims that
  describe a table at a Delta version and never a row. ADR-0020's manifest names its source
  locator; for a Delta table that locator is this record's anchor (`uri`, `rev` the version,
  d5), and a manifest `derives_from` the table's promoted schema claim (d6), which ADR-0020 d8
  already admits as a "source-describing claim". So a reviewer of a row import reads the schema
  the mapping spec was written against, reviewed once, not restated in the manifest. ADR-0020
  d4's dataset workarea and d11's indexing default are unchanged.
    - ***Amends ADR-0020 d7: the client reads the snapshot; the server only receives.*** *Decided
      by the operator on 2026-11-05.* The import runner moves out of `oml-server` into the lake
      build of the CLI (d3): under the client's credentials it reads the pinned snapshot, applies
      the mapping spec and streams the mapped rows over the authenticated HTTP API to the capture
      route, chunk by chunk. Everything ADR-0020 d7 does at capture stays in `oml-server`, the one
      key-holding locus: a random DEK per row wrapped under the manifest's wrapping key, the
      quarantine chunk objects and both Merkle roots, computed over what the server received, so
      `import.manifest_submitted` is signed over bytes the server holds. The server holds no lake
      credential, opens no lake object and makes no outbound request for an import; the snapshot
      pin is the importing key's declaration, as ADR-0020's honesty list already says of source
      authority, and a reviewer who wants it re-derived runs the mapping on the client, as d12's
      CI key does for schemas. The costs: every row crosses the client's network once in
      plaintext under TLS, as any write does; a ten-million-row import is a long-running client
      process that resumes by chunk index (ADR-0020 d6's chunking, now on the upload side); and its
      throughput is unmeasured (#161).

- **d2 — The `delta` adapter reads a table's log at one version and only four actions; it never
  opens a data file.** `oml sync <lake uri> --workarea <w> --adapter delta` reads, for each table
  under the source (d10), `_delta_log/_last_checkpoint`, the newest checkpoint at or below the
  version, and the JSON commits after it up to the version, and from them only:
    - **`protocol`**, for its reader and writer features (d4);
    - **`metaData`**, for `id`, `name`, `description`, `schemaString`, `partitionColumns` and the
      `configuration` keys listed in d9, and no other key;
    - **`domainMetadata`** of the `delta.clustering` domain, for clustering columns;
    - the **version number** itself.

    It never decodes `add`, `remove`, `cdc`, `txn`, `sidecar` or `commitInfo`. So no file path,
    partition value, per-file statistic, row count, user name, notebook path or job id reaches the
    client's memory, let alone the server: a partition value is data, and `commitInfo` is free
    form and routinely names people. A checkpoint is read by Parquet column projection on the
    `protocol`, `metaData` and `domainMetadata` columns alone, so the `add` column chunks, where a
    large table's checkpoint holds its bulk, are never fetched; a V2 checkpoint's sidecars, which
    hold only `add` and `remove`, are never listed. The fixture's request log asserts that no
    object outside `_delta_log/` is ever opened (d19).

- **d3 — A minimal log reader in a new crate, linked into a second CLI build; delta-rs is the
  test oracle and never ships in the CLI.** The reader is `crates/adapters/oml-delta-log`: a pure
  function from the log files' `(path, bytes)` to `{version, protocol, metadata,
  clustering}`, over ADR-0061 d4's set extraction (*amends* it: the set is one table's log files,
  not a source tree's), with `serde_json` (already in `oml-cli`) for commits and the `parquet`
  crate without its `arrow` feature for checkpoints. Fetching is a port, `LogStore {list, get,
  get_range}`, implemented over `object_store` with its `aws`, `azure` and `gcp` features on
  `reqwest` and `rustls` with ring, the provider the workspace already pins. The reader and the
  store are compiled into `oml` only under the cargo feature `lake-sync`, and the release adds one
  artifact per CLI target, released as `oml-lake-X.Y.Z-<target>` (`.zip` on Windows), holding an
  `oml` binary built with it (*decided by the operator on 2026-11-05*): the same verbs, plus `--adapter delta` and the catalog fetchers (d13). The
  default `oml` stays as it is and refuses `--adapter delta` naming the lake build. The
  constraints, each a reason:
    - **Size and targets.** `object_store`, `reqwest`, `tokio` and `parquet` add an async runtime
      and three cloud clients to a 6 MB binary that agents install, and most agents never read a
      lake; the added size is unmeasured, and the lake build ships only once #161 has measured it
      on all three targets. Parquet's `zstd` codec compiles C through `zstd-sys`, of the same
      class as ring's C that the musl lane already builds; if it does not build there, a
      `zstd`-compressed checkpoint is reported `unsupported` naming the codec, never misread.
    - **Why not delta-rs in the CLI.** It reads data and writes tables, which this reader must
      never do; with `datafusion` it resolved more packages than OML itself; and its build on this
      host failed. Without `datafusion` it is smaller, still arrow-based, and still a table API.
    - **Why not `delta_kernel` alone.** It is the protocol's reference reader and handles every
      log shape, but its default engine is arrow and object_store too, and its API reads scans. The
      minimal reader needs four actions.
    - **The oracle.** The test lake's generator (d19) is a separate cargo workspace under
      `testing/lake/` that depends on `deltalake`; it writes the fixture tables and, for every
      table and version, the schema, partitioning and configuration delta-rs reads, as
      `oracle.json`. Both are committed; CI never builds the generator, and a test asserts the
      reader's output equals the oracle for every fixture version. Regenerating is a manual step
      whose diff is reviewed like any fixture change.

- **d4 — The reader fails closed on what it does not know and names it.** A table whose
  `protocol` lists a reader or writer feature outside an allowlist kept in the reader is
  `unsupported`, with the feature named, and its claims are neither written nor retired
  (ADR-0061 d10). The allowlist starts with the features that change no part of the log the reader
  reads (`columnMapping`, `deletionVectors`, `timestampNtz`, `typeWidening`, `v2Checkpoint`,
  `domainMetadata`, `clustering`, `inCommitTimestamp`, `appendOnly`, `invariants`,
  `checkConstraints`, `changeDataFeed`, `generatedColumns`, `identityColumns`, `rowTracking`,
  `vacuumProtocolCheck`, `variantType`, `icebergCompatV1`, `icebergCompatV2`), each confirmed
  against the protocol text when the reader is built. `catalogManaged` is refused by name: its
  commits are not in `_delta_log` alone. A log with a gap between the checkpoint and the first
  commit, a checkpoint that does not parse, or a `_last_checkpoint` naming a missing file is
  `unreadable`. Neither is ever `removed`.

- **d5 — The anchor of a lake claim: the table's location as `uri`, its Delta version as `rev`,
  and a column as `col=<path>`.** *Amends ADR-0057 d3 and d21.*

    | Field | Rule |
    |---|---|
    | `uri` | ADR-0057 d3's `https` URL, or `s3://<bucket>/<path>` or `gs://<bucket>/<path>`, the bucket a DNS-safe label and the path NFC; for all three no user information, no `?` and no `#`, at most 2,048 characters, no trailing `/`. ADLS `abfss://<container>@<account>.dfs.core.windows.net/<path>` is written as `https://<account>.dfs.core.windows.net/<container>/<path>`, the same object's endpoint, so `@` stays refused. The client derives it from the location it read, lower-casing scheme and host and keeping the path's case. |
    | `rev` | The Delta version in decimal, `0` to `9223372036854775807`, no sign or leading zero; ADR-0057 d3 already admits it for `uri`. |
    | `fragment` | Absent for the table and schema claims; `col=` and the column's path for a column claim: the logical names from the top-level field down, each percent-encoded outside `A-Z a-z 0-9 - _ ~`, joined by `.`. Refused when malformed, as d21 refuses a bad line span. |
    | `hash`, `hasher` | One of d7's hashers. |

    The anchor never names a local filesystem path: a lake read from a mounted directory or a copy
    (the test lake, a laptop mirror) is read with `--uri-base <canonical uri>`, and the anchor
    carries the canonical location. The server checks the grammar, scans it and computes nothing
    from it (ADR-0057 d2). The cost: a bucket and path, once written, are kept for good, as a
    repository path is.

- **d6 — Three claim shapes, keyed by the table's place under the source and a column's physical
  name.** A table's **key stem** is `delta/` and its path relative to the source uri, lower-cased,
  characters outside ADR-0050 d2's grammar mapped to `-`; when that mapping changed any character
  (an upper-case letter, a space), the stem ends in `-` and the first 8 hex digits of the SHA-256
  of the exact relative path, so `Sales/Orders` and `sales/orders`, two tables on a case-sensitive
  store, never share a key. Keys are fitted to 128 characters as ADR-0057 d7 fits them.

    | Claim | Key | Text | Format |
    |---|---|---|---|
    | **Table** | `<stem>/table` | `Delta table <name or last path segment> at <uri>.` then, each on its own line when present: `Purpose:` the description; `Owner:`, `Grain:`, `Freshness:` from d9's properties; `Partitioned by:` and `Clustered by:` the columns in order. | `text/markdown` |
    | **Schema** | `<stem>/schema` | One JSON object: `table`, `partition_columns`, `clustering_columns`, `column_mapping` (`none`, `name` or `id`) and `columns`, an array in schema order of `{path, type, nullable}`, nested struct fields flattened to dotted paths, `type` Delta's type name (`decimal(18,2)`, `array<string>`, `map<string,long>`). Keys sorted, no whitespace. | `application/json` |
    | **Column** | `<stem>/col/<physical name>` | `Column <path> of table <name>: <meaning>`, the meaning the column's `comment`, or a mapping rule's term (d14), or both. No type: the schema claim states it. | `text/markdown` |

    The physical name is `delta.columnMapping.physicalName` when column mapping is on, and the
    column's name otherwise, mapped and fitted like the stem. A column claim exists only where a
    meaning is stated: a non-empty `comment` in the field's metadata, or a rule of d14 that names
    the column; a column with neither has no claim, and the schema claim lists it. A schema whose
    JSON would exceed ADR-0056 d6's caps (2,000 scalars, 64 KiB) is split into parts keyed
    `<stem>/schema/<n>` of 500 columns each, so a table up to 666 columns is one claim. Until
    ADR-0056 is built every claim carries no format and reads as plain text, and the schema's JSON
    is plain text; nothing else changes. The table name in the text is `metaData.name` when the
    writer set one, else the stem's last segment, since `name` is optional in the protocol.

- **d7 — Three hashers join the closed set for the `delta` adapter, each covering only the facts
  its claim states; d13 adds a fourth, `oml-catalog/1`.**
  *Amends ADR-0057 d3 and, on its branch, ADR-0061 d7.*
    - **`oml-delta-table/1`**: SHA-256 over one line per fact, in order: the table name, the
      description normalised as `oml-md/1` normalises, `owner`, `grain`, `freshness` the same,
      `part <column>` for each partition column in order, `clus <column>` for each clustering
      column in order.
    - **`oml-delta-schema/1`**: SHA-256 of the schema claim's JSON as written, keys sorted and no
      whitespace, which is already canonical.
    - **`oml-delta-col/1`**: SHA-256 over the column's logical path, its comment normalised as
      `oml-md/1`, and each rule-assigned CURIE sorted (d14).

    None covers the version, a commit time, a row count, a file, `metaData.id` or any property
    outside d9: a version advanced by an append changes no hash, so a lake written to every minute
    costs no review until its schema or its stated meaning changes. With d13's `oml-catalog/1`, the server's change is four
    enum values.

- **d8 — Each schema change makes exactly the claims that state it stale, and a rename is followed
  by column mapping.** The diff is ADR-0057 d8's, by claim key, with these readings:

    | Change at a new version | Table claim | Schema claim | Column claim |
    |---|---|---|---|
    | data appended, deleted, optimised, vacuumed | unchanged | unchanged | unchanged |
    | column added | unchanged | changed | `new` if it carries a comment or a rule names it |
    | column dropped | unchanged | changed | `removed`, its retirement reviewed |
    | type widened, nullability changed | unchanged | changed | unchanged |
    | comment set or edited on a column | unchanged | unchanged | `new` or `changed` |
    | table description or d9 property edited | changed | unchanged | unchanged |
    | partitioning or clustering changed | changed | changed | unchanged |
    | column renamed, column mapping on | unchanged, unless it is a partition or clustering column | changed | `changed`: the same key (its physical name did not change), a new `col=` fragment and text |
    | column mapping enabled on an existing table | unchanged | changed (`column_mapping`) | unchanged: the upgrade gives existing columns their old names as physical names, so keys hold |
    | table recreated: `metaData.id` differs from the claim's, or the version is below the anchored `rev` | `recreated` for every claim of the table: nothing is written and the run exits 1; `--accept-recreated` writes the changes as `changed` | | |

    Renames therefore cost one supersession of the column claim, which the reviewer reads as "the
    meaning still holds under the new name", and its `about` terms are carried as ADR-0051 d3
    carries them. A table without column mapping cannot rename a column (the protocol's reason
    for the feature), so no rename is missed; a drop and an add of the same name reuse the key
    and read as `changed` after a retirement. A version below the anchored `rev` is ADR-0057
    d20's rollback signature, and `recreated` is its lake form. `moved` (ADR-0057 d18) is not used
    for lakes in this record: a table copied to a new location is `removed` there and `new` here,
    and pairing by `metaData.id` is #161.

- **d9 — Purpose, owner, grain and freshness come from a closed list of table properties, and every
  other property is never read.** The table claim reads `metaData.description` as its purpose,
  and from `configuration` only `oml.owner`, `oml.grain` and `oml.freshness` (each at most 500
  characters, a stated cadence such as "daily by 06:00 UTC", never an observed one). Writers set
  them with their engine's `ALTER TABLE … SET TBLPROPERTIES`; where they are absent, the catalog
  importer's claims carry what the catalog knows (d13). `delta.columnMapping.mode` is read for
  d6. No other configuration key is read, because table properties are where engines leave
  storage options, and a secret has been written there before. Observed freshness, the last
  version and its time, is not claim text: it changes with every commit. It is the source check's
  `rev` (d11), and `oml sync --check` prints the last commit's time from the local read without
  sending it.

- **d10 — A source is a lake root and a reviewed file that says what under it is loaded; only
  `delta/` claims of a table proven gone are retired.** *Amends ADR-0057 d6.* A `uri` source is
  read on the client from object storage, never from a git tree, so ADR-0057 d6's git rules do
  not apply to it; what governs the run is a sync file, `oml-lake.toml`, committed in a git
  repository and read at its own `--rev` like any synced document, so a change to what is loaded is
  a reviewed pull request. It names the lake root, `include` and `exclude` globs over table paths,
  the discovery depth (default 3 prefixes below the root), the workarea per prefix (d17), the
  mapping rules (d14), and it refuses a key that looks like a
  credential (d16). A table is a prefix with a `_delta_log/` child, found by delimiter listing.
  Retirement is ADR-0061 d10's, in the `delta/` namespace: a claim whose table is excluded is out
  of the source and never retired; a claim whose table was `unreadable`, `unsupported` or listed
  with an error is reported and never retired; a table is `removed` only when a listing of its
  parent prefix succeeded and no longer shows it, and a column claim only when the table was read
  and the column is absent at that version.

- **d11 — An unchanged lake claim is re-checked at most once a day, and a table whose version did
  not move is not fetched.** *Amends ADR-0057 d5,* whose run records a source check for every
  anchored tip it compares. A lake run first lists each table's `_delta_log/` from the anchored
  `rev`; a table with no newer commit records nothing and fetches nothing. A table with newer
  commits is read; a claim found `unchanged` records a check `fresh` at the new version only when
  its latest check is older than `--recheck-after`, 24 hours unless set, so a lake synced hourly
  records about one check per claim per day; any other outcome records its check at once. The
  stale mark still comes from the source alone. The cost: `source_checked.rev` may trail the
  table's newest version by a day while nothing it states changed. The client already batches:
  #68's sync sends source checks at most 4 in flight, 600 checks in 9.5 s. A server route that
  takes a batch stays #161, to be built before a 2,000-table lake (*accepted at the
  recommendation, 2026-11-05*).

- **d12 — A schema claim, and a table claim with no stated meaning, may be approved by a key that
  re-reads the log at the anchored version; everything that states meaning goes to a person.**
  *Applies ADR-0047 d8 and d9 and ADR-0061 d12.* The re-deriving reviewer is ADR-0047 d8's CI
  key: a dreamer `held_by: agent`, never `service`, held in the operator's CI next to read-only
  lake credentials, never on the server. It runs the same reader at the anchor's `uri` and `rev`
  and approves a held claim only when its text and hash are byte-identical to its own
  rendering, rejecting with `mismatch` otherwise. It may approve: schema claims, and table claims
  whose text holds no `Purpose`, `Owner`, `Grain` or `Freshness` line. It never approves a column
  claim, a table claim with any of those lines, a catalog claim (d13), an edge or a term. The
  version must still be readable: a version vacuumed out of the log's retention cannot be
  re-derived and goes to a person. Until ADR-0047 d8's key is built, a person approves every
  batch through ADR-0055's listing, as ADR-0061 d12 has it for crates.

- **d13 — Catalog importers are a fetch step on the client and a pure adapter over its snapshot;
  catalog lineage becomes `derived_from` edges proposed for ratification.**
    - **Fetch.** `oml lake fetch <catalog> --out <file>`, in the lake build, reads a catalog's
      API under the client's credentials and writes one JSON snapshot: for each dataset, its
      qualified name, storage location, platform, description, owners, tags, glossary terms,
      domain, columns with descriptions, tags and glossary terms, foreign keys, and upstream
      datasets, and nothing else; no sample, profile, query log or usage statistic is requested.
      DataHub through its GraphQL API, OpenMetadata through its REST API, Glue through
      `GetDatabases` and `GetTables` signed by the client's AWS credentials. Hive metastore's
      Thrift API follows in the same phase, after the other three (d21); until then a Hive
      deployment that keeps Delta tables is read by the `delta` adapter from their locations.
    - **Adapt.** `oml sync <snapshot> --workarea <w> --adapter datahub|openmetadata|glue` is a
      pure function of the snapshot's bytes (ADR-0057 d7). Per dataset with a description, owner,
      tag or glossary term: one claim keyed `catalog/<catalog>/<qualified name>/table`, its text
      the catalog's statements and the storage location; per column with a description or a
      glossary term: one claim keyed `catalog/<catalog>/<qualified name>/col/<path>`. Their anchor
      is the catalog entity's API URL as `uri` (query and credentials stripped) and the catalog's
      own version of the entity, or the snapshot's SHA-256 prefix where it has none, as `rev`; the
      hasher `oml-catalog/1` covers the stated facts as lines. A catalog's glossary terms on a
      dataset or column become that claim's `about` (d14). Catalog claims never merge into the
      `delta` adapter's: the two sources state different things, a reviewer and a reader see both,
      and a disagreement between a table property and the catalog shows as two claims.
    - **Owners are groups only.** *Decided by the operator on 2026-11-05.* The fetch keeps an
      owner that the catalog records as a group or team, by the group's name, and drops every
      owner that is a person: no username, email address or phone number reaches the snapshot.
      A dataset owned only by people has no owner line.
    - **Edges.** The snapshot's upstream datasets become edge statements `{from: the downstream
      table claim, label: derived_from, to: the upstream table claim}`, and foreign keys
      `references` between table claims. `oml sync <snapshot> --edges` proposes them under a
      person's dreamer key once both ends are promoted, and `--edges --ratify` under a second
      dreamer key re-derives them from the snapshot and ratifies, exactly as ADR-0057 d10 does for
      `amends`. `derived_from` is an open label (ADR-0046 d6) stating data lineage between tables;
      it is not the submission field `derives_from`, it does not enter the lineage closure of R2,
      and it confers nothing.

- **d14 — Terms attach to tables and columns by deterministic rules and catalog assignments,
  proposed in the write and ratified by a person; nothing fuzzy, and no model.**
    - **Vocabularies.** ADR-0051 d1's facets, as vocabulary workareas: `biz:` for a business
      glossary, imported client-side from a catalog's glossary by ADR-0051 d10's import (a
      DataHub or OpenMetadata glossary maps term by term: name to `pref_label`, synonyms to
      `alt_labels`, definition to `definition`, parent term to an `oml:is_a` proposal, the
      catalog's term id as the term claim's anchor); and `pii:` for privacy classes, from a seed
      OML ships beside ADR-0051's `software-project` seed, `data-privacy` (email, phone, person
      name, postal address, date of birth, national id, payment card, account number, IP address,
      precise location, free text about a person), each held for a curator.
    - **Rules.** `oml-lake.toml`'s `[[terms]]` rules map columns and tables to CURIEs: an exact
      column name, a glob over names (`*_email`), optionally a type (`string`), a table path
      glob, or a catalog term id. They are evaluated in file order, at most three CURIEs per
      claim (ADR-0051 d4's cap), and a column a rule names gets a column claim whose meaning is
      the term's preferred label and definition. A rule naming a CURIE that is not an active term
      is a run error under `strict` and a warning under `annotate`, Cognee's two modes taken as the
      sync file's `term_policy`; `strict` with no vocabulary at all is refused.
    - **Proposed in the write.** The sync key sends the CURIEs as the claim's `about` (ADR-0051
      d3), so the terms are reviewed with the claim by a person and ratified by its verdict. The
      CURIEs enter `oml-delta-col/1`, so a rule change that changes a column's terms is a
      supersession of that column claim.
    - **Classifying what is already promoted.** A new glossary term or rule that would attach a
      term to many promoted claims without changing them is `oml lake classify`, under a key with
      ADR-0027's `classifier` profile: it proposes `oml:about` edges on promoted versions (ADR-0027
      d6), which a `curator` ratifies or rejects (d8, d12). This unparks ADR-0027 d1–d6, d8 and
      d12–d14 for lake vocabularies, and *amends ADR-0046 d6*: a standalone `oml:about` proposal
      is admitted from a `classifier` key and refused from every other key, as before.
    - **The crew.** One ontology crew per lake, `lake-ontology` (ADR-0027 d1, d2), whose
      `term_creator` key runs glossary imports and seeds, whose `classifier` key runs `oml lake
      classify`, and whose `curator` keys, held by people, review terms and ratify
      classifications. The sync key is a writer on the lake's workareas and no member of the crew,
      so no key both writes a column claim and ratifies its terms (R2).

- **d15 — No value ever leaves the lake: samples are not built.** *Decided by the operator on
  2026-11-05.* Column names, types, comments and terms are metadata and are written; a name such
  as `email` or `ssn` is fine. A value never is: the reader opens no data file (d2), reads no
  partition value and decodes no statistic, and no code in the lake build reads a data file.
  `oml-lake.toml` has no samples section, and an unknown section is refused. The enumeration case
  (`status in (open, paid, refunded)`) is served by a column comment or a catalog description that
  states it, reviewed as meaning. Revisited only on a concrete need (rejected alternatives).

- **d16 — Credentials never leave the client and never enter OML.** The lake build reads
  credentials through each cloud's standard chain (environment, profile, workload identity,
  instance metadata), as `object_store` resolves them, and catalogs' tokens from the environment
  or a file named on the command line. `oml-lake.toml` refuses any key whose name matches the
  secret-key list of ADR-0056 d6, so a credential cannot ride a reviewed file. Every anchor is
  stripped by d5's grammar on the client and refused on the server if anything slips: no user
  information, no query (where ADLS SAS tokens and pre-signed URLs carry their signatures), no
  fragment. Table properties outside d9 are never read. ADR-0057 d6's refusal of the public
  server without `--allow-public-server` applies, and the lake build adds to its warning that
  bucket names and table paths are kept for good. No route, setting or tool lets the server reach
  a lake (rejected alternatives).

- **d17 — A lake loads under one parent with one workarea per domain and one per catalog.** Under
  the operator's user root `e8kor` (ADR-0044): a parent `lake`; one child `lake-<domain>` per
  domain prefix of the lake root, which the `delta` adapter fills; one child `lake-catalog-<catalog>`
  per catalog, which its importer fills; and the vocabulary workareas `lake-glossary` (`biz:`) and
  `lake-privacy` (`pii:`) named by the crew. Workarea names are tenant-unique RFC 1035 labels
  ([ADR-0026](ADR-0026-names-are-keys.md) d2), so the operator's `e8kor.lake.<domain>` is this
  path, not a name. Recall at `lake` reads every child; each sync diffs only its own workarea
  (ADR-0057 d8). Keys: one writer key per adapter and lake, `e8kor-lake-sync` and
  `e8kor-catalog-<catalog>-sync`, since a writer supersedes only its own tips; the crew's keys
  (d14); the CI key (d12); and the operator's two dreamer keys for review and edges.

- **d18 — At 2,000 tables a load is about 4,000 mechanical claims and as many human reviews as
  there are stated meanings; afterwards, review follows schema and meaning changes, not data.**
    - **Claims.** Two per table (table and schema, more for tables over 666 columns) plus one per
      described or rule-named column: `4,000 + D` for `D` described columns. `D` is the lake's
      own number and unmeasured (#161); a lake where one column in ten of a 30-column average is
      described has `D = 6,000`, an illustration, not a measurement.
    - **Review.** Schema claims, and table claims with nothing stated, go to the CI key once it
      exists (d12) and otherwise to a person: 20 bulk requests of 200 (ADR-0057 d9). Every table
      claim with a stated meaning, every column claim and every catalog claim goes to a person, in
      batches of 200. At `D = 6,000` that is 30 requests of column claims alone; the operator
      loads by domain (d17), largest first, so the review is spread over runs.
    - **Requests to object storage.** Per table read: one listing of `_delta_log/` from the
      anchored version, one `_last_checkpoint`, a footer and three column-chunk ranges of the
      checkpoint, and one `GET` per commit since it, bounded by the writer's checkpoint interval.
      An unchanged table costs one listing (d11). At 2,000 tables a first load is in the tens of
      thousands of requests and a day's resyncs about 2,000 listings per run; the time and money
      are unmeasured (#161).
    - **Requests to OML.** One `POST` per written claim and one source check per changed or
      day-old claim (d11), each honouring `Retry-After`.
    - **Afterwards.** Appends, deletes and compactions cost nothing but checks; a schema change
      costs one mechanical review, a rename or a comment one human review, and a description or
      property change one human review. How often a real lake does each is unmeasured (#161).

- **d19 — A generated test lake in the repository is the golden fixture and the eval's corpus.**
  `testing/lake/` holds the generator (d3, its own cargo workspace on `deltalake`, never built by
  CI), the committed tables, `oracle.json`, three catalog snapshots, an `oml-lake.toml` with term
  rules, and the eval set, under 2 MiB in all. Domain `shop`:

    | Table | Exercises |
    |---|---|
    | `shop/orders` | partitioned by `order_date`; column mapping `name` from creation; comments on most columns; v2 adds `discount` with a comment; v4 renames `amount` to `total_amount`; v5 drops `legacy_flag`; a checkpoint at v3 so the schema is read through Parquet |
    | `shop/customers` | description and `oml.owner`, `oml.grain`, `oml.freshness`; PII-named columns (`email`, `phone`, `full_name`, `birth_date`); no column mapping until v2, then a rename of `phone` |
    | `shop/order_items` | no description, no property, no comment: the mechanical-only case of d12 |
    | `shop/events_raw` | 700 columns with nested structs, a map and an array: the split schema claim and dotted paths |
    | `shop/returns_legacy` | deleted after its first load: retirement |
    | `shop/daily_kpis` | recreated at version 0 with a new `metaData.id`: `recreated` |
    | `edge/catalog_managed`, `edge/zstd_checkpoint`, `edge/v2_checkpoint`, `edge/log_gap` | hand-written logs for `unsupported`, codec, sidecar-free reading and `unreadable` |

    Every table also carries a small data file with synthetic values, so a test can prove they
    are never opened. The catalog snapshots (`datahub.json`, `openmetadata.json`, `glue.json`)
    describe the same tables, with a glossary, owners, a disagreement with a table property,
    lineage `orders ← order_items`, and a foreign key `orders.customer_id → customers.id`. The
    **eval** (`eval/recall/lake/`) is a set of queries written before the claims are generated, in
    four categories with their expected claim keys: which table holds a fact ("where are refunds
    recorded"), what a column means ("what is `discount`"), a renamed column ("where did order
    amount go"), and which columns hold personal data (by `about: pii:`). It runs on the recall
    eval's harness (ADR-0054) against the promoted fixture, and its results are published with the
    phase that ships them; no target is set before a measurement (#161).

- **d20 — Every new failure has a doctor line.** `oml doctor` names: a CLI without the lake build
  when the clone's `oml-lake.toml` exists; a server that does not accept the `s3` or `gs` schemes,
  the `col=` fragment or the four hashers (before the phase that adds them), which the sync also
  refuses before reading anything; lake credentials the chain cannot resolve, naming the chain
  tried and never a value; an `oml-lake.toml` key that looks like a credential; for the key's
  default workarea, the counts of tables last reported `unsupported` (with the feature),
  `unreadable` and `recreated`; claims whose latest check is older than twice `--recheck-after`;
  a `term_policy` of `strict` with an empty vocabulary; rules naming inactive terms; and catalog
  edges waiting for a second key.

- **d21 — The build, in four phases on v0.6.x, each closed by named tests.** v0.6.0 stays ADR-0030's.
    1. **v0.6.1 — the reader, the test lake and the server's grammar.** Server: d5's schemes,
       `rev`, `col=`, d7's hashers and `oml-catalog/1`; `anchor_uri_prefix` on recall, the listing and `oml_read`
       (d22). CLI lake build on all three targets, size measured (#161); `--adapter delta` over
       a local lake with `--uri-base`; the test lake and its oracle. Tests:
       `the_reader_matches_the_delta_rs_oracle_at_every_fixture_version`,
       `no_object_outside_delta_log_is_ever_opened`,
       `commit_info_add_and_remove_are_never_decoded`,
       `a_checkpoint_is_read_by_projection_without_its_add_column`,
       `an_unknown_reader_feature_is_unsupported_by_name`, `a_catalog_managed_table_is_refused`,
       `a_log_gap_is_unreadable_and_retires_nothing`,
       `an_anchor_with_user_information_a_query_or_a_local_path_is_refused`,
       `adls_locations_are_written_as_their_https_endpoint`,
       `table_keys_that_differ_only_in_case_never_collide`,
       `the_default_cli_refuses_the_delta_adapter_naming_the_lake_build`.
    2. **v0.6.2 — object storage, staleness and re-derivation.** S3, ADLS and GCS through the
       chains; d8's table; d10's discovery and retirement; d11's cadence; the CI key's
       re-derivation where ADR-0047 d8's key exists. Tests:
       `an_append_changes_no_hash`, `a_renamed_column_keeps_its_key_and_is_changed`,
       `enabling_column_mapping_keeps_every_column_key`,
       `a_type_widening_changes_only_the_schema_claim`,
       `a_dropped_column_retires_its_claim`, `a_recreated_table_writes_nothing_and_exits_1`,
       `an_excluded_or_unreadable_table_is_never_retired`,
       `an_unchanged_table_is_listed_and_not_fetched`,
       `an_unchanged_claim_is_rechecked_at_most_once_per_recheck_after`,
       `the_ci_key_approves_a_schema_claim_only_on_byte_equality`,
       `the_ci_key_never_approves_a_claim_that_states_meaning`,
       `only_d9_properties_are_read`.
    3. **v0.6.3 — catalogs and lineage.** `oml lake fetch` and the `datahub`, `openmetadata` and
       `glue` adapters, then Hive metastore's Thrift fetch; `derived_from` and `references` edges.
       Tests:
       `each_catalog_adapter_is_a_pure_function_of_its_snapshot`,
       `the_fetch_requests_no_sample_profile_or_usage`,
       `owners_are_groups_and_never_a_person`, `catalog_lineage_is_proposed_only_between_promoted_table_claims`,
       `a_catalog_and_a_table_property_that_disagree_are_two_claims`.
    4. **v0.6.4 — ontology.** The `data-privacy` seed; glossary import; `[[terms]]` rules and
       `term_policy`; `oml lake classify` and ADR-0027's profiles. Tests:
       `a_rule_attaches_at_most_three_terms_and_never_fuzzily`,
       `a_rule_naming_an_inactive_term_fails_under_strict`,
       `a_rule_change_supersedes_the_column_claim`,
       `only_a_classifier_key_proposes_a_standalone_about_edge`,
       `the_sync_key_is_no_member_of_the_ontology_crew`.
       And in every phase: `no_data_file_is_ever_opened_by_any_lake_command`,
       `a_samples_section_in_oml_lake_toml_is_refused`.

    And against the lead, in `cli_e2e`, `oml_sync_loads_the_test_lake_and_an_append_writes_nothing`.

- **d22 — Each new call traces to a use case, and no MCP tool is added.**

    | Call | Use case |
    |---|---|
    | `oml sync <lake uri> --adapter delta`, `--uri-base`, `--recheck-after`, `--accept-recreated`; `oml-lake.toml` | UC-SM-07, load a lake's table descriptions and keep them in step |
    | `oml lake fetch`, `--adapter datahub|openmetadata|glue`, `--edges` for `derived_from` and `references` | UC-SM-08, import what a catalog says about the lake |
    | `anchor_uri_prefix` on recall, the listing, `oml_read` and `oml read` | UC-SM-09, find what is known about a table or a lake prefix (ADR-0057 d4's deferred `uri` filter, whose first use case this is) |
    | `[[terms]]` rules, `about` in a sync's write, `oml lake classify` | UC-SM-10, find the columns a reviewed term describes |

    `oml_read` gains one argument; the catalog stays three tools (ADR-0039 d8).

- **d23 — Phase 1 as built, and where it reads d1 to d22 differently (2026-11-06).** The operator
  asked for the reader, the `delta` adapter, the test lake and staleness first, without catalogs or
  the ontology. Built: `oml-delta-log` (d2 to d4), `oml sync <lake directory> --adapter delta` in the
  `lake-sync` build (d3, d6 to d8), the three hashers and the anchor grammar on the server (d5, d7),
  `testing/lake/` (d3, d19). Where the build reads the design differently, each is stated here so the
  design and the code agree:
    - **A local lake only.** `<lake uri>` is a directory or a `file://` URL, found by delimiter
      listing of the disk; `s3://`, `gs://` and `abfss://` locations are refused by name until v0.6.2.
      The anchor never carries the directory: a lake on a disk is always read with `--uri-base`,
      and a run without it is refused by name, so no machine's home path is stored. The operator
      admitted `file:///<path>` for a test lake, so the server's grammar accepts it beside d5's
      three schemes, and it reaches the server only when the operator writes it in `--uri-base`.
    - **The reader is bounded.** A log file name sizes no allocation, a version uses checked
      arithmetic, a panic inside the parquet crate makes that table `unreadable`, a symlink is never
      followed, and the commit, line, footer, projected-column, column and file counts have named
      caps (`Limits`), each refused as `unreadable` naming the cap. A column under column mapping
      is keyed by the physical names of its whole path, so two nested leaves called `city` keep
      both claims and turning mapping on re-keys none.
    - **`https` keeps its trailing slash.** d5's "no trailing `/`" is enforced for `s3`, `gs` and
      `file`; an `https` page anchor ending in `/` was valid under ADR-0057 d3 and stays valid.
    - **A column's key without column mapping** is its dotted logical path, mapped and fitted as the
      stem is; with column mapping it is the leaf's physical name, as d6 says.
    - **`recreated` is a rollback until phase 2.** A table read at a version below the one a claim
      was last known at is reported `rollback`, writes nothing and exits 1; d8's `metaData.id`
      comparison and `--accept-recreated` come with the rest of d8's table in v0.6.2.
    - **A check per version, not per day.** An unchanged claim records a `fresh` check when the table's
      version differs from the one its latest check or anchor names, and a table whose version did
      not move records nothing; d11's `--recheck-after` cadence comes with object storage.
    - **Own rows only.** The run lists the key's own claims; another key's claim under a lake key is
      the server's `submission.claim_key_conflict`, reported by the write. `anchor_uri_prefix` on recall
      and the listing (d22) is not built; the anchor's `rev` is the Delta version and its `hasher`
      one of d7's three.
    - **Not yet built, by phase:** `oml-lake.toml`, `--recheck-after`, retirement of a removed table
      and `doctor` lines (v0.6.2, d10, d20); `oml lake fetch` and the catalog adapters (v0.6.3); the
      ontology (v0.6.4). `oml-lake`'s release artifact waits on #161's size measurement; CI prints
      the size the lake build adds on each target.

## Contract sketch

**Anchor** (ADR-0057's schema, changed fields only):

```yaml
uri:      { type: string, maxLength: 2048,
            pattern: '^(https://[^@#?\s]+|s3://[a-z0-9][a-z0-9.-]{1,61}[a-z0-9](/[^@#?\s]+)?|gs://[a-z0-9][a-z0-9._-]{1,220}[a-z0-9](/[^@#?\s]+)?)$' }
fragment: { description: 'A slug, dN, L<n>, L<n>-L<m> or col=<pct-encoded path>; page=<n> reserved.' }
hasher:   { enum: [oml-md/1, oml-rustdoc/1, oml-crate/1, oml-delta-table/1, oml-delta-schema/1, oml-delta-col/1, oml-catalog/1] }
```

**Recall and listing** gain `anchor_uri_prefix` (a byte prefix of `uri`, at most 2,048 bytes),
combined by AND with the other filters, and leaving out unanchored rows and `repo` anchors.

**`oml-lake.toml`**:

```toml
lake       = "s3://acme-lake/warehouse"
depth      = 3
term_policy = "strict"            # or "annotate"

[[source]]
prefix   = "shop/"
workarea = "lake-shop"
exclude  = ["shop/tmp_*"]

[[terms]]
column = "*_email"
type   = "string"
about  = ["pii:email"]

[[terms]]
catalog_term = "urn:li:glossaryTerm:Revenue"
about        = ["biz:revenue"]
```

**CLI** (lake build):

```text
oml sync <lake uri> --workarea <w> --adapter delta [--config oml-lake.toml] --uri-base <uri>
         [--dry-run | --check [--record]] [--json] [--max-writes <n>] [--recheck-after <dur>]
         [--accept-recreated] [--allow-public-server]
oml lake fetch datahub|openmetadata|glue [--endpoint <url>] --out <snapshot.json>
oml sync <snapshot.json> --workarea <w> --adapter datahub|openmetadata|glue [--edges [--ratify]]
oml lake classify --workarea <w> [--dry-run]
oml read --query <q> --anchor-uri-prefix s3://acme-lake/warehouse/shop/orders
```

A dry run over the test lake, as it would print (lines elided):

```text
$ oml sync testing/lake/tables --uri-base s3://oml-test-lake --workarea lake-shop --adapter delta --dry-run
source    s3://oml-test-lake  adapter delta  hashers oml-delta-table/1, oml-delta-schema/1, oml-delta-col/1
workarea  lake-shop  key e8kor-lake-sync
new       delta/shop/orders/table           s3://oml-test-lake/shop/orders @ 5
new       delta/shop/orders/schema          s3://oml-test-lake/shop/orders @ 5
new       delta/shop/orders/col/col-3f9a…   s3://oml-test-lake/shop/orders @ 5 #col=total_amount
…
unsupported edge/catalog_managed           reader feature catalogManaged
recreated   delta/shop/daily_kpis/table    metaData.id changed at version 0; --accept-recreated writes it
plan      … new, 0 changed, 0 removed; 1 unsupported, 1 unreadable, 1 recreated; this run writes …
```

## Consequences

- **Costs.** Server: two `uri` schemes, one fragment form, four hasher values, one filter; nothing
  else, and no lake access. Client: a new crate and a second CLI artifact per target with an
  async runtime and three cloud clients, size unmeasured. Review: about 4,000 mechanical claims
  and every stated meaning at 2,000 tables, one human review per rename, comment or description
  change afterwards. Bucket names and table paths are kept for good.
- **Open work.** #161 (the lake build's size and build on the three targets, the reader's
  coverage of real-world logs: Spark-, Databricks- and delta-rs-written tables, described-column
  counts and schema-change rates of a real lake, which set the review burden, object-storage
  requests, time and cost of a 2,000-table sync, the lake eval's results, a table moved to a new
  location paired by `metaData.id`, a batched source-check route, the throughput of ADR-0020's
  row import streamed from the client).
- **Use cases.** UC-SM-07 to UC-SM-10, added to the [use cases](../product/use-cases.md#sources)
  and the [call map](../engineering/use-case-call-map.md#sources).
- **Pages to change at the build.** openapi.yaml's `Anchor`, recall and listing; mcp-tools.json's
  `oml_read`; the CLI reference (`--adapter delta|datahub|openmetadata|glue`, `oml lake`, the
  states `unsupported`, `unreadable`, `recreated`); the release workflow and the install page
  (the lake artifact); ontologies (the `data-privacy` seed, classifier keys); roles and keys
  (ADR-0027's profiles); the plugin's skill (that `delta/…` keys describe tables and
  `anchor_uri_prefix` finds them); and at acceptance the Status lines of ADR-0020, ADR-0046,
  ADR-0057 and, on their branches, ADR-0027, ADR-0051 and ADR-0061.

## The operator's answers of 2026-11-05

| # | Question | Answer |
|---|---|---|
| 1 | ADR-0020's row import and the server's lake access | Decided: the client reads and streams the rows; the server never touches a client lake and only receives (d1 amends ADR-0020 d7). |
| 2 | The lake build's form (d3) | Decided: a second `oml` artifact built with `lake-sync`, released as `oml-lake`; the default CLI is unchanged. |
| 3 | Workarea and crew names (d17) | Accepted at the recommendation, 2026-11-05: `lake`, `lake-<domain>`, `lake-catalog-<catalog>`, `lake-glossary`, `lake-privacy`, `lake-ontology`. |
| 4 | Source checks at scale (d11) | Accepted at the recommendation, 2026-11-05: daily re-checks through the client's batching (#68: at most 4 in flight, 600 checks in 9.5 s), and a server batch route (#161) before a 2,000-table lake. |
| 5 | Catalog owners (d13) | Decided: groups only, never a person's username. |
| 6 | The `oml.` table properties (d9) | Accepted at the recommendation, 2026-11-05: `oml.owner`, `oml.grain`, `oml.freshness`. |
| 7 | Samples (d15) | Decided: not built; a rejected alternative, revisited only on a concrete need. |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **The server reads the lake** (a crawler with stored lake credentials) | The server would hold every tenant's lake credentials and make outbound requests on their behalf, authoring memories under no key, so R1 and R2 have no author to check; ADR-0057 rejected the same for repositories. The client already holds credentials, and its writes are a key's. |
| **Sample values by default** | A value is data; one sample of a PII column puts personal data into a memory kept for good. |
| **Sample values on a per-table opt-in** (this record's first reading of d15) | *Rejected by the operator on 2026-11-05.* It is the only code that would open a data file, a reviewed opt-in still writes values into memories kept for good, and the enumeration case is served by a comment or catalog description that states the values as meaning. Revisit only on a concrete need. |
| **Per-file statistics** (min, max, null counts from `add.stats`) | They are values (the min of `email` is an email address), they change with every write, and they would make every append a claim change. The reader never decodes `add`. |
| **Descriptions written by a model on the server** | No model runs on the server (ADR-0051 d1, ADR-0048), a model's description of a column it has not seen the data of is a guess, and a reviewer of model prose adds little (ADR-0047 d9). An agent may still write a column's meaning with an anchor, reviewed like any memory. |
| **A claim per column, for every column** | At 2,000 tables of 30 columns, 60,000 claims a person reviews to learn a name and a type the schema claim already states, and a review per type change. A column claim exists only where someone stated a meaning. |
| **OML as a full catalog** (search UI, profiling, lineage graph, access policies) | DataHub, OpenMetadata, Glue and Unity already are; OML imports what they state as reviewed claims an agent can recall, and stays a memory provider. |
| **delta-rs or `delta_kernel` in the shipped CLI** | Both are table readers over arrow; the minimal reader needs four actions and no data path; delta-rs with `datafusion` resolved more packages than OML, and its build failed here. Both stay usable as the oracle. |
| **Shelling out to `aws`, `az` or `gsutil`** | Depends on what the client has installed, behaves differently per version and per OS, and turns a pure reader into one that parses another program's output. |
| **Hand-written signing over `ureq`** | The signing is small; the credential chains (profiles, SSO, workload identity, instance metadata) are not, and getting one wrong is a security defect. |
| **Freshness as claim text** | It changes on every commit, so every write to the lake would be a review. The cadence a team states is text; the observed version is the source check. |
| **Keying columns by logical name** | A rename would read as a removal and an addition, retiring the stated meaning and asking for it again; the physical name survives a rename by the protocol's design. |
| **Keying columns by the column-mapping id** | Columns of a table without column mapping have none, and enabling it would change every key; the physical name is the old name for upgraded columns and so carries both cases. |
| **A `moved` source check for renames** | The text names the column, so a recall by the new name would miss a claim that still reads the old one; a supersession costs one review and keeps the text true. |
| **Fuzzy matching of names to terms** (Cognee's `difflib` at 0.8) | It attaches terms nobody reviewed, and ADR-0051's eval found tags attached with no judgement gain nothing measurable while wrong tags cost nDCG@10 −0.106. Rules are exact, written in a reviewed file. |
| **Merging catalog and table-property statements into one claim** | It hides a disagreement a reader should see and makes one claim's review depend on two sources' versions. |
| **`derives_from` for data lineage** | It is lineage of reasoning and widens the reviewer-exclusion closure, so whoever described an upstream table could never review a downstream one. A `derived_from` claim edge states the data lineage and confers nothing. |
