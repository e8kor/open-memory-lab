<!--
id: adr/ADR-0059-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model
tier: engineering
status: accepted design · 2026-11-06
answers: which part of ADR-0036's valid time this slice builds and in what shape; how an author declares when a memory holds, including a vague period such as a year, and why it is signed and immutable; why a memory whose valid time has ended is hidden from recall by default and how a reader asks for it; how a fact is invalidated without the server deciding it; which date phrases recall reads from a query and how, with no model; how a date window becomes a retriever leg that row security can serve and how it is fused and capped; why a recency boost was tried and rejected; the API on oml_write, oml_read and HTTP; the migration, doctor line, laws and measurements; how get, the scoped listing and recall read memory as it was at a past instant under today's access, which two facts they take from that instant, and why the row's own recorded times answer it
-->

# ADR-0059 — A memory states when it is true, recall hides what has ended, and recall searches the dates a query names without a model

**Status:** accepted design · 2026-11-06 (was proposed) · introduced in v0.6.0 · first recorded 2026-11-05 · amended 2026-11-11 with as-of reads, built in part from ADR-0036 d3 and narrowed ([the amendment](#the-amendment-of-2026-11-11-as-of-reads), d14–d21; #192)

**Release:** introduced in v0.6.0 ([releases](../changelog.md))

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
      exclusive. Every period is read in UTC: `2026-11-05` runs from 00:00 to 24:00 UTC, which
      is 16:00 on 4 November to 16:00 on 5 November in UTC−8, and an instant with an offset is
      reduced to UTC. The interval is [from, until); an absent end is open; both present need from <
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
      inherits none: it states its own or is open. The served item carries both, and the review
      card, the bulk review table and `oml review next` / `oml review list` show them beside the
      text (`valid from <start>, <until <end> | open>`, or `none declared`), so a reviewer judges
      them with the text and a verdict approves both together.

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
  cut, so `keyword-all` may serve fewer than `k` when expired rows rank in its top `k`; that
  re-check is `keyword-all`'s one fence, and a law fails when it is removed. The now is a bound
  `int8` parameter in every statement (the least `int8` under `include_expired`), never a
  session setting: a column compared with a parameter is leakproof, so under row security the
  ended versions are read by the index on (`tenant_id`, `valid_until_us`), an index condition
  (Measurements), where `current_setting` in the comparison kept it a heap filter over every
  dated row of the tenant. Cost: every other scan statement gains one `int8` comparison per row.
    - **Statistics.** An expired version counts in no statistic, in either `bm25` pipeline, as
      it counts in no list: `bm25`'s scan computes its row count, mean length and document
      frequencies over the scope, which leaves it out, and `bm25-postings` takes the ended
      versions on the readable nodes out of its per-workspace counts by id. So the two pipelines
      rank alike whatever has expired, and with `include_expired` both count them again. A
      ranking that an invisible row moved would make visibility a weight.

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
  there. The row holds the end in force: after a ratification its `valid_until` is the ratified
  instant, and the end the author declared, if any, stays only in the submission event; a reader
  tells them apart by the version's history, not by the row. The reviewer of a successor is shown
  the end approving it gives (`replaces_valid_until` on the served item, and "approving ends
  version <n> at <instant>" on the card, the table and the CLI), so no end is approved unseen.
  Recall serves tips only, so a successor whose `valid_from` is later than now is served at once
  and the version it replaces stops being served at the ratification: valid time schedules no
  hand-over, and an author who wants the old fact served until a date keeps the successor held
  or declares no start. Nothing is deleted (R4). Detecting that two memories contradict each
  other is not built. This composes with ADR-0057's `expected_version` on a supersession (draft PR #69): the
  version a supersession names is the one the ratification ends.

- **d5 — A recall has one now, the server's clock unless the request names it.** `now`, a date
  or an instant as d2 parses it (a period reads as its first instant), anchors two things: the
  expiry cut (d3) and the relative dates the parser reads (d6), whose days are UTC days: a now of
  `2026-11-05T23:30-05:00` is 6 November in UTC, so "today" is 6 November. Absent,
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
    | month name alone after `in`, `during`, `since`, `until`, `till`, `before`, `after`, `of`, `last`, `this`, `early`, `mid`, `late`, or after `from` or `between` | `in March`, `mid March`, `a note from May` | the latest March that has begun: 2026-03 (`early`, `mid` and `late` read the whole month) |
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
    stay words. One window per query; a second expression is ignored, a named limit. A window
    that would reach outside the years 0000 to 9999, as an extreme `now` can make it, is no
    window (`search.window` is `null`), so every instant it shows has the fixed-width form. The rule
    lives in `oml-domain` with its table as tests, so a change to it is a change to this record.

- **d7 — A date window is a retriever leg, `time`, served under row security, fused by
  reciprocal rank and capped: pipeline `bm25-postings-time`.**
    - **The leg.** `time` reads, of the rows the text list found (their ids, at most the 50 or
      `k` the text list was read to), those in the scope (ADR-0049 d2, d3 above) whose declared
      interval overlaps the window: `valid_from_us < until` and (`valid_until_us` is null or `> from`). A
      row with no `valid_from` never matches, since an open start would match every window.
      Every predicate is an `int8` comparison, leakproof on PostgreSQL 18
      (`pg_proc.proleakproof` for `int8lt`, `int8gt`, `int8le`, `int8ge`, checked 2026-11-05),
      so the index on (`tenant_id`, `valid_from_us`) serves the serving role under row security
      ([ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)
      d1's reason for integers). Bounded by the text list's ids, the read is a primary-key probe
      of at most that list, whatever the window holds: a wide window ("since 2015") costs no
      more than a narrow one, and no row the words found is lost to a read cap. Its order: the
      rows that start inside the window first, equal among themselves, then those that started
      before it, nearest first; ties by id, so equal input gives equal output.
    - **Its order, in the core.** The core keeps the leg's rows that the text list holds, read
      50 deep, orders them by the leg's own score (inside the window first, then earlier
      starts, nearest first) and then by the text list's rank, and cuts the list at 50, the
      depth ADR-0049 d3 reads a fused list to: the cap. So the leg lifts a memory the words
      found and whose time fits, and never adds one the words did not find. This is the third
      of three orderings measured (Measurements below); the first two, which let rows no word
      matched enter or ordered by the text rank alone, measured lower.
    - **Fusion.** `bm25-postings-time` fuses `bm25_postings`, read 50 deep, and `time` by
      reciprocal rank, k 60 (ADR-0049 d3). A query with no window gives an empty `time` list, and
      reciprocal rank over one list keeps that list's order, so the pipeline answers as
      `bm25-postings` does, in the same order. `matched.via` names `time`.
    - **Opt-in.** The deployment enables it and a tenant or a request picks it (ADR-0049 d4).
      It becomes the default only by ADR-0049 d9's gate, on the measurements below.

- **d8 — A recency boost was tried and is rejected; revisit only with QA-level evidence.** The
  branch built `bm25-postings-time-recent`: after fusion, each of the first 50 scores multiplied
  by 0.9 + 0.2 · 2^(−age / 365 days), the age from the row's `valid_from` or its recorded time,
  never `promoted_at` or a verdict. It lowered every set measured (Measurements): LoCoMo headline
  nDCG@10 0.504 to 0.461 and S@1 0.384 to 0.304, LoCoMo temporal 0.594 to 0.537, LongMemEval
  temporal-reasoning 0.642 to 0.626, synthetic unchanged. In a conversation the latest turn is no
  likelier to be the evidence, and a newer memory is not a truer one. The pipeline, the factor
  and `matched.recency` are removed; recency returns only with evidence at the QA level, an
  answer judged right more often with it, not a ranking metric alone.

- **d9 — The API: arguments on the two tools and the three routes, no new tool.**
  `oml_write` gains `valid_from` and `valid_until`, with and without `replaces`. `oml_read`'s
  recall mode gains `include_expired` and `now`. `POST /v1/memories`, the supersede route and
  `POST /v1/memories/recall` take the same members. Every served item (get, listing, recall,
  review) gains `valid_from` and `valid_until`, each the signed string or `null`. The recall
  answer's `search` gains `window` (d6). Every recall hit, through HTTP and `oml_read`, carries
  its `valid_from` and `valid_until`, since the gain this record expects is at the QA level: a
  "when" question answered from the memory's own dates, not from its rank. That gain is
  measured by the chat lane's end-to-end QA benchmark (proposed ADR-0063 d12, draft branch
  `feat/chat-memory`: LongMemEval and LoCoMo answered by one fixed model from the top-k hits),
  not by the retrieval metrics below. The CLI, the SDKs' typed helpers and the review page show the ends where they show
  an item; declaring them from the CLI is a follow-up (open question 3).

- **d10 — The server reads no date out of text.** The author declares valid time; the server
  validates the form and nothing else. A client may suggest ends from the text it writes, under
  its own rules, and the author still declares them. This slice ships no such helper: the
  benchmark loaders declare the dates their datasets state as metadata (a LoCoMo session's
  date, a LongMemEval session's date).

- **d11 — Migration 0026 adds the columns, a check and two partial indexes, safely on a live
  database.** `memories` gains `valid_from text`, `valid_until text`, `valid_from_us int8` and
  `valid_until_us int8`, with a check that `valid_from_us < valid_until_us` when both are set,
  an index on (`tenant_id`, `valid_from_us`) where it is set, and one on (`tenant_id`,
  `valid_until_us`) where it is set. An existing row is open on both sides, so the migration
  rewrites nothing. It follows 0025's shape: a lock is waited for 5 s at most, then the
  migration fails and is run again; the check is added `NOT VALID` and validated under SHARE
  UPDATE EXCLUSIVE; each index is created `ON ONLY` the parent and each partition's is built on
  its own and attached, so no write waits on more than one partition at a time. A tenant
  partition created later inherits both indexes. If another migration takes 0026 first, the
  merge lane renumbers this one.

- **d12 — Doctor names what expiry hides and a time pipeline that has nothing to read.**
  `GET /v2/search` answers `valid_time: {declared, expired}`, the tenant's served versions that
  declare a `valid_from` and those whose end has passed, under every pipeline, since the expiry
  rule binds the default too. `oml doctor` prints
  `time: <declared> memories declare valid time; <expired> ended and hidden from recall unless
  include_expired` whenever either count is above zero, and warns when a time pipeline is
  enabled and no served memory declares valid time, since its `time` list is then always empty
  and it answers as `bm25-postings` does.

- **d13 — Laws.** Each is a test named after its sentence, red before the code:
  `an_expired_memory_is_hidden_from_recall_unless_asked`, under row security on PostgreSQL and
  on the in-memory store; `an_expired_memory_spends_no_place_in_k`;
  `the_time_leg_never_returns_a_row_outside_the_scope` (another node's row, another key's held
  row, steering, an empty scope); `a_row_with_no_valid_from_never_matches_a_window`;
  `a_query_with_no_window_answers_as_bm25_postings_does`; `the_date_parser_reads_each_form_in_its_table`
  (d6's table as cases); `valid_time_is_refused_on_steering_guardrail_and_a_retirement`;
  `a_successor_inherits_no_valid_time`;
  `a_ratified_supersession_ends_the_replaced_version_where_the_successor_begins` (and a held one
  ends nothing); `no_ranking_input_reads_promoted_at_or_a_verdict` (two rows differing only in `promoted_at` and
  their verdicts rank equal under every pipeline); the signed payload carries the ends only
  when sent; and, from the review of 2026-11-05:
  `a_reviewer_sees_the_valid_time_and_the_end_an_approval_gives_on_the_card_and_the_bulk_table`;
  `migration_0026_is_live_safe_and_leaves_every_partitions_indexes_attached`;
  `a_ratified_end_is_written_with_the_tip_move_or_not_at_all` (both stores);
  `the_expiry_cut_is_an_index_condition_for_the_serving_role_under_row_security`;
  `an_expired_memory_is_hidden_from_recall_unless_asked` over `keyword-all` too, which fails when
  the core's re-check is removed;
  `the_time_leg_reads_only_the_text_lists_rows_however_many_the_window_holds`; and
  `bm25_postings_ranks_as_bm25_does_when_a_readable_row_has_expired`.

## Measurements

Measured 2026-11-05 on this branch's release build, PostgreSQL 18 on the shared 8-core host
under other load, one client on loopback, `k` 100, through `oml_read` over `/mcp`
(`eval/bench/run.sh`). The loaders declare each memory's `valid_from` from the date the dataset
states as metadata and never from its text (d10): a LoCoMo turn its session's day, a
LongMemEval turn its session's instant. No `valid_until` is declared, so nothing is expired and
`bm25-postings` ranks exactly as on main. Each question is asked at a `now`: LoCoMo the day after
the conversation's last session, LongMemEval the question's own date. LongMemEval is
`longmemeval_s_cleaned` (MIT, 277 MB, not committed), its 133 temporal-reasoning questions, a
seeded 30 (seed 20261104), 14,826 turns, one workspace per question, the `has_answer` turns as
qrels; no answer is generated.

| Set (queries) | Pipeline | nDCG@10 | S@1 | S@5 | p50 ms | p95 ms |
|---|---|---:|---:|---:|---:|---:|
| synthetic (121) | `bm25-postings` | 0.874 | 0.835 | 0.926 | 9.3 | 15.4 |
| synthetic, temporal (11) | `bm25-postings` | 0.769 | 0.455 | 0.818 | 8.8 | 9.8 |
| synthetic, every category | `bm25-postings-time`, `-time-recent` (rejected, d8) | equal to `bm25-postings` | | | 9.6–9.9 | 15.8–16.1 |
| LoCoMo headline (1,535) | `bm25-postings` | 0.504 | 0.384 | 0.628 | 19.5 | 33.0 |
| LoCoMo headline | `bm25-postings-time` | 0.496 | 0.377 | 0.620 | 25.9 | 38.6 |
| LoCoMo headline | `bm25-postings-time-recent` (rejected, d8) | 0.461 | 0.304 | 0.613 | 20.4 | 29.9 |
| LoCoMo temporal (320) | `bm25-postings` / `-time` / `-time-recent` | 0.594 / 0.596 / 0.537 | 0.478 / – / 0.347 | 0.691 / 0.694 / 0.684 | | |
| LoCoMo single-hop (841) | the same three | 0.571 / 0.557 / 0.525 | | 0.673 / 0.656 / 0.655 | | |
| LoCoMo multi-hop (282) | the same three | 0.289 / 0.289 / 0.262 | | 0.507 / 0.511 / 0.482 | | |
| LoCoMo open-domain (92) | the same three | 0.233 / 0.222 / 0.224 | | 0.370 / 0.359 / 0.380 | | |
| LoCoMo adversarial (446) | the same three | 0.562 / 0.550 / 0.524 | | 0.668 / 0.655 / 0.650 | | |
| LongMemEval temporal-reasoning (30) | `bm25-postings` | 0.642 | 0.600 | 0.733 | 35.7 | 50.5 |
| LongMemEval temporal-reasoning | `bm25-postings-time` | 0.628 | 0.600 | 0.733 | 44.3 | 72.9 |
| LongMemEval temporal-reasoning | `bm25-postings-time-recent` (rejected, d8) | 0.626 | 0.600 | 0.733 | 40.3 | 50.5 |

The parser read a window in 3 of 121 synthetic queries (no synthetic memory declares a date, so
each time list was empty), 285 of 1,981 LoCoMo questions (151 single-hop, 68 adversarial, 45
temporal, 13 multi-hop, 8 open-domain; mostly "in August 2023", "on 31 October 2022") and 11
of 30 LongMemEval questions. The `-time-recent` rows of LoCoMo and LongMemEval were measured
with the first ordering of d7; the `-time` rows with the ordering d7 decides. The two earlier
orderings of the leg measured 0.494 (rows no word matched admitted, ordered by text rank) and
0.494 (words' rows only, ordered by text rank) on the LoCoMo headline, and 0.614 on LongMemEval.

The expiry cut's plan (d3), `EXPLAIN` of `bm25-postings` as the serving role under row
security on the five-row fixture of
`the_expiry_cut_is_an_index_condition_for_the_serving_role_under_row_security`: with the now
read from `current_setting`, the probe of `memories_valid_until` had only `tenant_id` as its
index condition and `valid_until_us <= COALESCE(current_setting(…))` as a heap filter (statement
cost 37.59); with the now bound as `int8`, `valid_until_us <= $15` is part of the index
condition (cost 37.58). The estimate barely moves on five rows; what changes is the read: the
filter form reads every row of the tenant that declares an end, which the review measured at
39,910 rows read to find 90 on a 200,000-row partition, and the index condition reads the ended
rows alone.

What it decides: the recency pipeline is removed (d8), and neither time pipeline passed ADR-0049 d9's gate against `bm25-postings`, which
stays the default. On LoCoMo the leg moves the temporal category by +0.002 nDCG@10 and costs
single-hop 0.014; on LongMemEval it costs 0.014. LoCoMo's temporal questions mostly ask when
something happened and name no date, so the leg cannot reach them; the questions that do name a
date are single-hop ones about a session, where the window holds a whole session of turns that
equal-scored text matches already rank. Recency lowers every set (LoCoMo S@1 0.384 to 0.304):
in a conversation the latest turn is no likelier to be the evidence. The value this slice ships
is the visibility rule: an ended fact is hidden from recall (d3), which no benchmark here
declares, so none measures it; its laws are the tests of d13. The latency cost of the leg is
one extra statement when the query names a window: p50 +0.6 to +6.4 ms on these runs, on a
loaded host.

## Consequences

- ADR-0036 stays proposed for what this record does not build: `as_of_seq`, `tip_intervals`
  and `journal_watermarks`, `valid_at` and valid-range filters on listings, MCP resources and
  `oml fs`. Its d2 is narrowed by d2 here (the name `valid_until` and the period end), and its
  d3 is built in part and narrowed by the amendment of 2026-11-11 (d14–d21).
- A tenant that declares no valid time sees no change but one comparison per scanned row: no
  row is expired, and the time pipelines answer as `bm25-postings`.
- Valid time is clear metadata outside the encrypted body, as ADR-0036 d2 said; an erasure
  leaves the ends wherever it leaves attribution metadata, under #146.
- The time leg reads only the text list's rows, at most the depth that list was read to, so
  its cost does not grow with a window's width; a row in the window that no word found is never
  reached, by design (d7).
- The parser is English only.

## The amendment of 2026-11-11: as-of reads

Issue #192 asked to ship reads of memory as it was at a past time or to state the boundary. This
amendment ships them for get, the scoped listing and recall, by building ADR-0036 d3, which is
proposed (draft PR #19, closed unmerged), in the part a read needs, and narrowing it where the
build found a simpler mechanism. The operator's brief of 2026-11-11 fixes three points this
record keeps: `as_of` is transaction time, the read applies **today's** grants and never the
grants of that time, and an erased row never reappears. Where ADR-0036 and the brief are silent,
the reading is recorded here before the code.

- **d14 — `as_of` is a transaction-time instant, apart from both valid-time inputs.** `as_of`,
  an RFC 3339 `date-time` with an offset and at most six fractional digits, rides
  `GET /v1/memories/{artifact_id}` and `GET /v1/memories` as a query parameter and
  `POST /v1/memories/recall` as a member, and `oml_read` takes it in its three modes. It asks what
  OML held at that instant. It is not `now` (d5), which anchors the expiry cut and the query's
  relative dates in valid time, and it is not the listing's `since` and `until`, which filter by
  recorded time and keep doing so; the three compose. A date or a period is refused
  (`request.validation`, `details.field` `as_of`): "as of 10 October" could mean its first or its
  last instant, and the two answers differ. This narrows ADR-0036 d3 and d7: no `as_of_seq`, and
  no `tip_as_of` on items, since every version an as-of listing or recall serves as promoted was
  the tip at the instant, and get at an instant answers whether the version existed then.

- **d15 — Rule `oml-asof/1`: two facts are taken at the instant, everything else is today's.**
  (1) *Existence:* a version whose submission was recorded after the instant does not exist
  (`created_at` > `as_of`). (2) *The tip:* among a claim's versions (one `workspace_id` and
  `claim_key`) whose `promoted_at` is at or before the instant, the one with the greatest
  `version` was its tip then. Everything else is read today: the key's role, grants and denials
  (ADR-0025 d1), each version's state, workspace, scan and in-progress boundary (ADR-0030 d3),
  retirement, kinds, the expiry cut at the request's `now` (d3), the origin filters, and the
  scores. So:
    - A tip at the instant that is not promoted today (demoted or erased, once those acts are
      built) is served at no instant, and no earlier version stands in for it (ADR-0036 d3, R4).
      An erased version never reappears.
    - A claim retired after the instant serves the tip it had then, a version today's get serves
      the reader as history (ADR-0025 d8); retired by the instant, recall serves nothing of it
      and the listing its retirement version, flagged, as the listing does today.
    - A held row is served at an instant only when it was recorded by then and today's rules
      serve it: the caller's own with `include_unreviewed`, or inside an in-progress scope
      (ADR-0030 d3). A version held at the instant and promoted since was not the tip then and is
      not held now, so it is absent.
    - Every row an as-of read serves is therefore one today's get serves the caller: access never
      widens, and a denial issued today hides the past too (the grants held at the instant are
      rejected below).

- **d16 — The row's own recorded times answer the instant; no interval table is built.** On
  main, a claim's tip moves only by `lifecycle.promote` and a ratified `lifecycle.supersede`,
  each setting the promoted version's `promoted_at` to its event's `at`, and a successor's
  `version` is one above every version before it (ADR-0025 d10, d18). (`promoted_at`, `version`)
  is therefore the claim's whole tip history, and `created_at` its existence. ADR-0036 d3's
  `tip_intervals`, `submitted_seq` and `journal_watermarks` are not built; a record that adds a
  tip move setting no `promoted_at` (a demotion that returns an earlier version, a tip override)
  adds the interval fold in the same change. Cost, stated: `at` may step backwards after a
  failover or a clock step ([storage](../architecture/storage.md#time)), and then one instant can
  name a set of facts that is not a prefix of the chain; it never names a row the key cannot read
  today. ADR-0036's watermark returns when a read must name one exact prefix, such as the
  repeatable recall of #176. A promoted version with no `promoted_at`, which a row folded before
  migration 0005 can be, is served at no instant, and the version it replaced stands in for it:
  `GET /v2/search` counts them as `valid_time.untimed`, and `oml doctor` warns `as_of: <n>
  promoted versions have no promoted_at` when the count is above zero.

- **d17 — No future, and no echo.** An `as_of` after the lead's clock at the request is
  `request.validation` naming `as_of`: the instant has not happened, and a read of it would answer
  differently once it had. This narrows ADR-0036 d3's `request.as_of_ahead`: one lead has no
  applied head to name, so no new code. No answer carries the instant or a `seq` (ADR-0036 d3).

- **d18 — Recall at an instant: the same pipelines over the scope at the instant, read by the
  scan where a list reads today's counts.** Steering is the steering tips at the instant on
  today's readable ancestors. The candidate scope of ADR-0049 d2 becomes the tips at the instant
  and the held rows of d15, in every statement that reads it: `keyword-any`, `bm25`,
  `bm25-trigram`, the `time` leg and `bm25-rerank`'s first stage, so `bm25`'s row count, mean
  length and document frequencies are the scope's at the instant, and a version served today but
  not then moves no score. The postings lists, `bm25_postings` and `sparse`, keep per-workspace
  counts and postings states of today's served versions alone (ADR-0052 d5), so under `as_of` each
  is read by `bm25`'s scan over the scope at the instant, `search.access` answering `scan`; and
  `keyword-all`'s two v0.5.0 statements, kept byte for byte (d3), are read under `as_of` by
  `keyword-any`'s statement with every word required. `search.pipeline` names the pipeline asked.
  An as-of recall reads its scope alone: `sources` other than `scope` is `request.validation`
  naming `/sources`, and a tenant whose default is `wide` reads `scope` (ADR-0072 d5 stays as it
  is for every other recall). `did_you_mean` is empty at an instant: the vocabulary it reads is
  today's (ADR-0049 d6), and a word only a version recorded after the instant holds would name
  a version that did not exist then. The request digest gains `as_of` when sent, as it gained
  `now` (d5), so a recall that sends none digests as before.

- **d19 — The listing and get at an instant.** `as_of` reads the scoped listing (ADR-0039 d4):
  the tips at the instant on the served nodes and, inside an in-progress scope, the held rows of
  d15, newest first under one cursor. With `own`, `with_own`, `author` or `tip` it is
  `request.validation` naming `as_of`, since the own listing serves every state and has no tip to
  take; `oml_read`'s listing, whose `own` defaults to true, reads the scoped listing when `as_of`
  is sent without `own`. Get at an instant answers `access.not_found` for a version recorded
  after it, and otherwise exactly as get answers today. Get takes `as_of` as its one query
  parameter, and any other is now `request.validation`, as the listing's unknown parameters are.
  A refusal names the field as the surface spells it: `as_of` for the HTTP query parameter,
  `/as_of` for the recall body's member and for `oml_read`'s argument in every mode.

- **d20 — One index serves the tip probe: migration 0041.** A version promoted by the instant
  is the tip then when it is today's tip, since a tip moves only forward (d16); the PostgreSQL
  statements and the in-memory store both take that short cut, and an act that makes a
  replaced version the tip again removes it with the interval fold. Otherwise the probe "the
  greatest version of this claim promoted at or before the instant", correlated so it runs once
  per replaced candidate, reads `memories_tip_moves` on (`tenant_id`, `workspace_id`,
  `claim_key`, `version`) `INCLUDE` (`promoted_at`) where `promoted_at` is set, backwards by an
  index-only scan whose index condition is the claim, under row security: equality on `uuid`
  and `text`, `int4gt` and `timestamptz_le` are leakproof (`pg_proc.proleakproof`, PostgreSQL 18,
  checked 2026-11-11). It holds the promoted versions of every state, so an erased successor
  still ends its predecessor's tip. The instant is inlined in the search statements as a literal,
  safe because an `EventTime` exists only through `EventTime::parse`, which admits digits and
  separators alone; binding it would renumber every statement's parameters, and such a statement
  is planned each time, never cached.
    - **The migration's cost on a live database.** Unlike 0026's indexes (d11), empty at
      creation, this one holds every promoted version, so each partition's build reads its
      tenant's promoted rows under a SHARE lock that makes that tenant's writes wait until it
      ends. 0041 is a `-- no-transaction` migration of one `DO` statement that creates the
      parent's index `ON ONLY`, then builds, attaches and commits each partition's in name
      order, so no lock outlives its own partition's build. An `INSERT`, which locks its
      tenant's partition alone, waits only for that tenant's build. A statement planned against
      the parent with the tenant as a parameter, such as a cached generic plan of an `UPDATE`
      through `memories`, can lock every partition as it starts, and then waits behind
      whichever tenant's build is running, for at most that one build. Reads never wait, since
      SHARE does not conflict with them. `CREATE INDEX
      CONCURRENTLY`, which would let writes run, cannot run inside a `DO` block or a
      multi-statement string, and the partitions are known only at run time. Each transaction
      waits at most 5 s for its lock, then the migration fails; every step is skipped when done,
      so running it again resumes. Forward-only, as every migration here: undoing it is `DROP
      INDEX memories_tip_moves`. After it, every promotion writes one more index entry.
    - **The read's cost, measured 2026-11-11** by `crates/adapters/oml-pg/tests/as_of_timing.rs`
      on `bm25` over a corpus where every row matches every query and every claim is a chain of
      `k` versions. At an instant a recall reads all `k` versions of each claim where today's
      reads the tip, and probes once per replaced version in each of `bm25`'s two scope reads, so
      the cost grows with `k`; two-version chains are the cheapest history. Each sample pairs a
      recall of today with one at the instant, back to back, and the figure is the median of the
      pairs' ratios. At `k` 2 and 10,000 rows: 2.0 on CI (today p50 80 ms, at the instant 168 ms)
      and 2.05 on the shared host. At `k` 5 and 20,000 rows on the shared host: 3.2 (81 ms against
      280 ms; 4,000 tips today against 20,000 rows read). At `k` 2 and 100,000 rows on the shared
      host under load, measured before the pairing: p50 490 ms against 1,736 ms, p95 541 ms against
      2,284 ms (3.5 and 4.2). `bm25-postings` at an instant is read by the scan and pays the same,
      3.2 at `k` 5. CI gates one figure: the median pair ratio at `k` 2 and 10,000 rows stays at or
      below 3. The `k` 5 and 100,000-row runs are measurements, run by hand and never gated. A tip
      set computed once per statement is the next step when an as-of use needs it faster.

- **d21 — Laws, each a test named after its sentence, red before the code.** Over both
  stores, from `oml-port-contracts::as_of`: `an_as_of_read_serves_the_tip_the_claim_had_then`
  (in recall over every pipeline served, steering, the listing, and get, where a version recorded
  after the instant is not found), `an_as_of_read_applies_todays_grants_and_denials`,
  `an_as_of_read_never_serves_a_version_erased_today` (PostgreSQL's runner names it
  `an_as_of_read_never_serves_a_version_erased_today_and_no_earlier_version_stands_in`),
  `a_retired_claim_serves_its_earlier_tip_before_the_retirement`,
  `held_rows_at_an_instant_follow_todays_rules`,
  `inside_an_in_progress_scope_held_rows_at_an_instant_follow_todays_rules` (ADR-0030 d3's
  other authors' held rows, recall and the listing), and
  `an_as_of_after_the_leads_clock_is_refused`; each recall law also checks that every row served
  at an instant is one today's get serves the caller. On PostgreSQL:
  `the_tip_probe_is_an_index_condition_for_the_serving_role_under_row_security`,
  `migration_0041_builds_each_partitions_index_in_its_own_transaction_and_a_second_run_changes_nothing`,
  `migration_0041_fails_on_a_locked_partition_keeping_the_ones_it_committed_and_a_rerun_completes`,
  `a_promoted_version_without_promoted_at_is_counted_for_doctor_and_served_at_no_instant`, and
  the timing gate `an_as_of_recall_costs_at_most_three_times_todays_over_two_version_chains`,
  with the measurements `the_as_of_recall_cost_at_100k_rows` and
  `the_as_of_recall_cost_over_five_version_chains`. Over HTTP,
  `as_of_reads_the_tip_at_an_instant_over_http`; over MCP,
  `oml_read_takes_as_of_in_every_mode_and_answers_as_http_does`.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A model parses the query's dates** | A model call on every recall costs latency and a dependency, and its answer is not reproducible; the rule table covers the forms the eval sets use, and a missing form is a row added to d6. |
| **The server extracts valid time from the text** | The author knows when a fact holds; a date in the text is often a mention, not the fact's span ("because the March 2026 outage..."). A wrong extracted date would hide a true memory. |
| **Automatic invalidation, a newer memory ending an older one** | It decides that two memories conflict, which is a judgment, and would change visibility with no review (R2, R3). Graphiti does it with a model; here a reviewed supersession does it. |
| **A recency boost, even opt-in and bounded** | Built and measured on this branch (d8): it lowered LoCoMo nDCG@10 from 0.504 to 0.461 and LongMemEval temporal-reasoning from 0.642 to 0.626. A newer memory is not a truer one. Revisit only with QA-level evidence. |
| **Expiry as a ranking weight** | An ended fact ranked lower still answers as if true; hiding it is a visibility rule, and `include_expired` asks for it plainly. |
| **Importance or trust weights** | They would read who wrote or approved a row, which ranking never reads (R3, ADR-0049 d2). |
| **Deleting or demoting expired memories** | An ended fact is history, not a mistake; R4. |
| **ADR-0036's full build first (as-of reads, resources, `oml fs`)** | No measurement asks for them yet, and valid time with expiry and a date leg is what the temporal categories test. They stay proposed. |
| **A date window as a filter instead of a leg** | A filter would remove every memory outside the window, and a query's date is often loose ("around March"); as a fused leg it lifts what matches and drops nothing the text found. |
| **d16 — ADR-0036 d3's `tip_intervals` and `journal_watermarks` now** | Two tables written in the chain lock's statement on every tip move and every event, and their reconciliation, to answer what (`promoted_at`, `version`) and `created_at` already answer while promote and a ratified supersede are the only tip moves. They return with the first tip move that sets no `promoted_at`, or a read that must name one exact prefix (#176). |
| **d17 — An instant after the lead's clock read as now** | The same request would answer differently once the instant passed, and recall's read log would hold an answer no later replay gives. |
| **d18 — Refusing every pipeline that reads postings under `as_of`** | `bm25-postings` is the default, so most as-of recalls would be refused; its counts are today's, and `bm25`'s scan over the scope at the instant is the ranking it approximates (ADR-0052 d4). |
| **d18 — Federated sources at an instant** | The reference set and the candidates are today's (ADR-0072 d4.2), so their statistics would mix two instants; no use asks for it yet (ADR-0039 Occam). |
| **d14 — `as_of_seq`, a journal seq, beside or instead of the instant** | A seq is the chain's own order, but answering at a seq names a key's tenant's event count to it, and bisecting seqs times events it cannot read (ADR-0036's own reason for never echoing one). No use asks for a seq; the instant answers the question asked. |
| **d14 — A date accepted as its first or its last instant** | "As of 10 October" reads as the day's start to one reader and its end to another, and the two answers differ by a day of writes; refusing it costs one `T00:00:00Z`. |
| **d14 — `tip_as_of` on every item** | Every promoted item an as-of listing or recall serves was the tip at the instant, so the flag would be constant there; get at an instant answers existence alone. |
| **d15 — The grants the key held at the instant** | A revoked key's past would stay readable, and a denial issued for cause could be stepped around by naming an earlier instant. Today's grants make an as-of read a subset of today's get. |
| **d15 — Serving the tip at the instant though it is demoted or erased today, or the version before it** | Demotion and erasure are journalled acts on what is served now (R4); the version before it was not what OML held at the instant. |
| **d19 — `as_of` on the own listing** | The own listing serves every state of the key's rows and has no tip to take; existence alone would be `until`, which it already takes. |
| **d20 — `CREATE INDEX CONCURRENTLY` per partition** | It cannot run inside a `DO` block or a multi-statement string, the forms a sqlx migration runs, and the partitions are known only at run time; a build per partition, committed one at a time, bounds each tenant's wait to its own rows. |
| **d20 — A gated p95 ratio** | Thirty unpaired samples on a shared runner make p95 one burst of load; the median of paired ratios moves with the code, not the host. |
