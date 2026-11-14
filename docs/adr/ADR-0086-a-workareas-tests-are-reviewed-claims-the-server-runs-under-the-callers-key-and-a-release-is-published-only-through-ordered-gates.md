<!--
id: adr/ADR-0086-a-workareas-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates
tier: engineering
status: accepted design · 2026-11-14
answers: what a workarea test case is and how it is reviewed; who runs a suite, under which key and against which content; what a run records and what it must never record or count; how a guardrail case reads a rule served inside an in-progress workarea; which runs count as a gate and which are advisory; the order of the gates before a release is published, who passes each and who may override it; what CI may do; and the questions left for the operator
-->

# ADR-0086 — A workarea's tests are reviewed claims the server runs under the caller's key, and a release is published only through ordered gates

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with its two questions decided at their
recommendations, the override route added (#384), and every other default as drafted · proposed
the same day

**Release:** not scheduled; after ADR-0085

**Twin parts** (ADR-0090 d18, PR #388): test runs, their case results and the gate records, overrides included, are journal or part, never excluded; the build names each column's class under ADR-0090 d4.

**Amends:** ADR-0012 (d3: the review policy object gains the gate list), ADR-0054 (d6: a test run
records no recall event and moves no rank)

## Context

A person building a workarea cannot write down "this query must find the refund runbook in the top
three", "`git push --force origin main` must be denied" or "the `refund-flow` skill must render",
run it before review, or see it fail when a later change breaks it. The recall eval measures its
own 200-memory corpus and does not evaluate what a workarea holds (`eval/recall/run.py:156`,
`docs/operations/recall-eval.md:10`); the real-usage replay is admin only and has no workarea filter
(`crates/oml-app/src/app/usage.rs:257`); the guardrail check runs on the client from a session
cache with no "evaluate this call" command (`crates/oml-guardrail/src/check.rs:118`). The scan, review,
commit and a release proposal (ADR-0085) are separate acts with no order and no record linking them.

Held against R1 (a gate a person must pass is passed by a person), R2 (a test case changes what a
release is held to, so it is reviewed content) and R3 (a run reads only what its caller may read)
([ADR-0001](ADR-0001-the-constitution.md)). Decided, not built.

## Decision

- **d1 — A test case is a claim of kind `test`, claim key `test/<name>`, in one of four shapes.**

  | Shape | Body | Passes when |
  |---|---|---|
  | `recall` | a query, optional filters, expected claim keys, `within` k, optional `absent` keys | every expected key is in the top k and no absent key is |
  | `guardrail` | a tool name and its input, as a PreToolUse payload | the decision under the signed `effect` equals `allow`, `ask` or `deny` (d4) |
  | `skill` | a skill name, optional `shadows` | it parses, fits ADR-0080 d9's size, is offered at its workarea, and shadows only what `shadows` names |
  | `document` | a document key | it parses, its passages fit ADR-0081 d3's limits, and its `doc/` links resolve |

  A case is reviewed like any claim (R2): one written with the change it tests is held with it.
  Inside the author's own user root, the author's second dreamer key may approve it, marked
  `same_user` ([ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) d2);
  elsewhere another person approves it. A test case is never served as a hit, a prompt or a resource.

- **d2 — The server runs a suite under the caller's key and records the caller's read scope.**
  `POST /v2/workareas/{workarea}/test-runs {against}` evaluates every promoted `test` claim in the
  subtree with the reads the server already has: recall with the workarea's pipeline,
  `oml-guardrail`'s `decide` over the guardrail set, the skill and document checks. It runs under
  the calling key's access, never wider (ADR-0072 d6), and records a digest of the read scope it
  ran with (the served node set and the binding resolution). A run is only as good as the scope it
  saw, and the record says which scope that was. `against` is `release:<label>`,
  `candidate:<proposal>` (d5) or `tip`, and it overrides the calling key's binding for the run;
  access still bounds it. A gate run whose read scope does not cover every node of the release
  subtree is refused as a gate, `gate.run_scope_insufficient`. A run is started from the CLI (`oml workarea test`), the
  console or CI, all through this route; a result computed on a client is never a gate.

- **d3 — A run is synthetic: it writes no recall event, moves no rank and no meter, and journals
  case ids and outcomes only.** Its reads are marked synthetic, so they enter neither
  `recall_events` nor the outcome labels (ADR-0054 d6), never feed a usage rank (ADR-0054 d7), and
  do not count on the `recalls` meter
  ([ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d2).
  A run counts once on a new `test_runs` meter. Its journal event, `workarea.test_run`, carries the
  run id, the target digest, the read-scope digest, the server version, and per case the claim key,
  the version and `pass` or `fail`. No query text, hit, tool input or decision text goes into the
  journal; the full result is served to the caller and kept for the release page for a retention set at the build.

- **d4 — A guardrail case is judged against the signed `effect`, never the in-progress downgrade.**
  Inside an in-progress workarea a held rule is served with `effect_inside: "ask"` (ADR-0080 d10,
  proposed), and a test of it reads the signed `effect`, so a suite passes or fails on what the
  rule will do once published, not on its development-time softening.

- **d5 — Gate runs count only against the candidate manifest; runs on in-progress content are
  advisory.** The candidate is the manifest a release proposal would publish (ADR-0085 d1): promoted
  versions only. A run against it is a gate run, valid for exactly that manifest digest, which is
  fixed at the proposal (ADR-0085 d1). A run under a `tip` key inside an in-progress
  workarea may read held items and is marked advisory, for the developer; it never satisfies a gate.

- **d6 — A release is published only through ordered gates, each recorded against the candidate
  digest.**

  | Gate | Runs | Passes when | Override |
  |---|---|---|---|
  | 1. Scan | at the proposal, a re-scan of every member whose `scanned_under` is older than the server's current rule set ([ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md); ADR-0065 d15) | no member draws a veto or a hold under the current rules | none: a finding is fixed by a new version, never waived |
  | 2. Review | per-item verdicts ([ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)) | every member promoted, which a manifest guarantees | none: R2 has no override |
  | 3. Tests | a gate run (d5) | every case passes and none regresses against the latest release | the ratifier, per case, with a reason |
  | 4. Impact | ADR-0087's report on the candidate, computed under the ratifier's key; that report is the one that counts | every blocking finding acknowledged | the ratifier, per finding, with a reason |
  | 5. Publish | ADR-0085 d4 | gates 3 and 4 recorded for this digest | not applicable: this is the person's act |

  An override is journalled as `workarea.gate_override {release, gate, case or finding, reason}`
  under the ratifier's key. Its route is
  `POST /v2/workareas/{w}/releases/{proposal}/gates/{gate}/override {reason, case | finding}`
  (#384), with `gate` `tests` or `impact` and the body naming the one case or finding it waives;
  the reason is required, stored in clear and scanned. Only the ratifier overrides: a person's
  dreamer key (`held_by: human`) that may publish the proposal under ADR-0085 d4, so never the
  proposer's principal, an agent key, an admin key or a service key, and the publish of a
  proposal with overrides is refused `workarea.release_refused`, reason `override_by_other`,
  unless the key that publishes made every override on it. The scan and review gates answer
  `request.validation`, `details.field: gate`: they have no override. The To do page lists a
  blocking gate on a candidate the person's key would ratify as a Gate overrides row
  (ADR-0083 d11). A workarea's review policy object (ADR-0012 d3) lists which of gates 1,
  3 and 4 apply; gates 2 and 5 always apply. The default is all of them where a suite exists, and
  gates 1 and 4 everywhere. Changing the list is a two-person act: one person's dreamer key on the
  node proposes it and another person's ratifies it, journalled as `workarea.gates_set`.

- **d7 — CI runs tests and attaches results; it never proposes, ratifies or overrides.** A CI
  service key learns of a proposal from the releases listing or the events tail (ADR-0085 d4),
  starts a run `against: candidate:<proposal>`, and attaches it; that is all it does. A service
  key's binding defaults to `latest` (ADR-0085 d7), and `against` sets what the run reads. Proposal
  and publication are two persons' acts (ADR-0085 d4).

- **d8 — Agent-driven scenario tests stay outside the gates.** A session of an agent against the
  workarea, judged afterwards, needs a model and a judge whose agreement is unmeasured. Its evidence
  can come from the release's telemetry (ADR-0085 d12) until a measured judge exists.

## Consequences

- A migration adds test runs and gate records; the kind list gains `test`.
- `oml workarea test [--against …] [--attach <proposal>]`; a Tests section on the workarea's page;
  the release page shows the gate records.
- Doctor lines: proposals with no gate run, or with one refused for scope; overrides on published
  releases.
- Metrics: `oml_test_run_seconds` (a histogram), `oml_test_cases_failed_total`,
  `oml_gate_overrides_total{gate}`.
- Each run costs one recall per `recall` case and one decision per `guardrail` case; a cap on cases
  per suite and runs per hour is set at the build, unmeasured until then.
- The operator's answers of 2026-11-14, each the recommendation: the ratifier overrides a failing
  test or a blocking finding with a reason (d6), through the route d6 names; test cases are
  reviewed claims (d1).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Client-side runs reported to the server** | The server cannot check a client's report; a gate would rest on the word of the thing it gates. |
| **Runs under a server identity that reads everything** | Would widen reads beyond the caller's grants and leak, through pass/fail, content the caller cannot read (R3). |
| **Test runs as ordinary recalls** | Would label outcomes, move usage rank and fill the meter with machine traffic, skewing ADR-0054's measurement. |
| **Gate on runs against in-progress content** | A held item could pass a test and then be rejected, so the gate would certify content that is never published. |
| **Test cases as unreviewed configuration** | Whoever writes the cases decides what a release is held to; unreviewed, one author could make any change pass. |
| **The commit as the gate point (ADR-0030 d6)** | The operator decided a release may be cut while in progress (ADR-0085 d3); the publish is the act that changes what agents receive. |
