<!--
id: adr/ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards
tier: engineering
status: proposed
answers: which acts an admin key may take on a production workarea's content; why each takes effect at once; what makes it break-glass (reason, mark, journal, confirmation); who confirms or reverts it afterwards and how a revert keeps R4; what readers see on a version served because of an admin fix; what an admin may read and on which surfaces; which constitutional rules this relaxes and by whose decision; the doctor lines and metrics for unconfirmed fixes; and the questions left for the operator
-->

# ADR-0089 — An admin fixes a production workarea at once by a marked break-glass act that a person confirms or reverts afterwards

**Status:** proposed · 2026-11-14

**Release:** not scheduled; with ADR-0085, whose releases it rolls back

**Amends:**
- ADR-0001 (d5: a third bounded exception, worded in d7 and accepted together with this record;
  d1, d2, d3 by reference to it: an admin's break-glass fix, served before review, is a bounded exception
  to R1, R2 and R3, named here by the operator's decision);
- ADR-0010 (d6: an admin key takes the fix acts of d1, which are not verdicts);
- ADR-0026 (d15: an admin key reads claim text in its tenant with a reason, under d6);
- ADR-0045 (d21: `memory.admin_read` gains a required reason);
- ADR-0077 (d2: `approver` gains `act` and `confirmed`; d5: the read-path law admits a marked
  admin fix);
- ADR-0009 (d3 and d5: a ladder move or a demotion by an admin is a fix act under d2; d5 also
  refuses an admin edit or retirement in a frozen workarea, d9);
- ADR-0025 (d11: an admin's demote is no longer irreversible: a person may revert it, d3);
- ADR-0011 (d3: outside a simulation, an admin's marked fix now changes served content without
  prior review; inside a simulation nothing changes, and no fix act reaches a simulation row);
- ADR-0044 (d7: an admin may edit and retire in `universe`, marked, d10).

## Context

On 2026-11-14 the operator said: "let's make focus on developers scenarios and admin should be the
person who can fix workareas on prod, so, all types of action, demote, edit, rollback imports for
example". Asked how, the operator answered that an admin's fix takes effect at once and is marked,
and that admins may read claim text in the workareas they administer, with every read journalled.
An admin key administers its whole tenant, and ADR-0045 d21 already serves it every memory of the
tenant on two routes; d5 keeps that reach and adds a required reason.

The developer path to fixing a bad publish exists: withdraw the release and fix forward
([ADR-0085](ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md)
d9, d11). It is too slow in two cases. The fix needs another person's review (R2), and a version
served to keys bound `tip`, or a version outside any release, has no release to withdraw. Today
nothing can take a served version out of service at once:

- Demote is designed (ADR-0009 d5, ADR-0010 d6) and not built: no transition reaches `Demoted`
  (`crates/oml-domain/src/model.rs:139`).
- An admin key cannot hold dreamer grants (`crates/oml-domain/src/access.rs:131`), so it cannot
  give a verdict.
- An admin key already reads every memory of its tenant on two routes, each read journalled as
  `memory.admin_read` ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d21; `crates/oml-app/src/app/pages.rs:1081`).

This record gives the admin the fix and keeps a person in the loop afterwards. It relaxes the
constitution. It does not hide that it does. Decided, not built.

## Decision

- **d1 — Seven fix acts, on the admin key role alone.** No agent key, service key or operator key
  takes them, on any transport.

  | Act | Effect |
  |---|---|
  | **demote** | A version stops being served. The claim's tip becomes its previous promoted version if there is one; otherwise the claim is served by no read. The demoted version's bytes are kept (ADR-0009 d5). |
  | **edit** | A supersession written by the admin key becomes the tip at once. |
  | **retire** | A retirement by the admin key takes effect at once. |
  | **roll back a release** | Withdraw it (ADR-0085 d9), or re-point a workarea's `latest` to an earlier published release until a person publishes again. |
  | **roll back an import** | For one `workarea.imported` event ([ADR-0084](ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md)), demote every version the import brought in that has since been promoted, which restores the tips that version replaced. Retire every claim key the import created. The import's held versions are marked `import_rolled_back` in the review list. |
  | **ladder move** | Propose or ratify a status move (ADR-0009 d3, unchanged in who may act). |
  | **erase** | Crypto-shred a version as a legal act ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d2). |

- **d2 — Every fix is break-glass: served at once, with a required reason, journalled, marked, and
  queued for a person.**
  - Each act takes effect in its own transaction. It is journalled as `admin.fix` with the act, the
    target ids, the admin key, and a reason that is required and scanned like any free text
    (ADR-0074).
  - In the same transaction it opens a confirmation item in the review queue of the persons who
    hold dreamer on the target's workarea.
  - A fix never waits for review. This is the operator's decision: a fix held for review is not a
    fix.

