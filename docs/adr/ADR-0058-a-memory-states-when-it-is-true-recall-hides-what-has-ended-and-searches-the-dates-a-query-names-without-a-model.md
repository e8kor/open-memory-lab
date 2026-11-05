<!--
id: adr/ADR-0058-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model
tier: engineering
status: proposed
answers: which part of ADR-0036's valid time this slice builds and in what shape; how an author declares when a memory holds, including a vague period such as a year, and why it is signed and immutable; why a memory whose valid time has ended is hidden from recall by default and how a reader asks for it; how a fact is invalidated without the server deciding it; which date phrases recall reads from a query and how, with no model; how a date window becomes a retriever leg that row security can serve and how it is fused and capped; the optional bounded recency boost; the API on oml_write, oml_read and HTTP; the migration, doctor line, laws and measurements
-->

# ADR-0058 — A memory states when it is true, recall hides what has ended, and recall searches the dates a query names without a model

**Status:** proposed · 2026-11-05

**Release:** not scheduled; built on draft branch `feat/valid-time`

**Amends:** ADR-0036 (d1, d2, d4: narrowed and built in part), ADR-0049 (d4: two pipelines;
d6: `search.window`, `matched.via` `time`), ADR-0039 (d3, d4: arguments on `oml_write` and
`oml_read`)

## Context

No pipeline OML serves reads time. On the synthetic set the temporal category sits at 0.76
nDCG@10 against 0.86 overall
([ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md)
d9); on LoCoMo, `bm25-postings` finds the evidence for 48% of temporal questions first and 69%
in the top five (`eval/bench/results/locomo/bm25-postings.json`), by their words alone. Nothing
in a memory says when it is true, so no read can tell a fact that ended from one that holds,
and a query phrase like "last week" or "in 2015" reaches the ranking only as words.

Two systems the competitor study read (2026-11-05, outside this repository) keep time apart from
text. Hindsight stores a fact's occurrence span apart from when it was mentioned, stores a vague
date as the whole period it names, parses a query's dates by rule, adds a date-window retrieval
leg, and caps any boost applied after its reranker at ±10%. Graphiti keeps `valid_at` and
`invalid_at` beside `created_at` and `expired_at`, hides an expired edge from search by default,
and ends a superseded fact by setting its end, never by deleting it; it decides the end with a
model.

ADR-0036, proposed on draft PR #19 and cited here by number until it lands, already decides the shape of valid time (two signed, immutable ends
beside the journal's transaction time) and filters that never rank. Its as-of reads, MCP
resources and `oml fs` are a larger build that no eval asks for yet. This record builds the part
that measurements can test, narrows ADR-0036 where the slice needs a sharper rule, and leaves
the rest of ADR-0036 proposed. It holds to [ADR-0001](ADR-0001-the-constitution.md) R1–R4, in
particular R3: the gate changes visibility, not ranking, and no ranking reads a verdict; and to
[ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md): no new tool, and every argument traces to a
use. Decided here and built on the same branch; the measurements below are this branch's.

## Decision

- **d1 — Two clocks, kept apart: recorded time is the journal's, valid time is the author's.**
  *Recorded time* is when OML learned a version: its submission event's `at`, folded as the
  row's `created_at`, unchanged. *Valid time* is when the version says it holds in the world,
  declared by the author (d2). Neither is derived from the other, and the server never reads
  either out of the text (d10). ADR-0036 d1's naming holds; its `simulated_time` is untouched.

