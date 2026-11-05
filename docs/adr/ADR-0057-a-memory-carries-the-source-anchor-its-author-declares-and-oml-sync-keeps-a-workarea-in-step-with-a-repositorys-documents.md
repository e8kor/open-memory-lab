<!--
id: adr/ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents
tier: engineering
status: accepted design · 2026-11-05
answers: why source mapping's first slice is its own record beside ADR-0047; what a memory's anchor holds, who declares it, what the server checks and what it never computes; how recall and the listing filter by origin and how a hit shows its anchor and whether its source moved; what `oml sync` reads, how its adapters extract, how claim keys and hashes are derived, how it diffs against a workarea and what each outcome writes; how runs are batched for bulk review and kept idempotent; how cross-document edges are proposed and ratified; how staleness is checked and confirmed; how a moved span, a concurrent supersession and a rollback are handled; what a fragment may name; how each retriever's list is capped before fusion; how ADR-0047's eleven questions are answered and which this slice exercises; what v0.5.8 builds and what waits for code; how OML's own ADRs are loaded and re-synced; the tests, doctor lines and costs; the contract the build lane starts from
-->

# ADR-0057 — A memory carries the source anchor its author declares, and `oml sync` keeps a workarea in step with a repository's documents

**Status:** accepted design · 2026-11-05 · proposed and accepted on 2026-11-05 with release v0.5.8,
with d18 to d22 added the same day from the competitor source studies (basic-memory, Hindsight)

