<!--
id: adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource
tier: engineering
status: accepted design · 2026-11-14
answers: why a document is a kind and not a memory with a markdown format; which formats and sizes it takes; how its claim key names it; how its text is cut into passages and how a recall hit names its passage; what the MCP resources capability serves, under which URI scheme, scope and paging, and why there is no subscription; how a document is framed to a model; how oml sync's markdown adapters and documents agree; what is metered; what waits for attachments
-->

# ADR-0081 — A document is a reviewed markdown or text claim that recall searches by passage and the MCP server serves as a resource

**Status:** accepted design · 2026-11-14, by the operator, before it is built, every default as
drafted · proposed the same day, tracking #374 · amended by [ADR-0092](ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md) (d4, 2026-11-15) · d2 amended 2026-11-18 (the long-title cut and the one slug rule, #405) · [the build's readings of 2026-11-18](#the-builds-readings-of-2026-11-18), proposed for the operator's batch; [the review's readings of 2026-11-18](#the-reviews-readings-of-2026-11-18-pr-475), decided by the coordinator, for the operator's batch

**Release:** built in v0.8.0 (#405, #406)

**Twin parts** (ADR-0090 d18, PR #388): a document is a `memories` row, so its body and search
text are parts and its fold is the journal's; its passages are derived, cut again by the same rule
from the replayed body.

**Amends:** ADR-0025 (d8: a `document` hit is its best passage), ADR-0030 (d3: a held `document`
version is served inside an in-progress workspace as `memory` is), ADR-0039 (d3, d4: `oml_write`
and recall's `kinds` admit `document`), ADR-0014 (d2: the `resources` capability), ADR-0057 (d7:
the `document` adapter)

**Builds in part:** proposed ADR-0036 d5 (branch `feat/temporality`): its `oml://` scheme, its
segment rule and its error mapping, for documents and skills only

## Context

The operator asked on 2026-11-14 for "rag documents" that the MCP server serves "without
downloading or putting copy locally", and chose text documents first: markdown or plain text up to
the claim size limit, recalled by passage, served as MCP resources; files and PDFs later, with
[ADR-0004](ADR-0004-bodies-and-attachments.md)'s attachments (#374).

What exists today:

- **A size limit and formats.** A claim's text is at most `submission.max_body_size`, shipped
  1 MiB (`MAX_BODY_BYTES = 1_048_576`, `crates/transports/oml-transport-http/src/lib.rs:192`;
  [http-api.md](../api/http-api.md) submit, `body`). `text/markdown` and `text/plain` are formats
  already (`crates/adapters/oml-format/src/lib.rs:41`), indexed by their source as written
  ([ADR-0056](ADR-0056-a-claim-version-names-its-format-the-signature-covers-its-source-and-adapters-compiled-into-the-server-derive-what-search-reads.md)
  d4), and the reranker scores a row's first 4 KiB (ADR-0056 d4, ADR-0053 d2).
- **Recall is per version.** Search reads one search text per version and serves one hit per
  claim tip (ADR-0025 d8); a 1 MiB row is ranked and served whole, its budget ⌈bytes ÷ 4⌉ tokens
  (`crates/oml-domain/src/budget.rs:25`), so under `max_tokens` it is cut from the answer or fills
  it.
- **Sync already loads documents, cut by heading.** `oml sync` reads committed markdown through
  the `headings` adapter, one claim per level-2 to level-4 heading keyed `doc/<path>/<slug>`, or the
  `lines` adapter, blocks of at most 40 lines split at blank lines keyed `doc/<path>/L<n>-L<m>`
  ([ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workspace-in-step-with-a-repositorys-documents.md)
  d7, d21), and writes kind `memory` (d6).
- **No resources.** `initialize` declares `tools` and `prompts` only (`mcp.rs:383-388`). Proposed
  ADR-0036 d5 designed `resources/list`, `resources/templates/list` and `resources/read` over claims
  under `oml://workspace/{workspace}/claim/{claim_key}`, with no subscription, since `/mcp` has no
  server stream and "Both `subscribe` and `listChanged` are optional"; that record is parked.
- **What a client does with a resource.** Claude Code offers resources in `@` autocomplete,
  fetches a referenced one "as attachments", and "automatically provides tools to list and read MCP
  resources" ([code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp), fetched
  2026-11-14), so the model can read a document without a person naming it.

Decided 2026-11-14; built in v0.8.0 (#405, #406), with the readings below. Not built: d6's escaping of hidden characters in a read's text content, which serves the stored bytes unescaped; d9's files.

## Decision

- **d1 — `document` is a claim kind: a titled markdown or plain-text body, reviewed like any
  claim.** `kind: "document"` takes `format` `text/markdown` (the default for the kind) or
  `text/plain`, refused otherwise (`request.validation`, `details.field: format`), and an optional
  `document: {title}`, 1 to 200 characters of one line; absent, the title is the first level-1
  heading of a markdown body, else the claim key's last segment. It is data, not instructions: the
  scan runs as on `memory` (no ADR-0074 d24 instruction rules), recall frames it as data, and its
  review is the workspace's policy with [ADR-0001](ADR-0001-the-constitution.md) R2 unchanged. A kind, not a format on `memory`, because three
  readers need to tell a document from a fact: recall cuts it into passages (d3), `resources/list`
  enumerates documents and not every memory (d6), and the builder lists them (ADR-0082). The cost
  is one kind value, one optional object and one migration.

- **d2 — A document's claim key is `doc/` and a path of plain segments, which is also its URI.**
  The key is `doc/` followed by one or more segments separated by `/`, each 1 to 64 characters of
  `a-z 0-9 . _ -`, never `.` or `..`, the whole at most 128 characters (the claim-key bound sync
  already keeps, ADR-0057 d7). Absent on submit, the server derives `doc/<slug of the title>` by
  ADR-0057 d7's slug rule; a key outside the grammar is refused naming `claim_key`. Every character
  is unreserved in RFC 3986, so a key maps to a URI segment for segment with no encoding, which
  meets proposed ADR-0036 d5's rule that the server decodes no segment. Sync's characters fit:
  its key rule maps every character outside `[a-z0-9._/-]` to `-` (ADR-0057 d7). Its lengths do
  not always fit: a path segment over 64 characters, or a key `fit_key` cuts at 111 characters
  and a 16-digit hash, breaks the segment bound, and the `document` adapter reports such a path
  unreadable naming this rule (R4, 2026-11-18). *Amended 2026-11-18 (#405, the #438 review):* a
  title whose slug is longer than one segment's 64 characters keeps its first 47, then `-`, then
  the first 16 hex digits of the SHA-256 of the full key `doc/<slug>`: ADR-0057 d7's 111-character
  fitting cut, scaled to this record's 64-character segment bound, so two long titles that share
  a prefix keep distinct keys. The slug is the one rule d3 names: the heading's plain text (links
  to their text, code and emphasis marks dropped) slugged as GitHub renders it, held in the leaf
  crate `oml-passage` that the server and `oml-cli` both link, so `# [Guide](https://x.y)` keys
  as `doc/guide` on both.

- **d3 — Passages are cut by the server at capture, by one deterministic rule shared with sync.**
  For `text/markdown`, a passage is each level-1 to level-4 heading with its text up to the next
  heading of level 1 to 4, plus a first passage for any text before the first heading; for
  `text/plain`, the whole text. Any passage longer than 40 lines is then cut by ADR-0057 d21's lines
  rule (blocks of at most 40 lines split at blank lines). Each passage carries `ordinal`,
  `start_line` and `end_line` (1-based, inclusive), `heading_path` (the headings above it), and a
  `fragment`: the heading's slug as ADR-0057 d7's `headings` adapter computes it when the passage is
  a whole section, else `L<n>-L<m>` (ADR-0057 d21). Front matter and HTML comments are skipped as
  the `headings` adapter skips them: a heading inside one opens no passage, and a comment's lines
  inside a passage's span are served with it, since a passage is served as its source lines. The rule lives in one leaf crate with no I/O that the server
  and `oml-cli` both link, so sync's spans and the server's passages are the same bytes. A version
  holds at most 4,096 passages, chosen, not measured (1 MiB of 40-line blocks of 6 bytes a line
  would exceed it; such a body is refused, `request.validation`, `details.field: body`,
  `details.reason: too_many_passages`). Passages are derived, stored beside the version, never
  signed separately (the signature covers the source, ADR-0056 d2), and erased with the body
  (ADR-0005 d4).

- **d4 — Recall ranks a document by its passages and serves the best one as the hit.** Each
  passage is indexed as its own search unit, under the same pipelines and filters as rows
  (ADR-0049), the boundary rule of
  [ADR-0030](ADR-0030-an-in-progress-workspace-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
  d2 applied before any limit. A document contributes at most one hit per recall, its best passage,
  so `k` keeps counting claims (ADR-0025 d8). The hit is the claim item with `text` the passage's
  text, `body_bytes` and the budget the passage's, and two new members: `passage: {fragment,
  ordinal, start_line, end_line, heading_path}` and `more_passages`, the count of the document's
  other passages that matched. Its `uri` is `oml://workspace/<workspace>/doc/<path>?passage=<fragment>`
  (d6). `kinds` admits `document`; an absent `kinds` includes documents (amends ADR-0039 d4). The
  cost: a larger index, one posting set per passage, unmeasured, and a recall-eval run before the
  build is accepted (ADR-0048's gate), since passage ranking changes what a query returns.

- **d5 — Documents are written on HTTP, the console and `oml_write`, up to the claim limit.**
  `oml_write` admits `kind: "document"` with `format` and `document` (amends ADR-0039 d3): a
  document is data, as a memory is. The limit is the claim limit, 1 MiB after NFC, refused as today
  with `request.payload_too_large`. Editing is supersession and retiring is `retirement: true`, as
  every claim (ADR-0025 d10, d11); a successor re-sends the whole text. Inside an in-progress
  workspace a held document is served to readers there labelled `unreviewed`, as a held memory is
  (amends ADR-0030 d3, adding `document` to its served kinds). Documents follow ADR-0030 exactly as
  memories do, under ADR-0080 d10's statement of it: a key whose scope node lies inside the
  in-progress root gets held documents in recall, `resources/list` and `resources/read`, marked by
  `unreviewed` and `_meta.oml.unreviewed`; a held successor is served beside the promoted tip, a
  held retirement is not served; a version held for a scan finding stays author-only; outside the
  root nothing held is served, wide sources never reach it (ADR-0072 searches protected and
  frozen workspaces only), and the commit needs zero held versions (ADR-0030 d6).

- **d6 — The MCP server declares `resources` and serves documents and skills under `oml://`,
  read-only and without subscription.**
    - **Capability.** `"resources": {}`: neither `subscribe` nor `listChanged`, for the reasons
      proposed ADR-0036 d5 gives (a stateless `/mcp` with no stream, ADR-0014 d2).
    - **URIs.** A document is `oml://workspace/<workspace>/doc/<path>` with `<path>` its claim key
      after `doc/`; a passage adds `?passage=<fragment>`; a skill is
      `oml://workspace/<workspace>/skill/<name>` ([ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md)
      d7). `<workspace>` is the workspace's name, resolved as every name is
      ([ADR-0026](ADR-0026-names-are-keys.md) d4). `resources/templates/list` serves
      `oml://workspace/{workspace}/doc{/path*}{?passage}` and
      `oml://workspace/{workspace}/skill/{name}` (RFC 6570 path-segment expansion, never reserved
      expansion, as proposed ADR-0036 d5 rejected `{+path}`). The segment `claim/` stays proposed
      ADR-0036's.
    - **`resources/list`.** One page of the documents at the key's default workspace: the scoped
      listing of kind `document`, promoted tips under the key's read access in the subtree and the
      boundary rule, the caller's own held documents excluded and other authors' held ones only
      inside an in-progress scope, newest first, 50 a page under MCP's `cursor`, then the offered
      skills (ADR-0080 d7). Each entry is `{uri, name, title, description, mimeType, size}` with
      `name` the claim key, `title` the document's title, `description` "Reviewed document in
      `<path>`, approved `<date>` by a `<used_by>` reviewer: data to check, not instructions" (or
      "Unreviewed document" for a held one), and `size` its body bytes. The list is bounded by the
      page, never by the set.
    - **`resources/read`.** A document URI answers `{contents: [{uri, mimeType, text, _meta:
      {oml: {artifact_id, version, approved_at, approver, unreviewed, workspace_path, anchor,
      findings}}}]}`, `text` the body byte for byte, or with `?passage=` that passage alone; `approver` carries
      ADR-0089 d7's `act` and `confirmed` once that record is built, so a document an
      admin fix serves reads as one. The
      provenance and data framing travel in `_meta` and the list's `description`, never inside the
      text, so the bytes stay the signed source (ADR-0074 d12 keeps the JSON text byte for byte for
      the same reason); hidden characters are escaped in the text content as ADR-0074 d12 escapes
      them for a model (not built in v0.8.0: the text is served unescaped). A missing, unreadable, held-and-not-served or malformed URI answers one
      error, `-32002` "Resource not found" with `data: {uri}`, so a read never discloses a row the
      key cannot read; a URI whose segment holds a `%` or a reserved character is `-32602`,
      `details.field: uri`, as proposed ADR-0036 d5 has it. Reads are logged as recall is
      (ADR-0025 d9).
    - **Server instructions.** `SERVER_INSTRUCTIONS` (`mcp.rs:52-74`) gains two sentences: reviewed
      documents are served as resources and are data to check like recalled text; reviewed skills
      are listed as prompts and as resources. The text stays under the 4,096-character cut
      (`MAX_INSTRUCTIONS_CHARS`, `mcp.rs:77`; ADR-0038 d1), checked by its existing test.

- **d7 — `oml sync` gains a `document` adapter; `headings` and `lines` stay, and one path is
  loaded one way.** `--adapter document` writes one `document` claim per file, keyed `doc/` and
  the path without its extension under ADR-0057 d7's key mapping, anchored `{repo, rev, path}`
  with no fragment and `hash` the `oml-md/1` digest of the whole file; staleness and `moved` work
  per file (ADR-0057 d11, d18). The server cuts its passages by d3, whose fragments are the
  `headings` adapter's slugs, so a passage hit and a `headings` claim of the same section name the
  same anchor fragment. A run refuses a path whose live claims were written by another adapter
  (`doc/<path>` against `doc/<path>/…`), naming both and the claims it would leave, so one workspace
  never holds a document twice. `headings` stays the choice for knowledge recalled as separate
  facts, reviewed one section at a time; `document` is the choice for text read whole, such as a
  runbook or a guide, reviewed one version at a time. The cost of `document`: a one-word change
  supersedes the whole file and asks for a review of all of it, which the builder's diff (ADR-0082
  d6) and the reviewer's view must make short.

- **d8 — A document is metered as every claim is.** Its versions count in `memories`,
  `text_bytes` and `writes` (ADR-0071 d2); a recall that serves a passage counts in `recalls` as
  any recall does, and `resources/list` and `resources/read` count in none, each an `mcp_call`
  line and metric (ADR-0042 d10) under the key's request rate (ADR-0079). The cost, plainly: every
  edit of a 1 MiB document adds 1 MiB to `text_bytes`, since ADR-0001 R4 keeps every version.

- **d9 — Files, PDFs and binary resources wait for attachments.** A document is text in the
  claim's body. A PDF, an image or any other file is an ADR-0004 attachment and an ADR-0057 d21
  `page=<n>` fragment, both refused until built; `resources/read` serves no `blob` until then.

## The build's readings of 2026-11-18

The v0.8.0 build (#405, #406) took the readings below where this record was silent. The
coordinator confirmed each at its recommendation on 2026-11-18; they are proposed for the
operator's batch, which confirms or changes them, and each line names the decision it reads.

- **R1 (d3).** A heading with no content line before the next heading of level 1 to 4 is not a
  passage of its own: its text joins the `heading_path` of the passages below it. Text before the
  first heading is a passage only when it holds a content line. A passage of one title line would
  be the shortest unit in the index and, under BM25's length normalisation, outrank the section
  that answers the query; the `headings` adapter already skips such a heading (ADR-0057 d7).
  Rejected: every heading a passage, which keeps `?passage=<slug>` resolvable for a bare title
  and ranks bare titles first.
- **R2 (d3, d4, d6).** A passage is indexed by the document's title, then its `heading_path`
  joined by ` > `, then its lines, as the `headings` adapter writes a section's claim text, so a
  section under "Install" of the "Deploy guide" matches "deploy install". The served `text` is the
  passage's source lines, `start_line` to `end_line`, byte for byte, so a passage read equals the
  same span of a full read. Rejected: indexing the lines alone, which misses a query naming the
  document's subject.
- **R3 (d4).** `more_passages` counts the document's other passages in the pipeline's fused
  candidate list before the `k` cut, from any retriever: a lower bound, bounded by each list's
  depth, as ADR-0092 d4's `matched_by_kind` is. Rejected: an exact count of every passage that
  holds a query word, one more statement per document hit.
- **R4 (d2, d7).** `--adapter document` reads `.md` as `text/markdown` and `.txt` as
  `text/plain`, the `lines` adapter's set, keyed by the path without that extension under
  ADR-0057 d7's mapping. A path whose key breaks d2's grammar (a segment over 64 characters, a
  key over 128) is reported unreadable naming the key rule, and nothing is written for it.
  Rejected: cutting each long segment by d2's title cut, a second key rule.
- **R5 (d7).** A run refuses a path when the workspace holds live claims written for it the other
  way: a `document` claim at `doc/<p>` beside claims of another kind under `doc/<p>/` whose anchor
  `path` is the same file, in either direction. The refusal names both adapters and the keys it
  would leave. The anchor path, not the key shape alone, decides, so `docs/guide.md` loaded as a
  document (`doc/docs/guide`) and `docs/guide/intro.md` loaded by `headings`
  (`doc/docs/guide/intro/<slug>`) never collide. Rejected: the key prefix alone, which refuses
  that pair.
- **R7 (d3, d7).** The `headings` adapter ends a section at a heading of any level, 1 to 6,
  and a passage runs on past a heading of level 5 or 6. The build keeps the adapter as it is, so
  no synced claim's span or hash changes. A passage and a `headings` claim of one section
  name one fragment and start on one line, and end on one line wherever the section holds no
  heading of level 5 or 6 (`the_server_and_sync_cut_the_same_passages_for_the_same_bytes`).
  Rejected: cutting `headings` at level 4, which would supersede every synced section that holds
  a deeper heading. Confirmed by the coordinator on 2026-11-18, for the operator's batch.
- **R6 (d4, implementation).** Passages are search units in the existing postings: `search_postings`
  gains a passage `ordinal`, a document version holds one unit per passage with its own search
  text, vector and length on `document_passages`, the existing fold triggers keep them with the
  version's state, and every retriever reads a document through its passages; the per-kind counts
  of ADR-0052 d5 count a document's passages under its own kind code. A version with no
  documents, which is every row written before this build, is ranked exactly as before.

- **R8 (d4, ADR-0049 d6).** A promoted document's words join the vocabulary `did_you_mean`
  draws on, as every other hit kind's do, so a misspelt word only a document holds is still
  offered a spelling. Rejected: leaving documents out of the vocabulary, which offers no
  spelling for a word a document-only workspace holds. Taken by the build; not yet confirmed by
  the coordinator, for the operator's batch.
- **R9 (d4, d6).** `passage`, `more_passages` and `uri` are members of the recall hit, beside
  `memory`, `score` and `matched`, and the hit's `memory.text` and `memory.tokens` are the
  passage's, since the claim item carries no byte count. `resources/list` pages the documents
  first, then the skills, under one MCP cursor whose prefix, `doc:` or `skill:`, names the list
  it continues. Rejected: the members inside `memory`, which every read of a claim item shares;
  and two cursors, which MCP's `resources/list` does not have. Taken by the build; not yet
  confirmed by the coordinator, for the operator's batch.

## The review's readings of 2026-11-18 (PR #475)

Two reviews of the build returned REVISE on 2026-11-18. The coordinator decided the items below
that same day, before the code that takes them; each is marked **for the operator's batch**, which
confirms or changes it, and names the reading or decision it amends.

- **R3 amended (d4), 2026-11-18, for the operator's batch.** `k` counts versions, never passages.
  Every retriever's statement keeps one row per version before its own limit: the version's best
  unit by score, then ordinal (`row_number() OVER (PARTITION BY id ORDER BY score DESC,
  ordinal)`), carrying `count(*) OVER (PARTITION BY id) - 1` as its `more_passages`; the fused
  list then keeps one hit per version across the lists, its `more_passages` the larger of the
  lists' counts and the other passages fused beside it. A recall of `k` = 10 over a document whose
  30 passages hold the word and 20 memories that hold it answers 10 hits. Rejected: collapsing
  after the cut, which let one document's passages fill `k`.
- **R6 amended (d4), 2026-11-18, for the operator's batch.** A document adds at most 1 to a term's
  document frequency: df counts distinct versions, never units, and the row count N counts
  versions; the mean length stays per unit, the mean of the units' lengths. A 4,096-passage
  document therefore cannot flatten a word's IDF for the memories beside it. R6's sentence
  "ranked exactly as before" holds for a scope with no document; a scope holding documents ranks
  its memories with N and df each moved by at most one per document version. Rejected: df and N
  per unit, under which one long document repeating a word makes it near-worthless for every
  memory in the workspace.
- **R10 (d6), 2026-11-18, for the operator's batch.** `resources/list` lists one entry per
  document: its promoted version when it has one, with a held successor the reader may see
  described beside it as an unreviewed edit; a document with only a held version is listed marked
  held. `resources/read` of a document URI serves the promoted version. A recall hit on a held
  version carries a `uri` with `?version=<n>`, which `resources/read` honours only for a reader
  allowed to see that version, and reads as absent for any other. Rejected: one URI naming
  whichever version is newest, which served a held edit and its promoted tip under one name.
- **R11 (d4, d6), 2026-11-18, for the operator's batch.** A passage's URI uses its heading slug
  only when the slug is non-empty and holds nothing outside `[a-z0-9_-]`; otherwise it uses its
  lines, `L<start>-L<end>`. `?passage=` matches either form, so every hit's `uri` is a valid
  RFC 3986 URI and every passage is readable through it, a passage under a Cyrillic, emoji or
  `!!!` heading included. Rejected: percent-encoding the slug, a second spelling of one fragment.
- **R12 (d1), 2026-11-18, for the operator's batch.** A sent title holding a hidden character
  (C0 or C1 controls, bidi controls, zero-width characters: the set skills refuse) is refused;
  a derived title has them stripped. A derived title is the first level-1 heading as
  `oml_passage::headings` reads it, through `plain()`, so a heading inside a fence, front matter
  or an HTML comment never names the document.
- **R13 (d7), 2026-11-18, for the operator's batch.** For a document item, a sync rollback match
  also requires the text digest to match, so an edit that changes whitespace alone is written as
  a new version and not mistaken for the version it restores.
- **R5 amended (d7), 2026-11-18, for the operator's batch.** The mixed-adapter refusal is taken per
  loaded path, never per source: a path is refused by the document and non-document split of the
  live claims anchored at that path, so a source loaded in two runs under two adapters is refused
  path by path.
- **R14 (migration 0059), 2026-11-18, for the operator's batch.** Migration 0059 takes no lock
  that blocks a read of `memories` or `search_postings` for its run: it replaces the fold's
  function and leaves its trigger, widens the kind check `NOT VALID`, and refills only tenants
  holding a `document` row. Its served index is rebuilt by a second, `-- no-transaction`
  migration under a new name, partition by partition, each build committed before the next and
  attached to the parent, then the old index dropped. The coordinator asked for `CREATE INDEX
  CONCURRENTLY`; PostgreSQL refuses it inside a `DO` block and inside a multi-statement string,
  and the partitions are known only at run time, so each partition's build is a plain `CREATE
  INDEX` under that partition's `SHARE` lock, as 0041 builds its index: reads never wait, and a
  tenant's writes wait for its own partition's build alone.
- **R15 (d3), 2026-11-18, for the operator's batch.** The cut reads markdown in linear time and
  bounds what it keeps: a heading's text is read to 200 characters, as a title is (d1), so a
  heading path entry and a passage's search text never carry more of one line; and a document
  with more than 16,384 headings, four for each passage it may hold, is refused as
  `too_many_passages` like one past 4,096 passages. Rejected: no heading bound, under which half a
  million empty headings are read and kept to cut no passage.

## Consequences

- `MemoryKind` gains `Document`; submit and supersede gain `document`; recall's hit gains
  `passage` and `more_passages` and its `kinds` admits `document`; the listing's `kind` admits it.
  One migration adds the passage table with its postings and the kind check, reserved at build.
  `openapi.yaml`, `http-api.md`, `mcp-plugin.md` and `errors.md` follow in the serving change.
- `mcp.rs` declares `resources` and answers three methods; `mcp-tools.json` is unchanged, since a
  resource is not a tool (ADR-0039 d2).
- A new leaf crate holds the passage rule (d3), linked by `oml-app` and `oml-cli` and outside
  `oml-web`'s forbidden list (ADR-0070 d1), which reaches it only through the API.
- The scan's write-time budget (ADR-0074 d16) has no measurement at 1 MiB, and capture now also
  cuts passages: unmeasured, to be filed as a `measure` issue ("scan and passage cut time for a
  1 MiB document at capture") before the build lands.
- `oml doctor` names: a server without `resources` ("this server predates documents"), a sync
  path refused for a mixed adapter, and a version refused for too many passages.
- The recall eval gains documents cut from OML's own guides, with questions whose answer is one
  passage, before d4 is accepted as built.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A document as `kind: memory` with `format: text/markdown`** | Recall cannot tell a long document to cut from a fact, `resources/list` would enumerate every memory, and the builder would list notes as documents. |
| **Serve-time passage choice: rank the whole document, then pick its best passage** | No index change, but BM25 length normalisation ranks a long document low for a query one section answers, and the reranker reads only the first 4 KiB (ADR-0056 d4). The operator chose indexed passages on 2026-11-14, gated on a recall-eval run. |
| **Store each passage as a claim of its own** | A document reviewed one passage at a time is what the `headings` adapter already does; the operator asked for a document reviewed and served as one item. |
| **Provenance as a header line inside the resource text** | The text would no longer be the signed source, and a passage read would differ from the same span of a full read. |
| **Subscriptions or `listChanged`** | `/mcp` has no server stream (ADR-0014 d2); proposed ADR-0036 d5 rejected both for the same reason. |
| **Percent-encoded or reserved-expansion URIs carrying any claim key** | Two spellings could name one segment; proposed ADR-0036 d5 rejected both. Constraining the key grammar keeps one spelling. |
| **Retire sync's `headings` adapter in favour of documents** | It serves a different review shape, one section a reviewer can judge alone, and loaded OML's own ADRs (ADR-0057 d14). |
| **Files and PDFs now** | Attachments are not built (ADR-0004; ADR-0071 d2: "every write refuses attachments"); the operator chose text first. |