- **d2 — Valid time is two optional ends, each a period or an instant, declared, signed and
  immutable per version.**
    - **Shape.** `valid_from` and `valid_until` ride a submission (`POST /v1/memories`,
      `oml_write`) and a supersession (`POST /v1/memories/{id}/supersede`, `oml_write` with
      `replaces`). Each is `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, or an RFC 3339 `date-time` with an
      offset and at most six fractional digits. A period end names the whole period: `valid_from`
      is the period's first instant and `valid_until` the instant after its last, so
      `valid_from: "2015", valid_until: "2015"` holds for all of 2015 and `valid_until:
      "2026-11-05"` holds through that day. An instant is exact, and an instant `valid_until` is
      exclusive. The interval is [from, until); an absent end is open; both present need from <
      until. Anything else is `request.validation` with `details.field` `/valid_from` or
      `/valid_until`. This narrows ADR-0036 d2, which named the end `valid_to` and read a date end
      as that day's midnight; an author saying "until 5 November" means through that day.
    - **Kinds.** `memory` and `derived_fact` take valid time. `steering`, which applies in every
      recall of its scope, `guardrail`, the vocabulary kinds and a retirement refuse it,
      `request.validation` naming the field (ADR-0036 d2 unchanged).
    - **Signed.** The submission payload (`submission.remember`, `submission.learn`, and a
      supersession's) carries `valid_from` and `valid_until`, each the normalized string, only
      when sent, so an event that declares none hashes as it did before this record. The
      request digest gains them the same way. A period stays the period in the signed form, an
      instant is the fixed-width UTC form of `at` ([chain format](../data/chain-format.md)).
    - **Stored.** `memories` gains the two strings and their bounds in microseconds since the
      epoch (`valid_from_us`, `valid_until_us`, `int8`), which every filter and index reads (d11).
    - **Declared once, reviewed.** A version's declared ends never change; the one later fact is
      the end a ratified supersession gives it (d4), signed in that ratification. A successor
      inherits none: it states its own or is open. The served item carries both, so a reviewer judges them with
      the text, and a verdict approves both together.

- **d3 — A memory whose valid time has ended is hidden from recall by default: a visibility
  rule, never a weight.** A version is *expired* when its `valid_until_us` is at or before the
  request's now (d5). Recall leaves it out of the candidate scope in every retriever's statement,
  so an expired row never spends a place in a list, a cap or `k`, and the core's admission
  re-checks every served row as it does for state (ADR-0049 d2). `include_expired: true` on
  recall serves expired rows, ranked as any other. Get by id and the listings serve a version as
  they always did, with its ends, so an agent that reaches an expired memory by id sees why
  recall hid it. A version whose `valid_from` is after now is served: valid time schedules
  nothing (ADR-0036 d2). Steering takes no valid time, so it is never hidden. The one exception
  is `keyword-all`, whose two v0.5.0 statements are kept byte for byte so the eval can compare
  against v0.5.0 (ADR-0049 d7, d9): the core's re-check removes an expired row after its list is
  cut, so `keyword-all` may serve fewer than `k` when expired rows rank in its top `k`. Cost:
  every other scan statement gains one `int8` comparison per row, and `bm25-postings` one probe
  of an index on (`tenant_id`, `valid_until_us`) that holds only rows with an end.

- **d4 — A fact ends by a reviewed supersession, never by the server alone.** Two ways, both
  reviewed. The author supersedes the claim with a successor that declares `valid_from`: once a
  reviewer ratifies it, the ratifying `lifecycle.supersede` event carries
  `replaces_valid_until`, the successor's start as an instant, and the fold sets it as the
  replaced version's end, when that start falls after the replaced version's start and before
  its declared end; an end it already has that is earlier stays. Or the author supersedes with a
  successor that carries its own `valid_until`, or retires the claim. Nothing ends on
  submission: a held successor changes nothing, a rejected one never ends anything, and a
  successor that declares no start ends nothing, since no end is inferred, guessed from the text,
  or computed from anything but the declared start the verdict approved. The replaced version's
  own submission stays as it was signed; its later end is a fact of the ratifying event, signed
  there. Nothing is deleted (R4). Detecting that two memories contradict each other is not
  built. This composes with ADR-0057's `expected_version` on a supersession (draft PR #69): the
  version a supersession names is the one the ratification ends.

- **d5 — A recall has one now, the server's clock unless the request names it.** `now`, a date
  or an instant as d2 parses it (a period reads as its first instant), anchors three things: the
  expiry cut (d3), the relative dates the parser reads (d6), and the recency age (d8). Absent,
  it is the lead's clock at the request. It widens nothing a key could not already read: an
  earlier now shows rows that ended since, which `include_expired` already serves, and a later
  one hides more. A replay or a simulation names its now, as ADR-0011 d7 sends its fictional
  time as data.

- **d6 — Recall reads at most one date window from the query, by rule, with no model: rule
  `oml-when/1`.** The parser lower-cases the query, splits it into words, and reads the first
  date expression from the left, trying the longer forms first:

    | Form | Example | Window, with now on Thursday 2026-11-05 |
    |---|---|---|
    | ISO day, month | `2026-03-14`, `2026-03` | that day; that month |
    | year, 1900 to 2100 | `2015`, `in 2015` | the year |
    | month name, with a day or a year or both | `March 2025`, `March 14, 2025`, `14 March 2025` | that month or day |
    | month name alone after `in`, `during`, `since`, `until`, `before`, `after`, `of`, `last`, `this`, `early`, `mid`, `late` | `in March` | the latest March that has begun: 2026-03 |
    | quarter, with or without a year | `Q1 2027`, `Q3` | that quarter; the latest begun Q3 |
    | `today`, `yesterday`, `tomorrow` | | that day |
    | `this` / `last` / `next` `week` (Monday first), `weekend`, `month`, `quarter`, `year` | `last week` | 2026-10-26 to 2026-11-02 |
    | weekday, bare or after `on`, `last`, `this`, `next` | `last Saturday` | 2026-10-31 (bare and `on`: the latest on or before today) |
    | *n* `days` / `weeks` / `months` / `years` `ago`, *n* a numeral, `a`, or `one` to `twelve` | `three weeks ago` | the week holding now less 21 days |
    | `last` / `past` *n* units | `past 3 days` | the 3 days before now, through today |
    | `between` A `and` B, `from` A `to` / `until` / `through` B | `between 2014 and 2016` | A's start to B's end |
    | `since` A, `after` A, `before` A, `until` A | `since March 2025` | open on the other side |

    A word that only looks like a date is a cost carried openly: `2015` is a year even in
    "error 2015", and a bare month name other than after the words listed is not read, so "may"
    and "march" as verbs are not dates. The answer's `search` gains `window`: `{from, until,
    phrase}` in the instant form, an open side `null`, or `null` when no expression was read, so
    an agent sees what recall understood. The query text is searched unchanged: the date words
    stay words. One window per query; a second expression is ignored, a named limit. The rule
    lives in `oml-domain` with its table as tests, so a change to it is a change to this record.

- **d7 — A date window is a retriever leg, `time`, served under row security, fused by
  reciprocal rank and capped: pipeline `bm25-postings-time`.**
    - **The leg.** `time` reads the scope's rows (ADR-0049 d2, d3 above) whose declared interval
      overlaps the window: `valid_from_us < until` and (`valid_until_us` is null or `> from`). A
      row with no `valid_from` never matches, since an open start would match every window.
      Every predicate is an `int8` comparison, leakproof on PostgreSQL 18
      (`pg_proc.proleakproof` for `int8lt`, `int8gt`, `int8le`, `int8ge`, checked 2026-11-05),
      so the index on (`tenant_id`, `valid_from_us`) serves the serving role under row security
      ([ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)
      d1's reason for integers). It reads at most 200 rows, nearest start to the window's start
      first, then id.
    - **Its order, in the core.** The core orders the leg's rows by the text list's rank where
      the text list holds them, then by the leg's own order, and cuts it at 50, the depth
      ADR-0049 d3 reads a fused list to. So a row that matches both the words and the window
      ranks first in both lists, and a row in the window that no word matches can enter only
      below every one that does, and only 50 deep: the cap.
    - **Fusion.** `bm25-postings-time` fuses `bm25_postings`, read 50 deep, and `time` by
      reciprocal rank, k 60 (ADR-0049 d3). A query with no window gives an empty `time` list, and
      reciprocal rank over one list keeps that list's order, so the pipeline answers as
      `bm25-postings` does, in the same order. `matched.via` names `time`.
    - **Opt-in.** The deployment enables it and a tenant or a request picks it (ADR-0049 d4).
      It becomes the default only by ADR-0049 d9's gate, on the measurements below.

- **d8 — A bounded recency boost, opt-in: pipeline `bm25-postings-time-recent`.** After fusion
  and any rerank, before `k`, the core multiplies each of the first 50 candidates' scores by
  0.9 + 0.2 · 2^(−age / 365 days), where age is now less the row's `valid_from`, or its recorded
  time when it declares none, and a start after now has age 0; then it reorders them by the new
  score, ties by id. The factor lies in (0.9, 1.1], so no candidate moves by more than ±10% of
  its own score, Hindsight's cap. It reads the row's valid and recorded times and nothing else:
  never `promoted_at`, a verdict or a reviewer (R3; ADR-0049 d2). Each hit says its factor in
  `matched.recency`. The half-life is unmeasured beyond this record's run. It ships opt-in and
  is never the default without passing ADR-0049 d9's gate; recency is never on by default,
  since a newer memory is not a truer one.

- **d9 — The API: arguments on the two tools and the three routes, no new tool.**
  `oml_write` gains `valid_from` and `valid_until`, with and without `replaces`. `oml_read`'s
  recall mode gains `include_expired` and `now`. `POST /v1/memories`, the supersede route and
  `POST /v1/memories/recall` take the same members. Every served item (get, listing, recall,
  review) gains `valid_from` and `valid_until`, each the signed string or `null`. The recall
  answer's `search` gains `window` (d6) and each hit's `matched` gains `recency` under d8's
  pipeline. The CLI, the SDKs' typed helpers and the review page show the ends where they show
  an item; declaring them from the CLI is a follow-up (open question 3).

- **d10 — The server reads no date out of text.** The author declares valid time; the server
  validates the form and nothing else. A client may suggest ends from the text it writes, under
  its own rules, and the author still declares them. This slice ships no such helper: the
  benchmark loaders declare the dates their datasets state as metadata (a LoCoMo session's
  date, a LongMemEval session's date).

- **d11 — Migration 0026 adds the columns, a check and two partial indexes.** `memories` gains
  `valid_from text`, `valid_until text`, `valid_from_us int8` and `valid_until_us int8`, with a
  check that `valid_from_us < valid_until_us` when both are set, an index on (`tenant_id`,
  `valid_from_us`) where it is set, and one on (`tenant_id`, `valid_until_us`) where it is set.
  An existing row is open on both sides, so the migration rewrites nothing. 0025 is ADR-0057's,
  on its branch; if another number lands first this one is renumbered at merge.

- **d12 — Doctor names a time pipeline that has nothing to read.** `GET /v2/search` answers
  `valid_time: {declared, expired}`, the served versions on the key's readable nodes that
  declare a `valid_from` and those whose end has passed. `oml doctor` prints
  `time: <declared> memories declare valid time; <expired> ended and hidden from recall` when a
  time pipeline is enabled, and warns when one is enabled and no served memory declares valid
  time, since its `time` list is then always empty and it answers as `bm25-postings` does.

- **d13 — Laws.** Each is a test named after its sentence, red before the code:
  `an_expired_memory_is_hidden_from_recall_unless_asked`, under row security on PostgreSQL and
  on the in-memory store; `an_expired_memory_spends_no_place_in_k`;
  `the_time_leg_never_returns_a_row_outside_the_scope` (another node's row, another key's held
  row, steering, an empty scope); `a_row_with_no_valid_from_never_matches_a_window`;
  `a_query_with_no_window_answers_as_bm25_postings_does`; `the_date_parser_reads_each_form_in_its_table`
  (d6's table as cases); `valid_time_is_refused_on_steering_guardrail_and_a_retirement`;
  `a_successor_inherits_no_valid_time`;
  `a_ratified_supersession_ends_the_replaced_version_where_the_successor_begins` (and a held one
  ends nothing); `the_recency_factor_stays_within_ten_percent`;
  `no_ranking_input_reads_promoted_at_or_a_verdict` (two rows differing only in `promoted_at` and
  their verdicts rank equal under every time pipeline); and the signed payload carries the ends
  only when sent.

## Measurements

Filled by this branch's run (`eval/bench`, release build, PostgreSQL 18 on the 8-core host, one
client on loopback); see [Consequences](#consequences) for what they decide.

## Consequences

- ADR-0036 stays proposed for what this record does not build: as-of reads, `tip_intervals`,
  `valid_at` and valid-range filters on listings, MCP resources and `oml fs`. Its d2 is
  narrowed by d2 here (the name `valid_until` and the period end).
- A tenant that declares no valid time sees no change but one comparison per scanned row: no
  row is expired, and the time pipelines answer as `bm25-postings`.
- Valid time is clear metadata outside the encrypted body, as ADR-0036 d2 said; an erasure
  leaves the ends wherever it leaves attribution metadata, under FR-1.
- The time leg's cost grows with the rows in a window, bounded by its 200-row read; a window as
  wide as a decade over a large tenant reads its 200 nearest starts, not every row in it, which
  can drop a matching row that starts far from the window's start. Named, unmeasured at scale.
- The parser is English only.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A model parses the query's dates** | A model call on every recall costs latency and a dependency, and its answer is not reproducible; the rule table covers the forms the eval sets use, and a missing form is a row added to d6. |
| **The server extracts valid time from the text** | The author knows when a fact holds; a date in the text is often a mention, not the fact's span ("because the March 2026 outage..."). A wrong extracted date would hide a true memory. |
| **Automatic invalidation, a newer memory ending an older one** | It decides that two memories conflict, which is a judgment, and would change visibility with no review (R2, R3). Graphiti does it with a model; here a reviewed supersession does it. |
| **Recency always on** | A newer memory is not a truer one, and a boost on by default would reorder every recall by an input the eval has not justified. Opt-in, bounded, gated. |
| **Expiry as a ranking weight** | An ended fact ranked lower still answers as if true; hiding it is a visibility rule, and `include_expired` asks for it plainly. |
| **Importance or trust weights** | They would read who wrote or approved a row, which ranking never reads (R3, ADR-0049 d2). |
| **Deleting or demoting expired memories** | An ended fact is history, not a mistake; R4. |
| **ADR-0036's full build first (as-of reads, resources, `oml fs`)** | No measurement asks for them yet, and valid time with expiry and a date leg is what the temporal categories test. They stay proposed. |
| **A date window as a filter instead of a leg** | A filter would remove every memory outside the window, and a query's date is often loose ("around March"); as a fused leg it lifts what matches and drops nothing the text found. |
