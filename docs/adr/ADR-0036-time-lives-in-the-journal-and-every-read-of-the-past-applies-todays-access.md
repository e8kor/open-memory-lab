<!--
id: adr/ADR-0036-time-lives-in-the-journal-and-every-read-of-the-past-applies-todays-access
tier: engineering
status: proposed
answers: which clocks OML keeps and why they stay apart; how a claim version states when it is true in the world, who judges it and how an erase shreds it; how a read asks what OML knew at a seq or an instant, and what today's access and state change in that answer; how valid_at and valid ranges filter recall and listings without ranking; how a version's files resolve over time; how agents read the same answers as MCP resources and terminals as oml fs; what the lake may derive; why no file system or storage engine is built; the build order
-->

# ADR-0036 — Time lives in the journal: a claim version may state when it is true, a read may ask what OML knew at a seq or an instant, and every read of the past applies today's access

**Status:** proposed · 2026-10-31

**Amends:** ADR-0005 (d4), ADR-0014 (d2), ADR-0018 (d4), ADR-0025 (d8, d9, d12, d13)

## Context

Agents ask two questions OML cannot answer. "What did we hold on 10 October?" needs a read of the
past, and none takes a point in time (the pages' only as-of is ADR-0020's dataset as-of). "What was
true in Q3?" needs a claim that says when it holds; the recall example at
[openapi.yaml](../api/openapi.yaml):82 says "valid until 2026-11-15" in words no read can filter on.

**The user's decisions** (2026-10-31), fixed: (1) no file system or storage engine of OML's own:
time and claims live in the journal and an index above write-once objects; (2) bitemporal:
transaction time is what OML knew, and when, and valid time is new; (3) an as-of read applies
today's grants, erased content stays erased, demoted and rejected versions follow ADR-0030's
held-or-promoted rule, and nothing is served that review has not let that key see today; (4) MCP
resources for agents and `oml fs` for terminals, FUSE and WebDAV rejected or deferred; (5) time, not
scale.

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
(in-progress workareas), ADR-0031 (connections), ADR-0034 (role split), ADR-0035 (deliverables).
Each decision is the maintainer's reading, which the user confirms before acceptance. Proposed, not
built.

## Decision

- **d1 — Two axes, named, and `simulated_time` stays a third clock neither reads.** *Transaction
  time* is what OML knew, and when: the journal's `seq`, the authority, and `at`, informational.
  *Valid time* is when a claim version says it holds in the world (d2). An as-of read (d3) moves
  along the first only, `valid_at` (d4) reads the second only, and neither reads `simulated_time`,
  so the journal's clock stays "never lent to fiction" ([ADR-0011](ADR-0011-simulations.md) d7): a
  scenario wanting the world at its fictional now sends that as `valid_at`. The listing's `since`
  and `until` are transaction time: the version's submission `at`.

