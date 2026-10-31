<!--
id: adr/ADR-0036-time-lives-in-the-journal-and-every-read-of-the-past-applies-todays-access
tier: engineering
status: proposed
answers: which clocks OML keeps and why they stay apart; how a claim version states when it is true in the world and who judges it; how a read asks what OML knew at a seq or an instant, and what today's access and state change in that answer; how valid_at and valid ranges filter recall and listings without ranking; how a version's files resolve over time; how agents read the same answers as MCP resources and terminals as oml fs; what the lake may derive; why no file system or storage engine is built; the build order
-->

# ADR-0036 — Time lives in the journal: a claim version may state when it is true, a read may ask what OML knew at a seq or an instant, and every read of the past applies today's access

**Status:** proposed · 2026-10-31

**Amends:** ADR-0005 (d4, d5), ADR-0014 (d2), ADR-0018 (d4), ADR-0025 (d8, d9, d12, d13)

## Context

Agents ask two questions OML cannot answer. "What did we hold on 10 October?" needs a read of the
past, and none takes a point in time (the pages' only as-of is ADR-0020's dataset as-of). "What was
true in Q3?" needs a claim that says when it holds; the recall example in
[openapi.yaml](../api/openapi.yaml) (openapi.yaml:82) says "valid until 2026-11-15" in words no read
can filter on.

**The user's decisions** (2026-10-31), fixed: (1) no file system or storage engine of OML's own:
time and claims live in the journal and an index above write-once objects; (2) bitemporal:
transaction time is what OML knew, and when, and valid time is new; (3) an as-of read applies
today's grants, erased content stays erased, demoted and rejected versions follow ADR-0030's
held-or-promoted rule, and nothing is served that review has not let that key see today; (4) MCP
resources for agents and `oml fs` for terminals, FUSE and WebDAV rejected or deferred; (5) time,
not scale.

