---
status: Stable
verified: 2026-11-19
answers: "how a person works in the web console day to day: what To do lists and how an item is judged, what the Memories page's Approved and Submitted views show, how a held write is withdrawn, and where skills, guardrails and documents are built"
---

# Work in the web console

The web console at `/web` is where a person signs in to judge what waits for them, follow what
their own keys wrote, take back a mistaken write and build a workspace's skills, guardrails and
documents. It holds no memory of its own: every page reads and writes through the HTTP API under
the person's own keys, `<user>-web` and `<user>-review`
([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md)).
The Personal side shows the person's own work; the Administration side, for an admin, sits under
its own inverted bar.

## To do: what waits for a verdict

1. **Read the badge.** Every page carries **To do · N** beside the account name: the held versions
   `<user>-review` may judge, across every workspace.
2. **Open To do** (`/web/todo`). Its **To review** group lists them oldest first. Each row names
   the decision (New memory, New skill, Edit of v2, Retirement of v1), the kind, workspace and
   author, `same user` where a key of the person wrote it inside their own root, and how long it
   has waited. A workspace filter narrows the list; **Next** opens the oldest item.
3. **Judge.** **Judge** (`/web/todo/judge`) draws the item whole, with the checks, what each
   verdict does and, for an edit, the change against the version it replaces. **Record verdict**
   records it as `<user>-review` and returns to To do; **Next** opens the next item without a
   verdict. A skill, a guardrail or steering needs a person's verdict, and a held one says "needs a
   person".

With nothing to judge, To do says "Nothing waits for this account." What to check before
approving is in [review held memories](../guides/reviewing.md#check-the-memory).

## Memories: Approved and Submitted

The **Memories** page is one cockpit with two views, two links under its heading that keep the
workspace and the filters.

- **Approved**, the default, lists the approved versions the person's keys read, newest first, 50
  a page, filtered by workspace, text, state (approved or superseded), origin or author key. It
  never lists a held version, inside an in-progress workspace either.
- **Submitted** lists what the person's own keys wrote that is held, rejected or withdrawn, and
  what was approved in the last 30 days. Each row opens with one sentence: "Waiting for review.",
  "Waiting for review. v2 is still served.", "Approved on 2026-11-10 and served as `deploy-window`
  v3.", "Rejected." or "Withdrawn.". Submitted has no text search, since a held row's text is read
  only by its author.

The **Catalog** page, after Memories, lists every kind the person's web key reads in a workspace
and below it, a tab per kind with its count ([ADR-0092](../adr/ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md) d7).

## Withdraw a mistaken write

A held version can be taken back by its author before anyone judges it. It is kept as `withdrawn`
with its text, leaves every review queue and To do, and is never served; no verdict is recorded
and no reviewer is involved
([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d13).

1. Under **Memories**, **Submitted**, find the row that says "Waiting for review.".
2. Choose **Withdraw** and confirm. The row then says "Withdrawn.".

The console withdraws any held version a key of the person wrote, an agent's included. A held
skill, guardrail or document on a workspace's page offers **Withdraw** the same way. An agent
withdraws its own write with `oml_write` and `withdraw`, and the CLI with `oml withdraw <artifact_id>`
([write and read memories](remember-and-recall.md#correct-withdraw-retire-and-link)). An approved version
is not withdrawn: it is corrected or retired by a new held version. A withdrawn version is never
reopened; a new write or a supersession of the served tip follows it.

## Build skills, guardrails and documents

**Workspaces** opens each workspace's own page, where **Approved contents by kind** links its
memories, skills, guardrails and documents, and **New skill**, **New guardrail** and **New
document** open a form that previews before it proposes. Every proposal is a held version under the
person's own key, reviewed like any other write
([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md)).
The form's fields and checks are in
[review held memories](../guides/reviewing.md#build-skills-guardrails-and-documents-on-a-workspaces-page),
and what each kind does once approved is in [skills](skills.md), [guardrails](guardrails.md) and
[documents](documents.md).