- **d2 — Valid time is two optional ends of the signed submission, bound by the digest of a form
  sealed under the version's DEK, immutable per version.**
    - **Shape.** `valid_from` and `valid_to` ride submit (remember, learn), supersede and a
      simulation write: an RFC 3339 `full-date` or a `date-time` with an offset, the half-open
      [from, to), an absent end open. Both present need from < to, else `request.validation` with
      `details.field`. A date compares as 00:00:00Z that day and stays a date in the signed form, so
      its precision survives; an instant is normalized to UTC with six fractional digits, as `at` is
      ([chain format](../data/chain-format.md)), and a second 60 or a seventh digit is refused.
    - **The envelope.** `submission.remember`, `submission.learn` and `simulation.write` carry
      `valid_digest`, the SHA-256 of `valid_sealed`, stored as `memories.valid_sealed`: the RFC 8785
      form of `{valid_from, valid_to}`, each the signed string or `null`, padded to one length and
      sealed before `BEGIN` ([ADR-0024](ADR-0024-performance-foundation.md) d1) under the version's
      body DEK with a random 96-bit nonce of its own and associated data binding `artifact_id` and
      the purpose `valid`. The journal, the outbox and bronze stay digest-only
      ([ADR-0004](ADR-0004-bodies-and-attachments.md) d1), and the keyless reconciler checks the
      stored bytes against the digest. Every submission of a kind that takes it carries one; an
      earlier event carries none and folds as open (ADR-0004 d2's additive rule). At capture the
      lead folds each end, as signed and as an instant, to `memories.valid_from` and `valid_to`,
      which only filter: every read that serves the ends, the review serve among them, serves those
      it decrypts from `valid_sealed`, and refuses with `system.internal`, logged as
      `state-mismatch` ([ADR-0017](ADR-0017-reconciliation.md) d4), where they differ from the
      columns.
    - **Immutable, never inherited.** An interval changes only by a reviewed supersession, and a
      successor inherits none ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d11 inherits
      `kind` alone), so a revision re-sends it; the server computes none from `derives_from`.
    - **Kinds.** `memory`, `derived_fact` and ADR-0035's `deliverable` take it; `steering`, which
      applies "in every recall of its scope" (ADR-0018 d4), the vocabulary kinds, which deprecate
      ([ADR-0021](ADR-0021-ai-human-ontologies.md) d3), and a retirement refuse it
      (`request.validation`).
    - **Review.** The serve shows both ends with the words and the verdict covers them (ADR-0004
      d5): a right figure under a wrong interval is a wrong claim. Valid time schedules nothing: a
      version valid from 2027 is served today to every read that does not filter.
    - **Erasure.** The ends are content and shred with the body; no read serves an erased version's
      ends. The erase nulls the two columns in its own transaction; the journal, the outbox and the
      lake hold only the digest; base backups and WAL keep the columns until the backup window
      closes, as they keep `tsvector` entries
      ([erasure inventory](../architecture/audit-and-lineage.md#the-erasure-inventory)). The cost:
      the keyless reconciler can neither check nor rebuild the columns, which the serve checks.

- **d3 — The as-of read resolves one seq and answers with today's access and state, rule
  `oml-asof/1`.**
    - **Parameters.** `as_of`, an RFC 3339 instant, or `as_of_seq`, a journal `seq`, never both
      (`request.validation`), on get, the scoped listing, recall, `oml_get`, `oml_recall`, the
      resources (d6) and `oml fs` (d7).
    - **Resolution.** `as_of` names the last seq of the longest chain prefix whose every `at` is at
      or before the instant, since `at` may regress. The lead keeps the running maximum of `at` in
      `journal_chain_head.at_watermark`, read with the head in round trip one, and writes it per seq
      to `journal_watermarks` (`tenant_id`, `seq`, `at_watermark`), indexed on (`tenant_id`,
      `at_watermark`), in round trip two's statement (ADR-0024 d1 unchanged); one probe for the
      first seq whose watermark exceeds the instant resolves it. An answer echoes what was asked,
      `as_of {at, rule: "oml-asof/1"}` with the instant normalized as `at` is, or
      `as_of {seq, rule}`; no answer serves the seq an instant resolves to.
    - **No future.** A seq past the applied head, and an instant the applied watermark does not
      strictly exceed, answer `request.as_of_ahead` (`409`, `{applied_seq}`), on the lead and a
      follower ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d13) alike, so an answered
      instant resolves to one seq forever: its prefix ends before an applied event stamped after it.
      The cost: in a quiet tenant a recent instant waits for the next event, `as_of_seq` at the
      tail's `head_seq` reading it meanwhile, and the refusal's edge tells a key its tenant's newest
      `at`, a time, never content. A follower never waits or forwards, and own held rows route to
      the lead (ADR-0024 d9).
    - **The rule.** A read at S serves exactly what the same read serves the key today, except that
      four tests are taken at S: whether a version had been submitted, was held, was promoted, and
      which version was a claim's served tip, default or the key's audience overlay
      ([ADR-0011](ADR-0011-simulations.md) d4). All else is today's: the key's role, grants and
      denials (ADR-0025 d1), ADR-0030's boundary and held readers on today's tree and marks, the
      audience, the `self_signed` scope, the version's placement after any reassignment, and its
      state. Tip moves (supersession, retirement, tip override) are history, read at S; demotion,
      rejection, re-entry to held, erasure, a revoked grant, a denial and a mark are today's state,
      and it wins.
    - **What winning means.** A version promoted at T and demoted later is, for every S ≥ T, the tip
      of nothing, no earlier version standing in; get answers as for a demoted version today, and a
      return to service returns it to as-of reads. An erased version's content is served at no S;
      get answers `410 lifecycle.erased` to the keys that could read it before. A version held at S,
      or held today, reaches only the keys today's reads serve held rows to (its author, ADR-0030
      d5's readers inside); recall and the listing serve one held at S only while it is held or
      promoted today, so a non-author outside sees as of S only what was promoted by S and is
      promoted today. Get at S of a version not submitted by S answers `access.not_found`. Items
      gain `tip_as_of` and `state_as_of`, `held` or `promoted`.
    - **Computation: an interval fold, not a replay per read.** `claim_intervals` (`tenant_id`,
      `artifact_id`, `kind` ∈ `held` | `tip`, `simulation_id` null for the default tip, `from_seq`,
      `to_seq` null while open) takes rows from submissions and tip moves only. A submission opens
      the version's `held` row and its first promotion, by `lifecycle.promote` or `supersede`,
      closes it, so `state_as_of` is `held` while that row is open at S and `promoted` after. A
      promote, `supersede` or `tip_override` that moves a tip closes the old tip's row and opens the
      new one's unless it is open already. Rejection, reopening, demotion, erasure and reassignment
      write none: today's state decides them. Each act writes its rows in round trip two's
      statement. The table is fenced per tenant as `memories` is, and partitioned as ADR-0034 d11
      partitions `memories` once that lands; it and `journal_watermarks` rebuild from the journal,
      and a disagreeing row is a `state-mismatch` finding. Recall tests intervals in the statement
      that applies `LIMIT`, over indexes that hold every promoted version (ADR-0025 d8). Scores are
      today's: what was known at S, ranked as the index ranks today, not the recall made at S, which
      the [read log](../architecture/retrieval.md#the-read-log) holds. Cost unmeasured —
      [FR-49](../product/roadmap.md).

- **d4 — `valid_at` and valid ranges filter versions and never rank them.** `valid_at`, a date or an
  instant, keeps a version when (`valid_from` null or ≤ t) and (`valid_to` null or t < it);
  `valid_since` and `valid_until`, either optional, keep one whose interval overlaps [since, until);
  `valid_at` with either is `request.validation`. Recall, both listings and `oml fs` take them; with
  `as_of`, d3 first decides what the key reads at S, then the filter keeps what was true at t.
    - **The served set only.** It filters tips, at S or today, and own held rows, never a superseded
      version, which is never a hit (ADR-0025 d8): a supersession corrects a claim for every period,
      and a fact that ended is closed by a supersession setting `valid_to`. An open version passes
      every filter, so steering, which states no period, is never filtered.
    - **Rank and budget.** It removes hits as `kinds` does, inside the scan before `k` and the
      budget, so kept hits keep the total order (R3) and a removed row takes no place in `k` or the
      budget, `hits_omitted` counting budget cuts only.
    - **The read log.** `query_digest`, which the answer carries, gains `as_of` and `as_of_seq` as
      asked and `valid_at`, `valid_since` and `valid_until`, each normalized and `null` when absent;
      the row records the resolved seq beside it (amends ADR-0025 d9).

- **d5 — A version's files over time are its file manifest, already pinned by its envelope.** Under
  ADR-0035 d2 a version's file manifest, bound by digest in the signed envelope, fixes the file set
  of any version an as-of read resolves, and the digest check on every serve (ADR-0004 d4) refuses
  other bytes. The fetch names a version (ADR-0035 d3) and serves under today's readability, state
  and keys; an erased version's DEKs are destroyed and its objects removed (ADR-0035 d9), so no seq
  brings a file back. One condition from the investigation binds ADR-0035's S1 without editing it: a
  put or copy on `file://` returns only once the file and its directory entry are durable, for the
  crate's local store leaves fsync "**disabled by default**" (object_store 0.14.2, `src/local.rs`,
  fetched 2026-10-31 by the investigation); the mechanism is S1's.

- **d6 — Agents read the same answers as MCP resources, read-only, with no subscription.**
    - **Capability** `"resources": {}`:
      [`/mcp`](../api/mcp-plugin.md#transport-streamable-http-stateless) is stateless with "no
      subscriptions", and "Both `subscribe` and `listChanged` are optional—servers can support
      neither, either, or both" (MCP 2025-06-18, server/resources, fetched 2026-10-31). List and
      read are request and response, carrying reads the tools already make (amends ADR-0014 d2,
      whose facade carried tools alone).
    - **Templates**: `oml://workarea/{workarea}/claim/{claim_key}{?as_of,as_of_seq}` and
      `oml://workarea/{workarea}/claim/{claim_key}/file{/path*}{?as_of,as_of_seq}`. Under RFC 6570
      an undefined variable is ignored, so the bare URI is today's tip; `{claim_key}`
      percent-encodes reserved characters, `/` included, and `{/path*}` takes the file path as its
      list of segments, each so encoded and joined by `/`, so a `?`, `#` or `%` in a name stays in
      its segment. `{workarea}` is the id, or the name once ADR-0026 lands. A past tip is read by
      `as_of`, any other version by `oml_get`.
    - **Contents.** A claim is one `application/json` text, the claim item `oml_get` returns
      (ADR-0025 d12) with `as_of`, and from S6 ADR-0035 d5's descriptors. A file is `text` for an
      inline type (ADR-0035 d4), base64 `blob` otherwise, only while `size_bytes` is within
      `submission.max_body_size`, the cap of ADR-0035 d5's inline get; past it the read is refused
      with `details.reason: "inline_cap"`, naming `oml attach get`.
    - **The list, bounded.** `resources/list` serves one page of the scoped listing at the key's
      token workarea, its cursor as MCP's, each entry a claim's bare URI with `name` its `claim_key`
      and `annotations.lastModified` its `promoted_at`; no files.
    - **Reads.** Each call authenticates as every `/mcp` call does (bearer or connection, ADR-0031).
      A claim resolves by the scoped listing at scope node `{workarea}`, as `X-OML-Workarea` sets it
      within the key's grants ([ADR-0022](ADR-0022-two-ring-api.md) d4), so ADR-0030 d4 decides
      there, with `workarea_id` `{workarea}`, `claim_key` and the `as_of` or `as_of_seq` asked; its
      item in that workarea is the tip, then get of that item at the same point; a file adds the
      fetch (ADR-0035 d3). Each read keeps its rule and refusal order and writes its own read-log
      row. Not found, unreadable and a listing that names no item answer the same bytes: JSON-RPC
      `-32002`, "Resource not found", `data: {uri}`, the spec's code; any other refusal is `-32602`
      with OML's typed error object as `data`. A resource read is never a review serve (ADR-0004
      d5); no method writes (R1).
    - **Unverified clients.** Claude Code "automatically provides tools to list and read MCP
      resources when servers support them" and lists them under `@`, naming no templates
      (code.claude.com/docs/en/mcp); claude.ai connectors list "Text and binary resources" and not
      "Resource subscriptions" (claude.com/docs/connectors/building); both fetched 2026-10-31.
      Whether either app expands templates, reads a blob or caps a resource as Claude Code caps tool
      output, at 25,000 tokens by default, is unverified — [FR-48](../product/roadmap.md).

- **d7 — Terminals read through `oml fs`, read-only, under an agent's or a reviewer's key.**
  `oml fs ls|tree|cat|stat|get|export` take `--as-of` and `--seq`, and `ls`, `tree` and `export`
  `--valid-at`. A path is `<workarea>/<claim_key>[/<file path>]`, the `claim_key` one
  percent-encoded segment as iteration 3's CLI sends a reference (ADR-0026 d2, d3), refused (exit
  `2`) when empty, `.` or `..`. `ls` lists claims or files, `tree` the nodes the key reads, `cat`
  prints a body or inline-type file and refuses a binary on a terminal (exit `2`), `stat` the claim
  item, and `get` writes a file once its digest matches, as `oml attach get --out` does (ADR-0035
  d5). A claim resolves as d6's does, at scope node `<workarea>`; each read is journalled as its
  HTTP read is, and the as-of echo goes to stderr.
    - **`export <workarea> <dir>`** reads at one `--as-of` or `--seq`, else at the tail's
      `head_seq`, walking the scoped listing at `<workarea>` before it writes, and refuses a
      non-empty `<dir>` and two `claim_key` segments of one workarea equal ignoring case (exit `2`).
      It writes `<workarea>/<claim_key>/body.txt` and the files under
      `<workarea>/<claim_key>/files/`, `<workarea>` its name once ADR-0026 lands and else its id,
      each renamed from a temporary name once written, a file only once its SHA-256 matches its
      descriptor, and `oml-export.json`: the as-of echo, per claim its `claim_key`, `artifact_id`,
      version and valid interval, per file its path, digest and media type. A mismatch stops it
      (exit `1`) and leaves nothing under that name. Two exports at one point differ only by what
      today's state and access changed between (d3).
    - **Keys.** `OML_URL` and `OML_API_KEY` alone, never the stored admin credentials, as
      [ADR-0029](ADR-0029-session-start-hook.md)'s hook; an admin key's content reads answer
      `access.verb_ceiling` (ADR-0026 d15), so `oml fs` reads no content under one.
    - **The residual.** Exported plaintext rests on the client, as after `oml attach get --out`,
      outside the deployment's control and ADR-0005's claim; the caches row names both (amends
      ADR-0005 d4). No subcommand writes to OML.