**The investigation** behind (1), run on 2026-10-31 outside this repository, surveyed nine systems
and found time above the bytes in each. Datomic: "An as-of filter returns a database 'as of' at a
particular point in time, ignoring any transactions after that point"
(docs.datomic.com/reference/filters.html); Delta: "A given snapshot of the table can be computed by
replaying the events committed to the table" (delta-io `PROTOCOL.md`); both fetched 2026-10-31. OML
has that shape: the journal orders every act by `seq` ([storage](../architecture/storage.md#time)),
and "only ratification moves the tip"
([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).
What is missing is a read, not a store.

Proposed records on other branches are cited by number, unlinked: ADR-0026 (names), ADR-0030
(in-progress workareas, whose d4 boundary and d5 held readers are decided at read time from today's
tree, marks and grants), ADR-0031 (connections), ADR-0034 (role split), ADR-0035 (deliverables).
Each decision is the maintainer's reading, which the user confirms before acceptance. Proposed, not
built.

## Decision

- **d1 — Two axes, named, and `simulated_time` stays a third clock neither reads.** *Transaction
  time* is what OML knew, and when: the journal's `seq`, the authority, and `at`, informational.
  *Valid time* is when a claim version says it holds in the world (d2), stated by its writer,
  judged by its reviewer, read off no clock. An as-of read (d3) moves along the first only,
  `valid_at` (d4) reads the second only, and neither reads `simulated_time`, a simulation's declared
  fictional now: a simulation row states valid time like any row, and a scenario wanting the world
  at its fictional now sends that as `valid_at` itself, so the journal's clock stays "never lent to
  fiction" ([ADR-0011](ADR-0011-simulations.md) d7). The listing's `since` and `until`, whose axis
  no page names, are transaction time: the version's submission `at`. XTDB keeps the same split:
  "Users (rightly) have no control over system-time" and "Valid time is the second temporal
  dimension" (docs.xtdb.com/about/time-in-xtdb.html, fetched 2026-10-31 by the investigation).

- **d2 — Valid time is two optional ends in the signed submission, immutable per version.**
    - **Shape.** `valid_from` and `valid_to` ride submit (remember, learn), supersede and a
      simulation write: an RFC 3339 `full-date` or a `date-time` with an offset, the half-open
      [from, to), an absent end open. Both present need from < to, else `request.validation` with
      `details.field`. A date compares as 00:00:00Z that day and stays a date in the signed form, so
      its precision survives; an instant is normalized to UTC with six fractional digits, as `at`
      is ([chain format](../data/chain-format.md)), and a second 60 or a seventh digit is refused.
    - **The envelope.** `submission.remember`, `submission.learn` and `simulation.write` carry both
      members, `null` when open; an earlier event carries neither and folds as open, under
      [ADR-0004](ADR-0004-bodies-and-attachments.md) d2's additive rule. The fold keeps each signed
      string, served as written, and its instant in `memories.valid_from` and `memories.valid_to`.
    - **Immutable, never inherited.** An interval changes only by a reviewed supersession. A
      successor inherits none, `kind` being the only inherited field
      ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d11), so a revision that keeps it
      re-sends it, as ADR-0035 d4 re-sends files; a derived fact or deliverable states its own,
      and the server computes none from `derives_from`.
    - **Kinds.** `memory`, `derived_fact` and ADR-0035's `deliverable` take it; `steering`, which
      applies "in every recall of its scope" (ADR-0018 d4), the vocabulary kinds, which deprecate
      ([ADR-0021](ADR-0021-ai-human-ontologies.md) d3), and a retirement refuse it (`request.validation`).
    - **Review and erasure.** The serve shows both ends with the words and the verdict covers them,
      the serve binding the version (ADR-0004 d5): a right figure under a wrong interval is a wrong
      claim. Valid time schedules nothing: a version valid from 2027 is served today to every read
      that does not filter. The ends are clear journal metadata, like `body_bytes`, and survive
      crypto-shredding: ADR-0005 d5's list of what an erase leaves gains them (amends d5).

- **d3 — The as-of read resolves one seq and answers with today's access and state, rule
  `oml-asof/1`.**
    - **Parameters.** `as_of`, an RFC 3339 instant, or `as_of_seq`, a journal `seq`, never both
      (`request.validation`), on get, the scoped listing, recall, `oml_get`, `oml_recall`, the
      resources (d6) and `oml fs` (d7).
    - **Resolution.** `as_of` names the last seq of the longest chain prefix whose every `at` is at
      or before the instant, since `at` may regress across seqs: the lead stores each event's
      running maximum of `at`, `at_watermark`, an unsigned column computed from the chain head the
      lock already reads ([ADR-0024](ADR-0024-performance-foundation.md) d1 unchanged), and one
      index probe resolves an instant. Every answer echoes `as_of {seq, at, rule: "oml-asof/1"}`.
    - **No future.** A seq past the applied head, an instant later than the primary's clock, and on
      a follower ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d13) an instant no
      applied watermark passes answer `request.as_of_ahead` (`409`, `{applied_seq, applied_at}`);
      a follower never waits or forwards, and own held rows route to the lead (ADR-0024 d9).
    - **The rule.** A read at S serves exactly what the same read serves the key today, except that
      four tests are taken at S: whether a version had been submitted, was held, was promoted, and
      which version was a claim's served tip, default or the key's audience overlay
      ([ADR-0011](ADR-0011-simulations.md) d4). All else is today's: the key's role, grants and
      denials (ADR-0025 d1), ADR-0030's boundary and held readers on today's tree and marks, the
      audience, the `self_signed` scope, the version's placement after any reassignment, and its
      state. Tip moves (supersession, retirement, tip override) are history, read at S; demotion,
      rejection, erasure, a revoked grant, a denial and a mark are today's state, and it wins.
    - **What winning means.** A version promoted at T and demoted later is, for every S ≥ T, the
      tip of nothing, and no earlier version stands in, none having been the tip at S; get answers
      as for a demoted version today, and a return to service returns it to as-of reads. An erased
      version's content is served at no S; get answers `410 lifecycle.erased` to the keys that
      could read it before ([errors](../api/errors.md)). A version held at S reaches only the keys
      today's reads serve held rows to (its author, ADR-0030 d5's readers inside), and recall and
      the listing serve it only while it is held or promoted today, so nothing review turned down
      returns; a non-author outside sees as of S only what was promoted by S. Items gain
      `tip_as_of` and `state_as_of`. Nothing is served that get and the events tail do not serve
      the key today: tip flips are tail events, and the echoed seq is the event-feed count
      [FR-31](../product/roadmap.md) records.
    - **Computation: an interval fold, not a replay per read.** `claim_intervals` (`tenant_id`,
      `artifact_id`, `kind` ∈ `held` | `tip`, `simulation_id` null for the default tip, `from_seq`,
      `to_seq` null while open) opens or closes a row on each submission, verdict, reopen and
      `lifecycle.promote`, `supersede`, `tip_override`, `demote`, `erase` and `reassign`: one insert
      and one update in round trip two's statement. Partitioned and fenced per tenant as `memories`
      is (ADR-0034 d11, d3), it rebuilds from the journal, and a disagreeing row is a
      `state-mismatch` finding ([ADR-0017](ADR-0017-reconciliation.md) d4). Recall tests intervals in
      the statement that applies `LIMIT`, over indexes that hold every promoted version (ADR-0025
      d8). Scores are today's: what was known at S, ranked as the index ranks today, not the recall
      made at S, which the [read log](../architecture/retrieval.md#the-read-log) holds. Cost
      unmeasured — [FR-48](../product/roadmap.md).
    - **Not Delta time travel**: derived, serving no read ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md)
      d1), holding no held body (its d3), lagging by the frontier, batch- not seq-grained, ending at
      VACUUM under the lake erasure bound ([Delta tables](../data/delta-tables.md#maintenance-optimize-vacuum-checkpoints)).

- **d4 — `valid_at` and valid ranges filter versions and never rank them.** `valid_at`, a date or an
  instant, keeps a version when (`valid_from` null or ≤ t) and (`valid_to` null or t < it);
  `valid_since` and `valid_until`, either optional, keep one whose interval overlaps [since, until);
  `valid_at` with either is `request.validation`. Recall, both listings, the claim resource and
  `oml fs` take them.
    - **With `as_of`**, d3 first decides what the key reads at S, then the filter keeps what was
      true at t: "what we knew at S about the world at t".
    - **The served set only.** It filters tips, at S or today, and own held rows, never a superseded
      version, which is history and never a hit (ADR-0025 d8): a supersession corrects a claim for
      every period, and a fact that ended is closed by a supersession setting `valid_to`, with a
      new claim for what followed. An open version passes every filter, served with `null` ends,
      so steering, which states no period (d2), is never filtered (ADR-0025 d8).
    - **Rank and budget.** It removes hits as `kinds` does, inside the scan before `k` and the
      budget; kept hits keep the total order, score descending then `artifact_id` ascending, so it
      never reorders (R3), and a removed row takes no place in `k` or the budget, whose object keeps
      every field's meaning, `hits_omitted` counting budget cuts only.
    - **The read log.** `query_digest` gains `as_of_seq` (resolved), `valid_at`, `valid_since` and
      `valid_until` (normalized), each `null` when absent; the row records the resolved seq
      (amends ADR-0025 d9).

- **d5 — A version's files over time are its file manifest, already pinned by its envelope.** Under
  ADR-0035 d2 a version names its files in its file manifest, its first attachment, bound by digest
  in the signed envelope, so any version an as-of read resolves names a fixed file set. The fetch
  names a version (ADR-0035 d3) and serves under today's readability, state and keys; an erased
  version's DEKs are destroyed and its objects removed (ADR-0035 d9), so no seq brings a file back.
  Two conditions from the investigation are obligations for ADR-0035's S1, not edits to it:
  objects are written with `PutMode::Create` alone, "returning Error::AlreadyExists if an object
  already exists at the provided path" (object_store 0.14.2, docs.rs, fetched 2026-10-31), so a
  pinned name never holds other bytes, and compare-and-set stays in Postgres, `LocalFileSystem`
  answering `NotImplemented` for `PutMode::Update` (`src/local.rs`); and `LocalFileSystem` is built
  `with_fsync(true)`, its fsync being "**disabled by default**" (same source), else a power cut can
  lose a file whose binding committed.

- **d6 — Agents read the same answers as MCP resources, read-only, with no subscription.**
    - **Capability** `"resources": {}`: [`/mcp`](../api/mcp-plugin.md#transport-streamable-http-stateless)
      is stateless with "no subscriptions", and "Both `subscribe` and `listChanged` are
      optional—servers can support neither, either, or both" (MCP 2025-06-18, server/resources,
      fetched 2026-10-31). List and read are request and response, carrying reads the tools already
      make (amends ADR-0014 d2, whose facade carried tools alone).
    - **Templates**: `oml://workarea/{workarea}/claim/{claim_key}{?version,as_of,as_of_seq,valid_at}`
      and `oml://workarea/{workarea}/claim/{claim_key}/file/{+path}{?version,as_of,as_of_seq}`.
      Under RFC 6570 an undefined variable is ignored, so the bare URI is today's tip; `{claim_key}`
      percent-encodes reserved characters, `/` included; `{+path}` keeps a path's slashes.
      `{workarea}` is the id, or the name once ADR-0026 lands; `version` is the ordinal (ADR-0025
      d18), and with `as_of` or `as_of_seq` is `request.validation`.
    - **Contents.** A claim is one `application/json` text, the claim item `oml_get` returns
      (ADR-0025 d12) with `as_of` and ADR-0035 d5's descriptors. A file is `text` for an inline
      type (ADR-0035 d4), base64 `blob` otherwise, only while `size_bytes` is within
      `submission.max_body_size`, the cap of ADR-0035 d5's inline get; past it the read is refused
      with `details.reason: "inline_cap"`, naming `oml attach get`.
    - **The list, bounded.** `resources/list` serves one page of the scoped listing at the key's
      token workarea, its cursor as MCP's, each entry a claim's bare URI with `name` its
      `claim_key` and `annotations.lastModified` its `promoted_at`; no files.
    - **Reads.** Each call authenticates as every `/mcp` call does (bearer or connection, ADR-0031),
      runs the HTTP read it mirrors (get, the listing, the fetch) under that read's rule and refusal
      order, and writes its read-log row. Not found and unreadable answer the same bytes: JSON-RPC
      `-32002`, "Resource not found", `data: {uri}`, the spec's code; any other refusal is `-32602`
      with OML's typed error object as `data`. A resource read is never a review serve (ADR-0004
      d5), and no method writes (R1).
    - **Unverified clients.** Claude Code "automatically provides tools to list and read MCP
      resources when servers support them" and lists them under `@`, naming no templates
      (code.claude.com/docs/en/mcp, fetched 2026-10-31); claude.ai connectors list "Text and binary
      resources" and not "Resource subscriptions" (claude.com/docs/connectors/building, fetched
      2026-10-31 by the investigation). Whether either app expands templates, reads a blob or caps a
      resource as Claude Code caps tool output, at 25,000 tokens by default, is unverified — [FR-47](../product/roadmap.md).

