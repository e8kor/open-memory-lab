---
status: Stable
verified: 2026-11-06
answers: "how a person reviews held memories: getting and storing a reviewer key, taking the next held memory, reading it and its sources, what to check, approving and rejecting, supersessions and steering, and a worked session"
---

# Review held memories

Every write lands held and becomes readable to others when an identity that did not write it
approves it. Why OML works this way is in [why a write is held](../concepts/why-a-write-is-held.md).
This page is the reviewer's routine. Agent crews review over HTTP instead: see
[Integrate a reviewer crew over HTTP](../api/reviewer-crews.md).

## Before starting

A reviewer key (role `reviewer`) is required. A tenant admin issues it; a reviewer cannot issue its own. The
key needs two grants on the workspace under review: `reviewer`, to take and judge held memories, and
`reader`, to read their sources. Without the reader grant, a held memory is served with an empty
`derives_from` and no replaced version.

```sh
# the admin, with the admin pair stored
oml keys issue alice-review --role reviewer --used-by person --principal people/alice \
  --grant crew:reviewer --grant crew:reader --default-workspace crew
```

The command prints `key issued — SHOWN ONCE` and the value once. The server keeps only its hash.
Give each reviewer their own key and principal, because a shared key makes two people one. An admin
key is never a reviewer key, and a `used_by: service` key is refused every review verb with
`access.verb_ceiling`. `oml quickstart` issues one reviewer key, `quickstart-reviewer`, into
`<dir>/keys/reviewer` ([run a server](../start/run-your-own.md)); it holds the `reviewer` grant only, so a held memory it serves has no sources. The admin's side is in
[Administer an OML server](administering.md).

Store the key once. `oml login` reads it without echo, checks it, and writes the URL and key to the
owner-only credentials file:

```sh
oml login https://oml.example.com
oml doctor
```

If an agent key also runs on this machine, keep the reviewer pair in a second credentials file
and select it with `OML_CLI_CREDENTIALS`.

## Take the next held memory

```sh
oml review next
```

The command serves one held memory, oldest first, and the key takes it. It serves only
workspaces where the key holds `reviewer`. It skips any memory whose lineage names the key or
principal. An empty queue prints `nothing in scope waits — the queue is empty (204).`

The output shows the artifact id and version, kind, writer, claim key and text, then the
`serve_id`. Add `--json` to see `derives_from`, `replaces`, `retirement` and, for a supersession,
the full version it replaces. Read each source with `oml read <artifact_id>`. A source that cannot be
read is a reason to reject, not to guess.

To read the queue without taking anything, run `oml review list` (a reviewer key used by a person).

## Reviewing on the user page

