<!--
id: adr/ADR-0056-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents
tier: engineering
status: proposed
answers: why source mapping's first slice is its own record beside ADR-0047; what a memory's anchor holds, who declares it, what the server checks and what it never computes; how recall and the listing filter by origin and how a hit shows its anchor and whether its source moved; what `oml sync` reads, how its adapters extract, how claim keys and hashes are derived, how it diffs against a workarea and what each outcome writes; how runs are batched for bulk review and kept idempotent; how cross-document edges are proposed and ratified; how staleness is checked and confirmed; how ADR-0047's eleven questions are answered and which this slice exercises; what v0.5.8 builds and what waits for code; how OML's own ADRs are loaded and re-synced; the tests, doctor lines and costs; the contract the build lane starts from
-->

# ADR-0056 — A memory carries the source anchor its author declares, and `oml sync` keeps a workarea in step with a repository's documents

**Status:** proposed · 2026-11-05

**Release:** planned for v0.5.8 ([releases](../product/releases.md#planned-releases))

**Amends:** ADR-0047 (d3, d12, d15 a, d17, and its open questions, answered here and in its own
answers table), ADR-0048 (d5: two of S4's filters built ahead of the stage), ADR-0039 (d3, d4:
one argument on `oml_write`, two on `oml_read`; d10: `oml sync`, `oml anchor`), ADR-0055 (d2: the
listed item carries the anchor); and, on its branch, ADR-0050 (d5: `sources` stay free citations
beside the one structured anchor)

**Depends on:** [ADR-0055](https://github.com/e8kor/open-memory-lab/pull/62) (bulk review, planned for
v0.5.7) for reviewing a load; [ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)
for edges.

## Context

**The operator's decision of 2026-11-05.** OML maps source code, projects and wikis into memories,
and every memory traces to its origin. v0.5.8 is the first slice: an anchor on every memory,
`oml sync` for markdown documents with staleness by commit, and OML's own ADRs loaded through it,
with the edges between them (amends, supersedes) proposed for ratification. ADR-0047's eleven
open questions are settled at its own recommended answers.

**Why a record of its own, and not an amendment inside ADR-0047.** ADR-0047 is proposed, has no
release, and gates everything past its skill text on an evaluation harness (d17) and on the
in-progress workareas of ADR-0030 (d5–d7). This slice needs neither: it adds no hook, injects
nothing into a session, creates no workarea per branch and serves no new kind. Written into
ADR-0047, the register would have to show one record as both "planned for v0.5.8" and "gated,
not assigned", and its `dN` handles, which later records amend by number, would shift. A separate
record can be accepted with v0.5.8 while ADR-0047 stays proposed for code; ADR-0047 points here
for its anchor (d3), its import (d12), its anchor rule (d15 a) and its gate's scope (d17), and
records its answers in its own table.

**The extractor this slice replaces.** A script written on 2026-11-05
(`adr_extract.py` and `adr_load.sh`, kept outside the tree) splits OML's ADRs into one memory per
decision plus a summary and a rejected-alternatives memory per ADR, under claim keys
`adr-NNNN-dN`, `adr-NNNN-summary` and `adr-NNNN-rejected`: 53 ADRs and 614 memories, reading
`main` and eight draft branches. Its claim keys and its split are kept. Four of its choices are
not, each measured on `origin/main` at `ca61c9e` (45 ADRs) on 2026-11-05:

- it cuts every text at 900 characters, and 218 of the 435 decisions are longer (median 902,
  90th percentile 2,713, longest 10,955), so half the decision memories end mid-decision;
- it reads rejected alternatives as bullets, and 27 of the 43 ADRs that have that section write
  it as a table, so it yields 16 rejected-alternatives memories where there are 43;
- it puts the origin in the text (`Source: docs/adr/….md`) and the record's status in every
  decision's text, so an accepted status rewrites every decision and nothing can filter by origin;
- its idempotency keys carry the load's date, and the server keeps an idempotency record for at
  least 24 hours ([HTTP API](../api/http-api.md#idempotency)), so a second load on another day
  writes everything again.

**What it is held against.** The constitution ([ADR-0001](ADR-0001-the-constitution.md)): R1, no
agent key promotes, proposes or ratifies; R2, the reviewer is never the author, per key and
principal; R3, a held row is read by its author and its reviewer; R4, nothing is deleted. The
Occam rule ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8 and the
[use-case call map](../engineering/use-case-call-map.md)): every new call traces to a documented
use case, and a capability is a parameter before it is a tool.

Decided, not built.

## Decision

- **d1 — Any memory may carry one anchor, the origin its author declares.** The anchor is
  `{repo, rev, path, fragment?, hash?, hasher?}` for a file in a git repository, or
  `{uri, rev?, fragment?, hash?, hasher?}` for a page outside one. `repo` names the repository
  (d3), `rev` the commit the author read, `path` the file, `fragment` the part of it (a heading's
  slug, or an ADR's `dN`), and `hash` the digest of that part's text under the named `hasher`
  (d7). It is optional on every kind, so ordinary writes are unchanged; ADR-0047 d15 a's "required
  on kind `code`" stands for when `code` is built, and its "refused on every other kind" is
  withdrawn. This replaces ADR-0047 d3's field list: `commit` is `rev`, `symbol` is `fragment`
  with ADR-0047 d4's hasher `oml-span/1`, and `branch` is added by ADR-0047 d5 when branch
  workareas are built. ADR-0050 d5's `sources`, proposed on its branch, stay free citations
  (a URL, an issue, a session); the anchor is the one structured origin, the one a read filters
  by and staleness compares.

- **d2 — The anchor is part of the signed submission, immutable per version, unverified, and
  shown to the reviewer.** It is journalled by value in `submission.remember` and
  `submission.learn` beside `derives_from`, so the chain signs it, and a version's anchor never
  changes after capture. A supersession declares its own anchor, inheriting nothing from its
  target, as attachments and terms are not inherited
  ([openapi-designed](../api/openapi-designed.yaml)); a successor sent without one is unanchored.
  The server checks the grammar (d3), scans it, stores it and computes nothing from it: it never
  fetches the repository or the page, and the agent tools keep `openWorldHint: false`. Like
  `derives_from` it is the author's statement, and the reviewer judges it: `review next`, the
  bulk listing `GET /v2/review/held` (ADR-0055 d2) and both web pages' review views show it
  beside the text. It is clear metadata, like ADR-0021 d8's terms, so crypto-shredding never
  erases it ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d3, d5): the server's
  deterministic scan runs over `repo`, `uri`, `path` and `fragment`, and a finding refuses the
  write with `submission.scan_veto` naming `/anchor/…`. The cost: a path, once written, is kept
  for good, and a repository whose file names are themselves sensitive should not be anchored.

- **d3 — The server checks one grammar and refuses anything else with `request.validation`
  naming the field.**

    | Field | Rule |
    |---|---|
    | `repo` | Lower case, 1 to 200 characters, 1 to 8 segments joined by `/`, each `[a-z0-9][a-z0-9._-]*` and not `.` or `..`; no `@`, `:` or whitespace, so no URL scheme, port or credentials. `oml sync` and `oml anchor` derive it from the `origin` remote, dropping the scheme, any user and password, and a trailing `.git` (`git@github.com:e8kor/open-memory-lab.git` → `github.com/e8kor/open-memory-lab`), or take `--repo`. Exactly one of `repo` and `uri`. |
    | `uri` | An absolute `https` URL of at most 2,048 characters, no user information, no `#` (the fragment is its own field). |
    | `rev` | With `repo`, required: 40 or 64 lower-case hex digits, a full git object id. With `uri`, optional: 1 to 128 printable ASCII characters without whitespace (a page version or an ETag). |
    | `path` | With `repo`, required; with `uri`, refused. NFC, a relative POSIX path of at most 1,024 UTF-8 bytes: no leading `/`, no `\`, no empty, `.` or `..` segment, no control character. |
    | `fragment` | Optional. NFC, 1 to 256 characters, no whitespace, control character or `#`. Absent means the whole file or page. |
    | `hash`, `hasher` | Both or neither. `hash` is `sha256:` and 64 lower-case hex digits; `hasher` is one of a closed set, `oml-md/1` (d7) in v0.5.8, which ADR-0047 d4 extends with `oml-span/1`. |

    An anchor without a hash is a citation that staleness can never check (d11): a phone agent
    citing a decision it read can still say where it came from. Unknown members of the anchor are
    refused, as every closed body refuses them (ADR-0025 d16).

- **d4 — Recall and the listing filter by origin, inside the candidate statement; no route and no
  tool is added.** `POST /v1/memories/recall` and `GET /v1/memories` take `anchor_repo` (equal
  to the anchor's `repo`) and `anchor_path_prefix` (a byte prefix of its `path`), each optional,
  combined with each other and every existing filter by AND; either one leaves out unanchored
  rows. They are two of the filters ADR-0048 d5 declares for its stage S4, built ahead of the
  rest because this slice's use cases need them (d15): a predicate in the candidate statement,
  before ranking's cut, so a filtered recall never under-returns by discarding candidates after
  it. The listing by origin is `GET /v1/memories?anchor_repo=…&anchor_path_prefix=…`, with
  `own=true` for the key's own rows in every state and without it for the promoted tips of the
  subtree; it is what `oml sync` diffs against (d8) and what "what do we know about this file"
  asks. `oml_read` gains the same two arguments in recall and listing modes (amends ADR-0039 d4).
  `anchor_symbol` (ADR-0048 d5) and a `uri` prefix filter wait for their first use case: code
  symbols and a wiki adapter.

- **d5 — Every claim item carries `anchor` and `stale`, and `stale` is computed from what a
  writer submitted, never from the source.** `anchor` is the version's anchor or `null`.
  `stale` is `changed` when the claim has a held successor that is not a retirement and carries
  an anchor differing from the served version's in `repo` or `uri`, `path`, `fragment` or `hash`;
  `removed` when it has a held retirement that carries an anchor; `null` otherwise. `oml sync`
  writes exactly those successors when a source changes (d8), so a hit says "the source moved;
  a version read from the newer source waits for review" from the moment the sync runs, on every
  surface, to readers with no checkout. The mark is as fresh as the last sync and no fresher:
  between a commit and the next sync nothing is marked, and FR-109 tracks that gap.
  **A reading of R3 this takes:** the mark tells a reader of the served version that a held
  successor exists, never its text, author, id, version or anchor. R3 governs who reads a held
  row; the existence of held versions already leaks to a successor's writer through version
  ordinals, a stated residual (FR-31), and this widens it to readers of an anchored tip. The
  operator confirms or refuses that reading (the PR's question); refused, `stale` is computed by
  `oml sync --check` and the CLI alone, and agents without a checkout see the anchor and no mark.

- **d6 — `oml sync <source> --workarea <workarea>` reads committed markdown at one commit, through
  the HTTP API under the caller's key.** `<source>` is a file or directory inside a git clone;
  the run reads the tree at `--rev` (default `HEAD`) through git's object store, never the
  working tree, so an untracked, ignored or uncommitted file is never read and staleness is by
  commit. It refuses a source outside a git clone, a `--rev` that is not a commit, a path whose
  name matches the client's secret patterns (ADR-0047 d3's list: `.env*`, `*.pem`, `*.key`,
  `id_*`, `*credentials*`, `*secret*`), and, as ADR-0047 d16 and question 11 have it for client
  code, the maintainer's public server `https://memory.doublegate.io`
  ([ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)) unless
  `--allow-public-server` is given, after printing that the server's admins read every memory
  and that a written anchor and text are never erased. It writes kind `memory` only; ADR-0047's
  `code` kind is a later flag. It writes through `POST /v1/memories` and
  `…/{artifact_id}/supersede`, one request per memory, honouring `Retry-After`; the batch route
  stays unbuilt (ADR-0047 d15).

- **d7 — Adapters are a port in the client; v0.5.8 builds `adr` and `headings`, and each is a pure
  function of a path and its bytes.** An adapter takes `(path, bytes)` and returns, in document
  order, items `{claim_key, fragment, text, span}` and edge statements `{from, label, to}`; the
  same input yields byte-identical output, and no adapter reads the clock, the network, the
  environment or another file. `--adapter` chooses one; omitted, `adr` for a directory whose
  files are named `ADR-NNNN-*.md` and `headings` otherwise. The hasher `oml-md/1` hashes a span:
  UTF-8 (an invalid file is reported `unreadable` and skipped), NFC, line endings to LF, every
  run of whitespace to one space, trimmed, SHA-256. Reflowing a paragraph keeps the hash; a
  changed word, link or punctuation mark changes it.
    - **`adr`** reads `docs/adr/ADR-NNNN-*.md`. One item per decision bullet `- **dN — …**`
      under `## Decision`, its span the bullet with its nested lines, its key `adr-NNNN-dN`, its
      fragment `dN`, its text `ADR-NNNN dN: ` and the bullet in plain text (links to their text,
      emphasis and code marks dropped). One summary, key `adr-NNNN-summary`, no fragment, its
      span the title, the header's bold lines (`Status`, `Release`, `Amends`, `Supersedes`,
      `Depends on`), the first paragraph of `## Context` and each decision's bold headline; its
      text the title, the status, that paragraph and the headlines. One rejected-alternatives
      item, key `adr-NNNN-rejected`, fragment `rejected-alternatives`, its span that section,
      read as a table's rows (`alternative: why not`) or as bullets, whichever it holds. No text
      is cut: a cut memory states less than its anchor claims, and the recall budget already
      cuts by tokens. No text names its path or the record's status, except the summary's
      status: the anchor carries the origin, and an accepted status rewrites one summary, not
      every decision. Other sections (Consequences, Downsides, open questions) are not loaded;
      the decisions restate what binds. Edge statements come from the `**Amends:**` and
      `**Supersedes:**` lines (d10).
    - **`headings`** reads every `*.md` under the source. One item per heading of level 2 to 4
      with body text before the next heading of any level; its span that heading and body; its
      fragment the heading's slug as GitHub renders it (lower case, punctuation dropped, spaces
      to `-`, a repeated slug suffixed `-1`, `-2`); its text the document's title, the heading
      path and the body in plain text; its key `doc/` and the path without `.md`, characters
      outside `[a-z0-9._/-]` mapped to `-`, then `/` and the slug. HTML comments and front matter
      are skipped. It states no edges.

    Every key fits ADR-0050 d2's proposed grammar (1 to 128 of `a-z 0-9 . _ : / -`); a key that
    would be longer keeps its first 111 characters and ends in `-` and the first 16 hex digits of
    the SHA-256 of the full key. A claim key is a function of the anchor's path and fragment, so
    a workarea's memories of one source are found by key as well as by anchor.

- **d8 — The run diffs the adapter's items against the workarea by claim key, and each outcome has
  one action.** It lists the key's own rows and the subtree's promoted tips anchored in this
  repository under the source's path (d4), and for each item and each listed claim decides:

    | Outcome | When | Action |
    |---|---|---|
    | `new` | no held, promoted or demoted version of the key lives in the workarea | `POST /v1/memories` with the claim key, text and anchor |
    | `changed` | the tip's anchor differs from the item's in `path`, `fragment` or `hash`, or its `plaintext_digest` differs from the item text's | supersede the tip with the item's text and anchor |
    | `removed` | a listed claim anchored under the source's path is yielded by no item at this `rev` | supersede with `retirement: true`, the text naming the repository, `rev` and path, the anchor without a hash |
    | `unchanged` | anchor and text digest equal | nothing; a newer `rev` with the same hash is not a change |
    | `pending` | the claim has a held version | nothing: one held version per claim, so the run never needs the unbuilt revision of a held version (RS-17); the next run after review takes it |
    | `diverged` | the tip has no anchor, or one in another repository | nothing, unless `--adopt`, which supersedes it as `changed` |
    | `foreign` | the tip was written by another key | nothing; a writer may supersede only its own tip ([roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)) |
    | `conflict` | the write answers `submission.claim_key_conflict` | nothing; another key holds the key |

    A claim whose tip is promoted and whose source changed again before review stays `pending`
    until its held version is judged. The run reads committed content only, so the outcome of a
    run at a `rev` against a workarea's state is fixed; a second run at the same `rev` writes
    nothing.

- **d9 — A run is idempotent, capped for bulk review, and shows its plan before it writes.**
    - **Order:** by path, then document order; a path's retirements after its writes.
    - **Idempotency:** each write's key is `sync:<workarea>:<claim_key>:<outcome>:` and the first
      16 hex digits of the span hash (`removed` for a retirement), so a retried run replays inside
      the server's window, and the diff (d8) keeps a later run from repeating a write after it.
    - **Batches:** a run writes at most `--max-writes` items, 200 unless set, the request cap of
      ADR-0055 d3, so one run is one bulk verdict request; the next run continues where it
      stopped. It ends by printing the review command for what it wrote:
      `oml review list --workarea <w> --author-key <key>`, then
      `oml review approve --ids -`.
    - **Dry run:** `--dry-run` sends no write and prints one line per item (outcome, claim key,
      anchor) and the counts; `--json` prints the plan as one object (Contract sketch), the same
      plan the run then executes, byte for byte, while the workarea does not change between
      them.
    - **Exit codes:** 0 when every planned write was accepted; 1 when any item was `diverged`,
      `foreign`, `conflict`, `unreadable` or refused; 2 for a usage error.

- **d10 — Edges between documents are proposed by a person's dreamer key after the claims promote,
  and ratified by another.** The `adr` adapter reads `**Amends:** ADR-0025 (d10, d12), ADR-0039
  (d3)` as edges labelled `amends` from the amending record's summary to each named decision
  (`adr-0025-d10`, `adr-0025-d12`, `adr-0039-d3`), or to the target's summary where no `dN` is
  named; `**Supersedes:**` the same with label `supersedes`. A target not in the workarea is
  reported `unresolved`. Inverse lines (`amended by` on a Status line) are not read, so the two
  can never disagree inside OML. `amends` and `supersedes` are open labels (ADR-0046 d6); a claim
  edge never moves a tip, so `supersedes` between records is not OML's supersession.
  `oml sync <source> --workarea <w> --edges` proposes, under the calling key, each stated edge
  between the two claims' promoted served tips that has no edge with the same endpoints and
  label, through `POST /v2/edges {kind: claim}`; an edge with an endpoint not yet promoted is
  `waiting`. Claim edges need dreamer (ADR-0046 d4), so the sync's writer key cannot propose
  them and no agent key does (R1); the operator runs it under a dreamer key of their own
  (`held_by: human`). `oml sync … --edges --ratify`, under a second dreamer key of another
  principal, re-derives the stated edges from the source and ratifies each pending edge that
  matches one, one `POST /v2/edges/{edge_id}/ratify` each; ADR-0046 d2's `same_user` marks the
  solo case. When an endpoint is superseded, its old edges stay on the old version, which recall
  no longer serves, and the next `--edges` proposes the edge between the new tips: each changed
  endpoint costs one proposal and one ratification per edge. An edge whose statement leaves the
  documents is reported `unstated` by `--check` and is not withdrawn, since no route withdraws an
  edge (FR-112).

- **d11 — Staleness is checked against a commit, and a confirmation is a reviewed supersession,
  never a silent re-anchor.** `oml sync <source> --workarea <w> --check` runs d8's diff at
  `--rev` without writing and prints each anchored claim as `fresh`, `stale` (`changed`),
  `removed`, `pending`, `untracked` (in the source, not in the workarea) or `unhashed` (anchored
  without a hash), and each edge as `stated`, `waiting` or `unstated`; it exits 1 when any claim
  is `stale`, `removed` or `untracked`, so CI on the repository can run it after every merge.
  Hits show `stale` (d5). **ADR-0047 question 1, answered a:** no route changes an anchor in
  place. A span whose hash changed while its extracted text did not (a renamed file, a moved
  heading, a changed link) is still `changed`, and the run writes a successor with the same text
  and the new anchor; the reviewer sees the text unchanged and the anchor moved, and approves
  that the memory still reads true of its new source. The cost is one review per moved span.

- **d12 — ADR-0047's eleven questions are answered at its recommendations, and question 10 is
  narrowed by bulk review.** The answers are recorded in [ADR-0047](ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle.md#the-maintainers-answers-of-2026-11-05);
  this slice exercises three of them:

    | # | Answer | Exercised by v0.5.8 |
    |---|---|---|
    | 1 | a: a confirmation is a supersession with the same text and a fresh anchor, reviewed | yes, d11 |
    | 2 | a: one workarea per branch, merge = commit | no: branch workareas wait (d13) |
    | 3 | a: a release-record child per release | no |
    | 4 | a: the reviewer proposes the merge commit, the CI key ratifies | no |
    | 5 | a: a writer creates branch workareas under an admin-marked branch parent | no |
    | 6 | a: compaction is a journalled erasure by a person's act, after ADR-0005 | no |
    | 7 | a: counts and a link to the forge, text only on `--post-text` | no |
    | 8 | a: OML beats no memory and is no worse than the `AGENTS.md` export, at no more tokens | no: the gate's scope is narrowed (d13) |
    | 9 | a: the agent writes the purpose line, a person approves | no |
    | 10 | a for a pull request's model-written knowledge: 20 entries; for a document sync, 200 items per run (d9) | yes, d9 |
    | 11 | a: the public server only with `--allow-public-server` | yes, d6 |

    Question 10's 20 bounded a person reading prose an agent wrote. A sync's items are extracted
    deterministically from documents already reviewed in their own pull requests, so the reviewer
    checks that an extraction is faithful, not that a claim is true, and ADR-0055's listing lets
    a person read 200 and judge them in one request. Both caps remain chosen, not measured
    (FR-79, FR-111).

- **d13 — v0.5.8 builds anchors, markdown sync, staleness and the ADR load; code waits.** Built:
  d1–d11, `oml anchor <path>[#fragment]` (prints the anchor of a committed file or fragment at
  `HEAD`, so an agent with a terminal can pass it to `oml_write`; ADR-0047 d3's verb, for
  documents), and `oml write --anchor`. Waiting, as ADR-0047 has them: kind `code`, the skeleton
  and the CI key's re-derived approvals (d1, d2, d8, d9); symbol spans and `oml-span/1` (d4);
  branch workareas, pull requests as review batches and merges as commits (d5–d7); the release
  record (d10); the hooks that mark and ask (d11, d13); `oml code export` (d12's output half);
  author revision of a held version (d15 c, RS-17); `anchor_symbol` and a `uri` adapter for wikis.
  **ADR-0047 d17's gate is narrowed** to those parts: its harness measures whether stored code
  knowledge helps an agent finish a task, and this slice stores no code knowledge, changes no
  default read (both filters are off unless sent, and recall unfiltered is byte-identical) and
  adds no session injection. Whether anchored hits are used is measured by ADR-0054's usage
  telemetry (planned for v0.5.7) like any hit.

- **d14 — OML's own ADRs are loaded into `oml-design` under the operator's root, reviewed in bulk,
  and re-synced after every change on `main`.**
    1. **The workarea.** `oml workareas create e8kor oml-design`, a child of the operator's user
       root ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)),
       `development` like every new workarea.
    2. **The keys.** The operator issues a writer key for the sync, `e8kor-design-sync`, with
       writer and reader on `oml-design`, to an owner-only file (`--out`). If the 2026-11-05
       script already loaded its memories, the sync runs under the key that wrote them
       (`e8kor-adr`), since a writer supersedes only its own tips (d8 `foreign`). Reviews and
       edges use two dreamer keys `held_by: human` with distinct principals: the reviewer key
       that runs `oml review` and `oml sync --edges`, and a second that runs `--edges --ratify`.
    3. **The source.** `docs/adr` of `github.com/e8kor/open-memory-lab` at a commit of `main`.
       Draft records on branches are not loaded: a sync reads one commit, and a later sync of
       `main` would retire anything read from a branch (d8 `removed`). A draft enters when it
       lands on `main`. The PR asks the operator whether drafts should load into a child
       workarea per draft instead.
    4. **The first load.** `oml sync docs/adr --workarea oml-design --adapter adr
       --allow-public-server --dry-run`, then three runs without `--dry-run`. At `ca61c9e` the
       plan is 523 items (435 decisions, 45 summaries, 43 rejected-alternatives memories) and 117
       `amends` edges with none unresolved, so three batches of 200, 200 and 123. If the
       2026-11-05 load ran, its claims are `diverged` (no anchor) and the runs take `--adopt`:
       each becomes a supersession carrying the anchor and the uncut text, reviewed the same way.
    5. **Review.** After each run, `oml review list --workarea oml-design --author-key
       e8kor-design-sync` (or `e8kor-adr`) to read the batch, then the same with `--ids-only`
       piped to `oml review approve --ids -`; each verdict is marked `same_user` (ADR-0055 d7).
    6. **Edges.** Once the batches are promoted, `--edges` under the reviewer key and
       `--edges --ratify` under the second.
    7. **Re-sync after an ADR changes.** After every merge to `main` that touches `docs/adr`, the
       operator (later CI, with ADR-0047 d8's key) runs, at the merge commit, `--check`, then
       `--dry-run`, then the sync: a changed decision is `changed` and superseded, a new one
       `new`, a deleted one `removed` and retired, and the record's summary `changed` whenever its
       status, header or a headline moved. The served versions show `stale` until review; then
       `--edges` and `--edges --ratify` for the superseded endpoints, and `--check` exits 0. A
       renumbered decision is a changed text under its old key: decision handles are stable by the
       register's rule ([template](TEMPLATE.md)), and a renumbering costs a review per moved handle.

- **d15 — Each new call traces to a documented use case, and no MCP tool is added.**

    | Call | Use case |
    |---|---|
    | `anchor` on `oml_write`, `POST /v1/memories`, `…/supersede`; `oml write --anchor`; `oml anchor` | UC-SM-01, say where a memory came from |
    | `anchor_repo`, `anchor_path_prefix` on recall, the listing and `oml_read`; `oml read --anchor-repo --anchor-path-prefix` | UC-SM-02, find what is known about a file or folder |
    | `anchor` and `stale` on every claim item; `oml sync --check` | UC-SM-03, see that a memory's source changed |
    | `oml sync`, its dry run and its batches | UC-SM-04, load a repository's documents and keep them in step |
    | `oml sync --edges`, `--edges --ratify` | UC-SM-05, link the decisions documents say amend each other |

    The [use cases](../product/use-cases.md#sources) and the
    [call map](../engineering/use-case-call-map.md#sources) declare them. `oml_write` gains one
    argument and `oml_read` two; the catalog stays three tools (ADR-0039 d8).

- **d16 — The build, each step closed by named tests.**
    1. **Server:** `an_anchor_is_optional_on_every_kind_and_journalled_in_the_submission_event`,
       `a_successor_declares_its_own_anchor_and_inherits_none`,
       `a_malformed_anchor_is_request_validation_naming_its_field` (one case per row of d3),
       `a_secret_shaped_anchor_path_is_vetoed_by_the_scan`,
       `review_next_and_the_held_listing_serve_the_anchor`,
       `an_anchor_filtered_recall_never_under_returns`,
       `recall_without_anchor_filters_is_byte_identical_to_v0_5_7`,
       `a_tip_with_a_held_anchored_successor_of_another_hash_is_stale_changed`,
       `a_tip_with_a_held_anchored_retirement_is_stale_removed`,
       `the_stale_mark_reveals_no_text_author_id_or_version_of_the_held_row`.
    2. **Adapters:** `extraction_is_a_pure_function_of_path_and_bytes`,
       `the_adr_adapter_yields_each_decision_a_summary_and_rejected_alternatives_from_a_table_or_bullets`,
       `no_extracted_text_is_cut_or_names_its_path`,
       `the_md_hash_ignores_reflow_and_changes_with_a_word`,
       `the_headings_adapter_slugs_repeated_headings_as_github_does`,
       `a_claim_key_over_128_characters_ends_in_a_digest`.
    3. **Sync:** `sync_reads_the_tree_at_rev_and_never_the_working_tree`,
       `sync_derives_repo_from_the_remote_without_its_credentials`,
       `the_diff_writes_new_supersedes_changed_retires_removed_and_skips_unchanged`,
       `a_claim_with_a_held_version_is_pending`, `an_unanchored_tip_is_diverged_unless_adopt`,
       `a_second_run_at_the_same_rev_writes_nothing`,
       `a_retried_run_replays_by_idempotency_key`,
       `a_run_writes_at_most_max_writes_and_the_next_continues`,
       `the_dry_run_plan_equals_the_runs`,
       `a_moved_span_with_the_same_text_is_a_supersession_never_a_reanchor`,
       `sync_refuses_the_public_server_without_allow_public_server`,
       `check_exits_1_on_stale_removed_or_untracked`.
    4. **Edges:** `edges_are_proposed_only_between_promoted_tips_under_a_dreamer_key`,
       `ratify_ratifies_only_pending_edges_the_source_states`,
       `an_unresolved_target_is_reported_and_not_proposed`.
    5. **The load:** `the_adr_load_at_ca61c9e_plans_523_items_and_117_amends_edges`, against a
       fixture of that commit's `docs/adr`.

- **d17 — Every new failure has a doctor line.** `oml doctor` names: a server that does not
  accept anchors (before v0.5.8), which `oml sync` also refuses before reading anything; an
  `origin` remote that carries credentials in its URL, which sync strips and the line asks to
  remove from `.git/config`; the public default server without `--allow-public-server` recorded
  for this clone; and, for the key's default workarea, the count of served versions marked
  `stale` and of claim edges waiting for a second key.

## Contract sketch

The build lane starts from this after v0.5.7. At the build, each piece moves into
[openapi.yaml](../api/openapi.yaml), [mcp-tools.json](../api/mcp-tools.json) and the
[CLI reference](../cli/reference.md); until then the request fields sit in
[openapi-designed.yaml](../api/openapi-designed.yaml) under `x-oml-designed-fields`.

**The schema.**

```yaml
Anchor:
  type: object
  additionalProperties: false
  description: The origin the author declares (ADR-0056 d1-d3). Signed, immutable per version,
    never fetched, never erased.
  oneOf:
    - required: [repo, rev, path]
    - required: [uri]
  dependentRequired: { hash: [hasher], hasher: [hash] }
  properties:
    repo:     { type: string, maxLength: 200, pattern: '^[a-z0-9][a-z0-9._-]*(/[a-z0-9][a-z0-9._-]*){0,7}$' }
    uri:      { type: string, format: uri, maxLength: 2048, pattern: '^https://[^@#\s]+$' }
    rev:      { type: string, maxLength: 128, description: 'With repo, 40 or 64 lower-case hex.' }
    path:     { type: string, description: 'Relative POSIX path, at most 1,024 UTF-8 bytes; no .. segment.' }
    fragment: { type: string, minLength: 1, maxLength: 256, pattern: '^[^\s#\x00-\x1f]+$' }
    hash:     { type: string, pattern: '^sha256:[0-9a-f]{64}$' }
    hasher:   { type: string, enum: [oml-md/1] }
Stale:
  type: [string, 'null']
  enum: [changed, removed, null]
  description: Computed at read from the claim's held successor (ADR-0056 d5); never from the source.
```

**Requests.** `SubmitRequest` and `SupersedeRequest` gain `anchor: Anchor`. `RecallRequest` and
`GET /v1/memories` gain `anchor_repo` (string, the `repo` grammar) and `anchor_path_prefix`
(string, at most 1,024 bytes). **Responses.** `MemorySummary`, and so `Memory`, every recall hit,
the `review next` serve and the `GET /v2/review/held` item, gain `anchor: Anchor | null` and
`stale: Stale`. **Journal.** `submission.remember` and `submission.learn` carry `anchor` (the
object or `null`). **Storage.** One migration adds nullable `anchor_repo`, `anchor_uri`,
`anchor_rev`, `anchor_path`, `anchor_fragment`, `anchor_hash` and `anchor_hasher` to `memories`
and an index on `(tenant_id, anchor_repo, anchor_path text_pattern_ops)`; its number is the next
free one when the lane starts. **MCP.** `oml_write` gains `anchor` (the schema above);
`oml_read` gains `anchor_repo` and `anchor_path_prefix` in recall and listing modes, refused in
get mode.

**Refusals.** A malformed anchor or filter: `request.validation`, `details.field` such as
`/anchor/path`. A scanned finding in it: `submission.scan_veto`. No new code.

**CLI.**

```text
oml sync <source> --workarea <workarea>
         [--adapter adr|headings] [--rev <commit>] [--repo <repo>]
         [--dry-run | --check] [--json] [--max-writes <n>] [--adopt]
         [--edges [--ratify]] [--allow-public-server]
oml anchor <path>[#<fragment>] [--rev <commit>] [--repo <repo>]      # prints one Anchor as JSON
oml write <text> --anchor <json | @file> …                           # beside today's flags
oml read --query <q> [--anchor-repo <repo>] [--anchor-path-prefix <prefix>] …
oml read [--own | --all] [--anchor-repo <repo>] [--anchor-path-prefix <prefix>] …
```

A dry run, as it would print at `ca61c9e` (lines elided):

```text
$ oml sync docs/adr --workarea oml-design --adapter adr --allow-public-server --dry-run
source    github.com/e8kor/open-memory-lab @ ca61c9edd258  docs/adr  adapter adr  hasher oml-md/1
workarea  oml-design  key e8kor-design-sync
new       adr-0001-summary     docs/adr/ADR-0001-the-constitution.md
new       adr-0001-d1          docs/adr/ADR-0001-the-constitution.md#d1
…
plan      523 new, 0 changed, 0 removed, 0 unchanged, 0 pending; this run writes 200 (--max-writes 200)
edges     117 amends, 0 supersedes stated; proposed by --edges once both ends are promoted
```

After an edit, the same command prints lines such as:

```text
changed   adr-0047-d3          docs/adr/ADR-0047-….md#d3   sha256:1f3a…→9c2e…
removed   adr-0047-d14         docs/adr/ADR-0047-….md#d14  absent at 4b7e0c1a9d22
pending   adr-0046-summary     version 2 is held
diverged  adr-0012-d4          the tip has no anchor; --adopt supersedes it
```

`--json` prints:

```json
{
  "source": {"repo": "github.com/e8kor/open-memory-lab", "rev": "ca61c9edd25895702e4ddc89df1848aae696d9ba",
             "path": "docs/adr", "adapter": "adr", "hasher": "oml-md/1"},
  "workarea": "oml-design",
  "items": [
    {"outcome": "new", "claim_key": "adr-0001-d1",
     "anchor": {"repo": "github.com/e8kor/open-memory-lab", "rev": "ca61c9edd25895702e4ddc89df1848aae696d9ba",
                "path": "docs/adr/ADR-0001-the-constitution.md", "fragment": "d1",
                "hash": "sha256:…", "hasher": "oml-md/1"},
     "replaces": null, "text_digest": "sha256:…", "idempotency_key": "sync:oml-design:adr-0001-d1:new:…"}
  ],
  "edges": [{"from": "adr-0046-summary", "label": "amends", "to": "adr-0009-d1", "state": "waiting"}],
  "summary": {"new": 523, "changed": 0, "removed": 0, "unchanged": 0, "pending": 0,
              "diverged": 0, "foreign": 0, "conflict": 0, "writes_this_run": 200}
}
```

`--check` prints one line per claim and edge in the same form, with states `fresh`, `stale`,
`removed`, `pending`, `untracked`, `unhashed`, `stated`, `waiting` and `unstated`, and exits 1 on
`stale`, `removed` or `untracked`.

## Consequences

- **Costs.** One migration and seven nullable columns on `memories`, with one index; the anchor's
  bytes in every anchored submission event; one lookup per served hit for `stale`, against the
  claim's held versions; the filters' cost inside `bm25-postings`, unmeasured (FR-110). Review:
  523 items to load OML's ADRs, and at least two per edited record afterwards (the decision and
  the summary); every renamed file or moved heading costs a review per span; every superseded
  endpoint costs a proposal and a ratification per edge. Paths are kept for good (d2).
- **Roadmap rows.** FR-109 (the stale mark's gap between a commit and the next sync), FR-110 (the
  filters' latency and the recall eval's filtered queries), FR-111 (the review cost of a sync,
  and its cap of 200), FR-112 (an edge whose statement leaves the documents cannot be withdrawn).
  FR-79 continues for ADR-0047's caps. RS-17 stays open; this slice avoids it (d8 `pending`).
- **Pages to change at the build.** The HTTP API and openapi.yaml; mcp-tools.json and the MCP
  plugin page; the CLI reference (`oml sync`, `oml anchor`, the new flags); the event catalog
  (`anchor` on the two submission events); the Postgres schema; retrieval (the filters and
  `stale`); admission (the scan over the anchor); the erasure inventory (the anchor as clear
  metadata, beside terms); the plugin's skill (when to send an anchor); and at acceptance the
  Status lines of ADR-0039, ADR-0047, ADR-0048 and ADR-0055.
- **Concurrent branches.** The FR numbers 109 to 112 follow the highest in use on any branch on
  2026-11-05 (FR-108, on `feat/usage-measure`).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Origin only in the text**, as the 2026-11-05 extractor wrote `Source: docs/adr/….md` | Nothing can filter, list or diff by it; the commit and the hash have nowhere to go, so staleness cannot be checked; moving a file rewrites the text; and the reviewer cannot tell a claim's words from its citation. |
| **One workarea per document** | 45 workarea names now and one per new record, each kept for good ([ADR-0026](ADR-0026-names-are-keys.md) d10); recall across them needs ADR-0048 S4's multi-workarea scope, capped at four; and every `amends` edge would cross workareas. The anchor's path prefix and the claim-key prefix already group a document's memories inside one workarea. |
| **`derives_from` for document structure**, each decision deriving from its record's summary | `derives_from` is lineage of reasoning, and it widens the reviewer-exclusion closure (R2): whoever wrote a summary could never review its decisions. A decision does not follow from its summary; both are read from the same document, which is what the anchor says. |
| **A `part_of` edge from every decision to its record** | 435 proposals and 435 ratifications that say what the claim key and the anchor's path already say, and an edge read per decision to answer "what belongs to this record", which the listing by origin answers in one call. |
| **A server-side crawler** that fetches repositories and wikis and writes their memories | The server would hold credentials for private repositories, make outbound requests on a tenant's behalf, and author memories under no person's key, so R1 and R2 would have no author to check. The client already has the checkout and the credentials, and its writes are a key's like any other. |
| **Keep the extractor's 900-character cut** | 218 of 435 decisions would end mid-sentence while their anchors claim the whole decision; the recall budget already cuts by tokens. |
| **Status in every decision's text** | Accepting a record would rewrite all of its decisions and cost a review each, for a fact the summary carries once. |
| **Idempotency keys alone for re-runs** | The server keeps a record for at least 24 hours; a later run would write everything again. The diff by claim key and hash is what makes a re-run write nothing. |
| **A re-anchor act without review** (ADR-0047 question 1 b) | It would change what a promoted memory claims to come from without a second identity; a supersession with the same text costs one review and keeps every promoted change reviewed. |
| **Load draft records from their branches**, as the extractor did | A sync reads one commit, and the next sync of `main` would retire every memory read from a branch; drafts change daily and each change costs a review. They enter when they land. |
| **Propose edges under the sync's writer key** | A claim edge needs dreamer (ADR-0046 d4), and an agent key never proposes (R1). |
| **Edges between records' summaries only** | "ADR-0047 amends ADR-0025" loses which decisions; the `Amends:` lines name them, and the decision memories exist to be pointed at. |
| **A server-computed `stale` from the repository** | It is the crawler above. The mark derived from submitted successors needs no fetch. |
| **A fourth MCP tool for sync or origin reads** | ADR-0039 d8: an argument on `oml_write` and two on `oml_read` carry it, and sync is a CLI run over a checkout. |
