---
status: Stable
verified: 2026-11-14
answers: "the four architecture views of OML as built: who uses it and what it depends on, the deployment from the client to the disk, one memory through the review gate, and everything one agent key is served; and where the proposed designs attach to them"
---

# Architecture views

Four views of one system, each answering one question. The context view says who and what OML talks to. The deployment view follows a request from the client to the disk. The two logical views follow a single memory through the review gate, and a single agent key to everything it is served. Every element is built unless it is drawn dashed.

In the figures, a solid box is a process or component. A dashed box is built and not installed, or served only in one mode. A tinted box with an amber edge is the element the view turns on, and an amber dashed outline is a trust boundary. The pages linked under each view hold the contracts; the views hold no rule of their own.

## Context: who uses OML and what it depends on

<figure markdown="span">
  [![Context view: people, admins and AI agents use Open Memory Lab; OML depends on Google and GitHub for sign-in and on GitHub Releases for signed builds, and feeds a Delta lake to analytics tools.](img/context.svg#only-light)](img/context.svg)
  [![Context view: people, admins and AI agents use Open Memory Lab; OML depends on Google and GitHub for sign-in and on GitHub Releases for signed builds, and feeds a Delta lake to analytics tools.](img/context-dark.svg#only-dark)](img/context-dark.svg)
  <figcaption>OML runs no generative model and calls no model provider: agents bring their own model and reach OML over MCP. The optional sparse and rerank search stages run small local models on the lead. The lead's outbound calls are the sign-in code exchange with Google or GitHub, an MCP client's metadata document, and, when an operator turns it on, the search models' files.</figcaption>
</figure>

The actors and their keys are on [the overview](overview.md#who-talks-to-oml) and [roles and keys](roles-and-keys.md); what the lake holds is on [storage](storage.md#the-lake-and-the-outbox).

## Deployment: a request from the client to the disk

One host, as production runs it. Compose runs the same lead, `oml-web`, Caddy and PostgreSQL; the Helm chart runs the lead and `oml-web` behind an Ingress, with PostgreSQL outside it; neither runs the lake writer. The lead is the only process that holds the signing key, so it sits alone inside the trust boundary.

<figure markdown="span">
  [![Deployment view: clients reach Caddy over HTTPS; Caddy routes console pages to oml-web, every other path to oml-server-lead, and landing paths to static files; oml-web calls the lead's public API; the lead commits each write to PostgreSQL in one transaction and signs with a key file on its host; oml-lakewriter drains the outbox into the Delta lake.](img/deployment.svg#only-light)](img/deployment.svg)
  [![Deployment view: clients reach Caddy over HTTPS; Caddy routes console pages to oml-web, every other path to oml-server-lead, and landing paths to static files; oml-web calls the lead's public API; the lead commits each write to PostgreSQL in one transaction and signs with a key file on its host; oml-lakewriter drains the outbox into the Delta lake.](img/deployment-dark.svg#only-dark)](img/deployment-dark.svg)
  <figcaption>A write enters through Caddy, is decided by the lead, and becomes durable only when one PostgreSQL transaction commits the signed journal event, the state change and an outbox row. Everything downstream (indexes, the lake) derives from that commit. The ports are production's: the lead listens on 127.0.0.1:8080 unless configured, and Compose keeps 8080. The operations row is the production host's own tooling, kept outside this repository.</figcaption>
</figure>

**Why one commit point.** A single transaction makes the journal and the state agree by construction. The cost is PostgreSQL latency on every write and a journal that only grows. See [storage](storage.md#the-commit-point).

**Why `oml-web` holds no data.** The console is a client of the same public API as the CLI ([ADR-0070](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)). A compromised console can do only what the signed-in person can do.

**Three database roles.** The owner role runs migrations at boot. The lead serves under a role that owns nothing, with row-level security forcing tenant isolation ([ADR-0034](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)). The lake writer reads under a role of its own, one tenant at a time ([ADR-0060](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0060-the-lake-writer-drains-the-outbox-it-has-seals-what-it-materializes-and-answers-only-the-operator.md)).

**Bodies at rest.** PostgreSQL holds memory bodies unencrypted, under the tenant's row security. The lake writer seals each lake copy under a per-version data key, wrapped by a key file only it holds. See [security](security.md#key-custody).

## Logical: one memory through the review gate

The same path for every kind of claim: a memory, a guardrail, steering. The four rules ([ADR-0001](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md)) are where the path cannot bend.

<figure markdown="span">
  [![The review gate: a write is scanned in the same call, lands held, is reviewed by a different identity, and only then is promoted and served. A veto refuses the write, a rejected version is kept but never served, and inside an in-progress workarea a held version the scan did not hold is served marked unreviewed.](img/gate.svg#only-light)](img/gate.svg)
  [![The review gate: a write is scanned in the same call, lands held, is reviewed by a different identity, and only then is promoted and served. A veto refuses the write, a rejected version is kept but never served, and inside an in-progress workarea a held version the scan did not hold is served marked unreviewed.](img/gate-dark.svg#only-dark)](img/gate-dark.svg)
  <figcaption>Step 3 is the only door to step 4, and no agent key can open it (R1). The scan's hold findings still reach a reviewer, who sees them on the card; a veto, such as a leaked key, writes nothing. An in-progress workarea is the one bounded exception to step 2's privacy: keys inside it see held work, marked unreviewed, and nothing leaves until a person commits the workarea.</figcaption>
</figure>

The pipeline's contract is [admission](admission.md); the in-progress mode is in [workareas](workareas.md#in-progress-workareas) ([ADR-0030](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)).

## Logical: one agent key and everything it is served

The key decides everything; the request decides nothing. Its default workarea places it in the tree, and the tree decides what reaches the agent and by which path.

<figure markdown="span">
  [![Serving view: an agent key whose default workarea is universe.payments.dev receives approved guardrails from dev and each ancestor it reads at session start, steering from the same nodes first in each recall, recalls from dev and its subtree with held work marked unreviewed because dev is in progress, and recalls from other committed or validated workareas it reads through federation.](img/serving.svg#only-light)](img/serving.svg)
  [![Serving view: an agent key whose default workarea is universe.payments.dev receives approved guardrails from dev and each ancestor it reads at session start, steering from the same nodes first in each recall, recalls from dev and its subtree with held work marked unreviewed because dev is in progress, and recalls from other committed or validated workareas it reads through federation.](img/serving-dark.svg#only-dark)](img/serving-dark.svg)
  <figcaption>The Claude Code plugin loads guardrails at session start and checks a guardrail's rule locally before each matching tool call; the agent kit's other tools get no local check. Recall is the only per-question read, and steering rides at its head. A request may name another workarea, which the key's grants still bound. A reader outside the in-progress subtree sees none of it, approved rows included, until a person with a reviewer key commits the workarea; the commit is refused while anything in it is held.</figcaption>
</figure>

Guardrails are in [ADR-0037](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md), recall and steering in [retrieval](retrieval.md), and federation in [ADR-0072](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md).

## Where the proposed design attaches

None of this is built. Each row names the view it changes, so the views above stay the reference for what runs today. The records are proposed in open pull requests and are not accepted.

| Proposal | Records | Changes view | What changes |
|---|---|---|---|
| Skills and documents | ADR-0080, ADR-0081 ([#375](https://github.com/e8kor/open-memory-lab/pull/375)) | gate, serving | Two new claim kinds through the same gate, served as MCP prompts and resources beside guardrails. |
| Cockpit and workarea builder | ADR-0082, ADR-0083 ([#375](https://github.com/e8kor/open-memory-lab/pull/375)) | deployment | New console pages in `oml-web`; writes go through the public API as the person, as today. |
| Releases and key binding | ADR-0085 ([#377](https://github.com/e8kor/open-memory-lab/pull/377)) | serving | A key follows the live tip, the latest release or a pinned release; a bound key keeps its release while the workarea is in progress. |
| Tests and promotion gates | ADR-0086, ADR-0087 ([#377](https://github.com/e8kor/open-memory-lab/pull/377)) | gate | Gates before a release: scan, test suites, impact report, review, ratification by a second person. |
| Admin production fix | ADR-0089 ([#377](https://github.com/e8kor/open-memory-lab/pull/377)) | gate | An admin's demote, edit or rollback is served at once, marked and journaled, then confirmed or reverted by a person. |
| Export and import | ADR-0084, ADR-0088 ([#377](https://github.com/e8kor/open-memory-lab/pull/377)) | context, deployment | A signed workarea bundle between servers; an import lands held and is reviewed again on the target unless its source server is pinned. |

The figures' sources are `design/diagrams/arch-*.svg`; `python scripts/gen_design_tokens.py` writes the files this page shows, in the paper and night themes.
