<!--
id: adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved
tier: engineering
status: accepted design · 2026-11-14
answers: what the views of the Memories cockpit show and which API call serves each; why held rows never appear under Approved; where the person judges and where the count of what waits comes from; what Submitted lists and why the API needs a new filter for it; what the admin side shows instead of Judge; what the To do page lists on each side and which read counts it; where the admin fix acts sit; why bulk review leaves the console; what the two navs hold and where the Guardrails view went; how an author withdraws a held version
-->

# ADR-0083 — The Memories page is one cockpit with Approved and Submitted views, what waits on a person is one To do page, and held rows never appear as approved

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with the screen
choices settled from the approved design system v18 mockups (d2, d3, d5 to d9, d11, d12), the withdraw act (d13) decided the same day, and every
other default as drafted · proposed the same day, tracking #374, with the operator's answers
(d10) · aligned with ADR-0089 (admins fix production at once, a person confirms or reverts)

**Release:** planned for v0.7.4

**Amends:** ADR-0045 (d9 and d11: the user page's Memories view gains Submitted, and its Review
view becomes the To do page with Judge at `/web/todo/judge`; d10: the admin page's Memories and
Review views become one page with no Judge; the navs, d12), ADR-0070 (d5 and d7: the cockpit's and
To do's calls and pages), ADR-0055 (d10: no console page selects in bulk; admin fixes join the
person's review as a To do group), ADR-0037 (d17: the Personal Guardrails view becomes the
workarea page's In force here panel, d12)

**Depends on:** ADR-0089 (PR #377, branch `design/workarea-lifecycle`): its fix acts, its
confirm and revert routes and its person-side fix read (its d2, d3, d8, d15); ADR-0084, ADR-0085
and ADR-0086 (PR #377) for the To do groups their acts add (d11)

**Twin parts** (ADR-0090 d18, PR #388): the cockpit, the To do page and the counts read store
nothing. A withdrawal (d13) is the journal event `memory.withdrawn`, whose fold sets the version's
state, so it is journal.

## Context

On 2026-11-14 the operator asked to "make memories cockpit out of memories tab" (#374): one tab
replacing the separate Review and Memories pages, with one card, one workarea picker and one
search over three views, Approved (the default), To review (others' held memories the person may
judge, with Judge and a count), and Submitted (the person's own held and rejected writes); held
rows never in Approved; the admin side the same with no Judge, since an admin key gives no verdict
([ADR-0001](ADR-0001-the-constitution.md) R1; ADR-0045 d6). The rules R1 to R4 are ADR-0001's;
this record cites them and restates none. The same day the operator decided that admins fix
production at once, each fix marked, journalled with a reason and then confirmed or reverted by a
person (ADR-0089), and admins already read memory text, each read journalled (ADR-0045
d21).

What serves each part today:

- **Memories** is `GET /web/memories` and `GET /web/admin/memories`, **Review** is
  `GET /web/review` and `GET /web/admin/review` (`crates/shared/oml-page/src/web/mod.rs:67-90`;
  router `crates/clients/oml-web/src/lib.rs:291-302`), each with the one picker (`picker.rs`) and
  the one card (`card.rs`).
- **The user list** is `GET /v1/memories?with_own=true`: the scoped listing at `universe` or the
  picked workarea merged with the rows of every key naming the person, newest first, one cursor
  (`crates/oml-app/src/app/pages.rs:865-942`). It takes `state` as one value of `held`,
  `promoted`, `superseded`, `rejected` or `demoted`, where `promoted` means the promoted tip
  (`pages.rs:881-889`), `author` (one of the person's keys, which drops the scoped part),
  `origin_kinds`, `include_text`, `limit` and `cursor`, and refuses any other parameter, `kind`
  included (`crates/transports/oml-transport-http/src/lib.rs:764-826`). Search is recall, which
  serves promoted rows unless `include_unreviewed` asks for the caller's own held ones
  ([http-api.md](../api/http-api.md), recall).
- **Review** is the served ceremony under `<user>-review`: `POST /v2/review/next`,
  `POST /v2/review/verdicts`, the bulk act and `GET /v2/review/held`, which lists "the held items
  this person's reviewer key may judge" and requires "a dreamer key held_by human" (http-api.md,
  `GET /v2/review/held`; ADR-0055). No call counts them: `GET /v2/review/queue` counts held rows
  per workarea for the admin role only, without content (http-api.md, `GET /v2/review/queue`).
- **The admin read** is `POST /v2/admin/memories/search` with `state` and `workarea` filters,
  every memory of the tenant with text and attribution, journalled per read (ADR-0045 d21, d24).

Two gaps follow: the person's own rows cannot be listed alone across all their keys (the merged
listing always adds the scoped part, and `author` names one key), and To review has no count.

Decided, not built.

## Decision

- **d1 — Approved is promoted tips only, and no held row can reach it.** It is
  `GET /v1/memories?with_own=true&state=promoted&include_text=true` at the picked workarea, as
  `<user>-web`, and the search box's recall with `include_unreviewed: false` sent explicitly, so an
  in-progress scope's default of `true` (ADR-0030 d4) does not apply. `state=promoted` reads the
  promoted tip (`pages.rs:884`), so neither the person's own held rows nor, inside an in-progress
  workarea, other authors' unreviewed rows appear; a page test proves both with a held row present.
  The view's facts on each card are the card's, `approved_at` and `approver` among them (ADR-0077
  d1). Approved is the default view.

- **d2 — What the person may judge is the To do page's To review group, judged at
  `/web/todo/judge`, and counted by the To do counts read** (the operator, 2026-11-14, approving
  the design system v18 mockups). The group lists `GET /v2/review/held` under capacity `review`,
  oldest first, every workarea or the one the To do filter picks; each row names the decision
  (New memory, New skill, Edit of v2, Retirement of v1), the kind, the workarea, the author with
  the person or agent glyph and word, `same user` where it applies, the approvals the workarea's
  policy asks for, and the wait. Its act is **Judge**, which opens `/web/todo/judge?memory=<id>`:
  the item at page density, the change against the served version (ADR-0082 d6), Verdict, Reason
  and **Record verdict**, the served ceremony for that item (ADR-0045 d11; the "Open to judge" of
  2026-11-13), moved off the Review page. The verdict posts to the same path; after it the page
  returns to To do with its notice, and **Next** on Judge opens the next To do item without a
  verdict. The breadcrumb is Personal / To do / Judge and the title `Judge · To do · OML`. The
  count is the `review` member of the To do counts read (d11), which counts the rows this list
  would serve up to 1,000 (chosen, not measured) with no content and no claim; the drafted
  `count=true` on `GET /v2/review/held` is not added, since one read serves every group's count.
  Under ADR-0082 d7, what the person wrote in the console inside their own root is theirs to
  judge, and its row and Judge page say the approval will be marked `same_user`; what they wrote
  outside it is not, and is neither listed nor counted here. A person with no `<user>-review`
  grant beyond their root sees their own root's held writes only.

- **d3 — Submitted is every row any of the person's keys wrote that is held, rejected or
  withdrawn, and what they had approved in the last 30 days; it needs a `mine` filter** (the
  operator, 2026-11-14, as the v18 mockups draw it). The API change: `GET /v1/memories` gains
  `mine=true`, admitted for a person's session web key: the rows of every key naming its person
  (the own part of `with_own`, `pages.rs:908-910`) with no scoped part, under the same cursor;
  `state` beside it takes a comma list; and `promoted_since=<instant>` limits the `promoted` rows
  among them to those approved at or after that instant, leaving the other states whole. Submitted
  reads `mine=true&state=held,rejected,withdrawn,promoted&promoted_since=<now minus 30 days>`, its
  State filter defaulting to all of them. `author` keeps meaning one key and narrows `mine`. Each
  row leads with one plain sentence: "Waiting for review. v2 is still served.", "Waiting for 2
  approvals …", "Approved on `<date>` and served as `<command, address or claim>`", "Rejected
  `<date>`", "Withdrawn `<date>`". Each card shows its state, its kind (a skill, a guardrail or a
  document from the builder, ADR-0082), the version it replaces if any, and the held reason the
  API already serves, scan findings and `needs_person` among them (ADR-0074 d17); a held row
  offers Withdraw (d13). A rejected or withdrawn row stays listed forever (ADR-0001 R4); why a
  version was rejected is not served to its author today (d10). Every row in Submitted is the
  caller's person's own, as ADR-0001 R3 requires.

- **d4 — One picker, one search, one card across the views.** The picker's workarea and
  the search query carry across the views. Search runs recall in Approved; in Submitted, which
  recall does not serve, it is not offered, and To do filters by workarea only, since a text search over held
  rows is a read fenced to their author that recall's shared index must never serve (ADR-0025 d5).
  Each view draws the same card (`card.rs`) in its row density.

- **d5 — The admin side has the same frame, three views and no Judge: Approved, Held and
  Rejected** (the operator, 2026-11-14, as drawn). Every admin view reads through `<user>-admin`
  and the journalled admin read (ADR-0045 d21, d24): Approved, the default, is
  `POST /v2/admin/memories/search` with `state` `promoted` (or `superseded` from the filter);
  Held is the same search with `state=held`, under a "Held per workarea" table from
  `GET /v2/review/queue` (workarea, held, oldest since, oldest first), each workarea a link that
  filters the rows; Rejected is the search with `state=rejected`. The current view's tab carries
  its count as a word and a number ("Held 21"), the others none, and the Memories nav item none.
  The capacity line on Held says the admin key never judges and that verdicts are given on the
  Personal side under the person's review key. A held row offers "Judge as `<user>-review`", a
  link to `/web/todo/judge?memory=<id>` on the Personal side, where the person's own review key
  is refused `review.author_in_lineage` if it wrote the row and `access.not_found` without a
  grant. No admin view draws Judge, Review next, a pick box or a verdict form; the only writes on
  the admin side are ADR-0089's fix acts (d9). The search's `kind` filter is an API change shared
  with the Personal side (ADR-0082 d8, item 3).

- **d6 — The routes, and a "To do · N" badge beside the account name on every page in place of
  any nav count** (the operator, 2026-11-14).
    - **Personal Memories** is `GET /web/memories` (Approved) and `GET /web/memories?view=submitted`,
      two links in `nav.oml-views` labelled "Memories views", the current one
      `aria-current="page"`; the views keep the workarea and every active filter.
    - **Administration Memories** is `GET /web/admin/memories` and `?view=held|rejected` (d5).
    - **To do** is `GET /web/todo` and `GET /web/admin/todo`, with `POST /web/todo`
      (`action=next`), `GET|POST /web/todo/judge?memory=` (d2) and `GET|POST /web/todo/fix?id=`
      (d8) (d11).
    - **The old paths redirect for one release, then go** (as the mockups draw it).
      `/web/review`, `/web/memories?view=review` and `/web/memories?view=fixes` answer `308` to
      `/web/todo`, keeping the workarea; `/web/admin/review` answers `308` to
      `/web/admin/memories?view=held`; `/web/guardrails` answers d12's `308`. The redirects ship in
      the release that builds this record and are removed in the next one, after which those paths
      answer the console's not-found page.
    - **The count.** Every page of both sides carries "To do · N" as a badge in the header's right
      corner, beside the account name and before it, a link to `/web/todo` (Personal) or
      `/web/admin/todo` (Administration): a word and a number in a pill outline, no colour
      carrying the count, no popup, no script, "To do · 0" at zero, `aria-current="page"` on the
      To do page itself. At 390px it stays in the header's first row beside the name. No nav item
      carries a count on either side; the Memories tab is plain. The badge covers every workarea
      whatever the page's filter. It is drawn from the To do counts read (d11), one store call per
      page view, which `web_console_reads.rs` records with the reason.
    - Every view stays server-rendered, paged by cursor, with no script (ADR-0045 d22, d26), in
      the neutral voice.

- **d7 — Bulk review leaves the console; the CLI keeps it** (the operator, 2026-11-14). With
  Judge gone from the admin side (d5) the admin page has no review view to hold ADR-0055 d10's
  bulk selection, and the person's To do judges one item per act. Bulk review stays the CLI's,
  `oml review list` and `oml review approve|reject --ids` (ADR-0055 d11), under the person's
  reviewer key. ADR-0055 d10 is amended: no console page selects in bulk.

- **d8 — Admin fixes to confirm is a To do group, with one page per fix** (the operator,
  2026-11-14). ADR-0089 d3 opens a confirmation item for every admin fix in the review queue of
  the persons who hold dreamer on its workarea. The To do group "Admin fixes to confirm" lists,
  for the filtered workarea or every workarea, each unconfirmed fix on a workarea where the
  person's `<user>-review` holds dreamer: the act, its target, `unconfirmed`, `alert` when it
  loosens a guardrail (ADR-0089 d6), the reason and the wait; never which admin (ADR-0089 d7).
  Its act, **Confirm or revert**, opens `/web/todo/fix?id=<fx>`: the target's card as it is served
  now (marked `act: admin_fix`, `confirmed: false`, ADR-0089 d7), the version it replaced where
  there is one, the reason and when, with **Confirm** and **Revert**, each asking for confirmation
  and posting to ADR-0089 d15's `POST /v2/review/fixes/{id}/confirm|revert` as `<user>-review`
  (capacity `review`). The list is `GET /v2/review/fixes` (ADR-0089 d15), which serves a person's
  dreamer key. A fix the person's own principal took is listed without either act (ADR-0089 d15);
  an erasure offers Confirm and a dispute comment only. A revert refused because a promoted
  version has since superseded the target answers `admin.fix_superseded` (ADR-0089 d8). The
  Memories cockpit has no fixes view. ADR-0055 is amended: the review a person runs covers held
  items and admin fixes, each its own act.

- **d9 — The fix acts sit on the admin workarea page's Releases view** (the operator,
  2026-11-14). `/web/admin/workarea?workarea=<w>&view=releases` carries a **Fix** form with
  ADR-0089's acts (demote, edit, retire, release rollback, import rollback, erase), each with its
  target and the required reason, and a confirmation step
  (`…&view=releases&act=<act>&target=<target>`) that shows what each release binding gets and
  says the fix is served at once and then waits for a person to confirm or revert it; the form
  posts to `/web/admin/fixes`, which calls `POST /v2/admin/fixes` under capacity `admin`. Neither
  the admin one-memory page nor any admin card carries a fix act. An admin's edit of a skill, a
  guardrail or a document is this act, served at once and marked, never a held version of the
  builder (ADR-0082 d1). Rolling back a release or an import is among the acts here; moving a
  status stays a workarea act on the admin Workareas page (ADR-0045 d28). A ladder move is not a
  fix (ADR-0009 d3).

- **d10 — The operator's answers of 2026-11-14, and what stays open.** To do's counts have their
  own read (d11), not a first page counted to "200+". The screen choices are settled by the
  approved design system v18 mockups (TodoScreen, MemoriesCockpit, AdminMemoriesCockpit,
  AdminFixReview, AdminReleases, WorkareaScreen, BrandHeader): d3's Submitted, d5's views, d6's
  routes and badge, d7's bulk review in the CLI only, d8's To do group and fix page, d9's Releases
  view, d11's To do page and d12's navs. A held version's author may withdraw it (d13), decided
  and to be built later. Open: serving a rejection's reason to its author with no reviewer named
  (ADR-0077 d2), which an issue tracks and Submitted does not show until then; a read of a shared
  workarea's reviewer count, which the mockups mark as needing an API and this record does not
  add; and `POST /v2/review/next` taking the To do filter's workarea, which the mockups ask for and
  this record does not decide.

- **d11 — One To do page per side lists what waits on its key, and one counts read serves the
  badge.**
    - **Personal, `/web/todo`.** `<h1>` To do, title `To do · OML`, one capacity line, the
      workarea filter (`WorkareaField`, GET; empty means every workarea), and **Next**, the page's
      one primary: a `POST` (`action=next`) that keeps the workarea and answers `303` to the oldest
      item's page across the groups. The groups, in order, each an `<h2>` "Name · N" with one line
      naming whose items they are and under which key, rows oldest first, an item in one group
      only: **To review** (d2; Judge), **Admin fixes to confirm** (d8; Confirm or revert),
      **Releases to ratify** (ADR-0085; Ratify), **Withdrawal alerts to acknowledge** (ADR-0085
      d9; Acknowledge), **Gate overrides** (ADR-0086; Override, never for the scan or review
      gates), **Imported versions to review** (ADR-0084; Judge with the source-author
      attestation). Empty groups fold into one line; with every group empty the page says
      "Nothing waits for this account." and links to Memories, Submitted.
    - **Administration, `/web/admin/todo`.** Title `Admin · To do · OML`, no filter, admin work
      only: **Admin fixes, unconfirmed** (fixes this admin key took, with their alert), 
      **Guardrail-loosening alerts** (withdrawals that stopped a guardrail, until a person of
      another principal acknowledges them; an alert raised by one of the admin's own fixes is
      listed with that fix) and **Open investigations** (ADR-0089 d10; the row opens the
      investigation). No row offers a verdict, a ratification, a confirmation or an
      acknowledgement: those are a person's acts (ADR-0001 R1).
    - **The counts read, an API change decided here.** `GET /v2/todo/counts?workarea=` under a
      person's session web key (capacity `web`) answers `{review, fixes, ratify, withdrawals,
      overrides, imports, truncated}`: for each group, the length the group's own list read below
      would serve to the person's review keys at that workarea and below it, or across every
      workarea without `workarea`, each counted up to 1,000 with `truncated` set past it, with no
      content, no claim key and no name. It widens nothing: every count is computed under the same
      grants, lineage exclusions and author fences as the list it counts, so it reports only items
      those keys could already list, and a workarea they cannot read contributes zero and is not
      named. Its administration counterpart, `GET /v2/admin/todo/counts` under capacity `admin`,
      answers `{fixes, alerts, investigations, truncated}` for the admin key, counted the same way.
      The badge is the sum.
    - **The lists.** Each list is under the person's `<user>-review` key (capacity `review`)
      across the workareas where it holds dreamer, or the one `workarea` names, oldest first, paged
      by cursor, and never names an admin or another person's key beyond what the act's own record
      serves.
        - To review: `GET /v2/review/held`, as today. Imported versions to review are the same
          read's rows that carry ADR-0084's `import_provenance` (its d4), which the held listing
          serves with each item (an API change decided here), grouped apart by the console.
        - Admin fixes to confirm: `GET /v2/review/fixes` (ADR-0089 d15).
        - Releases to ratify, an API change decided here: `GET /v2/review/releases?workarea=&cursor=`,
          the proposed releases (ADR-0085 d4) the key may publish, that is, whose proposer's
          principal differs from its own, with workarea, label, proposal id, the proposer's key
          name and a gate summary.
        - Withdrawal alerts to acknowledge, an API change decided here:
          `GET /v2/review/withdrawals?workarea=&cursor=`, the withdrawals that loosened a guardrail
          and wait for acknowledgement by a person of another principal (ADR-0085 d9).
        - Gate overrides, an API change decided here: `GET /v2/review/gates?workarea=&cursor=`, the
          blocking tests and impact gates (ADR-0086 d6) of proposals the key may ratify, each with
          its case or finding.
        - Administration: `GET /v2/admin/fixes?state=unconfirmed` (ADR-0089 d15), the doctor's
          guardrail-loosening alerts, and `GET /v2/admin/investigations?state=open` (ADR-0089 d15).
      **Next** picks the oldest first row among the groups the page drew; a held row opens Judge,
      any other row the page of its act.

- **d12 — The Personal Guardrails tab folds into the workarea page, and the navs are fixed**
  (the operator, 2026-11-14).
    - The Personal Guardrails view (ADR-0037 d17) is removed. Its set is the workarea page's
      **In force here** panel (ADR-0082 d1), which lists every guardrail an agent connected to the
      workarea loads at session start: first the inherited ones, each marked "inherited from
      `<workarea>`" and linking there, then this workarea's own, marked "here"; one with a held
      retirement is marked "binding while its retirement waits". One line gives the counts under
      the key's `guardrail_scope` (`ancestors` or `workarea`). It reads `GET /v1/guardrails?workarea=`
      as `<user>-web`, the read the Guardrails page makes today, so no API change.
      `/web/guardrails?workarea=<w>` answers `308` to `/web/workarea?workarea=<w>&kind=guardrail`;
      with no workarea, to the person's root's.
    - **The nav rule.** The Personal nav is Start, Memories, Atlas, Keys, Workareas; the
      Administration nav is People, Quotas, Memories, Atlas, Keys, Workareas, Curate. Review leaves
      both, Guardrails leaves the Personal one, and neither carries To do, which is the header
      badge (d6). The switch to the other side stays outside the nav.

- **d13 — The author of a held version may withdraw it** (the operator, 2026-11-14; decided, to be
  built later). A version's author withdraws it with
  `POST /v1/memories/{artifact_id}/withdraw {reason?}`, and over MCP with the tool
  `oml_withdraw {artifact_id, reason?, idempotency_key}`, which joins the agent-safe catalog
  (amends ADR-0025 d12 and ADR-0039's tool table). Who may: the author key itself, an agent's key
  included, on its own held version; and a person's session web key, from the console, for a held
  version any key of that person wrote (read the one way `same_user` reads it, as ADR-0082 d2 reads
  it for supersession) (the operator, 2026-11-14). R1 is untouched: a withdrawal is the author
  retracting its own unreviewed work, never a verdict on anyone's; it promotes nothing, signs
  nothing, demotes no served version and deletes nothing, since the version and its text are kept. It works in any workarea and only on a
  held version; any other state answers `memory.not_held`, and another author's version
  `access.author_only`. The version's state becomes `withdrawn`: it is kept as history with its
  text (ADR-0001 R4), leaves every review queue, the To do page and its counts, is never served
  (not inside an in-progress workarea, not as a skill, guardrail or document, never in a release
  cut), and is listed to its author under Submitted. It is journalled as `memory.withdrawn
  {artifact_id, reason}` under the withdrawing key; no verdict is recorded and no reviewer is
  involved. `state` on the listings admits `withdrawn`. Metrics: `oml_memories_withdrawn_total`.
  Doctor: `memory.withdrawn_in_queue` fails if a withdrawn version is still in a review queue,
  which the act's own transaction rules out. The console offers Withdraw on Submitted's held rows
  and on the workarea page's held rows of the person's own, each with a confirmation, in place of
  the mockups' "Cannot be withdrawn yet". A withdrawn version can be followed by a new one only
  through a fresh write or supersession of the served tip.

## Consequences

- API changes: `mine=true`, `promoted_since` and a comma-list `state` with `withdrawn` on
  `GET /v1/memories` (d3, d13); `POST /v1/memories/{artifact_id}/withdraw` (d13); the three
  `/v2/review/…` list reads and the import marker on the held listing (d11);
  `GET /v2/todo/counts` and `GET /v2/admin/todo/counts` (d11); `kind` on the admin search (d5);
  `openapi.yaml`, `http-api.md` and the Rust SDK's operation table (ADR-0070 d9) follow in the
  serving change.
- `oml-web`'s `memories.rs` keeps Approved and gains Submitted; `review.rs` becomes the To do
  page with Judge and the fix page; `shell` draws the To do badge on both sides; `PAGES`,
  `PERSONAL_TABS`, `ADMIN_TABS` and the route test follow d6 and d12; the `308`s keep every
  existing link working.
- Page tests: a held row present at the workarea never appears under Approved, inside and
  outside an in-progress workarea; Submitted shows the person's held and rejected rows across two
  of their keys and nothing scoped; the badge equals the sum of the counts read and To review's
  group length below the cap; the admin views and the admin To do draw no Judge, no verdict form
  and no confirm; neither nav carries a count.
- Costs, stated: one more store call on every page of both sides for the badge; a person who only
  reviews starts from To do, one click from Memories, and judges one item per act in the console
  (bulk review is the CLI's).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Approved as the merged listing with no state filter, held rows badged** | The operator asked that held rows never appear in Approved; a badge still shows them. |
| **Submitted from `with_own=true&state=held` and a second call for `rejected`** | The merged listing adds the scoped part, which inside an in-progress workarea holds other authors' held rows (ADR-0030 d3), so it is not "the person's own"; two calls also break one cursor. |
| **Submitted from `author=<key>` per key** | One call per key the person holds, merged in `oml-web`, with no shared cursor. |
| **Count from `GET /v2/workareas/{w}/tree?counts=true`** | Its `held` counts the person's own held rows, "as the caller's with_own listing would show them" (http-api.md, tree), not the rows they may judge. |
| **An admin Judge** | An admin key gives no verdict ([ADR-0001](ADR-0001-the-constitution.md) R1; ADR-0045 d6); its changes are ADR-0089's fix acts, which a person confirms. |
| **To review and Fixes to confirm as views of Memories, the count on the nav's Memories item** | The drafted options; the mockups showed the waiting work invisible from every other tab, and what waits is wider than memories (releases, gates, imports). The operator chose one To do page and a header badge on 2026-11-14. |
| **To do as a nav item** | It would lengthen both navs and read as one more page among peers; the operator chose a badge beside the account name. |
| **`count=true` on each listing, read per group** | Six count reads on every page view; one counts read serves the badge. |
| **Fix acts on the admin one-memory page or every admin card** | A fix reaches release-bound keys and bindings (ADR-0089 d5), which the Releases view shows beside the form; one place for a break-glass act. |
| **Bulk review stays on the admin page with Judge removed around it** | A review view on the admin side would hold the one act there that is not an admin's, and the admin side no longer reviews. |
| **Admin fixes confirmed on the admin side** | A fix is confirmed by a person's dreamer key whose principal differs from the admin's (ADR-0089 d8); the admin side acts as the admin key. |
| **Search over held rows in To review and Submitted by recall** | Recall's shared index never serves another author's held text (ADR-0025 d5); a held row is matched only through a read fenced to its author. |
