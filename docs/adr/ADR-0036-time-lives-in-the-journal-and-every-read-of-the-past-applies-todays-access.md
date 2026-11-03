<!--
id: adr/ADR-0036-time-lives-in-the-journal-and-every-read-of-the-past-applies-todays-access
tier: engineering
status: proposed
answers: which clocks OML keeps and why they stay apart; how a claim version states when it is true in the world, in clear metadata, reviewed and immutable; how a read asks what OML knew at a seq or an instant, which two facts it takes from then and why everything else is today's; how valid_at and valid ranges filter without ranking; how agents read the same answers as MCP resources and terminals through oml fs, and which claim keys and paths those views address; why no file system or storage engine is built; the build order beside ADR-0030 and ADR-0035
-->

# ADR-0036 — Time lives in the journal: a claim version may state when it is true, a read may ask what OML knew at a seq or an instant, and every read of the past applies today's access

**Status:** proposed · 2026-10-31

**Amends:** ADR-0005 (d4, d5), ADR-0014 (d2), ADR-0018 (d4), ADR-0025 (d8, d9, d12, d13)

## Context

Agents ask two questions OML cannot answer: "What did we hold on 10 October?", since no read takes
a point in time (the only as-of is [ADR-0020](ADR-0020-dataset-onboarding.md)'s dataset as-of),
and "What was true in Q3?", since no claim says when it holds (the submit example in
[openapi.yaml](../api/openapi.yaml) says "valid until 2026-11-15" in words no read can filter on).

**The user's decisions** (2026-10-31), fixed: (1) **bitemporal**: valid time, `valid_from` and
`valid_to`, signed and immutable per version, beside transaction time, which is the journal; (2)
an as-of read applies **today's grants** and today's erasure and demotion state; (3) the
**views** are MCP resources for agents and `oml fs` for terminals; (4) **no file system** or
storage engine of OML's own.

**The investigation** behind (4), run on 2026-10-31 outside this repository, surveyed nine systems
and found time above the bytes in each; Datomic: "An as-of filter returns a database 'as of' at a
particular point in time, ignoring any transactions after that point"
(docs.datomic.com/reference/filters.html, fetched 2026-10-31). OML has that shape: the journal
orders every act by `seq` ([storage](../architecture/storage.md#time)), and a claim's tip moves
only by journalled acts
([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).
What is missing is a read, not a store.

This revision is the smallest mechanism that makes the decisions true; each cut from the earlier
draft is a row among the alternatives. ADR-0030 and ADR-0035, proposed on other branches, are cited
by number, unlinked; names follow [ADR-0026](ADR-0026-names-are-keys.md). Every other decision is
the maintainer's reading, which the user confirms before acceptance. Proposed, not built.

## Decision

- **d1 — Two axes, named, and `simulated_time` stays a third clock neither reads.** *Transaction
  time* is what OML knew, and when: the journal's `seq`, with `at` informational. *Valid time* is
  when a claim version says it holds in the world (d2). The as-of read (d3) moves along the first,
  the valid filters (d4) read the second, and the journal's clock stays "never lent to fiction"
  ([ADR-0011](ADR-0011-simulations.md) d7): a scenario sends its fictional now as `valid_at`. The
  listing's `since` and `until` stay transaction time.

- **d2 — Valid time is two optional ends in clear in the signed submission, immutable per
  version.**
    - **Shape.** `valid_from` and `valid_to` ride submit (remember, learn), supersede and a
      simulation write, each an RFC 3339 `full-date` or a `date-time` with an offset; the interval
      is half-open [from, to), an absent end open, and both present need from < to, else
      `request.validation` with `details.field`. A date compares as 00:00:00Z that day and stays a
      date in the signed form, so "Q3" keeps its precision; an instant is normalized as `at` is
      ([chain format](../data/chain-format.md)). `memory` and `derived_fact` take valid time;
      `steering`, which applies "in every recall of its scope" (ADR-0018 d4), the vocabulary kinds
      and a retirement refuse it (`request.validation`).
    - **Clear metadata.** `submission.remember`, `submission.learn` and `simulation.write` carry
      both beside `claim_key`, outside the encrypted body, each the normalized string or `null`;
      an earlier event carries neither and folds as open
      ([ADR-0004](ADR-0004-bodies-and-attachments.md) d2's additive rule). `memories.valid_from`
      and `valid_to` fold them for every filter, and the keyless reconciler checks them.
    - **Immutable and reviewed.** An interval changes only by a reviewed supersession; a successor
      inherits none ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d11 inherits `kind`
      alone), and the server computes none. The review serve shows both ends beside the words, and
      a verdict judges them together. Valid time schedules nothing: a version valid from 2027 is
      served today to every read that does not filter.
    - **Erasure.** An erase leaves the ends wherever it leaves attribution metadata
      ([erasure inventory](../architecture/audit-and-lineage.md#the-erasure-inventory)), under
      [FR-1](../product/roadmap.md): two dates are minimal, not non-personal
      ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d5), the cost of filtering in clear.

- **d3 — An as-of read resolves one seq S and is today's read with two facts taken at S, rule
  `oml-asof/1`.**
    - **Parameters.** `as_of`, an RFC 3339 `date-time` with an offset, or `as_of_seq`, a journal
      `seq`, never both (`request.validation`), on every read d7 lists, d5's and d6's.
    - **The rule.** Two facts are taken at S: a version submitted after S does not exist, and a
      claim's tip, default or overlay, is the version the last tip move at or before S made one.
      All else is today's: the key's role, grants and denials (ADR-0025 d1); each version's state,
      workarea, taint and `self_signed` scope; the audience; and every rule today's reads apply.
      So a tip at S demoted, erased or held today is served at no S, no earlier version standing
      in; get at S answers as get does today, and `access.not_found` for a version submitted after
      S. Items gain `tip_as_of`. Scores are today's: not the recall made at S, which the
      [read log](../architecture/retrieval.md#the-read-log) holds.
    - **Resolution.** `as_of_seq` is S. An instant t names the longest chain prefix whose every
      `at` is at or before t, since `at` may regress ([storage](../architecture/storage.md#time)).
      The lead keeps the running maximum of `at` per event in `journal_watermarks` (`tenant_id`,
      `seq`, `at_watermark`), written in round trip two's statement
      ([ADR-0024](ADR-0024-performance-foundation.md) d1 unchanged) and indexed on (`tenant_id`,
      `at_watermark`); one probe for the first seq whose watermark exceeds t resolves it.
    - **No future, no echo.** An `as_of_seq` past the applied head, or an `as_of` the applied
      watermark does not exceed, answers `request.as_of_ahead` (`409`, `{applied_seq}`) on the
      lead and a follower ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d13) alike,
      so an instant that answers once names one S everywhere, and every page of a listing reads
      the same S. No answer carries S or an instant; only the read log records S (d4).
    - **The fold records exactly those two facts.** `memories.submitted_seq` holds each version's
      submission seq; `tip_intervals` (`tenant_id`, `artifact_id`, `from_seq`, `to_seq` null while
      open) holds every tip move: `lifecycle.promote`, `lifecycle.supersede` and
      `lifecycle.tip_override` each open a row for the version they make a tip and close the row
      of the version they take the tip from, in round trip two's statement. No other event writes
      either, since demotion, erasure, rejection and reassignment move no tip. Both are fenced per
      tenant as `memories` is and rebuild from the journal, a disagreeing row being a
      `state-mismatch` finding ([ADR-0017](ADR-0017-reconciliation.md) d4). Cost unmeasured —
      [FR-49](../product/roadmap.md).

- **d4 — `valid_at` and valid ranges filter versions and never rank them.** `valid_at`, a date or
  an instant, keeps a version when (`valid_from` null or ≤ t) and (`valid_to` null or t < it);
  `valid_since` and `valid_until`, either optional, keep one whose interval overlaps [since,
  until); `valid_at` with either is `request.validation`. Recall, both listings and `oml fs` take
  them; with an as-of, d3 decides what the key reads at S, then the filter keeps what was true at t.
    - **The served set only.** It filters the rows a read serves, tips at S or today, never a
      superseded version, which is never a hit (ADR-0025 d8): a supersession corrects a claim for
      every period. An open version passes every filter, so steering is never filtered.
    - **Rank and budget.** It removes rows as `kinds` does, inside the scan before `k` and the
      budget, so kept hits keep the total order and a removed row takes no place in either.
    - **The read log.** `query_digest` gains the five parameters, each normalized as asked and
      `null` when absent, and the row records S beside it (amends ADR-0025 d9).

- **d5 — Agents read the same answers as MCP resources, read-only, with no subscription.**
    - **Capability** `"resources": {}`, since "Both `subscribe` and `listChanged` are optional"
      (MCP 2025-06-18, server/resources, fetched 2026-10-31) and `/mcp` is
      [stateless](../api/mcp-plugin.md#transport-streamable-http-stateless). `resources/list`,
      `resources/templates/list` and `resources/read` answer from reads the tools already make, and
      none writes (R1); this amends [ADR-0014](ADR-0014-http-api-and-mcp-facade.md) d2.
    - **Templates and segments.** `oml://workarea/{workarea}/claim/{claim_key}{?as_of,as_of_seq}`
      and `oml://workarea/{workarea}/claim/{claim_key}/file{/path*}{?as_of,as_of_seq}` (RFC 6570
      simple and path-segment expansion only), `{workarea}` the workarea's name or id (ADR-0026
      d4); the bare URI reads today's tip. The server decodes no path segment: a path holding a
      `%`, a reserved character (RFC 3986 §2.2) inside a segment, or an empty, `.` or `..` segment
      is refused, `-32602` with `request.validation` and `details.field: "uri"`. A `claim_key` or
      file path therefore has a URI exactly when each segment is spelled in unreserved characters
      (§2.3), which expansion leaves unchanged; any other is read by `oml_get`. The query is
      decoded once.
    - **Reads.** A claim URI reads the served tip of `claim_key` in the node `{workarea}` names, resolved
      as every name is (ADR-0026 d4): the scoped listing at that node
      ([ADR-0022](ADR-0022-two-ring-api.md) d4), filtered by `claim_key`, keeps the item whose
      `workarea_id` is that node, and get of it gives one
      `application/json` text, the [claim item](../api/mcp-plugin.md#the-claim-item). A file URI
      adds the fetch of that version's file (ADR-0035 d3); its file manifest, bound by digest in the
      signed envelope (ADR-0035 d2), fixes the file set at every S. A file is `text` for an inline
      type (ADR-0035 d4), base64 `blob` otherwise, within `submission.max_body_size`, else refused
      with `details.reason: "inline_cap"`. Each read keeps its refusal order and writes its own
      read-log rows. A claim or file that does not exist, cannot be read, or has no item there
      answers the same bytes, the spec's `-32002`, "Resource not found", `data: {uri}`; any other
      refusal is `-32602` with OML's typed error as `data`.
    - **The list.** `resources/list` serves one page of the scoped listing at the key's default
      workarea, its cursor as MCP's: each claim's bare URI under its workarea's name, the last
      segment of its `workarea_path` (ADR-0026 d7), `name` its `claim_key`, `mimeType`
      `application/json`; no files, and no claim without a URI.
    - **Unverified clients.** Whether Claude Code or claude.ai expands templates, reads a blob or
      caps a resource is unverified — [FR-48](../product/roadmap.md).

- **d6 — Terminals read through `oml fs`, read-only, under the agent's own key.** `oml fs ls`,
  `cat` and `export` run under `OML_URL` and `OML_API_KEY` alone, as
  [ADR-0029](ADR-0029-session-start-hook.md)'s hook does (an admin key's content read is
  `access.verb_ceiling`, ADR-0026 d15). Each takes `--as-of` or `--seq`, `ls` and `export` take
  `--valid-at`, each read is the HTTP read it runs, and nothing writes.
    - **Paths.** `<workarea>/<claim_key>[/<file path>]`: `<workarea>` a name or an id, sent as the
      CLI sends every name ([conventions](../cli/reference.md#conventions)), never a derived path
      (ADR-0026 d7); `<claim_key>` and the file path under d5's segment rule, else exit `2`. A
      claim resolves as d5's does. `ls` lists a node's claims or a claim's files; `cat` prints a
      body or an inline file, refuses a binary on a terminal (exit `2`), and with `--json` prints
      the claim item.
    - **`export <workarea> <dir>`** walks the scoped listing at `<workarea>` at one point, `--seq`
      or `--as-of`, else the tail's `head_seq` read first, into an empty `<dir>` (else exit `2`):
      `<name>/<claim_key>/body.txt` and the claim's files under `files/` beside it, `<name>` as in
      d5's list, each renamed from a temporary name once complete, a file only once its SHA-256
      matches its descriptor (else exit `2`). `oml-export.json` records the point the
      caller gave, per claim its `claim_key`, `artifact_id`, version and valid interval, per file
      its path, digest and media type, and under `skipped` each claim without a path or equal to
      another key of its workarea ignoring case. The plaintext rests on the client, outside
      ADR-0005's claim, as its d4's caches row says of every such copy.

- **d7 — The wire, one code and the vocabulary, held here until built.** `valid_from` and
  `valid_to` on `SubmitRequest`, `SupersedeRequest`, `oml_remember`, `oml_learn`,
  `oml_supersede` and every claim item; `as_of` and `as_of_seq` on get, the scoped listing,
  `RecallRequest`, `oml_get` and `oml_recall`; `valid_at`, `valid_since` and `valid_until` on both
  listings, `RecallRequest` and `oml_recall`; `tip_as_of` on every item an as-of read serves.
  Twelve tools still (amends ADR-0025 d12, d13); one code, `request.as_of_ahead` (`409`); and
  three rows for ADR-0018 d4 (amends it):

    | Term | Means | Never |
    |---|---|---|
    | transaction time | what OML knew and when: the journal's `seq`, with `at` informational | "history" as the axis's name |
    | valid time | the interval [`valid_from`, `valid_to`) in which a claim version says it holds in the world | "effective date" |
    | as-of read | a read at one resolved `seq` under rule `oml-asof/1`: existence and the tip taken at that seq, all else today's | "time travel" for OML reads (Delta's term, bounded by VACUUM); "snapshot read" |

- **d8 — The build runs claims first; what needs ADR-0030 or ADR-0035 waits for it.** S1 to S5
  need nothing unbuilt. ADR-0030 and ADR-0035 are proposed and unbuilt, so S6 waits for ADR-0030's
  d4 and d5 and S7 for ADR-0035's S2 and S3, and a declined record drops its step by amendment.
  ADR-0035's `deliverable` takes valid time as `memory` does, in whichever of S1 and its S4 lands
  second. Each step changes its contract pages with its code.

    | Step | Lands | Check, tests named as sentences |
    |---|---|---|
    | S1 | d2 on every submission path: the clear ends, the fold, the serve | `a_valid_from_not_before_valid_to_is_refused`, `a_date_compares_as_utc_midnight_and_is_served_as_a_date`, `a_successor_that_omits_valid_time_is_open`, `steering_vocabulary_kinds_and_retirements_refuse_valid_time`, `an_event_written_before_valid_time_folds_as_open`, `the_review_serve_shows_both_ends_beside_the_words` |
    | S2 | d3 on get and the scoped listing: `submitted_seq`, `tip_intervals`, the watermark, `as_of`, `as_of_seq`, the code | `the_tip_intervals_and_watermarks_equal_a_replay_of_the_journal_after_every_event`, `an_instant_resolves_to_the_longest_prefix_stamped_at_or_before_it`, `no_as_of_answer_carries_the_seq_or_an_instant`, `lead_and_follower_refuse_a_point_they_have_not_applied`, `an_as_of_read_never_serves_a_version_demoted_or_erased_today`, `an_as_of_read_applies_todays_grants_and_denials`, `a_get_at_s_of_a_version_submitted_after_s_is_not_found` |
    | S3 | d3 on recall; d4 on recall and both listings; the digest members | `valid_at_removes_hits_and_never_reorders_the_rest`, `valid_at_never_removes_steering`, `a_filtered_row_takes_no_place_in_k_or_the_budget`, `recall_as_of_and_valid_at_answers_the_q3_example`, `the_request_digest_carries_the_point_asked_and_the_log_row_s` |
    | S4 | d5 over claims: the capability, the claim template, list, read, the segment rule, the error mapping | `initialize_declares_resources_without_subscribe_or_list_changed`, `a_claim_resource_resolves_at_its_named_workarea`, `a_uri_holding_a_percent_or_a_reserved_character_in_a_segment_is_refused`, `a_missing_and_an_unreadable_claim_resource_answer_the_same_bytes`, `no_resources_method_writes` |
    | S5 | d6 over claims and bodies | `fs_export_refuses_a_non_empty_directory`, `fs_export_skips_unaddressable_keys_and_case_twins`, `fs_under_an_admin_key_reads_no_content`, in `cli_e2e.rs` |
    | S6 | d3 through ADR-0030's boundary and held readers | `an_as_of_read_scoped_outside_an_in_progress_subtree_serves_nothing_from_inside` |
    | S7 | d5's file template, `oml fs` file reads and export's files | `an_as_of_read_resolves_the_file_set_its_version_pinned`, `a_binary_file_resource_past_the_inline_cap_is_refused`, `fs_export_writes_a_file_only_after_its_digest_matches` |
    | S8 | FR-48's dated app tests, after S4 and S7 | a recorded Claude Code and claude.ai session reading a claim, a text file and a binary file through the templates, at an `as_of` too |

## Consequences

**One claim, end to end**, in workarea `finance`: `agent-a` a writer and reader key, `agent-c` a
reader key, `rev-1` and `rev-2` dreamer keys, each addressed by its name. Seqs are illustrative.

| # | When | Key | Call | Answer |
|---|---|---|---|---|
| 1 | 2026-07-03, 07-04 | `agent-a`, then `rev-1` | `oml_remember {workarea: "finance", claim_key: "q3-forecast", text: "The Q3 forecast is 12M.", valid_from: "2026-07-01", valid_to: "2026-10-01"}`, then an approving verdict | held `V1` at seq 1040; `lifecycle.promote` at 1052 opens `V1`'s tip interval [1052, —) |
| 2 | 2026-10-14, 10-15 | `agent-a`, then `rev-2` | `oml_supersede {artifact_id: V1, text: "The Q3 forecast is 12.4M.", valid_from: "2026-07-01", valid_to: "2026-10-01"}`, then an approving verdict | held `V2` at 2310; `lifecycle.supersede` at 2321: `V1` [1052, 2321), `V2` [2321, —) |
| 3 | today | `agent-c` | `oml_recall {query: "Q3 forecast", as_of: "2026-10-10T00:00:00Z", valid_at: "2026-08-01"}` | `V1`, "12M", `tip_as_of: true`, `tip: false`; no seq and no instant in the answer |
| 4 | today | `agent-c` | the same without `as_of` | `V2`, "12.4M" |
| 5 | today | `agent-c` | step 3 with `valid_at: "2026-11-01"` | no hit: `V1`'s interval ends 2026-10-01 |
| 6 | today | `agent-a` | `oml_get {artifact_id: V2, as_of_seq: 2300}` | `access.not_found`: `V2` was submitted at 2310 |
| 7 | today | `agent-c` | `resources/read` of `oml://workarea/finance/claim/q3-forecast?as_of=2026-10-10T00%3A00%3A00Z`; `oml fs export finance q3 --as-of 2026-10-10T00:00:00Z` | `V1`'s claim item; `q3/finance/q3-forecast/body.txt` and `q3/oml-export.json`, recording the `--as-of` the caller gave |
| 8 | after an admin demotes `V1` | `agent-c` | step 3 again | no hit; `V2` does not stand in, not being the tip on 10 October |

**Costs, stated.** Every tip move writes an interval row and every journal row a watermark row in
the chain lock's statement, and an as-of recall joins intervals in its scan, all unmeasured
(FR-49). An as-of answer is the past seen through today, not a replay (d3). The valid ends outlive
an erase (d2, FR-1). `request.as_of_ahead`'s edge tells a key its tenant's newest `at`, a time,
never content (d3). A claim key or file path with a space, a `/` or a non-ASCII letter has no URI
and no `oml fs` path (d5). Exported plaintext is outside the erasure claim (d6).

**Opened.** [FR-47](../product/roadmap.md), time, closes when S1 to S5 and S7 pass and the table
above runs in continuous integration; FR-48 and FR-49 as above. No contract page changes now, so
`info.version` stays `0.3.0-design`.

**Process obligations.** With the proposal: this file, a [register](index.md) row, a `mkdocs.yml`
nav entry, `llms.txt`, FR-47 to FR-49 marked proposed, one [changelog](../changelog.md) entry. On
acceptance: this record and its row become `accepted design`, dated; ADR-0005, ADR-0014, ADR-0018
and ADR-0025 gain "· amended by ADR-0036" on their Status lines and frontmatter, text unedited;
the erasure inventory names the valid ends and d6's export; FR-31's row names the newest `at` the
refusal's edge shows; in whichever of this record and ADR-0030 is accepted second, `P-R3-BOUNDARY`
leaves out an as-of read's seq and `request.as_of_ahead`; retrieval, storage, the event catalog,
Postgres schema, HTTP API, errors, `openapi.yaml`, `mcp-tools.json`, the agent plugin and the CLI
reference take d3, d4 and d7; FR-25 names the as-of recall and the interval write; the D4, D12,
D13 and D14 cells name this record; the spec tag is cut.

## Rejected and deferred alternatives

| Alternative | Why not |
|---|---|
| **d1 — An own file system or storage engine** (the user's decision 4) | It adds no time, which sits above the bytes in all nine surveyed systems, and brings crash-consistency risk: "Data corruption is the most predominant consequence (40%)" of Linux file-system bugs (Lu et al., FAST 2013, fetched 2026-10-31 by the investigation). |
| **d1 — A Git-, Dolt- or lakeFS-style versioned store** | A second history beside the signed journal, and erasure becomes a rewrite and a garbage collection. |
| **d2 — The ends sealed under the version's DEK as a second ciphertext (the earlier draft), or inside the encrypted body** | Every serve would decrypt and cross-check them, the keyless reconciler could check nothing, and clear filter columns were needed anyway. |
| **d2 — An interval inherited from a predecessor or computed from lineage** | ADR-0025 d11 lets a successor inherit `kind` alone; a computed interval is a claim nobody stated or reviewed. |
| **d2 — Zone-local dates, or instants only** | A date without a zone names a different instant per reader; instants only force a false precision on "Q3". |
| **d2 — Valid time as embargo or expiry** | It would move versions on and off read surfaces by clock, a visibility act outside review; retention is [FR-26](../product/roadmap.md)'s. |
| **d3 — The grants a key held at S** | A revoked key's past would stay readable, and a denial issued for cause could be stepped around by naming an earlier seq. |
| **d3 — Serving the tip at S though it is demoted or erased today, or the tip before it** | Demotion and erasure are journalled acts on what is served now (R4); an earlier version was not what OML held at S. |
| **d3 — Each version's held, rejected and demoted periods folded and read at S** (the earlier draft) | Deferred: the as-of rule reads no state at S, and folding every state change exactly widened the fold past what any read uses. |
| **d3 — Echoing the instant, or the seq an instant resolves to** (the earlier drafts) | The instant returns only what the caller sent; the seq lets a key bisect `as_of` and time events it cannot read. |
| **d4 — Every promoted version whose interval contains t, the latest winning** (SQL:2011-style period rows) | It makes superseded versions hits again, against ADR-0025 d8, and leaves an old version answering for a period a narrowing supersession dropped. |
| **d4 — Absent valid time read as unknown and excluded** | Every claim with no period, all earlier claims among them, would leave every filtered read. |
| **d5 — RFC 6570 reserved expansion, `{+path}`, or percent-encoded segments the server decodes** (the earlier drafts) | `{+path}` puts a name's `?`, `#` or `%` into the URI as syntax, and decoding lets two spellings name one segment. |
| **d5 — A second pinned object per version, or a durability condition on ADR-0035's store** (the earlier draft) | ADR-0035 d2's file manifest, bound by digest in the envelope, already pins the set, and the store is ADR-0035's. |
| **d5 — Subscriptions or list-change notifications** | They need a server-to-client stream `/mcp` lacks (ADR-0014 d2), and claude.ai connectors list "Text and binary resources" but not "Resource subscriptions" (claude.com/docs/connectors/building, fetched 2026-10-31). |
| **d5 — Templates only, or every claim in one list** | The first leaves clients that present listed resources nothing; the second is unbounded. |
| **d6 — `oml fs tree`, `stat` and `get`** (the earlier draft) | `ls` per node, `cat --json` and `oml attach get` already do each. |
| **d6 — A read-only FUSE mount now** | Deferred until a maintained mount crate and a named user: fuser's README says "Pull requests are no longer being accepted" (github.com/cberner/fuser, fetched 2026-10-31 by the investigation). |
| **d6 — WebDAV** | An authoring protocol (RFC 4918) whose methods would mostly be refused, and its usual mount, rclone, caches plaintext on the client. |
| **Tip intervals in gold, Delta time travel, or a bitemporal gold table as the source** (the earlier draft) | Deferred: the lake serves no read ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) d1), time travel lags by the frontier and ends at VACUUM, and no lake reader needs the intervals yet. |
