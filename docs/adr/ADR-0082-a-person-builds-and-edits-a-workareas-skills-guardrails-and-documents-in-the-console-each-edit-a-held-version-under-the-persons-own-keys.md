<!--
id: adr/ADR-0082-a-person-builds-and-edits-a-workareas-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys
tier: engineering
status: proposed
answers: which console routes build and edit a workarea's skills, guardrails and documents; which of the person's keys writes each act and why the admin side writes none; why an edit is a held supersession and a removal a retirement proposal; how a guardrail is edited within ADR-0037; what preview shows and which call serves it; who may review what a person wrote in the console; what the API must add
-->

# ADR-0082 — A person builds and edits a workarea's skills, guardrails and documents in the console, each edit a held version under the person's own keys

**Status:** proposed · 2026-11-14 · tracking #374 · revised the same day with the operator's
answers (d7, d9) · what a screen shows (d1's layout, d5's
preview drawing, d6's diff placement) is offered as options until the operator approves the
builder mockups, published as artifacts first (the operator, 2026-11-14); the identities, the API
and the review rule are decided here

**Release:** planned for v0.7.4

**Amends:** ADR-0045 (d9: the user page writes skills, guardrails and documents through the
builder; d12: a person's web key supersedes rows any of the person's keys wrote), ADR-0037 (d17:
a page writes guardrails, through the builder only), ADR-0070 (d5: the builder's calls in its
tables), ADR-0044 (d8: outside the author's root, a version a person's session key wrote is excluded
from review by every key acting for the same person), the roles table (a person's dreamer session
key may start a held claim)

**Depends on:** [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md)
(kind `skill`), [ADR-0081](ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md)
(kind `document`)

## Context

On 2026-11-14 the operator asked to "add builder and editor to workarea, it should allow
introducing or editing skills, guardrails, rag documents" (#374): every edit a new held version,
reviewed by a different identity (R2), kept forever (R4).

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
- **Independence is per key.** R2 excludes the keys and principals of a row's lineage closure,
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

- **d1 — The builder lives on the user side only, behind its own paths, and the admin side has
  none; its layout waits for the mockups.** Whatever the layout, these hold: every builder page is
  a path in `PAGES` behind the human-only gate, the CSRF check and the hashed stylesheet, with no
  script (ADR-0045 d8, d26); a write answers a `303` back to the list it came from
  (Post/Redirect/Get, ADR-0070 d3); the picker's `workarea` chooses the workarea, the person's root
  by default; a list shows the claims of a kind on that workarea alone (not its subtree), one card
  per tip with its state, a "pending" line when a held successor of the person's own exists, and
  only the acts d2 admits. The admin side has no builder, since an admin key writes no claim (R1);
  a curator's `universe` items stay on `/web/admin/curate`. The layout is one of:
    - **(a) One Build page.** `GET /web/build` with three sections, Skills, Guardrails and
      Documents; `GET /web/build/edit?kind=…|memory=…` the form; `POST /web/build/edit` with
      `action` `preview`, `submit` or `retire`. A nav item, Build.
    - **(b) Inside Workareas.** The Workareas view's per-workarea acts (ADR-0045 d28) gain
      "Skills", "Guardrails" and "Documents", each opening `GET /web/workareas/build?workarea=…&kind=…`
      with the list and the form on one page. No new nav item.
    - **(c) Inside the Memories cockpit.** The cockpit's kind filter (ADR-0083) lists the three
      kinds, and each card and the empty state offer "New" or "Edit", opening the form at
      `/web/build/edit`. The Guardrails view gains the same "New" and "Edit".

    Recommended: (a), since it keeps writing out of the reading pages and gives the operator's
    "builder" a name in the nav; (b) if the nav must not grow. The route names below use (a).

- **d2 — Which key writes is decided by the act and the row, never by the form.**
    - **A new item, in any workarea the person may write in, held** (the operator, 2026-11-14).
      In the person's root and the workareas under it it is `POST /v1/memories` as `<user>-web`
      (capacity `web`), which holds writer there (ADR-0044 d3, d16). In any other workarea where
      the person's `<user>-review` holds dreamer, a shared or crew workarea included, it is the same
      call as `<user>-review` (capacity `review`). That needs one API change: a dreamer key that a
      person session serves may submit a new claim's first version, held as every write is, where
      today a dreamer proposes and supersedes and only a writer starts a claim
      ([roles and keys](../architecture/roles-and-keys.md), verbs table). It widens no read and no
      approval: the version is held, author-only (R3), and `<user>-review`, its author, is excluded
      from judging it by its lineage (R2). Agent keys gain nothing, since the change is for session
      keys alone. The builder never writes as `<user>-curate`: curation keeps its own page
      (ADR-0045 d12). Where no key of the person holds writer or dreamer, the builder offers no new
      item.
    - **An edit or a retirement of a row any of the person's keys wrote** is
      `POST /v1/memories/{artifact_id}/supersede` as `<user>-web` inside the person's root, and as
      `<user>-review` outside it (a dreamer supersedes). This needs one API change: a
      session web key supersedes a row authored by any key that acts for its person, read the one
      way `same_user` reads it (ADR-0044 d16), where today a writer supersedes only its own key's
      rows. The successor is still held and still reviewed by another key, so R2 is untouched; the
      change only lets a person revise what their agents wrote without a dreamer proposal.
    - **An edit or a retirement of another author's row** is the same call as `<user>-review`
      (capacity `review`), a dreamer proposal (ADR-0045 d12), offered only where that key holds
      dreamer; that key is then excluded from reviewing its own proposal (lineage closure).
    - Every write sends `expected_version`, the version the form rendered
      ([ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
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
      shown beside the field. A guardrail is never self-signed (ADR-0037 d10), and d17 of
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
  needs the same grant submit or supersede would. What the console draws from the answer waits for the
  mockups; the options are (a) the result beside the form on the same page, (b) a separate preview
  page with Back and Submit, and in either, optionally, what a session will receive (a skill's
  prompt text with ADR-0080 d6's header, a guardrail's frame line as the hook prints it, a
  document's passages). Recommended: (a) with the session's view, since it needs no extra route. The cost is one more route and a second
  pass of the scan per previewed edit.

- **d6 — An edit can be judged by what changed; where the diff is drawn waits for the
  mockups.** A line diff computed in `oml-web` from two texts the API already serves (the tip by
  `GET /v1/memories/{artifact_id}`, the draft from the form) needs no API change. Where it shows is
  one of: (a) on the edit form only; (b) on the form and on Review's card of a supersession,
  against its `replaces`, one more `GET` per served review item, which `web_console_reads.rs`
  records with the reason; (c) on Review only. Recommended: (b), since a long document's reviewer
  otherwise reads the whole text to find one changed line (ADR-0081 d7's stated cost).

- **d7 — Approval is the same as for memories today: inside the person's root their own review
  key may approve, marked `same_user`; in any other workarea another person approves** (the
  operator, 2026-11-14).
    - **Inside the root.** A version `<user>-web` wrote is approved by `<user>-review` as an agent's
      write is: R2 holds per key and principal
      ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
      d8), and the verdict carries `same_user: true` (ADR-0046 d2), served as `approver.same_user`
      to every reader (ADR-0077 d2) and in ADR-0080 d6's prompt header. Guardrails keep their own
      rule: never self-signed (ADR-0037 d10), which a verdict by another key satisfies. A person
      alone can therefore publish their own skills, guardrails and documents in their root, and every
      reader can see that the approver was the author's own person.
    - **Outside it.** A version a person's session key wrote in a workarea outside that person's
      root counts no verdict from a key acting for the same person, read the one way `same_user`
      reads it (ADR-0044 d16), refused `review.author_in_lineage` with `details.reason:
      same_person`, and it is absent from that person's To review (ADR-0083). Under d2 the author
      there is `<user>-review`, which its lineage already excludes, and no other key of the person
      holds dreamer outside the root (user keys stay inside it, ADR-0044 d3; an admin key never
      reviews), so the rule changes no outcome today; it states the operator's answer as a rule, so
      a later grant cannot quietly let a person approve what they typed in a shared workarea.
    - The cost: in a shared workarea a person's console write waits for someone else, as the
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

- **d9 — The operator's answers of 2026-11-14, and what stays open.** (1) Approval is as for
  memories today (d7). (2) New items may be started in any workarea the person may write in,
  shared ones where they hold only dreamer included, and all are held (d2). (3) The admin side gets
  no builder view; the admin cockpit's kind filter covers reading. Open until the mockups are
  approved: d1's layout, d5's preview drawing and d6's diff placement.

## Consequences

- `oml-web` gains `build.rs` with the routes d1's chosen layout names, the forms, the diff and
  the preview's drawing; `PAGES`, `serves` and `lib.rs`'s route test grow by them (under (a):
  `GET /web/build`, `GET /web/build/edit`, `POST /web/build/edit`, and Build in the user side's
  nav).
- ADR-0070 d5's tables gain: `build_list` → `GET /v1/memories?with_own=true&kind=…` (web);
  `build_submit` → `POST /v1/memories` (web); `build_supersede` → supersede (web or review);
  `build_preview` → `POST /v1/memories/preview` (web or review).
- `web_console_reads.rs` records the builder's reads and Review's added `GET` (d6).
- Page tests assert the copy of every state, refusal and confirmation, in the neutral voice
  (the operator's rule of 2026-11-13: no "you", "your" or "my").
- `oml doctor` names a server that does not serve the preview route ("this server predates the
  builder").
- Costs, stated: every edit is a full version that counts its text in `text_bytes`; a person
  in a shared workarea waits for another person (d7); a preview runs the scan twice for one write.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **The builder writes as `<user>-review` always** | A dreamer proposal is the right act for another author's row, but for the person's own new items it would make every write a proposal its own reviewer key may not judge, and `<user>-review` holds no writer to start one. |
| **An admin-side builder** | Admin keys never write claims (R1); curation has its own key and page (ADR-0045 d12). |
| **Edit in place, or delete a draft** | R4: nothing is deleted and the journal is never edited; a held version is the draft. |
| **Preview computed in `oml-web`** | `oml-web` cannot reach the scan, the rule parser or the passage cutter (ADR-0070 d1), and a second copy of them would drift from capture. |
| **Preview as submit with a `dry_run` flag** | One route whose answer is sometimes a write is easy to call wrongly; a separate route that never writes is checkable by its name. |
| **Another person always approves a console write, in the person's root too** | A person alone could never publish their own skill, guardrail or document; the operator chose "same as memories today" on 2026-11-14, with the `same_user` mark telling readers what the approval is worth. |
| **Split by kind: another person for skills and guardrails, the person's own review key for documents** | Two rules for one page, and the instruction kinds already carry their own guard (ADR-0080 d3's person approval, ADR-0037 d10's never self-signed); the operator chose one rule on 2026-11-14. |
| **Per-key independence everywhere, shared workareas included** | In a shared workarea the readers are other people, who are owed a reviewer other than the author; the operator asked for another person there. |
| **New items only in the person's root, edits elsewhere** | The operator asked on 2026-11-14 for new items anywhere the person may write; everything new is held, so the wider start widens no read. |
| **A rich-text or WYSIWYG editor** | It needs script, which the console does not run (ADR-0045 d26); a textarea of markdown with a server preview needs none. |
