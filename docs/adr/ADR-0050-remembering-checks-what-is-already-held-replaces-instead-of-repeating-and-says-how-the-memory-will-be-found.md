<!--
id: adr/ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found
tier: engineering
status: proposed
answers: what a write does about a memory OML already holds, and whether it refuses, warns or links; how a write to an existing subject replaces it instead of adding a second claim; how an author revises or withdraws its own held version (RS-17); how "the latest" of a changed fact is answered; what a writer may add so the memory is found (a title, keywords, validity dates, sources) and how the server tells the writer which terms it indexed; which quality problems the scan warns about without vetoing; how a memory says when it should be checked again without ever being deleted; how a session's findings are written as one bounded batch of held candidates; what the reviewer sees beside a held row; the oml_write field set, refusals and response in their cleanest form, and the skill's text; how each decision keeps R1 to R4; the doctor lines; the remember eval that gates each decision; the build order; and the questions left for the maintainer
-->

# ADR-0050 — Remembering checks what is already held, replaces instead of repeating, and says how the memory will be found

**Status:** proposed · 2026-11-04 · revised 2026-11-05 (r2): d1's check is a same-text and
near-copy match read from integer keys, with no recall and no model, specified in d17; its nine
questions answered at their recommendations ([the answers](#the-maintainers-answers-of-2026-11-05))

**Release:** W3's write side (d1, d17) built on `feat/remember-dedup` and proposed for v0.5.9;
W1, W2 and W3's reviewer side not assigned; W4–W7 not assigned, each gated on the remember eval
([releases](../product/releases.md#decisions))

**Amends:** ADR-0012 (d5: what a serve carries beside the item, and the reject verdict), ADR-0025 (d10: a held
version may be superseded by its author; d11: a retirement of one's own never-promoted version
withdraws it; d12: the claim item; d18: a withdrawn first version releases its key), ADR-0038
(d2: the skill's write guidance), ADR-0039 (d3: the `oml_write` field set and modes; d6: an
explicit `claim_key` naming a live claim supersedes it); [admission](../architecture/admission.md)'s
scan, which gains advisory findings; answers [RS-17](../product/roadmap.md)

The design mockups for this record are on one page:
[remember mockups](../design/remember/mockups.html) (an `oml_write` exchange in Claude Code with
a duplicate hint and a supersession, the response's indexed terms, the reviewer's view with
near-duplicates beside a held row, the user page's memories with stale and review-by markers,
and the remember eval's chart).

This record takes the number after ADR-0049, which the search lane (`feat/search-port`) holds;
ADR-0042 to ADR-0048 were taken when it was written. Its roadmap rows start at FR-90, leaving
FR-86 to FR-89 to that lane.

## Context

**Recall can only serve what was written well.** OML's recall eval
([REPORT](https://github.com/e8kor/open-memory-lab/blob/feat/recall-eval/eval/recall/REPORT.md),
data version 1, 2026-11-04) measured served recall at success@5 0.17, and the proposed
[ADR-0048](https://github.com/e8kor/open-memory-lab/blob/feat/code-map/docs/adr/ADR-0048-search-improves-in-stages-any-word-first-meaning-last-each-gated-on-the-recall-eval.md)
orders the search fixes. One failure no search method fixed: q100 "latest deploy window" ranks
the replaced window (m007, "until 2026-05-31") above the one in force (m008) under every method,
because the two are separate claims. The report's sixth recommendation is that writers
supersede a fact that changed, so the old tip leaves recall. That is the write path's job, and
today nothing in it helps:

- **Nothing is checked at write.** A submit runs the deterministic scan and captures
  ([admission](../architecture/admission.md#the-deterministic-scan)). The derived `claim_key`
  hashes the authoring key with the text (ADR-0039 d6), so the same key writing the same bytes
  twice answers `submission.claim_key_conflict`; anything else, a second key, one changed word,
  the same fact in other words, a changed value, is a new claim. The skill says "plan around that
  delay rather than writing the same fact twice", and gives the agent no means to know.
- **A subject has no stable name an agent may use.** The skill tells the agent to leave
  `claim_key` out, so every changed fact the agent does not supersede by `artifact_id` becomes a
  second claim. A sent `claim_key` naming a live claim is refused, so an agent that knows the
  subject must look up the tip and send `replaces`: two calls.
- **An author cannot revise its own held write** ([RS-17](../product/roadmap.md)), which
  [ADR-0025](ADR-0025-the-agent-only-memory-surface.md)'s rejected alternatives left open for
  want of "a held→held chain and a stale-promotion rule". An agent that corrects itself before
  review adds a second held claim for the reviewer.
- **The writer cannot see how the memory will be found.** Recall matches the words of the text
  and nothing else; no field carries a summary or the words a reader would use, and an
  identifier such as `useInvoiceFilters.ts` is one opaque token
  ([retrieval](../architecture/retrieval.md#text-handling)).
- **Nothing says when a fact should be checked again.** A version holds until superseded or
  retired.

**The remember eval measures the write side.** For this record a write-side harness was added,
`eval/remember/` ([README](https://github.com/e8kor/open-memory-lab/blob/feat/remember/eval/remember/README.md)):
40 seed memories in the recall eval's world, 24 writes (8 restating a seed, 8 changing a seed's
value, 8 new), 24 paraphrased queries for the writes admitted, 8 "latest" queries and 8 planted
low-quality texts, run offline by `run.py` (standard library). Its first run:

| measured offline, version 1 | today | with this record |
|---|---:|---:|
| writes that leave two live claims on one subject | 0.67 | 0.00 with a hint sharing ≥ 0.4 of the new text's terms (precision 0.94, no false alarm on the 8 new facts) |
| a fresh write found in the top 5 by a paraphrase | 0.00 (every word must match) | 0.67 text alone, any word; 0.88 with the author's title and keywords |
| "latest" queries ranking the replaced value first | 0.62 (two live claims) | 0.00 (one claim, its tip served) |
| planted problems caught by the advisory warnings | none checked | 8 of 8, no false warning on 64 well-formed texts |

One model wrote the seeds, the writes, the keywords, the queries, the labels and the warning
rules, so every number on the right is an upper bound and an order, not a size; the README says
so and states version 2 as the gate (d16). Two results hold whatever the size: today's write
path catches only a byte-identical repeat (1 pair of 16), and with two live claims on one subject
the replaced value ranks first for 5 of 8 "latest" queries, while under supersession it never
can.

**What others do, labelled.** mem0's 2025 paper had a model choose ADD, UPDATE, DELETE or NOOP
for each extracted fact against similar memories ([mem0 paper](https://arxiv.org/html/2504.19413),
VENDOR); its 2026 extraction is "a single LLM call that only adds", and "the new fact lives
alongside the old one, and both survive" ([mem0 blog](https://mem0.ai/blog/mem0-the-token-efficient-memory-algorithm),
VENDOR), leaving "latest" to ranking. Zep stores bi-temporal edges whose contradiction
invalidates the older one ([Zep paper](https://arxiv.org/html/2501.13956), VENDOR). GitHub
Copilot Memory stores repository facts "with citations pointing to the code that supports the
fact", checks them "against the current branch" when used, and expires them: "The default expiry
period is 90 days", and "Simply retrieving a memory does not reset this period"
([Copilot Memory](https://docs.github.com/en/copilot/concepts/agents/copilot-memory)). On
LongMemEval, appending short extracted facts to each indexed key raised BM25's recall@5 from
0.634 to 0.683 ([LongMemEval](https://arxiv.org/html/2410.10813v2), independent), the evidence
for write-time keys. Memory is an attack surface: "a single adversarial memory write can exert
long-term influence over agent behavior" ([arXiv 2606.04329](https://arxiv.org/abs/2606.04329)),
and a 2026 Claude Code compromise overwrote "every project's memory"
([Cisco](https://blogs.cisco.com/ai/identifying-and-remediating-a-persistent-memory-compromise-in-claude-code)).
OML's answer to the last is review: no write is read by another key until a second identity
ratifies it ([ADR-0001](ADR-0001-the-constitution.md) d2, d3), so a write-side feature must
never let a write replace or hide a reviewed fact without that review. All sources fetched
2026-11-04.

**The rules that must hold.** R1: the agent's surface never promotes, signs, demotes or deletes.
R2: the reviewer is never the author or in the lineage closure. R3: nothing is readable by
others until ratified, and review never reorders results. R4: no silent deletion; erasure is a
journalled crypto-shred ([ADR-0001](ADR-0001-the-constitution.md) d1–d4). Decided, not built.

## Decision

- **d1 — Every write is checked for the same text and for a near copy among what the writer may
  already read, by integer keys and no model, and the answer names them; it holds by default and
  refuses only when the writer asks.** Before capture, a submit, a supersession and each batch
  item (d8) are checked against the rows the writing key may read: the served promoted tips of
  the target workarea's recall scope, as `oml_read` recall at that `workarea` would read them
  (ADR-0044 d10 includes `universe`'s claims), and the key's own held rows in that scope, the
  rows `include_unreviewed: true` adds. Another key's held row is never checked or named. Kinds
  `memory`, `derived_fact` and `steering`; a supersession leaves out its own claim's versions.
  Two relations, each found without a recall and without a model (d17):

    - **`same_text`**: the new text's normalised words (d17), in order, equal the row's: the
      same words in the same order, case, punctuation and spacing aside.
    - **`near_copy`**: not the same text, and the Jaccard similarity of the two texts' word sets
      is at least **0.9**, served as `jaccard` to two decimals. Candidates come from MinHash
      bands (d17); each is verified by the exact ratio before it is listed, so a hash collision
      never names a row. A new text with **fewer than 10 distinct words, or whose characters
      carry less than 3.0 bits of entropy**, is checked for `same_text` alone: below 10 distinct
      words a ratio of 0.9 needs the same word set, which for two different texts means the same
      words reordered ("port 8080, not 8081" against "port 8081, not 8080"), and a low-entropy
      text is a repetition. A short text is never a near copy.

    At most three rows, `same_text` first, then by `jaccard`, then by artifact id, each a
    [claim item](../api/mcp-plugin.md#the-claim-item) without `text`, with `relation`,
    `jaccard` (1.0 for `same_text`), `version` and `own` (true when the writing key wrote it),
    so the hint costs the writer a few dozen tokens and a `get` reads whichever it wants.

    The server does not decide what a listed row means. A near copy may be a restatement, a
    corrected typo, a changed value or a deliberate second claim, and a lexical rule cannot tell
    which. So `on_similar` chooses: `"hold"`, the default, captures the write held and lists
    `similar`; `"refuse"` answers `409 submission.similar_exists` with `details.similar` when the
    list is not empty, and journals nothing. A refusal is decided before the idempotency record
    is claimed, as a supersession's step-1 refusal is
    ([http-api](../api/http-api.md#idempotency)), so the agent may resend the same
    `idempotency_key` with `on_similar: "hold"`; a write whose record is committed replays its
    stored response, `similar` as first answered. The third outcome is the writer's:
    **replace**, by writing with `replaces` set to the listed row and `expected_version` set to
    its `version` ([ADR-0057](https://github.com/e8kor/open-memory-lab/pull/69) d19), which a
    supersession refuses with `submission.version_conflict` when the claim has moved since, and
    which revises the writer's own held row under d3. The skill sends `"refuse"` on new claims
    once d16's gate passes (d12). A same-text repeat by the same key keeps answering
    `submission.claim_key_conflict`, now with `details.existing`, the artifact id of its own row.

    Nothing is dropped, merged or linked by the server: R4's "nothing silently deleted" read for
    a write is "nothing silently not written". A writer who keeps a write beside a listed row on
    purpose says how they relate through `links` (d9). The check is a read: when the read log is
    built it logs the rows named under the request digest, as a recall does (ADR-0025 d9). It
    adds no rank term and changes no row's visibility (R3).

- **d2 — A write that names a live claim's `claim_key` replaces its tip: an upsert, by the same
  rule as a supersession.** `claim_key` sent without `replaces` names a subject. When no held,
  promoted or demoted version of it lives in the target workarea, the write is version 1, as
  today. When one does and the key may supersede its tip, by the supersede rule of
  [roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) (the
  tip's author, or a dreamer's proposal), the write is a supersession of the served tip, or a
  revision of the key's own held head (d3), answered `mode: "supersede"`, `upserted: true` and
  `replaces`. When the key may not, it answers `submission.claim_key_conflict` as today, with the
  residual FR-31 already names. A self-signed upsert of a reviewed tip waits for a non-author
  verdict (ADR-0025 d17). This amends ADR-0039 d6, which refused it; the derived `auto-` key and
  its refusal stay.

    A subject key has a grammar: NFC, lower-cased by the server, then 1 to 128 characters of
    `a-z`, `0-9` and `. _ : / -`, starting with a letter or digit, and not starting `auto-`;
    anything else is `request.validation`, `details.field` `/claim_key`. The skill asks for one on
    a fact expected to change (a version, a schedule, a limit, an owner), named for the subject,
    not the value: `deploy-window`, `postgres/version`, `api/max-page-size`. The cost is the one
    an upsert always carries: a key reused by mistake proposes a replacement of an unrelated
    claim. The answer's `upserted` and `replaces` show it at once, the reviewer sees the
    replacement beside the version it would replace (ADR-0012 d5), and nothing changes until a
    second identity approves.

- **d3 — The author may revise or withdraw its own held version, which then never promotes
  (answers RS-17).** A supersession, or an upsert, whose target is a held version the calling
  key wrote and that no verdict has decided is a **revision**: the held version moves to state
  `withdrawn`, leaves the review queue and never promotes; a serve open on it answers
  `review.serve_invalid` at its verdict; the event carries `revises` with its id. The revision
  takes the withdrawn version's target as its own: the tip the withdrawn version would have
  replaced, compared and set at ratification as ADR-0025 d10 states, or none for a first
  version. So a chain of drafts reaches review as one held row against one tip, and only the
  last can promote. A retirement (`retirement: true`) of one's own never-promoted held version
  withdraws it at once, with no review, since no other key has read it outside an ADR-0030
  in-progress workarea, where its readers have and the retirement is reviewed like any other.
  A withdrawn first version with no other live version releases its `claim_key`, as a
  never-promoted rejection does (ADR-0025 d18). Withdrawn rows keep their bytes, stay readable
  by their author with `oml_read`, and are erased only by the journalled erasure of R4. This is
  the reading ADR-0047 d15 c takes for `code` entries, made general; that record calls the
  revised state `superseded`, and question 1 asks for one name.

- **d4 — "The latest" is the tip of one claim: recall serves the tip, and a version's history is
  read on request.** Recall already serves only served tips (ADR-0025 d8). With d1 to d3 a
  changed fact becomes a new version of one claim instead of a second claim, which is the only
  thing that fixes the recall eval's q100: on remember eval version 1 the replaced value ranked
  first for 5 of 8 "latest" queries with two live claims and for none under supersession. No
  ranking term reads `created_at` or `promoted_at`, which would let review's timing reorder
  results (R3; ADR-0048 d5). `oml_read` get gains `versions: true`: every version of the
  claim the key may read, newest first, each with `state`, `tip`, `replaces`, `revises` and
  `review_by` (d7). The listing's `claim_key` filter already lists one claim's tips.

- **d5 — A writer may add a title, keywords, validity dates and sources, all reviewed with the
  text, and the answer says which terms it indexed.**
    - **`title`**: one line, at most 100 characters, NFC, no line break; indexed at the highest
      weight beside the text, served in the claim item, so a recall hit or a listing row can be
      read at a glance. Optional: the skill always sends it, the scan warns when a text over 280
      bytes has none (d6), and question 4 asks whether it becomes required once d16's gate shows
      what it earns (on version 1, `title` alone moved success@1 from 0.50 to 0.58 and
      success@5 not at all).
    - **`keywords`**: ADR-0048 d6 exactly, taken here: up to 8 words or short phrases, each at
      most 40 characters, NFC, trimmed, lower-cased, deduplicated, indexed below the text's
      weight, part of the signed content and reviewed with it. A reviewer who wants others
      supersedes; no verdict adds a keyword, so review never changes a rank (R3). On version 1
      title and keywords together moved paraphrase success@5 from 0.67 to 0.88, written by the
      same model as the queries (an upper bound; FR-90).
    - **`valid_from`, `valid_until`**: dates the author states the fact holds between, reviewed
      with the text. A hit past its `valid_until` is served with `expired: true`, never hidden
      and never re-ranked; ADR-0048 S4's filters gain `valid_at` (a date, or `"now"`), which
      leaves out rows whose stated validity excludes it. Reading what OML knew at a past moment
      stays with the parked ADR-0036; these are the author's statement, not the store's time.
    - **`sources`**: up to 4 strings, each at most 300 characters: a URL, a repository path, a
      commit, an issue, `session:<id>`. Scanned like the text, reviewed with it, never fetched
      by the server (`openWorldHint` stays false). Copilot's citations are checked against code
      by Copilot; OML's are checked by the reviewer, and for code by ADR-0047's anchor.
    - **`indexed`** in every write answer: the terms the row is indexed under, by field, as the
      served configuration produced them from the writer's own text (`text`, `title`,
      `keywords`, at most 64 in all), and `identifiers`, each joined token with the parts it was
      split into once ADR-0048 S2 serves its normaliser. An agent sees that `PostgreSQL` is
      indexed as `postgresql` and not `postgres`, and adds the keyword. It discloses nothing
      beyond the writer's own text. Kind stays the one structural class; free tags are not
      added (rejected alternatives), since `claim_key` prefixes (ADR-0048 S4's
      `claim_key_prefix`) and keywords already filter and find.

- **d6 — The scan gains advisory findings, which warn the writer and the reviewer and never
  veto.** Deterministic rules in the versioned rule set, beside the secret rules, each a
  `quality.*` finding class: `quality.too_long` (text over 600 bytes), `quality.many_claims`
  (three or more list lines, or four or more sentences), `quality.vague` (a text that opens with
  a pronoun and uses a placeholder word such as "somehow" or "later"), `quality.relative_time`
  ("today", "yesterday", "last week", "currently", "now" with no ISO date), `quality.no_title`
  (d5) and `quality.no_source` (no `sources`, no `derives_from`, and no URL, path, commit or
  date in the text). They are answered in `warnings: [{finding, field, message}]`, recorded with
  the scan verdict under its `scanner_version`, and served to the reviewer beside the row (d11).
  A workarea's review policy may name a class in `human_required_classes` as it may any scan
  class ([admission](../architecture/admission.md#review-policy-objects)). No warning changes
  visibility or rank. The thresholds are chosen, not measured beyond version 1, where the rules
  were written by the model that wrote the data (FR-92). One checkable claim per memory is the
  skill's rule (d12), and `quality.many_claims` is its check.

- **d7 — A memory may say when it should be checked again; past that date it is served marked
  stale, never hidden and never deleted.** `review_by`, a date at most two years after the write,
  is stated by the author or, when omitted, set from the workarea's `freshness.review_days`
  setting, which an admin sets per workarea and which is unset by default. After it, the tip is
  served with `stale: true` and its `review_by` in recall, get and the listing; recall neither
  drops nor demotes it (R3), and nothing deletes it (R4). ADR-0048 S4's filters gain `fresh:
  true`, which leaves stale rows out. The fact is re-confirmed by `oml_write` with `replaces`
  and `confirm: true` and no `text`: a held successor carrying the target's text unchanged and a
  new `review_by`, reviewed like any supersession, the reviewer told the text is unchanged
  (d11). Reading a memory never moves its date, as Copilot also decided. An anchored `code`
  entry's staleness stays ADR-0047 d11's client-side workflow, and its "confirm" is this one.
  Copilot's 90-day expiry deletes; OML's date only marks, so a stale fact stays recallable,
  attributed and reviewed until someone supersedes or retires it.

- **d8 — A batch of up to 20 writes is one request, and a session's findings are written as
  held candidates by the agent, never by a hook.** `oml_write` with `items`, an array of 1 to
  `write.max_batch` (20, chosen; FR-94) argument sets, each a complete single write with its own
  `idempotency_key`, answers `mode: "batch"` and `items`: per item, in order, its write answer
  or its error envelope. Items are independent: no atomicity, each item's idempotency record its
  own, so a retried batch replays the items already written and writes the rest. Each item's
  `similar` also lists earlier items of the same batch by `batch_index`. On HTTP it is
  `POST /v1/memories/batch`, whose per-item keys stand in for the request's `Idempotency-Key`.
  Session capture is the skill's: when a task ends, or the user asks, the agent writes what it
  learned that the next session would need, at most `capture.max_per_session` items (5, shared
  with ADR-0047 d13; FR-94), as one batch with `on_similar: "refuse"`, each one claim with its
  title, keywords and `sources: ["session:<id>"]`. No hook writes and no hook prompts for capture:
  the session-start hook "performs no act" ([ADR-0029](ADR-0029-session-start-hook.md) d1) and the hooks added since keep that rule, a prompt at every stop
  fills review with noise (ADR-0047 d13), and a transcript summarised into memory unread is the
  poisoning path the Context names.

- **d9 — A writer may propose links from the new claim, ratified with it.** `links: [{to,
  label}]`, at most 4, proposes claim edges from the written version to readable versions under
  ADR-0046 d4–d6's labels (`supports`, `contradicts`, `relates_to`, `continues`), each held with
  the claim and ratified by the verdict that promotes it, whose reviewer is never the proposer
  (R2), or dropped with its rejection. A `contradicts` link is how a writer keeps a conflicting
  claim on purpose. This is the `links` field ADR-0046 d4 left to a later record; it is designed
  here and built after ADR-0046's claim edges are served (W7), and refused as a designed field
  until then.

- **d10 — `oml_write` keeps one tool and one flat argument set; the mode is chosen by the
  fields present, and every answer says what was stored, what is similar, what was indexed and
  what review it waits for.** No new MCP tool ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)
  d8). The fields:

    | Field | Type | Modes | New here |
    |---|---|---|---|
    | `text` | string, required except with `confirm` | submit, supersede | |
    | `idempotency_key` | string, required | all | |
    | `title` | ≤ 100 chars | submit, supersede | d5 |
    | `keywords` | ≤ 8 × ≤ 40 chars | submit, supersede | d5, ADR-0048 d6 |
    | `kind` | `memory` default | submit, supersede | |
    | `claim_key` | subject key, d2 grammar | submit (an upsert when live) | upsert, grammar |
    | `replaces` | artifact id | supersede, retire, confirm | held targets (d3) |
    | `retirement` | bool | retire | own held withdraws (d3) |
    | `confirm` | bool, no `text` | confirm | d7 |
    | `reason` | string | supersede, retire | |
    | `derives_from` | artifact ids | submit, supersede | |
    | `sources` | ≤ 4 strings | submit, supersede | d5 |
    | `valid_from`, `valid_until` | dates | submit, supersede | d5 |
    | `review_by` | date | submit, supersede, confirm | d7 |
    | `on_similar` | `hold` default, `refuse` | submit, supersede | d1 |
    | `links` | ≤ 4 `{to, label}` | submit, supersede | d9, designed |
    | `workarea` | id or name | all | |
    | `items` | ≤ 20 argument sets | batch | d8 |

    `items` excludes every other field at its level. An argument outside the chosen mode is
    `request.validation` naming it, as today. The answer:

    ```json
    {
      "mode": "supersede",
      "artifact_id": "0b7e5a1c-6d2f-4e83-9a41-c5f8d3b2e907",
      "claim_key": "deploy-window",
      "version": 3,
      "state": "held",
      "upserted": true,
      "replaces": {"artifact_id": "7d1e…", "version": 2},
      "revises": null,
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
      "similar": [{"artifact_id": "7d1e…", "claim_key": "deploy-window", "version": 2, "state": "promoted", "relation": "near_copy", "jaccard": 0.92, "own": true}],
      "warnings": [],
      "indexed": {"title": ["production", "deploy", "window"], "text": ["2026-06-01", "production", "deploy", "monday", "wednesday", "thursday", "09:00", "11:00", "cet", "window", "on-call", "lead", "approval"], "keywords": ["release", "schedule", "change", "window"], "identifiers": {}},
      "review": {"waits_for": "1 verdict by another identity", "human_required": false},
      "review_by": "2027-02-01",
      "idempotent_replay": false
    }
    ```

    `review` is read from the workarea's policy object (its `k` and `human_required_classes`
    against this row), never a queue position, which would count other keys' rows. The new
    refusal is `409 submission.similar_exists` (not retryable; `details.similar`); the others are
    today's, with `details.existing` added to a same-key `submission.claim_key_conflict`. On HTTP
    the submit and supersede bodies take the same fields, `body` keeping its name there
    (ADR-0039 d7).

- **d11 — The reviewer sees the near-duplicates, the warnings and what the author was shown,
  beside the held row, and may reject as a duplicate.** `POST /v2/review/next` serves, beside
  the item and the version it replaces (ADR-0012 d5): `similar`, recomputed at serve time by d1's
  rule over what the reviewer may read, which is the promoted tips its reader access reaches and
  the held rows its dreamer access could be served, never a row it could not otherwise see;
  `author_saw`, the ids of the author's write-time `similar` that the reviewer may read; the
  write's `warnings`; and for a confirmation, that the text is unchanged. None of it enters the
  serve's bound digest, so a new similar row does not invalidate a serve. The reject verdict
  gains `duplicate_of`, an artifact id the reviewer may read, journalled with the verdict and
  served on the author's own listing beside `rejected`, so the author's agent learns which
  memory it repeated, and real duplicate rates can be counted from verdicts (FR-93). Approval is
  unchanged: one verdict, by another identity.

- **d12 — The skill teaches one checkable claim per write, a subject key for what changes, the
  similar check, and capture at the end.** The `oml-memory` skill's "Remember one claim at a
  time" and "Revise or retire a memory" sections become, at W3:

    > **Before you write, let OML check.** Send `on_similar: "refuse"` with a new claim. If it is
    > refused with `submission.similar_exists`, read the rows in `details.similar`: if one says
    > the same, write nothing; if one says an older value of the same thing, write with
    > `replaces` set to it and `expected_version` set to its `version`; if they differ, write again with `on_similar: "hold"`, and with
    > `links` if one contradicts yours.
    >
    > **One checkable claim per write.** Write `text` so a reader with no context can check it:
    > what is true, since when, and where you saw it. Put the source in `sources` and the date
    > in the text. Give a `title` of one line and up to 8 `keywords`: the other words someone
    > would search for, such as an abbreviation, the old name or the error code.
    >
    > **Name what will change.** For a fact that will change, such as a version, a limit, a
    > schedule or an owner, pass a `claim_key` naming the subject, such as `deploy-window`. A
    > later write with the same `claim_key` replaces it for review. Otherwise leave `claim_key`
    > out. Set `review_by` when you know when it should be checked again.
    >
    > **Correct yourself.** To change your own write before it is reviewed, write again with
    > `replaces` set to it; only the last version is reviewed. To withdraw it, add
    > `retirement: true`.
    >
    > **Read the answer.** `indexed` lists the words the memory will be found by; add a keyword
    > for any word you expected and do not see. `warnings` are what the reviewer will see too.
    >
    > **Capture at the end.** When a task ends, or the user asks, write at most 5 things you
    > learned that the next session would need, as one `items` batch with
    > `on_similar: "refuse"`. Never write what the code or a repository file already says, or
    > the task's status.

    The skill's description keeps ADR-0038 d2's opening on when to recall; the recall guidance
    and the session-start line of ADR-0038 d3 are ADR-0048's to change.

- **d13 — The constitution holds at every decision, and this is how.**

    | | R1, the verb ceiling | R2, never the author | R3, visibility not ranking | R4, no silent deletion |
    |---|---|---|---|---|
    | d1 similar | a read and a refusal; no verb added | unchanged | lists only rows the writer may read; adds no rank term | nothing removed |
    | d2 upsert | a supersession under its rule; promotes nothing | a second identity ratifies | the tip moves only at ratification | the replaced version stays |
    | d3 revise, withdraw | the author's own unreviewed row only; a reviewed tip is untouched | no verdict involved | no other key has read the row, except inside ADR-0030, where the retirement is reviewed | `withdrawn` keeps the bytes |
    | d4 latest | — | — | no time term in ranking | history readable |
    | d5 findability | — | keywords are authored and reviewed | no verdict adds a term | erasure reaches the title, keywords and their index rows |
    | d6 warnings | — | — | warnings never change visibility or rank | — |
    | d7 freshness | confirmation is a supersession, reviewed | a second identity ratifies | stale rows are marked, not hidden or re-ranked | nothing expires or is deleted |
    | d8 batch | each item is a write | each item is reviewed | each item is held | — |
    | d9 links | proposed, never ratified by the writer | the claim's non-author reviewer ratifies | an edge confers no access (ADR-0046 d7) | — |
    | d11 review | the reviewer's served ceremony | `duplicate_of` changes nothing about who may judge | the reviewer sees only what it may read | a rejection keeps the row |

    Erasure: a memory's `title`, `keywords` and `sources` are body content, carried inside the
    body's envelope and shredded with the text, never attribution metadata, which
    [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d5 leaves in place; its validity and
    review dates are metadata, as `created_at` is. The `indexed` terms are rows of the full-text
    index, erased with the text's own (ADR-0005 d4's inventory gains the title and keyword
    vectors).

- **d14 — The build, in increments, each closed by named tests and its gate.**
    1. **W1, the claim (no search dependency):** d2, d3, d4, `duplicate_of` (d11).
       `an_upsert_by_claim_key_supersedes_the_tip_for_review`,
       `an_upsert_naming_another_authors_claim_is_a_claim_key_conflict`,
       `a_claim_key_outside_the_grammar_is_refused_and_one_inside_is_lower_cased`,
       `an_author_revises_its_held_version_and_only_the_last_reaches_review`,
       `a_withdrawn_version_never_promotes_and_its_open_serve_is_invalid`,
       `retiring_ones_own_unreviewed_version_withdraws_it_without_review`,
       `a_withdrawn_first_version_releases_its_claim_key`,
       `get_with_versions_lists_every_readable_version_newest_first`,
       `a_reject_as_duplicate_is_served_on_the_authors_listing`. Gate: the remember eval run
       through a live lead serves "stale first" 0 on its "latest" queries.
    2. **W2, findability (after ADR-0048 S1):** d5. `title_and_keywords_are_signed_reviewed_and_indexed`,
       `the_write_answer_lists_the_indexed_terms_by_field`,
       `a_hit_past_valid_until_is_served_expired_and_not_reranked`,
       `valid_at_now_leaves_out_rows_whose_validity_excludes_it`. Gate: d16's findability gate
       for any change to the skill's title guidance; the fields ship on their tests.
    3. **W3, the similar check (no search dependency since r2):** d1, d17, d11's `similar`,
       d12. `a_write_lists_a_same_text_row_the_writer_may_read`,
       `a_write_lists_a_near_copy_at_jaccard_0_9_and_not_below`,
       `a_short_text_is_never_listed_as_a_near_copy`,
       `a_write_never_lists_a_row_the_writer_may_not_read` (another key's held row, an
       unreadable workarea), `a_supersession_never_lists_its_own_claim`,
       `on_similar_refuse_journals_nothing_and_claims_no_idempotency_record`,
       `a_replayed_write_answers_the_similar_list_first_answered`,
       `similar_keys_computed_in_sql_equal_the_cores`,
       `review_next_serves_similar_rows_the_reviewer_may_read`. Gate: d16's near-copy gate on
       the labelled set before the field ships; the skill sends `refuse` after the hint gate on
       version 2. W3 is built in two parts: the write side (d1, d17), then the reviewer's
       `similar` (d11).
    4. **W4, warnings:** d6. `each_quality_finding_warns_and_never_vetoes`,
       `warnings_are_served_to_the_reviewer`. Gate: d16's warning gate.
    5. **W5, freshness:** d7. `a_tip_past_review_by_is_served_stale_and_not_reranked`,
       `confirm_carries_the_text_unchanged_and_needs_review`,
       `reading_a_memory_never_moves_its_review_by`,
       `fresh_true_leaves_stale_rows_out`.
    6. **W6, batch and capture:** d8. `a_batch_answers_each_item_in_order_and_writes_independently`,
       `a_retried_batch_replays_written_items_and_writes_the_rest`,
       `an_item_similar_to_an_earlier_item_names_its_batch_index`,
       `a_batch_over_write_max_batch_is_refused`. Gate: d16's batch gate.
    7. **W7, links (after ADR-0046's claim edges):** d9. `links_are_ratified_with_the_claim_and_dropped_with_its_rejection`.

- **d15 — Every new failure has a doctor line.** `oml doctor` (admin) names: the similar check's
  p95 over the last hour above `similar.max_ms`, with the write latency it adds (FR-91); versions
  that should hold similar keys and hold none, or keys whose state differs from their version's
  (d17), which `oml_similar_keys_refill` repairs; a share
  of the last day's writes refused `submission.similar_exists` and never resent, which is either
  duplicates avoided or a threshold too loose; held rows older than a day with a withdrawn
  sibling still served for review (a stale-promotion fault d3 forbids); workareas whose stale
  tips exceed `freshness.stale_warn_share`; a rule set missing the `quality.*` classes; and a
  batch route answering any item `system.*`. `oml doctor` in a clone with the plugin names a
  skill older than the server's `oml_write` schema, by comparing the fields the skill teaches
  with `tools/list`.

- **d16 — The remember eval gates each decision, and grows to real writes.** `eval/remember/`
  holds version 1 (synthetic), and since r2 the labelled near-duplicate set d17 is measured on,
  and runs offline in CI as a data check now, and against a live
  lead once W1 is built, as the recall eval does (ADR-0048 d12). The gates, on version 2:

    | Decision | Gate |
    |---|---|
    | d1, d17, the field | on the labelled near-duplicate set (`eval/remember/near-duplicates/`): same-text recall 1.00; near-copy recall ≥ 0.85; precision ≥ 0.95; no short text listed |
    | d1, the skill's `refuse` | on version 2: precision ≥ 0.80 at hint recall ≥ 0.90 on duplicate writes |
    | d2–d4 | stale first 0; latest correct@1 not below the store's success@1 on the same queries |
    | d5 | title and keywords ≥ +0.10 paraphrase success@5 over text alone; no category falls by more than one query; exact queries unchanged |
    | d6 | false warnings ≤ 0.10 on accepted memories; planted problems caught ≥ 0.80 |
    | d8 | within-batch duplicates hinted at d1's recall |

    **Measured, 2026-11-05, on the labelled set's version 1** (`eval/remember/near-duplicates/`):
    same-text recall 1.00 (240 of 240); near-copy recall 0.86 (613 of 714: a prefix or a dropped
    stopword always, a one-letter typo in 139 of 240, since a replaced word keeps 0.9 only in a
    text of 19 distinct words or more); a changed number 88 of 137; paraphrases 1 of 8; 952 rows
    named and none wrong (precision 1.00); no short text and no new fact named. The field's gate
    passes; the skill's `refuse` waits on version 2.

    Version 2 takes its writes from consented sessions of the maintainer's own crews, where held
    writes, the reviewers' `duplicate_of` verdicts and the session that wrote each give labelled
    duplicate and update pairs no model chose; its paraphrases are written by someone who did
    not write the memories or their keywords, at least 30 per category; and it adds held rows,
    several workareas and steering rows. The real-traffic duplicate rate is counted from
    verdicts, never from query text, which the request log does not keep (ADR-0042 d8).

- **d17 — The near-copy check: normalised words, MinHash over 60 permutations in 10 bands of 6,
  integer keys row security lets an index serve, and the exact ratio before anything is listed**
  (r2; adopted from Graphiti's deterministic resolution, [dedup_helpers.py](https://github.com/getzep/graphiti/blob/main/graphiti_core/utils/maintenance/dedup_helpers.py),
  read at commit `fb60e59` on 2026-11-05, Apache-2.0, re-implemented, no code copied).

    - **Words.** The NFC text with ASCII letters lower-cased, split at every ASCII character that
      is not a letter or a digit; a character outside ASCII belongs to its word, so a text in
      another script is compared by its words as written. No stems and no stopwords: the check
      looks for a copy, not for a meaning. `same_text` compares the word sequences; `near_copy`
      the sets of distinct words, shingles one word long.
    - **Keys.** A word's 64-bit id is ADR-0052 d2's `oml_token_id`; permutation *i* of 60 maps it
      to `(aᵢ·x + bᵢ) mod (2³¹ − 1)`, `x` the id reduced into that field, `aᵢ` and `bᵢ` taken
      from `oml_token_id('minhash:a:i')` and `('minhash:b:i')`; the signature is the 60 minima;
      band *j* of 10 is keyed `oml_token_id('b<j>:<its six minima>')`, and the text's
      `same_text` key is `oml_token_id('x:' || its words joined by single spaces)`. A pair at
      Jaccard 0.9 shares a band with probability 1 − (1 − 0.9⁶)¹⁰ = 0.9995, at 0.85 with 0.991,
      at 0.5 with 0.15. The verification reads at most 32 candidates, those sharing the most
      bands first, then by artifact id.
    - **Storage.** Migration 0026's `similar_keys`: one row per version and key, carrying the
      tenant, workarea, authoring key, memory id, band (0 for `same_text`, 1–10) and state (0 held,
      1 a served tip), list-partitioned by tenant as `search_postings` is, fenced, with no
      statistics on the parent; one index, `(key, workarea_id) INCLUDE (state, author_key_id,
      memory_id)`, reached only by `bigint` and `uuid` equalities, which are leakproof, so row
      security lets it serve (ADR-0052's Context). A trigger on `memories` writes a version's
      keys from `oml_similar_keys(claim)`, an immutable SQL function the core computes equally
      (a law holds the two equal), and follows the version's state: written held, served at
      promotion, deleted when the version is replaced, demoted, rejected or erased, so the table
      holds only what d1 may name. 0026 fills every existing version; `oml_similar_keys_refill`
      rebuilds one tenant's keys when the doctor names a fault (d15).
    - **The read.** One statement: the new text's keys, the scope's workareas, state 1, or state 0
      with the writing key as author; the core then reads the candidates' rows, admits each by
      recall's rule (a readable served tip, or its own held row) and computes the exact ratio.
      A key computed from another tenant's text can only meet its own tenant's partition.
    - **The labelled set** (`eval/remember/near-duplicates/`, version 1): the recall eval's 200
      memories and the remember eval's 40 seeds, with 1,115 probes derived from them by a seeded
      script: 240 normalised copies, 714 near copies (a transposed-letter typo, one stopword
      dropped, a `Note:` prefix), 137 changed values (one number moved), the remember eval's 8
      paraphrases, 8 paraphrased updates and 8 new facts, and 6 short pairs that differ by one
      value or by order. The pairs were derived by rule, not written by a model, and the
      paraphrases are those a model wrote for version 1: the set measures the check's
      mechanics, not real writes, which remain FR-90's. Its numbers are in the README beside it
      and in the build's measured row in d16.
    - **The cost**, measured 2026-11-05 by `bench/scale/similar.py` as the serving role under row
      security on a busy host ([the scale report](https://github.com/e8kor/open-memory-lab/blob/main/bench/scale/REPORT.md)):
      the lookup and the read of the candidates' rows take 0.82 / 1.25 ms p50 / p95 at 10k
      versions, 0.90 / 1.82 at 100k and 2.95 / 15.7 at 1M, against 128 / 180 for r1's recall at
      1M; an index-only scan at every scale. The keys take about twice the memories heap; the
      trigger adds 0.4 ms p50 to a held insert at 10k and 6.2 at 1M; 0026's fill takes 19 minutes
      at 1M with writes waiting, as 0021's does (FR-103). The scale lane's templated texts
      overstate the candidates.

## Downsides: argument or opportunity

| Downside | Verdict | Reasoning, and what it implies |
|---|---|---|
| Every write pays one lookup | argument | Since r2 an index read of at most 11 integer keys and a verification of at most 32 candidates, measured at 10k, 100k and 1M (d17), where the recall it replaces cost 128 ms p50 at 1M; the doctor line watches it (d15). |
| The check misses a paraphrase | argument, accepted | A near copy shares 90% of its words; "the same fact in other words" shares far fewer (1 of 8 paraphrases on the labelled set). The reviewer's `similar` is a second look (d11), and meaning-level duplicates wait for a dense leg (rejected alternatives). |
| A word set ignores order | argument, bounded | Two long texts with the same words in another order are listed as `near_copy` at 1.0, never `same_text`; under 10 distinct words they are not listed at all (d1). |
| A hint on an unrelated row costs the agent a read | argument, bounded | At most 3 rows without text; `refuse` only when the agent asks. |
| `refuse` costs a second call | argument | Against a reviewer's verdict per duplicate; the skill's default waits on d16's precision gate. |
| An upsert can replace the wrong claim | argument | `upserted` and `replaces` say so at once, and nothing changes before review (d2). |
| Subject keys disclose that a name is taken | argument, existing | The residual FR-31 already names, unchanged. |
| A withdrawn state is one more state | argument, small | The alternative is RS-17's second held claim per correction. |
| Warnings can nag | argument | Advisory, never a veto, thresholds measured (FR-92). |
| A stale flag adds review load | argument | Off unless an author or a workarea sets a date; FR-93 measures confirmations per stale row. |
| Duplicates in the review queue | opportunity | `duplicate_of` gives the first real duplicate rate (FR-93). |
| Agents do not know how they will be found | opportunity | `indexed` tells them, from their own text. |
| "Latest" fails under every search method | solved by structure | One claim per subject, its tip served (d2–d4). |
| Poisoning by write | opportunity | Upsert never reaches another author's claim, and no write replaces a reviewed fact without review. |

**What must not degrade.** R1–R4 as d13 states them. A plain write without the new fields
behaves as today except that its answer gains `similar`, `warnings`, `indexed` and `review`,
and a `claim_key` naming a live claim the key may supersede now supersedes it. Three MCP tools
stay three.

## Consequences

- **Roadmap rows.** RS-17 is answered by d3 at acceptance. FR-90 opens (the near-copy check's
  recall and precision on real writes, and the 0.9 threshold), FR-91 (the check's latency at
  stated corpus sizes, measured in part by d17's bench), FR-92 (the warning thresholds on accepted memories), FR-93 (the duplicate
  rate and the stale-confirmation load from verdicts) and FR-94 (`write.max_batch` and
  `capture.max_per_session`).
- **Pages to change at acceptance.** The MCP plugin page and `mcp-tools.json` (`oml_write`'s
  fields, modes and answer; `oml_read`'s `versions`, `valid_at`, `fresh`); the HTTP API (submit,
  supersede, `POST /v1/memories/batch`, review next and the reject verdict), openapi.yaml and
  errors.md (`submission.similar_exists`); the claim item (`title`, `withdrawn`, `revises`,
  `stale`, `review_by`, `expired`); admission (advisory findings, `withdrawn`); audit and lineage
  (revision of a held version; the released key); retrieval (`stale`, `expired`, the new
  filters); the schema and the event catalog (`revises`, `duplicate_of`, the new columns);
  the schema (0026's `similar_keys`); configuration (`similar.max_ms`, `freshness.review_days`,
  `freshness.stale_warn_share`, `write.max_batch`, `capture.max_per_session`); the `oml-memory`
  skill (d12); the user page's memories view (state, stale and review-by markers, withdrawn
  rows); and each amended record's Status line.

## The maintainer's answers of 2026-11-05

The operator answered all nine on 2026-11-05 by taking each recommendation, and narrowed d1's
check to a same-text and near-copy match with no recall (d17, r2). The last column says which the
first slice, W3's write side, exercises.

| # | Question | Options | Answer | Exercised by W3's write side |
|---|---|---|---|---|
| 1 | One name for a held version its author replaced or withdrew? | a. `withdrawn` for both; b. ADR-0047's `superseded` for a revision and `withdrawn` for a retirement | a: one state, `withdrawn`, one rule (never promotes) | no: W1 |
| 2 | The similar check's default? | a. warn (`hold`), the skill sends `refuse`; b. refuse by default; c. warn only | a: `hold` by default, the skill sends `refuse` once d16's gate passes | yes: `hold` and `refuse` |
| 3 | Does an explicit live `claim_key` upsert? | a. yes, under the supersede rule; b. keep the conflict, require `replaces` | a: an explicit live `claim_key` upserts under the supersede rule | no: W1 |
| 4 | Is `title` required? | a. optional with a warning; b. required on new claims | a: optional with a warning | no: W2 |
| 5 | A default `review_by` on the public server? | a. none; b. 90 days, as Copilot; c. per kind | a: none; workareas opt in | no: W5 |
| 6 | Does reading reset freshness? | a. never; b. on use | a: never | no: W5 |
| 7 | Batch semantics? | a. independent items; b. all or nothing | a: independent items | no: W6 |
| 8 | Release? | a. W1–W3 in v0.5.3 with ADR-0048 S1–S3; b. W1 alone first | a's grouping, W1 to W3 together; v0.5.3 shipped without them, and since r2 W3 no longer waits on S1, so its write side is built first, proposed for v0.5.9 and assigned by the maintainer | yes |
| 9 | Should `on_similar` look beyond the target workarea's recall scope, at the key's other workareas? | a. no; b. yes | a: the target workarea's recall scope only | yes |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A model decides ADD, UPDATE, DELETE or NOOP at write** (mem0, 2025) | A model in the write path would decide what replaces a reviewed fact without the reviewer, and its decision is not reproducible from a rule-set version as the scan's is ([admission](../architecture/admission.md#the-deterministic-scan)); mem0 itself moved to a call that "only adds". The agent writing is already a model and decides with the hint (d1). Revisit only as a reviewer crew's aid, never in the gate. |
| **Only add, and let ranking pick the newer** (mem0, 2026) | Ranking by time lets review's timing reorder results (R3), and on version 1 the replaced value ranked first for 5 of 8 "latest" queries with both live. |
| **Silently drop a duplicate write** | Nothing is deleted or dropped unseen (R4's spirit, ADR-0001 d4); the writer is told, and the reviewer decides. |
| **Merge or link similar rows automatically** | A link is a claim about two memories and needs a proposer and a non-author ratifier (ADR-0046); `links` lets the writer propose one (d9). |
| **Refuse every similar write** | The hint cannot tell a refinement from a repeat; refusing on a lexical rule would block correct writes, and its precision is unmeasured on real writes (FR-90). Opt-in `refuse` (d1). |
| **Embeddings for duplicate detection now** | ADR-0048 stages meaning last behind its eval; a dense leg is not served, and a cosine threshold (Hindsight adjudicates at 0.97) needs labels this repository does not have. A meaning-level hint is a later leg beside d17's, not a replacement. |
| **One recall per write, listing rows that share 0.4 of the new text's words** (r1's d1) | It cost 128 ms p50 and 180 ms p95 per write at 1M rows on the scale lane ([REPORT-baseline](https://github.com/e8kor/open-memory-lab/blob/main/bench/scale/REPORT-baseline.md), case g), ten times the write it guards at 100k, and its 0.94 precision was measured on texts the same model wrote as the labels. d17 reads integer keys instead. |
| **Graphiti's constants: character 3-grams, 32 permutations in 8 bands of 4** ([dedup_helpers.py](https://github.com/getzep/graphiti/blob/main/graphiti_core/utils/maintenance/dedup_helpers.py)) | Built for entity names of a few words; on whole memories 8 bands of 4 make a pair at Jaccard 0.2 a candidate one time in 80, about 509 candidates per write at 1M on the labelled set's distribution, against 114 for 10 bands of 6. Its entropy gate (1.5 bits) is kept as an idea and set for sentences (3.0, under the labelled set's lowest, 3.52). |
| **Word shingles of two or three words** | Order-aware, but at 0.9 they found 288 and 277 of 714 near copies on the labelled set, against 613 for one word, with no false listing in any; the reorder they would catch is what the 10-word guard and `same_text` already separate (d1). |
| **An LLM adjudicates candidates** (Graphiti's `dedupe_nodes`, mem0 2025) | A model in the write path, nondeterministic and not reproducible from a rule version; as a reviewer crew's aid it may come later, never in the gate. |
| **The server replaces, merges or links on a near copy** | A near copy can be a deliberate second claim or a changed value the writer has not checked; the writer chooses with `replaces` and `expected_version`, and the reviewer ratifies (d1, d2). |
| **Copilot's expiry** | Expiry deletes, which R4 forbids; a stale mark keeps the fact recallable and attributed (d7). |
| **Freshness reset on read** | Popularity is not truth; Copilot also rejected it. |
| **Reviewer-added keywords or titles** | A verdict would change a rank (R3); a reviewer supersedes (d5, ADR-0048 d6). |
| **Free-form tags** | A second vocabulary beside keywords and `claim_key` prefixes, with no review rule of its own; Occam's razor on the client API. |
| **A fourth MCP tool, `oml_update` or `oml_batch`** | ADR-0039 d8: parameters first; `claim_key`, `replaces` and `items` carry them. |
| **A hook that writes a session summary** | The hooks perform no act (ADR-0029 d1, kept by ADR-0047 d11); unread transcript content is the poisoning path; capture is the agent's, bounded and held (d8). |
| **An atomic batch** | One refused item would lose the rest, and the agent would have to split and retry; independent items with their own keys replay cleanly (d8). |
| **A self-signed confirmation of a reviewed tip** | ADR-0025 d17: a self-signed version never replaces a reviewed tip. |