Signed in at `/web`, every page carries a **To do · N** badge beside the account name: the number
of held memories the user's own reviewer key, `<user>-review`, may judge, across every workspace.
It links to **To do** (`/web/todo`), whose **To review** group lists them oldest first, each row
naming the decision (New memory, New skill, Edit of v2, Retirement of v1), its kind, workspace and
author, `same user` where a key of the person wrote it inside their own root, and how long it has
waited, with **Judge**. A workspace filter narrows the list; **Next** opens the oldest item.
**Judge** (`/web/todo/judge`) draws the item whole with the checks, what each verdict does and, for
an edit, the change against the version it replaces, name, description and body each in its own
block, removed words struck and added words underlined; **Record verdict** records the verdict as
`<user>-review` and returns to To do, and Next on Judge opens the next item without one. With
nothing to judge the page says "Nothing waits for this account." and links to Memories, Submitted.
The old **Review** address answers a redirect to To do for this release
([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d2, d6, d11). **Memories** has two views, two links under its
heading that keep the workspace and the filters. **Approved**, the default, lists the approved
versions the person's keys read, newest first, 50 a page, filtered by workspace, text, state
(approved or superseded), origin or author key; its search never serves a held version, inside an
in-progress workspace either, so a held row is never listed there. **Submitted** lists what the
person's own keys wrote that is held, rejected or withdrawn, and what was approved in the last 30
days, each row led by one sentence: "Waiting for review.", "Waiting for review. v2 is still
served.", "Approved on 2026-11-10 and served as `deploy-window` v3." (for a skill, "Approved on 2026-11-10 and
published as `/mcp__oml__bump-deps`."), "Rejected." or
"Withdrawn.". A held row offers **Withdraw**, which asks for confirmation first. Submitted has no
text search, since a held row's text is read only by its author; when a rejection or a withdrawal
happened is not served to its author, so neither sentence names a day
([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d1, d3, d4, d13). **Atlas** shows the workspaces as a map
sized by readable count, with the same numbers in a table below it. **Workspaces** gives each workspace one dropdown
of what the reviewer key may do there now: propose the next status, ratify another key's
proposal, or set who sees its drafts: everyone working here, or the writing key only, which waits while
any memory in it is held. Apply
shows what will happen before anything is recorded
([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d22–d30).

### Build skills, guardrails and documents on a workspace's page

Each row of **Workspaces** opens that workspace's own page, `/web/workspace?workspace=<name>`
([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d1). Under the workspace picker, **Approved contents by kind** links Memories (the Memories
page filtered to the workspace), Skills, Guardrails and Documents, each with its count of approved
items, one per claim, on that workspace alone. The kind's table lists every version: name, state,
for a held one the decision (New skill, Edit of v2, Retirement of v1), version and when it changed,
or severity and rule for a guardrail. An approved version with nothing held after it offers
**Edit** and **Retire**; one with a held successor says "in force · edit v3 waits for review" and
links that version; a held version offers **Judge it** where the person's reviewer key may judge
it, and **Withdraw** where a key of the person wrote it. Inside a workspace whose drafts are visible
to everyone working there, a held row is marked unreviewed and "served here, unreviewed". The
Guardrails kind opens with **In force here**: every guardrail an agent connected to the workspace
loads, the inherited ones first, each linking to the workspace it comes from, then the workspace's
own, with the counts under the key's guardrail scope
([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d12). The old Guardrails
address answers a redirect there.

**New skill**, **New guardrail** and **New document** open a form, offered where a key of the person
may write: `<user>-web` in the person's own root, `<user>-review` in a shared workspace where it
holds reviewer. A skill takes a name (fixed for every version), a description and a markdown body;
a guardrail a severity (`never`, `must`, `should`), a text of at most 300 bytes and an optional
rule (tool, match, effect); a document a title, a format (markdown or plain text), an optional
claim key and a body. **Preview** comes first: it writes nothing and draws above the form what a
session will receive, the command `/mcp__<server>__<name>` for a skill, the session-start line, what
the rule checks and the set's size against its cap of 12 for a guardrail, the resource address and
the passages for a document, with the scan's findings. **Propose** appears only on the page a
preview answered and writes exactly the previewed values, held until approved; a field changed
since then is previewed again instead. An edit also draws its change against the served tip, name,
description and body in their own blocks. **Retire** names what stops once it is approved and
needs a reason. A refusal keeps every value and names the error beside its field; when the claim
moved on since the form was drawn, the one action is **Edit from v3 with this text**. The same form
sent twice writes one version. In the person's root `<user>-review` may approve, marked same user;
in a shared workspace a reviewer outside the person approves, a person for a skill or a guardrail
([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d2-d7).

**Catalog. Designed, not built** ([ADR-0092](../adr/ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md), planned for v0.8.0). A **Catalog** page after
Memories will list what the person's key reads across kinds, with a tab and a count per kind. A
held version served inside an in-progress workspace carries the unreviewed mark; a held skill,
guardrail or steering version also carries "needs a person", and **Judge it** where the person's
reviewer key may judge it. The page approves nothing itself: the verdict is given on the judge
page, as today.

## Check the memory

Approve only what another agent could safely act on.

- **One checkable claim.** One fact or decision per memory, specific enough to be wrong. A
  conclusion names its sources in `derives_from`, and each source supports it.
- **A source and a date.** A quote reference, a commit, a document or an "as of". A memory with no
  anchor ages without anyone noticing.
- **No secrets.** No keys, passwords, tokens or personal data. A rejected memory keeps its bytes,
  so tell an admin about a secret instead of only rejecting it.
- **Scanner findings.** The `oml-scan/1.3` rule set vetoes leaked keys before anything is stored and
  holds what a reviewer should judge. The review card lists every finding on the version, one line
  each: its verdict, class and rule, and the field and line it fired on, never the matched text. The
  bulk table counts them per row. `oml admin rescan` reports what it finds in memories already
  stored. It does not replace reading.

## Approve or reject

```sh
oml review approve --note "matches quote ref AM-7741"
oml review reject --reason "no source and no date; resubmit with the quote reference"
```

Without a `serve_id`, the verdict applies to the one memory the key has taken. After
`oml review next` runs more than once, name it: `oml review approve <serve_id>`. Otherwise the verdict
is refused with `request.validation` naming `serve_id`. `--reason` is required to reject. `--note`
is optional and is journaled as the verdict's reason. Both answer
`verdict recorded — <artifact_id> is <state> (journal seq <n>)`.

A rejection is journaled, never silent. The author sees `rejected` in its own listing
(`oml read --state rejected`). The reason is in the journal, and `oml read` does not show it.
Reopening a rejection is not available.

After approval the memory is `promoted`. Any key with `reader` on its workspace or an ancestor finds
it with `oml read --query` or `oml_read`. Its rank does not change.

## Review a supersession or steering

A **supersession** is a new version of a promoted memory, written with `--replaces`. The old
version stays the one everyone reads until the new one is approved. Read the two together with
`--json`. With `retirement: true`, the write proposes retiring the memory, and its text says why.
Approving it stops recall serving the memory, and every version stays readable as history. If
another version won the tip first, the approval is refused with `submission.tip_conflict` and
nothing is journaled. The serve can still carry a reject.

A **steering** memory has kind `steering`. Once approved, every recall in that workspace and below
returns it first, unranked, and no budget cuts it. Read
steering more strictly than a fact, because it shapes every later session in the subtree. Steering
and guardrails are instructions, so only a person's reviewer key approves them; an agent's
reviewer key, or a person's key with a crew or a profile, is refused `review.person_required` and
may still reject. A service key reviews nothing (`access.verb_ceiling`). For now a
person's key is a reviewer key issued `--used-by person` with no crew and no profile, the CLI's
included; a confirmed, fresh sign-in comes with #214. `oml review next` never hands such a version to a key
that cannot approve it, and `oml review next --needs person` takes only those, round-robin across
authors, each author's oldest first.

## A worked session

```sh
$ oml review next
serving 1 held item

  artifact  4f9c2a7e-0b31-4d8e-9a55-1c6f3e2d8b10 @ v1   kind: memory   writer: crew-procure/agent-4
  claim_key supplier-quote-am-7741
  body      | Accra Metals quoted 412 USD/t for Q1-2027 delivery, valid until 2026-11-15. Quote ref AM-7741.

  serve_id: srv_4f9c2a7e0b314d8e9a551c6f3e2d8b10  (expires 2026-11-04T10:15:00Z)
  verdict with: oml review approve [--note …] | oml review reject --reason …

$ oml review next --json          # derives_from and replaces of the same taken item
$ oml read 7a1d0c3b-5e62-4f19-8b07-2d4e6f8a9c31   # each source it names
$ oml review approve --note "dated, sourced, one claim"
verdict recorded — 4f9c2a7e-0b31-4d8e-9a55-1c6f3e2d8b10 is promoted (journal seq 212)

$ oml review next
nothing in scope waits — the queue is empty (204).
```

The identifiers are illustrative. Every workspace has the same policy: one approving verdict
promotes. If a step fails, see [client troubleshooting](../help/troubleshooting-client.md).