- **d8 — The lake may derive tip intervals; it never answers a read.** Gold may add tip intervals
  per version from the outbox, read by no client path, never the authority: the journal wins (D2).

- **d9 — Time adds no way to see more or to write.** A key sees no past it cannot read today (d3;
  R3, R4). Every new parameter, resource method and `oml fs` command reads; valid time is written
  only through admission (R1), judged by a non-author (R2), and moves no version on or off a read
  surface; no fold changes a journal row.

- **d10 — The wire, one code and the vocabulary, held here until built.** `valid_from` and
  `valid_to` on `SubmitRequest`, `SupersedeRequest`, `oml_remember`, `oml_learn`, `oml_supersede`
  and every claim item; `as_of` and `as_of_seq` on get, the scoped listing, `RecallRequest`,
  `oml_get` and `oml_recall`; `valid_at`, `valid_since` and `valid_until` on both listings,
  `RecallRequest` and `oml_recall`; `tip_as_of`, `state_as_of` and `as_of` in as-of answers. Twelve
  tools still (amends ADR-0025 d12, d13); one code, `request.as_of_ahead` (`409`); and three rows
  for ADR-0018 d4 (amends it):

    | Term | Means | Never |
    |---|---|---|
    | transaction time | what OML knew and when: the journal's `seq`, with `at` informational | "history" as the axis's name |
    | valid time | the interval [`valid_from`, `valid_to`) in which a claim version says it holds in the world | "effective date" |
    | as-of read | a read at one resolved `seq` under rule `oml-asof/1`: four tests at that seq, all else today's | "time travel" for OML reads (Delta's term, bounded by VACUUM); "snapshot read" |

