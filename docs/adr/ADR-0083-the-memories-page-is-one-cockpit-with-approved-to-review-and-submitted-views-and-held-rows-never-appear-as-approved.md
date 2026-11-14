<!--
id: adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved
tier: engineering
status: proposed
answers: what the three views of the Memories cockpit show and which API call serves each; why held rows never appear under Approved; how To review judges and where its count comes from; what Submitted lists and why the API needs a new filter for it; what the admin side shows instead of Judge; which screen choices wait for the mockups
-->

# ADR-0083 — The Memories page is one cockpit with Approved, To review and Submitted views, and held rows never appear as approved

**Status:** proposed · 2026-11-14 · tracking #374 · what a screen shows (d6's routes, tabs and
count placement, d5's admin views) is offered as options until the operator approves the cockpit
mockups, published as artifacts first (the operator, 2026-11-14); which call serves each view, and
what each view may never show, are decided here

**Release:** planned for v0.7.4

**Amends:** ADR-0045 (d9 and d11: the user page's Memories and Review views become one page; d10:
the admin page's Memories and Review views become one page with no Judge), ADR-0070 (d5: the
cockpit's calls)

## Context

On 2026-11-14 the operator asked to "make memories cockpit out of memories tab" (#374): one tab
replacing the separate Review and Memories pages, with one card, one workarea picker and one
search over three views, Approved (the default), To review (others' held memories the person may
judge, with Judge and a count), and Submitted (the person's own held and rejected writes); held
rows never in Approved; the admin side the same with no Judge, since an admin key never judges (R1).

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

- **d2 — To review is `<user>-review`'s held set, with Judge, and its count from a new count
  read.** The list is `GET /v2/review/held` under capacity `review` at the picked workarea, oldest
  first, each card with Judge, which opens the served ceremony for that item (ADR-0045 d11; the
  "Open to judge" of 2026-11-13); bulk review stays ADR-0055's act. Its count needs an API change:
  `GET /v2/review/held?count=true` answers `{held, truncated}`, the number of rows the list would
  serve at that workarea counted up to 1,000 (chosen, not measured) and `truncated` past it, with
  no content and no claim. Under ADR-0082 d7 (a), rows the person wrote in the console are not
  theirs to judge and are neither listed nor counted here. A person with no `<user>-review` grant
  beyond their root sees their agents' held writes only.

- **d3 — Submitted is every row any of the person's keys wrote that is held or rejected, and
  nothing else; it needs a `mine` filter.** The API change: `GET /v1/memories` gains `mine=true`,
  admitted for a person's session web key: the rows of every key naming its person (the own part of
  `with_own`, `pages.rs:908-910`) with no scoped part, under the same cursor; and `state` beside it
  takes a comma list, here `held,rejected`. `author` keeps meaning one key and narrows `mine`. Each
  card shows its state, its kind (a skill, a guardrail or a document from the builder,
  ADR-0082), the version it replaces if any, and the held reason the API already serves, scan
  findings and `needs_person` among them (ADR-0074 d17). A rejected row stays listed forever (R4);
  why it was rejected is not served to its author today, and this record adds nothing for it
  (d7 question 3). R3 holds: every row in Submitted is the caller's person's own.

- **d4 — One picker, one search, one card across the three views.** The picker's workarea and
  the search query carry across the views. Search runs recall in Approved; in To review and
  Submitted, which recall does not serve, it filters by `claim_key_prefix` (To review, an existing
  parameter of `GET /v2/review/held`) or is not offered (Submitted), since a text search over held
  rows is a read fenced to their author that recall's shared index must never serve (ADR-0025 d5).
  Each view draws the same card (`card.rs`) in its row density.

- **d5 — The admin side has the same frame and no Judge; which views it shows waits for the
  mockups.** Every admin view reads through `<user>-admin` and the journalled admin read
  (ADR-0045 d21, d24), and no admin view has Judge, a verdict or a write (R1). The options:
    - **(a) Approved, Held, Rejected.** `POST /v2/admin/memories/search` with `state` `promoted`,
      `held` or `rejected`; Held carries the queue's depth from `GET /v2/review/queue` as its
      count, and each held card names the workarea's reviewers' gap, not a Judge.
    - **(b) Approved and Held.** As (a) without Rejected, which the admin search's state filter
      still reaches.
    - **(c) Approved, Held, and By author.** (b) plus a view filtered by the `author` the admin
      search already takes.

    Recommended: (a), since it mirrors the user side's three tabs while each says what an admin
    reads, and needs no API change.

- **d6 — Routes and the tab bar wait for the mockups; the old paths keep answering.** The
  options for the user side:
    - **(a) One path, a view parameter.** `GET /web/memories?view=approved|review|submitted`,
      Approved when absent; `/web/review` answers `308` to `/web/memories?view=review` with its
      query kept, so links and bookmarks still land.
    - **(b) Three paths.** `/web/memories`, `/web/memories/review`, `/web/memories/submitted`;
      `/web/review` answers `308` to the second.
    - **(c) Keep `/web/review` as To review's path** and add `/web/memories/submitted`, the nav
      showing one Memories item whose tabs link the three.

    The admin side follows the same choice with `/web/admin/memories` and `/web/admin/review`.
    The count's placement is one of: on the To review tab only; on the tab and on the nav's
    Memories item; on neither until the view is opened. Recommended: (a) with the count on the tab
    only, since the count read (d2) then runs only on Memories pages, one store call each, which
    `web_console_reads.rs` records with the reason. Every choice keeps the views server-rendered,
    paged by cursor, with no script (ADR-0045 d22, d26), and the neutral voice.

- **d7 — The operator's choices this record needs before acceptance.**
    1. *The screen choices of d5 (admin views) and d6 (routes, tabs, count placement)*, to be
       settled on the approved mockups.
    2. *Does To review count (d2) need the new read, or is "200+" from the first page of the
       list enough?* (a) the count read; (b) the first page, counting up to 200 and saying "200+"
       past it, no API change but the whole page's text read on every Memories load. Recommended:
       (a).
    3. *Is a rejection's reason served to its author in Submitted?* (a) not in v0.7.4, as
       written; (b) yes, the verdict's reason text with no reviewer named (ADR-0077 d2), an API
       change on the claim item. Recommended: (a) now and an issue for (b).

## Consequences

- API changes: `mine=true` and a comma-list `state` on `GET /v1/memories` (d3);
  `count=true` on `GET /v2/review/held` (d2); `openapi.yaml`, `http-api.md` and the Rust SDK's
  operation table (ADR-0070 d9) follow in the serving change.
- `oml-web`'s `memories.rs` and `review.rs` merge into one page module; `PAGES` and the route
  test follow d6's choice; the redirects keep every existing link working.
- Page tests: a held row present at the workarea never appears under Approved, inside and
  outside an in-progress workarea; Submitted shows the person's held and rejected rows across two
  of their keys and nothing scoped; To review's count equals its list's length below the cap; the
  admin views draw no Judge and no verdict form.
- Costs, stated: one more store call per Memories page for the count; three views where there
  were two pages, so a person who only reviews now starts one click from the queue unless d6
  makes To review the landing view for them.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Approved as the merged listing with no state filter, held rows badged** | The operator asked that held rows never appear in Approved; a badge still shows them. |
| **Submitted from `with_own=true&state=held` and a second call for `rejected`** | The merged listing adds the scoped part, which inside an in-progress workarea holds other authors' held rows (ADR-0030 d3), so it is not "the person's own"; two calls also break one cursor. |
| **Submitted from `author=<key>` per key** | One call per key the person holds, merged in `oml-web`, with no shared cursor. |
| **Count from `GET /v2/workareas/{w}/tree?counts=true`** | Its `held` counts the person's own held rows, "as the caller's with_own listing would show them" (http-api.md, tree), not the rows they may judge. |
| **An admin Judge** | An admin key never reviews (R1; ADR-0045 d6). |
| **Search over held rows in To review and Submitted by recall** | Recall's shared index never serves another author's held text (ADR-0025 d5); a held row is matched only through a read fenced to its author. |
