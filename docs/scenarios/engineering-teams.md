---
status: Beta
verified: 2026-11-14
answers: "how an engineering team and its agent crew build in a shared development workarea, author skills and guardrails as reviewed claims, test and gate a release, bind production agents to it, and fix and roll back production; which steps are built and which are designed"
---

# Engineering teams

The payments team, alice and bob, runs a crew of coding agents. They build the crew's working knowledge in one shared workarea, `payments.dev`: runbook memories, standing rules, skills and tests. Production agents read only what the team has reviewed and published.

The controls sit at two points. During development, nothing waits: the crew and the engineers read each other's held work at once, marked unreviewed. Toward production, a second person approves each item and, once releases are built, a second person publishes each release. The [workarea lifecycle](../architecture/lifecycle.md) draws the stages; [producers and reviewers](../concepts/producers-and-reviewers.md) says which approvals count, and its Euler views draw what each key sees.

## 1. Set up the shared workarea

An admin does this once. Built.

```sh
oml workareas create payments dev            # universe.payments.dev, written payments.dev below
oml crews create payments-crew
oml keys issue alice-dev --role writer --held-by human --principal people/alice --grant dev:writer
oml keys issue alice-dev-review --role dreamer --held-by human --principal people/alice \
  --grant dev:dreamer --grant dev:reader
oml keys issue-agent pay-agent-1 --principal crew/payments/agent-1 --workarea dev \
  --crew payments-crew --out keys/pay-agent-1
```

bob gets the same pair. Each agent gets its own key and principal, so review can tell them apart. Team keys are tenant keys, separate from each person's own root ([individual users](individual.md#7-keep-personal-knowledge-apart-from-team-work)).

Designed: in the console, the admin links `alice-dev` and `alice-dev-review` to alice's sign-in, and the console offers them for acts in `payments.dev` ([ADR-0085](../adr/ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d16). Today the console acts in a person's own root only, and team review runs from the CLI.

## 2. Develop in progress

alice puts the workarea in progress with the review key `alice-dev-review`. Built.

```sh
OML_API_KEY=$(cat keys/alice-dev-review) oml workareas mode dev in_progress
```

- Every key with reader access inside `payments.dev` now recalls the others' held memories, marked `unreviewed`. A version the scan held stays with its author.
- A key with `--profile curator` rejects, proposes retirements and flags contradictions there, and never approves.
- Designed ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d10, d16): held skills, guardrails and steering are served there too, marked, and keys at an ancestor such as `payments` join the audience; such a key is also offered the subtree's skills and guardrails below it, the nearer one winning a name clash. A held guardrail only asks before a tool call and never denies, so an unreviewed rule cannot stop an agent.

## 3. Author skills and guardrails as claims

Instructions are claims: written, held, reviewed and versioned like every memory.

**Guardrails and steering. Built.**

```sh
oml write --kind guardrail --severity never \
  --rule '{"rule":"oml-gr/1","tool":"shell","command":["git","push","*"],"effect":"deny"}' \
  --workarea dev "Never push from an agent session."
oml write --kind steering --workarea dev "Run cargo test -p <crate> before every commit."
```

The plugin loads promoted guardrails at session start and checks each rule before a matching tool call ([give agents rules](../use/guardrails.md)). A change is `oml write --replaces <id> --expected-version <n>`, held until reviewed; until then every session keeps the approved version. The rule shape is in [ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md).

**Skills. Designed** (ADR-0080). An agent stores a skill with `oml_write` and `kind: "skill"`: a name, a one-line description and a markdown body of at most 32 KiB. The MCP server offers each promoted skill as a prompt and a resource, loaded live, never copied to disk. The author key is offered its own held skill at once, marked unreviewed; a held edit of an approved skill appears beside it as `<name>-unreviewed`. The plugin asks before writing a file under `.claude/skills/`, since a file reaches one machine.

**Who approves.** Built: one verdict by any `dreamer` key outside the version's lineage, a reviewer crew's agent key included; the recorded policy requires no person for any kind. Designed (ADR-0080 d16): every instruction kind, steering, guardrail and skill, counts only a person's verdict, since an agent follows it rather than weighing it.

**Builder. Designed** ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workareas-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md)). The workarea page in the console edits skills, guardrails and documents in forms; each save is a held version under the person's own key, and Preview writes nothing.

## 4. Review

Built. bob reviews what alice and the agents wrote, never bob's own work.

```sh
OML_API_KEY=$(cat keys/bob-dev-review) oml review list --workarea dev
OML_API_KEY=$(cat keys/bob-dev-review) oml review list --workarea dev --author-key pay-agent-1 --ids-only \
  | OML_API_KEY=$(cat keys/bob-dev-review) oml review approve --ids -
```

A bulk act still records one verdict per item. A reviewer crew can judge knowledge over HTTP ([reviewer crews](../api/reviewer-crews.md)).

