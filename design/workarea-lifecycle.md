<!--
id: engineering/workarea-lifecycle
tier: engineering
status: design draft for review · proposed, not decided
answers: what a full development lifecycle for a workarea looks like, from a developer's standpoint; what OML offers today at each stage, with the decision and the line of code that offers it; what is missing or causes friction; which of the four constitutional rules constrains each stage; what mechanism each gap needs and which of those need a new decision record; how development, staging and production map onto one server or two; what a workarea release, a workarea test and a promotion gate would be; how a bad publish is rolled back when nothing is deleted; and the questions the operator has to answer
-->

# The workarea lifecycle: develop, test, verify, release, move and roll back

**Status:** design draft for review · proposed, not decided · 2026-11-14

On 2026-11-14 the operator asked to "review policy from development stand point so we can develop
workarea first, test it, verify against other in progress and published workareas and then promote
it to production", answered that "promote to production" means both same-server publishing and a
move between servers, and added: "We need to have proper sdlc cycle for it". This page is that
review. It walks one developer, alice, through every stage of a workarea's life, says what exists
today with the decision and the code that hold it, what is missing, which constitutional rule
constrains the stage, and what mechanism is proposed. Nothing here is decided. The one part ready to
be a decision record is the move between servers, proposed as
[ADR-0084](../docs/adr/ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md).

Citations are `ADR-nnnn dN :line` for a decision record in `docs/adr/` and `path:line` for code, at
`origin/main` 198bb6d8. ADR-0080 to ADR-0083 are proposed on the branch `design/workarea-builder`
(#374) and are cited by number only.

## The four rules, as this page uses them

| Rule | What it says here | Declared at |
|---|---|---|
| **R1** | An agent's key and an admin key never promote, sign, demote or delete. Promotion is a verdict by a person's dreamer key. | [ADR-0001](../docs/adr/ADR-0001-the-constitution.md) d1 :25; ADR-0010 d6 :68 (admin is not a reviewer); `crates/oml-domain/src/access.rs:131` |
| **R2** | The reviewer is never the author, checked against the lineage closure, per key and principal. | ADR-0001 d2 :31; `crates/oml-domain/src/model.rs:232` (`check_reviewer`); ADR-0044 d8 :237 (per key inside a user root) |
| **R3** | A held row is readable by its author only, except inside a workarea a person put in progress, where the keys connected there read it marked unreviewed. | ADR-0001 d3 :38 and its amendment in d5 :53; ADR-0030 d3 :105 |
| **R4** | Nothing is deleted silently. Rejection, retirement and demotion keep the bytes; erasure is a journalled crypto-shred. | ADR-0001 d4 :42; ADR-0005 d2 :47; ADR-0030 d12 :232 |

Two facts shape everything below.

- **A promotion is a publication.** Once a version is promoted, every key with reader access on its
  scope recalls it on the next request. There is no step between "approved" and "served"
  (ADR-0030 d6 :147 makes the commit the only boundary, and only for in-progress workareas).
- **`validated` freezes.** A frozen workarea refuses supersession (ADR-0009 d5 :61;
  `crates/oml-domain/src/ladder.rs:235`), so it cannot be the place where living knowledge is
  released; ADR-0047 d10 :173 reached the same conclusion for code releases.

## The running example

alice owns the user root `alice`. She builds `alice.payments`: three skills (`refund-flow`,
`ledger-lookup`, `incident-triage`), two guardrails (never `git push --force` on `main`; ask before
`psql` against production), a runbook document `doc/runbooks/refunds`, and memories her agents
collect while they work. Her team already reads a shared workarea `payments`, which has its own
skills and guardrails, and the tenant has federation on. When the work is good, she wants the
team's agents, which connect to `payments`, to receive it, and later she wants the same content on
the company's production server.

## The lifecycle at a glance

```text
 plan ──► develop ──► test ──► verify ──► review ──► commit ──► release ──► observe
 (change    (in        (cases    (impact     (per item,  (zero      (named,     (feedback,
  set)      progress)   run on    against     R2)         held)      immutable;  guardrail
                        server)   others)                            bind)       hits)
                                                                       │            │
                                                     export ──► import (target: in progress, held)
                                                     (ADR-0084)          └──► review ──► release
                                                                                    │
                                                    rollback: withdraw release, re-point,
                                                    then supersede; nothing deleted
```

Each stage below has the same four parts: **exists**, **missing**, **rule**, **proposed**.

## Stage 1. Plan: the unit of change

**Exists.**
- A workarea is created by a person's key inside their own root (ADR-0044 d6 :187;
  `crates/oml-app/src/app.rs:3463`), or by an admin key in the shared namespace
  (`app.rs:3478`). Its name is unique, immutable and never released (ADR-0026 d1 :81, d10 :390).
