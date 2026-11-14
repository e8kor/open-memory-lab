<!--
id: adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys
tier: engineering
status: accepted design · 2026-11-14
answers: which console routes build and edit a workarea's skills, guardrails and documents, on the workarea page; which of the person's keys writes each act and why the admin side writes none; why an edit is a held supersession and a removal a retirement proposal; how a guardrail is edited within ADR-0037; what preview shows and which call serves it; who may review what a person wrote in the console; what the API must add
-->

# ADR-0082 — A person builds and edits a workarea's skills, guardrails and documents in the console, each edit a held version under the person's own keys

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with the screen
choices settled from the approved design system v18 mockups (d1's layout on the workarea page,
d5's preview on the form's own page, d6's diff on the form and on Judge) and every other default
as drafted · proposed the same day, tracking #374, with the operator's answers (d7, d9)

**Release:** planned for v0.7.4

**Amends:** ADR-0045 (d9: the user page writes skills, guardrails and documents through the
builder; d12: a person's web key supersedes rows any of the person's keys wrote), ADR-0037 (d17:
a page writes guardrails, through the builder only), ADR-0070 (d5: the builder's calls in its
tables), ADR-0044 (d8: outside the author's root, a version a person's session key wrote is excluded
from review by every key acting for the same person), the roles table (a person's dreamer session
key may start a held claim)

**Depends on:** [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md)
(kind `skill`), [ADR-0081](ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md)
(kind `document`); ADR-0089 (PR #377: admin fixes, d1 of this record)

**Twin parts** (ADR-0090 d18, PR #388): a console edit is a `memories` row written by the same
capture as any write, so its body, rule and search text are parts and its fold is the journal's;
the preview stores nothing.

## Context

On 2026-11-14 the operator asked to "add builder and editor to workarea, it should allow
introducing or editing skills, guardrails, rag documents" (#374): every edit a new held version,
reviewed by a different identity, kept forever ([ADR-0001](ADR-0001-the-constitution.md) R2, R4, cited, not restated).

What the console does today:

- **It writes no memory on the user side.** ADR-0045 d9: the user page's Memories view "reads and
  writes no memory"; `oml-web` serves `GET /web/memory` and no `POST` to it
  (`crates/clients/oml-web/src/lib.rs:305`, asserted at `lib.rs:400`). The admin side writes
  through `<user>-curate` on `/web/admin/curate` and supersedes on `/web/admin/memory`
  (`lib.rs:306-313`; ADR-0045 d12).
- **It writes no guardrail.** ADR-0037 d17: "No page writes, supersedes, retires or exports a
  guardrail"; the Guardrails view shows the set in force (`lib.rs:147`, `lib.rs:282`).
- **The keys a page acts through.** A person's session keys are `<user>-web` (writer and reader on
  `universe.<user>`), `<user>-review` (dreamer and reader there, and wherever an admin extends it),
  `<user>-curate` and `<user>-admin`, each reached by a capacity credential compiled into the
  handler, never a form field ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d6; [ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)
  d2). A writer supersedes only its own memory; another key's answers `access.author_only`
  ([roles and keys](../architecture/roles-and-keys.md), "Refusal order"). A person's memory may
  be started in their root by their keys only (ADR-0044 d16).
- **Independence is per key.** ADR-0001 R2 excludes the keys and principals of a row's lineage closure,
  not the person, so a person approves their own agents' writes through `<user>-review`, marked
  `same_user` ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
  d8; ADR-0046 d2; ADR-0077 d2). That was decided for an agent's write reviewed by its person.
  A text the person types in the console has the person as its author, so the same rule would let
  the author approve it.
- **The console is an API client.** `oml-web` reaches the application only over HTTP and a test
  fails if its graph reaches `oml-app` or `oml-domain` (ADR-0070 d1), so anything a preview needs
  that the domain computes (the scan, a guardrail rule's parse, a document's passages) must be an
  API call.
- **One card, one picker.** Every memory is drawn by one card function (`card.rs`), and one
  workarea picker serves Memories, the Atlas, Review and Guardrails (`picker.rs`).

Decided, not built.

## Decision