- **d7 — Terminals read through `oml fs`, read-only, under an agent's or a reviewer's key.**
  `oml fs ls|tree|cat|stat|get|export` take `--as-of`, `--seq` and `--valid-at` and paths
  `<workarea>/<claim_key>[/<file path>]`: `ls` lists claims or files, `tree` walks the nodes the key
  reads, `cat` prints a body or inline-type file and refuses a binary on a terminal (exit `2`),
  `stat` prints the claim item, `get` writes a file once its digest matches, as `oml attach get
  --out` does (ADR-0035 d5). Each is the HTTP read it names, journalled so; the seq goes to stderr.
    - **`export <dir>`** resolves one seq and pins every read to it by `as_of_seq`. It refuses a
      non-empty `<dir>`, writes each body and file under `<workarea>/<claim_key>/` to a temporary
      name renamed once its SHA-256 matches the descriptor, and writes `oml-snapshot.json`: seq,
      `at`, rule, and per file its path, `artifact_id`, version, digest, media type and valid
      interval. A mismatch stops it (exit `1`) and leaves nothing under that name. Two exports at
      one seq differ only by what was demoted, erased or put out of the key's reach between (d3).
    - **Keys.** `OML_URL` and `OML_API_KEY` alone, never the stored admin credentials, as
      [ADR-0029](ADR-0029-session-start-hook.md)'s hook; an admin key's content reads answer
      `access.verb_ceiling` (ADR-0026 d15), so `oml fs` reads no content under one.
    - **The residual.** Exported plaintext rests on the client, as after `oml attach get --out`,
      outside the deployment's control and ADR-0005's claim; the caches row names both (amends
      ADR-0005 d4). No subcommand writes to OML.

