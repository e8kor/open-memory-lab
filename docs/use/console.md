---
status: Stable
verified: 2026-11-25
answers: "how a person works in the web console day to day: what To do lists and how an item is judged, how an admin fix is confirmed, a brief and a status proposal made, a key rotated, what the Memories page's Approved and Submitted views show, how a held write is withdrawn, and where skills, guardrails and documents are built"
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
   `<user>-review` may judge, the brief proposals it may ratify and the notices not dismissed,
   across every workspace.
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

## Notices

An admin act this account must be told about waits in To do's **Notices**, whatever the workspace,
and counts in the badge: an act against an admin, an act that narrows a person's or a workspace's
access (a production fix that loosens a guardrail, disabling a person, revoking a person's key,
denying a reviewer person's key), or a key, grant or connect code a sole admin took at once inside
this account's own root. Each row names what it tells about, the act, the admin person who took
it and when. **Dismiss** removes it from To do; the journal keeps the notice and the act
([ADR-0097](../adr/ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md) d20, d24).

## Brief proposals to ratify

A memory another key proposed for a workspace's brief waits in To do's **Brief proposals to
ratify**, after To review, when `<user>-review` may ratify it. A proposal by the review key
itself, or under its principal, never appears there. **Ratify** asks for confirmation first, then
the memory joins the brief. A proposal by another key of the same person is marked **same user**
([ADR-0096](../adr/ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits.md) d7).

## Admin fixes to confirm

An admin's break-glass fix in a workspace where `<user>-review` reviews waits in To do's **Admin
fixes to confirm**, with its act, claim, reason and how long it has waited; the admin is never
named. **Confirm or revert** opens the fix's own page, `/web/todo/fix`, which shows the act, the
version, the reason and the time. **Confirm** keeps what the fix serves, marked confirmed;
**Revert** brings back the text the fix took out of service as a new version this account
approves. Each asks first. Where the fix lies outside this account's root and the admin acted for
this same person, the page offers no act and says another person settles it.

## A workspace's brief and status

A workspace page's **Brief** kind lists the approved memories an agent connected there is served
first, and the proposals waiting to join, each with who proposed it and when. **Ratify** adds one,
as `<user>-review`, never on a proposal that key made. An approved memory's page offers **Propose
for brief**, which asks first and proposes it as `<user>-web`; proposing it again names the live
proposal instead. On a shared workspace where `<user>-review` holds reviewer, the page offers the
next status, protected or frozen, asked first; a second proposal of the same move names the live
one, made when and by which key. A user root takes no status move
([ADR-0095](../adr/ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people.md) d5, d6).

## Build a key

1. **Open Keys, then New key.** The builder at `/web/keys/new` starts from a shape: the whole
   root, one workspace, read only, write only, or a connector for Claude.
2. **Shape it.** Name the key after the `<user>-` prefix and pick its role: contributor (recall
   and write), reader (recall only) or submitter (write without recall). Each grant row names a
   workspace in the root, reader or submitter, the workspace and below or the workspace only, and
   the workspaces below it to leave out. **Add a grant** adds a row. A grant's **Creates child
   workspaces here** box lets the key create workspaces where the grant reaches. **Add a new
   workspace** adds a row that creates a workspace inside the root with the key, granted to it by
   its role. A date works through that day in every time zone, and a rate caps the key's requests
   a minute.
3. **Review it.** The review restates the access in words, marks the root's tree and shows the
   exact request. A refusal the page can decide, such as a session key's name or a grant outside
   the root, is listed at the top with its code and a link to the field.
4. **Issue key.** The value, or the connect code for a connector, is shown once. Only an admin
   changes the key's date or rate afterwards.

The admin console has the same builder at `/web/admin/keys/new`, under the admin Keys page's
**New key**. It works over the tenant's whole tree, takes a reviewer key, the user the key acts
for, who holds it, its principal and crew, and needs a reason. Outside a tenant that has only ever
had one admin, its key waits on another admin's confirmation: the value is shown once, and the key
works nowhere, and its new workspaces are not created, until a second admin confirms it. The admin
Workspaces page creates a workspace anywhere in the tenant at once
([ADR-0097](../adr/ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md) d29 to d31).

## Rotate a key

**Rotate**, beside **Cut off** on the Keys tab, gives a bearer key a new value and keeps its
name, grants and limits. The confirmation offers how long the previous value keeps working: no
time, one hour or 24 hours. The new value is shown once, with when the previous one stops. A
connector key takes **New connect code** instead, and a cut-off or expired key takes no new value.
The previous value keeps running memory reads and writes, verdicts, proposals and ratifications of
edges, fix settlements, withdrawals, uploads, imports into an existing workspace and exports until
then, and no admin act or act that creates a credential or changes structure or authority. An
admin rotates its own admin key and, from the admin one-key page, a tenant key that names no person
([ADR-0096](../adr/ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits.md) d13).

## Follow an import

An import runs from the command line. `/web/workspaces/import` shows the `oml workspaces import`
line for a target and a source key's fingerprint, and lists every import a key of the person ran,
with its state, the versions recorded and its counts.

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
