<!--
id: adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict
tier: engineering
status: accepted design · 2026-11-05 · amended 2026-11-13 (d1, d10), accepted by the operator the same day
answers: who may review in bulk and on which surfaces; why agent keys and MCP never reach it; what a reviewer is shown and how the selection is named; why the verdict names ids and never a filter; the cap per request; how each item is judged, journalled and signed; how R2 is held per item and what a skipped item answers; what a stale id answers; whether a claim approval is marked same_user; bulk reject and its reason; idempotency and retry; why the queue slowed with tens of thousands held and what fixed it, measured; the routes, the admin page's view and actions, the CLI verbs, the metric and the doctor line; the laws and their tests; the rejected alternatives
-->

# ADR-0055 — A person reviews a listed set of held items in one act, each item judged and signed as one verdict

**Status:** accepted design · 2026-11-05 · proposed and accepted on 2026-11-05 with release v0.5.7 · amended 2026-11-13 by [the amendment](#the-amendment-of-2026-11-13-the-persons-review-page-lists-what-waits-and-judges-one-item-from-the-list) (d1 and d10: the person's Review page lists what waits and judges one item from the list; ADR-0045 d9 and d11 and ADR-0070 d7 (b) follow), which the operator accepted the same day · amended by [ADR-0083](ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) (d10, 2026-11-14)

**Release:** introduced in v0.5.7 ([releases](../changelog.md))

**Amends:** ADR-0039 (d9: two served routes, `GET /v2/review/held` and
`POST /v2/review/verdicts/bulk`; d10: `oml review list`, and `--ids` on `oml review approve` and
`oml review reject`), ADR-0045 (d1: the admin page's closed action set gains `bulk_confirm` and
`bulk_verdict`, and its views `view=held`; d11: review from the admin page may judge a listed set),
ADR-0046 (d2: the `same_user` mark extends to a claim's ratification, `lifecycle.promote` and
`lifecycle.supersede`)

## Context

**The case.** The operator is about to load 614 held memories, one per ADR decision (claim keys
`adr-NNNN-dN`, `adr-NNNN-summary`, `adr-NNNN-rejected`), into a workarea under their user root.
Review today judges one item per call: `oml review next` claims one row for the key and serves
it, and `oml review approve` records one verdict
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d10); the admin page runs the same ceremony
([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
d11). Two calls and a read per item is 1,228 calls for this import, and
#98 records the queue itself slowing as it grows: six reviewer keys
approved about 77 rows a second with 5,000 held and 2 to 14 a second with 45,000 held
(`eval/bench/REPORT.md`, loading at scale; 2026-11-04).

**What it is held against.** The constitution ([ADR-0001](ADR-0001-the-constitution.md)): R1, an
agent never promotes; R2, the reviewer is never the author, checked per key and principal over
the lineage closure; R3, a held row is read by its author and by the reviewer it is served to;
R4, nothing is deleted silently. A bulk verdict is many verdicts, so each of those rules must
hold for every item, and the request must not become a way to approve what nobody looked at.
The Occam API rule (ADR-0039 d8) asks that each new call trace to a use case: here the import
above, and the review of any crew's backlog after it.

Decided, built on `feat/bulk-review`, and accepted and introduced with release v0.5.7.

## Decision

- **d1 — Bulk review is a person's act, on the admin page and the CLI only.** *(Amended 2026-11-13: the person's Review page also lists what waits and judges one item from the list; see [the amendment](#the-amendment-of-2026-11-13-the-persons-review-page-lists-what-waits-and-judges-one-item-from-the-list).)* The listing and the
  bulk verdict admit a key whose role is `dreamer` and whose `held_by` is `human`, the reviewer
  keys a person holds: the `<user>-review` session key the admin page acts through (ADR-0045 d6)
  and a dreamer key issued `--held-by human`, such as `oml quickstart`'s reviewer. Every other
  key is refused before any lookup with `access.verb_ceiling`, `details.reason:
  bulk_review_is_human_only`: a writer or reader key (R1), a `held_by: service` key (ADR-0025
  d1), and a dreamer key held by an agent, a reviewer crew's, which keeps reviewing one item per
  serve ([reviewer crews](../api/reviewer-crews.md)). The MCP catalog gains no tool, argument or
  resource; `/mcp` never names either route. The cost is that an automated crew cannot drain a
  backlog in bulk, which is the point: a bulk verdict is a person's attestation that they read a
  list, and `held_by` is the one field that says a person holds the key. It is declared at
  issuance by an admin, so it is as true as the admin's word, which the journal records.

- **d2 — The reviewer is shown a list, and the list is exactly what review would serve them.**
  `GET /v2/review/held` serves, with their text, the held rows the calling key may judge: the
  predicate of `review next` — rows in the key's dreamer access, not claimed by another key, whose
  lineage closure holds neither the key nor its principal, and no held successor whose rival the
  key authored — oldest first (`held_since`, then `artifact_id`), at most `limit` (1 to 200,
  default 50) a page, with `next_cursor`. It filters by `workarea` (that node's subtree, by id or
  name), `author_key` (a key id or name) and `claim_key_prefix`. It claims nothing and journals
  nothing, as `review next`'s serve journals nothing: it widens one serve to a page, never the
  set of rows the key may be shown. The item carries `same_user` (d7) and `held_since`.

- **d3 — The verdict names ids, never a filter.** `POST /v2/review/verdicts/bulk` takes
  `{verdict, artifact_ids, reason}`: `verdict` is `approve` or `reject`; `artifact_ids` is 1 to
  200 distinct artifact ids the reviewer was listed; `reason` is required to reject and optional
  to approve. A filter is never evaluated at verdict time, so a row written after the reviewer
  read the list is never judged by it, and the request records what the person chose. 200 per
  request bounds how long one request holds a connection and one page renders (d9 measures
  it); the CLI splits a longer list into requests of 200. An empty list, more than 200 ids, a
  repeated id, or an id that is not a UUID is `request.validation` naming `/artifact_ids`.

- **d4 — Each item is one verdict, judged and signed as a single approve is.** The server runs
  the verdict use case of `POST /v2/review/verdicts` once per id, in the order named: dreamer
  access on the row's workarea, R2 against the row's reviewer-exclusion set, the status
  transition, the frozen-area refusal, the supersession compare-and-set, then a `review.approve`
  or `review.reject` event under the reviewer key and, for an approval, `lifecycle.promote` or
  `lifecycle.supersede` under the server, each Ed25519-signed and chained, committed in that
  item's own transaction. No event names two items. An item that fails is answered and the next
  is judged; nothing already committed is undone. A chain-head conflict is retried per item as
  the single verdict's route retries it. The answer is `200` with one outcome per id —
  `{artifact_id, outcome: approved | rejected | skipped, state, journal_seq, same_user,
  replayed}`, a skipped one with the refusal's `code` and `message` as `POST /v2/review/verdicts`
  would have answered them — and a `summary` of the counts. A refusal of the request itself
  (d1, d3, authentication, writes closed) is the one error envelope (ADR-0039 d11).

- **d5 — R2 skips an item, never approves it.** An item whose reviewer-exclusion set holds the
  reviewing key or its principal is skipped with `review.author_in_lineage` and stays held for
  another reviewer. The listing never shows such a row (d2), so a skip means the id came from
  somewhere else; the check at verdict time is the one that decides. As ADR-0001 d6 states, the
  exclusion is as good as the declared `derives_from` graph.

- **d6 — A stale id is reported, never judged.** An id no longer held — already approved,
  rejected, or a supersession whose target lost the tip — answers the single verdict's refusal:
  `review.serve_invalid` with `details.reason: consumed` for a decided row,
  `submission.tip_conflict` for a lost tip; an id the key cannot see is `access.not_found`; one
  outside its dreamer access is `access.grant_refused`; an approval in a `validated` area is
  `workarea.frozen`. A row another key has claimed with `review next` is judged as a single
  verdict naming it would be, and the first verdict wins.

- **d7 — A claim's ratification carries `same_user`, single and bulk alike.** ADR-0046 d2 marks a
  transition's and an edge's ratification `same_user: true` when the ratifying key and the
  proposing key name one user. A claim's approval is the ratifying act at k = 1, and the
  operator's import is approved by their own `<user>-review` key over their own agent's writes,
  which ADR-0045 d11 permits. So `lifecycle.promote` and `lifecycle.supersede` carry `same_user`,
  `true` when the authoring key and the reviewing key act for one person. A key's person is read
  as a name: a user key's root's name (ADR-0044 d2), or a session key's `session_user`, since
  `<user>-review` is a tenant key that names its person that way (ADR-0045 d6); a tenant key
  with neither is no one's, so the mark is `false`. This is the one definition: transitions and
  edges use the same function, so a session key would read the same there. A tenant crew key an
  admin issues for a person's import is no one's, and every approval of its writes reads `false`;
  the operator who wants the mark loads through a key issued with `user`. The mark says what the step is worth to an auditor; it relaxes
  nothing, and it is not `self_signed` (ADR-0001 d5).

- **d8 — Bulk reject is built, with one reason applied to each item.** Rejecting a batch the
  reviewer read and found wrong is the same act as approving one, and a wrong import is the case
  that needs it most. Each rejected item journals its own `review.reject` with the one reason,
  and keeps its bytes (R4).

- **d9 — The queue's slowdown was a whole-queue join and sort, and an index and a guard remove
  it.** `review next` read `review_queue` joined to `memories` and ordered by `held_since` with
  no index on that order, and its rival-successor check (`NOT EXISTS` over held rows) was
  planned as a hash anti-join over every held row. Every call therefore scanned the queue and
  the tenant's memories twice, joined, and sorted all of them to return one. Measured with
  `EXPLAIN (ANALYZE, BUFFERS)` on 45,000 held rows loaded through the write route (2026-11-05,
  PostgreSQL 18, the maintainer's 8-core host): 100.4 ms a call, three sequential scans, a hash
  anti-join and a 45,000-row sort; under several reviewers each call also locks behind the
  others, which is the 2 to 14 rows a second #98 recorded. Migration `0024` adds
  `review_queue_order (tenant_id, held_since, memory_id)`, and the rival check runs only for a
  successor (`m.replaces_artifact_id IS NULL OR NOT EXISTS …`), the only row it can refuse. The
  plan then walks the index, reads one `memories` row by its key and stops: 0.17 ms on the same
  rows. The listing uses the same predicate and order; a page of 200 takes 12 to 30 ms, and a
  filter that matches nothing reads the whole queue, 160 ms at 45,000 held. The end-to-end
  numbers, through the HTTP router in process with a release build, 45,000 held rows loaded
  through the write route, one reviewer key, load average 12 to 15 on 8 cores while two other
  branches built (2026-11-05, `review_scale_measurement` in `tests/bulk_review.rs`): `review next`
  p50 3.2 ms and p95 12.8 ms, `next` then a single approve 79.5 items a second; listing 600 items
  0.04 s and approving them through `POST /v2/review/verdicts/bulk`, 200 a request, 7.06 s, 85
  items a second. Before the change six keys approved 2 to 14 a second at 45,000 held (#98).

- **d10 — The admin page lists, selects, confirms and shows each outcome.** `GET
  /web/admin?view=held` renders the listing of d2 through the person's `<user>-review` key with
  its three filters as a `GET` form, each item's text, claim key, author, workarea and age, a
  checkbox per item, and a "select all on this page" link that renders the page with every box
  checked, since the page carries no script (ADR-0045 d8). Action `bulk_confirm`
  (`artifact_id` repeated, `verdict`, `reason`) renders the confirmation: the verdict, the count
  and each chosen item's claim key and first line, with a form for `bulk_verdict` carrying the
  same fields and an idempotency key minted at confirmation. Action `bulk_verdict` runs d4 and
  renders the per-item outcome table. Both forms carry `csrf_token` and pass the same-origin
  check; neither is on the user page, whose bulk review is open. *(Amended 2026-11-13: the user page lists and judges one item per act; bulk selection stays the admin page's.)*

- **d11 — The CLI lists to a file and judges from one.** `oml review list [--workarea]
  [--author-key] [--claim-key-prefix] [--limit] [--all] [--ids-only]` prints the listing, every
  page with `--all`, and only the ids, one per line, with `--ids-only`. `oml review approve --ids
  <file | - | id,id,…>` and `oml review reject --ids … --reason <text>` send the ids in requests
  of 200, each under an idempotency key derived from the list's SHA-256 and the request's
  position, so a rerun of the same command replays, and print one line per item and a summary;
  the exit status is 0 when every item was decided and 1 when any was skipped. `--ids` and a
  `serve_id` are mutually exclusive.

- **d12 — A retry replays.** Each item records its outcome under the request's
  `Idempotency-Key` and its own id. A retry with the same key replays every item already decided
  (`replayed: true`) and judges the rest anew; a retry naming a decided item with another verdict
  or reason answers that item `idempotency.digest_mismatch`. Without the record the request is
  still safe to repeat: a decided row is not held, so it is reported as stale (d6).

- **d13 — One metric family and one doctor line.** `oml_review_bulk_items_total{outcome}` counts
  items by `approved`, `rejected` and `skipped`, from the route and the admin page alike, a
  replayed item not counted again, on the metrics listener (ADR-0042 d11), and the
  request line carries the route as every request does. `oml doctor` run with a dreamer key held
  by an agent warns that bulk review is refused for it and names the fix — a key issued
  `--held-by human` — since the refusal otherwise first appears mid-import.

## Consequences

- The served contract gains two operations, `reviewHeld` and `reviewVerdictsBulk`, and the SDKs
  regenerate; `ROUTES` and `docs/api/openapi.yaml` stay equal (ADR-0039 d9). `ADMIN_ACTIONS`
  gains two values and the router test that pins it changes with this record.
- `review_queue` gains one index (d9) in migration `0024`, numbered past the `0022` that two
  other branches hold; it is built at the first start of the new server.
- The event catalog's `lifecycle.promote` and `lifecycle.supersede` gain `same_user` (d7).
  Consumers that ignore unknown fields are unaffected.
- #98 is addressed by d3 and d9: one person judges a backlog in
  requests of 200 instead of a crew of keys one row at a time, and the queue statement no longer
  sorts the whole queue. Serving one key several claimed rows, #98's other option, is not
  built.
- The user page keeps one-at-a-time review; bulk on it waits for a person to ask. *(Amended 2026-11-13: it also lists what waits and judges one listed item per act; bulk selection on it still waits for a person to ask.)*

## Laws and their tests

| Law | Test |
|---|---|
| An agent key never reaches bulk review, on HTTP or MCP | `an_agent_key_cannot_list_or_bulk_approve_on_any_transport` |
| A dreamer key held by an agent is refused | `a_dreamer_key_held_by_an_agent_is_refused_bulk_review` |
| An item in the reviewer's own key's or principal's lineage is skipped, never approved | `an_item_the_reviewer_authored_or_derives_from_is_skipped_not_approved` |
| A stale id is reported, never approved | `a_stale_id_is_reported_not_approved` |
| Each approved item's journal equals a single approve's | `every_bulk_approved_item_has_its_own_verdict_and_ratification_equal_to_a_single_approve` |
| A retry replays | `a_bulk_retry_replays_every_decided_item` |
| The listing is what review would serve | `the_listing_serves_only_what_review_next_would_and_claims_nothing` |
| The verdict names ids, capped | `a_bulk_request_over_the_cap_or_with_a_repeated_id_is_refused` |
| Bulk reject journals one reason per item | `a_bulk_reject_journals_each_item_with_the_one_reason` |
| The admin page confirms before it acts | `the_admin_page_lists_confirms_and_shows_each_outcome` |

## The amendment of 2026-11-13: the person's Review page lists what waits and judges one item from the list

**Status of this amendment:** accepted design · 2026-11-13, by the operator, before it is built · built in v0.7.3 with the console redesign

On 2026-11-12 and 2026-11-13 the operator asked for a console round that includes a Review tab
showing every fact the way Memories does. After seeing the published mockups, the operator decided
on 2026-11-13 that the Personal Review page offers "Open to judge", answering "yes, amend the
records". d1, d10 and the last Consequence kept the listing and the listed-set verdict off the
user page, and the person's page served review next alone
([ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)
Phase (b) as built, d7 (b)). The mockups draw the person's Review as a list of what waits, by
workarea, with "Open to judge" on each row. This amendment records that decision and nothing
beyond it: what the build draws and how it groups the list are the build's, and are named below.

| Decision | Was | Is | Why |
|---|---|---|---|
| d1 | the listing and the bulk verdict are the admin page's and the CLI's: "Bulk review is a person's act, on the admin page and the CLI only" | a person's `<user>-review` session key, the `dreamer` key of `held_by: human` that d1 already admits, lists the held items it may judge on the Personal Review page (`GET /v2/review/held`, the listing of d2 as it is) and records a verdict on one item from that list (`POST /v2/review/verdicts/bulk`, `artifact_ids` holding that one id, d3). Every other key is refused as d1 states, `bulk_review_is_human_only`, and `/mcp` names neither route | the person already reviews through that key, one served item at a time (ADR-0045 d11); the listing shows what that key may be served; one id in d3's request is one verdict, judged and signed as d4 says. No route, member, event or migration is added. The cost: a `GET` of the person's Review page makes the listing's reads, which the page did not make; the build records the new store-call count in `web_console_reads.rs` with this reason (ADR-0070 d8) |
| d10 | the admin page lists, selects, confirms and shows each outcome; neither form "is on the user page, whose bulk review is open" | the admin page is unchanged. The person's page gains the listing and the one-id verdict, with no pick box, no "select all", no confirmation of a set and no `bulk_confirm`; whether its verdict runs under `bulk_verdict`'s action name or one of its own is the build's | the operator's decision is "Open to judge" on a row, one id; a selection of many on the person's side is not part of it |
| ADR-0045 d9, d11 | Review is `review_next` and `verdict`, the served ceremony, and "the form carries the `serve_id`" | Review is those two and the listing with its one-id verdict. `review_next` and its serve-bound verdict stay as d11 states. An "Open to judge" verdict carries no `serve_id`: the id the form names is what it judges, as in d3, and the server's checks of d4 to d6 decide whether it may | the listing claims nothing (d2), so there is no serve to carry. The cost: for this act the serve binding does not decide what is judged, and the id comes from the browser as the admin page's bulk ids do. The server refuses what the key may not judge as d4 to d6 state: `review.author_in_lineage` skipped, `access.not_found` for an id the key cannot see, `access.grant_refused`, `workarea.frozen`, `review.serve_invalid` with `details.reason: consumed` for a decided row. A row another key has claimed with `review next` is judged as a single verdict naming it would be, and the first verdict wins (d6) |
| Consequences | "The user page keeps one-at-a-time review; bulk on it waits for a person to ask." | the user page lists what waits and judges one listed item per act; bulk selection of many on it still waits for a person to ask | the decision covers one id |

**R2 is unchanged.** The listing never shows the reviewer's own writes or anything whose lineage
closure holds the reviewing key or its principal (d2), and the check at verdict time decides what a
listing could not (d5). The person's own agents' writes appear in the listing, marked `same_user`,
and an approval of one journals `same_user: true` (d7), as
[ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
d11 allows: the reviewing key is never in the row's closure, and the journal shows the reviewing
key beside the authoring one. That mark says what the step is worth to an auditor, and it is not
`self_signed` (ADR-0001 d5). The development exception of ADR-0001 d5 is untouched: the page
offers no self-signature. *(Amended 2026-11-14: the development self-signing exception is retired (ADR-0001 d5); the build never sets `self_signed`, so this text describes nothing that is served.)*

**What is not changed.**

- Bulk selection of many items on the Personal side is not added. A person who wants a selected
  set judges it on the admin page or with `oml review approve --ids`, as before.
- The admin page's queue and bulk review (d10), the CLI (d11), the refusal of every key that is
  not a person's reviewer key (d1), the cap and the idempotency of d3 and d12, and the metric and
  the doctor line of d13 are as they were. The MCP catalog gains nothing.
- The listing is not a claim: nothing is held for the reader, so a listed item waits for any
  reviewer until a verdict lands.

**What the build chooses**, and says it chose: how the list is grouped (by workarea, as the
mockups draw it), the page size within d2's 1 to 200, which filters the page offers, what each row
shows, and the one-id verdict's idempotency key (minted per form, as every act's is).

**Rejected, for this amendment:**

| Alternative | Why not |
|---|---|
| **The list read-only, with review next serving the oldest, as the person's page does today** | The fallback the mockups name. A person who reads a row in the list cannot judge it, only the oldest; the operator chose the act. |
| **Bulk selection on the Personal page, the admin page's `bulk_confirm` and `bulk_verdict`** | Not part of the decision, and d1's reasoning that a bulk verdict is a person's attestation that they read a list is carried by the admin page; it waits for a person to ask. |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A verdict over a server-side filter** ("approve every held row with prefix `adr-`") | It judges rows the reviewer never saw, including rows written after they looked; the request would record a query, not a choice (d3). |
| **One transaction for the whole batch** | One stale id would undo 199 good verdicts, and the chain head would stay locked for the whole batch against every writer. Per-item commits make partial success the honest answer (d4). |
| **One verdict event naming many artifacts** | An auditor reading one item's history would have to read a batch; the per-item signature and `served_body_digest` would cover a set, not the item (d4). |
| **Serving one key several claimed rows** (#98's other option) | Claims would pile up under one key while the person reads, hiding rows from every other reviewer; the listing claims nothing and the verdict decides (d2). |
| **Bulk for every dreamer key, crews included** | An automated crew approving by the page is the shape R1 exists to stop at the agent; one serve per verdict keeps a crew's review an item at a time (d1). |
| **A bulk tool on MCP** | R1 and ADR-0045 d7: no agent surface promotes. |
| **No cap, or a cap of 1,000** | A request of 45,000 ids holds a connection for minutes and a page of 1,000 checkboxes is not read; 200 approves in about the time a page takes to read (d9). |
| **Dedupe repeated ids silently** | A repeated id is a client defect; refusing it names the field (d3). |
| **No bulk reject** | The import that most needs a bulk act is a wrong one, and one reason fits it (d8). |
| **Keep `same_user` off claim approvals** | 614 approvals of a person's own agent's writes by the same person would look, in the journal, like review by a second person; the mark costs one field (d7). |