- **d3 — A person confirms or reverts the fix; a revert is a new version, and nothing is deleted
  but a legal erasure.**
  - The confirmer or reverter is a person's dreamer key (`held_by: human`) with dreamer on the
    workarea. Its principal must differ from the admin key's principal, so one person holding both
    keys cannot confirm their own fix (R2, per principal).
  - Confirming journals `admin.fix_confirmed` and clears the mark's `confirmed: false`.
  - Reverting journals `admin.fix_reverted` and writes the inverse as new state:
    - a demoted version comes back as a new version carrying its text;
    - an edit is superseded by a new version carrying the previous text;
    - a retirement is followed by a new version carrying the retired text;
    - a rolled-back release is re-published by re-pointing `latest`;
    - a rolled-back import is reversed version by version in the same way.
  - Every restored version is promoted by the reverter's verdict on text an earlier verdict already
    approved. R2 is checked against the restored version's lineage as for any verdict.
  - An erasure cannot be reverted; it is confirmed, or its reason is disputed in a comment that the
    journal keeps.
  - Every row an admin fix touched stays, in line with R4.

- **d4 — Readers see the mark on what an admin fix serves.**
  - ADR-0077 d2's `approver` gains two members:
    - `act`, which is `verdict` for every version promoted by review, or `admin_fix`;
    - `confirmed`, `false` until a person confirms and `null` when `act` is `verdict`.
  - A version an admin edit or retirement serves, or one a demotion or rollback made the tip again,
    carries `act: "admin_fix"`.
  - `held_by` is `human` or `service`, as the admin key attests; `same_user` is `false`.
  - As in ADR-0077 d2, no reader learns which admin.
  - A release whose latest was re-pointed by an admin reads `latest_by: "admin_fix"` in its status.
  - Agents see the mark on every surface, MCP included. The cost: an agent can tell its tenant has
    fixed something by hand.

- **d5 — An admin reads claim text tenant-wide, as ADR-0045 d21 already serves it, each read
  journalled with a reason.**
  - The reach is ADR-0045 d21's, already built: every memory of the admin key's tenant, every
    state, every workarea (`crates/oml-app/src/app/pages.rs:1081`). The admin console's views of a
    release's members, a fix's target and an import's versions read through the same routes.
  - Each read requires a reason and writes `memory.admin_read` with the ids served and a digest of
    the reason.
  - ADR-0026 d15's "never claim content" is narrowed to "never without a reason, and never on an
    agent surface".

- **d6 — Admin keys stay off the agent surfaces.**
  - Refused to an admin key with `access.verb_ceiling`, as today (`crates/oml-domain/src/access.rs`):
    - MCP: `oml_read`, `oml_write`, `oml_workarea`, prompts and resources;
    - `/v1` recall, listing and get;
    - the session-start and guardrail hooks.
  - An admin reads on the admin routes and fixes on the admin fix routes, never as an agent does.
  - The reasons:
    - An agent surface puts text into a model's context, where a reason cannot be asked for and a
      read cannot be judged.
    - Recall ranks and logs usage, which an admin's investigation must not move (ADR-0054).
    - Keeping the surfaces apart keeps "the gate is not callable by the thing being gated" true for
      every agent key.

- **d7 — This relaxes R1, R2 and R3 for admin keys, by the operator's decision, and nowhere else.**
  - **R1:** an admin key now demotes, and writes served content.
  - **R2:** an admin edit is served before any identity other than its author has reviewed it, and
    is reviewed only afterwards (d3).
  - **R3:** an admin reads held rows (d5).
  - The relaxation is bounded:
    - it holds only for the acts of d1 and the reads of d5;
    - each fix is marked to readers (d4) and journalled with a reason;
    - each fix is queued for a person who is not the admin.
  - **The proposed amendment of ADR-0001 d5**, to be accepted together with this record, appends:
    *"(Amended by ADR-0089, a third bounded exception, of R1, R2's order and R3, for admin keys
    only: an admin key's fix acts (demote, edit, retire, release and import rollback, ladder moves,
    erasure) take effect before review, each with a required reason, journalled, marked to every
    reader as `approver.act: "admin_fix"`, and queued for a person of another principal to confirm
    or revert; an admin key reads claim text tenant-wide with a reason on the admin routes alone.
    No agent key gains anything, and no other page widens it.)"*

