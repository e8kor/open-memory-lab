<!--
id: adr/ADR-0086-a-workareas-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates
tier: engineering
status: proposed
answers: what a workarea test case is and how it is reviewed; who runs a suite, under which key and against which content; what a run records and what it must never record or count; how a guardrail case reads a rule served inside an in-progress workarea; which runs count as a gate and which are advisory; the order of the gates before a release is published, who passes each and who may override it; what CI may do; and the questions left for the operator
-->

# ADR-0086 — A workarea's tests are reviewed claims the server runs under the caller's key, and a release is published only through ordered gates

**Status:** proposed · 2026-11-14

**Release:** not scheduled; after ADR-0085

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
  the calling key's access and binding, never wider (ADR-0072 d6), and records a digest of the read
  scope it ran with (the served node set and the binding resolution). A run is only as good as the
  scope it saw, and the record says which scope that was. `against` is `release:<label>`,
  `candidate:<proposal>` (d5) or `tip`. A run is started from the CLI (`oml workarea test`), the
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
  versions only. A run against it is a gate run, valid for exactly that manifest digest; any
  promotion that changes the candidate invalidates it. A run under a `tip` key inside an in-progress
  workarea may read held items and is marked advisory, for the developer; it never satisfies a gate.

- **d6 — A release is published only through ordered gates, each recorded against the candidate
  digest.**

  | Gate | Runs | Passes when | Override |
  |---|---|---|---|
  | 1. Scan | at every write ([ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md)) | no member is held by the scan | none: a veto is never waived |
  | 2. Review | per-item verdicts ([ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)) | every member promoted, which a manifest guarantees | none: R2 has no override |
  | 3. Tests | a gate run (d5) | every case passes and none regresses against the latest release | the ratifier, per case, with a reason |
  | 4. Impact | ADR-0087's report on the candidate | every blocking finding acknowledged | the ratifier, per finding, with a reason |
  | 5. Publish | ADR-0085 d4 | gates 3 and 4 recorded for this digest | not applicable: this is the person's act |

  An override is journalled as `workarea.gate_override {release, gate, case or finding, reason}`
  under the ratifier's key. A workarea's review policy object (ADR-0012 d3) lists which gates apply.
  The default is all of them where a suite exists and gate 4 everywhere.

- **d7 — CI runs tests and attaches results; it never proposes, ratifies or overrides.** A CI
  service key may start runs and attach a gate run to a proposal, and that is all it does. Proposal
  and publication are two persons' acts (ADR-0085 d4).

- **d8 — Agent-driven scenario tests stay outside the gates.** A session of an agent against the
  workarea, judged afterwards, needs a model and a judge whose agreement is unmeasured. Its evidence
  can come from the release's telemetry (ADR-0085 d12) until a measured judge exists.

## Consequences

- A migration adds test runs and gate records; the kind list gains `test`.
- `oml workarea test [--against …] [--attach <proposal>]`; a Tests section on the workarea's page;
  the release page shows the gate records.
- Doctor lines: proposals whose gate run is stale against their candidate; overrides on published releases.
- Each run costs one recall per `recall` case and one decision per `guardrail` case; a cap on cases
  per suite and runs per hour is set at the build, unmeasured until then.
- Questions for the operator, recommendation first:
  1. May the ratifier override a failing test or a blocking finding with a reason (d6), or never,
     or only with a second person?
  2. Test cases as reviewed claims (d1), or files in a repository synced by `oml sync` and reviewed
     in Git?

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Client-side runs reported to the server** | The server cannot check a client's report; a gate would rest on the word of the thing it gates. |
| **Runs under a server identity that reads everything** | Would widen reads beyond the caller's grants and leak, through pass/fail, content the caller cannot read (R3). |
| **Test runs as ordinary recalls** | Would label outcomes, move usage rank and fill the meter with machine traffic, skewing ADR-0054's measurement. |
| **Gate on runs against in-progress content** | A held item could pass a test and then be rejected, so the gate would certify content that is never published. |
| **Test cases as unreviewed configuration** | Whoever writes the cases decides what a release is held to; unreviewed, one author could make any change pass. |
| **The commit as the gate point (ADR-0030 d6)** | The operator decided a release may be cut while in progress (ADR-0085 d3); the publish is the act that changes what agents receive. |