- **d8 — The lake may derive bitemporal tables; it never answers a read.** Bronze gains nothing, as
  it mirrors journal events, valid ends included (ADR-0003 d3); `silver.claims` gains both ends.
  Gold may add a table of tip and valid intervals per version for metadata analytics, derived from
  the outbox, read by no client path, never the authority: the journal wins (D2).

- **d9 — Time adds no way to see more or to write.** An as-of read never serves erased content or a
  demoted or rejected version, and a held one only to keys that read held versions today (R3, R4).
  A key cannot see a past it cannot read today; one granted since S sees the workarea as of S as it
  sees its superseded history. Valid time takes no version off a read surface and puts none back: it
  is stated under review, and a filter is a reader's choice. Every new parameter, resource method
  and `oml fs` command reads; valid time is written only through admission (R1) and judged by a
  non-author (R2); the interval fold changes no journal row.

- **d10 — The wire, one code and the vocabulary, held here until built.** `valid_from` and
  `valid_to` on `SubmitRequest`, `SupersedeRequest`, `oml_remember`, `oml_learn`, `oml_supersede`
  and every claim item; `as_of` and `as_of_seq` on get, the scoped listing, `RecallRequest`,
  `oml_get` and `oml_recall`; `valid_at`, `valid_since` and `valid_until` on both listings,
  `RecallRequest` and `oml_recall`; `tip_as_of`, `state_as_of` and `as_of` in as-of answers. Twelve
  tools still (amends ADR-0025 d12, d13); one code, `request.as_of_ahead` (`409`); and four rows
  for ADR-0018 d4 (amends it):

    | Term | Means | Never |
    |---|---|---|
    | transaction time | what OML knew and when: the journal's `seq`, with `at` informational | "system time" on OML pages; "history" as the axis's name |
    | valid time | the interval [`valid_from`, `valid_to`) in which a claim version says it holds in the world | "effective date", "expiry" (it schedules nothing) |
    | as-of read | a read at one resolved `seq` under rule `oml-asof/1`: four tests at that seq, all else today's | "time travel" for OML reads (Delta's term, bounded by VACUUM); "snapshot read" |
    | snapshot | the directory `oml fs export` writes at one seq, with `oml-snapshot.json` | "backup", "evidence export", "manifest" unqualified |

