<!--
id: adr/ADR-0056-a-claim-version-names-its-format-the-signature-covers-its-source-and-adapters-compiled-into-the-server-derive-what-search-reads
tier: engineering
status: accepted
answers: whether a claim may be a Mermaid diagram, a JSON or YAML fragment or a code snippet, and how it says so; what the signature covers when it is; where the format code lives and why no plugin loads at run time; what each adapter validates, which text it gives the lexical, postings, sparse and rerank stages, what a page may render and how two versions are compared; how Mermaid is rendered on the pages without weakening their policy, and what that costs; how the deterministic scan reads a JSON or YAML claim without ever reading less than today; why PlantUML is not in the first set; how many facts one diagram claim may carry; what the reviewer sees; what recall serves and how the gain is measured; how formats feed `oml sync`; the field on every surface and the migration; the rejected roads, the cost, the doctor lines, the tests and the open questions
-->

# ADR-0056 — A claim version names its format, the signature covers its source as stored, and adapters compiled into the server derive what search reads and what a page may render

**Status:** accepted · 2026-11-06 (built in v0.6.x; the unbuilt parts are listed under [Build record](#build-record))

**Release:** introduced in v0.6.x ([releases](../product/releases.md#decisions)); migration 0033

**Amends:** ADR-0039 (d3: `oml_write` gains `format`; d4: every claim item carries `format`).
ADR-0045 (d8: the pages' policy gains one `frame-src` source, and one new route answers under its own
policy). ADR-0019 (d4: a fourth plugin point, the format port, under the same law). ADR-0049 (d12 item
1: the stored lexemes are computed from a row's search text, which is its claim unless d6 says
otherwise). ADR-0052 (d8: the trigger and the encoder read the same search text). ADR-0053 (d2: the
passage a reranker scores is the search text). ADR-0005 (d4: the erasure inventory gains
`memories.search_text`).

## Context

A claim today is text, and every surface treats it as text: the scan reads it, the trigger turns it into
lexemes ([ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md)
d12), the postings and the encoder read those ([ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)
d8), the reranker reads its first 4 KiB ([ADR-0053](ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md)
d2), and the pages print it escaped, with no script
([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d8).
Agents already write diagrams, configuration and code into that text: a Mermaid block describing a
flow, a YAML fragment of a retry policy, a function's signature. Three things go wrong. A reviewer
reads 40 lines of `A-->|authorize|B` and signs a flow nobody drew for them. Search indexes `-->|` and
`max_attempts:` as they fall, so the query "which service calls payments" depends on how the tokenizer
splits arrow syntax. And `oml sync`, which the v0.5.8 source-anchors design adds (anchors on every
memory and source adapters behind a port, on `design/source-anchors-r1`; until it lands, the anchor and
staleness rules of ADR-0047 d3 and d11 on `design/code-map-r2`), would propose a new version for every
re-indented YAML file, each costing one review.

The operator decided on 2026-11-05 to design claim formats now and build them after v0.5.8, with
Mermaid, JSON/YAML and code snippets as the first adapters and PlantUML outside the first set.

What must hold: R1–R4 ([ADR-0001](ADR-0001-the-constitution.md)), above all that the reviewer signs
what is stored and that review changes visibility and never rank (R3); the content-addressed envelope
and the serve binding ([ADR-0004](ADR-0004-bodies-and-attachments.md) d2, d5); compiled-in plugins only
([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d4); pure Rust on the server
([ADR-0006](ADR-0006-pure-rust.md)); a new agent capability is a parameter before it is a tool
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8). Decided, not built.

## Decision

- **d1 — Every claim version carries a `format`, a media type from a closed list, defaulting to
  `text/plain`.** The list, each entry's adapter decided below:

    | `format` | What it is | Adapter |
    |---|---|---|
    | `text/plain` | today's claims; the default | d4, identity |
    | `text/markdown` | prose a client may render as Markdown ([RFC 7763](https://www.rfc-editor.org/rfc/rfc7763)) | d4, identity |
    | `text/vnd.oml.mermaid` | a Mermaid diagram in the subset of d5 | d5 |
    | `application/json` | one JSON value ([RFC 8259](https://www.rfc-editor.org/rfc/rfc8259)) | d6 |
    | `application/yaml` | one YAML document ([RFC 9512](https://www.rfc-editor.org/rfc/rfc9512)) | d6 |
    | `text/vnd.oml.code; lang=<tag>` | a code snippet in a named language | d7 |

    The two `vnd.oml` names are OML's spellings, not IANA registrations (question 1); they are one
    spelling per concept ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d7), with no short
    aliases such as `mermaid`, and a refusal lists the accepted values so an agent corrects itself
    in one retry. The default is `text/plain`, not `text/markdown`, because every stored claim is
    shown and indexed as plain text today, so the default changes nothing; and because a rendered
    Markdown link can show one target and point at another, a page never renders Markdown (d9): the
    two differ only in what a client may do with them. Kinds other than `memory`, `derived_fact` and
    ADR-0047's proposed `code` take only `text/plain` or `text/markdown`: steering, guardrail and
    vocabulary rows are read by rules that expect prose, and a refusal names `format`.

- **d2 — The format is part of the signed submission and fixed for the version; the signature covers
  the source text exactly as stored.** The source is the text the writer sent, normalized to NFC as
  every claim is at capture ([admission](../architecture/admission.md)), and nothing else: no
  re-indentation, no key sorting, no line-ending change. The submission event's payload, beside
  `kind` and `plaintext_digest` (the SHA-256 of that stored source), gains `format`; ADR-0004 d2's
  additive rule holds, and an event without the field reads as `text/plain`, so no existing event
  changes digest. A format is never edited: a change of format, prose to diagram or JSON to YAML, is
  a new version by supersession, reviewed like any other. The serve binding of ADR-0004 d5 covers the
  served source digest and the format; a rendering is not part of what is signed, and d9 says why that
  is safe. The derived search text (d4) is not signed: it is a pure function of (format, source,
  adapter version), so it adds nothing a reviewer did not sign, and the reviewer is shown it (d9).

- **d3 — A `Format` port in the application layer, adapters compiled into the server and the CLI, and
  no plugin loaded at run time.** `oml_app::ports::Format` has five operations, each pure and
  deterministic, with no I/O, clock or randomness:

    | Operation | Returns | Used by |
    |---|---|---|
    | `validate(source)` | `Ok(Parsed)` or a refusal with a named reason, line and column | capture, before the scan |
    | `search_text(parsed)` | the text the lexical, postings, sparse and rerank stages read | capture; the refill of d4 |
    | `scan_views(parsed)` | decoded strings the scan reads beside the source (d6) | capture |
    | `render_hint()` | what a page and a client may do with the source (d9) | the pages, the SDKs |
    | `diff(old, new)` | `{equivalent, changes: [{op, path, before, after}]}` | review (d9), `oml sync` (d11) |

    The adapters live in one crate, `crates/adapters/oml-format`, with no I/O dependency, composed by
    `oml-server-lead` and `oml-cli`, so the server and the CLI compute the same search text and the
    same diff from the same code. Each adapter has an id and a version, `mermaid/1`, recorded with the
    scan verdict as `scanner_version` already is. `formats.enabled` in the server's settings may
    narrow the list, never widen it, and the change is journalled as `system.settings_applied`
    (ADR-0019 d4's law); a disabled format is refused on new writes and its stored rows are still
    served, as source, with doctor naming them. No dlopen, no WASM and no plugin store: a format
    adapter decides what text the scan and the index read beside a signed source, which puts it next
    to the signing path, and a downloaded adapter would make what a deployment indexes depend on a
    supply chain the release did not ship. ADR-0019 d4's revisit trigger applies unchanged.

- **d4 — Search reads one deterministic search text per version, stored beside the source, and
  prose stays exactly as it is indexed today.** `memories` gains `search_text` (text, `NULL` when the
  search text equals the source) and `search_text_by` (the adapter id and version). Capture computes
  both before the transaction opens, beside the scan. The stored-lexeme trigger reads
  `coalesce(search_text, claim)`, the encoder of ADR-0052 d8 reads the same, and the reranker scores
  its first 4 KiB as ADR-0053 d2 cuts a passage now. For `text/plain` and `text/markdown` the search
  text is the source, `search_text` stays `NULL`, and a prose row's lexemes, postings and rank are
  byte-for-byte today's (`omitting_format_stores_text_plain_and_indexes_exactly_as_today`). Markdown
  syntax is not stripped: stripping it would change the rank of every row an agent wrote with a
  bullet. An adapter's new version recomputes `search_text` for its rows after start, oldest first,
  as the encoder fills its backlog, and never touches the source; if the new version cannot derive a
  stored row's search text, the row is indexed by its source and doctor names it. ADR-0050 d5's
  `indexed` answer lists the terms of the search text, so the writer sees what a diagram will be
  found by. The column is plaintext derived from the body and is erased with it (ADR-0005 d4).

- **d5 — Mermaid: five diagram types in a strict subset, search text as sentences, rendered only in a
  sandboxed frame in the browser, never on the server.**
    - **Types.** `flowchart` (and `graph`), `sequenceDiagram`, `classDiagram`, `stateDiagram-v2` and
      `erDiagram`: each is nodes and labelled relations, so each yields sentences. Gantt, pie,
      timeline, journey, mindmap, quadrant, sankey, xychart, gitGraph, C4, block and architecture
      diagrams are refused as `mermaid.unsupported_diagram`; a chart's facts are data and belong in
      JSON or YAML, and a reopened type is a new decision on this record.
    - **Refused syntax**, each by name: `%%{init}%%` and other directives and a front matter with any
      key but `title` (`mermaid.directive_refused`: configuration could change the security level the
      frame relies on), `click`, `href` and `callback` (`mermaid.interaction_refused`), `style`,
      `classDef`, `class`, `linkStyle` and `:::` (`mermaid.styling_refused`: a node coloured as the
      background is invisible in the rendering and still signed), HTML in a label
      (`mermaid.html_label_refused`), and the bidirectional controls U+202A–U+202E and U+2066–U+2069
      anywhere (`format.bidi_control`). The validator is OML's own parser of this subset, in Rust;
      it is stricter than Mermaid's, so a claim can pass it and still fail to render in the browser,
      and d9 makes that harmless. Caps: 64 KiB of source, 200 nodes, 400 relations, all chosen, not
      measured (FR-109).
    - **Search text.** The title, then one sentence per relation in source order, labels in place of
      ids and the id kept beside a label that differs from it: `A -->|calls| B` with labels
      `Checkout` and `Payments` gives "Checkout (A) calls Payments (B)."; an unlabelled flowchart
      edge gives "Checkout leads to Payments."; a message gives "Checkout sends Payments:
      authorize amount."; inheritance gives "Card is a PaymentMethod."; composition "Order has
      LineItem."; a state transition "From Pending to Paid on capture."; an ER relation "Customer
      places Order, one to many." Then each node with no relation, as its label. `%%` comments are
      left out: they never appear in the rendering, and text a reader of the picture cannot see does
      not rank. Identifier splitting then runs over the sentences as it does over prose.
    - **Rendering** is d9's.

- **d6 — JSON and YAML: a strict parse, search text from key paths and values, and a scan that reads
  the decoded values as well as the source, never instead of it.**
    - **Validate.** JSON by `serde_json`, refusing a duplicate key (`json.duplicate_key`: two parsers
      would read two different documents from one signed text). YAML as one document of the YAML 1.2
      core schema by a maintained pure-Rust parser chosen at build under `cargo deny` (question 2),
      refusing anchors and aliases (`yaml.alias_refused`, which closes expansion bombs), tags
      (`yaml.tag_refused`), merge keys, a second document and a duplicate key. Both: nesting at most
      32 deep, at most 2,000 scalars, 64 KiB of source (FR-109).
    - **Search text.** One line per scalar, in document order: the key path with its segments
      separated by spaces, then "is", then the value, as `payments retry max_attempts is 5`; array
      items carry their index. Identifier splitting then finds `max attempts` as it does in prose.
      A value under a key the scan treats as secret-shaped never reaches the search text (below).
    - **The scan.** The deterministic scan runs over the source exactly as today, with the same rule
      set and the same bytes, so nothing it vetoes today passes. It then runs the same rules over
      each `scan_views` string, the scalars after JSON escapes and YAML quoting, folding and block
      scalars are decoded, because `"-----BEGIN PRIVATE KEY"` and a folded YAML scalar hide from a
      rule that reads only bytes. A finding on a view is reported with the key path, never the value.
      One rule is added for these two formats: a value under a key whose last segment matches the
      secret-key list (`password`, `passwd`, `secret`, `token`, `api_key`, `apikey`,
      `private_key`, `client_secret`, `authorization`, `credentials`, and `_secret`, `_token`, `_key`
      suffixes) is vetoed as `secret.keyed_value` unless it is empty or a placeholder (`<…>`,
      `${…}`, `{{…}}`, `***`, `changeme`, `example`, `redacted`). The law is
      `the_scans_findings_on_the_source_alone_are_a_subset_of_its_findings_on_the_source_and_its_views`:
      a format can add a finding and never remove one, as ADR-0019 d4 lets a plugin narrow and never
      widen. ADR-0050 d6's advisory findings count per format: `quality.many_claims` fires on more than
      12 relations in a diagram or 40 scalars in a fragment instead of on sentences, chosen, not
      measured (FR-113).

- **d7 — Code snippets: a language tag, today's identifier splitting, and no symbol extraction.**
  `lang` is one of a closed list kept in `oml-format` (the languages the scanner's and ctags' tables
  already name, starting with `rust`, `python`, `typescript`, `javascript`, `go`, `java`, `kotlin`,
  `scala`, `sql`, `shell`, `c`, `cpp`, `csharp`, `ruby`, `php`, `swift`, `hcl`, `toml`, `dockerfile`,
  `plantuml`), refused otherwise as `request.validation` naming `format`. Validation is UTF-8, 64 KiB
  and no bidirectional control anywhere (`format.bidi_control`, the Trojan Source class); there is no
  grammar, as ADR-0047 d4 keeps none. The search text is the source, so a snippet is indexed as
  prose is today, identifiers split by ADR-0049's normaliser; the language is not added as a word.
  No symbols are extracted on the server: that needs a grammar per language, and the symbol a fact is
  about is the anchor's to name, computed on the client (ADR-0047 d3, d4). The diff compares lines
  after ADR-0047 d4's span normalisation (`oml-span/1`), so a reformat is equivalent.

- **d8 — PlantUML is not a format.** Its renderer is Java, which neither the server nor the CLI
  carries ([ADR-0006](ADR-0006-pure-rust.md)); `!include` and `!includeurl` make the rendered diagram
  depend on files and URLs the signature does not cover, and rendering them is a network fetch from
  the server; its preprocessor (`!define`, `!procedure`, loops) is a language, so even search text
  would mean evaluating it; and the public renderer works by sending the diagram to a third party.
  Validate-and-index only was weighed and declined: it buys sentences for search at the cost of a
  preprocessor parser, for diagrams no page could ever show. A PlantUML source may be stored as
  `text/vnd.oml.code; lang=plantuml`: signed, scanned and indexed as code, never rendered. Revisit
  when a renderer exists that runs in the browser without a network or a JVM, or when the recall
  eval shows the code form loses facts that a diagram form would find.

- **d9 — The reviewer always sees the source; where the format allows, a rendering beside it in a
  sandboxed frame; a failed rendering never hides the source.**
    - **Render hints.** `text/plain`, `text/markdown`, JSON, YAML and code: the pages show the source
      escaped, as today, in a monospaced block; nothing is rendered on the server, and no Markdown is
      rendered on a page (d1). Mermaid: the source, and beside it the diagram drawn in the browser.
      Clients choose for themselves; the SDKs' `fence_for(format)` gives the Markdown fence info
      string (`mermaid`, `json`, `yaml`, the `lang`), so a client that renders fenced blocks can.
    - **The frame.** The parent pages keep ADR-0045 d8's policy and still carry no script; their
      `Content-Security-Policy` gains one source, `frame-src https://<issuer>/web/render/`. A page
      that shows a Mermaid claim embeds `<iframe sandbox="allow-scripts"
      src="/web/render/<token>">`, where the token is sealed by the connect page's seal
      (`oauth/seal.rs`), bound to the session, the artifact version and the read it belongs to (the
      review serve's `serve_id`, the user page's listing, or the admin read already journalled by
      ADR-0045 d10, so a rendering journals nothing twice), and expires in 5 minutes. The route
      answers `no-store` under its own policy: `sandbox allow-scripts; default-src 'none';
      script-src https://<issuer>/web/assets/mermaid-<sha256>.js 'sha256-<init>'; style-src
      'unsafe-inline'; img-src 'none'; font-src 'none'; connect-src 'none'; form-action 'none';
      base-uri 'none'; frame-ancestors https://<issuer>`. The frame document holds the source
      escaped in a `<pre class="mermaid">`, the pinned bundle and a hashed initialiser that runs
      Mermaid with `securityLevel: "strict"` and `htmlLabels: false` and, on error, prints Mermaid's
      message in the frame. The bundle is the Mermaid release (MIT) current at build, vendored into
      the repository, its SHA-256 in its file name and checked by a test, compiled into the binary
      with `include_bytes!` and served `immutable`; nothing is fetched from a CDN.
    - **The risk, stated.** The new code a browser runs is Mermaid, which has had cross-site
      scripting advisories. What it can reach is bounded by the frame: an opaque origin (the
      `sandbox` directive, without `allow-same-origin`), so no cookie, no storage and no script
      access to the parent; no network (`connect-src`, `img-src`, `font-src` and `form-action`
      `'none'`), so nothing it reads can leave; and its only input is the one claim's source, which
      the session could already read. `style-src 'unsafe-inline'` is needed because Mermaid writes
      its styles inline; with no network a stylesheet has nowhere to send anything. A compromised
      frame can draw a misleading picture of the source it was given, which is why the card says the
      source is what is signed and keeps it on the left. If the pinned bundle needs `unsafe-eval`
      or any network to draw the five types, Mermaid rendering does not ship and the pages show
      source only; a Chromium test decides (d14).
    - **The card.** A review card shows, top to bottom: the title, the `format`, the source (always,
      complete, scrollable), the rendering (where the hint allows), what it will be found by (the
      search text, ADR-0050 d5's `indexed`), the scan's findings including those on views, and for a
      supersession of the same format the structural diff against the version it replaces; across
      formats, "format changed" and both sources. A rendering that fails shows its error in the frame
      and nothing else changes. The verdict judges the served source digest, as now.

- **d10 — One claim, one fact: a diagram states one structure, and a relation that needs its own
  review is its own claim or edge.** A non-prose claim needs ADR-0050 d5's `title`, refused as
  `request.validation` naming `title` without it, and the title states the one fact the source
  shows: "The payment flow from checkout to settlement, as of 2026-11-05", "The payments client's
  retry policy". If this record is built before ADR-0050 W2, it builds `title` exactly as d5 there
  defines it. A diagram may show many relations as one structure, judged by one verdict; when a single
  relation must be found, cited or reviewed on its own ("Checkout calls Payments synchronously"), it
  is a prose claim of its own, linked by a claim edge ([ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md))
  or, for what a memory is about, an `oml:about` term edge (ADR-0051, PR #58). `quality.many_claims`
  (d6) warns the writer and the reviewer when a diagram grows past one structure. The skill says it
  in three sentences: put a structure in a diagram with a title that states it; put each fact someone
  must cite in a claim of its own; never put a secret in a fragment, since the write will be refused.

- **d11 — `oml sync` reads formats through the same adapters, and an equivalent change proposes
  nothing.** A source adapter of the v0.5.8 design maps what it reads to a format: `.mmd` and a
  fenced `mermaid` block to Mermaid, `.json` to JSON, `.yaml` and `.yml` to YAML, a file extension in
  d7's table to code, anything else to `text/plain`. When an anchored source changed, sync calls the
  CLI's `diff(stored, current)`: `equivalent` (reordered JSON keys, a re-indented YAML file, a
  reflowed diagram, a reformatted function) proposes no supersession and costs no review; otherwise
  sync proposes one, whose review card shows the same diff the CLI computed, since both run
  `oml-format`. Whether an equivalent change refreshes the anchor's hash without a review is the
  source-anchors design's to settle (question 5).

- **d12 — Recall serves the source and its format; whether diagram claims help is measured before the
  skill recommends them.** Every claim item, on every mode of `oml_read`, HTTP and the SDKs, gains
  `format`. Agents get the source: it is what they read best, and the search text is an index, never
  served as the claim. `max_tokens` counts the source. Two measurements gate the skill's advice, not
  the field: the recall eval (ADR-0048, ADR-0049 d9) gains format rows, the same facts stored once as
  prose and once as a diagram or fragment with paraphrased queries, and the gate is that d5's and
  d6's search texts find the format rows at success@5 within 0.05 of their prose twins, with every
  existing row's score unchanged; and ADR-0047 d17's task harness gains an arm where the captured
  knowledge includes diagram claims, against the same knowledge as prose. Until both pass, the skill
  does not suggest a format, and a writer who sends one gets it (FR-112).

- **d13 — The field on every surface, with no new tool; pre-release, nothing breaks.** `oml_write`
  gains an optional `format` (string; the enum of d1 with a pattern for `lang`), in submit and
  supersede modes; `POST /v1/memories` and `…/supersede` take it; every claim item returns it; the
  SDKs regenerate and gain `fence_for`; `oml write --format <media type>` reads the text from standard
  input when the text is `-`; the review and admin reads carry it. An unknown or disabled format is
  `request.validation` with `details.field: format` and `details.allowed`; content that fails its
  adapter is a new code, `submission.format_invalid` (422, nothing journalled), with
  `details.reason` (`mermaid.directive_refused`, `json.duplicate_key`, …), `details.line` and
  `details.column`; a scan veto stays `submission.scan_veto`, its `details.finding` naming the key
  path for a view finding. Backward compatibility: the field is optional with today's behaviour as
  its default, every stored row is `text/plain` by the event rule of d2 and by a migration that sets
  the column's default, and a client that ignores `format` in a claim item still reads correct text.
  The one visible change is that a client which wrote Mermaid as plain text keeps getting it back as
  plain text; to get a rendering, it writes a new version with the format. ADR-0039 d1 still applies:
  the changelog lists the new field, the new code and the policy change. `oml_remember`, removed in
  v0.5.1, gains nothing.

- **d14 — Cost, doctor lines and the build, each step closed by named tests.**
    - **Cost.** The largest is the Mermaid subset parser, which OML owns and must keep in step with
      what the pinned bundle draws (FR-109). The bundle adds to the binary and to the first rendered
      card; its size and render time for the pinned release are unmeasured (FR-110). Parsing and the
      search text add CPU to a format write and `search_text` adds bytes to a format row; prose rows
      pay nothing (FR-111). Each review card with a diagram is one more request.
    - **Doctor** (`oml doctor`, admin) names: the enabled formats with their adapter versions; rows
      whose `search_text_by` is behind its adapter (the refill backlog) and rows indexed by source
      because the adapter could not derive their search text; rows in a disabled format; the served
      bundle's digest against the pinned one; and the frame route's policy, fetched and compared
      with d9's. `/readyz` lists the enabled formats. `oml_format_refusals_total{format, reason}`
      counts refusals.
    - **Build.** F1: the field, the closed list, the envelope and the port with the identity
      adapters (`omitting_format_stores_text_plain_and_indexes_exactly_as_today`,
      `the_signature_covers_the_format_and_the_stored_source_bytes`,
      `a_format_changes_only_by_supersession`). F2: JSON and YAML with the scan views. F3: code.
      F4: Mermaid validation and search text. F5: the frame, after the Chromium tests pass. F6: the
      diff in the CLI for `oml sync`. F7: the skill text, after d12's gates.

## Consequences

- ADR-0039's contract, [the HTTP API](../api/http-api.md), [errors](../api/errors.md),
  [openapi.yaml](../api/openapi.yaml), [the MCP plugin page](../api/mcp-plugin.md) and the SDKs gain
  `format` and `submission.format_invalid` in the change that serves them, not before
  (ADR-0039 d9). The [admission](../architecture/admission.md) page's scan section gains the views and
  `secret.keyed_value`; the event catalog's `submission.*` payloads gain `format`.
- ADR-0045 d8's sentence "no script" stays true of every page; a script runs only in the frame route,
  which is a new route and a new policy to keep. The pages' tests assert both policies.
- The search stages read one more column, and the trigger's guarantee becomes "a write cannot store
  lexemes that are not its search text's", the search text being a pure function of signed content;
  doctor's refill check samples rows and recomputes it.
- One more vendored dependency outside Cargo: the Mermaid bundle, updated by a pull request that
  changes its file name and digest. `cargo deny` does not see it; the test that checks its digest
  does.
- Roadmap rows opened: FR-109 (Mermaid subset fidelity and the caps), FR-110 (bundle size and
  render time), FR-111 (write cost of formats), FR-112 (whether diagram claims help recall and
  tasks), FR-113 (the per-format quality thresholds).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Diagrams as image attachments only** | A PNG or SVG of a diagram is opaque to search (ADR-0004 d6 forbids decoding it as text), a reviewer signs pixels nobody can diff, an agent cannot read it, and sync cannot tell a moved box from a new edge. The source is smaller, diffable and what agents read best. An attachment stays possible beside a claim for a picture that has no source. |
| **Rendering on the server** (a headless browser or a Mermaid port producing SVG) | A headless browser in the lead is a large process with a network stack next to the signing key; a Rust port of Mermaid's layout does not exist and would be OML's to maintain; and a server-rendered SVG embedded in a page is markup from claim text inside the parent origin, the injection ADR-0045 d8 closes with "no script". The browser in a sandboxed frame is already isolated and costs the server nothing. |
| **Dynamic format plugins** (WASM or dlopen, a plugin store) | What a deployment indexes and scans next to a signed source would depend on code the release did not ship, and two deployments could derive different search texts from one signed claim. ADR-0019 d4 rejects dynamic loading for the scanner for the same reason; its revisit trigger stands. |
| **One "document" kind holding any rich text** | A kind decides who reads a row and how ([ADR-0021](ADR-0021-ai-human-ontologies.md), ADR-0047 d2); a format decides how its text is parsed. A document kind would mix the two, carry no grammar to validate, give search nothing better than today's text, and invite many facts per claim, against d10. |
| **Format inferred from the text** (a fenced block, a leading `{`) | Inference is a guess the signature would not cover: two adapter versions could infer differently from one stored text. The writer states the format and signs it. |
| **`text/markdown` as the default** | It would change nothing for search and invite pages to render Markdown, where a link's text and its target can differ under one signature. Plain text keeps the default exactly today's. |
| **Short format names** (`mermaid`, `json`) beside media types | Two spellings per concept, against ADR-0039 d7; the refusal lists the accepted values instead. |
| **Mermaid with styling, directives and click handlers** | Styling can hide a node in the rendering while the source holds it; directives can change the security level; handlers are script. The subset costs colour and nothing a reviewer needs. |

## Open questions

1. Register `text/vnd.oml.mermaid` and `text/vnd.oml.code` with IANA, or keep them as OML's own
   spellings? Nothing in the design depends on registration.
2. Which pure-Rust YAML 1.2 parser: one that is maintained, refuses aliases and tags by
   configuration, and passes `cargo deny`. `serde_yaml` is archived by its author; the candidates
   are to be checked at build. *Answered at build: `saphyr-parser` 0.3.3; see the build record.*
3. Is d6's secret-key list right, and should `secret.keyed_value` hold for review rather than veto
   when the value looks like a placeholder the list does not know? Veto is the fail-closed reading.
4. Should recall take a `formats` filter? No use case asks for one yet (ADR-0039's Occam rule), so
   none is designed.
5. Does an equivalent change found by `oml sync` refresh the anchor's hash without a review? It
   touches no signed text, so it may be the source-anchors design's to allow.
6. Should a page draw a JSON or YAML fragment as a collapsible tree? It would be server-side markup
   from claim text; the source is readable without it, so not in the first set.

## Build record

Written when the decisions above were built, 2026-11-06. Nothing above changes; this section records
what the build chose where the text left a choice, and what it did not build.

**The YAML crate (question 2).** `saphyr-parser` 0.3.3, released 2026-10-27, `MIT OR Apache-2.0`,
10.3 million downloads on crates.io, with `thiserror` and `arraydeque` as its only non-test
dependencies (crates.io API, fetched 2026-11-06). It is the event parser of the `saphyr` project
for YAML 1.2, in pure Rust, and it reports each anchor, alias, tag, document and scalar as an
event with its line and column, so the adapter refuses an alias, a tag, a merge key, a second
document and a duplicate key by name from the events and never builds a value an alias could
expand. Rejected: `serde_yaml`, whose last release is 0.9.34+deprecated of 2024-03-25 and which
its author archived; `serde_yaml_ng`, a fork over `unsafe-libyaml`, a machine translation of the
C library, whose serde layer resolves aliases and merges before the adapter could refuse them;
and `saphyr`, the loader above this parser, for the same reason. The pure-Rust rule of
[ADR-0006](ADR-0006-pure-rust.md) holds. `cargo deny` is run in CI; the licence is on its allow
list.

**Readings the build made.**

- `title` is built as ADR-0050 d5 defines it: one line, at most 100 characters, NFC, signed with
  the version, served in the claim item, required of a claim whose format is not prose. It is
  indexed by being the first line of the search text, not at a higher weight: a prose claim that
  carries a title stores `search_text` as the title, a line break and its text, and a prose claim
  without one stores none, so rows that predate the field rank as they did. The weight waits for
  ADR-0050's own build.
- The word `class` is a styling statement in a flowchart or a state diagram and is refused as
  `mermaid.styling_refused`; in a `classDiagram` it declares a class and is accepted, while
  `cssClass` and `:::` are refused everywhere.
- A source line the subset does not read is `mermaid.syntax_error` with its line, not text the
  parser drops, since the signature covers it. The reasons the build added to d5 and d6 are
  `mermaid.syntax_error`, `mermaid.too_many_nodes`, `mermaid.too_many_relations`, `json.invalid`,
  `yaml.invalid`, `yaml.empty`, `yaml.complex_key`, `yaml.merge_refused`, `yaml.second_document`,
  `format.too_large`, `format.too_deep` and `format.too_many_scalars`.
- The scan reads each key of a JSON or YAML mapping as a view, beside its values, since a key is
  text a writer chose. The title is scanned as free text that reaches the journal.
- The submission event always carries `format`, including `text/plain`; an event written before
  the field reads as `text/plain` and keeps its digest, and `plaintext_digest` stays the digest of
  the source whatever its format.
- A render token seals the source it draws, the memory id, the session's CSRF token and a
  5-minute expiry, so the frame draws what the session was shown and opens for no other session; the
  route reads nothing from the store and journals nothing.
- A Mermaid card shows the title, the source, the drawing, then the format and what the claim will
  be found by; the structural diff on a card and the cross-format "format changed" line are not
  built, since the diff is an operation of the port with no caller on a page yet.

**The pinned bundle and the Chromium check (d9, d14).** Mermaid 12.3.0 (`mermaid.min.js`, MIT,
5,537,838 bytes, SHA-256 `336df803…74b9a3`, from the npm registry on 2026-11-06), vendored under
`crates/transports/oml-transport-http/assets/` with its digest in the file name, compiled in with
`include_bytes!`, and held to that digest by a test. In Chrome for Testing 153.0.8010.12 (the Playwright cache's headless
shell) the bundle drew a flowchart, a sequence, a class, a state and an entity-relationship diagram
under the frame's own policy, whose `script-src` names the bundle and one hashed initialiser and has
no `unsafe-eval`, and a source that does not parse printed Mermaid's message in the frame
(`tests/render_frame_chromium.rs`). Because the check passed, the frame ships on by default, and
no flag exists. FR-110 still asks for the bundle's size cost and its render time, unmeasured.

**Not built in this change.**

- `formats.enabled` and the doctor line for rows in a disabled format (d3, d14): the list is the
  compiled-in one.
- The adapter-upgrade refill of `search_text` (d4): no adapter has had a second version yet, so no
  row can be behind; the check that `search_text_by` equals the adapter's version, and the doctor
  line for the backlog, come with the first change to an adapter.
- `oml_format_refusals_total` (d14), and the doctor lines for the served bundle's digest and for
  the frame route's policy compared with d9's. The formats line of `oml doctor`, from `/readyz`,
  is built.
- `oml sync` calling the adapters' `diff` (d11, F6): the diff is built and tested in `oml-format`,
  and the CLI's `oml write --format` and `--title` are built, with `-` reading the text from
  standard input.
- The recall eval's format rows and the task harness's diagram arm (d12, FR-112): until they pass,
  the plugin's skill does not suggest a format (F7).

## Test plan

Laws in `oml-format`'s tests and `oml-port-contracts`, run on every adapter, with property tests over
generated sources where named:

- `omitting_format_stores_text_plain_and_indexes_exactly_as_today` (postings equal to a v0.5.8 build's)
- `the_signature_covers_the_format_and_the_stored_source_bytes`
- `a_format_changes_only_by_supersession`
- `an_event_without_a_format_reads_as_text_plain_and_keeps_its_digest`
- `search_text_is_the_same_on_every_run_and_platform` (property)
- `the_server_and_the_cli_compute_the_same_search_text_and_the_same_diff`
- `a_mermaid_relation_becomes_one_sentence_in_source_order`
- `a_mermaid_comment_never_reaches_the_search_text`
- `a_mermaid_directive_click_or_style_is_refused_by_name`
- `an_unsupported_mermaid_diagram_is_refused_by_name`
- `a_bidirectional_control_is_refused_in_every_non_prose_format`
- `a_json_duplicate_key_is_refused`; `a_yaml_alias_tag_or_second_document_is_refused`
- `the_scans_findings_on_the_source_alone_are_a_subset_of_its_findings_on_the_source_and_its_views` (property)
- `the_scan_vetoes_a_private_key_hidden_by_a_json_escape`
- `the_scan_vetoes_a_private_key_in_a_folded_yaml_scalar`
- `a_value_under_a_secret_shaped_key_is_vetoed_naming_the_path_never_the_value`
- `a_placeholder_under_a_secret_shaped_key_passes`
- `no_secret_shaped_value_reaches_the_search_text_or_the_postings`
- `reordered_json_keys_and_reindented_yaml_are_equivalent`
- `an_added_mermaid_edge_is_one_change_naming_the_edge`
- `an_adapter_upgrade_refills_search_text_and_never_rewrites_a_source`
- `the_reranker_scores_the_search_text`
- `a_hit_carries_its_format_on_every_surface` (HTTP, MCP, both SDKs)
- `a_non_prose_claim_without_a_title_is_refused_naming_title`

In Chromium, against the lead's pages, skipped by name where no browser is present:

- `the_parent_pages_carry_no_script_and_frame_only_the_render_route`
- `the_render_frame_runs_only_the_pinned_bundle_and_reaches_no_network` (CSP violation reports)
- `the_render_frame_cannot_read_the_session_cookie_or_the_parent`
- `the_pinned_bundle_draws_the_five_types_without_unsafe_eval`
- `the_review_card_shows_the_source_when_the_rendering_fails`
- `a_render_token_is_bound_to_its_session_and_version_and_expires`

## Mockup

[Review card mockups](../design/claim-formats/mockups.html): a held Mermaid supersession on the admin
page's review card, with its source, its rendering in the frame, its search text and its structural
diff; the same card when the rendering fails; and a JSON fragment refused by the scan. Every field,
layout and value there is illustrative.