Designed ([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md)): one To do page lists what waits on a person, in a shared workarea through tenant keys linked to the sign-in (ADR-0083 d11, ADR-0085 d16), and the Memories page becomes one cockpit with Approved and Submitted views, where a held row never appears as approved.

## 5. Test and verify

**Designed** ([ADR-0086](../adr/ADR-0086-a-workareas-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates.md), [ADR-0087](../adr/ADR-0087-an-impact-report-shows-under-the-callers-own-reads-what-a-candidate-release-changes-for-the-workareas-that-receive-it.md)).

- A test case is a claim of kind `test`, claim key `test/<name>`, reviewed like any claim. Four shapes: a recall expectation, a guardrail decision on a tool call, a skill check, a document check.
- `oml workareas test` runs the suite on the server under the caller's key. A run on in-progress content is advisory. A run against a release candidate counts as a gate.
- A guardrail case reads the rule's signed effect, not its in-progress softening, so a suite tests what production will do.
- An impact report shows, under the caller's own reads, what the candidate changes for every workarea that receives it: near-copies, name collisions, guardrail decisions that change, and a per-scope diff of skills, guardrails and steering.

Built today: `oml sync --pr <n>` writes a draft pull request's records into `<workarea>-pr-<n>` for review beside the code ([sync](../use/sync.md)), and the [recall eval](../operations/recall-eval.md) measures search on its own corpus.

## 6. Cut and publish a release

**Designed** (ADR-0085). The workarea can stay in progress.

```sh
OML_API_KEY=$(cat keys/alice-dev-review) oml workareas release propose dev --label 1.4.0
OML_API_KEY=$(cat keys/bob-dev-review)   oml workareas release publish dev 1.4.0
```

- The release is a fixed list of promoted versions. No held, rejected or simulation row is in it.
- Gates run in order: scan, review, tests, impact, publish. Only the ratifier overrides the tests or impact gate, per case, with a reason. CI attaches test runs and does nothing else.
- Release notes are generated; a removed skill or a weaker guardrail is flagged breaking and needs a new major label.

Built today: the ladder. One key proposes `committed` and another ratifies it; committing needs zero held items, and a `validated` workarea takes no new writes.

```sh
OML_API_KEY=$(cat keys/alice-dev-review) oml workareas status propose dev --to committed
OML_API_KEY=$(cat keys/bob-dev-review)   oml workareas status ratify dev <transition-id>
```

## 7. Bind production agents

**Designed** (ADR-0085 d6, d7). Every key has a binding.

| Binding | Reads | Default for |
|---|---|---|
| `tip` | the promoted tip, plus held work in an in-progress subtree, marked | a key held by a person; any key scoped in a user root; an agent key may be issued at `tip` when its scope is inside an in-progress root or at an ancestor of one |
| `latest` | the newest published release; never a held item | every other agent or service key |
| a pin | one release by label, such as `payments.dev@1.3.0` | none; set on purpose |

The defaults are ADR-0085 d7's.

A production agent bound to `latest` reads 1.4.0 from its next request and loads its guardrails and skills at its next session start. Its own held writes are still served to it. Only the key's issuer or its own person widens a binding; an admin only narrows one. Today every key reads as a `tip` key.

## 8. Fix and roll back production

**Withdraw a release. Designed** (ADR-0085 d9). bob withdraws 1.4.0 with a reason. `latest` keys read 1.3.0 on their next request. One person suffices; if 1.3.0 lacks a guardrail 1.4.0 has, the withdrawal still takes effect and raises an alert until another person acknowledges it.

**Fix forward. Designed** (ADR-0085 d11). `oml workareas revert --to 1.3.0` drafts held supersessions back to 1.3.0's text, reviewed and cut as 1.4.1.

**Admin fix at once. Designed** ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)). At 03:00, with nobody on the team answering, the tenant's admin demotes the bad skill version:

```sh
oml admin fix demote <artifact_id> --reason "refund-flow step 3 refunds twice"
```

- `tip` keys get the previous promoted version at once. Every release that lists the bad version serves the previous one as a marked overlay, `approver.act: "admin_fix"`, `confirmed: false`, and reads `degraded`.
- No one can cut 1.4.1 over an unconfirmed fix.
- In the morning bob sees the fix under To do's "Admin fixes to confirm" (ADR-0083 d8), or with `oml review fixes`, reads the reason, and confirms or reverts it. The admin is never named to the reviewer. In a production scope only another person confirms, never the admin's own.

Admin fixes are demote, edit, retire, release rollback, import rollback and erasure. Each takes an admin key held by a person and a reason, is marked to every reader, and is journalled. An admin key never reaches an agent surface.

Built today: rejection and retirement through review. Demotion and erasure are not built.

## What the team pays

- Each item needs a second person's look before production agents act on it; in development, nothing waits.
- Under `latest`, an approved memory reaches production agents only with the next release. Agents that should see approvals at once stay on `tip` and skip the staging step.
- A production fix that cannot wait is one admin's act, marked until a second person confirms.

Moving `payments.dev`'s release to a separate production server is on [import and export](import-export.md).
