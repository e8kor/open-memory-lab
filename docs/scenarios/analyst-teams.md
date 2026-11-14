---
status: Beta
verified: 2026-11-14
answers: "how a business-analyst team prepares documents, reference material and vocabulary on a staging server, reviews and validates it, releases it, exports it and imports it to production, and what the agents that consume it see at each stage; which steps are built and which are designed"
---

# Business-analyst teams

The pricing analysts, priya and sam, prepare what the support agents should know about pricing: policy documents, a glossary of plan names, and a few standing instructions. They draft on a staging server and hand a reviewed release to the production server, which the support agents read.

The controls cost the analysts two things: a second analyst's look at each item, and a second review on production for each move. Drafting itself waits on nobody. The [workspace lifecycle](../architecture/lifecycle.md) draws the stages; [producers and reviewers](../concepts/producers-and-reviewers.md) says who may approve what.

## 1. Draft on staging

Built. An admin on staging creates `pricing.draft`, issues priya and sam a writer key and a reviewer key each (`--used-by person`, as for [engineering teams](engineering-teams.md#1-set-up-the-shared-workspace)), and priya puts it in progress:

```sh
oml workspaces create universe pricing
oml workspaces create pricing draft
OML_API_KEY=$(cat keys/priya-review) oml workspaces mode draft in_progress
```

From then on both analysts, and any drafting agent with a key there, read each other's held work, marked unreviewed.

## 2. Load documents and reference material

**Built: markdown claims and repository sync.**

```sh
oml write --format text/markdown --title "Refund window for annual plans" --workspace draft - < refunds.md
oml sync policies/ --workspace draft --adapter headings --dry-run
oml sync policies/ --workspace draft --adapter headings
```

`oml write --format text/markdown` stores a markdown body with its one-line title. `oml sync` reads committed files at a revision, one claim per heading, and plans what is new, changed or removed before writing anything held ([sync docs and code](../use/sync.md)). Each claim keeps its source anchor, so a reviewer can open the file it came from.

**Designed: documents** ([ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md)). A `document` claim, key `doc/<path>`, is a titled markdown or text body. Recall ranks it by passage and serves the best passage as the hit; the MCP server serves the whole document as a resource, `oml://…`, so an agent reads the policy, not a fragment. PDFs and other files wait for attachments.

## 3. Define vocabulary

**Built: terms as reviewed claims.** `oml write --kind term` and `--kind term_mapping` store a plan name and what it maps to, written over the CLI or HTTP and reviewed like any claim. Query expansion by approved terms is not built: recall sends no terms today.

**Not built: ontology crews.** A crew that classifies claims and proposes terms is proposed in a separate record, not on `main`.

## 4. Review and validate

**Built.** sam reviews priya's items and the drafting agents', never sam's own:

```sh
OML_API_KEY=$(cat keys/sam-review) oml review list --workspace draft
OML_API_KEY=$(cat keys/sam-review) oml review list --workspace draft --ids-only \
  | OML_API_KEY=$(cat keys/sam-review) oml review approve --ids -
```

The review card shows each scan finding and, for a synced claim, the anchor. A reviewer crew may approve knowledge; a standing instruction counts only a person's verdict once [ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d16 is built.

**Designed** ([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md)): the console's To do page lists what waits on each analyst, in a shared workspace once the analyst's tenant keys are linked to the sign-in (ADR-0083 d11, ADR-0085 d16), and the Memories cockpit shows Approved, To review and Submitted, where a held row never appears as approved. The builder edits documents in a form, each save a held version under the analyst's own key ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md)).

**Designed** ([ADR-0086](../adr/ADR-0086-a-workspaces-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates.md), [ADR-0087](../adr/ADR-0087-an-impact-report-shows-under-the-callers-own-reads-what-a-candidate-release-changes-for-the-workspaces-that-receive-it.md)): recall test cases such as "a query for 'annual refund' finds `doc/policies/refunds` in the top 3", and an impact report naming near-copies already approved elsewhere.

## 5. Release on staging

**Designed** ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md)). priya cuts `pricing.draft@2.0.0`; sam publishes it once the gates pass. The release notes list every document, term and instruction added, changed or retired.

Built today: the ladder. Committing `pricing.draft` needs zero held items and two keys; a `frozen` workspace takes no new writes.

## 6. Export from staging, import to production

**Designed** ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md), [import and export](import-export.md)).

1. On staging, priya exports the release, encrypted to the production team's age key: `oml workspaces export draft --release 2.0.0 --recipient age1… --out pricing-2.0.0.oml`. The tenant's export policy must allow a shared workspace's export.
2. On production, an admin has created `pricing.prod` and granted the team. An analyst puts it in progress and imports with a writer key, and so becomes the author of every imported version on production. The production scan runs on each version.
3. A second analyst on production reviews every imported item again, attesting no authorship of it on staging. The staging verdicts show as evidence and count for nothing.
4. Two analysts cut and publish `pricing.prod@2.0.0`.

## What the consuming agents see

| Stage | A support agent on production, bound to `latest` | A drafting agent on staging, bound to `tip`, inside `pricing.draft` |
|---|---|---|
| Drafted, held | nothing | the draft, marked unreviewed |
| Approved on staging | nothing | the approved version |
| Released on staging | nothing | the approved version |
| Imported, held on production | nothing: binding wins over the in-progress mark | not applicable |
| Reviewed on production | nothing until the release | not applicable |
| Released on production | the release, every kind: documents, terms, instructions | not applicable |
| Admin fix on production | the fixed item, marked `admin_fix`, until a person confirms | not applicable |

Built today, without bindings, every key reads as `tip`: a support agent with reader access on a protected `pricing.prod` reads each version the moment it is approved there.

## What the team pays

- One review per item on staging and one again on production for each move. That second review is the price of not trusting the staging server's verdicts; a trusted source server is proposed and deferred ([ADR-0088](../adr/ADR-0088-an-imported-version-counts-as-reviewed-only-from-a-trusted-source-a-tenant-pinned-and-never-for-instructions.md)).
- Under `latest`, an approved correction reaches support agents only with the next release. An urgent correction goes through an admin fix, confirmed afterwards by an analyst.