- Review takes a listed set of held ids in one act, each item its own verdict (ADR-0055 d1 :46,
  d3 :69, d4 :78; `app.rs:5826`, at most 200 ids).
- A "change" exists only as individual versions. Nothing groups "the versions alice made for this
  piece of work"; the bulk review list filters by subtree, author key or claim-key prefix
  (`app.rs:5663`), not by intent.

**Missing.**
- No change set: no record of which versions belong together, no description, no diff of the
  workarea against a previous state, no place where the reviewer sees the whole change the way a
  pull request shows it.
- No previous state to diff against, since no release exists (Stage 4).

**Rule.** R2 keeps each verdict per item (ADR-0055 d4): a change set may group the review, never
replace the per-item verdict with one signature over the group.

**Proposed.** A **change set** is the workarea subtree's versions written since its last release
(held, promoted, rejected and retirement proposals), computed, not stored: the server derives it
from the release manifest (Stage 4) and the claim versions after it. A change set page shows, for
each claim key, the released version, the candidate, a line diff (the console already computes one,
ADR-0082 d6), the scan findings and the near-copies, then the test run and the impact report
(Stages 5 and 6). Review stays ADR-0055's act: the reviewer approves or rejects the listed ids, and
the change set adds a title and description written by the author as a held `workarea.change_set`
note, so the PR shape exists without a second verdict path. **Needs a new ADR** (with releases).

## Stage 2. Develop: in progress

**Exists.**
- A person's own dreamer key marks a `development` subtree in progress (ADR-0030 d1 :62;
  `crates/oml-domain/src/in_progress.rs:174`, `may_set_mode`); the route is
  `POST /v2/workareas/{workarea}/mode` and the event `workarea.mode_set` (`app.rs:4103`).
- Inside, every key with reader access recalls the others' held memories, marked `unreviewed`
  (ADR-0030 d3 :105, d4 :125; `app.rs:464`, `served_unreviewed`). Scan-held rows stay
  author-only (ADR-0074 d20 :1073).
- Proposed: held skills and guardrails are served there too, a held guardrail asks and never
  denies, a held version only adds, and a held skill edit is served as `<name>-unreviewed`
  (ADR-0080 d10, d13).
- Agents write with `oml_write`; `oml sync` keeps documents in step with a repository
  (ADR-0057 d6 :154, d8 :209). A curator profile lets an agent reject, retire or write inside the
  subtree and never approve (ADR-0030 d8 :177, d9 :187; `in_progress.rs:239`).
- The console builder writes held versions under the person's own keys and previews a write that
  stores nothing (ADR-0082 d2, d5).

**Missing.**
- alice cannot put a skill she is changing in `payments` in progress without putting `payments`
  itself in progress: a subtree must be all `development` to be marked (`in_progress.rs:154`,
  `check_mark`), and `payments` is `committed`. Developing against a live workarea means a separate
  workarea, and there is no verb that carries the result back (Stage 7).
- Held skills and guardrails inside in progress are proposed, not built (ADR-0080 d10).

**Rule.** R3's in-progress exception (ADR-0030 d3) is what makes iteration possible; R2 holds there
without exception, so alice's own agents' writes still need a key other than the writing one. In
her user root her second dreamer key may approve, marked `same_user` (ADR-0046 d2 :62).

**Proposed.** Keep in progress as the development environment. The development copy of a live
workarea is a new in-progress workarea that imports the live one's latest release from the same
server (ADR-0084 d10, the same-server source), so alice develops against the real starting content,
and the result returns by the same import in the other direction. No new rule, no exception to R3.

## Stage 3. Environments: development, staging, production

