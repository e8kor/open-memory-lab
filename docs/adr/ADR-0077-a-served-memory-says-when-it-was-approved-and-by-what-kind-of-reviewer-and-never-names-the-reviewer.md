<!--
id: adr/ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer
tier: engineering
status: proposed
answers: what a served memory says about its approval over HTTP, MCP and the SDKs; why it says the kind of reviewer and never who; which verdict is "the approver" when a policy needs more than one; what an unreviewed hit, an anonymous universe hit and a federated hit carry; where the approval is stored and how rows promoted before it are filled; what erasing or pseudonymising attribution must cover; and which readings wait for the operator
-->

# ADR-0077 — A served memory says when it was approved and by what kind of reviewer, and never names the reviewer

**Status:** proposed · 2026-11-12, with #175

**Release:** not assigned; proposed for v0.7.2

**Amends:** ADR-0025 (d12: the claim item gains `approved_at` and `approver`)

## Context

A reader of a memory is shown who wrote it and that it was approved, never by what kind of
reviewer. `GET /v1/memories/{artifact_id}`, recall and every listing serve `principal`, the
author's, and `promoted_at`; the web console's card says "approved by a key outside its
lineage" (`crates/clients/oml-web/src/memories.rs`). #175 asks that each hit carry the reviewer
and the approval time beside the author, over HTTP, MCP and both SDKs, with a crew's or a
self-review's approval shown as that.

Three accepted decisions already say who may see a reviewer's identity, and all three say no
reader of a memory may:

