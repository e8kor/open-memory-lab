<!--
id: engineering/workarea-lifecycle
tier: engineering
status: design review · decided, not built
answers: what a full development lifecycle for a workarea looks like from a developer's standpoint; what OML offers today at each stage, with the decision and the line of code that offers it; what is missing or causes friction; which of the four constitutional rules constrains each stage; which proposed record answers each gap; how development, staging and production map onto one server or two; the operator's decisions of 2026-11-14; and how each question it raised was answered
-->

# The workarea lifecycle: develop, test, verify, publish, move and roll back

**Status:** design review · decided, not built · 2026-11-14, revised the same day after review
and the operator's four decisions; the operator accepted ADR-0084 to ADR-0087 and ADR-0089 the
same day, with every recommendation below, and kept ADR-0088 deferred

On 2026-11-14 the operator asked to "review policy from development stand point so we can develop
workarea first, test it, verify against other in progress and published workareas and then promote
it to production". Asked what promotion to production means, the operator answered that it means
both same-server publishing and a move between servers. The operator added: "We need to have
proper sdlc cycle for it".

This page is that review. It follows a team through every stage of a workarea's life. For each
stage it says:

- what exists today, citing the decision and the code that hold it;
- what is missing;
- which constitutional rule constrains the stage;
- which record answers the gap.

The decisions themselves are in six records, five accepted on 2026-11-14 and one deferred:

| Record | Decides | Order |
|---|---|---|
| [ADR-0085](../docs/adr/ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) | releases, publishing, key binding, withdrawal, the end of admin content acts | first |
| [ADR-0086](../docs/adr/ADR-0086-a-workareas-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates.md) | test suites as claims, server runs, the ordered gates | second |
| [ADR-0087](../docs/adr/ADR-0087-an-impact-report-shows-under-the-callers-own-reads-what-a-candidate-release-changes-for-the-workareas-that-receive-it.md) | the impact report on other workareas | beside ADR-0086 |
| [ADR-0084](../docs/adr/ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) | the bundle, export, and an import that lands held | third |
| [ADR-0089](../docs/adr/ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) | the admin fixes production at once, marked, confirmed by a person afterwards | with ADR-0085 |
| [ADR-0088](../docs/adr/ADR-0088-an-imported-version-counts-as-reviewed-only-from-a-trusted-source-a-tenant-pinned-and-never-for-instructions.md) | verdicts from a trusted source server | deferred |