**Exists.** One server, three workarea statuses and one mode. `development` with `in_progress` is
a development environment. `committed` and `validated` are trusted sources for federation
(ADR-0072 d1 :204; `crates/oml-domain/src/federation.rs:29`). Nothing corresponds to staging:
a promotion is visible at once to every reader. Moving content between servers does not exist; the
nearest is the person export `GET /v2/web/export` (`crates/oml-app/src/app/signup.rs:678`), a JSON
dump of a root with no import, and `oml-backup`, a whole-database backup (Stage 9).

**Missing.** A staging step between "approved" and "what agents in production receive", on either
mapping.

**Rule.** R3: nothing outside in progress may read held rows, so staging cannot be "held but
visible to testers" outside an in-progress workarea. R1: whatever moves content into production is
a person's act.

**Proposed.** Both mappings, chosen per deployment.

| | One server: environments are states and workareas | Two servers: environments are servers |
|---|---|---|
| Development | an in-progress workarea (`alice.payments`, or a development copy of `payments`) | the same, on the staging server |
| Staging | the workarea's live tip after commit, read by the keys bound to `tip` (developers, CI, test runs) | the staging server's latest release, read by staging agents |
| Production | the workarea's **latest release**, read by every key bound to `latest` (Stage 4) | the production server, after export and import (ADR-0084) |
| Promotion | cutting a release: a person's key proposes, another ratifies | export the release, import it on production into an in-progress workarea, review again (or trust a pinned source), commit, release |
| Isolation | grants and keys separate readers; one database, one journal, one scan version | separate databases, keys, people, server versions and blast radius |
| Cost | lowest: no copies, one review; needs releases and key binding (both new) | a second server to run, identities that do not exist on the target, a second review per move unless a source is pinned, drift between servers to watch; needs ADR-0084 |

The recommendation is the one-server mapping as the default path, because it is a single review
and no copy, and the two-server mapping for what one server cannot give: testing a new server
release against real content, an organisation that keeps production people and keys apart, backup
of one workarea, and sharing between tenants.

## Stage 4. Versioning and releases of a workarea

**Exists.**
- Every claim is versioned; one promoted tip per key (`migrations/0005_claim_versions.sql:166`).
- `as_of`, an RFC 3339 instant, on get, list, recall and `oml_read` returns the tip promoted at or
  before the instant, under today's access, retirement and erasure (ADR-0059 d14 :357, d15 :369;
  `app.rs:1969`; `migrations/0041_as_of_tips.sql`). An as-of recall reads its scope only and
  refuses `sources` (`app.rs:6406`). Steering is taken as of the instant (`app.rs:6429`).
- No release object exists. A pinned prefix of the journal for repeatable recall is deferred
  (#176; ADR-0059 :402). ADR-0047 d10 :173 proposes releases as a frozen `validated` child holding a
  release record, for code; ADR-0021 :91 uses the word for ontologies.

**Missing.**
- A name for "what production receives": `alice.payments@1.4.0`.
- A way for a key to receive a release instead of the live tip.
- Guardrails and skills have no `as_of` read; a guardrail set cannot be pinned.
- Release notes.

**Rule.** R4: a release never removes a version; a version demoted or erased after a release
disappears from that release too, as `as_of` already does (ADR-0059 d15), because a demotion must
reach every reader. R1 and R2: a release changes what agents receive, so it is a promotion in
effect, proposed by one person's key and ratified by another's, never an agent's, an admin's or a
service key's.

**Proposed.**
- **A release is an immutable, named manifest.** `<workarea path>@<label>`, label a semantic
  version. The manifest lists, for every claim key in the workarea (and, by choice, its subtree),
  the promoted version, its artifact id and its plaintext digest at the cut, plus the instant. The
  manifest digest is signed in a `workarea.release_cut` event. Names follow ADR-0026: unique,
  immutable, never released.
- **Proposed and ratified like a ladder move.** `POST /v2/workareas/{w}/releases` by a person's
  dreamer key or a CI service key (a proposal promotes nothing, as in ADR-0047 d7); ratification by
  a person's dreamer key that is not the proposer, `same_user` marked inside a user root
  (ADR-0046 d2). An admin key may neither, unlike today's ladder ratification (ADR-0009 d3 :47
  admits an admin), which this page flags as an inconsistency with R1 as the operator states it.