- **[ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d15.** The events tail shows
  `review.*` events to no key, and serves every other event to a key outside its actor's key
  set with `principal`, `api_key_id` and every payload field naming a key as null; "the
  evidence export carries the identities". It rejected *Serve the actor's identity on the
  tail*: "Every reader would see which reviewer ratified a row … which the review queue and
  `oml_my_work` withhold."
- **[ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
  d5.** A served guardrail carries `verdicts: [{held_by, at}]`: "No reviewer key name is
  served: ADR-0025 d15 keeps review events in the evidence export. The cost: every reader in
  scope learns how many reviewers ratified a rule and how many of them were human."
- **[ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
  d9.** "The journal keeps every attribution field in the clear — … the reviewer in the review
  events — and the evidence export carries them." The only read that names a reviewer is the
  admin read ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d21), and each one is journalled.

What a reader can already learn of an approval through another call: its time (`promoted_at`),
and, on `GET /v2/events`, the `lifecycle.promote` payload with its `same_user` mark
([ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
d7). The MCP claim item carries no author either (ADR-0025 d12).

So this record builds what those decisions allow, and names the wider reading #175's outcome
sentence asks for as a question for the operator (Open questions, 1). It is held against R2,
the reviewer is never the author, and R3, the gate changes visibility and not ranking
([ADR-0001](ADR-0001-the-constitution.md) d2, d3): nothing here is read by any ranking.

## Decision

- **d1 — Every served claim item carries `approved_at` and `approver`.** On HTTP they are
  members of `MemorySummary`, so get, recall's hits and steering, every listing and the review
  serve carry them through the one function that serves a memory. On MCP they join the claim
  item (amending ADR-0025 d12), on recall and get alike. The SDKs' generated types carry them
  from `openapi.yaml`. Both are null on a version that is not promoted. The cost is two more
  members on every served item.

- **d2 — `approver` says what kind of reviewer approved, never who.** It is `{held_by,
  same_user}`: the reviewing key's attestation, `agent`, `human` or `service`
  ([roles and keys](../architecture/roles-and-keys.md)), as ADR-0037 d5 serves it for a
  guardrail's verdicts, and ADR-0055 d7's mark, `true` when the reviewing key and the authoring
  key act for one person. It carries no key id, key name, principal or crew, on any surface, to
  any reader. That is the answer to #175's "a memory approved by a crew or by a self-review
  shows that, not a person": a crew's reviewer reads `held_by: "agent"`
  ([ADR-0012](ADR-0012-admission-and-review.md) d2), a person reads `held_by: "human"`, and a
  person approving their own agents' writes
  ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
  d8) reads `same_user: true`. No reader is shown a person's name as an approver, since no
  reader is shown one today. The cost is ADR-0037 d5's, now on every memory: each reader of a
  memory learns whether an agent or a person reviewed it, and whether that was the author's
  own person. `same_user` is already theirs to read on `GET /v2/events`; `held_by` on a memory
  is new.

- **d3 — The approver is the approving verdict that ratified the version.** A promotion's
  event names it as `ratification_event_id`. Under the one policy built, k = 1
  ([admission](../architecture/admission.md)), that is the version's only approving verdict.
  `approved_at` is that verdict's own `at`, which is at or just before `promoted_at`: the two
  events are sealed in one commit. ADR-0012 d3 lets a policy require more than one verdict, and
  no record says which of them a reader is shown. This record's reading is the last, whose
  arrival met the policy. No such policy is built, and the reading waits for the operator
  (Open questions, 2).

- **d4 — An anonymous `universe` row carries neither, and a federated hit carries its own
  row's.** ADR-0044 d9 lists what stays on a `universe` version served to a key outside its
  authoring key set: the id, `claim_key`, `version`, `kind`, `text`, `tokens`, `created_at`,
  `promoted_at` and `plaintext_digest`. The list names no approval, so `approved_at` and
  `approver` are null wherever `principal` is, as `author_key_id` and `scan_finding` are. Its
  author is served its own row as every author is. A hit from a federated source
  ([ADR-0072](ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md)
  d8) is a row the key already reads, served as any hit of its workarea is. The cost: a reader
  of `universe` learns when a curated claim was promoted and nothing of how it was reviewed.

- **d5 — A hit served unreviewed carries neither, and every other hit's reviewer is not its
  author.** The caller's own held rows, and other authors' held rows inside an in-progress
  workarea ([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
  d3, d4), are served with `unreviewed: true` and `approver` null. A law holds R2 on the read
  path: for every hit not served unreviewed, the journal's approving verdict is by another key
  and another principal than the author's, and the served `approved_at` is that verdict's. The
  build marks no row `self_signed`: ADR-0025 d17's development exception is not built. So the
  law has no exception today, and a build of that exception states its own.

- **d6 — What a read serves of the approval is stored on the version's row when it is
  promoted, and the reviewer is not.** The fold of `lifecycle.promote` and
  `lifecycle.supersede` writes three columns of `memories` beside `promoted_at` (migration
  0044): `approver_held_by`, `approver_same_user` and `approved_at`. A read of a memory
  therefore reads no journal row and no key row: the guardrail read finds its verdicts by one
  pass over the tenant's chain (`App::approvals`), which a recall of up to 100 hits cannot pay.
  The row names no reviewing key and no principal, so the reviewer's identity stays in one
  place, the journal's review events. The migration fills rows promoted before it from the
  journal: each version's latest promotion event and the verdict it names, as
  `ratification_event_id` or, as v0.1.0 wrote it, `verdict_event_id`. A promotion whose event
  names no verdict the journal holds leaves its row unfilled, and the version is served with
  `approver` null. `approver_held_by` is the attestation when the verdict was given; no act
  changes a key's `held_by` after issuance. The cost: three nullable columns, and one update of
  every promoted row at the upgrade, unmeasured.

- **d7 — A pseudonymised approver stays pseudonymised in recall, because recall never held
  it.** #146's act erases or pseudonymises the attribution a deployment keeps. This record
  adds none: `memories` gains no reviewer key or principal (d6), and no read of a memory
  serves one (d2). The reviewer's identity is where it was, in the envelope of the
  `review.approve` event, which #146 has to cover with every other journalled actor whatever
  this record does.
  `held_by` and `same_user` name no one. If a later record serves a reviewer's identity (Open
  questions, 1), it names the field it reads, and that field joins #146's inventory with it.

## Open questions

1. **Should a reader who is shown the author be shown the reviewer's principal too?** #175's
   outcome says "the reviewer identity … so a reader sees who approved a memory". ADR-0025 d15
   and ADR-0037 d5 withhold it from every reader, and this record keeps to them. Serving it
   would amend both and ADR-0044 d9's sentence, and needs its own rule for MCP, whose claim
   item names no author. It would be one more member of `approver` and a stored field for it
   to read, which d6 leaves out.
2. **Which verdict is the approver under a policy with k > 1?** d3 reads it as the last, whose
   arrival met the policy. The alternative is every approving verdict, as ADR-0037 d5 serves a
   guardrail's. Decided with the build of such a policy.
3. **Is `held_by` on every memory acceptable?** d2 extends ADR-0037 d5's disclosure from
   guardrails to memories. Without it, `approver` is `{same_user}` and a crew's reviewer cannot
   be told from a person.

## Consequences

- `docs/api/openapi.yaml` gains `MemorySummary.approved_at`, `MemorySummary.approver` and the
  `Approver` schema, and the generated SDK layers follow it.
  [Audit and lineage](../architecture/audit-and-lineage.md) says what a reader is shown.
- Migration 0044 adds the three columns and the fill. An older server refuses to start once it has
  run, as after any migration ([upgrades](../operations/upgrades.md)).
- `MetadataFold::MemoryPromoted` and `MetadataFold::TipSuperseded` carry the approval, so both
  stores write it in the promotion's own commit; a port law holds them equal.
- The web console's card still says "approved by a key outside its lineage". Showing the kind
  there is a separate change.
- Laws, in `crates/bins/oml-server-lead/tests/recall_approver.rs`:
  `a_hit_says_when_it_was_approved_and_whether_a_crew_a_person_or_the_authors_own_person_approved_it`,
  `get_and_recall_serve_the_same_approval_over_http_and_over_mcp`,
  `the_reviewer_differs_from_the_author_on_every_hit_that_is_not_served_unreviewed`,
  `no_read_of_a_memory_names_its_reviewer`,
  `a_universe_hit_carries_no_approval_for_any_key_but_its_authors`,
  `an_unreviewed_hit_carries_no_approval_and_gains_one_when_it_is_approved`,
  `a_successor_carries_its_own_approval_and_the_version_it_replaced_keeps_its_own`,
  `migration_0044_fills_from_the_journal_exactly_what_a_promotion_stores` and
  `migration_0044_reads_a_promotion_that_names_its_verdict_as_v0_1_0_did`; and, for every
  journal store,
  `a_promotion_stores_the_approving_verdict_on_its_row_and_a_held_version_holds_none`.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Serve the reviewer's principal and key id in the form the author's are served** | It reverses ADR-0025 d15's rejected alternative and ADR-0037 d5 inside a build change. It is Open question 1, for the operator. |
| **Serve the reviewing key's crew** | A crew's name is not served on any read of a memory, and it narrows who the reviewer is. |
| **One `kind` member: `person`, `crew` or `self_review`** | A third vocabulary for two marks the API already has; a self-review by an agent's key and by a person's key would read alike. |
| **Read the approval from the journal at each read** | One pass over the tenant's chain per recall, as the guardrail read pays for a handful of rules. |
| **Join `api_keys` at each read for `held_by`** | A second table on every memory read, for a value the row can keep. |
| **Store the reviewing key's id on the row** | No read serves it, and a second copy of who reviewed is one more field for #146 to erase. |
| **Serve `promoted_at` as the approval time** | It is the promotion's time. They differ by the gap between two seals today, and need not under a policy that ratifies later than its last verdict. |
| **Leave rows promoted before the migration unfilled** | Every memory a server holds at the upgrade would read as approved by no one. |
| **Serve `approver` on an anonymous `universe` row** | ADR-0044 d9's list is closed; adding to it is that record's to decide. |