**Release:** introduced in v0.5.8 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0047 (d3, d12, d15 a, d17, and its open questions, answered here and in its own
answers table), ADR-0048 (d5: two of S4's filters built ahead of the stage), ADR-0039 (d3, d4:
one argument on `oml_write`, two on `oml_read`; d10: `oml sync`, `oml anchor`), ADR-0055 (d2: the
listed item carries the anchor); ADR-0025 (d10: a supersession names the claim's latest version, d19);
ADR-0049 (d13: each retriever's list capped before fusion, d22); on its branch, ADR-0056 (claim formats, #63: its question 5,
answered in d11); and, on its branch, ADR-0050 (d5: `sources` stay free citations
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

Accepted for build in v0.5.8 on 2026-11-05, and introduced with release v0.5.8.

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
    | `fragment` | Optional. NFC, 1 to 256 characters, no whitespace, control character or `#`. Absent means the whole file or page. One that reads as a line span or a page must be a well-formed one (d21). |
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

- **d5 — Every claim item carries `anchor` and `stale`, and `stale` comes from the source alone,
  recorded on the promoted version by a source check; it is never inferred from a held version.**
  *Decided by the operator on 2026-11-05.* `anchor` is the version's
  anchor or `null`. A **source check** is a journalled statement that a version's anchored span
  was compared with the repository at a commit: `POST /v1/memories/{artifact_id}/source-check
  {rev, result, hash?}`, `result` one of `fresh`, `equivalent`, `moved`, `changed` or `removed`, `hash` the
  span's current hash, and, for `moved` alone, the span's new `path` and `fragment` (d18). It admits only a promoted, anchored served tip, and only from the
  version's author key or a dreamer on its workarea; it journals `source.checked`, changes no
  text and no anchor, and moves nothing through review, since it states an observation, as the
  anchor itself does, and is no more verified than the anchor. `oml sync` records one for every
  anchored tip it compares (d8), and `oml sync --check --record` records them without writing a
  memory, for CI. `stale` on a claim item is the version's latest check: `changed` or `removed`,
  else `null`; the item also carries `source_checked {rev, at, result, moved_to}` or `null`, `moved_to` the
  `{path, fragment}` a `moved` check recorded and otherwise `null`. Served versions are
  the only ones marked, and the mark is computed from the source, so a hit reveals nothing about
  any held row and R3 holds unread. It is the HTTP API and the CLI only; no MCP argument records
  one. Where held versions matter, `oml sync --check` reports `pending` from the key's own rows,
  which only that key lists (ADR-0025 d13). The mark is as fresh as the last check: between a
  commit and the next sync or check nothing is marked, and FR-114 tracks that gap. A newer
  check supersedes an older one's mark, so a source reverted to the anchored text reads `fresh`
  again.

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
  one action.** It lists the key's own rows and the promoted tips anchored in this repository
  under the source's path (d4), in the named workarea itself and not its children, so a child's
  claims (d14's drafts) are never diffed against a parent's source, and for each item and each listed claim decides:

    | Outcome | When | Action |
    |---|---|---|
    | `new` | no held, promoted or demoted version of the key lives in the workarea | `POST /v1/memories` with the claim key, text and anchor |
    | `changed` | the tip's anchor differs from the item's in `path`, `fragment` or `hash`, or its `plaintext_digest` differs from the item text's, and the change is not `equivalent`, `moved` or `rollback` | a source check `changed`, then supersede the tip with the item's text and anchor, naming the claim's latest version (d19) |
    | `removed` | a listed claim anchored under the source's path is yielded by no item at this `rev` | a source check `removed`, then supersede with `retirement: true`, the text naming the repository, `rev` and path, the anchor without a hash |
    | `moved` | the claim's span hash is found, with the same extracted text, at another `path` or `fragment` under the source, and no item yields the claim's own key (d18) | a source check `moved` naming the new `path` and `fragment`: nothing is written and nothing is reviewed |
    | `rollback` | the item's hash equals the anchor hash of a version of the claim older than its tip that was promoted (d20) | nothing, and the run exits 1; `--allow-rollback` writes it as `changed`, a revert the reviewer judges |
    | `unchanged` | anchor and text digest equal | a source check `fresh` (d5); a newer `rev` with the same hash is not a change |
    | `equivalent` | the hash differs, and the format's `diff` (ADR-0056 d11) finds the change equivalent and the extracted text equal | a source check `equivalent`: nothing is written and the anchor keeps its hash (d11) |
    | `pending` | the claim has a held version | nothing: one held version per claim, so the run never needs the unbuilt revision of a held version (RS-17); the next run after review takes it |
    | `diverged` | the tip has no anchor, or one in another repository | nothing, unless `--adopt`, which supersedes it as `changed`: a general capability for claims written before their source was synced, not used by d14's load |
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
      `foreign`, `conflict`, `rollback`, `unreadable` or refused; 2 for a usage error.

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
  edge (FR-117).

- **d11 — Staleness is checked against a commit; a confirmation is a reviewed supersession, never
  a silent re-anchor; and an equivalent change refreshes nothing.** `oml sync <source> --workarea
  <w> --check` runs d8's diff at `--rev` without writing a memory and prints each anchored claim
  as `fresh`, `equivalent`, `stale` (`changed`), `removed`, `pending` (the key's own held
  version), `untracked` (in the source, not in the workarea) or `unhashed` (anchored without a
  hash), and each edge as `stated`, `waiting` or `unstated`; it exits 1 when any claim is
  `stale`, `removed` or `untracked`, so CI on the repository can run it after every merge, and
  with `--record` it records d5's source checks. Hits show `stale` (d5).
    - **ADR-0047 question 1, answered a:** no route changes an anchor in place. A span whose
      extracted text is unchanged while the anchor moved within the source (a renamed file, a
      moved heading) is `moved` (d18) and costs no review; one whose text is unchanged while its
      hash changed (a changed link target) is `changed`, and the run writes a successor with the same text and the new
      anchor; the reviewer sees the text unchanged and the anchor moved, and approves that the
      memory still reads true of its new source. The cost is one review per moved span.
    - **ADR-0056 question 5, answered: no.** A change that the format's `diff` finds equivalent
      (ADR-0056 d11) and that leaves the extracted text equal and the anchor's `path` and
      `fragment` unchanged refreshes no hash, with or without review: the anchor is signed and
      immutable per version (d2), and an unreviewed hash refresh is the silent re-anchor
      question 1 refuses. Sync writes nothing and records a source check `equivalent` at the new
      `rev`, so the hit reads not stale and `--check` exits 0, while the anchor keeps the hash
      of the text the reviewer judged. The next change that is not equivalent supersedes the
      version, and its successor carries the new hash. For markdown, `oml-md/1` already gives
      a reflow the same hash; `equivalent` matters for the formats ADR-0056 adds. The cost:
      every later check re-runs the `diff` for that span until the next supersession.

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
    (FR-79, FR-116).

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

- **d14 — OML's own ADRs are loaded into `oml-design` under `oml-knowledge` in the operator's root,
  reviewed in bulk, and re-synced after every change on `main`.**
    1. **The workareas.** *Decided by the operator on 2026-11-05.* One parent,
       `oml-knowledge`, a child of the operator's user root
       ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)),
       holds one workarea per source, each `development` like every new workarea: `oml-design`
       for `docs/adr` with the `adr` adapter; `oml-design-pr-<n>` for a draft pull request's
       records (item 3), also `adr`; `oml-docs` for `docs` with `--exclude 'docs/adr/**'` and the
       `headings` adapter; and `oml-code` for the Rust sources with the `rust` adapter, from
       v0.5.9 ([ADR-0061](https://github.com/e8kor/open-memory-lab/pull/70)).
       `oml workareas create e8kor oml-knowledge`, then `oml workareas create oml-knowledge
       oml-design` and the same for `oml-docs`; a draft's `oml-design-pr-<n>` is created under
       `oml-knowledge` when its pull request is first loaded. Recall at `oml-knowledge` reads all
       of them. Each workarea answers for one source with one adapter: a run diffs only its
       adapter's claim-key namespace, keeps the adapter its claims were written by unless
       `--change-adapter`, and treats `--exclude`d paths as out of the source, never retired.
    2. **The keys.** The operator issues a writer key for the sync, `e8kor-design-sync`, with
       writer and reader on `oml-knowledge` and its children, to an owner-only file (`--out`). It
       stays the sync key for good, since a writer supersedes only its own tips (d8 `foreign`).
       The 2026-11-05 script never ran, so nothing is adopted. Reviews and
       edges use two dreamer keys `held_by: human` with distinct principals: the reviewer key
       that runs `oml review` and `oml sync --edges`, and a second that runs `--edges --ratify`.
    3. **The source.** `docs/adr` of `github.com/e8kor/open-memory-lab` at a commit of `main`.
       **Drafts** (*decided by the operator on 2026-11-05*) load
       into a workarea per draft pull request under `oml-knowledge`, `oml-design-pr-<n>`, as ADR-0047 d5 gives a
       branch its own workarea: `oml sync docs/adr/ADR-0047-….md --workarea oml-design-pr-59
       --rev origin/design/code-map-r2`, naming only the records the pull request adds or
       changes. A sync of `main` diffs `oml-design` alone (d8), so it never retires a draft's
       claims. When the pull request merges or closes, `oml sync <the same files> --workarea
       oml-design-pr-<n> --retire-all` proposes retiring every claim the key wrote there, reviewed
       in bulk like any batch; a merged record then arrives in `oml-design` through the next sync
       of `main`. The cost: a draft's decisions are reviewed twice, once as a draft and once on
       `main`, and a reader at `oml-design` sees both until the draft's retirements are approved.
    4. **The first load.** `oml sync docs/adr --workarea oml-design --adapter adr
       --allow-public-server --dry-run`, then three runs without `--dry-run`. At `ca61c9e` the
       plan is 523 items (435 decisions, 45 summaries, 43 rejected-alternatives memories) and 133
       `amends` edges with none unresolved (117 when this record was proposed, a count that read
       only the first handle of a parenthesis of `;` parts), so three batches of 200, 200 and 123.
       The documents follow: `oml sync docs --workarea oml-docs --adapter headings --exclude
       'docs/adr/**' --allow-public-server --dry-run`, then the runs.
    5. **Review.** After each run, `oml review list --workarea oml-design --author-key
       e8kor-design-sync` to read the batch, then the same with `--ids-only`
       piped to `oml review approve --ids -`; each verdict is marked `same_user` (ADR-0055 d7).
    6. **Edges.** Once the batches are promoted, `--edges` under the reviewer key and
       `--edges --ratify` under the second.
    7. **Re-sync after an ADR changes.** After every merge to `main` that touches `docs/adr`, the
       operator (later CI, with ADR-0047 d8's key) runs, at the merge commit, `--check`, then
       `--dry-run`, then the sync: a changed decision is `changed` and superseded, a new one
       `new`, a deleted one `removed` and retired, and the record's summary `changed` whenever its
       status, header or a headline moved. The served versions show `stale` from that run's source checks until their successors are approved; then
       `--edges` and `--edges --ratify` for the superseded endpoints, and `--check` exits 0. A
       renumbered decision is a changed text under its old key: decision handles are stable by the
       register's rule ([template](TEMPLATE.md)), and a renumbering costs a review per moved handle.

- **d15 — Each new call traces to a documented use case, and no MCP tool is added.**

    | Call | Use case |
    |---|---|
    | `anchor` on `oml_write`, `POST /v1/memories`, `…/supersede`; `oml write --anchor`; `oml anchor` | UC-SM-01, say where a memory came from |
    | `anchor_repo`, `anchor_path_prefix` on recall, the listing and `oml_read`; `oml read --anchor-repo --anchor-path-prefix` | UC-SM-02, find what is known about a file or folder |
    | `anchor`, `stale` and `source_checked` on every claim item; `POST /v1/memories/{artifact_id}/source-check`; `oml sync --check [--record]` | UC-SM-03, see that a memory's source changed |
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
       `a_source_check_admits_only_a_promoted_anchored_tip_from_its_author_or_a_dreamer`,
       `stale_is_the_latest_source_check_of_the_served_version`,
       `a_held_successor_never_marks_its_served_version_stale`.
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
       `an_equivalent_change_writes_nothing_and_keeps_the_anchors_hash`,
       `a_sync_of_a_parent_never_retires_a_childs_claims`, `retire_all_proposes_retiring_every_claim_the_key_wrote`,
       `sync_refuses_the_public_server_without_allow_public_server`,
       `check_exits_1_on_stale_removed_or_untracked`.
    4. **Edges:** `edges_are_proposed_only_between_promoted_tips_under_a_dreamer_key`,
       `ratify_ratifies_only_pending_edges_the_source_states`,
       `an_unresolved_target_is_reported_and_not_proposed`.
    5. **The load:** `the_adr_load_at_ca61c9e_plans_523_items_and_133_amends_edges`, against a
       fixture of that commit's `docs/adr`.
    6. **The amendments (d18–d22):** `a_renamed_file_is_moved_and_writes_nothing`,
       `a_moved_claim_changed_later_is_superseded_at_its_new_location`,
       `a_supersession_naming_an_older_version_is_version_conflict`,
       `a_rollback_to_an_earlier_promoted_hash_is_never_written`,
       `a_malformed_line_span_or_a_page_fragment_is_request_validation`,
       `no_retriever_contributes_more_candidates_than_its_cap`.

- **d17 — Every new failure has a doctor line.** `oml doctor` names: a server that does not
  accept anchors (before v0.5.8), which `oml sync` also refuses before reading anything; an
  `origin` remote that carries credentials in its URL, which sync strips and the line asks to
  remove from `.git/config`; the public default server without `--allow-public-server` recorded
  for this clone; and, for the key's default workarea, the count of served versions marked
  `stale` and of claim edges waiting for a second key.

- **d18 — A span found by its hash at a new path or fragment is `moved`: a source check records
  where it went, and nothing goes back through review.** *Added 2026-11-05 from basic-memory's
  move detection (`indexing/change_detector.py:128-142`).* Before the diff names a claim
  `removed` and an item `new`, it pairs them when the item's span hash and extracted text equal
  the claim's anchor hash and text: the claim is `moved`, the item is not `new`. The anchor still
  states the truth at its own `rev`, which names the commit the author read, so the version is
  not superseded; the run records a source check `moved` carrying the new `path` and `fragment`,
  which reads not stale (d5) and which the next run locates the claim by: a listed claim's
  location is its anchor's, or the `moved_to` of its latest check when that is `moved`. A later
  change of the moved span is `changed` at its new location, and the successor carries the new
  anchor and keeps the claim's key, which never changes once written. A move out of the synced
  source (another directory than `<source>`) is not seen by that run: it reads `removed` and
  `new`, as before. The cost: a claim key is no longer always a function of its anchor's path,
  so a moved claim is found by the listing (d4), not by key arithmetic; a span moved and edited
  in one commit is `removed` plus `new`.

- **d19 — A supersession names the claim's latest version, and anything newer is refused by
  name.** *Added 2026-11-05 from basic-memory's `expected_checksum`
  (`mcp/tools/write.py:71-72, 142-148`).* `POST /v1/memories/{artifact_id}/supersede`, `oml_write`
  with `replaces` and `oml write --replaces` take a required `expected_version`, the ordinal of
  the claim's latest version the caller can read, as it last read it: the greatest `version`
  among the claim's promoted versions (where the caller reads the workarea) and the held and
  promoted versions its own keys wrote, the tip's own when there is none. When that differs, the
  write is refused with `submission.version_conflict` and `details.current_version`, after the
  readability check and before the tip check, and nothing is captured. Two sessions of one key
  that both read version 3 can no longer each leave a held successor for the reviewer to
  arbitrate: the first captures version 4, the second is refused and re-reads. Another author's
  held version never changes the answer, so polling the refusal tells a caller nothing it cannot
  read (R3; *amended 2026-11-05 on review of #69*); rival successors by different keys remain
  possible, as ADR-0025 d10 allows, and the compare-and-set at ratification decides between them. A
  rejected or demoted version does not count, so a rejection never blocks the next proposal.
  The web console's dreamer edit sends the version it rendered. Pre-release (ADR-0060's rule
  for OML, ADR-0039 d11): the field is required, not defaulted, so no caller supersedes blind.

- **d20 — Sync never writes a rollback.** *Added 2026-11-05 from basic-memory's lineage check
  (`indexing/note_content_reconciliation.py:93-108`).* An item whose span hash equals the anchor
  hash of a promoted version of the claim older than its tip is the signature of a stale read
  (a checkout of an old commit, a `--rev` behind `main`): the outcome is `rollback`, nothing is
  written, and the run exits 1 naming the earlier version. `--allow-rollback` writes it as
  `changed` for a deliberate revert, which the reviewer then judges. The check costs nothing
  beyond the listing the diff already reads, since the sync key authored every version it may
  supersede (d8 `foreign`) and `own=true` lists them all.

- **d21 — A fragment names a heading, a decision, or a line span; a page waits for a PDF
  hasher.** *Added 2026-11-05, chunk-level anchors.* Beside d3's rule, a fragment is one of: a
  heading's slug (d7 `headings`) or an ADR decision handle `dN` (d7 `adr`), free under d3; a line
  span `L<n>` or `L<n>-L<m>`, 1-based and inclusive, `1 <= n <= m <= 10^7`, for a document a
  chunker splits by lines because it has no headings; and `page=<n>` for a PDF page, reserved and
  refused with `request.validation` until a hasher reads PDFs (ADR-0035's attachments). A
  fragment that starts with `L` and a digit, or with `page=`, and is not well formed is refused
  naming `/anchor/fragment`. The `lines` adapter (`--adapter lines`) is v0.5.8's generic
  chunker: blocks of at most 40 lines split at blank lines, keyed `doc/<path>/L<n>-L<m>`; its
  spans move whenever a line is added above them, so it suits documents that are appended to,
  and a document with headings should use `headings`.

- **d22 — Each retriever's list is capped before fusion.** *Added 2026-11-05 from Hindsight's
  per-arm cap (`hindsight_api/engine/search/fusion.py:8-26`); recorded as
  [ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md)
  d13.* The core keeps each list's first `depth` candidates before `fuse` reads it, so no
  retriever contributes more than any other to the fused pool or the reranker's head, whatever
  its adapter returns. The anchor filters (d4) sit inside the candidate scope, before this cap,
  so a filtered recall still never under-returns.

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
  description: The origin the author declares (ADR-0057 d1-d3). Signed, immutable per version,
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
    fragment: { type: string, minLength: 1, maxLength: 256, pattern: '^[^\s#\x00-\x1f]+$',
                description: 'A slug, dN, L<n> or L<n>-L<m>; page=<n> reserved (d21).' }
    hash:     { type: string, pattern: '^sha256:[0-9a-f]{64}$' }
    hasher:   { type: string, enum: [oml-md/1] }
Stale:
  type: [string, 'null']
  enum: [changed, removed, null]
  description: The served version's latest source check (ADR-0057 d5); never inferred from a held version.
```

**Requests.** `SubmitRequest` and `SupersedeRequest` gain `anchor: Anchor`; `SupersedeRequest`
gains a required `expected_version` (integer, at least 1; d19). `RecallRequest` and
`GET /v1/memories` gain `anchor_repo` (string, the `repo` grammar) and `anchor_path_prefix`
(string, at most 1,024 bytes). **Responses.** `MemorySummary`, and so `Memory`, every recall hit,
the `review next` serve and the `GET /v2/review/held` item, gain `anchor: Anchor | null` and
`stale: Stale` and `source_checked: {rev, at, result, moved_to: {path, fragment} | null} | null`. **A route.** `POST
/v1/memories/{artifact_id}/source-check`, body `{rev, result: fresh | equivalent | moved | changed |
removed, hash?, path?, fragment?}` (`path` required for `moved` and refused otherwise), `Idempotency-Key` required, answering the version's new `stale`; refused with
`access.author_only` for a writer that did not author the version, `access.not_found` for a
version the key cannot read, and `request.validation` for a held, unanchored or superseded one.
It journals `source.checked {artifact_id, rev, result, hash}`. **Journal.** `submission.remember` and `submission.learn` carry `anchor` (the
object or `null`). **Storage.** One migration adds nullable `anchor_repo`, `anchor_uri`,
`anchor_rev`, `anchor_path`, `anchor_fragment`, `anchor_hash` and `anchor_hasher` to `memories`
and an index on `(tenant_id, anchor_repo, anchor_path text_pattern_ops)`; its number is the next
free one when the lane starts. **MCP.** `oml_write` gains `anchor` (the schema above);
`oml_read` gains `anchor_repo` and `anchor_path_prefix` in recall and listing modes, refused in
get mode.

**Refusals.** A malformed anchor or filter: `request.validation`, `details.field` such as
`/anchor/path`. A scanned finding in it: `submission.scan_veto`. A supersession whose
`expected_version` is not the claim's latest version the caller can read: `submission.version_conflict` (409),
`details.current_version`, the one new code (d19).

**CLI.**

```text
oml sync <source> --workarea <workarea>
         [--adapter adr|headings|lines] [--rev <commit>] [--repo <repo>]
         [--dry-run | --check [--record]] [--json] [--max-writes <n>] [--adopt] [--retire-all]
         [--edges [--ratify]] [--allow-public-server] [--allow-rollback]
         [--exclude <glob> …] [--change-exclude] [--change-adapter] [--pr <n> [--base <branch>]]
oml anchor <path>[#<fragment>] [--rev <commit>] [--repo <repo>] [--adapter …]   # prints one Anchor as JSON
oml write <text> --anchor <json | @file> …                           # beside today's flags
oml write <text> --replaces <id> --expected-version <n> …            # d19
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
edges     133 amends, 0 supersedes stated; proposed by --edges once both ends are promoted
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
              "moved": 0, "rollback": 0, "diverged": 0, "foreign": 0, "conflict": 0, "writes_this_run": 200}
}
```

`--check` prints one line per claim and edge in the same form, with states `fresh`, `equivalent`, `stale`,
`removed`, `pending`, `untracked`, `unhashed`, `stated`, `waiting` and `unstated`, and exits 1 on
`stale`, `removed` or `untracked`.

## Consequences

- **Costs.** One migration and seven nullable columns on `memories`, with one index; the anchor's
  bytes in every anchored submission event; one route and one event, `source.checked`, a check
  per anchored tip per sync; one lookup per served hit for its latest check; the filters' cost inside `bm25-postings`, unmeasured (FR-115). Review:
  523 items to load OML's ADRs, and at least two per edited record afterwards (the decision and
  the summary); every renamed file or moved heading costs a review per span; every superseded
  endpoint costs a proposal and a ratification per edge. Paths are kept for good (d2).
- **Roadmap rows.** FR-114 (the stale mark's gap between a commit and the next sync), FR-115 (the
  filters' latency and the recall eval's filtered queries), FR-116 (the review cost of a sync,
  and its cap of 200), FR-117 (an edge whose statement leaves the documents cannot be withdrawn).
  FR-79 continues for ADR-0047's caps. RS-17 stays open; this slice avoids it (d8 `pending`).
- **Pages to change at the build.** The HTTP API and openapi.yaml; mcp-tools.json and the MCP
  plugin page; the CLI reference (`oml sync`, `oml anchor`, the new flags); the event catalog
  (`anchor` on the two submission events); the Postgres schema; retrieval (the filters and
  `stale`); admission (the scan over the anchor); the erasure inventory (the anchor as clear
  metadata, beside terms); the plugin's skill (when to send an anchor); and at acceptance the
  Status lines of ADR-0039, ADR-0047, ADR-0048 and ADR-0055.
- **Concurrent branches.** ADR-0056 (claim formats, #63) holds FR-109 to FR-113, so this record's
  rows are FR-114 to FR-117.

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
| **A server-computed `stale` from the repository** | It is the crawler above. A source check is recorded by the client that holds the checkout. |
| **`stale` inferred from a held successor** | It tells every reader of a served version that a held row exists, which R3 does not grant; a check recorded from the source says the same thing without it. |
| **An equivalent change refreshes the anchor's hash** (ADR-0056 question 5) | The anchor is signed and immutable per version; an unreviewed refresh is a silent re-anchor. A check `equivalent` keeps the hit unmarked at no review. |
| **A moved span superseded with its new anchor** (this record's first reading of question 1) | Every rename sends every claim of the file back through review although nothing it states changed; the anchor names its commit and stays true there, and a source check records where the span went. |
| **`expected_version` optional, defaulting to the target's** | A caller that omits it supersedes blind, which is the lost update the field exists to refuse; pre-release, the contract can require it. |
| **The rollback guard on the server** | The server never reads the source (d2), so it cannot tell a stale checkout from a deliberate revert; the sync holds the commit graph and the listing. |
| **A fourth MCP tool for sync or origin reads** | ADR-0039 d8: an argument on `oml_write` and two on `oml_read` carry it, and sync is a CLI run over a checkout. |