- **Agents bind to a release.** A key gains `binding`: `tip` (today's behaviour; the default for a
  key held by a person and for the keys inside an in-progress workarea), `latest` (the newest
  release that is not withdrawn), or `pinned:<label>`. Set at issuance and changed by the key's
  issuer, never by the key itself, as ADR-0079 treats a key's date. A read under `latest` or a pin
  serves the manifest's versions, by artifact id, not an instant, so two promotions in one
  millisecond cannot blur it. Federation through `sources` reads each source's latest release.
- **Release notes are generated.** From the change set: added, changed and retired items by kind,
  each with claim key, version, the reviewer's kind and `same_user` (never the reviewer's name,
  ADR-0077 d2 :65), the test run and the impact report. Stored as the release's own document claim
  (ADR-0081), so it is searchable and reviewed with the release.
- **Guardrails and skills gain `as_of` and release reads**, so the hook loads the bound release's
  set (ADR-0037 d5 :127, ADR-0080 d11).
- **Needs a new ADR: workarea releases and key binding.** It amends ADR-0009 (releases instead of
  `validated` for living knowledge), ADR-0037 d5, ADR-0059 d14 and ADR-0072 d5.

## Stage 5. Tests as gates

**Exists.**
- `POST /v1/memories/preview` (proposed, ADR-0082 d5) returns, without writing, the refusal, the
  findings, the parsed rule, the prompt name and its shadows.
- Guardrails: `oml hook pre-tool-use` decides a PreToolUse JSON from the session cache with no
  network call (`crates/oml-guardrail/src/check.rs:118`, `decide`); there is no "evaluate this tool
  call" command. `oml guardrails show` and `export --check` exist (`crates/bins/oml-cli/src/main.rs:254`,
  `:262`).
- Recall: `eval/recall/` measures a fixed 200-memory corpus with 121 queries across pipelines
  (`docs/operations/recall-eval.md:10`); `run.py --workarea` loads that corpus into a workarea and
  does not evaluate what is there (`eval/recall/run.py:156`). `POST /v2/usage/eval` replays the
  newest 500 labelled real recalls, admin only, without a workarea filter
  (`crates/oml-app/src/app/usage.rs:257`).
- ADR-0011 (simulations, not scheduled) fences synthetic writes; a test that only reads needs no
  fence.

**Missing.** alice cannot write down "the query `refund over 30 days` must find
`doc/runbooks/refunds` in the top 3", "`git push --force origin main` must be denied", or
"`refund-flow` must render under 32 KiB", nor run them before review, nor see them fail when a later
change breaks them.

**Rule.** A test case changes what a release is held to, so it is reviewed content (R2): a case
written with the change it tests is held with it, and alice cannot approve her own case. A test
run writes no claim and promotes nothing; a failing gate is waived only by a person (R1).

**Proposed.**
- **A test case is a claim of a new kind `test`**, claim key `test/<name>`, a small structured body
  with one of four shapes:

  | Shape | Body | Passes when |
  |---|---|---|
  | `recall` | a query, optional filters, expected claim keys, `within` k, optional `absent` keys | every expected key is in the top k and no absent key is |
  | `guardrail` | a tool name and input, as a PreToolUse payload | the decision equals `allow`, `ask` or `deny` |
  | `skill` | a skill name | it parses, renders under ADR-0080 d9's limit, its name is offered at the named scope and not shadowed unless `shadows` says so |
  | `document` | a document key | it parses, its passages cut under ADR-0081 d3's limit, its links to other `doc/` keys resolve |

- **The server runs the suite.** `POST /v2/workareas/{w}/test-runs {against: tip | release:<label> |
  in_progress}` evaluates every promoted `test` claim in the subtree (and, inside in progress, the
  held ones, marked) with the reads the server already has: recall with the workarea's pipeline,
  `oml-guardrail`'s `decide` over the set the bound key would load, the preview checks. The run is
  journalled as `workarea.test_run` with the manifest digest it ran against, each case's result and
  the server version. A run started from the CLI (`oml workarea test`) or CI calls the same route;
  a client-side run is rejected as a gate, because the server cannot trust a client's report.