- **d1 — The builder lives on the user side only, on the workarea's own page, and the admin side
  has none** (the operator, 2026-11-14, as drawn in the v18 mockups WorkareaScreen and
  ItemEditor). Every builder page is a path in `PAGES` behind the human-only gate, the CSRF check
  and the hashed stylesheet, with no script (ADR-0045 d8, d26); a write answers a `303` back to
  the page it came from (Post/Redirect/Get, ADR-0070 d3).
    - **The workarea page**, `GET /web/workarea?workarea=<reference>&kind=<kind>`, reached from
      the workarea's row on Workareas, offered for every workarea the person may write in, a
      shared one where they hold dreamer included. A breadcrumb Personal / Workareas /
      `<reference>`; the `<h1>` the reference; the workarea picker above the kinds, keeping the
      kind. "Approved contents by kind", `nav.oml-views`: Memories, Skills, Guardrails and
      Documents, each with its count of approved items, one per claim (Memories leads to the
      Memories cockpit filtered to the workarea). Under the kind: one line where it changes a
      decision, the one primary **New skill**, **New guardrail** or **New document** with a line
      saying who approves under the workarea's policy (in the person's root, `<user>-review`
      marked same user; in a shared workarea, a reviewer outside the author's person, a person for
      a skill or a guardrail), and the table of that kind's claims
      on this workarea alone (not its subtree): name, state with the decision word for a held
      version, version and changed (severity and rule for a guardrail), and the act, **Edit** and
      **Retire** on an approved tip with nothing held after it, "in force · edit v3 waits for
      review" otherwise, and **Judge it** where the person's review key may judge a held version.
      Every version stays listed. Inside an in-progress workarea a held row carries the
      `unreviewed` chip and "served here, unreviewed" (ADR-0080 d10).
    - **Guardrails, In force here.** The Guardrails kind opens with the set an agent connected
      to the workarea loads, inherited first, then this workarea's own (ADR-0083 d12, which
      removes the Personal Guardrails tab), then this workarea's guardrails with New, Edit, Retire
      and Judge it.
    - **The forms**, `GET|POST /web/workarea/new`, `/web/workarea/edit` and
      `/web/workarea/retire` (Personal only), each a `form.oml-form` with a breadcrumb to the
      workarea, the act as `<h1>` ("New skill", "Edit release-checklist", "Retire
      run-tests-before-commit"), one capacity line naming the key and the workarea's policy, the
      fields (d4), and one primary named for the version it produces ("Propose skill",
      "Propose v3", "Propose retirement"); no control says "save", "publish" or "delete".
    - **The admin side** has `GET /web/admin/workarea?workarea=&kind=`, the same contents by kind,
      read-only, and no builder: the builder writes held versions under a person's keys, while an
      admin's change to a skill, a guardrail or a document is one of ADR-0089's fix acts, served at
      once, marked and then confirmed or reverted by a person, taken from the admin workarea
      page's Releases view (ADR-0083 d9); a curator's `universe` items stay on `/web/admin/curate`.
    - A renamed skill is a new skill and a retirement of the old one (the operator, 2026-11-14).

- **d2 — Which key writes is decided by the act and the row, never by the form.**
    - **A new item, in any workarea the person may write in, held** (the operator, 2026-11-14).
      In the person's root and the workareas under it it is `POST /v1/memories` as `<user>-web`
      (capacity `web`), which holds writer there (ADR-0044 d3, d16). In any other workarea where
      the person's `<user>-review` holds dreamer, a shared or crew workarea included, it is the same
      call as `<user>-review` (capacity `review`). That needs one API change: a dreamer key that a
      person session serves may submit a new claim's first version, held as every write is, where
      today a dreamer proposes and supersedes and only a writer starts a claim
      ([roles and keys](../architecture/roles-and-keys.md), verbs table). It widens no read and no
      approval: the version is held, author-only (ADR-0001 R3), and `<user>-review`, its author, is excluded
      from judging it by its lineage (ADR-0001 R2). Agent keys gain nothing, since the change is for session
      keys alone. The builder never writes as `<user>-curate`: curation keeps its own page
      (ADR-0045 d12). Where no key of the person holds writer or dreamer, the builder offers no new
      item.
    - **An edit or a retirement of a row any of the person's keys wrote** is
      `POST /v1/memories/{artifact_id}/supersede` as `<user>-web` inside the person's root, and as
      `<user>-review` outside it (a dreamer supersedes). This needs one API change: a
      session web key supersedes a row authored by any key that acts for its person, read the one
      way `same_user` reads it (ADR-0044 d16), where today a writer supersedes only its own key's
      rows. The successor is still held and still reviewed by another key, so ADR-0001 R2 is untouched; the
      change only lets a person revise what their agents wrote without a dreamer proposal.
    - **An edit or a retirement of another author's row** is the same call as `<user>-review`
      (capacity `review`), a dreamer proposal (ADR-0045 d12), offered only where that key holds
      dreamer; that key is then excluded from reviewing its own proposal (lineage closure).
    - Every write sends `expected_version`, the version the form rendered
      ([ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workspace-in-step-with-a-repositorys-documents.md)
      d19), and an `Idempotency-Key` from the form, so a double submit captures one version. While
      a held successor of the person's own exists the builder offers no second edit of that claim
      (a second would be a rival, ADR-0025 d10); it links the pending version.

- **d3 — An edit is a held supersession, a removal is a retirement proposal, and nothing is
  deleted.** Submit on an existing item supersedes it; the served tip stays the old version until
  the successor promotes (ADR-0025 d10). Retire asks for a confirmation naming what stops when it
  promotes ("the command /oml:`<name>` is no longer offered", "the rule no longer loads", "the
  document is no longer listed or recalled"), then sends `retirement: true` (ADR-0025 d11). No
  control erases, demotes or deletes; rejected versions stay listed to their author under
  Submitted ([ADR-0083](ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md)).

- **d4 — The forms carry exactly each kind's fields.**
    - **Skill:** name (fixed on an edit, ADR-0080 d2), description, body (markdown).
    - **Guardrail:** severity (`never`, `must`, `should`), body, and an optional rule in `oml-gr/1`
      ([ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
      d2, d8), each re-sent on a supersession (d10 of ADR-0037); the body cap and the set cap are
      ADR-0037 d7's (`BODY_MAX_BYTES = 300`, `SET_CAP = 12`, `crates/oml-guardrail/src/lib.rs:39,54`),
      shown beside the field. Guardrails, like every instruction kind, need a person's verdict
      (ADR-0080 d16), and d17 of
      ADR-0037 is amended from "no page writes" to "only the builder writes, under this record".
    - **Document:** title, format (`text/markdown` or `text/plain`), body.
    - Every form has an optional claim-key field only for documents (ADR-0081 d2), and a free
      "why" line the person may fill, sent as the version's `sources` citation where the API
      admits it and otherwise kept out of the write.

- **d5 — Preview is an API call that writes nothing: `POST /v1/memories/preview`.** It takes the
  body of submit, or of supersede with `replaces`, and answers what capture would decide without
  capturing: `{would_capture, refusal?, findings, needs_person, body_bytes, budget_tokens,
  passages?, rule?, offered_as?, shadows?, set_after?}` — the typed refusal submit would answer
  (validation, size, quota headroom, name taken, version conflict), the scan's findings and
  whether a person must approve (ADR-0074 d24), a document's passages by fragment and lines
  (ADR-0081 d3), a guardrail rule's canonical form or its parse error, a skill's prompt name and
  any name it shadows or is shadowed by along the chain (ADR-0080 d5), and a guardrail's set size
  after it at its workarea against the cap. It journals nothing, holds nothing, counts in no meter
  (ADR-0071 d2 counts versions, and none is made), passes the key's request rate (ADR-0079), and
  needs the same grant submit or supersede would. The console draws it on the form's own page
  (the operator, 2026-11-14, as drawn): every form posts **Preview** first (`step=preview`, first
  in the DOM and focus order, so Enter previews), and the page it answers draws the result above
  the form with what a session will receive (a skill's command as the pattern
  `/mcp__<server>__<name>`, its index line and its `prompts/list` entry; a guardrail's
  session-start line, what its rule checks and who loads it; a document's resource address and its
  passages). **Propose** is drawn only on a page a preview answered and posts the previewed
  values; if a field changed, the page redraws the preview instead of writing. A refusal keeps the
  form with every value and the error beside its field, leading with the sentence and ending with
  the code. When the tip moved (`submission.tip_conflict`), the one primary is **Edit from v3 with
  this text** (`step=rebase`), the console's own step, which re-reads the tip and redraws the form
  with the typed text and the change against it; no API change. The cost is one more route and a
  second pass of the scan per previewed edit.

- **d6 — An edit is judged by what changed, drawn on the edit form and on the Judge page** (the
  operator, 2026-11-14, as drawn). The change is the console's own word comparison (longest
  common subsequence over whitespace-split words, struck and underlined, no model), computed in
  `oml-web` from two texts the API already serves (the tip by `GET /v1/memories/{artifact_id}`,
  the draft from the form, or the served version and its successor on Judge), with name,
  description and body each in its own labelled block, "Change against v2 · baseline: v2, the
  served tip". It is drawn on the edit form after Preview and on To do's Judge page
  (`/web/todo/judge`, ADR-0083 d2) for a supersession, against its `replaces`: one more `GET` per
  served item, which `web_console_reads.rs` records with the reason. A list row draws no change;
  it says "Replaces v2" and links the page. A long document's reviewer reads the changed words,
  not the whole text (ADR-0081 d7's stated cost).

- **d7 — Approval is the same as for memories today: inside the person's root their own review
  key may approve, marked `same_user`; in any other workarea a reviewer outside the author's
  person approves, a person for an instruction kind** (the operator, 2026-11-14).
    - **Inside the root.** A version `<user>-web` wrote is approved by `<user>-review` as an agent's
      write is: ADR-0001 R2 holds per key and principal
      ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
      d8), and the verdict carries `same_user: true` (ADR-0046 d2), served as `approver.same_user`
      to every reader (ADR-0077 d2) and in ADR-0080 d6's prompt header. Guardrails, like every
      instruction kind, need a person's verdict (ADR-0080 d16), which the person's own review key
      gives here. A person
      alone can therefore publish their own skills, guardrails and documents in their root, and every
      reader can see that the approver was the author's own person.
    - **A user root never leaves development** (the operator, 2026-11-14: "Cap at development";
      ADR-0085 d17). Every workarea in it is a development scope, so the `same_user` approval above
      holds throughout the root, and production work happens in shared workareas, where the
      approver is never the author's person: for a document any key outside its lineage, a
      reviewer crew's agent key included (the operator, 2026-11-14: "Agents approve knowledge"), and for a skill or a guardrail a person
      (ADR-0080 d16).
    - **Linked tenant keys.** A tenant key acts for a person only once it is linked to that
      person (ADR-0085 d16, `key.linked_person`). Until then the same-person rule below reaches
      the person's user keys and session keys only, and its check for tenant keys ships with
      `key.linked_person`.
    - **Outside it.** A version any key of a person wrote, an agent's key, a session key or a
      tenant key acting for that person, in a workarea outside that person's root counts no
      verdict from a key acting for the same person, read the one way `same_user`
      reads it (ADR-0044 d16), refused `review.author_in_lineage` with `details.reason:
      same_person`, and it is absent from that person's To review (ADR-0083). Under d2 the author
      there is `<user>-review`, which its lineage already excludes, and no other key of the person
      holds dreamer outside the root (user keys stay inside it, ADR-0044 d3; an admin key never
      reviews), so for a console write the rule changes no outcome today. For an agent key's write
      it does: a person's tenant dreamer key no longer approves their own agent's write outside
      their root (the operator, 2026-11-14: "own root only"), which amends ADR-0044 d8 there.
    - The cost: in a shared workarea a person's console write waits for another reviewer, as the
      operator chose; inside a root the `same_user` mark is the only signal that the author's own
      person approved.

- **d8 — The API changes, plainly.**
    1. `POST /v1/memories/preview`, new (d5).
    2. `POST /v1/memories/{artifact_id}/supersede`: a session web key may supersede a row any key
       of its person wrote (d2); `access.author_only` otherwise, unchanged.
    3. `GET /v1/memories` with `with_own`, `author` or ADR-0083's `mine`: admits `kind`, a comma
       list of kinds, `guardrail`, `skill` and `document` among them; today a `kind` beside
       `with_own` is refused as an unknown combination
       (`crates/transports/oml-transport-http/src/lib.rs:822-826`) and the listing's `kind`
       enum omits `guardrail` ([http-api.md](../api/http-api.md), `GET /v1/memories`).
    4. Review: outside the author's root, a version a session key wrote excludes every key of the
       same person (d7), `details.reason: same_person` on the existing `review.author_in_lineage`.
       Inside the root nothing changes.
    5. Submit: a dreamer key served by a person session may start a new claim, held (d2).
    6. No change is needed for `<user>-web` to call `POST /v1/memories`: the session refusals
       name only the key acts (`crates/transports/oml-transport-http/src/console.rs:57-70`,
       `console.rs:1250`, `console.rs:1338`, `console.rs:1393`); the build proves it with a
       failing test first, since ADR-0070 d5's table lists submit under `curate` alone.
    7. An API change decided here: `GET /v2/workareas/{workarea}` gains `approved_by_kind`, the count of approved claims of
       each kind at that workarea, one per claim, for the workarea page's kind links (d1); until it
       is served the links carry no number.

- **d9 — The operator's answers of 2026-11-14.** (1) Approval is as for
  memories today (d7). (2) New items may be started in any workarea the person may write in,
  shared ones where they hold only dreamer included, and all are held (d2). (3) The admin side gets
  no builder view; the admin cockpit's kind filter covers reading. The screen choices are settled
  by the approved design system v18 mockups: d1's workarea page and forms, d5's preview on the
  form's own page, d6's diff on the form and on Judge. Nothing stays open.

## Consequences

- `oml-web` gains `workarea.rs` with d1's routes, the forms, the diff and the preview's
  drawing; `PAGES`, `serves` and `lib.rs`'s route test grow by them: `GET /web/workarea`,
  `GET|POST /web/workarea/new`, `GET|POST /web/workarea/edit`, `GET|POST /web/workarea/retire`
  and `GET /web/admin/workarea`. Workareas' rows link to the workarea page; neither nav gains an
  item (ADR-0083 d12). ADR-0070 d7's list of pages gains them.
- ADR-0070 d5's tables gain: `build_list` → `GET /v1/memories?with_own=true&kind=…` (web);
  `build_submit` → `POST /v1/memories` (web); `build_supersede` → supersede (web or review);
  `build_preview` → `POST /v1/memories/preview` (web or review).
- `web_console_reads.rs` records the builder's reads, the counts by kind and Judge's added `GET` (d6).
- Page tests assert the copy of every state, refusal and confirmation, in the neutral voice
  (the operator's rule of 2026-11-13: no "you", "your" or "my").
- `oml doctor` names a server that does not serve the preview route ("this server predates the
  builder").
- Costs, stated: every edit is a full version that counts its text in `text_bytes`; a person
  in a shared workarea waits for another reviewer (d7); a preview runs the scan twice for one write.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **The builder writes as `<user>-review` always** | A dreamer proposal is the right act for another author's row, but for the person's own new items it would make every write a proposal its own reviewer key may not judge, and `<user>-review` holds no writer to start one. |
| **An admin-side builder** | A builder writes held versions for review; an admin's change is a fix served at once and confirmed afterwards (ADR-0089 d2, d3, d8), a different act on a different page (ADR-0083 d9). Curation has its own key and page (ADR-0045 d12). |
| **A Build page in the nav, or New and Edit inside the Memories cockpit** | The drafted options (a) and (c); the mockups put a workarea's contents and their acts on the workarea's own page, where the policy that decides who approves is shown, and the nav does not grow. |
| **Diff on the edit form only, or on Judge only** | The drafted options (a) and (c); the author checks the change before proposing it and the reviewer judges what changed, so the mockups draw both. |
| **Edit in place, or delete a draft** | ADR-0001 R4 keeps every version, and the only deletion proposed anywhere is ADR-0089's legal erasure, an admin act; a held version is the draft. |
| **Preview computed in `oml-web`** | `oml-web` cannot reach the scan, the rule parser or the passage cutter (ADR-0070 d1), and a second copy of them would drift from capture. |
| **Preview as submit with a `dry_run` flag** | One route whose answer is sometimes a write is easy to call wrongly; a separate route that never writes is checkable by its name. |
| **Another person always approves a console write, in the person's root too** | A person alone could never publish their own skill, guardrail or document; the operator chose "same as memories today" on 2026-11-14, with the `same_user` mark telling readers what the approval is worth. |
| **Split by kind: another person for skills and guardrails, the person's own review key for documents** | Two rules for one page, and the instruction kinds already carry their own guard (a person's verdict for every instruction kind, ADR-0080 d16); the operator chose one rule on 2026-11-14. |
| **Per-key independence everywhere, shared workareas included** | In a shared workarea the readers are other people, who are owed a reviewer other than the author; the operator asked for another person there. |
| **New items only in the person's root, edits elsewhere** | The operator asked on 2026-11-14 for new items anywhere the person may write; everything new is held, so the wider start widens no read. |
| **A rich-text or WYSIWYG editor** | It needs script, which the console does not run (ADR-0045 d26); a textarea of markdown with a server preview needs none. |