- **d11 — The build follows iteration 3 and runs beside ADR-0035, in dependency order.** S1 to S5
  serve claims and bodies only and may precede ADR-0035's substrate; each step changes its contract
  pages with its code, and migrations follow iteration 3's and ADR-0035's. d3's boundary and held
  readers land with or after ADR-0030's d4 and d5; until then a version held at S reaches its author
  alone.

    | Step | Lands | Check, tests named as sentences |
    |---|---|---|
    | S1 | d2: both ends sealed on every submission path, the fold columns, the serve showing them | `a_valid_from_not_before_valid_to_is_refused`, `a_date_compares_as_utc_midnight_and_is_served_as_a_date`, `a_successor_that_omits_valid_time_is_open`, `steering_vocabulary_kinds_and_retirements_refuse_valid_time`, `an_event_written_before_valid_time_folds_as_open`, `the_journal_and_outbox_carry_only_the_sealed_ends_digest`, `the_review_serve_shows_both_ends_with_the_words`, `a_served_end_that_differs_from_its_sealed_form_is_refused` |
    | S2 | d3: `claim_intervals`, the watermark, `as_of` and `as_of_seq` on get and the scoped listing, the echo, `request.as_of_ahead` | `the_interval_and_watermark_folds_equal_a_replay_of_the_journal_after_every_lifecycle_event`, `an_instant_resolves_to_the_longest_prefix_stamped_at_or_before_it`, `an_instant_is_echoed_and_its_seq_is_served_by_no_answer`, `the_resolver_refuses_a_seq_or_instant_its_applied_head_has_not_passed`, `an_as_of_read_never_serves_a_version_demoted_since`, `a_version_demoted_and_returned_to_service_is_served_as_of_every_s_after_its_promotion`, `an_as_of_read_never_serves_erased_content`, `an_as_of_read_applies_todays_grants_and_denials`, `a_version_rejected_since_s_is_never_served_as_of_s`, `a_get_at_s_of_a_version_not_yet_submitted_is_not_found`, `a_held_version_at_s_reaches_only_its_author_and_d5_readers` (with ADR-0030), `a_version_reassigned_back_to_held_is_served_as_of_s_to_no_other_key` |
    | S3 | d3 on recall; d4 on recall and both listings; the digest members | `valid_at_removes_hits_and_never_reorders_the_rest`, `valid_at_never_removes_steering`, `a_filtered_row_takes_no_place_in_k_or_the_budget`, `recall_as_of_and_valid_at_answers_the_q3_example`, `the_request_digest_carries_the_as_of_asked_and_the_log_row_the_resolved_seq` |
    | S4 | d6 over claims: the capability, the claim template, list, read, the error mapping | `initialize_declares_resources_without_subscribe_or_list_changed`, `a_claim_resource_resolves_at_its_own_workarea_as_scope_node`, `a_missing_and_an_unreadable_claim_resource_answer_the_same_bytes`, `every_resource_read_writes_the_read_log_rows_of_the_reads_it_runs`, `resources_list_pages_the_scoped_listing`, `no_resources_method_writes` |
    | S5 | d7 over claims and bodies: `oml fs` but `get` | `fs_export_refuses_a_non_empty_directory`, `fs_export_writes_each_claim_key_as_one_segment_and_refuses_dot_segments_and_case_twins`, `fs_under_an_admin_key_reads_no_content`, `two_exports_at_one_point_differ_only_by_what_todays_state_and_access_changed` in `cli_e2e.rs` |
    | S6 | d5 once ADR-0035's S2 lands, its S1 taking d5's condition; d6's file template and descriptors; `oml fs get` and export's files | `an_as_of_read_resolves_the_file_set_its_version_pinned`, `no_seq_serves_a_file_of_an_erased_version`, `a_local_put_is_durable_before_it_returns`, `a_file_name_with_a_question_mark_hash_or_percent_round_trips_through_its_uri`, `a_binary_file_resource_past_the_inline_cap_is_refused`, `fs_export_writes_a_file_only_after_its_digest_matches` |
    | S7 | FR-48's dated app tests, after S4 and S6 | a recorded Claude Code and claude.ai session reading a claim, a text file and a binary file through the templates, at an `as_of` too |

