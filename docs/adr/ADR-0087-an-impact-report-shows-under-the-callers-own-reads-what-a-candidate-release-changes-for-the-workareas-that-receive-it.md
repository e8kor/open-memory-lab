<!--
id: adr/ADR-0087-an-impact-report-shows-under-the-callers-own-reads-what-a-candidate-release-changes-for-the-workareas-that-receive-it
tier: engineering
status: proposed
answers: what a person sees before publishing a release about its effect on other workareas; whose reads the report runs under and what it says about that; how near-copies, skill and document name collisions and guardrail overlaps are found and in which scopes; what the per-scope diff of skills, guardrails and steering holds; which findings block; what it costs; and what it never reports
-->

# ADR-0087 — An impact report shows, under the caller's own reads, what a candidate release changes for the workareas that receive it

**Status:** proposed · 2026-11-14

**Release:** not scheduled; after ADR-0085, beside ADR-0086

**Amends:** ADR-0050 (d1: the near-copy check may run over every workarea the caller reads, for this
report only)

## Context

Publishing a release changes what agents elsewhere receive. A release can add skills that shadow a
child workarea's skill of the same name (ADR-0080 d5, proposed), add guardrails that every
descendant scope under `guardrail_scope = ancestors` obeys
([ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
d4), or repeat a memory another workarea already holds. Today nothing reports any of this.

- The near-copy check reads only the target workarea's recall scope, `universe`, and the writer's
  own held rows (`crates/oml-app/src/app.rs:5040`;
  [ADR-0050](ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md) d1).
- Guardrail conflicts are flagged only when two rules share a claim key
  (`crates/oml-domain/src/guardrail/mod.rs:190`); ADR-0037 :124 says a contradiction under
  different keys is not detectable.

Held against R3: a report run by one person may show only what that person's key reads
([ADR-0001](ADR-0001-the-constitution.md) d3; [ADR-0072](ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md) d6).
Decided, not built.

## Decision

- **d1 — The report runs under the caller's key, and its header says so.**
  `GET /v2/workareas/{workarea}/impact?candidate=<proposal>&baseline=latest` computes every section
  under the calling key's access. The header names the key, the read-scope digest (as ADR-0086 d2
  records it) and the sentence "computed over the workareas this key reads; workareas it does not
  read are not covered". The report never mentions another person's held or in-progress work, not
  even as a count: a count of overlapping keys would tell the caller that unreadable work exists
  and what it touches.

- **d2 — Receiving scopes are the workareas whose reads would serve the release, among those the
  caller reads.** These are the release node's descendants (a skill, guardrail or steering row
  reaches them through the ancestor chain), the workareas whose keys may federate to it
  (ADR-0072 d4), and the release node itself. ADR-0085 d8's consumers read gives the count of keys
  per scope.

- **d3 — Near-copies of each changed item across every workarea the caller reads.** ADR-0050's
  measure is unchanged: the same word sequence, or MinHash with an exact Jaccard of at least 0.9
  (ADR-0050 d17). Only the node set widens, to every workarea the caller reads, trusted or in
  progress, for this report only. Remembering keeps its own scope. The cost grows with the number
  of workareas and rows read; the MinHash index is per workarea today, so each changed item costs a
  band lookup in each. That is a bottleneck row with the build, unmeasured until then, and the
  report is computed off the request path and cached per candidate digest.

- **d4 — Name collisions are computed per receiving chain the caller can read.** For each receiving
  scope, the skill names offered along its chain after publication are compared with today's
  (ADR-0080 d5's nearest-wins rule). The report names every name the release would newly shadow,
  or newly be shadowed by. Document keys are compared within each receiving workarea. A chain with
  a node the caller cannot read is reported as "partly covered", without naming the node.

- **d5 — Guardrail overlap is tested only against the suite's guardrail cases.** For each
  `guardrail` test case of the workarea and of each receiving scope the caller reads (ADR-0086 d1),
  the report decides the case under today's set and under the set after publication, and lists
  every case whose decision changes. ADR-0037's same-key check is kept. No pattern analysis beyond
  the cases is attempted: a rule overlap with no case to show it is not reported, and the report
  says so.

- **d6 — A per-scope diff says what changes for agents there.** For each receiving scope: skills
  added, changed, retired or shadowed; guardrails added, changed (with their effect), retired or
  of lower severity; steering added or ended; the count of keys bound `tip`, `latest` and pinned;
  and the regression result of ADR-0086's gate run where one is attached.

- **d7 — Blocking findings and the record.** These findings block until acknowledged
  (ADR-0086 d6 gate 4):
  - a newly shadowed skill name in a receiving scope;
  - a guardrail case whose decision changes in a receiving scope;
  - any breaking change in ADR-0085 d10's sense.

  Near-copies are advisory. The report is journalled as `workarea.impact_report` with its digest,
  the candidate digest and the read-scope digest, and with no claim text.

- **d8 — Contradictions between claims are out of scope until measured.** Detecting that two claims
  disagree needs a model; ADR-0074 d13 keeps the model tier off the write path, and no detector's
  precision has been measured. A later record may add an advisory section.

## Consequences

- A cached report per candidate digest; the release page links it.
- The CLI gains `oml workarea impact <workarea> --candidate <proposal>`.
- A doctor line names proposals whose report is older than their candidate.
- Two people with different reads see different reports of one candidate. That is intended, and the
  ratifier's report is the one recorded for the gate.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A report under a server identity that reads the whole tenant** | Would show the caller content and structure its key cannot read (R3), and an admin-wide read is what ADR-0026 d15 forbids for content. |
| **A count of overlapping keys in other people's in-progress work** | The count itself discloses that unreadable work exists and what it touches. |
| **Pattern-level guardrail conflict analysis** | Rules are shell-token patterns; proving two never overlap is undecidable in general, and a heuristic without cases produces findings nobody can check. |
| **Model-based contradiction detection now** | Off the write path by ADR-0074 d13 and unmeasured; a blocking gate on an unmeasured detector would stop releases on noise. |
| **Widen the remembering near-copy scope too** | Every write would pay a tenant-wide search; the cost belongs to the publish, which happens rarely. |