- **d11 — The build follows iteration 3 and runs beside ADR-0035, in dependency order.** As-of over
  claims needs no files, so S1 to S5 may precede ADR-0035's substrate; each step changes its
  contract pages with its code, and migrations follow iteration 3's and ADR-0035's.

    | Step | Lands | Check, tests named as sentences |
    |---|---|---|
    | S1 | d2: both ends on every submission path, the fold columns, the serve showing them | `a_valid_from_not_before_valid_to_is_refused`, `a_date_compares_as_utc_midnight_and_is_served_as_a_date`, `a_successor_that_omits_valid_time_is_open`, `steering_vocabulary_kinds_and_retirements_refuse_valid_time`, `an_event_written_before_valid_time_folds_as_open`, `the_review_serve_shows_both_ends_with_the_words` |
    | S2 | d3: `claim_intervals`, `at_watermark`, `as_of` and `as_of_seq` on get and the scoped listing, the echo, `request.as_of_ahead` | `the_interval_fold_equals_a_replay_of_the_journal_after_every_lifecycle_event`, `an_instant_resolves_to_the_longest_prefix_stamped_at_or_before_it`, `an_as_of_read_never_serves_a_version_demoted_since`, `an_as_of_read_never_serves_erased_content`, `an_as_of_read_applies_todays_grants_and_denials`, `a_version_rejected_since_s_is_never_served_as_of_s`, `a_held_version_at_s_reaches_only_its_author_and_d5_readers`, `a_follower_refuses_a_seq_it_has_not_applied` |
    | S3 | d3 on recall; d4 on recall and both listings; the digest members | `valid_at_removes_hits_and_never_reorders_the_rest`, `valid_at_never_removes_steering`, `a_filtered_row_takes_no_place_in_k_or_the_budget`, `recall_as_of_and_valid_at_answers_the_q3_example`, `the_request_digest_carries_the_resolved_seq_and_the_valid_filters` |
    | S4 | d6: the capability, templates, list, read, the error mapping | `initialize_declares_resources_without_subscribe_or_list_changed`, `a_missing_and_an_unreadable_claim_resource_answer_the_same_bytes`, `every_resource_read_writes_the_read_log_row_of_the_read_it_mirrors`, `resources_list_pages_the_scoped_listing`, `no_resources_method_writes` |
    | S5 | d7: `oml fs` | `fs_export_writes_a_file_only_after_its_digest_matches`, `fs_export_refuses_a_non_empty_directory`, `fs_under_an_admin_key_reads_no_content`, `two_exports_at_one_seq_differ_only_by_what_was_demoted_or_erased_between` in `cli_e2e.rs` |
    | S6 | d5 once ADR-0035's S2 lands; its S1 takes d5's two conditions | `an_as_of_read_resolves_the_file_set_its_version_pinned`, `no_seq_serves_a_file_of_an_erased_version`, `every_object_put_uses_create_and_none_overwrites`, `the_local_store_is_built_with_fsync`, `a_binary_file_resource_past_the_inline_cap_is_refused` |
    | S7 | FR-47's dated app tests, after S4 and S6 | a recorded Claude Code and claude.ai session reading a claim, a text file and a binary file through the templates, at an `as_of` too |