## Consequences

**One claim, end to end**, in workarea `finance`: `agent-a` a writer and reader key, `agent-c` a
reader key, `rev-1` and `rev-2` dreamer keys. Seqs are illustrative.

| # | When | Key | Call | Answer |
|---|---|---|---|---|
| 1 | 2026-07-03, 07-04 | `agent-a`, then `rev-1` | `oml_remember {claim_key: "q3-forecast", text: "The Q3 forecast is 12M.", valid_from: "2026-07-01", valid_to: "2026-10-01"}`, then an approving verdict | held `V1` at seq 1040, promoted at 1052: `held` [1040, 1052), `tip` [1052, —) |
| 2 | 2026-10-14, 10-15 | `agent-a`, then `rev-2` | `oml_supersede {artifact_id: V1, text: "The Q3 forecast is 12.4M.", valid_from: "2026-07-01", valid_to: "2026-10-01"}`, then an approving verdict | held `V2` at 2310; `lifecycle.supersede` at 2321: `V1` `tip` [1052, 2321), `V2` `held` [2310, 2321), `tip` [2321, —) |
| 3 | today | `agent-c` | `oml_recall {query: "Q3 forecast", as_of: "2026-10-10T00:00:00Z", valid_at: "2026-08-01"}` | `as_of {at: "2026-10-10T00:00:00.000000Z", rule: "oml-asof/1"}`; `V1`, "12M", `tip_as_of: true`, `tip: false` |
| 4 | today | `agent-c` | the same without `as_of` | `V2`, "12.4M" |
| 5 | today | `agent-c` | step 3 with `valid_at: "2026-11-01"` | no hit: `V1`'s interval ends 2026-10-01 |
| 6 | today | `agent-c`, `agent-a` | `as_of: "2026-07-03T12:00:00Z"`, `include_own_held: true` | `agent-c`: no hit; `agent-a`: `V1`, `state_as_of: "held"`, `state: "promoted"` |
| 7 | today | `agent-c` | `resources/read` of `oml://workarea/finance/claim/q3-forecast?as_of=2026-10-10T00%3A00%3A00Z`; `oml fs export finance q3 --as-of 2026-10-10T00:00:00Z` | `V1`'s claim item as JSON; `q3/finance/q3-forecast/body.txt` and `q3/oml-export.json`, each echoing the instant |
| 8 | after an admin demotes `V1` | `agent-c` | step 3 again | no hit; `V2` does not stand in, not being the tip on 10 October |

