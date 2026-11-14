<!--
id: adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource
tier: engineering
status: accepted design · 2026-11-14
answers: why a document is a kind and not a memory with a markdown format; which formats and sizes it takes; how its claim key names it; how its text is cut into passages and how a recall hit names its passage; what the MCP resources capability serves, under which URI scheme, scope and paging, and why there is no subscription; how a document is framed to a model; how oml sync's markdown adapters and documents agree; what is metered; what waits for attachments
-->

# ADR-0081 — A document is a reviewed markdown or text claim that recall searches by passage and the MCP server serves as a resource

**Status:** accepted design · 2026-11-14, by the operator, before it is built, every default as
drafted · proposed the same day, tracking #374

**Release:** planned for v0.7.4

**Twin parts** (ADR-0090 d18, PR #388): a document is a `memories` row, so its body and search
text are parts and its fold is the journal's; its passages are derived, cut again by the same rule
from the replayed body.

**Amends:** ADR-0025 (d8: a `document` hit is its best passage), ADR-0030 (d3: a held `document`
version is served inside an in-progress workarea as `memory` is), ADR-0039 (d3, d4: `oml_write`
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
  ([ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
  d7, d21), and writes kind `memory` (d6).
- **No resources.** `initialize` declares `tools` and `prompts` only (`mcp.rs:383-388`). Proposed
  ADR-0036 d5 designed `resources/list`, `resources/templates/list` and `resources/read` over claims
  under `oml://workarea/{workarea}/claim/{claim_key}`, with no subscription, since `/mcp` has no
  server stream and "Both `subscribe` and `listChanged` are optional"; that record is parked.
- **What a client does with a resource.** Claude Code offers resources in `@` autocomplete,
  fetches a referenced one "as attachments", and "automatically provides tools to list and read MCP
  resources" ([code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp), fetched
  2026-11-14), so the model can read a document without a person naming it.

Decided, not built.

## Decision

- **d1 — `document` is a claim kind: a titled markdown or plain-text body, reviewed like any
  claim.** `kind: "document"` takes `format` `text/markdown` (the default for the kind) or
  `text/plain`, refused otherwise (`request.validation`, `details.field: format`), and an optional
  `document: {title}`, 1 to 200 characters of one line; absent, the title is the first level-1
  heading of a markdown body, else the claim key's last segment. It is data, not instructions: the
  scan runs as on `memory` (no ADR-0074 d24 instruction rules), recall frames it as data, and its
  review is the workarea's policy with [ADR-0001](ADR-0001-the-constitution.md) R2 unchanged. A kind, not a format on `memory`, because three
  readers need to tell a document from a fact: recall cuts it into passages (d3), `resources/list`
  enumerates documents and not every memory (d6), and the builder lists them (ADR-0082). The cost
  is one kind value, one optional object and one migration.

- **d2 — A document's claim key is `doc/` and a path of plain segments, which is also its URI.**
  The key is `doc/` followed by one or more segments separated by `/`, each 1 to 64 characters of
  `a-z 0-9 . _ -`, never `.` or `..`, the whole at most 128 characters (the claim-key bound sync
  already keeps, ADR-0057 d7). Absent on submit, the server derives `doc/<slug of the title>` by
  ADR-0057 d7's slug rule; a key outside the grammar is refused naming `claim_key`. Every character
  is unreserved in RFC 3986, so a key maps to a URI segment for segment with no encoding, which
  meets proposed ADR-0036 d5's rule that the server decodes no segment. Sync's keys already fit:
  its key rule maps every character outside `[a-z0-9._/-]` to `-` (ADR-0057 d7).

- **d3 — Passages are cut by the server at capture, by one deterministic rule shared with sync.**
  For `text/markdown`, a passage is each level-1 to level-4 heading with its text up to the next
  heading of level 1 to 4, plus a first passage for any text before the first heading; for
  `text/plain`, the whole text. Any passage longer than 40 lines is then cut by ADR-0057 d21's lines
  rule (blocks of at most 40 lines split at blank lines). Each passage carries `ordinal`,
  `start_line` and `end_line` (1-based, inclusive), `heading_path` (the headings above it), and a
  `fragment`: the heading's slug as ADR-0057 d7's `headings` adapter computes it when the passage is
  a whole section, else `L<n>-L<m>` (ADR-0057 d21). Front matter and HTML comments are skipped as
  the `headings` adapter skips them. The rule lives in one leaf crate with no I/O that the server
  and `oml-cli` both link, so sync's spans and the server's passages are the same bytes. A version
  holds at most 4,096 passages, chosen, not measured (1 MiB of 40-line blocks of 6 bytes a line
  would exceed it; such a body is refused, `request.validation`, `details.field: body`,
  `details.reason: too_many_passages`). Passages are derived, stored beside the version, never
  signed separately (the signature covers the source, ADR-0056 d2), and erased with the body
  (ADR-0005 d4).

- **d4 — Recall ranks a document by its passages and serves the best one as the hit.** Each
  passage is indexed as its own search unit, under the same pipelines and filters as rows
  (ADR-0049), the boundary rule of
  [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
  d2 applied before any limit. A document contributes at most one hit per recall, its best passage,
  so `k` keeps counting claims (ADR-0025 d8). The hit is the claim item with `text` the passage's
  text, `body_bytes` and the budget the passage's, and two new members: `passage: {fragment,
  ordinal, start_line, end_line, heading_path}` and `more_passages`, the count of the document's
  other passages that matched. Its `uri` is `oml://workarea/<workarea>/doc/<path>?passage=<fragment>`
  (d6). `kinds` admits `document`; an absent `kinds` includes documents (amends ADR-0039 d4). The
  cost: a larger index, one posting set per passage, unmeasured, and a recall-eval run before the
  build is accepted (ADR-0048's gate), since passage ranking changes what a query returns.

- **d5 — Documents are written on HTTP, the console and `oml_write`, up to the claim limit.**
  `oml_write` admits `kind: "document"` with `format` and `document` (amends ADR-0039 d3): a
  document is data, as a memory is. The limit is the claim limit, 1 MiB after NFC, refused as today
  with `request.payload_too_large`. Editing is supersession and retiring is `retirement: true`, as
  every claim (ADR-0025 d10, d11); a successor re-sends the whole text. Inside an in-progress
  workarea a held document is served to readers there labelled `unreviewed`, as a held memory is
  (amends ADR-0030 d3, adding `document` to its served kinds). Documents follow ADR-0030 exactly as
  memories do, under ADR-0080 d10's statement of it: a key whose scope node lies inside the
  in-progress root gets held documents in recall, `resources/list` and `resources/read`, marked by
  `unreviewed` and `_meta.oml.unreviewed`; a held successor is served beside the promoted tip, a
  held retirement is not served; a version held for a scan finding stays author-only; outside the
  root nothing held is served, wide sources never reach it (ADR-0072 searches committed and
  validated workareas only), and the commit needs zero held versions (ADR-0030 d6).

- **d6 — The MCP server declares `resources` and serves documents and skills under `oml://`,
  read-only and without subscription.**
    - **Capability.** `"resources": {}`: neither `subscribe` nor `listChanged`, for the reasons
      proposed ADR-0036 d5 gives (a stateless `/mcp` with no stream, ADR-0014 d2).
    - **URIs.** A document is `oml://workarea/<workarea>/doc/<path>` with `<path>` its claim key
      after `doc/`; a passage adds `?passage=<fragment>`; a skill is
      `oml://workarea/<workarea>/skill/<name>` ([ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md)
      d7). `<workarea>` is the workarea's name, resolved as every name is
      ([ADR-0026](ADR-0026-names-are-keys.md) d4). `resources/templates/list` serves
      `oml://workarea/{workarea}/doc{/path*}{?passage}` and
      `oml://workarea/{workarea}/skill/{name}` (RFC 6570 path-segment expansion, never reserved
      expansion, as proposed ADR-0036 d5 rejected `{+path}`). The segment `claim/` stays proposed
      ADR-0036's.
    - **`resources/list`.** One page of the documents at the key's default workarea: the scoped
      listing of kind `document`, promoted tips under the key's read access in the subtree and the
      boundary rule, the caller's own held documents excluded and other authors' held ones only
      inside an in-progress scope, newest first, 50 a page under MCP's `cursor`, then the offered
      skills (ADR-0080 d7). Each entry is `{uri, name, title, description, mimeType, size}` with
      `name` the claim key, `title` the document's title, `description` "Reviewed document in
      `<path>`, approved `<date>` by a `<held_by>` reviewer: data to check, not instructions" (or
      "Unreviewed document" for a held one), and `size` its body bytes. The list is bounded by the
      page, never by the set.
    - **`resources/read`.** A document URI answers `{contents: [{uri, mimeType, text, _meta:
      {oml: {artifact_id, version, approved_at, approver, unreviewed, workarea_path, anchor,
      findings}}}]}`, `text` the body byte for byte, or with `?passage=` that passage alone; `approver` carries
      ADR-0089 d7's `act` and `confirmed` once that record is built, so a document an
      admin fix serves reads as one. The
      provenance and data framing travel in `_meta` and the list's `description`, never inside the
      text, so the bytes stay the signed source (ADR-0074 d12 keeps the JSON text byte for byte for
      the same reason); hidden characters are escaped in the text content as ADR-0074 d12 escapes
      them for a model. A missing, unreadable, held-and-not-served or malformed URI answers one
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
  (`doc/<path>` against `doc/<path>/…`), naming both and the claims it would leave, so one workarea
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