## Consequences

**One claim, end to end**, in workarea `finance`: `agent-a` a writer and reader key, `agent-c` a
reader key, `rev-1` and `rev-2` dreamer keys. Seqs are illustrative.

| # | When | Key | Call | Answer |
|---|---|---|---|---|
| 1 | 2026-07-03, 07-04 | `agent-a`, then `rev-1` | `oml_remember {claim_key: "q3-forecast", text: "The Q3 forecast is 12M.", valid_from: "2026-07-01", valid_to: "2026-10-01"}`, then an approving verdict | held `V1` at seq 1040, promoted at 1052: `held` [1040, 1052), `tip` [1052, —) |
| 2 | 2026-10-14, 10-15 | `agent-a`, then `rev-2` | `oml_supersede {artifact_id: V1, text: "The Q3 forecast is 12.4M.", valid_from: "2026-07-01", valid_to: "2026-10-01"}`, then an approving verdict | held `V2` at 2310; `lifecycle.supersede` at 2321: `V1` `tip` [1052, 2321), `V2` `tip` [2321, —) |
| 3 | today | `agent-c` | `oml_recall {query: "Q3 forecast", as_of: "2026-10-10T00:00:00Z", valid_at: "2026-08-01"}` | `as_of {seq: 2288, …, rule: "oml-asof/1"}`; `V1`, "12M", `tip_as_of: true`, `tip: false` |
| 4 | today | `agent-c` | the same without `as_of` | `V2`, "12.4M" |
| 5 | today | `agent-c` | step 3 with `valid_at: "2026-11-01"` | no hit: `V1`'s interval ends 2026-10-01 |
| 6 | today | `agent-c`, `agent-a` | `as_of: "2026-07-03T12:00:00Z"`, `include_own_held: true` | `agent-c`: no hit; `agent-a`: `V1`, `state_as_of: "held"`, `state: "promoted"` |
| 7 | today | `agent-c` | `resources/read` of `oml://workarea/finance/claim/q3-forecast?as_of=2026-10-10T00%3A00%3A00Z&valid_at=2026-08-01`; `oml fs export q3 --as-of 2026-10-10T00:00:00Z` | step 3's item as JSON; a snapshot at seq 2288 |
| 8 | after an admin demotes `V1` | `agent-c` | step 3 again | no hit; `V2` does not stand in, not being the tip at 2288 |

**Costs, stated.** Every lifecycle act writes an interval row inside the chain lock's statement,
every journal row carries a watermark, and an as-of recall joins intervals in its scan, all
unmeasured (FR-48). An as-of answer is the past seen through today, not a replay: a grant revoked,
a version demoted or a subtree marked since S changes what S shows, scores are today's, and only the
read log says what a key was served at S. An ended fact takes a closing supersession and a new
claim. Valid ends survive erasure (d2); exported plaintext is outside the erasure claim (d7).

**Opened.** [FR-46](../product/roadmap.md), time, closes when S1 to S6 pass and the table above runs
in continuous integration; FR-47 and FR-48 as above. New surface: d10's fields and code,
`claim_intervals`, `journal.at_watermark` and two `memories` columns. No contract page changes
now, so `info.version` stays `0.2.0-design`.