**Costs, stated.** Every submission and tip move writes an interval row inside the chain lock's
statement, every journal row a watermark row, and an as-of recall joins intervals in its scan, all
unmeasured (FR-49). An as-of answer is the past seen through today, not a replay (d3). The keyless
reconciler cannot check the valid columns (d2); the edge of `request.as_of_ahead` tells a key its
tenant's newest `at` (d3); exported plaintext is outside the erasure claim (d7).

**Opened.** [FR-47](../product/roadmap.md), time, closes when S1 to S6 pass and the table above runs
in continuous integration; FR-48 and FR-49 as above. New surface: d10's fields and code,
`valid_digest`, `claim_intervals`, `journal_watermarks`, `journal_chain_head.at_watermark` and
`memories.valid_sealed`, `valid_from` and `valid_to`. No contract page changes now, so
`info.version` stays `0.2.0-design`.

**Process obligations.** With the proposal: this file, a [register](index.md) row, a `mkdocs.yml`
nav entry, `llms.txt`, FR-47 to FR-49 marked proposed, one [changelog](../changelog.md) entry. On
acceptance, not before: this record and its row become `accepted design` with the date; ADR-0005,
ADR-0014, ADR-0018 and ADR-0025 gain "· amended by ADR-0036" on their Status lines and frontmatter,
their text unedited; the [erasure inventory](../architecture/audit-and-lineage.md) names d2's
columns and sealed form and d7's export; FR-31's row names the newest `at` that
`request.as_of_ahead`'s edge shows (d3); ADR-0030's `P-R3-BOUNDARY` leaves out reads at an
`as_of_seq`, the rows an instant reaches where `at` regresses, and `request.as_of_ahead`, as it
leaves out the tail's `seq`; [retrieval](../architecture/retrieval.md) declares d3's rule, d4's
filter and the digest members; storage the watermark; the event catalog, Postgres schema, HTTP API
(`since` and `until` as transaction time), errors, `openapi.yaml`, `mcp-tools.json`, the agent
plugin and the CLI reference take d10 and the surface above; FR-25's exit criterion names the as-of
recall and the interval write; the D4, D12, D13 and D14 cells name this record; the spec tag is cut.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — An own file system or storage engine** (the user's decision 1) | It adds no time, which sits above the bytes in all nine surveyed systems, and brings crash-consistency risk: bug patches are "nearly 40% of patches" of Linux file systems and "Data corruption is the most predominant consequence (40%)" (Lu et al., FAST 2013), and ext4 took from "the first experimental version" in 2.6.19 (November 2006) to "a stable filesystem" in 2.6.28 (December 2008) (kernelnewbies.org); each fetched 2026-10-31 by the investigation. Its disks would join ADR-0005's inventory as new copy classes. |
| **d1 — A Git-, Dolt- or lakeFS-style versioned store** | A second history beside the signed journal, and erasure becomes a rewrite and a garbage collection; on GitHub, "To remove Git LFS objects from a repository, delete and recreate the repository" (docs.github.com, fetched 2026-10-31 by the investigation). |
| **d2 — Valid ends in clear in the signed envelope** | They would outlive every erase in the journal, the outbox and bronze, against the user's decision 3 and ADR-0005 d5's "Signed identifying metadata must be minimized or individually erasable, never assumed non-personal": a hospital stay's dates are personal. |
| **d2 — The sealed ends inline in the event payload** | The journal, the outbox and bronze would carry content, against the digest-only rule of ADR-0002 d5, ADR-0003 d3, ADR-0004 d1 and ADR-0005 d4's inventory. |
| **d2 — An interval inherited from a predecessor or computed from lineage** | ADR-0025 d11 lets a successor inherit `kind` alone; a computed interval is a claim nobody stated or reviewed. |
| **d2 — Zone-local dates, or instants only** | A date without a zone names a different instant per reader; instants only force a false precision on "Q3". |
| **d2 — Valid time as embargo or expiry** | It would move versions on and off read surfaces by clock, a visibility act outside review; retention is [FR-26](../product/roadmap.md)'s. |
| **d3 — The grants a key held at S** | A revoked key's past would stay readable, and a denial issued for cause could be stepped around by naming an earlier seq. |
| **d3 — Serving the tip at S though it is demoted or erased today, or the tip before it** | Demotion and erasure are journalled acts on what is served now (R4); an earlier version was not what OML held at S. |
| **d3 — Echoing the seq an instant resolves to** | Bisecting `as_of` would tell a key, to the microsecond, when each event it cannot read committed, and no reader needs it: an answered instant resolves to one seq forever. |
| **d4 — Every promoted version whose interval contains t, the latest winning** (per-period rows, as SQL:2011 tables keep) | It makes superseded versions hits again, against ADR-0025 d8, and a supersession narrowing an interval would leave the old version answering for the dropped period, a reading no reviewer ratified. |
| **d4 — Absent valid time read as unknown and excluded** | Every claim with no period, all earlier claims among them, would leave every filtered read. |
| **d5 — A second pinned object per version** (the investigation's T2) | ADR-0035 d2's file manifest, bound by digest in the envelope, already is one. |
| **d6 — Subscriptions or list-change notifications** | Need a server-to-client stream `/mcp` lacks (ADR-0014 d2), and claude.ai connectors do not support subscriptions. |
| **d6 — Templates only, or every claim in one list** | The first leaves clients that present listed resources, as Claude Code's `@` menu does, nothing; the second is unbounded. |
| **d7 — A read-only FUSE mount now** | fuser's README says "Pull requests are no longer being accepted" and lists "macOS (untested)" (github.com/cberner/fuser), and macFUSE bundled with commercial software is "not allowed without specific prior written permission" (github.com/macfuse/macfuse); fetched 2026-10-31 by the investigation. Deferred until a maintained mount crate and a named user of one. |
| **d7 — WebDAV** | An authoring protocol (RFC 4918) on the API origin, most of whose methods would be refused, and its usual mount, rclone, keeps plaintext on the client: "File caching is necessary to make the VFS layer appear compatible with a normal file system" (rclone.org, fetched 2026-10-31 by the investigation). Rejected. |
| **d3, d8 — Delta time travel or a bitemporal gold table as the source** | The lake is derived and serves no read (ADR-0003 d1); time travel lags by the frontier, is batch- not seq-grained, holds no held body (its d3) and ends at VACUUM under the lake erasure bound ([Delta tables](../data/delta-tables.md#maintenance-optimize-vacuum-checkpoints)). |