Citations are `ADR-nnnn dN :line` for a decision record in `docs/adr/` and `path:line` for code, at
`origin/main` 198bb6d8. ADR-0080 to ADR-0083, accepted the same day on the branch
`design/workarea-builder` (#374), are cited by number only.

## The operator's decisions of 2026-11-14

1. **Team delivery through shared development workareas.** A team develops in a shared workarea
   such as `payments.dev`, written with persons' tenant keys, and binds to its releases. No verb is
   added beyond import, whose same-server form makes a development copy (ADR-0084 d14). Personal
   roots stay personal (ADR-0044 d3 unchanged).
2. **Binding wins over in progress.** A key bound to `latest` or to a pinned release is served the
   release manifest by artifact id, whatever the workarea's marks, and never sees held items. Only
   `tip` keys are served as ADR-0030 d3 and ADR-0080 d10 serve.
3. **A release may be cut while the workarea is in progress.** It is cut from approved versions.
   Held work by others is not in it.
4. **The admin fixes production.** In the operator's words: "admin should be the person who can fix
   workareas on prod, so, all types of action, demote, edit, rollback imports for example". A fix
   takes effect at once and is marked; an admin reads claim text tenant-wide, as ADR-0045 d21
   already allows, each read journalled with a reason. This replaced, the same day, an earlier
   decision that admins keep erasure only. It is ADR-0089: the third bounded exception in ADR-0001 d5,
   of R2's order and R3's first sentence, for admin keys; R1 does not change.
5. **Releases govern every kind, memories included.** An agent on `latest` or a pin receives a newly
   approved memory only through a release. Its own held writes are still served to it, and writes
   check against the promoted tip (ADR-0085 d5).
6. **An admin fix reaches release-bound keys as a marked overlay** on every release that lists the
   claim; the release reads degraded until a person confirms (ADR-0085 d6, ADR-0089 d5).
7. **Loosening a guardrail is one person's act, with an alert,** for an admin's fix and a
   developer's withdrawal alike (ADR-0085 d9, ADR-0089 d6).
8. **Only the key's issuer or its own person widens a binding;** an admin only narrows; every change
   is journalled (ADR-0085 d7).

## The four rules

R1 to R4 are declared in [ADR-0001](../docs/adr/ADR-0001-the-constitution.md) and not restated
here: R1 is the verb ceiling, R2 the reviewer who is never the author, R3 the gate that changes
visibility, not ranking, with its in-progress exception (ADR-0030 d3), and R4 no silent deletion.
Two facts this page leans on: a version is promoted by a verdict of a dreamer key, a person's or a
reviewer crew's (ADR-0012 d2), never the key or principal that wrote it; and ADR-0089 adds the third bounded
exception, an admin's marked fix that a person confirms afterwards.

Two facts about today shape the whole lifecycle:

- **A promotion is a publication.** Once a version is promoted, every key with reader access on its
  scope recalls it on the next request. ADR-0030 d6 :147 makes the commit the only boundary, and
  only for in-progress workareas.
- **`validated` freezes.** A frozen workarea refuses supersession (ADR-0009 d5 :61;
  `crates/oml-domain/src/ladder.rs:235`), so it cannot be the place where living knowledge is
  released. ADR-0047 d10 :173 reached the same conclusion for code releases.

## The running example

The payments team works in the shared workarea `payments.dev`. The admin created it and issued
each person tenant keys `held_by: human`: `alice-dev` and `bob-dev` (writer) and `alice-dev-review`
and `bob-dev-review` (dreamer). alice marked it in progress with `alice-dev-review`. The admin issued
the developers' agent keys at `tip`, which ADR-0085 d7 allows because their scope is inside the
in-progress root, and the production agents' keys at `latest`. In the console, each person acts with
the tenant key linked to their sign-in (ADR-0085 d16). alice, bob and their agents write:

- three skills: `refund-flow`, `ledger-lookup` and `incident-triage`;
- two guardrails: never `git push --force` on `main`, and ask before `psql` against production;
- a runbook document, `doc/runbooks/refunds`;
- test cases and memories.

Because releases govern every kind, a memory bob approves on Monday reaches the production agents
only with the next release; the developers' `tip` keys see it at once. alice also keeps a personal experiment in `alice.scratch`, which stays in her root.

## The lifecycle at a glance

```text
 plan ──► develop ──► test ──► verify ──► review ──► propose ──► publish ──► observe
 (change    (in        (advisory  (impact     (per item,  (person A;  (person B;  (release-
  set,      progress,   runs on    report,     R2)         gate runs   gates       labelled
  computed) tip keys)   held)      ADR-0087)               attached)   pass)       telemetry)
                                                                          │
                                       latest-bound keys receive it ◄─────┤
                                                                          │
                         export (ADR-0084) ──► import on target, held, in progress ──► review ──► publish
                                                                          │
                     rollback: withdraw the release (ADR-0085 d9), then revert the tip (d11); nothing deleted
```

## Stage 1. Plan: the change set

**Exists.**
- A person's key creates a workarea inside the person's own root (ADR-0044 d6 :187;
  `crates/oml-app/src/app.rs:3463`). An admin key creates one in the shared namespace
  (`app.rs:3478`).
- Bulk review takes a listed set of held ids in one act, and each item is its own verdict
  (ADR-0055 d1 :46, d3 :69, d4 :78; `app.rs:5826`, at most 200 ids). The list filters by subtree,
  author key or claim-key prefix (`app.rs:5663`).

**Missing.** Nothing groups the versions of one piece of work. Nothing diffs a workarea against a
previous published state, because no published state exists.

**Rule.** R2 keeps each verdict per item (ADR-0055 d4).

**Answer.** ADR-0085 d11. A change set is computed review UI: the subtree's versions since the
latest release, with a line diff per claim key. It is never stored and has no event and no note,
so it adds no second verdict path.

## Stage 2. Develop: in progress, shared or personal

**Exists.**
- A person's own dreamer key marks a `development` subtree in progress (ADR-0030 d1 :62;
  `crates/oml-domain/src/in_progress.rs:174`).