**Process obligations.** With the proposal: this file, a [register](index.md) row, a `mkdocs.yml` nav
entry, `llms.txt`, FR-46 to FR-48 marked proposed, one [changelog](../changelog.md) entry. On
acceptance, not before: this record and its row become `accepted design` with the date; ADR-0005,
ADR-0014, ADR-0018 and ADR-0025 gain "· amended by ADR-0036" on their Status lines and frontmatter,
their text unedited; the [erasure inventory](../architecture/audit-and-lineage.md) names d2's ends
and d7's export; [retrieval](../architecture/retrieval.md) declares d3's rule, d4's filter and the
digest members; storage the watermark; the event catalog, Postgres schema, HTTP API (`since` and
`until` as transaction time), errors, `openapi.yaml`, `mcp-tools.json`, the agent plugin and the
CLI reference take d10; FR-25's exit criterion names the as-of recall and the interval write; the
D4, D12, D13 and D14 cells name this record; the spec tag is cut.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — An own file system or storage engine** (the user's decision 1) | It adds no time, which sits above the bytes in all nine surveyed systems, and brings crash-consistency risk: bug patches are "nearly 40% of patches" of Linux file systems and "Data corruption is the most predominant consequence (40%)" (Lu et al., FAST 2013), and ext4 took from "the first experimental version" in 2.6.19 (November 2006) to "a stable filesystem" in 2.6.28 (December 2008) (kernelnewbies.org); each fetched 2026-10-31 by the investigation. Its disks would join ADR-0005's inventory as new copy classes. |
| **d1 — A Git-, Dolt- or lakeFS-style versioned store** | A second history beside the signed journal, and erasure becomes a rewrite and a garbage collection; on GitHub, "To remove Git LFS objects from a repository, delete and recreate the repository" (docs.github.com, fetched 2026-10-31 by the investigation). |
| **d1 — `as_of` in `simulated_time`, or valid time defaulted from it** | Lends the journal's clock to fiction (ADR-0011 d7), and a real claim would carry a scenario's dates. |
| **d2 — Valid time inside the encrypted body** | Every filter would decrypt, rows a budget cuts included (ADR-0025 d7), and the keyless reconciler could not rebuild the fold. |
| **d2 — An interval inherited from a predecessor or computed from lineage** | ADR-0025 d11 lets a successor inherit `kind` alone; a computed interval is a claim nobody stated or reviewed. |
| **d2 — Zone-local dates, or instants only** | A date without a zone names a different instant per reader; instants only force a false precision on "Q3". |
| **d2 — Valid time as embargo or expiry** | It would move versions on and off read surfaces by clock, a visibility act outside review; retention is [FR-26](../product/roadmap.md)'s. |
| **d3 — The grants a key held at S** | A revoked key's past would stay readable, and a denial issued for cause could be stepped around by naming an earlier seq. |
| **d3 — Serving the tip at S though it is demoted or erased today, or the tip before it** | Demotion and erasure are journalled acts on what is served now (R4); an earlier version was not what OML held at S. |
| **d3 — Replaying the journal fold per read** | Cost grows with the tenant's history on every read; the replay stays the rebuild and the reconciler's check. |
| **d3 — The last seq stamped at or before the instant, with no watermark** | A regressed `at` would let an event stamped after the instant into the answer. |
| **d4 — Every promoted version whose interval contains t, the latest winning** (per-period rows, as SQL:2011 tables keep) | It makes superseded versions hits again, against ADR-0025 d8, and a supersession narrowing an interval would leave the old version answering for the dropped period, a reading no reviewer ratified. |
| **d4 — Absent valid time read as unknown and excluded** | Every claim with no period, all earlier claims among them, would leave every filtered read. |
| **d5 — A second pinned object per version** (the investigation's T2) | ADR-0035 d2's file manifest, bound by digest in the envelope, already is one. |
| **d6 — Subscriptions or list-change notifications** | Need a server-to-client stream `/mcp` lacks (ADR-0014 d2), and claude.ai connectors do not support subscriptions. |
| **d6 — Templates only, or every claim in one list** | The first leaves clients that present listed resources, as Claude Code's `@` menu does, nothing; the second is unbounded. |
| **d7 — A read-only FUSE mount now** | fuser's README says "Pull requests are no longer being accepted" and lists "macOS (untested)" (github.com/cberner/fuser), and macFUSE bundled with commercial software is "not allowed without specific prior written permission" (github.com/macfuse/macfuse); fetched 2026-10-31 by the investigation. Deferred until a maintained mount crate and a named user of one. |
| **d7 — WebDAV** | An authoring protocol (RFC 4918) on the API origin, most of whose methods would be refused, and its usual mount, rclone, keeps plaintext on the client: "File caching is necessary to make the VFS layer appear compatible with a normal file system" (rclone.org, fetched 2026-10-31 by the investigation). Rejected. |
| **d3, d8 — Delta time travel or a bitemporal gold table as the source** | The lake is derived and serves no read (ADR-0003 d1); time travel lags, is batch-grained, holds no held body and ends at VACUUM (d3). |