- **Regression against the previous release.** The same suite runs against the candidate tip and
  the latest release; a case that passed on the release and fails on the tip is a regression and
  blocks (Stage 7).
- **Agent-driven scenario tests** (an agent session against the workarea, judged afterwards) stay
  out of the gate: they need a model and a judge, and their pass rate is unmeasured. The hook
  telemetry of Stage 8 is where their evidence would come from.
- **Needs a new ADR: workarea test suites as claims.**

## Stage 6. Verify against other workareas

**Exists.**
- Near-copies: `similar_to` compares word sequences and MinHash Jaccard ≥ 0.9 against the target
  workarea's recall scope plus `universe` and the writer's own held rows (ADR-0050 d1 :115, d17 :509;
  `crates/oml-domain/src/similar.rs:22`; `app.rs:5040`). It does not reach other branches of the
  tree, federated sources, or another person's in-progress workarea.
- Guardrail conflicts: flagged only when rows on one chain share a claim key with different
  severities (`crates/oml-domain/src/guardrail/mod.rs:190`); ADR-0037 :124 says plainly that a
  contradiction under different keys is not detectable. The union applies and deny wins over ask
  (ADR-0037 d8 :179).
- Skill name collisions along the ancestor chain: the nearest wins and the farther one is reachable
  as a resource; the doctor reports shadowed names (ADR-0080 d5, proposed). Documents are unique
  per workarea by claim key (ADR-0081 d2).
- What publishing changes elsewhere: a `committed` node becomes a trusted federation candidate only
  for keys that already read it and recall with `wide` or a list (ADR-0072 d4 :306, d6 :436;
  `crates/oml-app/src/app/federation.rs:483`).

**Missing.**
- No report says what a change does to agents outside the workarea: which skills they gain or lose,
  which names it shadows in a child workarea's chain, which guardrails they will start obeying, which
  recalls move.
- Contradiction between claims is not detected at all; near-copy detection does not cross the tree.
- Another person's in-progress workarea is invisible to alice (R3), so "conflicts with what Bob is
  building" cannot be checked from alice's side.

**Rule.** R3: a check run by alice's key may compare only against what her key may read; Bob's held
rows are out of reach, and a report that leaked their existence would breach R3. Access never
widens for verification (ADR-0072 d6).