- **d9 — In a frozen workarea an admin demotes and erases, and never edits or retires.** A
  `validated` workarea admits demotion, erasure and crew metadata only (ADR-0009 d5). An admin
  demote or erase there is a fix like any other; an admin edit or retirement is refused
  `workarea.frozen`, because either would change frozen content rather than withdraw it
  (*recommended by the alignment audit; operator to confirm*).

- **d10 — In `universe` an admin may edit and retire, marked.** `universe` serves curated claims
  without their source (ADR-0044 d9) and takes writes only from curators (ADR-0044 d7). An admin
  fix there is allowed and carries the d4 mark even though ADR-0077 d4 serves `universe` rows with
  no approver: a `universe` row served because of an admin fix carries `approver.act:
  "admin_fix"` and nothing else (*recommended by the alignment audit; operator to confirm*).

- **d11 — ADR-0077 d5's read-path law gains the fix.** The law becomes: every served hit is
  served unreviewed, or its approving verdict is by another key and another principal than its
  author's, or it carries `approver.act: "admin_fix"` with an `admin.fix` event in the journal
  naming it. A hit marked `admin_fix` and `confirmed: true` also has an `admin.fix_confirmed`
  by a principal other than the admin's.

- **d8 — Surfaces, doctor lines and metrics.**
  - **HTTP:** `POST /v2/admin/fixes {act, target, reason}`, `GET /v2/admin/fixes?state=unconfirmed`,
    and `POST /v2/admin/fixes/{id}/confirm|revert`. The confirm and revert routes take a person's
    dreamer key.
  - **CLI:** `oml admin fix demote|edit|retire|release-rollback|import-rollback --reason`, and
    `oml review fixes`.
  - **Console:** fix actions on the administration side, and a "Fixes to confirm" list on the
    personal Review page.
  - **Review-side read:** `GET /v2/review/fixes?count=true`, for a person's dreamer key (the
    `<user>-review` key the Personal side reads with, ADR-0083 d8). It serves the unconfirmed admin
    fixes on workareas where that key holds dreamer, and their count. Each entry carries the act,
    the target version, the version it replaced, the reason and the time. It never names the admin
    key, its principal or its user, as ADR-0077 d2 never names a reviewer. A fix taken by the
    reader's own principal is listed without confirm or revert, since d3 refuses that principal.
    Any other key is refused `access.verb_ceiling`.
  - **Doctor lines:** unconfirmed fixes, with the oldest one's age; fixes whose workarea has no
    person holding dreamer, so nobody can confirm them; fixes visible on the review-side read only
    to the admin's own principal, so no one who sees them may confirm them; and reverted fixes.
  - **Metrics:** `oml_admin_fixes_total{act}`, `oml_admin_fixes_unconfirmed` and
    `oml_admin_fix_oldest_unconfirmed_seconds`, on the metrics port (ADR-0042).

## Consequences

- Demotion is built as part of this record, as an admin act and as the confirmation's inverse; the
  transitions in `crates/oml-domain/src/model.rs:139` gain `Demoted`.
- ADR-0085's admin release view lists members and claim keys and offers the release rollback.
  ADR-0084 records every version an import wrote, so that its rollback can find them.
- ADR-0077's contract change is additive: two members.
- A tenant whose only person is the admin cannot confirm its own fixes. They stay unconfirmed and
  the doctor names them.
- Questions for the operator, recommendation first:
  1. Should an admin edit be allowed in every workarea (recommended for production), or only in a
     workarea with published releases?
  2. Should an unconfirmed fix expire? The recommendation is no: it stays served and marked, and the
     doctor names it.
  3. May a person who holds an admin key confirm a fix that another admin took? Yes (recommended),
     since the principal differs.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Erasure only, as the legal act** | Leaves production with no fast lever between "served" and "shredded". A harmful but lawful version stays served until a second person reviews a fix, which the operator ruled out. |
| **Fixes held for review** | A fix that waits for a reviewer is the developer path (ADR-0085 d9, d11) and is too slow in exactly the cases a fix is for. |
| **Unmarked fixes** | Readers, agents included, would take a version no review approved for a reviewed one, and an audit could not separate the two without the journal. |
| **A fix by a person's dreamer key instead of the admin** | A dreamer key gives verdicts, which R2 requires to be another identity's. A served edit by a dreamer would erase the line between review and fix without the admin's journal and mark. |
| **Admin reads on the agent surfaces** | Puts admin-visible text, held rows included, into a model's context with no reason asked, and moves recall telemetry. |
| **Confirmation by any key** | An agent confirming an admin's fix is the gate called by the thing gated (R1). |