- Inside it, keys with reader access recall the others' held memories, marked unreviewed
  (ADR-0030 d3 :105; `app.rs:464`). Rows held by the scan stay author-only (ADR-0074 d20 :1073).
- Held skills and guardrails are served there too, and a held rule asks and never denies
  (ADR-0080 d10).
- Curators reject or retire there and never approve (ADR-0030 d9 :187).
- The builder previews a write without storing it (ADR-0082 d5).

**Missing.**
- A `committed` workarea cannot be marked, because the whole subtree must be `development`
  (`in_progress.rs:154`).
- Before decision 3, an in-progress workarea served nothing outside until its commit
  (ADR-0030 d2, d7). A team that developed continuously could therefore never publish.

**Rule.** R3's in-progress exception (ADR-0030 d3) makes iteration possible. R2 holds there without
exception. Inside alice's own root, her second dreamer key may approve her agents' work, marked
`same_user` (ADR-0046 d2 :62).

**Answer.** Decisions 1 and 3, in ADR-0085.
- `payments.dev` stays in progress for as long as the team works.
- A release is cut from its approved versions while the workarea stays in progress (ADR-0085 d3,
  amending ADR-0030 d7). The cut is refused if its subtree crosses into a second in-progress root.
- A development copy of a release on the same server is a streamed import (ADR-0084 d14), with no
  return path.

## Stage 3. Environments

**Exists.**
- One server.
- `development` with `in_progress` serves as a development environment.
- `committed` and `validated` are trusted federation sources (ADR-0072 d1 :204;
  `crates/oml-domain/src/federation.rs:29`).
- Nothing corresponds to staging.

**Rule.** R3 rules out "held but visible to testers" outside in progress. R1 makes publishing a
person's act.

**Answer.**

| | One server (the recommended default) | Two servers |
|---|---|---|
| Development | `payments.dev` in progress, read by `tip` keys | the same, on the staging server |
| Staging | the promoted tip, read by `tip` keys, tests and the impact report | the staging server's releases |
| Production | the latest release, read by `latest` and pinned keys (ADR-0085 d5, d6) | the production server, after export, an import that lands held (ADR-0084), its own review and its own release |
| Promotion | publish: person A proposes, person B ratifies (ADR-0085 d4) | the same, plus export and import; a trusted source server would spare the second review (ADR-0088, deferred) |
| Isolation | grants and bindings; one database, one journal, one scan version | separate databases, keys, people, server versions and blast radius |
| Cost | lowest: no copies, one review; needs releases and binding | a second server, identities that do not cross, a second review per move, drift to watch |

## Stage 4. Releases and binding

**Exists.**
- `as_of` reads the tip at an instant under today's access, retirement and erasure (ADR-0059 d14
  :357, d15 :369; `app.rs:1969`; `migrations/0041_as_of_tips.sql`).