**Proposed.**
- **An impact report** `GET /v2/workareas/{w}/impact?candidate=tip&baseline=latest`, computed under
  the caller's own access, journalled as `workarea.impact_report` with its digest. Sections:
  1. near-copies of each changed item across every workarea the caller reads, trusted or in
     progress (extend `similar_to`'s node set; the threshold is unchanged);
  2. name collisions: every skill name and document key in the change against the chains of every
     workarea that will receive it (the workarea's descendants under `guardrail_scope = ancestors`,
     and federation candidates), naming what shadows what;
  3. guardrail overlap: rules whose patterns match a common tool call with different decisions,
     computed by running each rule's pattern through `decide` against the other rule's examples and
     the suite's `guardrail` cases (a heuristic, said as such), plus today's same-key check;
  4. a "what changes for agents at X" diff for each receiving scope: skills added, changed, removed
     or shadowed; guardrails added or changed with their effect; steering added or ended;
  5. the regression result from Stage 5.
- **Conflicts with another person's in-progress work** are shown only as a count of claim keys both
  changes touch in workareas both may read; the content stays behind R3.
- **Contradictions** stay out of the gate until a measured detector exists; a model tier off the
  write path (ADR-0074 d13 :786) is where one would run, advisory only.
- **Needs a new ADR** (with promotion gates).

## Stage 7. Review, commit and the promotion pipeline

**Exists.**
- Review: per-item verdicts by a dreamer key, R2 against the lineage (`model.rs:232`); bulk review
  by a person's key (ADR-0055; `app.rs:562`). A skill needs a person's verdict (ADR-0080 d3).
- Commit: a person's key sets `standard` on the in-progress root; refused while anything is held
  (ADR-0030 d6 :147; `app.rs:4202`, re-checked in the transaction at
  `crates/adapters/oml-pg/src/lib.rs:3791`).
- Ladder: `development` → `committed` → `validated`, proposed and ratified by different keys
  (ADR-0009 d3 :47; `ladder.rs:116`, `:181`); refused inside an in-progress subtree
  (`app.rs:2214`).
- Scan at write: refuse secrets, hold for a person, annotate (ADR-0074 d1 :270, d2 :281).

**Missing.** The gates exist as separate acts with no order and no record tying them together: the
commit does not know whether tests ran, the ladder move does not know whether the impact report
was read, and nothing between approval and serving lets a person say "this whole change goes out
now".

**Rule.** R1 for every gate that changes what agents receive; R2 for every verdict and ratification;
R4 throughout.

**Proposed.** One pipeline, each gate recorded in the journal against the change set's candidate
manifest digest, so a later gate refuses when an earlier one ran against different content.

| Gate | Runs | Passes by | Who may override |
|---|---|---|---|
| 1. Scan | at every write (ADR-0074) | no refusal; holds go to a person | nobody: a veto is never waived |
| 2. Tests | `workarea.test_run` on the candidate | every case passes, no regression | the release ratifier, with a reason journalled per case |
| 3. Verification | `workarea.impact_report` on the candidate | blocking findings (a shadowed skill name in a receiving scope, a guardrail overlap) acknowledged | the release ratifier, per finding, with a reason |
| 4. Review | per-item verdicts (ADR-0055) | zero held in the change set | none: R2 has no override |
| 5. Commit | ADR-0030 d6 on an in-progress root | zero held | none |
| 6. Release | proposal then ratification (Stage 4) | gates 2 to 5 recorded against this manifest | not applicable: this is the person's act |
| 7. Move (optional) | export, import, gates 1 to 6 again on the target | ADR-0084 | per ADR-0084 |

Promotion to production is gate 6 on one server and gate 7 then 6 across servers. Each override
and each ratification is a person's dreamer key; no agent, admin or service key may pass a gate
that a person must pass. **Needs a new ADR: promotion gates**, amending ADR-0030 d6 (a commit may
require a passing run where the workarea's policy says so) and ADR-0012 d3 :61 (the review policy
object gains the gate list).

## Stage 8. Deploy and observe

**Exists.** Recall telemetry and outcome labels (`migrations/0023_recall_telemetry.sql`;
`crates/oml-app/src/telemetry.rs:60`; ADR-0054 d6 :386), served approval fields (ADR-0077),
skill reads logged like recall (ADR-0080 d6, proposed). Guardrail decisions are made on the client
with no network call and are not reported anywhere (ADR-0037 d9 :209).

**Missing.** Telemetry is not labelled by release, so "did 1.4.0 recall worse than 1.3.0" cannot be
asked. Guardrail hits are invisible to the workarea's owner. No signal ties a rollback to evidence.

**Rule.** No rule blocks telemetry; ADR-0077's rule that a reader never learns the reviewer's name
holds in every report.

**Proposed.** Record the release a read was served from on `recall_events`; an opt-in, counts-only
guardrail decision report from the hook (`deny`, `ask`, `allow` per rule claim key per day), sent
at session end; a release page that shows both against the previous release. **Folds into the
releases ADR**; the hook report needs its own opt-in, like ADR-0038's failure recall.

## Stage 9. Roll back a bad publish

**Exists.** Supersede (a held successor that becomes the tip when someone else approves it) and
retire (a held retirement proposal) (`app.rs:7708`; ADR-0030 d9 :187); reject keeps the bytes;
`as_of` reads the past. Demote and erase are designed (ADR-0009 d5, ADR-0010 d6) and not built:
`MemoryStatus` has the states and no transition reaches them (`crates/oml-domain/src/model.rs:139`).
No verb moves a workarea down the ladder (ADR-0009 d3).

**Missing.** A bad promotion is fixed only by writing a new version and getting it reviewed, so the
bad content stays served while the fix waits for a second person. There is no fast lever.

**Rule.** R4 forbids deleting the bad version; R1 forbids an agent or admin from demoting it; R2
means the fix needs another reviewer.

**Proposed.** Three levers, fastest first, none of which deletes.
1. **Withdraw the release.** A person's key withdraws `alice.payments@1.4.0` (journalled
   `workarea.release_withdrawn` with a reason); every key bound to `latest` reads 1.3.0 on its next
   request, and a guardrail or skill set on the next session start. The release stays listed as
   withdrawn. One person suffices, because withdrawing only returns readers to content a second
   person already ratified; this is an operator decision.