- No release object exists (#176 deferred; ADR-0059 :402).
- ADR-0047 d10 :173 proposes a frozen release child for code.

**Rule.**
- R1 and R2: a release changes what agents receive, so it takes two persons.
- R3: a release never carries a held row.
- R4: a version demoted or erased after the cut drops out of every release that lists it.

**Answer.** ADR-0085:
- **d1:** a release is a manifest of promoted versions by artifact id, fixed when it is proposed.
  It never holds a held or simulation row, or an unconfirmed admin fix.
- **d2:** a release is "published"; a version is "promoted".
- **d4:** two persons publish. CI never proposes or ratifies.
- **d5:** a release governs every kind for the keys bound to it. The cost is said plainly: approved
  memories reach those keys only at the next release. A key's own held writes are still served to
  it.
- **d6:** each key binds to `tip`, `latest` or a pin, resolved per workarea; binding wins over
  in-progress marks; an admin fix overlays every release that lists its claim.
- **d7:** the key's issuer or its own person widens a binding; an admin only narrows it.
- **d8:** a consumers read shows counts to the ratifier and key names to admins.
- **d9:** one person withdraws a release; a withdrawal that loosens a guardrail raises an alert
  until a second person acknowledges it.
- **d10:** release notes are generated and flag breaking changes.
- **d13:** publishing stays with persons. The admin release view reads every release and rolls one
  back as a fix (ADR-0089).
- **d14:** ADR-0047 d10 is answered without a frozen child.
- **d15, d16:** a released in-progress workarea federates at its latest release, and the console acts
  with linked tenant keys (both decided by the operator on 2026-11-14).

## Stage 5. Tests

**Exists.**
- The recall eval measures its own 200-memory corpus (`docs/operations/recall-eval.md:10`). Its
  `--workarea` option loads that corpus into a workarea; it does not evaluate what the workarea
  holds (`eval/recall/run.py:156`).
- The real-usage replay is admin-only and has no workarea filter
  (`crates/oml-app/src/app/usage.rs:257`).
- Guardrails are decided on the client from the session cache (`crates/oml-guardrail/src/check.rs:118`).
  No command evaluates a single tool call.

**Rule.**
- R2: a test case decides what a release is held to, so the case is reviewed. Inside alice's own
  root, her second key may approve her case, marked `same_user`; elsewhere a reviewer outside its
  lineage that is not a key of the author's person approves it, a reviewer crew's agent key
  included (ADR-0086 d1).
- R3: a run reads only what its caller reads.

**Answer.** ADR-0086:
- **d1:** test cases are `test` claims in four shapes.
- **d2:** the server runs them under the caller's key and records a digest of the caller's read
  scope.
- **d3:** runs are synthetic. They add no `recall_events` and move no rank and no `recalls` meter.
  The journal records only case ids and outcomes.
- **d4:** a guardrail case judges the signed `effect`, not `effect_inside`.
- **d5:** a gate run counts only against the candidate manifest. Runs on in-progress content are
  advisory.

## Stage 6. Verify against other workareas

**Exists.**
- The near-copy check reads the target's recall scope, `universe` and the writer's own held rows
  (ADR-0050 d1 :115; `app.rs:5040`).
- Guardrail conflicts are found only between rows that share a claim key
  (`crates/oml-domain/src/guardrail/mod.rs:190`; ADR-0037 :124).
- The doctor reports skill shadowing along a chain (ADR-0080 d5).

**Rule.** R3 and ADR-0072 d6: verification reads only what the caller reads. Another person's held
or in-progress work is not shown, not even as a count.

**Answer.** ADR-0087:
- **d1:** the report runs under the caller's key, and its header says so.
- **d2:** it covers the receiving scopes the caller reads.
- **d3:** near-copies across those workareas. MinHash runs per workarea, so the cost is a
  bottleneck row, and the report is computed off the request path.
- **d4:** name collisions per receiving chain the caller can read.
- **d5:** guardrail decision changes, checked only against the suite's guardrail cases.
- **d6:** a per-scope diff of skills, guardrails and steering.
- **d7:** blocking findings.
- **d8:** contradictions stay out of scope until a detector has been measured.

## Stage 7. Gates and publishing

**Exists.**
- Per-item review under R2 (`model.rs:232`).
- The commit is refused while anything is held (ADR-0030 d6; `app.rs:4202`,
  `crates/adapters/oml-pg/src/lib.rs:3791`).
- Ladder moves are proposed and ratified by different keys. Today an admin may ratify
  (ADR-0009 d3 :47; `ladder.rs:116`, `:181`).
- The scan runs at write (ADR-0074 d1 :270).

**Answer.** ADR-0086 d6 sets the gate order:

1. scan;
2. review;
3. tests, run against the candidate;
4. impact, acknowledged;
5. publish, by a second person.

Only the ratifier may override a gate, with a reason, journalled as `workarea.gate_override`. CI
attaches runs and nothing else (d7). The commit is not a gate, because decision 3 lets a release
leave an in-progress workarea before the commit.

## Stage 8. Deploy and observe

**Exists.**
- Recall telemetry and outcome labels (`migrations/0023_recall_telemetry.sql`;
  `crates/oml-app/src/telemetry.rs:60`; ADR-0054 d6 :386).
- Guardrail decisions are made on the client and never reported (ADR-0037 d9 :209).

**Answer.**
- `recall_events` records which release a read was served from (ADR-0085 d12).
- A guardrail-hit report from the hook needs its own opt-in record or an ADR-0037 amendment. It is
  not written yet.

## Stage 9. Roll back

**Exists.**
- Supersede and retire, each a held proposal that another person approves (`app.rs:7708`).
- Reject, which keeps the bytes.
- Demote and erase are designed and not built (`crates/oml-domain/src/model.rs:139`).

**Rule.**
- R4: nothing is deleted.
- R1: no agent demotes; an admin does, as a marked fix (ADR-0089).
- R2: a developer's fix needs another reviewer; an admin's fix is reviewed afterwards.

**Answer.** ADR-0085:
- **d9:** withdraw the release. `latest` keys return to the previous release at once.
- **d11:** revert the tip. This drafts the inverse change set, which is reviewed and published as a
  new release.
- ADR-0089: when the fix cannot wait, an admin demotes, edits, retires, rolls back the release or
  rolls back an import at once. The fix is marked to readers, journalled with a reason, and queued
  for a person on the team to confirm or revert.

## Scenario: a bad publish found in production

`payments.dev@1.4.0` is published on Tuesday. On Wednesday an on-call agent, bound to `latest`,
follows the new `refund-flow` skill and asks to refund twice: the skill's step 3 is wrong.

1. **The developer withdraws the release** (ADR-0085 d9). bob, on the team, withdraws 1.4.0 with a
   reason. One person suffices; had 1.3.0 lacked a guardrail 1.4.0 has, the withdrawal would still
   take effect and raise an alert until alice acknowledges it. Every
   `latest` key reads 1.3.0 on its next request and loads 1.3.0's skills at its next session start.
   Nothing is deleted: 1.4.0 stays listed as withdrawn.
2. **The developer fixes forward.** alice runs `oml workarea revert --to 1.3.0` for the one skill,
   or writes a corrected `refund-flow`. bob reviews it, the tests and impact report run against the
   candidate (ADR-0086, ADR-0087), and alice proposes 1.4.1, which bob publishes.
3. **When the fix cannot wait.** Suppose instead that the bad step shipped in 1.3.0 already, so a
   withdrawal would return readers to the same text; the developers' `tip` keys read it too; and it
   is 03:00, with nobody on the team answering. The tenant's admin demotes the bad `refund-flow` version with a reason
   (ADR-0089 d2). At once, `tip` keys are served the previous promoted version, and every release
   that lists the bad one, 1.3.0 and 1.4.0 and any pin to them, serves the previous version as a
   marked overlay (ADR-0089 d5): `approver.act: "admin_fix"`, `confirmed: false`, and the releases
   read `degraded`. The doctor and `oml_admin_fixes_unconfirmed` show one open fix, and no one can
   cut 1.4.1 over it until it is confirmed.
4. **The team confirms or reverts the admin's fix.** In the morning, bob sees "Fixes to confirm" on
   his Personal Review page (`GET /v2/review/fixes`, which never names the admin), reads the reason,
   and confirms. The mark's `confirmed` turns true and the releases read `fixed`. Had the admin been
   wrong, bob would revert: the server writes a new version carrying the demoted text under the fix
   id, promoted by bob's verdict, journalled as `admin.fix_reverted`. Either way, nothing is
   deleted, and alice then fixes forward and cuts 1.4.1.

## Stage 10. Audit

**Exists.**
- A signed, hash-chained journal (`migrations/0001_init.sql:88`, append-only at :111;
  `crates/oml-domain/src/chain.rs:105`, `:213`).
- `oml doctor` (`crates/bins/oml-cli/src/main.rs:5132`).

**Answer.**
- New events, each carrying the digest of its manifest or report:
  - `workarea.release_proposed`, `workarea.release_ratified`, `workarea.release_withdrawn`,
    `workarea.release_withdrawal_acknowledged`;
  - `key.binding_set`;
  - `workarea.test_run`, `workarea.gate_override`, `workarea.gates_set`, `workarea.impact_report`;
  - `workarea.exported`, `workarea.imported`, `import.erasure_notice`, `export.allowance_set`;
  - `admin.fix`, `admin.fix_confirmed`, `admin.fix_reverted`, and `memory.admin_read` with a reason.
- A bundle carries the source's signed events for every version it holds (ADR-0084 d1).

## Proposed changes in priority order

| # | Change | Record |
|---|---|---|
| 1 | Releases governing every kind, cut and publish by two persons, binding, admin-fix overlays, withdrawal with an alert, revert, release notes, consumers read, the admin release view and rollback | **ADR-0085**, new |
| 2 | Test suites as claims, server runs, ordered gates and overrides | **ADR-0086**, new |
| 3 | Impact report | **ADR-0087**, new |
| 4 | Bundle, export, an import that lands held, the same-server copy | **ADR-0084**, new |
| 5 | Admin fixes in production: demote, edit, retire, release and import rollback, ladder moves, erasure; marked, confirmed afterwards; admin reads with a reason | **ADR-0089**, new |
| 6 | Verdicts from a trusted source server | **ADR-0088**, new, deferred |
| 7 | Opt-in guardrail-hit report | a later record or an ADR-0037 amendment |
| 8 | Erasure built | none needed: ADR-0005 decides it; demotion is built with ADR-0089 |

## The questions, answered

The operator's eight decisions are recorded above. The questions that remained were each put with
the recommendation first, and on 2026-11-14 the operator accepted every recommendation with the
records; question 10 stays open with the deferred ADR-0088:

1. **Default binding:** `latest` for agent keys and `tip` for person keys (ADR-0085), or `tip`
   everywhere?
2. **A cut over an unconfirmed admin fix:** refused (ADR-0085 d1), or allowed and marked?
3. **A pin to a withdrawn release:** served and marked, or refused?
4. **Breaking changes:** force a major label, or only flag them?
5. **Gate overrides:** by the ratifier with a reason (ADR-0086 d6), never, or with a second person?
6. **Test cases:** reviewed claims, or files synced from a repository?
7. **Who exports:** a person's key only, or also a CI key with an `exporter` profile (ADR-0084)?
8. **Encryption:** always, or plaintext allowed for a same-host move?
9. **Default export policy:** `own_root`, `off` or `allowed`?
10. **Verdicts from another server:** may they ever count (ADR-0088, deferred)?
11. **Admin edits:** allowed in every workarea, or only in one with published releases (ADR-0089)?
12. **Unconfirmed admin fixes:** stay served and marked indefinitely (recommended), or expire?
13. **Admin keys that fix:** `held_by: human` only (recommended, ADR-0089 d1)?
14. **Frozen workareas and `universe`:** admin edits refused when frozen, allowed and marked in
    `universe` (ADR-0089 d12, d13)?
15. **Cross-server attestation:** a reviewer of an imported version attests "did not author this on
    the source" (ADR-0084 d4)?
16. **Federation of a released in-progress workarea** at its latest release (ADR-0085 d15)?
17. **Console acts through linked tenant keys** (ADR-0085 d16)?
18. **Self-confirmation of an admin fix** in a development scope, marked `same_user`, with another
    principal confirming in a production scope (ADR-0089 d8)?