2. **Revert the tip.** `oml workarea revert --to 1.3.0` drafts the inverse change set: held
   supersessions back to 1.3.0's text and held retirements of keys 1.3.0 did not have. They go
   through review like any change and are released as 1.4.1.
3. **Demote** a version that must not be served even to `tip` readers, once demotion is built: a
   person's act, journalled, bytes kept (ADR-0009 d5).

## Stage 10. Audit and traceability

**Exists.** A per-tenant signed, hash-chained journal (`migrations/0001_init.sql:88`, append only by
REVOKE and triggers at :111; `crates/oml-domain/src/chain.rs:105`, `:213`); `oml doctor`
(`crates/bins/oml-cli/src/main.rs:5132`) and the CLI's chain verifier
(`crates/bins/oml-cli/src/verify.rs:252`); ladder moves journalled with the review policy snapshot
(ADR-0009 d3).

**Missing.** No event names a change set, a test run, an impact report, a release or a move, so
"what was in production on 3 November, who ratified it, what did its tests say" is unanswerable.

**Proposed.** The events named above (`workarea.change_set`, `workarea.test_run`,
`workarea.impact_report`, `workarea.release_cut`, `workarea.release_withdrawn`,
`workarea.exported`, `workarea.imported`), each carrying the manifest digest; a release's
provenance page that walks them; and a bundle (ADR-0084) that carries the source's signed events
for every version it holds, so a move keeps its history.

## Proposed changes in priority order

| # | Change | Stage | Decision record |
|---|---|---|---|
| 1 | Workarea releases: immutable named manifests, proposed and ratified by persons, key `binding`, withdraw | 4, 9 | **new ADR** (amends ADR-0009, ADR-0037 d5, ADR-0059 d14, ADR-0072 d5) |
| 2 | Test suites as `test` claims, run on the server, journalled | 5 | **new ADR** |
| 3 | Promotion gates: the ordered pipeline, overrides by a person with a reason | 7 | **new ADR** (amends ADR-0030 d6, ADR-0012 d3) |
| 4 | Impact report, including the cross-tree near-copy check and the "what changes for agents at X" diff | 6 | in the gates ADR |
| 5 | Workarea export and import, including same-server import as the development-copy verb | 3, 7 | **ADR-0084, proposed here** |
| 6 | Change set page and note | 1 | in the releases ADR |
| 7 | Release notes as a generated document claim | 4 | in the releases ADR |
| 8 | Release label on recall telemetry; opt-in guardrail decision report | 8 | releases ADR; the hook report a new ADR or an ADR-0037 amendment |
| 9 | `oml workarea revert --to` | 9 | in the releases ADR |
| 10 | Admin ratification of ladder moves removed, to match R1 as stated | 4 | amendment of ADR-0009 d3 |
| 11 | Demote and erase built | 9 | none: ADR-0009 d5 and ADR-0005 already decide them |

## Questions for the operator

1. **Default environment mapping.** One server with releases (recommended), two servers, or both
   as equals?
2. **What agents bind to by default.** `latest` for agent keys and `tip` for person keys
   (recommended), `tip` everywhere (today's behaviour, no staging), or a pin required?
3. **Who may withdraw a release.** One person's key (recommended: it returns readers to ratified
   content) or two, like a cut?
4. **May an admin key ratify a ladder move or a release.** Today ADR-0009 d3 admits one; R1 as
   stated says no (recommended: no, for releases at once and for ladder moves by amendment).
5. **May a failing test or a blocking impact finding be overridden.** By the release ratifier with
   a journalled reason (recommended), never, or by a second person?
6. **Test cases as claims or as files.** Claims, reviewed like everything else (recommended), or
   files in a repository synced by `oml sync`, reviewed in Git?
7. **Where tests run.** On the server, the only result that counts as a gate (recommended), or
   client runs accepted with a signature from a CI key?
8. **Release granularity.** One workarea, or a workarea and its subtree (recommended: the subtree,
   because the child-status rule already treats it as one)?
9. **The export/import decisions** listed in ADR-0084's open questions.
